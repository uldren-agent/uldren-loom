//! Prototype: compare per-object compression-frame candidates.
//!
//! Loom compresses **per object** (each record's payload independently), not one big stream, so this
//! measures each codec over *sets of objects* across representative workloads, reporting compression
//! ratio, space saved, and compress/decompress throughput. zstd is the C-bound libzstd (best ratio,
//! not wasm32-clean); lz4_flex and miniz_oxide are pure-Rust and compile to wasm32 (so the browser's
//! IndexedDB/OPFS-backed store can use the same frame - compression is NOT native-only).
//!
//! Run: `cargo run --release` in this directory.

use std::time::{Duration, Instant};

/// Deterministic SplitMix64 - reproducible workloads, no rng dependency.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const WORDS: &[&str] = &[
    "loom", "object", "commit", "tree", "branch", "merge", "digest", "blake3", "workspace", "vector",
    "ledger", "graph", "chunk", "store", "the", "a", "of", "to", "and", "in", "is", "content",
    "addressed", "version", "engine", "facet", "prolly", "superblock", "wasm", "rust",
];

/// Source-like / prose text: highly compressible (repeated vocabulary + structure).
fn gen_text() -> Vec<Vec<u8>> {
    let mut r = Rng(1);
    (0..512)
        .map(|_| {
            let mut s = String::new();
            let words = 200 + r.below(800);
            for i in 0..words {
                s.push_str(WORDS[r.below(WORDS.len())]);
                s.push(if i % 12 == 11 { '\n' } else { ' ' });
            }
            s.into_bytes()
        })
        .collect()
}

/// JSON-like structured records: compressible (repeated keys/structure).
fn gen_json() -> Vec<Vec<u8>> {
    let mut r = Rng(2);
    (0..4000)
        .map(|i| {
            let tags: Vec<String> = (0..r.below(5))
                .map(|_| format!("\"{}\"", WORDS[r.below(WORDS.len())]))
                .collect();
            format!(
                "{{\"id\":{i},\"name\":\"{}\",\"ts\":{},\"active\":{},\"score\":{},\"tags\":[{}]}}",
                WORDS[r.below(WORDS.len())],
                1_700_000_000 + r.below(50_000_000),
                i % 2 == 0,
                r.below(1000),
                tags.join(",")
            )
            .into_bytes()
        })
        .collect()
}

/// Log lines: extremely repetitive (timestamps + levels + a few templates).
fn gen_logs() -> Vec<Vec<u8>> {
    let mut r = Rng(3);
    let levels = ["INFO", "WARN", "ERROR", "DEBUG"];
    (0..6000)
        .map(|i| {
            format!(
                "2026-06-18T{:02}:{:02}:{:02}.{:03}Z {} loom::{}: request {} completed in {}ms\n",
                r.below(24),
                r.below(60),
                r.below(60),
                r.below(1000),
                levels[r.below(levels.len())],
                WORDS[r.below(WORDS.len())],
                i,
                r.below(500)
            )
            .into_bytes()
        })
        .collect()
}

/// Many small objects (<=256 B): the loom-like case (kv values, doc fields, small blobs) where
/// per-object framing overhead bites hardest.
fn gen_small() -> Vec<Vec<u8>> {
    let mut r = Rng(4);
    (0..30_000)
        .map(|_| {
            let n = 32 + r.below(224);
            let mut s = String::new();
            while s.len() < n {
                s.push_str(WORDS[r.below(WORDS.len())]);
                s.push(' ');
            }
            s.truncate(n);
            s.into_bytes()
        })
        .collect()
}

/// Random / already-compressed bytes (jpeg, zip, encrypted): incompressible - codecs should detect
/// this and not expand much (and a per-object policy would store these identity).
fn gen_random() -> Vec<Vec<u8>> {
    let mut r = Rng(5);
    (0..2000)
        .map(|_| {
            let n = 1024 + r.below(8192);
            (0..n).map(|_| (r.next() & 0xff) as u8).collect()
        })
        .collect()
}

fn compress(codec: &str, data: &[u8]) -> Vec<u8> {
    match codec {
        "none" => data.to_vec(),
        "lz4_flex" => lz4_flex::compress_prepend_size(data),
        "miniz(6)" => miniz_oxide::deflate::compress_to_vec(data, 6),
        "zstd(3)" => zstd::bulk::compress(data, 3).unwrap(),
        "zstd(19)" => zstd::bulk::compress(data, 19).unwrap(),
        _ => unreachable!(),
    }
}

fn decompress(codec: &str, comp: &[u8], orig_len: usize) -> Vec<u8> {
    match codec {
        "none" => comp.to_vec(),
        "lz4_flex" => lz4_flex::decompress_size_prepended(comp).unwrap(),
        "miniz(6)" => miniz_oxide::inflate::decompress_to_vec(comp).unwrap(),
        "zstd(3)" | "zstd(19)" => zstd::bulk::decompress(comp, orig_len).unwrap(),
        _ => unreachable!(),
    }
}

fn mbps(bytes: usize, dur: Duration) -> f64 {
    let secs = dur.as_secs_f64();
    if secs <= 0.0 {
        return f64::INFINITY;
    }
    (bytes as f64 / 1_000_000.0) / secs
}

fn main() {
    let workloads: Vec<(&str, Vec<Vec<u8>>)> = vec![
        ("text/source", gen_text()),
        ("json-records", gen_json()),
        ("logs", gen_logs()),
        ("small-objects<=256B", gen_small()),
        ("random/incompressible", gen_random()),
    ];
    let codecs = ["none", "lz4_flex", "miniz(6)", "zstd(3)", "zstd(19)"];

    println!(
        "{:<22} {:<10} {:>6} {:>9} {:>9} {:>7} {:>9} {:>9}",
        "workload", "codec", "objs", "orig KB", "comp KB", "ratio", "comp MB/s", "dcmp MB/s"
    );
    println!("{}", "-".repeat(92));

    for (wname, objs) in &workloads {
        let orig: usize = objs.iter().map(Vec::len).sum();
        let avg = orig / objs.len().max(1);
        for codec in codecs {
            // Compress all objects, timing only the codec calls.
            let t0 = Instant::now();
            let comp: Vec<Vec<u8>> = objs.iter().map(|o| compress(codec, o)).collect();
            let ctime = t0.elapsed();
            let comp_total: usize = comp.iter().map(Vec::len).sum();

            // Decompress all + verify round-trip.
            let t1 = Instant::now();
            for (c, o) in comp.iter().zip(objs) {
                let back = decompress(codec, c, o.len());
                assert_eq!(&back, o, "round-trip mismatch: {codec} / {wname}");
            }
            let dtime = t1.elapsed();

            let ratio = orig as f64 / comp_total as f64;
            println!(
                "{:<22} {:<10} {:>6} {:>9} {:>9} {:>6.2}x {:>9.0} {:>9.0}",
                wname,
                codec,
                objs.len(),
                orig / 1024,
                comp_total / 1024,
                ratio,
                mbps(orig, ctime),
                mbps(orig, dtime),
            );
        }
        println!("  (avg object {avg} B)");
    }
}
