use std::collections::BTreeMap;

use loom_codec::Value as CborValue;
use loom_core::Object;
use loom_templates::{DiagnosticSeverity, TemplateBindings, TemplateProcessor};

use crate::{ExecError, Manifest};

pub const TEMPLATE_RESULT_SCHEMA: &str = "loom.template.result.v1";

#[derive(Debug, Clone, PartialEq)]
pub struct TemplateExecution {
    pub outputs: BTreeMap<String, serde_json::Value>,
    pub logs: Vec<String>,
    pub source_digest: loom_core::Digest,
    pub ast_digest: loom_core::Digest,
    pub host_calls: Vec<String>,
    pub diagnostics: Vec<String>,
}

pub fn render_template_program(
    manifest: &Manifest,
    source: &str,
    inputs: &BTreeMap<String, Vec<u8>>,
) -> Result<TemplateExecution, ExecError> {
    validate_template_manifest(manifest, source)?;
    let bindings = bindings_from_inputs(inputs)?;
    let rendered = TemplateProcessor::new()
        .render(&manifest.name, source, &bindings)
        .map_err(|err| ExecError::Program(format!("template render: {err}")))?;
    let (outputs, logs) = parse_output_mapping(&rendered.html)?;
    Ok(TemplateExecution {
        outputs,
        logs,
        source_digest: rendered.plan.source_digest,
        ast_digest: rendered.plan.ast_digest,
        host_calls: rendered
            .plan
            .host_calls
            .into_iter()
            .map(|call| call.target)
            .collect(),
        diagnostics: rendered
            .plan
            .diagnostics
            .into_iter()
            .map(|diag| {
                let severity = match diag.severity {
                    DiagnosticSeverity::Warning => "warning",
                };
                format!("{severity}:{}:{}", diag.code, diag.message)
            })
            .collect(),
    })
}

impl TemplateExecution {
    pub fn to_cbor(&self) -> Result<Vec<u8>, loom_codec::CodecError> {
        loom_codec::encode(&CborValue::Map(vec![
            text_pair(
                "schema",
                CborValue::Text(TEMPLATE_RESULT_SCHEMA.to_string()),
            ),
            text_pair("outputs", json_object_to_cbor_map(&self.outputs)),
            text_pair(
                "logs",
                CborValue::Array(self.logs.iter().cloned().map(CborValue::Text).collect()),
            ),
            text_pair(
                "source_digest",
                CborValue::Bytes(self.source_digest.bytes().to_vec()),
            ),
            text_pair(
                "ast_digest",
                CborValue::Bytes(self.ast_digest.bytes().to_vec()),
            ),
            text_pair(
                "host_calls",
                CborValue::Array(
                    self.host_calls
                        .iter()
                        .cloned()
                        .map(CborValue::Text)
                        .collect(),
                ),
            ),
            text_pair(
                "diagnostics",
                CborValue::Array(
                    self.diagnostics
                        .iter()
                        .cloned()
                        .map(CborValue::Text)
                        .collect(),
                ),
            ),
        ]))
    }
}

fn validate_template_manifest(manifest: &Manifest, source: &str) -> Result<(), ExecError> {
    if manifest.engine != "template" || manifest.abi_version != 1 || manifest.entry != "render" {
        return Err(ExecError::Program(
            "template manifest must target template abi v1 entry render".to_string(),
        ));
    }
    let body = Object::Blob(source.as_bytes().to_vec()).digest();
    if manifest.body != body {
        return Err(ExecError::Program(
            "template source does not match manifest digest".to_string(),
        ));
    }
    if !manifest.grants.is_grantable() {
        return Err(ExecError::Denied(
            "template manifest declares a non-grantable facet".to_string(),
        ));
    }
    Ok(())
}

fn bindings_from_inputs(inputs: &BTreeMap<String, Vec<u8>>) -> Result<TemplateBindings, ExecError> {
    let mut bindings = TemplateBindings::default();
    for (name, value) in inputs {
        if let Some(key) = name.strip_prefix("loom.") {
            bindings = bindings.with_loom_value(key, json_input(name, value)?);
        } else if let Some(key) = name.strip_prefix("program.") {
            bindings = bindings.with_program_output(key, utf8_input(name, value)?);
        } else if name == "meta" {
            bindings = bindings.with_meta(json_input(name, value)?);
        } else if let Some(key) = name.strip_prefix("request.") {
            bindings
                .request
                .insert(key.to_string(), utf8_input(name, value)?);
        } else if let Some(key) = name.strip_prefix("response.") {
            bindings
                .response
                .insert(key.to_string(), utf8_input(name, value)?);
        } else if let Some(key) = name.strip_prefix("session.") {
            bindings
                .session
                .insert(key.to_string(), utf8_input(name, value)?);
        } else if let Some(key) = name.strip_prefix("cookie.") {
            bindings
                .cookie
                .insert(key.to_string(), utf8_input(name, value)?);
        } else {
            return Err(ExecError::Program(format!(
                "unsupported template input binding {name}"
            )));
        }
    }
    Ok(bindings)
}

fn parse_output_mapping(
    rendered: &str,
) -> Result<(BTreeMap<String, serde_json::Value>, Vec<String>), ExecError> {
    let value: serde_json::Value = serde_json::from_str(rendered)
        .map_err(|err| ExecError::Program(format!("template output must be JSON: {err}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| ExecError::Program("template output must be a JSON object".to_string()))?;
    let outputs = object
        .get("outputs")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            ExecError::Program("template output must contain an outputs object".to_string())
        })?
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    let logs = match object.get("logs") {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| ExecError::Program("template logs must be strings".to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => {
            return Err(ExecError::Program(
                "template logs must be an array".to_string(),
            ));
        }
        None => Vec::new(),
    };
    Ok((outputs, logs))
}

fn json_input(name: &str, value: &[u8]) -> Result<serde_json::Value, ExecError> {
    serde_json::from_slice(value)
        .map_err(|err| ExecError::Program(format!("template input {name} must be JSON: {err}")))
}

fn utf8_input(name: &str, value: &[u8]) -> Result<String, ExecError> {
    String::from_utf8(value.to_vec())
        .map_err(|_| ExecError::Program(format!("template input {name} must be UTF-8")))
}

fn json_object_to_cbor_map(values: &BTreeMap<String, serde_json::Value>) -> CborValue {
    CborValue::Map(
        values
            .iter()
            .map(|(key, value)| (CborValue::Text(key.clone()), json_to_cbor(value)))
            .collect(),
    )
}

fn json_to_cbor(value: &serde_json::Value) -> CborValue {
    match value {
        serde_json::Value::Null => CborValue::Null,
        serde_json::Value::Bool(value) => CborValue::Bool(*value),
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_u64() {
                CborValue::Uint(value)
            } else if let Some(value) = number.as_i64() {
                CborValue::int(value)
            } else {
                CborValue::Float(number.as_f64().expect("JSON number is finite"))
            }
        }
        serde_json::Value::String(value) => CborValue::Text(value.clone()),
        serde_json::Value::Array(items) => {
            CborValue::Array(items.iter().map(json_to_cbor).collect())
        }
        serde_json::Value::Object(values) => {
            let mut sorted = values.iter().collect::<Vec<_>>();
            sorted.sort_by_key(|(key, _)| *key);
            CborValue::Map(
                sorted
                    .into_iter()
                    .map(|(key, value)| (CborValue::Text(key.clone()), json_to_cbor(value)))
                    .collect(),
            )
        }
    }
}

fn text_pair(key: &str, value: CborValue) -> (CborValue, CborValue) {
    (CborValue::Text(key.to_string()), value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GrantSet;

    #[test]
    fn template_program_renders_canonical_output_mapping() {
        let source = r#"{"outputs":{"html":{{ loom.page | tojson }},"summary":{{ loom.program("summary") | tojson }},"path":{{ request.path | tojson }}},"logs":["rendered"]}"#;
        let manifest = Manifest::for_template("page-card", source, GrantSet::all_facets());
        let mut inputs = BTreeMap::new();
        inputs.insert(
            "loom.page".to_string(),
            br#""<h1>Ready</h1>""#.as_slice().to_vec(),
        );
        inputs.insert("program.summary".to_string(), b"ok".to_vec());
        inputs.insert("request.path".to_string(), b"/pages/ready".to_vec());

        let rendered = render_template_program(&manifest, source, &inputs).unwrap();

        assert_eq!(
            rendered.outputs.get("html"),
            Some(&serde_json::Value::String("<h1>Ready</h1>".to_string()))
        );
        assert_eq!(
            rendered.outputs.get("summary"),
            Some(&serde_json::Value::String("ok".to_string()))
        );
        assert_eq!(rendered.logs, vec!["rendered"]);
        assert!(rendered.host_calls.contains(&"summary".to_string()));
        let cbor = loom_codec::decode(&rendered.to_cbor().unwrap()).unwrap();
        let CborValue::Map(fields) = cbor else {
            panic!("template result must be a map");
        };
        assert!(fields.iter().any(|(key, value)| {
            matches!((key, value), (CborValue::Text(key), CborValue::Text(value)) if key == "schema" && value == TEMPLATE_RESULT_SCHEMA)
        }));
    }

    #[test]
    fn template_program_requires_outputs_object() {
        let source = r#"{"html":"nope"}"#;
        let manifest = Manifest::for_template("bad", source, GrantSet::all_facets());
        let err = render_template_program(&manifest, source, &BTreeMap::new()).unwrap_err();
        assert!(matches!(err, ExecError::Program(message) if message.contains("outputs object")));
    }

    #[test]
    fn template_program_rejects_source_digest_mismatch() {
        let source = r#"{"outputs":{"x":"ok"}}"#;
        let manifest = Manifest::for_template("bad", "different", GrantSet::all_facets());
        let err = render_template_program(&manifest, source, &BTreeMap::new()).unwrap_err();
        assert!(matches!(err, ExecError::Program(message) if message.contains("manifest digest")));
    }
}
