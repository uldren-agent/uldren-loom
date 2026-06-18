//! Canonical wire codecs for the version-control facet's composite values (the IDL `MergeResult`,
//! `ReplayOutcome`, `Status`/`Change`, blame rows, and the `ConflictResolution` input).
//!
//! These are the VersionControl IDL contracts, distinct from the SQL result-item encoders in
//! `loom-sql` (e.g. `merge_outcome_cbor`); the two are intentionally not shared because they describe
//! unrelated shapes. The `diff` wire form is the engine's own canonical `LMDIFF` envelope
//! (`Loom::diff_commits`) and needs no codec here.

use loom_codec::{Value as CborValue, decode, encode};
use loom_core::digest::Digest;
use loom_core::{Change, ChangeKind, ConflictResolution, MergeOutcome, ReplayOutcome, Status};
use loom_types::{Code, LoomError};

fn encode_value(value: CborValue) -> Result<Vec<u8>, LoomError> {
    encode(&value).map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))
}

fn text_list(items: &[String]) -> CborValue {
    CborValue::Array(items.iter().map(|s| CborValue::Text(s.clone())).collect())
}

/// Encode a [`MergeOutcome`] as the IDL `MergeResult` `[commit digest|null, fast_forwarded bool,
/// conflicts [text...]]`.
pub fn merge_result_to_cbor(outcome: &MergeOutcome) -> Result<Vec<u8>, LoomError> {
    let (commit, fast_forwarded, conflicts) = match outcome {
        MergeOutcome::UpToDate => (CborValue::Null, false, Vec::new()),
        MergeOutcome::FastForward(digest) => {
            (CborValue::Text(digest.to_string()), true, Vec::new())
        }
        MergeOutcome::Merged(digest) => (CborValue::Text(digest.to_string()), false, Vec::new()),
        MergeOutcome::Conflicts(paths) => (CborValue::Null, false, paths.clone()),
    };
    encode_value(CborValue::Array(vec![
        commit,
        CborValue::Bool(fast_forwarded),
        text_list(&conflicts),
    ]))
}

/// Decode a [`MergeOutcome`] from its IDL `MergeResult` wire form (the inverse of
/// [`merge_result_to_cbor`]: `[commit digest|null, fast_forwarded bool, conflicts [text...]]`).
pub fn merge_result_from_cbor(bytes: &[u8]) -> Result<MergeOutcome, LoomError> {
    let value =
        decode(bytes).map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))?;
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid("merge result must be a cbor array"));
    };
    let [commit, fast_forwarded, conflicts] = items.as_slice() else {
        return Err(LoomError::invalid(
            "merge result must be [commit, fast_forwarded, conflicts]",
        ));
    };
    let &CborValue::Bool(fast_forwarded) = fast_forwarded else {
        return Err(LoomError::invalid(
            "merge result fast_forwarded must be a bool",
        ));
    };
    let CborValue::Array(conflict_items) = conflicts else {
        return Err(LoomError::invalid(
            "merge result conflicts must be a cbor array",
        ));
    };
    let conflicts = conflict_items
        .iter()
        .map(|item| match item {
            CborValue::Text(path) => Ok(path.clone()),
            _ => Err(LoomError::invalid("merge conflict path must be text")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !conflicts.is_empty() {
        return Ok(MergeOutcome::Conflicts(conflicts));
    }
    match (commit, fast_forwarded) {
        (CborValue::Null, false) => Ok(MergeOutcome::UpToDate),
        (CborValue::Text(digest), true) => Ok(MergeOutcome::FastForward(Digest::parse(digest)?)),
        (CborValue::Text(digest), false) => Ok(MergeOutcome::Merged(Digest::parse(digest)?)),
        _ => Err(LoomError::invalid(
            "merge result has an inconsistent commit/fast_forwarded combination",
        )),
    }
}

/// The stable wire tag for a [`ReplayOutcome`] kind (IDL: REPLAYED=0, CLEAN=1, CONFLICTS=2, EMPTY=3).
fn replay_kind_tag(outcome: &ReplayOutcome) -> u64 {
    match outcome {
        ReplayOutcome::Replayed(_) => 0,
        ReplayOutcome::Clean => 1,
        ReplayOutcome::Conflicts(_) => 2,
        ReplayOutcome::Empty => 3,
    }
}

/// Encode a [`ReplayOutcome`] as `[kind_tag uint, tip digest|null, paths [text...]]`.
pub fn replay_outcome_to_cbor(outcome: &ReplayOutcome) -> Result<Vec<u8>, LoomError> {
    let tip = match outcome {
        ReplayOutcome::Replayed(digest) => CborValue::Text(digest.to_string()),
        _ => CborValue::Null,
    };
    let paths = match outcome {
        ReplayOutcome::Conflicts(paths) => text_list(paths),
        _ => CborValue::Array(Vec::new()),
    };
    encode_value(CborValue::Array(vec![
        CborValue::Uint(replay_kind_tag(outcome)),
        tip,
        paths,
    ]))
}

fn change_to_value(change: &Change) -> CborValue {
    CborValue::Array(vec![
        CborValue::Text(change.path.clone()),
        CborValue::Uint(u64::from(change.kind.stable_tag())),
    ])
}

/// Encode a [`Status`] as `[staged [Change...], unstaged [Change...], untracked [text...], conflicts
/// [text...]]`, where each `Change` is `[path, change_kind_tag]`.
pub fn status_to_cbor(status: &Status) -> Result<Vec<u8>, LoomError> {
    encode_value(CborValue::Array(vec![
        CborValue::Array(status.staged.iter().map(change_to_value).collect()),
        CborValue::Array(status.unstaged.iter().map(change_to_value).collect()),
        text_list(&status.untracked),
        text_list(&status.conflicts),
    ]))
}

fn text_list_from_value(value: &CborValue, what: &str) -> Result<Vec<String>, LoomError> {
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid(format!("{what} must be a cbor array")));
    };
    items
        .iter()
        .map(|item| match item {
            CborValue::Text(text) => Ok(text.clone()),
            _ => Err(LoomError::invalid(format!("{what} entries must be text"))),
        })
        .collect()
}

fn change_from_value(value: &CborValue) -> Result<Change, LoomError> {
    let CborValue::Array(fields) = value else {
        return Err(LoomError::invalid("change must be a [path, kind] array"));
    };
    let [CborValue::Text(path), CborValue::Uint(kind)] = fields.as_slice() else {
        return Err(LoomError::invalid("change must be [path text, kind uint]"));
    };
    let tag =
        u8::try_from(*kind).map_err(|_| LoomError::invalid("change kind tag out of range"))?;
    let kind = ChangeKind::from_stable_tag(tag)
        .ok_or_else(|| LoomError::invalid(format!("unknown change kind {tag}")))?;
    Ok(Change {
        path: path.clone(),
        kind,
    })
}

fn change_list_from_value(value: &CborValue, what: &str) -> Result<Vec<Change>, LoomError> {
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid(format!("{what} must be a cbor array")));
    };
    items.iter().map(change_from_value).collect()
}

/// Decode a [`Status`] from its wire form (the inverse of [`status_to_cbor`]: `[staged [Change...],
/// unstaged [Change...], untracked [text...], conflicts [text...]]`).
pub fn status_from_cbor(bytes: &[u8]) -> Result<Status, LoomError> {
    let value =
        decode(bytes).map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))?;
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid("status must be a cbor array"));
    };
    let [staged, unstaged, untracked, conflicts] = items.as_slice() else {
        return Err(LoomError::invalid(
            "status must be [staged, unstaged, untracked, conflicts]",
        ));
    };
    Ok(Status {
        staged: change_list_from_value(staged, "status staged")?,
        unstaged: change_list_from_value(unstaged, "status unstaged")?,
        untracked: text_list_from_value(untracked, "status untracked")?,
        conflicts: text_list_from_value(conflicts, "status conflicts")?,
    })
}

/// Decode a [`ReplayOutcome`] from its wire form (the inverse of [`replay_outcome_to_cbor`]:
/// `[kind_tag uint, tip digest|null, paths [text...]]`).
pub fn replay_outcome_from_cbor(bytes: &[u8]) -> Result<ReplayOutcome, LoomError> {
    let value =
        decode(bytes).map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))?;
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid("replay outcome must be a cbor array"));
    };
    let [kind, tip, paths] = items.as_slice() else {
        return Err(LoomError::invalid(
            "replay outcome must be [kind, tip, paths]",
        ));
    };
    let &CborValue::Uint(kind) = kind else {
        return Err(LoomError::invalid("replay outcome kind must be a uint"));
    };
    match kind {
        0 => {
            let CborValue::Text(digest) = tip else {
                return Err(LoomError::invalid(
                    "replayed outcome must carry a tip digest",
                ));
            };
            Ok(ReplayOutcome::Replayed(Digest::parse(digest)?))
        }
        1 => Ok(ReplayOutcome::Clean),
        2 => Ok(ReplayOutcome::Conflicts(text_list_from_value(
            paths,
            "replay conflicts",
        )?)),
        3 => Ok(ReplayOutcome::Empty),
        other => Err(LoomError::invalid(format!(
            "unknown replay outcome kind {other}"
        ))),
    }
}

/// Encode blame rows as a canonical CBOR array of `[path text, digest text]` pairs.
pub fn blame_rows_to_cbor(rows: &[(String, Digest)]) -> Result<Vec<u8>, LoomError> {
    let rows = rows
        .iter()
        .map(|(path, digest)| {
            CborValue::Array(vec![
                CborValue::Text(path.clone()),
                CborValue::Text(digest.to_string()),
            ])
        })
        .collect();
    encode_value(CborValue::Array(rows))
}

/// Decode blame rows (the inverse of [`blame_rows_to_cbor`]) as `[path, digest_text]` pairs. Returns the
/// digest as its canonical text (matching the MCP `vcs_blame` shape) rather than re-parsing to a
/// [`Digest`], so no re-canonicalization can diverge from the encoded form.
pub fn blame_rows_from_cbor(bytes: &[u8]) -> Result<Vec<(String, String)>, LoomError> {
    let value =
        decode(bytes).map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))?;
    let CborValue::Array(rows) = value else {
        return Err(LoomError::invalid("blame rows must be a cbor array"));
    };
    rows.iter()
        .map(|row| {
            let CborValue::Array(fields) = row else {
                return Err(LoomError::invalid(
                    "blame row must be a [path, digest] array",
                ));
            };
            let [CborValue::Text(path), CborValue::Text(digest)] = fields.as_slice() else {
                return Err(LoomError::invalid(
                    "blame row must be [path text, digest text]",
                ));
            };
            Ok((path.clone(), digest.clone()))
        })
        .collect()
}

/// Decode the one-byte `ConflictResolution` wire atom (`Ours=0`, `Theirs=1`, `Working=2`).
pub fn conflict_resolution_from_wire(bytes: &[u8]) -> Result<ConflictResolution, LoomError> {
    match bytes {
        [tag] => ConflictResolution::from_stable_tag(*tag)
            .ok_or_else(|| LoomError::invalid(format!("unknown conflict resolution {tag}"))),
        _ => Err(LoomError::invalid(
            "conflict resolution must be exactly one byte",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::digest::Algo;

    fn digest(seed: &[u8]) -> Digest {
        Digest::hash(Algo::Blake3, seed)
    }

    #[test]
    fn merge_result_round_trips_through_from_cbor() {
        let cases = [
            MergeOutcome::UpToDate,
            MergeOutcome::FastForward(digest(b"ff")),
            MergeOutcome::Merged(digest(b"m")),
            MergeOutcome::Conflicts(vec!["a/b".to_string(), "c".to_string()]),
        ];
        for outcome in cases {
            let encoded = merge_result_to_cbor(&outcome).unwrap();
            assert_eq!(merge_result_from_cbor(&encoded).unwrap(), outcome);
        }
    }

    fn arr(bytes: &[u8]) -> Vec<CborValue> {
        let CborValue::Array(items) = decode(bytes).unwrap() else {
            panic!("expected array");
        };
        items
    }

    #[test]
    fn merge_result_variants() {
        assert_eq!(
            arr(&merge_result_to_cbor(&MergeOutcome::UpToDate).unwrap()),
            vec![
                CborValue::Null,
                CborValue::Bool(false),
                CborValue::Array(vec![])
            ]
        );
        let d = digest(b"ff");
        assert_eq!(
            arr(&merge_result_to_cbor(&MergeOutcome::FastForward(d)).unwrap()),
            vec![
                CborValue::Text(d.to_string()),
                CborValue::Bool(true),
                CborValue::Array(vec![])
            ]
        );
        assert_eq!(
            arr(&merge_result_to_cbor(&MergeOutcome::Merged(d)).unwrap()),
            vec![
                CborValue::Text(d.to_string()),
                CborValue::Bool(false),
                CborValue::Array(vec![])
            ]
        );
        assert_eq!(
            arr(&merge_result_to_cbor(&MergeOutcome::Conflicts(vec!["a".into()])).unwrap()),
            vec![
                CborValue::Null,
                CborValue::Bool(false),
                CborValue::Array(vec![CborValue::Text("a".into())])
            ]
        );
    }

    #[test]
    fn replay_outcome_variants() {
        let d = digest(b"r");
        assert_eq!(
            arr(&replay_outcome_to_cbor(&ReplayOutcome::Replayed(d)).unwrap()),
            vec![
                CborValue::Uint(0),
                CborValue::Text(d.to_string()),
                CborValue::Array(vec![])
            ]
        );
        assert_eq!(
            arr(&replay_outcome_to_cbor(&ReplayOutcome::Clean).unwrap()),
            vec![
                CborValue::Uint(1),
                CborValue::Null,
                CborValue::Array(vec![])
            ]
        );
        assert_eq!(
            arr(&replay_outcome_to_cbor(&ReplayOutcome::Conflicts(vec!["p".into()])).unwrap()),
            vec![
                CborValue::Uint(2),
                CborValue::Null,
                CborValue::Array(vec![CborValue::Text("p".into())])
            ]
        );
        assert_eq!(
            arr(&replay_outcome_to_cbor(&ReplayOutcome::Empty).unwrap()),
            vec![
                CborValue::Uint(3),
                CborValue::Null,
                CborValue::Array(vec![])
            ]
        );
    }

    #[test]
    fn status_and_change_shape() {
        use loom_core::ChangeKind;
        let status = Status {
            staged: vec![Change {
                path: "s.txt".into(),
                kind: ChangeKind::Added,
            }],
            unstaged: vec![Change {
                path: "u.txt".into(),
                kind: ChangeKind::Modified,
            }],
            untracked: vec!["n.txt".into()],
            conflicts: vec!["c.txt".into()],
        };
        let items = arr(&status_to_cbor(&status).unwrap());
        assert_eq!(items.len(), 4);
        assert_eq!(
            items[0],
            CborValue::Array(vec![CborValue::Array(vec![
                CborValue::Text("s.txt".into()),
                CborValue::Uint(u64::from(ChangeKind::Added.stable_tag())),
            ])])
        );
        assert_eq!(
            items[1],
            CborValue::Array(vec![CborValue::Array(vec![
                CborValue::Text("u.txt".into()),
                CborValue::Uint(u64::from(ChangeKind::Modified.stable_tag())),
            ])])
        );
        assert_eq!(
            items[2],
            CborValue::Array(vec![CborValue::Text("n.txt".into())])
        );
        assert_eq!(
            items[3],
            CborValue::Array(vec![CborValue::Text("c.txt".into())])
        );
    }

    #[test]
    fn blame_rows_shape() {
        let d = digest(b"b");
        let items = arr(&blame_rows_to_cbor(&[("f.txt".into(), d)]).unwrap());
        assert_eq!(
            items,
            vec![CborValue::Array(vec![
                CborValue::Text("f.txt".into()),
                CborValue::Text(d.to_string()),
            ])]
        );
    }

    #[test]
    fn status_round_trips_through_from_cbor() {
        use loom_core::ChangeKind;
        let status = Status {
            staged: vec![Change {
                path: "s.txt".into(),
                kind: ChangeKind::Added,
            }],
            unstaged: vec![Change {
                path: "u.txt".into(),
                kind: ChangeKind::Modified,
            }],
            untracked: vec!["n.txt".into()],
            conflicts: vec!["c.txt".into()],
        };
        let encoded = status_to_cbor(&status).unwrap();
        assert_eq!(status_from_cbor(&encoded).unwrap(), status);
    }

    #[test]
    fn replay_outcome_round_trips_through_from_cbor() {
        let cases = [
            ReplayOutcome::Replayed(digest(b"r")),
            ReplayOutcome::Clean,
            ReplayOutcome::Conflicts(vec!["a/b".into(), "c".into()]),
            ReplayOutcome::Empty,
        ];
        for outcome in cases {
            let encoded = replay_outcome_to_cbor(&outcome).unwrap();
            assert_eq!(replay_outcome_from_cbor(&encoded).unwrap(), outcome);
        }
    }

    #[test]
    fn blame_rows_round_trip_through_from_cbor() {
        let d = digest(b"b");
        let encoded = blame_rows_to_cbor(&[("f.txt".into(), d)]).unwrap();
        assert_eq!(
            blame_rows_from_cbor(&encoded).unwrap(),
            vec![("f.txt".to_string(), d.to_string())]
        );
    }

    #[test]
    fn conflict_resolution_atom() {
        for res in [
            ConflictResolution::Ours,
            ConflictResolution::Theirs,
            ConflictResolution::Working,
        ] {
            assert_eq!(
                conflict_resolution_from_wire(&[res.stable_tag()]).unwrap(),
                res
            );
        }
        assert_eq!(
            conflict_resolution_from_wire(&[3]).unwrap_err().code,
            Code::InvalidArgument
        );
        assert_eq!(
            conflict_resolution_from_wire(&[]).unwrap_err().code,
            Code::InvalidArgument
        );
        assert_eq!(
            conflict_resolution_from_wire(&[0, 1]).unwrap_err().code,
            Code::InvalidArgument
        );
    }
}
