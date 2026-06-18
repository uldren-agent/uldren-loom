# YouTube channel ingest

## Idea

Build a YouTube channel importer for Loom that ingests a creator's videos, transcripts, metadata,
playlists, topics, claims, ideas, and references into a versioned memory store.

The importer should support a single video, a playlist, or a full channel. It should preserve raw
source material as files, normalize video and channel records as documents, create embeddings for
semantic recall, extract entities and relations into graph-ready annotations, and eventually support
full-text search across transcripts and metadata.

The first example corpus is Nate Herk's AI automation channel. A second example corpus should be a
popular finance channel so the importer proves it can handle another domain with different entities,
claims, and tagging needs. Caleb Hammer's `Financial Audit` corpus is a good second example because it
is public, high-volume, finance-focused, and structurally different from AI automation tutorials.

## Sources checked

- The video `https://www.youtube.com/watch?v=DTCyvo6cC54` resolves through YouTube oEmbed as
  `Every Level of a Claude Second Brain Explained` by `Nate Herk | AI Automation`.
  Source: `https://www.youtube.com/oembed?url=https://www.youtube.com/watch?v=DTCyvo6cC54&format=json`.
- Nate Herk's YouTube channel is `https://www.youtube.com/@nateherk`.
  Source: `https://www.youtube.com/@nateherk`.
- The YouTube Data API `channels.list` method returns channel resources and supports lookup by
  `forHandle`, `forUsername`, or `id`. The `contentDetails` part includes nested properties such as
  related playlists.
  Source: `https://developers.google.com/youtube/v3/docs/channels/list`.
- The YouTube Data API `playlistItems.list` method retrieves playlist items by playlist ID and
  supports pagination with `pageToken`. Google examples use a channel's uploads playlist to list
  uploaded videos.
  Source: `https://developers.google.com/youtube/v3/docs/playlistItems/list`.
- The YouTube Data API `videos.list` method returns video resources by ID and can include `snippet`,
  `contentDetails`, `statistics`, `status`, and `topicDetails`.
  Source: `https://developers.google.com/youtube/v3/docs/videos/list`.
- The YouTube Data API captions resource represents caption tracks for one video. `captions.list`
  lists tracks but does not include caption text; `captions.download` retrieves caption content.
  Source: `https://developers.google.com/youtube/v3/docs/captions`.
- The YouTube Data API `captions.download` method requires the caller to have permission to edit the
  video and costs 200 quota units. This means official caption download is for the video owner or an
  authorized content partner, not a paid public transcript gateway for arbitrary public videos.
  Source: `https://developers.google.com/youtube/v3/docs/captions/download`.
- The YouTube Data API uses OAuth for private user data and does not support service accounts. Content
  owner parameters are intended for YouTube content partners acting on behalf of linked content owners.
  Source: `https://developers.google.com/youtube/v3/guides/authentication`,
  `https://developers.google.com/youtube/v3/docs/captions/download`.
- Caleb Hammer is a finance YouTuber known for hosting `Financial Audit`, a show analyzing guests'
  personal finances. Public references list the YouTube handle as `CalebHammer`.
  Source: `https://en.wikipedia.org/wiki/Caleb_Hammer`.
- Business Insider describes Caleb Hammer as host of `Financial Audit` and reports that his channel
  had almost 3 million subscribers in March 2026.
  Source: `https://www.businessinsider.com/caleb-built-youtube-creator-debt-financial-subscription-business-2026-3`.
- Loom workspaces are independent typed trees inside one Loom. Workspace types include `files`,
  `document`, `vector`, `graph`, `search`, `sql`, `ledger`, and other facets. History and writes are
  scoped to one workspace, while explicit read-only queries may span workspaces.
  Source: `specs/0014-workspaces.md:7`, `specs/0014-workspaces.md:32`,
  `specs/0014-workspaces.md:99`.

## Example corpora

### Nate Herk AI automation

Channel: `https://www.youtube.com/@nateherk`

Use this corpus to test:

- AI automation concepts.
- Agent harness comparisons.
- Second brain architectures.
- Prompt, tool, skill, and workflow extraction.
- Cross-video idea evolution.
- Tutorial routing and wiki generation.

Example query targets:

- When did the channel first discuss semantic search for second brains?
- Which videos compare routing files, wikis, vector search, and knowledge graphs?
- What tools are repeatedly recommended for agent workflows?
- How has the channel's advice about Claude, Codex, and other harnesses evolved?

### Caleb Hammer finance

Channel: `https://www.youtube.com/@CalebHammer`

Use this corpus to test:

- Personal finance entities and patterns.
- Debt, income, spending, housing, car, credit card, subscription, and budget tags.
- Guest-level case studies.
- Recurring advice and decision rules.
- Claims about common money mistakes.
- Ethical handling of personal financial transcript data.

Example query targets:

- What are the most common financial mistakes discussed across episodes?
- How often are car payments, credit cards, and buy-now-pay-later services mentioned?
- Which advice patterns recur for high-interest debt?
- How does the host's advice differ by income, debt load, or age group when the transcript provides
  those facts?

### Optional algorithmic trading corpus

If the project needs a stock-trading-algorithm example instead of personal finance, use a channel or
playlist around QuantConnect and LEAN. QuantConnect is a public algorithmic trading platform with an
open-source LEAN engine and a known educational surface. That corpus would stress code symbols,
markets, strategies, backtests, indicators, brokerages, and risk metrics rather than consumer finance.

## Access options

### Official API path

Use the YouTube Data API for stable metadata:

- Resolve the channel with `channels.list?part=snippet,contentDetails,statistics&forHandle=@nateherk`.
- Read the uploads playlist from `contentDetails.relatedPlaylists.uploads`.
- Page through videos with `playlistItems.list?part=snippet,contentDetails&playlistId={uploads}`.
- Batch hydrate video records with `videos.list?part=snippet,contentDetails,statistics,status,topicDetails&id=...`.
- Store API ETags and page tokens in the import manifest.

This path is good for channel inventory, metadata, thumbnails, durations, publish times, tags, view
counts, playlist order, and update detection.

### Transcript path

Use caption tracks where available:

- List caption tracks for each video.
- Prefer manually uploaded English captions over automatic speech recognition.
- Download caption content when authorized and available.
- Normalize captions into transcript entries with start time, end time, text, language, and track kind.

Not every public video will expose downloadable captions through the official API to every caller.
The importer must record transcript availability and source, such as `official_caption`,
`auto_caption`, `user_supplied_transcript`, or `missing`.

### Paid gateway status

As of the checked YouTube Data API docs, there is no official paid public gateway that lets a third
party buy transcript or video downloads for arbitrary public YouTube videos.

Official access is narrower:

- Metadata is available through the Data API subject to quota.
- Caption download through `captions.download` requires permission to edit the video, or a content
  partner flow for linked content owners.
- The API does not support service accounts for unattended access to a YouTube account.
- Owner-only video details such as processing and file details are not a public video download
  service.

The practical ecosystem for arbitrary public videos is therefore:

- Use official APIs for metadata.
- Use official captions only when the user owns or is authorized for the video.
- Accept user-supplied transcript files when the user has obtained them through a permitted workflow.
- Treat community downloaders as best-effort, policy-sensitive integrations rather than a stable
  official source.

### User-supplied transcript path

Support transcripts pasted by the user, downloaded through a user's own permitted workflow, or exported
from another tool. The attached transcript for `DTCyvo6cC54` is the model for this path.

User-supplied transcripts should include:

- Video ID.
- Source URL.
- Title if known.
- Channel if known.
- Timestamped transcript lines when available.
- Retrieval time.
- Transcript source and license or permission note.

## Ingest shape

### Files projection

Store raw and readable material in `files:"youtube"`:

- `/youtube/channels/{channel_id}/channel.json`: raw channel API snapshot.
- `/youtube/channels/{channel_id}/videos/{video_id}/video.json`: raw video API snapshot.
- `/youtube/channels/{channel_id}/videos/{video_id}/captions/{track_id}.json`: raw caption metadata.
- `/youtube/channels/{channel_id}/videos/{video_id}/transcript.jsonl`: normalized transcript entries.
- `/youtube/channels/{channel_id}/videos/{video_id}/transcript.md`: readable transcript.
- `/youtube/channels/{channel_id}/videos/{video_id}/notes.md`: generated summary, outline, concepts,
  and references.
- `/youtube/channels/{channel_id}/manifests/import-{timestamp}.json`: import run report.

The raw API JSON and transcript JSONL are the audit source. Markdown files are deterministic readable
projections.

### Document projection

Store normalized records in `document:"youtube"`:

- `youtube:channel:{channel_id}`.
- `youtube:video:{video_id}`.
- `youtube:playlist:{playlist_id}`.
- `youtube:caption:{video_id}:{track_id}`.
- `youtube:transcript-span:{video_id}:{span_id}`.
- `youtube:annotation:{annotation_id}`.

Video records should include channel ID, title, description digest, published time, duration,
thumbnails, tags, category ID, topic details, statistics snapshot, transcript availability, source
digests, and import status.

### Vector projection

Store embeddings in `vector:"youtube-memory"`:

- `youtube:video:{video_id}:summary`.
- `youtube:video:{video_id}:description`.
- `youtube:video:{video_id}:transcript:{chunk_index}`.
- `youtube:video:{video_id}:concept:{concept_id}`.
- `youtube:video:{video_id}:claim:{claim_id}`.
- `youtube:video:{video_id}:code:{symbol_id}` where a video includes code or tool setup.

Embedding metadata should include channel ID, video ID, published time, chunk kind, start time, end
time, language, source digest, extraction version, and domain.

### Graph projection

Store graph-ready annotations in `graph:"youtube-memory"` when the graph facade lands:

- Nodes: `Channel`, `Video`, `Playlist`, `Creator`, `Guest`, `Sponsor`, `Tool`, `Concept`, `Claim`,
  `Technique`, `Task`, `Decision`, `Risk`, `Metric`, `FinancialPattern`, `TradingStrategy`,
  `CodeSymbol`, `TranscriptChunk`, `Reference`, `Embedding`.
- Edges: `PUBLISHED`, `IN_PLAYLIST`, `MENTIONS`, `DISCUSSES`, `INTRODUCES`, `EVOLVES_TO`,
  `RECOMMENDS`, `CRITIQUES`, `USES_TOOL`, `HAS_TRANSCRIPT_CHUNK`, `HAS_EMBEDDING`, `CITES`,
  `SUPPORTS`, `CONTRADICTS`, `HAS_GUEST`, `HAS_SPONSOR`.

Every graph edge derived from transcript text must point back to a transcript span. Metadata-only
edges, such as `PUBLISHED`, point back to the API snapshot digest.

### Search projection

When the search facet lands, index video titles, descriptions, tags, transcript chunks, extracted
concept labels, aliases, claims, tool names, finance categories, and code symbols.

### SQL projection

Optional tables in `sql:"youtube-analytics"`:

- `channels`.
- `videos`.
- `playlists`.
- `video_statistics_snapshots`.
- `transcript_spans`.
- `annotations`.
- `entities`.
- `relations`.
- `import_runs`.

This gives cheap reporting for channel coverage, transcript availability, vocabulary growth, and
topic timelines.

## Channel ingest algorithm

1. Resolve channel.
   - Accept a handle, channel URL, channel ID, playlist URL, or video URL.
   - For handles, use `channels.list` with `forHandle`.
   - Store the resolved channel ID and uploads playlist ID.

2. Inventory videos.
   - Page the uploads playlist with `playlistItems.list`.
   - Store playlist item ID, video ID, title, publish time, position, and API ETag.
   - Stop when no `nextPageToken` remains.

3. Hydrate video metadata.
   - Batch video IDs into `videos.list` requests.
   - Store `snippet`, `contentDetails`, `statistics`, `status`, and `topicDetails` where available.
   - Keep a statistics snapshot history because counts change over time.

4. Fetch transcripts.
   - List caption tracks.
   - Select the best track by language, track kind, and quality policy.
   - Download caption text where allowed.
   - Fall back to user-supplied transcripts.
   - Mark missing transcripts explicitly.

5. Normalize transcript.
   - Convert captions to ordered spans.
   - Preserve start time, end time, text, language, source track, and source digest.
   - Merge tiny adjacent spans when needed for readability, but keep the original span IDs.

6. Write raw files.
   - Store raw API and transcript source data first.
   - Store deterministic markdown and JSONL projections second.
   - Commit each import run with a manifest.

7. Extract structure.
   - Apply rule-based extraction for URLs, handles, tools, tickers, dates, money values, percentages,
     code symbols, and explicit references.
   - Apply LLM extraction for concepts, claims, techniques, risks, tasks, decisions, and idea evolution.
   - Ground every annotation in a transcript span or metadata digest.

8. Project derived data.
   - Write normalized records to `document`.
   - Write embeddings to `vector`.
   - Write graph nodes and edges when available.
   - Write search documents when available.
   - Write SQL tables when reporting is needed.

9. Incremental sync.
   - Reuse ETags, video IDs, published times, and source digests to avoid rewriting unchanged records.
   - Add new videos.
   - Update metadata snapshots.
   - Recompute derived records only when source digests or extraction versions change.

## Domain tags

Reuse the core annotation nomenclature from `specs/studio/MEETINGS.md`, then add domain-specific
subtypes instead of new top-level kinds.

For Nate Herk:

- `Tool`: `Claude Code`, `Codex`, `Hermes`, `Obsidian`, `LightRAG`, `Pinecone`, `Supabase`.
- `Technique`: `routing file`, `LLM wiki`, `semantic search`, `knowledge graph`, `auto memory`.
- `Concept`: `second brain`, `AI operating system`, `context`, `connections`, `cadence`.
- `Task`: `ingest transcript`, `build wiki`, `extract relationships`, `sync memory`.

For Caleb Hammer:

- `FinancialPattern`: `high-interest debt`, `car payment`, `credit card balance`, `buy-now-pay-later`,
  `subscription overspending`, `income instability`, `housing cost`.
- `Metric`: income, debt balance, interest rate, payment amount, savings rate, monthly spend.
- `Advice`: `build emergency fund`, `cut discretionary spend`, `pay high-interest debt`, `sell car`.
- `Risk`: bankruptcy risk, negative cash flow, predatory loan, lifestyle inflation.

For an algorithmic trading channel:

- `TradingStrategy`: momentum, mean reversion, pairs trade, options strategy, market making.
- `MarketInstrument`: equity, ETF, futures, options, FX, crypto.
- `Indicator`: moving average, RSI, MACD, volatility, drawdown, Sharpe ratio.
- `BacktestMetric`: CAGR, max drawdown, win rate, turnover, slippage, fees.
- `CodeSymbol`: algorithm class, indicator function, data source, brokerage adapter.

## Copyright and safety

The importer should preserve user-authorized source data inside the Loom store, but exports and
assistant responses should avoid reproducing long copyrighted transcript passages. Derived summaries,
short snippets, citations, embeddings, and graph facts are safer than bulk transcript redistribution.

For finance and trading channels, generated answers must distinguish educational content from
financial advice. Tags and graph facts can record claims and strategies, but Loom should not present
them as recommendations without a policy layer.

## Implementation plan

1. Build a single-video importer.
   - Accept URL, video ID, title, channel, and user-supplied transcript.
   - Write raw transcript JSONL and readable markdown.
   - Produce a normalized `youtube:video:{video_id}` document.

2. Build official metadata sync.
   - Resolve channel handles.
   - Discover uploads playlist.
   - Page playlist items.
   - Hydrate video metadata.

3. Add transcript acquisition.
   - Use official captions where permitted.
   - Support pasted or local transcript files.
   - Record transcript source and availability.

4. Add deterministic chunking.
   - Preserve timestamps.
   - Chunk by token budget.
   - Keep source digest and span IDs.

5. Add extraction and tagging.
   - Reuse the annotation schema from `specs/studio/MEETINGS.md`.
   - Add channel-specific domain subtypes.
   - Store annotations as files and documents.

6. Add vector projection.
   - Embed summaries, transcript chunks, concepts, claims, and code snippets.
   - Store embedding metadata with model and source digest.

7. Add graph projection.
   - Connect videos, concepts, tools, claims, creators, guests, and source spans.
   - Track first-seen and evolves-to edges for ideas across videos.

8. Add channel analytics.
   - Report imported videos, missing transcripts, repeated concepts, trend lines, and extraction
     confidence.

9. Add incremental sync.
   - Store checkpoints by channel ID and uploads playlist.
   - Re-run only changed videos or changed extraction versions.

## Open questions

- Should the importer require official YouTube API credentials, or support a metadata-only oEmbed path
  for single videos?
- Which transcript sources are acceptable for public videos when official captions are unavailable?
- Should channel ingestion store full transcripts by default, or only derived notes and embeddings?
- How should the importer handle deleted, private, age-restricted, or region-blocked videos?
- Should comments be ingested, ignored, or treated as a separate source with stricter privacy and spam
  controls?
- Should finance and trading channels require a domain safety policy before answering user questions?
