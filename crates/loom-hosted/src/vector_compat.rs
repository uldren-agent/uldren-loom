use loom_core::error::{Code, LoomError, Result};
use loom_core::tabular::Value as CellValue;
use loom_core::vector::MetaFilter;
use serde_json::Value;

pub const PRIMARY_QDRANT_VECTOR: &str = "";
pub const DEFAULT_PINECONE_WORKSPACE: &str = "";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorPresentationMapping {
    pub profile: &'static str,
    pub collection: String,
    pub vector_set: String,
    pub profile_workspace: Option<String>,
    pub named_vector: Option<String>,
}

pub fn generic_mapping(collection: &str) -> Result<VectorPresentationMapping> {
    require_name("collection", collection)?;
    Ok(VectorPresentationMapping {
        profile: "generic",
        collection: collection.to_string(),
        vector_set: format!("compat/generic/{}", segment(collection)),
        profile_workspace: None,
        named_vector: None,
    })
}

pub fn qdrant_mapping(
    collection: &str,
    vector_name: Option<&str>,
) -> Result<VectorPresentationMapping> {
    require_name("collection", collection)?;
    let vector_name = vector_name.unwrap_or(PRIMARY_QDRANT_VECTOR);
    let vector_path = if vector_name.is_empty() {
        "primary".to_string()
    } else {
        format!("named/{}", segment(vector_name))
    };
    Ok(VectorPresentationMapping {
        profile: "qdrant",
        collection: collection.to_string(),
        vector_set: format!(
            "compat/qdrant/{}/vectors/{vector_path}",
            segment(collection)
        ),
        profile_workspace: None,
        named_vector: if vector_name.is_empty() {
            None
        } else {
            Some(vector_name.to_string())
        },
    })
}

pub fn pinecone_mapping(index: &str, workspace: Option<&str>) -> Result<VectorPresentationMapping> {
    require_name("index", index)?;
    let workspace = workspace.unwrap_or(DEFAULT_PINECONE_WORKSPACE);
    Ok(VectorPresentationMapping {
        profile: "pinecone",
        collection: index.to_string(),
        vector_set: format!(
            "compat/pinecone/{}/workspaces/{}",
            segment(index),
            segment(workspace)
        ),
        profile_workspace: Some(workspace.to_string()),
        named_vector: None,
    })
}

pub fn qdrant_filter_from_json(value: &Value) -> Result<MetaFilter> {
    match value {
        Value::Null => Ok(MetaFilter::All),
        Value::Object(object) if object.is_empty() => Ok(MetaFilter::All),
        Value::Object(object) if is_qdrant_condition(object) => qdrant_condition(value),
        Value::Object(object) => {
            let mut parts = Vec::new();
            if let Some(must) = object.get("must") {
                parts.push(and_all(qdrant_filter_array(must, "must")?));
            }
            if let Some(should) = object.get("should") {
                parts.push(or_all(qdrant_filter_array(should, "should")?));
            }
            if let Some(must_not) = object.get("must_not") {
                for filter in qdrant_filter_array(must_not, "must_not")? {
                    parts.push(MetaFilter::Not(Box::new(filter)));
                }
            }
            if parts.is_empty() {
                Err(unsupported("qdrant filter object has no supported clauses"))
            } else {
                Ok(and_all(parts))
            }
        }
        _ => Err(invalid("qdrant filter must be an object")),
    }
}

pub fn pinecone_filter_from_json(value: &Value) -> Result<MetaFilter> {
    match value {
        Value::Null => Ok(MetaFilter::All),
        Value::Object(object) if object.is_empty() => Ok(MetaFilter::All),
        Value::Object(object) => {
            let mut parts = Vec::new();
            for (key, value) in object {
                match key.as_str() {
                    "$and" => parts.push(and_all(pinecone_filter_array(value, "$and")?)),
                    "$or" => parts.push(or_all(pinecone_filter_array(value, "$or")?)),
                    "$nor" => {
                        for filter in pinecone_filter_array(value, "$nor")? {
                            parts.push(MetaFilter::Not(Box::new(filter)));
                        }
                    }
                    key if key.starts_with('$') => {
                        return Err(unsupported(format!("pinecone operator {key}")));
                    }
                    field => parts.push(pinecone_field_filter(field, value)?),
                }
            }
            Ok(and_all(parts))
        }
        _ => Err(invalid("pinecone filter must be an object")),
    }
}

fn qdrant_filter_array(value: &Value, name: &str) -> Result<Vec<MetaFilter>> {
    let Value::Array(items) = value else {
        return Ok(vec![qdrant_condition(value)?]);
    };
    if items.is_empty() {
        return Err(invalid(format!("qdrant {name} must not be empty")));
    }
    items.iter().map(qdrant_condition).collect()
}

fn qdrant_condition(value: &Value) -> Result<MetaFilter> {
    let object = object(value, "qdrant condition")?;
    if object.contains_key("nested") {
        return Err(unsupported("qdrant nested filter"));
    }
    if object.contains_key("has_id") {
        return Err(unsupported("qdrant has_id filter"));
    }
    if let Some(is_empty) = object.get("is_empty") {
        let key = qdrant_key_object(is_empty, "is_empty")?;
        return Ok(MetaFilter::Not(Box::new(MetaFilter::Exists(key))));
    }
    if let Some(is_null) = object.get("is_null") {
        let key = qdrant_key_object(is_null, "is_null")?;
        return Ok(MetaFilter::Eq(key, CellValue::Null));
    }
    let key = string_field(value, "key")?;
    let mut parts = Vec::new();
    if let Some(match_value) = object.get("match") {
        parts.push(qdrant_match_filter(&key, match_value)?);
    }
    if let Some(range) = object.get("range") {
        parts.push(qdrant_range_filter(&key, range)?);
    }
    if let Some(values_count) = object.get("values_count") {
        return Err(unsupported(format!(
            "qdrant values_count filter for {key}: {values_count}"
        )));
    }
    if parts.is_empty() {
        Err(unsupported("qdrant condition has no supported operator"))
    } else {
        Ok(and_all(parts))
    }
}

fn qdrant_match_filter(key: &str, value: &Value) -> Result<MetaFilter> {
    let object = object(value, "qdrant match")?;
    if let Some(value) = object.get("value") {
        return Ok(MetaFilter::Eq(key.to_string(), cell_value(value)?));
    }
    if let Some(any) = object.get("any") {
        return Ok(MetaFilter::In(
            key.to_string(),
            cell_array(any, "match.any")?,
        ));
    }
    if let Some(except) = object.get("except") {
        return Ok(MetaFilter::Not(Box::new(MetaFilter::In(
            key.to_string(),
            cell_array(except, "match.except")?,
        ))));
    }
    if object.contains_key("text") {
        return Err(unsupported("qdrant match.text full-text filter"));
    }
    Err(unsupported("qdrant match operator"))
}

fn qdrant_range_filter(key: &str, value: &Value) -> Result<MetaFilter> {
    let object = object(value, "qdrant range")?;
    let mut parts = Vec::new();
    for (operator, value) in object {
        let value = cell_value(value)?;
        parts.push(match operator.as_str() {
            "gt" => MetaFilter::Gt(key.to_string(), value),
            "gte" => MetaFilter::Ge(key.to_string(), value),
            "lt" => MetaFilter::Lt(key.to_string(), value),
            "lte" => MetaFilter::Le(key.to_string(), value),
            other => return Err(unsupported(format!("qdrant range operator {other}"))),
        });
    }
    if parts.is_empty() {
        Err(invalid("qdrant range must not be empty"))
    } else {
        Ok(and_all(parts))
    }
}

fn pinecone_filter_array(value: &Value, name: &str) -> Result<Vec<MetaFilter>> {
    let Value::Array(items) = value else {
        return Err(invalid(format!("pinecone {name} must be an array")));
    };
    if items.is_empty() {
        return Err(invalid(format!("pinecone {name} must not be empty")));
    }
    items.iter().map(pinecone_filter_from_json).collect()
}

fn pinecone_field_filter(field: &str, value: &Value) -> Result<MetaFilter> {
    if let Value::Object(object) = value {
        let mut parts = Vec::new();
        for (operator, value) in object {
            parts.push(match operator.as_str() {
                "$eq" => MetaFilter::Eq(field.to_string(), cell_value(value)?),
                "$ne" => MetaFilter::Ne(field.to_string(), cell_value(value)?),
                "$gt" => MetaFilter::Gt(field.to_string(), cell_value(value)?),
                "$gte" => MetaFilter::Ge(field.to_string(), cell_value(value)?),
                "$lt" => MetaFilter::Lt(field.to_string(), cell_value(value)?),
                "$lte" => MetaFilter::Le(field.to_string(), cell_value(value)?),
                "$in" => MetaFilter::In(field.to_string(), cell_array(value, "$in")?),
                "$nin" => MetaFilter::Not(Box::new(MetaFilter::In(
                    field.to_string(),
                    cell_array(value, "$nin")?,
                ))),
                "$exists" => exists_filter(field, value)?,
                other => return Err(unsupported(format!("pinecone field operator {other}"))),
            });
        }
        if parts.is_empty() {
            Err(invalid(format!("pinecone field {field} has no operators")))
        } else {
            Ok(and_all(parts))
        }
    } else {
        Ok(MetaFilter::Eq(field.to_string(), cell_value(value)?))
    }
}

fn exists_filter(field: &str, value: &Value) -> Result<MetaFilter> {
    match value {
        Value::Bool(true) => Ok(MetaFilter::Exists(field.to_string())),
        Value::Bool(false) => Ok(MetaFilter::Not(Box::new(MetaFilter::Exists(
            field.to_string(),
        )))),
        _ => Err(invalid("pinecone $exists must be boolean")),
    }
}

fn and_all(mut parts: Vec<MetaFilter>) -> MetaFilter {
    if parts.is_empty() {
        return MetaFilter::All;
    }
    let first = parts.remove(0);
    parts.into_iter().fold(first, |left, right| {
        MetaFilter::And(Box::new(left), Box::new(right))
    })
}

fn or_all(mut parts: Vec<MetaFilter>) -> MetaFilter {
    if parts.is_empty() {
        return MetaFilter::All;
    }
    let first = parts.remove(0);
    parts.into_iter().fold(first, |left, right| {
        MetaFilter::Or(Box::new(left), Box::new(right))
    })
}

fn is_qdrant_condition(object: &serde_json::Map<String, Value>) -> bool {
    object.contains_key("key")
        || object.contains_key("is_empty")
        || object.contains_key("is_null")
        || object.contains_key("has_id")
        || object.contains_key("nested")
}

fn qdrant_key_object(value: &Value, name: &str) -> Result<String> {
    string_field(value, "key").map_err(|_| invalid(format!("qdrant {name} requires key")))
}

fn string_field(value: &Value, name: &str) -> Result<String> {
    value
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| invalid(format!("{name} must be a non-empty string")))
}

fn object<'a>(value: &'a Value, name: &str) -> Result<&'a serde_json::Map<String, Value>> {
    match value {
        Value::Object(object) => Ok(object),
        _ => Err(invalid(format!("{name} must be an object"))),
    }
}

fn cell_array(value: &Value, name: &str) -> Result<Vec<CellValue>> {
    let Value::Array(values) = value else {
        return Err(invalid(format!("{name} must be an array")));
    };
    if values.is_empty() {
        return Err(invalid(format!("{name} must not be empty")));
    }
    values.iter().map(cell_value).collect()
}

fn cell_value(value: &Value) -> Result<CellValue> {
    match value {
        Value::Null => Ok(CellValue::Null),
        Value::Bool(value) => Ok(CellValue::Bool(*value)),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(CellValue::Int(value))
            } else if let Some(value) = value.as_f64().filter(|value| value.is_finite()) {
                Ok(CellValue::Float(value))
            } else {
                Err(invalid("metadata number is outside supported range"))
            }
        }
        Value::String(value) => Ok(CellValue::Text(value.clone())),
        Value::Array(_) | Value::Object(_) => Err(invalid(
            "metadata values must be null, bool, number, or string",
        )),
    }
}

fn require_name(kind: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        Err(invalid(format!("{kind} must not be empty")))
    } else {
        Ok(())
    }
}

fn segment(value: &str) -> String {
    let mut out = String::with_capacity(1 + value.len() * 2);
    out.push('h');
    for byte in value.as_bytes() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn invalid(message: impl Into<String>) -> LoomError {
    LoomError::new(Code::InvalidArgument, message)
}

fn unsupported(message: impl Into<String>) -> LoomError {
    LoomError::new(
        Code::InvalidArgument,
        format!(
            "unsupported vector compatibility filter: {}",
            message.into()
        ),
    )
}

#[cfg(test)]
mod tests {
    use loom_core::vector::VectorSet;

    use super::*;

    #[test]
    fn qdrant_filter_maps_to_base_predicates_and_preserves_search_order() {
        let filter = qdrant_filter_from_json(&serde_json::json!({
            "must": [
                {"key": "lang", "match": {"any": ["en", "fr"]}},
                {"key": "score", "range": {"gte": 8, "lt": 10}}
            ],
            "must_not": [
                {"key": "archived", "match": {"value": true}}
            ]
        }))
        .unwrap();
        let mut set = VectorSet::new(2, loom_core::Metric::Dot);
        set.upsert("b", vec![1.0, 0.0], metadata("en", 9, false))
            .unwrap();
        set.upsert("a", vec![1.0, 0.0], metadata("en", 9, false))
            .unwrap();
        set.upsert("c", vec![0.9, 0.0], metadata("fr", 8, false))
            .unwrap();
        set.upsert("d", vec![1.0, 0.0], metadata("en", 9, true))
            .unwrap();
        let hits = set.search(&[1.0, 0.0], 10, &filter).unwrap();
        assert_eq!(
            hits.into_iter().map(|hit| hit.id).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
    }

    #[test]
    fn pinecone_filter_maps_to_base_predicates() {
        let filter = pinecone_filter_from_json(&serde_json::json!({
            "$and": [
                {"lang": {"$in": ["en", "fr"]}},
                {"score": {"$gte": 8, "$lt": 10}},
                {"archived": {"$exists": false}}
            ]
        }))
        .unwrap();
        let mut set = VectorSet::new(2, loom_core::Metric::Dot);
        set.upsert("a", vec![1.0, 0.0], metadata_without_archived("en", 9))
            .unwrap();
        set.upsert("b", vec![1.0, 0.0], metadata("en", 9, false))
            .unwrap();
        set.upsert("c", vec![1.0, 0.0], metadata_without_archived("es", 9))
            .unwrap();
        let hits = set.search(&[1.0, 0.0], 10, &filter).unwrap();
        assert_eq!(
            hits.into_iter().map(|hit| hit.id).collect::<Vec<_>>(),
            ["a"]
        );
    }

    #[test]
    fn unsupported_vendor_filters_return_stable_errors() {
        let err = qdrant_filter_from_json(&serde_json::json!({
            "must": [{"key": "body", "match": {"text": "needle"}}]
        }))
        .unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
        assert!(
            err.message
                .contains("unsupported vector compatibility filter")
        );

        let err = pinecone_filter_from_json(&serde_json::json!({
            "body": {"$regex": "needle"}
        }))
        .unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
        assert!(
            err.message
                .contains("unsupported vector compatibility filter")
        );
    }

    #[test]
    fn compatibility_mapping_is_deterministic_and_profile_scoped() {
        let primary = qdrant_mapping("docs/prod", None).unwrap();
        let named = qdrant_mapping("docs/prod", Some("image")).unwrap();
        let pinecone_default = pinecone_mapping("docs/prod", None).unwrap();
        let pinecone_tenant = pinecone_mapping("docs/prod", Some("tenant/a")).unwrap();
        assert_eq!(
            primary.vector_set,
            "compat/qdrant/h646f63732f70726f64/vectors/primary"
        );
        assert_eq!(
            named.vector_set,
            "compat/qdrant/h646f63732f70726f64/vectors/named/h696d616765"
        );
        assert_eq!(
            pinecone_default.vector_set,
            "compat/pinecone/h646f63732f70726f64/workspaces/h"
        );
        assert_eq!(
            pinecone_tenant.vector_set,
            "compat/pinecone/h646f63732f70726f64/workspaces/h74656e616e742f61"
        );
        assert_ne!(primary.vector_set, pinecone_default.vector_set);
    }

    fn metadata(
        lang: &str,
        score: i64,
        archived: bool,
    ) -> std::collections::BTreeMap<String, CellValue> {
        let mut out = metadata_without_archived(lang, score);
        out.insert("archived".to_string(), CellValue::Bool(archived));
        out
    }

    fn metadata_without_archived(
        lang: &str,
        score: i64,
    ) -> std::collections::BTreeMap<String, CellValue> {
        std::collections::BTreeMap::from([
            ("lang".to_string(), CellValue::Text(lang.to_string())),
            ("score".to_string(), CellValue::Int(score)),
        ])
    }
}
