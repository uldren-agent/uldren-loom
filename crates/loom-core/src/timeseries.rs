//! The time-series facet - a versioned series of points keyed by a signed 64-bit timestamp, valued by
//! opaque bytes. Pure-Rust, `wasm32`-clean, deterministic. Each series is staged as a structured
//! time-series root (metadata, a reachable point-field prolly root, and rollup roots) and versions,
//! branches, and syncs through the engine.
//!
//! Query visibility, raw pruning, and materialized rollups are collection policy and derived state.

use crate::acl::AclRight;
use crate::cbor::{self, Value};
use crate::error::{Code, Result};
use crate::provider::ObjectStore;
use crate::vcs::Loom;
use crate::workspace::{FacetKind, WorkspaceId, facet_path, facet_root};
use std::collections::BTreeMap;

type PointIdentity = (String, Vec<(String, String)>, i64);
type RollupIdentity = (String, Vec<(String, String)>, i64, String);

#[derive(Debug, Clone, PartialEq)]
pub enum TimeSeriesValue {
    Int(i64),
    Float(f64),
    Text(String),
    Bool(bool),
    Bytes(Vec<u8>),
}

impl TimeSeriesValue {
    fn encode(&self) -> Value {
        match self {
            Self::Int(value) => Value::int(*value),
            Self::Float(value) => Value::Float(*value),
            Self::Text(value) => Value::Text(value.clone()),
            Self::Bool(value) => Value::Bool(*value),
            Self::Bytes(value) => Value::Bytes(value.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructuredPoint {
    pub measurement: String,
    pub tags: BTreeMap<String, String>,
    pub timestamp_ns: i64,
    pub fields: BTreeMap<String, TimeSeriesValue>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimeSeriesPolicy {
    pub query_start_ns: Option<i64>,
    pub rollups: Vec<TimeSeriesRollup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeSeriesRollup {
    pub name: String,
    pub resolution_ns: i64,
    pub aggregation: TimeSeriesAggregation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeSeriesAggregation {
    Count,
    Sum,
    Min,
    Max,
    Mean,
}

impl TimeSeriesRollup {
    pub fn new(
        name: impl Into<String>,
        resolution_ns: i64,
        aggregation: TimeSeriesAggregation,
    ) -> Result<Self> {
        let rollup = Self {
            name: name.into(),
            resolution_ns,
            aggregation,
        };
        rollup.validate()?;
        Ok(rollup)
    }

    fn validate(&self) -> Result<()> {
        if self.name.is_empty()
            || self.name.contains('/')
            || self.name == "metadata"
            || self.name == "points"
            || self.name == "rollups"
            || self.resolution_ns <= 0
        {
            return Err(crate::LoomError::invalid(
                "time-series rollup name and resolution must be valid",
            ));
        }
        Ok(())
    }
}

impl TimeSeriesAggregation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Sum => "sum",
            Self::Min => "min",
            Self::Max => "max",
            Self::Mean => "mean",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "count" => Ok(Self::Count),
            "sum" => Ok(Self::Sum),
            "min" => Ok(Self::Min),
            "max" => Ok(Self::Max),
            "mean" => Ok(Self::Mean),
            _ => Err(crate::LoomError::corrupt(
                "unsupported time-series rollup aggregation",
            )),
        }
    }
}

fn validate_policy(policy: &TimeSeriesPolicy) -> Result<()> {
    let mut names = BTreeMap::new();
    for rollup in &policy.rollups {
        rollup.validate()?;
        if names.insert(rollup.name.clone(), ()).is_some() {
            return Err(crate::LoomError::invalid(
                "time-series rollup names must be unique",
            ));
        }
    }
    Ok(())
}

impl StructuredPoint {
    pub fn new(
        measurement: impl Into<String>,
        tags: BTreeMap<String, String>,
        timestamp_ns: i64,
        fields: BTreeMap<String, TimeSeriesValue>,
    ) -> Result<Self> {
        let point = Self {
            measurement: measurement.into(),
            tags,
            timestamp_ns,
            fields,
        };
        point.validate()?;
        Ok(point)
    }

    fn validate(&self) -> Result<()> {
        if self.measurement.is_empty() || self.fields.is_empty() {
            return Err(crate::LoomError::invalid(
                "time-series measurement and fields must be non-empty",
            ));
        }
        if self.tags.keys().any(|key| key.is_empty())
            || self.tags.values().any(String::is_empty)
            || self.fields.keys().any(|key| key.is_empty())
            || self
                .fields
                .values()
                .any(|value| matches!(value, TimeSeriesValue::Float(value) if !value.is_finite()))
        {
            return Err(crate::LoomError::invalid(
                "time-series tag and field names and values must be non-empty",
            ));
        }
        Ok(())
    }
}

/// A versioned time series: points keyed by `i64` timestamp, in time order. A repeated timestamp
/// replaces the point at that instant (last write at that key wins *within* one writer).
#[derive(Debug, Clone, Default)]
pub struct Series {
    points: BTreeMap<i64, Vec<u8>>,
}

impl Series {
    /// An empty series.
    pub fn new() -> Self {
        Self::default()
    }
    /// Number of points.
    pub fn len(&self) -> usize {
        self.points.len()
    }
    /// Whether the series has no points.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
    /// Record `value` at `ts` (replaces any existing point at that timestamp).
    pub fn put(&mut self, ts: i64, value: Vec<u8>) {
        self.points.insert(ts, value);
    }
    /// The point at `ts`, if any.
    pub fn get(&self, ts: i64) -> Option<&[u8]> {
        self.points.get(&ts).map(Vec::as_slice)
    }
    /// Points with `from <= ts < to`, in time order (half-open range query).
    pub fn range(&self, from: i64, to: i64) -> Vec<(i64, &[u8])> {
        self.points
            .range(from..to)
            .map(|(t, v)| (*t, v.as_slice()))
            .collect()
    }
    /// The most recent point (largest timestamp), if any.
    pub fn latest(&self) -> Option<(i64, &[u8])> {
        self.points
            .iter()
            .next_back()
            .map(|(t, v)| (*t, v.as_slice()))
    }
    /// All points in time order.
    pub fn iter(&self) -> impl Iterator<Item = (i64, &[u8])> {
        self.points.iter().map(|(t, v)| (*t, v.as_slice()))
    }

    /// Canonical bytes: points in timestamp order. Deterministic.
    pub fn encode(&self) -> Vec<u8> {
        let items = self
            .points
            .iter()
            .map(|(ts, v)| Value::Array(vec![Value::int(*ts), Value::Bytes(v.clone())]))
            .collect();
        cbor::encode(&Value::Array(items))
    }
    /// Parse a series from [`Series::encode`] output.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut s = Series::new();
        for item in cbor::decode_array(bytes)? {
            let mut f = cbor::Fields::new(cbor::as_array(item)?);
            let ts = f.int()?;
            let v = f.bytes()?;
            f.end()?;
            s.points.insert(ts, v);
        }
        Ok(s)
    }
}

/// Encode a single time-series point as the canonical `[ts, value]` CBOR pair — the same element shape
/// [`Series::encode`] uses. This is the wire payload for the `TimeSeries.latest` API method (wrapped in
/// the method's `optional`), so a remote client recovers BOTH the timestamp and the value rather than the
/// value alone. Absence is carried by the surrounding `Option`, not by this encoding.
pub fn latest_point_to_cbor(ts: i64, value: &[u8]) -> Vec<u8> {
    cbor::encode(&Value::Array(vec![
        Value::int(ts),
        Value::Bytes(value.to_vec()),
    ]))
}

/// Decode a `[ts, value]` pair produced by [`latest_point_to_cbor`] back into `(ts, value)`.
pub fn latest_point_from_cbor(bytes: &[u8]) -> Result<(i64, Vec<u8>)> {
    let mut f = cbor::Fields::new(cbor::decode_array(bytes)?);
    let ts = f.int()?;
    let value = f.bytes()?;
    f.end()?;
    Ok((ts, value))
}

fn series_path(collection: &str) -> String {
    facet_path(FacetKind::TimeSeries, collection)
}

const LEGACY_MEASUREMENT: &str = "_loom_legacy";
const LEGACY_FIELD: &str = "value";

fn legacy_point(timestamp_ns: i64, value: Vec<u8>) -> StructuredPoint {
    StructuredPoint::new(
        LEGACY_MEASUREMENT,
        BTreeMap::new(),
        timestamp_ns,
        BTreeMap::from([(LEGACY_FIELD.to_string(), TimeSeriesValue::Bytes(value))]),
    )
    .expect("fixed legacy time-series point is valid")
}

fn structured_metadata(policy: &TimeSeriesPolicy) -> Vec<u8> {
    let rollups = policy
        .rollups
        .iter()
        .map(|rollup| {
            Value::Array(vec![
                Value::Text(rollup.name.clone()),
                Value::int(rollup.resolution_ns),
                Value::Text(rollup.aggregation.as_str().to_string()),
            ])
        })
        .collect();
    cbor::encode(&Value::Array(vec![
        Value::Uint(2),
        policy.query_start_ns.map(Value::int).unwrap_or(Value::Null),
        Value::Array(rollups),
    ]))
}

fn decode_structured_metadata(bytes: &[u8]) -> Result<TimeSeriesPolicy> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let version = fields.uint()?;
    let query_start_ns = match fields.next_field()? {
        Value::Null => None,
        value => Some(cbor::as_int(value)?),
    };
    let rollups = match version {
        2 => fields
            .array()?
            .into_iter()
            .map(|item| {
                let mut fields = cbor::Fields::new(cbor::as_array(item)?);
                let rollup = TimeSeriesRollup::new(
                    fields.text()?,
                    fields.int()?,
                    TimeSeriesAggregation::parse(&fields.text()?)?,
                )?;
                fields.end()?;
                Ok(rollup)
            })
            .collect::<Result<Vec<_>>>()?,
        _ => {
            return Err(crate::LoomError::corrupt(
                "unsupported time-series metadata version",
            ));
        }
    };
    fields.end()?;
    Ok(TimeSeriesPolicy {
        query_start_ns,
        rollups,
    })
}

pub fn ts_policy<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<TimeSeriesPolicy> {
    loom.authorize_collection(ns, FacetKind::TimeSeries, collection, AclRight::Read)?;
    match loom.time_series_parts_reserved(ns, &series_path(collection)) {
        Ok((metadata, _, _)) => decode_structured_metadata(&loom.load_content(metadata)?),
        Err(error) if error.code == Code::NotFound => Ok(TimeSeriesPolicy::default()),
        Err(error) => Err(error),
    }
}

pub fn ts_set_policy<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    policy: TimeSeriesPolicy,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::TimeSeries, collection, AclRight::Write)?;
    validate_policy(&policy)?;
    let path = series_path(collection);
    let (root, mut rollups) = match loom.time_series_parts_reserved(ns, &path) {
        Ok((_, root, rollups)) => (root, rollups),
        Err(error) if error.code == Code::NotFound => (None, BTreeMap::new()),
        Err(error) => return Err(error),
    };
    let declared: BTreeMap<String, ()> = policy
        .rollups
        .iter()
        .map(|rollup| (rollup.name.clone(), ()))
        .collect();
    rollups.retain(|name, _| declared.contains_key(name));
    loom.create_directory_reserved(ns, &facet_root(FacetKind::TimeSeries), true)?;
    loom.stage_time_series_reserved(ns, &path, &structured_metadata(&policy), root, rollups)
}

fn structured_point_key(point: &StructuredPoint, field: &str) -> Vec<u8> {
    let tags = point
        .tags
        .iter()
        .map(|(name, value)| {
            Value::Array(vec![Value::Text(name.clone()), Value::Text(value.clone())])
        })
        .collect();
    cbor::encode(&Value::Array(vec![
        Value::Text(point.measurement.clone()),
        Value::Array(tags),
        Value::int(point.timestamp_ns),
        Value::Text(field.to_string()),
    ]))
}

fn decode_structured_key(bytes: &[u8]) -> Result<(String, BTreeMap<String, String>, i64, String)> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let measurement = fields.text()?;
    let mut tags = BTreeMap::new();
    for item in fields.array()? {
        let mut tag = cbor::Fields::new(cbor::as_array(item)?);
        let name = tag.text()?;
        let value = tag.text()?;
        tag.end()?;
        if name.is_empty() || value.is_empty() || tags.insert(name, value).is_some() {
            return Err(crate::LoomError::corrupt("invalid time-series tag set"));
        }
    }
    let timestamp_ns = fields.int()?;
    let field = fields.text()?;
    fields.end()?;
    if measurement.is_empty() || field.is_empty() {
        return Err(crate::LoomError::corrupt(
            "time-series measurement and field must be non-empty",
        ));
    }
    Ok((measurement, tags, timestamp_ns, field))
}

fn decode_structured_value(bytes: &[u8]) -> Result<TimeSeriesValue> {
    match cbor::decode(bytes)? {
        Value::Uint(value) => i64::try_from(value)
            .map(TimeSeriesValue::Int)
            .map_err(|_| crate::LoomError::corrupt("time-series integer exceeds i64")),
        Value::Nint(value) => i64::try_from(value)
            .map(|value| TimeSeriesValue::Int(-1 - value))
            .map_err(|_| crate::LoomError::corrupt("time-series integer exceeds i64")),
        Value::Float(value) if value.is_finite() => Ok(TimeSeriesValue::Float(value)),
        Value::Text(value) => Ok(TimeSeriesValue::Text(value)),
        Value::Bool(value) => Ok(TimeSeriesValue::Bool(value)),
        Value::Bytes(value) => Ok(TimeSeriesValue::Bytes(value)),
        _ => Err(crate::LoomError::corrupt("invalid time-series field value")),
    }
}

pub fn ts_put_point<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    point: StructuredPoint,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::TimeSeries, collection, AclRight::Write)?;
    point.validate()?;
    let path = series_path(collection);
    let (metadata, root, rollups) = match loom.time_series_parts_reserved(ns, &path) {
        Ok((metadata, root, rollups)) => (loom.load_content(metadata)?, root, rollups),
        Err(error) if error.code == Code::NotFound => (
            structured_metadata(&TimeSeriesPolicy::default()),
            None,
            BTreeMap::new(),
        ),
        Err(error) => return Err(error),
    };
    let mut next = root;
    for (field, value) in &point.fields {
        next = Some(crate::prolly::insert(
            loom.store_mut(),
            next.as_ref(),
            &structured_point_key(&point, field),
            &cbor::encode(&value.encode()),
        )?);
    }
    loom.create_directory_reserved(ns, &facet_root(FacetKind::TimeSeries), true)?;
    loom.stage_time_series_reserved(ns, &path, &metadata, next, rollups)
}

pub fn ts_range_points<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    from_ns: i64,
    to_ns: i64,
) -> Result<Vec<StructuredPoint>> {
    loom.authorize_collection(ns, FacetKind::TimeSeries, collection, AclRight::Read)?;
    let path = series_path(collection);
    let parts = match loom.time_series_parts_reserved(ns, &path) {
        Ok(parts) => parts,
        Err(error) if error.code == Code::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let policy = decode_structured_metadata(&loom.load_content(parts.0)?)?;
    let from_ns = policy
        .query_start_ns
        .map_or(from_ns, |start| from_ns.max(start));
    let Some(root) = parts.1 else {
        return Ok(Vec::new());
    };
    range_points_from_root(loom, &root, from_ns, to_ns)
}

pub fn ts_range_rollup_points<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    rollup: &str,
    from_ns: i64,
    to_ns: i64,
) -> Result<Vec<StructuredPoint>> {
    loom.authorize_collection(ns, FacetKind::TimeSeries, collection, AclRight::Read)?;
    let path = series_path(collection);
    let (_, _, rollups) = match loom.time_series_parts_reserved(ns, &path) {
        Ok(parts) => parts,
        Err(error) if error.code == Code::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let Some(root) = rollups.get(rollup) else {
        return Ok(Vec::new());
    };
    range_points_from_root(loom, root, from_ns, to_ns)
}

pub fn ts_materialize_rollup<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    rollup_name: &str,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::TimeSeries, collection, AclRight::Write)?;
    let path = series_path(collection);
    let (metadata, root, mut rollups) = loom.time_series_parts_reserved(ns, &path)?;
    let metadata_bytes = loom.load_content(metadata)?;
    let policy = decode_structured_metadata(&metadata_bytes)?;
    let rollup = policy
        .rollups
        .iter()
        .find(|rollup| rollup.name == rollup_name)
        .ok_or_else(|| crate::LoomError::not_found("time-series rollup is not declared"))?
        .clone();
    let Some(root) = root else {
        rollups.remove(rollup_name);
        return loom.stage_time_series_reserved(ns, &path, &metadata_bytes, None, rollups);
    };
    let raw_points = range_points_from_root(loom, &root, i64::MIN, i64::MAX)?;
    let entries = rollup_entries(&rollup, raw_points)?;
    if let Some(root) = crate::prolly::build(loom.store_mut(), &entries)? {
        rollups.insert(rollup_name.to_string(), root);
    } else {
        rollups.remove(rollup_name);
    }
    loom.stage_time_series_reserved(ns, &path, &metadata_bytes, Some(root), rollups)
}

pub fn ts_prune_before<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    cutoff_ns: i64,
) -> Result<usize> {
    loom.authorize_collection(ns, FacetKind::TimeSeries, collection, AclRight::Write)?;
    let path = series_path(collection);
    let (metadata, root, rollups) = match loom.time_series_parts_reserved(ns, &path) {
        Ok(parts) => parts,
        Err(error) if error.code == Code::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let Some(mut root) = root else {
        return Ok(0);
    };
    let mut removed = 0usize;
    for (key, _) in crate::prolly::entries(loom.store(), &root)? {
        let (_, _, timestamp_ns, _) = decode_structured_key(&key)?;
        if timestamp_ns < cutoff_ns {
            match crate::prolly::remove(loom.store_mut(), &root, &key)? {
                Some(next) => root = next,
                None => {
                    removed += 1;
                    let metadata = loom.load_content(metadata)?;
                    loom.stage_time_series_reserved(ns, &path, &metadata, None, rollups)?;
                    return Ok(removed);
                }
            }
            removed += 1;
        }
    }
    if removed > 0 {
        let metadata = loom.load_content(metadata)?;
        loom.stage_time_series_reserved(ns, &path, &metadata, Some(root), rollups)?;
    }
    Ok(removed)
}

fn range_points_from_root<S: ObjectStore>(
    loom: &Loom<S>,
    root: &crate::digest::Digest,
    from_ns: i64,
    to_ns: i64,
) -> Result<Vec<StructuredPoint>> {
    let mut points: BTreeMap<PointIdentity, BTreeMap<String, TimeSeriesValue>> = BTreeMap::new();
    for (key, value) in crate::prolly::entries(loom.store(), root)? {
        let (measurement, tags, timestamp_ns, field) = decode_structured_key(&key)?;
        if !(from_ns..to_ns).contains(&timestamp_ns) {
            continue;
        }
        let tag_key = tags.into_iter().collect::<Vec<_>>();
        points
            .entry((measurement, tag_key, timestamp_ns))
            .or_default()
            .insert(field, decode_structured_value(&value)?);
    }
    points
        .into_iter()
        .map(|((measurement, tags, timestamp_ns), fields)| {
            StructuredPoint::new(
                measurement,
                tags.into_iter().collect(),
                timestamp_ns,
                fields,
            )
        })
        .collect()
}

#[derive(Debug, Clone)]
struct RollupAccumulator {
    aggregation: TimeSeriesAggregation,
    count: u64,
    sum: f64,
    min: Option<f64>,
    max: Option<f64>,
}

impl RollupAccumulator {
    fn new(aggregation: TimeSeriesAggregation) -> Self {
        Self {
            aggregation,
            count: 0,
            sum: 0.0,
            min: None,
            max: None,
        }
    }

    fn push(&mut self, value: &TimeSeriesValue) {
        if self.aggregation == TimeSeriesAggregation::Count {
            self.count += 1;
            return;
        }
        let value = match value {
            TimeSeriesValue::Int(value) => *value as f64,
            TimeSeriesValue::Float(value) => *value,
            TimeSeriesValue::Text(_) | TimeSeriesValue::Bool(_) | TimeSeriesValue::Bytes(_) => {
                return;
            }
        };
        self.count += 1;
        self.sum += value;
        self.min = Some(self.min.map_or(value, |current| current.min(value)));
        self.max = Some(self.max.map_or(value, |current| current.max(value)));
    }

    fn finish(&self) -> Option<TimeSeriesValue> {
        match self.aggregation {
            TimeSeriesAggregation::Count => {
                Some(TimeSeriesValue::Int(i64::try_from(self.count).ok()?))
            }
            TimeSeriesAggregation::Sum if self.count > 0 => Some(TimeSeriesValue::Float(self.sum)),
            TimeSeriesAggregation::Min => self.min.map(TimeSeriesValue::Float),
            TimeSeriesAggregation::Max => self.max.map(TimeSeriesValue::Float),
            TimeSeriesAggregation::Mean if self.count > 0 => {
                Some(TimeSeriesValue::Float(self.sum / self.count as f64))
            }
            TimeSeriesAggregation::Sum | TimeSeriesAggregation::Mean => None,
        }
    }
}

fn rollup_entries(
    rollup: &TimeSeriesRollup,
    raw_points: Vec<StructuredPoint>,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let mut acc: BTreeMap<RollupIdentity, RollupAccumulator> = BTreeMap::new();
    for point in raw_points {
        let bucket = point
            .timestamp_ns
            .div_euclid(rollup.resolution_ns)
            .checked_mul(rollup.resolution_ns)
            .ok_or_else(|| crate::LoomError::invalid("time-series rollup bucket overflow"))?;
        let tags = point.tags.into_iter().collect::<Vec<_>>();
        for (field, value) in point.fields {
            acc.entry((point.measurement.clone(), tags.clone(), bucket, field))
                .or_insert_with(|| RollupAccumulator::new(rollup.aggregation))
                .push(&value);
        }
    }
    let mut entries = Vec::new();
    for ((measurement, tags, timestamp_ns, field), acc) in acc {
        let Some(value) = acc.finish() else {
            continue;
        };
        let point = StructuredPoint::new(
            measurement,
            tags.into_iter().collect(),
            timestamp_ns,
            BTreeMap::from([(field.clone(), value.clone())]),
        )?;
        entries.push((
            structured_point_key(&point, &field),
            cbor::encode(&value.encode()),
        ));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(entries)
}

/// Stage `series` under `collection` in `ns` through the structured point root.
pub fn put_series<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    series: &Series,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::TimeSeries, collection, AclRight::Write)?;
    for (timestamp_ns, value) in series.iter() {
        ts_put_point(
            loom,
            ns,
            collection,
            legacy_point(timestamp_ns, value.to_vec()),
        )?;
    }
    Ok(())
}

/// Load the series named `collection` from `ns`'s current working tree, or `NOT_FOUND`.
pub fn get_series<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<Series> {
    loom.authorize_collection(ns, FacetKind::TimeSeries, collection, AclRight::Read)?;
    match ts_range_points(loom, ns, collection, i64::MIN, i64::MAX) {
        Ok(points) => {
            let mut series = Series::new();
            for point in points {
                if point.measurement == LEGACY_MEASUREMENT
                    && point.tags.is_empty()
                    && let Some(TimeSeriesValue::Bytes(value)) = point.fields.get(LEGACY_FIELD)
                {
                    series.put(point.timestamp_ns, value.clone());
                }
            }
            Ok(series)
        }
        Err(error) => Err(error),
    }
}

/// Record `value` at timestamp `ts` in series `collection` of `ns`, creating the series and the `time-series`
/// facet if absent, and stage it. A repeated timestamp replaces the point at that instant.
pub fn ts_put<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    ts: i64,
    value: Vec<u8>,
) -> Result<()> {
    ts_put_point(loom, ns, collection, legacy_point(ts, value))
}

/// The point at `ts` in `collection`, or `None` when the timestamp or series is absent.
pub fn ts_get<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    ts: i64,
) -> Result<Option<Vec<u8>>> {
    let to = ts.saturating_add(1);
    for point in ts_range_points(loom, ns, collection, ts, to)? {
        if point.timestamp_ns == ts
            && point.measurement == LEGACY_MEASUREMENT
            && point.tags.is_empty()
            && let Some(TimeSeriesValue::Bytes(value)) = point.fields.get(LEGACY_FIELD)
        {
            return Ok(Some(value.clone()));
        }
    }
    Ok(None)
}

/// The points of `collection` with `from <= ts < to`, in time order (half-open), as a sub-series.
pub fn ts_range<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    from: i64,
    to: i64,
) -> Result<Series> {
    let mut out = Series::new();
    for point in ts_range_points(loom, ns, collection, from, to)? {
        if point.measurement == LEGACY_MEASUREMENT
            && point.tags.is_empty()
            && let Some(TimeSeriesValue::Bytes(value)) = point.fields.get(LEGACY_FIELD)
        {
            out.put(point.timestamp_ns, value.clone());
        }
    }
    Ok(out)
}

/// The most recent point (largest timestamp) of `collection`, or `None` when the series is absent or empty.
pub fn ts_latest<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
) -> Result<Option<(i64, Vec<u8>)>> {
    Ok(ts_range(loom, ns, collection, i64::MIN, i64::MAX)?
        .latest()
        .map(|(ts, value)| (ts, value.to_vec())))
}

/// The time-series collection names present in `ns`'s current working tree, sorted and de-duplicated.
/// Enumeration is within the workspace, not a global index. Reserved names beginning with `.` are
/// excluded.
pub fn ts_list_collections<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId) -> Result<Vec<String>> {
    loom.authorize_collection(ns, FacetKind::TimeSeries, "", AclRight::Read)?;
    let prefix = format!("{}/", facet_root(FacetKind::TimeSeries));
    let mut out: Vec<String> = loom
        .staged_paths(ns)
        .into_iter()
        .filter_map(|p| {
            let rest = p.strip_prefix(&prefix)?;
            if rest.contains('/') || rest.starts_with('.') {
                return None;
            }
            Some(rest.to_string())
        })
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acl::{AclRight, AclSubject};
    use crate::error::Code;
    use crate::identity::IdentityStore;
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};
    use std::collections::BTreeSet;

    fn hex_to_bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    // Canonical time-series metadata root vector (MX-280 closure proof). Pins the CBOR encoding
    // `[version=2, query_start_ns|null, [[name, resolution_ns, aggregation]...]]`.
    #[test]
    fn time_series_metadata_canonical_vectors() {
        let empty = TimeSeriesPolicy {
            query_start_ns: None,
            rollups: Vec::new(),
        };
        assert_eq!(structured_metadata(&empty), hex_to_bytes("8302f680"));

        let with_rollup = TimeSeriesPolicy {
            query_start_ns: Some(1_000_000_000),
            rollups: vec![
                TimeSeriesRollup::new("1m", 60_000_000_000, TimeSeriesAggregation::Mean).unwrap(),
            ],
        };
        let bytes = structured_metadata(&with_rollup);
        assert_eq!(
            bytes,
            hex_to_bytes("83021a3b9aca00818362316d1b0000000df8475800646d65616e")
        );
        // Round-trips to itself (canonical identity).
        assert_eq!(
            structured_metadata(&decode_structured_metadata(&bytes).unwrap()),
            bytes
        );
        assert_eq!(
            decode_structured_metadata(&hex_to_bytes("8201f6"))
                .unwrap_err()
                .code,
            Code::CorruptObject
        );
    }

    // Canonical point prolly-key vector (MX-280). Pins `[measurement, [[tag_name, tag_value]...],
    // timestamp_ns, field]` with tags in sorted order and nanosecond timestamps.
    #[test]
    fn time_series_point_key_canonical_vector() {
        let mut tags = BTreeMap::new();
        tags.insert("host".to_string(), "a".to_string());
        tags.insert("region".to_string(), "us".to_string());
        let point = StructuredPoint {
            measurement: "cpu".to_string(),
            tags,
            timestamp_ns: 1_234_567_890,
            fields: BTreeMap::new(),
        };
        let key = structured_point_key(&point, "value");
        assert_eq!(
            key,
            hex_to_bytes(
                "8463637075828264686f737461618266726567696f6e6275731a499602d26576616c7565"
            )
        );
        let (measurement, decoded_tags, timestamp_ns, field) = decode_structured_key(&key).unwrap();
        assert_eq!(measurement, "cpu");
        assert_eq!(timestamp_ns, 1_234_567_890);
        assert_eq!(field, "value");
        assert_eq!(
            decoded_tags.keys().cloned().collect::<Vec<_>>(),
            vec!["host".to_string(), "region".to_string()]
        );
        // Duplicate-point policy is last-write-wins keyed by this identity: an identical logical
        // point produces byte-identical prolly keys.
        assert_eq!(structured_point_key(&point, "value"), key);
    }

    // Negative decode vectors (MX-280): malformed/unsupported metadata and invalid point keys.
    #[test]
    fn time_series_negative_decode_vectors() {
        for metadata in ["8201f6", "8303f680"] {
            assert_eq!(
                decode_structured_metadata(&hex_to_bytes(metadata))
                    .unwrap_err()
                    .code,
                Code::CorruptObject
            );
        }
        // Non-array metadata.
        assert!(decode_structured_metadata(&hex_to_bytes("01")).is_err());

        let bad_key = |tags: Vec<(&str, &str)>, measurement: &str, field: &str| {
            let tag_items = tags
                .into_iter()
                .map(|(name, value)| {
                    Value::Array(vec![
                        Value::Text(name.to_string()),
                        Value::Text(value.to_string()),
                    ])
                })
                .collect();
            cbor::encode(&Value::Array(vec![
                Value::Text(measurement.to_string()),
                Value::Array(tag_items),
                Value::int(1),
                Value::Text(field.to_string()),
            ]))
        };
        // Empty measurement, duplicate tag name, and empty tag value are all rejected.
        assert!(decode_structured_key(&bad_key(vec![("host", "a")], "", "value")).is_err());
        assert!(
            decode_structured_key(&bad_key(vec![("host", "a"), ("host", "b")], "cpu", "value"))
                .is_err()
        );
        assert!(decode_structured_key(&bad_key(vec![("host", "")], "cpu", "value")).is_err());

        // Unsorted tags decode (into a sorted map) but are not canonical: re-encoding yields the
        // sorted canonical key, distinct from the unsorted input bytes.
        let unsorted = hex_to_bytes(
            "8463637075828266726567696f6e6275738264686f737461611a499602d26576616c7565",
        );
        let (measurement, decoded_tags, timestamp_ns, field) =
            decode_structured_key(&unsorted).unwrap();
        let canonical = structured_point_key(
            &StructuredPoint {
                measurement,
                tags: decoded_tags,
                timestamp_ns,
                fields: BTreeMap::new(),
            },
            &field,
        );
        assert_ne!(canonical, unsorted);
        assert_eq!(
            canonical,
            hex_to_bytes(
                "8463637075828264686f737461618266726567696f6e6275731a499602d26576616c7565"
            )
        );
    }

    #[test]
    fn structured_points_commit_with_reachable_prolly_roots() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::TimeSeries,
                None,
                WorkspaceId::from_bytes([44; 16]),
            )
            .unwrap();
        let mut tags = BTreeMap::new();
        tags.insert("host".to_string(), "api-1".to_string());
        let mut fields = BTreeMap::new();
        fields.insert("value".to_string(), TimeSeriesValue::Float(0.4));
        fields.insert("count".to_string(), TimeSeriesValue::Int(1));
        ts_put_point(
            &mut loom,
            ns,
            "metrics",
            StructuredPoint::new("cpu", tags.clone(), 100, fields).unwrap(),
        )
        .unwrap();
        let mut replacement = BTreeMap::new();
        replacement.insert("value".to_string(), TimeSeriesValue::Float(0.5));
        ts_put_point(
            &mut loom,
            ns,
            "metrics",
            StructuredPoint::new("cpu", tags, 100, replacement).unwrap(),
        )
        .unwrap();
        let points = ts_range_points(&loom, ns, "metrics", 0, 200).unwrap();
        assert_eq!(points.len(), 1);
        assert_eq!(
            points[0].fields.get("count"),
            Some(&TimeSeriesValue::Int(1))
        );
        assert_eq!(
            points[0].fields.get("value"),
            Some(&TimeSeriesValue::Float(0.5))
        );
        let commit = loom.commit(ns, "test", "structured point", 1).unwrap();
        assert!(loom.reachable(&[commit], &BTreeSet::new()).unwrap().len() > 3);
    }

    #[test]
    fn policy_limits_live_structured_queries_without_removing_raw_points() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::TimeSeries,
                None,
                WorkspaceId::from_bytes([45; 16]),
            )
            .unwrap();
        for timestamp_ns in [100, 200] {
            ts_put_point(
                &mut loom,
                ns,
                "metrics",
                StructuredPoint::new(
                    "cpu",
                    BTreeMap::new(),
                    timestamp_ns,
                    BTreeMap::from([("value".to_string(), TimeSeriesValue::Int(timestamp_ns))]),
                )
                .unwrap(),
            )
            .unwrap();
        }
        ts_set_policy(
            &mut loom,
            ns,
            "metrics",
            TimeSeriesPolicy {
                query_start_ns: Some(200),
                rollups: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(
            ts_policy(&loom, ns, "metrics").unwrap().query_start_ns,
            Some(200)
        );
        let visible = ts_range_points(&loom, ns, "metrics", 0, 300).unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].timestamp_ns, 200);
    }

    #[test]
    fn rollups_materialize_and_raw_prune_removes_only_authoritative_points() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::TimeSeries,
                None,
                WorkspaceId::from_bytes([46; 16]),
            )
            .unwrap();
        for (timestamp_ns, value) in [(100, 1), (150, 3), (250, 9)] {
            ts_put_point(
                &mut loom,
                ns,
                "metrics",
                StructuredPoint::new(
                    "cpu",
                    BTreeMap::from([("host".to_string(), "api-1".to_string())]),
                    timestamp_ns,
                    BTreeMap::from([("value".to_string(), TimeSeriesValue::Int(value))]),
                )
                .unwrap(),
            )
            .unwrap();
        }
        ts_set_policy(
            &mut loom,
            ns,
            "metrics",
            TimeSeriesPolicy {
                query_start_ns: None,
                rollups: vec![
                    TimeSeriesRollup::new("minute_mean", 100, TimeSeriesAggregation::Mean).unwrap(),
                ],
            },
        )
        .unwrap();
        ts_materialize_rollup(&mut loom, ns, "metrics", "minute_mean").unwrap();

        let rollup = ts_range_rollup_points(&loom, ns, "metrics", "minute_mean", 0, 300).unwrap();
        assert_eq!(rollup.len(), 2);
        assert_eq!(rollup[0].timestamp_ns, 100);
        assert_eq!(
            rollup[0].fields.get("value"),
            Some(&TimeSeriesValue::Float(2.0))
        );
        assert_eq!(rollup[1].timestamp_ns, 200);

        assert_eq!(ts_prune_before(&mut loom, ns, "metrics", 200).unwrap(), 2);
        let raw = ts_range_points(&loom, ns, "metrics", 0, 300).unwrap();
        assert_eq!(raw.len(), 1);
        assert_eq!(raw[0].timestamp_ns, 250);
        let retained_rollup =
            ts_range_rollup_points(&loom, ns, "metrics", "minute_mean", 0, 300).unwrap();
        assert_eq!(retained_rollup.len(), 2);
    }

    #[test]
    fn list_collections_enumerates_series_names_sorted() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::TimeSeries,
                None,
                WorkspaceId::from_bytes([9; 16]),
            )
            .unwrap();
        assert!(ts_list_collections(&loom, ns).unwrap().is_empty());
        ts_put(&mut loom, ns, "cpu", 100, b"a".to_vec()).unwrap();
        ts_put(&mut loom, ns, "mem", 200, b"b".to_vec()).unwrap();
        ts_put(&mut loom, ns, "cpu", 300, b"c".to_vec()).unwrap();
        assert_eq!(ts_list_collections(&loom, ns).unwrap(), vec!["cpu", "mem"]);
    }

    #[test]
    fn points_in_time_order_range_and_latest() {
        let mut s = Series::new();
        s.put(300, b"c".to_vec());
        s.put(100, b"a".to_vec());
        s.put(200, b"b".to_vec());
        let times: Vec<i64> = s.iter().map(|(t, _)| t).collect();
        assert_eq!(times, [100, 200, 300]);
        // half-open range [100, 300): 100, 200
        assert_eq!(
            s.range(100, 300)
                .iter()
                .map(|(t, _)| *t)
                .collect::<Vec<_>>(),
            [100, 200]
        );
        assert_eq!(s.latest(), Some((300, &b"c"[..])));
        // repeated timestamp replaces
        s.put(200, b"b2".to_vec());
        assert_eq!(s.get(200), Some(&b"b2"[..]));
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn authenticated_time_series_operations_are_acl_checked() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::TimeSeries,
                None,
                WorkspaceId::from_bytes([26; 16]),
            )
            .unwrap();
        let root = WorkspaceId::from_bytes([1; 16]);
        let mut identity = IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);

        assert_eq!(
            ts_put(&mut loom, ns, "cpu", 1, b"one".to_vec())
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );

        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::TimeSeries),
                [AclRight::Write, AclRight::Read],
            )
            .unwrap();

        ts_put(&mut loom, ns, "cpu", 1, b"one".to_vec()).unwrap();
        assert_eq!(
            ts_get(&loom, ns, "cpu", 1).unwrap().as_deref(),
            Some(&b"one"[..])
        );
    }

    #[test]
    fn encode_round_trips_and_versions() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::TimeSeries,
                None,
                WorkspaceId::from_bytes([8; 16]),
            )
            .unwrap();
        let mut s = Series::new();
        s.put(1, b"x".to_vec());
        s.put(2, b"y".to_vec());
        assert_eq!(Series::decode(&s.encode()).unwrap().len(), 2);

        put_series(&mut loom, ns, "cpu", &s).unwrap();
        let c1 = loom.commit(ns, "nas", "two points", 1).unwrap();
        s.put(3, b"z".to_vec());
        put_series(&mut loom, ns, "cpu", &s).unwrap();
        loom.commit(ns, "nas", "three points", 2).unwrap();
        assert_eq!(get_series(&loom, ns, "cpu").unwrap().len(), 3);
        loom.checkout_commit(ns, c1).unwrap();
        assert_eq!(get_series(&loom, ns, "cpu").unwrap().len(), 2);
    }

    #[test]
    fn facade_put_get_range_latest_and_absent() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::TimeSeries,
                None,
                WorkspaceId::from_bytes([9; 16]),
            )
            .unwrap();

        assert_eq!(ts_get(&loom, ns, "s", 1).unwrap(), None);
        assert!(ts_latest(&loom, ns, "s").unwrap().is_none());

        for (t, v) in [(100i64, b"a"), (200, b"b"), (300, b"c")] {
            ts_put(&mut loom, ns, "s", t, v.to_vec()).unwrap();
        }
        assert_eq!(
            ts_get(&loom, ns, "s", 200).unwrap().as_deref(),
            Some(&b"b"[..])
        );
        // half-open [100, 300) excludes 300.
        let times: Vec<i64> = ts_range(&loom, ns, "s", 100, 300)
            .unwrap()
            .iter()
            .map(|(t, _)| t)
            .collect();
        assert_eq!(times, [100, 200]);
        assert_eq!(
            ts_latest(&loom, ns, "s").unwrap(),
            Some((300, b"c".to_vec()))
        );
    }
}
