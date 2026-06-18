# Meetings Import Fixtures

These fixtures exercise the normalized Meetings import contract used by Granola API, Granola app,
Granola MCP, CSV, and generic batch adapters. They are not raw vendor exports. Raw source payloads
and vendor-specific fields are represented through retained sidecars so the importer can prove both
first-class lowering and source retention.

`source/granola-broad-snapshot.json` is the current broad Granola-shaped acceptance fixture. It
contains complete, partial, and degraded source observations in one import run so importer tests can
verify identity, folders, owners, attendees, calendar references, summaries, transcripts,
annotations, structured extraction rows, retained source payloads, coverage gaps, retry windows,
and checkpoint state.

`source/granola-api-snapshot.json`, `source/granola-mcp-snapshot.json`, and
`source/granola-csv-snapshot.json` execute the remaining input-profile selectors. They keep profile
coverage explicit instead of treating the app-cache fixture as proof for every source path.

`expected/comparison.json` is the stable comparison summary used by the Rust importer test.
