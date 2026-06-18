use loom_core::error::Result;
use loom_substrate::view::{ViewDefinition, validate_view_id};

pub(crate) const VIEW_DIR: &str = ".loom/substrate/views";

pub(crate) fn view_path(view_id: &str) -> Result<String> {
    validate_view_id(view_id)?;
    Ok(format!("{VIEW_DIR}/{view_id}.lcv"))
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ViewDefinitionSummary {
    pub view_id: String,
    pub source_scopes: Vec<String>,
    pub source_facets: Vec<String>,
    pub projection_ref: String,
    pub output_facet: Option<String>,
    pub media_type: String,
    pub freshness_policy: String,
    pub output_digest: Option<String>,
    pub source_digests: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projection: Option<serde_json::Value>,
}

impl From<ViewDefinition> for ViewDefinitionSummary {
    fn from(view: ViewDefinition) -> Self {
        Self {
            view_id: view.view_id,
            source_scopes: view.source_scopes,
            source_facets: view.source_facets,
            projection_ref: view.projection_ref,
            output_facet: view.output_facet,
            media_type: view.media_type,
            freshness_policy: view.freshness_policy.as_str().to_string(),
            output_digest: view.output_digest.map(|digest| digest.to_string()),
            source_digests: view
                .source_digests
                .into_iter()
                .map(|digest| digest.to_string())
                .collect(),
            projection: None,
        }
    }
}
