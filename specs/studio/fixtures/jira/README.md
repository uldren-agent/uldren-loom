# Jira Import Fixture

This fixture is a broad normalized Jira Cloud snapshot for the current Jira importer. It is not a
live Jira API client fixture. The source shape is derived from Atlassian Jira Cloud REST API
documentation and normalized into the `loom interchange import-jira` input contract.

Official source references used for the fixture:

- Jira issue fields, changelog, transitions, and issue search surfaces.
- Jira project metadata, project categories, issue types, components, versions, roles, and insight.
- Jira comments, attachments, worklogs, issue links, watchers, votes, and Jira Software sprints.

The expected comparison proves:

- project creation with Jira key preservation;
- issue creation with external identity lookup;
- first-class status-mapped Ticket Board creation and deterministic card placement;
- retention of core issue fields, ADF-style description/environment bodies, user/object references,
  status category, dates, parent, security, votes, watchers, sprint, components, versions, issue
  links, subtasks, transitions, properties, development metadata, custom fields, and source labels;
- label-to-policy-label preservation;
- duplicate-safe storage through existing project and ticket identity checks;
- fidelity issues for Jira entities that still need native Studio projections.

The current importer accepts a normalized Jira snapshot, not live Jira Cloud API pages, Jira backup
archives, or Jira Software board exports. Those remain target work in `specs/studio/JIRAISH.md`.
