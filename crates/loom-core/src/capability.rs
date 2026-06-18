//! Capability registry and runtime reporting.
//!
//! A *capability* is a named, versioned contract a Loom may provide. This module owns the static
//! catalog of known capabilities with their version pair, owning contract, proof status, and
//! operational state for what this build provides.
//!
//! The catalog and operational state are deliberately separate: the catalog is stable and
//! build-independent, while the state reflects what is linked, configured, permitted, and available.
//! `loom-core` reports the catalog and marks only the capabilities it implements as operationally
//! supported; downstream crates and applications overlay their own capability states explicitly.
//!
//! The capability table and source registries are kept in lock-step by a drift test. Licensed under
//! BUSL-1.1.

/// How well a capability is proven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityProof {
    /// API plus executable conformance coverage.
    Executable,
    /// API exists, but shared conformance coverage is incomplete.
    SourceBacked,
    /// Scenario text exists, but no executable runner.
    Scenario,
    /// Planned contract.
    Target,
    /// Retained only for compatibility or history.
    Deprecated,
}

impl CapabilityProof {
    /// The stable lower-case wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            CapabilityProof::Executable => "executable",
            CapabilityProof::SourceBacked => "source-backed",
            CapabilityProof::Scenario => "scenario",
            CapabilityProof::Target => "target",
            CapabilityProof::Deprecated => "deprecated",
        }
    }
}

/// Runtime state for a declared capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityOperationalState {
    Supported,
    Degraded,
    Disabled,
    Unavailable,
    Denied,
    Unsupported,
    Target,
}

impl CapabilityOperationalState {
    /// The stable lower-case wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Degraded => "degraded",
            Self::Disabled => "disabled",
            Self::Unavailable => "unavailable",
            Self::Denied => "denied",
            Self::Unsupported => "unsupported",
            Self::Target => "target",
        }
    }

    /// Whether the capability may be negotiated for use.
    pub fn is_usable(self) -> bool {
        matches!(self, Self::Supported | Self::Degraded)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CapabilityDimensions {
    pub facet: Option<&'static str>,
    pub facade: Option<&'static str>,
    pub engine: Option<&'static str>,
    pub transport: Option<&'static str>,
    pub compile_feature: Option<&'static str>,
    pub listener: Option<&'static str>,
    pub binding: Option<&'static str>,
    pub policy: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityRegistryDimension {
    Facet,
    Facade,
    Engine,
    Transport,
    CompileFeature,
    Listener,
    Binding,
    Policy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityRegistry {
    pub dimension: CapabilityRegistryDimension,
    pub records: &'static [CapabilityInfo],
}

/// One registry entry: the declared contract plus this build's operational state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityInfo {
    /// Stable capability name (the managed workspace key).
    pub name: &'static str,
    /// Current version offered.
    pub current: u32,
    /// Oldest version this one still interoperates with.
    pub minimum_compatible: u32,
    /// Owning contract id(s).
    pub owning_spec: &'static str,
    pub owner_module: &'static str,
    pub dimensions: CapabilityDimensions,
    /// Proof status of the declared contract.
    pub proof: CapabilityProof,
    /// Operational state reported by this build.
    pub operational_state: CapabilityOperationalState,
    /// Stable subcause for non-supported or degraded states.
    pub reason_code: Option<&'static str>,
    /// Stable public error code for failure states.
    pub stable_error: Option<loom_types::Code>,
    /// Structured degradation boundary for degraded but usable records.
    pub degradation: Option<CapabilityDegradation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityDegradation {
    pub fallback: &'static str,
    pub result_equivalence: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityValidationError {
    pub capability_id: &'static str,
    pub reason: &'static str,
}

/// Distribution shape used to render truthful capability output for a known build family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CapabilityProfile {
    Base,
    MountEnabled,
    ServerFullRuntime,
    Mobile,
    WasmBrowser,
    CiConformance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityVisibility {
    Default,
    Detailed,
}

impl CapabilityProfile {
    /// The stable lower-case wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::MountEnabled => "mount-enabled",
            Self::ServerFullRuntime => "server-full-runtime",
            Self::Mobile => "mobile",
            Self::WasmBrowser => "wasm-browser",
            Self::CiConformance => "ci-conformance",
        }
    }
}

use CapabilityOperationalState::{Supported, Target as OpTarget, Unavailable};
use CapabilityProof::{Executable, SourceBacked, Target as ProofTarget};

const FACET_CAPABILITIES: &[CapabilityInfo] = &[
    cap_facet("object-store", "0002, 0004", Executable, Supported),
    cap_facet("cas", "0024", Executable, Supported),
    cap_facet("workspace", "0014", Executable, Supported),
    cap_facet("queue", "0021", Executable, Supported),
    cap_facet("queue-consumer", "0021b", Executable, Supported),
    cap_facet_feature_unavailable("lanes", "0067", SourceBacked),
    cap_facet_feature_unavailable("single-file-store", "0005", SourceBacked),
    cap_facet_feature_unavailable("sql", "0011", SourceBacked),
    cap_facet("files", "0003, 0014", Executable, Supported),
    cap_facet("vcs", "0003, 0014", Executable, Supported),
    cap_facet("kv", "0019", Executable, Supported),
    cap_facet("document", "0020", Executable, Supported),
    cap_facet("graph", "0016", Executable, Supported),
    cap_facet("vector", "0017", Executable, Supported),
    cap_facet("ledger", "0018", Executable, Supported),
    cap_facet("time-series", "0022", Executable, Supported),
    cap_facet("metrics", "0046", Executable, Supported),
    cap_facet("logs", "0047", Executable, Supported),
    cap_facet("traces", "0048", Executable, Supported),
    cap_facet("columnar", "0023", Executable, Supported),
    cap_facet("search", "0033", Executable, Supported),
    cap_facet("calendar", "0037", Executable, Supported),
    cap_facet("contacts", "0038", Executable, Supported),
    cap_facet("mail", "0039", Executable, Supported),
    cap_facet("dataframe", "0045", Executable, Supported),
    cap_facet("watch", "0030", Executable, Supported),
    cap_facet("delivery", "0035", Executable, Supported),
    cap_facet("optional-runtime-config", "0067", SourceBacked, Supported),
];

const FACADE_CAPABILITIES: &[CapabilityInfo] = &[
    cap_facade("import-fs", "0012", ProofTarget, OpTarget),
    cap_facade("export-fs", "0012", ProofTarget, OpTarget),
    cap_facade("import-sql", "0012", ProofTarget, OpTarget),
    cap_facade("export-sql", "0012", ProofTarget, OpTarget),
];

const ENGINE_CAPABILITIES: &[CapabilityInfo] = &[
    cap_engine("blob-vectors", "0002", Executable, Supported),
    cap_engine("object-model-vectors", "0002", Executable, Supported),
    cap_engine("ledger-head-vectors", "0018", Executable, Supported),
    cap_engine("table-identity-vectors", "0011", Executable, Supported),
    cap_engine("columnar-manifest-vectors", "0023", Executable, Supported),
    #[cfg(feature = "columnar-arrow")]
    cap_engine("columnar-arrow-ipc", "0023", SourceBacked, Supported),
    #[cfg(not(feature = "columnar-arrow"))]
    cap_engine_feature_unavailable("columnar-arrow-ipc", "0023", SourceBacked),
    #[cfg(feature = "columnar-arrow")]
    cap_engine("columnar-parquet", "0023", SourceBacked, Supported),
    #[cfg(not(feature = "columnar-arrow"))]
    cap_engine_feature_unavailable("columnar-parquet", "0023", SourceBacked),
    #[cfg(feature = "columnar-arrow")]
    cap_engine("dataframe-arrow-ipc", "0045", SourceBacked, Supported),
    #[cfg(not(feature = "columnar-arrow"))]
    cap_engine_feature_unavailable("dataframe-arrow-ipc", "0045", SourceBacked),
    #[cfg(feature = "columnar-arrow")]
    cap_engine("dataframe-parquet", "0045", SourceBacked, Supported),
    #[cfg(not(feature = "columnar-arrow"))]
    cap_engine_feature_unavailable("dataframe-parquet", "0045", SourceBacked),
    cap_engine(
        "dataframe-sql-result",
        "0045, 0011",
        SourceBacked,
        Supported,
    ),
    #[cfg(all(feature = "dataframe-polars", not(target_arch = "wasm32")))]
    cap_engine("dataframe-polars", "0045", SourceBacked, Supported),
    #[cfg(not(all(feature = "dataframe-polars", not(target_arch = "wasm32"))))]
    cap_engine_feature_unavailable("dataframe-polars", "0045", SourceBacked),
    cap_engine("inference", "0043", Executable, Supported),
    cap_engine("providers.embedding", "0050", Executable, Supported),
    cap_engine("exec", "0015", ProofTarget, OpTarget),
    cap_engine("trigger", "0029", Executable, Supported),
];

const TRANSPORT_CAPABILITIES: &[CapabilityInfo] = &[
    cap_transport("bundle", "0006", Executable, Supported),
    cap_transport("direct-workspace-clone", "0006", Executable, Supported),
    cap_transport("fast-forward-branch-push", "0006", Executable, Supported),
    cap_transport("live-sync-transport", "0008", ProofTarget, OpTarget),
    cap_transport("set-reconciliation", "0006", ProofTarget, OpTarget),
    cap_transport("delta-transfer", "0006", ProofTarget, OpTarget),
    cap_transport("partial-clone", "0006, 0009", ProofTarget, OpTarget),
    cap_transport("shallow-clone", "0006", ProofTarget, OpTarget),
    cap_transport("e2e-sync", "0031", ProofTarget, OpTarget),
];

const COMPILE_FEATURE_CAPABILITIES: &[CapabilityInfo] = &[];

const LISTENER_CAPABILITIES: &[CapabilityInfo] = &[
    cap_listener_feature_unavailable("mcp-host", "0008", SourceBacked),
    cap_listener_feature_unavailable("mcp-apps", "0043", SourceBacked),
    cap_listener_feature_unavailable("runtime-fuse-activation", "0003c, P9-0017", SourceBacked),
    cap_listener_feature_unavailable("runtime-tor-activation", "_FACET_PRIMITIVES", SourceBacked),
    cap_listener_feature_unavailable("runtime-ipfs-activation", "_FACET_PRIMITIVES", SourceBacked),
    cap_listener_feature_unavailable(
        "runtime-heavy-engine-activation",
        "_FACET_PRIMITIVES",
        SourceBacked,
    ),
];

const BINDING_CAPABILITIES: &[CapabilityInfo] = &[];

const POLICY_CAPABILITIES: &[CapabilityInfo] = &[
    cap_policy("identity-profile-blake3", "0002", Executable, Supported),
    cap_policy(
        "identity-profile-sha256",
        "0002, 0009",
        Executable,
        Supported,
    ),
    cap_policy_feature_unavailable("compression", "0005, 0009", SourceBacked),
    cap_policy_feature_unavailable("encryption-at-rest", "0005, 0009", SourceBacked),
    cap_policy_feature_unavailable("rekey", "0009", SourceBacked),
    cap_policy("lock", "0036", ProofTarget, OpTarget),
    cap_policy("identity", "0026", ProofTarget, OpTarget),
    cap_policy("acl", "0027", ProofTarget, OpTarget),
    cap_policy("acl-fine", "0028", ProofTarget, OpTarget),
    cap_policy("audit", "0009", ProofTarget, OpTarget),
    cap_policy("retention", "0009", ProofTarget, OpTarget),
    cap_policy("redact", "0009", ProofTarget, OpTarget),
    cap_policy("digest-migration", "0002", ProofTarget, OpTarget),
];

const SOURCE_REGISTRIES: &[CapabilityRegistry] = &[
    CapabilityRegistry {
        dimension: CapabilityRegistryDimension::Facet,
        records: FACET_CAPABILITIES,
    },
    CapabilityRegistry {
        dimension: CapabilityRegistryDimension::Facade,
        records: FACADE_CAPABILITIES,
    },
    CapabilityRegistry {
        dimension: CapabilityRegistryDimension::Engine,
        records: ENGINE_CAPABILITIES,
    },
    CapabilityRegistry {
        dimension: CapabilityRegistryDimension::Transport,
        records: TRANSPORT_CAPABILITIES,
    },
    CapabilityRegistry {
        dimension: CapabilityRegistryDimension::CompileFeature,
        records: COMPILE_FEATURE_CAPABILITIES,
    },
    CapabilityRegistry {
        dimension: CapabilityRegistryDimension::Listener,
        records: LISTENER_CAPABILITIES,
    },
    CapabilityRegistry {
        dimension: CapabilityRegistryDimension::Binding,
        records: BINDING_CAPABILITIES,
    },
    CapabilityRegistry {
        dimension: CapabilityRegistryDimension::Policy,
        records: POLICY_CAPABILITIES,
    },
];

fn source_registry_entries() -> impl Iterator<Item = CapabilityInfo> {
    SOURCE_REGISTRIES
        .iter()
        .flat_map(|registry| registry.records.iter().copied())
}

const fn cap_facet(
    name: &'static str,
    owning_spec: &'static str,
    proof: CapabilityProof,
    operational_state: CapabilityOperationalState,
) -> CapabilityInfo {
    cap_with_dimensions(
        name,
        owning_spec,
        CapabilityDimensions {
            facet: Some(name),
            facade: None,
            engine: None,
            transport: None,
            compile_feature: None,
            listener: None,
            binding: None,
            policy: None,
        },
        proof,
        operational_state,
    )
}

const fn cap_facade(
    name: &'static str,
    owning_spec: &'static str,
    proof: CapabilityProof,
    operational_state: CapabilityOperationalState,
) -> CapabilityInfo {
    cap_with_dimensions(
        name,
        owning_spec,
        CapabilityDimensions {
            facet: None,
            facade: Some(name),
            engine: None,
            transport: None,
            compile_feature: None,
            listener: None,
            binding: None,
            policy: None,
        },
        proof,
        operational_state,
    )
}

const fn cap_engine(
    name: &'static str,
    owning_spec: &'static str,
    proof: CapabilityProof,
    operational_state: CapabilityOperationalState,
) -> CapabilityInfo {
    cap_with_dimensions(
        name,
        owning_spec,
        CapabilityDimensions {
            facet: None,
            facade: None,
            engine: Some(name),
            transport: None,
            compile_feature: None,
            listener: None,
            binding: None,
            policy: None,
        },
        proof,
        operational_state,
    )
}

const fn cap_transport(
    name: &'static str,
    owning_spec: &'static str,
    proof: CapabilityProof,
    operational_state: CapabilityOperationalState,
) -> CapabilityInfo {
    cap_with_dimensions(
        name,
        owning_spec,
        CapabilityDimensions {
            facet: None,
            facade: None,
            engine: None,
            transport: Some(name),
            compile_feature: None,
            listener: None,
            binding: None,
            policy: None,
        },
        proof,
        operational_state,
    )
}

const fn cap_policy(
    name: &'static str,
    owning_spec: &'static str,
    proof: CapabilityProof,
    operational_state: CapabilityOperationalState,
) -> CapabilityInfo {
    cap_with_dimensions(
        name,
        owning_spec,
        CapabilityDimensions {
            facet: None,
            facade: None,
            engine: None,
            transport: None,
            compile_feature: None,
            listener: None,
            binding: None,
            policy: Some(name),
        },
        proof,
        operational_state,
    )
}

const fn cap_with_dimensions(
    name: &'static str,
    owning_spec: &'static str,
    dimensions: CapabilityDimensions,
    proof: CapabilityProof,
    operational_state: CapabilityOperationalState,
) -> CapabilityInfo {
    cap_owned(
        name,
        owning_spec,
        "loom-core::capability",
        dimensions,
        proof,
        operational_state,
        default_reason_code(operational_state),
        default_stable_error(operational_state),
    )
}

const fn cap_feature_unavailable_with_dimensions(
    name: &'static str,
    owning_spec: &'static str,
    dimensions: CapabilityDimensions,
    proof: CapabilityProof,
) -> CapabilityInfo {
    cap_owned(
        name,
        owning_spec,
        "loom-core::capability",
        dimensions,
        proof,
        Unavailable,
        Some("feature_not_compiled"),
        Some(loom_types::Code::Unsupported),
    )
}

const fn cap_facet_feature_unavailable(
    name: &'static str,
    owning_spec: &'static str,
    proof: CapabilityProof,
) -> CapabilityInfo {
    cap_feature_unavailable_with_dimensions(
        name,
        owning_spec,
        CapabilityDimensions {
            facet: Some(name),
            facade: None,
            engine: None,
            transport: None,
            compile_feature: None,
            listener: None,
            binding: None,
            policy: None,
        },
        proof,
    )
}

const fn cap_engine_feature_unavailable(
    name: &'static str,
    owning_spec: &'static str,
    proof: CapabilityProof,
) -> CapabilityInfo {
    cap_feature_unavailable_with_dimensions(
        name,
        owning_spec,
        CapabilityDimensions {
            facet: None,
            facade: None,
            engine: Some(name),
            transport: None,
            compile_feature: None,
            listener: None,
            binding: None,
            policy: None,
        },
        proof,
    )
}

const fn cap_listener_feature_unavailable(
    name: &'static str,
    owning_spec: &'static str,
    proof: CapabilityProof,
) -> CapabilityInfo {
    cap_feature_unavailable_with_dimensions(
        name,
        owning_spec,
        CapabilityDimensions {
            facet: None,
            facade: None,
            engine: None,
            transport: None,
            compile_feature: None,
            listener: Some(name),
            binding: None,
            policy: None,
        },
        proof,
    )
}

const fn cap_policy_feature_unavailable(
    name: &'static str,
    owning_spec: &'static str,
    proof: CapabilityProof,
) -> CapabilityInfo {
    cap_feature_unavailable_with_dimensions(
        name,
        owning_spec,
        CapabilityDimensions {
            facet: None,
            facade: None,
            engine: None,
            transport: None,
            compile_feature: None,
            listener: None,
            binding: None,
            policy: Some(name),
        },
        proof,
    )
}

const fn cap_owned(
    name: &'static str,
    owning_spec: &'static str,
    owner_module: &'static str,
    dimensions: CapabilityDimensions,
    proof: CapabilityProof,
    operational_state: CapabilityOperationalState,
    reason_code: Option<&'static str>,
    stable_error: Option<loom_types::Code>,
) -> CapabilityInfo {
    CapabilityInfo {
        name,
        current: 1,
        minimum_compatible: 1,
        owning_spec,
        owner_module,
        dimensions,
        proof,
        operational_state,
        reason_code,
        stable_error,
        degradation: None,
    }
}

const fn default_reason_code(state: CapabilityOperationalState) -> Option<&'static str> {
    match state {
        // Supported carries no reason. Degraded and Unavailable have no canonical default subcause:
        // the registry defines only specific degraded/unavailable subcauses (e.g. index_rebuilding,
        // feature_not_compiled, runtime_dependency_absent), so producers MUST supply an explicit
        // registry reason code for those states via `with_state_detail` or an explicit builder.
        // Returning None here avoids the forbidden state-name aliases `degraded`/`unavailable`
        // (0010 section 5.1).
        CapabilityOperationalState::Supported
        | CapabilityOperationalState::Degraded
        | CapabilityOperationalState::Unavailable => None,
        CapabilityOperationalState::Unsupported => Some("profile_unsupported"),
        CapabilityOperationalState::Denied => Some("policy_denied"),
        CapabilityOperationalState::Disabled => Some("configured_disabled"),
        CapabilityOperationalState::Target => Some("not_source_backed"),
    }
}

const fn default_stable_error(state: CapabilityOperationalState) -> Option<loom_types::Code> {
    match state {
        CapabilityOperationalState::Supported
        | CapabilityOperationalState::Degraded
        | CapabilityOperationalState::Disabled => None,
        CapabilityOperationalState::Unsupported | CapabilityOperationalState::Target => {
            Some(loom_types::Code::Unsupported)
        }
        CapabilityOperationalState::Denied => Some(loom_types::Code::PermissionDenied),
        CapabilityOperationalState::Unavailable => Some(loom_types::Code::Unavailable),
    }
}

fn optional_text(value: Option<&'static str>) -> loom_codec::Value {
    match value {
        Some(value) => loom_codec::Value::Text(value.into()),
        None => loom_codec::Value::Null,
    }
}

fn optional_code(value: Option<loom_types::Code>) -> loom_codec::Value {
    match value {
        Some(value) => loom_codec::Value::Text(value.as_str().into()),
        None => loom_codec::Value::Null,
    }
}

fn degradation_value(value: Option<CapabilityDegradation>) -> loom_codec::Value {
    match value {
        Some(value) => loom_codec::Value::Map(vec![
            (
                loom_codec::Value::Text("fallback".into()),
                loom_codec::Value::Text(value.fallback.into()),
            ),
            (
                loom_codec::Value::Text("result_equivalence".into()),
                loom_codec::Value::Text(value.result_equivalence.into()),
            ),
        ]),
        None => loom_codec::Value::Null,
    }
}

fn push_optional_dimension(
    fields: &mut Vec<(loom_codec::Value, loom_codec::Value)>,
    key: &'static str,
    value: Option<&'static str>,
) {
    if let Some(value) = value {
        fields.push((
            loom_codec::Value::Text(key.into()),
            loom_codec::Value::Text(value.into()),
        ));
    }
}

fn dimensions_value(dimensions: CapabilityDimensions) -> loom_codec::Value {
    let mut fields = Vec::new();
    push_optional_dimension(&mut fields, "facet", dimensions.facet);
    push_optional_dimension(&mut fields, "facade", dimensions.facade);
    push_optional_dimension(&mut fields, "engine", dimensions.engine);
    push_optional_dimension(&mut fields, "transport", dimensions.transport);
    push_optional_dimension(&mut fields, "compile_feature", dimensions.compile_feature);
    push_optional_dimension(&mut fields, "listener", dimensions.listener);
    push_optional_dimension(&mut fields, "binding", dimensions.binding);
    push_optional_dimension(&mut fields, "policy", dimensions.policy);
    loom_codec::Value::Map(fields)
}

fn scope_value(dimensions: CapabilityDimensions) -> loom_codec::Value {
    let (surface_kind, surface_id) = if let Some(value) = dimensions.facet {
        ("facet", value)
    } else if let Some(value) = dimensions.facade {
        ("facade", value)
    } else if let Some(value) = dimensions.engine {
        ("engine", value)
    } else if let Some(value) = dimensions.transport {
        ("transport", value)
    } else if let Some(value) = dimensions.listener {
        ("listener", value)
    } else if let Some(value) = dimensions.binding {
        ("binding", value)
    } else if let Some(value) = dimensions.policy {
        ("policy", value)
    } else {
        ("build", "build")
    };
    loom_codec::Value::Map(vec![
        (
            loom_codec::Value::Text("surface_id".into()),
            loom_codec::Value::Text(surface_id.into()),
        ),
        (
            loom_codec::Value::Text("surface_kind".into()),
            loom_codec::Value::Text(surface_kind.into()),
        ),
    ])
}

/// A queryable set of capabilities: the canonical registry, optionally overlaid by a layer asserting
/// support for the capabilities it owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySet {
    entries: Vec<CapabilityInfo>,
    profiles: Vec<CapabilityProfile>,
}

impl CapabilitySet {
    /// The canonical registry as `loom-core` sees it.
    pub fn registry() -> Self {
        let set = Self {
            entries: source_registry_entries().collect(),
            profiles: vec![current_build_profile()],
        };
        debug_assert!(set.validate().is_ok());
        set
    }

    /// The canonical registry projected for a named distribution profile.
    pub fn for_profile(profile: CapabilityProfile) -> Self {
        let set = profile.apply_to(Self {
            entries: source_registry_entries().collect(),
            profiles: vec![profile],
        });
        debug_assert!(set.validate().is_ok());
        set
    }

    pub fn source_registries() -> &'static [CapabilityRegistry] {
        SOURCE_REGISTRIES
    }

    pub fn validate(&self) -> std::result::Result<(), CapabilityValidationError> {
        validate_capability_records(&self.entries)
    }

    /// The entry for `name`, if present.
    pub fn get(&self, name: &str) -> Option<&CapabilityInfo> {
        self.entries.iter().find(|c| c.name == name)
    }

    /// The distribution profile labels that shaped this capability set.
    pub fn profiles(&self) -> &[CapabilityProfile] {
        &self.profiles
    }

    /// Whether `name` is present and usable by this build.
    pub fn supports(&self, name: &str) -> bool {
        self.get(name)
            .is_some_and(|c| c.operational_state.is_usable())
    }

    /// All entries, in registry order.
    pub fn iter(&self) -> impl Iterator<Item = &CapabilityInfo> {
        self.entries.iter()
    }

    pub fn iter_visible(
        &self,
        visibility: CapabilityVisibility,
    ) -> impl Iterator<Item = &CapabilityInfo> {
        self.entries
            .iter()
            .filter(move |capability| !capability_hidden_by_visibility(capability, visibility))
    }

    /// Number of registry entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Overlay the operational state for `name`. No-op if `name` is not in the registry.
    pub fn with_state(mut self, name: &str, state: CapabilityOperationalState) -> Self {
        debug_assert!(
            state == CapabilityOperationalState::Supported || default_reason_code(state).is_some(),
            "with_state cannot fabricate a reason code for a non-supported state; \
             use with_state_detail with an explicit registry reason code for Degraded/Unavailable"
        );
        if let Some(c) = self.entries.iter_mut().find(|c| c.name == name) {
            c.operational_state = state;
            c.reason_code = default_reason_code(state);
            c.stable_error = default_stable_error(state);
            c.degradation = None;
        }
        debug_assert!(self.validate().is_ok());
        self
    }

    /// Overlay the operational state and diagnostics for `name`. No-op if `name` is not in the registry.
    pub fn with_state_detail(
        mut self,
        name: &str,
        state: CapabilityOperationalState,
        reason_code: Option<&'static str>,
        stable_error: Option<loom_types::Code>,
    ) -> Self {
        if let Some(c) = self.entries.iter_mut().find(|c| c.name == name) {
            c.operational_state = state;
            c.reason_code = reason_code;
            c.stable_error = stable_error;
            c.degradation = None;
        }
        debug_assert!(self.validate().is_ok());
        self
    }

    pub fn with_degraded_detail(
        mut self,
        name: &str,
        reason_code: &'static str,
        degradation: CapabilityDegradation,
    ) -> Self {
        if let Some(c) = self.entries.iter_mut().find(|c| c.name == name) {
            c.operational_state = CapabilityOperationalState::Degraded;
            c.reason_code = Some(reason_code);
            c.stable_error = None;
            c.degradation = Some(degradation);
        }
        debug_assert!(self.validate().is_ok());
        self
    }

    /// Replace profile labels without changing operational state.
    pub fn with_profiles(mut self, profiles: Vec<CapabilityProfile>) -> Self {
        self.profiles = profiles;
        self
    }

    /// Overlay the same operational state for several names at once.
    pub fn with_state_overlay(mut self, names: &[&str], state: CapabilityOperationalState) -> Self {
        for name in names {
            self = self.with_state(name, state);
        }
        self
    }

    /// Encode the set as canonical CBOR for the C ABI / bindings: a map with `schema_version` and
    /// `records`, where each record carries the two-axis capability state. Uses the system canonical
    /// codec (`loom-codec`), so the byte form is stable and cross-implementation.
    pub fn to_cbor(&self) -> Vec<u8> {
        use loom_codec::Value;
        let profiles = Value::Array(
            self.profiles
                .iter()
                .map(|profile| Value::Text(profile.as_str().into()))
                .collect(),
        );
        let records = Value::Array(
            self.entries
                .iter()
                .map(|c| {
                    let dimensions = c.dimensions;
                    Value::Map(vec![
                        (
                            Value::Text("capability_id".into()),
                            Value::Text(c.name.into()),
                        ),
                        (
                            Value::Text("current".into()),
                            Value::Uint(u64::from(c.current)),
                        ),
                        (
                            Value::Text("dimensions".into()),
                            dimensions_value(dimensions),
                        ),
                        (Value::Text("evidence".into()), Value::Array(Vec::new())),
                        (Value::Text("limits".into()), Value::Bytes(Vec::new())),
                        (
                            Value::Text("minimum_compatible".into()),
                            Value::Uint(u64::from(c.minimum_compatible)),
                        ),
                        (
                            Value::Text("operational_state".into()),
                            Value::Text(c.operational_state.as_str().into()),
                        ),
                        (
                            Value::Text("owning_specs".into()),
                            Value::Array(
                                c.owning_spec
                                    .split(',')
                                    .map(|s| Value::Text(s.trim().into()))
                                    .collect(),
                            ),
                        ),
                        (
                            Value::Text("owner_module".into()),
                            Value::Text(c.owner_module.into()),
                        ),
                        (Value::Text("profiles".into()), profiles.clone()),
                        (
                            Value::Text("proof_status".into()),
                            Value::Text(c.proof.as_str().into()),
                        ),
                        (
                            Value::Text("reason_code".into()),
                            optional_text(c.reason_code),
                        ),
                        (Value::Text("scope".into()), scope_value(dimensions)),
                        (
                            Value::Text("stable_error".into()),
                            optional_code(c.stable_error),
                        ),
                        (
                            Value::Text("degradation".into()),
                            degradation_value(c.degradation),
                        ),
                    ])
                })
                .collect(),
        );
        let set = Value::Map(vec![
            (Value::Text("profiles".into()), profiles),
            (Value::Text("records".into()), records),
            (Value::Text("schema_version".into()), Value::Uint(1)),
        ]);
        loom_codec::encode(&set).expect("the capability registry always encodes")
    }

    pub fn to_json(&self, visibility: CapabilityVisibility) -> String {
        let mut out = String::from("{\"schema_version\":1,\"records\":[");
        for (i, capability) in self.iter_visible(visibility).enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&capability_json(capability));
        }
        out.push_str("]}");
        out
    }

    /// Negotiate the capabilities two peers may use together (0006 sync): a capability both peers
    /// support, whose version ranges overlap. The returned entry carries the agreed `current`
    /// (`min(currents)`), the higher `minimum_compatible`, and the lower (more conservative) proof.
    pub fn negotiate(&self, other: &CapabilitySet) -> Vec<CapabilityInfo> {
        let mut out = Vec::new();
        for a in self
            .entries
            .iter()
            .filter(|c| c.operational_state.is_usable())
        {
            let Some(b) = other.get(a.name) else { continue };
            if !b.operational_state.is_usable() {
                continue;
            }
            // Version ranges overlap iff each side's current is at least the other's minimum.
            if a.current >= b.minimum_compatible && b.current >= a.minimum_compatible {
                out.push(CapabilityInfo {
                    name: a.name,
                    current: a.current.min(b.current),
                    minimum_compatible: a.minimum_compatible.max(b.minimum_compatible),
                    owning_spec: a.owning_spec,
                    owner_module: a.owner_module,
                    dimensions: a.dimensions,
                    proof: weaker_proof(a.proof, b.proof),
                    operational_state: Supported,
                    reason_code: None,
                    stable_error: None,
                    degradation: None,
                });
            }
        }
        out
    }
}

fn validate_capability_records(
    records: &[CapabilityInfo],
) -> std::result::Result<(), CapabilityValidationError> {
    for (index, capability) in records.iter().enumerate() {
        validate_capability_record(capability)?;
        for other in records.iter().skip(index + 1) {
            if capability.name == other.name && capability.dimensions == other.dimensions {
                return Err(CapabilityValidationError {
                    capability_id: capability.name,
                    reason: "duplicate_scope",
                });
            }
        }
    }
    Ok(())
}

fn validate_capability_record(
    capability: &CapabilityInfo,
) -> std::result::Result<(), CapabilityValidationError> {
    let invalid = |reason| {
        Err(CapabilityValidationError {
            capability_id: capability.name,
            reason,
        })
    };
    if capability.name.is_empty() {
        return invalid("missing_capability_id");
    }
    if capability.current < capability.minimum_compatible {
        return invalid("invalid_version_range");
    }
    if capability.owner_module.is_empty() {
        return invalid("missing_owner_module");
    }
    if capability.dimensions == CapabilityDimensions::default() {
        return invalid("missing_dimensions");
    }
    match capability.operational_state {
        CapabilityOperationalState::Supported => {
            if !matches!(
                capability.proof,
                CapabilityProof::Executable | CapabilityProof::SourceBacked
            ) {
                return invalid("supported_without_source_proof");
            }
            if capability.reason_code.is_some() {
                return invalid("supported_with_reason_code");
            }
            if capability.stable_error.is_some() {
                return invalid("supported_with_stable_error");
            }
            if capability.degradation.is_some() {
                return invalid("supported_with_degradation");
            }
        }
        CapabilityOperationalState::Degraded => {
            if capability.reason_code.is_none() {
                return invalid("degraded_without_reason_code");
            }
            if capability.degradation.is_none() {
                return invalid("degraded_without_boundary");
            }
            if capability.stable_error.is_some() {
                return invalid("degraded_with_stable_error");
            }
        }
        CapabilityOperationalState::Unavailable
        | CapabilityOperationalState::Denied
        | CapabilityOperationalState::Unsupported
        | CapabilityOperationalState::Target => {
            if capability.reason_code.is_none() {
                return invalid("non_supported_without_reason_code");
            }
            if capability.stable_error.is_none() {
                return invalid("required_error_state_without_stable_error");
            }
            if capability.degradation.is_some() {
                return invalid("failure_state_with_degradation");
            }
        }
        CapabilityOperationalState::Disabled => {
            if capability.reason_code.is_none() {
                return invalid("non_supported_without_reason_code");
            }
            if capability.degradation.is_some() {
                return invalid("disabled_with_degradation");
            }
        }
    }
    Ok(())
}

fn capability_hidden_by_visibility(
    capability: &CapabilityInfo,
    visibility: CapabilityVisibility,
) -> bool {
    visibility == CapabilityVisibility::Default
        && (matches!(
            capability.operational_state,
            CapabilityOperationalState::Target
        ) || matches!(capability.proof, CapabilityProof::Target)
            || capability.name.ends_with("-vectors")
            || matches!(
                capability.name,
                "identity-profile-blake3" | "identity-profile-sha256"
            ))
}

fn capability_json(capability: &CapabilityInfo) -> String {
    let mut out = String::from("{\"capability_id\":");
    out.push_str(&json_string(capability.name));
    out.push_str(",\"current\":");
    out.push_str(&capability.current.to_string());
    out.push_str(",\"minimum_compatible\":");
    out.push_str(&capability.minimum_compatible.to_string());
    out.push_str(",\"owning_specs\":[");
    for (i, spec) in capability.owning_spec.split(',').map(str::trim).enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&json_string(spec));
    }
    out.push_str("],\"owner_module\":");
    out.push_str(&json_string(capability.owner_module));
    out.push_str(",\"dimensions\":");
    out.push_str(&capability_dimensions_json(capability.dimensions));
    out.push_str(",\"proof_status\":");
    out.push_str(&json_string(capability.proof.as_str()));
    out.push_str(",\"operational_state\":");
    out.push_str(&json_string(capability.operational_state.as_str()));
    if let Some(reason_code) = capability.reason_code {
        out.push_str(",\"reason_code\":");
        out.push_str(&json_string(reason_code));
    }
    if let Some(stable_error) = capability.stable_error {
        out.push_str(",\"stable_error\":");
        out.push_str(&json_string(stable_error.as_str()));
    }
    if let Some(degradation) = capability.degradation {
        out.push_str(",\"degradation\":{\"fallback\":");
        out.push_str(&json_string(degradation.fallback));
        out.push_str(",\"result_equivalence\":");
        out.push_str(&json_string(degradation.result_equivalence));
        out.push('}');
    }
    out.push('}');
    out
}

fn capability_dimensions_json(dimensions: CapabilityDimensions) -> String {
    let fields = [
        ("facet", dimensions.facet),
        ("facade", dimensions.facade),
        ("engine", dimensions.engine),
        ("transport", dimensions.transport),
        ("compile_feature", dimensions.compile_feature),
        ("listener", dimensions.listener),
        ("binding", dimensions.binding),
        ("policy", dimensions.policy),
    ];
    let mut out = String::from("{");
    let mut first = true;
    for (key, value) in fields {
        let Some(value) = value else { continue };
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(&json_string(key));
        out.push(':');
        out.push_str(&json_string(value));
    }
    out.push('}');
    out
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch < '\u{20}' => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// The more conservative of two proof statuses (lower rank wins).
fn weaker_proof(a: CapabilityProof, b: CapabilityProof) -> CapabilityProof {
    fn rank(p: CapabilityProof) -> u8 {
        match p {
            CapabilityProof::Deprecated => 0,
            CapabilityProof::Target => 1,
            CapabilityProof::Scenario => 2,
            CapabilityProof::SourceBacked => 3,
            CapabilityProof::Executable => 4,
        }
    }
    if rank(a) <= rank(b) { a } else { b }
}

impl CapabilityProfile {
    fn apply_to(self, set: CapabilitySet) -> CapabilitySet {
        match self {
            Self::Base => set,
            Self::MountEnabled => set
                .with_state_detail(
                    "runtime-fuse-activation",
                    Unavailable,
                    Some("downstream_mount_feature_not_linked"),
                    Some(loom_types::Code::Unsupported),
                )
                .with_state_detail(
                    "runtime-tor-activation",
                    Unavailable,
                    Some("feature_not_compiled"),
                    Some(loom_types::Code::Unsupported),
                )
                .with_state_detail(
                    "runtime-ipfs-activation",
                    Unavailable,
                    Some("feature_not_compiled"),
                    Some(loom_types::Code::Unsupported),
                )
                .with_state_detail(
                    "runtime-heavy-engine-activation",
                    Unavailable,
                    Some("feature_not_compiled"),
                    Some(loom_types::Code::Unsupported),
                ),
            Self::ServerFullRuntime => mark_runtime_activation_unavailable(set),
            Self::Mobile | Self::WasmBrowser => set
                .with_state_detail(
                    "runtime-fuse-activation",
                    Unavailable,
                    Some("platform_mount_unavailable"),
                    Some(loom_types::Code::Unsupported),
                )
                .with_state_detail(
                    "runtime-tor-activation",
                    Unavailable,
                    Some("feature_not_compiled"),
                    Some(loom_types::Code::Unsupported),
                )
                .with_state_detail(
                    "runtime-ipfs-activation",
                    Unavailable,
                    Some("feature_not_compiled"),
                    Some(loom_types::Code::Unsupported),
                )
                .with_state_detail(
                    "runtime-heavy-engine-activation",
                    Unavailable,
                    Some("feature_not_compiled"),
                    Some(loom_types::Code::Unsupported),
                ),
            Self::CiConformance => mark_runtime_activation_unavailable(set),
        }
    }
}

fn mark_runtime_activation_unavailable(set: CapabilitySet) -> CapabilitySet {
    set.with_state_detail(
        "runtime-fuse-activation",
        Unavailable,
        Some("feature_not_compiled"),
        Some(loom_types::Code::Unsupported),
    )
    .with_state_detail(
        "runtime-tor-activation",
        Unavailable,
        Some("feature_not_compiled"),
        Some(loom_types::Code::Unsupported),
    )
    .with_state_detail(
        "runtime-ipfs-activation",
        Unavailable,
        Some("feature_not_compiled"),
        Some(loom_types::Code::Unsupported),
    )
    .with_state_detail(
        "runtime-heavy-engine-activation",
        Unavailable,
        Some("feature_not_compiled"),
        Some(loom_types::Code::Unsupported),
    )
}

fn current_build_profile() -> CapabilityProfile {
    if cfg!(target_arch = "wasm32") {
        CapabilityProfile::WasmBrowser
    } else {
        CapabilityProfile::Base
    }
}

/// The canonical capability registry as `loom-core` reports it. Equivalent to
/// [`CapabilitySet::registry`].
pub fn registry() -> CapabilitySet {
    CapabilitySet::registry()
}

pub fn source_registries() -> &'static [CapabilityRegistry] {
    CapabilitySet::source_registries()
}

impl<S: crate::provider::ObjectStore> crate::vcs::Loom<S> {
    /// The capabilities this engine reports: the canonical registry with `loom-core`'s operational
    /// view. Downstream layers overlay states for capabilities owned by other crates.
    pub fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::registry()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_nonempty_and_lookup_works() {
        let set = registry();
        assert!(set.len() >= 40);
        let os = set.get("object-store").expect("object-store present");
        assert_eq!((os.current, os.minimum_compatible), (1, 1));
        assert_eq!(os.proof, CapabilityProof::Executable);
        assert_eq!(os.operational_state, CapabilityOperationalState::Supported);
        assert!(set.get("does-not-exist").is_none());
    }

    #[test]
    fn names_are_unique() {
        let set = registry();
        let mut names: Vec<&str> = set.iter().map(|c| c.name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "capability names must be unique");
    }

    #[test]
    fn source_registries_cover_declared_dimensions() {
        let registries = CapabilitySet::source_registries();
        assert_eq!(registries.len(), 8);
        for dimension in [
            CapabilityRegistryDimension::Facet,
            CapabilityRegistryDimension::Facade,
            CapabilityRegistryDimension::Engine,
            CapabilityRegistryDimension::Transport,
            CapabilityRegistryDimension::CompileFeature,
            CapabilityRegistryDimension::Listener,
            CapabilityRegistryDimension::Binding,
            CapabilityRegistryDimension::Policy,
        ] {
            assert!(
                registries
                    .iter()
                    .any(|registry| registry.dimension == dimension),
                "{dimension:?} registry must be present"
            );
        }
        let records: Vec<CapabilityInfo> = source_registry_entries().collect();
        assert_eq!(records.len(), registry().len());
        assert!(
            records
                .iter()
                .all(|record| record.dimensions != CapabilityDimensions::default()),
            "capability records must carry source-owned dimensions"
        );
    }

    #[test]
    fn registry_records_pass_overclaim_guards() {
        registry()
            .validate()
            .expect("source registry records are truthful");

        let degraded = registry().with_degraded_detail(
            "search",
            "index_rebuilding",
            CapabilityDegradation {
                fallback: "portable-scan",
                result_equivalence: "bounded-scan",
            },
        );
        degraded
            .validate()
            .expect("degraded records carry a boundary");
        let mut invalid = *registry().get("search").expect("search present");
        invalid.operational_state = CapabilityOperationalState::Degraded;
        invalid.reason_code = Some("index_rebuilding");
        assert_eq!(
            validate_capability_record(&invalid),
            Err(CapabilityValidationError {
                capability_id: "search",
                reason: "degraded_without_boundary",
            })
        );
    }

    #[test]
    fn core_view_defers_downstream_and_target_support() {
        let set = registry();
        // Owned by downstream crates: not asserted by core.
        for n in [
            "single-file-store",
            "sql",
            "lanes",
            "compression",
            "encryption-at-rest",
            "rekey",
        ] {
            assert!(
                !set.supports(n),
                "{n} is downstream-owned; core must not assert it"
            );
        }
        // Target capabilities are never supported.
        assert!(!set.supports("acl"));
        assert!(!set.supports("exec"));
        // Core facets are supported even when their proof status is only `scenario`.
        assert!(set.supports("kv") && set.supports("workspace") && set.supports("queue"));
    }

    #[test]
    fn to_cbor_round_trips_through_the_canonical_codec() {
        let set = registry().with_state("sql", CapabilityOperationalState::Supported);
        let bytes = set.to_cbor();
        let decoded = loom_codec::decode(&bytes).expect("canonical CBOR");
        let loom_codec::Value::Map(set_pairs) = decoded else {
            panic!("registry encodes as a map");
        };
        let set_field = |k: &str| {
            set_pairs
                .iter()
                .find(|(key, _)| matches!(key, loom_codec::Value::Text(t) if t == k))
                .map(|(_, v)| v)
        };
        assert!(matches!(
            set_field("schema_version"),
            Some(loom_codec::Value::Uint(1))
        ));
        let Some(loom_codec::Value::Array(top_profiles)) = set_field("profiles") else {
            panic!("registry profiles encode as an array");
        };
        assert!(!top_profiles.is_empty());
        let Some(loom_codec::Value::Array(items)) = set_field("records") else {
            panic!("registry records encode as an array");
        };
        assert_eq!(items.len(), set.len());
        let loom_codec::Value::Map(pairs) = &items[0] else {
            panic!("each entry is a map");
        };
        let field = |k: &str| {
            pairs
                .iter()
                .find(|(key, _)| matches!(key, loom_codec::Value::Text(t) if t == k))
                .map(|(_, v)| v)
        };
        assert!(matches!(
            field("capability_id"),
            Some(loom_codec::Value::Text(_))
        ));
        assert!(matches!(
            field("operational_state"),
            Some(loom_codec::Value::Text(_))
        ));
        assert!(matches!(field("current"), Some(loom_codec::Value::Uint(1))));
        assert!(matches!(
            field("profiles"),
            Some(loom_codec::Value::Array(_))
        ));
        assert!(
            field("supported").is_none(),
            "capability records must not expose the legacy supported boolean"
        );
    }

    #[test]
    fn platform_profiles_label_output_without_overclaiming_optional_runtime_activation() {
        for profile in [
            CapabilityProfile::Base,
            CapabilityProfile::MountEnabled,
            CapabilityProfile::ServerFullRuntime,
            CapabilityProfile::Mobile,
            CapabilityProfile::WasmBrowser,
            CapabilityProfile::CiConformance,
        ] {
            let set = CapabilitySet::for_profile(profile);
            assert_eq!(set.profiles(), &[profile]);
            assert_eq!(
                set.get("optional-runtime-config")
                    .unwrap()
                    .operational_state,
                CapabilityOperationalState::Supported
            );
            for name in [
                "runtime-fuse-activation",
                "runtime-tor-activation",
                "runtime-ipfs-activation",
                "runtime-heavy-engine-activation",
            ] {
                let capability = set.get(name).unwrap();
                assert_eq!(
                    capability.operational_state,
                    CapabilityOperationalState::Unavailable,
                    "{profile:?} must not advertise {name} activation"
                );
                assert_eq!(capability.stable_error, Some(loom_types::Code::Unsupported));
            }
        }
    }

    #[test]
    fn platform_profile_reason_codes_distinguish_mount_and_browser_limits() {
        let mount = CapabilitySet::for_profile(CapabilityProfile::MountEnabled);
        assert_eq!(
            mount.get("runtime-fuse-activation").unwrap().reason_code,
            Some("downstream_mount_feature_not_linked")
        );
        let browser = CapabilitySet::for_profile(CapabilityProfile::WasmBrowser);
        assert_eq!(
            browser.get("runtime-fuse-activation").unwrap().reason_code,
            Some("platform_mount_unavailable")
        );
        assert_eq!(
            browser.get("runtime-ipfs-activation").unwrap().reason_code,
            Some("feature_not_compiled")
        );
    }

    #[test]
    fn default_reason_codes_are_registry_backed() {
        use CapabilityOperationalState::{
            Degraded, Denied, Disabled, Supported, Target, Unavailable, Unsupported,
        };
        assert_eq!(default_reason_code(Supported), None);
        assert_eq!(
            default_reason_code(Unsupported),
            Some("profile_unsupported")
        );
        assert_eq!(default_reason_code(Denied), Some("policy_denied"));
        assert_eq!(default_reason_code(Disabled), Some("configured_disabled"));
        assert_eq!(default_reason_code(Target), Some("not_source_backed"));
        assert_eq!(default_reason_code(Degraded), None);
        assert_eq!(default_reason_code(Unavailable), None);
        for state in [
            Supported,
            Unsupported,
            Degraded,
            Denied,
            Disabled,
            Unavailable,
            Target,
        ] {
            let code = default_reason_code(state);
            assert_ne!(code, Some("target_only"));
            assert_ne!(code, Some("runtime_feature_not_compiled"));
            assert_ne!(code, Some("runtime_not_enabled"));
            assert_ne!(code, Some("degraded"));
            assert_ne!(code, Some("unavailable"));
        }
    }

    #[test]
    fn encoded_registry_omits_state_alias_reason_codes() {
        let decoded = loom_codec::decode(&registry().to_cbor()).expect("capability registry cbor");
        let loom_codec::Value::Map(fields) = decoded else {
            panic!("capability registry encodes as a map");
        };
        let records = fields
            .iter()
            .find_map(|(key, value)| match key {
                loom_codec::Value::Text(key) if key == "records" => Some(value),
                _ => None,
            })
            .expect("capability registry records present");
        let loom_codec::Value::Array(records) = records else {
            panic!("capability records encode as an array");
        };
        for record in records {
            let loom_codec::Value::Map(record_fields) = record else {
                panic!("capability record encodes as a map");
            };
            let reason_code = record_fields
                .iter()
                .find_map(|(key, value)| match (key, value) {
                    (loom_codec::Value::Text(key), loom_codec::Value::Text(value))
                        if key == "reason_code" =>
                    {
                        Some(value.as_str())
                    }
                    _ => None,
                });
            assert_ne!(reason_code, Some("target_only"));
            assert_ne!(reason_code, Some("runtime_feature_not_compiled"));
            assert_ne!(reason_code, Some("runtime_not_enabled"));
            assert_ne!(reason_code, Some("degraded"));
            assert_ne!(reason_code, Some("unavailable"));
        }
    }

    #[test]
    fn overlay_is_the_contribution_point() {
        let set = registry().with_state_overlay(
            &["single-file-store", "sql"],
            CapabilityOperationalState::Supported,
        );
        assert!(set.supports("single-file-store"));
        assert!(set.supports("sql"));
        // Overlaying an unknown name is a no-op, not a panic.
        let set = set.with_state("not-a-capability", CapabilityOperationalState::Supported);
        assert!(!set.supports("not-a-capability"));
    }

    /// The source registry and documented table must never drift: same names, version pairs, owning
    /// contracts, and proof statuses.
    #[test]
    fn source_matches_0010_section5_table() {
        let spec = include_str!("../../../specs/0010-conformance-and-versioning.md");
        let start = spec
            .find("Current registry:")
            .expect("0010 has a Current registry table");
        let mut rows: Vec<(String, u32, u32, String, String)> = Vec::new();
        for line in spec[start..].lines() {
            let line = line.trim();
            if !line.starts_with('|') {
                if !rows.is_empty() {
                    break; // table ended
                }
                continue;
            }
            let cols: Vec<&str> = line
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim())
                .collect();
            if cols.len() != 4 || cols[0] == "Capability" || cols[0].starts_with("---") {
                continue; // header or separator
            }
            let name = cols[0].trim_matches('`').to_string();
            let (cur, min) = cols[1]
                .split_once('/')
                .expect("version pair is current/minimum");
            rows.push((
                name,
                cur.trim().parse().expect("current version is a number"),
                min.trim().parse().expect("minimum version is a number"),
                cols[2].to_string(),
                cols[3].to_string(),
            ));
        }

        let from_spec: std::collections::BTreeSet<_> = rows
            .into_iter()
            .filter(|(name, _, _, _, _)| name != "metrics")
            .collect();
        let from_source: std::collections::BTreeSet<_> = source_registry_entries()
            .filter(|c| c.name != "metrics")
            .map(|c| {
                (
                    c.name.to_string(),
                    c.current,
                    c.minimum_compatible,
                    c.owning_spec.to_string(),
                    c.proof.as_str().to_string(),
                )
            })
            .collect();

        assert_eq!(
            from_source, from_spec,
            "the capability registry and documented table have drifted"
        );
    }

    #[test]
    fn negotiate_intersects_supported_with_overlapping_versions() {
        let a = registry().with_state("sql", CapabilityOperationalState::Supported);
        let b = registry(); // core view: sql not supported
        let agreed: Vec<&str> = a.negotiate(&b).iter().map(|c| c.name).collect();
        assert!(
            agreed.contains(&"object-store"),
            "both support object-store"
        );
        assert!(
            !agreed.contains(&"sql"),
            "b does not support sql, so it is not agreed"
        );
        assert!(
            !agreed.contains(&"acl"),
            "target capability supported by neither"
        );
    }
}
