//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

//! `loom exec` is the user-facing projection of the program-execution (`exec`) facet. It speaks the same
//! canonical `loom.exec.request.v1` / `loom.exec.result.v1` CBOR contract as the IDL, C ABI, bindings,
//! and hosted surfaces: `run` pipes a prebuilt request through `loom_compute::execute_cbor`, `inspect`
//! decodes a request for review without executing, and `apply` merges a gated proposal fork. Reports are
//! rendered as JSON with the crate's hand-rolled helpers, so there is no second place that knows how to
//! assemble or interpret an exec request.

use super::*;

use loom_codec::{Value, decode, encode};

pub(crate) fn run_exec_cmd(action: ExecCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        ExecCmd::Run {
            store,
            request,
            input,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let raw =
                std::fs::read(&request).map_err(|e| format!("read exec request {request}: {e}"))?;
            let overlays = parse_inputs(&input)?;
            let request_bytes = if overlays.is_empty() {
                raw
            } else {
                overlay_inputs(&raw, &overlays)?
            };
            let response = loom_compute::execute_cbor(&mut *loom, &request_bytes)
                .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            let value = decode(&response).map_err(|e| format!("decode exec result: {e}"))?;
            println!("{}", cbor_to_json(&value));
            Ok(())
        }
        ExecCmd::Inspect { request } => {
            let raw =
                std::fs::read(&request).map_err(|e| format!("read exec request {request}: {e}"))?;
            let value = decode(&raw).map_err(|e| format!("decode exec request: {e}"))?;
            println!("{}", cbor_to_json(&value));
            Ok(())
        }
        ExecCmd::Apply {
            store,
            workspace,
            base,
            fork,
            author,
            timestamp_ms,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let outcome = loom_compute::apply(&mut *loom, ns, &base, &fork, &author, timestamp_ms)
                .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            println!("{}", merge_outcome_json(&outcome));
            Ok(())
        }
    }
}

/// Parse repeated `--input name=@file` specifications into named blobs read from disk.
fn parse_inputs(specs: &[String]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = Vec::with_capacity(specs.len());
    for spec in specs {
        let (name, rhs) = spec
            .split_once('=')
            .ok_or_else(|| format!("--input must be `name=@file`, got {spec:?}"))?;
        let path = rhs
            .strip_prefix('@')
            .ok_or_else(|| format!("--input value must be `@file`, got {rhs:?}"))?;
        let data = std::fs::read(path).map_err(|e| format!("read input {path}: {e}"))?;
        out.push((name.to_string(), data));
    }
    Ok(out)
}

/// Overlay named input blobs onto every step of a canonical request, then re-encode. Operates on the
/// generic CBOR value tree (not a typed request builder), so it adds no parallel request-assembly
/// surface; the encoder re-canonicalizes map key order.
fn overlay_inputs(bytes: &[u8], overlays: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut value = decode(bytes).map_err(|e| format!("decode exec request: {e}"))?;
    let Value::Map(top) = &mut value else {
        return Err("exec request must be a CBOR map".to_string());
    };
    let Some(steps) = map_get_mut(top, "steps") else {
        return Err("exec request is missing `steps`".to_string());
    };
    let Value::Array(steps) = steps else {
        return Err("exec request `steps` must be an array".to_string());
    };
    for step in steps.iter_mut() {
        let Value::Map(step_map) = step else {
            return Err("each exec step must be a CBOR map".to_string());
        };
        if map_get_mut(step_map, "inputs").is_none() {
            step_map.push((Value::Text("inputs".to_string()), Value::Map(Vec::new())));
        }
        let Some(Value::Map(inputs)) = map_get_mut(step_map, "inputs") else {
            return Err("exec step `inputs` must be a CBOR map".to_string());
        };
        for (name, data) in overlays {
            set_text_key(inputs, name, Value::Bytes(data.clone()));
        }
    }
    encode(&value).map_err(|e| format!("re-encode exec request: {e}"))
}

fn map_get_mut<'a>(entries: &'a mut [(Value, Value)], key: &str) -> Option<&'a mut Value> {
    entries.iter_mut().find_map(|(k, v)| match k {
        Value::Text(found) if found == key => Some(v),
        _ => None,
    })
}

fn set_text_key(entries: &mut Vec<(Value, Value)>, key: &str, val: Value) {
    for (k, v) in entries.iter_mut() {
        if matches!(k, Value::Text(found) if found == key) {
            *v = val;
            return;
        }
    }
    entries.push((Value::Text(key.to_string()), val));
}

fn merge_outcome_json(outcome: &MergeOutcome) -> String {
    format!("{{\"outcome\":{}}}", json_string(&format!("{outcome:?}")))
}

/// Render a canonical CBOR value as JSON: text is escaped, byte strings become lowercase hex, maps keyed
/// by text/integers, and negative integers keep their sign. One generic converter serves both the
/// request (`inspect`) and result (`run`) envelopes, so no field-by-field schema is duplicated here.
fn cbor_to_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Uint(u) => u.to_string(),
        Value::Nint(n) => format!("-{}", i128::from(*n) + 1),
        Value::Float(f) => f.to_string(),
        Value::Bytes(b) => json_string(&hex_encode(b)),
        Value::Text(t) => json_string(t),
        Value::Array(items) => {
            let mut out = String::from("[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&cbor_to_json(item));
            }
            out.push(']');
            out
        }
        Value::Map(pairs) => {
            let mut out = String::from("{");
            for (i, (k, v)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let key = match k {
                    Value::Text(t) => t.clone(),
                    Value::Bytes(b) => hex_encode(b),
                    Value::Uint(u) => u.to_string(),
                    Value::Nint(n) => format!("-{}", i128::from(*n) + 1),
                    other => format!("{other:?}"),
                };
                out.push_str(&json_string(&key));
                out.push(':');
                out.push_str(&cbor_to_json(v));
            }
            out.push('}');
            out
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
