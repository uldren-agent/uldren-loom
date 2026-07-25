# Matrix Migration Backup and Artifact Cleanup Policy

Controlled Matrix migrations produce operational artifacts alongside the live store: pre-migration
rollback backups and short-lived migration or repair test stores. This policy defines what those
artifacts are, how they are named and located, how they stay out of normal repository scans, what is
retained, and the explicit owner-approval path required before any deletion. This is a pre-release
operational policy for the `matrix/` working directory; it does not authorize deleting anything on its
own.

## Artifact taxonomy

All artifacts live in the `matrix/` directory next to the live store.

1. Live operational store: `matrix/matrix.loom`. The current Matrix workspace store. Runtime data,
   never a source-controlled file.

2. Pre-migration rollback backup: `matrix/matrix.loom.pre-mx<ticket>[-<qualifier>]`. A full copy of
   the live store captured immediately before a controlled migration lands, so the migration can be
   rolled back. Examples on disk: `matrix.loom.pre-mx383`, `matrix.loom.pre-mx383-final`,
   `matrix.loom.pre-mx389`, `matrix.loom.pre-mx391`. The `<ticket>` segment records the migration
   ticket the backup was taken for; an optional `<qualifier>` (for example `final`) distinguishes
   multiple checkpoints for the same migration.

3. Migration and repair test store: `matrix/matrix.<qualifier>-test.loom` or
   `matrix/matrix.mx<ticket>*-test.loom`. A disposable store used to rehearse or verify a migration or
   repair without touching the live store. Examples on disk: `matrix.repair-test.loom`,
   `matrix.mx383-test.loom`, `matrix.mx391-test.loom`, `matrix.mx391-source-test.loom`.

## Naming contract

- Backups always extend the live store name with a `.pre-mx<ticket>` suffix, optionally followed by a
  single `-<qualifier>`. The `.loom` base name is preserved so the backup is recognizably a copy of
  `matrix.loom`.
- Test stores always end in `-test.loom` and carry the originating migration ticket in an `mx<ticket>`
  segment when they belong to a specific migration.
- New migration tooling that produces these artifacts must follow these two patterns so the exclusion
  rules below match them without per-artifact edits.

## Location and scan exclusion

- Artifacts stay in `matrix/`. They are not moved into the source tree and are not committed.
- `matrix/.gitignore` excludes the live store, all `matrix.loom.pre-*` backups, and all `*-test.loom`
  and `matrix.mx*.loom` migration test stores from version control and therefore from `git status`,
  `git add`, and any scan that honors gitignore. This keeps large operational data out of normal
  repository scans and out of the compaction-prone output the arbiter reviews.
- Because the patterns are name-based, a correctly named future artifact is excluded automatically. An
  artifact that does not match the naming contract will show up in scans; that is intentional, so a
  mis-named artifact is noticed rather than silently ignored.

## Retention

- Keep the most recent pre-migration backup for the currently active store lineage, plus any backup
  tagged `-final` for a migration that has shipped, until the owner confirms the migration is durable
  and no rollback is needed.
- Test stores are disposable. They may be recreated at any time from the live store or a backup and
  carry no durable value once their migration or repair has been verified.
- Retention is a floor, not a mandate to delete: nothing is removed automatically. Growth is bounded in
  practice because backups are per-migration and test stores are per-rehearsal.

## Deletion requires explicit owner approval

No process, script, or agent may delete a backup or test store as a side effect. Deletion is a
deliberate, owner-gated operation:

1. Propose the specific artifact paths to delete and the reason (for example, migration MX-<ticket> is
   confirmed durable and its pre-migration backup is no longer needed).
2. Obtain explicit owner approval recorded on the owning ticket. Approval names the exact paths.
3. Only after recorded approval, remove exactly the approved paths. Never use a broad glob delete.
4. Test stores may be deleted under the same recorded-approval path; because they are disposable, an
   approval that authorizes clearing test stores by the `-test.loom` pattern is sufficient and does not
   need to enumerate each one, but it must still be explicit.

Backups for a migration that has not been confirmed durable are never deletion candidates.

## Scope of this ticket

This ticket defines the policy and implements the scan exclusion (`matrix/.gitignore`). It does not
delete any backup or test store. Actual cleanup happens later only through the owner-approval path
above.
