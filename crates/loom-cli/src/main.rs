//! `loom` - the Uldren Loom command-line tool.
//!
//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Args, CommandFactory, Parser, Subcommand};
use gluesql_core::prelude::{Payload, Value as GValue};
#[cfg(all(test, feature = "integration-tests"))]
use loom_client::local::LocalLoomClient;
use loom_codec::Value as WireValue;
#[cfg(all(test, feature = "integration-tests"))]
use loom_core::EmbeddingModel;
#[cfg(test)]
use loom_core::Props;
use loom_core::keys::{EncryptionMeta, KeySpec, Suite};
#[cfg(test)]
use loom_core::tabular::ColumnType;
use loom_core::vector::{MetaFilter, Metric};
use loom_core::workspace::{FacetKind, WorkspaceId};
use loom_core::{
    AclDomain, AclEffect, AclGrant, AclPredicate, AclRight, AclScope, AclScopeKind, AclStore,
    AclSubject, Algo, AppCredential, Code, Digest, EphemeralPutOptions, ExternalCredential,
    ExternalCredentialKind, FieldValue, FileKind, IdentityRole, IdentityStore, KvMapConfig, KvTier,
    LiveRootDiagnostics, LockCoordinator, LockOwner, Loom, MergeOutcome, Object, ObjectStore,
    Principal, PrincipalKind, ProtectedRefPolicy, VERSION, WsSelector, bundle_export,
    clone_workspace, inference_instance_state, migrate_workspace_profile, search_collections,
};
#[cfg(feature = "inference-native-hf")]
use loom_inference::DownloadEvent;
use loom_inference::{DownloadJobManager, DownloadJobPlan};
use loom_interchange::ArchiveKind;
#[cfg(all(test, feature = "integration-tests"))]
use loom_interchange_io::TableImportMode;
use loom_interchange_io::{
    ArchiveExportOptions, ArchiveExportResult, ArchiveImportResult, CarExportOptions,
    CarExportResult, CarImportResult, FsExportOptions, TableCsvExportOptions, export_archive,
    export_car, export_fs, export_table_csv, import_report_from_json, input_profile_label,
    load_meetings_snapshot as load_meetings_snapshot_io, parse_meetings_input_profile,
};
use loom_lanes::{Lane, LaneDiagnostic, LaneKind, LaneStatus, LaneView};
use loom_remote_protocol::api_types::Digest as GeneratedDigest;
use loom_remote_protocol::codec::ToValue;
#[cfg(all(test, feature = "integration-tests"))]
use loom_store::GcSegmentBudget;
use loom_store::{
    DerivedArtifactRebuild, DerivedArtifactRecord, DerivedArtifactStatus, FileStore, LocalOpenAuth,
    ServedListenerRecord, StoreMaintenanceReport, StoreMaintenanceRunState, StorePolicy, daemon,
    gc_loom, open_loom_read_unlocked, save_loom,
};
#[cfg(all(test, feature = "integration-tests"))]
use loom_substrate::OperationEnvelope;
#[cfg(all(test, feature = "integration-tests"))]
use loom_substrate::body::BlockKind;
#[cfg(all(test, feature = "integration-tests"))]
use loom_substrate::drive::{DriveOperationLog, drive_operation_log_key};
use loom_substrate::drive::{DrivePolicyRegistry, drive_policy_registry_key};
#[cfg(all(test, feature = "integration-tests"))]
use loom_substrate::lifecycle::{LifecycleOperationLog, lifecycle_operation_log_key};
#[cfg(all(test, feature = "integration-tests"))]
use loom_substrate::meetings::PROFILE_CONTROL_PREFIX as MEETINGS_PROFILE_CONTROL_PREFIX;
use loom_substrate::meetings::{
    AnnotationRecord, AnnotationStatus, MeetingRecord, MeetingStatus, MeetingsProfileSnapshot,
};
#[cfg(all(test, feature = "integration-tests"))]
use loom_substrate::meetings::{
    Coverage as MeetingsCoverage, MeetingRecordInput, MeetingsProfileSnapshotParts,
    ProjectionAction, ProjectionKind, ProjectionOutput, ProjectionOutputSet, SourceRecord,
    SourceRecordInput, SpanKind, SpanRecord, meetings_profile_key,
};
#[cfg(all(test, feature = "integration-tests"))]
use loom_substrate::pages::{PageOperationLog, page_profile_operation_log_key};
#[cfg(all(test, feature = "integration-tests"))]
use loom_substrate::search::{
    EMBEDDING_PROJECTION_JOBS_DIR, EmbeddingProjectionJob, EmbeddingProjectionKey,
    EmbeddingProjectionStamp,
};
use loom_substrate::surfaces::{
    SurfaceAppDefinition, core_surface_catalog, meeting_memory_surface_catalog,
    surface_app_catalog, surface_catalog_json,
};
#[cfg(all(test, feature = "integration-tests"))]
use loom_substrate::versioning::{
    BodyRef, RevisionBackfillUpdate, RevisionIndex, load_optional_current_revision_index,
    persist_current_revision_index,
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
over the remote Loom while document/graph ref-index (edge) writes, the timestamped VCS writes, and other tools return a clear not-yet/local-only error. \
Local MCP uses the daemon-owned generated boundary and rejects `--stateless`; remote MCP statefulness is owned by the remote endpoint.",
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

fn vector_metric_wire_tag(metric: Metric) -> i64 {
    match metric {
        Metric::Cosine => 1,
        Metric::L2 => 2,
        Metric::Dot => 3,
    }
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

fn string_list_from_cbor(bytes: &[u8]) -> Result<Vec<String>, String> {
    let WireValue::Array(items) = loom_codec::decode(bytes).map_err(|e| e.to_string())? else {
        return Err("expected a CBOR text array".to_string());
    };
    items
        .into_iter()
        .map(|item| match item {
            WireValue::Text(text) => Ok(text),
            other => Err(format!("expected text list item, found {other:?}")),
        })
        .collect()
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

fn text_array_cbor(items: &[String]) -> Result<Vec<u8>, String> {
    loom_codec::encode(&WireValue::Array(
        items.iter().cloned().map(WireValue::Text).collect(),
    ))
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
            let meta = loom_core::calendar::CollectionMeta {
                display_name,
                component_set,
            };
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Calendar",
                "create_collection",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    collection.to_value(),
                    WireValue::Bytes(meta.encode()),
                ],
            )
        }
        CalendarCmd::DeleteCollection {
            store,
            workspace,
            principal,
            collection,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let present = execute_generated_bool(
                &client,
                "Calendar",
                "delete_collection",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    collection.to_value(),
                ],
            )?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let present = execute_generated_bool(
                &client,
                "Calendar",
                "delete_entry",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    collection.to_value(),
                    uid.to_value(),
                ],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Calendar",
                "get_collection",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    collection.to_value(),
                ],
            )?
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Calendar",
                "get_entry",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    collection.to_value(),
                    uid.to_value(),
                ],
            )?
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Calendar",
                "list_collections",
                vec![workspace.to_value(), principal.to_value()],
            )?;
            if let Some(out) = out {
                write_output(Some(&out), &encoded).map_err(|e| e.to_string())
            } else {
                let collections = string_list_from_cbor(&encoded)?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Calendar",
                "list_entries",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    collection.to_value(),
                ],
            )?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let etag = execute_generated_digest_string(
                &client,
                "Calendar",
                "put_entry",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    collection.to_value(),
                    WireValue::Bytes(bytes),
                ],
            )?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let etag = execute_generated_digest_string(
                &client,
                "Calendar",
                "put_ics",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    collection.to_value(),
                    ics.to_value(),
                ],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Calendar",
                "range",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    collection.to_value(),
                    from.to_value(),
                    to.to_value(),
                ],
            )?;
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
            let component = component
                .map(|component| component.as_str().to_string())
                .unwrap_or_default();
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Calendar",
                "search",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    collection.to_value(),
                    component.to_value(),
                    text.to_value(),
                ],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Calendar",
                "to_ics",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    collection.to_value(),
                    uid.to_value(),
                ],
            )?
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let present = execute_generated_bool(
                &client,
                "Cas",
                "delete",
                vec![
                    workspace.to_value(),
                    loom_remote_protocol::api_types::Digest(digest.to_string()).to_value(),
                ],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Cas",
                "get",
                vec![
                    workspace.to_value(),
                    GeneratedDigest(digest.to_string()).to_value(),
                ],
            )?
            else {
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let present = execute_generated_bool(
                &client,
                "Cas",
                "has",
                vec![
                    workspace.to_value(),
                    GeneratedDigest(digest.to_string()).to_value(),
                ],
            )?;
            println!("{present}");
            Ok(())
        }
        CasCmd::List {
            store,
            workspace,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let digests =
                execute_generated_digest_list(&client, "Cas", "list", vec![workspace.to_value()])?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let digest = execute_generated_digest_string(
                &client,
                "Cas",
                "put",
                vec![workspace.to_value(), WireValue::Bytes(bytes)],
            )?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let present = client.doc_delete(&workspace, &collection, &id)?;
            println!("{present}");
            Ok(())
        }
        DocumentCmd::DeleteCollection {
            store,
            workspace,
            collection,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let present = client.doc_delete_collection(&workspace, &collection)?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(document) = client.doc_get_text(&workspace, &collection, &id)? else {
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let result = client.doc_put_text(
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(document) = client.doc_get_binary(&workspace, &collection, &id)? else {
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let result = client.doc_put_binary(
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = client.doc_list_binary(&workspace, &collection)?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        DocumentCmd::Find {
            store,
            workspace,
            collection,
            index,
            value_json,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let ids = client.doc_find(&workspace, &collection, &index, &value_json)?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let result = client.doc_query(&workspace, &collection, bytes)?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            client.doc_index_create(&workspace, &collection, &name, &path, unique)
        }
        DocumentCmd::IndexCreateJson {
            store,
            workspace,
            collection,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            client.doc_index_create_json(&workspace, &collection, bytes)
        }
        DocumentCmd::IndexDrop {
            store,
            workspace,
            collection,
            name,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let dropped = client.doc_index_drop(&workspace, &collection, &name)?;
            println!("{dropped}");
            Ok(())
        }
        DocumentCmd::IndexList {
            store,
            workspace,
            collection,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            println!("{}", client.doc_index_list(&workspace, &collection)?);
            Ok(())
        }
        DocumentCmd::IndexRebuild {
            store,
            workspace,
            collection,
            name,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            client.doc_index_rebuild(&workspace, &collection, &name)
        }
        DocumentCmd::IndexStatus {
            store,
            workspace,
            collection,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            println!("{}", client.doc_index_statuses(&workspace, &collection)?);
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
            let meta = loom_core::contacts::BookMeta { display_name };
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Contacts",
                "create_book",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    book.to_value(),
                    WireValue::Bytes(meta.encode()),
                ],
            )
        }
        ContactsCmd::DeleteBook {
            store,
            workspace,
            principal,
            book,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let present = execute_generated_bool(
                &client,
                "Contacts",
                "delete_book",
                vec![workspace.to_value(), principal.to_value(), book.to_value()],
            )?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let present = execute_generated_bool(
                &client,
                "Contacts",
                "delete_entry",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    book.to_value(),
                    uid.to_value(),
                ],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Contacts",
                "get_book",
                vec![workspace.to_value(), principal.to_value(), book.to_value()],
            )?
            else {
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Contacts",
                "get_entry",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    book.to_value(),
                    uid.to_value(),
                ],
            )?
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Contacts",
                "list_books",
                vec![workspace.to_value(), principal.to_value()],
            )?;
            if let Some(out) = out {
                write_output(Some(&out), &encoded).map_err(|e| e.to_string())
            } else {
                let books = string_list_from_cbor(&encoded)?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Contacts",
                "list_entries",
                vec![workspace.to_value(), principal.to_value(), book.to_value()],
            )?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let etag = execute_generated_digest_string(
                &client,
                "Contacts",
                "put_entry",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    book.to_value(),
                    WireValue::Bytes(bytes),
                ],
            )?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let etag = execute_generated_digest_string(
                &client,
                "Contacts",
                "put_vcard",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    book.to_value(),
                    vcard.to_value(),
                ],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Contacts",
                "search",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    book.to_value(),
                    text.to_value(),
                ],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Contacts",
                "to_vcard",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    book.to_value(),
                    uid.to_value(),
                ],
            )?
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
            let key = loom_core::kv::key_to_cbor(&key);
            let client = remote::open_cli_generated_client(&store, keys)?;
            let present = execute_generated_bool(
                &client,
                "Kv",
                "delete",
                vec![
                    workspace.to_value(),
                    collection.to_value(),
                    WireValue::Bytes(key),
                ],
            )?;
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
            let key = loom_core::kv::key_to_cbor(&key);
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Kv",
                "get",
                vec![
                    workspace.to_value(),
                    collection.to_value(),
                    WireValue::Bytes(key),
                ],
            )?
            else {
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Kv",
                "list",
                vec![workspace.to_value(), collection.to_value()],
            )?;
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
            let key = loom_core::kv::key_to_cbor(&key);
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Kv",
                "put",
                vec![
                    workspace.to_value(),
                    collection.to_value(),
                    WireValue::Bytes(key),
                    WireValue::Bytes(bytes),
                ],
            )
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
            let from = loom_core::kv::key_to_cbor(&from);
            let to = loom_core::kv::key_to_cbor(&to);
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Kv",
                "range",
                vec![
                    workspace.to_value(),
                    collection.to_value(),
                    WireValue::Bytes(from),
                    WireValue::Bytes(to),
                ],
            )?;
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
            let meta = loom_core::mail::MailboxMeta { display_name };
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Mail",
                "create_mailbox",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    mailbox.to_value(),
                    WireValue::Bytes(meta.encode()),
                ],
            )
        }
        MailCmd::DeleteMailbox {
            store,
            workspace,
            principal,
            mailbox,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let present = execute_generated_bool(
                &client,
                "Mail",
                "delete_mailbox",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    mailbox.to_value(),
                ],
            )?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let present = execute_generated_bool(
                &client,
                "Mail",
                "delete_message",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    mailbox.to_value(),
                    uid.to_value(),
                ],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Mail",
                "get_flags",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    mailbox.to_value(),
                    uid.to_value(),
                ],
            )?;
            if let Some(out) = out {
                write_output(Some(&out), &encoded).map_err(|e| e.to_string())
            } else {
                let flags = string_list_from_cbor(&encoded)?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Mail",
                "get_mailbox",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    mailbox.to_value(),
                ],
            )?
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Mail",
                "get_message",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    mailbox.to_value(),
                    uid.to_value(),
                ],
            )?
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let digest = execute_generated_digest_string(
                &client,
                "Mail",
                "ingest_message",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    mailbox.to_value(),
                    uid.to_value(),
                    WireValue::Bytes(bytes),
                ],
            )?;
            println!("{digest}");
            Ok(())
        }
        MailCmd::ListMailboxes {
            store,
            workspace,
            principal,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Mail",
                "list_mailboxes",
                vec![workspace.to_value(), principal.to_value()],
            )?;
            if let Some(out) = out {
                write_output(Some(&out), &encoded).map_err(|e| e.to_string())
            } else {
                let mailboxes = string_list_from_cbor(&encoded)?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Mail",
                "list_messages",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    mailbox.to_value(),
                ],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Mail",
                "search",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    mailbox.to_value(),
                    text.to_value(),
                ],
            )?;
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
            let encoded = text_array_cbor(&flags)?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Mail",
                "set_flags",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    mailbox.to_value(),
                    uid.to_value(),
                    WireValue::Bytes(encoded),
                ],
            )
        }
        MailCmd::ToEml {
            store,
            workspace,
            principal,
            mailbox,
            uid,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Mail",
                "to_eml",
                vec![
                    workspace.to_value(),
                    principal.to_value(),
                    mailbox.to_value(),
                    uid.to_value(),
                ],
            )?
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let bytes = execute_generated_bytes(
                &client,
                "Meetings",
                "meetings_source_read",
                vec![workspace.to_value(), source_id.to_value(), leaf.to_value()],
            )?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let report_json = execute_generated_string(
                &client,
                "Meetings",
                "meetings_import_snapshot",
                vec![
                    workspace.to_value(),
                    input_profile_label(input_profile).to_value(),
                    WireValue::Bytes(bytes),
                    dry_run.to_value(),
                ],
            )?;
            let report = import_report_from_json(&report_json).map_err(|e| e.to_string())?;
            print_import_report(&report, &report_format)
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let workspace_id = client.ensure_workspace_id(&workspace, FacetKind::Vcs)?;
            let profile_id = workspace_id.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_project_create_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    project_id.to_value(),
                    key_prefix.to_value(),
                    name.to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_ticket_project_json(&raw, &format)
        }
        TicketsCmd::ProjectRekey {
            store,
            workspace,
            project_id,
            key_prefix,
            expected_root,
            format,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_project_rekey_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    project_id.to_value(),
                    key_prefix.to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_ticket_project_json(&raw, &format)
        }
        TicketsCmd::ProjectSettingsGet {
            store,
            workspace,
            project_id,
            include_contracts,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_project_settings_get_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    project_id.to_value(),
                    include_contracts.to_value(),
                ],
            )?;
            print_generated_ticket_project_json(&raw, &format)
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
            required_acceptance_reviews,
            replace_required_acceptance_reviews,
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
            let required_acceptance_reviews = required_acceptance_reviews
                .iter()
                .map(|review| loom_tickets::TicketReviewType::parse(review))
                .collect::<loom_core::Result<Vec<_>>>()
                .map_err(|e| e.to_string())?;
            let required_acceptance_reviews = if replace_required_acceptance_reviews {
                Some(required_acceptance_reviews.as_slice())
            } else {
                None
            };
            // Contract summaries/details accept `@path` to load the value from a file (markdown/text
            // can be large); `@@` escapes a literal leading `@`.
            let owner_contract_summary = expand_at_file_opt(owner_contract_summary)?;
            let owner_contract_details = expand_at_file_opt(owner_contract_details)?;
            let worker_contract_summary = expand_at_file_opt(worker_contract_summary)?;
            let worker_contract_details = expand_at_file_opt(worker_contract_details)?;
            let patch = ticket_project_settings_patch_to_cbor(TicketProjectSettingsPatchArgs {
                default_projection,
                enable_projections: &[],
                disable_projections: &[],
                actor_enforcement,
                project_owner_principal: project_owner.as_deref(),
                clear_project_owner_principal: clear_project_owner,
                acceptance_authorities,
                acceptance_evidence_enforcement,
                required_acceptance_evidence_keys,
                required_acceptance_reviews,
                owner_contract_summary: owner_contract_summary.as_deref(),
                owner_contract_details: owner_contract_details.as_deref(),
                worker_contract_summary: worker_contract_summary.as_deref(),
                worker_contract_details: worker_contract_details.as_deref(),
                expected_root: expected_root.as_deref(),
            })?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_project_settings_set_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    project_id.to_value(),
                    WireValue::Bytes(patch),
                ],
            )?;
            print_generated_ticket_project_json(&raw, &format)
        }
        TicketsCmd::Projects {
            store,
            workspace,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_projects_json",
                vec![workspace.to_string().to_value(), profile_id.to_value()],
            )?;
            print_generated_ticket_projects_json(&raw, &format)
        }
        TicketsCmd::Relations {
            store,
            workspace,
            ticket_id,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_relation_list_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    ticket_id.to_string().to_value(),
                ],
            )?;
            print_generated_ticket_relations_json(&raw, &format)
        }
        TicketsCmd::Fields {
            store,
            workspace,
            project_id,
            projection,
            operation,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_fields_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    project_id.map(|value| value.to_string()).to_value(),
                    projection.map(|value| value.to_string()).to_value(),
                    operation.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_ticket_field_catalog_json(&raw, &format)
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
            parse_ticket_field_cardinality(&cardinality)?;
            let applicable_type_ids_json =
                serde_json::to_string(&applicable_type_ids).map_err(|e| e.to_string())?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_field_put_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    project_id.to_value(),
                    field_id.to_value(),
                    key.to_value(),
                    name.to_value(),
                    description.map(|value| value.to_string()).to_value(),
                    field_type.to_value(),
                    option_set.map(|value| value.to_string()).to_value(),
                    max_length.unwrap_or(0).to_value(),
                    max_length.is_some().to_value(),
                    required.to_value(),
                    searchable.to_value(),
                    orderable.to_value(),
                    cardinality.to_value(),
                    applicable_type_ids_json.to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_ticket_field_catalog_json(&raw, &format)
        }
        TicketsCmd::FieldRetire {
            store,
            workspace,
            project_id,
            field_id,
            expected_root,
            format,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_field_retire_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    project_id.to_value(),
                    field_id.to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_ticket_field_catalog_json(&raw, &format)
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let projects_raw = client.generated_json(
                "Tickets",
                "tickets_projects_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.clone().to_value(),
                ],
            )?;
            let projects: serde_json::Value =
                serde_json::from_str(&projects_raw).map_err(|e| e.to_string())?;
            let projects = projects
                .as_array()
                .ok_or_else(|| "ticket projects JSON must be an array".to_string())?;
            let project = match project_id {
                Some(project_id) => project_id,
                None => match projects.as_slice() {
                    [only] => json_string_field(only, "project_id")?.to_string(),
                    [] => {
                        return Err("workspace has no ticket projects; create one with `tickets project create` or pass --project-id".to_string());
                    }
                    _ => {
                        return Err(
                            "workspace has multiple ticket projects; specify --project-id"
                                .to_string(),
                        );
                    }
                },
            };
            let projection = match projection.as_deref() {
                Some(projection) => loom_tickets::parse_ticket_projection(Some(projection))
                    .map_err(|e| e.to_string())?,
                None => {
                    let project_summary = projects
                        .iter()
                        .find(|summary| {
                            summary
                                .get("project_id")
                                .and_then(serde_json::Value::as_str)
                                == Some(project.as_str())
                        })
                        .ok_or_else(|| "ticket project not found".to_string())?;
                    loom_tickets::parse_ticket_projection(Some(json_string_field(
                        project_summary,
                        "default_projection",
                    )?))
                    .map_err(|e| e.to_string())?
                }
            };
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
            let raw = client.generated_json(
                "Tickets",
                "tickets_create_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    project.to_value(),
                    ticket_type.to_value(),
                    external_source.to_value(),
                    external_id.to_value(),
                    fields.to_string().to_value(),
                    serde_json::to_string(&policy_labels)
                        .map_err(|e| e.to_string())?
                        .to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_ticket_mutation_json(&raw, &format)
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
            let comment_evidence_json = request
                .comment
                .as_ref()
                .and_then(|comment| comment.evidence.as_ref())
                .map(serde_json::to_string)
                .transpose()
                .map_err(|e| e.to_string())?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&request.workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_update_json",
                vec![
                    request.workspace.to_string().to_value(),
                    profile_id.to_value(),
                    request.ticket_id.to_string().to_value(),
                    set_fields.map(|fields| fields.to_string()).to_value(),
                    serde_json::to_string(&delete_fields)
                        .map_err(|e| e.to_string())?
                        .to_value(),
                    request.action.map(|value| value.to_string()).to_value(),
                    request
                        .target_status
                        .map(|value| value.to_string())
                        .to_value(),
                    request
                        .observed_source_status
                        .map(|value| value.to_string())
                        .to_value(),
                    request
                        .observed_workflow_version
                        .map(|value| value.to_string())
                        .to_value(),
                    request.assignee.map(|value| value.to_string()).to_value(),
                    request
                        .comment
                        .as_ref()
                        .and_then(|comment| comment.comment_id.clone())
                        .to_value(),
                    request
                        .comment
                        .as_ref()
                        .and_then(|comment| comment.comment_type.clone())
                        .to_value(),
                    request
                        .comment
                        .as_ref()
                        .map(|comment| comment.body.clone())
                        .to_value(),
                    comment_evidence_json.to_value(),
                    request
                        .expected_root
                        .map(|value| value.to_string())
                        .to_value(),
                    (!request.comments.is_empty())
                        .then(|| serde_json::to_string(&request.comments))
                        .transpose()
                        .map_err(|e| e.to_string())?
                        .to_value(),
                    (!request.relation_sets.is_empty())
                        .then(|| serde_json::to_string(&request.relation_sets))
                        .transpose()
                        .map_err(|e| e.to_string())?
                        .to_value(),
                    (!request.relation_removes.is_empty())
                        .then(|| serde_json::to_string(&request.relation_removes))
                        .transpose()
                        .map_err(|e| e.to_string())?
                        .to_value(),
                ],
            )?;
            print_generated_ticket_mutation_json(&raw, &format)
        }
        TicketsCmd::Delete {
            store,
            workspace,
            ticket_id,
            expected_root,
            format,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_delete_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    ticket_id.to_string().to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_ticket_mutation_json(&raw, &format)
        }
        TicketsCmd::Comments {
            store,
            workspace,
            ticket_id,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_comments_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    ticket_id.to_string().to_value(),
                ],
            )?;
            print_generated_ticket_comments_json(&raw, &format)
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
            let body = read_ticket_comment_body(&body)?;
            let evidence = evidence
                .as_deref()
                .map(parse_ticket_comment_evidence)
                .transpose()?;
            let evidence_json = evidence
                .as_ref()
                .map(|evidence| serde_json::to_string(&evidence).map_err(|e| e.to_string()))
                .transpose()?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_comment_add_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    ticket_id.to_string().to_value(),
                    comment_id.map(|value| value.to_string()).to_value(),
                    Some(comment_type).to_value(),
                    body.to_value(),
                    evidence_json.to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_ticket_mutation_json(&raw, &format)
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
            let body = body.as_deref().map(read_ticket_comment_body).transpose()?;
            let evidence = evidence
                .as_deref()
                .map(parse_ticket_comment_evidence_update)
                .transpose()?;
            let evidence_json = evidence
                .as_ref()
                .map(|evidence| {
                    evidence
                        .as_ref()
                        .map_or_else(|| Ok("null".to_string()), serde_json::to_string)
                        .map_err(|e| e.to_string())
                })
                .transpose()?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_comment_update_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    ticket_id.to_string().to_value(),
                    comment_id.to_string().to_value(),
                    comment_type.map(|value| value.to_string()).to_value(),
                    body.to_value(),
                    evidence_json.to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_ticket_mutation_json(&raw, &format)
        }
        TicketsCmd::CommentDelete {
            store,
            workspace,
            ticket_id,
            comment_id,
            expected_root,
            format,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_comment_delete_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    ticket_id.to_string().to_value(),
                    comment_id.to_string().to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_ticket_mutation_json(&raw, &format)
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
            let columns = parse_board_columns(&columns)?;
            loom_tickets::BoardMode::parse(&mode).map_err(|e| e.to_string())?;
            let request = serde_json::json!({
                "board_id": board_id,
                "board_key": board_key,
                "name": name,
                "description": description,
                "project_id": project_id,
                "mode": mode,
                "columns": board_columns_json(&columns),
                "card_display_fields": card_display_fields,
                "updated_by": updated_by,
                "expected_root": expected_root,
            });
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "boards_create_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    request.to_string().to_value(),
                ],
            )?;
            print_generated_board_json(&raw, &format)
        }
        TicketsCmd::BoardGet {
            store,
            workspace,
            board_id,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "boards_get_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    board_id.to_string().to_value(),
                ],
            )?;
            print_generated_board_json(&raw, &format)
        }
        TicketsCmd::BoardList {
            store,
            workspace,
            include_deleted,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "boards_list_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    include_deleted.to_value(),
                ],
            )?;
            print_generated_boards_json(&raw, &format)
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
            board_status
                .as_deref()
                .map(loom_tickets::BoardStatus::parse)
                .transpose()
                .map_err(|e| e.to_string())?;
            let card_display_fields = if card_display_fields.is_empty() {
                None
            } else {
                Some(card_display_fields)
            };
            let request = serde_json::json!({
                "board_key": board_key,
                "name": name,
                "description": description,
                "board_status": board_status,
                "card_display_fields": card_display_fields,
                "updated_by": updated_by,
                "expected_root": expected_root,
            });
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "boards_update_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    board_id.to_string().to_value(),
                    request.to_string().to_value(),
                ],
            )?;
            print_generated_board_json(&raw, &format)
        }
        TicketsCmd::BoardDelete {
            store,
            workspace,
            board_id,
            updated_by,
            expected_root,
            format,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "boards_delete_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    board_id.to_string().to_value(),
                    updated_by.to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_board_json(&raw, &format)
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
            let columns = parse_board_columns(&columns)?;
            let mode = mode
                .as_deref()
                .map(loom_tickets::BoardMode::parse)
                .transpose()
                .map_err(|e| e.to_string())?;
            let request = serde_json::json!({
                "mode": mode.map(loom_tickets::BoardMode::as_str),
                "columns": board_columns_json(&columns),
                "updated_by": updated_by,
                "expected_root": expected_root,
            });
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "boards_configure_columns_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    board_id.to_string().to_value(),
                    request.to_string().to_value(),
                ],
            )?;
            print_generated_board_json(&raw, &format)
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
            let request = serde_json::json!({
                "ticket_id": ticket_id,
                "column_id": column_id,
                "rank_token": rank_token,
                "swimlane_id": swimlane_id,
                "updated_by": updated_by,
                "expected_root": expected_root,
            });
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "boards_move_card_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    board_id.to_string().to_value(),
                    request.to_string().to_value(),
                ],
            )?;
            print_generated_board_json(&raw, &format)
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_relation_set_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    ticket_id.to_string().to_value(),
                    relation_id.map(|value| value.to_string()).to_value(),
                    kind.to_value(),
                    target_id.to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_ticket_relation_mutation_json(&raw, &format)
        }
        TicketsCmd::RelationRemove {
            store,
            workspace,
            ticket_id,
            relation_id,
            expected_root,
            format,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_relation_remove_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    ticket_id.to_string().to_value(),
                    relation_id.to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_ticket_relation_mutation_json(&raw, &format)
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let request = serde_json::json!({
                "projection": projection,
                "statuses": statuses,
                "assignees": assignees,
                "priorities": priorities,
                "ticket_types": ticket_types,
                "labels": labels,
                "policy_labels": policy_labels,
                "lane": lane,
                "board": board,
                "ready": ready,
                "include_completed": include_completed,
                "limit": limit,
                "cursor": cursor,
            });
            let raw = client.generated_json(
                "Tickets",
                "tickets_list_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    Some(request.to_string()).to_value(),
                ],
            )?;
            print_generated_ticket_page_json(&raw, &format)
        }
        TicketsCmd::Get {
            store,
            workspace,
            ticket_id,
            projection,
            detailed,
            compact,
            format,
        } => {
            loom_tickets::parse_ticket_projection(projection.as_deref())
                .map_err(|e| e.to_string())?;
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let ticket_raw = client.generated_json(
                "Tickets",
                "tickets_get_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.clone().to_value(),
                    ticket_id.to_string().to_value(),
                    projection.map(|value| value.to_string()).to_value(),
                ],
            )?;
            let ticket: serde_json::Value =
                serde_json::from_str(&ticket_raw).map_err(|e| e.to_string())?;
            let primary_key = generated_ticket_text(&ticket, "primary_key")?.to_string();
            let history_raw = client.generated_json(
                "Tickets",
                "tickets_history_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.clone().to_value(),
                    Some(primary_key).to_value(),
                ],
            )?;
            let comments_raw = client.generated_json(
                "Tickets",
                "tickets_comments_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    ticket_id.to_string().to_value(),
                ],
            )?;
            print_generated_ticket_detail_json(
                &ticket,
                &history_raw,
                &comments_raw,
                detailed,
                compact,
                &format,
            )
        }
        TicketsCmd::History {
            store,
            workspace,
            ticket_id,
            detailed,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Tickets",
                "tickets_history_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    ticket_id.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_ticket_history_json(&raw, detailed, &format)
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let actor = resolve_generated_lane_actor(updated_by.as_deref())?;
            let lane_tickets =
                loom_lanes::lane_tickets_from_order(&tickets).map_err(|e| e.to_string())?;
            // Large-text lane fields accept `@path` to load from a file (`@@` escapes a literal @).
            let description = expand_at_file(&description)?;
            let status_report = expand_at_file(&status_report)?;
            let reviewer_feedback = expand_at_file(&reviewer_feedback)?;
            let lane_kind = LaneKind::parse(&kind).map_err(|e| e.to_string())?;
            let lane_status = LaneStatus::parse(&lane_status).map_err(|e| e.to_string())?;
            let lane = Lane {
                lane_id,
                lane_key,
                title,
                description,
                lane_kind: lane_kind.as_str().to_string(),
                owner_principal,
                lane_status: lane_status.as_str().to_string(),
                lane_tickets,
                active_ticket_id,
                status_report,
                reviewer_feedback,
                updated_at: updated_at.unwrap_or(current_time_ms()?),
                updated_by: actor,
            };
            let lane = client.lanes_create(&workspace, lane)?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            if detailed {
                let ticket_workspace_id = client.resolve_workspace_id(&workspace)?.to_string();
                let raw =
                    client.lanes_get_view_json(&workspace, &ticket_workspace_id, &lane_id, true)?;
                let view: Option<LaneView> =
                    serde_json::from_str(&raw).map_err(|e| e.to_string())?;
                let view = view.ok_or_else(|| "lane not found".to_string())?;
                print_lane_view(&view, &format, true)
            } else {
                let lane = client
                    .lanes_get(&workspace, &lane_id)?
                    .ok_or_else(|| "lane not found".to_string())?;
                print_lane(&lane, &format)
            }
        }
        LanesCmd::List {
            store,
            workspace,
            detailed,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let ticket_workspace_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.lanes_list_views_json(&workspace, &ticket_workspace_id, true)?;
            let views: Vec<LaneView> = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            print_lane_views(&views, &[], &format, detailed)
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
            // Large-text lane fields accept `@path` to load from a file (`@@` escapes a literal @).
            let description = expand_at_file_opt(description)?;
            let status_report = expand_at_file_opt(status_report)?;
            let reviewer_feedback = expand_at_file_opt(reviewer_feedback)?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let actor = resolve_generated_lane_actor(updated_by.as_deref())?;
            let lane = client.lanes_update(
                &workspace,
                &lane_id,
                title.as_deref(),
                description.as_deref(),
                lane_status.as_deref(),
                status_report.as_deref(),
                reviewer_feedback.as_deref(),
                &actor,
            )?;
            let envelope = lane_mutation_envelope(lane, "lane.updated", changes);
            print_lane_mutation(&envelope, &format)
        }
        LanesCmd::Closeout {
            store,
            workspace,
            lane_id,
            ticket_workspace_id,
            ticket_id,
            comment_type,
            comment_body,
            evidence,
            status_report,
            updated_by,
            format,
        } => {
            // Comment body and status summary accept `@path` to load from a file (`@@` escapes @).
            let comment_body = expand_at_file(&comment_body)?;
            let status_report = expand_at_file(&status_report)?;
            if comment_body.trim().is_empty() {
                return Err("lane closeout requires a non-empty comment body".to_string());
            }
            let evidence_json = match evidence {
                Some(json) => {
                    let value: serde_json::Value = serde_json::from_str(&json)
                        .map_err(|e| format!("invalid --evidence json: {e}"))?;
                    loom_tickets::TicketCommentEvidence::from_json(&value)
                        .map_err(|e| e.to_string())?;
                    Some(value.to_string())
                }
                None => None,
            };
            let client = remote::open_cli_generated_client(&store, keys)?;
            let actor = resolve_generated_lane_actor(updated_by.as_deref())?;
            let lane = client.lanes_closeout(remote::LaneCloseoutArgs {
                workspace: &workspace,
                lane_id: &lane_id,
                ticket_workspace_id: &ticket_workspace_id,
                ticket_id: &ticket_id,
                comment_type: &comment_type,
                comment_body: &comment_body,
                evidence_json: evidence_json.as_deref(),
                status_report: &status_report,
                updated_by: &actor,
                expected_root: None,
            })?;
            let changes = vec![
                MutationChange::field_set("comment_type", comment_type),
                MutationChange::field_set("status_report", status_report),
            ];
            let envelope = lane_mutation_envelope(lane, "lane.closeout", changes);
            print_lane_mutation(&envelope, &format)
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
            let (placement, anchor) = match (first, before.as_deref(), after.as_deref()) {
                (false, None, None) => (None, None),
                (true, None, None) => (Some("FIRST"), None),
                (false, Some(anchor), None) => (Some("BEFORE"), Some(anchor)),
                (false, None, Some(anchor)) => (Some("AFTER"), Some(anchor)),
                _ => {
                    return Err("at most one of --first, --before, --after may be set".to_string());
                }
            };
            let client = remote::open_cli_generated_client(&store, keys)?;
            let actor = resolve_generated_lane_actor(updated_by.as_deref())?;
            let lane = client
                .lanes_ticket_add(&workspace, &lane_id, &ticket_id, placement, anchor, &actor)?;
            let envelope = lane_mutation_envelope(
                lane,
                "lane.ticket_added",
                vec![MutationChange::relation_set(
                    ticket_id.clone(),
                    "lane_ticket",
                    ticket_id.clone(),
                )],
            );
            print_lane_mutation(&envelope, &format)
        }
        LanesCmd::TicketRemove {
            store,
            workspace,
            lane_id,
            ticket_id,
            updated_by,
            format,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let actor = resolve_generated_lane_actor(updated_by.as_deref())?;
            let lane = client.lanes_ticket_remove(&workspace, &lane_id, &ticket_id, &actor)?;
            let envelope = lane_mutation_envelope(
                lane,
                "lane.ticket_removed",
                vec![MutationChange::relation_removed(
                    ticket_id.clone(),
                    "lane_ticket",
                    ticket_id.clone(),
                )],
            );
            print_lane_mutation(&envelope, &format)
        }
        LanesCmd::TicketTransfer {
            store,
            workspace,
            source_lane_id,
            target_lane_id,
            ticket_id,
            updated_by,
            format,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let actor = resolve_generated_lane_actor(Some(&updated_by))?;
            let target = client.lanes_ticket_transfer(
                &workspace,
                &source_lane_id,
                &target_lane_id,
                &ticket_id,
                &actor,
            )?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let actor = resolve_generated_lane_actor(Some(&updated_by))?;
            let lane = client.lanes_delete(&workspace, &lane_id, &actor)?;
            let envelope =
                lane_mutation_envelope(lane, "lane.deleted", vec![MutationChange::ResourceDeleted]);
            print_lane_mutation(&envelope, &format)
        }
        LanesCmd::Cleanup {
            store,
            workspace,
            lane,
            apply,
            updated_by,
            format,
        } => {
            let client = if apply {
                remote::open_cli_generated_client(&store, keys)?
            } else {
                remote::open_cli_read_only_generated_client(&store, keys)?
            };
            let actor = resolve_generated_lane_actor(updated_by.as_deref())?;
            let raw = client.generated_json(
                "Lanes",
                "cleanup_json",
                vec![
                    workspace.to_string().to_value(),
                    lane.to_value(),
                    apply.to_value(),
                    actor.to_value(),
                ],
            )?;
            let reports: Vec<LaneCleanupReport> =
                serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            print_lane_cleanup_reports(&reports, &format)
        }
    }
}

/// A per-lane `lanes cleanup` report mirroring the MCP `lanes_cleanup` shape: in dry-run mode
/// `would_remove` lists the terminal members an apply would drop; in apply mode `removed` lists the
/// members actually dropped. `remaining_count`/`status_counts` describe the members that remain,
/// derived from live ticket statuses. Tickets and their history are never mutated.
#[derive(serde::Deserialize, serde::Serialize)]
struct LaneCleanupReport {
    lane_id: String,
    would_remove: Vec<String>,
    removed: Vec<String>,
    remaining_count: usize,
    status_counts: serde_json::Value,
}

fn print_lane_cleanup_reports(reports: &[LaneCleanupReport], format: &str) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(reports).map_err(|e| e.to_string())?
        );
    } else {
        for report in reports {
            println!(
                "{}\twould_remove={}\tremoved={}\tremaining={}",
                report.lane_id,
                report.would_remove.join(","),
                report.removed.join(","),
                report.remaining_count
            );
        }
    }
    Ok(())
}

/// Expand a CLI text argument that may reference a file: `@<path>` reads the file's contents, `@@...`
/// is an escaped literal beginning with `@`, and any other value is returned unchanged. Used for
/// flags that accept large markdown/text/JSON so callers can pass `@path` instead of inlining.
fn expand_at_file(value: &str) -> Result<String, String> {
    if let Some(rest) = value.strip_prefix("@@") {
        return Ok(format!("@{rest}"));
    }
    if let Some(path) = value.strip_prefix('@') {
        return std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"));
    }
    Ok(value.to_string())
}

/// Apply [`expand_at_file`] to an optional CLI text argument.
fn expand_at_file_opt(value: Option<String>) -> Result<Option<String>, String> {
    value.map(|value| expand_at_file(&value)).transpose()
}

const DERIVE_LANE_ACTOR: &str = "system:derive-lane-actor";

/// Resolve the actor recorded on a Lane mutation from the CLI.
///
/// Routine mutations omit `--updated-by` and derive the actor from the authenticated principal,
/// falling back to the workspace namespace when unauthenticated. An explicit override is honored
/// as-is when it matches the effective principal; when it differs it is authorized through the
/// shared ACL substrate (`Tickets` domain, `Admin` right) rather than any bespoke lane-only policy.
#[cfg(test)]
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

fn resolve_generated_lane_actor(provided: Option<&str>) -> Result<String, String> {
    if let Some(actor) = provided.filter(|value| !value.trim().is_empty()) {
        return Ok(actor.to_string());
    }
    Ok(DERIVE_LANE_ACTOR.to_string())
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

fn board_columns_json(columns: &[loom_tickets::BoardColumn]) -> serde_json::Value {
    serde_json::Value::Array(
        columns
            .iter()
            .map(|column| {
                serde_json::json!({
                    "column_id": column.column_id,
                    "name": column.name,
                    "mapped_statuses": column.mapped_statuses.iter().collect::<Vec<_>>(),
                    "wip_limit": column.wip_limit,
                    "hidden": column.hidden,
                    "rank": column.rank,
                })
            })
            .collect(),
    )
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Pages",
                "spaces_create_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    space_id.to_string().to_value(),
                    title.to_string().to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_page_space_json(&raw, &format)
        }
        PagesCmd::SpaceList {
            store,
            workspace,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Pages",
                "spaces_list_json",
                vec![workspace.to_string().to_value(), profile_id.to_value()],
            )?;
            print_generated_page_spaces_json(&raw, &format)
        }
        PagesCmd::SpaceGet {
            store,
            workspace,
            space_id,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Pages",
                "spaces_get_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    space_id.to_string().to_value(),
                ],
            )?;
            print_generated_page_space_json(&raw, &format)
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Pages",
                "pages_create_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    page_id.to_string().to_value(),
                    space_id.to_string().to_value(),
                    parent_page_id.map(|value| value.to_string()).to_value(),
                    title.to_string().to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_page_json(&raw, &format)
        }
        PagesCmd::Update {
            store,
            workspace,
            page_id,
            body,
            expected_root,
            format,
        } => {
            let body = parse_page_body_text(&body)?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Pages",
                "pages_update_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    page_id.to_string().to_value(),
                    body.to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_page_update_json(&raw, &format)
        }
        PagesCmd::Publish {
            store,
            workspace,
            page_id,
            expected_root,
            format,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Pages",
                "pages_publish_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    page_id.to_string().to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_page_publish_json(&raw, &format)
        }
        PagesCmd::Get {
            store,
            workspace,
            page_id,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Pages",
                "pages_get_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    page_id.to_string().to_value(),
                ],
            )?;
            print_generated_page_json(&raw, &format)
        }
        PagesCmd::History {
            store,
            workspace,
            page_id,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Pages",
                "pages_history_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    page_id.to_string().to_value(),
                ],
            )?;
            print_generated_page_history_json(&raw, &format)
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Pages",
                "structures_create_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    structure_id.to_string().to_value(),
                    space_id.to_string().to_value(),
                    kind.to_string().to_value(),
                    title.to_string().to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_page_structure_render_json(&raw, &format)
        }
        PagesCmd::StructureGet {
            store,
            workspace,
            structure_id,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Pages",
                "structures_get_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    structure_id.to_string().to_value(),
                ],
            )?;
            print_generated_page_structure_render_json(&raw, &format)
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Pages",
                "structures_add_node_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    structure_id.to_string().to_value(),
                    node_id.to_string().to_value(),
                    kind.to_string().to_value(),
                    label.to_string().to_value(),
                    body_digest.map(|value| value.to_string()).to_value(),
                    entity_ref.to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_page_structure_node_json(&raw, &format)
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Pages",
                "structures_update_node_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    structure_id.to_string().to_value(),
                    node_id.to_string().to_value(),
                    kind.to_string().to_value(),
                    label.to_string().to_value(),
                    body_digest.map(|value| value.to_string()).to_value(),
                    entity_ref.to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_page_structure_node_json(&raw, &format)
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Pages",
                "structures_bind_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    structure_id.to_string().to_value(),
                    node_id.to_string().to_value(),
                    entity_ref.to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_page_structure_node_json(&raw, &format)
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Pages",
                "structures_move_node_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    structure_id.to_string().to_value(),
                    node_id.to_string().to_value(),
                    parent_node_id.map(|value| value.to_string()).to_value(),
                    label.map(|value| value.to_string()).to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_page_structure_move_json(&raw, &format)
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Pages",
                "structures_link_node_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    structure_id.to_string().to_value(),
                    edge_id.to_string().to_value(),
                    src_node_id.to_string().to_value(),
                    dst_node_id.to_string().to_value(),
                    label.to_string().to_value(),
                    target_ref.to_value(),
                    expected_root.map(|value| value.to_string()).to_value(),
                ],
            )?;
            print_generated_page_structure_edge_json(&raw, &format)
        }
        PagesCmd::StructureDecomposeToTickets {
            store,
            workspace,
            structure_id,
            items,
            format,
        } => {
            let parsed_items = parse_page_structure_decompose_items(&items)?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            let profile_id = client.resolve_workspace_id(&workspace)?.to_string();
            let raw = client.generated_json(
                "Pages",
                "structures_decompose_to_tickets_json",
                vec![
                    workspace.to_string().to_value(),
                    profile_id.to_value(),
                    structure_id.to_string().to_value(),
                    serde_json::to_string(&parsed_items)
                        .map_err(|e| e.to_string())?
                        .to_value(),
                ],
            )?;
            print_generated_page_structure_decompose_json(&raw, &format)
        }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let raw = client.generated_json(
                "Lifecycle",
                "lifecycle_define_standard_json",
                vec![
                    workspace.to_value(),
                    kind.to_value(),
                    version.to_value(),
                    completion_predicate_digest.to_value(),
                ],
            )?;
            print_lifecycle_json(&raw, &format)
        }
        LifecycleCmd::Define {
            store,
            workspace,
            definition,
            format,
        } => {
            let bytes = read_input(&definition).map_err(|e| e.to_string())?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            let raw = client.generated_json(
                "Lifecycle",
                "lifecycle_define_json",
                vec![workspace.to_value(), WireValue::Bytes(bytes)],
            )?;
            print_lifecycle_json(&raw, &format)
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let raw = client.generated_json(
                "Lifecycle",
                "lifecycle_instantiate_json",
                vec![
                    workspace.to_value(),
                    instance_id.to_value(),
                    definition_id.to_value(),
                    subject_refs.to_value(),
                ],
            )?;
            print_lifecycle_json(&raw, &format)
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
            let gate_evaluations = read_lifecycle_gate_evaluations_json(&gate_evaluations)?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            let raw = client.generated_json(
                "Lifecycle",
                "lifecycle_transition_json",
                vec![
                    workspace.to_value(),
                    instance_id.to_value(),
                    transition_id.to_value(),
                    to_stage_id.to_value(),
                    actor_principal_id.to_value(),
                    gate_evaluations.to_value(),
                    snapshot_digest.to_value(),
                ],
            )?;
            print_lifecycle_json(&raw, &format)
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

fn read_lifecycle_gate_evaluations_json(input: &str) -> Result<String, String> {
    if let Some(path) = input.strip_prefix('@') {
        let bytes = read_input(path).map_err(|e| e.to_string())?;
        String::from_utf8(bytes).map_err(|_| "lifecycle gate evaluations must be UTF-8".to_string())
    } else {
        Ok(input.to_string())
    }
}

fn print_lifecycle_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    print_lifecycle(&value, format)
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

fn parse_page_body_text(input: &str) -> Result<String, String> {
    let text = if let Some(path) = input.strip_prefix('@') {
        String::from_utf8(read_input(path).map_err(|e| e.to_string())?)
            .map_err(|_| "page body input must be UTF-8".to_string())?
    } else {
        input.to_string()
    };
    Ok(text)
}

fn print_generated_page_space_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}",
            json_string_field(&value, "space_id")?,
            json_string_field(&value, "title")?,
            value
                .get("archived")
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| "page space JSON missing boolean archived".to_string())?,
            json_string_field(&value, "profile_root")?
        );
    }
    Ok(())
}

fn print_generated_page_spaces_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        let spaces = value
            .as_array()
            .ok_or_else(|| "page spaces JSON must be an array".to_string())?;
        for space in spaces {
            println!(
                "{}\t{}\t{}\t{}",
                json_string_field(space, "space_id")?,
                json_string_field(space, "title")?,
                space
                    .get("archived")
                    .and_then(serde_json::Value::as_bool)
                    .ok_or_else(|| "page space JSON missing boolean archived".to_string())?,
                json_string_field(space, "profile_root")?
            );
        }
    }
    Ok(())
}

fn print_generated_page_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            json_string_field(&value, "page_id")?,
            json_string_field(&value, "space_id")?,
            json_string_field(&value, "title")?,
            json_string_field(&value, "status")?,
            json_string_field(&value, "profile_root")?
        );
    }
    Ok(())
}

fn print_generated_page_update_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}",
            json_string_field(&value, "page_id")?,
            json_string_field(&value, "status")?,
            value
                .get("updated_at_ms")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| "page update JSON missing u64 updated_at_ms".to_string())?,
            json_string_field(&value, "profile_root")?
        );
    }
    Ok(())
}

fn print_generated_page_publish_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}",
            json_string_field(&value, "page_id")?,
            json_string_field(&value, "outcome")?,
            value
                .get("revision")
                .and_then(serde_json::Value::as_u64)
                .map(|revision| revision.to_string())
                .unwrap_or_default(),
            json_string_field(&value, "profile_root")?
        );
    }
    Ok(())
}

fn print_generated_page_history_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        let entries = value
            .as_array()
            .ok_or_else(|| "page history JSON must be an array".to_string())?;
        for entry in entries {
            println!(
                "{}\t{}\t{}\t{}",
                json_string_field(entry, "kind")?,
                json_string_field(entry, "page_id")?,
                entry
                    .get("revision")
                    .and_then(serde_json::Value::as_u64)
                    .map(|revision| revision.to_string())
                    .unwrap_or_default(),
                entry
                    .get("body_digest")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
            );
        }
    }
    Ok(())
}

fn print_generated_page_structure_render_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        let structure = value
            .get("structure")
            .ok_or_else(|| "structure render JSON missing structure".to_string())?;
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            json_string_field(structure, "structure_id")?,
            json_string_field(structure, "space_id")?,
            json_string_field(structure, "kind")?,
            json_string_field(structure, "title")?,
            json_array_len(&value, "nodes")?,
            json_array_len(&value, "edges")?,
            json_string_field(&value, "graph_collection")?
        );
    }
    Ok(())
}

fn print_generated_page_structure_node_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            json_string_field(&value, "structure_id")?,
            json_string_field(&value, "node_id")?,
            json_string_field(&value, "kind")?,
            json_string_field(&value, "label")?,
            value
                .get("entity_ref")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(""),
            json_string_field(&value, "profile_root")?
        );
    }
    Ok(())
}

fn print_generated_page_structure_edge_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            json_string_field(&value, "structure_id")?,
            json_string_field(&value, "edge_id")?,
            json_string_field(&value, "src_node_id")?,
            json_string_field(&value, "dst_node_id")?,
            json_string_field(&value, "label")?,
            json_string_field(&value, "profile_root")?
        );
    }
    Ok(())
}

fn print_generated_page_structure_move_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            json_string_field(&value, "structure_id")?,
            json_string_field(&value, "node_id")?,
            value
                .get("parent_node_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(""),
            json_string_field(&value, "label")?,
            json_string_field(&value, "profile_root")?
        );
    }
    Ok(())
}

fn print_generated_page_structure_decompose_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            json_string_field(&value, "workspace_id")?,
            json_string_field(&value, "structure_id")?,
            json_array_len(&value, "tickets")?,
            json_array_len(&value, "implemented_by_edges")?,
            json_string_field(&value, "graph_collection")?
        );
    }
    Ok(())
}

fn json_string_field<'a>(value: &'a serde_json::Value, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("JSON value missing string field {field:?}"))
}

fn json_array_len(value: &serde_json::Value, field: &str) -> Result<usize, String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .ok_or_else(|| format!("JSON value missing array field {field:?}"))
}

fn print_generated_ticket_page_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        let items = value
            .get("items")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| "ticket page JSON missing array items".to_string())?;
        for ticket in items {
            println!(
                "{}\t{}\t{}\t{}",
                json_string_field(ticket, "primary_key")?,
                json_string_field(ticket, "ticket_id")?,
                json_string_field(ticket, "project_id")?,
                json_string_field(ticket, "ticket_type")?
            );
        }
        if let Some(cursor) = value.get("next_cursor").and_then(serde_json::Value::as_str) {
            println!("next_cursor\t{cursor}");
        }
    }
    Ok(())
}

fn print_generated_ticket_comments_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        let comments = value
            .as_array()
            .ok_or_else(|| "ticket comments JSON must be an array".to_string())?;
        for comment in comments {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}",
                json_string_field(comment, "comment_id")?,
                json_string_field(comment, "comment_type")?,
                json_string_field(comment, "author_principal")?,
                comment
                    .get("created_at_ms")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| "ticket comment JSON missing u64 created_at_ms".to_string())?,
                comment
                    .get("updated_at_ms")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                comment
                    .get("redacted")
                    .and_then(serde_json::Value::as_bool)
                    .ok_or_else(|| "ticket comment JSON missing boolean redacted".to_string())?
            );
        }
    }
    Ok(())
}

fn generated_envelope_parts(
    value: &serde_json::Value,
) -> Result<(&serde_json::Value, &serde_json::Value), String> {
    let receipt = value
        .get("receipt")
        .ok_or_else(|| "mutation envelope JSON missing receipt".to_string())?;
    let resource = value
        .get("resource")
        .ok_or_else(|| "mutation envelope JSON missing resource".to_string())?;
    Ok((receipt, resource))
}

fn print_generated_mutation_receipt(receipt: &serde_json::Value) -> Result<(), String> {
    println!("operation={}", json_string_field(receipt, "operation")?);
    println!(
        "resource_kind={}",
        json_string_field(receipt, "resource_kind")?
    );
    println!("resource_id={}", json_string_field(receipt, "resource_id")?);
    println!(
        "operation_id={}",
        receipt
            .get("operation_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
    );
    println!(
        "root_before={}",
        receipt
            .get("root_before")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
    );
    println!(
        "root_after={}",
        receipt
            .get("root_after")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
    );
    let changes = receipt
        .get("changes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "mutation receipt JSON missing array changes".to_string())?;
    if changes.is_empty() {
        println!("change=[]");
    } else {
        for change in changes {
            println!(
                "change={}",
                serde_json::to_string(change).map_err(|e| e.to_string())?
            );
        }
    }
    Ok(())
}

fn print_generated_ticket_summary_value(ticket: &serde_json::Value) -> Result<(), String> {
    println!(
        "{}\t{}\t{}\t{}\t{}\t{}",
        json_string_field(ticket, "primary_key")?,
        json_string_field(ticket, "ticket_id")?,
        json_string_field(ticket, "project_id")?,
        json_string_field(ticket, "ticket_type")?,
        json_string_field(ticket, "projection")?,
        json_string_field(ticket, "profile_root")?
    );
    Ok(())
}

fn print_generated_ticket_relation_value(relation: &serde_json::Value) -> Result<(), String> {
    println!(
        "{}\t{}\t{}\t{}\t{}\t{}",
        json_string_field(relation, "ticket_id")?,
        json_string_field(relation, "relation_id")?,
        json_string_field(relation, "kind")?,
        json_string_field(relation, "target_type")?,
        json_string_field(relation, "target_id")?,
        json_string_field(relation, "graph_edge_id")?
    );
    Ok(())
}

fn print_generated_ticket_mutation_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        let (receipt, resource) = generated_envelope_parts(&value)?;
        print_generated_mutation_receipt(receipt)?;
        print_generated_ticket_summary_value(resource)?;
    }
    Ok(())
}

fn print_generated_ticket_relation_mutation_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        let (receipt, resource) = generated_envelope_parts(&value)?;
        print_generated_mutation_receipt(receipt)?;
        print_generated_ticket_relation_value(resource)?;
    }
    Ok(())
}

fn print_generated_ticket_relations_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let relations = value
        .as_array()
        .ok_or_else(|| "ticket relations JSON must be an array".to_string())?;
    if format == "json" {
        let items = relations
            .iter()
            .map(|relation| {
                serde_json::json!({
                    "direction": relation.get("direction").cloned().unwrap_or(serde_json::Value::Null),
                    "kind": relation.get("kind").cloned().unwrap_or(serde_json::Value::Null),
                    "target_ticket_id": relation.get("target_ticket_id").cloned().unwrap_or(serde_json::Value::Null),
                    "target_title": relation.get("target_title").cloned().unwrap_or(serde_json::Value::Null),
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
                json_string_field(relation, "direction")?,
                json_string_field(relation, "kind")?,
                json_string_field(relation, "target_ticket_id")?,
                json_string_field(relation, "target_title")?
            );
        }
    }
    Ok(())
}

fn generated_ticket_history_summary(record: &serde_json::Value) -> String {
    if let Some(status) = record
        .pointer("/envelope/payload/status")
        .and_then(serde_json::Value::as_str)
    {
        return format!("status={status}");
    }
    if let Some(target_status) = record
        .pointer("/envelope/payload/target_status")
        .and_then(serde_json::Value::as_str)
    {
        return format!("status={target_status}");
    }
    record
        .get("target_entity_id")
        .and_then(serde_json::Value::as_str)
        .map(|target| format!("target={target}"))
        .unwrap_or_default()
}

fn print_generated_ticket_history_json(
    raw: &str,
    detailed: bool,
    format: &str,
) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" || detailed {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        let history = value
            .as_array()
            .ok_or_else(|| "ticket history JSON must be an array".to_string())?;
        for record in history {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}",
                record
                    .pointer("/envelope/timestamp_ms")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                record
                    .pointer("/envelope/actor_principal")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(""),
                json_string_field(record, "operation_kind")?,
                generated_ticket_history_summary(record),
                record
                    .get("sequence")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| "ticket history JSON missing u64 sequence".to_string())?,
                json_string_field(record, "operation_id")?
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

struct TicketProjectSettingsPatchArgs<'a> {
    default_projection: Option<loom_tickets::TicketProjectionProfile>,
    enable_projections: &'a [loom_tickets::TicketProjectionProfile],
    disable_projections: &'a [loom_tickets::TicketProjectionProfile],
    actor_enforcement: Option<loom_tickets::TicketLifecycleAuthorizationPolicy>,
    project_owner_principal: Option<&'a str>,
    clear_project_owner_principal: bool,
    acceptance_authorities: Option<&'a [String]>,
    acceptance_evidence_enforcement: Option<bool>,
    required_acceptance_evidence_keys: Option<&'a [loom_tickets::TicketAcceptanceEvidenceKey]>,
    required_acceptance_reviews: Option<&'a [loom_tickets::TicketReviewType]>,
    owner_contract_summary: Option<&'a str>,
    owner_contract_details: Option<&'a str>,
    worker_contract_summary: Option<&'a str>,
    worker_contract_details: Option<&'a str>,
    expected_root: Option<&'a str>,
}

fn ticket_project_settings_patch_to_cbor(
    args: TicketProjectSettingsPatchArgs<'_>,
) -> Result<Vec<u8>, String> {
    let opt_text = |value: Option<&str>| {
        value
            .map(|value| WireValue::Text(value.to_string()))
            .unwrap_or(WireValue::Null)
    };
    let projections = |values: &[loom_tickets::TicketProjectionProfile]| {
        WireValue::Array(
            values
                .iter()
                .map(|profile| WireValue::Text(profile.profile_id().to_string()))
                .collect(),
        )
    };
    let optional_strings = |values: Option<&[String]>| {
        values
            .map(|values| {
                WireValue::Array(
                    values
                        .iter()
                        .map(|value| WireValue::Text(value.clone()))
                        .collect(),
                )
            })
            .unwrap_or(WireValue::Null)
    };
    let optional_keys = |values: Option<&[loom_tickets::TicketAcceptanceEvidenceKey]>| {
        values
            .map(|values| {
                WireValue::Array(
                    values
                        .iter()
                        .map(|value| WireValue::Text(value.as_str().to_string()))
                        .collect(),
                )
            })
            .unwrap_or(WireValue::Null)
    };
    let optional_reviews = |values: Option<&[loom_tickets::TicketReviewType]>| {
        values
            .map(|values| {
                WireValue::Array(
                    values
                        .iter()
                        .map(|value| WireValue::Text(value.as_str().to_string()))
                        .collect(),
                )
            })
            .unwrap_or(WireValue::Null)
    };
    let optional_bool = |value: Option<bool>| value.map(WireValue::Bool).unwrap_or(WireValue::Null);
    let value = WireValue::Array(vec![
        opt_text(args.default_projection.map(|profile| profile.profile_id())),
        projections(args.enable_projections),
        projections(args.disable_projections),
        opt_text(args.actor_enforcement.map(|policy| policy.as_str())),
        opt_text(args.project_owner_principal),
        WireValue::Bool(args.clear_project_owner_principal),
        optional_strings(args.acceptance_authorities),
        optional_bool(args.acceptance_evidence_enforcement),
        optional_keys(args.required_acceptance_evidence_keys),
        optional_reviews(args.required_acceptance_reviews),
        opt_text(args.owner_contract_summary),
        opt_text(args.owner_contract_details),
        opt_text(args.worker_contract_summary),
        opt_text(args.worker_contract_details),
        opt_text(args.expected_root),
    ]);
    loom_codec::encode(&value).map_err(|error| error.to_string())
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

#[derive(Debug, serde::Deserialize, serde::Serialize)]
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

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct TicketUpdateRelationSetInput {
    #[serde(default)]
    relation_id: Option<String>,
    kind: String,
    target_id: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
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

#[cfg(test)]
mod ticket_update_cli_tests {
    use super::*;

    #[test]
    fn at_file_expansion_reads_file_escapes_and_passes_through() {
        // Plain values pass through unchanged (both owner/worker contract flags route through this).
        assert_eq!(expand_at_file("inline value").unwrap(), "inline value");
        // `@@` escapes a literal leading `@`.
        assert_eq!(expand_at_file("@@literal").unwrap(), "@literal");
        // `@path` loads the file contents (the bug: previously the literal `@path` was stored).
        let path = std::env::temp_dir().join(format!("mx438-contract-{}.md", std::process::id()));
        std::fs::write(&path, "# Worker Contract\n\nloaded from file").unwrap();
        let arg = format!("@{}", path.display());
        assert_eq!(
            expand_at_file(&arg).unwrap(),
            "# Worker Contract\n\nloaded from file"
        );
        let _ = std::fs::remove_file(&path);
        // Representative JSON field: `@path` loads JSON text verbatim for downstream parsing.
        let json_path =
            std::env::temp_dir().join(format!("mx439-evidence-{}.json", std::process::id()));
        std::fs::write(&json_path, "{\"checks_run\":[\"cargo test\"]}").unwrap();
        assert_eq!(
            expand_at_file(&format!("@{}", json_path.display())).unwrap(),
            "{\"checks_run\":[\"cargo test\"]}"
        );
        let _ = std::fs::remove_file(&json_path);
        // A missing file is a clear error, not a silent literal.
        assert!(expand_at_file("@/no/such/mx438/contract").is_err());
        // The optional wrapper used for owner/worker contract summary+details.
        assert_eq!(expand_at_file_opt(None).unwrap(), None);
        assert_eq!(
            expand_at_file_opt(Some("plain".to_string())).unwrap(),
            Some("plain".to_string())
        );
    }

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

fn print_generated_ticket_project_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            json_string_field(&value, "project_id")?,
            json_string_field(&value, "key_prefix")?,
            json_string_field(&value, "name")?,
            json_string_field(&value, "lifecycle_authorization_policy")?,
            json_string_field(&value, "profile_root")?
        );
    }
    Ok(())
}

fn print_generated_ticket_projects_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        let projects = value
            .as_array()
            .ok_or_else(|| "ticket projects JSON must be an array".to_string())?;
        for project in projects {
            println!(
                "{}\t{}\t{}\t{}\t{}",
                json_string_field(project, "project_id")?,
                json_string_field(project, "key_prefix")?,
                json_string_field(project, "name")?,
                json_string_field(project, "lifecycle_authorization_policy")?,
                json_string_field(project, "profile_root")?
            );
        }
    }
    Ok(())
}

fn print_generated_ticket_field_catalog_json(raw: &str, format: &str) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "projection\t{}\noperation\t{}\nstrict_unknown_fields\t{}\ncustom_fields_source\t{}\nunknown_field_write_behavior\t{}",
            json_string_field(&value, "projection_profile")?,
            json_string_field(&value, "operation")?,
            value
                .get("strict_unknown_fields")
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| {
                    "ticket field catalog JSON missing boolean strict_unknown_fields".to_string()
                })?,
            json_string_field(&value, "custom_fields_source")?,
            json_string_field(&value, "unknown_field_write_behavior")?
        );
        let fields = value
            .get("fields")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| "ticket field catalog JSON missing array fields".to_string())?;
        for field in fields {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}",
                json_string_field(field, "native_field")?,
                json_string_field(field, "write_path")?,
                json_string_field(field, "field_type")?,
                json_string_field(field, "cardinality")?,
                field
                    .get("max_length")
                    .and_then(serde_json::Value::as_u64)
                    .map_or_else(String::new, |value| value.to_string()),
                field
                    .get("enum_values")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| "ticket field catalog JSON missing enum_values".to_string())?
                    .iter()
                    .map(|value| {
                        value.as_str().ok_or_else(|| {
                            "ticket field catalog enum_values entries must be strings".to_string()
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",")
            );
        }
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

/// The `lanes list` JSON payload with canonical Lane views and consistency warnings.
fn lane_list_json_payload(views: &[LaneView], diagnostics: &[LaneDiagnostic]) -> serde_json::Value {
    serde_json::json!({ "lanes": views, "diagnostics": diagnostics })
}

/// One Lane consistency warning rendered as a tab-separated text line for `lanes list`.
fn lane_diagnostic_text_line(diagnostic: &LaneDiagnostic) -> String {
    format!("diagnostic\t{}\t{}", diagnostic.lane_id, diagnostic.error)
}

fn print_lane_views(
    views: &[LaneView],
    diagnostics: &[LaneDiagnostic],
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

fn print_generated_ticket_detail_json(
    ticket: &serde_json::Value,
    history_raw: &str,
    comments_raw: &str,
    detailed: bool,
    compact: bool,
    format: &str,
) -> Result<(), String> {
    let history: serde_json::Value =
        serde_json::from_str(history_raw).map_err(|e| e.to_string())?;
    let comments: serde_json::Value =
        serde_json::from_str(comments_raw).map_err(|e| e.to_string())?;
    let history_items = history
        .as_array()
        .ok_or_else(|| "generated ticket history response is not an array".to_string())?;
    let comment_items = comments
        .as_array()
        .ok_or_else(|| "generated ticket comments response is not an array".to_string())?;
    let compact = compact && !detailed;
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
        if compact {
            let latest = latest_generated_ticket_update(history_items);
            let value = serde_json::json!({
                "primary_key": generated_ticket_text_opt(ticket, "primary_key"),
                "title": generated_ticket_field_text(ticket, "title"),
                "status": generated_ticket_field_text(ticket, "status"),
                "priority": generated_ticket_field_text(ticket, "priority"),
                "type": generated_ticket_text_opt(ticket, "ticket_type"),
                "assignee": generated_ticket_field_text(ticket, "assignee"),
                "assignee_display": generated_ticket_field_text(ticket, "assignee_display"),
                "project": generated_ticket_text_opt(ticket, "project_id"),
                "dependencies": {
                    "depends_on": ticket.get("depends_on").cloned().unwrap_or_else(|| serde_json::json!([])),
                    "blocks": ticket.get("blocks").cloned().unwrap_or_else(|| serde_json::json!([])),
                    "relations": generated_ticket_relation_compacts(ticket)
                },
                "comment_count": comment_items.len(),
                "latest_update": latest.map(|latest| serde_json::json!({
                    "actor": latest.actor,
                    "timestamp_ms": latest.timestamp_ms,
                    "operation_kind": latest.operation_kind,
                    "sequence": latest.sequence
                }))
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
            );
            return Ok(());
        }
        println!(
            "{}",
            serde_json::to_string_pretty(ticket).map_err(|e| e.to_string())?
        );
        return Ok(());
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
            serde_json::to_string_pretty(&comments).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    println!("key\t{}", generated_ticket_text(ticket, "primary_key")?);
    println!(
        "title\t{}",
        generated_ticket_field_text(ticket, "title").unwrap_or_default()
    );
    println!(
        "status\t{}",
        generated_ticket_field_text(ticket, "status").unwrap_or_default()
    );
    println!(
        "priority\t{}",
        generated_ticket_field_text(ticket, "priority").unwrap_or_default()
    );
    println!("type\t{}", generated_ticket_text(ticket, "ticket_type")?);
    let assignee = generated_ticket_field_text(ticket, "assignee").unwrap_or_default();
    match generated_ticket_field_text(ticket, "assignee_display") {
        Some(display) if display != assignee => {
            println!("assignee\t{assignee} ({display})");
        }
        _ => println!("assignee\t{assignee}"),
    }
    println!("project\t{}", generated_ticket_text(ticket, "project_id")?);
    if !compact {
        println!(
            "description\t{}",
            generated_ticket_field_text(ticket, "description").unwrap_or_default()
        );
    }
    println!(
        "depends_on\t{}",
        generated_ticket_string_list(ticket, "depends_on")
    );
    println!("blocks\t{}", generated_ticket_string_list(ticket, "blocks"));
    println!("relations\t{}", generated_ticket_relation_summary(ticket));
    println!("comments\t{}", comment_items.len());
    if let Some(latest) = latest_generated_ticket_update(history_items) {
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

fn latest_generated_ticket_update(history: &[serde_json::Value]) -> Option<TicketUpdateView> {
    history
        .iter()
        .max_by_key(|record| {
            record
                .get("sequence")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
        })
        .map(|record| {
            let envelope = record.get("envelope").unwrap_or(&serde_json::Value::Null);
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
                operation_kind: record
                    .get("operation_kind")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                sequence: record
                    .get("sequence")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
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

fn generated_ticket_text<'a>(
    ticket: &'a serde_json::Value,
    field: &str,
) -> Result<&'a str, String> {
    ticket
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("generated ticket response missing {field}"))
}

fn generated_ticket_text_opt(ticket: &serde_json::Value, field: &str) -> Option<String> {
    ticket
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn generated_ticket_field_text(ticket: &serde_json::Value, field: &str) -> Option<String> {
    match ticket.get("fields")?.get(field)? {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Object(map) => map
            .get("String")
            .or_else(|| map.get("Text"))
            .or_else(|| map.get("EnumOption"))
            .or_else(|| map.get("Principal"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn generated_ticket_string_list(ticket: &serde_json::Value, field: &str) -> String {
    let values = ticket
        .get(field)
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    compact_string_list(&values)
}

fn generated_ticket_relation_compacts(ticket: &serde_json::Value) -> Vec<serde_json::Value> {
    ticket
        .get("relations")
        .and_then(serde_json::Value::as_array)
        .map(|relations| {
            relations
                .iter()
                .map(|relation| {
                    serde_json::json!({
                        "kind": relation.get("kind").and_then(serde_json::Value::as_str).unwrap_or(""),
                        "target_id": relation.get("target_id").and_then(serde_json::Value::as_str).unwrap_or("")
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn generated_ticket_relation_summary(ticket: &serde_json::Value) -> String {
    let Some(relations) = ticket
        .get("relations")
        .and_then(serde_json::Value::as_array)
    else {
        return "none".to_string();
    };
    if relations.is_empty() {
        return "none".to_string();
    }
    relations
        .iter()
        .map(|relation| {
            format!(
                "{}:{}",
                relation
                    .get("kind")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(""),
                relation
                    .get("target_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join(",")
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

fn print_generated_board_json(raw: &str, format: &str) -> Result<(), String> {
    let board: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&board).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            board_text(&board, "board_key")?,
            board_text(&board, "board_id")?,
            board_text(&board, "name")?,
            board_text(&board, "project_id")?,
            board_text(&board, "mode")?,
            board_text(&board, "board_status")?,
            board_text(&board, "profile_root")?
        );
    }
    Ok(())
}

fn print_generated_boards_json(raw: &str, format: &str) -> Result<(), String> {
    let boards: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&boards).map_err(|e| e.to_string())?
        );
    } else {
        for board in boards
            .as_array()
            .ok_or_else(|| "generated boards response is not an array".to_string())?
        {
            println!(
                "{}\t{}\t{}\t{}\t{}",
                board_text(board, "board_key")?,
                board_text(board, "board_id")?,
                board_text(board, "name")?,
                board_text(board, "mode")?,
                board_text(board, "board_status")?
            );
        }
    }
    Ok(())
}

fn board_text<'a>(board: &'a serde_json::Value, field: &str) -> Result<&'a str, String> {
    board
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("generated board response missing {field}"))
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let seq = execute_generated_u64(
                &client,
                "Queue",
                "append",
                vec![
                    workspace.to_value(),
                    stream.to_value(),
                    WireValue::Bytes(bytes),
                ],
            )?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "QueueConsumers",
                "consumer_advance",
                vec![
                    workspace.to_value(),
                    stream.to_value(),
                    consumer.to_value(),
                    next.to_value(),
                ],
            )
        }
        QueueCmd::Get {
            store,
            workspace,
            stream,
            seq,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Queue",
                "get",
                vec![
                    workspace.to_value(),
                    stream.to_value(),
                    (seq as u64).to_value(),
                ],
            )?
            else {
                return Err(format!("queue sequence {seq} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        QueueCmd::Len {
            store,
            workspace,
            stream,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let len = execute_generated_u64(
                &client,
                "Queue",
                "len",
                vec![workspace.to_value(), stream.to_value()],
            )?;
            println!("{len}");
            Ok(())
        }
        QueueCmd::Position {
            store,
            workspace,
            stream,
            consumer,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let position = execute_generated_u64(
                &client,
                "QueueConsumers",
                "consumer_position",
                vec![workspace.to_value(), stream.to_value(), consumer.to_value()],
            )?;
            println!("{position}");
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let entries = execute_generated_bytes_list(
                &client,
                "Queue",
                "range",
                vec![
                    workspace.to_value(),
                    stream.to_value(),
                    (from as u64).to_value(),
                    (to as u64).to_value(),
                ],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let entries = execute_generated_bytes_list(
                &client,
                "QueueConsumers",
                "consumer_read",
                vec![
                    workspace.to_value(),
                    stream.to_value(),
                    consumer.to_value(),
                    (max as u32).to_value(),
                ],
            )?;
            write_output(out.as_deref(), &bytes_array_cbor(&entries)?).map_err(|e| e.to_string())
        }
        QueueCmd::Reset {
            store,
            workspace,
            stream,
            consumer,
            next,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "QueueConsumers",
                "consumer_reset",
                vec![
                    workspace.to_value(),
                    stream.to_value(),
                    consumer.to_value(),
                    next.to_value(),
                ],
            )
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "TimeSeries",
                "get",
                vec![
                    workspace.to_value(),
                    series.to_value(),
                    timestamp.to_value(),
                ],
            )?
            else {
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let mut result = loom_core::Series::new();
            if let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "TimeSeries",
                "latest",
                vec![workspace.to_value(), series.to_value()],
            )? {
                let (timestamp, value) = loom_core::timeseries::latest_point_from_cbor(&bytes)
                    .map_err(|e| e.to_string())?;
                result.put(timestamp, value);
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "TimeSeries",
                "put",
                vec![
                    workspace.to_value(),
                    series.to_value(),
                    timestamp.to_value(),
                    WireValue::Bytes(bytes),
                ],
            )
        }
        TimeSeriesCmd::Range {
            store,
            workspace,
            series,
            from,
            to,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "TimeSeries",
                "range",
                vec![
                    workspace.to_value(),
                    series.to_value(),
                    from.to_value(),
                    to.to_value(),
                ],
            )?;
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

fn run_inference_instance(action: InferenceInstanceCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        InferenceInstanceCmd::List {
            store,
            workspace,
            kind,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let result_json = execute_generated_string(
                &client,
                "InferenceInstance",
                "inference_instance_list_json",
                vec![workspace.to_value(), kind.to_value()],
            )?;
            let owned = parse_inference_instance_list(&result_json)?;
            let instances = owned
                .iter()
                .map(OwnedInferenceInstanceView::as_view)
                .collect::<Vec<_>>();
            print_inference_instance_list(&instances, &format)
        }
        InferenceInstanceCmd::Show {
            store,
            workspace,
            name,
            resolved,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let result_json = execute_generated_string(
                &client,
                "InferenceInstance",
                "inference_instance_get_json",
                vec![workspace.to_value(), name.to_value()],
            )?;
            let view = parse_inference_instance_view(&result_json)?;
            print_inference_instance(&view.as_view(), resolved, &format)
        }
        InferenceInstanceCmd::Create {
            store,
            workspace,
            name,
            model,
            kind,
            runtime,
            preset,
            settings,
        } => {
            let settings_json = inference_instance_settings_json(settings)?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            let result_json = execute_generated_string(
                &client,
                "InferenceInstance",
                "inference_instance_create_json",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    model.to_value(),
                    kind.to_value(),
                    runtime.to_value(),
                    preset.to_value(),
                    settings_json.to_value(),
                ],
            )?;
            let view = parse_inference_instance_view(&result_json)?;
            print_inference_instance(&view.as_view(), true, "text")
        }
        InferenceInstanceCmd::Update {
            store,
            workspace,
            name,
            preset,
            settings,
        } => {
            let settings_json = inference_instance_settings_json(settings)?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            let result_json = execute_generated_string(
                &client,
                "InferenceInstance",
                "inference_instance_update_json",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    preset.to_value(),
                    settings_json.to_value(),
                ],
            )?;
            let view = parse_inference_instance_view(&result_json)?;
            print_inference_instance(&view.as_view(), true, "text")
        }
        InferenceInstanceCmd::Delete {
            store,
            workspace,
            name,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let result_json = execute_generated_string(
                &client,
                "InferenceInstance",
                "inference_instance_delete_json",
                vec![workspace.to_value(), name.to_value()],
            )?;
            let result: InferenceInstanceDeleteView =
                serde_json::from_str(&result_json).map_err(|error| error.to_string())?;
            if !result.deleted {
                return Err("inference instance delete returned deleted=false".to_string());
            }
            println!("deleted\t{}", result.name);
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
    let opened = cli_open_loom_read(store, keys)?;
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

#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
struct OwnedInferenceInstanceView {
    instance: loom_types::InferenceInstanceDescriptor,
    refs: usize,
}

impl OwnedInferenceInstanceView {
    fn as_view(&self) -> InferenceInstanceView<'_> {
        InferenceInstanceView {
            instance: &self.instance,
            refs: self.refs,
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
struct InferenceInstanceDeleteView {
    name: String,
    deleted: bool,
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

fn inference_instance_settings_json(settings: Vec<String>) -> Result<String, String> {
    serde_json::to_string(&parse_instance_settings(settings)?).map_err(|error| error.to_string())
}

fn parse_inference_instance_view(raw: &str) -> Result<OwnedInferenceInstanceView, String> {
    serde_json::from_str(raw).map_err(|error| error.to_string())
}

fn parse_inference_instance_list(raw: &str) -> Result<Vec<OwnedInferenceInstanceView>, String> {
    serde_json::from_str(raw).map_err(|error| error.to_string())
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
                    let overlay = serde_json::json!({
                        "generation": report.overlay_health.current_generation,
                        "current_records": report.overlay_health.current_record_count,
                        "tombstones": report.overlay_health.tombstone_count,
                        "obsolete_records": report.overlay_obsolete_record_count,
                        "live_checkpoint_references": report.overlay_health.live_checkpoint_references,
                        "reclaimable_pages": report.overlay_health.reclaimable_overlay_pages,
                        "obsolete_pages": report.overlay_obsolete_page_count,
                        "blocked_reclamation_reasons": report.overlay_health.blocked_reclamation_reasons,
                        "retained_checkpoint_blockers": report.overlay_health.blocked_reclamation_reasons,
                        "hot_write_count": report.overlay_health.hot_write_count,
                        "active_writer_contention_indicators": report.overlay_health.active_writer_contention_indicators,
                    });
                    let mvcc_pins = report
                        .mvcc_snapshots
                        .pins
                        .iter()
                        .map(|pin| {
                            serde_json::json!({
                                "pin_id": pin.pin_id,
                                "overlay_generation": pin.identity.overlay_generation.as_u64(),
                                "base_root": pin
                                    .identity
                                    .immutable_base_root
                                    .as_ref()
                                    .map(|root| root.to_string()),
                                "owner": pin.owner,
                            })
                        })
                        .collect::<Vec<_>>();
                    let mvcc = serde_json::json!({
                        "active_snapshots": report.mvcc_snapshots.active_snapshot_count,
                        "oldest_pinned_generation": report
                            .mvcc_snapshots
                            .oldest_pinned_overlay_generation
                            .map(|generation| generation.as_u64()),
                        "pinned_reader_reclaim_pressure": report.mvcc_snapshots.active_snapshot_count > 0
                            && report.overlay_obsolete_record_count > 0,
                        "pins": mvcc_pins,
                    });
                    let group_commit = serde_json::json!({
                        "group_commit_batches_total": report.status.group_commit.group_commit_batches_total,
                        "group_commit_transactions_total": report.status.group_commit.group_commit_transactions_total,
                        "group_commit_records_total": report.status.group_commit.group_commit_records_total,
                        "fsync_total_micros": report.status.group_commit.fsync_total_micros,
                        "fsync_count": report.status.group_commit.fsync_count,
                        "write_lock_wait_total_micros": report.status.group_commit.write_lock_wait_total_micros,
                        "write_lock_wait_count": report.status.group_commit.write_lock_wait_count,
                        "pending_durable_window_transactions": report.status.group_commit.pending_durable_window_transactions,
                        "pending_durable_window_records": report.status.group_commit.pending_durable_window_records,
                        "pinned_reader_blockers": report.status.group_commit.pinned_reader_blockers,
                    });
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
                        "overlay": overlay,
                        "mvcc": mvcc,
                        "group_commit": group_commit,
                    });
                    if let Ok(loom) = store_doctor_diagnostics_loom(&paths.store, &fs, keys)
                        && let Ok(diagnostics) = cli_live_root_diagnostics(&loom)
                    {
                        maintenance["live_root_diagnostics"] =
                            cli_live_root_diagnostics_json(&diagnostics);
                    }
                    if let Ok(diagnostics) = fs.root_codec_diagnostics() {
                        maintenance["root_codecs"] = root_codec_diagnostics_json(&diagnostics);
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

fn root_codec_diagnostics_json(
    diagnostics: &loom_store::StoreRootCodecDiagnostics,
) -> serde_json::Value {
    serde_json::json!({
        "state": if diagnostics.failures.is_empty() { "ok" } else { "error" },
        "checked": diagnostics.checked_roots,
        "failures": diagnostics.failures.len(),
        "roots": diagnostics.details.iter().map(|diagnostic| {
            serde_json::json!({
                "root": diagnostic.root_name,
                "family_id": diagnostic.family_id,
                "root_page": diagnostic.root_page,
                "byte_offset": diagnostic.byte_offset,
                "expected_codec": diagnostic.expected_codec,
                "expected_discriminator": diagnostic.expected_discriminator,
                "raw_magic": diagnostic.raw_magic,
                "raw_flags": diagnostic.raw_flags,
                "actual_discriminator": diagnostic.actual_discriminator,
                "in_range": diagnostic.in_range,
                "checksum_ok": diagnostic.checksum_ok,
                "magic_ok": diagnostic.magic_ok,
                "codec_ok": diagnostic.codec_ok,
                "reachable": diagnostic.reachable,
                "failure": diagnostic.failure,
            })
        }).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod mu15b_smoke_b_tests {
    use super::*;

    #[test]
    fn mu15b_smoke_b_root_codec_json_output_includes_detailed_rows() {
        let diagnostics = loom_store::StoreRootCodecDiagnostics {
            checked_roots: 1,
            failures: vec![loom_store::StoreRootCodecDiagnostic {
                root_name: "owner_tokens",
                family_id: Some(0x0110),
                root_page: 9,
                byte_offset: 8192,
                expected_codec: "PackedRecordRefCodec",
                expected_discriminator: 0x10,
                raw_magic: Some(0xb7),
                raw_flags: Some(0x00),
                actual_discriminator: Some(0x00),
                in_range: true,
                checksum_ok: true,
                magic_ok: true,
                codec_ok: false,
                reachable: true,
                failure: Some("btree_node_codec_discriminator_mismatch"),
            }],
            details: vec![loom_store::StoreRootCodecDiagnostic {
                root_name: "owner_tokens",
                family_id: Some(0x0110),
                root_page: 9,
                byte_offset: 8192,
                expected_codec: "PackedRecordRefCodec",
                expected_discriminator: 0x10,
                raw_magic: Some(0xb7),
                raw_flags: Some(0x00),
                actual_discriminator: Some(0x00),
                in_range: true,
                checksum_ok: true,
                magic_ok: true,
                codec_ok: false,
                reachable: true,
                failure: Some("btree_node_codec_discriminator_mismatch"),
            }],
        };

        let json = root_codec_diagnostics_json(&diagnostics);
        assert_eq!(json["state"], "error");
        assert_eq!(json["checked"], 1);
        assert_eq!(json["failures"], 1);
        assert_eq!(json["roots"][0]["root"], "owner_tokens");
        assert_eq!(json["roots"][0]["family_id"], 0x0110);
        assert_eq!(json["roots"][0]["expected_discriminator"], 0x10);
        assert_eq!(json["roots"][0]["actual_discriminator"], 0x00);
        assert_eq!(
            json["roots"][0]["failure"],
            "btree_node_codec_discriminator_mismatch"
        );
    }
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
            let metric = vector_metric_wire_tag(parse_vector_metric(&metric)?);
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Vector",
                "create",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    WireValue::Uint(dim as u64),
                    WireValue::int(metric),
                ],
            )?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Vector",
                "upsert",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    id.to_value(),
                    WireValue::Bytes(vector),
                    WireValue::Bytes(metadata),
                ],
            )
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Vector",
                "upsert_source",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    id.to_value(),
                    WireValue::Bytes(vector),
                    WireValue::Bytes(metadata),
                    WireValue::Bytes(source_text),
                    model_id.to_value(),
                    weights_digest.to_value(),
                ],
            )
        }
        VectorCmd::Get {
            store,
            workspace,
            name,
            id,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Vector",
                "get",
                vec![workspace.to_value(), name.to_value(), id.to_value()],
            )?
            else {
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Vector",
                "source_text",
                vec![workspace.to_value(), name.to_value(), id.to_value()],
            )?
            else {
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let ids_bytes = execute_generated_bytes(
                &client,
                "Vector",
                "ids",
                vec![workspace.to_value(), name.to_value(), prefix.to_value()],
            )?;
            if let Some(out) = out {
                write_output(Some(&out), &ids_bytes).map_err(|e| e.to_string())
            } else {
                let ids = string_list_from_cbor(&ids_bytes)?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let index_keys_bytes = execute_generated_bytes(
                &client,
                "Vector",
                "metadata_index_keys",
                vec![workspace.to_value(), name.to_value()],
            )?;
            if let Some(out) = out {
                write_output(Some(&out), &index_keys_bytes).map_err(|e| e.to_string())
            } else {
                let index_keys = string_list_from_cbor(&index_keys_bytes)?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let changed = execute_generated_bool(
                &client,
                "Vector",
                "create_metadata_index",
                vec![workspace.to_value(), name.to_value(), key.to_value()],
            )?;
            println!("{changed}");
            Ok(())
        }
        VectorCmd::DropIndex {
            store,
            workspace,
            name,
            key,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let changed = execute_generated_bool(
                &client,
                "Vector",
                "drop_metadata_index",
                vec![workspace.to_value(), name.to_value(), key.to_value()],
            )?;
            println!("{changed}");
            Ok(())
        }
        VectorCmd::Delete {
            store,
            workspace,
            name,
            id,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let present = execute_generated_bool(
                &client,
                "Vector",
                "delete",
                vec![workspace.to_value(), name.to_value(), id.to_value()],
            )?;
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
            let policy_int = match policy.as_str() {
                "exact" => 0_i32,
                "approximate-pq" => 1_i32,
                other => {
                    return Err(format!(
                        "unknown vector accelerator policy {other}; expected exact or approximate-pq"
                    ));
                }
            };
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let hits_bytes = execute_generated_bytes(
                &client,
                "Vector",
                "search_policy",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    WireValue::Bytes(query),
                    (k as u64).to_value(),
                    WireValue::Bytes(filter),
                    policy_int.to_value(),
                    (threshold as u64).to_value(),
                    (ef as u64).to_value(),
                    (pq_m as u64).to_value(),
                    (pq_k as u64).to_value(),
                    (pq_iters as u64).to_value(),
                ],
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
    current_token: String,
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
            expected_token,
            expect_absent,
            metric,
            format,
        } => {
            let source_text = text_input(text, text_file, "text")?;
            let expected_token = expected_token
                .map(|token| hex::decode(token).map_err(|e| format!("expected token: {e}")))
                .transpose()?;
            // Keep raw metadata CBOR bytes: forwarded as-is to the generated Vector surface.
            let metadata_bytes = match metadata {
                Some(path) => read_input(&path).map_err(|e| e.to_string())?,
                None => Vec::new(),
            };
            // Remote text upsert embeds locally and sends the computed vector plus source text through
            // the generated Vector mutation surface. The server never receives a local model path.
            if remote::target_is_remote(&store)? {
                let handle = resolve_local_text_embedding(embedding_instance.as_deref())?;
                let model = handle
                    .model()
                    .ok_or_else(|| "text embedding provider did not expose a model".to_string())?;
                let vectors = handle
                    .embed(std::slice::from_ref(&source_text))
                    .map_err(|e| e.to_string())?;
                let vector_bytes = vector_floats_to_bytes(&vectors[0]);
                let metric_tag = vector_metric_wire_tag(parse_vector_metric(&metric)?);
                let request = loom_wire::vector::TextUpsertRequest {
                    workspace: workspace.clone(),
                    name: name.clone(),
                    id: id.clone(),
                    vector: vector_bytes,
                    metadata: metadata_bytes,
                    source_text: source_text.clone().into_bytes(),
                    model_id: Some(model.model_id.clone()),
                    weights_digest: model.weights_digest.clone(),
                    create,
                    metric: metric_tag as i32,
                    expected_token: expected_token.clone(),
                    expect_absent,
                };
                let client = remote::open_cli_generated_client(&store, keys)?;
                let report_bytes = execute_generated_bytes(
                    &client,
                    "Vector",
                    "vector_text_upsert",
                    vec![WireValue::Bytes(
                        loom_wire::vector::text_upsert_request_to_cbor(&request),
                    )],
                )?;
                let report = loom_wire::vector::text_upsert_report_from_cbor(&report_bytes)
                    .map_err(|e| e.to_string())?;
                let view = VectorTextUpsertView {
                    store,
                    workspace,
                    collection: name,
                    id,
                    embedding_instance: model.model_id.clone(),
                    model: vector_text_model_view(model),
                    current_token: hex::encode(report.current_token),
                };
                return print_vector_text_upsert(&view, &format);
            }
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
            let vectors = resolved
                .handle
                .embed(std::slice::from_ref(&source_text))
                .map_err(|e| e.to_string())?;
            let vector_bytes = vector_floats_to_bytes(&vectors[0]);
            drop(loom);
            let metric_tag = vector_metric_wire_tag(parse_vector_metric(&metric)?);
            let request = loom_wire::vector::TextUpsertRequest {
                workspace: workspace.clone(),
                name: name.clone(),
                id: id.clone(),
                vector: vector_bytes,
                metadata: metadata_bytes,
                source_text: source_text.clone().into_bytes(),
                model_id: Some(model.model_id.clone()),
                weights_digest: model.weights_digest.clone(),
                create,
                metric: metric_tag as i32,
                expected_token,
                expect_absent,
            };
            let client = remote::open_cli_generated_client(&store, keys)?;
            let report_bytes = execute_generated_bytes(
                &client,
                "Vector",
                "vector_text_upsert",
                vec![WireValue::Bytes(
                    loom_wire::vector::text_upsert_request_to_cbor(&request),
                )],
            )?;
            let report = loom_wire::vector::text_upsert_report_from_cbor(&report_bytes)
                .map_err(|e| e.to_string())?;
            let view = VectorTextUpsertView {
                store,
                workspace,
                collection: name,
                id,
                embedding_instance: resolved.instance.name,
                model: vector_text_model_view(model),
                current_token: hex::encode(report.current_token),
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
                let client = remote::open_cli_read_only_generated_client(&store, keys)?;
                let hits_bytes = execute_generated_bytes(
                    &client,
                    "Vector",
                    "search",
                    vec![
                        workspace.to_value(),
                        name.to_value(),
                        WireValue::Bytes(query_bytes),
                        (top_k as u64).to_value(),
                        WireValue::Bytes(filter_bytes),
                    ],
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
                    let source_text = execute_generated_optional_bytes(
                        &client,
                        "Vector",
                        "source_text",
                        vec![workspace.to_value(), name.to_value(), hit_id.to_value()],
                    )?
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
                "vector_text_upsert\t{}\t{}\t{}\tembedding_instance={}\tmodel={}\tcurrent_token={}",
                view.workspace,
                view.collection,
                view.id,
                view.embedding_instance,
                view.model.model_id,
                view.current_token
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

#[derive(serde::Deserialize)]
struct StudioReindexEnqueueResult {
    workspace: String,
    profile: String,
    job_path: String,
    state: String,
    source_digest: String,
    model_id: String,
    vector_records_indexed: u64,
    vector_records_deleted: u64,
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let result_json = execute_generated_string(
                &client,
                "StudioMaintenance",
                "studio_reindex_json",
                vec![workspace.to_value(), profile.to_value()],
            )?;
            let result: StudioReindexEnqueueResult =
                serde_json::from_str(&result_json).map_err(|error| error.to_string())?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let report_json = execute_generated_string(
                &client,
                "StudioMaintenance",
                "studio_revisions_rebuild_json",
                vec![workspace.to_value(), profile.to_value(), dry_run.to_value()],
            )?;
            let report: RevisionRebuildReport =
                serde_json::from_str(&report_json).map_err(|error| error.to_string())?;
            print_revision_rebuild_report(&report, &format)
        }
    }
}

#[derive(Debug, serde::Deserialize)]
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

#[cfg(all(test, feature = "integration-tests"))]
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

#[cfg(all(test, feature = "integration-tests"))]
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

#[cfg(all(test, feature = "integration-tests"))]
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

#[cfg(all(test, feature = "integration-tests"))]
fn rebuild_pages_revision_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    scope_id: &str,
    dry_run: bool,
) -> Result<RevisionRebuildReport, String> {
    loom.authorize(workspace, FacetKind::Vcs, AclRight::Write)
        .map_err(|e| e.to_string())?;
    let log = loom_pages::page_operation_log(loom.store(), scope_id).map_err(|e| e.to_string())?;
    if log.records.is_empty() {
        return Err("pages operation log not found".to_string());
    }
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

#[cfg(all(test, feature = "integration-tests"))]
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

#[cfg(all(test, feature = "integration-tests"))]
fn apply_revision_backfill(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    scope_id: &str,
    profile: &str,
    dry_run: bool,
    updates: Vec<RevisionBackfillUpdate>,
) -> Result<RevisionRebuildReport, String> {
    let (mut index, index_present_before) =
        match load_optional_current_revision_index(loom, workspace, scope_id)
            .map_err(|e| e.to_string())?
        {
            Some(index) => (index, true),
            None => (RevisionIndex::new(), false),
        };
    let candidates = updates.len() as u64;
    let backfill = index
        .backfill_missing_current(scope_id, updates)
        .map_err(|e| e.to_string())?;
    if !dry_run && backfill.inserted > 0 {
        persist_current_revision_index(loom, workspace, scope_id, FacetKind::Document, &index)
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

#[cfg(all(test, feature = "integration-tests"))]
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

#[cfg(all(test, feature = "integration-tests"))]
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
            let request_json = serde_json::json!({
                "embedding-instance": embedding_instance.clone(),
            })
            .to_string();
            let client = remote::open_cli_generated_client(&store, keys)?;
            let binding_json = execute_generated_string(
                &client,
                "Vector",
                "vector_workspace_configure_json",
                vec![workspace.to_value(), request_json.to_value()],
            )?;
            let binding: loom_inference::VectorWorkspaceBinding =
                serde_json::from_str(&binding_json).map_err(|error| error.to_string())?;
            print_vector_workspace_binding(&binding, &format)
        }
    }
}

#[cfg(all(test, feature = "integration-tests"))]
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
        workspace: ns.to_string(),
        profile: profile.to_string(),
        job_path,
        state: job.state.as_str().to_string(),
        source_digest: source_digest.to_string(),
        model_id: job.stamp.model_id,
        vector_records_indexed: vector_records_indexed as u64,
        vector_records_deleted: vector_records_deleted as u64,
    })
}

#[cfg(all(test, feature = "integration-tests"))]
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

#[cfg(all(test, feature = "integration-tests"))]
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

#[cfg(all(test, feature = "integration-tests"))]
fn studio_meetings_profile_ids(ns: WorkspaceId, profile: &str) -> Vec<String> {
    match profile {
        "all" | "meetings" => vec![ns.to_string()],
        profile => vec![profile.to_string()],
    }
}

#[cfg(all(test, feature = "integration-tests"))]
fn meetings_vector_collection(profile_id: &str) -> String {
    format!("meetings/{profile_id}")
}

#[cfg(all(test, feature = "integration-tests"))]
fn meetings_vector_id(output: &ProjectionOutput) -> String {
    output
        .output_ref
        .strip_prefix("vector:")
        .unwrap_or(&output.output_ref)
        .to_string()
}

#[cfg(all(test, feature = "integration-tests"))]
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

#[cfg(all(test, feature = "integration-tests"))]
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

#[cfg(all(test, feature = "integration-tests"))]
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

#[cfg(all(test, feature = "integration-tests"))]
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

#[cfg(all(test, feature = "integration-tests"))]
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
                result.workspace,
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
            out.push_str(&json_string(&result.workspace));
            out.push_str(",\"profile\":");
            out.push_str(&json_string(&result.profile));
            out.push_str(",\"state\":");
            out.push_str(&json_string(&result.state));
            out.push_str(",\"job_path\":");
            out.push_str(&json_string(&result.job_path));
            out.push_str(",\"source_digest\":");
            out.push_str(&json_string(&result.source_digest));
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
                "vector_workspace\t{}\tembedding_instance={}",
                binding.workspace, binding.embedding_instance
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Graph",
                "upsert_node",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    id.to_value(),
                    WireValue::Bytes(props),
                ],
            )
        }
        GraphCmd::GetNode {
            store,
            workspace,
            name,
            id,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Graph",
                "get_node",
                vec![workspace.to_value(), name.to_value(), id.to_value()],
            )?
            else {
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Graph",
                "remove_node",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    id.to_value(),
                    cascade.to_value(),
                ],
            )
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Graph",
                "upsert_edge",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    id.to_value(),
                    src.to_value(),
                    dst.to_value(),
                    label.to_value(),
                    WireValue::Bytes(props),
                ],
            )
        }
        GraphCmd::GetEdge {
            store,
            workspace,
            name,
            id,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Graph",
                "get_edge",
                vec![workspace.to_value(), name.to_value(), id.to_value()],
            )?
            else {
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let present = execute_generated_bool(
                &client,
                "Graph",
                "remove_edge",
                vec![workspace.to_value(), name.to_value(), id.to_value()],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Graph",
                "neighbors",
                vec![workspace.to_value(), name.to_value(), id.to_value()],
            )?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        GraphCmd::OutEdges {
            store,
            workspace,
            name,
            id,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Graph",
                "out_edges",
                vec![workspace.to_value(), name.to_value(), id.to_value()],
            )?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        GraphCmd::InEdges {
            store,
            workspace,
            name,
            id,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Graph",
                "in_edges",
                vec![workspace.to_value(), name.to_value(), id.to_value()],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Graph",
                "reachable",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    start.to_value(),
                    max_depth.to_value(),
                    via_label.unwrap_or_default().to_value(),
                ],
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(path) = execute_generated_optional_bytes(
                &client,
                "Graph",
                "shortest_path",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    from.to_value(),
                    to.to_value(),
                    via_label.unwrap_or_default().to_value(),
                ],
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Graph",
                "query",
                vec![workspace.to_value(), name.to_value(), query.to_value()],
            )?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        GraphCmd::ExplainQuery {
            store,
            workspace,
            name,
            query,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Graph",
                "explain_query",
                vec![workspace.to_value(), name.to_value(), query.to_value()],
            )?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let seq = execute_generated_u64(
                &client,
                "Ledger",
                "append",
                vec![
                    workspace.to_value(),
                    collection.to_value(),
                    WireValue::Bytes(payload),
                ],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(payload) = execute_generated_optional_bytes(
                &client,
                "Ledger",
                "get",
                vec![workspace.to_value(), collection.to_value(), seq.to_value()],
            )?
            else {
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(head) = execute_generated_optional_digest(
                &client,
                "Ledger",
                "head",
                vec![workspace.to_value(), collection.to_value()],
            )?
            else {
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let len = execute_generated_u64(
                &client,
                "Ledger",
                "len",
                vec![workspace.to_value(), collection.to_value()],
            )?;
            println!("{len}");
            Ok(())
        }
        LedgerCmd::Verify {
            store,
            workspace,
            collection,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Ledger",
                "verify",
                vec![workspace.to_value(), collection.to_value()],
            )?;
            println!("ok");
            Ok(())
        }
    }
}

fn execute_generated_value(
    client: &remote::CliGeneratedClient,
    interface: &str,
    method: &str,
    args: Vec<WireValue>,
) -> Result<WireValue, String> {
    client.execute_unary(&remote::CliGeneratedOperation::new(
        interface, method, args,
    )?)
}

fn execute_generated_void(
    client: &remote::CliGeneratedClient,
    interface: &str,
    method: &str,
    args: Vec<WireValue>,
) -> Result<(), String> {
    match execute_generated_value(client, interface, method, args)? {
        WireValue::Null => Ok(()),
        value => Err(format!(
            "{interface}.{method} returned unexpected value {value:?}"
        )),
    }
}

fn execute_generated_bytes(
    client: &remote::CliGeneratedClient,
    interface: &str,
    method: &str,
    args: Vec<WireValue>,
) -> Result<Vec<u8>, String> {
    match execute_generated_value(client, interface, method, args)? {
        WireValue::Bytes(bytes) => Ok(bytes),
        value => Err(format!(
            "{interface}.{method} returned unexpected value {value:?}"
        )),
    }
}

fn execute_generated_key_add_wrap(
    client: &remote::CliGeneratedClient,
    new_spec: KeySpec,
    allow_no_recovery: bool,
) -> Result<(), String> {
    let (method, credential) = generated_key_add_wrap_method_and_credential(new_spec);
    execute_generated_void(
        client,
        "KeySource",
        method,
        vec![WireValue::Bytes(credential), allow_no_recovery.to_value()],
    )
}

fn generated_key_add_wrap_method_and_credential(new_spec: KeySpec) -> (&'static str, Vec<u8>) {
    match new_spec {
        KeySpec::Passphrase(passphrase) => ("key_add_wrap_keyed", passphrase.as_bytes().to_vec()),
        KeySpec::RawKek(kek) => ("key_add_wrap_with_kek", kek.to_vec()),
    }
}

#[cfg(test)]
mod mu6i_d4_generated_cli_source_tests {
    use super::*;

    #[test]
    fn keysource_generated_wrap_method_selection_is_type_directed() {
        let (method, credential) =
            generated_key_add_wrap_method_and_credential(KeySpec::passphrase("secret"));
        assert_eq!(method, "key_add_wrap_keyed");
        assert_eq!(credential, b"secret");

        let (method, credential) =
            generated_key_add_wrap_method_and_credential(KeySpec::raw_kek([0x5a; 32]));
        assert_eq!(method, "key_add_wrap_with_kek");
        assert_eq!(credential, vec![0x5a; 32]);
    }

    #[test]
    fn generated_cli_administration_source_uses_required_methods() {
        let rows = [
            (
                "store key",
                include_str!("main.rs"),
                "StoreCmd::Key { action } => match action",
                &[
                    "open_cli_generated_client",
                    "\"KeySource\"",
                    "execute_generated_key_add_wrap",
                    "\"key_remove_wrap\"",
                ][..],
                &[
                    "admin_key_add_wrap",
                    "admin_key_remove_wrap",
                    "FileStore::open(",
                ][..],
            ),
            (
                "audit compact",
                include_str!("audit_cmd.rs"),
                "fn run_audit_compact",
                &[
                    "open_cli_generated_client",
                    "\"Audit\"",
                    "\"audit_compact\"",
                    "audit_compact_result_from_cbor",
                ][..],
                &[
                    "cli_open_loom(",
                    "require_global_admin_actor",
                    "audit_prune_through",
                ][..],
            ),
            (
                "maintenance",
                include_str!("daemon_cmd.rs"),
                "fn run_daemon_maintenance",
                &[
                    "open_cli_generated_client",
                    "\"StoreAdmin\"",
                    "\"store_maintenance_status\"",
                    "\"store_maintenance_policy_set\"",
                    "\"store_maintenance_run\"",
                ][..],
                &[
                    "cli_open_loom(",
                    "cli_open_loom_read(",
                    "set_store_maintenance_policy",
                    "run_store_maintenance_once(",
                ][..],
            ),
        ];

        for (name, source, marker, required, rejected) in rows {
            let body = braced_body_after(source, marker);
            for needle in required {
                assert!(body.contains(needle), "{name} missing {needle}");
            }
            for needle in rejected {
                assert!(!body.contains(needle), "{name} retains {needle}");
            }
        }
    }

    fn braced_body_after<'a>(source: &'a str, marker: &str) -> &'a str {
        let start = source.rfind(marker).expect("marker present");
        let search_start = start + marker.len();
        let brace = source[search_start..].find('{').expect("body starts") + search_start;
        let mut depth = 0usize;
        for (offset, ch) in source[brace..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &source[brace..=brace + offset];
                    }
                }
                _ => {}
            }
        }
        panic!("body closes");
    }
}

fn execute_generated_optional_bytes(
    client: &remote::CliGeneratedClient,
    interface: &str,
    method: &str,
    args: Vec<WireValue>,
) -> Result<Option<Vec<u8>>, String> {
    match execute_generated_value(client, interface, method, args)? {
        WireValue::Null => Ok(None),
        WireValue::Bytes(bytes) => Ok(Some(bytes)),
        value => Err(format!(
            "{interface}.{method} returned unexpected value {value:?}"
        )),
    }
}

fn execute_generated_bytes_list(
    client: &remote::CliGeneratedClient,
    interface: &str,
    method: &str,
    args: Vec<WireValue>,
) -> Result<Vec<Vec<u8>>, String> {
    match execute_generated_value(client, interface, method, args)? {
        WireValue::Array(items) => items
            .into_iter()
            .map(|item| match item {
                WireValue::Bytes(bytes) => Ok(bytes),
                value => Err(format!(
                    "{interface}.{method} returned unexpected list item {value:?}"
                )),
            })
            .collect(),
        value => Err(format!(
            "{interface}.{method} returned unexpected value {value:?}"
        )),
    }
}

fn execute_generated_optional_digest(
    client: &remote::CliGeneratedClient,
    interface: &str,
    method: &str,
    args: Vec<WireValue>,
) -> Result<Option<Digest>, String> {
    match execute_generated_value(client, interface, method, args)? {
        WireValue::Null => Ok(None),
        WireValue::Text(text) => Digest::parse(&text).map(Some).map_err(|e| e.to_string()),
        value => Err(format!(
            "{interface}.{method} returned unexpected value {value:?}"
        )),
    }
}

fn execute_generated_digest_list(
    client: &remote::CliGeneratedClient,
    interface: &str,
    method: &str,
    args: Vec<WireValue>,
) -> Result<Vec<Digest>, String> {
    match execute_generated_value(client, interface, method, args)? {
        WireValue::Array(items) => items
            .into_iter()
            .map(|item| match item {
                WireValue::Text(text) => Digest::parse(&text).map_err(|e| e.to_string()),
                value => Err(format!(
                    "{interface}.{method} returned unexpected list item {value:?}"
                )),
            })
            .collect(),
        value => Err(format!(
            "{interface}.{method} returned unexpected value {value:?}"
        )),
    }
}

fn execute_generated_string(
    client: &remote::CliGeneratedClient,
    interface: &str,
    method: &str,
    args: Vec<WireValue>,
) -> Result<String, String> {
    match execute_generated_value(client, interface, method, args)? {
        WireValue::Text(text) => Ok(text),
        value => Err(format!(
            "{interface}.{method} returned unexpected value {value:?}"
        )),
    }
}

fn execute_generated_optional_string(
    client: &remote::CliGeneratedClient,
    interface: &str,
    method: &str,
    args: Vec<WireValue>,
) -> Result<Option<String>, String> {
    match execute_generated_value(client, interface, method, args)? {
        WireValue::Null => Ok(None),
        WireValue::Text(text) => Ok(Some(text)),
        value => Err(format!(
            "{interface}.{method} returned unexpected value {value:?}"
        )),
    }
}

fn execute_generated_bool(
    client: &remote::CliGeneratedClient,
    interface: &str,
    method: &str,
    args: Vec<WireValue>,
) -> Result<bool, String> {
    match execute_generated_value(client, interface, method, args)? {
        WireValue::Bool(value) => Ok(value),
        value => Err(format!(
            "{interface}.{method} returned unexpected value {value:?}"
        )),
    }
}

fn execute_generated_u64(
    client: &remote::CliGeneratedClient,
    interface: &str,
    method: &str,
    args: Vec<WireValue>,
) -> Result<u64, String> {
    match execute_generated_value(client, interface, method, args)? {
        WireValue::Uint(value) => Ok(value),
        value => Err(format!(
            "{interface}.{method} returned unexpected value {value:?}"
        )),
    }
}

fn execute_generated_digest_string(
    client: &remote::CliGeneratedClient,
    interface: &str,
    method: &str,
    args: Vec<WireValue>,
) -> Result<String, String> {
    match execute_generated_value(client, interface, method, args)? {
        WireValue::Text(text) => Ok(text),
        value => Err(format!(
            "{interface}.{method} returned unexpected value {value:?}"
        )),
    }
}

fn generated_import_report_from_cbor(
    bytes: &[u8],
) -> Result<loom_interchange::ImportReport, String> {
    loom_interchange::ImportReport::decode(bytes).map_err(|e| e.to_string())
}

fn generated_import_report_from_value(
    value: WireValue,
) -> Result<loom_interchange::ImportReport, String> {
    let bytes = loom_codec::encode(&value).map_err(|e| e.to_string())?;
    generated_import_report_from_cbor(&bytes)
}

fn generated_car_import_result_from_cbor(bytes: &[u8]) -> Result<CarImportResult, String> {
    let WireValue::Array(fields) = loom_codec::decode(bytes).map_err(|e| e.to_string())? else {
        return Err("generated CAR import result must be a CBOR array".to_string());
    };
    let [workspace, root_cid, blocks_read, report] = fields.as_slice() else {
        return Err("generated CAR import result has an invalid field shape".to_string());
    };
    let workspace = match workspace {
        WireValue::Null => None,
        WireValue::Text(id) => Some(WorkspaceId::parse(id).map_err(|e| e.to_string())?),
        _ => return Err("generated CAR import workspace must be null or text".to_string()),
    };
    let WireValue::Text(root_cid_hex) = root_cid else {
        return Err("generated CAR import root cid must be text".to_string());
    };
    let WireValue::Uint(blocks_read) = blocks_read else {
        return Err("generated CAR import block count must be uint".to_string());
    };
    Ok(CarImportResult {
        workspace,
        root_cid_hex: root_cid_hex.clone(),
        blocks_read: *blocks_read,
        report: generated_import_report_from_value(report.clone())?,
    })
}

fn generated_archive_import_result_from_cbor(bytes: &[u8]) -> Result<ArchiveImportResult, String> {
    let WireValue::Array(fields) = loom_codec::decode(bytes).map_err(|e| e.to_string())? else {
        return Err("generated archive import result must be a CBOR array".to_string());
    };
    let [manifest, report] = fields.as_slice() else {
        return Err("generated archive import result has an invalid field shape".to_string());
    };
    let manifest = generated_archive_manifest(manifest)?;
    let report = generated_import_report_from_value(report.clone())?;
    Ok(ArchiveImportResult { manifest, report })
}

fn generated_archive_manifest(
    value: &WireValue,
) -> Result<loom_interchange::ArchiveManifest, String> {
    let WireValue::Array(fields) = value else {
        return Err("generated archive manifest must be a CBOR array".to_string());
    };
    let [
        WireValue::Text(archive_id),
        WireValue::Text(kind),
        WireValue::Text(root_digest),
        entries,
    ] = fields.as_slice()
    else {
        return Err("generated archive manifest has an invalid field shape".to_string());
    };
    let kind = parse_archive_kind(kind)?;
    let root_digest = Digest::parse(root_digest).map_err(|e| e.to_string())?;
    let mut manifest =
        loom_interchange::ArchiveManifest::new(archive_id.clone(), kind, root_digest)
            .map_err(|e| e.to_string())?;
    let WireValue::Array(entries) = entries else {
        return Err("generated archive manifest entries must be an array".to_string());
    };
    manifest.entries = entries
        .iter()
        .cloned()
        .map(loom_interchange::ArchiveEntry::from_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(manifest)
}

fn run_metrics(action: MetricsCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        MetricsCmd::PutDescriptor {
            store,
            workspace,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Metrics",
                "put_descriptor",
                vec![workspace.to_value(), WireValue::Bytes(bytes)],
            )
        }
        MetricsCmd::GetDescriptor {
            store,
            workspace,
            name,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Metrics",
                "get_descriptor",
                vec![workspace.to_value(), name.to_value()],
            )?
            else {
                return Err(format!("metric descriptor {name:?} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        MetricsCmd::PutObservation {
            store,
            workspace,
            descriptor,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Metrics",
                "put_observation",
                vec![
                    workspace.to_value(),
                    descriptor.to_value(),
                    WireValue::Bytes(bytes),
                ],
            )
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let bytes = execute_generated_bytes(
                &client,
                "Metrics",
                "query",
                vec![
                    workspace.to_value(),
                    descriptor.to_value(),
                    from.to_value(),
                    to.to_value(),
                    max_series.to_value(),
                    max_groups.to_value(),
                    max_samples.to_value(),
                    max_output_bytes.to_value(),
                    now.to_value(),
                ],
            )?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
    }
}

fn run_logs(action: LogsCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        LogsCmd::PutRecord {
            store,
            workspace,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            let record_id = execute_generated_string(
                &client,
                "Logs",
                "put_record",
                vec![workspace.to_value(), WireValue::Bytes(bytes)],
            )?;
            println!("{record_id}");
            Ok(())
        }
        LogsCmd::GetRecord {
            store,
            workspace,
            record_id,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Logs",
                "get_record",
                vec![workspace.to_value(), record_id.to_value()],
            )?
            else {
                return Err(format!("log record {record_id:?} not found"));
            };
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let bytes = execute_generated_bytes(
                &client,
                "Logs",
                "query",
                vec![
                    workspace.to_value(),
                    from.to_value(),
                    to.to_value(),
                    max_records.to_value(),
                    max_output_bytes.to_value(),
                ],
            )?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
    }
}

fn run_traces(action: TracesCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        TracesCmd::PutSpan {
            store,
            workspace,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Traces",
                "put_span",
                vec![workspace.to_value(), WireValue::Bytes(bytes)],
            )
        }
        TracesCmd::GetSpan {
            store,
            workspace,
            trace_id,
            span_id,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Traces",
                "get_span",
                vec![
                    workspace.to_value(),
                    trace_id.to_value(),
                    span_id.to_value(),
                ],
            )?
            else {
                return Err(format!("span {trace_id}/{span_id} not found"));
            };
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let bytes = execute_generated_bytes(
                &client,
                "Traces",
                "trace_spans",
                vec![
                    workspace.to_value(),
                    trace_id.to_value(),
                    max_spans.to_value(),
                    max_output_bytes.to_value(),
                ],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let bytes = execute_generated_bytes(
                &client,
                "Traces",
                "query",
                vec![
                    workspace.to_value(),
                    from.to_value(),
                    to.to_value(),
                    max_spans.to_value(),
                    max_output_bytes.to_value(),
                ],
            )?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
    }
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let bytes = execute_generated_bytes(
                &client,
                "Program",
                "program_put",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    WireValue::Bytes(manifest.encode()),
                    WireValue::Bytes(body),
                ],
            )?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let bytes = execute_generated_bytes(
                &client,
                "Program",
                "program_put",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    WireValue::Bytes(manifest.encode()),
                    WireValue::Bytes(source.into_bytes()),
                ],
            )?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let bytes = execute_generated_bytes(
                &client,
                "Program",
                "program_put",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    WireValue::Bytes(manifest.encode()),
                    WireValue::Bytes(source.into_bytes()),
                ],
            )?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        ProgramCmd::Inspect {
            store,
            workspace,
            name,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Program",
                "program_inspect",
                vec![workspace.to_value(), name.to_value()],
            )?
            else {
                return Err(format!("program {name:?} not found"));
            };
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        ProgramCmd::Get {
            store,
            workspace,
            name,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(bytes) = execute_generated_optional_bytes(
                &client,
                "Program",
                "program_get",
                vec![workspace.to_value(), name.to_value()],
            )?
            else {
                return Err(format!("program {name:?} not found"));
            };
            let body = program_get_body_from_cbor(&bytes)?;
            write_output(out.as_deref(), &body).map_err(|e| e.to_string())
        }
        ProgramCmd::List {
            store,
            workspace,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let bytes = execute_generated_bytes(
                &client,
                "Program",
                "program_list",
                vec![workspace.to_value()],
            )?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        ProgramCmd::Remove {
            store,
            workspace,
            name,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let removed = execute_generated_bool(
                &client,
                "Program",
                "program_remove",
                vec![workspace.to_value(), name.to_value()],
            )?;
            println!("{}", if removed { "removed" } else { "missing" });
            Ok(())
        }
    }
}

fn program_get_body_from_cbor(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let WireValue::Array(mut fields) = loom_codec::decode(bytes).map_err(|e| e.to_string())? else {
        return Err("program_get result must be a CBOR array".to_string());
    };
    if fields.len() != 2 {
        return Err("program_get result must contain record and body".to_string());
    }
    match fields.remove(1) {
        WireValue::Bytes(body) => Ok(body),
        _ => Err("program_get body must be bytes".to_string()),
    }
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Columnar",
                "create",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    WireValue::Bytes(columns),
                    (target_segment_rows as u64).to_value(),
                ],
            )?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Columnar",
                "append",
                vec![workspace.to_value(), name.to_value(), WireValue::Bytes(row)],
            )
        }
        ColumnarCmd::Scan {
            store,
            workspace,
            name,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Columnar",
                "scan",
                vec![workspace.to_value(), name.to_value()],
            )?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        ColumnarCmd::Columns {
            store,
            workspace,
            name,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Columnar",
                "columns",
                vec![workspace.to_value(), name.to_value()],
            )?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        ColumnarCmd::Rows {
            store,
            workspace,
            name,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let rows = execute_generated_u64(
                &client,
                "Columnar",
                "rows",
                vec![workspace.to_value(), name.to_value()],
            )?;
            println!("{rows}");
            Ok(())
        }
        ColumnarCmd::Compact {
            store,
            workspace,
            name,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Columnar",
                "compact",
                vec![workspace.to_value(), name.to_value()],
            )
        }
        ColumnarCmd::Inspect {
            store,
            workspace,
            name,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Columnar",
                "inspect",
                vec![workspace.to_value(), name.to_value()],
            )?;
            write_output(out.as_deref(), &encoded).map_err(|e| e.to_string())
        }
        ColumnarCmd::SourceDigest {
            store,
            workspace,
            name,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let digest_bytes = execute_generated_bytes(
                &client,
                "Columnar",
                "source_digest",
                vec![workspace.to_value(), name.to_value()],
            )?;
            let digest = match loom_codec::decode(&digest_bytes).map_err(|e| e.to_string())? {
                WireValue::Text(text) => text,
                _ => return Err("columnar source digest must be CBOR text".to_string()),
            };
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Columnar",
                "select",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    WireValue::Bytes(columns),
                    WireValue::Bytes(filter),
                ],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Columnar",
                "aggregate",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    WireValue::Bytes(aggregates),
                    WireValue::Bytes(filter),
                ],
            )?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_bytes(
                &client,
                "Columnar",
                "columnar_import_arrow",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    WireValue::Bytes(bytes),
                    (target_segment_rows as u64).to_value(),
                    replace.to_value(),
                    false.to_value(),
                ],
            )?;
            Ok(())
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_bytes(
                &client,
                "Columnar",
                "columnar_import_parquet",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    WireValue::Bytes(bytes),
                    (target_segment_rows as u64).to_value(),
                    replace.to_value(),
                    false.to_value(),
                ],
            )?;
            Ok(())
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
            let plan = read_input(&plan).map_err(|e| e.to_string())?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Dataframe",
                "create",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    WireValue::Bytes(plan),
                ],
            )?;
            println!("created {name}");
            Ok(())
        }
        DataframeCmd::Collect {
            store,
            workspace,
            name,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let bytes = execute_generated_bytes(
                &client,
                "Dataframe",
                "collect",
                vec![workspace.to_value(), name.to_value()],
            )?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        DataframeCmd::Materialize {
            store,
            workspace,
            name,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let digest = execute_generated_optional_string(
                &client,
                "Dataframe",
                "materialize",
                vec![workspace.to_value(), name.to_value()],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let digest = execute_generated_string(
                &client,
                "Dataframe",
                "plan_digest",
                vec![workspace.to_value(), name.to_value()],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let bytes = execute_generated_bytes(
                &client,
                "Dataframe",
                "preview",
                vec![workspace.to_value(), name.to_value(), rows.to_value()],
            )?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        DataframeCmd::SourceDigests {
            store,
            workspace,
            name,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let bytes = execute_generated_bytes(
                &client,
                "Dataframe",
                "source_digests",
                vec![workspace.to_value(), name.to_value()],
            )?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Search",
                "create",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    WireValue::Bytes(mapping),
                ],
            )?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Search",
                "index",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    WireValue::Bytes(id),
                    WireValue::Bytes(doc),
                ],
            )
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let Some(doc) = execute_generated_optional_bytes(
                &client,
                "Search",
                "get",
                vec![workspace.to_value(), name.to_value(), WireValue::Bytes(id)],
            )?
            else {
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let present = execute_generated_bool(
                &client,
                "Search",
                "delete",
                vec![workspace.to_value(), name.to_value(), WireValue::Bytes(id)],
            )?;
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
            let has_prefix = prefix.is_some();
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let ids = execute_generated_bytes(
                &client,
                "Search",
                "ids",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    WireValue::Bytes(prefix.unwrap_or_default()),
                    has_prefix.to_value(),
                ],
            )?;
            write_output(out.as_deref(), &ids).map_err(|e| e.to_string())
        }
        SearchCmd::Remap {
            store,
            workspace,
            name,
            mapping,
        } => {
            let mapping = read_input(&mapping).map_err(|e| e.to_string())?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "Search",
                "remap",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    WireValue::Bytes(mapping),
                ],
            )
        }
        SearchCmd::Query {
            store,
            workspace,
            name,
            request,
            out,
        } => {
            let request = read_input(&request).map_err(|e| e.to_string())?;
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let response = execute_generated_bytes(
                &client,
                "Search",
                "query",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    WireValue::Bytes(request),
                ],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let status_bytes = execute_generated_bytes(
                &client,
                "Search",
                "status",
                vec![
                    workspace.to_value(),
                    name.to_value(),
                    engine_version.to_value(),
                ],
            )?;
            let (source_digest, status) = loom_store::decode_search_status_result(&status_bytes)
                .map_err(|e| e.to_string())?;
            print_search_status(
                &format,
                &workspace,
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
        #[cfg(feature = "mcp-daemon-cli-tests")]
        Command::McpDaemonCliTestHoldSession { store, millis } => {
            run_mcp_daemon_cli_test_hold_session(&store, millis, keys)
        }
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

#[cfg(feature = "mcp-daemon-cli-tests")]
fn run_mcp_daemon_cli_test_hold_session(
    store: &str,
    millis: u64,
    keys: &KeyOpts,
) -> Result<(), String> {
    let _ = keys;
    hold_daemon_session_for_test(store, std::time::Duration::from_millis(millis))
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let report_bytes = execute_generated_bytes(
                &client,
                "StoreAdmin",
                "store_bundle_import",
                vec![WireValue::Bytes(bytes), false.to_value()],
            )?;
            let report =
                loom_wire::store_admin::store_bundle_import_result_from_cbor(&report_bytes)
                    .map_err(|e| e.to_string())?;
            let facets = report.facets.join(",");
            println!(
                "imported {} [{}] ({} new, {} skipped)",
                report.workspace_name, facets, report.objects_transferred, report.objects_skipped
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
            let freshness_watermark = store_copy_freshness_watermark(&source);
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
                    freshness_watermark: freshness_watermark.clone(),
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
                    freshness_watermark: freshness_watermark.clone(),
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
                freshness_watermark,
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
                let client = remote::open_cli_generated_client(&store, keys)?;
                let new_source = resolve_new_key_source(new_key_source.as_deref(), keys)?;
                let new_spec = acquire_key_spec(&new_source, "New passphrase", true)?;
                execute_generated_key_add_wrap(&client, new_spec, allow_no_recovery)?;
                if client.target() == remote::CliExecutionTarget::Remote {
                    println!("added unlock wrap to remote store {store}");
                    return Ok(());
                }
                println!("added unlock wrap to {store}");
                Ok(())
            }
            KeyCmd::RemoveWrap {
                store,
                index,
                allow_no_recovery,
            } => {
                let client = remote::open_cli_generated_client(&store, keys)?;
                execute_generated_void(
                    &client,
                    "KeySource",
                    "key_remove_wrap",
                    vec![(index as u64).to_value(), allow_no_recovery.to_value()],
                )?;
                if client.target() == remote::CliExecutionTarget::Remote {
                    println!("removed unlock wrap {index} from remote store {store}");
                    return Ok(());
                }
                println!("removed unlock wrap {index} from {store}");
                Ok(())
            }
        },
        StoreCmd::Policy {
            store,
            fips_required,
            default_durability,
            facet_durability,
            clear_facet_durability,
        } => {
            let update_requested = fips_required.is_some()
                || default_durability.is_some()
                || !facet_durability.is_empty()
                || !clear_facet_durability.is_empty();
            let client = if update_requested {
                remote::open_cli_generated_client(&store, keys)?
            } else {
                remote::open_cli_read_only_generated_client(&store, keys)?
            };
            let bytes = if update_requested {
                let update = store_policy_update_from_cli(
                    fips_required,
                    default_durability.as_deref(),
                    facet_durability,
                    clear_facet_durability,
                )?;
                execute_generated_bytes(
                    &client,
                    "StoreAdmin",
                    "store_policy_set",
                    vec![WireValue::Bytes(
                        loom_wire::store_admin::store_policy_update_to_cbor(&update),
                    )],
                )?
            } else {
                execute_generated_bytes(&client, "StoreAdmin", "store_policy_get", Vec::new())?
            };
            let result = loom_wire::store_admin::store_policy_result_from_cbor(&bytes)
                .map_err(|e| e.to_string())?;
            println!("{}", store_policy_result_json(result));
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
            let new_source = resolve_new_key_source(new_key_source.as_deref(), keys)?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            let credential = store_rekey_credential_from_key_spec(acquire_key_spec(
                &new_source,
                "New passphrase",
                true,
            )?);
            let request = loom_wire::store_admin::StoreRekeyRequest {
                credential,
                reseal,
                suite,
            };
            let bytes = execute_generated_bytes(
                &client,
                "StoreAdmin",
                "store_rekey",
                vec![WireValue::Bytes(
                    loom_wire::store_admin::store_rekey_request_to_cbor(&request),
                )],
            )?;
            let result = loom_wire::store_admin::store_rekey_result_from_cbor(&bytes)
                .map_err(|e| e.to_string())?;
            println!(
                "{}",
                store_rekey_result_summary(client.target(), &store, result)
            );
            Ok(())
        }
        StoreCmd::Stat { store } => {
            let context = remote::open_cli_execution_context(&store)?;
            println!("{}", remote::generated_store_stat_json(context, keys)?);
            Ok(())
        }
        StoreCmd::Attribution {
            store,
            workspace,
            max_objects,
            examples,
            format,
        } => run_store_attribution(&store, &workspace, max_objects, examples, &format, keys),
        StoreCmd::PreflightReplacement {
            store,
            workspace,
            live_store,
            candidate_report,
            force_owner_approval,
            backup_store,
            format,
        } => run_store_replacement_preflight(
            &store,
            &workspace,
            live_store.as_deref(),
            candidate_report.as_deref(),
            force_owner_approval.as_deref(),
            backup_store.as_deref(),
            &format,
            keys,
        ),
        StoreCmd::Replace {
            active_store,
            candidate_store,
            workspace,
            candidate_report,
            backup_store,
            report_file,
            force_owner_approval,
            dry_run,
            format,
        } => run_store_replacement_activation(
            StoreReplacementActivation {
                active_store: &active_store,
                candidate_store: &candidate_store,
                workspace: &workspace,
                candidate_report: &candidate_report,
                backup_store: &backup_store,
                report_file: report_file.as_deref(),
                force_owner_approval: force_owner_approval.as_deref(),
                dry_run,
                format: &format,
            },
            keys,
        ),
    }
}

#[derive(Clone)]
struct AttributionClass {
    class: String,
    count: u64,
    bytes: u64,
    examples: Vec<String>,
}

#[derive(Clone)]
struct StoreAttributionReport {
    store: String,
    workspace: String,
    physical_bytes: u64,
    live_bytes: u64,
    reusable_free_bytes: u64,
    candidate_reclaimable_bytes: u64,
    tail_free_bytes: u64,
    byte_attribution_mode: &'static str,
    page_class_bytes: Vec<loom_store::StorePageClass>,
    path_total: usize,
    sampled_paths: usize,
    path_sample_truncated: bool,
    sampled_path_byte_classes: Vec<AttributionClass>,
    path_classes: Vec<AttributionClass>,
    live_root_classes: Vec<AttributionClass>,
}

fn run_store_attribution(
    store: &str,
    workspace: &str,
    max_objects: usize,
    examples: usize,
    format: &str,
    keys: &KeyOpts,
) -> Result<(), String> {
    let loom = cli_open_loom_read(store, keys)?;
    let workspace_id = resolve_ns(&loom, workspace)?;
    let report = build_store_attribution_report(
        store,
        workspace,
        &loom,
        workspace_id,
        max_objects,
        examples,
    )?;
    match format {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&store_attribution_json(&report))
                    .map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            print_store_attribution_text(&report);
            Ok(())
        }
        other => Err(format!(
            "unknown attribution output format {other:?} (expected text or json)"
        )),
    }
}

fn build_store_attribution_report(
    store: &str,
    workspace: &str,
    loom: &Loom<FileStore>,
    workspace_id: WorkspaceId,
    max_objects: usize,
    examples: usize,
) -> Result<StoreAttributionReport, String> {
    let maintenance = loom
        .store()
        .store_maintenance_report(now_ms())
        .map_err(|e| e.to_string())?;
    let page_class = loom
        .store()
        .page_class_attribution(examples)
        .map_err(|e| e.to_string())?;
    let mut sampled_path_byte_classes =
        std::collections::BTreeMap::<String, AttributionClass>::new();
    let mut path_classes = std::collections::BTreeMap::<String, AttributionClass>::new();
    let paths = loom.walk(workspace_id, "").map_err(|e| e.to_string())?;
    for path in &paths {
        let class = store_attribution_path_class(path);
        add_attribution_class(&mut path_classes, &class, 0, path.clone(), examples);
    }
    for path in paths.iter().take(max_objects) {
        let class = format!("sampled_{}", store_attribution_path_class(path));
        let bytes = loom
            .read_file(workspace_id, path)
            .map(|bytes| bytes.len() as u64)
            .unwrap_or(0);
        add_attribution_class(
            &mut sampled_path_byte_classes,
            &class,
            bytes,
            path.clone(),
            examples,
        );
    }
    let diagnostics = cli_live_root_diagnostics(loom)?;
    let mut live_root_classes = Vec::new();
    for class in diagnostics.classes {
        live_root_classes.push(AttributionClass {
            class: class.class.to_string(),
            count: class.count,
            bytes: 0,
            examples: class
                .examples
                .into_iter()
                .map(|example| format!("{}={}", example.id, example.digest))
                .collect(),
        });
    }
    Ok(StoreAttributionReport {
        store: store.to_string(),
        workspace: workspace.to_string(),
        physical_bytes: maintenance.status.physical_bytes,
        live_bytes: maintenance.live_bytes,
        reusable_free_bytes: maintenance.reusable_free_bytes,
        candidate_reclaimable_bytes: maintenance.candidate_reclaimable_bytes,
        tail_free_bytes: maintenance.tail_free_bytes,
        byte_attribution_mode: "page_class_with_bounded_path_sample",
        page_class_bytes: page_class.classes,
        path_total: paths.len(),
        sampled_paths: paths.len().min(max_objects),
        path_sample_truncated: paths.len() > max_objects,
        sampled_path_byte_classes: attribution_classes_sorted(sampled_path_byte_classes),
        path_classes: attribution_classes_sorted(path_classes),
        live_root_classes,
    })
}

fn add_attribution_class(
    classes: &mut std::collections::BTreeMap<String, AttributionClass>,
    class: &str,
    bytes: u64,
    example: String,
    max_examples: usize,
) {
    let entry = classes
        .entry(class.to_string())
        .or_insert(AttributionClass {
            class: class.to_string(),
            count: 0,
            bytes: 0,
            examples: Vec::new(),
        });
    entry.count += 1;
    entry.bytes = entry.bytes.saturating_add(bytes);
    if entry.examples.len() < max_examples {
        entry.examples.push(example);
    }
}

fn attribution_classes_sorted(
    classes: std::collections::BTreeMap<String, AttributionClass>,
) -> Vec<AttributionClass> {
    let mut classes = classes.into_values().collect::<Vec<_>>();
    classes.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| b.count.cmp(&a.count)));
    classes
}

fn store_attribution_path_class(path: &str) -> String {
    if path.starts_with(".loom/facets/cas/") {
        "path.cas".to_string()
    } else if let Some(rest) = path.strip_prefix(".loom/facets/document/.bodies/") {
        let collection = rest.split('/').next().unwrap_or("");
        let collection = hex::decode(collection)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_else(|| collection.to_string());
        format!("path.document_body.{collection}")
    } else if path.starts_with(".loom/facets/document/.maps/") {
        "path.document_map".to_string()
    } else if path.starts_with(".loom/facets/document/") {
        "path.document_collection_root".to_string()
    } else if path.starts_with(".loom/substrate/tickets/") {
        "path.ticket_substrate".to_string()
    } else if path.starts_with(".loom/facets/queue/") {
        "path.queue_stream".to_string()
    } else if path.starts_with(".loom/facets/graph/") {
        "path.graph".to_string()
    } else if path.starts_with(".loom/facets/vector/") {
        "path.vector".to_string()
    } else if path.starts_with("results/") {
        "path.legacy_results".to_string()
    } else if path.starts_with("decisions/") {
        "path.legacy_decisions".to_string()
    } else {
        "path.other".to_string()
    }
}

fn store_attribution_json(report: &StoreAttributionReport) -> serde_json::Value {
    serde_json::json!({
        "store": report.store,
        "workspace": report.workspace,
        "physical_bytes": report.physical_bytes,
        "live_bytes": report.live_bytes,
        "reusable_free_bytes": report.reusable_free_bytes,
        "candidate_reclaimable_bytes": report.candidate_reclaimable_bytes,
        "tail_free_bytes": report.tail_free_bytes,
        "byte_attribution_mode": report.byte_attribution_mode,
        "page_class_bytes": page_classes_json(&report.page_class_bytes),
        "path_total": report.path_total,
        "sampled_paths": report.sampled_paths,
        "path_sample_truncated": report.path_sample_truncated,
        "sampled_path_byte_classes": attribution_classes_json(&report.sampled_path_byte_classes),
        "path_classes": attribution_classes_json(&report.path_classes),
        "live_root_classes": attribution_classes_json(&report.live_root_classes),
    })
}

fn attribution_classes_json(classes: &[AttributionClass]) -> serde_json::Value {
    serde_json::Value::Array(
        classes
            .iter()
            .map(|class| {
                serde_json::json!({
                    "class": class.class,
                    "count": class.count,
                    "bytes": class.bytes,
                    "examples": class.examples,
                })
            })
            .collect(),
    )
}

fn page_classes_json(classes: &[loom_store::StorePageClass]) -> serde_json::Value {
    serde_json::Value::Array(
        classes
            .iter()
            .map(|class| {
                serde_json::json!({
                    "class": class.class,
                    "pages": class.pages,
                    "bytes": class.bytes,
                    "examples": class.examples,
                })
            })
            .collect(),
    )
}

fn print_store_attribution_text(report: &StoreAttributionReport) {
    println!("store attribution");
    println!("store\t{}", report.store);
    println!("workspace\t{}", report.workspace);
    println!("physical_bytes\t{}", report.physical_bytes);
    println!("live_bytes\t{}", report.live_bytes);
    println!("reusable_free_bytes\t{}", report.reusable_free_bytes);
    println!(
        "candidate_reclaimable_bytes\t{}",
        report.candidate_reclaimable_bytes
    );
    println!("tail_free_bytes\t{}", report.tail_free_bytes);
    println!("byte_attribution_mode\t{}", report.byte_attribution_mode);
    for class in &report.page_class_bytes {
        println!(
            "page_class\t{}\tpages={}\tbytes={}\texamples={}",
            class.class,
            class.pages,
            class.bytes,
            class.examples.join(",")
        );
    }
    println!(
        "paths\ttotal={}\tsampled={}\ttruncated={}",
        report.path_total, report.sampled_paths, report.path_sample_truncated
    );
    print_attribution_classes("sampled_path_byte_class", &report.sampled_path_byte_classes);
    print_attribution_classes("path_class", &report.path_classes);
    print_attribution_classes("live_root_class", &report.live_root_classes);
}

fn print_attribution_classes(label: &str, classes: &[AttributionClass]) {
    for class in classes {
        println!(
            "{label}\t{}\tcount={}\tbytes={}\texamples={}",
            class.class,
            class.count,
            class.bytes,
            class.examples.join(",")
        );
    }
}

fn run_store_replacement_preflight(
    store: &str,
    workspace: &str,
    live_store: Option<&str>,
    candidate_report: Option<&str>,
    force_owner_approval: Option<&str>,
    backup_store: Option<&str>,
    format: &str,
    keys: &KeyOpts,
) -> Result<(), String> {
    let report = build_store_replacement_preflight_report(
        store,
        workspace,
        live_store,
        candidate_report,
        force_owner_approval,
        backup_store,
        keys,
    );
    print_store_replacement_preflight_report(&report, format)?;
    if report["ok"].as_bool() == Some(true) {
        Ok(())
    } else {
        Err("store replacement preflight failed; do not replace the active store".to_string())
    }
}

fn build_store_replacement_preflight_report(
    store: &str,
    workspace: &str,
    live_store: Option<&str>,
    candidate_report: Option<&str>,
    force_owner_approval: Option<&str>,
    backup_store: Option<&str>,
    keys: &KeyOpts,
) -> serde_json::Value {
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
                match loom_lanes::list_lanes(loom, workspace_id) {
                    Ok(lanes) => {
                        checks.push(store_preflight_check(
                            "lanes_list",
                            true,
                            format!("lanes={}", lanes.len()),
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
                    lane_id: None,
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
                match loom.vcs_namespace_preflight(workspace_id) {
                    Ok(report) if report.is_clean() => checks.push(store_preflight_check(
                        "vcs_namespace_preflight",
                        true,
                        "no VCS namespace collisions from legacy projections",
                    )),
                    Ok(report) => {
                        let collisions = report
                            .conflicts
                            .iter()
                            .map(|conflict| {
                                format!("{} -> {}", conflict.leaf_path, conflict.child_path)
                            })
                            .collect::<Vec<_>>()
                            .join("; ");
                        checks.push(store_preflight_check(
                            "vcs_namespace_preflight",
                            false,
                            format!("legacy projection collisions: {collisions}"),
                        ));
                    }
                    Err(error) => checks.push(store_preflight_check(
                        "vcs_namespace_preflight",
                        false,
                        error.to_string(),
                    )),
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
        match fs.root_codec_diagnostics() {
            Ok(diagnostics) => checks.push(store_replacement_root_codec_check(&diagnostics)),
            Err(error) => checks.push(store_preflight_check(
                "root_codecs",
                false,
                format!("root codec diagnostics are not readable by this binary: {error}"),
            )),
        }
    }

    push_store_replacement_freshness_checks(
        &mut checks,
        store,
        workspace,
        live_store,
        candidate_report,
        force_owner_approval,
        backup_store,
        keys,
    );

    let ok = checks
        .iter()
        .all(|check| check["ok"].as_bool() == Some(true));
    store_replacement_preflight_report(
        store,
        workspace,
        ok,
        &checks,
        live_store,
        candidate_report,
        force_owner_approval,
        backup_store,
    )
}

fn print_store_replacement_preflight_report(
    report: &serde_json::Value,
    format: &str,
) -> Result<(), String> {
    match format {
        "text" => print_store_replacement_preflight_text(report),
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
            );
        }
        other => {
            return Err(format!(
                "unknown preflight output format {other:?} (expected text or json)"
            ));
        }
    }
    Ok(())
}

struct StoreReplacementActivation<'a> {
    active_store: &'a str,
    candidate_store: &'a str,
    workspace: &'a str,
    candidate_report: &'a str,
    backup_store: &'a str,
    report_file: Option<&'a str>,
    force_owner_approval: Option<&'a str>,
    dry_run: bool,
    format: &'a str,
}

fn run_store_replacement_activation(
    options: StoreReplacementActivation<'_>,
    keys: &KeyOpts,
) -> Result<(), String> {
    let StoreReplacementActivation {
        active_store,
        candidate_store,
        workspace,
        candidate_report,
        backup_store,
        report_file,
        force_owner_approval,
        dry_run,
        format,
    } = options;
    let mut steps = Vec::new();
    let active_path = std::path::Path::new(active_store);
    let candidate_path = std::path::Path::new(candidate_store);
    let backup_path = std::path::Path::new(backup_store);
    if !active_path.exists() {
        steps.push(store_preflight_check(
            "active_store_exists",
            false,
            "active store does not exist",
        ));
        let report = store_replacement_activation_report(
            active_store,
            candidate_store,
            workspace,
            backup_store,
            dry_run,
            steps,
            serde_json::Value::Null,
            serde_json::Value::Null,
        );
        emit_store_replacement_activation_report(&report, format, report_file)?;
        return Err("store replacement activation failed".to_string());
    }
    if !candidate_path.exists() {
        steps.push(store_preflight_check(
            "candidate_store_exists",
            false,
            "candidate store does not exist",
        ));
        let report = store_replacement_activation_report(
            active_store,
            candidate_store,
            workspace,
            backup_store,
            dry_run,
            steps,
            serde_json::Value::Null,
            serde_json::Value::Null,
        );
        emit_store_replacement_activation_report(&report, format, report_file)?;
        return Err("store replacement activation failed".to_string());
    }
    if backup_path.exists() {
        steps.push(store_preflight_check(
            "backup_store_available",
            false,
            "backup store already exists; choose a new rollback artifact path",
        ));
        let report = store_replacement_activation_report(
            active_store,
            candidate_store,
            workspace,
            backup_store,
            dry_run,
            steps,
            serde_json::Value::Null,
            serde_json::Value::Null,
        );
        emit_store_replacement_activation_report(&report, format, report_file)?;
        return Err("store replacement activation failed".to_string());
    }
    steps.push(store_preflight_check(
        "input_paths",
        true,
        "active and candidate stores exist and backup path is unused",
    ));

    if dry_run {
        steps.push(store_preflight_check(
            "backup_store",
            true,
            "dry run; active store backup was not written",
        ));
    } else {
        std::fs::copy(active_path, backup_path)
            .map_err(|e| format!("copy active store to backup: {e}"))?;
        steps.push(store_preflight_check(
            "backup_store",
            true,
            format!("active store copied to {backup_store}"),
        ));
    }

    let preflight = build_store_replacement_preflight_report(
        candidate_store,
        workspace,
        Some(active_store),
        Some(candidate_report),
        force_owner_approval,
        Some(backup_store),
        keys,
    );
    if preflight["ok"].as_bool() != Some(true) {
        steps.push(store_preflight_check(
            "preflight",
            false,
            "candidate failed replacement preflight",
        ));
        let report = store_replacement_activation_report(
            active_store,
            candidate_store,
            workspace,
            backup_store,
            dry_run,
            steps,
            preflight,
            serde_json::Value::Null,
        );
        emit_store_replacement_activation_report(&report, format, report_file)?;
        return Err("store replacement activation failed".to_string());
    }
    steps.push(store_preflight_check(
        "preflight",
        true,
        "candidate passed replacement preflight",
    ));

    let mut post_replacement = serde_json::Value::Null;
    if dry_run {
        steps.push(store_preflight_check(
            "replace_active_store",
            true,
            "dry run; active store was not replaced",
        ));
    } else {
        let temp_store = store_replacement_temp_path(active_path);
        if temp_store.exists() {
            return Err(format!(
                "temporary replacement path {} already exists",
                temp_store.display()
            ));
        }
        std::fs::copy(candidate_path, &temp_store)
            .map_err(|e| format!("copy candidate store to temporary replacement: {e}"))?;
        FileStore::open_read(&temp_store)
            .map_err(|e| format!("temporary replacement is not readable: {e}"))?;
        std::fs::rename(&temp_store, active_path)
            .map_err(|e| format!("replace active store with candidate: {e}"))?;
        steps.push(store_preflight_check(
            "replace_active_store",
            true,
            "active store path replaced with candidate bytes",
        ));
        post_replacement = build_store_replacement_preflight_report(
            active_store,
            workspace,
            None,
            None,
            None,
            Some(backup_store),
            keys,
        );
        steps.push(store_preflight_check(
            "post_replacement_surface_checks",
            post_replacement["ok"].as_bool() == Some(true),
            "post-replacement store open, workspace, lane, ticket, maintenance, and VCS checks completed",
        ));
    }

    let report = store_replacement_activation_report(
        active_store,
        candidate_store,
        workspace,
        backup_store,
        dry_run,
        steps,
        preflight,
        post_replacement,
    );
    emit_store_replacement_activation_report(&report, format, report_file)?;
    if report["ok"].as_bool() == Some(true) {
        Ok(())
    } else {
        Err("store replacement activation failed".to_string())
    }
}

fn store_replacement_activation_report(
    active_store: &str,
    candidate_store: &str,
    workspace: &str,
    backup_store: &str,
    dry_run: bool,
    steps: Vec<serde_json::Value>,
    preflight: serde_json::Value,
    post_replacement: serde_json::Value,
) -> serde_json::Value {
    let ok = steps.iter().all(|step| step["ok"].as_bool() == Some(true));
    serde_json::json!({
        "active_store": active_store,
        "candidate_store": candidate_store,
        "workspace": workspace,
        "backup_store": backup_store,
        "dry_run": dry_run,
        "ok": ok,
        "safe_to_keep_replacement": ok,
        "rollback_artifacts": {
            "backup_store": backup_store,
            "candidate_store": candidate_store,
        },
        "preflight": preflight,
        "post_replacement": post_replacement,
        "steps": steps,
    })
}

fn emit_store_replacement_activation_report(
    report: &serde_json::Value,
    format: &str,
    report_file: Option<&str>,
) -> Result<(), String> {
    let json = serde_json::to_string_pretty(report).map_err(|e| e.to_string())?;
    if let Some(path) = report_file {
        std::fs::write(path, &json).map_err(|e| format!("write report file {path}: {e}"))?;
    }
    match format {
        "json" => println!("{json}"),
        "text" => print_store_replacement_activation_text(report),
        other => {
            return Err(format!(
                "unknown replacement output format {other:?} (expected text or json)"
            ));
        }
    }
    Ok(())
}

fn print_store_replacement_activation_text(report: &serde_json::Value) {
    println!("store replacement activation");
    println!(
        "active_store\t{}",
        report["active_store"].as_str().unwrap_or("")
    );
    println!(
        "candidate_store\t{}",
        report["candidate_store"].as_str().unwrap_or("")
    );
    println!("workspace\t{}", report["workspace"].as_str().unwrap_or(""));
    println!(
        "backup_store\t{}",
        report["backup_store"].as_str().unwrap_or("")
    );
    println!(
        "status\t{}",
        if report["ok"].as_bool() == Some(true) {
            "ok"
        } else {
            "blocked"
        }
    );
    if let Some(steps) = report["steps"].as_array() {
        for step in steps {
            println!(
                "{}\t{}\t{}",
                step["name"].as_str().unwrap_or("unknown"),
                if step["ok"].as_bool() == Some(true) {
                    "ok"
                } else {
                    "blocked"
                },
                step["message"].as_str().unwrap_or("")
            );
        }
    }
}

fn store_replacement_temp_path(active_path: &std::path::Path) -> std::path::PathBuf {
    let mut file_name = active_path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("active.loom"));
    file_name.push(format!(".replacement-{}.tmp", std::process::id()));
    active_path.with_file_name(file_name)
}

fn push_store_replacement_freshness_checks(
    checks: &mut Vec<serde_json::Value>,
    store: &str,
    workspace: &str,
    live_store: Option<&str>,
    candidate_report: Option<&str>,
    force_owner_approval: Option<&str>,
    backup_store: Option<&str>,
    keys: &KeyOpts,
) {
    let Some(report_path) = candidate_report else {
        checks.push(store_preflight_check(
            "freshness_watermark",
            true,
            "no candidate report supplied; live rollback freshness was not checked",
        ));
        return;
    };
    let Some(live_store) = live_store else {
        checks.push(store_preflight_check(
            "freshness_watermark",
            false,
            "candidate report requires --live-store to check rollback freshness",
        ));
        return;
    };
    let report = match read_store_copy_report(report_path) {
        Ok(report) => report,
        Err(error) => {
            checks.push(store_preflight_check(
                "freshness_watermark",
                false,
                format!("candidate report is not readable: {error}"),
            ));
            return;
        }
    };
    if report
        .get("destination")
        .and_then(serde_json::Value::as_str)
        != Some(store)
    {
        checks.push(store_preflight_check(
            "freshness_candidate",
            false,
            "candidate report destination does not match the candidate store path",
        ));
    }
    if report.get("source").and_then(serde_json::Value::as_str) != Some(live_store) {
        checks.push(store_preflight_check(
            "freshness_source",
            false,
            "candidate report source does not match --live-store",
        ));
    }
    let Some(watermark) = report.get("freshness_watermark") else {
        checks.push(store_preflight_check(
            "freshness_watermark",
            false,
            "candidate report has no freshness_watermark",
        ));
        return;
    };
    let mut lost = Vec::new();
    let live = match cli_open_loom_read(live_store, keys) {
        Ok(live) => live,
        Err(error) => {
            checks.push(store_preflight_check(
                "freshness_live_open",
                false,
                format!("live store is not readable: {error}"),
            ));
            return;
        }
    };
    compare_watermark_root(
        &mut lost,
        "reference_root",
        watermark.get("source_reference_root"),
        live.store().reference_root(),
    );
    compare_watermark_root(
        &mut lost,
        "control_root",
        watermark.get("source_control_root"),
        live.store().control_root(),
    );
    match resolve_ns(&live, workspace) {
        Ok(workspace_id) => {
            let profile_id = workspace_id.to_string();
            let live_latest =
                ticket_profile_latest_operation(&live, workspace_id, &profile_id).unwrap_or(None);
            let recorded_latest = watermark
                .get("workspaces")
                .and_then(serde_json::Value::as_array)
                .and_then(|workspaces| {
                    workspaces.iter().find(|entry| {
                        entry
                            .get("workspace_id")
                            .and_then(serde_json::Value::as_str)
                            == Some(profile_id.as_str())
                    })
                })
                .and_then(|entry| entry.get("latest_ticket_operation"));
            let recorded_sequence = recorded_latest
                .and_then(|latest| latest.get("sequence"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let recorded_operation_id = recorded_latest
                .and_then(|latest| latest.get("operation_id"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if let Some(live_latest) = live_latest
                && live_latest.sequence > recorded_sequence
            {
                lost.push(format!(
                    "ticket_operation sequence {} operation_id {} is newer than candidate watermark sequence {} operation_id {}",
                    live_latest.sequence,
                    live_latest.operation_id,
                    recorded_sequence,
                    recorded_operation_id
                ));
            }
        }
        Err(error) => lost.push(format!(
            "workspace {workspace:?} no longer resolves: {error}"
        )),
    }
    if lost.is_empty() {
        checks.push(store_preflight_check(
            "freshness_watermark",
            true,
            "candidate freshness watermark matches the live store",
        ));
        return;
    }
    if force_owner_approval.is_some_and(|approval| !approval.trim().is_empty())
        && backup_store.is_some_and(|backup| std::path::Path::new(backup).exists())
    {
        checks.push(store_preflight_check(
            "freshness_watermark",
            true,
            format!(
                "owner-approved force accepted with backup {}; lost_mutations={}",
                backup_store.unwrap_or(""),
                lost.join("; ")
            ),
        ));
    } else {
        checks.push(store_preflight_check(
            "freshness_watermark",
            false,
            format!(
                "candidate is stale relative to live store; lost_mutations={}; rerun copy or provide --force-owner-approval and --backup-store",
                lost.join("; ")
            ),
        ));
    }
}

fn read_store_copy_report(path: &str) -> Result<serde_json::Value, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

fn compare_watermark_root(
    lost: &mut Vec<String>,
    name: &str,
    recorded: Option<&serde_json::Value>,
    live: Option<Digest>,
) {
    let recorded = recorded.and_then(serde_json::Value::as_str).unwrap_or("");
    let live = live.map(|root| root.to_string()).unwrap_or_default();
    if recorded != live {
        lost.push(format!("{name} advanced from {recorded} to {live}"));
    }
}

#[derive(Clone)]
struct TicketOperationWatermark {
    sequence: u64,
    operation_id: String,
}

fn ticket_profile_latest_operation(
    loom: &Loom<FileStore>,
    workspace_id: WorkspaceId,
    profile_id: &str,
) -> Result<Option<TicketOperationWatermark>, String> {
    Ok(loom_tickets::history(loom, workspace_id, profile_id, None)
        .map_err(|e| e.to_string())?
        .into_iter()
        .max_by_key(|record| record.sequence)
        .map(|record| TicketOperationWatermark {
            sequence: record.sequence,
            operation_id: record.operation_id,
        }))
}

fn store_preflight_check(name: &str, ok: bool, message: impl Into<String>) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "ok": ok,
        "message": message.into(),
    })
}

fn store_replacement_root_codec_check(
    diagnostics: &loom_store::StoreRootCodecDiagnostics,
) -> serde_json::Value {
    if diagnostics.failures.is_empty() {
        store_preflight_check(
            "root_codecs",
            true,
            format!("checked={} failures=0", diagnostics.checked_roots),
        )
    } else {
        store_preflight_check(
            "root_codecs",
            false,
            format!(
                "root codec diagnostics failed: checked={} failures={} {}",
                diagnostics.checked_roots,
                diagnostics.failures.len(),
                root_codec_failure_summary(&diagnostics.failures)
            ),
        )
    }
}

fn root_codec_failure_summary(failures: &[loom_store::StoreRootCodecDiagnostic]) -> String {
    failures
        .iter()
        .take(4)
        .map(|failure| {
            format!(
                "{}:page={} expected={} actual={}",
                failure.root_name,
                failure.root_page,
                failure.expected_codec,
                failure
                    .actual_discriminator
                    .map_or_else(|| "none".to_string(), |value| format!("0x{value:02x}"))
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn store_replacement_preflight_report(
    store: &str,
    workspace: &str,
    ok: bool,
    checks: &[serde_json::Value],
    live_store: Option<&str>,
    candidate_report: Option<&str>,
    force_owner_approval: Option<&str>,
    backup_store: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "store": store,
        "workspace": workspace,
        "ok": ok,
        "safe_to_replace": ok,
        "freshness_watermark": store_replacement_freshness_watermark_report(candidate_report),
        "backup_plan": store_replacement_backup_plan_report(force_owner_approval, backup_store),
        "active_store_freshness": store_replacement_active_freshness_report(live_store, candidate_report, checks),
        "legacy_projection_collision_risks": store_replacement_legacy_projection_report(checks),
        "checks": checks,
    })
}

fn store_replacement_freshness_watermark_report(
    candidate_report: Option<&str>,
) -> serde_json::Value {
    match candidate_report {
        Some(path) => match read_store_copy_report(path) {
            Ok(report) => {
                let watermark = report
                    .get("freshness_watermark")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                serde_json::json!({
                    "candidate_report": path,
                    "available": !watermark.is_null(),
                    "watermark": watermark,
                    "read_error": serde_json::Value::Null,
                })
            }
            Err(error) => serde_json::json!({
                "candidate_report": path,
                "available": false,
                "watermark": serde_json::Value::Null,
                "read_error": error,
            }),
        },
        None => serde_json::json!({
            "candidate_report": serde_json::Value::Null,
            "available": false,
            "watermark": serde_json::Value::Null,
            "read_error": serde_json::Value::Null,
        }),
    }
}

fn store_replacement_backup_plan_report(
    force_owner_approval: Option<&str>,
    backup_store: Option<&str>,
) -> serde_json::Value {
    let force_owner_approval_present =
        force_owner_approval.is_some_and(|approval| !approval.trim().is_empty());
    let backup_exists = backup_store.map(|path| std::path::Path::new(path).exists());
    serde_json::json!({
        "backup_store": backup_store,
        "backup_exists": backup_exists,
        "force_owner_approval_present": force_owner_approval_present,
        "stale_candidate_override_ready": force_owner_approval_present && backup_exists.unwrap_or(false),
    })
}

fn store_replacement_active_freshness_report(
    live_store: Option<&str>,
    candidate_report: Option<&str>,
    checks: &[serde_json::Value],
) -> serde_json::Value {
    let freshness_checks = checks
        .iter()
        .filter(|check| {
            check
                .get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|name| name.starts_with("freshness_"))
        })
        .cloned()
        .collect::<Vec<_>>();
    let ok = freshness_checks
        .iter()
        .all(|check| check["ok"].as_bool() == Some(true));
    serde_json::json!({
        "live_store": live_store,
        "checked": live_store.is_some() && candidate_report.is_some(),
        "ok": ok,
        "checks": freshness_checks,
    })
}

fn store_replacement_legacy_projection_report(checks: &[serde_json::Value]) -> serde_json::Value {
    let preflight = checks.iter().find(|check| {
        check.get("name").and_then(serde_json::Value::as_str) == Some("vcs_namespace_preflight")
    });
    match preflight {
        Some(check) => serde_json::json!({
            "checked": true,
            "ok": check["ok"].as_bool().unwrap_or(false),
            "collision_risk": check["ok"].as_bool() != Some(true),
            "message": check["message"].as_str().unwrap_or(""),
        }),
        None => serde_json::json!({
            "checked": false,
            "ok": false,
            "collision_risk": true,
            "message": "VCS namespace preflight did not run",
        }),
    }
}

fn print_store_replacement_preflight_text(report: &serde_json::Value) {
    let store = report["store"].as_str().unwrap_or("");
    let workspace = report["workspace"].as_str().unwrap_or("");
    let ok = report["ok"].as_bool() == Some(true);
    let empty = Vec::new();
    let checks = report["checks"].as_array().unwrap_or(&empty);
    println!("store replacement preflight");
    println!("store\t{store}");
    println!("workspace\t{workspace}");
    println!("status\t{}", if ok { "ok" } else { "blocked" });
    println!(
        "safe_to_replace\t{}",
        if report["safe_to_replace"].as_bool() == Some(true) {
            "yes"
        } else {
            "no"
        }
    );
    println!(
        "freshness_watermark\t{}",
        if report["freshness_watermark"]["available"].as_bool() == Some(true) {
            "available"
        } else {
            "missing"
        }
    );
    println!(
        "backup_plan\tforce_owner_approval_present={} backup_exists={}",
        report["backup_plan"]["force_owner_approval_present"]
            .as_bool()
            .unwrap_or(false),
        report["backup_plan"]["backup_exists"]
            .as_bool()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!(
        "active_store_freshness\t{}",
        if report["active_store_freshness"]["ok"].as_bool() == Some(true) {
            "ok"
        } else {
            "blocked"
        }
    );
    println!(
        "legacy_projection_collision_risks\t{}",
        if report["legacy_projection_collision_risks"]["collision_risk"].as_bool() == Some(true) {
            "present"
        } else {
            "none"
        }
    );
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let chat_workspace_id = client.resolve_workspace_id(&workspace)?.to_string();
            let channels = execute_generated_json::<Vec<loom_chat::HostedChatChannelSummary>>(
                &client,
                "Chat",
                "chat_list_channels_json",
                vec![workspace.to_value(), chat_workspace_id.to_value()],
            )?;
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
            let (client, chat_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let channel_id = match channel_id {
                Some(value) => parse_chat_workspace_id(&value)?,
                None => random_workspace_id()?,
            };
            let channel = execute_generated_json::<loom_chat::HostedChatChannelSummary>(
                &client,
                "Chat",
                "chat_create_channel_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    channel_id.to_string().to_value(),
                    handle.to_value(),
                    name.to_value(),
                    WireValue::Null,
                ],
            )?;
            print_chat_channel_summary(&channel, &format)
        }
        ChatCmd::RenameChannel {
            store,
            workspace,
            channel,
            handle,
            format,
        } => {
            let (client, chat_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let channel = execute_generated_json::<loom_chat::HostedChatChannelSummary>(
                &client,
                "Chat",
                "chat_rename_channel_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    channel.to_value(),
                    handle.to_value(),
                    WireValue::Null,
                ],
            )?;
            print_chat_channel_summary(&channel, &format)
        }
        ChatCmd::Messages {
            store,
            workspace,
            channel,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let chat_workspace_id = client.resolve_workspace_id(&workspace)?.to_string();
            let projection = execute_generated_json::<loom_chat::HostedChatChannel>(
                &client,
                "Chat",
                "chat_messages_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    channel.to_value(),
                ],
            )?;
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
            let max = u64::try_from(max)
                .map_err(|_| "chat event max exceeds protocol u64 range".to_string())?;
            let from_sequence = from_sequence.max(1);
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let chat_workspace_id = client.resolve_workspace_id(&workspace)?.to_string();
            let events =
                execute_generated_json::<loom_substrate::changes::HostedOperationChangesBatch>(
                    &client,
                    "Chat",
                    "chat_fetch_events_json",
                    vec![
                        workspace.to_value(),
                        chat_workspace_id.to_value(),
                        channel.to_value(),
                        from_sequence.to_value(),
                        max.to_value(),
                    ],
                )?;
            print_chat_events(&events, &format)
        }
        ChatCmd::Cursor {
            store,
            workspace,
            channel,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let chat_workspace_id = client.resolve_workspace_id(&workspace)?.to_string();
            let cursor = execute_generated_json::<loom_chat::HostedChatCursor>(
                &client,
                "Chat",
                "chat_cursor_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    channel.to_value(),
                ],
            )?;
            print_chat_cursor(&cursor, &format)
        }
        ChatCmd::UpdateCursor {
            store,
            workspace,
            channel,
            next_sequence,
            format,
        } => {
            let (client, chat_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let cursor = execute_generated_json::<loom_chat::HostedChatCursor>(
                &client,
                "Chat",
                "chat_update_cursor_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    channel.to_value(),
                    next_sequence.to_value(),
                    WireValue::Null,
                ],
            )?;
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
            let (client, chat_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_chat::HostedChatWrite>(
                &client,
                "Chat",
                "chat_post_message_bytes_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    channel.to_value(),
                    message_id.to_value(),
                    thread.to_value(),
                    WireValue::Bytes(body),
                    WireValue::Null,
                ],
            )?;
            print_chat_write(&write, &format)
        }
        ChatCmd::Edit {
            store,
            workspace,
            channel,
            message_id,
            input,
            expected_entity_tag,
            format,
        } => {
            let body = read_input(&input).map_err(|e| e.to_string())?;
            let (client, chat_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_chat::HostedChatWrite>(
                &client,
                "Chat",
                "chat_edit_message_bytes_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    channel.to_value(),
                    message_id.to_value(),
                    WireValue::Bytes(body),
                    expected_entity_tag.to_value(),
                ],
            )?;
            print_chat_write(&write, &format)
        }
        ChatCmd::Redact {
            store,
            workspace,
            channel,
            message_id,
            reason,
            format,
        } => {
            let (client, chat_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_chat::HostedChatWrite>(
                &client,
                "Chat",
                "chat_redact_message_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    channel.to_value(),
                    message_id.to_value(),
                    reason.to_value(),
                    WireValue::Null,
                ],
            )?;
            print_chat_write(&write, &format)
        }
        ChatCmd::CreateThread {
            store,
            workspace,
            channel,
            thread_id,
            parent_message_id,
            format,
        } => {
            let (client, chat_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_chat::HostedChatWrite>(
                &client,
                "Chat",
                "chat_create_thread_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    channel.to_value(),
                    thread_id.to_value(),
                    parent_message_id.to_value(),
                    WireValue::Null,
                ],
            )?;
            print_chat_write(&write, &format)
        }
        ChatCmd::CreateTask {
            store,
            workspace,
            channel,
            task_id,
            title,
            message_id,
            format,
        } => {
            let (client, chat_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_chat::HostedChatWrite>(
                &client,
                "Chat",
                "chat_create_task_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    channel.to_value(),
                    task_id.to_value(),
                    message_id.to_value(),
                    title.to_value(),
                    WireValue::Null,
                ],
            )?;
            print_chat_write(&write, &format)
        }
        ChatCmd::ClaimTask {
            store,
            workspace,
            channel,
            task_id,
            claim_id,
            lease_token,
            format,
        } => {
            let (client, chat_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_chat::HostedChatWrite>(
                &client,
                "Chat",
                "chat_claim_task_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    channel.to_value(),
                    task_id.to_value(),
                    claim_id.to_value(),
                    lease_token.to_value(),
                    WireValue::Null,
                ],
            )?;
            print_chat_write(&write, &format)
        }
        ChatCmd::CompleteTask {
            store,
            workspace,
            channel,
            task_id,
            claim_id,
            result_message_id,
            format,
        } => {
            let (client, chat_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_chat::HostedChatWrite>(
                &client,
                "Chat",
                "chat_complete_task_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    channel.to_value(),
                    task_id.to_value(),
                    claim_id.to_value(),
                    result_message_id.to_value(),
                    WireValue::Null,
                ],
            )?;
            print_chat_write(&write, &format)
        }
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
            let agent_principal = parse_chat_workspace_id(&agent_principal)?.to_string();
            let source_message_ids_json =
                serde_json::to_string(&source_message_ids).map_err(|e| e.to_string())?;
            let (client, chat_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_chat::HostedChatWrite>(
                &client,
                "Chat",
                "chat_invoke_agent_bytes_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    channel.to_value(),
                    invocation_id.to_value(),
                    agent_principal.to_value(),
                    source_message_ids_json.to_value(),
                    WireValue::Bytes(prompt),
                    WireValue::Null,
                ],
            )?;
            print_chat_write(&write, &format)
        }
        ChatCmd::AgentReply {
            store,
            workspace,
            channel,
            invocation_id,
            message_id,
            format,
        } => {
            let (client, chat_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_chat::HostedChatWrite>(
                &client,
                "Chat",
                "chat_agent_reply_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    channel.to_value(),
                    invocation_id.to_value(),
                    message_id.to_value(),
                    WireValue::Null,
                ],
            )?;
            print_chat_write(&write, &format)
        }
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
            let from_agent_principal = parse_chat_workspace_id(&from_agent_principal)?.to_string();
            let to_principal = to_principal
                .as_deref()
                .map(parse_chat_workspace_id)
                .transpose()?
                .map(|id| id.to_string());
            let (client, chat_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_chat::HostedChatWrite>(
                &client,
                "Chat",
                "chat_request_handoff_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    channel.to_value(),
                    handoff_id.to_value(),
                    from_agent_principal.to_value(),
                    to_principal.to_value(),
                    reason.to_value(),
                    WireValue::Null,
                ],
            )?;
            print_chat_write(&write, &format)
        }
        ChatCmd::AddReaction {
            store,
            workspace,
            channel,
            message_id,
            kind,
            format,
        } => {
            let (client, chat_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_chat::HostedChatWrite>(
                &client,
                "Chat",
                "chat_add_reaction_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    channel.to_value(),
                    message_id.to_value(),
                    kind.to_value(),
                    WireValue::Null,
                ],
            )?;
            print_chat_write(&write, &format)
        }
        ChatCmd::RemoveReaction {
            store,
            workspace,
            channel,
            message_id,
            kind,
            format,
        } => {
            let (client, chat_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_chat::HostedChatWrite>(
                &client,
                "Chat",
                "chat_remove_reaction_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    channel.to_value(),
                    message_id.to_value(),
                    kind.to_value(),
                    WireValue::Null,
                ],
            )?;
            print_chat_write(&write, &format)
        }
        ChatCmd::EmojiList {
            store,
            workspace,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let chat_workspace_id = client.resolve_workspace_id(&workspace)?.to_string();
            let registry = execute_generated_json::<loom_chat::HostedChatEmojiRegistry>(
                &client,
                "Chat",
                "chat_emoji_list_json",
                vec![workspace.to_value(), chat_workspace_id.to_value()],
            )?;
            print_chat_emoji_registry(&registry, &format)
        }
        ChatCmd::EmojiRegister {
            store,
            workspace,
            kind,
            format,
        } => {
            let (client, chat_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let registry = execute_generated_json::<loom_chat::HostedChatEmojiRegistry>(
                &client,
                "Chat",
                "chat_emoji_register_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    kind.to_value(),
                    WireValue::Null,
                ],
            )?;
            print_chat_emoji_registry(&registry, &format)
        }
        ChatCmd::EmojiUnregister {
            store,
            workspace,
            kind,
            format,
        } => {
            let (client, chat_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let registry = execute_generated_json::<loom_chat::HostedChatEmojiRegistry>(
                &client,
                "Chat",
                "chat_emoji_unregister_json",
                vec![
                    workspace.to_value(),
                    chat_workspace_id.to_value(),
                    kind.to_value(),
                    WireValue::Null,
                ],
            )?;
            print_chat_emoji_registry(&registry, &format)
        }
    }
}

fn generated_workspace_context(
    store: &str,
    workspace: &str,
    keys: &KeyOpts,
) -> Result<(remote::CliGeneratedClient, String), String> {
    let client = remote::open_cli_generated_client(store, keys)?;
    let workspace_id = client.resolve_workspace_id(workspace)?.to_string();
    Ok((client, workspace_id))
}

fn execute_generated_json<T>(
    client: &remote::CliGeneratedClient,
    interface: &str,
    method: &str,
    args: Vec<WireValue>,
) -> Result<T, String>
where
    T: serde::de::DeserializeOwned,
{
    let json = execute_generated_string(client, interface, method, args)?;
    serde_json::from_str(&json).map_err(|e| e.to_string())
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
    batch: &loom_substrate::changes::HostedOperationChangesBatch,
    format: &str,
) -> Result<(), String> {
    match format {
        "json" => {
            let events = batch
                .events
                .iter()
                .map(|event| {
                    let loom_substrate::changes::HostedOperationChangeEvent::Operation {
                        workspace_id,
                        app_id,
                        scope_id,
                        operation_id,
                        operation_kind,
                        sequence,
                        actor_principal,
                        timestamp_ms,
                        root_after,
                        payload_digest,
                        policy_labels,
                    } = event;
                    serde_json::json!({
                        "workspace_id": workspace_id,
                        "app_id": app_id,
                        "scope_id": scope_id,
                        "operation_id": operation_id,
                        "operation_kind": operation_kind,
                        "sequence": sequence,
                        "actor_principal": actor_principal,
                        "timestamp_ms": timestamp_ms,
                        "root_after": root_after,
                        "payload_digest": payload_digest,
                        "policy_labels": policy_labels
                    })
                })
                .collect::<Vec<_>>();
            let body = serde_json::json!({
                "events": events,
                "next": batch.next
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "text" => {
            for event in &batch.events {
                let loom_substrate::changes::HostedOperationChangeEvent::Operation {
                    operation_id,
                    operation_kind,
                    sequence,
                    root_after,
                    ..
                } = event;
                println!(
                    "{}\t{}\t{}\t{}",
                    sequence, operation_id, operation_kind, root_after
                );
            }
            println!("next\t{}", batch.next);
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

fn run_drive(action: DriveCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        DriveCmd::List {
            store,
            workspace,
            folder_id,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let drive_workspace_id = client.resolve_workspace_id(&workspace)?.to_string();
            let folder = execute_generated_json::<loom_drive::HostedDriveFolder>(
                &client,
                "Drive",
                "drive_list_json",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    folder_id.to_value(),
                ],
            )?;
            print_drive_folder(&folder, &format)
        }
        DriveCmd::Stat {
            store,
            workspace,
            folder_id,
            name,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let drive_workspace_id = client.resolve_workspace_id(&workspace)?.to_string();
            let stat = execute_generated_json::<loom_drive::HostedDriveStat>(
                &client,
                "Drive",
                "drive_stat_json",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    folder_id.to_value(),
                    name.to_value(),
                ],
            )?;
            print_drive_stat(&stat, &format)
        }
        DriveCmd::Read {
            store,
            workspace,
            file_id,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let drive_workspace_id = client.resolve_workspace_id(&workspace)?.to_string();
            let bytes = execute_generated_bytes(
                &client,
                "Drive",
                "drive_read_file",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    file_id.to_value(),
                ],
            )?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        DriveCmd::ListVersions {
            store,
            workspace,
            file_id,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let drive_workspace_id = client.resolve_workspace_id(&workspace)?.to_string();
            let versions = execute_generated_json::<Vec<loom_drive::HostedDriveVersion>>(
                &client,
                "Drive",
                "drive_list_versions_json",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    file_id.to_value(),
                ],
            )?;
            print_drive_versions(&versions, &format)
        }
        DriveCmd::ListConflicts {
            store,
            workspace,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let drive_workspace_id = client.resolve_workspace_id(&workspace)?.to_string();
            let conflicts = execute_generated_json::<Vec<loom_drive::HostedDriveConflict>>(
                &client,
                "Drive",
                "drive_list_conflicts_json",
                vec![workspace.to_value(), drive_workspace_id.to_value()],
            )?;
            print_drive_conflicts(&conflicts, &format)
        }
        DriveCmd::ListShares {
            store,
            workspace,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let drive_workspace_id = client.resolve_workspace_id(&workspace)?.to_string();
            let shares = execute_generated_json::<Vec<loom_drive::HostedDriveShareGrant>>(
                &client,
                "Drive",
                "drive_list_shares_json",
                vec![workspace.to_value(), drive_workspace_id.to_value()],
            )?;
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
            let (client, drive_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_drive::HostedDriveWrite>(
                &client,
                "Drive",
                "drive_grant_share_json",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    grant_id.to_value(),
                    target_kind.to_value(),
                    target_id.to_value(),
                    principal.to_value(),
                    role.to_value(),
                    granted_at_ms.to_value(),
                    expires_at_ms.to_value(),
                ],
            )?;
            print_drive_write(&write, &format)
        }
        DriveCmd::RevokeShare {
            store,
            workspace,
            grant_id,
            format,
        } => {
            let (client, drive_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_drive::HostedDriveWrite>(
                &client,
                "Drive",
                "drive_revoke_share_json",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    grant_id.to_value(),
                ],
            )?;
            print_drive_write(&write, &format)
        }
        DriveCmd::ApplyShareExpiry {
            store,
            workspace,
            now_ms,
            format,
        } => {
            let (client, drive_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let applied = execute_generated_json::<loom_drive::HostedDriveShareExpiryApply>(
                &client,
                "Drive",
                "drive_apply_share_expiry_json",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    now_ms.to_value(),
                ],
            )?;
            print_drive_share_expiry_apply(&applied, &format)
        }
        DriveCmd::ListRetention {
            store,
            workspace,
            format,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let drive_workspace_id = client.resolve_workspace_id(&workspace)?.to_string();
            let pins = execute_generated_json::<Vec<loom_drive::HostedDriveRetentionPin>>(
                &client,
                "Drive",
                "drive_list_retention_json",
                vec![workspace.to_value(), drive_workspace_id.to_value()],
            )?;
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
            let (client, drive_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_drive::HostedDriveWrite>(
                &client,
                "Drive",
                "drive_pin_retention_json",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    pin_id.to_value(),
                    kind.to_value(),
                    root.to_value(),
                    target_entity_id.to_value(),
                    added_at_ms.to_value(),
                    expires_at_ms.to_value(),
                ],
            )?;
            print_drive_write(&write, &format)
        }
        DriveCmd::UnpinRetention {
            store,
            workspace,
            pin_id,
            format,
        } => {
            let (client, drive_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_drive::HostedDriveWrite>(
                &client,
                "Drive",
                "drive_unpin_retention_json",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    pin_id.to_value(),
                ],
            )?;
            print_drive_write(&write, &format)
        }
        DriveCmd::ApplyRetention {
            store,
            workspace,
            now_ms,
            format,
        } => {
            let (client, drive_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let applied = execute_generated_json::<loom_drive::HostedDriveRetentionApply>(
                &client,
                "Drive",
                "drive_apply_retention_json",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    now_ms.to_value(),
                ],
            )?;
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
            let (client, drive_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_drive::HostedDriveWrite>(
                &client,
                "Drive",
                "drive_create_folder_json",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    parent_folder_id.to_value(),
                    folder_id.to_value(),
                    name.to_value(),
                    expected_root.to_value(),
                ],
            )?;
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
            let (client, drive_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let upload = execute_generated_json::<loom_drive::HostedDriveUploadSession>(
                &client,
                "Drive",
                "drive_create_upload_json",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    upload_id.to_value(),
                    parent_folder_id.to_value(),
                    name.to_value(),
                    file_id.to_value(),
                    expected_root.to_value(),
                    created_at_ms.to_value(),
                    replace_file.to_value(),
                ],
            )?;
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
            let (client, drive_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let upload = execute_generated_json::<loom_drive::HostedDriveUploadSession>(
                &client,
                "Drive",
                "drive_upload_chunk_json",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    upload_id.to_value(),
                    WireValue::Bytes(bytes),
                ],
            )?;
            print_drive_upload(&upload, &format)
        }
        DriveCmd::CommitUpload {
            store,
            workspace,
            upload_id,
            format,
        } => {
            let (client, drive_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_drive::HostedDriveWrite>(
                &client,
                "Drive",
                "drive_commit_upload_json",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    upload_id.to_value(),
                ],
            )?;
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
            let (client, drive_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_drive::HostedDriveWrite>(
                &client,
                "Drive",
                "drive_rename_json",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    folder_id.to_value(),
                    node_id.to_value(),
                    new_name.to_value(),
                    expected_root.to_value(),
                ],
            )?;
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
            let (client, drive_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_drive::HostedDriveWrite>(
                &client,
                "Drive",
                "drive_move_json",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    source_folder_id.to_value(),
                    target_folder_id.to_value(),
                    node_id.to_value(),
                    expected_root.to_value(),
                ],
            )?;
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
            let (client, drive_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_drive::HostedDriveWrite>(
                &client,
                "Drive",
                "drive_delete_json",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    folder_id.to_value(),
                    node_id.to_value(),
                    expected_root.to_value(),
                ],
            )?;
            print_drive_write(&write, &format)
        }
        DriveCmd::ResolveConflict {
            store,
            workspace,
            conflict_id,
            resolution,
            format,
        } => {
            let _resolution = parse_drive_conflict_resolution(&resolution)?;
            let (client, drive_workspace_id) =
                generated_workspace_context(&store, &workspace, keys)?;
            let write = execute_generated_json::<loom_drive::HostedDriveWrite>(
                &client,
                "Drive",
                "drive_resolve_conflict_json",
                vec![
                    workspace.to_value(),
                    drive_workspace_id.to_value(),
                    conflict_id.to_value(),
                    resolution.to_value(),
                ],
            )?;
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let stat_bytes = execute_generated_bytes(
                &client,
                "FileSystem",
                "stat",
                vec![workspace.to_value(), path.to_value()],
            )?;
            let stat = loom_wire::fs::fs_stat_from_cbor(&stat_bytes).map_err(|e| e.to_string())?;
            match stat.kind {
                FileKind::Directory => execute_generated_void(
                    &client,
                    "FileSystem",
                    "remove_directory",
                    vec![workspace.to_value(), path.to_value(), recursive.to_value()],
                ),
                FileKind::File | FileKind::Symlink => execute_generated_void(
                    &client,
                    "FileSystem",
                    "remove_file",
                    vec![workspace.to_value(), path.to_value()],
                ),
            }
        }
        FilesCmd::Ls { store, workspace } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            for p in generated_files_list(&client, &workspace)? {
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "FileSystem",
                "create_directory",
                vec![workspace.to_value(), path.to_value(), parents.to_value()],
            )
        }
        FilesCmd::Read {
            store,
            workspace,
            path,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let bytes = execute_generated_bytes(
                &client,
                "FileSystem",
                "read_file",
                vec![workspace.to_value(), path.to_value()],
            )?;
            write_output(out.as_deref(), &bytes).map_err(|e| e.to_string())
        }
        FilesCmd::Write {
            store,
            workspace,
            path,
            input,
        } => {
            let bytes = read_input(&input).map_err(|e| e.to_string())?;
            let client = remote::open_cli_generated_client(&store, keys)?;
            if let Some((parent, _)) = path.rsplit_once('/') {
                execute_generated_void(
                    &client,
                    "FileSystem",
                    "create_directory",
                    vec![workspace.to_value(), parent.to_value(), true.to_value()],
                )?;
            }
            execute_generated_void(
                &client,
                "FileSystem",
                "write_file",
                vec![
                    workspace.to_value(),
                    path.to_value(),
                    WireValue::Bytes(bytes),
                    0o100644u64.to_value(),
                ],
            )
        }
    }
}

fn generated_files_list(
    client: &remote::CliGeneratedClient,
    workspace: &str,
) -> Result<Vec<String>, String> {
    fn walk(
        client: &remote::CliGeneratedClient,
        workspace: &str,
        dir: &str,
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        let bytes = execute_generated_bytes(
            client,
            "FileSystem",
            "list_directory",
            vec![workspace.to_value(), dir.to_value()],
        )?;
        let entries = loom_wire::fs::dir_listing_from_cbor(&bytes).map_err(|e| e.to_string())?;
        for entry in entries {
            let child = if dir.is_empty() {
                entry.name
            } else {
                format!("{dir}/{}", entry.name)
            };
            match entry.kind {
                FileKind::Directory => walk(client, workspace, &child, out)?,
                FileKind::File | FileKind::Symlink => out.push(child),
            }
        }
        Ok(())
    }

    let mut out = Vec::new();
    walk(client, workspace, "", &mut out)?;
    out.sort();
    Ok(out)
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
    let payload =
        std::fs::read(snapshot).map_err(|e| format!("read Redmine import {snapshot}: {e}"))?;
    let client = remote::open_cli_generated_client(store, keys)?;
    let encoded = execute_generated_bytes(
        &client,
        "InterchangeProfiles",
        "import_redmine",
        vec![
            workspace.to_value(),
            profile.to_value(),
            snapshot.to_value(),
            WireValue::Bytes(payload),
            field_policy.to_value(),
            dry_run.to_value(),
        ],
    )?;
    let report = generated_import_report_from_cbor(&encoded)?;
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
    let payload =
        std::fs::read(snapshot).map_err(|e| format!("read Asana import {snapshot}: {e}"))?;
    let client = remote::open_cli_generated_client(store, keys)?;
    let encoded = execute_generated_bytes(
        &client,
        "InterchangeProfiles",
        "import_asana",
        vec![
            workspace.to_value(),
            profile.to_value(),
            snapshot.to_value(),
            WireValue::Bytes(payload),
            field_policy.to_value(),
            dry_run.to_value(),
        ],
    )?;
    let report = generated_import_report_from_cbor(&encoded)?;
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
    let payload =
        std::fs::read(snapshot).map_err(|e| format!("read Confluence import {snapshot}: {e}"))?;
    let client = remote::open_cli_generated_client(store, keys)?;
    let encoded = execute_generated_bytes(
        &client,
        "InterchangeProfiles",
        "import_confluence",
        vec![
            workspace.to_value(),
            profile.to_value(),
            snapshot.to_value(),
            WireValue::Bytes(payload),
            default_space.to_value(),
            dry_run.to_value(),
        ],
    )?;
    let report = generated_import_report_from_cbor(&encoded)?;
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
    let payload =
        std::fs::read(snapshot).map_err(|e| format!("read Slack import {snapshot}: {e}"))?;
    let client = remote::open_cli_generated_client(store, keys)?;
    let encoded = execute_generated_bytes(
        &client,
        "InterchangeProfiles",
        "import_slack",
        vec![
            workspace.to_value(),
            profile.to_value(),
            snapshot.to_value(),
            WireValue::Bytes(payload),
            dry_run.to_value(),
        ],
    )?;
    let report = generated_import_report_from_cbor(&encoded)?;
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
    let payload =
        std::fs::read(snapshot).map_err(|e| format!("read Drive import {snapshot}: {e}"))?;
    let client = remote::open_cli_generated_client(store, keys)?;
    let encoded = execute_generated_bytes(
        &client,
        "InterchangeProfiles",
        "import_drive",
        vec![
            workspace.to_value(),
            profile.to_value(),
            snapshot.to_value(),
            WireValue::Bytes(payload),
            dry_run.to_value(),
        ],
    )?;
    let report = generated_import_report_from_cbor(&encoded)?;
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
    let payload =
        std::fs::read(snapshot).map_err(|e| format!("read Jira import {snapshot}: {e}"))?;
    let client = remote::open_cli_generated_client(store, keys)?;
    let encoded = execute_generated_bytes(
        &client,
        "InterchangeProfiles",
        "import_jira",
        vec![
            workspace.to_value(),
            profile.to_value(),
            snapshot.to_value(),
            WireValue::Bytes(payload),
            field_policy.to_value(),
            dry_run.to_value(),
        ],
    )?;
    let report = generated_import_report_from_cbor(&encoded)?;
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
    let payload = markdown_import_archive(src)?;
    let client = remote::open_cli_generated_client(store, keys)?;
    let encoded = execute_generated_bytes(
        &client,
        "InterchangeProfiles",
        "import_markdown",
        vec![
            workspace.to_value(),
            profile.to_value(),
            src.to_value(),
            WireValue::Bytes(payload),
            space.to_value(),
            dry_run.to_value(),
        ],
    )?;
    let report = generated_import_report_from_cbor(&encoded)?;
    print_import_report(&report, format)
}

fn markdown_import_archive(src: &str) -> Result<Vec<u8>, String> {
    let root = PathBuf::from(src);
    if !root.is_dir() {
        return Err(format!("Markdown import source {src} must be a directory"));
    }
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    write_markdown_archive_dir(&root, &root, &mut archive, options)?;
    archive
        .finish()
        .map_err(|e| format!("finish Markdown import archive: {e}"))
        .map(std::io::Cursor::into_inner)
}

fn write_markdown_archive_dir(
    root: &std::path::Path,
    current: &std::path::Path,
    archive: &mut zip::ZipWriter<std::io::Cursor<Vec<u8>>>,
    options: zip::write::SimpleFileOptions,
) -> Result<(), String> {
    let mut entries = std::fs::read_dir(current)
        .map_err(|e| format!("read Markdown import directory {}: {e}", current.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|e| format!("stat Markdown import path {}: {e}", path.display()))?;
        let relative = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if metadata.is_dir() {
            archive
                .add_directory(format!("{relative}/"), options)
                .map_err(|e| format!("add Markdown archive directory {relative}: {e}"))?;
            write_markdown_archive_dir(root, &path, archive, options)?;
        } else if metadata.is_file() {
            archive
                .start_file(&relative, options)
                .map_err(|e| format!("add Markdown archive file {relative}: {e}"))?;
            let bytes = std::fs::read(&path)
                .map_err(|e| format!("read Markdown import file {}: {e}", path.display()))?;
            archive
                .write_all(&bytes)
                .map_err(|e| format!("write Markdown archive file {relative}: {e}"))?;
        }
    }
    Ok(())
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
    let payload =
        std::fs::read(snapshot).map_err(|e| format!("read Notion import {snapshot}: {e}"))?;
    let client = remote::open_cli_generated_client(store, keys)?;
    let encoded = execute_generated_bytes(
        &client,
        "InterchangeProfiles",
        "import_notion",
        vec![
            workspace.to_value(),
            profile.to_value(),
            snapshot.to_value(),
            WireValue::Bytes(payload),
            default_space.to_value(),
            dry_run.to_value(),
        ],
    )?;
    let report = generated_import_report_from_cbor(&encoded)?;
    print_import_report(&report, format)
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
                return Err("`import fs` to/from a remote store is local-only because it sends a host filesystem path; remote filesystem-tree import requires a byte-transfer contract".to_string());
            }
            let client = remote::open_cli_generated_client_for_dry_run(&store, keys, dry_run)?;
            let encoded = execute_generated_bytes(
                &client,
                "FileSystem",
                "import_fs",
                vec![
                    workspace.to_value(),
                    src.to_value(),
                    Some(author).to_value(),
                    Some(message).to_value(),
                    commit.to_value(),
                    dry_run.to_value(),
                ],
            )?;
            let report = generated_import_report_from_cbor(&encoded)?;
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
            if remote::target_is_remote(&store)? {
                return Err("`import archive` to/from a remote store is local-only because it sends a host archive path; remote archive import requires a byte-transfer contract".to_string());
            }
            let client = remote::open_cli_generated_client_for_dry_run(&store, keys, dry_run)?;
            let encoded = execute_generated_bytes(
                &client,
                "Archive",
                "archive_import",
                vec![
                    workspace.to_value(),
                    archive.to_value(),
                    kind.to_value(),
                    gzip_output_path.to_value(),
                    commit.to_value(),
                    Some(author).to_value(),
                    Some(message).to_value(),
                    dry_run.to_value(),
                ],
            )?;
            let result = generated_archive_import_result_from_cbor(&encoded)?;
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
            let payload =
                std::fs::read(&csv).map_err(|e| format!("read table CSV import {csv}: {e}"))?;
            let client = remote::open_cli_generated_client_for_dry_run(&store, keys, dry_run)?;
            let encoded = execute_generated_bytes(
                &client,
                "InterchangeProfiles",
                "import_table_csv",
                vec![
                    workspace.to_value(),
                    csv.to_value(),
                    WireValue::Bytes(payload),
                    database.to_value(),
                    table.to_value(),
                    schema.to_value(),
                    primary_key.to_value(),
                    mode.to_value(),
                    commit.to_value(),
                    Some(author).to_value(),
                    Some(message).to_value(),
                    dry_run.to_value(),
                ],
            )?;
            let report = generated_import_report_from_cbor(&encoded)?;
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
            let client = remote::open_cli_generated_client_for_dry_run(&store, keys, dry_run)?;
            let encoded = execute_generated_bytes(
                &client,
                "Car",
                "car_import",
                vec![src.to_value(), dry_run.to_value()],
            )?;
            let result = generated_car_import_result_from_cbor(&encoded)?;
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

#[cfg(all(test, feature = "integration-tests"))]
fn parse_table_csv_import_mode(mode: &str) -> Result<TableImportMode, String> {
    match mode {
        "snapshot" => Ok(TableImportMode::Snapshot),
        "append-only" => Ok(TableImportMode::AppendOnly),
        other => Err(format!(
            "unsupported table CSV import mode {other:?}; expected snapshot or append-only"
        )),
    }
}

#[cfg(all(test, feature = "integration-tests"))]
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

#[cfg(all(test, feature = "integration-tests"))]
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

#[cfg(all(test, feature = "integration-tests"))]
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
    freshness_watermark: serde_json::Value,
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
    freshness_watermark: serde_json::Value,
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
            freshness_watermark: input.freshness_watermark,
        }
    }
}

fn store_copy_freshness_watermark(source: &Loom<FileStore>) -> serde_json::Value {
    let workspaces = source
        .registry()
        .list(None)
        .into_iter()
        .map(|info| {
            let workspace_id = info.id.to_string();
            let latest = ticket_profile_latest_operation(source, info.id, &workspace_id)
                .ok()
                .flatten()
                .map(|operation| {
                    serde_json::json!({
                        "sequence": operation.sequence,
                        "operation_id": operation.operation_id,
                    })
                });
            serde_json::json!({
                "workspace_id": workspace_id,
                "workspace_name": info.name,
                "latest_ticket_operation": latest,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "created_at_ms": now_ms(),
        "source_reference_root": source.store().reference_root().map(|root| root.to_string()).unwrap_or_default(),
        "source_control_root": source.store().control_root().map(|root| root.to_string()).unwrap_or_default(),
        "workspaces": workspaces,
    })
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
    out.push_str(",\"freshness_watermark\":");
    out.push_str(&report.freshness_watermark.to_string());
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
    out.push_str(",\"default_durability\":");
    out.push_str(&json_string(policy.default_durability.as_str()));
    out.push_str(",\"facet_durability_overrides\":{");
    let mut first = true;
    for facet in FacetKind::ALL {
        if let Some(policy) = policy.facet_durability_overrides[facet.stable_tag() as usize] {
            if !first {
                out.push(',');
            }
            first = false;
            out.push_str(&json_string(facet.as_str()));
            out.push(':');
            out.push_str(&json_string(policy.as_str()));
        }
    }
    out.push('}');
    out.push_str(",\"audit_seq\":");
    push_json_u64(&mut out, audit_seq);
    out.push('}');
    out
}

fn store_policy_result_json(result: loom_wire::store_admin::StorePolicyResult) -> String {
    let mut policy = StorePolicy {
        fips_required: result.fips_required,
        default_durability: result.default_durability,
        ..StorePolicy::default()
    };
    for assignment in result.facet_durability_overrides {
        policy.facet_durability_overrides[assignment.facet.stable_tag() as usize] =
            Some(assignment.durability);
    }
    store_policy_json(policy, result.audit_seq)
}

fn store_policy_update_from_cli(
    fips_required: Option<bool>,
    default_durability: Option<&str>,
    facet_durability: Vec<String>,
    clear_facet_durability: Vec<String>,
) -> Result<loom_wire::store_admin::StorePolicyUpdate, String> {
    let default_durability = default_durability
        .map(loom_store::parse_store_durability_policy)
        .transpose()
        .map_err(|e| e.to_string())?;
    let mut facet_durability_assignments = Vec::new();
    for assignment in facet_durability {
        let (facet, durability) = assignment.split_once('=').ok_or_else(|| {
            format!("facet durability override {assignment:?} must use <facet>=<policy>")
        })?;
        facet_durability_assignments.push(loom_wire::store_admin::StoreFacetDurabilityAssignment {
            facet: FacetKind::parse(facet).map_err(|e| e.to_string())?,
            durability: loom_store::parse_store_durability_policy(durability)
                .map_err(|e| e.to_string())?,
        });
    }
    let clear_facet_durability = clear_facet_durability
        .into_iter()
        .map(|facet| FacetKind::parse(&facet).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(loom_wire::store_admin::StorePolicyUpdate {
        fips_required,
        default_durability,
        facet_durability_assignments,
        clear_facet_durability,
    })
}

fn store_rekey_credential_from_key_spec(
    spec: KeySpec,
) -> loom_wire::store_admin::StoreRekeyCredential {
    match spec {
        KeySpec::Passphrase(passphrase) => {
            loom_wire::store_admin::StoreRekeyCredential::Passphrase(passphrase.as_bytes().to_vec())
        }
        KeySpec::RawKek(kek) => loom_wire::store_admin::StoreRekeyCredential::RawKek(*kek),
    }
}

fn store_rekey_result_summary(
    target: remote::CliExecutionTarget,
    store: &str,
    result: loom_wire::store_admin::StoreRekeyResult,
) -> String {
    if target == remote::CliExecutionTarget::Remote {
        let bytes = match (result.bytes_before, result.bytes_after) {
            (Some(before), Some(after)) => format!(" ({before} -> {after} bytes)"),
            _ => String::new(),
        };
        return format!(
            "rekeyed remote store (resealed={}, suite={}, audit_seq={}){}",
            result.resealed, result.suite, result.audit_seq, bytes
        );
    }
    match (result.resealed, result.bytes_before, result.bytes_after) {
        (true, Some(before), Some(after)) => format!(
            "rekeyed {store} (re-sealed every object under a fresh DEK, suite {}; {before} -> {after} bytes)",
            result.suite
        ),
        _ => format!("rekeyed {store} (DEK re-wrapped under the new credential)"),
    }
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "VersionControl",
                "branch",
                vec![workspace.to_value(), branch.to_value()],
            )
        }
        VcsCmd::Commit {
            store,
            workspace,
            message,
            author,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let commit = execute_generated_digest_string(
                &client,
                "VersionControl",
                "commit",
                vec![
                    workspace.to_value(),
                    author.to_value(),
                    message.to_value(),
                    current_time_ms()?.to_value(),
                ],
            )?;
            println!("{commit}");
            Ok(())
        }
        VcsCmd::Checkout {
            store,
            workspace,
            branch,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            execute_generated_void(
                &client,
                "VersionControl",
                "checkout",
                vec![workspace.to_value(), branch.to_value()],
            )
        }
        VcsCmd::Diff {
            store,
            workspace,
            from,
            to,
            format,
            out,
        } => {
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let bytes = execute_generated_bytes(
                &client,
                "VersionControl",
                "diff",
                vec![workspace.to_value(), from.to_value(), to.to_value()],
            )?;
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
            let client = remote::open_cli_read_only_generated_client(&store, keys)?;
            let branch = execute_generated_string(
                &client,
                "VersionControl",
                "head_branch",
                vec![workspace.to_value()],
            )?;
            for commit in execute_generated_digest_list(
                &client,
                "VersionControl",
                "log",
                vec![workspace.to_value(), branch.to_value()],
            )? {
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
            let client = remote::open_cli_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "VersionControl",
                "merge",
                vec![
                    workspace.to_value(),
                    from.to_value(),
                    author.to_value(),
                    cells.to_value(),
                    current_time_ms()?.to_value(),
                ],
            )?;
            let outcome =
                loom_wire::vcs::merge_result_from_cbor(&encoded).map_err(|e| e.to_string())?;
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

fn print_sql_exec_result_cbor(bytes: &[u8]) -> Result<(), String> {
    let payload = loom_result::result_view::decode(bytes).map_err(|e| e.to_string())?;
    match payload {
        loom_result::result_view::ResultPayload::Statements(statements) => {
            for payload in &statements {
                print_sql_payload_value(payload)?;
            }
        }
        loom_result::result_view::ResultPayload::Reader(_) => {
            return Err("Sql.sql_exec_result returned corrupt reader payload".to_string());
        }
    }
    Ok(())
}

fn print_sql_payload_value(payload: &loom_result::result_view::Statement) -> Result<(), String> {
    let payload = sql_payload_from_result_statement(payload)?;
    print_payload(&payload);
    Ok(())
}

fn sql_payload_from_result_statement(
    statement: &loom_result::result_view::Statement,
) -> Result<Payload, String> {
    use loom_result::result_view::Statement;
    Ok(match statement {
        Statement::Select { labels, rows } => {
            let rows = rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(sql_gluesql_value_from_tabular)
                        .collect::<Result<Vec<_>, _>>()
                })
                .collect::<Result<Vec<_>, _>>()?;
            Payload::Select {
                labels: labels.clone(),
                rows,
            }
        }
        Statement::SelectMap(rows) => {
            let rows = rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|(label, value)| {
                            Ok((label.clone(), sql_gluesql_value_from_tabular(value)?))
                        })
                        .collect::<Result<BTreeMap<_, _>, String>>()
                })
                .collect::<Result<Vec<_>, _>>()?;
            Payload::SelectMap(rows)
        }
        Statement::ShowColumns(columns) => {
            let columns = columns
                .iter()
                .map(|column| {
                    Ok((
                        column.name.clone(),
                        loom_sql::data_type_from_result_label(&column.type_name)
                            .map_err(|e| e.to_string())?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            Payload::ShowColumns(columns)
        }
        Statement::Insert(n) => Payload::Insert(sql_result_count(*n)?),
        Statement::Delete(n) => Payload::Delete(sql_result_count(*n)?),
        Statement::Update(n) => Payload::Update(sql_result_count(*n)?),
        Statement::DropTable(n) => Payload::DropTable(sql_result_count(*n)?),
        Statement::Create => Payload::Create,
        Statement::DropFunction => Payload::DropFunction,
        Statement::AlterTable => Payload::AlterTable,
        Statement::CreateIndex => Payload::CreateIndex,
        Statement::DropIndex => Payload::DropIndex,
        Statement::StartTransaction => Payload::StartTransaction,
        Statement::Commit => Payload::Commit,
        Statement::Rollback => Payload::Rollback,
        Statement::ShowVariable(variable) => Payload::ShowVariable(match variable {
            loom_result::result_view::ShowVariable::Tables(values) => {
                gluesql_core::prelude::PayloadVariable::Tables(values.clone())
            }
            loom_result::result_view::ShowVariable::Functions(values) => {
                gluesql_core::prelude::PayloadVariable::Functions(values.clone())
            }
            loom_result::result_view::ShowVariable::Version(value) => {
                gluesql_core::prelude::PayloadVariable::Version(value.clone())
            }
        }),
    })
}

fn sql_result_count(count: u64) -> Result<usize, String> {
    usize::try_from(count).map_err(|_| format!("SQL result count {count} exceeds usize"))
}

fn sql_gluesql_value_from_tabular(value: &loom_core::tabular::Value) -> Result<GValue, String> {
    loom_sql::value_from_tabular(value).map_err(|e| e.to_string())
}

#[cfg(test)]
fn generated_sql_cell_text(value: &loom_core::tabular::Value) -> Result<String, String> {
    Ok(format_value(&sql_gluesql_value_from_tabular(value)?))
}

fn run_sql_cmd(action: SqlCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        SqlCmd::Exec {
            store,
            workspace,
            sql,
            db,
        } => {
            let client = remote::open_cli_generated_client(&store, keys)?;
            let encoded = execute_generated_bytes(
                &client,
                "Sql",
                "sql_exec_result",
                vec![workspace.to_value(), db.to_value(), sql.to_value()],
            )?;
            print_sql_exec_result_cbor(&encoded)
        }
        SqlCmd::Table { action } => run_table(action, keys),
    }
}

#[cfg(test)]
mod root_help_tests {
    use super::*;

    #[test]
    fn generated_sql_value_formatting_matches_legacy_gluesql_formatter() {
        let samples = vec![
            loom_core::tabular::Value::Null,
            loom_core::tabular::Value::Bool(true),
            loom_core::tabular::Value::I8(-8),
            loom_core::tabular::Value::I16(-16),
            loom_core::tabular::Value::I32(-32),
            loom_core::tabular::Value::Int(-64),
            loom_core::tabular::Value::I128(-128),
            loom_core::tabular::Value::U8(8),
            loom_core::tabular::Value::U16(16),
            loom_core::tabular::Value::U32(32),
            loom_core::tabular::Value::U64(64),
            loom_core::tabular::Value::U128(128),
            loom_core::tabular::Value::F32(-1.5),
            loom_core::tabular::Value::Float(2.5),
            loom_core::tabular::Value::Decimal {
                mantissa: 1234500,
                scale: 4,
            },
            loom_core::tabular::Value::Text("hello".into()),
            loom_core::tabular::Value::Bytes(vec![0, 1, 2, 255]),
            loom_core::tabular::Value::Inet(std::net::IpAddr::V4(std::net::Ipv4Addr::new(
                10, 0, 0, 1,
            ))),
            loom_core::tabular::Value::Date(20_260),
            loom_core::tabular::Value::Time(49_530_123_456_789),
            loom_core::tabular::Value::Timestamp(-876_544),
            loom_core::tabular::Value::Interval {
                months: 15,
                micros: 0,
            },
            loom_core::tabular::Value::Interval {
                months: 0,
                micros: -987_654,
            },
            loom_core::tabular::Value::Uuid(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef),
            loom_core::tabular::Value::Point { x: 1.25, y: -2.5 },
            loom_core::tabular::Value::List(vec![
                loom_core::tabular::Value::Int(1),
                loom_core::tabular::Value::Text("x".into()),
            ]),
            loom_core::tabular::Value::Map(BTreeMap::from([(
                "k".to_string(),
                loom_core::tabular::Value::Int(9),
            )])),
        ];
        for value in samples {
            let legacy = format_value(&loom_sql::value_from_tabular(&value).unwrap());
            let generated = generated_sql_cell_text(&value).unwrap();
            assert_eq!(generated, legacy, "generated formatting for {value:?}");
        }
    }

    #[test]
    fn generated_sql_statements_convert_exhaustively_to_gluesql_payloads() {
        use gluesql_core::ast::DataType;
        use gluesql_core::prelude::PayloadVariable;
        use loom_result::result_view::{Column, ShowVariable, Statement};

        let select_row = vec![
            loom_core::tabular::Value::Int(7),
            loom_core::tabular::Value::Text("seven".to_string()),
        ];
        let select_map_row =
            BTreeMap::from([("flag".to_string(), loom_core::tabular::Value::Bool(true))]);
        let cases = vec![
            (
                Statement::Select {
                    labels: vec!["id".to_string(), "name".to_string()],
                    rows: vec![select_row.clone()],
                },
                Payload::Select {
                    labels: vec!["id".to_string(), "name".to_string()],
                    rows: vec![vec![GValue::I64(7), GValue::Str("seven".to_string())]],
                },
            ),
            (
                Statement::SelectMap(vec![select_map_row]),
                Payload::SelectMap(vec![BTreeMap::from([(
                    "flag".to_string(),
                    GValue::Bool(true),
                )])]),
            ),
            (
                Statement::ShowColumns(vec![
                    Column {
                        name: "id".to_string(),
                        type_name: "Int".to_string(),
                    },
                    Column {
                        name: "body".to_string(),
                        type_name: "Text".to_string(),
                    },
                ]),
                Payload::ShowColumns(vec![
                    ("id".to_string(), DataType::Int),
                    ("body".to_string(), DataType::Text),
                ]),
            ),
            (Statement::Insert(1), Payload::Insert(1)),
            (Statement::Delete(2), Payload::Delete(2)),
            (Statement::Update(3), Payload::Update(3)),
            (Statement::DropTable(4), Payload::DropTable(4)),
            (Statement::Create, Payload::Create),
            (Statement::DropFunction, Payload::DropFunction),
            (Statement::AlterTable, Payload::AlterTable),
            (Statement::CreateIndex, Payload::CreateIndex),
            (Statement::DropIndex, Payload::DropIndex),
            (Statement::StartTransaction, Payload::StartTransaction),
            (Statement::Commit, Payload::Commit),
            (Statement::Rollback, Payload::Rollback),
            (
                Statement::ShowVariable(ShowVariable::Tables(vec!["t".to_string()])),
                Payload::ShowVariable(PayloadVariable::Tables(vec!["t".to_string()])),
            ),
            (
                Statement::ShowVariable(ShowVariable::Functions(vec!["f".to_string()])),
                Payload::ShowVariable(PayloadVariable::Functions(vec!["f".to_string()])),
            ),
            (
                Statement::ShowVariable(ShowVariable::Version("0.19".to_string())),
                Payload::ShowVariable(PayloadVariable::Version("0.19".to_string())),
            ),
        ];

        for (statement, expected) in cases {
            assert_eq!(
                sql_payload_from_result_statement(&statement).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn sql_exec_result_rejects_reader_payloads() {
        let reader = WireValue::Map(vec![
            (
                WireValue::Text("kind".into()),
                WireValue::Text("Rows".into()),
            ),
            (
                WireValue::Text("columns".into()),
                WireValue::Array(Vec::new()),
            ),
            (WireValue::Text("rows".into()), WireValue::Array(Vec::new())),
        ]);
        let bytes = loom_codec::encode(&reader).unwrap();
        let error = print_sql_exec_result_cbor(&bytes).unwrap_err();
        assert!(error.contains("corrupt reader payload"));
    }

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
            long.contains("rejects `--stateless`"),
            "help must note local MCP rejects --stateless"
        );
        assert!(
            long.contains("remote MCP statefulness is owned by the remote endpoint"),
            "help must explain remote MCP statefulness"
        );
    }

    #[cfg(feature = "mcp")]
    #[test]
    fn mcp_help_documents_stateless_boundaries() {
        let command = cli_command_for_test();
        let mcp = command
            .find_subcommand("mcp")
            .expect("mcp command should be present");
        let about = mcp.get_about().map(|s| s.to_string()).unwrap_or_default();
        assert!(
            about.contains("daemon-owned generated boundary"),
            "mcp help must explain local daemon boundary"
        );
        assert!(
            about.contains("Local MCP rejects `--stateless`"),
            "mcp help must say local MCP rejects --stateless"
        );
        assert!(
            about.contains("remote endpoint statefulness is owned by that endpoint"),
            "mcp help must explain remote endpoint statefulness"
        );
        let stateless = mcp
            .get_arguments()
            .find(|arg| arg.get_id() == "stateless")
            .expect("mcp --stateless arg should be present");
        let help = stateless
            .get_help()
            .map(|s| s.to_string())
            .unwrap_or_default();
        assert!(
            help.contains("Local MCP rejects this"),
            "--stateless arg help must not claim local stateless support"
        );
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
    fn store_policy_cli_updates_durability_settings() {
        let update = store_policy_update_from_cli(
            Some(true),
            Some("relaxed"),
            vec!["document=normal".to_string(), "ledger=strict".to_string()],
            vec!["search".to_string()],
        )
        .unwrap();

        assert_eq!(
            update.default_durability,
            Some(loom_store::StoreDurabilityPolicy::Relaxed)
        );
        assert_eq!(
            update.facet_durability_assignments[1].durability,
            loom_store::StoreDurabilityPolicy::Strict
        );
        assert_eq!(update.clear_facet_durability, vec![FacetKind::Search]);

        let json: serde_json::Value = serde_json::from_str(&store_policy_result_json(
            loom_wire::store_admin::StorePolicyResult {
                fips_required: true,
                default_durability: loom_store::StoreDurabilityPolicy::Relaxed,
                facet_durability_overrides: update.facet_durability_assignments,
                audit_seq: Some(7),
            },
        ))
        .unwrap();
        assert_eq!(json["fips_required"], true);
        assert_eq!(json["default_durability"], "relaxed");
        assert_eq!(json["facet_durability_overrides"]["document"], "normal");
        assert_eq!(json["audit_seq"], 7);
    }

    #[test]
    fn store_rekey_cli_preserves_raw_kek_request_kind() {
        let raw = [0x42; loom_core::keys::KEY_LEN];
        let credential = store_rekey_credential_from_key_spec(KeySpec::raw_kek(raw));
        let request = loom_wire::store_admin::StoreRekeyRequest {
            credential,
            reseal: true,
            suite: Some("aes-256-gcm".to_string()),
        };
        let decoded = loom_wire::store_admin::store_rekey_request_from_cbor(
            &loom_wire::store_admin::store_rekey_request_to_cbor(&request),
        )
        .unwrap();
        assert!(matches!(
            decoded.credential,
            loom_wire::store_admin::StoreRekeyCredential::RawKek(kek) if kek == raw
        ));
        assert!(decoded.reseal);
        assert_eq!(decoded.suite.as_deref(), Some("aes-256-gcm"));
    }

    #[test]
    fn store_replacement_preflight_report_surfaces_migration_fields() {
        let report_path = std::env::temp_dir().join(format!(
            "loom-store-replacement-preflight-{}.json",
            std::process::id()
        ));
        std::fs::write(
            &report_path,
            r#"{"freshness_watermark":{"source_reference_root":"r1","source_control_root":"c1","workspaces":[]}}"#,
        )
        .unwrap();
        let backup_path = std::env::temp_dir().join(format!(
            "loom-store-replacement-backup-{}",
            std::process::id()
        ));
        std::fs::write(&backup_path, b"backup").unwrap();
        let report_path = report_path.to_string_lossy().into_owned();
        let backup_path = backup_path.to_string_lossy().into_owned();
        let body = store_replacement_preflight_report(
            "candidate.loom",
            "main",
            false,
            &[
                store_preflight_check(
                    "freshness_watermark",
                    false,
                    "candidate is stale relative to live store",
                ),
                store_preflight_check(
                    "vcs_namespace_preflight",
                    false,
                    "legacy projection collisions: docs -> files",
                ),
            ],
            Some("live.loom"),
            Some(&report_path),
            Some("owner-approved"),
            Some(&backup_path),
        );

        assert_eq!(body["safe_to_replace"], serde_json::json!(false));
        assert_eq!(
            body["freshness_watermark"]["available"],
            serde_json::json!(true)
        );
        assert_eq!(
            body["backup_plan"]["stale_candidate_override_ready"],
            serde_json::json!(true)
        );
        assert_eq!(
            body["active_store_freshness"]["ok"],
            serde_json::json!(false)
        );
        assert_eq!(
            body["legacy_projection_collision_risks"]["collision_risk"],
            serde_json::json!(true)
        );

        let _ = std::fs::remove_file(&report_path);
        let _ = std::fs::remove_file(&backup_path);
    }

    #[test]
    fn store_replacement_preflight_rejects_root_codec_mismatch() {
        let diagnostics = loom_store::StoreRootCodecDiagnostics {
            checked_roots: 2,
            failures: vec![loom_store::StoreRootCodecDiagnostic {
                root_name: "owner_tokens",
                family_id: Some(0x0110),
                root_page: 9,
                byte_offset: 36_864,
                expected_codec: "PackedRecordRefCodec",
                expected_discriminator: 0x10,
                raw_magic: Some(0xb7),
                raw_flags: Some(0x00),
                actual_discriminator: Some(0x00),
                in_range: true,
                checksum_ok: true,
                magic_ok: true,
                codec_ok: false,
                reachable: true,
                failure: Some("btree_node_codec_discriminator_mismatch"),
            }],
            details: Vec::new(),
        };
        let root_codec_check = store_replacement_root_codec_check(&diagnostics);
        let body = store_replacement_preflight_report(
            "candidate.loom",
            "matrix2",
            false,
            &[
                store_preflight_check("store_stat", true, "objects=1"),
                root_codec_check,
            ],
            None,
            None,
            None,
            None,
        );

        assert_eq!(body["ok"], serde_json::json!(false));
        assert_eq!(body["safe_to_replace"], serde_json::json!(false));
        assert_eq!(body["checks"][1]["name"], serde_json::json!("root_codecs"));
        assert_eq!(body["checks"][1]["ok"], serde_json::json!(false));
        assert!(
            body["checks"][1]["message"]
                .as_str()
                .unwrap()
                .contains("owner_tokens:page=9 expected=PackedRecordRefCodec actual=0x00")
        );
    }

    #[test]
    fn store_replacement_preflight_reports_descendant_root_codec_failure_page() {
        let diagnostics = loom_store::StoreRootCodecDiagnostics {
            checked_roots: 1,
            failures: vec![loom_store::StoreRootCodecDiagnostic {
                root_name: "retained_history",
                family_id: Some(0x0100),
                root_page: 72,
                byte_offset: 294_912,
                expected_codec: "PackedRecordRefCodec",
                expected_discriminator: 0x10,
                raw_magic: Some(0xb7),
                raw_flags: Some(0x01),
                actual_discriminator: Some(0x00),
                in_range: true,
                checksum_ok: true,
                magic_ok: true,
                codec_ok: false,
                reachable: true,
                failure: Some("btree_node_codec_discriminator_mismatch"),
            }],
            details: Vec::new(),
        };
        let body = store_replacement_preflight_report(
            "candidate.loom",
            "matrix2",
            false,
            &[
                store_preflight_check("store_stat", true, "objects=1"),
                store_replacement_root_codec_check(&diagnostics),
            ],
            None,
            None,
            None,
            None,
        );

        assert_eq!(body["ok"], serde_json::json!(false));
        assert_eq!(body["safe_to_replace"], serde_json::json!(false));
        assert!(
            body["checks"][1]["message"]
                .as_str()
                .unwrap()
                .contains("retained_history:page=72 expected=PackedRecordRefCodec actual=0x00")
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

#[cfg(test)]
mod interchange_cli_tests {
    use super::*;

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

    #[test]
    fn import_table_csv_routes_generated_contract() {
        let store = temp_store("table-csv-import-generated");
        let mut csv = std::env::temp_dir();
        csv.push(format!(
            "loom-cli-table-csv-{}-{}.csv",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&csv, "id,name,note\n1,alpha,\"from cli\"\n2,beta,\"\"\n").unwrap();

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
            InterchangeCmd::ImportTableCsv {
                store: store.clone(),
                workspace: "main".to_string(),
                database: "app".to_string(),
                table: "items".to_string(),
                csv: csv.to_string_lossy().into_owned(),
                schema: "id:int,name:text,note:text".to_string(),
                primary_key: "id".to_string(),
                mode: "snapshot".to_string(),
                commit: true,
                dry_run: false,
                author: "tester".to_string(),
                message: "table import".to_string(),
                format: "json".to_string(),
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let table = loom_core::get_table(
            &loom,
            ns,
            &loom_core::workspace::facet_path(FacetKind::Sql, "app/tables/items"),
        )
        .unwrap();
        let mut rows = table.scan(&loom_core::Predicate::All);
        rows.sort_by_key(|row| match &row[0] {
            loom_core::Value::Int(id) => *id,
            other => panic!("unexpected id cell {other:?}"),
        });
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], loom_core::Value::Int(1));
        assert_eq!(rows[0][1], loom_core::Value::Text("alpha".to_string()));
        assert_eq!(rows[0][2], loom_core::Value::Text("from cli".to_string()));
        assert_eq!(rows[1][2], loom_core::Value::Text(String::new()));

        let _ = std::fs::remove_file(&store);
        let _ = std::fs::remove_file(csv);
    }

    #[test]
    fn ticket_profile_imports_route_generated_contracts() {
        let cases = [
            (
                "redmine",
                InterchangeCmd::ImportRedmine {
                    store: String::new(),
                    workspace: "main".to_string(),
                    profile: "studio".to_string(),
                    snapshot: String::new(),
                    dry_run: false,
                    field_policy: "infer".to_string(),
                    format: "json".to_string(),
                },
                "redmine",
                "issue:42",
                r#"{"source_scope":"redmine://example","projects":[{"id":1,"identifier":"core","key_prefix":"CORE","name":"Core"}],"issues":[{"id":42,"project_identifier":"core","tracker":"Bug","subject":"Login fails","custom_fields":{"severity":"critical"}}]}"#,
            ),
            (
                "asana",
                InterchangeCmd::ImportAsana {
                    store: String::new(),
                    workspace: "main".to_string(),
                    profile: "studio".to_string(),
                    snapshot: String::new(),
                    dry_run: false,
                    field_policy: "infer".to_string(),
                    format: "json".to_string(),
                },
                "asana",
                "task:t1",
                r#"{"source_scope":"asana://workspace","projects":[{"gid":"p1","key_prefix":"AS","name":"Asana Project"}],"tasks":[{"gid":"t1","project_gid":"p1","name":"Ship importer","custom_fields":{"size":"M"}}]}"#,
            ),
            (
                "jira",
                InterchangeCmd::ImportJira {
                    store: String::new(),
                    workspace: "main".to_string(),
                    profile: "studio".to_string(),
                    snapshot: String::new(),
                    dry_run: false,
                    field_policy: "infer".to_string(),
                    format: "json".to_string(),
                },
                "jira",
                "issue:10042",
                r#"{"source_scope":"jira://site","projects":[{"id":10001,"key":"CORE","name":"Core"}],"issues":[{"id":10042,"key":"CORE-42","project_key":"CORE","issue_type":"Bug","summary":"Login fails","custom_fields":{"severity":"critical"}}]}"#,
            ),
        ];

        for (tag, template, source, external_id, payload) in cases {
            let store = temp_store(&format!("{tag}-profile-generated"));
            let snapshot = std::env::temp_dir().join(format!(
                "loom-cli-{tag}-{}-{}.json",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::write(&snapshot, payload).unwrap();
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
            let snapshot = snapshot.to_string_lossy().into_owned();
            let action = match template {
                InterchangeCmd::ImportRedmine {
                    workspace,
                    profile,
                    dry_run,
                    field_policy,
                    format,
                    ..
                } => InterchangeCmd::ImportRedmine {
                    store: store.clone(),
                    workspace,
                    profile,
                    snapshot: snapshot.clone(),
                    dry_run,
                    field_policy,
                    format,
                },
                InterchangeCmd::ImportAsana {
                    workspace,
                    profile,
                    dry_run,
                    field_policy,
                    format,
                    ..
                } => InterchangeCmd::ImportAsana {
                    store: store.clone(),
                    workspace,
                    profile,
                    snapshot: snapshot.clone(),
                    dry_run,
                    field_policy,
                    format,
                },
                InterchangeCmd::ImportJira {
                    workspace,
                    profile,
                    dry_run,
                    field_policy,
                    format,
                    ..
                } => InterchangeCmd::ImportJira {
                    store: store.clone(),
                    workspace,
                    profile,
                    snapshot: snapshot.clone(),
                    dry_run,
                    field_policy,
                    format,
                },
                _ => unreachable!("profile import case"),
            };
            run_interchange(action, &KeyOpts::default()).unwrap();
            let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
            let ns = resolve_ns(&loom, "main").unwrap();
            let reader = loom_tickets::TicketProfileReader::open(&loom, ns, "studio")
                .unwrap()
                .unwrap();
            let identity = loom_tickets::ExternalTicketIdentity::new(source, external_id).unwrap();
            assert!(
                reader
                    .ticket_by_external_identity(&identity)
                    .unwrap()
                    .is_some()
            );
            let _ = std::fs::remove_file(&store);
            let _ = std::fs::remove_file(snapshot);
        }
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
        let history =
            loom_substrate::versioning::load_current_revision_index(&loom, ns, &profile_id)
                .unwrap();
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
            loom_substrate::versioning::load_current_revision_index(&loom, ns, &profile_id)
                .unwrap();
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
        let history =
            loom_substrate::versioning::load_current_revision_index(&loom, ns, &profile_id)
                .unwrap();
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
        let history =
            loom_substrate::versioning::load_current_revision_index(&loom, ns, &profile_id)
                .unwrap();
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
        let history =
            loom_substrate::versioning::load_current_revision_index(&loom, ns, &profile_id)
                .unwrap();
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
        assert_eq!(result.workspace, ns.to_string());
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
            workspace: WorkspaceId::from_bytes([7; 16]).to_string(),
            embedding_instance: "fast-embed".to_string(),
        });
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        init_control_state(&fs).unwrap();
        let mut loom = open_loom_from(fs, &KeyOpts::default(), false).unwrap();
        let workspace = WorkspaceId::from_bytes([7; 16]);
        loom.registry_mut()
            .create(FacetKind::Vector, Some("main"), workspace)
            .unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);
        let client = LocalLoomClient::new(&store);
        let session = client.open().unwrap();
        let instance = state.find_instance("fast-embed").unwrap();
        client
            .inference_instance_create_json(
                &session,
                "main",
                instance.name.clone(),
                instance.model.repo_id.clone(),
                instance.kind.as_str().to_string(),
                instance.runtime.as_str().to_string(),
                instance.preset.clone(),
                "{}",
            )
            .unwrap();
        client
            .vector_workspace_configure_json(
                &session,
                "main",
                r#"{"embedding-instance":"fast-embed"}"#,
            )
            .unwrap();
        client.close(&session);
        let loom = loom_store::open_loom_read(&store).unwrap();
        let hardware = smoke_hardware_report(&hf_cache);
        let resolved = resolve_vector_text_embedding_instance_from_cache(
            &hf_cache, hardware, &loom, workspace, None,
        )
        .unwrap();
        drop(loom);
        let fs = FileStore::open(&store).unwrap();
        let mut loom = open_loom_from(fs, &KeyOpts::default(), false).unwrap();
        let ns = loom
            .registry()
            .open(&WsSelector::Name("main".to_string()))
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
        let body = loom_substrate::body::Body::decode(page.body.as_deref().unwrap()).unwrap();
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
                    required_acceptance_reviews: Vec::new(),
                    replace_required_acceptance_reviews: false,
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
                    ticket_type: "task".to_string(),
                    project_id: Some("core".to_string()),
                    title: None,
                    description: None,
                    priority: None,
                    assignee: None,
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
                    comment_body: None,
                    comment_id: None,
                    comment_type: None,
                    comment_evidence: None,
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
                    comment_body: None,
                    comment_id: None,
                    comment_type: None,
                    comment_evidence: None,
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
                    compact: false,
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
    fn store_replacement_preflight_rejects_stale_ticket_candidate() {
        let store = temp_store("replacement-live");
        let candidate = temp_store("replacement-candidate");
        let mut report = std::path::PathBuf::from(&candidate);
        report.set_extension("json");
        let report = report.to_string_lossy().into_owned();
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
                action: TicketsCmd::Create {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    ticket_type: "task".to_string(),
                    project_id: Some("core".to_string()),
                    title: Some("Replace safely".to_string()),
                    description: None,
                    priority: None,
                    assignee: None,
                    fields: "{}".to_string(),
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
            Command::Store {
                action: StoreCmd::Copy {
                    src: store.clone(),
                    dst: candidate.clone(),
                    with: Vec::new(),
                    format: "json".to_string(),
                    report_file: Some(report.clone()),
                    dry_run: false,
                    new_key_source: None,
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
                    assignee: None,
                    title: None,
                    description: None,
                    priority: None,
                    fields: Vec::new(),
                    delete_fields: Vec::new(),
                    action: None,
                    comment_body: Some("advanced live state".to_string()),
                    comment_id: None,
                    comment_type: None,
                    comment_evidence: None,
                    observed_source_status: None,
                    observed_workflow_version: None,
                    expected_root: None,
                    format: "text".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();
        let error = run_store_replacement_preflight(
            &candidate,
            "main",
            Some(&store),
            Some(&report),
            None,
            None,
            "json",
            &KeyOpts::default(),
        )
        .unwrap_err();
        assert!(error.contains("store replacement preflight failed"));
        let report_body = store_replacement_preflight_report(
            &candidate,
            "main",
            false,
            &[
                store_preflight_check(
                    "freshness_watermark",
                    false,
                    "candidate is stale relative to live store",
                ),
                store_preflight_check(
                    "vcs_namespace_preflight",
                    true,
                    "no VCS namespace collisions from legacy projections",
                ),
            ],
            Some(&store),
            Some(&report),
            None,
            None,
        );
        assert_eq!(report_body["safe_to_replace"], serde_json::json!(false));
        assert_eq!(
            report_body["freshness_watermark"]["available"],
            serde_json::json!(true)
        );
        assert_eq!(
            report_body["backup_plan"]["stale_candidate_override_ready"],
            serde_json::json!(false)
        );
        assert_eq!(
            report_body["active_store_freshness"]["ok"],
            serde_json::json!(false)
        );
        assert_eq!(
            report_body["legacy_projection_collision_risks"]["collision_risk"],
            serde_json::json!(false)
        );

        let _ = std::fs::remove_file(&store);
        let _ = std::fs::remove_file(&candidate);
        let _ = std::fs::remove_file(&report);
    }

    #[test]
    fn lane_list_output_surfaces_consistency_warnings_in_json_and_text() {
        let diagnostics = vec![LaneDiagnostic {
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
                    title: String::new(),
                    description: String::new(),
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
        let intro_body =
            loom_substrate::body::Body::decode(intro.body.as_deref().unwrap()).unwrap();
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
        let embed_body =
            loom_substrate::body::Body::decode(embed.body.as_deref().unwrap()).unwrap();
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
        let body = loom_substrate::body::Body::decode(page.body.as_deref().unwrap()).unwrap();
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
        let mut report = loom_interchange::ImportReport::new(loom_interchange::ImportReportInput {
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

        let store_path = temp_store("meetings-import");
        let store = FileStore::create_with_profile(&store_path, Algo::Blake3).unwrap();
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
        save_loom(&mut loom).unwrap();
        drop(loom);
        let client = LocalLoomClient::new(&store_path);
        let session = client.open().unwrap();
        let result_json = client
            .meetings_import_snapshot(
                &session,
                "studio",
                "granola-api",
                serde_json::to_string(&input).unwrap().as_bytes(),
                false,
            )
            .unwrap();
        client.close(&session);
        let result = import_report_from_json(&result_json).unwrap();
        let loom = loom_store::open_loom_read(&store_path).unwrap();
        let snapshot = load_meetings_snapshot_io(&loom, &workspace_id.to_string())
            .unwrap()
            .unwrap();

        assert_eq!(result.rows_imported, 1);
        assert_eq!(result.operations_planned, 7);
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
        let history = loom_substrate::versioning::load_current_revision_index(
            &loom,
            workspace_id,
            &profile_id,
        )
        .unwrap();
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
                &loom_interchange_io::meetings_source_payload_path(
                    &profile_id,
                    "source-a",
                    "source.json",
                )
            )
            .unwrap(),
            br#"{"raw":"source"}"#
        );
        assert_eq!(
            loom.read_file_reserved(
                workspace_id,
                &loom_interchange_io::meetings_source_payload_path(
                    &profile_id,
                    "source-a",
                    "summary.txt",
                )
            )
            .unwrap(),
            b"Planning summary"
        );
        assert_eq!(
            loom.read_file_reserved(
                workspace_id,
                &loom_interchange_io::meetings_source_payload_path(
                    &profile_id,
                    "source-a",
                    "transcript.jsonl",
                )
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
        let history = loom_substrate::versioning::load_current_revision_index(
            &loom,
            workspace_id,
            &profile_id,
        )
        .unwrap();
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
            workspace: "main".to_string(),
            embedding_instance: "fast-embed".to_string(),
        };
        let rendered = serde_json::to_string_pretty(&binding).unwrap();
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        assert!(value.get("store").is_none());
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

    #[test]
    fn ticket_project_create_ensures_workspace_through_generated_client() {
        let store = mx250_temp_store("mx495-ticket-project-create");
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
            Command::Tickets {
                action: TicketsCmd::ProjectCreate {
                    store: store.clone(),
                    workspace: "main".to_string(),
                    project_id: "core".to_string(),
                    key_prefix: "CORE".to_string(),
                    name: "Core".to_string(),
                    expected_root: None,
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        assert!(loom.registry().has_facet(ns, FacetKind::Vcs).unwrap());
        let profile_id = ns.to_string();
        let project = loom_tickets::get_project(&loom, ns, &profile_id, "core")
            .unwrap()
            .unwrap();
        assert_eq!(project.key_prefix, "CORE");

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn lanes_get_and_list_read_through_generated_client() {
        let store = mx250_temp_store("mx495-lanes-read");
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
                    lane_id: "agent-read".to_string(),
                    lane_key: "agent-read".to_string(),
                    kind: "assignment".to_string(),
                    title: "Agent Read".to_string(),
                    description: String::new(),
                    owner_principal: Some("agent:read".to_string()),
                    lane_status: "ready".to_string(),
                    active_ticket_id: None,
                    status_report: "ready".to_string(),
                    reviewer_feedback: String::new(),
                    updated_at: Some(1),
                    updated_by: Some("agent:read".to_string()),
                    tickets: vec!["MX-495".to_string()],
                    format: "json".to_string(),
                },
            },
            &KeyOpts::default(),
        )
        .unwrap();

        for detailed in [false, true] {
            run(
                Command::Lanes {
                    action: LanesCmd::Get {
                        store: store.clone(),
                        workspace: "main".to_string(),
                        lane_id: "agent-read".to_string(),
                        detailed,
                        format: "json".to_string(),
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
                        detailed,
                        format: "json".to_string(),
                    },
                },
                &KeyOpts::default(),
            )
            .unwrap();
        }

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "main").unwrap();
        let lane = loom_lanes::get_lane(&loom, ns, "agent-read")
            .unwrap()
            .unwrap();
        assert_eq!(lane.lane_key, "agent-read");
        assert_eq!(lane.lane_tickets[0].ticket_id, "MX-495");

        let _ = std::fs::remove_file(&store);
    }
}
