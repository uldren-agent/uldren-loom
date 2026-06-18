//! `loom` - the Uldren Loom command-line tool.
//!
//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Args, CommandFactory, Parser, Subcommand};
use futures::executor::block_on;
use gluesql_core::prelude::{Glue, Payload, Value as GValue};
use loom_codec::Value as WireValue;
use loom_core::keys::{EncryptionMeta, KeySpec, Suite};
use loom_core::search::AggregationRequest;
use loom_core::tabular::{CmpOp, ColumnType, cell_from, cell_value};
use loom_core::vector::{Hit, MetaFilter, Metric};
use loom_core::workspace::{FacetKind, WorkspaceId};
use loom_core::{
    AcceleratorPolicy, AclDomain, AclEffect, AclGrant, AclPredicate, AclRight, AclScope,
    AclScopeKind, AclStore, AclSubject, Algo, AppCredential, Bundle, Code, ColumnarAggregate,
    ColumnarAggregateOp, ColumnarInspect, Digest, Document, Edge, EmbeddingModel,
    EphemeralPutOptions, ExternalCredential, ExternalCredentialKind, FieldMapping, FieldType,
    FieldValue, IdentityRole, IdentityStore, KvMapConfig, KvTier, LiveRootDiagnostics,
    LockCoordinator, LockOwner, Loom, Mapping, MergeOutcome, Object, ObjectStore, Principal,
    PrincipalKind, Props, ProtectedRefPolicy, Query, QueryRequest, QueryResponse, VERSION,
    WsSelector, bundle_export, bundle_import, clone_workspace, inference_instance_state,
    migrate_workspace_profile, put_inference_instance_state, search_collections,
};
#[cfg(feature = "inference-native-hf")]
use loom_inference::DownloadEvent;
use loom_inference::{DownloadJobManager, DownloadJobPlan};
use loom_interchange::ArchiveKind;
#[cfg(all(test, feature = "integration-tests"))]
use loom_interchange::ImportReportInput;
use loom_interchange_io::{
    ArchiveExportOptions, ArchiveExportResult, ArchiveImportOptions, ArchiveImportResult,
    CarExportOptions, CarExportResult, CarImportOptions, CarImportResult, FsExportOptions,
    FsImportOptions, ResolvedImportInput, TableCsvExportOptions, TableCsvImportOptions,
    TableImportMode, export_archive, export_car, export_fs, export_table_csv, import_archive,
    import_car, import_fs, import_meetings_bytes, import_table_csv,
    load_meetings_snapshot as load_meetings_snapshot_io, meetings_source_payload_path,
    parse_meetings_input_profile, persist_import_checkpoint, retain_import_input,
    validate_meetings_source_payload_leaf,
};
use loom_lanes::{
    Lane, LaneDecodeDiagnostic, LaneInput, LaneKind, LaneStatus, LaneTicketView, LaneView,
};
use loom_sql::LoomSqlStore;
use loom_store::{
    AuditConfig, DerivedArtifactRebuild, DerivedArtifactRecord, DerivedArtifactStatus, FileStore,
    GcSegmentBudget, LocalOpenAuth, ServedListenerRecord, StoreMaintenanceReport,
    StoreMaintenanceRunState, StorePolicy, daemon, gc_loom, open_loom_read_unlocked, save_loom,
};
use loom_substrate::OperationEnvelope;
#[cfg(all(test, feature = "integration-tests"))]
use loom_substrate::body::BlockKind;
use loom_substrate::body::Body;
use loom_substrate::drive::{
    DriveOperationLog, DrivePolicyRegistry, DrivePolicyTarget, drive_operation_log_key,
    drive_policy_registry_key,
};
use loom_substrate::lifecycle::{LifecycleOperationLog, lifecycle_operation_log_key};
#[cfg(all(test, feature = "integration-tests"))]
use loom_substrate::meetings::PROFILE_CONTROL_PREFIX as MEETINGS_PROFILE_CONTROL_PREFIX;
use loom_substrate::meetings::{
    AnnotationRecord, AnnotationStatus, MeetingRecord, MeetingStatus, MeetingsProfileSnapshot,
    ProjectionAction, ProjectionKind, ProjectionOutput, ProjectionOutputSet, meetings_profile_key,
};
#[cfg(all(test, feature = "integration-tests"))]
use loom_substrate::meetings::{
    Coverage as MeetingsCoverage, InputProfile, MeetingRecordInput, MeetingsProfileSnapshotParts,
    SourceRecord, SourceRecordInput, SpanKind, SpanRecord,
};
use loom_substrate::pages::{PageOperationLog, page_profile_operation_log_key};
use loom_substrate::search::{
    EMBEDDING_PROJECTION_JOBS_DIR, EmbeddingProjectionJob, EmbeddingProjectionKey,
    EmbeddingProjectionStamp,
};
use loom_substrate::surfaces::{
    SurfaceAppDefinition, core_surface_catalog, meeting_memory_surface_catalog,
    surface_app_catalog, surface_catalog_json,
};
use loom_substrate::versioning::{
    BodyRef, REVISION_INDEX_DIR, RevisionBackfillUpdate, RevisionIndex, revision_index_path,
};
use loom_types::{
    InferenceModelKind, ModelFitReport, ModelRef, MutationChange, MutationEnvelope,
    MutationReceipt, RevisionRef, RuntimeKind,
};
use std::collections::BTreeMap;

mod audit_cmd;
mod certificate_cmd;
mod cli;
mod context_cmd;
mod daemon_cmd;
mod exec_cmd;
mod helpers;
mod locator_cx;
mod management_cmd;
mod network_access_cmd;
#[cfg(feature = "mcp")]
mod refs_cmd;
mod remote;
mod serve_cmd;
mod table_cmd;
mod tls_crypto;
pub(crate) use audit_cmd::*;
pub(crate) use certificate_cmd::*;
pub(crate) use cli::*;
pub(crate) use context_cmd::*;
pub(crate) use daemon_cmd::*;
pub(crate) use exec_cmd::*;
pub(crate) use helpers::*;
pub(crate) use management_cmd::*;
pub(crate) use network_access_cmd::*;
#[cfg(feature = "mcp")]
pub(crate) use refs_cmd::*;
pub(crate) use serve_cmd::*;
pub(crate) use table_cmd::*;

#[derive(Parser)]
#[command(
    name = "loom",
    version,
    about = "Uldren Loom - an encrypted, versioned, multi-model data engine in a single file",
    long_about = "Uldren Loom - an encrypted, versioned, multi-model data engine in a single file.\n\n\
STORE forms: a `.loom` path or a `file://` URL open a local store; an `https://` URL opens a remote \
endpoint. A first-class context from `contexts.toml` owns a local or remote target plus optional \
workspace, auth, TLS, discovery, and timeout defaults. `--context` selects that context for commands \
that use the `context` store locator; explicit command selectors override context defaults. Context \
config precedence, highest first: each `--config` file (in command-line order), \
`<project>/.loom/contexts.toml`, `~/.loom/contexts.toml`, then `/etc/loom/contexts.toml`; `--project` \
sets the project root (default: the working directory). Remote \
endpoints fail fast on discovery, TLS trust, auth, network-access, or protocol-version errors and never \
queue commands for later replay. `loom mcp` against a local store serves the full tool surface; against \
a remote locator it serves the KV, CAS, Queue, Ledger, TimeSeries, full-text search, columnar, calendar, \
contacts, mail, filesystem, and vector tool families (plus document reads, VCS reads + non-timestamped writes, and graph reads + node writes) \
over the remote Loom while document/graph ref-index (edge) writes, the timestamped VCS writes, and other tools return a clear not-yet/local-only error, and \
`--stateless` applies only to a local MCP host.",
    propagate_version = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Where the encryption passphrase comes from: `prompt` (default, secure no-echo TTY),
    /// `file:<path>`, or `fd:<n>`. Environment variables are never a key source.
    #[arg(
        long,
        global = true,
        default_value = "prompt",
        help_heading = "Key Options"
    )]
    key_source: String,
    /// Principal UUID to authenticate this command as.
    #[arg(long, global = true, help_heading = "Authentication Options")]
    auth_principal: Option<String>,
    /// Key source for the principal passphrase: `prompt`, `file:<path>`, or `fd:<n>`.
    #[arg(
        long,
        global = true,
        default_value = "prompt",
        help_heading = "Authentication Options"
    )]
    auth_key_source: String,
    /// Project root whose `.loom/contexts.toml` layer is used when resolving contexts.
    /// Defaults to the working directory. Valid before or after the subcommand.
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help_heading = "Locator Options"
    )]
    project: Option<PathBuf>,
    /// Additional context-config TOML file, highest precedence. Repeatable; later files override earlier
    /// ones. Valid before or after the subcommand.
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help_heading = "Locator Options"
    )]
    config: Vec<PathBuf>,
    /// First-class CLI context name. Valid before or after the subcommand.
    #[arg(
        long,
        global = true,
        value_name = "NAME",
        help_heading = "Locator Options"
    )]
    context: Option<String>,
}

/// Where a passphrase is read from. `prompt` is the primary, secure path; `file:`/`fd:` are
/// the non-interactive automation paths. Environment variables are intentionally not a source.
#[derive(Clone)]
enum KeySource {
    /// Interactive no-echo prompt on the controlling terminal.
    Prompt,
    /// Read a passphrase from a file (trailing newline trimmed).
    File(String),
    /// Read a passphrase from an inherited file descriptor (unix).
    Fd(i32),
    /// Read a raw 256-bit KEK as 64 hex chars from a file. The host computed the KEK from an external
    /// provider (keychain/Secure Enclave/passkey PRF/KMS); the CLI uses it to wrap/unwrap the DEK
    /// directly, with no passphrase KDF. Advanced/testing.
    RawKekFile(String),
    /// Read a raw 256-bit KEK as 64 hex chars from an inherited file descriptor.
    RawKekFd(i32),
}

/// The resolved key sources for one CLI invocation: the current/unlock passphrase and (for `rekey`) the
/// new one.
struct KeyOpts {
    source: KeySource,
    new_source: KeySource,
    auth_principal: Option<String>,
    auth_source: KeySource,
}

impl Default for KeyOpts {
    fn default() -> Self {
        Self {
            source: KeySource::Prompt,
            new_source: KeySource::Prompt,
            auth_principal: None,
            auth_source: KeySource::Prompt,
        }
    }
}

/// Parse the `--key-source` grammar: `prompt` | `file:<path>` | `fd:<n>` |
/// `raw-kek:file:<path>` | `raw-kek:fd:<n>`.
fn parse_key_source(s: &str) -> Result<KeySource, String> {
    let parse_fd = |n: &str, make: fn(i32) -> KeySource, what: &str| {
        n.parse().map(make).map_err(|_| {
            format!("invalid --key-source {what}: {n:?} is not a file descriptor number")
        })
    };
    if s == "prompt" {
        Ok(KeySource::Prompt)
    } else if let Some(rest) = s.strip_prefix("raw-kek:") {
        if let Some(path) = rest.strip_prefix("file:") {
            Ok(KeySource::RawKekFile(path.to_string()))
        } else if let Some(n) = rest.strip_prefix("fd:") {
            parse_fd(n, KeySource::RawKekFd, "raw-kek:fd")
        } else {
            Err(format!(
                "unknown raw-kek source {s:?} (expected `raw-kek:file:<path>` or `raw-kek:fd:<n>`)"
            ))
        }
    } else if let Some(path) = s.strip_prefix("file:") {
        Ok(KeySource::File(path.to_string()))
    } else if let Some(n) = s.strip_prefix("fd:") {
        parse_fd(n, KeySource::Fd, "fd")
    } else {
        Err(format!(
            "unknown key source {s:?} (expected `prompt`, `file:<path>`, `fd:<n>`, `raw-kek:file:<path>`, or `raw-kek:fd:<n>`)"
        ))
    }
}

/// Resolve a per-command `--new-key-source` argument. When absent, fall back to the ambient
/// [`KeyOpts::new_source`] (tests construct `KeyOpts` directly; the CLI default is `prompt`).
fn resolve_new_key_source(arg: Option<&str>, keys: &KeyOpts) -> Result<KeySource, String> {
    match arg {
        Some(value) => parse_key_source(value),
        None => Ok(keys.new_source.clone()),
    }
}

/// Resolve a key source to a [`KeySpec`]: a passphrase (`prompt`/`file:`/`fd:`) or a raw 256-bit
/// KEK (`raw-kek:file:`/`raw-kek:fd:`). `confirm` is honored only for the interactive prompt.
fn acquire_key_spec(src: &KeySource, label: &str, confirm: bool) -> Result<KeySpec, String> {
    match src {
        KeySource::Prompt | KeySource::File(_) | KeySource::Fd(_) => {
            Ok(KeySpec::passphrase(acquire(src, label, confirm)?))
        }
        KeySource::RawKekFile(path) => {
            let raw = std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?;
            Ok(KeySpec::raw_kek(parse_hex_kek(raw.trim())?))
        }
        KeySource::RawKekFd(n) => {
            let raw = read_fd_string(*n)?;
            Ok(KeySpec::raw_kek(parse_hex_kek(raw.trim())?))
        }
    }
}

/// Decode a 256-bit KEK from exactly 64 lowercase/uppercase hex characters.
fn parse_hex_kek(hex: &str) -> Result<[u8; 32], String> {
    if hex.len() != 64 {
        return Err(format!(
            "raw KEK must be 64 hex chars (256 bits), got {} chars",
            hex.len()
        ));
    }
    let mut kek = [0u8; 32];
    for (i, byte) in kek.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| "raw KEK is not valid hex".to_string())?;
    }
    Ok(kek)
}

/// Acquire a passphrase from `src`. `confirm` (honored only for the interactive prompt) re-prompts and
/// requires a match - used when creating a passphrase (`init`, `rekey`'s new passphrase) so a typo can't
/// permanently lock an immutable-at-creation store. Empty passphrases are rejected.
fn acquire(src: &KeySource, label: &str, confirm: bool) -> Result<String, String> {
    match src {
        KeySource::Prompt => {
            use std::io::IsTerminal;
            if !std::io::stdin().is_terminal() {
                return Err(format!(
                    "{label}: no terminal for an interactive passphrase; use --key-source file:<path> or fd:<n>"
                ));
            }
            let pass =
                rpassword::prompt_password(format!("{label}: ")).map_err(|e| e.to_string())?;
            if pass.is_empty() {
                return Err(format!("{label}: empty passphrase"));
            }
            if confirm {
                let again = rpassword::prompt_password(format!("Confirm {label}: "))
                    .map_err(|e| e.to_string())?;
                if again != pass {
                    return Err("passphrases do not match".to_string());
                }
            }
            Ok(pass)
        }
        KeySource::File(path) => {
            let raw = std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?;
            let pass = raw.trim_end_matches(['\n', '\r']).to_string();
            if pass.is_empty() {
                return Err(format!("passphrase file {path} is empty"));
            }
            Ok(pass)
        }
        KeySource::Fd(n) => read_fd_passphrase(*n),
        KeySource::RawKekFile(_) | KeySource::RawKekFd(_) => Err(format!(
            "{label}: a raw KEK is a key, not a passphrase (use acquire_key_spec)"
        )),
    }
}

/// Read a passphrase from an inherited file descriptor: the secure-pipe pattern that keeps the secret
/// out of `argv`, the environment, and disk (`printf '%s' "$pw" | loom ... --key-source fd:0`). v1
/// supports `fd:0` (standard input), read with safe std I/O. Wrapping an arbitrary fd number requires
/// `unsafe` (`FromRawFd`), which the workspace forbids in this crate.
fn read_fd_passphrase(fd: i32) -> Result<String, String> {
    let pass = read_fd_string(fd)?
        .trim_end_matches(['\n', '\r'])
        .to_string();
    if pass.is_empty() {
        return Err("fd:0 (stdin) provided an empty passphrase".to_string());
    }
    Ok(pass)
}

/// Read the full contents of an inherited file descriptor (v1: only `fd:0`/stdin, safe std I/O). Shared
/// by the passphrase (`fd:`) and raw-KEK (`raw-kek:fd:`) sources.
fn read_fd_string(fd: i32) -> Result<String, String> {
    if fd != 0 {
        return Err(format!(
            "--key-source fd:{fd}: only fd:0 (standard input) is supported in v1; pipe the value \
             to stdin, or use file:<path> / prompt"
        ));
    }
    let mut raw = String::new();
    std::io::stdin()
        .read_to_string(&mut raw)
        .map_err(|e| format!("read from stdin (fd:0): {e}"))?;
    Ok(raw)
}

fn main() -> std::process::ExitCode {
    if let Some(code) = display_exit_code() {
        return code;
    }
    std::process::ExitCode::from(real_main() as u8)
}

fn display_exit_code() -> Option<std::process::ExitCode> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("loom {VERSION}");
        return Some(std::process::ExitCode::SUCCESS);
    }
    if args.iter().any(|arg| arg == "--llms-full") {
        print_llms_reference(true);
        return Some(std::process::ExitCode::SUCCESS);
    }
    if args.iter().any(|arg| arg == "--llms") || args.first().map(String::as_str) == Some("llms") {
        print_llms_reference(false);
        return Some(std::process::ExitCode::SUCCESS);
    }
    // `loom <path...> --help` and `loom help <path...>` render the same help tree.
    let (skip, end) = if args.first().map(String::as_str) == Some("help") {
        (1, args.len())
    } else {
        let help_at = args.iter().position(|arg| arg == "--help" || arg == "-h")?;
        (0, help_at)
    };
    let mut command = Cli::command();
    let path = args[skip..end]
        .iter()
        .filter(|arg| !arg.starts_with('-'))
        .map(String::as_str)
        .collect::<Vec<_>>();
    print_help_for_path(&mut command, &path);
    Some(std::process::ExitCode::SUCCESS)
}

fn print_help_for_path(command: &mut clap::Command, path: &[&str]) {
    if let Some((head, tail)) = path.split_first()
        && let Some(subcommand) = command.find_subcommand_mut(head)
    {
        print_help_for_path(subcommand, tail);
        return;
    }
    if command.get_name() == "loom" {
        print_root_help(command);
        return;
    }
    let _ = command.print_help();
    println!();
}

/// The sectioned layout of the top-level help.
const COMMAND_SECTIONS: &[(&str, &[&str])] = &[
    (
        "Data Facets",
        &[
            "cas",
            "capabilities",
            "columnar",
            "dataframe",
            "document",
            "files",
            "fts",
            "graph",
            "kv",
            "ledger",
            "logs",
            "metrics",
            "queue",
            "search",
            "sql",
            "time-series",
            "traces",
            "vector",
        ],
    ),
    ("PIM Facets", &["calendar", "contacts", "mail"]),
    (
        "Studio",
        &[
            "chat", "drive", "lanes", "meetings", "pages", "studio", "tickets",
        ],
    ),
    ("Compute", &["exec", "inference", "program", "lock"]),
    ("Versioning", &["refs", "vcs"]),
    ("Bindings", &["daemon", "mcp", "mount", "serve"]),
    (
        "Security",
        &["acl", "audit", "certificate", "identity", "network-access"],
    ),
    (
        "Management",
        &["context", "workspace", "protected-ref", "store"],
    ),
    ("Integrations", &["interchange"]),
    (
        "General",
        &["doctor", "lifecycle", "llms", "version", "help"],
    ),
];

/// Render the grouped top-level help. clap cannot section subcommands, so the root help is
/// rendered by hand from the clap metadata; every deeper level stays clap-rendered.
fn print_root_help(command: &mut clap::Command) {
    // Shallow build: adds the auto `help` subcommand and `-h`/`-V` args without recursively
    // building (and debug-asserting) every subtree.
    let _ = command.render_usage();
    if let Some(about) = command.get_about() {
        println!("{about}");
        println!();
    }
    println!("Usage: loom [OPTIONS] <COMMAND>");
    let width = command
        .get_subcommands()
        .map(|sub| root_help_entry_name(sub).len())
        .max()
        .unwrap_or(0)
        + 2;
    for (title, names) in COMMAND_SECTIONS {
        println!();
        println!("{title}:");
        for name in *names {
            if let Some(sub) = command.find_subcommand(name) {
                print_root_help_entry(sub, width);
            }
        }
    }
    let sectioned = COMMAND_SECTIONS
        .iter()
        .flat_map(|(_, names)| names.iter().copied())
        .collect::<std::collections::BTreeSet<_>>();
    let other = command
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set() && !sectioned.contains(sub.get_name()))
        .map(|sub| sub.get_name().to_string())
        .collect::<Vec<_>>();
    if !other.is_empty() {
        println!();
        println!("Other:");
        for name in &other {
            if let Some(sub) = command.find_subcommand(name) {
                print_root_help_entry(sub, width);
            }
        }
    }
    print_root_help_options(command, width);
    println!();
    println!(
        "Run `loom <command> --help` for details on a command, `loom --llms` for the usage \
         reference, or `loom --llms-full` to add the argument and option glossaries."
    );
}

fn root_help_entry_name(sub: &clap::Command) -> String {
    let mut name = sub.get_name().to_string();
    for alias in sub.get_visible_aliases() {
        name.push_str(", ");
        name.push_str(alias);
    }
    name
}

fn print_root_help_entry(sub: &clap::Command, width: usize) {
    let about = sub.get_about().map(ToString::to_string).unwrap_or_default();
    let name = root_help_entry_name(sub);
    println!("  {name:<width$}{about}");
}

/// Render the top-level options grouped by their clap `help_heading` (unheaded args land in
/// the plain `Options` section).
fn print_root_help_options(command: &clap::Command, width: usize) {
    let mut sections: Vec<(&str, Vec<(String, String)>)> = vec![("Options", Vec::new())];
    for arg in command.get_arguments() {
        if arg.is_hide_set() {
            continue;
        }
        let mut left = match (arg.get_short(), arg.get_long()) {
            (Some(short), Some(long)) => format!("-{short}, --{long}"),
            (None, Some(long)) => format!("    --{long}"),
            (Some(short), None) => format!("-{short}"),
            (None, None) => continue,
        };
        if matches!(
            arg.get_action(),
            clap::ArgAction::Set | clap::ArgAction::Append
        ) {
            let value = arg
                .get_value_names()
                .and_then(|names| names.first().map(ToString::to_string))
                .unwrap_or_else(|| arg.get_id().to_string().to_uppercase().replace('-', "_"));
            left.push_str(&format!(" <{value}>"));
        }
        let help = arg
            .get_help()
            .map(ToString::to_string)
            .unwrap_or_default()
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        let heading = arg.get_help_heading().unwrap_or("Options");
        match sections.iter_mut().find(|(title, _)| *title == heading) {
            Some((_, entries)) => entries.push((left, help)),
            None => sections.push((heading, vec![(left, help)])),
        }
    }
    for (title, entries) in &sections {
        if entries.is_empty() {
            continue;
        }
        println!();
        println!("{title}:");
        let left_width = entries
            .iter()
            .map(|(left, _)| left.len())
            .max()
            .unwrap_or(0)
            .max(width)
            + 2;
        for (left, help) in entries {
            println!("  {left:<left_width$}{help}");
        }
    }
}

/// Print the command reference for LLM contexts: one usage line per leaf command, showing
/// every parameter position (`loom --llms` / `loom llms`). With `full` (`loom --llms-full`),
/// also print the global options and consolidated, alphabetized glossaries for arguments
/// and options.
fn print_llms_reference(full: bool) {
    let mut command = Cli::command();
    let _ = command.render_usage(); // shallow build
    if let Some(about) = command.get_about() {
        println!("{about}");
        println!();
    }
    if full {
        println!("Global options (accepted by every command):");
        for arg in command.get_arguments() {
            if matches!(arg.get_id().as_str(), "help" | "version") || arg.is_hide_set() {
                continue;
            }
            if let Some(long) = arg.get_long() {
                let help = llms_arg_help(arg);
                println!("  --{long} <{}>  {help}", llms_value_name(arg));
            }
        }
        println!();
    }
    println!("Commands:");
    let mut arguments = BTreeMap::new();
    let mut options = BTreeMap::new();
    for name in visible_subcommand_names(&command) {
        let sub = command
            .find_subcommand(&name)
            .expect("visible subcommand exists")
            .clone();
        println!();
        let mut header = format!("# {}", root_help_entry_name(&sub));
        if let Some(about) = sub.get_about() {
            header.push_str(&format!(" - {about}"));
        }
        println!("{header}");
        collect_llms_usage(sub, &format!("loom {name}"), &mut arguments, &mut options);
    }
    if !full {
        return;
    }
    println!();
    println!("Arguments (consolidated; a placeholder may mean different things per command):");
    print_llms_glossary(&arguments);
    println!();
    println!("Options (consolidated; global options and `--help`/`--version` omitted):");
    print_llms_glossary(&options);
}

/// Walk to the leaves, print one usage line per leaf, and record every argument and option
/// into the consolidated glossaries.
fn collect_llms_usage(
    command: clap::Command,
    path: &str,
    arguments: &mut BTreeMap<String, std::collections::BTreeSet<String>>,
    options: &mut BTreeMap<String, std::collections::BTreeSet<String>>,
) {
    let mut command = command.bin_name(path.to_string());
    let children = visible_subcommand_names(&command);
    if !children.is_empty() {
        for name in children {
            let sub = command
                .find_subcommand(&name)
                .expect("visible subcommand exists")
                .clone();
            collect_llms_usage(sub, &format!("{path} {name}"), arguments, options);
        }
        return;
    }
    let usage = command.render_usage().to_string();
    let mut line = usage.trim_start_matches("Usage:").trim().to_string();
    let aliases = command
        .get_visible_aliases()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if !aliases.is_empty() {
        line.push_str(&format!("  (alias: {})", aliases.join(", ")));
    }
    println!("{line}");
    for arg in command.get_arguments() {
        if matches!(arg.get_id().as_str(), "help" | "version")
            || arg.is_global_set()
            || arg.is_hide_set()
        {
            continue;
        }
        let (key, glossary) = if arg.is_positional() {
            // Bare name: the usage lines show `<NAME>` (required) or `[NAME]` (optional),
            // so the glossary key is bracket-agnostic.
            (llms_value_name(arg), &mut *arguments)
        } else if let Some(long) = arg.get_long() {
            let key = if matches!(
                arg.get_action(),
                clap::ArgAction::Set | clap::ArgAction::Append
            ) {
                format!("--{long} <{}>", llms_value_name(arg))
            } else {
                format!("--{long}")
            };
            (key, &mut *options)
        } else {
            continue;
        };
        let entry = glossary.entry(key).or_default();
        let help = llms_arg_help(arg);
        if !help.is_empty() {
            entry.insert(help);
        }
    }
}

fn print_llms_glossary(entries: &BTreeMap<String, std::collections::BTreeSet<String>>) {
    let width = entries.keys().map(String::len).max().unwrap_or(0) + 2;
    for (key, helps) in entries {
        match helps.len() {
            0 => println!("  {key}"),
            1 => println!("  {key:<width$}{}", helps.first().expect("one entry")),
            _ => {
                println!("  {key}");
                for help in helps {
                    println!("    - {help}");
                }
            }
        }
    }
}

/// First help line of an arg, or empty when undocumented.
fn llms_arg_help(arg: &clap::Arg) -> String {
    arg.get_help()
        .map(ToString::to_string)
        .unwrap_or_default()
        .lines()
        .next()
        .unwrap_or("")
        .to_string()
}

fn llms_value_name(arg: &clap::Arg) -> String {
    arg.get_value_names()
        .and_then(|names| names.first().map(ToString::to_string))
        .unwrap_or_else(|| arg.get_id().to_string().to_uppercase().replace('-', "_"))
}

fn visible_subcommand_names(command: &clap::Command) -> Vec<String> {
    let mut names = command
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set() && sub.get_name() != "help")
        .map(|sub| sub.get_name().to_string())
        .collect::<Vec<_>>();
    names.sort_unstable();
    names
}

#[cfg(test)]
fn cli_command_for_test() -> clap::Command {
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(Cli::command)
        .expect("spawn clap command builder")
        .join()
        .expect("build clap command")
}

#[cfg(test)]
fn cli_try_parse_for_test<const N: usize>(args: [&'static str; N]) -> Result<Cli, clap::Error> {
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(move || Cli::try_parse_from(args))
        .expect("spawn clap parser")
        .join()
        .expect("parse cli")
}

fn real_main() -> i32 {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            let code = if err.use_stderr() { 1 } else { 0 };
            let _ = err.print();
            return code;
        }
    };
    match locator_cx::LocatorContext::from_globals(cli.project, cli.config, cli.context) {
        Ok(context) => locator_cx::install(context),
        Err(err) => {
            eprintln!("error: {err}");
            return 1;
        }
    }
    let command = match cli.command {
        Some(command) => command,
        None => {
            print_root_help(&mut Cli::command());
            return 1;
        }
    };
    let keys = match (
        parse_key_source(&cli.key_source),
        parse_key_source(&cli.auth_key_source),
    ) {
        (Ok(source), Ok(auth_source)) => KeyOpts {
            source,
            new_source: KeySource::Prompt,
            auth_principal: cli.auth_principal,
            auth_source,
        },
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    match run(command, &keys) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("error: {err}");
            1
        }
    }
}

fn parse_vector_metric(value: &str) -> Result<Metric, String> {
    match value {
        "cosine" | "1" => Ok(Metric::Cosine),
        "l2" | "2" => Ok(Metric::L2),
        "dot" | "3" => Ok(Metric::Dot),
        other => Err(format!(
            "unknown vector metric {other:?} (expected cosine, l2, or dot)"
        )),
    }
}

fn vector_floats_from_bytes(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if !bytes.len().is_multiple_of(4) {
        return Err("vector bytes length must be a multiple of 4".to_string());
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

fn vector_floats_to_bytes(vector: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn wire_cell_from(value: WireValue) -> Result<loom_core::Value, String> {
    let bytes = loom_codec::encode(&value).map_err(|e| e.to_string())?;
    loom_core::key_from_cbor(&bytes).map_err(|e| e.to_string())
}

fn wire_cell_value(value: &loom_core::Value) -> Result<WireValue, String> {
    let bytes = loom_core::key_to_cbor(value);
    loom_codec::decode(&bytes).map_err(|e| e.to_string())
}

fn vector_metadata_from_cbor(bytes: &[u8]) -> Result<BTreeMap<String, loom_core::Value>, String> {
    if bytes.is_empty() {
        return Ok(BTreeMap::new());
    }
    let WireValue::Map(pairs) = loom_codec::decode(bytes).map_err(|e| e.to_string())? else {
        return Err("vector metadata must be a CBOR map".to_string());
    };
    let mut out = BTreeMap::new();
    for (key, value) in pairs {
        let WireValue::Text(key) = key else {
            return Err("vector metadata keys must be text".to_string());
        };
        out.insert(key, wire_cell_from(value)?);
    }
    Ok(out)
}

fn vector_metadata_value(
    metadata: &BTreeMap<String, loom_core::Value>,
) -> Result<WireValue, String> {
    let pairs = metadata
        .iter()
        .map(|(key, value)| Ok((WireValue::Text(key.clone()), wire_cell_value(value)?)))
        .collect::<Result<Vec<_>, String>>()?;
    Ok(WireValue::Map(pairs))
}

fn render_structural_diff_text(bytes: &[u8]) -> Result<String, String> {
    let value = loom_codec::decode(bytes).map_err(|e| e.to_string())?;
    let WireValue::Array(frame) = value else {
        return Err("structural diff must be a CBOR array".to_string());
    };
    if frame.len() != 6 || !matches!(&frame[0], WireValue::Text(tag) if tag == "LMDIFF") {
        return Err("structural diff has an unknown frame".to_string());
    }
    let WireValue::Array(facets) = &frame[5] else {
        return Err("structural diff facets must be an array".to_string());
    };
    let mut out = String::new();
    if facets.is_empty() {
        out.push_str("no changes\n");
        return Ok(out);
    }
    for facet_value in facets {
        let WireValue::Array(facet_section) = facet_value else {
            return Err("facet section must be an array".to_string());
        };
        if facet_section.len() != 2 {
            return Err("facet section must have 2 fields".to_string());
        }
        let WireValue::Text(facet) = &facet_section[0] else {
            return Err("facet name must be text".to_string());
        };
        let WireValue::Array(collections) = &facet_section[1] else {
            return Err("collection sections must be an array".to_string());
        };
        for collection_value in collections {
            let WireValue::Array(collection) = collection_value else {
                return Err("collection section must be an array".to_string());
            };
            if collection.len() != 3 {
                return Err("collection section must have 3 fields".to_string());
            }
            let path = render_diff_collection_path(&collection[0])?;
            let summary = render_diff_summary(&collection[1])?;
            if path.is_empty() {
                out.push_str(&format!("{facet}: {summary}\n"));
            } else {
                out.push_str(&format!("{facet}.{path}: {summary}\n"));
            }
            let WireValue::Array(units) = &collection[2] else {
                return Err("unit changes must be an array".to_string());
            };
            for unit in units {
                out.push_str("  ");
                out.push_str(&render_diff_unit(unit)?);
                out.push('\n');
            }
        }
    }
    Ok(out)
}

fn render_diff_collection_path(value: &WireValue) -> Result<String, String> {
    let WireValue::Array(parts) = value else {
        return Err("collection path must be an array".to_string());
    };
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        let WireValue::Text(part) = part else {
            return Err("collection path segment must be text".to_string());
        };
        out.push(part.clone());
    }
    Ok(out.join("."))
}

fn render_diff_summary(value: &WireValue) -> Result<String, String> {
    let WireValue::Array(summary) = value else {
        return Err("diff summary must be an array".to_string());
    };
    if summary.len() != 5 {
        return Err("diff summary must have 5 fields".to_string());
    }
    let added = diff_u64(&summary[0], "added")?;
    let removed = diff_u64(&summary[1], "removed")?;
    let changed = diff_u64(&summary[2], "changed")?;
    let appended = diff_u64(&summary[3], "appended")?;
    let WireValue::Bool(coarse) = summary[4] else {
        return Err("diff summary coarse flag must be bool".to_string());
    };
    let mut parts = Vec::new();
    if added > 0 {
        parts.push(format!("{added} added"));
    }
    if removed > 0 {
        parts.push(format!("{removed} removed"));
    }
    if changed > 0 {
        parts.push(format!("{changed} changed"));
    }
    if appended > 0 {
        parts.push(format!("{appended} appended"));
    }
    if parts.is_empty() {
        parts.push("0 changes".to_string());
    }
    if coarse {
        parts.push("coarse".to_string());
    }
    Ok(parts.join(", "))
}

fn render_diff_unit(value: &WireValue) -> Result<String, String> {
    let WireValue::Array(unit) = value else {
        return Err("unit change must be an array".to_string());
    };
    if unit.len() != 7 {
        return Err("unit change must have 7 fields".to_string());
    }
    let WireValue::Text(kind) = &unit[0] else {
        return Err("unit kind must be text".to_string());
    };
    let WireValue::Bytes(key) = &unit[1] else {
        return Err("unit key must be bytes".to_string());
    };
    let WireValue::Text(change) = &unit[2] else {
        return Err("unit change must be text".to_string());
    };
    let rendered_key = loom_codec::decode(key)
        .map(render_diff_key)
        .unwrap_or_else(|_| format!("0x{}", hex_bytes(key)));
    Ok(format!("{change} {kind} {rendered_key}"))
}

fn render_diff_key(value: WireValue) -> String {
    match value {
        WireValue::Uint(v) => v.to_string(),
        WireValue::Nint(v) => format!("-{}", v + 1),
        WireValue::Text(v) => v,
        WireValue::Bytes(v) => format!("0x{}", hex_bytes(&v)),
        WireValue::Bool(v) => v.to_string(),
        WireValue::Null => "null".to_string(),
        WireValue::Array(items) => {
            let parts = items.into_iter().map(render_diff_key).collect::<Vec<_>>();
            format!("[{}]", parts.join(","))
        }
        WireValue::Map(_) => "<map>".to_string(),
        WireValue::Float(v) => v.to_string(),
    }
}

fn diff_u64(value: &WireValue, field: &str) -> Result<u64, String> {
    match value {
        WireValue::Uint(v) => Ok(*v),
        _ => Err(format!("diff summary {field} count must be uint")),
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn vector_filter_from_value(value: WireValue) -> Result<MetaFilter, String> {
    let WireValue::Array(items) = value else {
        return Err("vector filter must be a CBOR array".to_string());
    };
    let mut iter = items.into_iter();
    let tag = match iter.next() {
        Some(WireValue::Uint(tag)) => tag,
        _ => return Err("vector filter tag must be a uint".to_string()),
    };
    match tag {
        0 => Ok(MetaFilter::All),
        1 => {
            let key = vector_filter_key(&mut iter, "Eq")?;
            let value = vector_filter_cell(&mut iter, "Eq")?;
            Ok(MetaFilter::Eq(key, wire_cell_from(value)?))
        }
        2 => {
            let (left, right) = vector_filter_operands(&mut iter, "And")?;
            Ok(MetaFilter::And(
                Box::new(vector_filter_from_value(left)?),
                Box::new(vector_filter_from_value(right)?),
            ))
        }
        3 => {
            let (left, right) = vector_filter_operands(&mut iter, "Or")?;
            Ok(MetaFilter::Or(
                Box::new(vector_filter_from_value(left)?),
                Box::new(vector_filter_from_value(right)?),
            ))
        }
        4 => {
            let inner = iter
                .next()
                .ok_or_else(|| "vector filter Not is missing its operand".to_string())?;
            Ok(MetaFilter::Not(Box::new(vector_filter_from_value(inner)?)))
        }
        5 => {
            let key = vector_filter_key(&mut iter, "Exists")?;
            Ok(MetaFilter::Exists(key))
        }
        6 => {
            let key = vector_filter_key(&mut iter, "Ne")?;
            let value = vector_filter_cell(&mut iter, "Ne")?;
            Ok(MetaFilter::Ne(key, wire_cell_from(value)?))
        }
        7 => {
            let key = vector_filter_key(&mut iter, "Lt")?;
            let value = vector_filter_cell(&mut iter, "Lt")?;
            Ok(MetaFilter::Lt(key, wire_cell_from(value)?))
        }
        8 => {
            let key = vector_filter_key(&mut iter, "Le")?;
            let value = vector_filter_cell(&mut iter, "Le")?;
            Ok(MetaFilter::Le(key, wire_cell_from(value)?))
        }
        9 => {
            let key = vector_filter_key(&mut iter, "Gt")?;
            let value = vector_filter_cell(&mut iter, "Gt")?;
            Ok(MetaFilter::Gt(key, wire_cell_from(value)?))
        }
        10 => {
            let key = vector_filter_key(&mut iter, "Ge")?;
            let value = vector_filter_cell(&mut iter, "Ge")?;
            Ok(MetaFilter::Ge(key, wire_cell_from(value)?))
        }
        11 => {
            let key = vector_filter_key(&mut iter, "In")?;
            let values = match iter.next() {
                Some(WireValue::Array(values)) => values
                    .into_iter()
                    .map(wire_cell_from)
                    .collect::<Result<Vec<_>, _>>()?,
                _ => return Err("vector filter In values must be an array".to_string()),
            };
            Ok(MetaFilter::In(key, values))
        }
        other => Err(format!("unknown vector filter tag {other}")),
    }
}

fn vector_filter_key<I>(iter: &mut I, name: &str) -> Result<String, String>
where
    I: Iterator<Item = WireValue>,
{
    match iter.next() {
        Some(WireValue::Text(key)) => Ok(key),
        _ => Err(format!("vector filter {name} key must be text")),
    }
}

fn vector_filter_cell<I>(iter: &mut I, name: &str) -> Result<WireValue, String>
where
    I: Iterator<Item = WireValue>,
{
    iter.next()
        .ok_or_else(|| format!("vector filter {name} is missing its value"))
}

fn vector_filter_operands<I>(iter: &mut I, name: &str) -> Result<(WireValue, WireValue), String>
where
    I: Iterator<Item = WireValue>,
{
    let left = iter
        .next()
        .ok_or_else(|| format!("vector filter {name} is missing its left operand"))?;
    let right = iter
        .next()
        .ok_or_else(|| format!("vector filter {name} is missing its right operand"))?;
    Ok((left, right))
}

fn vector_filter_from_cbor(bytes: &[u8]) -> Result<MetaFilter, String> {
    if bytes.is_empty() {
        return Ok(MetaFilter::All);
    }
    let value = loom_codec::decode(bytes).map_err(|e| e.to_string())?;
    vector_filter_from_value(value)
}

fn vector_get_cbor(
    vector: Vec<f32>,
    metadata: BTreeMap<String, loom_core::Value>,
) -> Result<Vec<u8>, String> {
    loom_codec::encode(&WireValue::Array(vec![
        WireValue::Bytes(vector_floats_to_bytes(&vector)),
        vector_metadata_value(&metadata)?,
    ]))
    .map_err(|e| e.to_string())
}

fn vector_ids_cbor(ids: &[String]) -> Result<Vec<u8>, String> {
    loom_codec::encode(&WireValue::Array(
        ids.iter().cloned().map(WireValue::Text).collect(),
    ))
    .map_err(|e| e.to_string())
}

fn vector_hits_cbor(hits: &[Hit]) -> Result<Vec<u8>, String> {
    let items = hits
        .iter()
        .map(|hit| {
            Ok(WireValue::Array(vec![
                WireValue::Text(hit.id.clone()),
                wire_cell_value(&loom_core::Value::F32(hit.score))?,
            ]))
        })
        .collect::<Result<Vec<_>, String>>()?;
    loom_codec::encode(&WireValue::Array(items)).map_err(|e| e.to_string())
}

fn bytes_array_cbor(items: &[Vec<u8>]) -> Result<Vec<u8>, String> {
    loom_codec::encode(&WireValue::Array(
        items
            .iter()
            .map(|item| WireValue::Bytes(item.clone()))
            .collect(),
    ))
    .map_err(|e| e.to_string())
}

fn record_array_cbor(items: impl IntoIterator<Item = Vec<u8>>) -> Result<Vec<u8>, String> {
    let records = items
        .into_iter()
        .map(|bytes| loom_codec::decode(&bytes).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    loom_codec::encode(&WireValue::Array(records)).map_err(|e| e.to_string())
}

fn text_array_cbor(items: &[String]) -> Result<Vec<u8>, String> {
    loom_codec::encode(&WireValue::Array(
        items.iter().cloned().map(WireValue::Text).collect(),
    ))
    .map_err(|e| e.to_string())
}

fn metadata_cbor(display_name: &str) -> Result<Vec<u8>, String> {
    loom_codec::encode(&WireValue::Map(vec![(
        WireValue::Text("display_name".into()),
        WireValue::Text(display_name.to_string()),
    )]))
    .map_err(|e| e.to_string())
}

fn calendar_collection_cbor(meta: &loom_core::calendar::CollectionMeta) -> Result<Vec<u8>, String> {
    loom_codec::encode(&WireValue::Map(vec![
        (
            WireValue::Text("display_name".into()),
            WireValue::Text(meta.display_name.clone()),
        ),
        (
            WireValue::Text("component_set".into()),
            WireValue::Array(
                meta.component_set
                    .iter()
                    .map(|component| WireValue::Text(component.as_str().into()))
                    .collect(),
            ),
        ),
    ]))
    .map_err(|e| e.to_string())
}

fn parse_calendar_component(value: &str) -> Result<loom_core::calendar::Component, String> {
    match value {
        "event" => Ok(loom_core::calendar::Component::Event),
        "todo" => Ok(loom_core::calendar::Component::Todo),
        other => Err(format!(
            "unknown calendar component {other:?} (expected event or todo)"
        )),
    }
}

fn parse_calendar_datetime(value: &str) -> Result<loom_core::calendar::DateTime, String> {
    let value = value.strip_suffix('Z').unwrap_or(value);
    let (date, time) = match value.split_once('T') {
        Some((date, time)) => (date, time),
        None => (value, "000000"),
    };
    if date.len() != 8 || time.len() != 6 {
        return Err(format!(
            "invalid calendar date-time {value:?} (expected YYYYMMDD or YYYYMMDDTHHMMSS[Z])"
        ));
    }
    let year = date[0..4]
        .parse::<i32>()
        .map_err(|_| format!("invalid calendar year in {value:?}"))?;
    let month = date[4..6]
        .parse::<u8>()
        .map_err(|_| format!("invalid calendar month in {value:?}"))?;
    let day = date[6..8]
        .parse::<u8>()
        .map_err(|_| format!("invalid calendar day in {value:?}"))?;
    let hour = time[0..2]
        .parse::<u8>()
        .map_err(|_| format!("invalid calendar hour in {value:?}"))?;
    let minute = time[2..4]
        .parse::<u8>()
        .map_err(|_| format!("invalid calendar minute in {value:?}"))?;
    let second = time[4..6]
        .parse::<u8>()
        .map_err(|_| format!("invalid calendar second in {value:?}"))?;
    let month = loom_core::calendar::IcalMonth::try_from(month)
        .map_err(|_| format!("invalid calendar month in {value:?}"))?;
    let date = loom_core::calendar::IcalDate::from_calendar_date(year, month, day)
        .map_err(|_| format!("invalid calendar date {value:?}"))?;
    let time = loom_core::calendar::IcalTime::from_hms(hour, minute, second)
        .map_err(|_| format!("invalid calendar time {value:?}"))?;
    Ok(loom_core::calendar::DateTime::new(date, time))
}

fn calendar_range_cbor(items: &[loom_core::calendar::Occurrence]) -> Result<Vec<u8>, String> {
    loom_codec::encode(&WireValue::Array(
        items
            .iter()
            .map(|occ| {
                WireValue::Array(vec![
                    WireValue::Text(occ.uid.clone()),
                    WireValue::Text(occ.start.to_string()),
                ])
            })
            .collect(),
    ))
    .map_err(|e| e.to_string())
}

fn ensure_facet_workspace(
    loom: &mut Loom<FileStore>,
    workspace: &str,
    facet: FacetKind,
) -> Result<WorkspaceId, String> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: facet,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(|e| e.to_string())?;
    loom.registry_mut()
        .add_facet(ns, facet)
        .map_err(|e| e.to_string())?;
    Ok(ns)
}

fn ensure_vector_workspace(
    loom: &mut Loom<FileStore>,
    workspace: &str,
) -> Result<WorkspaceId, String> {
    ensure_facet_workspace(loom, workspace, FacetKind::Vector)
}

fn parse_kv_key_input(path: &str) -> Result<loom_core::Value, String> {
    loom_core::key_from_cbor(&read_input(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

fn props_from_cbor(bytes: &[u8]) -> Result<Props, String> {
    if bytes.is_empty() {
        return Ok(Props::new());
    }
    let WireValue::Map(pairs) = loom_codec::decode(bytes).map_err(|e| e.to_string())? else {
        return Err("graph props must be a CBOR map".to_string());
    };
    let mut props = Props::new();
    for (key, value) in pairs {
        let WireValue::Text(key) = key else {
            return Err("graph prop key must be text".to_string());
        };
        props.insert(key, graph_value_from_cbor(value)?);
    }
    Ok(props)
}

fn graph_value_from_cbor(value: WireValue) -> Result<loom_core::GraphValue, String> {
    match value {
        WireValue::Null => Ok(loom_core::GraphValue::Null),
        WireValue::Bool(value) => Ok(loom_core::GraphValue::Bool(value)),
        WireValue::Uint(value) => i64::try_from(value)
            .map(loom_core::GraphValue::Int)
            .map_err(|_| "graph property integer exceeds i64".to_string()),
        WireValue::Nint(value) => i64::try_from(value)
            .map(|value| loom_core::GraphValue::Int(-1 - value))
            .map_err(|_| "graph property integer exceeds i64".to_string()),
        WireValue::Float(value) if value.is_finite() => Ok(loom_core::GraphValue::Float(value)),
        WireValue::Float(_) => Err("graph property float must be finite".to_string()),
        WireValue::Text(value) => Ok(loom_core::GraphValue::Text(value)),
        WireValue::Bytes(value) => Ok(loom_core::GraphValue::Bytes(value)),
        WireValue::Array(values) if cbor_array_has_geometry_tag(&values) => {
            graph_geometry_from_cbor(values).map(loom_core::GraphValue::Geometry)
        }
        WireValue::Array(values) => values
            .into_iter()
            .map(graph_value_from_cbor)
            .collect::<Result<Vec<_>, _>>()
            .map(loom_core::GraphValue::List),
        WireValue::Map(pairs) => {
            let mut values = BTreeMap::new();
            for (key, value) in pairs {
                let WireValue::Text(key) = key else {
                    return Err("graph map key must be text".to_string());
                };
                values.insert(key, graph_value_from_cbor(value)?);
            }
            Ok(loom_core::GraphValue::Map(values))
        }
    }
}

fn graph_value_to_cbor(value: &loom_core::GraphValue) -> WireValue {
    match value {
        loom_core::GraphValue::Null => WireValue::Null,
        loom_core::GraphValue::Bool(value) => WireValue::Bool(*value),
        loom_core::GraphValue::Int(value) => WireValue::int(*value),
        loom_core::GraphValue::Float(value) => WireValue::Float(*value),
        loom_core::GraphValue::Text(value) => WireValue::Text(value.clone()),
        loom_core::GraphValue::Bytes(value) => WireValue::Bytes(value.clone()),
        loom_core::GraphValue::List(values) => {
            WireValue::Array(values.iter().map(graph_value_to_cbor).collect())
        }
        loom_core::GraphValue::Map(values) => WireValue::Map(
            values
                .iter()
                .map(|(key, value)| (WireValue::Text(key.clone()), graph_value_to_cbor(value)))
                .collect(),
        ),
        loom_core::GraphValue::Geometry(value) => graph_geometry_to_cbor(value),
    }
}

fn graph_geometry_to_cbor(value: &loom_core::GraphGeometry) -> WireValue {
    match value {
        loom_core::GraphGeometry::Point(point) => WireValue::Array(vec![
            WireValue::Text(loom_core::GRAPH_GEOMETRY_TAG.to_string()),
            WireValue::Text("point".to_string()),
            WireValue::Text(point.crs.as_str().to_string()),
            WireValue::Float(point.x),
            WireValue::Float(point.y),
            point.z.map(WireValue::Float).unwrap_or(WireValue::Null),
        ]),
    }
}

fn graph_geometry_from_cbor(values: Vec<WireValue>) -> Result<loom_core::GraphGeometry, String> {
    let [tag, kind, crs, x, y, z]: [WireValue; 6] = values
        .try_into()
        .map_err(|_| "malformed graph geometry value".to_string())?;
    if cbor_text(tag)? != loom_core::GRAPH_GEOMETRY_TAG {
        return Err("malformed graph geometry tag".to_string());
    }
    match cbor_text(kind)?.as_str() {
        "point" => {
            let crs =
                loom_core::GraphCrs::parse(&cbor_text(crs)?).map_err(|err| err.to_string())?;
            let x = cbor_finite_float(x, "graph geometry x coordinate")?;
            let y = cbor_finite_float(y, "graph geometry y coordinate")?;
            let z = match z {
                WireValue::Null => None,
                other => Some(cbor_finite_float(other, "graph geometry z coordinate")?),
            };
            loom_core::GraphGeometry::point(crs, x, y, z).map_err(|err| err.to_string())
        }
        _ => Err("unsupported graph geometry kind".to_string()),
    }
}

fn cbor_array_has_geometry_tag(values: &[WireValue]) -> bool {
    matches!(values.first(), Some(WireValue::Text(tag)) if tag == loom_core::GRAPH_GEOMETRY_TAG)
}

fn cbor_text(value: WireValue) -> Result<String, String> {
    match value {
        WireValue::Text(value) => Ok(value),
        _ => Err("graph geometry field must be text".to_string()),
    }
}

fn cbor_finite_float(value: WireValue, name: &str) -> Result<f64, String> {
    match value {
        WireValue::Float(value) if value.is_finite() => Ok(value),
        WireValue::Uint(value) => Ok(value as f64),
        WireValue::Nint(value) => Ok(-1.0 - value as f64),
        _ => Err(format!("{name} must be finite")),
    }
}

fn props_to_cbor(props: &Props) -> Result<Vec<u8>, String> {
    let pairs = props
        .iter()
        .map(|(key, value)| (WireValue::Text(key.clone()), graph_value_to_cbor(value)))
        .collect();
    loom_codec::encode(&WireValue::Map(pairs)).map_err(|e| e.to_string())
}

fn edge_value(edge: &Edge) -> WireValue {
    let props = edge
        .props
        .iter()
        .map(|(key, value)| (WireValue::Text(key.clone()), graph_value_to_cbor(value)))
        .collect();
    WireValue::Array(vec![
        WireValue::Text(edge.src.clone()),
        WireValue::Text(edge.dst.clone()),
        WireValue::Text(edge.label.clone()),
        WireValue::Map(props),
    ])
}

fn graph_edge_cbor(edge: &Edge) -> Result<Vec<u8>, String> {
    loom_codec::encode(&edge_value(edge)).map_err(|e| e.to_string())
}

fn graph_strings_cbor(ids: Vec<String>) -> Result<Vec<u8>, String> {
    loom_codec::encode(&WireValue::Array(
        ids.into_iter().map(WireValue::Text).collect(),
    ))
    .map_err(|e| e.to_string())
}

fn graph_edges_cbor(edges: Vec<(String, Edge)>) -> Result<Vec<u8>, String> {
    let items = edges
        .into_iter()
        .map(|(id, edge)| WireValue::Array(vec![WireValue::Text(id), edge_value(&edge)]))
        .collect();
    loom_codec::encode(&WireValue::Array(items)).map_err(|e| e.to_string())
}

fn columnar_cmp_op(tag: u64) -> Result<CmpOp, String> {
    match tag {
        0 => Ok(CmpOp::Eq),
        1 => Ok(CmpOp::Ne),
        2 => Ok(CmpOp::Lt),
        3 => Ok(CmpOp::Le),
        4 => Ok(CmpOp::Gt),
        5 => Ok(CmpOp::Ge),
        other => Err(format!("unknown columnar op tag {other}")),
    }
}

fn columnar_aggregate_op(tag: u64) -> Result<ColumnarAggregateOp, String> {
    match tag {
        0 => Ok(ColumnarAggregateOp::Count),
        1 => Ok(ColumnarAggregateOp::CountNonNull),
        2 => Ok(ColumnarAggregateOp::Min),
        3 => Ok(ColumnarAggregateOp::Max),
        4 => Ok(ColumnarAggregateOp::Sum),
        other => Err(format!("unknown columnar aggregate op tag {other}")),
    }
}

fn columnar_columns_from_cbor(bytes: &[u8]) -> Result<Vec<(String, ColumnType)>, String> {
    let WireValue::Array(items) = loom_codec::decode(bytes).map_err(|e| e.to_string())? else {
        return Err("columnar columns must be a CBOR array".to_string());
    };
    let mut columns = Vec::with_capacity(items.len());
    for item in items {
        let WireValue::Array(pair) = item else {
            return Err("each columnar column must be a [name, type_tag] array".to_string());
        };
        let mut iter = pair.into_iter();
        let name = match iter.next() {
            Some(WireValue::Text(name)) => name,
            _ => return Err("columnar column name must be text".to_string()),
        };
        let tag = match iter.next() {
            Some(WireValue::Uint(tag)) => u8::try_from(tag)
                .map_err(|_| "columnar column type tag out of range".to_string())?,
            _ => return Err("columnar column type tag must be a uint".to_string()),
        };
        columns.push((name, ColumnType::from_tag(tag).map_err(|e| e.to_string())?));
    }
    Ok(columns)
}

fn columnar_columns_cbor(columns: Vec<(String, ColumnType)>) -> Result<Vec<u8>, String> {
    let items = columns
        .into_iter()
        .map(|(name, ty)| {
            WireValue::Array(vec![
                WireValue::Text(name),
                WireValue::Uint(u64::from(ty.tag())),
            ])
        })
        .collect();
    loom_codec::encode(&WireValue::Array(items)).map_err(|e| e.to_string())
}

fn columnar_row_from_cbor(bytes: &[u8]) -> Result<Vec<loom_core::Value>, String> {
    let WireValue::Array(items) = loom_codec::decode(bytes).map_err(|e| e.to_string())? else {
        return Err("columnar row must be a CBOR cell array".to_string());
    };
    items
        .into_iter()
        .map(|item| cell_from(item).map_err(|e| e.to_string()))
        .collect()
}

fn columnar_rows_cbor(rows: Vec<Vec<loom_core::Value>>) -> Result<Vec<u8>, String> {
    let items = rows
        .into_iter()
        .map(|row| WireValue::Array(row.iter().map(cell_value).collect()))
        .collect();
    loom_codec::encode(&WireValue::Array(items)).map_err(|e| e.to_string())
}

fn columnar_values_cbor(values: Vec<loom_core::Value>) -> Result<Vec<u8>, String> {
    loom_codec::encode(&WireValue::Array(
        values.iter().map(cell_value).collect::<Vec<_>>(),
    ))
    .map_err(|e| e.to_string())
}

fn columnar_inspect_cbor(inspect: ColumnarInspect) -> Result<Vec<u8>, String> {
    loom_codec::encode(&WireValue::Array(vec![
        WireValue::Array(
            inspect
                .columns
                .into_iter()
                .map(|(name, ty)| {
                    WireValue::Array(vec![
                        WireValue::Text(name),
                        WireValue::Uint(u64::from(ty.tag())),
                    ])
                })
                .collect(),
        ),
        WireValue::Uint(inspect.rows as u64),
        WireValue::Uint(inspect.segment_count as u64),
        WireValue::Uint(inspect.target_segment_rows as u64),
        WireValue::Text(inspect.source_digest.to_string()),
    ]))
    .map_err(|e| e.to_string())
}

fn columnar_aggregates_from_cbor(bytes: &[u8]) -> Result<Vec<ColumnarAggregate>, String> {
    let WireValue::Array(items) = loom_codec::decode(bytes).map_err(|e| e.to_string())? else {
        return Err("columnar aggregates must be a CBOR array".to_string());
    };
    items
        .into_iter()
        .map(|item| {
            let WireValue::Array(fields) = item else {
                return Err("columnar aggregate must be [op, column?]".to_string());
            };
            let mut iter = fields.into_iter();
            let op = match iter.next() {
                Some(WireValue::Uint(tag)) => columnar_aggregate_op(tag)?,
                _ => return Err("columnar aggregate op must be a uint".to_string()),
            };
            let column = match iter.next() {
                Some(WireValue::Text(column)) => Some(column),
                Some(WireValue::Null) | None => None,
                _ => return Err("columnar aggregate column must be text or null".to_string()),
            };
            if iter.next().is_some() {
                return Err("columnar aggregate has extra fields".to_string());
            }
            Ok(ColumnarAggregate { op, column })
        })
        .collect()
}

fn dataframe_batch_cbor(batch: loom_core::DataframeBatch) -> Result<Vec<u8>, String> {
    let columns = batch
        .columns
        .iter()
        .map(|column| {
            WireValue::Array(vec![
                WireValue::Text(column.name.clone()),
                WireValue::Uint(u64::from(column.column_type.tag())),
                WireValue::Bool(column.nullable),
            ])
        })
        .collect();
    let rows = batch
        .rows
        .iter()
        .map(|row| WireValue::Array(row.iter().map(cell_value).collect()))
        .collect();
    loom_codec::encode(&WireValue::Array(vec![
        WireValue::Array(columns),
        WireValue::Array(rows),
    ]))
    .map_err(|e| e.to_string())
}

fn columnar_select_columns_from_cbor(bytes: &[u8]) -> Result<Vec<String>, String> {
    let WireValue::Array(items) = loom_codec::decode(bytes).map_err(|e| e.to_string())? else {
        return Err("columnar select columns must be a CBOR array".to_string());
    };
    items
        .into_iter()
        .map(|item| match item {
            WireValue::Text(name) => Ok(name),
            _ => Err("columnar select column must be text".to_string()),
        })
        .collect()
}

fn columnar_filter_from_cbor(
    bytes: &[u8],
) -> Result<Option<(String, CmpOp, loom_core::Value)>, String> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let WireValue::Array(items) = loom_codec::decode(bytes).map_err(|e| e.to_string())? else {
        return Err("columnar select filter must be a CBOR array".to_string());
    };
    let mut iter = items.into_iter();
    let column = match iter.next() {
        Some(WireValue::Text(column)) => column,
        _ => return Err("columnar filter column must be text".to_string()),
    };
    let op = match iter.next() {
        Some(WireValue::Uint(tag)) => columnar_cmp_op(tag)?,
        _ => return Err("columnar filter op must be a uint".to_string()),
    };
    let value = iter
        .next()
        .ok_or_else(|| "columnar filter is missing its value cell".to_string())?;
    Ok(Some((
        column,
        op,
        cell_from(value).map_err(|e| e.to_string())?,
    )))
}

fn ensure_columnar_import_target(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    name: &str,
    replace: bool,
) -> Result<(), String> {
    match loom_core::get_columnar(loom, ns, name) {
        Ok(_) if replace => Ok(()),
        Ok(_) => Err(format!(
            "columnar dataset {name:?} already exists; pass --replace to overwrite it"
        )),
        Err(err) if err.code == loom_core::error::Code::NotFound => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

fn search_field_type(tag: u64) -> Result<FieldType, String> {
    match tag {
        0 => Ok(FieldType::Text),
        1 => Ok(FieldType::Keyword),
        other => Err(format!("unknown search field type tag {other}")),
    }
}

fn search_mapping_from_cbor(bytes: &[u8]) -> Result<Mapping, String> {
    let WireValue::Map(pairs) = loom_codec::decode(bytes).map_err(|e| e.to_string())? else {
        return Err("search mapping must be a CBOR map".to_string());
    };
    let mut mapping = Mapping::new();
    for (key, value) in pairs {
        let WireValue::Text(field) = key else {
            return Err("search mapping field name must be text".to_string());
        };
        let WireValue::Array(parts) = value else {
            return Err("search field mapping must be an array".to_string());
        };
        if parts.len() != 3 && parts.len() != 6 {
            return Err("search field mapping must have 3 or 6 fields".to_string());
        }
        let mut iter = parts.into_iter();
        let field_type = match iter.next() {
            Some(WireValue::Uint(tag)) => search_field_type(tag)?,
            _ => return Err("search field type tag must be a uint".to_string()),
        };
        let stored = matches!(iter.next(), Some(WireValue::Bool(true)));
        let faceted = matches!(iter.next(), Some(WireValue::Bool(true)));
        let analysis = loom_core::AnalyzerMapping {
            index_analyzer: search_opt_text(iter.next())?,
            search_analyzer: search_opt_text(iter.next())?,
            normalizer: search_opt_text(iter.next())?,
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

fn search_opt_text(value: Option<WireValue>) -> Result<Option<String>, String> {
    match value {
        Some(WireValue::Null) | None => Ok(None),
        Some(WireValue::Text(value)) => Ok(Some(value)),
        _ => Err("search analyzer field must be text or null".to_string()),
    }
}

fn search_document_from_cbor(bytes: &[u8]) -> Result<Document, String> {
    let WireValue::Map(pairs) = loom_codec::decode(bytes).map_err(|e| e.to_string())? else {
        return Err("search document must be a CBOR map".to_string());
    };
    let mut doc = Document::new();
    for (key, value) in pairs {
        let WireValue::Text(field) = key else {
            return Err("search document field name must be text".to_string());
        };
        let value = match value {
            WireValue::Text(text) => FieldValue::Text(text),
            WireValue::Bytes(bytes) => FieldValue::Bytes(bytes),
            _ => return Err("search document value must be text or bytes".to_string()),
        };
        doc.insert(field, value);
    }
    Ok(doc)
}

fn search_document_cbor(doc: &Document) -> Result<Vec<u8>, String> {
    let pairs = doc
        .iter()
        .map(|(field, value)| {
            let value = match value {
                FieldValue::Text(text) => WireValue::Text(text.clone()),
                FieldValue::Bytes(bytes) => WireValue::Bytes(bytes.clone()),
            };
            (WireValue::Text(field.clone()), value)
        })
        .collect();
    loom_codec::encode(&WireValue::Map(pairs)).map_err(|e| e.to_string())
}

fn search_opt_bytes(value: Option<WireValue>) -> Result<Option<Vec<u8>>, String> {
    match value {
        Some(WireValue::Null) | None => Ok(None),
        Some(WireValue::Bytes(bytes)) => Ok(Some(bytes)),
        _ => Err("search range bound must be bytes or null".to_string()),
    }
}

fn search_query_from_value(value: WireValue) -> Result<Query, String> {
    let WireValue::Array(items) = value else {
        return Err("search query node must be a CBOR array".to_string());
    };
    let mut iter = items.into_iter();
    let tag = match iter.next() {
        Some(WireValue::Uint(tag)) => tag,
        _ => return Err("search query tag must be a uint".to_string()),
    };
    let text = |value: Option<WireValue>, what: &str| match value {
        Some(WireValue::Text(text)) => Ok(text),
        _ => Err(format!("search query {what} must be text")),
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
                Some(WireValue::Bytes(bytes)) => bytes,
                Some(WireValue::Text(text)) => text.into_bytes(),
                _ => return Err("search Term value must be bytes".to_string()),
            };
            Ok(Query::Term { field, value })
        }
        2 => {
            let field = text(iter.next(), "Phrase field")?;
            let terms = match iter.next() {
                Some(WireValue::Array(terms)) => terms
                    .into_iter()
                    .map(|term| match term {
                        WireValue::Text(term) => Ok(term),
                        _ => Err("search Phrase term must be text".to_string()),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                _ => return Err("search Phrase terms must be an array".to_string()),
            };
            let slop = match iter.next() {
                Some(WireValue::Uint(slop)) => u32::try_from(slop)
                    .map_err(|_| "search Phrase slop out of range".to_string())?,
                _ => return Err("search Phrase slop must be a uint".to_string()),
            };
            Ok(Query::Phrase { field, terms, slop })
        }
        3 => {
            let field = text(iter.next(), "Range field")?;
            let lower = search_opt_bytes(iter.next())?;
            let upper = search_opt_bytes(iter.next())?;
            let include_lower = matches!(iter.next(), Some(WireValue::Bool(true)));
            let include_upper = matches!(iter.next(), Some(WireValue::Bool(true)));
            Ok(Query::Range {
                field,
                lower,
                upper,
                include_lower,
                include_upper,
            })
        }
        4 => {
            let list = |value: Option<WireValue>, what: &str| match value {
                Some(WireValue::Array(queries)) => queries
                    .into_iter()
                    .map(search_query_from_value)
                    .collect::<Result<Vec<_>, _>>(),
                _ => Err(format!("search Bool {what} must be an array")),
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
                Some(WireValue::Bytes(bytes)) => bytes,
                Some(WireValue::Text(text)) => text.into_bytes(),
                _ => return Err("search Prefix value must be bytes".to_string()),
            };
            Ok(Query::Prefix { field, value })
        }
        7 => {
            let field = text(iter.next(), "Wildcard field")?;
            let pattern = match iter.next() {
                Some(WireValue::Bytes(bytes)) => bytes,
                Some(WireValue::Text(text)) => text.into_bytes(),
                _ => return Err("search Wildcard pattern must be bytes".to_string()),
            };
            Ok(Query::Wildcard { field, pattern })
        }
        8 => {
            let field = text(iter.next(), "Fuzzy field")?;
            let text = text(iter.next(), "Fuzzy text")?;
            let max_distance = match iter.next() {
                Some(WireValue::Uint(value)) => u32::try_from(value)
                    .map_err(|_| "search Fuzzy distance out of range".to_string())?,
                _ => return Err("search Fuzzy distance must be a uint".to_string()),
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
                Some(WireValue::Uint(value)) => u32::try_from(value)
                    .map_err(|_| "search Similar min_should_match out of range".to_string())?,
                _ => return Err("search Similar min_should_match must be a uint".to_string()),
            };
            Ok(Query::Similar {
                field,
                text,
                min_should_match,
            })
        }
        other => Err(format!("unknown search query tag {other}")),
    }
}

fn search_text_list_from_value(value: WireValue, what: &str) -> Result<Vec<String>, String> {
    let WireValue::Array(items) = value else {
        return Err(format!("{what} must be an array"));
    };
    items
        .into_iter()
        .map(|item| match item {
            WireValue::Text(text) => Ok(text),
            _ => Err(format!("{what} entries must be text")),
        })
        .collect()
}

fn search_aggregations_from_value(value: WireValue) -> Result<Vec<AggregationRequest>, String> {
    let WireValue::Array(items) = value else {
        return Err("search request aggregations must be an array".to_string());
    };
    items
        .into_iter()
        .map(|item| {
            let WireValue::Array(parts) = item else {
                return Err("search aggregation request must be an array".to_string());
            };
            if parts.len() != 3 {
                return Err("search aggregation request must have tag, name, and field".to_string());
            }
            let tag = match &parts[0] {
                WireValue::Uint(tag) => *tag,
                _ => return Err("search aggregation tag must be a uint".to_string()),
            };
            let WireValue::Text(name) = parts[1].clone() else {
                return Err("search aggregation name must be text".to_string());
            };
            let WireValue::Text(field) = parts[2].clone() else {
                return Err("search aggregation field must be text".to_string());
            };
            match tag {
                0 => Ok(AggregationRequest::Terms { name, field }),
                1 => Ok(AggregationRequest::ValueCount { name, field }),
                other => Err(format!("unknown search aggregation tag {other}")),
            }
        })
        .collect()
}

fn search_request_from_cbor(bytes: &[u8]) -> Result<QueryRequest, String> {
    let WireValue::Array(items) = loom_codec::decode(bytes).map_err(|e| e.to_string())? else {
        return Err("search request must be a CBOR array".to_string());
    };
    let mut iter = items.into_iter();
    let query = search_query_from_value(
        iter.next()
            .ok_or_else(|| "search request is missing its query".to_string())?,
    )?;
    let limit = match iter.next() {
        Some(WireValue::Uint(limit)) => {
            u32::try_from(limit).map_err(|_| "search limit out of range".to_string())?
        }
        _ => return Err("search request limit must be a uint".to_string()),
    };
    let offset = match iter.next() {
        Some(WireValue::Uint(offset)) => {
            u32::try_from(offset).map_err(|_| "search offset out of range".to_string())?
        }
        _ => return Err("search request offset must be a uint".to_string()),
    };
    let facets = match iter.next() {
        Some(value) => search_text_list_from_value(value, "search request facets")?,
        None => Vec::new(),
    };
    let highlight = match iter.next() {
        Some(value) => search_text_list_from_value(value, "search request highlight")?,
        None => Vec::new(),
    };
    let aggregations = match iter.next() {
        Some(value) => search_aggregations_from_value(value)?,
        None => Vec::new(),
    };
    if iter.next().is_some() {
        return Err("search request has extra fields".to_string());
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

fn search_response_cbor(response: &QueryResponse) -> Result<Vec<u8>, String> {
    let hits = response
        .hits
        .iter()
        .map(|hit| {
            Ok(WireValue::Array(vec![
                WireValue::Bytes(hit.id.clone()),
                wire_cell_value(&loom_core::Value::F32(hit.score))?,
            ]))
        })
        .collect::<Result<Vec<_>, String>>()?;
    loom_codec::encode(&WireValue::Array(vec![
        WireValue::Bool(response.reduced),
        WireValue::Array(hits),
    ]))
    .map_err(|e| e.to_string())
}

fn search_ids_cbor(ids: Vec<Vec<u8>>) -> Result<Vec<u8>, String> {
    loom_codec::encode(&WireValue::Array(
        ids.into_iter().map(WireValue::Bytes).collect(),
    ))
    .map_err(|e| e.to_string())
}

fn search_bytes_arg(
    text: Option<String>,
    path: Option<String>,
    label: &str,
) -> Result<Vec<u8>, String> {
    match (text, path) {
        (Some(_), Some(_)) => Err(format!(
            "provide either {label} or --{label}-file, not both"
        )),
        (Some(text), None) => Ok(text.into_bytes()),
        (None, Some(path)) => read_input(&path).map_err(|e| e.to_string()),
        (None, None) => Err(format!("missing {label}")),
    }
}

fn search_optional_bytes_arg(
    text: Option<String>,
    path: Option<String>,
    label: &str,
) -> Result<Option<Vec<u8>>, String> {
    match (text, path) {
        (Some(_), Some(_)) => Err(format!(
            "provide either {label} or --{label}-file, not both"
        )),
        (Some(text), None) => Ok(Some(text.into_bytes())),
        (None, Some(path)) => read_input(&path).map(Some).map_err(|e| e.to_string()),
        (None, None) => Ok(None),
    }
}

fn run_calendar(action: CalendarCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        CalendarCmd::CreateCollection {
            store,
            workspace,
            principal,
            collection,
            display_name,
            component,
        } => {
            let component_set = if component.is_empty() {
                vec![loom_core::calendar::Component::Event]
            } else {
                component
                    .iter()
                    .map(|value| parse_calendar_component(value))
                    .collect::<Result<Vec<_>, _>>()?
            };
            let client = remote::open_store_client(&store)?;
            client.cal_create_collection(
                keys,
                &workspace,
                &principal,
                &collection,
                display_name,
                component_set,
            )
        }
        CalendarCmd::DeleteCollection {
            store,
            workspace,
            principal,
            collection,
        } => {
            let client = remote::open_store_client(&store)?;
            let present =
                client.cal_delete_collection(keys, &workspace, &principal, &collection)?;
            println!("{present}");
            Ok(())
        }
        CalendarCmd::DeleteEntry {
            store,
            workspace,
            principal,
            collection,
            uid,
        } => {
            let client = remote::open_store_client(&store)?;
            let present =
                client.cal_delete_entry(keys, &workspace, &principal, &collection, &uid)?;
            println!("{present}");
            Ok(())
        }
        CalendarCmd::GetCollection {
            store,
            workspace,
            principal,
            collection,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(bytes) =
                client.cal_get_collection(keys, &workspace, &principal, &collection)?
            else {
                return Err(format!("calendar collection {collection:?} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        CalendarCmd::GetEntry {
            store,
            workspace,
            principal,
            collection,
            uid,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(bytes) =
                client.cal_get_entry(keys, &workspace, &principal, &collection, &uid)?
            else {
                return Err(format!("calendar entry {uid:?} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        CalendarCmd::ListCollections {
            store,
            workspace,
            principal,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let collections = client.cal_list_collections(keys, &workspace, &principal)?;
            if let Some(out) = out {
                write_output(Some(&out), &text_array_cbor(&collections)?).map_err(|e| e.to_string())
            } else {
                for collection in collections {
                    println!("{collection}");
                }
                Ok(())
            }
        }
        CalendarCmd::ListEntries {
            store,
            workspace,
            principal,
            collection,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded = client.cal_list_entries(keys, &workspace, &principal, &collection)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        CalendarCmd::PutEntry {
            store,
            workspace,
            principal,
            collection,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            let etag = client.cal_put_entry(keys, &workspace, &principal, &collection, bytes)?;
            println!("{etag}");
            Ok(())
        }
        CalendarCmd::PutIcs {
            store,
            workspace,
            principal,
            collection,
            input,
        } => {
            let ics = String::from_utf8(read_input(&input).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            let etag = client.cal_put_ics(keys, &workspace, &principal, &collection, ics)?;
            println!("{etag}");
            Ok(())
        }
        CalendarCmd::Range {
            store,
            workspace,
            principal,
            collection,
            from,
            to,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded =
                client.cal_range(keys, &workspace, &principal, &collection, &from, &to)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        CalendarCmd::Search {
            store,
            workspace,
            principal,
            collection,
            component,
            text,
            out,
        } => {
            let component = component
                .as_deref()
                .map(parse_calendar_component)
                .transpose()?;
            let client = remote::open_store_client(&store)?;
            let encoded =
                client.cal_search(keys, &workspace, &principal, &collection, component, text)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        CalendarCmd::ToIcs {
            store,
            workspace,
            principal,
            collection,
            uid,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(bytes) = client.cal_to_ics(keys, &workspace, &principal, &collection, &uid)?
            else {
                return Err(format!("calendar entry {uid:?} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
    }
}

fn run_cas(action: CasCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        CasCmd::Delete {
            store,
            workspace,
            digest,
        } => {
            let digest = Digest::parse(&digest).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            let present = client.cas_delete(keys, &workspace, &digest)?;
            println!("{present}");
            Ok(())
        }
        CasCmd::Get {
            store,
            workspace,
            digest,
            out,
        } => {
            let digest = Digest::parse(&digest).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            let Some(bytes) = client.cas_get(keys, &workspace, &digest)? else {
                return Err(format!("cas blob {digest} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        CasCmd::Has {
            store,
            workspace,
            digest,
        } => {
            let digest = Digest::parse(&digest).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            println!("{}", client.cas_has(keys, &workspace, &digest)?);
            Ok(())
        }
        CasCmd::List {
            store,
            workspace,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let digests = client.cas_list(keys, &workspace)?;
            let items = digests.iter().map(ToString::to_string).collect::<Vec<_>>();
            if let Some(out) = out {
                write_output(Some(&out), &text_array_cbor(&items)?).map_err(|e| e.to_string())
            } else {
                for item in items {
                    println!("{item}");
                }
                Ok(())
            }
        }
        CasCmd::Put {
            store,
            workspace,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            let digest = client.cas_put(keys, &workspace, bytes)?;
            println!("{digest}");
            Ok(())
        }
    }
}

fn run_document(action: DocumentCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        DocumentCmd::Delete {
            store,
            workspace,
            collection,
            id,
        } => {
            let client = remote::open_store_client(&store)?;
            let present = client.doc_delete(keys, &workspace, &collection, &id)?;
            println!("{present}");
            Ok(())
        }
        DocumentCmd::GetText {
            store,
            workspace,
            collection,
            id,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(document) = client.doc_get_text(keys, &workspace, &collection, &id)? else {
                return Err(format!("document id {id:?} not found"));
            };
            write_output(out.as_deref(), document.text.as_bytes()).map_err(|e| e.to_string())
        }
        DocumentCmd::PutText {
            store,
            workspace,
            collection,
            id,
            input,
            expected_entity_tag,
        } => {
            let text = String::from_utf8(read_input(&input).map_err(|e| e.to_string())?)
                .map_err(|_| Code::DocumentNotText.as_str().to_string())?;
            let client = remote::open_store_client(&store)?;
            let result = client.doc_put_text(
                keys,
                &workspace,
                &collection,
                &id,
                &text,
                expected_entity_tag.as_deref(),
            )?;
            println!("{}", result.entity_tag);
            Ok(())
        }
        DocumentCmd::GetBinary {
            store,
            workspace,
            collection,
            id,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(document) = client.doc_get_binary(keys, &workspace, &collection, &id)? else {
                return Err(format!("document id {id:?} not found"));
            };
            write_output(out.as_deref(), &document.bytes).map_err(|e| e.to_string())
        }
        DocumentCmd::PutBinary {
            store,
            workspace,
            collection,
            id,
            input,
            expected_entity_tag,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            let result = client.doc_put_binary(
                keys,
                &workspace,
                &collection,
                &id,
                bytes,
                expected_entity_tag.as_deref(),
            )?;
            println!("{}", result.entity_tag);
            Ok(())
        }
        DocumentCmd::ListBinary {
            store,
            workspace,
            collection,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded = client.doc_list_binary(keys, &workspace, &collection)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        DocumentCmd::Find {
            store,
            workspace,
            collection,
            index,
            value_json,
        } => {
            let client = remote::open_store_client(&store)?;
            let ids = client.doc_find(keys, &workspace, &collection, &index, &value_json)?;
            println!("{}", serde_json::json!({ "ids": ids }));
            Ok(())
        }
        DocumentCmd::Query {
            store,
            workspace,
            collection,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            let result = client.doc_query(keys, &workspace, &collection, &bytes)?;
            println!("{result}");
            Ok(())
        }
        DocumentCmd::IndexCreate {
            store,
            workspace,
            collection,
            name,
            path,
            unique,
        } => {
            let client = remote::open_store_client(&store)?;
            client.doc_index_create(keys, &workspace, &collection, &name, &path, unique)
        }
        DocumentCmd::IndexCreateJson {
            store,
            workspace,
            collection,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            client.doc_index_create_json(keys, &workspace, &collection, &bytes)
        }
        DocumentCmd::IndexDrop {
            store,
            workspace,
            collection,
            name,
        } => {
            let client = remote::open_store_client(&store)?;
            let dropped = client.doc_index_drop(keys, &workspace, &collection, &name)?;
            println!("{dropped}");
            Ok(())
        }
        DocumentCmd::IndexList {
            store,
            workspace,
            collection,
        } => {
            let client = remote::open_store_client(&store)?;
            println!("{}", client.doc_index_list(keys, &workspace, &collection)?);
            Ok(())
        }
        DocumentCmd::IndexRebuild {
            store,
            workspace,
            collection,
            name,
        } => {
            let client = remote::open_store_client(&store)?;
            client.doc_index_rebuild(keys, &workspace, &collection, &name)
        }
        DocumentCmd::IndexStatus {
            store,
            workspace,
            collection,
        } => {
            let client = remote::open_store_client(&store)?;
            println!(
                "{}",
                client.doc_index_statuses(keys, &workspace, &collection)?
            );
            Ok(())
        }
    }
}

fn run_contacts(action: ContactsCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        ContactsCmd::CreateBook {
            store,
            workspace,
            principal,
            book,
            display_name,
        } => {
            let client = remote::open_store_client(&store)?;
            client.con_create_book(keys, &workspace, &principal, &book, display_name)
        }
        ContactsCmd::DeleteBook {
            store,
            workspace,
            principal,
            book,
        } => {
            let client = remote::open_store_client(&store)?;
            let present = client.con_delete_book(keys, &workspace, &principal, &book)?;
            println!("{present}");
            Ok(())
        }
        ContactsCmd::DeleteEntry {
            store,
            workspace,
            principal,
            book,
            uid,
        } => {
            let client = remote::open_store_client(&store)?;
            let present = client.con_delete_entry(keys, &workspace, &principal, &book, &uid)?;
            println!("{present}");
            Ok(())
        }
        ContactsCmd::GetBook {
            store,
            workspace,
            principal,
            book,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(bytes) = client.con_get_book(keys, &workspace, &principal, &book)? else {
                return Err(format!("contacts book {book:?} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        ContactsCmd::GetEntry {
            store,
            workspace,
            principal,
            book,
            uid,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(bytes) = client.con_get_entry(keys, &workspace, &principal, &book, &uid)?
            else {
                return Err(format!("contacts entry {uid:?} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        ContactsCmd::ListBooks {
            store,
            workspace,
            principal,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let books = client.con_list_books(keys, &workspace, &principal)?;
            if let Some(out) = out {
                write_output(Some(&out), &text_array_cbor(&books)?).map_err(|e| e.to_string())
            } else {
                for book in books {
                    println!("{book}");
                }
                Ok(())
            }
        }
        ContactsCmd::ListEntries {
            store,
            workspace,
            principal,
            book,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded = client.con_list_entries(keys, &workspace, &principal, &book)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        ContactsCmd::PutEntry {
            store,
            workspace,
            principal,
            book,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            let etag = client.con_put_entry(keys, &workspace, &principal, &book, bytes)?;
            println!("{etag}");
            Ok(())
        }
        ContactsCmd::PutVcard {
            store,
            workspace,
            principal,
            book,
            input,
        } => {
            let vcard = String::from_utf8(read_input(&input).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            let etag = client.con_put_vcard(keys, &workspace, &principal, &book, vcard)?;
            println!("{etag}");
            Ok(())
        }
        ContactsCmd::Search {
            store,
            workspace,
            principal,
            book,
            text,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded = client.con_search(keys, &workspace, &principal, &book, &text)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        ContactsCmd::ToVcard {
            store,
            workspace,
            principal,
            book,
            uid,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(bytes) = client.con_to_vcard(keys, &workspace, &principal, &book, &uid)?
            else {
                return Err(format!("contacts entry {uid:?} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
    }
}

fn run_kv(action: KvCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        KvCmd::Delete {
            store,
            workspace,
            collection,
            key,
        } => {
            let key = parse_kv_key_input(&key)?;
            let client = remote::open_store_client(&store)?;
            let present = client.kv_delete(keys, &workspace, &collection, key)?;
            println!("{present}");
            Ok(())
        }
        KvCmd::Get {
            store,
            workspace,
            collection,
            key,
            out,
        } => {
            let key = parse_kv_key_input(&key)?;
            let client = remote::open_store_client(&store)?;
            let Some(bytes) = client.kv_get(keys, &workspace, &collection, key)? else {
                return Err("kv key not found".to_string());
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        KvCmd::List {
            store,
            workspace,
            collection,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded = client.kv_list(keys, &workspace, &collection)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        KvCmd::Put {
            store,
            workspace,
            collection,
            key,
            input,
        } => {
            let key = parse_kv_key_input(&key)?;
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            client.kv_put(keys, &workspace, &collection, key, bytes)
        }
        KvCmd::Range {
            store,
            workspace,
            collection,
            from,
            to,
            out,
        } => {
            let from = parse_kv_key_input(&from)?;
            let to = parse_kv_key_input(&to)?;
            let client = remote::open_store_client(&store)?;
            let encoded = client.kv_range(keys, &workspace, &collection, from, to)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
    }
}

fn run_mail(action: MailCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        MailCmd::CreateMailbox {
            store,
            workspace,
            principal,
            mailbox,
            display_name,
        } => {
            let client = remote::open_store_client(&store)?;
            client.mail_create_mailbox(keys, &workspace, &principal, &mailbox, display_name)
        }
        MailCmd::DeleteMailbox {
            store,
            workspace,
            principal,
            mailbox,
        } => {
            let client = remote::open_store_client(&store)?;
            let present = client.mail_delete_mailbox(keys, &workspace, &principal, &mailbox)?;
            println!("{present}");
            Ok(())
        }
        MailCmd::DeleteMessage {
            store,
            workspace,
            principal,
            mailbox,
            uid,
        } => {
            let client = remote::open_store_client(&store)?;
            let present =
                client.mail_delete_message(keys, &workspace, &principal, &mailbox, &uid)?;
            println!("{present}");
            Ok(())
        }
        MailCmd::GetFlags {
            store,
            workspace,
            principal,
            mailbox,
            uid,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let flags = client.mail_get_flags(keys, &workspace, &principal, &mailbox, &uid)?;
            if let Some(out) = out {
                write_output(Some(&out), &text_array_cbor(&flags)?).map_err(|e| e.to_string())
            } else {
                for flag in flags {
                    println!("{flag}");
                }
                Ok(())
            }
        }
        MailCmd::GetMailbox {
            store,
            workspace,
            principal,
            mailbox,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(bytes) = client.mail_get_mailbox(keys, &workspace, &principal, &mailbox)?
            else {
                return Err(format!("mailbox {mailbox:?} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        MailCmd::GetMessage {
            store,
            workspace,
            principal,
            mailbox,
            uid,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(bytes) =
                client.mail_get_message(keys, &workspace, &principal, &mailbox, &uid)?
            else {
                return Err(format!("mail message {uid:?} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        MailCmd::IngestMessage {
            store,
            workspace,
            principal,
            mailbox,
            uid,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            let digest =
                client.mail_ingest_message(keys, &workspace, &principal, &mailbox, &uid, bytes)?;
            println!("{digest}");
            Ok(())
        }
        MailCmd::ListMailboxes {
            store,
            workspace,
            principal,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let mailboxes = client.mail_list_mailboxes(keys, &workspace, &principal)?;
            if let Some(out) = out {
                write_output(Some(&out), &text_array_cbor(&mailboxes)?).map_err(|e| e.to_string())
            } else {
                for mailbox in mailboxes {
                    println!("{mailbox}");
                }
                Ok(())
            }
        }
        MailCmd::ListMessages {
            store,
            workspace,
            principal,
            mailbox,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded = client.mail_list_messages(keys, &workspace, &principal, &mailbox)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        MailCmd::Search {
            store,
            workspace,
            principal,
            mailbox,
            text,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded = client.mail_search(keys, &workspace, &principal, &mailbox, &text)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        MailCmd::SetFlags {
            store,
            workspace,
            principal,
            mailbox,
            uid,
            flags,
        } => {
            let client = remote::open_store_client(&store)?;
            client.mail_set_flags(keys, &workspace, &principal, &mailbox, &uid, flags)
        }
        MailCmd::ToEml {
            store,
            workspace,
            principal,
            mailbox,
            uid,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(bytes) = client.mail_to_eml(keys, &workspace, &principal, &mailbox, &uid)?
            else {
                return Err(format!("mail message {uid:?} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
    }
}

fn run_meetings(action: MeetingsCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        MeetingsCmd::List {
            store,
            workspace,
            limit,
            offset,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let snapshot = load_meetings_snapshot(&loom, &profile_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "meetings snapshot not found".to_string())?;
            let total = snapshot.meetings.len();
            let meetings = snapshot
                .meetings
                .iter()
                .skip(offset)
                .take(limit)
                .map(meeting_summary_json)
                .collect::<Vec<_>>();
            let body = serde_json::json!({
                "workspace_id": snapshot.workspace_id,
                "total": total,
                "offset": offset,
                "limit": limit,
                "meetings": meetings,
            });
            print_meetings_json_or_table(&format, &body, &["meeting_id", "title", "status"])
        }
        MeetingsCmd::Get {
            store,
            workspace,
            meeting_id,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let snapshot = load_meetings_snapshot(&loom, &profile_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "meetings snapshot not found".to_string())?;
            let meeting = snapshot
                .meetings
                .iter()
                .find(|meeting| meeting.meeting_id == meeting_id)
                .ok_or_else(|| "meeting not found".to_string())?;
            let body = meeting_detail_json(&snapshot.workspace_id, meeting, &snapshot.annotations);
            print_meetings_json_or_table(&format, &body, &[])
        }
        MeetingsCmd::Search {
            store,
            workspace,
            query,
            field,
            limit,
            offset,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let hits = collect_unified_search_hits(
                &loom,
                UnifiedSearchReadArgs {
                    query: &query,
                    workspace: Some(&workspace),
                    collection: Some(&profile_id),
                    field: field.as_deref(),
                    limit,
                    offset,
                },
            )?;
            print_unified_search(&format, &hits)
        }
        MeetingsCmd::SourceRead {
            store,
            workspace,
            source_id,
            leaf,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            validate_meetings_source_payload_leaf(&leaf).map_err(|e| e.to_string())?;
            let path = meetings_source_payload_path(&profile_id, &source_id, &leaf);
            let bytes = loom
                .read_file_reserved(workspace_id, &path)
                .map_err(|e| e.to_string())?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        MeetingsCmd::Import {
            store,
            workspace,
            input_profile,
            input,
            dry_run,
            report_format,
        } => {
            let input_profile =
                parse_meetings_input_profile(&input_profile).map_err(|e| e.to_string())?;
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = ensure_facet_workspace(&mut loom, &workspace, FacetKind::Vcs)?;
            let result =
                import_meetings_bytes(&mut loom, workspace_id, input_profile, &bytes, dry_run)
                    .map_err(|e| e.to_string())?;
            print_import_report(&result.report, &report_format)
        }
    }
}

fn load_meetings_snapshot(
    loom: &Loom<FileStore>,
    profile_id: &str,
) -> Result<Option<MeetingsProfileSnapshot>, String> {
    load_meetings_snapshot_io(loom, profile_id).map_err(|e| e.to_string())
}

fn run_tickets(action: TicketsCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        TicketsCmd::ProjectCreate {
            store,
            workspace,
            project_id,
            key_prefix,
            name,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = ensure_facet_workspace(&mut loom, &workspace, FacetKind::Vcs)?;
            let profile_id = workspace_id.to_string();
            let project = loom_tickets::create_project(
                &mut loom,
                workspace_id,
                &profile_id,
                &project_id,
                &key_prefix,
                &name,
                expected_root.as_deref(),
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_ticket_project(&project, &format)
        }
        TicketsCmd::ProjectRekey {
            store,
            workspace,
            project_id,
            key_prefix,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let project = loom_tickets::rekey_project(
                &mut loom,
                workspace_id,
                &profile_id,
                &project_id,
                &key_prefix,
                expected_root.as_deref(),
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_ticket_project(&project, &format)
        }
        TicketsCmd::ProjectSettingsGet {
            store,
            workspace,
            project_id,
            include_contracts,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let project = loom_tickets::get_project_with_contract_details(
                &loom,
                workspace_id,
                &profile_id,
                &project_id,
                include_contracts,
            )
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "ticket project not found".to_string())?;
            print_ticket_project(&project, &format)
        }
        TicketsCmd::ProjectSettingsSet {
            store,
            workspace,
            project_id,
            default_projection,
            actor_enforcement,
            project_owner,
            clear_project_owner,
            acceptance_authorities,
            replace_acceptance_authorities,
            acceptance_evidence_enforcement,
            required_acceptance_evidence_keys,
            replace_required_acceptance_evidence_keys,
            owner_contract_summary,
            owner_contract_details,
            worker_contract_summary,
            worker_contract_details,
            expected_root,
            format,
        } => {
            let default_projection = default_projection
                .as_deref()
                .map(loom_tickets::TicketProjectionProfile::parse)
                .transpose()
                .map_err(|e| e.to_string())?;
            let actor_enforcement = actor_enforcement
                .as_deref()
                .map(loom_tickets::TicketLifecycleAuthorizationPolicy::parse)
                .transpose()
                .map_err(|e| e.to_string())?;
            let acceptance_authorities = if replace_acceptance_authorities {
                Some(acceptance_authorities.as_slice())
            } else {
                None
            };
            let required_acceptance_evidence_keys = required_acceptance_evidence_keys
                .iter()
                .map(|key| loom_tickets::TicketAcceptanceEvidenceKey::parse(key))
                .collect::<loom_core::Result<Vec<_>>>()
                .map_err(|e| e.to_string())?;
            let required_acceptance_evidence_keys = if replace_required_acceptance_evidence_keys {
                Some(required_acceptance_evidence_keys.as_slice())
            } else {
                None
            };
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let project = loom_tickets::set_project_settings(
                &mut loom,
                workspace_id,
                loom_tickets::TicketProjectSettingsRequest {
                    workspace_id: &profile_id,
                    project_id: &project_id,
                    default_projection,
                    enable_projections: &[],
                    disable_projections: &[],
                    actor_enforcement,
                    project_owner_principal: project_owner.as_deref(),
                    clear_project_owner_principal: clear_project_owner,
                    acceptance_authorities,
                    acceptance_evidence_enforcement,
                    required_acceptance_evidence_keys,
                    owner_contract_summary: owner_contract_summary.as_deref(),
                    owner_contract_details: owner_contract_details.as_deref(),
                    worker_contract_summary: worker_contract_summary.as_deref(),
                    worker_contract_details: worker_contract_details.as_deref(),
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_ticket_project(&project, &format)
        }
        TicketsCmd::Projects {
            store,
            workspace,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let projects = loom_tickets::list_projects(&loom, workspace_id, &profile_id)
                .map_err(|e| e.to_string())?;
            print_ticket_projects(&projects, &format)
        }
        TicketsCmd::Relations {
            store,
            workspace,
            ticket_id,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let relations =
                loom_tickets::list_ticket_relations(&loom, workspace_id, &profile_id, &ticket_id)
                    .map_err(|e| e.to_string())?;
            print_ticket_relations(&relations, &format)
        }
        TicketsCmd::Fields {
            store,
            workspace,
            project_id,
            projection,
            operation,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let projection = loom_tickets::parse_ticket_projection(projection.as_deref())
                .map_err(|e| e.to_string())?;
            let catalog = if let Some(project_id) = project_id {
                loom_tickets::ticket_field_catalog_for_project(
                    &loom,
                    workspace_id,
                    &profile_id,
                    &project_id,
                    projection,
                    operation.as_deref(),
                )
            } else {
                loom_tickets::ticket_field_catalog(projection, operation.as_deref())
            }
            .map_err(|e| e.to_string())?;
            print_ticket_field_catalog(&catalog, &format)
        }
        TicketsCmd::FieldPut {
            store,
            workspace,
            project_id,
            field_id,
            key,
            name,
            field_type,
            option_set,
            description,
            max_length,
            required,
            searchable,
            orderable,
            cardinality,
            applicable_type_ids,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let catalog = loom_tickets::put_ticket_field_definition(
                &mut loom,
                workspace_id,
                loom_tickets::TicketFieldDefinitionWriteRequest {
                    workspace_id: &profile_id,
                    project_id: &project_id,
                    field_id: &field_id,
                    key: &key,
                    name: &name,
                    description: description.as_deref(),
                    field_type: &field_type,
                    option_set: option_set.as_deref(),
                    max_length,
                    required,
                    searchable,
                    orderable,
                    cardinality: parse_ticket_field_cardinality(&cardinality)?,
                    applicable_type_ids: &applicable_type_ids,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_ticket_field_catalog(&catalog, &format)
        }
        TicketsCmd::FieldRetire {
            store,
            workspace,
            project_id,
            field_id,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let catalog = loom_tickets::retire_ticket_field_definition(
                &mut loom,
                workspace_id,
                loom_tickets::TicketFieldDefinitionRetireRequest {
                    workspace_id: &profile_id,
                    project_id: &project_id,
                    field_id: &field_id,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_ticket_field_catalog(&catalog, &format)
        }
        TicketsCmd::Create {
            store,
            workspace,
            ticket_type,
            project_id,
            title,
            description,
            priority,
            assignee,
            fields,
            projection,
            external_source,
            external_id,
            policy_labels,
            expected_root,
            format,
        } => {
            let fields_input = parse_ticket_fields(&fields)?;
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            // Resolve the project: explicit --project-id, else the sole project when unambiguous.
            let project_id = match project_id {
                Some(project_id) => project_id,
                None => {
                    let projects = loom_tickets::list_projects(&loom, workspace_id, &profile_id)
                        .map_err(|e| e.to_string())?;
                    match projects.as_slice() {
                        [only] => only.project_id.clone(),
                        [] => {
                            return Err("workspace has no ticket projects; create one with `tickets project create` or pass --project-id".to_string());
                        }
                        _ => {
                            return Err(
                                "workspace has multiple ticket projects; specify --project-id"
                                    .to_string(),
                            );
                        }
                    }
                }
            };
            // Resolve the input projection: explicit --projection, else the project's default.
            let projection = match projection.as_deref() {
                Some(projection) => loom_tickets::parse_ticket_projection(Some(projection))
                    .map_err(|e| e.to_string())?,
                None => {
                    match loom_tickets::get_project(&loom, workspace_id, &profile_id, &project_id)
                        .map_err(|e| e.to_string())?
                    {
                        Some(project) => {
                            loom_tickets::parse_ticket_projection(Some(&project.default_projection))
                                .map_err(|e| e.to_string())?
                        }
                        None => None,
                    }
                }
            };
            // `--fields` (projected vocabulary) and first-class canonical flags converge on one
            // native field map and one create-time validation path (create_ticket).
            let mut fields =
                loom_tickets::normalize_ticket_fields_for_projection(&fields_input, projection)
                    .map_err(|e| e.to_string())?;
            let Some(object) = fields.as_object_mut() else {
                return Err("ticket fields must be a JSON object".to_string());
            };
            for (key, value) in [
                ("title", title),
                ("description", description),
                ("priority", priority),
                ("assignee", assignee),
            ] {
                if let Some(value) = value {
                    if object.contains_key(key) {
                        return Err(format!(
                            "canonical field `{key}` was provided by both --{key} and --fields"
                        ));
                    }
                    object.insert(key.to_string(), serde_json::Value::String(value));
                }
            }
            let ticket = loom_tickets::create_ticket(
                &mut loom,
                workspace_id,
                loom_tickets::TicketCreateRequest {
                    workspace_id: &profile_id,
                    project_id: &project_id,
                    ticket_type: &ticket_type,
                    external_source: external_source.as_deref(),
                    external_id: external_id.as_deref(),
                    fields: &fields,
                    policy_labels: &policy_labels,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            update_ticket_reference_index(&mut loom, workspace_id, &ticket)?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            let envelope = ticket_mutation_envelope(
                ticket,
                "ticket.created",
                expected_root.as_deref(),
                vec![MutationChange::ResourceCreated],
            );
            print_ticket_mutation(&envelope, &format)
        }
        TicketsCmd::Update {
            store,
            workspace,
            ticket_id,
            request,
            projection,
            status,
            assignee,
            title,
            description,
            priority,
            fields,
            delete_fields,
            action,
            comment_body,
            comment_id,
            comment_type,
            comment_evidence,
            observed_source_status,
            observed_workflow_version,
            expected_root,
            format,
        } => {
            let request = ticket_update_request_from_parts(TicketUpdateCliParts {
                request,
                workspace,
                ticket_id,
                projection,
                status,
                assignee,
                title,
                description,
                priority,
                fields,
                delete_fields,
                action,
                comment_body,
                comment_id,
                comment_type,
                comment_evidence,
                observed_source_status,
                observed_workflow_version,
                expected_root,
            })?;
            let projection = loom_tickets::parse_ticket_projection(request.projection.as_deref())
                .map_err(|e| e.to_string())?;
            let set_fields = request
                .set_fields
                .as_ref()
                .map(|fields| {
                    loom_tickets::normalize_ticket_fields_for_projection(fields, projection)
                })
                .transpose()
                .map_err(|e| e.to_string())?;
            let delete_fields = loom_tickets::normalize_ticket_delete_fields_for_projection(
                &request.delete_fields,
                projection,
            );
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &request.workspace)?;
            let profile_id = workspace_id.to_string();
            let action = request
                .action
                .as_deref()
                .map(loom_tickets::TicketLifecycleAction::parse)
                .transpose()
                .map_err(|error| error.to_string())?;
            let changes = cli_ticket_update_changes(CliTicketUpdateChangeInputs {
                set_fields: set_fields.as_ref(),
                delete_fields: &delete_fields,
                action_applied: action.is_some(),
                target_status: request.target_status.as_deref(),
                observed_source_status: request.observed_source_status.as_deref(),
                assignee: request.assignee.as_deref(),
                comment: request.comment.as_ref(),
                comments: &request.comments,
                relation_sets: &request.relation_sets,
                relation_removes: &request.relation_removes,
            });
            let comment = request
                .comment
                .as_ref()
                .map(|comment| {
                    comment
                        .evidence
                        .as_ref()
                        .map(loom_tickets::TicketCommentEvidence::from_json)
                        .transpose()
                        .map(|evidence| loom_tickets::TicketUpdateCommentRequest {
                            comment_id: comment.comment_id.as_deref(),
                            comment_type: comment.comment_type.as_deref(),
                            body: &comment.body,
                            evidence,
                        })
                })
                .transpose()
                .map_err(|error| error.to_string())?;
            let comments = request
                .comments
                .iter()
                .map(|comment| {
                    comment
                        .evidence
                        .as_ref()
                        .map(loom_tickets::TicketCommentEvidence::from_json)
                        .transpose()
                        .map(|evidence| loom_tickets::TicketUpdateCommentRequest {
                            comment_id: comment.comment_id.as_deref(),
                            comment_type: comment.comment_type.as_deref(),
                            body: &comment.body,
                            evidence,
                        })
                })
                .collect::<loom_core::Result<Vec<_>>>()
                .map_err(|error| error.to_string())?;
            let relation_sets = request
                .relation_sets
                .iter()
                .map(|relation| {
                    loom_tickets::TicketRelationKind::parse(&relation.kind).map(|kind| {
                        loom_tickets::TicketUpdateRelationSetRequest {
                            relation_id: relation.relation_id.as_deref(),
                            kind,
                            target_id: &relation.target_id,
                        }
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            let relation_removes = request
                .relation_removes
                .iter()
                .map(|relation| loom_tickets::TicketUpdateRelationRemoveRequest {
                    relation_id: &relation.relation_id,
                })
                .collect::<Vec<_>>();
            let ticket = loom_tickets::update_ticket(
                &mut loom,
                workspace_id,
                loom_tickets::TicketUpdateRequest {
                    workspace_id: &profile_id,
                    ticket_id: &request.ticket_id,
                    set_fields: set_fields.as_ref(),
                    delete_fields: &delete_fields,
                    action,
                    target_status: request.target_status.as_deref(),
                    observed_source_status: request.observed_source_status.as_deref(),
                    observed_workflow_version: request.observed_workflow_version.as_deref(),
                    assignee: request.assignee.as_deref(),
                    expected_root: request.expected_root.as_deref(),
                    comment,
                    comments: &comments,
                    relation_sets: &relation_sets,
                    relation_removes: &relation_removes,
                },
            )
            .map_err(|e| e.to_string())?;
            update_ticket_reference_index(&mut loom, workspace_id, &ticket)?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            let envelope = ticket_mutation_envelope(
                ticket,
                "ticket.updated",
                request.expected_root.as_deref(),
                changes,
            );
            print_ticket_mutation(&envelope, &format)
        }
        TicketsCmd::Delete {
            store,
            workspace,
            ticket_id,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let ticket = loom_tickets::delete_ticket(
                &mut loom,
                workspace_id,
                loom_tickets::TicketDeleteRequest {
                    workspace_id: &profile_id,
                    ticket_id: &ticket_id,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            update_ticket_reference_index(&mut loom, workspace_id, &ticket)?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            let envelope = ticket_mutation_envelope(
                ticket,
                "ticket.deleted",
                expected_root.as_deref(),
                vec![MutationChange::ResourceDeleted],
            );
            print_ticket_mutation(&envelope, &format)
        }
        TicketsCmd::Comments {
            store,
            workspace,
            ticket_id,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let comments =
                loom_tickets::list_ticket_comments(&loom, workspace_id, &profile_id, &ticket_id)
                    .map_err(|e| e.to_string())?;
            print_ticket_comments(&comments, &format)
        }
        TicketsCmd::CommentAdd {
            store,
            workspace,
            ticket_id,
            body,
            comment_id,
            comment_type,
            evidence,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let body = read_ticket_comment_body(&body)?;
            let evidence = evidence
                .as_deref()
                .map(parse_ticket_comment_evidence)
                .transpose()?;
            let ticket = loom_tickets::add_ticket_comment(
                &mut loom,
                workspace_id,
                loom_tickets::TicketCommentRequest {
                    workspace_id: &profile_id,
                    ticket_id: &ticket_id,
                    comment_id: comment_id.as_deref(),
                    comment_type: Some(&comment_type),
                    body: &body,
                    evidence,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            update_ticket_reference_index(&mut loom, workspace_id, &ticket)?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            let envelope = ticket_mutation_envelope(
                ticket,
                "ticket.comment_added",
                expected_root.as_deref(),
                vec![MutationChange::field_set("comment", comment_type)],
            );
            print_ticket_mutation(&envelope, &format)
        }
        TicketsCmd::CommentUpdate {
            store,
            workspace,
            ticket_id,
            comment_id,
            body,
            comment_type,
            evidence,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let body = body.as_deref().map(read_ticket_comment_body).transpose()?;
            let evidence = evidence
                .as_deref()
                .map(parse_ticket_comment_evidence_update)
                .transpose()?;
            let ticket = loom_tickets::update_ticket_comment(
                &mut loom,
                workspace_id,
                loom_tickets::TicketCommentUpdateRequest {
                    workspace_id: &profile_id,
                    ticket_id: &ticket_id,
                    comment_id: &comment_id,
                    comment_type: comment_type.as_deref(),
                    body: body.as_deref(),
                    evidence,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            update_ticket_reference_index(&mut loom, workspace_id, &ticket)?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            let envelope = ticket_mutation_envelope(
                ticket,
                "ticket.comment_updated",
                expected_root.as_deref(),
                vec![MutationChange::field_changed(
                    "comment",
                    None::<String>,
                    Some(comment_id),
                )],
            );
            print_ticket_mutation(&envelope, &format)
        }
        TicketsCmd::CommentDelete {
            store,
            workspace,
            ticket_id,
            comment_id,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let ticket = loom_tickets::delete_ticket_comment(
                &mut loom,
                workspace_id,
                loom_tickets::TicketCommentDeleteRequest {
                    workspace_id: &profile_id,
                    ticket_id: &ticket_id,
                    comment_id: &comment_id,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            update_ticket_reference_index(&mut loom, workspace_id, &ticket)?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            let envelope = ticket_mutation_envelope(
                ticket,
                "ticket.comment_deleted",
                expected_root.as_deref(),
                vec![MutationChange::field_deleted("comment", Some(comment_id))],
            );
            print_ticket_mutation(&envelope, &format)
        }
        TicketsCmd::BoardCreate {
            store,
            workspace,
            board_id,
            board_key,
            project_id,
            name,
            mode,
            description,
            columns,
            card_display_fields,
            updated_by,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let columns = parse_board_columns(&columns)?;
            let board = loom_tickets::create_board(
                &mut loom,
                workspace_id,
                loom_tickets::BoardCreateRequest {
                    workspace_id: &profile_id,
                    board_id: &board_id,
                    board_key: &board_key,
                    name: &name,
                    description: &description,
                    project_id: &project_id,
                    scope: if mode == "manual" {
                        loom_tickets::BoardScope::ManualSet
                    } else {
                        loom_tickets::BoardScope::project(project_id.clone())
                    },
                    mode: loom_tickets::BoardMode::parse(&mode).map_err(|e| e.to_string())?,
                    columns: &columns,
                    swimlanes: &[],
                    card_display_fields: &card_display_fields,
                    owner_principal: None,
                    coordinator_principal: None,
                    updated_by: &updated_by,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_board(&board, &format)
        }
        TicketsCmd::BoardGet {
            store,
            workspace,
            board_id,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let board = loom_tickets::get_board(&loom, workspace_id, &profile_id, &board_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "board not found".to_string())?;
            print_board(&board, &format)
        }
        TicketsCmd::BoardList {
            store,
            workspace,
            include_deleted,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let boards =
                loom_tickets::list_boards(&loom, workspace_id, &profile_id, include_deleted)
                    .map_err(|e| e.to_string())?;
            print_boards(&boards, &format)
        }
        TicketsCmd::BoardUpdate {
            store,
            workspace,
            board_id,
            board_key,
            name,
            description,
            board_status,
            card_display_fields,
            updated_by,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let board_status = board_status
                .as_deref()
                .map(loom_tickets::BoardStatus::parse)
                .transpose()
                .map_err(|e| e.to_string())?;
            let card_display_fields = if card_display_fields.is_empty() {
                None
            } else {
                Some(card_display_fields.as_slice())
            };
            let board = loom_tickets::update_board(
                &mut loom,
                workspace_id,
                loom_tickets::BoardUpdateRequest {
                    workspace_id: &profile_id,
                    board_id: &board_id,
                    board_key: board_key.as_deref(),
                    name: name.as_deref(),
                    description: description.as_deref(),
                    scope: None,
                    owner_principal: None,
                    coordinator_principal: None,
                    card_display_fields,
                    board_status,
                    updated_by: &updated_by,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_board(&board, &format)
        }
        TicketsCmd::BoardDelete {
            store,
            workspace,
            board_id,
            updated_by,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let board = loom_tickets::update_board(
                &mut loom,
                workspace_id,
                loom_tickets::BoardUpdateRequest {
                    workspace_id: &profile_id,
                    board_id: &board_id,
                    board_key: None,
                    name: None,
                    description: None,
                    scope: None,
                    owner_principal: None,
                    coordinator_principal: None,
                    card_display_fields: None,
                    board_status: Some(loom_tickets::BoardStatus::Deleted),
                    updated_by: &updated_by,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_board(&board, &format)
        }
        TicketsCmd::BoardConfigureColumns {
            store,
            workspace,
            board_id,
            mode,
            columns,
            updated_by,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let columns = parse_board_columns(&columns)?;
            let mode = mode
                .as_deref()
                .map(loom_tickets::BoardMode::parse)
                .transpose()
                .map_err(|e| e.to_string())?;
            let board = loom_tickets::configure_board_columns(
                &mut loom,
                workspace_id,
                loom_tickets::BoardColumnConfigureRequest {
                    workspace_id: &profile_id,
                    board_id: &board_id,
                    mode,
                    columns: &columns,
                    swimlanes: &[],
                    updated_by: &updated_by,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_board(&board, &format)
        }
        TicketsCmd::BoardMoveCard {
            store,
            workspace,
            board_id,
            ticket_id,
            column_id,
            rank_token,
            swimlane_id,
            updated_by,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let board = loom_tickets::move_board_card(
                &mut loom,
                workspace_id,
                loom_tickets::BoardCardMoveRequest {
                    workspace_id: &profile_id,
                    board_id: &board_id,
                    ticket_id: &ticket_id,
                    column_id: &column_id,
                    rank_token: &rank_token,
                    swimlane_id: swimlane_id.as_deref(),
                    updated_by: &updated_by,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_board(&board, &format)
        }
        TicketsCmd::RelationSet {
            store,
            workspace,
            ticket_id,
            kind,
            target_id,
            relation_id,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let kind = loom_tickets::TicketRelationKind::parse(&kind).map_err(|e| e.to_string())?;
            let relation = loom_tickets::add_ticket_relation(
                &mut loom,
                workspace_id,
                loom_tickets::TicketRelationRequest {
                    workspace_id: &profile_id,
                    ticket_id: &ticket_id,
                    relation_id: relation_id.as_deref(),
                    kind,
                    target_id: &target_id,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            let change = MutationChange::relation_set(
                relation.relation_id.clone(),
                relation.kind.clone(),
                relation.target_id.clone(),
            );
            let envelope = relation_mutation_envelope(
                relation,
                "ticket.relation_set",
                expected_root.as_deref(),
                vec![change],
            );
            print_ticket_relation_mutation(&envelope, &format)
        }
        TicketsCmd::RelationRemove {
            store,
            workspace,
            ticket_id,
            relation_id,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let relation = loom_tickets::remove_ticket_relation(
                &mut loom,
                workspace_id,
                loom_tickets::TicketRelationRemoveRequest {
                    workspace_id: &profile_id,
                    ticket_id: &ticket_id,
                    relation_id: &relation_id,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            let change = MutationChange::relation_removed(
                relation.relation_id.clone(),
                relation.kind.clone(),
                relation.target_id.clone(),
            );
            let envelope = relation_mutation_envelope(
                relation,
                "ticket.relation_removed",
                expected_root.as_deref(),
                vec![change],
            );
            print_ticket_relation_mutation(&envelope, &format)
        }
        TicketsCmd::List {
            store,
            workspace,
            projection,
            statuses,
            assignees,
            priorities,
            ticket_types,
            labels,
            policy_labels,
            lane,
            board,
            ready,
            include_completed,
            limit,
            cursor,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            // First-class Lane membership: resolve lane -> ticket ids via loom-lanes and pass the
            // allowlist down; the native list has no lane concept of its own.
            let lane_member_ids = match lane.as_deref() {
                Some(lane_id) => {
                    let lane = loom_lanes::get_lane(&loom, workspace_id, lane_id)
                        .map_err(|e| e.to_string())?
                        .ok_or_else(|| format!("lane {lane_id:?} not found"))?;
                    Some(
                        lane.lane_tickets
                            .iter()
                            .map(|ticket| ticket.ticket_id.clone())
                            .collect::<Vec<_>>(),
                    )
                }
                None => None,
            };
            let query = loom_tickets::TicketListQuery {
                projection: loom_tickets::parse_ticket_projection(projection.as_deref())
                    .map_err(|e| e.to_string())?,
                statuses,
                assignees,
                priorities,
                ticket_types,
                labels,
                policy_labels,
                ready_only: ready,
                include_completed,
                lane_member_ids,
                board_id: board,
                cursor,
                limit,
            };
            let page = loom_tickets::list_tickets_page(&loom, workspace_id, &profile_id, &query)
                .map_err(|e| e.to_string())?;
            print_ticket_page(&page, &format)
        }
        TicketsCmd::Get {
            store,
            workspace,
            ticket_id,
            projection,
            detailed,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let ticket = loom_tickets::get_ticket_with_projection(
                &loom,
                workspace_id,
                &profile_id,
                &ticket_id,
                loom_tickets::parse_ticket_projection(projection.as_deref())
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "ticket not found".to_string())?;
            let history =
                loom_tickets::history(&loom, workspace_id, &profile_id, Some(&ticket.primary_key))
                    .map_err(|e| e.to_string())?;
            let comments =
                loom_tickets::list_ticket_comments(&loom, workspace_id, &profile_id, &ticket_id)
                    .map_err(|e| e.to_string())?;
            print_ticket_detail(&ticket, &history, &comments, detailed, &format)
        }
        TicketsCmd::History {
            store,
            workspace,
            ticket_id,
            detailed,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let history =
                loom_tickets::history(&loom, workspace_id, &profile_id, ticket_id.as_deref())
                    .map_err(|e| e.to_string())?;
            print_ticket_history(&history, detailed, &format)
        }
    }
}

fn run_lanes(action: LanesCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        LanesCmd::Create {
            store,
            workspace,
            lane_id,
            lane_key,
            kind,
            title,
            description,
            owner_principal,
            lane_status,
            active_ticket_id,
            status_report,
            reviewer_feedback,
            updated_at,
            updated_by,
            tickets,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let actor = resolve_lane_actor(&loom, workspace_id, updated_by.as_deref())?;
            let lane_tickets =
                loom_lanes::lane_tickets_from_order(&tickets).map_err(|e| e.to_string())?;
            let lane = Lane::new(LaneInput {
                lane_id: &lane_id,
                lane_key: &lane_key,
                title: &title,
                description: &description,
                lane_kind: LaneKind::parse(&kind).map_err(|e| e.to_string())?,
                owner_principal: owner_principal.as_deref(),
                lane_status: LaneStatus::parse(&lane_status).map_err(|e| e.to_string())?,
                lane_tickets: &lane_tickets,
                active_ticket_id: active_ticket_id.as_deref(),
                status_report: &status_report,
                reviewer_feedback: &reviewer_feedback,
                updated_at: updated_at.unwrap_or(current_time_ms()?),
                updated_by: &actor,
            })
            .map_err(|e| e.to_string())?;
            let lane = loom_lanes::create_lane(&mut loom, workspace_id, lane)
                .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            let envelope =
                lane_mutation_envelope(lane, "lane.created", vec![MutationChange::ResourceCreated]);
            print_lane_mutation(&envelope, &format)
        }
        LanesCmd::Get {
            store,
            workspace,
            lane_id,
            detailed,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let lane = loom_lanes::get_lane(&loom, workspace_id, &lane_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "lane not found".to_string())?;
            if detailed {
                let view = build_lane_view(&loom, workspace_id, &workspace, &lane);
                print_lane_view(&view, &format, true)
            } else {
                print_lane(&lane, &format)
            }
        }
        LanesCmd::List {
            store,
            workspace,
            detailed,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let (lanes, diagnostics) = loom_lanes::list_lanes_with_diagnostics(&loom, workspace_id)
                .map_err(|e| e.to_string())?;
            let views = lanes
                .iter()
                .map(|lane| build_lane_view(&loom, workspace_id, &workspace, lane))
                .collect::<Vec<_>>();
            print_lane_views(&views, &diagnostics, &format, detailed)
        }
        LanesCmd::Update {
            store,
            workspace,
            lane_id,
            title,
            description,
            lane_status,
            status_report,
            reviewer_feedback,
            updated_by,
            format,
        } => {
            if title.is_none()
                && description.is_none()
                && lane_status.is_none()
                && status_report.is_none()
                && reviewer_feedback.is_none()
            {
                return Err("lane update requires at least one field option".to_string());
            }
            let mut changes = Vec::new();
            if let Some(title) = title.as_ref() {
                changes.push(MutationChange::field_set("title", title.clone()));
            }
            if let Some(description) = description.as_ref() {
                changes.push(MutationChange::field_set(
                    "description",
                    description.clone(),
                ));
            }
            if let Some(lane_status) = lane_status.as_ref() {
                changes.push(MutationChange::field_set(
                    "lane_status",
                    lane_status.clone(),
                ));
            }
            if let Some(status_report) = status_report.as_ref() {
                changes.push(MutationChange::field_set(
                    "status_report",
                    status_report.clone(),
                ));
            }
            if let Some(reviewer_feedback) = reviewer_feedback.as_ref() {
                changes.push(MutationChange::field_set(
                    "reviewer_feedback",
                    reviewer_feedback.clone(),
                ));
            }
            mutate_lane(
                &store,
                &workspace,
                &lane_id,
                keys,
                &format,
                "lane.updated",
                changes,
                |lane, loom, ns| {
                    if let Some(title) = title {
                        lane.title = title;
                    }
                    if let Some(description) = description {
                        lane.description = description;
                    }
                    if let Some(lane_status) = lane_status {
                        lane.lane_status = LaneStatus::parse(&lane_status)
                            .map_err(|e| e.to_string())?
                            .as_str()
                            .to_string();
                    }
                    if let Some(status_report) = status_report {
                        lane.status_report = status_report;
                    }
                    if let Some(reviewer_feedback) = reviewer_feedback {
                        lane.reviewer_feedback = reviewer_feedback;
                    }
                    let actor = resolve_lane_actor(loom, ns, updated_by.as_deref())?;
                    apply_lane_update_metadata(lane, &actor)?;
                    Ok(())
                },
            )
        }
        LanesCmd::TicketAdd {
            store,
            workspace,
            lane_id,
            ticket_id,
            first,
            before,
            after,
            updated_by,
            format,
        } => {
            let placement = match (first, before.as_deref(), after.as_deref()) {
                (false, None, None) => loom_lanes::LaneTicketPlacement::Append,
                (true, None, None) => loom_lanes::LaneTicketPlacement::First,
                (false, Some(anchor), None) => loom_lanes::LaneTicketPlacement::Before(anchor),
                (false, None, Some(anchor)) => loom_lanes::LaneTicketPlacement::After(anchor),
                _ => {
                    return Err("at most one of --first, --before, --after may be set".to_string());
                }
            };
            mutate_lane(
                &store,
                &workspace,
                &lane_id,
                keys,
                &format,
                "lane.ticket_added",
                vec![MutationChange::relation_set(
                    ticket_id.clone(),
                    "lane_ticket",
                    ticket_id.clone(),
                )],
                |lane, loom, ns| {
                    loom_lanes::place_lane_ticket(lane, &ticket_id, placement)
                        .map_err(|e| e.to_string())?;
                    let actor = resolve_lane_actor(loom, ns, updated_by.as_deref())?;
                    apply_lane_update_metadata(lane, &actor)?;
                    Ok(())
                },
            )
        }
        LanesCmd::TicketRemove {
            store,
            workspace,
            lane_id,
            ticket_id,
            updated_by,
            format,
        } => mutate_lane(
            &store,
            &workspace,
            &lane_id,
            keys,
            &format,
            "lane.ticket_removed",
            vec![MutationChange::relation_removed(
                ticket_id.clone(),
                "lane_ticket",
                ticket_id.clone(),
            )],
            |lane, loom, ns| {
                lane.lane_tickets
                    .retain(|lane_ticket| lane_ticket.ticket_id != ticket_id);
                if lane.active_ticket_id.as_deref() == Some(&ticket_id) {
                    lane.active_ticket_id = None;
                }
                let actor = resolve_lane_actor(loom, ns, updated_by.as_deref())?;
                apply_lane_update_metadata(lane, &actor)?;
                Ok(())
            },
        ),
        LanesCmd::TicketTransfer {
            store,
            workspace,
            source_lane_id,
            target_lane_id,
            ticket_id,
            updated_by,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let (_source, target) = loom_lanes::transfer_assignment_lane_ticket(
                &mut loom,
                workspace_id,
                &source_lane_id,
                &target_lane_id,
                &ticket_id,
                current_time_ms()?,
                &updated_by,
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            let envelope = lane_mutation_envelope(
                target,
                "lane.ticket_transferred",
                vec![MutationChange::relation_set(
                    ticket_id.clone(),
                    "lane_ticket",
                    ticket_id.clone(),
                )],
            );
            print_lane_mutation(&envelope, &format)
        }
        LanesCmd::Delete {
            store,
            workspace,
            lane_id,
            updated_by,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let lane = loom_lanes::delete_lane(
                &mut loom,
                workspace_id,
                &lane_id,
                current_time_ms()?,
                &updated_by,
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            let envelope =
                lane_mutation_envelope(lane, "lane.deleted", vec![MutationChange::ResourceDeleted]);
            print_lane_mutation(&envelope, &format)
        }
    }
}

fn mutate_lane<F>(
    store: &str,
    workspace: &str,
    lane_id: &str,
    keys: &KeyOpts,
    format: &str,
    operation: &str,
    changes: Vec<MutationChange>,
    mutate: F,
) -> Result<(), String>
where
    F: FnOnce(&mut Lane, &Loom<FileStore>, WorkspaceId) -> Result<(), String>,
{
    let mut loom = cli_open_loom(store, keys)?;
    let workspace_id = resolve_ns(&loom, workspace)?;
    let mut lane = loom_lanes::get_lane(&loom, workspace_id, lane_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "lane not found".to_string())?;
    mutate(&mut lane, &loom, workspace_id)?;
    let lane = loom_lanes::put_lane(&mut loom, workspace_id, lane).map_err(|e| e.to_string())?;
    save_loom(&mut loom).map_err(|e| e.to_string())?;
    let envelope = lane_mutation_envelope(lane, operation, changes);
    print_lane_mutation(&envelope, format)
}

fn apply_lane_update_metadata(lane: &mut Lane, updated_by: &str) -> Result<(), String> {
    lane.updated_at = current_time_ms()?;
    lane.updated_by = updated_by.to_string();
    Ok(())
}

/// Resolve the actor recorded on a Lane mutation from the CLI.
///
/// Routine mutations omit `--updated-by` and derive the actor from the authenticated principal,
/// falling back to the workspace namespace when unauthenticated. An explicit override is honored
/// as-is when it matches the effective principal; when it differs it is authorized through the
/// shared ACL substrate (`Tickets` domain, `Admin` right) rather than any bespoke lane-only policy.
fn resolve_lane_actor(
    loom: &Loom<FileStore>,
    workspace_id: WorkspaceId,
    provided: Option<&str>,
) -> Result<String, String> {
    let effective = loom
        .effective_principal()
        .map_err(|e| e.to_string())?
        .map(|principal| principal.to_string());
    match provided.filter(|value| !value.trim().is_empty()) {
        Some(actor) => {
            if Some(actor) != effective.as_deref() {
                loom.authorize_domain(workspace_id, AclDomain::Tickets, AclRight::Admin)
                    .map_err(|e| e.to_string())?;
            }
            Ok(actor.to_string())
        }
        None => Ok(effective.unwrap_or_else(|| workspace_id.to_string())),
    }
}

fn parse_board_columns(values: &[String]) -> Result<Vec<loom_tickets::BoardColumn>, String> {
    if values.is_empty() {
        return Err("board requires at least one --column".to_string());
    }
    values
        .iter()
        .enumerate()
        .map(|(idx, value)| {
            let mut parts = value.splitn(4, ':');
            let column_id = parts
                .next()
                .filter(|part| !part.is_empty())
                .ok_or_else(|| format!("invalid board column {value:?}"))?;
            let name = parts
                .next()
                .filter(|part| !part.is_empty())
                .ok_or_else(|| {
                    format!(
                        "invalid board column {value:?}; expected column_id:name[:statuses][:rank]"
                    )
                })?;
            let statuses = parts
                .next()
                .unwrap_or("")
                .split(',')
                .filter(|status| !status.is_empty())
                .map(str::to_string)
                .collect::<std::collections::BTreeSet<_>>();
            let rank = match parts.next() {
                Some(rank) if !rank.is_empty() => rank
                    .parse()
                    .map_err(|_| format!("invalid board column rank in {value:?}"))?,
                _ => ((idx as u64) + 1) * 100,
            };
            loom_tickets::BoardColumn::with_display(column_id, name, statuses, None, false, rank)
                .map_err(|e| e.to_string())
        })
        .collect()
}

fn run_pages(action: PagesCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        PagesCmd::SpaceCreate {
            store,
            workspace,
            space_id,
            title,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = ensure_facet_workspace(&mut loom, &workspace, FacetKind::Vcs)?;
            let profile_id = workspace_id.to_string();
            let space = loom_pages::create_space(
                &mut loom,
                workspace_id,
                &profile_id,
                &space_id,
                &title,
                expected_root.as_deref(),
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_page_space(&space, &format)
        }
        PagesCmd::SpaceList {
            store,
            workspace,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let spaces = loom_pages::list_spaces(&loom, workspace_id, &profile_id)
                .map_err(|e| e.to_string())?;
            print_page_spaces(&spaces, &format)
        }
        PagesCmd::SpaceGet {
            store,
            workspace,
            space_id,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let space = loom_pages::get_space(&loom, workspace_id, &profile_id, &space_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "space not found".to_string())?;
            print_page_space(&space, &format)
        }
        PagesCmd::Create {
            store,
            workspace,
            page_id,
            space_id,
            title,
            parent_page_id,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let page = loom_pages::create_page(
                &mut loom,
                workspace_id,
                loom_pages::PageCreateRequest {
                    workspace_id: &profile_id,
                    page_id: &page_id,
                    space_id: &space_id,
                    parent_page_id: parent_page_id.as_deref(),
                    title: &title,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_page(&page, &format)
        }
        PagesCmd::Update {
            store,
            workspace,
            page_id,
            body,
            expected_root,
            format,
        } => {
            let body = parse_page_body(&body)?;
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let update = loom_pages::update_page(
                &mut loom,
                workspace_id,
                &profile_id,
                &page_id,
                body,
                current_time_ms()?,
                expected_root.as_deref(),
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_page_update(&update, &format)
        }
        PagesCmd::Publish {
            store,
            workspace,
            page_id,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let publish = loom_pages::publish_page(
                &mut loom,
                workspace_id,
                &profile_id,
                &page_id,
                current_time_ms()?,
                expected_root.as_deref(),
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_page_publish(&publish, &format)
        }
        PagesCmd::Get {
            store,
            workspace,
            page_id,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let page = loom_pages::get_page(&loom, workspace_id, &profile_id, &page_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "page not found".to_string())?;
            print_page(&page, &format)
        }
        PagesCmd::History {
            store,
            workspace,
            page_id,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let history = loom_pages::page_history(&loom, workspace_id, &profile_id, &page_id)
                .map_err(|e| e.to_string())?;
            print_page_history(&history, &format)
        }
        PagesCmd::StructureCreate {
            store,
            workspace,
            structure_id,
            space_id,
            kind,
            title,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let render = loom_pages::create_structure(
                &mut loom,
                workspace_id,
                loom_pages::StructureCreateRequest {
                    workspace_id: &profile_id,
                    structure_id: &structure_id,
                    space_id: &space_id,
                    kind: &kind,
                    title: &title,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_page_structure_render(&render, &format)
        }
        PagesCmd::StructureGet {
            store,
            workspace,
            structure_id,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let render = loom_pages::get_structure(&loom, workspace_id, &profile_id, &structure_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "structure not found".to_string())?;
            print_page_structure_render(&render, &format)
        }
        PagesCmd::StructureAddNode {
            store,
            workspace,
            structure_id,
            node_id,
            kind,
            label,
            body_digest,
            entity_ref,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let node = loom_pages::add_structure_node(
                &mut loom,
                workspace_id,
                loom_pages::StructureNodeRequest {
                    workspace_id: &profile_id,
                    structure_id: &structure_id,
                    node_id: &node_id,
                    kind: &kind,
                    label: &label,
                    body_digest: body_digest.as_deref(),
                    entity_ref,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_page_structure_node(&node, &format)
        }
        PagesCmd::StructureUpdateNode {
            store,
            workspace,
            structure_id,
            node_id,
            kind,
            label,
            body_digest,
            entity_ref,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let node = loom_pages::update_structure_node(
                &mut loom,
                workspace_id,
                loom_pages::StructureNodeRequest {
                    workspace_id: &profile_id,
                    structure_id: &structure_id,
                    node_id: &node_id,
                    kind: &kind,
                    label: &label,
                    body_digest: body_digest.as_deref(),
                    entity_ref,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_page_structure_node(&node, &format)
        }
        PagesCmd::StructureBind {
            store,
            workspace,
            structure_id,
            node_id,
            entity_ref,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let node = loom_pages::bind_structure_node(
                &mut loom,
                workspace_id,
                loom_pages::StructureBindRequest {
                    workspace_id: &profile_id,
                    structure_id: &structure_id,
                    node_id: &node_id,
                    entity_ref,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_page_structure_node(&node, &format)
        }
        PagesCmd::StructureMoveNode {
            store,
            workspace,
            structure_id,
            node_id,
            parent_node_id,
            label,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let moved = loom_pages::move_structure_node(
                &mut loom,
                workspace_id,
                loom_pages::StructureMoveRequest {
                    workspace_id: &profile_id,
                    structure_id: &structure_id,
                    node_id: &node_id,
                    parent_node_id: parent_node_id.as_deref(),
                    label: label.as_deref(),
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_page_structure_move(&moved, &format)
        }
        PagesCmd::StructureLinkNode {
            store,
            workspace,
            structure_id,
            edge_id,
            src_node_id,
            dst_node_id,
            label,
            target_ref,
            expected_root,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let edge = loom_pages::link_structure_node(
                &mut loom,
                workspace_id,
                loom_pages::StructureLinkRequest {
                    workspace_id: &profile_id,
                    structure_id: &structure_id,
                    edge_id: &edge_id,
                    src_node_id: &src_node_id,
                    dst_node_id: &dst_node_id,
                    label: &label,
                    target_ref,
                    expected_root: expected_root.as_deref(),
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_page_structure_edge(&edge, &format)
        }
        PagesCmd::StructureDecomposeToTickets {
            store,
            workspace,
            structure_id,
            items,
            format,
        } => {
            let parsed_items = parse_page_structure_decompose_items(&items)?;
            let request_items = parsed_items
                .iter()
                .map(|item| loom_pages::StructureDecomposeItem {
                    node_id: item.node_id.as_str(),
                    project_id: item.project_id.as_str(),
                    ticket_type: item.ticket_type.as_deref(),
                    fields: item.fields.as_ref(),
                    policy_labels: &item.policy_labels,
                })
                .collect::<Vec<_>>();
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let summary = loom_pages::decompose_to_tickets(
                &mut loom,
                workspace_id,
                loom_pages::StructureDecomposeRequest {
                    workspace_id: &profile_id,
                    structure_id: &structure_id,
                    items: &request_items,
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_page_structure_decompose(&summary, &format)
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct PageStructureDecomposeItemJson {
    node_id: String,
    project_id: String,
    ticket_type: Option<String>,
    fields: Option<serde_json::Value>,
    #[serde(default)]
    policy_labels: Vec<String>,
}

fn parse_page_structure_decompose_items(
    input: &str,
) -> Result<Vec<PageStructureDecomposeItemJson>, String> {
    let bytes = if let Some(path) = input.strip_prefix('@') {
        read_input(path).map_err(|e| e.to_string())?
    } else {
        input.as_bytes().to_vec()
    };
    let items: Vec<PageStructureDecomposeItemJson> =
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    for item in &items {
        if let Some(fields) = &item.fields
            && !fields.is_object()
        {
            return Err("structure decomposition fields must be JSON objects".to_string());
        }
    }
    Ok(items)
}

fn print_page_structure_render(
    render: &loom_pages::StructureRenderSummary,
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(render).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            render.structure.structure_id,
            render.structure.space_id,
            render.structure.kind,
            render.structure.title,
            render.nodes.len(),
            render.edges.len(),
            render.graph_collection
        );
    }
    Ok(())
}

fn print_page_structure_node(
    node: &loom_pages::StructureNodeSummary,
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(node).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            node.structure_id,
            node.node_id,
            node.kind,
            node.label,
            node.entity_ref.as_deref().unwrap_or(""),
            node.profile_root
        );
    }
    Ok(())
}

fn print_page_structure_edge(
    edge: &loom_pages::StructureEdgeSummary,
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(edge).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            edge.structure_id,
            edge.edge_id,
            edge.src_node_id,
            edge.dst_node_id,
            edge.label,
            edge.profile_root
        );
    }
    Ok(())
}

fn print_page_structure_move(
    moved: &loom_pages::StructureMoveSummary,
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(moved).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            moved.structure_id,
            moved.node_id,
            moved.parent_node_id.as_deref().unwrap_or(""),
            moved.label,
            moved.profile_root
        );
    }
    Ok(())
}

fn print_page_structure_decompose(
    summary: &loom_pages::StructureDecomposeSummary,
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(summary).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            summary.workspace_id,
            summary.structure_id,
            summary.tickets.len(),
            summary.implemented_by_edges.len(),
            summary.graph_collection
        );
    }
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct LifecycleGateEvaluationJson {
    gate_id: String,
    passed: bool,
    principal_id: Option<String>,
    evidence_digest: Option<String>,
    evaluated_at_ms: Option<u64>,
}

fn run_lifecycle(action: LifecycleCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        LifecycleCmd::DefineStandard {
            store,
            workspace,
            kind,
            version,
            completion_predicate_digest,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = ensure_facet_workspace(&mut loom, &workspace, FacetKind::Vcs)?;
            let profile_id = workspace_id.to_string();
            let definition = loom_lifecycle::define_standard_lifecycle(
                &mut loom,
                workspace_id,
                loom_lifecycle::StandardLifecycleRequest {
                    workspace_id: &profile_id,
                    kind: &kind,
                    version: &version,
                    completion_predicate_digest: &completion_predicate_digest,
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_lifecycle(&definition, &format)
        }
        LifecycleCmd::Define {
            store,
            workspace,
            definition,
            format,
        } => {
            let bytes = read_input(&definition).map_err(|e| e.to_string())?;
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = ensure_facet_workspace(&mut loom, &workspace, FacetKind::Vcs)?;
            let profile_id = workspace_id.to_string();
            let definition =
                loom_lifecycle::define_lifecycle(&mut loom, workspace_id, &profile_id, &bytes)
                    .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_lifecycle(&definition, &format)
        }
        LifecycleCmd::Definitions {
            store,
            workspace,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let definitions = loom_lifecycle::list_definitions(&loom, workspace_id, &profile_id)
                .map_err(|e| e.to_string())?;
            print_lifecycle(&definitions, &format)
        }
        LifecycleCmd::Definition {
            store,
            workspace,
            definition_id,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let definition =
                loom_lifecycle::get_definition(&loom, workspace_id, &profile_id, &definition_id)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| "lifecycle definition not found".to_string())?;
            print_lifecycle(&definition, &format)
        }
        LifecycleCmd::Instantiate {
            store,
            workspace,
            instance_id,
            definition_id,
            subject_refs,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let instance = loom_lifecycle::instantiate(
                &mut loom,
                workspace_id,
                &profile_id,
                &instance_id,
                &definition_id,
                subject_refs,
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_lifecycle(&instance, &format)
        }
        LifecycleCmd::Instances {
            store,
            workspace,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let instances = loom_lifecycle::list_instances(&loom, workspace_id, &profile_id)
                .map_err(|e| e.to_string())?;
            print_lifecycle(&instances, &format)
        }
        LifecycleCmd::Instance {
            store,
            workspace,
            instance_id,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let instance =
                loom_lifecycle::get_instance(&loom, workspace_id, &profile_id, &instance_id)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| "lifecycle instance not found".to_string())?;
            print_lifecycle(&instance, &format)
        }
        LifecycleCmd::Transition {
            store,
            workspace,
            instance_id,
            transition_id,
            to_stage_id,
            actor_principal_id,
            gate_evaluations,
            snapshot_digest,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let now = current_time_ms()?;
            let gate_evaluations = parse_lifecycle_gate_evaluations(&gate_evaluations, now)?;
            let actor_principal_id = actor_principal_id.unwrap_or_else(|| workspace_id.to_string());
            let result = loom_lifecycle::transition(
                &mut loom,
                workspace_id,
                loom_lifecycle::LifecycleTransitionRequest {
                    workspace_id: &profile_id,
                    instance_id: &instance_id,
                    transition_id: &transition_id,
                    to_stage_id: &to_stage_id,
                    actor_principal_id: &actor_principal_id,
                    gate_evaluations,
                    snapshot_digest: snapshot_digest.as_deref(),
                    recorded_at_ms: now,
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_lifecycle(&result, &format)
        }
        LifecycleCmd::SnapshotPlan {
            store,
            workspace,
            instance_id,
            to_stage_id,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let plan = loom_lifecycle::snapshot_plan(
                &loom,
                workspace_id,
                &profile_id,
                &instance_id,
                &to_stage_id,
            )
            .map_err(|e| e.to_string())?;
            print_lifecycle(&plan, &format)
        }
        LifecycleCmd::CurrentSurface {
            store,
            workspace,
            instance_id,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let surface =
                loom_lifecycle::current_surface(&loom, workspace_id, &profile_id, &instance_id)
                    .map_err(|e| e.to_string())?;
            print_lifecycle(&surface, &format)
        }
        LifecycleCmd::Snapshots {
            store,
            workspace,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let snapshots = loom_lifecycle::list_snapshots(&loom, workspace_id, &profile_id)
                .map_err(|e| e.to_string())?;
            print_lifecycle(&snapshots, &format)
        }
        LifecycleCmd::Snapshot {
            store,
            workspace,
            snapshot_id,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let snapshot =
                loom_lifecycle::get_snapshot(&loom, workspace_id, &profile_id, &snapshot_id)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| "lifecycle snapshot not found".to_string())?;
            print_lifecycle(&snapshot, &format)
        }
        LifecycleCmd::SnapshotContent {
            store,
            workspace,
            snapshot_id,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let content =
                loom_lifecycle::snapshot_content(&loom, workspace_id, &profile_id, &snapshot_id)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| "lifecycle snapshot not found".to_string())?;
            write_output(out.as_deref(), &content).map_err(|e| e.to_string())
        }
        LifecycleCmd::OperationLog {
            store,
            workspace,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let log = loom_lifecycle::operation_log(&loom, workspace_id, &profile_id)
                .map_err(|e| e.to_string())?;
            print_lifecycle(&log, &format)
        }
    }
}

fn parse_lifecycle_gate_evaluations(
    input: &str,
    now_ms: u64,
) -> Result<Vec<loom_lifecycle::LifecycleGateEvaluationInput>, String> {
    let bytes = if let Some(path) = input.strip_prefix('@') {
        read_input(path).map_err(|e| e.to_string())?
    } else {
        input.as_bytes().to_vec()
    };
    let values: Vec<LifecycleGateEvaluationJson> =
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    Ok(values
        .into_iter()
        .map(|value| loom_lifecycle::LifecycleGateEvaluationInput {
            gate_id: value.gate_id,
            passed: value.passed,
            principal_id: value.principal_id,
            evidence_digest: value.evidence_digest,
            evaluated_at_ms: value.evaluated_at_ms.unwrap_or(now_ms),
        })
        .collect())
}

fn print_lifecycle<T: serde::Serialize>(value: &T, format: &str) -> Result<(), String> {
    match format {
        "json" | "text" => {
            println!(
                "{}",
                serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        other => Err(format!("unsupported lifecycle output format {other:?}")),
    }
}

fn parse_page_body(input: &str) -> Result<Vec<u8>, String> {
    let text = if let Some(path) = input.strip_prefix('@') {
        String::from_utf8(read_input(path).map_err(|e| e.to_string())?)
            .map_err(|_| "page body input must be UTF-8".to_string())?
    } else {
        input.to_string()
    };
    Body::from_plain_text(text)
        .and_then(|body| body.encode())
        .map_err(|e| e.to_string())
}

fn print_page_space(space: &loom_pages::SpaceSummary, format: &str) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(space).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}",
            space.space_id, space.title, space.archived, space.profile_root
        );
    }
    Ok(())
}

fn print_page_spaces(spaces: &[loom_pages::SpaceSummary], format: &str) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(spaces).map_err(|e| e.to_string())?
        );
    } else {
        for space in spaces {
            println!(
                "{}\t{}\t{}\t{}",
                space.space_id, space.title, space.archived, space.profile_root
            );
        }
    }
    Ok(())
}

fn print_page(page: &loom_pages::PageSummary, format: &str) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(page).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            page.page_id, page.space_id, page.title, page.status, page.profile_root
        );
    }
    Ok(())
}

fn print_page_update(update: &loom_pages::PageUpdateSummary, format: &str) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(update).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}",
            update.page_id, update.status, update.updated_at_ms, update.profile_root
        );
    }
    Ok(())
}

fn print_page_publish(
    publish: &loom_pages::PagePublishSummary,
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(publish).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}",
            publish.page_id,
            publish.outcome,
            publish
                .revision
                .map(|revision| revision.to_string())
                .unwrap_or_default(),
            publish.profile_root
        );
    }
    Ok(())
}

fn print_page_history(
    history: &[loom_pages::PageHistoryEntry],
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(history).map_err(|e| e.to_string())?
        );
    } else {
        for entry in history {
            println!(
                "{}\t{}\t{}\t{}",
                entry.kind,
                entry.page_id,
                entry
                    .revision
                    .map(|revision| revision.to_string())
                    .unwrap_or_default(),
                entry.body_digest.as_deref().unwrap_or("")
            );
        }
    }
    Ok(())
}

fn parse_ticket_fields(input: &str) -> Result<serde_json::Value, String> {
    let bytes = if let Some(path) = input.strip_prefix('@') {
        read_input(path).map_err(|e| e.to_string())?
    } else {
        input.as_bytes().to_vec()
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if !value.is_object() {
        return Err("ticket fields must be a JSON object".to_string());
    }
    Ok(value)
}

fn read_ticket_comment_body(input: &str) -> Result<String, String> {
    let bytes = if let Some(path) = input.strip_prefix('@') {
        read_input(path).map_err(|e| e.to_string())?
    } else {
        input.as_bytes().to_vec()
    };
    String::from_utf8(bytes).map_err(|_| "ticket comment body must be UTF-8".to_string())
}

fn parse_ticket_comment_evidence(
    input: &str,
) -> Result<loom_tickets::TicketCommentEvidence, String> {
    let value = parse_ticket_fields(input)?;
    loom_tickets::TicketCommentEvidence::from_json(&value).map_err(|error| error.to_string())
}

fn parse_ticket_comment_evidence_update(
    input: &str,
) -> Result<Option<loom_tickets::TicketCommentEvidence>, String> {
    let bytes = if let Some(path) = input.strip_prefix('@') {
        read_input(path).map_err(|e| e.to_string())?
    } else {
        input.as_bytes().to_vec()
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if value.is_null() {
        Ok(None)
    } else {
        loom_tickets::TicketCommentEvidence::from_json(&value)
            .map(Some)
            .map_err(|error| error.to_string())
    }
}

fn parse_ticket_field_cardinality(
    value: &str,
) -> Result<loom_tickets::TicketFieldCardinality, String> {
    match value {
        "single" => Ok(loom_tickets::TicketFieldCardinality::Single),
        "optional" => Ok(loom_tickets::TicketFieldCardinality::Optional),
        "list" => Ok(loom_tickets::TicketFieldCardinality::List {
            min_items: 0,
            max_items: None,
        }),
        _ => Err("ticket field cardinality must be single, optional, or list".to_string()),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TicketUpdateInput {
    workspace: String,
    ticket_id: String,
    #[serde(default)]
    projection: Option<String>,
    #[serde(default)]
    set_fields: Option<serde_json::Value>,
    #[serde(default)]
    delete_fields: Vec<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    target_status: Option<String>,
    #[serde(default)]
    observed_source_status: Option<String>,
    #[serde(default)]
    observed_workflow_version: Option<String>,
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    expected_root: Option<String>,
    #[serde(default)]
    comment: Option<TicketUpdateCommentInput>,
    #[serde(default)]
    comments: Vec<TicketUpdateCommentInput>,
    #[serde(default)]
    relation_sets: Vec<TicketUpdateRelationSetInput>,
    #[serde(default)]
    relation_removes: Vec<TicketUpdateRelationRemoveInput>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TicketUpdateCommentInput {
    #[serde(default)]
    comment_id: Option<String>,
    #[serde(default)]
    comment_type: Option<String>,
    body: String,
    #[serde(default)]
    evidence: Option<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TicketUpdateRelationSetInput {
    #[serde(default)]
    relation_id: Option<String>,
    kind: String,
    target_id: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TicketUpdateRelationRemoveInput {
    relation_id: String,
}

fn parse_ticket_update_request(input: &str) -> Result<TicketUpdateInput, String> {
    let bytes = if let Some(path) = input.strip_prefix('@') {
        read_input(path).map_err(|error| error.to_string())?
    } else {
        input.as_bytes().to_vec()
    };
    let request: TicketUpdateInput =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if request.set_fields.is_none()
        && request.delete_fields.is_empty()
        && request.action.is_none()
        && request.target_status.is_none()
        && request.comment.is_none()
        && request.comments.is_empty()
        && request.relation_sets.is_empty()
        && request.relation_removes.is_empty()
    {
        return Err(
            "ticket update request requires set_fields, delete_fields, action, target_status, comment, comments, relation_sets, or relation_removes"
                .to_string(),
        );
    }
    Ok(request)
}

fn read_text_arg(input: &str) -> Result<String, String> {
    if let Some(path) = input.strip_prefix('@') {
        String::from_utf8(read_input(path).map_err(|error| error.to_string())?)
            .map_err(|_| "text input must be UTF-8".to_string())
    } else {
        Ok(input.to_string())
    }
}

fn parse_ticket_update_field_value(value: &str) -> serde_json::Value {
    serde_json::from_str(value).unwrap_or_else(|_| serde_json::Value::String(value.to_string()))
}

fn parse_ticket_update_field_assignments(
    fields: &[String],
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let mut object = serde_json::Map::new();
    for field in fields {
        let (key, value) = field
            .split_once('=')
            .ok_or_else(|| format!("invalid ticket field {field:?} (expected key=value)"))?;
        if key.is_empty() {
            return Err("ticket field key cannot be empty".to_string());
        }
        object.insert(key.to_string(), parse_ticket_update_field_value(value));
    }
    Ok(object)
}

struct TicketUpdateCliParts {
    request: Option<String>,
    workspace: Option<String>,
    ticket_id: Option<String>,
    projection: Option<String>,
    status: Option<String>,
    assignee: Option<String>,
    title: Option<String>,
    description: Option<String>,
    priority: Option<String>,
    fields: Vec<String>,
    delete_fields: Vec<String>,
    action: Option<String>,
    comment_body: Option<String>,
    comment_id: Option<String>,
    comment_type: Option<String>,
    comment_evidence: Option<String>,
    observed_source_status: Option<String>,
    observed_workflow_version: Option<String>,
    expected_root: Option<String>,
}

fn ticket_update_request_from_parts(
    parts: TicketUpdateCliParts,
) -> Result<TicketUpdateInput, String> {
    let TicketUpdateCliParts {
        request,
        workspace,
        ticket_id,
        projection,
        status,
        assignee,
        title,
        description,
        priority,
        fields,
        delete_fields,
        action,
        comment_body,
        comment_id,
        comment_type,
        comment_evidence,
        observed_source_status,
        observed_workflow_version,
        expected_root,
    } = parts;
    if let Some(request) = request {
        let direct_flags_present = workspace.is_some()
            || ticket_id.is_some()
            || projection.is_some()
            || status.is_some()
            || assignee.is_some()
            || title.is_some()
            || description.is_some()
            || priority.is_some()
            || !fields.is_empty()
            || !delete_fields.is_empty()
            || action.is_some()
            || comment_body.is_some()
            || comment_id.is_some()
            || comment_type.is_some()
            || comment_evidence.is_some()
            || observed_source_status.is_some()
            || observed_workflow_version.is_some()
            || expected_root.is_some();
        if direct_flags_present {
            return Err("ticket update --request cannot be combined with direct update flags or positional workspace/ticket_id".to_string());
        }
        return parse_ticket_update_request(&request);
    }

    let workspace = workspace.ok_or_else(|| {
        "ticket update requires workspace unless --request is supplied".to_string()
    })?;
    let ticket_id = ticket_id.ok_or_else(|| {
        "ticket update requires ticket_id unless --request is supplied".to_string()
    })?;

    let mut field_object = parse_ticket_update_field_assignments(&fields)?;
    for (key, value) in [
        ("title", title),
        ("description", description),
        ("priority", priority),
    ] {
        if let Some(value) = value {
            if field_object.contains_key(key) {
                return Err(format!(
                    "canonical field `{key}` was provided by both --{key} and --field"
                ));
            }
            field_object.insert(key.to_string(), serde_json::Value::String(value));
        }
    }
    let set_fields = (!field_object.is_empty()).then_some(serde_json::Value::Object(field_object));
    if comment_body.is_none()
        && (comment_id.is_some() || comment_type.is_some() || comment_evidence.is_some())
    {
        return Err(
            "--comment-id, --comment-type, and --comment-evidence require --comment-body"
                .to_string(),
        );
    }
    let comment_evidence = comment_evidence
        .as_deref()
        .map(parse_ticket_comment_evidence)
        .transpose()?;
    let comment_evidence = comment_evidence
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| error.to_string())?;
    let comment = comment_body
        .map(|body| {
            read_text_arg(&body).map(|body| TicketUpdateCommentInput {
                comment_id,
                comment_type,
                body,
                evidence: comment_evidence,
            })
        })
        .transpose()?;
    let direct_request = TicketUpdateInput {
        workspace,
        ticket_id,
        projection,
        set_fields,
        delete_fields,
        action,
        target_status: status,
        observed_source_status,
        observed_workflow_version,
        assignee,
        expected_root,
        comment,
        comments: Vec::new(),
        relation_sets: Vec::new(),
        relation_removes: Vec::new(),
    };
    if direct_request.set_fields.is_none()
        && direct_request.delete_fields.is_empty()
        && direct_request.action.is_none()
        && direct_request.target_status.is_none()
        && direct_request.assignee.is_none()
        && direct_request.comment.is_none()
        && direct_request.comments.is_empty()
        && direct_request.relation_sets.is_empty()
        && direct_request.relation_removes.is_empty()
    {
        return Err("ticket update requires at least one update flag: --status, --assignee, --title, --description, --priority, --field, --delete-field, --action, or --comment-body".to_string());
    }
    Ok(direct_request)
}

fn update_ticket_reference_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    ticket: &loom_tickets::TicketSummary,
) -> Result<(), String> {
    loom_tickets::update_ticket_field_references(
        loom,
        workspace,
        &ticket.workspace_id,
        &ticket.ticket_id,
        &ticket.fields,
    )
    .map_err(|e| e.to_string())?;
    let Some(operation_id) = ticket.operation_id.as_deref() else {
        return Ok(());
    };
    let source_root = Digest::parse(&ticket.profile_root).map_err(|e| e.to_string())?;
    loom_tickets::enqueue_ticket_reference_candidates(
        loom,
        workspace,
        loom_tickets::TicketReferenceCandidateRequest {
            workspace_id: &ticket.workspace_id,
            ticket_id: &ticket.ticket_id,
            operation_id,
            source_root,
            fields: &ticket.fields,
            now_ms: current_time_ms()?,
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod ticket_update_cli_tests {
    use super::*;

    #[test]
    fn ticket_update_direct_flags_build_typed_request() {
        let request = ticket_update_request_from_parts(TicketUpdateCliParts {
            request: None,
            workspace: Some("main".to_string()),
            ticket_id: Some("CORE-1".to_string()),
            projection: Some("jira".to_string()),
            status: Some("in_progress".to_string()),
            assignee: Some("agent:1".to_string()),
            title: Some("Direct update".to_string()),
            description: None,
            priority: Some("high".to_string()),
            fields: vec!["component=cli".to_string(), "points=3".to_string()],
            delete_fields: vec!["obsolete".to_string()],
            action: None,
            comment_body: None,
            comment_id: None,
            comment_type: None,
            comment_evidence: None,
            observed_source_status: None,
            observed_workflow_version: None,
            expected_root: Some("root-before".to_string()),
        })
        .unwrap();

        assert_eq!(request.workspace, "main");
        assert_eq!(request.ticket_id, "CORE-1");
        assert_eq!(request.target_status.as_deref(), Some("in_progress"));
        assert_eq!(request.assignee.as_deref(), Some("agent:1"));
        assert_eq!(request.delete_fields, vec!["obsolete"]);
        assert_eq!(request.expected_root.as_deref(), Some("root-before"));
        let fields = request.set_fields.unwrap();
        assert_eq!(fields["title"], "Direct update");
        assert_eq!(fields["priority"], "high");
        assert_eq!(fields["component"], "cli");
        assert_eq!(fields["points"], 3);
    }

    #[test]
    fn ticket_update_request_rejects_mixed_input_modes() {
        let error = ticket_update_request_from_parts(TicketUpdateCliParts {
            request: Some(
                r#"{"workspace":"main","ticket_id":"CORE-1","target_status":"done"}"#.to_string(),
            ),
            workspace: Some("main".to_string()),
            ticket_id: None,
            projection: None,
            status: None,
            assignee: None,
            title: None,
            description: None,
            priority: None,
            fields: Vec::new(),
            delete_fields: Vec::new(),
            action: None,
            comment_body: None,
            comment_id: None,
            comment_type: None,
            comment_evidence: None,
            observed_source_status: None,
            observed_workflow_version: None,
            expected_root: None,
        })
        .unwrap_err();

        assert!(error.contains("--request cannot be combined"));
    }

    #[test]
    fn ticket_update_request_accepts_composable_comments_and_relations() {
        let request = parse_ticket_update_request(
            r#"{
                "workspace":"main",
                "ticket_id":"CORE-1",
                "target_status":"blocked",
                "comments":[{
                    "comment_id":"blocked",
                    "comment_type":"blocker",
                    "body":"Blocked",
                    "evidence":{"source_anchors":["crates/loom-cli/src/main.rs:1"]}
                }],
                "relation_sets":[{"relation_id":"dependency","kind":"depends_on","target_id":"CORE-2"}],
                "relation_removes":[{"relation_id":"old-dependency"}]
            }"#,
        )
        .unwrap();

        assert_eq!(request.target_status.as_deref(), Some("blocked"));
        assert_eq!(request.comments.len(), 1);
        assert_eq!(request.comments[0].comment_id.as_deref(), Some("blocked"));
        assert_eq!(
            request.comments[0].evidence.as_ref().unwrap()["source_anchors"][0],
            "crates/loom-cli/src/main.rs:1"
        );
        assert_eq!(request.relation_sets.len(), 1);
        assert_eq!(request.relation_sets[0].kind, "depends_on");
        assert_eq!(request.relation_sets[0].target_id, "CORE-2");
        assert_eq!(request.relation_removes.len(), 1);
        assert_eq!(request.relation_removes[0].relation_id, "old-dependency");
    }
}

fn current_time_ms() -> Result<u64, String> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?;
    Ok(duration.as_millis() as u64)
}

fn print_ticket_project(
    project: &loom_tickets::TicketProjectSummary,
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(project).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            project.project_id,
            project.key_prefix,
            project.name,
            project.lifecycle_authorization_policy,
            project.profile_root
        );
    }
    Ok(())
}

fn print_lane(lane: &Lane, format: &str) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&loom_lanes::public_lane(lane))
                .map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            lane.lane_id,
            lane.lane_key,
            lane.title,
            lane.lane_kind,
            lane.owner_principal.as_deref().unwrap_or(""),
            lane.lane_status,
            lane.active_ticket_id.as_deref().unwrap_or("")
        );
    }
    Ok(())
}

fn lane_mutation_envelope(
    lane: Lane,
    operation: &str,
    changes: Vec<MutationChange>,
) -> MutationEnvelope<loom_lanes::PublicLane> {
    let resource = loom_lanes::public_lane(&lane);
    let receipt =
        MutationReceipt::new(operation, "lane", resource.lane_id.clone()).changes(changes);
    MutationEnvelope::new(resource, receipt)
}

fn print_lane_mutation(
    envelope: &MutationEnvelope<loom_lanes::PublicLane>,
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(envelope).map_err(|e| e.to_string())?
        );
    } else {
        let receipt = &envelope.receipt;
        println!("operation={}", receipt.operation);
        println!("resource_kind={}", receipt.resource_kind);
        println!("resource_id={}", receipt.resource_id);
        print_mutation_changes(&receipt.changes)?;
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            envelope.resource.lane_id,
            envelope.resource.lane_key,
            envelope.resource.title,
            envelope.resource.lane_kind,
            envelope.resource.owner_principal.as_deref().unwrap_or(""),
            envelope.resource.lane_status,
            envelope.resource.active_ticket_id.as_deref().unwrap_or("")
        );
    }
    Ok(())
}

fn build_lane_view(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    ticket_workspace_id: &str,
    lane: &Lane,
) -> LaneView {
    let ticket_views = lane
        .lane_tickets
        .iter()
        .map(|lane_ticket| {
            let ticket = loom_tickets::get_ticket(
                loom,
                workspace,
                ticket_workspace_id,
                &lane_ticket.ticket_id,
            )
            .ok()
            .flatten();
            LaneTicketView {
                ticket_id: lane_ticket.ticket_id.clone(),
                status: ticket
                    .as_ref()
                    .and_then(|ticket| ticket_field_text(ticket, "status")),
                priority: ticket
                    .as_ref()
                    .and_then(|ticket| ticket_field_text(ticket, "priority")),
                title: ticket.as_ref().and_then(ticket_title_text),
            }
        })
        .collect();
    let mut view = loom_lanes::lane_view(lane, ticket_views);
    // resolve the lane owner's display alias at the projection layer using the shared
    // ticket-service resolver (loom-lanes cannot see the identity store).
    view.owner_display = view
        .owner_principal
        .as_deref()
        .map(|id| loom_tickets::resolve_principal_display(loom.identity_store(), id));
    view
}

fn ticket_field_text(ticket: &loom_tickets::TicketSummary, field: &str) -> Option<String> {
    ticket.fields.get(field).and_then(|value| match value {
        serde_json::Value::String(value) => Some(value.clone()),
        _ => None,
    })
}

fn ticket_title_text(ticket: &loom_tickets::TicketSummary) -> Option<String> {
    ticket_field_text(ticket, "title").or_else(|| ticket_field_text(ticket, "summary"))
}

fn print_lane_view(view: &LaneView, format: &str, detailed: bool) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(view).map_err(|e| e.to_string())?
        );
    } else {
        print_lane_view_text(view, detailed);
    }
    Ok(())
}

/// The `lanes list` JSON payload: healthy lane views plus one diagnostic per record that failed to
/// decode, so malformed coordination records surface instead of being dropped.
fn lane_list_json_payload(
    views: &[LaneView],
    diagnostics: &[LaneDecodeDiagnostic],
) -> serde_json::Value {
    serde_json::json!({ "lanes": views, "diagnostics": diagnostics })
}

/// One fail-soft decode diagnostic rendered as a tab-separated text line for `lanes list`.
fn lane_diagnostic_text_line(diagnostic: &LaneDecodeDiagnostic) -> String {
    format!("diagnostic\t{}\t{}", diagnostic.lane_id, diagnostic.error)
}

fn print_lane_views(
    views: &[LaneView],
    diagnostics: &[LaneDecodeDiagnostic],
    format: &str,
    detailed: bool,
) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&lane_list_json_payload(views, diagnostics))
                .map_err(|e| e.to_string())?
        );
    } else {
        for view in views {
            print_lane_view_text(view, detailed);
        }
        for diagnostic in diagnostics {
            println!("{}", lane_diagnostic_text_line(diagnostic));
        }
    }
    Ok(())
}

fn print_lane_view_text(view: &LaneView, detailed: bool) {
    let tickets = view
        .lane_tickets
        .iter()
        .map(
            |ticket| match (&ticket.status, &ticket.priority, &ticket.title) {
                (Some(status), Some(priority), Some(title)) => {
                    format!("{} [{} {}] {}", ticket.ticket_id, status, priority, title)
                }
                (Some(status), _, Some(title)) => {
                    format!("{} [{}] {}", ticket.ticket_id, status, title)
                }
                (Some(status), _, _) => format!("{} [{}]", ticket.ticket_id, status),
                (_, _, Some(title)) => format!("{} {}", ticket.ticket_id, title),
                _ => ticket.ticket_id.clone(),
            },
        )
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "{}\t{}\t{}\t{}",
        view.lane_key, view.display_status, view.title, tickets
    );
    if detailed {
        println!(
            "stored_status={}\tlane_kind={}\towner={}\tupdated_at={}\tupdated_by={}",
            view.stored_lane_status,
            view.lane_kind,
            view.owner_principal.as_deref().unwrap_or(""),
            view.updated_at,
            view.updated_by
        );
        if !view.status_report.is_empty() {
            println!("status_report={}", view.status_report);
        }
        if !view.reviewer_feedback.is_empty() {
            println!("reviewer_feedback={}", view.reviewer_feedback);
        }
    }
}

fn print_ticket(ticket: &loom_tickets::TicketSummary, format: &str) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(ticket).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            ticket.primary_key,
            ticket.ticket_id,
            ticket.project_id,
            ticket.ticket_type,
            ticket.projection_profile,
            ticket.profile_root
        );
    }
    Ok(())
}

fn print_ticket_comments(
    comments: &[loom_tickets::TicketComment],
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(comments).map_err(|e| e.to_string())?
        );
    } else {
        for comment in comments {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}",
                comment.comment_id,
                comment.comment_type,
                comment.author_principal,
                comment.created_at_ms,
                comment.updated_at_ms.unwrap_or(0),
                comment.redacted
            );
        }
    }
    Ok(())
}

fn print_ticket_detail(
    ticket: &loom_tickets::TicketSummary,
    history: &[loom_tickets::TicketHistoryRecord],
    comments: &[loom_tickets::TicketComment],
    detailed: bool,
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        if detailed {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "ticket": ticket,
                    "comments": comments,
                    "history": history
                }))
                .map_err(|e| e.to_string())?
            );
            return Ok(());
        }
        return print_ticket(ticket, format);
    }
    if detailed {
        println!("ticket");
        println!(
            "{}",
            serde_json::to_string_pretty(ticket).map_err(|e| e.to_string())?
        );
        println!("comments");
        println!(
            "{}",
            serde_json::to_string_pretty(comments).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    println!("key\t{}", ticket.primary_key);
    println!(
        "title\t{}",
        ticket_field_text(ticket, "title").unwrap_or_default()
    );
    println!(
        "status\t{}",
        ticket_field_text(ticket, "status").unwrap_or_default()
    );
    println!(
        "priority\t{}",
        ticket_field_text(ticket, "priority").unwrap_or_default()
    );
    println!("type\t{}", ticket.ticket_type);
    let assignee = ticket_field_text(ticket, "assignee").unwrap_or_default();
    match ticket_field_text(ticket, "assignee_display") {
        Some(display) if display != assignee => {
            println!("assignee\t{assignee} ({display})");
        }
        _ => println!("assignee\t{assignee}"),
    }
    println!("project\t{}", ticket.project_id);
    println!(
        "description\t{}",
        ticket_field_text(ticket, "description").unwrap_or_default()
    );
    println!("depends_on\t{}", compact_string_list(&ticket.depends_on));
    println!("blocks\t{}", compact_string_list(&ticket.blocks));
    println!("relations\t{}", compact_relation_summary(&ticket.relations));
    println!("comments\t{}", comments.len());
    if let Some(latest) = latest_ticket_update(history) {
        println!("latest_update_actor\t{}", latest.actor);
        println!("latest_update_at_ms\t{}", latest.timestamp_ms);
        println!("latest_update_operation\t{}", latest.operation_kind);
        println!("latest_update_sequence\t{}", latest.sequence);
    }
    Ok(())
}

struct TicketUpdateView {
    actor: String,
    timestamp_ms: u64,
    operation_kind: String,
    sequence: u64,
}

fn latest_ticket_update(history: &[loom_tickets::TicketHistoryRecord]) -> Option<TicketUpdateView> {
    history
        .iter()
        .max_by_key(|record| record.sequence)
        .map(|record| {
            let envelope = &record.envelope;
            TicketUpdateView {
                actor: envelope
                    .get("actor_principal")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                timestamp_ms: envelope
                    .get("timestamp_ms")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                operation_kind: record.operation_kind.clone(),
                sequence: record.sequence,
            }
        })
}

fn compact_string_list(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(",")
    }
}

fn compact_relation_summary(relations: &[loom_tickets::TicketRelationCompact]) -> String {
    if relations.is_empty() {
        return "none".to_string();
    }
    relations
        .iter()
        .map(|relation| format!("{}:{}", relation.kind, relation.target_id))
        .collect::<Vec<_>>()
        .join(",")
}

fn ticket_field_value_changes(fields: &serde_json::Value) -> Vec<MutationChange> {
    fields.as_object().map_or_else(Vec::new, |fields| {
        fields
            .iter()
            .map(|(field, value)| MutationChange::field_set(field.clone(), value.to_string()))
            .collect()
    })
}

struct CliTicketUpdateChangeInputs<'a> {
    set_fields: Option<&'a serde_json::Value>,
    delete_fields: &'a [String],
    action_applied: bool,
    target_status: Option<&'a str>,
    observed_source_status: Option<&'a str>,
    assignee: Option<&'a str>,
    comment: Option<&'a TicketUpdateCommentInput>,
    comments: &'a [TicketUpdateCommentInput],
    relation_sets: &'a [TicketUpdateRelationSetInput],
    relation_removes: &'a [TicketUpdateRelationRemoveInput],
}

fn cli_ticket_update_changes(input: CliTicketUpdateChangeInputs<'_>) -> Vec<MutationChange> {
    let mut changes = input
        .set_fields
        .map(ticket_field_value_changes)
        .unwrap_or_default();
    changes.extend(
        input
            .delete_fields
            .iter()
            .map(|field| MutationChange::field_deleted(field.clone(), None::<String>)),
    );
    if let Some(target_status) = input.target_status {
        changes.push(MutationChange::field_changed(
            "status",
            input.observed_source_status.map(str::to_string),
            Some(target_status.to_string()),
        ));
    }
    if let Some(assignee) = input.assignee {
        changes.push(MutationChange::field_changed(
            "assignee",
            None::<String>,
            Some(assignee.to_string()),
        ));
    }
    if input.action_applied && input.target_status.is_none() {
        changes.push(MutationChange::field_set("lifecycle_action", "applied"));
    }
    if let Some(comment) = input.comment {
        changes.push(MutationChange::field_set(
            "comment",
            comment.comment_type.as_deref().unwrap_or("general"),
        ));
    }
    changes.extend(input.comments.iter().map(|comment| {
        MutationChange::field_set(
            "comment",
            comment.comment_type.as_deref().unwrap_or("general"),
        )
    }));
    changes.extend(input.relation_sets.iter().map(|relation| {
        MutationChange::relation_set(
            relation
                .relation_id
                .clone()
                .unwrap_or_else(|| "default".to_string()),
            relation.kind.clone(),
            relation.target_id.clone(),
        )
    }));
    changes.extend(input.relation_removes.iter().map(|relation| {
        MutationChange::field_deleted(format!("relation:{}", relation.relation_id), None::<String>)
    }));
    changes
}

fn ticket_mutation_envelope(
    ticket: loom_tickets::TicketSummary,
    operation: &str,
    root_before: Option<&str>,
    changes: Vec<MutationChange>,
) -> MutationEnvelope<loom_tickets::TicketSummary> {
    let receipt = MutationReceipt::new(operation, "ticket", ticket.primary_key.clone())
        .operation_id(ticket.operation_id.clone())
        .roots(
            root_before.map(str::to_string),
            Some(ticket.profile_root.clone()),
        )
        .changes(changes);
    MutationEnvelope::new(ticket, receipt)
}

fn relation_mutation_envelope(
    relation: loom_tickets::TicketRelationSummary,
    operation: &str,
    root_before: Option<&str>,
    changes: Vec<MutationChange>,
) -> MutationEnvelope<loom_tickets::TicketRelationSummary> {
    let receipt = MutationReceipt::new(operation, "ticket_relation", relation.relation_id.clone())
        .operation_id(Some(relation.operation_id.clone()))
        .roots(
            root_before.map(str::to_string),
            Some(relation.profile_root.clone()),
        )
        .changes(changes);
    MutationEnvelope::new(relation, receipt)
}

fn print_mutation_changes(changes: &[MutationChange]) -> Result<(), String> {
    if changes.is_empty() {
        println!("change=[]");
        return Ok(());
    }
    for change in changes {
        println!(
            "change={}",
            serde_json::to_string(change).map_err(|e| e.to_string())?
        );
    }
    Ok(())
}

fn print_ticket_mutation(
    envelope: &MutationEnvelope<loom_tickets::TicketSummary>,
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(envelope).map_err(|e| e.to_string())?
        );
    } else {
        let receipt = &envelope.receipt;
        println!("operation={}", receipt.operation);
        println!("resource_kind={}", receipt.resource_kind);
        println!("resource_id={}", receipt.resource_id);
        println!(
            "operation_id={}",
            receipt.operation_id.as_deref().unwrap_or("")
        );
        println!(
            "root_before={}",
            receipt.root_before.as_deref().unwrap_or("")
        );
        println!("root_after={}", receipt.root_after.as_deref().unwrap_or(""));
        print_mutation_changes(&receipt.changes)?;
        print_ticket(&envelope.resource, format)?;
    }
    Ok(())
}

fn print_ticket_relation_mutation(
    envelope: &MutationEnvelope<loom_tickets::TicketRelationSummary>,
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(envelope).map_err(|e| e.to_string())?
        );
    } else {
        let receipt = &envelope.receipt;
        println!("operation={}", receipt.operation);
        println!("resource_kind={}", receipt.resource_kind);
        println!("resource_id={}", receipt.resource_id);
        println!(
            "operation_id={}",
            receipt.operation_id.as_deref().unwrap_or("")
        );
        println!(
            "root_before={}",
            receipt.root_before.as_deref().unwrap_or("")
        );
        println!("root_after={}", receipt.root_after.as_deref().unwrap_or(""));
        print_mutation_changes(&receipt.changes)?;
        print_ticket_relation(&envelope.resource, format)?;
    }
    Ok(())
}

fn print_board(board: &loom_tickets::BoardSummary, format: &str) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(board).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            board.board_key,
            board.board_id,
            board.name,
            board.project_id,
            board.mode,
            board.board_status,
            board.profile_root
        );
    }
    Ok(())
}

fn print_boards(boards: &[loom_tickets::BoardSummary], format: &str) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(boards).map_err(|e| e.to_string())?
        );
    } else {
        for board in boards {
            println!(
                "{}\t{}\t{}\t{}\t{}",
                board.board_key, board.board_id, board.name, board.mode, board.board_status
            );
        }
    }
    Ok(())
}

fn print_ticket_relation(
    relation: &loom_tickets::TicketRelationSummary,
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(relation).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            relation.ticket_id,
            relation.relation_id,
            relation.kind,
            relation.target_type,
            relation.target_id,
            relation.graph_edge_id
        );
    }
    Ok(())
}

fn print_ticket_relations(
    relations: &[loom_tickets::TicketRelationView],
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        let items = relations
            .iter()
            .map(|relation| {
                serde_json::json!({
                    "direction": relation.direction,
                    "kind": relation.kind,
                    "target_ticket_id": relation.target_ticket_id,
                    "target_title": relation.target_title,
                })
            })
            .collect::<Vec<_>>();
        let payload = serde_json::json!({ "relations": items });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?
        );
    } else if relations.is_empty() {
        println!("(no ticket relations)");
    } else {
        for relation in relations {
            println!(
                "{}\t{}\t{}\t{}",
                relation.direction, relation.kind, relation.target_ticket_id, relation.target_title
            );
        }
    }
    Ok(())
}

fn print_ticket_projects(
    projects: &[loom_tickets::TicketProject],
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        let items = projects
            .iter()
            .map(|project| {
                serde_json::json!({
                    "project_id": project.project_id,
                    "key_prefix": project.key_prefix,
                    "name": project.name,
                    "next_ticket_number": project.next_ticket_number,
                    "default_projection": project
                        .projection_config
                        .default_display_projection
                        .profile_id(),
                })
            })
            .collect::<Vec<_>>();
        let payload = serde_json::json!({ "projects": items });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?
        );
    } else if projects.is_empty() {
        println!("(no ticket projects)");
    } else {
        for project in projects {
            println!(
                "{}\t{}\t{}\tdefault_projection={}",
                project.project_id,
                project.key_prefix,
                project.name,
                project
                    .projection_config
                    .default_display_projection
                    .profile_id()
            );
        }
    }
    Ok(())
}

fn print_ticket_field_catalog(
    catalog: &loom_tickets::TicketFieldCatalog,
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(catalog).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "projection\t{}\noperation\t{}\nstrict_unknown_fields\t{}\ncustom_fields_source\t{}",
            catalog.projection_profile,
            catalog.operation,
            catalog.strict_unknown_fields,
            catalog.custom_fields_source
        );
        for field in &catalog.fields {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}",
                field.native_field,
                field.write_path,
                field.field_type,
                field.cardinality,
                field
                    .max_length
                    .map_or_else(String::new, |value| value.to_string()),
                field.enum_values.join(",")
            );
        }
    }
    Ok(())
}

fn print_ticket_page(page: &loom_tickets::TicketListPage, format: &str) -> Result<(), String> {
    if format == "json" {
        let value = serde_json::json!({
            "items": page.items,
            "total": page.total,
            "next_cursor": page.next_cursor,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        for ticket in &page.items {
            println!(
                "{}\t{}\t{}\t{}",
                ticket.primary_key, ticket.ticket_id, ticket.project_id, ticket.ticket_type
            );
        }
        if let Some(cursor) = &page.next_cursor {
            println!("next_cursor\t{cursor}");
        }
    }
    Ok(())
}

fn print_ticket_history(
    history: &[loom_tickets::TicketHistoryRecord],
    detailed: bool,
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(history).map_err(|e| e.to_string())?
        );
    } else if detailed {
        println!(
            "{}",
            serde_json::to_string_pretty(history).map_err(|e| e.to_string())?
        );
    } else {
        for record in history {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}",
                record
                    .envelope
                    .get("timestamp_ms")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                record
                    .envelope
                    .get("actor_principal")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(""),
                record.operation_kind,
                ticket_history_summary(record),
                record.sequence,
                record.operation_id
            );
        }
    }
    Ok(())
}

fn ticket_history_summary(record: &loom_tickets::TicketHistoryRecord) -> String {
    if let Some(status) = record
        .envelope
        .pointer("/payload/status")
        .and_then(serde_json::Value::as_str)
    {
        return format!("status={status}");
    }
    if let Some(target_status) = record
        .envelope
        .pointer("/payload/target_status")
        .and_then(serde_json::Value::as_str)
    {
        return format!("status={target_status}");
    }
    record
        .target_entity_id
        .as_ref()
        .map(|target| format!("target={target}"))
        .unwrap_or_default()
}

fn meeting_summary_json(meeting: &MeetingRecord) -> serde_json::Value {
    serde_json::json!({
        "meeting_id": &meeting.meeting_id,
        "title": &meeting.title,
        "starts_at_ms": meeting.starts_at_ms,
        "ends_at_ms": meeting.ends_at_ms,
        "status": meeting_status_label(meeting.status),
        "source_refs": &meeting.source_refs,
        "updated_at_ms": meeting.updated_at_ms,
    })
}

fn meeting_detail_json(
    workspace_id: &str,
    meeting: &MeetingRecord,
    annotations: &[AnnotationRecord],
) -> serde_json::Value {
    let meeting_annotations = annotations
        .iter()
        .filter(|annotation| annotation.meeting_id == meeting.meeting_id)
        .map(annotation_json)
        .collect::<Vec<_>>();
    serde_json::json!({
        "workspace_id": workspace_id,
        "meeting_id": &meeting.meeting_id,
        "title": &meeting.title,
        "starts_at_ms": meeting.starts_at_ms,
        "ends_at_ms": meeting.ends_at_ms,
        "calendar_event_ref": &meeting.calendar_event_ref,
        "owner_principal": &meeting.owner_principal,
        "attendee_refs": &meeting.attendee_refs,
        "folder_refs": &meeting.folder_refs,
        "source_refs": &meeting.source_refs,
        "current_source_digest": meeting.current_source_digest.to_string(),
        "summary_ref": &meeting.summary_ref,
        "status": meeting_status_label(meeting.status),
        "created_at_ms": meeting.created_at_ms,
        "updated_at_ms": meeting.updated_at_ms,
        "annotations": meeting_annotations,
    })
}

fn annotation_json(annotation: &AnnotationRecord) -> serde_json::Value {
    serde_json::json!({
        "annotation_id": &annotation.annotation_id,
        "meeting_id": &annotation.meeting_id,
        "source_span_ids": &annotation.source_span_ids,
        "kind": &annotation.kind,
        "label": &annotation.label,
        "normalized_id": &annotation.normalized_id,
        "confidence_ppm": annotation.confidence_ppm,
        "evidence_digest": annotation.evidence_digest.map(|digest| digest.to_string()),
        "extractor": &annotation.extractor,
        "status": annotation_status_label(annotation.status),
        "created_at_ms": annotation.created_at_ms,
        "accepted_by": &annotation.accepted_by,
        "accepted_at_ms": annotation.accepted_at_ms,
    })
}

fn annotation_status_label(status: AnnotationStatus) -> &'static str {
    match status {
        AnnotationStatus::Observed => "observed",
        AnnotationStatus::Suggested => "suggested",
        AnnotationStatus::Accepted => "accepted",
        AnnotationStatus::Rejected => "rejected",
        AnnotationStatus::Superseded => "superseded",
        AnnotationStatus::Merged => "merged",
    }
}

fn meeting_status_label(status: MeetingStatus) -> &'static str {
    match status {
        MeetingStatus::Active => "active",
        MeetingStatus::DeletedAtSource => "deleted-at-source",
        MeetingStatus::Redacted => "redacted",
        MeetingStatus::RetainedMetadataOnly => "retained-metadata-only",
    }
}

fn print_meetings_json_or_table(
    format: &str,
    body: &serde_json::Value,
    table_columns: &[&str],
) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(body).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" if table_columns.is_empty() => {
            println!(
                "{}",
                serde_json::to_string_pretty(body).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            let Some(meetings) = body.get("meetings").and_then(|value| value.as_array()) else {
                return Err("meetings output is not a list".to_string());
            };
            println!("{}", table_columns.join("\t"));
            for meeting in meetings {
                let row = table_columns
                    .iter()
                    .map(|column| {
                        meeting
                            .get(*column)
                            .and_then(|value| value.as_str())
                            .unwrap_or("")
                            .to_string()
                    })
                    .collect::<Vec<_>>();
                println!("{}", row.join("\t"));
            }
            Ok(())
        }
        other => Err(format!("unsupported meetings output format {other:?}")),
    }
}

fn run_queue(action: QueueCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        QueueCmd::Append {
            store,
            workspace,
            stream,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            let seq = client.queue_append(keys, &workspace, &stream, bytes)?;
            println!("{seq}");
            Ok(())
        }
        QueueCmd::Advance {
            store,
            workspace,
            stream,
            consumer,
            next,
        } => {
            let client = remote::open_store_client(&store)?;
            client.queue_consumer_advance(keys, &workspace, &stream, &consumer, next)
        }
        QueueCmd::Get {
            store,
            workspace,
            stream,
            seq,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(bytes) = client.queue_get(keys, &workspace, &stream, seq)? else {
                return Err(format!("queue sequence {seq} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        QueueCmd::Len {
            store,
            workspace,
            stream,
        } => {
            let client = remote::open_store_client(&store)?;
            println!("{}", client.queue_len(keys, &workspace, &stream)?);
            Ok(())
        }
        QueueCmd::Position {
            store,
            workspace,
            stream,
            consumer,
        } => {
            let client = remote::open_store_client(&store)?;
            println!(
                "{}",
                client.queue_consumer_position(keys, &workspace, &stream, &consumer)?
            );
            Ok(())
        }
        QueueCmd::Range {
            store,
            workspace,
            stream,
            from,
            to,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let entries = client.queue_range(keys, &workspace, &stream, from as u64, to as u64)?;
            write_output(out.as_deref(), &bytes_array_cbor(&entries)?).map_err(|e| e.to_string())
        }
        QueueCmd::Read {
            store,
            workspace,
            stream,
            consumer,
            max,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let entries = client.queue_consumer_read(keys, &workspace, &stream, &consumer, max)?;
            write_output(out.as_deref(), &bytes_array_cbor(&entries)?).map_err(|e| e.to_string())
        }
        QueueCmd::Reset {
            store,
            workspace,
            stream,
            consumer,
            next,
        } => {
            let client = remote::open_store_client(&store)?;
            client.queue_consumer_reset(keys, &workspace, &stream, &consumer, next)
        }
    }
}

fn run_time_series(action: TimeSeriesCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        TimeSeriesCmd::Get {
            store,
            workspace,
            series,
            timestamp,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(bytes) = client.ts_get(keys, &workspace, &series, timestamp)? else {
                return Err(format!("time-series point {timestamp} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        TimeSeriesCmd::Latest {
            store,
            workspace,
            series,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let mut result = loom_core::Series::new();
            if let Some((timestamp, bytes)) =
                loom_core::ts_latest(&loom, ns, &series).map_err(|e| e.to_string())?
            {
                result.put(timestamp, bytes);
            }
            write_output(out.as_deref(), &result.encode()).map_err(|e| e.to_string())
        }
        TimeSeriesCmd::Put {
            store,
            workspace,
            series,
            timestamp,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            client.ts_put(keys, &workspace, &series, timestamp, bytes)
        }
        TimeSeriesCmd::Range {
            store,
            workspace,
            series,
            from,
            to,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded = client.ts_range(keys, &workspace, &series, from, to)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
    }
}

fn run_inference(action: InferenceCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        InferenceCmd::Model { action } => run_inference_model(action, keys),
        InferenceCmd::Instance { action } => run_inference_instance(action, keys),
        InferenceCmd::List {
            cache_dir,
            remote,
            kind,
            runtime,
            format,
        } => {
            let kind = parse_inference_kind_filter(kind)?;
            let runtime = parse_inference_runtime_filter(runtime)?;
            if remote {
                return print_curated_inference_models(kind, runtime, format.as_str());
            }
            let cache_dir = inference_cache_dir(cache_dir)?;
            let manager = DownloadJobManager::new(&cache_dir);
            let inventory =
                loom_inference::discover_installed_models(&cache_dir).map_err(|e| e.to_string())?;
            let jobs = manager.list().map_err(|e| e.to_string())?;
            match format.as_str() {
                "text" => {
                    println!("local");
                    for record in inventory.models {
                        if kind.is_some_and(|kind| record.model.kind != kind)
                            || runtime.is_some_and(|runtime| record.runtime != runtime)
                        {
                            continue;
                        }
                        println!(
                            "{}\t{}\t{}\t{}\tinstalled",
                            record.model.kind.as_str(),
                            record.model.repo_id,
                            record.model.revision.value(),
                            record.runtime.as_str()
                        );
                    }
                    println!("jobs");
                    for job in jobs {
                        if kind.is_some_and(|kind| job.model.kind != kind)
                            || runtime.is_some_and(|runtime| job.runtime != runtime)
                        {
                            continue;
                        }
                        print_inference_job_text(&job);
                    }
                    Ok(())
                }
                "json" => {
                    let installed = inventory
                        .models
                        .into_iter()
                        .filter(|record| {
                            kind.is_none_or(|kind| record.model.kind == kind)
                                && runtime.is_none_or(|runtime| record.runtime == runtime)
                        })
                        .collect::<Vec<_>>();
                    let jobs = jobs
                        .into_iter()
                        .filter(|job| {
                            kind.is_none_or(|kind| job.model.kind == kind)
                                && runtime.is_none_or(|runtime| job.runtime == runtime)
                        })
                        .collect::<Vec<_>>();
                    let body = serde_json::json!({
                        "cache_dir": cache_dir,
                        "installed": installed,
                        "jobs": jobs,
                    });
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?
                    );
                    Ok(())
                }
                other => Err(format!(
                    "unknown inference output format {other:?} (expected text or json)"
                )),
            }
        }
        InferenceCmd::Status {
            job_id,
            cache_dir,
            format,
        } => {
            let manager = DownloadJobManager::new(inference_cache_dir(cache_dir)?);
            match (job_id, format.as_str()) {
                (Some(job_id), "text") => {
                    let job = manager.status(&job_id).map_err(|e| e.to_string())?;
                    print_inference_job_text(&job);
                    Ok(())
                }
                (Some(job_id), "json") => {
                    let job = manager.status(&job_id).map_err(|e| e.to_string())?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&job).map_err(|e| e.to_string())?
                    );
                    Ok(())
                }
                (None, "text") => {
                    for job in manager.list().map_err(|e| e.to_string())? {
                        print_inference_job_text(&job);
                    }
                    Ok(())
                }
                (None, "json") => {
                    let jobs = manager.list().map_err(|e| e.to_string())?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&jobs).map_err(|e| e.to_string())?
                    );
                    Ok(())
                }
                (_, other) => Err(format!(
                    "unknown inference output format {other:?} (expected text or json)"
                )),
            }
        }
        InferenceCmd::Show {
            kind,
            repo,
            runtime,
            revision,
            cache_dir,
            format,
        } => {
            let cache_dir = inference_cache_dir(cache_dir)?;
            let model = inference_model_ref(kind, repo, revision)?;
            let runtime = RuntimeKind::parse(&runtime).map_err(|e| e.to_string())?;
            let record = loom_inference::discover_installed_model(&cache_dir, &model, runtime)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "local inference model not found".to_string())?;
            print_inference_model_record(&record, format.as_str())
        }
        InferenceCmd::Download {
            kind,
            repo,
            files,
            runtime,
            revision,
            job_id,
            cache_dir,
            token,
            foreground,
        } => {
            let kind = InferenceModelKind::parse(&kind).map_err(|e| e.to_string())?;
            let runtime = RuntimeKind::parse(&runtime).map_err(|e| e.to_string())?;
            let model = ModelRef::new(kind, repo).with_revision(parse_inference_revision(revision));
            let plan = DownloadJobPlan::new(model, runtime, files).map_err(|e| e.to_string())?;
            let manager = DownloadJobManager::new(inference_cache_dir(cache_dir)?);
            if !foreground && !should_run_inference_download_inline(&manager, false)? {
                eprintln!("inference coordinator busy; another Loom download is active");
                return Ok(());
            }
            let job = match job_id {
                Some(job_id) => manager
                    .enqueue_with_id(job_id, plan)
                    .map_err(|e| e.to_string())?,
                None => manager.enqueue(plan).map_err(|e| e.to_string())?,
            };
            if !foreground {
                eprintln!("inference coordinator unavailable; running download inline");
            }
            eprintln!("job\t{}\tstate={}", job.id, job.state.as_str());
            run_inference_download(&manager, &job.id, token)
        }
        InferenceCmd::Cancel { job_id, cache_dir } => {
            let manager = DownloadJobManager::new(inference_cache_dir(cache_dir)?);
            let job = manager.cancel(&job_id).map_err(|e| e.to_string())?;
            print_inference_job_text(&job);
            Ok(())
        }
        InferenceCmd::Remove {
            kind,
            repo,
            runtime,
            revision,
            cache_dir,
            dry_run,
            yes,
        } => run_inference_remove(InferenceRemoveRequest {
            kind,
            repo,
            runtime,
            revision,
            cache_dir,
            dry_run,
            yes,
        }),
        InferenceCmd::Refresh { cache_dir, format } => {
            let cache_dir = inference_cache_dir(cache_dir)?;
            let manager = DownloadJobManager::new(&cache_dir);
            let inventory =
                loom_inference::discover_installed_models(&cache_dir).map_err(|e| e.to_string())?;
            let jobs = manager.list().map_err(|e| e.to_string())?;
            match format.as_str() {
                "text" => {
                    println!(
                        "refreshed\tlocal={}\tjobs={}",
                        inventory.models.len(),
                        jobs.len()
                    );
                    Ok(())
                }
                "json" => {
                    let body = serde_json::json!({
                        "cache_dir": cache_dir,
                        "local": inventory.models.len(),
                        "jobs": jobs.len(),
                    });
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?
                    );
                    Ok(())
                }
                other => Err(format!(
                    "unknown inference output format {other:?} (expected text or json)"
                )),
            }
        }
    }
}

fn run_inference_model(action: InferenceModelCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        InferenceModelCmd::List {
            cache_dir,
            local: _,
            remote,
            downloads: _,
            kind,
            runtime,
            format,
        } => run_inference(
            InferenceCmd::List {
                cache_dir,
                remote,
                kind,
                runtime,
                format,
            },
            keys,
        ),
        InferenceModelCmd::Show {
            repo,
            kind,
            runtime,
            revision,
            cache_dir,
            format,
        } => run_inference(
            InferenceCmd::Show {
                kind,
                repo,
                runtime,
                revision,
                cache_dir,
                format,
            },
            keys,
        ),
        InferenceModelCmd::Download {
            repo,
            files,
            kind,
            runtime,
            revision,
            job_id,
            cache_dir,
            token,
            foreground,
        } => run_inference(
            InferenceCmd::Download {
                kind,
                repo,
                files,
                runtime,
                revision,
                job_id,
                cache_dir,
                token,
                foreground,
            },
            keys,
        ),
        InferenceModelCmd::Status {
            job_id,
            cache_dir,
            format,
        } => run_inference(
            InferenceCmd::Status {
                job_id,
                cache_dir,
                format,
            },
            keys,
        ),
        InferenceModelCmd::Cancel { job_id, cache_dir } => {
            run_inference(InferenceCmd::Cancel { job_id, cache_dir }, keys)
        }
        InferenceModelCmd::Remove {
            repo,
            kind,
            runtime,
            revision,
            cache_dir,
            dry_run,
            yes,
        } => run_inference(
            InferenceCmd::Remove {
                kind,
                repo,
                runtime,
                revision,
                cache_dir,
                dry_run,
                yes,
            },
            keys,
        ),
        InferenceModelCmd::Refresh {
            cache_dir,
            kind: _,
            format,
        } => run_inference(InferenceCmd::Refresh { cache_dir, format }, keys),
    }
}

fn load_inference_instance_state(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
) -> Result<loom_inference::InferenceInstanceState, String> {
    inference_instance_state(loom, workspace).map_err(|error| error.to_string())
}

fn save_inference_instance_state(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    state: &loom_inference::InferenceInstanceState,
) -> Result<(), String> {
    put_inference_instance_state(loom, workspace, state).map_err(|error| error.to_string())?;
    save_loom(loom).map_err(|error| error.to_string())
}

fn run_inference_instance(action: InferenceInstanceCmd, keys: &KeyOpts) -> Result<(), String> {
    let (store_path, workspace) = match &action {
        InferenceInstanceCmd::List {
            store, workspace, ..
        }
        | InferenceInstanceCmd::Show {
            store, workspace, ..
        }
        | InferenceInstanceCmd::Create {
            store, workspace, ..
        }
        | InferenceInstanceCmd::Update {
            store, workspace, ..
        }
        | InferenceInstanceCmd::Delete {
            store, workspace, ..
        } => (store.as_str(), workspace.as_str()),
    };
    let mut opened = cli_open_loom(store_path, keys)?;
    let workspace_id = resolve_ns(&opened, workspace)?;
    let mut state = load_inference_instance_state(&opened, workspace_id)?;
    match action {
        InferenceInstanceCmd::List { kind, format, .. } => {
            let kind = parse_inference_kind_filter(kind)?;
            let instances = state
                .instances
                .iter()
                .filter(|instance| kind.is_none_or(|kind| instance.kind == kind))
                .map(|instance| InferenceInstanceView {
                    instance,
                    refs: state.instance_ref_count(&instance.name),
                })
                .collect::<Vec<_>>();
            print_inference_instance_list(&instances, &format)
        }
        InferenceInstanceCmd::Show {
            name,
            resolved,
            format,
            ..
        } => {
            let instance = state
                .find_instance(&name)
                .ok_or_else(|| format!("inference instance {name:?} not found"))?;
            let view = InferenceInstanceView {
                instance,
                refs: state.instance_ref_count(&instance.name),
            };
            print_inference_instance(&view, resolved, &format)
        }
        InferenceInstanceCmd::Create {
            name,
            model,
            kind,
            runtime,
            preset,
            settings,
            ..
        } => {
            if state.find_instance(&name).is_some() {
                return Err(format!("inference instance {name:?} already exists"));
            }
            let model = inference_model_ref(kind, model, None)?;
            let runtime = RuntimeKind::parse(&runtime).map_err(|e| e.to_string())?;
            let instance = loom_inference::build_instance_descriptor(
                name,
                model.kind,
                model,
                runtime,
                preset,
                parse_instance_settings(settings)?,
            )
            .map_err(|e| e.to_string())?;
            state.upsert_instance(instance.clone());
            save_inference_instance_state(&mut opened, workspace_id, &state)?;
            let view = InferenceInstanceView {
                instance: &instance,
                refs: state.instance_ref_count(&instance.name),
            };
            print_inference_instance(&view, true, "text")?;
            Ok(())
        }
        InferenceInstanceCmd::Update {
            name,
            preset,
            settings,
            ..
        } => {
            let instance = state
                .find_instance(&name)
                .cloned()
                .ok_or_else(|| format!("inference instance {name:?} not found"))?;
            let instance = loom_inference::update_instance_descriptor(
                instance,
                preset,
                parse_instance_settings(settings)?,
            )
            .map_err(|e| e.to_string())?;
            state.upsert_instance(instance.clone());
            save_inference_instance_state(&mut opened, workspace_id, &state)?;
            let view = InferenceInstanceView {
                instance: &instance,
                refs: state.instance_ref_count(&instance.name),
            };
            print_inference_instance(&view, true, "text")
        }
        InferenceInstanceCmd::Delete { name, .. } => {
            let refs = state.instance_ref_count(&name);
            if refs != 0 {
                return Err(format!(
                    "inference instance {name:?} is still referenced by {refs} binding(s)"
                ));
            }
            state
                .remove_instance(&name)
                .ok_or_else(|| format!("inference instance {name:?} not found"))?;
            save_inference_instance_state(&mut opened, workspace_id, &state)?;
            println!("deleted\t{name}");
            Ok(())
        }
    }
}

fn run_inference_instance_doctor(
    store: &str,
    workspace: &str,
    name: &str,
    format: &str,
    keys: &KeyOpts,
) -> Result<(), String> {
    let cache_dir = inference_cache_dir(None)?;
    let opened = cli_open_loom(store, keys)?;
    let workspace_id = resolve_ns(&opened, workspace)?;
    let state = load_inference_instance_state(&opened, workspace_id)?;
    let instance = state
        .find_instance(name)
        .ok_or_else(|| format!("inference instance {name:?} not found"))?;
    let report = collect_inference_instance_doctor(&cache_dir, &state, instance)?;
    print_inference_instance_doctor(&report, format)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "kebab-case")]
struct InferenceInstanceView<'a> {
    instance: &'a loom_types::InferenceInstanceDescriptor,
    refs: usize,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "kebab-case")]
struct InferenceInstanceDoctorView<'a> {
    instance: &'a loom_types::InferenceInstanceDescriptor,
    refs: usize,
    installed: bool,
    fit: Option<ModelFitReport>,
}

fn parse_instance_settings(settings: Vec<String>) -> Result<BTreeMap<String, String>, String> {
    let mut parsed = BTreeMap::new();
    for setting in settings {
        let (key, value) = setting
            .split_once('=')
            .ok_or_else(|| format!("invalid inference setting {setting:?} (expected key=value)"))?;
        if parsed.insert(key.to_string(), value.to_string()).is_some() {
            return Err(format!("duplicate inference setting {key:?}"));
        }
    }
    Ok(parsed)
}

fn print_inference_instance_list(
    instances: &[InferenceInstanceView<'_>],
    format: &str,
) -> Result<(), String> {
    match format {
        "text" => {
            for view in instances {
                print!("{}", render_inference_instance_text(view, false));
            }
            Ok(())
        }
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(instances).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        other => Err(format!(
            "unknown inference instance output format {other:?} (expected text or json)"
        )),
    }
}

fn print_inference_instance(
    view: &InferenceInstanceView<'_>,
    resolved: bool,
    format: &str,
) -> Result<(), String> {
    match format {
        "text" => {
            print!("{}", render_inference_instance_text(view, resolved));
            Ok(())
        }
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(view).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        other => Err(format!(
            "unknown inference instance output format {other:?} (expected text or json)"
        )),
    }
}

fn render_inference_instance_text(view: &InferenceInstanceView<'_>, resolved: bool) -> String {
    let instance = view.instance;
    let mut out = String::new();
    out.push_str(&format!(
        "{}\t{}\t{}\t{}\tpreset={}\trefs={}\n",
        instance.name,
        instance.kind.as_str(),
        instance.model.repo_id,
        instance.runtime.as_str(),
        instance.preset.as_deref().unwrap_or("balanced"),
        view.refs
    ));
    for (key, value) in &instance.settings.overrides {
        out.push_str(&format!("setting\t{key}={value}\n"));
    }
    if resolved {
        for (key, value) in &instance.resolved_settings {
            out.push_str(&format!("resolved\t{key}={value}\n"));
        }
    }
    out
}

fn collect_inference_instance_doctor<'a>(
    cache_dir: &std::path::Path,
    state: &'a loom_inference::InferenceInstanceState,
    instance: &'a loom_types::InferenceInstanceDescriptor,
) -> Result<InferenceInstanceDoctorView<'a>, String> {
    let installed =
        loom_inference::discover_installed_model(cache_dir, &instance.model, instance.runtime)
            .map_err(|e| e.to_string())?;
    let mut hardware = loom_inference::probe_hardware().map_err(|e| e.to_string())?;
    hardware.hf_cache_dir = Some(cache_dir.to_string_lossy().into_owned());
    let fit = installed.as_ref().map(|record| {
        loom_inference::evaluate_installed_model_fit(record, &hardware, Some(cache_dir))
    });
    Ok(InferenceInstanceDoctorView {
        instance,
        refs: state.instance_ref_count(&instance.name),
        installed: installed.is_some(),
        fit,
    })
}

fn print_inference_instance_doctor(
    report: &InferenceInstanceDoctorView<'_>,
    format: &str,
) -> Result<(), String> {
    match format {
        "text" => {
            println!(
                "instance_doctor\t{}\tinstalled={}\trefs={}",
                report.instance.name, report.installed, report.refs
            );
            if let Some(fit) = &report.fit {
                println!(
                    "fit\t{}\trunnable={}\treasons={}",
                    fit.runtime.as_str(),
                    fit.runnable,
                    fit.reasons
                        .iter()
                        .map(|reason| format!("{reason:?}"))
                        .collect::<Vec<_>>()
                        .join(",")
                );
            }
            Ok(())
        }
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(report).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        other => Err(format!(
            "unknown inference instance doctor format {other:?} (expected text or json)"
        )),
    }
}

struct ResolvedTextEmbeddingInstance {
    instance: loom_types::InferenceInstanceDescriptor,
    handle: loom_inference::TextEmbeddingHandle,
}

fn resolve_vector_text_embedding_instance(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    embedding_instance: Option<&str>,
) -> Result<ResolvedTextEmbeddingInstance, String> {
    let cache_dir = inference_cache_dir(None)?;
    let mut hardware = loom_inference::probe_hardware().map_err(|e| e.to_string())?;
    hardware.hf_cache_dir = Some(cache_dir.to_string_lossy().into_owned());
    resolve_vector_text_embedding_instance_from_cache(
        &cache_dir,
        hardware,
        loom,
        workspace,
        embedding_instance,
    )
}

fn resolve_vector_text_embedding_instance_from_cache(
    cache_dir: &std::path::Path,
    hardware: loom_types::HardwareReport,
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    embedding_instance: Option<&str>,
) -> Result<ResolvedTextEmbeddingInstance, String> {
    let state = load_inference_instance_state(loom, workspace)?;
    let instance_name = match embedding_instance {
        Some(name) => name.to_string(),
        None => state
            .vector_bindings
            .iter()
            .find(|binding| binding.workspace == workspace.to_string())
            .map(|binding| binding.embedding_instance.clone())
            .ok_or_else(|| {
                format!("no text-embedding instance is bound to workspace {workspace}")
            })?,
    };
    let instance = state
        .find_instance(&instance_name)
        .cloned()
        .ok_or_else(|| format!("inference instance {instance_name:?} not found"))?;
    if instance.kind != InferenceModelKind::TextEmbedding {
        return Err(format!(
            "inference instance {instance_name:?} is not a text-embedding instance"
        ));
    }
    let record =
        loom_inference::discover_installed_model(cache_dir, &instance.model, instance.runtime)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                format!(
                    "model {:?} is not installed for runtime {}",
                    instance.model.repo_id,
                    instance.runtime.as_str()
                )
            })?;
    let handle = loom_inference::activate_text_embedding(&record, &hardware, cache_dir)
        .map_err(|e| e.to_string())?;
    Ok(ResolvedTextEmbeddingInstance { instance, handle })
}

/// Resolve a text-embedding handle from the LOCAL install inventory, without any data store
/// (task 650 client-embed). For a remote store the embedder instance definitions are store-backed
/// and are not remotely readable without new IDL, so for client-embed the client owns model
/// selection: `--embedding-instance` names a locally-installed text-embedding model (by repo id).
/// When omitted, the single installed text-embedding model is used, else selection is required.
fn resolve_local_text_embedding(
    embedding_instance: Option<&str>,
) -> Result<loom_inference::TextEmbeddingHandle, String> {
    let cache_dir = inference_cache_dir(None)?;
    let mut hardware = loom_inference::probe_hardware().map_err(|e| e.to_string())?;
    hardware.hf_cache_dir = Some(cache_dir.to_string_lossy().into_owned());
    let inventory =
        loom_inference::discover_installed_models(&cache_dir).map_err(|e| e.to_string())?;
    let candidates: Vec<&loom_inference::InstalledModelRecord> = inventory
        .models
        .iter()
        .filter(|record| record.model.kind == InferenceModelKind::TextEmbedding)
        .collect();
    let record = match embedding_instance {
        Some(selector) => {
            let matched: Vec<&loom_inference::InstalledModelRecord> = candidates
                .iter()
                .copied()
                .filter(|record| record.model.repo_id == selector)
                .collect();
            match matched.as_slice() {
                [record] => *record,
                [] => {
                    return Err(format!(
                        "no locally-installed text-embedding model matches {selector:?}; for a remote store, --embedding-instance names a locally-installed embedding model (client-embed)"
                    ));
                }
                _ => {
                    return Err(format!(
                        "text-embedding model {selector:?} is installed for multiple runtimes; uninstall the extra install to disambiguate client-embed"
                    ));
                }
            }
        }
        None => match candidates.as_slice() {
            [record] => *record,
            [] => {
                return Err("no text-embedding model is installed locally; install one, then pass --embedding-instance <model-repo-id> (client-embed for a remote store)".to_string());
            }
            _ => {
                return Err("multiple text-embedding models are installed; pass --embedding-instance <model-repo-id> to select one for client-embed".to_string());
            }
        },
    };
    loom_inference::activate_text_embedding(record, &hardware, &cache_dir)
        .map_err(|e| e.to_string())
}

fn text_input(
    text: Option<String>,
    text_file: Option<String>,
    label: &str,
) -> Result<String, String> {
    match (text, text_file) {
        (Some(text), None) => Ok(text),
        (None, Some(path)) => String::from_utf8(read_input(&path).map_err(|e| e.to_string())?)
            .map_err(|_| format!("{label} text must be UTF-8")),
        (Some(_), Some(_)) => Err(format!(
            "provide either --{label} or --{label}-file, not both"
        )),
        (None, None) => Err(format!("provide --{label} or --{label}-file")),
    }
}

struct InferenceDoctorReport {
    cache_dir: PathBuf,
    cache_exists: bool,
    hardware: loom_types::HardwareReport,
    local_models: usize,
    jobs: usize,
    model_fit: Vec<ModelFitReport>,
    runtime_support: Vec<loom_inference::RuntimeSupportReport>,
    mlx_bundle: loom_inference::MlxBundleInspection,
    llama_cpp_bundle: loom_inference::LlamaCppBundleInspection,
    native_hf: bool,
}

fn collect_inference_doctor_report(
    cache_dir: Option<String>,
) -> Result<InferenceDoctorReport, String> {
    let cache_dir = inference_cache_dir(cache_dir)?;
    let manager = DownloadJobManager::new(&cache_dir);
    let inventory =
        loom_inference::discover_installed_models(&cache_dir).map_err(|e| e.to_string())?;
    let jobs = manager.list().map_err(|e| e.to_string())?;
    let mut hardware = loom_inference::probe_hardware().map_err(|e| e.to_string())?;
    hardware.hf_cache_dir = Some(cache_dir.to_string_lossy().into_owned());
    let model_fit = inventory
        .models
        .iter()
        .map(|record| {
            loom_inference::evaluate_installed_model_fit(record, &hardware, Some(&cache_dir))
        })
        .collect::<Vec<_>>();
    let runtime_support = loom_inference::probe_runtime_support(&hardware);
    let mlx_bundle = loom_inference::inspect_mlx_bundle(inference_mlx_bundle_dir(&hardware));
    let llama_cpp_bundle =
        loom_inference::inspect_llama_cpp_bundle(inference_llama_cpp_bundle_dir(&hardware));
    Ok(InferenceDoctorReport {
        cache_exists: cache_dir.is_dir(),
        local_models: inventory.models.len(),
        jobs: jobs.len(),
        cache_dir,
        hardware,
        model_fit,
        runtime_support,
        mlx_bundle,
        llama_cpp_bundle,
        native_hf: cfg!(feature = "inference-native-hf"),
    })
}

fn inference_mlx_bundle_dir(hardware: &loom_types::HardwareReport) -> PathBuf {
    std::env::var_os("LOOM_MLX_BUNDLE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            loom_inference::default_mlx_bundle_dir(hardware.target_triple.as_deref())
        })
}

fn inference_llama_cpp_bundle_dir(hardware: &loom_types::HardwareReport) -> PathBuf {
    std::env::var_os("LOOM_LLAMA_CPP_BUNDLE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            loom_inference::default_llama_cpp_bundle_dir(hardware.target_triple.as_deref())
        })
}

fn print_inference_doctor_report(
    report: &InferenceDoctorReport,
    format: &str,
) -> Result<(), String> {
    match format {
        "text" => {
            print_hardware_doctor_text(&report.hardware);
            print_inference_state_doctor_text(report);
            Ok(())
        }
        "json" => {
            let body = serde_json::json!({
                "cache_dir": report.cache_dir,
                "cache_exists": report.cache_exists,
                "hardware": report.hardware,
                "local_models": report.local_models,
                "jobs": report.jobs,
                "model_fit": report.model_fit,
                "runtime_support": report.runtime_support,
                "mlx_bundle": report.mlx_bundle,
                "llama_cpp_bundle": report.llama_cpp_bundle,
                "native_hf": report.native_hf,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        other => Err(format!(
            "unknown doctor output format {other:?} (expected text or json)"
        )),
    }
}

fn print_hardware_doctor_text(hardware: &loom_types::HardwareReport) {
    println!(
        "hardware\tarch={}\tos={}\tcpus={}\tmemory={}",
        hardware.cpu_arch,
        hardware.os,
        hardware.cpu_count,
        hardware
            .total_memory_bytes
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!(
        "accelerators\tmetal={}\tcuda={}",
        hardware.metal_available, hardware.cuda_available
    );
    println!(
        "candle_accelerators\tcpu_compiled={}\tcuda_compiled={}",
        hardware.candle_cpu_compiled, hardware.candle_cuda_compiled
    );
    let runtimes = hardware
        .compiled_runtimes
        .iter()
        .map(|runtime| runtime.as_str())
        .collect::<Vec<_>>();
    println!("compiled_runtimes\t{}", runtimes.join(","));
}

fn print_inference_state_doctor_text(report: &InferenceDoctorReport) {
    println!(
        "cache_dir\t{}\texists={}",
        report.cache_dir.display(),
        report.cache_exists
    );
    println!("local_models\tcount={}", report.local_models);
    println!("jobs\tactive={}", report.jobs);
    for fit in &report.model_fit {
        let reasons = fit
            .reasons
            .iter()
            .map(|reason| format!("{reason:?}"))
            .collect::<Vec<_>>();
        println!(
            "model_fit\t{}\t{}\t{}\trunnable={}\treasons={}",
            fit.model.kind.as_str(),
            fit.model.repo_id,
            fit.runtime.as_str(),
            fit.runnable,
            reasons.join(",")
        );
    }
    for runtime in &report.runtime_support {
        println!(
            "runtime_support\t{}\tavailable={}\treasons={}",
            runtime.runtime.as_str(),
            runtime.available,
            runtime.reasons.join(",")
        );
    }
    println!("{}", mlx_bundle_doctor_line(&report.mlx_bundle));
    println!("{}", llama_cpp_bundle_doctor_line(&report.llama_cpp_bundle));
    println!("native_hf\t{}", report.native_hf);
}

fn mlx_bundle_doctor_line(inspection: &loom_inference::MlxBundleInspection) -> String {
    let files = inspection
        .files
        .iter()
        .map(|file| file.name.as_str())
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "mlx_bundle\tstatus={}\tdir={}\tabi={}\tadapter={}\tfiles={}",
        inspection.status.as_str(),
        inspection.layout.root.display(),
        inspection.abi.version,
        inspection.abi.library,
        files
    )
}

fn llama_cpp_bundle_doctor_line(inspection: &loom_inference::LlamaCppBundleInspection) -> String {
    let files = inspection
        .files
        .iter()
        .map(|file| file.name.as_str())
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "llama_cpp_bundle\tstatus={}\tdir={}\tabi={}\tadapter={}\tfiles={}",
        inspection.status.as_str(),
        inspection.layout.root.display(),
        inspection.abi.version,
        inspection.abi.library,
        files
    )
}

fn run_doctor(action: DoctorCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        DoctorCmd::All {
            store,
            cache_dir,
            format,
        } => run_doctor_all(store, cache_dir, format, keys),
        DoctorCmd::Store { store } => store_doctor(&store, keys),
        DoctorCmd::Daemon { store } => daemon_doctor(&store, keys),
        DoctorCmd::Inference { cache_dir, format } => {
            let report = collect_inference_doctor_report(cache_dir)?;
            print_inference_doctor_report(&report, format.as_str())
        }
        DoctorCmd::InferenceInstance {
            store,
            workspace,
            name,
            format,
        } => run_inference_instance_doctor(&store, &workspace, &name, &format, keys),
    }
}

fn run_doctor_all(
    store: Option<String>,
    cache_dir: Option<String>,
    format: String,
    keys: &KeyOpts,
) -> Result<(), String> {
    match format.as_str() {
        "text" => {
            if let Some(store) = store.as_deref() {
                store_doctor(store, keys)?;
                daemon_doctor(store, keys)?;
            }
            let report = collect_inference_doctor_report(cache_dir)?;
            print_inference_doctor_report(&report, "text")?;
            Ok(())
        }
        "json" => {
            let inference_report = collect_inference_doctor_report(cache_dir)?;
            let body = serde_json::json!({
                "store": store
                    .as_deref()
                    .map(|store| store_doctor_json_value(store, keys))
                    .transpose()?,
                "daemon": store
                    .as_deref()
                    .map(daemon_doctor_json_value)
                    .transpose()?,
                "hardware": &inference_report.hardware,
                "inference": serde_json::json!({
                    "cache_dir": &inference_report.cache_dir,
                    "cache_exists": inference_report.cache_exists,
                    "local_models": inference_report.local_models,
                    "jobs": inference_report.jobs,
                    "model_fit": &inference_report.model_fit,
                    "runtime_support": &inference_report.runtime_support,
                    "mlx_bundle": &inference_report.mlx_bundle,
                    "llama_cpp_bundle": &inference_report.llama_cpp_bundle,
                    "native_hf": inference_report.native_hf,
                }),
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        other => Err(format!(
            "unknown doctor output format {other:?} (expected text or json)"
        )),
    }
}

fn store_doctor_json_value(store: &str, keys: &KeyOpts) -> Result<serde_json::Value, String> {
    let paths = daemon::paths(store).map_err(|e| e.to_string())?;
    let mut body = serde_json::json!({
        "store": paths.store,
    });
    match FileStore::open_read(&paths.store) {
        Ok(fs) => {
            body["encrypted"] = serde_json::json!(fs.is_encrypted());
            body["control_plane"] = serde_json::json!(["lock_fences", "identity_acl", "audit"]);
            body["maintenance"] = match fs.store_maintenance_report(now_ms()) {
                Ok(report) => {
                    let mut maintenance = serde_json::json!({
                        "state": "ok",
                        "eligible": report.eligible,
                        "reason": report.reason,
                        "physical_bytes": report.status.physical_bytes,
                        "marked_live_objects": report.marked_live_objects,
                        "marked_live_bytes": report.marked_live_bytes,
                        "candidate_reclaimable_bytes": report.candidate_reclaimable_bytes,
                        "reusable_free_bytes": report.reusable_free_bytes,
                        "tail_free_pages": report.tail_free_pages,
                        "tail_free_bytes": report.tail_free_bytes,
                        "tail_trim_eligible": report.tail_trim_eligible,
                        "tail_blocked_by_live_objects": report.tail_blocked_by_live_objects,
                        "tail_compaction_eligible": report.tail_compaction_eligible,
                        "full_compaction_required_for_shrink": report.full_compaction_required_for_shrink,
                        "tail_trim_attempted": report.tail_trim_attempted,
                        "tail_trim_pages": report.tail_trim_pages,
                        "tail_trim_bytes": report.tail_trim_bytes,
                        "tail_compaction_attempted": report.tail_compaction_attempted,
                        "tail_compaction_relocated_objects": report.tail_compaction_relocated_objects,
                        "tail_compaction_relocated_pages": report.tail_compaction_relocated_pages,
                        "tail_compaction_relocated_bytes": report.tail_compaction_relocated_bytes,
                        "tail_compaction_truncated_pages": report.tail_compaction_truncated_pages,
                        "tail_compaction_conflicts": report.tail_compaction_conflicts,
                        "last_shrink_skip_reason": report.last_shrink_skip_reason,
                        "retained_control_roots": report.retained_control_roots,
                        "derived_payload_count": report.derived_payload_count,
                        "mark_epoch": report.mark_epoch,
                        "mark_completed": report.mark_completed,
                        "last_validated_mark_epoch": report.status.last_validated_mark_epoch,
                    });
                    if let Ok(loom) = store_doctor_diagnostics_loom(&paths.store, &fs, keys)
                        && let Ok(diagnostics) = cli_live_root_diagnostics(&loom)
                    {
                        maintenance["live_root_diagnostics"] =
                            cli_live_root_diagnostics_json(&diagnostics);
                    }
                    maintenance
                }
                Err(error) => {
                    serde_json::json!({ "state": "error", "message": error.to_string() })
                }
            };
            body["runtime_data"] = match daemon_kv_loom(&paths.store) {
                Ok(_) => serde_json::json!({ "pure_ephemeral_kv": "available" }),
                Err(error) if error.code == loom_core::Code::E2eLocked => {
                    serde_json::json!({ "pure_ephemeral_kv": "requires_unlock" })
                }
                Err(error) => serde_json::json!({
                    "pure_ephemeral_kv": "error",
                    "message": error.to_string(),
                }),
            };
            body["certificate_bundles"] = match certificate_bundle_doctor_lines(&fs) {
                Ok(lines) => serde_json::json!({ "state": "ok", "lines": lines }),
                Err(error) => serde_json::json!({
                    "state": "error",
                    "message": error.to_string(),
                }),
            };
            body["network_access_policies"] = match network_access_policy_doctor_lines(&fs) {
                Ok(lines) => serde_json::json!({ "state": "ok", "lines": lines }),
                Err(error) => serde_json::json!({
                    "state": "error",
                    "message": error.to_string(),
                }),
            };
        }
        Err(error) => {
            body["encrypted"] = serde_json::json!({
                "state": "error",
                "message": error.to_string(),
            });
        }
    }
    body["reference_reconciliation"] = match cli_open_loom_read(&paths.store, keys) {
        Ok(_) => serde_json::json!({ "state": "available" }),
        Err(error) => serde_json::json!({
            "state": "unavailable",
            "message": error.to_string(),
        }),
    };
    Ok(body)
}

fn store_doctor_diagnostics_loom(
    store: &str,
    fs: &FileStore,
    keys: &KeyOpts,
) -> Result<Loom<FileStore>, String> {
    let key = if fs.is_encrypted() {
        Some(acquire_key_spec(&keys.source, "key", false)?)
    } else {
        None
    };
    open_loom_read_unlocked(store, key.as_ref()).map_err(|e| e.to_string())
}

fn cli_live_root_diagnostics(loom: &Loom<FileStore>) -> Result<LiveRootDiagnostics, String> {
    let mut extra_roots = Vec::new();
    let derived_roots = loom
        .store()
        .derived_artifact_roots()
        .map_err(|e| e.to_string())?;
    for (idx, root) in derived_roots.into_iter().enumerate() {
        extra_roots.push(("derived_artifact_roots", format!("derived:{idx}"), root));
    }
    if let Some(epoch) = loom
        .store()
        .active_reachability_mark_epoch()
        .map_err(|e| e.to_string())?
    {
        if let Some(root) = epoch.reference_root {
            extra_roots.push((
                "maintenance_mark_epoch_captured_roots",
                format!("epoch:{}:reference_root", epoch.epoch),
                root,
            ));
        }
        if let Some(root) = epoch.control_fingerprint {
            extra_roots.push((
                "maintenance_mark_epoch_captured_roots",
                format!("epoch:{}:control_fingerprint", epoch.epoch),
                root,
            ));
        }
        for (idx, root) in epoch.derived_roots.into_iter().enumerate() {
            extra_roots.push((
                "maintenance_mark_epoch_captured_roots",
                format!("epoch:{}:derived:{idx}", epoch.epoch),
                root,
            ));
        }
    }
    loom.live_root_diagnostics(loom.store().reference_root(), extra_roots, 8)
        .map_err(|e| e.to_string())
}

fn cli_live_root_diagnostics_json(diagnostics: &LiveRootDiagnostics) -> serde_json::Value {
    serde_json::json!({
        "sample_limit": diagnostics.sample_limit,
        "classes": diagnostics.classes.iter().map(|class| {
            serde_json::json!({
                "class": class.class,
                "count": class.count,
                "examples": class.examples.iter().map(|example| {
                    serde_json::json!({
                        "id": example.id,
                        "digest": example.digest.to_string(),
                    })
                }).collect::<Vec<_>>(),
                "truncated": class.truncated,
            })
        }).collect::<Vec<_>>(),
    })
}

fn daemon_doctor_json_value(store: &str) -> Result<serde_json::Value, String> {
    let paths = daemon::paths(store).map_err(|e| e.to_string())?;
    let runtime_artifacts = match daemon::validate_runtime_artifacts(&paths) {
        Ok(()) => serde_json::json!({ "state": "ok" }),
        Err(error) => serde_json::json!({ "state": "error", "message": error.to_string() }),
    };
    Ok(serde_json::json!({
        "store": paths.store,
        "runtime_dir": daemon::runtime_dir(),
        "addr_file": paths.addr_file,
        "pid_file": paths.pid_file,
        "lock_file": paths.lock_file,
        "sock_file": paths.sock_file,
        "pipe_name": paths.pipe_name,
        "runtime_artifacts": runtime_artifacts,
    }))
}

fn inference_cache_dir(cache_dir: Option<String>) -> Result<PathBuf, String> {
    if let Some(cache_dir) = cache_dir {
        return Ok(PathBuf::from(cache_dir));
    }
    if let Some(hf_home) = std::env::var_os("HF_HOME") {
        return Ok(PathBuf::from(hf_home).join("hub"));
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "home directory is unavailable".to_string())?;
    Ok(home.join(".cache").join("huggingface").join("hub"))
}

fn parse_inference_revision(revision: Option<String>) -> RevisionRef {
    match revision {
        Some(value) if value.starts_with("commit:") => {
            RevisionRef::Commit(value.trim_start_matches("commit:").to_string())
        }
        Some(value) if value.starts_with("tag:") => {
            RevisionRef::Tag(value.trim_start_matches("tag:").to_string())
        }
        Some(value) if value.starts_with("branch:") => {
            RevisionRef::Branch(value.trim_start_matches("branch:").to_string())
        }
        Some(value) => RevisionRef::Branch(value),
        None => RevisionRef::main(),
    }
}

fn inference_model_ref(
    kind: String,
    repo: String,
    revision: Option<String>,
) -> Result<ModelRef, String> {
    let kind = InferenceModelKind::parse(&kind).map_err(|e| e.to_string())?;
    Ok(ModelRef::new(kind, repo).with_revision(parse_inference_revision(revision)))
}

fn parse_inference_kind_filter(
    value: Option<String>,
) -> Result<Option<InferenceModelKind>, String> {
    value
        .map(|value| InferenceModelKind::parse(&value).map_err(|e| e.to_string()))
        .transpose()
}

fn parse_inference_runtime_filter(value: Option<String>) -> Result<Option<RuntimeKind>, String> {
    value
        .map(|value| RuntimeKind::parse(&value).map_err(|e| e.to_string()))
        .transpose()
}

fn print_curated_inference_models(
    kind: Option<InferenceModelKind>,
    runtime: Option<RuntimeKind>,
    format: &str,
) -> Result<(), String> {
    let hardware = loom_inference::probe_hardware().map_err(|e| e.to_string())?;
    let models = loom_inference::curated_models()
        .iter()
        .copied()
        .filter(|model| model.matches_kind(kind) && model.matches_runtime(runtime))
        .map(|model| CuratedInferenceModelView {
            model,
            fit: loom_inference::evaluate_curated_model_fit(model, &hardware),
        })
        .collect::<Vec<_>>();
    match format {
        "text" => {
            print!("{}", render_curated_inference_models_text(&models));
            Ok(())
        }
        "json" => {
            println!("{}", render_curated_inference_models_json(&models)?);
            Ok(())
        }
        other => Err(format!(
            "unknown inference output format {other:?} (expected text or json)"
        )),
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "kebab-case")]
struct CuratedInferenceModelView {
    model: loom_inference::CuratedModelSpec,
    fit: ModelFitReport,
}

fn render_curated_inference_models_text(models: &[CuratedInferenceModelView]) -> String {
    let mut out = String::new();
    for view in models {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\tfit={}\t{}\n",
            view.model.kind.as_str(),
            view.model.repo_id,
            view.model.revision,
            view.model.runtime.as_str(),
            curated_fit_label(&view.fit),
            view.model.summary
        ));
        out.push_str(&format!("files\t{}\n", view.model.files.join(",")));
    }
    out
}

fn render_curated_inference_models_json(
    models: &[CuratedInferenceModelView],
) -> Result<String, String> {
    serde_json::to_string_pretty(models).map_err(|e| e.to_string())
}

fn curated_fit_label(fit: &ModelFitReport) -> String {
    if fit.runnable {
        return "ok".to_string();
    }
    let reasons = fit
        .reasons
        .iter()
        .map(|reason| format!("{reason:?}"))
        .collect::<Vec<_>>();
    format!("blocked:{}", reasons.join(","))
}

fn print_inference_model_record(
    record: &loom_inference::InstalledModelRecord,
    format: &str,
) -> Result<(), String> {
    match format {
        "text" => {
            print!("{}", render_inference_model_record_text(record));
            Ok(())
        }
        "json" => {
            println!("{}", render_inference_model_record_json(record)?);
            Ok(())
        }
        other => Err(format!(
            "unknown inference output format {other:?} (expected text or json)"
        )),
    }
}

fn render_inference_model_record_text(record: &loom_inference::InstalledModelRecord) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{}\t{}\t{}\t{}\n",
        record.model.kind.as_str(),
        record.model.repo_id,
        record.model.revision.value(),
        record.runtime.as_str()
    ));
    for file in &record.files {
        out.push_str(&format!(
            "file\t{}\tbytes={}\tdigest={}\n",
            file.relative_path,
            file.size_bytes,
            file.digest.as_deref().unwrap_or("")
        ));
    }
    for active in &record.active_provider_refs {
        out.push_str(&format!("active\t{active}\n"));
    }
    out
}

fn render_inference_model_record_json(
    record: &loom_inference::InstalledModelRecord,
) -> Result<String, String> {
    serde_json::to_string_pretty(record).map_err(|e| e.to_string())
}

struct InferenceRemoveRequest {
    kind: String,
    repo: String,
    runtime: String,
    revision: Option<String>,
    cache_dir: Option<String>,
    dry_run: bool,
    yes: bool,
}

fn run_inference_remove(request: InferenceRemoveRequest) -> Result<(), String> {
    let cache_dir = inference_cache_dir(request.cache_dir)?;
    let model = inference_model_ref(request.kind, request.repo, request.revision)?;
    let runtime = RuntimeKind::parse(&request.runtime).map_err(|e| e.to_string())?;
    let record = loom_inference::discover_installed_model(&cache_dir, &model, runtime)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "local inference model not found".to_string())?;
    let paths = planned_inference_remove_paths(&cache_dir, &record)?;
    for path in &paths {
        println!("remove\t{}", path.display());
    }
    if request.dry_run || !request.yes {
        println!("dry_run\ttrue");
        return Ok(());
    }

    let manager = DownloadJobManager::new(&cache_dir);
    let _lock = manager.acquire_cache_lock().map_err(|e| e.to_string())?;
    for path in &paths {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("remove {}: {error}", path.display())),
        }
    }
    for path in &paths {
        prune_empty_cache_dirs(&cache_dir, path)?;
    }
    println!("removed\t{}\t{}", model.kind.as_str(), model.repo_id);
    Ok(())
}

fn planned_inference_remove_paths(
    cache_dir: &std::path::Path,
    record: &loom_inference::InstalledModelRecord,
) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::with_capacity(record.files.len());
    for file in &record.files {
        paths.push(guarded_cache_path(cache_dir, &file.relative_path)?);
    }
    Ok(paths)
}

fn guarded_cache_path(cache_dir: &std::path::Path, relative_path: &str) -> Result<PathBuf, String> {
    let relative = std::path::Path::new(relative_path);
    if relative.components().any(|component| {
        !matches!(
            component,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    }) {
        return Err(format!("invalid cache-relative path: {relative_path}"));
    }
    let path = cache_dir.join(relative_path);
    if !path.starts_with(cache_dir) {
        return Err(format!(
            "refusing to remove path outside cache root: {}",
            path.display()
        ));
    }
    Ok(path)
}

fn prune_empty_cache_dirs(
    cache_dir: &std::path::Path,
    file_path: &std::path::Path,
) -> Result<(), String> {
    let mut current = file_path.parent();
    while let Some(dir) = current {
        if dir == cache_dir {
            break;
        }
        if !dir.starts_with(cache_dir) {
            return Err(format!(
                "refusing to prune path outside cache root: {}",
                dir.display()
            ));
        }
        match std::fs::remove_dir(dir) {
            Ok(()) => current = dir.parent(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => current = dir.parent(),
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => break,
            Err(error) => return Err(format!("prune {}: {error}", dir.display())),
        }
    }
    Ok(())
}

fn print_inference_job_text(job: &loom_types::DownloadJob) {
    println!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
        job.id,
        job.model.kind.as_str(),
        job.model.repo_id,
        job.model.revision.value(),
        job.runtime.as_str(),
        job.state.as_str(),
        job.downloaded_bytes
    );
}

fn should_run_inference_download_inline(
    manager: &DownloadJobManager,
    foreground: bool,
) -> Result<bool, String> {
    if foreground {
        return Ok(true);
    }
    match manager.acquire_cache_lock() {
        Ok(lock) => {
            drop(lock);
            Ok(true)
        }
        Err(error) if error.code == Code::Locked => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(feature = "inference-native-hf")]
fn run_inference_download(
    manager: &DownloadJobManager,
    job_id: &str,
    token: Option<String>,
) -> Result<(), String> {
    let downloader = loom_inference::HfDownloader::from_env(token).map_err(|e| e.to_string())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let job = runtime
        .block_on(manager.run_hf(job_id, &downloader, print_inference_download_event))
        .map_err(|e| e.to_string())?;
    print_inference_job_text(&job);
    Ok(())
}

#[cfg(not(feature = "inference-native-hf"))]
fn run_inference_download(
    _manager: &DownloadJobManager,
    _job_id: &str,
    _token: Option<String>,
) -> Result<(), String> {
    Err("loom was built without inference-native-hf; Hugging Face downloads are unavailable".into())
}

#[cfg(feature = "inference-native-hf")]
fn print_inference_download_event(event: DownloadEvent) {
    match event {
        DownloadEvent::StateChanged { job_id, state } => {
            eprintln!("job\t{job_id}\tstate={}", state.as_str());
        }
        DownloadEvent::FileStarted { job_id, file } => {
            eprintln!("job\t{job_id}\tfile={file}\tstate=started");
        }
        DownloadEvent::FileFinished {
            job_id,
            file,
            size_bytes,
            digest,
            ..
        } => {
            eprintln!("job\t{job_id}\tfile={file}\tbytes={size_bytes}\tdigest={digest}");
        }
        DownloadEvent::Retry {
            job_id,
            file,
            attempt,
            message,
        } => {
            eprintln!("job\t{job_id}\tfile={file}\tretry={attempt}\terror={message}");
        }
        DownloadEvent::Failed { job_id, message } => {
            eprintln!("job\t{job_id}\tstate=failed\terror={message}");
        }
    }
}

fn run_vector(action: VectorCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        VectorCmd::Workspace { action } => run_vector_workspace(action, keys),
        VectorCmd::Text { action } => run_vector_text(action, keys),
        VectorCmd::Create {
            store,
            workspace,
            name,
            dim,
            metric,
        } => {
            let client = remote::open_store_client(&store)?;
            client.v_create(keys, &workspace, &name, dim as u64, &metric)?;
            println!("created {name}");
            Ok(())
        }
        VectorCmd::Upsert {
            store,
            workspace,
            name,
            id,
            vector,
            metadata,
        } => {
            let vector = read_input(&vector).map_err(|e| e.to_string())?;
            let metadata = match metadata {
                Some(path) => read_input(&path).map_err(|e| e.to_string())?,
                None => Vec::new(),
            };
            let client = remote::open_store_client(&store)?;
            client.v_upsert(keys, &workspace, &name, &id, vector, metadata)
        }
        VectorCmd::UpsertSource {
            store,
            workspace,
            name,
            id,
            vector,
            source,
            metadata,
            model_id,
            weights_digest,
        } => {
            let vector = read_input(&vector).map_err(|e| e.to_string())?;
            let source_text = read_input(&source).map_err(|e| e.to_string())?;
            let metadata = match metadata {
                Some(path) => read_input(&path).map_err(|e| e.to_string())?,
                None => Vec::new(),
            };
            let client = remote::open_store_client(&store)?;
            client.v_upsert_source(
                keys,
                &workspace,
                &name,
                &id,
                vector,
                metadata,
                source_text,
                model_id,
                weights_digest,
            )
        }
        VectorCmd::Get {
            store,
            workspace,
            name,
            id,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(bytes) = client.v_get(keys, &workspace, &name, &id)? else {
                return Err(format!("vector id {id:?} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        VectorCmd::Source {
            store,
            workspace,
            name,
            id,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(bytes) = client.v_source_text(keys, &workspace, &name, &id)? else {
                return Err(format!("vector id {id:?} has no source text"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        VectorCmd::Ids {
            store,
            workspace,
            name,
            prefix,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let ids = client.v_ids(keys, &workspace, &name, prefix.as_deref())?;
            if let Some(out) = out {
                write_output(Some(&out), &vector_ids_cbor(&ids)?).map_err(|e| e.to_string())
            } else {
                for id in ids {
                    println!("{id}");
                }
                Ok(())
            }
        }
        VectorCmd::IndexKeys {
            store,
            workspace,
            name,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let index_keys = client.v_index_keys(keys, &workspace, &name)?;
            if let Some(out) = out {
                write_output(Some(&out), &vector_ids_cbor(&index_keys)?).map_err(|e| e.to_string())
            } else {
                for key in index_keys {
                    println!("{key}");
                }
                Ok(())
            }
        }
        VectorCmd::CreateIndex {
            store,
            workspace,
            name,
            key,
        } => {
            let client = remote::open_store_client(&store)?;
            let changed = client.v_create_index(keys, &workspace, &name, &key)?;
            println!("{changed}");
            Ok(())
        }
        VectorCmd::DropIndex {
            store,
            workspace,
            name,
            key,
        } => {
            let client = remote::open_store_client(&store)?;
            let changed = client.v_drop_index(keys, &workspace, &name, &key)?;
            println!("{changed}");
            Ok(())
        }
        VectorCmd::Delete {
            store,
            workspace,
            name,
            id,
        } => {
            let client = remote::open_store_client(&store)?;
            let present = client.v_delete(keys, &workspace, &name, &id)?;
            println!("{present}");
            Ok(())
        }
        VectorCmd::Search {
            store,
            workspace,
            name,
            query,
            k,
            filter,
            policy,
            threshold,
            ef,
            pq_m,
            pq_k,
            pq_iters,
            out,
        } => {
            let query = read_input(&query).map_err(|e| e.to_string())?;
            let filter = match filter {
                Some(path) => read_input(&path).map_err(|e| e.to_string())?,
                None => Vec::new(),
            };
            let client = remote::open_store_client(&store)?;
            let hits_bytes = client.v_search(
                keys,
                &workspace,
                &name,
                query,
                k as u64,
                filter,
                &policy,
                threshold as u64,
                ef as u64,
                pq_m as u64,
                pq_k as u64,
                pq_iters as u64,
            )?;
            if let Some(out) = out {
                write_output(Some(&out), &hits_bytes).map_err(|e| e.to_string())
            } else {
                // Reproduce the `id\tscore` lines from the canonical hits CBOR (`[[id, score_cell]...]`).
                let WireValue::Array(items) =
                    loom_codec::decode(&hits_bytes).map_err(|e| e.to_string())?
                else {
                    return Err("vector hits must be a CBOR array".to_string());
                };
                for item in items {
                    let WireValue::Array(pair) = item else {
                        return Err("vector hit must be a [id, score] array".to_string());
                    };
                    let mut fields = pair.into_iter();
                    let id = match fields.next() {
                        Some(WireValue::Text(id)) => id,
                        _ => return Err("vector hit id must be text".to_string()),
                    };
                    let score = match fields.next() {
                        Some(cell) => match wire_cell_from(cell)? {
                            loom_core::Value::F32(score) => score,
                            _ => return Err("vector hit score must be an f32 cell".to_string()),
                        },
                        None => return Err("vector hit is missing its score".to_string()),
                    };
                    println!("{id}\t{score}");
                }
                Ok(())
            }
        }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "kebab-case")]
struct VectorTextModelView {
    model_id: String,
    dimension: usize,
    weights_digest: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "kebab-case")]
struct VectorTextUpsertView {
    store: String,
    workspace: String,
    collection: String,
    id: String,
    embedding_instance: String,
    model: VectorTextModelView,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "kebab-case")]
struct VectorTextHitView {
    id: String,
    score: f32,
    source_text: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "kebab-case")]
struct VectorTextQueryView {
    store: String,
    workspace: String,
    collection: String,
    query: String,
    embedding_instance: String,
    model: VectorTextModelView,
    hits: Vec<VectorTextHitView>,
}

fn vector_text_model_view(model: loom_inference::TextEmbeddingModel) -> VectorTextModelView {
    VectorTextModelView {
        model_id: model.model_id,
        dimension: model.dimension,
        weights_digest: model.weights_digest,
    }
}

fn run_vector_text(action: VectorTextCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        VectorTextCmd::Upsert {
            store,
            workspace,
            name,
            id,
            text,
            text_file,
            embedding_instance,
            metadata,
            create,
            metric,
            format,
        } => {
            let source_text = text_input(text, text_file, "text")?;
            // Keep raw metadata CBOR bytes: forwarded as-is to the remote Vector surface, decoded
            // to a map for the local path (empty bytes decode to an empty map).
            let metadata_bytes = match metadata {
                Some(path) => read_input(&path).map_err(|e| e.to_string())?,
                None => Vec::new(),
            };
            // task 650 client-embed: for a remote store the client embeds text locally (owning model
            // selection) and routes the computed vector + explicit model metadata through the
            // already-remote Vector.upsert_source. The server never infers. No local path is sent.
            if remote::target_is_remote(&store)? {
                vector_metadata_from_cbor(&metadata_bytes)?; // validate before the network round-trip
                let handle = resolve_local_text_embedding(embedding_instance.as_deref())?;
                let model = handle
                    .model()
                    .ok_or_else(|| "text embedding provider did not expose a model".to_string())?;
                let vectors = handle
                    .embed(std::slice::from_ref(&source_text))
                    .map_err(|e| e.to_string())?;
                let vector_bytes = vector_floats_to_bytes(&vectors[0]);
                let client = remote::open_store_client(&store)?;
                if create {
                    client.v_create(keys, &workspace, &name, model.dimension as u64, &metric)?;
                }
                client.v_upsert_source(
                    keys,
                    &workspace,
                    &name,
                    &id,
                    vector_bytes,
                    metadata_bytes,
                    source_text.clone().into_bytes(),
                    Some(model.model_id.clone()),
                    model.weights_digest.clone(),
                )?;
                let view = VectorTextUpsertView {
                    store,
                    workspace,
                    collection: name,
                    id,
                    embedding_instance: model.model_id.clone(),
                    model: vector_text_model_view(model),
                };
                return print_vector_text_upsert(&view, &format);
            }
            let metadata = vector_metadata_from_cbor(&metadata_bytes)?;
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = if create {
                ensure_vector_workspace(&mut loom, &workspace)?
            } else {
                resolve_ns(&loom, &workspace)?
            };
            let resolved =
                resolve_vector_text_embedding_instance(&loom, ns, embedding_instance.as_deref())?;
            let model = resolved
                .handle
                .model()
                .ok_or_else(|| "text embedding provider did not expose a model".to_string())?;
            if create {
                let metric = parse_vector_metric(&metric)?;
                match loom_core::vector_create(&mut loom, ns, &name, model.dimension, metric) {
                    Ok(()) => {}
                    Err(err) if err.code == Code::Conflict => {}
                    Err(err) => return Err(err.to_string()),
                }
            }
            loom_core::vector_upsert_text(
                &mut loom,
                ns,
                &name,
                &id,
                &source_text,
                metadata,
                &resolved.handle,
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            let view = VectorTextUpsertView {
                store,
                workspace,
                collection: name,
                id,
                embedding_instance: resolved.instance.name,
                model: vector_text_model_view(model),
            };
            print_vector_text_upsert(&view, &format)
        }
        VectorTextCmd::Query {
            store,
            workspace,
            name,
            query,
            query_file,
            top_k,
            embedding_instance,
            filter,
            format,
        } => {
            let query = text_input(query, query_file, "query")?;
            let filter_bytes = match filter {
                Some(path) => read_input(&path).map_err(|e| e.to_string())?,
                None => Vec::new(),
            };
            // task 650 client-embed: remote query embeds locally, then searches over the remote
            // Vector surface with the client-computed query vector.
            if remote::target_is_remote(&store)? {
                let handle = resolve_local_text_embedding(embedding_instance.as_deref())?;
                let model = handle
                    .model()
                    .ok_or_else(|| "text embedding provider did not expose a model".to_string())?;
                let query_vectors = handle
                    .embed(std::slice::from_ref(&query))
                    .map_err(|e| e.to_string())?;
                let query_bytes = vector_floats_to_bytes(&query_vectors[0]);
                let client = remote::open_store_client(&store)?;
                let hits_bytes = client.v_search(
                    keys,
                    &workspace,
                    &name,
                    query_bytes,
                    top_k as u64,
                    filter_bytes,
                    "exact",
                    0,
                    0,
                    0,
                    0,
                    0,
                )?;
                let WireValue::Array(items) =
                    loom_codec::decode(&hits_bytes).map_err(|e| e.to_string())?
                else {
                    return Err("vector hits must be a CBOR array".to_string());
                };
                let mut hits = Vec::with_capacity(items.len());
                for item in items {
                    let WireValue::Array(pair) = item else {
                        return Err("vector hit must be a [id, score] array".to_string());
                    };
                    let mut fields = pair.into_iter();
                    let hit_id = match fields.next() {
                        Some(WireValue::Text(id)) => id,
                        _ => return Err("vector hit id must be text".to_string()),
                    };
                    let score = match fields.next() {
                        Some(cell) => match wire_cell_from(cell)? {
                            loom_core::Value::F32(score) => score,
                            _ => return Err("vector hit score must be an f32 cell".to_string()),
                        },
                        None => return Err("vector hit is missing its score".to_string()),
                    };
                    let source_text = client
                        .v_source_text(keys, &workspace, &name, &hit_id)?
                        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
                    hits.push(VectorTextHitView {
                        id: hit_id,
                        score,
                        source_text,
                    });
                }
                let view = VectorTextQueryView {
                    store,
                    workspace,
                    collection: name,
                    query,
                    embedding_instance: model.model_id.clone(),
                    model: vector_text_model_view(model),
                    hits,
                };
                return print_vector_text_query(&view, &format);
            }
            let filter = vector_filter_from_cbor(&filter_bytes)?;
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let resolved =
                resolve_vector_text_embedding_instance(&loom, ns, embedding_instance.as_deref())?;
            let model = resolved
                .handle
                .model()
                .ok_or_else(|| "text embedding provider did not expose a model".to_string())?;
            let query_vectors = resolved
                .handle
                .embed(std::slice::from_ref(&query))
                .map_err(|e| e.to_string())?;
            let hits =
                loom_core::vector_search(&loom, ns, &name, &query_vectors[0], top_k, &filter)
                    .map_err(|e| e.to_string())?;
            let hits = hits
                .into_iter()
                .map(|hit| {
                    let source_text = loom_core::vector_source_text(&loom, ns, &name, &hit.id)
                        .map_err(|e| e.to_string())?;
                    Ok(VectorTextHitView {
                        id: hit.id,
                        score: hit.score,
                        source_text,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let view = VectorTextQueryView {
                store,
                workspace,
                collection: name,
                query,
                embedding_instance: resolved.instance.name,
                model: vector_text_model_view(model),
                hits,
            };
            print_vector_text_query(&view, &format)
        }
    }
}

fn print_vector_text_upsert(view: &VectorTextUpsertView, format: &str) -> Result<(), String> {
    match format {
        "text" => {
            println!(
                "vector_text_upsert\t{}\t{}\t{}\tembedding_instance={}\tmodel={}",
                view.workspace,
                view.collection,
                view.id,
                view.embedding_instance,
                view.model.model_id
            );
            Ok(())
        }
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(view).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        other => Err(format!(
            "unknown vector text output format {other:?} (expected text or json)"
        )),
    }
}

fn print_vector_text_query(view: &VectorTextQueryView, format: &str) -> Result<(), String> {
    match format {
        "text" => {
            print!("{}", render_vector_text_query_text(view));
            Ok(())
        }
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(view).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        other => Err(format!(
            "unknown vector text output format {other:?} (expected text or json)"
        )),
    }
}

fn render_vector_text_query_text(view: &VectorTextQueryView) -> String {
    let mut out = String::new();
    for hit in &view.hits {
        out.push_str(&format!(
            "{}\t{}\t{}\n",
            hit.id,
            hit.score,
            hit.source_text.as_deref().unwrap_or("")
        ));
    }
    out
}

fn print_surface_catalog(
    workspace: &str,
    set: &str,
    apps: &[SurfaceAppDefinition],
    format: &str,
) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                surface_catalog_json(workspace, set).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            for app in apps {
                println!("{}\t{}\t{}", app.app_id, app.display_name, app.resource_uri);
            }
            Ok(())
        }
        other => Err(format!(
            "unsupported Studio surface catalog format {other:?}; supported formats: text, json"
        )),
    }
}

struct StudioReindexEnqueueResult {
    workspace_id: WorkspaceId,
    profile: String,
    job_path: String,
    state: String,
    source_digest: Digest,
    model_id: String,
    vector_records_indexed: usize,
    vector_records_deleted: usize,
}

fn run_studio(action: StudioCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        StudioCmd::Surfaces { action } => run_studio_surfaces(action),
        StudioCmd::Reindex {
            store,
            workspace,
            profile,
            format,
        } => {
            let result = enqueue_studio_reindex(&store, &workspace, &profile, None, keys)?;
            print_studio_reindex_enqueue(&result, &format)
        }
        StudioCmd::Revisions { action } => run_studio_revisions(action, keys),
    }
}

fn run_studio_surfaces(action: StudioSurfacesCmd) -> Result<(), String> {
    match action {
        StudioSurfacesCmd::Catalog {
            workspace,
            set,
            format,
        } => {
            let apps = match set.as_str() {
                "core" => core_surface_catalog(&workspace).map_err(|e| e.to_string())?,
                "all" => surface_app_catalog(&workspace).map_err(|e| e.to_string())?,
                "meeting-memory" => {
                    meeting_memory_surface_catalog(&workspace).map_err(|e| e.to_string())?
                }
                other => {
                    return Err(format!(
                        "unsupported Studio surface catalog set {other:?}; supported sets: core, all, meeting-memory"
                    ));
                }
            };
            print_surface_catalog(&workspace, &set, &apps, &format)
        }
    }
}

fn run_studio_revisions(action: StudioRevisionsCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        StudioRevisionsCmd::Rebuild {
            store,
            workspace,
            profile,
            dry_run,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let report = rebuild_studio_revision_index(&mut loom, workspace_id, &profile, dry_run)?;
            if !dry_run && report.inserted > 0 {
                save_loom(&mut loom).map_err(|e| e.to_string())?;
            }
            print_revision_rebuild_report(&report, &format)
        }
    }
}

#[derive(Debug)]
struct RevisionRebuildReport {
    workspace: String,
    scope_id: String,
    profile: String,
    index_present_before: bool,
    candidates: u64,
    inserted: u64,
    skipped_existing: u64,
    dry_run: bool,
}

fn rebuild_studio_revision_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    profile: &str,
    dry_run: bool,
) -> Result<RevisionRebuildReport, String> {
    let scope_id = workspace.to_string();
    match profile {
        "drive" => rebuild_drive_revision_index(loom, workspace, &scope_id, dry_run),
        "lifecycle" => rebuild_lifecycle_revision_index(loom, workspace, &scope_id, dry_run),
        "meetings" => rebuild_meetings_revision_index(loom, workspace, &scope_id, dry_run),
        "pages" => rebuild_pages_revision_index(loom, workspace, &scope_id, dry_run),
        other => Err(format!(
            "unsupported Studio revision rebuild profile {other:?}; supported profiles: drive, lifecycle, meetings, pages"
        )),
    }
}

fn rebuild_meetings_revision_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    scope_id: &str,
    dry_run: bool,
) -> Result<RevisionRebuildReport, String> {
    loom.authorize(workspace, FacetKind::Vcs, AclRight::Write)
        .map_err(|e| e.to_string())?;
    let key = meetings_profile_key(scope_id).map_err(|e| e.to_string())?;
    let Some(bytes) = loom.store().control_get(&key).map_err(|e| e.to_string())? else {
        return Err("meetings snapshot not found".to_string());
    };
    let snapshot = MeetingsProfileSnapshot::decode(&bytes).map_err(|e| e.to_string())?;
    let root = Digest::hash(loom.store().digest_algo(), &bytes);
    let updates = snapshot
        .meetings
        .iter()
        .map(|meeting| {
            let body = meeting.encode().map_err(|e| e.to_string())?;
            RevisionBackfillUpdate::new(
                format!("meeting:{}", meeting.meeting_id),
                format!("meetings:{scope_id}:{}:backfill:1", meeting.meeting_id),
                BodyRef::new(
                    Digest::hash(loom.store().digest_algo(), &body),
                    body.len() as u64,
                    "application/vnd.uldren.loom.meetings.meeting+cbor",
                )
                .map_err(|e| e.to_string())?,
                root,
                meeting.updated_at_ms,
                format!("{}:backfill:1", meeting.meeting_id),
            )
            .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    apply_revision_backfill(loom, workspace, scope_id, "meetings", dry_run, updates)
}

fn rebuild_drive_revision_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    scope_id: &str,
    dry_run: bool,
) -> Result<RevisionRebuildReport, String> {
    loom.authorize(workspace, FacetKind::Vcs, AclRight::Write)
        .map_err(|e| e.to_string())?;
    let key = drive_operation_log_key(scope_id).map_err(|e| e.to_string())?;
    let Some(bytes) = loom.store().control_get(&key).map_err(|e| e.to_string())? else {
        return Err("drive operation log not found".to_string());
    };
    let log = DriveOperationLog::decode(&bytes).map_err(|e| e.to_string())?;
    let mut latest = BTreeMap::new();
    for record in log.records.iter().rev() {
        let Some(target) = record.target_entity_id.as_deref() else {
            continue;
        };
        let entity_id = format!("drive:metadata:{target}");
        if latest.contains_key(&entity_id) {
            continue;
        }
        let envelope = OperationEnvelope::decode(&record.envelope).map_err(|e| e.to_string())?;
        latest.insert(
            entity_id.clone(),
            revision_backfill_update(
                loom,
                entity_id,
                record.operation_id.clone(),
                record.root_after,
                &record.envelope,
                "application/vnd.uldren.loom.drive.operation+cbor",
                envelope.timestamp_ms,
                format!("drive:metadata:{target}:backfill:1"),
            )?,
        );
    }
    apply_revision_backfill(
        loom,
        workspace,
        scope_id,
        "drive",
        dry_run,
        latest.into_values().collect(),
    )
}

fn rebuild_pages_revision_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    scope_id: &str,
    dry_run: bool,
) -> Result<RevisionRebuildReport, String> {
    loom.authorize(workspace, FacetKind::Vcs, AclRight::Write)
        .map_err(|e| e.to_string())?;
    let key = page_profile_operation_log_key(scope_id).map_err(|e| e.to_string())?;
    let Some(bytes) = loom.store().control_get(&key).map_err(|e| e.to_string())? else {
        return Err("pages operation log not found".to_string());
    };
    let log = PageOperationLog::decode(&bytes).map_err(|e| e.to_string())?;
    let mut latest = BTreeMap::new();
    for record in log.records.iter().rev() {
        let Some(target) = record.target_entity_id.as_deref() else {
            continue;
        };
        let entity_id = page_operation_revision_entity_id(record.operation_kind.as_str(), target);
        if latest.contains_key(&entity_id) {
            continue;
        }
        let envelope = OperationEnvelope::decode(&record.envelope).map_err(|e| e.to_string())?;
        latest.insert(
            entity_id.clone(),
            revision_backfill_update(
                loom,
                entity_id,
                record.operation_id.clone(),
                record.root_after,
                &record.envelope,
                "application/vnd.uldren.loom.pages.operation+cbor",
                envelope.timestamp_ms,
                format!("pages:{scope_id}:{target}:backfill:1"),
            )?,
        );
    }
    apply_revision_backfill(
        loom,
        workspace,
        scope_id,
        "pages",
        dry_run,
        latest.into_values().collect(),
    )
}

fn rebuild_lifecycle_revision_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    scope_id: &str,
    dry_run: bool,
) -> Result<RevisionRebuildReport, String> {
    loom.authorize(workspace, FacetKind::Vcs, AclRight::Write)
        .map_err(|e| e.to_string())?;
    let mut updates = Vec::new();
    for (key, bytes) in loom
        .store()
        .control_scan_prefix(format!("profile/lifecycle/v1/{scope_id}/definitions/").as_bytes())
        .map_err(|e| e.to_string())?
    {
        let definition_id = String::from_utf8_lossy(&key)
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string();
        let root = Digest::hash(loom.store().digest_algo(), &bytes);
        updates.push(revision_backfill_update(
            loom,
            format!("lifecycle:definition:{definition_id}"),
            format!("lifecycle.definition.backfill:{scope_id}:{definition_id}"),
            root,
            &bytes,
            "application/vnd.uldren.loom.lifecycle.definition+cbor",
            0,
            format!("lifecycle:definition:{definition_id}:backfill:1"),
        )?);
    }
    for (key, bytes) in loom
        .store()
        .control_scan_prefix(format!("profile/lifecycle/v1/{scope_id}/instances/").as_bytes())
        .map_err(|e| e.to_string())?
    {
        let instance_id = String::from_utf8_lossy(&key)
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string();
        let root = Digest::hash(loom.store().digest_algo(), &bytes);
        updates.push(revision_backfill_update(
            loom,
            format!("lifecycle:instance:{instance_id}"),
            format!("lifecycle.instance.backfill:{scope_id}:{instance_id}"),
            root,
            &bytes,
            "application/vnd.uldren.loom.lifecycle.instance+cbor",
            0,
            format!("lifecycle:instance:{instance_id}:backfill:1"),
        )?);
    }
    let key = lifecycle_operation_log_key(scope_id).map_err(|e| e.to_string())?;
    if let Some(bytes) = loom.store().control_get(&key).map_err(|e| e.to_string())? {
        let log = LifecycleOperationLog::decode(&bytes).map_err(|e| e.to_string())?;
        for record in log.records.iter().rev() {
            let entity_id = format!("lifecycle:instance:{}", record.instance_id);
            if updates.iter().any(|update| update.entity_id == entity_id) {
                continue;
            }
            let envelope =
                OperationEnvelope::decode(&record.envelope).map_err(|e| e.to_string())?;
            updates.push(revision_backfill_update(
                loom,
                entity_id,
                record.operation_id.clone(),
                record.root_after,
                &record.envelope,
                "application/vnd.uldren.loom.lifecycle.operation+cbor",
                envelope.timestamp_ms,
                format!("lifecycle:{}:backfill:1", record.instance_id),
            )?);
        }
    }
    apply_revision_backfill(loom, workspace, scope_id, "lifecycle", dry_run, updates)
}

fn apply_revision_backfill(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    scope_id: &str,
    profile: &str,
    dry_run: bool,
    updates: Vec<RevisionBackfillUpdate>,
) -> Result<RevisionRebuildReport, String> {
    let index_path = revision_index_path(scope_id).map_err(|e| e.to_string())?;
    let (mut index, index_present_before) = match loom.read_file_reserved(workspace, &index_path) {
        Ok(bytes) => (
            RevisionIndex::decode(&bytes).map_err(|e| e.to_string())?,
            true,
        ),
        Err(err) if err.code == Code::NotFound => (RevisionIndex::new(), false),
        Err(err) => return Err(err.to_string()),
    };
    let candidates = updates.len() as u64;
    let backfill = index
        .backfill_missing_current(scope_id, updates)
        .map_err(|e| e.to_string())?;
    if !dry_run && backfill.inserted > 0 {
        loom.create_directory_reserved(workspace, REVISION_INDEX_DIR, true)
            .map_err(|e| e.to_string())?;
        let encoded = index.encode().map_err(|e| e.to_string())?;
        loom.write_file_reserved(workspace, &index_path, &encoded, 0o100644)
            .map_err(|e| e.to_string())?;
    }
    Ok(RevisionRebuildReport {
        workspace: workspace.to_string(),
        scope_id: scope_id.to_string(),
        profile: profile.to_string(),
        index_present_before,
        candidates,
        inserted: backfill.inserted,
        skipped_existing: backfill.skipped_existing,
        dry_run,
    })
}

fn revision_backfill_update(
    loom: &Loom<FileStore>,
    entity_id: String,
    operation_id: String,
    root: Digest,
    body: &[u8],
    media_type: &str,
    timestamp_ms: u64,
    checkpoint_id: String,
) -> Result<RevisionBackfillUpdate, String> {
    RevisionBackfillUpdate::new(
        entity_id,
        operation_id,
        BodyRef::new(
            Digest::hash(loom.store().digest_algo(), body),
            body.len() as u64,
            media_type,
        )
        .map_err(|e| e.to_string())?,
        root,
        timestamp_ms,
        checkpoint_id,
    )
    .map_err(|e| e.to_string())
}

fn page_operation_revision_entity_id(operation_kind: &str, target_entity_id: &str) -> String {
    match operation_kind {
        "space.created" => format!("space:{target_entity_id}"),
        "page.created" | "page.updated" => format!("page:draft:{target_entity_id}"),
        "structure.created" => format!("structure:{target_entity_id}"),
        "structure.node_added"
        | "structure.node_updated"
        | "structure.node_bound"
        | "structure.node_moved" => format!("structure-node:{target_entity_id}"),
        "structure.node_linked" => format!("structure-edge:{target_entity_id}"),
        _ => format!("pages:operation:{target_entity_id}"),
    }
}

fn run_vector_workspace(action: VectorWorkspaceCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        VectorWorkspaceCmd::Configure {
            store,
            workspace,
            embedding_instance,
            format,
        } => {
            let embedding_instance = embedding_instance.ok_or_else(|| {
                "vector workspace configure requires --embedding-instance".to_string()
            })?;
            let mut opened = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&opened, &workspace)?;
            let mut state = load_inference_instance_state(&opened, workspace_id)?;
            let instance = state
                .find_instance(&embedding_instance)
                .cloned()
                .ok_or_else(|| format!("inference instance {embedding_instance:?} not found"))?;
            if instance.kind != InferenceModelKind::TextEmbedding {
                return Err(format!(
                    "inference instance {embedding_instance:?} is not a text-embedding instance"
                ));
            }
            let binding = loom_inference::VectorWorkspaceBinding {
                store: store.clone(),
                workspace: workspace_id.to_string(),
                embedding_instance: embedding_instance.clone(),
            };
            state.upsert_vector_binding(binding.clone());
            save_inference_instance_state(&mut opened, workspace_id, &state)?;
            print_vector_workspace_binding(&binding, &format)
        }
    }
}

fn enqueue_studio_reindex(
    store: &str,
    workspace: &str,
    profile: &str,
    instance: Option<&loom_types::InferenceInstanceDescriptor>,
    keys: &KeyOpts,
) -> Result<StudioReindexEnqueueResult, String> {
    let mut opened = cli_open_loom(store, keys)?;
    let ns = resolve_ns(&opened, workspace)?;
    let source_digest = studio_reindex_source_digest(&opened, ns, profile)?;
    let job = studio_reindex_job(ns, profile, source_digest, instance)?;
    let job_path = job
        .job_path(opened.store().digest_algo())
        .map_err(|e| e.to_string())?;
    opened
        .create_directory_reserved(ns, EMBEDDING_PROJECTION_JOBS_DIR, true)
        .map_err(|e| e.to_string())?;
    opened
        .write_file_reserved(
            ns,
            &job_path,
            &job.encode().map_err(|e| e.to_string())?,
            0o100644,
        )
        .map_err(|e| e.to_string())?;
    let mut vector_records_indexed = 0usize;
    let mut vector_records_deleted = 0usize;
    if let Some(resolved) = resolve_optional_vector_binding(&opened, ns, instance)? {
        let summary = drain_meetings_vector_outputs(&mut opened, ns, profile, &resolved)?;
        vector_records_indexed = summary.indexed;
        vector_records_deleted = summary.deleted;
    }
    save_loom(&mut opened).map_err(|e| e.to_string())?;
    Ok(StudioReindexEnqueueResult {
        workspace_id: ns,
        profile: profile.to_string(),
        job_path,
        state: job.state.as_str().to_string(),
        source_digest,
        model_id: job.stamp.model_id,
        vector_records_indexed,
        vector_records_deleted,
    })
}

struct StudioVectorDrainSummary {
    indexed: usize,
    deleted: usize,
}

fn resolve_optional_vector_binding(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    instance: Option<&loom_types::InferenceInstanceDescriptor>,
) -> Result<Option<ResolvedTextEmbeddingInstance>, String> {
    let cache_dir = inference_cache_dir(None)?;
    let mut hardware = loom_inference::probe_hardware().map_err(|e| e.to_string())?;
    hardware.hf_cache_dir = Some(cache_dir.to_string_lossy().into_owned());
    let state = load_inference_instance_state(loom, workspace)?;
    let instance_name = match instance {
        Some(instance) => instance.name.clone(),
        None => match state
            .vector_bindings
            .iter()
            .find(|binding| binding.workspace == workspace.to_string())
        {
            Some(binding) => binding.embedding_instance.clone(),
            None => return Ok(None),
        },
    };
    resolve_vector_text_embedding_instance_from_cache(
        &cache_dir,
        hardware,
        loom,
        workspace,
        Some(&instance_name),
    )
    .map(Some)
}

fn drain_meetings_vector_outputs(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    profile: &str,
    resolved: &ResolvedTextEmbeddingInstance,
) -> Result<StudioVectorDrainSummary, String> {
    let model = resolved
        .handle
        .model()
        .ok_or_else(|| "text embedding provider did not expose a model".to_string())?;
    let mut summary = StudioVectorDrainSummary {
        indexed: 0,
        deleted: 0,
    };
    for profile_id in studio_meetings_profile_ids(ns, profile) {
        let Some(snapshot) =
            load_meetings_snapshot_io(loom, &profile_id).map_err(|e| e.to_string())?
        else {
            continue;
        };
        let profile_root = Digest::hash(
            loom.store().digest_algo(),
            &snapshot.encode().map_err(|e| e.to_string())?,
        );
        let output_set =
            ProjectionOutputSet::from_snapshot(&snapshot).map_err(|e| e.to_string())?;
        let collection = meetings_vector_collection(&profile_id);
        match loom_core::vector_create(loom, ns, &collection, model.dimension, Metric::Cosine) {
            Ok(()) => {}
            Err(err) if err.code == Code::Conflict => {}
            Err(err) => return Err(err.to_string()),
        }
        for output in output_set.outputs_for(ProjectionKind::Vector) {
            let job =
                meetings_vector_projection_job(ns, &profile_id, profile_root, output, resolved)?;
            let path = job
                .job_path(loom.store().digest_algo())
                .map_err(|e| e.to_string())?;
            match output.action {
                ProjectionAction::Upsert | ProjectionAction::Append => {
                    loom_core::vector_upsert_text(
                        loom,
                        ns,
                        &collection,
                        &meetings_vector_id(output),
                        &output.text_body(),
                        meetings_vector_metadata(output),
                        &resolved.handle,
                    )
                    .map_err(|e| e.to_string())?;
                    summary.indexed = summary.indexed.saturating_add(1);
                }
                ProjectionAction::Invalidate | ProjectionAction::RetainMetadata => {
                    let removed = loom_core::vector_delete(
                        loom,
                        ns,
                        &collection,
                        &meetings_vector_id(output),
                    )
                    .map_err(|e| e.to_string())?;
                    if removed {
                        summary.deleted = summary.deleted.saturating_add(1);
                    }
                }
            }
            loom.create_directory_reserved(ns, EMBEDDING_PROJECTION_JOBS_DIR, true)
                .map_err(|e| e.to_string())?;
            loom.write_file_reserved(
                ns,
                &path,
                &job.ready().encode().map_err(|e| e.to_string())?,
                0o100644,
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(summary)
}

fn studio_meetings_profile_ids(ns: WorkspaceId, profile: &str) -> Vec<String> {
    match profile {
        "all" | "meetings" => vec![ns.to_string()],
        profile => vec![profile.to_string()],
    }
}

fn meetings_vector_collection(profile_id: &str) -> String {
    format!("meetings/{profile_id}")
}

fn meetings_vector_id(output: &ProjectionOutput) -> String {
    output
        .output_ref
        .strip_prefix("vector:")
        .unwrap_or(&output.output_ref)
        .to_string()
}

fn meetings_vector_metadata(output: &ProjectionOutput) -> BTreeMap<String, loom_core::Value> {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "entity_kind".to_string(),
        loom_core::Value::Text(output.entity_kind.clone()),
    );
    metadata.insert(
        "entity_id".to_string(),
        loom_core::Value::Text(output.entity_id.clone()),
    );
    metadata.insert(
        "output_ref".to_string(),
        loom_core::Value::Text(output.output_ref.clone()),
    );
    metadata.insert(
        "output_id".to_string(),
        loom_core::Value::Text(output.output_id.clone()),
    );
    metadata.insert(
        "source_ids".to_string(),
        loom_core::Value::List(
            output
                .source_ids
                .iter()
                .cloned()
                .map(loom_core::Value::Text)
                .collect(),
        ),
    );
    metadata
}

fn meetings_vector_projection_job(
    ns: WorkspaceId,
    profile_id: &str,
    source_digest: Digest,
    output: &ProjectionOutput,
    resolved: &ResolvedTextEmbeddingInstance,
) -> Result<EmbeddingProjectionJob, String> {
    let key =
        EmbeddingProjectionKey::new(ns.to_string(), "meetings", profile_id, &output.output_id)
            .map_err(|e| e.to_string())?;
    let stamp = studio_reindex_stamp_for_instance(source_digest, &resolved.instance)?;
    Ok(EmbeddingProjectionJob::queued(key, stamp))
}

fn studio_reindex_source_digest(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    profile: &str,
) -> Result<Digest, String> {
    let head = loom.registry().head_branch(ns).map_err(|e| e.to_string())?;
    if let Some(tip) = loom
        .registry()
        .branch_tip(ns, &head)
        .map_err(|e| e.to_string())?
    {
        Ok(tip)
    } else {
        let seed = format!("studio-reindex:{ns}:{profile}");
        Ok(Digest::hash(loom.store().digest_algo(), seed.as_bytes()))
    }
}

fn studio_reindex_job(
    ns: WorkspaceId,
    profile: &str,
    source_digest: Digest,
    instance: Option<&loom_types::InferenceInstanceDescriptor>,
) -> Result<EmbeddingProjectionJob, String> {
    let key = EmbeddingProjectionKey::new(ns.to_string(), "studio", profile, "reindex")
        .map_err(|e| e.to_string())?;
    let stamp = match instance {
        Some(instance) => studio_reindex_stamp_for_instance(source_digest, instance)?,
        None => EmbeddingProjectionStamp::new(
            source_digest,
            "loom-built-in-embedding",
            None,
            "unconfigured",
        )
        .map_err(|e| e.to_string())?,
    };
    let job = EmbeddingProjectionJob::queued(key, stamp);
    match instance {
        Some(_) => Ok(job),
        None => job
            .no_engine("built-in embedding inference is not configured")
            .map_err(|e| e.to_string()),
    }
}

fn studio_reindex_stamp_for_instance(
    source_digest: Digest,
    instance: &loom_types::InferenceInstanceDescriptor,
) -> Result<EmbeddingProjectionStamp, String> {
    let descriptor_bytes = serde_json::to_vec(instance).map_err(|e| e.to_string())?;
    let descriptor_digest = Digest::hash(source_digest.algo(), &descriptor_bytes);
    EmbeddingProjectionStamp::new(
        source_digest,
        format!(
            "{}@{}",
            instance.model.repo_id,
            instance.model.revision.value()
        ),
        None,
        format!(
            "{}:{}",
            instance.runtime.as_str(),
            descriptor_digest.to_hex()
        ),
    )
    .map_err(|e| e.to_string())
}

fn print_studio_reindex_enqueue(
    result: &StudioReindexEnqueueResult,
    format: &str,
) -> Result<(), String> {
    match format {
        "text" => {
            println!(
                "studio_reindex\t{}\tprofile={}\tstate={}\tindexed={}\tdeleted={}\tjob={}",
                result.workspace_id,
                result.profile,
                result.state,
                result.vector_records_indexed,
                result.vector_records_deleted,
                result.job_path
            );
            Ok(())
        }
        "json" => {
            let mut out = String::new();
            out.push('{');
            out.push_str("\"workspace\":");
            out.push_str(&json_string(&result.workspace_id.to_string()));
            out.push_str(",\"profile\":");
            out.push_str(&json_string(&result.profile));
            out.push_str(",\"state\":");
            out.push_str(&json_string(&result.state));
            out.push_str(",\"job_path\":");
            out.push_str(&json_string(&result.job_path));
            out.push_str(",\"source_digest\":");
            out.push_str(&json_string(&result.source_digest.to_string()));
            out.push_str(",\"model_id\":");
            out.push_str(&json_string(&result.model_id));
            out.push_str(",\"vector_records_indexed\":");
            out.push_str(&result.vector_records_indexed.to_string());
            out.push_str(",\"vector_records_deleted\":");
            out.push_str(&result.vector_records_deleted.to_string());
            out.push('}');
            println!("{out}");
            Ok(())
        }
        other => Err(format!(
            "unknown studio reindex output format {other:?} (expected text or json)"
        )),
    }
}

fn print_revision_rebuild_report(
    report: &RevisionRebuildReport,
    format: &str,
) -> Result<(), String> {
    match format {
        "text" => {
            println!(
                "studio_revisions_rebuild\t{}\tprofile={}\tcandidates={}\tinserted={}\tskipped_existing={}\tdry_run={}",
                report.workspace,
                report.profile,
                report.candidates,
                report.inserted,
                report.skipped_existing,
                report.dry_run
            );
            Ok(())
        }
        "json" => {
            let body = serde_json::json!({
                "workspace": &report.workspace,
                "scope_id": &report.scope_id,
                "profile": &report.profile,
                "index_present_before": report.index_present_before,
                "candidates": report.candidates,
                "inserted": report.inserted,
                "skipped_existing": report.skipped_existing,
                "dry_run": report.dry_run,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        other => Err(format!(
            "unknown studio revisions rebuild output format {other:?} (expected text or json)"
        )),
    }
}

fn print_vector_workspace_binding(
    binding: &loom_inference::VectorWorkspaceBinding,
    format: &str,
) -> Result<(), String> {
    match format {
        "text" => {
            println!(
                "vector_workspace\t{}\t{}\tembedding_instance={}",
                binding.store, binding.workspace, binding.embedding_instance
            );
            Ok(())
        }
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(binding).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        other => Err(format!(
            "unknown vector workspace output format {other:?} (expected text or json)"
        )),
    }
}

fn run_graph(action: GraphCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        GraphCmd::UpsertNode {
            store,
            workspace,
            name,
            id,
            props,
        } => {
            let props = match props {
                Some(path) => read_input(&path).map_err(|e| e.to_string())?,
                None => Vec::new(),
            };
            let client = remote::open_store_client(&store)?;
            client.g_upsert_node(keys, &workspace, &name, &id, props)
        }
        GraphCmd::GetNode {
            store,
            workspace,
            name,
            id,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(bytes) = client.g_get_node(keys, &workspace, &name, &id)? else {
                return Err(format!("graph node {id:?} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        GraphCmd::RemoveNode {
            store,
            workspace,
            name,
            id,
            cascade,
        } => {
            let client = remote::open_store_client(&store)?;
            client.g_remove_node(keys, &workspace, &name, &id, cascade)
        }
        GraphCmd::UpsertEdge {
            store,
            workspace,
            name,
            id,
            src,
            dst,
            label,
            props,
        } => {
            let props = match props {
                Some(path) => read_input(&path).map_err(|e| e.to_string())?,
                None => Vec::new(),
            };
            let client = remote::open_store_client(&store)?;
            client.g_upsert_edge(keys, &workspace, &name, &id, &src, &dst, &label, props)
        }
        GraphCmd::GetEdge {
            store,
            workspace,
            name,
            id,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(bytes) = client.g_get_edge(keys, &workspace, &name, &id)? else {
                return Err(format!("graph edge {id:?} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        GraphCmd::RemoveEdge {
            store,
            workspace,
            name,
            id,
        } => {
            let client = remote::open_store_client(&store)?;
            let present = client.g_remove_edge(keys, &workspace, &name, &id)?;
            println!("{present}");
            Ok(())
        }
        GraphCmd::Neighbors {
            store,
            workspace,
            name,
            id,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded = client.g_neighbors(keys, &workspace, &name, &id)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        GraphCmd::OutEdges {
            store,
            workspace,
            name,
            id,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded = client.g_out_edges(keys, &workspace, &name, &id)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        GraphCmd::InEdges {
            store,
            workspace,
            name,
            id,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded = client.g_in_edges(keys, &workspace, &name, &id)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        GraphCmd::Reachable {
            store,
            workspace,
            name,
            start,
            max_depth,
            via_label,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded = client.g_reachable(
                keys,
                &workspace,
                &name,
                &start,
                max_depth,
                via_label.as_deref(),
            )?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        GraphCmd::ShortestPath {
            store,
            workspace,
            name,
            from,
            to,
            via_label,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(path) = client.g_shortest_path(
                keys,
                &workspace,
                &name,
                &from,
                &to,
                via_label.as_deref(),
            )?
            else {
                return Err(format!("no graph path from {from:?} to {to:?}"));
            };
            write_output(out.as_deref(), &path).map_err(|e| e.to_string())
        }
        GraphCmd::Query {
            store,
            workspace,
            name,
            query,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded = client.g_query(keys, &workspace, &name, &query)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        GraphCmd::ExplainQuery {
            store,
            workspace,
            name,
            query,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded = client.g_explain_query(keys, &workspace, &name, &query)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
    }
}

fn run_ledger(action: LedgerCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        LedgerCmd::Append {
            store,
            workspace,
            collection,
            payload,
        } => {
            let payload = read_input(&payload).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            let seq = client.ledger_append(keys, &workspace, &collection, payload)?;
            println!("{seq}");
            Ok(())
        }
        LedgerCmd::Get {
            store,
            workspace,
            collection,
            seq,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(payload) = client.ledger_get(keys, &workspace, &collection, seq)? else {
                return Err(format!("ledger entry {seq} not found"));
            };
            write_output(out.as_deref(), &payload).map_err(|e| e.to_string())
        }
        LedgerCmd::Head {
            store,
            workspace,
            collection,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let Some(head) = client.ledger_head(keys, &workspace, &collection)? else {
                return Err("ledger is empty".to_string());
            };
            if let Some(out) = out {
                write_output(Some(&out), head.bytes()).map_err(|e| e.to_string())
            } else {
                println!("{head}");
                Ok(())
            }
        }
        LedgerCmd::Len {
            store,
            workspace,
            collection,
        } => {
            let client = remote::open_store_client(&store)?;
            let len = client.ledger_len(keys, &workspace, &collection)?;
            println!("{len}");
            Ok(())
        }
        LedgerCmd::Verify {
            store,
            workspace,
            collection,
        } => {
            let client = remote::open_store_client(&store)?;
            client.ledger_verify(keys, &workspace, &collection)?;
            println!("ok");
            Ok(())
        }
    }
}

fn run_metrics(action: MetricsCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        MetricsCmd::PutDescriptor {
            store,
            workspace,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let descriptor =
                loom_core::MetricDescriptor::decode(&bytes).map_err(|e| e.to_string())?;
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = ensure_facet_workspace(&mut loom, &workspace, FacetKind::Metrics)?;
            loom_core::metrics_put_descriptor(&mut loom, ns, &descriptor)
                .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())
        }
        MetricsCmd::GetDescriptor {
            store,
            workspace,
            name,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let Some(descriptor) =
                loom_core::metrics_get_descriptor(&loom, ns, &name).map_err(|e| e.to_string())?
            else {
                return Err(format!("metric descriptor {name:?} not found"));
            };
            let bytes = descriptor.encode().map_err(|e| e.to_string())?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        MetricsCmd::PutObservation {
            store,
            workspace,
            descriptor,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let observation =
                loom_core::MetricObservation::decode(&bytes).map_err(|e| e.to_string())?;
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = ensure_facet_workspace(&mut loom, &workspace, FacetKind::Metrics)?;
            loom_core::metrics_put_observation(&mut loom, ns, &descriptor, &observation)
                .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())
        }
        MetricsCmd::Query {
            store,
            workspace,
            descriptor,
            from,
            to,
            max_series,
            max_groups,
            max_samples,
            max_output_bytes,
            now,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let result = loom_core::metrics_query_observations(
                &loom,
                ns,
                &descriptor,
                &loom_core::MetricQuery {
                    from_timestamp_ms: from,
                    to_timestamp_ms: to,
                    max_series,
                    max_groups,
                    max_samples,
                    max_output_bytes,
                    now_timestamp_ms: now,
                },
            )
            .map_err(|e| e.to_string())?;
            let bytes = metrics_query_result_cbor(result)?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
    }
}

fn metrics_query_result_cbor(result: loom_core::MetricQueryResult) -> Result<Vec<u8>, String> {
    let observations = result
        .observations
        .iter()
        .map(|observation| {
            observation
                .encode()
                .map(WireValue::Bytes)
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    loom_codec::encode(&WireValue::Array(vec![
        WireValue::Array(observations),
        WireValue::Bool(result.partial),
        WireValue::Bool(result.stale),
    ]))
    .map_err(|e| e.to_string())
}

fn run_logs(action: LogsCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        LogsCmd::PutRecord {
            store,
            workspace,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let record = loom_core::LogRecord::decode(&bytes).map_err(|e| e.to_string())?;
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = ensure_facet_workspace(&mut loom, &workspace, FacetKind::Logs)?;
            let record_id =
                loom_core::logs_put_record(&mut loom, ns, &record).map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            println!("{record_id}");
            Ok(())
        }
        LogsCmd::GetRecord {
            store,
            workspace,
            record_id,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let Some(record) =
                loom_core::logs_get_record(&loom, ns, &record_id).map_err(|e| e.to_string())?
            else {
                return Err(format!("log record {record_id:?} not found"));
            };
            let bytes = record.encode().map_err(|e| e.to_string())?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        LogsCmd::Query {
            store,
            workspace,
            from,
            to,
            max_records,
            max_output_bytes,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let result = loom_core::logs_query(
                &loom,
                ns,
                &loom_core::LogQuery {
                    from_time_unix_nano: from,
                    to_time_unix_nano: to,
                    max_records,
                    max_output_bytes,
                },
            )
            .map_err(|e| e.to_string())?;
            let bytes = log_query_result_cbor(result)?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
    }
}

fn log_query_result_cbor(result: loom_core::LogQueryResult) -> Result<Vec<u8>, String> {
    let records = result
        .records
        .iter()
        .map(|record| {
            record
                .encode()
                .map(WireValue::Bytes)
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    loom_codec::encode(&WireValue::Array(vec![
        WireValue::Array(records),
        WireValue::Bool(result.partial),
    ]))
    .map_err(|e| e.to_string())
}

fn run_traces(action: TracesCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        TracesCmd::PutSpan {
            store,
            workspace,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let span = loom_core::SpanRecord::decode(&bytes).map_err(|e| e.to_string())?;
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = ensure_facet_workspace(&mut loom, &workspace, FacetKind::Traces)?;
            loom_core::traces_put_span(&mut loom, ns, &span).map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())
        }
        TracesCmd::GetSpan {
            store,
            workspace,
            trace_id,
            span_id,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let Some(span) = loom_core::traces_get_span(&loom, ns, &trace_id, &span_id)
                .map_err(|e| e.to_string())?
            else {
                return Err(format!("span {trace_id}/{span_id} not found"));
            };
            let bytes = span.encode().map_err(|e| e.to_string())?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        TracesCmd::TraceSpans {
            store,
            workspace,
            trace_id,
            max_spans,
            max_output_bytes,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let result =
                loom_core::traces_trace_spans(&loom, ns, &trace_id, max_spans, max_output_bytes)
                    .map_err(|e| e.to_string())?;
            let bytes = trace_query_result_cbor(result)?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        TracesCmd::Query {
            store,
            workspace,
            from,
            to,
            max_spans,
            max_output_bytes,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let result = loom_core::traces_query(
                &loom,
                ns,
                &loom_core::TraceQuery {
                    from_start_time_ns: from,
                    to_start_time_ns: to,
                    max_spans,
                    max_output_bytes,
                },
            )
            .map_err(|e| e.to_string())?;
            let bytes = trace_query_result_cbor(result)?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
    }
}

fn trace_query_result_cbor(result: loom_core::TraceQueryResult) -> Result<Vec<u8>, String> {
    let spans = result
        .spans
        .iter()
        .map(|span| {
            span.encode()
                .map(WireValue::Bytes)
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    loom_codec::encode(&WireValue::Array(vec![
        WireValue::Array(spans),
        WireValue::Bool(result.partial),
    ]))
    .map_err(|e| e.to_string())
}

fn run_program(action: ProgramCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        ProgramCmd::PutWasm {
            store,
            workspace,
            name,
            input,
            out,
        } => {
            let body = read_input(&input).map_err(|e| e.to_string())?;
            let manifest = loom_compute::Manifest::for_wasm(
                &name,
                &body,
                loom_compute::GrantSet::all_facets(),
            );
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = ensure_facet_workspace(&mut loom, &workspace, FacetKind::Program)?;
            let record = loom_compute::program_put_wasm(&mut loom, ns, &name, manifest, &body)
                .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            write_output(out.as_deref(), &program_record_cbor(&record)?).map_err(|e| e.to_string())
        }
        ProgramCmd::PutTemplate {
            store,
            workspace,
            name,
            input,
            out,
        } => {
            let body = read_input(&input).map_err(|e| e.to_string())?;
            let source = String::from_utf8(body)
                .map_err(|_| "template program body must be UTF-8".to_string())?;
            let manifest = loom_compute::Manifest::for_template(
                &name,
                &source,
                loom_compute::GrantSet::all_facets(),
            );
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = ensure_facet_workspace(&mut loom, &workspace, FacetKind::Program)?;
            let record =
                loom_compute::program_put_template(&mut loom, ns, &name, manifest, &source)
                    .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            write_output(out.as_deref(), &program_record_cbor(&record)?).map_err(|e| e.to_string())
        }
        ProgramCmd::PutCel {
            store,
            workspace,
            name,
            input,
            out,
        } => {
            let body = read_input(&input).map_err(|e| e.to_string())?;
            let source = String::from_utf8(body)
                .map_err(|_| "cel program body must be UTF-8".to_string())?;
            let manifest = loom_compute::Manifest::for_cel(
                &name,
                &source,
                loom_compute::GrantSet::all_facets(),
            );
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = ensure_facet_workspace(&mut loom, &workspace, FacetKind::Program)?;
            let record = loom_compute::program_put_cel(&mut loom, ns, &name, manifest, &source)
                .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            write_output(out.as_deref(), &program_record_cbor(&record)?).map_err(|e| e.to_string())
        }
        ProgramCmd::Inspect {
            store,
            workspace,
            name,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let Some(record) =
                loom_compute::program_inspect(&loom, ns, &name).map_err(|e| e.to_string())?
            else {
                return Err(format!("program {name:?} not found"));
            };
            write_output(out.as_deref(), &program_record_cbor(&record)?).map_err(|e| e.to_string())
        }
        ProgramCmd::Get {
            store,
            workspace,
            name,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let Some(program) =
                loom_compute::program_get(&loom, ns, &name).map_err(|e| e.to_string())?
            else {
                return Err(format!("program {name:?} not found"));
            };
            write_output(out.as_deref(), &program.body).map_err(|e| e.to_string())
        }
        ProgramCmd::List {
            store,
            workspace,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let records = loom_compute::program_list(&loom, ns).map_err(|e| e.to_string())?;
            write_output(out.as_deref(), &program_list_cbor(&records)?).map_err(|e| e.to_string())
        }
        ProgramCmd::Remove {
            store,
            workspace,
            name,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let removed =
                loom_compute::program_remove(&mut loom, ns, &name).map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            println!("{}", if removed { "removed" } else { "missing" });
            Ok(())
        }
    }
}

fn program_list_cbor(records: &[loom_compute::StoredProgram]) -> Result<Vec<u8>, String> {
    let records = records
        .iter()
        .map(program_record_value)
        .collect::<Result<Vec<_>, _>>()?;
    loom_codec::encode(&WireValue::Array(records)).map_err(|e| e.to_string())
}

fn program_record_cbor(record: &loom_compute::StoredProgram) -> Result<Vec<u8>, String> {
    loom_codec::encode(&program_record_value(record)?).map_err(|e| e.to_string())
}

fn program_record_value(record: &loom_compute::StoredProgram) -> Result<WireValue, String> {
    Ok(WireValue::Map(vec![
        text_value(
            "schema",
            WireValue::Text("loom.program.record.summary.v1".to_string()),
        ),
        text_value("name", WireValue::Text(record.name.clone())),
        text_value("engine", WireValue::Text(record.manifest.engine.clone())),
        text_value(
            "abi_version",
            WireValue::Uint(record.manifest.abi_version.into()),
        ),
        text_value("entry", WireValue::Text(record.manifest.entry.clone())),
        text_value(
            "manifest_digest",
            WireValue::Bytes(record.manifest_digest.bytes().to_vec()),
        ),
        text_value(
            "body_digest",
            WireValue::Bytes(record.body_digest.bytes().to_vec()),
        ),
        text_value("body_len", WireValue::Uint(record.body_len)),
        text_value("manifest", WireValue::Bytes(record.manifest.encode())),
    ]))
}

fn text_value(key: &str, value: WireValue) -> (WireValue, WireValue) {
    (WireValue::Text(key.to_string()), value)
}

fn run_columnar(action: ColumnarCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        ColumnarCmd::Create {
            store,
            workspace,
            name,
            columns,
            target_segment_rows,
        } => {
            let columns = read_input(&columns).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            client.col_create(keys, &workspace, &name, columns, target_segment_rows as u64)?;
            println!("created {name}");
            Ok(())
        }
        ColumnarCmd::Append {
            store,
            workspace,
            name,
            row,
        } => {
            let row = read_input(&row).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            client.col_append(keys, &workspace, &name, row)
        }
        ColumnarCmd::Scan {
            store,
            workspace,
            name,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded = client.col_scan(keys, &workspace, &name)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        ColumnarCmd::Columns {
            store,
            workspace,
            name,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded = client.col_columns(keys, &workspace, &name)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        ColumnarCmd::Rows {
            store,
            workspace,
            name,
        } => {
            let client = remote::open_store_client(&store)?;
            let rows = client.col_rows(keys, &workspace, &name)?;
            println!("{rows}");
            Ok(())
        }
        ColumnarCmd::Compact {
            store,
            workspace,
            name,
        } => {
            let client = remote::open_store_client(&store)?;
            client.col_compact(keys, &workspace, &name)
        }
        ColumnarCmd::Inspect {
            store,
            workspace,
            name,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let encoded = client.col_inspect(keys, &workspace, &name)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        ColumnarCmd::SourceDigest {
            store,
            workspace,
            name,
        } => {
            let client = remote::open_store_client(&store)?;
            let digest = client.col_source_digest(keys, &workspace, &name)?;
            println!("{digest}");
            Ok(())
        }
        ColumnarCmd::Select {
            store,
            workspace,
            name,
            columns,
            filter,
            out,
        } => {
            let columns = read_input(&columns).map_err(|e| e.to_string())?;
            let filter = match filter {
                Some(path) => read_input(&path).map_err(|e| e.to_string())?,
                None => Vec::new(),
            };
            let client = remote::open_store_client(&store)?;
            let encoded = client.col_select(keys, &workspace, &name, columns, filter)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        ColumnarCmd::Aggregate {
            store,
            workspace,
            name,
            aggregates,
            filter,
            out,
        } => {
            let aggregates = read_input(&aggregates).map_err(|e| e.to_string())?;
            let filter = match filter {
                Some(path) => read_input(&path).map_err(|e| e.to_string())?,
                None => Vec::new(),
            };
            let client = remote::open_store_client(&store)?;
            let encoded = client.col_aggregate(keys, &workspace, &name, aggregates, filter)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        ColumnarCmd::ImportArrow {
            store,
            workspace,
            name,
            input,
            target_segment_rows,
            replace,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let dataset = loom_core::columnar_from_arrow_ipc(&bytes, target_segment_rows)
                .map_err(|e| e.to_string())?;
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = ensure_facet_workspace(&mut loom, &workspace, FacetKind::Columnar)?;
            ensure_columnar_import_target(&loom, ns, &name, replace)?;
            loom_core::put_columnar(&mut loom, ns, &name, &dataset).map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())
        }
        ColumnarCmd::ExportArrow {
            store,
            workspace,
            name,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let dataset = loom_core::get_columnar(&loom, ns, &name).map_err(|e| e.to_string())?;
            let bytes = loom_core::columnar_to_arrow_ipc(&dataset).map_err(|e| e.to_string())?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        ColumnarCmd::ImportParquet {
            store,
            workspace,
            name,
            input,
            target_segment_rows,
            replace,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let dataset = loom_core::columnar_from_parquet(&bytes, target_segment_rows)
                .map_err(|e| e.to_string())?;
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = ensure_facet_workspace(&mut loom, &workspace, FacetKind::Columnar)?;
            ensure_columnar_import_target(&loom, ns, &name, replace)?;
            loom_core::put_columnar(&mut loom, ns, &name, &dataset).map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())
        }
        ColumnarCmd::ExportParquet {
            store,
            workspace,
            name,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let dataset = loom_core::get_columnar(&loom, ns, &name).map_err(|e| e.to_string())?;
            let bytes = loom_core::columnar_to_parquet(&dataset).map_err(|e| e.to_string())?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
    }
}

fn run_dataframe(action: DataframeCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        DataframeCmd::Create {
            store,
            workspace,
            name,
            plan,
        } => {
            let plan =
                loom_core::DataframePlan::decode(&read_input(&plan).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = ensure_facet_workspace(&mut loom, &workspace, FacetKind::Dataframe)?;
            loom_core::dataframe_create(&mut loom, ns, &name, &plan).map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            println!("created {name}");
            Ok(())
        }
        DataframeCmd::Collect {
            store,
            workspace,
            name,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let batch =
                loom_core::dataframe_collect(&loom, ns, &name).map_err(|e| e.to_string())?;
            write_output(out.as_deref(), &dataframe_batch_cbor(batch)?).map_err(|e| e.to_string())
        }
        DataframeCmd::Materialize {
            store,
            workspace,
            name,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let digest = loom_core::dataframe_materialize(&mut loom, ns, &name)
                .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            if let Some(digest) = digest {
                println!("{digest}");
            } else {
                println!("ok");
            }
            Ok(())
        }
        DataframeCmd::PlanDigest {
            store,
            workspace,
            name,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let digest =
                loom_core::dataframe_plan_digest(&loom, ns, &name).map_err(|e| e.to_string())?;
            println!("{digest}");
            Ok(())
        }
        DataframeCmd::Preview {
            store,
            workspace,
            name,
            rows,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let batch =
                loom_core::dataframe_preview(&loom, ns, &name, rows).map_err(|e| e.to_string())?;
            write_output(out.as_deref(), &dataframe_batch_cbor(batch)?).map_err(|e| e.to_string())
        }
        DataframeCmd::SourceDigests {
            store,
            workspace,
            name,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let digests = loom_core::dataframe_source_digests(&loom, ns, &name)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|digest| digest.to_string())
                .collect::<Vec<_>>();
            write_output(out.as_deref(), &text_array_cbor(&digests)?).map_err(|e| e.to_string())
        }
    }
}

fn run_search(action: SearchCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        SearchCmd::Create {
            store,
            workspace,
            name,
            mapping,
        } => {
            let mapping = read_input(&mapping).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            client.search_create(keys, &workspace, &name, mapping)?;
            println!("created {name}");
            Ok(())
        }
        SearchCmd::Index {
            store,
            workspace,
            name,
            mut id,
            id_file,
            mut doc,
        } => {
            // With `--id-file`, the id positional slot may carry the doc input instead.
            if id_file.is_some() && doc.is_none() {
                doc = id.take();
            }
            let doc = doc.ok_or_else(|| "missing doc input".to_string())?;
            let id = search_bytes_arg(id, id_file, "id")?;
            let doc = read_input(&doc).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            client.search_index(keys, &workspace, &name, id, doc)
        }
        SearchCmd::Get {
            store,
            workspace,
            name,
            id,
            id_file,
            out,
        } => {
            let id = search_bytes_arg(id, id_file, "id")?;
            let client = remote::open_store_client(&store)?;
            let Some(doc) = client.search_get(keys, &workspace, &name, id)? else {
                return Err("search document not found".to_string());
            };
            write_output(out.as_deref(), &doc).map_err(|e| e.to_string())
        }
        SearchCmd::Delete {
            store,
            workspace,
            name,
            id,
            id_file,
        } => {
            let id = search_bytes_arg(id, id_file, "id")?;
            let client = remote::open_store_client(&store)?;
            let present = client.search_delete(keys, &workspace, &name, id)?;
            println!("{present}");
            Ok(())
        }
        SearchCmd::Ids {
            store,
            workspace,
            name,
            prefix,
            prefix_file,
            out,
        } => {
            let prefix = search_optional_bytes_arg(prefix, prefix_file, "prefix")?;
            let client = remote::open_store_client(&store)?;
            let ids = client.search_ids(keys, &workspace, &name, prefix)?;
            write_output(out.as_deref(), &ids).map_err(|e| e.to_string())
        }
        SearchCmd::Remap {
            store,
            workspace,
            name,
            mapping,
        } => {
            let mapping = read_input(&mapping).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            client.search_remap(keys, &workspace, &name, mapping)
        }
        SearchCmd::Query {
            store,
            workspace,
            name,
            request,
            out,
        } => {
            let request = read_input(&request).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            let response = client.search_query(keys, &workspace, &name, request)?;
            write_output(out.as_deref(), &response).map_err(|e| e.to_string())
        }
        SearchCmd::Rebuild {
            store,
            workspace,
            name,
            engine_version,
            format,
        } => rebuild_search_tantivy_index(keys, &store, &workspace, &name, engine_version, &format),
        SearchCmd::Status {
            store,
            workspace,
            name,
            engine_version,
            format,
        } => {
            let client = remote::open_store_client(&store)?;
            let (ws_display, source_digest, status) =
                client.search_status(keys, &workspace, &name, &engine_version)?;
            print_search_status(
                &format,
                &ws_display,
                &name,
                source_digest,
                &engine_version,
                &status,
            )
        }
    }
}

#[derive(Debug)]
struct UnifiedSearchHit {
    workspace: String,
    collection: String,
    entity_id: String,
    field: String,
    snippet: String,
}

struct UnifiedSearchArgs {
    store: String,
    query: String,
    workspace: Option<String>,
    collection: Option<String>,
    field: Option<String>,
    limit: u32,
    offset: u32,
    format: String,
}

struct UnifiedSearchReadArgs<'a> {
    query: &'a str,
    workspace: Option<&'a str>,
    collection: Option<&'a str>,
    field: Option<&'a str>,
    limit: u32,
    offset: u32,
}

fn run_unified_search(args: UnifiedSearchArgs, keys: &KeyOpts) -> Result<(), String> {
    if args.query.is_empty() {
        return Err("search query must not be empty".to_string());
    }
    let loom = cli_open_loom_read(&args.store, keys)?;
    let hits = collect_unified_search_hits(
        &loom,
        UnifiedSearchReadArgs {
            query: &args.query,
            workspace: args.workspace.as_deref(),
            collection: args.collection.as_deref(),
            field: args.field.as_deref(),
            limit: args.limit,
            offset: args.offset,
        },
    )?;
    print_unified_search(&args.format, &hits)
}

fn collect_unified_search_hits(
    loom: &Loom<FileStore>,
    args: UnifiedSearchReadArgs<'_>,
) -> Result<Vec<UnifiedSearchHit>, String> {
    if args.query.is_empty() {
        return Err("search query must not be empty".to_string());
    }
    let workspaces = match args.workspace {
        Some(workspace) => {
            let ns = resolve_ns(loom, workspace)?;
            let label = loom
                .registry()
                .list(None)
                .into_iter()
                .find(|info| info.id == ns)
                .map(|info| info.name)
                .unwrap_or_else(|| ns.to_string());
            vec![(ns, label)]
        }
        None => loom
            .registry()
            .list(Some(FacetKind::Search))
            .into_iter()
            .map(|info| (info.id, info.name))
            .collect(),
    };
    let lowered = args.query.to_ascii_lowercase();
    let mut hits = Vec::new();
    for (ns, workspace_label) in workspaces {
        let collections = match args.collection {
            Some(collection) => vec![collection.to_string()],
            None => search_collections(loom, ns).map_err(|e| e.to_string())?,
        };
        for collection in collections {
            for id in
                loom_core::search_ids(loom, ns, &collection, None).map_err(|e| e.to_string())?
            {
                let Some(doc) =
                    loom_core::search_get(loom, ns, &collection, &id).map_err(|e| e.to_string())?
                else {
                    continue;
                };
                for (field_name, value) in doc {
                    if args.field.is_some_and(|wanted| wanted != field_name) {
                        continue;
                    }
                    let FieldValue::Text(text) = value else {
                        continue;
                    };
                    let text_lower = text.to_ascii_lowercase();
                    let Some(start) = text_lower.find(&lowered) else {
                        continue;
                    };
                    hits.push(UnifiedSearchHit {
                        workspace: workspace_label.clone(),
                        collection: collection.clone(),
                        entity_id: hex_bytes(&id),
                        field: field_name,
                        snippet: snippet_text(&text, start, start + lowered.len()),
                    });
                }
            }
        }
    }
    hits.sort_by(|a, b| {
        a.workspace
            .cmp(&b.workspace)
            .then_with(|| a.collection.cmp(&b.collection))
            .then_with(|| a.entity_id.cmp(&b.entity_id))
            .then_with(|| a.field.cmp(&b.field))
    });
    let hits = hits
        .into_iter()
        .skip(args.offset as usize)
        .take(if args.limit == 0 {
            usize::MAX
        } else {
            args.limit as usize
        })
        .collect::<Vec<_>>();
    Ok(hits)
}

fn print_unified_search(format: &str, hits: &[UnifiedSearchHit]) -> Result<(), String> {
    match format {
        "text" => {
            println!("index_status\tlexical=ready semantic=not_built graph=not_built");
            println!("reduced\ttrue");
            println!("degraded\ttrue\treason=scan_backed_lexical");
            for hit in hits {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    hit.workspace, hit.collection, hit.entity_id, hit.field, hit.snippet
                );
            }
            Ok(())
        }
        "json" => {
            let mut out = String::new();
            out.push_str("{\"hits\":[");
            for (idx, hit) in hits.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                out.push_str("{\"workspace\":");
                out.push_str(&json_string(&hit.workspace));
                out.push_str(",\"collection\":");
                out.push_str(&json_string(&hit.collection));
                out.push_str(",\"entity_id\":");
                out.push_str(&json_string(&hit.entity_id));
                out.push_str(",\"field\":");
                out.push_str(&json_string(&hit.field));
                out.push_str(",\"snippet\":");
                out.push_str(&json_string(&hit.snippet));
                out.push('}');
            }
            out.push_str("],\"engine\":{\"rungs_available\":[\"lexical\"],\"rung_selected_ceiling\":\"lexical\",\"rrf_k\":60,\"rung_depth\":");
            out.push_str(&hits.len().to_string());
            out.push_str("},\"index_status\":{\"lexical\":\"ready\",\"semantic\":\"not_built\",\"graph\":\"not_built\"},\"reduced\":true,\"degraded\":{\"is_degraded\":true,\"reason\":\"scan_backed_lexical\"}}");
            println!("{out}");
            Ok(())
        }
        other => Err(format!(
            "unknown search output format {other:?} (expected text or json)"
        )),
    }
}

fn snippet_text(text: &str, start: usize, end: usize) -> String {
    let mut prefix = start.saturating_sub(40);
    while prefix > 0 && !text.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let mut suffix = (end + 40).min(text.len());
    while suffix < text.len() && !text.is_char_boundary(suffix) {
        suffix += 1;
    }
    text[prefix..suffix].to_string()
}

fn print_search_status(
    format: &str,
    workspace: &str,
    collection: &str,
    source_digest: Digest,
    engine_version: &str,
    status: &DerivedArtifactStatus,
) -> Result<(), String> {
    match format {
        "text" => {
            println!(
                "workspace\t{}\ncollection\t{}\nsource_digest\t{}\nengine_version\t{}\nstatus\t{}",
                workspace,
                collection,
                source_digest,
                engine_version,
                status.name()
            );
            match status {
                DerivedArtifactStatus::Ready { record }
                | DerivedArtifactStatus::Stale { record } => {
                    println!("payload_digest\t{}", record.payload_digest);
                    println!("payload_len\t{}", record.payload_len);
                }
                DerivedArtifactStatus::Rebuilding { run_id, .. } => {
                    println!("run_id\t{run_id}");
                }
                DerivedArtifactStatus::Failed { message, .. }
                | DerivedArtifactStatus::Unsupported { message, .. } => {
                    println!("message\t{message}");
                }
                DerivedArtifactStatus::Missing => {}
            }
        }
        "json" => println!(
            "{}",
            search_status_json(workspace, collection, source_digest, engine_version, status)
        ),
        other => {
            return Err(format!(
                "unknown fts status output format {other:?} (expected text or json)"
            ));
        }
    }
    Ok(())
}

fn rebuild_search_tantivy_index(
    keys: &KeyOpts,
    store: &str,
    workspace: &str,
    collection: &str,
    engine_version: Option<String>,
    format: &str,
) -> Result<(), String> {
    let loom = cli_open_loom_read(store, keys)?;
    let ns = resolve_ns(&loom, workspace)?;
    let source_digest =
        loom_core::search_source_digest(&loom, ns, collection).map_err(|e| e.to_string())?;
    let engine_version = search_tantivy_engine_version(engine_version)?;
    let rebuild = loom
        .store()
        .begin_search_tantivy_rebuild(ns, collection, source_digest, &engine_version)
        .map_err(|e| e.to_string())?;
    match rebuild {
        DerivedArtifactRebuild::AlreadyReady { record } => {
            let status = DerivedArtifactStatus::Ready { record };
            print_search_status(
                format,
                &ns.to_string(),
                collection,
                source_digest,
                &engine_version,
                &status,
            )
        }
        DerivedArtifactRebuild::Coalesced { run_id } => {
            let status = loom
                .store()
                .search_tantivy_status(ns, collection, source_digest, &engine_version)
                .map_err(|e| e.to_string())?;
            if !matches!(status, DerivedArtifactStatus::Rebuilding { .. }) {
                return Err(format!(
                    "search Tantivy rebuild {run_id} coalesced but status is {}",
                    status.name()
                ));
            }
            print_search_status(
                format,
                &ns.to_string(),
                collection,
                source_digest,
                &engine_version,
                &status,
            )
        }
        DerivedArtifactRebuild::Started { run_id } => finish_search_tantivy_rebuild(
            &loom,
            ns,
            collection,
            source_digest,
            &engine_version,
            &run_id,
            format,
        ),
    }
}

#[cfg(feature = "native-fts")]
fn search_tantivy_engine_version(engine_version: Option<String>) -> Result<String, String> {
    Ok(engine_version.unwrap_or_else(loom_tantivy::tantivy_search_engine_version))
}

#[cfg(not(feature = "native-fts"))]
fn search_tantivy_engine_version(engine_version: Option<String>) -> Result<String, String> {
    engine_version.ok_or_else(|| {
        "fts rebuild requires --engine-version when native FTS is disabled".to_string()
    })
}

#[cfg(feature = "native-fts")]
fn finish_search_tantivy_rebuild(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    collection: &str,
    source_digest: Digest,
    engine_version: &str,
    run_id: &str,
    format: &str,
) -> Result<(), String> {
    let search = loom_core::get_search(loom, workspace, collection).map_err(|e| e.to_string())?;
    let payload = match loom_tantivy::build_tantivy_index_payload(&search) {
        Ok(payload) => payload,
        Err(err) => {
            loom.store()
                .fail_search_tantivy_rebuild(
                    workspace,
                    collection,
                    run_id,
                    source_digest,
                    engine_version,
                    err.to_string(),
                )
                .map_err(|e| e.to_string())?;
            return Err(err.to_string());
        }
    };
    let record = loom
        .store()
        .finish_search_tantivy_rebuild(
            workspace,
            collection,
            run_id,
            source_digest,
            engine_version,
            &payload,
        )
        .map_err(|e| e.to_string())?;
    let status = DerivedArtifactStatus::Ready { record };
    print_search_status(
        format,
        &workspace.to_string(),
        collection,
        source_digest,
        engine_version,
        &status,
    )
}

#[cfg(not(feature = "native-fts"))]
fn finish_search_tantivy_rebuild(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    collection: &str,
    source_digest: Digest,
    engine_version: &str,
    run_id: &str,
    format: &str,
) -> Result<(), String> {
    let message = "native FTS is not enabled in this loom binary";
    loom.store()
        .fail_search_tantivy_rebuild(
            workspace,
            collection,
            run_id,
            source_digest,
            engine_version,
            message,
        )
        .map_err(|e| e.to_string())?;
    loom.store()
        .mark_search_tantivy_unsupported(
            workspace,
            collection,
            source_digest,
            engine_version,
            message,
        )
        .map_err(|e| e.to_string())?;
    let status = loom
        .store()
        .search_tantivy_status(workspace, collection, source_digest, engine_version)
        .map_err(|e| e.to_string())?;
    print_search_status(
        format,
        &workspace.to_string(),
        collection,
        source_digest,
        engine_version,
        &status,
    )
}

fn search_status_json(
    workspace: &str,
    collection: &str,
    source_digest: Digest,
    engine_version: &str,
    status: &DerivedArtifactStatus,
) -> String {
    let mut out = String::from("{\"workspace\":");
    out.push_str(&json_string(workspace));
    out.push_str(",\"collection\":");
    out.push_str(&json_string(collection));
    out.push_str(",\"source_digest\":");
    out.push_str(&json_string(&source_digest.to_string()));
    out.push_str(",\"engine_version\":");
    out.push_str(&json_string(engine_version));
    out.push_str(",\"status\":");
    out.push_str(&json_string(status.name()));
    match status {
        DerivedArtifactStatus::Ready { record } | DerivedArtifactStatus::Stale { record } => {
            push_search_status_record(&mut out, record);
        }
        DerivedArtifactStatus::Rebuilding { run_id, stamp } => {
            out.push_str(",\"run_id\":");
            out.push_str(&json_string(run_id));
            push_search_status_stamp(&mut out, stamp);
        }
        DerivedArtifactStatus::Failed { stamp, message }
        | DerivedArtifactStatus::Unsupported { stamp, message } => {
            out.push_str(",\"message\":");
            out.push_str(&json_string(message));
            push_search_status_stamp(&mut out, stamp);
        }
        DerivedArtifactStatus::Missing => {}
    }
    out.push('}');
    out
}

fn push_search_status_record(out: &mut String, record: &DerivedArtifactRecord) {
    out.push_str(",\"payload_digest\":");
    out.push_str(&json_string(&record.payload_digest.to_string()));
    out.push_str(",\"payload_len\":");
    out.push_str(&record.payload_len.to_string());
    push_search_status_stamp(out, &record.stamp);
}

fn push_search_status_stamp(out: &mut String, stamp: &loom_store::DerivedArtifactStamp) {
    out.push_str(",\"stamp\":{\"source_digest\":");
    out.push_str(&json_string(&stamp.source_digest.to_string()));
    out.push_str(",\"engine_version\":");
    out.push_str(&json_string(&stamp.engine_version));
    out.push_str(",\"format_version\":");
    out.push_str(&json_string(&stamp.format_version));
    out.push('}');
}

fn run_capabilities(format: &str, all: bool) -> Result<(), String> {
    let set = loom_core::capability::registry();
    let visibility = if all {
        loom_core::CapabilityVisibility::Detailed
    } else {
        loom_core::CapabilityVisibility::Default
    };
    match format {
        "text" => {
            let rows = set.iter_visible(visibility).collect::<Vec<_>>();
            print_capabilities_text(&rows);
            Ok(())
        }
        "json" => {
            println!("{}", set.to_json(visibility));
            Ok(())
        }
        other => Err(format!("unknown capability output format {other:?}")),
    }
}

fn print_capabilities_text(rows: &[&loom_core::CapabilityInfo]) {
    println!(
        "{:<32}  {:<11}  {:<13}  {:<18}  reason",
        "capability", "state", "proof", "dimension"
    );
    for capability in rows {
        println!(
            "{:<32}  {:<11}  {:<13}  {:<18}  {}",
            capability.name,
            capability.operational_state.as_str(),
            capability.proof.as_str(),
            capability_dimension_label(capability.dimensions),
            capability.reason_code.unwrap_or("")
        );
    }
}

fn capability_dimension_label(dimensions: loom_core::CapabilityDimensions) -> String {
    if let Some(value) = dimensions.facet {
        format!("facet:{value}")
    } else if let Some(value) = dimensions.facade {
        format!("facade:{value}")
    } else if let Some(value) = dimensions.engine {
        format!("engine:{value}")
    } else if let Some(value) = dimensions.transport {
        format!("transport:{value}")
    } else if let Some(value) = dimensions.compile_feature {
        format!("compile_feature:{value}")
    } else if let Some(value) = dimensions.listener {
        format!("listener:{value}")
    } else if let Some(value) = dimensions.binding {
        format!("binding:{value}")
    } else if let Some(value) = dimensions.policy {
        format!("policy:{value}")
    } else {
        "build".to_string()
    }
}

fn run(command: Command, keys: &KeyOpts) -> Result<(), String> {
    match command {
        Command::Audit { action } => run_audit(action, keys),
        Command::Calendar { action } => run_calendar(action, keys),
        Command::Cas { action } => run_cas(action, keys),
        Command::Capabilities { format, all } => run_capabilities(&format, all),
        Command::Certificate { action } => run_certificate(action, keys),
        Command::Chat { action } => run_chat(action, keys),
        Command::Columnar { action } => run_columnar(action, keys),
        Command::Contacts { action } => run_contacts(action, keys),
        Command::Context { action } => run_context(action),
        Command::Dataframe { action } => run_dataframe(action, keys),
        Command::Daemon { action } => run_daemon(action, keys),
        Command::Document { action } => run_document(action, keys),
        Command::Drive { action } => run_drive(action, keys),
        Command::Doctor { action } => run_doctor(action, keys),
        Command::Exec { action } => run_exec_cmd(action, keys),
        Command::Program { action } => run_program(action, keys),
        Command::Files { action } => run_files(action, keys),
        Command::Graph { action } => run_graph(action, keys),
        Command::Kv { action } => run_kv(action, keys),
        Command::Ledger { action } => run_ledger(action, keys),
        Command::Metrics { action } => run_metrics(action, keys),
        Command::Logs { action } => run_logs(action, keys),
        Command::Traces { action } => run_traces(action, keys),
        Command::Lifecycle { action } => run_lifecycle(action, keys),
        Command::Lock { action } => run_lock(action, keys),
        Command::Mail { action } => run_mail(action, keys),
        Command::Meetings { action } => run_meetings(action, keys),
        Command::Pages { action } => run_pages(action, keys),
        Command::Tickets { action } => run_tickets(action, keys),
        Command::Lanes { action } => run_lanes(action, keys),
        Command::Management { action } => run_management(action, keys),
        Command::NetworkAccess { action } => run_network_access(action, keys),
        Command::Inference { action } => run_inference(action, keys),
        Command::Acl { action } => run_acl(action, keys),
        Command::Identity { action } => run_identity(action, keys),
        Command::Interchange { action } => run_interchange(action, keys),
        Command::Workspace { action } => run_management_workspace(action, keys),
        Command::ProtectedRef { action } => run_protected_ref(action, keys),
        #[cfg(feature = "mcp")]
        Command::Refs { action } => run_refs(action, keys),
        #[cfg(feature = "mcp")]
        Command::Mcp {
            store,
            workspace,
            collection,
            http,
            network_access,
            stateless,
        } => run_mcp(
            &store,
            workspace,
            collection,
            http,
            network_access,
            stateless,
            keys,
        ),
        #[cfg(any(feature = "fuse", feature = "nfs"))]
        Command::Mount { action } => run_mount(action, keys),
        Command::Queue { action } => run_queue(action, keys),
        Command::Search {
            store,
            query,
            workspace,
            collection,
            field,
            limit,
            offset,
            format,
        } => run_unified_search(
            UnifiedSearchArgs {
                store,
                query,
                workspace,
                collection,
                field,
                limit,
                offset,
                format,
            },
            keys,
        ),
        Command::Fts { action } => run_search(action, keys),
        Command::Serve { action } => run_serve(action, keys),
        Command::Studio { action } => run_studio(action, keys),
        Command::Sql { action } => run_sql_cmd(action, keys),
        Command::Store { action } => run_store(action, keys),
        Command::TimeSeries { action } => run_time_series(action, keys),
        Command::Vcs { action } => run_vcs(action, keys),
        Command::Vector { action } => run_vector(action, keys),
        Command::Llms => {
            print_llms_reference(false);
            Ok(())
        }
        Command::Version => {
            println!("loom {VERSION}");
            Ok(())
        }
    }
}

#[cfg(any(feature = "fuse", feature = "nfs"))]
fn run_mount(action: MountCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        #[cfg(feature = "fuse")]
        MountCmd::Fuse {
            store,
            workspace,
            mountpoint,
            read_only,
        } => mount_fuse_flow(store, workspace, mountpoint, read_only, keys),
        #[cfg(feature = "nfs")]
        MountCmd::Nfs {
            store,
            workspace,
            mountpoint,
            listen,
            read_only,
        } => {
            let mount_auth = mount_open_auth(&store, keys)?;
            if !read_only {
                ensure_mount_workspace(&store, &workspace, &mount_auth)?;
            }
            daemon_start_with_transport(&store, "native")?;
            mount_nfs_flow(
                &store,
                &workspace,
                &listen,
                &mountpoint,
                read_only,
                mount_auth,
            )
        }
    }
}

#[cfg(feature = "fuse")]
fn mount_fuse_flow(
    store: String,
    workspace: String,
    mountpoint: String,
    read_only: bool,
    keys: &KeyOpts,
) -> Result<(), String> {
    let mount_auth = mount_open_auth(&store, keys)?;
    if !read_only {
        ensure_mount_workspace(&store, &workspace, &mount_auth)?;
    }
    daemon_start_with_transport(&store, "native")?;
    let pin = format!("mount-fuse:{mountpoint}");
    let _pin_lease = MountPinLease::acquire(&store, &pin)?;
    loom_vfs_fuse::mount_with_auth(
        std::path::Path::new(&store),
        &workspace,
        std::path::Path::new(&mountpoint),
        read_only,
        mount_auth,
    )
    .map_err(|e| e.to_string())
}

fn run_store(action: StoreCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        StoreCmd::BundleExport {
            store,
            workspace,
            out,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let bundle = bundle_export(&loom, ns).map_err(|e| e.to_string())?;
            std::fs::write(&out, bundle.encode()).map_err(|e| e.to_string())?;
            println!("exported {} object(s) to {out}", bundle.objects.len());
            Ok(())
        }
        StoreCmd::BundleImport { store, input } => {
            let bytes = std::fs::read(&input).map_err(|e| e.to_string())?;
            let bundle = Bundle::decode(&bytes).map_err(|e| e.to_string())?;
            let mut loom = cli_open_loom(&store, keys)?;
            let (_, report) = bundle_import(&mut loom, &bundle).map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            let facets = bundle
                .facets
                .iter()
                .map(|facet| facet.as_str())
                .collect::<Vec<_>>()
                .join(",");
            println!(
                "imported {} [{}] ({} new, {} skipped)",
                bundle.ns_name, facets, report.objects_transferred, report.objects_skipped
            );
            Ok(())
        }
        StoreCmd::Clone {
            src,
            workspace,
            dst,
        } => {
            let source = cli_open_loom_read(&src, keys)?;
            let src_ns = resolve_ns(&source, &workspace)?;
            let mut target = cli_open_loom(&dst, keys)?;
            let id = random_workspace_id()?;
            let (_, report) =
                clone_workspace(&source, src_ns, &mut target, id).map_err(|e| e.to_string())?;
            save_loom(&mut target).map_err(|e| e.to_string())?;
            println!("cloned {} object(s) into {dst}", report.objects_transferred);
            Ok(())
        }
        StoreCmd::Copy {
            src,
            dst,
            with,
            format,
            report_file,
            dry_run,
            new_key_source,
        } => {
            let modifiers = parse_store_copy_modifiers(&with)?;
            let format = parse_store_copy_format(&format)?;
            if std::path::Path::new(&dst).exists() {
                return Err(format!("destination {dst:?} already exists"));
            }
            let source_fs = FileStore::open_read(&src).map_err(|e| e.to_string())?;
            let source_algo = source_fs.digest_algo();
            let target_algo = if modifiers.fips {
                Algo::Sha256
            } else {
                source_algo
            };
            let profile_changing = source_algo != target_algo;
            let mode = if profile_changing {
                "identity-profile migration"
            } else if modifiers.compacted {
                "file copy plus compaction"
            } else {
                "file copy"
            };
            let source = cli_open_loom_read(&src, keys)?;
            let source_encrypted = source.store().is_encrypted();
            let workspace_count = source.registry().list(None).len();
            let listener_count = source
                .store()
                .served_listeners()
                .map_err(|e| e.to_string())?
                .len();
            if dry_run {
                let mut report = StoreCopyReport::new(StoreCopyReportInput {
                    source: &src,
                    destination: &dst,
                    source_algo,
                    target_algo,
                    modifiers,
                    mode,
                    workspaces: workspace_count,
                    source_encrypted,
                    destination_encrypted: source_encrypted,
                    dry_run: true,
                });
                report
                    .warnings
                    .push("dry run; destination was not written".to_string());
                report.served_listeners_to_import_disabled = listener_count;
                emit_store_copy_report(&report, format, report_file.as_deref())?;
                return Ok(());
            }
            if !profile_changing {
                std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
                let mut report = StoreCopyReport::new(StoreCopyReportInput {
                    source: &src,
                    destination: &dst,
                    source_algo,
                    target_algo,
                    modifiers,
                    mode,
                    workspaces: workspace_count,
                    source_encrypted,
                    destination_encrypted: source_encrypted,
                    dry_run: false,
                });
                if modifiers.compacted {
                    let mut copied = cli_open_loom(&dst, keys)?;
                    let stats = gc_loom(&mut copied).map_err(|e| e.to_string())?;
                    report.compaction_before_bytes = Some(stats.before);
                    report.compaction_after_bytes = Some(stats.after);
                }
                emit_store_copy_report(&report, format, report_file.as_deref())?;
                return Ok(());
            }
            ensure_store_copy_clean(&source)?;
            let target_fs = if source_encrypted {
                let suite = if target_algo == Algo::Sha256 {
                    Suite::Aes256Gcm
                } else {
                    Suite::XChaCha20Poly1305
                };
                let new_source = resolve_new_key_source(new_key_source.as_deref(), keys)?;
                let spec = acquire_key_spec(&new_source, "New target passphrase", true)?;
                let salt = rand_bytes(16)?;
                let mut dek = [0u8; loom_core::keys::KEY_LEN];
                getrandom::fill(&mut dek).map_err(|e| format!("rng: {e}"))?;
                let wrap_nonce = rand_bytes(24)?;
                let (meta, session) = EncryptionMeta::create(&spec, suite, salt, dek, wrap_nonce)
                    .map_err(|e| e.to_string())?;
                FileStore::create_encrypted_with_profile(&dst, meta.encode(), session, target_algo)
                    .map_err(|e| e.to_string())?
            } else {
                FileStore::create_with_profile(&dst, target_algo).map_err(|e| e.to_string())?
            };
            copy_control_metadata(source.store(), &target_fs)?;
            let mut target = attach_control_state(Loom::new(target_fs), keys)?;
            let mut objects_written = 0;
            let mut content_written = 0;
            let mut prolly_nodes_written = 0;
            for info in source.registry().list(None) {
                let (_, report) = migrate_workspace_profile(&source, info.id, &mut target)
                    .map_err(|e| e.to_string())?;
                objects_written += report.objects_written;
                content_written += report.content_written;
                prolly_nodes_written += report.prolly_nodes_written;
            }
            save_loom(&mut target).map_err(|e| e.to_string())?;
            let mut report = StoreCopyReport::new(StoreCopyReportInput {
                source: &src,
                destination: &dst,
                source_algo,
                target_algo,
                modifiers,
                mode,
                workspaces: workspace_count,
                source_encrypted,
                destination_encrypted: source_encrypted,
                dry_run: false,
            });
            report.objects_written = objects_written;
            report.content_written = content_written;
            report.prolly_nodes_written = prolly_nodes_written;
            report.audit_policy_imported = true;
            report.served_listeners_imported_disabled = listener_count;
            if modifiers.compacted {
                let stats = gc_loom(&mut target).map_err(|e| e.to_string())?;
                report.compaction_before_bytes = Some(stats.before);
                report.compaction_after_bytes = Some(stats.after);
            }
            emit_store_copy_report(&report, format, report_file.as_deref())?;
            Ok(())
        }
        StoreCmd::Get { store, digest, out } => {
            if remote::target_is_remote(&store)? {
                return Err("`store get` (raw global blob read) is not available over a remote store: it bypasses workspace/facet authorization. Use workspace-scoped `cas get`, or `loom export`/Transfer, for remote data movement.".to_string());
            }
            let addr = Digest::parse(&digest).map_err(|e| e.to_string())?;
            let fs = FileStore::open_read(&store).map_err(|e| e.to_string())?;
            unlock_if_encrypted(&fs, keys)?;
            let canonical = fs
                .get(&addr)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("object {digest} not found"))?;
            let payload = match Object::decode(&canonical).map_err(|e| e.to_string())? {
                Object::Blob(bytes) => bytes,
                other => {
                    return Err(format!(
                        "object {digest} is a {:?}, not a Blob",
                        other.object_type()
                    ));
                }
            };
            write_output(out.as_deref(), &payload).map_err(|e| e.to_string())
        }
        StoreCmd::Hash { path } => {
            let bytes = read_input(&path).map_err(|e| e.to_string())?;
            println!("{}", Object::Blob(bytes).digest());
            Ok(())
        }
        StoreCmd::Init {
            store,
            encrypt,
            suite,
            identity_profile,
            fips,
        } => {
            if remote::target_is_remote(&store)? {
                return Err("`store init` creates a local store on disk and cannot target a remote endpoint. Provision the store where it will be served, then `loom serve remote` it.".to_string());
            }
            if fips
                && let Some(profile) = identity_profile.as_deref()
                && !matches!(profile, "fips" | "sha256")
            {
                return Err("--fips requires `--identity-profile fips`".to_string());
            }
            let default_algo = default_init_digest_algo();
            let profile = if fips {
                "fips"
            } else {
                identity_profile
                    .as_deref()
                    .unwrap_or_else(|| default_algo.as_str())
            };
            let digest_algo = match profile {
                "default" | "blake3" => Algo::Blake3,
                "fips" | "sha256" => Algo::Sha256,
                other => {
                    return Err(format!(
                        "unknown identity profile {other:?} (expected `default` or `fips`)"
                    ));
                }
            };
            if fips && digest_algo != Algo::Sha256 {
                return Err("--fips requires `--identity-profile fips`".to_string());
            }
            if cfg!(feature = "fips") && digest_algo != Algo::Sha256 {
                return Err(
                    "FIPS strict mode requires `--identity-profile fips` for new stores"
                        .to_string(),
                );
            }
            if encrypt || suite.is_some() {
                // The FIPS profile pairs AES-256-GCM by default; the default profile pairs XChaCha.
                let suite = match suite.as_deref() {
                    Some(s) => Suite::parse(s).map_err(|e| e.to_string())?,
                    None if digest_algo == Algo::Sha256 => Suite::Aes256Gcm,
                    None => Suite::XChaCha20Poly1305,
                };
                // Acquire the credential from the configured key source: a passphrase
                // (confirmed twice - a typo would permanently lock this immutable-at-creation store) or a
                // raw KEK. `create` records the matching wrap source.
                let spec = acquire_key_spec(&keys.source, "New passphrase", true)?;
                // The key layer takes randomness as input; the CLI supplies it (salt, DEK, wrap nonce).
                let salt = rand_bytes(16)?;
                let mut dek = [0u8; loom_core::keys::KEY_LEN];
                getrandom::fill(&mut dek).map_err(|e| format!("rng: {e}"))?;
                let wrap_nonce = rand_bytes(24)?;
                let (meta, session) = EncryptionMeta::create(&spec, suite, salt, dek, wrap_nonce)
                    .map_err(|e| e.to_string())?;
                let fs = FileStore::create_encrypted_with_profile(
                    &store,
                    meta.encode(),
                    session,
                    digest_algo,
                )
                .map_err(|e| e.to_string())?;
                init_control_state(&fs)?;
                println!(
                    "initialized encrypted {store} (identity {}, suite {})",
                    digest_algo.as_str(),
                    suite.as_str()
                );
                return Ok(());
            }
            let fs =
                FileStore::create_with_profile(&store, digest_algo).map_err(|e| e.to_string())?;
            init_control_state(&fs)?;
            println!("initialized {store} (identity {})", digest_algo.as_str());
            Ok(())
        }
        StoreCmd::Key { action } => match action {
            KeyCmd::AddWrap {
                store,
                allow_no_recovery,
                new_key_source,
            } => {
                let client = remote::open_store_client(&store)?;
                if client.is_remote() {
                    let new_source = resolve_new_key_source(new_key_source.as_deref(), keys)?;
                    let new_passphrase = acquire(&new_source, "New passphrase", true)?.into_bytes();
                    client.admin_key_add_wrap(new_passphrase, allow_no_recovery)?;
                    println!("added unlock wrap to remote store {store}");
                    return Ok(());
                }
                let fs = FileStore::open(&store).map_err(|e| e.to_string())?;
                fs.unlock(&acquire_key_spec(
                    &keys.source,
                    "Current passphrase",
                    false,
                )?)
                .map_err(|e| e.to_string())?;
                let new_source = resolve_new_key_source(new_key_source.as_deref(), keys)?;
                let new_spec = acquire_key_spec(&new_source, "New passphrase", true)?;
                fs.add_wrap(
                    &new_spec,
                    rand_bytes(16)?,
                    rand_bytes(24)?,
                    allow_no_recovery,
                )
                .map_err(|e| e.to_string())?;
                println!("added unlock wrap to {store}");
                Ok(())
            }
            KeyCmd::RemoveWrap {
                store,
                index,
                allow_no_recovery,
            } => {
                let client = remote::open_store_client(&store)?;
                if client.is_remote() {
                    client.admin_key_remove_wrap(index as u64, allow_no_recovery)?;
                    println!("removed unlock wrap {index} from remote store {store}");
                    return Ok(());
                }
                let fs = FileStore::open(&store).map_err(|e| e.to_string())?;
                fs.unlock(&acquire_key_spec(
                    &keys.source,
                    "Current passphrase",
                    false,
                )?)
                .map_err(|e| e.to_string())?;
                fs.remove_wrap(index, allow_no_recovery)
                    .map_err(|e| e.to_string())?;
                println!("removed unlock wrap {index} from {store}");
                Ok(())
            }
        },
        StoreCmd::Policy {
            store,
            fips_required,
        } => {
            let client = remote::open_store_client(&store)?;
            if client.is_remote() {
                let json = match fips_required {
                    Some(f) => client.admin_policy_set_json(f)?,
                    None => client.admin_policy_get_json()?,
                };
                println!("{json}");
                return Ok(());
            }
            if let Some(fips_required) = fips_required {
                let fs = cli_open_store_for_write(&store)?;
                unlock_if_encrypted(&fs, keys)?;
                let policy = StorePolicy { fips_required };
                let target = format!("fips_required={fips_required}");
                let seq = fs
                    .save_store_policy_audited(policy, None, "store.policy.set", Some(&target))
                    .map_err(|e| e.to_string())?;
                println!("{}", store_policy_json(policy, Some(seq)));
                return Ok(());
            }
            let fs = FileStore::open_read(&store).map_err(|e| e.to_string())?;
            unlock_if_encrypted(&fs, keys)?;
            println!(
                "{}",
                store_policy_json(fs.store_policy().map_err(|e| e.to_string())?, None)
            );
            Ok(())
        }
        StoreCmd::Put { store, path } => {
            if remote::target_is_remote(&store)? {
                return Err("`store put` (raw global blob write) is not available over a remote store: it bypasses workspace/facet authorization. Use workspace-scoped `cas put`, or `loom import`/Transfer, for remote data movement.".to_string());
            }
            let bytes = read_input(&path).map_err(|e| e.to_string())?;
            let fs = cli_open_store_for_write(&store)?;
            unlock_if_encrypted(&fs, keys)?;
            let digest = fs
                .put(&Object::Blob(bytes).canonical())
                .map_err(|e| e.to_string())?;
            println!("{digest}");
            Ok(())
        }
        StoreCmd::Rekey {
            store,
            suite,
            reseal,
            new_key_source,
        } => {
            let client = remote::open_store_client(&store)?;
            if client.is_remote() {
                // Remote rekey is server-side and passphrase-based: the client sends only the new
                // passphrase; the server mints the salt/nonce/DEK and never returns key material.
                let new_source = resolve_new_key_source(new_key_source.as_deref(), keys)?;
                let new_passphrase = acquire(&new_source, "New passphrase", true)?.into_bytes();
                println!(
                    "{}",
                    client.admin_rekey_summary(new_passphrase, reseal, suite)?
                );
                return Ok(());
            }
            let mut fs = FileStore::open(&store).map_err(|e| e.to_string())?;
            let meta = fs
                .encryption_meta()
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("{store} is not encrypted"))?;
            // Current credential from --key-source; the new one from --new-key-source (confirmed). Each
            // may be a passphrase or a raw KEK.
            fs.unlock(&acquire_key_spec(
                &keys.source,
                "Current passphrase",
                false,
            )?)
            .map_err(|e| e.to_string())?;
            let new_source = resolve_new_key_source(new_key_source.as_deref(), keys)?;
            let new_spec = acquire_key_spec(&new_source, "New passphrase", true)?;
            let salt = rand_bytes(16)?;
            // 24 random bytes cover either wrap nonce length (XChaCha 24 / AES-GCM 12); `create`/`rewrap`
            // use the leading bytes for the profile's wrap AEAD.
            let wrap_nonce = rand_bytes(24)?;
            if reseal {
                // Full data pass: a fresh DEK, optionally a new suite, re-sealing every object.
                let target_suite = match &suite {
                    Some(s) => Suite::parse(s).map_err(|e| e.to_string())?,
                    None => meta.active_suite,
                };
                let new_dek: [u8; 32] = rand_bytes(32)?
                    .try_into()
                    .map_err(|_| "DEK must be 32 bytes".to_string())?;
                let (new_meta, new_session) =
                    EncryptionMeta::create(&new_spec, target_suite, salt, new_dek, wrap_nonce)
                        .map_err(|e| e.to_string())?;
                let stats = fs
                    .rekey_reseal(new_meta.encode(), new_session)
                    .map_err(|e| e.to_string())?;
                println!(
                    "rekeyed {store} (re-sealed every object under a fresh DEK, suite {}; {} -> {} bytes)",
                    target_suite.as_str(),
                    stats.before,
                    stats.after
                );
            } else {
                // Cheap path: DEK re-wrap only; changing the AEAD suite needs --reseal.
                if let Some(s) = &suite {
                    let want = Suite::parse(s).map_err(|e| e.to_string())?;
                    if want != meta.active_suite {
                        return Err(format!(
                            "changing the AEAD suite ({} -> {}) requires re-sealing every object; \
                             re-run with --reseal",
                            meta.active_suite.as_str(),
                            want.as_str()
                        ));
                    }
                }
                fs.rekey(&new_spec, salt, wrap_nonce)
                    .map_err(|e| e.to_string())?;
                println!("rekeyed {store} (DEK re-wrapped under the new credential)");
            }
            Ok(())
        }
        StoreCmd::Stat { store } => {
            let client = remote::open_store_client(&store)?;
            if client.is_remote() {
                println!("{}", client.admin_stat_json()?);
                return Ok(());
            }
            let fs = FileStore::open_read(&store).map_err(|e| e.to_string())?;
            println!("{}: {} object(s)", store, fs.len());
            let status = fs.maintenance_status().map_err(|e| e.to_string())?;
            println!(
                "maintenance: generation={} object_count={} physical_pages={} physical_bytes={} reusable_free_pages={} candidate_dead_pages={} tail_free_pages={} tail_free_bytes={} last_validated_mark_epoch={} touched_segments={} candidate_segments={} segment_overflow={}",
                status.generation,
                status.object_count,
                status.physical_page_count,
                status.physical_bytes,
                status.reusable_free_pages,
                status.candidate_dead_pages,
                status.tail_free_pages,
                status.tail_free_bytes,
                status.last_validated_mark_epoch,
                status.touched_segments.len(),
                status.candidate_segments.len(),
                status.segment_overflow
            );
            Ok(())
        }
        StoreCmd::PreflightReplacement {
            store,
            workspace,
            format,
        } => run_store_replacement_preflight(&store, &workspace, &format, keys),
    }
}

fn run_store_replacement_preflight(
    store: &str,
    workspace: &str,
    format: &str,
    keys: &KeyOpts,
) -> Result<(), String> {
    let mut checks = Vec::new();
    let mut store_opened = None;
    match FileStore::open_read(store) {
        Ok(fs) => {
            let status = fs.maintenance_status().map_err(|e| e.to_string());
            match status {
                Ok(status) => {
                    checks.push(store_preflight_check(
                        "store_stat",
                        true,
                        format!(
                            "objects={} maintenance_generation={} maintenance_object_count={} physical_bytes={}",
                            fs.len(),
                            status.generation,
                            status.object_count,
                            status.physical_bytes
                        ),
                    ));
                    store_opened = Some(fs);
                }
                Err(error) => checks.push(store_preflight_check("store_stat", false, error)),
            }
        }
        Err(error) => checks.push(store_preflight_check(
            "store_open",
            false,
            format!("candidate store is not readable by this binary: {error}"),
        )),
    }

    let mut opened_loom = None;
    match cli_open_loom_read(store, keys) {
        Ok(loom) => {
            let workspace_count = loom.registry().list(None).len();
            checks.push(store_preflight_check(
                "workspace_list",
                true,
                format!("workspaces={workspace_count}"),
            ));
            opened_loom = Some(loom);
        }
        Err(error) => checks.push(store_preflight_check(
            "workspace_list",
            false,
            format!("workspace registry is not readable by this binary: {error}"),
        )),
    }

    if let Some(loom) = opened_loom.as_ref() {
        match resolve_ns(loom, workspace) {
            Ok(workspace_id) => {
                checks.push(store_preflight_check(
                    "workspace_resolve",
                    true,
                    format!("workspace_id={workspace_id}"),
                ));
                match loom_lanes::list_lanes_with_diagnostics(loom, workspace_id) {
                    Ok((lanes, diagnostics)) if diagnostics.is_empty() => {
                        checks.push(store_preflight_check(
                            "lanes_list",
                            true,
                            format!("lanes={}", lanes.len()),
                        ));
                    }
                    Ok((lanes, diagnostics)) => {
                        checks.push(store_preflight_check(
                            "lanes_list",
                            false,
                            format!(
                                "lanes={} decode_diagnostics={}",
                                lanes.len(),
                                diagnostics.len()
                            ),
                        ));
                    }
                    Err(error) => {
                        checks.push(store_preflight_check(
                            "lanes_list",
                            false,
                            error.to_string(),
                        ));
                    }
                }
                let query = loom_tickets::TicketListQuery {
                    projection: None,
                    statuses: Vec::new(),
                    assignees: Vec::new(),
                    priorities: Vec::new(),
                    ticket_types: Vec::new(),
                    labels: Vec::new(),
                    policy_labels: Vec::new(),
                    ready_only: false,
                    include_completed: true,
                    lane_member_ids: None,
                    board_id: None,
                    cursor: None,
                    limit: Some(1),
                };
                let profile_id = workspace_id.to_string();
                match loom_tickets::list_tickets_page(loom, workspace_id, &profile_id, &query) {
                    Ok(page) => checks.push(store_preflight_check(
                        "tickets_list",
                        true,
                        format!("total={} sampled={}", page.total, page.items.len()),
                    )),
                    Err(error) => {
                        checks.push(store_preflight_check(
                            "tickets_list",
                            false,
                            error.to_string(),
                        ));
                    }
                }
            }
            Err(error) => checks.push(store_preflight_check(
                "workspace_resolve",
                false,
                error.to_string(),
            )),
        }
    }

    if let Some(fs) = store_opened.as_ref() {
        match fs.store_maintenance_report(now_ms()) {
            Ok(report) => checks.push(store_preflight_check(
                "doctor_store",
                true,
                format!(
                    "maintenance_state=ok candidate_reclaimable_bytes={} reusable_free_bytes={}",
                    report.candidate_reclaimable_bytes, report.reusable_free_bytes
                ),
            )),
            Err(error) => checks.push(store_preflight_check(
                "doctor_store",
                false,
                format!("maintenance report is not readable by this binary: {error}"),
            )),
        }
    }

    let ok = checks
        .iter()
        .all(|check| check["ok"].as_bool() == Some(true));
    match format {
        "text" => print_store_replacement_preflight_text(store, workspace, ok, &checks),
        "json" => {
            let body = serde_json::json!({
                "store": store,
                "workspace": workspace,
                "ok": ok,
                "checks": checks,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?
            );
        }
        other => {
            return Err(format!(
                "unknown preflight output format {other:?} (expected text or json)"
            ));
        }
    }
    if ok {
        Ok(())
    } else {
        Err("store replacement preflight failed; do not replace the active store".to_string())
    }
}

fn store_preflight_check(name: &str, ok: bool, message: impl Into<String>) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "ok": ok,
        "message": message.into(),
    })
}

fn print_store_replacement_preflight_text(
    store: &str,
    workspace: &str,
    ok: bool,
    checks: &[serde_json::Value],
) {
    println!("store replacement preflight");
    println!("store\t{store}");
    println!("workspace\t{workspace}");
    println!("status\t{}", if ok { "ok" } else { "blocked" });
    for check in checks {
        println!(
            "{}\t{}\t{}",
            check["name"].as_str().unwrap_or("unknown"),
            if check["ok"].as_bool() == Some(true) {
                "ok"
            } else {
                "blocked"
            },
            check["message"].as_str().unwrap_or("")
        );
    }
}

fn run_chat(action: ChatCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        ChatCmd::Channels {
            store,
            workspace,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let channels = loom_chat::list_channels(&loom, workspace_id, &profile_id)
                .map_err(|e| e.to_string())?;
            print_chat_channels(&channels, &format)
        }
        ChatCmd::CreateChannel {
            store,
            workspace,
            handle,
            name,
            channel_id,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let channel_id = match channel_id {
                Some(value) => parse_chat_workspace_id(&value)?,
                None => random_workspace_id()?,
            };
            let channel = loom_chat::ensure_channel(
                &mut loom,
                workspace_id,
                &profile_id,
                channel_id,
                &handle,
                &name,
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_chat_channel_summary(&channel, &format)
        }
        ChatCmd::RenameChannel {
            store,
            workspace,
            channel,
            handle,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let channel =
                loom_chat::rename_channel(&mut loom, workspace_id, &profile_id, &channel, &handle)
                    .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_chat_channel_summary(&channel, &format)
        }
        ChatCmd::Messages {
            store,
            workspace,
            channel,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let projection =
                loom_chat::channel_projection(&loom, workspace_id, &profile_id, &channel)
                    .map_err(|e| e.to_string())?;
            print_chat_channel(&projection, &format)
        }
        ChatCmd::Events {
            store,
            workspace,
            channel,
            from_sequence,
            max,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let events = loom_chat::operation_changes(
                &loom,
                workspace_id,
                &profile_id,
                &channel,
                from_sequence,
                max,
            )
            .map_err(|e| e.to_string())?;
            print_chat_events(&events, &format)
        }
        ChatCmd::Cursor {
            store,
            workspace,
            channel,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let cursor = loom_chat::read_cursor(&loom, workspace_id, &profile_id, &channel)
                .map_err(|e| e.to_string())?;
            print_chat_cursor(&cursor, &format)
        }
        ChatCmd::UpdateCursor {
            store,
            workspace,
            channel,
            next_sequence,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let cursor = loom_chat::update_cursor(
                &mut loom,
                workspace_id,
                &profile_id,
                &channel,
                next_sequence,
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_chat_cursor(&cursor, &format)
        }
        ChatCmd::Post {
            store,
            workspace,
            channel,
            message_id,
            thread,
            input,
            format,
        } => {
            let body = read_input(&input).map_err(|e| e.to_string())?;
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let write = loom_chat::post_message(
                &mut loom,
                workspace_id,
                &profile_id,
                &channel,
                &message_id,
                thread.as_deref(),
                body,
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_chat_write(&write, &format)
        }
        ChatCmd::Edit {
            store,
            workspace,
            channel,
            message_id,
            input,
            format,
        } => {
            let body = read_input(&input).map_err(|e| e.to_string())?;
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let write = loom_chat::edit_message(
                &mut loom,
                workspace_id,
                &profile_id,
                &channel,
                &message_id,
                body,
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_chat_write(&write, &format)
        }
        ChatCmd::Redact {
            store,
            workspace,
            channel,
            message_id,
            reason,
            format,
        } => chat_write(
            keys,
            &store,
            &workspace,
            &format,
            |loom, workspace_id, profile_id| {
                loom_chat::redact_message(
                    loom,
                    workspace_id,
                    profile_id,
                    &channel,
                    &message_id,
                    reason.as_deref(),
                )
            },
        ),
        ChatCmd::CreateThread {
            store,
            workspace,
            channel,
            thread_id,
            parent_message_id,
            format,
        } => chat_write(
            keys,
            &store,
            &workspace,
            &format,
            |loom, workspace_id, profile_id| {
                loom_chat::create_thread(
                    loom,
                    workspace_id,
                    profile_id,
                    &channel,
                    &thread_id,
                    &parent_message_id,
                )
            },
        ),
        ChatCmd::CreateTask {
            store,
            workspace,
            channel,
            task_id,
            title,
            message_id,
            format,
        } => chat_write(
            keys,
            &store,
            &workspace,
            &format,
            |loom, workspace_id, profile_id| {
                loom_chat::create_task(
                    loom,
                    workspace_id,
                    profile_id,
                    &channel,
                    &task_id,
                    message_id.as_deref(),
                    &title,
                )
            },
        ),
        ChatCmd::ClaimTask {
            store,
            workspace,
            channel,
            task_id,
            claim_id,
            lease_token,
            format,
        } => chat_write(
            keys,
            &store,
            &workspace,
            &format,
            |loom, workspace_id, profile_id| {
                loom_chat::claim_task(
                    loom,
                    workspace_id,
                    profile_id,
                    &channel,
                    &task_id,
                    &claim_id,
                    lease_token.as_deref(),
                )
            },
        ),
        ChatCmd::CompleteTask {
            store,
            workspace,
            channel,
            task_id,
            claim_id,
            result_message_id,
            format,
        } => chat_write(
            keys,
            &store,
            &workspace,
            &format,
            |loom, workspace_id, profile_id| {
                loom_chat::complete_task(
                    loom,
                    workspace_id,
                    profile_id,
                    &channel,
                    &task_id,
                    &claim_id,
                    result_message_id.as_deref(),
                )
            },
        ),
        ChatCmd::InvokeAgent {
            store,
            workspace,
            channel,
            invocation_id,
            agent_principal,
            source_message_ids,
            input,
            format,
        } => {
            let prompt = read_input(&input).map_err(|e| e.to_string())?;
            let agent_principal = parse_chat_workspace_id(&agent_principal)?;
            chat_write(
                keys,
                &store,
                &workspace,
                &format,
                |loom, workspace_id, profile_id| {
                    loom_chat::invoke_agent(
                        loom,
                        workspace_id,
                        profile_id,
                        &channel,
                        &invocation_id,
                        agent_principal,
                        source_message_ids,
                        prompt,
                    )
                },
            )
        }
        ChatCmd::AgentReply {
            store,
            workspace,
            channel,
            invocation_id,
            message_id,
            format,
        } => chat_write(
            keys,
            &store,
            &workspace,
            &format,
            |loom, workspace_id, profile_id| {
                loom_chat::agent_reply(
                    loom,
                    workspace_id,
                    profile_id,
                    &channel,
                    &invocation_id,
                    &message_id,
                )
            },
        ),
        ChatCmd::RequestHandoff {
            store,
            workspace,
            channel,
            handoff_id,
            from_agent_principal,
            to_principal,
            reason,
            format,
        } => {
            let from_agent_principal = parse_chat_workspace_id(&from_agent_principal)?;
            let to_principal = to_principal
                .as_deref()
                .map(parse_chat_workspace_id)
                .transpose()?;
            chat_write(
                keys,
                &store,
                &workspace,
                &format,
                |loom, workspace_id, profile_id| {
                    loom_chat::request_handoff(
                        loom,
                        workspace_id,
                        profile_id,
                        &channel,
                        &handoff_id,
                        from_agent_principal,
                        to_principal,
                        reason.as_deref(),
                    )
                },
            )
        }
        ChatCmd::AddReaction {
            store,
            workspace,
            channel,
            message_id,
            kind,
            format,
        } => chat_write(
            keys,
            &store,
            &workspace,
            &format,
            |loom, workspace_id, profile_id| {
                loom_chat::add_reaction(
                    loom,
                    workspace_id,
                    profile_id,
                    &channel,
                    &message_id,
                    &kind,
                )
            },
        ),
        ChatCmd::RemoveReaction {
            store,
            workspace,
            channel,
            message_id,
            kind,
            format,
        } => chat_write(
            keys,
            &store,
            &workspace,
            &format,
            |loom, workspace_id, profile_id| {
                loom_chat::remove_reaction(
                    loom,
                    workspace_id,
                    profile_id,
                    &channel,
                    &message_id,
                    &kind,
                )
            },
        ),
        ChatCmd::EmojiList {
            store,
            workspace,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let registry = loom_chat::emoji_registry(&loom, workspace_id, &profile_id)
                .map_err(|e| e.to_string())?;
            print_chat_emoji_registry(&registry, &format)
        }
        ChatCmd::EmojiRegister {
            store,
            workspace,
            kind,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let registry = loom_chat::register_emoji(&mut loom, workspace_id, &profile_id, &kind)
                .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_chat_emoji_registry(&registry, &format)
        }
        ChatCmd::EmojiUnregister {
            store,
            workspace,
            kind,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let workspace_id = resolve_ns(&loom, &workspace)?;
            let profile_id = workspace_id.to_string();
            let registry = loom_chat::unregister_emoji(&mut loom, workspace_id, &profile_id, &kind)
                .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_chat_emoji_registry(&registry, &format)
        }
    }
}

fn chat_write<F>(
    keys: &KeyOpts,
    store: &str,
    workspace: &str,
    format: &str,
    f: F,
) -> Result<(), String>
where
    F: FnOnce(
        &mut Loom<FileStore>,
        WorkspaceId,
        &str,
    ) -> loom_core::Result<loom_chat::HostedChatWrite>,
{
    let mut loom = cli_open_loom(store, keys)?;
    let workspace_id = resolve_ns(&loom, workspace)?;
    let profile_id = workspace_id.to_string();
    let write = f(&mut loom, workspace_id, &profile_id).map_err(|e| e.to_string())?;
    save_loom(&mut loom).map_err(|e| e.to_string())?;
    print_chat_write(&write, format)
}

fn parse_chat_workspace_id(value: &str) -> Result<WorkspaceId, String> {
    WorkspaceId::parse(value).map_err(|e| e.to_string())
}

fn print_chat_channels(
    channels: &[loom_chat::HostedChatChannelSummary],
    format: &str,
) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(channels).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            for channel in channels {
                println!(
                    "{}\t{}\t{}",
                    channel.channel_id, channel.handle, channel.name
                );
            }
            Ok(())
        }
        other => Err(format!(
            "unsupported chat output format {other:?}; supported formats: text, json"
        )),
    }
}

fn print_chat_channel_summary(
    channel: &loom_chat::HostedChatChannelSummary,
    format: &str,
) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(channel).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            println!(
                "{}\t{}\t{}",
                channel.channel_id, channel.handle, channel.name
            );
            Ok(())
        }
        other => Err(format!(
            "unsupported chat output format {other:?}; supported formats: text, json"
        )),
    }
}

fn print_chat_channel(channel: &loom_chat::HostedChatChannel, format: &str) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(channel).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            for message in &channel.messages {
                println!(
                    "{}\t{}\t{}",
                    message.message_id,
                    message.thread_id.as_deref().unwrap_or(""),
                    String::from_utf8_lossy(&message.body)
                );
            }
            Ok(())
        }
        other => Err(format!(
            "unsupported chat output format {other:?}; supported formats: text, json"
        )),
    }
}

fn print_chat_events(
    batch: &loom_substrate::changes::OperationChangeBatch,
    format: &str,
) -> Result<(), String> {
    match format {
        "json" => {
            let events = batch
                .events
                .iter()
                .map(|event| {
                    serde_json::json!({
                        "workspace_id": event.workspace_id,
                        "app_id": event.app_id,
                        "scope_id": event.scope_id,
                        "operation_id": event.operation_id,
                        "operation_kind": event.operation_kind,
                        "sequence": event.sequence,
                        "actor_principal": event.actor_principal,
                        "timestamp_ms": event.timestamp_ms,
                        "root_after": event.root_after.to_string(),
                        "payload_digest": event.payload_digest.to_string(),
                        "policy_labels": event.policy_labels
                    })
                })
                .collect::<Vec<_>>();
            let body = serde_json::json!({
                "events": events,
                "next": batch.next.encode()
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            for event in &batch.events {
                println!(
                    "{}\t{}\t{}\t{}",
                    event.sequence, event.operation_id, event.operation_kind, event.root_after
                );
            }
            println!("next\t{}", batch.next.encode());
            Ok(())
        }
        other => Err(format!(
            "unsupported chat output format {other:?}; supported formats: text, json"
        )),
    }
}

fn print_chat_cursor(cursor: &loom_chat::HostedChatCursor, format: &str) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(cursor).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            println!(
                "{}\t{}\t{}\t{}",
                cursor.principal, cursor.next_sequence, cursor.head_sequence, cursor.unread_count
            );
            Ok(())
        }
        other => Err(format!(
            "unsupported chat output format {other:?}; supported formats: text, json"
        )),
    }
}

fn print_chat_emoji_registry(
    registry: &loom_chat::HostedChatEmojiRegistry,
    format: &str,
) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(registry).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            for kind in &registry.custom {
                println!("{kind}");
            }
            Ok(())
        }
        other => Err(format!(
            "unsupported chat output format {other:?}; supported formats: text, json"
        )),
    }
}

fn print_chat_write(write: &loom_chat::HostedChatWrite, format: &str) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(write).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            println!(
                "{}\t{}\t{}\t{}",
                write.operation_id, write.operation_kind, write.sequence, write.root_after
            );
            Ok(())
        }
        other => Err(format!(
            "unsupported chat output format {other:?}; supported formats: text, json"
        )),
    }
}

fn open_drive_read(
    store: &str,
    workspace: &str,
    keys: &KeyOpts,
) -> Result<(Loom<FileStore>, WorkspaceId, String), String> {
    let loom = cli_open_loom_read(store, keys)?;
    let workspace_id = resolve_ns(&loom, workspace)?;
    let profile_id = workspace_id.to_string();
    Ok((loom, workspace_id, profile_id))
}

fn open_drive_write(
    store: &str,
    workspace: &str,
    keys: &KeyOpts,
) -> Result<(CliLoom, WorkspaceId, String), String> {
    let loom = cli_open_loom(store, keys)?;
    let workspace_id = resolve_ns(&loom, workspace)?;
    let profile_id = workspace_id.to_string();
    Ok((loom, workspace_id, profile_id))
}

fn run_drive(action: DriveCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        DriveCmd::List {
            store,
            workspace,
            folder_id,
            format,
        } => {
            let (loom, workspace_id, profile_id) = open_drive_read(&store, &workspace, keys)?;
            let folder = loom_drive::list_folder(&loom, workspace_id, &profile_id, &folder_id)
                .map_err(|e| e.to_string())?;
            print_drive_folder(&folder, &format)
        }
        DriveCmd::Stat {
            store,
            workspace,
            folder_id,
            name,
            format,
        } => {
            let (loom, workspace_id, profile_id) = open_drive_read(&store, &workspace, keys)?;
            let stat = loom_drive::stat_node(&loom, workspace_id, &profile_id, &folder_id, &name)
                .map_err(|e| e.to_string())?;
            print_drive_stat(&stat, &format)
        }
        DriveCmd::Read {
            store,
            workspace,
            file_id,
            out,
        } => {
            let (loom, workspace_id, profile_id) = open_drive_read(&store, &workspace, keys)?;
            let bytes = loom_drive::read_file(&loom, workspace_id, &profile_id, &file_id)
                .map_err(|e| e.to_string())?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        DriveCmd::ListVersions {
            store,
            workspace,
            file_id,
            format,
        } => {
            let (loom, workspace_id, profile_id) = open_drive_read(&store, &workspace, keys)?;
            let versions = loom_drive::list_versions(&loom, workspace_id, &profile_id, &file_id)
                .map_err(|e| e.to_string())?;
            print_drive_versions(&versions, &format)
        }
        DriveCmd::ListConflicts {
            store,
            workspace,
            format,
        } => {
            let (loom, workspace_id, profile_id) = open_drive_read(&store, &workspace, keys)?;
            let conflicts = loom_drive::list_conflicts(&loom, workspace_id, &profile_id)
                .map_err(|e| e.to_string())?;
            print_drive_conflicts(&conflicts, &format)
        }
        DriveCmd::ListShares {
            store,
            workspace,
            format,
        } => {
            let (loom, workspace_id, profile_id) = open_drive_read(&store, &workspace, keys)?;
            let shares = loom_drive::list_shares(&loom, workspace_id, &profile_id)
                .map_err(|e| e.to_string())?;
            print_drive_shares(&shares, &format)
        }
        DriveCmd::GrantShare {
            store,
            workspace,
            grant_id,
            target_kind,
            target_id,
            principal,
            role,
            granted_at_ms,
            expires_at_ms,
            format,
        } => {
            let (mut loom, workspace_id, profile_id) = open_drive_write(&store, &workspace, keys)?;
            let write = loom_drive::grant_share(
                &mut loom,
                workspace_id,
                loom_drive::HostedDriveGrantShare {
                    workspace_id: &profile_id,
                    grant_id: &grant_id,
                    target_kind: &target_kind,
                    target_id: &target_id,
                    principal: &principal,
                    role: &role,
                    granted_at_ms,
                    expires_at_ms,
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_drive_write(&write, &format)
        }
        DriveCmd::RevokeShare {
            store,
            workspace,
            grant_id,
            format,
        } => {
            let (mut loom, workspace_id, profile_id) = open_drive_write(&store, &workspace, keys)?;
            let write = loom_drive::revoke_share(&mut loom, workspace_id, &profile_id, &grant_id)
                .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_drive_write(&write, &format)
        }
        DriveCmd::ApplyShareExpiry {
            store,
            workspace,
            now_ms,
            format,
        } => {
            let (mut loom, workspace_id, profile_id) = open_drive_write(&store, &workspace, keys)?;
            let applied =
                loom_drive::apply_share_expiry(&mut loom, workspace_id, &profile_id, now_ms)
                    .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_drive_share_expiry_apply(&applied, &format)
        }
        DriveCmd::ListRetention {
            store,
            workspace,
            format,
        } => {
            let (loom, workspace_id, profile_id) = open_drive_read(&store, &workspace, keys)?;
            let pins = loom_drive::list_retention(&loom, workspace_id, &profile_id)
                .map_err(|e| e.to_string())?;
            print_drive_retention(&pins, &format)
        }
        DriveCmd::PinRetention {
            store,
            workspace,
            pin_id,
            kind,
            root,
            target_entity_id,
            added_at_ms,
            expires_at_ms,
            format,
        } => {
            let (mut loom, workspace_id, profile_id) = open_drive_write(&store, &workspace, keys)?;
            let write = loom_drive::pin_retention(
                &mut loom,
                workspace_id,
                loom_drive::HostedDrivePinRetention {
                    workspace_id: &profile_id,
                    pin_id: &pin_id,
                    kind: &kind,
                    root: &root,
                    target_entity_id: target_entity_id.as_deref(),
                    added_at_ms,
                    expires_at_ms,
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_drive_write(&write, &format)
        }
        DriveCmd::UnpinRetention {
            store,
            workspace,
            pin_id,
            format,
        } => {
            let (mut loom, workspace_id, profile_id) = open_drive_write(&store, &workspace, keys)?;
            let write = loom_drive::unpin_retention(&mut loom, workspace_id, &profile_id, &pin_id)
                .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_drive_write(&write, &format)
        }
        DriveCmd::ApplyRetention {
            store,
            workspace,
            now_ms,
            format,
        } => {
            let (mut loom, workspace_id, profile_id) = open_drive_write(&store, &workspace, keys)?;
            let applied = loom_drive::apply_retention(&mut loom, workspace_id, &profile_id, now_ms)
                .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_drive_retention_apply(&applied, &format)
        }
        DriveCmd::CreateFolder {
            store,
            workspace,
            parent_folder_id,
            folder_id,
            name,
            expected_root,
            format,
        } => {
            let (mut loom, workspace_id, profile_id) = open_drive_write(&store, &workspace, keys)?;
            let write = loom_drive::create_folder(
                &mut loom,
                workspace_id,
                &profile_id,
                &parent_folder_id,
                &folder_id,
                &name,
                &expected_root,
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_drive_write(&write, &format)
        }
        DriveCmd::CreateUpload {
            store,
            workspace,
            upload_id,
            parent_folder_id,
            name,
            file_id,
            expected_root,
            created_at_ms,
            replace_file,
            format,
        } => {
            let (mut loom, workspace_id, profile_id) = open_drive_write(&store, &workspace, keys)?;
            let upload = loom_drive::create_upload(
                &mut loom,
                workspace_id,
                loom_drive::HostedDriveCreateUpload {
                    workspace_id: &profile_id,
                    upload_id: &upload_id,
                    parent_folder_id: &parent_folder_id,
                    name: &name,
                    file_id: &file_id,
                    expected_root: &expected_root,
                    created_at_ms,
                    replace_file,
                },
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_drive_upload(&upload, &format)
        }
        DriveCmd::UploadChunk {
            store,
            workspace,
            upload_id,
            input,
            format,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let (mut loom, workspace_id, profile_id) = open_drive_write(&store, &workspace, keys)?;
            let upload =
                loom_drive::upload_chunk(&mut loom, workspace_id, &profile_id, &upload_id, &bytes)
                    .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_drive_upload(&upload, &format)
        }
        DriveCmd::CommitUpload {
            store,
            workspace,
            upload_id,
            format,
        } => {
            let (mut loom, workspace_id, profile_id) = open_drive_write(&store, &workspace, keys)?;
            let write = loom_drive::commit_upload(&mut loom, workspace_id, &profile_id, &upload_id)
                .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_drive_write(&write, &format)
        }
        DriveCmd::Rename {
            store,
            workspace,
            folder_id,
            node_id,
            new_name,
            expected_root,
            format,
        } => {
            let (mut loom, workspace_id, profile_id) = open_drive_write(&store, &workspace, keys)?;
            let write = loom_drive::rename_node(
                &mut loom,
                workspace_id,
                &profile_id,
                &folder_id,
                &node_id,
                &new_name,
                &expected_root,
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_drive_write(&write, &format)
        }
        DriveCmd::Move {
            store,
            workspace,
            source_folder_id,
            target_folder_id,
            node_id,
            expected_root,
            format,
        } => {
            let (mut loom, workspace_id, profile_id) = open_drive_write(&store, &workspace, keys)?;
            let write = loom_drive::move_node(
                &mut loom,
                workspace_id,
                &profile_id,
                &source_folder_id,
                &target_folder_id,
                &node_id,
                &expected_root,
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_drive_write(&write, &format)
        }
        DriveCmd::Delete {
            store,
            workspace,
            folder_id,
            node_id,
            expected_root,
            format,
        } => {
            let (mut loom, workspace_id, profile_id) = open_drive_write(&store, &workspace, keys)?;
            let write = loom_drive::delete_node(
                &mut loom,
                workspace_id,
                &profile_id,
                &folder_id,
                &node_id,
                &expected_root,
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_drive_write(&write, &format)
        }
        DriveCmd::ResolveConflict {
            store,
            workspace,
            conflict_id,
            resolution,
            format,
        } => {
            let resolution = parse_drive_conflict_resolution(&resolution)?;
            let (mut loom, workspace_id, profile_id) = open_drive_write(&store, &workspace, keys)?;
            let write = loom_drive::resolve_conflict(
                &mut loom,
                workspace_id,
                &profile_id,
                &conflict_id,
                resolution,
            )
            .map_err(|e| e.to_string())?;
            save_loom(&mut loom).map_err(|e| e.to_string())?;
            print_drive_write(&write, &format)
        }
    }
}

fn print_drive_folder(folder: &loom_drive::HostedDriveFolder, format: &str) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string(folder).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            for entry in &folder.entries {
                println!(
                    "{}\t{}\t{}\t{}",
                    entry.name, entry.kind, entry.node_id, entry.fold_key
                );
            }
            Ok(())
        }
        other => Err(format!(
            "unsupported drive output format {other:?}; supported formats: text, json"
        )),
    }
}

fn print_drive_stat(stat: &loom_drive::HostedDriveStat, format: &str) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string(stat).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            println!("name\t{}", stat.name);
            println!("node_id\t{}", stat.node_id);
            println!("kind\t{}", stat.kind);
            if let Some(version) = &stat.latest_version {
                println!("version\t{}", version.version);
                println!("content_digest\t{}", version.content_digest);
                println!("size\t{}", version.size);
            }
            Ok(())
        }
        other => Err(format!(
            "unsupported drive output format {other:?}; supported formats: text, json"
        )),
    }
}

fn print_drive_versions(
    versions: &[loom_drive::HostedDriveVersion],
    format: &str,
) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(versions).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            for version in versions {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    version.file_id,
                    version.version,
                    version.size,
                    version.content_digest,
                    version.timestamp_ms
                );
            }
            Ok(())
        }
        other => Err(format!(
            "unsupported drive output format {other:?}; supported formats: text, json"
        )),
    }
}

fn print_drive_conflicts(
    conflicts: &[loom_drive::HostedDriveConflict],
    format: &str,
) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(conflicts).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            for conflict in conflicts {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    conflict.conflict_id,
                    conflict.folder_id,
                    conflict.visible_node_id,
                    conflict.conflict_node_id,
                    conflict.resolution,
                    conflict.conflict_name
                );
            }
            Ok(())
        }
        other => Err(format!(
            "unsupported drive output format {other:?}; supported formats: text, json"
        )),
    }
}

fn print_drive_shares(
    shares: &[loom_drive::HostedDriveShareGrant],
    format: &str,
) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(shares).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            for share in shares {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    share.grant_id,
                    share.target_kind,
                    share.target_id,
                    share.role,
                    share.principal,
                    share
                        .expires_at_ms
                        .map(|value| value.to_string())
                        .unwrap_or_default()
                );
            }
            Ok(())
        }
        other => Err(format!(
            "unsupported drive output format {other:?}; supported formats: text, json"
        )),
    }
}

fn print_drive_retention(
    pins: &[loom_drive::HostedDriveRetentionPin],
    format: &str,
) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(pins).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            for pin in pins {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    pin.pin_id,
                    pin.kind,
                    pin.root,
                    pin.target_entity_id.as_deref().unwrap_or(""),
                    pin.expires_at_ms
                        .map(|value| value.to_string())
                        .unwrap_or_default()
                );
            }
            Ok(())
        }
        other => Err(format!(
            "unsupported drive output format {other:?}; supported formats: text, json"
        )),
    }
}

fn print_drive_upload(
    upload: &loom_drive::HostedDriveUploadSession,
    format: &str,
) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(upload).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            println!("upload_id\t{}", upload.upload_id);
            println!("target_kind\t{}", upload.target_kind);
            println!("parent_folder_id\t{}", upload.parent_folder_id);
            println!("file_id\t{}", upload.file_id);
            println!("chunk_count\t{}", upload.chunk_count);
            println!("total_size\t{}", upload.total_size);
            Ok(())
        }
        other => Err(format!(
            "unsupported drive output format {other:?}; supported formats: text, json"
        )),
    }
}

fn print_drive_write(write: &loom_drive::HostedDriveWrite, format: &str) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(write).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            println!(
                "{}\t{}\t{}\t{}\t{}",
                write.operation_id,
                write.operation_kind,
                write.sequence,
                write.profile_root,
                write.target_entity_id.as_deref().unwrap_or("")
            );
            if let Some(conflict_id) = &write.conflict_id {
                println!("conflict_id\t{conflict_id}");
            }
            Ok(())
        }
        other => Err(format!(
            "unsupported drive output format {other:?}; supported formats: text, json"
        )),
    }
}

fn print_drive_retention_apply(
    applied: &loom_drive::HostedDriveRetentionApply,
    format: &str,
) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(applied).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            println!("now_ms\t{}", applied.now_ms);
            println!("expired\t{}", applied.expired_pin_ids.join(","));
            println!("remaining\t{}", applied.remaining_pins);
            if let Some(write) = &applied.operation {
                print_drive_write(write, "text")?;
            }
            Ok(())
        }
        other => Err(format!(
            "unsupported drive output format {other:?}; supported formats: text, json"
        )),
    }
}

fn print_drive_share_expiry_apply(
    applied: &loom_drive::HostedDriveShareExpiryApply,
    format: &str,
) -> Result<(), String> {
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(applied).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            println!("now_ms\t{}", applied.now_ms);
            println!("expired\t{}", applied.expired_grant_ids.join(","));
            println!("remaining\t{}", applied.remaining_grants);
            if let Some(write) = &applied.operation {
                print_drive_write(write, "text")?;
            }
            Ok(())
        }
        other => Err(format!(
            "unsupported drive output format {other:?}; supported formats: text, json"
        )),
    }
}

fn parse_drive_conflict_resolution(
    value: &str,
) -> Result<loom_drive::HostedDriveConflictResolution, String> {
    match value {
        "keep-current" => Ok(loom_drive::HostedDriveConflictResolution::KeepCurrent),
        "keep-conflict" => Ok(loom_drive::HostedDriveConflictResolution::KeepConflict),
        "keep-both" => Ok(loom_drive::HostedDriveConflictResolution::KeepBoth),
        other => Err(format!(
            "unsupported drive conflict resolution {other:?}; supported values: keep-current, keep-conflict, keep-both"
        )),
    }
}

fn run_files(action: FilesCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        FilesCmd::Delete {
            store,
            workspace,
            path,
            recursive,
        } => {
            let client = remote::open_store_client(&store)?;
            client.fs_delete(keys, &workspace, &path, recursive)
        }
        FilesCmd::Ls { store, workspace } => {
            let client = remote::open_store_client(&store)?;
            for p in client.fs_ls(keys, &workspace)? {
                println!("{p}");
            }
            Ok(())
        }
        FilesCmd::Mkdir {
            store,
            workspace,
            path,
            parents,
        } => {
            let client = remote::open_store_client(&store)?;
            client.fs_mkdir(keys, &workspace, &path, parents)
        }
        FilesCmd::Read {
            store,
            workspace,
            path,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let bytes = client.fs_read_file(keys, &workspace, &path)?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        FilesCmd::Write {
            store,
            workspace,
            path,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_store_client(&store)?;
            client.fs_write_file(keys, &workspace, &path, bytes)
        }
    }
}

fn run_redmine_import(
    store: &str,
    workspace: &str,
    profile: &str,
    snapshot: &str,
    dry_run: bool,
    field_policy: &str,
    format: &str,
    keys: &KeyOpts,
) -> Result<(), String> {
    let (mut loom, ns, input) = open_profile_import_input(store, workspace, snapshot, keys)?;
    let bytes = file_import_bytes(&input, "Redmine")?;
    let field_policy = loom_interchange_io::TicketImportFieldPolicy::parse(field_policy)
        .map_err(|e| e.to_string())?;
    let report = loom_interchange_io::import_redmine_bytes_with_field_policy(
        &mut loom,
        ns,
        profile,
        &input.source_scope,
        bytes,
        dry_run,
        field_policy,
    )
    .map_err(|e| e.to_string())?;
    let persisted = persist_profile_import_artifacts(&mut loom, ns, profile, &input, &report)?;
    if report.operations_applied > 0 || persisted {
        save_loom(&mut loom).map_err(|e| e.to_string())?;
    }
    print_import_report(&report, format)
}

fn run_asana_import(
    store: &str,
    workspace: &str,
    profile: &str,
    snapshot: &str,
    dry_run: bool,
    field_policy: &str,
    format: &str,
    keys: &KeyOpts,
) -> Result<(), String> {
    let (mut loom, ns, input) = open_profile_import_input(store, workspace, snapshot, keys)?;
    let bytes = file_import_bytes(&input, "Asana")?;
    let field_policy = loom_interchange_io::TicketImportFieldPolicy::parse(field_policy)
        .map_err(|e| e.to_string())?;
    let report = loom_interchange_io::import_asana_bytes_with_field_policy(
        &mut loom,
        ns,
        profile,
        &input.source_scope,
        bytes,
        dry_run,
        field_policy,
    )
    .map_err(|e| e.to_string())?;
    let persisted = persist_profile_import_artifacts(&mut loom, ns, profile, &input, &report)?;
    if report.operations_applied > 0 || persisted {
        save_loom(&mut loom).map_err(|e| e.to_string())?;
    }
    print_import_report(&report, format)
}

fn run_confluence_import(
    store: &str,
    workspace: &str,
    profile: &str,
    snapshot: &str,
    default_space: &str,
    dry_run: bool,
    format: &str,
    keys: &KeyOpts,
) -> Result<(), String> {
    let (mut loom, ns, input) = open_profile_import_input(store, workspace, snapshot, keys)?;
    let bytes = file_import_bytes(&input, "Confluence")?;
    let report = loom_interchange_io::import_confluence_bytes(
        &mut loom,
        ns,
        profile,
        &input.source_scope,
        default_space,
        bytes,
        dry_run,
    )
    .map_err(|e| e.to_string())?;
    let persisted = persist_profile_import_artifacts(&mut loom, ns, profile, &input, &report)?;
    if report.operations_applied > 0 || persisted {
        save_loom(&mut loom).map_err(|e| e.to_string())?;
    }
    print_import_report(&report, format)
}

fn run_slack_import(
    store: &str,
    workspace: &str,
    profile: &str,
    snapshot: &str,
    dry_run: bool,
    format: &str,
    keys: &KeyOpts,
) -> Result<(), String> {
    let (mut loom, ns, input) = open_profile_import_input(store, workspace, snapshot, keys)?;
    let bytes = file_import_bytes(&input, "Slack")?;
    let report = loom_interchange_io::import_slack_bytes(
        &mut loom,
        ns,
        profile,
        &input.source_scope,
        bytes,
        dry_run,
    )
    .map_err(|e| e.to_string())?;
    let persisted = persist_profile_import_artifacts(&mut loom, ns, profile, &input, &report)?;
    if report.operations_applied > 0 || persisted {
        save_loom(&mut loom).map_err(|e| e.to_string())?;
    }
    print_import_report(&report, format)
}

fn run_drive_import(
    store: &str,
    workspace: &str,
    profile: &str,
    snapshot: &str,
    dry_run: bool,
    format: &str,
    keys: &KeyOpts,
) -> Result<(), String> {
    let (mut loom, ns, input) = open_profile_import_input(store, workspace, snapshot, keys)?;
    let bytes = file_import_bytes(&input, "Drive")?;
    let snapshot_dir = input
        .path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let report = loom_interchange_io::import_drive_bytes(
        &mut loom,
        ns,
        profile,
        &input.source_scope,
        bytes,
        snapshot_dir,
        dry_run,
    )
    .map_err(|e| e.to_string())?;
    let persisted = persist_profile_import_artifacts(&mut loom, ns, profile, &input, &report)?;
    if report.operations_applied > 0 || persisted {
        save_loom(&mut loom).map_err(|e| e.to_string())?;
    }
    print_import_report(&report, format)
}

fn run_jira_import(
    store: &str,
    workspace: &str,
    profile: &str,
    snapshot: &str,
    dry_run: bool,
    field_policy: &str,
    format: &str,
    keys: &KeyOpts,
) -> Result<(), String> {
    let (mut loom, ns, input) = open_profile_import_input(store, workspace, snapshot, keys)?;
    let bytes = file_import_bytes(&input, "Jira")?;
    let field_policy = loom_interchange_io::TicketImportFieldPolicy::parse(field_policy)
        .map_err(|e| e.to_string())?;
    let report = loom_interchange_io::import_jira_bytes_with_field_policy(
        &mut loom,
        ns,
        profile,
        &input.source_scope,
        bytes,
        dry_run,
        field_policy,
    )
    .map_err(|e| e.to_string())?;
    let persisted = persist_profile_import_artifacts(&mut loom, ns, profile, &input, &report)?;
    if report.operations_applied > 0 || persisted {
        save_loom(&mut loom).map_err(|e| e.to_string())?;
    }
    print_import_report(&report, format)
}

fn run_markdown_import(
    store: &str,
    workspace: &str,
    profile: &str,
    src: &str,
    space: &str,
    dry_run: bool,
    format: &str,
    keys: &KeyOpts,
) -> Result<(), String> {
    let (mut loom, ns, input) = open_profile_import_input(store, workspace, src, keys)?;
    let report = loom_interchange_io::import_markdown_path(
        &mut loom,
        ns,
        profile,
        &input.source_scope,
        &input.path,
        space,
        dry_run,
    )
    .map_err(|e| e.to_string())?;
    let persisted = persist_profile_import_artifacts(&mut loom, ns, profile, &input, &report)?;
    if report.operations_applied > 0 || persisted {
        save_loom(&mut loom).map_err(|e| e.to_string())?;
    }
    print_import_report(&report, format)
}

fn run_notion_import(
    store: &str,
    workspace: &str,
    profile: &str,
    snapshot: &str,
    default_space: &str,
    dry_run: bool,
    format: &str,
    keys: &KeyOpts,
) -> Result<(), String> {
    let (mut loom, ns, input) = open_profile_import_input(store, workspace, snapshot, keys)?;
    let bytes = file_import_bytes(&input, "Notion")?;
    let report = loom_interchange_io::import_notion_bytes(
        &mut loom,
        ns,
        profile,
        &input.source_scope,
        default_space,
        bytes,
        dry_run,
    )
    .map_err(|e| e.to_string())?;
    let persisted = persist_profile_import_artifacts(&mut loom, ns, profile, &input, &report)?;
    if report.operations_applied > 0 || persisted {
        save_loom(&mut loom).map_err(|e| e.to_string())?;
    }
    print_import_report(&report, format)
}

fn open_profile_import_input(
    store: &str,
    workspace: &str,
    source: &str,
    keys: &KeyOpts,
) -> Result<(CliLoom, WorkspaceId, ResolvedImportInput), String> {
    let loom = cli_open_loom(store, keys)?;
    let ns = resolve_ns(&loom, workspace)?;
    let input = loom_interchange_io::resolve_import_input(
        std::path::Path::new(source),
        loom.store().digest_algo(),
    )
    .map_err(|e| e.to_string())?;
    Ok((loom, ns, input))
}

fn file_import_bytes<'a>(
    input: &'a ResolvedImportInput,
    profile: &str,
) -> Result<&'a [u8], String> {
    input
        .bytes
        .as_deref()
        .ok_or_else(|| format!("{profile} import requires a file input"))
}

fn persist_profile_import_artifacts(
    loom: &mut CliLoom,
    ns: WorkspaceId,
    profile: &str,
    input: &ResolvedImportInput,
    report: &loom_interchange::ImportReport,
) -> Result<bool, String> {
    if report.dry_run {
        return Ok(false);
    }
    let retained =
        retain_import_input(loom, ns, profile, input, None).map_err(|e| e.to_string())?;
    let checkpoint_id = format!("{}:{}", profile, input.source_digest);
    let mut checkpoint = input
        .checkpoint(profile, &checkpoint_id)
        .map_err(|e| e.to_string())?;
    checkpoint.profile_state_digest = Some(retained.manifest_digest);
    persist_import_checkpoint(loom, ns, &checkpoint, None).map_err(|e| e.to_string())?;
    Ok(true)
}

fn run_interchange(action: InterchangeCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        InterchangeCmd::ImportFs {
            store,
            workspace,
            src,
            commit,
            dry_run,
            author,
            message,
            format,
        } => {
            if remote::target_is_remote(&store)? {
                return Err("`import fs` to/from a remote store is not supported yet (fs-tree byte transfer is deferred, specs/0067 §17.2); use `import archive` with a tar/zip payload, or run against a local store".to_string());
            }
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let src_path = PathBuf::from(&src);
            let mut options = FsImportOptions::new(&src);
            options.commit = commit;
            options.dry_run = dry_run;
            options.author = author;
            options.message = message;
            let report =
                import_fs(&mut loom, ns, &src_path, &options).map_err(|e| e.to_string())?;
            if !dry_run {
                save_loom(&mut loom).map_err(|e| e.to_string())?;
            }
            print_import_report(&report, &format)
        }
        InterchangeCmd::ImportArchive {
            store,
            workspace,
            archive,
            kind,
            gzip_output_path,
            commit,
            dry_run,
            author,
            message,
            format,
        } => {
            let client = remote::open_store_client(&store)?;
            if client.is_remote() {
                // Remote: read the archive locally and drive the byte-transfer contract (§17). v1 does
                // not thread `gzip_output_path`/`author`/`message` over the transfer contract.
                let summary = client.transfer_import(
                    keys,
                    &workspace,
                    archive_transfer_kind_name(&kind)?,
                    &archive,
                    commit,
                    dry_run,
                )?;
                println!("{summary}");
                return Ok(());
            }
            let kind = parse_archive_kind(&kind)?;
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let archive_path = PathBuf::from(&archive);
            let mut options = ArchiveImportOptions::new(&archive);
            options.gzip_output_path = gzip_output_path;
            options.commit = commit;
            options.dry_run = dry_run;
            options.author = author;
            options.message = message;
            let result = import_archive(&mut loom, ns, &archive_path, kind, &options)
                .map_err(|e| e.to_string())?;
            if !dry_run {
                save_loom(&mut loom).map_err(|e| e.to_string())?;
            }
            print_archive_import_result(&result, &format)
        }
        InterchangeCmd::ImportTableCsv {
            store,
            workspace,
            database,
            table,
            csv,
            schema,
            primary_key,
            mode,
            commit,
            dry_run,
            author,
            message,
            format,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let csv_path = PathBuf::from(&csv);
            let mut options = TableCsvImportOptions::new(
                &csv,
                database,
                table,
                parse_table_csv_schema(&schema)?,
                parse_table_csv_primary_key(&primary_key)?,
            );
            options.mode = parse_table_csv_import_mode(&mode)?;
            options.commit = commit;
            options.dry_run = dry_run;
            options.author = author;
            options.message = message;
            let report =
                import_table_csv(&mut loom, ns, &csv_path, &options).map_err(|e| e.to_string())?;
            if !dry_run {
                save_loom(&mut loom).map_err(|e| e.to_string())?;
            }
            print_import_report(&report, &format)
        }
        InterchangeCmd::ImportRedmine {
            store,
            workspace,
            profile,
            snapshot,
            dry_run,
            field_policy,
            format,
        } => run_redmine_import(
            &store,
            &workspace,
            &profile,
            &snapshot,
            dry_run,
            &field_policy,
            &format,
            keys,
        ),
        InterchangeCmd::ImportAsana {
            store,
            workspace,
            profile,
            snapshot,
            dry_run,
            field_policy,
            format,
        } => run_asana_import(
            &store,
            &workspace,
            &profile,
            &snapshot,
            dry_run,
            &field_policy,
            &format,
            keys,
        ),
        InterchangeCmd::ImportJira {
            store,
            workspace,
            profile,
            snapshot,
            dry_run,
            field_policy,
            format,
        } => run_jira_import(
            &store,
            &workspace,
            &profile,
            &snapshot,
            dry_run,
            &field_policy,
            &format,
            keys,
        ),
        InterchangeCmd::ImportConfluence {
            store,
            workspace,
            profile,
            snapshot,
            space,
            dry_run,
            format,
        } => run_confluence_import(
            &store, &workspace, &profile, &snapshot, &space, dry_run, &format, keys,
        ),
        InterchangeCmd::ImportSlack {
            store,
            workspace,
            profile,
            snapshot,
            dry_run,
            format,
        } => run_slack_import(
            &store, &workspace, &profile, &snapshot, dry_run, &format, keys,
        ),
        InterchangeCmd::ImportDrive {
            store,
            workspace,
            profile,
            snapshot,
            dry_run,
            format,
        } => run_drive_import(
            &store, &workspace, &profile, &snapshot, dry_run, &format, keys,
        ),
        InterchangeCmd::ImportMarkdown {
            store,
            workspace,
            profile,
            src,
            space,
            dry_run,
            format,
        } => run_markdown_import(
            &store, &workspace, &profile, &src, &space, dry_run, &format, keys,
        ),
        InterchangeCmd::ImportNotion {
            store,
            workspace,
            profile,
            snapshot,
            space,
            dry_run,
            format,
        } => run_notion_import(
            &store, &workspace, &profile, &snapshot, &space, dry_run, &format, keys,
        ),
        InterchangeCmd::ExportArchive {
            store,
            workspace,
            archive,
            kind,
            revision,
            dry_run,
            format,
        } => {
            let client = remote::open_store_client(&store)?;
            if client.is_remote() {
                if dry_run {
                    return Err("dry-run export is not supported over a remote store".to_string());
                }
                let summary = client.transfer_export(
                    keys,
                    &workspace,
                    archive_transfer_kind_name(&kind)?,
                    revision.as_deref(),
                    &archive,
                )?;
                println!("{summary}");
                return Ok(());
            }
            let kind = parse_archive_kind(&kind)?;
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let archive_path = PathBuf::from(&archive);
            let mut options = ArchiveExportOptions::new(&archive);
            options.revision = revision;
            options.dry_run = dry_run;
            let result = export_archive(&loom, ns, &archive_path, kind, &options)
                .map_err(|e| e.to_string())?;
            print_archive_export_result(&result, &format)
        }
        InterchangeCmd::ExportFs {
            store,
            workspace,
            dst,
            revision,
            dry_run,
            format,
        } => {
            if remote::target_is_remote(&store)? {
                return Err("`export fs` to/from a remote store is not supported yet (fs-tree byte transfer is deferred, specs/0067 §17.2); use `export archive` with a tar/zip payload, or run against a local store".to_string());
            }
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let dst_path = PathBuf::from(&dst);
            let mut options = FsExportOptions::new(&dst);
            options.dry_run = dry_run;
            options.revision = revision;
            let report = export_fs(&loom, ns, &dst_path, &options).map_err(|e| e.to_string())?;
            print_export_report(&report, &format)
        }
        InterchangeCmd::ExportTableCsv {
            store,
            workspace,
            database,
            table,
            csv,
            dry_run,
            format,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let csv_path = PathBuf::from(&csv);
            let mut options = TableCsvExportOptions::new(&csv, database, table);
            options.dry_run = dry_run;
            let report =
                export_table_csv(&loom, ns, &csv_path, &options).map_err(|e| e.to_string())?;
            print_export_report(&report, &format)
        }
        InterchangeCmd::ExportCar {
            store,
            workspace,
            dst,
            dry_run,
            format,
        } => {
            let client = remote::open_store_client(&store)?;
            if client.is_remote() {
                if dry_run {
                    return Err("dry-run export is not supported over a remote store".to_string());
                }
                let summary = client.transfer_export(keys, &workspace, "car", None, &dst)?;
                println!("{summary}");
                return Ok(());
            }
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let dst_path = PathBuf::from(&dst);
            let mut options = CarExportOptions::new(&dst);
            options.dry_run = dry_run;
            let result = export_car(&loom, ns, &dst_path, &options).map_err(|e| e.to_string())?;
            print_car_export_result(&result, &format)
        }
        InterchangeCmd::ImportCar {
            store,
            src,
            dry_run,
            format,
        } => {
            let client = remote::open_store_client(&store)?;
            if client.is_remote() {
                // A CAR import derives its own workspace from the manifest; workspace is unused here.
                let summary = client.transfer_import(keys, "", "car", &src, false, dry_run)?;
                println!("{summary}");
                return Ok(());
            }
            let mut loom = cli_open_loom(&store, keys)?;
            let src_path = PathBuf::from(&src);
            let mut options = CarImportOptions::new(&src);
            options.dry_run = dry_run;
            let result = import_car(&mut loom, &src_path, &options).map_err(|e| e.to_string())?;
            if !dry_run {
                save_loom(&mut loom).map_err(|e| e.to_string())?;
            }
            print_car_import_result(&result, &format)
        }
    }
}

fn parse_archive_kind(kind: &str) -> Result<ArchiveKind, String> {
    match kind {
        "zip" => Ok(ArchiveKind::Zip),
        "tar" => Ok(ArchiveKind::Tar),
        "tar-zstd" | "tar.zstd" | "tzst" => Ok(ArchiveKind::TarZstd),
        "tar-gzip" | "tar.gz" | "tgz" => Ok(ArchiveKind::TarGzip),
        "gzip" | "gz" => Ok(ArchiveKind::Gzip),
        other => Err(format!(
            "unsupported archive kind {other:?}; expected tar-zstd, tar, tar-gzip, zip, or gzip"
        )),
    }
}

/// Normalize a CLI archive-kind string (including aliases like `tzst`/`tar.gz`) to the canonical
/// byte-transfer kind name (`tar`/`tar-zstd`/`tar-gzip`/`zip`/`gzip`) used by the `Transfer` contract.
fn archive_transfer_kind_name(kind: &str) -> Result<&'static str, String> {
    match kind {
        "zip" => Ok("zip"),
        "tar" => Ok("tar"),
        "tar-zstd" | "tar.zstd" | "tzst" => Ok("tar-zstd"),
        "tar-gzip" | "tar.gz" | "tgz" => Ok("tar-gzip"),
        "gzip" | "gz" => Ok("gzip"),
        other => Err(format!(
            "unsupported archive kind {other:?}; expected tar-zstd, tar, tar-gzip, zip, or gzip"
        )),
    }
}

fn parse_table_csv_import_mode(mode: &str) -> Result<TableImportMode, String> {
    match mode {
        "snapshot" => Ok(TableImportMode::Snapshot),
        "append-only" => Ok(TableImportMode::AppendOnly),
        other => Err(format!(
            "unsupported table CSV import mode {other:?}; expected snapshot or append-only"
        )),
    }
}

fn parse_table_csv_primary_key(value: &str) -> Result<Vec<String>, String> {
    let columns: Vec<String> = value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect();
    if columns.is_empty() {
        return Err("table CSV primary key is empty".to_string());
    }
    Ok(columns)
}

fn parse_table_csv_schema(value: &str) -> Result<Vec<(String, ColumnType)>, String> {
    let mut columns = Vec::new();
    for item in value.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let (name, ty) = item
            .split_once(':')
            .ok_or_else(|| format!("table CSV schema item {item:?} is missing ':'"))?;
        let name = name.trim();
        if name.is_empty() {
            return Err(format!("table CSV schema item {item:?} has an empty name"));
        }
        columns.push((name.to_string(), parse_table_csv_column_type(ty.trim())?));
    }
    if columns.is_empty() {
        return Err("table CSV schema is empty".to_string());
    }
    Ok(columns)
}

fn parse_table_csv_column_type(value: &str) -> Result<ColumnType, String> {
    match value {
        "int" | "integer" => Ok(ColumnType::Int),
        "float" | "double" => Ok(ColumnType::Float),
        "text" | "string" => Ok(ColumnType::Text),
        "bool" | "boolean" => Ok(ColumnType::Bool),
        "i8" => Ok(ColumnType::I8),
        "i16" => Ok(ColumnType::I16),
        "i32" => Ok(ColumnType::I32),
        "i128" => Ok(ColumnType::I128),
        "u8" => Ok(ColumnType::U8),
        "u16" => Ok(ColumnType::U16),
        "u32" => Ok(ColumnType::U32),
        "u64" => Ok(ColumnType::U64),
        "u128" => Ok(ColumnType::U128),
        "f32" => Ok(ColumnType::F32),
        "decimal" | "numeric" => Ok(ColumnType::Decimal),
        "date" => Ok(ColumnType::Date),
        "time" => Ok(ColumnType::Time),
        "timestamp" => Ok(ColumnType::Timestamp),
        "uuid" => Ok(ColumnType::Uuid),
        other => Err(format!(
            "unsupported table CSV column type {other:?}; expected int, float, text, bool, decimal, date, time, timestamp, uuid, or sized integer/float aliases"
        )),
    }
}

fn print_import_report(
    report: &loom_interchange::ImportReport,
    format: &str,
) -> Result<(), String> {
    match format {
        "text" => {
            println!(
                "profile\t{}\nsource_scope\t{}\ndry_run\t{}\noperations_planned\t{}\noperations_applied\t{}\nbytes_in\t{}\nbytes_stored\t{}\ncommit\t{}",
                report.profile,
                report.source_scope,
                report.dry_run,
                report.operations_planned,
                report.operations_applied,
                report.bytes_in,
                report.bytes_stored,
                report
                    .commit
                    .map(|digest| digest.to_string())
                    .unwrap_or_else(|| "none".to_string())
            );
            Ok(())
        }
        "json" => {
            let json = serde_json::json!({
                "profile": &report.profile,
                "source_scope": &report.source_scope,
                "commit": report.commit.map(|digest| digest.to_string()),
                "objects_added": report.objects_added,
                "bytes_in": report.bytes_in,
                "bytes_stored": report.bytes_stored,
                "rows_imported": report.rows_imported,
                "skipped": report.skipped,
                "operations_planned": report.operations_planned,
                "operations_applied": report.operations_applied,
                "dry_run": report.dry_run,
                "warnings": &report.warnings,
                "fidelity_issues": report.fidelity_issues.iter().map(|ticket| serde_json::json!({
                    "severity": format!("{:?}", ticket.severity),
                    "source_entity_id": &ticket.source_entity_id,
                    "field": &ticket.field,
                    "reason": &ticket.reason,
                    "source_digest": ticket.source_digest.map(|digest| digest.to_string())
                })).collect::<Vec<_>>()
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        other => Err(format!(
            "unsupported format {other:?}; expected text or json"
        )),
    }
}

fn print_archive_import_result(result: &ArchiveImportResult, format: &str) -> Result<(), String> {
    match format {
        "text" => {
            println!(
                "archive_id\t{}\narchive_kind\t{:?}\narchive_root\t{}\narchive_entries\t{}",
                result.manifest.archive_id,
                result.manifest.kind,
                result.manifest.root_digest,
                result.manifest.entries.len()
            );
            print_import_report(&result.report, format)
        }
        "json" => {
            let report = &result.report;
            let json = serde_json::json!({
                "archive": {
                    "archive_id": &result.manifest.archive_id,
                    "kind": format!("{:?}", result.manifest.kind),
                    "root_digest": result.manifest.root_digest.to_string(),
                    "entry_count": result.manifest.entries.len(),
                    "entries": result.manifest.entries.iter().map(|entry| serde_json::json!({
                        "path": &entry.path,
                        "kind": format!("{:?}", entry.kind),
                        "size": entry.size,
                        "digest": entry.digest.map(|digest| digest.to_string()),
                        "link_target": &entry.link_target,
                    })).collect::<Vec<_>>()
                },
                "report": {
                    "profile": &report.profile,
                    "source_scope": &report.source_scope,
                    "commit": report.commit.map(|digest| digest.to_string()),
                    "objects_added": report.objects_added,
                    "bytes_in": report.bytes_in,
                    "bytes_stored": report.bytes_stored,
                    "rows_imported": report.rows_imported,
                    "skipped": report.skipped,
                    "operations_planned": report.operations_planned,
                    "operations_applied": report.operations_applied,
                    "dry_run": report.dry_run,
                    "warnings": &report.warnings,
                    "fidelity_issues": report.fidelity_issues.iter().map(|ticket| serde_json::json!({
                        "severity": format!("{:?}", ticket.severity),
                        "source_entity_id": &ticket.source_entity_id,
                        "field": &ticket.field,
                        "reason": &ticket.reason,
                        "source_digest": ticket.source_digest.map(|digest| digest.to_string())
                    })).collect::<Vec<_>>()
                }
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        other => Err(format!(
            "unsupported format {other:?}; expected text or json"
        )),
    }
}

fn print_archive_export_result(result: &ArchiveExportResult, format: &str) -> Result<(), String> {
    match format {
        "text" => {
            println!(
                "archive_id\t{}\narchive_kind\t{:?}\narchive_root\t{}\narchive_entries\t{}\nbytes_out\t{}",
                result.manifest.archive_id,
                result.manifest.kind,
                result.manifest.root_digest,
                result.manifest.entries.len(),
                result.report.bytes_out
            );
            print_export_report(&result.report, format)
        }
        "json" => {
            let report = &result.report;
            let json = serde_json::json!({
                "archive": {
                    "archive_id": &result.manifest.archive_id,
                    "kind": format!("{:?}", result.manifest.kind),
                    "root_digest": result.manifest.root_digest.to_string(),
                    "entry_count": result.manifest.entries.len(),
                    "entries": result.manifest.entries.iter().map(|entry| serde_json::json!({
                        "path": &entry.path,
                        "kind": format!("{:?}", entry.kind),
                        "size": entry.size,
                        "digest": entry.digest.map(|digest| digest.to_string()),
                        "link_target": &entry.link_target,
                    })).collect::<Vec<_>>()
                },
                "report": {
                    "profile": &report.profile,
                    "destination_scope": &report.destination_scope,
                    "files_written": report.files_written,
                    "rows_written": report.rows_written,
                    "bytes_out": report.bytes_out,
                    "dry_run": report.dry_run,
                    "warnings": &report.warnings,
                    "fidelity_issues": report.fidelity_issues.iter().map(|ticket| serde_json::json!({
                        "severity": format!("{:?}", ticket.severity),
                        "source_entity_id": &ticket.source_entity_id,
                        "field": &ticket.field,
                        "reason": &ticket.reason,
                        "source_digest": ticket.source_digest.map(|digest| digest.to_string())
                    })).collect::<Vec<_>>()
                }
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        other => Err(format!(
            "unsupported format {other:?}; expected text or json"
        )),
    }
}

fn print_export_report(
    report: &loom_interchange::ExportReport,
    format: &str,
) -> Result<(), String> {
    match format {
        "text" => {
            println!(
                "profile\t{}\ndestination_scope\t{}\ndry_run\t{}\nfiles_written\t{}\nrows_written\t{}\nbytes_out\t{}",
                report.profile,
                report.destination_scope,
                report.dry_run,
                report.files_written,
                report.rows_written,
                report.bytes_out
            );
            Ok(())
        }
        "json" => {
            let json = serde_json::json!({
                "profile": &report.profile,
                "destination_scope": &report.destination_scope,
                "files_written": report.files_written,
                "rows_written": report.rows_written,
                "bytes_out": report.bytes_out,
                "dry_run": report.dry_run,
                "warnings": &report.warnings,
                "fidelity_issues": report.fidelity_issues.iter().map(|ticket| serde_json::json!({
                    "severity": format!("{:?}", ticket.severity),
                    "source_entity_id": &ticket.source_entity_id,
                    "field": &ticket.field,
                    "reason": &ticket.reason,
                    "source_digest": ticket.source_digest.map(|digest| digest.to_string())
                })).collect::<Vec<_>>()
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        other => Err(format!(
            "unsupported format {other:?}; expected text or json"
        )),
    }
}

fn print_car_export_result(result: &CarExportResult, format: &str) -> Result<(), String> {
    match format {
        "text" => {
            println!(
                "profile\t{}\ndestination_scope\t{}\ndry_run\t{}\nroot_cid\t{}\nblocks_written\t{}\nbytes_out\t{}",
                result.report.profile,
                result.report.destination_scope,
                result.report.dry_run,
                result.root_cid_hex,
                result.blocks_written,
                result.bytes_out
            );
            Ok(())
        }
        "json" => {
            let report = &result.report;
            let json = serde_json::json!({
                "root_cid": &result.root_cid_hex,
                "blocks_written": result.blocks_written,
                "bytes_out": result.bytes_out,
                "report": {
                    "profile": &report.profile,
                    "destination_scope": &report.destination_scope,
                    "files_written": report.files_written,
                    "rows_written": report.rows_written,
                    "bytes_out": report.bytes_out,
                    "dry_run": report.dry_run,
                    "warnings": &report.warnings,
                    "fidelity_issues": report.fidelity_issues.iter().map(|ticket| serde_json::json!({
                        "severity": format!("{:?}", ticket.severity),
                        "source_entity_id": &ticket.source_entity_id,
                        "field": &ticket.field,
                        "reason": &ticket.reason,
                        "source_digest": ticket.source_digest.map(|digest| digest.to_string())
                    })).collect::<Vec<_>>()
                }
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        other => Err(format!(
            "unsupported format {other:?}; expected text or json"
        )),
    }
}

fn print_car_import_result(result: &CarImportResult, format: &str) -> Result<(), String> {
    match format {
        "text" => {
            println!(
                "profile\t{}\nsource_scope\t{}\ndry_run\t{}\nworkspace\t{}\nroot_cid\t{}\nblocks_read\t{}\nobjects_added\t{}\nskipped\t{}",
                result.report.profile,
                result.report.source_scope,
                result.report.dry_run,
                result
                    .workspace
                    .map(|ns| ns.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                result.root_cid_hex,
                result.blocks_read,
                result.report.objects_added,
                result.report.skipped
            );
            Ok(())
        }
        "json" => {
            let report = &result.report;
            let json = serde_json::json!({
                "workspace": result.workspace.map(|ns| ns.to_string()),
                "root_cid": &result.root_cid_hex,
                "blocks_read": result.blocks_read,
                "report": {
                    "profile": &report.profile,
                    "source_scope": &report.source_scope,
                    "commit": report.commit.map(|digest| digest.to_string()),
                    "objects_added": report.objects_added,
                    "bytes_in": report.bytes_in,
                    "bytes_stored": report.bytes_stored,
                    "rows_imported": report.rows_imported,
                    "skipped": report.skipped,
                    "operations_planned": report.operations_planned,
                    "operations_applied": report.operations_applied,
                    "dry_run": report.dry_run,
                    "warnings": &report.warnings,
                    "fidelity_issues": report.fidelity_issues.iter().map(|ticket| serde_json::json!({
                        "severity": format!("{:?}", ticket.severity),
                        "source_entity_id": &ticket.source_entity_id,
                        "field": &ticket.field,
                        "reason": &ticket.reason,
                        "source_digest": ticket.source_digest.map(|digest| digest.to_string())
                    })).collect::<Vec<_>>()
                }
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        other => Err(format!(
            "unsupported format {other:?}; expected text or json"
        )),
    }
}

#[derive(Clone, Copy, Default)]
struct StoreCopyModifiers {
    fips: bool,
    compacted: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StoreCopyFormat {
    Text,
    Json,
}

struct StoreCopyReport {
    source: String,
    destination: String,
    source_identity: &'static str,
    target_identity: &'static str,
    mode: &'static str,
    with_fips: bool,
    with_compacted: bool,
    dry_run: bool,
    profile_changed: bool,
    source_encrypted: bool,
    destination_encrypted: bool,
    workspaces: usize,
    objects_written: u64,
    content_written: u64,
    prolly_nodes_written: u64,
    audit_policy_imported: bool,
    served_listeners_to_import_disabled: usize,
    served_listeners_imported_disabled: usize,
    compaction_before_bytes: Option<u64>,
    compaction_after_bytes: Option<u64>,
    omitted_items: Vec<String>,
    warnings: Vec<String>,
}

struct StoreCopyReportInput<'a> {
    source: &'a str,
    destination: &'a str,
    source_algo: Algo,
    target_algo: Algo,
    modifiers: StoreCopyModifiers,
    mode: &'static str,
    workspaces: usize,
    source_encrypted: bool,
    destination_encrypted: bool,
    dry_run: bool,
}

impl StoreCopyReport {
    fn new(input: StoreCopyReportInput<'_>) -> Self {
        let mut omitted_items = Vec::new();
        if input.source_algo != input.target_algo {
            omitted_items.push("encrypted profile-changing copies".to_string());
        }
        Self {
            source: input.source.to_string(),
            destination: input.destination.to_string(),
            source_identity: input.source_algo.as_str(),
            target_identity: input.target_algo.as_str(),
            mode: input.mode,
            with_fips: input.modifiers.fips,
            with_compacted: input.modifiers.compacted,
            dry_run: input.dry_run,
            profile_changed: input.source_algo != input.target_algo,
            source_encrypted: input.source_encrypted,
            destination_encrypted: input.destination_encrypted,
            workspaces: input.workspaces,
            objects_written: 0,
            content_written: 0,
            prolly_nodes_written: 0,
            audit_policy_imported: false,
            served_listeners_to_import_disabled: 0,
            served_listeners_imported_disabled: 0,
            compaction_before_bytes: None,
            compaction_after_bytes: None,
            omitted_items,
            warnings: Vec::new(),
        }
    }
}

fn parse_store_copy_modifiers(values: &[String]) -> Result<StoreCopyModifiers, String> {
    let mut modifiers = StoreCopyModifiers::default();
    for value in values {
        match value.as_str() {
            "fips" => modifiers.fips = true,
            "compacted" => modifiers.compacted = true,
            other => {
                return Err(format!(
                    "unknown copy modifier {other:?} (expected `fips` or `compacted`)"
                ));
            }
        }
    }
    Ok(modifiers)
}

fn parse_store_copy_format(value: &str) -> Result<StoreCopyFormat, String> {
    match value {
        "text" => Ok(StoreCopyFormat::Text),
        "json" => Ok(StoreCopyFormat::Json),
        other => Err(format!(
            "unknown store copy format {other:?} (expected `text` or `json`)"
        )),
    }
}

fn emit_store_copy_report(
    report: &StoreCopyReport,
    format: StoreCopyFormat,
    report_file: Option<&str>,
) -> Result<(), String> {
    let json = store_copy_report_json(report);
    if let Some(path) = report_file {
        std::fs::write(path, &json).map_err(|e| format!("write report file {path}: {e}"))?;
    }
    match format {
        StoreCopyFormat::Text => print_store_copy_report(report),
        StoreCopyFormat::Json => println!("{json}"),
    }
    Ok(())
}

fn print_store_copy_report(report: &StoreCopyReport) {
    if report.dry_run {
        println!("store copy plan");
        println!("source\t{}", report.source);
        println!("destination\t{}", report.destination);
        println!("source_identity\t{}", report.source_identity);
        println!("target_identity\t{}", report.target_identity);
        println!("mode\t{}", report.mode);
        println!("with_fips\t{}", report.with_fips);
        println!("with_compacted\t{}", report.with_compacted);
        println!("source_encrypted\t{}", report.source_encrypted);
        println!("destination_encrypted\t{}", report.destination_encrypted);
        println!("workspaces\t{}", report.workspaces);
        println!(
            "served_listeners_to_import_disabled\t{}",
            report.served_listeners_to_import_disabled
        );
        return;
    }
    if report.profile_changed {
        let mut message = format!(
            "copied {} to {} ({} -> {}, workspaces {}, objects {}, content {}, prolly nodes {}",
            report.source,
            report.destination,
            report.source_identity,
            report.target_identity,
            report.workspaces,
            report.objects_written,
            report.content_written,
            report.prolly_nodes_written
        );
        if let (Some(before), Some(after)) = (
            report.compaction_before_bytes,
            report.compaction_after_bytes,
        ) {
            message.push_str(&format!(", compacted {before} -> {after} bytes"));
        }
        message.push(')');
        println!("{message}");
    } else if let (Some(before), Some(after)) = (
        report.compaction_before_bytes,
        report.compaction_after_bytes,
    ) {
        println!(
            "copied {} to {} (identity {}, compacted {} -> {} bytes)",
            report.source, report.destination, report.target_identity, before, after
        );
    } else {
        println!(
            "copied {} to {} (identity {}, workspaces {})",
            report.source, report.destination, report.target_identity, report.workspaces
        );
    }
}

fn store_copy_report_json(report: &StoreCopyReport) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"source\":");
    out.push_str(&json_string(&report.source));
    out.push_str(",\"destination\":");
    out.push_str(&json_string(&report.destination));
    out.push_str(",\"source_identity_profile\":");
    out.push_str(&json_string(report.source_identity));
    out.push_str(",\"destination_identity_profile\":");
    out.push_str(&json_string(report.target_identity));
    out.push_str(",\"mode\":");
    out.push_str(&json_string(report.mode));
    out.push_str(",\"with_fips\":");
    out.push_str(if report.with_fips { "true" } else { "false" });
    out.push_str(",\"with_compacted\":");
    out.push_str(if report.with_compacted {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"dry_run\":");
    out.push_str(if report.dry_run { "true" } else { "false" });
    out.push_str(",\"profile_changed\":");
    out.push_str(if report.profile_changed {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"source_encrypted\":");
    out.push_str(if report.source_encrypted {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"destination_encrypted\":");
    out.push_str(if report.destination_encrypted {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"workspaces\":");
    out.push_str(&report.workspaces.to_string());
    out.push_str(",\"objects_written\":");
    out.push_str(&report.objects_written.to_string());
    out.push_str(",\"content_written\":");
    out.push_str(&report.content_written.to_string());
    out.push_str(",\"prolly_nodes_written\":");
    out.push_str(&report.prolly_nodes_written.to_string());
    out.push_str(",\"audit_policy_imported\":");
    out.push_str(if report.audit_policy_imported {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"served_listeners_to_import_disabled\":");
    out.push_str(&report.served_listeners_to_import_disabled.to_string());
    out.push_str(",\"served_listeners_imported_disabled\":");
    out.push_str(&report.served_listeners_imported_disabled.to_string());
    out.push_str(",\"compaction_before_bytes\":");
    push_json_u64(&mut out, report.compaction_before_bytes);
    out.push_str(",\"compaction_after_bytes\":");
    push_json_u64(&mut out, report.compaction_after_bytes);
    out.push_str(",\"omitted_items\":");
    push_json_string_array(&mut out, &report.omitted_items);
    out.push_str(",\"warnings\":");
    push_json_string_array(&mut out, &report.warnings);
    out.push('}');
    out
}

fn push_json_u64(out: &mut String, value: Option<u64>) {
    match value {
        Some(value) => out.push_str(&value.to_string()),
        None => out.push_str("null"),
    }
}

fn push_json_string_array(out: &mut String, values: &[String]) {
    out.push('[');
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_string(value));
    }
    out.push(']');
}

fn store_policy_json(policy: StorePolicy, audit_seq: Option<u64>) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"fips_required\":");
    out.push_str(if policy.fips_required {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"audit_seq\":");
    push_json_u64(&mut out, audit_seq);
    out.push('}');
    out
}

fn ensure_store_copy_clean(loom: &Loom<FileStore>) -> Result<(), String> {
    for info in loom.registry().list(None) {
        let status = loom.status(info.id).map_err(|e| e.to_string())?;
        if !status.staged.is_empty() || !status.unstaged.is_empty() || !status.untracked.is_empty()
        {
            return Err(format!(
                "workspace {} ({}) has uncommitted changes; commit or discard them before a profile-changing copy",
                info.name, info.id
            ));
        }
    }
    Ok(())
}

fn copy_control_metadata(src: &FileStore, dst: &FileStore) -> Result<(), String> {
    if let Some(identity) = src.identity_store().map_err(|e| e.to_string())? {
        dst.save_identity_store(&identity)
            .map_err(|e| e.to_string())?;
    } else {
        init_control_state(dst)?;
    }
    if let Some(acl) = src.acl_store().map_err(|e| e.to_string())? {
        dst.save_acl_store(&acl).map_err(|e| e.to_string())?;
    }
    let policy = src.store_policy().map_err(|e| e.to_string())?;
    dst.save_store_policy_audited(
        policy,
        None,
        "store.copy.policy.import",
        Some("source=store-policy"),
    )
    .map_err(|e| e.to_string())?;
    let audit_config = src.audit_config().map_err(|e| e.to_string())?;
    dst.save_audit_config_audited(
        audit_config,
        None,
        "store.copy.audit_config.import",
        Some("source=audit-config"),
    )
    .map_err(|e| e.to_string())?;
    for mut record in src.served_listeners().map_err(|e| e.to_string())? {
        record.enabled = false;
        record.last_modified_audit_seq = None;
        let target = served_listener_target(&record);
        dst.save_served_listener_audited(
            &record,
            None,
            "store.copy.served_listener.import_disabled",
            Some(&target),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn default_init_digest_algo() -> Algo {
    #[cfg(feature = "fips")]
    {
        loom_hosted::hosted_runtime_profile().default_identity_profile
    }
    #[cfg(not(feature = "fips"))]
    {
        Algo::Blake3
    }
}

fn run_vcs(action: VcsCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        VcsCmd::Branch {
            store,
            workspace,
            branch,
        } => {
            let client = remote::open_store_client(&store)?;
            client.vcs_branch(keys, &workspace, &branch)
        }
        VcsCmd::Commit {
            store,
            workspace,
            message,
            author,
        } => {
            let client = remote::open_store_client(&store)?;
            let commit = client.vcs_commit(keys, &workspace, &author, &message)?;
            println!("{commit}");
            Ok(())
        }
        VcsCmd::Checkout {
            store,
            workspace,
            branch,
        } => {
            let client = remote::open_store_client(&store)?;
            client.vcs_checkout(keys, &workspace, &branch)
        }
        VcsCmd::Diff {
            store,
            workspace,
            from,
            to,
            format,
            out,
        } => {
            let client = remote::open_store_client(&store)?;
            let bytes = client.vcs_diff(keys, &workspace, &from, &to)?;
            match format.as_str() {
                "cbor" => write_output(out.as_deref(), &bytes).map_err(|e| e.to_string()),
                "text" => {
                    let text = render_structural_diff_text(&bytes)?;
                    write_output(out.as_deref(), text.as_bytes()).map_err(|e| e.to_string())
                }
                other => Err(format!(
                    "unknown diff format {other:?} (expected text or cbor)"
                )),
            }
        }
        VcsCmd::Log { store, workspace } => {
            let client = remote::open_store_client(&store)?;
            for commit in client.vcs_log(keys, &workspace)? {
                println!("{commit}");
            }
            Ok(())
        }
        VcsCmd::Merge {
            store,
            workspace,
            from,
            cells,
            author,
        } => {
            let client = remote::open_store_client(&store)?;
            let outcome = client.vcs_merge(keys, &workspace, &from, &author, cells)?;
            // A conflicting merge changed nothing; report it as a failure with the unresolved paths.
            if let MergeOutcome::Conflicts(paths) = &outcome {
                return Err(format!("merge conflicts: {}", paths.join(", ")));
            }
            match outcome {
                MergeOutcome::UpToDate => println!("already up to date"),
                MergeOutcome::FastForward(c) => println!("fast-forward to {c}"),
                MergeOutcome::Merged(c) => println!("merged as {c}"),
                MergeOutcome::Conflicts(_) => unreachable!("handled above"),
            }
            Ok(())
        }
    }
}

fn run_sql_cmd(action: SqlCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        SqlCmd::Exec {
            store,
            workspace,
            sql,
            db,
        } => {
            let mut loom = cli_open_loom(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            loom.registry_mut()
                .add_facet(ns, FacetKind::Sql)
                .map_err(|e| e.to_string())?;
            // Build the SQL store over a lock-free read snapshot - the lazy base, which
            // streams durable rows on demand; the write loom is used only to persist any mutations. An
            // absent catalog yields an empty store (first use).
            let read = cli_open_loom_read(&store, keys)?;
            let state = LoomSqlStore::open_write(read, ns, &db).map_err(|e| e.to_string())?;
            let mut glue = Glue::new(state);
            let payloads = block_on(glue.execute(&sql)).map_err(|e| e.to_string())?;
            if glue.storage.is_dirty() {
                glue.storage
                    .persist(&mut loom, ns, &db)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())?;
            }
            for payload in &payloads {
                print_payload(payload);
            }
            Ok(())
        }
        SqlCmd::Table { action } => run_table(action, keys),
    }
}

#[cfg(test)]
mod root_help_tests {
    use super::*;

    #[test]
    fn every_visible_root_command_has_a_section() {
        let command = cli_command_for_test();
        let sectioned = COMMAND_SECTIONS
            .iter()
            .flat_map(|(_, names)| names.iter().copied())
            .collect::<std::collections::BTreeSet<_>>();
        let unsectioned = command
            .get_subcommands()
            .filter(|subcommand| !subcommand.is_hide_set())
            .map(|subcommand| subcommand.get_name())
            .filter(|name| !sectioned.contains(name))
            .collect::<Vec<_>>();

        assert!(
            unsectioned.is_empty(),
            "unsectioned commands: {unsectioned:?}"
        );
    }

    #[test]
    fn llms_command_sections_are_alphabetized() {
        let names = visible_subcommand_names(&cli_command_for_test());

        assert!(names.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn capability_json_hides_target_rows_by_default() {
        let set = loom_core::capability::registry();
        let rendered = set.to_json(loom_core::CapabilityVisibility::Default);
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        let records = value["records"].as_array().unwrap();
        assert!(!records.is_empty());
        assert!(
            records
                .iter()
                .all(|record| record["operational_state"] != "target")
        );
        assert!(
            records
                .iter()
                .all(|record| record["proof_status"] != "target")
        );
        assert!(
            records
                .iter()
                .all(|record| record["capability_id"] != "acl")
        );
        assert!(
            records
                .iter()
                .all(|record| record.get("dimensions").is_some())
        );
    }

    #[test]
    fn capability_json_all_includes_target_rows() {
        let set = loom_core::capability::registry();
        let rendered = set.to_json(loom_core::CapabilityVisibility::Detailed);
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        let records = value["records"].as_array().unwrap();
        assert!(
            records
                .iter()
                .any(|record| record["operational_state"] == "target")
        );
        assert!(
            records
                .iter()
                .any(|record| record["capability_id"] == "acl")
        );
    }

    #[test]
    fn doctor_subcommands_are_top_level_only() {
        let inference = cli_try_parse_for_test(["loom", "doctor", "inference"]).unwrap();
        match inference.command.unwrap() {
            Command::Doctor {
                action: DoctorCmd::Inference { format, .. },
            } => assert_eq!(format, "text"),
            _ => panic!("expected doctor inference command"),
        }
        let instance = cli_try_parse_for_test([
            "loom",
            "doctor",
            "inference-instance",
            "store.loom",
            "main",
            "embed",
            "--format",
            "json",
        ])
        .unwrap();
        match instance.command.unwrap() {
            Command::Doctor {
                action:
                    DoctorCmd::InferenceInstance {
                        store,
                        workspace,
                        name,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(name, "embed");
                assert_eq!(format, "json");
            }
            _ => panic!("expected doctor inference-instance command"),
        }
        assert!(cli_try_parse_for_test(["loom", "daemon", "doctor", "store.loom"]).is_err());
        assert!(cli_try_parse_for_test(["loom", "inference", "doctor"]).is_err());
        assert!(
            cli_try_parse_for_test(["loom", "inference", "model", "doctor", "bge-small"]).is_err()
        );
        assert!(
            cli_try_parse_for_test(["loom", "inference", "instance", "doctor", "store.loom"])
                .is_err()
        );
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod cli_parse_tests {
    use super::*;
    use loom_substrate::drive::DriveOperationRecord;
    use loom_substrate::lifecycle::LifecycleOperationRecord;
    use loom_substrate::pages::PageOperationRecord;
    use loom_substrate::{ActorKind, OperationEnvelopeInput};

    fn temp_store(tag: &str) -> String {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-cli-{tag}-{}-{}.loom",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&path);
        path.to_string_lossy().into_owned()
    }

    fn digest(label: &[u8]) -> Digest {
        Digest::hash(Algo::Blake3, label)
    }

    fn test_envelope(
        scope_id: &str,
        operation_id: &str,
        operation_kind: &str,
        sequence: u64,
        target_entity_id: Option<&str>,
        timestamp_ms: u64,
    ) -> Vec<u8> {
        OperationEnvelope::new(
            Algo::Blake3,
            OperationEnvelopeInput {
                workspace_id: scope_id,
                app_id: "studio-test",
                scope_id,
                operation_id,
                operation_kind,
                sequence,
                actor_principal: WorkspaceId::from_bytes([99; 16]),
                actor_kind: ActorKind::User,
                timestamp_ms,
                idempotency_key: operation_id,
                base_root: digest(b"base-root"),
                base_entity_version: None,
                target_entity_id,
                payload: operation_id.as_bytes(),
                policy_labels: &[],
                signature: None,
                agent: None,
            },
        )
        .unwrap()
        .encode()
        .unwrap()
    }

    fn sample_meetings_snapshot(workspace: WorkspaceId) -> MeetingsProfileSnapshot {
        let mut source = SourceRecord::new(SourceRecordInput {
            source_id: "src-1",
            source_system: "granola-api",
            external_id: "not_1",
            source_digest: digest(b"source"),
            observed_at_ms: 100,
            access_scope: "personal-notes",
            coverage: MeetingsCoverage::Partial,
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
        let mut annotation = loom_substrate::meetings::AnnotationRecord::new(
            "ann-1",
            "meet-1",
            vec!["span-1".to_string()],
            "Decision",
            "Use normalized import snapshots",
            130,
        )
        .unwrap();
        annotation.status = loom_substrate::meetings::AnnotationStatus::Accepted;
        annotation.accepted_by = Some("principal-1".to_string());
        annotation.accepted_at_ms = Some(140);
        MeetingsProfileSnapshot::new(
            workspace.to_string(),
            MeetingsProfileSnapshotParts {
                sources: vec![source],
                meetings: vec![meeting],
                spans: vec![span],
                annotations: vec![annotation],
                vocabulary_terms: Vec::new(),
                entity_merges: Vec::new(),
                promotions: Vec::new(),
                import_runs: Vec::new(),
                redactions: Vec::new(),
            },
        )
        .unwrap()
    }

    struct FixedEmbedding;

    impl loom_inference::TextEmbedding for FixedEmbedding {
        fn model_id(&self) -> &str {
            "test-embedding"
        }

        fn dimension(&self) -> usize {
            3
        }

        fn embed(&self, texts: &[String]) -> loom_types::Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|text| {
                    let len = text.len() as f32;
                    [len, len / 2.0, 1.0].to_vec()
                })
                .collect())
        }
    }

    #[test]
    fn search_top_level_is_distinct_from_fts() {
        let command = cli_command_for_test();
        assert!(command.find_subcommand("fts").is_some());
        assert!(command.find_subcommand("search").is_some());
        let cli = cli_try_parse_for_test([
            "loom",
            "search",
            "store.loom",
            "loom",
            "--workspace",
            "main",
            "--collection",
            "docs",
            "--field",
            "body",
            "--limit",
            "10",
            "--format",
            "json",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Search {
                store,
                query,
                workspace,
                collection,
                field,
                limit,
                format,
                ..
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(query, "loom");
                assert_eq!(workspace.as_deref(), Some("main"));
                assert_eq!(collection.as_deref(), Some("docs"));
                assert_eq!(field.as_deref(), Some("body"));
                assert_eq!(limit, 10);
                assert_eq!(format, "json");
            }
            _ => panic!("expected search command"),
        }
    }

    #[test]
    fn search_snippet_respects_utf8_boundaries() {
        let text = "alpha cafe\u{301} loom beta";
        let start = text.find("loom").unwrap();
        let snippet = snippet_text(text, start, start + "loom".len());
        assert_eq!(snippet, text);
    }

    #[test]
    fn metrics_commands_expose_raw_cbor_projection() {
        let command = cli_command_for_test();
        assert!(command.find_subcommand("metrics").is_some());

        let put = cli_try_parse_for_test([
            "loom",
            "metrics",
            "put-descriptor",
            "store.loom",
            "ops",
            "--input",
            "descriptor.cbor",
        ])
        .unwrap();
        match put.command.unwrap() {
            Command::Metrics {
                action:
                    MetricsCmd::PutDescriptor {
                        store,
                        workspace,
                        input,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "ops");
                assert_eq!(input, "descriptor.cbor");
            }
            _ => panic!("expected metrics put-descriptor command"),
        }

        let query = cli_try_parse_for_test([
            "loom",
            "metrics",
            "query",
            "store.loom",
            "ops",
            "requests",
            "--from",
            "10",
            "--to",
            "20",
            "--max-series",
            "3",
            "--max-groups",
            "4",
            "--max-samples",
            "5",
            "--max-output-bytes",
            "1000",
            "--now",
            "30",
            "--out",
            "result.cbor",
        ])
        .unwrap();
        match query.command.unwrap() {
            Command::Metrics {
                action:
                    MetricsCmd::Query {
                        store,
                        workspace,
                        descriptor,
                        from,
                        to,
                        max_series,
                        max_groups,
                        max_samples,
                        max_output_bytes,
                        now,
                        out,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "ops");
                assert_eq!(descriptor, "requests");
                assert_eq!(from, 10);
                assert_eq!(to, 20);
                assert_eq!(max_series, 3);
                assert_eq!(max_groups, 4);
                assert_eq!(max_samples, 5);
                assert_eq!(max_output_bytes, 1000);
                assert_eq!(now, 30);
                assert_eq!(out.as_deref(), Some("result.cbor"));
            }
            _ => panic!("expected metrics query command"),
        }
    }

    #[test]
    fn program_commands_expose_local_lifecycle_projection() {
        let command = cli_command_for_test();
        assert!(command.find_subcommand("program").is_some());

        let put = cli_try_parse_for_test([
            "loom",
            "program",
            "put-template",
            "store.loom",
            "programs",
            "page-card",
            "--input",
            "template.json",
            "--out",
            "record.cbor",
        ])
        .unwrap();
        match put.command.unwrap() {
            Command::Program {
                action:
                    ProgramCmd::PutTemplate {
                        store,
                        workspace,
                        name,
                        input,
                        out,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "programs");
                assert_eq!(name, "page-card");
                assert_eq!(input, "template.json");
                assert_eq!(out.as_deref(), Some("record.cbor"));
            }
            _ => panic!("expected program put-template command"),
        }

        let get = cli_try_parse_for_test([
            "loom",
            "program",
            "get",
            "store.loom",
            "programs",
            "page-card",
            "--out",
            "body.out",
        ])
        .unwrap();
        match get.command.unwrap() {
            Command::Program {
                action:
                    ProgramCmd::Get {
                        store,
                        workspace,
                        name,
                        out,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "programs");
                assert_eq!(name, "page-card");
                assert_eq!(out.as_deref(), Some("body.out"));
            }
            _ => panic!("expected program get command"),
        }
    }

    #[test]
    fn program_commands_round_trip_all_engine_types() {
        let store = temp_store("program-cli");
        let dir = std::env::temp_dir().join(format!(
            "loom-program-cli-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let wasm = dir.join("program.wasm");
        let template = dir.join("program.template");
        let cel = dir.join("program.cel");
        let wasm_body = dir.join("wasm.body");
        let list_out = dir.join("programs.cbor");
        std::fs::write(&wasm, b"\0asm").unwrap();
        std::fs::write(&template, br#"{"outputs":{"html":"ready"}}"#).unwrap();
        std::fs::write(&cel, b"request.amount < 100").unwrap();

        run(
            Command::Program {
                action: ProgramCmd::PutWasm {
                    store: store.clone(),
                    workspace: "programs".to_string(),
                    name: "wasm-file-writer".to_string(),
                    input: wasm.to_string_lossy().into_owned(),
                    out: Some(dir.join("wasm.cbor").to_string_lossy().into_owned()),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Program {
                action: ProgramCmd::PutTemplate {
                    store: store.clone(),
                    workspace: "programs".to_string(),
                    name: "template-card".to_string(),
                    input: template.to_string_lossy().into_owned(),
                    out: Some(dir.join("template.cbor").to_string_lossy().into_owned()),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Program {
                action: ProgramCmd::PutCel {
                    store: store.clone(),
                    workspace: "programs".to_string(),
                    name: "cel-threshold".to_string(),
                    input: cel.to_string_lossy().into_owned(),
                    out: Some(dir.join("cel.cbor").to_string_lossy().into_owned()),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Program {
                action: ProgramCmd::Get {
                    store: store.clone(),
                    workspace: "programs".to_string(),
                    name: "wasm-file-writer".to_string(),
                    out: Some(wasm_body.to_string_lossy().into_owned()),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        assert_eq!(std::fs::read(&wasm_body).unwrap(), b"\0asm");

        run(
            Command::Program {
                action: ProgramCmd::List {
                    store: store.clone(),
                    workspace: "programs".to_string(),
                    out: Some(list_out.to_string_lossy().into_owned()),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        let listed = loom_codec::decode(&std::fs::read(&list_out).unwrap()).unwrap();
        let WireValue::Array(records) = listed else {
            panic!("program list must be an array");
        };
        let names = records
            .into_iter()
            .map(|record| {
                let WireValue::Map(fields) = record else {
                    panic!("program record must be a map");
                };
                fields
                    .into_iter()
                    .find_map(|(key, value)| match (key, value) {
                        (WireValue::Text(key), WireValue::Text(value)) if key == "name" => {
                            Some(value)
                        }
                        _ => None,
                    })
                    .expect("program record name")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec!["cel-threshold", "template-card", "wasm-file-writer"]
        );

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "programs").unwrap();
        assert_eq!(
            loom_compute::program_inspect(&loom, ns, "wasm-file-writer")
                .unwrap()
                .unwrap()
                .manifest
                .engine,
            "wasm"
        );
        assert_eq!(
            loom_compute::program_inspect(&loom, ns, "template-card")
                .unwrap()
                .unwrap()
                .manifest
                .engine,
            "template"
        );
        assert_eq!(
            loom_compute::program_inspect(&loom, ns, "cel-threshold")
                .unwrap()
                .unwrap()
                .manifest
                .engine,
            "cel"
        );
    }

    #[test]
    fn fts_status_requires_engine_version_and_formats_json() {
        let cli = cli_try_parse_for_test([
            "loom",
            "fts",
            "status",
            "store.loom",
            "docs",
            "--workspace",
            "main",
            "--engine-version",
            "tantivy-test",
            "--format",
            "json",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Fts {
                action:
                    SearchCmd::Status {
                        store,
                        workspace,
                        name,
                        engine_version,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(name, "docs");
                assert_eq!(engine_version, "tantivy-test");
                assert_eq!(format, "json");
            }
            _ => panic!("expected fts status command"),
        }
        assert!(cli_try_parse_for_test(["loom", "fts", "status", "store.loom", "docs"]).is_err());

        let json = search_status_json(
            &WorkspaceId::from_bytes([7; 16]).to_string(),
            "docs",
            Digest::blake3(b"source"),
            "tantivy-test",
            &DerivedArtifactStatus::Missing,
        );
        assert!(json.contains("\"collection\":\"docs\""));
        assert!(json.contains("\"engine_version\":\"tantivy-test\""));
        assert!(json.contains("\"status\":\"missing\""));
    }

    #[test]
    fn fts_rebuild_accepts_optional_engine_version_and_json_format() {
        let cli = cli_try_parse_for_test([
            "loom",
            "fts",
            "rebuild",
            "store.loom",
            "docs",
            "--workspace",
            "main",
            "--engine-version",
            "tantivy-test",
            "--format",
            "json",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Fts {
                action:
                    SearchCmd::Rebuild {
                        store,
                        workspace,
                        name,
                        engine_version,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(name, "docs");
                assert_eq!(engine_version.as_deref(), Some("tantivy-test"));
                assert_eq!(format, "json");
            }
            _ => panic!("expected fts rebuild command"),
        }
    }

    #[test]
    fn doctor_all_accepts_optional_store() {
        let cli = cli_try_parse_for_test(["loom", "doctor", "all", "store.loom"]).unwrap();
        match cli.command.unwrap() {
            Command::Doctor {
                action: DoctorCmd::All { store, format, .. },
            } => {
                assert_eq!(store.as_deref(), Some("store.loom"));
                assert_eq!(format, "text");
            }
            _ => panic!("expected doctor command"),
        }
        assert!(cli_try_parse_for_test(["loom", "doctor", "store.loom"]).is_err());
    }

    #[test]
    fn root_help_documents_locator_semantics() {
        let long = cli_command_for_test()
            .get_long_about()
            .map(|s| s.to_string())
            .unwrap_or_default();
        assert!(
            long.contains("STORE forms"),
            "help must explain STORE forms"
        );
        assert!(long.contains("context"), "help must explain contexts");
        assert!(long.contains("--project"), "help must mention --project");
        assert!(
            long.contains("fail fast"),
            "help must mention remote fail-fast"
        );
        assert!(
            long.contains("--stateless"),
            "help must note local-only --stateless"
        );
    }

    #[test]
    fn project_flag_parses_before_and_after_subcommand() {
        let after = cli_try_parse_for_test([
            "loom",
            "doctor",
            "store",
            "store.loom",
            "--project",
            "/tmp/p",
        ])
        .unwrap();
        assert_eq!(after.project, Some(PathBuf::from("/tmp/p")));
        let before = cli_try_parse_for_test([
            "loom",
            "--project",
            "/tmp/p",
            "doctor",
            "store",
            "store.loom",
        ])
        .unwrap();
        assert_eq!(before.project, Some(PathBuf::from("/tmp/p")));
    }

    #[test]
    fn config_flag_is_repeatable_in_command_line_order() {
        let cli = cli_try_parse_for_test([
            "loom",
            "doctor",
            "store",
            "store.loom",
            "--config",
            "a.toml",
            "--config",
            "b.toml",
        ])
        .unwrap();
        assert_eq!(
            cli.config,
            vec![PathBuf::from("a.toml"), PathBuf::from("b.toml")]
        );
    }

    #[cfg(feature = "mcp")]
    #[test]
    fn mcp_accepts_project_after_subcommand() {
        let cli = cli_try_parse_for_test(["loom", "mcp", "prod", "--project", "/tmp/p"]).unwrap();
        assert_eq!(cli.project, Some(PathBuf::from("/tmp/p")));
        match cli.command.unwrap() {
            Command::Mcp { store, .. } => assert_eq!(store, "prod"),
            _ => panic!("expected mcp command"),
        }
    }

    #[test]
    fn doctor_all_accepts_default_hardware_and_inference_without_store() {
        let cli = cli_try_parse_for_test(["loom", "doctor", "all", "--format", "json"]).unwrap();
        match cli.command.unwrap() {
            Command::Doctor {
                action: DoctorCmd::All { store, format, .. },
            } => {
                assert!(store.is_none());
                assert_eq!(format, "json");
            }
            _ => panic!("expected doctor command"),
        }
        assert!(cli_try_parse_for_test(["loom", "doctor", "--inference"]).is_err());
        assert!(cli_try_parse_for_test(["loom", "doctor", "--hardware"]).is_err());
    }

    #[test]
    fn mlx_bundle_doctor_line_reports_status_and_abi() {
        let inspection = loom_inference::MlxBundleInspection {
            layout: loom_inference::MlxBundleLayout::new("/tmp/loom-mlx-test"),
            status: loom_inference::MlxBundleStatus::MissingAdapterLibrary,
            files: vec![loom_inference::MlxBundleFile {
                name: loom_inference::MLX_C_LIBRARY.to_string(),
                path: PathBuf::from("/tmp/loom-mlx-test/libmlxc.dylib"),
                size_bytes: 12,
            }],
            abi: loom_inference::MlxAdapterAbi::current(),
        };

        let line = mlx_bundle_doctor_line(&inspection);

        assert!(line.contains("mlx_bundle\tstatus=missing-adapter-library"));
        assert!(line.contains("\tabi=1\t"));
        assert!(line.contains("adapter=libloom_mlx_adapter.dylib"));
        assert!(line.contains("files=libmlxc.dylib"));
    }

    #[test]
    fn llama_cpp_bundle_doctor_line_reports_status_and_abi() {
        let inspection = loom_inference::LlamaCppBundleInspection {
            layout: loom_inference::LlamaCppBundleLayout::new("/tmp/loom-llama-cpp-test"),
            status: loom_inference::LlamaCppBundleStatus::MissingAdapterLibrary,
            files: vec![loom_inference::LlamaCppBundleFile {
                name: "libllama.dylib".to_string(),
                path: PathBuf::from("/tmp/loom-llama-cpp-test/libllama.dylib"),
                size_bytes: 12,
            }],
            abi: loom_inference::LlamaCppAdapterAbi::current(),
        };

        let line = llama_cpp_bundle_doctor_line(&inspection);

        assert!(line.contains("llama_cpp_bundle\tstatus=missing-adapter-library"));
        assert!(line.contains("\tabi=1\t"));
        assert!(line.contains("adapter="));
        assert!(line.contains("files=libllama.dylib"));
    }

    #[test]
    fn inference_model_list_accepts_remote_json_shape() {
        let cli = cli_try_parse_for_test([
            "loom",
            "inference",
            "model",
            "list",
            "--remote",
            "--kind",
            "text-embedding",
            "--format",
            "json",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Inference {
                action:
                    InferenceCmd::Model {
                        action:
                            InferenceModelCmd::List {
                                remote,
                                kind,
                                format,
                                ..
                            },
                    },
            } => {
                assert!(remote);
                assert_eq!(kind.as_deref(), Some("text-embedding"));
                assert_eq!(format, "json");
            }
            _ => panic!("expected inference model list command"),
        }
    }

    #[test]
    fn inference_model_download_accepts_target_shape() {
        let cli = cli_try_parse_for_test([
            "loom",
            "inference",
            "model",
            "download",
            "sentence-transformers/all-MiniLM-L6-v2",
            "config.json",
            "model.safetensors",
            "--kind",
            "text-embedding",
            "--runtime",
            "candle-safetensors",
            "--foreground",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Inference {
                action:
                    InferenceCmd::Model {
                        action:
                            InferenceModelCmd::Download {
                                repo,
                                files,
                                kind,
                                runtime,
                                foreground,
                                ..
                            },
                    },
            } => {
                assert_eq!(repo, "sentence-transformers/all-MiniLM-L6-v2");
                assert_eq!(files, vec!["config.json", "model.safetensors"]);
                assert_eq!(kind, "text-embedding");
                assert_eq!(runtime, "candle-safetensors");
                assert!(foreground);
            }
            _ => panic!("expected inference model download command"),
        }
    }

    #[test]
    fn inference_download_runs_inline_when_cache_lock_is_free() {
        let root = inference_download_temp_dir("inline-free");
        let manager = DownloadJobManager::new(root.join("hub"));

        assert!(should_run_inference_download_inline(&manager, false).unwrap());
        assert!(should_run_inference_download_inline(&manager, true).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inference_download_reports_busy_when_cache_lock_is_held() {
        let root = inference_download_temp_dir("inline-locked");
        let manager = DownloadJobManager::new(root.join("hub"));
        let lock = manager.acquire_cache_lock().unwrap();

        assert!(!should_run_inference_download_inline(&manager, false).unwrap());
        assert!(should_run_inference_download_inline(&manager, true).unwrap());
        drop(lock);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inference_remove_plans_paths_from_live_cache_discovery() {
        let root = inference_download_temp_dir("remove-plan");
        let cache_dir = root.join("hub");
        let model = write_curated_embedding_cache(&cache_dir);
        let record = loom_inference::discover_installed_model(
            &cache_dir,
            &model,
            RuntimeKind::CandleSafetensors,
        )
        .unwrap()
        .unwrap();

        let paths = planned_inference_remove_paths(&cache_dir, &record).unwrap();

        assert_eq!(paths.len(), record.files.len());
        assert!(paths.iter().all(|path| path.starts_with(&cache_dir)));
        assert!(paths.iter().any(|path| path.ends_with(
            "models--sentence-transformers--all-MiniLM-L6-v2/snapshots/abc123/model.safetensors"
        )));
        std::fs::remove_dir_all(root).unwrap();
    }

    fn inference_download_temp_dir(tag: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-cli-inference-download-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_curated_embedding_cache(cache_dir: &std::path::Path) -> ModelRef {
        let repo_dir = cache_dir.join("models--sentence-transformers--all-MiniLM-L6-v2");
        let snapshot = repo_dir.join("snapshots").join("abc123");
        std::fs::create_dir_all(repo_dir.join("refs")).unwrap();
        std::fs::write(repo_dir.join("refs").join("main"), "abc123\n").unwrap();
        for file in [
            "config.json",
            "model.safetensors",
            "special_tokens_map.json",
            "tokenizer.json",
            "tokenizer_config.json",
        ] {
            let path = snapshot.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, format!("file:{file}")).unwrap();
        }
        ModelRef::new(
            InferenceModelKind::TextEmbedding,
            "sentence-transformers/all-MiniLM-L6-v2",
        )
        .with_revision(RevisionRef::Branch("main".to_string()))
    }

    #[test]
    fn inference_instance_create_accepts_settings() {
        let cli = cli_try_parse_for_test([
            "loom",
            "inference",
            "instance",
            "create",
            "store.loom",
            "main",
            "fast-embed",
            "--model",
            "sentence-transformers/all-MiniLM-L6-v2",
            "--kind",
            "text-embedding",
            "--runtime",
            "candle-safetensors",
            "--preset",
            "fast",
            "--set",
            "batch_size=8",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Inference {
                action:
                    InferenceCmd::Instance {
                        action:
                            InferenceInstanceCmd::Create {
                                store,
                                workspace,
                                name,
                                model,
                                kind,
                                preset,
                                settings,
                                ..
                            },
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(name, "fast-embed");
                assert_eq!(model, "sentence-transformers/all-MiniLM-L6-v2");
                assert_eq!(kind, "text-embedding");
                assert_eq!(preset.as_deref(), Some("fast"));
                assert_eq!(settings, vec!["batch_size=8"]);
            }
            _ => panic!("expected inference instance create command"),
        }
    }

    #[test]
    fn vector_workspace_configure_accepts_embedding_instance() {
        let cli = cli_try_parse_for_test([
            "loom",
            "vector",
            "workspace",
            "configure",
            "store.loom",
            "main",
            "--embedding-instance",
            "fast-embed",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Vector {
                action:
                    VectorCmd::Workspace {
                        action:
                            VectorWorkspaceCmd::Configure {
                                store,
                                workspace,
                                embedding_instance,
                                format,
                            },
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(embedding_instance.as_deref(), Some("fast-embed"));
                assert_eq!(format, "text");
            }
            _ => panic!("expected vector workspace configure command"),
        }
    }

    #[test]
    fn studio_reindex_accepts_profile_and_json_format() {
        let cli = cli_try_parse_for_test([
            "loom",
            "studio",
            "reindex",
            "store.loom",
            "main",
            "--profile",
            "meetings",
            "--format",
            "json",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Studio {
                action:
                    StudioCmd::Reindex {
                        store,
                        workspace,
                        profile,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(profile, "meetings");
                assert_eq!(format, "json");
            }
            _ => panic!("expected studio reindex command"),
        }
    }

    #[test]
    fn studio_surfaces_catalog_accepts_set_and_json_format() {
        let cli = cli_try_parse_for_test([
            "loom", "studio", "surfaces", "catalog", "main", "--set", "core", "--format", "json",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Studio {
                action:
                    StudioCmd::Surfaces {
                        action:
                            StudioSurfacesCmd::Catalog {
                                workspace,
                                set,
                                format,
                            },
                    },
            } => {
                assert_eq!(workspace, "main");
                assert_eq!(set, "core");
                assert_eq!(format, "json");
            }
            _ => panic!("expected studio surfaces catalog command"),
        }
    }

    #[test]
    fn chat_and_drive_profile_commands_parse() {
        let cli = cli_try_parse_for_test([
            "loom",
            "chat",
            "post",
            "store.loom",
            "studio",
            "general",
            "m1",
            "--thread",
            "t1",
            "--input",
            "body.txt",
            "--format",
            "json",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Chat {
                action:
                    ChatCmd::Post {
                        store,
                        workspace,
                        channel,
                        message_id,
                        thread,
                        input,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "studio");
                assert_eq!(channel, "general");
                assert_eq!(message_id, "m1");
                assert_eq!(thread.as_deref(), Some("t1"));
                assert_eq!(input, "body.txt");
                assert_eq!(format, "json");
            }
            _ => panic!("expected chat post command"),
        }

        let cli = cli_try_parse_for_test([
            "loom",
            "chat",
            "create-channel",
            "store.loom",
            "studio",
            "general",
            "General",
            "--channel-id",
            "11111111-1111-4111-8111-111111111111",
            "--format",
            "json",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Chat {
                action:
                    ChatCmd::CreateChannel {
                        store,
                        workspace,
                        handle,
                        name,
                        channel_id,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "studio");
                assert_eq!(handle, "general");
                assert_eq!(name, "General");
                assert_eq!(
                    channel_id.as_deref(),
                    Some("11111111-1111-4111-8111-111111111111")
                );
                assert_eq!(format, "json");
            }
            _ => panic!("expected chat create-channel command"),
        }

        let cli = cli_try_parse_for_test([
            "loom",
            "chat",
            "invoke-agent",
            "store.loom",
            "studio",
            "general",
            "inv-1",
            "22222222-2222-4222-8222-222222222222",
            "--source-message-ids",
            "m1,m2",
            "--input",
            "prompt.txt",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Chat {
                action:
                    ChatCmd::InvokeAgent {
                        store,
                        workspace,
                        channel,
                        invocation_id,
                        agent_principal,
                        source_message_ids,
                        input,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "studio");
                assert_eq!(channel, "general");
                assert_eq!(invocation_id, "inv-1");
                assert_eq!(agent_principal, "22222222-2222-4222-8222-222222222222");
                assert_eq!(source_message_ids, vec!["m1", "m2"]);
                assert_eq!(input, "prompt.txt");
                assert_eq!(format, "text");
            }
            _ => panic!("expected chat invoke-agent command"),
        }

        let cli = cli_try_parse_for_test([
            "loom",
            "chat",
            "update-cursor",
            "store.loom",
            "studio",
            "general",
            "42",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Chat {
                action:
                    ChatCmd::UpdateCursor {
                        store,
                        workspace,
                        channel,
                        next_sequence,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "studio");
                assert_eq!(channel, "general");
                assert_eq!(next_sequence, 42);
                assert_eq!(format, "text");
            }
            _ => panic!("expected chat update-cursor command"),
        }

        let cli = cli_try_parse_for_test([
            "loom",
            "chat",
            "add-reaction",
            "store.loom",
            "studio",
            "general",
            "m1",
            "approved",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Chat {
                action:
                    ChatCmd::AddReaction {
                        store,
                        workspace,
                        channel,
                        message_id,
                        kind,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "studio");
                assert_eq!(channel, "general");
                assert_eq!(message_id, "m1");
                assert_eq!(kind, "approved");
                assert_eq!(format, "text");
            }
            _ => panic!("expected chat add-reaction command"),
        }

        let cli = cli_try_parse_for_test([
            "loom",
            "chat",
            "emoji-register",
            "store.loom",
            "studio",
            "ship",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Chat {
                action:
                    ChatCmd::EmojiRegister {
                        store,
                        workspace,
                        kind,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "studio");
                assert_eq!(kind, "ship");
                assert_eq!(format, "text");
            }
            _ => panic!("expected chat emoji-register command"),
        }

        let cli = cli_try_parse_for_test([
            "loom",
            "drive",
            "stat",
            "store.loom",
            "studio",
            "root",
            "plan.md",
            "--format",
            "json",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Drive {
                action:
                    DriveCmd::Stat {
                        store,
                        workspace,
                        folder_id,
                        name,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "studio");
                assert_eq!(folder_id, "root");
                assert_eq!(name, "plan.md");
                assert_eq!(format, "json");
            }
            _ => panic!("expected drive stat command"),
        }

        let cli = cli_try_parse_for_test([
            "loom",
            "drive",
            "create-upload",
            "store.loom",
            "studio",
            "upload-1",
            "root",
            "plan.md",
            "file-1",
            "b3:root",
            "--created-at-ms",
            "100",
            "--replace-file",
            "--format",
            "json",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Drive {
                action:
                    DriveCmd::CreateUpload {
                        store,
                        workspace,
                        upload_id,
                        parent_folder_id,
                        name,
                        file_id,
                        expected_root,
                        created_at_ms,
                        replace_file,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "studio");
                assert_eq!(upload_id, "upload-1");
                assert_eq!(parent_folder_id, "root");
                assert_eq!(name, "plan.md");
                assert_eq!(file_id, "file-1");
                assert_eq!(expected_root, "b3:root");
                assert_eq!(created_at_ms, 100);
                assert!(replace_file);
                assert_eq!(format, "json");
            }
            _ => panic!("expected drive create-upload command"),
        }

        let cli = cli_try_parse_for_test([
            "loom",
            "drive",
            "resolve-conflict",
            "store.loom",
            "studio",
            "conflict-1",
            "keep-both",
            "--format",
            "json",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Drive {
                action:
                    DriveCmd::ResolveConflict {
                        store,
                        workspace,
                        conflict_id,
                        resolution,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "studio");
                assert_eq!(conflict_id, "conflict-1");
                assert_eq!(resolution, "keep-both");
                assert_eq!(format, "json");
            }
            _ => panic!("expected drive resolve-conflict command"),
        }
    }

    #[test]
    fn studio_revisions_rebuild_accepts_profile_dry_run_and_json_format() {
        let cli = cli_try_parse_for_test([
            "loom",
            "studio",
            "revisions",
            "rebuild",
            "store.loom",
            "main",
            "--profile",
            "meetings",
            "--dry-run",
            "--format",
            "json",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Command::Studio {
                action:
                    StudioCmd::Revisions {
                        action:
                            StudioRevisionsCmd::Rebuild {
                                store,
                                workspace,
                                profile,
                                dry_run,
                                format,
                            },
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(profile, "meetings");
                assert!(dry_run);
                assert_eq!(format, "json");
            }
            _ => panic!("expected studio revisions rebuild command"),
        }
    }

    #[test]
    fn studio_revisions_rebuild_backfills_meetings_index() {
        let store = temp_store("studio-revisions-rebuild");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        init_control_state(&fs).unwrap();
        let mut loom = open_loom_from(fs, &KeyOpts::default(), false).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Vector,
                Some("main"),
                WorkspaceId::from_bytes([42; 16]),
            )
            .unwrap();
        let snapshot = sample_meetings_snapshot(ns);
        let profile_id = ns.to_string();
        loom.store()
            .control_set(
                &meetings_profile_key(&profile_id).unwrap(),
                snapshot.encode().unwrap(),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        run_studio(
            StudioCmd::Revisions {
                action: StudioRevisionsCmd::Rebuild {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    profile: "meetings".to_string(),
                    dry_run: false,
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let loom = loom_store::open_loom_read(&store).unwrap();
        let history_path = revision_index_path(&profile_id).unwrap();
        let history =
            RevisionIndex::decode(&loom.read_file_reserved(ns, &history_path).unwrap()).unwrap();
        let revisions = history.history("meeting:meet-1");
        assert_eq!(revisions.len(), 1);
        assert_eq!(revisions[0].revision, 1);
        assert_eq!(
            revisions[0].body.media_type,
            "application/vnd.uldren.loom.meetings.meeting+cbor"
        );
        assert_eq!(history.checkpoints().len(), 1);
        drop(loom);

        run_studio(
            StudioCmd::Revisions {
                action: StudioRevisionsCmd::Rebuild {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    profile: "meetings".to_string(),
                    dry_run: false,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let loom = loom_store::open_loom_read(&store).unwrap();
        let history =
            RevisionIndex::decode(&loom.read_file_reserved(ns, &history_path).unwrap()).unwrap();
        assert_eq!(history.history("meeting:meet-1").len(), 1);
        assert_eq!(history.checkpoints().len(), 1);
    }

    #[test]
    fn studio_revisions_rebuild_backfills_drive_index() {
        let store = temp_store("studio-revisions-rebuild-drive");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        init_control_state(&fs).unwrap();
        let mut loom = open_loom_from(fs, &KeyOpts::default(), false).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Vector,
                Some("main"),
                WorkspaceId::from_bytes([43; 16]),
            )
            .unwrap();
        let profile_id = ns.to_string();
        let envelope = test_envelope(
            &profile_id,
            "drive-op-1",
            "file.renamed",
            1,
            Some("file-1"),
            180,
        );
        let log = DriveOperationLog::new(
            &profile_id,
            vec![
                DriveOperationRecord::new(
                    1,
                    "drive-op-1",
                    "file.renamed",
                    Some("file-1".to_string()),
                    digest(b"drive-root"),
                    envelope,
                )
                .unwrap(),
            ],
        )
        .unwrap();
        loom.store()
            .control_set(
                &drive_operation_log_key(&profile_id).unwrap(),
                log.encode().unwrap(),
            )
            .unwrap();

        let report = rebuild_studio_revision_index(&mut loom, ns, "drive", false).unwrap();

        assert_eq!(report.candidates, 1);
        assert_eq!(report.inserted, 1);
        let history_path = revision_index_path(&profile_id).unwrap();
        let history =
            RevisionIndex::decode(&loom.read_file_reserved(ns, &history_path).unwrap()).unwrap();
        let revisions = history.history("drive:metadata:file-1");
        assert_eq!(revisions.len(), 1);
        assert_eq!(revisions[0].revision, 1);
        assert_eq!(
            revisions[0].body.media_type,
            "application/vnd.uldren.loom.drive.operation+cbor"
        );
    }

    #[test]
    fn studio_revisions_rebuild_backfills_pages_index() {
        let store = temp_store("studio-revisions-rebuild-pages");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        init_control_state(&fs).unwrap();
        let mut loom = open_loom_from(fs, &KeyOpts::default(), false).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Vector,
                Some("main"),
                WorkspaceId::from_bytes([44; 16]),
            )
            .unwrap();
        let profile_id = ns.to_string();
        let envelope = test_envelope(
            &profile_id,
            "page-op-1",
            "structure.node_bound",
            1,
            Some("node-1"),
            190,
        );
        let log = PageOperationLog::new(
            &profile_id,
            vec![
                PageOperationRecord::new(
                    1,
                    "page-op-1",
                    "structure.node_bound",
                    Some("node-1".to_string()),
                    digest(b"pages-root"),
                    envelope,
                )
                .unwrap(),
            ],
        )
        .unwrap();
        loom.store()
            .control_set(
                &page_profile_operation_log_key(&profile_id).unwrap(),
                log.encode().unwrap(),
            )
            .unwrap();

        let report = rebuild_studio_revision_index(&mut loom, ns, "pages", false).unwrap();

        assert_eq!(report.candidates, 1);
        assert_eq!(report.inserted, 1);
        let history_path = revision_index_path(&profile_id).unwrap();
        let history =
            RevisionIndex::decode(&loom.read_file_reserved(ns, &history_path).unwrap()).unwrap();
        let revisions = history.history("structure-node:node-1");
        assert_eq!(revisions.len(), 1);
        assert_eq!(revisions[0].revision, 1);
        assert_eq!(
            revisions[0].body.media_type,
            "application/vnd.uldren.loom.pages.operation+cbor"
        );
    }

    #[test]
    fn studio_revisions_rebuild_backfills_lifecycle_index() {
        let store = temp_store("studio-revisions-rebuild-lifecycle");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        init_control_state(&fs).unwrap();
        let mut loom = open_loom_from(fs, &KeyOpts::default(), false).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Vector,
                Some("main"),
                WorkspaceId::from_bytes([45; 16]),
            )
            .unwrap();
        let profile_id = ns.to_string();
        let envelope = test_envelope(
            &profile_id,
            "lifecycle-op-1",
            "lifecycle.transitioned",
            1,
            Some("lifecycle:inst-1"),
            200,
        );
        let log = LifecycleOperationLog::new(
            &profile_id,
            vec![
                LifecycleOperationRecord::new(
                    1,
                    "lifecycle-op-1",
                    "lifecycle.transitioned",
                    "inst-1",
                    Some("lifecycle:inst-1".to_string()),
                    digest(b"lifecycle-root"),
                    envelope,
                )
                .unwrap(),
            ],
        )
        .unwrap();
        loom.store()
            .control_set(
                &lifecycle_operation_log_key(&profile_id).unwrap(),
                log.encode().unwrap(),
            )
            .unwrap();

        let report = rebuild_studio_revision_index(&mut loom, ns, "lifecycle", false).unwrap();

        assert_eq!(report.candidates, 1);
        assert_eq!(report.inserted, 1);
        let history_path = revision_index_path(&profile_id).unwrap();
        let history =
            RevisionIndex::decode(&loom.read_file_reserved(ns, &history_path).unwrap()).unwrap();
        let revisions = history.history("lifecycle:instance:inst-1");
        assert_eq!(revisions.len(), 1);
        assert_eq!(revisions[0].revision, 1);
        assert_eq!(
            revisions[0].body.media_type,
            "application/vnd.uldren.loom.lifecycle.operation+cbor"
        );
    }

    #[test]
    fn studio_reindex_enqueue_persists_no_engine_job() {
        let store = temp_store("studio-reindex-no-engine");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        init_control_state(&fs).unwrap();
        let mut loom = open_loom_from(fs, &KeyOpts::default(), false).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Vector,
                Some("main"),
                WorkspaceId::from_bytes([31; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let result =
            enqueue_studio_reindex(&store, "main", "meetings", None, &KeyOpts::default()).unwrap();
        assert_eq!(result.workspace_id, ns);
        assert_eq!(result.state, "no_engine");

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let job_bytes = loom.read_file_reserved(ns, &result.job_path).unwrap();
        let job = EmbeddingProjectionJob::decode(&job_bytes).unwrap();
        assert_eq!(job.state.as_str(), "no_engine");
        assert_eq!(job.key.facet, "studio");
        assert_eq!(job.key.collection, "meetings");
    }

    #[test]
    fn studio_reindex_drains_meetings_vectors_with_bound_instance() {
        let store = temp_store("studio-reindex-meetings-vector");
        let model = ModelRef::new(InferenceModelKind::TextEmbedding, "test-embedding")
            .with_revision(RevisionRef::Branch("main".to_string()));
        let resolved = ResolvedTextEmbeddingInstance {
            instance: loom_inference::build_instance_descriptor(
                "fixed-embed",
                InferenceModelKind::TextEmbedding,
                model,
                RuntimeKind::CandleSafetensors,
                None,
                BTreeMap::new(),
            )
            .unwrap(),
            handle: loom_inference::TextEmbeddingHandle::with_provider(Box::new(FixedEmbedding)),
        };
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        init_control_state(&fs).unwrap();
        let mut loom = open_loom_from(fs, &KeyOpts::default(), false).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Vector,
                Some("main"),
                WorkspaceId::from_bytes([41; 16]),
            )
            .unwrap();
        let snapshot = sample_meetings_snapshot(ns);
        let profile_id = ns.to_string();
        loom.store()
            .control_set(
                &meetings_profile_key(&profile_id).unwrap(),
                snapshot.encode().unwrap(),
            )
            .unwrap();
        let summary = drain_meetings_vector_outputs(&mut loom, ns, "meetings", &resolved).unwrap();
        let collection = meetings_vector_collection(&profile_id);
        let output = ProjectionOutputSet::from_snapshot(&snapshot)
            .unwrap()
            .outputs_for(ProjectionKind::Vector)
            .into_iter()
            .find(|output| output.entity_id == "span-1")
            .cloned()
            .unwrap();
        let source_text =
            loom_core::vector_source_text(&loom, ns, &collection, &meetings_vector_id(&output))
                .unwrap();
        let profile_root = Digest::hash(loom.store().digest_algo(), &snapshot.encode().unwrap());
        let job = meetings_vector_projection_job(ns, &profile_id, profile_root, &output, &resolved)
            .unwrap();
        let job_path = job.job_path(loom.store().digest_algo()).unwrap();
        let job = EmbeddingProjectionJob::decode(&loom.read_file_reserved(ns, &job_path).unwrap())
            .unwrap();

        assert_eq!(summary.indexed, 2);
        assert_eq!(summary.deleted, 0);
        assert!(
            source_text
                .as_deref()
                .is_some_and(|text| text.contains("span-1"))
        );
        assert_eq!(job.state.as_str(), "ready");
    }

    #[test]
    fn vector_text_upsert_and_query_parse() {
        let upsert = cli_try_parse_for_test([
            "loom",
            "vector",
            "text",
            "upsert",
            "store.loom",
            "--workspace",
            "main",
            "notes",
            "intro",
            "--text",
            "Loom stores embeddings.",
            "--embedding-instance",
            "fast-embed",
            "--create",
            "--format",
            "json",
        ])
        .unwrap();
        match upsert.command.unwrap() {
            Command::Vector {
                action:
                    VectorCmd::Text {
                        action:
                            VectorTextCmd::Upsert {
                                store,
                                workspace,
                                name,
                                id,
                                text,
                                embedding_instance,
                                create,
                                format,
                                ..
                            },
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(name, "notes");
                assert_eq!(id, "intro");
                assert_eq!(text.as_deref(), Some("Loom stores embeddings."));
                assert_eq!(embedding_instance.as_deref(), Some("fast-embed"));
                assert!(create);
                assert_eq!(format, "json");
            }
            _ => panic!("expected vector text upsert command"),
        }

        let query = cli_try_parse_for_test([
            "loom",
            "vector",
            "text",
            "query",
            "store.loom",
            "--workspace",
            "main",
            "notes",
            "--query",
            "Where are embeddings stored?",
            "--top-k",
            "3",
        ])
        .unwrap();
        match query.command.unwrap() {
            Command::Vector {
                action:
                    VectorCmd::Text {
                        action:
                            VectorTextCmd::Query {
                                store,
                                workspace,
                                name,
                                query,
                                top_k,
                                embedding_instance,
                                ..
                            },
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(name, "notes");
                assert_eq!(query.as_deref(), Some("Where are embeddings stored?"));
                assert_eq!(top_k, 3);
                assert!(embedding_instance.is_none());
            }
            _ => panic!("expected vector text query command"),
        }
    }

    #[test]
    fn vector_text_query_text_output_includes_source_text() {
        let view = VectorTextQueryView {
            store: "store.loom".to_string(),
            workspace: "main".to_string(),
            collection: "notes".to_string(),
            query: "embeddings".to_string(),
            embedding_instance: "fast-embed".to_string(),
            model: VectorTextModelView {
                model_id: "sentence-transformers/all-MiniLM-L6-v2".to_string(),
                dimension: 384,
                weights_digest: None,
            },
            hits: vec![VectorTextHitView {
                id: "intro".to_string(),
                score: 0.75,
                source_text: Some("Loom stores embeddings.".to_string()),
            }],
        };
        let rendered = render_vector_text_query_text(&view);
        assert_eq!(rendered, "intro\t0.75\tLoom stores embeddings.\n");
    }

    #[cfg(not(feature = "backend-candle-cpu"))]
    #[test]
    fn vector_text_bound_instance_executes_with_local_smoke_provider() {
        let root = temp_test_dir("vector-text-smoke");
        let hf_cache = root.join("hub");
        let store = root.join("store.loom").to_string_lossy().into_owned();
        write_smoke_embedding_files(&hf_cache);
        let model = ModelRef::new(
            InferenceModelKind::TextEmbedding,
            "sentence-transformers/all-MiniLM-L6-v2",
        )
        .with_revision(RevisionRef::Branch("main".to_string()));
        let mut state = loom_inference::InferenceInstanceState::default();
        state.upsert_instance(
            loom_inference::build_instance_descriptor(
                "fast-embed",
                InferenceModelKind::TextEmbedding,
                model,
                RuntimeKind::CandleSafetensors,
                Some("fast".to_string()),
                BTreeMap::new(),
            )
            .unwrap(),
        );
        state.upsert_vector_binding(loom_inference::VectorWorkspaceBinding {
            store: store.clone(),
            workspace: WorkspaceId::from_bytes([7; 16]).to_string(),
            embedding_instance: "fast-embed".to_string(),
        });
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        init_control_state(&fs).unwrap();
        let mut loom = open_loom_from(fs, &KeyOpts::default(), false).unwrap();
        let workspace = WorkspaceId::from_bytes([7; 16]);
        put_inference_instance_state(&mut loom, workspace, &state).unwrap();
        let hardware = smoke_hardware_report(&hf_cache);
        let resolved = resolve_vector_text_embedding_instance_from_cache(
            &hf_cache, hardware, &loom, workspace, None,
        )
        .unwrap();
        let ns = loom
            .registry_mut()
            .create(FacetKind::Vector, Some("main"), workspace)
            .unwrap();
        let model = resolved.handle.model().unwrap();
        loom_core::vector_create(&mut loom, ns, "notes", model.dimension, Metric::Cosine).unwrap();
        loom_core::vector_upsert_text(
            &mut loom,
            ns,
            "notes",
            "intro",
            "Loom stores embeddings.",
            BTreeMap::new(),
            &resolved.handle,
        )
        .unwrap();
        let query_vectors = resolved
            .handle
            .embed(&["Loom stores embeddings.".to_string()])
            .unwrap();
        let hits =
            loom_core::vector_search(&loom, ns, "notes", &query_vectors[0], 1, &MetaFilter::All)
                .unwrap();
        let source_text = loom_core::vector_source_text(&loom, ns, "notes", &hits[0].id).unwrap();
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(resolved.instance.name, "fast-embed");
        assert_eq!(hits[0].id, "intro");
        assert_eq!(source_text.as_deref(), Some("Loom stores embeddings."));
    }

    #[cfg(not(feature = "backend-candle-cpu"))]
    fn temp_test_dir(tag: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-cli-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(not(feature = "backend-candle-cpu"))]
    fn write_smoke_embedding_files(cache_dir: &std::path::Path) {
        let json_digest = "sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a";
        let weights_digest =
            "sha256:9a129038d9a00aed0cf6a7ea059ca50a813449061ab87848cf1a13eafdf33b2c";
        let repo_dir = cache_dir.join("models--sentence-transformers--all-MiniLM-L6-v2");
        let snapshot = repo_dir.join("snapshots").join("abc123");
        std::fs::create_dir_all(repo_dir.join("refs")).unwrap();
        std::fs::write(repo_dir.join("refs").join("main"), "abc123\n").unwrap();
        [
            ("config.json", b"{}".as_slice(), json_digest),
            ("special_tokens_map.json", b"{}".as_slice(), json_digest),
            ("tokenizer.json", b"{}".as_slice(), json_digest),
            ("tokenizer_config.json", b"{}".as_slice(), json_digest),
            ("model.safetensors", b"weights".as_slice(), weights_digest),
        ]
        .into_iter()
        .for_each(|(relative_path, bytes, _digest)| {
            let path = snapshot.join(relative_path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes).unwrap();
        });
    }

    #[cfg(not(feature = "backend-candle-cpu"))]
    fn smoke_hardware_report(cache_dir: &std::path::Path) -> loom_types::HardwareReport {
        loom_types::HardwareReport {
            cpu_arch: std::env::consts::ARCH.to_string(),
            os: std::env::consts::OS.to_string(),
            target_triple: None,
            cpu_count: 8,
            total_memory_bytes: Some(8 * 1024 * 1024 * 1024),
            metal_available: cfg!(target_os = "macos"),
            cuda_available: false,
            candle_cpu_compiled: true,
            candle_cuda_compiled: false,
            browser_storage_quota_bytes: None,
            compiled_runtimes: vec![RuntimeKind::CandleSafetensors],
            hf_home: None,
            hf_cache_dir: Some(cache_dir.to_string_lossy().into_owned()),
        }
    }

    #[test]
    fn interchange_import_and_export_commands_parse() {
        assert_eq!(parse_archive_kind("zip").unwrap(), ArchiveKind::Zip);
        assert_eq!(parse_archive_kind("tar").unwrap(), ArchiveKind::Tar);
        assert_eq!(
            parse_archive_kind("tar-zstd").unwrap(),
            ArchiveKind::TarZstd
        );
        assert_eq!(
            parse_archive_kind("tar.zstd").unwrap(),
            ArchiveKind::TarZstd
        );
        assert_eq!(
            parse_archive_kind("tar-gzip").unwrap(),
            ArchiveKind::TarGzip
        );
        assert_eq!(parse_archive_kind("tar.gz").unwrap(), ArchiveKind::TarGzip);
        assert_eq!(parse_archive_kind("tgz").unwrap(), ArchiveKind::TarGzip);
        assert_eq!(parse_archive_kind("gzip").unwrap(), ArchiveKind::Gzip);
        assert_eq!(parse_archive_kind("gz").unwrap(), ArchiveKind::Gzip);
        assert!(parse_archive_kind("rar").is_err());

        let import = cli_try_parse_for_test([
            "loom",
            "interchange",
            "import-fs",
            "store.loom",
            "main",
            "/tmp/import-src",
            "--commit",
            "--dry-run",
            "--author",
            "alice",
            "--message",
            "snapshot",
            "--format",
            "json",
        ])
        .unwrap();
        match import.command.unwrap() {
            Command::Interchange {
                action:
                    InterchangeCmd::ImportFs {
                        store,
                        workspace,
                        src,
                        commit,
                        dry_run,
                        author,
                        message,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(src, "/tmp/import-src");
                assert!(commit);
                assert!(dry_run);
                assert_eq!(author, "alice");
                assert_eq!(message, "snapshot");
                assert_eq!(format, "json");
            }
            _ => panic!("expected interchange import-fs command"),
        }

        let import_archive = cli_try_parse_for_test([
            "loom",
            "interchange",
            "import-archive",
            "store.loom",
            "main",
            "/tmp/archive.zip",
            "--kind",
            "zip",
            "--dry-run",
            "--commit",
            "--author",
            "alice",
            "--message",
            "archive snapshot",
            "--format",
            "json",
        ])
        .unwrap();
        match import_archive.command.unwrap() {
            Command::Interchange {
                action:
                    InterchangeCmd::ImportArchive {
                        store,
                        workspace,
                        archive,
                        kind,
                        gzip_output_path,
                        commit,
                        dry_run,
                        author,
                        message,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(archive, "/tmp/archive.zip");
                assert_eq!(kind, "zip");
                assert!(gzip_output_path.is_none());
                assert!(commit);
                assert!(dry_run);
                assert_eq!(author, "alice");
                assert_eq!(message, "archive snapshot");
                assert_eq!(format, "json");
            }
            _ => panic!("expected interchange import-archive command"),
        }

        let import_redmine = cli_try_parse_for_test([
            "loom",
            "interchange",
            "import-redmine",
            "store.loom",
            "main",
            "studio",
            "/tmp/redmine.json",
            "--dry-run",
            "--format",
            "json",
        ])
        .unwrap();
        match import_redmine.command.unwrap() {
            Command::Interchange {
                action:
                    InterchangeCmd::ImportRedmine {
                        store,
                        workspace,
                        profile,
                        snapshot,
                        dry_run,
                        field_policy,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(profile, "studio");
                assert_eq!(snapshot, "/tmp/redmine.json");
                assert!(dry_run);
                assert_eq!(field_policy, "strict");
                assert_eq!(format, "json");
            }
            _ => panic!("expected interchange import-redmine command"),
        }

        let import_asana = cli_try_parse_for_test([
            "loom",
            "interchange",
            "import-asana",
            "store.loom",
            "main",
            "studio",
            "/tmp/asana.json",
            "--dry-run",
            "--format",
            "json",
        ])
        .unwrap();
        match import_asana.command.unwrap() {
            Command::Interchange {
                action:
                    InterchangeCmd::ImportAsana {
                        store,
                        workspace,
                        profile,
                        snapshot,
                        dry_run,
                        field_policy,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(profile, "studio");
                assert_eq!(snapshot, "/tmp/asana.json");
                assert!(dry_run);
                assert_eq!(field_policy, "strict");
                assert_eq!(format, "json");
            }
            _ => panic!("expected interchange import-asana command"),
        }

        let import_jira = cli_try_parse_for_test([
            "loom",
            "interchange",
            "import-jira",
            "store.loom",
            "main",
            "studio",
            "/tmp/jira.json",
            "--dry-run",
            "--format",
            "json",
        ])
        .unwrap();
        match import_jira.command.unwrap() {
            Command::Interchange {
                action:
                    InterchangeCmd::ImportJira {
                        store,
                        workspace,
                        profile,
                        snapshot,
                        dry_run,
                        field_policy,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(profile, "studio");
                assert_eq!(snapshot, "/tmp/jira.json");
                assert!(dry_run);
                assert_eq!(field_policy, "strict");
                assert_eq!(format, "json");
            }
            _ => panic!("expected interchange import-jira command"),
        }

        let import_confluence = cli_try_parse_for_test([
            "loom",
            "interchange",
            "import-confluence",
            "store.loom",
            "main",
            "pages",
            "/tmp/confluence.json",
            "--space",
            "wiki",
            "--dry-run",
            "--format",
            "json",
        ])
        .unwrap();
        match import_confluence.command.unwrap() {
            Command::Interchange {
                action:
                    InterchangeCmd::ImportConfluence {
                        store,
                        workspace,
                        profile,
                        snapshot,
                        space,
                        dry_run,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(profile, "pages");
                assert_eq!(snapshot, "/tmp/confluence.json");
                assert_eq!(space, "wiki");
                assert!(dry_run);
                assert_eq!(format, "json");
            }
            _ => panic!("expected interchange import-confluence command"),
        }

        let import_slack = cli_try_parse_for_test([
            "loom",
            "interchange",
            "import-slack",
            "store.loom",
            "main",
            "chat",
            "/tmp/slack.json",
            "--dry-run",
            "--format",
            "json",
        ])
        .unwrap();
        match import_slack.command.unwrap() {
            Command::Interchange {
                action:
                    InterchangeCmd::ImportSlack {
                        store,
                        workspace,
                        profile,
                        snapshot,
                        dry_run,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(profile, "chat");
                assert_eq!(snapshot, "/tmp/slack.json");
                assert!(dry_run);
                assert_eq!(format, "json");
            }
            _ => panic!("expected interchange import-slack command"),
        }

        let import_drive = cli_try_parse_for_test([
            "loom",
            "interchange",
            "import-drive",
            "store.loom",
            "main",
            "drive",
            "/tmp/drive.json",
            "--dry-run",
            "--format",
            "json",
        ])
        .unwrap();
        match import_drive.command.unwrap() {
            Command::Interchange {
                action:
                    InterchangeCmd::ImportDrive {
                        store,
                        workspace,
                        profile,
                        snapshot,
                        dry_run,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(profile, "drive");
                assert_eq!(snapshot, "/tmp/drive.json");
                assert!(dry_run);
                assert_eq!(format, "json");
            }
            _ => panic!("expected interchange import-drive command"),
        }

        let import_markdown = cli_try_parse_for_test([
            "loom",
            "interchange",
            "import-markdown",
            "store.loom",
            "main",
            "pages",
            "/tmp/vault",
            "--space",
            "docs",
            "--dry-run",
            "--format",
            "json",
        ])
        .unwrap();
        match import_markdown.command.unwrap() {
            Command::Interchange {
                action:
                    InterchangeCmd::ImportMarkdown {
                        store,
                        workspace,
                        profile,
                        src,
                        space,
                        dry_run,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(profile, "pages");
                assert_eq!(src, "/tmp/vault");
                assert_eq!(space, "docs");
                assert!(dry_run);
                assert_eq!(format, "json");
            }
            _ => panic!("expected interchange import-markdown command"),
        }

        let import_notion = cli_try_parse_for_test([
            "loom",
            "interchange",
            "import-notion",
            "store.loom",
            "main",
            "pages",
            "/tmp/notion.json",
            "--space",
            "wiki",
            "--dry-run",
            "--format",
            "json",
        ])
        .unwrap();
        match import_notion.command.unwrap() {
            Command::Interchange {
                action:
                    InterchangeCmd::ImportNotion {
                        store,
                        workspace,
                        profile,
                        snapshot,
                        space,
                        dry_run,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(profile, "pages");
                assert_eq!(snapshot, "/tmp/notion.json");
                assert_eq!(space, "wiki");
                assert!(dry_run);
                assert_eq!(format, "json");
            }
            _ => panic!("expected interchange import-notion command"),
        }

        let import_table = cli_try_parse_for_test([
            "loom",
            "interchange",
            "import-table-csv",
            "store.loom",
            "main",
            "app",
            "items",
            "/tmp/items.csv",
            "--schema",
            "id:int,name:text,amount:decimal",
            "--primary-key",
            "id",
            "--mode",
            "append-only",
            "--commit",
            "--dry-run",
            "--format",
            "json",
        ])
        .unwrap();
        match import_table.command.unwrap() {
            Command::Interchange {
                action:
                    InterchangeCmd::ImportTableCsv {
                        store,
                        workspace,
                        database,
                        table,
                        csv,
                        schema,
                        primary_key,
                        mode,
                        commit,
                        dry_run,
                        author: _,
                        message: _,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(database, "app");
                assert_eq!(table, "items");
                assert_eq!(csv, "/tmp/items.csv");
                assert_eq!(schema, "id:int,name:text,amount:decimal");
                assert_eq!(primary_key, "id");
                assert_eq!(mode, "append-only");
                assert!(commit);
                assert!(dry_run);
                assert_eq!(format, "json");
                assert_eq!(
                    parse_table_csv_import_mode(&mode).unwrap(),
                    TableImportMode::AppendOnly
                );
                assert_eq!(
                    parse_table_csv_schema(&schema).unwrap(),
                    vec![
                        ("id".to_string(), ColumnType::Int),
                        ("name".to_string(), ColumnType::Text),
                        ("amount".to_string(), ColumnType::Decimal)
                    ]
                );
            }
            _ => panic!("expected interchange import-table-csv command"),
        }

        let export_archive = cli_try_parse_for_test([
            "loom",
            "interchange",
            "export-archive",
            "store.loom",
            "main",
            "/tmp/archive.tar.zstd",
            "--kind",
            "tar-zstd",
            "--revision",
            "HEAD",
            "--dry-run",
            "--format",
            "json",
        ])
        .unwrap();
        match export_archive.command.unwrap() {
            Command::Interchange {
                action:
                    InterchangeCmd::ExportArchive {
                        store,
                        workspace,
                        archive,
                        kind,
                        revision,
                        dry_run,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(archive, "/tmp/archive.tar.zstd");
                assert_eq!(kind, "tar-zstd");
                assert_eq!(revision.as_deref(), Some("HEAD"));
                assert!(dry_run);
                assert_eq!(format, "json");
            }
            _ => panic!("expected interchange export-archive command"),
        }

        let export = cli_try_parse_for_test([
            "loom",
            "interchange",
            "export-fs",
            "store.loom",
            "main",
            "/tmp/export-dst",
            "--revision",
            "HEAD",
            "--dry-run",
            "--format",
            "json",
        ])
        .unwrap();
        match export.command.unwrap() {
            Command::Interchange {
                action:
                    InterchangeCmd::ExportFs {
                        store,
                        workspace,
                        dst,
                        revision,
                        dry_run,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(dst, "/tmp/export-dst");
                assert_eq!(revision.as_deref(), Some("HEAD"));
                assert!(dry_run);
                assert_eq!(format, "json");
            }
            _ => panic!("expected interchange export-fs command"),
        }

        let export_table = cli_try_parse_for_test([
            "loom",
            "interchange",
            "export-table-csv",
            "store.loom",
            "main",
            "app",
            "items",
            "/tmp/items.csv",
            "--dry-run",
            "--format",
            "json",
        ])
        .unwrap();
        match export_table.command.unwrap() {
            Command::Interchange {
                action:
                    InterchangeCmd::ExportTableCsv {
                        store,
                        workspace,
                        database,
                        table,
                        csv,
                        dry_run,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(database, "app");
                assert_eq!(table, "items");
                assert_eq!(csv, "/tmp/items.csv");
                assert!(dry_run);
                assert_eq!(format, "json");
            }
            _ => panic!("expected interchange export-table-csv command"),
        }

        let export_car = cli_try_parse_for_test([
            "loom",
            "interchange",
            "export-car",
            "store.loom",
            "main",
            "/tmp/export.car",
            "--dry-run",
            "--format",
            "json",
        ])
        .unwrap();
        match export_car.command.unwrap() {
            Command::Interchange {
                action:
                    InterchangeCmd::ExportCar {
                        store,
                        workspace,
                        dst,
                        dry_run,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "main");
                assert_eq!(dst, "/tmp/export.car");
                assert!(dry_run);
                assert_eq!(format, "json");
            }
            _ => panic!("expected interchange export-car command"),
        }

        let import_car = cli_try_parse_for_test([
            "loom",
            "interchange",
            "import-car",
            "store.loom",
            "/tmp/export.car",
            "--dry-run",
            "--format",
            "json",
        ])
        .unwrap();
        match import_car.command.unwrap() {
            Command::Interchange {
                action:
                    InterchangeCmd::ImportCar {
                        store,
                        src,
                        dry_run,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(src, "/tmp/export.car");
                assert!(dry_run);
                assert_eq!(format, "json");
            }
            _ => panic!("expected interchange import-car command"),
        }
    }

    #[test]
    fn redmine_import_lowers_tickets_idempotently() {
        let store = temp_store("redmine-import");
        let mut snapshot = std::env::temp_dir();
        snapshot.push(format!(
            "loom-cli-redmine-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &snapshot,
            r##"{
              "source_scope": "redmine://example",
              "projects": [
                {"id": 1, "identifier": "core", "key_prefix": "CORE", "name": "Core"}
              ],
              "issues": [
                {
                  "id": 42,
                  "project_identifier": "core",
                  "tracker": "Bug",
                  "subject": "Login fails",
                  "description": "Fails on Safari",
                  "status": "New",
                  "priority": "High",
                  "assigned_to": "alice",
                  "custom_fields": {"severity": "critical"},
                  "journals": [{"id": 7, "notes": "Status changed"}],
                  "comments": [{"id": 8, "text": "Needs logs"}],
                  "attachments": [{"id": 9, "filename": "error.txt"}],
                  "time_entries": [{"id": 10, "hours": 1.5}],
                  "relations": [{"id": 11, "relation_type": "blocks"}]
                }
              ],
              "wiki_pages": [
                {
                  "id": "Home",
                  "project_identifier": "core",
                  "page_id": "home",
                  "title": "Home",
                  "markdown": "# Home\nRedmine wiki body"
                }
              ]
            }"##,
        )
        .unwrap();

        run(
            Command::Store {
                action: StoreCmd::Init {
                    store: store.clone(),
                    encrypt: false,
                    suite: None,
                    identity_profile: None,
                    fips: false,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Workspace {
                action: WorkspaceCmd::Create {
                    store: store.clone(),
                    name: "main".to_string(),
                    facet: None,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run_interchange(
            InterchangeCmd::ImportRedmine {
                store: store.clone(),
                workspace: "main".to_string(),
                profile: "studio".to_string(),
                snapshot: snapshot.to_string_lossy().into_owned(),
                dry_run: false,
                field_policy: "infer".to_string(),
                format: "text".to_string(),
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run_interchange(
            InterchangeCmd::ImportRedmine {
                store: store.clone(),
                workspace: "main".to_string(),
                profile: "studio".to_string(),
                snapshot: snapshot.to_string_lossy().into_owned(),
                dry_run: false,
                field_policy: "infer".to_string(),
                format: "text".to_string(),
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let reader = loom_tickets::TicketProfileReader::open(&loom, ns, "studio")
            .unwrap()
            .unwrap();
        let project = reader.project("core").unwrap().unwrap();
        assert_eq!(project.key_prefix, "CORE");
        let identity = loom_tickets::ExternalTicketIdentity::new("redmine", "issue:42").unwrap();
        let ticket = reader
            .ticket_by_external_identity(&identity)
            .unwrap()
            .unwrap();
        assert_eq!(ticket.project_id, "core");
        assert_eq!(
            ticket_source_values(&ticket, "redmine_journals")[0]["notes"],
            "Status changed"
        );
        assert_eq!(
            ticket_source_values(&ticket, "redmine_comments")[0]["text"],
            "Needs logs"
        );
        assert_eq!(
            ticket_source_values(&ticket, "redmine_attachments")[0]["filename"],
            "error.txt"
        );
        assert_eq!(
            ticket_source_values(&ticket, "redmine_time_entries")[0]["hours"],
            1.5
        );
        assert_eq!(
            ticket_source_values(&ticket, "redmine_relations")[0]["relation_type"],
            "blocks"
        );
        assert_eq!(reader.tickets().unwrap().len(), 1);
        let space = loom_pages::get_space(&loom, ns, "studio", "core")
            .unwrap()
            .unwrap();
        assert_eq!(space.title, "core");
        let page = loom_pages::get_page(&loom, ns, "studio", "home")
            .unwrap()
            .unwrap();
        assert_eq!(page.title, "Home");
        let body = Body::decode(page.body.as_deref().unwrap()).unwrap();
        assert_eq!(body.blocks.len(), 2);
        assert_eq!(body.blocks[0].runs[0].text, "Home");
        assert_eq!(body.blocks[1].runs[0].text, "Redmine wiki body");

        let _ = std::fs::remove_file(&store);
        let _ = std::fs::remove_file(snapshot);
    }

    fn ticket_source_values(
        ticket: &loom_tickets::Ticket,
        field_id: &str,
    ) -> Vec<serde_json::Value> {
        ticket
            .fields
            .get(field_id)
            .unwrap()
            .to_json()
            .as_array()
            .unwrap()
            .iter()
            .map(|value| match value {
                serde_json::Value::String(text) => serde_json::from_str(text).unwrap(),
                value => value.clone(),
            })
            .collect()
    }

    #[test]
    fn asana_import_lowers_tasks_idempotently() {
        let store = temp_store("asana-import");
        let mut snapshot = std::env::temp_dir();
        snapshot.push(format!(
            "loom-cli-asana-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &snapshot,
            r#"{
              "source_scope": "asana://workspace",
              "projects": [
                {"gid": "p1", "key_prefix": "AS", "name": "Asana Project"}
              ],
              "tasks": [
                {
                  "gid": "t1",
                  "project_gid": "p1",
                  "name": "Ship importer",
                  "notes": "Normalize Asana task data",
                  "resource_subtype": "default_task",
                  "completed": false,
                  "assignee": "alice",
                  "due_on": "2026-07-31",
                  "tags": ["import"],
                  "custom_fields": {"size": "M"}
                }
              ]
            }"#,
        )
        .unwrap();

        run(
            Command::Store {
                action: StoreCmd::Init {
                    store: store.clone(),
                    encrypt: false,
                    suite: None,
                    identity_profile: None,
                    fips: false,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Workspace {
                action: WorkspaceCmd::Create {
                    store: store.clone(),
                    name: "main".to_string(),
                    facet: None,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        for _ in 0..2 {
            run_interchange(
                InterchangeCmd::ImportAsana {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    profile: "studio".to_string(),
                    snapshot: snapshot.to_string_lossy().into_owned(),
                    dry_run: false,
                    field_policy: "infer".to_string(),
                    format: "text".to_string(),
                },
                &KeyOpts::default(),
            )
            .unwrap();
        }

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let reader = loom_tickets::TicketProfileReader::open(&loom, ns, "studio")
            .unwrap()
            .unwrap();
        let project = reader.project("p1").unwrap().unwrap();
        assert_eq!(project.key_prefix, "AS");
        let identity = loom_tickets::ExternalTicketIdentity::new("asana", "task:t1").unwrap();
        let ticket = reader
            .ticket_by_external_identity(&identity)
            .unwrap()
            .unwrap();
        assert_eq!(ticket.project_id, "p1");
        assert_eq!(reader.tickets().unwrap().len(), 1);
        assert_eq!(
            ticket.fields.get("subject").unwrap().to_json(),
            serde_json::json!("Ship importer")
        );

        let _ = std::fs::remove_file(&store);
        let _ = std::fs::remove_file(snapshot);
    }

    #[test]
    fn jira_import_lowers_issues_idempotently() {
        let store = temp_store("jira-import");
        let mut snapshot = std::env::temp_dir();
        snapshot.push(format!(
            "loom-cli-jira-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &snapshot,
            r#"{
              "source_scope": "jira://site",
              "projects": [
                {"id": 10001, "key": "CORE", "name": "Core"}
              ],
              "issues": [
                {
                  "id": 10042,
                  "key": "CORE-42",
                  "project_key": "CORE",
                  "issue_type": "Bug",
                  "summary": "Login fails",
                  "description": "Fails on Safari",
                  "status": "To Do",
                  "priority": "High",
                  "assignee": "alice",
                  "reporter": "bob",
                  "labels": ["auth"],
                  "custom_fields": {"severity": "critical"}
                }
              ]
            }"#,
        )
        .unwrap();

        run(
            Command::Store {
                action: StoreCmd::Init {
                    store: store.clone(),
                    encrypt: false,
                    suite: None,
                    identity_profile: None,
                    fips: false,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Workspace {
                action: WorkspaceCmd::Create {
                    store: store.clone(),
                    name: "main".to_string(),
                    facet: None,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        for _ in 0..2 {
            run_interchange(
                InterchangeCmd::ImportJira {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    profile: "studio".to_string(),
                    snapshot: snapshot.to_string_lossy().into_owned(),
                    dry_run: false,
                    field_policy: "infer".to_string(),
                    format: "text".to_string(),
                },
                &KeyOpts::default(),
            )
            .unwrap();
        }

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let reader = loom_tickets::TicketProfileReader::open(&loom, ns, "studio")
            .unwrap()
            .unwrap();
        let project = reader.project("CORE").unwrap().unwrap();
        assert_eq!(project.key_prefix, "CORE");
        let identity = loom_tickets::ExternalTicketIdentity::new("jira", "issue:10042").unwrap();
        let ticket = reader
            .ticket_by_external_identity(&identity)
            .unwrap()
            .unwrap();
        assert_eq!(ticket.project_id, "CORE");
        assert_eq!(ticket.ticket_type, loom_tickets::TicketType::Bug);
        assert_eq!(reader.tickets().unwrap().len(), 1);
        assert_eq!(
            ticket.fields.get("jira_issue_key").unwrap().to_json(),
            serde_json::json!("CORE-42")
        );

        let _ = std::fs::remove_file(&store);
        let _ = std::fs::remove_file(snapshot);
    }

    #[test]
    fn tickets_cli_creates_updates_lists_reads_and_reports_history() {
        let store = temp_store("tickets-cli");
        run(
            Command::Store {
                action: StoreCmd::Init {
                    store: store.clone(),
                    encrypt: false,
                    suite: None,
                    identity_profile: None,
                    fips: false,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Workspace {
                action: WorkspaceCmd::Create {
                    store: store.clone(),
                    name: "main".to_string(),
                    facet: None,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Tickets {
                action: TicketsCmd::ProjectCreate {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    project_id: "core".to_string(),
                    key_prefix: "CORE".to_string(),
                    name: "Core".to_string(),
                    expected_root: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Tickets {
                action: TicketsCmd::ProjectSettingsSet {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    project_id: "core".to_string(),
                    default_projection: None,
                    actor_enforcement: Some("write-access".to_string()),
                    project_owner: None,
                    clear_project_owner: false,
                    acceptance_authorities: Vec::new(),
                    replace_acceptance_authorities: false,
                    acceptance_evidence_enforcement: None,
                    required_acceptance_evidence_keys: Vec::new(),
                    replace_required_acceptance_evidence_keys: false,
                    owner_contract_summary: None,
                    owner_contract_details: None,
                    worker_contract_summary: None,
                    worker_contract_details: None,
                    expected_root: None,
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Tickets {
                action: TicketsCmd::ProjectSettingsGet {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    project_id: "core".to_string(),
                    include_contracts: false,
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Tickets {
                action: TicketsCmd::Create {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    project_id: "core".to_string(),
                    ticket_type: "task".to_string(),
                    fields: r#"{"title":"Build CLI tickets","status":"planned"}"#.to_string(),
                    projection: None,
                    external_source: None,
                    external_id: None,
                    policy_labels: Vec::new(),
                    expected_root: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Tickets {
                action: TicketsCmd::Update {
                    store: store.clone(),
                    workspace: None,
                    ticket_id: None,
                    request: Some(r#"{"workspace":"main","ticket_id":"CORE-1","set_fields":{"status_category":"active"},"action":"claim","assignee":"writer"}"#.to_string()),
                    projection: None,
                    status: None,
                    assignee: None,
                    title: None,
                    description: None,
                    priority: None,
                    fields: Vec::new(),
                    delete_fields: Vec::new(),
                    action: None,
                    observed_source_status: None,
                    observed_workflow_version: None,
                    expected_root: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Tickets {
                action: TicketsCmd::Update {
                    store: store.clone(),
                    workspace: Some("main".to_string()),
                    ticket_id: Some("CORE-1".to_string()),
                    request: None,
                    projection: None,
                    status: Some("in_progress".to_string()),
                    assignee: Some("writer".to_string()),
                    title: Some("Build ergonomic CLI tickets".to_string()),
                    description: None,
                    priority: Some("high".to_string()),
                    fields: vec!["component=cli".to_string()],
                    delete_fields: Vec::new(),
                    action: None,
                    observed_source_status: None,
                    observed_workflow_version: None,
                    expected_root: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Tickets {
                action: TicketsCmd::List {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    projection: Some("jira".to_string()),
                    statuses: Vec::new(),
                    assignees: Vec::new(),
                    priorities: Vec::new(),
                    ticket_types: Vec::new(),
                    labels: Vec::new(),
                    policy_labels: Vec::new(),
                    lane: None,
                    board: None,
                    ready: false,
                    include_completed: false,
                    limit: None,
                    cursor: None,
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Tickets {
                action: TicketsCmd::Get {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    ticket_id: "CORE-1".to_string(),
                    projection: Some("jira".to_string()),
                    detailed: false,
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Tickets {
                action: TicketsCmd::History {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    ticket_id: None,
                    detailed: false,
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Tickets {
                action: TicketsCmd::History {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    ticket_id: Some("CORE-1".to_string()),
                    detailed: false,
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Tickets {
                action: TicketsCmd::BoardCreate {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    board_id: "core-board".to_string(),
                    board_key: "CORE-BOARD".to_string(),
                    project_id: "core".to_string(),
                    name: "Core Board".to_string(),
                    mode: "manual".to_string(),
                    description: "Manual planning board".to_string(),
                    columns: vec!["todo:To Do::10".to_string(), "doing:Doing::20".to_string()],
                    card_display_fields: vec!["title".to_string(), "status".to_string()],
                    updated_by: "cli-test".to_string(),
                    expected_root: None,
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Tickets {
                action: TicketsCmd::BoardList {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    include_deleted: false,
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Tickets {
                action: TicketsCmd::BoardMoveCard {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    board_id: "core-board".to_string(),
                    ticket_id: "CORE-1".to_string(),
                    column_id: "doing".to_string(),
                    rank_token: "0001".to_string(),
                    swimlane_id: None,
                    updated_by: "cli-test".to_string(),
                    expected_root: None,
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Tickets {
                action: TicketsCmd::BoardGet {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    board_id: "core-board".to_string(),
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let profile_id = ns.to_string();
        let projected = loom_tickets::get_ticket_with_projection(
            &loom,
            ns,
            &profile_id,
            "CORE-1",
            loom_tickets::parse_ticket_projection(Some("jira")).unwrap(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(projected.projection_profile, "jira");
        assert_eq!(projected.projection_kind, "ticket.projected.jira");
        assert_eq!(projected.projection_source, "canonical_ticket");
        assert_eq!(projected.projection_selection_source, "explicit_request");
        assert_eq!(
            projected.fields["fields.summary"],
            serde_json::json!("Build CLI tickets")
        );
        assert!(!projected.fields.contains_key("title"));
        let ticket = loom_tickets::get_ticket(&loom, ns, &profile_id, "CORE-1")
            .unwrap()
            .unwrap();
        assert_eq!(ticket.fields["title"], "Build CLI tickets");
        assert_eq!(ticket.fields["status"], "in_progress");
        assert_eq!(ticket.fields["status_category"], "active");
        assert_eq!(
            loom_tickets::history(&loom, ns, &profile_id, None)
                .unwrap()
                .len(),
            6
        );
        let board = loom_tickets::get_board(&loom, ns, &profile_id, "core-board")
            .unwrap()
            .unwrap();
        assert_eq!(board.name, "Core Board");
        assert_eq!(board.cards.len(), 1);
        assert_eq!(board.cards[0].ticket_id, "CORE-1");
        assert_eq!(board.cards[0].column_id, "doing");

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn lane_list_output_surfaces_decode_diagnostics_in_json_and_text() {
        let diagnostics = vec![LaneDecodeDiagnostic {
            lane_id: "agent-broken".to_string(),
            error: "lane document is invalid: expected value".to_string(),
        }];
        let payload = lane_list_json_payload(&[], &diagnostics);
        assert_eq!(payload["lanes"], serde_json::json!([]));
        assert_eq!(payload["diagnostics"][0]["lane_id"], "agent-broken");
        assert!(
            payload["diagnostics"][0]["error"]
                .as_str()
                .unwrap()
                .contains("invalid")
        );

        let line = lane_diagnostic_text_line(&diagnostics[0]);
        assert_eq!(
            line,
            "diagnostic\tagent-broken\tlane document is invalid: expected value"
        );
    }

    #[test]
    fn lanes_cli_creates_updates_positions_and_reads_shared_model() {
        let store = temp_store("lanes-cli");
        run(
            Command::Store {
                action: StoreCmd::Init {
                    store: store.clone(),
                    encrypt: false,
                    suite: None,
                    identity_profile: None,
                    fips: false,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Workspace {
                action: WorkspaceCmd::Create {
                    store: store.clone(),
                    name: "main".to_string(),
                    facet: None,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Lanes {
                action: LanesCmd::Create {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    lane_id: "agent-3".to_string(),
                    lane_key: "agent-3".to_string(),
                    kind: "assignment".to_string(),
                    owner_principal: Some("agent:3".to_string()),
                    lane_status: "closed".to_string(),
                    active_ticket_id: Some("MX-102".to_string()),
                    status_report: "ready".to_string(),
                    reviewer_feedback: String::new(),
                    updated_at: Some(1),
                    updated_by: Some("agent:3".to_string()),
                    tickets: vec!["MX-102".to_string(), "MX-103".to_string()],
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();

        run(
            Command::Lanes {
                action: LanesCmd::Update {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    lane_id: "agent-3".to_string(),
                    title: None,
                    description: None,
                    lane_status: None,
                    status_report: Some("working MX-103".to_string()),
                    reviewer_feedback: Some("looks good".to_string()),
                    updated_by: Some("reviewer".to_string()),
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Lanes {
                action: LanesCmd::TicketAdd {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    lane_id: "agent-3".to_string(),
                    ticket_id: "MX-104".to_string(),
                    first: true,
                    before: None,
                    after: None,
                    updated_by: Some("agent:3".to_string()),
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Lanes {
                action: LanesCmd::TicketRemove {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    lane_id: "agent-3".to_string(),
                    ticket_id: "MX-102".to_string(),
                    updated_by: Some("agent:3".to_string()),
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Lanes {
                action: LanesCmd::List {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    detailed: false,
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Lanes {
                action: LanesCmd::Get {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    lane_id: "agent-3".to_string(),
                    detailed: false,
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let lane = loom_lanes::get_lane(&loom, ns, "agent-3").unwrap().unwrap();
        assert_eq!(lane.lane_status, "closed");
        assert_eq!(lane.status_report, "working MX-103");
        assert_eq!(lane.reviewer_feedback, "looks good");
        assert_eq!(lane.active_ticket_id, None);
        assert_eq!(lane.lane_tickets[0].ticket_id, "MX-104");
        assert_eq!(lane.lane_tickets[1].ticket_id, "MX-103");
        drop(loom);
        run(
            Command::Lanes {
                action: LanesCmd::Delete {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    lane_id: "agent-3".to_string(),
                    updated_by: "agent:3".to_string(),
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let lane = loom_lanes::get_lane(&loom, ns, "agent-3").unwrap();
        assert!(lane.is_none());

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn pages_cli_creates_updates_publishes_reads_and_reports_history() {
        let store = temp_store("pages-cli");
        run(
            Command::Store {
                action: StoreCmd::Init {
                    store: store.clone(),
                    encrypt: false,
                    suite: None,
                    identity_profile: None,
                    fips: false,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Workspace {
                action: WorkspaceCmd::Create {
                    store: store.clone(),
                    name: "main".to_string(),
                    facet: None,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::SpaceCreate {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    space_id: "docs".to_string(),
                    title: "Docs".to_string(),
                    expected_root: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::Create {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    page_id: "intro".to_string(),
                    space_id: "docs".to_string(),
                    title: "Intro".to_string(),
                    parent_page_id: None,
                    expected_root: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::Update {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    page_id: "intro".to_string(),
                    body: "Welcome to Loom.".to_string(),
                    expected_root: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::Publish {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    page_id: "intro".to_string(),
                    expected_root: None,
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::SpaceList {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::SpaceGet {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    space_id: "docs".to_string(),
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::Get {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    page_id: "intro".to_string(),
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::History {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    page_id: "intro".to_string(),
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let profile_id = ns.to_string();
        let page = loom_pages::get_page(&loom, ns, &profile_id, "intro")
            .unwrap()
            .unwrap();
        assert_eq!(page.status, "published");
        assert_eq!(page.current_revision, Some(1));
        assert_eq!(page.body.as_deref(), Some(b"Welcome to Loom.".as_slice()));
        assert_eq!(
            loom_pages::page_history(&loom, ns, &profile_id, "intro")
                .unwrap()
                .len(),
            1
        );

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn pages_cli_manages_structures() {
        let store = temp_store("pages-structures-cli");
        run(
            Command::Store {
                action: StoreCmd::Init {
                    store: store.clone(),
                    encrypt: false,
                    suite: None,
                    identity_profile: None,
                    fips: false,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Workspace {
                action: WorkspaceCmd::Create {
                    store: store.clone(),
                    name: "main".to_string(),
                    facet: None,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::SpaceCreate {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    space_id: "docs".to_string(),
                    title: "Docs".to_string(),
                    expected_root: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::StructureCreate {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    structure_id: "roadmap".to_string(),
                    space_id: "docs".to_string(),
                    kind: "mindmap".to_string(),
                    title: "Roadmap".to_string(),
                    expected_root: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::StructureAddNode {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    structure_id: "roadmap".to_string(),
                    node_id: "root".to_string(),
                    kind: "topic".to_string(),
                    label: "Root".to_string(),
                    body_digest: None,
                    entity_ref: None,
                    expected_root: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::StructureAddNode {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    structure_id: "roadmap".to_string(),
                    node_id: "feature".to_string(),
                    kind: "feature".to_string(),
                    label: "Feature".to_string(),
                    body_digest: None,
                    entity_ref: Some("ticket:CORE-1".to_string()),
                    expected_root: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::StructureUpdateNode {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    structure_id: "roadmap".to_string(),
                    node_id: "feature".to_string(),
                    kind: "feature".to_string(),
                    label: "Feature updated".to_string(),
                    body_digest: None,
                    entity_ref: Some("ticket:CORE-1".to_string()),
                    expected_root: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::StructureBind {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    structure_id: "roadmap".to_string(),
                    node_id: "root".to_string(),
                    entity_ref: Some("page:roadmap".to_string()),
                    expected_root: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::StructureMoveNode {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    structure_id: "roadmap".to_string(),
                    node_id: "feature".to_string(),
                    parent_node_id: Some("root".to_string()),
                    label: None,
                    expected_root: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::StructureLinkNode {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    structure_id: "roadmap".to_string(),
                    edge_id: "relates".to_string(),
                    src_node_id: "root".to_string(),
                    dst_node_id: "feature".to_string(),
                    label: "relates_to".to_string(),
                    target_ref: None,
                    expected_root: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Tickets {
                action: TicketsCmd::ProjectCreate {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    project_id: "core".to_string(),
                    key_prefix: "CORE".to_string(),
                    name: "Core".to_string(),
                    expected_root: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::StructureDecomposeToTickets {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    structure_id: "roadmap".to_string(),
                    items: r#"[{"node_id":"feature","project_id":"core","ticket_type":"task","fields":{"title":"Build feature"},"policy_labels":["engineering"]}]"#
                        .to_string(),
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Pages {
                action: PagesCmd::StructureGet {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    structure_id: "roadmap".to_string(),
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let profile_id = ns.to_string();
        let render = loom_pages::get_structure(&loom, ns, &profile_id, "roadmap")
            .unwrap()
            .unwrap();
        assert_eq!(render.structure.kind, "mindmap");
        assert_eq!(render.nodes.len(), 2);
        assert_eq!(render.edges.len(), 2);
        assert!(render.nodes.iter().any(|node| {
            node.node_id == "feature"
                && node.label == "Feature updated"
                && node.entity_ref.as_deref() == Some("ticket:CORE-1")
        }));
        assert!(render.edges.iter().any(|edge| edge.label == "child_of"));
        assert!(render.edges.iter().any(|edge| edge.edge_id == "relates"));
        let tickets = loom_tickets::list_tickets(&loom, ns, &profile_id).unwrap();
        assert!(tickets.iter().any(|ticket| {
            ticket.project_id == "core"
                && ticket.ticket_type == "task"
                && ticket.primary_key == "CORE-1"
        }));

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn lifecycle_cli_defines_instantiates_transitions_and_reads() {
        let store = temp_store("lifecycle-cli");
        run(
            Command::Store {
                action: StoreCmd::Init {
                    store: store.clone(),
                    encrypt: false,
                    suite: None,
                    identity_profile: None,
                    fips: false,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Workspace {
                action: WorkspaceCmd::Create {
                    store: store.clone(),
                    name: "main".to_string(),
                    facet: None,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Lifecycle {
                action: LifecycleCmd::DefineStandard {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    kind: "feature".to_string(),
                    version: "1".to_string(),
                    completion_predicate_digest: digest(b"predicate").to_string(),
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Lifecycle {
                action: LifecycleCmd::Definitions {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Lifecycle {
                action: LifecycleCmd::Definition {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    definition_id: "feature".to_string(),
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Lifecycle {
                action: LifecycleCmd::Instantiate {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    instance_id: "feat-1".to_string(),
                    definition_id: "feature".to_string(),
                    subject_refs: vec!["page:roadmap".to_string()],
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Lifecycle {
                action: LifecycleCmd::SnapshotPlan {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    instance_id: "feat-1".to_string(),
                    to_stage_id: "draft".to_string(),
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Lifecycle {
                action: LifecycleCmd::Transition {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    instance_id: "feat-1".to_string(),
                    transition_id: "tr-1".to_string(),
                    to_stage_id: "draft".to_string(),
                    actor_principal_id: None,
                    gate_evaluations: r#"[{"gate_id":"enter-draft","passed":true}]"#.to_string(),
                    snapshot_digest: None,
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Lifecycle {
                action: LifecycleCmd::Instances {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Lifecycle {
                action: LifecycleCmd::Instance {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    instance_id: "feat-1".to_string(),
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Lifecycle {
                action: LifecycleCmd::CurrentSurface {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    instance_id: "feat-1".to_string(),
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Lifecycle {
                action: LifecycleCmd::Snapshots {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Lifecycle {
                action: LifecycleCmd::OperationLog {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let profile_id = ns.to_string();
        let instance = loom_lifecycle::get_instance(&loom, ns, &profile_id, "feat-1")
            .unwrap()
            .unwrap();
        assert_eq!(instance.current_stage_id, "draft");
        assert_eq!(instance.stage_history.len(), 1);
        assert_eq!(
            loom_lifecycle::operation_log(&loom, ns, &profile_id)
                .unwrap()
                .records
                .len(),
            1
        );

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn llms_reference_exposes_studio_profile_commands() {
        let mut command = cli_command_for_test();
        let _ = command.render_usage();
        let lifecycle = command
            .find_subcommand("lifecycle")
            .expect("lifecycle command is visible");
        assert!(lifecycle.find_subcommand("define-standard").is_some());
        assert!(lifecycle.find_subcommand("instantiate").is_some());
        assert!(lifecycle.find_subcommand("transition").is_some());
        let pages = command
            .find_subcommand("pages")
            .expect("pages command is visible");
        assert!(pages.find_subcommand("create").is_some());
        assert!(pages.find_subcommand("get").is_some());
        assert!(pages.find_subcommand("history").is_some());
        assert!(pages.find_subcommand("structure-create").is_some());
        assert!(pages.find_subcommand("structure-get").is_some());
        assert!(pages.find_subcommand("structure-link-node").is_some());
        let tickets = command
            .find_subcommand("tickets")
            .expect("tickets command is visible");
        assert!(tickets.find_subcommand("create").is_some());
        assert!(tickets.find_subcommand("get").is_some());
        assert!(tickets.find_subcommand("history").is_some());
    }

    #[test]
    fn markdown_import_lowers_pages_idempotently() {
        let store = temp_store("markdown-import");
        let mut root = std::env::temp_dir();
        root.push(format!(
            "loom-cli-markdown-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("guides")).unwrap();
        std::fs::write(
            root.join("Intro.md"),
            "# Intro\nWelcome to Loom.\n- [ ] Import task\n1. Ordered step\n> Quoted\n---\n",
        )
        .unwrap();
        std::fs::write(root.join("Embed.md"), "![[Intro]]\n").unwrap();
        std::fs::write(root.join("guides").join("Setup.md"), "# Setup\nRun init.\n").unwrap();

        run(
            Command::Store {
                action: StoreCmd::Init {
                    store: store.clone(),
                    encrypt: false,
                    suite: None,
                    identity_profile: None,
                    fips: false,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Workspace {
                action: WorkspaceCmd::Create {
                    store: store.clone(),
                    name: "main".to_string(),
                    facet: None,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        for _ in 0..2 {
            run_interchange(
                InterchangeCmd::ImportMarkdown {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    profile: "pages".to_string(),
                    src: root.to_string_lossy().into_owned(),
                    space: "docs".to_string(),
                    dry_run: false,
                    format: "text".to_string(),
                },
                &KeyOpts::default(),
            )
            .unwrap();
        }

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let spaces = loom_pages::list_spaces(&loom, ns, "pages").unwrap();
        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0].space_id, "docs");
        let intro = loom_pages::get_page(&loom, ns, "pages", "intro")
            .unwrap()
            .unwrap();
        assert_eq!(intro.title, "Intro");
        assert!(intro.body.is_some());
        let intro_body = Body::decode(intro.body.as_deref().unwrap()).unwrap();
        assert_eq!(intro_body.blocks.len(), 6);
        assert!(matches!(
            intro_body.blocks[2].kind,
            BlockKind::ListItem { ordered: false }
        ));
        assert_eq!(intro_body.blocks[2].runs[0].text, "Import task");
        assert!(matches!(
            intro_body.blocks[3].kind,
            BlockKind::ListItem { ordered: true }
        ));
        assert!(matches!(intro_body.blocks[4].kind, BlockKind::Quote));
        assert!(matches!(intro_body.blocks[5].kind, BlockKind::Divider));
        assert_eq!(
            loom_pages::page_history(&loom, ns, "pages", "intro")
                .unwrap()
                .len(),
            1
        );
        let embed = loom_pages::get_page(&loom, ns, "pages", "embed")
            .unwrap()
            .unwrap();
        let embed_body = Body::decode(embed.body.as_deref().unwrap()).unwrap();
        assert_eq!(embed_body.blocks.len(), 1);
        match &embed_body.blocks[0].kind {
            BlockKind::BlockRef {
                entity_id,
                block_id,
                section,
                pin,
            } => {
                assert_eq!(entity_id, "page:intro");
                assert!(block_id.is_none());
                assert!(!section);
                assert!(pin.is_none());
            }
            other => panic!("expected block ref, got {other:?}"),
        }
        assert!(
            loom_pages::get_page(&loom, ns, "pages", "guides-setup")
                .unwrap()
                .is_some()
        );

        let _ = std::fs::remove_file(&store);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn notion_import_lowers_pages_idempotently() {
        let store = temp_store("notion-import");
        let snapshot = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../specs/studio/fixtures/notion/source/notion-api-bundle.json");

        run(
            Command::Store {
                action: StoreCmd::Init {
                    store: store.clone(),
                    encrypt: false,
                    suite: None,
                    identity_profile: None,
                    fips: false,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Workspace {
                action: WorkspaceCmd::Create {
                    store: store.clone(),
                    name: "main".to_string(),
                    facet: None,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        for _ in 0..2 {
            run_interchange(
                InterchangeCmd::ImportNotion {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    profile: "pages".to_string(),
                    snapshot: snapshot.to_string_lossy().into_owned(),
                    space: "notion".to_string(),
                    dry_run: false,
                    format: "text".to_string(),
                },
                &KeyOpts::default(),
            )
            .unwrap();
        }

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let spaces = loom_pages::list_spaces(&loom, ns, "pages").unwrap();
        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0].space_id, "notion");
        let page = loom_pages::get_page(&loom, ns, "pages", "page-intro")
            .unwrap()
            .unwrap();
        assert_eq!(page.title, "Intro");
        assert!(page.body.is_some());
        assert_eq!(
            loom_pages::page_history(&loom, ns, "pages", "page-intro")
                .unwrap()
                .len(),
            1
        );
        let child = loom_pages::get_page(&loom, ns, "pages", "child")
            .unwrap()
            .unwrap();
        assert_eq!(child.parent_page_id.as_deref(), Some("page-intro"));
        assert!(child.body.is_some());

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn confluence_import_preserves_storage_body_idempotently() {
        let store = temp_store("confluence-import");
        let mut snapshot = std::env::temp_dir();
        snapshot.push(format!(
            "loom-cli-confluence-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &snapshot,
            r#"{
              "source_scope": "confluence://site",
              "pages": [
                {
                  "id": "123",
                  "title": "Home",
                  "space_id": "wiki",
                  "storage_xhtml": "<p>Hello <strong>Confluence</strong></p>"
                }
              ]
            }"#,
        )
        .unwrap();

        run(
            Command::Store {
                action: StoreCmd::Init {
                    store: store.clone(),
                    encrypt: false,
                    suite: None,
                    identity_profile: None,
                    fips: false,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Workspace {
                action: WorkspaceCmd::Create {
                    store: store.clone(),
                    name: "main".to_string(),
                    facet: None,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        for _ in 0..2 {
            run_interchange(
                InterchangeCmd::ImportConfluence {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    profile: "pages".to_string(),
                    snapshot: snapshot.to_string_lossy().into_owned(),
                    space: "wiki".to_string(),
                    dry_run: false,
                    format: "text".to_string(),
                },
                &KeyOpts::default(),
            )
            .unwrap();
        }

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let page = loom_pages::get_page(&loom, ns, "pages", "123")
            .unwrap()
            .unwrap();
        let body = Body::decode(page.body.as_deref().unwrap()).unwrap();
        match &body.blocks[0].kind {
            BlockKind::Opaque { kind, payload } => {
                assert_eq!(kind, "confluence.storage");
                assert_eq!(payload, b"<p>Hello <strong>Confluence</strong></p>");
            }
            other => panic!("expected opaque Confluence body, got {other:?}"),
        }
        assert_eq!(
            loom_pages::page_history(&loom, ns, "pages", "123")
                .unwrap()
                .len(),
            1
        );

        let _ = std::fs::remove_file(&store);
        let _ = std::fs::remove_file(snapshot);
    }

    #[test]
    fn slack_import_lowers_chat_idempotently() {
        let store = temp_store("slack-import");
        let mut snapshot = std::env::temp_dir();
        snapshot.push(format!(
            "loom-cli-slack-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &snapshot,
            r#"{
              "source_scope": "slack://workspace",
              "channels": [
                {"id": "C123", "name": "general"}
              ],
              "messages": [
                {
                  "channel_id": "C123",
                  "ts": "1710000000.000100",
                  "user": "U1",
                  "text": "Hello from Slack",
                  "reactions": [{"name": "wave", "users": ["U2"]}]
                },
                {
                  "channel_id": "C123",
                  "ts": "1710000001.000200",
                  "thread_ts": "1710000000.000100",
                  "user": "U2",
                  "text": "Thread reply"
                }
              ]
            }"#,
        )
        .unwrap();

        run(
            Command::Store {
                action: StoreCmd::Init {
                    store: store.clone(),
                    encrypt: false,
                    suite: None,
                    identity_profile: None,
                    fips: false,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Workspace {
                action: WorkspaceCmd::Create {
                    store: store.clone(),
                    name: "main".to_string(),
                    facet: None,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        for _ in 0..2 {
            run_interchange(
                InterchangeCmd::ImportSlack {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    profile: "chat".to_string(),
                    snapshot: snapshot.to_string_lossy().into_owned(),
                    dry_run: false,
                    format: "text".to_string(),
                },
                &KeyOpts::default(),
            )
            .unwrap();
        }

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let channel = loom_chat::resolve_channel_id(&loom, ns, "chat", "general").unwrap();
        let projection = loom_chat::channel_projection(&loom, ns, "chat", &channel).unwrap();
        assert_eq!(projection.channel_id, channel);
        assert_eq!(projection.messages.len(), 2);
        assert_eq!(
            String::from_utf8(projection.messages[0].body.clone()).unwrap(),
            "Hello from Slack"
        );
        assert_eq!(projection.messages[0].reactions.len(), 1);
        assert_eq!(projection.messages[0].reactions[0].kind, "wave");
        assert_eq!(projection.threads.len(), 1);
        assert_eq!(
            projection.messages[1].thread_id.as_deref(),
            Some("1710000000.000100")
        );

        let _ = std::fs::remove_file(&store);
        let _ = std::fs::remove_file(snapshot);
    }

    #[test]
    fn slack_zip_import_lowers_chat_messages() {
        let store = temp_store("slack-zip-import");
        let mut zip_path = std::env::temp_dir();
        zip_path.push(format!(
            "loom-cli-slack-{}-{}.zip",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("channels.json", options).unwrap();
            zip.write_all(br#"[{"id":"CZIP","name":"general","members":["U1"]}]"#)
                .unwrap();
            zip.start_file("general/2024-01-01.json", options).unwrap();
            zip.write_all(br#"[{"ts":"1710000100.000100","user":"U1","text":"Hello from zip"}]"#)
                .unwrap();
            zip.finish().unwrap();
        }

        run(
            Command::Store {
                action: StoreCmd::Init {
                    store: store.clone(),
                    encrypt: false,
                    suite: None,
                    identity_profile: None,
                    fips: false,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Workspace {
                action: WorkspaceCmd::Create {
                    store: store.clone(),
                    name: "main".to_string(),
                    facet: None,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run_interchange(
            InterchangeCmd::ImportSlack {
                store: store.clone(),
                workspace: "main".to_string(),
                profile: "chat".to_string(),
                snapshot: zip_path.to_string_lossy().into_owned(),
                dry_run: false,
                format: "text".to_string(),
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let channel = loom_chat::resolve_channel_id(&loom, ns, "chat", "general").unwrap();
        let projection = loom_chat::channel_projection(&loom, ns, "chat", &channel).unwrap();
        assert_eq!(projection.messages.len(), 1);
        assert_eq!(
            String::from_utf8(projection.messages[0].body.clone()).unwrap(),
            "Hello from zip"
        );

        let _ = std::fs::remove_file(&store);
        let _ = std::fs::remove_file(zip_path);
    }

    #[test]
    fn drive_import_lowers_files_idempotently() {
        let store = temp_store("drive-import");
        let mut snapshot = std::env::temp_dir();
        snapshot.push(format!(
            "loom-cli-drive-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &snapshot,
            r#"{
              "source_scope": "drive://export",
              "folders": [
                {"id": "docs", "parent_id": "root", "name": "Docs"}
              ],
              "files": [
                {"id": "readme", "parent_id": "docs", "name": "README.md", "text": "Drive import body"},
                {"id": "binary", "parent_id": "docs", "name": "binary.bin", "content_hex": "000102ff"}
              ]
            }"#,
        )
        .unwrap();

        run(
            Command::Store {
                action: StoreCmd::Init {
                    store: store.clone(),
                    encrypt: false,
                    suite: None,
                    identity_profile: None,
                    fips: false,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Workspace {
                action: WorkspaceCmd::Create {
                    store: store.clone(),
                    name: "main".to_string(),
                    facet: None,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        for _ in 0..2 {
            run_interchange(
                InterchangeCmd::ImportDrive {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    profile: "drive".to_string(),
                    snapshot: snapshot.to_string_lossy().into_owned(),
                    dry_run: false,
                    format: "text".to_string(),
                },
                &KeyOpts::default(),
            )
            .unwrap();
        }

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let root = loom_drive::list_folder(&loom, ns, "drive", "root").unwrap();
        assert_eq!(root.entries.len(), 1);
        assert_eq!(root.entries[0].kind, "folder");
        let docs = loom_drive::list_folder(&loom, ns, "drive", "docs").unwrap();
        assert_eq!(docs.entries.len(), 2);
        let entry_names = docs
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(entry_names.contains("README.md"));
        assert!(entry_names.contains("binary.bin"));
        assert_eq!(
            loom_drive::read_file(&loom, ns, "drive", "readme").unwrap(),
            b"Drive import body"
        );
        assert_eq!(
            loom_drive::read_file(&loom, ns, "drive", "binary").unwrap(),
            vec![0, 1, 2, 255]
        );
        assert_eq!(
            loom_drive::list_versions(&loom, ns, "drive", "readme")
                .unwrap()
                .len(),
            1
        );

        let _ = std::fs::remove_file(&store);
        let _ = std::fs::remove_file(snapshot);
    }

    #[test]
    fn structured_imports_report_unsupported_source_fields() {
        let mut report = loom_interchange::ImportReport::new(ImportReportInput {
            profile: "test",
            source_scope: "source",
            commit: None,
            objects_added: 0,
            bytes_in: 1,
            bytes_stored: 0,
            rows_imported: 0,
            skipped: 0,
            operations_planned: 1,
            operations_applied: 0,
            dry_run: true,
        })
        .unwrap();
        let mut loom = Loom::new(
            FileStore::create_with_profile(temp_store("fidelity"), Algo::Blake3).unwrap(),
        );
        let ns = WorkspaceId::from_bytes([88; 16]);
        let redmine_report = loom_interchange_io::import_redmine_snapshot(
            &mut loom,
            ns,
            "tickets",
            "redmine.json",
            1,
            loom_interchange_io::RedmineImportSnapshot {
                source_scope: None,
                projects: Vec::new(),
                issues: vec![loom_interchange_io::RedmineIssue {
                    id: serde_json::json!(1),
                    project_id: None,
                    project_identifier: None,
                    tracker: None,
                    subject: "Issue".to_string(),
                    description: None,
                    status: None,
                    priority: None,
                    category: None,
                    assigned_to: None,
                    author: None,
                    created_at: None,
                    created_on: None,
                    updated_at: None,
                    updated_on: None,
                    start_date: None,
                    due_date: None,
                    closed_on: None,
                    done_ratio: None,
                    estimated_hours: None,
                    fixed_version: None,
                    affected_version: None,
                    affected_versions: Vec::new(),
                    parent_issue_id: None,
                    is_private: None,
                    url: None,
                    custom_fields: None,
                    policy_labels: Vec::new(),
                    journals: vec![serde_json::json!({})],
                    comments: Vec::new(),
                    watchers: Vec::new(),
                    attachments: vec![serde_json::json!({})],
                    time_entries: Vec::new(),
                    relations: vec![serde_json::json!({})],
                    children: Vec::new(),
                    changesets: Vec::new(),
                    allowed_statuses: Vec::new(),
                }],
                wiki_pages: Vec::new(),
                time_entries: Vec::new(),
            },
            true,
            loom_interchange_io::TicketImportFieldPolicy::Strict,
        )
        .unwrap();
        report
            .fidelity_issues
            .extend(redmine_report.fidelity_issues);
        let asana_report = loom_interchange_io::import_asana_snapshot(
            &mut loom,
            ns,
            "tickets",
            "asana.json",
            1,
            loom_interchange_io::AsanaImportSnapshot {
                source_scope: None,
                projects: Vec::new(),
                tasks: vec![loom_interchange_io::AsanaTask {
                    gid: serde_json::json!("t1"),
                    name: "Task".to_string(),
                    project_gid: None,
                    project_id: None,
                    notes: None,
                    html_notes: None,
                    resource_subtype: None,
                    approval_status: None,
                    assignee_status: None,
                    completed: None,
                    completed_at: None,
                    completed_by: None,
                    created_at: None,
                    created_by: None,
                    modified_at: None,
                    assigned_by: None,
                    assignee: None,
                    assignee_section: None,
                    workspace: None,
                    parent: None,
                    external: None,
                    due_on: None,
                    due_at: None,
                    start_on: None,
                    start_at: None,
                    tags: Vec::new(),
                    custom_fields: None,
                    dependencies: Vec::new(),
                    dependents: Vec::new(),
                    memberships: Vec::new(),
                    followers: Vec::new(),
                    likes: Vec::new(),
                    liked: None,
                    num_likes: None,
                    num_subtasks: None,
                    actual_time_minutes: None,
                    is_rendered_as_separator: None,
                    subtasks: vec![serde_json::json!({})],
                    stories: vec![serde_json::json!({})],
                    attachments: vec![serde_json::json!({})],
                    portfolios: Vec::new(),
                    goals: Vec::new(),
                }],
            },
            true,
            loom_interchange_io::TicketImportFieldPolicy::Strict,
        )
        .unwrap();
        report.fidelity_issues.extend(asana_report.fidelity_issues);
        let jira_report = loom_interchange_io::import_jira_snapshot(
            &mut loom,
            ns,
            "tickets",
            "jira.json",
            1,
            loom_interchange_io::JiraImportSnapshot {
                source_scope: None,
                projects: Vec::new(),
                issues: vec![loom_interchange_io::JiraIssue {
                    id: serde_json::json!(2),
                    key: "CORE-2".to_string(),
                    project_id: None,
                    project_key: None,
                    issue_type: None,
                    summary: "Bug".to_string(),
                    description: None,
                    status: None,
                    status_category: None,
                    priority: None,
                    resolution: None,
                    resolution_date: None,
                    assignee: None,
                    reporter: None,
                    creator: None,
                    created_at: None,
                    updated_at: None,
                    due_date: None,
                    environment: None,
                    parent: None,
                    security: None,
                    votes: None,
                    watches: None,
                    sprint: None,
                    transitions: Vec::new(),
                    labels: Vec::new(),
                    custom_fields: None,
                    components: Vec::new(),
                    fix_versions: Vec::new(),
                    affected_versions: Vec::new(),
                    issue_links: Vec::new(),
                    subtasks: Vec::new(),
                    properties: None,
                    development: None,
                    changelog: Some(serde_json::json!({})),
                    comments: vec![serde_json::json!({})],
                    attachments: Vec::new(),
                    worklog: Vec::new(),
                }],
            },
            true,
            loom_interchange_io::TicketImportFieldPolicy::Strict,
        )
        .unwrap();
        report.fidelity_issues.extend(jira_report.fidelity_issues);
        let confluence_report = loom_interchange_io::import_confluence_snapshot(
            &mut loom,
            ns,
            "pages",
            "confluence.json",
            "docs",
            1,
            loom_interchange_io::ConfluenceImportSnapshot {
                source_scope: None,
                spaces: Vec::new(),
                pages: vec![loom_interchange_io::ConfluencePage {
                    id: "p1".to_string(),
                    title: "Page".to_string(),
                    space_id: None,
                    status: None,
                    version: None,
                    author_id: None,
                    owner_id: None,
                    created_at: None,
                    links: None,
                    ancestors: Vec::new(),
                    descendants: Vec::new(),
                    labels: Vec::new(),
                    properties: Vec::new(),
                    restrictions: Vec::new(),
                    parent_page_id: None,
                    storage_xhtml: Some("<p>x</p>".to_string()),
                    adf_json: None,
                    text: None,
                    markdown: None,
                    attachments: Vec::new(),
                    comments: vec![serde_json::json!({})],
                }],
            },
            true,
        )
        .unwrap();
        report
            .fidelity_issues
            .extend(confluence_report.fidelity_issues);
        let markdown_dir = PathBuf::from(temp_store("markdown-fidelity-dir"));
        std::fs::create_dir_all(&markdown_dir).unwrap();
        std::fs::write(
            markdown_dir.join("Page.md"),
            "---\ntags: [a]\n---\n[[Other]]\n",
        )
        .unwrap();
        let markdown_report = loom_interchange_io::import_markdown_path(
            &mut loom,
            ns,
            "pages",
            "markdown",
            &markdown_dir,
            "docs",
            true,
        )
        .unwrap();
        report
            .fidelity_issues
            .extend(markdown_report.fidelity_issues);
        let notion_report = loom_interchange_io::import_notion_snapshot(
            &mut loom,
            ns,
            "pages",
            "notion.json",
            "docs",
            1,
            loom_interchange_io::NotionImportSnapshot {
                source_scope: None,
                pages: vec![loom_interchange_io::NotionPage {
                    id: "n1".to_string(),
                    title: "Page".to_string(),
                    space_id: None,
                    parent_page_id: None,
                    markdown: None,
                    text: None,
                    blocks: Vec::new(),
                    database: Some(serde_json::json!({})),
                    property_values: Vec::new(),
                    formulas: Vec::new(),
                    rollups: Vec::new(),
                    views: Vec::new(),
                    comments: vec![serde_json::json!({})],
                    permissions: Vec::new(),
                    attachments: Vec::new(),
                    synced_blocks: vec![serde_json::json!({})],
                    rich_text_semantics: Vec::new(),
                    unsupported_blocks: vec![serde_json::json!({})],
                    users: Vec::new(),
                    source_metadata: None,
                }],
            },
            true,
        )
        .unwrap();
        report.fidelity_issues.extend(notion_report.fidelity_issues);
        let slack_report = loom_interchange_io::import_slack_snapshot(
            &mut loom,
            ns,
            "chat",
            "slack.json",
            1,
            loom_interchange_io::SlackImportSnapshot {
                source_scope: None,
                channels: vec![loom_interchange_io::SlackChannel {
                    id: "C1".to_string(),
                    handle: None,
                    name: Some("general".to_string()),
                    name_normalized: None,
                    is_channel: None,
                    is_group: None,
                    is_im: None,
                    is_mpim: None,
                    is_private: None,
                    is_archived: None,
                    is_general: None,
                    is_shared: None,
                    is_ext_shared: None,
                    created: None,
                    updated: None,
                    creator: None,
                    topic: None,
                    purpose: None,
                    properties: None,
                    previous_names: Vec::new(),
                    shared_team_ids: Vec::new(),
                    members: vec!["U1".to_string()],
                }],
                messages: vec![loom_interchange_io::SlackMessage {
                    r#type: None,
                    subtype: None,
                    channel_id: "C1".to_string(),
                    ts: "1.0".to_string(),
                    thread_ts: None,
                    user: None,
                    username: None,
                    bot_id: None,
                    app_id: None,
                    team: None,
                    channel_type: None,
                    text: Some("hi".to_string()),
                    body: None,
                    edited: None,
                    is_starred: None,
                    pinned_to: Vec::new(),
                    blocks: Vec::new(),
                    attachments: Vec::new(),
                    files: Vec::new(),
                    metadata: None,
                    client_msg_id: None,
                    permalink: None,
                    hidden: None,
                    deleted_ts: None,
                    event_ts: None,
                    reactions: vec![loom_interchange_io::SlackReaction {
                        name: "wave".to_string(),
                        count: None,
                        users: vec!["U1".to_string()],
                    }],
                }],
                users: Vec::new(),
                usergroups: Vec::new(),
                files: vec![serde_json::json!({})],
                custom_emoji: vec![serde_json::json!({})],
                pins: vec![serde_json::json!({})],
            },
            true,
        )
        .unwrap();
        report.fidelity_issues.extend(slack_report.fidelity_issues);
        let drive_report = loom_interchange_io::import_drive_snapshot(
            &mut loom,
            ns,
            "drive",
            "drive.json",
            1,
            std::path::Path::new("."),
            loom_interchange_io::DriveImportSnapshot {
                source_scope: None,
                folders: vec![loom_interchange_io::DriveFolder {
                    id: "d1".to_string(),
                    parent_id: None,
                    parents: Vec::new(),
                    name: "Folder".to_string(),
                    source_system: None,
                    mime_type: None,
                    drive_id: None,
                    created_time: None,
                    modified_time: None,
                    trashed: None,
                    web_view_link: None,
                    sharepoint_ids: None,
                    retention_label: None,
                    permissions: vec![serde_json::json!({})],
                    comments: vec![serde_json::json!({})],
                    metadata: Some(serde_json::json!({})),
                }],
                files: vec![loom_interchange_io::DriveFile {
                    id: "f1".to_string(),
                    parent_id: None,
                    parents: Vec::new(),
                    name: "a.txt".to_string(),
                    source_system: None,
                    mime_type: None,
                    drive_id: None,
                    created_time: None,
                    modified_time: None,
                    trashed: None,
                    text: Some("a".to_string()),
                    content_hex: None,
                    content_path: None,
                    web_view_link: None,
                    web_content_link: None,
                    download_url: None,
                    size: None,
                    md5_checksum: None,
                    sha1_checksum: None,
                    sha256_checksum: None,
                    owners: Vec::new(),
                    last_modifying_user: None,
                    labels: Vec::new(),
                    capabilities: None,
                    content_restrictions: Vec::new(),
                    link_share_metadata: None,
                    sharepoint_ids: None,
                    retention_label: None,
                    list_item: None,
                    thumbnails: Vec::new(),
                    remote_item: None,
                    permissions: vec![serde_json::json!({})],
                    comments: vec![serde_json::json!({})],
                    revisions: vec![serde_json::json!({})],
                    metadata: None,
                    shortcut_target: Some("other".to_string()),
                }],
            },
            true,
        )
        .unwrap();
        report.fidelity_issues.extend(drive_report.fidelity_issues);

        let fields = report
            .fidelity_issues
            .iter()
            .map(|issue| issue.field.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for field in [
            "attachments",
            "stories",
            "subtasks",
            "changelog",
            "comments",
            "frontmatter",
            "wikilinks",
            "database",
            "synced_blocks",
            "members",
            "reaction_users",
            "permissions",
            "shortcut_target",
        ] {
            assert!(fields.contains(field), "missing fidelity issue for {field}");
        }
    }

    #[test]
    fn meetings_import_command_parses_and_lowers_snapshot() {
        let command = cli_try_parse_for_test([
            "loom",
            "meetings",
            "list",
            "store.loom",
            "studio",
            "--limit",
            "10",
            "--offset",
            "2",
            "--format",
            "json",
        ])
        .unwrap();
        match command.command.unwrap() {
            Command::Meetings {
                action:
                    MeetingsCmd::List {
                        store,
                        workspace,
                        limit,
                        offset,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "studio");
                assert_eq!(limit, 10);
                assert_eq!(offset, 2);
                assert_eq!(format, "json");
            }
            _ => panic!("expected meetings list command"),
        }

        let command = cli_try_parse_for_test([
            "loom",
            "meetings",
            "get",
            "store.loom",
            "studio",
            "meeting/source-a",
            "--format",
            "json",
        ])
        .unwrap();
        match command.command.unwrap() {
            Command::Meetings {
                action:
                    MeetingsCmd::Get {
                        store,
                        workspace,
                        meeting_id,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "studio");
                assert_eq!(meeting_id, "meeting/source-a");
                assert_eq!(format, "json");
            }
            _ => panic!("expected meetings get command"),
        }

        let command = cli_try_parse_for_test([
            "loom",
            "meetings",
            "search",
            "store.loom",
            "studio",
            "architecture",
            "--field",
            "body",
            "--limit",
            "5",
            "--offset",
            "1",
            "--format",
            "json",
        ])
        .unwrap();
        match command.command.unwrap() {
            Command::Meetings {
                action:
                    MeetingsCmd::Search {
                        store,
                        workspace,
                        query,
                        field,
                        limit,
                        offset,
                        format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "studio");
                assert_eq!(query, "architecture");
                assert_eq!(field.as_deref(), Some("body"));
                assert_eq!(limit, 5);
                assert_eq!(offset, 1);
                assert_eq!(format, "json");
            }
            _ => panic!("expected meetings search command"),
        }

        let command = cli_try_parse_for_test([
            "loom",
            "meetings",
            "import",
            "store.loom",
            "studio",
            "--input-profile",
            "granola-api",
            "--input",
            "snapshot.json",
            "--dry-run",
            "--report-format",
            "json",
        ])
        .unwrap();
        match command.command.unwrap() {
            Command::Meetings {
                action:
                    MeetingsCmd::Import {
                        store,
                        workspace,
                        input_profile,
                        input,
                        dry_run,
                        report_format,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "studio");
                assert_eq!(input_profile, "granola-api");
                assert_eq!(input, "snapshot.json");
                assert!(dry_run);
                assert_eq!(report_format, "json");
            }
            _ => panic!("expected meetings import command"),
        }

        let command = cli_try_parse_for_test([
            "loom",
            "meetings",
            "source-read",
            "store.loom",
            "studio",
            "source-a",
            "summary.txt",
            "--out",
            "summary.out",
        ])
        .unwrap();
        match command.command.unwrap() {
            Command::Meetings {
                action:
                    MeetingsCmd::SourceRead {
                        store,
                        workspace,
                        source_id,
                        leaf,
                        out,
                    },
            } => {
                assert_eq!(store, "store.loom");
                assert_eq!(workspace, "studio");
                assert_eq!(source_id, "source-a");
                assert_eq!(leaf, "summary.txt");
                assert_eq!(out.as_deref(), Some("summary.out"));
            }
            _ => panic!("expected meetings source-read command"),
        }

        let store =
            FileStore::create_with_profile(temp_store("meetings-import"), Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace_id = WorkspaceId::parse("1b1b1b1b-1b1b-4b1b-9b1b-1b1b1b1b1b1b").unwrap();
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("studio"), workspace_id)
            .unwrap();
        let source_digest = Digest::hash(Algo::Blake3, b"source").to_string();
        let sidecar_digest = Digest::hash(Algo::Blake3, b"sidecar").to_string();
        let input = serde_json::json!({
            "snapshot_version": 1,
            "profile": "granola-api",
            "source_system": "granola-api",
            "source_scope": "personal-notes",
            "observed_at": 100,
            "coverage": "partial",
            "source_cursor": "cursor-1",
            "source_sidecar_digest": sidecar_digest,
            "coverage_gaps": ["rate-limit"],
            "items": [{
                "source_entity_id": "not_1",
                "source_digest": source_digest,
                "source_created_at": 90,
                "source_updated_at": 100,
                "title": "Architecture review",
                "owner": "principal/alice",
                "attendees": ["principal/bob"],
                "folder_refs": ["folder/design"],
                "summary_text": "Discussed import shape.",
                "transcript_spans": [{
                    "span_id": "span/not_1/transcript/0",
                    "speaker": "principal/alice",
                    "language": "en",
                    "text": "Use normalized snapshots."
                }],
                "tasks": [{
                    "label": "Publish the normalized Meetings import contract.",
                    "normalized_id": "task/import-contract"
                }],
                "topics": [{
                    "label": "Import shape",
                    "source_span_ids": ["span/not_1/transcript/0"],
                    "confidence_ppm": 990000,
                    "extractor": "granola-api"
                }]
            }]
        });
        let result = import_meetings_bytes(
            &mut loom,
            workspace_id,
            InputProfile::GranolaApi,
            serde_json::to_string(&input).unwrap().as_bytes(),
            false,
        )
        .unwrap();
        let snapshot = load_meetings_snapshot_io(&loom, &workspace_id.to_string())
            .unwrap()
            .unwrap();

        assert_eq!(result.report.rows_imported, 1);
        assert_eq!(result.report.operations_planned, 7);
        assert_eq!(snapshot.sources[0].source_id, "not_1");
        assert_eq!(snapshot.meetings[0].meeting_id, "meeting/not_1");
        assert_eq!(snapshot.meetings[0].source_refs, vec!["not_1"]);
        let transcript = snapshot
            .spans
            .iter()
            .find(|span| span.span_id == "span/not_1/transcript/0")
            .unwrap();
        assert!(transcript.text_digest.is_some());
        assert!(
            snapshot
                .spans
                .iter()
                .any(|span| span.span_id == "span/not_1/metadata/tasks/0")
        );
        assert_eq!(snapshot.annotations.len(), 2);
        assert_eq!(snapshot.annotations[0].kind, "Task");
        assert_eq!(
            snapshot.annotations[0].label,
            "Publish the normalized Meetings import contract."
        );
        assert_eq!(
            snapshot.annotations[0].source_span_ids,
            vec!["span/not_1/metadata/tasks/0"]
        );
        assert_eq!(
            snapshot.annotations[0].status,
            loom_substrate::meetings::AnnotationStatus::Observed
        );
        assert_eq!(snapshot.annotations[1].kind, "Topic");
        assert_eq!(
            snapshot.annotations[1].source_span_ids,
            vec!["span/not_1/transcript/0"]
        );
        assert_eq!(snapshot.annotations[1].confidence_ppm, Some(990000));
        assert_eq!(snapshot.import_runs[0].coverage_gaps, vec!["rate-limit"]);
    }

    #[test]
    fn meetings_import_command_writes_profile_snapshot() {
        let store_path = temp_store("meetings-import-write");
        let input_path = temp_store("meetings-import-input-json");
        FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
        let source_digest = Digest::hash(Algo::Blake3, b"source").to_string();
        let input = serde_json::json!({
            "snapshot_version": 1,
            "profile": "generic",
            "source_system": "generic",
            "source_scope": "team-notes",
            "observed_at": 200,
            "coverage": "complete",
            "items": [{
                "source_entity_id": "source-a",
                "source_digest": source_digest,
                "source_sidecar": {"raw": "source"},
                "title": "Planning",
                "summary_text": "Planning summary",
                "transcript_spans": [{"text": "Ship the import command."}]
            }]
        });
        std::fs::write(&input_path, serde_json::to_vec(&input).unwrap()).unwrap();

        run_meetings(
            MeetingsCmd::Import {
                store: store_path.clone(),
                workspace: "studio".to_string(),
                input_profile: "generic".to_string(),
                input: input_path.clone(),
                dry_run: false,
                report_format: "json".to_string(),
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let loom = loom_store::open_loom_read(&store_path).unwrap();
        let profiles = loom
            .store()
            .control_scan_prefix(MEETINGS_PROFILE_CONTROL_PREFIX.as_bytes())
            .unwrap();
        assert_eq!(profiles.len(), 1);
        let snapshot = MeetingsProfileSnapshot::decode(&profiles[0].1).unwrap();
        WorkspaceId::parse(&snapshot.workspace_id).unwrap();
        assert_eq!(snapshot.meetings[0].meeting_id, "meeting/source-a");
        assert_eq!(snapshot.spans[0].span_id, "span/source-a/0");
        assert_eq!(snapshot.import_runs[0].observed_ids, vec!["source-a"]);
        let profile_id = snapshot.workspace_id.clone();
        let workspace_id = WorkspaceId::parse(&profile_id).unwrap();
        let history_path = revision_index_path(&profile_id).unwrap();
        let history = loom
            .read_file_reserved(workspace_id, &history_path)
            .unwrap();
        let history = RevisionIndex::decode(&history).unwrap();
        let revisions = history.history("meeting:meeting/source-a");
        assert_eq!(revisions.len(), 1);
        assert_eq!(revisions[0].revision, 1);
        assert_eq!(
            revisions[0].body.media_type,
            "application/vnd.uldren.loom.meetings.meeting+cbor"
        );
        assert_eq!(history.checkpoints().len(), 1);
        assert_eq!(
            history.checkpoints()[0].checkpoint_id,
            "meeting:meeting/source-a:1"
        );
        assert_eq!(
            loom.read_file_reserved(
                workspace_id,
                &meetings_source_payload_path(&profile_id, "source-a", "source.json")
            )
            .unwrap(),
            br#"{"raw":"source"}"#
        );
        assert_eq!(
            loom.read_file_reserved(
                workspace_id,
                &meetings_source_payload_path(&profile_id, "source-a", "summary.txt")
            )
            .unwrap(),
            b"Planning summary"
        );
        assert_eq!(
            loom.read_file_reserved(
                workspace_id,
                &meetings_source_payload_path(&profile_id, "source-a", "transcript.jsonl")
            )
            .unwrap(),
            br#"{"language":null,"locator":"transcript/0","span_id":"span/source-a/0","speaker":null,"text":"Ship the import command."}
"#
        );
        drop(loom);
        let summary_out = temp_store("meetings-import-summary-out");
        run_meetings(
            MeetingsCmd::SourceRead {
                store: store_path.clone(),
                workspace: "studio".to_string(),
                source_id: "source-a".to_string(),
                leaf: "summary.txt".to_string(),
                out: Some(summary_out.clone()),
            },
            &KeyOpts::default(),
        )
        .unwrap();
        assert_eq!(std::fs::read(&summary_out).unwrap(), b"Planning summary");
        assert!(matches!(
            run_meetings(
                MeetingsCmd::SourceRead {
                    store: store_path.clone(),
                    workspace: "studio".to_string(),
                    source_id: "source-a".to_string(),
                    leaf: "../snapshot".to_string(),
                    out: None,
                },
                &KeyOpts::default(),
            ),
            Err(message) if message.contains("unsupported meetings source payload leaf")
        ));

        run_meetings(
            MeetingsCmd::List {
                store: store_path.clone(),
                workspace: "studio".to_string(),
                limit: 10,
                offset: 0,
                format: "json".to_string(),
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run_meetings(
            MeetingsCmd::Get {
                store: store_path.clone(),
                workspace: "studio".to_string(),
                meeting_id: "meeting/source-a".to_string(),
                format: "json".to_string(),
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run_meetings(
            MeetingsCmd::Import {
                store: store_path.clone(),
                workspace: "studio".to_string(),
                input_profile: "generic".to_string(),
                input: input_path,
                dry_run: false,
                report_format: "json".to_string(),
            },
            &KeyOpts::default(),
        )
        .unwrap();
        let loom = loom_store::open_loom_read(&store_path).unwrap();
        let history = loom
            .read_file_reserved(workspace_id, &history_path)
            .unwrap();
        let history = RevisionIndex::decode(&history).unwrap();
        assert_eq!(history.history("meeting:meeting/source-a").len(), 1);
    }

    #[test]
    fn inference_model_list_text_renderer_is_stable() {
        let model = loom_inference::curated_models()[0];
        let fit = ModelFitReport {
            model: model.model_ref(),
            runtime: model.runtime,
            runnable: false,
            reasons: vec![loom_types::ModelFitReason::RuntimeNotCompiled],
            estimated_memory_bytes: model.minimum_memory_bytes,
        };
        let rendered =
            render_curated_inference_models_text(&[CuratedInferenceModelView { model, fit }]);

        assert_eq!(
            rendered,
            concat!(
                "text-embedding\tsentence-transformers/all-MiniLM-L6-v2\tmain\t",
                "candle-safetensors\tfit=blocked:RuntimeNotCompiled\t",
                "Small Apache-2.0 embedding model with safetensors weights.\n",
                "files\tconfig.json,model.safetensors,special_tokens_map.json,",
                "tokenizer.json,tokenizer_config.json\n",
            )
        );
    }

    #[test]
    fn inference_model_list_json_renderer_has_model_and_fit() {
        let model = loom_inference::curated_models()[0];
        let fit = ModelFitReport {
            model: model.model_ref(),
            runtime: model.runtime,
            runnable: true,
            reasons: Vec::new(),
            estimated_memory_bytes: model.minimum_memory_bytes,
        };
        let rendered =
            render_curated_inference_models_json(&[CuratedInferenceModelView { model, fit }])
                .unwrap();
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        assert_eq!(
            value[0]["model"]["repo-id"],
            "sentence-transformers/all-MiniLM-L6-v2"
        );
        assert_eq!(value[0]["model"]["kind"], "text-embedding");
        assert_eq!(value[0]["fit"]["runnable"], true);
    }

    #[test]
    fn installed_inference_model_text_renderer_is_stable() {
        let record = loom_inference::InstalledModelRecord {
            model: ModelRef::new(
                InferenceModelKind::TextEmbedding,
                "sentence-transformers/all-MiniLM-L6-v2",
            )
            .with_revision(RevisionRef::Branch("main".to_string())),
            runtime: RuntimeKind::CandleSafetensors,
            files: vec![loom_inference::InstalledModelFile {
                relative_path: "snapshots/main/model.safetensors".to_string(),
                size_bytes: 42,
                digest: Some("sha256:abc".to_string()),
            }],
            active_provider_refs: vec!["vector:main/emb".to_string()],
        };

        assert_eq!(
            render_inference_model_record_text(&record),
            concat!(
                "text-embedding\tsentence-transformers/all-MiniLM-L6-v2\tmain\t",
                "candle-safetensors\n",
                "file\tsnapshots/main/model.safetensors\tbytes=42\tdigest=sha256:abc\n",
                "active\tvector:main/emb\n",
            )
        );
    }

    #[test]
    fn installed_inference_model_json_renderer_has_files() {
        let record = loom_inference::InstalledModelRecord {
            model: ModelRef::new(
                InferenceModelKind::TextEmbedding,
                "sentence-transformers/all-MiniLM-L6-v2",
            ),
            runtime: RuntimeKind::CandleSafetensors,
            files: vec![loom_inference::InstalledModelFile {
                relative_path: "snapshots/main/tokenizer.json".to_string(),
                size_bytes: 12,
                digest: Some("sha256:def".to_string()),
            }],
            active_provider_refs: Vec::new(),
        };
        let rendered = render_inference_model_record_json(&record).unwrap();
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        assert_eq!(value["runtime"], "candle-safetensors");
        assert_eq!(
            value["files"][0]["relative-path"],
            "snapshots/main/tokenizer.json"
        );
    }

    #[test]
    fn inference_instance_text_renderer_is_stable() {
        let mut overrides = BTreeMap::new();
        overrides.insert("batch_size".to_string(), "8".to_string());
        let instance = loom_inference::build_instance_descriptor(
            "fast-embed",
            InferenceModelKind::TextEmbedding,
            ModelRef::new(
                InferenceModelKind::TextEmbedding,
                "sentence-transformers/all-MiniLM-L6-v2",
            ),
            RuntimeKind::CandleSafetensors,
            Some("fast".to_string()),
            overrides,
        )
        .unwrap();
        let view = InferenceInstanceView {
            instance: &instance,
            refs: 2,
        };

        assert_eq!(
            render_inference_instance_text(&view, true),
            concat!(
                "fast-embed\ttext-embedding\tsentence-transformers/all-MiniLM-L6-v2\t",
                "candle-safetensors\tpreset=fast\trefs=2\n",
                "setting\tbatch_size=8\n",
                "resolved\tbatch_size=8\n",
                "resolved\teffort=fast\n",
                "resolved\tnormalize=true\n",
                "resolved\truntime=candle-safetensors\n",
            )
        );
    }

    #[test]
    fn inference_instance_json_renderer_has_refs_and_settings() {
        let instance = loom_inference::build_instance_descriptor(
            "chat-small",
            InferenceModelKind::Llm,
            ModelRef::new(InferenceModelKind::Llm, "Qwen/Qwen2.5-0.5B-Instruct"),
            RuntimeKind::CandleSafetensors,
            Some("deterministic".to_string()),
            BTreeMap::new(),
        )
        .unwrap();
        let view = InferenceInstanceView {
            instance: &instance,
            refs: 0,
        };
        let rendered = serde_json::to_string_pretty(&view).unwrap();
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        assert_eq!(value["instance"]["name"], "chat-small");
        assert_eq!(value["instance"]["kind"], "llm");
        assert_eq!(value["instance"]["resolved-settings"]["temperature"], "0");
        assert_eq!(value["refs"], 0);
    }

    #[test]
    fn vector_workspace_binding_json_renderer_is_stable() {
        let binding = loom_inference::VectorWorkspaceBinding {
            store: "store.loom".to_string(),
            workspace: "main".to_string(),
            embedding_instance: "fast-embed".to_string(),
        };
        let rendered = serde_json::to_string_pretty(&binding).unwrap();
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        assert_eq!(value["store"], "store.loom");
        assert_eq!(value["workspace"], "main");
        assert_eq!(value["embedding-instance"], "fast-embed");
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod store_copy_tests {
    use super::*;

    fn temp(tag: &str) -> String {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-store-copy-{tag}-{}-{}.loom",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&path);
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn store_copy_modifiers_parse_supported_values() {
        let modifiers =
            parse_store_copy_modifiers(&["fips".to_string(), "compacted".to_string()]).unwrap();
        assert!(modifiers.fips);
        assert!(modifiers.compacted);
        assert!(parse_store_copy_modifiers(&["unknown".to_string()]).is_err());
    }

    #[test]
    fn profile_changing_store_copy_rejects_dirty_workspace() {
        let store = temp("dirty");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        init_control_state(&fs).unwrap();
        let mut loom = open_loom_from(fs, &KeyOpts::default(), false).unwrap();
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("work"),
                WorkspaceId::from_bytes([9; 16]),
            )
            .unwrap();
        loom.write_file(ns, "draft.txt", b"draft", 0o100644)
            .unwrap();

        let err = ensure_store_copy_clean(&loom).unwrap_err();
        assert!(err.contains("uncommitted changes"));
        let _ = std::fs::remove_file(store);
    }
}

// default-feature CLI evidence for lane actor derivation. This module is gated only on
// `#[cfg(test)]` (not `integration-tests`), so it runs under `cargo test -p uldren-loom-cli lanes`.
#[cfg(test)]
mod mx250_lanes_cli_default_tests {
    use super::*;

    fn mx250_temp_store(tag: &str) -> String {
        let mut path = std::env::temp_dir();
        let unique = format!(
            "{tag}-{}-{}.loom",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        path.push(unique);
        path.to_string_lossy().into_owned()
    }

    // Routine lane CLI mutations no longer require --updated-by; the actor is derived from context
    // (namespace fallback when no identity is configured, as in a plain CLI store).
    #[test]
    fn lanes_cli_derives_actor_when_updated_by_omitted() {
        let store = mx250_temp_store("mx250-lanes-cli");
        run(
            Command::Store {
                action: StoreCmd::Init {
                    store: store.clone(),
                    encrypt: false,
                    suite: None,
                    identity_profile: None,
                    fips: false,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        run(
            Command::Workspace {
                action: WorkspaceCmd::Create {
                    store: store.clone(),
                    name: "main".to_string(),
                    facet: None,
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();

        // Derived actor this store resolves to when no override is supplied.
        let expected_actor = {
            let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
            let ns = resolve_ns(&loom, "main").unwrap();
            resolve_lane_actor(&loom, ns, None).unwrap()
        };
        assert!(
            !expected_actor.is_empty(),
            "derived actor must not be empty"
        );

        // Create a lane WITHOUT --updated-by: the argument is optional and the actor is derived.
        run(
            Command::Lanes {
                action: LanesCmd::Create {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    lane_id: "agent-derive".to_string(),
                    lane_key: "agent-derive".to_string(),
                    kind: "assignment".to_string(),
                    title: String::new(),
                    description: String::new(),
                    owner_principal: Some("agent:9".to_string()),
                    lane_status: "ready".to_string(),
                    active_ticket_id: None,
                    status_report: String::new(),
                    reviewer_feedback: String::new(),
                    updated_at: Some(1),
                    updated_by: None,
                    tickets: Vec::new(),
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let lane = loom_lanes::get_lane(&loom, ns, "agent-derive")
            .unwrap()
            .unwrap();
        assert_eq!(
            lane.updated_by, expected_actor,
            "create without --updated-by should record the derived actor"
        );
        drop(loom);

        // A routine mutation without --updated-by also records the derived actor.
        run(
            Command::Lanes {
                action: LanesCmd::Update {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    lane_id: "agent-derive".to_string(),
                    title: None,
                    description: None,
                    lane_status: None,
                    status_report: Some("working".to_string()),
                    reviewer_feedback: None,
                    updated_by: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let lane = loom_lanes::get_lane(&loom, ns, "agent-derive")
            .unwrap()
            .unwrap();
        assert_eq!(lane.status_report, "working");
        assert_eq!(
            lane.updated_by, expected_actor,
            "status-report update without --updated-by should record the derived actor"
        );

        let _ = std::fs::remove_file(&store);
    }
}
