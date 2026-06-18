use super::*;

impl<S: ObjectStore> Loom<S> {
    /// Store or replace the protected-ref policy for an exact `branch/name` or `tag/name` ref.
    pub fn set_protected_ref_policy(
        &mut self,
        ns: WorkspaceId,
        ref_name: &str,
        policy: ProtectedRefPolicy,
    ) -> Result<()> {
        validate_protected_ref_name(ref_name)?;
        self.authorize_ref(ns, ref_name, AclRight::Admin)?;
        self.protected_refs
            .insert((ns, ref_name.to_string()), policy);
        Ok(())
    }

    /// Remove the protected-ref policy for an exact `branch/name` or `tag/name` ref.
    pub fn remove_protected_ref_policy(&mut self, ns: WorkspaceId, ref_name: &str) -> Result<bool> {
        validate_protected_ref_name(ref_name)?;
        self.authorize_ref(ns, ref_name, AclRight::Admin)?;
        Ok(self
            .protected_refs
            .remove(&(ns, ref_name.to_string()))
            .is_some())
    }

    /// Return the protected-ref policy for an exact ref, if one is configured.
    pub fn protected_ref_policy(
        &self,
        ns: WorkspaceId,
        ref_name: &str,
    ) -> Result<Option<ProtectedRefPolicy>> {
        validate_protected_ref_name(ref_name)?;
        self.authorize_ref(ns, ref_name, AclRight::Read)?;
        Ok(self
            .protected_refs
            .get(&(ns, ref_name.to_string()))
            .cloned())
    }

    /// List protected-ref policies for one workspace in ref-name order.
    pub fn protected_ref_policies(
        &self,
        ns: WorkspaceId,
    ) -> Result<Vec<(String, ProtectedRefPolicy)>> {
        self.authorize(ns, FacetKind::Vcs, AclRight::Read)?;
        Ok(self
            .protected_refs
            .iter()
            .filter(|((policy_ns, _), _)| *policy_ns == ns)
            .map(|((_, ref_name), policy)| (ref_name.clone(), policy.clone()))
            .collect())
    }

    pub(crate) fn evaluate_protected_ref_update(
        &self,
        ns: WorkspaceId,
        ref_name: &str,
        old: Option<Digest>,
        new: Option<Digest>,
    ) -> Result<()> {
        let Some(policy) = self.protected_refs.get(&(ns, ref_name.to_string())) else {
            return Ok(());
        };
        if new.is_none()
            && (policy.fast_forward_only || policy.retention_lock || policy.governance_lock)
        {
            return Err(protected_ref_denied(ref_name));
        }
        if policy.fast_forward_only
            && let (Some(old), Some(new)) = (old, new)
            && old != new
            && !self.is_ancestor(old, new)?
        {
            return Err(protected_ref_denied(ref_name));
        }
        if policy.signed_commits_required
            || policy.signed_ref_advance_required
            || policy.required_review_count > 0
        {
            return Err(protected_ref_denied(ref_name));
        }
        Ok(())
    }

    pub(crate) fn protected_ref_policy_unchecked(
        &self,
        ns: WorkspaceId,
        ref_name: &str,
    ) -> Result<Option<&ProtectedRefPolicy>> {
        validate_protected_ref_name(ref_name)?;
        Ok(self.protected_refs.get(&(ns, ref_name.to_string())))
    }
}

fn protected_ref_denied(ref_name: &str) -> LoomError {
    LoomError::new(
        Code::PermissionDenied,
        format!("protected ref {ref_name:?} policy denied the ref update"),
    )
}

fn validate_protected_ref_name(ref_name: &str) -> Result<()> {
    let Some((kind, name)) = ref_name.split_once('/') else {
        return Err(LoomError::invalid(format!(
            "protected ref {ref_name:?} must be branch/name or tag/name"
        )));
    };
    if kind != "branch" && kind != "tag" {
        return Err(LoomError::invalid(format!(
            "protected ref {ref_name:?} must be branch/name or tag/name"
        )));
    }
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
            "protected ref {ref_name:?} is reserved or invalid"
        )));
    }
    Ok(())
}
