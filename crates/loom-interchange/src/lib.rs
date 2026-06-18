use std::collections::BTreeSet;

use loom_codec::Value;
use loom_types::{Digest, LoomError, Result};

pub const IMPORT_REPORT_SCHEMA: &str = "loom.interchange.import-report.v1";
pub const EXPORT_REPORT_SCHEMA: &str = "loom.interchange.export-report.v1";
pub const FIDELITY_ISSUE_SCHEMA: &str = "loom.interchange.fidelity-issue.v1";
pub const IMPORT_CHECKPOINT_SCHEMA: &str = "loom.interchange.import-checkpoint.v1";
pub const IMPORT_BATCH_SCHEMA: &str = "loom.interchange.import-batch.v1";
pub const IMPORT_BATCH_ITEM_SCHEMA: &str = "loom.interchange.import-batch-item.v1";
pub const IMPORT_EXECUTION_BATCH_SCHEMA: &str = "loom.interchange.import-execution-batch.v1";
pub const IMPORT_EXECUTION_PAYLOAD_SCHEMA: &str = "loom.interchange.import-execution-payload.v1";
pub const ARCHIVE_MANIFEST_SCHEMA: &str = "loom.interchange.archive-manifest.v1";
pub const ARCHIVE_ENTRY_SCHEMA: &str = "loom.interchange.archive-entry.v1";
pub const PROFILE_IMPORT_PLAN_SCHEMA: &str = "loom.interchange.profile-import-plan.v1";
pub const PROFILE_IMPORT_ACTION_SCHEMA: &str = "loom.interchange.profile-import-action.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FidelitySeverity {
    Info,
    Warning,
    Error,
}

impl FidelitySeverity {
    const fn tag(self) -> u64 {
        match self {
            Self::Info => 0,
            Self::Warning => 1,
            Self::Error => 2,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Info),
            1 => Ok(Self::Warning),
            2 => Ok(Self::Error),
            other => Err(LoomError::corrupt(format!(
                "unknown fidelity severity tag {other}"
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
            other => Err(LoomError::corrupt(format!("unknown coverage tag {other}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    Tar,
    Gzip,
    TarZstd,
    TarGzip,
}

impl ArchiveKind {
    const fn tag(self) -> u64 {
        match self {
            Self::Zip => 0,
            Self::Tar => 1,
            Self::Gzip => 2,
            Self::TarZstd => 3,
            Self::TarGzip => 4,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Zip),
            1 => Ok(Self::Tar),
            2 => Ok(Self::Gzip),
            3 => Ok(Self::TarZstd),
            4 => Ok(Self::TarGzip),
            other => Err(LoomError::corrupt(format!(
                "unknown archive kind tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveEntryKind {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceSystem {
    Redmine,
    Jira,
    ConfluenceStorage,
    ConfluenceAdf,
    Markdown,
    Notion,
    Asana,
    Slack,
    Drive,
    GranolaApi,
    GranolaApp,
    GranolaMcp,
    Csv,
}

impl SourceSystem {
    const fn tag(self) -> u64 {
        match self {
            Self::Redmine => 0,
            Self::Jira => 1,
            Self::ConfluenceStorage => 2,
            Self::ConfluenceAdf => 3,
            Self::Markdown => 4,
            Self::Notion => 5,
            Self::Asana => 6,
            Self::Slack => 7,
            Self::Drive => 8,
            Self::GranolaApi => 9,
            Self::GranolaApp => 10,
            Self::GranolaMcp => 11,
            Self::Csv => 12,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Redmine),
            1 => Ok(Self::Jira),
            2 => Ok(Self::ConfluenceStorage),
            3 => Ok(Self::ConfluenceAdf),
            4 => Ok(Self::Markdown),
            5 => Ok(Self::Notion),
            6 => Ok(Self::Asana),
            7 => Ok(Self::Slack),
            8 => Ok(Self::Drive),
            9 => Ok(Self::GranolaApi),
            10 => Ok(Self::GranolaApp),
            11 => Ok(Self::GranolaMcp),
            12 => Ok(Self::Csv),
            other => Err(LoomError::corrupt(format!(
                "unknown source system tag {other}"
            ))),
        }
    }

    pub const fn profile_name(self) -> &'static str {
        match self {
            Self::Redmine => "redmine",
            Self::Jira => "jira",
            Self::ConfluenceStorage => "confluence-storage",
            Self::ConfluenceAdf => "confluence-adf",
            Self::Markdown => "markdown",
            Self::Notion => "notion",
            Self::Asana => "asana",
            Self::Slack => "slack",
            Self::Drive => "drive",
            Self::GranolaApi => "granola-api",
            Self::GranolaApp => "granola-app",
            Self::GranolaMcp => "granola-mcp",
            Self::Csv => "csv",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetProfile {
    Tickets,
    Pages,
    Chat,
    Drive,
    Meetings,
}

impl TargetProfile {
    const fn tag(self) -> u64 {
        match self {
            Self::Tickets => 0,
            Self::Pages => 1,
            Self::Chat => 2,
            Self::Drive => 3,
            Self::Meetings => 4,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Tickets),
            1 => Ok(Self::Pages),
            2 => Ok(Self::Chat),
            3 => Ok(Self::Drive),
            4 => Ok(Self::Meetings),
            other => Err(LoomError::corrupt(format!(
                "unknown target profile tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileImportActionKind {
    TicketProject,
    Ticket,
    TicketWorkflow,
    TicketComment,
    TicketAttachment,
    PageSpace,
    Page,
    PageBodyReplace,
    PageAnnotation,
    PageAttachment,
    SourceSidecar,
}

impl ProfileImportActionKind {
    const fn tag(self) -> u64 {
        match self {
            Self::TicketProject => 0,
            Self::Ticket => 1,
            Self::TicketWorkflow => 2,
            Self::TicketComment => 3,
            Self::TicketAttachment => 4,
            Self::PageSpace => 5,
            Self::Page => 6,
            Self::PageBodyReplace => 7,
            Self::PageAnnotation => 8,
            Self::PageAttachment => 9,
            Self::SourceSidecar => 10,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::TicketProject),
            1 => Ok(Self::Ticket),
            2 => Ok(Self::TicketWorkflow),
            3 => Ok(Self::TicketComment),
            4 => Ok(Self::TicketAttachment),
            5 => Ok(Self::PageSpace),
            6 => Ok(Self::Page),
            7 => Ok(Self::PageBodyReplace),
            8 => Ok(Self::PageAnnotation),
            9 => Ok(Self::PageAttachment),
            10 => Ok(Self::SourceSidecar),
            other => Err(LoomError::corrupt(format!(
                "unknown profile import action kind tag {other}"
            ))),
        }
    }
}

impl ArchiveEntryKind {
    const fn tag(self) -> u64 {
        match self {
            Self::File => 0,
            Self::Directory => 1,
            Self::Symlink => 2,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::File),
            1 => Ok(Self::Directory),
            2 => Ok(Self::Symlink),
            other => Err(LoomError::corrupt(format!(
                "unknown archive entry kind tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FidelityIssue {
    pub severity: FidelitySeverity,
    pub source_entity_id: String,
    pub field: String,
    pub reason: String,
    pub source_digest: Option<Digest>,
}

impl FidelityIssue {
    pub fn new(
        severity: FidelitySeverity,
        source_entity_id: impl Into<String>,
        field: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<Self> {
        let issue = Self {
            severity,
            source_entity_id: source_entity_id.into(),
            field: field.into(),
            reason: reason.into(),
            source_digest: None,
        };
        issue.validate()?;
        Ok(issue)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(FIDELITY_ISSUE_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Uint(self.severity.tag()),
                Value::Text(self.source_entity_id.clone()),
                Value::Text(self.field.clone()),
                Value::Text(self.reason.clone()),
                optional_digest(self.source_digest),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "fidelity ticket")?;
        outer.expect_text(FIDELITY_ISSUE_SCHEMA)?;
        let mut fields = Fields::array(outer.next("fidelity ticket fields")?, "fidelity ticket")?;
        outer.end("fidelity ticket")?;
        let ticket = Self {
            severity: FidelitySeverity::from_tag(fields.uint("severity")?)?,
            source_entity_id: fields.text("source_entity_id")?,
            field: fields.text("field")?,
            reason: fields.text("reason")?,
            source_digest: fields.optional_digest("source_digest")?,
        };
        fields.end("fidelity ticket")?;
        ticket.validate()?;
        Ok(ticket)
    }

    fn validate(&self) -> Result<()> {
        validate_text("source_entity_id", &self.source_entity_id)?;
        validate_text("field", &self.field)?;
        validate_text("reason", &self.reason)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub profile: String,
    pub source_scope: String,
    pub commit: Option<Digest>,
    pub objects_added: u64,
    pub bytes_in: u64,
    pub bytes_stored: u64,
    pub rows_imported: u64,
    pub skipped: u64,
    pub operations_planned: u64,
    pub operations_applied: u64,
    pub dry_run: bool,
    pub warnings: Vec<String>,
    pub fidelity_issues: Vec<FidelityIssue>,
}

#[derive(Debug, Clone)]
pub struct ImportReportInput<'a> {
    pub profile: &'a str,
    pub source_scope: &'a str,
    pub commit: Option<Digest>,
    pub objects_added: u64,
    pub bytes_in: u64,
    pub bytes_stored: u64,
    pub rows_imported: u64,
    pub skipped: u64,
    pub operations_planned: u64,
    pub operations_applied: u64,
    pub dry_run: bool,
}

impl ImportReport {
    pub fn new(input: ImportReportInput<'_>) -> Result<Self> {
        let report = Self {
            profile: input.profile.to_string(),
            source_scope: input.source_scope.to_string(),
            commit: input.commit,
            objects_added: input.objects_added,
            bytes_in: input.bytes_in,
            bytes_stored: input.bytes_stored,
            rows_imported: input.rows_imported,
            skipped: input.skipped,
            operations_planned: input.operations_planned,
            operations_applied: input.operations_applied,
            dry_run: input.dry_run,
            warnings: Vec::new(),
            fidelity_issues: Vec::new(),
        };
        report.validate()?;
        Ok(report)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(IMPORT_REPORT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.profile.clone()),
                Value::Text(self.source_scope.clone()),
                optional_digest(self.commit),
                Value::Uint(self.objects_added),
                Value::Uint(self.bytes_in),
                Value::Uint(self.bytes_stored),
                Value::Uint(self.rows_imported),
                Value::Uint(self.skipped),
                Value::Uint(self.operations_planned),
                Value::Uint(self.operations_applied),
                bool_value(self.dry_run),
                string_array(&self.warnings),
                Value::Array(
                    self.fidelity_issues
                        .iter()
                        .map(FidelityIssue::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "import report")?;
        outer.expect_text(IMPORT_REPORT_SCHEMA)?;
        let mut fields = Fields::array(outer.next("import report fields")?, "import report")?;
        outer.end("import report")?;
        let report = Self {
            profile: fields.text("profile")?,
            source_scope: fields.text("source_scope")?,
            commit: fields.optional_digest("commit")?,
            objects_added: fields.uint("objects_added")?,
            bytes_in: fields.uint("bytes_in")?,
            bytes_stored: fields.uint("bytes_stored")?,
            rows_imported: fields.uint("rows_imported")?,
            skipped: fields.uint("skipped")?,
            operations_planned: fields.uint("operations_planned")?,
            operations_applied: fields.uint("operations_applied")?,
            dry_run: fields.bool("dry_run")?,
            warnings: fields.string_array("warnings")?,
            fidelity_issues: fields.fidelity_issue_array("fidelity_issues")?,
        };
        fields.end("import report")?;
        report.validate()?;
        Ok(report)
    }

    fn validate(&self) -> Result<()> {
        validate_text("profile", &self.profile)?;
        validate_text("source_scope", &self.source_scope)?;
        validate_texts("warning", &self.warnings)?;
        for ticket in &self.fidelity_issues {
            ticket.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    pub profile: String,
    pub destination_scope: String,
    pub files_written: u64,
    pub rows_written: u64,
    pub bytes_out: u64,
    pub dry_run: bool,
    pub warnings: Vec<String>,
    pub fidelity_issues: Vec<FidelityIssue>,
}

impl ExportReport {
    pub fn new(profile: impl Into<String>, destination_scope: impl Into<String>) -> Result<Self> {
        let report = Self {
            profile: profile.into(),
            destination_scope: destination_scope.into(),
            files_written: 0,
            rows_written: 0,
            bytes_out: 0,
            dry_run: false,
            warnings: Vec::new(),
            fidelity_issues: Vec::new(),
        };
        report.validate()?;
        Ok(report)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(EXPORT_REPORT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.profile.clone()),
                Value::Text(self.destination_scope.clone()),
                Value::Uint(self.files_written),
                Value::Uint(self.rows_written),
                Value::Uint(self.bytes_out),
                bool_value(self.dry_run),
                string_array(&self.warnings),
                Value::Array(
                    self.fidelity_issues
                        .iter()
                        .map(FidelityIssue::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "export report")?;
        outer.expect_text(EXPORT_REPORT_SCHEMA)?;
        let mut fields = Fields::array(outer.next("export report fields")?, "export report")?;
        outer.end("export report")?;
        let report = Self {
            profile: fields.text("profile")?,
            destination_scope: fields.text("destination_scope")?,
            files_written: fields.uint("files_written")?,
            rows_written: fields.uint("rows_written")?,
            bytes_out: fields.uint("bytes_out")?,
            dry_run: fields.bool("dry_run")?,
            warnings: fields.string_array("warnings")?,
            fidelity_issues: fields.fidelity_issue_array("fidelity_issues")?,
        };
        fields.end("export report")?;
        report.validate()?;
        Ok(report)
    }

    fn validate(&self) -> Result<()> {
        validate_text("profile", &self.profile)?;
        validate_text("destination_scope", &self.destination_scope)?;
        validate_texts("warning", &self.warnings)?;
        for ticket in &self.fidelity_issues {
            ticket.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBatchItem {
    pub source_entity_id: String,
    pub source_digest: Digest,
    pub source_updated_at_ms: Option<u64>,
    pub sidecar_digest: Option<Digest>,
}

impl ImportBatchItem {
    pub fn new(source_entity_id: impl Into<String>, source_digest: Digest) -> Result<Self> {
        let item = Self {
            source_entity_id: source_entity_id.into(),
            source_digest,
            source_updated_at_ms: None,
            sidecar_digest: None,
        };
        item.validate()?;
        Ok(item)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(IMPORT_BATCH_ITEM_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.source_entity_id.clone()),
                Value::Text(self.source_digest.to_string()),
                optional_u64(self.source_updated_at_ms),
                optional_digest(self.sidecar_digest),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "import batch item")?;
        outer.expect_text(IMPORT_BATCH_ITEM_SCHEMA)?;
        let mut fields =
            Fields::array(outer.next("import batch item fields")?, "import batch item")?;
        outer.end("import batch item")?;
        let item = Self {
            source_entity_id: fields.text("source_entity_id")?,
            source_digest: fields.digest("source_digest")?,
            source_updated_at_ms: fields.optional_u64("source_updated_at_ms")?,
            sidecar_digest: fields.optional_digest("sidecar_digest")?,
        };
        fields.end("import batch item")?;
        item.validate()?;
        Ok(item)
    }

    fn validate(&self) -> Result<()> {
        validate_text("source_entity_id", &self.source_entity_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBatch {
    pub profile: String,
    pub source_system: String,
    pub source_scope: String,
    pub observed_at_ms: u64,
    pub coverage: Coverage,
    pub source_cursor: Option<String>,
    pub source_sidecar_digest: Option<Digest>,
    pub items: Vec<ImportBatchItem>,
}

impl ImportBatch {
    pub fn new(
        profile: impl Into<String>,
        source_system: impl Into<String>,
        source_scope: impl Into<String>,
        observed_at_ms: u64,
        coverage: Coverage,
    ) -> Result<Self> {
        let batch = Self {
            profile: profile.into(),
            source_system: source_system.into(),
            source_scope: source_scope.into(),
            observed_at_ms,
            coverage,
            source_cursor: None,
            source_sidecar_digest: None,
            items: Vec::new(),
        };
        batch.validate()?;
        Ok(batch)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(IMPORT_BATCH_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.profile.clone()),
                Value::Text(self.source_system.clone()),
                Value::Text(self.source_scope.clone()),
                Value::Uint(self.observed_at_ms),
                Value::Uint(self.coverage.tag()),
                optional_text_value(self.source_cursor.as_deref()),
                optional_digest(self.source_sidecar_digest),
                Value::Array(self.items.iter().map(ImportBatchItem::to_value).collect()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "import batch")?;
        outer.expect_text(IMPORT_BATCH_SCHEMA)?;
        let mut fields = Fields::array(outer.next("import batch fields")?, "import batch")?;
        outer.end("import batch")?;
        let batch = Self {
            profile: fields.text("profile")?,
            source_system: fields.text("source_system")?,
            source_scope: fields.text("source_scope")?,
            observed_at_ms: fields.uint("observed_at_ms")?,
            coverage: Coverage::from_tag(fields.uint("coverage")?)?,
            source_cursor: fields.optional_text("source_cursor")?,
            source_sidecar_digest: fields.optional_digest("source_sidecar_digest")?,
            items: fields.import_batch_item_array("items")?,
        };
        fields.end("import batch")?;
        batch.validate()?;
        Ok(batch)
    }

    fn validate(&self) -> Result<()> {
        validate_text("profile", &self.profile)?;
        validate_text("source_system", &self.source_system)?;
        validate_text("source_scope", &self.source_scope)?;
        if let Some(source_cursor) = &self.source_cursor {
            validate_text("source_cursor", source_cursor)?;
        }
        let mut seen = BTreeSet::new();
        for item in &self.items {
            item.validate()?;
            if !seen.insert(item.source_entity_id.as_str()) {
                return Err(LoomError::invalid(format!(
                    "duplicate source entity id {}",
                    item.source_entity_id
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportExecutionPayload {
    pub payload_id: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
    pub source_digest: Digest,
    pub source_updated_at_ms: Option<u64>,
}

impl ImportExecutionPayload {
    pub fn new(
        payload_id: impl Into<String>,
        media_type: impl Into<String>,
        bytes: Vec<u8>,
        algo: loom_types::Algo,
    ) -> Result<Self> {
        let source_digest = Digest::hash(algo, &bytes);
        let payload = Self {
            payload_id: payload_id.into(),
            media_type: media_type.into(),
            bytes,
            source_digest,
            source_updated_at_ms: None,
        };
        payload.validate()?;
        Ok(payload)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(IMPORT_EXECUTION_PAYLOAD_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.payload_id.clone()),
                Value::Text(self.media_type.clone()),
                Value::Bytes(self.bytes.clone()),
                Value::Text(self.source_digest.to_string()),
                optional_u64(self.source_updated_at_ms),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "import execution payload")?;
        outer.expect_text(IMPORT_EXECUTION_PAYLOAD_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("import execution payload fields")?,
            "import execution payload",
        )?;
        outer.end("import execution payload")?;
        let payload = Self {
            payload_id: fields.text("payload_id")?,
            media_type: fields.text("media_type")?,
            bytes: fields.bytes("bytes")?,
            source_digest: fields.digest("source_digest")?,
            source_updated_at_ms: fields.optional_u64("source_updated_at_ms")?,
        };
        fields.end("import execution payload")?;
        payload.validate()?;
        Ok(payload)
    }

    fn validate(&self) -> Result<()> {
        validate_text("payload_id", &self.payload_id)?;
        validate_text("media_type", &self.media_type)?;
        if Digest::hash(self.source_digest.algo(), &self.bytes) != self.source_digest {
            return Err(LoomError::new(
                loom_types::Code::IntegrityFailure,
                "import execution payload digest mismatch",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportExecutionBatch {
    pub profile: String,
    pub source_system: String,
    pub source_scope: String,
    pub observed_at_ms: u64,
    pub coverage: Coverage,
    pub default_space: Option<String>,
    pub payloads: Vec<ImportExecutionPayload>,
}

impl ImportExecutionBatch {
    pub fn new(
        profile: impl Into<String>,
        source_system: impl Into<String>,
        source_scope: impl Into<String>,
        observed_at_ms: u64,
        coverage: Coverage,
    ) -> Result<Self> {
        let batch = Self {
            profile: profile.into(),
            source_system: source_system.into(),
            source_scope: source_scope.into(),
            observed_at_ms,
            coverage,
            default_space: None,
            payloads: Vec::new(),
        };
        batch.validate_header()?;
        Ok(batch)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(IMPORT_EXECUTION_BATCH_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.profile.clone()),
                Value::Text(self.source_system.clone()),
                Value::Text(self.source_scope.clone()),
                Value::Uint(self.observed_at_ms),
                Value::Uint(self.coverage.tag()),
                optional_text_value(self.default_space.as_deref()),
                Value::Array(
                    self.payloads
                        .iter()
                        .map(ImportExecutionPayload::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "import execution batch")?;
        outer.expect_text(IMPORT_EXECUTION_BATCH_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("import execution batch fields")?,
            "import execution batch",
        )?;
        outer.end("import execution batch")?;
        let batch = Self {
            profile: fields.text("profile")?,
            source_system: fields.text("source_system")?,
            source_scope: fields.text("source_scope")?,
            observed_at_ms: fields.uint("observed_at_ms")?,
            coverage: Coverage::from_tag(fields.uint("coverage")?)?,
            default_space: fields.optional_text("default_space")?,
            payloads: fields.import_execution_payload_array("payloads")?,
        };
        fields.end("import execution batch")?;
        batch.validate()?;
        Ok(batch)
    }

    fn validate(&self) -> Result<()> {
        self.validate_header()?;
        if self.payloads.is_empty() {
            return Err(LoomError::invalid("import execution batch has no payloads"));
        }
        let mut seen = BTreeSet::new();
        for payload in &self.payloads {
            payload.validate()?;
            if !seen.insert(payload.payload_id.as_str()) {
                return Err(LoomError::invalid(format!(
                    "duplicate import execution payload id {}",
                    payload.payload_id
                )));
            }
        }
        Ok(())
    }

    fn validate_header(&self) -> Result<()> {
        validate_text("profile", &self.profile)?;
        validate_text("source_system", &self.source_system)?;
        validate_text("source_scope", &self.source_scope)?;
        if let Some(default_space) = &self.default_space {
            validate_text("default_space", default_space)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileImportAction {
    pub source_entity_id: String,
    pub source_digest: Digest,
    pub target_profile: TargetProfile,
    pub action_kind: ProfileImportActionKind,
    pub target_entity_id: Option<String>,
    pub payload_digest: Option<Digest>,
    pub source_updated_at_ms: Option<u64>,
    pub notes: Vec<String>,
}

impl ProfileImportAction {
    pub fn new(
        source_entity_id: impl Into<String>,
        source_digest: Digest,
        target_profile: TargetProfile,
        action_kind: ProfileImportActionKind,
    ) -> Result<Self> {
        let action = Self {
            source_entity_id: source_entity_id.into(),
            source_digest,
            target_profile,
            action_kind,
            target_entity_id: None,
            payload_digest: None,
            source_updated_at_ms: None,
            notes: Vec::new(),
        };
        action.validate()?;
        Ok(action)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PROFILE_IMPORT_ACTION_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.source_entity_id.clone()),
                Value::Text(self.source_digest.to_string()),
                Value::Uint(self.target_profile.tag()),
                Value::Uint(self.action_kind.tag()),
                optional_text_value(self.target_entity_id.as_deref()),
                optional_digest(self.payload_digest),
                optional_u64(self.source_updated_at_ms),
                string_array(&self.notes),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "profile import action")?;
        outer.expect_text(PROFILE_IMPORT_ACTION_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("profile import action fields")?,
            "profile import action",
        )?;
        outer.end("profile import action")?;
        let action = Self {
            source_entity_id: fields.text("source_entity_id")?,
            source_digest: fields.digest("source_digest")?,
            target_profile: TargetProfile::from_tag(fields.uint("target_profile")?)?,
            action_kind: ProfileImportActionKind::from_tag(fields.uint("action_kind")?)?,
            target_entity_id: fields.optional_text("target_entity_id")?,
            payload_digest: fields.optional_digest("payload_digest")?,
            source_updated_at_ms: fields.optional_u64("source_updated_at_ms")?,
            notes: fields.string_array("notes")?,
        };
        fields.end("profile import action")?;
        action.validate()?;
        Ok(action)
    }

    fn validate(&self) -> Result<()> {
        validate_text("source_entity_id", &self.source_entity_id)?;
        if let Some(target_entity_id) = &self.target_entity_id {
            validate_text("target_entity_id", target_entity_id)?;
        }
        validate_texts("profile import note", &self.notes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileImportPlan {
    pub profile: String,
    pub source_system: SourceSystem,
    pub source_scope: String,
    pub observed_at_ms: u64,
    pub coverage: Coverage,
    pub source_cursor: Option<String>,
    pub source_sidecar_digest: Option<Digest>,
    pub actions: Vec<ProfileImportAction>,
    pub fidelity_issues: Vec<FidelityIssue>,
}

impl ProfileImportPlan {
    pub fn new(
        source_system: SourceSystem,
        source_scope: impl Into<String>,
        observed_at_ms: u64,
        coverage: Coverage,
    ) -> Result<Self> {
        let plan = Self {
            profile: source_system.profile_name().to_string(),
            source_system,
            source_scope: source_scope.into(),
            observed_at_ms,
            coverage,
            source_cursor: None,
            source_sidecar_digest: None,
            actions: Vec::new(),
            fidelity_issues: Vec::new(),
        };
        plan.validate()?;
        Ok(plan)
    }

    pub fn redmine(source_scope: impl Into<String>, observed_at_ms: u64) -> Result<Self> {
        Self::new(
            SourceSystem::Redmine,
            source_scope,
            observed_at_ms,
            Coverage::Partial,
        )
    }

    pub fn jira(source_scope: impl Into<String>, observed_at_ms: u64) -> Result<Self> {
        Self::new(
            SourceSystem::Jira,
            source_scope,
            observed_at_ms,
            Coverage::Complete,
        )
    }

    pub fn confluence_storage(
        source_scope: impl Into<String>,
        observed_at_ms: u64,
    ) -> Result<Self> {
        Self::new(
            SourceSystem::ConfluenceStorage,
            source_scope,
            observed_at_ms,
            Coverage::Complete,
        )
    }

    pub fn confluence_adf(source_scope: impl Into<String>, observed_at_ms: u64) -> Result<Self> {
        Self::new(
            SourceSystem::ConfluenceAdf,
            source_scope,
            observed_at_ms,
            Coverage::Complete,
        )
    }

    pub fn markdown(source_scope: impl Into<String>, observed_at_ms: u64) -> Result<Self> {
        Self::new(
            SourceSystem::Markdown,
            source_scope,
            observed_at_ms,
            Coverage::Complete,
        )
    }

    pub fn notion(source_scope: impl Into<String>, observed_at_ms: u64) -> Result<Self> {
        Self::new(
            SourceSystem::Notion,
            source_scope,
            observed_at_ms,
            Coverage::Partial,
        )
    }

    pub fn asana(source_scope: impl Into<String>, observed_at_ms: u64) -> Result<Self> {
        Self::new(
            SourceSystem::Asana,
            source_scope,
            observed_at_ms,
            Coverage::Partial,
        )
    }

    pub fn slack(source_scope: impl Into<String>, observed_at_ms: u64) -> Result<Self> {
        Self::new(
            SourceSystem::Slack,
            source_scope,
            observed_at_ms,
            Coverage::Complete,
        )
    }

    pub fn drive(source_scope: impl Into<String>, observed_at_ms: u64) -> Result<Self> {
        Self::new(
            SourceSystem::Drive,
            source_scope,
            observed_at_ms,
            Coverage::Partial,
        )
    }

    pub fn granola_api(source_scope: impl Into<String>, observed_at_ms: u64) -> Result<Self> {
        Self::new(
            SourceSystem::GranolaApi,
            source_scope,
            observed_at_ms,
            Coverage::Partial,
        )
    }

    pub fn granola_app(source_scope: impl Into<String>, observed_at_ms: u64) -> Result<Self> {
        Self::new(
            SourceSystem::GranolaApp,
            source_scope,
            observed_at_ms,
            Coverage::Partial,
        )
    }

    pub fn granola_mcp(source_scope: impl Into<String>, observed_at_ms: u64) -> Result<Self> {
        Self::new(
            SourceSystem::GranolaMcp,
            source_scope,
            observed_at_ms,
            Coverage::Partial,
        )
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(PROFILE_IMPORT_PLAN_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.profile.clone()),
                Value::Uint(self.source_system.tag()),
                Value::Text(self.source_scope.clone()),
                Value::Uint(self.observed_at_ms),
                Value::Uint(self.coverage.tag()),
                optional_text_value(self.source_cursor.as_deref()),
                optional_digest(self.source_sidecar_digest),
                Value::Array(
                    self.actions
                        .iter()
                        .map(ProfileImportAction::to_value)
                        .collect(),
                ),
                Value::Array(
                    self.fidelity_issues
                        .iter()
                        .map(FidelityIssue::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "profile import plan")?;
        outer.expect_text(PROFILE_IMPORT_PLAN_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("profile import plan fields")?,
            "profile import plan",
        )?;
        outer.end("profile import plan")?;
        let plan = Self {
            profile: fields.text("profile")?,
            source_system: SourceSystem::from_tag(fields.uint("source_system")?)?,
            source_scope: fields.text("source_scope")?,
            observed_at_ms: fields.uint("observed_at_ms")?,
            coverage: Coverage::from_tag(fields.uint("coverage")?)?,
            source_cursor: fields.optional_text("source_cursor")?,
            source_sidecar_digest: fields.optional_digest("source_sidecar_digest")?,
            actions: fields.profile_import_action_array("actions")?,
            fidelity_issues: fields.fidelity_issue_array("fidelity_issues")?,
        };
        fields.end("profile import plan")?;
        plan.validate()?;
        Ok(plan)
    }

    fn validate(&self) -> Result<()> {
        validate_text("profile", &self.profile)?;
        if self.profile != self.source_system.profile_name() {
            return Err(LoomError::invalid(
                "profile import plan profile must match source system",
            ));
        }
        validate_text("source_scope", &self.source_scope)?;
        if let Some(source_cursor) = &self.source_cursor {
            validate_text("source_cursor", source_cursor)?;
        }
        let mut seen = BTreeSet::new();
        for action in &self.actions {
            action.validate()?;
            let key = (
                action.source_entity_id.as_str(),
                action.target_profile.tag(),
                action.action_kind.tag(),
            );
            if !seen.insert(key) {
                return Err(LoomError::invalid(format!(
                    "duplicate profile import action {}",
                    action.source_entity_id
                )));
            }
        }
        for ticket in &self.fidelity_issues {
            ticket.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportCheckpoint {
    pub checkpoint_id: String,
    pub profile: String,
    pub source_scope: String,
    pub observed_ids: Vec<String>,
    pub completed_units: Vec<String>,
    pub coverage_gaps: Vec<String>,
    pub retry_windows: Vec<String>,
    pub resume_state: Vec<u8>,
    pub profile_state_digest: Option<Digest>,
}

impl ImportCheckpoint {
    pub fn new(
        checkpoint_id: impl Into<String>,
        profile: impl Into<String>,
        source_scope: impl Into<String>,
        resume_state: impl Into<Vec<u8>>,
    ) -> Result<Self> {
        let checkpoint = Self {
            checkpoint_id: checkpoint_id.into(),
            profile: profile.into(),
            source_scope: source_scope.into(),
            observed_ids: Vec::new(),
            completed_units: Vec::new(),
            coverage_gaps: Vec::new(),
            retry_windows: Vec::new(),
            resume_state: resume_state.into(),
            profile_state_digest: None,
        };
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(IMPORT_CHECKPOINT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.checkpoint_id.clone()),
                Value::Text(self.profile.clone()),
                Value::Text(self.source_scope.clone()),
                string_array(&self.observed_ids),
                string_array(&self.completed_units),
                string_array(&self.coverage_gaps),
                string_array(&self.retry_windows),
                Value::Bytes(self.resume_state.clone()),
                optional_digest(self.profile_state_digest),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "import checkpoint")?;
        outer.expect_text(IMPORT_CHECKPOINT_SCHEMA)?;
        let mut fields =
            Fields::array(outer.next("import checkpoint fields")?, "import checkpoint")?;
        outer.end("import checkpoint")?;
        let checkpoint = Self {
            checkpoint_id: fields.text("checkpoint_id")?,
            profile: fields.text("profile")?,
            source_scope: fields.text("source_scope")?,
            observed_ids: fields.string_array("observed_ids")?,
            completed_units: fields.string_array("completed_units")?,
            coverage_gaps: fields.string_array("coverage_gaps")?,
            retry_windows: fields.string_array("retry_windows")?,
            resume_state: fields.bytes("resume_state")?,
            profile_state_digest: fields.optional_digest("profile_state_digest")?,
        };
        fields.end("import checkpoint")?;
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    fn validate(&self) -> Result<()> {
        validate_text("checkpoint_id", &self.checkpoint_id)?;
        validate_text("profile", &self.profile)?;
        validate_text("source_scope", &self.source_scope)?;
        validate_texts("observed_id", &self.observed_ids)?;
        validate_texts("completed_unit", &self.completed_units)?;
        validate_texts("coverage_gap", &self.coverage_gaps)?;
        validate_texts("retry_window", &self.retry_windows)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub path: String,
    pub kind: ArchiveEntryKind,
    pub size: u64,
    pub digest: Option<Digest>,
    pub link_target: Option<String>,
}

impl ArchiveEntry {
    pub fn new(path: impl Into<String>, kind: ArchiveEntryKind, size: u64) -> Result<Self> {
        let entry = Self {
            path: path.into(),
            kind,
            size,
            digest: None,
            link_target: None,
        };
        entry.validate()?;
        Ok(entry)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(ARCHIVE_ENTRY_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.path.clone()),
                Value::Uint(self.kind.tag()),
                Value::Uint(self.size),
                optional_digest(self.digest),
                optional_text_value(self.link_target.as_deref()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "archive entry")?;
        outer.expect_text(ARCHIVE_ENTRY_SCHEMA)?;
        let mut fields = Fields::array(outer.next("archive entry fields")?, "archive entry")?;
        outer.end("archive entry")?;
        let entry = Self {
            path: fields.text("path")?,
            kind: ArchiveEntryKind::from_tag(fields.uint("kind")?)?,
            size: fields.uint("size")?,
            digest: fields.optional_digest("digest")?,
            link_target: fields.optional_text("link_target")?,
        };
        fields.end("archive entry")?;
        entry.validate()?;
        Ok(entry)
    }

    fn validate(&self) -> Result<()> {
        validate_relative_path(&self.path)?;
        if let Some(link_target) = &self.link_target {
            validate_text("link_target", link_target)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveManifest {
    pub archive_id: String,
    pub kind: ArchiveKind,
    pub root_digest: Digest,
    pub entries: Vec<ArchiveEntry>,
}

impl ArchiveManifest {
    pub fn new(
        archive_id: impl Into<String>,
        kind: ArchiveKind,
        root_digest: Digest,
    ) -> Result<Self> {
        let manifest = Self {
            archive_id: archive_id.into(),
            kind,
            root_digest,
            entries: Vec::new(),
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(ARCHIVE_MANIFEST_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.archive_id.clone()),
                Value::Uint(self.kind.tag()),
                Value::Text(self.root_digest.to_string()),
                Value::Array(self.entries.iter().map(ArchiveEntry::to_value).collect()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "archive manifest")?;
        outer.expect_text(ARCHIVE_MANIFEST_SCHEMA)?;
        let mut fields = Fields::array(outer.next("archive manifest fields")?, "archive manifest")?;
        outer.end("archive manifest")?;
        let manifest = Self {
            archive_id: fields.text("archive_id")?,
            kind: ArchiveKind::from_tag(fields.uint("kind")?)?,
            root_digest: fields.digest("root_digest")?,
            entries: fields.archive_entry_array("entries")?,
        };
        fields.end("archive manifest")?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<()> {
        validate_text("archive_id", &self.archive_id)?;
        let mut seen = BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !seen.insert(entry.path.as_str()) {
                return Err(LoomError::invalid(format!(
                    "duplicate archive entry path {}",
                    entry.path
                )));
            }
        }
        Ok(())
    }
}

fn codec_error(error: loom_codec::CodecError) -> LoomError {
    LoomError::corrupt(error.to_string())
}

fn bool_value(value: bool) -> Value {
    Value::Uint(u64::from(value))
}

fn string_array(values: &[String]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|value| Value::Text(value.clone()))
            .collect(),
    )
}

fn optional_text_value(value: Option<&str>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), Value::Text(value.to_string())]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn optional_u64(value: Option<u64>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), Value::Uint(value)]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn optional_digest(value: Option<Digest>) -> Value {
    optional_text_value(value.map(|digest| digest.to_string()).as_deref())
}

fn validate_text(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(LoomError::invalid(format!("{name} must not be blank")));
    }
    Ok(())
}

fn validate_texts(name: &str, values: &[String]) -> Result<()> {
    for value in values {
        validate_text(name, value)?;
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> Result<()> {
    validate_text("path", path)?;
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(LoomError::invalid("archive entry path must be relative"));
    }
    for segment in path.split(['/', '\\']) {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(LoomError::invalid(
                "archive entry path escapes the archive root",
            ));
        }
    }
    Ok(())
}

struct Fields {
    items: std::vec::IntoIter<Value>,
}

impl Fields {
    fn array(value: Value, name: &str) -> Result<Self> {
        match value {
            Value::Array(items) => Ok(Self {
                items: items.into_iter(),
            }),
            _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
        }
    }

    fn next(&mut self, name: &str) -> Result<Value> {
        self.items
            .next()
            .ok_or_else(|| LoomError::corrupt(format!("{name} is missing")))
    }

    fn expect_text(&mut self, expected: &str) -> Result<()> {
        match self.next("schema")? {
            Value::Text(actual) if actual == expected => Ok(()),
            _ => Err(LoomError::corrupt("unexpected schema")),
        }
    }

    fn text(&mut self, name: &str) -> Result<String> {
        match self.next(name)? {
            Value::Text(value) => Ok(value),
            _ => Err(LoomError::corrupt(format!("{name} must be text"))),
        }
    }

    fn bytes(&mut self, name: &str) -> Result<Vec<u8>> {
        match self.next(name)? {
            Value::Bytes(value) => Ok(value),
            _ => Err(LoomError::corrupt(format!("{name} must be bytes"))),
        }
    }

    fn uint(&mut self, name: &str) -> Result<u64> {
        match self.next(name)? {
            Value::Uint(value) => Ok(value),
            _ => Err(LoomError::corrupt(format!("{name} must be uint"))),
        }
    }

    fn bool(&mut self, name: &str) -> Result<bool> {
        match self.uint(name)? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(LoomError::corrupt(format!(
                "{name} has invalid bool tag {other}"
            ))),
        }
    }

    fn digest(&mut self, name: &str) -> Result<Digest> {
        Digest::parse(&self.text(name)?)
    }

    fn string_array(&mut self, name: &str) -> Result<Vec<String>> {
        match self.next(name)? {
            Value::Array(items) => items
                .into_iter()
                .map(|item| match item {
                    Value::Text(value) => Ok(value),
                    _ => Err(LoomError::corrupt(format!("{name} item must be text"))),
                })
                .collect(),
            _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
        }
    }

    fn optional_text(&mut self, name: &str) -> Result<Option<String>> {
        match optional_value(self.next(name)?, name)? {
            Some(Value::Text(value)) => Ok(Some(value)),
            Some(_) => Err(LoomError::corrupt(format!("{name} value must be text"))),
            None => Ok(None),
        }
    }

    fn optional_u64(&mut self, name: &str) -> Result<Option<u64>> {
        match optional_value(self.next(name)?, name)? {
            Some(Value::Uint(value)) => Ok(Some(value)),
            Some(_) => Err(LoomError::corrupt(format!("{name} value must be uint"))),
            None => Ok(None),
        }
    }

    fn optional_digest(&mut self, name: &str) -> Result<Option<Digest>> {
        self.optional_text(name)?
            .map(|value| Digest::parse(&value))
            .transpose()
    }

    fn fidelity_issue_array(&mut self, name: &str) -> Result<Vec<FidelityIssue>> {
        match self.next(name)? {
            Value::Array(items) => items.into_iter().map(FidelityIssue::from_value).collect(),
            _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
        }
    }

    fn import_batch_item_array(&mut self, name: &str) -> Result<Vec<ImportBatchItem>> {
        match self.next(name)? {
            Value::Array(items) => items.into_iter().map(ImportBatchItem::from_value).collect(),
            _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
        }
    }

    fn import_execution_payload_array(
        &mut self,
        name: &str,
    ) -> Result<Vec<ImportExecutionPayload>> {
        match self.next(name)? {
            Value::Array(items) => items
                .into_iter()
                .map(ImportExecutionPayload::from_value)
                .collect(),
            _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
        }
    }

    fn profile_import_action_array(&mut self, name: &str) -> Result<Vec<ProfileImportAction>> {
        match self.next(name)? {
            Value::Array(items) => items
                .into_iter()
                .map(ProfileImportAction::from_value)
                .collect(),
            _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
        }
    }

    fn archive_entry_array(&mut self, name: &str) -> Result<Vec<ArchiveEntry>> {
        match self.next(name)? {
            Value::Array(items) => items.into_iter().map(ArchiveEntry::from_value).collect(),
            _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
        }
    }

    fn end(mut self, name: &str) -> Result<()> {
        if self.items.next().is_some() {
            return Err(LoomError::corrupt(format!("{name} has trailing fields")));
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use loom_types::Algo;

    fn digest(label: &[u8]) -> Digest {
        Digest::hash(Algo::Blake3, label)
    }

    #[test]
    fn schema_names_match_the_public_interchange_contract() {
        assert_eq!(IMPORT_REPORT_SCHEMA, "loom.interchange.import-report.v1");
        assert_eq!(EXPORT_REPORT_SCHEMA, "loom.interchange.export-report.v1");
        assert_eq!(FIDELITY_ISSUE_SCHEMA, "loom.interchange.fidelity-issue.v1");
        assert_eq!(
            IMPORT_CHECKPOINT_SCHEMA,
            "loom.interchange.import-checkpoint.v1"
        );
        assert_eq!(IMPORT_BATCH_SCHEMA, "loom.interchange.import-batch.v1");
        assert_eq!(
            IMPORT_BATCH_ITEM_SCHEMA,
            "loom.interchange.import-batch-item.v1"
        );
        assert_eq!(
            IMPORT_EXECUTION_BATCH_SCHEMA,
            "loom.interchange.import-execution-batch.v1"
        );
        assert_eq!(
            IMPORT_EXECUTION_PAYLOAD_SCHEMA,
            "loom.interchange.import-execution-payload.v1"
        );
        assert_eq!(
            ARCHIVE_MANIFEST_SCHEMA,
            "loom.interchange.archive-manifest.v1"
        );
        assert_eq!(ARCHIVE_ENTRY_SCHEMA, "loom.interchange.archive-entry.v1");
        assert_eq!(
            PROFILE_IMPORT_PLAN_SCHEMA,
            "loom.interchange.profile-import-plan.v1"
        );
        assert_eq!(
            PROFILE_IMPORT_ACTION_SCHEMA,
            "loom.interchange.profile-import-action.v1"
        );
    }

    #[test]
    fn import_report_round_trips_canonical_bytes() {
        let mut report = ImportReport::new(ImportReportInput {
            profile: "granola-api",
            source_scope: "organization:alpha",
            commit: Some(digest(b"commit")),
            objects_added: 7,
            bytes_in: 1024,
            bytes_stored: 512,
            rows_imported: 3,
            skipped: 1,
            operations_planned: 5,
            operations_applied: 4,
            dry_run: true,
        })
        .unwrap();
        report.warnings.push("missing transcript".to_string());
        report.fidelity_issues.push(
            FidelityIssue::new(
                FidelitySeverity::Warning,
                "meeting:1",
                "transcript",
                "source omitted transcript",
            )
            .unwrap(),
        );
        let encoded = report.encode().unwrap();
        assert_eq!(ImportReport::decode(&encoded).unwrap(), report);
        assert_eq!(
            ImportReport::decode(&encoded).unwrap().encode().unwrap(),
            encoded
        );
    }

    #[test]
    fn import_batch_rejects_duplicate_source_entities() {
        let mut batch = ImportBatch::new(
            "jira",
            "jira-cloud",
            "site:example",
            1000,
            Coverage::Partial,
        )
        .unwrap();
        batch
            .items
            .push(ImportBatchItem::new("ticket:1", digest(b"one")).unwrap());
        batch
            .items
            .push(ImportBatchItem::new("ticket:1", digest(b"two")).unwrap());
        assert!(batch.encode().is_err());
    }

    #[test]
    fn import_execution_batch_round_trips_payload_bytes() {
        let mut batch = ImportExecutionBatch::new(
            "tickets",
            "redmine",
            "redmine://example",
            1000,
            Coverage::Complete,
        )
        .unwrap();
        batch.payloads.push(
            ImportExecutionPayload::new(
                "redmine-export",
                "application/json",
                br#"{"issues":[]}"#.to_vec(),
                Algo::Blake3,
            )
            .unwrap(),
        );

        let encoded = batch.encode().unwrap();

        assert_eq!(ImportExecutionBatch::decode(&encoded).unwrap(), batch);
        assert_eq!(
            ImportExecutionBatch::decode(&encoded)
                .unwrap()
                .encode()
                .unwrap(),
            encoded
        );
    }

    #[test]
    fn import_execution_payload_rejects_digest_mismatch() {
        let value = Value::Array(vec![
            Value::Text(IMPORT_EXECUTION_PAYLOAD_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text("payload".to_string()),
                Value::Text("application/json".to_string()),
                Value::Bytes(b"changed".to_vec()),
                Value::Text(digest(b"original").to_string()),
                optional_u64(None),
            ]),
        ]);

        let err = ImportExecutionPayload::from_value(value).unwrap_err();

        assert_eq!(err.code, loom_types::Code::IntegrityFailure);
    }

    #[test]
    fn import_execution_batch_rejects_duplicate_payload_ids() {
        let mut batch =
            ImportExecutionBatch::new("tickets", "jira", "jira://example", 1000, Coverage::Partial)
                .unwrap();
        batch.payloads.push(
            ImportExecutionPayload::new(
                "snapshot",
                "application/json",
                b"one".to_vec(),
                Algo::Blake3,
            )
            .unwrap(),
        );
        batch.payloads.push(
            ImportExecutionPayload::new(
                "snapshot",
                "application/json",
                b"two".to_vec(),
                Algo::Blake3,
            )
            .unwrap(),
        );

        assert!(batch.encode().is_err());
    }

    #[test]
    fn redmine_profile_import_plan_round_trips_ticket_and_page_actions() {
        let mut plan = ProfileImportPlan::redmine("redmine:site", 1000).unwrap();
        plan.source_cursor = Some("offset:42".to_string());
        let mut ticket = ProfileImportAction::new(
            "ticket:9",
            digest(b"redmine ticket"),
            TargetProfile::Tickets,
            ProfileImportActionKind::Ticket,
        )
        .unwrap();
        ticket.target_entity_id = Some("ticket:redmine-9".to_string());
        ticket.payload_digest = Some(digest(b"ticket payload"));
        let mut wiki = ProfileImportAction::new(
            "wiki:Home",
            digest(b"redmine wiki"),
            TargetProfile::Pages,
            ProfileImportActionKind::PageBodyReplace,
        )
        .unwrap();
        wiki.notes.push("redmine wiki page body".to_string());
        plan.actions = vec![ticket, wiki];
        let encoded = plan.encode().unwrap();
        assert_eq!(ProfileImportPlan::decode(&encoded).unwrap(), plan);
        assert_eq!(
            ProfileImportPlan::decode(&encoded).unwrap().coverage,
            Coverage::Partial
        );
    }

    #[test]
    fn jira_profile_import_plan_pins_ticket_actions() {
        let mut plan = ProfileImportPlan::jira("jira:cloud-site", 2000).unwrap();
        plan.actions.push(
            ProfileImportAction::new(
                "project:LOOM",
                digest(b"project"),
                TargetProfile::Tickets,
                ProfileImportActionKind::TicketProject,
            )
            .unwrap(),
        );
        plan.actions.push(
            ProfileImportAction::new(
                "ticket:LOOM-52",
                digest(b"ticket"),
                TargetProfile::Tickets,
                ProfileImportActionKind::Ticket,
            )
            .unwrap(),
        );
        plan.actions.push(
            ProfileImportAction::new(
                "comment:LOOM-52:1",
                digest(b"comment"),
                TargetProfile::Tickets,
                ProfileImportActionKind::TicketComment,
            )
            .unwrap(),
        );
        let encoded = plan.encode().unwrap();
        let decoded = ProfileImportPlan::decode(&encoded).unwrap();
        assert_eq!(decoded.source_system, SourceSystem::Jira);
        assert_eq!(decoded.actions.len(), 3);
    }

    #[test]
    fn confluence_profile_import_plan_supports_both_source_formats() {
        let mut storage = ProfileImportPlan::confluence_storage("space:ENG", 3000).unwrap();
        storage.actions.push(
            ProfileImportAction::new(
                "page:123",
                digest(b"xhtml"),
                TargetProfile::Pages,
                ProfileImportActionKind::PageBodyReplace,
            )
            .unwrap(),
        );
        let mut adf = ProfileImportPlan::confluence_adf("space:ENG", 3000).unwrap();
        adf.actions.push(
            ProfileImportAction::new(
                "page:123",
                digest(b"adf"),
                TargetProfile::Pages,
                ProfileImportActionKind::PageBodyReplace,
            )
            .unwrap(),
        );

        assert_eq!(
            ProfileImportPlan::decode(&storage.encode().unwrap())
                .unwrap()
                .source_system,
            SourceSystem::ConfluenceStorage
        );
        assert_eq!(
            ProfileImportPlan::decode(&adf.encode().unwrap())
                .unwrap()
                .source_system,
            SourceSystem::ConfluenceAdf
        );
    }

    #[test]
    fn remaining_profile_import_sources_have_planning_contracts() {
        let cases = [
            (
                ProfileImportPlan::markdown("vault:docs", 4000).unwrap(),
                SourceSystem::Markdown,
                TargetProfile::Pages,
                ProfileImportActionKind::PageBodyReplace,
            ),
            (
                ProfileImportPlan::notion("organization:notion", 4000).unwrap(),
                SourceSystem::Notion,
                TargetProfile::Pages,
                ProfileImportActionKind::Page,
            ),
            (
                ProfileImportPlan::asana("organization:asana", 4000).unwrap(),
                SourceSystem::Asana,
                TargetProfile::Tickets,
                ProfileImportActionKind::Ticket,
            ),
            (
                ProfileImportPlan::slack("organization:slack", 4000).unwrap(),
                SourceSystem::Slack,
                TargetProfile::Chat,
                ProfileImportActionKind::SourceSidecar,
            ),
            (
                ProfileImportPlan::drive("drive:shared", 4000).unwrap(),
                SourceSystem::Drive,
                TargetProfile::Drive,
                ProfileImportActionKind::SourceSidecar,
            ),
        ];

        for (mut plan, source_system, target_profile, action_kind) in cases {
            plan.actions.push(
                ProfileImportAction::new(
                    "source:1",
                    digest(plan.profile.as_bytes()),
                    target_profile,
                    action_kind,
                )
                .unwrap(),
            );
            let decoded = ProfileImportPlan::decode(&plan.encode().unwrap()).unwrap();
            assert_eq!(decoded.source_system, source_system);
            assert_eq!(decoded.actions[0].target_profile, target_profile);
        }
    }

    #[test]
    fn profile_import_plan_rejects_duplicate_planned_actions() {
        let mut plan = ProfileImportPlan::jira("jira:site", 5000).unwrap();
        let action = ProfileImportAction::new(
            "ticket:LOOM-1",
            digest(b"one"),
            TargetProfile::Tickets,
            ProfileImportActionKind::Ticket,
        )
        .unwrap();
        plan.actions.push(action.clone());
        plan.actions.push(action);
        assert!(plan.encode().is_err());
    }

    #[test]
    fn granola_profile_import_plans_target_meetings() {
        let mut plans = vec![
            ProfileImportPlan::granola_api("organization:api", 6000).unwrap(),
            ProfileImportPlan::granola_app("organization:app-cache", 6000).unwrap(),
            ProfileImportPlan::granola_mcp("organization:mcp", 6000).unwrap(),
        ];

        for plan in &mut plans {
            plan.actions.push(
                ProfileImportAction::new(
                    "meeting:1",
                    digest(plan.profile.as_bytes()),
                    TargetProfile::Meetings,
                    ProfileImportActionKind::SourceSidecar,
                )
                .unwrap(),
            );
            let decoded = ProfileImportPlan::decode(&plan.encode().unwrap()).unwrap();
            assert_eq!(decoded.actions[0].target_profile, TargetProfile::Meetings);
            assert_eq!(decoded.coverage, Coverage::Partial);
        }
    }

    #[test]
    fn checkpoint_round_trips_profile_state() {
        let mut checkpoint = ImportCheckpoint::new(
            "checkpoint:1",
            "granola-api",
            "organization:alpha",
            b"cursor-1",
        )
        .unwrap();
        checkpoint.observed_ids.push("meeting:1".to_string());
        checkpoint.completed_units.push("page:1".to_string());
        checkpoint.coverage_gaps.push("rate limited".to_string());
        checkpoint
            .retry_windows
            .push("after:1700000000000".to_string());
        checkpoint.profile_state_digest = Some(digest(b"profile-state"));
        let encoded = checkpoint.encode().unwrap();
        assert_eq!(ImportCheckpoint::decode(&encoded).unwrap(), checkpoint);
    }

    #[test]
    fn archive_manifest_rejects_absolute_paths_and_parent_escape() {
        assert!(ArchiveEntry::new("/tmp/file", ArchiveEntryKind::File, 1).is_err());
        assert!(ArchiveEntry::new("a/../file", ArchiveEntryKind::File, 1).is_err());

        let mut manifest =
            ArchiveManifest::new("archive:1", ArchiveKind::Zip, digest(b"archive")).unwrap();
        manifest
            .entries
            .push(ArchiveEntry::new("notes/a.md", ArchiveEntryKind::File, 42).unwrap());
        let encoded = manifest.encode().unwrap();
        assert_eq!(ArchiveManifest::decode(&encoded).unwrap(), manifest);
    }
}
