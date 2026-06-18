# Drive and SharePoint Import Fixture

This fixture is a broad normalized Drive snapshot for the current Drive importer. It is not a live
Google Drive or Microsoft Graph client fixture. The source shape is derived from Google Drive API and
Microsoft Graph DriveItem documentation, then normalized into the `loom interchange import-drive`
input contract.

Official source references used for the fixture:

- Google Drive `files` resource fields, including parents, MIME type, shortcuts, links, checksums,
  labels, restrictions, owners, capabilities, and embedded permissions.
- Google Drive `permissions`, `comments`, and `revisions` resources.
- Microsoft Graph `driveItem` fields and relationships, including `parentReference`, `file`,
  `folder`, `package`, `remoteItem`, `sharepointIds`, `retentionLabel`, `permissions`, `versions`,
  `thumbnails`, `listItem`, `webUrl`, and `@microsoft.graph.downloadUrl`.

The expected comparison proves:

- Drive folder creation through the reusable Drive service;
- file import through inline text, inline hex bytes, and sidecar `content_path` bytes;
- current-head file bytes are readable after import;
- generic 0012 execution-batch dispatch for normalized Drive snapshots;
- unsupported Google Drive and SharePoint fields are classified as fidelity issues instead of being
  silently ignored.

The generic execution-batch path accepts a single normalized snapshot payload. Sidecar bytes for
`content_path` remain direct-import only until the shared 0012 batch contract grows an explicit
sidecar-payload materialization rule.
