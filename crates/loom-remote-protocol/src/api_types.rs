//! Support types for the generated `LoomApi` surface.
//!
//! These are the wire-level types the generated trait signatures use: opaque handle ids, a UUID, a
//! content address, and the stream alias. Structured IDL records cross as canonical CBOR `Vec<u8>` at
//! this layer. They live in this engine-free crate so both `LocalLoomClient` (engine) and
//! `RemoteLoomClient` (engine-free) implement the same generated traits over the same types.
//!
//! Licensed under BUSL-1.1.

use crate::codec::{ArgError, FromValue, RemoteHandleId, ToValue};
use loom_codec::Value;
use loom_types::LoomError;

/// An asynchronous sequence of `Result<T, LoomError>` items.
pub type LoomStream<T> =
    core::pin::Pin<Box<dyn futures_core::Stream<Item = Result<T, LoomError>> + Send>>;

/// Public placement value for inserting a ticket into a Lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneTicketPlacement {
    First,
    Last,
    Before,
    After,
}

impl LaneTicketPlacement {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::First => "FIRST",
            Self::Last => "LAST",
            Self::Before => "BEFORE",
            Self::After => "AFTER",
        }
    }
}

impl ToValue for LaneTicketPlacement {
    fn to_value(&self) -> Value {
        Value::Text(self.as_str().to_string())
    }
}

impl FromValue for LaneTicketPlacement {
    fn from_value(value: &Value) -> Result<Self, ArgError> {
        match value {
            Value::Text(text) => match text.as_str() {
                "FIRST" => Ok(Self::First),
                "LAST" => Ok(Self::Last),
                "BEFORE" => Ok(Self::Before),
                "AFTER" => Ok(Self::After),
                _ => Err(ArgError::TypeMismatch {
                    expected: "LaneTicketPlacement",
                }),
            },
            _ => Err(ArgError::TypeMismatch {
                expected: "LaneTicketPlacement",
            }),
        }
    }
}

/// The canonical opaque handle id shared by the whole surface. This is the wire
/// handle [`RemoteHandleId`]: a kind label, the server-minted id bytes, a generation counter, and the
/// owning session id. Every typed handle newtype below wraps one, so a handle a client receives from the
/// server is echoed back verbatim (lossless) rather than reconstructed.
pub type HandleId = RemoteHandleId;

/// A UUID as 16 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Uuid(pub [u8; 16]);

impl ToValue for Uuid {
    fn to_value(&self) -> Value {
        Value::Bytes(self.0.to_vec())
    }
}

impl FromValue for Uuid {
    fn from_value(value: &Value) -> Result<Self, ArgError> {
        match value {
            Value::Bytes(bytes) => <[u8; 16]>::try_from(bytes.as_slice())
                .map(Uuid)
                .map_err(|_| ArgError::TypeMismatch { expected: "Uuid" }),
            _ => Err(ArgError::TypeMismatch { expected: "Uuid" }),
        }
    }
}

/// A content address in `algo:hex` form (the IDL `Digest`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digest(pub String);

impl ToValue for Digest {
    fn to_value(&self) -> Value {
        Value::Text(self.0.clone())
    }
}

impl FromValue for Digest {
    fn from_value(value: &Value) -> Result<Self, ArgError> {
        match value {
            Value::Text(text) => Ok(Digest(text.clone())),
            _ => Err(ArgError::TypeMismatch { expected: "Digest" }),
        }
    }
}

/// Define a typed handle newtype over [`HandleId`] with its wire conversions. Every handle crosses as the
/// same `RemoteHandleId` array, so client, server dispatch, and the local client share one encoding.
macro_rules! handle_newtype {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name(pub HandleId);

        impl ToValue for $name {
            fn to_value(&self) -> Value {
                self.0.to_value()
            }
        }

        impl FromValue for $name {
            fn from_value(value: &Value) -> Result<Self, ArgError> {
                HandleId::from_value(value).map($name)
            }
        }
    };
}

handle_newtype! {
    /// An open store session handle.
    LoomSession
}
handle_newtype! {
    /// An open SQL session handle.
    SqlSession
}
handle_newtype! {
    /// An open SQL transaction batch handle.
    SqlBatch
}
handle_newtype! {
    /// A forward-only row iterator handle.
    RowIter
}
handle_newtype! {
    /// An asynchronous task handle.
    Task
}
handle_newtype! {
    /// A decoded result view handle (client-local).
    ResultView
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{decode_args, encode_args};

    #[test]
    fn handles_wrap_ids() {
        let handle = LoomSession(HandleId {
            kind: "session".to_string(),
            id: vec![1, 2, 3],
            generation: 7,
            owner_session: vec![9],
        });
        assert_eq!(handle.0.generation, 7);
        assert_eq!(Uuid([0; 16]).0.len(), 16);
        assert_eq!(Digest("blake3:ab".to_string()).0, "blake3:ab");
    }

    #[test]
    fn handle_and_scalar_conversions_round_trip() {
        let handle = SqlSession(HandleId {
            kind: "sql_session".to_string(),
            id: vec![7, 7],
            generation: 2,
            owner_session: vec![1],
        });
        let bytes = encode_args(&[
            handle.to_value(),
            Digest("blake3:af".to_string()).to_value(),
            Uuid([3; 16]).to_value(),
        ])
        .unwrap();
        let decoded = decode_args(&bytes).unwrap();
        assert_eq!(SqlSession::from_value(&decoded[0]).unwrap(), handle);
        assert_eq!(
            Digest::from_value(&decoded[1]).unwrap(),
            Digest("blake3:af".to_string())
        );
        assert_eq!(Uuid::from_value(&decoded[2]).unwrap(), Uuid([3; 16]));
    }

    #[test]
    fn lane_ticket_placement_round_trips() {
        let value = LaneTicketPlacement::Before.to_value();
        assert_eq!(
            LaneTicketPlacement::from_value(&value).unwrap(),
            LaneTicketPlacement::Before
        );
        assert!(LaneTicketPlacement::from_value(&Value::Text("APPEND".to_string())).is_err());
    }

    #[test]
    fn ticket_project_settings_generated_shape_exposes_full_contract() {
        let get = crate::generated::METHODS
            .iter()
            .find(|method| {
                method.interface == "Tickets"
                    && method.method == "tickets_project_settings_get_json"
            })
            .expect("generated get method exists");
        assert_eq!(
            get.args,
            &[
                ("LoomSession", "handle"),
                ("string", "workspace"),
                ("string", "ticket_workspace_id"),
                ("string", "project_id"),
                ("bool", "include_contracts"),
            ]
        );
        let set = crate::generated::METHODS
            .iter()
            .find(|method| {
                method.interface == "Tickets"
                    && method.method == "tickets_project_settings_set_json"
            })
            .expect("generated set method exists");
        assert_eq!(
            set.args,
            &[
                ("LoomSession", "handle"),
                ("string", "workspace"),
                ("string", "ticket_workspace_id"),
                ("string", "project_id"),
                ("TicketProjectSettingsPatch", "patch"),
            ]
        );
    }
}
