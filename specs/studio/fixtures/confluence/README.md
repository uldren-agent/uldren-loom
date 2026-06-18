# Confluence Import Fixture

This fixture is a broad normalized Confluence Cloud snapshot for the current Confluence importer. It
is not a live Confluence API client fixture. The source shape is derived from Atlassian Confluence
Cloud REST API v2 documentation and normalized into the `loom interchange import-confluence` input
contract.

Official source references used for the fixture:

- Confluence pages, page bodies, versions, ancestors, descendants, labels, links, restrictions, and
  content properties.
- Confluence spaces, space labels, space properties, roles, permissions, and homepage metadata.
- Confluence attachments and comments.
- Confluence storage-format XHTML and Atlassian Document Format snapshots.

The expected comparison proves:

- explicit space creation and page-inferred space reuse;
- page creation and parent placement;
- byte-preserving storage XHTML body retention as an opaque body block;
- byte-preserving ADF body retention as an opaque body block;
- markdown and text fallback lowering through the markdown path;
- unchanged-page idempotency through the existing Pages service path;
- fidelity issues for Confluence entities that still need native Studio projections.

The current importer accepts a normalized Confluence snapshot, not live Confluence API pages, site
exports, offline export archives, or raw REST pagination. Full XHTML/ADF block lowering,
cross-format semantic equivalence, attachment import, comments, labels, restrictions, properties,
and native page version metadata remain target work in `specs/studio/PAGES.md`.
