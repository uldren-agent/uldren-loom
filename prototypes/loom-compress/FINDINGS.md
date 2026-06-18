# Compression-frame prototype findings

Per-object compression candidates measured over representative workloads. Loom compresses **per object**
(each record's payload independently, no shared dictionary), so these are sets of objects, not one
stream. `cargo run --release` in this dir reproduces the table (deterministic workloads).

## Results (this run)

| workload | avg obj | none | lz4_flex | miniz(6) | zstd(3) | zstd(19) |
|---|--:|--:|--:|--:|--:|--:|
| text/source | 3.7 KB | 1.00x | 2.08x | **3.71x** | 3.53x | 3.89x |
| json-records | 93 B | 1.00x | 0.95x (expands) | 1.10x | 1.00x | 1.05x |
| logs | 75 B | 1.00x | 0.93x | 0.96x | 0.89x | 0.90x |
| small-objects <=256B | 143 B | 1.00x | 1.16x | **1.61x** | 1.41x | 1.46x |
| random/incompressible | 5 KB | 1.00x | ~1.00x | ~1.00x | ~1.00x | ~1.00x |

Throughput (MB/s, this machine): lz4_flex compress ~450-4200 / decompress ~1700-21000 (fastest by far);
zstd(3) compress ~45-1080; miniz(6) compress ~20-126 (slowest); zstd(19) compress ~10-46 (write-path
prohibitive). Decompress is fast for all.

## Conclusions

1. **Per-object on small payloads (the common Loom case) often loses.** At ~75-150 B objects, lz4_flex
   and zstd(3) *expand* json/logs; only DEFLATE (miniz) reliably shrinks small objects (<=256 B -> 1.61x).
   So a per-object frame **must** store identity when compression doesn't shrink the payload, and is
   best gated to payloads above a threshold (~256 B-1 KB).
2. **Big wins are on large, compressible objects** (text/source -> 3.5-3.9x). That's where a frame pays.
3. **zstd does not decisively win here.** Without a shared dictionary on small objects, zstd(3) is
   *worse* on ratio than DEFLATE and slower than lz4; zstd(19) gives the best ratio but at
   write-prohibitive speed. Its C build + non-`wasm32` cost is not justified by these numbers.
4. **A pure-Rust compressor is the right call** - and it matters: the browser's IndexedDB/OPFS-backed
   store is `wasm32`, so a wasm-clean codec lets the browser store compress too (compression is **not**
   native-only). Both candidates compile to `wasm32`:
   - **miniz_oxide (DEFLATE)** - best pure-Rust ratio (3.7x text, 1.6x small), slow compress.
   - **lz4_flex (LZ4)** - fastest by a wide margin, modest ratio (2x text), expands tiny objects.

## Compiled size (linked code, stripped release, native aarch64)

Minimal binary diffed against a 312 KiB baseline (own profile: `lto=thin`, `strip`, `panic=abort`):

| codec | linked-code delta |
|---|--:|
| lz4_flex | ~8 KiB |
| miniz_oxide | ~40 KiB |
| both | ~48 KiB (= sum; no shared code) |

Small in absolute terms. The two share nothing, so "include both" costs ~48 KiB. **The per-object
frame-id makes this a non-decision up front:** ship one codec under a new frame id now; adding the
second later is just another frame id (decompress dispatches on the stored byte) with **zero format
migration**. So runtime switching between lz4 (fast) and miniz (ratio) is a clean *additive* capability
to add only if a latency-sensitive write path actually needs lz4 - not a now-or-never choice.
(wasm32 absolute sizes differ; the ~5:1 lz4:miniz ratio is algorithm-driven and should roughly carry.)

## Recommendation

**miniz_oxide (DEFLATE), per-object, compress-only-if-it-shrinks, gated to payloads >= ~1 KB** - captures
the large-object wins, sidesteps the small-object/incompressible losses, stays pure-Rust + wasm-clean,
and adds no C build. Choose **lz4_flex** instead if write throughput dominates over ratio. **zstd is not
recommended**: no decisive ratio win on per-object data, and it forfeits wasm parity + the zero-C-dep
posture. (Owner decides; this prototype is the evidence.)
