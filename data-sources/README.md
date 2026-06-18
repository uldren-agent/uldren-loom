# data-sources

Public, open datasets that map to each Loom **workspace type**, for demos and example imports. Each
is a natural fit for one facet, so we can show "import a real dataset -> query it as that facet, with
version control and sync for free." URLs verified mid-2026; sizes/licenses below.

Run `./download.sh list` to see everything, `./download.sh <name>` to fetch one,
`./download.sh small` for the quick (<100 MB) demo set. Files land in `downloads/<name>/`. Nothing
downloads unless you ask; the 154 GB Wikidata dump is gated behind a confirmation.

## Mapping: workspace type -> dataset

| Workspace | Dataset | Name | Size | License | Why it fits |
|---|---|---|---|---|---|
| `files` | enwik8 (Wikipedia XML, 10^8 B) | `enwik8` | 36 MB | Public domain (CC-BY-SA text) | a real file tree / large document to snapshot |
| `vcs` | Pallets `flask` repo | `flask` | ~60 MB | BSD-3-Clause | import a git repo's *files* into a versioned `vcs` workspace |
| `sql` | MovieLens (small) | `movielens` | 978 KB | GroupLens (non-commercial) | classic relational tables (ratings, movies, tags) |
| `sql` | US Census county estimates 2024 | `census` | 1.8 MB | US Gov public domain | the "census database" demo - typed columns, PK by FIPS |
| `kv` | Google Trillion-Word `count_1w` | `words` | 5 MB | MIT | a big word->count map |
| `document` | Amazon Reviews 2023 (All_Beauty) | `amazon-reviews` | 327 MB | CC-BY-NC 4.0 | JSONL documents by id with rich fields |
| `vector` | MS MARCO passage collection | `msmarco` | 1.04 GB | MS MARCO research | a text corpus to embed + nearest-neighbour search |
| `graph` | SNAP ego-Facebook edges | `snap-facebook` | 218 KB | BSD-style (research) | a small property graph (edge list) |
| `graph` | OpenStreetMap Rhode Island | `osm-rhode-island` | 52 MB | ODbL 1.0 | a richer geo/knowledge graph |
| `columnar` | NYC TLC Yellow Taxi (Parquet) | `nyc-taxi` | 59 MB | NYC TLC public | columnar Parquet segments for OLAP scans |
| `queue` / `append-log` | GH Archive hourly events | `gharchive` | 74 MB | Public (GH Archive) | an ordered append-only event stream |
| `time-series` | NOAA GHCN-Daily 2024 | `noaa-ghcn` | ~1 GB | US Gov public domain | `(station, date) -> measurement` points + rollups |
| `cas` | enwik9 (large blob) | `enwik9` | 322 MB | Public domain | a big compressible blob for content-addressed put/get |
| `document` / `graph` | Wikidata full JSON | `wikidata` | 154 GB | CC0 | the "Wikipedia DB" demo - entities + relations (gated) |
| `ledger` | Blockchair Bitcoin tx dumps | (manual) | varies/day | Free for research | append-only hash-chained transactions (rate-limited; add manually) |

Notes: `ledger` (Blockchair) is rate-limited and dated, so it is not in the auto-manifest - grab a
day's TSV from `https://gz.blockchair.com/bitcoin/transactions/` manually. `vcs` uses `git clone`
(we import the *files*, since git import itself is out of scope). `wikidata` is huge; prefer a
C4 shard (`https://huggingface.co/datasets/allenai/c4`) for a smaller text/document corpus.

## Status

This is a download helper + catalog. The matching **importers** (dataset → workspace) are not built
yet; they are the per-facet ingest work (the "Data-source loaders"). The script gives us the
raw inputs to build and demo those against.
