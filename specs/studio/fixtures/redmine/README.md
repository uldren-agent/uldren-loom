# Redmine Import Fixture

This fixture is a representative Redmine API bundle for importer conformance.

Source basis:

- Redmine REST API lists XML and JSON formats and resources for issues, projects, time entries,
  issue relations, wiki pages, attachments, and journals:
  https://www.redmine.org/projects/redmine/wiki/Rest_api
- Redmine issue reads can include attachments, relations, and journals:
  https://www.redmine.org/projects/redmine/wiki/Rest_Issues
- Redmine wiki page reads include text, parent, version, author, comments, timestamps, and
  attachments:
  https://www.redmine.org/projects/redmine/wiki/Rest_WikiPages
- Redmine time entries are a separate resource with issue/project, spent date, hours, activity,
  comments, and user fields:
  https://www.redmine.org/projects/redmine/wiki/Rest_TimeEntries
- Redmine projects can include trackers, issue categories, enabled modules, time-entry activities,
  custom fields, parent, default version, default assignee, homepage, status, and public flag:
  https://www.redmine.org/projects/redmine/wiki/Rest_Projects
- Redmine trackers, issue statuses, versions, custom-field definitions, and enumerations are exposed
  by separate API resources:
  https://www.redmine.org/projects/redmine/wiki/Rest_Trackers
  https://www.redmine.org/projects/redmine/wiki/Rest_IssueStatuses
  https://www.redmine.org/projects/redmine/wiki/Rest_Versions
  https://www.redmine.org/projects/redmine/wiki/Rest_CustomFields
  https://www.redmine.org/projects/redmine/wiki/Rest_Enumerations

The fixture combines those API-shaped resource payloads into one local bundle so the importer can be
tested without network credentials or a live Redmine server. The verifier imports this bundle into a
clean Loom store and compares the represented source fields against the resulting ticket and page
state.
