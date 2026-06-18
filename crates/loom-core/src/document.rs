//! The document facet - a versioned value of id-keyed documents (opaque bytes, typically JSON).
//! Pure-Rust, `wasm32`-clean, deterministic. A structured canonical root versions, branches,
//! and syncs through the engine, with large bodies split into canonical chunks.
//!
//! Secondary indexes provide exact-match lookup over declared scalar JSON paths.

use crate::acl::AclRight;
use crate::cbor::{self, Value as CborValue};
use crate::digest::Digest;
use crate::error::{Code, LoomError, Result};
use crate::object::{ChunkRef, EntryKind, Object, TreeEntry};
use crate::provider::ObjectStore;
use crate::tabular::{CmpOp, Value as IndexedValue, cell_from, cell_value};
use crate::vcs::{Loom, StagedEntry, normalize_path};
use crate::workspace::{FacetKind, WorkspaceId, facet_path, facet_root};
use loom_types::{
    CompareCondition, CompareOutcome, ConflictReason, ContentTag, EntityTag, MutationMode,
    MutationRequest,
};
use std::collections::{BTreeMap, BTreeSet};

const INDEX_CATALOG_SCHEMA: &str = "loom.document.index-catalog.v1";
// Canonical single-field JSON-scalar index profile. The document index catalog stores
// `DocumentIndexDeclaration` records as the load-bearing source of truth; physical index
// materializations remain derived. These constants pin the reduced compatibility profile.
const DOCUMENT_INDEX_EXTRACTOR_JSON_SCALAR: &str = "json-scalar";
const DOCUMENT_INDEX_KEY_CODEC_SCALAR: &str = "loom-scalar.v1";
const DOCUMENT_INDEX_COMPARATOR_CANONICAL: &str = "canonical-byte-order";
const DOCUMENT_INDEX_FAILURE_POLICY_SKIP_MISSING: &str = "skip-missing";
const INDEX_STATE_SCHEMA: &str = "loom.document.index-state.v1";
const DOCUMENT_MAP_SCHEMA: &str = "loom.document.map.v1";
const DOCUMENT_TOMBSTONE_SCHEMA: &str = "loom.document.tombstones.v1";
pub const DOCUMENT_COLLECTION_ROOT_FORMAT: &str = "loom.document.collection-root.v1";
pub const DOCUMENT_CHUNK_THRESHOLD: usize = crate::chunk::CHUNK_THRESHOLD;
const DOCUMENT_RETENTION_POLICY_NONE: &str = "no-retained-tombstones.v1";
const DOCUMENT_RETENTION_POLICY_RETAIN: &str = "retain-tombstones.v1";
const DOCUMENT_TOMBSTONE_RETENTION_CLASS: &str = "retained-delete.v1";
const DOCUMENT_DELETION_REVISION_SCHEMA: &str = "loom.document.deletion-revision.v1";
const DOCUMENT_ROOT_MANIFEST_ENTRY: &str = "manifest";
const DOCUMENT_ROOT_DOCUMENTS_ENTRY: &str = "documents";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentId {
    String(String),
    Generated { value: String, generator: String },
    External { system: String, value: String },
    Partitioned { partition: String, local_id: String },
}

impl DocumentId {
    pub fn string(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_document_text_field("document id", &value)?;
        Ok(Self::String(value))
    }

    pub fn generated(value: impl Into<String>, generator: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let generator = generator.into();
        validate_document_text_field("generated document id", &value)?;
        validate_document_text_field("document id generator", &generator)?;
        Ok(Self::Generated { value, generator })
    }

    pub fn external(system: impl Into<String>, value: impl Into<String>) -> Result<Self> {
        let system = system.into();
        let value = value.into();
        validate_document_text_field("external document id system", &system)?;
        validate_document_text_field("external document id", &value)?;
        Ok(Self::External { system, value })
    }

    pub fn partitioned(partition: impl Into<String>, local_id: impl Into<String>) -> Result<Self> {
        let partition = partition.into();
        let local_id = local_id.into();
        validate_document_text_field("partitioned document id partition", &partition)?;
        validate_document_text_field("partitioned document id", &local_id)?;
        Ok(Self::Partitioned {
            partition,
            local_id,
        })
    }

    pub fn canonical_key(&self) -> Vec<u8> {
        cbor::encode(&self.to_cbor())
    }

    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&self.to_cbor())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_cbor(cbor::decode(bytes)?)
    }

    fn to_cbor(&self) -> CborValue {
        match self {
            Self::String(value) => CborValue::Array(vec![
                CborValue::Text("string".to_string()),
                CborValue::Text(value.clone()),
            ]),
            Self::Generated { value, generator } => CborValue::Array(vec![
                CborValue::Text("generated".to_string()),
                CborValue::Text(value.clone()),
                CborValue::Text(generator.clone()),
            ]),
            Self::External { system, value } => CborValue::Array(vec![
                CborValue::Text("external".to_string()),
                CborValue::Text(system.clone()),
                CborValue::Text(value.clone()),
            ]),
            Self::Partitioned {
                partition,
                local_id,
            } => CborValue::Array(vec![
                CborValue::Text("partitioned".to_string()),
                CborValue::Text(partition.clone()),
                CborValue::Text(local_id.clone()),
            ]),
        }
    }

    pub(crate) fn from_cbor(value: CborValue) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::as_array(value)?);
        let kind = fields.text()?;
        let id = match kind.as_str() {
            "string" => Self::string(fields.text()?)?,
            "generated" => Self::generated(fields.text()?, fields.text()?)?,
            "external" => Self::external(fields.text()?, fields.text()?)?,
            "partitioned" => Self::partitioned(fields.text()?, fields.text()?)?,
            other => {
                return Err(crate::LoomError::corrupt(format!(
                    "unknown document id kind {other}"
                )));
            }
        };
        fields.end()?;
        Ok(id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentBodyRef {
    Direct { digest: Digest },
    Chunked { root: Digest },
}

impl DocumentBodyRef {
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&self.to_cbor())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_cbor(cbor::decode(bytes)?)
    }

    fn to_cbor(&self) -> CborValue {
        match self {
            Self::Direct { digest } => CborValue::Array(vec![
                CborValue::Text("direct".to_string()),
                cbor::digest_value(digest),
            ]),
            Self::Chunked { root } => CborValue::Array(vec![
                CborValue::Text("chunked".to_string()),
                cbor::digest_value(root),
            ]),
        }
    }

    pub(crate) fn from_cbor(value: CborValue) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::as_array(value)?);
        let kind = fields.text()?;
        let body_ref = match kind.as_str() {
            "direct" => Self::Direct {
                digest: fields.digest()?,
            },
            "chunked" => Self::Chunked {
                root: fields.digest()?,
            },
            other => {
                return Err(crate::LoomError::corrupt(format!(
                    "unknown document body ref kind {other}"
                )));
            }
        };
        fields.end()?;
        Ok(body_ref)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentRecordState {
    Live,
    Deleted,
}

impl DocumentRecordState {
    fn to_cbor(&self) -> CborValue {
        CborValue::Text(
            match self {
                Self::Live => "live",
                Self::Deleted => "deleted",
            }
            .to_string(),
        )
    }

    pub(crate) fn from_cbor(value: CborValue) -> Result<Self> {
        match cbor::as_text(value)?.as_str() {
            "live" => Ok(Self::Live),
            "deleted" => Ok(Self::Deleted),
            other => Err(crate::LoomError::corrupt(format!(
                "unknown document record state {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentRecord {
    pub document_id: DocumentId,
    pub body_ref: DocumentBodyRef,
    pub byte_length: u64,
    pub entity_tag: String,
    pub record_revision: String,
    pub record_state: DocumentRecordState,
    pub media_type: Option<String>,
    pub charset: Option<String>,
    pub content_encoding: Option<String>,
    pub source_metadata: BTreeMap<String, CborValue>,
    pub policy_flags: Vec<String>,
}

impl DocumentRecord {
    pub fn new(
        document_id: DocumentId,
        body_ref: DocumentBodyRef,
        byte_length: u64,
        entity_tag: impl Into<String>,
        record_revision: impl Into<String>,
        record_state: DocumentRecordState,
    ) -> Result<Self> {
        let entity_tag = entity_tag.into();
        let record_revision = record_revision.into();
        validate_document_text_field("document entity tag", &entity_tag)?;
        validate_document_text_field("document record revision", &record_revision)?;
        Ok(Self {
            document_id,
            body_ref,
            byte_length,
            entity_tag,
            record_revision,
            record_state,
            media_type: None,
            charset: None,
            content_encoding: None,
            source_metadata: BTreeMap::new(),
            policy_flags: Vec::new(),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&self.to_cbor())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_cbor(cbor::decode(bytes)?)
    }

    fn to_cbor(&self) -> CborValue {
        CborValue::Array(vec![
            self.document_id.to_cbor(),
            self.body_ref.to_cbor(),
            CborValue::Uint(self.byte_length),
            CborValue::Text(self.entity_tag.clone()),
            CborValue::Text(self.record_revision.clone()),
            self.record_state.to_cbor(),
            optional_text_to_cbor(&self.media_type),
            optional_text_to_cbor(&self.charset),
            optional_text_to_cbor(&self.content_encoding),
            text_map_to_cbor(&self.source_metadata),
            string_list_to_cbor(&self.policy_flags),
        ])
    }

    pub(crate) fn from_cbor(value: CborValue) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::as_array(value)?);
        let record = Self {
            document_id: DocumentId::from_cbor(fields.next_field()?)?,
            body_ref: DocumentBodyRef::from_cbor(fields.next_field()?)?,
            byte_length: fields.uint()?,
            entity_tag: fields.text()?,
            record_revision: fields.text()?,
            record_state: DocumentRecordState::from_cbor(fields.next_field()?)?,
            media_type: optional_text_from_cbor(fields.next_field()?)?,
            charset: optional_text_from_cbor(fields.next_field()?)?,
            content_encoding: optional_text_from_cbor(fields.next_field()?)?,
            source_metadata: text_map_from_cbor(fields.next_field()?)?,
            policy_flags: string_list_from_cbor(fields.next_field()?)?,
        };
        fields.end()?;
        validate_document_text_field("document entity tag", &record.entity_tag)?;
        validate_document_text_field("document record revision", &record.record_revision)?;
        Ok(record)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentTombstoneRecord {
    pub document_id: DocumentId,
    pub deleted_entity_tag: String,
    pub prior_entity_tag: String,
    pub deletion_revision: String,
    pub retention_class: String,
    pub reclaim_after: Option<String>,
    pub deletion_reason: Option<String>,
    pub source_metadata: BTreeMap<String, CborValue>,
}

impl DocumentTombstoneRecord {
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&self.to_cbor())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_cbor(cbor::decode(bytes)?)
    }

    fn to_cbor(&self) -> CborValue {
        CborValue::Array(vec![
            self.document_id.to_cbor(),
            CborValue::Text(self.deleted_entity_tag.clone()),
            CborValue::Text(self.prior_entity_tag.clone()),
            CborValue::Text(self.deletion_revision.clone()),
            CborValue::Text(self.retention_class.clone()),
            optional_text_to_cbor(&self.reclaim_after),
            optional_text_to_cbor(&self.deletion_reason),
            text_map_to_cbor(&self.source_metadata),
        ])
    }

    fn from_cbor(value: CborValue) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::as_array(value)?);
        let tombstone = Self {
            document_id: DocumentId::from_cbor(fields.next_field()?)?,
            deleted_entity_tag: fields.text()?,
            prior_entity_tag: fields.text()?,
            deletion_revision: fields.text()?,
            retention_class: fields.text()?,
            reclaim_after: optional_text_from_cbor(fields.next_field()?)?,
            deletion_reason: optional_text_from_cbor(fields.next_field()?)?,
            source_metadata: text_map_from_cbor(fields.next_field()?)?,
        };
        fields.end()?;
        validate_document_text_field("deleted document entity tag", &tombstone.deleted_entity_tag)?;
        validate_document_text_field("prior document entity tag", &tombstone.prior_entity_tag)?;
        validate_document_text_field("document deletion revision", &tombstone.deletion_revision)?;
        validate_document_text_field("document retention class", &tombstone.retention_class)?;
        Ok(tombstone)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentIndexUniqueness {
    NonUnique,
    Unique,
}

impl DocumentIndexUniqueness {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NonUnique => "non_unique",
            Self::Unique => "unique",
        }
    }

    pub fn from_label(value: &str) -> Result<Self> {
        match value {
            "non_unique" | "non-unique" | "nonUnique" => Ok(Self::NonUnique),
            "unique" => Ok(Self::Unique),
            other => Err(crate::LoomError::invalid(format!(
                "unknown document index uniqueness {other}"
            ))),
        }
    }

    fn to_cbor(&self) -> CborValue {
        CborValue::Text(self.as_str().to_string())
    }

    fn from_cbor(value: CborValue) -> Result<Self> {
        Self::from_label(&cbor::as_text(value)?).map_err(|error| LoomError::corrupt(error.message))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentIndexDeclaration {
    pub index_id: String,
    pub index_name: String,
    pub source_selector: DocumentFieldPath,
    pub extractor: String,
    pub key_codec: String,
    pub comparator: String,
    pub uniqueness: DocumentIndexUniqueness,
    pub failure_policy: String,
    pub declaration_version: u64,
    pub analyzer_profile: Option<String>,
    pub projection: Option<DocumentFieldPath>,
    pub partial_filter: Option<String>,
    pub metadata: BTreeMap<String, CborValue>,
}

impl DocumentIndexDeclaration {
    pub fn new_single_field_json_scalar(
        name: impl Into<String>,
        path: DocumentFieldPath,
        unique: bool,
    ) -> Result<Self> {
        let def = DocumentIndexDef::new(name, path, unique)?;
        Ok(declaration_from_def(&def))
    }

    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&self.to_cbor())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_cbor(cbor::decode(bytes)?)
    }

    fn to_cbor(&self) -> CborValue {
        CborValue::Array(vec![
            CborValue::Text(self.index_id.clone()),
            CborValue::Text(self.index_name.clone()),
            self.source_selector.to_cbor(),
            CborValue::Text(self.extractor.clone()),
            CborValue::Text(self.key_codec.clone()),
            CborValue::Text(self.comparator.clone()),
            self.uniqueness.to_cbor(),
            CborValue::Text(self.failure_policy.clone()),
            CborValue::Uint(self.declaration_version),
            optional_text_to_cbor(&self.analyzer_profile),
            optional_path_to_cbor(&self.projection),
            optional_text_to_cbor(&self.partial_filter),
            text_map_to_cbor(&self.metadata),
        ])
    }

    fn from_cbor(value: CborValue) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::as_array(value)?);
        let declaration = Self {
            index_id: fields.text()?,
            index_name: fields.text()?,
            source_selector: DocumentFieldPath::from_cbor(fields.next_field()?)?,
            extractor: fields.text()?,
            key_codec: fields.text()?,
            comparator: fields.text()?,
            uniqueness: DocumentIndexUniqueness::from_cbor(fields.next_field()?)?,
            failure_policy: fields.text()?,
            declaration_version: fields.uint()?,
            analyzer_profile: optional_text_from_cbor(fields.next_field()?)?,
            projection: optional_path_from_cbor(fields.next_field()?)?,
            partial_filter: optional_text_from_cbor(fields.next_field()?)?,
            metadata: text_map_from_cbor(fields.next_field()?)?,
        };
        fields.end()?;
        validate_document_text_field("document index id", &declaration.index_id)?;
        validate_index_name(&declaration.index_name)?;
        validate_document_text_field("document index extractor", &declaration.extractor)?;
        validate_document_text_field("document index key codec", &declaration.key_codec)?;
        validate_document_text_field("document index comparator", &declaration.comparator)?;
        validate_document_text_field("document index failure policy", &declaration.failure_policy)?;
        Ok(declaration)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentPolicyConfig {
    pub name: String,
    pub parameters: BTreeMap<String, CborValue>,
}

impl DocumentPolicyConfig {
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        validate_document_text_field("document policy name", &name)?;
        Ok(Self {
            name,
            parameters: BTreeMap::new(),
        })
    }

    fn to_cbor(&self) -> CborValue {
        CborValue::Array(vec![
            CborValue::Text(self.name.clone()),
            text_map_to_cbor(&self.parameters),
        ])
    }

    fn from_cbor(value: CborValue) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::as_array(value)?);
        let config = Self {
            name: fields.text()?,
            parameters: text_map_from_cbor(fields.next_field()?)?,
        };
        fields.end()?;
        validate_document_text_field("document policy name", &config.name)?;
        Ok(config)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DocumentCollectionPolicy {
    Digest(Digest),
    Inline(BTreeMap<String, CborValue>),
}

impl DocumentCollectionPolicy {
    fn to_cbor(&self) -> CborValue {
        match self {
            Self::Digest(digest) => CborValue::Array(vec![
                CborValue::Text("digest".to_string()),
                cbor::digest_value(digest),
            ]),
            Self::Inline(policy) => CborValue::Array(vec![
                CborValue::Text("inline".to_string()),
                text_map_to_cbor(policy),
            ]),
        }
    }

    fn from_cbor(value: CborValue) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::as_array(value)?);
        let kind = fields.text()?;
        let policy = match kind.as_str() {
            "digest" => Self::Digest(fields.digest()?),
            "inline" => Self::Inline(text_map_from_cbor(fields.next_field()?)?),
            other => {
                return Err(crate::LoomError::corrupt(format!(
                    "unknown document collection policy kind {other}"
                )));
            }
        };
        fields.end()?;
        Ok(policy)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentCollectionManifest {
    pub schema_version: u64,
    pub collection_id: String,
    pub id_codec: DocumentPolicyConfig,
    pub document_map_root: Digest,
    pub index_catalog_root: Digest,
    pub tombstone_root: Option<Digest>,
    pub policy: DocumentCollectionPolicy,
    pub merge_policy: DocumentPolicyConfig,
    pub retention_policy: DocumentPolicyConfig,
    pub capabilities: Vec<String>,
    pub metadata: BTreeMap<String, CborValue>,
}

impl DocumentCollectionManifest {
    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&self.to_cbor())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_cbor(cbor::decode(bytes)?)
    }

    fn to_cbor(&self) -> CborValue {
        CborValue::Array(vec![
            CborValue::Text(DOCUMENT_COLLECTION_ROOT_FORMAT.to_string()),
            CborValue::Uint(self.schema_version),
            CborValue::Text(self.collection_id.clone()),
            self.id_codec.to_cbor(),
            cbor::digest_value(&self.document_map_root),
            cbor::digest_value(&self.index_catalog_root),
            optional_digest_to_cbor(&self.tombstone_root),
            self.policy.to_cbor(),
            self.merge_policy.to_cbor(),
            self.retention_policy.to_cbor(),
            string_list_to_cbor(&self.capabilities),
            text_map_to_cbor(&self.metadata),
        ])
    }

    fn from_cbor(value: CborValue) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::as_array(value)?);
        let format = fields.text()?;
        if format != DOCUMENT_COLLECTION_ROOT_FORMAT {
            return Err(crate::LoomError::corrupt(
                "unknown document collection root format",
            ));
        }
        let manifest = Self {
            schema_version: fields.uint()?,
            collection_id: fields.text()?,
            id_codec: DocumentPolicyConfig::from_cbor(fields.next_field()?)?,
            document_map_root: fields.digest()?,
            index_catalog_root: fields.digest()?,
            tombstone_root: optional_digest_from_cbor(fields.next_field()?)?,
            policy: DocumentCollectionPolicy::from_cbor(fields.next_field()?)?,
            merge_policy: DocumentPolicyConfig::from_cbor(fields.next_field()?)?,
            retention_policy: DocumentPolicyConfig::from_cbor(fields.next_field()?)?,
            capabilities: string_list_from_cbor(fields.next_field()?)?,
            metadata: text_map_from_cbor(fields.next_field()?)?,
        };
        fields.end()?;
        validate_document_text_field("document collection id", &manifest.collection_id)?;
        Ok(manifest)
    }
}

/// A versioned document value: documents (opaque bytes) keyed by string id, in id order.
#[derive(Debug, Clone, Default)]
pub struct Collection {
    docs: BTreeMap<String, Vec<u8>>,
}

impl Collection {
    /// An empty value.
    pub fn new() -> Self {
        Self::default()
    }
    /// Number of documents.
    pub fn len(&self) -> usize {
        self.docs.len()
    }
    /// Whether the value is empty.
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
    /// Insert or replace the document at `id`.
    pub fn put(&mut self, id: impl Into<String>, doc: Vec<u8>) {
        self.docs.insert(id.into(), doc);
    }
    /// The document at `id`.
    pub fn get(&self, id: &str) -> Option<&[u8]> {
        self.docs.get(id).map(Vec::as_slice)
    }
    /// Remove `id`; returns whether it was present.
    pub fn delete(&mut self, id: &str) -> bool {
        self.docs.remove(id).is_some()
    }
    /// Document ids in order.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.docs.keys().map(String::as_str)
    }
    /// `(id, document)` pairs in id order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.docs.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    /// Canonical bytes: documents in id order. Deterministic.
    pub fn encode(&self) -> Vec<u8> {
        let items = self
            .docs
            .iter()
            .map(|(id, doc)| {
                CborValue::Array(vec![
                    CborValue::Text(id.clone()),
                    CborValue::Bytes(doc.clone()),
                ])
            })
            .collect();
        cbor::encode(&CborValue::Array(items))
    }
    /// Parse a value from [`Collection::encode`] output.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut col = Collection::new();
        for item in cbor::decode_array(bytes)? {
            let mut f = cbor::Fields::new(cbor::as_array(item)?);
            let id = f.text()?;
            let doc = f.bytes()?;
            f.end()?;
            col.docs.insert(id, doc);
        }
        Ok(col)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentText {
    pub text: String,
    pub digest: Digest,
    pub entity_tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentBinary {
    pub bytes: Vec<u8>,
    pub digest: Digest,
    pub entity_tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentPutResult {
    pub digest: Digest,
    pub entity_tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentMutationResult {
    pub digest: Digest,
    pub outcome: CompareOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentFieldPathSegment {
    Field(String),
    Index(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentFieldPath {
    segments: Vec<DocumentFieldPathSegment>,
}

impl DocumentFieldPath {
    pub fn new(segments: Vec<DocumentFieldPathSegment>) -> Result<Self> {
        if segments.is_empty() {
            return Err(crate::LoomError::invalid("document field path is empty"));
        }
        for segment in &segments {
            if let DocumentFieldPathSegment::Field(field) = segment {
                validate_field_path_name(field)?;
            }
        }
        Ok(Self { segments })
    }

    pub fn dotted(path: &str) -> Result<Self> {
        if path.is_empty() {
            return Err(crate::LoomError::invalid("document field path is empty"));
        }
        let segments = path
            .split('.')
            .map(|part| {
                validate_field_path_name(part)?;
                Ok(DocumentFieldPathSegment::Field(part.to_string()))
            })
            .collect::<Result<Vec<_>>>()?;
        Self::new(segments)
    }

    pub fn segments(&self) -> &[DocumentFieldPathSegment] {
        &self.segments
    }

    fn to_cbor(&self) -> CborValue {
        CborValue::Array(
            self.segments
                .iter()
                .map(|segment| match segment {
                    DocumentFieldPathSegment::Field(field) => {
                        CborValue::Array(vec![CborValue::Uint(0), CborValue::Text(field.clone())])
                    }
                    DocumentFieldPathSegment::Index(index) => {
                        CborValue::Array(vec![CborValue::Uint(1), CborValue::Uint(*index)])
                    }
                })
                .collect(),
        )
    }

    fn from_cbor(value: CborValue) -> Result<Self> {
        let segments = cbor::as_array(value)?
            .into_iter()
            .map(|segment| {
                let mut fields = cbor::Fields::new(cbor::as_array(segment)?);
                let segment = match fields.uint()? {
                    0 => DocumentFieldPathSegment::Field(fields.text()?),
                    1 => DocumentFieldPathSegment::Index(fields.uint()?),
                    other => {
                        return Err(crate::LoomError::corrupt(format!(
                            "unknown document field path segment {other}"
                        )));
                    }
                };
                fields.end()?;
                Ok(segment)
            })
            .collect::<Result<Vec<_>>>()?;
        Self::new(segments)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentIndexDef {
    pub name: String,
    pub path: DocumentFieldPath,
    pub unique: bool,
}

impl DocumentIndexDef {
    pub fn new(name: impl Into<String>, path: DocumentFieldPath, unique: bool) -> Result<Self> {
        let name = name.into();
        validate_index_name(&name)?;
        Ok(Self { name, path, unique })
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocumentIndexCatalog {
    declarations: Vec<DocumentIndexDeclaration>,
}

impl DocumentIndexCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Canonical index declarations in index-name order. This is the load-bearing catalog: the
    /// collection manifest's `index_catalog_root` digests exactly these encoded bytes.
    pub fn declarations(&self) -> &[DocumentIndexDeclaration] {
        &self.declarations
    }

    /// Reduced `DocumentIndexDef` projection of the canonical declarations. This is a surface
    /// projection for engine query/build paths and callers that do not need the full declaration.
    pub fn indexes(&self) -> Vec<DocumentIndexDef> {
        self.declarations.iter().map(def_from_declaration).collect()
    }

    pub fn index(&self, name: &str) -> Option<DocumentIndexDef> {
        self.declaration(name).map(def_from_declaration)
    }

    pub fn declaration(&self, name: &str) -> Option<&DocumentIndexDeclaration> {
        self.declarations
            .iter()
            .find(|declaration| declaration.index_name == name)
    }

    pub fn insert(&mut self, index: DocumentIndexDef) -> Result<()> {
        self.insert_declaration(declaration_from_def(&index))
    }

    pub fn insert_declaration(&mut self, declaration: DocumentIndexDeclaration) -> Result<()> {
        if self.declaration(&declaration.index_name).is_some() {
            return Err(crate::LoomError::invalid(format!(
                "duplicate document index {:?}",
                declaration.index_name
            )));
        }
        self.declarations.push(declaration);
        self.declarations
            .sort_by(|a, b| a.index_name.cmp(&b.index_name));
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.declarations.len();
        self.declarations
            .retain(|declaration| declaration.index_name != name);
        self.declarations.len() != before
    }

    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&CborValue::Array(vec![
            CborValue::Text(INDEX_CATALOG_SCHEMA.to_string()),
            CborValue::Array(
                self.declarations
                    .iter()
                    .map(DocumentIndexDeclaration::to_cbor)
                    .collect(),
            ),
        ]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
        let schema = fields.text()?;
        if schema != INDEX_CATALOG_SCHEMA {
            return Err(crate::LoomError::corrupt(
                "unknown document index catalog schema",
            ));
        }
        let raw_declarations = fields.array()?;
        fields.end()?;
        let mut catalog = Self::new();
        for raw in raw_declarations {
            catalog.insert_declaration(DocumentIndexDeclaration::from_cbor(raw)?)?;
        }
        Ok(catalog)
    }
}

/// Build the canonical declaration for the reduced `DocumentIndexDef` surface input using the
/// current single-field JSON-scalar profile. The field values must match the `index-declaration`
/// conformance vector.
fn declaration_from_def(def: &DocumentIndexDef) -> DocumentIndexDeclaration {
    DocumentIndexDeclaration {
        index_id: def.name.clone(),
        index_name: def.name.clone(),
        source_selector: def.path.clone(),
        extractor: DOCUMENT_INDEX_EXTRACTOR_JSON_SCALAR.to_string(),
        key_codec: DOCUMENT_INDEX_KEY_CODEC_SCALAR.to_string(),
        comparator: DOCUMENT_INDEX_COMPARATOR_CANONICAL.to_string(),
        uniqueness: if def.unique {
            DocumentIndexUniqueness::Unique
        } else {
            DocumentIndexUniqueness::NonUnique
        },
        failure_policy: DOCUMENT_INDEX_FAILURE_POLICY_SKIP_MISSING.to_string(),
        declaration_version: 1,
        analyzer_profile: None,
        projection: None,
        partial_filter: None,
        metadata: BTreeMap::new(),
    }
}

fn def_from_declaration(declaration: &DocumentIndexDeclaration) -> DocumentIndexDef {
    DocumentIndexDef {
        name: declaration.index_name.clone(),
        path: declaration.source_selector.clone(),
        unique: matches!(declaration.uniqueness, DocumentIndexUniqueness::Unique),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentIndexStatus {
    pub name: String,
    pub ready: bool,
    pub entries: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentProjection {
    pub name: String,
    pub path: DocumentFieldPath,
}

impl DocumentProjection {
    pub fn new(name: impl Into<String>, path: DocumentFieldPath) -> Result<Self> {
        let name = name.into();
        validate_index_name(&name)?;
        Ok(Self { name, path })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DocumentPredicate {
    Compare {
        path: DocumentFieldPath,
        op: CmpOp,
        value: IndexedValue,
    },
    And(Vec<DocumentPredicate>),
    Or(Vec<DocumentPredicate>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentQuery {
    pub predicate: Option<DocumentPredicate>,
    pub projections: Vec<DocumentProjection>,
    pub cursor: Option<String>,
    pub limit: usize,
    pub include_document: bool,
}

impl DocumentQuery {
    pub fn new() -> Self {
        Self {
            predicate: None,
            projections: Vec::new(),
            cursor: None,
            limit: 100,
            include_document: false,
        }
    }
}

impl Default for DocumentQuery {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentQueryItem {
    pub id: String,
    pub document: Option<Vec<u8>>,
    pub projections: BTreeMap<String, Option<IndexedValue>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentQueryResult {
    pub items: Vec<DocumentQueryItem>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DocumentIndexState {
    entries: BTreeMap<IndexedValue, BTreeSet<String>>,
}

impl DocumentIndexState {
    fn insert(&mut self, value: IndexedValue, id: &str, unique: bool) -> Result<()> {
        let ids = self.entries.entry(value).or_default();
        if unique && !ids.is_empty() && !ids.contains(id) {
            return Err(crate::LoomError::new(
                Code::Conflict,
                "document index unique constraint violation",
            ));
        }
        ids.insert(id.to_string());
        Ok(())
    }

    fn remove(&mut self, value: &IndexedValue, id: &str) {
        if let Some(ids) = self.entries.get_mut(value) {
            ids.remove(id);
            if ids.is_empty() {
                self.entries.remove(value);
            }
        }
    }

    fn find(&self, value: &IndexedValue) -> Vec<String> {
        self.entries
            .get(value)
            .map(|ids| ids.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn find_cmp(&self, op: CmpOp, value: &IndexedValue) -> BTreeSet<String> {
        match op {
            CmpOp::Eq => self.entries.get(value).cloned().unwrap_or_default(),
            CmpOp::Ne => self
                .entries
                .iter()
                .filter(|(entry_value, _)| *entry_value != value)
                .flat_map(|(_, ids)| ids.iter().cloned())
                .collect(),
            CmpOp::Lt => self
                .entries
                .range(..value)
                .flat_map(|(_, ids)| ids.iter().cloned())
                .collect(),
            CmpOp::Le => self
                .entries
                .range(..=value)
                .flat_map(|(_, ids)| ids.iter().cloned())
                .collect(),
            CmpOp::Gt => self
                .entries
                .range((std::ops::Bound::Excluded(value), std::ops::Bound::Unbounded))
                .flat_map(|(_, ids)| ids.iter().cloned())
                .collect(),
            CmpOp::Ge => self
                .entries
                .range(value..)
                .flat_map(|(_, ids)| ids.iter().cloned())
                .collect(),
        }
    }

    fn encode(&self) -> Vec<u8> {
        cbor::encode(&CborValue::Array(vec![
            CborValue::Text(INDEX_STATE_SCHEMA.to_string()),
            CborValue::Array(
                self.entries
                    .iter()
                    .map(|(value, ids)| {
                        CborValue::Array(vec![
                            cell_value(value),
                            CborValue::Array(ids.iter().cloned().map(CborValue::Text).collect()),
                        ])
                    })
                    .collect(),
            ),
        ]))
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
        let schema = fields.text()?;
        if schema != INDEX_STATE_SCHEMA {
            return Err(crate::LoomError::corrupt(
                "unknown document index state schema",
            ));
        }
        let raw_entries = fields.array()?;
        fields.end()?;
        let mut state = Self::default();
        for raw in raw_entries {
            let mut entry = cbor::Fields::new(cbor::as_array(raw)?);
            let value = cell_from(entry.next_field()?)?;
            let ids = entry
                .array()?
                .into_iter()
                .map(cbor::as_text)
                .collect::<Result<BTreeSet<_>>>()?;
            entry.end()?;
            state.entries.insert(value, ids);
        }
        Ok(state)
    }
}

fn col_path(collection: &str) -> String {
    facet_path(FacetKind::Document, collection)
}

fn collection_key(collection: &str) -> String {
    hex::encode(collection.as_bytes())
}

fn document_map_path(collection: &str) -> String {
    facet_path(
        FacetKind::Document,
        &format!(".maps/{}", collection_key(collection)),
    )
}

fn document_tombstone_path(collection: &str) -> String {
    facet_path(
        FacetKind::Document,
        &format!(".tombstones/{}", collection_key(collection)),
    )
}

fn document_body_dir(collection: &str) -> String {
    facet_path(
        FacetKind::Document,
        &format!(".bodies/{}", collection_key(collection)),
    )
}

fn document_body_path(collection: &str, digest: &Digest) -> String {
    facet_path(
        FacetKind::Document,
        &format!(".bodies/{}/{}", collection_key(collection), digest.to_hex()),
    )
}

fn document_chunk_dir(collection: &str) -> String {
    facet_path(
        FacetKind::Document,
        &format!(".chunks/{}", collection_key(collection)),
    )
}

fn document_chunk_path(collection: &str, digest: &Digest) -> String {
    facet_path(
        FacetKind::Document,
        &format!(".chunks/{}/{}", collection_key(collection), digest.to_hex()),
    )
}

fn index_catalog_path(collection: &str) -> String {
    facet_path(
        FacetKind::Document,
        &format!(".indexes/{}", hex::encode(collection.as_bytes())),
    )
}

fn index_state_path(collection: &str, index: &str) -> String {
    facet_path(
        FacetKind::Document,
        &format!(
            ".index-data/{}/{}",
            hex::encode(collection.as_bytes()),
            hex::encode(index.as_bytes())
        ),
    )
}

pub fn put_collection<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    value: &Collection,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Write)?;
    let states = rebuild_declared_index_states_unchecked(loom, ns, collection, value)?;
    put_collection_unchecked(loom, ns, collection, value)?;
    write_index_states_unchecked(loom, ns, collection, states)
}

fn put_collection_unchecked<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    value: &Collection,
) -> Result<()> {
    let existing_manifest = load_collection_manifest_unchecked(loom, ns, collection)?;
    let retention_policy = existing_manifest
        .as_ref()
        .map(|manifest| manifest.retention_policy.clone())
        .unwrap_or(DocumentPolicyConfig::new(DOCUMENT_RETENTION_POLICY_NONE)?);
    let tombstones = existing_manifest
        .as_ref()
        .and_then(|manifest| manifest.tombstone_root)
        .map(|root| load_document_tombstones(loom, ns, collection, root))
        .transpose()?
        .unwrap_or_default();
    put_collection_with_tombstones_unchecked(
        loom,
        ns,
        collection,
        value,
        retention_policy,
        tombstones,
    )
}

fn put_collection_with_tombstones_unchecked<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    value: &Collection,
    retention_policy: DocumentPolicyConfig,
    tombstones: BTreeMap<String, DocumentTombstoneRecord>,
) -> Result<()> {
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Document), true)?;
    loom.create_directory_reserved(ns, &facet_path(FacetKind::Document, ".maps"), true)?;
    loom.create_directory_reserved(ns, &facet_path(FacetKind::Document, ".tombstones"), true)?;
    loom.create_directory_reserved(ns, &document_body_dir(collection), true)?;
    loom.create_directory_reserved(ns, &document_chunk_dir(collection), true)?;
    let mut bodies = BTreeMap::new();
    for (_, doc) in value.iter() {
        let body = write_document_body(loom, ns, collection, doc)?;
        bodies.insert(doc.to_vec(), body);
    }
    let (document_map_root, prolly_root) = build_document_map_root(loom, value, &bodies)?;
    let index_catalog = load_index_catalog_unchecked(loom, ns, collection)?;
    let index_catalog_bytes = index_catalog.encode();
    let tombstone_root =
        write_document_tombstones(loom, ns, collection, &retention_policy, &tombstones)?;
    let manifest = DocumentCollectionManifest {
        schema_version: 1,
        collection_id: collection.to_string(),
        id_codec: DocumentPolicyConfig::new("document-id-envelope.v1")?,
        document_map_root,
        index_catalog_root: Digest::blake3(&index_catalog_bytes),
        tombstone_root,
        policy: DocumentCollectionPolicy::Inline(BTreeMap::new()),
        merge_policy: DocumentPolicyConfig::new("replace-document.v1")?,
        retention_policy,
        capabilities: vec!["text-access".to_string(), "binary-access".to_string()],
        metadata: BTreeMap::new(),
    };
    cleanup_unreferenced_document_body_components(loom, ns, collection, &bodies)?;
    stage_document_root_unchecked(loom, ns, collection, &manifest, prolly_root)
}

#[derive(Clone)]
struct StoredDocumentBody {
    body_ref: DocumentBodyRef,
    content_digest: Digest,
    component_digests: Vec<Digest>,
}

fn write_document_body<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    doc: &[u8],
) -> Result<StoredDocumentBody> {
    let content_digest = document_digest(loom, doc);
    if doc.len() <= DOCUMENT_CHUNK_THRESHOLD {
        loom.write_file_reserved(
            ns,
            &document_body_path(collection, &content_digest),
            doc,
            0o100644,
        )?;
        return Ok(StoredDocumentBody {
            body_ref: DocumentBodyRef::Direct {
                digest: content_digest,
            },
            content_digest,
            component_digests: Vec::new(),
        });
    }

    let mut entries = Vec::new();
    let mut component_digests = Vec::new();
    for piece in crate::chunk::chunk(doc) {
        let chunk = Object::Blob(piece.to_vec());
        let digest = chunk.digest_with(loom.store().digest_algo());
        loom.write_file_reserved(
            ns,
            &document_chunk_path(collection, &digest),
            piece,
            0o100644,
        )?;
        entries.push(ChunkRef {
            target: digest,
            size: piece.len() as u64,
        });
        component_digests.push(digest);
    }
    let chunk_list = Object::ChunkList {
        total_size: doc.len() as u64,
        entries,
    };
    let root = chunk_list.digest_with(loom.store().digest_algo());
    loom.write_file_reserved(
        ns,
        &document_body_path(collection, &root),
        &chunk_list.canonical(),
        0o100644,
    )?;
    Ok(StoredDocumentBody {
        body_ref: DocumentBodyRef::Chunked { root },
        content_digest,
        component_digests,
    })
}

fn cleanup_unreferenced_document_body_components<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    bodies: &BTreeMap<Vec<u8>, StoredDocumentBody>,
) -> Result<()> {
    let keep_bodies = bodies
        .values()
        .map(|body| match body.body_ref {
            DocumentBodyRef::Direct { digest } => digest,
            DocumentBodyRef::Chunked { root } => root,
        })
        .map(|digest| digest.to_hex())
        .collect::<BTreeSet<_>>();
    let prefix = format!("{}/", document_body_dir(collection));
    for path in loom.staged_paths(ns) {
        let Some(name) = path.strip_prefix(&prefix) else {
            continue;
        };
        if name.contains('/') || keep_bodies.contains(name) {
            continue;
        }
        match loom.remove_file_reserved(ns, &path) {
            Ok(()) => {}
            Err(error) if error.code == Code::NotFound => {}
            Err(error) => return Err(error),
        }
    }

    let keep_chunks = bodies
        .values()
        .flat_map(|body| body.component_digests.iter())
        .map(Digest::to_hex)
        .collect::<BTreeSet<_>>();
    let prefix = format!("{}/", document_chunk_dir(collection));
    for path in loom.staged_paths(ns) {
        let Some(name) = path.strip_prefix(&prefix) else {
            continue;
        };
        if name.contains('/') || keep_chunks.contains(name) {
            continue;
        }
        match loom.remove_file_reserved(ns, &path) {
            Ok(()) => {}
            Err(error) if error.code == Code::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn write_document_tombstones<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    retention_policy: &DocumentPolicyConfig,
    tombstones: &BTreeMap<String, DocumentTombstoneRecord>,
) -> Result<Option<Digest>> {
    match retention_policy.name.as_str() {
        DOCUMENT_RETENTION_POLICY_NONE => {
            match loom.remove_file_reserved(ns, &document_tombstone_path(collection)) {
                Ok(()) => {}
                Err(error) if error.code == Code::NotFound => {}
                Err(error) => return Err(error),
            }
            Ok(None)
        }
        DOCUMENT_RETENTION_POLICY_RETAIN => {
            if tombstones.is_empty() {
                return Ok(None);
            }
            let bytes = encode_document_tombstones(tombstones)?;
            let root = Digest::blake3(&bytes);
            loom.write_file_reserved(ns, &document_tombstone_path(collection), &bytes, 0o100644)?;
            Ok(Some(root))
        }
        _ => Err(LoomError::invalid(
            "unsupported document tombstone retention policy",
        )),
    }
}

fn encode_document_tombstones(
    tombstones: &BTreeMap<String, DocumentTombstoneRecord>,
) -> Result<Vec<u8>> {
    let entries = tombstones
        .iter()
        .map(|(id, tombstone)| {
            let document_id = DocumentId::string(id)?;
            if tombstone.document_id != document_id {
                return Err(LoomError::corrupt("document tombstone id mismatch"));
            }
            Ok(CborValue::Array(vec![
                document_id.to_cbor(),
                tombstone.to_cbor(),
            ]))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(cbor::encode(&CborValue::Array(vec![
        CborValue::Text(DOCUMENT_TOMBSTONE_SCHEMA.to_string()),
        CborValue::Array(entries),
    ])))
}

fn load_collection_manifest_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<Option<DocumentCollectionManifest>> {
    let path = normalize_path(&col_path(collection))?;
    match loom.work.get(&ns).and_then(|work| work.get(&path)) {
        Some(StagedEntry::Document(root)) => {
            let (manifest, _) = document_root_parts(loom, *root)?;
            Ok(Some(manifest))
        }
        Some(StagedEntry::File(file)) => Ok(Some(DocumentCollectionManifest::decode(
            &loom.load_content(file.content_addr)?,
        )?)),
        Some(_) => Err(LoomError::invalid(format!("{path:?} is not a document"))),
        None => Ok(None),
    }
}

fn document_map_entries(
    value: &Collection,
    bodies: &BTreeMap<Vec<u8>, StoredDocumentBody>,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let entries = value
        .iter()
        .map(|(id, doc)| {
            let document_id = DocumentId::string(id)?;
            let body = bodies
                .get(doc)
                .ok_or_else(|| LoomError::corrupt("missing document body record"))?;
            let record = DocumentRecord::new(
                document_id.clone(),
                body.body_ref.clone(),
                doc.len() as u64,
                document_entity_tag_from_digest(body.content_digest).to_string(),
                body.content_digest.to_string(),
                DocumentRecordState::Live,
            )?;
            Ok((document_id.canonical_key(), record.encode()))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(entries)
}

fn empty_document_map_root(algo: crate::Algo) -> Digest {
    Digest::hash(algo, &empty_document_map_bytes())
}

fn empty_document_map_bytes() -> Vec<u8> {
    cbor::encode(&CborValue::Array(vec![
        CborValue::Text(DOCUMENT_MAP_SCHEMA.to_string()),
        CborValue::Array(Vec::new()),
    ]))
}

fn build_document_map_root<S: ObjectStore>(
    loom: &mut Loom<S>,
    value: &Collection,
    bodies: &BTreeMap<Vec<u8>, StoredDocumentBody>,
) -> Result<(Digest, Option<Digest>)> {
    let mut entries = document_map_entries(value, bodies)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    match crate::prolly::build(loom.store_mut(), &entries)? {
        Some(root) => Ok((root, Some(root))),
        None => Ok((empty_document_map_root(loom.store().digest_algo()), None)),
    }
}

fn stage_document_root_unchecked<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    manifest: &DocumentCollectionManifest,
    documents_root: Option<Digest>,
) -> Result<()> {
    let manifest_addr = loom.store_content(ns, &manifest.encode())?;
    let mut entries = vec![TreeEntry {
        name: DOCUMENT_ROOT_MANIFEST_ENTRY.to_string(),
        kind: EntryKind::Blob,
        target: manifest_addr,
        mode: 0,
    }];
    if let Some(root) = documents_root {
        entries.push(TreeEntry {
            name: DOCUMENT_ROOT_DOCUMENTS_ENTRY.to_string(),
            kind: EntryKind::ProllyMap,
            target: root,
            mode: 0,
        });
    }
    let root = loom.put_object(&Object::tree(entries)?)?;
    loom.work.entry(ns).or_default().insert(
        normalize_path(&col_path(collection))?,
        StagedEntry::Document(root),
    );
    Ok(())
}

pub(crate) fn try_merge_document_roots<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    base: Option<Digest>,
    ours: Digest,
    theirs: Digest,
) -> Result<Option<Digest>> {
    let (ours_manifest, _) = document_root_parts(loom, ours)?;
    let (theirs_manifest, _) = document_root_parts(loom, theirs)?;
    if !document_manifests_merge_compatible(&ours_manifest, &theirs_manifest) {
        return Ok(None);
    }
    let base_entries = match base {
        Some(root) => document_root_map_entries(loom, root)?,
        None => BTreeMap::new(),
    };
    let ours_entries = document_root_map_entries(loom, ours)?;
    let theirs_entries = document_root_map_entries(loom, theirs)?;
    let mut merged = BTreeMap::new();
    for key in base_entries
        .keys()
        .chain(ours_entries.keys())
        .chain(theirs_entries.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
    {
        let base_value = base_entries.get(&key);
        let ours_value = ours_entries.get(&key);
        let theirs_value = theirs_entries.get(&key);
        let value = if ours_value == theirs_value {
            ours_value
        } else if ours_value == base_value {
            theirs_value
        } else if theirs_value == base_value {
            ours_value
        } else {
            return Ok(None);
        };
        if let Some(value) = value {
            merged.insert(key, value.clone());
        }
    }
    let entries = merged.into_iter().collect::<Vec<_>>();
    let documents_root = crate::prolly::build(loom.store_mut(), &entries)?;
    let mut manifest = ours_manifest;
    manifest.document_map_root =
        documents_root.unwrap_or_else(|| empty_document_map_root(loom.store().digest_algo()));
    let manifest_addr = loom.store_content(ns, &manifest.encode())?;
    let mut root_entries = vec![TreeEntry {
        name: DOCUMENT_ROOT_MANIFEST_ENTRY.to_string(),
        kind: EntryKind::Blob,
        target: manifest_addr,
        mode: 0,
    }];
    if let Some(root) = documents_root {
        root_entries.push(TreeEntry {
            name: DOCUMENT_ROOT_DOCUMENTS_ENTRY.to_string(),
            kind: EntryKind::ProllyMap,
            target: root,
            mode: 0,
        });
    }
    Ok(Some(loom.put_object(&Object::tree(root_entries)?)?))
}

fn document_manifests_merge_compatible(
    ours: &DocumentCollectionManifest,
    theirs: &DocumentCollectionManifest,
) -> bool {
    let mut ours = ours.clone();
    let mut theirs = theirs.clone();
    ours.document_map_root = Digest::blake3(b"document-map-root");
    theirs.document_map_root = Digest::blake3(b"document-map-root");
    ours == theirs
}

fn document_root_map_entries<S: ObjectStore>(
    loom: &Loom<S>,
    root: Digest,
) -> Result<BTreeMap<Vec<u8>, Vec<u8>>> {
    let (_, documents_root) = document_root_parts(loom, root)?;
    match documents_root {
        Some(root) => Ok(crate::prolly::entries(loom.store(), &root)?
            .into_iter()
            .collect()),
        None => Ok(BTreeMap::new()),
    }
}

fn put_index_catalog_unchecked<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    catalog: &DocumentIndexCatalog,
) -> Result<()> {
    loom.create_directory_reserved(ns, &facet_path(FacetKind::Document, ".indexes"), true)?;
    loom.write_file_reserved(
        ns,
        &index_catalog_path(collection),
        &catalog.encode(),
        0o100644,
    )
}

/// Recompute the manifest's `index_catalog_root` from the current canonical index catalog and
/// rewrite the manifest atomically, leaving `document_map_root` and every document record (and
/// therefore document entity tags) untouched. No-op when the collection has no manifest yet; the
/// next `put_collection` binds the root.
fn refresh_index_catalog_root_unchecked<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<()> {
    let index_catalog_root =
        Digest::blake3(&load_index_catalog_unchecked(loom, ns, collection)?.encode());
    let path = normalize_path(&col_path(collection))?;
    match loom.work.get(&ns).and_then(|work| work.get(&path)).copied() {
        Some(StagedEntry::Document(root)) => {
            let (mut manifest, documents_root) = document_root_parts(loom, root)?;
            if manifest.index_catalog_root != index_catalog_root {
                manifest.index_catalog_root = index_catalog_root;
                stage_document_root_unchecked(loom, ns, collection, &manifest, documents_root)?;
            }
            Ok(())
        }
        Some(StagedEntry::File(file)) => {
            let mut manifest =
                DocumentCollectionManifest::decode(&loom.load_content(file.content_addr)?)?;
            if manifest.index_catalog_root != index_catalog_root {
                manifest.index_catalog_root = index_catalog_root;
                loom.write_file_reserved(ns, &col_path(collection), &manifest.encode(), 0o100644)?;
            }
            Ok(())
        }
        Some(_) => Err(LoomError::invalid(format!("{path:?} is not a document"))),
        None => Ok(()),
    }
}

fn put_index_state_unchecked<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    index: &str,
    state: &DocumentIndexState,
) -> Result<()> {
    loom.create_directory_reserved(
        ns,
        &facet_path(
            FacetKind::Document,
            &format!(".index-data/{}", hex::encode(collection.as_bytes())),
        ),
        true,
    )?;
    loom.write_file_reserved(
        ns,
        &index_state_path(collection, index),
        &state.encode(),
        0o100644,
    )
}

fn load_index_catalog_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<DocumentIndexCatalog> {
    match loom.read_file_reserved(ns, &index_catalog_path(collection)) {
        Ok(bytes) => DocumentIndexCatalog::decode(&bytes),
        Err(e) if e.code == Code::NotFound => Ok(DocumentIndexCatalog::new()),
        Err(e) => Err(e),
    }
}

fn try_load_index_state_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    index: &str,
) -> Result<Option<DocumentIndexState>> {
    match loom.read_file_reserved(ns, &index_state_path(collection, index)) {
        Ok(bytes) => Ok(Some(DocumentIndexState::decode(&bytes)?)),
        Err(e) if e.code == Code::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Load the value named `collection` from `ns`'s current working tree, or `NOT_FOUND`.
pub fn get_collection<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<Collection> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Read)?;
    load_collection_unchecked(loom, ns, collection)
}

/// Load the value named `collection`, or an empty value when it does not exist yet. The facade
/// reads treat an absent value as empty rather than an error.
fn load_or_empty_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<Collection> {
    match load_collection_unchecked(loom, ns, collection) {
        Ok(collection) => Ok(collection),
        Err(e) if e.code == Code::NotFound => Ok(Collection::new()),
        Err(e) => Err(e),
    }
}

fn load_collection_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<Collection> {
    let path = normalize_path(&col_path(collection))?;
    match loom.work.get(&ns).and_then(|work| work.get(&path)) {
        Some(StagedEntry::Document(root)) => {
            load_collection_from_document_root(loom, ns, collection, *root)
        }
        Some(StagedEntry::File(file)) => load_collection_from_flat_manifest(
            loom,
            ns,
            collection,
            &loom.load_content(file.content_addr)?,
        ),
        Some(_) => Err(LoomError::invalid(format!("{path:?} is not a document"))),
        None => Err(LoomError::not_found(format!(
            "document collection {collection:?} not staged"
        ))),
    }
}

fn load_collection_from_flat_manifest<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    manifest_bytes: &[u8],
) -> Result<Collection> {
    let manifest = DocumentCollectionManifest::decode(manifest_bytes)?;
    if manifest.collection_id != collection {
        return Err(LoomError::corrupt(
            "document collection manifest id mismatch",
        ));
    }
    let map_bytes = loom.read_file_reserved(ns, &document_map_path(collection))?;
    if Digest::blake3(&map_bytes) != manifest.document_map_root {
        return Err(LoomError::corrupt("document map root mismatch"));
    }
    let index_catalog_bytes = load_index_catalog_unchecked(loom, ns, collection)?.encode();
    if Digest::blake3(&index_catalog_bytes) != manifest.index_catalog_root {
        return Err(LoomError::corrupt("document index catalog root mismatch"));
    }
    if let Some(root) = manifest.tombstone_root {
        load_document_tombstones(loom, ns, collection, root)?;
    }
    decode_flat_document_map(loom, ns, collection, &map_bytes)
}

fn load_collection_from_document_root<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    root: Digest,
) -> Result<Collection> {
    let (manifest, documents_root) = document_root_parts(loom, root)?;
    if manifest.collection_id != collection {
        return Err(LoomError::corrupt(
            "document collection manifest id mismatch",
        ));
    }
    let out = match documents_root {
        Some(root) => decode_prolly_document_map(loom, ns, collection, root)?,
        None => {
            if manifest.document_map_root != empty_document_map_root(loom.store().digest_algo()) {
                return Err(LoomError::corrupt("document map root mismatch"));
            }
            Collection::new()
        }
    };
    let index_catalog_bytes = load_index_catalog_unchecked(loom, ns, collection)?.encode();
    if Digest::blake3(&index_catalog_bytes) != manifest.index_catalog_root {
        return Err(LoomError::corrupt("document index catalog root mismatch"));
    }
    if let Some(root) = manifest.tombstone_root {
        load_document_tombstones(loom, ns, collection, root)?;
    }
    Ok(out)
}

fn document_root_parts<S: ObjectStore>(
    loom: &Loom<S>,
    root: Digest,
) -> Result<(DocumentCollectionManifest, Option<Digest>)> {
    let Object::Tree(entries) = loom.get_object(&root)? else {
        return Err(LoomError::corrupt("document root is not a Tree"));
    };
    let mut manifest_addr = None;
    let mut documents_root = None;
    for entry in entries {
        match entry.name.as_str() {
            DOCUMENT_ROOT_MANIFEST_ENTRY if entry.kind == EntryKind::Blob => {
                manifest_addr = Some(entry.target);
            }
            DOCUMENT_ROOT_DOCUMENTS_ENTRY if entry.kind == EntryKind::ProllyMap => {
                documents_root = Some(entry.target);
            }
            _ => return Err(LoomError::corrupt("invalid document root entry")),
        }
    }
    let manifest = DocumentCollectionManifest::decode(&loom.load_content(
        manifest_addr.ok_or_else(|| LoomError::corrupt("document root has no manifest"))?,
    )?)?;
    if let Some(root) = documents_root
        && manifest.document_map_root != root
    {
        return Err(LoomError::corrupt("document map root mismatch"));
    }
    Ok((manifest, documents_root))
}

fn load_document_tombstones<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    root: Digest,
) -> Result<BTreeMap<String, DocumentTombstoneRecord>> {
    let bytes = loom.read_file_reserved(ns, &document_tombstone_path(collection))?;
    if Digest::blake3(&bytes) != root {
        return Err(LoomError::corrupt("document tombstone root mismatch"));
    }
    decode_document_tombstones(&bytes)
}

fn decode_document_tombstones(bytes: &[u8]) -> Result<BTreeMap<String, DocumentTombstoneRecord>> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let schema = fields.text()?;
    if schema != DOCUMENT_TOMBSTONE_SCHEMA {
        return Err(LoomError::corrupt("unknown document tombstone schema"));
    }
    let raw_entries = fields.array()?;
    fields.end()?;
    let mut out = BTreeMap::new();
    for raw_entry in raw_entries {
        let mut entry = cbor::Fields::new(cbor::as_array(raw_entry)?);
        let document_id = DocumentId::from_cbor(entry.next_field()?)?;
        let tombstone = DocumentTombstoneRecord::from_cbor(entry.next_field()?)?;
        entry.end()?;
        if tombstone.document_id != document_id {
            return Err(LoomError::corrupt("document tombstone id mismatch"));
        }
        let id = match document_id {
            DocumentId::String(id) => id,
            _ => {
                return Err(LoomError::corrupt(
                    "document tombstone id is not string-backed",
                ));
            }
        };
        if out.insert(id, tombstone).is_some() {
            return Err(LoomError::corrupt("duplicate document tombstone id"));
        }
    }
    Ok(out)
}

fn decode_flat_document_map<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    bytes: &[u8],
) -> Result<Collection> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let schema = fields.text()?;
    if schema != DOCUMENT_MAP_SCHEMA {
        return Err(LoomError::corrupt("unknown document map schema"));
    }
    let raw_entries = fields.array()?;
    fields.end()?;
    // Parse and gate every entry before loading any body so a malformed map is rejected as a pure
    // decode failure. The ordered document identifier map must contain each string id exactly once,
    // in strictly ascending id order (the order the encoder emits from the id-keyed collection). A
    // duplicate or out-of-order id is a negative-decode case, not a silent last-writer-wins overlay.
    let mut parsed: Vec<(String, DocumentRecord)> = Vec::with_capacity(raw_entries.len());
    for raw_entry in raw_entries {
        let mut entry = cbor::Fields::new(cbor::as_array(raw_entry)?);
        let document_id = DocumentId::from_cbor(entry.next_field()?)?;
        let record = DocumentRecord::from_cbor(entry.next_field()?)?;
        entry.end()?;
        if record.document_id != document_id {
            return Err(LoomError::corrupt("document map id mismatch"));
        }
        let id = match document_id {
            DocumentId::String(id) => id,
            _ => return Err(LoomError::corrupt("document map id is not string-backed")),
        };
        if let Some((previous_id, _)) = parsed.last() {
            match id.cmp(previous_id) {
                std::cmp::Ordering::Equal => {
                    return Err(LoomError::corrupt("duplicate document id in document map"));
                }
                std::cmp::Ordering::Less => {
                    return Err(LoomError::corrupt("document map ids out of order"));
                }
                std::cmp::Ordering::Greater => {}
            }
        }
        parsed.push((id, record));
    }
    let mut out = Collection::new();
    for (id, record) in parsed {
        let body = load_document_body(loom, ns, collection, &record)?;
        out.put(id, body);
    }
    Ok(out)
}

fn decode_prolly_document_map<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    root: Digest,
) -> Result<Collection> {
    let mut out = Collection::new();
    let mut previous_key: Option<Vec<u8>> = None;
    for (key, value) in crate::prolly::entries(loom.store(), &root)? {
        if previous_key
            .as_ref()
            .is_some_and(|previous| previous >= &key)
        {
            return Err(LoomError::corrupt("document map ids out of order"));
        }
        previous_key = Some(key.clone());
        let document_id = DocumentId::decode(&key)?;
        let record = DocumentRecord::decode(&value)?;
        if record.document_id != document_id {
            return Err(LoomError::corrupt("document map id mismatch"));
        }
        let id = match document_id {
            DocumentId::String(id) => id,
            _ => return Err(LoomError::corrupt("document map id is not string-backed")),
        };
        let body = load_document_body(loom, ns, collection, &record)?;
        out.put(id, body);
    }
    Ok(out)
}

fn load_document_body<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    record: &DocumentRecord,
) -> Result<Vec<u8>> {
    let bytes = match record.body_ref {
        DocumentBodyRef::Direct { digest } => {
            loom.read_file_reserved(ns, &document_body_path(collection, &digest))?
        }
        DocumentBodyRef::Chunked { root } => {
            load_chunked_document_body(loom, ns, collection, root)?
        }
    };
    if bytes.len() as u64 != record.byte_length {
        return Err(LoomError::corrupt("document body length mismatch"));
    }
    if DocumentEntityTagString(document_entity_tag(loom, &bytes)).to_string() != record.entity_tag {
        return Err(LoomError::corrupt("document body entity tag mismatch"));
    }
    Ok(bytes)
}

fn load_chunked_document_body<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    root: Digest,
) -> Result<Vec<u8>> {
    let chunk_list_bytes = loom.read_file_reserved(ns, &document_body_path(collection, &root))?;
    let chunk_list = Object::decode(&chunk_list_bytes)?;
    if chunk_list.digest_with(loom.store().digest_algo()) != root {
        return Err(LoomError::corrupt("document chunk list root mismatch"));
    }
    let Object::ChunkList {
        total_size,
        entries,
    } = chunk_list
    else {
        return Err(LoomError::corrupt(
            "document chunked body root is not a ChunkList",
        ));
    };
    let mut out = Vec::with_capacity(total_size as usize);
    for entry in entries {
        let chunk = loom.read_file_reserved(ns, &document_chunk_path(collection, &entry.target))?;
        if chunk.len() as u64 != entry.size {
            return Err(LoomError::corrupt("document chunk length mismatch"));
        }
        if Object::Blob(chunk.clone()).digest_with(loom.store().digest_algo()) != entry.target {
            return Err(LoomError::corrupt("document chunk digest mismatch"));
        }
        out.extend_from_slice(&chunk);
    }
    if out.len() as u64 != total_size {
        return Err(LoomError::corrupt(
            "document chunk list total size mismatch",
        ));
    }
    Ok(out)
}

/// Put document `doc` at string `id` in value `collection` of `ns` (selected by the caller's selector),
/// creating the value and the `document` facet if absent, and stage it. A later put at the same id
/// replaces the document.
pub fn doc_put<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    id: &str,
    doc: Vec<u8>,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Write)?;
    let mut col = load_or_empty_unchecked(loom, ns, collection)?;
    let old = col.get(id).map(<[u8]>::to_vec);
    col.put(id, doc);
    let states = prepare_indexes_for_put_unchecked(loom, ns, collection, &col, id, old.as_deref())?;
    put_collection_unchecked(loom, ns, collection, &col)?;
    write_index_states_unchecked(loom, ns, collection, states)
}

fn document_digest<S: ObjectStore>(loom: &Loom<S>, bytes: &[u8]) -> Digest {
    Digest::hash(loom.store().digest_algo(), bytes)
}

fn document_entity_tag_from_digest(digest: Digest) -> DocumentEntityTagString {
    DocumentEntityTagString(ContentTag::new(digest).to_entity_tag())
}

pub fn document_entity_tag_string<S: ObjectStore>(loom: &Loom<S>, bytes: &[u8]) -> String {
    DocumentEntityTagString(document_entity_tag(loom, bytes)).to_string()
}

pub fn document_entity_tag_string_from_digest(digest: Digest) -> String {
    document_entity_tag_from_digest(digest).to_string()
}

pub fn parse_document_entity_tag(value: &str) -> Result<EntityTag> {
    let Some(hex) = value.strip_prefix("entity-tag:") else {
        return Err(LoomError::invalid(
            "document entity tag must use entity-tag:<hex>",
        ));
    };
    let bytes = hex::decode(hex)
        .map_err(|_| LoomError::invalid("document entity tag contains invalid hex"))?;
    Ok(EntityTag::opaque(bytes))
}

pub fn document_content_tag<S: ObjectStore>(loom: &Loom<S>, bytes: &[u8]) -> ContentTag {
    ContentTag::new(document_digest(loom, bytes))
}

pub fn document_entity_tag<S: ObjectStore>(loom: &Loom<S>, bytes: &[u8]) -> EntityTag {
    document_content_tag(loom, bytes).to_entity_tag()
}

#[derive(Clone)]
struct DocumentEntityTagString(EntityTag);

impl std::fmt::Display for DocumentEntityTagString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "entity-tag:{}", hex::encode(self.0.as_bytes()))
    }
}

fn check_expected_digest<S: ObjectStore>(
    loom: &Loom<S>,
    current: Option<&[u8]>,
    expected_digest: Option<&Digest>,
) -> Result<()> {
    let Some(expected_digest) = expected_digest else {
        return Ok(());
    };
    let Some(current) = current else {
        return Err(LoomError::cas_mismatch(
            "document digest guard did not match",
        ));
    };
    if document_digest(loom, current) == *expected_digest {
        Ok(())
    } else {
        Err(LoomError::cas_mismatch(
            "document digest guard did not match",
        ))
    }
}

fn condition_conflict(reason: ConflictReason) -> LoomError {
    LoomError::new(Code::Conflict, reason.as_str())
}

fn check_document_mutation_request<S: ObjectStore>(
    loom: &Loom<S>,
    current: Option<&[u8]>,
    request: &MutationRequest,
) -> Result<()> {
    if request.mode.requires_existing_record() && current.is_none() {
        return Err(condition_conflict(ConflictReason::MissingRecord));
    }
    match request.compare_condition() {
        CompareCondition::Any => Ok(()),
        CompareCondition::Absent => {
            if current.is_none() {
                Ok(())
            } else {
                Err(condition_conflict(ConflictReason::RecordAlreadyExists))
            }
        }
        CompareCondition::Exact(expected) => {
            let Some(current) = current else {
                return Err(condition_conflict(ConflictReason::MissingRecord));
            };
            if document_entity_tag(loom, current) == expected {
                Ok(())
            } else {
                Err(condition_conflict(ConflictReason::ExpectedTagMismatch))
            }
        }
        CompareCondition::Generation(_) => Err(condition_conflict(ConflictReason::StaleRevision)),
        CompareCondition::OperationAnchor(_) => {
            Err(condition_conflict(ConflictReason::StaleOperationAnchor))
        }
    }
}

pub fn document_put_binary<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    id: &str,
    bytes: Vec<u8>,
    expected_digest: Option<&Digest>,
) -> Result<Digest> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Write)?;
    let mut col = load_or_empty_unchecked(loom, ns, collection)?;
    let old = col.get(id).map(<[u8]>::to_vec);
    check_expected_digest(loom, old.as_deref(), expected_digest)?;
    let digest = document_digest(loom, &bytes);
    col.put(id, bytes);
    let states = prepare_indexes_for_put_unchecked(loom, ns, collection, &col, id, old.as_deref())?;
    put_collection_unchecked(loom, ns, collection, &col)?;
    write_index_states_unchecked(loom, ns, collection, states)?;
    Ok(digest)
}

pub fn document_put_binary_with_request<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    id: &str,
    bytes: Vec<u8>,
    request: MutationRequest,
) -> Result<DocumentMutationResult> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Write)?;
    let mut col = load_or_empty_unchecked(loom, ns, collection)?;
    let old = col.get(id).map(<[u8]>::to_vec);
    check_document_mutation_request(loom, old.as_deref(), &request)?;
    let digest = document_digest(loom, &bytes);
    let entity_tag = document_entity_tag(loom, &bytes);
    col.put(id, bytes);
    let states = prepare_indexes_for_put_unchecked(loom, ns, collection, &col, id, old.as_deref())?;
    put_collection_unchecked(loom, ns, collection, &col)?;
    write_index_states_unchecked(loom, ns, collection, states)?;
    Ok(DocumentMutationResult {
        digest,
        outcome: CompareOutcome::applied(Some(entity_tag)),
    })
}

fn request_from_expected_entity_tag(expected_entity_tag: Option<&str>) -> Result<MutationRequest> {
    let mode = match expected_entity_tag {
        Some(entity_tag) => MutationMode::ReplaceIfMatch(parse_document_entity_tag(entity_tag)?),
        None => MutationMode::UpsertBlind,
    };
    Ok(MutationRequest::new(mode))
}

pub fn document_put_binary_with_entity_tag<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    id: &str,
    bytes: Vec<u8>,
    expected_entity_tag: Option<&str>,
) -> Result<DocumentPutResult> {
    let result = document_put_binary_with_request(
        loom,
        ns,
        collection,
        id,
        bytes,
        request_from_expected_entity_tag(expected_entity_tag)?,
    )?;
    Ok(DocumentPutResult {
        digest: result.digest,
        entity_tag: result
            .outcome
            .entity_tag
            .map(|tag| DocumentEntityTagString(tag).to_string())
            .ok_or_else(|| LoomError::corrupt("document put result is missing entity tag"))?,
    })
}

pub fn document_delete_with_request<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    id: &str,
    request: MutationRequest,
) -> Result<CompareOutcome> {
    if !matches!(
        request.mode,
        MutationMode::DeleteIfPresent | MutationMode::DeleteIfMatch(_)
    ) {
        return Err(LoomError::invalid(
            "document delete requires a delete mutation mode",
        ));
    }
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Write)?;
    let mut col = load_or_empty_unchecked(loom, ns, collection)?;
    let old = col.get(id).map(<[u8]>::to_vec);
    check_document_mutation_request(loom, old.as_deref(), &request)?;
    col.delete(id);
    let states =
        prepare_indexes_for_delete_unchecked(loom, ns, collection, &col, id, old.as_deref())?;
    put_deleted_collection_unchecked(loom, ns, collection, &col, id, old.as_deref())?;
    write_index_states_unchecked(loom, ns, collection, states)?;
    Ok(CompareOutcome::applied(None))
}

pub fn document_get_binary<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    id: &str,
) -> Result<Option<DocumentBinary>> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Read)?;
    Ok(load_or_empty_unchecked(loom, ns, collection)?
        .get(id)
        .map(|bytes| DocumentBinary {
            bytes: bytes.to_vec(),
            digest: document_digest(loom, bytes),
            entity_tag: document_entity_tag_string(loom, bytes),
        }))
}

pub fn document_set_retention_policy<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    retention_policy: DocumentPolicyConfig,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Write)?;
    match retention_policy.name.as_str() {
        DOCUMENT_RETENTION_POLICY_NONE | DOCUMENT_RETENTION_POLICY_RETAIN => {}
        _ => {
            return Err(LoomError::invalid(
                "unsupported document tombstone retention policy",
            ));
        }
    }
    let col = load_or_empty_unchecked(loom, ns, collection)?;
    let tombstones = load_collection_manifest_unchecked(loom, ns, collection)?
        .as_ref()
        .and_then(|manifest| manifest.tombstone_root)
        .map(|root| load_document_tombstones(loom, ns, collection, root))
        .transpose()?
        .unwrap_or_default();
    put_collection_with_tombstones_unchecked(
        loom,
        ns,
        collection,
        &col,
        retention_policy,
        tombstones,
    )
}

pub fn document_list_binary<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<Vec<u8>> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Read)?;
    Ok(load_or_empty_unchecked(loom, ns, collection)?.encode())
}

pub fn document_put_text<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    id: &str,
    text: &str,
    expected_digest: Option<&Digest>,
) -> Result<Digest> {
    document_put_binary(
        loom,
        ns,
        collection,
        id,
        text.as_bytes().to_vec(),
        expected_digest,
    )
}

pub fn document_put_text_with_entity_tag<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    id: &str,
    text: &str,
    expected_entity_tag: Option<&str>,
) -> Result<DocumentPutResult> {
    document_put_binary_with_entity_tag(
        loom,
        ns,
        collection,
        id,
        text.as_bytes().to_vec(),
        expected_entity_tag,
    )
}

pub fn document_get_text<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    id: &str,
) -> Result<Option<DocumentText>> {
    let Some(document) = document_get_binary(loom, ns, collection, id)? else {
        return Ok(None);
    };
    let text = String::from_utf8(document.bytes)
        .map_err(|_| LoomError::document_not_text("document payload is not valid UTF-8 text"))?;
    Ok(Some(DocumentText {
        text,
        digest: document.digest,
        entity_tag: document.entity_tag,
    }))
}

/// The document at `id` in `collection`, or `None` when the id or value is absent.
pub fn doc_get<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    id: &str,
) -> Result<Option<Vec<u8>>> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Read)?;
    Ok(load_or_empty_unchecked(loom, ns, collection)?
        .get(id)
        .map(<[u8]>::to_vec))
}

fn put_deleted_collection_unchecked<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    value: &Collection,
    id: &str,
    old: Option<&[u8]>,
) -> Result<()> {
    let manifest = load_collection_manifest_unchecked(loom, ns, collection)?;
    let retention_policy = manifest
        .as_ref()
        .map(|manifest| manifest.retention_policy.clone())
        .unwrap_or(DocumentPolicyConfig::new(DOCUMENT_RETENTION_POLICY_NONE)?);
    let mut tombstones = manifest
        .as_ref()
        .and_then(|manifest| manifest.tombstone_root)
        .map(|root| load_document_tombstones(loom, ns, collection, root))
        .transpose()?
        .unwrap_or_default();
    if retention_policy.name == DOCUMENT_RETENTION_POLICY_RETAIN
        && let Some(old) = old
    {
        tombstones.insert(
            id.to_string(),
            document_tombstone_from_delete_policy(loom, id, old, &retention_policy)?,
        );
    }
    put_collection_with_tombstones_unchecked(
        loom,
        ns,
        collection,
        value,
        retention_policy,
        tombstones,
    )
}

fn document_tombstone_from_delete_policy<S: ObjectStore>(
    loom: &Loom<S>,
    id: &str,
    old: &[u8],
    retention_policy: &DocumentPolicyConfig,
) -> Result<DocumentTombstoneRecord> {
    let document_id = DocumentId::string(id)?;
    let prior_digest = document_digest(loom, old);
    let prior_entity_tag = document_entity_tag_from_digest(prior_digest).to_string();
    let deletion_revision = document_deletion_revision(loom, &document_id, &prior_entity_tag);
    let deleted_entity_tag = document_entity_tag_from_digest(deletion_revision).to_string();
    Ok(DocumentTombstoneRecord {
        document_id,
        deleted_entity_tag,
        prior_entity_tag,
        deletion_revision: deletion_revision.to_string(),
        retention_class: retention_policy_text_parameter(
            retention_policy,
            "retention_class",
            DOCUMENT_TOMBSTONE_RETENTION_CLASS,
        )?,
        reclaim_after: optional_retention_policy_text_parameter(retention_policy, "reclaim_after")?,
        deletion_reason: optional_retention_policy_text_parameter(
            retention_policy,
            "deletion_reason",
        )?,
        source_metadata: BTreeMap::new(),
    })
}

fn document_deletion_revision<S: ObjectStore>(
    loom: &Loom<S>,
    document_id: &DocumentId,
    prior_entity_tag: &str,
) -> Digest {
    let bytes = cbor::encode(&CborValue::Array(vec![
        CborValue::Text(DOCUMENT_DELETION_REVISION_SCHEMA.to_string()),
        document_id.to_cbor(),
        CborValue::Text(prior_entity_tag.to_string()),
    ]));
    Digest::hash(loom.store().digest_algo(), &bytes)
}

fn retention_policy_text_parameter(
    retention_policy: &DocumentPolicyConfig,
    key: &str,
    default: &str,
) -> Result<String> {
    optional_retention_policy_text_parameter(retention_policy, key)
        .map(|value| value.unwrap_or_else(|| default.to_string()))
}

fn optional_retention_policy_text_parameter(
    retention_policy: &DocumentPolicyConfig,
    key: &str,
) -> Result<Option<String>> {
    retention_policy
        .parameters
        .get(key)
        .cloned()
        .map(cbor::as_text)
        .transpose()
}

/// Remove `id` from `collection`; returns whether it was present. A no-op (absent id or value) does not
/// write.
pub fn doc_delete<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    id: &str,
) -> Result<bool> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Write)?;
    let mut col = load_or_empty_unchecked(loom, ns, collection)?;
    let old = col.get(id).map(<[u8]>::to_vec);
    let present = col.delete(id);
    if present {
        let states =
            prepare_indexes_for_delete_unchecked(loom, ns, collection, &col, id, old.as_deref())?;
        put_deleted_collection_unchecked(loom, ns, collection, &col, id, old.as_deref())?;
        write_index_states_unchecked(loom, ns, collection, states)?;
    }
    Ok(present)
}

/// The whole value named `collection` in id order, or an empty value when absent.
pub fn doc_list<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<Collection> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Read)?;
    load_or_empty_unchecked(loom, ns, collection)
}

/// The document collection names present in `ns`'s current working tree, sorted and de-duplicated.
/// Enumeration is within the workspace, not a global index. Reserved names beginning with `.` are
/// excluded.
pub fn doc_list_collections<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
) -> Result<Vec<String>> {
    loom.authorize_collection(ns, FacetKind::Document, "", AclRight::Read)?;
    let prefix = format!("{}/", facet_root(FacetKind::Document));
    let mut out: Vec<String> = loom
        .staged_paths(ns)
        .into_iter()
        .filter_map(|p| {
            let rest = p.strip_prefix(&prefix)?;
            if rest.contains('/') || rest.starts_with('.') {
                return None;
            }
            Some(rest.to_string())
        })
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

pub fn doc_create_index<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    index: DocumentIndexDef,
) -> Result<()> {
    doc_create_index_declaration(loom, ns, collection, declaration_from_def(&index))
}

pub fn doc_create_index_declaration<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    declaration: DocumentIndexDeclaration,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Write)?;
    let mut catalog = load_index_catalog_unchecked(loom, ns, collection)?;
    let index = def_from_declaration(&declaration);
    let state = build_index_state_unchecked(loom, ns, collection, &index)?;
    catalog.insert_declaration(declaration)?;
    put_index_catalog_unchecked(loom, ns, collection, &catalog)?;
    put_index_state_unchecked(loom, ns, collection, &index.name, &state)?;
    refresh_index_catalog_root_unchecked(loom, ns, collection)
}

pub fn doc_drop_index<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    name: &str,
) -> Result<bool> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Write)?;
    let mut catalog = load_index_catalog_unchecked(loom, ns, collection)?;
    let removed = catalog.remove(name);
    if removed {
        put_index_catalog_unchecked(loom, ns, collection, &catalog)?;
        loom.remove_file_reserved(ns, &index_state_path(collection, name))?;
        refresh_index_catalog_root_unchecked(loom, ns, collection)?;
    }
    Ok(removed)
}

pub fn doc_list_indexes<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<Vec<DocumentIndexDef>> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Read)?;
    Ok(load_index_catalog_unchecked(loom, ns, collection)?.indexes())
}

pub fn doc_list_index_declarations<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<Vec<DocumentIndexDeclaration>> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Read)?;
    Ok(load_index_catalog_unchecked(loom, ns, collection)?
        .declarations()
        .to_vec())
}

pub fn doc_index_statuses<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<Vec<DocumentIndexStatus>> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Read)?;
    load_index_catalog_unchecked(loom, ns, collection)?
        .indexes()
        .into_iter()
        .map(|index| {
            let state = try_load_index_state_unchecked(loom, ns, collection, &index.name)?;
            Ok(DocumentIndexStatus {
                name: index.name,
                ready: state.is_some(),
                entries: state
                    .as_ref()
                    .map(|state| state.entries.values().map(BTreeSet::len).sum())
                    .unwrap_or(0),
            })
        })
        .collect()
}

pub fn doc_rebuild_index<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    name: &str,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Write)?;
    let catalog = load_index_catalog_unchecked(loom, ns, collection)?;
    let index = catalog
        .index(name)
        .ok_or_else(|| crate::LoomError::not_found(format!("document index {name:?}")))?
        .clone();
    let state = build_index_state_unchecked(loom, ns, collection, &index)?;
    put_index_state_unchecked(loom, ns, collection, &index.name, &state)
}

pub fn doc_find<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    index: &str,
    value: &IndexedValue,
) -> Result<Vec<String>> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Read)?;
    let catalog = load_index_catalog_unchecked(loom, ns, collection)?;
    if catalog.index(index).is_none() {
        return Err(crate::LoomError::not_found(format!(
            "document index {index:?}"
        )));
    }
    let state = try_load_index_state_unchecked(loom, ns, collection, index)?
        .ok_or_else(|| crate::LoomError::index_not_ready(format!("document index {index:?}")))?;
    Ok(state.find(value))
}

pub fn doc_query<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    query: &DocumentQuery,
) -> Result<DocumentQueryResult> {
    loom.authorize_collection(ns, FacetKind::Document, collection, AclRight::Read)?;
    let collection_value = load_or_empty_unchecked(loom, ns, collection)?;
    let mut ids = match &query.predicate {
        Some(predicate) => {
            evaluate_document_predicate(loom, ns, collection, &collection_value, predicate)?
        }
        None => collection_value.ids().map(str::to_string).collect(),
    };
    if let Some(cursor) = &query.cursor {
        ids.retain(|id| id.as_str() > cursor.as_str());
    }
    let limit = query.limit.clamp(1, 1000);
    let mut items = Vec::new();
    let mut next_cursor = None;
    let mut last_returned = None;
    for id in ids {
        if items.len() == limit {
            next_cursor = last_returned;
            break;
        }
        let doc = collection_value
            .get(&id)
            .ok_or_else(|| crate::LoomError::corrupt("document query selected a missing id"))?;
        let mut projections = BTreeMap::new();
        for projection in &query.projections {
            projections.insert(
                projection.name.clone(),
                doc_extract_index_value(doc, &projection.path)?,
            );
        }
        items.push(DocumentQueryItem {
            id: id.clone(),
            document: if query.include_document {
                Some(doc.to_vec())
            } else {
                None
            },
            projections,
        });
        last_returned = Some(id);
    }
    Ok(DocumentQueryResult { items, next_cursor })
}

pub fn doc_extract_index_value(
    doc: &[u8],
    path: &DocumentFieldPath,
) -> Result<Option<IndexedValue>> {
    let json: serde_json::Value =
        serde_json::from_slice(doc).map_err(|e| crate::LoomError::invalid(e.to_string()))?;
    let Some(value) = document_json_path(&json, path) else {
        return Ok(None);
    };
    json_scalar_to_indexed_value(value).map(Some)
}

pub fn document_query_from_json(value: &serde_json::Value) -> Result<DocumentQuery> {
    let predicate = match value.get("predicate") {
        Some(serde_json::Value::Null) | None => None,
        Some(predicate) => Some(document_predicate_from_json(predicate)?),
    };
    let projections = match value.get("projections") {
        Some(serde_json::Value::Null) | None => Vec::new(),
        Some(serde_json::Value::Array(projections)) => projections
            .iter()
            .map(document_projection_from_json)
            .collect::<Result<Vec<_>>>()?,
        Some(_) => return Err(crate::LoomError::invalid("projections must be an array")),
    };
    Ok(DocumentQuery {
        predicate,
        projections,
        cursor: value
            .get("cursor")
            .and_then(serde_json::Value::as_str)
            .filter(|cursor| !cursor.is_empty())
            .map(str::to_string),
        limit: value
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .and_then(|limit| usize::try_from(limit).ok())
            .unwrap_or(100),
        include_document: value
            .get("include_document")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    })
}

pub fn document_query_result_json(result: DocumentQueryResult) -> serde_json::Value {
    serde_json::json!({
        "items": result.items.into_iter().map(|item| {
            let projections = item
                .projections
                .into_iter()
                .map(|(name, value)| (name, value.map_or(serde_json::Value::Null, indexed_value_json)))
                .collect::<serde_json::Map<_, _>>();
            serde_json::json!({
                "id": item.id,
                "document_hex": item.document.map(hex::encode),
                "projections": serde_json::Value::Object(projections)
            })
        }).collect::<Vec<_>>(),
        "next_cursor": result.next_cursor
    })
}

pub fn document_field_path_string(path: &DocumentFieldPath) -> String {
    let mut out = String::new();
    for segment in path.segments() {
        match segment {
            DocumentFieldPathSegment::Field(field) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(field);
            }
            DocumentFieldPathSegment::Index(index) => {
                out.push('[');
                out.push_str(&index.to_string());
                out.push(']');
            }
        }
    }
    out
}

pub fn document_index_declaration_from_json(
    value: &serde_json::Value,
) -> Result<DocumentIndexDeclaration> {
    let object = value
        .as_object()
        .ok_or_else(|| LoomError::invalid("document index declaration must be an object"))?;
    let index_name = json_required_string_alias(object, "index_name", "name")?;
    let index_id = json_optional_string(object, "index_id")?.unwrap_or_else(|| index_name.clone());
    let selector = json_required_string_alias(object, "source_selector", "path")?;
    let uniqueness = match json_optional_string(object, "uniqueness")? {
        Some(value) => DocumentIndexUniqueness::from_label(&value)?,
        None => {
            if json_optional_bool(object, "unique")?.unwrap_or(false) {
                DocumentIndexUniqueness::Unique
            } else {
                DocumentIndexUniqueness::NonUnique
            }
        }
    };
    let declaration = DocumentIndexDeclaration {
        index_id,
        index_name,
        source_selector: DocumentFieldPath::dotted(&selector)?,
        extractor: json_optional_string(object, "extractor")?
            .unwrap_or_else(|| DOCUMENT_INDEX_EXTRACTOR_JSON_SCALAR.to_string()),
        key_codec: json_optional_string(object, "key_codec")?
            .unwrap_or_else(|| DOCUMENT_INDEX_KEY_CODEC_SCALAR.to_string()),
        comparator: json_optional_string(object, "comparator")?
            .unwrap_or_else(|| DOCUMENT_INDEX_COMPARATOR_CANONICAL.to_string()),
        uniqueness,
        failure_policy: json_optional_string(object, "failure_policy")?
            .unwrap_or_else(|| DOCUMENT_INDEX_FAILURE_POLICY_SKIP_MISSING.to_string()),
        declaration_version: json_optional_u64(object, "declaration_version")?.unwrap_or(1),
        analyzer_profile: json_optional_string(object, "analyzer_profile")?,
        projection: json_optional_string(object, "projection")?
            .map(|path| DocumentFieldPath::dotted(&path))
            .transpose()?,
        partial_filter: json_optional_string(object, "partial_filter")?,
        metadata: match object.get("metadata") {
            Some(serde_json::Value::Null) | None => BTreeMap::new(),
            Some(serde_json::Value::Object(metadata)) => metadata
                .iter()
                .map(|(key, value)| Ok((key.clone(), json_to_cbor_value(value)?)))
                .collect::<Result<BTreeMap<_, _>>>()?,
            Some(_) => {
                return Err(LoomError::invalid(
                    "document index metadata must be an object",
                ));
            }
        },
    };
    DocumentIndexDeclaration::decode(&declaration.encode())
}

pub fn document_index_declaration_json(declaration: DocumentIndexDeclaration) -> serde_json::Value {
    serde_json::json!({
        "index_id": declaration.index_id,
        "index_name": declaration.index_name,
        "name": declaration.index_name,
        "source_selector": document_field_path_string(&declaration.source_selector),
        "path": document_field_path_string(&declaration.source_selector),
        "extractor": declaration.extractor,
        "key_codec": declaration.key_codec,
        "comparator": declaration.comparator,
        "uniqueness": declaration.uniqueness.as_str(),
        "unique": matches!(declaration.uniqueness, DocumentIndexUniqueness::Unique),
        "failure_policy": declaration.failure_policy,
        "declaration_version": declaration.declaration_version,
        "analyzer_profile": declaration.analyzer_profile,
        "projection": declaration.projection.as_ref().map(document_field_path_string),
        "partial_filter": declaration.partial_filter,
        "metadata": cbor_text_map_json(declaration.metadata)
    })
}

pub fn document_index_declarations_json(
    declarations: Vec<DocumentIndexDeclaration>,
) -> serde_json::Value {
    serde_json::json!({
        "indexes": declarations
            .into_iter()
            .map(document_index_declaration_json)
            .collect::<Vec<_>>()
    })
}

pub fn document_indexes_json(indexes: Vec<DocumentIndexDef>) -> serde_json::Value {
    serde_json::json!({
        "indexes": indexes
            .into_iter()
            .map(|index| {
                serde_json::json!({
                    "name": index.name,
                    "path": document_field_path_string(&index.path),
                    "unique": index.unique
                })
            })
            .collect::<Vec<_>>()
    })
}

pub fn document_index_statuses_json(statuses: Vec<DocumentIndexStatus>) -> serde_json::Value {
    serde_json::json!({
        "indexes": statuses
            .into_iter()
            .map(|status| {
                serde_json::json!({
                    "name": status.name,
                    "ready": status.ready,
                    "entries": status.entries
                })
            })
            .collect::<Vec<_>>()
    })
}

pub fn document_ids_json(ids: Vec<String>) -> serde_json::Value {
    serde_json::json!({ "ids": ids })
}

pub fn document_index_value_from_json(value: &serde_json::Value) -> Result<IndexedValue> {
    json_scalar_to_indexed_value(value)
}

fn document_predicate_from_json(value: &serde_json::Value) -> Result<DocumentPredicate> {
    if let Some(children) = value.get("and") {
        let children = children
            .as_array()
            .ok_or_else(|| crate::LoomError::invalid("and must be an array"))?;
        return children
            .iter()
            .map(document_predicate_from_json)
            .collect::<Result<Vec<_>>>()
            .map(DocumentPredicate::And);
    }
    if let Some(children) = value.get("or") {
        let children = children
            .as_array()
            .ok_or_else(|| crate::LoomError::invalid("or must be an array"))?;
        return children
            .iter()
            .map(document_predicate_from_json)
            .collect::<Result<Vec<_>>>()
            .map(DocumentPredicate::Or);
    }
    let path = value
        .get("path")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| crate::LoomError::invalid("predicate path is required"))?;
    let op = value
        .get("op")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| crate::LoomError::invalid("predicate op is required"))
        .and_then(document_cmp_op_from_str)?;
    let raw_value = value
        .get("value")
        .ok_or_else(|| crate::LoomError::invalid("predicate value is required"))?;
    Ok(DocumentPredicate::Compare {
        path: DocumentFieldPath::dotted(path)?,
        op,
        value: document_index_value_from_json(raw_value)?,
    })
}

fn document_projection_from_json(value: &serde_json::Value) -> Result<DocumentProjection> {
    let name = value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| crate::LoomError::invalid("projection name is required"))?;
    let path = value
        .get("path")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| crate::LoomError::invalid("projection path is required"))?;
    DocumentProjection::new(name, DocumentFieldPath::dotted(path)?)
}

fn document_cmp_op_from_str(value: &str) -> Result<CmpOp> {
    match value {
        "eq" | "=" => Ok(CmpOp::Eq),
        "ne" | "!=" => Ok(CmpOp::Ne),
        "lt" | "<" => Ok(CmpOp::Lt),
        "le" | "<=" => Ok(CmpOp::Le),
        "gt" | ">" => Ok(CmpOp::Gt),
        "ge" | ">=" => Ok(CmpOp::Ge),
        _ => Err(crate::LoomError::invalid(
            "unsupported document comparison op",
        )),
    }
}

fn indexed_value_json(value: IndexedValue) -> serde_json::Value {
    match value {
        IndexedValue::Null => serde_json::Value::Null,
        IndexedValue::Bool(value) => serde_json::Value::Bool(value),
        IndexedValue::Int(value) => serde_json::json!(value),
        IndexedValue::U64(value) => serde_json::json!(value),
        IndexedValue::Float(value) => serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        IndexedValue::Text(value) => serde_json::Value::String(value),
        _ => serde_json::Value::Null,
    }
}

fn evaluate_document_predicate<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    collection_value: &Collection,
    predicate: &DocumentPredicate,
) -> Result<BTreeSet<String>> {
    match predicate {
        DocumentPredicate::Compare { path, op, value } => {
            evaluate_document_compare(loom, ns, collection, collection_value, path, *op, value)
        }
        DocumentPredicate::And(predicates) => {
            if predicates.is_empty() {
                return Ok(collection_value.ids().map(str::to_string).collect());
            }
            let mut iter = predicates.iter();
            let first = evaluate_document_predicate(
                loom,
                ns,
                collection,
                collection_value,
                iter.next().expect("non-empty"),
            )?;
            iter.try_fold(first, |acc, predicate| {
                let next =
                    evaluate_document_predicate(loom, ns, collection, collection_value, predicate)?;
                Ok(acc.intersection(&next).cloned().collect())
            })
        }
        DocumentPredicate::Or(predicates) => {
            let mut out = BTreeSet::new();
            for predicate in predicates {
                out.extend(evaluate_document_predicate(
                    loom,
                    ns,
                    collection,
                    collection_value,
                    predicate,
                )?);
            }
            Ok(out)
        }
    }
}

fn evaluate_document_compare<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    collection_value: &Collection,
    path: &DocumentFieldPath,
    op: CmpOp,
    value: &IndexedValue,
) -> Result<BTreeSet<String>> {
    if let Some(index) = load_index_catalog_unchecked(loom, ns, collection)?
        .indexes()
        .into_iter()
        .find(|index| &index.path == path)
    {
        let state = try_load_index_state_unchecked(loom, ns, collection, &index.name)?.ok_or_else(
            || crate::LoomError::index_not_ready(format!("document index {:?}", index.name)),
        )?;
        return Ok(state.find_cmp(op, value));
    }
    let mut out = BTreeSet::new();
    for (id, doc) in collection_value.iter() {
        if let Some(actual) = doc_extract_index_value(doc, path)?
            && compare_indexed_value(&actual, op, value)
        {
            out.insert(id.to_string());
        }
    }
    Ok(out)
}

fn compare_indexed_value(actual: &IndexedValue, op: CmpOp, expected: &IndexedValue) -> bool {
    match op {
        CmpOp::Eq => actual == expected,
        CmpOp::Ne => actual != expected,
        CmpOp::Lt => actual < expected,
        CmpOp::Le => actual <= expected,
        CmpOp::Gt => actual > expected,
        CmpOp::Ge => actual >= expected,
    }
}

fn document_json_path<'a>(
    mut value: &'a serde_json::Value,
    path: &DocumentFieldPath,
) -> Option<&'a serde_json::Value> {
    for segment in path.segments() {
        match segment {
            DocumentFieldPathSegment::Field(field) => value = value.get(field)?,
            DocumentFieldPathSegment::Index(index) => {
                let index = usize::try_from(*index).ok()?;
                value = value.get(index)?;
            }
        }
    }
    Some(value)
}

fn json_scalar_to_indexed_value(value: &serde_json::Value) -> Result<IndexedValue> {
    match value {
        serde_json::Value::Null => Ok(IndexedValue::Null),
        serde_json::Value::Bool(value) => Ok(IndexedValue::Bool(*value)),
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                Ok(IndexedValue::Int(value))
            } else if let Some(value) = number.as_u64() {
                Ok(IndexedValue::U64(value))
            } else {
                number
                    .as_f64()
                    .filter(|value| value.is_finite())
                    .map(IndexedValue::Float)
                    .ok_or_else(|| crate::LoomError::invalid("unsupported JSON number"))
            }
        }
        serde_json::Value::String(value) => Ok(IndexedValue::Text(value.clone())),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => Err(
            crate::LoomError::invalid("document index field must resolve to a scalar JSON value"),
        ),
    }
}

fn rebuild_declared_index_states_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    value: &Collection,
) -> Result<Vec<(String, DocumentIndexState)>> {
    load_index_catalog_unchecked(loom, ns, collection)?
        .indexes()
        .into_iter()
        .map(|index| {
            Ok((
                index.name.clone(),
                build_index_state_from_collection(&index, value)?,
            ))
        })
        .collect()
}

fn write_index_states_unchecked<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    states: Vec<(String, DocumentIndexState)>,
) -> Result<()> {
    for (name, state) in states {
        put_index_state_unchecked(loom, ns, collection, &name, &state)?;
    }
    Ok(())
}

fn build_index_state_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    index: &DocumentIndexDef,
) -> Result<DocumentIndexState> {
    build_index_state_from_collection(index, &load_or_empty_unchecked(loom, ns, collection)?)
}

fn build_index_state_from_collection(
    index: &DocumentIndexDef,
    collection: &Collection,
) -> Result<DocumentIndexState> {
    let mut state = DocumentIndexState::default();
    for (id, doc) in collection.iter() {
        if let Some(value) = doc_extract_index_value(doc, &index.path)? {
            state.insert(value, id, index.unique)?;
        }
    }
    Ok(state)
}

fn prepare_indexes_for_put_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    current: &Collection,
    id: &str,
    old: Option<&[u8]>,
) -> Result<Vec<(String, DocumentIndexState)>> {
    let Some(new) = current.get(id) else {
        return Ok(Vec::new());
    };
    load_index_catalog_unchecked(loom, ns, collection)?
        .indexes()
        .into_iter()
        .map(|index| {
            let mut state = match try_load_index_state_unchecked(loom, ns, collection, &index.name)?
            {
                Some(state) => state,
                None => build_index_state_from_collection(&index, current)?,
            };
            if let Some(old_value) = old
                .map(|doc| doc_extract_index_value(doc, &index.path))
                .transpose()?
                .flatten()
            {
                state.remove(&old_value, id);
            }
            if let Some(new_value) = doc_extract_index_value(new, &index.path)? {
                state.insert(new_value, id, index.unique)?;
            }
            Ok((index.name, state))
        })
        .collect()
}

fn prepare_indexes_for_delete_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    current: &Collection,
    id: &str,
    old: Option<&[u8]>,
) -> Result<Vec<(String, DocumentIndexState)>> {
    let Some(old) = old else {
        return Ok(Vec::new());
    };
    load_index_catalog_unchecked(loom, ns, collection)?
        .indexes()
        .into_iter()
        .map(|index| {
            let mut state = match try_load_index_state_unchecked(loom, ns, collection, &index.name)?
            {
                Some(state) => state,
                None => build_index_state_from_collection(&index, current)?,
            };
            if let Some(old_value) = doc_extract_index_value(old, &index.path)? {
                state.remove(&old_value, id);
            }
            Ok((index.name, state))
        })
        .collect()
}

fn optional_text_to_cbor(value: &Option<String>) -> CborValue {
    value
        .as_ref()
        .map(|value| CborValue::Text(value.clone()))
        .unwrap_or(CborValue::Null)
}

fn optional_text_from_cbor(value: CborValue) -> Result<Option<String>> {
    match value {
        CborValue::Null => Ok(None),
        CborValue::Text(value) => Ok(Some(value)),
        _ => Err(crate::LoomError::corrupt("expected optional text")),
    }
}

fn optional_digest_to_cbor(value: &Option<Digest>) -> CborValue {
    value
        .as_ref()
        .map(cbor::digest_value)
        .unwrap_or(CborValue::Null)
}

fn optional_digest_from_cbor(value: CborValue) -> Result<Option<Digest>> {
    match value {
        CborValue::Null => Ok(None),
        value => cbor::as_digest(value).map(Some),
    }
}

fn optional_path_to_cbor(value: &Option<DocumentFieldPath>) -> CborValue {
    value
        .as_ref()
        .map(DocumentFieldPath::to_cbor)
        .unwrap_or(CborValue::Null)
}

fn optional_path_from_cbor(value: CborValue) -> Result<Option<DocumentFieldPath>> {
    match value {
        CborValue::Null => Ok(None),
        value => DocumentFieldPath::from_cbor(value).map(Some),
    }
}

fn string_list_to_cbor(values: &[String]) -> CborValue {
    CborValue::Array(values.iter().cloned().map(CborValue::Text).collect())
}

fn string_list_from_cbor(value: CborValue) -> Result<Vec<String>> {
    cbor::as_array(value)?
        .into_iter()
        .map(cbor::as_text)
        .collect()
}

fn text_map_to_cbor(values: &BTreeMap<String, CborValue>) -> CborValue {
    CborValue::Map(
        values
            .iter()
            .map(|(key, value)| (CborValue::Text(key.clone()), value.clone()))
            .collect(),
    )
}

fn text_map_from_cbor(value: CborValue) -> Result<BTreeMap<String, CborValue>> {
    cbor::as_map(value)?
        .into_iter()
        .map(|(key, value)| Ok((cbor::as_text(key)?, value)))
        .collect()
}

fn json_required_string_alias(
    object: &serde_json::Map<String, serde_json::Value>,
    primary: &str,
    alias: &str,
) -> Result<String> {
    json_optional_string(object, primary)?
        .or(json_optional_string(object, alias)?)
        .ok_or_else(|| LoomError::invalid(format!("missing document index field {primary}")))
}

fn json_optional_string(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<String>> {
    match object.get(key) {
        Some(serde_json::Value::Null) | None => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(LoomError::invalid(format!(
            "document index field {key} must be a string"
        ))),
    }
}

fn json_optional_bool(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<bool>> {
    match object.get(key) {
        Some(serde_json::Value::Null) | None => Ok(None),
        Some(serde_json::Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(LoomError::invalid(format!(
            "document index field {key} must be a bool"
        ))),
    }
}

fn json_optional_u64(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<u64>> {
    match object.get(key) {
        Some(serde_json::Value::Null) | None => Ok(None),
        Some(serde_json::Value::Number(value)) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| LoomError::invalid(format!("document index field {key} must be a u64"))),
        Some(_) => Err(LoomError::invalid(format!(
            "document index field {key} must be a u64"
        ))),
    }
}

fn json_to_cbor_value(value: &serde_json::Value) -> Result<CborValue> {
    match value {
        serde_json::Value::Null => Ok(CborValue::Null),
        serde_json::Value::Bool(value) => Ok(CborValue::Bool(*value)),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_u64() {
                Ok(CborValue::Uint(value))
            } else if let Some(value) = value.as_i64() {
                if value >= 0 {
                    Ok(CborValue::Uint(value as u64))
                } else {
                    Ok(CborValue::Nint((-1 - value) as u64))
                }
            } else if let Some(value) = value.as_f64() {
                Ok(CborValue::Float(value))
            } else {
                Err(LoomError::invalid("unsupported JSON number"))
            }
        }
        serde_json::Value::String(value) => Ok(CborValue::Text(value.clone())),
        serde_json::Value::Array(values) => values
            .iter()
            .map(json_to_cbor_value)
            .collect::<Result<Vec<_>>>()
            .map(CborValue::Array),
        serde_json::Value::Object(values) => values
            .iter()
            .map(|(key, value)| Ok((CborValue::Text(key.clone()), json_to_cbor_value(value)?)))
            .collect::<Result<Vec<_>>>()
            .map(CborValue::Map),
    }
}

fn cbor_value_json(value: CborValue) -> serde_json::Value {
    match value {
        CborValue::Null => serde_json::Value::Null,
        CborValue::Bool(value) => serde_json::Value::Bool(value),
        CborValue::Uint(value) => serde_json::Value::Number(value.into()),
        CborValue::Nint(value) => serde_json::Value::Number((-(value as i64) - 1).into()),
        CborValue::Float(value) => serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        CborValue::Bytes(value) => serde_json::Value::String(hex::encode(value)),
        CborValue::Text(value) => serde_json::Value::String(value),
        CborValue::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(cbor_value_json).collect())
        }
        CborValue::Map(values) => serde_json::Value::Object(
            values
                .into_iter()
                .filter_map(|(key, value)| {
                    let CborValue::Text(key) = key else {
                        return None;
                    };
                    Some((key, cbor_value_json(value)))
                })
                .collect(),
        ),
    }
}

fn cbor_text_map_json(values: BTreeMap<String, CborValue>) -> serde_json::Value {
    serde_json::Value::Object(
        values
            .into_iter()
            .map(|(key, value)| (key, cbor_value_json(value)))
            .collect(),
    )
}

fn validate_document_text_field(field: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.contains('\0') {
        return Err(crate::LoomError::invalid(format!("invalid {field}")));
    }
    Ok(())
}

fn validate_index_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0') {
        return Err(crate::LoomError::invalid("invalid document index name"));
    }
    Ok(())
}

fn validate_field_path_name(name: &str) -> Result<()> {
    if name.is_empty() || name.contains('\0') {
        return Err(crate::LoomError::invalid(
            "invalid document field path segment",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acl::{AclRight, AclSubject};
    use crate::error::Code;
    use crate::identity::IdentityStore;
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};

    fn test_document_manifest<S: ObjectStore>(
        loom: &Loom<S>,
        ns: WorkspaceId,
        collection: &str,
    ) -> DocumentCollectionManifest {
        load_collection_manifest_unchecked(loom, ns, collection)
            .unwrap()
            .expect("document manifest")
    }

    fn test_document_records<S: ObjectStore>(
        loom: &Loom<S>,
        ns: WorkspaceId,
        collection: &str,
    ) -> BTreeMap<String, DocumentRecord> {
        let path = normalize_path(&col_path(collection)).unwrap();
        let StagedEntry::Document(root) = loom.work.get(&ns).unwrap().get(&path).unwrap() else {
            panic!("document collection must use structured root");
        };
        let (_, documents_root) = document_root_parts(loom, *root).unwrap();
        let mut out = BTreeMap::new();
        if let Some(root) = documents_root {
            for (key, value) in crate::prolly::entries(loom.store(), &root).unwrap() {
                let DocumentId::String(id) = DocumentId::decode(&key).unwrap() else {
                    panic!("test document ids are string-backed");
                };
                out.insert(id, DocumentRecord::decode(&value).unwrap());
            }
        }
        out
    }

    fn restage_test_document_manifest<S: ObjectStore>(
        loom: &mut Loom<S>,
        ns: WorkspaceId,
        collection: &str,
        manifest: &DocumentCollectionManifest,
    ) {
        let path = normalize_path(&col_path(collection)).unwrap();
        let StagedEntry::Document(root) = loom.work.get(&ns).unwrap().get(&path).copied().unwrap()
        else {
            panic!("document collection must use structured root");
        };
        let (_, documents_root) = document_root_parts(loom, root).unwrap();
        stage_document_root_unchecked(loom, ns, collection, manifest, documents_root).unwrap();
    }

    #[test]
    fn canonical_root_manifest_round_trips() {
        let root = Digest::blake3(b"root");
        let empty = Digest::blake3(b"empty");
        let mut policy = BTreeMap::new();
        policy.insert("retained".to_string(), CborValue::Bool(true));
        let manifest = DocumentCollectionManifest {
            schema_version: 1,
            collection_id: "notes".to_string(),
            id_codec: DocumentPolicyConfig::new("document-id-envelope.v1").unwrap(),
            document_map_root: root,
            index_catalog_root: empty,
            tombstone_root: Some(empty),
            policy: DocumentCollectionPolicy::Inline(policy),
            merge_policy: DocumentPolicyConfig::new("replace-document.v1").unwrap(),
            retention_policy: DocumentPolicyConfig::new("retain-tombstones.v1").unwrap(),
            capabilities: vec!["text-access".to_string(), "binary-access".to_string()],
            metadata: BTreeMap::new(),
        };

        let decoded = DocumentCollectionManifest::decode(&manifest.encode()).unwrap();

        assert_eq!(decoded, manifest);
        assert_eq!(decoded.schema_version, 1);
        assert_eq!(decoded.tombstone_root, Some(empty));
    }

    fn sample_index_declaration(name: &str) -> DocumentIndexDeclaration {
        DocumentIndexDeclaration {
            index_id: name.to_string(),
            index_name: name.to_string(),
            source_selector: DocumentFieldPath::dotted("profile.email").unwrap(),
            extractor: "json-scalar".to_string(),
            key_codec: "loom-scalar.v1".to_string(),
            comparator: "canonical-byte-order".to_string(),
            uniqueness: DocumentIndexUniqueness::NonUnique,
            failure_policy: "skip-missing".to_string(),
            declaration_version: 1,
            analyzer_profile: None,
            projection: None,
            partial_filter: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn manifest_decode_rejects_missing_component_roots() {
        let empty = Digest::blake3(b"empty");
        let manifest = DocumentCollectionManifest {
            schema_version: 1,
            collection_id: "notes".to_string(),
            id_codec: DocumentPolicyConfig::new("document-id-envelope.v1").unwrap(),
            document_map_root: empty,
            index_catalog_root: empty,
            tombstone_root: None,
            policy: DocumentCollectionPolicy::Inline(BTreeMap::new()),
            merge_policy: DocumentPolicyConfig::new("replace-document.v1").unwrap(),
            retention_policy: DocumentPolicyConfig::new("retain-tombstones.v1").unwrap(),
            capabilities: vec!["text-access".to_string()],
            metadata: BTreeMap::new(),
        };
        let CborValue::Array(mut arr) = manifest.to_cbor() else {
            panic!("manifest encodes as a CBOR array");
        };
        // Keep format, schema_version, collection_id, id_codec; drop every required component root.
        arr.truncate(4);
        let bad = cbor::encode(&CborValue::Array(arr));
        let err = DocumentCollectionManifest::decode(&bad).unwrap_err();
        assert_eq!(err.code, Code::CorruptObject);
        assert!(
            err.message.contains("missing field"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn index_declaration_decode_rejects_missing_fields() {
        let CborValue::Array(mut arr) = sample_index_declaration("by_email").to_cbor() else {
            panic!("declaration encodes as a CBOR array");
        };
        arr.truncate(2); // index_id + index_name only; required selector/codec/... tail removed
        let bad = cbor::encode(&CborValue::Array(arr));
        let err = DocumentIndexDeclaration::decode(&bad).unwrap_err();
        assert_eq!(err.code, Code::CorruptObject);
        assert!(
            err.message.contains("missing field"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn index_catalog_decode_rejects_duplicate_index_name() {
        let decl = sample_index_declaration("by_email");
        let blob = cbor::encode(&CborValue::Array(vec![
            CborValue::Text(INDEX_CATALOG_SCHEMA.to_string()),
            CborValue::Array(vec![decl.to_cbor(), decl.to_cbor()]),
        ]));
        let err = DocumentIndexCatalog::decode(&blob).unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
        assert!(
            err.message.contains("duplicate document index"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn document_map_decode_rejects_duplicate_and_out_of_order_ids() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([41; 16]))
            .unwrap();

        let entry = |id: &str| -> CborValue {
            let document_id = DocumentId::string(id).unwrap();
            let record = DocumentRecord::new(
                document_id.clone(),
                DocumentBodyRef::Direct {
                    digest: Digest::blake3(id.as_bytes()),
                },
                id.len() as u64,
                "etag",
                "rev",
                DocumentRecordState::Live,
            )
            .unwrap();
            CborValue::Array(vec![document_id.to_cbor(), record.to_cbor()])
        };

        let duplicate = cbor::encode(&CborValue::Array(vec![
            CborValue::Text(DOCUMENT_MAP_SCHEMA.to_string()),
            CborValue::Array(vec![entry("dup"), entry("dup")]),
        ]));
        let err = decode_flat_document_map(&loom, ns, "notes", &duplicate).unwrap_err();
        assert_eq!(err.code, Code::CorruptObject);
        assert!(
            err.message.contains("duplicate document id"),
            "got: {}",
            err.message
        );

        let unordered = cbor::encode(&CborValue::Array(vec![
            CborValue::Text(DOCUMENT_MAP_SCHEMA.to_string()),
            CborValue::Array(vec![entry("b"), entry("a")]),
        ]));
        let err = decode_flat_document_map(&loom, ns, "notes", &unordered).unwrap_err();
        assert_eq!(err.code, Code::CorruptObject);
        assert!(err.message.contains("out of order"), "got: {}", err.message);
    }

    #[test]
    fn canonical_document_records_and_indexes_round_trip() {
        let document_id = DocumentId::partitioned("tenant-a", "doc-1").unwrap();
        let body = DocumentBodyRef::Direct {
            digest: Digest::blake3(b"hello"),
        };
        let record = DocumentRecord::new(
            document_id.clone(),
            body,
            5,
            "etag-1",
            "rev-1",
            DocumentRecordState::Live,
        )
        .unwrap();
        let tombstone = DocumentTombstoneRecord {
            document_id: document_id.clone(),
            deleted_entity_tag: "etag-deleted".to_string(),
            prior_entity_tag: "etag-1".to_string(),
            deletion_revision: "rev-2".to_string(),
            retention_class: "sync-gap".to_string(),
            reclaim_after: None,
            deletion_reason: None,
            source_metadata: BTreeMap::new(),
        };
        let declaration = DocumentIndexDeclaration {
            index_id: "idx-email".to_string(),
            index_name: "by_email".to_string(),
            source_selector: DocumentFieldPath::dotted("profile.email").unwrap(),
            extractor: "json-scalar".to_string(),
            key_codec: "loom-scalar.v1".to_string(),
            comparator: "canonical-byte-order".to_string(),
            uniqueness: DocumentIndexUniqueness::Unique,
            failure_policy: "skip-missing".to_string(),
            declaration_version: 1,
            analyzer_profile: None,
            projection: None,
            partial_filter: None,
            metadata: BTreeMap::new(),
        };

        assert_eq!(DocumentRecord::decode(&record.encode()).unwrap(), record);
        assert_eq!(
            DocumentTombstoneRecord::decode(&tombstone.encode()).unwrap(),
            tombstone
        );
        assert_eq!(
            DocumentIndexDeclaration::decode(&declaration.encode()).unwrap(),
            declaration
        );
        assert_eq!(
            document_id.canonical_key(),
            document_id.clone().canonical_key()
        );
    }

    #[test]
    fn document_storage_uses_structured_root_components() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([28; 16]))
            .unwrap();

        doc_put(&mut loom, ns, "notes", "a", b"one".to_vec()).unwrap();
        let path = normalize_path(&col_path("notes")).unwrap();
        assert!(matches!(
            loom.work.get(&ns).unwrap().get(&path),
            Some(StagedEntry::Document(_))
        ));
        let manifest = test_document_manifest(&loom, ns, "notes");
        assert_eq!(manifest.collection_id, "notes");
        assert_eq!(
            test_document_records(&loom, ns, "notes")
                .get("a")
                .unwrap()
                .document_id,
            DocumentId::string("a").unwrap()
        );
        let body_digest = document_digest(&loom, b"one");
        assert_eq!(
            loom.read_file_reserved(ns, &document_body_path("notes", &body_digest))
                .unwrap(),
            b"one"
        );
        assert_eq!(
            get_collection(&loom, ns, "notes").unwrap().get("a"),
            Some(&b"one"[..])
        );

        doc_put(&mut loom, ns, "notes", "a", b"two".to_vec()).unwrap();
        assert!(
            loom.read_file_reserved(ns, &document_body_path("notes", &body_digest))
                .is_err()
        );
        assert_eq!(
            doc_get(&loom, ns, "notes", "a").unwrap(),
            Some(b"two".to_vec())
        );

        let mut legacy = Collection::new();
        legacy.put("x", b"old".to_vec());
        loom.write_file_reserved(ns, &col_path("legacy"), &legacy.encode(), 0o100644)
            .unwrap();
        assert!(get_collection(&loom, ns, "legacy").is_err());
    }

    #[test]
    fn document_delete_retains_tombstone_when_policy_requires_it() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([31; 16]))
            .unwrap();
        let mut policy = DocumentPolicyConfig::new(DOCUMENT_RETENTION_POLICY_RETAIN).unwrap();
        policy.parameters.insert(
            "retention_class".to_string(),
            CborValue::Text("sync-gap".to_string()),
        );
        policy.parameters.insert(
            "reclaim_after".to_string(),
            CborValue::Text("2026-08-01T00:00:00Z".to_string()),
        );
        policy.parameters.insert(
            "deletion_reason".to_string(),
            CborValue::Text("delete-request".to_string()),
        );

        document_set_retention_policy(&mut loom, ns, "notes", policy).unwrap();
        doc_put(&mut loom, ns, "notes", "a", b"one".to_vec()).unwrap();
        let prior_entity_tag = document_entity_tag_string(&loom, b"one");
        assert!(doc_delete(&mut loom, ns, "notes", "a").unwrap());
        assert_eq!(doc_get(&loom, ns, "notes", "a").unwrap(), None);

        let manifest = test_document_manifest(&loom, ns, "notes");
        assert_eq!(
            manifest.retention_policy.name,
            DOCUMENT_RETENTION_POLICY_RETAIN
        );
        let tombstone_root = manifest.tombstone_root.expect("retained tombstone root");
        let tombstone_bytes = loom
            .read_file_reserved(ns, &document_tombstone_path("notes"))
            .unwrap();
        assert_eq!(Digest::blake3(&tombstone_bytes), tombstone_root);
        let tombstones = decode_document_tombstones(&tombstone_bytes).unwrap();
        let tombstone = tombstones.get("a").expect("retained tombstone");
        assert_eq!(tombstone.prior_entity_tag, prior_entity_tag);
        assert_eq!(tombstone.retention_class, "sync-gap");
        assert_eq!(
            tombstone.reclaim_after.as_deref(),
            Some("2026-08-01T00:00:00Z")
        );
        assert_eq!(tombstone.deletion_reason.as_deref(), Some("delete-request"));
    }

    #[test]
    fn document_delete_omits_tombstone_by_default_and_verifies_retained_root() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([32; 16]))
            .unwrap();

        doc_put(&mut loom, ns, "default", "a", b"one".to_vec()).unwrap();
        assert!(doc_delete(&mut loom, ns, "default", "a").unwrap());
        let manifest = test_document_manifest(&loom, ns, "default");
        assert_eq!(
            manifest.retention_policy.name,
            DOCUMENT_RETENTION_POLICY_NONE
        );
        assert!(manifest.tombstone_root.is_none());
        assert!(
            loom.read_file_reserved(ns, &document_tombstone_path("default"))
                .is_err()
        );

        document_set_retention_policy(
            &mut loom,
            ns,
            "retained",
            DocumentPolicyConfig::new(DOCUMENT_RETENTION_POLICY_RETAIN).unwrap(),
        )
        .unwrap();
        doc_put(&mut loom, ns, "retained", "a", b"one".to_vec()).unwrap();
        assert!(doc_delete(&mut loom, ns, "retained", "a").unwrap());
        let mut retained_manifest = test_document_manifest(&loom, ns, "retained");
        retained_manifest.tombstone_root = Some(Digest::blake3(b"wrong"));
        restage_test_document_manifest(&mut loom, ns, "retained", &retained_manifest);
        let error = get_collection(&loom, ns, "retained").unwrap_err();
        assert_eq!(error.code, Code::CorruptObject);
        assert!(error.message.contains("tombstone root"));
    }

    #[test]
    fn document_large_body_uses_chunked_body_ref() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([29; 16]))
            .unwrap();
        let large = (0..=DOCUMENT_CHUNK_THRESHOLD)
            .map(|i| (i % 251) as u8)
            .collect::<Vec<_>>();

        doc_put(&mut loom, ns, "notes", "large", large.clone()).unwrap();
        assert_eq!(doc_get(&loom, ns, "notes", "large").unwrap(), Some(large));

        let manifest = test_document_manifest(&loom, ns, "notes");
        let records = test_document_records(&loom, ns, "notes");
        assert_eq!(records.len(), 1);
        let record = records.get("large").unwrap();
        assert_eq!(manifest.document_map_root, {
            let path = normalize_path(&col_path("notes")).unwrap();
            let StagedEntry::Document(root) = loom.work.get(&ns).unwrap().get(&path).unwrap()
            else {
                panic!("document collection must use structured root");
            };
            document_root_parts(&loom, *root).unwrap().1.unwrap()
        });

        let root = match record.body_ref {
            DocumentBodyRef::Chunked { root } => root,
            DocumentBodyRef::Direct { .. } => panic!("large document must be chunked"),
        };
        let chunk_list = Object::decode(
            &loom
                .read_file_reserved(ns, &document_body_path("notes", &root))
                .unwrap(),
        )
        .unwrap();
        let Object::ChunkList {
            total_size,
            entries,
        } = chunk_list
        else {
            panic!("chunked body root must store a ChunkList");
        };
        assert_eq!(total_size, (DOCUMENT_CHUNK_THRESHOLD + 1) as u64);
        assert!(!entries.is_empty());
        for chunk in entries {
            let bytes = loom
                .read_file_reserved(ns, &document_chunk_path("notes", &chunk.target))
                .unwrap();
            assert_eq!(bytes.len() as u64, chunk.size);
            assert_eq!(
                Object::Blob(bytes).digest_with(loom.store().digest_algo()),
                chunk.target
            );
        }
    }

    #[test]
    fn document_body_ref_threshold_boundary_is_pinned() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([30; 16]))
            .unwrap();
        let small = vec![b'a'; DOCUMENT_CHUNK_THRESHOLD];
        let large = vec![b'b'; DOCUMENT_CHUNK_THRESHOLD + 1];

        doc_put(&mut loom, ns, "notes", "small", small).unwrap();
        doc_put(&mut loom, ns, "notes", "large", large).unwrap();
        let records = test_document_records(&loom, ns, "notes");
        assert_eq!(records.len(), 2);
        let mut refs = BTreeMap::new();
        for (id, record) in records {
            refs.insert(id, record.body_ref);
        }
        assert!(matches!(
            refs.get("small").unwrap(),
            DocumentBodyRef::Direct { .. }
        ));
        assert!(matches!(
            refs.get("large").unwrap(),
            DocumentBodyRef::Chunked { .. }
        ));
    }

    #[test]
    fn document_merge_combines_independent_id_changes() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([46; 16]))
            .unwrap();
        doc_put(&mut loom, ns, "notes", "base", b"base".to_vec()).unwrap();
        loom.commit(ns, "nas", "base", 1).unwrap();
        loom.branch(ns, "feature").unwrap();

        doc_put(&mut loom, ns, "notes", "ours", b"ours".to_vec()).unwrap();
        loom.commit(ns, "nas", "ours", 2).unwrap();

        loom.checkout_branch(ns, "feature").unwrap();
        doc_put(&mut loom, ns, "notes", "theirs", b"theirs".to_vec()).unwrap();
        loom.commit(ns, "nas", "theirs", 3).unwrap();

        loom.checkout_branch(ns, "main").unwrap();
        assert!(matches!(
            loom.merge(ns, "feature", "nas", 4).unwrap(),
            crate::vcs::MergeOutcome::Merged(_)
        ));
        let collection = get_collection(&loom, ns, "notes").unwrap();
        assert_eq!(collection.get("base"), Some(&b"base"[..]));
        assert_eq!(collection.get("ours"), Some(&b"ours"[..]));
        assert_eq!(collection.get("theirs"), Some(&b"theirs"[..]));
    }

    #[test]
    fn list_collections_enumerates_collection_names_sorted() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([6; 16]))
            .unwrap();
        assert!(doc_list_collections(&loom, ns).unwrap().is_empty());
        doc_put(&mut loom, ns, "notes", "1", br#"{"a":1}"#.to_vec()).unwrap();
        doc_put(&mut loom, ns, "notes", "2", br#"{"a":2}"#.to_vec()).unwrap();
        doc_put(&mut loom, ns, "events", "1", br#"{"b":1}"#.to_vec()).unwrap();
        assert_eq!(
            doc_list_collections(&loom, ns).unwrap(),
            vec!["events", "notes"]
        );
    }

    #[test]
    fn list_collections_hides_structured_root_internal_collections() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([46; 16]))
            .unwrap();
        doc_put(&mut loom, ns, "people", "ann", br#"{"x":1}"#.to_vec()).unwrap();
        doc_create_index(
            &mut loom,
            ns,
            "people",
            DocumentIndexDef::new("by_x", DocumentFieldPath::dotted("x").unwrap(), false).unwrap(),
        )
        .unwrap();

        // The structured root stages reserved internal roots (.maps/.bodies/.indexes/.index-data)
        // under the document facet dir. Neither the generic facet lister nor the document lister may
        // present them as user collections.
        let generic = loom.list_collections(ns, FacetKind::Document);
        assert_eq!(generic, vec!["people".to_string()]);
        assert!(!generic.iter().any(|name| name.starts_with('.')));
        assert_eq!(
            doc_list_collections(&loom, ns).unwrap(),
            vec!["people".to_string()]
        );
    }

    #[test]
    fn put_get_delete_and_order() {
        let mut c = Collection::new();
        c.put("b", br#"{"x":2}"#.to_vec());
        c.put("a", br#"{"x":1}"#.to_vec());
        assert_eq!(c.ids().collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(c.get("a"), Some(&br#"{"x":1}"#[..]));
        assert!(c.delete("a"));
        assert_eq!(c.get("a"), None);
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn document_index_catalog_round_trips_in_canonical_order() {
        let mut catalog = DocumentIndexCatalog::new();
        catalog
            .insert(
                DocumentIndexDef::new(
                    "by_email",
                    DocumentFieldPath::dotted("profile.email").unwrap(),
                    true,
                )
                .unwrap(),
            )
            .unwrap();
        catalog
            .insert(
                DocumentIndexDef::new(
                    "by_age",
                    DocumentFieldPath::dotted("profile.age").unwrap(),
                    false,
                )
                .unwrap(),
            )
            .unwrap();

        let decoded = DocumentIndexCatalog::decode(&catalog.encode()).unwrap();
        assert_eq!(
            decoded
                .indexes()
                .iter()
                .map(|index| index.name.as_str())
                .collect::<Vec<_>>(),
            vec!["by_age", "by_email"]
        );
        assert!(
            DocumentIndexDef::new("bad/name", DocumentFieldPath::dotted("x").unwrap(), false)
                .is_err()
        );
        assert!(DocumentFieldPath::dotted("profile..email").is_err());
    }

    #[test]
    fn document_index_catalog_stores_canonical_declarations() {
        let mut catalog = DocumentIndexCatalog::new();
        catalog
            .insert(
                DocumentIndexDef::new(
                    "by_email",
                    DocumentFieldPath::dotted("profile.email").unwrap(),
                    true,
                )
                .unwrap(),
            )
            .unwrap();

        let decoded = DocumentIndexCatalog::decode(&catalog.encode()).unwrap();
        let declaration = decoded
            .declaration("by_email")
            .expect("canonical declaration present");
        assert_eq!(declaration.index_id, "by_email");
        assert_eq!(declaration.index_name, "by_email");
        assert_eq!(
            declaration.source_selector,
            DocumentFieldPath::dotted("profile.email").unwrap()
        );
        assert_eq!(declaration.extractor, "json-scalar");
        assert_eq!(declaration.key_codec, "loom-scalar.v1");
        assert_eq!(declaration.comparator, "canonical-byte-order");
        assert_eq!(declaration.failure_policy, "skip-missing");
        assert_eq!(declaration.uniqueness, DocumentIndexUniqueness::Unique);
        assert_eq!(declaration.declaration_version, 1);
    }

    #[test]
    fn document_index_declaration_json_projects_full_surface() {
        let declaration = document_index_declaration_from_json(&serde_json::json!({
            "index_id": "idx_email",
            "index_name": "by_email",
            "source_selector": "profile.email",
            "extractor": "json-scalar",
            "key_codec": "loom-scalar.v1",
            "comparator": "canonical-byte-order",
            "uniqueness": "unique",
            "failure_policy": "skip-missing",
            "declaration_version": 1,
            "analyzer_profile": "email.v1",
            "projection": "profile.name",
            "partial_filter": "profile.email exists",
            "metadata": {"owner": "ops"}
        }))
        .unwrap();

        assert_eq!(declaration.index_id, "idx_email");
        assert_eq!(declaration.index_name, "by_email");
        assert_eq!(declaration.uniqueness, DocumentIndexUniqueness::Unique);
        let json = document_index_declaration_json(declaration);
        assert_eq!(json["index_id"], "idx_email");
        assert_eq!(json["index_name"], "by_email");
        assert_eq!(json["source_selector"], "profile.email");
        assert_eq!(json["unique"], true);
        assert_eq!(json["metadata"]["owner"], "ops");
    }

    #[test]
    fn document_index_ddl_refreshes_catalog_root_and_preserves_document_records() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([44; 16]))
            .unwrap();
        doc_put(
            &mut loom,
            ns,
            "people",
            "ann",
            br#"{"profile":{"email":"ann@example.com"}}"#.to_vec(),
        )
        .unwrap();

        let before = test_document_manifest(&loom, ns, "people");

        doc_create_index(
            &mut loom,
            ns,
            "people",
            DocumentIndexDef::new(
                "by_email",
                DocumentFieldPath::dotted("profile.email").unwrap(),
                true,
            )
            .unwrap(),
        )
        .unwrap();

        let after = test_document_manifest(&loom, ns, "people");

        // Creating an index refreshes index_catalog_root atomically but never touches the document
        // map, so document records and their entity tags are preserved.
        assert_ne!(before.index_catalog_root, after.index_catalog_root);
        assert_eq!(before.document_map_root, after.document_map_root);
        assert!(get_collection(&loom, ns, "people").is_ok());

        // Dropping the index refreshes the root back to the canonical empty-catalog root.
        assert!(doc_drop_index(&mut loom, ns, "people", "by_email").unwrap());
        let dropped = test_document_manifest(&loom, ns, "people");
        assert_eq!(before.index_catalog_root, dropped.index_catalog_root);
        assert_eq!(before.document_map_root, dropped.document_map_root);
        assert!(get_collection(&loom, ns, "people").is_ok());
    }

    #[test]
    fn document_load_rejects_stale_index_catalog_root() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([45; 16]))
            .unwrap();
        doc_put(&mut loom, ns, "people", "ann", br#"{"x":1}"#.to_vec()).unwrap();

        let mut manifest = test_document_manifest(&loom, ns, "people");
        manifest.index_catalog_root = Digest::blake3(b"tampered-index-catalog-root");
        restage_test_document_manifest(&mut loom, ns, "people", &manifest);

        let error =
            get_collection(&loom, ns, "people").expect_err("stale index catalog root must fail");
        assert_eq!(error.code, Code::CorruptObject);
        assert!(
            error
                .message
                .contains("document index catalog root mismatch")
        );
    }

    #[test]
    fn document_index_declarations_are_stored_separately_from_documents() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([26; 16]))
            .unwrap();
        doc_put(
            &mut loom,
            ns,
            "people",
            "ann",
            br#"{"profile":{"email":"ann@example.com"}}"#.to_vec(),
        )
        .unwrap();
        doc_create_index(
            &mut loom,
            ns,
            "people",
            DocumentIndexDef::new(
                "by_email",
                DocumentFieldPath::dotted("profile.email").unwrap(),
                true,
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            doc_list_indexes(&loom, ns, "people")
                .unwrap()
                .iter()
                .map(|index| index.name.as_str())
                .collect::<Vec<_>>(),
            vec!["by_email"]
        );
        assert_eq!(doc_list_collections(&loom, ns).unwrap(), vec!["people"]);
        assert!(doc_drop_index(&mut loom, ns, "people", "by_email").unwrap());
        assert!(doc_list_indexes(&loom, ns, "people").unwrap().is_empty());
    }

    #[test]
    fn document_indexes_backfill_and_find_exact_matches() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([27; 16]))
            .unwrap();
        doc_put(
            &mut loom,
            ns,
            "people",
            "ann",
            br#"{"profile":{"city":"Paris"}}"#.to_vec(),
        )
        .unwrap();
        doc_put(
            &mut loom,
            ns,
            "people",
            "bob",
            br#"{"profile":{"city":"Paris"}}"#.to_vec(),
        )
        .unwrap();
        doc_put(
            &mut loom,
            ns,
            "people",
            "cat",
            br#"{"profile":{"city":"Berlin"}}"#.to_vec(),
        )
        .unwrap();

        doc_create_index(
            &mut loom,
            ns,
            "people",
            DocumentIndexDef::new(
                "by_city",
                DocumentFieldPath::dotted("profile.city").unwrap(),
                false,
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            doc_find(
                &loom,
                ns,
                "people",
                "by_city",
                &IndexedValue::Text("Paris".to_string())
            )
            .unwrap(),
            vec!["ann".to_string(), "bob".to_string()]
        );
        assert_eq!(
            doc_index_statuses(&loom, ns, "people").unwrap(),
            vec![DocumentIndexStatus {
                name: "by_city".to_string(),
                ready: true,
                entries: 3,
            }]
        );
    }

    #[test]
    fn document_indexes_are_maintained_on_put_and_delete() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([28; 16]))
            .unwrap();
        doc_put(
            &mut loom,
            ns,
            "people",
            "ann",
            br#"{"profile":{"city":"Paris"}}"#.to_vec(),
        )
        .unwrap();
        doc_create_index(
            &mut loom,
            ns,
            "people",
            DocumentIndexDef::new(
                "by_city",
                DocumentFieldPath::dotted("profile.city").unwrap(),
                false,
            )
            .unwrap(),
        )
        .unwrap();

        doc_put(
            &mut loom,
            ns,
            "people",
            "ann",
            br#"{"profile":{"city":"Rome"}}"#.to_vec(),
        )
        .unwrap();
        assert!(
            doc_find(
                &loom,
                ns,
                "people",
                "by_city",
                &IndexedValue::Text("Paris".to_string())
            )
            .unwrap()
            .is_empty()
        );
        assert_eq!(
            doc_find(
                &loom,
                ns,
                "people",
                "by_city",
                &IndexedValue::Text("Rome".to_string())
            )
            .unwrap(),
            vec!["ann".to_string()]
        );

        assert!(doc_delete(&mut loom, ns, "people", "ann").unwrap());
        assert!(
            doc_find(
                &loom,
                ns,
                "people",
                "by_city",
                &IndexedValue::Text("Rome".to_string())
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn document_unique_indexes_reject_duplicates_before_document_write() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([29; 16]))
            .unwrap();
        doc_put(
            &mut loom,
            ns,
            "people",
            "ann",
            br#"{"profile":{"email":"ann@example.com"}}"#.to_vec(),
        )
        .unwrap();
        doc_create_index(
            &mut loom,
            ns,
            "people",
            DocumentIndexDef::new(
                "by_email",
                DocumentFieldPath::dotted("profile.email").unwrap(),
                true,
            )
            .unwrap(),
        )
        .unwrap();

        let err = doc_put(
            &mut loom,
            ns,
            "people",
            "bob",
            br#"{"profile":{"email":"ann@example.com"}}"#.to_vec(),
        )
        .unwrap_err();
        assert_eq!(err.code, Code::Conflict);
        assert!(doc_get(&loom, ns, "people", "bob").unwrap().is_none());
        assert_eq!(
            doc_find(
                &loom,
                ns,
                "people",
                "by_email",
                &IndexedValue::Text("ann@example.com".to_string())
            )
            .unwrap(),
            vec!["ann".to_string()]
        );
    }

    #[test]
    fn document_index_rebuild_and_drop_manage_readiness_state() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([30; 16]))
            .unwrap();
        doc_put(
            &mut loom,
            ns,
            "people",
            "ann",
            br#"{"profile":{"city":"Paris"}}"#.to_vec(),
        )
        .unwrap();
        doc_create_index(
            &mut loom,
            ns,
            "people",
            DocumentIndexDef::new(
                "by_city",
                DocumentFieldPath::dotted("profile.city").unwrap(),
                false,
            )
            .unwrap(),
        )
        .unwrap();

        loom.remove_file_reserved(ns, &index_state_path("people", "by_city"))
            .unwrap();
        assert_eq!(
            doc_index_statuses(&loom, ns, "people").unwrap(),
            vec![DocumentIndexStatus {
                name: "by_city".to_string(),
                ready: false,
                entries: 0,
            }]
        );
        assert_eq!(
            doc_find(
                &loom,
                ns,
                "people",
                "by_city",
                &IndexedValue::Text("Paris".to_string())
            )
            .unwrap_err()
            .code,
            Code::IndexNotReady
        );

        doc_rebuild_index(&mut loom, ns, "people", "by_city").unwrap();
        assert_eq!(
            doc_find(
                &loom,
                ns,
                "people",
                "by_city",
                &IndexedValue::Text("Paris".to_string())
            )
            .unwrap(),
            vec!["ann".to_string()]
        );
        assert!(doc_drop_index(&mut loom, ns, "people", "by_city").unwrap());
        assert_eq!(
            loom.read_file_reserved(ns, &index_state_path("people", "by_city"))
                .unwrap_err()
                .code,
            Code::NotFound
        );
    }

    #[test]
    fn document_query_supports_range_boolean_cursor_and_projection() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([31; 16]))
            .unwrap();
        for (id, age, active, city) in [
            ("ann", 31, true, "Paris"),
            ("bob", 44, false, "Paris"),
            ("cat", 27, true, "Berlin"),
            ("dan", 51, true, "Rome"),
        ] {
            doc_put(
                &mut loom,
                ns,
                "people",
                id,
                format!(r#"{{"profile":{{"age":{age},"active":{active},"city":"{city}"}}}}"#)
                    .into_bytes(),
            )
            .unwrap();
        }
        doc_create_index(
            &mut loom,
            ns,
            "people",
            DocumentIndexDef::new(
                "by_age",
                DocumentFieldPath::dotted("profile.age").unwrap(),
                false,
            )
            .unwrap(),
        )
        .unwrap();
        doc_create_index(
            &mut loom,
            ns,
            "people",
            DocumentIndexDef::new(
                "by_active",
                DocumentFieldPath::dotted("profile.active").unwrap(),
                false,
            )
            .unwrap(),
        )
        .unwrap();

        let query = DocumentQuery {
            predicate: Some(DocumentPredicate::And(vec![
                DocumentPredicate::Compare {
                    path: DocumentFieldPath::dotted("profile.age").unwrap(),
                    op: CmpOp::Ge,
                    value: IndexedValue::Int(31),
                },
                DocumentPredicate::Compare {
                    path: DocumentFieldPath::dotted("profile.active").unwrap(),
                    op: CmpOp::Eq,
                    value: IndexedValue::Bool(true),
                },
            ])),
            projections: vec![
                DocumentProjection::new("city", DocumentFieldPath::dotted("profile.city").unwrap())
                    .unwrap(),
            ],
            cursor: None,
            limit: 1,
            include_document: false,
        };
        let first = doc_query(&loom, ns, "people", &query).unwrap();
        assert_eq!(first.items.len(), 1);
        assert_eq!(first.items[0].id, "ann");
        assert_eq!(
            first.items[0].projections.get("city"),
            Some(&Some(IndexedValue::Text("Paris".to_string())))
        );
        assert_eq!(first.next_cursor.as_deref(), Some("ann"));

        let next = doc_query(
            &loom,
            ns,
            "people",
            &DocumentQuery {
                cursor: Some("ann".to_string()),
                limit: 10,
                ..query
            },
        )
        .unwrap();
        assert_eq!(
            next.items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["dan"]
        );
    }

    #[test]
    fn document_query_or_falls_back_to_scan_when_no_index_matches() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([32; 16]))
            .unwrap();
        doc_put(
            &mut loom,
            ns,
            "people",
            "ann",
            br#"{"profile":{"city":"Paris"}}"#.to_vec(),
        )
        .unwrap();
        doc_put(
            &mut loom,
            ns,
            "people",
            "bob",
            br#"{"profile":{"city":"Berlin"}}"#.to_vec(),
        )
        .unwrap();
        let result = doc_query(
            &loom,
            ns,
            "people",
            &DocumentQuery {
                predicate: Some(DocumentPredicate::Or(vec![
                    DocumentPredicate::Compare {
                        path: DocumentFieldPath::dotted("profile.city").unwrap(),
                        op: CmpOp::Eq,
                        value: IndexedValue::Text("Paris".to_string()),
                    },
                    DocumentPredicate::Compare {
                        path: DocumentFieldPath::dotted("profile.city").unwrap(),
                        op: CmpOp::Eq,
                        value: IndexedValue::Text("Rome".to_string()),
                    },
                ])),
                limit: 100,
                ..DocumentQuery::new()
            },
        )
        .unwrap();
        assert_eq!(
            result
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["ann"]
        );
    }

    #[test]
    fn document_index_value_extraction_normalizes_json_scalars() {
        let doc = br#"{
            "profile": {"email": "ann@example.com", "age": 42, "active": true},
            "visits": [10, 20],
            "big": 18446744073709551615,
            "score": 3.5,
            "none": null
        }"#;

        assert_eq!(
            doc_extract_index_value(doc, &DocumentFieldPath::dotted("profile.email").unwrap())
                .unwrap(),
            Some(IndexedValue::Text("ann@example.com".into()))
        );
        assert_eq!(
            doc_extract_index_value(doc, &DocumentFieldPath::dotted("profile.age").unwrap())
                .unwrap(),
            Some(IndexedValue::Int(42))
        );
        assert_eq!(
            doc_extract_index_value(doc, &DocumentFieldPath::dotted("profile.active").unwrap())
                .unwrap(),
            Some(IndexedValue::Bool(true))
        );
        assert_eq!(
            doc_extract_index_value(doc, &DocumentFieldPath::dotted("big").unwrap()).unwrap(),
            Some(IndexedValue::U64(u64::MAX))
        );
        assert_eq!(
            doc_extract_index_value(doc, &DocumentFieldPath::dotted("score").unwrap()).unwrap(),
            Some(IndexedValue::Float(3.5))
        );
        assert_eq!(
            doc_extract_index_value(doc, &DocumentFieldPath::dotted("none").unwrap()).unwrap(),
            Some(IndexedValue::Null)
        );
        assert_eq!(
            doc_extract_index_value(doc, &DocumentFieldPath::dotted("missing").unwrap()).unwrap(),
            None
        );
        assert_eq!(
            doc_extract_index_value(
                doc,
                &DocumentFieldPath::new(vec![
                    DocumentFieldPathSegment::Field("visits".to_string()),
                    DocumentFieldPathSegment::Index(1),
                ])
                .unwrap(),
            )
            .unwrap(),
            Some(IndexedValue::Int(20))
        );
        assert!(
            doc_extract_index_value(doc, &DocumentFieldPath::dotted("profile").unwrap()).is_err()
        );
    }

    #[test]
    fn document_text_and_binary_facade_reports_digest_and_guards_updates() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([33; 16]))
            .unwrap();

        let digest = document_put_text(&mut loom, ns, "notes", "a", "one\né", None).unwrap();
        assert_eq!(
            digest,
            Digest::hash(loom.store().digest_algo(), "one\né".as_bytes())
        );
        let text = document_get_text(&loom, ns, "notes", "a")
            .unwrap()
            .expect("text document");
        assert_eq!(text.text, "one\né");
        assert_eq!(text.digest, digest);
        let text_bytes = document_get_binary(&loom, ns, "notes", "a")
            .unwrap()
            .expect("text document as bytes");
        assert_eq!(text_bytes.bytes, "one\né".as_bytes());
        assert_eq!(text_bytes.digest, digest);

        let stale_digest = Digest::hash(loom.store().digest_algo(), b"not-current");
        assert_eq!(
            document_put_text(&mut loom, ns, "notes", "a", "stale", Some(&stale_digest))
                .unwrap_err()
                .code,
            Code::CasMismatch
        );

        let updated = document_put_text(&mut loom, ns, "notes", "a", "two", Some(&digest)).unwrap();
        assert_eq!(
            document_get_text(&loom, ns, "notes", "a")
                .unwrap()
                .expect("updated text")
                .text,
            "two"
        );
        assert_ne!(updated, digest);

        let old_tag = document_entity_tag(&loom, b"two");
        let stale_tag = EntityTag::opaque(b"stale-document-tag");
        assert_eq!(
            document_put_binary_with_request(
                &mut loom,
                ns,
                "notes",
                "a",
                b"stale".to_vec(),
                MutationRequest::new(MutationMode::ReplaceIfMatch(stale_tag))
            )
            .unwrap_err()
            .code,
            Code::Conflict
        );
        let tagged = document_put_binary_with_request(
            &mut loom,
            ns,
            "notes",
            "a",
            b"three".to_vec(),
            MutationRequest::new(MutationMode::ReplaceIfMatch(old_tag)),
        )
        .unwrap();
        assert_eq!(
            tagged.outcome.entity_tag,
            Some(document_entity_tag(&loom, b"three"))
        );
        assert_ne!(tagged.digest, updated);

        assert_eq!(
            document_put_binary_with_request(
                &mut loom,
                ns,
                "notes",
                "a",
                b"collision".to_vec(),
                MutationRequest::new(MutationMode::CreateIfAbsent)
            )
            .unwrap_err()
            .code,
            Code::Conflict
        );
        let delete_outcome = document_delete_with_request(
            &mut loom,
            ns,
            "notes",
            "a",
            MutationRequest::new(MutationMode::DeleteIfPresent),
        )
        .unwrap();
        assert_eq!(
            delete_outcome.disposition,
            loom_types::CompareDisposition::Applied
        );
        assert!(
            document_get_binary(&loom, ns, "notes", "a")
                .unwrap()
                .is_none()
        );

        let binary_digest =
            document_put_binary(&mut loom, ns, "bin", "raw", vec![0xff, 0xfe], None).unwrap();
        let binary = document_get_binary(&loom, ns, "bin", "raw")
            .unwrap()
            .expect("binary document");
        assert_eq!(binary.bytes, vec![0xff, 0xfe]);
        assert_eq!(binary.digest, binary_digest);
        assert_eq!(
            document_get_text(&loom, ns, "bin", "raw").unwrap_err().code,
            Code::DocumentNotText
        );

        let listed = Collection::decode(
            &document_list_binary(&loom, ns, "notes").expect("binary collection"),
        )
        .unwrap();
        assert!(listed.get("a").is_none());
    }

    #[test]
    fn authenticated_document_operations_are_acl_checked() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([24; 16]))
            .unwrap();
        let root = WorkspaceId::from_bytes([1; 16]);
        let mut identity = IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);

        assert_eq!(
            doc_put(&mut loom, ns, "c", "a", b"one".to_vec())
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );

        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Document),
                [AclRight::Write, AclRight::Read],
            )
            .unwrap();

        doc_put(&mut loom, ns, "c", "a", b"one".to_vec()).unwrap();
        assert_eq!(
            doc_get(&loom, ns, "c", "a").unwrap().as_deref(),
            Some(&b"one"[..])
        );
    }

    #[test]
    fn authenticated_document_operations_honor_collection_scopes() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([25; 16]))
            .unwrap();
        let root = WorkspaceId::from_bytes([1; 16]);
        let mut identity = IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);
        loom.acl_store_mut()
            .grant(crate::AclGrant {
                subject: AclSubject::Principal(root),
                workspace: Some(ns),
                domain: Some(FacetKind::Document.into()),
                ref_glob: None,
                scopes: vec![crate::AclScope::Prefix {
                    kind: crate::AclScopeKind::Collection,
                    prefix: b"work".to_vec(),
                }],
                rights: [AclRight::Write, AclRight::Read].into_iter().collect(),
                effect: crate::AclEffect::Allow,
                predicate: None,
            })
            .unwrap();

        doc_put(&mut loom, ns, "work", "a", b"one".to_vec()).unwrap();
        assert_eq!(
            doc_get(&loom, ns, "work", "a").unwrap().as_deref(),
            Some(&b"one"[..])
        );
        assert_eq!(
            doc_put(&mut loom, ns, "private", "a", b"two".to_vec())
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
    }

    #[test]
    fn encode_round_trips_and_versions() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([4; 16]))
            .unwrap();
        let mut c = Collection::new();
        c.put("d1", b"one".to_vec());
        c.put("d2", b"two".to_vec());
        assert_eq!(Collection::decode(&c.encode()).unwrap().len(), 2);

        put_collection(&mut loom, ns, "people", &c).unwrap();
        let c1 = loom.commit(ns, "nas", "two docs", 1).unwrap();
        c.put("d3", b"three".to_vec());
        put_collection(&mut loom, ns, "people", &c).unwrap();
        loom.commit(ns, "nas", "three docs", 2).unwrap();
        assert_eq!(get_collection(&loom, ns, "people").unwrap().len(), 3);
        loom.checkout_commit(ns, c1).unwrap();
        assert_eq!(get_collection(&loom, ns, "people").unwrap().len(), 2);
    }

    #[test]
    fn facade_put_get_delete_and_absent() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([6; 16]))
            .unwrap();

        assert_eq!(doc_get(&loom, ns, "c", "x").unwrap(), None);
        assert_eq!(doc_list(&loom, ns, "c").unwrap().len(), 0);

        doc_put(&mut loom, ns, "c", "a", b"one".to_vec()).unwrap();
        doc_put(&mut loom, ns, "c", "b", b"two".to_vec()).unwrap();
        assert_eq!(
            doc_get(&loom, ns, "c", "a").unwrap().as_deref(),
            Some(&b"one"[..])
        );
        // A later put replaces the document.
        doc_put(&mut loom, ns, "c", "a", b"uno".to_vec()).unwrap();
        assert_eq!(
            doc_get(&loom, ns, "c", "a").unwrap().as_deref(),
            Some(&b"uno"[..])
        );
        assert_eq!(doc_list(&loom, ns, "c").unwrap().len(), 2);

        assert!(doc_delete(&mut loom, ns, "c", "a").unwrap());
        assert!(!doc_delete(&mut loom, ns, "c", "a").unwrap());
        assert_eq!(doc_get(&loom, ns, "c", "a").unwrap(), None);
    }
}
