use loom_codec::Value;
use loom_types::{LoomError, Result};

use crate::{Fields, codec_error, validate_text};

pub const REF_SCHEMA: &str = "loom.substrate.ref.v1";
pub const ALIAS_BINDING_SCHEMA: &str = "loom.substrate.alias-binding.v1";
pub const ALIAS_INDEX_SCHEMA: &str = "loom.substrate.alias-index.v1";
pub const REF_SOURCE_SCHEMA: &str = "loom.substrate.ref-source.v1";
pub const REF_EDGE_SCHEMA: &str = "loom.substrate.ref-edge.v1";
pub const REF_INDEX_SCHEMA: &str = "loom.substrate.ref-index.v1";
pub const UNRESOLVED_REFERENCE_SCHEMA: &str = "loom.substrate.unresolved-reference.v1";
pub const REFERENCE_RESOLUTION_SCHEMA: &str = "loom.substrate.reference-resolution.v1";
pub const REFERENCE_ARTIFACT_SCHEMA: &str = "loom.substrate.reference-artifact.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedReference {
    pub candidate_id: String,
    pub source: ReferenceSource,
    pub source_operation_id: String,
    pub source_root: loom_types::Digest,
    pub alias_text: String,
    pub relation: String,
    pub span_start: u64,
    pub span_end: u64,
    pub evidence: String,
    pub attempts: u32,
    pub next_attempt_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedReferenceInput {
    pub candidate_id: String,
    pub source: ReferenceSource,
    pub source_operation_id: String,
    pub source_root: loom_types::Digest,
    pub alias_text: String,
    pub relation: String,
    pub span_start: u64,
    pub span_end: u64,
    pub evidence: String,
    pub next_attempt_ms: u64,
}

impl UnresolvedReference {
    pub fn new(input: UnresolvedReferenceInput) -> Result<Self> {
        let candidate = Self {
            candidate_id: input.candidate_id,
            source: input.source,
            source_operation_id: input.source_operation_id,
            source_root: input.source_root,
            alias_text: input.alias_text,
            relation: input.relation,
            span_start: input.span_start,
            span_end: input.span_end,
            evidence: input.evidence,
            attempts: 0,
            next_attempt_ms: input.next_attempt_ms,
        };
        candidate.validate()?;
        Ok(candidate)
    }

    pub fn retry_at(&self, next_attempt_ms: u64) -> Result<Self> {
        let attempts = self
            .attempts
            .checked_add(1)
            .ok_or_else(|| LoomError::invalid("reference resolution attempts overflow"))?;
        let mut candidate = self.clone();
        candidate.attempts = attempts;
        candidate.next_attempt_ms = next_attempt_ms;
        candidate.validate()?;
        Ok(candidate)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(UNRESOLVED_REFERENCE_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.candidate_id.clone()),
                self.source.to_value(),
                Value::Text(self.source_operation_id.clone()),
                Value::Text(self.source_root.to_string()),
                Value::Text(self.alias_text.clone()),
                Value::Text(self.relation.clone()),
                Value::Uint(self.span_start),
                Value::Uint(self.span_end),
                Value::Text(self.evidence.clone()),
                Value::Uint(u64::from(self.attempts)),
                Value::Uint(self.next_attempt_ms),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "unresolved reference")?;
        outer.expect_text(UNRESOLVED_REFERENCE_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("unresolved reference fields")?,
            "unresolved reference",
        )?;
        outer.end("unresolved reference")?;
        let candidate_id = fields.text("candidate_id")?;
        let source = ReferenceSource::from_value(fields.next("source")?)?;
        let source_operation_id = fields.text("source_operation_id")?;
        let source_root = loom_types::Digest::parse(&fields.text("source_root")?)?;
        let alias_text = fields.text("alias_text")?;
        let relation = fields.text("relation")?;
        let span_start = fields.uint("span_start")?;
        let span_end = fields.uint("span_end")?;
        let evidence = fields.text("evidence")?;
        let attempts = u32::try_from(fields.uint("attempts")?)
            .map_err(|_| LoomError::corrupt("reference attempts are too large"))?;
        let next_attempt_ms = fields.uint("next_attempt_ms")?;
        fields.end("unresolved reference")?;
        let mut candidate = Self::new(UnresolvedReferenceInput {
            candidate_id,
            source,
            source_operation_id,
            source_root,
            alias_text,
            relation,
            span_start,
            span_end,
            evidence,
            next_attempt_ms,
        })?;
        candidate.attempts = attempts;
        Ok(candidate)
    }

    fn validate(&self) -> Result<()> {
        validate_text("reference candidate_id", &self.candidate_id)?;
        validate_text("reference source operation", &self.source_operation_id)?;
        validate_text("reference alias text", &self.alias_text)?;
        validate_ref_segment("reference relation", &self.relation)?;
        validate_text("reference evidence", &self.evidence)?;
        if self.span_start >= self.span_end || self.span_end as usize > self.evidence.len() {
            return Err(LoomError::invalid("reference candidate span is invalid"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceResolution {
    pub operation_id: String,
    pub candidate_id: String,
    pub source_operation_id: String,
    pub binding_root: loom_types::Digest,
    pub target: EntityRef,
    pub resolver_principal: String,
    pub resolved_at_ms: u64,
}

impl ReferenceResolution {
    pub fn new(
        operation_id: impl Into<String>,
        candidate_id: impl Into<String>,
        source_operation_id: impl Into<String>,
        binding_root: loom_types::Digest,
        target: EntityRef,
        resolver_principal: impl Into<String>,
        resolved_at_ms: u64,
    ) -> Result<Self> {
        let record = Self {
            operation_id: operation_id.into(),
            candidate_id: candidate_id.into(),
            source_operation_id: source_operation_id.into(),
            binding_root,
            target,
            resolver_principal: resolver_principal.into(),
            resolved_at_ms,
        };
        validate_text("reference resolution operation", &record.operation_id)?;
        validate_text("reference resolution candidate", &record.candidate_id)?;
        validate_text(
            "reference resolution source operation",
            &record.source_operation_id,
        )?;
        validate_text("reference resolution principal", &record.resolver_principal)?;
        Ok(record)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(REFERENCE_RESOLUTION_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.operation_id.clone()),
                Value::Text(self.candidate_id.clone()),
                Value::Text(self.source_operation_id.clone()),
                Value::Text(self.binding_root.to_string()),
                self.target.to_value(),
                Value::Text(self.resolver_principal.clone()),
                Value::Uint(self.resolved_at_ms),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "reference resolution")?;
        outer.expect_text(REFERENCE_RESOLUTION_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("reference resolution fields")?,
            "reference resolution",
        )?;
        outer.end("reference resolution")?;
        let operation_id = fields.text("operation_id")?;
        let candidate_id = fields.text("candidate_id")?;
        let source_operation_id = fields.text("source_operation_id")?;
        let binding_root = loom_types::Digest::parse(&fields.text("binding_root")?)?;
        let target = EntityRef::from_value(fields.next("target")?)?;
        let resolver_principal = fields.text("resolver_principal")?;
        let resolved_at_ms = fields.uint("resolved_at_ms")?;
        fields.end("reference resolution")?;
        Self::new(
            operation_id,
            candidate_id,
            source_operation_id,
            binding_root,
            target,
            resolver_principal,
            resolved_at_ms,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntityRef {
    pub kind: String,
    pub id: String,
}

impl EntityRef {
    pub fn parse(value: &str) -> Result<Self> {
        let Some((kind, id)) = value.split_once(':') else {
            return Err(LoomError::invalid("reference must be kind:id"));
        };
        validate_ref_segment("reference kind", kind)?;
        validate_ref_segment("reference id", id)?;
        Ok(Self {
            kind: kind.to_string(),
            id: id.to_string(),
        })
    }

    pub fn as_str(&self) -> String {
        format!("{}:{}", self.kind, self.id)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(REF_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.kind.clone()),
                Value::Text(self.id.clone()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "reference")?;
        outer.expect_text(REF_SCHEMA)?;
        let mut fields = Fields::array(outer.next("reference fields")?, "reference")?;
        outer.end("reference")?;
        let kind = fields.text("kind")?;
        let id = fields.text("id")?;
        fields.end("reference")?;
        EntityRef::parse(&format!("{kind}:{id}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceArtifactKind {
    Reference,
    Artifact,
}

impl ReferenceArtifactKind {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "reference" => Ok(Self::Reference),
            "artifact" => Ok(Self::Artifact),
            _ => Err(LoomError::invalid(
                "reference artifact kind must be reference or artifact",
            )),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reference => "reference",
            Self::Artifact => "artifact",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceArtifactRecord {
    pub record_id: String,
    pub kind: ReferenceArtifactKind,
    pub label: String,
    pub source_ref: EntityRef,
    pub source_operation_id: String,
    pub target_ref: Option<EntityRef>,
    pub created_by: String,
    pub created_at_ms: u64,
}

pub struct ReferenceArtifactInput<'a> {
    pub record_id: &'a str,
    pub kind: ReferenceArtifactKind,
    pub label: &'a str,
    pub source_ref: EntityRef,
    pub source_operation_id: &'a str,
    pub target_ref: Option<EntityRef>,
    pub created_by: &'a str,
    pub created_at_ms: u64,
}

impl ReferenceArtifactRecord {
    pub fn new(input: ReferenceArtifactInput<'_>) -> Result<Self> {
        let record = Self {
            record_id: input.record_id.to_string(),
            kind: input.kind,
            label: input.label.to_string(),
            source_ref: input.source_ref,
            source_operation_id: input.source_operation_id.to_string(),
            target_ref: input.target_ref,
            created_by: input.created_by.to_string(),
            created_at_ms: input.created_at_ms,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn entity_ref(&self) -> EntityRef {
        EntityRef {
            kind: self.kind.as_str().to_string(),
            id: self.record_id.clone(),
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(REFERENCE_ARTIFACT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.record_id.clone()),
                Value::Text(self.kind.as_str().to_string()),
                Value::Text(self.label.clone()),
                self.source_ref.to_value(),
                Value::Text(self.source_operation_id.clone()),
                optional_entity_ref(self.target_ref.as_ref()),
                Value::Text(self.created_by.clone()),
                Value::Uint(self.created_at_ms),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "reference artifact")?;
        outer.expect_text(REFERENCE_ARTIFACT_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("reference artifact fields")?,
            "reference artifact",
        )?;
        outer.end("reference artifact")?;
        let record_id = fields.text("record_id")?;
        let kind = ReferenceArtifactKind::parse(&fields.text("kind")?)?;
        let label = fields.text("label")?;
        let source_ref = EntityRef::from_value(fields.next("source_ref")?)?;
        let source_operation_id = fields.text("source_operation_id")?;
        let target_ref = optional_entity_ref_from_value(fields.next("target_ref")?)?;
        let created_by = fields.text("created_by")?;
        let created_at_ms = fields.uint("created_at_ms")?;
        fields.end("reference artifact")?;
        Self::new(ReferenceArtifactInput {
            record_id: &record_id,
            kind,
            label: &label,
            source_ref,
            source_operation_id: &source_operation_id,
            target_ref,
            created_by: &created_by,
            created_at_ms,
        })
    }

    fn validate(&self) -> Result<()> {
        validate_ref_segment("reference artifact id", &self.record_id)?;
        validate_text("reference artifact label", &self.label)?;
        validate_text(
            "reference artifact source operation",
            &self.source_operation_id,
        )?;
        validate_text("reference artifact created_by", &self.created_by)
    }
}

fn optional_entity_ref(value: Option<&EntityRef>) -> Value {
    value.map_or(Value::Null, EntityRef::to_value)
}

fn optional_entity_ref_from_value(value: Value) -> Result<Option<EntityRef>> {
    match value {
        Value::Null => Ok(None),
        other => EntityRef::from_value(other).map(Some),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefOccurrence {
    pub target: EntityRef,
    pub span_start: usize,
    pub span_end: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasBinding {
    pub alias: String,
    pub target: EntityRef,
    pub scope_id: String,
    pub sequence: u64,
}

impl AliasBinding {
    pub fn new(
        alias: impl Into<String>,
        target: EntityRef,
        scope_id: impl Into<String>,
        sequence: u64,
    ) -> Result<Self> {
        let alias = alias.into();
        let scope_id = scope_id.into();
        validate_alias(&alias)?;
        validate_text("scope_id", &scope_id)?;
        Ok(Self {
            alias,
            target,
            scope_id,
            sequence,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(ALIAS_BINDING_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.alias.clone()),
                self.target.to_value(),
                Value::Text(self.scope_id.clone()),
                Value::Uint(self.sequence),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "alias binding")?;
        outer.expect_text(ALIAS_BINDING_SCHEMA)?;
        let mut fields = Fields::array(outer.next("alias binding fields")?, "alias binding")?;
        outer.end("alias binding")?;
        let alias = fields.text("alias")?;
        let target = EntityRef::from_value(fields.next("target")?)?;
        let scope_id = fields.text("scope_id")?;
        let sequence = fields.uint("sequence")?;
        fields.end("alias binding")?;
        AliasBinding::new(alias, target, scope_id, sequence)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AliasIndex {
    bindings: Vec<AliasBinding>,
}

impl AliasIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bind(&mut self, binding: AliasBinding) {
        self.release(&binding.scope_id, &binding.alias);
        self.bindings.push(binding);
        self.bindings
            .sort_by(|left, right| alias_binding_key(left).cmp(&alias_binding_key(right)));
    }

    pub fn next_sequence(&self, scope_id: &str) -> u64 {
        self.bindings
            .iter()
            .filter(|binding| binding.scope_id == scope_id)
            .map(|binding| binding.sequence)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    pub fn release(&mut self, scope_id: &str, alias: &str) -> Option<AliasBinding> {
        let idx = self
            .bindings
            .iter()
            .position(|binding| binding.scope_id == scope_id && binding.alias == alias)?;
        Some(self.bindings.remove(idx))
    }

    pub fn resolve(&self, scope_id: &str, alias: &str) -> Option<&AliasBinding> {
        self.bindings
            .iter()
            .find(|binding| binding.scope_id == scope_id && binding.alias == alias)
    }

    pub fn bindings_for_scope(&self, scope_id: &str) -> Vec<AliasBinding> {
        self.bindings
            .iter()
            .filter(|binding| binding.scope_id == scope_id)
            .cloned()
            .collect()
    }

    pub fn bindings(&self) -> &[AliasBinding] {
        &self.bindings
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(ALIAS_INDEX_SCHEMA.to_string()),
            Value::Array(self.bindings.iter().map(AliasBinding::to_value).collect()),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "alias index")?;
        outer.expect_text(ALIAS_INDEX_SCHEMA)?;
        let bindings = match outer.next("bindings")? {
            Value::Array(items) => items
                .into_iter()
                .map(AliasBinding::from_value)
                .collect::<Result<Vec<_>>>()?,
            _ => return Err(LoomError::corrupt("alias index bindings must be an array")),
        };
        outer.end("alias index")?;
        let mut index = AliasIndex::new();
        for binding in bindings {
            index.bind(binding);
        }
        Ok(index)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceSource {
    pub facet: String,
    pub collection: String,
    pub entity_id: String,
    pub field: String,
}

impl ReferenceSource {
    pub fn new(
        facet: impl Into<String>,
        collection: impl Into<String>,
        entity_id: impl Into<String>,
        field: impl Into<String>,
    ) -> Result<Self> {
        let facet = facet.into();
        let collection = collection.into();
        let entity_id = entity_id.into();
        let field = field.into();
        validate_ref_segment("reference source facet", &facet)?;
        validate_text("reference source collection", &collection)?;
        validate_text("reference source entity_id", &entity_id)?;
        validate_text("reference source field", &field)?;
        Ok(Self {
            facet,
            collection,
            entity_id,
            field,
        })
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(REF_SOURCE_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.facet.clone()),
                Value::Text(self.collection.clone()),
                Value::Text(self.entity_id.clone()),
                Value::Text(self.field.clone()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "reference source")?;
        outer.expect_text(REF_SOURCE_SCHEMA)?;
        let mut fields = Fields::array(outer.next("reference source fields")?, "reference source")?;
        outer.end("reference source")?;
        let facet = fields.text("facet")?;
        let collection = fields.text("collection")?;
        let entity_id = fields.text("entity_id")?;
        let field = fields.text("field")?;
        fields.end("reference source")?;
        ReferenceSource::new(facet, collection, entity_id, field)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceEdge {
    pub source: ReferenceSource,
    pub target: EntityRef,
    pub relation: String,
    pub span_start: usize,
    pub span_end: usize,
    pub evidence: String,
}

impl ReferenceEdge {
    pub fn new(
        source: ReferenceSource,
        target: EntityRef,
        relation: impl Into<String>,
        span_start: usize,
        span_end: usize,
        evidence: impl Into<String>,
    ) -> Result<Self> {
        let relation = relation.into();
        let evidence = evidence.into();
        validate_ref_segment("reference relation", &relation)?;
        validate_text("reference evidence", &evidence)?;
        if span_start >= span_end {
            return Err(LoomError::invalid("reference span is empty or inverted"));
        }
        Ok(Self {
            source,
            target,
            relation,
            span_start,
            span_end,
            evidence,
        })
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(REF_EDGE_SCHEMA.to_string()),
            Value::Array(vec![
                self.source.to_value(),
                self.target.to_value(),
                Value::Text(self.relation.clone()),
                Value::Uint(self.span_start as u64),
                Value::Uint(self.span_end as u64),
                Value::Text(self.evidence.clone()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "reference edge")?;
        outer.expect_text(REF_EDGE_SCHEMA)?;
        let mut fields = Fields::array(outer.next("reference edge fields")?, "reference edge")?;
        outer.end("reference edge")?;
        let source = ReferenceSource::from_value(fields.next("source")?)?;
        let target = EntityRef::from_value(fields.next("target")?)?;
        let relation = fields.text("relation")?;
        let span_start = usize::try_from(fields.uint("span_start")?)
            .map_err(|_| LoomError::corrupt("reference span_start is too large"))?;
        let span_end = usize::try_from(fields.uint("span_end")?)
            .map_err(|_| LoomError::corrupt("reference span_end is too large"))?;
        let evidence = fields.text("evidence")?;
        fields.end("reference edge")?;
        ReferenceEdge::new(source, target, relation, span_start, span_end, evidence)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReferenceIndex {
    edges: Vec<ReferenceEdge>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownReferenceKind {
    Typed,
    PrincipalHandle,
    ChannelHandle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownReferenceCandidate {
    pub kind: MarkdownReferenceKind,
    pub span_start: usize,
    pub span_end: usize,
    pub text: String,
}

impl ReferenceIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, edge: ReferenceEdge) {
        self.edges.push(edge);
        self.edges
            .sort_by(|left, right| reference_edge_key(left).cmp(&reference_edge_key(right)));
        self.edges.dedup();
    }

    pub fn add_text_refs(
        &mut self,
        source: ReferenceSource,
        relation: &str,
        text: &str,
    ) -> Result<()> {
        for occurrence in extract_ref_occurrences(text)? {
            self.add(ReferenceEdge::new(
                source.clone(),
                occurrence.target,
                relation,
                occurrence.span_start,
                occurrence.span_end,
                occurrence.text,
            )?);
        }
        Ok(())
    }

    pub fn remove_source(&mut self, source: &ReferenceSource) -> usize {
        let before = self.edges.len();
        self.edges.retain(|edge| &edge.source != source);
        before - self.edges.len()
    }

    pub fn remove_sources_matching<F>(&mut self, mut predicate: F) -> usize
    where
        F: FnMut(&ReferenceSource) -> bool,
    {
        let before = self.edges.len();
        self.edges.retain(|edge| !predicate(&edge.source));
        before - self.edges.len()
    }

    pub fn replace_text_refs(
        &mut self,
        source: ReferenceSource,
        relation: &str,
        text: &str,
    ) -> Result<usize> {
        self.remove_source(&source);
        let before = self.edges.len();
        self.add_text_refs(source, relation, text)?;
        Ok(self.edges.len() - before)
    }

    pub fn inbound(&self, target: &EntityRef) -> Vec<ReferenceEdge> {
        self.edges
            .iter()
            .filter(|edge| &edge.target == target)
            .cloned()
            .collect()
    }

    pub fn outbound(&self, source: &ReferenceSource) -> Vec<ReferenceEdge> {
        self.edges
            .iter()
            .filter(|edge| &edge.source == source)
            .cloned()
            .collect()
    }

    pub fn edges(&self) -> &[ReferenceEdge] {
        &self.edges
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(REF_INDEX_SCHEMA.to_string()),
            Value::Array(self.edges.iter().map(ReferenceEdge::to_value).collect()),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "reference index")?;
        outer.expect_text(REF_INDEX_SCHEMA)?;
        let edges = match outer.next("edges")? {
            Value::Array(items) => items
                .into_iter()
                .map(ReferenceEdge::from_value)
                .collect::<Result<Vec<_>>>()?,
            _ => return Err(LoomError::corrupt("reference index edges must be an array")),
        };
        outer.end("reference index")?;
        let mut index = ReferenceIndex::new();
        for edge in edges {
            index.add(edge);
        }
        Ok(index)
    }
}

pub fn extract_refs(text: &str) -> Result<Vec<EntityRef>> {
    let mut refs = extract_ref_occurrences(text)?
        .into_iter()
        .map(|occurrence| occurrence.target)
        .collect::<Vec<_>>();
    refs.sort();
    refs.dedup();
    Ok(refs)
}

pub fn extract_ref_occurrences(text: &str) -> Result<Vec<RefOccurrence>> {
    let mut occurrences = Vec::new();
    let mut token_start = None;
    for (idx, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if let Some(start) = token_start.take() {
                push_token_ref(text, start, idx, &mut occurrences)?;
            }
        } else if token_start.is_none() {
            token_start = Some(idx);
        }
    }
    if let Some(start) = token_start {
        push_token_ref(text, start, text.len(), &mut occurrences)?;
    }
    Ok(occurrences)
}

pub fn extract_markdown_reference_candidates(text: &str) -> Vec<MarkdownReferenceCandidate> {
    let excluded = markdown_code_bytes(text);
    let mut candidates = Vec::new();
    let mut token_start = None;
    for (index, character) in text.char_indices() {
        if character.is_whitespace() || excluded[index] {
            if let Some(start) = token_start.take() {
                push_markdown_candidate(text, start, index, &excluded, &mut candidates);
            }
        } else if token_start.is_none() {
            token_start = Some(index);
        }
    }
    if let Some(start) = token_start {
        push_markdown_candidate(text, start, text.len(), &excluded, &mut candidates);
    }
    candidates
}

fn push_markdown_candidate(
    text: &str,
    token_start: usize,
    token_end: usize,
    excluded: &[bool],
    candidates: &mut Vec<MarkdownReferenceCandidate>,
) {
    if excluded[token_start..token_end]
        .iter()
        .any(|excluded| *excluded)
    {
        return;
    }
    let (start, end) = trim_token_bounds(text, token_start, token_end);
    if start == end {
        return;
    }
    let token = &text[start..end];
    let (kind, candidate) = if let Some(handle) = token.strip_prefix('@') {
        (MarkdownReferenceKind::PrincipalHandle, handle)
    } else if let Some(handle) = token.strip_prefix('#') {
        (MarkdownReferenceKind::ChannelHandle, handle)
    } else if let Some(target) = token.strip_prefix('!')
        && EntityRef::parse(target).is_ok()
    {
        (MarkdownReferenceKind::Typed, target)
    } else {
        return;
    };
    if candidate.is_empty() || !candidate_bytes_valid(kind, candidate) {
        return;
    }
    candidates.push(MarkdownReferenceCandidate {
        kind,
        span_start: start,
        span_end: end,
        text: token.to_string(),
    });
}

fn candidate_bytes_valid(kind: MarkdownReferenceKind, value: &str) -> bool {
    match kind {
        MarkdownReferenceKind::Typed => EntityRef::parse(value).is_ok(),
        MarkdownReferenceKind::PrincipalHandle | MarkdownReferenceKind::ChannelHandle => {
            let bytes = value.as_bytes();
            !bytes.is_empty()
                && bytes.len() <= 64
                && bytes[0].is_ascii_alphanumeric()
                && bytes[bytes.len() - 1].is_ascii_alphanumeric()
                && bytes
                    .iter()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'-'))
        }
    }
}

fn markdown_code_bytes(text: &str) -> Vec<bool> {
    let bytes = text.as_bytes();
    let mut excluded = vec![false; bytes.len()];
    let mut index = 0;
    let mut line_start = true;
    let mut fence = None;
    while index < bytes.len() {
        if line_start {
            let mut marker = index;
            while marker < bytes.len() && marker - index < 4 && bytes[marker] == b' ' {
                marker += 1;
            }
            let run = backtick_run(bytes, marker);
            if let Some(width) = fence {
                if run >= width {
                    mark_bytes(&mut excluded, index, marker + run);
                    index = marker + run;
                    fence = None;
                    line_start = false;
                    continue;
                }
            } else if run >= 3 {
                fence = Some(run);
                mark_bytes(&mut excluded, index, marker + run);
                index = marker + run;
                line_start = false;
                continue;
            }
        }
        if fence.is_some() {
            excluded[index] = true;
            line_start = bytes[index] == b'\n';
            index += 1;
            continue;
        }
        if bytes[index] == b'`' {
            let width = backtick_run(bytes, index);
            if width < 3
                && let Some(close) = find_inline_code_close(bytes, index + width, width)
            {
                mark_bytes(&mut excluded, index, close + width);
                index = close + width;
                line_start = false;
                continue;
            }
        }
        line_start = bytes[index] == b'\n';
        index += 1;
    }
    excluded
}

fn backtick_run(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .take_while(|byte| **byte == b'`')
        .count()
}

fn find_inline_code_close(bytes: &[u8], mut index: usize, width: usize) -> Option<usize> {
    while index < bytes.len() {
        if bytes[index] == b'\n' {
            return None;
        }
        if backtick_run(bytes, index) == width {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn mark_bytes(bytes: &mut [bool], start: usize, end: usize) {
    for byte in &mut bytes[start..end] {
        *byte = true;
    }
}

fn push_token_ref(
    text: &str,
    token_start: usize,
    token_end: usize,
    occurrences: &mut Vec<RefOccurrence>,
) -> Result<()> {
    let (start, end) = trim_token_bounds(text, token_start, token_end);
    if start == end {
        return Ok(());
    }
    let token = &text[start..end];
    if let Some(target_text) = token.strip_prefix('!')
        && let Ok(target) = EntityRef::parse(target_text)
    {
        occurrences.push(RefOccurrence {
            target,
            span_start: start,
            span_end: end,
            text: token.to_string(),
        });
    }
    Ok(())
}

fn trim_token_bounds(text: &str, mut start: usize, mut end: usize) -> (usize, usize) {
    while start < end {
        let ch = text[start..end].chars().next().unwrap();
        if !is_ref_outer_punctuation(ch) {
            break;
        }
        start += ch.len_utf8();
    }
    while start < end {
        let ch = text[start..end].chars().next_back().unwrap();
        if !is_ref_outer_punctuation(ch) {
            break;
        }
        end -= ch.len_utf8();
    }
    (start, end)
}

fn is_ref_outer_punctuation(ch: char) -> bool {
    matches!(ch, ',' | '.' | ';' | ')' | '(' | '[' | ']' | '{' | '}')
}

fn reference_edge_key(
    edge: &ReferenceEdge,
) -> (&str, &str, &str, &str, &str, &str, &str, usize, usize) {
    (
        edge.target.kind.as_str(),
        edge.target.id.as_str(),
        edge.source.facet.as_str(),
        edge.source.collection.as_str(),
        edge.source.entity_id.as_str(),
        edge.source.field.as_str(),
        edge.relation.as_str(),
        edge.span_start,
        edge.span_end,
    )
}

fn alias_binding_key(binding: &AliasBinding) -> (&str, &str, u64, &str, &str) {
    (
        binding.scope_id.as_str(),
        binding.alias.as_str(),
        binding.sequence,
        binding.target.kind.as_str(),
        binding.target.id.as_str(),
    )
}

fn validate_ref_segment(name: &str, value: &str) -> Result<()> {
    validate_text(name, value)?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(LoomError::invalid(format!(
            "{name} must use ascii alnum, dot, underscore, or hyphen"
        )));
    }
    Ok(())
}

fn validate_alias(value: &str) -> Result<()> {
    validate_text("alias", value)?;
    if value.contains(':') {
        return Err(LoomError::invalid("alias must not contain ':'"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_ref_parses_and_encodes() {
        let entity_ref = EntityRef::parse("ticket:LOOM-1").unwrap();
        assert_eq!(entity_ref.kind, "ticket");
        assert_eq!(entity_ref.id, "LOOM-1");
        assert_eq!(entity_ref.as_str(), "ticket:LOOM-1");
        assert!(!entity_ref.encode().unwrap().is_empty());
        assert_eq!(
            EntityRef::decode(&entity_ref.encode().unwrap()).unwrap(),
            entity_ref
        );
        assert!(EntityRef::parse("ticket/LOOM-1").is_err());
        assert!(EntityRef::parse("ticket:LOOM 1").is_err());
    }

    #[test]
    fn aliases_bind_display_names_to_stable_refs() {
        let binding = AliasBinding::new(
            "LOOM-1",
            EntityRef::parse("ticket:01HX").unwrap(),
            "PROJ",
            7,
        )
        .unwrap();
        assert!(!binding.encode().unwrap().is_empty());
        assert_eq!(
            AliasBinding::decode(&binding.encode().unwrap()).unwrap(),
            binding
        );
        assert!(AliasBinding::new("bad:alias", binding.target.clone(), "PROJ", 8).is_err());
    }

    #[test]
    fn unresolved_reference_and_resolution_round_trip() {
        let source = ReferenceSource::new("tickets", "studio", "ticket-1", "description").unwrap();
        let root = loom_types::Digest::hash(loom_types::Algo::Blake3, b"source-root");
        let candidate = UnresolvedReference::new(UnresolvedReferenceInput {
            candidate_id: "candidate-1".to_string(),
            source,
            source_operation_id: "studio:2".to_string(),
            source_root: root,
            alias_text: "CORE-52".to_string(),
            relation: "refers_to".to_string(),
            span_start: 8,
            span_end: 15,
            evidence: "Relates CORE-52".to_string(),
            next_attempt_ms: 100,
        })
        .unwrap();
        assert_eq!(
            UnresolvedReference::decode(&candidate.encode().unwrap()).unwrap(),
            candidate
        );
        let retry = candidate.retry_at(200).unwrap();
        assert_eq!(retry.attempts, 1);
        assert_eq!(retry.next_attempt_ms, 200);
        let resolution = ReferenceResolution::new(
            "reference.resolved:candidate-1",
            "candidate-1",
            "studio:2",
            root,
            EntityRef::parse("ticket:ticket-52").unwrap(),
            "resolver-service",
            300,
        )
        .unwrap();
        assert_eq!(
            ReferenceResolution::decode(&resolution.encode().unwrap()).unwrap(),
            resolution
        );
    }

    #[test]
    fn alias_index_rebinds_releases_and_round_trips() {
        let target = EntityRef::parse("ticket:01HX").unwrap();
        let renamed = EntityRef::parse("ticket:01HY").unwrap();
        let mut index = AliasIndex::new();
        index.bind(AliasBinding::new("LOOM-1", target.clone(), "studio", 1).unwrap());
        assert_eq!(index.resolve("studio", "LOOM-1").unwrap().target, target);
        assert_eq!(index.next_sequence("studio"), 2);

        index.bind(AliasBinding::new("LOOM-1", renamed.clone(), "studio", 2).unwrap());

        assert_eq!(index.bindings_for_scope("studio").len(), 1);
        assert_eq!(index.resolve("studio", "LOOM-1").unwrap().target, renamed);
        assert_eq!(AliasIndex::decode(&index.encode().unwrap()).unwrap(), index);
        assert!(index.release("studio", "LOOM-1").is_some());
        assert!(index.resolve("studio", "LOOM-1").is_none());
    }

    #[test]
    fn extraction_is_sorted_and_unique() {
        let refs = extract_refs("See !ticket:LOOM-1, !page:Spec and !ticket:LOOM-1.").unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].as_str(), "page:Spec");
        assert_eq!(refs[1].as_str(), "ticket:LOOM-1");
    }

    #[test]
    fn occurrences_preserve_byte_spans() {
        let text = "See (!ticket:LOOM-1), then !page:Spec.";
        let refs = extract_ref_occurrences(text).unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].text, "!ticket:LOOM-1");
        assert_eq!(
            &text[refs[0].span_start..refs[0].span_end],
            "!ticket:LOOM-1"
        );
        assert_eq!(refs[1].text, "!page:Spec");
    }

    #[test]
    fn markdown_candidates_exclude_code_and_preserve_reference_spans() {
        let text = "See CORE-52, !ticket:CORE-53, @Alex, and #team-chat. `!ticket:CORE-54 @skip`\n```md\n# hidden !ticket:CORE-55\n```\n";
        let candidates = extract_markdown_reference_candidates(text);
        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0].kind, MarkdownReferenceKind::Typed);
        assert_eq!(candidates[0].text, "!ticket:CORE-53");
        assert_eq!(candidates[1].kind, MarkdownReferenceKind::PrincipalHandle);
        assert_eq!(candidates[1].text, "@Alex");
        assert_eq!(candidates[2].kind, MarkdownReferenceKind::ChannelHandle);
        assert_eq!(candidates[2].text, "#team-chat");
        assert_eq!(
            &text[candidates[2].span_start..candidates[2].span_end],
            "#team-chat"
        );
    }

    #[test]
    fn extraction_ignores_malformed_colon_tokens() {
        let refs = extract_refs(
            r#"{"id":"ask-1","uri":"ui://repo/mcp/apps/internal/decisions"} !ticket:1"#,
        )
        .unwrap();
        assert_eq!(refs, vec![EntityRef::parse("ticket:1").unwrap()]);
    }

    #[test]
    fn reference_index_returns_inbound_edges_in_stable_order() {
        let source = ReferenceSource::new("document", "pages", "intro", "body").unwrap();
        let mut index = ReferenceIndex::new();
        index
            .add_text_refs(
                source,
                "refers_to",
                "See !ticket:LOOM-2 and !ticket:LOOM-1 and !ticket:LOOM-1.",
            )
            .unwrap();
        let inbound = index.inbound(&EntityRef::parse("ticket:LOOM-1").unwrap());
        assert_eq!(inbound.len(), 2);
        assert_eq!(inbound[0].source.entity_id, "intro");
        assert_eq!(inbound[0].relation, "refers_to");
        assert_eq!(index.edges().len(), 3);
    }

    #[test]
    fn reference_index_replaces_edges_for_one_source() {
        let source = ReferenceSource::new("document", "pages", "intro", "body").unwrap();
        let other = ReferenceSource::new("document", "pages", "guide", "body").unwrap();
        let mut index = ReferenceIndex::new();
        index
            .add_text_refs(
                source.clone(),
                "refers_to",
                "See !ticket:OLD and !page:Keep.",
            )
            .unwrap();
        index
            .add_text_refs(other, "refers_to", "See !ticket:OTHER.")
            .unwrap();

        let added = index
            .replace_text_refs(source, "refers_to", "Now see !ticket:NEW.")
            .unwrap();

        assert_eq!(added, 1);
        assert!(
            index
                .inbound(&EntityRef::parse("ticket:OLD").unwrap())
                .is_empty()
        );
        assert_eq!(
            index
                .inbound(&EntityRef::parse("ticket:NEW").unwrap())
                .len(),
            1
        );
        assert_eq!(
            index
                .inbound(&EntityRef::parse("ticket:OTHER").unwrap())
                .len(),
            1
        );
    }

    #[test]
    fn reference_index_removes_sources_by_predicate() {
        let mut index = ReferenceIndex::new();
        index
            .add_text_refs(
                ReferenceSource::new("tickets", "studio", "ticket-1", "summary").unwrap(),
                "refers_to",
                "See !page:One.",
            )
            .unwrap();
        index
            .add_text_refs(
                ReferenceSource::new("tickets", "studio", "ticket-2", "summary").unwrap(),
                "refers_to",
                "See !page:Two.",
            )
            .unwrap();

        let removed = index.remove_sources_matching(|source| {
            source.facet == "tickets" && source.entity_id == "ticket-1"
        });

        assert_eq!(removed, 1);
        assert!(
            index
                .inbound(&EntityRef::parse("page:One").unwrap())
                .is_empty()
        );
        assert_eq!(
            index.inbound(&EntityRef::parse("page:Two").unwrap()).len(),
            1
        );
    }

    #[test]
    fn reference_index_encodes_edges_in_stable_order() {
        let mut index = ReferenceIndex::new();
        index
            .add_text_refs(
                ReferenceSource::new("document", "pages", "intro", "body").unwrap(),
                "refers_to",
                "See !ticket:B and !ticket:A.",
            )
            .unwrap();
        index
            .add_text_refs(
                ReferenceSource::new("graph", "links", "edge-1", "target").unwrap(),
                "mentions",
                "See !ticket:A.",
            )
            .unwrap();

        let decoded = ReferenceIndex::decode(&index.encode().unwrap()).unwrap();

        assert_eq!(decoded, index);
        let inbound = decoded.inbound(&EntityRef::parse("ticket:A").unwrap());
        assert_eq!(inbound.len(), 2);
        assert_eq!(inbound[0].source.facet, "document");
        assert_eq!(inbound[1].source.facet, "graph");
    }
}
