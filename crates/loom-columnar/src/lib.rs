//! The columnar facet - a versioned, append-oriented typed dataset stored as ordered segments. Pure
//! Rust, `wasm32`-clean, deterministic, and versioned through the engine.
//!
//! Segment policy: the writer honors a **target segment size** (rows per segment), rolling to a new
//! segment at the target so it never surprises with background rewrites, and the caller runs
//! **explicit [`ColumnarSet::compact`]** to merge the small-segment tail. This module does not
//! reconcile same-segment edits across branches.

use loom_types::digest::{Algo, DIGEST_LEN, Digest};
use loom_types::error::{LoomError, Result};
use loom_types::tabular::{CmpOp, ColumnType, Value, cell_from, cell_value};
use std::cmp::Ordering;

#[cfg(feature = "arrow")]
mod arrow;
#[cfg(feature = "arrow")]
pub use arrow::{
    columnar_from_arrow_ipc, columnar_from_parquet, columnar_to_arrow_ipc, columnar_to_parquet,
};

mod cbor {
    use loom_types::digest::Digest;
    use loom_types::error::{LoomError, Result};

    pub use loom_codec::Value;

    pub fn encode(value: &Value) -> Vec<u8> {
        loom_codec::encode(value).expect("columnar CBOR value is encodable")
    }

    pub fn decode(bytes: &[u8]) -> Result<Value> {
        loom_codec::decode(bytes).map_err(err)
    }

    pub fn decode_array(bytes: &[u8]) -> Result<Vec<Value>> {
        as_array(decode(bytes)?)
    }

    pub fn digest_value(digest: &Digest) -> Value {
        Value::Bytes(digest.bytes().to_vec())
    }

    pub fn as_array(value: Value) -> Result<Vec<Value>> {
        match value {
            Value::Array(items) => Ok(items),
            _ => Err(LoomError::corrupt("expected CBOR array")),
        }
    }

    pub fn as_bytes(value: Value) -> Result<Vec<u8>> {
        match value {
            Value::Bytes(bytes) => Ok(bytes),
            _ => Err(LoomError::corrupt("expected CBOR bytes")),
        }
    }

    fn as_text(value: Value) -> Result<String> {
        match value {
            Value::Text(text) => Ok(text),
            _ => Err(LoomError::corrupt("expected CBOR text")),
        }
    }

    fn as_uint(value: Value) -> Result<u64> {
        match value {
            Value::Uint(value) => Ok(value),
            _ => Err(LoomError::corrupt("expected CBOR unsigned integer")),
        }
    }

    fn err(error: loom_codec::CodecError) -> LoomError {
        LoomError::corrupt(format!("CBOR decode failed: {error}"))
    }

    pub struct Fields {
        items: Vec<Value>,
        index: usize,
    }

    impl Fields {
        pub fn new(items: Vec<Value>) -> Self {
            Self { items, index: 0 }
        }

        pub fn next_field(&mut self) -> Result<Value> {
            let value = self
                .items
                .get(self.index)
                .cloned()
                .ok_or_else(|| LoomError::corrupt("missing CBOR field"))?;
            self.index += 1;
            Ok(value)
        }

        pub fn uint(&mut self) -> Result<u64> {
            as_uint(self.next_field()?)
        }

        pub fn bytes(&mut self) -> Result<Vec<u8>> {
            as_bytes(self.next_field()?)
        }

        pub fn text(&mut self) -> Result<String> {
            as_text(self.next_field()?)
        }

        pub fn array(&mut self) -> Result<Vec<Value>> {
            as_array(self.next_field()?)
        }

        pub fn end(&self) -> Result<()> {
            if self.index == self.items.len() {
                Ok(())
            } else {
                Err(LoomError::corrupt("trailing CBOR fields"))
            }
        }
    }
}

const DEFAULT_TARGET_SEGMENT_ROWS: usize = 8192;
const COLUMNAR_FORMAT_VERSION: u64 = 2;
const COLUMNAR_SEGMENT_ENCODING_NATIVE_CBOR: u64 = 1;
const COLUMNAR_STATISTICS_BASIC: u64 = 1;
const COLUMNAR_COMPRESSION_NONE: u64 = 0;
const COLUMNAR_SEGMENT_DIGEST_DOMAIN: &[u8] = b"loom-columnar-segment-v1";

/// A versioned columnar dataset: typed columns, rows kept in append order across ordered segments.
#[derive(Debug, Clone)]
pub struct ColumnarSet {
    columns: Vec<(String, ColumnType)>,
    segments: Vec<Vec<Vec<Value>>>, // ordered segments; each segment is a batch of rows
    target_segment_rows: usize,
}

/// Summary metadata for a columnar dataset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnarInspect {
    pub columns: Vec<(String, ColumnType)>,
    pub rows: usize,
    pub segment_count: usize,
    pub target_segment_rows: usize,
    pub source_digest: Digest,
}

/// Stable metadata committed with a columnar dataset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnarManifest {
    pub version: u64,
    pub columns: Vec<(String, ColumnType)>,
    pub target_segment_rows: usize,
    pub statistics_policy: ColumnarStatisticsPolicy,
    pub compression_policy: ColumnarCompressionPolicy,
    pub segments: Vec<ColumnarSegmentManifest>,
}

impl ColumnarManifest {
    /// Canonical structured-root manifest bytes. Segment payload bytes are stored separately.
    pub fn encode(&self) -> Vec<u8> {
        use cbor::Value::{Array, Text, Uint};
        let columns = self
            .columns
            .iter()
            .map(|(name, ty)| Array(vec![Text(name.clone()), Uint(u64::from(ty.tag()))]))
            .collect();
        let segments = self
            .segments
            .iter()
            .map(|segment| {
                Array(vec![
                    Uint(segment.ordinal as u64),
                    Uint(segment.row_start as u64),
                    Uint(segment.row_count as u64),
                    Uint(u64::from(segment.encoding)),
                    cbor::digest_value(&segment.digest),
                    encode_segment_statistics(&segment.statistics),
                ])
            })
            .collect();
        cbor::encode(&Array(vec![
            Uint(self.version),
            Array(columns),
            Uint(self.target_segment_rows as u64),
            Uint(u64::from(self.statistics_policy)),
            Uint(u64::from(self.compression_policy)),
            Array(segments),
        ]))
    }

    /// Decode structured-root manifest bytes.
    pub fn decode(bytes: &[u8], algo: Algo) -> Result<Self> {
        let mut f = cbor::Fields::new(cbor::decode_array(bytes)?);
        let version = f.uint()?;
        let columns_raw = f.array()?;
        let target_segment_rows = usize::try_from(f.uint()?)
            .map_err(|_| LoomError::corrupt("columnar target out of range"))?;
        let statistics_policy = ColumnarStatisticsPolicy::decode(f.uint()?)?;
        let compression_policy = ColumnarCompressionPolicy::decode(f.uint()?)?;
        let segments_raw = f.array()?;
        f.end()?;

        let mut columns = Vec::with_capacity(columns_raw.len());
        for column in columns_raw {
            let mut cf = cbor::Fields::new(cbor::as_array(column)?);
            let name = cf.text()?;
            let tag = u8::try_from(cf.uint()?)
                .map_err(|_| LoomError::corrupt("column type tag out of range"))?;
            cf.end()?;
            columns.push((name, ColumnType::from_tag(tag)?));
        }

        let ncols = columns.len();
        let mut segments = Vec::with_capacity(segments_raw.len());
        for item in segments_raw {
            let mut sf = cbor::Fields::new(cbor::as_array(item)?);
            let ordinal = usize::try_from(sf.uint()?)
                .map_err(|_| LoomError::corrupt("columnar segment ordinal out of range"))?;
            let row_start = usize::try_from(sf.uint()?)
                .map_err(|_| LoomError::corrupt("columnar segment row start out of range"))?;
            let row_count = usize::try_from(sf.uint()?)
                .map_err(|_| LoomError::corrupt("columnar segment row count out of range"))?;
            let encoding = ColumnarSegmentEncoding::decode(sf.uint()?)?;
            let digest = digest_from_value(sf.next_field()?, algo)?;
            let statistics = decode_segment_statistics(sf.next_field()?, ncols)?;
            sf.end()?;
            segments.push(ColumnarSegmentManifest {
                ordinal,
                row_start,
                row_count,
                encoding,
                digest,
                statistics,
            });
        }
        Ok(Self {
            version,
            columns,
            target_segment_rows,
            statistics_policy,
            compression_policy,
            segments,
        })
    }
}

/// The deterministic statistics profile committed for each segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnarStatisticsPolicy {
    Basic,
}

impl From<ColumnarStatisticsPolicy> for u64 {
    fn from(value: ColumnarStatisticsPolicy) -> Self {
        match value {
            ColumnarStatisticsPolicy::Basic => COLUMNAR_STATISTICS_BASIC,
        }
    }
}

impl ColumnarStatisticsPolicy {
    fn decode(value: u64) -> Result<Self> {
        match value {
            COLUMNAR_STATISTICS_BASIC => Ok(Self::Basic),
            _ => Err(LoomError::corrupt("unknown columnar statistics policy")),
        }
    }
}

/// The deterministic compression profile committed for each segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnarCompressionPolicy {
    None,
}

impl From<ColumnarCompressionPolicy> for u64 {
    fn from(value: ColumnarCompressionPolicy) -> Self {
        match value {
            ColumnarCompressionPolicy::None => COLUMNAR_COMPRESSION_NONE,
        }
    }
}

impl ColumnarCompressionPolicy {
    fn decode(value: u64) -> Result<Self> {
        match value {
            COLUMNAR_COMPRESSION_NONE => Ok(Self::None),
            _ => Err(LoomError::corrupt("unknown columnar compression policy")),
        }
    }
}

/// One ordered segment entry in the committed columnar manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnarSegmentManifest {
    pub ordinal: usize,
    pub row_start: usize,
    pub row_count: usize,
    pub encoding: ColumnarSegmentEncoding,
    pub digest: Digest,
    pub statistics: ColumnarSegmentStatistics,
}

/// One durable segment payload paired with the manifest entry that references it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnarSegmentMaterial {
    pub manifest: ColumnarSegmentManifest,
    pub bytes: Vec<u8>,
}

/// The committed segment byte encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnarSegmentEncoding {
    NativeCborRows,
}

impl From<ColumnarSegmentEncoding> for u64 {
    fn from(value: ColumnarSegmentEncoding) -> Self {
        match value {
            ColumnarSegmentEncoding::NativeCborRows => COLUMNAR_SEGMENT_ENCODING_NATIVE_CBOR,
        }
    }
}

impl ColumnarSegmentEncoding {
    fn decode(value: u64) -> Result<Self> {
        match value {
            COLUMNAR_SEGMENT_ENCODING_NATIVE_CBOR => Ok(Self::NativeCborRows),
            _ => Err(LoomError::corrupt("unknown columnar segment encoding")),
        }
    }
}

/// Per-column statistics for one segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnarSegmentStatistics {
    pub columns: Vec<ColumnarColumnStatistics>,
}

/// Deterministic statistics for one column inside one segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnarColumnStatistics {
    pub null_count: usize,
    pub min: Option<Value>,
    pub max: Option<Value>,
}

/// Aggregate operation over a columnar dataset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnarAggregateOp {
    Count,
    CountNonNull,
    Min,
    Max,
    Sum,
}

/// One aggregate expression. `Count` ignores `column`; every other operation requires one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnarAggregate {
    pub op: ColumnarAggregateOp,
    pub column: Option<String>,
}

impl ColumnarSet {
    /// A new dataset over `columns`, rolling to a new segment every `target_segment_rows` rows
    /// (0 selects the default). `INVALID_ARGUMENT` if `columns` is empty.
    pub fn new(columns: Vec<(String, ColumnType)>, target_segment_rows: usize) -> Result<Self> {
        if columns.is_empty() {
            return Err(LoomError::invalid("columnar dataset has no columns"));
        }
        let target = if target_segment_rows == 0 {
            DEFAULT_TARGET_SEGMENT_ROWS
        } else {
            target_segment_rows
        };
        Ok(Self {
            columns,
            segments: Vec::new(),
            target_segment_rows: target,
        })
    }

    /// The column `(name, type)` list.
    pub fn columns(&self) -> &[(String, ColumnType)] {
        &self.columns
    }
    /// Total row count across all segments.
    pub fn rows(&self) -> usize {
        self.segments.iter().map(Vec::len).sum()
    }
    /// Whether the dataset has no rows.
    pub fn is_empty(&self) -> bool {
        self.rows() == 0
    }
    /// Number of segments.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }
    /// The configured target rows-per-segment.
    pub fn target_segment_rows(&self) -> usize {
        self.target_segment_rows
    }

    /// The source-backed manifest that defines this dataset's canonical identity.
    pub fn manifest(&self) -> ColumnarManifest {
        self.manifest_with_algo(Algo::Blake3)
    }

    /// The source-backed manifest under an explicit identity-profile algorithm.
    pub fn manifest_with_algo(&self, algo: Algo) -> ColumnarManifest {
        let mut row_start = 0usize;
        let segments = self
            .segments
            .iter()
            .enumerate()
            .map(|(ordinal, rows)| {
                let segment_bytes = encode_segment_rows(rows);
                let manifest = ColumnarSegmentManifest {
                    ordinal,
                    row_start,
                    row_count: rows.len(),
                    encoding: ColumnarSegmentEncoding::NativeCborRows,
                    digest: segment_digest(algo, &segment_bytes),
                    statistics: segment_statistics(rows, self.columns.len()),
                };
                row_start += rows.len();
                manifest
            })
            .collect();
        ColumnarManifest {
            version: COLUMNAR_FORMAT_VERSION,
            columns: self.columns.clone(),
            target_segment_rows: self.target_segment_rows,
            statistics_policy: ColumnarStatisticsPolicy::Basic,
            compression_policy: ColumnarCompressionPolicy::None,
            segments,
        }
    }

    /// Segment payloads under an explicit identity-profile algorithm.
    pub fn segment_materials_with_algo(&self, algo: Algo) -> Vec<ColumnarSegmentMaterial> {
        let mut row_start = 0usize;
        self.segments
            .iter()
            .enumerate()
            .map(|(ordinal, rows)| {
                let bytes = encode_segment_rows(rows);
                let manifest = ColumnarSegmentManifest {
                    ordinal,
                    row_start,
                    row_count: rows.len(),
                    encoding: ColumnarSegmentEncoding::NativeCborRows,
                    digest: segment_digest(algo, &bytes),
                    statistics: segment_statistics(rows, self.columns.len()),
                };
                row_start += rows.len();
                ColumnarSegmentMaterial { manifest, bytes }
            })
            .collect()
    }

    /// Rebuild a dataset from a structured manifest plus durable segment payloads.
    pub fn from_manifest_segments(
        manifest: ColumnarManifest,
        materials: Vec<ColumnarSegmentMaterial>,
        algo: Algo,
    ) -> Result<Self> {
        if manifest.version != COLUMNAR_FORMAT_VERSION {
            return Err(LoomError::corrupt(format!(
                "unknown columnar format version {}",
                manifest.version
            )));
        }
        if manifest.statistics_policy != ColumnarStatisticsPolicy::Basic {
            return Err(LoomError::corrupt("unknown columnar statistics policy"));
        }
        if manifest.compression_policy != ColumnarCompressionPolicy::None {
            return Err(LoomError::corrupt("unknown columnar compression policy"));
        }
        if manifest.segments.len() != materials.len() {
            return Err(LoomError::corrupt(
                "columnar segment manifest arity mismatch",
            ));
        }
        let ncols = manifest.columns.len();
        let mut expected_row_start = 0usize;
        let mut segments = Vec::with_capacity(materials.len());
        for (expected_ordinal, (expected, material)) in
            manifest.segments.iter().zip(materials).enumerate()
        {
            if material.manifest != *expected {
                return Err(LoomError::corrupt("columnar segment manifest mismatch"));
            }
            if expected.ordinal != expected_ordinal {
                return Err(LoomError::corrupt("columnar segment ordinal mismatch"));
            }
            if expected.row_start != expected_row_start {
                return Err(LoomError::corrupt("columnar segment row range mismatch"));
            }
            if expected.encoding != ColumnarSegmentEncoding::NativeCborRows {
                return Err(LoomError::corrupt("unknown columnar segment encoding"));
            }
            if expected.digest != segment_digest(algo, &material.bytes) {
                return Err(LoomError::corrupt("columnar segment digest mismatch"));
            }
            let rows = decode_segment_rows(&material.bytes, ncols)?;
            if rows.len() != expected.row_count {
                return Err(LoomError::corrupt("columnar segment row count mismatch"));
            }
            if expected.statistics != segment_statistics(&rows, ncols) {
                return Err(LoomError::corrupt("columnar segment statistics mismatch"));
            }
            expected_row_start += expected.row_count;
            segments.push(rows);
        }
        ColumnarSet::from_segments(manifest.columns, segments, manifest.target_segment_rows)
    }

    /// Append a row (validating arity + column types), rolling to a fresh segment when the open one
    /// reaches the target size.
    pub fn append_row(&mut self, row: Vec<Value>) -> Result<()> {
        if row.len() != self.columns.len() {
            return Err(LoomError::invalid(format!(
                "row has {} values, dataset has {} columns",
                row.len(),
                self.columns.len()
            )));
        }
        for (v, (name, ty)) in row.iter().zip(&self.columns) {
            if !v.matches(*ty) {
                return Err(LoomError::invalid(format!(
                    "column {name:?} expects {ty:?}"
                )));
            }
        }
        let need_new = self
            .segments
            .last()
            .map(|s| s.len() >= self.target_segment_rows)
            .unwrap_or(true);
        if need_new {
            self.segments.push(Vec::new());
        }
        self.segments.last_mut().expect("segment present").push(row);
        Ok(())
    }

    /// All rows in append order (across segments) - the scan path.
    pub fn scan(&self) -> impl Iterator<Item = &Vec<Value>> {
        self.segments.iter().flatten()
    }

    /// The portable StateAccess query: project `columns` (by name, in the requested order) from the rows
    /// matching `filter` (`(column, op, value)`, or `None` for every row), as a single deterministic
    /// in-order scan. `INVALID_ARGUMENT` if a projected or filter column name is unknown. This is the
    /// `wasm32`-clean default; a native Polars-backed executor is gated separately (ADR-0008).
    pub fn select(
        &self,
        columns: &[&str],
        filter: Option<(&str, CmpOp, &Value)>,
    ) -> Result<Vec<Vec<Value>>> {
        let index_of = |name: &str| {
            self.columns
                .iter()
                .position(|(n, _)| n == name)
                .ok_or_else(|| LoomError::invalid(format!("unknown column {name:?}")))
        };
        let projection = columns
            .iter()
            .map(|c| index_of(c))
            .collect::<Result<Vec<usize>>>()?;
        let filter = match filter {
            Some((col, op, value)) => Some((index_of(col)?, op, value)),
            None => None,
        };
        let mut out = Vec::new();
        for row in self.scan() {
            if let Some((col, op, value)) = filter
                && !cmp_matches(op, &row[col], value)
            {
                continue;
            }
            out.push(projection.iter().map(|&i| row[i].clone()).collect());
        }
        Ok(out)
    }

    /// Evaluate aggregate expressions over rows matching `filter`.
    pub fn aggregate(
        &self,
        aggregates: &[ColumnarAggregate],
        filter: Option<(&str, CmpOp, &Value)>,
    ) -> Result<Vec<Value>> {
        let filter = match filter {
            Some((col, op, value)) => Some((self.index_of(col)?, op, value)),
            None => None,
        };
        let aggregate_indexes = aggregates
            .iter()
            .map(|aggregate| match aggregate.op {
                ColumnarAggregateOp::Count => Ok(None),
                _ => Ok(Some(self.index_of(
                    aggregate.column.as_deref().ok_or_else(|| {
                        LoomError::invalid("columnar aggregate column is required")
                    })?,
                )?)),
            })
            .collect::<Result<Vec<_>>>()?;
        let rows = self
            .scan()
            .filter(|row| filter.is_none_or(|(col, op, value)| cmp_matches(op, &row[col], value)))
            .collect::<Vec<_>>();
        aggregates
            .iter()
            .zip(aggregate_indexes)
            .map(|(aggregate, index)| evaluate_aggregate(aggregate.op, index, &rows))
            .collect()
    }

    /// Merge the segments into target-sized segments: flatten all rows in order and re-chunk at
    /// `target_segment_rows`. Segment boundaries are part of the promoted identity, so a changed
    /// layout changes canonical bytes.
    pub fn compact(&mut self) {
        let all: Vec<Vec<Value>> = self.segments.drain(..).flatten().collect();
        self.segments = all
            .chunks(self.target_segment_rows.max(1))
            .map(<[Vec<Value>]>::to_vec)
            .collect();
    }

    fn index_of(&self, name: &str) -> Result<usize> {
        self.columns
            .iter()
            .position(|(n, _)| n == name)
            .ok_or_else(|| LoomError::invalid(format!("unknown column {name:?}")))
    }

    /// Canonical bytes: a versioned Loom Canonical CBOR manifest with schema, target segment size,
    /// statistics policy, compression policy, and ordered segment records. Segment records include
    /// ordinal, row range, encoding, digest, encoded rows, and deterministic per-column statistics.
    pub fn encode(&self) -> Vec<u8> {
        self.encode_with_algo(Algo::Blake3)
    }

    /// Canonical bytes under an explicit identity-profile algorithm.
    pub fn encode_with_algo(&self, algo: Algo) -> Vec<u8> {
        use cbor::Value::{Array, Text, Uint};
        let columns = self
            .columns
            .iter()
            .map(|(name, ty)| Array(vec![Text(name.clone()), Uint(u64::from(ty.tag()))]))
            .collect();
        let segments = self
            .segments
            .iter()
            .enumerate()
            .scan(0usize, |row_start, (ordinal, rows)| {
                let segment_bytes = encode_segment_rows(rows);
                let statistics = segment_statistics(rows, self.columns.len());
                let segment = Array(vec![
                    Uint(ordinal as u64),
                    Uint(*row_start as u64),
                    Uint(rows.len() as u64),
                    Uint(COLUMNAR_SEGMENT_ENCODING_NATIVE_CBOR),
                    cbor::digest_value(&segment_digest(algo, &segment_bytes)),
                    cbor::Value::Bytes(segment_bytes),
                    encode_segment_statistics(&statistics),
                ]);
                *row_start += rows.len();
                Some(segment)
            })
            .collect();
        cbor::encode(&Array(vec![
            Uint(COLUMNAR_FORMAT_VERSION),
            Array(columns),
            Uint(self.target_segment_rows as u64),
            Uint(COLUMNAR_STATISTICS_BASIC),
            Uint(COLUMNAR_COMPRESSION_NONE),
            Array(segments),
        ]))
    }
    /// Parse a dataset from [`ColumnarSet::encode`] output, validating segment order, row ranges,
    /// digests, and deterministic statistics.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::decode_with_algo(bytes, Algo::Blake3)
    }

    /// Parse a dataset under an explicit identity-profile algorithm.
    pub fn decode_with_algo(bytes: &[u8], algo: Algo) -> Result<Self> {
        let mut f = cbor::Fields::new(cbor::decode_array(bytes)?);
        let version = f.uint()?;
        if version != COLUMNAR_FORMAT_VERSION {
            return Err(LoomError::corrupt(format!(
                "unknown columnar format version {version}"
            )));
        }
        let columns_raw = f.array()?;
        let target = usize::try_from(f.uint()?)
            .map_err(|_| LoomError::corrupt("columnar target out of range"))?;
        let statistics_policy = f.uint()?;
        if statistics_policy != COLUMNAR_STATISTICS_BASIC {
            return Err(LoomError::corrupt(format!(
                "unknown columnar statistics policy {statistics_policy}"
            )));
        }
        let compression_policy = f.uint()?;
        if compression_policy != COLUMNAR_COMPRESSION_NONE {
            return Err(LoomError::corrupt(format!(
                "unknown columnar compression policy {compression_policy}"
            )));
        }
        let segments_raw = f.array()?;
        f.end()?;

        let mut columns = Vec::with_capacity(columns_raw.len());
        for col in columns_raw {
            let mut cf = cbor::Fields::new(cbor::as_array(col)?);
            let name = cf.text()?;
            let tag = u8::try_from(cf.uint()?)
                .map_err(|_| LoomError::corrupt("column type tag out of range"))?;
            cf.end()?;
            columns.push((name, ColumnType::from_tag(tag)?));
        }
        let ncols = columns.len();
        let mut segments = Vec::with_capacity(segments_raw.len());
        let mut expected_row_start = 0usize;
        for (expected_ordinal, segment) in segments_raw.into_iter().enumerate() {
            let mut sf = cbor::Fields::new(cbor::as_array(segment)?);
            let ordinal = usize::try_from(sf.uint()?)
                .map_err(|_| LoomError::corrupt("columnar segment ordinal out of range"))?;
            if ordinal != expected_ordinal {
                return Err(LoomError::corrupt("columnar segment ordinal mismatch"));
            }
            let row_start = usize::try_from(sf.uint()?)
                .map_err(|_| LoomError::corrupt("columnar segment row start out of range"))?;
            if row_start != expected_row_start {
                return Err(LoomError::corrupt("columnar segment row range mismatch"));
            }
            let row_count = usize::try_from(sf.uint()?)
                .map_err(|_| LoomError::corrupt("columnar segment row count out of range"))?;
            let encoding = sf.uint()?;
            if encoding != COLUMNAR_SEGMENT_ENCODING_NATIVE_CBOR {
                return Err(LoomError::corrupt(format!(
                    "unknown columnar segment encoding {encoding}"
                )));
            }
            let digest = digest_from_value(sf.next_field()?, algo)?;
            let segment_bytes = sf.bytes()?;
            if digest != segment_digest(algo, &segment_bytes) {
                return Err(LoomError::corrupt("columnar segment digest mismatch"));
            }
            let statistics = decode_segment_statistics(sf.next_field()?, ncols)?;
            sf.end()?;
            let rows = decode_segment_rows(&segment_bytes, ncols)?;
            if rows.len() != row_count {
                return Err(LoomError::corrupt("columnar segment row count mismatch"));
            }
            if statistics != segment_statistics(&rows, ncols) {
                return Err(LoomError::corrupt("columnar segment statistics mismatch"));
            }
            expected_row_start += row_count;
            segments.push(rows);
        }
        ColumnarSet::from_segments(columns, segments, target)
    }

    fn from_segments(
        columns: Vec<(String, ColumnType)>,
        segments: Vec<Vec<Vec<Value>>>,
        target_segment_rows: usize,
    ) -> Result<Self> {
        if columns.is_empty() {
            return Err(LoomError::invalid("columnar dataset has no columns"));
        }
        let target = if target_segment_rows == 0 {
            DEFAULT_TARGET_SEGMENT_ROWS
        } else {
            target_segment_rows
        };
        let ncols = columns.len();
        for segment in &segments {
            for row in segment {
                if row.len() != ncols {
                    return Err(LoomError::corrupt("columnar row arity mismatch"));
                }
                for (v, (name, ty)) in row.iter().zip(&columns) {
                    if !v.matches(*ty) {
                        return Err(LoomError::corrupt(format!(
                            "column {name:?} expects {ty:?}"
                        )));
                    }
                }
            }
        }
        Ok(Self {
            columns,
            segments,
            target_segment_rows: target,
        })
    }
}

fn encode_segment_rows(rows: &[Vec<Value>]) -> Vec<u8> {
    cbor::encode(&cbor::Value::Array(
        rows.iter()
            .map(|row| cbor::Value::Array(row.iter().map(cell_value).collect()))
            .collect(),
    ))
}

fn decode_segment_rows(bytes: &[u8], ncols: usize) -> Result<Vec<Vec<Value>>> {
    cbor::decode_array(bytes)?
        .into_iter()
        .map(|r| {
            let items = cbor::as_array(r)?;
            if items.len() != ncols {
                return Err(LoomError::corrupt("columnar row arity mismatch"));
            }
            items.into_iter().map(cell_from).collect::<Result<Vec<_>>>()
        })
        .collect()
}

fn segment_digest(algo: Algo, segment_bytes: &[u8]) -> Digest {
    let mut bytes = Vec::with_capacity(COLUMNAR_SEGMENT_DIGEST_DOMAIN.len() + segment_bytes.len());
    bytes.extend_from_slice(COLUMNAR_SEGMENT_DIGEST_DOMAIN);
    bytes.extend_from_slice(segment_bytes);
    Digest::hash(algo, &bytes)
}

fn digest_from_value(value: cbor::Value, algo: Algo) -> Result<Digest> {
    let bytes = cbor::as_bytes(value)?;
    let arr: [u8; DIGEST_LEN] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| LoomError::corrupt("digest field is not 32 bytes"))?;
    Ok(Digest::of(algo, arr))
}

fn segment_statistics(rows: &[Vec<Value>], ncols: usize) -> ColumnarSegmentStatistics {
    let mut columns = (0..ncols)
        .map(|_| ColumnarColumnStatistics {
            null_count: 0,
            min: None,
            max: None,
        })
        .collect::<Vec<_>>();
    for row in rows {
        for (index, value) in row.iter().enumerate() {
            if matches!(value, Value::Null) {
                columns[index].null_count += 1;
                continue;
            }
            match &columns[index].min {
                Some(min) if value >= min => {}
                _ => columns[index].min = Some(value.clone()),
            }
            match &columns[index].max {
                Some(max) if value <= max => {}
                _ => columns[index].max = Some(value.clone()),
            }
        }
    }
    ColumnarSegmentStatistics { columns }
}

fn encode_segment_statistics(statistics: &ColumnarSegmentStatistics) -> cbor::Value {
    cbor::Value::Array(
        statistics
            .columns
            .iter()
            .map(|column| {
                cbor::Value::Array(vec![
                    cbor::Value::Uint(column.null_count as u64),
                    match &column.min {
                        Some(value) => cell_value(value),
                        None => cbor::Value::Null,
                    },
                    match &column.max {
                        Some(value) => cell_value(value),
                        None => cbor::Value::Null,
                    },
                ])
            })
            .collect(),
    )
}

fn decode_segment_statistics(
    value: cbor::Value,
    ncols: usize,
) -> Result<ColumnarSegmentStatistics> {
    let raw = cbor::as_array(value)?;
    if raw.len() != ncols {
        return Err(LoomError::corrupt(
            "columnar segment statistics arity mismatch",
        ));
    }
    let columns = raw
        .into_iter()
        .map(|entry| {
            let mut f = cbor::Fields::new(cbor::as_array(entry)?);
            let null_count = usize::try_from(f.uint()?)
                .map_err(|_| LoomError::corrupt("columnar null count out of range"))?;
            let min = match f.next_field()? {
                cbor::Value::Null => None,
                value => Some(cell_from(value)?),
            };
            let max = match f.next_field()? {
                cbor::Value::Null => None,
                value => Some(cell_from(value)?),
            };
            f.end()?;
            Ok(ColumnarColumnStatistics {
                null_count,
                min,
                max,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(ColumnarSegmentStatistics { columns })
}

fn cmp_matches(op: CmpOp, lhs: &Value, rhs: &Value) -> bool {
    let ord = lhs.cmp(rhs);
    match op {
        CmpOp::Eq => ord == Ordering::Equal,
        CmpOp::Ne => ord != Ordering::Equal,
        CmpOp::Lt => ord == Ordering::Less,
        CmpOp::Le => ord != Ordering::Greater,
        CmpOp::Gt => ord == Ordering::Greater,
        CmpOp::Ge => ord != Ordering::Less,
    }
}

fn evaluate_aggregate(
    op: ColumnarAggregateOp,
    index: Option<usize>,
    rows: &[&Vec<Value>],
) -> Result<Value> {
    match op {
        ColumnarAggregateOp::Count => Ok(Value::U64(rows.len() as u64)),
        ColumnarAggregateOp::CountNonNull => {
            let index =
                index.ok_or_else(|| LoomError::invalid("columnar aggregate column is required"))?;
            Ok(Value::U64(
                rows.iter()
                    .filter(|row| !matches!(row[index], Value::Null))
                    .count() as u64,
            ))
        }
        ColumnarAggregateOp::Min => {
            let index =
                index.ok_or_else(|| LoomError::invalid("columnar aggregate column is required"))?;
            Ok(rows
                .iter()
                .map(|row| &row[index])
                .filter(|value| !matches!(value, Value::Null))
                .min()
                .cloned()
                .unwrap_or(Value::Null))
        }
        ColumnarAggregateOp::Max => {
            let index =
                index.ok_or_else(|| LoomError::invalid("columnar aggregate column is required"))?;
            Ok(rows
                .iter()
                .map(|row| &row[index])
                .filter(|value| !matches!(value, Value::Null))
                .max()
                .cloned()
                .unwrap_or(Value::Null))
        }
        ColumnarAggregateOp::Sum => {
            let index =
                index.ok_or_else(|| LoomError::invalid("columnar aggregate column is required"))?;
            sum_values(rows.iter().map(|row| &row[index]))
        }
    }
}

fn sum_values<'a>(values: impl Iterator<Item = &'a Value>) -> Result<Value> {
    let mut sum: Option<Value> = None;
    for value in values.filter(|value| !matches!(value, Value::Null)) {
        sum = Some(match (sum.take(), value) {
            (None, value) => numeric_zero_like(value)?,
            (Some(acc), _) => acc,
        });
        if let Some(acc) = sum.take() {
            sum = Some(add_numeric(acc, value)?);
        }
    }
    Ok(sum.unwrap_or(Value::Null))
}

fn numeric_zero_like(value: &Value) -> Result<Value> {
    match value {
        Value::Int(_) => Ok(Value::Int(0)),
        Value::Float(_) => Ok(Value::Float(0.0)),
        Value::I8(_) => Ok(Value::I8(0)),
        Value::I16(_) => Ok(Value::I16(0)),
        Value::I32(_) => Ok(Value::I32(0)),
        Value::I128(_) => Ok(Value::I128(0)),
        Value::U8(_) => Ok(Value::U8(0)),
        Value::U16(_) => Ok(Value::U16(0)),
        Value::U32(_) => Ok(Value::U32(0)),
        Value::U64(_) => Ok(Value::U64(0)),
        Value::U128(_) => Ok(Value::U128(0)),
        Value::F32(_) => Ok(Value::F32(0.0)),
        _ => Err(LoomError::invalid("columnar sum requires a numeric column")),
    }
}

fn add_numeric(acc: Value, value: &Value) -> Result<Value> {
    match (acc, value) {
        (Value::Int(a), Value::Int(b)) => a
            .checked_add(*b)
            .map(Value::Int)
            .ok_or_else(|| LoomError::invalid("columnar sum overflow")),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
        (Value::I8(a), Value::I8(b)) => a
            .checked_add(*b)
            .map(Value::I8)
            .ok_or_else(|| LoomError::invalid("columnar sum overflow")),
        (Value::I16(a), Value::I16(b)) => a
            .checked_add(*b)
            .map(Value::I16)
            .ok_or_else(|| LoomError::invalid("columnar sum overflow")),
        (Value::I32(a), Value::I32(b)) => a
            .checked_add(*b)
            .map(Value::I32)
            .ok_or_else(|| LoomError::invalid("columnar sum overflow")),
        (Value::I128(a), Value::I128(b)) => a
            .checked_add(*b)
            .map(Value::I128)
            .ok_or_else(|| LoomError::invalid("columnar sum overflow")),
        (Value::U8(a), Value::U8(b)) => a
            .checked_add(*b)
            .map(Value::U8)
            .ok_or_else(|| LoomError::invalid("columnar sum overflow")),
        (Value::U16(a), Value::U16(b)) => a
            .checked_add(*b)
            .map(Value::U16)
            .ok_or_else(|| LoomError::invalid("columnar sum overflow")),
        (Value::U32(a), Value::U32(b)) => a
            .checked_add(*b)
            .map(Value::U32)
            .ok_or_else(|| LoomError::invalid("columnar sum overflow")),
        (Value::U64(a), Value::U64(b)) => a
            .checked_add(*b)
            .map(Value::U64)
            .ok_or_else(|| LoomError::invalid("columnar sum overflow")),
        (Value::U128(a), Value::U128(b)) => a
            .checked_add(*b)
            .map(Value::U128)
            .ok_or_else(|| LoomError::invalid("columnar sum overflow")),
        (Value::F32(a), Value::F32(b)) => Ok(Value::F32(a + b)),
        _ => Err(LoomError::invalid("columnar sum type mismatch")),
    }
}

/// A derived, native columnar query executor (for example a Polars-backed engine in a gated `loom-polars`
/// crate). It MUST reconcile to the portable [`ColumnarSet::select`] result: the same projected rows in
/// the same order, so switching it on never changes results, only speed. Defined here so the engine
/// stays `wasm32`-clean (no heavy dependency); the native implementation lives behind a feature gate in a
/// separate crate and is injected at the call site, mirroring the vector accelerator injection pattern.
pub trait ColumnarExecutor {
    /// Project `columns` from `set`'s rows matching `filter`, reconciled to [`ColumnarSet::select`].
    fn select(
        &self,
        set: &ColumnarSet,
        columns: &[&str],
        filter: Option<(&str, CmpOp, &Value)>,
    ) -> Result<Vec<Vec<Value>>>;

    /// Evaluate aggregates over `set`, reconciled to [`ColumnarSet::aggregate`].
    fn aggregate(
        &self,
        set: &ColumnarSet,
        aggregates: &[ColumnarAggregate],
        filter: Option<(&str, CmpOp, &Value)>,
    ) -> Result<Vec<Value>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols() -> Vec<(String, ColumnType)> {
        vec![
            ("id".into(), ColumnType::Int),
            ("label".into(), ColumnType::Text),
        ]
    }

    fn dataset() -> ColumnarSet {
        let mut set = ColumnarSet::new(cols(), 2).unwrap();
        set.append_row(vec![Value::Int(1), Value::Text("a".into())])
            .unwrap();
        set.append_row(vec![Value::Int(2), Value::Text("b".into())])
            .unwrap();
        set.append_row(vec![Value::Int(3), Value::Text("c".into())])
            .unwrap();
        set
    }

    #[test]
    fn append_rolls_segments_at_target_and_compacts() {
        let mut set = ColumnarSet::new(cols(), 2).unwrap();
        for i in 0..5 {
            set.append_row(vec![Value::Int(i), Value::Text(format!("n{i}"))])
                .unwrap();
        }
        assert_eq!(set.rows(), 5);
        assert_eq!(set.segment_count(), 3);
        assert_eq!(
            set.scan().map(|row| row[0].clone()).collect::<Vec<_>>(),
            vec![
                Value::Int(0),
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Int(4)
            ]
        );
        set.compact();
        assert_eq!(set.segment_count(), 3);
        assert_eq!(set.rows(), 5);
    }

    #[test]
    fn manifest_records_ordered_segments_and_statistics() {
        let mut set = ColumnarSet::new(cols(), 2).unwrap();
        set.append_row(vec![Value::Int(1), Value::Text("b".into())])
            .unwrap();
        set.append_row(vec![Value::Int(3), Value::Null]).unwrap();
        set.append_row(vec![Value::Int(2), Value::Text("a".into())])
            .unwrap();

        let manifest = set.manifest();
        assert_eq!(manifest.version, COLUMNAR_FORMAT_VERSION);
        assert_eq!(manifest.statistics_policy, ColumnarStatisticsPolicy::Basic);
        assert_eq!(manifest.compression_policy, ColumnarCompressionPolicy::None);
        assert_eq!(manifest.segments.len(), 2);
        assert_eq!(manifest.segments[0].ordinal, 0);
        assert_eq!(manifest.segments[0].row_start, 0);
        assert_eq!(manifest.segments[0].row_count, 2);
        assert_eq!(manifest.segments[1].ordinal, 1);
        assert_eq!(manifest.segments[1].row_start, 2);
        assert_eq!(manifest.segments[1].row_count, 1);
        assert_eq!(
            manifest.segments[0].statistics.columns[0].min,
            Some(Value::Int(1))
        );
        assert_eq!(
            manifest.segments[0].statistics.columns[0].max,
            Some(Value::Int(3))
        );
        assert_eq!(manifest.segments[0].statistics.columns[1].null_count, 1);
    }

    #[test]
    fn segment_layout_is_part_of_canonical_identity() {
        let columns = cols();
        let rows = vec![
            vec![Value::Int(1), Value::Text("a".into())],
            vec![Value::Int(2), Value::Text("b".into())],
            vec![Value::Int(3), Value::Text("c".into())],
        ];
        let mut fragmented = ColumnarSet::from_segments(
            columns,
            rows.iter().cloned().map(|row| vec![row]).collect(),
            2,
        )
        .unwrap();
        let before = fragmented.encode();
        fragmented.compact();
        assert_eq!(fragmented.segment_count(), 2);
        assert_ne!(fragmented.encode(), before);
        assert_eq!(fragmented.scan().cloned().collect::<Vec<_>>(), rows);
    }

    #[test]
    fn type_and_arity_validation() {
        let mut set = ColumnarSet::new(cols(), 0).unwrap();
        assert!(set.append_row(vec![Value::Int(1)]).is_err());
        assert!(
            set.append_row(vec![Value::Text("x".into()), Value::Text("y".into())])
                .is_err()
        );
        assert!(set.append_row(vec![Value::Int(1), Value::Null]).is_ok());
        assert!(ColumnarSet::new(vec![], 0).is_err());
    }

    #[test]
    fn encode_decode_preserves_manifest_and_rows() {
        let set = dataset();
        let decoded = ColumnarSet::decode(&set.encode()).unwrap();
        assert_eq!(decoded.columns(), set.columns());
        assert_eq!(
            decoded.scan().cloned().collect::<Vec<_>>(),
            set.scan().cloned().collect::<Vec<_>>()
        );
        assert_eq!(decoded.manifest(), set.manifest());
    }

    #[test]
    fn select_and_aggregate_use_portable_semantics() {
        let set = dataset();
        let selected = set
            .select(&["label"], Some(("id", CmpOp::Gt, &Value::Int(1))))
            .unwrap();
        assert_eq!(
            selected,
            vec![vec![Value::Text("b".into())], vec![Value::Text("c".into())],]
        );
        let aggregate = set
            .aggregate(
                &[ColumnarAggregate {
                    op: ColumnarAggregateOp::Sum,
                    column: Some("id".into()),
                }],
                None,
            )
            .unwrap();
        assert_eq!(aggregate, vec![Value::Int(6)]);
    }

    #[test]
    fn decode_rejects_segment_digest_tampering() {
        let mut raw = cbor::decode(&dataset().encode()).unwrap();
        let cbor::Value::Array(top) = &mut raw else {
            panic!("columnar top-level shape");
        };
        let cbor::Value::Array(segments) = &mut top[5] else {
            panic!("columnar segment list shape");
        };
        let cbor::Value::Array(first) = &mut segments[0] else {
            panic!("columnar segment shape");
        };
        first[4] = cbor::Value::Bytes(vec![0; DIGEST_LEN]);
        let bytes = cbor::encode(&raw);
        assert_eq!(
            ColumnarSet::decode(&bytes).unwrap_err().code,
            loom_types::Code::CorruptObject
        );
    }
}
