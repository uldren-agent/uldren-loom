use std::collections::BTreeSet;
use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;

use loom_core::{Algo, FacetKind, Loom, LoomError, Result};
use loom_interchange::{Coverage, ImportCheckpoint, ImportExecutionBatch, ImportExecutionPayload};
use loom_interchange_io::{
    ImportExecutionBatchResult, execute_import_execution_batch, import_meetings_bytes,
    load_meetings_snapshot, meetings_import_checkpoint_key, meetings_source_payload_path,
    parse_meetings_input_profile,
};
use loom_store::FileStore;
use loom_substrate::meetings::{Coverage as MeetingsCoverage, MeetingStatus};
use loom_types::{Code, WorkspaceId};
use serde_json::Value;
use zip::write::SimpleFileOptions;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioImportFixtureConformanceSummary {
    pub fixtures_run: usize,
    pub rows_imported: u64,
    pub fidelity_fields_checked: usize,
    pub profiles_run: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingsImportExecutionConformanceSummary {
    pub cases_run: usize,
    pub rows_imported: u64,
    pub retained_sources_checked: usize,
    pub statuses_checked: usize,
}

#[derive(Clone, Copy)]
struct StudioImportFixture {
    name: &'static str,
    profile: &'static str,
    source_system: &'static str,
    source_scope: &'static str,
    default_space: Option<&'static str>,
    expected_json: &'static str,
    payloads: fn() -> Result<Vec<ImportExecutionPayload>>,
    verify: fn(&Loom<FileStore>, WorkspaceId, &str, &Value) -> Result<()>,
}

pub fn run_studio_import_fixture_conformance() -> Result<StudioImportFixtureConformanceSummary> {
    let fixtures = studio_import_fixtures();
    let mut summary = StudioImportFixtureConformanceSummary {
        fixtures_run: 0,
        rows_imported: 0,
        fidelity_fields_checked: 0,
        profiles_run: Vec::new(),
    };
    for (index, fixture) in fixtures.iter().enumerate() {
        let expected = expected_json(fixture.name, fixture.expected_json)?;
        let (mut loom, ns, temp) = fresh_loom(index as u8 + 70)?;
        let result = run_fixture(&mut loom, ns, fixture)?;
        let verify_result = (|| {
            verify_report(fixture, &result, &expected)?;
            (fixture.verify)(&loom, ns, &ns.to_string(), &expected)?;
            Ok(())
        })();
        let cleanup_result = fs::remove_dir_all(&temp).map_err(|error| {
            LoomError::new(
                Code::Io,
                format!(
                    "remove conformance fixture directory {}: {error}",
                    temp.display()
                ),
            )
        });
        match (verify_result, cleanup_result) {
            (Ok(()), Ok(())) => {}
            (Err(error), _) | (Ok(()), Err(error)) => return Err(error),
        }
        summary.fixtures_run += 1;
        summary.rows_imported += result.report.rows_imported;
        summary.fidelity_fields_checked += expected_unsupported_fields(&expected)?.len();
        summary.profiles_run.push(fixture.profile);
    }
    Ok(summary)
}

pub fn run_meetings_import_execution_conformance()
-> Result<MeetingsImportExecutionConformanceSummary> {
    let vectors = meetings_execution_fidelity_json()?;
    let cases = vectors
        .get("cases")
        .and_then(Value::as_array)
        .ok_or_else(|| LoomError::invalid("meetings execution-fidelity cases must be an array"))?;
    let mut summary = MeetingsImportExecutionConformanceSummary {
        cases_run: 0,
        rows_imported: 0,
        retained_sources_checked: 0,
        statuses_checked: 0,
    };

    for (index, case) in cases.iter().enumerate() {
        let (mut loom, ns, temp) = fresh_loom(index as u8 + 90)?;
        let verify_result =
            run_meetings_execution_case(&mut loom, ns, case, &vectors, &mut summary);
        let cleanup_result = fs::remove_dir_all(&temp).map_err(|error| {
            LoomError::new(
                Code::Io,
                format!(
                    "remove meetings conformance directory {}: {error}",
                    temp.display()
                ),
            )
        });
        match (verify_result, cleanup_result) {
            (Ok(()), Ok(())) => {}
            (Err(error), _) | (Ok(()), Err(error)) => return Err(error),
        }
    }

    run_invalid_meetings_source_state_case(&vectors)?;
    Ok(summary)
}

fn studio_import_fixtures() -> Vec<StudioImportFixture> {
    vec![
        StudioImportFixture {
            name: "redmine",
            profile: "tickets",
            source_system: "redmine",
            source_scope: "redmine-api",
            default_space: None,
            expected_json: include_str!(
                "../../../specs/studio/fixtures/redmine/expected/comparison.json"
            ),
            payloads: redmine_payloads,
            verify: verify_redmine,
        },
        StudioImportFixture {
            name: "asana",
            profile: "tickets",
            source_system: "asana",
            source_scope: "asana-normalized",
            default_space: None,
            expected_json: include_str!(
                "../../../specs/studio/fixtures/asana/expected/comparison.json"
            ),
            payloads: asana_payloads,
            verify: verify_asana,
        },
        StudioImportFixture {
            name: "jira",
            profile: "tickets",
            source_system: "jira",
            source_scope: "jira-normalized",
            default_space: None,
            expected_json: include_str!(
                "../../../specs/studio/fixtures/jira/expected/comparison.json"
            ),
            payloads: jira_payloads,
            verify: verify_jira,
        },
        StudioImportFixture {
            name: "confluence",
            profile: "pages",
            source_system: "confluence",
            source_scope: "confluence-normalized",
            default_space: Some("default"),
            expected_json: include_str!(
                "../../../specs/studio/fixtures/confluence/expected/comparison.json"
            ),
            payloads: confluence_payloads,
            verify: verify_confluence,
        },
        StudioImportFixture {
            name: "slack",
            profile: "chat",
            source_system: "slack",
            source_scope: "slack-normalized",
            default_space: None,
            expected_json: include_str!(
                "../../../specs/studio/fixtures/slack/expected/comparison.json"
            ),
            payloads: slack_payloads,
            verify: verify_slack,
        },
        StudioImportFixture {
            name: "drive",
            profile: "drive",
            source_system: "drive",
            source_scope: "drive://workspace/example",
            default_space: None,
            expected_json: include_str!(
                "../../../specs/studio/fixtures/drive/expected/comparison.json"
            ),
            payloads: drive_payloads,
            verify: verify_drive,
        },
        StudioImportFixture {
            name: "markdown",
            profile: "pages",
            source_system: "markdown",
            source_scope: "vault",
            default_space: Some("docs"),
            expected_json: include_str!(
                "../../../specs/studio/fixtures/markdown/expected/comparison.json"
            ),
            payloads: markdown_payloads,
            verify: verify_markdown,
        },
        StudioImportFixture {
            name: "notion",
            profile: "pages",
            source_system: "notion",
            source_scope: "notion-api",
            default_space: Some("notion"),
            expected_json: include_str!(
                "../../../specs/studio/fixtures/notion/expected/comparison.json"
            ),
            payloads: notion_payloads,
            verify: verify_notion,
        },
        StudioImportFixture {
            name: "meetings",
            profile: "meetings",
            source_system: "granola-app",
            source_scope: "granola-api",
            default_space: None,
            expected_json: include_str!(
                "../../../specs/studio/fixtures/meetings/expected/comparison.json"
            ),
            payloads: meetings_payloads,
            verify: verify_meetings,
        },
    ]
}

fn run_meetings_execution_case(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    case: &Value,
    vectors: &Value,
    summary: &mut MeetingsImportExecutionConformanceSummary,
) -> Result<()> {
    let name = case_string(case, "name")?;
    let profile = parse_meetings_input_profile(case_string(case, "input_profile")?)?;
    let fixture = meetings_fixture_bytes(case_string(case, "fixture")?)?;
    let first = import_meetings_bytes(loom, ns, profile, fixture, false)?;
    let retry = import_meetings_bytes(loom, ns, profile, fixture, false)?;

    assert!(first.changed, "{name}");
    assert!(!retry.changed, "{name}");
    assert_eq!(retry.report.operations_applied, 0, "{name}");
    let idempotent_warning = vectors
        .get("idempotent_warning")
        .and_then(Value::as_str)
        .ok_or_else(|| LoomError::invalid("meetings idempotent warning must be a string"))?;
    assert!(
        retry
            .report
            .warnings
            .iter()
            .any(|warning| warning == idempotent_warning),
        "{name}"
    );
    assert_eq!(
        first.report.source_scope,
        case_string(case, "source_scope")?,
        "{name}"
    );
    let rows_imported = case_u64(case, "rows_imported")?;
    assert_eq!(first.report.rows_imported, rows_imported, "{name}");

    let profile_id = ns.to_string();
    let snapshot = load_meetings_snapshot(loom, &profile_id)?
        .ok_or_else(|| LoomError::not_found("meetings conformance snapshot missing"))?;
    assert_eq!(
        snapshot.sources.len() as u64,
        case_u64(case, "sources")?,
        "{name}"
    );
    assert_eq!(
        snapshot.meetings.len() as u64,
        case_u64(case, "meetings")?,
        "{name}"
    );
    assert_eq!(
        snapshot.spans.len() as u64,
        case_u64(case, "spans")?,
        "{name}"
    );
    assert_eq!(
        snapshot.annotations.len() as u64,
        case_u64(case, "annotations")?,
        "{name}"
    );

    let run = snapshot
        .import_runs
        .first()
        .ok_or_else(|| LoomError::not_found("meetings import run missing"))?;
    assert_eq!(
        coverage_label(run.coverage),
        case_string(case, "coverage")?,
        "{name}"
    );
    let checkpoint = ImportCheckpoint::decode(
        &loom
            .store()
            .control_get(&meetings_import_checkpoint_key(
                &profile_id,
                &run.import_run_id,
            ))?
            .ok_or_else(|| LoomError::not_found("meetings import checkpoint missing"))?,
    )?;
    assert_eq!(checkpoint.observed_ids, run.observed_ids, "{name}");
    assert_eq!(checkpoint.coverage_gaps, run.coverage_gaps, "{name}");
    assert_eq!(checkpoint.retry_windows, run.retry_windows, "{name}");
    assert!(checkpoint.profile_state_digest.is_some(), "{name}");

    let retained_sources = case
        .get("payload_sources")
        .and_then(Value::as_array)
        .ok_or_else(|| LoomError::invalid("meetings payload_sources must be an array"))?;
    for source_id in retained_sources {
        let source_id = source_id
            .as_str()
            .ok_or_else(|| LoomError::invalid("meetings payload source must be a string"))?;
        loom.read_file_reserved(
            ns,
            &meetings_source_payload_path(&profile_id, source_id, "source.json"),
        )?;
        summary.retained_sources_checked += 1;
    }

    let expected_statuses = case
        .get("meeting_statuses")
        .and_then(Value::as_object)
        .ok_or_else(|| LoomError::invalid("meetings meeting_statuses must be an object"))?;
    for meeting in &snapshot.meetings {
        let expected = expected_statuses
            .get(&meeting.meeting_id)
            .and_then(Value::as_str)
            .ok_or_else(|| LoomError::invalid("meetings expected status missing"))?;
        assert_eq!(meeting_status_label(meeting.status), expected, "{name}");
        summary.statuses_checked += 1;
    }

    summary.cases_run += 1;
    summary.rows_imported += rows_imported;
    Ok(())
}

fn run_invalid_meetings_source_state_case(vectors: &Value) -> Result<()> {
    let (mut loom, ns, temp) = fresh_loom(99)?;
    let source_digest = loom_types::Digest::hash(Algo::Blake3, b"source").to_string();
    let invalid = serde_json::to_vec(&serde_json::json!({
        "snapshot_version": 1,
        "profile": "granola-app",
        "source_system": "granola-app",
        "source_scope": "local-cache",
        "observed_at": 500,
        "coverage": "complete",
        "items": [{
            "source_entity_id": "note-1",
            "source_digest": source_digest,
            "source_state": "missing",
            "title": "Invalid state"
        }]
    }))
    .map_err(|error| LoomError::invalid(format!("encode invalid meetings fixture: {error}")))?;
    let profile = parse_meetings_input_profile("granola-app")?;
    let result = import_meetings_bytes(&mut loom, ns, profile, &invalid, false);
    let cleanup_result = fs::remove_dir_all(&temp).map_err(|error| {
        LoomError::new(
            Code::Io,
            format!(
                "remove invalid meetings conformance directory {}: {error}",
                temp.display()
            ),
        )
    });
    match (result, cleanup_result) {
        (Err(error), Ok(())) => {
            assert_eq!(error.code, Code::InvalidArgument);
            let message = vectors
                .get("invalid_source_state_message")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    LoomError::invalid("meetings invalid source-state message must be a string")
                })?;
            assert!(error.message.contains(message));
            Ok(())
        }
        (Ok(_), _) => Err(LoomError::invalid(
            "invalid meetings source state unexpectedly imported",
        )),
        (Err(error), Err(cleanup)) => Err(LoomError::new(
            cleanup.code,
            format!(
                "{}; after expected import error: {}",
                cleanup.message, error.message
            ),
        )),
    }
}

fn meetings_execution_fidelity_json() -> Result<Value> {
    expected_json(
        "meetings execution fidelity",
        include_str!("../../../specs/studio/fixtures/meetings/expected/execution-fidelity.json"),
    )
}

fn meetings_fixture_bytes(name: &str) -> Result<&'static [u8]> {
    match name {
        "granola-broad-snapshot.json" => Ok(include_bytes!(
            "../../../specs/studio/fixtures/meetings/source/granola-broad-snapshot.json"
        )),
        "granola-api-snapshot.json" => Ok(include_bytes!(
            "../../../specs/studio/fixtures/meetings/source/granola-api-snapshot.json"
        )),
        "granola-mcp-snapshot.json" => Ok(include_bytes!(
            "../../../specs/studio/fixtures/meetings/source/granola-mcp-snapshot.json"
        )),
        "granola-csv-snapshot.json" => Ok(include_bytes!(
            "../../../specs/studio/fixtures/meetings/source/granola-csv-snapshot.json"
        )),
        other => Err(LoomError::invalid(format!(
            "unknown meetings conformance fixture {other}"
        ))),
    }
}

fn case_string<'a>(case: &'a Value, key: &str) -> Result<&'a str> {
    case.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| LoomError::invalid(format!("meetings case {key} must be a string")))
}

fn case_u64(case: &Value, key: &str) -> Result<u64> {
    case.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| LoomError::invalid(format!("meetings case {key} must be an integer")))
}

const fn coverage_label(coverage: MeetingsCoverage) -> &'static str {
    match coverage {
        MeetingsCoverage::Complete => "complete",
        MeetingsCoverage::Partial => "partial",
        MeetingsCoverage::Degraded => "degraded",
    }
}

const fn meeting_status_label(status: MeetingStatus) -> &'static str {
    match status {
        MeetingStatus::Active => "active",
        MeetingStatus::DeletedAtSource => "deleted_at_source",
        MeetingStatus::Redacted => "redacted",
        MeetingStatus::RetainedMetadataOnly => "retained_metadata_only",
    }
}

fn fresh_loom(seed: u8) -> Result<(Loom<FileStore>, WorkspaceId, PathBuf)> {
    let temp = std::env::temp_dir().join(format!(
        "loom-conformance-studio-import-{}-{seed}-{}",
        std::process::id(),
        now_ms()
    ));
    fs::create_dir_all(&temp).map_err(|error| {
        LoomError::new(
            Code::Io,
            format!(
                "create conformance fixture directory {}: {error}",
                temp.display()
            ),
        )
    })?;
    let store = FileStore::create_with_profile(temp.join("fixture.loom"), Algo::Blake3)?;
    let mut loom = Loom::new(store);
    let ns = WorkspaceId::from_bytes([seed; 16]);
    loom.registry_mut()
        .create(FacetKind::Vcs, Some("studio"), ns)?;
    Ok((loom, ns, temp))
}

fn run_fixture(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    fixture: &StudioImportFixture,
) -> Result<ImportExecutionBatchResult> {
    let mut batch = ImportExecutionBatch::new(
        fixture.profile,
        fixture.source_system,
        fixture.source_scope,
        1_700_000_000_000,
        Coverage::Complete,
    )?;
    batch.default_space = fixture.default_space.map(str::to_string);
    batch.payloads = (fixture.payloads)()?;
    let bytes = batch.encode()?;
    execute_import_execution_batch(loom, ns, &bytes, false, None)
}

fn verify_report(
    fixture: &StudioImportFixture,
    result: &ImportExecutionBatchResult,
    expected: &Value,
) -> Result<()> {
    let expected_profile = if fixture.profile == "meetings" {
        fixture.profile
    } else {
        fixture.source_system
    };
    assert_eq!(result.report.profile, expected_profile, "{}", fixture.name);
    if let Some(source_scope) = expected.get("source_scope").and_then(Value::as_str) {
        assert_eq!(result.report.source_scope, source_scope, "{}", fixture.name);
    }
    if let Some(rows_imported) = expected.get("rows_imported").and_then(Value::as_u64) {
        assert_eq!(
            result.report.rows_imported, rows_imported,
            "{}",
            fixture.name
        );
    } else {
        assert!(result.report.rows_imported > 0, "{}", fixture.name);
    }
    assert!(result.changed, "{}", fixture.name);
    verify_unsupported_fields(fixture.name, result, expected)
}

fn verify_unsupported_fields(
    name: &str,
    result: &ImportExecutionBatchResult,
    expected: &Value,
) -> Result<()> {
    let expected_fields = expected_unsupported_fields(expected)?;
    let reported = result
        .report
        .fidelity_issues
        .iter()
        .map(|issue| issue.field.as_str())
        .collect::<BTreeSet<_>>();
    for field in expected_fields {
        assert!(
            reported.contains(field),
            "{name}: missing fidelity field {field}"
        );
    }
    Ok(())
}

fn expected_unsupported_fields(expected: &Value) -> Result<Vec<&str>> {
    let Some(fields) = expected.get("unsupported_fields") else {
        return Ok(Vec::new());
    };
    let Some(values) = fields.as_array() else {
        return Err(LoomError::invalid("unsupported_fields must be an array"));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| LoomError::invalid("unsupported_fields entries must be strings"))
        })
        .collect()
}

fn expected_json(name: &str, bytes: &str) -> Result<Value> {
    serde_json::from_str(bytes)
        .map_err(|error| LoomError::invalid(format!("parse {name} comparison fixture: {error}")))
}

fn redmine_payloads() -> Result<Vec<ImportExecutionPayload>> {
    json_payload(
        "redmine-api-bundle.xml",
        "application/xml",
        include_bytes!("../../../specs/studio/fixtures/redmine/source/redmine-api-bundle.xml"),
    )
}

fn asana_payloads() -> Result<Vec<ImportExecutionPayload>> {
    json_payload(
        "asana-normalized-snapshot.json",
        "application/json",
        include_bytes!(
            "../../../specs/studio/fixtures/asana/source/asana-normalized-snapshot.json"
        ),
    )
}

fn jira_payloads() -> Result<Vec<ImportExecutionPayload>> {
    json_payload(
        "jira-normalized-snapshot.json",
        "application/json",
        include_bytes!("../../../specs/studio/fixtures/jira/source/jira-normalized-snapshot.json"),
    )
}

fn confluence_payloads() -> Result<Vec<ImportExecutionPayload>> {
    json_payload(
        "confluence-normalized-snapshot.json",
        "application/json",
        include_bytes!(
            "../../../specs/studio/fixtures/confluence/source/confluence-normalized-snapshot.json"
        ),
    )
}

fn slack_payloads() -> Result<Vec<ImportExecutionPayload>> {
    json_payload(
        "slack-normalized-snapshot.json",
        "application/json",
        include_bytes!(
            "../../../specs/studio/fixtures/slack/source/slack-normalized-snapshot.json"
        ),
    )
}

fn drive_payloads() -> Result<Vec<ImportExecutionPayload>> {
    let mut payloads = json_payload(
        "drive-sharepoint-snapshot.json",
        "application/json",
        include_bytes!(
            "../../../specs/studio/fixtures/drive/source/drive-sharepoint-snapshot.json"
        ),
    )?;
    payloads.push(ImportExecutionPayload::new(
        "files/path-note.txt",
        "text/plain",
        include_bytes!("../../../specs/studio/fixtures/drive/source/files/path-note.txt").to_vec(),
        Algo::Blake3,
    )?);
    Ok(payloads)
}

fn markdown_payloads() -> Result<Vec<ImportExecutionPayload>> {
    Ok(vec![ImportExecutionPayload::new(
        "vault.zip",
        "application/zip",
        markdown_archive()?,
        Algo::Blake3,
    )?])
}

fn notion_payloads() -> Result<Vec<ImportExecutionPayload>> {
    json_payload(
        "notion-api-bundle.json",
        "application/json",
        include_bytes!("../../../specs/studio/fixtures/notion/source/notion-api-bundle.json"),
    )
}

fn meetings_payloads() -> Result<Vec<ImportExecutionPayload>> {
    json_payload(
        "granola-broad-snapshot.json",
        "application/json",
        include_bytes!(
            "../../../specs/studio/fixtures/meetings/source/granola-broad-snapshot.json"
        ),
    )
}

fn json_payload(
    payload_id: &str,
    media_type: &str,
    bytes: &'static [u8],
) -> Result<Vec<ImportExecutionPayload>> {
    Ok(vec![ImportExecutionPayload::new(
        payload_id,
        media_type,
        bytes.to_vec(),
        Algo::Blake3,
    )?])
}

fn markdown_archive() -> Result<Vec<u8>> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for dir in ["Guides/", ".obsidian/", "Attachments/"] {
        archive.add_directory(dir, options).map_err(zip_error)?;
    }
    for (path, bytes) in markdown_files() {
        archive.start_file(path, options).map_err(zip_error)?;
        archive.write_all(bytes).map_err(|error| {
            LoomError::new(Code::Io, format!("write markdown fixture archive: {error}"))
        })?;
    }
    let cursor = archive.finish().map_err(zip_error)?;
    Ok(cursor.into_inner())
}

fn markdown_files() -> [(&'static str, &'static [u8]); 8] {
    [
        (
            "Intro.md",
            include_bytes!("../../../specs/studio/fixtures/markdown/source/vault/Intro.md"),
        ),
        (
            "Embed.md",
            include_bytes!("../../../specs/studio/fixtures/markdown/source/vault/Embed.md"),
        ),
        (
            "Guides/Guides.md",
            include_bytes!("../../../specs/studio/fixtures/markdown/source/vault/Guides/Guides.md"),
        ),
        (
            "Guides/Setup.md",
            include_bytes!("../../../specs/studio/fixtures/markdown/source/vault/Guides/Setup.md"),
        ),
        (
            ".obsidian/app.json",
            include_bytes!(
                "../../../specs/studio/fixtures/markdown/source/vault/.obsidian/app.json"
            ),
        ),
        (
            ".obsidian/types.json",
            include_bytes!(
                "../../../specs/studio/fixtures/markdown/source/vault/.obsidian/types.json"
            ),
        ),
        (
            "Board.canvas",
            include_bytes!("../../../specs/studio/fixtures/markdown/source/vault/Board.canvas"),
        ),
        (
            "Sketch.excalidraw",
            include_bytes!(
                "../../../specs/studio/fixtures/markdown/source/vault/Sketch.excalidraw"
            ),
        ),
    ]
}

fn zip_error(error: zip::result::ZipError) -> LoomError {
    LoomError::new(Code::Io, format!("build markdown fixture archive: {error}"))
}

fn verify_redmine(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    expected: &Value,
) -> Result<()> {
    verify_project(
        loom,
        ns,
        workspace_id,
        expected.pointer("/project/id"),
        expected.pointer("/project/name"),
    )?;
    let ticket = ticket_by_external(loom, ns, workspace_id, "redmine", "issue:42")?;
    assert_eq!(
        ticket.fields.get("subject"),
        expected.pointer("/issue/subject")
    );
    assert_eq!(
        ticket.fields.get("status"),
        expected.pointer("/issue/status")
    );
    assert!(loom_pages::get_page(loom, ns, workspace_id, "home")?.is_some());
    Ok(())
}

fn verify_asana(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    expected: &Value,
) -> Result<()> {
    verify_project(
        loom,
        ns,
        workspace_id,
        expected.pointer("/project/project_id"),
        expected.pointer("/project/name"),
    )?;
    let ticket = ticket_by_external(loom, ns, workspace_id, "asana", "task:t-100")?;
    assert_eq!(
        ticket.fields.get("subject"),
        expected.pointer("/task/subject")
    );
    assert_eq!(
        ticket.fields.get("description"),
        expected.pointer("/task/description")
    );
    let approval = ticket_by_external(loom, ns, workspace_id, "asana", "task:101")?;
    assert!(
        approval
            .policy_labels
            .iter()
            .any(|label| label == "approval")
    );
    Ok(())
}

fn verify_jira(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    expected: &Value,
) -> Result<()> {
    verify_project(
        loom,
        ns,
        workspace_id,
        expected.pointer("/project/project_id"),
        expected.pointer("/project/name"),
    )?;
    let ticket = ticket_by_external(loom, ns, workspace_id, "jira", "issue:10042")?;
    assert_eq!(
        ticket.fields.get("subject"),
        expected.pointer("/issue/subject")
    );
    assert_eq!(
        ticket.fields.get("status"),
        expected.pointer("/issue/status")
    );
    let bug = ticket_by_external(loom, ns, workspace_id, "jira", "issue:10043")?;
    assert!(bug.policy_labels.iter().any(|label| label == "bug"));
    Ok(())
}

fn verify_confluence(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    expected: &Value,
) -> Result<()> {
    assert_eq!(loom_pages::list_spaces(loom, ns, workspace_id)?.len(), 2);
    verify_page_title(
        loom,
        ns,
        workspace_id,
        "home",
        expected.pointer("/pages/home/title"),
    )?;
    verify_page_title(
        loom,
        ns,
        workspace_id,
        "child-adf",
        expected.pointer("/pages/child-adf/title"),
    )?;
    let child = loom_pages::get_page(loom, ns, workspace_id, "child-adf")?
        .ok_or_else(|| LoomError::not_found("confluence child page missing"))?;
    assert_eq!(
        child.parent_page_id.as_deref(),
        expected
            .pointer("/pages/child-adf/parent_page_id")
            .and_then(Value::as_str)
    );
    Ok(())
}

fn verify_slack(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    expected: &Value,
) -> Result<()> {
    let channel_id = loom_chat::resolve_channel_id(loom, ns, workspace_id, "eng-imports")?;
    let projection = loom_chat::channel_projection(loom, ns, workspace_id, &channel_id)?;
    assert_eq!(
        projection.messages.len() as u64,
        expected
            .pointer("/channel/message_count")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    );
    assert!(projection.messages.iter().any(|message| {
        String::from_utf8_lossy(&message.body)
            == expected
                .pointer("/channel/root_body")
                .and_then(Value::as_str)
                .unwrap_or("")
    }));
    assert_eq!(
        projection.threads.len() as u64,
        expected
            .pointer("/channel/thread_count")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    );
    Ok(())
}

fn verify_drive(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    expected: &Value,
) -> Result<()> {
    assert_eq!(
        loom_drive::list_folder(loom, ns, workspace_id, "root")?
            .entries
            .len() as u64,
        expected
            .pointer("/folders/root_count")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    );
    assert_eq!(
        String::from_utf8_lossy(&loom_drive::read_file(
            loom,
            ns,
            workspace_id,
            "file-readme"
        )?),
        expected
            .pointer("/files/readme")
            .and_then(Value::as_str)
            .unwrap_or("")
    );
    assert_eq!(
        String::from_utf8_lossy(&loom_drive::read_file(
            loom,
            ns,
            workspace_id,
            "file-sidecar"
        )?),
        expected
            .pointer("/files/sidecar")
            .and_then(Value::as_str)
            .unwrap_or("")
    );
    Ok(())
}

fn verify_markdown(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    expected: &Value,
) -> Result<()> {
    verify_page_title(
        loom,
        ns,
        workspace_id,
        "intro",
        expected.pointer("/pages/intro/title"),
    )?;
    verify_page_title(
        loom,
        ns,
        workspace_id,
        "embed",
        expected.pointer("/pages/embed/title"),
    )?;
    verify_page_title(
        loom,
        ns,
        workspace_id,
        "guides",
        expected.pointer("/pages/guides/title"),
    )?;
    verify_page_title(
        loom,
        ns,
        workspace_id,
        "guides-setup",
        expected.pointer("/pages/guides-setup/title"),
    )
}

fn verify_notion(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    expected: &Value,
) -> Result<()> {
    verify_page_title(
        loom,
        ns,
        workspace_id,
        "page-intro",
        expected.pointer("/pages/page-intro/title"),
    )?;
    verify_page_title(
        loom,
        ns,
        workspace_id,
        "child",
        expected.pointer("/pages/child/title"),
    )?;
    verify_page_title(
        loom,
        ns,
        workspace_id,
        "database-row",
        expected.pointer("/pages/database-row/title"),
    )
}

fn verify_meetings(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    expected: &Value,
) -> Result<()> {
    let snapshot = loom_interchange_io::load_meetings_snapshot(loom, workspace_id)?
        .ok_or_else(|| LoomError::not_found("meetings snapshot missing"))?;
    assert_eq!(
        snapshot.sources.len() as u64,
        expected.get("sources").and_then(Value::as_u64).unwrap_or(0)
    );
    assert_eq!(
        snapshot.meetings.len() as u64,
        expected
            .get("meetings")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    );
    assert_eq!(ns.to_string(), workspace_id);
    Ok(())
}

fn verify_project(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    project_id: Option<&Value>,
    name: Option<&Value>,
) -> Result<()> {
    let project_id = project_id
        .and_then(Value::as_str)
        .ok_or_else(|| LoomError::invalid("expected project id missing"))?;
    let name = name
        .and_then(Value::as_str)
        .ok_or_else(|| LoomError::invalid("expected project name missing"))?;
    let project = loom_tickets::list_projects(loom, ns, workspace_id)?
        .into_iter()
        .find(|project| project.project_id == project_id)
        .ok_or_else(|| LoomError::not_found("imported project missing"))?;
    assert_eq!(project.name, name);
    Ok(())
}

fn ticket_by_external(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    source: &str,
    id: &str,
) -> Result<loom_tickets::TicketSummary> {
    loom_tickets::list_tickets(loom, ns, workspace_id)?
        .into_iter()
        .find(|ticket| {
            ticket.external_source.as_deref() == Some(source)
                && ticket.external_id.as_deref() == Some(id)
        })
        .ok_or_else(|| LoomError::not_found("imported ticket missing"))
}

fn verify_page_title(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    page_id: &str,
    expected: Option<&Value>,
) -> Result<()> {
    let expected = expected
        .and_then(Value::as_str)
        .ok_or_else(|| LoomError::invalid("expected page title missing"))?;
    let page = loom_pages::get_page(loom, ns, workspace_id, page_id)?
        .ok_or_else(|| LoomError::not_found("imported page missing"))?;
    assert_eq!(page.title, expected);
    Ok(())
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn studio_import_fixtures_execute_against_source_backed_profiles() {
        let summary = run_studio_import_fixture_conformance().unwrap();
        assert_eq!(summary.fixtures_run, 9);
        assert_eq!(
            summary.profiles_run,
            vec![
                "tickets", "tickets", "tickets", "pages", "chat", "drive", "pages", "pages",
                "meetings"
            ]
        );
        assert!(summary.rows_imported >= 34);
        assert!(summary.fidelity_fields_checked >= 150);
    }

    #[test]
    fn meetings_import_execution_fidelity_vectors_pass() {
        let summary = run_meetings_import_execution_conformance().unwrap();
        assert_eq!(summary.cases_run, 4);
        assert_eq!(summary.rows_imported, 9);
        assert_eq!(summary.retained_sources_checked, 9);
        assert_eq!(summary.statuses_checked, 9);
    }
}
