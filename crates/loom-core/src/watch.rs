//! Pull watch types and stateless cursor encoding.

use crate::AclRight;
use crate::digest::Digest;
use crate::error::{Code, LoomError, Result};
use crate::provider::ObjectStore;
use crate::vcs::{ChangeKind, Loom, StagedEntry};
use crate::workspace::{FacetKind, WorkspaceId};
pub use loom_watch::{
    ChangeEvent, DomainChange, UnsupportedDomainDetail, WatchBatch, WatchCursor, WatchDomainDetail,
    WatchDomainSupport, WatchPathChange, WatchSelector, change_event_from_cbor,
    watch_batch_from_cbor, watch_batch_to_cbor, watch_domain_support, watch_domain_supports,
};
use loom_watch::{
    ensure_supported_cursor_selector, ensure_supported_selector, watch_domain_for_path,
};
use std::collections::BTreeSet;

impl<S: ObjectStore> Loom<S> {
    pub fn watch_subscribe(
        &self,
        selector: &WatchSelector,
        from: Option<Digest>,
    ) -> Result<WatchCursor> {
        ensure_supported_selector(selector)?;
        self.authorize_ref(
            selector.workspace,
            &format!("branch/{}", selector.branch),
            AclRight::Read,
        )?;
        self.authorize_watch_selector(selector)?;
        let commit = match from {
            Some(commit) => {
                ensure_commit_in_log(self, selector.workspace, &selector.branch, commit)?;
                Some(commit)
            }
            None => self
                .registry()
                .branch_tip(selector.workspace, &selector.branch)?,
        };
        Ok(WatchCursor::from_selector(selector, commit))
    }

    pub fn watch_poll(&self, cursor: &WatchCursor, max: usize) -> Result<WatchBatch> {
        ensure_supported_cursor_selector(cursor)?;
        self.authorize_ref(
            cursor.workspace,
            &format!("branch/{}", cursor.branch),
            AclRight::Read,
        )?;
        let log = self.log_unchecked(cursor.workspace, &cursor.branch)?;
        let cutoff = match cursor.commit {
            Some(commit) => log
                .iter()
                .position(|candidate| *candidate == commit)
                .ok_or_else(|| self.watch_gap_for_commit(commit))?,
            None => log.len(),
        };
        let mut events = Vec::new();
        let mut last_scanned = None;
        for index in (0..cutoff).rev() {
            if events.len() == max {
                break;
            }
            let commit = log[index];
            let parent = self.get_commit(commit)?.parents.first().copied();
            let commit_changes = self.watch_commit_changes(parent, commit)?;
            let file_changes = filter_file_changes(
                commit_changes.file_changes,
                cursor.path_prefix.as_deref(),
                &cursor.change_kinds,
            );
            let file_changes = self.authorized_file_changes(cursor.workspace, file_changes)?;
            let unsupported_domains = if cursor.facet == Some(FacetKind::Files)
                || cursor.path_prefix.is_some()
                || !cursor.change_kinds.is_empty()
            {
                Vec::new()
            } else {
                self.authorized_unsupported_domains(
                    cursor.workspace,
                    commit_changes.unsupported_domains,
                )?
            };
            last_scanned = Some(commit);
            if file_changes.is_empty() && unsupported_domains.is_empty() {
                continue;
            }
            let path_changes = file_changes
                .iter()
                .map(|change| WatchPathChange::file(change.path.clone(), change.kind))
                .collect();
            let changes = file_changes
                .into_iter()
                .map(|change| {
                    DomainChange::file(change.path, change.kind, change.before, change.after)
                })
                .collect();
            events.push(ChangeEvent {
                workspace: cursor.workspace,
                branch: cursor.branch.clone(),
                commit,
                parent,
                seq: (log.len() - index) as u64,
                changes,
                unsupported_domains,
                path_changes,
            });
        }
        let next = if let Some(commit) = last_scanned {
            WatchCursor::new(cursor.workspace, cursor.branch.clone(), Some(commit), 0)?
                .with_selector_from(cursor)
        } else {
            cursor.clone()
        };
        Ok(WatchBatch { events, next })
    }

    fn watch_commit_changes(
        &self,
        parent: Option<Digest>,
        commit: Digest,
    ) -> Result<CommitWatchChanges> {
        match parent {
            Some(parent) => {
                let (before, _) = self.flatten_commit(parent)?;
                let (after, _) = self.flatten_commit(commit)?;
                let mut paths = BTreeSet::new();
                paths.extend(before.keys().cloned());
                paths.extend(after.keys().cloned());
                let mut changes = CommitWatchChanges::default();
                for path in paths {
                    let before_entry = before.get(&path);
                    let after_entry = after.get(&path);
                    let kind = match (before_entry, after_entry) {
                        (None, Some(_)) => ChangeKind::Added,
                        (Some(_), None) => ChangeKind::Deleted,
                        (Some(a), Some(b)) if a != b => ChangeKind::Modified,
                        _ => continue,
                    };
                    changes.add_path_change(
                        path,
                        kind,
                        before_entry.map(staged_entry_digest),
                        after_entry.map(staged_entry_digest),
                    );
                }
                Ok(changes)
            }
            None => {
                let (files, _) = self.flatten_commit(commit)?;
                let mut changes = CommitWatchChanges::default();
                for (path, entry) in files {
                    changes.add_path_change(
                        path,
                        ChangeKind::Added,
                        None,
                        Some(staged_entry_digest(&entry)),
                    );
                }
                Ok(changes)
            }
        }
    }

    fn authorize_watch_selector(&self, selector: &WatchSelector) -> Result<()> {
        match (selector.facet, selector.path_prefix.as_deref()) {
            (Some(FacetKind::Files), Some(prefix)) | (None, Some(prefix)) => {
                self.authorize_path(selector.workspace, prefix, AclRight::Read)
            }
            (Some(FacetKind::Files), None) => {
                self.authorize(selector.workspace, FacetKind::Files, AclRight::Read)
            }
            _ => Ok(()),
        }
    }

    fn authorized_file_changes(
        &self,
        workspace: WorkspaceId,
        changes: Vec<FileWatchChange>,
    ) -> Result<Vec<FileWatchChange>> {
        changes
            .into_iter()
            .map(|change| {
                self.path_read_allowed(workspace, &change.path)
                    .map(|allowed| allowed.then_some(change))
            })
            .filter_map(|result| result.transpose())
            .collect()
    }

    fn authorized_unsupported_domains(
        &self,
        workspace: WorkspaceId,
        domains: BTreeSet<FacetKind>,
    ) -> Result<Vec<UnsupportedDomainDetail>> {
        domains
            .into_iter()
            .map(|domain| {
                Ok(if self.domain_read_allowed(workspace, domain)? {
                    UnsupportedDomainDetail::from_facet(domain)
                } else {
                    None
                })
            })
            .filter_map(|result| result.transpose())
            .collect()
    }

    fn path_read_allowed(&self, workspace: WorkspaceId, path: &str) -> Result<bool> {
        match self.authorize_path(workspace, path, AclRight::Read) {
            Ok(()) => Ok(true),
            Err(err) if err.code == Code::PermissionDenied => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn domain_read_allowed(&self, workspace: WorkspaceId, facet: FacetKind) -> Result<bool> {
        match self.authorize(workspace, facet, AclRight::Read) {
            Ok(()) => Ok(true),
            Err(err) if err.code == Code::PermissionDenied => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn watch_gap_for_commit(&self, commit: Digest) -> LoomError {
        match self.store().get(&commit) {
            Ok(Some(_)) => {
                LoomError::retained_gap("watch cursor commit is outside retained replay")
            }
            _ => cursor_invalid(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileWatchChange {
    path: String,
    kind: ChangeKind,
    before: Option<Digest>,
    after: Option<Digest>,
}

#[derive(Debug, Default)]
struct CommitWatchChanges {
    file_changes: Vec<FileWatchChange>,
    unsupported_domains: BTreeSet<FacetKind>,
}

impl CommitWatchChanges {
    fn add_path_change(
        &mut self,
        path: String,
        kind: ChangeKind,
        before: Option<Digest>,
        after: Option<Digest>,
    ) {
        match watch_domain_for_path(&path) {
            Some(FacetKind::Files) => self.file_changes.push(FileWatchChange {
                path,
                kind,
                before,
                after,
            }),
            Some(facet) if watch_domain_support(facet).is_some() => {
                self.unsupported_domains.insert(facet);
            }
            _ => {}
        }
    }
}

fn staged_entry_digest(entry: &StagedEntry) -> Digest {
    match entry {
        StagedEntry::File(file) => file.content_addr,
        StagedEntry::Table(digest)
        | StagedEntry::Stream(digest)
        | StagedEntry::TimeSeries(digest)
        | StagedEntry::Graph(digest)
        | StagedEntry::Ledger(digest)
        | StagedEntry::Columnar(digest)
        | StagedEntry::Document(digest) => *digest,
    }
}

fn filter_file_changes(
    changes: Vec<FileWatchChange>,
    path_prefix: Option<&str>,
    change_kinds: &[ChangeKind],
) -> Vec<FileWatchChange> {
    changes
        .into_iter()
        .filter(|change| {
            path_prefix.is_none_or(|prefix| change.path.starts_with(prefix))
                && (change_kinds.is_empty() || change_kinds.contains(&change.kind))
        })
        .collect()
}

fn ensure_commit_in_log<S: ObjectStore>(
    loom: &Loom<S>,
    workspace: WorkspaceId,
    branch: &str,
    commit: Digest,
) -> Result<()> {
    if loom.log_unchecked(workspace, branch)?.contains(&commit) {
        Ok(())
    } else {
        Err(cursor_invalid())
    }
}

fn cursor_invalid() -> LoomError {
    LoomError::cursor_invalid("invalid watch cursor")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStore;
    use crate::workspace::{DEFAULT_BRANCH, FacetKind};
    use loom_watch::{FILES_DOMAIN, FILES_DOMAIN_CHANGE_SCHEMA_VERSION};

    #[test]
    fn cursor_round_trips_without_commit() {
        let workspace = WorkspaceId::v4_from_bytes([1u8; 16]);
        let cursor = WatchCursor::new(workspace, "main", None, 0).unwrap();
        let encoded = cursor.encode();

        assert_eq!(WatchCursor::decode(&encoded).unwrap(), cursor);
    }

    #[test]
    fn cursor_round_trips_with_commit_and_branch_bytes() {
        let workspace = WorkspaceId::v4_from_bytes([2u8; 16]);
        let commit = Digest::blake3(b"commit");
        let cursor = WatchCursor::new(workspace, "feature/a", Some(commit), 7).unwrap();
        let encoded = cursor.encode();

        assert_eq!(WatchCursor::decode(&encoded).unwrap(), cursor);
    }

    #[test]
    fn malformed_cursor_is_cursor_invalid() {
        let err = WatchCursor::decode("not-a-cursor").unwrap_err();

        assert_eq!(err.code, Code::CursorInvalid);
    }

    #[test]
    fn selector_seeds_cursor() {
        let workspace = WorkspaceId::v4_from_bytes([3u8; 16]);
        let selector = WatchSelector::new(workspace, "main").unwrap();
        let commit = Digest::blake3(b"commit");

        assert_eq!(
            WatchCursor::from_selector(&selector, Some(commit)),
            WatchCursor {
                workspace,
                branch: "main".to_string(),
                commit: Some(commit),
                intra_commit_index: 0,
                facet: None,
                path_prefix: None,
                change_kinds: Vec::new(),
            }
        );
    }

    #[test]
    fn subscribe_without_from_starts_at_branch_tip() {
        let (loom, workspace, _, _, c2) = loom_with_history();
        let selector = WatchSelector::new(workspace, DEFAULT_BRANCH).unwrap();

        let cursor = loom.watch_subscribe(&selector, None).unwrap();
        let batch = loom.watch_poll(&cursor, 10).unwrap();

        assert_eq!(cursor.commit, Some(c2));
        assert!(batch.events.is_empty());
        assert_eq!(batch.next, cursor);
    }

    #[test]
    fn poll_from_empty_cursor_replays_history_in_commit_order() {
        let (loom, workspace, c0, c1, c2) = loom_with_history();
        let cursor = WatchCursor::new(workspace, DEFAULT_BRANCH, None, 0).unwrap();

        let batch = loom.watch_poll(&cursor, 10).unwrap();

        assert_eq!(
            batch
                .events
                .iter()
                .map(|event| event.commit)
                .collect::<Vec<_>>(),
            vec![c0, c1, c2]
        );
        assert_eq!(
            batch
                .events
                .iter()
                .map(|event| event.seq)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(batch.next.commit, Some(c2));
    }

    #[test]
    fn poll_resumes_from_cursor_and_advances() {
        let (loom, workspace, c0, c1, c2) = loom_with_history();
        let cursor = WatchCursor::new(workspace, DEFAULT_BRANCH, Some(c0), 0).unwrap();

        let first = loom.watch_poll(&cursor, 1).unwrap();
        let second = loom.watch_poll(&first.next, 10).unwrap();

        assert_eq!(first.events.len(), 1);
        assert_eq!(first.events[0].commit, c1);
        assert_eq!(first.events[0].parent, Some(c0));
        assert_eq!(first.next.commit, Some(c1));
        assert_eq!(second.events.len(), 1);
        assert_eq!(second.events[0].commit, c2);
        assert_eq!(second.events[0].parent, Some(c1));
        assert_eq!(second.next.commit, Some(c2));
    }

    #[test]
    fn poll_includes_sorted_path_changes() {
        let (loom, workspace, c0, _, _) = loom_with_history();
        let cursor = WatchCursor::new(workspace, DEFAULT_BRANCH, Some(c0), 0).unwrap();

        let batch = loom.watch_poll(&cursor, 1).unwrap();

        assert_eq!(
            batch.events[0].path_changes,
            vec![
                WatchPathChange {
                    path: "a.txt".to_string(),
                    kind: ChangeKind::Modified,
                },
                WatchPathChange {
                    path: "b.txt".to_string(),
                    kind: ChangeKind::Added,
                },
            ]
        );
    }

    #[test]
    fn root_event_reports_added_paths() {
        let (loom, workspace, _, _, _) = loom_with_history();
        let cursor = WatchCursor::new(workspace, DEFAULT_BRANCH, None, 0).unwrap();

        let batch = loom.watch_poll(&cursor, 1).unwrap();

        assert_eq!(
            batch.events[0].path_changes,
            vec![WatchPathChange {
                path: "a.txt".to_string(),
                kind: ChangeKind::Added,
            }]
        );
    }

    #[test]
    fn poll_includes_file_domain_changes_with_digests() {
        let (loom, workspace, c0, c1, c2) = loom_with_history();
        let cursor = WatchCursor::new(workspace, DEFAULT_BRANCH, None, 0).unwrap();

        let batch = loom.watch_poll(&cursor, 10).unwrap();

        assert_eq!(
            batch.events[0].changes,
            vec![DomainChange {
                domain: FILES_DOMAIN.to_string(),
                schema_version: FILES_DOMAIN_CHANGE_SCHEMA_VERSION,
                kind: "added".to_string(),
                key: b"a.txt".to_vec(),
                before: None,
                after: Some(path_digest(&loom, c0, "a.txt")),
                detail: None,
            }]
        );
        assert_eq!(
            batch.events[1].changes,
            vec![
                DomainChange {
                    domain: FILES_DOMAIN.to_string(),
                    schema_version: FILES_DOMAIN_CHANGE_SCHEMA_VERSION,
                    kind: "modified".to_string(),
                    key: b"a.txt".to_vec(),
                    before: Some(path_digest(&loom, c0, "a.txt")),
                    after: Some(path_digest(&loom, c1, "a.txt")),
                    detail: None,
                },
                DomainChange {
                    domain: FILES_DOMAIN.to_string(),
                    schema_version: FILES_DOMAIN_CHANGE_SCHEMA_VERSION,
                    kind: "added".to_string(),
                    key: b"b.txt".to_vec(),
                    before: None,
                    after: Some(path_digest(&loom, c1, "b.txt")),
                    detail: None,
                },
            ]
        );
        assert_eq!(
            batch.events[2].changes,
            vec![DomainChange {
                domain: FILES_DOMAIN.to_string(),
                schema_version: FILES_DOMAIN_CHANGE_SCHEMA_VERSION,
                kind: "deleted".to_string(),
                key: b"b.txt".to_vec(),
                before: Some(path_digest(&loom, c1, "b.txt")),
                after: None,
                detail: None,
            }]
        );
        assert_eq!(batch.next.commit, Some(c2));
    }

    #[test]
    fn poll_rejects_unreachable_cursor() {
        let (loom, workspace, _, _, _) = loom_with_history();
        let cursor = WatchCursor::new(
            workspace,
            DEFAULT_BRANCH,
            Some(Digest::blake3(b"missing")),
            0,
        )
        .unwrap();

        let err = loom.watch_poll(&cursor, 10).unwrap_err();

        assert_eq!(err.code, Code::CursorInvalid);
    }

    #[test]
    fn poll_reports_retained_gap_for_known_commit_outside_branch_replay() {
        let (mut loom, workspace, _, _, _) = loom_with_history();
        loom.branch(workspace, "feature").unwrap();
        loom.checkout_branch(workspace, "feature").unwrap();
        loom.write_file(workspace, "feature.txt", b"feature", 0o100644)
            .unwrap();
        let feature_commit = loom.commit(workspace, "watch", "feature", 4).unwrap();
        loom.checkout_branch(workspace, DEFAULT_BRANCH).unwrap();
        let cursor = WatchCursor::new(workspace, DEFAULT_BRANCH, Some(feature_commit), 0).unwrap();

        let err = loom.watch_poll(&cursor, 10).unwrap_err();

        assert_eq!(err.code, Code::RetainedGap);
    }

    #[test]
    fn subscribe_accepts_files_selector() {
        let (loom, workspace, _, _, _) = loom_with_history();
        let selector = WatchSelector::new(workspace, DEFAULT_BRANCH)
            .unwrap()
            .with_facet(FacetKind::Files);

        let cursor = loom.watch_subscribe(&selector, None).unwrap();

        assert_eq!(cursor.facet, Some(FacetKind::Files));
    }

    #[test]
    fn watch_domain_supports_report_detail_status() {
        let files = watch_domain_support(FacetKind::Files).unwrap();

        assert_eq!(files.domain, "files");
        assert_eq!(files.capability, "watch.domain.files");
        assert_eq!(files.detail, WatchDomainDetail::Stable);
        assert_eq!(
            watch_domain_support(FacetKind::Kv).unwrap().detail,
            WatchDomainDetail::Unsupported
        );
        assert!(watch_domain_support(FacetKind::Vcs).is_none());
        for facet in FacetKind::ALL {
            if facet != FacetKind::Vcs {
                assert!(
                    watch_domain_support(facet).is_some(),
                    "missing watch domain support row for {facet}"
                );
            }
        }
    }

    #[test]
    fn subscribe_rejects_non_file_selector_until_promoted() {
        let (loom, workspace, _, _, _) = loom_with_history();
        let selector = WatchSelector::new(workspace, DEFAULT_BRANCH)
            .unwrap()
            .with_facet(FacetKind::Sql);

        let err = loom.watch_subscribe(&selector, None).unwrap_err();

        assert_eq!(err.code, Code::Unsupported);
        assert!(err.message.contains("watch.domain.sql"));
    }

    #[test]
    fn broad_watch_reports_unsupported_non_file_domain() {
        let mut loom = Loom::new(MemoryStore::new());
        let workspace = loom
            .registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::v4_from_bytes([43u8; 16]))
            .unwrap();
        crate::kv_put(
            &mut loom,
            workspace,
            "settings",
            crate::Value::Text("theme".to_string()),
            b"dark".to_vec(),
        )
        .unwrap();
        let commit = loom.commit(workspace, "watch", "kv", 1).unwrap();
        let cursor = WatchCursor::new(workspace, DEFAULT_BRANCH, None, 0).unwrap();

        let batch = loom.watch_poll(&cursor, 10).unwrap();

        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].commit, commit);
        assert!(batch.events[0].changes.is_empty());
        assert!(batch.events[0].path_changes.is_empty());
        assert_eq!(
            batch.events[0].unsupported_domains,
            vec![UnsupportedDomainDetail {
                domain: "kv".to_string(),
                capability: "watch.domain.kv".to_string(),
            }]
        );
    }

    #[test]
    fn path_prefix_selector_filters_file_changes_and_cursor() {
        let (loom, workspace, c0, _, c2) = loom_with_history();
        let selector = WatchSelector::new(workspace, DEFAULT_BRANCH)
            .unwrap()
            .with_facet(FacetKind::Files)
            .with_path_prefix("a.");
        let cursor = loom.watch_subscribe(&selector, Some(c0)).unwrap();
        let encoded = cursor.encode();
        let decoded = WatchCursor::decode(&encoded).unwrap();

        assert_eq!(decoded.facet, Some(FacetKind::Files));
        assert_eq!(decoded.path_prefix.as_deref(), Some("a."));

        let batch = loom.watch_poll(&decoded, 10).unwrap();

        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].path_changes.len(), 1);
        assert_eq!(batch.events[0].path_changes[0].path, "a.txt");
        assert_eq!(batch.next.commit, Some(c2));
        assert_eq!(batch.next.path_prefix.as_deref(), Some("a."));
    }

    #[test]
    fn change_kind_selector_filters_file_changes() {
        let (loom, workspace, c0, _, c2) = loom_with_history();
        let selector = WatchSelector::new(workspace, DEFAULT_BRANCH)
            .unwrap()
            .with_change_kind(ChangeKind::Added);
        let cursor = loom.watch_subscribe(&selector, Some(c0)).unwrap();
        let batch = loom.watch_poll(&cursor, 10).unwrap();

        assert_eq!(batch.events.len(), 1);
        assert_eq!(
            batch.events[0].path_changes,
            vec![WatchPathChange {
                path: "b.txt".to_string(),
                kind: ChangeKind::Added,
            }]
        );
        assert_eq!(batch.next.commit, Some(c2));
        assert_eq!(batch.next.change_kinds, vec![ChangeKind::Added]);
    }

    #[test]
    fn authenticated_watch_requires_ref_read() {
        let (mut loom, workspace, _, _, _) = loom_with_history();
        authenticate_root_without_grants(&mut loom);
        let selector = WatchSelector::new(workspace, DEFAULT_BRANCH).unwrap();

        let err = loom.watch_subscribe(&selector, None).unwrap_err();

        assert_eq!(err.code, Code::PermissionDenied);
    }

    #[test]
    fn authenticated_watch_rejects_unauthorized_path_prefix() {
        let (mut loom, workspace, c0, _, _) = loom_with_history();
        authenticate_root_without_grants(&mut loom);
        grant_vcs_read(&mut loom, workspace);
        let selector = WatchSelector::new(workspace, DEFAULT_BRANCH)
            .unwrap()
            .with_path_prefix("a.");

        let err = loom.watch_subscribe(&selector, Some(c0)).unwrap_err();

        assert_eq!(err.code, Code::PermissionDenied);
    }

    #[test]
    fn authenticated_watch_filters_unauthorized_paths() {
        let (mut loom, workspace, c0, _, c2) = loom_with_history();
        authenticate_root_without_grants(&mut loom);
        grant_vcs_read(&mut loom, workspace);
        grant_files_path_read(&mut loom, workspace, b"a.");
        let selector = WatchSelector::new(workspace, DEFAULT_BRANCH).unwrap();
        let cursor = loom.watch_subscribe(&selector, Some(c0)).unwrap();

        let batch = loom.watch_poll(&cursor, 10).unwrap();

        assert_eq!(batch.events.len(), 1);
        assert_eq!(
            batch.events[0].path_changes,
            vec![WatchPathChange {
                path: "a.txt".to_string(),
                kind: ChangeKind::Modified,
            }]
        );
        assert_eq!(batch.next.commit, Some(c2));
    }

    fn loom_with_history() -> (Loom<MemoryStore>, WorkspaceId, Digest, Digest, Digest) {
        let mut loom = Loom::new(MemoryStore::new());
        let workspace = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                None,
                WorkspaceId::v4_from_bytes([42u8; 16]),
            )
            .unwrap();
        loom.write_file(workspace, "a.txt", b"a", 0o100644).unwrap();
        let c0 = loom.commit(workspace, "watch", "c0", 1).unwrap();
        loom.write_file(workspace, "a.txt", b"a2", 0o100644)
            .unwrap();
        loom.write_file(workspace, "b.txt", b"b", 0o100644).unwrap();
        let c1 = loom.commit(workspace, "watch", "c1", 2).unwrap();
        loom.remove_file(workspace, "b.txt").unwrap();
        let c2 = loom.commit(workspace, "watch", "c2", 3).unwrap();
        (loom, workspace, c0, c1, c2)
    }

    fn path_digest(loom: &Loom<MemoryStore>, commit: Digest, path: &str) -> Digest {
        let (files, _) = loom.flatten_commit(commit).unwrap();
        staged_entry_digest(files.get(path).unwrap())
    }

    fn authenticate_root_without_grants(loom: &mut Loom<MemoryStore>) {
        let root = WorkspaceId::v4_from_bytes([200u8; 16]);
        let mut identity = crate::IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);
    }

    fn grant_vcs_read(loom: &mut Loom<MemoryStore>, workspace: WorkspaceId) {
        let root = WorkspaceId::v4_from_bytes([200u8; 16]);
        loom.acl_store_mut()
            .allow(
                crate::AclSubject::Principal(root),
                Some(workspace),
                Some(FacetKind::Vcs),
                [AclRight::Read],
            )
            .unwrap();
    }

    fn grant_files_path_read(loom: &mut Loom<MemoryStore>, workspace: WorkspaceId, prefix: &[u8]) {
        let root = WorkspaceId::v4_from_bytes([200u8; 16]);
        loom.acl_store_mut()
            .grant(crate::AclGrant {
                subject: crate::AclSubject::Principal(root),
                workspace: Some(workspace),
                domain: Some(FacetKind::Files.into()),
                ref_glob: None,
                scopes: vec![crate::AclScope::Prefix {
                    kind: crate::AclScopeKind::Path,
                    prefix: prefix.to_vec(),
                }],
                rights: [AclRight::Read].into_iter().collect(),
                effect: crate::AclEffect::Allow,
                predicate: None,
            })
            .unwrap();
    }
}
