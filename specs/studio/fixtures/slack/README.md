# Slack Import Fixture

This fixture is a broad normalized Slack snapshot for the current Slack importer. It is not a live
Slack API client fixture. The source shape is derived from Slack export documentation and Slack
Developer object references, then normalized into the `loom interchange import-slack` input
contract.

Official source references used for the fixture:

- Slack workspace export zip shape for public channels, private channels, DMs, messages, file links,
  channel audit exports, and scheduled exports.
- Slack conversation objects, including channel type flags, topic, purpose, properties, previous
  names, sharing metadata, and members.
- Slack message events, including `type`, `subtype`, `user`, `text`, `ts`, edits, hidden/deleted
  messages, pins, stars, reactions, and thread timestamps.
- Slack message formatting and Block Kit blocks.
- Slack user objects, user groups, and file objects.

The expected comparison proves:

- Chat channel creation from a Slack channel;
- plain text message lowering through the Chat service;
- present-parent thread creation;
- reaction-kind registration and reaction attachment;
- Slack metadata detection and fidelity reporting for fields not yet natively lowered;
- generic 0012 import execution over the same normalized input.

The current importer accepts normalized Slack snapshots and Slack export zip files with
`channels.json` plus channel message JSON files. Zip `users.json` and `usergroups.json` are
recognized as source metadata and reported as unsupported. Native principal mapping, per-user
reaction authorship, mrkdwn and Block Kit lowering, files, pins, custom emoji assets, channel
membership, and coexistence bridge behavior remain target work in `specs/studio/SLACKISH.md`.
