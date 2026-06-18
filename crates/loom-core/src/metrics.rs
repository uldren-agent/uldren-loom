//! Persistent native metric descriptors.

use crate::acl::AclRight;
use crate::error::{Code, Result};
use crate::provider::ObjectStore;
use crate::vcs::Loom;
use crate::workspace::{FacetKind, WorkspaceId, facet_path, facet_root};
use loom_metrics::{
    MetricDerivedRollupId, MetricDescriptor, MetricExemplar, MetricHistogram,
    MetricMaterializedRollup, MetricObservation, MetricRollupAggregation, MetricRollupProfile,
    MetricRollupValue, MetricRollupWindow, MetricRollupWindowStatus, MetricValue,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub struct MetricQuery {
    pub from_timestamp_ms: u64,
    pub to_timestamp_ms: u64,
    pub max_series: u32,
    pub max_groups: u32,
    pub max_samples: u32,
    pub max_output_bytes: u64,
    pub now_timestamp_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricQueryResult {
    pub observations: Vec<MetricObservation>,
    pub partial: bool,
    pub stale: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricQueryTemporalSemantics {
    InclusiveStartExclusiveEnd,
    OpenStartClosedEnd,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricTieredQuery {
    pub from_timestamp_ms: u64,
    pub to_timestamp_ms: u64,
    pub resolution_ms: u64,
    pub aggregation: MetricRollupAggregation,
    pub temporal_semantics: MetricQueryTemporalSemantics,
    pub max_series: u32,
    pub max_groups: u32,
    pub max_samples: u32,
    pub max_output_bytes: u64,
    pub now_timestamp_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetricQueryTier {
    Raw,
    Rollup {
        profile_name: String,
        profile_id: String,
        period_ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricQueryPlan {
    pub tier: MetricQueryTier,
    pub partial: bool,
    pub stale: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetricRollupMaintenanceResult {
    pub written: u64,
    pub marked_stale: u64,
    pub compacted: u64,
}

fn descriptor_path(name: &str) -> Result<String> {
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return Err(crate::LoomError::invalid(
            "metric descriptor name is invalid",
        ));
    }
    Ok(facet_path(
        FacetKind::Metrics,
        &format!("descriptors/{name}"),
    ))
}

fn observation_dir(descriptor: &str) -> Result<String> {
    let descriptor = descriptor_path(descriptor)?;
    Ok(descriptor.replacen("descriptors/", "observations/", 1))
}

fn observation_series_dir(descriptor: &str, series_id: &str) -> Result<String> {
    if series_id.is_empty() || series_id.contains('/') {
        return Err(crate::LoomError::invalid("metric series id is invalid"));
    }
    Ok(format!("{}/{series_id}", observation_dir(descriptor)?))
}

fn observation_path(descriptor: &str, series_id: &str, timestamp_ms: u64) -> Result<String> {
    Ok(format!(
        "{}/{timestamp_ms}",
        observation_series_dir(descriptor, series_id)?
    ))
}

fn rollup_profile_dir(descriptor: &str, profile_id: &str) -> Result<String> {
    if profile_id.is_empty() || profile_id.contains('/') {
        return Err(crate::LoomError::invalid(
            "metric rollup profile id is invalid",
        ));
    }
    Ok(facet_path(
        FacetKind::Metrics,
        &format!("rollups/{descriptor}/{profile_id}"),
    ))
}

fn rollup_series_dir(descriptor: &str, profile_id: &str, series_id: &str) -> Result<String> {
    if series_id.is_empty() || series_id.contains('/') {
        return Err(crate::LoomError::invalid("metric series id is invalid"));
    }
    Ok(format!(
        "{}/{series_id}",
        rollup_profile_dir(descriptor, profile_id)?
    ))
}

fn rollup_path(
    descriptor: &str,
    profile_id: &str,
    series_id: &str,
    window_end_ms: u64,
) -> Result<String> {
    Ok(format!(
        "{}/{window_end_ms}",
        rollup_series_dir(descriptor, profile_id, series_id)?
    ))
}

fn redacted_observation(observation: &MetricObservation) -> Result<MetricObservation> {
    let exemplars = observation
        .exemplars
        .iter()
        .map(|exemplar| {
            MetricExemplar::new(
                redacted_pairs(&exemplar.attributes),
                exemplar.timestamp_ms,
                exemplar.value,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    MetricObservation::with_value_and_flags(
        observation.descriptor,
        redacted_pairs(&observation.resource),
        redacted_pairs(&observation.scope),
        redacted_pairs(&observation.attributes),
        observation.start_timestamp_ms,
        observation.timestamp_ms,
        observation.value.clone(),
        observation.flags.clone(),
        exemplars,
    )
}

fn redacted_pairs(values: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    values
        .iter()
        .map(|(key, value)| {
            if metric_redacts_key(key) {
                (key.clone(), "redacted".to_string())
            } else {
                (key.clone(), value.clone())
            }
        })
        .collect()
}

fn metric_redacts_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("password")
        || key.contains("passwd")
        || key.contains("secret")
        || key.contains("token")
        || key.contains("credential")
        || key.contains("authorization")
        || key.contains("cookie")
        || key.contains("session")
        || key.contains("api_key")
        || key.contains("apikey")
        || key == "path"
        || key.ends_with("_path")
        || key.ends_with(".path")
        || key == "url"
        || key.ends_with("_url")
        || key.ends_with(".url")
        || key == "uri"
        || key.ends_with("_uri")
        || key.ends_with(".uri")
        || key == "payload"
        || key.ends_with("_payload")
        || key.ends_with(".payload")
        || key == "body"
        || key.ends_with("_body")
        || key.ends_with(".body")
        || key == "error"
        || key.ends_with("_error")
        || key.ends_with(".error")
        || key == "message"
        || key.ends_with("_message")
        || key.ends_with(".message")
        || key == "stack"
        || key.ends_with("_stack")
        || key.ends_with(".stack")
}

fn profile_by_name<'a>(
    descriptor: &'a MetricDescriptor,
    profile_name: &str,
) -> Result<&'a MetricRollupProfile> {
    descriptor
        .rollup_profile(profile_name)
        .ok_or_else(|| crate::LoomError::not_found("metric rollup profile not found"))
}

fn read_rollup_at_path<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    path: &str,
) -> Result<Option<MetricMaterializedRollup>> {
    match loom.read_file_reserved(ns, path) {
        Ok(bytes) => MetricMaterializedRollup::decode(&bytes).map(Some),
        Err(error) if error.code == Code::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn observations_for_series_window<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    descriptor_name: &str,
    series_id: &str,
    window: &MetricRollupWindow,
) -> Result<Vec<MetricObservation>> {
    let series_dir = observation_series_dir(descriptor_name, series_id)?;
    let entries = match loom.list_directory(ns, &series_dir) {
        Ok(entries) => entries,
        Err(error) if error.code == Code::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut observations = Vec::new();
    for entry in entries {
        let timestamp_ms = entry
            .name
            .parse::<u64>()
            .map_err(|_| crate::LoomError::corrupt("metric observation timestamp is corrupt"))?;
        if !window.contains(timestamp_ms) {
            continue;
        }
        let path = observation_path(descriptor_name, series_id, timestamp_ms)?;
        observations.push(MetricObservation::decode(
            &loom.read_file_reserved(ns, &path)?,
        )?);
    }
    observations.sort_by_key(|observation| observation.timestamp_ms);
    Ok(observations)
}

fn max_observation_timestamp<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    descriptor_name: &str,
) -> Result<Option<u64>> {
    let dir = observation_dir(descriptor_name)?;
    let series_entries = match loom.list_directory(ns, &dir) {
        Ok(entries) => entries,
        Err(error) if error.code == Code::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut max_timestamp: Option<u64> = None;
    for series in series_entries {
        let series_dir = observation_series_dir(descriptor_name, &series.name)?;
        for entry in loom.list_directory(ns, &series_dir)? {
            let timestamp_ms = entry.name.parse::<u64>().map_err(|_| {
                crate::LoomError::corrupt("metric observation timestamp is corrupt")
            })?;
            max_timestamp =
                Some(max_timestamp.map_or(timestamp_ms, |value| value.max(timestamp_ms)));
        }
    }
    Ok(max_timestamp)
}

fn observation_series_entries<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    descriptor_name: &str,
) -> Result<Vec<crate::fs::DirEntry>> {
    match loom.list_directory(ns, &observation_dir(descriptor_name)?) {
        Ok(entries) => Ok(entries),
        Err(error) if error.code == Code::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn aligned_rollup_window_ends(
    from_timestamp_ms: u64,
    to_timestamp_ms: u64,
    period_ms: u64,
) -> Vec<u64> {
    if period_ms == 0
        || from_timestamp_ms >= to_timestamp_ms
        || !from_timestamp_ms.is_multiple_of(period_ms)
        || !to_timestamp_ms.is_multiple_of(period_ms)
    {
        return Vec::new();
    }
    let mut window_end_ms = from_timestamp_ms.saturating_add(period_ms);
    let mut windows = Vec::new();
    while window_end_ms <= to_timestamp_ms {
        windows.push(window_end_ms);
        window_end_ms = window_end_ms.saturating_add(period_ms);
    }
    windows
}

fn aggregate_rollup(
    profile: &MetricRollupProfile,
    observations: &[MetricObservation],
) -> Result<MetricRollupValue> {
    match profile.aggregation {
        MetricRollupAggregation::Sum => {
            let mut sum = 0.0;
            for observation in observations {
                let MetricValue::Number(value) = &observation.value else {
                    return Err(crate::LoomError::invalid(
                        "metric rollup source value is invalid",
                    ));
                };
                sum += *value;
            }
            Ok(MetricRollupValue::Number(sum))
        }
        MetricRollupAggregation::Last => {
            let Some(observation) = observations.last() else {
                return Err(crate::LoomError::invalid("metric rollup has no samples"));
            };
            let MetricValue::Number(value) = &observation.value else {
                return Err(crate::LoomError::invalid(
                    "metric rollup source value is invalid",
                ));
            };
            Ok(MetricRollupValue::Number(*value))
        }
        MetricRollupAggregation::MinMaxAvg => {
            let mut min = f64::INFINITY;
            let mut max = f64::NEG_INFINITY;
            let mut sum = 0.0;
            for observation in observations {
                let MetricValue::Number(value) = &observation.value else {
                    return Err(crate::LoomError::invalid(
                        "metric rollup source value is invalid",
                    ));
                };
                min = min.min(*value);
                max = max.max(*value);
                sum += *value;
            }
            Ok(MetricRollupValue::MinMaxAvg {
                min,
                max,
                avg: sum / observations.len() as f64,
            })
        }
        MetricRollupAggregation::HistogramMerge => {
            let mut buckets: Vec<u64> = Vec::new();
            let mut count = 0_u64;
            let mut sum = 0.0;
            for observation in observations {
                let MetricValue::Histogram(histogram) = &observation.value else {
                    return Err(crate::LoomError::invalid(
                        "metric rollup source value is invalid",
                    ));
                };
                if buckets.is_empty() {
                    buckets = vec![0; histogram.buckets.len()];
                }
                if buckets.len() != histogram.buckets.len() {
                    return Err(crate::LoomError::invalid(
                        "metric rollup histogram layout is invalid",
                    ));
                }
                for (target, value) in buckets.iter_mut().zip(&histogram.buckets) {
                    *target = (*target).saturating_add(*value);
                }
                count = count.saturating_add(histogram.count);
                sum += histogram.sum;
            }
            Ok(MetricRollupValue::Histogram(MetricHistogram::new(
                buckets, count, sum,
            )?))
        }
    }
}

fn materialize_rollup_window<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    descriptor_name: &str,
    descriptor: &MetricDescriptor,
    profile: &MetricRollupProfile,
    series_id: &str,
    window: &MetricRollupWindow,
    max_timestamp_ms: u64,
) -> Result<bool> {
    let observations =
        observations_for_series_window(loom, ns, descriptor_name, series_id, window)?;
    if observations.is_empty() {
        return Ok(false);
    }
    let profile_id = profile.profile_id()?;
    let id = MetricDerivedRollupId::new(
        descriptor.digest()?,
        series_id.to_string(),
        profile_id.clone(),
        window.clone(),
    )?;
    let watermark_ms = max_timestamp_ms.saturating_sub(profile.lateness_ms);
    let status = if window.end_ms <= watermark_ms {
        MetricRollupWindowStatus::Final
    } else {
        MetricRollupWindowStatus::Partial
    };
    let source_start_ms = observations
        .first()
        .map(|observation| observation.timestamp_ms)
        .ok_or_else(|| crate::LoomError::invalid("metric rollup has no samples"))?;
    let source_end_ms = observations
        .last()
        .map(|observation| observation.timestamp_ms)
        .ok_or_else(|| crate::LoomError::invalid("metric rollup has no samples"))?;
    let rollup = MetricMaterializedRollup {
        id,
        profile_name: profile.name.clone(),
        aggregation: profile.aggregation,
        status,
        sample_count: observations.len() as u64,
        value: aggregate_rollup(profile, &observations)?,
        source_start_ms,
        source_end_ms,
        watermark_ms,
    };
    rollup.validate()?;
    let profile_dir = rollup_profile_dir(descriptor_name, &profile_id)?;
    let series_dir = rollup_series_dir(descriptor_name, &profile_id, series_id)?;
    let path = rollup_path(descriptor_name, &profile_id, series_id, window.end_ms)?;
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Metrics), true)?;
    loom.create_directory_reserved(ns, &facet_path(FacetKind::Metrics, "rollups"), true)?;
    loom.create_directory_reserved(
        ns,
        &facet_path(FacetKind::Metrics, &format!("rollups/{descriptor_name}")),
        true,
    )?;
    loom.create_directory_reserved(ns, &profile_dir, true)?;
    loom.create_directory_reserved(ns, &series_dir, true)?;
    loom.write_file_reserved(ns, &path, &rollup.encode()?, 0o100644)?;
    Ok(true)
}

pub fn metrics_put_descriptor<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    descriptor: &MetricDescriptor,
) -> Result<()> {
    descriptor.validate()?;
    let path = descriptor_path(&descriptor.name)?;
    loom.authorize_facet_path(ns, FacetKind::Metrics, &path, AclRight::Write)?;
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Metrics), true)?;
    loom.create_directory_reserved(ns, &facet_path(FacetKind::Metrics, "descriptors"), true)?;
    loom.write_file_reserved(ns, &path, &descriptor.encode()?, 0o100644)
}

pub fn metrics_get_descriptor<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<Option<MetricDescriptor>> {
    let path = descriptor_path(name)?;
    loom.authorize_facet_path(ns, FacetKind::Metrics, &path, AclRight::Read)?;
    match loom.read_file_reserved(ns, &path) {
        Ok(bytes) => MetricDescriptor::decode(&bytes).map(Some),
        Err(error) if error.code == Code::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn metrics_put_observation<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    descriptor_name: &str,
    observation: &MetricObservation,
) -> Result<()> {
    let descriptor = metrics_get_descriptor(loom, ns, descriptor_name)?
        .ok_or_else(|| crate::LoomError::not_found("metric descriptor not found"))?;
    let observation = redacted_observation(observation)?;
    descriptor.validate_observation(&observation)?;
    let series_id = observation.series_id()?;
    let dir = observation_dir(descriptor_name)?;
    let series_dir = observation_series_dir(descriptor_name, &series_id)?;
    let path = observation_path(descriptor_name, &series_id, observation.timestamp_ms)?;
    loom.authorize_facet_path(ns, FacetKind::Metrics, &path, AclRight::Write)?;
    let active_series = match loom.list_directory(ns, &dir) {
        Ok(entries) => entries.len(),
        Err(error) if error.code == Code::NotFound => 0,
        Err(error) => return Err(error),
    };
    if active_series >= descriptor.max_active_series as usize
        && loom.list_directory(ns, &series_dir).is_err()
    {
        return Err(crate::LoomError::new(
            Code::ResourceExhausted,
            "metric active series limit exceeded",
        ));
    }
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Metrics), true)?;
    loom.create_directory_reserved(ns, &facet_path(FacetKind::Metrics, "observations"), true)?;
    loom.create_directory_reserved(
        ns,
        &facet_path(
            FacetKind::Metrics,
            &format!("observations/{descriptor_name}"),
        ),
        true,
    )?;
    loom.create_directory_reserved(ns, &series_dir, true)?;
    loom.write_file_reserved(ns, &path, &observation.encode()?, 0o100644)?;
    refresh_rollups_for_observation(loom, ns, descriptor_name, &descriptor, &observation)
}

fn refresh_rollups_for_observation<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    descriptor_name: &str,
    descriptor: &MetricDescriptor,
    observation: &MetricObservation,
) -> Result<()> {
    if descriptor.rollup_profiles.is_empty() {
        return Ok(());
    }
    let series_id = observation.series_id()?;
    let max_timestamp_ms = max_observation_timestamp(loom, ns, descriptor_name)?
        .ok_or_else(|| crate::LoomError::not_found("metric observation not found"))?;
    for profile in &descriptor.rollup_profiles {
        let window =
            MetricRollupWindow::for_timestamp(observation.timestamp_ms, profile.period_ms)?;
        let profile_id = profile.profile_id()?;
        let path = rollup_path(descriptor_name, &profile_id, &series_id, window.end_ms)?;
        if let Some(existing) = read_rollup_at_path(loom, ns, &path)?
            && existing.status == MetricRollupWindowStatus::Final
            && observation.timestamp_ms <= existing.watermark_ms
        {
            let stale = existing.with_status(MetricRollupWindowStatus::Stale)?;
            loom.write_file_reserved(ns, &path, &stale.encode()?, 0o100644)?;
            continue;
        }
        materialize_rollup_window(
            loom,
            ns,
            descriptor_name,
            descriptor,
            profile,
            &series_id,
            &window,
            max_timestamp_ms,
        )?;
    }
    Ok(())
}

pub fn metrics_get_observation<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    descriptor_name: &str,
    timestamp_ms: u64,
) -> Result<Option<MetricObservation>> {
    let dir = observation_dir(descriptor_name)?;
    loom.authorize_facet_path(ns, FacetKind::Metrics, &dir, AclRight::Read)?;
    let entries = match loom.list_directory(ns, &dir) {
        Ok(entries) => entries,
        Err(error) if error.code == Code::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let path = observation_path(descriptor_name, &entry.name, timestamp_ms)?;
        match loom.read_file_reserved(ns, &path) {
            Ok(bytes) => return MetricObservation::decode(&bytes).map(Some),
            Err(error) if error.code == Code::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

pub fn metrics_query_observations<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    descriptor_name: &str,
    query: &MetricQuery,
) -> Result<MetricQueryResult> {
    if query.from_timestamp_ms >= query.to_timestamp_ms
        || query.max_series == 0
        || query.max_groups == 0
        || query.max_samples == 0
        || query.max_output_bytes == 0
    {
        return Err(crate::LoomError::invalid("metric query bounds are invalid"));
    }
    let descriptor = metrics_get_descriptor(loom, ns, descriptor_name)?
        .ok_or_else(|| crate::LoomError::not_found("metric descriptor not found"))?;
    let dir = observation_dir(descriptor_name)?;
    loom.authorize_facet_path(ns, FacetKind::Metrics, &dir, AclRight::Read)?;
    let series_entries = match loom.list_directory(ns, &dir) {
        Ok(entries) => entries,
        Err(error) if error.code == Code::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    let mut observations = Vec::new();
    let mut groups = BTreeSet::new();
    let mut output_bytes = 0_u64;
    let mut partial = false;
    let mut stale = false;
    for (series_count, series) in series_entries.into_iter().enumerate() {
        if series_count >= query.max_series as usize {
            partial = true;
            break;
        }
        let series_dir = observation_series_dir(descriptor_name, &series.name)?;
        for entry in loom.list_directory(ns, &series_dir)? {
            let Ok(timestamp_ms) = entry.name.parse::<u64>() else {
                return Err(crate::LoomError::corrupt(
                    "metric observation timestamp is corrupt",
                ));
            };
            if timestamp_ms < query.from_timestamp_ms || timestamp_ms >= query.to_timestamp_ms {
                continue;
            }
            if query.now_timestamp_ms.saturating_sub(timestamp_ms) > descriptor.retention_ms {
                continue;
            }
            let path = observation_path(descriptor_name, &series.name, timestamp_ms)?;
            let observation = MetricObservation::decode(&loom.read_file_reserved(ns, &path)?)?;
            if descriptor.stale_after_ms > 0
                && query.now_timestamp_ms.saturating_sub(timestamp_ms) > descriptor.stale_after_ms
            {
                stale = true;
            }
            if !groups.contains(&series.name) && groups.len() >= query.max_groups as usize {
                partial = true;
                return Ok(MetricQueryResult {
                    observations,
                    partial,
                    stale,
                });
            }
            let encoded_len = observation.encode()?.len() as u64;
            if output_bytes.saturating_add(encoded_len) > query.max_output_bytes {
                partial = true;
                return Ok(MetricQueryResult {
                    observations,
                    partial,
                    stale,
                });
            }
            output_bytes = output_bytes.saturating_add(encoded_len);
            groups.insert(series.name.clone());
            observations.push(observation);
            if observations.len() >= query.max_samples as usize {
                partial = true;
                return Ok(MetricQueryResult {
                    observations,
                    partial,
                    stale,
                });
            }
        }
    }
    Ok(MetricQueryResult {
        observations,
        partial,
        stale,
    })
}

pub fn metrics_plan_query<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    descriptor_name: &str,
    query: &MetricTieredQuery,
) -> Result<MetricQueryPlan> {
    if query.from_timestamp_ms >= query.to_timestamp_ms
        || query.resolution_ms == 0
        || query.max_series == 0
        || query.max_groups == 0
        || query.max_samples == 0
        || query.max_output_bytes == 0
    {
        return Err(crate::LoomError::invalid("metric query bounds are invalid"));
    }
    let descriptor = metrics_get_descriptor(loom, ns, descriptor_name)?
        .ok_or_else(|| crate::LoomError::not_found("metric descriptor not found"))?;
    let dir = observation_dir(descriptor_name)?;
    loom.authorize_facet_path(ns, FacetKind::Metrics, &dir, AclRight::Read)?;
    if query.temporal_semantics != MetricQueryTemporalSemantics::OpenStartClosedEnd {
        return Ok(MetricQueryPlan {
            tier: MetricQueryTier::Raw,
            partial: false,
            stale: false,
        });
    }
    let Some(profile) = descriptor.rollup_profiles.iter().find(|profile| {
        profile.period_ms == query.resolution_ms && profile.aggregation == query.aggregation
    }) else {
        return Ok(MetricQueryPlan {
            tier: MetricQueryTier::Raw,
            partial: false,
            stale: false,
        });
    };
    let window_ends = aligned_rollup_window_ends(
        query.from_timestamp_ms,
        query.to_timestamp_ms,
        profile.period_ms,
    );
    if window_ends.is_empty() {
        return Ok(MetricQueryPlan {
            tier: MetricQueryTier::Raw,
            partial: false,
            stale: false,
        });
    }
    if query.now_timestamp_ms.saturating_sub(query.to_timestamp_ms) > profile.retention_ms {
        return Ok(MetricQueryPlan {
            tier: MetricQueryTier::Raw,
            partial: false,
            stale: false,
        });
    }
    let series_entries = observation_series_entries(loom, ns, descriptor_name)?;
    if series_entries.len() > query.max_series as usize
        || series_entries.len() > query.max_groups as usize
        || series_entries.len().saturating_mul(window_ends.len()) > query.max_samples as usize
    {
        return Ok(MetricQueryPlan {
            tier: MetricQueryTier::Raw,
            partial: true,
            stale: false,
        });
    }
    let profile_id = profile.profile_id()?;
    let mut output_bytes = 0_u64;
    let mut stale = false;
    for series in series_entries {
        for window_end_ms in &window_ends {
            let path = rollup_path(descriptor_name, &profile_id, &series.name, *window_end_ms)?;
            loom.authorize_facet_path(ns, FacetKind::Metrics, &path, AclRight::Read)?;
            let Some(rollup) = read_rollup_at_path(loom, ns, &path)? else {
                return Ok(MetricQueryPlan {
                    tier: MetricQueryTier::Raw,
                    partial: false,
                    stale,
                });
            };
            if rollup.status != MetricRollupWindowStatus::Final {
                stale |= rollup.status == MetricRollupWindowStatus::Stale;
                return Ok(MetricQueryPlan {
                    tier: MetricQueryTier::Raw,
                    partial: false,
                    stale,
                });
            }
            if query
                .now_timestamp_ms
                .saturating_sub(rollup.id.window_end_ms)
                > profile.retention_ms
            {
                return Ok(MetricQueryPlan {
                    tier: MetricQueryTier::Raw,
                    partial: false,
                    stale,
                });
            }
            output_bytes = output_bytes.saturating_add(rollup.encode()?.len() as u64);
            if output_bytes > query.max_output_bytes {
                return Ok(MetricQueryPlan {
                    tier: MetricQueryTier::Raw,
                    partial: true,
                    stale,
                });
            }
        }
    }
    Ok(MetricQueryPlan {
        tier: MetricQueryTier::Rollup {
            profile_name: profile.name.clone(),
            profile_id,
            period_ms: profile.period_ms,
        },
        partial: false,
        stale,
    })
}

pub fn metrics_get_rollup<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    descriptor_name: &str,
    profile_name: &str,
    series_id: &str,
    window_end_ms: u64,
) -> Result<Option<MetricMaterializedRollup>> {
    let descriptor = metrics_get_descriptor(loom, ns, descriptor_name)?
        .ok_or_else(|| crate::LoomError::not_found("metric descriptor not found"))?;
    let profile = profile_by_name(&descriptor, profile_name)?;
    let profile_id = profile.profile_id()?;
    let path = rollup_path(descriptor_name, &profile_id, series_id, window_end_ms)?;
    loom.authorize_facet_path(ns, FacetKind::Metrics, &path, AclRight::Read)?;
    read_rollup_at_path(loom, ns, &path)
}

pub fn metrics_materialize_rollups<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    descriptor_name: &str,
) -> Result<MetricRollupMaintenanceResult> {
    metrics_materialize_rollups_in_range(loom, ns, descriptor_name, None)
}

pub fn metrics_rebuild_rollups<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    descriptor_name: &str,
    from_timestamp_ms: u64,
    to_timestamp_ms: u64,
) -> Result<MetricRollupMaintenanceResult> {
    if from_timestamp_ms >= to_timestamp_ms {
        return Err(crate::LoomError::invalid(
            "metric rollup rebuild range is invalid",
        ));
    }
    metrics_materialize_rollups_in_range(
        loom,
        ns,
        descriptor_name,
        Some((from_timestamp_ms, to_timestamp_ms)),
    )
}

fn metrics_materialize_rollups_in_range<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    descriptor_name: &str,
    range: Option<(u64, u64)>,
) -> Result<MetricRollupMaintenanceResult> {
    let descriptor = metrics_get_descriptor(loom, ns, descriptor_name)?
        .ok_or_else(|| crate::LoomError::not_found("metric descriptor not found"))?;
    let dir = observation_dir(descriptor_name)?;
    loom.authorize_facet_path(ns, FacetKind::Metrics, &dir, AclRight::Write)?;
    let max_timestamp_ms = match max_observation_timestamp(loom, ns, descriptor_name)? {
        Some(timestamp) => timestamp,
        None => return Ok(MetricRollupMaintenanceResult::default()),
    };
    let series_entries = match loom.list_directory(ns, &dir) {
        Ok(entries) => entries,
        Err(error) if error.code == Code::NotFound => {
            return Ok(MetricRollupMaintenanceResult::default());
        }
        Err(error) => return Err(error),
    };
    let mut result = MetricRollupMaintenanceResult::default();
    let mut visited = BTreeSet::new();
    for profile in &descriptor.rollup_profiles {
        for series in &series_entries {
            let series_dir = observation_series_dir(descriptor_name, &series.name)?;
            for entry in loom.list_directory(ns, &series_dir)? {
                let timestamp_ms = entry.name.parse::<u64>().map_err(|_| {
                    crate::LoomError::corrupt("metric observation timestamp is corrupt")
                })?;
                let window = MetricRollupWindow::for_timestamp(timestamp_ms, profile.period_ms)?;
                if let Some((from, to)) = range
                    && (window.end_ms <= from || window.start_ms >= to)
                {
                    continue;
                }
                if !visited.insert((profile.name.clone(), series.name.clone(), window.end_ms)) {
                    continue;
                }
                if materialize_rollup_window(
                    loom,
                    ns,
                    descriptor_name,
                    &descriptor,
                    profile,
                    &series.name,
                    &window,
                    max_timestamp_ms,
                )? {
                    result.written = result.written.saturating_add(1);
                }
            }
        }
    }
    Ok(result)
}

pub fn metrics_compact_rollups<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    descriptor_name: &str,
    now_timestamp_ms: u64,
) -> Result<MetricRollupMaintenanceResult> {
    let descriptor = metrics_get_descriptor(loom, ns, descriptor_name)?
        .ok_or_else(|| crate::LoomError::not_found("metric descriptor not found"))?;
    let mut result = MetricRollupMaintenanceResult::default();
    for profile in &descriptor.rollup_profiles {
        let profile_id = profile.profile_id()?;
        let profile_dir = rollup_profile_dir(descriptor_name, &profile_id)?;
        loom.authorize_facet_path(ns, FacetKind::Metrics, &profile_dir, AclRight::Write)?;
        let series_entries = match loom.list_directory(ns, &profile_dir) {
            Ok(entries) => entries,
            Err(error) if error.code == Code::NotFound => continue,
            Err(error) => return Err(error),
        };
        for series in series_entries {
            let series_dir = rollup_series_dir(descriptor_name, &profile_id, &series.name)?;
            for entry in loom.list_directory(ns, &series_dir)? {
                let window_end_ms = entry
                    .name
                    .parse::<u64>()
                    .map_err(|_| crate::LoomError::corrupt("metric rollup window is corrupt"))?;
                if now_timestamp_ms.saturating_sub(window_end_ms) <= profile.retention_ms {
                    continue;
                }
                loom.remove_file_reserved(
                    ns,
                    &rollup_path(descriptor_name, &profile_id, &series.name, window_end_ms)?,
                )?;
                result.compacted = result.compacted.saturating_add(1);
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::memory::MemoryStore;
    use loom_metrics::{
        DEFAULT_RETENTION_MS, InstrumentKind, MetricDescriptorPolicy, MetricDistribution,
        MetricExemplar, MetricRollupAggregation, MetricRollupProfile, MetricRollupValue,
        MetricRollupWindowStatus, Temporality,
    };
    use std::collections::BTreeMap;

    #[test]
    fn descriptor_round_trips_through_the_metrics_facet() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Metrics, None, WorkspaceId::from_bytes([5; 16]))
            .unwrap();
        let descriptor = MetricDescriptor::new(
            "requests".into(),
            String::new(),
            "1".into(),
            InstrumentKind::Counter,
            Temporality::Cumulative,
            vec!["method".into()],
            64,
            30_000,
        )
        .unwrap();
        metrics_put_descriptor(&mut loom, ns, &descriptor).unwrap();
        assert_eq!(
            metrics_get_descriptor(&loom, ns, "requests").unwrap(),
            Some(descriptor)
        );
    }

    #[test]
    fn observation_requires_a_descriptor_and_round_trips() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Metrics, None, WorkspaceId::from_bytes([6; 16]))
            .unwrap();
        let descriptor = MetricDescriptor::new(
            "requests".into(),
            String::new(),
            "1".into(),
            InstrumentKind::Counter,
            Temporality::Cumulative,
            vec!["method".into()],
            64,
            30_000,
        )
        .unwrap();
        metrics_put_descriptor(&mut loom, ns, &descriptor).unwrap();
        let observation = MetricObservation::new(
            descriptor.digest().unwrap(),
            BTreeMap::from([("method".into(), "GET".into())]),
            1,
            1.0,
        )
        .unwrap();
        metrics_put_observation(&mut loom, ns, "requests", &observation).unwrap();
        assert_eq!(
            metrics_get_observation(&loom, ns, "requests", 1).unwrap(),
            Some(observation)
        );
    }

    #[test]
    fn observation_redacts_sensitive_fields_before_persistence() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Metrics, None, WorkspaceId::from_bytes([8; 16]))
            .unwrap();
        let descriptor = MetricDescriptor::with_policy(
            "requests".into(),
            String::new(),
            "1".into(),
            InstrumentKind::Counter,
            Temporality::Cumulative,
            vec![
                "authorization".into(),
                "method".into(),
                "request_path".into(),
            ],
            MetricDescriptorPolicy::new(
                BTreeMap::new(),
                64,
                30_000,
                DEFAULT_RETENTION_MS,
                MetricDistribution::explicit_histogram(Vec::new(), 1).unwrap(),
            ),
        )
        .unwrap();
        metrics_put_descriptor(&mut loom, ns, &descriptor).unwrap();
        let observation = MetricObservation::with_value_and_flags(
            descriptor.digest().unwrap(),
            BTreeMap::from([("service.name".into(), "api".into())]),
            BTreeMap::from([("telemetry.sdk.name".into(), "loom".into())]),
            BTreeMap::from([
                ("authorization".into(), "Bearer secret-token".into()),
                ("method".into(), "GET".into()),
                ("request_path".into(), "/users/123/profile".into()),
            ]),
            None,
            1,
            loom_metrics::MetricValue::Number(1.0),
            vec!["partial".into()],
            vec![
                MetricExemplar::new(BTreeMap::from([("token".into(), "abc".into())]), 1, 1.0)
                    .unwrap(),
            ],
        )
        .unwrap();
        metrics_put_observation(&mut loom, ns, "requests", &observation).unwrap();
        let stored = metrics_get_observation(&loom, ns, "requests", 1)
            .unwrap()
            .unwrap();
        assert_eq!(
            stored.resource.get("service.name"),
            Some(&"api".to_string())
        );
        assert_eq!(
            stored.scope.get("telemetry.sdk.name"),
            Some(&"loom".to_string())
        );
        assert_eq!(
            stored.attributes.get("authorization"),
            Some(&"redacted".to_string())
        );
        assert_eq!(
            stored.attributes.get("request_path"),
            Some(&"redacted".to_string())
        );
        assert_eq!(stored.attributes.get("method"), Some(&"GET".to_string()));
        assert_eq!(stored.flags, vec!["partial".to_string()]);
        assert_eq!(
            stored.exemplars[0].attributes.get("token"),
            Some(&"redacted".to_string())
        );
    }

    #[test]
    fn observation_series_are_bounded_and_query_is_limited() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Metrics, None, WorkspaceId::from_bytes([7; 16]))
            .unwrap();
        let descriptor = MetricDescriptor::new(
            "requests".into(),
            String::new(),
            "1".into(),
            InstrumentKind::Counter,
            Temporality::Cumulative,
            vec!["method".into()],
            1,
            10,
        )
        .unwrap();
        metrics_put_descriptor(&mut loom, ns, &descriptor).unwrap();
        let get = MetricObservation::new(
            descriptor.digest().unwrap(),
            BTreeMap::from([("method".into(), "GET".into())]),
            1,
            1.0,
        )
        .unwrap();
        let post = MetricObservation::new(
            descriptor.digest().unwrap(),
            BTreeMap::from([("method".into(), "POST".into())]),
            2,
            1.0,
        )
        .unwrap();
        metrics_put_observation(&mut loom, ns, "requests", &get).unwrap();
        assert_eq!(
            metrics_put_observation(&mut loom, ns, "requests", &post)
                .unwrap_err()
                .code,
            Code::ResourceExhausted
        );
        let result = metrics_query_observations(
            &loom,
            ns,
            "requests",
            &MetricQuery {
                from_timestamp_ms: 0,
                to_timestamp_ms: 10,
                max_series: 1,
                max_groups: 1,
                max_samples: 1,
                max_output_bytes: 1024,
                now_timestamp_ms: 20,
            },
        )
        .unwrap();
        assert_eq!(result.observations, vec![get]);
        assert!(result.partial);
        assert!(result.stale);
        let result = metrics_query_observations(
            &loom,
            ns,
            "requests",
            &MetricQuery {
                from_timestamp_ms: 0,
                to_timestamp_ms: 10,
                max_series: 1,
                max_groups: 1,
                max_samples: 8,
                max_output_bytes: 1,
                now_timestamp_ms: 20,
            },
        )
        .unwrap();
        assert!(result.observations.is_empty());
        assert!(result.partial);
    }

    #[test]
    fn query_limits_returned_groups() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Metrics, None, WorkspaceId::from_bytes([9; 16]))
            .unwrap();
        let descriptor = MetricDescriptor::new(
            "requests".into(),
            String::new(),
            "1".into(),
            InstrumentKind::Counter,
            Temporality::Cumulative,
            vec!["method".into()],
            4,
            30_000,
        )
        .unwrap();
        metrics_put_descriptor(&mut loom, ns, &descriptor).unwrap();
        let get = MetricObservation::new(
            descriptor.digest().unwrap(),
            BTreeMap::from([("method".into(), "GET".into())]),
            1,
            1.0,
        )
        .unwrap();
        let post = MetricObservation::new(
            descriptor.digest().unwrap(),
            BTreeMap::from([("method".into(), "POST".into())]),
            2,
            1.0,
        )
        .unwrap();
        metrics_put_observation(&mut loom, ns, "requests", &get).unwrap();
        metrics_put_observation(&mut loom, ns, "requests", &post).unwrap();
        let result = metrics_query_observations(
            &loom,
            ns,
            "requests",
            &MetricQuery {
                from_timestamp_ms: 0,
                to_timestamp_ms: 10,
                max_series: 4,
                max_groups: 1,
                max_samples: 8,
                max_output_bytes: 1024,
                now_timestamp_ms: 20,
            },
        )
        .unwrap();
        assert_eq!(result.observations.len(), 1);
        assert!(result.partial);
    }

    #[test]
    fn rollups_materialize_mark_stale_rebuild_and_compact() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Metrics, None, WorkspaceId::from_bytes([10; 16]))
            .unwrap();
        let descriptor = MetricDescriptor::with_policy(
            "requests".into(),
            String::new(),
            "1".into(),
            InstrumentKind::Counter,
            Temporality::Cumulative,
            vec!["method".into()],
            MetricDescriptorPolicy::new(
                BTreeMap::new(),
                4,
                30_000,
                10,
                MetricDistribution::default(),
            )
            .with_rollup_profiles(vec![
                MetricRollupProfile::new("10ms".into(), 10, 10, 2, MetricRollupAggregation::Sum)
                    .unwrap(),
            ]),
        )
        .unwrap();
        metrics_put_descriptor(&mut loom, ns, &descriptor).unwrap();
        let first = MetricObservation::new(
            descriptor.digest().unwrap(),
            BTreeMap::from([("method".into(), "GET".into())]),
            10,
            1.0,
        )
        .unwrap();
        let second = MetricObservation::new(
            descriptor.digest().unwrap(),
            BTreeMap::from([("method".into(), "GET".into())]),
            20,
            2.0,
        )
        .unwrap();
        metrics_put_observation(&mut loom, ns, "requests", &first).unwrap();
        metrics_put_observation(&mut loom, ns, "requests", &second).unwrap();
        let series_id = first.series_id().unwrap();
        let result = metrics_materialize_rollups(&mut loom, ns, "requests").unwrap();
        assert_eq!(result.written, 2);
        let rollup = metrics_get_rollup(&loom, ns, "requests", "10ms", &series_id, 10)
            .unwrap()
            .unwrap();
        assert_eq!(rollup.status, MetricRollupWindowStatus::Final);
        assert_eq!(rollup.sample_count, 1);
        assert_eq!(rollup.value, MetricRollupValue::Number(1.0));
        let duplicate = metrics_materialize_rollups(&mut loom, ns, "requests").unwrap();
        assert_eq!(duplicate.written, 2);
        assert_eq!(
            metrics_get_rollup(&loom, ns, "requests", "10ms", &series_id, 10).unwrap(),
            Some(rollup)
        );

        let late = MetricObservation::new(
            descriptor.digest().unwrap(),
            BTreeMap::from([("method".into(), "GET".into())]),
            9,
            3.0,
        )
        .unwrap();
        metrics_put_observation(&mut loom, ns, "requests", &late).unwrap();
        let stale = metrics_get_rollup(&loom, ns, "requests", "10ms", &series_id, 10)
            .unwrap()
            .unwrap();
        assert_eq!(stale.status, MetricRollupWindowStatus::Stale);
        assert_eq!(stale.value, MetricRollupValue::Number(1.0));

        let rebuilt = metrics_rebuild_rollups(&mut loom, ns, "requests", 0, 10).unwrap();
        assert_eq!(rebuilt.written, 1);
        let rebuilt = metrics_get_rollup(&loom, ns, "requests", "10ms", &series_id, 10)
            .unwrap()
            .unwrap();
        assert_eq!(rebuilt.status, MetricRollupWindowStatus::Final);
        assert_eq!(rebuilt.sample_count, 2);
        assert_eq!(rebuilt.value, MetricRollupValue::Number(4.0));

        let compacted = metrics_compact_rollups(&mut loom, ns, "requests", 25).unwrap();
        assert_eq!(compacted.compacted, 1);
        assert!(
            metrics_get_rollup(&loom, ns, "requests", "10ms", &series_id, 10)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn query_planning_selects_rollups_only_when_equivalent() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Metrics, None, WorkspaceId::from_bytes([11; 16]))
            .unwrap();
        let descriptor = MetricDescriptor::with_policy(
            "requests".into(),
            String::new(),
            "1".into(),
            InstrumentKind::Counter,
            Temporality::Cumulative,
            vec!["method".into()],
            MetricDescriptorPolicy::new(
                BTreeMap::new(),
                4,
                30_000,
                100,
                MetricDistribution::default(),
            )
            .with_rollup_profiles(vec![
                MetricRollupProfile::new("10ms".into(), 10, 100, 2, MetricRollupAggregation::Sum)
                    .unwrap(),
            ]),
        )
        .unwrap();
        metrics_put_descriptor(&mut loom, ns, &descriptor).unwrap();
        let first = MetricObservation::new(
            descriptor.digest().unwrap(),
            BTreeMap::from([("method".into(), "GET".into())]),
            10,
            1.0,
        )
        .unwrap();
        let second = MetricObservation::new(
            descriptor.digest().unwrap(),
            BTreeMap::from([("method".into(), "GET".into())]),
            20,
            2.0,
        )
        .unwrap();
        let series_id = first.series_id().unwrap();
        metrics_put_observation(&mut loom, ns, "requests", &first).unwrap();
        metrics_put_observation(&mut loom, ns, "requests", &second).unwrap();
        let future = MetricObservation::new(
            descriptor.digest().unwrap(),
            BTreeMap::from([("method".into(), "GET".into())]),
            40,
            4.0,
        )
        .unwrap();
        metrics_put_observation(&mut loom, ns, "requests", &future).unwrap();
        metrics_materialize_rollups(&mut loom, ns, "requests").unwrap();
        let query = MetricTieredQuery {
            from_timestamp_ms: 0,
            to_timestamp_ms: 20,
            resolution_ms: 10,
            aggregation: MetricRollupAggregation::Sum,
            temporal_semantics: MetricQueryTemporalSemantics::OpenStartClosedEnd,
            max_series: 1,
            max_groups: 1,
            max_samples: 2,
            max_output_bytes: 4096,
            now_timestamp_ms: 20,
        };
        let plan = metrics_plan_query(&loom, ns, "requests", &query).unwrap();
        assert!(matches!(
            plan.tier,
            MetricQueryTier::Rollup {
                ref profile_name,
                period_ms: 10,
                ..
            } if profile_name == "10ms"
        ));
        assert!(!plan.partial);
        assert!(!plan.stale);

        let mut mismatched_temporal = query.clone();
        mismatched_temporal.temporal_semantics =
            MetricQueryTemporalSemantics::InclusiveStartExclusiveEnd;
        assert_eq!(
            metrics_plan_query(&loom, ns, "requests", &mismatched_temporal)
                .unwrap()
                .tier,
            MetricQueryTier::Raw
        );

        let mut mismatched_aggregation = query.clone();
        mismatched_aggregation.aggregation = MetricRollupAggregation::Last;
        assert_eq!(
            metrics_plan_query(&loom, ns, "requests", &mismatched_aggregation)
                .unwrap()
                .tier,
            MetricQueryTier::Raw
        );

        let mut output_limited = query.clone();
        output_limited.max_output_bytes = 1;
        let output_limited_plan =
            metrics_plan_query(&loom, ns, "requests", &output_limited).unwrap();
        assert_eq!(output_limited_plan.tier, MetricQueryTier::Raw);
        assert!(output_limited_plan.partial);

        let late = MetricObservation::new(
            descriptor.digest().unwrap(),
            BTreeMap::from([("method".into(), "GET".into())]),
            9,
            3.0,
        )
        .unwrap();
        metrics_put_observation(&mut loom, ns, "requests", &late).unwrap();
        assert_eq!(
            metrics_get_rollup(&loom, ns, "requests", "10ms", &series_id, 10)
                .unwrap()
                .unwrap()
                .status,
            MetricRollupWindowStatus::Stale
        );
        let stale_plan = metrics_plan_query(&loom, ns, "requests", &query).unwrap();
        assert_eq!(stale_plan.tier, MetricQueryTier::Raw);
        assert!(stale_plan.stale);
    }
}
