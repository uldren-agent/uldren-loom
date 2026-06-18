//! The WASM execution substrate.
//!
//! One engine runs a program against an in-memory file set under a fuel budget, enforcing the
//! program's grants; the choice is a build-time decision a program author never sees (both engines
//! implement the identical host ABI). wasmi (pure Rust) is the default and the only engine that
//! targets `wasm32`/browser; Wasmtime (native JIT) is selected on native builds via the
//! `engine-wasmtime` feature. The in-memory `run` host ABI (module `env`) is `file_write`,
//! `file_remove`, `file_read`, `input_get`. The real-state `run_state` path always uses wasmi because
//! it carries a borrowed [`crate::StateAccess`] through the host state. It extends the ABI with
//! state-backed files (`file_list`), versioned KV (`kv_put`, `kv_get`, `kv_delete`, `kv_len`,
//! `kv_scan`), documents (`doc_put`,
//! `doc_get`, `doc_delete`), append-only ledgers (`ledger_append`, `ledger_get`, `ledger_len`), and
//! content-addressed blobs (`cas_put`, `cas_get`, `cas_has`, `cas_delete`), append-only queue streams
//! (`queue_append`, `queue_get`, `queue_range` returning a canonical-CBOR array of byte strings in
//! sequence order, `queue_len`), graph reads (`graph_neighbors` returning a canonical-CBOR text array;
//! `graph_get_node` returning a canonical `[key, value]` property pair array; `graph_get_edge`
//! returning a canonical `[src, dst, label, props]` edge; `graph_out_edges`/`graph_in_edges` returning
//! a canonical array of `[edge_id, edge]` pairs), graph writes (`graph_upsert_node`,
//! `graph_upsert_edge` taking a property pair array the host decodes and rejects when malformed,
//! `graph_remove_node`, `graph_remove_edge`), graph queries (`graph_reachable` returning a
//! canonical-CBOR text array and `graph_shortest_path` returning that array or the absent-value
//! sentinel; both take `max_depth`/`via_label` as optional args where a negative value means None),
//! vectors (`vector_create` with a metric tag, `vector_upsert` taking raw little-endian `f32`
//! components plus a `[key, cell]` metadata array the host decodes, `vector_get` returning
//! `[vector_bytes, metadata]`, `vector_delete`, `vector_ids` returning a text array, and
//! `vector_search`/`vector_search_filtered` returning an `[id, score]` hit array), columnar
//! (`columnar_create` taking a `[name, type_tag]` schema array,
//! `columnar_append` taking a row of cells, `columnar_scan` returning an array of cell-array rows,
//! `columnar_columns` returning the schema array, `columnar_rows` returning the row count,
//! `columnar_select` returning rows, and `columnar_aggregate` returning cells), and time series
//! (`ts_put`, `ts_get`,
//! `ts_latest` returning a canonical `[timestamp, value]` point, `ts_range` returning a
//! canonical-CBOR array of those points in time order), and fails closed when authorization rejects a
//! host operation. Programs may emit ordered diagnostic lines through the `log` host call; capture is
//! bounded (see [`LogBuffer`]). The guest exports `memory` and `run`.

use std::collections::BTreeMap;

// wasmi is also the real-state engine because it can carry borrowed host state.
#[path = "engine_wasmi.rs"]
mod engine_wasmi;
#[cfg(all(not(target_arch = "wasm32"), feature = "engine-wasmtime"))]
#[path = "engine_wasmtime.rs"]
mod engine_wasmtime;

#[cfg(not(all(not(target_arch = "wasm32"), feature = "engine-wasmtime")))]
pub use engine_wasmi::run;
pub use engine_wasmi::run_state;
#[cfg(all(not(target_arch = "wasm32"), feature = "engine-wasmtime"))]
pub use engine_wasmtime::run;

/// A file set the program reads and writes: path -> content bytes (the files facet).
pub type FileSet = BTreeMap<String, Vec<u8>>;

/// The outcome of a metered program run.
pub struct RunResult {
    /// The file set after the program ran (only capability-permitted writes were applied).
    pub files: FileSet,
    /// Fuel consumed.
    pub fuel_used: u64,
}

/// The outcome of a `StateAccess`-backed [`run_state`]: fuel consumed and the ordered, bounded log
/// lines the program emitted through the `log` host call.
pub struct RunOutcome {
    /// Fuel consumed.
    pub fuel_used: u64,
    /// Diagnostic log lines in the order the program emitted them, bounded by [`LogBuffer`].
    pub logs: Vec<String>,
}

/// The maximum number of log entries captured from one run; entries beyond this are dropped.
pub(crate) const MAX_LOG_ENTRIES: usize = 256;
/// The maximum total log bytes captured from one run; an entry that would exceed this is dropped.
pub(crate) const MAX_LOG_BYTES: usize = 64 * 1024;

/// Encode a bounded `kv_scan` result as the wire form the host ABI returns through the single output
/// buffer: a canonical Loom CBOR array of `[key_cbor_bytes, value_bytes]` pairs, in `Value` order. Each
/// key is its `key_to_cbor` cell form, so the guest reads back the same typed key bytes it would pass to
/// `kv_get`. One shared encoder keeps the wire form identical across every engine that serves `run_state`.
pub(crate) fn encode_scan_entries(entries: &[(loom_core::tabular::Value, Vec<u8>)]) -> Vec<u8> {
    use loom_codec::Value as Cbor;
    let items = entries
        .iter()
        .map(|(key, value)| {
            Cbor::Array(vec![
                Cbor::Bytes(loom_core::key_to_cbor(key)),
                Cbor::Bytes(value.clone()),
            ])
        })
        .collect();
    loom_codec::encode(&Cbor::Array(items)).expect("a canonical CBOR array of byte strings encodes")
}

/// Encode a list of strings as a canonical Loom CBOR text array - the wire form structured
/// list-returning host functions (e.g. `graph_neighbors`) return through the single output buffer.
pub(crate) fn encode_string_list(items: &[String]) -> Vec<u8> {
    use loom_codec::Value as Cbor;
    let arr = items.iter().map(|s| Cbor::Text(s.clone())).collect();
    loom_codec::encode(&Cbor::Array(arr)).expect("a canonical CBOR array of text encodes")
}

fn file_kind_tag(kind: loom_core::FileKind) -> u64 {
    match kind {
        loom_core::FileKind::File => 0,
        loom_core::FileKind::Directory => 1,
        loom_core::FileKind::Symlink => 2,
    }
}

/// Encode directory entries as canonical `[name, kind_tag]` pairs. Kind tags are `0=file`,
/// `1=directory`, and `2=symlink`.
pub(crate) fn encode_dir_entries(entries: &[loom_core::DirEntry]) -> Vec<u8> {
    use loom_codec::Value as Cbor;
    let arr = entries
        .iter()
        .map(|entry| {
            Cbor::Array(vec![
                Cbor::Text(entry.name.clone()),
                Cbor::Uint(file_kind_tag(entry.kind)),
            ])
        })
        .collect();
    loom_codec::encode(&Cbor::Array(arr)).expect("a canonical CBOR directory listing encodes")
}

/// Encode a time-series point as the canonical Loom CBOR pair `[timestamp, value]` the `ts_latest`
/// host function returns through the single output buffer.
pub(crate) fn encode_ts_point(timestamp: i64, value: &[u8]) -> Vec<u8> {
    use loom_codec::Value as Cbor;
    loom_codec::encode(&Cbor::Array(vec![
        Cbor::int(timestamp),
        Cbor::Bytes(value.to_vec()),
    ]))
    .expect("a canonical CBOR time-series point encodes")
}

/// Encode a time-series range result as a canonical Loom CBOR array of `[timestamp, value]` points, in
/// time order - the wire form `ts_range` returns through the single output buffer. Each element uses
/// the same point shape as [`encode_ts_point`], so a guest decodes one array of the pairs it already
/// knows from `ts_latest`.
pub(crate) fn encode_ts_points(points: &[(i64, Vec<u8>)]) -> Vec<u8> {
    use loom_codec::Value as Cbor;
    let items = points
        .iter()
        .map(|(ts, value)| Cbor::Array(vec![Cbor::int(*ts), Cbor::Bytes(value.clone())]))
        .collect();
    loom_codec::encode(&Cbor::Array(items)).expect("a canonical CBOR time-series range encodes")
}

/// Encode a list of opaque byte entries as a canonical Loom CBOR array of byte strings - the wire form
/// `queue_range` returns through the single output buffer, in sequence order. One shared encoder keeps
/// the wire form identical across every engine that serves `run_state`.
pub(crate) fn encode_bytes_list(items: &[Vec<u8>]) -> Vec<u8> {
    use loom_codec::Value as Cbor;
    let arr = items.iter().map(|b| Cbor::Bytes(b.clone())).collect();
    loom_codec::encode(&Cbor::Array(arr)).expect("a canonical CBOR array of byte strings encodes")
}

fn graph_value_to_cbor(value: &loom_core::GraphValue) -> loom_codec::Value {
    use loom_codec::Value as Cbor;
    match value {
        loom_core::GraphValue::Null => Cbor::Null,
        loom_core::GraphValue::Bool(value) => Cbor::Bool(*value),
        loom_core::GraphValue::Int(value) => Cbor::int(*value),
        loom_core::GraphValue::Float(value) => Cbor::Float(*value),
        loom_core::GraphValue::Text(value) => Cbor::Text(value.clone()),
        loom_core::GraphValue::Bytes(value) => Cbor::Bytes(value.clone()),
        loom_core::GraphValue::List(values) => {
            Cbor::Array(values.iter().map(graph_value_to_cbor).collect())
        }
        loom_core::GraphValue::Map(values) => Cbor::Map(
            values
                .iter()
                .map(|(key, value)| (Cbor::Text(key.clone()), graph_value_to_cbor(value)))
                .collect(),
        ),
        loom_core::GraphValue::Geometry(value) => graph_geometry_to_cbor(value),
    }
}

fn graph_value_from_cbor(value: loom_codec::Value) -> Option<loom_core::GraphValue> {
    use loom_codec::Value as Cbor;
    match value {
        Cbor::Null => Some(loom_core::GraphValue::Null),
        Cbor::Bool(value) => Some(loom_core::GraphValue::Bool(value)),
        Cbor::Uint(value) => i64::try_from(value).ok().map(loom_core::GraphValue::Int),
        Cbor::Nint(value) => i64::try_from(value)
            .ok()
            .map(|value| loom_core::GraphValue::Int(-1 - value)),
        Cbor::Float(value) if value.is_finite() => Some(loom_core::GraphValue::Float(value)),
        Cbor::Text(value) => Some(loom_core::GraphValue::Text(value)),
        Cbor::Bytes(value) => Some(loom_core::GraphValue::Bytes(value)),
        Cbor::Array(values) if cbor_array_has_geometry_tag(&values) => {
            graph_geometry_from_cbor(values).map(loom_core::GraphValue::Geometry)
        }
        Cbor::Array(values) => values
            .into_iter()
            .map(graph_value_from_cbor)
            .collect::<Option<Vec<_>>>()
            .map(loom_core::GraphValue::List),
        Cbor::Map(pairs) => {
            let mut values = std::collections::BTreeMap::new();
            for (key, value) in pairs {
                let Cbor::Text(key) = key else {
                    return None;
                };
                values.insert(key, graph_value_from_cbor(value)?);
            }
            Some(loom_core::GraphValue::Map(values))
        }
        _ => None,
    }
}

fn graph_geometry_to_cbor(value: &loom_core::GraphGeometry) -> loom_codec::Value {
    use loom_codec::Value as Cbor;
    match value {
        loom_core::GraphGeometry::Point(point) => Cbor::Array(vec![
            Cbor::Text(loom_core::GRAPH_GEOMETRY_TAG.to_string()),
            Cbor::Text("point".to_string()),
            Cbor::Text(point.crs.as_str().to_string()),
            Cbor::Float(point.x),
            Cbor::Float(point.y),
            point.z.map(Cbor::Float).unwrap_or(Cbor::Null),
        ]),
    }
}

fn graph_geometry_from_cbor(values: Vec<loom_codec::Value>) -> Option<loom_core::GraphGeometry> {
    use loom_codec::Value as Cbor;
    let [tag, kind, crs, x, y, z]: [Cbor; 6] = values.try_into().ok()?;
    if cbor_text(tag)? != loom_core::GRAPH_GEOMETRY_TAG {
        return None;
    }
    match cbor_text(kind)?.as_str() {
        "point" => {
            let crs = loom_core::GraphCrs::parse(&cbor_text(crs)?).ok()?;
            let x = cbor_finite_float(x)?;
            let y = cbor_finite_float(y)?;
            let z = match z {
                Cbor::Null => None,
                other => Some(cbor_finite_float(other)?),
            };
            loom_core::GraphGeometry::point(crs, x, y, z).ok()
        }
        _ => None,
    }
}

fn cbor_array_has_geometry_tag(values: &[loom_codec::Value]) -> bool {
    matches!(values.first(), Some(loom_codec::Value::Text(tag)) if tag == loom_core::GRAPH_GEOMETRY_TAG)
}

fn cbor_text(value: loom_codec::Value) -> Option<String> {
    match value {
        loom_codec::Value::Text(value) => Some(value),
        _ => None,
    }
}

fn cbor_finite_float(value: loom_codec::Value) -> Option<f64> {
    match value {
        loom_codec::Value::Float(value) if value.is_finite() => Some(value),
        loom_codec::Value::Uint(value) => Some(value as f64),
        loom_codec::Value::Nint(value) => Some(-1.0 - value as f64),
        _ => None,
    }
}

/// A graph property map rendered as a canonical Loom CBOR value: an array of `[key_text, value_scalar]`
/// pairs in key order. `Props` is a `BTreeMap`, so iteration order is already the canonical key order;
/// the pair-array framing matches `kv_scan` rather than a CBOR map so the wire form never depends on
/// map-key canonicalization.
fn props_value(props: &loom_core::graph::Props) -> loom_codec::Value {
    use loom_codec::Value as Cbor;
    Cbor::Array(
        props
            .iter()
            .map(|(k, v)| Cbor::Array(vec![Cbor::Text(k.clone()), graph_value_to_cbor(v)]))
            .collect(),
    )
}

/// A graph edge rendered as the canonical Loom CBOR array `[src, dst, label, props]`, where `props` is
/// the pair array from [`props_value`]. Shared by `graph_get_edge` and the edge-list encoders so a
/// single edge shape travels the wire everywhere.
fn edge_value(edge: &loom_core::graph::Edge) -> loom_codec::Value {
    use loom_codec::Value as Cbor;
    Cbor::Array(vec![
        Cbor::Text(edge.src.clone()),
        Cbor::Text(edge.dst.clone()),
        Cbor::Text(edge.label.clone()),
        props_value(&edge.props),
    ])
}

/// Encode a graph node's properties as the canonical Loom CBOR pair array - the wire form
/// `graph_get_node` returns through the single output buffer.
pub(crate) fn encode_props(props: &loom_core::graph::Props) -> Vec<u8> {
    loom_codec::encode(&props_value(props)).expect("a canonical CBOR property map encodes")
}

/// Encode a graph edge as the canonical Loom CBOR `[src, dst, label, props]` array - the wire form
/// `graph_get_edge` returns through the single output buffer.
pub(crate) fn encode_edge(edge: &loom_core::graph::Edge) -> Vec<u8> {
    loom_codec::encode(&edge_value(edge)).expect("a canonical CBOR edge encodes")
}

/// Encode an `(edge_id, edge)` list as a canonical Loom CBOR array of `[edge_id_text, edge_array]`
/// pairs, in the order the source returns them - the wire form `graph_out_edges` and `graph_in_edges`
/// return through the single output buffer. Each `edge_array` is the shape [`encode_edge`] produces.
pub(crate) fn encode_edge_list(edges: &[(String, loom_core::graph::Edge)]) -> Vec<u8> {
    use loom_codec::Value as Cbor;
    let items = edges
        .iter()
        .map(|(id, edge)| Cbor::Array(vec![Cbor::Text(id.clone()), edge_value(edge)]))
        .collect();
    loom_codec::encode(&Cbor::Array(items)).expect("a canonical CBOR edge list encodes")
}

/// Decode a graph property map from the same canonical Loom CBOR pair array [`encode_props`] produces:
/// an array of `[key_text, value_scalar]` pairs. Returns `None` if the bytes are not exactly that shape,
/// so a host call can trap a malformed guest input rather than silently accept it. This is the guest's
/// input path for `graph_upsert_node`/`graph_upsert_edge`; a later duplicate key keeps the last value.
pub(crate) fn decode_props(bytes: &[u8]) -> Option<loom_core::graph::Props> {
    use loom_codec::Value as Cbor;
    let Ok(Cbor::Array(items)) = loom_codec::decode(bytes) else {
        return None;
    };
    let mut props = loom_core::graph::Props::new();
    for item in items {
        let Cbor::Array(mut pair) = item else {
            return None;
        };
        if pair.len() != 2 {
            return None;
        }
        // pop yields value then key (the pair is `[key, value]`).
        let value = graph_value_from_cbor(pair.pop()?)?;
        let key = match pair.pop() {
            Some(Cbor::Text(k)) => k,
            _ => return None,
        };
        props.insert(key, value);
    }
    Some(props)
}

/// Encode an `f32` vector as raw little-endian IEEE-754 bytes (4 bytes per component) - the wire form
/// vector components travel in for `vector_upsert`/`vector_search` inputs and inside the `vector_get`
/// result. This mirrors the loom-core on-disk vector encoding, so it is lossless and endian-stable.
pub(crate) fn encode_f32_vec(vector: &[f32]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(vector.len() * 4);
    for f in vector {
        raw.extend_from_slice(&f.to_bits().to_le_bytes());
    }
    raw
}

/// Decode a raw little-endian `f32` vector produced by [`encode_f32_vec`]; `None` if the byte length is
/// not a multiple of 4, so a host call can trap malformed guest input.
pub(crate) fn decode_f32_vec(raw: &[u8]) -> Option<Vec<f32>> {
    if !raw.len().is_multiple_of(4) {
        return None;
    }
    Some(
        raw.chunks_exact(4)
            .map(|c| f32::from_bits(u32::from_le_bytes([c[0], c[1], c[2], c[3]])))
            .collect(),
    )
}

/// Vector metadata as a canonical Loom CBOR value: an array of `[key_text, cell_value]` pairs in key
/// order, where each value uses the shared `tabular::Value` cell codec. `BTreeMap` iteration gives the
/// canonical key order; the pair-array framing avoids depending on CBOR map-key canonicalization.
fn meta_value(meta: &BTreeMap<String, loom_core::tabular::Value>) -> loom_codec::Value {
    use loom_codec::Value as Cbor;
    Cbor::Array(
        meta.iter()
            .map(|(k, v)| {
                Cbor::Array(vec![
                    Cbor::Text(k.clone()),
                    loom_core::tabular::cell_value(v),
                ])
            })
            .collect(),
    )
}

/// Encode vector metadata as the canonical pair array `meta_value` produces.
#[cfg(test)]
pub(crate) fn encode_meta(meta: &BTreeMap<String, loom_core::tabular::Value>) -> Vec<u8> {
    loom_codec::encode(&meta_value(meta)).expect("canonical CBOR vector metadata encodes")
}

/// Decode vector metadata from the canonical pair array, using the shared cell codec for values.
pub(crate) fn decode_meta(bytes: &[u8]) -> Option<BTreeMap<String, loom_core::tabular::Value>> {
    use loom_codec::Value as Cbor;
    let Ok(Cbor::Array(items)) = loom_codec::decode(bytes) else {
        return None;
    };
    let mut meta = BTreeMap::new();
    for item in items {
        let Cbor::Array(mut pair) = item else {
            return None;
        };
        if pair.len() != 2 {
            return None;
        }
        let value_cbor = pair.pop()?;
        let key = match pair.pop() {
            Some(Cbor::Text(k)) => k,
            _ => return None,
        };
        let value = loom_core::tabular::cell_from(value_cbor).ok()?;
        meta.insert(key, value);
    }
    Some(meta)
}

/// Encode a stored vector entry as the canonical Loom CBOR array `[vector_bytes, metadata]`, where
/// `vector_bytes` is the raw little-endian form from [`encode_f32_vec`] and `metadata` is the pair
/// array from [`meta_value`] - the wire form `vector_get` returns through the single output buffer.
pub(crate) fn encode_vector_entry(
    vector: &[f32],
    meta: &BTreeMap<String, loom_core::tabular::Value>,
) -> Vec<u8> {
    use loom_codec::Value as Cbor;
    loom_codec::encode(&Cbor::Array(vec![
        Cbor::Bytes(encode_f32_vec(vector)),
        meta_value(meta),
    ]))
    .expect("canonical CBOR vector entry encodes")
}

/// Encode vector search results as a canonical Loom CBOR array of `[id_text, score_float]` pairs, in
/// result order - the wire form `vector_search` returns through the single output buffer.
pub(crate) fn encode_hits(hits: &[loom_core::vector::Hit]) -> Vec<u8> {
    use loom_codec::Value as Cbor;
    let items = hits
        .iter()
        .map(|h| {
            Cbor::Array(vec![
                Cbor::Text(h.id.clone()),
                Cbor::Float(f64::from(h.score)),
            ])
        })
        .collect();
    loom_codec::encode(&Cbor::Array(items)).expect("canonical CBOR vector hits encode")
}

fn decode_vector_filter_value(value: loom_codec::Value) -> Option<loom_core::MetaFilter> {
    use loom_codec::Value as Cbor;
    let Cbor::Array(items) = value else {
        return None;
    };
    let mut iter = items.into_iter();
    let tag = match iter.next()? {
        Cbor::Uint(tag) => tag,
        _ => return None,
    };
    Some(match tag {
        0 => loom_core::MetaFilter::All,
        1 => {
            let key = match iter.next()? {
                Cbor::Text(key) => key,
                _ => return None,
            };
            loom_core::MetaFilter::Eq(key, loom_core::tabular::cell_from(iter.next()?).ok()?)
        }
        2 => loom_core::MetaFilter::And(
            Box::new(decode_vector_filter_value(iter.next()?)?),
            Box::new(decode_vector_filter_value(iter.next()?)?),
        ),
        3 => loom_core::MetaFilter::Ne(
            match iter.next()? {
                Cbor::Text(key) => key,
                _ => return None,
            },
            loom_core::tabular::cell_from(iter.next()?).ok()?,
        ),
        4 => loom_core::MetaFilter::Lt(
            match iter.next()? {
                Cbor::Text(key) => key,
                _ => return None,
            },
            loom_core::tabular::cell_from(iter.next()?).ok()?,
        ),
        5 => loom_core::MetaFilter::Le(
            match iter.next()? {
                Cbor::Text(key) => key,
                _ => return None,
            },
            loom_core::tabular::cell_from(iter.next()?).ok()?,
        ),
        6 => loom_core::MetaFilter::Gt(
            match iter.next()? {
                Cbor::Text(key) => key,
                _ => return None,
            },
            loom_core::tabular::cell_from(iter.next()?).ok()?,
        ),
        7 => loom_core::MetaFilter::Ge(
            match iter.next()? {
                Cbor::Text(key) => key,
                _ => return None,
            },
            loom_core::tabular::cell_from(iter.next()?).ok()?,
        ),
        8 => {
            let key = match iter.next()? {
                Cbor::Text(key) => key,
                _ => return None,
            };
            let Cbor::Array(values) = iter.next()? else {
                return None;
            };
            loom_core::MetaFilter::In(
                key,
                values
                    .into_iter()
                    .map(loom_core::tabular::cell_from)
                    .collect::<loom_core::Result<Vec<_>>>()
                    .ok()?,
            )
        }
        9 => loom_core::MetaFilter::Exists(match iter.next()? {
            Cbor::Text(key) => key,
            _ => return None,
        }),
        10 => loom_core::MetaFilter::Or(
            Box::new(decode_vector_filter_value(iter.next()?)?),
            Box::new(decode_vector_filter_value(iter.next()?)?),
        ),
        11 => loom_core::MetaFilter::Not(Box::new(decode_vector_filter_value(iter.next()?)?)),
        _ => return None,
    })
}

pub(crate) fn decode_vector_filter(bytes: &[u8]) -> Option<loom_core::MetaFilter> {
    if bytes.is_empty() {
        return Some(loom_core::MetaFilter::All);
    }
    decode_vector_filter_value(loom_codec::decode(bytes).ok()?)
}

/// A tabular row as a canonical Loom CBOR array of cells (each via the shared `tabular::Value` cell
/// codec) - the shape a columnar row travels in for `columnar_append` and inside scan results.
fn row_value(row: &[loom_core::tabular::Value]) -> loom_codec::Value {
    loom_codec::Value::Array(row.iter().map(loom_core::tabular::cell_value).collect())
}

/// Decode a tabular row from the cell array [`row_value`] produces; `None` if the bytes are not an
/// array or a cell fails to decode, so a host call can trap malformed guest input.
pub(crate) fn decode_row(bytes: &[u8]) -> Option<Vec<loom_core::tabular::Value>> {
    let Ok(loom_codec::Value::Array(items)) = loom_codec::decode(bytes) else {
        return None;
    };
    let mut row = Vec::with_capacity(items.len());
    for item in items {
        row.push(loom_core::tabular::cell_from(item).ok()?);
    }
    Some(row)
}

/// Encode a set of rows as a canonical Loom CBOR array of cell arrays, in row order - the wire form
/// `columnar_scan` returns through the single output buffer.
pub(crate) fn encode_rows(rows: &[Vec<loom_core::tabular::Value>]) -> Vec<u8> {
    let items = rows.iter().map(|r| row_value(r)).collect();
    loom_codec::encode(&loom_codec::Value::Array(items))
        .expect("canonical CBOR columnar rows encode")
}

/// Encode a columnar schema as a canonical Loom CBOR array of `[name_text, type_tag]` pairs, in column
/// order - the wire form `columnar_columns` returns through the single output buffer.
pub(crate) fn encode_columns(columns: &[(String, loom_core::tabular::ColumnType)]) -> Vec<u8> {
    use loom_codec::Value as Cbor;
    let items = columns
        .iter()
        .map(|(name, ty)| {
            Cbor::Array(vec![
                Cbor::Text(name.clone()),
                Cbor::Uint(u64::from(ty.tag())),
            ])
        })
        .collect();
    loom_codec::encode(&Cbor::Array(items)).expect("canonical CBOR columnar schema encodes")
}

/// Decode a columnar schema from the `[name_text, type_tag]` pair array [`encode_columns`] produces;
/// `None` if the shape or any type tag is invalid, so `columnar_create` can trap malformed input.
pub(crate) fn decode_columns(
    bytes: &[u8],
) -> Option<Vec<(String, loom_core::tabular::ColumnType)>> {
    use loom_codec::Value as Cbor;
    let Ok(Cbor::Array(items)) = loom_codec::decode(bytes) else {
        return None;
    };
    let mut columns = Vec::with_capacity(items.len());
    for item in items {
        let Cbor::Array(mut pair) = item else {
            return None;
        };
        if pair.len() != 2 {
            return None;
        }
        let tag = match pair.pop() {
            Some(Cbor::Uint(t)) => u8::try_from(t).ok()?,
            _ => return None,
        };
        let name = match pair.pop() {
            Some(Cbor::Text(n)) => n,
            _ => return None,
        };
        let ty = loom_core::tabular::ColumnType::from_tag(tag).ok()?;
        columns.push((name, ty));
    }
    Some(columns)
}

pub(crate) fn decode_columnar_select_columns(bytes: &[u8]) -> Option<Vec<String>> {
    let Ok(loom_codec::Value::Array(items)) = loom_codec::decode(bytes) else {
        return None;
    };
    items
        .into_iter()
        .map(|item| match item {
            loom_codec::Value::Text(name) => Some(name),
            _ => None,
        })
        .collect()
}

fn decode_cmp_op(tag: u64) -> Option<loom_core::tabular::CmpOp> {
    match tag {
        0 => Some(loom_core::tabular::CmpOp::Eq),
        1 => Some(loom_core::tabular::CmpOp::Ne),
        2 => Some(loom_core::tabular::CmpOp::Lt),
        3 => Some(loom_core::tabular::CmpOp::Le),
        4 => Some(loom_core::tabular::CmpOp::Gt),
        5 => Some(loom_core::tabular::CmpOp::Ge),
        _ => None,
    }
}

pub(crate) fn decode_columnar_filter(
    bytes: &[u8],
) -> Option<Option<(String, loom_core::tabular::CmpOp, loom_core::tabular::Value)>> {
    if bytes.is_empty() {
        return Some(None);
    }
    let Ok(loom_codec::Value::Array(items)) = loom_codec::decode(bytes) else {
        return None;
    };
    if items.is_empty() {
        return Some(None);
    }
    let mut iter = items.into_iter();
    let column = match iter.next()? {
        loom_codec::Value::Text(column) => column,
        _ => return None,
    };
    let op = match iter.next()? {
        loom_codec::Value::Uint(tag) => decode_cmp_op(tag)?,
        _ => return None,
    };
    let value = loom_core::tabular::cell_from(iter.next()?).ok()?;
    if iter.next().is_some() {
        return None;
    }
    Some(Some((column, op, value)))
}

fn decode_columnar_aggregate_op(tag: u64) -> Option<loom_core::ColumnarAggregateOp> {
    match tag {
        0 => Some(loom_core::ColumnarAggregateOp::Count),
        1 => Some(loom_core::ColumnarAggregateOp::CountNonNull),
        2 => Some(loom_core::ColumnarAggregateOp::Min),
        3 => Some(loom_core::ColumnarAggregateOp::Max),
        4 => Some(loom_core::ColumnarAggregateOp::Sum),
        _ => None,
    }
}

pub(crate) fn decode_columnar_aggregates(
    bytes: &[u8],
) -> Option<Vec<loom_core::ColumnarAggregate>> {
    let Ok(loom_codec::Value::Array(items)) = loom_codec::decode(bytes) else {
        return None;
    };
    items
        .into_iter()
        .map(|item| {
            let loom_codec::Value::Array(fields) = item else {
                return None;
            };
            let mut iter = fields.into_iter();
            let op = match iter.next()? {
                loom_codec::Value::Uint(tag) => decode_columnar_aggregate_op(tag)?,
                _ => return None,
            };
            let column = match iter.next() {
                Some(loom_codec::Value::Text(column)) => Some(column),
                Some(loom_codec::Value::Null) | None => None,
                _ => return None,
            };
            if iter.next().is_some() {
                return None;
            }
            Some(loom_core::ColumnarAggregate { op, column })
        })
        .collect()
}

/// An ordered, bounded buffer of program log lines. Bounds are enforced on push, so capture never
/// grows without limit regardless of the guest's behavior, and ordering is preserved.
#[derive(Default)]
pub(crate) struct LogBuffer {
    entries: Vec<String>,
    total_bytes: usize,
}

impl LogBuffer {
    /// Append `line` unless it would exceed the entry-count or total-byte bound, in which case it is
    /// dropped (the run is unaffected; logging is best-effort).
    pub(crate) fn push(&mut self, line: String) {
        if self.entries.len() >= MAX_LOG_ENTRIES {
            return;
        }
        if self.total_bytes.saturating_add(line.len()) > MAX_LOG_BYTES {
            return;
        }
        self.total_bytes += line.len();
        self.entries.push(line);
    }

    /// Consume the buffer into its ordered entries.
    pub(crate) fn into_entries(self) -> Vec<String> {
        self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_ts_points_preserves_order_and_point_shape() {
        use loom_codec::Value as Cbor;
        // A negative timestamp exercises the Nint arm; caller supplies time order.
        let points = vec![(-5i64, vec![]), (1, vec![10u8]), (2, vec![20u8, 21])];
        let decoded = loom_codec::decode(&encode_ts_points(&points)).expect("range decodes");
        let Cbor::Array(items) = decoded else {
            panic!("ts_range wire form is a CBOR array");
        };
        assert_eq!(items.len(), 3);
        assert_eq!(
            items[0],
            Cbor::Array(vec![Cbor::int(-5), Cbor::Bytes(vec![])])
        );
        assert_eq!(
            items[2],
            Cbor::Array(vec![Cbor::int(2), Cbor::Bytes(vec![20, 21])])
        );
        // The empty range is a valid empty array.
        assert_eq!(
            loom_codec::decode(&encode_ts_points(&[])).unwrap(),
            Cbor::Array(vec![])
        );
    }

    #[test]
    fn encode_bytes_list_preserves_order() {
        use loom_codec::Value as Cbor;
        let items = vec![vec![1u8, 2], vec![], vec![9u8]];
        let decoded = loom_codec::decode(&encode_bytes_list(&items)).expect("list decodes");
        assert_eq!(
            decoded,
            Cbor::Array(vec![
                Cbor::Bytes(vec![1, 2]),
                Cbor::Bytes(vec![]),
                Cbor::Bytes(vec![9]),
            ])
        );
        assert_eq!(
            loom_codec::decode(&encode_bytes_list(&[])).unwrap(),
            Cbor::Array(vec![])
        );
    }

    #[test]
    fn encode_dir_entries_uses_stable_kind_tags() {
        use loom_codec::Value as Cbor;
        let entries = vec![
            loom_core::DirEntry {
                name: "file.txt".to_string(),
                kind: loom_core::FileKind::File,
            },
            loom_core::DirEntry {
                name: "sub".to_string(),
                kind: loom_core::FileKind::Directory,
            },
        ];
        assert_eq!(
            loom_codec::decode(&encode_dir_entries(&entries)).expect("dir entries decode"),
            Cbor::Array(vec![
                Cbor::Array(vec![Cbor::Text("file.txt".to_string()), Cbor::Uint(0)]),
                Cbor::Array(vec![Cbor::Text("sub".to_string()), Cbor::Uint(1)]),
            ])
        );
    }

    #[test]
    fn encode_props_is_a_key_ordered_pair_array() {
        use loom_codec::Value as Cbor;
        // Inserted out of order; the `BTreeMap` yields canonical key order ("a" before "b").
        let mut props = loom_core::graph::Props::new();
        props.insert("b".to_string(), loom_core::GraphValue::Bytes(vec![2u8]));
        props.insert("a".to_string(), loom_core::GraphValue::Int(1));
        assert_eq!(
            loom_codec::decode(&encode_props(&props)).expect("props decode"),
            Cbor::Array(vec![
                Cbor::Array(vec![Cbor::Text("a".to_string()), Cbor::Uint(1)]),
                Cbor::Array(vec![Cbor::Text("b".to_string()), Cbor::Bytes(vec![2])]),
            ])
        );
    }

    #[test]
    fn encode_edge_and_edge_list_share_the_edge_shape() {
        use loom_codec::Value as Cbor;
        let mut props = loom_core::graph::Props::new();
        props.insert(
            "k".to_string(),
            loom_core::GraphValue::Text("v".to_string()),
        );
        let edge = loom_core::graph::Edge {
            src: "n1".to_string(),
            dst: "n2".to_string(),
            label: "rel".to_string(),
            props,
        };
        let edge_val = Cbor::Array(vec![
            Cbor::Text("n1".to_string()),
            Cbor::Text("n2".to_string()),
            Cbor::Text("rel".to_string()),
            Cbor::Array(vec![Cbor::Array(vec![
                Cbor::Text("k".to_string()),
                Cbor::Text("v".to_string()),
            ])]),
        ]);
        assert_eq!(
            loom_codec::decode(&encode_edge(&edge)).expect("edge decode"),
            edge_val.clone()
        );
        // The edge-list wire form wraps each edge as `[edge_id, edge]`, reusing the same edge shape.
        let list = vec![("e1".to_string(), edge)];
        assert_eq!(
            loom_codec::decode(&encode_edge_list(&list)).expect("edge list decode"),
            Cbor::Array(vec![Cbor::Array(vec![
                Cbor::Text("e1".to_string()),
                edge_val
            ])])
        );
    }

    #[test]
    fn decode_props_round_trips_and_rejects_malformed() {
        use loom_codec::Value as Cbor;
        let mut props = loom_core::graph::Props::new();
        props.insert("a".to_string(), loom_core::GraphValue::Bool(true));
        props.insert("b".to_string(), loom_core::GraphValue::Bytes(vec![]));
        // encode -> decode is the identity on a valid property map.
        assert_eq!(decode_props(&encode_props(&props)), Some(props));
        // Not canonical CBOR at all.
        assert_eq!(decode_props(&[0xFF]), None);
        // An element that is not a 2-tuple.
        let short = loom_codec::encode(&Cbor::Array(vec![Cbor::Array(vec![Cbor::Text(
            "x".to_string(),
        )])]))
        .unwrap();
        assert_eq!(decode_props(&short), None);
        // A pair whose value is not a graph value.
        let wrong_value = loom_codec::encode(&Cbor::Array(vec![Cbor::Array(vec![
            Cbor::Text("x".to_string()),
            Cbor::Map(vec![(Cbor::Uint(1), Cbor::Bool(true))]),
        ])]))
        .unwrap();
        assert_eq!(decode_props(&wrong_value), None);
    }

    #[test]
    fn f32_vec_round_trips_raw_little_endian() {
        let v = vec![1.0f32, -2.5, 0.0];
        assert_eq!(decode_f32_vec(&encode_f32_vec(&v)), Some(v));
        // A byte length that is not a multiple of 4 is rejected.
        assert_eq!(decode_f32_vec(&[0u8, 1, 2]), None);
    }

    #[test]
    fn vector_meta_round_trips_via_cell_codec() {
        use loom_core::tabular::Value;
        let mut meta = BTreeMap::new();
        meta.insert("a".to_string(), Value::Int(7));
        meta.insert("b".to_string(), Value::Text("x".to_string()));
        assert_eq!(decode_meta(&encode_meta(&meta)), Some(meta));
        assert_eq!(decode_meta(&[0xFF]), None);
    }

    #[test]
    fn encode_hits_is_id_score_pairs() {
        use loom_codec::Value as Cbor;
        let hits = vec![
            loom_core::vector::Hit {
                id: "a".to_string(),
                score: 1.0,
            },
            loom_core::vector::Hit {
                id: "b".to_string(),
                score: 0.5,
            },
        ];
        assert_eq!(
            loom_codec::decode(&encode_hits(&hits)).expect("hits decode"),
            Cbor::Array(vec![
                Cbor::Array(vec![Cbor::Text("a".to_string()), Cbor::Float(1.0)]),
                Cbor::Array(vec![Cbor::Text("b".to_string()), Cbor::Float(0.5)]),
            ])
        );
    }

    #[test]
    fn encode_vector_entry_pairs_bytes_and_meta() {
        use loom_codec::Value as Cbor;
        use loom_core::tabular::Value;
        let mut meta = BTreeMap::new();
        meta.insert("k".to_string(), Value::Int(1));
        let decoded =
            loom_codec::decode(&encode_vector_entry(&[1.0f32], &meta)).expect("entry decodes");
        let Cbor::Array(parts) = decoded else {
            panic!("vector entry is a CBOR array");
        };
        assert_eq!(parts.len(), 2);
        // First element: raw little-endian vector bytes; second: the metadata pair array.
        assert_eq!(parts[0], Cbor::Bytes(encode_f32_vec(&[1.0f32])));
        assert_eq!(parts[1], loom_codec::decode(&encode_meta(&meta)).unwrap());
    }

    #[test]
    fn columnar_schema_round_trips() {
        use loom_core::tabular::ColumnType;
        let cols = vec![
            ("id".to_string(), ColumnType::Int),
            ("name".to_string(), ColumnType::Text),
        ];
        assert_eq!(decode_columns(&encode_columns(&cols)), Some(cols));
        // Not canonical CBOR / not a pair array.
        assert_eq!(decode_columns(&[0xFF]), None);
    }

    #[test]
    fn columnar_row_round_trips_and_rows_encode() {
        use loom_codec::Value as Cbor;
        use loom_core::tabular::{Value, cell_value};
        let row = vec![Value::Int(1), Value::Text("a".to_string())];
        let row_wire = loom_codec::encode(&Cbor::Array(row.iter().map(cell_value).collect()))
            .expect("row encodes");
        assert_eq!(decode_row(&row_wire), Some(row.clone()));
        assert_eq!(decode_row(&[0xFF]), None);
        // encode_rows wraps each row as its cell array, in row order.
        let rows = vec![row, vec![Value::Int(2)]];
        let Cbor::Array(items) = loom_codec::decode(&encode_rows(&rows)).expect("rows decode")
        else {
            panic!("columnar scan wire form is a CBOR array");
        };
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], loom_codec::decode(&row_wire).unwrap());
    }

    #[test]
    fn log_buffer_enforces_entry_count_bound() {
        let mut buffer = LogBuffer::default();
        for _ in 0..(MAX_LOG_ENTRIES + 16) {
            buffer.push("x".to_string());
        }
        assert_eq!(buffer.entries.len(), MAX_LOG_ENTRIES);
    }

    #[test]
    fn log_buffer_enforces_total_byte_bound() {
        let mut buffer = LogBuffer::default();
        buffer.push("a".repeat(MAX_LOG_BYTES - 1));
        // Fits exactly to the bound.
        buffer.push("b".to_string());
        assert_eq!(buffer.total_bytes, MAX_LOG_BYTES);
        // Any further byte would exceed the bound and is dropped.
        buffer.push("c".to_string());
        assert_eq!(buffer.entries.len(), 2);
        assert!(buffer.total_bytes <= MAX_LOG_BYTES);
    }
}
