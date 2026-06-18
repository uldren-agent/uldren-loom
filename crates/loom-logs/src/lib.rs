//! Canonical native logs contracts.

use loom_codec::Value;
use loom_types::{Digest, LoomError, Result};
use std::collections::BTreeMap;

pub const LOG_RECORD_SCHEMA: &str = "loom.logs.record.v1";
pub const MAX_LOG_ATTRIBUTES: usize = 128;
pub const MAX_LOG_KEY_BYTES: usize = 128;
pub const MAX_LOG_TEXT_BYTES: usize = 4096;
pub const MAX_LOG_BYTES: usize = 16_384;
pub const MAX_LOG_VALUE_DEPTH: usize = 16;

#[derive(Debug, Clone, PartialEq)]
pub enum LogValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<LogValue>),
    Map(BTreeMap<String, LogValue>),
}

impl LogValue {
    pub fn validate(&self) -> Result<()> {
        self.validate_depth(0)
    }

    fn validate_depth(&self, depth: usize) -> Result<()> {
        if depth > MAX_LOG_VALUE_DEPTH {
            return Err(LoomError::invalid("log value nesting is too deep"));
        }
        match self {
            Self::Null | Self::Bool(_) | Self::Int(_) => Ok(()),
            Self::Float(value) if value.is_finite() => Ok(()),
            Self::Float(_) => Err(LoomError::invalid("log float value is invalid")),
            Self::String(value) if value.len() <= MAX_LOG_TEXT_BYTES => Ok(()),
            Self::String(_) => Err(LoomError::invalid("log text value is too large")),
            Self::Bytes(value) if value.len() <= MAX_LOG_BYTES => Ok(()),
            Self::Bytes(_) => Err(LoomError::invalid("log bytes value is too large")),
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
            return Err(LoomError::corrupt("log value is invalid"));
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
            "map" => {
                let mut values = BTreeMap::new();
                for (key, value) in map(fields[1].clone())? {
                    let key = text(&key)?.to_owned();
                    if values.insert(key, Self::decode_value(value)?).is_some() {
                        return Err(LoomError::corrupt("duplicate log value key"));
                    }
                }
                Self::Map(values)
            }
            _ => return Err(LoomError::corrupt("log value kind is invalid")),
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogSeverityNumber(u8);

impl LogSeverityNumber {
    pub fn new(value: u8) -> Result<Self> {
        if !(1..=24).contains(&value) {
            return Err(LoomError::invalid("log severity number is invalid"));
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogTraceContext {
    pub trace_id: [u8; 16],
    pub span_id: [u8; 8],
    pub trace_flags: u8,
}

impl LogTraceContext {
    pub fn new(trace_id: [u8; 16], span_id: [u8; 8], trace_flags: u8) -> Result<Self> {
        if trace_id == [0; 16] || span_id == [0; 8] || trace_flags > 1 {
            return Err(LoomError::invalid("log trace context is invalid"));
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
            return Err(LoomError::corrupt("log trace context is invalid"));
        }
        let trace_id = fixed_bytes::<16>(&fields[0], "log trace id is invalid")?;
        let span_id = fixed_bytes::<8>(&fields[1], "log span id is invalid")?;
        let trace_flags = u64_value(&fields[2])?
            .try_into()
            .map_err(|_| LoomError::corrupt("log trace flags are invalid"))?;
        Self::new(trace_id, span_id, trace_flags)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogRecord {
    pub timestamp_ns: u64,
    pub observed_timestamp_ns: Option<u64>,
    pub severity_number: LogSeverityNumber,
    pub severity_text: String,
    pub body: LogValue,
    pub attributes: BTreeMap<String, LogValue>,
    pub resource: BTreeMap<String, LogValue>,
    pub scope: BTreeMap<String, LogValue>,
    pub trace_context: Option<LogTraceContext>,
}

impl LogRecord {
    pub fn new(
        timestamp_ns: u64,
        observed_timestamp_ns: Option<u64>,
        severity_number: LogSeverityNumber,
        severity_text: String,
        body: LogValue,
    ) -> Result<Self> {
        let record = Self {
            timestamp_ns,
            observed_timestamp_ns,
            severity_number,
            severity_text,
            body,
            attributes: BTreeMap::new(),
            resource: BTreeMap::new(),
            scope: BTreeMap::new(),
            trace_context: None,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn with_context(
        mut self,
        attributes: BTreeMap<String, LogValue>,
        resource: BTreeMap<String, LogValue>,
        scope: BTreeMap<String, LogValue>,
        trace_context: Option<LogTraceContext>,
    ) -> Result<Self> {
        self.attributes = attributes;
        self.resource = resource;
        self.scope = scope;
        self.trace_context = trace_context;
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<()> {
        if self.timestamp_ns == 0
            || self.severity_text.is_empty()
            || self.severity_text.len() > MAX_LOG_KEY_BYTES
        {
            return Err(LoomError::invalid("log record is invalid"));
        }
        if let Some(observed_timestamp_ns) = self.observed_timestamp_ns
            && (observed_timestamp_ns == 0 || observed_timestamp_ns < self.timestamp_ns)
        {
            return Err(LoomError::invalid("log observed timestamp is invalid"));
        }
        self.severity_number.validate()?;
        self.body.validate()?;
        validate_value_map(&self.attributes, 0)?;
        validate_value_map(&self.resource, 0)?;
        validate_value_map(&self.scope, 0)?;
        if let Some(trace_context) = &self.trace_context {
            LogTraceContext::new(
                trace_context.trace_id,
                trace_context.span_id,
                trace_context.trace_flags,
            )?;
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        loom_codec::encode(&Value::Array(vec![
            Value::Text(LOG_RECORD_SCHEMA.into()),
            Value::Uint(self.timestamp_ns),
            optional_u64(self.observed_timestamp_ns),
            Value::Uint(self.severity_number.get().into()),
            Value::Text(self.severity_text.clone()),
            self.body.encode_value(),
            encode_value_map(&self.attributes),
            encode_value_map(&self.resource),
            encode_value_map(&self.scope),
            optional_trace_context(&self.trace_context),
        ]))
        .map_err(|err| LoomError::invalid(format!("log record encoding failed: {err}")))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let fields = array(
            loom_codec::decode(bytes)
                .map_err(|err| LoomError::corrupt(format!("invalid log record CBOR: {err}")))?,
        )?;
        if fields.len() != 10 || text(&fields[0])? != LOG_RECORD_SCHEMA {
            return Err(LoomError::corrupt("invalid log record schema"));
        }
        let record = Self {
            timestamp_ns: u64_value(&fields[1])?,
            observed_timestamp_ns: optional_u64_value(&fields[2])?,
            severity_number: LogSeverityNumber::new(
                u64_value(&fields[3])?
                    .try_into()
                    .map_err(|_| LoomError::corrupt("log severity number is invalid"))?,
            )?,
            severity_text: text(&fields[4])?.to_owned(),
            body: LogValue::decode_value(fields[5].clone())?,
            attributes: decode_value_map(fields[6].clone())?,
            resource: decode_value_map(fields[7].clone())?,
            scope: decode_value_map(fields[8].clone())?,
            trace_context: optional_trace_context_value(&fields[9])?,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn record_id(&self) -> Result<String> {
        Ok(Digest::blake3(&self.encode()?).to_hex())
    }
}

impl LogSeverityNumber {
    fn validate(self) -> Result<()> {
        Self::new(self.0).map(|_| ())
    }
}

fn validate_value_map(values: &BTreeMap<String, LogValue>, depth: usize) -> Result<()> {
    if values.len() > MAX_LOG_ATTRIBUTES {
        return Err(LoomError::invalid("log attribute count is too large"));
    }
    for (key, value) in values {
        if key.is_empty() || key.len() > MAX_LOG_KEY_BYTES || key.chars().any(char::is_control) {
            return Err(LoomError::invalid("log attribute key is invalid"));
        }
        value.validate_depth(depth)?;
    }
    Ok(())
}

fn encode_value_map(values: &BTreeMap<String, LogValue>) -> Value {
    Value::Map(
        values
            .iter()
            .map(|(key, value)| (Value::Text(key.clone()), value.encode_value()))
            .collect(),
    )
}

fn decode_value_map(value: Value) -> Result<BTreeMap<String, LogValue>> {
    let mut values = BTreeMap::new();
    for (key, value) in map(value)? {
        let key = text(&key)?.to_owned();
        if values.insert(key, LogValue::decode_value(value)?).is_some() {
            return Err(LoomError::corrupt("duplicate log attribute key"));
        }
    }
    validate_value_map(&values, 0)?;
    Ok(values)
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

fn optional_trace_context(value: &Option<LogTraceContext>) -> Value {
    value
        .as_ref()
        .map_or(Value::Null, LogTraceContext::encode_value)
}

fn optional_trace_context_value(value: &Value) -> Result<Option<LogTraceContext>> {
    if *value == Value::Null {
        Ok(None)
    } else {
        LogTraceContext::decode_value(value.clone()).map(Some)
    }
}

fn fixed_bytes<const N: usize>(value: &Value, message: &'static str) -> Result<[u8; N]> {
    bytes(value)?
        .try_into()
        .map_err(|_| LoomError::corrupt(message))
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

    fn sample_record() -> LogRecord {
        LogRecord::new(
            1_725_000_000_000_000_000,
            Some(1_725_000_000_010_000_000),
            LogSeverityNumber::new(13).unwrap(),
            "WARN".into(),
            LogValue::String("cache miss".into()),
        )
        .unwrap()
        .with_context(
            BTreeMap::from([
                ("cache.hit".into(), LogValue::Bool(false)),
                ("latency.ms".into(), LogValue::Float(12.5)),
            ]),
            BTreeMap::from([("service.name".into(), LogValue::String("api".into()))]),
            BTreeMap::from([
                ("name".into(), LogValue::String("loom".into())),
                ("version".into(), LogValue::String("0.1.0".into())),
            ]),
            Some(
                LogTraceContext::new(
                    [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                    [17, 18, 19, 20, 21, 22, 23, 24],
                    1,
                )
                .unwrap(),
            ),
        )
        .unwrap()
    }

    #[test]
    fn record_round_trips() {
        let record = sample_record();
        assert_eq!(
            LogRecord::decode(&record.encode().unwrap()).unwrap(),
            record
        );
    }

    #[test]
    fn rejects_invalid_trace_context() {
        assert!(LogTraceContext::new([0; 16], [1; 8], 1).is_err());
        assert!(LogTraceContext::new([1; 16], [1; 8], 2).is_err());
    }
}
