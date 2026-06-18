use loom_codec::Value as CborValue;
use loom_types::{Code, Digest, LoomError, Result, Value as TabularValue};
use serde_json::{Map, Value as JsonValue};

pub const PREDICATE_SCHEMA: &str = "loom.predicate.v1";
pub const MAX_JSON_BYTES: usize = 64 * 1024;
pub const MAX_COMPILED_BYTES: usize = 32 * 1024;
pub const MAX_DEPTH: usize = 32;
pub const MAX_NODES: usize = 1024;
pub const MAX_BOOL_ARITY: usize = 256;
pub const MAX_IN_VALUES: usize = 1024;
pub const MAX_PATH_SEGMENTS: usize = 16;
pub const MAX_PATH_SEGMENT_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
}

impl CompareOp {
    pub fn from_wire(op: &str) -> Option<Self> {
        match op {
            "eq" => Some(Self::Eq),
            "ne" => Some(Self::Ne),
            "lt" => Some(Self::Lt),
            "lte" => Some(Self::Lte),
            "gt" => Some(Self::Gt),
            "gte" => Some(Self::Gte),
            _ => None,
        }
    }

    pub const fn tag(self) -> u64 {
        match self {
            CompareOp::Eq => 10,
            CompareOp::Ne => 11,
            CompareOp::Lt => 12,
            CompareOp::Lte => 13,
            CompareOp::Gt => 14,
            CompareOp::Gte => 15,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextOp {
    StartsWith,
    EndsWith,
    Contains,
}

impl TextOp {
    fn from_wire(op: &str) -> Option<Self> {
        match op {
            "starts_with" => Some(Self::StartsWith),
            "ends_with" => Some(Self::EndsWith),
            "contains" => Some(Self::Contains),
            _ => None,
        }
    }

    const fn tag(self) -> u64 {
        match self {
            TextOp::StartsWith => 20,
            TextOp::EndsWith => 21,
            TextOp::Contains => 22,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Null,
    Bool(bool),
    I64(i64),
    U64(u64),
    Text(String),
    Bytes(Vec<u8>),
    Digest(Digest),
    TimestampMs(i64),
    List(Vec<Literal>),
}

impl Literal {
    pub fn to_tabular_value(&self) -> Result<TabularValue> {
        match self {
            Literal::Null => Ok(TabularValue::Null),
            Literal::Bool(value) => Ok(TabularValue::Bool(*value)),
            Literal::I64(value) => Ok(TabularValue::Int(*value)),
            Literal::U64(value) => Ok(TabularValue::U64(*value)),
            Literal::Text(value) => Ok(TabularValue::Text(value.clone())),
            Literal::Bytes(value) => Ok(TabularValue::Bytes(value.clone())),
            Literal::TimestampMs(value) => Ok(TabularValue::Timestamp(*value)),
            Literal::List(values) => values
                .iter()
                .map(Literal::to_tabular_value)
                .collect::<Result<Vec<_>>>()
                .map(TabularValue::List),
            Literal::Digest(_) => Err(LoomError::unsupported(
                "digest predicate literal cannot lower to current tabular value",
            )),
        }
    }

    fn to_value(&self) -> CborValue {
        match self {
            Literal::Null => CborValue::Array(vec![CborValue::Uint(0), CborValue::Null]),
            Literal::Bool(value) => {
                CborValue::Array(vec![CborValue::Uint(1), CborValue::Bool(*value)])
            }
            Literal::I64(value) => CborValue::Array(vec![CborValue::Uint(2), cbor_i64(*value)]),
            Literal::U64(value) => {
                CborValue::Array(vec![CborValue::Uint(3), CborValue::Uint(*value)])
            }
            Literal::Text(value) => {
                CborValue::Array(vec![CborValue::Uint(4), CborValue::Text(value.clone())])
            }
            Literal::Bytes(value) => {
                CborValue::Array(vec![CborValue::Uint(5), CborValue::Bytes(value.clone())])
            }
            Literal::Digest(value) => CborValue::Array(vec![
                CborValue::Uint(6),
                CborValue::Array(vec![
                    CborValue::Uint(u64::from(value.algo().code())),
                    CborValue::Bytes(value.bytes().to_vec()),
                ]),
            ]),
            Literal::TimestampMs(value) => {
                CborValue::Array(vec![CborValue::Uint(7), cbor_i64(*value)])
            }
            Literal::List(values) => CborValue::Array(vec![
                CborValue::Uint(8),
                CborValue::Array(values.iter().map(Literal::to_value).collect()),
            ]),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PredicateExpr {
    True,
    False,
    And(Vec<PredicateExpr>),
    Or(Vec<PredicateExpr>),
    Not(Box<PredicateExpr>),
    Compare {
        op: CompareOp,
        path: Vec<String>,
        value: Literal,
    },
    Between {
        path: Vec<String>,
        lo: Literal,
        hi: Literal,
        include_lo: bool,
        include_hi: bool,
    },
    In {
        path: Vec<String>,
        values: Vec<Literal>,
    },
    Exists {
        path: Vec<String>,
    },
    IsNull {
        path: Vec<String>,
    },
    Text {
        op: TextOp,
        path: Vec<String>,
        value: String,
    },
}

impl PredicateExpr {
    pub fn to_value(&self) -> CborValue {
        match self {
            PredicateExpr::True => CborValue::Array(vec![CborValue::Uint(0)]),
            PredicateExpr::False => CborValue::Array(vec![CborValue::Uint(1)]),
            PredicateExpr::And(args) => CborValue::Array(vec![
                CborValue::Uint(2),
                CborValue::Array(args.iter().map(PredicateExpr::to_value).collect()),
            ]),
            PredicateExpr::Or(args) => CborValue::Array(vec![
                CborValue::Uint(3),
                CborValue::Array(args.iter().map(PredicateExpr::to_value).collect()),
            ]),
            PredicateExpr::Not(arg) => CborValue::Array(vec![CborValue::Uint(4), arg.to_value()]),
            PredicateExpr::Compare { op, path, value } => CborValue::Array(vec![
                CborValue::Uint(op.tag()),
                path_value(path),
                value.to_value(),
            ]),
            PredicateExpr::Between {
                path,
                lo,
                hi,
                include_lo,
                include_hi,
            } => CborValue::Array(vec![
                CborValue::Uint(16),
                path_value(path),
                lo.to_value(),
                hi.to_value(),
                CborValue::Bool(*include_lo),
                CborValue::Bool(*include_hi),
            ]),
            PredicateExpr::In { path, values } => CborValue::Array(vec![
                CborValue::Uint(17),
                path_value(path),
                CborValue::Array(values.iter().map(Literal::to_value).collect()),
            ]),
            PredicateExpr::Exists { path } => {
                CborValue::Array(vec![CborValue::Uint(18), path_value(path)])
            }
            PredicateExpr::IsNull { path } => {
                CborValue::Array(vec![CborValue::Uint(19), path_value(path)])
            }
            PredicateExpr::Text { op, path, value } => CborValue::Array(vec![
                CborValue::Uint(op.tag()),
                path_value(path),
                CborValue::Text(value.clone()),
            ]),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Predicate {
    pub expr: PredicateExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SimpleComparison {
    pub path: Vec<String>,
    pub op: CompareOp,
    pub value: Literal,
}

#[derive(Default)]
struct Budget {
    nodes: usize,
}

impl Predicate {
    pub fn from_json_value(value: &JsonValue) -> Result<Self> {
        let object = json_object(value, "predicate")?;
        if object.len() != 2 || !object.contains_key("version") || !object.contains_key("expr") {
            return Err(LoomError::invalid(
                "predicate root must contain exactly version and expr",
            ));
        }
        if object.get("version").and_then(JsonValue::as_u64) != Some(1) {
            return Err(LoomError::invalid("predicate.version must be 1"));
        }
        let mut budget = Budget::default();
        let expr = parse_expr(
            object
                .get("expr")
                .ok_or_else(|| LoomError::invalid("predicate.expr is required"))?,
            0,
            &mut budget,
        )?;
        Ok(Self { expr })
    }

    pub fn compile_json_str(json: &str) -> Result<Vec<u8>> {
        if json.len() > MAX_JSON_BYTES {
            return Err(LoomError::invalid("predicate JSON exceeds size limit"));
        }
        let value: JsonValue = serde_json::from_str(json)
            .map_err(|e| LoomError::invalid(format!("predicate JSON: {e}")))?;
        Self::from_json_value(&value)?.encode()
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let bytes = loom_codec::encode(&self.to_value()).map_err(codec_error)?;
        if bytes.len() > MAX_COMPILED_BYTES {
            return Err(LoomError::invalid("compiled predicate exceeds size limit"));
        }
        Ok(bytes)
    }

    pub fn to_value(&self) -> CborValue {
        CborValue::Array(vec![
            CborValue::Text(PREDICATE_SCHEMA.to_string()),
            self.expr.to_value(),
        ])
    }

    pub fn as_simple_comparison(&self) -> Option<SimpleComparison> {
        match &self.expr {
            PredicateExpr::Compare { op, path, value } if path.len() == 1 => {
                Some(SimpleComparison {
                    path: path.clone(),
                    op: *op,
                    value: value.clone(),
                })
            }
            _ => None,
        }
    }
}

fn parse_expr(value: &JsonValue, depth: usize, budget: &mut Budget) -> Result<PredicateExpr> {
    if depth > MAX_DEPTH {
        return Err(LoomError::invalid("predicate exceeds maximum depth"));
    }
    budget.nodes += 1;
    if budget.nodes > MAX_NODES {
        return Err(LoomError::invalid("predicate exceeds maximum node count"));
    }
    let object = json_object(value, "predicate expr")?;
    let op = required_str(object, "op", "predicate expr op")?;
    match op {
        "true" => {
            require_only(object, &["op"], "predicate true")?;
            Ok(PredicateExpr::True)
        }
        "false" => {
            require_only(object, &["op"], "predicate false")?;
            Ok(PredicateExpr::False)
        }
        "and" | "or" => {
            require_only(object, &["op", "args"], "predicate boolean")?;
            let args = required_array(object, "args", "predicate boolean args")?;
            if args.is_empty() {
                return Err(LoomError::invalid(
                    "predicate and/or args must not be empty",
                ));
            }
            if args.len() > MAX_BOOL_ARITY {
                return Err(LoomError::invalid("predicate boolean arity exceeds limit"));
            }
            let parsed = args
                .iter()
                .map(|arg| parse_expr(arg, depth + 1, budget))
                .collect::<Result<Vec<_>>>()?;
            if op == "and" {
                Ok(PredicateExpr::And(parsed))
            } else {
                Ok(PredicateExpr::Or(parsed))
            }
        }
        "not" => {
            require_only(object, &["op", "arg"], "predicate not")?;
            Ok(PredicateExpr::Not(Box::new(parse_expr(
                required(object, "arg", "predicate not arg")?,
                depth + 1,
                budget,
            )?)))
        }
        "eq" | "ne" | "lt" | "lte" | "gt" | "gte" => {
            require_only(object, &["op", "path", "value"], "predicate comparison")?;
            Ok(PredicateExpr::Compare {
                op: CompareOp::from_wire(op).expect("comparison op is matched"),
                path: parse_path(required(object, "path", "predicate path")?)?,
                value: parse_literal(required(object, "value", "predicate value")?, depth + 1)?,
            })
        }
        "between" => {
            require_keys(
                object,
                &["op", "path", "lo", "hi", "include_lo", "include_hi"],
                &["op", "path", "lo", "hi"],
                "predicate between",
            )?;
            Ok(PredicateExpr::Between {
                path: parse_path(required(object, "path", "predicate path")?)?,
                lo: parse_literal(required(object, "lo", "predicate lo")?, depth + 1)?,
                hi: parse_literal(required(object, "hi", "predicate hi")?, depth + 1)?,
                include_lo: optional_bool(object, "include_lo")?.unwrap_or(true),
                include_hi: optional_bool(object, "include_hi")?.unwrap_or(true),
            })
        }
        "in" => {
            require_only(object, &["op", "path", "values"], "predicate in")?;
            let values = required_array(object, "values", "predicate values")?;
            if values.is_empty() {
                return Err(LoomError::invalid("predicate in values must not be empty"));
            }
            if values.len() > MAX_IN_VALUES {
                return Err(LoomError::invalid("predicate in values exceed limit"));
            }
            Ok(PredicateExpr::In {
                path: parse_path(required(object, "path", "predicate path")?)?,
                values: values
                    .iter()
                    .map(|value| parse_literal(value, depth + 1))
                    .collect::<Result<Vec<_>>>()?,
            })
        }
        "exists" => {
            require_only(object, &["op", "path"], "predicate exists")?;
            Ok(PredicateExpr::Exists {
                path: parse_path(required(object, "path", "predicate path")?)?,
            })
        }
        "is_null" => {
            require_only(object, &["op", "path"], "predicate is_null")?;
            Ok(PredicateExpr::IsNull {
                path: parse_path(required(object, "path", "predicate path")?)?,
            })
        }
        "starts_with" | "ends_with" | "contains" => {
            require_only(object, &["op", "path", "value"], "predicate text")?;
            let literal = parse_literal(required(object, "value", "predicate value")?, depth + 1)?;
            let Literal::Text(value) = literal else {
                return Err(LoomError::invalid("text predicate value must be text"));
            };
            Ok(PredicateExpr::Text {
                op: TextOp::from_wire(op).expect("text op is matched"),
                path: parse_path(required(object, "path", "predicate path")?)?,
                value,
            })
        }
        _ => Err(LoomError::invalid("unknown predicate operator")),
    }
}

fn parse_literal(value: &JsonValue, depth: usize) -> Result<Literal> {
    if depth > MAX_DEPTH {
        return Err(LoomError::invalid(
            "predicate literal exceeds maximum depth",
        ));
    }
    let object = json_object(value, "predicate literal")?;
    require_only(object, &["type", "value"], "predicate literal")?;
    let ty = required_str(object, "type", "predicate literal type")?;
    let raw = required(object, "value", "predicate literal value")?;
    match ty {
        "null" => {
            if raw.is_null() {
                Ok(Literal::Null)
            } else {
                Err(LoomError::invalid("null literal value must be null"))
            }
        }
        "bool" => raw
            .as_bool()
            .map(Literal::Bool)
            .ok_or_else(|| LoomError::invalid("bool literal value must be boolean")),
        "i64" => json_i64(raw).map(Literal::I64),
        "u64" => json_u64(raw).map(Literal::U64),
        "text" => raw
            .as_str()
            .map(|value| Literal::Text(value.to_string()))
            .ok_or_else(|| LoomError::invalid("text literal value must be string")),
        "bytes" => raw
            .as_str()
            .map(hex_bytes)
            .transpose()?
            .map(Literal::Bytes)
            .ok_or_else(|| LoomError::invalid("bytes literal value must be lowercase hex string")),
        "digest" => raw
            .as_str()
            .map(Digest::parse)
            .transpose()?
            .map(Literal::Digest)
            .ok_or_else(|| LoomError::invalid("digest literal value must be string")),
        "timestamp_ms" => json_i64(raw).map(Literal::TimestampMs),
        "list" => {
            let items = raw
                .as_array()
                .ok_or_else(|| LoomError::invalid("list literal value must be an array"))?;
            items
                .iter()
                .map(|item| parse_literal(item, depth + 1))
                .collect::<Result<Vec<_>>>()
                .map(Literal::List)
        }
        _ => Err(LoomError::invalid("unknown predicate literal type")),
    }
}

fn parse_path(value: &JsonValue) -> Result<Vec<String>> {
    let parts = value
        .as_array()
        .ok_or_else(|| LoomError::invalid("predicate path must be an array"))?;
    if parts.is_empty() {
        return Err(LoomError::invalid("predicate path must not be empty"));
    }
    if parts.len() > MAX_PATH_SEGMENTS {
        return Err(LoomError::invalid("predicate path has too many segments"));
    }
    parts
        .iter()
        .map(|part| {
            let segment = part
                .as_str()
                .ok_or_else(|| LoomError::invalid("predicate path segment must be a string"))?;
            if segment.is_empty()
                || segment == "."
                || segment == ".."
                || segment.contains('/')
                || segment.len() > MAX_PATH_SEGMENT_BYTES
            {
                return Err(LoomError::invalid("invalid predicate path segment"));
            }
            Ok(segment.to_string())
        })
        .collect()
}

fn path_value(path: &[String]) -> CborValue {
    CborValue::Array(
        path.iter()
            .map(|segment| CborValue::Text(segment.clone()))
            .collect(),
    )
}

fn json_object<'a>(value: &'a JsonValue, name: &str) -> Result<&'a Map<String, JsonValue>> {
    value
        .as_object()
        .ok_or_else(|| LoomError::invalid(format!("{name} must be an object")))
}

fn required<'a>(
    object: &'a Map<String, JsonValue>,
    key: &str,
    name: &str,
) -> Result<&'a JsonValue> {
    object
        .get(key)
        .ok_or_else(|| LoomError::invalid(format!("{name} is required")))
}

fn required_str<'a>(object: &'a Map<String, JsonValue>, key: &str, name: &str) -> Result<&'a str> {
    required(object, key, name)?
        .as_str()
        .ok_or_else(|| LoomError::invalid(format!("{name} must be a string")))
}

fn required_array<'a>(
    object: &'a Map<String, JsonValue>,
    key: &str,
    name: &str,
) -> Result<&'a Vec<JsonValue>> {
    required(object, key, name)?
        .as_array()
        .ok_or_else(|| LoomError::invalid(format!("{name} must be an array")))
}

fn optional_bool(object: &Map<String, JsonValue>, key: &str) -> Result<Option<bool>> {
    object
        .get(key)
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| LoomError::invalid(format!("{key} must be boolean")))
        })
        .transpose()
}

fn require_only(object: &Map<String, JsonValue>, allowed: &[&str], name: &str) -> Result<()> {
    require_keys(object, allowed, allowed, name)
}

fn require_keys(
    object: &Map<String, JsonValue>,
    allowed: &[&str],
    required_keys: &[&str],
    name: &str,
) -> Result<()> {
    let allowed = allowed
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    for key in object.keys() {
        if !allowed.contains(key.as_str()) {
            return Err(LoomError::invalid(format!(
                "{name} contains unknown key {key:?}"
            )));
        }
    }
    for key in required_keys {
        if !object.contains_key(*key) {
            return Err(LoomError::invalid(format!("{name} missing key {key:?}")));
        }
    }
    Ok(())
}

fn json_i64(value: &JsonValue) -> Result<i64> {
    if let Some(value) = value.as_i64() {
        return Ok(value);
    }
    value
        .as_str()
        .ok_or_else(|| LoomError::invalid("integer literal value must be integer or string"))?
        .parse::<i64>()
        .map_err(|_| LoomError::invalid("integer literal value is out of range"))
}

fn json_u64(value: &JsonValue) -> Result<u64> {
    if let Some(value) = value.as_u64() {
        return Ok(value);
    }
    value
        .as_str()
        .ok_or_else(|| LoomError::invalid("unsigned literal value must be integer or string"))?
        .parse::<u64>()
        .map_err(|_| LoomError::invalid("unsigned literal value is out of range"))
}

fn hex_bytes(value: &str) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return Err(LoomError::invalid("hex literal has odd length"));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let hi = hex_nibble(pair[0])?;
            let lo = hex_nibble(pair[1])?;
            Ok((hi << 4) | lo)
        })
        .collect()
}

fn hex_nibble(value: u8) -> Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(LoomError::invalid("hex literal must use lowercase hex")),
    }
}

fn cbor_i64(value: i64) -> CborValue {
    if value >= 0 {
        CborValue::Uint(value as u64)
    } else {
        CborValue::Nint((-1 - value) as u64)
    }
}

fn codec_error(error: loom_codec::CodecError) -> LoomError {
    LoomError::new(Code::InvalidArgument, format!("predicate cbor: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn predicate(value: JsonValue) -> Predicate {
        Predicate::from_json_value(&value).unwrap()
    }

    #[test]
    fn equivalent_numeric_spellings_compile_to_same_bytes() {
        let as_int = predicate(json!({
            "version": 1,
            "expr": { "op": "gte", "path": ["priority"], "value": { "type": "i64", "value": 3 } }
        }));
        let as_string = predicate(json!({
            "version": 1,
            "expr": { "op": "gte", "path": ["priority"], "value": { "type": "i64", "value": "3" } }
        }));
        assert_eq!(as_int.encode().unwrap(), as_string.encode().unwrap());
    }

    #[test]
    fn every_operator_family_parses() {
        let value = json!({
            "version": 1,
            "expr": {
                "op": "and",
                "args": [
                    { "op": "true" },
                    { "op": "not", "arg": { "op": "false" } },
                    { "op": "between", "path": ["n"], "lo": { "type": "u64", "value": 1 }, "hi": { "type": "u64", "value": "9" }, "include_hi": false },
                    { "op": "in", "path": ["status"], "values": [{ "type": "text", "value": "open" }] },
                    { "op": "exists", "path": ["assignee"] },
                    { "op": "is_null", "path": ["closed_at"] },
                    { "op": "contains", "path": ["title"], "value": { "type": "text", "value": "loom" } }
                ]
            }
        });
        assert!(
            !Predicate::from_json_value(&value)
                .unwrap()
                .encode()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn simple_comparison_lowers_for_current_columnar_path() {
        let pred = predicate(json!({
            "version": 1,
            "expr": { "op": "eq", "path": ["name"], "value": { "type": "text", "value": "alpha" } }
        }));
        let simple = pred.as_simple_comparison().unwrap();
        assert_eq!(simple.path, vec!["name"]);
        assert_eq!(simple.op, CompareOp::Eq);
        assert_eq!(
            simple.value.to_tabular_value().unwrap(),
            TabularValue::Text("alpha".to_string())
        );
    }

    #[test]
    fn invalid_paths_and_unknown_keys_are_rejected() {
        assert!(
            Predicate::from_json_value(&json!({
                "version": 1,
                "expr": { "op": "exists", "path": [".."] }
            }))
            .is_err()
        );
        assert!(
            Predicate::from_json_value(&json!({
                "version": 1,
                "expr": { "op": "true", "extra": 1 }
            }))
            .is_err()
        );
    }

    #[test]
    fn uppercase_hex_is_rejected() {
        assert!(
            Predicate::from_json_value(&json!({
                "version": 1,
                "expr": { "op": "eq", "path": ["b"], "value": { "type": "bytes", "value": "AA" } }
            }))
            .is_err()
        );
    }
}
