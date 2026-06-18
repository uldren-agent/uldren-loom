//! The program manifest: the content-addressed declaration of a program.
//!
//! Fields: name, engine and ABI version, entry point, the fine-grained capability grants the program
//! requires, optional input/output schema digests, and the body digest. The canonical form is Loom
//! Canonical CBOR v1 (`loom_codec`): the object framing `[epoch, type, ...fields]` with
//! [`MANIFEST_TYPE_CODE`] as the type and [`MANIFEST_SCHEMA_VERSION`] as the first field, decoded
//! strictly. The manifest is stored as a content-addressed `Blob` of that form, so a program's identity
//! is its manifest digest. Grants are held in canonical
//! order ([`GrantSet`]) and the facet axis is the single canonical `FacetKind::stable_tag`, so the
//! bytes and the digest are deterministic and cross-language.

use loom_codec::Value;
use loom_core::{Digest, Object, ObjectStore, Result};

use crate::capability::{Capability, Grant, GrantSet, Mode, Scope, is_program_grantable};

/// The manifest schema type, written as the object-framing type code inside the Loom Canonical CBOR
/// array. Bumped only on an incompatible manifest schema change; [`Manifest::decode`] rejects any
/// other type code. This is manifest-local and independent of the `loom-core` object-type space
/// (a manifest is carried inside a `Blob`, never decoded as a core object).
pub const MANIFEST_TYPE_CODE: u16 = 1;

/// The manifest schema version, encoded as the first manifest payload field (after the object-framing
/// type). Bumped on each schema revision; [`Manifest::decode`] rejects any version it does not
/// understand. A separate axis from the `loom_codec` codec epoch, [`MANIFEST_TYPE_CODE`], and the
/// program `abi_version`.
pub const MANIFEST_SCHEMA_VERSION: u64 = 1;

/// Scope tag: a whole-facet grant.
const SCOPE_ALL: u64 = 0;
/// Scope tag: a prefix grant, followed by the prefix text.
const SCOPE_PREFIX: u64 = 1;

/// The phase in which a manifest guard is checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardPhase {
    /// Precondition, checked against the base before the program runs.
    Pre,
    /// Postcondition / invariant, checked against the proposal after the program runs.
    Post,
}

impl GuardPhase {
    const fn as_u64(self) -> u64 {
        match self {
            GuardPhase::Pre => 0,
            GuardPhase::Post => 1,
        }
    }

    const fn from_u64(v: u64) -> Option<Self> {
        match v {
            0 => Some(GuardPhase::Pre),
            1 => Some(GuardPhase::Post),
            _ => None,
        }
    }
}

/// A guard declaration carried in the manifest: a CEL expression plus the phase it is checked in. This
/// is data only - it folds the guard into the program's content-addressed identity so guards cannot be
/// swapped without changing the manifest digest. Evaluation is the `guards` feature's job
/// ([`crate::guard`]), which this always-compiled type deliberately does not depend on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestGuard {
    pub phase: GuardPhase,
    pub expr: String,
}

/// A program's content-addressed declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub engine: String,
    pub abi_version: u32,
    pub entry: String,
    pub grants: GrantSet,
    pub input_schema: Option<Digest>,
    pub output_schema: Option<Digest>,
    pub body: Digest,
    /// Guard predicates folded into manifest identity; empty when the program declares none.
    pub guards: Vec<ManifestGuard>,
}

impl Manifest {
    /// Build a manifest for a WASM program, computing the body digest via the loom-core object model.
    pub fn for_wasm(name: &str, body: &[u8], grants: GrantSet) -> Self {
        Self {
            name: name.to_string(),
            engine: "wasm".to_string(),
            abi_version: 1,
            entry: "run".to_string(),
            grants,
            input_schema: None,
            output_schema: None,
            body: Object::Blob(body.to_vec()).digest(),
            guards: Vec::new(),
        }
    }

    /// Build a manifest for a render-only Loom template program.
    pub fn for_template(name: &str, source: &str, grants: GrantSet) -> Self {
        Self {
            name: name.to_string(),
            engine: "template".to_string(),
            abi_version: 1,
            entry: "render".to_string(),
            grants,
            input_schema: None,
            output_schema: None,
            body: Object::Blob(source.as_bytes().to_vec()).digest(),
            guards: Vec::new(),
        }
    }

    /// Build a manifest for a read-only interpreted CEL program body.
    pub fn for_cel(name: &str, source: &str, grants: GrantSet) -> Self {
        Self {
            name: name.to_string(),
            engine: "cel".to_string(),
            abi_version: 1,
            entry: "eval".to_string(),
            grants,
            input_schema: None,
            output_schema: None,
            body: Object::Blob(source.as_bytes().to_vec()).digest(),
            guards: Vec::new(),
        }
    }

    /// Canonical Loom CBOR bytes; deterministic because the grant set is already canonically ordered
    /// and the encoder emits exactly one form per value.
    pub fn encode(&self) -> Vec<u8> {
        let grants = self.grants.grants.iter().map(grant_value).collect();
        let guards = self.guards.iter().map(guard_value).collect();
        let fields = [
            Value::Uint(MANIFEST_SCHEMA_VERSION),
            Value::Text(self.name.clone()),
            Value::Text(self.engine.clone()),
            Value::Uint(u64::from(self.abi_version)),
            Value::Text(self.entry.clone()),
            digest_value(&self.body),
            opt_digest_value(self.input_schema.as_ref()),
            opt_digest_value(self.output_schema.as_ref()),
            Value::Array(grants),
            Value::Array(guards),
        ];
        // A manifest carries no floats and no maps, so the codec's only two encode error paths
        // (non-canonical float, duplicate map key) cannot occur here.
        loom_codec::encode_object(MANIFEST_TYPE_CODE, &fields)
            .expect("manifest encodes to canonical CBOR")
    }

    /// Store the manifest as a content-addressed `Blob`; the returned digest is the program identity.
    pub fn store(&self, store: &mut dyn ObjectStore) -> Result<Digest> {
        store.put(&Object::Blob(self.encode()).canonical())
    }

    /// Parse canonical bytes back into a manifest, or `None` if the bytes are not a strictly canonical
    /// manifest object (wrong epoch, type code, or schema version, non-canonical CBOR, unknown facet or
    /// mode tag, a non-grantable facet, malformed scope, wrong shape, or trailing fields/bytes).
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let (type_code, fields) = loom_codec::decode_object(bytes).ok()?;
        if type_code != MANIFEST_TYPE_CODE {
            return None;
        }
        let mut f = Fields::new(fields);
        if f.uint()? != MANIFEST_SCHEMA_VERSION {
            return None;
        }
        let name = f.text()?;
        let engine = f.text()?;
        let abi_version = u32::try_from(f.uint()?).ok()?;
        let entry = f.text()?;
        let body = f.digest()?;
        let input_schema = f.opt_digest()?;
        let output_schema = f.opt_digest()?;
        let grants = f
            .array()?
            .into_iter()
            .map(decode_grant)
            .collect::<Option<Vec<Grant>>>()?;
        let guards = f
            .array()?
            .into_iter()
            .map(decode_guard)
            .collect::<Option<Vec<ManifestGuard>>>()?;
        f.end()?;
        Some(Manifest {
            name,
            engine,
            abi_version,
            entry,
            grants: GrantSet::new(grants),
            input_schema,
            output_schema,
            body,
            guards,
        })
    }
}

/// A digest as a raw 32-byte CBOR byte string (the loom-core convention: the algorithm is a store-level
/// property, not encoded per digest, and digest equality is over bytes only).
fn digest_value(d: &Digest) -> Value {
    Value::Bytes(d.bytes().to_vec())
}

fn opt_digest_value(d: Option<&Digest>) -> Value {
    match d {
        Some(d) => digest_value(d),
        None => Value::Null,
    }
}

fn grant_value(g: &Grant) -> Value {
    let scopes = g.scopes.iter().map(scope_value).collect();
    Value::Array(vec![
        Value::Uint(u64::from(g.facet.stable_tag())),
        Value::Uint(u64::from(g.mode.as_u8())),
        Value::Array(scopes),
    ])
}

fn scope_value(s: &Scope) -> Value {
    match s {
        Scope::All => Value::Array(vec![Value::Uint(SCOPE_ALL)]),
        Scope::Prefix(prefix) => {
            Value::Array(vec![Value::Uint(SCOPE_PREFIX), Value::Text(prefix.clone())])
        }
    }
}

/// A guard as `[phase_tag, expr]` (phase 0 = Pre, 1 = Post).
fn guard_value(g: &ManifestGuard) -> Value {
    Value::Array(vec![
        Value::Uint(g.phase.as_u64()),
        Value::Text(g.expr.clone()),
    ])
}

fn decode_guard(v: Value) -> Option<ManifestGuard> {
    let Value::Array(items) = v else {
        return None;
    };
    let mut f = Fields::new(items);
    let phase = GuardPhase::from_u64(f.uint()?)?;
    let expr = f.text()?;
    f.end()?;
    Some(ManifestGuard { phase, expr })
}

/// Reconstruct a digest from a 32-byte CBOR byte string (tagged BLAKE3 by convention; the store's real
/// profile is contextual and digest equality ignores the tag).
fn decode_digest(v: Value) -> Option<Digest> {
    let Value::Bytes(bytes) = v else {
        return None;
    };
    let arr: [u8; 32] = bytes.as_slice().try_into().ok()?;
    Some(Digest::from_blake3_bytes(arr))
}

fn decode_grant(v: Value) -> Option<Grant> {
    let Value::Array(items) = v else {
        return None;
    };
    let mut f = Fields::new(items);
    let facet = Capability::from_stable_tag(u8::try_from(f.uint()?).ok()?)?;
    if !is_program_grantable(facet) {
        return None;
    }
    let mode = Mode::from_u8(u8::try_from(f.uint()?).ok()?)?;
    let scopes = f
        .array()?
        .into_iter()
        .map(decode_scope)
        .collect::<Option<Vec<Scope>>>()?;
    f.end()?;
    Some(Grant {
        facet,
        mode,
        scopes,
    })
}

fn decode_scope(v: Value) -> Option<Scope> {
    let Value::Array(items) = v else {
        return None;
    };
    let mut f = Fields::new(items);
    let scope = match f.uint()? {
        SCOPE_ALL => Scope::All,
        SCOPE_PREFIX => Scope::Prefix(f.text()?),
        _ => return None,
    };
    f.end()?;
    Some(scope)
}

/// Consumes a decoded array's positional fields with a per-field type check; [`Fields::end`] rejects
/// any extra trailing field, so a manifest with the wrong arity fails to decode.
struct Fields {
    items: std::vec::IntoIter<Value>,
}

impl Fields {
    fn new(items: Vec<Value>) -> Self {
        Self {
            items: items.into_iter(),
        }
    }

    fn next_field(&mut self) -> Option<Value> {
        self.items.next()
    }

    fn uint(&mut self) -> Option<u64> {
        match self.next_field()? {
            Value::Uint(n) => Some(n),
            _ => None,
        }
    }

    fn text(&mut self) -> Option<String> {
        match self.next_field()? {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }

    fn array(&mut self) -> Option<Vec<Value>> {
        match self.next_field()? {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    fn digest(&mut self) -> Option<Digest> {
        decode_digest(self.next_field()?)
    }

    fn opt_digest(&mut self) -> Option<Option<Digest>> {
        match self.next_field()? {
            Value::Null => Some(None),
            other => Some(Some(decode_digest(other)?)),
        }
    }

    fn end(mut self) -> Option<()> {
        if self.items.next().is_some() {
            None
        } else {
            Some(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trips() {
        let grants = GrantSet::new(vec![
            Grant {
                facet: Capability::Kv,
                scopes: vec![Scope::Prefix("user:".into())],
                mode: Mode::ReadWrite,
            },
            Grant {
                facet: Capability::Files,
                scopes: vec![Scope::All],
                mode: Mode::Read,
            },
        ]);
        let manifest = Manifest::for_wasm("demo", b"\0asm", grants);
        let decoded = Manifest::decode(&manifest.encode()).expect("decode");
        assert_eq!(manifest, decoded);
    }

    #[test]
    fn encoding_is_deterministic() {
        let manifest = Manifest::for_wasm("demo", b"\0asm", GrantSet::all_facets());
        assert_eq!(manifest.encode(), manifest.encode());
    }

    #[test]
    fn round_trips_with_schemas() {
        let mut manifest = Manifest::for_wasm("demo", b"\0asm", GrantSet::all_facets());
        manifest.input_schema = Some(Digest::blake3(b"in"));
        manifest.output_schema = Some(Digest::blake3(b"out"));
        let decoded = Manifest::decode(&manifest.encode()).expect("decode");
        assert_eq!(manifest, decoded);
    }

    #[test]
    fn store_returns_content_address() {
        let mut store = loom_core::MemoryStore::new();
        let manifest = Manifest::for_wasm("demo", b"\0asm", GrantSet::all_facets());
        let digest = manifest.store(&mut store).expect("store");
        assert_eq!(digest, Object::Blob(manifest.encode()).digest());
    }

    // A minimal manifest whose canonical bytes are hand-verifiable: empty strings, abi 0, an all-zero
    // body digest, no schemas, and one grant `Files`(read) over `All`.
    fn pinned_manifest() -> Manifest {
        Manifest {
            name: String::new(),
            engine: String::new(),
            abi_version: 0,
            entry: String::new(),
            grants: GrantSet::new(vec![Grant {
                facet: Capability::Files,
                mode: Mode::Read,
                scopes: vec![Scope::All],
            }]),
            input_schema: None,
            output_schema: None,
            body: Digest::from_blake3_bytes([0u8; 32]),
            guards: Vec::new(),
        }
    }

    #[test]
    fn canonical_byte_vector_is_pinned() {
        // Loom object array [epoch=1, type=1, schema=1, name"", engine"", abi=0, entry"",
        // body(32 zeros), null, null, grants=[[facet=0, mode=0, [[scope_all=0]]]], guards=[]].
        // Twelve elements (0x8C): the ten payload fields plus epoch and type. The empty guards field
        // is the trailing 0x80 (empty array); a program with no guards costs exactly one byte.
        let mut expected = vec![0x8C, 0x01, 0x01, 0x01, 0x60, 0x60, 0x00, 0x60, 0x58, 0x20];
        expected.extend_from_slice(&[0u8; 32]); // body digest bytes
        expected.extend_from_slice(&[0xF6, 0xF6, 0x81, 0x83, 0x00, 0x00, 0x81, 0x81, 0x00, 0x80]);
        assert_eq!(pinned_manifest().encode(), expected);
        assert_eq!(Manifest::decode(&expected), Some(pinned_manifest()));
    }

    #[test]
    fn round_trips_with_guards() {
        let mut manifest = Manifest::for_wasm("demo", b"\0asm", GrantSet::all_facets());
        manifest.guards = vec![
            ManifestGuard {
                phase: GuardPhase::Pre,
                expr: r#"kv.state == "ready""#.into(),
            },
            ManifestGuard {
                phase: GuardPhase::Post,
                expr: "ledger_ok".into(),
            },
        ];
        let decoded = Manifest::decode(&manifest.encode()).expect("decode");
        assert_eq!(manifest, decoded);
        // Guards are part of identity: changing a guard changes the encoded bytes (and so the digest).
        let mut other = manifest.clone();
        other.guards[0].expr = r#"kv.state == "draft""#.into();
        assert_ne!(manifest.encode(), other.encode());
    }

    // Positional fields of a well-formed manifest (schema version plus caller-supplied grants), for
    // building negative cases by mutating exactly one field.
    fn manifest_fields(grants: Vec<Value>) -> [Value; 10] {
        [
            Value::Uint(MANIFEST_SCHEMA_VERSION),
            Value::Text(String::new()),
            Value::Text(String::new()),
            Value::Uint(0),
            Value::Text(String::new()),
            Value::Bytes(vec![0u8; 32]),
            Value::Null,
            Value::Null,
            Value::Array(grants),
            Value::Array(vec![]),
        ]
    }

    fn grant_cbor(facet: u64, mode: u64) -> Value {
        Value::Array(vec![
            Value::Uint(facet),
            Value::Uint(mode),
            Value::Array(vec![Value::Array(vec![Value::Uint(SCOPE_ALL)])]),
        ])
    }

    #[test]
    fn decode_rejects_wrong_type_code() {
        let bytes = loom_codec::encode_object(MANIFEST_TYPE_CODE + 1, &manifest_fields(vec![]))
            .expect("encode");
        assert!(Manifest::decode(&bytes).is_none());
    }

    #[test]
    fn decode_rejects_unknown_schema_version() {
        let mut fields = manifest_fields(vec![]);
        fields[0] = Value::Uint(MANIFEST_SCHEMA_VERSION + 1);
        let bytes = loom_codec::encode_object(MANIFEST_TYPE_CODE, &fields).expect("encode");
        assert!(Manifest::decode(&bytes).is_none());
    }

    #[test]
    fn decode_rejects_non_grantable_facets() {
        for facet in [Capability::Vcs, Capability::Program] {
            let bytes = loom_codec::encode_object(
                MANIFEST_TYPE_CODE,
                &manifest_fields(vec![grant_cbor(u64::from(facet.stable_tag()), 0)]),
            )
            .expect("encode");
            assert!(
                Manifest::decode(&bytes).is_none(),
                "{facet:?} must be rejected on decode"
            );
        }
    }

    #[test]
    fn decode_rejects_trailing_bytes() {
        let mut bytes = pinned_manifest().encode();
        bytes.push(0x00);
        assert!(Manifest::decode(&bytes).is_none());
    }

    #[test]
    fn decode_rejects_truncated_input() {
        let bytes = pinned_manifest().encode();
        assert!(Manifest::decode(&bytes[..bytes.len() - 1]).is_none());
    }

    #[test]
    fn decode_rejects_unknown_facet_tag() {
        let bytes = loom_codec::encode_object(
            MANIFEST_TYPE_CODE,
            &manifest_fields(vec![grant_cbor(200, 0)]),
        )
        .expect("encode");
        assert!(Manifest::decode(&bytes).is_none());
    }

    #[test]
    fn decode_rejects_bad_mode_tag() {
        let bytes =
            loom_codec::encode_object(MANIFEST_TYPE_CODE, &manifest_fields(vec![grant_cbor(0, 9)]))
                .expect("encode");
        assert!(Manifest::decode(&bytes).is_none());
    }

    #[test]
    fn decode_rejects_extra_trailing_field() {
        let mut fields = manifest_fields(vec![]).to_vec();
        fields.push(Value::Uint(0));
        let bytes = loom_codec::encode_object(MANIFEST_TYPE_CODE, &fields).expect("encode");
        assert!(Manifest::decode(&bytes).is_none());
    }
}
