use std::collections::BTreeMap;

use loom_codec::Value;
use loom_types::{Digest, LoomError, Result};

use crate::{
    Fields, codec_error, optional_digest, optional_text_value, string_array, validate_text,
};

pub const VIEW_DEFINITION_SCHEMA: &str = "loom.substrate.view-definition.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessPolicy {
    OnWrite,
    OnRead,
    Scheduled,
}

impl FreshnessPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnWrite => "on_write",
            Self::OnRead => "on_read",
            Self::Scheduled => "scheduled",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "on_write" => Ok(Self::OnWrite),
            "on_read" => Ok(Self::OnRead),
            "scheduled" => Ok(Self::Scheduled),
            _ => Err(LoomError::invalid(format!(
                "freshness_policy must be on_write, on_read, or scheduled, got {value:?}"
            ))),
        }
    }

    const fn tag(self) -> u64 {
        match self {
            Self::OnWrite => 0,
            Self::OnRead => 1,
            Self::Scheduled => 2,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::OnWrite),
            1 => Ok(Self::OnRead),
            2 => Ok(Self::Scheduled),
            _ => Err(LoomError::corrupt("unknown view freshness policy")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewDefinition {
    pub view_id: String,
    pub source_scopes: Vec<String>,
    pub source_facets: Vec<String>,
    pub projection_ref: String,
    pub output_facet: Option<String>,
    pub media_type: String,
    pub freshness_policy: FreshnessPolicy,
    pub output_digest: Option<Digest>,
    pub source_digests: Vec<Digest>,
}

#[derive(Debug, Clone)]
pub struct ViewDefinitionInput<'a> {
    pub view_id: &'a str,
    pub source_scopes: &'a [&'a str],
    pub source_facets: &'a [&'a str],
    pub projection_ref: &'a str,
    pub output_facet: Option<&'a str>,
    pub media_type: &'a str,
    pub freshness_policy: FreshnessPolicy,
    pub output_digest: Option<Digest>,
    pub source_digests: &'a [Digest],
}

impl ViewDefinition {
    pub fn new(input: ViewDefinitionInput<'_>) -> Result<Self> {
        validate_view_id(input.view_id)?;
        validate_string_list("source scope", input.source_scopes)?;
        validate_string_list("source facet", input.source_facets)?;
        validate_text("projection_ref", input.projection_ref)?;
        if let Some(output_facet) = input.output_facet {
            validate_text("output_facet", output_facet)?;
        }
        validate_media_type(input.media_type)?;
        let mut source_scopes = input
            .source_scopes
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>();
        source_scopes.sort();
        source_scopes.dedup();
        let mut source_facets = input
            .source_facets
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>();
        source_facets.sort();
        source_facets.dedup();
        let mut source_digests = input.source_digests.to_vec();
        source_digests.sort();
        source_digests.dedup();
        Ok(Self {
            view_id: input.view_id.to_string(),
            source_scopes,
            source_facets,
            projection_ref: input.projection_ref.to_string(),
            output_facet: input.output_facet.map(str::to_string),
            media_type: input.media_type.to_string(),
            freshness_policy: input.freshness_policy,
            output_digest: input.output_digest,
            source_digests,
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
            Value::Text(VIEW_DEFINITION_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.view_id.clone()),
                string_array(&self.source_scopes),
                string_array(&self.source_facets),
                Value::Text(self.projection_ref.clone()),
                optional_text_value(self.output_facet.as_deref()),
                Value::Text(self.media_type.clone()),
                Value::Uint(self.freshness_policy.tag()),
                optional_digest(self.output_digest),
                digest_array(&self.source_digests),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "view definition")?;
        outer.expect_text(VIEW_DEFINITION_SCHEMA)?;
        let mut fields = Fields::array(outer.next("view definition fields")?, "view definition")?;
        outer.end("view definition")?;
        let view_id = fields.text("view_id")?;
        let source_scopes = fields.string_array("source_scopes")?;
        let source_facets = fields.string_array("source_facets")?;
        let projection_ref = fields.text("projection_ref")?;
        let output_facet = fields.optional_text("output_facet")?;
        let media_type = fields.text("media_type")?;
        let freshness_policy = FreshnessPolicy::from_tag(fields.uint("freshness_policy")?)?;
        let output_digest = fields.optional_digest("output_digest")?;
        let source_digests = digest_list(fields.next("source_digests")?)?;
        fields.end("view definition")?;
        let source_scope_refs = source_scopes.iter().map(String::as_str).collect::<Vec<_>>();
        let source_facet_refs = source_facets.iter().map(String::as_str).collect::<Vec<_>>();
        Self::new(ViewDefinitionInput {
            view_id: &view_id,
            source_scopes: &source_scope_refs,
            source_facets: &source_facet_refs,
            projection_ref: &projection_ref,
            output_facet: output_facet.as_deref(),
            media_type: &media_type,
            freshness_policy,
            output_digest,
            source_digests: &source_digests,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewRegistry {
    views: BTreeMap<String, ViewDefinition>,
}

impl ViewRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn define(&mut self, view: ViewDefinition) {
        self.views.insert(view.view_id.clone(), view);
    }

    pub fn get(&self, view_id: &str) -> Option<&ViewDefinition> {
        self.views.get(view_id)
    }

    pub fn list(&self) -> impl Iterator<Item = &ViewDefinition> {
        self.views.values()
    }
}

pub fn validate_view_id(value: &str) -> Result<()> {
    validate_text("view_id", value)?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(LoomError::invalid(
            "view_id must use ascii alnum, dot, underscore, or hyphen",
        ));
    }
    Ok(())
}

fn validate_string_list(name: &str, values: &[&str]) -> Result<()> {
    if values.is_empty() {
        return Err(LoomError::invalid(format!("{name} list must not be empty")));
    }
    for value in values {
        validate_text(name, value)?;
    }
    Ok(())
}

fn validate_media_type(value: &str) -> Result<()> {
    validate_text("media_type", value)?;
    if !value.contains('/') {
        return Err(LoomError::invalid("media_type must contain '/'"));
    }
    Ok(())
}

fn digest_array(digests: &[Digest]) -> Value {
    Value::Array(
        digests
            .iter()
            .map(|digest| Value::Text(digest.to_string()))
            .collect(),
    )
}

fn digest_list(value: Value) -> Result<Vec<Digest>> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                Value::Text(value) => Digest::parse(&value),
                _ => Err(LoomError::corrupt("source_digests item must be text")),
            })
            .collect(),
        _ => Err(LoomError::corrupt("source_digests must be an array")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_types::Algo;

    fn digest(value: &[u8]) -> Digest {
        Digest::hash(Algo::Blake3, value)
    }

    #[test]
    fn view_definition_round_trips_canonical_bytes() {
        let view = ViewDefinition::new(ViewDefinitionInput {
            view_id: "bootstrap",
            source_scopes: &["organization", "organization"],
            source_facets: &["document", "graph"],
            projection_ref: "program:bootstrap-v1",
            output_facet: Some("document"),
            media_type: "text/markdown",
            freshness_policy: FreshnessPolicy::OnRead,
            output_digest: Some(digest(b"out")),
            source_digests: &[digest(b"b"), digest(b"a")],
        })
        .unwrap();
        let decoded = ViewDefinition::decode(&view.encode().unwrap()).unwrap();
        assert_eq!(decoded, view);
        assert_eq!(decoded.source_scopes, vec!["organization"]);
        let mut expected_digests = vec![digest(b"a"), digest(b"b")];
        expected_digests.sort();
        assert_eq!(decoded.source_digests, expected_digests);
    }

    #[test]
    fn registry_replaces_by_view_id_and_lists_in_order() {
        let mut registry = ViewRegistry::new();
        for id in ["z", "a"] {
            registry.define(
                ViewDefinition::new(ViewDefinitionInput {
                    view_id: id,
                    source_scopes: &["organization"],
                    source_facets: &["document"],
                    projection_ref: "program:p",
                    output_facet: None,
                    media_type: "application/json",
                    freshness_policy: FreshnessPolicy::OnWrite,
                    output_digest: None,
                    source_digests: &[],
                })
                .unwrap(),
            );
        }
        assert_eq!(
            registry
                .list()
                .map(|view| view.view_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "z"]
        );
    }
}
