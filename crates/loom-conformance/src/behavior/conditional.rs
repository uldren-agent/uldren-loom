use super::*;
use loom_core::LoomError;
use loom_types::{
    CompareDisposition, CompareOutcome, ConflictReason, ContentTag, EntityTag, EntityTagDerivation,
    EntityTagSource, IdempotencyKey, MutationMode, MutationRequest,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RealOwnerConditionKind {
    Any,
    Absent,
    Exact,
    Generation,
    OperationAnchor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StableConditionalError {
    pub code: Code,
    pub reason_fragment: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RealOwnerConditionalScenario {
    pub name: &'static str,
    pub owner_target: &'static str,
    pub atomic_scope: &'static str,
    pub condition_kind: RealOwnerConditionKind,
    pub stale_error: StableConditionalError,
}

#[derive(Debug)]
pub struct RealOwnerConditionalOutcome {
    pub applied: bool,
    pub stale_rejected: bool,
    pub no_partial_mutation: bool,
    pub stale_error: Option<LoomError>,
}

pub fn run_real_owner_conditional_scenario<F>(
    scenario: RealOwnerConditionalScenario,
    run: F,
) -> Result<()>
where
    F: FnOnce() -> Result<RealOwnerConditionalOutcome>,
{
    if scenario.name.is_empty() {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "conditional scenario name is empty",
        ));
    }
    if scenario.owner_target.is_empty() {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "conditional scenario owner target is empty",
        ));
    }
    if scenario.atomic_scope.is_empty() {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "conditional scenario atomic scope is empty",
        ));
    }

    let outcome = run()?;
    if !outcome.applied {
        return Err(LoomError::new(
            Code::Internal,
            format!("{} did not apply a satisfied condition", scenario.name),
        ));
    }
    if !outcome.stale_rejected {
        return Err(LoomError::new(
            Code::Internal,
            format!("{} did not reject a stale condition", scenario.name),
        ));
    }
    if !outcome.no_partial_mutation {
        return Err(LoomError::new(
            Code::Internal,
            format!("{} partially applied a rejected condition", scenario.name),
        ));
    }

    let Some(stale_error) = outcome.stale_error else {
        return Err(LoomError::new(
            Code::Internal,
            format!("{} did not report a stable stale error", scenario.name),
        ));
    };
    if stale_error.code != scenario.stale_error.code {
        return Err(LoomError::new(
            Code::Internal,
            format!("{} stale error code drift", scenario.name),
        ));
    }
    if let Some(reason) = scenario.stale_error.reason_fragment
        && !stale_error.message.contains(reason)
    {
        return Err(LoomError::new(
            Code::Internal,
            format!("{} stale error reason drift", scenario.name),
        ));
    }

    Ok(())
}

pub fn run_conditional_mutation_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let digest = Digest::blake3(b"record bytes");
    let content_tag = ContentTag::new(digest);
    let content_entity_tag = content_tag.to_entity_tag();
    if content_entity_tag.as_bytes() == digest.bytes() {
        return Err(LoomError::new(
            Code::Internal,
            "content tag bytes must not be accepted as the entity tag token",
        ));
    }

    let generated = EntityTagDerivation {
        source: EntityTagSource::MutableStateVersion(7),
        atomic_scope: b"kv/orders".to_vec(),
        representation: None,
    }
    .entity_tag();
    if generated != EntityTag::from_generation(b"kv/orders", 7) {
        return Err(LoomError::new(
            Code::Internal,
            "generation entity tag derivation drift",
        ));
    }
    if generated == EntityTag::from_generation(b"kv/orders", 8) {
        return Err(LoomError::new(
            Code::Internal,
            "generation entity tag must change with generation",
        ));
    }

    let rejected = CompareOutcome::rejected(CompareDisposition::ExactMismatch);
    let Some(conflict) = rejected.conflict else {
        return Err(LoomError::new(
            Code::Internal,
            "exact mismatch did not carry conflict",
        ));
    };
    if conflict.reason != ConflictReason::ExpectedTagMismatch {
        return Err(LoomError::new(
            Code::Internal,
            "exact mismatch reason drift",
        ));
    }

    run_document_conditional_scenario(loom)?;

    Ok(())
}

fn run_document_conditional_scenario<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    run_real_owner_conditional_scenario(
        RealOwnerConditionalScenario {
            name: "document-entity-tag-exact-replace",
            owner_target: "document:docs/a",
            atomic_scope: "document:workspace/docs/a",
            condition_kind: RealOwnerConditionKind::Exact,
            stale_error: StableConditionalError {
                code: Code::Conflict,
                reason_fragment: Some("expected_tag_mismatch"),
            },
        },
        || {
            let ns = loom.registry_mut().create(
                FacetKind::Document,
                None,
                WorkspaceId::from_bytes([72; 16]),
            )?;
            let created = document_put_binary_with_request(
                loom,
                ns,
                "docs",
                "a",
                b"one".to_vec(),
                MutationRequest::new(MutationMode::CreateIfAbsent),
            )?;
            let Some(first_tag) = created.outcome.entity_tag else {
                return Err(LoomError::new(
                    Code::Internal,
                    "document create did not return entity tag",
                ));
            };

            let collision = document_put_binary_with_request(
                loom,
                ns,
                "docs",
                "a",
                b"collision".to_vec(),
                MutationRequest::new(MutationMode::CreateIfAbsent),
            )
            .unwrap_err();
            if collision.code != Code::Conflict
                || !collision.message.contains("record_already_exists")
            {
                return Err(LoomError::new(
                    Code::Internal,
                    "create_if_absent collision reason drift",
                ));
            }

            let stale = document_put_binary_with_request(
                loom,
                ns,
                "docs",
                "a",
                b"stale".to_vec(),
                MutationRequest::new(MutationMode::ReplaceIfMatch(EntityTag::opaque(
                    b"not-current",
                ))),
            )
            .unwrap_err();

            let replaced = document_put_binary_with_request(
                loom,
                ns,
                "docs",
                "a",
                b"two".to_vec(),
                MutationRequest::new(MutationMode::ReplaceIfMatch(first_tag.clone())),
            )?;
            if replaced.outcome.disposition != CompareDisposition::Applied {
                return Err(LoomError::new(
                    Code::Internal,
                    "conditional document replace did not apply",
                ));
            }

            let retry_key = IdempotencyKey::opaque(b"retry-key");
            let stale_retry = document_put_binary_with_request(
                loom,
                ns,
                "docs",
                "a",
                b"retry".to_vec(),
                MutationRequest::with_idempotency_key(
                    MutationMode::ReplaceIfMatch(first_tag.clone()),
                    retry_key,
                ),
            )
            .unwrap_err();
            if stale_retry.code != Code::Conflict
                || !stale_retry.message.contains("expected_tag_mismatch")
            {
                return Err(LoomError::new(
                    Code::Internal,
                    "idempotency key changed stale entity tag behavior",
                ));
            }

            let request_a = MutationRequest::with_idempotency_key(
                MutationMode::ReplaceIfMatch(first_tag.clone()),
                IdempotencyKey::opaque(b"a"),
            );
            let request_b = MutationRequest::with_idempotency_key(
                MutationMode::ReplaceIfMatch(first_tag),
                IdempotencyKey::opaque(b"b"),
            );
            if request_a.compare_condition() != request_b.compare_condition() {
                return Err(LoomError::new(
                    Code::Internal,
                    "idempotency keys must not change compare conditions",
                ));
            }

            let body = document_get_binary(loom, ns, "docs", "a")?
                .ok_or_else(|| LoomError::new(Code::Conflict, "document missing after replace"))?;
            let deleted = document_delete_with_request(
                loom,
                ns,
                "docs",
                "a",
                MutationRequest::new(MutationMode::DeleteIfPresent),
            )?;
            if deleted.disposition != CompareDisposition::Applied {
                return Err(LoomError::new(
                    Code::Internal,
                    "delete_if_present did not apply",
                ));
            }

            Ok(RealOwnerConditionalOutcome {
                applied: replaced.outcome.disposition == CompareDisposition::Applied,
                stale_rejected: stale.code == Code::Conflict
                    && stale.message.contains("expected_tag_mismatch"),
                no_partial_mutation: body.bytes == b"two",
                stale_error: Some(stale),
            })
        },
    )
}

pub fn run_projection_adapter_conditional_mutation_behavior() -> Result<()> {
    run_real_owner_conditional_scenario(
        RealOwnerConditionalScenario {
            name: "document-expected-entity-tag-adapter",
            owner_target: "document:adapter/a",
            atomic_scope: "document:workspace/adapter/a",
            condition_kind: RealOwnerConditionKind::Exact,
            stale_error: StableConditionalError {
                code: Code::Conflict,
                reason_fragment: Some("expected_tag_mismatch"),
            },
        },
        || {
            let mut loom = Loom::new(loom_core::MemoryStore::new());
            let ns = loom.registry_mut().create(
                FacetKind::Document,
                None,
                WorkspaceId::from_bytes([0x68; 16]),
            )?;

            let created = document_put_binary_with_entity_tag(
                &mut loom,
                ns,
                "adapter",
                "a",
                b"one".to_vec(),
                None,
            )?;
            let replaced = document_put_binary_with_entity_tag(
                &mut loom,
                ns,
                "adapter",
                "a",
                b"two".to_vec(),
                Some(&created.entity_tag),
            )?;
            if replaced.entity_tag == created.entity_tag {
                return Err(LoomError::new(
                    Code::Internal,
                    "adapter entity tag did not change after replacement",
                ));
            }

            let stale = document_put_binary_with_entity_tag(
                &mut loom,
                ns,
                "adapter",
                "a",
                b"stale".to_vec(),
                Some(&created.entity_tag),
            )
            .unwrap_err();
            let body = document_get_binary(&loom, ns, "adapter", "a")?.ok_or_else(|| {
                LoomError::new(Code::Conflict, "adapter document missing after stale write")
            })?;

            Ok(RealOwnerConditionalOutcome {
                applied: true,
                stale_rejected: stale.code == Code::Conflict
                    && stale.message.contains("expected_tag_mismatch"),
                no_partial_mutation: body.bytes == b"two",
                stale_error: Some(stale),
            })
        },
    )
}

pub fn run_kv_conditional_mutation_behavior() -> Result<()> {
    run_real_owner_conditional_scenario(
        RealOwnerConditionalScenario {
            name: "kv-single-key-exact-token",
            owner_target: "kv:conformance/conditional",
            atomic_scope: "kv:workspace/conformance/conditional",
            condition_kind: RealOwnerConditionKind::Exact,
            stale_error: StableConditionalError {
                code: Code::Conflict,
                reason_fragment: None,
            },
        },
        || {
            let mut loom = Loom::new(loom_core::MemoryStore::new());
            let ns = loom.registry_mut().create(
                FacetKind::Kv,
                None,
                WorkspaceId::from_bytes([0x4b; 16]),
            )?;
            let key = Value::Text("conditional".into());

            kv_put_conditioned(
                &mut loom,
                ns,
                "conformance",
                key.clone(),
                b"one".to_vec(),
                KvCondition::Absent,
            )?;
            let token = kv_exact_token(&loom, ns, "conformance", &key)?
                .ok_or_else(|| LoomError::new(Code::Conflict, "missing exact KV token"))?;

            let error = kv_put_conditioned(
                &mut loom,
                ns,
                "conformance",
                key.clone(),
                b"two".to_vec(),
                KvCondition::Absent,
            )
            .expect_err("an existing entry must fail an absent condition");
            if error.code != Code::AlreadyExists {
                return Err(error);
            }
            if kv_get(&loom, ns, "conformance", &key)?.as_deref() != Some(&b"one"[..]) {
                return Err(LoomError::new(
                    Code::Conflict,
                    "failed KV conditional mutation changed the entry",
                ));
            }

            kv_put_conditioned(
                &mut loom,
                ns,
                "conformance",
                Value::Text("unrelated".into()),
                b"other".to_vec(),
                KvCondition::Any,
            )?;

            kv_put_conditioned(
                &mut loom,
                ns,
                "conformance",
                key.clone(),
                b"one".to_vec(),
                KvCondition::Exact(token.clone()),
            )?;
            let stale_identical = kv_put_conditioned(
                &mut loom,
                ns,
                "conformance",
                key.clone(),
                b"two".to_vec(),
                KvCondition::Exact(token),
            )
            .expect_err("an exact token must stale after an identical-value write");
            if stale_identical.code != Code::Conflict {
                return Err(stale_identical);
            }
            let body_after_stale = kv_get(&loom, ns, "conformance", &key)?;
            if body_after_stale.as_deref() != Some(&b"one"[..]) {
                return Err(LoomError::new(
                    Code::Conflict,
                    "failed KV exact mutation changed the entry",
                ));
            }

            let token = kv_exact_token(&loom, ns, "conformance", &key)?
                .ok_or_else(|| LoomError::new(Code::Conflict, "missing exact KV token"))?;
            kv_put_conditioned(
                &mut loom,
                ns,
                "conformance",
                key.clone(),
                b"two".to_vec(),
                KvCondition::Exact(token.clone()),
            )?;
            let error = kv_delete_conditioned(
                &mut loom,
                ns,
                "conformance",
                &key,
                KvCondition::Exact(token),
            )
            .expect_err("an exact token must stale after a changed-value write");
            if error.code != Code::Conflict {
                return Err(error);
            }

            let token = kv_exact_token(&loom, ns, "conformance", &key)?
                .ok_or_else(|| LoomError::new(Code::Conflict, "missing exact KV token"))?;
            kv_delete(&mut loom, ns, "conformance", &key)?;
            kv_put(&mut loom, ns, "conformance", key.clone(), b"two".to_vec())?;
            let error = kv_delete_conditioned(
                &mut loom,
                ns,
                "conformance",
                &key,
                KvCondition::Exact(token),
            )
            .expect_err("an exact token must stale after delete and recreate");
            if error.code != Code::Conflict {
                return Err(error);
            }

            let token = kv_exact_token(&loom, ns, "conformance", &key)?
                .ok_or_else(|| LoomError::new(Code::Conflict, "missing exact KV token"))?;
            let mut replacement = KvMap::new();
            replacement.put(key.clone(), b"two".to_vec());
            replacement.put(Value::Text("unrelated".into()), b"other".to_vec());
            replace_kv_map(&mut loom, ns, "conformance", &replacement)?;
            let error = kv_put_conditioned(
                &mut loom,
                ns,
                "conformance",
                key,
                b"three".to_vec(),
                KvCondition::Exact(token),
            )
            .expect_err("a whole-map replacement must stale every entry token");
            if error.code != Code::Conflict {
                return Err(error);
            }

            Ok(RealOwnerConditionalOutcome {
                applied: true,
                stale_rejected: true,
                no_partial_mutation: body_after_stale.as_deref() == Some(&b"one"[..]),
                stale_error: Some(stale_identical),
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::MemoryStore;

    #[test]
    fn conditional_mutation_behavior_passes() {
        let mut loom = Loom::new(MemoryStore::new());
        run_conditional_mutation_behavior(&mut loom)
            .expect("conditional mutation behavior must pass");
    }

    #[test]
    fn kv_conditional_mutation_behavior_passes() {
        run_kv_conditional_mutation_behavior().expect("kv conditional mutation behavior must pass");
    }

    #[test]
    fn projection_adapter_conditional_mutation_behavior_passes() {
        run_projection_adapter_conditional_mutation_behavior()
            .expect("projection adapter conditional mutation behavior must pass");
    }
}
