use super::*;
use loom_metrics::{
    DEFAULT_RETENTION_MS, InstrumentKind, MetricDescriptor, MetricDescriptorPolicy,
    MetricDistribution, MetricExemplar, MetricHistogram, MetricObservation,
    MetricRollupAggregation, MetricRollupProfile, MetricRollupValue, MetricRollupWindowStatus,
    MetricValue, Temporality,
};

pub struct MetricCanonicalVector {
    pub name: &'static str,
    pub descriptor: MetricDescriptor,
    pub expect_descriptor_canonical: &'static str,
    pub observation: MetricObservation,
    pub expect_observation_canonical: &'static str,
    pub expect_rollup_profile_id: &'static str,
    pub expect_derived_rollup_digest: &'static str,
    pub expect_materialized_rollup_canonical: &'static str,
}

pub struct MetricNegativeVector {
    pub name: &'static str,
    pub canonical: &'static str,
}

pub fn metric_canonical_vectors() -> Result<Vec<MetricCanonicalVector>> {
    let descriptor = MetricDescriptor::with_policy(
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
            MetricDistribution::explicit_histogram(vec![10.0, 100.0, 1_000.0], 2)?,
        )
        .with_rollup_profiles(vec![
            MetricRollupProfile::new(
                "1m".into(),
                60_000,
                DEFAULT_RETENTION_MS * 7,
                5_000,
                MetricRollupAggregation::HistogramMerge,
            )?,
            MetricRollupProfile::new(
                "5m".into(),
                300_000,
                DEFAULT_RETENTION_MS * 30,
                10_000,
                MetricRollupAggregation::HistogramMerge,
            )?,
        ]),
    )?;
    let observation = MetricObservation::with_value_and_flags(
        descriptor.digest()?,
        BTreeMap::from([("service.name".into(), "api".into())]),
        BTreeMap::from([("telemetry.sdk.name".into(), "loom".into())]),
        BTreeMap::from([("route".into(), "/v1/items".into())]),
        Some(1_724_999_999_000),
        1_725_000_000_000,
        MetricValue::Histogram(MetricHistogram {
            buckets: vec![1, 2, 3, 4],
            count: 10,
            sum: 125.0,
        }),
        vec!["partial".into()],
        vec![MetricExemplar::new(
            BTreeMap::from([("trace".into(), "abc".into())]),
            1_725_000_000_000,
            12.5,
        )?],
    )?;
    Ok(vec![MetricCanonicalVector {
        name: "histogram-delta-observation",
        descriptor,
        expect_descriptor_canonical: "8d781a6c6f6f6d2e6d6574726963732e64657363726970746f722e763170726571756573742e6475726174696f6e7052657175657374206475726174696f6e626d7369686973746f6772616d6564656c74618165726f757465818265726f757465188018401975301a05265c008283fb4024000000000000fb4059000000000000fb408f400000000000028286781e6c6f6f6d2e6d6574726963732e726f6c6c75702e70726f66696c652e763162316d19ea601a240c84001913886f686973746f6772616d5f6d6572676586781e6c6f6f6d2e6d6574726963732e726f6c6c75702e70726f66696c652e763162356d1a000493e01a9a7ec8001927106f686973746f6772616d5f6d65726765",
        observation,
        expect_observation_canonical: "8a781b6c6f6f6d2e6d6574726963732e6f62736572766174696f6e2e76317847626c616b65333a6134393435663439396239303663316462383334336139393437306439643661353737396636313531663634653164326661636137663232346331613939346581826c736572766963652e6e616d656361706981827274656c656d657472792e73646b2e6e616d65646c6f6f6d818265726f757465692f76312f6974656d731b00000191a2031e181b00000191a20322008469686973746f6772616d84010203040afb405f40000000000081677061727469616c81838182657472616365636162631b00000191a2032200fb4029000000000000",
        expect_rollup_profile_id: "410de219a6ca64745b7bcc9f1a67784b207c16017f0339e4cd3546d92a86deb0",
        expect_derived_rollup_digest: "8b3525719dfdca4b3f7f4a9e3bf80d6770e9cafd5d63edb2371a25b333871512",
        expect_materialized_rollup_canonical: "8a781d6c6f6f6d2e6d6574726963732e726f6c6c75702e7265636f72642e76318678216c6f6f6d2e6d6574726963732e726f6c6c75702e646572697665642d69642e76317847626c616b65333a613439343566343939623930366331646238333433613939343730643964366135373739663631353166363465316432666163613766323234633161393934657840366566626130396462393935346133386161333639316431623638663264643030396233653964313937666364393331316639633364366535643334396339357840343130646532313961366361363437343562376263633966316136373738346232303763313630313766303333396534636433353436643932613836646562301b00000191a20237a01b00000191a203220062316d6f686973746f6772616d5f6d65726765677061727469616c018469686973746f6772616d84010203040afb405f4000000000001b00000191a20322001b00000191a20322001b00000191a2030e78",
    }])
}

pub const METRIC_NEGATIVE_VECTORS: &[MetricNegativeVector] = &[
    MetricNegativeVector {
        name: "descriptor-wrong-schema",
        canonical: "816178",
    },
    MetricNegativeVector {
        name: "observation-duplicate-attribute",
        canonical: "85781b6c6f6f6d2e6d6574726963732e6f62736572766174696f6e2e76316178828261616178826161617901fb3ff0000000000000",
    },
    MetricNegativeVector {
        name: "descriptor-unknown-kind",
        canonical: "8d781a6c6f6f6d2e6d6574726963732e64657363726970746f722e76316872657175657374736061316574696d65726a63756d756c617469766581666d6574686f648018401975301a05265c0082800080",
    },
    MetricNegativeVector {
        name: "descriptor-unknown-temporality",
        canonical: "8d781a6c6f6f6d2e6d6574726963732e64657363726970746f722e763168726571756573747360613167636f756e7465726677696e646f7781666d6574686f648018401975301a05265c0082800080",
    },
    MetricNegativeVector {
        name: "descriptor-unsorted-attributes",
        canonical: "8d781a6c6f6f6d2e6d6574726963732e64657363726970746f722e763168726571756573747360613167636f756e7465726a63756d756c617469766582617a61618018401975301a05265c0082800080",
    },
    MetricNegativeVector {
        name: "descriptor-unknown-attribute-limit",
        canonical: "8d781a6c6f6f6d2e6d6574726963732e64657363726970746f722e763168726571756573747360613167636f756e7465726a63756d756c617469766581666d6574686f64818265726f7574650a18401975301a05265c0082800080",
    },
    MetricNegativeVector {
        name: "descriptor-empty-retention",
        canonical: "8d781a6c6f6f6d2e6d6574726963732e64657363726970746f722e763168726571756573747360613167636f756e7465726a63756d756c617469766581666d6574686f648018401975300082800080",
    },
    MetricNegativeVector {
        name: "descriptor-histogram-missing-bounds",
        canonical: "8d781a6c6f6f6d2e6d6574726963732e64657363726970746f722e763168726571756573747360613169686973746f6772616d6564656c746181666d6574686f648018401975301a05265c0082800080",
    },
    MetricNegativeVector {
        name: "descriptor-non-histogram-with-bounds",
        canonical: "8d781a6c6f6f6d2e6d6574726963732e64657363726970746f722e763168726571756573747360613167636f756e7465726a63756d756c617469766581666d6574686f648018401975301a05265c008281fb3ff00000000000000080",
    },
    MetricNegativeVector {
        name: "descriptor-invalid-rollup-retention",
        canonical: "8d781a6c6f6f6d2e6d6574726963732e64657363726970746f722e763168726571756573747360613167636f756e7465726a63756d756c617469766581666d6574686f648018401975301a05265c008280008186781e6c6f6f6d2e6d6574726963732e726f6c6c75702e70726f66696c652e763162316d19ea601903e8006373756d",
    },
    MetricNegativeVector {
        name: "descriptor-duplicate-rollup-resolution",
        canonical: "8d781a6c6f6f6d2e6d6574726963732e64657363726970746f722e763168726571756573747360613169686973746f6772616d6564656c74618165726f7574658018401975301a05265c008281fb4024000000000000018286781e6c6f6f6d2e6d6574726963732e726f6c6c75702e70726f66696c652e763162316d19ea601a240c840019138874686973746f6772616d5f6d6572676586781e6c6f6f6d2e6d6574726963732e726f6c6c75702e70726f66696c652e763162326d19ea601a240c840019138874686973746f6772616d5f6d65726765",
    },
    MetricNegativeVector {
        name: "descriptor-invalid-rollup-aggregation",
        canonical: "8d781a6c6f6f6d2e6d6574726963732e64657363726970746f722e763168726571756573747360613169686973746f6772616d6564656c74618165726f7574658018401975301a05265c008281fb4024000000000000018186781e6c6f6f6d2e6d6574726963732e726f6c6c75702e70726f66696c652e763162316d19ea601a240c84001913886373756d",
    },
    MetricNegativeVector {
        name: "observation-zero-timestamp",
        canonical: "87781b6c6f6f6d2e6d6574726963732e6f62736572766174696f6e2e76317847626c616b65333a303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030308182666d6574686f6463474554f60082666e756d626572fb3ff000000000000080",
    },
    MetricNegativeVector {
        name: "observation-start-after-timestamp",
        canonical: "87781b6c6f6f6d2e6d6574726963732e6f62736572766174696f6e2e76317847626c616b65333a303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030308182666d6574686f64634745540b0a82666e756d626572fb3ff000000000000080",
    },
    MetricNegativeVector {
        name: "observation-unknown-value-kind",
        canonical: "87781b6c6f6f6d2e6d6574726963732e6f62736572766174696f6e2e76317847626c616b65333a303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030308182666d6574686f6463474554f6018266737472696e676362616480",
    },
    MetricNegativeVector {
        name: "observation-invalid-histogram-count",
        canonical: "87781b6c6f6f6d2e6d6574726963732e6f62736572766174696f6e2e76317847626c616b65333a303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030308182666d6574686f6463474554f6018469686973746f6772616d82010201fb400800000000000080",
    },
    MetricNegativeVector {
        name: "observation-exemplar-future-timestamp",
        canonical: "87781b6c6f6f6d2e6d6574726963732e6f62736572766174696f6e2e76317847626c616b65333a303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030308182666d6574686f6463474554f60182666e756d626572fb3ff0000000000000818381826574726163656361626302fb3ff0000000000000",
    },
    MetricNegativeVector {
        name: "observation-unsorted-flags",
        canonical: "8a781b6c6f6f6d2e6d6574726963732e6f62736572766174696f6e2e76317847626c616b65333a3030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303030303080808182666d6574686f6463474554f60182666e756d626572fb3ff000000000000082617a616180",
    },
];

pub fn run_metrics_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let ns =
        loom.registry_mut()
            .create(FacetKind::Metrics, None, WorkspaceId::from_bytes([31; 16]))?;
    for vector in metric_canonical_vectors()? {
        let descriptor_bytes = vector.descriptor.encode()?;
        assert_eq!(
            hex::encode(&descriptor_bytes),
            vector.expect_descriptor_canonical,
            "metric descriptor canonical bytes mismatch for '{}'",
            vector.name
        );
        assert_eq!(
            MetricDescriptor::decode(&descriptor_bytes)?,
            vector.descriptor,
            "metric descriptor canonical round-trip mismatch for '{}'",
            vector.name
        );

        let observation_bytes = vector.observation.encode()?;
        assert_eq!(
            hex::encode(&observation_bytes),
            vector.expect_observation_canonical,
            "metric observation canonical bytes mismatch for '{}'",
            vector.name
        );
        assert_eq!(
            MetricObservation::decode(&observation_bytes)?,
            vector.observation,
            "metric observation canonical round-trip mismatch for '{}'",
            vector.name
        );
        let profile = vector
            .descriptor
            .rollup_profile("1m")
            .ok_or_else(|| loom_core::LoomError::not_found("metric rollup profile not found"))?;
        assert_eq!(
            profile.profile_id()?,
            vector.expect_rollup_profile_id,
            "metric rollup profile identity mismatch for '{}'",
            vector.name
        );
        let rollup_id = vector
            .descriptor
            .derived_rollup_id(&vector.observation, "1m")?;
        assert_eq!(
            rollup_id.window_start_ms, 1_724_999_940_000,
            "metric rollup window start mismatch for '{}'",
            vector.name
        );
        assert_eq!(
            rollup_id.window_end_ms, 1_725_000_000_000,
            "metric rollup window end mismatch for '{}'",
            vector.name
        );
        assert_eq!(
            rollup_id.digest()?,
            vector.expect_derived_rollup_digest,
            "metric derived rollup identity mismatch for '{}'",
            vector.name
        );

        loom_core::metrics_put_descriptor(loom, ns, &vector.descriptor)?;
        loom_core::metrics_put_observation(loom, ns, &vector.descriptor.name, &vector.observation)?;
        let materialized = loom_core::metrics_get_rollup(
            loom,
            ns,
            &vector.descriptor.name,
            "1m",
            &vector.observation.series_id()?,
            rollup_id.window_end_ms,
        )?
        .ok_or_else(|| loom_core::LoomError::not_found("metric rollup not materialized"))?;
        assert_eq!(
            materialized.status,
            MetricRollupWindowStatus::Partial,
            "metric rollup must start partial for '{}'",
            vector.name
        );
        assert_eq!(
            materialized.value,
            MetricRollupValue::Histogram(MetricHistogram {
                buckets: vec![1, 2, 3, 4],
                count: 10,
                sum: 125.0,
            }),
            "metric rollup value mismatch for '{}'",
            vector.name
        );
        assert_eq!(
            hex::encode(materialized.encode()?),
            vector.expect_materialized_rollup_canonical,
            "metric materialized rollup canonical bytes mismatch for '{}'",
            vector.name
        );
        let duplicate = loom_core::metrics_materialize_rollups(loom, ns, &vector.descriptor.name)?;
        assert_eq!(
            duplicate.written, 2,
            "metric rollup materialization must be repeatable for '{}'",
            vector.name
        );
        assert_eq!(
            loom_core::metrics_get_descriptor(loom, ns, &vector.descriptor.name)?,
            Some(vector.descriptor.clone()),
            "metric descriptor storage round-trip mismatch for '{}'",
            vector.name
        );
        assert_eq!(
            loom_core::metrics_get_observation(
                loom,
                ns,
                &vector.descriptor.name,
                vector.observation.timestamp_ms
            )?,
            Some(vector.observation.clone()),
            "metric observation storage round-trip mismatch for '{}'",
            vector.name
        );
        let query = loom_core::metrics_query_observations(
            loom,
            ns,
            &vector.descriptor.name,
            &loom_core::MetricQuery {
                from_timestamp_ms: vector.observation.timestamp_ms - 1,
                to_timestamp_ms: vector.observation.timestamp_ms + 1,
                max_series: 1,
                max_groups: 1,
                max_samples: 1,
                max_output_bytes: 1024,
                now_timestamp_ms: vector.observation.timestamp_ms + 60_000,
            },
        )?;
        assert_eq!(
            query.observations,
            vec![vector.observation.clone()],
            "metric query storage round-trip mismatch for '{}'",
            vector.name
        );
        assert!(
            query.partial,
            "metric query must report sample-limit partiality"
        );
        assert!(query.stale, "metric query must report stale visibility");
        let byte_limited_query = loom_core::metrics_query_observations(
            loom,
            ns,
            &vector.descriptor.name,
            &loom_core::MetricQuery {
                from_timestamp_ms: vector.observation.timestamp_ms - 1,
                to_timestamp_ms: vector.observation.timestamp_ms + 1,
                max_series: 1,
                max_groups: 1,
                max_samples: 8,
                max_output_bytes: 1,
                now_timestamp_ms: vector.observation.timestamp_ms + 60_000,
            },
        )?;
        assert!(
            byte_limited_query.observations.is_empty(),
            "metric query must respect output-byte bounds"
        );
        assert!(
            byte_limited_query.partial,
            "metric query must report byte-limit partiality"
        );

        let future_observation = MetricObservation::with_value_and_flags(
            vector.descriptor.digest()?,
            vector.observation.resource.clone(),
            vector.observation.scope.clone(),
            vector.observation.attributes.clone(),
            Some(1_725_000_059_000),
            1_725_000_060_000,
            MetricValue::Histogram(MetricHistogram {
                buckets: vec![1, 1, 1, 1],
                count: 4,
                sum: 40.0,
            }),
            Vec::new(),
            Vec::new(),
        )?;
        loom_core::metrics_put_observation(loom, ns, &vector.descriptor.name, &future_observation)?;
        loom_core::metrics_materialize_rollups(loom, ns, &vector.descriptor.name)?;
        let rollup_plan = loom_core::metrics_plan_query(
            loom,
            ns,
            &vector.descriptor.name,
            &loom_core::MetricTieredQuery {
                from_timestamp_ms: rollup_id.window_start_ms,
                to_timestamp_ms: rollup_id.window_end_ms,
                resolution_ms: 60_000,
                aggregation: MetricRollupAggregation::HistogramMerge,
                temporal_semantics: loom_core::MetricQueryTemporalSemantics::OpenStartClosedEnd,
                max_series: 1,
                max_groups: 1,
                max_samples: 1,
                max_output_bytes: 4096,
                now_timestamp_ms: future_observation.timestamp_ms,
            },
        )?;
        assert!(
            matches!(rollup_plan.tier, loom_core::MetricQueryTier::Rollup { .. }),
            "metric query planner must select equivalent rollup data for '{}'",
            vector.name
        );
        assert!(
            !rollup_plan.partial && !rollup_plan.stale,
            "metric query planner rollup selection must preserve freshness and bounds for '{}'",
            vector.name
        );
        let raw_plan = loom_core::metrics_plan_query(
            loom,
            ns,
            &vector.descriptor.name,
            &loom_core::MetricTieredQuery {
                from_timestamp_ms: rollup_id.window_start_ms,
                to_timestamp_ms: rollup_id.window_end_ms,
                resolution_ms: 60_000,
                aggregation: MetricRollupAggregation::HistogramMerge,
                temporal_semantics:
                    loom_core::MetricQueryTemporalSemantics::InclusiveStartExclusiveEnd,
                max_series: 1,
                max_groups: 1,
                max_samples: 1,
                max_output_bytes: 4096,
                now_timestamp_ms: future_observation.timestamp_ms,
            },
        )?;
        assert_eq!(
            raw_plan.tier,
            loom_core::MetricQueryTier::Raw,
            "metric query planner must fall back to raw data when temporal semantics differ for '{}'",
            vector.name
        );

        let late_observation = MetricObservation::with_value_and_flags(
            vector.descriptor.digest()?,
            vector.observation.resource.clone(),
            vector.observation.scope.clone(),
            vector.observation.attributes.clone(),
            Some(1_724_999_998_000),
            1_724_999_999_000,
            MetricValue::Histogram(MetricHistogram {
                buckets: vec![2, 3, 4, 5],
                count: 14,
                sum: 140.0,
            }),
            Vec::new(),
            Vec::new(),
        )?;
        loom_core::metrics_put_observation(loom, ns, &vector.descriptor.name, &late_observation)?;
        let stale = loom_core::metrics_get_rollup(
            loom,
            ns,
            &vector.descriptor.name,
            "1m",
            &vector.observation.series_id()?,
            rollup_id.window_end_ms,
        )?
        .ok_or_else(|| loom_core::LoomError::not_found("metric rollup not materialized"))?;
        assert_eq!(
            stale.status,
            MetricRollupWindowStatus::Stale,
            "late metric observation must mark finalized rollup stale for '{}'",
            vector.name
        );
        let stale_plan = loom_core::metrics_plan_query(
            loom,
            ns,
            &vector.descriptor.name,
            &loom_core::MetricTieredQuery {
                from_timestamp_ms: rollup_id.window_start_ms,
                to_timestamp_ms: rollup_id.window_end_ms,
                resolution_ms: 60_000,
                aggregation: MetricRollupAggregation::HistogramMerge,
                temporal_semantics: loom_core::MetricQueryTemporalSemantics::OpenStartClosedEnd,
                max_series: 1,
                max_groups: 1,
                max_samples: 1,
                max_output_bytes: 4096,
                now_timestamp_ms: future_observation.timestamp_ms,
            },
        )?;
        assert_eq!(
            stale_plan.tier,
            loom_core::MetricQueryTier::Raw,
            "metric query planner must fall back to raw data for stale rollups for '{}'",
            vector.name
        );
        assert!(
            stale_plan.stale,
            "metric query planner must report stale rollup visibility for '{}'",
            vector.name
        );
        let rebuilt = loom_core::metrics_rebuild_rollups(
            loom,
            ns,
            &vector.descriptor.name,
            rollup_id.window_start_ms,
            rollup_id.window_end_ms,
        )?;
        assert_eq!(
            rebuilt.written, 2,
            "metric rollup rebuild must rewrite affected profile windows for '{}'",
            vector.name
        );
        let rebuilt = loom_core::metrics_get_rollup(
            loom,
            ns,
            &vector.descriptor.name,
            "1m",
            &vector.observation.series_id()?,
            rollup_id.window_end_ms,
        )?
        .ok_or_else(|| loom_core::LoomError::not_found("metric rollup not materialized"))?;
        assert_eq!(
            rebuilt.status,
            MetricRollupWindowStatus::Final,
            "metric rollup rebuild must converge for '{}'",
            vector.name
        );
        assert_eq!(
            rebuilt.value,
            MetricRollupValue::Histogram(MetricHistogram {
                buckets: vec![3, 5, 7, 9],
                count: 24,
                sum: 265.0,
            }),
            "metric rebuilt rollup value mismatch for '{}'",
            vector.name
        );
        let compacted = loom_core::metrics_compact_rollups(
            loom,
            ns,
            &vector.descriptor.name,
            rollup_id.window_end_ms + DEFAULT_RETENTION_MS * 7 + 1,
        )?;
        assert_eq!(
            compacted.compacted, 1,
            "metric rollup compaction must remove expired tier data for '{}'",
            vector.name
        );
    }
    let redaction_descriptor = MetricDescriptor::with_policy(
        "redaction.check".into(),
        "Redaction check".into(),
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
            MetricDistribution::explicit_histogram(Vec::new(), 1)?,
        ),
    )?;
    let redaction_observation = MetricObservation::with_value(
        redaction_descriptor.digest()?,
        BTreeMap::from([("service.name".into(), "api".into())]),
        BTreeMap::from([("telemetry.sdk.name".into(), "loom".into())]),
        BTreeMap::from([
            ("authorization".into(), "Bearer secret-token".into()),
            ("method".into(), "GET".into()),
            ("request_path".into(), "/private/user/123".into()),
        ]),
        None,
        1_725_000_010_000,
        MetricValue::Number(1.0),
        vec![MetricExemplar::new(
            BTreeMap::from([("token".into(), "secret".into())]),
            1_725_000_010_000,
            1.0,
        )?],
    )?;
    loom_core::metrics_put_descriptor(loom, ns, &redaction_descriptor)?;
    loom_core::metrics_put_observation(
        loom,
        ns,
        &redaction_descriptor.name,
        &redaction_observation,
    )?;
    let stored_redaction = loom_core::metrics_get_observation(
        loom,
        ns,
        &redaction_descriptor.name,
        redaction_observation.timestamp_ms,
    )?
    .ok_or_else(|| loom_core::LoomError::not_found("redacted metric observation not found"))?;
    assert_eq!(
        stored_redaction.attributes.get("authorization"),
        Some(&"redacted".to_string()),
        "metric storage must redact credential attributes"
    );
    assert_eq!(
        stored_redaction.attributes.get("request_path"),
        Some(&"redacted".to_string()),
        "metric storage must redact path attributes"
    );
    assert_eq!(
        stored_redaction.exemplars[0].attributes.get("token"),
        Some(&"redacted".to_string()),
        "metric storage must redact exemplar token attributes"
    );
    let group_descriptor = MetricDescriptor::new(
        "group.limit".into(),
        "Group limit".into(),
        "1".into(),
        InstrumentKind::Counter,
        Temporality::Cumulative,
        vec!["method".into()],
        4,
        30_000,
    )?;
    let get_group = MetricObservation::new(
        group_descriptor.digest()?,
        BTreeMap::from([("method".into(), "GET".into())]),
        1_725_000_020_000,
        1.0,
    )?;
    let post_group = MetricObservation::new(
        group_descriptor.digest()?,
        BTreeMap::from([("method".into(), "POST".into())]),
        1_725_000_020_001,
        1.0,
    )?;
    loom_core::metrics_put_descriptor(loom, ns, &group_descriptor)?;
    loom_core::metrics_put_observation(loom, ns, &group_descriptor.name, &get_group)?;
    loom_core::metrics_put_observation(loom, ns, &group_descriptor.name, &post_group)?;
    let group_limited_query = loom_core::metrics_query_observations(
        loom,
        ns,
        &group_descriptor.name,
        &loom_core::MetricQuery {
            from_timestamp_ms: 1_725_000_019_999,
            to_timestamp_ms: 1_725_000_020_002,
            max_series: 4,
            max_groups: 1,
            max_samples: 8,
            max_output_bytes: 1024,
            now_timestamp_ms: 1_725_000_020_002,
        },
    )?;
    assert_eq!(
        group_limited_query.observations.len(),
        1,
        "metric query must respect group-count bounds"
    );
    assert!(
        group_limited_query.partial,
        "metric query must report group-limit partiality"
    );
    for vector in METRIC_NEGATIVE_VECTORS {
        let bytes = hex::decode(vector.canonical).map_err(|err| {
            loom_core::LoomError::invalid(format!(
                "bad metric negative vector {}: {err}",
                vector.name
            ))
        })?;
        assert!(
            MetricDescriptor::decode(&bytes).is_err() || MetricObservation::decode(&bytes).is_err(),
            "metric negative vector '{}' must be rejected",
            vector.name
        );
    }
    Ok(())
}
