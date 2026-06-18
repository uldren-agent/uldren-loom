# Announcing Structured Diff: git for your data, at the field level

> Mock announcement (non-normative). A marketing-voice preview of the commit-diff contract defined in
> 0003b (with invariant 0001 A6, and the ACL-scoped presentation in 0027). Not a conformance document.

Version control changed how the world ships code, but it stops at the file. Ask git what changed and it
says "these 1,200 lines in these 40 files." It has no idea that a row was inserted, a price went from
14.00 to 15.00, or a calendar event moved an hour. Your data has been living without that, diffed as
opaque blobs or not at all. Loom changes that.

Loom is a content-addressed, versioned store, and every facet - tables, key-value, documents,
time-series, vectors, graphs, calendars, contacts, mail - commits at its own **natural unit**. One
uniform operation, `diff_commits(from, to)`, tells you exactly what changed, per facet, at that unit.
Never "a blob is different."

## What makes it different

- **Diffs at the natural unit, not the file.** A SQL diff reports rows by primary key. A KV diff reports
  keys. A document diff reports ids. A graph diff reports nodes and edges. A calendar diff reports events
  by UID. You get `sql.orders: 3 rows inserted, 1 updated, 2 deleted`, not "the database file changed."
- **Field-level, and typed - better than a line diff.** Below the unit, Loom derives a semantic
  field/cell diff from the typed records: `status: "open" -> "done"`, `dtstart: 14:00 -> 15:00`,
  per-column row deltas, per-property node deltas. Because the records are structured, there is no
  fragile `<<< === >>>` text merge - three-way merges resolve by field, not by guessing text hunks.
- **One contract, every facet.** The same `diff_commits` call spans every storage type in the workspace.
  Learn it once; it works on your rows, your vectors, and your inbox alike.
- **Sublinear.** Storage is prolly trees with structural sharing, so a diff walks only the subtrees that
  actually differ. Comparing two commits does not mean re-reading the data - it means touching what
  changed.
- **Time-travel built in.** Read any unit as of any commit and diff forward: "fetch this entry as of two
  weeks ago and tell me what changed since." History is the storage, not a side log, so every past state
  is fully traversable.
- **Powers incremental everything.** Because the diff names exactly which units changed, downstream work
  recomputes only the delta - incremental GraphRAG reindexing, incremental materialized views,
  incremental sync - instead of full rebuilds. The diff is the enabler.
- **Safe to expose.** Diff and log output is ACL-scoped per viewer: authorized units appear fully
  qualified with their field deltas, unauthorized ones are omitted or rolled up to an opaque count that
  leaks no name or value. The audit truth stays whole in storage; each viewer sees only their slice.

## Why it matters

"Something in the database changed" is not an answer an enterprise can audit, review, or act on. A diff
that speaks rows, fields, events, and edges turns version control from a code tool into a data tool: code
review for your data, reproducible audits, incremental pipelines, and conflict-free structured merges.

Natural-unit. Field-level. Typed. Uniform. Sublinear. Temporal. Incremental. ACL-scoped.

This is the diff your data always deserved.
