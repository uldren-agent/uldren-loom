//! THROWAWAY principal + stored-grant evaluator (workspace-level and fine-grained).
//!
//! It mirrors the compute layer's capability grammar (`prototypes/loom-compute/src/access.rs`:
//! `Facet` + `Scope` + mode) and extends it into the access-control model:
//! - the `Right` set widens the program-facing `Mode`: Read, Write, Advance, Merge,
//!   Admin, Exec;
//! - a `Grant` adds `effect` (Allow/Deny), a `subject` (principal or role), a `workspace` selector,
//!   and a `ref_glob`, on top of `facet` + `scope`;
//! - the policy enforcement point evaluates with **deny-precedence** and **default-deny**,
//!   so a broad Deny beats a narrow Allow (specificity does not override a deny);
//! - a cross-workspace read requires a Read grant on every workspace it touches.
//!
//! `main` runs the worked examples as assertions; `cargo test` runs the same as unit tests.
//! Stubbed: real principals/credentials, the stored system workspace, and conditional CEL
//! predicates - this slice is the matching core only.

use std::collections::BTreeMap;

// ---- Grammar (mirrors loom-compute/src/access.rs, widened for principals) ----

// The complete facet catalog, mirrored from loom-compute. The worked examples exercise
// only a few; the rest are present so the grammar matches the real one.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Facet {
    Files,
    Sql,
    KeyValue,
    Document,
    Graph,
    Vector,
    Columnar,
    Log,
    TimeSeries,
    Cas,
    Ledger,
}

/// The right set: the program-facing Read/Write/ReadWrite mode widened with the
/// version-control and compute operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Right {
    Read,
    Write,
    Advance,
    Merge,
    Admin,
    Exec,
}

/// Scope within a facet; `Prefix` meaning is facet-specific (path, key, table, series), exactly as
/// loom-compute `Scope`.
#[derive(Clone, Debug)]
enum Scope {
    All,
    Prefix(String),
}

impl Scope {
    fn matches(&self, target: &str) -> bool {
        match self {
            Scope::All => true,
            Scope::Prefix(p) => target.starts_with(p.as_str()),
        }
    }
}

/// Which workspace(s) a grant covers. Simplified to names for the slice.
#[derive(Clone, Debug)]
enum NsSel {
    Ns(String),
    AllOfType(String), // a workspace type, e.g. "files"; matched against (type, name)
    All,
}

#[derive(Clone, Copy, Debug)]
enum FacetSel {
    One(Facet),
    AllFacets,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Effect {
    Allow,
    Deny,
}

#[derive(Clone, Debug)]
enum Subject {
    Principal(u32),
    Role(u32),
}

/// A grant.
#[derive(Clone, Debug)]
struct Grant {
    effect: Effect,
    subject: Subject,
    workspace: NsSel,
    ref_glob: String, // "*" for all refs
    scope: Scope,
    facet: FacetSel,
    rights: Vec<Right>,
}

// ---- Principals and the store (credentials are stubbed; identity reduced to ids/roles) ----

struct Principal {
    id: u32,
    name: String,
    roles: Vec<u32>,
    enabled: bool,
}

struct Role {
    id: u32,
}

struct AuthStore {
    principals: Vec<Principal>,
    roles: Vec<Role>,
    grants: Vec<Grant>, // direct and role grants together; subject distinguishes them
}

/// A request the policy enforcement point decides.
struct Request<'a> {
    principal: u32,
    right: Right,
    ns_type: &'a str,
    ns_name: &'a str,
    ref_name: &'a str,
    target: &'a str, // path/key within the facet
    facet: Facet,
}

/// Minimal glob: `*` matches any sequence, `?` any one byte. Used for `ref_glob`.
fn glob_match(pat: &str, s: &str) -> bool {
    let (p, t) = (pat.as_bytes(), s.as_bytes());
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == b'?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

impl AuthStore {
    fn principal(&self, id: u32) -> Option<&Principal> {
        self.principals.iter().find(|p| p.id == id)
    }

    /// Whether a grant's subject applies to this principal (directly or via one of its roles).
    fn subject_applies(&self, g: &Grant, p: &Principal) -> bool {
        match &g.subject {
            Subject::Principal(pid) => *pid == p.id,
            Subject::Role(rid) => p.roles.contains(rid),
        }
    }

    fn ns_applies(sel: &NsSel, ty: &str, name: &str) -> bool {
        match sel {
            NsSel::All => true,
            NsSel::AllOfType(t) => t == ty,
            NsSel::Ns(n) => n == name,
        }
    }

    fn facet_applies(sel: FacetSel, f: Facet) -> bool {
        match sel {
            FacetSel::AllFacets => true,
            FacetSel::One(x) => x == f,
        }
    }

    fn applicable(&self, g: &Grant, p: &Principal, req: &Request) -> bool {
        self.subject_applies(g, p)
            && Self::ns_applies(&g.workspace, req.ns_type, req.ns_name)
            && glob_match(&g.ref_glob, req.ref_name)
            && g.scope.matches(req.target)
            && Self::facet_applies(g.facet, req.facet)
            && g.rights.iter().any(|r| *r == req.right)
    }

    /// The policy enforcement point: deny-precedence, then allow, else default-deny.
    fn authorize(&self, req: &Request) -> bool {
        let Some(p) = self.principal(req.principal) else {
            return false; // unknown principal: fail closed
        };
        if !p.enabled {
            return false; // disabled principal: fail closed (triggers rely on this)
        }
        let mut allowed = false;
        for g in &self.grants {
            if self.applicable(g, p, req) {
                match g.effect {
                    Effect::Deny => return false, // any applicable deny wins
                    Effect::Allow => allowed = true,
                }
            }
        }
        allowed
    }

    /// A cross-workspace read: allowed only if every touched workspace permits Read and
    /// none denies it. Returns true iff all pass.
    fn authorize_cross_ns_read(&self, principal: u32, workspaces: &[(&str, &str)]) -> bool {
        workspaces.iter().all(|(ty, name)| {
            self.authorize(&Request {
                principal,
                right: Right::Read,
                ns_type: ty,
                ns_name: name,
                ref_name: "branch/main",
                target: "",
                facet: Facet::Files,
            })
        })
    }
}

// ---- Convenience builders for readable grant sets ----

fn allow(subject: Subject, workspace: NsSel, ref_glob: &str, scope: Scope, facet: FacetSel, rights: &[Right]) -> Grant {
    Grant { effect: Effect::Allow, subject, workspace, ref_glob: ref_glob.into(), scope, facet, rights: rights.to_vec() }
}
fn deny(subject: Subject, workspace: NsSel, ref_glob: &str, scope: Scope, facet: FacetSel, rights: &[Right]) -> Grant {
    Grant { effect: Effect::Deny, subject, workspace, ref_glob: ref_glob.into(), scope, facet, rights: rights.to_vec() }
}

fn req<'a>(principal: u32, right: Right, ns_type: &'a str, ns_name: &'a str, ref_name: &'a str, target: &'a str, facet: Facet) -> Request<'a> {
    Request { principal, right, ns_type, ns_name, ref_name, target, facet }
}

// ---- The worked examples ----

/// Builds a store with one analyst principal (id 1) and the worked-example grants, plus a role demo.
fn example_store() -> AuthStore {
    let analyst = Subject::Principal(1);
    let grants = vec![
        // 1: read-write the `docs` workspace except the `secrets/` path.
        allow(analyst.clone(), NsSel::Ns("docs".into()), "*", Scope::All, FacetSel::AllFacets, &[Right::Read, Right::Write]),
        deny(analyst.clone(), NsSel::Ns("docs".into()), "*", Scope::Prefix("secrets/".into()), FacetSel::AllFacets, &[Right::Read, Right::Write]),
        // 2: read-only on main, read-write on dev, in the `code` workspace.
        allow(analyst.clone(), NsSel::Ns("code".into()), "branch/dev", Scope::All, FacetSel::AllFacets, &[Right::Read, Right::Write]),
        allow(analyst.clone(), NsSel::Ns("code".into()), "branch/main", Scope::All, FacetSel::AllFacets, &[Right::Read]),
        // 4: may run programs against `reports/` but not write directly, in `analytics`.
        allow(analyst.clone(), NsSel::Ns("analytics".into()), "*", Scope::Prefix("reports/".into()), FacetSel::AllFacets, &[Right::Exec]),
        // 5: deny beats allow - a broad deny over the `audit` workspace plus a narrow allow under it.
        deny(analyst.clone(), NsSel::Ns("audit".into()), "*", Scope::All, FacetSel::AllFacets, &[Right::Read]),
        allow(analyst.clone(), NsSel::Ns("audit".into()), "*", Scope::Prefix("public/".into()), FacetSel::AllFacets, &[Right::Read]),
        // 6: cross-workspace read - Read on `a` and `b`, nothing on `c`.
        allow(analyst.clone(), NsSel::Ns("a".into()), "*", Scope::All, FacetSel::AllFacets, &[Right::Read]),
        allow(analyst.clone(), NsSel::Ns("b".into()), "*", Scope::All, FacetSel::AllFacets, &[Right::Read]),
        // Role demo: role 7 may read the `vector` facet of `mem`; principal 1 holds role 7.
        allow(Subject::Role(7), NsSel::Ns("mem".into()), "*", Scope::All, FacetSel::One(Facet::Vector), &[Right::Read]),
        // Role 7 may also advance and merge `dev` on any files workspace (AllOfType + version-control rights).
        allow(Subject::Role(7), NsSel::AllOfType("files".into()), "branch/dev", Scope::All, FacetSel::AllFacets, &[Right::Advance, Right::Merge]),
        // Root (principal 0) may do everything everywhere (NsSel::All, the full right set incl. Admin).
        allow(Subject::Principal(0), NsSel::All, "*", Scope::All, FacetSel::AllFacets, &[Right::Read, Right::Write, Right::Advance, Right::Merge, Right::Admin, Right::Exec]),
    ];
    AuthStore {
        principals: vec![
            Principal { id: 0, name: "root".into(), roles: vec![], enabled: true },
            Principal { id: 1, name: "analyst".into(), roles: vec![7], enabled: true },
        ],
        roles: vec![Role { id: 7 }],
        grants,
    }
}

fn main() {
    let s = example_store();

    // A table of (label, decision, expected) so the demo doubles as a self-check.
    let mut checks: BTreeMap<&str, (bool, bool)> = BTreeMap::new();
    checks.insert("write docs/reports (allow)", (s.authorize(&req(1, Right::Write, "files", "docs", "branch/main", "reports/q3", Facet::Files)), true));
    checks.insert("write docs/secrets (deny wins)", (s.authorize(&req(1, Right::Write, "files", "docs", "branch/main", "secrets/key", Facet::Files)), false));
    checks.insert("write code dev (allow)", (s.authorize(&req(1, Right::Write, "files", "code", "branch/dev", "src/lib.rs", Facet::Files)), true));
    checks.insert("write code main (default-deny)", (s.authorize(&req(1, Right::Write, "files", "code", "branch/main", "src/lib.rs", Facet::Files)), false));
    checks.insert("read code main (allow)", (s.authorize(&req(1, Right::Read, "files", "code", "branch/main", "src/lib.rs", Facet::Files)), true));
    checks.insert("exec analytics/reports (allow)", (s.authorize(&req(1, Right::Exec, "files", "analytics", "branch/main", "reports/job", Facet::Files)), true));
    checks.insert("write analytics/reports (default-deny)", (s.authorize(&req(1, Right::Write, "files", "analytics", "branch/main", "reports/job", Facet::Files)), false));
    checks.insert("read audit/public (deny beats allow)", (s.authorize(&req(1, Right::Read, "files", "audit", "branch/main", "public/report", Facet::Files)), false));
    checks.insert("read mem vector via role (allow)", (s.authorize(&req(1, Right::Read, "vector", "mem", "branch/main", "", Facet::Vector)), true));
    checks.insert("read mem files (no role grant, default-deny)", (s.authorize(&req(1, Right::Read, "files", "mem", "branch/main", "x", Facet::Files)), false));

    // Version-control and admin rights exercised below.
    checks.insert("analyst advance code dev via role (allow)", (s.authorize(&req(1, Right::Advance, "files", "code", "branch/dev", "", Facet::Files)), true));
    checks.insert("analyst merge code main (default-deny)", (s.authorize(&req(1, Right::Merge, "files", "code", "branch/main", "", Facet::Files)), false));
    checks.insert("analyst admin (default-deny)", (s.authorize(&req(1, Right::Admin, "files", "docs", "branch/main", "", Facet::Files)), false));
    checks.insert("root admin anywhere (allow)", (s.authorize(&req(0, Right::Admin, "ledger", "audit", "branch/main", "", Facet::Ledger)), true));

    println!("== authz evaluator slice for 0027/0028 ==");
    println!(
        "store: {} principals, roles registered {:?}",
        s.principals.len(),
        s.roles.iter().map(|r| r.id).collect::<Vec<_>>()
    );
    for p in &s.principals {
        println!("  principal {} '{}' roles={:?} enabled={}", p.id, p.name, p.roles, p.enabled);
    }
    println!();
    let mut all_ok = true;
    for (label, (got, want)) in &checks {
        let ok = got == want;
        all_ok &= ok;
        println!("  [{}] {:<42} got={} want={}", if ok { "ok" } else { "XX" }, label, got, want);
    }

    // Cross-workspace read.
    let ab = s.authorize_cross_ns_read(1, &[("files", "a"), ("files", "b")]);
    let abc = s.authorize_cross_ns_read(1, &[("files", "a"), ("files", "b"), ("files", "c")]);
    println!("\n  cross-ns read {{a,b}} (both granted) = {} (want true)", ab);
    println!("  cross-ns read {{a,b,c}} (c ungranted)  = {} (want false)", abc);
    all_ok &= ab && !abc;

    assert!(all_ok, "an authorization check did not match its expected decision");
    println!("\nall authorization checks held.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_beats_allow() {
        let s = example_store();
        // broad deny on `audit` plus narrow allow under `public/` -> read denied.
        assert!(!s.authorize(&req(1, Right::Read, "files", "audit", "branch/main", "public/x", Facet::Files)));
    }

    #[test]
    fn default_deny() {
        let s = example_store();
        // no grant mentions Advance anywhere -> denied.
        assert!(!s.authorize(&req(1, Right::Advance, "files", "docs", "branch/main", "reports/x", Facet::Files)));
    }

    #[test]
    fn ref_scoped() {
        let s = example_store();
        assert!(s.authorize(&req(1, Right::Write, "files", "code", "branch/dev", "x", Facet::Files)));
        assert!(!s.authorize(&req(1, Right::Write, "files", "code", "branch/main", "x", Facet::Files)));
    }

    #[test]
    fn facet_scoped_via_role() {
        let s = example_store();
        assert!(s.authorize(&req(1, Right::Read, "vector", "mem", "branch/main", "", Facet::Vector)));
        assert!(!s.authorize(&req(1, Right::Read, "files", "mem", "branch/main", "x", Facet::Files)));
    }

    #[test]
    fn cross_ns_read_needs_all() {
        let s = example_store();
        assert!(s.authorize_cross_ns_read(1, &[("files", "a"), ("files", "b")]));
        assert!(!s.authorize_cross_ns_read(1, &[("files", "a"), ("files", "c")]));
    }

    #[test]
    fn disabled_principal_fails_closed() {
        let mut s = example_store();
        s.principals.iter_mut().find(|p| p.id == 1).unwrap().enabled = false;
        assert!(!s.authorize(&req(1, Right::Read, "files", "code", "branch/main", "x", Facet::Files)));
    }

    #[test]
    fn glob() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("branch/*", "branch/dev"));
        assert!(!glob_match("branch/main", "branch/dev"));
    }
}
