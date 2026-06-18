//! The run-on-a-branch verification gate over `loom-core::vcs`.
//!
//! A program never touches a live branch. The gate forks `base_branch`, runs the metered program
//! against the fork's files, commits the result to a new `fork_branch`, and returns a reviewable
//! [`RunReport`] (before/after commits, the deterministic state root, the diff, and fuel used). The
//! caller then **adopts** the proposal (merge `fork_branch` into `base_branch`) or **discards** it;
//! either way the base is untouched until an explicit merge. An out-of-fuel program aborts before
//! any fork is created.

use crate::capability::GrantSet;
use crate::engine::{self, FileSet};
use crate::error::ExecError;
use loom_core::vcs::{Change, Loom};
use loom_core::workspace::WorkspaceId;
use loom_core::{Digest, ObjectStore};
use std::collections::{BTreeMap, BTreeSet};

/// Mode applied to files the program writes (files facet, default regular-file mode).
const PROGRAM_FILE_MODE: u32 = 0o100644;

/// The reviewable result of running a program on a forked branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    /// The fork branch the proposal was committed to.
    pub fork_branch: String,
    /// The base commit the run started from.
    pub before: Digest,
    /// The proposed commit on the fork.
    pub after: Digest,
    /// The state root (root Tree digest) of the proposal - deterministic for a given program, base,
    /// and inputs.
    pub after_root: Digest,
    /// Path-level changes from base to proposal.
    pub changes: Vec<Change>,
    /// Fuel consumed by the program.
    pub fuel_used: u64,
}

/// Run `wasm` against a fork of `base_branch` in workspace `ns`, committing the proposal to
/// `fork_branch` for review. The program reaches state only through the files facet, gated by
/// `grants`; `inputs` are read-only. The base is never disturbed - the caller adopts the proposal
/// by merging `fork_branch`, or discards it by ignoring the fork. An out-of-fuel program returns
/// [`ExecError::BudgetExceeded`] before any fork exists.
// The parameters are all distinct and explicit (target branches, program, grants, inputs, budget);
// a builder would add ceremony without clarity.
#[allow(clippy::too_many_arguments)]
pub fn run_on_branch<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    base_branch: &str,
    fork_branch: &str,
    wasm: &[u8],
    grants: GrantSet,
    inputs: BTreeMap<String, Vec<u8>>,
    fuel: u64,
) -> Result<RunReport, ExecError> {
    loom.registry().supports_branching(ns)?;
    let before = loom
        .registry()
        .branch_tip(ns, base_branch)?
        .ok_or_else(|| ExecError::Program(format!("base branch {base_branch:?} has no commits")))?;

    // Materialize the base into the file set the program operates on (POSIX-style leading `/`).
    loom.checkout_branch(ns, base_branch)?;
    let mut files_in = FileSet::new();
    for path in loom.staged_paths(ns) {
        let bytes = loom.read_file(ns, &path)?;
        files_in.insert(format!("/{path}"), bytes);
    }

    // Run the program (metered). Out-of-fuel returns here, before any fork is created.
    let result = engine::run(wasm, files_in, fuel, grants, inputs)?;

    // Fork, set the fork's working tree to the proposed file set, and commit the proposal.
    loom.branch(ns, fork_branch)?;
    loom.checkout_branch(ns, fork_branch)?;
    let proposed: BTreeSet<String> = result
        .files
        .keys()
        .map(|p| p.trim_start_matches('/').to_string())
        .collect();
    for path in loom.staged_paths(ns) {
        if !proposed.contains(&path) {
            loom.remove_file(ns, &path)?;
        }
    }
    for (path, bytes) in &result.files {
        if let Some((parent, _)) = path.trim_start_matches('/').rsplit_once('/') {
            loom.create_directory(ns, parent, true)?;
        }
        loom.write_file(ns, path, bytes, PROGRAM_FILE_MODE)?;
    }
    let after = loom.commit(ns, "program", "program transition", 0)?;
    let after_root = loom.commit_tree(after)?;
    let changes = loom.diff(ns, before, after)?;

    Ok(RunReport {
        fork_branch: fork_branch.to_string(),
        before,
        after,
        after_root,
        changes,
        fuel_used: result.fuel_used,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{Capability, Grant, Mode, Scope};
    use loom_core::MemoryStore;
    use loom_core::vcs::ChangeKind;
    use loom_core::workspace::{FacetKind, WorkspaceId};

    fn nid(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    // A program that writes "/greeting" = "hello world" via the files facet.
    fn writer_wasm() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "/greeting")
                 (data (i32.const 16) "hello world")
                 (func (export "run")
                   (call $fw (i32.const 0) (i32.const 9) (i32.const 16) (i32.const 11))))"#,
        )
        .expect("assemble writer wasm")
    }

    // A program that loops forever (exhausts any fuel budget).
    fn runaway_wasm() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (memory (export "memory") 1)
                 (func (export "run") (loop (br 0))))"#,
        )
        .expect("assemble runaway wasm")
    }

    fn seeded_loom_with_facet(
        seed: u8,
        facet: FacetKind,
    ) -> (Loom<MemoryStore>, WorkspaceId, Digest) {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom.registry_mut().create(facet, None, nid(seed)).unwrap();
        loom.write_file(ns, "/seed", b"s", 0o100644).unwrap();
        let base = loom.commit(ns, "nas", "base", 1).unwrap();
        (loom, ns, base)
    }

    fn seeded_loom(seed: u8) -> (Loom<MemoryStore>, WorkspaceId, Digest) {
        seeded_loom_with_facet(seed, FacetKind::Files)
    }

    fn all_files() -> GrantSet {
        GrantSet::new(vec![Grant {
            facet: Capability::Files,
            scopes: vec![Scope::All],
            mode: Mode::ReadWrite,
        }])
    }

    #[test]
    fn gate_proposes_and_diffs() {
        let (mut loom, ns, base) = seeded_loom_with_facet(1, FacetKind::Files);
        let report = run_on_branch(
            &mut loom,
            ns,
            "main",
            "proposed",
            &writer_wasm(),
            all_files(),
            BTreeMap::new(),
            100_000,
        )
        .unwrap();
        assert_eq!(report.before, base);
        assert_ne!(report.after, base);
        assert!(report.fuel_used > 0);
        // The proposal adds the greeting file; the base is untouched.
        assert_eq!(
            report.changes,
            vec![Change {
                path: "greeting".into(),
                kind: ChangeKind::Added
            }]
        );
        assert_eq!(loom.registry().branch_tip(ns, "main").unwrap(), Some(base));
    }

    #[test]
    fn run_is_deterministic() {
        // Two independent runs of the same program over the same base yield the same state root.
        let (mut a, na, _) = seeded_loom(1);
        let (mut b, nb, _) = seeded_loom(1);
        let ra = run_on_branch(
            &mut a,
            na,
            "main",
            "p",
            &writer_wasm(),
            all_files(),
            BTreeMap::new(),
            100_000,
        )
        .unwrap();
        let rb = run_on_branch(
            &mut b,
            nb,
            "main",
            "p",
            &writer_wasm(),
            all_files(),
            BTreeMap::new(),
            100_000,
        )
        .unwrap();
        assert_eq!(ra.after_root, rb.after_root);
        assert_eq!(ra.after, rb.after);
    }

    #[test]
    fn capability_denied_write_is_a_noop() {
        let (mut loom, ns, _) = seeded_loom(1);
        // Grant writes only under "/allowed/"; the program writes "/greeting" -> denied -> no-op.
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Files,
            scopes: vec![Scope::Prefix("/allowed/".into())],
            mode: Mode::ReadWrite,
        }]);
        let report = run_on_branch(
            &mut loom,
            ns,
            "main",
            "proposed",
            &writer_wasm(),
            grants,
            BTreeMap::new(),
            100_000,
        )
        .unwrap();
        assert!(
            report.changes.is_empty(),
            "denied write must not change state"
        );
    }

    #[test]
    fn budget_exceeded_leaves_base_untouched() {
        let (mut loom, ns, base) = seeded_loom(1);
        let err = run_on_branch(
            &mut loom,
            ns,
            "main",
            "proposed",
            &runaway_wasm(),
            all_files(),
            BTreeMap::new(),
            10_000,
        )
        .unwrap_err();
        assert!(matches!(err, ExecError::BudgetExceeded { .. }));
        // No fork was created and the base is unmoved.
        assert_eq!(loom.registry().branch_tip(ns, "main").unwrap(), Some(base));
        assert!(
            !loom
                .registry()
                .branch_list(ns)
                .unwrap()
                .contains(&"proposed".to_string())
        );
    }

    #[test]
    fn proposal_can_be_adopted_by_merge() {
        let (mut loom, ns, _) = seeded_loom(1);
        let report = run_on_branch(
            &mut loom,
            ns,
            "main",
            "proposed",
            &writer_wasm(),
            all_files(),
            BTreeMap::new(),
            100_000,
        )
        .unwrap();
        // Adopt: fast-forward main onto the proposal.
        loom.checkout_branch(ns, "main").unwrap();
        loom.merge(ns, "proposed", "nas", 2).unwrap();
        assert_eq!(
            loom.registry().branch_tip(ns, "main").unwrap(),
            Some(report.after)
        );
        // The adopted state contains the greeting the program wrote.
        assert_eq!(loom.read_file(ns, "/greeting").unwrap(), b"hello world");
    }
}
