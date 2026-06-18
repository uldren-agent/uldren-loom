//! Workspaces - named buckets with typed facets inside one Loom.
//!
//! Each workspace has a stable [`WorkspaceId`], a mutable `name`, a set of [`FacetKind`] values, and
//! its own ref store (branches, tags, `HEAD`) over the Loom's shared content-addressed object store.
//! The [`Registry`] maps ids to records and enforces:
//!
//! - name uniqueness across the Loom;
//! - default naming: a write with no name targets `"Default"`, created on first write, while a read of
//!   a missing workspace is `NOT_FOUND`;
//! - isolation: history operations touch exactly one workspace; spanning two raises
//!   `CROSS_WORKSPACE`;
//! - facets describe the data and projections available inside the workspace.
//!
//! The registry is mutable metadata, not a mergeable tree, so it is one in-memory structure rather
//! than a content-addressed one. Workspace ids are UUIDv4: the 16 random bytes are supplied by the
//! caller so this crate stays dependency-free and `wasm32`-clean; [`WorkspaceId::v4_from_bytes`]
//! stamps the version/variant nibbles.

use crate::cbor::{self, Value};
use crate::digest::Digest;
use crate::error::{Code, LoomError, Result};
use crate::provider::CompressionHint;
pub use loom_types::workspace::{AclDomain, FacetKind, WorkspaceId};
use std::collections::{BTreeMap, BTreeSet};

/// The name of the first workspace created when the caller supplies none.
pub const DEFAULT_NAME: &str = "Default";
/// The single branch of every workspace's initial line of history.
pub const DEFAULT_BRANCH: &str = "main";
/// Reserved root for Loom metadata and facet projections inside a workspace tree.
pub const LOOM_RESERVED_DIR: &str = ".loom";
/// Reserved root for non-files facets inside a workspace tree.
pub const FACETS_RESERVED_DIR: &str = ".loom/facets";

/// Whether a normalized, root-relative working-tree `path` is the reserved `.loom` directory or lies
/// within it. Loom metadata and facet storage live here; the public `fs` facade rejects user writes to
/// this subtree, while typed facet facades write through privileged internal paths.
/// Reads and directory listings of the subtree are allowed.
pub fn is_reserved_path(path: &str) -> bool {
    let r = LOOM_RESERVED_DIR;
    path == r || (path.len() > r.len() && path.starts_with(r) && path.as_bytes()[r.len()] == b'/')
}

/// The default compression hint for a workspace's facets. This is a write policy only; it never
/// affects object identity.
pub fn default_compression_for_facets(facets: &[FacetKind]) -> CompressionHint {
    if facets.iter().any(|facet| {
        matches!(
            facet,
            FacetKind::Files
                | FacetKind::Document
                | FacetKind::Sql
                | FacetKind::Program
                | FacetKind::Columnar
                | FacetKind::Calendar
                | FacetKind::Contacts
                | FacetKind::Mail
                | FacetKind::Search
                | FacetKind::Dataframe
        )
    }) {
        CompressionHint::Small
    } else if facets.iter().any(|facet| {
        matches!(
            facet,
            FacetKind::TimeSeries | FacetKind::Queue | FacetKind::Ledger | FacetKind::Graph
        )
    }) {
        CompressionHint::Fast
    } else {
        CompressionHint::None
    }
}

/// Canonical root path for a non-files facet inside a workspace tree.
pub fn facet_root(facet: FacetKind) -> String {
    format!("{FACETS_RESERVED_DIR}/{}", facet.as_str())
}

/// Canonical path under a non-files facet inside a workspace tree.
pub fn facet_path(facet: FacetKind, relative: &str) -> String {
    let root = facet_root(facet);
    let relative = relative.trim_start_matches('/');
    if relative.is_empty() {
        root
    } else {
        format!("{root}/{relative}")
    }
}

/// How a caller selects a workspace. Always explicit: there is no ambient "current workspace".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsSelector {
    /// By stable id.
    Id(WorkspaceId),
    /// By workspace name.
    Name(String),
    /// By workspace name with a required facet.
    Typed {
        /// The facet expected inside the workspace.
        ty: FacetKind,
        /// The workspace name.
        name: String,
    },
    /// The `"Default"` workspace with a required facet.
    Default(FacetKind),
    /// The `"Default"` workspace with a required facet.
    DefaultFacet(FacetKind),
}

/// A read-only view of a workspace for listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceInfo {
    /// Stable id.
    pub id: WorkspaceId,
    /// Current name.
    pub name: String,
    /// Facets present in this workspace.
    pub facets: Vec<FacetKind>,
    /// Tip of the current `HEAD` branch, if any commits exist yet.
    pub head: Option<Digest>,
}

/// One workspace's mutable record: its name, facets, and ref store.
#[derive(Debug, Clone)]
struct Record {
    name: String,
    facets: BTreeSet<FacetKind>,
    /// Branch name -> tip commit digest.
    branches: BTreeMap<String, Digest>,
    /// Tag name -> target digest.
    tags: BTreeMap<String, Digest>,
    /// The attached branch name `HEAD` points at; `HEAD` is always attached.
    head: String,
}

/// The workspace registry: `id -> record`, with a `name -> id` uniqueness index.
#[derive(Debug, Default)]
pub struct Registry {
    by_id: BTreeMap<WorkspaceId, Record>,
    by_name: BTreeMap<String, WorkspaceId>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a workspace named `name` (or `"Default"` when `None`) with the caller-supplied `id`.
    /// Errors `ALREADY_EXISTS` if the name or id is already in use.
    pub fn create_workspace(&mut self, name: Option<&str>, id: WorkspaceId) -> Result<WorkspaceId> {
        let name = name.unwrap_or(DEFAULT_NAME).to_string();
        if self.by_name.contains_key(&name) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                format!("workspace {name:?} already exists"),
            ));
        }
        if self.by_id.contains_key(&id) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                format!("workspace id {id} already in use"),
            ));
        }
        self.by_id.insert(
            id,
            Record {
                name: name.clone(),
                facets: BTreeSet::new(),
                branches: BTreeMap::new(),
                tags: BTreeMap::new(),
                head: DEFAULT_BRANCH.to_string(),
            },
        );
        self.by_name.insert(name, id);
        Ok(id)
    }

    /// Create a workspace and mark `facet` present.
    pub fn create(
        &mut self,
        facet: FacetKind,
        name: Option<&str>,
        id: WorkspaceId,
    ) -> Result<WorkspaceId> {
        let id = self.create_workspace(name, id)?;
        self.add_facet(id, facet)?;
        Ok(id)
    }

    fn resolve(&self, sel: &WsSelector) -> Result<WorkspaceId> {
        match sel {
            WsSelector::Id(id) => {
                if self.by_id.contains_key(id) {
                    Ok(*id)
                } else {
                    Err(LoomError::not_found(format!("workspace {id}")))
                }
            }
            WsSelector::Name(name) => self
                .by_name
                .get(name)
                .copied()
                .ok_or_else(|| LoomError::not_found(format!("workspace {name:?}"))),
            WsSelector::Typed { ty, name } => {
                let id = self
                    .by_name
                    .get(name)
                    .copied()
                    .ok_or_else(|| LoomError::not_found(format!("workspace {name:?}")))?;
                if self.has_facet(id, *ty)? {
                    Ok(id)
                } else {
                    Err(LoomError::not_found(format!(
                        "facet {} in workspace {name:?}",
                        ty.as_str()
                    )))
                }
            }
            WsSelector::Default(facet) | WsSelector::DefaultFacet(facet) => {
                let id = self
                    .by_name
                    .get(DEFAULT_NAME)
                    .copied()
                    .ok_or_else(|| LoomError::not_found("default workspace"))?;
                if self.has_facet(id, *facet)? {
                    Ok(id)
                } else {
                    Err(LoomError::not_found(format!(
                        "facet {} in default workspace",
                        facet.as_str()
                    )))
                }
            }
        }
    }

    /// Resolve a selector for a **read**. A missing workspace is `NOT_FOUND`; reads never create.
    pub fn open(&self, sel: &WsSelector) -> Result<WorkspaceId> {
        self.resolve(sel)
    }

    /// Resolve a selector for a **write**, creating the workspace on first use. A `Default` selector
    /// creates the `"Default"` workspace; a `Typed` selector creates or updates a workspace by name
    /// with the required facet; an unknown `Id` is `NOT_FOUND` (a workspace cannot be created by id
    /// alone). `new_id` is used only in the create case.
    pub fn ensure_for_write(
        &mut self,
        sel: &WsSelector,
        new_id: WorkspaceId,
    ) -> Result<WorkspaceId> {
        match self.resolve(sel) {
            Ok(id) => Ok(id),
            Err(e) if e.code == Code::NotFound => match sel {
                WsSelector::Default(facet) | WsSelector::DefaultFacet(facet) => {
                    if let Some(id) = self.by_name.get(DEFAULT_NAME).copied() {
                        self.add_facet(id, *facet)?;
                        Ok(id)
                    } else {
                        self.create(*facet, None, new_id)
                    }
                }
                WsSelector::Name(name) => self.create_workspace(Some(name), new_id),
                WsSelector::Typed { ty, name } => {
                    if let Some(id) = self.by_name.get(name).copied() {
                        self.add_facet(id, *ty)?;
                        Ok(id)
                    } else {
                        self.create(*ty, Some(name), new_id)
                    }
                }
                WsSelector::Id(_) => Err(e),
            },
            Err(e) => Err(e),
        }
    }

    /// List workspaces, optionally filtered by facet.
    pub fn list(&self, facet: Option<FacetKind>) -> Vec<WorkspaceInfo> {
        self.by_id
            .iter()
            .filter(|(_, r)| match facet {
                None => true,
                Some(f) => r.facets.contains(&f),
            })
            .map(|(id, r)| WorkspaceInfo {
                id: *id,
                name: r.name.clone(),
                facets: r.facets.iter().copied().collect(),
                head: r.branches.get(&r.head).copied(),
            })
            .collect()
    }

    /// Rename a workspace, keeping names unique. Renaming to the current name is a no-op.
    pub fn rename(&mut self, id: WorkspaceId, new_name: &str) -> Result<()> {
        let key = new_name.to_string();
        if let Some(existing) = self.by_name.get(&key) {
            return if *existing == id {
                Ok(())
            } else {
                Err(LoomError::new(
                    Code::AlreadyExists,
                    format!("workspace {new_name:?} already exists"),
                ))
            };
        }
        let old_name = self.record(&id)?.name.clone();
        self.by_name.remove(&old_name);
        self.by_name.insert(key, id);
        self.record_mut(&id)?.name = new_name.to_string();
        Ok(())
    }

    /// Delete a workspace: drop its refs and registry entry. The objects its history reached are
    /// reclaimed by GC over all remaining roots; deletion itself is O(1).
    pub fn delete(&mut self, id: WorkspaceId) -> Result<()> {
        let record = self
            .by_id
            .remove(&id)
            .ok_or_else(|| LoomError::not_found(format!("workspace {id}")))?;
        self.by_name.remove(&record.name);
        Ok(())
    }

    fn record(&self, id: &WorkspaceId) -> Result<&Record> {
        self.by_id
            .get(id)
            .ok_or_else(|| LoomError::not_found(format!("workspace {id}")))
    }

    fn record_mut(&mut self, id: &WorkspaceId) -> Result<&mut Record> {
        self.by_id
            .get_mut(id)
            .ok_or_else(|| LoomError::not_found(format!("workspace {id}")))
    }

    /// The workspace's facets.
    pub fn facets(&self, id: WorkspaceId) -> Result<Vec<FacetKind>> {
        Ok(self.record(&id)?.facets.iter().copied().collect())
    }

    /// Whether `facet` is present in the workspace.
    pub fn has_facet(&self, id: WorkspaceId, facet: FacetKind) -> Result<bool> {
        Ok(self.record(&id)?.facets.contains(&facet))
    }

    /// Whether the workspace permits branch and merge operations.
    pub fn supports_branching(&self, id: WorkspaceId) -> Result<bool> {
        self.record(&id)?;
        Ok(true)
    }

    /// Mark `facet` present in the workspace.
    pub fn add_facet(&mut self, id: WorkspaceId, facet: FacetKind) -> Result<()> {
        self.record_mut(&id)?.facets.insert(facet);
        Ok(())
    }

    /// The workspace's current name.
    pub fn name(&self, id: WorkspaceId) -> Result<String> {
        Ok(self.record(&id)?.name.clone())
    }

    /// The branch name `HEAD` is attached to.
    pub fn head_branch(&self, id: WorkspaceId) -> Result<String> {
        Ok(self.record(&id)?.head.clone())
    }

    /// Point `HEAD` at `branch`. The branch must already exist (or be the default name, which may be
    /// unborn); otherwise `NOT_FOUND`.
    pub fn set_head(&mut self, id: WorkspaceId, branch: &str) -> Result<()> {
        validate_ref_name("branch", branch)?;
        let record = self.record_mut(&id)?;
        if branch != DEFAULT_BRANCH && !record.branches.contains_key(branch) {
            return Err(LoomError::not_found(format!("branch {branch:?}")));
        }
        record.head = branch.to_string();
        Ok(())
    }

    /// The tip commit of `branch`, or `None` if the branch is unborn / absent.
    pub fn branch_tip(&self, id: WorkspaceId, branch: &str) -> Result<Option<Digest>> {
        validate_ref_name("branch", branch)?;
        Ok(self.record(&id)?.branches.get(branch).copied())
    }

    /// All branch names, sorted.
    pub fn branch_list(&self, id: WorkspaceId) -> Result<Vec<String>> {
        Ok(self.record(&id)?.branches.keys().cloned().collect())
    }

    /// Create a branch at `at`. `ALREADY_EXISTS` if the branch is present.
    pub fn branch_create(&mut self, id: WorkspaceId, name: &str, at: Digest) -> Result<()> {
        validate_ref_name("branch", name)?;
        let record = self.record_mut(&id)?;
        if record.branches.contains_key(name) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                format!("branch {name:?} already exists"),
            ));
        }
        record.branches.insert(name.to_string(), at);
        Ok(())
    }

    /// Compare-and-swap a branch tip. `expected` must equal the current tip (`None` for an unborn
    /// branch); a mismatch is `CAS_MISMATCH` so a losing writer can retry.
    pub fn update_branch(
        &mut self,
        id: WorkspaceId,
        branch: &str,
        expected: Option<Digest>,
        new: Digest,
    ) -> Result<()> {
        validate_ref_name("branch", branch)?;
        let record = self.record_mut(&id)?;
        let current = record.branches.get(branch).copied();
        if current != expected {
            return Err(LoomError::cas_mismatch(format!(
                "branch {branch:?}: expected {expected:?}, found {current:?}"
            )));
        }
        record.branches.insert(branch.to_string(), new);
        Ok(())
    }

    /// Delete branch `name`. `NOT_FOUND` if it does not exist. The current `HEAD` branch is rejected.
    pub fn branch_delete(&mut self, id: WorkspaceId, name: &str) -> Result<()> {
        validate_ref_name("branch", name)?;
        let record = self.record_mut(&id)?;
        if record.head == name {
            return Err(LoomError::invalid("cannot delete the current branch"));
        }
        if record.branches.remove(name).is_none() {
            return Err(LoomError::not_found(format!("branch {name:?}")));
        }
        Ok(())
    }

    /// Create a tag pointing at `target`. `ALREADY_EXISTS` if the tag name is taken.
    pub fn tag_create(&mut self, id: WorkspaceId, name: &str, target: Digest) -> Result<()> {
        validate_ref_name("tag", name)?;
        let record = self.record_mut(&id)?;
        if record.tags.contains_key(name) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                format!("tag {name:?} already exists"),
            ));
        }
        record.tags.insert(name.to_string(), target);
        Ok(())
    }

    /// All tag names, sorted.
    pub fn tag_list(&self, id: WorkspaceId) -> Result<Vec<String>> {
        Ok(self.record(&id)?.tags.keys().cloned().collect())
    }

    /// The target of a tag, or `None` if the tag is absent.
    pub fn tag_target(&self, id: WorkspaceId, name: &str) -> Result<Option<Digest>> {
        validate_ref_name("tag", name)?;
        Ok(self.record(&id)?.tags.get(name).copied())
    }

    /// Delete tag `name`. `NOT_FOUND` if it does not exist (git-faithful; not a silent no-op).
    pub fn tag_delete(&mut self, id: WorkspaceId, name: &str) -> Result<()> {
        validate_ref_name("tag", name)?;
        if self.record_mut(&id)?.tags.remove(name).is_none() {
            return Err(LoomError::not_found(format!("tag {name:?}")));
        }
        Ok(())
    }

    /// Rename tag `old` to `new`, preserving its target. `NOT_FOUND` if `old` is absent;
    /// `ALREADY_EXISTS` if `new` is taken.
    pub fn tag_rename(&mut self, id: WorkspaceId, old: &str, new: &str) -> Result<()> {
        validate_ref_name("tag", old)?;
        validate_ref_name("tag", new)?;
        let record = self.record_mut(&id)?;
        if !record.tags.contains_key(old) {
            return Err(LoomError::not_found(format!("tag {old:?}")));
        }
        if record.tags.contains_key(new) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                format!("tag {new:?} already exists"),
            ));
        }
        let target = record.tags.remove(old).expect("tag present");
        record.tags.insert(new.to_string(), target);
        Ok(())
    }

    /// Require two workspace ids to be identical; otherwise `CROSS_WORKSPACE`. History operations
    /// (merge/rebase/diff/cherry-pick) call this on their operands.
    pub fn require_same(a: WorkspaceId, b: WorkspaceId) -> Result<()> {
        if a == b {
            Ok(())
        } else {
            Err(LoomError::cross_workspace(format!(
                "operation spanned workspaces {a} and {b}"
            )))
        }
    }

    /// Serialize the whole registry (every workspace's id/name/facets/HEAD + branches + tags) to
    /// canonical bytes. The registry is mutable metadata, not a mergeable tree, so it is one
    /// deterministic state blob rather than a content-addressed structure. Records iterate in id
    /// order (`BTreeMap`), so the encoding is stable.
    pub fn encode(&self) -> Vec<u8> {
        let records = self
            .by_id
            .iter()
            .map(|(id, rec)| {
                Value::Array(vec![
                    Value::Bytes(id.as_bytes().to_vec()),
                    Value::Uint(2),
                    Value::Text(rec.name.clone()),
                    Value::Array(
                        rec.facets
                            .iter()
                            .map(|f| Value::Text(f.as_str().to_string()))
                            .collect(),
                    ),
                    Value::Text(rec.head.clone()),
                    refs_map(&rec.branches),
                    refs_map(&rec.tags),
                ])
            })
            .collect();
        cbor::encode(&Value::Array(records))
    }

    /// Rebuild a registry from [`Registry::encode`] output, including the name index.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut reg = Registry::new();
        for item in cbor::decode_array(bytes)? {
            let fields = cbor::as_array(item)?;
            let mut f = cbor::Fields::new(fields.clone());
            let id = WorkspaceId::from_bytes(id_bytes(f.bytes()?)?);
            let (name, facets, head, branches, tags) = if fields.len() == 7 {
                let version = f.uint()?;
                if version != 2 {
                    return Err(LoomError::corrupt("unsupported workspace record version"));
                }
                let name = f.text()?;
                let facets = facets_from_array(f.array()?)?;
                let head = f.text()?;
                let branches = refs_from_map(f.map()?)?;
                let tags = refs_from_map(f.map()?)?;
                (name, facets, head, branches, tags)
            } else {
                return Err(LoomError::corrupt("workspace record field count"));
            };
            f.end()?;
            if reg.by_name.insert(name.clone(), id).is_some() {
                return Err(LoomError::corrupt("duplicate workspace name"));
            }
            reg.by_id.insert(
                id,
                Record {
                    name,
                    facets,
                    branches,
                    tags,
                    head,
                },
            );
        }
        Ok(reg)
    }
}

// ---- registry codec helpers --------------------------------------------------------------------

/// A `name -> digest` ref table as a canonical CBOR map (keys sorted on encode).
fn refs_map(refs: &BTreeMap<String, Digest>) -> Value {
    Value::Map(
        refs.iter()
            .map(|(name, d)| (Value::Text(name.clone()), cbor::digest_value(d)))
            .collect(),
    )
}

/// Parse the `name -> digest` map produced by [`refs_map`].
fn refs_from_map(pairs: Vec<(Value, Value)>) -> Result<BTreeMap<String, Digest>> {
    let mut out = BTreeMap::new();
    for (k, v) in pairs {
        out.insert(cbor::as_text(k)?, cbor::as_digest(v)?);
    }
    Ok(out)
}

fn validate_ref_name(kind: &str, name: &str) -> Result<()> {
    if name.is_empty()
        || name == "HEAD"
        || name.starts_with("refs/")
        || name.starts_with('.')
        || name.ends_with('.')
        || name.contains("..")
        || name.contains('/')
        || name.contains('\\')
        || name.chars().any(char::is_control)
    {
        return Err(LoomError::invalid(format!(
            "{kind} name {name:?} is reserved or invalid"
        )));
    }
    Ok(())
}

fn facets_from_array(items: Vec<Value>) -> Result<BTreeSet<FacetKind>> {
    let mut out = BTreeSet::new();
    for item in items {
        out.insert(FacetKind::parse(&cbor::as_text(item)?)?);
    }
    Ok(out)
}

/// A workspace id from a 16-byte CBOR byte string.
fn id_bytes(raw: Vec<u8>) -> Result<[u8; 16]> {
    raw.as_slice()
        .try_into()
        .map_err(|_| LoomError::corrupt("workspace id is not 16 bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nid(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    fn dig(s: &str) -> Digest {
        Digest::blake3(s.as_bytes())
    }

    #[test]
    fn facet_predicates() {
        assert!(FacetKind::Files.is_mountable());
        assert!(!FacetKind::Sql.is_mountable());
        assert!(FacetKind::Files.supports_file_write_projection());
        assert!(!FacetKind::Sql.supports_file_write_projection());
        assert_eq!(
            default_compression_for_facets(&[FacetKind::Files]),
            CompressionHint::Small
        );
        assert_eq!(
            default_compression_for_facets(&[FacetKind::Ledger]),
            CompressionHint::Fast
        );
        assert_eq!(
            default_compression_for_facets(&[FacetKind::Cas]),
            CompressionHint::None
        );
        assert_eq!(FacetKind::Vcs.as_str(), "vcs");
        assert_eq!(FacetKind::parse("vcs").unwrap(), FacetKind::Vcs);
        assert_eq!(FacetKind::Search.as_str(), "search");
        assert_eq!(FacetKind::parse("search").unwrap(), FacetKind::Search);
    }

    #[test]
    fn facet_stable_tags_round_trip() {
        // Dynamic: adding a facet needs no edit here. Every facet round-trips through its stable tag,
        // tags are unique and form a gap-free `0..ALL.len()` range, and the first out-of-range tag is
        // unknown. The exact wire values (append-only, never renumbered) are additionally locked by the
        // byte-pinned vectors (manifest canonical bytes, ACL-store codec), so this guards structure.
        let mut tags = std::collections::BTreeSet::new();
        for facet in FacetKind::ALL {
            let tag = facet.stable_tag();
            assert_eq!(FacetKind::from_stable_tag(tag), Some(facet));
            assert!(tags.insert(tag), "duplicate stable tag {tag} for {facet:?}");
        }
        let count = FacetKind::ALL.len() as u8;
        assert_eq!(
            tags.into_iter().collect::<Vec<_>>(),
            (0..count).collect::<Vec<_>>(),
            "stable tags must be a gap-free 0..N range"
        );
        assert_eq!(FacetKind::from_stable_tag(count), None);
    }

    #[test]
    fn workspace_name_uniqueness_and_facets() {
        let mut reg = Registry::new();
        let id = reg.create(FacetKind::Sql, None, nid(1)).unwrap();
        assert!(reg.has_facet(id, FacetKind::Sql).unwrap());
        reg.ensure_for_write(&WsSelector::Default(FacetKind::Vector), nid(2))
            .unwrap();
        assert!(reg.has_facet(id, FacetKind::Vector).unwrap());
        let err = reg.create(FacetKind::Sql, None, nid(3)).unwrap_err();
        assert_eq!(err.code, Code::AlreadyExists);
        assert_eq!(reg.list(None).len(), 1);
        assert_eq!(reg.list(Some(FacetKind::Sql)).len(), 1);
        assert_eq!(reg.list(Some(FacetKind::Vector)).len(), 1);
    }

    #[test]
    fn read_missing_is_not_found_write_creates() {
        let mut reg = Registry::new();
        // Read of a missing default is NOT_FOUND (no side effect).
        let err = reg.open(&WsSelector::Default(FacetKind::Sql)).unwrap_err();
        assert_eq!(err.code, Code::NotFound);
        assert!(reg.list(None).is_empty());
        // A write creates the default workspace on first use.
        let id = reg
            .ensure_for_write(&WsSelector::Default(FacetKind::Sql), nid(7))
            .unwrap();
        assert_eq!(reg.open(&WsSelector::Default(FacetKind::Sql)).unwrap(), id);
        // A second write resolves to the same workspace (no duplicate).
        let again = reg
            .ensure_for_write(&WsSelector::Default(FacetKind::Sql), nid(8))
            .unwrap();
        assert_eq!(again, id);
    }

    #[test]
    fn rename_keeps_uniqueness() {
        let mut reg = Registry::new();
        let a = reg
            .create(FacetKind::Sql, Some("analytics"), nid(1))
            .unwrap();
        reg.create(FacetKind::Sql, Some("staging"), nid(2)).unwrap();
        // Renaming onto an existing name is rejected.
        assert_eq!(
            reg.rename(a, "staging").unwrap_err().code,
            Code::AlreadyExists
        );
        // A free name works and the old name frees up.
        reg.rename(a, "warehouse").unwrap();
        assert_eq!(reg.name(a).unwrap(), "warehouse");
        reg.create(FacetKind::Sql, Some("analytics"), nid(3))
            .unwrap();
    }

    #[test]
    fn delete_drops_workspace() {
        let mut reg = Registry::new();
        let id = reg.create(FacetKind::Files, None, nid(1)).unwrap();
        reg.delete(id).unwrap();
        assert_eq!(
            reg.open(&WsSelector::Id(id)).unwrap_err().code,
            Code::NotFound
        );
        // The name is free again.
        reg.create(FacetKind::Files, None, nid(2)).unwrap();
    }

    #[test]
    fn files_workspace_can_branch() {
        let mut reg = Registry::new();
        let id = reg.create(FacetKind::Files, None, nid(1)).unwrap();
        assert!(reg.supports_branching(id).unwrap());
        reg.branch_create(id, DEFAULT_BRANCH, dig("c0")).unwrap();
        reg.branch_create(id, "feature", dig("c1")).unwrap();
        assert_eq!(reg.branch_list(id).unwrap(), vec!["feature", "main"]);
    }

    #[test]
    fn workspace_history_branches() {
        let mut reg = Registry::new();
        let id = reg.create(FacetKind::Files, None, nid(1)).unwrap();
        assert!(reg.supports_branching(id).unwrap());
        reg.branch_create(id, DEFAULT_BRANCH, dig("c0")).unwrap();
        reg.branch_create(id, "feature", dig("c0")).unwrap();
        assert_eq!(reg.branch_list(id).unwrap(), vec!["feature", "main"]);
        assert_eq!(
            reg.branch_create(id, "feature", dig("c2"))
                .unwrap_err()
                .code,
            Code::AlreadyExists
        );
    }

    #[test]
    fn workspace_history_rejects_reserved_ref_names() {
        let mut reg = Registry::new();
        let id = reg.create(FacetKind::Files, None, nid(1)).unwrap();
        for name in [
            "",
            "HEAD",
            "refs/heads/main",
            "feature/a",
            ".hidden",
            "bad..name",
        ] {
            assert_eq!(
                reg.branch_create(id, name, dig("c0")).unwrap_err().code,
                Code::InvalidArgument
            );
            assert_eq!(
                reg.tag_create(id, name, dig("c0")).unwrap_err().code,
                Code::InvalidArgument
            );
        }
        reg.branch_create(id, "feature", dig("c0")).unwrap();
        reg.tag_create(id, "v1", dig("c0")).unwrap();
    }

    #[test]
    fn multi_facet_workspace_can_branch() {
        let mut reg = Registry::new();
        let id = reg.create(FacetKind::Files, None, nid(1)).unwrap();
        reg.add_facet(id, FacetKind::Sql).unwrap();
        assert!(reg.supports_branching(id).unwrap());
        reg.branch_create(id, "feature", dig("c1")).unwrap();
        assert_eq!(reg.branch_list(id).unwrap(), vec!["feature".to_string()]);
    }

    #[test]
    fn branch_cas() {
        let mut reg = Registry::new();
        let id = reg.create(FacetKind::Files, None, nid(1)).unwrap();
        // Unborn branch: expected None.
        reg.update_branch(id, DEFAULT_BRANCH, None, dig("c0"))
            .unwrap();
        // Stale expectation is rejected.
        assert_eq!(
            reg.update_branch(id, DEFAULT_BRANCH, None, dig("c1"))
                .unwrap_err()
                .code,
            Code::CasMismatch
        );
        // Correct expectation advances the tip.
        reg.update_branch(id, DEFAULT_BRANCH, Some(dig("c0")), dig("c1"))
            .unwrap();
        assert_eq!(reg.branch_tip(id, DEFAULT_BRANCH).unwrap(), Some(dig("c1")));
    }

    #[test]
    fn isolation_rejects_cross_workspace() {
        assert!(Registry::require_same(nid(1), nid(1)).is_ok());
        assert_eq!(
            Registry::require_same(nid(1), nid(2)).unwrap_err().code,
            Code::CrossWorkspace
        );
    }

    #[test]
    fn workspace_id_v4_and_roundtrip() {
        let id = WorkspaceId::v4_from_bytes([0xAB; 16]);
        // Version nibble is 4, variant top bits are 10xx.
        assert_eq!(id.as_bytes()[6] >> 4, 0x4);
        assert_eq!(id.as_bytes()[8] >> 6, 0b10);
        let s = id.to_string();
        assert_eq!(s.len(), 36); // 32 hex + 4 hyphens
        assert_eq!(WorkspaceId::parse(&s).unwrap(), id);
    }
}
