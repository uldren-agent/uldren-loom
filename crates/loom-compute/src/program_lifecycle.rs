use loom_codec::Value;
use loom_core::{
    AclRight, Code, Digest, Object, ObjectStore, Result,
    error::LoomError,
    fs::FileKind,
    vcs::Loom,
    workspace::{FacetKind, WorkspaceId, facet_path},
};
use loom_templates::TemplateProcessor;

use crate::manifest::Manifest;

const PROGRAMS_DIR: &str = "programs";
const MANIFESTS_DIR: &str = "programs/manifests";
const BODIES_DIR: &str = "programs/bodies";
const PROGRAM_RECORD_SCHEMA: &str = "loom.program.record.v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredProgram {
    pub name: String,
    pub manifest_digest: Digest,
    pub body_digest: Digest,
    pub body_len: u64,
    pub manifest: Manifest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramBody {
    pub record: StoredProgram,
    pub body: Vec<u8>,
}

pub fn program_put_wasm<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    manifest: Manifest,
    body: &[u8],
) -> Result<StoredProgram> {
    program_put(loom, ns, name, manifest, body)
}

pub fn program_put_template<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    manifest: Manifest,
    source: &str,
) -> Result<StoredProgram> {
    program_put(loom, ns, name, manifest, source.as_bytes())
}

pub fn program_put_cel<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    manifest: Manifest,
    source: &str,
) -> Result<StoredProgram> {
    program_put(loom, ns, name, manifest, source.as_bytes())
}

pub fn program_put<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    manifest: Manifest,
    body: &[u8],
) -> Result<StoredProgram> {
    validate_program_name(name)?;
    loom.authorize(ns, FacetKind::Program, AclRight::Write)?;
    if manifest.name != name {
        return Err(LoomError::invalid(
            "program name does not match manifest name",
        ));
    }
    validate_program_body(&manifest, body)?;
    let body_digest = Object::Blob(body.to_vec()).digest();
    if manifest.body != body_digest {
        return Err(LoomError::integrity_failure(
            "program body digest does not match manifest body digest",
        ));
    }

    let manifest_bytes = manifest.encode();
    let manifest_digest = manifest.store(loom.store_mut())?;
    let stored = StoredProgram {
        name: name.to_string(),
        manifest_digest,
        body_digest,
        body_len: body.len() as u64,
        manifest,
    };

    loom.create_directory_reserved(ns, &facet_path(FacetKind::Program, PROGRAMS_DIR), true)?;
    loom.create_directory_reserved(ns, &facet_path(FacetKind::Program, MANIFESTS_DIR), true)?;
    loom.create_directory_reserved(ns, &facet_path(FacetKind::Program, BODIES_DIR), true)?;
    loom.write_file_reserved(
        ns,
        &manifest_path(manifest_digest),
        &manifest_bytes,
        0o100644,
    )?;
    loom.write_file_reserved(ns, &body_path(body_digest), body, 0o100644)?;
    loom.write_file_reserved(ns, &record_path(name), &stored.encode()?, 0o100644)?;
    Ok(stored)
}

pub fn program_inspect<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<Option<StoredProgram>> {
    validate_program_name(name)?;
    loom.authorize(ns, FacetKind::Program, AclRight::Read)?;
    let bytes = match loom.read_file_reserved(ns, &record_path(name)) {
        Ok(bytes) => bytes,
        Err(err) if err.code == Code::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let stored = StoredProgram::decode(&bytes)?;
    if stored.name != name {
        return Err(LoomError::corrupt(
            "program record name does not match path",
        ));
    }
    let manifest_bytes = loom.read_file_reserved(ns, &manifest_path(stored.manifest_digest))?;
    let manifest = Manifest::decode(&manifest_bytes)
        .ok_or_else(|| LoomError::corrupt("stored program manifest is not canonical"))?;
    if manifest != stored.manifest {
        return Err(LoomError::corrupt(
            "program record manifest does not match stored manifest",
        ));
    }
    let body = loom.read_file_reserved(ns, &body_path(stored.body_digest))?;
    if Object::Blob(body).digest() != stored.body_digest {
        return Err(LoomError::integrity_failure(
            "stored program body digest mismatch",
        ));
    }
    Ok(Some(stored))
}

pub fn program_get<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<Option<ProgramBody>> {
    let Some(record) = program_inspect(loom, ns, name)? else {
        return Ok(None);
    };
    let body = loom.read_file_reserved(ns, &body_path(record.body_digest))?;
    if Object::Blob(body.clone()).digest() != record.body_digest {
        return Err(LoomError::integrity_failure(
            "stored program body digest mismatch",
        ));
    }
    if body.len() as u64 != record.body_len {
        return Err(LoomError::corrupt(
            "program body length does not match record",
        ));
    }
    Ok(Some(ProgramBody { record, body }))
}

pub fn program_list<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId) -> Result<Vec<StoredProgram>> {
    loom.authorize(ns, FacetKind::Program, AclRight::Read)?;
    let dir = facet_path(FacetKind::Program, PROGRAMS_DIR);
    let entries = match loom.list_directory(ns, &dir) {
        Ok(entries) => entries,
        Err(err) if err.code == Code::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut programs = Vec::new();
    for entry in entries {
        if entry.kind != FileKind::File || !entry.name.ends_with(".cbor") {
            continue;
        }
        let Some(name) = entry.name.strip_suffix(".cbor") else {
            continue;
        };
        if let Some(program) = program_inspect(loom, ns, name)? {
            programs.push(program);
        }
    }
    programs.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(programs)
}

pub fn program_remove<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<bool> {
    validate_program_name(name)?;
    loom.authorize(ns, FacetKind::Program, AclRight::Write)?;
    let path = record_path(name);
    match loom.read_file_reserved(ns, &path) {
        Ok(_) => {
            loom.remove_file_reserved(ns, &path)?;
            Ok(true)
        }
        Err(err) if err.code == Code::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

impl StoredProgram {
    fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&Value::Array(vec![
            Value::Text(PROGRAM_RECORD_SCHEMA.to_string()),
            Value::Text(self.name.clone()),
            digest_value(self.manifest_digest),
            digest_value(self.body_digest),
            Value::Uint(self.body_len),
            Value::Bytes(self.manifest.encode()),
        ]))
        .map_err(|e| LoomError::invalid(format!("encode program record: {e}")))
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        let value = loom_codec::decode(bytes)
            .map_err(|e| LoomError::corrupt(format!("decode program record: {e}")))?;
        let Value::Array(items) = value else {
            return Err(LoomError::corrupt("program record must be an array"));
        };
        let mut fields = items.into_iter();
        match fields.next() {
            Some(Value::Text(schema)) if schema == PROGRAM_RECORD_SCHEMA => {}
            _ => return Err(LoomError::corrupt("unsupported program record schema")),
        }
        let name = match fields.next() {
            Some(Value::Text(name)) => name,
            _ => return Err(LoomError::corrupt("program record name must be text")),
        };
        let manifest_digest = decode_digest(fields.next(), "manifest digest")?;
        let body_digest = decode_digest(fields.next(), "body digest")?;
        let body_len = match fields.next() {
            Some(Value::Uint(len)) => len,
            _ => {
                return Err(LoomError::corrupt(
                    "program record body length must be uint",
                ));
            }
        };
        let manifest_bytes = match fields.next() {
            Some(Value::Bytes(bytes)) => bytes,
            _ => return Err(LoomError::corrupt("program record manifest must be bytes")),
        };
        if fields.next().is_some() {
            return Err(LoomError::corrupt("program record has trailing fields"));
        }
        let manifest = Manifest::decode(&manifest_bytes)
            .ok_or_else(|| LoomError::corrupt("program record manifest is not canonical"))?;
        Ok(Self {
            name,
            manifest_digest,
            body_digest,
            body_len,
            manifest,
        })
    }
}

fn validate_program_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(LoomError::invalid("program name must not be empty or dot"));
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(LoomError::invalid(
            "program name must use ASCII letters, digits, dot, underscore, or hyphen",
        ));
    }
    Ok(())
}

fn validate_program_body(manifest: &Manifest, body: &[u8]) -> Result<()> {
    if !manifest.grants.is_grantable() {
        return Err(LoomError::invalid(
            "program manifest declares a non-grantable facet",
        ));
    }
    match manifest.engine.as_str() {
        "wasm" => validate_engine_shape(manifest, 1, "run"),
        "template" => {
            validate_engine_shape(manifest, 1, "render")?;
            let source = utf8_body(body, "template")?;
            TemplateProcessor::new()
                .process(&manifest.name, source)
                .map_err(|err| LoomError::invalid(format!("template program is invalid: {err}")))?;
            Ok(())
        }
        "cel" => {
            validate_engine_shape(manifest, 1, "eval")?;
            utf8_body(body, "cel")?;
            Ok(())
        }
        _ => Err(LoomError::unsupported(format!(
            "unsupported program engine {}",
            manifest.engine
        ))),
    }
}

fn validate_engine_shape(manifest: &Manifest, abi_version: u32, entry: &str) -> Result<()> {
    if manifest.abi_version != abi_version || manifest.entry != entry {
        return Err(LoomError::invalid(format!(
            "program engine {} requires abi v{abi_version} entry {entry}",
            manifest.engine
        )));
    }
    Ok(())
}

fn utf8_body<'a>(body: &'a [u8], engine: &str) -> Result<&'a str> {
    std::str::from_utf8(body)
        .map_err(|_| LoomError::invalid(format!("engine={engine} body must be UTF-8")))
}

fn digest_value(digest: Digest) -> Value {
    Value::Bytes(digest.bytes().to_vec())
}

fn decode_digest(value: Option<Value>, field: &str) -> Result<Digest> {
    let Some(Value::Bytes(bytes)) = value else {
        return Err(LoomError::corrupt(format!(
            "program record {field} must be bytes"
        )));
    };
    let bytes: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| LoomError::corrupt(format!("program record {field} has wrong length")))?;
    Ok(Digest::from_blake3_bytes(bytes))
}

fn record_path(name: &str) -> String {
    facet_path(FacetKind::Program, &format!("{PROGRAMS_DIR}/{name}.cbor"))
}

fn manifest_path(digest: Digest) -> String {
    facet_path(
        FacetKind::Program,
        &format!("{MANIFESTS_DIR}/{}.cbor", hex_digest(digest)),
    )
}

fn body_path(digest: Digest) -> String {
    facet_path(
        FacetKind::Program,
        &format!("{BODIES_DIR}/{}.body", hex_digest(digest)),
    )
}

fn hex_digest(digest: Digest) -> String {
    let mut out = String::with_capacity(64);
    for byte in digest.bytes() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GrantSet, Manifest};
    use loom_core::{MemoryStore, workspace::WorkspaceId};

    fn id(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    #[test]
    fn wasm_program_put_and_inspect_round_trip() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = id(9);
        loom.registry_mut()
            .create(FacetKind::Program, Some("programs"), ns)
            .unwrap();
        let body = b"\0asm";
        let manifest = Manifest::for_wasm("demo.wasm", body, GrantSet::all_facets());

        let stored = program_put_wasm(&mut loom, ns, "demo.wasm", manifest.clone(), body).unwrap();
        assert_eq!(stored.manifest, manifest);
        assert_eq!(stored.body_digest, Object::Blob(body.to_vec()).digest());

        let inspected = program_inspect(&loom, ns, "demo.wasm").unwrap().unwrap();
        assert_eq!(inspected, stored);
        assert_eq!(program_inspect(&loom, ns, "missing").unwrap(), None);
    }

    #[test]
    fn wasm_program_rejects_digest_mismatch() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = id(10);
        loom.registry_mut()
            .create(FacetKind::Program, Some("programs"), ns)
            .unwrap();
        let manifest = Manifest::for_wasm("demo", b"expected", GrantSet::all_facets());

        let err = program_put(&mut loom, ns, "demo", manifest, b"actual").unwrap_err();
        assert_eq!(err.code, Code::IntegrityFailure);
    }

    #[test]
    fn generic_program_put_supports_template_and_cel() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = id(11);
        loom.registry_mut()
            .create(FacetKind::Program, Some("programs"), ns)
            .unwrap();
        let template = r#"{"outputs":{"html":"ready"}}"#;
        let template_manifest =
            Manifest::for_template("page-card", template, GrantSet::all_facets());
        let cel = "request.amount < 100";
        let cel_manifest = Manifest::for_cel("limit-check", cel, GrantSet::all_facets());

        let stored_template = program_put_template(
            &mut loom,
            ns,
            "page-card",
            template_manifest.clone(),
            template,
        )
        .unwrap();
        let stored_cel =
            program_put_cel(&mut loom, ns, "limit-check", cel_manifest.clone(), cel).unwrap();

        assert_eq!(stored_template.manifest.engine, "template");
        assert_eq!(stored_cel.manifest.engine, "cel");
        assert_eq!(
            program_inspect(&loom, ns, "page-card").unwrap().unwrap(),
            stored_template
        );
        assert_eq!(
            program_inspect(&loom, ns, "limit-check").unwrap().unwrap(),
            stored_cel
        );
    }

    #[test]
    fn generic_program_put_rejects_invalid_template_source() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = id(12);
        loom.registry_mut()
            .create(FacetKind::Program, Some("programs"), ns)
            .unwrap();
        let source = "{% if broken %}";
        let manifest = Manifest::for_template("bad-template", source, GrantSet::all_facets());

        let err =
            program_put_template(&mut loom, ns, "bad-template", manifest, source).unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
    }

    #[test]
    fn generic_program_put_rejects_invalid_names_and_manifest_name_mismatch() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = id(16);
        loom.registry_mut()
            .create(FacetKind::Program, Some("programs"), ns)
            .unwrap();
        let body = b"\0asm";
        let manifest = Manifest::for_wasm("demo", body, GrantSet::all_facets());

        let err = program_put(&mut loom, ns, "bad/name", manifest.clone(), body).unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
        let err = program_put(&mut loom, ns, "other", manifest, body).unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
    }

    #[test]
    fn generic_program_put_rejects_invalid_engine_shapes() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = id(17);
        loom.registry_mut()
            .create(FacetKind::Program, Some("programs"), ns)
            .unwrap();
        let wasm = b"\0asm";
        let template = r#"{"outputs":{"html":"ready"}}"#;
        let cel = "request.amount < 100";

        let mut bad_wasm = Manifest::for_wasm("bad-wasm", wasm, GrantSet::all_facets());
        bad_wasm.entry = "render".to_string();
        let err = program_put(&mut loom, ns, "bad-wasm", bad_wasm, wasm).unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);

        let mut bad_template =
            Manifest::for_template("bad-template", template, GrantSet::all_facets());
        bad_template.abi_version = 2;
        let err = program_put_template(&mut loom, ns, "bad-template", bad_template, template)
            .unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);

        let mut bad_cel = Manifest::for_cel("bad-cel", cel, GrantSet::all_facets());
        bad_cel.entry = "render".to_string();
        let err = program_put_cel(&mut loom, ns, "bad-cel", bad_cel, cel).unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);

        let mut unsupported = Manifest::for_cel("bad-engine", cel, GrantSet::all_facets());
        unsupported.engine = "python".to_string();
        let err =
            program_put(&mut loom, ns, "bad-engine", unsupported, cel.as_bytes()).unwrap_err();
        assert_eq!(err.code, Code::Unsupported);
    }

    #[test]
    fn generic_program_put_rejects_non_utf8_template_and_cel_bodies() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = id(18);
        loom.registry_mut()
            .create(FacetKind::Program, Some("programs"), ns)
            .unwrap();
        let invalid = [0xff, 0xfe];

        let mut template = Manifest::for_template("template", "", GrantSet::all_facets());
        template.body = Object::Blob(invalid.to_vec()).digest();
        let err = program_put(&mut loom, ns, "template", template, &invalid).unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);

        let mut cel = Manifest::for_cel("cel", "", GrantSet::all_facets());
        cel.body = Object::Blob(invalid.to_vec()).digest();
        let err = program_put(&mut loom, ns, "cel", cel, &invalid).unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
    }

    #[test]
    fn generic_program_get_list_and_remove_manage_named_records() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = id(13);
        loom.registry_mut()
            .create(FacetKind::Program, Some("programs"), ns)
            .unwrap();
        let wasm = b"\0asm";
        let template = r#"{"outputs":{"html":"ready"}}"#;
        let cel = "request.amount < 100";

        program_put(
            &mut loom,
            ns,
            "z-wasm",
            Manifest::for_wasm("z-wasm", wasm, GrantSet::all_facets()),
            wasm,
        )
        .unwrap();
        program_put_template(
            &mut loom,
            ns,
            "a-template",
            Manifest::for_template("a-template", template, GrantSet::all_facets()),
            template,
        )
        .unwrap();
        program_put_cel(
            &mut loom,
            ns,
            "m-cel",
            Manifest::for_cel("m-cel", cel, GrantSet::all_facets()),
            cel,
        )
        .unwrap();

        let names = program_list(&loom, ns)
            .unwrap()
            .into_iter()
            .map(|program| program.name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["a-template", "m-cel", "z-wasm"]);

        let loaded = program_get(&loom, ns, "a-template").unwrap().unwrap();
        assert_eq!(loaded.record.manifest.engine, "template");
        assert_eq!(loaded.body, template.as_bytes());

        assert!(program_remove(&mut loom, ns, "a-template").unwrap());
        assert!(!program_remove(&mut loom, ns, "a-template").unwrap());
        assert!(program_get(&loom, ns, "a-template").unwrap().is_none());
        let names = program_list(&loom, ns)
            .unwrap()
            .into_iter()
            .map(|program| program.name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["m-cel", "z-wasm"]);
    }

    #[test]
    fn program_get_detects_corrupt_body_bytes_and_length() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = id(14);
        loom.registry_mut()
            .create(FacetKind::Program, Some("programs"), ns)
            .unwrap();
        let body = b"\0asm";
        let stored = program_put_wasm(
            &mut loom,
            ns,
            "demo",
            Manifest::for_wasm("demo", body, GrantSet::all_facets()),
            body,
        )
        .unwrap();

        loom.write_file_reserved(ns, &body_path(stored.body_digest), b"corrupt", 0o100644)
            .unwrap();
        let err = program_get(&loom, ns, "demo").unwrap_err();
        assert_eq!(err.code, Code::IntegrityFailure);

        loom.write_file_reserved(ns, &body_path(stored.body_digest), body, 0o100644)
            .unwrap();
        let mut wrong_len = stored;
        wrong_len.body_len += 1;
        loom.write_file_reserved(
            ns,
            &record_path("demo"),
            &wrong_len.encode().unwrap(),
            0o100644,
        )
        .unwrap();
        let err = program_get(&loom, ns, "demo").unwrap_err();
        assert_eq!(err.code, Code::CorruptObject);
    }

    #[test]
    fn program_list_rejects_corrupt_records() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = id(15);
        loom.registry_mut()
            .create(FacetKind::Program, Some("programs"), ns)
            .unwrap();
        loom.create_directory_reserved(ns, &facet_path(FacetKind::Program, PROGRAMS_DIR), true)
            .unwrap();
        loom.write_file_reserved(ns, &record_path("bad"), b"not cbor", 0o100644)
            .unwrap();

        let err = program_list(&loom, ns).unwrap_err();
        assert_eq!(err.code, Code::CorruptObject);
    }
}
