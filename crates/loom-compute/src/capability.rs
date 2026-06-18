//! The capability model a program declares and the host enforces.
//!
//! A grant is fine-grained: a [`Capability`] (which facet) plus a [`Scope`] (which part of it) plus
//! a [`Mode`] (read, write, or both). A program's manifest carries a [`GrantSet`]; the engine grants
//! exactly those and checks each operation against them. The vocabulary is public, since a caller
//! authors it; the execution surface that consumes it stays private to the engine.

pub type Capability = loom_core::FacetKind;

/// Whether a program may be granted this facet. `Vcs` and `Program` are excluded: version-control
/// internals are the gate's responsibility, and nested program execution is not promoted. Every other
/// [`Capability`] is grantable.
pub fn is_program_grantable(facet: Capability) -> bool {
    !matches!(facet, Capability::Vcs | Capability::Program)
}

/// The program-grantable facets, in `FacetKind::ALL` declaration order. Derived from the single facet
/// source of truth so a newly added facet is grantable by default and the set cannot silently drift.
pub fn grantable_facets() -> impl Iterator<Item = Capability> {
    Capability::ALL
        .into_iter()
        .filter(|facet| is_program_grantable(*facet))
}

/// The access mode a grant allows. An operation requests [`Mode::Read`] or [`Mode::Write`]; a grant
/// of [`Mode::ReadWrite`] covers both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Read,
    Write,
    ReadWrite,
}

impl Mode {
    /// Whether a grant of `self` permits an operation requesting `requested`.
    pub fn covers(self, requested: Mode) -> bool {
        matches!(
            (self, requested),
            (Mode::ReadWrite, _) | (Mode::Read, Mode::Read) | (Mode::Write, Mode::Write)
        )
    }

    /// Stable tag used in the manifest encoding.
    pub fn as_u8(self) -> u8 {
        match self {
            Mode::Read => 0,
            Mode::Write => 1,
            Mode::ReadWrite => 2,
        }
    }

    /// Inverse of [`Mode::as_u8`]; `None` for an unknown tag.
    pub fn from_u8(tag: u8) -> Option<Self> {
        Some(match tag {
            0 => Mode::Read,
            1 => Mode::Write,
            2 => Mode::ReadWrite,
            _ => return None,
        })
    }
}

/// The part of a facet a grant covers. `Prefix` meaning is facet-specific: a path prefix for files,
/// a key prefix for key-value, a table-name prefix for relational, a series prefix for time-series.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    All,
    Prefix(String),
}

impl Scope {
    /// Whether this scope covers `target`.
    pub fn matches(&self, target: &str) -> bool {
        match self {
            Scope::All => true,
            Scope::Prefix(prefix) => target.starts_with(prefix.as_str()),
        }
    }
}

/// One fine-grained capability: a facet, the allowed mode, and the scope(s) within it. `scopes` is
/// one or more scopes; the grant applies if the resource matches ANY of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub facet: Capability,
    pub mode: Mode,
    pub scopes: Vec<Scope>,
}

/// The grants a program declared in its manifest and the host approved. Grants are held in a
/// canonical order, so a grant set's identity does not depend on the order it was declared in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GrantSet {
    pub grants: Vec<Grant>,
}

impl GrantSet {
    /// Build a grant set, normalizing the grants into canonical order.
    pub fn new(mut grants: Vec<Grant>) -> Self {
        // Canonicalize: sort each grant's scopes, then sort the grants, so a grant set's identity is
        // independent of declared order.
        for grant in &mut grants {
            grant
                .scopes
                .sort_by(|a, b| scope_order(a).cmp(&scope_order(b)));
        }
        grants.sort_by(|a, b| {
            (
                a.facet.stable_tag(),
                a.mode.as_u8(),
                scopes_order(&a.scopes),
            )
                .cmp(&(
                    b.facet.stable_tag(),
                    b.mode.as_u8(),
                    scopes_order(&b.scopes),
                ))
        });
        Self { grants }
    }

    /// A whole-facet, read-write grant for every program-grantable facet. Convenience for tests and
    /// full-trust hosts. Derived from [`grantable_facets`], so it never lists `Vcs` or `Program`.
    pub fn all_facets() -> Self {
        Self::new(
            grantable_facets()
                .map(|facet| Grant {
                    facet,
                    scopes: vec![Scope::All],
                    mode: Mode::ReadWrite,
                })
                .collect(),
        )
    }

    /// Whether every grant names a program-grantable facet. The grantability preflight run before a
    /// manifest's grants are honored; a grant on `Vcs` or `Program` fails it.
    pub fn is_grantable(&self) -> bool {
        self.grants
            .iter()
            .all(|grant| is_program_grantable(grant.facet))
    }

    /// Whether any grant mentions `facet` (the coarse gate that hands out a facet handle).
    pub fn has_facet(&self, facet: Capability) -> bool {
        self.grants.iter().any(|grant| grant.facet == facet)
    }

    /// Whether some grant permits `mode` on `target` within `facet` (the per-operation gate).
    pub fn permits(&self, facet: Capability, mode: Mode, target: &str) -> bool {
        self.grants.iter().any(|grant| {
            grant.facet == facet
                && grant.mode.covers(mode)
                && grant.scopes.iter().any(|scope| scope.matches(target))
        })
    }

    /// Whether every grant in `other` is covered by at least one grant in `self`.
    pub fn covers(&self, other: &GrantSet) -> bool {
        other.grants.iter().all(|requested| {
            self.grants
                .iter()
                .any(|available| grant_covers(available, requested))
        })
    }
}

/// Canonical order over a scope list (each scope mapped via `scope_order`), for grant sorting.
fn scopes_order(scopes: &[Scope]) -> Vec<(u8, &str)> {
    scopes.iter().map(scope_order).collect()
}

/// Total order over a scope, for canonical grant sorting: `All` before any `Prefix`.
fn scope_order(scope: &Scope) -> (u8, &str) {
    match scope {
        Scope::All => (0, ""),
        Scope::Prefix(prefix) => (1, prefix.as_str()),
    }
}

fn grant_covers(available: &Grant, requested: &Grant) -> bool {
    available.facet == requested.facet
        && available.mode.covers(requested.mode)
        && requested
            .scopes
            .iter()
            .all(|scope| available.scopes.iter().any(|a| scope_covers(a, scope)))
}

fn scope_covers(available: &Scope, requested: &Scope) -> bool {
    match (available, requested) {
        (Scope::All, _) => true,
        (Scope::Prefix(a), Scope::Prefix(r)) => r.starts_with(a.as_str()),
        (Scope::Prefix(_), Scope::All) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_coverage() {
        assert!(Mode::ReadWrite.covers(Mode::Read));
        assert!(Mode::ReadWrite.covers(Mode::Write));
        assert!(Mode::Write.covers(Mode::Write));
        assert!(!Mode::Write.covers(Mode::Read));
        assert!(!Mode::Read.covers(Mode::Write));
    }

    #[test]
    fn permits_respects_facet_scope_and_mode() {
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Kv,
            scopes: vec![Scope::Prefix("session:".into())],
            mode: Mode::Write,
        }]);
        assert!(grants.permits(Capability::Kv, Mode::Write, "session:1"));
        assert!(!grants.permits(Capability::Kv, Mode::Write, "user:1"));
        assert!(!grants.permits(Capability::Kv, Mode::Read, "session:1"));
        assert!(!grants.permits(Capability::Graph, Mode::Write, "session:1"));
        assert!(grants.has_facet(Capability::Kv));
        assert!(!grants.has_facet(Capability::Graph));
    }

    #[test]
    fn grant_order_does_not_affect_identity() {
        let a = GrantSet::new(vec![
            Grant {
                facet: Capability::Files,
                scopes: vec![Scope::All],
                mode: Mode::Read,
            },
            Grant {
                facet: Capability::Kv,
                scopes: vec![Scope::All],
                mode: Mode::Write,
            },
        ]);
        let b = GrantSet::new(vec![
            Grant {
                facet: Capability::Kv,
                scopes: vec![Scope::All],
                mode: Mode::Write,
            },
            Grant {
                facet: Capability::Files,
                scopes: vec![Scope::All],
                mode: Mode::Read,
            },
        ]);
        assert_eq!(a, b);
    }

    #[test]
    fn grant_set_covers_narrower_grants_only() {
        let upper = GrantSet::new(vec![
            Grant {
                facet: Capability::Kv,
                scopes: vec![Scope::Prefix("session:".into())],
                mode: Mode::ReadWrite,
            },
            Grant {
                facet: Capability::Files,
                scopes: vec![Scope::All],
                mode: Mode::Read,
            },
        ]);
        assert!(upper.covers(&GrantSet::new(vec![Grant {
            facet: Capability::Kv,
            scopes: vec![Scope::Prefix("session:1".into())],
            mode: Mode::Write,
        }])));
        assert!(upper.covers(&GrantSet::new(vec![Grant {
            facet: Capability::Files,
            scopes: vec![Scope::Prefix("/public".into())],
            mode: Mode::Read,
        }])));
        assert!(!upper.covers(&GrantSet::new(vec![Grant {
            facet: Capability::Kv,
            scopes: vec![Scope::Prefix("user:".into())],
            mode: Mode::Write,
        }])));
        assert!(!upper.covers(&GrantSet::new(vec![Grant {
            facet: Capability::Files,
            scopes: vec![Scope::All],
            mode: Mode::Write,
        }])));
    }

    #[test]
    fn capability_and_mode_tags_round_trip() {
        let mut tags = std::collections::BTreeSet::new();
        for facet in Capability::ALL {
            let tag = facet.stable_tag();
            let cap = Capability::from_stable_tag(tag).unwrap();
            assert_eq!(cap, facet);
            assert!(tags.insert(tag), "duplicate capability tag {tag}");
        }
        let count = Capability::ALL.len() as u8;
        assert_eq!(
            tags.into_iter().collect::<Vec<_>>(),
            (0..count).collect::<Vec<_>>()
        );
        for tag in 0..=2 {
            let mode = Mode::from_u8(tag).unwrap();
            assert_eq!(mode.as_u8(), tag);
        }
        assert!(Capability::from_stable_tag(count).is_none());
        assert!(Mode::from_u8(3).is_none());
    }

    #[test]
    fn grantable_set_excludes_vcs_and_program_only() {
        let grantable: Vec<Capability> = grantable_facets().collect();
        assert!(!grantable.contains(&Capability::Vcs));
        assert!(!grantable.contains(&Capability::Program));
        assert_eq!(grantable.len(), Capability::ALL.len() - 2);
        for facet in Capability::ALL {
            assert_eq!(
                is_program_grantable(facet),
                grantable.contains(&facet),
                "grantable set and predicate must agree for {facet:?}"
            );
        }
    }

    #[test]
    fn all_facets_is_derived_from_the_grantable_set() {
        let all = GrantSet::all_facets();
        assert_eq!(all.grants.len(), grantable_facets().count());
        assert!(all.is_grantable());
        assert!(!all.has_facet(Capability::Vcs));
        assert!(!all.has_facet(Capability::Program));
        for grant in &all.grants {
            assert_eq!(grant.mode, Mode::ReadWrite);
            assert_eq!(grant.scopes, vec![Scope::All]);
        }
    }

    #[test]
    fn preflight_rejects_non_grantable_facets() {
        let ok = GrantSet::new(vec![Grant {
            facet: Capability::Kv,
            scopes: vec![Scope::All],
            mode: Mode::Read,
        }]);
        assert!(ok.is_grantable());
        for facet in [Capability::Vcs, Capability::Program] {
            let bad = GrantSet::new(vec![Grant {
                facet,
                scopes: vec![Scope::All],
                mode: Mode::Read,
            }]);
            assert!(!bad.is_grantable(), "{facet:?} must fail the preflight");
        }
    }
}
