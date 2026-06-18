//! The vector facet substrate: a pure-Rust, `wasm32`-clean, deterministic **exact** vector
//! store: a fixed dimension + metric per set, id-keyed vectors with metadata, an exact top-k search
//! with a metadata **pre-filter**, and a canonical encoding so a set versions/syncs like any other
//! Loom state.
//!
//! Exact search is the cross-platform **contract**: identical results on native and `wasm32`, recall
//! 1.0, no index to build. Accelerators sit behind separate Rust helper APIs and re-score returned
//! candidates exactly, but approximate candidate recall is not part of this facade. Derived ANN indexes
//! are never stored here, so nothing in this module needs a non-portable dependency.

use crate::AclRight;
use crate::cbor;
use crate::error::{Code, LoomError, Result};
use crate::inference::{TextEmbeddingHandle, TextEmbeddingModel};
use crate::provider::ObjectStore;
use crate::tabular::Value;
use crate::vcs::Loom;
use crate::workspace::{FacetKind, WorkspaceId, facet_path, facet_root};
pub use loom_vector::{
    AcceleratorPolicy, Hit, MetaFilter, Metric, PqIndex, VectorAccelerator, VectorEntry, VectorSet,
    search_auto, search_with_policy, sort_hits,
};
use std::collections::{BTreeMap, BTreeSet};

// ---- versioned-set facade over the engine -------------------------------------------------------

fn set_path(name: &str) -> String {
    facet_path(FacetKind::Vector, name)
}

fn manifest_path(name: &str) -> String {
    format!("{}/.manifest", set_path(name))
}

fn entries_path(name: &str) -> String {
    format!("{}/entries", set_path(name))
}

fn indexes_path(name: &str) -> String {
    format!("{}/indexes", set_path(name))
}

fn sources_path(name: &str) -> String {
    format!("{}/sources", set_path(name))
}

fn entry_path(name: &str, id: &str) -> String {
    format!("{}/{}", entries_path(name), vector_id_file_name(id))
}

fn source_path(name: &str, id: &str) -> String {
    format!("{}/{}", sources_path(name), vector_id_file_name(id))
}

fn embedding_model_path(name: &str) -> String {
    format!("{}/.embedding", set_path(name))
}

fn metadata_index_key_path(name: &str, key: &str) -> String {
    format!("{}/{}", indexes_path(name), path_segment_for_text(key))
}

fn metadata_index_value_path(name: &str, key: &str, value: &Value) -> String {
    format!(
        "{}/{}",
        metadata_index_key_path(name, key),
        path_segment_for_value(value)
    )
}

fn metadata_index_marker_path(name: &str, key: &str, value: &Value, id: &str) -> String {
    format!(
        "{}/{}",
        metadata_index_value_path(name, key, value),
        vector_id_file_name(id)
    )
}

fn vector_id_file_name(id: &str) -> String {
    path_segment_for_text(id)
}

fn path_segment_for_text(value: &str) -> String {
    cbor::encode(&cbor::Value::Text(value.to_string()))
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn path_segment_for_value(value: &Value) -> String {
    cbor::encode(&crate::tabular::cell_value(value))
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn vector_id_from_file_name(name: &str) -> Result<String> {
    let bytes = hex::decode(name).map_err(|_| LoomError::corrupt("vector id path is not hex"))?;
    cbor::as_text(cbor::decode(&bytes)?)
}

fn vector_bytes(vector: &[f32]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(vector.len() * 4);
    for f in vector {
        raw.extend_from_slice(&f.to_bits().to_le_bytes());
    }
    raw
}

fn vector_from_bytes(raw: &[u8]) -> Result<Vec<f32>> {
    if !raw.len().is_multiple_of(4) {
        return Err(LoomError::corrupt(
            "vector byte length is not a multiple of 4",
        ));
    }
    Ok(raw
        .chunks_exact(4)
        .map(|c| f32::from_bits(u32::from_le_bytes([c[0], c[1], c[2], c[3]])))
        .collect())
}

fn metadata_value(metadata: &BTreeMap<String, Value>) -> cbor::Value {
    use cbor::Value::{Map, Text};
    Map(metadata
        .iter()
        .map(|(k, v)| (Text(k.clone()), crate::tabular::cell_value(v)))
        .collect())
}

fn metadata_from_pairs(pairs: Vec<(cbor::Value, cbor::Value)>) -> Result<BTreeMap<String, Value>> {
    let mut metadata = BTreeMap::new();
    for (key, value) in pairs {
        metadata.insert(cbor::as_text(key)?, crate::tabular::cell_from(value)?);
    }
    Ok(metadata)
}

fn encode_embedding_model(model: &TextEmbeddingModel) -> Vec<u8> {
    use cbor::Value::{Array, Text, Uint};
    cbor::encode(&Array(vec![
        Uint(1),
        Text(model.model_id.clone()),
        Uint(model.dimension as u64),
        Text(model.weights_digest.clone().unwrap_or_default()),
    ]))
}

fn decode_embedding_model(bytes: &[u8]) -> Result<TextEmbeddingModel> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let version = fields.uint()?;
    if version != 1 {
        return Err(LoomError::corrupt(format!(
            "unknown vector embedding model version {version}"
        )));
    }
    let model_id = fields.text()?;
    let dimension = usize::try_from(fields.uint()?)
        .map_err(|_| LoomError::corrupt("embedding model dimension out of range"))?;
    let weights_digest = match fields.text()? {
        digest if digest.is_empty() => None,
        digest => Some(digest),
    };
    fields.end()?;
    Ok(TextEmbeddingModel {
        model_id,
        dimension,
        weights_digest,
    })
}

fn encode_manifest(dim: usize, metric: Metric, metadata_indexes: &BTreeSet<String>) -> Vec<u8> {
    use cbor::Value::{Array, Text, Uint};
    cbor::encode(&Array(vec![
        Uint(2),
        Uint(dim as u64),
        Uint(u64::from(metric.tag())),
        Array(metadata_indexes.iter().cloned().map(Text).collect()),
    ]))
}

fn decode_manifest(bytes: &[u8]) -> Result<(usize, Metric, BTreeSet<String>)> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let version = fields.uint()?;
    if version != 2 {
        return Err(LoomError::corrupt(format!(
            "unknown vector manifest version {version}"
        )));
    }
    let dim = usize::try_from(fields.uint()?)
        .map_err(|_| LoomError::corrupt("vector dim out of range"))?;
    let tag = u8::try_from(fields.uint()?)
        .map_err(|_| LoomError::corrupt("vector metric tag out of range"))?;
    let mut metadata_indexes = BTreeSet::new();
    for key in fields.array()? {
        metadata_indexes.insert(cbor::as_text(key)?);
    }
    fields.end()?;
    Ok((dim, Metric::from_tag(tag)?, metadata_indexes))
}

fn encode_entry_raw(vector: Vec<u8>, metadata: &BTreeMap<String, Value>) -> Vec<u8> {
    use cbor::Value::{Array, Bytes};
    cbor::encode(&Array(vec![Bytes(vector), metadata_value(metadata)]))
}

fn encode_entry(vector: &[f32], metadata: &BTreeMap<String, Value>) -> Vec<u8> {
    encode_entry_raw(vector_bytes(vector), metadata)
}

fn decode_entry_raw(bytes: &[u8]) -> Result<(Vec<u8>, BTreeMap<String, Value>)> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let vector = fields.bytes()?;
    if !vector.len().is_multiple_of(4) {
        return Err(LoomError::corrupt(
            "vector byte length is not a multiple of 4",
        ));
    }
    let metadata = metadata_from_pairs(fields.map()?)?;
    fields.end()?;
    Ok((vector, metadata))
}

fn decode_entry(bytes: &[u8]) -> Result<VectorEntry> {
    let (vector, metadata) = decode_entry_raw(bytes)?;
    Ok((vector_from_bytes(&vector)?, metadata))
}

pub(crate) fn merge_entry_bytes(
    base: Option<&[u8]>,
    ours: &[u8],
    theirs: &[u8],
) -> Result<Option<Vec<u8>>> {
    if ours == theirs {
        return Ok(Some(ours.to_vec()));
    }
    let base = match base {
        Some(bytes) => Some(decode_entry_raw(bytes)?),
        None => None,
    };
    let ours = decode_entry_raw(ours)?;
    let theirs = decode_entry_raw(theirs)?;

    let vector = if ours.0 == theirs.0 {
        ours.0
    } else if base.as_ref().is_some_and(|(vector, _)| vector == &ours.0) {
        theirs.0
    } else if base.as_ref().is_some_and(|(vector, _)| vector == &theirs.0) {
        ours.0
    } else {
        return Ok(None);
    };

    let mut keys = BTreeSet::new();
    keys.extend(ours.1.keys().cloned());
    keys.extend(theirs.1.keys().cloned());
    if let Some((_, base_meta)) = &base {
        keys.extend(base_meta.keys().cloned());
    }
    let mut metadata = BTreeMap::new();
    for key in keys {
        let base_value = base.as_ref().and_then(|(_, meta)| meta.get(&key));
        let ours_value = ours.1.get(&key);
        let theirs_value = theirs.1.get(&key);
        let merged = if ours_value == theirs_value {
            ours_value.cloned()
        } else if base_value == ours_value {
            theirs_value.cloned()
        } else if base_value == theirs_value {
            ours_value.cloned()
        } else {
            return Ok(None);
        };
        if let Some(value) = merged {
            metadata.insert(key, value);
        }
    }
    Ok(Some(encode_entry_raw(vector, &metadata)))
}

fn read_manifest<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<Option<(usize, Metric, BTreeSet<String>)>> {
    match loom.read_file_reserved(ns, &manifest_path(name)) {
        Ok(bytes) => Ok(Some(decode_manifest(&bytes)?)),
        Err(err) if err.code == Code::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn require_manifest<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<(usize, Metric, BTreeSet<String>)> {
    read_manifest(loom, ns, name)?
        .ok_or_else(|| LoomError::not_found(format!("vector set {name:?} not found")))
}

fn load_structured_set<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    dim: usize,
    metric: Metric,
    metadata_indexes: BTreeSet<String>,
) -> Result<VectorSet> {
    let mut set = VectorSet::new(dim, metric);
    for key in metadata_indexes {
        set.add_metadata_index(key);
    }
    for entry in loom.list_directory(ns, &entries_path(name))? {
        if entry.kind != crate::fs::FileKind::File {
            return Err(LoomError::corrupt("vector entry path is not a file"));
        }
        let id = vector_id_from_file_name(&entry.name)?;
        let (vector, metadata) =
            decode_entry(&loom.read_file_reserved(ns, &entry_path(name, &id))?)?;
        set.upsert(id, vector, metadata)?;
    }
    Ok(set)
}

fn remove_marker_files_under<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    path: &str,
) -> Result<()> {
    match loom.list_directory(ns, path) {
        Ok(entries) => {
            for entry in entries {
                let child = format!("{path}/{}", entry.name);
                match entry.kind {
                    crate::fs::FileKind::File => loom.remove_file_reserved(ns, &child)?,
                    crate::fs::FileKind::Directory => remove_marker_files_under(loom, ns, &child)?,
                    crate::fs::FileKind::Symlink => {
                        return Err(LoomError::corrupt(
                            "vector metadata index path is a symlink",
                        ));
                    }
                }
            }
            Ok(())
        }
        Err(err) if err.code == Code::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn write_metadata_index_marker<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    key: &str,
    value: &Value,
    id: &str,
) -> Result<()> {
    let value_path = metadata_index_value_path(name, key, value);
    loom.create_directory_reserved(ns, &value_path, true)?;
    loom.write_file_reserved(
        ns,
        &metadata_index_marker_path(name, key, value, id),
        &[],
        0o100644,
    )
}

fn remove_metadata_index_marker<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    key: &str,
    value: &Value,
    id: &str,
) -> Result<()> {
    loom.remove_file_reserved(ns, &metadata_index_marker_path(name, key, value, id))
}

fn write_metadata_index_markers<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
    metadata: &BTreeMap<String, Value>,
    metadata_indexes: &BTreeSet<String>,
) -> Result<()> {
    for key in metadata_indexes {
        if let Some(value) = metadata.get(key) {
            write_metadata_index_marker(loom, ns, name, key, value, id)?;
        }
    }
    Ok(())
}

fn remove_metadata_index_markers<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
    metadata: &BTreeMap<String, Value>,
    metadata_indexes: &BTreeSet<String>,
) -> Result<()> {
    for key in metadata_indexes {
        if let Some(value) = metadata.get(key) {
            remove_metadata_index_marker(loom, ns, name, key, value, id)?;
        }
    }
    Ok(())
}

fn remove_source_text<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
) -> Result<()> {
    match loom.remove_file_reserved(ns, &source_path(name, id)) {
        Ok(()) => Ok(()),
        Err(err) if err.code == Code::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn write_embedding_model<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    model: &TextEmbeddingModel,
) -> Result<()> {
    match vector_embedding_model(loom, ns, name)? {
        Some(existing) if existing != *model => Err(LoomError::new(
            Code::Conflict,
            format!(
                "vector set {name:?} uses embedding model {:?}, not {:?}",
                existing.model_id, model.model_id
            ),
        )),
        Some(_) => Ok(()),
        None => loom.write_file_reserved(
            ns,
            &embedding_model_path(name),
            &encode_embedding_model(model),
            0o100644,
        ),
    }
}

fn metadata_index_ids<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    key: &str,
    value: &Value,
) -> Result<BTreeSet<String>> {
    let value_path = metadata_index_value_path(name, key, value);
    let entries = match loom.list_directory(ns, &value_path) {
        Ok(entries) => entries,
        Err(err) if err.code == Code::NotFound => return Ok(BTreeSet::new()),
        Err(err) => return Err(err),
    };
    let mut ids = BTreeSet::new();
    for entry in entries {
        if entry.kind != crate::fs::FileKind::File {
            return Err(LoomError::corrupt(
                "vector metadata index marker is not a file",
            ));
        }
        ids.insert(vector_id_from_file_name(&entry.name)?);
    }
    Ok(ids)
}

fn indexed_filter_candidate_ids<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    filter: &MetaFilter,
    metadata_indexes: &BTreeSet<String>,
) -> Result<Option<BTreeSet<String>>> {
    match filter {
        MetaFilter::All => Ok(None),
        MetaFilter::Eq(key, value) => {
            if metadata_indexes.contains(key) {
                Ok(Some(metadata_index_ids(loom, ns, name, key, value)?))
            } else {
                Ok(None)
            }
        }
        MetaFilter::And(left, right) => {
            let left = indexed_filter_candidate_ids(loom, ns, name, left, metadata_indexes)?;
            let right = indexed_filter_candidate_ids(loom, ns, name, right, metadata_indexes)?;
            Ok(match (left, right) {
                (Some(left), Some(right)) => {
                    Some(left.intersection(&right).cloned().collect::<BTreeSet<_>>())
                }
                (Some(ids), None) | (None, Some(ids)) => Some(ids),
                (None, None) => None,
            })
        }
        MetaFilter::In(key, values) => {
            if metadata_indexes.contains(key) {
                let mut ids = BTreeSet::new();
                for value in values {
                    ids.extend(metadata_index_ids(loom, ns, name, key, value)?);
                }
                Ok(Some(ids))
            } else {
                Ok(None)
            }
        }
        MetaFilter::Ne(_, _)
        | MetaFilter::Lt(_, _)
        | MetaFilter::Le(_, _)
        | MetaFilter::Gt(_, _)
        | MetaFilter::Ge(_, _)
        | MetaFilter::Exists(_)
        | MetaFilter::Or(_, _)
        | MetaFilter::Not(_) => Ok(None),
    }
}

/// Stage a vector set under `name` in `ns` as a manifest plus one entry file per id. The derived ANN
/// index is not stored, only the source-of-truth vectors; `commit` snapshots it.
pub fn put_vector_set<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    set: &VectorSet,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Write)?;
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Vector), true)?;
    loom.create_directory_reserved(ns, &set_path(name), true)?;
    loom.create_directory_reserved(ns, &entries_path(name), true)?;
    loom.create_directory_reserved(ns, &indexes_path(name), true)?;
    loom.create_directory_reserved(ns, &sources_path(name), true)?;
    loom.write_file_reserved(
        ns,
        &manifest_path(name),
        &encode_manifest(
            set.dim(),
            set.metric(),
            &set.metadata_indexes()
                .map(ToOwned::to_owned)
                .collect::<BTreeSet<_>>(),
        ),
        0o100644,
    )?;

    let desired = set.ids().map(vector_id_file_name).collect::<BTreeSet<_>>();
    for entry in loom.list_directory(ns, &entries_path(name))? {
        if entry.kind == crate::fs::FileKind::File && !desired.contains(&entry.name) {
            let path = format!("{}/{}", entries_path(name), entry.name);
            loom.remove_file_reserved(ns, &path)?;
        }
    }
    for entry in loom.list_directory(ns, &sources_path(name))? {
        if entry.kind == crate::fs::FileKind::File && !desired.contains(&entry.name) {
            let path = format!("{}/{}", sources_path(name), entry.name);
            loom.remove_file_reserved(ns, &path)?;
        }
    }
    for (id, vector, metadata) in set.entries() {
        loom.write_file_reserved(
            ns,
            &entry_path(name, id),
            &encode_entry(vector, metadata),
            0o100644,
        )?;
    }
    remove_marker_files_under(loom, ns, &indexes_path(name))?;
    let metadata_indexes = set
        .metadata_indexes()
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    for key in &metadata_indexes {
        loom.create_directory_reserved(ns, &metadata_index_key_path(name, key), true)?;
    }
    for (id, _, metadata) in set.entries() {
        write_metadata_index_markers(loom, ns, name, id, metadata, &metadata_indexes)?;
    }
    Ok(())
}

/// Load the vector set named `name` from `ns`'s current working tree, or `NOT_FOUND`.
pub fn get_vector_set<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<VectorSet> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Read)?;
    let (dim, metric, metadata_indexes) = require_manifest(loom, ns, name)?;
    load_structured_set(loom, ns, name, dim, metric, metadata_indexes)
}

/// Digest of the committed vector source that derived accelerators must be stamped against.
pub fn vector_source_digest<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<crate::Digest> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Read)?;
    let (dim, metric, metadata_indexes) = require_manifest(loom, ns, name)?;
    let set = load_structured_set(loom, ns, name, dim, metric, metadata_indexes.clone())?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"loom-vector-derived-source-v1");
    bytes.extend_from_slice(&encode_manifest(dim, metric, &metadata_indexes));
    for (id, vector, metadata) in set.entries() {
        bytes.extend_from_slice(&(id.len() as u64).to_be_bytes());
        bytes.extend_from_slice(id.as_bytes());
        let entry = encode_entry(vector, metadata);
        bytes.extend_from_slice(&(entry.len() as u64).to_be_bytes());
        bytes.extend_from_slice(&entry);
    }
    Ok(crate::Digest::hash(loom.store().digest_algo(), &bytes))
}

/// Create an empty vector set `name` in `ns` with fixed `dim` and `metric`, staging it. `CONFLICT` if a
/// set already exists under `name` (dimension and metric are immutable once created).
pub fn vector_create<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    dim: usize,
    metric: Metric,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Write)?;
    if read_manifest(loom, ns, name)?.is_some() {
        return Err(LoomError::new(
            Code::Conflict,
            format!("vector set {name:?} already exists"),
        ));
    }
    if loom.exists(ns, &set_path(name))? {
        return Err(LoomError::new(
            Code::Conflict,
            format!("vector set {name:?} already exists"),
        ));
    }
    put_vector_set(loom, ns, name, &VectorSet::new(dim, metric))
}

/// Insert or replace the vector + metadata at `id` in set `name`, staging the result. `NOT_FOUND` if the
/// set was never created; `DIMENSION_MISMATCH` if the vector width is wrong.
pub fn vector_upsert<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
    vector: Vec<f32>,
    metadata: BTreeMap<String, Value>,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Write)?;
    let (dim, _, metadata_indexes) = require_manifest(loom, ns, name)?;
    if vector.len() != dim {
        return Err(LoomError::dimension_mismatch(format!(
            "vector has dimension {}, set is {}",
            vector.len(),
            dim
        )));
    }
    let old = match loom.read_file_reserved(ns, &entry_path(name, id)) {
        Ok(bytes) => Some(decode_entry(&bytes)?),
        Err(err) if err.code == Code::NotFound => None,
        Err(err) => return Err(err),
    };
    if let Some((_, metadata)) = &old {
        remove_metadata_index_markers(loom, ns, name, id, metadata, &metadata_indexes)?;
    }
    loom.write_file_reserved(
        ns,
        &entry_path(name, id),
        &encode_entry(&vector, &metadata),
        0o100644,
    )?;
    remove_source_text(loom, ns, name, id)?;
    write_metadata_index_markers(loom, ns, name, id, &metadata, &metadata_indexes)
}

/// Insert or replace `id` with an already-computed vector while storing the source text that produced
/// it. When `model` is present, the vector set records one model profile and rejects mismatched
/// source-aware writes.
#[allow(clippy::too_many_arguments)]
pub fn vector_upsert_with_source<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
    vector: Vec<f32>,
    metadata: BTreeMap<String, Value>,
    source_text: &str,
    model: Option<TextEmbeddingModel>,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Write)?;
    let (dim, _, _) = require_manifest(loom, ns, name)?;
    if vector.len() != dim {
        return Err(LoomError::dimension_mismatch(format!(
            "vector has dimension {}, set is {}",
            vector.len(),
            dim
        )));
    }
    if let Some(model) = &model {
        if model.dimension != dim {
            return Err(LoomError::dimension_mismatch(format!(
                "embedding model has dimension {}, set is {}",
                model.dimension, dim
            )));
        }
        write_embedding_model(loom, ns, name, model)?;
    }
    vector_upsert(loom, ns, name, id, vector, metadata)?;
    loom.create_directory_reserved(ns, &sources_path(name), true)?;
    loom.write_file_reserved(ns, &source_path(name, id), source_text.as_bytes(), 0o100644)
}

/// Embed `source_text` through the installed provider and store both the resulting vector and source
/// text. `UNSUPPORTED` is returned when no embedding provider is configured.
pub fn vector_upsert_text<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
    source_text: &str,
    metadata: BTreeMap<String, Value>,
    embeddings: &TextEmbeddingHandle,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Write)?;
    let model = embeddings.model().ok_or_else(|| {
        LoomError::unsupported("no embedding provider is configured for this loom")
    })?;
    let texts = vec![source_text.to_string()];
    let mut vectors = embeddings.embed(&texts)?;
    let vector = vectors
        .pop()
        .ok_or_else(|| LoomError::new(Code::Internal, "embedding provider returned no vectors"))?;
    vector_upsert_with_source(
        loom,
        ns,
        name,
        id,
        vector,
        metadata,
        source_text,
        Some(model),
    )
}

/// The vector + metadata at `id` in set `name`, or `None` when `id` is absent. `NOT_FOUND` if the set
/// does not exist.
pub fn vector_get<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
) -> Result<Option<VectorEntry>> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Read)?;
    let (dim, _, _) = require_manifest(loom, ns, name)?;
    match loom.read_file_reserved(ns, &entry_path(name, id)) {
        Ok(bytes) => {
            let entry = decode_entry(&bytes)?;
            if entry.0.len() != dim {
                return Err(LoomError::corrupt("vector entry dimension mismatch"));
            }
            Ok(Some(entry))
        }
        Err(err) if err.code == Code::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Source text stored for vector `id`, or `None` when the vector was inserted without source text.
pub fn vector_source_text<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
) -> Result<Option<String>> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Read)?;
    require_manifest(loom, ns, name)?;
    match loom.read_file_reserved(ns, &source_path(name, id)) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| LoomError::corrupt("vector source text is not UTF-8")),
        Err(err) if err.code == Code::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Embedding model profile recorded for source-aware writes to set `name`.
pub fn vector_embedding_model<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<Option<TextEmbeddingModel>> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Read)?;
    require_manifest(loom, ns, name)?;
    match loom.read_file_reserved(ns, &embedding_model_path(name)) {
        Ok(bytes) => Ok(Some(decode_embedding_model(&bytes)?)),
        Err(err) if err.code == Code::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Vector ids in set `name`, sorted ascending. When `prefix` is present, only ids with that string
/// prefix are returned. `NOT_FOUND` if the set does not exist.
pub fn vector_ids<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    prefix: Option<&str>,
) -> Result<Vec<String>> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Read)?;
    require_manifest(loom, ns, name)?;
    let mut ids = Vec::new();
    for entry in loom.list_directory(ns, &entries_path(name))? {
        if entry.kind != crate::fs::FileKind::File {
            return Err(LoomError::corrupt("vector entry path is not a file"));
        }
        let id = vector_id_from_file_name(&entry.name)?;
        if prefix.is_none_or(|p| id.starts_with(p)) {
            ids.push(id);
        }
    }
    ids.sort();
    Ok(ids)
}

/// Declared metadata equality indexes for set `name`, sorted ascending.
pub fn vector_metadata_index_keys<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<Vec<String>> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Read)?;
    let (_, _, metadata_indexes) = require_manifest(loom, ns, name)?;
    Ok(metadata_indexes.into_iter().collect())
}

/// Declare and build an exact equality index for metadata `key`. Returns whether a new index was
/// declared. Indexed search still validates the full filter against every narrowed candidate.
pub fn vector_create_metadata_index<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    key: &str,
) -> Result<bool> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Write)?;
    let (dim, metric, mut metadata_indexes) = require_manifest(loom, ns, name)?;
    if !metadata_indexes.insert(key.to_string()) {
        return Ok(false);
    }
    loom.write_file_reserved(
        ns,
        &manifest_path(name),
        &encode_manifest(dim, metric, &metadata_indexes),
        0o100644,
    )?;
    remove_marker_files_under(loom, ns, &metadata_index_key_path(name, key))?;
    loom.create_directory_reserved(ns, &metadata_index_key_path(name, key), true)?;
    for entry in loom.list_directory(ns, &entries_path(name))? {
        if entry.kind != crate::fs::FileKind::File {
            return Err(LoomError::corrupt("vector entry path is not a file"));
        }
        let id = vector_id_from_file_name(&entry.name)?;
        let (_, metadata) = decode_entry(&loom.read_file_reserved(ns, &entry_path(name, &id))?)?;
        if let Some(value) = metadata.get(key) {
            write_metadata_index_marker(loom, ns, name, key, value, &id)?;
        }
    }
    Ok(true)
}

/// Drop the maintained equality index for metadata `key`. Returns whether an index was present.
pub fn vector_drop_metadata_index<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    key: &str,
) -> Result<bool> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Write)?;
    let (dim, metric, mut metadata_indexes) = require_manifest(loom, ns, name)?;
    if !metadata_indexes.remove(key) {
        return Ok(false);
    }
    loom.write_file_reserved(
        ns,
        &manifest_path(name),
        &encode_manifest(dim, metric, &metadata_indexes),
        0o100644,
    )?;
    remove_marker_files_under(loom, ns, &metadata_index_key_path(name, key))?;
    Ok(true)
}

/// Remove `id` from set `name`; returns whether it was present. `NOT_FOUND` if the set does not exist.
pub fn vector_delete<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
) -> Result<bool> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Write)?;
    let (_, _, metadata_indexes) = require_manifest(loom, ns, name)?;
    match loom.read_file_reserved(ns, &entry_path(name, id)) {
        Ok(bytes) => {
            let (_, metadata) = decode_entry(&bytes)?;
            remove_metadata_index_markers(loom, ns, name, id, &metadata, &metadata_indexes)?;
            loom.remove_file_reserved(ns, &entry_path(name, id))?;
            remove_source_text(loom, ns, name, id)?;
            Ok(true)
        }
        Err(err) if err.code == Code::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

/// Exact top-`k` nearest neighbours of `query` in set `name` among vectors passing `filter`. `NOT_FOUND`
/// if the set does not exist; `DIMENSION_MISMATCH` if the query width is wrong.
pub fn vector_search<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    query: &[f32],
    k: usize,
    filter: &MetaFilter,
) -> Result<Vec<Hit>> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Read)?;
    let (dim, metric, metadata_indexes) = require_manifest(loom, ns, name)?;
    if query.len() != dim {
        return Err(LoomError::dimension_mismatch(format!(
            "query has dimension {}, set is {}",
            query.len(),
            dim
        )));
    }
    if let Some(ids) = indexed_filter_candidate_ids(loom, ns, name, filter, &metadata_indexes)? {
        let mut hits = Vec::new();
        for id in ids {
            let bytes = loom
                .read_file_reserved(ns, &entry_path(name, &id))
                .map_err(|err| {
                    if err.code == Code::NotFound {
                        LoomError::corrupt("vector metadata index references a missing entry")
                    } else {
                        err
                    }
                })?;
            let (vector, metadata) = decode_entry(&bytes)?;
            if vector.len() != dim {
                return Err(LoomError::corrupt("vector entry dimension mismatch"));
            }
            if filter.eval(&metadata) {
                hits.push(Hit {
                    id,
                    score: metric.score(query, &vector),
                });
            }
        }
        Ok(sort_hits(hits, k))
    } else {
        load_structured_set(loom, ns, name, dim, metric, metadata_indexes)?.search(query, k, filter)
    }
}

/// Build the wasm-clean product-quantization accelerator over set `name`: `m` subspaces, `k` centroids
/// per subspace, `iters` k-means iterations. A derived, rebuildable view over the source-of-truth
/// vectors - never stored, rebuilt from the set on demand. `NOT_FOUND` if the set does not exist.
pub fn vector_build_pq_index<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    m: usize,
    k: usize,
    iters: usize,
) -> Result<PqIndex> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Read)?;
    PqIndex::build(&get_vector_set(loom, ns, name)?, m, k, iters)
}

/// Top-`k` search of set `name` that uses `accel` above `threshold` vectors and exact search below it.
/// The exact path returns the portable contract. The accelerator path returns exact-scored hits in
/// deterministic order for the candidates it finds, but approximate candidate recall can differ. Pass
/// `None` for `accel` to always run exact. `NOT_FOUND` if the set does not exist;
/// `DIMENSION_MISMATCH` on a wrong-width query.
#[allow(clippy::too_many_arguments)]
pub fn vector_search_auto<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    query: &[f32],
    k: usize,
    filter: &MetaFilter,
    accel: Option<&dyn VectorAccelerator>,
    threshold: usize,
    ef: usize,
) -> Result<Vec<Hit>> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Read)?;
    search_auto(
        &get_vector_set(loom, ns, name)?,
        query,
        k,
        filter,
        accel,
        threshold,
        ef,
    )
}

/// Top-`k` search of set `name` with an explicit accelerator policy. The exact policy is the
/// portable contract; approximate policies use the supplied accelerator only when their policy says
/// to do so.
#[allow(clippy::too_many_arguments)]
pub fn vector_search_with_policy<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    query: &[f32],
    k: usize,
    filter: &MetaFilter,
    accel: Option<&dyn VectorAccelerator>,
    policy: AcceleratorPolicy,
    ef: usize,
) -> Result<Vec<Hit>> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Read)?;
    search_with_policy(
        &get_vector_set(loom, ns, name)?,
        query,
        k,
        filter,
        accel,
        policy,
        ef,
    )
}

/// Top-`k` search with an explicit policy over the built-in wasm-clean PQ accelerator. Exact policy
/// keeps the portable exact path; approximate policy builds a derived PQ index from the current set
/// and uses it only above the supplied threshold.
#[allow(clippy::too_many_arguments)]
pub fn vector_search_with_pq_policy<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    query: &[f32],
    k: usize,
    filter: &MetaFilter,
    policy: AcceleratorPolicy,
    ef: usize,
    pq_m: usize,
    pq_k: usize,
    pq_iters: usize,
) -> Result<Vec<Hit>> {
    loom.authorize_collection(ns, FacetKind::Vector, name, AclRight::Read)?;
    let set = get_vector_set(loom, ns, name)?;
    let accel = match policy {
        AcceleratorPolicy::ExactAlways => None,
        AcceleratorPolicy::ApproximateAbove { threshold } if set.len() > threshold => {
            Some(PqIndex::build(&set, pq_m, pq_k, pq_iters)?)
        }
        AcceleratorPolicy::ApproximateAbove { .. } => None,
    };
    search_with_policy(
        &set,
        query,
        k,
        filter,
        accel.as_ref().map(|a| a as &dyn VectorAccelerator),
        policy,
        ef,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestEmbeddingProvider;

    impl crate::inference::TextEmbedding for TestEmbeddingProvider {
        fn model_id(&self) -> &str {
            "test-embed"
        }

        fn dimension(&self) -> usize {
            2
        }

        fn weights_digest(&self) -> Option<&str> {
            Some("sha256:test")
        }

        fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|text| vec![text.len() as f32, text.bytes().map(f32::from).sum()])
                .collect())
        }
    }

    fn set() -> VectorSet {
        let mut s = VectorSet::new(2, Metric::Cosine);
        let mut m = BTreeMap::new();
        m.insert("lang".to_string(), Value::Text("en".into()));
        s.upsert("a", vec![1.0, 0.0], m.clone()).unwrap();
        s.upsert("b", vec![0.0, 1.0], m).unwrap();
        let mut m2 = BTreeMap::new();
        m2.insert("lang".to_string(), Value::Text("fr".into()));
        s.upsert("c", vec![0.9, 0.1], m2).unwrap();
        s
    }

    #[test]
    fn vector_set_versions_with_commits() {
        use crate::provider::memory::MemoryStore;
        use crate::workspace::{FacetKind, WorkspaceId};

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Vector, None, WorkspaceId::from_bytes([7; 16]))
            .unwrap();
        let mut s = set(); // 3 vectors
        put_vector_set(&mut loom, ns, "emb", &s).unwrap();
        let c1 = loom.commit(ns, "nas", "three", 1).unwrap();

        s.upsert("d", vec![0.5, 0.5], BTreeMap::new()).unwrap();
        put_vector_set(&mut loom, ns, "emb", &s).unwrap();
        loom.commit(ns, "nas", "four", 2).unwrap();
        assert_eq!(get_vector_set(&loom, ns, "emb").unwrap().len(), 4);

        // Checking out the first commit restores the 3-vector set (index would be rebuilt).
        loom.checkout_commit(ns, c1).unwrap();
        assert_eq!(get_vector_set(&loom, ns, "emb").unwrap().len(), 3);
    }

    #[test]
    fn source_text_versions_and_raw_upsert_clears_it() {
        use crate::provider::memory::MemoryStore;
        use crate::workspace::{FacetKind, WorkspaceId};

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Vector, None, WorkspaceId::from_bytes([31; 16]))
            .unwrap();
        let embeddings = TextEmbeddingHandle::with_provider(Box::new(TestEmbeddingProvider));

        vector_create(&mut loom, ns, "emb", 2, Metric::Cosine).unwrap();
        vector_upsert_text(
            &mut loom,
            ns,
            "emb",
            "a",
            "alpha",
            BTreeMap::new(),
            &embeddings,
        )
        .unwrap();
        assert_eq!(
            vector_source_text(&loom, ns, "emb", "a")
                .unwrap()
                .as_deref(),
            Some("alpha")
        );
        assert_eq!(
            vector_embedding_model(&loom, ns, "emb").unwrap(),
            Some(TextEmbeddingModel::new(
                "test-embed",
                2,
                Some("sha256:test".to_string())
            ))
        );
        let c1 = loom.commit(ns, "nas", "source", 1).unwrap();

        vector_upsert(&mut loom, ns, "emb", "a", vec![1.0, 0.0], BTreeMap::new()).unwrap();
        assert_eq!(vector_source_text(&loom, ns, "emb", "a").unwrap(), None);
        loom.checkout_commit(ns, c1).unwrap();
        assert_eq!(
            vector_source_text(&loom, ns, "emb", "a")
                .unwrap()
                .as_deref(),
            Some("alpha")
        );
        assert!(vector_delete(&mut loom, ns, "emb", "a").unwrap());
        assert_eq!(vector_source_text(&loom, ns, "emb", "a").unwrap(), None);
    }

    #[test]
    fn source_aware_upsert_rejects_model_dimension_mismatch() {
        use crate::provider::memory::MemoryStore;
        use crate::workspace::{FacetKind, WorkspaceId};

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Vector, None, WorkspaceId::from_bytes([32; 16]))
            .unwrap();

        vector_create(&mut loom, ns, "emb", 3, Metric::Cosine).unwrap();
        let embeddings = TextEmbeddingHandle::with_provider(Box::new(TestEmbeddingProvider));
        let err = vector_upsert_text(
            &mut loom,
            ns,
            "emb",
            "a",
            "alpha",
            BTreeMap::new(),
            &embeddings,
        )
        .unwrap_err();
        assert_eq!(err.code, Code::DimensionMismatch);
    }

    #[test]
    fn vector_text_upsert_uses_activated_embedding_provider() {
        use crate::provider::memory::MemoryStore;
        use crate::workspace::{FacetKind, WorkspaceId};
        let embeddings = TextEmbeddingHandle::with_provider(Box::new(TestEmbeddingProvider));
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Vector, None, WorkspaceId::from_bytes([33; 16]))
            .unwrap();

        vector_create(&mut loom, ns, "emb", 2, Metric::Cosine).unwrap();
        vector_upsert_text(
            &mut loom,
            ns,
            "emb",
            "a",
            "alpha",
            BTreeMap::new(),
            &embeddings,
        )
        .unwrap();

        assert_eq!(
            vector_source_text(&loom, ns, "emb", "a")
                .unwrap()
                .as_deref(),
            Some("alpha")
        );
        assert_eq!(
            vector_embedding_model(&loom, ns, "emb").unwrap(),
            embeddings.model()
        );
    }

    #[test]
    fn vector_set_uses_structured_entry_files() {
        use crate::provider::memory::MemoryStore;
        use crate::workspace::{FacetKind, WorkspaceId};

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Vector, None, WorkspaceId::from_bytes([8; 16]))
            .unwrap();

        vector_create(&mut loom, ns, "emb", 2, Metric::Cosine).unwrap();
        vector_upsert(&mut loom, ns, "emb", "a/b", vec![1.0, 0.0], BTreeMap::new()).unwrap();
        vector_upsert(&mut loom, ns, "emb", "z", vec![0.0, 1.0], BTreeMap::new()).unwrap();

        let slash_id_path = entry_path("emb", "a/b");
        assert!(slash_id_path.starts_with(&entries_path("emb")));
        assert!(!slash_id_path.ends_with("a/b"));
        let before = loom.read_file_reserved(ns, &slash_id_path).unwrap();

        vector_upsert(&mut loom, ns, "emb", "z", vec![0.5, 0.5], BTreeMap::new()).unwrap();
        assert_eq!(loom.read_file_reserved(ns, &slash_id_path).unwrap(), before);

        assert!(vector_delete(&mut loom, ns, "emb", "z").unwrap());
        assert_eq!(
            loom.read_file_reserved(ns, &entry_path("emb", "z"))
                .unwrap_err()
                .code,
            Code::NotFound
        );
        assert_eq!(vector_ids(&loom, ns, "emb", None).unwrap(), vec!["a/b"]);
    }

    #[test]
    fn vector_set_persists_metadata_index_declarations() {
        use crate::provider::memory::MemoryStore;
        use crate::workspace::{FacetKind, WorkspaceId};

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Vector, None, WorkspaceId::from_bytes([18; 16]))
            .unwrap();
        let mut s = set();
        assert!(s.add_metadata_index("lang"));
        put_vector_set(&mut loom, ns, "emb", &s).unwrap();

        let loaded = get_vector_set(&loom, ns, "emb").unwrap();
        assert_eq!(loaded.metadata_indexes().collect::<Vec<_>>(), ["lang"]);
    }

    #[test]
    fn facade_create_upsert_search_delete() {
        use crate::provider::memory::MemoryStore;
        use crate::workspace::{FacetKind, WorkspaceId};

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Vector, None, WorkspaceId::from_bytes([9; 16]))
            .unwrap();
        // Operations require an explicit create (dimension and metric are fixed at creation).
        assert_eq!(
            vector_get(&loom, ns, "emb", "a").unwrap_err().code,
            Code::NotFound
        );
        vector_create(&mut loom, ns, "emb", 2, Metric::Cosine).unwrap();
        // Re-create is a conflict.
        assert_eq!(
            vector_create(&mut loom, ns, "emb", 2, Metric::Cosine)
                .unwrap_err()
                .code,
            Code::Conflict
        );
        let mut meta = BTreeMap::new();
        meta.insert("lang".to_string(), Value::Text("en".into()));
        vector_upsert(&mut loom, ns, "emb", "a", vec![1.0, 0.0], meta.clone()).unwrap();
        vector_upsert(&mut loom, ns, "emb", "b", vec![0.0, 1.0], meta).unwrap();
        // A wrong-width vector is a dimension mismatch.
        assert_eq!(
            vector_upsert(&mut loom, ns, "emb", "c", vec![1.0], BTreeMap::new())
                .unwrap_err()
                .code,
            Code::DimensionMismatch
        );
        let hits = vector_search(&loom, ns, "emb", &[1.0, 0.0], 2, &MetaFilter::All).unwrap();
        assert_eq!(hits[0].id, "a");
        assert_eq!(
            vector_ids(&loom, ns, "emb", None).unwrap(),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            vector_ids(&loom, ns, "emb", Some("b")).unwrap(),
            vec!["b".to_string()]
        );
        assert!(vector_delete(&mut loom, ns, "emb", "a").unwrap());
        assert!(!vector_delete(&mut loom, ns, "emb", "a").unwrap());
        assert_eq!(vector_get(&loom, ns, "emb", "a").unwrap(), None);
    }

    #[test]
    fn authenticated_vector_operations_honor_collection_scopes() {
        use crate::provider::memory::MemoryStore;
        use crate::workspace::{FacetKind, WorkspaceId};

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Vector, None, WorkspaceId::from_bytes([41; 16]))
            .unwrap();
        let root = WorkspaceId::from_bytes([1; 16]);
        let mut identity = crate::IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);
        loom.acl_store_mut()
            .grant(crate::AclGrant {
                subject: crate::AclSubject::Principal(root),
                workspace: Some(ns),
                domain: Some(FacetKind::Vector.into()),
                ref_glob: None,
                scopes: vec![crate::AclScope::Prefix {
                    kind: crate::AclScopeKind::Collection,
                    prefix: b"work".to_vec(),
                }],
                rights: [crate::AclRight::Write, crate::AclRight::Read]
                    .into_iter()
                    .collect(),
                effect: crate::AclEffect::Allow,
                predicate: None,
            })
            .unwrap();

        vector_create(&mut loom, ns, "work", 2, Metric::Cosine).unwrap();
        vector_upsert(&mut loom, ns, "work", "a", vec![1.0, 0.0], BTreeMap::new()).unwrap();
        assert!(vector_get(&loom, ns, "work", "a").unwrap().is_some());
        assert_eq!(
            vector_create(&mut loom, ns, "private", 2, Metric::Cosine)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
    }

    #[test]
    fn metadata_index_narrows_exact_search_candidates() {
        use crate::provider::memory::MemoryStore;
        use crate::workspace::{FacetKind, WorkspaceId};

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Vector, None, WorkspaceId::from_bytes([19; 16]))
            .unwrap();
        vector_create(&mut loom, ns, "emb", 2, Metric::Cosine).unwrap();
        let mut en = BTreeMap::new();
        en.insert("lang".to_string(), Value::Text("en".into()));
        let mut fr = BTreeMap::new();
        fr.insert("lang".to_string(), Value::Text("fr".into()));
        vector_upsert(&mut loom, ns, "emb", "a", vec![1.0, 0.0], en).unwrap();
        vector_upsert(&mut loom, ns, "emb", "b", vec![0.0, 1.0], fr).unwrap();
        assert!(vector_create_metadata_index(&mut loom, ns, "emb", "lang").unwrap());
        assert_eq!(
            vector_metadata_index_keys(&loom, ns, "emb").unwrap(),
            ["lang".to_string()]
        );

        loom.remove_file_reserved(ns, &entry_path("emb", "b"))
            .unwrap();
        let only_en = MetaFilter::Eq("lang".into(), Value::Text("en".into()));
        let hits = vector_search(&loom, ns, "emb", &[1.0, 0.0], 10, &only_en).unwrap();
        assert_eq!(
            hits.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(),
            ["a"]
        );
    }

    #[test]
    fn metadata_index_updates_on_upsert_delete_and_drop() {
        use crate::provider::memory::MemoryStore;
        use crate::workspace::{FacetKind, WorkspaceId};

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Vector, None, WorkspaceId::from_bytes([20; 16]))
            .unwrap();
        vector_create(&mut loom, ns, "emb", 2, Metric::Cosine).unwrap();
        assert!(vector_create_metadata_index(&mut loom, ns, "emb", "lang").unwrap());
        assert!(!vector_create_metadata_index(&mut loom, ns, "emb", "lang").unwrap());

        let mut en = BTreeMap::new();
        en.insert("lang".to_string(), Value::Text("en".into()));
        vector_upsert(&mut loom, ns, "emb", "a", vec![1.0, 0.0], en).unwrap();
        let en_filter = MetaFilter::Eq("lang".into(), Value::Text("en".into()));
        assert_eq!(
            vector_search(&loom, ns, "emb", &[1.0, 0.0], 10, &en_filter)
                .unwrap()
                .iter()
                .map(|h| h.id.as_str())
                .collect::<Vec<_>>(),
            ["a"]
        );

        let mut fr = BTreeMap::new();
        fr.insert("lang".to_string(), Value::Text("fr".into()));
        vector_upsert(&mut loom, ns, "emb", "a", vec![1.0, 0.0], fr).unwrap();
        let fr_filter = MetaFilter::Eq("lang".into(), Value::Text("fr".into()));
        assert!(
            vector_search(&loom, ns, "emb", &[1.0, 0.0], 10, &en_filter)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            vector_search(&loom, ns, "emb", &[1.0, 0.0], 10, &fr_filter)
                .unwrap()
                .iter()
                .map(|h| h.id.as_str())
                .collect::<Vec<_>>(),
            ["a"]
        );

        assert!(vector_delete(&mut loom, ns, "emb", "a").unwrap());
        assert!(
            vector_search(&loom, ns, "emb", &[1.0, 0.0], 10, &fr_filter)
                .unwrap()
                .is_empty()
        );
        assert!(vector_drop_metadata_index(&mut loom, ns, "emb", "lang").unwrap());
        assert!(!vector_drop_metadata_index(&mut loom, ns, "emb", "lang").unwrap());
        assert!(
            vector_metadata_index_keys(&loom, ns, "emb")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn facade_pq_accelerator_reconciles_with_exact() {
        use crate::provider::memory::MemoryStore;
        use crate::workspace::{FacetKind, WorkspaceId};

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Vector, None, WorkspaceId::from_bytes([10; 16]))
            .unwrap();
        vector_create(&mut loom, ns, "emb", 2, Metric::Cosine).unwrap();
        for (id, v) in [("a", [1.0, 0.0]), ("b", [0.0, 1.0]), ("c", [0.9, 0.1])] {
            vector_upsert(&mut loom, ns, "emb", id, v.to_vec(), BTreeMap::new()).unwrap();
        }
        // The derived PQ accelerator is built from the set (rebuildable; never stored).
        let pq = vector_build_pq_index(&loom, ns, "emb", 1, 2, 4).unwrap();
        let exact = vector_search(&loom, ns, "emb", &[1.0, 0.0], 3, &MetaFilter::All).unwrap();
        // threshold 0 forces the accelerator path; it reconciles to the exact order.
        let accel = vector_search_auto(
            &loom,
            ns,
            "emb",
            &[1.0, 0.0],
            3,
            &MetaFilter::All,
            Some(&pq),
            0,
            16,
        )
        .unwrap();
        assert_eq!(
            exact.iter().map(|h| &h.id).collect::<Vec<_>>(),
            accel.iter().map(|h| &h.id).collect::<Vec<_>>()
        );
        // No accelerator falls back to exact search.
        let none = vector_search_auto(
            &loom,
            ns,
            "emb",
            &[1.0, 0.0],
            3,
            &MetaFilter::All,
            None,
            0,
            16,
        )
        .unwrap();
        assert_eq!(none[0].id, "a");
        let forced_exact = vector_search_with_policy(
            &loom,
            ns,
            "emb",
            &[1.0, 0.0],
            3,
            &MetaFilter::All,
            Some(&pq),
            AcceleratorPolicy::ExactAlways,
            16,
        )
        .unwrap();
        assert_eq!(
            exact.iter().map(|h| &h.id).collect::<Vec<_>>(),
            forced_exact.iter().map(|h| &h.id).collect::<Vec<_>>()
        );
    }
}
