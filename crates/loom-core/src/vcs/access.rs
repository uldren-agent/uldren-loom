use super::*;

impl<S: ObjectStore> Loom<S> {
    /// Build an engine over a fresh object store and an empty registry.
    pub fn new(store: S) -> Self {
        Self {
            store,
            registry: Registry::new(),
            lazy_state_sections: None,
            content: BTreeMap::new(),
            work: BTreeMap::new(),
            dirs: BTreeMap::new(),
            compression: BTreeMap::new(),
            pending_chunklists: BTreeSet::new(),
            consumer_offsets: BTreeMap::new(),
            stream_low_water_marks: BTreeMap::new(),
            merge_state: BTreeMap::new(),
            index: BTreeMap::new(),
            inodes: BTreeMap::new(),
            handles: BTreeMap::new(),
            path_to_inode: BTreeMap::new(),
            next_inode: 1,
            next_handle: 1,
            ephemeral_kv: BTreeMap::new(),
            protected_refs: BTreeMap::new(),
            identity: None,
            acl: AclStore::new(),
            predicate_evaluator: None,
            session: None,
        }
    }

    /// Shared read access to the workspace registry.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Mutable access to the workspace registry (create / open / rename / delete workspaces).
    pub fn registry_mut(&mut self) -> &mut Registry {
        &mut self.registry
    }

    /// The underlying object store.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// Mutable access to the underlying object store. A persistence backend uses this to set its
    /// mutable root (the reference-store pointer) after [`Loom::save_state`].
    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    /// Consume the engine and return its underlying object store. Used to hand an opened (e.g.
    /// lock-free read-only) store to a long-lived holder - the lazy SQL base snapshot moves a read
    /// view's store into an `Arc<dyn ObjectStore>` so it can read durable table roots for the
    /// session's lifetime without being generic over the backend.
    pub fn into_store(self) -> S {
        self.store
    }

    /// Install the current principal registry used by engine authorization.
    pub fn set_identity_store(&mut self, identity: IdentityStore) {
        self.identity = Some(identity);
    }

    /// Current principal registry, if identity is enabled for this engine.
    pub fn identity_store(&self) -> Option<&IdentityStore> {
        self.identity.as_ref()
    }

    /// Mutable principal registry, if identity is enabled for this engine.
    pub fn identity_store_mut(&mut self) -> Option<&mut IdentityStore> {
        self.identity.as_mut()
    }

    /// Replace the current ACL grant store used by engine authorization.
    pub fn set_acl_store(&mut self, acl: AclStore) {
        self.acl = acl;
    }

    /// Current ACL grant store.
    pub fn acl_store(&self) -> &AclStore {
        &self.acl
    }

    /// Mutable ACL grant store.
    pub fn acl_store_mut(&mut self) -> &mut AclStore {
        &mut self.acl
    }

    pub fn set_acl_predicate_evaluator(&mut self, evaluator: Arc<dyn AclPredicateEvaluator>) {
        self.predicate_evaluator = Some(evaluator);
    }

    pub fn clear_acl_predicate_evaluator(&mut self) {
        self.predicate_evaluator = None;
    }

    /// Bind subsequent engine calls to an authenticated session id.
    pub fn set_session(&mut self, session: impl Into<String>) {
        self.session = Some(session.into());
    }

    /// Clear the session binding. In authenticated mode this makes protected operations fail closed.
    pub fn clear_session(&mut self) {
        self.session = None;
    }

    /// Resolve the principal for the current session, or the unauthenticated root principal while
    /// bootstrap mode is still active.
    pub fn effective_principal(&self) -> Result<Option<PrincipalId>> {
        self.identity
            .as_ref()
            .map(|identity| identity.effective_principal(self.session.as_deref()))
            .transpose()
    }

    pub fn authorize(&self, ns: WorkspaceId, facet: FacetKind, right: AclRight) -> Result<()> {
        self.authorize_resource(AclResource::all(ns, facet), right)
    }

    pub fn authorize_domain(
        &self,
        ns: WorkspaceId,
        domain: AclDomain,
        right: AclRight,
    ) -> Result<()> {
        self.authorize_resource(AclResource::all(ns, domain), right)
    }

    pub fn authorize_resource(&self, resource: AclResource<'_>, right: AclRight) -> Result<()> {
        let Some(identity) = &self.identity else {
            return Ok(());
        };
        let principal = identity.effective_principal(self.session.as_deref())?;
        let roles = identity.effective_roles(principal)?;
        self.acl.authorize_resource_with_roles_and_evaluator(
            identity.authenticated_mode(),
            principal,
            roles,
            resource,
            right,
            self.predicate_evaluator.as_deref(),
        )
    }

    pub fn authorize_file_path(&self, ns: WorkspaceId, path: &str, right: AclRight) -> Result<()> {
        let path = normalize_projection_path(path)?;
        self.authorize_facet_path(ns, FacetKind::Files, &path, right)
    }

    pub(crate) fn authorize_path(
        &self,
        ns: WorkspaceId,
        path: &str,
        right: AclRight,
    ) -> Result<()> {
        self.authorize_file_path(ns, path, right)
    }

    pub(crate) fn authorize_facet_path(
        &self,
        ns: WorkspaceId,
        facet: FacetKind,
        path: &str,
        right: AclRight,
    ) -> Result<()> {
        self.authorize_resource(
            AclResource::scoped(
                ns,
                facet,
                None,
                AclResourceScope::Prefix {
                    kind: AclScopeKind::Path,
                    value: path.as_bytes(),
                },
            ),
            right,
        )
    }

    pub(crate) fn authorize_collection(
        &self,
        ns: WorkspaceId,
        facet: FacetKind,
        collection: &str,
        right: AclRight,
    ) -> Result<()> {
        self.authorize_resource(
            AclResource::scoped(
                ns,
                facet,
                None,
                AclResourceScope::Prefix {
                    kind: AclScopeKind::Collection,
                    value: collection.as_bytes(),
                },
            ),
            right,
        )
    }

    pub(crate) fn authorize_key(
        &self,
        ns: WorkspaceId,
        facet: FacetKind,
        key: &[u8],
        right: AclRight,
    ) -> Result<()> {
        self.authorize_resource(
            AclResource::scoped(
                ns,
                facet,
                None,
                AclResourceScope::Prefix {
                    kind: AclScopeKind::Key,
                    value: key,
                },
            ),
            right,
        )
    }

    pub(crate) fn authorize_table(
        &self,
        ns: WorkspaceId,
        table: &str,
        right: AclRight,
    ) -> Result<()> {
        self.authorize_resource(
            AclResource::scoped(
                ns,
                FacetKind::Sql,
                None,
                AclResourceScope::Prefix {
                    kind: AclScopeKind::Table,
                    value: table.as_bytes(),
                },
            ),
            right,
        )
    }

    pub(crate) fn authorize_ref(
        &self,
        ns: WorkspaceId,
        ref_name: &str,
        right: AclRight,
    ) -> Result<()> {
        self.authorize_resource(
            AclResource::scoped(ns, FacetKind::Vcs, Some(ref_name), AclResourceScope::All),
            right,
        )
    }

    pub fn authorize_global_admin(&self) -> Result<()> {
        let Some(identity) = &self.identity else {
            return Ok(());
        };
        let principal = identity.effective_principal(self.session.as_deref())?;
        let roles = identity.effective_roles(principal)?;
        self.acl
            .authorize_global_admin_with_roles(identity.authenticated_mode(), principal, roles)
    }

    pub(crate) fn authorize_workspace_facets(
        &self,
        ns: WorkspaceId,
        right: AclRight,
    ) -> Result<()> {
        for facet in self.registry.facets(ns)? {
            self.authorize(ns, facet, right)?;
        }
        Ok(())
    }

    pub(crate) fn authorize_branch_update(
        &self,
        ns: WorkspaceId,
        ref_name: &str,
        old: Option<Digest>,
        new: Digest,
    ) -> Result<()> {
        self.evaluate_protected_ref_update(ns, ref_name, old, Some(new))?;
        if let Some(old) = old
            && !self.is_ancestor(old, new)?
        {
            self.authorize(ns, FacetKind::Vcs, AclRight::Admin)?;
        }
        Ok(())
    }
}

fn normalize_projection_path(path: &str) -> Result<String> {
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        Ok(String::new())
    } else {
        normalize_path(path)
    }
}
