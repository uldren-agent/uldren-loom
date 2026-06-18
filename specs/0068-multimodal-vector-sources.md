# 0068 - Multimodal Vector Sources

**Status:** Draft target; ownership boundary source-backed from 0062 closure. **Version:** 0.1.1-target.
**Capability:** `multimodal-vector-sources`.

**Depends on:** 0017 (vector layer), 0050 (text embedding providers), 0062 (inference model
downloads), 0020 (document layer), 0024 (CAS layer), 0032 (native / web parity), 0035 (durable
delivery), 0036 (locking and coordination). **Relates to:** 0033 (search layer), 0040 (GraphRAG),
0064 (unified search / discovery).

This spec owns the pipeline that turns non-text source material into vector-searchable units. It
covers PDFs, images, audio, video, and multimodal bundles before they become ordinary vectors in 0017.
The Queue 6 inference-download closure leaves this scope here rather than expanding 0062 beyond
`llm` and `text-embedding` models.

This spec does not own raw vector storage, text-only embedding providers, or model downloads:

- 0017 owns vector sets, vector ids, metadata filters, exact search, accelerators, and source-text
  sidecars.
- 0050 owns the text-only embedding provider contract.
- 0062 owns acquiring, diagnosing, configuring, and activating local or hosted inference models.
- 0068 owns media extraction, source unit identity, provenance, modality metadata, and multimodal
  embedding provider contracts.

Decision Points: none.

## 1. Source Checks

Checked before creating this spec:

- 0017 defines the vector facet as the owner of raw vector sets and source-text sidecars, and states
  that images, audio, video, PDFs, and other documents need a source pipeline before they enter the
  vector facet (`specs/0017-vector-layer.md:133`).
- 0050 is explicitly text-only and excludes image, audio, video, PDF, and multimodal embeddings
  (`specs/0050-providers.md:15`).
- 0062 is scoped to `llm` and `text-embedding` model acquisition and activation. It does not define
  media extraction or multimodal source metadata (`specs/0062.md:10`, `specs/0062.md:25`).

## 2. Boundary

The durable source of truth for non-text content is not the vector facet. A source asset lives in the
facet that naturally owns it:

| Source kind | Source owner | 0068 responsibility | Vector output |
| --- | --- | --- | --- |
| Text | 0050 plus 0017 source text | None beyond interoperability | Existing `TextEmbedding` path |
| PDF | Files, CAS, or document | Page, region, table, OCR, and extracted-text units | One or more vectors with page/range metadata |
| Image | Files or CAS | Whole-image, crop, region, caption, and perceptual metadata units | One or more vectors with region metadata |
| Audio | Files or CAS | Time-window, transcript, speaker, and acoustic metadata units | One or more vectors with time-range metadata |
| Video | Files or CAS | Scene, frame, audio-window, transcript, and alignment units | One or more vectors with frame/time metadata |
| Multimodal bundle | Owning application facet or CAS | Stable grouping, per-part provenance, and cross-modal alignment | Multiple vectors linked by source group id |

The vector facet receives fixed-width vectors plus typed metadata. It may store text sidecars when a
unit has textual source, such as OCR text, a caption, or a transcript span. It must not become the
owner of PDF parsing, image decoding, audio segmentation, video scene detection, or model-specific
multimodal payloads.

## 3. Source Unit Model

A multimodal pipeline produces source units. A source unit is the smallest provenance-preserving item
that can be embedded, searched, cited, and refreshed independently.

Target source unit fields:

| Field | Meaning |
| --- | --- |
| `unit_id` | Stable id derived from source ref, extractor profile, and source range. |
| `source_ref` | Reference to the source asset in files, CAS, document, or an application facet. |
| `source_kind` | `pdf`, `image`, `audio`, `video`, `document`, or `multimodal`. |
| `range` | Page, byte range, character range, time range, frame range, crop rectangle, or region id. |
| `mime_type` | Media type observed or declared by the ingest path. |
| `extractor_profile` | Extractor name, version, and settings that produced the unit. |
| `embedding_profile` | Model id, dimension, weights digest, runtime kind, and canonical settings profile. |
| `text` | Optional extracted text, caption, OCR text, or transcript span. |
| `metadata` | Typed metadata cells suitable for 0017 filtering and hosted compatibility projections. |
| `parent_unit_id` | Optional parent unit for page-to-region, video-to-scene, or audio-to-transcript links. |

The target encoding MUST be deterministic. A unit id MUST remain stable for unchanged source bytes,
unchanged extractor profile, and unchanged range selection. If a pipeline changes chunking,
segmentation, OCR, captioning, or alignment behavior, it must use a different extractor profile so old
and new vectors do not masquerade as the same derived unit.

## 4. Provider Contracts

0068 adds multimodal provider contracts beside, not inside, the 0050 text-only provider.

The target contract uses a shared envelope with typed modality payloads:

```idl
enum VectorSourceKind {
  pdf,
  image,
  audio,
  video,
  document,
  multimodal
}

interface MultimodalEmbeddingProvider {
  model_id(): string
  dimension(): u32
  supported_source_kinds(): List<VectorSourceKind>
  embed_units(units: List<VectorSourceUnitInput>): Future<List<Vector>>
}
```

The provider receives bounded, already-decoded or referenced units. Decoding large media, OCR,
captioning, transcription, scene detection, and chunk selection are explicit pipeline steps with
their own extractor profiles. A provider may combine extraction and embedding only when it still
reports the extractor profile and source unit metadata with the same fidelity as a split pipeline.

Provider settings are Loom-owned typed settings, following the 0062 instance-setting rule. Runtime
libraries may be used underneath, but hidden model defaults must not define public Loom behavior.

## 5. Versioning And Recompute

Non-text embeddings follow the same recompute discipline as text embeddings:

- The source asset and source unit records are the versioned truth.
- Derived vectors may be rebuilt from source units and provider profiles.
- Accelerator indexes are derived and remain outside canonical identity, as defined by 0017.
- If vectors are stored and synced directly, the provider path must identify the canonical model,
  settings, and arithmetic profile clearly enough for detect-and-warn and conformance.

Backends may produce small floating-point differences. That is acceptable for local recompute indexes
and unacceptable for a claimed byte-identical stored-vector profile unless the profile defines a
canonical deterministic path.

## 6. Integration Rules

- `loom inference model download` may later download curated multimodal models only after those
  models declare a 0068 provider contract and supported source kinds.
- `loom vector` may expose source-unit search and citation helpers, but 0017 remains the raw vector
  storage owner.
- `loom search` and 0064 may consume 0068 source units for hybrid search, but they do not own
  extraction profiles.
- GraphRAG may reference source units as evidence nodes, while graph storage and graph query behavior
  remain owned by 0016 and 0040.
- Hosted vector compatibility profiles may accept vendor integrated-embedding fields only when a
  0068 pipeline or 0050 text provider is configured. Otherwise they must return a stable unsupported
  or provider-not-configured error.

## 7. Security And Policy

- Never run remote model code as part of extraction or embedding.
- Never download a model implicitly during a read or search.
- Treat source media as user data. Extracted text, captions, thumbnails, transcripts, and frame
  metadata inherit the source asset's access policy unless an owning spec defines stricter rules.
- Media extractors must bound input size, decoded frame count, page count, audio duration, and output
  unit count.
- OCR, captioning, transcription, and scene detection may reveal sensitive derived content. Audit and
  hosted responses must identify derived source units without leaking inaccessible source payloads.

## 8. Conformance

The first source-backed implementation must include:

- deterministic source unit id tests for unchanged source and extractor profiles;
- negative tests showing changed chunking, segmentation, OCR, captioning, or transcription profiles
  produce different unit identities;
- fake-provider vector upsert tests that prove 0068 units become ordinary 0017 vectors with typed
  metadata;
- access-control tests for source unit reads and derived text or transcript exposure;
- no-network tests for each promoted source kind;
- manual or ignored tests for any runtime that requires large model downloads or special hardware.

## 9. Resolved Decisions

- **RD1 - Spec ownership.** 0068 owns non-text vector source pipelines. 0017, 0050, and 0062 remain
  scoped to raw vectors, text embeddings, and model acquisition.
- **RD2 - Source truth.** Non-text source assets and source unit records are the versioned truth.
  Derived vectors and accelerators are rebuildable unless a future profile explicitly stores vectors
  as canonical data.
- **RD3 - Provider split.** Text-only providers stay in 0050. Multimodal providers use a 0068 contract
  with explicit supported source kinds and source unit metadata.
- **RD4 - Acquisition split.** Downloading or activating multimodal models is 0062 work only after a
  model declares compatibility with this spec's source unit and provider contracts.

## 10. Target Work

| Order | Work item | Owner | Exit criteria |
| --- | --- | --- | --- |
| T1 | Source unit encoding | 0068 | Canonical source unit shape, deterministic ids, and conformance vectors are defined. |
| T2 | PDF extraction profile | 0068 plus files/CAS/document | Page and region source units, extracted text policy, and metadata mapping are source-backed. |
| T3 | Image extraction profile | 0068 plus files/CAS | Whole-image and region source units, EXIF policy, and caption/OCR boundary are source-backed. |
| T4 | Audio extraction profile | 0068 plus files/CAS | Time-window and transcript source units, speaker metadata policy, and citation ranges are source-backed. |
| T5 | Video extraction profile | 0068 plus files/CAS | Frame, scene, transcript, and audio-alignment units are source-backed. |
| T6 | Multimodal provider adapter | 0068 plus 0062 | At least one curated provider embeds a promoted non-text source kind through typed Loom settings. |
| T7 | Hosted projection | 0068 plus 0008/0064 | Source-unit search, citation, and unsupported-provider behavior are exposed over selected hosted surfaces. |
