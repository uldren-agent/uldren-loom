//! Durable-local derived artifacts for native indexes and analytical accelerators.

use super::*;
use loom_core::{
    CompressionHint, FacetKind, ObjectStore, WorkspaceId,
    capability::{CapabilityDegradation, CapabilityOperationalState, CapabilitySet},
};

pub(crate) const DERIVED_PREFIX: &[u8] = b"derived/artifact/v1/";
const REBUILD_PREFIX: &[u8] = b"derived/rebuild/v1/";
const RECORD_MAGIC: &[u8; 5] = b"LDAR1";
const REBUILD_MAGIC: &[u8; 5] = b"LDRB1";
const MAX_FIELD_LEN: usize = u16::MAX as usize;
pub const SEARCH_TANTIVY_ARTIFACT: &str = "tantivy";
pub const SEARCH_TANTIVY_FORMAT_VERSION: &str = "search-tantivy-v1";
pub const SEARCH_EMBEDDING_ARTIFACT_PREFIX: &str = "embedding:";
pub const SEARCH_EMBEDDING_FORMAT_VERSION: &str = "search-embedding-v1";
pub const COLUMNAR_ARROW_ARTIFACT: &str = "arrow";
pub const COLUMNAR_ARROW_FORMAT_VERSION: &str = "columnar-arrow-ipc-v1";
pub const GRAPH_PROPERTY_INDEX_ARTIFACT_PREFIX: &str = "property-index:";
pub const GRAPH_PROPERTY_INDEX_FORMAT_VERSION: &str = "graph-property-index-v1";
pub const GRAPH_SPATIAL_INDEX_ARTIFACT_PREFIX: &str = "spatial-index:";
pub const GRAPH_SPATIAL_INDEX_FORMAT_VERSION: &str = "graph-spatial-index-v1";
pub const DATAFRAME_MATERIALIZATION_ARTIFACT_PREFIX: &str = "materialization:";
pub const DATAFRAME_MATERIALIZATION_FORMAT_VERSION: &str = "dataframe-materialization-v1";
pub const PIM_DERIVED_INDEX_ARTIFACT_PREFIX: &str = "derived-index:";
pub const CALENDAR_DERIVED_INDEX_FORMAT_VERSION: &str = "calendar-derived-index-v1";
pub const CONTACTS_DERIVED_INDEX_FORMAT_VERSION: &str = "contacts-derived-index-v1";
pub const MAIL_DERIVED_INDEX_FORMAT_VERSION: &str = "mail-derived-index-v1";
pub const VECTOR_PQ_ARTIFACT: &str = "pq";
pub const VECTOR_PQ_FORMAT_VERSION: &str = "vector-pq-v1";
pub const VECTOR_HNSW_ARTIFACT: &str = "hnsw";
pub const VECTOR_HNSW_FORMAT_VERSION: &str = "vector-hnsw-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivedArtifactKind {
    pub facet: FacetKind,
    pub artifact_family: &'static str,
    pub format_version: &'static str,
}

pub const SEARCH_TANTIVY_KIND: DerivedArtifactKind = DerivedArtifactKind {
    facet: FacetKind::Search,
    artifact_family: SEARCH_TANTIVY_ARTIFACT,
    format_version: SEARCH_TANTIVY_FORMAT_VERSION,
};

pub const SEARCH_EMBEDDING_KIND: DerivedArtifactKind = DerivedArtifactKind {
    facet: FacetKind::Search,
    artifact_family: SEARCH_EMBEDDING_ARTIFACT_PREFIX,
    format_version: SEARCH_EMBEDDING_FORMAT_VERSION,
};

pub const COLUMNAR_ARROW_KIND: DerivedArtifactKind = DerivedArtifactKind {
    facet: FacetKind::Columnar,
    artifact_family: COLUMNAR_ARROW_ARTIFACT,
    format_version: COLUMNAR_ARROW_FORMAT_VERSION,
};

pub const GRAPH_PROPERTY_INDEX_KIND: DerivedArtifactKind = DerivedArtifactKind {
    facet: FacetKind::Graph,
    artifact_family: GRAPH_PROPERTY_INDEX_ARTIFACT_PREFIX,
    format_version: GRAPH_PROPERTY_INDEX_FORMAT_VERSION,
};

pub const GRAPH_SPATIAL_INDEX_KIND: DerivedArtifactKind = DerivedArtifactKind {
    facet: FacetKind::Graph,
    artifact_family: GRAPH_SPATIAL_INDEX_ARTIFACT_PREFIX,
    format_version: GRAPH_SPATIAL_INDEX_FORMAT_VERSION,
};

pub const DATAFRAME_MATERIALIZATION_KIND: DerivedArtifactKind = DerivedArtifactKind {
    facet: FacetKind::Dataframe,
    artifact_family: DATAFRAME_MATERIALIZATION_ARTIFACT_PREFIX,
    format_version: DATAFRAME_MATERIALIZATION_FORMAT_VERSION,
};

pub const CALENDAR_DERIVED_INDEX_KIND: DerivedArtifactKind = DerivedArtifactKind {
    facet: FacetKind::Calendar,
    artifact_family: PIM_DERIVED_INDEX_ARTIFACT_PREFIX,
    format_version: CALENDAR_DERIVED_INDEX_FORMAT_VERSION,
};

pub const CONTACTS_DERIVED_INDEX_KIND: DerivedArtifactKind = DerivedArtifactKind {
    facet: FacetKind::Contacts,
    artifact_family: PIM_DERIVED_INDEX_ARTIFACT_PREFIX,
    format_version: CONTACTS_DERIVED_INDEX_FORMAT_VERSION,
};

pub const MAIL_DERIVED_INDEX_KIND: DerivedArtifactKind = DerivedArtifactKind {
    facet: FacetKind::Mail,
    artifact_family: PIM_DERIVED_INDEX_ARTIFACT_PREFIX,
    format_version: MAIL_DERIVED_INDEX_FORMAT_VERSION,
};

pub const VECTOR_PQ_KIND: DerivedArtifactKind = DerivedArtifactKind {
    facet: FacetKind::Vector,
    artifact_family: VECTOR_PQ_ARTIFACT,
    format_version: VECTOR_PQ_FORMAT_VERSION,
};

pub const VECTOR_HNSW_KIND: DerivedArtifactKind = DerivedArtifactKind {
    facet: FacetKind::Vector,
    artifact_family: VECTOR_HNSW_ARTIFACT,
    format_version: VECTOR_HNSW_FORMAT_VERSION,
};

pub const DERIVED_ARTIFACT_REGISTRY: &[DerivedArtifactKind] = &[
    SEARCH_TANTIVY_KIND,
    SEARCH_EMBEDDING_KIND,
    COLUMNAR_ARROW_KIND,
    GRAPH_PROPERTY_INDEX_KIND,
    GRAPH_SPATIAL_INDEX_KIND,
    DATAFRAME_MATERIALIZATION_KIND,
    CALENDAR_DERIVED_INDEX_KIND,
    CONTACTS_DERIVED_INDEX_KIND,
    MAIL_DERIVED_INDEX_KIND,
    VECTOR_PQ_KIND,
    VECTOR_HNSW_KIND,
];

pub fn derived_artifact_format_version(
    facet: FacetKind,
    artifact_family: &str,
) -> Option<&'static str> {
    DERIVED_ARTIFACT_REGISTRY
        .iter()
        .find(|kind| {
            kind.facet == facet && artifact_family_matches(kind.artifact_family, artifact_family)
        })
        .map(|kind| kind.format_version)
}

fn artifact_family_matches(registered: &str, candidate: &str) -> bool {
    registered == candidate || registered.ends_with(':') && candidate.starts_with(registered)
}

#[derive(Debug, Clone, Copy)]
pub struct SearchEmbeddingProjection<'a> {
    pub workspace: WorkspaceId,
    pub collection: &'a str,
    pub entity_id: &'a str,
    pub content_digest: Digest,
    pub model_id: &'a str,
    pub model_weights_digest: Option<&'a str>,
    pub engine_version: &'a str,
}

impl SearchEmbeddingProjection<'_> {
    fn key(&self) -> Result<DerivedArtifactKey> {
        search_embedding_artifact_key(self.workspace, self.collection, self.entity_id)
    }

    fn stamp(&self) -> Result<DerivedArtifactStamp> {
        search_embedding_artifact_stamp(
            self.content_digest,
            self.model_id,
            self.model_weights_digest,
            self.engine_version,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedArtifactKey {
    pub workspace: WorkspaceId,
    pub facet: FacetKind,
    pub collection: String,
    pub artifact: String,
}

impl DerivedArtifactKey {
    pub fn new(
        workspace: WorkspaceId,
        facet: FacetKind,
        collection: impl Into<String>,
        artifact: impl Into<String>,
    ) -> Result<Self> {
        let key = Self {
            workspace,
            facet,
            collection: collection.into(),
            artifact: artifact.into(),
        };
        validate_component("collection", &key.collection)?;
        validate_component("artifact", &key.artifact)?;
        Ok(key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedArtifactStamp {
    pub source_digest: Digest,
    pub engine_version: String,
    pub format_version: String,
}

impl DerivedArtifactStamp {
    pub fn new(
        source_digest: Digest,
        engine_version: impl Into<String>,
        format_version: impl Into<String>,
    ) -> Result<Self> {
        let stamp = Self {
            source_digest,
            engine_version: engine_version.into(),
            format_version: format_version.into(),
        };
        validate_component("engine_version", &stamp.engine_version)?;
        validate_component("format_version", &stamp.format_version)?;
        Ok(stamp)
    }
}

pub fn search_tantivy_artifact_key(
    workspace: WorkspaceId,
    collection: impl Into<String>,
) -> Result<DerivedArtifactKey> {
    DerivedArtifactKey::new(
        workspace,
        SEARCH_TANTIVY_KIND.facet,
        collection,
        SEARCH_TANTIVY_KIND.artifact_family,
    )
}

pub fn search_tantivy_artifact_stamp(
    source_digest: Digest,
    engine_version: impl Into<String>,
) -> Result<DerivedArtifactStamp> {
    DerivedArtifactStamp::new(
        source_digest,
        engine_version,
        SEARCH_TANTIVY_KIND.format_version,
    )
}

pub fn search_embedding_artifact_key(
    workspace: WorkspaceId,
    collection: impl Into<String>,
    entity_id: impl AsRef<str>,
) -> Result<DerivedArtifactKey> {
    let entity_id = entity_id.as_ref();
    validate_component("entity_id", entity_id)?;
    DerivedArtifactKey::new(
        workspace,
        SEARCH_EMBEDDING_KIND.facet,
        collection,
        format!("{}{entity_id}", SEARCH_EMBEDDING_KIND.artifact_family),
    )
}

pub fn search_embedding_artifact_stamp(
    content_digest: Digest,
    model_id: impl AsRef<str>,
    model_weights_digest: Option<&str>,
    engine_version: impl AsRef<str>,
) -> Result<DerivedArtifactStamp> {
    let model_id = model_id.as_ref();
    let engine_version = engine_version.as_ref();
    validate_component("model_id", model_id)?;
    if let Some(digest) = model_weights_digest {
        validate_component("model_weights_digest", digest)?;
    }
    validate_component("engine_version", engine_version)?;
    let mut version = String::new();
    push_component(&mut version, model_id);
    push_component(&mut version, model_weights_digest.unwrap_or(""));
    push_component(&mut version, engine_version);
    DerivedArtifactStamp::new(
        content_digest,
        version,
        SEARCH_EMBEDDING_KIND.format_version,
    )
}

pub fn columnar_arrow_artifact_key(
    workspace: WorkspaceId,
    collection: impl Into<String>,
) -> Result<DerivedArtifactKey> {
    DerivedArtifactKey::new(
        workspace,
        COLUMNAR_ARROW_KIND.facet,
        collection,
        COLUMNAR_ARROW_KIND.artifact_family,
    )
}

pub fn columnar_arrow_artifact_stamp(
    source_digest: Digest,
    engine_version: impl Into<String>,
) -> Result<DerivedArtifactStamp> {
    DerivedArtifactStamp::new(
        source_digest,
        engine_version,
        COLUMNAR_ARROW_KIND.format_version,
    )
}

pub fn graph_property_index_artifact_key(
    workspace: WorkspaceId,
    graph_name: impl Into<String>,
    index_name: impl AsRef<str>,
) -> Result<DerivedArtifactKey> {
    let index_name = index_name.as_ref();
    validate_component("index_name", index_name)?;
    DerivedArtifactKey::new(
        workspace,
        GRAPH_PROPERTY_INDEX_KIND.facet,
        graph_name,
        format!("{}{index_name}", GRAPH_PROPERTY_INDEX_KIND.artifact_family),
    )
}

pub fn graph_property_index_artifact_stamp(
    source_digest: Digest,
    engine_version: impl Into<String>,
) -> Result<DerivedArtifactStamp> {
    DerivedArtifactStamp::new(
        source_digest,
        engine_version,
        GRAPH_PROPERTY_INDEX_KIND.format_version,
    )
}

pub fn graph_spatial_index_artifact_key(
    workspace: WorkspaceId,
    graph_name: impl Into<String>,
    index_name: impl AsRef<str>,
) -> Result<DerivedArtifactKey> {
    let index_name = index_name.as_ref();
    validate_component("index_name", index_name)?;
    DerivedArtifactKey::new(
        workspace,
        GRAPH_SPATIAL_INDEX_KIND.facet,
        graph_name,
        format!("{}{index_name}", GRAPH_SPATIAL_INDEX_KIND.artifact_family),
    )
}

pub fn graph_spatial_index_artifact_stamp(
    source_digest: Digest,
    engine_version: impl Into<String>,
) -> Result<DerivedArtifactStamp> {
    DerivedArtifactStamp::new(
        source_digest,
        engine_version,
        GRAPH_SPATIAL_INDEX_KIND.format_version,
    )
}

pub fn dataframe_materialization_artifact_key(
    workspace: WorkspaceId,
    frame_name: impl Into<String>,
    materialization_id: impl AsRef<str>,
) -> Result<DerivedArtifactKey> {
    let materialization_id = materialization_id.as_ref();
    validate_component("materialization_id", materialization_id)?;
    DerivedArtifactKey::new(
        workspace,
        DATAFRAME_MATERIALIZATION_KIND.facet,
        frame_name,
        format!(
            "{}{materialization_id}",
            DATAFRAME_MATERIALIZATION_KIND.artifact_family
        ),
    )
}

pub fn dataframe_materialization_artifact_stamp(
    source_digest: Digest,
    engine_version: impl Into<String>,
) -> Result<DerivedArtifactStamp> {
    DerivedArtifactStamp::new(
        source_digest,
        engine_version,
        DATAFRAME_MATERIALIZATION_KIND.format_version,
    )
}

pub fn calendar_derived_index_artifact_key(
    workspace: WorkspaceId,
    principal: impl AsRef<str>,
    collection: impl AsRef<str>,
    index_name: impl AsRef<str>,
) -> Result<DerivedArtifactKey> {
    pim_derived_index_artifact_key(
        workspace,
        CALENDAR_DERIVED_INDEX_KIND,
        principal,
        collection,
        index_name,
    )
}

pub fn calendar_derived_index_artifact_stamp(
    source_digest: Digest,
    engine_version: impl Into<String>,
) -> Result<DerivedArtifactStamp> {
    DerivedArtifactStamp::new(
        source_digest,
        engine_version,
        CALENDAR_DERIVED_INDEX_KIND.format_version,
    )
}

pub fn contacts_derived_index_artifact_key(
    workspace: WorkspaceId,
    principal: impl AsRef<str>,
    book: impl AsRef<str>,
    index_name: impl AsRef<str>,
) -> Result<DerivedArtifactKey> {
    pim_derived_index_artifact_key(
        workspace,
        CONTACTS_DERIVED_INDEX_KIND,
        principal,
        book,
        index_name,
    )
}

pub fn contacts_derived_index_artifact_stamp(
    source_digest: Digest,
    engine_version: impl Into<String>,
) -> Result<DerivedArtifactStamp> {
    DerivedArtifactStamp::new(
        source_digest,
        engine_version,
        CONTACTS_DERIVED_INDEX_KIND.format_version,
    )
}

pub fn mail_derived_index_artifact_key(
    workspace: WorkspaceId,
    principal: impl AsRef<str>,
    mailbox: impl AsRef<str>,
    index_name: impl AsRef<str>,
) -> Result<DerivedArtifactKey> {
    pim_derived_index_artifact_key(
        workspace,
        MAIL_DERIVED_INDEX_KIND,
        principal,
        mailbox,
        index_name,
    )
}

pub fn mail_derived_index_artifact_stamp(
    source_digest: Digest,
    engine_version: impl Into<String>,
) -> Result<DerivedArtifactStamp> {
    DerivedArtifactStamp::new(
        source_digest,
        engine_version,
        MAIL_DERIVED_INDEX_KIND.format_version,
    )
}

fn pim_derived_index_artifact_key(
    workspace: WorkspaceId,
    kind: DerivedArtifactKind,
    principal: impl AsRef<str>,
    collection: impl AsRef<str>,
    index_name: impl AsRef<str>,
) -> Result<DerivedArtifactKey> {
    let principal = principal.as_ref();
    let collection = collection.as_ref();
    let index_name = index_name.as_ref();
    validate_component("principal", principal)?;
    validate_component("collection", collection)?;
    validate_component("index_name", index_name)?;
    DerivedArtifactKey::new(
        workspace,
        kind.facet,
        format!("{principal}/{collection}"),
        format!("{}{index_name}", kind.artifact_family),
    )
}

pub fn vector_pq_artifact_key(
    workspace: WorkspaceId,
    set_name: impl Into<String>,
) -> Result<DerivedArtifactKey> {
    DerivedArtifactKey::new(
        workspace,
        VECTOR_PQ_KIND.facet,
        set_name,
        VECTOR_PQ_KIND.artifact_family,
    )
}

pub fn vector_pq_artifact_stamp(
    source_digest: Digest,
    engine_version: impl Into<String>,
) -> Result<DerivedArtifactStamp> {
    DerivedArtifactStamp::new(source_digest, engine_version, VECTOR_PQ_KIND.format_version)
}

pub fn vector_hnsw_artifact_key(
    workspace: WorkspaceId,
    set_name: impl Into<String>,
) -> Result<DerivedArtifactKey> {
    DerivedArtifactKey::new(
        workspace,
        VECTOR_HNSW_KIND.facet,
        set_name,
        VECTOR_HNSW_KIND.artifact_family,
    )
}

pub fn vector_hnsw_artifact_stamp(
    source_digest: Digest,
    engine_version: impl Into<String>,
) -> Result<DerivedArtifactStamp> {
    DerivedArtifactStamp::new(
        source_digest,
        engine_version,
        VECTOR_HNSW_KIND.format_version,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedArtifactRecord {
    pub stamp: DerivedArtifactStamp,
    pub payload_digest: Digest,
    pub payload_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DerivedArtifactRead {
    Missing,
    Stale {
        record: DerivedArtifactRecord,
    },
    Ready {
        record: DerivedArtifactRecord,
        payload: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DerivedArtifactStatus {
    Missing,
    Stale {
        record: DerivedArtifactRecord,
    },
    Ready {
        record: DerivedArtifactRecord,
    },
    Rebuilding {
        run_id: String,
        stamp: DerivedArtifactStamp,
    },
    Failed {
        stamp: DerivedArtifactStamp,
        message: String,
    },
    Unsupported {
        stamp: DerivedArtifactStamp,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivedArtifactServingMode {
    DerivedArtifact,
    AuthoritativeSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivedArtifactServingPolicy {
    pub mode: DerivedArtifactServingMode,
    pub operational_state: CapabilityOperationalState,
    pub reason_code: Option<&'static str>,
    pub stable_error: Option<Code>,
    pub degradation: Option<CapabilityDegradation>,
}

impl DerivedArtifactStatus {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Stale { .. } => "stale",
            Self::Ready { .. } => "ready",
            Self::Rebuilding { .. } => "rebuilding",
            Self::Failed { .. } => "failed",
            Self::Unsupported { .. } => "unsupported",
        }
    }

    pub fn serving_policy(&self) -> DerivedArtifactServingPolicy {
        match self {
            Self::Ready { .. } => DerivedArtifactServingPolicy {
                mode: DerivedArtifactServingMode::DerivedArtifact,
                operational_state: CapabilityOperationalState::Supported,
                reason_code: None,
                stable_error: None,
                degradation: None,
            },
            Self::Missing => DerivedArtifactServingPolicy {
                mode: DerivedArtifactServingMode::AuthoritativeSource,
                operational_state: CapabilityOperationalState::Degraded,
                reason_code: Some("derived_artifact_missing"),
                stable_error: None,
                degradation: Some(authoritative_source_degradation()),
            },
            Self::Stale { .. } => DerivedArtifactServingPolicy {
                mode: DerivedArtifactServingMode::AuthoritativeSource,
                operational_state: CapabilityOperationalState::Degraded,
                reason_code: Some("derived_artifact_stale"),
                stable_error: None,
                degradation: Some(authoritative_source_degradation()),
            },
            Self::Rebuilding { .. } => DerivedArtifactServingPolicy {
                mode: DerivedArtifactServingMode::AuthoritativeSource,
                operational_state: CapabilityOperationalState::Degraded,
                reason_code: Some("index_rebuilding"),
                stable_error: None,
                degradation: Some(authoritative_source_degradation()),
            },
            Self::Failed { .. } => DerivedArtifactServingPolicy {
                mode: DerivedArtifactServingMode::AuthoritativeSource,
                operational_state: CapabilityOperationalState::Degraded,
                reason_code: Some("derived_artifact_failed"),
                stable_error: None,
                degradation: Some(authoritative_source_degradation()),
            },
            Self::Unsupported { .. } => DerivedArtifactServingPolicy {
                mode: DerivedArtifactServingMode::AuthoritativeSource,
                operational_state: CapabilityOperationalState::Unsupported,
                reason_code: Some("profile_unsupported"),
                stable_error: Some(Code::Unsupported),
                degradation: None,
            },
        }
    }

    pub fn apply_serving_policy_to_capabilities(
        &self,
        set: CapabilitySet,
        capability_name: &str,
    ) -> CapabilitySet {
        let policy = self.serving_policy();
        if policy.operational_state == CapabilityOperationalState::Degraded {
            let degradation = policy
                .degradation
                .expect("degraded serving policy carries a boundary");
            return set.with_degraded_detail(
                capability_name,
                policy
                    .reason_code
                    .expect("degraded serving policy has a reason"),
                degradation,
            );
        }
        set.with_state_detail(
            capability_name,
            policy.operational_state,
            policy.reason_code,
            policy.stable_error,
        )
    }
}

const fn authoritative_source_degradation() -> CapabilityDegradation {
    CapabilityDegradation {
        fallback: "authoritative-source",
        result_equivalence: "source-equivalent",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DerivedArtifactRebuild {
    AlreadyReady { record: DerivedArtifactRecord },
    Started { run_id: String },
    Coalesced { run_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RebuildState {
    Rebuilding,
    Failed,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RebuildRecord {
    stamp: DerivedArtifactStamp,
    run_id: String,
    state: RebuildState,
    message: String,
}

impl FileStore {
    pub fn put_derived_artifact(
        &self,
        key: &DerivedArtifactKey,
        stamp: DerivedArtifactStamp,
        payload: &[u8],
    ) -> Result<DerivedArtifactRecord> {
        let payload_digest = self.put_hint(payload, CompressionHint::Small)?;
        let record = DerivedArtifactRecord {
            stamp,
            payload_digest,
            payload_len: payload.len() as u64,
        };
        self.control_set(&derived_control_key(key)?, encode_record(&record)?)?;
        Ok(record)
    }

    pub fn derived_artifact_record(
        &self,
        key: &DerivedArtifactKey,
    ) -> Result<Option<DerivedArtifactRecord>> {
        self.control_get(&derived_control_key(key)?)?
            .map(|bytes| decode_record(&bytes, self.digest_algo))
            .transpose()
    }

    pub fn read_derived_artifact(
        &self,
        key: &DerivedArtifactKey,
        expected: &DerivedArtifactStamp,
    ) -> Result<DerivedArtifactRead> {
        let Some(record) = self.derived_artifact_record(key)? else {
            return Ok(DerivedArtifactRead::Missing);
        };
        if &record.stamp != expected {
            return Ok(DerivedArtifactRead::Stale { record });
        }
        let payload = self
            .get(&record.payload_digest)?
            .ok_or_else(|| corrupt("derived artifact payload object missing"))?;
        if payload.len() as u64 != record.payload_len {
            return Err(corrupt("derived artifact payload length mismatch"));
        }
        Ok(DerivedArtifactRead::Ready { record, payload })
    }

    pub fn delete_derived_artifact(&self, key: &DerivedArtifactKey) -> Result<bool> {
        self.control_delete(&derived_control_key(key)?)
    }

    pub fn derived_artifact_status(
        &self,
        key: &DerivedArtifactKey,
        expected: &DerivedArtifactStamp,
    ) -> Result<DerivedArtifactStatus> {
        match self.read_derived_artifact(key, expected)? {
            DerivedArtifactRead::Ready { record, .. } => {
                return Ok(DerivedArtifactStatus::Ready { record });
            }
            DerivedArtifactRead::Stale { record } => {
                if let Some(rebuild) = self.derived_rebuild_record(key)?
                    && rebuild.stamp == *expected
                {
                    return Ok(rebuild_status(rebuild));
                }
                return Ok(DerivedArtifactStatus::Stale { record });
            }
            DerivedArtifactRead::Missing => {}
        }
        if let Some(rebuild) = self.derived_rebuild_record(key)?
            && rebuild.stamp == *expected
        {
            return Ok(rebuild_status(rebuild));
        }
        Ok(DerivedArtifactStatus::Missing)
    }

    pub fn begin_derived_artifact_rebuild(
        &self,
        key: &DerivedArtifactKey,
        stamp: DerivedArtifactStamp,
    ) -> Result<DerivedArtifactRebuild> {
        if let DerivedArtifactRead::Ready { record, .. } =
            self.read_derived_artifact(key, &stamp)?
        {
            return Ok(DerivedArtifactRebuild::AlreadyReady { record });
        }
        if let Some(rebuild) = self.derived_rebuild_record(key)?
            && rebuild.stamp == stamp
            && rebuild.state == RebuildState::Rebuilding
        {
            return Ok(DerivedArtifactRebuild::Coalesced {
                run_id: rebuild.run_id,
            });
        }
        let run_id = derived_run_id(key, &stamp, self.digest_algo)?;
        let rebuild = RebuildRecord {
            stamp,
            run_id: run_id.clone(),
            state: RebuildState::Rebuilding,
            message: String::new(),
        };
        self.control_set(&derived_rebuild_key(key)?, encode_rebuild_record(&rebuild)?)?;
        Ok(DerivedArtifactRebuild::Started { run_id })
    }

    pub fn finish_derived_artifact_rebuild(
        &self,
        key: &DerivedArtifactKey,
        run_id: &str,
        stamp: DerivedArtifactStamp,
        payload: &[u8],
    ) -> Result<DerivedArtifactRecord> {
        self.require_active_rebuild(key, run_id, &stamp)?;
        let payload_digest = self.put_hint(payload, CompressionHint::Small)?;
        let record = DerivedArtifactRecord {
            stamp,
            payload_digest,
            payload_len: payload.len() as u64,
        };
        let mut map = self.control_map()?;
        map.insert(derived_control_key(key)?, encode_record(&record)?);
        map.remove(&derived_rebuild_key(key)?);
        self.write_control_map(map)?;
        Ok(record)
    }

    pub fn fail_derived_artifact_rebuild(
        &self,
        key: &DerivedArtifactKey,
        run_id: &str,
        stamp: DerivedArtifactStamp,
        message: impl Into<String>,
    ) -> Result<()> {
        self.require_active_rebuild(key, run_id, &stamp)?;
        let rebuild = RebuildRecord {
            stamp,
            run_id: run_id.to_string(),
            state: RebuildState::Failed,
            message: message.into(),
        };
        validate_component("message", &rebuild.message)?;
        self.control_set(&derived_rebuild_key(key)?, encode_rebuild_record(&rebuild)?)
    }

    pub fn mark_derived_artifact_unsupported(
        &self,
        key: &DerivedArtifactKey,
        stamp: DerivedArtifactStamp,
        message: impl Into<String>,
    ) -> Result<()> {
        let run_id = derived_run_id(key, &stamp, self.digest_algo)?;
        let rebuild = RebuildRecord {
            stamp,
            run_id,
            state: RebuildState::Unsupported,
            message: message.into(),
        };
        validate_component("message", &rebuild.message)?;
        self.control_set(&derived_rebuild_key(key)?, encode_rebuild_record(&rebuild)?)
    }

    pub fn search_tantivy_status(
        &self,
        workspace: WorkspaceId,
        collection: &str,
        source_digest: Digest,
        engine_version: &str,
    ) -> Result<DerivedArtifactStatus> {
        let key = search_tantivy_artifact_key(workspace, collection)?;
        let stamp = search_tantivy_artifact_stamp(source_digest, engine_version)?;
        self.derived_artifact_status(&key, &stamp)
    }

    pub fn begin_search_tantivy_rebuild(
        &self,
        workspace: WorkspaceId,
        collection: &str,
        source_digest: Digest,
        engine_version: &str,
    ) -> Result<DerivedArtifactRebuild> {
        let key = search_tantivy_artifact_key(workspace, collection)?;
        let stamp = search_tantivy_artifact_stamp(source_digest, engine_version)?;
        self.begin_derived_artifact_rebuild(&key, stamp)
    }

    pub fn finish_search_tantivy_rebuild(
        &self,
        workspace: WorkspaceId,
        collection: &str,
        run_id: &str,
        source_digest: Digest,
        engine_version: &str,
        payload: &[u8],
    ) -> Result<DerivedArtifactRecord> {
        let key = search_tantivy_artifact_key(workspace, collection)?;
        let stamp = search_tantivy_artifact_stamp(source_digest, engine_version)?;
        self.finish_derived_artifact_rebuild(&key, run_id, stamp, payload)
    }

    pub fn fail_search_tantivy_rebuild(
        &self,
        workspace: WorkspaceId,
        collection: &str,
        run_id: &str,
        source_digest: Digest,
        engine_version: &str,
        message: impl Into<String>,
    ) -> Result<()> {
        let key = search_tantivy_artifact_key(workspace, collection)?;
        let stamp = search_tantivy_artifact_stamp(source_digest, engine_version)?;
        self.fail_derived_artifact_rebuild(&key, run_id, stamp, message)
    }

    pub fn mark_search_tantivy_unsupported(
        &self,
        workspace: WorkspaceId,
        collection: &str,
        source_digest: Digest,
        engine_version: &str,
        message: impl Into<String>,
    ) -> Result<()> {
        let key = search_tantivy_artifact_key(workspace, collection)?;
        let stamp = search_tantivy_artifact_stamp(source_digest, engine_version)?;
        self.mark_derived_artifact_unsupported(&key, stamp, message)
    }

    pub fn columnar_arrow_status(
        &self,
        workspace: WorkspaceId,
        collection: &str,
        source_digest: Digest,
        engine_version: &str,
    ) -> Result<DerivedArtifactStatus> {
        let key = columnar_arrow_artifact_key(workspace, collection)?;
        let stamp = columnar_arrow_artifact_stamp(source_digest, engine_version)?;
        self.derived_artifact_status(&key, &stamp)
    }

    pub fn begin_columnar_arrow_rebuild(
        &self,
        workspace: WorkspaceId,
        collection: &str,
        source_digest: Digest,
        engine_version: &str,
    ) -> Result<DerivedArtifactRebuild> {
        let key = columnar_arrow_artifact_key(workspace, collection)?;
        let stamp = columnar_arrow_artifact_stamp(source_digest, engine_version)?;
        self.begin_derived_artifact_rebuild(&key, stamp)
    }

    pub fn finish_columnar_arrow_rebuild(
        &self,
        workspace: WorkspaceId,
        collection: &str,
        run_id: &str,
        source_digest: Digest,
        engine_version: &str,
        payload: &[u8],
    ) -> Result<DerivedArtifactRecord> {
        let key = columnar_arrow_artifact_key(workspace, collection)?;
        let stamp = columnar_arrow_artifact_stamp(source_digest, engine_version)?;
        self.finish_derived_artifact_rebuild(&key, run_id, stamp, payload)
    }

    pub fn fail_columnar_arrow_rebuild(
        &self,
        workspace: WorkspaceId,
        collection: &str,
        run_id: &str,
        source_digest: Digest,
        engine_version: &str,
        message: impl Into<String>,
    ) -> Result<()> {
        let key = columnar_arrow_artifact_key(workspace, collection)?;
        let stamp = columnar_arrow_artifact_stamp(source_digest, engine_version)?;
        self.fail_derived_artifact_rebuild(&key, run_id, stamp, message)
    }

    pub fn mark_columnar_arrow_unsupported(
        &self,
        workspace: WorkspaceId,
        collection: &str,
        source_digest: Digest,
        engine_version: &str,
        message: impl Into<String>,
    ) -> Result<()> {
        let key = columnar_arrow_artifact_key(workspace, collection)?;
        let stamp = columnar_arrow_artifact_stamp(source_digest, engine_version)?;
        self.mark_derived_artifact_unsupported(&key, stamp, message)
    }

    pub fn graph_property_index_status(
        &self,
        workspace: WorkspaceId,
        graph_name: &str,
        index_name: &str,
        source_digest: Digest,
        engine_version: &str,
    ) -> Result<DerivedArtifactStatus> {
        let key = graph_property_index_artifact_key(workspace, graph_name, index_name)?;
        let stamp = graph_property_index_artifact_stamp(source_digest, engine_version)?;
        self.derived_artifact_status(&key, &stamp)
    }

    pub fn begin_graph_property_index_rebuild(
        &self,
        workspace: WorkspaceId,
        graph_name: &str,
        index_name: &str,
        source_digest: Digest,
        engine_version: &str,
    ) -> Result<DerivedArtifactRebuild> {
        let key = graph_property_index_artifact_key(workspace, graph_name, index_name)?;
        let stamp = graph_property_index_artifact_stamp(source_digest, engine_version)?;
        self.begin_derived_artifact_rebuild(&key, stamp)
    }

    pub fn finish_graph_property_index_rebuild(
        &self,
        workspace: WorkspaceId,
        graph_name: &str,
        index_name: &str,
        run_id: &str,
        source_digest: Digest,
        engine_version: &str,
        payload: &[u8],
    ) -> Result<DerivedArtifactRecord> {
        let key = graph_property_index_artifact_key(workspace, graph_name, index_name)?;
        let stamp = graph_property_index_artifact_stamp(source_digest, engine_version)?;
        self.finish_derived_artifact_rebuild(&key, run_id, stamp, payload)
    }

    pub fn fail_graph_property_index_rebuild(
        &self,
        workspace: WorkspaceId,
        graph_name: &str,
        index_name: &str,
        run_id: &str,
        source_digest: Digest,
        engine_version: &str,
        message: impl Into<String>,
    ) -> Result<()> {
        let key = graph_property_index_artifact_key(workspace, graph_name, index_name)?;
        let stamp = graph_property_index_artifact_stamp(source_digest, engine_version)?;
        self.fail_derived_artifact_rebuild(&key, run_id, stamp, message)
    }

    pub fn mark_graph_property_index_unsupported(
        &self,
        workspace: WorkspaceId,
        graph_name: &str,
        index_name: &str,
        source_digest: Digest,
        engine_version: &str,
        message: impl Into<String>,
    ) -> Result<()> {
        let key = graph_property_index_artifact_key(workspace, graph_name, index_name)?;
        let stamp = graph_property_index_artifact_stamp(source_digest, engine_version)?;
        self.mark_derived_artifact_unsupported(&key, stamp, message)
    }

    pub fn graph_spatial_index_status(
        &self,
        workspace: WorkspaceId,
        graph_name: &str,
        index_name: &str,
        source_digest: Digest,
        engine_version: &str,
    ) -> Result<DerivedArtifactStatus> {
        let key = graph_spatial_index_artifact_key(workspace, graph_name, index_name)?;
        let stamp = graph_spatial_index_artifact_stamp(source_digest, engine_version)?;
        self.derived_artifact_status(&key, &stamp)
    }

    pub fn begin_graph_spatial_index_rebuild(
        &self,
        workspace: WorkspaceId,
        graph_name: &str,
        index_name: &str,
        source_digest: Digest,
        engine_version: &str,
    ) -> Result<DerivedArtifactRebuild> {
        let key = graph_spatial_index_artifact_key(workspace, graph_name, index_name)?;
        let stamp = graph_spatial_index_artifact_stamp(source_digest, engine_version)?;
        self.begin_derived_artifact_rebuild(&key, stamp)
    }

    pub fn finish_graph_spatial_index_rebuild(
        &self,
        workspace: WorkspaceId,
        graph_name: &str,
        index_name: &str,
        run_id: &str,
        source_digest: Digest,
        engine_version: &str,
        payload: &[u8],
    ) -> Result<DerivedArtifactRecord> {
        let key = graph_spatial_index_artifact_key(workspace, graph_name, index_name)?;
        let stamp = graph_spatial_index_artifact_stamp(source_digest, engine_version)?;
        self.finish_derived_artifact_rebuild(&key, run_id, stamp, payload)
    }

    pub fn fail_graph_spatial_index_rebuild(
        &self,
        workspace: WorkspaceId,
        graph_name: &str,
        index_name: &str,
        run_id: &str,
        source_digest: Digest,
        engine_version: &str,
        message: impl Into<String>,
    ) -> Result<()> {
        let key = graph_spatial_index_artifact_key(workspace, graph_name, index_name)?;
        let stamp = graph_spatial_index_artifact_stamp(source_digest, engine_version)?;
        self.fail_derived_artifact_rebuild(&key, run_id, stamp, message)
    }

    pub fn mark_graph_spatial_index_unsupported(
        &self,
        workspace: WorkspaceId,
        graph_name: &str,
        index_name: &str,
        source_digest: Digest,
        engine_version: &str,
        message: impl Into<String>,
    ) -> Result<()> {
        let key = graph_spatial_index_artifact_key(workspace, graph_name, index_name)?;
        let stamp = graph_spatial_index_artifact_stamp(source_digest, engine_version)?;
        self.mark_derived_artifact_unsupported(&key, stamp, message)
    }

    pub fn dataframe_materialization_status(
        &self,
        workspace: WorkspaceId,
        frame_name: &str,
        materialization_id: &str,
        source_digest: Digest,
        engine_version: &str,
    ) -> Result<DerivedArtifactStatus> {
        let key =
            dataframe_materialization_artifact_key(workspace, frame_name, materialization_id)?;
        let stamp = dataframe_materialization_artifact_stamp(source_digest, engine_version)?;
        self.derived_artifact_status(&key, &stamp)
    }

    pub fn begin_dataframe_materialization_rebuild(
        &self,
        workspace: WorkspaceId,
        frame_name: &str,
        materialization_id: &str,
        source_digest: Digest,
        engine_version: &str,
    ) -> Result<DerivedArtifactRebuild> {
        let key =
            dataframe_materialization_artifact_key(workspace, frame_name, materialization_id)?;
        let stamp = dataframe_materialization_artifact_stamp(source_digest, engine_version)?;
        self.begin_derived_artifact_rebuild(&key, stamp)
    }

    pub fn finish_dataframe_materialization_rebuild(
        &self,
        workspace: WorkspaceId,
        frame_name: &str,
        materialization_id: &str,
        run_id: &str,
        source_digest: Digest,
        engine_version: &str,
        payload: &[u8],
    ) -> Result<DerivedArtifactRecord> {
        let key =
            dataframe_materialization_artifact_key(workspace, frame_name, materialization_id)?;
        let stamp = dataframe_materialization_artifact_stamp(source_digest, engine_version)?;
        self.finish_derived_artifact_rebuild(&key, run_id, stamp, payload)
    }

    pub fn fail_dataframe_materialization_rebuild(
        &self,
        workspace: WorkspaceId,
        frame_name: &str,
        materialization_id: &str,
        run_id: &str,
        source_digest: Digest,
        engine_version: &str,
        message: impl Into<String>,
    ) -> Result<()> {
        let key =
            dataframe_materialization_artifact_key(workspace, frame_name, materialization_id)?;
        let stamp = dataframe_materialization_artifact_stamp(source_digest, engine_version)?;
        self.fail_derived_artifact_rebuild(&key, run_id, stamp, message)
    }

    pub fn mark_dataframe_materialization_unsupported(
        &self,
        workspace: WorkspaceId,
        frame_name: &str,
        materialization_id: &str,
        source_digest: Digest,
        engine_version: &str,
        message: impl Into<String>,
    ) -> Result<()> {
        let key =
            dataframe_materialization_artifact_key(workspace, frame_name, materialization_id)?;
        let stamp = dataframe_materialization_artifact_stamp(source_digest, engine_version)?;
        self.mark_derived_artifact_unsupported(&key, stamp, message)
    }

    pub fn vector_pq_status(
        &self,
        workspace: WorkspaceId,
        set_name: &str,
        source_digest: Digest,
        engine_version: &str,
    ) -> Result<DerivedArtifactStatus> {
        let key = vector_pq_artifact_key(workspace, set_name)?;
        let stamp = vector_pq_artifact_stamp(source_digest, engine_version)?;
        self.derived_artifact_status(&key, &stamp)
    }

    pub fn begin_vector_pq_rebuild(
        &self,
        workspace: WorkspaceId,
        set_name: &str,
        source_digest: Digest,
        engine_version: &str,
    ) -> Result<DerivedArtifactRebuild> {
        let key = vector_pq_artifact_key(workspace, set_name)?;
        let stamp = vector_pq_artifact_stamp(source_digest, engine_version)?;
        self.begin_derived_artifact_rebuild(&key, stamp)
    }

    pub fn finish_vector_pq_rebuild(
        &self,
        workspace: WorkspaceId,
        set_name: &str,
        run_id: &str,
        source_digest: Digest,
        engine_version: &str,
        payload: &[u8],
    ) -> Result<DerivedArtifactRecord> {
        let key = vector_pq_artifact_key(workspace, set_name)?;
        let stamp = vector_pq_artifact_stamp(source_digest, engine_version)?;
        self.finish_derived_artifact_rebuild(&key, run_id, stamp, payload)
    }

    pub fn fail_vector_pq_rebuild(
        &self,
        workspace: WorkspaceId,
        set_name: &str,
        run_id: &str,
        source_digest: Digest,
        engine_version: &str,
        message: impl Into<String>,
    ) -> Result<()> {
        let key = vector_pq_artifact_key(workspace, set_name)?;
        let stamp = vector_pq_artifact_stamp(source_digest, engine_version)?;
        self.fail_derived_artifact_rebuild(&key, run_id, stamp, message)
    }

    pub fn mark_vector_pq_unsupported(
        &self,
        workspace: WorkspaceId,
        set_name: &str,
        source_digest: Digest,
        engine_version: &str,
        message: impl Into<String>,
    ) -> Result<()> {
        let key = vector_pq_artifact_key(workspace, set_name)?;
        let stamp = vector_pq_artifact_stamp(source_digest, engine_version)?;
        self.mark_derived_artifact_unsupported(&key, stamp, message)
    }

    pub fn vector_hnsw_status(
        &self,
        workspace: WorkspaceId,
        set_name: &str,
        source_digest: Digest,
        engine_version: &str,
    ) -> Result<DerivedArtifactStatus> {
        let key = vector_hnsw_artifact_key(workspace, set_name)?;
        let stamp = vector_hnsw_artifact_stamp(source_digest, engine_version)?;
        self.derived_artifact_status(&key, &stamp)
    }

    pub fn begin_vector_hnsw_rebuild(
        &self,
        workspace: WorkspaceId,
        set_name: &str,
        source_digest: Digest,
        engine_version: &str,
    ) -> Result<DerivedArtifactRebuild> {
        let key = vector_hnsw_artifact_key(workspace, set_name)?;
        let stamp = vector_hnsw_artifact_stamp(source_digest, engine_version)?;
        self.begin_derived_artifact_rebuild(&key, stamp)
    }

    pub fn finish_vector_hnsw_rebuild(
        &self,
        workspace: WorkspaceId,
        set_name: &str,
        run_id: &str,
        source_digest: Digest,
        engine_version: &str,
        payload: &[u8],
    ) -> Result<DerivedArtifactRecord> {
        let key = vector_hnsw_artifact_key(workspace, set_name)?;
        let stamp = vector_hnsw_artifact_stamp(source_digest, engine_version)?;
        self.finish_derived_artifact_rebuild(&key, run_id, stamp, payload)
    }

    pub fn fail_vector_hnsw_rebuild(
        &self,
        workspace: WorkspaceId,
        set_name: &str,
        run_id: &str,
        source_digest: Digest,
        engine_version: &str,
        message: impl Into<String>,
    ) -> Result<()> {
        let key = vector_hnsw_artifact_key(workspace, set_name)?;
        let stamp = vector_hnsw_artifact_stamp(source_digest, engine_version)?;
        self.fail_derived_artifact_rebuild(&key, run_id, stamp, message)
    }

    pub fn mark_vector_hnsw_unsupported(
        &self,
        workspace: WorkspaceId,
        set_name: &str,
        source_digest: Digest,
        engine_version: &str,
        message: impl Into<String>,
    ) -> Result<()> {
        let key = vector_hnsw_artifact_key(workspace, set_name)?;
        let stamp = vector_hnsw_artifact_stamp(source_digest, engine_version)?;
        self.mark_derived_artifact_unsupported(&key, stamp, message)
    }

    pub fn search_embedding_status(
        &self,
        projection: SearchEmbeddingProjection<'_>,
    ) -> Result<DerivedArtifactStatus> {
        let key = projection.key()?;
        let stamp = projection.stamp()?;
        self.derived_artifact_status(&key, &stamp)
    }

    pub fn begin_search_embedding_rebuild(
        &self,
        projection: SearchEmbeddingProjection<'_>,
    ) -> Result<DerivedArtifactRebuild> {
        let key = projection.key()?;
        let stamp = projection.stamp()?;
        self.begin_derived_artifact_rebuild(&key, stamp)
    }

    pub fn finish_search_embedding_rebuild(
        &self,
        projection: SearchEmbeddingProjection<'_>,
        run_id: &str,
        payload: &[u8],
    ) -> Result<DerivedArtifactRecord> {
        let key = projection.key()?;
        let stamp = projection.stamp()?;
        self.finish_derived_artifact_rebuild(&key, run_id, stamp, payload)
    }

    pub fn fail_search_embedding_rebuild(
        &self,
        projection: SearchEmbeddingProjection<'_>,
        run_id: &str,
        message: impl Into<String>,
    ) -> Result<()> {
        let key = projection.key()?;
        let stamp = projection.stamp()?;
        self.fail_derived_artifact_rebuild(&key, run_id, stamp, message)
    }

    pub fn mark_search_embedding_no_keys(
        &self,
        projection: SearchEmbeddingProjection<'_>,
        message: impl Into<String>,
    ) -> Result<()> {
        let key = projection.key()?;
        let stamp = projection.stamp()?;
        self.mark_derived_artifact_unsupported(&key, stamp, message)
    }

    pub(crate) fn derived_payload_digests(&self) -> Result<BTreeSet<[u8; 32]>> {
        self.derived_payload_digests_from_control_map(&self.control_map()?)
    }

    pub(crate) fn derived_payload_digests_from_control_map(
        &self,
        map: &std::collections::BTreeMap<Vec<u8>, Vec<u8>>,
    ) -> Result<BTreeSet<[u8; 32]>> {
        let mut out = BTreeSet::new();
        for value in map
            .iter()
            .filter(|(key, _)| key.starts_with(DERIVED_PREFIX))
            .map(|(_, value)| value)
        {
            let record = decode_record(value, self.digest_algo)?;
            out.insert(*record.payload_digest.bytes());
        }
        Ok(out)
    }

    fn derived_rebuild_record(&self, key: &DerivedArtifactKey) -> Result<Option<RebuildRecord>> {
        self.control_get(&derived_rebuild_key(key)?)?
            .map(|bytes| decode_rebuild_record(&bytes))
            .transpose()
    }

    fn require_active_rebuild(
        &self,
        key: &DerivedArtifactKey,
        run_id: &str,
        stamp: &DerivedArtifactStamp,
    ) -> Result<()> {
        let Some(rebuild) = self.derived_rebuild_record(key)? else {
            return Err(LoomError::new(
                Code::Conflict,
                "derived artifact rebuild is not active",
            ));
        };
        if rebuild.state != RebuildState::Rebuilding
            || rebuild.run_id != run_id
            || &rebuild.stamp != stamp
        {
            return Err(LoomError::new(
                Code::Conflict,
                "derived artifact rebuild token is stale",
            ));
        }
        Ok(())
    }
}

fn validate_component(name: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(LoomError::invalid(format!(
            "derived artifact {name} is empty"
        )));
    }
    if value.len() > MAX_FIELD_LEN {
        return Err(LoomError::invalid(format!(
            "derived artifact {name} is too long"
        )));
    }
    Ok(())
}

fn derived_control_key(key: &DerivedArtifactKey) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(DERIVED_PREFIX);
    out.extend_from_slice(&derived_key_suffix(key)?);
    Ok(out)
}

fn derived_rebuild_key(key: &DerivedArtifactKey) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(REBUILD_PREFIX);
    out.extend_from_slice(&derived_key_suffix(key)?);
    Ok(out)
}

fn derived_key_suffix(key: &DerivedArtifactKey) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(key.workspace.as_bytes());
    put_str(&mut out, key.facet.as_str())?;
    put_str(&mut out, &key.collection)?;
    put_str(&mut out, &key.artifact)?;
    Ok(out)
}

fn derived_run_id(
    key: &DerivedArtifactKey,
    stamp: &DerivedArtifactStamp,
    algo: Algo,
) -> Result<String> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&derived_key_suffix(key)?);
    bytes.extend_from_slice(stamp.source_digest.bytes());
    put_str(&mut bytes, &stamp.engine_version)?;
    put_str(&mut bytes, &stamp.format_version)?;
    Ok(Digest::hash(algo, &bytes).to_hex())
}

fn encode_record(record: &DerivedArtifactRecord) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(RECORD_MAGIC);
    out.push(record.stamp.source_digest.algo().code());
    out.extend_from_slice(record.stamp.source_digest.bytes());
    out.push(record.payload_digest.algo().code());
    out.extend_from_slice(record.payload_digest.bytes());
    out.extend_from_slice(&record.payload_len.to_be_bytes());
    put_str(&mut out, &record.stamp.engine_version)?;
    put_str(&mut out, &record.stamp.format_version)?;
    Ok(out)
}

fn decode_record(bytes: &[u8], store_algo: Algo) -> Result<DerivedArtifactRecord> {
    let mut p = 0;
    take_magic(bytes, &mut p)?;
    let source_algo = Algo::from_code(take_u8(bytes, &mut p)?)?;
    let source_digest = Digest::of(source_algo, take_digest(bytes, &mut p)?);
    let payload_algo = Algo::from_code(take_u8(bytes, &mut p)?)?;
    if payload_algo != store_algo {
        return Err(corrupt("derived artifact payload digest profile mismatch"));
    }
    let payload_digest = Digest::of(payload_algo, take_digest(bytes, &mut p)?);
    let payload_len = u64::from_be_bytes(take_array(bytes, &mut p)?);
    let engine_version = take_str(bytes, &mut p)?;
    let format_version = take_str(bytes, &mut p)?;
    if p != bytes.len() {
        return Err(corrupt("derived artifact record has trailing bytes"));
    }
    Ok(DerivedArtifactRecord {
        stamp: DerivedArtifactStamp {
            source_digest,
            engine_version,
            format_version,
        },
        payload_digest,
        payload_len,
    })
}

/// Wire magic for a `[source_digest, DerivedArtifactStatus]` result (`Search.status`).
const SEARCH_STATUS_MAGIC: &[u8; 5] = b"LSST1";

fn put_digest(out: &mut Vec<u8>, d: &Digest) {
    out.push(d.algo().code());
    out.extend_from_slice(d.bytes());
}

fn take_digest_full(bytes: &[u8], p: &mut usize) -> Result<Digest> {
    let algo = Algo::from_code(take_u8(bytes, p)?)?;
    Ok(Digest::of(algo, take_digest(bytes, p)?))
}

fn put_stamp(out: &mut Vec<u8>, s: &DerivedArtifactStamp) -> Result<()> {
    put_digest(out, &s.source_digest);
    put_str(out, &s.engine_version)?;
    put_str(out, &s.format_version)
}

fn take_stamp(bytes: &[u8], p: &mut usize) -> Result<DerivedArtifactStamp> {
    let source_digest = take_digest_full(bytes, p)?;
    let engine_version = take_str(bytes, p)?;
    let format_version = take_str(bytes, p)?;
    Ok(DerivedArtifactStamp {
        source_digest,
        engine_version,
        format_version,
    })
}

fn put_status_record(out: &mut Vec<u8>, r: &DerivedArtifactRecord) -> Result<()> {
    put_stamp(out, &r.stamp)?;
    put_digest(out, &r.payload_digest);
    out.extend_from_slice(&r.payload_len.to_be_bytes());
    Ok(())
}

fn take_status_record(bytes: &[u8], p: &mut usize) -> Result<DerivedArtifactRecord> {
    let stamp = take_stamp(bytes, p)?;
    let payload_digest = take_digest_full(bytes, p)?;
    let payload_len = u64::from_be_bytes(take_array(bytes, p)?);
    Ok(DerivedArtifactRecord {
        stamp,
        payload_digest,
        payload_len,
    })
}

/// Encode a `search status` result: the source digest plus the served store's derived-artifact
/// [`DerivedArtifactStatus`] to canonical bytes so it can be returned over the remote `Search.status`
/// method and decoded identically by a local or remote client.
pub fn encode_search_status_result(
    source_digest: &Digest,
    status: &DerivedArtifactStatus,
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(SEARCH_STATUS_MAGIC);
    put_digest(&mut out, source_digest);
    match status {
        DerivedArtifactStatus::Missing => out.push(0),
        DerivedArtifactStatus::Stale { record } => {
            out.push(1);
            put_status_record(&mut out, record)?;
        }
        DerivedArtifactStatus::Ready { record } => {
            out.push(2);
            put_status_record(&mut out, record)?;
        }
        DerivedArtifactStatus::Rebuilding { run_id, stamp } => {
            out.push(3);
            put_str(&mut out, run_id)?;
            put_stamp(&mut out, stamp)?;
        }
        DerivedArtifactStatus::Failed { stamp, message } => {
            out.push(4);
            put_stamp(&mut out, stamp)?;
            put_str(&mut out, message)?;
        }
        DerivedArtifactStatus::Unsupported { stamp, message } => {
            out.push(5);
            put_stamp(&mut out, stamp)?;
            put_str(&mut out, message)?;
        }
    }
    Ok(out)
}

/// Decode the inverse of [`encode_search_status_result`] into `(source_digest, status)`.
pub fn decode_search_status_result(bytes: &[u8]) -> Result<(Digest, DerivedArtifactStatus)> {
    let mut p = 0;
    let magic = take_slice(bytes, &mut p, SEARCH_STATUS_MAGIC.len())?;
    if magic != SEARCH_STATUS_MAGIC {
        return Err(corrupt("search status result magic mismatch"));
    }
    let source_digest = take_digest_full(bytes, &mut p)?;
    let status = match take_u8(bytes, &mut p)? {
        0 => DerivedArtifactStatus::Missing,
        1 => DerivedArtifactStatus::Stale {
            record: take_status_record(bytes, &mut p)?,
        },
        2 => DerivedArtifactStatus::Ready {
            record: take_status_record(bytes, &mut p)?,
        },
        3 => DerivedArtifactStatus::Rebuilding {
            run_id: take_str(bytes, &mut p)?,
            stamp: take_stamp(bytes, &mut p)?,
        },
        4 => DerivedArtifactStatus::Failed {
            stamp: take_stamp(bytes, &mut p)?,
            message: take_str(bytes, &mut p)?,
        },
        5 => DerivedArtifactStatus::Unsupported {
            stamp: take_stamp(bytes, &mut p)?,
            message: take_str(bytes, &mut p)?,
        },
        _ => return Err(corrupt("unknown search status tag")),
    };
    if p != bytes.len() {
        return Err(corrupt("search status result has trailing bytes"));
    }
    Ok((source_digest, status))
}

fn rebuild_status(record: RebuildRecord) -> DerivedArtifactStatus {
    match record.state {
        RebuildState::Rebuilding => DerivedArtifactStatus::Rebuilding {
            run_id: record.run_id,
            stamp: record.stamp,
        },
        RebuildState::Failed => DerivedArtifactStatus::Failed {
            stamp: record.stamp,
            message: record.message,
        },
        RebuildState::Unsupported => DerivedArtifactStatus::Unsupported {
            stamp: record.stamp,
            message: record.message,
        },
    }
}

fn encode_rebuild_record(record: &RebuildRecord) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(REBUILD_MAGIC);
    out.push(match record.state {
        RebuildState::Rebuilding => 1,
        RebuildState::Failed => 2,
        RebuildState::Unsupported => 3,
    });
    out.push(record.stamp.source_digest.algo().code());
    out.extend_from_slice(record.stamp.source_digest.bytes());
    put_str(&mut out, &record.stamp.engine_version)?;
    put_str(&mut out, &record.stamp.format_version)?;
    put_str(&mut out, &record.run_id)?;
    put_str(&mut out, &record.message)?;
    Ok(out)
}

fn decode_rebuild_record(bytes: &[u8]) -> Result<RebuildRecord> {
    let mut p = 0;
    let got = take_slice(bytes, &mut p, REBUILD_MAGIC.len())?;
    if got != REBUILD_MAGIC {
        return Err(corrupt("derived rebuild record magic mismatch"));
    }
    let state = match take_u8(bytes, &mut p)? {
        1 => RebuildState::Rebuilding,
        2 => RebuildState::Failed,
        3 => RebuildState::Unsupported,
        _ => return Err(corrupt("derived rebuild record state mismatch")),
    };
    let source_algo = Algo::from_code(take_u8(bytes, &mut p)?)?;
    let source_digest = Digest::of(source_algo, take_digest(bytes, &mut p)?);
    let engine_version = take_str(bytes, &mut p)?;
    let format_version = take_str(bytes, &mut p)?;
    let run_id = take_str(bytes, &mut p)?;
    let message = take_str(bytes, &mut p)?;
    if p != bytes.len() {
        return Err(corrupt("derived rebuild record has trailing bytes"));
    }
    Ok(RebuildRecord {
        stamp: DerivedArtifactStamp {
            source_digest,
            engine_version,
            format_version,
        },
        run_id,
        state,
        message,
    })
}

fn put_str(out: &mut Vec<u8>, value: &str) -> Result<()> {
    if value.len() > MAX_FIELD_LEN {
        return Err(LoomError::invalid("derived artifact field is too long"));
    }
    out.extend_from_slice(&(value.len() as u16).to_be_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn push_component(out: &mut String, value: &str) {
    if !out.is_empty() {
        out.push('|');
    }
    out.push_str(&value.len().to_string());
    out.push(':');
    out.push_str(value);
}

fn take_magic(bytes: &[u8], p: &mut usize) -> Result<()> {
    let got = take_slice(bytes, p, RECORD_MAGIC.len())?;
    if got != RECORD_MAGIC {
        return Err(corrupt("derived artifact record magic mismatch"));
    }
    Ok(())
}

fn take_u8(bytes: &[u8], p: &mut usize) -> Result<u8> {
    Ok(take_slice(bytes, p, 1)?[0])
}

fn take_digest(bytes: &[u8], p: &mut usize) -> Result<[u8; 32]> {
    take_array(bytes, p)
}

fn take_str(bytes: &[u8], p: &mut usize) -> Result<String> {
    let len = u16::from_be_bytes(take_array(bytes, p)?) as usize;
    let raw = take_slice(bytes, p, len)?;
    std::str::from_utf8(raw)
        .map(|s| s.to_string())
        .map_err(|_| corrupt("derived artifact string is not utf-8"))
}

fn take_array<const N: usize>(bytes: &[u8], p: &mut usize) -> Result<[u8; N]> {
    let slice = take_slice(bytes, p, N)?;
    slice
        .try_into()
        .map_err(|_| corrupt("derived artifact record ended early"))
}

fn take_slice<'a>(bytes: &'a [u8], p: &mut usize, len: usize) -> Result<&'a [u8]> {
    let end = p
        .checked_add(len)
        .ok_or_else(|| corrupt("derived artifact record offset overflow"))?;
    if end > bytes.len() {
        return Err(corrupt("derived artifact record ended early"));
    }
    let out = &bytes[*p..end];
    *p = end;
    Ok(out)
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    #[test]
    fn derived_artifact_registry_has_no_drift() {
        for kind in DERIVED_ARTIFACT_REGISTRY {
            assert!(
                !kind.artifact_family.is_empty(),
                "registry kind has an empty artifact_family"
            );
            assert!(
                !kind.format_version.is_empty(),
                "registry kind {} has an empty format_version",
                kind.artifact_family
            );
        }
        for (i, a) in DERIVED_ARTIFACT_REGISTRY.iter().enumerate() {
            for b in &DERIVED_ARTIFACT_REGISTRY[i + 1..] {
                assert_ne!(
                    a.format_version, b.format_version,
                    "duplicate derived-artifact format_version {}",
                    a.format_version
                );
                assert!(
                    !(a.facet == b.facet && a.artifact_family == b.artifact_family),
                    "duplicate derived-artifact (facet, artifact_family) for {}",
                    a.artifact_family
                );
            }
        }
    }

    #[test]
    fn derived_artifact_format_version_resolves_registered_kinds() {
        assert_eq!(
            derived_artifact_format_version(FacetKind::Search, SEARCH_TANTIVY_ARTIFACT),
            Some(SEARCH_TANTIVY_FORMAT_VERSION)
        );
        assert_eq!(
            derived_artifact_format_version(FacetKind::Search, SEARCH_EMBEDDING_ARTIFACT_PREFIX),
            Some(SEARCH_EMBEDDING_FORMAT_VERSION)
        );
        assert_eq!(
            derived_artifact_format_version(FacetKind::Search, "embedding:doc-1"),
            Some(SEARCH_EMBEDDING_FORMAT_VERSION)
        );
        assert_eq!(
            derived_artifact_format_version(FacetKind::Columnar, COLUMNAR_ARROW_ARTIFACT),
            Some(COLUMNAR_ARROW_FORMAT_VERSION)
        );
        assert_eq!(
            derived_artifact_format_version(FacetKind::Graph, GRAPH_PROPERTY_INDEX_ARTIFACT_PREFIX),
            Some(GRAPH_PROPERTY_INDEX_FORMAT_VERSION)
        );
        assert_eq!(
            derived_artifact_format_version(FacetKind::Graph, "property-index:person_name"),
            Some(GRAPH_PROPERTY_INDEX_FORMAT_VERSION)
        );
        assert_eq!(
            derived_artifact_format_version(FacetKind::Graph, GRAPH_SPATIAL_INDEX_ARTIFACT_PREFIX),
            Some(GRAPH_SPATIAL_INDEX_FORMAT_VERSION)
        );
        assert_eq!(
            derived_artifact_format_version(FacetKind::Graph, "spatial-index:person_loc"),
            Some(GRAPH_SPATIAL_INDEX_FORMAT_VERSION)
        );
        assert_eq!(
            derived_artifact_format_version(
                FacetKind::Dataframe,
                DATAFRAME_MATERIALIZATION_ARTIFACT_PREFIX
            ),
            Some(DATAFRAME_MATERIALIZATION_FORMAT_VERSION)
        );
        assert_eq!(
            derived_artifact_format_version(FacetKind::Dataframe, "materialization:columnar"),
            Some(DATAFRAME_MATERIALIZATION_FORMAT_VERSION)
        );
        assert_eq!(
            derived_artifact_format_version(FacetKind::Calendar, "derived-index:range-search"),
            Some(CALENDAR_DERIVED_INDEX_FORMAT_VERSION)
        );
        assert_eq!(
            derived_artifact_format_version(FacetKind::Contacts, "derived-index:text-search"),
            Some(CONTACTS_DERIVED_INDEX_FORMAT_VERSION)
        );
        assert_eq!(
            derived_artifact_format_version(FacetKind::Mail, "derived-index:text-search"),
            Some(MAIL_DERIVED_INDEX_FORMAT_VERSION)
        );
        assert_eq!(
            derived_artifact_format_version(FacetKind::Search, "unregistered"),
            None
        );
    }

    #[test]
    fn typed_stamps_use_registry_format_version() {
        let digest = Digest::blake3(b"derived-artifact-registry-test");
        let tantivy = search_tantivy_artifact_stamp(digest, "engine-1").expect("tantivy stamp");
        assert_eq!(tantivy.format_version, SEARCH_TANTIVY_KIND.format_version);
        let embedding = search_embedding_artifact_stamp(digest, "model-1", None, "engine-1")
            .expect("embedding stamp");
        assert_eq!(
            embedding.format_version,
            SEARCH_EMBEDDING_KIND.format_version
        );
        let arrow = columnar_arrow_artifact_stamp(digest, "engine-1").expect("arrow stamp");
        assert_eq!(arrow.format_version, COLUMNAR_ARROW_KIND.format_version);
        let property_index =
            graph_property_index_artifact_stamp(digest, "engine-1").expect("property stamp");
        assert_eq!(
            property_index.format_version,
            GRAPH_PROPERTY_INDEX_KIND.format_version
        );
        let spatial_index =
            graph_spatial_index_artifact_stamp(digest, "engine-1").expect("spatial stamp");
        assert_eq!(
            spatial_index.format_version,
            GRAPH_SPATIAL_INDEX_KIND.format_version
        );
        let dataframe_materialization =
            dataframe_materialization_artifact_stamp(digest, "engine-1").expect("dataframe stamp");
        assert_eq!(
            dataframe_materialization.format_version,
            DATAFRAME_MATERIALIZATION_KIND.format_version
        );
        let calendar_index =
            calendar_derived_index_artifact_stamp(digest, "engine-1").expect("calendar stamp");
        assert_eq!(
            calendar_index.format_version,
            CALENDAR_DERIVED_INDEX_KIND.format_version
        );
        let contacts_index =
            contacts_derived_index_artifact_stamp(digest, "engine-1").expect("contacts stamp");
        assert_eq!(
            contacts_index.format_version,
            CONTACTS_DERIVED_INDEX_KIND.format_version
        );
        let mail_index = mail_derived_index_artifact_stamp(digest, "engine-1").expect("mail stamp");
        assert_eq!(
            mail_index.format_version,
            MAIL_DERIVED_INDEX_KIND.format_version
        );
    }
}
