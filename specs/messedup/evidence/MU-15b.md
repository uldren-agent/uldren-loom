# MU-15b Evidence

## Current Submission

Completion state: Blocked at strict replacement preflight.

Agent 2 created a canonical Uldren candidate from the legacy active source. Candidate construction,
strict candidate reopen, logical migration reporting, and active-source immutability succeeded.
Matrix was not touched and no replacement or dry-run activation was attempted.

### Artifacts

- Active source: `/Users/nxkavian/Drive/Source/Uldren/loom/uldren.loom`
- Candidate:
  `/Users/nxkavian/Drive/Source/Uldren/loom/uldren.mu15b.candidate.20260730T191654Z.loom`
- Copy report:
  `/Users/nxkavian/Drive/Source/Uldren/loom/uldren.mu15b.copy-report.20260730T191654Z.json`
- Preflight report:
  `/Users/nxkavian/Drive/Source/Uldren/loom/uldren.mu15b.preflight.20260730T191654Z.json`

### Successful Evidence

- No active source holder was reported by `lsof`.
- Active source stayed at 7,630,848 bytes with SHA-256
  `23d9f63ffe298825e4ebb4e2e72ee179a118f3c8ad5cc3af8e4306316d769821`.
- `loom store copy` completed and reported `legacy source layout migrated to canonical candidate`.
- The report contains the accepted freshness watermark, workspace identity, control root, reference
  root, and latest ticket operation sequence.
- The candidate strict-opened through `loom store stat`, reporting generation 244 and 1,266 objects.
- Candidate surface checks passed for store stat, workspace resolution, lanes, tickets, VCS
  namespace preflight, and store doctor.

### Blocker

`loom store preflight-replacement` failed only at `freshness_live_open`:

```text
CORRUPT_OBJECT: loom-store: btree node entry count out of range
```

The preflight tries to open the legacy active source through the ordinary strict-open path even
though candidate construction already used the accepted migration-aware read authority. This makes
the migration preflight reject the exact legacy source it is intended to replace.

### Safety Boundary

- Active source length, mtime, and digest remained unchanged.
- No activation command ran.
- No rollback path was created.
- Matrix was not read, copied, or modified.

## Arbiter Review

Review state: Blocker confirmed. A bounded remediation directive is required.

The remediation must use one accepted migration-aware, immutable live-source freshness authority for
legacy replacement preflight while retaining ordinary strict-open behavior for canonical stores and
all normal runtime opens. It must compare the candidate report watermark to a freshly observed
legacy source identity without mutating the source or introducing a general legacy fallback.
