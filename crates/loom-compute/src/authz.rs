//! Runtime authorization for the compute facade.

use loom_core::{
    AclResource, AclResourceScope, AclRight, AclScopeKind, AclStore, Code, Loom, LoomError,
    ObjectStore, PrincipalId, RoleId, WorkspaceId,
};

use crate::capability::{Capability, GrantSet, Mode, is_program_grantable};
use crate::error::ExecError;

/// Request-time identity, branch, and manifest grants for one program execution.
#[derive(Clone, Debug)]
pub struct ExecContext {
    /// Workspace scope for execution.
    pub workspace: WorkspaceId,
    /// Principal running the program.
    pub principal: PrincipalId,
    /// Role grants carried by the principal.
    pub roles: Vec<RoleId>,
    /// Whether principal and role ACLs are enforced.
    pub authenticated: bool,
    /// Branch used as the execution base.
    pub base_branch: String,
    /// Manifest grants approved for this execution.
    pub grants: GrantSet,
}

impl ExecContext {
    /// Whether every declared grant names a program-grantable facet.
    pub fn grants_are_grantable(&self) -> bool {
        self.grants.is_grantable()
    }

    /// Authorize one program operation under the principal ACL and manifest grant intersection.
    pub fn authorize_operation(
        &self,
        acl: &AclStore,
        facet: Capability,
        mode: Mode,
        target: &str,
    ) -> Result<(), ExecError> {
        if !is_program_grantable(facet) {
            return Err(ExecError::Denied(format!(
                "facet {facet:?} is not program-grantable"
            )));
        }

        acl.authorize_resource_with_roles(
            self.authenticated,
            self.principal,
            self.roles.iter().copied(),
            AclResource::scoped(
                self.workspace,
                facet,
                Some(&self.base_branch),
                AclResourceScope::Prefix {
                    kind: AclScopeKind::Exec,
                    value: target.as_bytes(),
                },
            ),
            AclRight::Execute,
        )?;

        if !self.grants.permits(facet, mode, target) {
            return Err(ExecError::Denied(format!(
                "manifest grant does not permit {mode:?} on facet {facet:?} at {target:?}"
            )));
        }

        Ok(())
    }
}

pub fn run_as_context<S: ObjectStore>(
    loom: &Loom<S>,
    workspace: WorkspaceId,
    principal: PrincipalId,
    base_branch: impl Into<String>,
    grants: GrantSet,
) -> Result<ExecContext, ExecError> {
    let (authenticated, roles) = match loom.identity_store() {
        Some(identity) => {
            let principal_record = identity.principal(principal).map_err(trigger_denied)?;
            if !principal_record.enabled {
                return Err(trigger_denied(LoomError::new(
                    Code::PermissionDenied,
                    "trigger run_as principal disabled",
                )));
            }
            let roles = identity
                .effective_roles(principal)
                .map_err(trigger_denied)?
                .into_iter()
                .collect();
            (identity.authenticated_mode(), roles)
        }
        None => (false, Vec::new()),
    };
    Ok(ExecContext {
        workspace,
        principal,
        roles,
        authenticated,
        base_branch: base_branch.into(),
        grants,
    })
}

fn trigger_denied(err: LoomError) -> ExecError {
    ExecError::Core(LoomError::new(
        Code::TriggerDenied,
        format!("trigger run_as denied: {}", err.message),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{Grant, Scope};
    use loom_core::{
        AclSubject, IdentityStore, MemoryStore, PrincipalKind, ROLE_SERVICE_ID, vcs::Loom,
    };

    fn nid(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    fn pid(seed: u8) -> PrincipalId {
        PrincipalId::from_bytes([seed; 16])
    }

    fn kv_write_ctx(authenticated: bool) -> ExecContext {
        ExecContext {
            workspace: nid(1),
            principal: pid(9),
            roles: Vec::new(),
            authenticated,
            base_branch: "main".to_string(),
            grants: GrantSet::new(vec![Grant {
                facet: Capability::Kv,
                mode: Mode::Write,
                scopes: vec![Scope::Prefix("session:".into())],
            }]),
        }
    }

    #[test]
    fn allow_when_acl_and_manifest_both_permit() {
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Principal(pid(9)),
            Some(nid(1)),
            Some(Capability::Kv),
            [AclRight::Execute],
        )
        .unwrap();
        let ctx = kv_write_ctx(true);
        assert!(
            ctx.authorize_operation(&acl, Capability::Kv, Mode::Write, "session:1")
                .is_ok()
        );
    }

    #[test]
    fn deny_when_acl_explicitly_denies() {
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Principal(pid(9)),
            Some(nid(1)),
            Some(Capability::Kv),
            [AclRight::Execute],
        )
        .unwrap();
        acl.deny(
            AclSubject::Principal(pid(9)),
            Some(nid(1)),
            Some(Capability::Kv),
            [AclRight::Execute],
        )
        .unwrap();
        let ctx = kv_write_ctx(true);
        assert!(matches!(
            ctx.authorize_operation(&acl, Capability::Kv, Mode::Write, "session:1"),
            Err(ExecError::Core(_))
        ));
    }

    #[test]
    fn default_deny_when_no_acl_grant() {
        let acl = AclStore::new();
        let ctx = kv_write_ctx(true);
        assert!(matches!(
            ctx.authorize_operation(&acl, Capability::Kv, Mode::Write, "session:1"),
            Err(ExecError::Core(_))
        ));
    }

    #[test]
    fn manifest_grant_must_also_permit_the_operation() {
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Principal(pid(9)),
            Some(nid(1)),
            None,
            [AclRight::Execute],
        )
        .unwrap();
        let ctx = kv_write_ctx(true);
        assert!(matches!(
            ctx.authorize_operation(&acl, Capability::Kv, Mode::Write, "user:1"),
            Err(ExecError::Denied(_))
        ));
        assert!(matches!(
            ctx.authorize_operation(&acl, Capability::Kv, Mode::Read, "session:1"),
            Err(ExecError::Denied(_))
        ));
        assert!(matches!(
            ctx.authorize_operation(&acl, Capability::Files, Mode::Write, "session:1"),
            Err(ExecError::Denied(_))
        ));
        assert!(
            ctx.authorize_operation(&acl, Capability::Kv, Mode::Write, "session:42")
                .is_ok()
        );
    }

    #[test]
    fn scoped_exec_acl_narrows_the_operation_target() {
        let mut acl = AclStore::new();
        acl.grant(loom_core::AclGrant {
            subject: AclSubject::Principal(pid(9)),
            workspace: Some(nid(1)),
            domain: Some(Capability::Kv.into()),
            ref_glob: None,
            scopes: vec![loom_core::AclScope::Prefix {
                kind: loom_core::AclScopeKind::Exec,
                prefix: b"session:".to_vec(),
            }],
            rights: [AclRight::Execute].into_iter().collect(),
            effect: loom_core::AclEffect::Allow,
            predicate: None,
        })
        .unwrap();

        let ctx = kv_write_ctx(true);
        assert!(
            ctx.authorize_operation(&acl, Capability::Kv, Mode::Write, "session:1")
                .is_ok()
        );
        assert!(matches!(
            ctx.authorize_operation(&acl, Capability::Kv, Mode::Write, "user:1"),
            Err(ExecError::Core(_))
        ));
    }

    #[test]
    fn non_grantable_facet_is_denied_before_acl() {
        let mut acl = AclStore::new();
        acl.allow(AclSubject::Everyone, None, None, [AclRight::Execute])
            .unwrap();
        let ctx = ExecContext {
            workspace: nid(1),
            principal: pid(9),
            roles: Vec::new(),
            authenticated: true,
            base_branch: "main".to_string(),
            grants: GrantSet::new(vec![Grant {
                facet: Capability::Vcs,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            }]),
        };
        assert!(!ctx.grants_are_grantable());
        assert!(matches!(
            ctx.authorize_operation(&acl, Capability::Vcs, Mode::Write, "anything"),
            Err(ExecError::Denied(_))
        ));
        assert!(matches!(
            ctx.authorize_operation(&acl, Capability::Program, Mode::Read, "anything"),
            Err(ExecError::Denied(_))
        ));
    }

    #[test]
    fn unauthenticated_store_passes_the_acl_layer_but_still_checks_the_manifest() {
        let acl = AclStore::new();
        let ctx = kv_write_ctx(false);
        assert!(
            ctx.authorize_operation(&acl, Capability::Kv, Mode::Write, "session:1")
                .is_ok()
        );
        assert!(matches!(
            ctx.authorize_operation(&acl, Capability::Kv, Mode::Write, "user:1"),
            Err(ExecError::Denied(_))
        ));
    }

    #[test]
    fn run_as_context_resolves_current_roles_at_fire_time() {
        let mut loom = Loom::new(MemoryStore::new());
        let root = nid(1);
        let service = nid(2);
        let mut identity = IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        identity
            .add_principal(service, "svc", PrincipalKind::Service)
            .unwrap();
        identity.assign_role(service, ROLE_SERVICE_ID).unwrap();
        loom.set_identity_store(identity);

        let context = run_as_context(
            &loom,
            nid(9),
            service,
            "main",
            GrantSet::new(vec![Grant {
                facet: Capability::Kv,
                mode: Mode::Read,
                scopes: vec![Scope::All],
            }]),
        )
        .unwrap();

        assert!(context.authenticated);
        assert_eq!(context.principal, service);
        assert_eq!(context.roles, vec![ROLE_SERVICE_ID]);
    }

    #[test]
    fn run_as_context_fails_closed_for_missing_principal() {
        let mut loom = Loom::new(MemoryStore::new());
        let root = nid(1);
        let mut identity = IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        loom.set_identity_store(identity);

        let err = run_as_context(&loom, nid(9), nid(44), "main", GrantSet::default()).unwrap_err();

        assert_eq!(err.code(), Code::TriggerDenied);
    }
}
