//! The search facet: committed full-text documents plus a field mapping and a portable linear-scan
//! query path.
//!
//! Committed Loom state is only the document map (id -> field document) and the field mapping; both
//! version, branch, diff, and sync. The reduced query path is deterministic and available on every
//! platform.
//!
//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use crate::AclRight;
use crate::cbor::{self, Value as CborValue};
use crate::digest::{DIGEST_LEN, Digest};
use crate::error::{Code, LoomError, Result};
use crate::object::content_address_with;
use crate::provider::ObjectStore;
use crate::tabular::{Value as TabularValue, cell_value};
use crate::vcs::Loom;
use crate::workspace::{FacetKind, WorkspaceId, facet_path, facet_root};
use std::cmp::Ordering;
use std::collections::BTreeMap;

pub(crate) const STRUCTURED_SEARCH_ROOT_SCHEMA: &str = "loom.search.structured-collection-root.v1";
const STRUCTURED_SEARCH_ROOT_SCHEMA_PREFIX: &str = "loom.search.structured-collection-root.";

/// How a mapped field is treated by the query layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    /// Analyzed full-text: tokenized (whitespace, lower-cased) for `Match`/`Phrase` queries.
    Text,
    /// Exact value: matched whole for `Term`/`Range` queries, never tokenized.
    Keyword,
}

impl FieldType {
    const fn tag(self) -> u64 {
        match self {
            FieldType::Text => 0,
            FieldType::Keyword => 1,
        }
    }
    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(FieldType::Text),
            1 => Ok(FieldType::Keyword),
            other => Err(LoomError::corrupt(format!(
                "unknown field type tag {other}"
            ))),
        }
    }
}

/// Analyzer and normalizer names declared for one mapped field.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AnalyzerMapping {
    pub index_analyzer: Option<String>,
    pub search_analyzer: Option<String>,
    pub normalizer: Option<String>,
}

/// The mapping for one field: its type, whether the source value is stored, whether it is faceted,
/// and the declared analyzer configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldMapping {
    pub field_type: FieldType,
    pub stored: bool,
    pub faceted: bool,
    pub analysis: AnalyzerMapping,
}

impl FieldMapping {
    /// An analyzed, stored, non-faceted text field (the common case).
    pub fn text() -> Self {
        Self {
            field_type: FieldType::Text,
            stored: true,
            faceted: false,
            analysis: AnalyzerMapping::default(),
        }
    }
    /// An exact, stored keyword field.
    pub fn keyword() -> Self {
        Self {
            field_type: FieldType::Keyword,
            stored: true,
            faceted: false,
            analysis: AnalyzerMapping::default(),
        }
    }
}

/// The field mapping for a search collection: field name -> [`FieldMapping`]. A field absent here is
/// stored in the source document but not queryable.
pub type Mapping = BTreeMap<String, FieldMapping>;

/// A field value in a document: analyzed text or an opaque exact value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValue {
    Text(String),
    Bytes(Vec<u8>),
}

impl FieldValue {
    /// The value as comparable bytes (text as its UTF-8 bytes), for `Term`/`Range`.
    fn as_bytes(&self) -> &[u8] {
        match self {
            FieldValue::Text(s) => s.as_bytes(),
            FieldValue::Bytes(b) => b,
        }
    }
}

/// A document: field name -> value. Document ids are opaque byte strings with deterministic ordering.
pub type Document = BTreeMap<String, FieldValue>;

/// A versioned search collection: the field mapping plus the id-keyed document map. Committed state.
#[derive(Debug, Clone, Default)]
pub struct SearchCollection {
    mapping: Mapping,
    docs: BTreeMap<Vec<u8>, Document>,
}

impl SearchCollection {
    /// An empty collection with the given field `mapping`.
    pub fn new(mapping: Mapping) -> Self {
        Self {
            mapping,
            docs: BTreeMap::new(),
        }
    }

    /// The field mapping.
    pub fn mapping(&self) -> &Mapping {
        &self.mapping
    }
    pub(crate) fn from_parts(mapping: Mapping, docs: BTreeMap<Vec<u8>, Document>) -> Self {
        Self { mapping, docs }
    }
    pub(crate) fn docs(&self) -> &BTreeMap<Vec<u8>, Document> {
        &self.docs
    }
    /// Number of documents.
    pub fn len(&self) -> usize {
        self.docs.len()
    }
    /// Whether the collection has no documents.
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// Insert or replace the document at `id`.
    pub fn index(&mut self, id: Vec<u8>, doc: Document) {
        self.docs.insert(id, doc);
    }
    /// The document at `id`, or `None`.
    pub fn get(&self, id: &[u8]) -> Option<&Document> {
        self.docs.get(id)
    }
    /// Remove `id`; returns whether it was present.
    pub fn delete(&mut self, id: &[u8]) -> bool {
        self.docs.remove(id).is_some()
    }
    /// Document ids in order, optionally restricted to those starting with `prefix`.
    pub fn ids(&self, prefix: Option<&[u8]>) -> Vec<Vec<u8>> {
        self.docs
            .keys()
            .filter(|id| prefix.is_none_or(|p| id.starts_with(p)))
            .cloned()
            .collect()
    }
    /// Replace the field mapping (a `remap`; the derived index is rebuilt by the native layer).
    pub fn remap(&mut self, mapping: Mapping) {
        self.mapping = mapping;
    }

    /// The reduced, deterministic linear-scan query. Hits are score-descending, id-ascending after
    /// `offset`/`limit`. `NO_SUCH_FIELD` if a query names an unmapped field.
    pub fn query(&self, request: &QueryRequest) -> Result<QueryResponse> {
        let mut hits: Vec<SearchHit> = Vec::new();
        let mut matched_docs: Vec<&Document> = Vec::new();
        for (id, doc) in &self.docs {
            let (matched, score) = self.eval(&request.query, doc)?;
            if matched {
                matched_docs.push(doc);
                hits.push(SearchHit {
                    id: id.clone(),
                    score,
                    highlights: self.highlights(request, doc)?,
                });
            }
        }
        hits.sort_by(|a, b| match b.score.total_cmp(&a.score) {
            Ordering::Equal => a.id.cmp(&b.id),
            other => other,
        });
        let offset = request.offset as usize;
        let limit = request.limit as usize;
        let hits = hits
            .into_iter()
            .skip(offset)
            .take(if limit == 0 { usize::MAX } else { limit })
            .collect();
        Ok(QueryResponse {
            reduced: true,
            hits,
            facets: self.facets(request, &matched_docs)?,
            aggregations: self.aggregations(request, &matched_docs)?,
        })
    }

    /// Evaluate `query` against `doc`, returning `(matched, score)`. `NO_SUCH_FIELD` for an unmapped
    /// field reference.
    fn eval(&self, query: &Query, doc: &Document) -> Result<(bool, f32)> {
        match query {
            Query::MatchAll => Ok((true, 1.0)),
            Query::Match { field, text } => {
                self.require_field(field)?;
                let wanted = tokenize(text);
                let have = doc.get(field).map(field_tokens).unwrap_or_default();
                let score = have.iter().filter(|t| wanted.contains(*t)).count() as f32;
                Ok((score > 0.0, score))
            }
            Query::Term { field, value } => {
                self.require_field(field)?;
                let hit = doc.get(field).is_some_and(|v| v.as_bytes() == &value[..]);
                Ok((hit, if hit { 1.0 } else { 0.0 }))
            }
            Query::Phrase { field, terms, slop } => {
                self.require_field(field)?;
                let have = doc.get(field).map(field_tokens).unwrap_or_default();
                let hit = phrase_matches(&have, terms, *slop as usize);
                Ok((hit, if hit { 1.0 } else { 0.0 }))
            }
            Query::Range {
                field,
                lower,
                upper,
                include_lower,
                include_upper,
            } => {
                self.require_field(field)?;
                let hit = doc.get(field).is_some_and(|v| {
                    let b = v.as_bytes();
                    let lo_ok = lower.as_ref().is_none_or(|l| {
                        if *include_lower {
                            b >= &l[..]
                        } else {
                            b > &l[..]
                        }
                    });
                    let hi_ok = upper.as_ref().is_none_or(|u| {
                        if *include_upper {
                            b <= &u[..]
                        } else {
                            b < &u[..]
                        }
                    });
                    lo_ok && hi_ok
                });
                Ok((hit, if hit { 1.0 } else { 0.0 }))
            }
            Query::Prefix { field, value } => {
                self.require_field(field)?;
                let hit = doc
                    .get(field)
                    .is_some_and(|v| v.as_bytes().starts_with(value));
                Ok((hit, if hit { 1.0 } else { 0.0 }))
            }
            Query::Wildcard { field, pattern } => {
                self.require_field(field)?;
                let hit = doc
                    .get(field)
                    .is_some_and(|v| wildcard_matches(pattern, v.as_bytes()));
                Ok((hit, if hit { 1.0 } else { 0.0 }))
            }
            Query::Fuzzy {
                field,
                text,
                max_distance,
            } => {
                self.require_field(field)?;
                let wanted = tokenize(text);
                let have = doc.get(field).map(field_tokens).unwrap_or_default();
                let hit = wanted.iter().any(|want| {
                    have.iter()
                        .any(|candidate| levenshtein_at_most(want, candidate, *max_distance))
                });
                Ok((hit, if hit { 1.0 } else { 0.0 }))
            }
            Query::Similar {
                field,
                text,
                min_should_match,
            } => {
                self.require_field(field)?;
                let wanted = tokenize(text);
                let have = doc.get(field).map(field_tokens).unwrap_or_default();
                let matched = wanted.iter().filter(|term| have.contains(*term)).count() as u32;
                let required = (*min_should_match).max(1);
                let hit = matched >= required;
                Ok((hit, if hit { matched as f32 } else { 0.0 }))
            }
            Query::Bool {
                must,
                should,
                must_not,
            } => {
                let mut score = 0.0;
                let mut must_ok = true;
                for q in must {
                    let (m, s) = self.eval(q, doc)?;
                    must_ok &= m;
                    score += s;
                }
                let mut should_matched = false;
                for q in should {
                    let (m, s) = self.eval(q, doc)?;
                    if m {
                        should_matched = true;
                        score += s;
                    }
                }
                let mut not_ok = true;
                for q in must_not {
                    let (m, _) = self.eval(q, doc)?;
                    not_ok &= !m;
                }
                // With must clauses, should is a bonus; without, at least one should must match (unless
                // there are no should clauses at all).
                let positive = if must.is_empty() {
                    should.is_empty() || should_matched
                } else {
                    must_ok
                };
                Ok((must_ok && not_ok && positive, score))
            }
        }
    }

    fn highlights(
        &self,
        request: &QueryRequest,
        doc: &Document,
    ) -> Result<BTreeMap<String, Vec<String>>> {
        let mut out = BTreeMap::new();
        for field in &request.highlight {
            self.require_field(field)?;
            if let Some(FieldValue::Text(value)) = doc.get(field) {
                out.insert(field.clone(), vec![value.clone()]);
            }
        }
        Ok(out)
    }

    fn facets(
        &self,
        request: &QueryRequest,
        matched_docs: &[&Document],
    ) -> Result<BTreeMap<String, Vec<FacetBucket>>> {
        let mut out = BTreeMap::new();
        for field in &request.facets {
            let mapping = self.require_field(field)?;
            if !mapping.faceted {
                return Err(LoomError::invalid(format!(
                    "field {field:?} is not faceted in the search mapping"
                )));
            }
            let mut counts = BTreeMap::<Vec<u8>, u64>::new();
            for doc in matched_docs {
                if let Some(value) = doc.get(field) {
                    *counts.entry(value.as_bytes().to_vec()).or_default() += 1;
                }
            }
            out.insert(
                field.clone(),
                counts
                    .into_iter()
                    .map(|(value, count)| FacetBucket { value, count })
                    .collect(),
            );
        }
        Ok(out)
    }

    fn aggregations(
        &self,
        request: &QueryRequest,
        matched_docs: &[&Document],
    ) -> Result<BTreeMap<String, AggregationResult>> {
        let mut out = BTreeMap::new();
        for aggregation in &request.aggregations {
            match aggregation {
                AggregationRequest::Terms { name, field } => {
                    self.require_field(field)?;
                    let mut counts = BTreeMap::<Vec<u8>, u64>::new();
                    for doc in matched_docs {
                        if let Some(value) = doc.get(field) {
                            *counts.entry(value.as_bytes().to_vec()).or_default() += 1;
                        }
                    }
                    out.insert(
                        name.clone(),
                        AggregationResult::Buckets(
                            counts
                                .into_iter()
                                .map(|(value, count)| FacetBucket { value, count })
                                .collect(),
                        ),
                    );
                }
                AggregationRequest::ValueCount { name, field } => {
                    self.require_field(field)?;
                    let count = matched_docs
                        .iter()
                        .filter(|doc| doc.contains_key(field))
                        .count() as u64;
                    out.insert(name.clone(), AggregationResult::Count(count));
                }
            }
        }
        Ok(out)
    }

    fn require_field(&self, field: &str) -> Result<&FieldMapping> {
        self.mapping.get(field).ok_or_else(|| {
            LoomError::no_such_field(format!("field {field:?} is not in the search mapping"))
        })
    }

    /// Canonical bytes: `[mapping, docs]`. `mapping` is a list of
    /// `[name, type-tag, stored, faceted, index-analyzer, search-analyzer, normalizer]`; `docs` is
    /// a list of `[id, [[field, value-tag, value] ...]]` in id order. Deterministic.
    pub fn encode(&self) -> Vec<u8> {
        use CborValue::{Array, Bytes, Text, Uint};
        let mapping = self
            .mapping
            .iter()
            .map(|(name, m)| {
                Array(vec![
                    Text(name.clone()),
                    Uint(m.field_type.tag()),
                    Uint(u64::from(m.stored)),
                    Uint(u64::from(m.faceted)),
                    opt_text_value(&m.analysis.index_analyzer),
                    opt_text_value(&m.analysis.search_analyzer),
                    opt_text_value(&m.analysis.normalizer),
                ])
            })
            .collect();
        let docs = self
            .docs
            .iter()
            .map(|(id, doc)| {
                let fields = doc
                    .iter()
                    .map(|(name, value)| match value {
                        FieldValue::Text(s) => {
                            Array(vec![Text(name.clone()), Uint(0), Text(s.clone())])
                        }
                        FieldValue::Bytes(b) => {
                            Array(vec![Text(name.clone()), Uint(1), Bytes(b.clone())])
                        }
                    })
                    .collect();
                Array(vec![Bytes(id.clone()), Array(fields)])
            })
            .collect();
        cbor::encode(&Array(vec![Array(mapping), Array(docs)]))
    }

    /// Parse a collection from [`SearchCollection::encode`] output.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut top = cbor::Fields::new(cbor::decode_array(bytes)?);
        let mapping_raw = top.array()?;
        let docs_raw = top.array()?;
        top.end()?;

        let mut mapping = Mapping::new();
        for item in mapping_raw {
            let fields = cbor::as_array(item)?;
            if fields.len() != 4 && fields.len() != 7 {
                return Err(LoomError::corrupt(
                    "search mapping entry must have 4 or 7 fields",
                ));
            }
            let name = cbor::as_text(fields[0].clone())?;
            let field_type = FieldType::from_tag(cbor::as_uint(fields[1].clone())?)?;
            let stored = cbor::as_uint(fields[2].clone())? != 0;
            let faceted = cbor::as_uint(fields[3].clone())? != 0;
            let analysis = if fields.len() == 4 {
                AnalyzerMapping::default()
            } else {
                AnalyzerMapping {
                    index_analyzer: opt_text_from_value(fields[4].clone())?,
                    search_analyzer: opt_text_from_value(fields[5].clone())?,
                    normalizer: opt_text_from_value(fields[6].clone())?,
                }
            };
            mapping.insert(
                name,
                FieldMapping {
                    field_type,
                    stored,
                    faceted,
                    analysis,
                },
            );
        }

        let mut docs = BTreeMap::new();
        for item in docs_raw {
            let mut f = cbor::Fields::new(cbor::as_array(item)?);
            let id = f.bytes()?;
            let fields_raw = f.array()?;
            f.end()?;
            let mut doc = Document::new();
            for field in fields_raw {
                let mut ff = cbor::Fields::new(cbor::as_array(field)?);
                let name = ff.text()?;
                let tag = ff.uint()?;
                let value = match tag {
                    0 => FieldValue::Text(ff.text()?),
                    1 => FieldValue::Bytes(ff.bytes()?),
                    other => {
                        return Err(LoomError::corrupt(format!(
                            "unknown field value tag {other}"
                        )));
                    }
                };
                ff.end()?;
                doc.insert(name, value);
            }
            docs.insert(id, doc);
        }
        Ok(Self { mapping, docs })
    }
}

/// A structured query leaf or composite over a search collection.
#[derive(Debug, Clone)]
pub enum Query {
    /// Every document matches.
    MatchAll,
    /// Any analyzed term of `text` occurs in `field`.
    Match { field: String, text: String },
    /// `field`'s exact value equals `value`.
    Term { field: String, value: Vec<u8> },
    /// `terms` occur in order in `field` within `slop` extra tokens between them.
    Phrase {
        field: String,
        terms: Vec<String>,
        slop: u32,
    },
    /// `field`'s value falls in the byte range `[lower, upper]` per the inclusivity flags.
    Range {
        field: String,
        lower: Option<Vec<u8>>,
        upper: Option<Vec<u8>>,
        include_lower: bool,
        include_upper: bool,
    },
    /// `field` starts with `value`.
    Prefix { field: String, value: Vec<u8> },
    /// `field` matches a byte pattern with `*` and `?` wildcards.
    Wildcard { field: String, pattern: Vec<u8> },
    /// A token in `field` is within `max_distance` edits of a query token.
    Fuzzy {
        field: String,
        text: String,
        max_distance: u32,
    },
    /// At least `min_should_match` tokens from `text` appear in `field`.
    Similar {
        field: String,
        text: String,
        min_should_match: u32,
    },
    /// Boolean composition: all `must` match, no `must_not` matches, `should` boosts the score.
    Bool {
        must: Vec<Query>,
        should: Vec<Query>,
        must_not: Vec<Query>,
    },
}

/// A query plus paging.
#[derive(Debug, Clone)]
pub struct QueryRequest {
    pub query: Query,
    /// Maximum hits to return; `0` means unbounded.
    pub limit: u32,
    /// Hits to skip before returning.
    pub offset: u32,
    /// Faceted fields to count over the matched collection.
    pub facets: Vec<String>,
    /// Stored text fields to return as reduced highlight snippets.
    pub highlight: Vec<String>,
    /// Named aggregation requests. The portable engine records the request shape but only executes
    /// facet buckets.
    pub aggregations: Vec<AggregationRequest>,
}

impl QueryRequest {
    pub fn new(query: Query, limit: u32, offset: u32) -> Self {
        Self {
            query,
            limit,
            offset,
            facets: Vec::new(),
            highlight: Vec::new(),
            aggregations: Vec::new(),
        }
    }
}

/// One search result.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub id: Vec<u8>,
    pub score: f32,
    pub highlights: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetBucket {
    pub value: Vec<u8>,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggregationRequest {
    Terms { name: String, field: String },
    ValueCount { name: String, field: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggregationResult {
    Buckets(Vec<FacetBucket>),
    Count(u64),
}

/// A query result. `reduced` is `true` when produced by the portable linear-scan fallback.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryResponse {
    pub reduced: bool,
    pub hits: Vec<SearchHit>,
    pub facets: BTreeMap<String, Vec<FacetBucket>>,
    pub aggregations: BTreeMap<String, AggregationResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchAliasTarget {
    pub collection: String,
    pub is_write_index: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchAlias {
    pub targets: Vec<SearchAliasTarget>,
}

pub fn search_mapping_from_cbor(bytes: &[u8]) -> Result<Mapping> {
    let CborValue::Map(pairs) = cbor::decode(bytes)? else {
        return Err(LoomError::invalid("search mapping must be a CBOR map"));
    };
    let mut mapping = Mapping::new();
    for (key, value) in pairs {
        let CborValue::Text(field) = key else {
            return Err(LoomError::invalid("search mapping field name must be text"));
        };
        let CborValue::Array(parts) = value else {
            return Err(LoomError::invalid(
                "search field mapping must be [type_tag, stored, faceted] or [type_tag, stored, faceted, index_analyzer, search_analyzer, normalizer]",
            ));
        };
        if parts.len() != 3 && parts.len() != 6 {
            return Err(LoomError::invalid(
                "search field mapping must have 3 or 6 fields",
            ));
        }
        let field_type = match &parts[0] {
            CborValue::Uint(tag) => FieldType::from_tag(*tag)?,
            _ => return Err(LoomError::invalid("search field type tag must be uint")),
        };
        let stored = matches!(parts[1], CborValue::Bool(true));
        let faceted = matches!(parts[2], CborValue::Bool(true));
        let analysis = if parts.len() == 3 {
            AnalyzerMapping::default()
        } else {
            AnalyzerMapping {
                index_analyzer: opt_text_from_value(parts[3].clone())?,
                search_analyzer: opt_text_from_value(parts[4].clone())?,
                normalizer: opt_text_from_value(parts[5].clone())?,
            }
        };
        mapping.insert(
            field,
            FieldMapping {
                field_type,
                stored,
                faceted,
                analysis,
            },
        );
    }
    Ok(mapping)
}

/// Encode a search [`Mapping`] to the canonical CBOR the facade accepts (the inverse of
/// [`search_mapping_from_cbor`]): a map of field name to `[type_tag, stored, faceted, index_analyzer,
/// search_analyzer, normalizer]` (the 6-field form, which round-trips any analyzer configuration).
pub fn search_mapping_cbor(mapping: &Mapping) -> Vec<u8> {
    cbor::encode(&CborValue::Map(
        mapping
            .iter()
            .map(|(field, fm)| {
                (
                    CborValue::Text(field.clone()),
                    CborValue::Array(vec![
                        CborValue::Uint(fm.field_type.tag()),
                        CborValue::Bool(fm.stored),
                        CborValue::Bool(fm.faceted),
                        opt_text_value(&fm.analysis.index_analyzer),
                        opt_text_value(&fm.analysis.search_analyzer),
                        opt_text_value(&fm.analysis.normalizer),
                    ]),
                )
            })
            .collect(),
    ))
}

fn opt_text_value(value: &Option<String>) -> CborValue {
    match value {
        Some(value) => CborValue::Text(value.clone()),
        None => CborValue::Null,
    }
}

fn opt_text_from_value(value: CborValue) -> Result<Option<String>> {
    match value {
        CborValue::Null => Ok(None),
        CborValue::Text(value) => Ok(Some(value)),
        _ => Err(LoomError::invalid(
            "search analyzer field must be text or null",
        )),
    }
}

fn text_list_from_value(value: CborValue, what: &str) -> Result<Vec<String>> {
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid(format!("{what} must be an array")));
    };
    items
        .into_iter()
        .map(|item| match item {
            CborValue::Text(text) => Ok(text),
            _ => Err(LoomError::invalid(format!("{what} entries must be text"))),
        })
        .collect()
}

fn aggregations_from_value(value: CborValue) -> Result<Vec<AggregationRequest>> {
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid(
            "search request aggregations must be an array",
        ));
    };
    items
        .into_iter()
        .map(|item| {
            let CborValue::Array(parts) = item else {
                return Err(LoomError::invalid(
                    "search aggregation request must be an array",
                ));
            };
            if parts.len() != 3 {
                return Err(LoomError::invalid(
                    "search aggregation request must have tag, name, and field",
                ));
            }
            let tag = cbor::as_uint(parts[0].clone())?;
            let name = cbor::as_text(parts[1].clone())?;
            let field = cbor::as_text(parts[2].clone())?;
            match tag {
                0 => Ok(AggregationRequest::Terms { name, field }),
                1 => Ok(AggregationRequest::ValueCount { name, field }),
                other => Err(LoomError::invalid(format!(
                    "unknown search aggregation tag {other}"
                ))),
            }
        })
        .collect()
}

fn highlights_to_cbor(highlights: &BTreeMap<String, Vec<String>>) -> CborValue {
    CborValue::Map(
        highlights
            .iter()
            .map(|(field, snippets)| {
                (
                    CborValue::Text(field.clone()),
                    CborValue::Array(snippets.iter().cloned().map(CborValue::Text).collect()),
                )
            })
            .collect(),
    )
}

fn facets_to_cbor(facets: &BTreeMap<String, Vec<FacetBucket>>) -> CborValue {
    CborValue::Map(
        facets
            .iter()
            .map(|(field, buckets)| {
                (
                    CborValue::Text(field.clone()),
                    CborValue::Array(
                        buckets
                            .iter()
                            .map(|bucket| {
                                CborValue::Array(vec![
                                    CborValue::Bytes(bucket.value.clone()),
                                    CborValue::Uint(bucket.count),
                                ])
                            })
                            .collect(),
                    ),
                )
            })
            .collect(),
    )
}

fn aggregations_to_cbor(aggregations: &BTreeMap<String, AggregationResult>) -> CborValue {
    CborValue::Map(
        aggregations
            .iter()
            .map(|(name, result)| {
                let value = match result {
                    AggregationResult::Buckets(buckets) => CborValue::Array(vec![
                        CborValue::Uint(0),
                        CborValue::Array(
                            buckets
                                .iter()
                                .map(|bucket| {
                                    CborValue::Array(vec![
                                        CborValue::Bytes(bucket.value.clone()),
                                        CborValue::Uint(bucket.count),
                                    ])
                                })
                                .collect(),
                        ),
                    ]),
                    AggregationResult::Count(count) => {
                        CborValue::Array(vec![CborValue::Uint(1), CborValue::Uint(*count)])
                    }
                };
                (CborValue::Text(name.clone()), value)
            })
            .collect(),
    )
}

pub fn search_document_from_cbor(bytes: &[u8]) -> Result<Document> {
    let CborValue::Map(pairs) = cbor::decode(bytes)? else {
        return Err(LoomError::invalid("search document must be a CBOR map"));
    };
    let mut doc = Document::new();
    for (key, value) in pairs {
        let CborValue::Text(field) = key else {
            return Err(LoomError::invalid(
                "search document field name must be text",
            ));
        };
        let value = match value {
            CborValue::Text(text) => FieldValue::Text(text),
            CborValue::Bytes(bytes) => FieldValue::Bytes(bytes),
            _ => {
                return Err(LoomError::invalid(
                    "search document value must be text or bytes",
                ));
            }
        };
        doc.insert(field, value);
    }
    Ok(doc)
}

pub fn search_document_cbor(doc: &Document) -> Vec<u8> {
    cbor::encode(&CborValue::Map(
        doc.iter()
            .map(|(field, value)| {
                let value = match value {
                    FieldValue::Text(text) => CborValue::Text(text.clone()),
                    FieldValue::Bytes(bytes) => CborValue::Bytes(bytes.clone()),
                };
                (CborValue::Text(field.clone()), value)
            })
            .collect(),
    ))
}

fn search_opt_bytes(value: Option<CborValue>) -> Result<Option<Vec<u8>>> {
    match value {
        Some(CborValue::Null) | None => Ok(None),
        Some(CborValue::Bytes(bytes)) => Ok(Some(bytes)),
        _ => Err(LoomError::invalid(
            "search range bound must be bytes or null",
        )),
    }
}

fn search_query_from_value(value: CborValue) -> Result<Query> {
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid("search query node must be a CBOR array"));
    };
    let mut iter = items.into_iter();
    let tag = match iter.next() {
        Some(CborValue::Uint(tag)) => tag,
        _ => return Err(LoomError::invalid("search query tag must be uint")),
    };
    let text = |value: Option<CborValue>, what: &str| match value {
        Some(CborValue::Text(text)) => Ok(text),
        _ => Err(LoomError::invalid(format!(
            "search query {what} must be text"
        ))),
    };
    match tag {
        5 => Ok(Query::MatchAll),
        0 => Ok(Query::Match {
            field: text(iter.next(), "Match field")?,
            text: text(iter.next(), "Match text")?,
        }),
        1 => {
            let field = text(iter.next(), "Term field")?;
            let value = match iter.next() {
                Some(CborValue::Bytes(bytes)) => bytes,
                Some(CborValue::Text(text)) => text.into_bytes(),
                _ => return Err(LoomError::invalid("search Term value must be bytes")),
            };
            Ok(Query::Term { field, value })
        }
        2 => {
            let field = text(iter.next(), "Phrase field")?;
            let terms = match iter.next() {
                Some(CborValue::Array(terms)) => terms
                    .into_iter()
                    .map(|term| match term {
                        CborValue::Text(term) => Ok(term),
                        _ => Err(LoomError::invalid("search Phrase term must be text")),
                    })
                    .collect::<Result<Vec<_>>>()?,
                _ => return Err(LoomError::invalid("search Phrase terms must be an array")),
            };
            let slop = match iter.next() {
                Some(CborValue::Uint(slop)) => u32::try_from(slop)
                    .map_err(|_| LoomError::invalid("search Phrase slop out of range"))?,
                _ => return Err(LoomError::invalid("search Phrase slop must be uint")),
            };
            Ok(Query::Phrase { field, terms, slop })
        }
        3 => {
            let field = text(iter.next(), "Range field")?;
            let lower = search_opt_bytes(iter.next())?;
            let upper = search_opt_bytes(iter.next())?;
            let include_lower = matches!(iter.next(), Some(CborValue::Bool(true)));
            let include_upper = matches!(iter.next(), Some(CborValue::Bool(true)));
            Ok(Query::Range {
                field,
                lower,
                upper,
                include_lower,
                include_upper,
            })
        }
        4 => {
            let list = |value: Option<CborValue>, what: &str| match value {
                Some(CborValue::Array(queries)) => queries
                    .into_iter()
                    .map(search_query_from_value)
                    .collect::<Result<Vec<_>>>(),
                _ => Err(LoomError::invalid(format!(
                    "search Bool {what} must be an array"
                ))),
            };
            Ok(Query::Bool {
                must: list(iter.next(), "must")?,
                should: list(iter.next(), "should")?,
                must_not: list(iter.next(), "must_not")?,
            })
        }
        6 => {
            let field = text(iter.next(), "Prefix field")?;
            let value = match iter.next() {
                Some(CborValue::Bytes(bytes)) => bytes,
                Some(CborValue::Text(text)) => text.into_bytes(),
                _ => return Err(LoomError::invalid("search Prefix value must be bytes")),
            };
            Ok(Query::Prefix { field, value })
        }
        7 => {
            let field = text(iter.next(), "Wildcard field")?;
            let pattern = match iter.next() {
                Some(CborValue::Bytes(bytes)) => bytes,
                Some(CborValue::Text(text)) => text.into_bytes(),
                _ => return Err(LoomError::invalid("search Wildcard pattern must be bytes")),
            };
            Ok(Query::Wildcard { field, pattern })
        }
        8 => {
            let field = text(iter.next(), "Fuzzy field")?;
            let text = text(iter.next(), "Fuzzy text")?;
            let max_distance = match iter.next() {
                Some(CborValue::Uint(distance)) => u32::try_from(distance)
                    .map_err(|_| LoomError::invalid("search Fuzzy distance out of range"))?,
                _ => return Err(LoomError::invalid("search Fuzzy distance must be uint")),
            };
            Ok(Query::Fuzzy {
                field,
                text,
                max_distance,
            })
        }
        9 => {
            let field = text(iter.next(), "Similar field")?;
            let text = text(iter.next(), "Similar text")?;
            let min_should_match = match iter.next() {
                Some(CborValue::Uint(value)) => u32::try_from(value).map_err(|_| {
                    LoomError::invalid("search Similar min_should_match out of range")
                })?,
                _ => {
                    return Err(LoomError::invalid(
                        "search Similar min_should_match must be uint",
                    ));
                }
            };
            Ok(Query::Similar {
                field,
                text,
                min_should_match,
            })
        }
        other => Err(LoomError::invalid(format!(
            "unknown search query tag {other}"
        ))),
    }
}

pub fn search_request_from_cbor(bytes: &[u8]) -> Result<QueryRequest> {
    let CborValue::Array(items) = cbor::decode(bytes)? else {
        return Err(LoomError::invalid("search request must be a CBOR array"));
    };
    let mut iter = items.into_iter();
    let query = search_query_from_value(
        iter.next()
            .ok_or_else(|| LoomError::invalid("search request is missing its query"))?,
    )?;
    let limit = match iter.next() {
        Some(CborValue::Uint(limit)) => {
            u32::try_from(limit).map_err(|_| LoomError::invalid("search limit out of range"))?
        }
        _ => return Err(LoomError::invalid("search request limit must be uint")),
    };
    let offset = match iter.next() {
        Some(CborValue::Uint(offset)) => {
            u32::try_from(offset).map_err(|_| LoomError::invalid("search offset out of range"))?
        }
        _ => return Err(LoomError::invalid("search request offset must be uint")),
    };
    let facets = match iter.next() {
        Some(value) => text_list_from_value(value, "search request facets")?,
        None => Vec::new(),
    };
    let highlight = match iter.next() {
        Some(value) => text_list_from_value(value, "search request highlight")?,
        None => Vec::new(),
    };
    let aggregations = match iter.next() {
        Some(value) => aggregations_from_value(value)?,
        None => Vec::new(),
    };
    if iter.next().is_some() {
        return Err(LoomError::invalid("search request has extra fields"));
    }
    Ok(QueryRequest {
        query,
        limit,
        offset,
        facets,
        highlight,
        aggregations,
    })
}

pub fn search_response_cbor(response: &QueryResponse) -> Vec<u8> {
    cbor::encode(&CborValue::Array(vec![
        CborValue::Bool(response.reduced),
        CborValue::Array(
            response
                .hits
                .iter()
                .map(|hit| {
                    CborValue::Array(vec![
                        CborValue::Bytes(hit.id.clone()),
                        cell_value(&TabularValue::F32(hit.score)),
                        highlights_to_cbor(&hit.highlights),
                    ])
                })
                .collect(),
        ),
        facets_to_cbor(&response.facets),
        aggregations_to_cbor(&response.aggregations),
    ]))
}

pub fn search_ids_cbor(ids: Vec<Vec<u8>>) -> Vec<u8> {
    cbor::encode(&CborValue::Array(
        ids.into_iter().map(CborValue::Bytes).collect(),
    ))
}

/// A native search engine for a committed [`SearchCollection`], implemented outside `loom-core`.
pub trait SearchEngine {
    /// Execute `request` against `collection`. Native engines may use richer analyzers and ranking.
    fn query(&self, collection: &SearchCollection, request: &QueryRequest)
    -> Result<QueryResponse>;
}

/// Whitespace/punctuation tokenizer: lower-cased alphanumeric runs. Deterministic, `wasm32`-clean.
fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The analyzed tokens of a field value (text is tokenized; bytes are a single opaque token).
fn field_tokens(value: &FieldValue) -> Vec<String> {
    match value {
        FieldValue::Text(s) => tokenize(s),
        FieldValue::Bytes(_) => Vec::new(),
    }
}

/// Whether `terms` appear in order within `have`, allowing up to `slop` extra tokens between
/// consecutive matches.
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
            match have[pos..limit].iter().position(|t| t == term) {
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
                current[1..(value.len() + 1)].copy_from_slice(&previous[..value.len()]);
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

fn collection_path(name: &str) -> String {
    facet_path(FacetKind::Search, name)
}

fn collection_key(collection: &str) -> String {
    hex::encode(collection.as_bytes())
}

pub(crate) fn source_document_dir(collection: &str) -> String {
    facet_path(
        FacetKind::Search,
        &format!(".documents/{}", collection_key(collection)),
    )
}

pub(crate) fn source_document_path(collection: &str, digest: &Digest) -> String {
    facet_path(
        FacetKind::Search,
        &format!(
            ".documents/{}/{}",
            collection_key(collection),
            digest.to_hex()
        ),
    )
}

fn alias_dir() -> String {
    facet_path(FacetKind::Search, ".aliases")
}

fn alias_path(name: &str) -> String {
    facet_path(FacetKind::Search, &format!(".aliases/{name}"))
}

fn validate_alias_name(name: &str) -> Result<()> {
    if name.is_empty() || name.contains('/') {
        return Err(LoomError::invalid(
            "search alias name must be non-empty and contain no slash",
        ));
    }
    Ok(())
}

fn alias_to_cbor(alias: &SearchAlias) -> Vec<u8> {
    cbor::encode(&CborValue::Array(
        alias
            .targets
            .iter()
            .map(|target| {
                CborValue::Array(vec![
                    CborValue::Text(target.collection.clone()),
                    CborValue::Bool(target.is_write_index),
                ])
            })
            .collect(),
    ))
}

fn alias_from_cbor(bytes: &[u8]) -> Result<SearchAlias> {
    let CborValue::Array(items) = cbor::decode(bytes)? else {
        return Err(LoomError::corrupt("search alias must be a CBOR array"));
    };
    let mut targets = Vec::new();
    for item in items {
        let CborValue::Array(mut parts) = item else {
            return Err(LoomError::corrupt("search alias target must be an array"));
        };
        if parts.len() != 2 {
            return Err(LoomError::corrupt(
                "search alias target must have collection and write flag",
            ));
        }
        let write = match parts.pop() {
            Some(CborValue::Bool(value)) => value,
            _ => return Err(LoomError::corrupt("search alias write flag must be bool")),
        };
        let collection = match parts.pop() {
            Some(CborValue::Text(value)) => value,
            _ => {
                return Err(LoomError::corrupt("search alias collection must be text"));
            }
        };
        targets.push(SearchAliasTarget {
            collection,
            is_write_index: write,
        });
    }
    Ok(SearchAlias { targets })
}

fn encode_structured_search_root<S: ObjectStore>(
    loom: &Loom<S>,
    collection_name: &str,
    collection: &SearchCollection,
) -> (Vec<u8>, BTreeMap<String, Vec<u8>>) {
    let algo = loom.store().digest_algo();
    let mapping =
        cbor::decode(&search_mapping_cbor(collection.mapping())).expect("mapping cbor encodes");
    let mut components = BTreeMap::new();
    let docs = collection
        .docs
        .iter()
        .map(|(id, doc)| {
            let bytes = search_document_cbor(doc);
            let digest = content_address_with(algo, &bytes);
            components.insert(
                source_document_path(collection_name, &digest),
                bytes.clone(),
            );
            CborValue::Array(vec![
                CborValue::Bytes(id.clone()),
                CborValue::Bytes(digest.bytes().to_vec()),
                CborValue::Uint(bytes.len() as u64),
            ])
        })
        .collect();
    (
        cbor::encode(&CborValue::Array(vec![
            CborValue::Text(STRUCTURED_SEARCH_ROOT_SCHEMA.to_string()),
            CborValue::Uint(u64::from(algo.code())),
            mapping,
            CborValue::Array(docs),
        ])),
        components,
    )
}

pub(crate) fn encode_structured_search_storage<S: ObjectStore>(
    loom: &Loom<S>,
    collection_name: &str,
    collection: &SearchCollection,
) -> (Vec<u8>, BTreeMap<String, Vec<u8>>) {
    encode_structured_search_root(loom, collection_name, collection)
}

fn digest_from_bytes(algo: crate::digest::Algo, bytes: Vec<u8>) -> Result<Digest> {
    let bytes: [u8; DIGEST_LEN] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| LoomError::corrupt("search document digest field is not 32 bytes"))?;
    Ok(Digest::of(algo, bytes))
}

fn decode_structured_search_root_with_components<F>(
    algo: crate::digest::Algo,
    bytes: &[u8],
    mut read_component: F,
) -> Result<Option<SearchCollection>>
where
    F: FnMut(&Digest) -> Result<Vec<u8>>,
{
    let root = cbor::decode_array(bytes)?;
    match root.first() {
        Some(CborValue::Text(schema)) if schema == STRUCTURED_SEARCH_ROOT_SCHEMA => {}
        _ => return Ok(None),
    }
    let mut fields = cbor::Fields::new(root);
    let schema = fields.text()?;
    if schema != STRUCTURED_SEARCH_ROOT_SCHEMA {
        return Ok(None);
    }
    let root_algo = crate::digest::Algo::from_code(cbor::u8_from(fields.uint()?)?)?;
    if root_algo != algo {
        return Err(LoomError::corrupt(
            "search structured root digest profile mismatch",
        ));
    }
    let mapping = search_mapping_from_cbor(&cbor::encode(&fields.next_field()?))?;
    let docs_raw = fields.array()?;
    fields.end()?;

    let mut docs = BTreeMap::new();
    for item in docs_raw {
        let mut entry = cbor::Fields::new(cbor::as_array(item)?);
        let id = entry.bytes()?;
        let digest = digest_from_bytes(algo, entry.bytes()?)?;
        let len = entry.uint()?;
        entry.end()?;
        let bytes = read_component(&digest)?;
        if bytes.len() as u64 != len {
            return Err(LoomError::integrity_failure(
                "search document length mismatch",
            ));
        }
        let actual = content_address_with(algo, &bytes);
        if actual != digest {
            return Err(LoomError::integrity_failure(
                "search document digest mismatch",
            ));
        }
        let doc = search_document_from_cbor(&bytes)?;
        if docs.insert(id, doc).is_some() {
            return Err(LoomError::corrupt(
                "duplicate search structured root document id",
            ));
        }
    }
    Ok(Some(SearchCollection { mapping, docs }))
}

pub(crate) fn decode_search_storage_with_components<F>(
    algo: crate::digest::Algo,
    bytes: &[u8],
    read_component: F,
) -> Result<SearchCollection>
where
    F: FnMut(&Digest) -> Result<Vec<u8>>,
{
    if let Some(collection) =
        decode_structured_search_root_with_components(algo, bytes, read_component)?
    {
        Ok(collection)
    } else {
        if let Ok(root) = cbor::decode_array(bytes)
            && matches!(
                root.first(),
                Some(CborValue::Text(schema))
                    if schema.starts_with(STRUCTURED_SEARCH_ROOT_SCHEMA_PREFIX)
            )
        {
            return Err(LoomError::corrupt(
                "unsupported search structured root schema",
            ));
        }
        SearchCollection::decode(bytes)
    }
}

pub(crate) fn merge_search_collections(
    base: Option<&SearchCollection>,
    ours: &SearchCollection,
    theirs: &SearchCollection,
) -> Option<SearchCollection> {
    let empty_mapping = Mapping::new();
    let base_mapping = base
        .map(SearchCollection::mapping)
        .unwrap_or(&empty_mapping);
    if ours.mapping() != base_mapping || theirs.mapping() != base_mapping {
        return None;
    }
    let empty = BTreeMap::new();
    let base_docs = base.map(SearchCollection::docs).unwrap_or(&empty);
    let mut ids = std::collections::BTreeSet::new();
    ids.extend(base_docs.keys().cloned());
    ids.extend(ours.docs().keys().cloned());
    ids.extend(theirs.docs().keys().cloned());
    let mut docs = BTreeMap::new();
    for id in ids {
        let base_doc = base_docs.get(&id);
        let our_doc = ours.docs().get(&id);
        let their_doc = theirs.docs().get(&id);
        let merged = if our_doc == their_doc {
            our_doc
        } else if base_doc == our_doc {
            their_doc
        } else if base_doc == their_doc {
            our_doc
        } else {
            return None;
        };
        if let Some(doc) = merged {
            docs.insert(id, doc.clone());
        }
    }
    Some(SearchCollection::from_parts(ours.mapping().clone(), docs))
}

fn decode_search_storage<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    bytes: &[u8],
) -> Result<SearchCollection> {
    if let Some(collection) = decode_structured_search_root_with_components(
        loom.store().digest_algo(),
        bytes,
        |digest| loom.read_file_reserved(ns, &source_document_path(collection, digest)),
    )? {
        Ok(collection)
    } else {
        SearchCollection::decode(bytes)
    }
}

fn stage_search_collection<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    collection: &SearchCollection,
) -> Result<()> {
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Search), true)?;
    loom.create_directory_reserved(ns, &source_document_dir(name), true)?;
    let (root, components) = encode_structured_search_root(loom, name, collection);
    for (path, bytes) in &components {
        loom.write_file_reserved(ns, path, bytes, 0o100644)?;
    }
    let document_prefix = format!("{}/", source_document_dir(name));
    for path in loom.staged_paths(ns) {
        if path.starts_with(&document_prefix) && !components.contains_key(&path) {
            loom.remove_file_reserved(ns, &path)?;
        }
    }
    loom.write_file_reserved(ns, &collection_path(name), &root, 0o100644)
}

/// Stage `collection` under `name` in `ns`'s search facet as a structured source root.
pub fn put_search<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    collection: &SearchCollection,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Search, name, AclRight::Write)?;
    stage_search_collection(loom, ns, name, collection)
}

/// Load the collection named `name` from `ns`'s current working tree, or `NOT_FOUND`.
pub fn get_search<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<SearchCollection> {
    loom.authorize_collection(ns, FacetKind::Search, name, AclRight::Read)?;
    decode_search_storage(
        loom,
        ns,
        name,
        &loom.read_file_reserved(ns, &collection_path(name))?,
    )
}

/// Remove a search collection and prune alias records that target it. Returns whether it existed.
pub fn search_drop<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<bool> {
    loom.authorize_collection(ns, FacetKind::Search, name, AclRight::Write)?;
    let path = collection_path(name);
    match loom.read_file_reserved(ns, &path) {
        Ok(bytes) => {
            decode_search_storage(loom, ns, name, &bytes)?;
        }
        Err(err) if err.code == Code::NotFound => return Ok(false),
        Err(err) => return Err(err),
    }
    loom.remove_file_reserved(ns, &path)?;
    match loom.walk(ns, &source_document_dir(name)) {
        Ok(paths) => {
            for path in paths {
                loom.remove_file_reserved(ns, &path)?;
            }
        }
        Err(err) if err.code == Code::NotFound => {}
        Err(err) => return Err(err),
    }
    for alias_name in search_aliases(loom, ns)? {
        let Some(mut alias) = search_alias_get(loom, ns, &alias_name)? else {
            continue;
        };
        alias.targets.retain(|target| target.collection != name);
        if alias.targets.is_empty() {
            loom.remove_file_reserved(ns, &alias_path(&alias_name))?;
        } else {
            search_alias_set(loom, ns, &alias_name, alias)?;
        }
    }
    Ok(true)
}

pub fn search_collections<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId) -> Result<Vec<String>> {
    let root = facet_path(FacetKind::Search, "");
    let prefix = format!("{root}/");
    let mut collections = Vec::new();
    let paths = match loom.walk(ns, &root) {
        Ok(paths) => paths,
        Err(err) if err.code == Code::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    for path in paths {
        let Some(name) = path.strip_prefix(&prefix) else {
            continue;
        };
        if name.starts_with('.') || name.contains('/') {
            continue;
        }
        if loom
            .authorize_collection(ns, FacetKind::Search, name, AclRight::Read)
            .is_err()
        {
            continue;
        }
        if decode_search_storage(loom, ns, name, &loom.read_file_reserved(ns, &path)?).is_ok() {
            collections.push(name.to_string());
        }
    }
    Ok(collections)
}

pub fn search_alias_set<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    alias: SearchAlias,
) -> Result<()> {
    validate_alias_name(name)?;
    if alias.targets.is_empty() {
        return Err(LoomError::invalid(
            "search alias requires at least one target",
        ));
    }
    for target in &alias.targets {
        loom.authorize_collection(ns, FacetKind::Search, &target.collection, AclRight::Write)?;
        get_search(loom, ns, &target.collection)?;
    }
    loom.create_directory_reserved(ns, &alias_dir(), true)?;
    loom.write_file_reserved(ns, &alias_path(name), &alias_to_cbor(&alias), 0o100644)
}

pub fn search_alias_get<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<Option<SearchAlias>> {
    validate_alias_name(name)?;
    match loom.read_file_reserved(ns, &alias_path(name)) {
        Ok(bytes) => {
            let alias = alias_from_cbor(&bytes)?;
            for target in &alias.targets {
                loom.authorize_collection(
                    ns,
                    FacetKind::Search,
                    &target.collection,
                    AclRight::Read,
                )?;
            }
            Ok(Some(alias))
        }
        Err(err) if err.code == Code::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

pub fn search_alias_delete<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<bool> {
    validate_alias_name(name)?;
    let Some(alias) = search_alias_get(loom, ns, name)? else {
        return Ok(false);
    };
    for target in &alias.targets {
        loom.authorize_collection(ns, FacetKind::Search, &target.collection, AclRight::Write)?;
    }
    loom.remove_file_reserved(ns, &alias_path(name))?;
    Ok(true)
}

pub fn search_aliases<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId) -> Result<Vec<String>> {
    let root = alias_dir();
    let prefix = format!("{root}/");
    let paths = match loom.walk(ns, &root) {
        Ok(paths) => paths,
        Err(err) if err.code == Code::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut names = Vec::new();
    for path in paths {
        let Some(name) = path.strip_prefix(&prefix) else {
            continue;
        };
        if name.contains('/') {
            continue;
        }
        if search_alias_get(loom, ns, name)?.is_some() {
            names.push(name.to_string());
        }
    }
    Ok(names)
}

/// Digest of the committed search source that native indexes must be stamped against.
pub fn search_source_digest<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<crate::Digest> {
    loom.authorize_collection(ns, FacetKind::Search, name, AclRight::Read)?;
    let collection = get_search(loom, ns, name)?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"loom-search-derived-source-v1");
    bytes.extend_from_slice(&collection.encode());
    Ok(crate::Digest::hash(loom.store().digest_algo(), &bytes))
}

/// Create an empty search collection `name` in `ns` with field `mapping`, staging it. `CONFLICT` if a
/// collection already exists under `name` (use `search_remap` to change the mapping).
pub fn search_create<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    mapping: Mapping,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Search, name, AclRight::Write)?;
    match loom.read_file_reserved(ns, &collection_path(name)) {
        Ok(_) => Err(LoomError::new(
            Code::Conflict,
            format!("search collection {name:?} already exists"),
        )),
        Err(e) if e.code == Code::NotFound => {
            stage_search_collection(loom, ns, name, &SearchCollection::new(mapping))
        }
        Err(e) => Err(e),
    }
}

/// Insert or replace the document at `id` in collection `name`, staging the result. `NOT_FOUND` if the
/// collection was never created.
pub fn search_index<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: Vec<u8>,
    doc: Document,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Search, name, AclRight::Write)?;
    let mut collection = decode_search_storage(
        loom,
        ns,
        name,
        &loom.read_file_reserved(ns, &collection_path(name))?,
    )?;
    collection.index(id, doc);
    stage_search_collection(loom, ns, name, &collection)
}

/// The document at `id` in collection `name`, or `None`. `NOT_FOUND` if the collection does not exist.
pub fn search_get<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &[u8],
) -> Result<Option<Document>> {
    loom.authorize_collection(ns, FacetKind::Search, name, AclRight::Read)?;
    Ok(get_search(loom, ns, name)?.get(id).cloned())
}

/// Remove `id` from collection `name`; returns whether it was present. `NOT_FOUND` if the collection
/// does not exist.
pub fn search_delete<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &[u8],
) -> Result<bool> {
    loom.authorize_collection(ns, FacetKind::Search, name, AclRight::Write)?;
    let mut collection = decode_search_storage(
        loom,
        ns,
        name,
        &loom.read_file_reserved(ns, &collection_path(name))?,
    )?;
    let present = collection.delete(id);
    if present {
        stage_search_collection(loom, ns, name, &collection)?;
    }
    Ok(present)
}

/// Document ids in collection `name`, optionally restricted to those starting with `prefix`.
/// `NOT_FOUND` if the collection does not exist.
pub fn search_ids<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    prefix: Option<&[u8]>,
) -> Result<Vec<Vec<u8>>> {
    loom.authorize_collection(ns, FacetKind::Search, name, AclRight::Read)?;
    Ok(get_search(loom, ns, name)?.ids(prefix))
}

/// Replace the field mapping of collection `name`, staging the result. `NOT_FOUND` if the collection
/// does not exist.
pub fn search_remap<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    mapping: Mapping,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Search, name, AclRight::Write)?;
    let mut collection = decode_search_storage(
        loom,
        ns,
        name,
        &loom.read_file_reserved(ns, &collection_path(name))?,
    )?;
    collection.remap(mapping);
    stage_search_collection(loom, ns, name, &collection)
}

/// Run the portable linear-scan query over collection `name` (see [`SearchCollection::query`]).
/// `NOT_FOUND` if the collection does not exist; `NO_SUCH_FIELD` for an unmapped query field.
pub fn search_query<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    request: &QueryRequest,
) -> Result<QueryResponse> {
    loom.authorize_collection(ns, FacetKind::Search, name, AclRight::Read)?;
    get_search(loom, ns, name)?.query(request)
}

/// Run a search query through an injected native engine, or the portable reduced path when no engine
/// is available.
pub fn search_query_auto<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    request: &QueryRequest,
    engine: Option<&dyn SearchEngine>,
) -> Result<QueryResponse> {
    loom.authorize_collection(ns, FacetKind::Search, name, AclRight::Read)?;
    let collection = get_search(loom, ns, name)?;
    match engine {
        Some(engine) => engine.query(&collection, request),
        None => collection.query(request),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::memory::MemoryStore;
    use crate::workspace::WorkspaceId;

    fn mapping() -> Mapping {
        let mut m = Mapping::new();
        m.insert("title".to_string(), FieldMapping::text());
        m.insert("lang".to_string(), FieldMapping::keyword());
        m
    }

    fn doc(title: &str, lang: &str) -> Document {
        let mut d = Document::new();
        d.insert("title".to_string(), FieldValue::Text(title.to_string()));
        d.insert(
            "lang".to_string(),
            FieldValue::Bytes(lang.as_bytes().to_vec()),
        );
        d
    }

    struct NativeLikeSearchEngine;

    impl SearchEngine for NativeLikeSearchEngine {
        fn query(
            &self,
            collection: &SearchCollection,
            request: &QueryRequest,
        ) -> Result<QueryResponse> {
            let mut response = collection.query(request)?;
            response.reduced = false;
            Ok(response)
        }
    }

    #[test]
    fn encode_round_trips() {
        let mut c = SearchCollection::new(mapping());
        c.index(b"a".to_vec(), doc("the quick brown fox", "en"));
        c.index(b"b".to_vec(), doc("le renard brun", "fr"));
        let d = SearchCollection::decode(&c.encode()).unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d.encode(), c.encode());
        assert_eq!(d.get(b"a"), c.get(b"a"));
    }

    #[test]
    fn linear_match_term_and_bool() {
        let mut c = SearchCollection::new(mapping());
        c.index(b"a".to_vec(), doc("the quick brown fox", "en"));
        c.index(b"b".to_vec(), doc("quick green turtle", "en"));
        c.index(b"c".to_vec(), doc("le renard brun", "fr"));

        // Match scores by token overlap; "quick" hits a and b.
        let resp = c
            .query(&QueryRequest::new(
                Query::Match {
                    field: "title".into(),
                    text: "quick".into(),
                },
                0,
                0,
            ))
            .unwrap();
        assert!(resp.reduced);
        assert_eq!(
            resp.hits.iter().map(|h| h.id.clone()).collect::<Vec<_>>(),
            vec![b"a".to_vec(), b"b".to_vec()]
        );

        // Bool: must match "quick" in title AND keyword lang == "en".
        let resp = c
            .query(&QueryRequest::new(
                Query::Bool {
                    must: vec![
                        Query::Match {
                            field: "title".into(),
                            text: "quick".into(),
                        },
                        Query::Term {
                            field: "lang".into(),
                            value: b"en".to_vec(),
                        },
                    ],
                    should: vec![],
                    must_not: vec![],
                },
                0,
                0,
            ))
            .unwrap();
        assert_eq!(resp.hits.len(), 2);
    }

    #[test]
    fn phrase_and_unknown_field() {
        let mut c = SearchCollection::new(mapping());
        c.index(b"a".to_vec(), doc("the quick brown fox", "en"));
        let hit = c
            .query(&QueryRequest::new(
                Query::Phrase {
                    field: "title".into(),
                    terms: vec!["quick".into(), "brown".into()],
                    slop: 0,
                },
                0,
                0,
            ))
            .unwrap();
        assert_eq!(hit.hits.len(), 1);
        // An unmapped field is a NO_SUCH_FIELD error.
        assert_eq!(
            c.query(&QueryRequest::new(
                Query::Match {
                    field: "body".into(),
                    text: "x".into(),
                },
                0,
                0,
            ))
            .unwrap_err()
            .code,
            Code::NoSuchField
        );
    }

    #[test]
    fn extended_query_shapes_facets_highlights_and_aggregations() {
        let mut mapping = mapping();
        let mut lang = FieldMapping::keyword();
        lang.faceted = true;
        mapping.insert("lang".to_string(), lang);
        let mut c = SearchCollection::new(mapping);
        c.index(b"a".to_vec(), doc("the quick brown fox", "en"));
        c.index(b"b".to_vec(), doc("quick green turtle", "en"));
        c.index(b"c".to_vec(), doc("le renard brun", "fr"));

        let mut request = QueryRequest::new(Query::MatchAll, 0, 0);
        request.facets.push("lang".to_string());
        request.highlight.push("title".to_string());
        request.aggregations.push(AggregationRequest::ValueCount {
            name: "titles".to_string(),
            field: "title".to_string(),
        });
        request.aggregations.push(AggregationRequest::Terms {
            name: "langs".to_string(),
            field: "lang".to_string(),
        });
        let response = c.query(&request).unwrap();
        assert_eq!(response.hits.len(), 3);
        assert!(response.hits[0].highlights.contains_key("title"));
        assert_eq!(
            response.facets["lang"]
                .iter()
                .map(|bucket| (bucket.value.clone(), bucket.count))
                .collect::<Vec<_>>(),
            vec![(b"en".to_vec(), 2), (b"fr".to_vec(), 1)]
        );
        assert_eq!(response.aggregations["titles"], AggregationResult::Count(3));
        assert_eq!(
            response.aggregations["langs"],
            AggregationResult::Buckets(vec![
                FacetBucket {
                    value: b"en".to_vec(),
                    count: 2,
                },
                FacetBucket {
                    value: b"fr".to_vec(),
                    count: 1,
                },
            ])
        );

        let prefix = c
            .query(&QueryRequest::new(
                Query::Prefix {
                    field: "lang".to_string(),
                    value: b"e".to_vec(),
                },
                0,
                0,
            ))
            .unwrap();
        assert_eq!(prefix.hits.len(), 2);
        let wildcard = c
            .query(&QueryRequest::new(
                Query::Wildcard {
                    field: "title".to_string(),
                    pattern: b"*turtle".to_vec(),
                },
                0,
                0,
            ))
            .unwrap();
        assert_eq!(wildcard.hits[0].id, b"b".to_vec());
        let fuzzy = c
            .query(&QueryRequest::new(
                Query::Fuzzy {
                    field: "title".to_string(),
                    text: "quik".to_string(),
                    max_distance: 1,
                },
                0,
                0,
            ))
            .unwrap();
        assert_eq!(fuzzy.hits.len(), 2);
        let similar = c
            .query(&QueryRequest::new(
                Query::Similar {
                    field: "title".to_string(),
                    text: "quick fox".to_string(),
                    min_should_match: 2,
                },
                0,
                0,
            ))
            .unwrap();
        assert_eq!(similar.hits[0].id, b"a".to_vec());
    }

    #[test]
    fn facade_create_index_get_query() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Search, None, WorkspaceId::from_bytes([11; 16]))
            .unwrap();
        // Operations require an explicit create.
        assert_eq!(
            search_index(&mut loom, ns, "idx", b"a".to_vec(), doc("hi", "en"))
                .unwrap_err()
                .code,
            Code::NotFound
        );
        search_create(&mut loom, ns, "idx", mapping()).unwrap();
        assert_eq!(
            search_create(&mut loom, ns, "idx", mapping())
                .unwrap_err()
                .code,
            Code::Conflict
        );
        search_index(
            &mut loom,
            ns,
            "idx",
            b"a".to_vec(),
            doc("quick brown fox", "en"),
        )
        .unwrap();
        search_index(
            &mut loom,
            ns,
            "idx",
            b"b".to_vec(),
            doc("slow turtle", "en"),
        )
        .unwrap();
        assert_eq!(search_ids(&loom, ns, "idx", None).unwrap().len(), 2);
        assert!(search_get(&loom, ns, "idx", b"a").unwrap().is_some());
        let resp = search_query(
            &loom,
            ns,
            "idx",
            &QueryRequest::new(
                Query::Match {
                    field: "title".into(),
                    text: "fox".into(),
                },
                0,
                0,
            ),
        )
        .unwrap();
        assert_eq!(resp.hits.len(), 1);
        assert_eq!(resp.hits[0].id, b"a".to_vec());
        assert!(search_delete(&mut loom, ns, "idx", b"a").unwrap());
        assert!(!search_delete(&mut loom, ns, "idx", b"a").unwrap());
        search_create(&mut loom, ns, "idx2", mapping()).unwrap();
        search_alias_set(
            &mut loom,
            ns,
            "all",
            SearchAlias {
                targets: vec![
                    SearchAliasTarget {
                        collection: "idx".into(),
                        is_write_index: false,
                    },
                    SearchAliasTarget {
                        collection: "idx2".into(),
                        is_write_index: true,
                    },
                ],
            },
        )
        .unwrap();
        assert!(search_drop(&mut loom, ns, "idx").unwrap());
        assert!(matches!(
            get_search(&loom, ns, "idx").unwrap_err().code,
            Code::NotFound
        ));
        let alias = search_alias_get(&loom, ns, "all").unwrap().unwrap();
        assert_eq!(alias.targets.len(), 1);
        assert_eq!(alias.targets[0].collection, "idx2");
        assert!(search_drop(&mut loom, ns, "idx2").unwrap());
        assert!(search_alias_get(&loom, ns, "all").unwrap().is_none());
        assert!(!search_drop(&mut loom, ns, "idx2").unwrap());
    }

    #[test]
    fn structured_root_separates_mapping_and_document_components() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Search, None, WorkspaceId::from_bytes([14; 16]))
            .unwrap();
        search_create(&mut loom, ns, "idx", mapping()).unwrap();
        search_index(
            &mut loom,
            ns,
            "idx",
            b"a".to_vec(),
            doc("quick brown fox", "en"),
        )
        .unwrap();
        search_index(
            &mut loom,
            ns,
            "idx",
            b"b".to_vec(),
            doc("slow turtle", "en"),
        )
        .unwrap();
        let source_digest = search_source_digest(&loom, ns, "idx").unwrap();

        let root = loom
            .read_file_reserved(ns, &collection_path("idx"))
            .unwrap();
        assert!(
            !root
                .windows(b"quick brown fox".len())
                .any(|window| window == b"quick brown fox")
        );
        let CborValue::Array(root_fields) = cbor::decode(&root).unwrap() else {
            panic!("search root must be an array");
        };
        assert!(matches!(
            &root_fields[0],
            CborValue::Text(schema) if schema == STRUCTURED_SEARCH_ROOT_SCHEMA
        ));
        let CborValue::Array(entries) = &root_fields[3] else {
            panic!("search root entries must be an array");
        };
        assert_eq!(entries.len(), 2);
        for entry in entries {
            let mut fields = cbor::Fields::new(cbor::as_array(entry.clone()).unwrap());
            let _id = fields.bytes().unwrap();
            let digest =
                digest_from_bytes(loom.store().digest_algo(), fields.bytes().unwrap()).unwrap();
            let _len = fields.uint().unwrap();
            fields.end().unwrap();
            let component = loom
                .read_file_reserved(ns, &source_document_path("idx", &digest))
                .unwrap();
            assert!(!search_document_from_cbor(&component).unwrap().is_empty());
        }

        assert!(search_delete(&mut loom, ns, "idx", b"a").unwrap());
        assert!(search_get(&loom, ns, "idx", b"a").unwrap().is_none());
        assert_ne!(
            source_digest,
            search_source_digest(&loom, ns, "idx").unwrap()
        );
    }

    #[test]
    fn structured_root_rejects_duplicate_document_ids() {
        let doc_bytes = search_document_cbor(&doc("quick brown fox", "en"));
        let digest = content_address_with(crate::digest::Algo::Blake3, &doc_bytes);
        let mapping = cbor::decode(&search_mapping_cbor(&mapping())).unwrap();
        let entry = CborValue::Array(vec![
            CborValue::Bytes(b"a".to_vec()),
            CborValue::Bytes(digest.bytes().to_vec()),
            CborValue::Uint(doc_bytes.len() as u64),
        ]);
        let root = cbor::encode(&CborValue::Array(vec![
            CborValue::Text(STRUCTURED_SEARCH_ROOT_SCHEMA.to_string()),
            CborValue::Uint(u64::from(crate::digest::Algo::Blake3.code())),
            mapping,
            CborValue::Array(vec![entry.clone(), entry]),
        ]));
        let err = decode_structured_search_root_with_components(
            crate::digest::Algo::Blake3,
            &root,
            |_| Ok(doc_bytes.clone()),
        )
        .unwrap_err();
        assert_eq!(err.code, Code::CorruptObject);
    }

    #[test]
    fn structured_root_rejects_digest_profile_mismatch() {
        let doc_bytes = search_document_cbor(&doc("quick brown fox", "en"));
        let digest = content_address_with(crate::digest::Algo::Sha256, &doc_bytes);
        let mapping = cbor::decode(&search_mapping_cbor(&mapping())).unwrap();
        let root = cbor::encode(&CborValue::Array(vec![
            CborValue::Text(STRUCTURED_SEARCH_ROOT_SCHEMA.to_string()),
            CborValue::Uint(u64::from(crate::digest::Algo::Sha256.code())),
            mapping,
            CborValue::Array(vec![CborValue::Array(vec![
                CborValue::Bytes(b"a".to_vec()),
                CborValue::Bytes(digest.bytes().to_vec()),
                CborValue::Uint(doc_bytes.len() as u64),
            ])]),
        ]));
        let err = decode_structured_search_root_with_components(
            crate::digest::Algo::Blake3,
            &root,
            |_| Ok(doc_bytes.clone()),
        )
        .unwrap_err();
        assert_eq!(err.code, Code::CorruptObject);
    }

    #[test]
    fn structured_root_rejects_future_schema_as_unsupported_root() {
        let mapping = cbor::decode(&search_mapping_cbor(&mapping())).unwrap();
        let root = cbor::encode(&CborValue::Array(vec![
            CborValue::Text("loom.search.structured-collection-root.v2".to_string()),
            CborValue::Uint(u64::from(crate::digest::Algo::Blake3.code())),
            mapping,
            CborValue::Array(Vec::new()),
        ]));
        let err = decode_search_storage_with_components(crate::digest::Algo::Blake3, &root, |_| {
            panic!("future root schema must fail before reading components")
        })
        .unwrap_err();
        assert_eq!(err.code, Code::CorruptObject);
    }

    #[test]
    fn structured_root_rejects_component_length_mismatch() {
        let doc_bytes = search_document_cbor(&doc("quick brown fox", "en"));
        let digest = content_address_with(crate::digest::Algo::Blake3, &doc_bytes);
        let mapping = cbor::decode(&search_mapping_cbor(&mapping())).unwrap();
        let root = cbor::encode(&CborValue::Array(vec![
            CborValue::Text(STRUCTURED_SEARCH_ROOT_SCHEMA.to_string()),
            CborValue::Uint(u64::from(crate::digest::Algo::Blake3.code())),
            mapping,
            CborValue::Array(vec![CborValue::Array(vec![
                CborValue::Bytes(b"a".to_vec()),
                CborValue::Bytes(digest.bytes().to_vec()),
                CborValue::Uint(doc_bytes.len() as u64 + 1),
            ])]),
        ]));
        let err = decode_structured_search_root_with_components(
            crate::digest::Algo::Blake3,
            &root,
            |_| Ok(doc_bytes.clone()),
        )
        .unwrap_err();
        assert_eq!(err.code, Code::IntegrityFailure);
    }

    #[test]
    fn structured_root_rejects_component_digest_mismatch() {
        let doc_bytes = search_document_cbor(&doc("quick brown fox", "en"));
        let replacement = search_document_cbor(&doc("slow turtle", "en"));
        let digest = content_address_with(crate::digest::Algo::Blake3, &doc_bytes);
        let mapping = cbor::decode(&search_mapping_cbor(&mapping())).unwrap();
        let root = cbor::encode(&CborValue::Array(vec![
            CborValue::Text(STRUCTURED_SEARCH_ROOT_SCHEMA.to_string()),
            CborValue::Uint(u64::from(crate::digest::Algo::Blake3.code())),
            mapping,
            CborValue::Array(vec![CborValue::Array(vec![
                CborValue::Bytes(b"a".to_vec()),
                CborValue::Bytes(digest.bytes().to_vec()),
                CborValue::Uint(replacement.len() as u64),
            ])]),
        ]));
        let err = decode_structured_search_root_with_components(
            crate::digest::Algo::Blake3,
            &root,
            |_| Ok(replacement.clone()),
        )
        .unwrap_err();
        assert_eq!(err.code, Code::IntegrityFailure);
    }

    #[test]
    fn search_query_auto_uses_native_engine_when_injected() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Search,
                Some("native"),
                WorkspaceId::from_bytes([33; 16]),
            )
            .unwrap();
        search_create(&mut loom, ns, "docs", mapping()).unwrap();
        search_index(
            &mut loom,
            ns,
            "docs",
            b"a".to_vec(),
            doc("alpha beta", "en"),
        )
        .unwrap();
        let request = QueryRequest::new(
            Query::Match {
                field: "title".into(),
                text: "alpha".into(),
            },
            0,
            0,
        );

        let portable = search_query_auto(&loom, ns, "docs", &request, None).unwrap();
        assert!(portable.reduced);
        let native =
            search_query_auto(&loom, ns, "docs", &request, Some(&NativeLikeSearchEngine)).unwrap();
        assert!(!native.reduced);
        assert_eq!(native.hits, portable.hits);
    }

    #[test]
    fn search_wire_helpers_round_trip_canonical_shapes() {
        let mapping_bytes = cbor::encode(&CborValue::Map(vec![(
            CborValue::Text("title".to_string()),
            CborValue::Array(vec![
                CborValue::Uint(0),
                CborValue::Bool(true),
                CborValue::Bool(false),
                CborValue::Text("standard".to_string()),
                CborValue::Text("standard".to_string()),
                CborValue::Null,
            ]),
        )]));
        let mapping = search_mapping_from_cbor(&mapping_bytes).unwrap();
        assert_eq!(
            mapping["title"].analysis.index_analyzer.as_deref(),
            Some("standard")
        );

        let doc = doc("Hello Loom", "en");
        let doc_bytes = search_document_cbor(&doc);
        assert_eq!(search_document_from_cbor(&doc_bytes).unwrap(), doc);

        let request_bytes = cbor::encode(&CborValue::Array(vec![
            CborValue::Array(vec![
                CborValue::Uint(0),
                CborValue::Text("title".to_string()),
                CborValue::Text("hello".to_string()),
            ]),
            CborValue::Uint(10),
            CborValue::Uint(0),
            CborValue::Array(vec![CborValue::Text("lang".to_string())]),
            CborValue::Array(vec![CborValue::Text("title".to_string())]),
            CborValue::Array(vec![CborValue::Array(vec![
                CborValue::Uint(1),
                CborValue::Text("titles".to_string()),
                CborValue::Text("title".to_string()),
            ])]),
        ]));
        let request = search_request_from_cbor(&request_bytes).unwrap();
        assert!(matches!(request.query, Query::Match { .. }));
        assert_eq!(request.facets, vec!["lang"]);
        assert_eq!(request.highlight, vec!["title"]);
        assert_eq!(
            request.aggregations,
            vec![AggregationRequest::ValueCount {
                name: "titles".to_string(),
                field: "title".to_string(),
            }]
        );

        let response = QueryResponse {
            reduced: true,
            hits: vec![SearchHit {
                id: b"doc-1".to_vec(),
                score: 1.0,
                highlights: BTreeMap::new(),
            }],
            facets: BTreeMap::new(),
            aggregations: BTreeMap::new(),
        };
        assert_eq!(
            loom_codec::encode(&loom_codec::decode(&search_response_cbor(&response)).unwrap())
                .unwrap(),
            search_response_cbor(&response)
        );
        assert_eq!(
            loom_codec::decode(&search_ids_cbor(vec![b"a".to_vec()])).unwrap(),
            CborValue::Array(vec![CborValue::Bytes(b"a".to_vec())])
        );
    }

    #[test]
    fn authenticated_search_operations_honor_collection_scopes() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Search, None, WorkspaceId::from_bytes([31; 16]))
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
                domain: Some(FacetKind::Search.into()),
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

        search_create(&mut loom, ns, "work", mapping()).unwrap();
        search_index(&mut loom, ns, "work", b"a".to_vec(), doc("hi", "en")).unwrap();
        assert!(search_get(&loom, ns, "work", b"a").unwrap().is_some());
        assert_eq!(
            search_create(&mut loom, ns, "private", mapping())
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
    }
}
