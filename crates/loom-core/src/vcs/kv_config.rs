use super::*;

fn kv_key_scope(name: &str, key: &crate::tabular::Value) -> Vec<u8> {
    let mut scope = Vec::with_capacity(name.len() + 1);
    scope.extend_from_slice(name.as_bytes());
    scope.push(0);
    scope.extend_from_slice(&crate::kv::key_to_cbor(key));
    scope
}

impl<S: ObjectStore> Loom<S> {
    /// Configure the storage tier for a named KV map.
    pub fn configure_kv_map(
        &mut self,
        ns: WorkspaceId,
        name: &str,
        config: KvMapConfig,
    ) -> Result<()> {
        self.authorize_collection(ns, FacetKind::Kv, name, AclRight::Admin)?;
        self.registry.name(ns)?;
        if name.is_empty() {
            return Err(LoomError::invalid("kv map name must not be empty"));
        }
        if name == ".config" {
            return Err(LoomError::invalid("kv map name '.config' is reserved"));
        }
        // The config is a committed reserved file so it versions and syncs with the workspace; the
        // runtime cache entries stay coordinator-local.
        if config.tier == KvTier::Versioned {
            crate::kv::remove_kv_config(self, ns, name)?;
            self.ephemeral_kv.remove(&(ns, name.to_string()));
        } else {
            crate::kv::put_kv_config(self, ns, name, &config)?;
        }
        Ok(())
    }

    /// Durable configuration for a named KV map, read from the versioned reserved config file (or the
    /// default versioned config when none is stored).
    pub fn kv_map_config(&self, ns: WorkspaceId, name: &str) -> KvMapConfig {
        crate::kv::get_kv_config(self, ns, name)
    }

    /// Tier-aware KV put. Versioned maps use the ordinary KV path. Ephemeral maps update the runtime
    /// cache and apply the configured write mode - cache-only, write-through (synchronous backing
    /// write), write-around (backing write, no cache population), or write-behind (buffered async
    /// backing write) - plus back-pressure on the write-behind flush queue. Mode precedence:
    /// write_around > write_behind > write_through > cache-only.
    pub fn kv_put_configured(
        &mut self,
        ns: WorkspaceId,
        name: &str,
        key: crate::tabular::Value,
        value: Vec<u8>,
        opts: Option<EphemeralPutOptions>,
        now_ms: u64,
    ) -> Result<()> {
        self.authorize_key(
            ns,
            FacetKind::Kv,
            &kv_key_scope(name, &key),
            AclRight::Write,
        )?;
        let config = self.kv_map_config(ns, name);
        if config.tier == KvTier::Versioned {
            return kv_put(self, ns, name, key, value);
        }
        let put_opts = opts.unwrap_or(config.default_put);
        let cache_key = (ns, name.to_string());

        // Write-around: persist to the backing map and invalidate (do not populate) the cache.
        if config.write_around {
            kv_put(self, ns, name, key.clone(), value)?;
            if let Some(cache) = self.ephemeral_kv.get_mut(&cache_key) {
                cache.delete(&key);
            }
            return Ok(());
        }

        // Write-behind Pressure pre-check: reject a new write while saturated, before mutating, so a
        // rejected write leaves no trace.
        if config.write_behind && config.back_pressure == BackPressure::Pressure {
            let pct = config.flush_high_water_pct.unwrap_or(100);
            let saturated = self
                .ephemeral_kv
                .get(&cache_key)
                .is_some_and(|c| c.over_high_water(pct));
            if saturated {
                return Err(LoomError::locked(
                    "kv write-behind queue at high-water; retry (back_pressure=pressure)",
                ));
            }
        }

        // Write-through (only when not write-behind): persist to the backing map before caching.
        if config.write_through && !config.write_behind {
            kv_put(self, ns, name, key.clone(), value.clone())?;
        }

        // Populate the cache (with capacity eviction); buffer the backing write for write-behind.
        let evicted = {
            let cache = self.ephemeral_kv.entry(cache_key.clone()).or_default();
            cache.set_limits(config.max_entries, config.max_bytes, config.eviction);
            let evicted = cache.put_evicting(key.clone(), value.clone(), put_opts, now_ms)?;
            if config.write_behind {
                cache.mark_dirty_put(key, value);
            }
            evicted
        };

        // Flush capacity-evicted entries to the backing map when configured to do so.
        if config.on_evict == OnEvict::WriteThrough {
            for (k, v) in evicted {
                kv_put(self, ns, name, k, v)?;
            }
        }

        // Write-behind back-pressure relief once the cache crosses the soft high-water mark: drain the
        // whole queue (Block) or flush one bounded batch (Assisted). Pressure was pre-checked above.
        if config.write_behind {
            let pct = config.flush_high_water_pct.unwrap_or(100);
            let over = self
                .ephemeral_kv
                .get(&cache_key)
                .is_some_and(|c| c.over_high_water(pct));
            if over {
                match config.back_pressure {
                    BackPressure::Block => {
                        self.flush_pending(ns, name, None)?;
                    }
                    BackPressure::Assisted => {
                        self.flush_pending(ns, name, config.flush_batch)?;
                    }
                    BackPressure::Pressure => {}
                }
            }
        }
        Ok(())
    }

    /// Tier-aware KV get.
    pub fn kv_get_configured(
        &mut self,
        ns: WorkspaceId,
        name: &str,
        key: &crate::tabular::Value,
        now_ms: u64,
    ) -> Result<Option<Vec<u8>>> {
        self.authorize_key(ns, FacetKind::Kv, &kv_key_scope(name, key), AclRight::Read)?;
        let config = self.kv_map_config(ns, name);
        match config.tier {
            KvTier::Versioned => kv_get(self, ns, name, key),
            KvTier::Ephemeral => {
                if let Some(value) = self
                    .ephemeral_kv
                    .entry((ns, name.to_string()))
                    .or_default()
                    .get(key, now_ms)
                {
                    return Ok(Some(value));
                }
                if !config.read_through {
                    return Ok(None);
                }
                let Some(value) = kv_get(self, ns, name, key)? else {
                    return Ok(None);
                };
                let cache = self.ephemeral_kv.entry((ns, name.to_string())).or_default();
                cache.set_limits(config.max_entries, config.max_bytes, config.eviction);
                cache.put(key.clone(), value.clone(), config.default_put, now_ms)?;
                Ok(Some(value))
            }
        }
    }

    /// Tier-aware KV delete. Ephemeral deletes honor the configured write mode: write-through and
    /// write-around delete the backing map synchronously, write-behind buffers the backing delete for
    /// the next flush, and cache-only removes from the runtime cache only.
    pub fn kv_delete_configured(
        &mut self,
        ns: WorkspaceId,
        name: &str,
        key: &crate::tabular::Value,
    ) -> Result<bool> {
        self.authorize_key(ns, FacetKind::Kv, &kv_key_scope(name, key), AclRight::Write)?;
        let config = self.kv_map_config(ns, name);
        if config.tier == KvTier::Versioned {
            return kv_delete(self, ns, name, key);
        }
        let cache_key = (ns, name.to_string());
        let cache_present = self
            .ephemeral_kv
            .entry(cache_key.clone())
            .or_default()
            .delete(key);
        if config.write_around || (config.write_through && !config.write_behind) {
            let backing = kv_delete(self, ns, name, key)?;
            return Ok(backing || cache_present);
        }
        if config.write_behind {
            self.ephemeral_kv
                .entry(cache_key)
                .or_default()
                .mark_dirty_delete(key.clone());
        }
        Ok(cache_present)
    }

    /// Tier-aware KV range.
    pub fn kv_range_configured(
        &mut self,
        ns: WorkspaceId,
        name: &str,
        lo: &crate::tabular::Value,
        hi: &crate::tabular::Value,
        now_ms: u64,
    ) -> Result<KvMap> {
        self.authorize_collection(ns, FacetKind::Kv, name, AclRight::Read)?;
        let config = self.kv_map_config(ns, name);
        match config.tier {
            KvTier::Versioned => crate::kv::kv_range(self, ns, name, lo, hi),
            KvTier::Ephemeral => {
                let mut out = KvMap::new();
                for (key, value) in self
                    .ephemeral_kv
                    .entry((ns, name.to_string()))
                    .or_default()
                    .range(lo, hi, now_ms)
                {
                    out.put(key, value);
                }
                Ok(out)
            }
        }
    }

    /// Tier-aware KV list.
    pub fn kv_list_configured(
        &mut self,
        ns: WorkspaceId,
        name: &str,
        now_ms: u64,
    ) -> Result<KvMap> {
        self.authorize_collection(ns, FacetKind::Kv, name, AclRight::Read)?;
        let config = self.kv_map_config(ns, name);
        match config.tier {
            KvTier::Versioned => crate::kv::kv_list(self, ns, name),
            KvTier::Ephemeral => {
                let mut out = KvMap::new();
                for (key, value) in self
                    .ephemeral_kv
                    .entry((ns, name.to_string()))
                    .or_default()
                    .list(now_ms)
                {
                    out.put(key, value);
                }
                Ok(out)
            }
        }
    }

    /// Drain up to `max` buffered write-behind mutations for one ephemeral map to its backing versioned
    /// map (coalesced, in key order; `None` flushes the whole queue). Returns the number flushed; a
    /// no-op for an absent map. Best-effort: a backing-write error aborts the drain and the remaining
    /// taken ops for this batch are not re-buffered.
    pub fn flush_pending(
        &mut self,
        ns: WorkspaceId,
        name: &str,
        max: Option<u64>,
    ) -> Result<usize> {
        let batch = match self.ephemeral_kv.get_mut(&(ns, name.to_string())) {
            Some(cache) => cache.take_flush_batch(max),
            None => return Ok(0),
        };
        let flushed = batch.len();
        for (key, op) in batch {
            match op {
                Some(value) => kv_put(self, ns, name, key, value)?,
                None => {
                    kv_delete(self, ns, name, &key)?;
                }
            }
        }
        Ok(flushed)
    }

    /// Flush every ephemeral map in `ns` that has buffered write-behind mutations; returns the total
    /// flushed. Drive-point for host checkpoint/shutdown; the host owns the cadence.
    pub fn flush_all_pending(&mut self, ns: WorkspaceId) -> Result<usize> {
        let names: Vec<String> = self
            .ephemeral_kv
            .iter()
            .filter(|((n, _), cache)| *n == ns && cache.has_pending())
            .map(|((_, name), _)| name.clone())
            .collect();
        let mut total = 0;
        for name in names {
            total += self.flush_pending(ns, &name, None)?;
        }
        Ok(total)
    }

    /// Number of buffered write-behind mutations for one ephemeral map (0 when absent).
    pub fn pending_flush_count(&self, ns: WorkspaceId, name: &str) -> usize {
        self.ephemeral_kv
            .get(&(ns, name.to_string()))
            .map_or(0, EphemeralKvMap::pending_len)
    }

    /// Reclaim expired entries for one ephemeral map; returns the count reclaimed. GC drive-point for
    /// the host's flush/GC cadence; a no-op for an absent map.
    pub fn sweep_expired(&mut self, ns: WorkspaceId, name: &str, now_ms: u64) -> usize {
        self.ephemeral_kv
            .get_mut(&(ns, name.to_string()))
            .map_or(0, |c| c.sweep_expired(now_ms))
    }

    /// Reclaim expired entries across every ephemeral map in `ns`; returns the total reclaimed.
    pub fn sweep_all_expired(&mut self, ns: WorkspaceId, now_ms: u64) -> usize {
        self.ephemeral_kv
            .iter_mut()
            .filter(|((n, _), _)| *n == ns)
            .map(|(_, cache)| cache.sweep_expired(now_ms))
            .sum()
    }

    /// Drop the runtime cache (entries and any buffered write-behind mutations) for one ephemeral map.
    /// Callers wanting durability must [`Loom::flush_pending`] first. Used to invalidate a cache whose
    /// backing working tree changed under it (checkout/merge).
    pub fn clear_ephemeral_cache(&mut self, ns: WorkspaceId, name: &str) {
        self.ephemeral_kv.remove(&(ns, name.to_string()));
    }

    /// Drop all runtime ephemeral caches for `ns`. Flush first if the buffered write-behind deltas matter.
    pub fn drop_ephemeral_caches(&mut self, ns: WorkspaceId) {
        self.ephemeral_kv.retain(|(n, _), _| *n != ns);
    }

    // ---- persistence: reference store (registry) + content map ----------------------
}
