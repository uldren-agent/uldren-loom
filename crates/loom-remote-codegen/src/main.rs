//! Emit the Loom Remote Protocol artifacts from `idl/loom.idl`.
//!
//! The IDL is the single source of truth. This tool parses every `interface` block and
//! its methods and writes the committed files: the method registry and the `LoomApi` trait families in
//! `loom-remote-protocol`, and the `RemoteLoomClient` stubs in `loom-remote-client`. Output is formatted
//! through `rustfmt`, so
//! `cargo fmt --check` is a no-op on the generated files and `--check` here detects any drift from the
//! IDL. Regenerate with `cargo run -p uldren-loom-remote-codegen`.
//!
//! Licensed under BUSL-1.1.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

#[derive(Debug)]
struct Method {
    interface: String,
    name: String,
    ret: String,
    args: Vec<(String, String)>,
    effect: OperationEffect,
}

#[derive(Debug)]
struct StructDef {
    name: String,
    fields: Vec<(String, String)>,
}

#[derive(Debug)]
struct EnumDef {
    name: String,
    values: Vec<String>,
}

#[derive(Debug, Default)]
struct TypeModel {
    structs: Vec<StructDef>,
    enums: Vec<EnumDef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperationEffect {
    Read,
    Mutation,
    Control,
}

impl OperationEffect {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "read" => Ok(Self::Read),
            "mutation" => Ok(Self::Mutation),
            "control" => Ok(Self::Control),
            other => Err(format!("unknown operation effect {other:?}")),
        }
    }

    fn generated_variant(self) -> &'static str {
        match self {
            Self::Read => "Read",
            Self::Mutation => "Mutation",
            Self::Control => "Control",
        }
    }
}

fn pascal_identifier(value: &str) -> String {
    let mut out = String::new();
    let mut uppercase = true;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if uppercase {
                out.push(ch.to_ascii_uppercase());
                uppercase = false;
            } else {
                out.push(ch);
            }
        } else {
            uppercase = true;
        }
    }
    out
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn parse(idl: &str) -> Vec<Method> {
    parse_methods(idl).expect("IDL parser rejected input")
}

fn parse_type_model(idl: &str) -> Result<TypeModel, String> {
    let mut struct_names = std::collections::BTreeSet::new();
    let mut enum_names = std::collections::BTreeSet::new();
    let mut model = TypeModel::default();
    let mut lines = idl.lines().enumerate().peekable();
    while let Some((index, raw)) = lines.next() {
        let line_no = index + 1;
        let line = strip_comment(raw).trim();
        if let Some(rest) = line.strip_prefix("struct ") {
            let Some((name, inline_body)) = parse_block_header(rest) else {
                return Err(format!("struct at line {line_no} has no name"));
            };
            if inline_body.is_some() {
                return Err(format!(
                    "struct {name} at line {line_no} must use a multiline body"
                ));
            }
            if !struct_names.insert(name.clone()) {
                return Err(format!("duplicate struct {name} at line {line_no}"));
            }
            let mut fields = Vec::new();
            let mut field_names = std::collections::BTreeSet::new();
            loop {
                let Some((field_index, raw_field)) = lines.next() else {
                    return Err(format!("struct {name} is missing closing brace"));
                };
                let field_line_no = field_index + 1;
                let field = strip_comment(raw_field).trim();
                if field.is_empty() {
                    continue;
                }
                if field == "}" {
                    break;
                }
                if !field.ends_with(';') {
                    return Err(format!(
                        "field at line {field_line_no} is missing semicolon"
                    ));
                }
                let field = field.trim_end_matches(';').trim();
                let mut tokens: Vec<&str> = field.split_whitespace().collect();
                let field_name = tokens
                    .pop()
                    .ok_or_else(|| format!("field at line {field_line_no} has no name"))?
                    .to_string();
                if tokens.is_empty() {
                    return Err(format!(
                        "field {field_name} at line {field_line_no} has no type"
                    ));
                }
                if !field_names.insert(field_name.clone()) {
                    return Err(format!(
                        "duplicate field {field_name} in struct {name} at line {field_line_no}"
                    ));
                }
                fields.push((tokens.join(" "), field_name));
            }
            model.structs.push(StructDef { name, fields });
        } else if let Some(rest) = line.strip_prefix("enum ") {
            let Some((name, inline_body)) = parse_block_header(rest) else {
                return Err(format!("enum at line {line_no} has no name"));
            };
            if !enum_names.insert(name.clone()) {
                return Err(format!("duplicate enum {name} at line {line_no}"));
            }
            let mut values = Vec::new();
            let mut enum_values = std::collections::BTreeSet::new();
            if let Some(inline_body) = inline_body {
                for value in inline_body.split(',') {
                    let value = value.trim();
                    if value.is_empty() {
                        continue;
                    }
                    if value.contains(char::is_whitespace) {
                        return Err(format!("enum value at line {line_no} contains whitespace"));
                    }
                    if !enum_values.insert(value.to_string()) {
                        return Err(format!(
                            "duplicate enum value {value} in enum {name} at line {line_no}"
                        ));
                    }
                    values.push(value.to_string());
                }
                if values.is_empty() {
                    return Err(format!("enum {name} at line {line_no} has no values"));
                }
                model.enums.push(EnumDef { name, values });
                continue;
            }
            loop {
                let Some((value_index, raw_value)) = lines.next() else {
                    return Err(format!("enum {name} is missing closing brace"));
                };
                let value_line_no = value_index + 1;
                let value = strip_comment(raw_value)
                    .trim()
                    .trim_end_matches(',')
                    .trim()
                    .to_string();
                if value.is_empty() {
                    continue;
                }
                if value == "}" {
                    break;
                }
                if value.contains(char::is_whitespace) {
                    return Err(format!(
                        "enum value at line {value_line_no} contains whitespace"
                    ));
                }
                if !enum_values.insert(value.clone()) {
                    return Err(format!(
                        "duplicate enum value {value} in enum {name} at line {value_line_no}"
                    ));
                }
                values.push(value);
            }
            if values.is_empty() {
                return Err(format!("enum {name} at line {line_no} has no values"));
            }
            model.enums.push(EnumDef { name, values });
        }
    }
    Ok(model)
}

fn parse_block_header(rest: &str) -> Option<(String, Option<String>)> {
    let (name, after_name) = rest
        .split_once(char::is_whitespace)
        .map_or((rest, ""), |(name, after_name)| (name, after_name));
    let name = name.trim_end_matches('{').trim();
    if name.is_empty() {
        return None;
    }
    let after_name = after_name.trim();
    if let Some(body) = after_name
        .strip_prefix('{')
        .and_then(|body| body.strip_suffix('}'))
    {
        return Some((name.to_string(), Some(body.trim().to_string())));
    }
    Some((name.to_string(), None))
}

fn strip_comment(line: &str) -> &str {
    line.split_once("//").map_or(line, |(before, _)| before)
}

fn parse_methods(idl: &str) -> Result<Vec<Method>, String> {
    let mut methods = Vec::new();
    let mut interface: Option<String> = None;
    let mut buffer = String::new();
    let mut pending_effect: Option<OperationEffect> = None;
    for (index, raw) in idl.lines().enumerate() {
        let line_no = index + 1;
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("interface ") {
            if pending_effect.is_some() {
                return Err(format!(
                    "effect annotation at line {line_no} was not followed by a method"
                ));
            }
            interface = rest.split_whitespace().next().map(str::to_string);
            buffer.clear();
            continue;
        }
        let Some(current) = interface.as_deref() else {
            if line.starts_with("@effect(") {
                return Err(format!(
                    "effect annotation at line {line_no} appears outside an interface"
                ));
            }
            continue;
        };
        if line == "}" {
            if pending_effect.is_some() {
                return Err(format!(
                    "effect annotation at line {line_no} was not followed by a method"
                ));
            }
            interface = None;
            buffer.clear();
            continue;
        }
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if line.starts_with("@effect(") {
            if !buffer.is_empty() {
                return Err(format!(
                    "effect annotation at line {line_no} appears inside a method signature"
                ));
            }
            if pending_effect.is_some() {
                return Err(format!("duplicate effect annotation at line {line_no}"));
            }
            let Some(value) = line
                .strip_prefix("@effect(")
                .and_then(|rest| rest.strip_suffix(')'))
            else {
                return Err(format!("malformed effect annotation at line {line_no}"));
            };
            pending_effect = Some(OperationEffect::parse(value)?);
            continue;
        }
        if !buffer.is_empty() {
            buffer.push(' ');
        }
        buffer.push_str(line);
        if buffer.trim_end().ends_with(");") {
            let effect = pending_effect
                .take()
                .ok_or_else(|| format!("missing effect annotation at line {line_no}"))?;
            let method = parse_signature(current, &buffer, effect)
                .ok_or_else(|| format!("invalid method signature ending at line {line_no}"))?;
            methods.push(method);
            buffer.clear();
        }
    }
    if pending_effect.is_some() {
        return Err("effect annotation at end of file was not followed by a method".to_string());
    }
    Ok(methods)
}

fn parse_signature(interface: &str, signature: &str, effect: OperationEffect) -> Option<Method> {
    let signature = signature.trim().trim_end_matches(';').trim();
    let open = signature.find('(')?;
    let close = signature.rfind(')')?;
    let head = signature[..open].trim();
    let params = &signature[open + 1..close];

    let mut head_tokens: Vec<&str> = head.split_whitespace().collect();
    let name = head_tokens.pop()?.to_string();
    let ret = head_tokens.join(" ");
    let ret = if ret.is_empty() {
        "void".to_string()
    } else {
        ret
    };

    let mut args = Vec::new();
    for param in params.split(',') {
        let param = param.trim();
        if param.is_empty() {
            continue;
        }
        let mut tokens: Vec<&str> = param.split_whitespace().collect();
        let arg_name = tokens.pop()?.to_string();
        let arg_type = tokens.join(" ");
        args.push((arg_type, arg_name));
    }

    Some(Method {
        interface: interface.to_string(),
        name,
        ret,
        args,
        effect,
    })
}

// ---- registry emitter ---------------------------------------------------------------------------

fn render_registry(methods: &[Method], types: &TypeModel) -> Result<String, String> {
    let mut out = String::new();
    out.push_str("//! Generated by `uldren-loom-remote-codegen` from `idl/loom.idl`. Do not edit by hand.\n//!\n//! Regenerate with `cargo run -p uldren-loom-remote-codegen`.\n\n");
    out.push_str("/// Typed identifier for one IDL method generated from `idl/loom.idl`.\n");
    out.push_str("#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]\n");
    out.push_str("pub enum GeneratedOperationId {\n");
    for method in methods {
        out.push_str("    ");
        out.push_str(&pascal_identifier(&format!(
            "{}_{}",
            method.interface, method.name
        )));
        out.push_str(",\n");
    }
    out.push_str("}\n\n");
    out.push_str("impl GeneratedOperationId {\n");
    out.push_str(
        "    /// The IDL `(interface, method)` pair this generated operation identifies.\n",
    );
    out.push_str("    pub const fn projection(self) -> (&'static str, &'static str) {\n");
    out.push_str("        match self {\n");
    for method in methods {
        out.push_str("            Self::");
        out.push_str(&pascal_identifier(&format!(
            "{}_{}",
            method.interface, method.name
        )));
        out.push_str(" => (");
        out.push_str(&format!("{:?}, {:?}", method.interface, method.name));
        out.push_str("),\n");
    }
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
    out.push_str("/// The declared side-effect class for one generated IDL method.\n");
    out.push_str("#[derive(Clone, Copy, PartialEq, Eq, Debug)]\n");
    out.push_str("pub enum GeneratedOperationEffect {\n");
    out.push_str(
        "    /// Reads semantic state without publishing a durable mutation.\n    Read,\n",
    );
    out.push_str(
        "    /// Publishes semantic state or durable operational metadata.\n    Mutation,\n",
    );
    out.push_str("    /// Controls runtime/session/handle state without ordinary data publication.\n    Control,\n");
    out.push_str("}\n\n");
    out.push_str("/// One IDL method signature, generated from `idl/loom.idl`.\n");
    out.push_str("#[derive(Debug, Clone, Copy)]\n");
    out.push_str("pub struct MethodSig {\n");
    out.push_str(
        "    /// The generated operation identifier.\n    pub operation: GeneratedOperationId,\n",
    );
    out.push_str("    /// The IDL interface name.\n    pub interface: &'static str,\n");
    out.push_str("    /// The IDL method name.\n    pub method: &'static str,\n");
    out.push_str("    /// The argument `(type, name)` pairs in declaration order.\n    pub args: &'static [(&'static str, &'static str)],\n");
    out.push_str("    /// The argument `(type, name)` pairs after removing the leading generated session handle, when present.\n    pub args_without_handle: &'static [(&'static str, &'static str)],\n");
    out.push_str("    /// The IDL return type.\n    pub ret: &'static str,\n");
    out.push_str("    /// The declared operation side-effect class.\n    pub effect: GeneratedOperationEffect,\n");
    out.push_str(
        "    /// Whether generated transports attach an automatic idempotency key.\n    pub requires_idempotency_key: bool,\n",
    );
    out.push_str("    /// JSON Schema for the full generated IDL request object.\n    pub request_json_schema: &'static str,\n");
    out.push_str("    /// JSON Schema for the generated IDL response value.\n    pub response_json_schema: &'static str,\n");
    out.push_str("}\n\n");
    out.push_str("/// Every method declared in `idl/loom.idl`, in file order.\n");
    out.push_str("pub const METHODS: &[MethodSig] = &[\n");
    for method in methods {
        let operation = pascal_identifier(&format!("{}_{}", method.interface, method.name));
        let args = method
            .args
            .iter()
            .map(|(ty, name)| format!("({ty:?}, {name:?})"))
            .collect::<Vec<_>>()
            .join(", ");
        let args_without_handle = args_without_handle(&method.args)
            .iter()
            .map(|(ty, name)| format!("({ty:?}, {name:?})"))
            .collect::<Vec<_>>()
            .join(", ");
        let request_schema = operation_request_schema(method, types).map_err(|err| {
            format!(
                "{}.{} request schema generation failed: {err}",
                method.interface, method.name
            )
        })?;
        let response_schema = operation_response_schema(method, types).map_err(|err| {
            format!(
                "{}.{} response schema generation failed: {err}",
                method.interface, method.name
            )
        })?;
        out.push_str(&format!(
            "    MethodSig {{ operation: GeneratedOperationId::{operation}, interface: {:?}, method: {:?}, args: &[{}], args_without_handle: &[{}], ret: {:?}, effect: GeneratedOperationEffect::{}, requires_idempotency_key: {}, request_json_schema: {:?}, response_json_schema: {:?} }},\n",
            method.interface,
            method.name,
            args,
            args_without_handle,
            method.ret,
            method.effect.generated_variant(),
            requires_idempotency_key(&method.interface, &method.name),
            request_schema,
            response_schema,
        ));
    }
    out.push_str("];\n");
    Ok(out)
}

fn args_without_handle(args: &[(String, String)]) -> &[(String, String)] {
    if args
        .first()
        .is_some_and(|(idl_type, name)| idl_type == "LoomSession" && name == "handle")
    {
        &args[1..]
    } else {
        args
    }
}

fn operation_request_schema(method: &Method, types: &TypeModel) -> Result<String, String> {
    object_schema_for_fields(&method.args, types)
}

fn operation_response_schema(method: &Method, types: &TypeModel) -> Result<String, String> {
    schema_document_for_type(&method.ret, types)
}

fn object_schema_for_fields(
    fields: &[(String, String)],
    types: &TypeModel,
) -> Result<String, String> {
    let mut referenced = std::collections::BTreeSet::new();
    let mut properties = Vec::new();
    let mut required = Vec::new();
    for (idl_type, name) in fields {
        collect_referenced_types(idl_type, types, &mut referenced)?;
        properties.push(format!("{:?}:{}", name, schema_for_type(idl_type, types)?));
        required.push(format!("{name:?}"));
    }
    let mut schema = String::from(
        "{\"$schema\":\"https://json-schema.org/draft/2020-12/schema\",\"type\":\"object\",\"properties\":{",
    );
    schema.push_str(&properties.join(","));
    schema.push_str("},\"required\":[");
    schema.push_str(&required.join(","));
    schema.push_str("],\"additionalProperties\":false");
    append_definitions(&mut schema, types, &referenced)?;
    schema.push('}');
    Ok(schema)
}

fn schema_document_for_type(idl_type: &str, types: &TypeModel) -> Result<String, String> {
    let mut referenced = std::collections::BTreeSet::new();
    collect_referenced_types(idl_type, types, &mut referenced)?;
    let mut schema = String::from("{\"$schema\":\"https://json-schema.org/draft/2020-12/schema\",");
    let type_schema = schema_for_type(idl_type, types)?;
    schema.push_str(&type_schema[1..]);
    if schema.ends_with('}') {
        schema.pop();
    }
    append_definitions(&mut schema, types, &referenced)?;
    schema.push('}');
    Ok(schema)
}

fn append_definitions(
    schema: &mut String,
    types: &TypeModel,
    referenced: &std::collections::BTreeSet<String>,
) -> Result<(), String> {
    if referenced.is_empty() {
        return Ok(());
    }
    let definitions = referenced
        .iter()
        .map(|name| schema_definition(name, types).map(|schema| format!("{name:?}:{schema}")))
        .collect::<Result<Vec<_>, _>>()?;
    schema.push_str(",\"$defs\":{");
    schema.push_str(&definitions.join(","));
    schema.push('}');
    Ok(())
}

fn schema_definition(name: &str, types: &TypeModel) -> Result<String, String> {
    if let Some(record) = types.structs.iter().find(|record| record.name == name) {
        let properties = record
            .fields
            .iter()
            .map(|(idl_type, field)| {
                schema_for_type(idl_type, types).map(|schema| format!("{field:?}:{schema}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let required = record
            .fields
            .iter()
            .map(|(_, field)| format!("{field:?}"))
            .collect::<Vec<_>>();
        return Ok(format!(
            "{{\"type\":\"object\",\"properties\":{{{}}},\"required\":[{}],\"additionalProperties\":false}}",
            properties.join(","),
            required.join(",")
        ));
    }
    if let Some(enumeration) = types
        .enums
        .iter()
        .find(|enumeration| enumeration.name == name)
    {
        let values = enumeration
            .values
            .iter()
            .map(|value| format!("{value:?}"))
            .collect::<Vec<_>>();
        return Ok(format!(
            "{{\"type\":\"string\",\"enum\":[{}]}}",
            values.join(",")
        ));
    }
    Err(format!("unknown IDL type {name}"))
}

fn schema_for_type(idl_type: &str, types: &TypeModel) -> Result<String, String> {
    let idl_type = idl_type.trim();
    if let Some(inner) = strip_generic(idl_type, "optional") {
        return Ok(format!(
            "{{\"anyOf\":[{},{{\"type\":\"null\"}}]}}",
            schema_for_type(&inner, types)?
        ));
    }
    if let Some(inner) = strip_generic(idl_type, "list") {
        return Ok(format!(
            "{{\"type\":\"array\",\"items\":{}}}",
            schema_for_type(&inner, types)?
        ));
    }
    if let Some(inner) = strip_generic(idl_type, "stream") {
        return Ok(format!(
            "{{\"type\":\"array\",\"items\":{},\"x-loom-stream\":true}}",
            schema_for_type(&inner, types)?
        ));
    }
    Ok(match idl_type {
        "void" => "{\"type\":\"null\"}".to_string(),
        "bool" => "{\"type\":\"boolean\"}".to_string(),
        "string" => "{\"type\":\"string\"}".to_string(),
        "bytes" => {
            "{\"type\":\"string\",\"contentEncoding\":\"base64\",\"x-loom-bytes\":true}".to_string()
        }
        "u8" => "{\"type\":\"integer\",\"minimum\":0,\"maximum\":255}".to_string(),
        "i32" | "i64" => "{\"type\":\"integer\"}".to_string(),
        "u32" | "u64" => "{\"type\":\"integer\",\"minimum\":0}".to_string(),
        "f64" => "{\"type\":\"number\"}".to_string(),
        "Uuid" => "{\"type\":\"string\",\"format\":\"uuid\"}".to_string(),
        "LoomSession" | "SqlSession" | "SqlBatch" | "RowIter" | "Task" | "ResultView" => {
            format!("{{\"type\":\"string\",\"x-loom-handle\":{idl_type:?}}}")
        }
        named
            if types
                .structs
                .iter()
                .any(|record| record.name.as_str() == named)
                || types
                    .enums
                    .iter()
                    .any(|enumeration| enumeration.name.as_str() == named) =>
        {
            format!("{{\"$ref\":\"#/$defs/{named}\"}}")
        }
        named => return Err(format!("unknown IDL type {named}")),
    })
}

fn strip_generic(idl_type: &str, generic: &str) -> Option<String> {
    if generic == "optional" {
        if let Some(inner) = idl_type.strip_suffix('?') {
            return Some(inner.trim().to_string());
        }
    }
    let rest = idl_type.strip_prefix(generic)?;
    if let Some(inner) = rest.strip_prefix('<') {
        return inner.strip_suffix('>').map(str::trim).map(str::to_string);
    }
    rest.strip_prefix(char::is_whitespace)
        .map(str::trim)
        .filter(|inner| !inner.is_empty())
        .map(str::to_string)
}

fn collect_referenced_types(
    idl_type: &str,
    types: &TypeModel,
    out: &mut std::collections::BTreeSet<String>,
) -> Result<(), String> {
    let idl_type = idl_type.trim();
    if let Some(inner) = strip_generic(idl_type, "optional")
        .or_else(|| strip_generic(idl_type, "list"))
        .or_else(|| strip_generic(idl_type, "stream"))
    {
        return collect_referenced_types(&inner, types, out);
    }
    if is_schema_primitive(idl_type) || is_handle_type(idl_type) {
        return Ok(());
    }
    let is_known = types.structs.iter().any(|record| record.name == idl_type)
        || types
            .enums
            .iter()
            .any(|enumeration| enumeration.name == idl_type);
    if !is_known {
        return Err(format!("unknown IDL type {idl_type}"));
    }
    if !out.insert(idl_type.to_string()) {
        return Ok(());
    }
    if let Some(record) = types.structs.iter().find(|record| record.name == idl_type) {
        for (field_type, _) in &record.fields {
            collect_referenced_types(field_type, types, out)?;
        }
    }
    Ok(())
}

fn is_schema_primitive(idl_type: &str) -> bool {
    matches!(
        idl_type,
        "void"
            | "bool"
            | "string"
            | "bytes"
            | "u8"
            | "i32"
            | "i64"
            | "u32"
            | "u64"
            | "f64"
            | "Uuid"
    )
}

fn is_handle_type(idl_type: &str) -> bool {
    matches!(
        idl_type,
        "LoomSession" | "SqlSession" | "SqlBatch" | "RowIter" | "Task" | "ResultView"
    )
}

// ---- trait API emitter --------------------------------------------------------------------------

/// Methods that never perform I/O (transport `Lo`): they are plain `fn`, not `async`.
fn is_sync(interface: &str, method: &str) -> bool {
    matches!(
        (interface, method),
        ("ResultViews", _)
            | ("Diagnostics", "result_to_json")
            | ("Diagnostics", "result_to_bridge_json")
            | ("Diagnostics", "last_error")
            | ("Store", "blob_digest")
            | ("Tasks", "iter_free")
            | ("Tasks", "task_free")
    )
}

/// The methods the parity matrix classifies `key` (specs/0067 §12): mutating operations that are not
/// naturally idempotent, so §6 requires an idempotency key. Generated clients auto-attach a key for these
/// so a transport retry cannot double-apply them; the lower-level `RemoteLoomClient::call` still accepts a
/// caller-supplied key for durable app-level retries. This list is kept identical to the `| key |` rows of
/// the §12 matrix; `sql_exec_async` is on `Tasks`, and `append` is distinct on Columnar/Ledger/Queue.
fn requires_idempotency_key(interface: &str, method: &str) -> bool {
    matches!(
        (interface, method),
        ("KeySource", "key_add_wrap_keyed")
            | ("KeySource", "key_add_wrap_with_kek")
            | ("KeySource", "key_remove_wrap")
            | ("Exec", "apply_cbor")
            | ("Exec", "exec_cbor")
            | ("Sql", "sql_exec_result")
            | ("Identity", "identity_add_principal")
            | ("Identity", "identity_create_external_credential")
            | ("Identity", "identity_add_public_key")
            | ("Identity", "identity_create_app_credential")
            | ("Identity", "identity_force_detach_authority_json")
            | ("Identity", "identity_replicate_authority_json")
            | ("Identity", "identity_configure_authority_replication_json")
            | ("Identity", "identity_remove_authority_replication_json")
            | ("Lifecycle", "lifecycle_define_standard_json")
            | ("Lifecycle", "lifecycle_define_json")
            | ("Lifecycle", "lifecycle_instantiate_json")
            | ("Lifecycle", "lifecycle_transition_json")
            | ("Refs", "refs_reconcile_json")
            | ("Audit", "audit_compact")
            | ("VersionControl", "merge")
            | ("VersionControl", "merge_continue")
            | ("VersionControl", "merge_async")
            | ("VersionControl", "squash")
            | ("FileSystem", "append_file")
            | ("Columnar", "append")
            | ("Ledger", "append")
            | ("Queue", "append")
            | ("Chat", "chat_create_channel_json")
            | ("Chat", "chat_rename_channel_json")
            | ("Chat", "chat_post_message_json")
            | ("Chat", "chat_post_message_bytes_json")
            | ("Chat", "chat_edit_message_json")
            | ("Chat", "chat_edit_message_bytes_json")
            | ("Chat", "chat_redact_message_json")
            | ("Chat", "chat_create_thread_json")
            | ("Chat", "chat_create_task_json")
            | ("Chat", "chat_claim_task_json")
            | ("Chat", "chat_complete_task_json")
            | ("Chat", "chat_invoke_agent_json")
            | ("Chat", "chat_invoke_agent_bytes_json")
            | ("Chat", "chat_agent_reply_json")
            | ("Chat", "chat_request_handoff_json")
            | ("Chat", "chat_add_reaction_json")
            | ("Chat", "chat_remove_reaction_json")
            | ("Chat", "chat_emoji_register_json")
            | ("Chat", "chat_emoji_unregister_json")
            | ("Chat", "chat_update_cursor_json")
            | ("Lanes", "create")
            | ("Lanes", "update")
            | ("Lanes", "ticket_add")
            | ("Lanes", "ticket_remove")
            | ("Sql", "sql_exec")
            | ("Sql", "sql_batch_exec")
            | ("Sql", "sql_batch_commit")
            | ("Sql", "sql_batch_commit_vcs")
            | ("Tasks", "sql_exec_async")
            | ("Transfer", "transfer_import_open")
            | ("Transfer", "transfer_import_write")
            | ("Transfer", "transfer_import_finish")
            | ("StoreAdmin", "store_policy_set")
            | ("StoreAdmin", "store_rekey")
            | ("StoreAdmin", "store_maintenance_policy_set")
            | ("StoreAdmin", "store_maintenance_run")
    )
}

#[cfg(test)]
mod tests {
    use super::{
        OperationEffect, object_schema_for_fields, parse, parse_methods, parse_type_model,
        pascal_identifier, render_registry, requires_idempotency_key, rustfmt,
        schema_document_for_type, schema_for_type,
    };

    #[test]
    fn mu6j_b2a_effect_annotations_attach_to_immediate_method() {
        let methods = parse_methods(
            r#"
            interface Files {
                @effect(read)
                // comment between annotation and method

                bytes read_file(LoomSession handle, string workspace, string path);

                @effect(mutation)
                void write_file(LoomSession handle, string workspace, string path, bytes body);

                @effect(control)
                void close(LoomSession handle);
            }
            "#,
        )
        .expect("valid effects");

        assert_eq!(methods.len(), 3);
        assert_eq!(methods[0].name, "read_file");
        assert_eq!(methods[0].effect, OperationEffect::Read);
        assert_eq!(methods[1].name, "write_file");
        assert_eq!(methods[1].effect, OperationEffect::Mutation);
        assert_eq!(methods[2].name, "close");
        assert_eq!(methods[2].effect, OperationEffect::Control);
    }

    #[test]
    fn mu6j_b2b_missing_effect_annotation_is_rejected() {
        let error = parse_methods(
            r#"
            interface Files {
                bytes read_file(LoomSession handle, string workspace, string path);
            }
            "#,
        )
        .expect_err("missing effect must fail after MU-6j-b2b");

        assert!(error.contains("missing effect annotation"), "{error}");
    }

    #[test]
    fn mu6j_b2b_production_idl_annotation_count_matches_method_count() {
        let idl = include_str!("../../../idl/loom.idl");
        let annotations = idl.matches("@effect(").count();
        let methods = parse(idl);

        assert_eq!(annotations, methods.len());
    }

    #[test]
    fn generated_registry_matches_current_idl() {
        let idl = include_str!("../../../idl/loom.idl");
        let generated = include_str!("../../loom-remote-protocol/src/generated.rs");
        let methods = parse(idl);
        let types = parse_type_model(idl).expect("IDL type model");

        assert_eq!(
            generated,
            rustfmt(&render_registry(&methods, &types).expect("registry render"))
        );
    }

    #[test]
    fn production_idl_schema_generation_recognizes_every_referenced_type() {
        let idl = include_str!("../../../idl/loom.idl");
        let methods = parse(idl);
        let types = parse_type_model(idl).expect("IDL type model");
        let registry = render_registry(&methods, &types).expect("production schemas");

        assert!(
            !registry.contains("x-loom-idl-type"),
            "production schemas must not contain fallback type markers"
        );
    }

    #[test]
    fn schema_generation_rejects_unknown_idl_types() {
        let types = parse_type_model(
            r#"
            struct Known {
                Unknown missing;
            }
            "#,
        )
        .expect("type parse");
        let err = schema_document_for_type("Known", &types).expect_err("unknown field type");
        assert!(err.contains("unknown IDL type Unknown"), "{err}");

        let err = schema_for_type("Unknown", &types).expect_err("unknown top-level type");
        assert!(err.contains("unknown IDL type Unknown"), "{err}");
    }

    #[test]
    fn schema_generation_models_u8_as_bounded_integer() {
        let schema = schema_for_type("u8", &parse_type_model("").expect("empty type model"))
            .expect("u8 schema");
        assert_eq!(
            schema,
            "{\"type\":\"integer\",\"minimum\":0,\"maximum\":255}"
        );
    }

    #[test]
    fn schema_generation_rejects_duplicate_or_malformed_declarations() {
        let duplicate_struct = parse_type_model(
            r#"
            struct A {
                string name;
            }
            struct A {
                string name;
            }
            "#,
        )
        .expect_err("duplicate struct rejected");
        assert!(
            duplicate_struct.contains("duplicate struct A"),
            "{duplicate_struct}"
        );

        let duplicate_field = parse_type_model(
            r#"
            struct A {
                string name;
                string name;
            }
            "#,
        )
        .expect_err("duplicate field rejected");
        assert!(
            duplicate_field.contains("duplicate field name"),
            "{duplicate_field}"
        );

        let malformed_field = parse_type_model(
            r#"
            struct A {
                string name
            }
            "#,
        )
        .expect_err("malformed field rejected");
        assert!(
            malformed_field.contains("missing semicolon"),
            "{malformed_field}"
        );

        let duplicate_enum = parse_type_model(
            r#"
            enum A {
                ONE,
            }
            enum A {
                TWO,
            }
            "#,
        )
        .expect_err("duplicate enum rejected");
        assert!(
            duplicate_enum.contains("duplicate enum A"),
            "{duplicate_enum}"
        );

        let duplicate_enum_value =
            parse_type_model("enum A { ONE, ONE }").expect_err("duplicate enum value rejected");
        assert!(
            duplicate_enum_value.contains("duplicate enum value ONE"),
            "{duplicate_enum_value}"
        );

        let malformed_request = object_schema_for_fields(
            &[("Unknown".to_string(), "value".to_string())],
            &parse_type_model("").expect("empty type model"),
        )
        .expect_err("unknown request field");
        assert!(
            malformed_request.contains("unknown IDL type Unknown"),
            "{malformed_request}"
        );
    }

    #[test]
    fn mu6j_b2a_duplicate_effect_annotation_is_rejected() {
        let error = parse_methods(
            r#"
            interface Files {
                @effect(read)
                @effect(mutation)
                bytes read_file(LoomSession handle);
            }
            "#,
        )
        .expect_err("duplicate effect must fail");

        assert!(error.contains("duplicate effect annotation"), "{error}");
    }

    #[test]
    fn mu6j_b2a_unknown_effect_annotation_is_rejected() {
        let error = parse_methods(
            r#"
            interface Files {
                @effect(write)
                bytes read_file(LoomSession handle);
            }
            "#,
        )
        .expect_err("unknown effect must fail");

        assert!(error.contains("unknown operation effect"), "{error}");
    }

    #[test]
    fn mu6j_b2a_misplaced_effect_annotations_are_rejected() {
        let outside = parse_methods("@effect(read)\ninterface Files {\n}\n")
            .expect_err("outside interface effect must fail");
        assert!(
            outside.contains("outside an interface"),
            "outside error: {outside}"
        );

        let before_close = parse_methods(
            r#"
            interface Files {
                @effect(read)
            }
            "#,
        )
        .expect_err("effect before close must fail");
        assert!(
            before_close.contains("was not followed by a method"),
            "close error: {before_close}"
        );

        let before_interface = parse_methods(
            r#"
            interface Files {
                @effect(read)
                interface Other {
                }
            }
            "#,
        )
        .expect_err("effect before new interface must fail");
        assert!(
            before_interface.contains("was not followed by a method"),
            "interface error: {before_interface}"
        );

        let at_eof = parse_methods(
            r#"
            interface Files {
                @effect(read)
            "#,
        )
        .expect_err("effect at eof must fail");
        assert!(
            at_eof.contains("end of file") || at_eof.contains("was not followed by a method"),
            "eof error: {at_eof}"
        );
    }

    #[test]
    fn mu_6h_k_b_chat_generated_mutations_require_idempotency_key() {
        for method in [
            "chat_create_channel_json",
            "chat_rename_channel_json",
            "chat_post_message_json",
            "chat_post_message_bytes_json",
            "chat_edit_message_json",
            "chat_edit_message_bytes_json",
            "chat_redact_message_json",
            "chat_create_thread_json",
            "chat_create_task_json",
            "chat_claim_task_json",
            "chat_complete_task_json",
            "chat_invoke_agent_json",
            "chat_invoke_agent_bytes_json",
            "chat_agent_reply_json",
            "chat_request_handoff_json",
            "chat_add_reaction_json",
            "chat_remove_reaction_json",
            "chat_emoji_register_json",
            "chat_emoji_unregister_json",
            "chat_update_cursor_json",
        ] {
            assert!(requires_idempotency_key("Chat", method), "{method}");
        }
    }

    #[test]
    fn mu_6i_d1_audit_and_maintenance_contracts_match_idl() {
        let idl = include_str!("../../../idl/loom.idl");
        let methods = parse(idl);
        let audit_compact = methods
            .iter()
            .find(|method| method.interface == "Audit" && method.name == "audit_compact")
            .expect("Audit.audit_compact generated method");
        assert_eq!(audit_compact.ret, "AuditCompactResult");
        assert_eq!(
            audit_compact.args,
            [
                ("LoomSession".to_string(), "handle".to_string()),
                ("u64".to_string(), "through_seq".to_string())
            ]
        );
        assert_eq!(
            pascal_identifier("Audit_audit_compact"),
            "AuditAuditCompact"
        );
        assert!(requires_idempotency_key("Audit", "audit_compact"));

        let status = methods
            .iter()
            .find(|method| {
                method.interface == "StoreAdmin" && method.name == "store_maintenance_status"
            })
            .expect("StoreAdmin.store_maintenance_status generated method");
        assert_eq!(status.ret, "StoreMaintenanceStatusResult");
        assert_eq!(
            status.args,
            [
                ("LoomSession".to_string(), "handle".to_string()),
                (
                    "StoreMaintenanceStatusRequest".to_string(),
                    "request".to_string()
                )
            ]
        );
        assert_eq!(
            pascal_identifier("StoreAdmin_store_maintenance_status"),
            "StoreAdminStoreMaintenanceStatus"
        );
        assert!(!requires_idempotency_key(
            "StoreAdmin",
            "store_maintenance_status"
        ));

        let policy = methods
            .iter()
            .find(|method| {
                method.interface == "StoreAdmin" && method.name == "store_maintenance_policy_set"
            })
            .expect("StoreAdmin.store_maintenance_policy_set generated method");
        assert_eq!(policy.ret, "StoreMaintenanceStatusResult");
        assert_eq!(
            policy.args,
            [
                ("LoomSession".to_string(), "handle".to_string()),
                (
                    "StoreMaintenancePolicyUpdate".to_string(),
                    "update".to_string()
                )
            ]
        );
        assert_eq!(
            pascal_identifier("StoreAdmin_store_maintenance_policy_set"),
            "StoreAdminStoreMaintenancePolicySet"
        );
        assert!(requires_idempotency_key(
            "StoreAdmin",
            "store_maintenance_policy_set"
        ));

        let run = methods
            .iter()
            .find(|method| {
                method.interface == "StoreAdmin" && method.name == "store_maintenance_run"
            })
            .expect("StoreAdmin.store_maintenance_run generated method");
        assert_eq!(run.ret, "StoreMaintenanceRunResult");
        assert_eq!(
            run.args,
            [
                ("LoomSession".to_string(), "handle".to_string()),
                (
                    "StoreMaintenanceRunRequest".to_string(),
                    "request".to_string()
                )
            ]
        );
        assert_eq!(
            pascal_identifier("StoreAdmin_store_maintenance_run"),
            "StoreAdminStoreMaintenanceRun"
        );
        assert!(requires_idempotency_key(
            "StoreAdmin",
            "store_maintenance_run"
        ));
    }
}

/// Map an IDL type to the wire-level Rust type used by the generated trait surface. Named composite
/// types (structs and enums) cross as canonical CBOR `Vec<u8>`, matching the IDL's own use of `bytes`
/// for structured payloads.
fn map_type(ty: &str) -> String {
    let ty = ty.trim();
    if let Some(inner) = ty.strip_prefix("optional ") {
        return format!("Option<{}>", map_type(inner));
    }
    if let Some(inner) = ty.strip_suffix('?') {
        return format!("Option<{}>", map_type(inner));
    }
    if let Some(rest) = ty.strip_prefix("list<") {
        let inner = rest.strip_suffix('>').unwrap_or(rest);
        return format!("Vec<{}>", map_type(inner));
    }
    if let Some(rest) = ty.strip_prefix("stream<") {
        let inner = rest.strip_suffix('>').unwrap_or(rest);
        return format!("LoomStream<{}>", map_type(inner));
    }
    match ty {
        "void" => "()".to_string(),
        "bool" | "u8" | "i32" | "i64" | "u32" | "u64" | "f64" => ty.to_string(),
        "string" => "String".to_string(),
        "bytes" => "Vec<u8>".to_string(),
        "Uuid" => "Uuid".to_string(),
        "Digest" => "Digest".to_string(),
        "LaneTicketPlacement" => "LaneTicketPlacement".to_string(),
        "LoomSession" | "SqlSession" | "SqlBatch" | "RowIter" | "Task" | "ResultView" => {
            ty.to_string()
        }
        _ => "Vec<u8>".to_string(),
    }
}

const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
    "where", "while", "async", "await",
];

fn ident(name: &str) -> String {
    if RUST_KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

fn render_api(methods: &[Method]) -> String {
    let mut order: Vec<String> = Vec::new();
    for method in methods {
        if !order.contains(&method.interface) {
            order.push(method.interface.clone());
        }
    }

    let mut out = String::new();
    out.push_str("//! Generated by `uldren-loom-remote-codegen` from `idl/loom.idl`. Do not edit by hand.\n//!\n//! The `LoomApi` trait families: one trait per IDL interface plus the `LoomClient`\n//! supertrait. Methods that can perform a remote round trip return `impl Future + Send`; local-only\n//! methods (transport `Lo`) are plain `fn`. Regenerate with\n//! `cargo run -p uldren-loom-remote-codegen`.\n\n");
    out.push_str("// Generated bindings mirror the fixed IDL argument lists verbatim, so wide method\n// signatures are intentional.\n#![allow(clippy::too_many_arguments)]\n\n");
    out.push_str("use crate::api_types::{Digest, LaneTicketPlacement, LoomSession, LoomStream, ResultView, RowIter, SqlBatch, SqlSession, Task, Uuid};\n");
    out.push_str("use loom_types::LoomError;\n\n");

    for interface in &order {
        out.push_str(&format!(
            "/// Generated trait for the IDL `{interface}` interface.\n"
        ));
        out.push_str(&format!("pub trait {interface} {{\n"));
        for method in methods.iter().filter(|m| &m.interface == interface) {
            let mut params = String::from("&self");
            for (ty, name) in &method.args {
                if name == "loom_path" {
                    continue;
                }
                params.push_str(&format!(", {}: {}", ident(name), map_type(ty)));
            }
            let ret = map_type(&method.ret);
            if is_sync(interface, &method.name) {
                out.push_str(&format!(
                    "    /// Generated binding for `{}.{}`.\n    fn {}({}) -> Result<{}, LoomError>;\n",
                    interface,
                    method.name,
                    ident(&method.name),
                    params,
                    ret
                ));
            } else {
                out.push_str(&format!(
                    "    /// Generated binding for `{}.{}`.\n    fn {}({}) -> impl ::core::future::Future<Output = Result<{}, LoomError>> + Send;\n",
                    interface,
                    method.name,
                    ident(&method.name),
                    params,
                    ret
                ));
            }
        }
        out.push_str("}\n\n");
    }

    out.push_str(
        "/// The full client surface: every generated interface trait plus `Send + Sync`.\n",
    );
    out.push_str(&format!(
        "pub trait LoomClient: {} + Send + Sync {{}}\n",
        order.join(" + ")
    ));
    out
}

// ---- client stub emitter ------------------------------------------------------------------------

/// The inner type of an `optional X` / `X?`, if any.
fn strip_optional(ty: &str) -> Option<String> {
    let ty = ty.trim();
    if let Some(inner) = ty.strip_prefix("optional ") {
        return Some(inner.trim().to_string());
    }
    ty.strip_suffix('?').map(|inner| inner.trim().to_string())
}

/// The inner type of a `list<X>`, if any.
fn strip_list(ty: &str) -> Option<String> {
    let ty = ty.trim();
    ty.strip_prefix("list<")
        .and_then(|rest| rest.strip_suffix('>'))
        .map(|inner| inner.trim().to_string())
}

/// The inner type of a `stream<X>`, if any.
fn strip_stream(ty: &str) -> Option<String> {
    let ty = ty.trim();
    ty.strip_prefix("stream<")
        .and_then(|rest| rest.strip_suffix('>'))
        .map(|inner| inner.trim().to_string())
}

/// A Rust expression encoding `expr` (of the mapped type for IDL `ty`) into a `loom_codec::Value`.
fn encode_expr(ty: &str, expr: &str) -> String {
    if let Some(inner) = strip_optional(ty) {
        return format!(
            "match {expr} {{ Some(inner) => {{ {} }}, None => ::loom_codec::Value::Null }}",
            encode_expr(&inner, "inner")
        );
    }
    if let Some(inner) = strip_list(ty) {
        return format!(
            "::loom_codec::Value::Array({expr}.into_iter().map(|item| {{ {} }}).collect())",
            encode_expr(&inner, "item")
        );
    }
    // `bytes` and every named composite record cross as a canonical CBOR byte string.
    if map_type(ty) == "Vec<u8>" {
        return format!("::loom_codec::Value::Bytes({expr})");
    }
    // Scalars, `String`, `Uuid`, `Digest`, and handle newtypes all have a `ToValue` impl.
    format!("::loom_remote_protocol::codec::ToValue::to_value(&{expr})")
}

/// A Rust expression decoding `expr` (a `loom_codec::Value`) into `Result<mapped, LoomError>`.
fn decode_expr(ty: &str, expr: &str) -> String {
    if let Some(inner) = strip_optional(ty) {
        return format!(
            "match {expr} {{ ::loom_codec::Value::Null => Ok(None), other => Ok(Some({}?)) }}",
            decode_expr(&inner, "other")
        );
    }
    if let Some(inner) = strip_list(ty) {
        return format!(
            "match {expr} {{ ::loom_codec::Value::Array(items) => items.into_iter().map(|item| {{ {} }}).collect::<::core::result::Result<Vec<_>, _>>(), _ => Err(crate::wire::shape(\"list\")) }}",
            decode_expr(&inner, "item")
        );
    }
    let mapped = map_type(ty);
    if mapped == "()" {
        return format!("{{ let _ = {expr}; Ok(()) }}");
    }
    if mapped == "Vec<u8>" {
        return format!(
            "match {expr} {{ ::loom_codec::Value::Bytes(bytes) => Ok(bytes), _ => Err(crate::wire::shape(\"bytes\")) }}"
        );
    }
    format!("crate::wire::from_wire::<{mapped}>(&{expr})")
}

fn render_client(methods: &[Method]) -> String {
    let mut order: Vec<String> = Vec::new();
    for method in methods {
        if !order.contains(&method.interface) {
            order.push(method.interface.clone());
        }
    }

    let mut out = String::new();
    out.push_str("//! Generated by `uldren-loom-remote-codegen` from `idl/loom.idl`. Do not edit by hand.\n//!\n//! The `RemoteLoomClient` implementations of the generated `LoomApi` traits.\n//! Every round-trip method forwards to `RemoteLoomClient::call` (unary) or `open_stream` (streams),\n//! encoding its arguments and decoding the reply through the shared codec; every local (`Lo`) method\n//! defers to the hand-written `lo_*` helpers in `local_ops.rs`. Regenerate with\n//! `cargo run -p uldren-loom-remote-codegen`.\n\n");
    // Generated argument lists mirror the IDL verbatim; the uniform `list<bytes>` encode closure is
    // mechanically emitted rather than hand-simplified.
    out.push_str("#![allow(clippy::too_many_arguments, clippy::redundant_closure)]\n\n");
    out.push_str("use crate::client::{CallOptions, RemoteLoomClient};\n");
    out.push_str("use crate::transport::Transport;\n");
    out.push_str("use loom_remote_protocol::api_types::{Digest, LaneTicketPlacement, LoomSession, LoomStream, ResultView, RowIter, SqlBatch, SqlSession, Task, Uuid};\n");
    out.push_str("use loom_remote_protocol::generated_api::*;\n");
    out.push_str("use loom_types::LoomError;\n\n");

    for interface in &order {
        out.push_str(&format!(
            "impl<T: Transport + Send + Sync> {interface} for RemoteLoomClient<T> {{\n"
        ));
        for method in methods.iter().filter(|m| &m.interface == interface) {
            let name = ident(&method.name);
            let mut params = String::from("&self");
            let mut arg_idents: Vec<String> = Vec::new();
            for (ty, arg) in &method.args {
                if arg == "loom_path" {
                    continue;
                }
                params.push_str(&format!(", {}: {}", ident(arg), map_type(ty)));
                arg_idents.push(ident(arg));
            }
            let ret = map_type(&method.ret);

            if is_sync(interface, &method.name) {
                // Local (`Lo`) method: defer to the hand-written helper.
                let call_args = arg_idents.join(", ");
                out.push_str(&format!(
                    "    fn {name}({params}) -> Result<{ret}, LoomError> {{\n        self.lo_{}({call_args})\n    }}\n",
                    method.name
                ));
                continue;
            }

            // Build the argument value list once, shared by unary and stream forms.
            let mut arg_values = String::new();
            for (ty, arg) in method.args.iter().filter(|(_, a)| a != "loom_path") {
                arg_values.push_str(&format!("            {},\n", encode_expr(ty, &ident(arg))));
            }

            out.push_str(&format!(
                "    fn {name}({params}) -> impl ::core::future::Future<Output = Result<{ret}, LoomError>> + Send {{\n"
            ));
            out.push_str(&format!(
                "        let args: Vec<::loom_codec::Value> = vec![\n{arg_values}        ];\n"
            ));
            if let Some(_inner) = strip_stream(&method.ret) {
                out.push_str(&format!(
                    "        async move {{\n            let stream = self.open_stream({:?}, {:?}, args).await?;\n            Ok(crate::wire::into_loom_stream(stream))\n        }}\n",
                    interface, method.name
                ));
            } else {
                let options_expr = if requires_idempotency_key(interface, &method.name) {
                    "&self.idempotency_options()"
                } else {
                    "&CallOptions::default()"
                };
                out.push_str(&format!(
                    "        async move {{\n            let value = self.call({:?}, {:?}, args, {}).await?;\n            {}\n        }}\n",
                    interface,
                    method.name,
                    options_expr,
                    decode_expr(&method.ret, "value")
                ));
            }
            out.push_str("    }\n");
        }
        out.push_str("}\n\n");
    }

    out.push_str("/// `RemoteLoomClient` is the full generated client surface.\n");
    out.push_str("impl<T: Transport + Send + Sync> LoomClient for RemoteLoomClient<T> {}\n");
    out
}

// ---- server dispatch emitter --------------------------------------------------------------------

/// A Rust expression decoding a `&loom_codec::Value` (`{expr}`) into `Result<mapped, LoomError>` for
/// server-side argument decode. This is the inverse of the client's `encode_expr`: same shapes, but
/// using the server-visible `loom_remote_protocol::codec` traits and `InvalidArgument` on a bad shape.
fn dispatch_decode(ty: &str, expr: &str) -> String {
    if let Some(inner) = strip_optional(ty) {
        return format!(
            "match {expr} {{ ::loom_codec::Value::Null => Ok(None), other => Ok(Some({}?)) }}",
            dispatch_decode(&inner, "other")
        );
    }
    if let Some(inner) = strip_list(ty) {
        return format!(
            "match {expr} {{ ::loom_codec::Value::Array(items) => items.iter().map(|item| {{ {} }}).collect::<::core::result::Result<Vec<_>, _>>(), _ => Err(shape(\"list\")) }}",
            dispatch_decode(&inner, "item")
        );
    }
    // `bytes` and every named composite record cross as a canonical CBOR byte string.
    if map_type(ty) == "Vec<u8>" {
        return format!(
            "match {expr} {{ ::loom_codec::Value::Bytes(bytes) => Ok(bytes.clone()), _ => Err(shape(\"bytes\")) }}"
        );
    }
    let mapped = map_type(ty);
    format!(
        "<{mapped} as ::loom_remote_protocol::codec::FromValue>::from_value({expr}).map_err(arg_err)"
    )
}

/// A Rust expression encoding an owned reply `expr` (of the mapped type for IDL `ty`) into a
/// `loom_codec::Value`. Reuses the client's `encode_expr`; `void` crosses as `Null`.
fn dispatch_encode_ret(ty: &str, expr: &str) -> String {
    if map_type(ty) == "()" {
        return format!("{{ let _ = {expr}; ::loom_codec::Value::Null }}");
    }
    encode_expr(ty, expr)
}

fn render_dispatch(methods: &[Method]) -> String {
    let mut out = String::new();
    out.push_str("//! Generated by `uldren-loom-remote-codegen` from `idl/loom.idl`. Do not edit by hand.\n//!\n//! The server dispatch that decodes a decoded request, calls the generated `LoomClient` service trait\n//! on `LocalLoomClient`, and encodes the reply with the same canonical codec rules as the generated\n//! `RemoteLoomClient`. Regenerate with `cargo run -p uldren-loom-remote-codegen`.\n\n");
    out.push_str("#![allow(clippy::too_many_arguments, clippy::redundant_closure, clippy::match_single_binding)]\n\n");
    out.push_str("use loom_client::LocalLoomClient;\n");
    out.push_str("use loom_remote_protocol::api_types::{Digest, LaneTicketPlacement, LoomSession, LoomStream, ResultView, RowIter, SqlBatch, SqlSession, Task, Uuid};\n");
    out.push_str("use loom_remote_protocol::generated_api::*;\n");
    out.push_str("use loom_types::{Code, LoomError};\n\n");

    out.push_str("/// The outcome of a generated dispatch: a unary reply value or a stream of item payloads.\n");
    out.push_str("pub enum Dispatched {\n    /// A unary reply value.\n    Unary(::loom_codec::Value),\n    /// A stream of canonical-CBOR item payloads.\n    Stream(LoomStream<Vec<u8>>),\n}\n\n");

    out.push_str("fn shape(expected: &str) -> LoomError {\n    LoomError::new(Code::InvalidArgument, format!(\"unexpected argument shape (expected {expected})\"))\n}\n\n");
    out.push_str("fn arg_err(err: ::loom_remote_protocol::codec::ArgError) -> LoomError {\n    LoomError::new(Code::InvalidArgument, format!(\"argument decode failed: {err}\"))\n}\n\n");
    out.push_str("fn take(args: &[::loom_codec::Value], idx: usize) -> Result<&::loom_codec::Value, LoomError> {\n    args.get(idx).ok_or_else(|| LoomError::new(Code::InvalidArgument, \"missing request argument\"))\n}\n\n");
    out.push_str("fn hosted_path_import_rejected(interface: &str, method: &str) -> LoomError {\n    LoomError::new(\n        Code::Unsupported,\n        format!(\"{interface}.{method} is local-only because it accepts a host filesystem path; remote import requires a byte-transfer contract\"),\n    )\n}\n\n");
    out.push_str("/// Poll an immediately-ready `LocalLoomClient` future once and flatten the result. In-process\n/// futures never pend; a `Pending` is a bug, reported as `INTERNAL` rather than spun on.\nfn poll_ready<T>(fut: impl ::core::future::Future<Output = Result<T, LoomError>>) -> Result<T, LoomError> {\n    let mut fut = ::std::pin::pin!(fut);\n    match fut\n        .as_mut()\n        .poll(&mut ::core::task::Context::from_waker(::std::task::Waker::noop()))\n    {\n        ::core::task::Poll::Ready(output) => output,\n        ::core::task::Poll::Pending => Err(LoomError::new(\n            Code::Internal,\n            \"in-process future returned Pending\",\n        )),\n    }\n}\n\n");
    out.push_str("/// Drain a `LoomStream` into item payloads for transports that still require a buffered response.\n/// In-process streams never pend; a `Pending` is a bug, reported as `INTERNAL`.\npub fn drain_stream(mut stream: LoomStream<Vec<u8>>) -> Result<Vec<Vec<u8>>, LoomError> {\n    let mut cx = ::core::task::Context::from_waker(::std::task::Waker::noop());\n    let mut items = Vec::new();\n    loop {\n        match stream.as_mut().poll_next(&mut cx) {\n            ::core::task::Poll::Ready(Some(Ok(item))) => items.push(item),\n            ::core::task::Poll::Ready(Some(Err(err))) => return Err(err),\n            ::core::task::Poll::Ready(None) => return Ok(items),\n            ::core::task::Poll::Pending => {\n                return Err(LoomError::new(\n                    Code::Internal,\n                    \"in-process stream returned Pending\",\n                ));\n            }\n        }\n    }\n}\n\n");

    out.push_str("/// Decode one hosted request onto the `LoomClient` trait implemented by `LocalLoomClient`.\npub fn dispatch(\n    client: &LocalLoomClient,\n    engine: &LoomSession,\n    interface: &str,\n    method: &str,\n    args: &[::loom_codec::Value],\n) -> Result<Dispatched, LoomError> {\n    dispatch_with_host_path_access(client, engine, interface, method, args, false)\n}\n\n/// Decode one trusted in-process request, including methods that consume local host paths.\npub fn dispatch_local(\n    client: &LocalLoomClient,\n    engine: &LoomSession,\n    interface: &str,\n    method: &str,\n    args: &[::loom_codec::Value],\n) -> Result<Dispatched, LoomError> {\n    dispatch_with_host_path_access(client, engine, interface, method, args, true)\n}\n\nfn dispatch_with_host_path_access(\n    client: &LocalLoomClient,\n    engine: &LoomSession,\n    interface: &str,\n    method: &str,\n    args: &[::loom_codec::Value],\n    allow_host_paths: bool,\n) -> Result<Dispatched, LoomError> {\n    match (interface, method) {\n");

    for method in methods {
        let iface = &method.interface;
        if matches!(
            (iface.as_str(), method.name.as_str()),
            ("FileSystem", "import_fs")
                | ("FileSystem", "import_fs_async")
                | ("Archive", "archive_import")
                | ("Archive", "archive_import_async")
        ) {
            out.push_str(&format!(
                "        ({iface:?}, {:?}) if !allow_host_paths => {{\n",
                method.name
            ));
            out.push_str(&format!(
                "            Err(hosted_path_import_rejected({iface:?}, {:?}))\n",
                method.name
            ));
            out.push_str("        }\n");
        }
        let mut bindings = String::new();
        let mut call_args: Vec<String> = Vec::new();
        let mut wire_idx = 0usize;
        for (ty, name) in &method.args {
            if name == "loom_path" {
                continue;
            }
            if name == "handle" && ty == "LoomSession" {
                call_args.push("engine.clone()".to_string());
                wire_idx += 1;
                continue;
            }
            let id = ident(name);
            let mapped = map_type(ty);
            bindings.push_str(&format!(
                "            let {id} = {{\n                let __v = take(args, {wire_idx})?;\n                let __decoded: ::core::result::Result<{mapped}, LoomError> = {{ {} }};\n                __decoded\n            }}?;\n",
                dispatch_decode(ty, "__v")
            ));
            call_args.push(id);
            wire_idx += 1;
        }
        let sep = if call_args.is_empty() { "" } else { ", " };
        let call = format!(
            "<LocalLoomClient as {iface}>::{}(client{sep}{})",
            ident(&method.name),
            call_args.join(", ")
        );
        out.push_str(&format!("        ({iface:?}, {:?}) => {{\n", method.name));
        out.push_str(&bindings);
        if strip_stream(&method.ret).is_some() {
            out.push_str(&format!("            let stream = poll_ready({call})?;\n"));
            out.push_str("            Ok(Dispatched::Stream(stream))\n");
        } else {
            let call_expr = if is_sync(iface, &method.name) {
                format!("{call}?")
            } else {
                format!("poll_ready({call})?")
            };
            if map_type(&method.ret) == "()" {
                // Void reply: run the call for its effect and encode `Null` without binding a unit value.
                out.push_str(&format!("            {call_expr};\n"));
                out.push_str("            Ok(Dispatched::Unary(::loom_codec::Value::Null))\n");
            } else {
                out.push_str(&format!("            let out = {call_expr};\n"));
                out.push_str(&format!(
                    "            Ok(Dispatched::Unary({}))\n",
                    dispatch_encode_ret(&method.ret, "out")
                ));
            }
        }
        out.push_str("        }\n");
    }

    out.push_str("        _ => Err(LoomError::new(Code::NotFound, format!(\"unknown method {interface}.{method}\"))),\n");
    out.push_str("    }\n}\n");
    out
}

fn rustfmt(source: &str) -> String {
    let mut child = match Command::new("rustfmt")
        .arg("--edition")
        .arg("2024")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return source.to_string(),
    };
    if let Some(mut stdin) = child.stdin.take()
        && stdin.write_all(source.as_bytes()).is_err()
    {
        return source.to_string();
    }
    match child.wait_with_output() {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).to_string()
        }
        _ => source.to_string(),
    }
}

struct Artifact {
    path: PathBuf,
    content: String,
}

fn main() -> ExitCode {
    let check = std::env::args().any(|a| a == "--check");
    let root = repo_root();
    let idl_path = root.join("idl").join("loom.idl");

    let idl = match std::fs::read_to_string(&idl_path) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("cannot read {}: {err}", idl_path.display());
            return ExitCode::FAILURE;
        }
    };
    let methods = parse(&idl);
    let types = match parse_type_model(&idl) {
        Ok(types) => types,
        Err(err) => {
            eprintln!(
                "cannot parse IDL type model from {}: {err}",
                idl_path.display()
            );
            return ExitCode::FAILURE;
        }
    };

    let artifacts = [
        Artifact {
            path: root
                .join("crates")
                .join("loom-remote-protocol")
                .join("src")
                .join("generated.rs"),
            content: match render_registry(&methods, &types) {
                Ok(content) => rustfmt(&content),
                Err(err) => {
                    eprintln!("cannot render generated registry: {err}");
                    return ExitCode::FAILURE;
                }
            },
        },
        Artifact {
            path: root
                .join("crates")
                .join("loom-remote-protocol")
                .join("src")
                .join("generated_api.rs"),
            content: rustfmt(&render_api(&methods)),
        },
        Artifact {
            path: root
                .join("crates")
                .join("loom-remote-client")
                .join("src")
                .join("generated_client.rs"),
            content: rustfmt(&render_client(&methods)),
        },
        Artifact {
            path: root
                .join("crates")
                .join("loom-hosted-core")
                .join("src")
                .join("generated_dispatch.rs"),
            content: rustfmt(&render_dispatch(&methods)),
        },
    ];

    let mut stale = false;
    for artifact in &artifacts {
        if check {
            let current = std::fs::read_to_string(&artifact.path).unwrap_or_default();
            if current != artifact.content {
                eprintln!("stale: {}", artifact.path.display());
                stale = true;
            }
        } else if let Err(err) = std::fs::write(&artifact.path, &artifact.content) {
            eprintln!("cannot write {}: {err}", artifact.path.display());
            return ExitCode::FAILURE;
        } else {
            println!("wrote {}", artifact.path.display());
        }
    }

    if check && stale {
        eprintln!("run `cargo run -p uldren-loom-remote-codegen` to regenerate");
        return ExitCode::FAILURE;
    }
    if check {
        println!(
            "generated artifacts are up to date ({} methods)",
            methods.len()
        );
    }
    ExitCode::SUCCESS
}
