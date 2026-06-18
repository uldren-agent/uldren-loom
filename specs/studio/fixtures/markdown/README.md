# Markdown and Obsidian Import Fixture

This fixture is a representative Markdown tree with Obsidian-style extensions for importer
conformance.

Source basis:

- CommonMark defines the base block and inline syntax used by portable Markdown documents:
  https://spec.commonmark.org/0.31.2/
- GitHub Flavored Markdown extends CommonMark with tables, task-list items, strikethrough, and
  autolinks:
  https://github.github.com/gfm/
- Obsidian vaults use Markdown files with internal links, embeds, properties, and application
  extensions:
  https://obsidian.md/help/links
- Obsidian properties are stored as YAML at the top of the file and include text, list, number,
  checkbox, date, date-time, tags, aliases, and cssclasses:
  https://obsidian.md/help/properties
- Obsidian embeds use `![[...]]` links for notes, headings, blocks, images, PDFs, canvases, and
  width or page attributes:
  https://obsidian.md/help/embeds
- Obsidian callouts use blockquote syntax with `[!type]` markers:
  https://obsidian.md/help/callouts
- JSON Canvas 1.0 defines text, file, link, group nodes, node geometry, colors, and directed edges:
  https://jsoncanvas.org/spec/1.0/

The verifier imports the vault into a clean Loom store and compares supported source fields against
the resulting Pages state. Richer Markdown and Obsidian constructs that are present in the fixture
are classified as fidelity warnings until their structured lowering is implemented.
