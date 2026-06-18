//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

// ---------------------------------------------------------------------------------------------------
// Key-value (Kv facet) - put/get/delete/list/range over a named map in a workspace, by UUID or name.
// Keys cross as Loom Canonical CBOR typed cells (one tagged cell each); list/range return the canonical
// CBOR array of `[key, value]` pairs in key order.
// ---------------------------------------------------------------------------------------------------

fn ensure_kv_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Kv,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Kv)?;
    Ok(ns)
}

fn parse_kv_tier(value: i32) -> LoomResult<KvTier> {
    match value {
        0 => Ok(KvTier::Versioned),
        1 => Ok(KvTier::Ephemeral),
        other => Err(LoomError::invalid(format!("unknown kv tier {other}"))),
    }
}

fn kv_tier_str(tier: KvTier) -> &'static str {
    match tier {
        KvTier::Versioned => "versioned",
        KvTier::Ephemeral => "ephemeral",
    }
}

fn opt_u64_json(value: Option<u64>) -> String {
    value.map_or_else(|| "null".to_string(), |v| v.to_string())
}

fn kv_map_config_json(config: KvMapConfig) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"tier\":");
    out.push_str(&json_string(kv_tier_str(config.tier)));
    out.push_str(",\"default_ttl_ms\":");
    out.push_str(&opt_u64_json(config.default_put.ttl_ms));
    out.push_str(",\"default_idle_ttl_ms\":");
    out.push_str(&opt_u64_json(config.default_put.idle_ttl_ms));
    out.push_str(",\"read_through\":");
    out.push_str(if config.read_through { "true" } else { "false" });
    out.push_str(",\"write_through\":");
    out.push_str(if config.write_through {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"max_entries\":");
    out.push_str(&opt_u64_json(config.max_entries));
    out.push_str(",\"max_bytes\":");
    out.push_str(&opt_u64_json(config.max_bytes));
    out.push_str(",\"eviction\":");
    out.push_str(&json_string(eviction_str(config.eviction)));
    out.push_str(",\"on_evict\":");
    out.push_str(&json_string(on_evict_str(config.on_evict)));
    out.push_str(",\"write_behind\":");
    out.push_str(if config.write_behind { "true" } else { "false" });
    out.push_str(",\"write_around\":");
    out.push_str(if config.write_around { "true" } else { "false" });
    out.push_str(",\"back_pressure\":");
    out.push_str(&json_string(back_pressure_str(config.back_pressure)));
    out.push_str(",\"flush_high_water_pct\":");
    out.push_str(&opt_u64_json(config.flush_high_water_pct.map(u64::from)));
    out.push_str(",\"flush_batch\":");
    out.push_str(&opt_u64_json(config.flush_batch));
    out.push('}');
    out
}

fn eviction_str(e: loom_core::EvictionPolicy) -> &'static str {
    use loom_core::EvictionPolicy::{Fifo, Lfu, Lru, None, Random, TtlPriority};
    match e {
        None => "none",
        Lru => "lru",
        Lfu => "lfu",
        Random => "random",
        Fifo => "fifo",
        TtlPriority => "ttl_priority",
    }
}

fn on_evict_str(o: loom_core::OnEvict) -> &'static str {
    match o {
        loom_core::OnEvict::Drop => "drop",
        loom_core::OnEvict::WriteThrough => "write_through",
    }
}

fn back_pressure_str(b: loom_core::BackPressure) -> &'static str {
    match b {
        loom_core::BackPressure::Block => "block",
        loom_core::BackPressure::Pressure => "pressure",
        loom_core::BackPressure::Assisted => "assisted",
    }
}

/// Build a `KvMapConfig` from the flattened C ABI arguments. `0` means "absent" for the optional
/// `u64` bounds and TTLs; `flush_high_water_pct` uses a negative value for "absent". `eviction`,
/// `on_evict`, and `back_pressure` are the stable enum tags.
#[allow(clippy::too_many_arguments)]
fn kv_config_from_args(
    tier: i32,
    default_ttl_ms: u64,
    default_idle_ttl_ms: u64,
    read_through: i32,
    write_through: i32,
    max_entries: u64,
    max_bytes: u64,
    eviction: i32,
    on_evict: i32,
    write_behind: i32,
    write_around: i32,
    back_pressure: i32,
    flush_high_water_pct: i32,
    flush_batch: u64,
) -> LoomResult<KvMapConfig> {
    let tag = |v: i32, what: &str| -> LoomResult<u8> {
        u8::try_from(v).map_err(|_| LoomError::invalid(format!("invalid {what} tag {v}")))
    };
    Ok(KvMapConfig {
        tier: parse_kv_tier(tier)?,
        default_put: EphemeralPutOptions {
            ttl_ms: (default_ttl_ms != 0).then_some(default_ttl_ms),
            idle_ttl_ms: (default_idle_ttl_ms != 0).then_some(default_idle_ttl_ms),
        },
        read_through: read_through != 0,
        write_through: write_through != 0,
        max_entries: (max_entries != 0).then_some(max_entries),
        max_bytes: (max_bytes != 0).then_some(max_bytes),
        eviction: loom_core::EvictionPolicy::from_u8(tag(eviction, "eviction")?)?,
        on_evict: loom_core::OnEvict::from_u8(tag(on_evict, "on_evict")?)?,
        write_behind: write_behind != 0,
        write_around: write_around != 0,
        back_pressure: loom_core::BackPressure::from_u8(tag(back_pressure, "back_pressure")?)?,
        flush_high_water_pct: (flush_high_water_pct >= 0)
            .then(|| u8::try_from(flush_high_water_pct.min(100)).unwrap_or(100)),
        flush_batch: (flush_batch != 0).then_some(flush_batch),
    })
}

fn kv_put_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    key: &[u8],
    value: &[u8],
) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = ensure_kv_ns(&mut loom, workspace)?;
    let key = key_from_cbor(key)?;
    kv_put(&mut loom, ns, collection, key, value.to_vec())?;
    save_loom(&mut loom)?;
    Ok(())
}

fn kv_get_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    key: &[u8],
) -> LoomResult<Option<Vec<u8>>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let key = key_from_cbor(key)?;
    kv_get(&loom, ns, collection, &key)
}

fn kv_delete_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    key: &[u8],
) -> LoomResult<bool> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let key = key_from_cbor(key)?;
    let present = kv_delete(&mut loom, ns, collection, &key)?;
    if present {
        save_loom(&mut loom)?;
    }
    Ok(present)
}

fn kv_list_cbor_ns(h: &LoomSession, workspace: &str, collection: &str) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(kv_list(&loom, ns, collection)?.encode())
}

fn kv_range_cbor_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    lo: &[u8],
    hi: &[u8],
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let lo = key_from_cbor(lo)?;
    let hi = key_from_cbor(hi)?;
    Ok(kv_range(&loom, ns, collection, &lo, &hi)?.encode())
}

/// Set the storage tier config for map `collection` in workspace `workspace`. `tier`: 0 versioned,
/// 1 ephemeral. `default_ttl_ms`/`default_idle_ttl_ms` use 0 for absent; positive values apply to
/// default ephemeral puts. `read_through`/`write_through`/`write_behind`/`write_around` are boolean
/// flags (0/1). `max_entries`/`max_bytes`/`flush_batch` use 0 for unbounded. `eviction` is the policy
/// tag (0 none, 1 lru, 2 lfu, 3 random, 4 fifo, 5 ttl_priority); `on_evict` (0 drop, 1 write_through);
/// `back_pressure` (0 block, 1 pressure, 2 assisted). `flush_high_water_pct` is the soft flush
/// threshold percent (0..=100), or a negative value for "only the hard bound".
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection` valid C strings.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn loom_management_kv_set_config(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    tier: i32,
    default_ttl_ms: u64,
    default_idle_ttl_ms: u64,
    read_through: i32,
    write_through: i32,
    max_entries: u64,
    max_bytes: u64,
    eviction: i32,
    on_evict: i32,
    write_behind: i32,
    write_around: i32,
    back_pressure: i32,
    flush_high_water_pct: i32,
    flush_batch: u64,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_management_kv_set_config");
    let workspace = arg_str!(workspace, "loom_management_kv_set_config");
    let collection = arg_str!(collection, "loom_management_kv_set_config");
    let result = (|| -> LoomResult<()> {
        let config = kv_config_from_args(
            tier,
            default_ttl_ms,
            default_idle_ttl_ms,
            read_through,
            write_through,
            max_entries,
            max_bytes,
            eviction,
            on_evict,
            write_behind,
            write_around,
            back_pressure,
            flush_high_water_pct,
            flush_batch,
        )?;
        let mut loom = open_h_write(h)?;
        let ns = ensure_kv_ns(&mut loom, workspace)?;
        let actor = loom.effective_principal()?;
        loom.configure_kv_map(ns, collection, config)?;
        save_loom(&mut loom)?;
        let target = format!("workspace={ns};collection={collection}");
        loom.store()
            .audit_append(actor, "management.kv.set_config", Some(&target))?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Return the durable KV map tier config as JSON. A map with no explicit config returns the
/// default versioned configuration.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection` valid C strings; `out` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_management_kv_get_config_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_management_kv_get_config_json");
    let workspace = arg_str!(workspace, "loom_management_kv_get_config_json");
    let collection = arg_str!(collection, "loom_management_kv_get_config_json");
    match open_h_read(h).and_then(|loom| {
        let ns = resolve_workspace_arg(&loom, workspace)?;
        loom.authorize(ns, FacetKind::Kv, AclRight::Admin)?;
        Ok(kv_map_config_json(loom.kv_map_config(ns, collection)))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Put `value` at the typed key `key` (Loom Canonical CBOR cell) in map `collection` of workspace `workspace`
/// (UUID or name, created with the `kv` facet if absent). Returns `0`. A later put at the same key
/// replaces the value.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection` valid C strings; `key`/`value` null or
/// `key_len`/`value_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_kv_put(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    key: *const c_uchar,
    key_len: usize,
    value: *const c_uchar,
    value_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_kv_put");
    let workspace = arg_str!(workspace, "loom_kv_put");
    let collection = arg_str!(collection, "loom_kv_put");
    // SAFETY: caller guarantees `(key, key_len)` and `(value, value_len)` are readable/null (see docs).
    let key = unsafe { byte_slice(key, key_len) };
    let value = unsafe { byte_slice(value, value_len) };
    match kv_put_ns(h, workspace, collection, key, value) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Fetch the value at typed key `key` in map `collection` of workspace `workspace`. On success returns `0`
/// and sets `*out_found`: present -> `*out_found = 1` and bytes at `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]); absent -> `*out_found = 0` and `(*out_ptr, *out_len)` are `(null, 0)`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection` valid C strings; `key` null or `key_len`
/// readable bytes; `out_ptr`/`out_len`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_kv_get(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    key: *const c_uchar,
    key_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_kv_get");
    let workspace = arg_str!(workspace, "loom_kv_get");
    let collection = arg_str!(collection, "loom_kv_get");
    // SAFETY: caller guarantees `(key, key_len)` is readable/null (see docs).
    let key = unsafe { byte_slice(key, key_len) };
    match kv_get_ns(h, workspace, collection, key) {
        Ok(Some(bytes)) => {
            if !out_found.is_null() {
                // SAFETY: `out_found` is writable per fn docs.
                unsafe { *out_found = 1 };
            }
            // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
            unsafe { ok_bytes(out_ptr, out_len, bytes) }
        }
        Ok(None) => {
            // SAFETY: each non-null out-pointer is writable per fn docs.
            unsafe {
                if !out_found.is_null() {
                    *out_found = 0;
                }
                if !out_ptr.is_null() {
                    *out_ptr = core::ptr::null_mut();
                }
                if !out_len.is_null() {
                    *out_len = 0;
                }
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Remove the typed key `key` from map `collection` of workspace `workspace`; writes whether it was present
/// (`1`/`0`) to `*out_found` and returns `0`. Removing an absent key or map is a no-op.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection` valid C strings; `key` null or `key_len`
/// readable bytes; `out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_kv_delete(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    key: *const c_uchar,
    key_len: usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_kv_delete");
    let workspace = arg_str!(workspace, "loom_kv_delete");
    let collection = arg_str!(collection, "loom_kv_delete");
    // SAFETY: caller guarantees `(key, key_len)` is readable/null (see docs).
    let key = unsafe { byte_slice(key, key_len) };
    match kv_delete_ns(h, workspace, collection, key) {
        Ok(found) => {
            if !out_found.is_null() {
                // SAFETY: `out_found` is writable per fn docs.
                unsafe { *out_found = i32::from(found) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// List map `collection` of workspace `workspace` as the Loom Canonical CBOR array of `[key, value]` pairs in
/// key order (an absent map is the empty array). Writes owned bytes to `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection` valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_kv_list_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_kv_list_cbor");
    let workspace = arg_str!(workspace, "loom_kv_list_cbor");
    let collection = arg_str!(collection, "loom_kv_list_cbor");
    match kv_list_cbor_ns(h, workspace, collection) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// The entries of map `collection` with `lo <= key < hi` (half-open, key order) as the Loom Canonical CBOR
/// array of `[key, value]` pairs. `lo`/`hi` are typed-cell CBOR keys. Writes owned bytes to
/// `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection` valid C strings; `lo`/`hi` null or
/// `lo_len`/`hi_len` readable bytes; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_kv_range_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    lo: *const c_uchar,
    lo_len: usize,
    hi: *const c_uchar,
    hi_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_kv_range_cbor");
    let workspace = arg_str!(workspace, "loom_kv_range_cbor");
    let collection = arg_str!(collection, "loom_kv_range_cbor");
    // SAFETY: caller guarantees `(lo, lo_len)` and `(hi, hi_len)` are readable/null (see docs).
    let lo = unsafe { byte_slice(lo, lo_len) };
    let hi = unsafe { byte_slice(hi, hi_len) };
    match kv_range_cbor_ns(h, workspace, collection, lo, hi) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}
