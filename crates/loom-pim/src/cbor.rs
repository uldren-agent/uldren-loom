use loom_types::error::{LoomError, Result};

pub use loom_codec::Value;

pub fn encode(value: &Value) -> Vec<u8> {
    loom_codec::encode(value).expect("PIM CBOR value is encodable")
}

pub fn decode(bytes: &[u8]) -> Result<Value> {
    loom_codec::decode(bytes).map_err(|e| LoomError::corrupt(format!("cbor: {e}")))
}

pub fn as_uint(value: Value) -> Result<u64> {
    match value {
        Value::Uint(value) => Ok(value),
        _ => Err(LoomError::corrupt("expected a uint")),
    }
}

pub fn as_text(value: Value) -> Result<String> {
    match value {
        Value::Text(value) => Ok(value),
        _ => Err(LoomError::corrupt("expected a text string")),
    }
}

pub fn as_array(value: Value) -> Result<Vec<Value>> {
    match value {
        Value::Array(value) => Ok(value),
        _ => Err(LoomError::corrupt("expected an array")),
    }
}

pub fn as_map(value: Value) -> Result<Vec<(Value, Value)>> {
    match value {
        Value::Map(value) => Ok(value),
        _ => Err(LoomError::corrupt("expected a map")),
    }
}

pub struct Fields {
    items: std::vec::IntoIter<Value>,
}

impl Fields {
    pub fn new(items: Vec<Value>) -> Self {
        Self {
            items: items.into_iter(),
        }
    }

    pub fn text(&mut self) -> Result<String> {
        as_text(self.next_field()?)
    }

    pub fn next_field(&mut self) -> Result<Value> {
        self.items
            .next()
            .ok_or_else(|| LoomError::corrupt("missing field"))
    }

    pub fn end(mut self) -> Result<()> {
        if self.items.next().is_some() {
            return Err(LoomError::corrupt("trailing field"));
        }
        Ok(())
    }
}
