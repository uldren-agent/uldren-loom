use crate::cbor::{self, Value};
use loom_types::error::{LoomError, Result};
use mail_parser::{Address, MessageParser};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MailMessage {
    pub uid: String,
    pub body: String,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub date: String,
    pub message_id: Option<String>,
    pub size: u64,
    pub headers: Vec<(String, String)>,
}

impl MailMessage {
    pub fn from_rfc5322(
        uid: impl Into<String>,
        body: impl Into<String>,
        raw: &[u8],
    ) -> Result<Self> {
        let parsed = MessageParser::default()
            .parse(raw)
            .ok_or_else(|| LoomError::invalid("mail: unparseable RFC 5322 message"))?;

        let raw_message = parsed.raw_message();
        let headers: Vec<(String, String)> = parsed
            .headers()
            .iter()
            .map(|header| {
                let value = raw_message
                    .get(header.offset_start as usize..header.offset_end as usize)
                    .map(|bytes| {
                        String::from_utf8_lossy(bytes)
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default();
                (header.name().to_string(), value)
            })
            .collect();

        Ok(MailMessage {
            uid: uid.into(),
            body: body.into(),
            from: parsed.from().map(address_first).unwrap_or_default(),
            to: parsed.to().map(address_all).unwrap_or_default(),
            subject: parsed.subject().unwrap_or("").to_string(),
            date: parsed
                .date()
                .map(|date| date.to_rfc3339())
                .unwrap_or_default(),
            message_id: parsed.message_id().map(str::to_string),
            size: raw.len() as u64,
            headers,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut map: Vec<(Value, Value)> = Vec::new();
        let put = |map: &mut Vec<(Value, Value)>, key: &str, value: Value| {
            map.push((Value::Text(key.into()), value));
        };
        put(&mut map, "uid", Value::Text(self.uid.clone()));
        put(&mut map, "body", Value::Text(self.body.clone()));
        put(&mut map, "from", Value::Text(self.from.clone()));
        if !self.to.is_empty() {
            put(
                &mut map,
                "to",
                Value::Array(self.to.iter().cloned().map(Value::Text).collect()),
            );
        }
        put(&mut map, "subject", Value::Text(self.subject.clone()));
        put(&mut map, "date", Value::Text(self.date.clone()));
        if let Some(value) = &self.message_id {
            put(&mut map, "message_id", Value::Text(value.clone()));
        }
        put(&mut map, "size", Value::Uint(self.size));
        if !self.headers.is_empty() {
            let items = self
                .headers
                .iter()
                .map(|(key, value)| {
                    Value::Array(vec![Value::Text(key.clone()), Value::Text(value.clone())])
                })
                .collect();
            put(&mut map, "headers", Value::Array(items));
        }
        cbor::encode(&Value::Map(map))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let pairs = cbor::as_map(cbor::decode(bytes)?)?;
        let get = |key: &str| {
            pairs
                .iter()
                .find(|(field, _)| matches!(field, Value::Text(text) if text == key))
                .map(|(_, value)| value.clone())
        };
        let text = |key: &str| -> Result<String> {
            cbor::as_text(
                get(key).ok_or_else(|| LoomError::corrupt(format!("mail: missing {key}")))?,
            )
        };
        let to = match get("to") {
            Some(value) => cbor::as_array(value)?
                .into_iter()
                .map(cbor::as_text)
                .collect::<Result<_>>()?,
            None => Vec::new(),
        };
        let message_id = get("message_id").map(cbor::as_text).transpose()?;
        let size = match get("size") {
            Some(value) => cbor::as_uint(value)?,
            None => 0,
        };
        let headers = match get("headers") {
            Some(value) => {
                let mut out = Vec::new();
                for item in cbor::as_array(value)? {
                    let mut fields = cbor::Fields::new(cbor::as_array(item)?);
                    let key = fields.text()?;
                    let value = fields.text()?;
                    fields.end()?;
                    out.push((key, value));
                }
                out
            }
            None => Vec::new(),
        };
        Ok(MailMessage {
            uid: text("uid")?,
            body: text("body")?,
            from: text("from")?,
            to,
            subject: text("subject")?,
            date: text("date")?,
            message_id,
            size,
            headers,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MailboxMeta {
    pub display_name: String,
}

impl MailboxMeta {
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Map(vec![(
            Value::Text("display_name".into()),
            Value::Text(self.display_name.clone()),
        )]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let pairs = cbor::as_map(cbor::decode(bytes)?)?;
        let display_name = pairs
            .into_iter()
            .find(|(key, _)| matches!(key, Value::Text(text) if text == "display_name"))
            .map(|(_, value)| cbor::as_text(value))
            .transpose()?
            .unwrap_or_default();
        Ok(MailboxMeta { display_name })
    }
}

fn address_first(addr: &Address) -> String {
    addr.first()
        .and_then(|address| address.address())
        .unwrap_or("")
        .to_string()
}

fn address_all(addr: &Address) -> Vec<String> {
    addr.iter()
        .filter_map(|address| address.address().map(str::to_string))
        .collect()
}
