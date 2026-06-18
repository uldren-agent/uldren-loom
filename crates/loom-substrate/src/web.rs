use std::collections::BTreeSet;

use loom_codec::Value;
use loom_types::{Code, Digest, LoomError, Result, WorkspaceId};

use crate::{Fields, codec_error, string_array, validate_text};

pub const APP_ID: &str = "web";
pub const LISTENER_SCHEMA: &str = "loom.studio.web.listener.v1";
pub const ROUTE_TABLE_SCHEMA: &str = "loom.studio.web.route-table.v1";
pub const HOOK_CHAIN_SCHEMA: &str = "loom.studio.web.hook-chain.v1";
pub const PROFILE_CONTROL_PREFIX: &str = "profile/web/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WebMethod {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
}

impl WebMethod {
    const fn tag(self) -> u64 {
        match self {
            Self::Get => 0,
            Self::Head => 1,
            Self::Post => 2,
            Self::Put => 3,
            Self::Patch => 4,
            Self::Delete => 5,
            Self::Options => 6,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Get),
            1 => Ok(Self::Head),
            2 => Ok(Self::Post),
            3 => Ok(Self::Put),
            4 => Ok(Self::Patch),
            5 => Ok(Self::Delete),
            6 => Ok(Self::Options),
            other => Err(LoomError::corrupt(format!(
                "unknown web method tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebProtocol {
    Http,
    Https,
}

impl WebProtocol {
    const fn tag(self) -> u64 {
        match self {
            Self::Http => 0,
            Self::Https => 1,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Http),
            1 => Ok(Self::Https),
            other => Err(LoomError::corrupt(format!(
                "unknown web protocol tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebMountRef {
    Branch(String),
    Tag(String),
    Commit(Digest),
}

impl WebMountRef {
    fn to_value(&self) -> Value {
        match self {
            Self::Branch(value) => Value::Array(vec![Value::Uint(0), Value::Text(value.clone())]),
            Self::Tag(value) => Value::Array(vec![Value::Uint(1), Value::Text(value.clone())]),
            Self::Commit(value) => {
                Value::Array(vec![Value::Uint(2), Value::Text(value.to_string())])
            }
        }
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "web mount ref")?;
        let reference = match fields.uint("mount ref tag")? {
            0 => Self::Branch(fields.text("branch")?),
            1 => Self::Tag(fields.text("tag")?),
            2 => Self::Commit(fields.digest("commit")?),
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown web mount ref tag {other}"
                )));
            }
        };
        fields.end("web mount ref")?;
        reference.validate()?;
        Ok(reference)
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Branch(value) => validate_text("web branch", value),
            Self::Tag(value) => validate_text("web tag", value),
            Self::Commit(_) => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebRouteMode {
    StaticFile,
    Presentation,
    Program,
    Redirect,
    ReverseProxy,
    Error,
}

impl WebRouteMode {
    const fn tag(self) -> u64 {
        match self {
            Self::StaticFile => 0,
            Self::Presentation => 1,
            Self::Program => 2,
            Self::Redirect => 3,
            Self::ReverseProxy => 4,
            Self::Error => 5,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::StaticFile),
            1 => Ok(Self::Presentation),
            2 => Ok(Self::Program),
            3 => Ok(Self::Redirect),
            4 => Ok(Self::ReverseProxy),
            5 => Ok(Self::Error),
            other => Err(LoomError::corrupt(format!(
                "unknown web route mode tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebRoute {
    pub route_id: String,
    pub methods: Vec<WebMethod>,
    pub host_pattern: Option<String>,
    pub path_prefix: String,
    pub workspace: Option<WorkspaceId>,
    pub root_path: String,
    pub reference: Option<WebMountRef>,
    pub mode: WebRouteMode,
    pub target: Option<String>,
    pub auth_policy: Option<String>,
    pub cache_policy: Option<String>,
    pub timeout_ms: u64,
}

impl WebRoute {
    pub fn new(
        route_id: impl Into<String>,
        methods: Vec<WebMethod>,
        host_pattern: Option<String>,
        path_prefix: impl AsRef<str>,
        root_path: impl AsRef<str>,
        mode: WebRouteMode,
    ) -> Result<Self> {
        let route = Self {
            route_id: route_id.into(),
            methods,
            host_pattern,
            path_prefix: normalize_web_path(path_prefix.as_ref())?,
            workspace: None,
            root_path: normalize_web_path(root_path.as_ref())?,
            reference: None,
            mode,
            target: None,
            auth_policy: None,
            cache_policy: None,
            timeout_ms: 30_000,
        };
        route.validate()?;
        Ok(route)
    }

    pub fn materialized_path(&self, request_path: &str) -> Result<String> {
        let request_path = normalize_web_path(request_path)?;
        if !path_matches_prefix(&request_path, &self.path_prefix) {
            return Err(LoomError::not_found("web route path does not match"));
        }
        let suffix = request_path
            .strip_prefix(&self.path_prefix)
            .unwrap_or("")
            .trim_start_matches('/');
        join_web_paths(&self.root_path, suffix)
    }

    fn matches(&self, host: Option<&str>, method: WebMethod, path: &str) -> bool {
        self.methods.contains(&method)
            && self.host_matches(host)
            && path_matches_prefix(path, &self.path_prefix)
    }

    fn host_matches(&self, host: Option<&str>) -> bool {
        match (&self.host_pattern, host) {
            (None, _) => true,
            (Some(pattern), Some(host)) => host.eq_ignore_ascii_case(pattern),
            (Some(_), None) => false,
        }
    }

    fn validate(&self) -> Result<()> {
        validate_text("web route_id", &self.route_id)?;
        if self.methods.is_empty() {
            return Err(LoomError::invalid("web route must declare methods"));
        }
        let mut methods = BTreeSet::new();
        for method in &self.methods {
            if !methods.insert(*method) {
                return Err(LoomError::invalid("web route methods must be unique"));
            }
        }
        if let Some(pattern) = &self.host_pattern {
            validate_text("web host_pattern", pattern)?;
        }
        if let Some(reference) = &self.reference {
            reference.validate()?;
        }
        if let Some(target) = &self.target {
            validate_text("web route target", target)?;
        }
        validate_optional_text("web auth_policy", self.auth_policy.as_deref())?;
        validate_optional_text("web cache_policy", self.cache_policy.as_deref())?;
        normalize_web_path(&self.path_prefix)?;
        normalize_web_path(&self.root_path)?;
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.route_id.clone()),
            Value::Array(
                self.methods
                    .iter()
                    .map(|method| Value::Uint(method.tag()))
                    .collect(),
            ),
            optional_text_value(self.host_pattern.as_deref()),
            Value::Text(self.path_prefix.clone()),
            optional_id_value(self.workspace),
            Value::Text(self.root_path.clone()),
            optional_ref_value(self.reference.as_ref()),
            Value::Uint(self.mode.tag()),
            optional_text_value(self.target.as_deref()),
            optional_text_value(self.auth_policy.as_deref()),
            optional_text_value(self.cache_policy.as_deref()),
            Value::Uint(self.timeout_ms),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "web route")?;
        let route = Self {
            route_id: fields.text("route_id")?,
            methods: method_list(fields.next("methods")?)?,
            host_pattern: fields.optional_text("host_pattern")?,
            path_prefix: normalize_web_path(&fields.text("path_prefix")?)?,
            workspace: fields.optional_id("workspace")?,
            root_path: normalize_web_path(&fields.text("root_path")?)?,
            reference: optional_ref(fields.next("reference")?)?,
            mode: WebRouteMode::from_tag(fields.uint("mode")?)?,
            target: fields.optional_text("target")?,
            auth_policy: fields.optional_text("auth_policy")?,
            cache_policy: fields.optional_text("cache_policy")?,
            timeout_ms: fields.uint("timeout_ms")?,
        };
        fields.end("web route")?;
        route.validate()?;
        Ok(route)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebRouteTable {
    pub routes: Vec<WebRoute>,
}

impl WebRouteTable {
    pub fn new(routes: Vec<WebRoute>) -> Result<Self> {
        let table = Self { routes };
        table.validate()?;
        Ok(table)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn resolve(
        &self,
        host: Option<&str>,
        method: WebMethod,
        path: &str,
    ) -> Result<WebResolvedRoute> {
        let path = normalize_web_path(path)?;
        let route = self
            .routes
            .iter()
            .filter(|route| route.matches(host, method, &path))
            .max_by_key(|route| route.path_prefix.len())
            .ok_or_else(|| LoomError::not_found("web route not found"))?;
        Ok(WebResolvedRoute {
            route_id: route.route_id.clone(),
            workspace: route.workspace,
            reference: route.reference.clone(),
            mode: route.mode,
            materialized_path: route.materialized_path(&path)?,
        })
    }

    fn validate(&self) -> Result<()> {
        let mut route_ids = BTreeSet::new();
        for route in &self.routes {
            route.validate()?;
            if !route_ids.insert(route.route_id.clone()) {
                return Err(LoomError::new(Code::AlreadyExists, "web route id exists"));
            }
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(ROUTE_TABLE_SCHEMA.to_string()),
            Value::Array(vec![Value::Array(
                self.routes.iter().map(WebRoute::to_value).collect(),
            )]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "web route table")?;
        outer.expect_text(ROUTE_TABLE_SCHEMA)?;
        let mut fields = Fields::array(outer.next("web route table fields")?, "web route table")?;
        outer.end("web route table")?;
        let routes = route_list(fields.next("routes")?)?;
        fields.end("web route table")?;
        Self::new(routes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebResolvedRoute {
    pub route_id: String,
    pub workspace: Option<WorkspaceId>,
    pub reference: Option<WebMountRef>,
    pub mode: WebRouteMode,
    pub materialized_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebListener {
    pub listener_id: String,
    pub bind_address: String,
    pub port: u16,
    pub protocol: WebProtocol,
    pub tls_profile: Option<String>,
    pub default_workspace: WorkspaceId,
    pub default_ref: Option<WebMountRef>,
    pub root_path: String,
    pub routes: WebRouteTable,
    pub hooks: WebHookChain,
    pub principal_policy: Option<String>,
    pub cache_policy: Option<String>,
    pub log_policy: Option<String>,
}

impl WebListener {
    pub fn new(
        listener_id: impl Into<String>,
        bind_address: impl Into<String>,
        port: u16,
        protocol: WebProtocol,
        default_workspace: WorkspaceId,
        root_path: impl AsRef<str>,
    ) -> Result<Self> {
        let listener = Self {
            listener_id: listener_id.into(),
            bind_address: bind_address.into(),
            port,
            protocol,
            tls_profile: None,
            default_workspace,
            default_ref: None,
            root_path: normalize_web_path(root_path.as_ref())?,
            routes: WebRouteTable::new(Vec::new())?,
            hooks: WebHookChain::new(Vec::new())?,
            principal_policy: None,
            cache_policy: None,
            log_policy: None,
        };
        listener.validate()?;
        Ok(listener)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn route(
        &self,
        host: Option<&str>,
        method: WebMethod,
        path: &str,
    ) -> Result<WebResolvedRoute> {
        match self.routes.resolve(host, method, path) {
            Ok(mut resolved) => {
                if resolved.workspace.is_none() {
                    resolved.workspace = Some(self.default_workspace);
                }
                Ok(resolved)
            }
            Err(err) if err.code == Code::NotFound => {
                let path = normalize_web_path(path)?;
                Ok(WebResolvedRoute {
                    route_id: "default".to_string(),
                    workspace: Some(self.default_workspace),
                    reference: self.default_ref.clone(),
                    mode: WebRouteMode::StaticFile,
                    materialized_path: resolve_default_file(&self.root_path, &path)?,
                })
            }
            Err(err) => Err(err),
        }
    }

    fn validate(&self) -> Result<()> {
        validate_text("web listener_id", &self.listener_id)?;
        validate_text("web bind_address", &self.bind_address)?;
        if self.port == 0 {
            return Err(LoomError::invalid("web listener port must be non-zero"));
        }
        if self.protocol == WebProtocol::Https && self.tls_profile.is_none() {
            return Err(LoomError::invalid(
                "https web listener requires a tls profile",
            ));
        }
        validate_optional_text("web tls_profile", self.tls_profile.as_deref())?;
        if let Some(reference) = &self.default_ref {
            reference.validate()?;
        }
        normalize_web_path(&self.root_path)?;
        self.routes.validate()?;
        self.hooks.validate()?;
        validate_optional_text("web principal_policy", self.principal_policy.as_deref())?;
        validate_optional_text("web cache_policy", self.cache_policy.as_deref())?;
        validate_optional_text("web log_policy", self.log_policy.as_deref())?;
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(LISTENER_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.listener_id.clone()),
                Value::Text(self.bind_address.clone()),
                Value::Uint(u64::from(self.port)),
                Value::Uint(self.protocol.tag()),
                optional_text_value(self.tls_profile.as_deref()),
                Value::Text(self.default_workspace.to_string()),
                optional_ref_value(self.default_ref.as_ref()),
                Value::Text(self.root_path.clone()),
                self.routes.to_value(),
                self.hooks.to_value(),
                optional_text_value(self.principal_policy.as_deref()),
                optional_text_value(self.cache_policy.as_deref()),
                optional_text_value(self.log_policy.as_deref()),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "web listener")?;
        outer.expect_text(LISTENER_SCHEMA)?;
        let mut fields = Fields::array(outer.next("web listener fields")?, "web listener")?;
        outer.end("web listener")?;
        let listener_id = fields.text("listener_id")?;
        let bind_address = fields.text("bind_address")?;
        let port = u16::try_from(fields.uint("port")?)
            .map_err(|_| LoomError::corrupt("web listener port exceeds u16"))?;
        let listener = Self {
            listener_id,
            bind_address,
            port,
            protocol: WebProtocol::from_tag(fields.uint("protocol")?)?,
            tls_profile: fields.optional_text("tls_profile")?,
            default_workspace: fields.id("default_workspace")?,
            default_ref: optional_ref(fields.next("default_ref")?)?,
            root_path: normalize_web_path(&fields.text("root_path")?)?,
            routes: WebRouteTable::from_value(fields.next("routes")?)?,
            hooks: WebHookChain::from_value(fields.next("hooks")?)?,
            principal_policy: fields.optional_text("principal_policy")?,
            cache_policy: fields.optional_text("cache_policy")?,
            log_policy: fields.optional_text("log_policy")?,
        };
        fields.end("web listener")?;
        listener.validate()?;
        Ok(listener)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WebHookPhase {
    Accept,
    Tls,
    EarlyRequest,
    Normalize,
    PreRoute,
    Route,
    Authenticate,
    Authorize,
    VariantSelect,
    PreHandler,
    Handler,
    PostHandler,
    Error,
    Log,
    Delivery,
}

impl WebHookPhase {
    const fn tag(self) -> u64 {
        match self {
            Self::Accept => 0,
            Self::Tls => 1,
            Self::EarlyRequest => 2,
            Self::Normalize => 3,
            Self::PreRoute => 4,
            Self::Route => 5,
            Self::Authenticate => 6,
            Self::Authorize => 7,
            Self::VariantSelect => 8,
            Self::PreHandler => 9,
            Self::Handler => 10,
            Self::PostHandler => 11,
            Self::Error => 12,
            Self::Log => 13,
            Self::Delivery => 14,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Accept),
            1 => Ok(Self::Tls),
            2 => Ok(Self::EarlyRequest),
            3 => Ok(Self::Normalize),
            4 => Ok(Self::PreRoute),
            5 => Ok(Self::Route),
            6 => Ok(Self::Authenticate),
            7 => Ok(Self::Authorize),
            8 => Ok(Self::VariantSelect),
            9 => Ok(Self::PreHandler),
            10 => Ok(Self::Handler),
            11 => Ok(Self::PostHandler),
            12 => Ok(Self::Error),
            13 => Ok(Self::Log),
            14 => Ok(Self::Delivery),
            other => Err(LoomError::corrupt(format!(
                "unknown web hook phase tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebHookFailurePolicy {
    FailClosed,
    Continue,
    Redirect,
}

impl WebHookFailurePolicy {
    const fn tag(self) -> u64 {
        match self {
            Self::FailClosed => 0,
            Self::Continue => 1,
            Self::Redirect => 2,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::FailClosed),
            1 => Ok(Self::Continue),
            2 => Ok(Self::Redirect),
            other => Err(LoomError::corrupt(format!(
                "unknown web hook failure policy tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebHook {
    pub hook_id: String,
    pub phase: WebHookPhase,
    pub order: u64,
    pub program: Digest,
    pub grants: Vec<String>,
    pub match_prefix: Option<String>,
    pub timeout_ms: u64,
    pub failure_policy: WebHookFailurePolicy,
}

impl WebHook {
    pub fn new(
        hook_id: impl Into<String>,
        phase: WebHookPhase,
        order: u64,
        program: Digest,
        grants: Vec<String>,
    ) -> Result<Self> {
        let hook = Self {
            hook_id: hook_id.into(),
            phase,
            order,
            program,
            grants,
            match_prefix: None,
            timeout_ms: 5_000,
            failure_policy: WebHookFailurePolicy::FailClosed,
        };
        hook.validate()?;
        Ok(hook)
    }

    fn validate(&self) -> Result<()> {
        validate_text("web hook_id", &self.hook_id)?;
        for grant in &self.grants {
            validate_text("web hook grant", grant)?;
        }
        if let Some(prefix) = &self.match_prefix {
            normalize_web_path(prefix)?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.hook_id.clone()),
            Value::Uint(self.phase.tag()),
            Value::Uint(self.order),
            Value::Text(self.program.to_string()),
            string_array(&self.grants),
            optional_text_value(self.match_prefix.as_deref()),
            Value::Uint(self.timeout_ms),
            Value::Uint(self.failure_policy.tag()),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "web hook")?;
        let hook = Self {
            hook_id: fields.text("hook_id")?,
            phase: WebHookPhase::from_tag(fields.uint("phase")?)?,
            order: fields.uint("order")?,
            program: fields.digest("program")?,
            grants: fields.string_array("grants")?,
            match_prefix: fields.optional_text("match_prefix")?,
            timeout_ms: fields.uint("timeout_ms")?,
            failure_policy: WebHookFailurePolicy::from_tag(fields.uint("failure_policy")?)?,
        };
        fields.end("web hook")?;
        hook.validate()?;
        Ok(hook)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebHookChain {
    pub hooks: Vec<WebHook>,
}

impl WebHookChain {
    pub fn new(mut hooks: Vec<WebHook>) -> Result<Self> {
        hooks.sort_by_key(|hook| (hook.phase, hook.order, hook.hook_id.clone()));
        let chain = Self { hooks };
        chain.validate()?;
        Ok(chain)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        let mut hook_ids = BTreeSet::new();
        let mut slots = BTreeSet::new();
        for hook in &self.hooks {
            hook.validate()?;
            if !hook_ids.insert(hook.hook_id.clone()) {
                return Err(LoomError::new(Code::AlreadyExists, "web hook id exists"));
            }
            if !slots.insert((hook.phase, hook.order)) {
                return Err(LoomError::new(
                    Code::AlreadyExists,
                    "web hook phase order exists",
                ));
            }
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(HOOK_CHAIN_SCHEMA.to_string()),
            Value::Array(vec![Value::Array(
                self.hooks.iter().map(WebHook::to_value).collect(),
            )]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "web hook chain")?;
        outer.expect_text(HOOK_CHAIN_SCHEMA)?;
        let mut fields = Fields::array(outer.next("web hook chain fields")?, "web hook chain")?;
        outer.end("web hook chain")?;
        let hooks = hook_list(fields.next("hooks")?)?;
        fields.end("web hook chain")?;
        Self::new(hooks)
    }
}

pub fn web_profile_listener_key(listener_id: &str) -> Result<Vec<u8>> {
    validate_text("listener_id", listener_id)?;
    Ok(format!("{PROFILE_CONTROL_PREFIX}/listeners/{listener_id}").into_bytes())
}

pub fn normalize_web_path(path: &str) -> Result<String> {
    if path.is_empty() {
        return Err(LoomError::invalid("web path must not be empty"));
    }
    if path.contains('\0') {
        return Err(LoomError::invalid(
            "web path contains a forbidden character",
        ));
    }
    let mut out = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            return Err(LoomError::invalid("web path traversal is forbidden"));
        }
        out.push(segment);
    }
    if out.is_empty() {
        Ok("/".to_string())
    } else {
        Ok(format!("/{}", out.join("/")))
    }
}

fn resolve_default_file(root_path: &str, request_path: &str) -> Result<String> {
    let request_path = normalize_web_path(request_path)?;
    let suffix = if request_path == "/" {
        "index.html".to_string()
    } else if request_path.ends_with('/') {
        format!("{}index.html", request_path.trim_start_matches('/'))
    } else {
        request_path.trim_start_matches('/').to_string()
    };
    join_web_paths(root_path, &suffix)
}

fn join_web_paths(root: &str, suffix: &str) -> Result<String> {
    let root = normalize_web_path(root)?;
    let suffix = suffix.trim_matches('/');
    if suffix.is_empty() {
        Ok(root)
    } else if root == "/" {
        normalize_web_path(&format!("/{suffix}"))
    } else {
        normalize_web_path(&format!("{root}/{suffix}"))
    }
}

fn path_matches_prefix(path: &str, prefix: &str) -> bool {
    path == prefix
        || prefix == "/"
        || path
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn validate_optional_text(name: &str, value: Option<&str>) -> Result<()> {
    if let Some(value) = value {
        validate_text(name, value)?;
    }
    Ok(())
}

fn optional_text_value(value: Option<&str>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), Value::Text(value.to_string())]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn optional_id_value(value: Option<WorkspaceId>) -> Value {
    optional_text_value(value.map(|id| id.to_string()).as_deref())
}

fn optional_ref_value(value: Option<&WebMountRef>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), value.to_value()]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn optional_ref(value: Value) -> Result<Option<WebMountRef>> {
    match optional_value(value, "web mount ref")? {
        Some(value) => WebMountRef::from_value(value).map(Some),
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

fn method_list(value: Value) -> Result<Vec<WebMethod>> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                Value::Uint(tag) => WebMethod::from_tag(tag),
                _ => Err(LoomError::corrupt("web method must be uint")),
            })
            .collect(),
        _ => Err(LoomError::corrupt("web methods must be an array")),
    }
}

fn route_list(value: Value) -> Result<Vec<WebRoute>> {
    match value {
        Value::Array(items) => items.into_iter().map(WebRoute::from_value).collect(),
        _ => Err(LoomError::corrupt("web routes must be an array")),
    }
}

fn hook_list(value: Value) -> Result<Vec<WebHook>> {
    match value {
        Value::Array(items) => items.into_iter().map(WebHook::from_value).collect(),
        _ => Err(LoomError::corrupt("web hooks must be an array")),
    }
}

#[cfg(test)]
mod tests {
    use loom_types::Algo;

    use super::*;

    fn ns(byte: u8) -> WorkspaceId {
        WorkspaceId::v4_from_bytes([byte; 16])
    }

    fn digest(byte: u8) -> Digest {
        Digest::hash(Algo::Blake3, &[byte])
    }

    #[test]
    fn listener_round_trips_and_routes_default_static_files() {
        let mut listener = WebListener::new(
            "public",
            "127.0.0.1",
            8080,
            WebProtocol::Http,
            ns(1),
            "/public",
        )
        .unwrap();
        listener.default_ref = Some(WebMountRef::Branch("main".to_string()));
        let decoded = WebListener::decode(&listener.encode().unwrap()).unwrap();
        assert_eq!(decoded, listener);

        let resolved = decoded.route(None, WebMethod::Get, "/docs/").unwrap();
        assert_eq!(resolved.route_id, "default");
        assert_eq!(resolved.workspace, Some(ns(1)));
        assert_eq!(resolved.materialized_path, "/public/docs");
        assert_eq!(
            web_profile_listener_key("public").unwrap(),
            b"profile/web/v1/listeners/public".to_vec()
        );
    }

    #[test]
    fn route_table_uses_host_method_and_longest_prefix() {
        let mut public_route = WebRoute::new(
            "public",
            vec![WebMethod::Get],
            None,
            "/",
            "/public",
            WebRouteMode::StaticFile,
        )
        .unwrap();
        public_route.workspace = Some(ns(1));
        let mut api_route = WebRoute::new(
            "api",
            vec![WebMethod::Get, WebMethod::Post],
            None,
            "/api",
            "/server/api",
            WebRouteMode::Program,
        )
        .unwrap();
        api_route.host_pattern = Some("example.com".to_string());
        api_route.workspace = Some(ns(2));
        api_route.target = Some("program/api".to_string());
        let table = WebRouteTable::new(vec![public_route, api_route]).unwrap();

        let resolved = table
            .resolve(Some("EXAMPLE.com"), WebMethod::Post, "/api/items")
            .unwrap();
        assert_eq!(resolved.route_id, "api");
        assert_eq!(resolved.workspace, Some(ns(2)));
        assert_eq!(resolved.materialized_path, "/server/api/items");
        assert_eq!(
            WebRouteTable::decode(&table.encode().unwrap()).unwrap(),
            table
        );
    }

    #[test]
    fn route_paths_reject_traversal_and_https_requires_tls() {
        assert_eq!(
            normalize_web_path("/public/../secret").unwrap_err().code,
            Code::InvalidArgument
        );
        let err =
            WebListener::new("secure", "0.0.0.0", 443, WebProtocol::Https, ns(1), "/").unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
    }

    #[test]
    fn hook_chain_orders_and_round_trips() {
        let first = WebHook::new(
            "auth",
            WebHookPhase::Authorize,
            10,
            digest(1),
            vec!["web.read".to_string()],
        )
        .unwrap();
        let second = WebHook::new(
            "normalize",
            WebHookPhase::Normalize,
            1,
            digest(2),
            vec!["web.route".to_string()],
        )
        .unwrap();
        let chain = WebHookChain::new(vec![first.clone(), second.clone()]).unwrap();

        assert_eq!(chain.hooks[0], second);
        assert_eq!(chain.hooks[1], first);
        assert_eq!(
            WebHookChain::decode(&chain.encode().unwrap()).unwrap(),
            chain
        );
        assert_eq!(
            WebHookChain::new(vec![first.clone(), first])
                .unwrap_err()
                .code,
            Code::AlreadyExists
        );
    }
}
