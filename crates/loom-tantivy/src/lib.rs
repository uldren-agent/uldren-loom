//! Optional native Tantivy full-text engine for Uldren Loom.
//!
//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use loom_core::{
    Code, Document, FieldType, FieldValue, LoomError, Query, QueryRequest, QueryResponse, Result,
    SearchCollection, SearchEngine, SearchHit,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, STORED, STRING, Schema, TEXT, TantivyDocument, Value};
use tantivy::{Index, TantivyError};

const LOOM_ID_FIELD: &str = "__loom_id_hex";
const PAYLOAD_MAGIC: &[u8; 5] = b"LTSI1";

/// First native Tantivy engine implementation behind the portable `SearchEngine` trait.
#[derive(Debug, Default)]
pub struct TantivySearchEngine;

impl TantivySearchEngine {
    /// Construct a native Tantivy search engine.
    pub const fn new() -> Self {
        Self
    }
}

impl SearchEngine for TantivySearchEngine {
    fn query(
        &self,
        collection: &SearchCollection,
        request: &QueryRequest,
    ) -> Result<QueryResponse> {
        let built = build_index(collection)?;
        run_query(&built, collection, request)
    }
}

/// Return the underlying Tantivy version string used by this crate.
pub fn tantivy_search_engine_version() -> String {
    tantivy::version_string().to_string()
}

/// Build a derived Tantivy index payload for durable-local storage.
pub fn build_tantivy_index_payload(collection: &SearchCollection) -> Result<Vec<u8>> {
    let dir = tempfile::tempdir().map_err(io_err)?;
    build_index_in_dir(collection, dir.path())?;
    pack_dir(dir.path())
}

/// Query a derived Tantivy index payload previously returned by [`build_tantivy_index_payload`].
pub fn query_tantivy_index_payload(
    payload: &[u8],
    request: &QueryRequest,
) -> Result<QueryResponse> {
    let dir = tempfile::tempdir().map_err(io_err)?;
    unpack_dir(payload, dir.path())?;
    let index = Index::open_in_dir(dir.path()).map_err(tantivy_err)?;
    let id_field = index
        .schema()
        .get_field(LOOM_ID_FIELD)
        .map_err(|_| LoomError::corrupt("native search payload is missing its Loom id field"))?;
    run_payload_query(&BuiltIndex { index, id_field }, request)
}

struct BuiltIndex {
    index: Index,
    id_field: Field,
}

fn build_index(collection: &SearchCollection) -> Result<BuiltIndex> {
    let schema = schema_for_collection(collection)?;
    let id_field = schema
        .get_field(LOOM_ID_FIELD)
        .map_err(|_| LoomError::corrupt("native search schema is missing its Loom id field"))?;
    let index = Index::create_in_ram(schema.clone());
    write_collection_to_index(collection, &index, &schema)?;
    Ok(BuiltIndex { index, id_field })
}

fn build_index_in_dir(collection: &SearchCollection, path: &Path) -> Result<()> {
    let schema = schema_for_collection(collection)?;
    let index = Index::create_in_dir(path, schema.clone()).map_err(tantivy_err)?;
    write_collection_to_index(collection, &index, &schema)
}

fn schema_for_collection(collection: &SearchCollection) -> Result<Schema> {
    if collection.mapping().contains_key(LOOM_ID_FIELD) {
        return Err(LoomError::invalid(format!(
            "search field {LOOM_ID_FIELD:?} is reserved"
        )));
    }

    let mut schema_builder = Schema::builder();
    schema_builder.add_text_field(LOOM_ID_FIELD, STRING | STORED);
    for (name, mapping) in collection.mapping() {
        let options = match mapping.field_type {
            FieldType::Text => TEXT | STORED,
            FieldType::Keyword => STRING | STORED,
        };
        schema_builder.add_text_field(name, options);
    }
    Ok(schema_builder.build())
}

fn write_collection_to_index(
    collection: &SearchCollection,
    index: &Index,
    schema: &Schema,
) -> Result<()> {
    let id_field = schema
        .get_field(LOOM_ID_FIELD)
        .map_err(|_| LoomError::corrupt("native search schema is missing its Loom id field"))?;
    let fields = fields_for_collection(collection, schema)?;

    let mut writer = index
        .writer::<TantivyDocument>(50_000_000)
        .map_err(tantivy_err)?;

    for id in collection.ids(None) {
        let mut doc = TantivyDocument::new();
        doc.add_text(id_field, hex_encode(&id));
        let Some(fields_doc) = collection.get(&id) else {
            return Err(LoomError::corrupt(
                "search id list referenced a missing document",
            ));
        };
        for (name, value) in fields_doc {
            if let Some(field) = fields.get(name) {
                match value {
                    FieldValue::Text(text) => doc.add_text(*field, text),
                    FieldValue::Bytes(bytes) => doc.add_text(*field, hex_encode(bytes)),
                }
            }
        }
        writer.add_document(doc).map_err(tantivy_err)?;
    }
    writer.commit().map_err(tantivy_err)?;

    Ok(())
}

fn fields_for_collection(
    collection: &SearchCollection,
    schema: &Schema,
) -> Result<BTreeMap<String, Field>> {
    collection
        .mapping()
        .keys()
        .map(|name| {
            let field = schema.get_field(name).map_err(|_| {
                LoomError::corrupt(format!("native search schema is missing field {name:?}"))
            })?;
            Ok((name.clone(), field))
        })
        .collect()
}

fn run_query(
    built: &BuiltIndex,
    collection: &SearchCollection,
    request: &QueryRequest,
) -> Result<QueryResponse> {
    let scores = query_scores(built, collection, &request.query)?;
    let portable = collection.query(request)?;
    let highlights = portable
        .hits
        .into_iter()
        .map(|hit| (hit.id, hit.highlights))
        .collect::<BTreeMap<_, _>>();
    let mut hits = page_hits(scores, request.offset, request.limit);
    for hit in &mut hits {
        hit.highlights = highlights.get(&hit.id).cloned().unwrap_or_default();
    }
    Ok(QueryResponse {
        reduced: false,
        hits,
        facets: portable.facets,
        aggregations: portable.aggregations,
    })
}

fn run_payload_query(built: &BuiltIndex, request: &QueryRequest) -> Result<QueryResponse> {
    let Query::Match { field, text } = &request.query else {
        return Err(LoomError::unsupported(
            "native Tantivy payload query currently supports match queries",
        ));
    };
    let scores = match_scores(built, field, text)?;
    Ok(QueryResponse {
        reduced: false,
        hits: page_hits(scores, request.offset, request.limit),
        facets: BTreeMap::new(),
        aggregations: BTreeMap::new(),
    })
}

fn query_scores(
    built: &BuiltIndex,
    collection: &SearchCollection,
    query: &Query,
) -> Result<BTreeMap<Vec<u8>, f32>> {
    match query {
        Query::MatchAll => {
            let mut scores = BTreeMap::new();
            for id in collection.ids(None) {
                scores.insert(id, 1.0);
            }
            Ok(scores)
        }
        Query::Match { field, text } => match_scores(built, field, text),
        Query::Term { field, value } => deterministic_scores(collection, field, |doc| {
            doc.get(field)
                .is_some_and(|field_value| field_value_bytes(field_value) == &value[..])
        }),
        Query::Phrase { field, terms, slop } => deterministic_scores(collection, field, |doc| {
            let have = doc.get(field).map(field_tokens).unwrap_or_default();
            phrase_matches(&have, terms, *slop as usize)
        }),
        Query::Range {
            field,
            lower,
            upper,
            include_lower,
            include_upper,
        } => deterministic_scores(collection, field, |doc| {
            doc.get(field).is_some_and(|field_value| {
                let value = field_value_bytes(field_value);
                let lower_ok = lower.as_ref().is_none_or(|lower| {
                    if *include_lower {
                        value >= &lower[..]
                    } else {
                        value > &lower[..]
                    }
                });
                let upper_ok = upper.as_ref().is_none_or(|upper| {
                    if *include_upper {
                        value <= &upper[..]
                    } else {
                        value < &upper[..]
                    }
                });
                lower_ok && upper_ok
            })
        }),
        Query::Prefix { field, value } => deterministic_scores(collection, field, |doc| {
            doc.get(field)
                .is_some_and(|field_value| field_value_bytes(field_value).starts_with(value))
        }),
        Query::Wildcard { field, pattern } => deterministic_scores(collection, field, |doc| {
            doc.get(field).is_some_and(|field_value| {
                wildcard_matches(pattern, field_value_bytes(field_value))
            })
        }),
        Query::Fuzzy {
            field,
            text,
            max_distance,
        } => deterministic_scores(collection, field, |doc| {
            let wanted = field_tokens(&FieldValue::Text(text.clone()));
            let have = doc.get(field).map(field_tokens).unwrap_or_default();
            wanted.iter().any(|want| {
                have.iter()
                    .any(|candidate| levenshtein_at_most(want, candidate, *max_distance))
            })
        }),
        Query::Similar {
            field,
            text,
            min_should_match,
        } => deterministic_scores(collection, field, |doc| {
            let wanted = field_tokens(&FieldValue::Text(text.clone()));
            let have = doc.get(field).map(field_tokens).unwrap_or_default();
            let matched = wanted.iter().filter(|term| have.contains(*term)).count() as u32;
            matched >= (*min_should_match).max(1)
        }),
        Query::Bool {
            must,
            should,
            must_not,
        } => bool_scores(built, collection, must, should, must_not),
    }
}

fn match_scores(built: &BuiltIndex, field: &str, text: &str) -> Result<BTreeMap<Vec<u8>, f32>> {
    let schema = built.index.schema();
    let field = schema.get_field(field).map_err(|_| {
        LoomError::no_such_field(format!("field {field:?} is not in the search mapping"))
    })?;
    let query = QueryParser::for_index(&built.index, vec![field])
        .parse_query(text)
        .map_err(|e| LoomError::new(Code::QueryParseError, e.to_string()))?;
    let reader = built.index.reader().map_err(tantivy_err)?;
    let searcher = reader.searcher();
    let indexed_docs = searcher.num_docs() as usize;
    if indexed_docs == 0 {
        return Ok(BTreeMap::new());
    }
    let mut scores = BTreeMap::new();
    for (score, address) in searcher
        .search(&query, &TopDocs::with_limit(indexed_docs).order_by_score())
        .map_err(tantivy_err)?
    {
        let doc: TantivyDocument = searcher.doc(address).map_err(tantivy_err)?;
        let id_hex = doc
            .get_first(built.id_field)
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or_else(|| LoomError::corrupt("native search hit is missing its Loom id"))?;
        scores.insert(hex_decode(&id_hex)?, score);
    }
    Ok(scores)
}

fn deterministic_scores(
    collection: &SearchCollection,
    field: &str,
    matches: impl Fn(&Document) -> bool,
) -> Result<BTreeMap<Vec<u8>, f32>> {
    require_field(collection, field)?;
    let mut scores = BTreeMap::new();
    for id in collection.ids(None) {
        let Some(doc) = collection.get(&id) else {
            return Err(LoomError::corrupt(
                "search id list referenced a missing document",
            ));
        };
        if matches(doc) {
            scores.insert(id, 1.0);
        }
    }
    Ok(scores)
}

fn bool_scores(
    built: &BuiltIndex,
    collection: &SearchCollection,
    must: &[Query],
    should: &[Query],
    must_not: &[Query],
) -> Result<BTreeMap<Vec<u8>, f32>> {
    let universe = collection.ids(None);
    let must_scores = must
        .iter()
        .map(|query| query_scores(built, collection, query))
        .collect::<Result<Vec<_>>>()?;
    let should_scores = should
        .iter()
        .map(|query| query_scores(built, collection, query))
        .collect::<Result<Vec<_>>>()?;
    let must_not_scores = must_not
        .iter()
        .map(|query| query_scores(built, collection, query))
        .collect::<Result<Vec<_>>>()?;

    let mut scores = BTreeMap::new();
    for id in universe {
        if must_scores
            .iter()
            .any(|query_scores| !query_scores.contains_key(&id))
        {
            continue;
        }
        if must_not_scores
            .iter()
            .any(|query_scores| query_scores.contains_key(&id))
        {
            continue;
        }
        let mut score = must_scores
            .iter()
            .filter_map(|query_scores| query_scores.get(&id))
            .sum::<f32>();
        let should_score = should_scores
            .iter()
            .filter_map(|query_scores| query_scores.get(&id))
            .sum::<f32>();
        if must_scores.is_empty() && !should_scores.is_empty() && should_score == 0.0 {
            continue;
        }
        score += should_score;
        scores.insert(id, score);
    }
    Ok(scores)
}

fn page_hits(scores: BTreeMap<Vec<u8>, f32>, offset: u32, limit: u32) -> Vec<SearchHit> {
    let mut hits = scores
        .into_iter()
        .map(|(id, score)| SearchHit {
            id,
            score,
            highlights: BTreeMap::new(),
        })
        .collect::<Vec<_>>();
    hits.sort_by(|a, b| match b.score.total_cmp(&a.score) {
        std::cmp::Ordering::Equal => a.id.cmp(&b.id),
        other => other,
    });
    hits.into_iter()
        .skip(offset as usize)
        .take(if limit == 0 {
            usize::MAX
        } else {
            limit as usize
        })
        .collect()
}

fn wildcard_matches(pattern: &[u8], value: &[u8]) -> bool {
    let mut previous = vec![false; value.len() + 1];
    previous[0] = true;
    for &token in pattern {
        let mut current = vec![false; value.len() + 1];
        match token {
            b'*' => {
                current[0] = previous[0];
                for idx in 1..=value.len() {
                    current[idx] = previous[idx] || current[idx - 1];
                }
            }
            b'?' => {
                for idx in 1..=value.len() {
                    current[idx] = previous[idx - 1];
                }
            }
            byte => {
                for idx in 1..=value.len() {
                    current[idx] = previous[idx - 1] && value[idx - 1] == byte;
                }
            }
        }
        previous = current;
    }
    previous[value.len()]
}

fn levenshtein_at_most(a: &str, b: &str, max: u32) -> bool {
    let max = max as usize;
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len().abs_diff(b.len()) > max {
        return false;
    }
    let mut previous = (0..=b.len()).collect::<Vec<_>>();
    for (i, &left) in a.iter().enumerate() {
        let mut current = Vec::with_capacity(b.len() + 1);
        current.push(i + 1);
        let mut row_min = current[0];
        for (j, &right) in b.iter().enumerate() {
            let insert = current[j] + 1;
            let delete = previous[j + 1] + 1;
            let replace = previous[j] + usize::from(left != right);
            let cost = insert.min(delete).min(replace);
            row_min = row_min.min(cost);
            current.push(cost);
        }
        if row_min > max {
            return false;
        }
        previous = current;
    }
    previous[b.len()] <= max
}

fn require_field(collection: &SearchCollection, field: &str) -> Result<()> {
    if collection.mapping().contains_key(field) {
        Ok(())
    } else {
        Err(LoomError::no_such_field(format!(
            "field {field:?} is not in the search mapping"
        )))
    }
}

fn field_value_bytes(value: &FieldValue) -> &[u8] {
    match value {
        FieldValue::Text(text) => text.as_bytes(),
        FieldValue::Bytes(bytes) => bytes,
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn field_tokens(value: &FieldValue) -> Vec<String> {
    match value {
        FieldValue::Text(text) => tokenize(text),
        FieldValue::Bytes(_) => Vec::new(),
    }
}

fn phrase_matches(have: &[String], terms: &[String], slop: usize) -> bool {
    if terms.is_empty() {
        return true;
    }
    let wanted = tokenize(&terms.join(" "));
    if wanted.is_empty() {
        return true;
    }
    for start in 0..have.len() {
        if have[start] != wanted[0] {
            continue;
        }
        let mut pos = start + 1;
        let mut ok = true;
        for term in &wanted[1..] {
            let limit = (pos + slop + 1).min(have.len());
            match have[pos..limit]
                .iter()
                .position(|candidate| candidate == term)
            {
                Some(off) => pos += off + 1,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            return true;
        }
    }
    false
}

fn tantivy_err(err: TantivyError) -> LoomError {
    LoomError::new(Code::Internal, err.to_string())
}

fn io_err(err: std::io::Error) -> LoomError {
    LoomError::new(Code::Io, err.to_string())
}

fn pack_dir(root: &Path) -> Result<Vec<u8>> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = Vec::new();
    out.extend_from_slice(PAYLOAD_MAGIC);
    put_u32(&mut out, files.len())?;
    for (path, bytes) in files {
        put_str(&mut out, &path)?;
        out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
        out.extend_from_slice(&bytes);
    }
    Ok(out)
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> Result<()> {
    let mut entries = fs::read_dir(dir)
        .map_err(io_err)?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(io_err)?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type().map_err(io_err)?;
        if file_type.is_dir() {
            collect_files(root, &path, out)?;
        } else if file_type.is_file() {
            let relative = relative_payload_path(root, &path)?;
            let bytes = fs::read(&path).map_err(io_err)?;
            out.push((relative, bytes));
        }
    }
    Ok(())
}

fn relative_payload_path(root: &Path, path: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| LoomError::corrupt("native search payload path escaped its root"))?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let std::path::Component::Normal(part) = component else {
            return Err(LoomError::corrupt(
                "native search payload path contains a non-normal component",
            ));
        };
        let part = part
            .to_str()
            .ok_or_else(|| LoomError::corrupt("native search payload path is not utf-8"))?;
        parts.push(part.to_string());
    }
    if parts.is_empty() {
        return Err(LoomError::corrupt("native search payload path is empty"));
    }
    Ok(parts.join("/"))
}

fn unpack_dir(payload: &[u8], root: &Path) -> Result<()> {
    let mut p = 0;
    if take_slice(payload, &mut p, PAYLOAD_MAGIC.len())? != PAYLOAD_MAGIC {
        return Err(LoomError::corrupt("native search payload magic mismatch"));
    }
    let file_count = take_u32(payload, &mut p)?;
    for _ in 0..file_count {
        let path = take_str(payload, &mut p)?;
        let len = u64::from_be_bytes(take_array(payload, &mut p)?) as usize;
        let bytes = take_slice(payload, &mut p, len)?;
        let target = payload_target(root, &path)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(io_err)?;
        }
        fs::write(target, bytes).map_err(io_err)?;
    }
    if p != payload.len() {
        return Err(LoomError::corrupt(
            "native search payload has trailing bytes",
        ));
    }
    Ok(())
}

fn payload_target(root: &Path, relative: &str) -> Result<PathBuf> {
    if relative.is_empty() || relative.starts_with('/') {
        return Err(LoomError::corrupt("native search payload path is invalid"));
    }
    let mut out = PathBuf::from(root);
    for part in relative.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(LoomError::corrupt(
                "native search payload path contains a non-normal component",
            ));
        }
        out.push(part);
    }
    Ok(out)
}

fn put_u32(out: &mut Vec<u8>, value: usize) -> Result<()> {
    let value = u32::try_from(value).map_err(|_| {
        LoomError::new(
            Code::ResourceExhausted,
            "native search payload has too many files",
        )
    })?;
    out.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

fn put_str(out: &mut Vec<u8>, value: &str) -> Result<()> {
    let len = u16::try_from(value.len()).map_err(|_| {
        LoomError::new(
            Code::ResourceExhausted,
            "native search payload path is too long",
        )
    })?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn take_u32(bytes: &[u8], p: &mut usize) -> Result<u32> {
    Ok(u32::from_be_bytes(take_array(bytes, p)?))
}

fn take_str(bytes: &[u8], p: &mut usize) -> Result<String> {
    let len = u16::from_be_bytes(take_array(bytes, p)?) as usize;
    let raw = take_slice(bytes, p, len)?;
    std::str::from_utf8(raw)
        .map(str::to_string)
        .map_err(|_| LoomError::corrupt("native search payload path is not utf-8"))
}

fn take_array<const N: usize>(bytes: &[u8], p: &mut usize) -> Result<[u8; N]> {
    let slice = take_slice(bytes, p, N)?;
    slice
        .try_into()
        .map_err(|_| LoomError::corrupt("native search payload ended early"))
}

fn take_slice<'a>(bytes: &'a [u8], p: &mut usize, len: usize) -> Result<&'a [u8]> {
    let end = p
        .checked_add(len)
        .ok_or_else(|| LoomError::corrupt("native search payload offset overflow"))?;
    if end > bytes.len() {
        return Err(LoomError::corrupt("native search payload ended early"));
    }
    let out = &bytes[*p..end];
    *p = end;
    Ok(out)
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

fn hex_decode(hex: &str) -> Result<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return Err(LoomError::corrupt("odd-length hex document id"));
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_bytes().chunks_exact(2) {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        out.push((high << 4) | low);
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(LoomError::corrupt("invalid hex document id")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{Document, FieldMapping, Mapping};

    fn mapping() -> Mapping {
        let mut mapping = Mapping::new();
        mapping.insert("title".to_string(), FieldMapping::text());
        mapping.insert("lang".to_string(), FieldMapping::keyword());
        mapping
    }

    fn doc(title: &str, lang: &str) -> Document {
        let mut doc = Document::new();
        doc.insert("title".to_string(), FieldValue::Text(title.to_string()));
        doc.insert(
            "lang".to_string(),
            FieldValue::Bytes(lang.as_bytes().to_vec()),
        );
        doc
    }

    fn collection() -> SearchCollection {
        let mut collection = SearchCollection::new(mapping());
        collection.index(b"a".to_vec(), doc("the quick brown fox", "en"));
        collection.index(b"b".to_vec(), doc("quick green turtle", "en"));
        collection.index(b"c".to_vec(), doc("le renard brun", "fr"));
        collection
    }

    fn quick_request() -> QueryRequest {
        QueryRequest::new(
            Query::Match {
                field: "title".to_string(),
                text: "quick".to_string(),
            },
            0,
            0,
        )
    }

    #[test]
    fn native_match_query_uses_tantivy() {
        let collection = collection();

        let response = TantivySearchEngine::new()
            .query(&collection, &quick_request())
            .unwrap();

        assert!(!response.reduced);
        assert_eq!(response.hits.len(), 2);
        let mut ids = response
            .hits
            .iter()
            .map(|hit| hit.id.clone())
            .collect::<Vec<_>>();
        ids.sort();
        assert_eq!(ids, vec![b"a".to_vec(), b"b".to_vec()]);
        assert!(response.hits.iter().all(|hit| hit.score > 0.0));
    }

    #[test]
    fn derived_payload_round_trips_tantivy_index() {
        let payload = build_tantivy_index_payload(&collection()).unwrap();
        let response = query_tantivy_index_payload(&payload, &quick_request()).unwrap();

        assert!(!response.reduced);
        assert_eq!(response.hits.len(), 2);
        let mut ids = response
            .hits
            .iter()
            .map(|hit| hit.id.clone())
            .collect::<Vec<_>>();
        ids.sort();
        assert_eq!(ids, vec![b"a".to_vec(), b"b".to_vec()]);
    }

    #[test]
    fn native_match_query_reports_unknown_field() {
        let collection = SearchCollection::new(mapping());
        let error = TantivySearchEngine::new()
            .query(
                &collection,
                &QueryRequest::new(
                    Query::Match {
                        field: "body".to_string(),
                        text: "quick".to_string(),
                    },
                    0,
                    0,
                ),
            )
            .unwrap_err();
        assert_eq!(error.code, Code::NoSuchField);
    }

    #[test]
    fn native_deterministic_query_shapes_cover_parity_subset() {
        let collection = collection();

        let term = TantivySearchEngine::new()
            .query(&collection, &request(term_query()))
            .unwrap();
        assert!(!term.reduced);
        assert_eq!(sorted_ids(&term), vec![b"a".to_vec(), b"b".to_vec()]);

        let phrase = TantivySearchEngine::new()
            .query(
                &collection,
                &request(Query::Phrase {
                    field: "title".to_string(),
                    terms: vec!["quick".to_string(), "brown".to_string()],
                    slop: 0,
                }),
            )
            .unwrap();
        assert!(!phrase.reduced);
        assert_eq!(sorted_ids(&phrase), vec![b"a".to_vec()]);

        let range = TantivySearchEngine::new()
            .query(
                &collection,
                &request(Query::Range {
                    field: "lang".to_string(),
                    lower: Some(b"en".to_vec()),
                    upper: Some(b"fr".to_vec()),
                    include_lower: true,
                    include_upper: false,
                }),
            )
            .unwrap();
        assert!(!range.reduced);
        assert_eq!(sorted_ids(&range), vec![b"a".to_vec(), b"b".to_vec()]);

        let bool_query = TantivySearchEngine::new()
            .query(
                &collection,
                &request(Query::Bool {
                    must: vec![Query::Match {
                        field: "title".to_string(),
                        text: "quick".to_string(),
                    }],
                    should: Vec::new(),
                    must_not: vec![Query::Phrase {
                        field: "title".to_string(),
                        terms: vec!["quick".to_string(), "brown".to_string()],
                        slop: 0,
                    }],
                }),
            )
            .unwrap();
        assert!(!bool_query.reduced);
        assert_eq!(sorted_ids(&bool_query), vec![b"b".to_vec()]);
    }

    #[test]
    fn payload_query_rejects_shapes_that_require_source_collection() {
        let payload = build_tantivy_index_payload(&collection()).unwrap();
        let error = query_tantivy_index_payload(&payload, &request(term_query())).unwrap_err();
        assert_eq!(error.code, Code::Unsupported);
    }

    #[test]
    fn native_bm25_scores_rank_term_frequency_and_tie_by_id() {
        let mut collection = SearchCollection::new(mapping());
        collection.index(b"b".to_vec(), doc("quick", "en"));
        collection.index(b"a".to_vec(), doc("quick", "en"));
        collection.index(b"freq".to_vec(), doc("quick quick quick", "en"));
        collection.index(b"none".to_vec(), doc("slow turtle", "en"));

        let response = TantivySearchEngine::new()
            .query(&collection, &quick_request())
            .unwrap();

        assert!(!response.reduced);
        assert_eq!(
            response
                .hits
                .iter()
                .map(|hit| hit.id.clone())
                .collect::<Vec<_>>(),
            vec![b"freq".to_vec(), b"a".to_vec(), b"b".to_vec()]
        );
        assert!(response.hits[0].score > response.hits[1].score);
        assert_eq!(response.hits[1].score, response.hits[2].score);
    }

    #[test]
    fn native_bm25_scores_handle_empty_and_no_hit_collections() {
        let empty = SearchCollection::new(mapping());
        let empty_response = TantivySearchEngine::new()
            .query(&empty, &quick_request())
            .unwrap();

        assert!(!empty_response.reduced);
        assert!(empty_response.hits.is_empty());

        let mut no_hit = SearchCollection::new(mapping());
        no_hit.index(b"a".to_vec(), doc("slow turtle", "en"));
        let no_hit_response = TantivySearchEngine::new()
            .query(&no_hit, &quick_request())
            .unwrap();

        assert!(!no_hit_response.reduced);
        assert!(no_hit_response.hits.is_empty());
    }

    #[test]
    fn hex_round_trips_document_ids() {
        let id = b"\x00loom\xff";
        assert_eq!(hex_decode(&hex_encode(id)).unwrap(), id);
    }

    fn request(query: Query) -> QueryRequest {
        QueryRequest::new(query, 0, 0)
    }

    fn term_query() -> Query {
        Query::Term {
            field: "lang".to_string(),
            value: b"en".to_vec(),
        }
    }

    fn sorted_ids(response: &QueryResponse) -> Vec<Vec<u8>> {
        let mut ids = response
            .hits
            .iter()
            .map(|hit| hit.id.clone())
            .collect::<Vec<_>>();
        ids.sort();
        ids
    }
}
