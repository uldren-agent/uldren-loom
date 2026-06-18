use std::collections::BTreeSet;

use loom_codec::Value;
use loom_types::{Code, Digest, LoomError, Result};

use crate::promotion::{StudioPromotionTarget, validate_studio_promotion};
use crate::{Fields, codec_error, optional_text_value, validate_text};

pub const APP_ID: &str = "meetings";
pub const SOURCE_SCHEMA: &str = "loom.studio.meetings.source.v1";
pub const MEETING_SCHEMA: &str = "loom.studio.meetings.meeting.v1";
pub const SPAN_SCHEMA: &str = "loom.studio.meetings.span.v1";
pub const ANNOTATION_SCHEMA: &str = "loom.studio.meetings.annotation.v1";
pub const VOCABULARY_TERM_SCHEMA: &str = "loom.studio.meetings.vocabulary-term.v1";
pub const ENTITY_MERGE_SCHEMA: &str = "loom.studio.meetings.entity-merge.v1";
pub const PROMOTION_SCHEMA: &str = "loom.studio.meetings.promotion.v1";
pub const EXTRACTION_REVIEW_SCHEMA: &str = "loom.studio.meetings.extraction-review.v1";
pub const IMPORT_RUN_SCHEMA: &str = "loom.studio.meetings.import-run.v1";
pub const REDACTION_SCHEMA: &str = "loom.studio.meetings.redaction.v1";
pub const PROJECTION_EFFECT_SCHEMA: &str = "loom.studio.meetings.projection-effect.v1";
pub const PROJECTION_EFFECT_SET_SCHEMA: &str = "loom.studio.meetings.projection-effect-set.v1";
pub const PROJECTION_OUTPUT_SCHEMA: &str = "loom.studio.meetings.projection-output.v1";
pub const PROJECTION_OUTPUT_SET_SCHEMA: &str = "loom.studio.meetings.projection-output-set.v1";
pub const PROJECTION_OUTPUT_PAYLOAD_SCHEMA: &str = "loom.studio.meetings.projection-payload.v1";
pub const PROFILE_SNAPSHOT_SCHEMA: &str = "loom.studio.meetings.profile-snapshot.v1";
pub const PROFILE_CONTROL_PREFIX: &str = "profile/meetings/v1";

pub fn meetings_profile_key(workspace_id: &str) -> Result<Vec<u8>> {
    validate_text("workspace_id", workspace_id)?;
    Ok(format!("{PROFILE_CONTROL_PREFIX}/{workspace_id}/snapshot").into_bytes())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputProfile {
    Generic,
    GranolaApi,
    GranolaApp,
    GranolaMcp,
    Csv,
}

impl InputProfile {
    const fn tag(self) -> u64 {
        match self {
            Self::Generic => 0,
            Self::GranolaApi => 1,
            Self::GranolaApp => 2,
            Self::GranolaMcp => 3,
            Self::Csv => 4,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Generic),
            1 => Ok(Self::GranolaApi),
            2 => Ok(Self::GranolaApp),
            3 => Ok(Self::GranolaMcp),
            4 => Ok(Self::Csv),
            other => Err(LoomError::corrupt(format!(
                "unknown meetings input profile tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    Complete,
    Partial,
    Degraded,
}

impl Coverage {
    const fn tag(self) -> u64 {
        match self {
            Self::Complete => 0,
            Self::Partial => 1,
            Self::Degraded => 2,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Complete),
            1 => Ok(Self::Partial),
            2 => Ok(Self::Degraded),
            other => Err(LoomError::corrupt(format!(
                "unknown meetings coverage tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    TranscriptEntry,
    TranscriptChunk,
    SummaryRange,
    NoteBodyRange,
    MetadataField,
}

impl SpanKind {
    const fn tag(self) -> u64 {
        match self {
            Self::TranscriptEntry => 0,
            Self::TranscriptChunk => 1,
            Self::SummaryRange => 2,
            Self::NoteBodyRange => 3,
            Self::MetadataField => 4,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::TranscriptEntry),
            1 => Ok(Self::TranscriptChunk),
            2 => Ok(Self::SummaryRange),
            3 => Ok(Self::NoteBodyRange),
            4 => Ok(Self::MetadataField),
            other => Err(LoomError::corrupt(format!(
                "unknown meetings span kind tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationStatus {
    Observed,
    Suggested,
    Accepted,
    Rejected,
    Superseded,
    Merged,
}

impl AnnotationStatus {
    const fn tag(self) -> u64 {
        match self {
            Self::Observed => 0,
            Self::Suggested => 1,
            Self::Accepted => 2,
            Self::Rejected => 3,
            Self::Superseded => 4,
            Self::Merged => 5,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Observed),
            1 => Ok(Self::Suggested),
            2 => Ok(Self::Accepted),
            3 => Ok(Self::Rejected),
            4 => Ok(Self::Superseded),
            5 => Ok(Self::Merged),
            other => Err(LoomError::corrupt(format!(
                "unknown meetings annotation status tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VocabularyTermStatus {
    Proposed,
    Accepted,
    Rejected,
}

impl VocabularyTermStatus {
    const fn tag(self) -> u64 {
        match self {
            Self::Proposed => 0,
            Self::Accepted => 1,
            Self::Rejected => 2,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Proposed),
            1 => Ok(Self::Accepted),
            2 => Ok(Self::Rejected),
            other => Err(LoomError::corrupt(format!(
                "unknown meetings vocabulary status tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactionState {
    Live,
    Redacted,
    RetainedMetadataOnly,
}

impl RedactionState {
    const fn tag(self) -> u64 {
        match self {
            Self::Live => 0,
            Self::Redacted => 1,
            Self::RetainedMetadataOnly => 2,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Live),
            1 => Ok(Self::Redacted),
            2 => Ok(Self::RetainedMetadataOnly),
            other => Err(LoomError::corrupt(format!(
                "unknown meetings redaction state tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingStatus {
    Active,
    DeletedAtSource,
    Redacted,
    RetainedMetadataOnly,
}

impl MeetingStatus {
    const fn tag(self) -> u64 {
        match self {
            Self::Active => 0,
            Self::DeletedAtSource => 1,
            Self::Redacted => 2,
            Self::RetainedMetadataOnly => 3,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Active),
            1 => Ok(Self::DeletedAtSource),
            2 => Ok(Self::Redacted),
            3 => Ok(Self::RetainedMetadataOnly),
            other => Err(LoomError::corrupt(format!(
                "unknown meetings status tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProjectionKind {
    Document,
    Files,
    Graph,
    Vector,
    Search,
    SqlDataframe,
    Ledger,
}

impl ProjectionKind {
    const fn tag(self) -> u64 {
        match self {
            Self::Document => 0,
            Self::Files => 1,
            Self::Graph => 2,
            Self::Vector => 3,
            Self::Search => 4,
            Self::SqlDataframe => 5,
            Self::Ledger => 6,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Document),
            1 => Ok(Self::Files),
            2 => Ok(Self::Graph),
            3 => Ok(Self::Vector),
            4 => Ok(Self::Search),
            5 => Ok(Self::SqlDataframe),
            6 => Ok(Self::Ledger),
            other => Err(LoomError::corrupt(format!(
                "unknown meetings projection tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionAction {
    Upsert,
    Append,
    Invalidate,
    RetainMetadata,
}

impl ProjectionAction {
    const fn tag(self) -> u64 {
        match self {
            Self::Upsert => 0,
            Self::Append => 1,
            Self::Invalidate => 2,
            Self::RetainMetadata => 3,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Upsert),
            1 => Ok(Self::Append),
            2 => Ok(Self::Invalidate),
            3 => Ok(Self::RetainMetadata),
            other => Err(LoomError::corrupt(format!(
                "unknown meetings projection action tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRecord {
    pub source_id: String,
    pub source_system: String,
    pub external_id: String,
    pub source_digest: Digest,
    pub observed_at_ms: u64,
    pub source_created_at_ms: Option<u64>,
    pub source_updated_at_ms: Option<u64>,
    pub owner_principal: Option<String>,
    pub access_scope: String,
    pub coverage: Coverage,
    pub sidecar_digest: Option<Digest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRecordInput<'a> {
    pub source_id: &'a str,
    pub source_system: &'a str,
    pub external_id: &'a str,
    pub source_digest: Digest,
    pub observed_at_ms: u64,
    pub access_scope: &'a str,
    pub coverage: Coverage,
}

impl SourceRecord {
    pub fn new(input: SourceRecordInput<'_>) -> Result<Self> {
        let record = Self {
            source_id: input.source_id.to_string(),
            source_system: input.source_system.to_string(),
            external_id: input.external_id.to_string(),
            source_digest: input.source_digest,
            observed_at_ms: input.observed_at_ms,
            source_created_at_ms: None,
            source_updated_at_ms: None,
            owner_principal: None,
            access_scope: input.access_scope.to_string(),
            coverage: input.coverage,
            sidecar_digest: None,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("source_id", &self.source_id)?;
        validate_text("source_system", &self.source_system)?;
        validate_text("external_id", &self.external_id)?;
        validate_text("access_scope", &self.access_scope)?;
        if let Some(owner) = &self.owner_principal {
            validate_text("owner_principal", owner)?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(SOURCE_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.source_id.clone()),
                Value::Text(self.source_system.clone()),
                Value::Text(self.external_id.clone()),
                Value::Text(self.source_digest.to_string()),
                Value::Uint(self.observed_at_ms),
                optional_u64_value(self.source_created_at_ms),
                optional_u64_value(self.source_updated_at_ms),
                optional_text_value(self.owner_principal.as_deref()),
                Value::Text(self.access_scope.clone()),
                Value::Uint(self.coverage.tag()),
                optional_digest_value(self.sidecar_digest),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "meeting source")?;
        outer.expect_text(SOURCE_SCHEMA)?;
        let mut fields = Fields::array(outer.next("meeting source fields")?, "meeting source")?;
        outer.end("meeting source")?;
        let record = Self {
            source_id: fields.text("source_id")?,
            source_system: fields.text("source_system")?,
            external_id: fields.text("external_id")?,
            source_digest: fields.digest("source_digest")?,
            observed_at_ms: fields.uint("observed_at_ms")?,
            source_created_at_ms: read_optional_u64(&mut fields, "source_created_at_ms")?,
            source_updated_at_ms: read_optional_u64(&mut fields, "source_updated_at_ms")?,
            owner_principal: fields.optional_text("owner_principal")?,
            access_scope: fields.text("access_scope")?,
            coverage: Coverage::from_tag(fields.uint("coverage")?)?,
            sidecar_digest: fields.optional_digest("sidecar_digest")?,
        };
        fields.end("meeting source")?;
        record.validate()?;
        Ok(record)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingRecord {
    pub meeting_id: String,
    pub title: String,
    pub starts_at_ms: Option<u64>,
    pub ends_at_ms: Option<u64>,
    pub calendar_event_ref: Option<String>,
    pub owner_principal: Option<String>,
    pub attendee_refs: Vec<String>,
    pub folder_refs: Vec<String>,
    pub source_refs: Vec<String>,
    pub current_source_digest: Digest,
    pub summary_ref: Option<String>,
    pub status: MeetingStatus,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingRecordInput<'a> {
    pub meeting_id: &'a str,
    pub title: &'a str,
    pub current_source_digest: Digest,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl MeetingRecord {
    pub fn new(input: MeetingRecordInput<'_>) -> Result<Self> {
        let record = Self {
            meeting_id: input.meeting_id.to_string(),
            title: input.title.to_string(),
            starts_at_ms: None,
            ends_at_ms: None,
            calendar_event_ref: None,
            owner_principal: None,
            attendee_refs: Vec::new(),
            folder_refs: Vec::new(),
            source_refs: Vec::new(),
            current_source_digest: input.current_source_digest,
            summary_ref: None,
            status: MeetingStatus::Active,
            created_at_ms: input.created_at_ms,
            updated_at_ms: input.updated_at_ms,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("meeting_id", &self.meeting_id)?;
        validate_text("meeting title", &self.title)?;
        if let Some(starts_at) = self.starts_at_ms
            && let Some(ends_at) = self.ends_at_ms
            && ends_at < starts_at
        {
            return Err(LoomError::invalid("meeting end must not precede start"));
        }
        validate_text_list("attendee_ref", &self.attendee_refs)?;
        validate_text_list("folder_ref", &self.folder_refs)?;
        validate_text_list("source_ref", &self.source_refs)?;
        validate_optional_text("calendar_event_ref", self.calendar_event_ref.as_deref())?;
        validate_optional_text("owner_principal", self.owner_principal.as_deref())?;
        validate_optional_text("summary_ref", self.summary_ref.as_deref())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(MEETING_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.meeting_id.clone()),
                Value::Text(self.title.clone()),
                optional_u64_value(self.starts_at_ms),
                optional_u64_value(self.ends_at_ms),
                optional_text_value(self.calendar_event_ref.as_deref()),
                optional_text_value(self.owner_principal.as_deref()),
                string_array(&self.attendee_refs),
                string_array(&self.folder_refs),
                string_array(&self.source_refs),
                Value::Text(self.current_source_digest.to_string()),
                optional_text_value(self.summary_ref.as_deref()),
                Value::Uint(self.status.tag()),
                Value::Uint(self.created_at_ms),
                Value::Uint(self.updated_at_ms),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "meeting record")?;
        outer.expect_text(MEETING_SCHEMA)?;
        let mut fields = Fields::array(outer.next("meeting record fields")?, "meeting record")?;
        outer.end("meeting record")?;
        let record = Self {
            meeting_id: fields.text("meeting_id")?,
            title: fields.text("title")?,
            starts_at_ms: read_optional_u64(&mut fields, "starts_at_ms")?,
            ends_at_ms: read_optional_u64(&mut fields, "ends_at_ms")?,
            calendar_event_ref: fields.optional_text("calendar_event_ref")?,
            owner_principal: fields.optional_text("owner_principal")?,
            attendee_refs: fields.string_array("attendee_refs")?,
            folder_refs: fields.string_array("folder_refs")?,
            source_refs: fields.string_array("source_refs")?,
            current_source_digest: fields.digest("current_source_digest")?,
            summary_ref: fields.optional_text("summary_ref")?,
            status: MeetingStatus::from_tag(fields.uint("status")?)?,
            created_at_ms: fields.uint("created_at_ms")?,
            updated_at_ms: fields.uint("updated_at_ms")?,
        };
        fields.end("meeting record")?;
        record.validate()?;
        Ok(record)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanRecord {
    pub span_id: String,
    pub meeting_id: String,
    pub source_id: String,
    pub span_kind: SpanKind,
    pub locator: String,
    pub speaker_ref: Option<String>,
    pub speaker_source: Option<String>,
    pub text_digest: Option<Digest>,
    pub language: Option<String>,
    pub redaction_state: RedactionState,
}

impl SpanRecord {
    pub fn new(
        span_id: impl Into<String>,
        meeting_id: impl Into<String>,
        source_id: impl Into<String>,
        span_kind: SpanKind,
        locator: impl Into<String>,
    ) -> Result<Self> {
        let record = Self {
            span_id: span_id.into(),
            meeting_id: meeting_id.into(),
            source_id: source_id.into(),
            span_kind,
            locator: locator.into(),
            speaker_ref: None,
            speaker_source: None,
            text_digest: None,
            language: None,
            redaction_state: RedactionState::Live,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("span_id", &self.span_id)?;
        validate_text("span meeting_id", &self.meeting_id)?;
        validate_text("span source_id", &self.source_id)?;
        validate_text("span locator", &self.locator)?;
        validate_optional_text("speaker_ref", self.speaker_ref.as_deref())?;
        validate_optional_text("speaker_source", self.speaker_source.as_deref())?;
        validate_optional_text("language", self.language.as_deref())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(SPAN_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.span_id.clone()),
                Value::Text(self.meeting_id.clone()),
                Value::Text(self.source_id.clone()),
                Value::Uint(self.span_kind.tag()),
                Value::Text(self.locator.clone()),
                optional_text_value(self.speaker_ref.as_deref()),
                optional_text_value(self.speaker_source.as_deref()),
                optional_digest_value(self.text_digest),
                optional_text_value(self.language.as_deref()),
                Value::Uint(self.redaction_state.tag()),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "meeting span")?;
        outer.expect_text(SPAN_SCHEMA)?;
        let mut fields = Fields::array(outer.next("meeting span fields")?, "meeting span")?;
        outer.end("meeting span")?;
        let record = Self {
            span_id: fields.text("span_id")?,
            meeting_id: fields.text("meeting_id")?,
            source_id: fields.text("source_id")?,
            span_kind: SpanKind::from_tag(fields.uint("span_kind")?)?,
            locator: fields.text("locator")?,
            speaker_ref: fields.optional_text("speaker_ref")?,
            speaker_source: fields.optional_text("speaker_source")?,
            text_digest: fields.optional_digest("text_digest")?,
            language: fields.optional_text("language")?,
            redaction_state: RedactionState::from_tag(fields.uint("redaction_state")?)?,
        };
        fields.end("meeting span")?;
        record.validate()?;
        Ok(record)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotationRecord {
    pub annotation_id: String,
    pub meeting_id: String,
    pub source_span_ids: Vec<String>,
    pub kind: String,
    pub label: String,
    pub normalized_id: Option<String>,
    pub confidence_ppm: Option<u32>,
    pub evidence_digest: Option<Digest>,
    pub extractor: Option<String>,
    pub status: AnnotationStatus,
    pub created_at_ms: u64,
    pub accepted_by: Option<String>,
    pub accepted_at_ms: Option<u64>,
}

impl AnnotationRecord {
    pub fn new(
        annotation_id: impl Into<String>,
        meeting_id: impl Into<String>,
        source_span_ids: Vec<String>,
        kind: impl Into<String>,
        label: impl Into<String>,
        created_at_ms: u64,
    ) -> Result<Self> {
        let record = Self {
            annotation_id: annotation_id.into(),
            meeting_id: meeting_id.into(),
            source_span_ids,
            kind: kind.into(),
            label: label.into(),
            normalized_id: None,
            confidence_ppm: None,
            evidence_digest: None,
            extractor: None,
            status: AnnotationStatus::Suggested,
            created_at_ms,
            accepted_by: None,
            accepted_at_ms: None,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn accept(&mut self, principal_id: impl Into<String>, accepted_at_ms: u64) -> Result<()> {
        if !matches!(
            self.status,
            AnnotationStatus::Observed | AnnotationStatus::Suggested
        ) {
            return Err(LoomError::invalid(
                "annotation must be observed or suggested to accept",
            ));
        }
        if accepted_at_ms < self.created_at_ms {
            return Err(LoomError::invalid(
                "annotation acceptance must not precede creation",
            ));
        }
        self.status = AnnotationStatus::Accepted;
        self.accepted_by = Some(principal_id.into());
        self.accepted_at_ms = Some(accepted_at_ms);
        self.validate()
    }

    pub fn reject(&mut self) -> Result<()> {
        if !matches!(
            self.status,
            AnnotationStatus::Observed | AnnotationStatus::Suggested
        ) {
            return Err(LoomError::invalid(
                "annotation must be observed or suggested to reject",
            ));
        }
        self.status = AnnotationStatus::Rejected;
        self.accepted_by = None;
        self.accepted_at_ms = None;
        self.validate()
    }

    fn validate(&self) -> Result<()> {
        validate_text("annotation_id", &self.annotation_id)?;
        validate_text("annotation meeting_id", &self.meeting_id)?;
        if self.source_span_ids.is_empty() {
            return Err(LoomError::invalid("annotation requires source spans"));
        }
        validate_text_list("source_span_id", &self.source_span_ids)?;
        validate_text("annotation kind", &self.kind)?;
        validate_text("annotation label", &self.label)?;
        validate_optional_text("normalized_id", self.normalized_id.as_deref())?;
        validate_optional_text("extractor", self.extractor.as_deref())?;
        validate_optional_text("accepted_by", self.accepted_by.as_deref())?;
        if self.status == AnnotationStatus::Accepted
            && (self.accepted_by.is_none() || self.accepted_at_ms.is_none())
        {
            return Err(LoomError::invalid(
                "accepted annotation requires acceptance metadata",
            ));
        }
        if let Some(confidence) = self.confidence_ppm
            && confidence > 1_000_000
        {
            return Err(LoomError::invalid(
                "annotation confidence_ppm exceeds 1000000",
            ));
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(ANNOTATION_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.annotation_id.clone()),
                Value::Text(self.meeting_id.clone()),
                string_array(&self.source_span_ids),
                Value::Text(self.kind.clone()),
                Value::Text(self.label.clone()),
                optional_text_value(self.normalized_id.as_deref()),
                optional_u32_value(self.confidence_ppm),
                optional_digest_value(self.evidence_digest),
                optional_text_value(self.extractor.as_deref()),
                Value::Uint(self.status.tag()),
                Value::Uint(self.created_at_ms),
                optional_text_value(self.accepted_by.as_deref()),
                optional_u64_value(self.accepted_at_ms),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "meeting annotation")?;
        outer.expect_text(ANNOTATION_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("meeting annotation fields")?,
            "meeting annotation",
        )?;
        outer.end("meeting annotation")?;
        let record = Self {
            annotation_id: fields.text("annotation_id")?,
            meeting_id: fields.text("meeting_id")?,
            source_span_ids: fields.string_array("source_span_ids")?,
            kind: fields.text("kind")?,
            label: fields.text("label")?,
            normalized_id: fields.optional_text("normalized_id")?,
            confidence_ppm: read_optional_u32(&mut fields, "confidence_ppm")?,
            evidence_digest: fields.optional_digest("evidence_digest")?,
            extractor: fields.optional_text("extractor")?,
            status: AnnotationStatus::from_tag(fields.uint("status")?)?,
            created_at_ms: fields.uint("created_at_ms")?,
            accepted_by: fields.optional_text("accepted_by")?,
            accepted_at_ms: read_optional_u64(&mut fields, "accepted_at_ms")?,
        };
        fields.end("meeting annotation")?;
        record.validate()?;
        Ok(record)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabularyTermRecord {
    pub term_id: String,
    pub kind: String,
    pub label: String,
    pub aliases: Vec<String>,
    pub status: VocabularyTermStatus,
    pub evidence_annotation_ids: Vec<String>,
    pub proposed_by: Option<String>,
    pub reviewed_by: Option<String>,
    pub created_at_ms: u64,
    pub reviewed_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabularyTermInput<'a> {
    pub term_id: &'a str,
    pub kind: &'a str,
    pub label: &'a str,
    pub evidence_annotation_ids: Vec<String>,
    pub created_at_ms: u64,
}

impl VocabularyTermRecord {
    pub fn new(input: VocabularyTermInput<'_>) -> Result<Self> {
        let record = Self {
            term_id: input.term_id.to_string(),
            kind: input.kind.to_string(),
            label: input.label.to_string(),
            aliases: Vec::new(),
            status: VocabularyTermStatus::Proposed,
            evidence_annotation_ids: input.evidence_annotation_ids,
            proposed_by: None,
            reviewed_by: None,
            created_at_ms: input.created_at_ms,
            reviewed_at_ms: None,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn accept(&mut self, reviewer: impl Into<String>, reviewed_at_ms: u64) -> Result<()> {
        if self.status != VocabularyTermStatus::Proposed {
            return Err(LoomError::invalid(
                "vocabulary term must be proposed to accept",
            ));
        }
        if reviewed_at_ms < self.created_at_ms {
            return Err(LoomError::invalid(
                "vocabulary review must not precede creation",
            ));
        }
        self.status = VocabularyTermStatus::Accepted;
        self.reviewed_by = Some(reviewer.into());
        self.reviewed_at_ms = Some(reviewed_at_ms);
        self.validate()
    }

    pub fn reject(&mut self, reviewer: impl Into<String>, reviewed_at_ms: u64) -> Result<()> {
        if self.status != VocabularyTermStatus::Proposed {
            return Err(LoomError::invalid(
                "vocabulary term must be proposed to reject",
            ));
        }
        if reviewed_at_ms < self.created_at_ms {
            return Err(LoomError::invalid(
                "vocabulary review must not precede creation",
            ));
        }
        self.status = VocabularyTermStatus::Rejected;
        self.reviewed_by = Some(reviewer.into());
        self.reviewed_at_ms = Some(reviewed_at_ms);
        self.validate()
    }

    fn validate(&self) -> Result<()> {
        validate_text("vocabulary term_id", &self.term_id)?;
        validate_text("vocabulary kind", &self.kind)?;
        validate_text("vocabulary label", &self.label)?;
        validate_text_list("vocabulary alias", &self.aliases)?;
        if self.evidence_annotation_ids.is_empty() {
            return Err(LoomError::invalid(
                "vocabulary term requires annotation evidence",
            ));
        }
        validate_text_list(
            "vocabulary evidence annotation",
            &self.evidence_annotation_ids,
        )?;
        validate_optional_text("vocabulary proposed_by", self.proposed_by.as_deref())?;
        validate_optional_text("vocabulary reviewed_by", self.reviewed_by.as_deref())?;
        if self.status != VocabularyTermStatus::Proposed
            && (self.reviewed_by.is_none() || self.reviewed_at_ms.is_none())
        {
            return Err(LoomError::invalid(
                "reviewed vocabulary term requires review metadata",
            ));
        }
        if let Some(reviewed_at) = self.reviewed_at_ms
            && reviewed_at < self.created_at_ms
        {
            return Err(LoomError::invalid(
                "vocabulary review must not precede creation",
            ));
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(VOCABULARY_TERM_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.term_id.clone()),
                Value::Text(self.kind.clone()),
                Value::Text(self.label.clone()),
                string_array(&self.aliases),
                Value::Uint(self.status.tag()),
                string_array(&self.evidence_annotation_ids),
                optional_text_value(self.proposed_by.as_deref()),
                optional_text_value(self.reviewed_by.as_deref()),
                Value::Uint(self.created_at_ms),
                optional_u64_value(self.reviewed_at_ms),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "meeting vocabulary term")?;
        outer.expect_text(VOCABULARY_TERM_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("meeting vocabulary term fields")?,
            "meeting vocabulary term",
        )?;
        outer.end("meeting vocabulary term")?;
        let record = Self {
            term_id: fields.text("term_id")?,
            kind: fields.text("kind")?,
            label: fields.text("label")?,
            aliases: fields.string_array("aliases")?,
            status: VocabularyTermStatus::from_tag(fields.uint("status")?)?,
            evidence_annotation_ids: fields.string_array("evidence_annotation_ids")?,
            proposed_by: fields.optional_text("proposed_by")?,
            reviewed_by: fields.optional_text("reviewed_by")?,
            created_at_ms: fields.uint("created_at_ms")?,
            reviewed_at_ms: read_optional_u64(&mut fields, "reviewed_at_ms")?,
        };
        fields.end("meeting vocabulary term")?;
        record.validate()?;
        Ok(record)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityMergeRecord {
    pub merge_id: String,
    pub canonical_entity_id: String,
    pub merged_entity_ids: Vec<String>,
    pub evidence_annotation_ids: Vec<String>,
    pub decided_by: String,
    pub decided_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityMergeInput<'a> {
    pub merge_id: &'a str,
    pub canonical_entity_id: &'a str,
    pub merged_entity_ids: Vec<String>,
    pub evidence_annotation_ids: Vec<String>,
    pub decided_by: &'a str,
    pub decided_at_ms: u64,
}

impl EntityMergeRecord {
    pub fn new(input: EntityMergeInput<'_>) -> Result<Self> {
        let record = Self {
            merge_id: input.merge_id.to_string(),
            canonical_entity_id: input.canonical_entity_id.to_string(),
            merged_entity_ids: input.merged_entity_ids,
            evidence_annotation_ids: input.evidence_annotation_ids,
            decided_by: input.decided_by.to_string(),
            decided_at_ms: input.decided_at_ms,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("entity merge_id", &self.merge_id)?;
        validate_text("canonical_entity_id", &self.canonical_entity_id)?;
        if self.merged_entity_ids.is_empty() {
            return Err(LoomError::invalid("entity merge requires merged entities"));
        }
        if self.evidence_annotation_ids.is_empty() {
            return Err(LoomError::invalid(
                "entity merge requires annotation evidence",
            ));
        }
        validate_text_list("merged_entity_id", &self.merged_entity_ids)?;
        validate_text_list(
            "entity merge evidence annotation",
            &self.evidence_annotation_ids,
        )?;
        validate_text("entity merge decided_by", &self.decided_by)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(ENTITY_MERGE_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.merge_id.clone()),
                Value::Text(self.canonical_entity_id.clone()),
                string_array(&self.merged_entity_ids),
                string_array(&self.evidence_annotation_ids),
                Value::Text(self.decided_by.clone()),
                Value::Uint(self.decided_at_ms),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "meeting entity merge")?;
        outer.expect_text(ENTITY_MERGE_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("meeting entity merge fields")?,
            "meeting entity merge",
        )?;
        outer.end("meeting entity merge")?;
        let record = Self {
            merge_id: fields.text("merge_id")?,
            canonical_entity_id: fields.text("canonical_entity_id")?,
            merged_entity_ids: fields.string_array("merged_entity_ids")?,
            evidence_annotation_ids: fields.string_array("evidence_annotation_ids")?,
            decided_by: fields.text("decided_by")?,
            decided_at_ms: fields.uint("decided_at_ms")?,
        };
        fields.end("meeting entity merge")?;
        record.validate()?;
        Ok(record)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionRecord {
    pub promotion_id: String,
    pub operation_kind: String,
    pub source_annotation_id: String,
    pub target_profile: String,
    pub target_entity_ref: String,
    pub promoted_by: String,
    pub promoted_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionInput<'a> {
    pub promotion_id: &'a str,
    pub operation_kind: &'a str,
    pub source_annotation_id: &'a str,
    pub target_profile: &'a str,
    pub target_entity_ref: &'a str,
    pub promoted_by: &'a str,
    pub promoted_at_ms: u64,
}

impl PromotionRecord {
    pub fn new(input: PromotionInput<'_>) -> Result<Self> {
        let record = Self {
            promotion_id: input.promotion_id.to_string(),
            operation_kind: input.operation_kind.to_string(),
            source_annotation_id: input.source_annotation_id.to_string(),
            target_profile: input.target_profile.to_string(),
            target_entity_ref: input.target_entity_ref.to_string(),
            promoted_by: input.promoted_by.to_string(),
            promoted_at_ms: input.promoted_at_ms,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("promotion_id", &self.promotion_id)?;
        validate_text("promotion operation_kind", &self.operation_kind)?;
        validate_text("promotion source_annotation_id", &self.source_annotation_id)?;
        validate_text("promotion target_profile", &self.target_profile)?;
        validate_text("promotion target_entity_ref", &self.target_entity_ref)?;
        validate_text("promotion promoted_by", &self.promoted_by)?;
        StudioPromotionTarget::new(&self.target_profile, &self.target_entity_ref)?;
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PROMOTION_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.promotion_id.clone()),
                Value::Text(self.operation_kind.clone()),
                Value::Text(self.source_annotation_id.clone()),
                Value::Text(self.target_profile.clone()),
                Value::Text(self.target_entity_ref.clone()),
                Value::Text(self.promoted_by.clone()),
                Value::Uint(self.promoted_at_ms),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "meeting promotion")?;
        outer.expect_text(PROMOTION_SCHEMA)?;
        let mut fields =
            Fields::array(outer.next("meeting promotion fields")?, "meeting promotion")?;
        outer.end("meeting promotion")?;
        let record = Self {
            promotion_id: fields.text("promotion_id")?,
            operation_kind: fields.text("operation_kind")?,
            source_annotation_id: fields.text("source_annotation_id")?,
            target_profile: fields.text("target_profile")?,
            target_entity_ref: fields.text("target_entity_ref")?,
            promoted_by: fields.text("promoted_by")?,
            promoted_at_ms: fields.uint("promoted_at_ms")?,
        };
        fields.end("meeting promotion")?;
        record.validate()?;
        Ok(record)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionReviewProjection {
    pub workspace_id: String,
    pub suggested_annotation_ids: Vec<String>,
    pub accepted_annotation_ids: Vec<String>,
    pub rejected_annotation_ids: Vec<String>,
    pub vocabulary_terms: Vec<VocabularyTermRecord>,
    pub entity_merges: Vec<EntityMergeRecord>,
}

impl ExtractionReviewProjection {
    pub fn new(
        workspace_id: impl Into<String>,
        annotations: &[AnnotationRecord],
        vocabulary_terms: Vec<VocabularyTermRecord>,
        entity_merges: Vec<EntityMergeRecord>,
    ) -> Result<Self> {
        let mut projection = Self {
            workspace_id: workspace_id.into(),
            suggested_annotation_ids: Vec::new(),
            accepted_annotation_ids: Vec::new(),
            rejected_annotation_ids: Vec::new(),
            vocabulary_terms,
            entity_merges,
        };
        for annotation in annotations {
            match annotation.status {
                AnnotationStatus::Observed | AnnotationStatus::Suggested => projection
                    .suggested_annotation_ids
                    .push(annotation.annotation_id.clone()),
                AnnotationStatus::Accepted | AnnotationStatus::Merged => projection
                    .accepted_annotation_ids
                    .push(annotation.annotation_id.clone()),
                AnnotationStatus::Rejected | AnnotationStatus::Superseded => projection
                    .rejected_annotation_ids
                    .push(annotation.annotation_id.clone()),
            }
        }
        projection.suggested_annotation_ids.sort();
        projection.accepted_annotation_ids.sort();
        projection.rejected_annotation_ids.sort();
        projection.validate()?;
        Ok(projection)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("extraction review workspace_id", &self.workspace_id)?;
        validate_text_list("suggested annotation id", &self.suggested_annotation_ids)?;
        validate_text_list("accepted annotation id", &self.accepted_annotation_ids)?;
        validate_text_list("rejected annotation id", &self.rejected_annotation_ids)?;
        unique_ids(
            "vocabulary term ids",
            self.vocabulary_terms
                .iter()
                .map(|term| term.term_id.as_str()),
        )?;
        unique_ids(
            "entity merge ids",
            self.entity_merges
                .iter()
                .map(|merge| merge.merge_id.as_str()),
        )?;
        for term in &self.vocabulary_terms {
            term.validate()?;
        }
        for merge in &self.entity_merges {
            merge.validate()?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(EXTRACTION_REVIEW_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                string_array(&self.suggested_annotation_ids),
                string_array(&self.accepted_annotation_ids),
                string_array(&self.rejected_annotation_ids),
                Value::Array(
                    self.vocabulary_terms
                        .iter()
                        .map(VocabularyTermRecord::to_value)
                        .collect(),
                ),
                Value::Array(
                    self.entity_merges
                        .iter()
                        .map(EntityMergeRecord::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "meeting extraction review")?;
        outer.expect_text(EXTRACTION_REVIEW_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("meeting extraction review fields")?,
            "meeting extraction review",
        )?;
        outer.end("meeting extraction review")?;
        let projection = Self {
            workspace_id: fields.text("workspace_id")?,
            suggested_annotation_ids: fields.string_array("suggested_annotation_ids")?,
            accepted_annotation_ids: fields.string_array("accepted_annotation_ids")?,
            rejected_annotation_ids: fields.string_array("rejected_annotation_ids")?,
            vocabulary_terms: vocabulary_term_list(fields.next("vocabulary_terms")?)?,
            entity_merges: entity_merge_list(fields.next("entity_merges")?)?,
        };
        fields.end("meeting extraction review")?;
        projection.validate()?;
        Ok(projection)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRunRecord {
    pub import_run_id: String,
    pub input_profile: InputProfile,
    pub source_scope: String,
    pub coverage: Coverage,
    pub source_cursor: Option<String>,
    pub source_sidecar_digest: Option<Digest>,
    pub observed_ids: Vec<String>,
    pub coverage_gaps: Vec<String>,
    pub retry_windows: Vec<String>,
    pub resume_state: Option<String>,
    pub started_at_ms: u64,
    pub completed_at_ms: Option<u64>,
}

impl ImportRunRecord {
    pub fn new(
        import_run_id: impl Into<String>,
        input_profile: InputProfile,
        source_scope: impl Into<String>,
        coverage: Coverage,
        started_at_ms: u64,
    ) -> Result<Self> {
        let record = Self {
            import_run_id: import_run_id.into(),
            input_profile,
            source_scope: source_scope.into(),
            coverage,
            source_cursor: None,
            source_sidecar_digest: None,
            observed_ids: Vec::new(),
            coverage_gaps: Vec::new(),
            retry_windows: Vec::new(),
            resume_state: None,
            started_at_ms,
            completed_at_ms: None,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("import_run_id", &self.import_run_id)?;
        validate_text("source_scope", &self.source_scope)?;
        validate_optional_text("source_cursor", self.source_cursor.as_deref())?;
        validate_text_list("observed_id", &self.observed_ids)?;
        validate_text_list("coverage_gap", &self.coverage_gaps)?;
        validate_text_list("retry_window", &self.retry_windows)?;
        validate_optional_text("resume_state", self.resume_state.as_deref())?;
        if let Some(completed_at) = self.completed_at_ms
            && completed_at < self.started_at_ms
        {
            return Err(LoomError::invalid(
                "import run completion must not precede start",
            ));
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(IMPORT_RUN_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.import_run_id.clone()),
                Value::Uint(self.input_profile.tag()),
                Value::Text(self.source_scope.clone()),
                Value::Uint(self.coverage.tag()),
                optional_text_value(self.source_cursor.as_deref()),
                optional_digest_value(self.source_sidecar_digest),
                string_array(&self.observed_ids),
                string_array(&self.coverage_gaps),
                string_array(&self.retry_windows),
                optional_text_value(self.resume_state.as_deref()),
                Value::Uint(self.started_at_ms),
                optional_u64_value(self.completed_at_ms),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "meeting import run")?;
        outer.expect_text(IMPORT_RUN_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("meeting import run fields")?,
            "meeting import run",
        )?;
        outer.end("meeting import run")?;
        let record = Self {
            import_run_id: fields.text("import_run_id")?,
            input_profile: InputProfile::from_tag(fields.uint("input_profile")?)?,
            source_scope: fields.text("source_scope")?,
            coverage: Coverage::from_tag(fields.uint("coverage")?)?,
            source_cursor: fields.optional_text("source_cursor")?,
            source_sidecar_digest: fields.optional_digest("source_sidecar_digest")?,
            observed_ids: fields.string_array("observed_ids")?,
            coverage_gaps: fields.string_array("coverage_gaps")?,
            retry_windows: fields.string_array("retry_windows")?,
            resume_state: fields.optional_text("resume_state")?,
            started_at_ms: fields.uint("started_at_ms")?,
            completed_at_ms: read_optional_u64(&mut fields, "completed_at_ms")?,
        };
        fields.end("meeting import run")?;
        record.validate()?;
        Ok(record)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactionRecord {
    pub redaction_id: String,
    pub target_id: String,
    pub target_kind: String,
    pub state: RedactionState,
    pub policy_id: String,
    pub applied_at_ms: u64,
    pub retained_digest: Option<Digest>,
}

impl RedactionRecord {
    pub fn new(
        redaction_id: impl Into<String>,
        target_id: impl Into<String>,
        target_kind: impl Into<String>,
        state: RedactionState,
        policy_id: impl Into<String>,
        applied_at_ms: u64,
    ) -> Result<Self> {
        let record = Self {
            redaction_id: redaction_id.into(),
            target_id: target_id.into(),
            target_kind: target_kind.into(),
            state,
            policy_id: policy_id.into(),
            applied_at_ms,
            retained_digest: None,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("redaction_id", &self.redaction_id)?;
        validate_text("redaction target_id", &self.target_id)?;
        validate_text("redaction target_kind", &self.target_kind)?;
        validate_text("redaction policy_id", &self.policy_id)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(REDACTION_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.redaction_id.clone()),
                Value::Text(self.target_id.clone()),
                Value::Text(self.target_kind.clone()),
                Value::Uint(self.state.tag()),
                Value::Text(self.policy_id.clone()),
                Value::Uint(self.applied_at_ms),
                optional_digest_value(self.retained_digest),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "meeting redaction")?;
        outer.expect_text(REDACTION_SCHEMA)?;
        let mut fields =
            Fields::array(outer.next("meeting redaction fields")?, "meeting redaction")?;
        outer.end("meeting redaction")?;
        let record = Self {
            redaction_id: fields.text("redaction_id")?,
            target_id: fields.text("target_id")?,
            target_kind: fields.text("target_kind")?,
            state: RedactionState::from_tag(fields.uint("state")?)?,
            policy_id: fields.text("policy_id")?,
            applied_at_ms: fields.uint("applied_at_ms")?,
            retained_digest: fields.optional_digest("retained_digest")?,
        };
        fields.end("meeting redaction")?;
        record.validate()?;
        Ok(record)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionEffect {
    pub effect_id: String,
    pub projection: ProjectionKind,
    pub action: ProjectionAction,
    pub entity_kind: String,
    pub entity_id: String,
    pub source_ids: Vec<String>,
    pub output_ref: Option<String>,
    pub payload_digest: Option<Digest>,
    pub redaction_state: Option<RedactionState>,
    pub recorded_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionEffectInput<'a> {
    pub effect_id: &'a str,
    pub projection: ProjectionKind,
    pub action: ProjectionAction,
    pub entity_kind: &'a str,
    pub entity_id: &'a str,
    pub recorded_at_ms: u64,
}

impl ProjectionEffect {
    pub fn new(input: ProjectionEffectInput<'_>) -> Result<Self> {
        let effect = Self {
            effect_id: input.effect_id.to_string(),
            projection: input.projection,
            action: input.action,
            entity_kind: input.entity_kind.to_string(),
            entity_id: input.entity_id.to_string(),
            source_ids: Vec::new(),
            output_ref: None,
            payload_digest: None,
            redaction_state: None,
            recorded_at_ms: input.recorded_at_ms,
        };
        effect.validate()?;
        Ok(effect)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("projection effect_id", &self.effect_id)?;
        validate_text("projection entity_kind", &self.entity_kind)?;
        validate_text("projection entity_id", &self.entity_id)?;
        validate_text_list("projection source_id", &self.source_ids)?;
        validate_optional_text("projection output_ref", self.output_ref.as_deref())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PROJECTION_EFFECT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.effect_id.clone()),
                Value::Uint(self.projection.tag()),
                Value::Uint(self.action.tag()),
                Value::Text(self.entity_kind.clone()),
                Value::Text(self.entity_id.clone()),
                string_array(&self.source_ids),
                optional_text_value(self.output_ref.as_deref()),
                optional_digest_value(self.payload_digest),
                optional_redaction_state_value(self.redaction_state),
                Value::Uint(self.recorded_at_ms),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "meeting projection effect")?;
        outer.expect_text(PROJECTION_EFFECT_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("meeting projection effect fields")?,
            "meeting projection effect",
        )?;
        outer.end("meeting projection effect")?;
        let effect = Self {
            effect_id: fields.text("effect_id")?,
            projection: ProjectionKind::from_tag(fields.uint("projection")?)?,
            action: ProjectionAction::from_tag(fields.uint("action")?)?,
            entity_kind: fields.text("entity_kind")?,
            entity_id: fields.text("entity_id")?,
            source_ids: fields.string_array("source_ids")?,
            output_ref: fields.optional_text("output_ref")?,
            payload_digest: fields.optional_digest("payload_digest")?,
            redaction_state: read_optional_redaction_state(&mut fields, "redaction_state")?,
            recorded_at_ms: fields.uint("recorded_at_ms")?,
        };
        fields.end("meeting projection effect")?;
        effect.validate()?;
        Ok(effect)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionEffectSet {
    pub workspace_id: String,
    pub effects: Vec<ProjectionEffect>,
}

impl ProjectionEffectSet {
    pub fn new(workspace_id: impl Into<String>, effects: Vec<ProjectionEffect>) -> Result<Self> {
        let set = Self {
            workspace_id: workspace_id.into(),
            effects,
        };
        set.validate()?;
        Ok(set)
    }

    pub fn from_snapshot(snapshot: &MeetingsProfileSnapshot) -> Result<Self> {
        let mut effects = Vec::new();
        for source in &snapshot.sources {
            push_effect(
                &mut effects,
                ProjectionSeed {
                    workspace_id: &snapshot.workspace_id,
                    projection: ProjectionKind::Document,
                    action: ProjectionAction::Upsert,
                    entity_kind: "source",
                    entity_id: &source.source_id,
                    recorded_at_ms: source.observed_at_ms,
                    source_ids: &[source.source_id.as_str()],
                    output_ref: Some(format!("document:source/{}", source.source_id)),
                    payload_digest: Some(source.source_digest),
                    redaction_state: None,
                },
            )?;
            push_effect(
                &mut effects,
                ProjectionSeed {
                    workspace_id: &snapshot.workspace_id,
                    projection: ProjectionKind::Files,
                    action: ProjectionAction::Upsert,
                    entity_kind: "source_snapshot",
                    entity_id: &source.source_id,
                    recorded_at_ms: source.observed_at_ms,
                    source_ids: &[source.source_id.as_str()],
                    output_ref: Some(format!("files/raw/{}", source.source_id)),
                    payload_digest: Some(source.source_digest),
                    redaction_state: None,
                },
            )?;
        }
        for meeting in &snapshot.meetings {
            let source_ids: Vec<&str> = meeting.source_refs.iter().map(String::as_str).collect();
            for (projection, output_ref) in [
                (
                    ProjectionKind::Document,
                    format!("document:meeting/{}", meeting.meeting_id),
                ),
                (
                    ProjectionKind::Graph,
                    format!("graph:node/meeting/{}", meeting.meeting_id),
                ),
                (
                    ProjectionKind::Search,
                    format!("search:meeting/{}", meeting.meeting_id),
                ),
                (
                    ProjectionKind::SqlDataframe,
                    format!("sql:meetings/{}", meeting.meeting_id),
                ),
            ] {
                push_effect(
                    &mut effects,
                    ProjectionSeed {
                        workspace_id: &snapshot.workspace_id,
                        projection,
                        action: ProjectionAction::Upsert,
                        entity_kind: "meeting",
                        entity_id: &meeting.meeting_id,
                        recorded_at_ms: meeting.updated_at_ms,
                        source_ids: &source_ids,
                        output_ref: Some(output_ref),
                        payload_digest: Some(meeting.current_source_digest),
                        redaction_state: Some(meeting_status_redaction_state(meeting.status)),
                    },
                )?;
            }
        }
        for span in &snapshot.spans {
            let action = match span.redaction_state {
                RedactionState::Live => ProjectionAction::Upsert,
                RedactionState::Redacted => ProjectionAction::Invalidate,
                RedactionState::RetainedMetadataOnly => ProjectionAction::RetainMetadata,
            };
            for (projection, output_ref) in [
                (
                    ProjectionKind::Document,
                    format!("document:span/{}", span.span_id),
                ),
                (
                    ProjectionKind::Files,
                    format!("files/transcript/{}", span.meeting_id),
                ),
                (
                    ProjectionKind::Graph,
                    format!("graph:evidence/{}", span.span_id),
                ),
                (
                    ProjectionKind::Search,
                    format!("search:span/{}", span.span_id),
                ),
                (
                    ProjectionKind::Vector,
                    format!("vector:span/{}", span.span_id),
                ),
                (
                    ProjectionKind::SqlDataframe,
                    format!("sql:spans/{}", span.span_id),
                ),
            ] {
                push_effect(
                    &mut effects,
                    ProjectionSeed {
                        workspace_id: &snapshot.workspace_id,
                        projection,
                        action,
                        entity_kind: "span",
                        entity_id: &span.span_id,
                        recorded_at_ms: 0,
                        source_ids: &[span.source_id.as_str()],
                        output_ref: Some(output_ref),
                        payload_digest: span.text_digest,
                        redaction_state: Some(span.redaction_state),
                    },
                )?;
            }
        }
        for annotation in &snapshot.annotations {
            let action = match annotation.status {
                AnnotationStatus::Accepted => ProjectionAction::Upsert,
                AnnotationStatus::Rejected | AnnotationStatus::Superseded => {
                    ProjectionAction::Invalidate
                }
                AnnotationStatus::Merged => ProjectionAction::RetainMetadata,
                AnnotationStatus::Observed | AnnotationStatus::Suggested => {
                    ProjectionAction::RetainMetadata
                }
            };
            for (projection, output_ref) in [
                (
                    ProjectionKind::Document,
                    format!("document:annotation/{}", annotation.annotation_id),
                ),
                (
                    ProjectionKind::Graph,
                    format!("graph:annotation/{}", annotation.annotation_id),
                ),
                (
                    ProjectionKind::Search,
                    format!("search:annotation/{}", annotation.annotation_id),
                ),
                (
                    ProjectionKind::Vector,
                    format!("vector:annotation/{}", annotation.annotation_id),
                ),
                (
                    ProjectionKind::SqlDataframe,
                    format!("sql:annotations/{}", annotation.annotation_id),
                ),
            ] {
                push_effect(
                    &mut effects,
                    ProjectionSeed {
                        workspace_id: &snapshot.workspace_id,
                        projection,
                        action,
                        entity_kind: "annotation",
                        entity_id: &annotation.annotation_id,
                        recorded_at_ms: annotation.created_at_ms,
                        source_ids: &annotation
                            .source_span_ids
                            .iter()
                            .map(String::as_str)
                            .collect::<Vec<_>>(),
                        output_ref: Some(output_ref),
                        payload_digest: annotation.evidence_digest,
                        redaction_state: None,
                    },
                )?;
            }
            if annotation.status == AnnotationStatus::Accepted {
                push_effect(
                    &mut effects,
                    ProjectionSeed {
                        workspace_id: &snapshot.workspace_id,
                        projection: ProjectionKind::Ledger,
                        action: ProjectionAction::Append,
                        entity_kind: "annotation_accept",
                        entity_id: &annotation.annotation_id,
                        recorded_at_ms: annotation
                            .accepted_at_ms
                            .unwrap_or(annotation.created_at_ms),
                        source_ids: &annotation
                            .source_span_ids
                            .iter()
                            .map(String::as_str)
                            .collect::<Vec<_>>(),
                        output_ref: Some(format!("ledger:annotation/{}", annotation.annotation_id)),
                        payload_digest: annotation.evidence_digest,
                        redaction_state: None,
                    },
                )?;
            }
        }
        for import_run in &snapshot.import_runs {
            for (projection, output_ref) in [
                (
                    ProjectionKind::Document,
                    format!("document:import-run/{}", import_run.import_run_id),
                ),
                (
                    ProjectionKind::Files,
                    format!("files/fidelity/{}", import_run.import_run_id),
                ),
                (
                    ProjectionKind::Ledger,
                    format!("ledger:import-run/{}", import_run.import_run_id),
                ),
            ] {
                push_effect(
                    &mut effects,
                    ProjectionSeed {
                        workspace_id: &snapshot.workspace_id,
                        projection,
                        action: if projection == ProjectionKind::Ledger {
                            ProjectionAction::Append
                        } else {
                            ProjectionAction::Upsert
                        },
                        entity_kind: "import_run",
                        entity_id: &import_run.import_run_id,
                        recorded_at_ms: import_run
                            .completed_at_ms
                            .unwrap_or(import_run.started_at_ms),
                        source_ids: &import_run
                            .observed_ids
                            .iter()
                            .map(String::as_str)
                            .collect::<Vec<_>>(),
                        output_ref: Some(output_ref),
                        payload_digest: import_run.source_sidecar_digest,
                        redaction_state: None,
                    },
                )?;
            }
        }
        for promotion in &snapshot.promotions {
            for (projection, output_ref) in [
                (
                    ProjectionKind::Document,
                    format!("document:promotion/{}", promotion.promotion_id),
                ),
                (
                    ProjectionKind::Graph,
                    format!("graph:promotion/{}", promotion.promotion_id),
                ),
                (
                    ProjectionKind::SqlDataframe,
                    format!("sql:promotions/{}", promotion.promotion_id),
                ),
                (
                    ProjectionKind::Ledger,
                    format!("ledger:promotion/{}", promotion.promotion_id),
                ),
            ] {
                push_effect(
                    &mut effects,
                    ProjectionSeed {
                        workspace_id: &snapshot.workspace_id,
                        projection,
                        action: if projection == ProjectionKind::Ledger {
                            ProjectionAction::Append
                        } else {
                            ProjectionAction::Upsert
                        },
                        entity_kind: "promotion",
                        entity_id: &promotion.promotion_id,
                        recorded_at_ms: promotion.promoted_at_ms,
                        source_ids: &[promotion.source_annotation_id.as_str()],
                        output_ref: Some(output_ref),
                        payload_digest: None,
                        redaction_state: None,
                    },
                )?;
            }
        }
        for redaction in &snapshot.redactions {
            for projection in [
                ProjectionKind::Document,
                ProjectionKind::Files,
                ProjectionKind::Graph,
                ProjectionKind::Search,
                ProjectionKind::Vector,
                ProjectionKind::Ledger,
            ] {
                push_effect(
                    &mut effects,
                    ProjectionSeed {
                        workspace_id: &snapshot.workspace_id,
                        projection,
                        action: match projection {
                            ProjectionKind::Graph => ProjectionAction::RetainMetadata,
                            ProjectionKind::Ledger => ProjectionAction::Append,
                            _ => ProjectionAction::Invalidate,
                        },
                        entity_kind: "redaction",
                        entity_id: &redaction.redaction_id,
                        recorded_at_ms: redaction.applied_at_ms,
                        source_ids: &[redaction.target_id.as_str()],
                        output_ref: Some(format!(
                            "{}:redaction/{}",
                            projection_label(projection),
                            redaction.redaction_id
                        )),
                        payload_digest: redaction.retained_digest,
                        redaction_state: Some(redaction.state),
                    },
                )?;
            }
        }
        Self::new(snapshot.workspace_id.clone(), effects)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn effects_for(&self, projection: ProjectionKind) -> Vec<&ProjectionEffect> {
        self.effects
            .iter()
            .filter(|effect| effect.projection == projection)
            .collect()
    }

    fn validate(&self) -> Result<()> {
        validate_text("projection workspace_id", &self.workspace_id)?;
        unique_ids(
            "projection effect ids",
            self.effects.iter().map(|effect| effect.effect_id.as_str()),
        )?;
        for effect in &self.effects {
            effect.validate()?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PROJECTION_EFFECT_SET_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Array(
                    self.effects
                        .iter()
                        .map(ProjectionEffect::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "meeting projection effect set")?;
        outer.expect_text(PROJECTION_EFFECT_SET_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("meeting projection effect set fields")?,
            "meeting projection effect set",
        )?;
        outer.end("meeting projection effect set")?;
        let workspace_id = fields.text("workspace_id")?;
        let effects = projection_effect_list(fields.next("effects")?)?;
        fields.end("meeting projection effect set")?;
        Self::new(workspace_id, effects)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectionOutput {
    pub output_id: String,
    pub projection: ProjectionKind,
    pub action: ProjectionAction,
    pub output_ref: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub source_ids: Vec<String>,
    pub payload: Value,
    pub redaction_state: Option<RedactionState>,
    pub recorded_at_ms: u64,
}

impl ProjectionOutput {
    fn from_effect(snapshot: &MeetingsProfileSnapshot, effect: &ProjectionEffect) -> Result<Self> {
        let output_ref = effect
            .output_ref
            .clone()
            .ok_or_else(|| LoomError::invalid("projection effect requires output_ref"))?;
        let output = Self {
            output_id: effect.effect_id.clone(),
            projection: effect.projection,
            action: effect.action,
            output_ref,
            entity_kind: effect.entity_kind.clone(),
            entity_id: effect.entity_id.clone(),
            source_ids: effect.source_ids.clone(),
            payload: projection_payload(snapshot, effect)?,
            redaction_state: effect.redaction_state,
            recorded_at_ms: effect.recorded_at_ms,
        };
        output.validate()?;
        Ok(output)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn text_body(&self) -> String {
        format!(
            "{} {} {} {} {}",
            self.entity_kind,
            self.entity_id,
            self.output_ref,
            self.source_ids.join(" "),
            value_text(&self.payload)
        )
    }

    fn validate(&self) -> Result<()> {
        validate_text("projection output_id", &self.output_id)?;
        validate_text("projection output_ref", &self.output_ref)?;
        validate_text("projection output entity_kind", &self.entity_kind)?;
        validate_text("projection output entity_id", &self.entity_id)?;
        validate_text_list("projection output source_id", &self.source_ids)?;
        validate_projection_payload(
            &self.payload,
            self.projection,
            self.action,
            &self.entity_kind,
            &self.entity_id,
        )
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PROJECTION_OUTPUT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.output_id.clone()),
                Value::Uint(self.projection.tag()),
                Value::Uint(self.action.tag()),
                Value::Text(self.output_ref.clone()),
                Value::Text(self.entity_kind.clone()),
                Value::Text(self.entity_id.clone()),
                string_array(&self.source_ids),
                self.payload.clone(),
                optional_redaction_state_value(self.redaction_state),
                Value::Uint(self.recorded_at_ms),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "meeting projection output")?;
        outer.expect_text(PROJECTION_OUTPUT_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("meeting projection output fields")?,
            "meeting projection output",
        )?;
        outer.end("meeting projection output")?;
        let output = Self {
            output_id: fields.text("output_id")?,
            projection: ProjectionKind::from_tag(fields.uint("projection")?)?,
            action: ProjectionAction::from_tag(fields.uint("action")?)?,
            output_ref: fields.text("output_ref")?,
            entity_kind: fields.text("entity_kind")?,
            entity_id: fields.text("entity_id")?,
            source_ids: fields.string_array("source_ids")?,
            payload: fields.next("payload")?,
            redaction_state: read_optional_redaction_state(&mut fields, "redaction_state")?,
            recorded_at_ms: fields.uint("recorded_at_ms")?,
        };
        fields.end("meeting projection output")?;
        output.validate()?;
        Ok(output)
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::Text(value) => value.clone(),
        Value::Bytes(bytes) => hex_text(bytes),
        Value::Array(values) => values.iter().map(value_text).collect::<Vec<_>>().join(" "),
        Value::Map(values) => values
            .iter()
            .map(|(key, value)| format!("{} {}", value_text(key), value_text(value)))
            .collect::<Vec<_>>()
            .join(" "),
        Value::Uint(value) => value.to_string(),
        Value::Nint(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Null => String::new(),
        Value::Float(value) => value.to_string(),
    }
}

fn hex_text(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectionOutputSet {
    pub workspace_id: String,
    pub outputs: Vec<ProjectionOutput>,
}

impl ProjectionOutputSet {
    pub fn from_snapshot(snapshot: &MeetingsProfileSnapshot) -> Result<Self> {
        let effects = ProjectionEffectSet::from_snapshot(snapshot)?;
        let mut outputs = effects
            .effects
            .iter()
            .map(|effect| ProjectionOutput::from_effect(snapshot, effect))
            .collect::<Result<Vec<_>>>()?;
        outputs.sort_by(|left, right| left.output_id.cmp(&right.output_id));
        Self::new(snapshot.workspace_id.clone(), outputs)
    }

    pub fn new(workspace_id: impl Into<String>, outputs: Vec<ProjectionOutput>) -> Result<Self> {
        let set = Self {
            workspace_id: workspace_id.into(),
            outputs,
        };
        set.validate()?;
        Ok(set)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn outputs_for(&self, projection: ProjectionKind) -> Vec<&ProjectionOutput> {
        self.outputs
            .iter()
            .filter(|output| output.projection == projection)
            .collect()
    }

    fn validate(&self) -> Result<()> {
        validate_text("projection output workspace_id", &self.workspace_id)?;
        unique_ids(
            "projection output ids",
            self.outputs.iter().map(|output| output.output_id.as_str()),
        )?;
        for output in &self.outputs {
            output.validate()?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PROJECTION_OUTPUT_SET_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Array(
                    self.outputs
                        .iter()
                        .map(ProjectionOutput::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "meeting projection output set")?;
        outer.expect_text(PROJECTION_OUTPUT_SET_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("meeting projection output set fields")?,
            "meeting projection output set",
        )?;
        outer.end("meeting projection output set")?;
        let workspace_id = fields.text("workspace_id")?;
        let outputs = projection_output_list(fields.next("outputs")?)?;
        fields.end("meeting projection output set")?;
        Self::new(workspace_id, outputs)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingsProfileSnapshot {
    pub workspace_id: String,
    pub sources: Vec<SourceRecord>,
    pub meetings: Vec<MeetingRecord>,
    pub spans: Vec<SpanRecord>,
    pub annotations: Vec<AnnotationRecord>,
    pub vocabulary_terms: Vec<VocabularyTermRecord>,
    pub entity_merges: Vec<EntityMergeRecord>,
    pub promotions: Vec<PromotionRecord>,
    pub import_runs: Vec<ImportRunRecord>,
    pub redactions: Vec<RedactionRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingsProfileSnapshotParts {
    pub sources: Vec<SourceRecord>,
    pub meetings: Vec<MeetingRecord>,
    pub spans: Vec<SpanRecord>,
    pub annotations: Vec<AnnotationRecord>,
    pub vocabulary_terms: Vec<VocabularyTermRecord>,
    pub entity_merges: Vec<EntityMergeRecord>,
    pub promotions: Vec<PromotionRecord>,
    pub import_runs: Vec<ImportRunRecord>,
    pub redactions: Vec<RedactionRecord>,
}

impl MeetingsProfileSnapshot {
    pub fn new(
        workspace_id: impl Into<String>,
        parts: MeetingsProfileSnapshotParts,
    ) -> Result<Self> {
        let snapshot = Self {
            workspace_id: workspace_id.into(),
            sources: parts.sources,
            meetings: parts.meetings,
            spans: parts.spans,
            annotations: parts.annotations,
            vocabulary_terms: parts.vocabulary_terms,
            entity_merges: parts.entity_merges,
            promotions: parts.promotions,
            import_runs: parts.import_runs,
            redactions: parts.redactions,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn accept_annotation(
        &mut self,
        annotation_id: &str,
        principal_id: impl Into<String>,
        accepted_at_ms: u64,
    ) -> Result<AnnotationRecord> {
        let annotation = self
            .annotations
            .iter_mut()
            .find(|annotation| annotation.annotation_id == annotation_id)
            .ok_or_else(|| LoomError::not_found("meeting annotation not found"))?;
        annotation.accept(principal_id, accepted_at_ms)?;
        Ok(annotation.clone())
    }

    pub fn reject_annotation(&mut self, annotation_id: &str) -> Result<AnnotationRecord> {
        let annotation = self
            .annotations
            .iter_mut()
            .find(|annotation| annotation.annotation_id == annotation_id)
            .ok_or_else(|| LoomError::not_found("meeting annotation not found"))?;
        annotation.reject()?;
        Ok(annotation.clone())
    }

    pub fn add_vocabulary_term(
        &mut self,
        term: VocabularyTermRecord,
    ) -> Result<VocabularyTermRecord> {
        if self
            .vocabulary_terms
            .iter()
            .any(|existing| existing.term_id == term.term_id)
        {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "meeting vocabulary term already exists",
            ));
        }
        let added = term.clone();
        self.vocabulary_terms.push(term);
        self.validate()?;
        Ok(added)
    }

    pub fn accept_vocabulary_term(
        &mut self,
        term_id: &str,
        reviewer: impl Into<String>,
        reviewed_at_ms: u64,
    ) -> Result<VocabularyTermRecord> {
        let term = self
            .vocabulary_terms
            .iter_mut()
            .find(|term| term.term_id == term_id)
            .ok_or_else(|| LoomError::not_found("meeting vocabulary term not found"))?;
        term.accept(reviewer, reviewed_at_ms)?;
        Ok(term.clone())
    }

    pub fn reject_vocabulary_term(
        &mut self,
        term_id: &str,
        reviewer: impl Into<String>,
        reviewed_at_ms: u64,
    ) -> Result<VocabularyTermRecord> {
        let term = self
            .vocabulary_terms
            .iter_mut()
            .find(|term| term.term_id == term_id)
            .ok_or_else(|| LoomError::not_found("meeting vocabulary term not found"))?;
        term.reject(reviewer, reviewed_at_ms)?;
        Ok(term.clone())
    }

    pub fn add_entity_merge(&mut self, merge: EntityMergeRecord) -> Result<EntityMergeRecord> {
        if self
            .entity_merges
            .iter()
            .any(|existing| existing.merge_id == merge.merge_id)
        {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "meeting entity merge already exists",
            ));
        }
        let added = merge.clone();
        self.entity_merges.push(merge);
        self.validate()?;
        Ok(added)
    }

    pub fn add_promotion(&mut self, promotion: PromotionRecord) -> Result<PromotionRecord> {
        if self
            .promotions
            .iter()
            .any(|existing| existing.promotion_id == promotion.promotion_id)
        {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "meeting promotion already exists",
            ));
        }
        let annotation = self
            .annotations
            .iter()
            .find(|annotation| annotation.annotation_id == promotion.source_annotation_id)
            .ok_or_else(|| LoomError::not_found("meeting promotion annotation not found"))?;
        match annotation.status {
            AnnotationStatus::Observed | AnnotationStatus::Accepted | AnnotationStatus::Merged => {}
            AnnotationStatus::Suggested
            | AnnotationStatus::Rejected
            | AnnotationStatus::Superseded => {
                return Err(LoomError::invalid(
                    "meeting promotion requires observed or accepted annotation evidence",
                ));
            }
        }
        validate_studio_promotion(
            &annotation.kind,
            &promotion.operation_kind,
            &promotion.target_profile,
            &promotion.target_entity_ref,
        )?;
        let added = promotion.clone();
        self.promotions.push(promotion);
        self.validate()?;
        Ok(added)
    }

    fn validate(&self) -> Result<()> {
        validate_text("workspace_id", &self.workspace_id)?;
        unique_ids(
            "source ids",
            self.sources.iter().map(|source| source.source_id.as_str()),
        )?;
        unique_ids(
            "meeting ids",
            self.meetings
                .iter()
                .map(|meeting| meeting.meeting_id.as_str()),
        )?;
        unique_ids(
            "span ids",
            self.spans.iter().map(|span| span.span_id.as_str()),
        )?;
        unique_ids(
            "annotation ids",
            self.annotations
                .iter()
                .map(|annotation| annotation.annotation_id.as_str()),
        )?;
        unique_ids(
            "vocabulary term ids",
            self.vocabulary_terms
                .iter()
                .map(|term| term.term_id.as_str()),
        )?;
        unique_ids(
            "entity merge ids",
            self.entity_merges
                .iter()
                .map(|merge| merge.merge_id.as_str()),
        )?;
        unique_ids(
            "promotion ids",
            self.promotions
                .iter()
                .map(|promotion| promotion.promotion_id.as_str()),
        )?;
        for source in &self.sources {
            source.validate()?;
        }
        for meeting in &self.meetings {
            meeting.validate()?;
        }
        for span in &self.spans {
            span.validate()?;
        }
        for annotation in &self.annotations {
            annotation.validate()?;
        }
        for term in &self.vocabulary_terms {
            term.validate()?;
        }
        for merge in &self.entity_merges {
            merge.validate()?;
        }
        for promotion in &self.promotions {
            promotion.validate()?;
        }
        for import_run in &self.import_runs {
            import_run.validate()?;
        }
        for redaction in &self.redactions {
            redaction.validate()?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PROFILE_SNAPSHOT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Array(self.sources.iter().map(SourceRecord::to_value).collect()),
                Value::Array(self.meetings.iter().map(MeetingRecord::to_value).collect()),
                Value::Array(self.spans.iter().map(SpanRecord::to_value).collect()),
                Value::Array(
                    self.annotations
                        .iter()
                        .map(AnnotationRecord::to_value)
                        .collect(),
                ),
                Value::Array(
                    self.vocabulary_terms
                        .iter()
                        .map(VocabularyTermRecord::to_value)
                        .collect(),
                ),
                Value::Array(
                    self.entity_merges
                        .iter()
                        .map(EntityMergeRecord::to_value)
                        .collect(),
                ),
                Value::Array(
                    self.promotions
                        .iter()
                        .map(PromotionRecord::to_value)
                        .collect(),
                ),
                Value::Array(
                    self.import_runs
                        .iter()
                        .map(ImportRunRecord::to_value)
                        .collect(),
                ),
                Value::Array(
                    self.redactions
                        .iter()
                        .map(RedactionRecord::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "meetings profile snapshot")?;
        outer.expect_text(PROFILE_SNAPSHOT_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("meetings profile snapshot fields")?,
            "meetings profile snapshot",
        )?;
        outer.end("meetings profile snapshot")?;
        let workspace_id = fields.text("workspace_id")?;
        let sources = source_list(fields.next("sources")?)?;
        let meetings = meeting_list(fields.next("meetings")?)?;
        let spans = span_list(fields.next("spans")?)?;
        let annotations = annotation_list(fields.next("annotations")?)?;
        let vocabulary_terms = vocabulary_term_list(fields.next("vocabulary_terms")?)?;
        let entity_merges = entity_merge_list(fields.next("entity_merges")?)?;
        let promotions = promotion_list(fields.next("promotions")?)?;
        let import_runs = import_run_list(fields.next("import_runs")?)?;
        let redactions = redaction_list(fields.next("redactions")?)?;
        fields.end("meetings profile snapshot")?;
        Self::new(
            workspace_id,
            MeetingsProfileSnapshotParts {
                sources,
                meetings,
                spans,
                annotations,
                vocabulary_terms,
                entity_merges,
                promotions,
                import_runs,
                redactions,
            },
        )
    }
}

fn string_array(values: &[String]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|value| Value::Text(value.clone()))
            .collect(),
    )
}

fn optional_u32_value(value: Option<u32>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), Value::Uint(u64::from(value))]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn optional_u64_value(value: Option<u64>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), Value::Uint(value)]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn optional_digest_value(value: Option<Digest>) -> Value {
    optional_text_value(value.map(|digest| digest.to_string()).as_deref())
}

fn optional_redaction_state_value(value: Option<RedactionState>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), Value::Uint(value.tag())]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn read_optional_u32(fields: &mut Fields, name: &str) -> Result<Option<u32>> {
    fields.optional_u32(name)
}

fn read_optional_u64(fields: &mut Fields, name: &str) -> Result<Option<u64>> {
    match optional_value(fields.next(name)?, name)? {
        Some(Value::Uint(value)) => Ok(Some(value)),
        Some(_) => Err(LoomError::corrupt(format!("{name} value must be uint"))),
        None => Ok(None),
    }
}

fn read_optional_redaction_state(
    fields: &mut Fields,
    name: &str,
) -> Result<Option<RedactionState>> {
    match optional_value(fields.next(name)?, name)? {
        Some(Value::Uint(value)) => RedactionState::from_tag(value).map(Some),
        Some(_) => Err(LoomError::corrupt(format!("{name} value must be uint"))),
        None => Ok(None),
    }
}

fn optional_value(value: Value, name: &str) -> Result<Option<Value>> {
    let mut fields = Fields::array(value, name)?;
    let tag = fields.uint(name)?;
    let value = match tag {
        0 => None,
        1 => Some(fields.next(name)?),
        other => {
            return Err(LoomError::corrupt(format!(
                "{name} has unknown optional tag {other}"
            )));
        }
    };
    fields.end(name)?;
    Ok(value)
}

fn validate_optional_text(name: &str, value: Option<&str>) -> Result<()> {
    if let Some(value) = value {
        validate_text(name, value)?;
    }
    Ok(())
}

fn validate_text_list(name: &str, values: &[String]) -> Result<()> {
    for value in values {
        validate_text(name, value)?;
    }
    Ok(())
}

fn unique_ids<'a>(name: &str, ids: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for id in ids {
        validate_text(name, id)?;
        if !seen.insert(id) {
            return Err(LoomError::invalid(format!("{name} must be unique")));
        }
    }
    Ok(())
}

fn source_list(value: Value) -> Result<Vec<SourceRecord>> {
    read_list(value, "source records", SourceRecord::from_value)
}

fn meeting_list(value: Value) -> Result<Vec<MeetingRecord>> {
    read_list(value, "meeting records", MeetingRecord::from_value)
}

fn span_list(value: Value) -> Result<Vec<SpanRecord>> {
    read_list(value, "span records", SpanRecord::from_value)
}

fn annotation_list(value: Value) -> Result<Vec<AnnotationRecord>> {
    read_list(value, "annotation records", AnnotationRecord::from_value)
}

fn vocabulary_term_list(value: Value) -> Result<Vec<VocabularyTermRecord>> {
    read_list(
        value,
        "vocabulary term records",
        VocabularyTermRecord::from_value,
    )
}

fn entity_merge_list(value: Value) -> Result<Vec<EntityMergeRecord>> {
    read_list(value, "entity merge records", EntityMergeRecord::from_value)
}

fn promotion_list(value: Value) -> Result<Vec<PromotionRecord>> {
    read_list(value, "promotion records", PromotionRecord::from_value)
}

fn import_run_list(value: Value) -> Result<Vec<ImportRunRecord>> {
    read_list(value, "import run records", ImportRunRecord::from_value)
}

fn redaction_list(value: Value) -> Result<Vec<RedactionRecord>> {
    read_list(value, "redaction records", RedactionRecord::from_value)
}

fn projection_effect_list(value: Value) -> Result<Vec<ProjectionEffect>> {
    read_list(
        value,
        "meeting projection effects",
        ProjectionEffect::from_value,
    )
}

fn projection_output_list(value: Value) -> Result<Vec<ProjectionOutput>> {
    read_list(
        value,
        "meeting projection outputs",
        ProjectionOutput::from_value,
    )
}

fn read_list<T>(value: Value, name: &str, read: fn(Value) -> Result<T>) -> Result<Vec<T>> {
    match value {
        Value::Array(items) => items.into_iter().map(read).collect(),
        _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
    }
}

fn projection_payload(
    snapshot: &MeetingsProfileSnapshot,
    effect: &ProjectionEffect,
) -> Result<Value> {
    let entity = match effect.entity_kind.as_str() {
        "source" | "source_snapshot" => snapshot
            .sources
            .iter()
            .find(|source| source.source_id == effect.entity_id)
            .map(SourceRecord::to_value),
        "meeting" => snapshot
            .meetings
            .iter()
            .find(|meeting| meeting.meeting_id == effect.entity_id)
            .map(MeetingRecord::to_value),
        "span" => snapshot
            .spans
            .iter()
            .find(|span| span.span_id == effect.entity_id)
            .map(SpanRecord::to_value),
        "annotation" | "annotation_accept" => snapshot
            .annotations
            .iter()
            .find(|annotation| annotation.annotation_id == effect.entity_id)
            .map(AnnotationRecord::to_value),
        "import_run" => snapshot
            .import_runs
            .iter()
            .find(|import_run| import_run.import_run_id == effect.entity_id)
            .map(ImportRunRecord::to_value),
        "promotion" => snapshot
            .promotions
            .iter()
            .find(|promotion| promotion.promotion_id == effect.entity_id)
            .map(PromotionRecord::to_value),
        "redaction" => snapshot
            .redactions
            .iter()
            .find(|redaction| redaction.redaction_id == effect.entity_id)
            .map(RedactionRecord::to_value),
        _ => None,
    }
    .ok_or_else(|| {
        LoomError::invalid(format!(
            "projection entity {}:{} is missing",
            effect.entity_kind, effect.entity_id
        ))
    })?;

    Ok(Value::Array(vec![
        Value::Text(PROJECTION_OUTPUT_PAYLOAD_SCHEMA.to_string()),
        Value::Array(vec![
            Value::Uint(effect.projection.tag()),
            Value::Uint(effect.action.tag()),
            Value::Text(effect.entity_kind.clone()),
            Value::Text(effect.entity_id.clone()),
            entity,
        ]),
    ]))
}

fn validate_projection_payload(
    payload: &Value,
    projection: ProjectionKind,
    action: ProjectionAction,
    entity_kind: &str,
    entity_id: &str,
) -> Result<()> {
    let mut outer = Fields::array(payload.clone(), "meeting projection payload")?;
    outer.expect_text(PROJECTION_OUTPUT_PAYLOAD_SCHEMA)?;
    let mut fields = Fields::array(
        outer.next("meeting projection payload fields")?,
        "meeting projection payload",
    )?;
    outer.end("meeting projection payload")?;
    if ProjectionKind::from_tag(fields.uint("projection")?)? != projection {
        return Err(LoomError::invalid(
            "projection output payload projection mismatch",
        ));
    }
    if ProjectionAction::from_tag(fields.uint("action")?)? != action {
        return Err(LoomError::invalid(
            "projection output payload action mismatch",
        ));
    }
    if fields.text("entity_kind")? != entity_kind {
        return Err(LoomError::invalid(
            "projection output payload entity_kind mismatch",
        ));
    }
    if fields.text("entity_id")? != entity_id {
        return Err(LoomError::invalid(
            "projection output payload entity_id mismatch",
        ));
    }
    fields.next("entity")?;
    fields.end("meeting projection payload")
}

struct ProjectionSeed<'a> {
    workspace_id: &'a str,
    projection: ProjectionKind,
    action: ProjectionAction,
    entity_kind: &'a str,
    entity_id: &'a str,
    recorded_at_ms: u64,
    source_ids: &'a [&'a str],
    output_ref: Option<String>,
    payload_digest: Option<Digest>,
    redaction_state: Option<RedactionState>,
}

fn push_effect(effects: &mut Vec<ProjectionEffect>, seed: ProjectionSeed<'_>) -> Result<()> {
    let mut effect = ProjectionEffect::new(ProjectionEffectInput {
        effect_id: &format!(
            "{}:{}:{}:{}",
            seed.workspace_id,
            projection_label(seed.projection),
            seed.entity_kind,
            seed.entity_id
        ),
        projection: seed.projection,
        action: seed.action,
        entity_kind: seed.entity_kind,
        entity_id: seed.entity_id,
        recorded_at_ms: seed.recorded_at_ms,
    })?;
    effect.source_ids = seed
        .source_ids
        .iter()
        .map(|source_id| (*source_id).to_string())
        .collect();
    effect.output_ref = seed.output_ref;
    effect.payload_digest = seed.payload_digest;
    effect.redaction_state = seed.redaction_state;
    effect.validate()?;
    effects.push(effect);
    Ok(())
}

fn projection_label(projection: ProjectionKind) -> &'static str {
    match projection {
        ProjectionKind::Document => "document",
        ProjectionKind::Files => "files",
        ProjectionKind::Graph => "graph",
        ProjectionKind::Vector => "vector",
        ProjectionKind::Search => "search",
        ProjectionKind::SqlDataframe => "sql-dataframe",
        ProjectionKind::Ledger => "ledger",
    }
}

fn meeting_status_redaction_state(status: MeetingStatus) -> RedactionState {
    match status {
        MeetingStatus::Active | MeetingStatus::DeletedAtSource => RedactionState::Live,
        MeetingStatus::Redacted => RedactionState::Redacted,
        MeetingStatus::RetainedMetadataOnly => RedactionState::RetainedMetadataOnly,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_types::Algo;

    fn digest(label: &[u8]) -> Digest {
        Digest::hash(Algo::Blake3, label)
    }

    fn sample_snapshot() -> MeetingsProfileSnapshot {
        let mut source = SourceRecord::new(SourceRecordInput {
            source_id: "src-1",
            source_system: "granola-api",
            external_id: "not_1",
            source_digest: digest(b"source"),
            observed_at_ms: 100,
            access_scope: "personal-notes",
            coverage: Coverage::Partial,
        })
        .unwrap();
        source.sidecar_digest = Some(digest(b"sidecar"));
        let mut meeting = MeetingRecord::new(MeetingRecordInput {
            meeting_id: "meet-1",
            title: "Architecture review",
            current_source_digest: digest(b"source"),
            created_at_ms: 100,
            updated_at_ms: 120,
        })
        .unwrap();
        meeting.source_refs = vec!["src-1".to_string()];
        let mut span = SpanRecord::new(
            "span-1",
            "meet-1",
            "src-1",
            SpanKind::TranscriptEntry,
            "granola:not_1/transcript/0",
        )
        .unwrap();
        span.text_digest = Some(digest(b"text"));
        let mut annotation = AnnotationRecord::new(
            "ann-1",
            "meet-1",
            vec!["span-1".to_string()],
            "Decision",
            "Use normalized import snapshots",
            130,
        )
        .unwrap();
        annotation.status = AnnotationStatus::Accepted;
        annotation.accepted_by = Some("principal-1".to_string());
        annotation.accepted_at_ms = Some(140);
        let mut import_run = ImportRunRecord::new(
            "run-1",
            InputProfile::GranolaApi,
            "personal-notes",
            Coverage::Partial,
            90,
        )
        .unwrap();
        import_run.observed_ids = vec!["not_1".to_string()];
        import_run.coverage_gaps = vec!["rate-limit".to_string()];
        let redaction = RedactionRecord::new(
            "redact-1",
            "span-1",
            "span",
            RedactionState::RetainedMetadataOnly,
            "policy-1",
            150,
        )
        .unwrap();
        let mut snapshot = MeetingsProfileSnapshot::new(
            "organization",
            MeetingsProfileSnapshotParts {
                sources: vec![source],
                meetings: vec![meeting],
                spans: vec![span],
                annotations: vec![annotation],
                vocabulary_terms: Vec::new(),
                entity_merges: Vec::new(),
                promotions: Vec::new(),
                import_runs: vec![import_run],
                redactions: vec![redaction],
            },
        )
        .unwrap();
        snapshot
            .add_promotion(
                PromotionRecord::new(PromotionInput {
                    promotion_id: "promote-1",
                    operation_kind: "decision.promoted",
                    source_annotation_id: "ann-1",
                    target_profile: "decision-log",
                    target_entity_ref: "decision:dec-1",
                    promoted_by: "principal-1",
                    promoted_at_ms: 160,
                })
                .unwrap(),
            )
            .unwrap();
        snapshot
    }

    #[test]
    fn meetings_profile_snapshot_round_trips_canonical_bytes() {
        let snapshot = sample_snapshot();
        let encoded = snapshot.encode().unwrap();
        let decoded = MeetingsProfileSnapshot::decode(&encoded).unwrap();
        assert_eq!(decoded, snapshot);
        assert_eq!(decoded.encode().unwrap(), encoded);
    }

    #[test]
    fn projection_effect_set_round_trips_canonical_bytes() {
        let snapshot = sample_snapshot();
        let effects = ProjectionEffectSet::from_snapshot(&snapshot).unwrap();
        let encoded = effects.encode().unwrap();
        let decoded = ProjectionEffectSet::decode(&encoded).unwrap();
        assert_eq!(decoded, effects);
        assert_eq!(decoded.encode().unwrap(), encoded);
    }

    #[test]
    fn projection_effects_cover_required_meetings_projections() {
        let snapshot = sample_snapshot();
        let effects = ProjectionEffectSet::from_snapshot(&snapshot).unwrap();
        for projection in [
            ProjectionKind::Document,
            ProjectionKind::Files,
            ProjectionKind::Graph,
            ProjectionKind::Vector,
            ProjectionKind::Search,
            ProjectionKind::SqlDataframe,
            ProjectionKind::Ledger,
        ] {
            assert!(!effects.effects_for(projection).is_empty());
        }
    }

    #[test]
    fn promotion_requires_observed_or_accepted_annotation() {
        let mut snapshot = sample_snapshot();
        let mut rejected = AnnotationRecord::new(
            "ann-rejected",
            "meet-1",
            vec!["span-1".to_string()],
            "Task",
            "Rejected task",
            170,
        )
        .unwrap();
        rejected.status = AnnotationStatus::Rejected;
        snapshot.annotations.push(rejected);
        let err = snapshot
            .add_promotion(
                PromotionRecord::new(PromotionInput {
                    promotion_id: "promote-rejected",
                    operation_kind: "task.promoted",
                    source_annotation_id: "ann-rejected",
                    target_profile: "tickets",
                    target_entity_ref: "ticket:LOOM-2",
                    promoted_by: "principal-1",
                    promoted_at_ms: 180,
                })
                .unwrap(),
            )
            .unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
    }

    #[test]
    fn projection_outputs_materialize_required_meetings_projections() {
        let snapshot = sample_snapshot();
        let outputs = ProjectionOutputSet::from_snapshot(&snapshot).unwrap();
        let encoded = outputs.encode().unwrap();
        let decoded = ProjectionOutputSet::decode(&encoded).unwrap();
        assert_eq!(decoded, outputs);
        assert_eq!(decoded.encode().unwrap(), encoded);
        for projection in [
            ProjectionKind::Document,
            ProjectionKind::Files,
            ProjectionKind::Graph,
            ProjectionKind::Vector,
            ProjectionKind::Search,
            ProjectionKind::SqlDataframe,
            ProjectionKind::Ledger,
        ] {
            assert!(!outputs.outputs_for(projection).is_empty());
        }
        let meeting = outputs
            .outputs_for(ProjectionKind::Document)
            .into_iter()
            .find(|output| output.entity_kind == "meeting")
            .unwrap();
        assert_eq!(meeting.output_ref, "document:meeting/meet-1");
        match &meeting.payload {
            Value::Array(items) => assert_eq!(
                items.first(),
                Some(&Value::Text(PROJECTION_OUTPUT_PAYLOAD_SCHEMA.to_string()))
            ),
            _ => panic!("projection payload must be array"),
        }
    }

    #[test]
    fn redaction_projection_effects_invalidate_derived_text_indexes() {
        let mut snapshot = sample_snapshot();
        snapshot.spans[0].redaction_state = RedactionState::Redacted;
        let effects = ProjectionEffectSet::from_snapshot(&snapshot).unwrap();
        for projection in [ProjectionKind::Search, ProjectionKind::Vector] {
            let effect = effects
                .effects_for(projection)
                .into_iter()
                .find(|effect| effect.entity_id == "span-1")
                .unwrap();
            assert_eq!(effect.action, ProjectionAction::Invalidate);
            assert_eq!(effect.redaction_state, Some(RedactionState::Redacted));
        }
        let ledger_effect = effects
            .effects_for(ProjectionKind::Ledger)
            .into_iter()
            .find(|effect| effect.entity_id == "redact-1")
            .unwrap();
        assert_eq!(ledger_effect.action, ProjectionAction::Append);
    }

    #[test]
    fn projection_outputs_preserve_redaction_actions() {
        let mut snapshot = sample_snapshot();
        snapshot.spans[0].redaction_state = RedactionState::Redacted;
        let outputs = ProjectionOutputSet::from_snapshot(&snapshot).unwrap();
        for projection in [ProjectionKind::Search, ProjectionKind::Vector] {
            let output = outputs
                .outputs_for(projection)
                .into_iter()
                .find(|output| output.entity_id == "span-1")
                .unwrap();
            assert_eq!(output.action, ProjectionAction::Invalidate);
            assert_eq!(output.redaction_state, Some(RedactionState::Redacted));
        }
    }

    #[test]
    fn extraction_review_projection_buckets_annotation_statuses() {
        let mut snapshot = sample_snapshot();
        let suggested = AnnotationRecord::new(
            "ann-2",
            "meet-1",
            vec!["span-1".to_string()],
            "Risk",
            "Migration risk",
            150,
        )
        .unwrap();
        let mut rejected = AnnotationRecord::new(
            "ann-3",
            "meet-1",
            vec!["span-1".to_string()],
            "Task",
            "Rewrite history",
            160,
        )
        .unwrap();
        rejected.status = AnnotationStatus::Rejected;
        snapshot.annotations.push(suggested);
        snapshot.annotations.push(rejected);
        let mut term = VocabularyTermRecord::new(VocabularyTermInput {
            term_id: "term-1",
            kind: "DomainTerm",
            label: "LCB",
            evidence_annotation_ids: vec!["ann-2".to_string()],
            created_at_ms: 170,
        })
        .unwrap();
        term.aliases = vec!["loom control block".to_string()];
        let merge = EntityMergeRecord::new(EntityMergeInput {
            merge_id: "merge-1",
            canonical_entity_id: "person:ava",
            merged_entity_ids: vec!["person:a.vazquez".to_string()],
            evidence_annotation_ids: vec!["ann-1".to_string()],
            decided_by: "principal-1",
            decided_at_ms: 180,
        })
        .unwrap();
        let review = ExtractionReviewProjection::new(
            "organization",
            &snapshot.annotations,
            vec![term],
            vec![merge],
        )
        .unwrap();
        assert_eq!(review.suggested_annotation_ids, vec!["ann-2"]);
        assert_eq!(review.accepted_annotation_ids, vec!["ann-1"]);
        assert_eq!(review.rejected_annotation_ids, vec!["ann-3"]);
        let encoded = review.encode().unwrap();
        let decoded = ExtractionReviewProjection::decode(&encoded).unwrap();
        assert_eq!(decoded, review);
        assert_eq!(decoded.encode().unwrap(), encoded);
    }

    #[test]
    fn extraction_review_workflow_accepts_and_rejects_records() {
        let mut accepted = AnnotationRecord::new(
            "ann-accept",
            "meet-1",
            vec!["span-1".to_string()],
            "Decision",
            "Use the durable importer",
            10,
        )
        .unwrap();
        accepted.accept("principal-1", 20).unwrap();
        assert_eq!(accepted.status, AnnotationStatus::Accepted);
        assert_eq!(accepted.accepted_by.as_deref(), Some("principal-1"));
        assert_eq!(accepted.accepted_at_ms, Some(20));
        assert_eq!(
            accepted.reject().unwrap_err().code,
            loom_types::Code::InvalidArgument
        );

        let mut rejected = AnnotationRecord::new(
            "ann-reject",
            "meet-1",
            vec!["span-1".to_string()],
            "Risk",
            "Unsupported claim",
            10,
        )
        .unwrap();
        rejected.reject().unwrap();
        assert_eq!(rejected.status, AnnotationStatus::Rejected);
        assert_eq!(rejected.accepted_by, None);
        assert_eq!(rejected.accepted_at_ms, None);

        let mut term = VocabularyTermRecord::new(VocabularyTermInput {
            term_id: "term-1",
            kind: "DomainTerm",
            label: "LCB",
            evidence_annotation_ids: vec!["ann-accept".to_string()],
            created_at_ms: 10,
        })
        .unwrap();
        term.accept("principal-1", 30).unwrap();
        assert_eq!(term.status, VocabularyTermStatus::Accepted);
        assert_eq!(term.reviewed_by.as_deref(), Some("principal-1"));
        assert_eq!(term.reviewed_at_ms, Some(30));
        assert_eq!(
            term.reject("principal-1", 40).unwrap_err().code,
            loom_types::Code::InvalidArgument
        );
    }

    #[test]
    fn vocabulary_and_entity_merge_require_annotation_evidence() {
        let term = VocabularyTermRecord::new(VocabularyTermInput {
            term_id: "term-1",
            kind: "DomainTerm",
            label: "LCB",
            evidence_annotation_ids: Vec::new(),
            created_at_ms: 1,
        })
        .unwrap_err();
        assert_eq!(term.code, loom_types::Code::InvalidArgument);
        let merge = EntityMergeRecord::new(EntityMergeInput {
            merge_id: "merge-1",
            canonical_entity_id: "person:ava",
            merged_entity_ids: vec!["person:a.vazquez".to_string()],
            evidence_annotation_ids: Vec::new(),
            decided_by: "principal-1",
            decided_at_ms: 1,
        })
        .unwrap_err();
        assert_eq!(merge.code, loom_types::Code::InvalidArgument);
    }

    #[test]
    fn annotation_requires_source_evidence() {
        let err = AnnotationRecord::new("ann-1", "meet-1", Vec::new(), "Decision", "choice", 1)
            .unwrap_err();
        assert_eq!(err.code, loom_types::Code::InvalidArgument);
    }

    #[test]
    fn meeting_rejects_inverted_time_range() {
        let mut meeting = MeetingRecord::new(MeetingRecordInput {
            meeting_id: "meet-1",
            title: "Architecture review",
            current_source_digest: digest(b"source"),
            created_at_ms: 100,
            updated_at_ms: 120,
        })
        .unwrap();
        meeting.starts_at_ms = Some(200);
        meeting.ends_at_ms = Some(100);
        assert_eq!(
            meeting.validate().unwrap_err().code,
            loom_types::Code::InvalidArgument
        );
    }

    #[test]
    fn profile_snapshot_rejects_duplicate_ids() {
        let mut snapshot = sample_snapshot();
        snapshot.sources.push(snapshot.sources[0].clone());
        assert_eq!(
            snapshot.validate().unwrap_err().code,
            loom_types::Code::InvalidArgument
        );
    }
}
