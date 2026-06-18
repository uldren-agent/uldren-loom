//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Dispatch the `table` subcommands (row-level blame / diff over committed tables), printing each
/// output line. The formatting lives in [`blame_output`] / [`diff_output`] so tests assert on content.
pub(crate) fn run_table(action: TableCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        TableCmd::Blame {
            store,
            workspace,
            table,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let head = loom.registry().head_branch(ns).map_err(|e| e.to_string())?;
            for line in blame_output(&loom, ns, &head, &table)? {
                println!("{line}");
            }
            Ok(())
        }
        TableCmd::Diff {
            store,
            workspace,
            table,
            from,
            to,
        } => {
            let loom = cli_open_loom_read(&store, keys)?;
            let ns = resolve_ns(&loom, &workspace)?;
            let from = Digest::parse(&from).map_err(|e| e.to_string())?;
            let to = Digest::parse(&to).map_err(|e| e.to_string())?;
            for line in diff_output(&loom, ns, &table, from, to)? {
                println!("{line}");
            }
            Ok(())
        }
    }
}

/// Row-level blame of `table` on `branch` as output lines: `<commit>\t<tab-separated cells>` per row,
/// in primary-key order. Cells are the table's real typed columns (the SQL `__key` plus each column).
pub(crate) fn blame_output(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    branch: &str,
    table: &str,
) -> Result<Vec<String>, String> {
    loom.blame_table(ns, branch, table)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(row, commit)| Ok(format!("{commit}\t{}", format_row(&row))))
        .collect()
}

/// Row-level diff of `table` between two commits as output lines: `+ ` added, `- ` removed, and
/// `~ <from> => <to>` updated, each with tab-separated typed cells.
pub(crate) fn diff_output(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    table: &str,
    from: Digest,
    to: Digest,
) -> Result<Vec<String>, String> {
    use loom_core::tabular::RowDiff;
    loom.diff_table(ns, table, from, to)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|change| {
            Ok(match change {
                RowDiff::Added(r) => format!("+ {}", format_row(&r)),
                RowDiff::Removed(r) => format!("- {}", format_row(&r)),
                RowDiff::Updated { from, to } => {
                    format!("~ {} => {}", format_row(&from), format_row(&to))
                }
            })
        })
        .collect()
}
