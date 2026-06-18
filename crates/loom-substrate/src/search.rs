use loom_codec::Value;
use loom_types::{Algo, Digest, LoomError, Result, WorkspaceId};
use std::collections::BTreeMap;

use crate::validate_text;

pub const EMBEDDING_PROJECTION_SCHEMA: &str = "loom.substrate.embedding-projection.v1";
pub const EMBEDDING_PROJECTION_JOBS_DIR: &str = ".loom/substrate/embedding-projection-jobs";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Text,
    Regex,
    Semantic,
    Hybrid,
    SimilarTo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionScope {
    Latest,
    AsOf(String),
    History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingProjectionState {
    Queued,
    Building,
    Ready,
    Stale,
    Failed,
    NoKeys,
    NoEngine,
}

impl EmbeddingProjectionState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Building => "building",
            Self::Ready => "ready",
            Self::Stale => "stale",
            Self::Failed => "failed",
            Self::NoKeys => "no_keys",
            Self::NoEngine => "no_engine",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "building" => Ok(Self::Building),
            "ready" => Ok(Self::Ready),
            "stale" => Ok(Self::Stale),
            "failed" => Ok(Self::Failed),
            "no_keys" => Ok(Self::NoKeys),
            "no_engine" => Ok(Self::NoEngine),
            other => Err(LoomError::corrupt(format!(
                "unknown embedding projection state {other:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingProjectionKey {
    pub workspace: String,
    pub facet: String,
    pub collection: String,
    pub entity_id: String,
}

impl EmbeddingProjectionKey {
    pub fn new(
        workspace: impl Into<String>,
        facet: impl Into<String>,
        collection: impl Into<String>,
        entity_id: impl Into<String>,
    ) -> Result<Self> {
        let key = Self {
            workspace: workspace.into(),
            facet: facet.into(),
            collection: collection.into(),
            entity_id: entity_id.into(),
        };
        validate_text("embedding workspace", &key.workspace)?;
        validate_text("embedding facet", &key.facet)?;
        validate_text("embedding collection", &key.collection)?;
        validate_text("embedding entity_id", &key.entity_id)?;
        Ok(key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingProjectionStamp {
    pub content_digest: Digest,
    pub model_id: String,
    pub model_weights_digest: Option<String>,
    pub engine_version: String,
}

impl EmbeddingProjectionStamp {
    pub fn new(
        content_digest: Digest,
        model_id: impl Into<String>,
        model_weights_digest: Option<String>,
        engine_version: impl Into<String>,
    ) -> Result<Self> {
        let stamp = Self {
            content_digest,
            model_id: model_id.into(),
            model_weights_digest,
            engine_version: engine_version.into(),
        };
        validate_text("embedding model_id", &stamp.model_id)?;
        if let Some(digest) = &stamp.model_weights_digest {
            validate_text("embedding model_weights_digest", digest)?;
        }
        validate_text("embedding engine_version", &stamp.engine_version)?;
        Ok(stamp)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingProjectionJob {
    pub key: EmbeddingProjectionKey,
    pub stamp: EmbeddingProjectionStamp,
    pub state: EmbeddingProjectionState,
    pub run_id: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingProjectionPlanAction {
    RebuildUnit,
    RemoveUnit,
    RebuildCollection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingProjectionPlanItem {
    pub action: EmbeddingProjectionPlanAction,
    pub facet: String,
    pub collection_path: Vec<String>,
    pub entity_id: Option<String>,
    pub content_digest: Option<Digest>,
}

impl EmbeddingProjectionPlanItem {
    fn rebuild_unit(
        facet: impl Into<String>,
        collection_path: Vec<String>,
        entity_id: impl Into<String>,
        content_digest: Digest,
    ) -> Result<Self> {
        let item = Self {
            action: EmbeddingProjectionPlanAction::RebuildUnit,
            facet: facet.into(),
            collection_path,
            entity_id: Some(entity_id.into()),
            content_digest: Some(content_digest),
        };
        item.validate()?;
        Ok(item)
    }

    fn remove_unit(
        facet: impl Into<String>,
        collection_path: Vec<String>,
        entity_id: impl Into<String>,
    ) -> Result<Self> {
        let item = Self {
            action: EmbeddingProjectionPlanAction::RemoveUnit,
            facet: facet.into(),
            collection_path,
            entity_id: Some(entity_id.into()),
            content_digest: None,
        };
        item.validate()?;
        Ok(item)
    }

    fn rebuild_collection(facet: impl Into<String>, collection_path: Vec<String>) -> Result<Self> {
        let item = Self {
            action: EmbeddingProjectionPlanAction::RebuildCollection,
            facet: facet.into(),
            collection_path,
            entity_id: None,
            content_digest: None,
        };
        item.validate()?;
        Ok(item)
    }

    fn validate(&self) -> Result<()> {
        validate_text("embedding plan facet", &self.facet)?;
        for segment in &self.collection_path {
            validate_text("embedding plan collection segment", segment)?;
        }
        if let Some(entity_id) = &self.entity_id {
            validate_text("embedding plan entity_id", entity_id)?;
        }
        Ok(())
    }
}

impl EmbeddingProjectionJob {
    pub fn queued(key: EmbeddingProjectionKey, stamp: EmbeddingProjectionStamp) -> Self {
        Self {
            key,
            stamp,
            state: EmbeddingProjectionState::Queued,
            run_id: None,
            message: None,
        }
    }

    pub fn building(mut self, run_id: impl Into<String>) -> Result<Self> {
        let run_id = run_id.into();
        validate_text("embedding run_id", &run_id)?;
        self.state = EmbeddingProjectionState::Building;
        self.run_id = Some(run_id);
        self.message = None;
        Ok(self)
    }

    pub fn ready(mut self) -> Self {
        self.state = EmbeddingProjectionState::Ready;
        self.run_id = None;
        self.message = None;
        self
    }

    pub fn stale(mut self, stamp: EmbeddingProjectionStamp) -> Self {
        self.stamp = stamp;
        self.state = EmbeddingProjectionState::Stale;
        self.run_id = None;
        self.message = None;
        self
    }

    pub fn failed(mut self, message: impl Into<String>) -> Result<Self> {
        let message = message.into();
        validate_text("embedding failure message", &message)?;
        self.state = EmbeddingProjectionState::Failed;
        self.run_id = None;
        self.message = Some(message);
        Ok(self)
    }

    pub fn no_keys(mut self, message: impl Into<String>) -> Result<Self> {
        let message = message.into();
        validate_text("embedding no_keys message", &message)?;
        self.state = EmbeddingProjectionState::NoKeys;
        self.run_id = None;
        self.message = Some(message);
        Ok(self)
    }

    pub fn no_engine(mut self, message: impl Into<String>) -> Result<Self> {
        let message = message.into();
        validate_text("embedding no_engine message", &message)?;
        self.state = EmbeddingProjectionState::NoEngine;
        self.run_id = None;
        self.message = Some(message);
        Ok(self)
    }

    pub fn coalesces_with(&self, other: &Self) -> bool {
        self.key == other.key && self.stamp == other.stamp
    }

    pub fn freshness_against(
        &self,
        current: &EmbeddingProjectionStamp,
    ) -> EmbeddingProjectionState {
        match self.state {
            EmbeddingProjectionState::NoKeys
            | EmbeddingProjectionState::NoEngine
            | EmbeddingProjectionState::Failed
            | EmbeddingProjectionState::Queued
            | EmbeddingProjectionState::Building => self.state,
            EmbeddingProjectionState::Ready | EmbeddingProjectionState::Stale => {
                if &self.stamp == current {
                    EmbeddingProjectionState::Ready
                } else {
                    EmbeddingProjectionState::Stale
                }
            }
        }
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(EMBEDDING_PROJECTION_SCHEMA.to_string()),
            key_value(&self.key),
            stamp_value(&self.stamp),
            Value::Text(self.state.as_str().to_string()),
            optional_text_value(self.run_id.as_deref()),
            optional_text_value(self.message.as_deref()),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let fields = expect_array(value, "embedding projection job")?;
        if fields.len() != 6 {
            return Err(LoomError::corrupt(
                "embedding projection job field count mismatch",
            ));
        }
        expect_text_value(
            &fields[0],
            EMBEDDING_PROJECTION_SCHEMA,
            "embedding projection schema",
        )?;
        Ok(Self {
            key: key_from_value(fields[1].clone())?,
            stamp: stamp_from_value(fields[2].clone())?,
            state: EmbeddingProjectionState::parse(&expect_text(
                fields[3].clone(),
                "embedding projection state",
            )?)?,
            run_id: optional_text_from_value(&fields[4], "embedding projection run_id")?,
            message: optional_text_from_value(&fields[5], "embedding projection message")?,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value())
            .map_err(|e| LoomError::corrupt(format!("embedding projection job cbor encode: {e}")))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(|e| {
            LoomError::corrupt(format!("embedding projection job cbor decode: {e}"))
        })?)
    }

    pub fn identity_digest(&self, algo: Algo) -> Result<Digest> {
        let bytes = loom_codec::encode(&Value::Array(vec![
            Value::Text(EMBEDDING_PROJECTION_SCHEMA.to_string()),
            key_value(&self.key),
            stamp_value(&self.stamp),
        ]))
        .map_err(|e| {
            LoomError::corrupt(format!("embedding projection identity cbor encode: {e}"))
        })?;
        Ok(Digest::hash(algo, &bytes))
    }

    pub fn job_path(&self, algo: Algo) -> Result<String> {
        Ok(format!(
            "{}/{}.cbor",
            EMBEDDING_PROJECTION_JOBS_DIR,
            self.identity_digest(algo)?.to_hex()
        ))
    }
}

fn key_value(key: &EmbeddingProjectionKey) -> Value {
    Value::Array(vec![
        Value::Text(key.workspace.clone()),
        Value::Text(key.facet.clone()),
        Value::Text(key.collection.clone()),
        Value::Text(key.entity_id.clone()),
    ])
}

fn key_from_value(value: Value) -> Result<EmbeddingProjectionKey> {
    let fields = expect_array(value, "embedding projection key")?;
    if fields.len() != 4 {
        return Err(LoomError::corrupt(
            "embedding projection key field count mismatch",
        ));
    }
    EmbeddingProjectionKey::new(
        expect_text(fields[0].clone(), "embedding projection workspace")?,
        expect_text(fields[1].clone(), "embedding projection facet")?,
        expect_text(fields[2].clone(), "embedding projection collection")?,
        expect_text(fields[3].clone(), "embedding projection entity_id")?,
    )
}

fn stamp_value(stamp: &EmbeddingProjectionStamp) -> Value {
    Value::Array(vec![
        Value::Text(stamp.content_digest.to_string()),
        Value::Text(stamp.model_id.clone()),
        optional_text_value(stamp.model_weights_digest.as_deref()),
        Value::Text(stamp.engine_version.clone()),
    ])
}

fn stamp_from_value(value: Value) -> Result<EmbeddingProjectionStamp> {
    let fields = expect_array(value, "embedding projection stamp")?;
    if fields.len() != 4 {
        return Err(LoomError::corrupt(
            "embedding projection stamp field count mismatch",
        ));
    }
    EmbeddingProjectionStamp::new(
        Digest::parse(&expect_text(
            fields[0].clone(),
            "embedding projection content digest",
        )?)?,
        expect_text(fields[1].clone(), "embedding projection model_id")?,
        optional_text_from_value(&fields[2], "embedding projection model_weights_digest")?,
        expect_text(fields[3].clone(), "embedding projection engine_version")?,
    )
}

fn optional_text_value(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |value| Value::Text(value.to_string()))
}

fn optional_text_from_value(value: &Value, name: &str) -> Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::Text(value) => Ok(Some(value.clone())),
        _ => Err(LoomError::corrupt(format!("{name} must be text or null"))),
    }
}

pub fn plan_embedding_projection_from_diff(
    diff_envelope: &[u8],
    digest_algo: Algo,
) -> Result<Vec<EmbeddingProjectionPlanItem>> {
    let frame = expect_array(
        loom_codec::decode(diff_envelope)
            .map_err(|e| LoomError::corrupt(format!("diff envelope cbor: {e}")))?,
        "diff envelope",
    )?;
    if frame.len() != 6 {
        return Err(LoomError::corrupt("diff envelope field count mismatch"));
    }
    expect_text_value(&frame[0], "LMDIFF", "diff envelope magic")?;
    expect_uint_value(&frame[1], "diff envelope version").and_then(|version| {
        if version == 1 {
            Ok(())
        } else {
            Err(LoomError::corrupt("unsupported diff envelope version"))
        }
    })?;
    expect_workspace_bytes(&frame[2])?;
    let _ = digest_value(&frame[3], digest_algo, "from_commit")?;
    let _ = digest_value(&frame[4], digest_algo, "to_commit")?;
    let facets = expect_array(frame[5].clone(), "diff facets")?;
    let mut out = Vec::new();
    for facet_section in facets {
        let section = expect_array(facet_section, "diff facet section")?;
        if section.len() != 2 {
            return Err(LoomError::corrupt(
                "diff facet section field count mismatch",
            ));
        }
        let facet = expect_text(section[0].clone(), "diff facet")?;
        let collections = expect_array(section[1].clone(), "diff collections")?;
        for collection in collections {
            plan_collection(&facet, collection, digest_algo, &mut out)?;
        }
    }
    Ok(out)
}

fn plan_collection(
    facet: &str,
    collection: Value,
    digest_algo: Algo,
    out: &mut Vec<EmbeddingProjectionPlanItem>,
) -> Result<()> {
    let fields = expect_array(collection, "diff collection")?;
    if fields.len() != 3 {
        return Err(LoomError::corrupt(
            "diff collection section field count mismatch",
        ));
    }
    let collection_path = text_array(fields[0].clone(), "diff collection path")?;
    let summary = expect_array(fields[1].clone(), "diff collection summary")?;
    if summary.len() != 5 {
        return Err(LoomError::corrupt("diff collection summary count mismatch"));
    }
    let coarse = expect_bool(summary[4].clone(), "diff collection coarse")?;
    if coarse && facet_embeds(facet) {
        out.push(EmbeddingProjectionPlanItem::rebuild_collection(
            facet,
            collection_path.clone(),
        )?);
    }
    let units = expect_array(fields[2].clone(), "diff unit changes")?;
    for unit in units {
        if let Some(item) = plan_unit(facet, &collection_path, unit, digest_algo)? {
            out.push(item);
        }
    }
    Ok(())
}

fn plan_unit(
    facet: &str,
    collection_path: &[String],
    unit: Value,
    digest_algo: Algo,
) -> Result<Option<EmbeddingProjectionPlanItem>> {
    let fields = expect_array(unit, "diff unit")?;
    if fields.len() != 7 {
        return Err(LoomError::corrupt("diff unit field count mismatch"));
    }
    let unit_kind = expect_text(fields[0].clone(), "diff unit kind")?;
    let unit_key = expect_bytes(fields[1].clone(), "diff unit key")?;
    let change = expect_text(fields[2].clone(), "diff unit change")?;
    let after = optional_digest_value(&fields[4], digest_algo, "diff unit after")?;
    let Some(entity_id) = embedding_entity_id(facet, &unit_kind, &unit_key)? else {
        return Ok(None);
    };
    match change.as_str() {
        "added" | "changed" | "appended" => after
            .map(|digest| {
                EmbeddingProjectionPlanItem::rebuild_unit(
                    facet,
                    collection_path.to_vec(),
                    entity_id,
                    digest,
                )
            })
            .transpose(),
        "removed" => {
            EmbeddingProjectionPlanItem::remove_unit(facet, collection_path.to_vec(), entity_id)
                .map(Some)
        }
        _ => Err(LoomError::corrupt("unknown diff unit change")),
    }
}

fn facet_embeds(facet: &str) -> bool {
    matches!(
        facet,
        "files" | "document" | "queue" | "calendar" | "contacts" | "mail"
    )
}

fn embedding_entity_id(facet: &str, unit_kind: &str, unit_key: &[u8]) -> Result<Option<String>> {
    match (facet, unit_kind) {
        ("files", "path")
        | ("document", "document")
        | ("calendar", "event")
        | ("contacts", "contact")
        | ("mail", "message") => decoded_key_text(unit_key).map(Some),
        ("queue", "entry") => decoded_key_uint(unit_key).map(|seq| Some(seq.to_string())),
        _ => Ok(None),
    }
}

fn decoded_key_text(bytes: &[u8]) -> Result<String> {
    expect_text(
        loom_codec::decode(bytes).map_err(|e| LoomError::corrupt(format!("unit key cbor: {e}")))?,
        "diff unit key text",
    )
}

fn decoded_key_uint(bytes: &[u8]) -> Result<u64> {
    expect_uint(
        loom_codec::decode(bytes).map_err(|e| LoomError::corrupt(format!("unit key cbor: {e}")))?,
        "diff unit key uint",
    )
}

fn expect_workspace_bytes(value: &Value) -> Result<WorkspaceId> {
    let bytes = expect_bytes(value.clone(), "diff workspace")?;
    let bytes: [u8; 16] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| LoomError::corrupt("diff workspace is not 16 bytes"))?;
    Ok(WorkspaceId::from_bytes(bytes))
}

fn optional_digest_value(value: &Value, algo: Algo, name: &str) -> Result<Option<Digest>> {
    match value {
        Value::Null => Ok(None),
        _ => digest_value(value, algo, name).map(Some),
    }
}

fn digest_value(value: &Value, algo: Algo, name: &str) -> Result<Digest> {
    let bytes = expect_bytes(value.clone(), name)?;
    let bytes: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| LoomError::corrupt(format!("{name} is not 32 bytes")))?;
    Ok(Digest::of(algo, bytes))
}

fn text_array(value: Value, name: &str) -> Result<Vec<String>> {
    expect_array(value, name)?
        .into_iter()
        .map(|value| expect_text(value, name))
        .collect()
}

fn expect_array(value: Value, name: &str) -> Result<Vec<Value>> {
    match value {
        Value::Array(values) => Ok(values),
        _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
    }
}

fn expect_text(value: Value, name: &str) -> Result<String> {
    match value {
        Value::Text(text) => Ok(text),
        _ => Err(LoomError::corrupt(format!("{name} must be text"))),
    }
}

fn expect_text_value(value: &Value, expected: &str, name: &str) -> Result<()> {
    match value {
        Value::Text(text) if text == expected => Ok(()),
        _ => Err(LoomError::corrupt(format!("{name} mismatch"))),
    }
}

fn expect_uint(value: Value, name: &str) -> Result<u64> {
    match value {
        Value::Uint(n) => Ok(n),
        _ => Err(LoomError::corrupt(format!("{name} must be uint"))),
    }
}

fn expect_uint_value(value: &Value, name: &str) -> Result<u64> {
    match value {
        Value::Uint(n) => Ok(*n),
        _ => Err(LoomError::corrupt(format!("{name} must be uint"))),
    }
}

fn expect_bool(value: Value, name: &str) -> Result<bool> {
    match value {
        Value::Bool(value) => Ok(value),
        _ => Err(LoomError::corrupt(format!("{name} must be bool"))),
    }
}

fn expect_bytes(value: Value, name: &str) -> Result<Vec<u8>> {
    match value {
        Value::Bytes(bytes) => Ok(bytes),
        _ => Err(LoomError::corrupt(format!("{name} must be bytes"))),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchScope {
    pub workspace: Option<String>,
    pub collection: Option<String>,
    pub facet: Option<String>,
}

impl SearchScope {
    pub fn explicit_collection(
        workspace: impl Into<String>,
        collection: impl Into<String>,
        facet: impl Into<String>,
    ) -> Result<Self> {
        let workspace = workspace.into();
        let collection = collection.into();
        let facet = facet.into();
        validate_text("search workspace", &workspace)?;
        validate_text("search collection", &collection)?;
        validate_text("search facet", &facet)?;
        Ok(Self {
            workspace: Some(workspace),
            collection: Some(collection),
            facet: Some(facet),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchRequest {
    pub query: String,
    pub mode: SearchMode,
    pub scope: SearchScope,
    pub version: VersionScope,
    pub limit: u32,
    pub offset: u32,
}

impl SearchRequest {
    pub fn new(
        query: impl Into<String>,
        mode: SearchMode,
        scope: SearchScope,
        version: VersionScope,
        limit: u32,
        offset: u32,
    ) -> Result<Self> {
        let query = query.into();
        validate_text("search query", &query)?;
        if limit == 0 {
            return Err(LoomError::invalid("search limit must be positive"));
        }
        Ok(Self {
            query,
            mode,
            scope,
            version,
            limit,
            offset,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub facet: String,
    pub workspace: String,
    pub collection: String,
    pub entity_id: String,
    pub field: String,
    pub snippet: String,
    pub offsets: Vec<[u64; 2]>,
    pub match_via: String,
    pub contributing_rungs: Vec<String>,
    pub rung: String,
    pub root: Option<String>,
}

impl SearchHit {
    pub fn validate(&self) -> Result<()> {
        validate_text("search hit facet", &self.facet)?;
        validate_text("search hit workspace", &self.workspace)?;
        validate_text("search hit collection", &self.collection)?;
        validate_text("search hit entity_id", &self.entity_id)?;
        validate_text("search hit field", &self.field)?;
        validate_text("search hit match_via", &self.match_via)?;
        validate_text("search hit rung", &self.rung)?;
        for rung in &self.contributing_rungs {
            validate_text("search hit contributing rung", rung)?;
        }
        for offset in &self.offsets {
            if offset[0] > offset[1] {
                return Err(LoomError::invalid("search hit offset is inverted"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchEngineStatus {
    pub rungs_available: Vec<String>,
    pub rung_selected_ceiling: String,
    pub rrf_k: u32,
    pub rung_depth: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchDegraded {
    pub is_degraded: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub engine: SearchEngineStatus,
    pub reduced: bool,
    pub degraded: SearchDegraded,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RankedSearchHit {
    pub hit: SearchHit,
    pub raw_score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchRungResults {
    pub rung: String,
    pub hits: Vec<RankedSearchHit>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FusedSearchHit {
    pub hit: SearchHit,
    pub fused_score: f64,
    pub raw_score: f64,
    pub rung: String,
}

#[derive(Debug)]
struct FusionEntry {
    hit: SearchHit,
    fused_score: f64,
    raw_score: f64,
    rung: String,
    contributing_rungs: Vec<String>,
    best_priority: u8,
}

pub fn rrf_fuse(
    rungs: &[SearchRungResults],
    rrf_k: u32,
    limit: u32,
    offset: u32,
) -> Result<Vec<FusedSearchHit>> {
    let mut by_key = BTreeMap::new();
    for rung in rungs {
        validate_text("search fusion rung", &rung.rung)?;
        for (rank, ranked) in rung.hits.iter().enumerate() {
            ranked.hit.validate()?;
            let key = hit_identity(&ranked.hit);
            let contribution = 1.0 / f64::from(rrf_k + rank as u32 + 1);
            let priority = match_via_priority(&rung.rung);
            by_key
                .entry(key)
                .and_modify(|entry: &mut FusionEntry| {
                    entry.fused_score += contribution;
                    if !entry.contributing_rungs.contains(&rung.rung) {
                        entry.contributing_rungs.push(rung.rung.clone());
                    }
                    if priority < entry.best_priority
                        || (priority == entry.best_priority
                            && ranked.raw_score.total_cmp(&entry.raw_score).is_gt())
                    {
                        entry.hit = ranked.hit.clone();
                        entry.raw_score = ranked.raw_score;
                        entry.rung = rung.rung.clone();
                        entry.best_priority = priority;
                    }
                })
                .or_insert_with(|| FusionEntry {
                    hit: ranked.hit.clone(),
                    fused_score: contribution,
                    raw_score: ranked.raw_score,
                    rung: rung.rung.clone(),
                    contributing_rungs: vec![rung.rung.clone()],
                    best_priority: priority,
                });
        }
    }
    let mut fused = by_key
        .into_values()
        .map(|mut entry| {
            entry.contributing_rungs.sort();
            entry.hit.contributing_rungs = entry.contributing_rungs;
            entry.hit.match_via = if entry.hit.contributing_rungs.len() > 1 {
                "hybrid".to_string()
            } else {
                entry.rung.clone()
            };
            entry.hit.rung = entry.rung.clone();
            FusedSearchHit {
                hit: entry.hit,
                fused_score: entry.fused_score,
                raw_score: entry.raw_score,
                rung: entry.rung,
            }
        })
        .collect::<Vec<_>>();
    fused.sort_by(|a, b| {
        b.fused_score
            .total_cmp(&a.fused_score)
            .then_with(|| {
                match_via_priority(&a.hit.match_via).cmp(&match_via_priority(&b.hit.match_via))
            })
            .then_with(|| a.hit.entity_id.cmp(&b.hit.entity_id))
            .then_with(|| a.hit.root.cmp(&b.hit.root))
    });
    Ok(fused
        .into_iter()
        .skip(offset as usize)
        .take(if limit == 0 {
            usize::MAX
        } else {
            limit as usize
        })
        .collect())
}

fn hit_identity(hit: &SearchHit) -> (String, String, String, String, String, Option<String>) {
    (
        hit.facet.clone(),
        hit.workspace.clone(),
        hit.collection.clone(),
        hit.entity_id.clone(),
        hit.field.clone(),
        hit.root.clone(),
    )
}

fn match_via_priority(match_via: &str) -> u8 {
    match match_via {
        "lexical" => 0,
        "hybrid" => 0,
        "semantic" => 1,
        "graph" => 2,
        _ => 3,
    }
}

impl SearchResponse {
    pub fn new(
        hits: Vec<SearchHit>,
        engine: SearchEngineStatus,
        reduced: bool,
        degraded: SearchDegraded,
    ) -> Result<Self> {
        for hit in &hits {
            hit.validate()?;
        }
        validate_text("selected search rung", &engine.rung_selected_ceiling)?;
        for rung in &engine.rungs_available {
            validate_text("available search rung", rung)?;
        }
        if degraded.is_degraded {
            validate_text("search degraded reason", &degraded.reason)?;
        }
        Ok(Self {
            hits,
            engine,
            reduced,
            degraded,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_types::{Algo, Digest};

    fn digest(value: &[u8]) -> Digest {
        Digest::hash(Algo::Blake3, value)
    }

    fn embedding_key() -> EmbeddingProjectionKey {
        EmbeddingProjectionKey::new("work", "document", "docs", "doc-1").unwrap()
    }

    fn embedding_stamp(value: &[u8], model: &str) -> EmbeddingProjectionStamp {
        EmbeddingProjectionStamp::new(
            digest(value),
            model,
            Some("weights-a".to_string()),
            "semantic-v1",
        )
        .unwrap()
    }

    fn digest_value(value: &[u8]) -> Value {
        Value::Bytes(digest(value).bytes().to_vec())
    }

    fn key_text(value: &str) -> Vec<u8> {
        loom_codec::encode(&Value::Text(value.to_string())).unwrap()
    }

    fn key_uint(value: u64) -> Vec<u8> {
        loom_codec::encode(&Value::Uint(value)).unwrap()
    }

    fn unit(
        kind: &str,
        key: Vec<u8>,
        change: &str,
        before: Option<&[u8]>,
        after: Option<&[u8]>,
    ) -> Value {
        Value::Array(vec![
            Value::Text(kind.to_string()),
            Value::Bytes(key),
            Value::Text(change.to_string()),
            before.map_or(Value::Null, digest_value),
            after.map_or(Value::Null, digest_value),
            Value::Text("none".to_string()),
            Value::Null,
        ])
    }

    fn collection(path: &[&str], coarse: bool, units: Vec<Value>) -> Value {
        Value::Array(vec![
            Value::Array(
                path.iter()
                    .map(|segment| Value::Text((*segment).to_string()))
                    .collect(),
            ),
            Value::Array(vec![
                Value::Uint(0),
                Value::Uint(0),
                Value::Uint(0),
                Value::Uint(0),
                Value::Bool(coarse),
            ]),
            Value::Array(units),
        ])
    }

    fn facet(name: &str, collections: Vec<Value>) -> Value {
        Value::Array(vec![
            Value::Text(name.to_string()),
            Value::Array(collections),
        ])
    }

    fn diff_frame(facets: Vec<Value>) -> Vec<u8> {
        loom_codec::encode(&Value::Array(vec![
            Value::Text("LMDIFF".to_string()),
            Value::Uint(1),
            Value::Bytes([1u8; 16].to_vec()),
            digest_value(b"from"),
            digest_value(b"to"),
            Value::Array(facets),
        ]))
        .unwrap()
    }

    fn search_hit(entity_id: &str, rung: &str, root: Option<&str>) -> SearchHit {
        SearchHit {
            facet: "document".to_string(),
            workspace: "app".to_string(),
            collection: "docs".to_string(),
            entity_id: entity_id.to_string(),
            field: "body".to_string(),
            snippet: entity_id.to_string(),
            offsets: Vec::new(),
            match_via: rung.to_string(),
            contributing_rungs: vec![rung.to_string()],
            rung: rung.to_string(),
            root: root.map(str::to_string),
        }
    }

    #[test]
    fn embedding_projection_job_coalesces_on_key_and_stamp() {
        let key = embedding_key();
        let stamp = embedding_stamp(b"doc body", "embed-small");
        let queued = EmbeddingProjectionJob::queued(key.clone(), stamp.clone());
        let duplicate = EmbeddingProjectionJob::queued(key, stamp);
        assert!(queued.coalesces_with(&duplicate));

        let changed_model = EmbeddingProjectionJob::queued(
            embedding_key(),
            embedding_stamp(b"doc body", "embed-large"),
        );
        assert!(!queued.coalesces_with(&changed_model));
    }

    #[test]
    fn embedding_projection_job_reports_freshness() {
        let ready = EmbeddingProjectionJob::queued(
            embedding_key(),
            embedding_stamp(b"doc body", "embed-small"),
        )
        .building("run-1")
        .unwrap()
        .ready();
        assert_eq!(
            ready.freshness_against(&embedding_stamp(b"doc body", "embed-small")),
            EmbeddingProjectionState::Ready
        );
        assert_eq!(
            ready.freshness_against(&embedding_stamp(b"doc body changed", "embed-small")),
            EmbeddingProjectionState::Stale
        );
    }

    #[test]
    fn embedding_projection_job_preserves_terminal_reasons() {
        let queued = EmbeddingProjectionJob::queued(
            embedding_key(),
            embedding_stamp(b"doc body", "embed-small"),
        );
        let failed = queued.clone().failed("provider failed").unwrap();
        assert_eq!(failed.state, EmbeddingProjectionState::Failed);
        assert_eq!(failed.message.as_deref(), Some("provider failed"));

        let no_keys = queued.no_keys("plaintext unavailable").unwrap();
        assert_eq!(no_keys.state, EmbeddingProjectionState::NoKeys);
        assert_eq!(
            no_keys.freshness_against(&embedding_stamp(b"doc body", "embed-small")),
            EmbeddingProjectionState::NoKeys
        );
    }

    #[test]
    fn embedding_projection_job_codec_round_trips_no_engine_state() {
        let job = EmbeddingProjectionJob::queued(
            embedding_key(),
            EmbeddingProjectionStamp::new(
                digest(b"root"),
                "loom-built-in-embedding",
                None,
                "unconfigured",
            )
            .unwrap(),
        )
        .no_engine("built-in embedding inference is not configured")
        .unwrap();
        let decoded = EmbeddingProjectionJob::decode(&job.encode().unwrap()).unwrap();
        assert_eq!(decoded, job);
        assert_eq!(
            decoded.identity_digest(Algo::Blake3).unwrap(),
            job.identity_digest(Algo::Blake3).unwrap()
        );
    }

    #[test]
    fn embedding_projection_planner_selects_embed_eligible_units() {
        let envelope = diff_frame(vec![
            facet(
                "document",
                vec![collection(
                    &["docs"],
                    false,
                    vec![unit(
                        "document",
                        key_text("doc-1"),
                        "changed",
                        Some(b"old"),
                        Some(b"new"),
                    )],
                )],
            ),
            facet(
                "queue",
                vec![collection(
                    &["events"],
                    false,
                    vec![unit(
                        "entry",
                        key_uint(7),
                        "appended",
                        None,
                        Some(b"payload"),
                    )],
                )],
            ),
            facet(
                "mail",
                vec![collection(
                    &["alice", "inbox"],
                    false,
                    vec![
                        unit("message", key_text("m1"), "added", None, Some(b"message")),
                        unit(
                            "flags",
                            key_text("m1"),
                            "changed",
                            Some(b"seen"),
                            Some(b"unseen"),
                        ),
                    ],
                )],
            ),
        ]);

        let plan = plan_embedding_projection_from_diff(&envelope, Algo::Blake3).unwrap();
        assert_eq!(plan.len(), 3);
        assert_eq!(
            plan[0],
            EmbeddingProjectionPlanItem::rebuild_unit(
                "document",
                vec!["docs".to_string()],
                "doc-1",
                digest(b"new")
            )
            .unwrap()
        );
        assert_eq!(
            plan[1],
            EmbeddingProjectionPlanItem::rebuild_unit(
                "queue",
                vec!["events".to_string()],
                "7",
                digest(b"payload")
            )
            .unwrap()
        );
        assert_eq!(
            plan[2],
            EmbeddingProjectionPlanItem::rebuild_unit(
                "mail",
                vec!["alice".to_string(), "inbox".to_string()],
                "m1",
                digest(b"message")
            )
            .unwrap()
        );
    }

    #[test]
    fn embedding_projection_planner_records_removals_and_coarse_rebuilds() {
        let envelope = diff_frame(vec![
            facet(
                "document",
                vec![collection(
                    &["docs"],
                    true,
                    vec![unit(
                        "document",
                        key_text("doc-2"),
                        "removed",
                        Some(b"old"),
                        None,
                    )],
                )],
            ),
            facet(
                "kv",
                vec![collection(
                    &["settings"],
                    true,
                    vec![unit(
                        "key",
                        key_text("theme"),
                        "changed",
                        Some(b"a"),
                        Some(b"b"),
                    )],
                )],
            ),
        ]);

        let plan = plan_embedding_projection_from_diff(&envelope, Algo::Blake3).unwrap();
        assert_eq!(
            plan,
            vec![
                EmbeddingProjectionPlanItem::rebuild_collection(
                    "document",
                    vec!["docs".to_string()]
                )
                .unwrap(),
                EmbeddingProjectionPlanItem::remove_unit(
                    "document",
                    vec!["docs".to_string()],
                    "doc-2"
                )
                .unwrap()
            ]
        );
    }

    #[test]
    fn rrf_fuse_merges_duplicate_hits_and_records_hybrid_provenance() {
        let fused = rrf_fuse(
            &[
                SearchRungResults {
                    rung: "lexical".to_string(),
                    hits: vec![
                        RankedSearchHit {
                            hit: search_hit("doc-a", "lexical", Some("root-a")),
                            raw_score: 12.0,
                        },
                        RankedSearchHit {
                            hit: search_hit("doc-b", "lexical", Some("root-a")),
                            raw_score: 4.0,
                        },
                    ],
                },
                SearchRungResults {
                    rung: "semantic".to_string(),
                    hits: vec![RankedSearchHit {
                        hit: search_hit("doc-a", "semantic", Some("root-a")),
                        raw_score: 0.9,
                    }],
                },
            ],
            60,
            10,
            0,
        )
        .unwrap();
        assert_eq!(fused.len(), 2);
        assert_eq!(fused[0].hit.entity_id, "doc-a");
        assert_eq!(fused[0].hit.match_via, "hybrid");
        assert_eq!(
            fused[0].hit.contributing_rungs,
            vec!["lexical".to_string(), "semantic".to_string()]
        );
        assert_eq!(fused[0].rung, "lexical");
        assert!(fused[0].fused_score > fused[1].fused_score);
    }

    #[test]
    fn rrf_fuse_orders_equal_scores_by_match_priority_entity_and_root() {
        let fused = rrf_fuse(
            &[
                SearchRungResults {
                    rung: "semantic".to_string(),
                    hits: vec![RankedSearchHit {
                        hit: search_hit("b", "semantic", Some("root-b")),
                        raw_score: 0.5,
                    }],
                },
                SearchRungResults {
                    rung: "lexical".to_string(),
                    hits: vec![RankedSearchHit {
                        hit: search_hit("a", "lexical", Some("root-b")),
                        raw_score: 0.1,
                    }],
                },
                SearchRungResults {
                    rung: "lexical".to_string(),
                    hits: vec![RankedSearchHit {
                        hit: search_hit("a", "lexical", Some("root-a")),
                        raw_score: 0.1,
                    }],
                },
            ],
            60,
            10,
            0,
        )
        .unwrap();
        assert_eq!(
            fused
                .iter()
                .map(|hit| (
                    hit.hit.match_via.as_str(),
                    hit.hit.entity_id.as_str(),
                    hit.hit.root.as_deref()
                ))
                .collect::<Vec<_>>(),
            vec![
                ("lexical", "a", Some("root-a")),
                ("lexical", "a", Some("root-b")),
                ("semantic", "b", Some("root-b")),
            ]
        );
    }

    #[test]
    fn search_request_requires_query_and_limit() {
        let scope = SearchScope::explicit_collection("app", "docs", "document").unwrap();
        assert!(
            SearchRequest::new(
                "hello",
                SearchMode::Text,
                scope,
                VersionScope::Latest,
                10,
                0
            )
            .is_ok()
        );
        let scope = SearchScope::explicit_collection("app", "docs", "document").unwrap();
        assert!(
            SearchRequest::new("", SearchMode::Text, scope, VersionScope::Latest, 10, 0).is_err()
        );
        let scope = SearchScope::explicit_collection("app", "docs", "document").unwrap();
        assert!(
            SearchRequest::new("hello", SearchMode::Text, scope, VersionScope::Latest, 0, 0)
                .is_err()
        );
    }

    #[test]
    fn search_response_validates_hit_offsets_and_degraded_reason() {
        let hit = SearchHit {
            facet: "document".to_string(),
            workspace: "app".to_string(),
            collection: "docs".to_string(),
            entity_id: "doc-1".to_string(),
            field: "body".to_string(),
            snippet: "hello".to_string(),
            offsets: vec![[0, 5]],
            match_via: "lexical".to_string(),
            contributing_rungs: vec!["lexical".to_string()],
            rung: "lexical".to_string(),
            root: None,
        };
        assert!(
            SearchResponse::new(
                vec![hit],
                SearchEngineStatus {
                    rungs_available: vec!["scan".to_string()],
                    rung_selected_ceiling: "text".to_string(),
                    rrf_k: 60,
                    rung_depth: 10,
                },
                true,
                SearchDegraded {
                    is_degraded: true,
                    reason: "semantic_not_built".to_string(),
                },
            )
            .is_ok()
        );
    }

    #[test]
    fn search_hit_rejects_inverted_offsets() {
        let hit = SearchHit {
            facet: "document".to_string(),
            workspace: "app".to_string(),
            collection: "docs".to_string(),
            entity_id: "doc-1".to_string(),
            field: "body".to_string(),
            snippet: "hello".to_string(),
            offsets: vec![[6, 5]],
            match_via: "lexical".to_string(),
            contributing_rungs: vec!["lexical".to_string()],
            rung: "lexical".to_string(),
            root: None,
        };
        assert!(hit.validate().is_err());
    }
}
