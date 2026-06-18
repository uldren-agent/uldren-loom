//! Canonical wire codec for the document facet's composite document result records.

use loom_codec::{Value, decode, encode};
use loom_types::{Code, LoomError};

/// Encode the IDL `DocumentReplaceTextResult`.
pub fn replace_text_result_to_cbor(
    replacements: u64,
    digest: &str,
    entity_tag: &str,
) -> Result<Vec<u8>, LoomError> {
    encode(&Value::Array(vec![
        Value::Uint(replacements),
        Value::Text(digest.to_string()),
        Value::Text(entity_tag.to_string()),
    ]))
    .map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))
}

/// Decode a `DocumentReplaceTextResult` wire form into `(replacements, digest, entity_tag)`.
pub fn replace_text_result_from_cbor(bytes: &[u8]) -> Result<(u64, String, String), LoomError> {
    let value =
        decode(bytes).map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))?;
    let Value::Array(items) = value else {
        return Err(LoomError::invalid(
            "replace-text result must be a cbor array",
        ));
    };
    let [replacements, digest, entity_tag] = items.as_slice() else {
        return Err(LoomError::invalid(
            "replace-text result must be [replacements, digest, entity_tag]",
        ));
    };
    let &Value::Uint(replacements) = replacements else {
        return Err(LoomError::invalid(
            "replace-text replacements must be a uint",
        ));
    };
    let Value::Text(digest) = digest else {
        return Err(LoomError::invalid("replace-text digest must be text"));
    };
    let Value::Text(entity_tag) = entity_tag else {
        return Err(LoomError::invalid("replace-text entity tag must be text"));
    };
    Ok((replacements, digest.clone(), entity_tag.clone()))
}

/// Encode the IDL `DocumentPutResult`.
pub fn put_result_to_cbor(digest: &str, entity_tag: &str) -> Result<Vec<u8>, LoomError> {
    encode(&Value::Array(vec![
        Value::Text(digest.to_string()),
        Value::Text(entity_tag.to_string()),
    ]))
    .map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))
}

/// Decode a `DocumentPutResult` wire form into `(digest, entity_tag)`.
pub fn put_result_from_cbor(bytes: &[u8]) -> Result<(String, String), LoomError> {
    let value =
        decode(bytes).map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))?;
    let Value::Array(items) = value else {
        return Err(LoomError::invalid(
            "document put result must be a cbor array",
        ));
    };
    let [digest, entity_tag] = items.as_slice() else {
        return Err(LoomError::invalid(
            "document put result must be [digest, entity_tag]",
        ));
    };
    let Value::Text(digest) = digest else {
        return Err(LoomError::invalid("document put digest must be text"));
    };
    let Value::Text(entity_tag) = entity_tag else {
        return Err(LoomError::invalid("document put entity tag must be text"));
    };
    Ok((digest.clone(), entity_tag.clone()))
}

/// Encode the IDL `DocumentTextResult`.
pub fn text_result_to_cbor(
    text: &str,
    digest: &str,
    entity_tag: &str,
) -> Result<Vec<u8>, LoomError> {
    encode(&Value::Array(vec![
        Value::Text(text.to_string()),
        Value::Text(digest.to_string()),
        Value::Text(entity_tag.to_string()),
    ]))
    .map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))
}

/// Decode a `DocumentTextResult` wire form into `(text, digest, entity_tag)`.
pub fn text_result_from_cbor(bytes: &[u8]) -> Result<(String, String, String), LoomError> {
    let value =
        decode(bytes).map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))?;
    let Value::Array(items) = value else {
        return Err(LoomError::invalid(
            "document text result must be a cbor array",
        ));
    };
    let [text, digest, entity_tag] = items.as_slice() else {
        return Err(LoomError::invalid(
            "document text result must be [text, digest, entity_tag]",
        ));
    };
    let Value::Text(text) = text else {
        return Err(LoomError::invalid("document text must be text"));
    };
    let Value::Text(digest) = digest else {
        return Err(LoomError::invalid("document text digest must be text"));
    };
    let Value::Text(entity_tag) = entity_tag else {
        return Err(LoomError::invalid("document text entity tag must be text"));
    };
    Ok((text.clone(), digest.clone(), entity_tag.clone()))
}

/// Encode the IDL `DocumentBinaryResult`.
pub fn binary_result_to_cbor(
    bytes: &[u8],
    digest: &str,
    entity_tag: &str,
) -> Result<Vec<u8>, LoomError> {
    encode(&Value::Array(vec![
        Value::Bytes(bytes.to_vec()),
        Value::Text(digest.to_string()),
        Value::Text(entity_tag.to_string()),
    ]))
    .map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))
}

/// Decode a `DocumentBinaryResult` wire form into `(bytes, digest, entity_tag)`.
pub fn binary_result_from_cbor(input: &[u8]) -> Result<(Vec<u8>, String, String), LoomError> {
    let value =
        decode(input).map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))?;
    let Value::Array(items) = value else {
        return Err(LoomError::invalid(
            "document binary result must be a cbor array",
        ));
    };
    let [bytes, digest, entity_tag] = items.as_slice() else {
        return Err(LoomError::invalid(
            "document binary result must be [bytes, digest, entity_tag]",
        ));
    };
    let Value::Bytes(bytes) = bytes else {
        return Err(LoomError::invalid(
            "document binary bytes must be a byte string",
        ));
    };
    let Value::Text(digest) = digest else {
        return Err(LoomError::invalid("document binary digest must be text"));
    };
    let Value::Text(entity_tag) = entity_tag else {
        return Err(LoomError::invalid(
            "document binary entity tag must be text",
        ));
    };
    Ok((bytes.clone(), digest.clone(), entity_tag.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_text_result_round_trips() {
        let bytes = replace_text_result_to_cbor(3, "blake3:abc", "entity-tag:abc").unwrap();
        assert_eq!(
            replace_text_result_from_cbor(&bytes).unwrap(),
            (3, "blake3:abc".to_string(), "entity-tag:abc".to_string())
        );
    }

    #[test]
    fn text_result_round_trips() {
        let bytes = text_result_to_cbor("hello", "blake3:abc", "entity-tag:abc").unwrap();
        assert_eq!(
            text_result_from_cbor(&bytes).unwrap(),
            (
                "hello".to_string(),
                "blake3:abc".to_string(),
                "entity-tag:abc".to_string()
            )
        );
    }

    #[test]
    fn binary_result_round_trips() {
        let bytes = binary_result_to_cbor(&[1, 2, 3], "blake3:abc", "entity-tag:abc").unwrap();
        assert_eq!(
            binary_result_from_cbor(&bytes).unwrap(),
            (
                vec![1, 2, 3],
                "blake3:abc".to_string(),
                "entity-tag:abc".to_string()
            )
        );
    }
}
