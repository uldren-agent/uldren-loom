//! Canonical native metrics contracts.

use loom_codec::Value;
use loom_types::{Digest, LoomError, Result};
use std::collections::BTreeMap;

pub const DESCRIPTOR_SCHEMA: &str = "loom.metrics.descriptor.v1";
pub const OBSERVATION_SCHEMA: &str = "loom.metrics.observation.v1";
pub const ROLLUP_PROFILE_SCHEMA: &str = "loom.metrics.rollup.profile.v1";
pub const ROLLUP_DERIVED_ID_SCHEMA: &str = "loom.metrics.rollup.derived-id.v1";
pub const ROLLUP_RECORD_SCHEMA: &str = "loom.metrics.rollup.record.v1";
pub const DEFAULT_RETENTION_MS: u64 = 86_400_000;
pub const MAX_EXEMPLARS_PER_OBSERVATION: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrumentKind {
    Counter,
    UpDownCounter,
    Gauge,
    Histogram,
    ExponentialHistogram,
    Summary,
}

impl InstrumentKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Counter => "counter",
            Self::UpDownCounter => "up_down_counter",
            Self::Gauge => "gauge",
            Self::Histogram => "histogram",
            Self::ExponentialHistogram => "exponential_histogram",
            Self::Summary => "summary",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "counter" => Ok(Self::Counter),
            "up_down_counter" => Ok(Self::UpDownCounter),
            "gauge" => Ok(Self::Gauge),
            "histogram" => Ok(Self::Histogram),
            "exponential_histogram" => Ok(Self::ExponentialHistogram),
            "summary" => Ok(Self::Summary),
            _ => Err(LoomError::corrupt("unknown metric instrument kind")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Temporality {
    Delta,
    Cumulative,
    Instant,
}

impl Temporality {
    fn as_str(self) -> &'static str {
        match self {
            Self::Delta => "delta",
            Self::Cumulative => "cumulative",
            Self::Instant => "instant",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "delta" => Ok(Self::Delta),
            "cumulative" => Ok(Self::Cumulative),
            "instant" => Ok(Self::Instant),
            _ => Err(LoomError::corrupt("unknown metric temporality")),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MetricDistribution {
    pub explicit_bounds: Vec<f64>,
    pub max_exemplars: u32,
}

impl MetricDistribution {
    pub fn explicit_histogram(explicit_bounds: Vec<f64>, max_exemplars: u32) -> Result<Self> {
        let distribution = Self {
            explicit_bounds,
            max_exemplars,
        };
        distribution.validate()?;
        Ok(distribution)
    }

    fn validate(&self) -> Result<()> {
        if self.max_exemplars > MAX_EXEMPLARS_PER_OBSERVATION {
            return Err(LoomError::invalid("metric exemplar limit is too large"));
        }
        if self.explicit_bounds.iter().any(|value| !value.is_finite())
            || self
                .explicit_bounds
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(LoomError::invalid(
                "metric histogram bounds must be finite and ascending",
            ));
        }
        Ok(())
    }

    fn encode(&self) -> Value {
        Value::Array(vec![
            Value::Array(
                self.explicit_bounds
                    .iter()
                    .copied()
                    .map(Value::Float)
                    .collect(),
            ),
            Value::Uint(self.max_exemplars.into()),
        ])
    }

    fn decode(value: Value) -> Result<Self> {
        let fields = array(value)?;
        if fields.len() != 2 {
            return Err(LoomError::corrupt("metric distribution is invalid"));
        }
        let explicit_bounds = array(fields[0].clone())?
            .into_iter()
            .map(f64_value)
            .collect::<Result<Vec<_>>>()?;
        let max_exemplars = u64_value(&fields[1])?
            .try_into()
            .map_err(|_| LoomError::corrupt("metric exemplar limit is invalid"))?;
        Self::explicit_histogram(explicit_bounds, max_exemplars)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricRollupAggregation {
    Sum,
    Last,
    MinMaxAvg,
    HistogramMerge,
}

impl MetricRollupAggregation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Sum => "sum",
            Self::Last => "last",
            Self::MinMaxAvg => "min_max_avg",
            Self::HistogramMerge => "histogram_merge",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "sum" => Ok(Self::Sum),
            "last" => Ok(Self::Last),
            "min_max_avg" => Ok(Self::MinMaxAvg),
            "histogram_merge" => Ok(Self::HistogramMerge),
            _ => Err(LoomError::corrupt("unknown metric rollup aggregation")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricRollupProfile {
    pub name: String,
    pub period_ms: u64,
    pub retention_ms: u64,
    pub lateness_ms: u64,
    pub aggregation: MetricRollupAggregation,
}

impl MetricRollupProfile {
    pub fn new(
        name: String,
        period_ms: u64,
        retention_ms: u64,
        lateness_ms: u64,
        aggregation: MetricRollupAggregation,
    ) -> Result<Self> {
        let profile = Self {
            name,
            period_ms,
            retention_ms,
            lateness_ms,
            aggregation,
        };
        profile.validate()?;
        Ok(profile)
    }

    fn validate(&self) -> Result<()> {
        if self.name.is_empty()
            || self.name.contains('/')
            || self.period_ms == 0
            || self.retention_ms == 0
            || self.retention_ms < self.period_ms
            || self.lateness_ms >= self.period_ms
        {
            return Err(LoomError::invalid("metric rollup profile is invalid"));
        }
        Ok(())
    }

    fn encode_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(ROLLUP_PROFILE_SCHEMA.into()),
            Value::Text(self.name.clone()),
            Value::Uint(self.period_ms),
            Value::Uint(self.retention_ms),
            Value::Uint(self.lateness_ms),
            Value::Text(self.aggregation.as_str().into()),
        ])
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        loom_codec::encode(&self.encode_value()).map_err(|err| {
            LoomError::invalid(format!("metric rollup profile encoding failed: {err}"))
        })
    }

    fn decode(value: Value) -> Result<Self> {
        let fields = array(value)?;
        if fields.len() != 6 || text(&fields[0])? != ROLLUP_PROFILE_SCHEMA {
            return Err(LoomError::corrupt("metric rollup profile is invalid"));
        }
        Self::new(
            text(&fields[1])?.to_owned(),
            u64_value(&fields[2])?,
            u64_value(&fields[3])?,
            u64_value(&fields[4])?,
            MetricRollupAggregation::parse(text(&fields[5])?)?,
        )
    }

    pub fn profile_id(&self) -> Result<String> {
        Ok(Digest::blake3(&self.encode()?).to_hex())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricRollupWindow {
    pub start_ms: u64,
    pub end_ms: u64,
}

impl MetricRollupWindow {
    pub fn for_timestamp(timestamp_ms: u64, period_ms: u64) -> Result<Self> {
        if timestamp_ms == 0 || period_ms == 0 {
            return Err(LoomError::invalid("metric rollup window is invalid"));
        }
        let start_ms = ((timestamp_ms - 1) / period_ms) * period_ms;
        Ok(Self {
            start_ms,
            end_ms: start_ms.saturating_add(period_ms),
        })
    }

    pub fn contains(&self, timestamp_ms: u64) -> bool {
        self.start_ms < timestamp_ms && timestamp_ms <= self.end_ms
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricDerivedRollupId {
    pub descriptor: Digest,
    pub series_id: String,
    pub profile_id: String,
    pub window_start_ms: u64,
    pub window_end_ms: u64,
}

impl MetricDerivedRollupId {
    pub fn new(
        descriptor: Digest,
        series_id: String,
        profile_id: String,
        window: MetricRollupWindow,
    ) -> Result<Self> {
        if series_id.is_empty()
            || profile_id.is_empty()
            || window.window_start_is_invalid()
            || window.end_ms <= window.start_ms
        {
            return Err(LoomError::invalid("metric derived rollup id is invalid"));
        }
        Ok(Self {
            descriptor,
            series_id,
            profile_id,
            window_start_ms: window.start_ms,
            window_end_ms: window.end_ms,
        })
    }

    fn encode_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(ROLLUP_DERIVED_ID_SCHEMA.into()),
            Value::Text(self.descriptor.to_string()),
            Value::Text(self.series_id.clone()),
            Value::Text(self.profile_id.clone()),
            Value::Uint(self.window_start_ms),
            Value::Uint(self.window_end_ms),
        ])
    }

    fn decode_value(value: Value) -> Result<Self> {
        let fields = array(value)?;
        if fields.len() != 6 || text(&fields[0])? != ROLLUP_DERIVED_ID_SCHEMA {
            return Err(LoomError::corrupt("metric derived rollup id is invalid"));
        }
        let window = MetricRollupWindow {
            start_ms: u64_value(&fields[4])?,
            end_ms: u64_value(&fields[5])?,
        };
        Self::new(
            Digest::parse(text(&fields[1])?)
                .map_err(|_| LoomError::corrupt("metric descriptor digest is invalid"))?,
            text(&fields[2])?.to_owned(),
            text(&fields[3])?.to_owned(),
            window,
        )
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.encode_value()).map_err(|err| {
            LoomError::invalid(format!("metric derived rollup id encoding failed: {err}"))
        })
    }

    pub fn digest(&self) -> Result<String> {
        Ok(Digest::blake3(&self.encode()?).to_hex())
    }
}

impl MetricRollupWindow {
    fn window_start_is_invalid(&self) -> bool {
        self.end_ms <= self.start_ms
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricRollupWindowStatus {
    Partial,
    Final,
    Stale,
}

impl MetricRollupWindowStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Partial => "partial",
            Self::Final => "final",
            Self::Stale => "stale",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "partial" => Ok(Self::Partial),
            "final" => Ok(Self::Final),
            "stale" => Ok(Self::Stale),
            _ => Err(LoomError::corrupt("unknown metric rollup window status")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MetricRollupValue {
    Number(f64),
    MinMaxAvg { min: f64, max: f64, avg: f64 },
    Histogram(MetricHistogram),
}

impl MetricRollupValue {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Number(value) if value.is_finite() => Ok(()),
            Self::MinMaxAvg { min, max, avg }
                if min.is_finite() && max.is_finite() && avg.is_finite() && min <= max =>
            {
                Ok(())
            }
            Self::Histogram(histogram) => histogram.validate(histogram.buckets.len() - 1),
            _ => Err(LoomError::invalid("metric rollup value is invalid")),
        }
    }

    fn encode(&self) -> Value {
        match self {
            Self::Number(value) => {
                Value::Array(vec![Value::Text("number".into()), Value::Float(*value)])
            }
            Self::MinMaxAvg { min, max, avg } => Value::Array(vec![
                Value::Text("min_max_avg".into()),
                Value::Float(*min),
                Value::Float(*max),
                Value::Float(*avg),
            ]),
            Self::Histogram(histogram) => Value::Array(vec![
                Value::Text("histogram".into()),
                Value::Array(histogram.buckets.iter().copied().map(Value::Uint).collect()),
                Value::Uint(histogram.count),
                Value::Float(histogram.sum),
            ]),
        }
    }

    fn decode(value: Value) -> Result<Self> {
        let fields = array(value)?;
        match fields.first().map(text).transpose()? {
            Some("number") if fields.len() == 2 => Ok(Self::Number(f64_value(fields[1].clone())?)),
            Some("min_max_avg") if fields.len() == 4 => Ok(Self::MinMaxAvg {
                min: f64_value(fields[1].clone())?,
                max: f64_value(fields[2].clone())?,
                avg: f64_value(fields[3].clone())?,
            }),
            Some("histogram") if fields.len() == 4 => {
                let buckets = array(fields[1].clone())?
                    .into_iter()
                    .map(|value| u64_value(&value))
                    .collect::<Result<Vec<_>>>()?;
                Ok(Self::Histogram(MetricHistogram {
                    buckets,
                    count: u64_value(&fields[2])?,
                    sum: f64_value(fields[3].clone())?,
                }))
            }
            _ => Err(LoomError::corrupt("metric rollup value is invalid")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricMaterializedRollup {
    pub id: MetricDerivedRollupId,
    pub profile_name: String,
    pub aggregation: MetricRollupAggregation,
    pub status: MetricRollupWindowStatus,
    pub sample_count: u64,
    pub value: MetricRollupValue,
    pub source_start_ms: u64,
    pub source_end_ms: u64,
    pub watermark_ms: u64,
}

impl MetricMaterializedRollup {
    pub fn with_status(&self, status: MetricRollupWindowStatus) -> Result<Self> {
        let rollup = Self {
            id: self.id.clone(),
            profile_name: self.profile_name.clone(),
            aggregation: self.aggregation,
            status,
            sample_count: self.sample_count,
            value: self.value.clone(),
            source_start_ms: self.source_start_ms,
            source_end_ms: self.source_end_ms,
            watermark_ms: self.watermark_ms,
        };
        rollup.validate()?;
        Ok(rollup)
    }

    pub fn validate(&self) -> Result<()> {
        if self.profile_name.is_empty()
            || self.profile_name.contains('/')
            || self.sample_count == 0
            || self.source_start_ms == 0
            || self.source_end_ms == 0
            || self.source_start_ms > self.source_end_ms
            || self.id.window_end_ms <= self.id.window_start_ms
        {
            return Err(LoomError::invalid("metric materialized rollup is invalid"));
        }
        self.value.validate()
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        loom_codec::encode(&Value::Array(vec![
            Value::Text(ROLLUP_RECORD_SCHEMA.into()),
            self.id.encode_value(),
            Value::Text(self.profile_name.clone()),
            Value::Text(self.aggregation.as_str().into()),
            Value::Text(self.status.as_str().into()),
            Value::Uint(self.sample_count),
            self.value.encode(),
            Value::Uint(self.source_start_ms),
            Value::Uint(self.source_end_ms),
            Value::Uint(self.watermark_ms),
        ]))
        .map_err(|err| {
            LoomError::invalid(format!("metric materialized rollup encoding failed: {err}"))
        })
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let fields = array(loom_codec::decode(bytes).map_err(|err| {
            LoomError::corrupt(format!("invalid metric materialized rollup CBOR: {err}"))
        })?)?;
        if fields.len() != 10 || text(&fields[0])? != ROLLUP_RECORD_SCHEMA {
            return Err(LoomError::corrupt(
                "invalid metric materialized rollup schema",
            ));
        }
        let rollup = Self {
            id: MetricDerivedRollupId::decode_value(fields[1].clone())?,
            profile_name: text(&fields[2])?.to_owned(),
            aggregation: MetricRollupAggregation::parse(text(&fields[3])?)?,
            status: MetricRollupWindowStatus::parse(text(&fields[4])?)?,
            sample_count: u64_value(&fields[5])?,
            value: MetricRollupValue::decode(fields[6].clone())?,
            source_start_ms: u64_value(&fields[7])?,
            source_end_ms: u64_value(&fields[8])?,
            watermark_ms: u64_value(&fields[9])?,
        };
        rollup.validate()?;
        Ok(rollup)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricDescriptor {
    pub name: String,
    pub description: String,
    pub unit: String,
    pub kind: InstrumentKind,
    pub temporality: Temporality,
    pub attribute_keys: Vec<String>,
    pub attribute_value_limits: BTreeMap<String, u32>,
    pub max_active_series: u32,
    pub stale_after_ms: u64,
    pub retention_ms: u64,
    pub distribution: MetricDistribution,
    pub rollup_profiles: Vec<MetricRollupProfile>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricDescriptorPolicy {
    pub attribute_value_limits: BTreeMap<String, u32>,
    pub max_active_series: u32,
    pub stale_after_ms: u64,
    pub retention_ms: u64,
    pub distribution: MetricDistribution,
    pub rollup_profiles: Vec<MetricRollupProfile>,
}

impl MetricDescriptorPolicy {
    pub fn new(
        attribute_value_limits: BTreeMap<String, u32>,
        max_active_series: u32,
        stale_after_ms: u64,
        retention_ms: u64,
        distribution: MetricDistribution,
    ) -> Self {
        Self {
            attribute_value_limits,
            max_active_series,
            stale_after_ms,
            retention_ms,
            distribution,
            rollup_profiles: Vec::new(),
        }
    }

    pub fn with_rollup_profiles(mut self, rollup_profiles: Vec<MetricRollupProfile>) -> Self {
        self.rollup_profiles = rollup_profiles;
        self
    }

    fn with_optional_rollup_profiles(mut self, rollup_profiles: Vec<MetricRollupProfile>) -> Self {
        self.rollup_profiles = rollup_profiles;
        self
    }
}

impl MetricDescriptor {
    pub fn new(
        name: String,
        description: String,
        unit: String,
        kind: InstrumentKind,
        temporality: Temporality,
        attribute_keys: Vec<String>,
        max_active_series: u32,
        stale_after_ms: u64,
    ) -> Result<Self> {
        Self::with_policy(
            name,
            description,
            unit,
            kind,
            temporality,
            attribute_keys,
            MetricDescriptorPolicy::new(
                BTreeMap::new(),
                max_active_series,
                stale_after_ms,
                DEFAULT_RETENTION_MS,
                MetricDistribution::default(),
            ),
        )
    }

    pub fn with_policy(
        name: String,
        description: String,
        unit: String,
        kind: InstrumentKind,
        temporality: Temporality,
        attribute_keys: Vec<String>,
        policy: MetricDescriptorPolicy,
    ) -> Result<Self> {
        let descriptor = Self {
            name,
            description,
            unit,
            kind,
            temporality,
            attribute_keys,
            attribute_value_limits: policy.attribute_value_limits,
            max_active_series: policy.max_active_series,
            stale_after_ms: policy.stale_after_ms,
            retention_ms: policy.retention_ms,
            distribution: policy.distribution,
            rollup_profiles: policy.rollup_profiles,
        };
        descriptor.validate()?;
        Ok(descriptor)
    }

    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty()
            || self.name.contains('/')
            || self.unit.is_empty()
            || self.max_active_series == 0
            || self.retention_ms == 0
            || self.attribute_keys.iter().any(String::is_empty)
        {
            return Err(LoomError::invalid("metric descriptor fields must be valid"));
        }
        if self
            .attribute_keys
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(LoomError::invalid(
                "metric descriptor attributes must be sorted and unique",
            ));
        }
        for (key, limit) in &self.attribute_value_limits {
            if *limit == 0 || self.attribute_keys.binary_search(key).is_err() {
                return Err(LoomError::invalid(
                    "metric attribute cardinality limits must target declared attributes",
                ));
            }
        }
        self.distribution.validate()?;
        let mut profile_names = BTreeMap::new();
        let mut profile_periods = BTreeMap::new();
        let mut profile_ids = BTreeMap::new();
        for profile in &self.rollup_profiles {
            profile.validate()?;
            if profile.retention_ms < self.retention_ms {
                return Err(LoomError::invalid(
                    "metric rollup retention must cover raw retention",
                ));
            }
            if !self.allows_rollup_aggregation(profile.aggregation) {
                return Err(LoomError::invalid(
                    "metric rollup aggregation is invalid for instrument",
                ));
            }
            if profile_names.insert(profile.name.clone(), ()).is_some()
                || profile_periods.insert(profile.period_ms, ()).is_some()
                || profile_ids.insert(profile.profile_id()?, ()).is_some()
            {
                return Err(LoomError::invalid(
                    "metric rollup profiles must be uniquely identified",
                ));
            }
        }
        if matches!(self.kind, InstrumentKind::Histogram) {
            if self.distribution.explicit_bounds.is_empty() {
                return Err(LoomError::invalid(
                    "metric histogram bounds must be declared",
                ));
            }
        } else if !self.distribution.explicit_bounds.is_empty() {
            return Err(LoomError::invalid(
                "metric distribution bounds require a histogram instrument",
            ));
        }
        if matches!(self.kind, InstrumentKind::Gauge) && self.temporality != Temporality::Instant {
            return Err(LoomError::invalid(
                "metric gauge temporality must be instant",
            ));
        }
        if !matches!(self.kind, InstrumentKind::Gauge)
            && matches!(self.temporality, Temporality::Instant)
        {
            return Err(LoomError::invalid(
                "metric temporality must be delta or cumulative",
            ));
        }
        Ok(())
    }

    fn allows_rollup_aggregation(&self, aggregation: MetricRollupAggregation) -> bool {
        match self.kind {
            InstrumentKind::Counter | InstrumentKind::UpDownCounter => {
                matches!(aggregation, MetricRollupAggregation::Sum)
            }
            InstrumentKind::Gauge => {
                matches!(
                    aggregation,
                    MetricRollupAggregation::Last | MetricRollupAggregation::MinMaxAvg
                )
            }
            InstrumentKind::Histogram => {
                matches!(aggregation, MetricRollupAggregation::HistogramMerge)
            }
            InstrumentKind::ExponentialHistogram | InstrumentKind::Summary => false,
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        loom_codec::encode(&Value::Array(vec![
            Value::Text(DESCRIPTOR_SCHEMA.into()),
            Value::Text(self.name.clone()),
            Value::Text(self.description.clone()),
            Value::Text(self.unit.clone()),
            Value::Text(self.kind.as_str().into()),
            Value::Text(self.temporality.as_str().into()),
            Value::Array(
                self.attribute_keys
                    .iter()
                    .cloned()
                    .map(Value::Text)
                    .collect(),
            ),
            Value::Array(
                self.attribute_value_limits
                    .iter()
                    .map(|(key, value)| {
                        Value::Array(vec![Value::Text(key.clone()), Value::Uint((*value).into())])
                    })
                    .collect(),
            ),
            Value::Uint(self.max_active_series.into()),
            Value::Uint(self.stale_after_ms),
            Value::Uint(self.retention_ms),
            self.distribution.encode(),
            Value::Array(
                self.rollup_profiles
                    .iter()
                    .map(MetricRollupProfile::encode_value)
                    .collect(),
            ),
        ]))
        .map_err(|err| LoomError::invalid(format!("metric descriptor encoding failed: {err}")))
    }

    pub fn digest(&self) -> Result<Digest> {
        Ok(Digest::blake3(&self.encode()?))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let fields = array(loom_codec::decode(bytes).map_err(|err| {
            LoomError::corrupt(format!("invalid metric descriptor CBOR: {err}"))
        })?)?;
        if text(&fields[0])? != DESCRIPTOR_SCHEMA {
            return Err(LoomError::corrupt("invalid metric descriptor schema"));
        }
        let keys = array(fields[6].clone())?
            .into_iter()
            .map(|value| text(&value).map(str::to_owned))
            .collect::<Result<Vec<_>>>()?;
        if fields.len() != 13 {
            return Err(LoomError::corrupt("invalid metric descriptor schema"));
        }
        let mut attribute_value_limits = BTreeMap::new();
        for pair in array(fields[7].clone())? {
            let pair = array(pair)?;
            if pair.len() != 2 {
                return Err(LoomError::corrupt("metric attribute limit is invalid"));
            }
            if attribute_value_limits
                .insert(
                    text(&pair[0])?.to_owned(),
                    u64_value(&pair[1])?
                        .try_into()
                        .map_err(|_| LoomError::corrupt("metric attribute limit is invalid"))?,
                )
                .is_some()
            {
                return Err(LoomError::corrupt(
                    "metric attribute limit key is duplicated",
                ));
            }
        }
        Self::with_policy(
            text(&fields[1])?.to_owned(),
            text(&fields[2])?.to_owned(),
            text(&fields[3])?.to_owned(),
            InstrumentKind::parse(text(&fields[4])?)?,
            Temporality::parse(text(&fields[5])?)?,
            keys,
            MetricDescriptorPolicy::new(
                attribute_value_limits,
                u64_value(&fields[8])?
                    .try_into()
                    .map_err(|_| LoomError::corrupt("metric series limit is invalid"))?,
                u64_value(&fields[9])?,
                u64_value(&fields[10])?,
                MetricDistribution::decode(fields[11].clone())?,
            )
            .with_optional_rollup_profiles(
                array(fields[12].clone())?
                    .into_iter()
                    .map(MetricRollupProfile::decode)
                    .collect::<Result<Vec<_>>>()?,
            ),
        )
    }

    pub fn rollup_profile(&self, name: &str) -> Option<&MetricRollupProfile> {
        self.rollup_profiles
            .iter()
            .find(|profile| profile.name == name)
    }

    pub fn derived_rollup_id(
        &self,
        observation: &MetricObservation,
        profile_name: &str,
    ) -> Result<MetricDerivedRollupId> {
        self.validate_observation(observation)?;
        let profile = self
            .rollup_profile(profile_name)
            .ok_or_else(|| LoomError::not_found("metric rollup profile not found"))?;
        let window =
            MetricRollupWindow::for_timestamp(observation.timestamp_ms, profile.period_ms)?;
        MetricDerivedRollupId::new(
            self.digest()?,
            observation.series_id()?,
            profile.profile_id()?,
            window,
        )
    }

    pub fn validate_observation(&self, observation: &MetricObservation) -> Result<()> {
        if observation.descriptor != self.digest()? {
            return Err(LoomError::invalid(
                "metric observation descriptor identity is invalid",
            ));
        }
        if observation.timestamp_ms == 0
            || observation
                .start_timestamp_ms
                .is_some_and(|start| start > observation.timestamp_ms)
        {
            return Err(LoomError::invalid(
                "metric observation timestamp is invalid",
            ));
        }
        if observation.attributes.len() > self.attribute_keys.len()
            || observation
                .attributes
                .keys()
                .any(|key| self.attribute_keys.binary_search(key).is_err())
        {
            return Err(LoomError::invalid(
                "metric observation has an undeclared attribute",
            ));
        }
        for (key, limit) in &self.attribute_value_limits {
            if observation
                .attributes
                .get(key)
                .is_some_and(|value| value.len() > *limit as usize)
            {
                return Err(LoomError::new(
                    loom_types::Code::ResourceExhausted,
                    "metric attribute cardinality limit exceeded",
                ));
            }
        }
        if observation.exemplars.len() > self.distribution.max_exemplars as usize {
            return Err(LoomError::new(
                loom_types::Code::ResourceExhausted,
                "metric exemplar limit exceeded",
            ));
        }
        match (&self.kind, &observation.value) {
            (InstrumentKind::Counter, MetricValue::Number(value)) if *value < 0.0 => {
                return Err(LoomError::invalid(
                    "metric counter value must not be negative",
                ));
            }
            (InstrumentKind::Histogram, MetricValue::Histogram(histogram)) => {
                histogram.validate(self.distribution.explicit_bounds.len())?;
            }
            (InstrumentKind::Histogram, _) => {
                return Err(LoomError::invalid(
                    "metric histogram observation must carry histogram buckets",
                ));
            }
            (_, MetricValue::Histogram(_)) => {
                return Err(LoomError::invalid(
                    "metric histogram buckets require a histogram descriptor",
                ));
            }
            (_, MetricValue::Number(value)) if !value.is_finite() => {
                return Err(LoomError::invalid("metric observation value is invalid"));
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricHistogram {
    pub buckets: Vec<u64>,
    pub count: u64,
    pub sum: f64,
}

impl MetricHistogram {
    pub fn new(buckets: Vec<u64>, count: u64, sum: f64) -> Result<Self> {
        let histogram = Self {
            buckets,
            count,
            sum,
        };
        histogram.validate(histogram.buckets.len().saturating_sub(1))?;
        Ok(histogram)
    }

    fn validate(&self, boundary_count: usize) -> Result<()> {
        if !self.sum.is_finite()
            || self.buckets.len() != boundary_count + 1
            || self.buckets.iter().sum::<u64>() != self.count
        {
            return Err(LoomError::invalid("metric histogram buckets are invalid"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MetricValue {
    Number(f64),
    Histogram(MetricHistogram),
}

impl MetricValue {
    fn encode(&self) -> Value {
        match self {
            Self::Number(value) => {
                Value::Array(vec![Value::Text("number".into()), Value::Float(*value)])
            }
            Self::Histogram(histogram) => Value::Array(vec![
                Value::Text("histogram".into()),
                Value::Array(histogram.buckets.iter().copied().map(Value::Uint).collect()),
                Value::Uint(histogram.count),
                Value::Float(histogram.sum),
            ]),
        }
    }

    fn decode(value: Value) -> Result<Self> {
        let fields = array(value)?;
        match fields.first().map(text).transpose()? {
            Some("number") if fields.len() == 2 => Ok(Self::Number(f64_value(fields[1].clone())?)),
            Some("histogram") if fields.len() == 4 => {
                let buckets = array(fields[1].clone())?
                    .into_iter()
                    .map(|value| u64_value(&value))
                    .collect::<Result<Vec<_>>>()?;
                Ok(Self::Histogram(MetricHistogram {
                    buckets,
                    count: u64_value(&fields[2])?,
                    sum: f64_value(fields[3].clone())?,
                }))
            }
            _ => Err(LoomError::corrupt("metric observation value is invalid")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricExemplar {
    pub attributes: BTreeMap<String, String>,
    pub timestamp_ms: u64,
    pub value: f64,
}

impl MetricExemplar {
    pub fn new(
        attributes: BTreeMap<String, String>,
        timestamp_ms: u64,
        value: f64,
    ) -> Result<Self> {
        let exemplar = Self {
            attributes,
            timestamp_ms,
            value,
        };
        exemplar.validate()?;
        Ok(exemplar)
    }

    fn validate(&self) -> Result<()> {
        if self.timestamp_ms == 0
            || !self.value.is_finite()
            || self
                .attributes
                .iter()
                .any(|(key, value)| key.is_empty() || value.is_empty())
        {
            return Err(LoomError::invalid("metric exemplar fields must be valid"));
        }
        Ok(())
    }

    fn encode(&self) -> Value {
        Value::Array(vec![
            encode_pairs(&self.attributes),
            Value::Uint(self.timestamp_ms),
            Value::Float(self.value),
        ])
    }

    fn decode(value: Value) -> Result<Self> {
        let fields = array(value)?;
        if fields.len() != 3 {
            return Err(LoomError::corrupt("metric exemplar is invalid"));
        }
        Self::new(
            decode_pairs(fields[0].clone())?,
            u64_value(&fields[1])?,
            f64_value(fields[2].clone())?,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricObservation {
    pub descriptor: Digest,
    pub resource: BTreeMap<String, String>,
    pub scope: BTreeMap<String, String>,
    pub attributes: BTreeMap<String, String>,
    pub start_timestamp_ms: Option<u64>,
    pub timestamp_ms: u64,
    pub value: MetricValue,
    pub flags: Vec<String>,
    pub exemplars: Vec<MetricExemplar>,
}

impl MetricObservation {
    pub fn new(
        descriptor: Digest,
        attributes: BTreeMap<String, String>,
        timestamp_ms: u64,
        value: f64,
    ) -> Result<Self> {
        Self::with_value(
            descriptor,
            BTreeMap::new(),
            BTreeMap::new(),
            attributes,
            None,
            timestamp_ms,
            MetricValue::Number(value),
            Vec::new(),
        )
    }

    pub fn with_value(
        descriptor: Digest,
        resource: BTreeMap<String, String>,
        scope: BTreeMap<String, String>,
        attributes: BTreeMap<String, String>,
        start_timestamp_ms: Option<u64>,
        timestamp_ms: u64,
        value: MetricValue,
        exemplars: Vec<MetricExemplar>,
    ) -> Result<Self> {
        Self::with_value_and_flags(
            descriptor,
            resource,
            scope,
            attributes,
            start_timestamp_ms,
            timestamp_ms,
            value,
            Vec::new(),
            exemplars,
        )
    }

    pub fn with_value_and_flags(
        descriptor: Digest,
        resource: BTreeMap<String, String>,
        scope: BTreeMap<String, String>,
        attributes: BTreeMap<String, String>,
        start_timestamp_ms: Option<u64>,
        timestamp_ms: u64,
        value: MetricValue,
        flags: Vec<String>,
        exemplars: Vec<MetricExemplar>,
    ) -> Result<Self> {
        let observation = Self {
            descriptor,
            resource,
            scope,
            attributes,
            start_timestamp_ms,
            timestamp_ms,
            value,
            flags,
            exemplars,
        };
        observation.validate()?;
        Ok(observation)
    }

    fn validate(&self) -> Result<()> {
        if self.timestamp_ms == 0
            || invalid_pairs(&self.resource)
            || invalid_pairs(&self.scope)
            || self
                .attributes
                .iter()
                .any(|(key, value)| key.is_empty() || value.is_empty())
            || self.flags.iter().any(String::is_empty)
            || self.flags.windows(2).any(|pair| pair[0] >= pair[1])
            || self
                .start_timestamp_ms
                .is_some_and(|start| start > self.timestamp_ms)
        {
            return Err(LoomError::invalid(
                "metric observation fields must be valid",
            ));
        }
        for exemplar in &self.exemplars {
            exemplar.validate()?;
            if exemplar.timestamp_ms > self.timestamp_ms {
                return Err(LoomError::invalid("metric exemplar timestamp is invalid"));
            }
        }
        match &self.value {
            MetricValue::Number(value) if !value.is_finite() => {
                return Err(LoomError::invalid("metric observation value is invalid"));
            }
            MetricValue::Histogram(histogram) => histogram.validate(histogram.buckets.len() - 1)?,
            _ => {}
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        loom_codec::encode(&Value::Array(vec![
            Value::Text(OBSERVATION_SCHEMA.into()),
            Value::Text(self.descriptor.to_string()),
            encode_pairs(&self.resource),
            encode_pairs(&self.scope),
            encode_pairs(&self.attributes),
            self.start_timestamp_ms
                .map(Value::Uint)
                .unwrap_or(Value::Null),
            Value::Uint(self.timestamp_ms),
            self.value.encode(),
            Value::Array(self.flags.iter().cloned().map(Value::Text).collect()),
            Value::Array(self.exemplars.iter().map(MetricExemplar::encode).collect()),
        ]))
        .map_err(|err| LoomError::invalid(format!("metric observation encoding failed: {err}")))
    }

    pub fn series_id(&self) -> Result<String> {
        let bytes = loom_codec::encode(&Value::Array(vec![
            encode_pairs(&self.resource),
            encode_pairs(&self.scope),
            encode_pairs(&self.attributes),
        ]))
        .map_err(|err| LoomError::invalid(format!("metric series encoding failed: {err}")))?;
        Ok(Digest::blake3(&bytes).to_hex())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let fields = array(loom_codec::decode(bytes).map_err(|err| {
            LoomError::corrupt(format!("invalid metric observation CBOR: {err}"))
        })?)?;
        if text(&fields[0])? != OBSERVATION_SCHEMA {
            return Err(LoomError::corrupt("invalid metric observation schema"));
        }
        if fields.len() == 5 {
            return Self::new(
                Digest::parse(text(&fields[1])?)
                    .map_err(|_| LoomError::corrupt("metric descriptor digest is invalid"))?,
                decode_pairs(fields[2].clone())?,
                u64_value(&fields[3])?,
                f64_value(fields[4].clone())?,
            );
        }
        if fields.len() == 7 {
            let start_timestamp_ms = match &fields[3] {
                Value::Null => None,
                value => Some(u64_value(value)?),
            };
            let exemplars = array(fields[6].clone())?
                .into_iter()
                .map(MetricExemplar::decode)
                .collect::<Result<Vec<_>>>()?;
            return Self::with_value(
                Digest::parse(text(&fields[1])?)
                    .map_err(|_| LoomError::corrupt("metric descriptor digest is invalid"))?,
                BTreeMap::new(),
                BTreeMap::new(),
                decode_pairs(fields[2].clone())?,
                start_timestamp_ms,
                u64_value(&fields[4])?,
                MetricValue::decode(fields[5].clone())?,
                exemplars,
            );
        }
        if fields.len() == 9 {
            let start_timestamp_ms = match &fields[5] {
                Value::Null => None,
                value => Some(u64_value(value)?),
            };
            let exemplars = array(fields[8].clone())?
                .into_iter()
                .map(MetricExemplar::decode)
                .collect::<Result<Vec<_>>>()?;
            return Self::with_value(
                Digest::parse(text(&fields[1])?)
                    .map_err(|_| LoomError::corrupt("metric descriptor digest is invalid"))?,
                decode_pairs(fields[2].clone())?,
                decode_pairs(fields[3].clone())?,
                decode_pairs(fields[4].clone())?,
                start_timestamp_ms,
                u64_value(&fields[6])?,
                MetricValue::decode(fields[7].clone())?,
                exemplars,
            );
        }
        if fields.len() != 10 {
            return Err(LoomError::corrupt("invalid metric observation schema"));
        }
        let start_timestamp_ms = match &fields[5] {
            Value::Null => None,
            value => Some(u64_value(value)?),
        };
        let flags = array(fields[8].clone())?
            .into_iter()
            .map(|value| text(&value).map(str::to_owned))
            .collect::<Result<Vec<_>>>()?;
        let exemplars = array(fields[9].clone())?
            .into_iter()
            .map(MetricExemplar::decode)
            .collect::<Result<Vec<_>>>()?;
        Self::with_value_and_flags(
            Digest::parse(text(&fields[1])?)
                .map_err(|_| LoomError::corrupt("metric descriptor digest is invalid"))?,
            decode_pairs(fields[2].clone())?,
            decode_pairs(fields[3].clone())?,
            decode_pairs(fields[4].clone())?,
            start_timestamp_ms,
            u64_value(&fields[6])?,
            MetricValue::decode(fields[7].clone())?,
            flags,
            exemplars,
        )
    }
}

fn invalid_pairs(values: &BTreeMap<String, String>) -> bool {
    values
        .iter()
        .any(|(key, value)| key.is_empty() || value.is_empty())
}

fn encode_pairs(values: &BTreeMap<String, String>) -> Value {
    Value::Array(
        values
            .iter()
            .map(|(key, value)| {
                Value::Array(vec![Value::Text(key.clone()), Value::Text(value.clone())])
            })
            .collect(),
    )
}

fn decode_pairs(value: Value) -> Result<BTreeMap<String, String>> {
    let mut pairs = BTreeMap::new();
    for pair in array(value)? {
        let pair = array(pair)?;
        if pair.len() != 2 {
            return Err(LoomError::corrupt("metric attribute pair is invalid"));
        }
        if pairs
            .insert(text(&pair[0])?.to_owned(), text(&pair[1])?.to_owned())
            .is_some()
        {
            return Err(LoomError::corrupt("metric attribute key is duplicated"));
        }
    }
    Ok(pairs)
}

fn array(value: Value) -> Result<Vec<Value>> {
    if let Value::Array(values) = value {
        Ok(values)
    } else {
        Err(LoomError::corrupt("metric record must be an array"))
    }
}

fn text(value: &Value) -> Result<&str> {
    if let Value::Text(value) = value {
        Ok(value)
    } else {
        Err(LoomError::corrupt("metric text field is invalid"))
    }
}

fn u64_value(value: &Value) -> Result<u64> {
    if let Value::Uint(value) = value {
        Ok(*value)
    } else {
        Err(LoomError::corrupt("metric unsigned field is invalid"))
    }
}

fn f64_value(value: Value) -> Result<f64> {
    match value {
        Value::Float(value) if value.is_finite() => Ok(value),
        _ => Err(LoomError::corrupt("metric float field is invalid")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn histogram_descriptor() -> MetricDescriptor {
        MetricDescriptor::with_policy(
            "request.duration".into(),
            "Request duration".into(),
            "ms".into(),
            InstrumentKind::Histogram,
            Temporality::Delta,
            vec!["route".into()],
            MetricDescriptorPolicy::new(
                BTreeMap::from([("route".into(), 128)]),
                64,
                30_000,
                DEFAULT_RETENTION_MS,
                MetricDistribution::explicit_histogram(vec![10.0, 100.0, 1_000.0], 2).unwrap(),
            )
            .with_rollup_profiles(vec![
                MetricRollupProfile::new(
                    "1m".into(),
                    60_000,
                    DEFAULT_RETENTION_MS * 7,
                    5_000,
                    MetricRollupAggregation::HistogramMerge,
                )
                .unwrap(),
            ]),
        )
        .unwrap()
    }

    #[test]
    fn descriptor_round_trips_canonical_bytes() {
        let descriptor = histogram_descriptor();
        let bytes = descriptor.encode().unwrap();
        assert_eq!(MetricDescriptor::decode(&bytes).unwrap(), descriptor);
        assert_eq!(descriptor.encode().unwrap(), bytes);
    }

    #[test]
    fn descriptor_rejects_duplicate_rollup_profiles() {
        let descriptor = MetricDescriptor::with_policy(
            "request.duration".into(),
            "Request duration".into(),
            "ms".into(),
            InstrumentKind::Histogram,
            Temporality::Delta,
            vec!["route".into()],
            MetricDescriptorPolicy::new(
                BTreeMap::new(),
                64,
                30_000,
                DEFAULT_RETENTION_MS,
                MetricDistribution::explicit_histogram(vec![10.0, 100.0], 2).unwrap(),
            )
            .with_rollup_profiles(vec![
                MetricRollupProfile::new(
                    "1m".into(),
                    60_000,
                    DEFAULT_RETENTION_MS * 7,
                    5_000,
                    MetricRollupAggregation::HistogramMerge,
                )
                .unwrap(),
                MetricRollupProfile::new(
                    "1m-copy".into(),
                    60_000,
                    DEFAULT_RETENTION_MS * 7,
                    5_000,
                    MetricRollupAggregation::HistogramMerge,
                )
                .unwrap(),
            ]),
        );
        assert!(descriptor.is_err());
    }

    #[test]
    fn rollup_identity_uses_half_open_membership() {
        let descriptor = histogram_descriptor();
        let observation = MetricObservation::with_value(
            descriptor.digest().unwrap(),
            BTreeMap::from([("service.name".into(), "api".into())]),
            BTreeMap::from([("telemetry.sdk.name".into(), "loom".into())]),
            BTreeMap::from([("route".into(), "/v1/items".into())]),
            Some(119_000),
            120_000,
            MetricValue::Histogram(MetricHistogram {
                buckets: vec![1, 2, 3, 4],
                count: 10,
                sum: 125.0,
            }),
            Vec::new(),
        )
        .unwrap();
        let id = descriptor.derived_rollup_id(&observation, "1m").unwrap();
        assert_eq!(id.window_start_ms, 60_000);
        assert_eq!(id.window_end_ms, 120_000);
        let same_id = descriptor.derived_rollup_id(&observation, "1m").unwrap();
        assert_eq!(id.digest().unwrap(), same_id.digest().unwrap());
        let next_window = MetricRollupWindow::for_timestamp(120_001, 60_000).unwrap();
        assert_eq!(next_window.start_ms, 120_000);
        assert_eq!(next_window.end_ms, 180_000);
        assert!(next_window.contains(120_001));
        assert!(!next_window.contains(120_000));
    }

    #[test]
    fn descriptor_rejects_unsorted_attributes() {
        assert!(
            MetricDescriptor::new(
                "requests".into(),
                String::new(),
                "1".into(),
                InstrumentKind::Counter,
                Temporality::Cumulative,
                vec!["zone".into(), "method".into()],
                1,
                0,
            )
            .is_err()
        );
    }

    #[test]
    fn descriptor_rejects_invalid_histogram_bounds() {
        assert!(
            MetricDescriptor::with_policy(
                "duration".into(),
                String::new(),
                "ms".into(),
                InstrumentKind::Histogram,
                Temporality::Delta,
                vec![],
                MetricDescriptorPolicy::new(
                    BTreeMap::new(),
                    1,
                    0,
                    DEFAULT_RETENTION_MS,
                    MetricDistribution {
                        explicit_bounds: vec![100.0, 10.0],
                        max_exemplars: 0,
                    },
                ),
            )
            .is_err()
        );
    }

    #[test]
    fn observation_rejects_non_finite_value() {
        assert!(
            MetricObservation::new(Digest::blake3(b"descriptor"), BTreeMap::new(), 1, f64::NAN,)
                .is_err()
        );
    }

    #[test]
    fn descriptor_rejects_negative_counter_observation() {
        let descriptor = MetricDescriptor::new(
            "requests".into(),
            String::new(),
            "1".into(),
            InstrumentKind::Counter,
            Temporality::Delta,
            vec![],
            1,
            0,
        )
        .unwrap();
        let observation =
            MetricObservation::new(descriptor.digest().unwrap(), BTreeMap::new(), 1, -1.0).unwrap();
        assert!(descriptor.validate_observation(&observation).is_err());
    }

    #[test]
    fn histogram_observation_validates_bucket_count() {
        let descriptor = histogram_descriptor();
        let observation = MetricObservation::with_value(
            descriptor.digest().unwrap(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::from([("route".into(), "/v1/items".into())]),
            Some(1),
            2,
            MetricValue::Histogram(MetricHistogram {
                buckets: vec![1, 2],
                count: 3,
                sum: 25.0,
            }),
            Vec::new(),
        )
        .unwrap();
        assert!(descriptor.validate_observation(&observation).is_err());
    }

    #[test]
    fn observation_decode_rejects_duplicate_attribute() {
        let bytes = loom_codec::encode(&Value::Array(vec![
            Value::Text(OBSERVATION_SCHEMA.into()),
            Value::Text(Digest::blake3(b"descriptor").to_string()),
            Value::Array(vec![
                Value::Array(vec![Value::Text("a".into()), Value::Text("x".into())]),
                Value::Array(vec![Value::Text("a".into()), Value::Text("y".into())]),
            ]),
            Value::Null,
            Value::Uint(1),
            Value::Array(vec![Value::Text("number".into()), Value::Float(1.0)]),
            Value::Array(vec![]),
        ]))
        .unwrap();
        assert!(MetricObservation::decode(&bytes).is_err());
    }
}
