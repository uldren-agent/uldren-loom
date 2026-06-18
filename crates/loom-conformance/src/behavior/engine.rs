//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Execute the `cas` behavioral suite against any [`ObjectStore`]: a put returns the content digest,
/// get round-trips, put is idempotent (same bytes, same digest, stored once), and an unknown digest
/// is absent. The `cas` facet is the object store, so this runs today.
pub fn run_cas_behavior<S: ObjectStore>(store: &mut S) -> Result<()> {
    let obj = Object::Blob(b"immutable blob".to_vec());
    let canonical = obj.canonical();

    let d1 = store.put(&canonical)?;
    assert_eq!(d1, obj.digest(), "cas put must return the content digest");
    assert_eq!(
        store.get(&d1)?.as_deref(),
        Some(canonical.as_slice()),
        "cas get must round-trip the stored bytes"
    );

    let before = store.len();
    let d2 = store.put(&canonical)?;
    assert_eq!(d2, d1, "cas put is idempotent: same bytes -> same digest");
    assert_eq!(store.len(), before, "an idempotent put stores nothing new");

    let unknown = Object::Blob(b"never stored".to_vec()).digest();
    assert!(
        store.get(&unknown)?.is_none(),
        "an unknown digest must be absent"
    );
    Ok(())
}

struct ConformanceInferenceProvider;

struct ConformanceEmbeddingProvider;

impl InferenceProvider for ConformanceInferenceProvider {
    fn id(&self) -> &str {
        "conformance"
    }

    fn infer(&self, request: &InferenceRequest) -> Result<InferenceResponse> {
        let content = request
            .messages
            .last()
            .map(|message| message.content.clone())
            .unwrap_or_default();
        Ok(InferenceResponse {
            model: "conformance-model".to_string(),
            content,
            stop_reason: Some("end_turn".to_string()),
        })
    }
}

impl EmbeddingProvider for ConformanceEmbeddingProvider {
    fn model_id(&self) -> &str {
        "conformance-embedding"
    }

    fn dimension(&self) -> usize {
        2
    }

    fn weights_digest(&self) -> Option<&str> {
        Some("sha256:conformance")
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|text| vec![text.len() as f32, text.bytes().map(f32::from).sum()])
            .collect())
    }
}

/// Execute the core `inference` seam suite (0043): no provider reports `UNSUPPORTED`, and an installed
/// provider is discoverable and receives the request exactly through the provider abstraction. This
/// proves the Loom capability seam, not the MCP sampling backend.
pub fn run_inference_behavior() -> Result<()> {
    let unavailable = Inference::none();
    assert!(
        !unavailable.is_available(),
        "no-provider inference must report unavailable"
    );
    assert_eq!(
        unavailable.provider_id(),
        None,
        "no-provider inference has no provider id"
    );
    assert_eq!(
        unavailable
            .infer(&InferenceRequest::default())
            .unwrap_err()
            .code,
        Code::Unsupported,
        "no-provider inference returns UNSUPPORTED"
    );

    let inference = Inference::with_provider(Box::new(ConformanceInferenceProvider));
    assert!(
        inference.is_available(),
        "installed-provider inference reports available"
    );
    assert_eq!(
        inference.provider_id(),
        Some("conformance"),
        "provider id comes from the installed backend"
    );
    let response = inference.infer(&InferenceRequest {
        messages: vec![
            Message::user("hello"),
            Message::assistant("ready"),
            Message::user("ping"),
        ],
        system_prompt: Some("echo the last turn".to_string()),
        max_tokens: Some(64),
        temperature: Some(0.2),
        ..Default::default()
    })?;
    assert_eq!(
        response.model, "conformance-model",
        "response reports the provider-selected model"
    );
    assert_eq!(
        response.content, "ping",
        "provider receives and handles the request"
    );
    assert_eq!(
        response.stop_reason.as_deref(),
        Some("end_turn"),
        "stop reason is preserved"
    );
    Ok(())
}

/// Execute the core `providers.embedding` seam suite (0050): no provider reports `UNSUPPORTED`, and
/// an installed provider is discoverable, batched, and dimension-checked by the core wrapper.
pub fn run_embedding_behavior() -> Result<()> {
    let unavailable = Embeddings::none();
    assert!(
        !unavailable.is_available(),
        "no-provider embedding must report unavailable"
    );
    assert_eq!(
        unavailable.model(),
        None,
        "no-provider embedding has no model profile"
    );
    assert_eq!(
        unavailable.embed(&["hello".to_string()]).unwrap_err().code,
        Code::Unsupported,
        "no-provider embedding returns UNSUPPORTED"
    );

    let embeddings = Embeddings::with_provider(Box::new(ConformanceEmbeddingProvider));
    assert!(
        embeddings.is_available(),
        "installed-provider embedding reports available"
    );
    assert_eq!(
        embeddings.model(),
        Some(EmbeddingModel::new(
            "conformance-embedding",
            2,
            Some("sha256:conformance".to_string())
        )),
        "model profile comes from the installed backend"
    );
    let vectors = embeddings.embed(&["a".to_string(), "bc".to_string()])?;
    assert_eq!(
        vectors,
        vec![vec![1.0, 97.0], vec![2.0, 197.0]],
        "provider receives and handles the whole batch"
    );
    Ok(())
}

/// Execute the workspace-scoped `cas` facade suite against a source and destination [`Loom`], over the
/// public `cas_put`/`cas_get`/`cas_has`/`cas_list` helpers: put returns the content address and
/// round-trips through get, has reports presence, list enumerates the workspace working tree, put is
/// idempotent (same digest, one entry), an unstored digest is absent (not an error), commit then
/// checkout versions the reachable blob set per workspace, and the reachable digest set survives a
/// `clone_workspace` and an offline `bundle` round-trip into the destination. This proves the facet
/// facade and its sync reachability, not just the object store underneath it.
pub fn run_cas_facade_behavior<S: ObjectStore, T: ObjectStore>(
    loom: &mut Loom<S>,
    dst: &mut Loom<T>,
) -> Result<()> {
    let ns = loom
        .registry_mut()
        .create(FacetKind::Cas, None, WorkspaceId::from_bytes([7; 16]))?;

    // put-get-has: the address is the content hash; get round-trips and has reports presence.
    let a = cas_put(loom, ns, b"alpha")?;
    assert_eq!(
        a,
        content_address(b"alpha"),
        "cas_put must return the content address"
    );
    assert_eq!(
        cas_get(loom, ns, &a)?.as_deref(),
        Some(&b"alpha"[..]),
        "cas_get must round-trip the stored bytes"
    );
    assert!(
        cas_has(loom, ns, &a)?,
        "cas_has must report a stored blob present"
    );
    assert_eq!(
        cas_list(loom, ns)?,
        vec![a],
        "cas_list must enumerate the workspace working tree"
    );

    // idempotent: identical bytes yield the same digest and a single entry (dedup).
    let a2 = cas_put(loom, ns, b"alpha")?;
    assert_eq!(a2, a, "cas_put is idempotent: same bytes -> same digest");
    assert_eq!(
        cas_list(loom, ns)?,
        vec![a],
        "an idempotent put adds no new entry"
    );

    // unknown-is-absent: an unstored digest reads as absent for both get and has, not an error.
    let unknown = content_address(b"never stored");
    assert!(
        !cas_has(loom, ns, &unknown)?,
        "an unstored digest must be absent for has"
    );
    assert_eq!(
        cas_get(loom, ns, &unknown)?,
        None,
        "an unstored digest must be absent for get"
    );

    // versioning: commit pins the one-blob set, a second blob commits, and checkout of the first commit
    // restores that commit's reachable set per workspace.
    let c1 = loom.commit(ns, "nas", "one blob", 1)?;
    let b = cas_put(loom, ns, b"beta")?;
    let c2 = loom.commit(ns, "nas", "two blobs", 2)?;
    assert_eq!(
        cas_list(loom, ns)?.len(),
        2,
        "both blobs are reachable after the second commit"
    );
    assert!(
        cas_has(loom, ns, &a)? && cas_has(loom, ns, &b)?,
        "both blobs present before checkout"
    );

    loom.checkout_commit(ns, c1)?;
    assert_eq!(
        cas_list(loom, ns)?,
        vec![a],
        "checkout restores the first commit's reachable set"
    );
    assert_eq!(
        cas_get(loom, ns, &a)?.as_deref(),
        Some(&b"alpha"[..]),
        "alpha still resolves after checkout"
    );
    assert!(
        !cas_has(loom, ns, &b)?,
        "beta is not reachable on the first commit"
    );

    // sync reachability via clone: the clone carries the committed CAS object closure, and per-commit
    // checkout in the destination restores the same reachable digest set the source committed.
    let (dst_ns, clone_report) = clone_workspace(loom, ns, dst, WorkspaceId::from_bytes([8; 16]))?;
    assert!(
        clone_report.objects_transferred > 0,
        "a clone into an empty destination must transfer the CAS object closure"
    );
    dst.checkout_commit(dst_ns, c2)?;
    assert_eq!(
        cas_list(dst, dst_ns)?.len(),
        2,
        "clone preserves the full reachable digest set at the later commit"
    );
    assert_eq!(
        cas_get(dst, dst_ns, &a)?.as_deref(),
        Some(&b"alpha"[..]),
        "a cloned blob round-trips by content address"
    );
    assert!(
        cas_has(dst, dst_ns, &b)?,
        "the later blob is reachable in the clone"
    );
    dst.checkout_commit(dst_ns, c1)?;
    assert_eq!(
        cas_list(dst, dst_ns)?,
        vec![a],
        "clone preserves per-commit reachability"
    );

    // sync reachability via bundle: drop the cloned workspace to free its name, export the source
    // workspace, decode it, import it under the source id, and confirm the reachable set survives an
    // offline transfer (object-addressed dedup is allowed).
    dst.registry_mut().delete(dst_ns)?;
    let bundle = bundle_export(loom, ns)?;
    let decoded = Bundle::decode(&bundle.encode())?;
    let (imported_ns, _) = bundle_import(dst, &decoded)?;
    dst.checkout_commit(imported_ns, c2)?;
    assert_eq!(
        cas_list(dst, imported_ns)?.len(),
        2,
        "bundle import preserves the full reachable digest set"
    );
    assert_eq!(
        cas_get(dst, imported_ns, &b)?.as_deref(),
        Some(&b"beta"[..]),
        "a bundled blob round-trips by content address"
    );

    // delete: drop a blob from the source working tree (now at the first commit, holding only `a`),
    // confirm it is unreachable and the delete is idempotent, then prove immutability - checking out
    // the later commit restores the dropped blob byte-for-byte.
    assert!(
        cas_delete(loom, ns, &a)?,
        "deleting a present blob reports true"
    );
    assert!(!cas_has(loom, ns, &a)?, "a deleted blob is unreachable");
    assert!(
        cas_get(loom, ns, &a)?.is_none(),
        "a deleted blob reads as absent"
    );
    assert!(
        !cas_delete(loom, ns, &a)?,
        "deleting an absent blob reports false"
    );
    loom.checkout_commit(ns, c2)?;
    assert!(
        cas_has(loom, ns, &a)? && cas_has(loom, ns, &b)?,
        "checkout restores a deleted blob (CAS stays immutable)"
    );
    Ok(())
}

/// Execute the workspace-scoped `kv` facade suite against a source and destination [`Loom`], over the
/// public `kv_put`/`kv_get`/`kv_delete`/`kv_list`/`kv_range` helpers: put-get round trip, a later put
/// replaces, an absent key reads as absent, a typed half-open range scans in `Value` order (so `Int(2)`
/// precedes `Int(10)`), delete reports presence and is a no-op when absent, commit then checkout
/// versions the map, and a `clone` preserves the committed map. This proves the facet facade, not just
/// the map blob underneath it.
pub fn run_kv_facade_behavior<S: ObjectStore, T: ObjectStore>(
    loom: &mut Loom<S>,
    dst: &mut Loom<T>,
) -> Result<()> {
    let ns = loom
        .registry_mut()
        .create(FacetKind::Kv, None, WorkspaceId::from_bytes([12; 16]))?;

    // put-get, replace, and absent-is-absent.
    kv_put(loom, ns, "m", Value::Int(1), b"one".to_vec())?;
    kv_put(loom, ns, "m", Value::Int(2), b"two".to_vec())?;
    assert_eq!(
        kv_get(loom, ns, "m", &Value::Int(1))?.as_deref(),
        Some(&b"one"[..]),
        "get returns the stored value"
    );
    kv_put(loom, ns, "m", Value::Int(1), b"uno".to_vec())?;
    assert_eq!(
        kv_get(loom, ns, "m", &Value::Int(1))?.as_deref(),
        Some(&b"uno"[..]),
        "a later put at the same key replaces the value"
    );
    assert_eq!(
        kv_get(loom, ns, "m", &Value::Int(9))?,
        None,
        "an absent key reads as absent, not an error"
    );

    // typed half-open range in Value order: keys are 1, 2, 10, so [2, 10) is exactly {2}.
    kv_put(loom, ns, "m", Value::Int(10), b"ten".to_vec())?;
    let in_range = kv_range(loom, ns, "m", &Value::Int(2), &Value::Int(10))?;
    assert_eq!(
        in_range.len(),
        1,
        "range [2,10) excludes 10 and 1 and orders numerically (2 before 10)"
    );
    assert!(
        in_range.get(&Value::Int(2)).is_some(),
        "range holds the in-bounds key"
    );

    // delete reports presence and is a no-op when absent.
    assert!(
        kv_delete(loom, ns, "m", &Value::Int(2))?,
        "deleting a present key reports true"
    );
    assert!(
        !kv_delete(loom, ns, "m", &Value::Int(2))?,
        "deleting an absent key reports false"
    );

    // versioning: commit pins the map, a later edit commits, and checkout restores the prior map.
    let c1 = loom.commit(ns, "conformance", "kv c1", 1)?;
    kv_put(loom, ns, "m", Value::Int(3), b"three".to_vec())?;
    loom.commit(ns, "conformance", "kv c2", 2)?;
    assert_eq!(
        kv_list(loom, ns, "m")?.len(),
        3,
        "after c2 the map holds keys 1, 10, and 3"
    );
    loom.checkout_commit(ns, c1)?;
    assert_eq!(
        kv_list(loom, ns, "m")?.len(),
        2,
        "checkout restores the c1 map (keys 1 and 10)"
    );

    // clone reachability: the cloned workspace carries the committed map and its values.
    let (dst_ns, clone_report) = clone_workspace(loom, ns, dst, WorkspaceId::from_bytes([13; 16]))?;
    assert!(
        clone_report.objects_transferred > 0,
        "a clone into an empty destination must transfer the kv object closure"
    );
    dst.checkout_commit(dst_ns, c1)?;
    assert_eq!(
        kv_get(dst, dst_ns, "m", &Value::Int(1))?.as_deref(),
        Some(&b"uno"[..]),
        "clone preserves the map values"
    );
    Ok(())
}

/// Execute the `kv-ephemeral` behavioral suite against a fresh Loom and an in-process cache.
pub fn run_ephemeral_kv_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let ns = loom
        .registry_mut()
        .create(FacetKind::Kv, None, WorkspaceId::from_bytes([41; 16]))?;

    let mut cache = EphemeralKvMap::new();
    cache.put(
        Value::Text("ttl".into()),
        b"value".to_vec(),
        EphemeralPutOptions {
            ttl_ms: Some(20),
            idle_ttl_ms: None,
        },
        100,
    )?;
    assert_eq!(
        cache.get(&Value::Text("ttl".into()), 119).as_deref(),
        Some(&b"value"[..]),
        "a ttl entry is readable before expiry"
    );
    assert_eq!(
        cache.get(&Value::Text("ttl".into()), 120),
        None,
        "a ttl entry is absent at its deadline"
    );

    cache.put(
        Value::Text("idle".into()),
        b"value".to_vec(),
        EphemeralPutOptions {
            ttl_ms: None,
            idle_ttl_ms: Some(10),
        },
        200,
    )?;
    assert!(
        cache.get(&Value::Text("idle".into()), 209).is_some(),
        "read before idle expiry succeeds and refreshes last access"
    );
    assert_eq!(
        cache.get(&Value::Text("idle".into()), 219),
        None,
        "idle ttl expires from the refreshed access time"
    );

    kv_put(
        loom,
        ns,
        "backing",
        Value::Text("k".into()),
        b"backed".to_vec(),
    )?;
    let read = ephemeral_kv_get_read_through(
        &mut cache,
        loom,
        ns,
        "backing",
        &Value::Text("k".into()),
        EphemeralPutOptions::default(),
        300,
    )?;
    assert_eq!(
        read.as_deref(),
        Some(&b"backed"[..]),
        "read-through returns the backing value"
    );
    assert_eq!(
        cache.get(&Value::Text("k".into()), 301).as_deref(),
        Some(&b"backed"[..]),
        "read-through populates the cache"
    );

    ephemeral_kv_put_write_through(
        &mut cache,
        loom,
        ns,
        "backing",
        Value::Text("w".into()),
        b"written".to_vec(),
        EphemeralPutOptions::default(),
        400,
    )?;
    let commit = loom.commit(ns, "conformance", "ephemeral write-through", 401)?;
    assert_eq!(
        kv_get(loom, ns, "backing", &Value::Text("w".into()))?.as_deref(),
        Some(&b"written"[..]),
        "write-through updates the backing versioned map"
    );
    loom.checkout_commit(ns, commit)?;
    assert_eq!(
        kv_get(loom, ns, "backing", &Value::Text("w".into()))?.as_deref(),
        Some(&b"written"[..]),
        "write-through backing data is versioned"
    );

    loom.configure_kv_map(
        ns,
        "configured",
        KvMapConfig {
            read_through: true,
            write_through: true,
            ..KvMapConfig::EPHEMERAL
        },
    )?;
    loom.kv_put_configured(
        ns,
        "configured",
        Value::Text("tier".into()),
        b"configured".to_vec(),
        None,
        500,
    )?;
    assert_eq!(
        loom.kv_get_configured(ns, "configured", &Value::Text("tier".into()), 501)?
            .as_deref(),
        Some(&b"configured"[..]),
        "tier-aware get reads the runtime cache"
    );
    let configured_commit = loom.commit(ns, "conformance", "configured ephemeral", 502)?;
    loom.checkout_commit(ns, configured_commit)?;
    assert_eq!(
        loom.kv_get_configured(ns, "configured", &Value::Text("tier".into()), 503)?
            .as_deref(),
        Some(&b"configured"[..]),
        "configured write-through survives checkout through versioned backing"
    );

    // Capacity + eviction: an LRU map bounded to two entries sheds its least-recently-used key.
    loom.configure_kv_map(
        ns,
        "evicting",
        KvMapConfig {
            max_entries: Some(2),
            eviction: EvictionPolicy::Lru,
            ..KvMapConfig::EPHEMERAL
        },
    )?;
    loom.kv_put_configured(ns, "evicting", Value::Int(1), b"a".to_vec(), None, 600)?;
    loom.kv_put_configured(ns, "evicting", Value::Int(2), b"b".to_vec(), None, 601)?;
    // Touch key 1 so key 2 is the least-recently-used victim when key 3 arrives.
    assert!(
        loom.kv_get_configured(ns, "evicting", &Value::Int(1), 602)?
            .is_some()
    );
    loom.kv_put_configured(ns, "evicting", Value::Int(3), b"c".to_vec(), None, 603)?;
    assert_eq!(
        loom.kv_list_configured(ns, "evicting", 604)?.len(),
        2,
        "the capacity bound holds after eviction"
    );
    assert_eq!(
        loom.kv_get_configured(ns, "evicting", &Value::Int(2), 605)?,
        None,
        "LRU eviction sheds the least-recently-used key"
    );

    // Write-behind: a put buffers the backing write; `flush_pending` drains it to the versioned map.
    loom.configure_kv_map(
        ns,
        "behind",
        KvMapConfig {
            write_behind: true,
            ..KvMapConfig::EPHEMERAL
        },
    )?;
    loom.kv_put_configured(ns, "behind", Value::Int(1), b"buffered".to_vec(), None, 700)?;
    assert_eq!(
        kv_get(loom, ns, "behind", &Value::Int(1))?,
        None,
        "write-behind does not touch the backing map until flushed"
    );
    assert_eq!(
        loom.pending_flush_count(ns, "behind"),
        1,
        "the dirty write is buffered"
    );
    assert_eq!(
        loom.flush_pending(ns, "behind", None)?,
        1,
        "flush drains the buffered write"
    );
    assert_eq!(
        kv_get(loom, ns, "behind", &Value::Int(1))?.as_deref(),
        Some(&b"buffered"[..]),
        "flushed write-behind reaches the backing map"
    );

    // Back-pressure: a saturated `pressure` cache rejects new writes with LOCKED.
    loom.configure_kv_map(
        ns,
        "pressure",
        KvMapConfig {
            write_behind: true,
            max_entries: Some(1),
            eviction: EvictionPolicy::Lru,
            back_pressure: BackPressure::Pressure,
            flush_high_water_pct: Some(100),
            ..KvMapConfig::EPHEMERAL
        },
    )?;
    loom.kv_put_configured(ns, "pressure", Value::Int(1), b"a".to_vec(), None, 710)?;
    assert_eq!(
        loom.kv_put_configured(ns, "pressure", Value::Int(2), b"b".to_vec(), None, 711)
            .unwrap_err()
            .code,
        Code::Locked,
        "Pressure back-pressure rejects writes at the high-water mark"
    );

    // Write-around: the backing map is written but the cache is not populated.
    loom.configure_kv_map(
        ns,
        "around",
        KvMapConfig {
            write_around: true,
            ..KvMapConfig::EPHEMERAL
        },
    )?;
    loom.kv_put_configured(ns, "around", Value::Int(1), b"v".to_vec(), None, 720)?;
    assert_eq!(
        kv_get(loom, ns, "around", &Value::Int(1))?.as_deref(),
        Some(&b"v"[..]),
        "write-around persists to the backing map"
    );
    assert_eq!(
        loom.kv_get_configured(ns, "around", &Value::Int(1), 721)?,
        None,
        "write-around does not populate the cache (no read-through)"
    );

    // GC sweep: an expired entry is reclaimed proactively, and the sweep is idempotent.
    loom.configure_kv_map(ns, "ttl", KvMapConfig::EPHEMERAL)?;
    loom.kv_put_configured(
        ns,
        "ttl",
        Value::Int(1),
        b"x".to_vec(),
        Some(EphemeralPutOptions {
            ttl_ms: Some(10),
            idle_ttl_ms: None,
        }),
        730,
    )?;
    assert_eq!(
        loom.sweep_expired(ns, "ttl", 745),
        1,
        "the GC sweep reclaims the expired entry"
    );
    assert_eq!(
        loom.sweep_expired(ns, "ttl", 745),
        0,
        "the GC sweep is idempotent"
    );

    // Checkout invalidation: a heated cache is dropped when the working tree is replaced.
    loom.configure_kv_map(
        ns,
        "invalidate",
        KvMapConfig {
            read_through: true,
            ..KvMapConfig::EPHEMERAL
        },
    )?;
    let inv_commit = loom.commit(ns, "conformance", "configure invalidate", 750)?;
    loom.kv_put_configured(ns, "invalidate", Value::Int(1), b"hot".to_vec(), None, 751)?;
    assert!(
        loom.kv_get_configured(ns, "invalidate", &Value::Int(1), 752)?
            .is_some(),
        "the cache serves the hot value before checkout"
    );
    loom.checkout_commit(ns, inv_commit)?;
    assert_eq!(
        loom.kv_get_configured(ns, "invalidate", &Value::Int(1), 753)?,
        None,
        "checkout drops the cache, so the read-through misses the empty backing"
    );
    Ok(())
}

/// Execute the workspace-scoped `document` facade suite over explicit text and binary operations:
/// text put/get with digest guards, binary put/get/list, delete reporting, commit/checkout versions
/// the collection, and clone preservation.
pub fn run_document_facade_behavior<S: ObjectStore, T: ObjectStore>(
    loom: &mut Loom<S>,
    dst: &mut Loom<T>,
) -> Result<()> {
    let ns =
        loom.registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([14; 16]))?;

    let digest = document_put_text(loom, ns, "c", "a", "one", None)?;
    document_put_text(loom, ns, "c", "b", "two", None)?;
    assert_eq!(
        document_get_text(loom, ns, "c", "a")?
            .as_ref()
            .map(|document| (document.text.as_str(), document.digest)),
        Some(("one", digest)),
        "text get returns the stored text and digest"
    );
    let stale_digest = Digest::hash(loom.store().digest_algo(), b"not-current");
    assert_eq!(
        document_put_text(loom, ns, "c", "a", "stale", Some(&stale_digest))
            .unwrap_err()
            .code,
        Code::CasMismatch,
        "stale text updates are rejected"
    );
    document_put_text(loom, ns, "c", "a", "uno", Some(&digest))?;
    assert_eq!(
        document_get_text(loom, ns, "c", "a")?
            .as_ref()
            .map(|document| document.text.as_str()),
        Some("uno"),
        "a guarded text put at the same id replaces the document"
    );
    assert_eq!(
        document_get_text(loom, ns, "c", "z")?,
        None,
        "an absent id reads as absent"
    );

    document_put_binary(loom, ns, "bin", "raw", vec![0xff, 0xfe], None)?;
    assert_eq!(
        document_get_binary(loom, ns, "bin", "raw")?
            .as_ref()
            .map(|document| document.bytes.as_slice()),
        Some(&[0xff, 0xfe][..]),
        "binary get returns exact bytes"
    );
    assert_eq!(
        document_get_text(loom, ns, "bin", "raw").unwrap_err().code,
        Code::DocumentNotText,
        "invalid UTF-8 maps to the stable document-not-text code"
    );

    assert!(
        doc_delete(loom, ns, "c", "b")?,
        "deleting a present id reports true"
    );
    assert!(
        !doc_delete(loom, ns, "c", "b")?,
        "deleting an absent id reports false"
    );

    document_put_binary(
        loom,
        ns,
        "people",
        "p1",
        br#"{"profile":{"age":31,"active":true,"city":"Paris"}}"#.to_vec(),
        None,
    )?;
    document_put_binary(
        loom,
        ns,
        "people",
        "p2",
        br#"{"profile":{"age":25,"active":true,"city":"Berlin"}}"#.to_vec(),
        None,
    )?;
    document_put_binary(
        loom,
        ns,
        "people",
        "p3",
        br#"{"profile":{"age":44,"active":true,"city":"Tokyo"}}"#.to_vec(),
        None,
    )?;
    doc_create_index(
        loom,
        ns,
        "people",
        DocumentIndexDef::new("age", DocumentFieldPath::dotted("profile.age")?, false)?,
    )?;
    assert_eq!(
        doc_find(loom, ns, "people", "age", &Value::Int(31))?,
        vec!["p1".to_string()],
        "indexed exact lookup finds the matching document id"
    );
    let statuses = doc_index_statuses(loom, ns, "people")?;
    assert_eq!(
        statuses
            .first()
            .map(|status| (status.ready, status.entries)),
        Some((true, 3)),
        "document index status reports readiness and entry count"
    );
    let first_page = document_query_from_json(&serde_json::json!({
        "predicate": {
            "and": [
                { "path": "profile.age", "op": ">=", "value": 30 },
                { "path": "profile.active", "op": "eq", "value": true }
            ]
        },
        "projections": [{ "name": "city", "path": "profile.city" }],
        "limit": 1
    }))?;
    let first_page = doc_query(loom, ns, "people", &first_page)?;
    assert_eq!(
        first_page.items.first().map(|item| item.id.as_str()),
        Some("p1"),
        "document query returns the first matching id"
    );
    assert_eq!(
        first_page.next_cursor.as_deref(),
        Some("p1"),
        "document query emits an id cursor when more results remain"
    );
    assert_eq!(
        first_page
            .items
            .first()
            .and_then(|item| item.projections.get("city"))
            .cloned()
            .flatten(),
        Some(Value::Text("Paris".to_string())),
        "document query returns selected scalar projections"
    );
    let json = document_query_result_json(first_page);
    assert_eq!(
        json["items"][0]["projections"]["city"],
        serde_json::Value::String("Paris".to_string()),
        "document query JSON result preserves projections"
    );

    let c1 = loom.commit(ns, "conformance", "doc c1", 1)?;
    document_put_text(loom, ns, "c", "c", "three", None)?;
    loom.commit(ns, "conformance", "doc c2", 2)?;
    assert_eq!(
        Collection::decode(&document_list_binary(loom, ns, "c")?)?.len(),
        2,
        "after c2 the collection holds a and c"
    );
    loom.checkout_commit(ns, c1)?;
    assert_eq!(
        Collection::decode(&document_list_binary(loom, ns, "c")?)?.len(),
        1,
        "checkout restores the c1 collection (just a)"
    );

    let (dst_ns, report) = clone_workspace(loom, ns, dst, WorkspaceId::from_bytes([15; 16]))?;
    assert!(
        report.objects_transferred > 0,
        "a clone must transfer the document object closure"
    );
    dst.checkout_commit(dst_ns, c1)?;
    assert_eq!(
        document_get_text(dst, dst_ns, "c", "a")?
            .as_ref()
            .map(|document| document.text.as_str()),
        Some("uno"),
        "clone preserves the documents"
    );
    Ok(())
}

/// Execute the workspace-scoped `time-series` facade suite over `ts_put`/`ts_get`/`ts_range`/
/// `ts_latest`: put-get, an absent timestamp reads as absent, a half-open range scans in time order,
/// latest returns the newest point, commit/checkout versions the series, and a clone preserves it.
pub fn run_timeseries_facade_behavior<S: ObjectStore, T: ObjectStore>(
    loom: &mut Loom<S>,
    dst: &mut Loom<T>,
) -> Result<()> {
    let ns = loom.registry_mut().create(
        FacetKind::TimeSeries,
        None,
        WorkspaceId::from_bytes([16; 16]),
    )?;

    for (t, v) in [(100i64, &b"a"[..]), (200, &b"b"[..]), (300, &b"c"[..])] {
        ts_put(loom, ns, "s", t, v.to_vec())?;
    }
    assert_eq!(
        ts_get(loom, ns, "s", 200)?.as_deref(),
        Some(&b"b"[..]),
        "get returns the point at a timestamp"
    );
    assert_eq!(
        ts_get(loom, ns, "s", 999)?,
        None,
        "an absent timestamp reads as absent"
    );
    let times: Vec<i64> = ts_range(loom, ns, "s", 100, 300)?
        .iter()
        .map(|(t, _)| t)
        .collect();
    assert_eq!(
        times,
        [100, 200],
        "range [100,300) is half-open and time-ordered"
    );
    assert_eq!(
        ts_latest(loom, ns, "s")?,
        Some((300, b"c".to_vec())),
        "latest is the newest point"
    );

    let tags = BTreeMap::from([("host".to_string(), "api-1".to_string())]);
    for (timestamp_ns, value) in [(100, 1), (150, 3), (250, 9)] {
        ts_put_point(
            loom,
            ns,
            "metrics",
            StructuredPoint::new(
                "cpu",
                tags.clone(),
                timestamp_ns,
                BTreeMap::from([("value".to_string(), TimeSeriesValue::Int(value))]),
            )?,
        )?;
    }
    ts_set_policy(
        loom,
        ns,
        "metrics",
        TimeSeriesPolicy {
            query_start_ns: Some(150),
            rollups: vec![TimeSeriesRollup::new(
                "hundred_mean",
                100,
                TimeSeriesAggregation::Mean,
            )?],
        },
    )?;
    ts_materialize_rollup(loom, ns, "metrics", "hundred_mean")?;
    let visible: Vec<i64> = ts_range_points(loom, ns, "metrics", 0, 300)?
        .into_iter()
        .map(|point| point.timestamp_ns)
        .collect();
    assert_eq!(
        visible,
        [150, 250],
        "query visibility hides raw points before the policy horizon"
    );
    let rollup = ts_range_rollup_points(loom, ns, "metrics", "hundred_mean", 0, 300)?;
    assert_eq!(rollup.len(), 2, "rollup materializes bucketed points");
    assert_eq!(
        rollup[0].fields.get("value"),
        Some(&TimeSeriesValue::Float(2.0)),
        "mean rollup aggregates numeric fields in a deterministic bucket"
    );

    let c1 = loom.commit(ns, "conformance", "ts c1", 1)?;
    ts_put(loom, ns, "s", 400, b"d".to_vec())?;
    loom.commit(ns, "conformance", "ts c2", 2)?;
    assert_eq!(
        ts_latest(loom, ns, "s")?,
        Some((400, b"d".to_vec())),
        "c2 latest is 400"
    );
    loom.checkout_commit(ns, c1)?;
    assert_eq!(
        ts_latest(loom, ns, "s")?,
        Some((300, b"c".to_vec())),
        "checkout restores c1 latest"
    );

    let (dst_ns, report) = clone_workspace(loom, ns, dst, WorkspaceId::from_bytes([17; 16]))?;
    assert!(
        report.objects_transferred > 0,
        "a clone must transfer the time-series object closure"
    );
    dst.checkout_commit(dst_ns, c1)?;
    assert_eq!(
        ts_get(dst, dst_ns, "s", 100)?.as_deref(),
        Some(&b"a"[..]),
        "clone preserves the points"
    );
    assert_eq!(
        ts_policy(dst, dst_ns, "metrics")?.query_start_ns,
        Some(150),
        "clone preserves time-series policy"
    );
    assert_eq!(
        ts_range_rollup_points(dst, dst_ns, "metrics", "hundred_mean", 0, 300)?.len(),
        2,
        "clone preserves materialized rollup roots"
    );
    assert_eq!(
        ts_prune_before(loom, ns, "metrics", 200)?,
        2,
        "explicit prune removes raw point fields before the cutoff"
    );
    assert_eq!(
        ts_range_points(loom, ns, "metrics", 0, 300)?
            .into_iter()
            .map(|point| point.timestamp_ns)
            .collect::<Vec<_>>(),
        [250],
        "prune removes authoritative raw points from current state"
    );
    assert_eq!(
        ts_range_rollup_points(loom, ns, "metrics", "hundred_mean", 0, 300)?.len(),
        2,
        "prune retains already materialized aggregate history"
    );
    Ok(())
}

/// Execute the workspace-scoped `ledger` facade suite over `ledger_append`/`ledger_get`/`ledger_head`/
/// `ledger_len`/`ledger_verify`: an absent ledger is empty and verifies, append assigns 0 then 1,
/// range scan is half-open and ordered, pruned retention ranges reject reads, the head is
/// profile-tagged, the chain verifies, commit/checkout versions the log, and a clone preserves a
/// verifiable chain.
pub fn run_ledger_facade_behavior<S: ObjectStore, T: ObjectStore>(
    loom: &mut Loom<S>,
    dst: &mut Loom<T>,
) -> Result<()> {
    use ed25519_dalek::Signer as _;

    let ns =
        loom.registry_mut()
            .create(FacetKind::Ledger, None, WorkspaceId::from_bytes([18; 16]))?;

    assert!(
        ledger_head(loom, ns, "audit")?.is_none(),
        "an absent ledger has no head"
    );
    assert_eq!(
        ledger_len(loom, ns, "audit")?,
        0,
        "an absent ledger has length 0"
    );
    ledger_verify(loom, ns, "audit")?;

    assert_eq!(
        ledger_append(loom, ns, "audit", b"e0".to_vec())?,
        0,
        "first append is sequence 0"
    );
    assert_eq!(
        ledger_append(loom, ns, "audit", b"e1".to_vec())?,
        1,
        "second append is sequence 1"
    );
    assert_eq!(ledger_len(loom, ns, "audit")?, 2);
    assert_eq!(
        ledger_get(loom, ns, "audit", 1)?.as_deref(),
        Some(&b"e1"[..]),
        "get returns the payload at a sequence"
    );
    let scan = ledger_range(loom, ns, "audit", 0, 2)?;
    assert_eq!(
        scan.state,
        LedgerRangeState::Retained,
        "unmarked ledger ranges are retained"
    );
    assert_eq!(
        scan.entries
            .iter()
            .map(|entry| (entry.seq, entry.payload.as_slice()))
            .collect::<Vec<_>>(),
        vec![(0, &b"e0"[..]), (1, &b"e1"[..])],
        "range scan is half-open and ordered by sequence"
    );
    ledger_append(loom, ns, "retention", b"r0".to_vec())?;
    ledger_append(loom, ns, "retention", b"r1".to_vec())?;
    ledger_set_retention_ranges(
        loom,
        ns,
        "retention",
        vec![LedgerRetentionRange {
            first_seq: 0,
            last_seq: 0,
            state: LedgerRangeState::Pruned,
        }],
    )?;
    assert_eq!(
        ledger_range(loom, ns, "retention", 0, 1).unwrap_err().code,
        Code::RetainedGap,
        "pruned ledger ranges reject range reads"
    );
    assert_eq!(
        ledger_get(loom, ns, "retention", 0).unwrap_err().code,
        Code::RetainedGap,
        "pruned ledger ranges reject point reads"
    );
    assert_eq!(
        ledger_range(loom, ns, "retention", 1, 2)?
            .entries
            .first()
            .map(|entry| entry.payload.as_slice()),
        Some(&b"r1"[..]),
        "unpruned ranges remain readable"
    );
    assert_eq!(
        ledger_append_with_mode(
            loom,
            ns,
            "authoritative",
            b"a0".to_vec(),
            LedgerAppendMode::Authoritative,
        )
        .unwrap_err()
        .code,
        Code::PermissionDenied,
        "authoritative append requires a fast-forward protected current branch"
    );
    loom.set_protected_ref_policy(
        ns,
        "branch/main",
        ProtectedRefPolicy {
            fast_forward_only: true,
            ..ProtectedRefPolicy::default()
        },
    )?;
    assert_eq!(
        ledger_append_with_mode(
            loom,
            ns,
            "authoritative",
            b"a0".to_vec(),
            LedgerAppendMode::Authoritative,
        )?,
        0,
        "authoritative append succeeds on a fast-forward protected current branch"
    );
    assert_eq!(
        ledger_head(loom, ns, "audit")?.unwrap().algo(),
        Algo::Blake3,
        "the head is tagged with the store's identity profile"
    );
    ledger_verify(loom, ns, "audit")?;

    let signer = WorkspaceId::from_bytes([20; 16]);
    let key_id = WorkspaceId::from_bytes([21; 16]);
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]);
    let mut identity = IdentityStore::new(signer);
    identity
        .add_public_key(
            signer,
            IdentityPublicKeySpec {
                id: key_id,
                label: "ledger-checkpoint".to_string(),
                algorithm: IDENTITY_SIGNATURE_SUITE_ED25519.to_string(),
                public_key: signing_key.verifying_key().to_bytes().to_vec(),
            },
        )
        .unwrap();
    loom.set_identity_store(identity);
    let checkpoint_payload = ledger_checkpoint_payload_bytes(loom, ns, "audit")?;
    let signed_payload = principal_signature_payload(
        signer,
        key_id,
        IDENTITY_SIGNATURE_SUITE_ED25519,
        LEDGER_CHECKPOINT_SIGNATURE_PURPOSE,
        &checkpoint_payload,
    )?;
    let signature = signing_key.sign(&signed_payload);
    ledger_attach_checkpoint_signature(
        loom,
        ns,
        "audit",
        signer,
        key_id,
        IDENTITY_SIGNATURE_SUITE_ED25519,
        signature.to_bytes().to_vec(),
    )?;
    assert_eq!(
        ledger_verify_checkpoint_signatures(loom, ns, "audit")?,
        1,
        "stored ledger checkpoint signatures verify against principal keys"
    );
    ledger_append(loom, ns, "audit", b"e2".to_vec())?;
    assert_eq!(
        ledger_verify_checkpoint_signatures(loom, ns, "audit")?,
        0,
        "append clears stale ledger checkpoint signatures"
    );
    let proof_tree = ledger_proof_tree(loom, ns, "audit")?;
    assert_eq!(
        proof_tree.tree_size, 3,
        "ledger proof tree covers the current append sequence"
    );
    let inclusion = ledger_inclusion_proof(loom, ns, "audit", 2)?;
    ledger_verify_inclusion_proof(&inclusion)?;
    assert_eq!(
        inclusion.tree.root_hash, proof_tree.root_hash,
        "inclusion proof targets the current derived proof root"
    );
    let consistency = ledger_consistency_proof(loom, ns, "audit", 2, 3)?;
    ledger_verify_consistency_proof(&consistency)?;
    assert_eq!(
        consistency.second_root_hash, proof_tree.root_hash,
        "consistency proof reaches the current derived proof root"
    );

    let c1 = loom.commit(ns, "conformance", "ledger c1", 1)?;
    ledger_append(loom, ns, "audit", b"e3".to_vec())?;
    loom.commit(ns, "conformance", "ledger c2", 2)?;
    assert_eq!(ledger_len(loom, ns, "audit")?, 4);
    loom.checkout_commit(ns, c1)?;
    assert_eq!(
        ledger_len(loom, ns, "audit")?,
        3,
        "checkout restores the prior chain length"
    );

    let (dst_ns, report) = clone_workspace(loom, ns, dst, WorkspaceId::from_bytes([19; 16]))?;
    assert!(
        report.objects_transferred > 0,
        "a clone must transfer the ledger object closure"
    );
    dst.checkout_commit(dst_ns, c1)?;
    ledger_verify(dst, dst_ns, "audit")?;
    assert_eq!(
        ledger_get(dst, dst_ns, "audit", 0)?.as_deref(),
        Some(&b"e0"[..]),
        "clone preserves a verifiable chain"
    );
    Ok(())
}

/// Execute the `graph` behavioral suite: node/edge upsert, directed traversal, edge removal,
/// versioning, and clone reachability.
pub fn run_graph_facade_behavior<S: ObjectStore, T: ObjectStore>(
    loom: &mut Loom<S>,
    dst: &mut Loom<T>,
) -> Result<()> {
    let ns =
        loom.registry_mut()
            .create(FacetKind::Graph, None, WorkspaceId::from_bytes([20; 16]))?;

    graph::graph_upsert_node(loom, ns, "g", "a", Props::new())?;
    graph::graph_upsert_node(loom, ns, "g", "b", Props::new())?;
    graph::graph_upsert_node(loom, ns, "g", "c", Props::new())?;
    graph::graph_upsert_node_with_labels(
        loom,
        ns,
        "g",
        "person",
        BTreeSet::from(["Person".to_string()]),
        BTreeMap::from([("name".to_string(), GraphValue::Text("Ada".to_string()))]),
    )?;
    graph::graph_upsert_node_with_labels(
        loom,
        ns,
        "g",
        "org",
        BTreeSet::from(["Organization".to_string()]),
        BTreeMap::from([("name".to_string(), GraphValue::Text("Uldren".to_string()))]),
    )?;
    graph::graph_upsert_edge(loom, ns, "g", "e1", "a", "b", "rel", Props::new())?;
    graph::graph_upsert_edge(loom, ns, "g", "e2", "b", "c", "rel", Props::new())?;
    graph::graph_upsert_edge(
        loom,
        ns,
        "g",
        "employment",
        "person",
        "org",
        "WORKS_AT",
        Props::new(),
    )?;
    assert!(
        graph::graph_get_node(loom, ns, "g", "a")?.is_some(),
        "a node round-trips"
    );
    assert_eq!(
        graph::graph_neighbors(loom, ns, "g", "a")?,
        vec!["b".to_string()],
        "neighbours are directed and sorted"
    );
    assert_eq!(
        graph::graph_reachable(loom, ns, "g", "a", None, None)?,
        vec!["b".to_string(), "c".to_string()],
        "reachable follows edges transitively"
    );
    graph::graph_upsert_node(loom, ns, "g", "person", Props::new())?;
    assert_eq!(
        graph::graph_get_node_labels(loom, ns, "g", "person")?,
        Some(BTreeSet::from(["Person".to_string()])),
        "property-only upsert preserves canonical labels"
    );
    graph::graph_upsert_node_with_labels(
        loom,
        ns,
        "g",
        "teammate",
        BTreeSet::from(["Person".to_string()]),
        BTreeMap::from([("name".to_string(), GraphValue::Text("Grace".to_string()))]),
    )?;
    graph::graph_upsert_node_with_labels(
        loom,
        ns,
        "g",
        "person",
        BTreeSet::from(["Person".to_string()]),
        BTreeMap::from([
            ("name".to_string(), GraphValue::Text("Ada".to_string())),
            (
                "loc".to_string(),
                GraphValue::Geometry(GraphGeometry::point(GraphCrs::Crs84_2d, 12.5, 55.0, None)?),
            ),
        ]),
    )?;
    graph::graph_upsert_node_with_labels(
        loom,
        ns,
        "g",
        "teammate",
        BTreeSet::from(["Person".to_string()]),
        BTreeMap::from([
            ("name".to_string(), GraphValue::Text("Grace".to_string())),
            (
                "loc".to_string(),
                GraphValue::Geometry(GraphGeometry::point(GraphCrs::Crs84_2d, 13.0, 55.0, None)?),
            ),
        ]),
    )?;
    graph::graph_upsert_edge(
        loom,
        ns,
        "g",
        "employment2",
        "teammate",
        "org",
        "WORKS_AT",
        Props::new(),
    )?;
    let query = GraphQuery::parse_opencypher(
        "MATCH (p:Person)-[r:WORKS_AT]->(o:Organization) \
         WHERE o.name = 'Uldren' RETURN p, r, o.name ORDER BY r ASC LIMIT 1",
    )?;
    let result = graph::graph_query(loom, ns, "g", &query)?;
    assert_eq!(result.rows.len(), 1, "native graph query returns one row");
    let row = &result.rows[0];
    assert!(
        matches!(
            row.get("p"),
            Some(GraphQueryValue::Node(GraphQueryNode { id, labels, .. }))
                if id == "person" && labels.contains("Person")
        ),
        "native graph query returns openCypher-style node values"
    );
    assert!(
        matches!(
            row.get("r"),
            Some(GraphQueryValue::Edge(GraphQueryEdge { id, label, .. }))
                if id == "employment" && label == "WORKS_AT"
        ),
        "native graph query returns relationship values"
    );
    assert_eq!(
        row.get("o.name"),
        Some(&GraphQueryValue::Scalar(GraphValue::Text(
            "Uldren".to_string()
        ))),
        "native graph query returns scalar projections"
    );
    let aggregate_query = GraphQuery::parse_opencypher(
        "MATCH (p:Person)-[r:WORKS_AT]->(o:Organization) \
         WHERE o.name <> 'Other' RETURN o.name, count(p) AS people \
         ORDER BY o.name ASC SKIP 0 LIMIT 10",
    )?;
    let aggregate = graph::graph_query(loom, ns, "g", &aggregate_query)?;
    assert_eq!(
        aggregate.rows.len(),
        1,
        "native graph grouped aggregate returns one row"
    );
    assert_eq!(
        aggregate.rows[0].get("o.name"),
        Some(&GraphQueryValue::Scalar(GraphValue::Text(
            "Uldren".to_string()
        ))),
        "native graph grouped aggregate preserves the group key"
    );
    assert_eq!(
        aggregate.rows[0].get("people"),
        Some(&GraphQueryValue::Scalar(GraphValue::Int(2))),
        "native graph grouped aggregate counts matching bindings"
    );
    let regex_query = GraphQuery::parse_opencypher(
        "MATCH (p:Person) WHERE p.name =~ '^G.*' RETURN p.name LIMIT 10",
    )?;
    let regex = graph::graph_query(loom, ns, "g", &regex_query)?;
    assert_eq!(
        regex.rows.len(),
        1,
        "native graph regex predicate uses deterministic non-backtracking matching"
    );
    let geo_query = GraphQuery::parse_opencypher(
        "MATCH (p:Person) WHERE distance(p.loc, point('crs84_2d', 12.5, 55.0)) <= 1 \
         AND within_bbox(p.loc, 12.0, 54.0, 12.6, 56.0) RETURN p ORDER BY id(p)",
    )?;
    let geo = graph::graph_query(loom, ns, "g", &geo_query)?;
    assert_eq!(
        geo.rows.len(),
        1,
        "native graph geospatial predicates filter point values"
    );
    assert!(
        matches!(
            geo.rows[0].get("p"),
            Some(GraphQueryValue::Node(GraphQueryNode { id, .. })) if id == "person"
        ),
        "native graph geospatial predicates preserve matching bindings"
    );
    let function_query = GraphQuery::parse_opencypher(
        "MATCH (p:Person)-[r:WORKS_AT]->(o:Organization) \
         RETURN id(p) AS pid, type(r) AS rel, startNode(r) AS start, endNode(r) AS finish \
         ORDER BY id(p) LIMIT 1",
    )?;
    let functions = graph::graph_query(loom, ns, "g", &function_query)?;
    assert_eq!(
        functions.rows[0].get("rel"),
        Some(&GraphQueryValue::Scalar(GraphValue::Text(
            "WORKS_AT".to_string()
        ))),
        "native graph scalar function registry returns relationship type"
    );
    assert!(
        matches!(
            functions.rows[0].get("start"),
            Some(GraphQueryValue::Node(GraphQueryNode { labels, .. }))
                if labels.contains("Person")
        ),
        "native graph function registry returns endpoint node values"
    );
    let list_map_query = GraphQuery::parse_opencypher(
        "MATCH p = (p1:Person)-[r:WORKS_AT]->(o:Organization) \
         RETURN labels(p1) AS labels, keys(o) AS keys, properties(r) AS props, nodes(p) AS nodes, relationships(p) AS rels \
         ORDER BY id(p1) LIMIT 1",
    )?;
    let list_map = graph::graph_query(loom, ns, "g", &list_map_query)?;
    assert!(
        matches!(
            list_map.rows[0].get("labels"),
            Some(GraphQueryValue::List(values))
                if values.contains(&GraphQueryValue::Scalar(GraphValue::Text("Person".to_string())))
        ),
        "native graph list function returns deterministic label values"
    );
    assert!(
        matches!(
            list_map.rows[0].get("keys"),
            Some(GraphQueryValue::List(values))
                if values.contains(&GraphQueryValue::Scalar(GraphValue::Text("name".to_string())))
        ),
        "native graph keys function returns deterministic property names"
    );
    assert!(
        matches!(
            list_map.rows[0].get("props"),
            Some(GraphQueryValue::Map(values)) if values.is_empty()
        ),
        "native graph properties function returns a deterministic property map"
    );
    assert!(
        matches!(
            list_map.rows[0].get("nodes"),
            Some(GraphQueryValue::List(values))
                if matches!(values.first(), Some(GraphQueryValue::Node(GraphQueryNode { labels, .. })) if labels.contains("Person"))
        ),
        "native graph nodes function returns path node values"
    );
    assert!(
        matches!(
            list_map.rows[0].get("rels"),
            Some(GraphQueryValue::List(values))
                if matches!(values.first(), Some(GraphQueryValue::Edge(GraphQueryEdge { label, .. })) if label == "WORKS_AT")
        ),
        "native graph relationships function returns path edge values"
    );
    loom.registry_mut().add_facet(ns, FacetKind::Search)?;
    let mut mapping = search::Mapping::new();
    mapping.insert("bio".to_string(), search::FieldMapping::text());
    search::search_create(loom, ns, "graph_people", mapping)?;
    let mut person_text = search::Document::new();
    person_text.insert(
        "bio".to_string(),
        search::FieldValue::Text("analytical engine researcher".to_string()),
    );
    search::search_index(loom, ns, "graph_people", b"person".to_vec(), person_text)?;
    let mut teammate_text = search::Document::new();
    teammate_text.insert(
        "bio".to_string(),
        search::FieldValue::Text("compiler systems pioneer".to_string()),
    );
    search::search_index(
        loom,
        ns,
        "graph_people",
        b"teammate".to_vec(),
        teammate_text,
    )?;
    let full_text_request = search::QueryRequest::new(
        search::Query::Match {
            field: "bio".to_string(),
            text: "compiler".to_string(),
        },
        10,
        0,
    );
    let full_text = graph::graph_query_with_full_text(
        loom,
        ns,
        "g",
        &GraphQuery::parse_opencypher("MATCH (p:Person) RETURN p ORDER BY id(p)")?,
        "p",
        "graph_people",
        &full_text_request,
    )?;
    assert_eq!(
        full_text.rows.len(),
        1,
        "native graph full-text bridge filters graph bindings through FTS hits"
    );
    assert!(
        matches!(
            full_text.rows[0].get("p"),
            Some(GraphQueryValue::Node(GraphQueryNode { id, .. })) if id == "teammate"
        ),
        "native graph full-text bridge treats FTS document ids as graph entity ids"
    );
    let mut index_graph = graph::get_graph(loom, ns, "g")?;
    index_graph.declare_property_index("org_name", GraphIndexEntity::Node, "name")?;
    let index_query =
        GraphQuery::parse_opencypher("MATCH (o:Organization) WHERE o.name = 'Uldren' RETURN o")?;
    let not_built = index_graph.explain_query(&index_query)?;
    assert!(
        not_built.fallback_scan,
        "declared but unbuilt graph property index requires fallback scan"
    );
    assert!(
        matches!(
            not_built.selections.first(),
            Some(selection)
                if selection.index.as_deref() == Some("org_name")
                    && selection.status == GraphIndexStatus::NotBuilt
        ),
        "graph explain reports an unbuilt declared index"
    );
    index_graph.rebuild_property_indexes()?;
    let ready = index_graph.explain_query(&index_query)?;
    assert!(
        !ready.fallback_scan,
        "ready graph property index is selected for equality lookup"
    );
    assert!(
        matches!(
            ready.selections.first(),
            Some(selection)
                if selection.index.as_deref() == Some("org_name")
                    && selection.status == GraphIndexStatus::Ready
        ),
        "graph explain reports the selected ready index"
    );
    index_graph.set_node_property("org", "name", GraphValue::Text("Uldren Labs".to_string()))?;
    let stale = index_graph.explain_query(&index_query)?;
    assert!(
        stale.fallback_scan,
        "stale graph property index is not selected"
    );
    assert!(
        matches!(
            stale.selections.first(),
            Some(selection)
                if selection.index.as_deref() == Some("org_name")
                    && selection.status == GraphIndexStatus::Stale
        ),
        "graph explain reports stale index materialization"
    );
    let fixed_path_query = GraphQuery::parse_opencypher(
        "MATCH p = (p1:Person)-[r1:WORKS_AT]->(o:Organization) RETURN p ORDER BY p LIMIT 1",
    )?;
    let fixed_path = graph::graph_query(loom, ns, "g", &fixed_path_query)?;
    assert_eq!(
        fixed_path.rows.len(),
        1,
        "native graph fixed path query returns one row"
    );
    assert!(
        matches!(
            fixed_path.rows[0].get("p"),
            Some(GraphQueryValue::Path(GraphPath { nodes, edges }))
                if nodes.len() == 2 && edges.len() == 1
        ),
        "native graph fixed path query returns a path value"
    );
    let variable_path_query = GraphQuery::parse_opencypher(
        "MATCH p = (p1:Person)-[:WORKS_AT*1..2]->(o:Organization) RETURN p, length(p) AS hops ORDER BY length(p), p LIMIT 10",
    )?;
    let variable_path = graph::graph_query(loom, ns, "g", &variable_path_query)?;
    assert_eq!(
        variable_path.rows.len(),
        2,
        "native graph bounded variable path query returns all simple matching paths"
    );
    assert_eq!(
        variable_path.rows[0].get("hops"),
        Some(&GraphQueryValue::Scalar(GraphValue::Int(1))),
        "native graph path length function returns hop count"
    );
    let shortest_path_query = GraphQuery::parse_opencypher(
        "MATCH p = shortestPath((p1:Person)-[:WORKS_AT*1..2]->(o:Organization)) \
         RETURN p, length(p) AS hops",
    )?;
    let shortest_path = graph::graph_query(loom, ns, "g", &shortest_path_query)?;
    assert_eq!(
        shortest_path.rows.len(),
        2,
        "native graph bounded shortestPath returns one shortest path for each endpoint pair"
    );
    assert_eq!(
        shortest_path.rows[0].get("hops"),
        Some(&GraphQueryValue::Scalar(GraphValue::Int(1))),
        "native graph bounded shortestPath returns shortest hop count"
    );
    let mutation_plan = GraphMutationPlan::new(vec![
        GraphMutation::CreateNode {
            id: "reviewer".to_string(),
            labels: BTreeSet::from(["Person".to_string()]),
            props: BTreeMap::from([("name".to_string(), GraphValue::Text("Lin".to_string()))]),
        },
        GraphMutation::CreateEdge {
            id: "review".to_string(),
            src: "reviewer".to_string(),
            dst: "org".to_string(),
            label: "REVIEWS".to_string(),
            props: Props::new(),
        },
        GraphMutation::SetEdgeProperty {
            id: "review".to_string(),
            property: "since".to_string(),
            value: GraphValue::Int(2026),
        },
    ]);
    assert_eq!(
        graph::graph_apply_mutations(loom, ns, "g", &mutation_plan)?.applied,
        3,
        "native graph mutation plan reports applied operations"
    );
    assert_eq!(
        graph::graph_get_edge(loom, ns, "g", "review")?
            .and_then(|edge| edge.props.get("since").cloned()),
        Some(GraphValue::Int(2026)),
        "native graph mutation plan can create edges and set properties"
    );
    let identity = GraphMutationIdentity::new(
        BTreeMap::from([
            ("p".to_string(), "reviewer-2".to_string()),
            ("o".to_string(), "org-2".to_string()),
        ]),
        BTreeMap::from([("r".to_string(), "review-2".to_string())]),
    );
    let text_plan = GraphMutationPlan::parse_opencypher(
        "CREATE (p:Person {name: 'Mira'})-[r:REVIEWS {since: 2026}]->(o:Organization {name: 'Uldren Labs'})",
        &identity,
    )?;
    assert_eq!(
        graph::graph_apply_mutations(loom, ns, "g", &text_plan)?.applied,
        3,
        "openCypher mutation text lowers through explicit Loom identity"
    );
    assert!(
        graph::graph_get_node(loom, ns, "g", "reviewer-2")?.is_some(),
        "identity envelope supplies canonical node id"
    );
    let merge_identity = GraphMutationIdentity::new(
        BTreeMap::from([
            ("p".to_string(), "reviewer-3".to_string()),
            ("o".to_string(), "org-3".to_string()),
        ]),
        BTreeMap::from([("r".to_string(), "review-3".to_string())]),
    );
    let merge_plan = GraphMutationPlan::parse_opencypher(
        "MERGE (p:Person {name: 'Nia'})-[r:REVIEWS {since: 2026}]->(o:Organization {name: 'Uldren Labs'})",
        &merge_identity,
    )?;
    assert_eq!(
        graph::graph_apply_mutations(loom, ns, "g", &merge_plan)?.applied,
        3,
        "openCypher MERGE creates through explicit Loom identity"
    );
    assert_eq!(
        graph::graph_apply_mutations(loom, ns, "g", &merge_plan)?.applied,
        3,
        "openCypher MERGE is idempotent through explicit Loom identity"
    );
    let conflict_plan =
        GraphMutationPlan::parse_opencypher("MERGE (p:Person {name: 'Other'})", &merge_identity)?;
    assert_eq!(
        graph::graph_apply_mutations(loom, ns, "g", &conflict_plan)
            .unwrap_err()
            .code,
        Code::Conflict,
        "openCypher MERGE rejects identities whose stored shape does not match"
    );

    // A missing endpoint is rejected; edge removal reports presence.
    assert_eq!(
        graph::graph_upsert_edge(loom, ns, "g", "x", "a", "ghost", "rel", Props::new())
            .unwrap_err()
            .code,
        Code::NotFound,
        "an edge to a missing node is NOT_FOUND"
    );
    assert!(
        graph::graph_remove_edge(loom, ns, "g", "e1")?,
        "removing a present edge reports true"
    );

    let c1 = loom.commit(ns, "conformance", "graph c1", 1)?;
    graph::graph_upsert_node(loom, ns, "g", "d", Props::new())?;
    loom.commit(ns, "conformance", "graph c2", 2)?;
    loom.checkout_commit(ns, c1)?;
    assert!(
        graph::graph_get_node(loom, ns, "g", "d")?.is_none(),
        "checkout restores the c1 graph"
    );

    let (dst_ns, report) = clone_workspace(loom, ns, dst, WorkspaceId::from_bytes([21; 16]))?;
    assert!(
        report.objects_transferred > 0,
        "a clone must transfer the graph object closure"
    );
    dst.checkout_commit(dst_ns, c1)?;
    assert!(
        graph::graph_get_node(dst, dst_ns, "g", "a")?.is_some(),
        "clone preserves the graph"
    );
    Ok(())
}

/// Execute the `vector` behavioral suite: create, upsert/get/remove, exact knn, dimension-mismatch,
/// versioning, and clone reachability (the derived ANN index is never stored; only the embeddings
/// travel).
pub fn run_vector_facade_behavior<S: ObjectStore, T: ObjectStore>(
    loom: &mut Loom<S>,
    dst: &mut Loom<T>,
) -> Result<()> {
    let ns =
        loom.registry_mut()
            .create(FacetKind::Vector, None, WorkspaceId::from_bytes([22; 16]))?;

    vector::vector_create(loom, ns, "emb", 2, vector::Metric::Cosine)?;
    let mut en = std::collections::BTreeMap::new();
    en.insert("lang".to_string(), Value::Text("en".into()));
    let mut fr = std::collections::BTreeMap::new();
    fr.insert("lang".to_string(), Value::Text("fr".into()));
    vector::vector_upsert(loom, ns, "emb", "a", vec![1.0, 0.0], en.clone())?;
    vector::vector_upsert(loom, ns, "emb", "b", vec![0.0, 1.0], fr)?;
    vector::vector_upsert(loom, ns, "emb", "c", vec![0.9, 0.1], en.clone())?;
    assert!(
        vector::vector_create_metadata_index(loom, ns, "emb", "lang")?,
        "metadata index creation reports a new declaration"
    );
    assert!(
        !vector::vector_create_metadata_index(loom, ns, "emb", "lang")?,
        "metadata index creation is idempotent"
    );
    assert_eq!(
        vector::vector_metadata_index_keys(loom, ns, "emb")?,
        vec!["lang".to_string()],
        "metadata index declarations are listed in ascending order"
    );
    assert!(
        vector::vector_get(loom, ns, "emb", "a")?.is_some(),
        "an upsert round-trips"
    );
    assert_eq!(
        vector::vector_ids(loom, ns, "emb", None)?,
        vec!["a".to_string(), "b".to_string(), "c".to_string()],
        "ids are listed in ascending order"
    );
    assert_eq!(
        vector::vector_ids(loom, ns, "emb", Some("b"))?,
        vec!["b".to_string()],
        "ids can be filtered by string prefix"
    );
    let hits = vector::vector_search(loom, ns, "emb", &[1.0, 0.0], 2, &vector::MetaFilter::All)?;
    assert_eq!(
        hits.iter().map(|h| h.id.clone()).collect::<Vec<_>>(),
        vec!["a".to_string(), "c".to_string()],
        "knn returns the two nearest, ties broken by id"
    );
    let filtered_hits = vector::vector_search(
        loom,
        ns,
        "emb",
        &[1.0, 0.0],
        3,
        &vector::MetaFilter::Eq("lang".into(), Value::Text("en".into())),
    )?;
    assert_eq!(
        filtered_hits
            .iter()
            .map(|h| h.id.clone())
            .collect::<Vec<_>>(),
        vec!["a".to_string(), "c".to_string()],
        "metadata-indexed exact search returns filtered candidates"
    );
    assert_eq!(
        vector::vector_upsert(
            loom,
            ns,
            "emb",
            "x",
            vec![1.0],
            std::collections::BTreeMap::new()
        )
        .unwrap_err()
        .code,
        Code::DimensionMismatch,
        "a wrong-width vector is rejected"
    );
    assert!(
        vector::vector_delete(loom, ns, "emb", "a")?,
        "delete reports presence"
    );
    assert!(
        vector::vector_get(loom, ns, "emb", "a")?.is_none(),
        "a deleted vector is absent"
    );
    assert!(
        vector::vector_drop_metadata_index(loom, ns, "emb", "lang")?,
        "metadata index drop reports a removed declaration"
    );
    assert!(
        !vector::vector_drop_metadata_index(loom, ns, "emb", "lang")?,
        "metadata index drop is idempotent"
    );
    assert!(
        vector::vector_metadata_index_keys(loom, ns, "emb")?.is_empty(),
        "metadata index declarations are absent after drop"
    );

    let c1 = loom.commit(ns, "conformance", "vec c1", 1)?;
    vector::vector_upsert(
        loom,
        ns,
        "emb",
        "d",
        vec![0.5, 0.5],
        std::collections::BTreeMap::new(),
    )?;
    loom.commit(ns, "conformance", "vec c2", 2)?;
    loom.checkout_commit(ns, c1)?;
    assert!(
        vector::vector_get(loom, ns, "emb", "d")?.is_none(),
        "checkout restores the c1 set"
    );

    let (dst_ns, report) = clone_workspace(loom, ns, dst, WorkspaceId::from_bytes([23; 16]))?;
    assert!(
        report.objects_transferred > 0,
        "a clone must transfer the embeddings"
    );
    dst.checkout_commit(dst_ns, c1)?;
    assert!(
        vector::vector_get(dst, dst_ns, "emb", "b")?.is_some(),
        "clone preserves the embeddings"
    );
    Ok(())
}

/// Execute the `columnar` behavioral suite: create, append, ordered scan, the StateAccess predicate
/// select matching a row scan, versioning, and clone reachability.
pub fn run_columnar_facade_behavior<S: ObjectStore, T: ObjectStore>(
    loom: &mut Loom<S>,
    dst: &mut Loom<T>,
) -> Result<()> {
    let ns =
        loom.registry_mut()
            .create(FacetKind::Columnar, None, WorkspaceId::from_bytes([24; 16]))?;

    let cols = vec![
        ("id".to_string(), ColumnType::Int),
        ("price".to_string(), ColumnType::Text),
    ];
    columnar::columnar_create(loom, ns, "t", cols, 0)?;
    columnar::columnar_append(loom, ns, "t", vec![Value::Int(1), Value::Text("10".into())])?;
    columnar::columnar_append(loom, ns, "t", vec![Value::Int(2), Value::Text("20".into())])?;
    let rows = columnar::columnar_scan(loom, ns, "t")?;
    assert_eq!(rows.len(), 2, "append-then-scan returns both rows");
    assert_eq!(
        rows[0][1],
        Value::Text("10".into()),
        "scan preserves append order"
    );

    let selected = columnar::columnar_select(
        loom,
        ns,
        "t",
        &["price"],
        Some(("id", CmpOp::Ge, &Value::Int(2))),
    )?;
    assert_eq!(
        selected.len(),
        1,
        "the StateAccess predicate select matches a row scan"
    );
    assert_eq!(selected[0][0], Value::Text("20".into()));

    let c1 = loom.commit(ns, "conformance", "col c1", 1)?;
    columnar::columnar_append(loom, ns, "t", vec![Value::Int(3), Value::Text("30".into())])?;
    loom.commit(ns, "conformance", "col c2", 2)?;
    assert_eq!(
        columnar::columnar_rows(loom, ns, "t")?,
        3,
        "c2 has three rows"
    );
    loom.checkout_commit(ns, c1)?;
    assert_eq!(
        columnar::columnar_rows(loom, ns, "t")?,
        2,
        "checkout restores the c1 dataset"
    );

    let (dst_ns, report) = clone_workspace(loom, ns, dst, WorkspaceId::from_bytes([25; 16]))?;
    assert!(
        report.objects_transferred > 0,
        "a clone must transfer the columnar object closure"
    );
    dst.checkout_commit(dst_ns, c1)?;
    assert_eq!(
        columnar::columnar_rows(dst, dst_ns, "t")?,
        2,
        "clone preserves the dataset"
    );
    Ok(())
}

/// Execute the `dataframe` behavioral suite: store a logical plan, load CSV through the files facet,
/// execute deterministic filter/select/sort, materialize to columnar, verify version checkout, and
/// clone reachability.
pub fn run_dataframe_facade_behavior<S: ObjectStore, T: ObjectStore>(
    loom: &mut Loom<S>,
    dst: &mut Loom<T>,
) -> Result<()> {
    let ns = loom.registry_mut().create(
        FacetKind::Dataframe,
        None,
        WorkspaceId::from_bytes([27; 16]),
    )?;
    loom.registry_mut().add_facet(ns, FacetKind::Files)?;
    loom.registry_mut().add_facet(ns, FacetKind::Columnar)?;
    loom.create_directory(ns, "inputs", true)?;
    loom.write_file(
        ns,
        "inputs/events.csv",
        b"id,kind,total\n1,purchase,10.5\n2,view,0\n3,purchase,7.5\n",
        0o100644,
    )?;

    let plan = dataframe::DataframePlan::new(vec![dataframe::DataframeSourceBinding::new(
        "events",
        dataframe::DataframeSourceKind::Files,
        "inputs/events.csv",
        dataframe::DataframeInputFormat::Csv,
    )])?
    .with_operations(vec![
        dataframe::DataframeOperation::Scan {
            source: "events".into(),
        },
        dataframe::DataframeOperation::Filter {
            expression: "kind == \"purchase\"".into(),
        },
        dataframe::DataframeOperation::Select {
            columns: vec!["id".into(), "total".into()],
        },
        dataframe::DataframeOperation::Sort {
            columns: vec!["id".into()],
            descending: true,
        },
    ])?
    .with_materialization(dataframe::DataframeMaterialization::new(
        dataframe::DataframeMaterializationTarget::Columnar,
        Some("analytics/purchases".into()),
        dataframe::DataframeInputFormat::Parquet,
    ))?;
    dataframe::dataframe_create(loom, ns, "etl/purchases", &plan)?;

    let batch = dataframe::dataframe_collect(loom, ns, "etl/purchases")?;
    assert_eq!(batch.row_count(), 2, "filter keeps two purchase rows");
    assert_eq!(
        batch.rows[0][0],
        Value::Int(3),
        "sort descending puts id 3 first"
    );
    dataframe::dataframe_materialize(loom, ns, "etl/purchases")?;
    assert_eq!(
        columnar::columnar_rows(loom, ns, "analytics/purchases")?,
        2,
        "materialization creates a columnar output"
    );

    let c1 = loom.commit(ns, "conformance", "dataframe c1", 1)?;
    loom.write_file(
        ns,
        "inputs/events.csv",
        b"id,kind,total\n1,purchase,10.5\n2,view,0\n3,purchase,7.5\n4,purchase,1\n",
        0o100644,
    )?;
    dataframe::dataframe_materialize(loom, ns, "etl/purchases")?;
    loom.commit(ns, "conformance", "dataframe c2", 2)?;
    assert_eq!(
        columnar::columnar_rows(loom, ns, "analytics/purchases")?,
        3,
        "refreshed input can materialize a new output"
    );
    loom.checkout_commit(ns, c1)?;
    assert_eq!(
        columnar::columnar_rows(loom, ns, "analytics/purchases")?,
        2,
        "checkout restores dataframe materialized output"
    );

    let (dst_ns, report) = clone_workspace(loom, ns, dst, WorkspaceId::from_bytes([28; 16]))?;
    assert!(
        report.objects_transferred > 0,
        "a clone must transfer dataframe input, plan, and output closure"
    );
    dst.checkout_commit(dst_ns, c1)?;
    assert_eq!(
        columnar::columnar_rows(dst, dst_ns, "analytics/purchases")?,
        2,
        "clone preserves dataframe materialized output"
    );
    Ok(())
}

/// Execute the `search` behavioral suite: create a mapped collection, index/get/delete documents, the
/// portable linear-scan match query, the unmapped-field error, versioning, and clone reachability.
pub fn run_search_facade_behavior<S: ObjectStore, T: ObjectStore>(
    loom: &mut Loom<S>,
    dst: &mut Loom<T>,
) -> Result<()> {
    let ns =
        loom.registry_mut()
            .create(FacetKind::Search, None, WorkspaceId::from_bytes([26; 16]))?;

    let mut mapping = search::Mapping::new();
    mapping.insert("title".to_string(), search::FieldMapping::text());
    search::search_create(loom, ns, "docs", mapping)?;

    let mut d1 = search::Document::new();
    d1.insert(
        "title".to_string(),
        search::FieldValue::Text("hello world".to_string()),
    );
    search::search_index(loom, ns, "docs", b"d1".to_vec(), d1)?;
    let mut tmp = search::Document::new();
    tmp.insert(
        "title".to_string(),
        search::FieldValue::Text("temporary".to_string()),
    );
    search::search_index(loom, ns, "docs", b"tmp".to_vec(), tmp)?;
    assert!(
        search::search_get(loom, ns, "docs", b"d1")?.is_some(),
        "an indexed document round-trips"
    );
    assert!(
        search::search_delete(loom, ns, "docs", b"tmp")?,
        "deleting a present document reports true"
    );
    assert!(
        search::search_get(loom, ns, "docs", b"tmp")?.is_none(),
        "a deleted document is absent"
    );

    let request = search::QueryRequest::new(
        search::Query::Match {
            field: "title".to_string(),
            text: "hello".to_string(),
        },
        10,
        0,
    );
    let response = search::search_query(loom, ns, "docs", &request)?;
    assert!(
        response.reduced,
        "the portable fallback marks the response reduced"
    );
    assert_eq!(
        response
            .hits
            .iter()
            .map(|h| h.id.clone())
            .collect::<Vec<_>>(),
        vec![b"d1".to_vec()],
        "the match query finds the document"
    );

    let unmapped = search::QueryRequest::new(
        search::Query::Match {
            field: "missing".to_string(),
            text: "x".to_string(),
        },
        10,
        0,
    );
    assert_eq!(
        search::search_query(loom, ns, "docs", &unmapped)
            .unwrap_err()
            .code,
        Code::NoSuchField,
        "an unmapped query field is NO_SUCH_FIELD"
    );

    let c1 = loom.commit(ns, "conformance", "search c1", 1)?;
    let mut d2 = search::Document::new();
    d2.insert(
        "title".to_string(),
        search::FieldValue::Text("second".to_string()),
    );
    search::search_index(loom, ns, "docs", b"d2".to_vec(), d2)?;
    loom.commit(ns, "conformance", "search c2", 2)?;
    loom.checkout_commit(ns, c1)?;
    assert!(
        search::search_get(loom, ns, "docs", b"d2")?.is_none(),
        "checkout restores the c1 documents"
    );

    let (dst_ns, report) = clone_workspace(loom, ns, dst, WorkspaceId::from_bytes([27; 16]))?;
    assert!(
        report.objects_transferred > 0,
        "a clone must transfer the search object closure"
    );
    dst.checkout_commit(dst_ns, c1)?;
    assert!(
        search::search_get(dst, dst_ns, "docs", b"d1")?.is_some(),
        "clone preserves the documents"
    );
    Ok(())
}
