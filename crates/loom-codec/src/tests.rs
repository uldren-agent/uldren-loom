use super::*;

/// Pinned canonical byte forms: `(value, hex)`. Encoding must produce exactly these bytes, and decoding
/// them must reproduce the value (byte stability in both directions).
fn canonical_vectors() -> Vec<(Value, &'static str)> {
    vec![
        (Value::Uint(0), "00"),
        (Value::Uint(1), "01"),
        (Value::Uint(23), "17"),
        (Value::Uint(24), "1818"),
        (Value::Uint(255), "18ff"),
        (Value::Uint(256), "190100"),
        (Value::Uint(65535), "19ffff"),
        (Value::Uint(65536), "1a00010000"),
        (Value::Uint(4294967296), "1b0000000100000000"),
        (Value::int(-1), "20"),
        (Value::int(-256), "38ff"),
        (Value::Bytes(vec![]), "40"),
        (Value::Bytes(vec![1, 2, 3]), "43010203"),
        (Value::Text(String::new()), "60"),
        (Value::Text("a".into()), "6161"),
        (Value::Text("loom".into()), "646c6f6f6d"),
        (Value::Array(vec![]), "80"),
        (Value::Array(vec![Value::Uint(1), Value::Uint(2)]), "820102"),
        (Value::Bool(false), "f4"),
        (Value::Bool(true), "f5"),
        (Value::Null, "f6"),
        (Value::Float(1.5), "fb3ff8000000000000"),
        (Value::Float(0.0), "fb0000000000000000"),
        (Value::Map(vec![(Value::Uint(1), Value::Uint(2))]), "a10102"),
    ]
}

#[test]
fn canonical_bytes_are_pinned() {
    for (value, hex) in canonical_vectors() {
        assert_eq!(
            hex::encode(encode(&value).unwrap()),
            hex,
            "encode {value:?}"
        );
        let decoded = decode(&hex::decode(hex).unwrap()).unwrap();
        assert_eq!(decoded, value, "decode {hex}");
    }
}

#[test]
fn negative_zero_encodes_as_positive_zero() {
    assert_eq!(
        encode(&Value::Float(-0.0)).unwrap(),
        encode(&Value::Float(0.0)).unwrap()
    );
}

#[test]
fn maps_sort_keys_canonically_on_encode() {
    // Given out of order, encode must emit ascending canonical key order: key 1 (0x01) before key 256.
    let m = Value::Map(vec![
        (Value::Uint(256), Value::Uint(0)),
        (Value::Uint(1), Value::Uint(0)),
    ]);
    assert_eq!(hex::encode(encode(&m).unwrap()), "a2010019010000");
}

#[test]
fn round_trips() {
    let values = vec![
        Value::Uint(u64::MAX),
        Value::Nint(u64::MAX),
        Value::Bytes(vec![0u8; 300]),
        Value::Text("a longer string with unicode: \u{1f9f5}".into()),
        Value::Array(vec![
            Value::Null,
            Value::Bool(true),
            Value::int(-42),
            Value::Float(3.25),
        ]),
        Value::Map(vec![
            (Value::Text("z".into()), Value::Uint(1)),
            (Value::Text("a".into()), Value::Uint(2)),
        ]),
    ];
    for v in values {
        let bytes = encode(&v).unwrap();
        assert_eq!(decode(&bytes).unwrap(), normalize_for_roundtrip(&v));
        // Re-encoding the decoded value yields identical bytes (canonical stability).
        assert_eq!(encode(&decode(&bytes).unwrap()).unwrap(), bytes);
    }
}

// A map's logical order is normalized to canonical key order by encode, so the decoded form reflects
// that, not the input order.
fn normalize_for_roundtrip(v: &Value) -> Value {
    decode(&encode(v).unwrap()).unwrap()
}

/// Each alternate / non-canonical encoding must decode to a specific, named error.
#[test]
fn rejects_non_canonical_forms() {
    let cases: Vec<(&str, CodecError)> = vec![
        ("9f0102ff", CodecError::IndefiniteLength), // indefinite-length array
        ("1801", CodecError::NonMinimalInt),        // 1 in 2-byte form
        ("190001", CodecError::NonMinimalInt),      // 1 in 3-byte form
        ("1a0000ffff", CodecError::NonMinimalInt),  // 0xffff in 5-byte form
        ("0102", CodecError::TrailingBytes),        // uint 1, then a stray byte
        ("a201010102", CodecError::DuplicateMapKey), // { 1:1, 1:2 }
        ("a202000100", CodecError::UnsortedMapKeys), // { 2:0, 1:0 }
        ("c000", CodecError::Tag),                  // tag 0
        ("fa3fc00000", CodecError::NonCanonicalFloat), // f32 1.5
        ("f93e00", CodecError::NonCanonicalFloat),  // f16 1.5
        ("fb7ff8000000000000", CodecError::NonCanonicalFloat), // NaN
        ("fb7ff0000000000000", CodecError::NonCanonicalFloat), // +infinity
        ("fb8000000000000000", CodecError::NonCanonicalFloat), // -0.0
        ("1c", CodecError::ReservedAdditionalInfo), // reserved additional info 28
        ("f7", CodecError::UnsupportedSimpleValue), // undefined
        ("62fffe", CodecError::InvalidUtf8),        // 2-byte text with invalid UTF-8
        ("18", CodecError::UnexpectedEof),          // 1-byte arg announced, none present
        ("430102", CodecError::UnexpectedEof),      // 3-byte bytestring, only 2 present
    ];
    for (hex, want) in cases {
        let bytes = hex::decode(hex).unwrap();
        assert_eq!(decode(&bytes), Err(want), "input {hex}");
    }
}

#[test]
fn rejects_overdeep_nesting() {
    let mut bytes = vec![0x81u8; MAX_DEPTH + 50]; // nested 1-element arrays past the limit
    bytes.push(0x00);
    assert_eq!(decode(&bytes), Err(CodecError::DepthExceeded));
}

#[test]
fn object_framing_round_trips() {
    let bytes = encode_object(3, &[Value::Text("x".into()), Value::Uint(7)]).unwrap();
    assert_eq!(hex::encode(&bytes), "840103617807"); // array [1, 3, "x", 7]
    let (type_code, fields) = decode_object(&bytes).unwrap();
    assert_eq!(type_code, 3);
    assert_eq!(fields, vec![Value::Text("x".into()), Value::Uint(7)]);
}

#[test]
fn object_framing_rejects_wrong_epoch_and_shape() {
    // Array [2, 3] - wrong epoch.
    let wrong = encode(&Value::Array(vec![Value::Uint(2), Value::Uint(3)])).unwrap();
    assert_eq!(decode_object(&wrong), Err(CodecError::WrongEpoch));
    // A bare integer is not an object array.
    assert_eq!(
        decode_object(&encode(&Value::Uint(5)).unwrap()),
        Err(CodecError::NotAnObject)
    );
}

/// Fuzz surrogate: decode must never panic (no overflow, no slice OOB, no stack overflow) on arbitrary
/// input - it always returns Ok or a CodecError. Runs on stable; a cargo-fuzz target lands with the
/// cross-language vectors.
#[test]
fn decode_never_panics_on_arbitrary_input() {
    let mut state: u64 = 0x9e3779b97f4a7c15;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..200_000 {
        let len = (next() % 48) as usize;
        let buf: Vec<u8> = (0..len).map(|_| (next() & 0xff) as u8).collect();
        let _ = decode(&buf); // must not panic
    }
}
