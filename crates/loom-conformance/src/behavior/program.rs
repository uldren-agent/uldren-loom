//! Behavioral conformance for the `Program` family: a canonical stored-record CBOR vector plus an
//! engine put/inspect/get/list/remove round-trip over a [`MemoryStore`].

use loom_codec::Value as CborValue;
use loom_compute::{
    GrantSet, Manifest, ProgramBody, StoredProgram, program_get, program_inspect, program_list,
    program_put, program_remove,
};
use loom_core::{FacetKind, Loom, MemoryStore, Result, WorkspaceId};

const PROGRAM_NAME: &str = "page-card";
const PROGRAM_SOURCE: &str = "Hello, {{ name }}";

pub struct ProgramCanonicalVector {
    pub name: &'static str,
    pub expect_record_canonical: &'static str,
}

/// The pinned canonical stored-record CBOR for the template fixture. Every binding must reproduce these
/// exact bytes for `program_inspect`.
pub const PROGRAM_CANONICAL_VECTOR: ProgramCanonicalVector = ProgramCanonicalVector {
    name: "template-page-card",
    expect_record_canonical: "8569706167652d636172647847626c616b65333a363734363263633730663864353134333731346532663133613435396263316562313339363235383034343039633932333639323734326265626333306230377847626c616b65333a356237363064623163396230336632336533636233373566643833303732653636333735353362336364643330663739363831343731636236393038653666341158458c01010169706167652d636172646874656d706c617465016672656e64657258205b760db1c9b03f23e3cb375fd83072e6637553b3cdd30f79681471cb6908e6f4f6f68080",
};

fn manifest_fixture() -> Manifest {
    Manifest::for_template(PROGRAM_NAME, PROGRAM_SOURCE, GrantSet::default())
}

fn program_record_value(record: &StoredProgram) -> CborValue {
    CborValue::Array(vec![
        CborValue::Text(record.name.clone()),
        CborValue::Text(record.manifest_digest.to_string()),
        CborValue::Text(record.body_digest.to_string()),
        CborValue::Uint(record.body_len),
        CborValue::Bytes(record.manifest.encode()),
    ])
}

fn program_record_to_cbor(record: &StoredProgram) -> Result<Vec<u8>> {
    loom_codec::encode(&program_record_value(record))
        .map_err(|err| loom_core::LoomError::invalid(format!("encode program record: {err}")))
}

pub fn run_program_behavior() -> Result<()> {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom.registry_mut().create(
        FacetKind::Program,
        None,
        WorkspaceId::from_bytes([0x70; 16]),
    )?;

    // Absent program: inspect/get are None, list is empty, remove is false.
    assert_eq!(program_inspect(&loom, ns, PROGRAM_NAME)?, None);
    assert_eq!(program_get(&loom, ns, PROGRAM_NAME)?, None);
    assert!(program_list(&loom, ns)?.is_empty());
    assert!(!program_remove(&mut loom, ns, PROGRAM_NAME)?);

    let manifest = manifest_fixture();
    let stored = program_put(
        &mut loom,
        ns,
        PROGRAM_NAME,
        manifest,
        PROGRAM_SOURCE.as_bytes(),
    )?;
    assert_eq!(stored.name, PROGRAM_NAME);
    assert_eq!(stored.body_len, PROGRAM_SOURCE.len() as u64);

    // The stored record's canonical CBOR is pinned.
    assert_eq!(
        hex::encode(program_record_to_cbor(&stored)?),
        PROGRAM_CANONICAL_VECTOR.expect_record_canonical,
        "program record canonical bytes mismatch for '{}'",
        PROGRAM_CANONICAL_VECTOR.name
    );

    let inspected = program_inspect(&loom, ns, PROGRAM_NAME)?.expect("record present");
    assert_eq!(inspected, stored);

    let body: ProgramBody = program_get(&loom, ns, PROGRAM_NAME)?.expect("body present");
    assert_eq!(body.record, stored);
    assert_eq!(body.body, PROGRAM_SOURCE.as_bytes());

    assert_eq!(program_list(&loom, ns)?, vec![stored]);

    assert!(program_remove(&mut loom, ns, PROGRAM_NAME)?);
    assert_eq!(program_inspect(&loom, ns, PROGRAM_NAME)?, None);
    assert!(program_list(&loom, ns)?.is_empty());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_behavior_passes() {
        run_program_behavior().expect("program behavior must pass");
    }
}
