# Asana Import Fixture

This fixture is a broad normalized snapshot for the current Asana importer. It is not a live Asana
API client fixture. The source shape is derived from the official Asana API reference and then
normalized into the `loom interchange import-asana` input contract.

Official source references used for the fixture:

- Asana Task fields: `gid`, `resource_type`, `name`, `resource_subtype`, `approval_status`,
  assignment, completion, date, time-tracking, dependency, membership, like, follower, custom-field,
  and external metadata.
- Asana Project fields: project identity, archive/color/icon/view/date metadata, workspace, team,
  owner, members, followers, current status, and custom-field settings.
- Asana Story fields: comment/system story identity, timestamp, actor, subtype, text, HTML text, and
  pinned state.
- Asana Attachment fields: identity, host/subtype, timestamps, URLs, parent, size, and app linkage.
- Asana Portfolio, Goal, Tag, User, Team, and Workspace fields used by task/project relationships.

The expected comparison proves:

- project creation with deterministic key prefix;
- task creation with external identity lookup;
- first-class manual Ticket Board creation from project sections and deterministic card placement;
- retention of core task fields, date fields, approval status, object references, dependency lists,
  membership lists, follower/like lists, custom fields, and source tags;
- tag-to-policy-label preservation;
- approval-task retention as a normal ticket until approval workflow lowering exists;
- fidelity issues for Asana entities that still need native Studio projections.

The current importer accepts a normalized Asana snapshot, not raw organization export archives, live
API pages, or resource-export JSON-lines. Those remain target work in `specs/studio/JIRAISH.md`.
