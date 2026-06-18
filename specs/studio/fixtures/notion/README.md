# Notion Import Fixture

This fixture is a representative Notion API bundle for importer conformance.

Source basis:

- The Notion Page object contains page properties, parent information, timestamps, icon, cover, URL,
  and related metadata:
  https://developers.notion.com/reference/page
- The Notion Block object represents page content as typed blocks such as headings, paragraphs,
  lists, to-do items, quotes, dividers, files, images, tables, and synced blocks:
  https://developers.notion.com/reference/block
- Page content is read through the block-children endpoint:
  https://developers.notion.com/reference/get-block-children
- Notion rich text carries annotations, links, mentions, and inline equations:
  https://developers.notion.com/reference/rich-text
- Notion data sources define property schemas for page rows:
  https://developers.notion.com/reference/data-source
- Notion views define table, board, calendar, timeline, gallery, list, form, chart, map, and
  dashboard presentations:
  https://developers.notion.com/reference/view
- Notion comments, users, and file objects are separate API objects that appear in page, block,
  permission, and media contexts:
  https://developers.notion.com/reference/comment-object
  https://developers.notion.com/reference/user
  https://developers.notion.com/reference/file-object

The fixture combines API-shaped page objects and block-children result payloads into one local
bundle. The verifier imports this bundle into a clean Loom store and compares supported source
fields against the resulting Pages state. Source metadata and block types outside the current
lowering subset are reported as fidelity warnings.
