//! Index-scan lookup-key **input** parsing.
//!
//! The lookup prefix for `loom_sql_index_scan` arrives as **canonical CBOR**: a CBOR array of
//! faithful cells (the same tagged cell form result payloads use), decoded here into tabular values to
//! seek with. This is input only, and it shares the one type-faithful cell codec
//! ([`loom_core::tabular::cell_from`]) with the result path, so the whole ABI - argument and result -
//! speaks one canonical form (no JSON anywhere on the wire).

use loom_codec::Value as Cbor;
use loom_core::error::{LoomError, Result};
use loom_core::tabular::{Value, cell_from};

/// Parse a canonical-CBOR array of faithful cells into the tabular values of an index-scan lookup
/// prefix. Each element is one cell (e.g. `cell_value(&Value::Int(2))`); the array order is the
/// index's key order.
pub fn values_from_cbor(bytes: &[u8]) -> Result<Vec<Value>> {
    let decoded =
        loom_codec::decode(bytes).map_err(|e| LoomError::invalid(format!("lookup cbor: {e}")))?;
    let Cbor::Array(items) = decoded else {
        return Err(LoomError::invalid(
            "lookup prefix must be a CBOR array of cells",
        ));
    };
    items.into_iter().map(cell_from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::tabular::cell_value;

    #[test]
    fn parses_cbor_cell_array() {
        let bytes = loom_codec::encode(&Cbor::Array(vec![
            cell_value(&Value::Int(2)),
            cell_value(&Value::Text("x".into())),
        ]))
        .unwrap();
        let vals = values_from_cbor(&bytes).unwrap();
        assert_eq!(vals, vec![Value::Int(2), Value::Text("x".into())]);
    }

    #[test]
    fn rejects_non_array() {
        let bytes = loom_codec::encode(&cell_value(&Value::Int(1))).unwrap();
        assert!(values_from_cbor(&bytes).is_err());
    }
}
