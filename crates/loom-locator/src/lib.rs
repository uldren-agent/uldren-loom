//! Loom locator parsing and context resolution.
//!
//! A `<STORE>` locator is classified syntactically by [`parse`] and then resolved against the layered
//! context configuration by [`ContextResolver`]. The crate is engine-free: it depends only on a TOML reader
//! and returns [`LocatorError`], which a client maps to the stable error `Code` at its boundary.
//!
//! Licensed under BUSL-1.1.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Context fields that carry secret material and are rejected in committable config.
pub const SECRET_FIELDS: &[&str] = &[
    "token",
    "password",
    "passphrase",
    "secret",
    "private_key",
    "client_key",
    "bearer",
    "api_key",
];

/// A locator classified by syntax alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocatorForm {
    /// A bare token with no path separator.
    Bare(String),
    /// An `http://` or `https://` remote URL.
    RemoteUrl(String),
    /// The path carried by a `file://` URL.
    FileUrl(String),
    /// A path-like local input (contains a separator or starts with `.` or `~`).
    Path(String),
}

/// Endpoint discovery mode for a remote target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Discovery {
    /// Service-root first for path URLs, else host well-known.
    #[default]
    Default,
    /// Fetch discovery from the locator service root only.
    ServiceRoot,
    /// Fetch discovery from the host-level well-known path only.
    WellKnown,
    /// Treat the locator URL as the exact protocol endpoint.
    Disabled,
}

impl Discovery {
    fn parse(value: &str) -> Result<Self, LocatorError> {
        match value {
            "default" => Ok(Self::Default),
            "service-root" => Ok(Self::ServiceRoot),
            "well-known" => Ok(Self::WellKnown),
            "disabled" => Ok(Self::Disabled),
            other => Err(LocatorError::Config(format!(
                "invalid discovery mode `{other}` (expected default, service-root, well-known, or disabled)"
            ))),
        }
    }
}

/// A resolved locator target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A remote Loom endpoint.
    Remote(RemoteTarget),
    /// A local `.loom` path.
    Local(PathBuf),
}

/// A resolved remote endpoint and its non-secret selectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTarget {
    /// The service-root candidate URL.
    pub url: String,
    /// Auth selector name, never secret material.
    pub auth: Option<String>,
    /// TLS trust selector, never key material.
    pub tls: Option<String>,
    /// Discovery mode.
    pub discovery: Discovery,
    /// Absolute path for custom discovery.
    pub discovery_path: Option<String>,
    /// Connect timeout in milliseconds.
    pub connect_timeout_ms: Option<u64>,
    /// Request timeout in milliseconds.
    pub request_timeout_ms: Option<u64>,
}

impl RemoteTarget {
    /// A remote target for a bare URL locator, with default selectors.
    fn from_url(url: String) -> Self {
        Self {
            url,
            auth: None,
            tls: None,
            discovery: Discovery::Default,
            discovery_path: None,
            connect_timeout_ms: None,
            request_timeout_ms: None,
        }
    }
}

/// A validated context definition loaded from configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextDef {
    /// The local file URL/path or remote service URL.
    pub target: String,
    is_file: bool,
    /// Optional default workspace selector.
    pub default_workspace: Option<String>,
    auth: Option<String>,
    tls: Option<String>,
    discovery: Discovery,
    discovery_path: Option<String>,
    connect_timeout_ms: Option<u64>,
    request_timeout_ms: Option<u64>,
}

impl ContextDef {
    /// Resolve this context to its local or remote target.
    pub fn to_target(&self) -> Target {
        if self.is_file {
            Target::Local(PathBuf::from(
                strip_file_scheme(&self.target).unwrap_or(&self.target),
            ))
        } else {
            Target::Remote(RemoteTarget {
                url: self.target.clone(),
                auth: self.auth.clone(),
                tls: self.tls.clone(),
                discovery: self.discovery,
                discovery_path: self.discovery_path.clone(),
                connect_timeout_ms: self.connect_timeout_ms,
                request_timeout_ms: self.request_timeout_ms,
            })
        }
    }
}

/// The raw shape of one `[contexts.<name>]` table. Unknown fields are rejected so a typo or a secret
/// field that slips past the explicit scan still fails loudly.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextEntry {
    pub target: String,
    #[serde(default)]
    pub default_workspace: Option<String>,
    #[serde(default)]
    pub auth: Option<String>,
    #[serde(default)]
    pub tls: Option<String>,
    #[serde(default)]
    pub discovery: Option<String>,
    #[serde(default)]
    pub discovery_path: Option<String>,
    #[serde(default)]
    pub connect_timeout_ms: Option<u64>,
    #[serde(default)]
    pub request_timeout_ms: Option<u64>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CliContextSelection {
    #[serde(default)]
    pub current_context: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ContextFile {
    #[serde(default)]
    pub contexts: BTreeMap<String, ContextEntry>,
    #[serde(default)]
    pub cli: CliContextSelection,
}

struct ParsedContextFile {
    contexts: BTreeMap<String, ContextDef>,
    cli: CliContextSelection,
}

/// An error classifying or resolving a locator.
#[derive(Debug, thiserror::Error)]
pub enum LocatorError {
    /// The locator string was empty.
    #[error("empty locator")]
    Empty,
    /// An explicit context did not match any loaded context.
    #[error("context not found: {0}")]
    ContextNotFound(String),
    /// The locator string was malformed.
    #[error("invalid locator: {0}")]
    Invalid(String),
    /// A URL carried forbidden user-info.
    #[error("url user-info is not allowed in a locator: {0}")]
    UserInfo(String),
    /// A context declared a secret-bearing field.
    #[error("context `{context}` declares secret-bearing field `{field}`, which is not allowed")]
    SecretField {
        /// The context name.
        context: String,
        /// The rejected field name.
        field: String,
    },
    /// The context configuration was invalid.
    #[error("invalid context config: {0}")]
    Config(String),
    /// A configuration file could not be read.
    #[error("cannot read context config {path}: {source}")]
    Io {
        /// The path that could not be read.
        path: String,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
}

/// Classify a locator string by syntax.
///
/// # Errors
/// Returns [`LocatorError`] for an empty string or a URL with user-info.
pub fn parse(input: &str) -> Result<LocatorForm, LocatorError> {
    if input.is_empty() {
        return Err(LocatorError::Empty);
    }
    let lower = input.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        check_no_userinfo(input)?;
        return Ok(LocatorForm::RemoteUrl(input.to_string()));
    }
    if let Some(path) = strip_file_scheme(input) {
        return Ok(LocatorForm::FileUrl(path.to_string()));
    }
    if is_path_like(input) {
        return Ok(LocatorForm::Path(input.to_string()));
    }
    Ok(LocatorForm::Bare(input.to_string()))
}

fn is_path_like(input: &str) -> bool {
    input.contains('/') || input.starts_with('.') || input.starts_with('~')
}

fn strip_file_scheme(input: &str) -> Option<&str> {
    let lower = input.to_ascii_lowercase();
    if lower.starts_with("file://") {
        Some(&input["file://".len()..])
    } else {
        None
    }
}

fn check_no_userinfo(url: &str) -> Result<(), LocatorError> {
    if let Some(after_scheme) = url.split_once("://").map(|(_, rest)| rest) {
        let authority = after_scheme
            .split(['/', '?', '#'])
            .next()
            .unwrap_or(after_scheme);
        if authority.contains('@') {
            return Err(LocatorError::UserInfo(url.to_string()));
        }
    }
    Ok(())
}

/// One configuration layer: an origin label and its TOML content.
#[derive(Debug, Clone)]
pub struct Layer {
    /// A human-readable origin (path or label) for error messages.
    pub origin: String,
    /// The TOML content of the layer.
    pub content: String,
}

impl Layer {
    /// Construct a layer from an origin label and content.
    pub fn new(origin: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            origin: origin.into(),
            content: content.into(),
        }
    }
}

/// Filesystem roots for the standard layered configuration.
#[derive(Debug, Default, Clone)]
pub struct LayerRoots {
    /// Explicit `--config` files, in command-line order.
    pub explicit_configs: Vec<PathBuf>,
    /// Project root; the file read is `<project>/.loom/contexts.toml`.
    pub project: Option<PathBuf>,
    /// User home; the file read is `<home>/.loom/contexts.toml`.
    pub user_home: Option<PathBuf>,
    /// System config root; the file read is `<system>/loom/contexts.toml`.
    pub system: Option<PathBuf>,
}

/// A layered context resolver. Higher-precedence layers shadow lower ones by context name.
#[derive(Debug, Default, Clone)]
pub struct ContextResolver {
    contexts: BTreeMap<String, ContextDef>,
    current_context: Option<String>,
}

impl ContextResolver {
    /// Build a resolver from layers ordered lowest precedence first. Higher-precedence layers overwrite lower-precedence
    /// contexts of the same name.
    ///
    /// # Errors
    /// Returns [`LocatorError`] for malformed TOML, an invalid context, or a secret-bearing field.
    pub fn from_layers(layers: &[Layer]) -> Result<Self, LocatorError> {
        let mut contexts = BTreeMap::new();
        let mut current_context = None;
        for layer in layers {
            let parsed = parse_file(&layer.origin, &layer.content)?;
            if parsed.cli.current_context.is_some() {
                current_context = parsed.cli.current_context;
            }
            for (name, def) in parsed.contexts {
                contexts.insert(name, def);
            }
        }
        Ok(Self {
            contexts,
            current_context,
        })
    }

    /// Load the standard layers from disk (system, then user, then project, then explicit configs,
    /// lowest precedence first). Missing files are skipped; unreadable files are an error.
    ///
    /// # Errors
    /// Returns [`LocatorError`] for an unreadable file, malformed TOML, or an invalid context.
    pub fn load(roots: &LayerRoots) -> Result<Self, LocatorError> {
        let mut paths: Vec<PathBuf> = Vec::new();
        if let Some(system) = &roots.system {
            paths.push(system.join("loom").join("contexts.toml"));
        }
        if let Some(home) = &roots.user_home {
            paths.push(home.join(".loom").join("contexts.toml"));
        }
        if let Some(project) = &roots.project {
            paths.push(project.join(".loom").join("contexts.toml"));
        }
        paths.extend(roots.explicit_configs.iter().cloned());

        let mut layers: Vec<Layer> = Vec::new();
        for path in paths {
            match std::fs::read_to_string(&path) {
                Ok(content) => layers.push(Layer::new(path.display().to_string(), content)),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(LocatorError::Io {
                        path: path.display().to_string(),
                        source,
                    });
                }
            }
        }
        Self::from_layers(&layers)
    }

    /// Whether a context of this name is loaded.
    pub fn has_context(&self, name: &str) -> bool {
        self.contexts.contains_key(name)
    }

    /// List loaded context names in sorted order.
    pub fn context_names(&self) -> Vec<String> {
        self.contexts.keys().cloned().collect()
    }

    /// Return the current context selected by the highest-precedence layer.
    pub fn current_context(&self) -> Option<&str> {
        self.current_context.as_deref()
    }

    /// Return one loaded context definition.
    pub fn context(&self, name: &str) -> Option<&ContextDef> {
        self.contexts.get(name)
    }

    /// Resolve a classified locator form to a target.
    ///
    /// # Errors
    /// Returns [`LocatorError`] when the locator cannot be parsed.
    pub fn resolve(&self, form: &LocatorForm) -> Result<Target, LocatorError> {
        match form {
            LocatorForm::RemoteUrl(url) => Ok(Target::Remote(RemoteTarget::from_url(url.clone()))),
            LocatorForm::FileUrl(path) => Ok(Target::Local(PathBuf::from(path))),
            LocatorForm::Bare(name) => Ok(Target::Local(PathBuf::from(name))),
            LocatorForm::Path(path) => Ok(Target::Local(PathBuf::from(path))),
        }
    }

    /// Parse and resolve a locator string in one call.
    ///
    /// # Errors
    /// Returns [`LocatorError`] from either [`parse`] or [`ContextResolver::resolve`].
    pub fn resolve_str(&self, input: &str) -> Result<Target, LocatorError> {
        self.resolve(&parse(input)?)
    }

    /// Resolve a named context to a target.
    pub fn resolve_context(&self, name: &str) -> Result<Target, LocatorError> {
        self.contexts
            .get(name)
            .map(ContextDef::to_target)
            .ok_or_else(|| LocatorError::ContextNotFound(name.to_string()))
    }
}

fn parse_file(origin: &str, content: &str) -> Result<ParsedContextFile, LocatorError> {
    let table: toml::Table =
        toml::from_str(content).map_err(|err| LocatorError::Config(format!("{origin}: {err}")))?;
    for key in table.keys() {
        if key != "contexts" && key != "cli" {
            return Err(LocatorError::Config(format!(
                "{origin}: unexpected top-level key `{key}` (expected `contexts` or `cli`)"
            )));
        }
    }
    let cli = table
        .get("cli")
        .cloned()
        .map(toml::Value::try_into)
        .transpose()
        .map_err(|err| LocatorError::Config(format!("{origin}: cli: {err}")))?
        .unwrap_or_default();
    let mut out = ParsedContextFile {
        cli,
        contexts: BTreeMap::new(),
    };
    let Some(contexts_value) = table.get("contexts") else {
        return Ok(out);
    };
    let contexts = contexts_value
        .as_table()
        .ok_or_else(|| LocatorError::Config(format!("{origin}: `contexts` must be a table")))?;
    for (name, entry_value) in contexts {
        let entry_table = entry_value.as_table().ok_or_else(|| {
            LocatorError::Config(format!("{origin}: context `{name}` must be a table"))
        })?;
        for secret in SECRET_FIELDS {
            if entry_table.contains_key(*secret) {
                return Err(LocatorError::SecretField {
                    context: name.clone(),
                    field: (*secret).to_string(),
                });
            }
        }
        let entry: ContextEntry = entry_value
            .clone()
            .try_into()
            .map_err(|err| LocatorError::Config(format!("{origin}: context `{name}`: {err}")))?;
        out.contexts
            .insert(name.clone(), context_def_from_entry(origin, name, entry)?);
    }
    Ok(out)
}

fn context_def_from_entry(
    origin: &str,
    name: &str,
    entry: ContextEntry,
) -> Result<ContextDef, LocatorError> {
    let lower = entry.target.to_ascii_lowercase();
    let (is_file, is_remote) = (
        lower.starts_with("file://"),
        lower.starts_with("http://") || lower.starts_with("https://"),
    );
    let is_path = !is_file
        && !is_remote
        && !entry.target.contains("://")
        && (is_path_like(&entry.target) || entry.target.ends_with(".loom"));
    if !is_file && !is_remote && !is_path {
        return Err(LocatorError::Config(format!(
            "{origin}: context `{name}` target must be an http(s) URL, file URL, or local path"
        )));
    }
    if is_remote {
        check_no_userinfo(&entry.target)?;
    }
    let discovery = match entry.discovery.as_deref() {
        Some(value) => Discovery::parse(value)?,
        None => Discovery::Default,
    };
    for (label, value) in [
        ("connect_timeout_ms", entry.connect_timeout_ms),
        ("request_timeout_ms", entry.request_timeout_ms),
    ] {
        if value == Some(0) {
            return Err(LocatorError::Config(format!(
                "{origin}: context `{name}` {label} must be a positive integer"
            )));
        }
    }
    Ok(ContextDef {
        target: entry.target,
        is_file: is_file || is_path,
        default_workspace: entry.default_workspace,
        auth: entry.auth,
        tls: entry.tls,
        discovery,
        discovery_path: entry.discovery_path,
        connect_timeout_ms: entry.connect_timeout_ms,
        request_timeout_ms: entry.request_timeout_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_urls_and_rejects_userinfo() {
        assert_eq!(
            parse("https://remote.host/app").unwrap(),
            LocatorForm::RemoteUrl("https://remote.host/app".to_string())
        );
        assert_eq!(
            parse("HTTPS://Remote.Host").unwrap(),
            LocatorForm::RemoteUrl("HTTPS://Remote.Host".to_string())
        );
        assert!(matches!(
            parse("https://user:pw@remote.host"),
            Err(LocatorError::UserInfo(_))
        ));
        assert_eq!(
            parse("file://app.loom").unwrap(),
            LocatorForm::FileUrl("app.loom".to_string())
        );
        assert_eq!(
            parse("file:///abs/app.loom").unwrap(),
            LocatorForm::FileUrl("/abs/app.loom".to_string())
        );
    }

    #[test]
    fn distinguishes_bare_from_path_like() {
        assert_eq!(
            parse("prod").unwrap(),
            LocatorForm::Bare("prod".to_string())
        );
        assert_eq!(
            parse("app.loom").unwrap(),
            LocatorForm::Bare("app.loom".to_string())
        );
        assert_eq!(
            parse("./prod").unwrap(),
            LocatorForm::Path("./prod".to_string())
        );
        assert_eq!(
            parse("/abs/app.loom").unwrap(),
            LocatorForm::Path("/abs/app.loom".to_string())
        );
        assert_eq!(
            parse("dir/app.loom").unwrap(),
            LocatorForm::Path("dir/app.loom".to_string())
        );
        assert!(matches!(parse(""), Err(LocatorError::Empty)));
    }

    #[test]
    fn percent_encoded_path_stays_local() {
        assert_eq!(
            parse("./a%20b.loom").unwrap(),
            LocatorForm::Path("./a%20b.loom".to_string())
        );
    }

    fn resolver_with(content: &str) -> ContextResolver {
        ContextResolver::from_layers(&[Layer::new("test", content)]).unwrap()
    }

    #[test]
    fn resolves_remote_and_file_contexts() {
        let r = resolver_with(
            "[contexts.prod]\ntarget = \"https://loom.example.com/prod\"\nauth = \"interactive\"\ntls = \"system\"\n\n[contexts.local]\ntarget = \"file://app.loom\"\n",
        );
        match r.resolve_context("prod").unwrap() {
            Target::Remote(t) => {
                assert_eq!(t.url, "https://loom.example.com/prod");
                assert_eq!(t.auth.as_deref(), Some("interactive"));
                assert_eq!(t.discovery, Discovery::Default);
            }
            other => panic!("expected remote, got {other:?}"),
        }
        assert_eq!(
            r.resolve_context("local").unwrap(),
            Target::Local(PathBuf::from("app.loom"))
        );
    }

    #[test]
    fn context_does_not_shadow_local_file() {
        let r = resolver_with("[contexts.prod]\ntarget = \"https://loom.example.com/prod\"\n");
        assert_eq!(
            r.resolve_str("prod").unwrap(),
            Target::Local(PathBuf::from("prod"))
        );
        assert!(matches!(
            r.resolve_context("prod").unwrap(),
            Target::Remote(_)
        ));
        assert_eq!(
            r.resolve_str("./prod").unwrap(),
            Target::Local(PathBuf::from("./prod"))
        );
    }

    #[test]
    fn unknown_bare_token_is_local_path() {
        let r = ContextResolver::default();
        assert_eq!(
            r.resolve_str("prod").unwrap(),
            Target::Local(PathBuf::from("prod"))
        );
    }

    #[test]
    fn explicit_context_must_exist() {
        let r = ContextResolver::default();
        assert!(matches!(
            r.resolve_context("prod"),
            Err(LocatorError::ContextNotFound(_))
        ));
    }

    #[test]
    fn higher_precedence_layer_shadows_lower() {
        let system = Layer::new(
            "system",
            "[contexts.prod]\ntarget = \"https://system.example\"\n",
        );
        let project = Layer::new(
            "project",
            "[contexts.prod]\ntarget = \"https://project.example\"\n",
        );
        let r = ContextResolver::from_layers(&[system, project]).unwrap();
        match r.resolve_context("prod").unwrap() {
            Target::Remote(t) => assert_eq!(t.url, "https://project.example"),
            other => panic!("expected remote, got {other:?}"),
        }
    }

    #[test]
    fn rejects_secret_fields() {
        let err = ContextResolver::from_layers(&[Layer::new(
            "test",
            "[contexts.prod]\ntarget = \"https://loom.example\"\ntoken = \"abc\"\n",
        )])
        .unwrap_err();
        match err {
            LocatorError::SecretField { context, field } => {
                assert_eq!(context, "prod");
                assert_eq!(field, "token");
            }
            other => panic!("expected secret-field error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_and_malformed() {
        assert!(matches!(
            ContextResolver::from_layers(&[Layer::new(
                "test",
                "[contexts.prod]\ntarget = \"https://loom.example\"\nnope = 1\n",
            )]),
            Err(LocatorError::Config(_))
        ));
        assert!(matches!(
            ContextResolver::from_layers(&[Layer::new("test", "not = valid = toml")]),
            Err(LocatorError::Config(_))
        ));
        assert!(matches!(
            ContextResolver::from_layers(&[Layer::new(
                "test",
                "[contexts.prod]\ntarget = \"ftp://loom.example\"\n",
            )]),
            Err(LocatorError::Config(_))
        ));
        assert!(matches!(
            ContextResolver::from_layers(&[Layer::new(
                "test",
                "[contexts.prod]\ntarget = \"https://loom.example\"\nconnect_timeout_ms = 0\n",
            )]),
            Err(LocatorError::Config(_))
        ));
    }

    #[test]
    fn duplicate_context_in_one_file_is_toml_error() {
        assert!(matches!(
            ContextResolver::from_layers(&[Layer::new(
                "test",
                "[contexts.prod]\ntarget = \"https://a\"\n[contexts.prod]\ntarget = \"https://b\"\n",
            )]),
            Err(LocatorError::Config(_))
        ));
    }

    #[test]
    fn load_reads_project_layer_and_skips_missing_files() {
        use std::fs;
        let base =
            std::env::temp_dir().join(format!("loom-locator-load-test-{}", std::process::id()));
        let project = base.join("project");
        fs::create_dir_all(project.join(".loom")).unwrap();
        fs::write(
            project.join(".loom").join("contexts.toml"),
            "[contexts.prod]\ntarget = \"https://proj.example\"\n",
        )
        .unwrap();
        let roots = LayerRoots {
            explicit_configs: Vec::new(),
            project: Some(project),
            user_home: Some(base.join("no-such-home")),
            system: Some(base.join("no-such-etc")),
        };
        let resolver = ContextResolver::load(&roots).unwrap();
        assert!(matches!(
            resolver.resolve_context("prod").unwrap(),
            Target::Remote(_)
        ));
        assert_eq!(
            resolver.resolve_str("other").unwrap(),
            Target::Local(PathBuf::from("other"))
        );
        fs::remove_dir_all(&base).ok();
    }
}
