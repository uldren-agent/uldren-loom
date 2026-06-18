//! vector-tradeoff - a MODEL (not a real encoder) that measures the
//! "recompute-instead-of-store embeddings" trade-off (the LEANN trick) against
//! the conventional "store FP32 vectors" approach, and shows how
//! content-addressed dedup stacks on top.
//!
//! Everything here is std-only and deterministic so it builds offline and
//! reproduces. The "embedding" is a synthetic, tunable workload - it is NOT a
//! semantic encoder. Its only job is to (a) be deterministic per text and
//! (b) cost a realistic handful of microseconds per call so that recompute
//! latency is non-trivial and comparable to a real on-device encoder.

use std::collections::HashMap;
use std::time::Instant;

const D: usize = 384; // embedding dimension (row in the (N, D) grid)

/// Inner-workload multiplier. Each embedding performs ~ D * K multiply-adds
/// driven by a PRNG seeded from the text bytes. K is tuned (see report) so one
/// embedding lands in the low-single-digit microseconds, modelling a small
/// on-device encoder. Raising K makes recompute proportionally more expensive.
const K: usize = 24;

const TOP_K: usize = 10; // we always retrieve top-10
const QUERIES: usize = 20; // queries averaged per measurement
const DEGREE: usize = 32; // modelled graph out-degree for the LEANN-style index
const VISIT_C: usize = 16; // visited-set constant: visited ~= c * log2(N)

// ---------------------------------------------------------------------------
// Deterministic PRNG (splitmix64). std has no rng, so we roll a tiny one.
// ---------------------------------------------------------------------------
#[inline(always)]
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// FNV-1a 64-bit hash of the text bytes - used both to seed the embedding and
/// as the content address for dedup.
#[inline(always)]
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// The MODEL encoder: text -> [f32; D].
///
/// Deterministic in the text bytes. Performs D*K multiply-adds over PRNG
/// output so the call has a measurable, tunable cost. `black_box`-style
/// accumulation via the returned vector prevents the optimiser from deleting
/// the work.
fn embed(text: &[u8]) -> [f32; D] {
    let seed = fnv1a(text);
    let mut state = seed;
    let mut out = [0.0f32; D];
    for d in 0..D {
        // K multiply-adds per dimension, all derived from the PRNG stream.
        let mut acc = 0.0f32;
        for _ in 0..K {
            let r = splitmix64(&mut state);
            // map to roughly [-1, 1)
            let a = ((r & 0xFFFF) as f32) / 32768.0 - 1.0;
            let b = (((r >> 16) & 0xFFFF) as f32) / 32768.0 - 1.0;
            acc = acc.mul_add(0.999, a * b);
        }
        out[d] = acc;
    }
    // L2-normalise so dot product behaves like cosine similarity.
    let mut norm = 0.0f32;
    for d in 0..D {
        norm += out[d] * out[d];
    }
    let inv = 1.0 / (norm.sqrt() + 1e-9);
    for d in 0..D {
        out[d] *= inv;
    }
    out
}

#[inline(always)]
fn dot(a: &[f32; D], b: &[f32; D]) -> f32 {
    let mut s = 0.0f32;
    for d in 0..D {
        s = a[d].mul_add(b[d], s);
    }
    s
}

/// Keep a running top-10 (max by score). Tiny insertion approach - N is large
/// but TOP_K is 10, so this is cheaper than sorting.
#[inline(always)]
fn push_top(top: &mut Vec<(f32, u32)>, score: f32, id: u32) {
    if top.len() < TOP_K {
        top.push((score, id));
        if top.len() == TOP_K {
            top.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        }
        return;
    }
    // top is kept ascending; top[0] is the smallest of the current top-10.
    if score > top[0].0 {
        top[0] = (score, id);
        // bubble the new smallest to the front
        let mut i = 0;
        while i + 1 < TOP_K && top[i].0 > top[i + 1].0 {
            top.swap(i, i + 1);
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Corpus generation
// ---------------------------------------------------------------------------

/// Generate a synthetic document of pseudo-random words. Deterministic in the
/// (seed) so the corpus reproduces. Returns the raw bytes.
fn gen_doc(seed: u64) -> Vec<u8> {
    let mut state = seed ^ 0xD1B5_4A32_D192_ED03;
    // 40..80 "words", each 3..9 chars - averages a few hundred bytes/doc,
    // realistic for a chunked passage.
    let n_words = 40 + (splitmix64(&mut state) % 40) as usize;
    let mut s = Vec::with_capacity(n_words * 8);
    for _ in 0..n_words {
        let wlen = 3 + (splitmix64(&mut state) % 7) as usize;
        for _ in 0..wlen {
            let c = b'a' + (splitmix64(&mut state) % 26) as u8;
            s.push(c);
        }
        s.push(b' ');
    }
    s
}

/// Build N unique documents (no duplicates).
fn build_corpus(n: usize) -> Vec<Vec<u8>> {
    (0..n).map(|i| gen_doc(i as u64)).collect()
}

/// Build N documents where ~`dup_frac` of them are exact duplicates of an
/// earlier document. Deterministic. Returns the corpus.
fn build_corpus_with_dups(n: usize, dup_frac: f64) -> Vec<Vec<u8>> {
    let mut docs: Vec<Vec<u8>> = Vec::with_capacity(n);
    let mut state = 0xABCD_1234_5678_9F0Eu64;
    let mut n_unique_seed = 0u64;
    for i in 0..n {
        let roll = (splitmix64(&mut state) % 1000) as f64 / 1000.0;
        if i > 0 && roll < dup_frac {
            // duplicate an earlier document exactly
            let j = (splitmix64(&mut state) as usize) % i;
            docs.push(docs[j].clone());
        } else {
            docs.push(gen_doc(n_unique_seed));
            n_unique_seed += 1;
        }
    }
    docs
}

fn total_text_bytes(docs: &[Vec<u8>]) -> usize {
    docs.iter().map(|d| d.len()).sum()
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------
fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn main() {
    println!("================================================================================");
    println!(" vector-tradeoff - MODEL of recompute-vs-store embeddings (LEANN trick)");
    println!("   NOTE: the 'embedding' is a synthetic, deterministic, tunable workload,");
    println!("         NOT a real semantic encoder. It exists only to give recompute a");
    println!("         realistic per-call cost so latencies are comparable.");
    println!("================================================================================");

    // --- Calibrate / report the per-embedding cost --------------------------
    {
        // warm up
        let warm = gen_doc(999_999);
        let mut sink = 0.0f32;
        for _ in 0..1000 {
            sink += embed(&warm)[0];
        }
        // measure
        let trials = 50_000usize;
        let t0 = Instant::now();
        for i in 0..trials {
            let v = embed(&warm);
            sink += v[(i as usize) & (D - 1)];
        }
        let elapsed = t0.elapsed();
        let per_us = elapsed.as_secs_f64() * 1e6 / trials as f64;
        println!();
        println!("Config: D = {D}, K = {K} (inner multiply-adds per dim = D*K = {}),", D * K);
        println!("        top-K = {TOP_K}, queries averaged = {QUERIES}, graph degree = {DEGREE},");
        println!("        visited-set model = {VISIT_C} * log2(N).");
        println!();
        println!(
            "Measured per-embedding cost: {:.3} µs/embedding  (sink={:.3}, prevents dead-code elim)",
            per_us, sink
        );
        println!("  -> recomputing all N embeddings costs ~ N * {:.3} µs.", per_us);
    }

    // -----------------------------------------------------------------------
    // Main grid: N in {1k, 10k, 100k}, D = 384
    // -----------------------------------------------------------------------
    println!();
    println!("================================================================================");
    println!(" MAIN GRID (unique corpus, D = {D})");
    println!("================================================================================");
    println!();
    println!(
        "{:>8} | {:<28} | {:>12} | {:>12} | {:>11}",
        "N", "mode", "storage(MB)", "vs A (×)", "q-lat(ms)"
    );
    println!("{}", "-".repeat(82));

    let grid_n = [1_000usize, 10_000, 100_000];

    for &n in &grid_n {
        let docs = build_corpus(n);
        let text_bytes = total_text_bytes(&docs);

        // Precompute / "store" all vectors for mode A.
        let stored: Vec<[f32; D]> = docs.iter().map(|d| embed(d)).collect();

        // Build query vectors (deterministic, drawn from doc-space).
        let queries: Vec<[f32; D]> = (0..QUERIES)
            .map(|q| embed(&gen_doc(10_000_000 + q as u64)))
            .collect();

        // ---- Mode A: store FP32 + exact scan ------------------------------
        let vec_bytes_a = n * D * 4;
        let storage_a = vec_bytes_a + text_bytes;
        let t0 = Instant::now();
        let mut sink = 0.0f32;
        for q in &queries {
            let mut top: Vec<(f32, u32)> = Vec::with_capacity(TOP_K);
            for (i, v) in stored.iter().enumerate() {
                push_top(&mut top, dot(q, v), i as u32);
            }
            sink += top[0].0;
        }
        let lat_a = t0.elapsed().as_secs_f64() * 1e3 / QUERIES as f64;

        // ---- Mode B: recompute-all + exact scan ---------------------------
        let storage_b = text_bytes;
        let t0 = Instant::now();
        for q in &queries {
            let mut top: Vec<(f32, u32)> = Vec::with_capacity(TOP_K);
            for (i, d) in docs.iter().enumerate() {
                let v = embed(d); // recompute, do not store
                push_top(&mut top, dot(q, &v), i as u32);
            }
            sink += top[0].0;
        }
        let lat_b = t0.elapsed().as_secs_f64() * 1e3 / QUERIES as f64;

        // ---- Mode C: recompute-visited (graph-pruned, LEANN approx) -------
        let graph_bytes = n * DEGREE * 4; // u32 neighbour ids
        let storage_c = text_bytes + graph_bytes;
        // visited ~= c * log2(N), clamped to n
        let log2n = (n as f64).log2();
        let visited = ((VISIT_C as f64 * log2n).round() as usize).min(n).max(1);
        // model the visited set as a deterministic stride over the corpus
        let stride = (n / visited).max(1);
        let t0 = Instant::now();
        for q in &queries {
            let mut top: Vec<(f32, u32)> = Vec::with_capacity(TOP_K);
            let mut visited_count = 0usize;
            let mut idx = 0usize;
            while visited_count < visited && idx < n {
                let v = embed(&docs[idx]); // recompute only visited nodes
                push_top(&mut top, dot(q, &v), idx as u32);
                idx += stride;
                visited_count += 1;
            }
            sink += top[0].0;
        }
        let lat_c = t0.elapsed().as_secs_f64() * 1e3 / QUERIES as f64;

        // ---- print rows ----------------------------------------------------
        println!(
            "{:>8} | {:<28} | {:>12.3} | {:>12} | {:>11.4}",
            n, "A. store-FP32 + exact", mb(storage_a), "1.00", lat_a
        );
        println!(
            "{:>8} | {:<28} | {:>12.3} | {:>12.4} | {:>11.4}",
            "", "B. recompute-all + exact", mb(storage_b),
            storage_b as f64 / storage_a as f64, lat_b
        );
        println!(
            "{:>8} | {:<28} | {:>12.3} | {:>12.4} | {:>11.4}",
            "",
            format!("C. recompute-visited(~{})", visited),
            mb(storage_c),
            storage_c as f64 / storage_a as f64,
            lat_c
        );
        // storage reduction summary for this N
        let red_b = 100.0 * (1.0 - storage_b as f64 / storage_a as f64);
        let red_c = 100.0 * (1.0 - storage_c as f64 / storage_a as f64);
        println!(
            "{:>8} | {:<28} | storage cut: B = {:>5.1}%   C = {:>5.1}%  (vs A)   [sink={:.2}]",
            "", "", red_b, red_c, sink
        );
        println!("{}", "-".repeat(82));
    }

    // -----------------------------------------------------------------------
    // Dedup demonstration: N = 100k, D = 384, 30% exact duplicates
    // -----------------------------------------------------------------------
    let n = 100_000usize;
    let dup_frac = 0.30;
    println!();
    println!("================================================================================");
    println!(" DEDUP DEMONSTRATION  (N = {n}, D = {D}, {:.0}% exact duplicates)", dup_frac * 100.0);
    println!("================================================================================");

    let docs = build_corpus_with_dups(n, dup_frac);
    let text_bytes_naive = total_text_bytes(&docs);

    // unique set, keyed by content address (FNV-1a of text)
    let mut seen: HashMap<u64, usize> = HashMap::new();
    let mut unique_idx: Vec<usize> = Vec::new();
    for (i, d) in docs.iter().enumerate() {
        let h = fnv1a(d);
        seen.entry(h).or_insert_with(|| {
            unique_idx.push(i);
            unique_idx.len() - 1
        });
    }
    let n_unique = unique_idx.len();
    let dup_count = n - n_unique;
    let text_bytes_unique: usize = unique_idx.iter().map(|&i| docs[i].len()).sum();

    println!();
    println!(
        "Corpus: {} documents, {} unique, {} duplicates ({:.1}% duplicate rate).",
        n,
        n_unique,
        dup_count,
        100.0 * dup_count as f64 / n as f64
    );

    // ---- Store-FP32 NAIVE (no dedup) --------------------------------------
    let vec_bytes_naive = n * D * 4;
    let store_naive = vec_bytes_naive + text_bytes_naive;

    // ---- Store-FP32 WITH content-addressed dedup --------------------------
    // store each unique text + vector exactly once; duplicates become a
    // pointer (8 bytes) into the unique table.
    let ptr_bytes = n * 8; // one u64 content-address / index per document
    let vec_bytes_dedup = n_unique * D * 4;
    let store_dedup = vec_bytes_dedup + text_bytes_unique + ptr_bytes;

    // ---- Recompute mode C WITH dedup --------------------------------------
    // drop vectors entirely; keep unique text + a graph over unique nodes +
    // the dedup pointer table.
    let graph_bytes_dedup = n_unique * DEGREE * 4;
    let recompute_c_dedup = text_bytes_unique + graph_bytes_dedup + ptr_bytes;

    // ---- combined effect: dedup AND drop-vectors vs naive store baseline --
    let combined = recompute_c_dedup;

    println!();
    println!(
        "{:<46} | {:>12} | {:>10}",
        "scheme", "bytes(MB)", "vs naive"
    );
    println!("{}", "-".repeat(74));
    println!(
        "{:<46} | {:>12.3} | {:>9}",
        "Store-FP32 NAIVE (vectors+text, no dedup)",
        mb(store_naive),
        "1.00×"
    );
    println!(
        "{:<46} | {:>12.3} | {:>8.3}×",
        "Store-FP32 + content-addressed dedup",
        mb(store_dedup),
        store_dedup as f64 / store_naive as f64
    );
    println!(
        "{:<46} | {:>12.3} | {:>8.3}×",
        "Recompute-C (drop vectors) + dedup",
        mb(recompute_c_dedup),
        recompute_c_dedup as f64 / store_naive as f64
    );
    println!("{}", "-".repeat(74));

    let saved_by_dedup_alone = 100.0 * (1.0 - store_dedup as f64 / store_naive as f64);
    let saved_by_drop_vectors_only = {
        // recompute-C WITHOUT dedup, for isolating the drop-vectors effect
        let graph_bytes = n * DEGREE * 4;
        let rc_no_dedup = text_bytes_naive + graph_bytes + 0; // no ptr table needed w/o dedup
        100.0 * (1.0 - rc_no_dedup as f64 / store_naive as f64)
    };
    let saved_combined = 100.0 * (1.0 - combined as f64 / store_naive as f64);

    println!();
    println!("Breakdown of savings vs the NAIVE store-FP32 baseline:");
    println!(
        "  • dedup ALONE (still storing vectors):        {:>6.1}% smaller",
        saved_by_dedup_alone
    );
    println!(
        "  • drop-vectors ALONE (recompute-C, no dedup): {:>6.1}% smaller",
        saved_by_drop_vectors_only
    );
    println!(
        "  • COMBINED (dedup AND drop-vectors):          {:>6.1}% smaller   <== stacked",
        saved_combined
    );
    println!();
    println!(
        "Absolute: naive = {:.2} MB  ->  combined = {:.2} MB   ({:.1}× smaller)",
        mb(store_naive),
        mb(combined),
        store_naive as f64 / combined as f64
    );
    println!("================================================================================");
}
