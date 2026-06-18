//! Principal authorization policy evaluation.

use std::collections::BTreeSet;

use crate::{AclDomain, Code, FacetKind, LoomError, PrincipalId, Result, RoleId, WorkspaceId};

pub const ACL_MAX_SCOPES_PER_GRANT: usize = 64;
pub const ACL_MAX_SCOPE_PREFIX_LEN: usize = 1024;
pub const ACL_MAX_GRANTS_PER_SUBJECT: usize = 256;
pub const ACL_MAX_PREDICATE_LEN: usize = 4096;
pub const ACL_PREDICATE_LANGUAGE_CEL: &str = "cel";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AclRight {
    Read,
    Write,
    Advance,
    Merge,
    Execute,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AclEffect {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AclSubject {
    Principal(PrincipalId),
    Role(RoleId),
    Everyone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AclScopeKind {
    Ref,
    Collection,
    Path,
    Key,
    Table,
    Exec,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AclScope {
    All,
    Prefix { kind: AclScopeKind, prefix: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AclPredicate {
    pub language: String,
    pub expression: String,
}

impl AclPredicate {
    pub fn cel(expression: impl Into<String>) -> Result<Self> {
        let predicate = Self {
            language: ACL_PREDICATE_LANGUAGE_CEL.to_string(),
            expression: expression.into(),
        };
        validate_predicate(&predicate)?;
        Ok(predicate)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AclResource<'a> {
    pub workspace: WorkspaceId,
    pub domain: AclDomain,
    pub ref_name: Option<&'a str>,
    pub scope: AclResourceScope<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AclEvaluationContext<'a> {
    pub principal: PrincipalId,
    pub roles: &'a BTreeSet<RoleId>,
    pub resource: AclResource<'a>,
    pub right: AclRight,
}

pub trait AclPredicateEvaluator: std::fmt::Debug + Send + Sync {
    fn evaluate(
        &self,
        predicate: &AclPredicate,
        context: &AclEvaluationContext<'_>,
    ) -> Result<bool>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AclResourceScope<'a> {
    All,
    Prefix { kind: AclScopeKind, value: &'a [u8] },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AclGrant {
    pub subject: AclSubject,
    pub workspace: Option<WorkspaceId>,
    pub domain: Option<AclDomain>,
    pub ref_glob: Option<String>,
    pub scopes: Vec<AclScope>,
    pub rights: BTreeSet<AclRight>,
    pub effect: AclEffect,
    pub predicate: Option<AclPredicate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AclStore {
    grants: Vec<AclGrant>,
}

impl AclStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn grants(&self) -> &[AclGrant] {
        &self.grants
    }

    pub fn grant(&mut self, grant: AclGrant) -> Result<()> {
        if grant.rights.is_empty() {
            return Err(LoomError::invalid(
                "acl grant must include at least one right",
            ));
        }
        validate_scopes(&grant.scopes)?;
        if let Some(predicate) = &grant.predicate {
            validate_predicate(predicate)?;
        }
        let existing_for_subject = self
            .grants
            .iter()
            .filter(|candidate| candidate.subject == grant.subject)
            .count();
        if existing_for_subject >= ACL_MAX_GRANTS_PER_SUBJECT {
            return Err(LoomError::invalid("acl grant subject limit exceeded"));
        }
        self.grants.push(grant);
        Ok(())
    }

    pub fn revoke(&mut self, grant: &AclGrant) -> bool {
        let before = self.grants.len();
        self.grants.retain(|candidate| candidate != grant);
        self.grants.len() != before
    }

    pub fn revoke_one(&mut self, grant: &AclGrant) -> bool {
        let Some(idx) = self.grants.iter().position(|candidate| candidate == grant) else {
            return false;
        };
        self.grants.remove(idx);
        true
    }

    pub fn authorize_global_admin(
        &self,
        authenticated_mode: bool,
        principal: PrincipalId,
    ) -> Result<()> {
        self.authorize_global_admin_with_roles(authenticated_mode, principal, [])
    }

    pub fn authorize_global_admin_with_roles(
        &self,
        authenticated_mode: bool,
        principal: PrincipalId,
        roles: impl IntoIterator<Item = RoleId>,
    ) -> Result<()> {
        if !authenticated_mode {
            return Ok(());
        }

        let roles: BTreeSet<RoleId> = roles.into_iter().collect();
        let mut allowed = false;
        for grant in self.grants.iter().filter(|grant| {
            grant.workspace.is_none()
                && grant.domain.is_none()
                && grant.rights.contains(&AclRight::Admin)
                && grant.subject_matches(principal, &roles)
        }) {
            if grant.predicate.is_some() {
                return Err(LoomError::new(
                    Code::PermissionDenied,
                    "acl predicate evaluator unavailable",
                ));
            }
            match grant.effect {
                AclEffect::Deny => {
                    return Err(LoomError::new(Code::PermissionDenied, "acl denied"));
                }
                AclEffect::Allow => allowed = true,
            }
        }

        if allowed {
            Ok(())
        } else {
            Err(LoomError::new(Code::PermissionDenied, "acl default deny"))
        }
    }

    pub fn allow(
        &mut self,
        subject: AclSubject,
        workspace: Option<WorkspaceId>,
        facet: Option<FacetKind>,
        rights: impl IntoIterator<Item = AclRight>,
    ) -> Result<()> {
        self.grant(AclGrant {
            subject,
            workspace,
            domain: facet.map(Into::into),
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: rights.into_iter().collect(),
            effect: AclEffect::Allow,
            predicate: None,
        })
    }

    pub fn deny(
        &mut self,
        subject: AclSubject,
        workspace: Option<WorkspaceId>,
        facet: Option<FacetKind>,
        rights: impl IntoIterator<Item = AclRight>,
    ) -> Result<()> {
        self.grant(AclGrant {
            subject,
            workspace,
            domain: facet.map(Into::into),
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: rights.into_iter().collect(),
            effect: AclEffect::Deny,
            predicate: None,
        })
    }

    pub fn authorize(
        &self,
        authenticated_mode: bool,
        principal: PrincipalId,
        workspace: WorkspaceId,
        domain: impl Into<AclDomain>,
        right: AclRight,
    ) -> Result<()> {
        self.authorize_with_roles(authenticated_mode, principal, [], workspace, domain, right)
    }

    pub fn authorize_with_roles(
        &self,
        authenticated_mode: bool,
        principal: PrincipalId,
        roles: impl IntoIterator<Item = RoleId>,
        workspace: WorkspaceId,
        domain: impl Into<AclDomain>,
        right: AclRight,
    ) -> Result<()> {
        self.authorize_resource_with_roles(
            authenticated_mode,
            principal,
            roles,
            AclResource::all(workspace, domain),
            right,
        )
    }

    pub fn authorize_resource_with_roles(
        &self,
        authenticated_mode: bool,
        principal: PrincipalId,
        roles: impl IntoIterator<Item = RoleId>,
        resource: AclResource<'_>,
        right: AclRight,
    ) -> Result<()> {
        self.authorize_resource_with_roles_and_evaluator(
            authenticated_mode,
            principal,
            roles,
            resource,
            right,
            None,
        )
    }

    pub fn authorize_resource_with_roles_and_evaluator(
        &self,
        authenticated_mode: bool,
        principal: PrincipalId,
        roles: impl IntoIterator<Item = RoleId>,
        resource: AclResource<'_>,
        right: AclRight,
        evaluator: Option<&dyn AclPredicateEvaluator>,
    ) -> Result<()> {
        if !authenticated_mode {
            return Ok(());
        }

        let roles: BTreeSet<RoleId> = roles.into_iter().collect();
        let context = AclEvaluationContext {
            principal,
            roles: &roles,
            resource,
            right,
        };
        let mut allowed = false;
        for grant in self
            .grants
            .iter()
            .filter(|grant| grant.base_matches(principal, &roles, resource, right))
        {
            let predicate_matches = match &grant.predicate {
                None => true,
                Some(predicate) => match evaluator {
                    Some(evaluator) => evaluator.evaluate(predicate, &context)?,
                    None => {
                        return Err(LoomError::new(
                            Code::PermissionDenied,
                            "acl predicate evaluator unavailable",
                        ));
                    }
                },
            };
            match grant.effect {
                AclEffect::Deny if predicate_matches => {
                    return Err(LoomError::new(Code::PermissionDenied, "acl denied"));
                }
                AclEffect::Deny => {}
                AclEffect::Allow if predicate_matches => allowed = true,
                AclEffect::Allow => {}
            }
        }

        if allowed {
            Ok(())
        } else {
            Err(LoomError::new(Code::PermissionDenied, "acl default deny"))
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"LACL");
        put_uvarint(&mut out, self.grants.len() as u64);
        for grant in &self.grants {
            match grant.subject {
                AclSubject::Everyone => out.push(0),
                AclSubject::Principal(principal) => {
                    out.push(1);
                    out.extend_from_slice(principal.as_bytes());
                }
                AclSubject::Role(role) => {
                    out.push(2);
                    out.extend_from_slice(role.as_bytes());
                }
            }
            put_opt_id(&mut out, grant.workspace);
            match grant.domain {
                None => out.push(0),
                Some(domain) => {
                    out.push(1);
                    out.push(domain.stable_tag());
                }
            }
            put_opt_str(&mut out, grant.ref_glob.as_deref());
            out.push(effect_to_u8(grant.effect));
            put_uvarint(&mut out, grant.rights.len() as u64);
            for right in &grant.rights {
                out.push(right_to_u8(*right));
            }
            put_uvarint(&mut out, grant.scopes.len() as u64);
            for scope in &grant.scopes {
                put_scope(&mut out, scope);
            }
            put_opt_predicate(&mut out, grant.predicate.as_ref());
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut cur = Cur { bytes, pos: 0 };
        let magic = cur.take(4)?;
        if magic != b"LACL" {
            return Err(LoomError::corrupt("bad acl-store magic"));
        }
        let grant_count = cur.uvarint()?;
        let mut grants = Vec::new();
        for _ in 0..grant_count {
            let subject = match cur.u8()? {
                0 => AclSubject::Everyone,
                1 => AclSubject::Principal(PrincipalId::from_bytes(cur.take16()?)),
                2 => AclSubject::Role(RoleId::from_bytes(cur.take16()?)),
                other => {
                    return Err(LoomError::corrupt(format!(
                        "unknown acl subject tag {other:#x}"
                    )));
                }
            };
            let workspace = cur.opt_id()?;
            let domain = match cur.u8()? {
                0 => None,
                1 => {
                    let tag = cur.u8()?;
                    Some(AclDomain::from_stable_tag(tag).ok_or_else(|| {
                        LoomError::corrupt(format!("unknown ACL domain {tag:#x}"))
                    })?)
                }
                other => {
                    return Err(LoomError::corrupt(format!(
                        "unknown ACL domain tag {other:#x}"
                    )));
                }
            };
            let ref_glob = cur.opt_str()?;
            let effect = effect_from_u8(cur.u8()?)?;
            let rights_count = cur.uvarint()?;
            let mut rights = BTreeSet::new();
            for _ in 0..rights_count {
                rights.insert(right_from_u8(cur.u8()?)?);
            }
            let scopes = cur.scopes()?;
            let predicate = cur.opt_predicate()?;
            let grant = AclGrant {
                subject,
                workspace,
                domain,
                ref_glob,
                scopes,
                rights,
                effect,
                predicate,
            };
            if grant.rights.is_empty() {
                return Err(LoomError::corrupt("acl grant without rights"));
            }
            validate_scopes(&grant.scopes)
                .map_err(|_| LoomError::corrupt("acl grant scope bounds exceeded"))?;
            if let Some(predicate) = &grant.predicate {
                validate_predicate(predicate)
                    .map_err(|_| LoomError::corrupt("acl grant predicate bounds exceeded"))?;
            }
            grants.push(grant);
        }
        if cur.pos != bytes.len() {
            return Err(LoomError::corrupt("trailing acl-store bytes"));
        }
        Ok(Self { grants })
    }
}

impl<'a> AclResource<'a> {
    pub fn all(workspace: WorkspaceId, domain: impl Into<AclDomain>) -> Self {
        Self {
            workspace,
            domain: domain.into(),
            ref_name: None,
            scope: AclResourceScope::All,
        }
    }

    pub fn scoped(
        workspace: WorkspaceId,
        domain: impl Into<AclDomain>,
        ref_name: Option<&'a str>,
        scope: AclResourceScope<'a>,
    ) -> Self {
        Self {
            workspace,
            domain: domain.into(),
            ref_name,
            scope,
        }
    }
}

impl AclGrant {
    fn base_matches(
        &self,
        principal: PrincipalId,
        roles: &BTreeSet<RoleId>,
        resource: AclResource<'_>,
        right: AclRight,
    ) -> bool {
        self.subject_matches(principal, roles)
            && self.workspace.is_none_or(|ns| ns == resource.workspace)
            && self.domain.is_none_or(|domain| domain == resource.domain)
            && self.ref_matches(resource.ref_name)
            && self
                .scopes
                .iter()
                .any(|scope| scope.matches(resource.scope))
            && (self.rights.contains(&right) || self.rights.contains(&AclRight::Admin))
    }

    fn subject_matches(&self, principal: PrincipalId, roles: &BTreeSet<RoleId>) -> bool {
        match self.subject {
            AclSubject::Everyone => true,
            AclSubject::Principal(p) => p == principal,
            AclSubject::Role(role) => roles.contains(&role),
        }
    }

    fn ref_matches(&self, resource_ref: Option<&str>) -> bool {
        match (&self.ref_glob, resource_ref) {
            (None, _) => true,
            (Some(pattern), Some(value)) => glob_matches(pattern, value),
            (Some(_), None) => false,
        }
    }
}

impl AclScope {
    fn matches(&self, resource: AclResourceScope<'_>) -> bool {
        match (self, resource) {
            (AclScope::All, _) => true,
            (AclScope::Prefix { .. }, AclResourceScope::All) => false,
            (
                AclScope::Prefix { kind, prefix },
                AclResourceScope::Prefix {
                    kind: resource_kind,
                    value,
                },
            ) => scope_kind_covers(*kind, resource_kind) && value.starts_with(prefix),
        }
    }
}

fn scope_kind_covers(grant: AclScopeKind, resource: AclScopeKind) -> bool {
    grant == resource
        || (grant == AclScopeKind::Collection
            && matches!(
                resource,
                AclScopeKind::Collection
                    | AclScopeKind::Path
                    | AclScopeKind::Key
                    | AclScopeKind::Table
            ))
}

fn validate_scopes(scopes: &[AclScope]) -> Result<()> {
    if scopes.is_empty() {
        return Err(LoomError::invalid(
            "acl grant must include at least one scope",
        ));
    }
    if scopes.len() > ACL_MAX_SCOPES_PER_GRANT {
        return Err(LoomError::invalid("acl grant scope limit exceeded"));
    }
    for scope in scopes {
        if let AclScope::Prefix { prefix, .. } = scope
            && prefix.len() > ACL_MAX_SCOPE_PREFIX_LEN
        {
            return Err(LoomError::invalid("acl grant scope prefix too long"));
        }
    }
    Ok(())
}

fn validate_predicate(predicate: &AclPredicate) -> Result<()> {
    if predicate.language != ACL_PREDICATE_LANGUAGE_CEL {
        return Err(LoomError::invalid("acl predicate language must be cel"));
    }
    if predicate.expression.is_empty() {
        return Err(LoomError::invalid(
            "acl predicate expression must not be empty",
        ));
    }
    if predicate.expression.len() > ACL_MAX_PREDICATE_LEN {
        return Err(LoomError::invalid("acl predicate expression too long"));
    }
    Ok(())
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    glob_matches_at(pattern.as_bytes(), value.as_bytes())
}

fn glob_matches_at(pattern: &[u8], value: &[u8]) -> bool {
    match pattern.split_first() {
        None => value.is_empty(),
        Some((&b'*', rest)) => {
            glob_matches_at(rest, value)
                || value
                    .split_first()
                    .is_some_and(|(_, tail)| glob_matches_at(pattern, tail))
        }
        Some((&b'?', rest)) => value
            .split_first()
            .is_some_and(|(_, tail)| glob_matches_at(rest, tail)),
        Some((&literal, rest)) => value
            .split_first()
            .is_some_and(|(&head, tail)| head == literal && glob_matches_at(rest, tail)),
    }
}

fn put_uvarint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

fn put_opt_id(out: &mut Vec<u8>, value: Option<WorkspaceId>) {
    match value {
        None => out.push(0),
        Some(id) => {
            out.push(1);
            out.extend_from_slice(id.as_bytes());
        }
    }
}

fn put_opt_str(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            put_bytes(out, value.as_bytes());
        }
    }
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    put_uvarint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn put_scope(out: &mut Vec<u8>, scope: &AclScope) {
    match scope {
        AclScope::All => out.push(0),
        AclScope::Prefix { kind, prefix } => {
            out.push(1);
            out.push(scope_kind_to_u8(*kind));
            put_bytes(out, prefix);
        }
    }
}

fn put_opt_predicate(out: &mut Vec<u8>, predicate: Option<&AclPredicate>) {
    match predicate {
        None => out.push(0),
        Some(predicate) => {
            out.push(1);
            put_bytes(out, predicate.language.as_bytes());
            put_bytes(out, predicate.expression.as_bytes());
        }
    }
}

fn scope_kind_to_u8(kind: AclScopeKind) -> u8 {
    match kind {
        AclScopeKind::Ref => 0,
        AclScopeKind::Collection => 1,
        AclScopeKind::Path => 2,
        AclScopeKind::Key => 3,
        AclScopeKind::Table => 4,
        AclScopeKind::Exec => 5,
    }
}

fn scope_kind_from_u8(byte: u8) -> Result<AclScopeKind> {
    match byte {
        0 => Ok(AclScopeKind::Ref),
        1 => Ok(AclScopeKind::Collection),
        2 => Ok(AclScopeKind::Path),
        3 => Ok(AclScopeKind::Key),
        4 => Ok(AclScopeKind::Table),
        5 => Ok(AclScopeKind::Exec),
        other => Err(LoomError::corrupt(format!(
            "unknown acl scope kind {other:#x}"
        ))),
    }
}

fn effect_to_u8(effect: AclEffect) -> u8 {
    match effect {
        AclEffect::Allow => 0,
        AclEffect::Deny => 1,
    }
}

fn effect_from_u8(byte: u8) -> Result<AclEffect> {
    match byte {
        0 => Ok(AclEffect::Allow),
        1 => Ok(AclEffect::Deny),
        other => Err(LoomError::corrupt(format!("unknown acl effect {other:#x}"))),
    }
}

fn right_to_u8(right: AclRight) -> u8 {
    match right {
        AclRight::Read => 0,
        AclRight::Write => 1,
        AclRight::Advance => 2,
        AclRight::Merge => 3,
        AclRight::Execute => 4,
        AclRight::Admin => 5,
    }
}

fn right_from_u8(byte: u8) -> Result<AclRight> {
    match byte {
        0 => Ok(AclRight::Read),
        1 => Ok(AclRight::Write),
        2 => Ok(AclRight::Advance),
        3 => Ok(AclRight::Merge),
        4 => Ok(AclRight::Execute),
        5 => Ok(AclRight::Admin),
        other => Err(LoomError::corrupt(format!("unknown acl right {other:#x}"))),
    }
}

struct Cur<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| LoomError::corrupt("acl-store bytes truncated"))?;
        let out = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn uvarint(&mut self) -> Result<u64> {
        let mut shift = 0u32;
        let mut out = 0u64;
        loop {
            let byte = self.u8()?;
            out |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(out);
            }
            shift += 7;
            if shift >= 64 {
                return Err(LoomError::corrupt("acl varint too long"));
            }
        }
    }

    fn take16(&mut self) -> Result<[u8; 16]> {
        let mut out = [0u8; 16];
        out.copy_from_slice(self.take(16)?);
        Ok(out)
    }

    fn opt_id(&mut self) -> Result<Option<WorkspaceId>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(WorkspaceId::from_bytes(self.take16()?))),
            other => Err(LoomError::corrupt(format!(
                "unknown acl optional-id tag {other:#x}"
            ))),
        }
    }

    fn bytes(&mut self) -> Result<&'a [u8]> {
        let len = self.uvarint()?;
        let len: usize = len
            .try_into()
            .map_err(|_| LoomError::corrupt("acl byte string too long"))?;
        self.take(len)
    }

    fn opt_str(&mut self) -> Result<Option<String>> {
        match self.u8()? {
            0 => Ok(None),
            1 => {
                let bytes = self.bytes()?;
                let value = std::str::from_utf8(bytes)
                    .map_err(|_| LoomError::corrupt("acl ref glob is not utf-8"))?;
                Ok(Some(value.to_string()))
            }
            other => Err(LoomError::corrupt(format!(
                "unknown acl optional-string tag {other:#x}"
            ))),
        }
    }

    fn scopes(&mut self) -> Result<Vec<AclScope>> {
        let count = self.uvarint()?;
        let count: usize = count
            .try_into()
            .map_err(|_| LoomError::corrupt("acl scope count too large"))?;
        if count > ACL_MAX_SCOPES_PER_GRANT {
            return Err(LoomError::corrupt("acl grant scope limit exceeded"));
        }
        let mut scopes = Vec::with_capacity(count);
        for _ in 0..count {
            scopes.push(match self.u8()? {
                0 => AclScope::All,
                1 => {
                    let kind = scope_kind_from_u8(self.u8()?)?;
                    let prefix = self.bytes()?.to_vec();
                    AclScope::Prefix { kind, prefix }
                }
                other => {
                    return Err(LoomError::corrupt(format!(
                        "unknown acl scope tag {other:#x}"
                    )));
                }
            });
        }
        Ok(scopes)
    }

    fn string(&mut self) -> Result<String> {
        let bytes = self.bytes()?;
        let value = std::str::from_utf8(bytes)
            .map_err(|_| LoomError::corrupt("acl predicate string is not utf-8"))?;
        Ok(value.to_string())
    }

    fn opt_predicate(&mut self) -> Result<Option<AclPredicate>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(AclPredicate {
                language: self.string()?,
                expression: self.string()?,
            })),
            other => Err(LoomError::corrupt(format!(
                "unknown acl predicate tag {other:#x}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(byte: u8) -> PrincipalId {
        WorkspaceId::from_bytes([byte; 16])
    }

    #[test]
    fn unauthenticated_mode_bypasses_acl() {
        let acl = AclStore::new();
        acl.authorize(false, id(1), id(9), FacetKind::Files, AclRight::Write)
            .unwrap();
    }

    #[test]
    fn authenticated_mode_defaults_to_deny() {
        let acl = AclStore::new();
        assert_eq!(
            acl.authorize(true, id(1), id(9), FacetKind::Files, AclRight::Read)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
    }

    #[test]
    fn matching_allow_grants_access() {
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Principal(id(1)),
            Some(id(9)),
            Some(FacetKind::Kv),
            [AclRight::Read],
        )
        .unwrap();
        acl.authorize(true, id(1), id(9), FacetKind::Kv, AclRight::Read)
            .unwrap();
        assert_eq!(
            acl.authorize(true, id(1), id(9), FacetKind::Kv, AclRight::Write)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
    }

    #[test]
    fn encode_uses_canonical_domain_tags() {
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Everyone,
            None,
            Some(FacetKind::Vcs),
            [AclRight::Read],
        )
        .unwrap();
        assert_eq!(
            acl.encode(),
            [b"LACL".as_slice(), &[1, 0, 0, 1, 16, 0, 0, 1, 0, 1, 0, 0]].concat()
        );
    }

    #[test]
    fn deny_precedence_over_allow() {
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Everyone,
            Some(id(9)),
            Some(FacetKind::Kv),
            [AclRight::Write],
        )
        .unwrap();
        acl.deny(
            AclSubject::Principal(id(1)),
            Some(id(9)),
            Some(FacetKind::Kv),
            [AclRight::Write],
        )
        .unwrap();
        assert_eq!(
            acl.authorize(true, id(1), id(9), FacetKind::Kv, AclRight::Write)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
    }

    #[test]
    fn admin_grant_covers_all_rights() {
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Principal(id(1)),
            Some(id(9)),
            None,
            [AclRight::Admin],
        )
        .unwrap();
        acl.authorize(true, id(1), id(9), FacetKind::Files, AclRight::Merge)
            .unwrap();
        acl.authorize(true, id(1), id(9), FacetKind::Kv, AclRight::Execute)
            .unwrap();
    }

    #[test]
    fn matching_role_grant_allows_access() {
        let mut acl = AclStore::new();
        let role = id(44);
        acl.allow(
            AclSubject::Role(role),
            Some(id(9)),
            Some(FacetKind::Kv),
            [AclRight::Read],
        )
        .unwrap();
        acl.authorize_with_roles(true, id(1), [role], id(9), FacetKind::Kv, AclRight::Read)
            .unwrap();
        assert_eq!(
            acl.authorize_with_roles(true, id(1), [], id(9), FacetKind::Kv, AclRight::Read,)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
    }

    #[test]
    fn scoped_prefix_grant_matches_only_same_scope_kind() {
        let mut acl = AclStore::new();
        acl.grant(AclGrant {
            subject: AclSubject::Principal(id(1)),
            workspace: Some(id(9)),
            domain: Some(FacetKind::Files.into()),
            ref_glob: None,
            scopes: vec![AclScope::Prefix {
                kind: AclScopeKind::Path,
                prefix: b"reports/".to_vec(),
            }],
            rights: [AclRight::Read].into_iter().collect(),
            effect: AclEffect::Allow,
            predicate: None,
        })
        .unwrap();

        acl.authorize_resource_with_roles(
            true,
            id(1),
            [],
            AclResource::scoped(
                id(9),
                FacetKind::Files,
                None,
                AclResourceScope::Prefix {
                    kind: AclScopeKind::Path,
                    value: b"reports/q1.txt",
                },
            ),
            AclRight::Read,
        )
        .unwrap();
        assert_eq!(
            acl.authorize_resource_with_roles(
                true,
                id(1),
                [],
                AclResource::scoped(
                    id(9),
                    FacetKind::Files,
                    None,
                    AclResourceScope::Prefix {
                        kind: AclScopeKind::Key,
                        value: b"reports/q1.txt",
                    },
                ),
                AclRight::Read,
            )
            .unwrap_err()
            .code,
            Code::PermissionDenied
        );
    }

    #[test]
    fn scoped_prefix_grant_does_not_authorize_broad_resource() {
        let mut acl = AclStore::new();
        acl.grant(AclGrant {
            subject: AclSubject::Principal(id(1)),
            workspace: Some(id(9)),
            domain: Some(FacetKind::Files.into()),
            ref_glob: None,
            scopes: vec![AclScope::Prefix {
                kind: AclScopeKind::Path,
                prefix: b"reports/".to_vec(),
            }],
            rights: [AclRight::Read].into_iter().collect(),
            effect: AclEffect::Allow,
            predicate: None,
        })
        .unwrap();

        assert_eq!(
            acl.authorize(true, id(1), id(9), FacetKind::Files, AclRight::Read)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
    }

    #[test]
    fn scoped_deny_precedence_over_allow() {
        let mut acl = AclStore::new();
        acl.grant(AclGrant {
            subject: AclSubject::Everyone,
            workspace: Some(id(9)),
            domain: Some(FacetKind::Kv.into()),
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: [AclRight::Write].into_iter().collect(),
            effect: AclEffect::Allow,
            predicate: None,
        })
        .unwrap();
        acl.grant(AclGrant {
            subject: AclSubject::Principal(id(1)),
            workspace: Some(id(9)),
            domain: Some(FacetKind::Kv.into()),
            ref_glob: None,
            scopes: vec![AclScope::Prefix {
                kind: AclScopeKind::Key,
                prefix: b"secrets/".to_vec(),
            }],
            rights: [AclRight::Write].into_iter().collect(),
            effect: AclEffect::Deny,
            predicate: None,
        })
        .unwrap();

        assert_eq!(
            acl.authorize_resource_with_roles(
                true,
                id(1),
                [],
                AclResource::scoped(
                    id(9),
                    FacetKind::Kv,
                    None,
                    AclResourceScope::Prefix {
                        kind: AclScopeKind::Key,
                        value: b"secrets/token",
                    },
                ),
                AclRight::Write,
            )
            .unwrap_err()
            .code,
            Code::PermissionDenied
        );
    }

    #[test]
    fn ref_glob_scopes_grant_to_matching_refs() {
        let mut acl = AclStore::new();
        acl.grant(AclGrant {
            subject: AclSubject::Principal(id(1)),
            workspace: Some(id(9)),
            domain: Some(FacetKind::Vcs.into()),
            ref_glob: Some("branch/release-*".to_string()),
            scopes: vec![AclScope::All],
            rights: [AclRight::Merge].into_iter().collect(),
            effect: AclEffect::Allow,
            predicate: None,
        })
        .unwrap();

        acl.authorize_resource_with_roles(
            true,
            id(1),
            [],
            AclResource::scoped(
                id(9),
                FacetKind::Vcs,
                Some("branch/release-1"),
                AclResourceScope::All,
            ),
            AclRight::Merge,
        )
        .unwrap();
        assert_eq!(
            acl.authorize_resource_with_roles(
                true,
                id(1),
                [],
                AclResource::scoped(
                    id(9),
                    FacetKind::Vcs,
                    Some("branch/dev"),
                    AclResourceScope::All,
                ),
                AclRight::Merge,
            )
            .unwrap_err()
            .code,
            Code::PermissionDenied
        );
        assert_eq!(
            acl.authorize(true, id(1), id(9), FacetKind::Vcs, AclRight::Merge)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
    }

    #[test]
    fn acl_store_codec_round_trips_scopes_and_ref_globs() {
        let mut acl = AclStore::new();
        acl.grant(AclGrant {
            subject: AclSubject::Principal(id(1)),
            workspace: Some(id(9)),
            domain: Some(FacetKind::Sql.into()),
            ref_glob: Some("branch/main".to_string()),
            scopes: vec![AclScope::Prefix {
                kind: AclScopeKind::Table,
                prefix: b"sales.".to_vec(),
            }],
            rights: [AclRight::Read, AclRight::Write].into_iter().collect(),
            effect: AclEffect::Allow,
            predicate: None,
        })
        .unwrap();

        let decoded = AclStore::decode(&acl.encode()).unwrap();
        assert_eq!(decoded.grants(), acl.grants());
        decoded
            .authorize_resource_with_roles(
                true,
                id(1),
                [],
                AclResource::scoped(
                    id(9),
                    FacetKind::Sql,
                    Some("branch/main"),
                    AclResourceScope::Prefix {
                        kind: AclScopeKind::Table,
                        value: b"sales.orders",
                    },
                ),
                AclRight::Read,
            )
            .unwrap();
    }

    #[test]
    fn acl_store_codec_round_trips_cel_predicates() {
        let mut acl = AclStore::new();
        let predicate = AclPredicate::cel("principal == 'alice'").unwrap();
        acl.grant(AclGrant {
            subject: AclSubject::Principal(id(1)),
            workspace: Some(id(9)),
            domain: Some(FacetKind::Files.into()),
            ref_glob: None,
            scopes: vec![AclScope::Prefix {
                kind: AclScopeKind::Path,
                prefix: b"reports/".to_vec(),
            }],
            rights: [AclRight::Read].into_iter().collect(),
            effect: AclEffect::Allow,
            predicate: Some(predicate.clone()),
        })
        .unwrap();

        let decoded = AclStore::decode(&acl.encode()).unwrap();
        assert_eq!(decoded.grants(), acl.grants());
        assert_eq!(decoded.grants()[0].predicate.as_ref(), Some(&predicate));
    }

    #[test]
    fn acl_predicates_are_validated() {
        assert_eq!(
            AclPredicate::cel("").unwrap_err().code,
            Code::InvalidArgument
        );
        assert_eq!(
            AclPredicate::cel("x".repeat(ACL_MAX_PREDICATE_LEN + 1))
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );

        let mut acl = AclStore::new();
        assert_eq!(
            acl.grant(AclGrant {
                subject: AclSubject::Principal(id(1)),
                workspace: Some(id(9)),
                domain: Some(FacetKind::Files.into()),
                ref_glob: None,
                scopes: vec![AclScope::All],
                rights: [AclRight::Read].into_iter().collect(),
                effect: AclEffect::Allow,
                predicate: Some(AclPredicate {
                    language: "other".to_string(),
                    expression: "true".to_string(),
                }),
            })
            .unwrap_err()
            .code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn acl_allow_predicates_fail_closed_until_evaluated() {
        let mut acl = AclStore::new();
        acl.grant(AclGrant {
            subject: AclSubject::Principal(id(1)),
            workspace: Some(id(9)),
            domain: Some(FacetKind::Files.into()),
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: [AclRight::Read].into_iter().collect(),
            effect: AclEffect::Allow,
            predicate: Some(AclPredicate::cel("true").unwrap()),
        })
        .unwrap();

        assert_eq!(
            acl.authorize(true, id(1), id(9), FacetKind::Files, AclRight::Read)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
    }

    #[test]
    fn acl_deny_predicates_fail_closed_as_deny() {
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Principal(id(1)),
            Some(id(9)),
            Some(FacetKind::Files),
            [AclRight::Read],
        )
        .unwrap();
        acl.grant(AclGrant {
            subject: AclSubject::Principal(id(1)),
            workspace: Some(id(9)),
            domain: Some(FacetKind::Files.into()),
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: [AclRight::Read].into_iter().collect(),
            effect: AclEffect::Deny,
            predicate: Some(AclPredicate::cel("false").unwrap()),
        })
        .unwrap();

        assert_eq!(
            acl.authorize(true, id(1), id(9), FacetKind::Files, AclRight::Read)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
    }

    #[derive(Debug)]
    struct LiteralEvaluator;

    impl AclPredicateEvaluator for LiteralEvaluator {
        fn evaluate(
            &self,
            predicate: &AclPredicate,
            context: &AclEvaluationContext<'_>,
        ) -> Result<bool> {
            assert_eq!(context.principal, id(1));
            assert_eq!(context.resource.domain, AclDomain::Files);
            match predicate.expression.as_str() {
                "true" => Ok(true),
                "false" => Ok(false),
                _ => Err(LoomError::invalid("unsupported test predicate")),
            }
        }
    }

    #[test]
    fn acl_allow_predicates_are_authorized_by_evaluator() {
        let mut acl = AclStore::new();
        acl.grant(AclGrant {
            subject: AclSubject::Principal(id(1)),
            workspace: Some(id(9)),
            domain: Some(FacetKind::Files.into()),
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: [AclRight::Read].into_iter().collect(),
            effect: AclEffect::Allow,
            predicate: Some(AclPredicate::cel("true").unwrap()),
        })
        .unwrap();

        acl.authorize_resource_with_roles_and_evaluator(
            true,
            id(1),
            [],
            AclResource::all(id(9), FacetKind::Files),
            AclRight::Read,
            Some(&LiteralEvaluator),
        )
        .unwrap();
    }

    #[test]
    fn acl_deny_predicates_only_apply_when_evaluator_matches() {
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Principal(id(1)),
            Some(id(9)),
            Some(FacetKind::Files),
            [AclRight::Read],
        )
        .unwrap();
        acl.grant(AclGrant {
            subject: AclSubject::Principal(id(1)),
            workspace: Some(id(9)),
            domain: Some(FacetKind::Files.into()),
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: [AclRight::Read].into_iter().collect(),
            effect: AclEffect::Deny,
            predicate: Some(AclPredicate::cel("false").unwrap()),
        })
        .unwrap();

        acl.authorize_resource_with_roles_and_evaluator(
            true,
            id(1),
            [],
            AclResource::all(id(9), FacetKind::Files),
            AclRight::Read,
            Some(&LiteralEvaluator),
        )
        .unwrap();
    }

    #[test]
    fn acl_store_rejects_scope_and_subject_limit_violations() {
        let mut acl = AclStore::new();
        let too_many_scopes = vec![AclScope::All; ACL_MAX_SCOPES_PER_GRANT + 1];
        assert_eq!(
            acl.grant(AclGrant {
                subject: AclSubject::Principal(id(1)),
                workspace: Some(id(9)),
                domain: Some(FacetKind::Kv.into()),
                ref_glob: None,
                scopes: too_many_scopes,
                rights: [AclRight::Read].into_iter().collect(),
                effect: AclEffect::Allow,
                predicate: None,
            })
            .unwrap_err()
            .code,
            Code::InvalidArgument
        );

        let long_prefix = vec![b'x'; ACL_MAX_SCOPE_PREFIX_LEN + 1];
        assert_eq!(
            acl.grant(AclGrant {
                subject: AclSubject::Principal(id(1)),
                workspace: Some(id(9)),
                domain: Some(FacetKind::Kv.into()),
                ref_glob: None,
                scopes: vec![AclScope::Prefix {
                    kind: AclScopeKind::Key,
                    prefix: long_prefix,
                }],
                rights: [AclRight::Read].into_iter().collect(),
                effect: AclEffect::Allow,
                predicate: None,
            })
            .unwrap_err()
            .code,
            Code::InvalidArgument
        );

        for idx in 0..ACL_MAX_GRANTS_PER_SUBJECT {
            acl.grant(AclGrant {
                subject: AclSubject::Principal(id(2)),
                workspace: Some(WorkspaceId::from_bytes([idx as u8; 16])),
                domain: Some(FacetKind::Kv.into()),
                ref_glob: None,
                scopes: vec![AclScope::All],
                rights: [AclRight::Read].into_iter().collect(),
                effect: AclEffect::Allow,
                predicate: None,
            })
            .unwrap();
        }
        assert_eq!(
            acl.grant(AclGrant {
                subject: AclSubject::Principal(id(2)),
                workspace: Some(id(10)),
                domain: Some(FacetKind::Kv.into()),
                ref_glob: None,
                scopes: vec![AclScope::All],
                rights: [AclRight::Read].into_iter().collect(),
                effect: AclEffect::Allow,
                predicate: None,
            })
            .unwrap_err()
            .code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn acl_store_codec_round_trips() {
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Principal(id(1)),
            Some(id(9)),
            Some(FacetKind::Kv),
            [AclRight::Read, AclRight::Write],
        )
        .unwrap();
        acl.deny(
            AclSubject::Everyone,
            Some(id(9)),
            Some(FacetKind::Kv),
            [AclRight::Write],
        )
        .unwrap();
        acl.allow(
            AclSubject::Role(id(44)),
            Some(id(9)),
            Some(FacetKind::Files),
            [AclRight::Read],
        )
        .unwrap();

        let decoded = AclStore::decode(&acl.encode()).unwrap();
        assert_eq!(decoded.grants().len(), 3);
        decoded
            .authorize(true, id(1), id(9), FacetKind::Kv, AclRight::Read)
            .unwrap();
        decoded
            .authorize_with_roles(
                true,
                id(1),
                [id(44)],
                id(9),
                FacetKind::Files,
                AclRight::Read,
            )
            .unwrap();
        assert_eq!(
            decoded
                .authorize(true, id(1), id(9), FacetKind::Kv, AclRight::Write)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
    }

    #[test]
    fn product_domains_do_not_inherit_facet_grants() {
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Principal(id(1)),
            Some(id(9)),
            Some(FacetKind::Vcs),
            [AclRight::Read, AclRight::Write],
        )
        .unwrap();
        acl.grant(AclGrant {
            subject: AclSubject::Principal(id(1)),
            workspace: Some(id(9)),
            domain: Some(AclDomain::Tickets),
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: [AclRight::Read].into_iter().collect(),
            effect: AclEffect::Allow,
            predicate: None,
        })
        .unwrap();

        acl.authorize(true, id(1), id(9), FacetKind::Vcs, AclRight::Write)
            .unwrap();
        acl.authorize(true, id(1), id(9), AclDomain::Tickets, AclRight::Read)
            .unwrap();
        assert_eq!(
            acl.authorize(true, id(1), id(9), AclDomain::Tickets, AclRight::Write)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
        assert_eq!(
            acl.authorize(true, id(1), id(9), AclDomain::Pages, AclRight::Read)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
    }

    #[test]
    fn acl_codec_round_trips_product_domains() {
        let mut acl = AclStore::new();
        acl.grant(AclGrant {
            subject: AclSubject::Everyone,
            workspace: Some(id(9)),
            domain: Some(AclDomain::Meetings),
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: [AclRight::Read].into_iter().collect(),
            effect: AclEffect::Allow,
            predicate: None,
        })
        .unwrap();

        let encoded = acl.encode();
        assert_eq!(&encoded[..4], b"LACL");
        let decoded = AclStore::decode(&encoded).unwrap();
        assert_eq!(decoded.grants()[0].domain, Some(AclDomain::Meetings));
    }
}
