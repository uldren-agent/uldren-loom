use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

use loom_core::{AclRight, Code, FacetKind, LoomError, Result, WorkspaceId};

use crate::{HostedAuth, HostedKernel};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServedResultScope {
    pub workspace: WorkspaceId,
    pub facet: FacetKind,
    pub right: AclRight,
}

impl ServedResultScope {
    pub fn new(workspace: WorkspaceId, facet: FacetKind, right: AclRight) -> Self {
        Self {
            workspace,
            facet,
            right,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServedResultHandle {
    pub id: String,
    pub principal: Option<WorkspaceId>,
    pub session_id: String,
    pub operation: String,
    pub resource: Option<String>,
    pub scopes: Vec<ServedResultScope>,
    pub created_at: SystemTime,
    pub expires_at: SystemTime,
    closed: bool,
}

#[derive(Default)]
pub struct ServedResultHandles {
    next_id: u64,
    handles: BTreeMap<String, ServedResultHandle>,
}

impl ServedResultHandles {
    pub fn insert(
        &mut self,
        auth: &HostedAuth,
        operation: impl Into<String>,
        scopes: Vec<ServedResultScope>,
        ttl: Duration,
    ) -> Result<String> {
        self.insert_resource(auth, operation, None, scopes, ttl)
    }

    pub fn insert_resource(
        &mut self,
        auth: &HostedAuth,
        operation: impl Into<String>,
        resource: Option<String>,
        scopes: Vec<ServedResultScope>,
        ttl: Duration,
    ) -> Result<String> {
        if ttl.is_zero() {
            return Err(LoomError::invalid("result handle ttl must be positive"));
        }
        let id = format!("rh-{}", self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        let now = SystemTime::now();
        self.handles.insert(
            id.clone(),
            ServedResultHandle {
                id: id.clone(),
                principal: auth.principal,
                session_id: auth.session_id.clone(),
                operation: operation.into(),
                resource,
                scopes,
                created_at: now,
                expires_at: now + ttl,
                closed: false,
            },
        );
        Ok(id)
    }

    pub fn authorize(
        &mut self,
        kernel: &HostedKernel,
        handle_id: &str,
        auth: &HostedAuth,
        expected_operation: &str,
    ) -> Result<()> {
        let handle = self.visible_handle(handle_id, auth, expected_operation)?;
        kernel.read(auth, |loom| {
            for scope in &handle.scopes {
                loom.authorize(scope.workspace, scope.facet, scope.right)?;
            }
            Ok(())
        })
    }

    pub fn authorize_and_close(
        &mut self,
        kernel: &HostedKernel,
        handle_id: &str,
        auth: &HostedAuth,
        expected_operation: &str,
    ) -> Result<()> {
        self.authorize(kernel, handle_id, auth, expected_operation)?;
        if let Some(handle) = self.handles.get_mut(handle_id) {
            handle.closed = true;
        }
        Ok(())
    }

    pub fn authorize_and_remove(
        &mut self,
        kernel: &HostedKernel,
        handle_id: &str,
        auth: &HostedAuth,
        expected_operation: &str,
    ) -> Result<ServedResultHandle> {
        self.authorize(kernel, handle_id, auth, expected_operation)?;
        self.handles
            .remove(handle_id)
            .ok_or_else(hidden_handle_error)
    }

    fn visible_handle(
        &mut self,
        handle_id: &str,
        auth: &HostedAuth,
        expected_operation: &str,
    ) -> Result<&ServedResultHandle> {
        let expired = self
            .handles
            .get(handle_id)
            .is_some_and(|handle| SystemTime::now() > handle.expires_at);
        if expired {
            self.handles.remove(handle_id);
        }
        let handle = self
            .handles
            .get(handle_id)
            .ok_or_else(hidden_handle_error)?;
        if handle.closed
            || handle.principal != auth.principal
            || handle.session_id != auth.session_id
            || handle.operation != expected_operation
        {
            return Err(hidden_handle_error());
        }
        Ok(handle)
    }
}

fn hidden_handle_error() -> LoomError {
    LoomError::new(Code::NotFound, "result handle not found")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use loom_core::{AclRight, AclSubject, FacetKind};
    use loom_store::FileStore;

    use super::{ServedResultHandles, ServedResultScope};
    use crate::test_support::{init, nid, temp_path};
    use crate::{HostedAuth, HostedKernel};

    #[test]
    fn result_handle_reauthenticates_and_rechecks_pep() {
        let path = temp_path("result-handle-authz");
        let user = nid(7);
        let ns = init(&path, Some(user));
        let grant = loom_coordination::with_local_store_write_lock(&path, || {
            let store = FileStore::open(&path).unwrap();
            let mut acl = store.acl_store().unwrap().unwrap();
            acl.allow(
                AclSubject::Principal(user),
                Some(ns),
                Some(FacetKind::Files),
                [AclRight::Read],
            )
            .unwrap();
            let grant = acl.grants().last().unwrap().clone();
            store.save_acl_store(&acl).unwrap();
            drop(store);
            Ok(grant)
        })
        .unwrap();

        let kernel = HostedKernel::new(&path);
        let auth = HostedAuth::passphrase(user, "alice-pass", "session-a");
        let mut handles = ServedResultHandles::default();
        let handle = handles
            .insert(
                &auth,
                "files.list",
                vec![ServedResultScope::new(ns, FacetKind::Files, AclRight::Read)],
                Duration::from_secs(60),
            )
            .unwrap();
        handles
            .authorize(&kernel, &handle, &auth, "files.list")
            .unwrap();

        loom_coordination::with_local_store_write_lock(&path, || {
            let store = FileStore::open(&path).unwrap();
            let mut acl = store.acl_store().unwrap().unwrap();
            assert!(acl.revoke(&grant));
            store.save_acl_store(&acl).unwrap();
            drop(store);
            Ok(())
        })
        .unwrap();

        let err = handles
            .authorize(&kernel, &handle, &auth, "files.list")
            .unwrap_err();
        assert_eq!(err.code, loom_core::Code::PermissionDenied);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn result_handle_hides_wrong_session_and_closed_handles() {
        let path = temp_path("result-handle-hidden");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "session-a");
        let mut handles = ServedResultHandles::default();
        let handle = handles
            .insert(
                &auth,
                "files.read",
                vec![ServedResultScope::new(ns, FacetKind::Files, AclRight::Read)],
                Duration::from_secs(60),
            )
            .unwrap();
        let other_session = HostedAuth::passphrase(nid(1), "root-pass", "session-b");
        let err = handles
            .authorize(&kernel, &handle, &other_session, "files.read")
            .unwrap_err();
        assert_eq!(err.code, loom_core::Code::NotFound);

        handles
            .authorize_and_close(&kernel, &handle, &auth, "files.read")
            .unwrap();
        let err = handles
            .authorize(&kernel, &handle, &auth, "files.read")
            .unwrap_err();
        assert_eq!(err.code, loom_core::Code::NotFound);
        fs::remove_file(path).unwrap();
    }
}
