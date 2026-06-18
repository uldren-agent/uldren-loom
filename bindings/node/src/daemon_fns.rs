//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Local daemon status for `loomPath` as JSON. Missing daemons return a STOPPED JSON payload.
#[napi]
pub fn daemon_status_json(loom_path: String) -> napi::Result<String> {
    let paths = daemon::paths(&loom_path).map_err(reason)?;
    Ok(daemon::status_json(&paths))
}
/// Attach or detach a named session from a running local daemon.
#[napi]
pub fn daemon_session_attach(loom_path: String, session: String) -> napi::Result<()> {
    let paths = daemon::paths(&loom_path).map_err(reason)?;
    daemon::session_attach(&paths, &session).map_err(reason)?;
    Ok(())
}
#[napi]
pub fn daemon_session_detach(loom_path: String, session: String) -> napi::Result<()> {
    let paths = daemon::paths(&loom_path).map_err(reason)?;
    daemon::session_detach(&paths, &session).map_err(reason)?;
    Ok(())
}
/// Add or remove a long-lived pin on a running local daemon.
#[napi]
pub fn daemon_pin_add(loom_path: String, pin: String) -> napi::Result<()> {
    let paths = daemon::paths(&loom_path).map_err(reason)?;
    daemon::pin_add(&paths, &pin).map_err(reason)?;
    Ok(())
}
#[napi]
pub fn daemon_pin_remove(loom_path: String, pin: String) -> napi::Result<()> {
    let paths = daemon::paths(&loom_path).map_err(reason)?;
    daemon::pin_remove(&paths, &pin).map_err(reason)?;
    Ok(())
}
/// Acquire, refresh, or release a daemon-backed lock. Token-returning calls return JSON.
#[napi]
pub fn lock_acquire_json(
    loom_path: String,
    key: String,
    principal: String,
    session: String,
    mode: String,
    permits: u32,
    capacity: u32,
    lease_ms: BigInt,
    wait_ms: Option<BigInt>,
) -> napi::Result<String> {
    let paths = daemon::paths(&loom_path).map_err(reason)?;
    let mode = daemon::parse_lock_mode(&mode, permits, capacity).map_err(reason)?;
    let lease_ms = bigint_to_u64(lease_ms, "lease_ms")?;
    let wait_ms = wait_ms
        .map(|value| bigint_to_u64(value, "wait_ms"))
        .transpose()?
        .unwrap_or(daemon::DEFAULT_LOCK_WAIT_MS);
    let response = daemon::lock_acquire(
        &paths,
        daemon::AcquireRequest {
            key: &key,
            principal: &principal,
            session: &session,
            mode,
            lease_ms,
            wait_ms,
            now_ms: now_ms(),
        },
    )
    .map_err(reason)?;
    daemon::lock_response_json(&response).map_err(reason)
}
#[napi]
pub fn lock_refresh_json(
    loom_path: String,
    key: String,
    principal: String,
    session: String,
    mode: String,
    permits: u32,
    capacity: u32,
    fence_low: BigInt,
    fence_high: BigInt,
    lease_ms: BigInt,
) -> napi::Result<String> {
    let paths = daemon::paths(&loom_path).map_err(reason)?;
    let mode = daemon::parse_lock_mode(&mode, permits, capacity).map_err(reason)?;
    let fence_low = bigint_to_u64(fence_low, "fence_low")?;
    let fence_high = bigint_to_u64(fence_high, "fence_high")?;
    let lease_ms = bigint_to_u64(lease_ms, "lease_ms")?;
    let response = daemon::lock_refresh(
        &paths,
        daemon::RefreshRequest {
            key: &key,
            principal: &principal,
            session: &session,
            mode,
            fence: loom_core::Fence::from_limbs(fence_low, fence_high),
            lease_ms,
            now_ms: now_ms(),
        },
    )
    .map_err(reason)?;
    daemon::lock_response_json(&response).map_err(reason)
}
#[napi]
pub fn lock_release(
    loom_path: String,
    key: String,
    principal: String,
    session: String,
    mode: String,
    permits: u32,
    capacity: u32,
    fence_low: BigInt,
    fence_high: BigInt,
) -> napi::Result<()> {
    let paths = daemon::paths(&loom_path).map_err(reason)?;
    let mode = daemon::parse_lock_mode(&mode, permits, capacity).map_err(reason)?;
    let fence_low = bigint_to_u64(fence_low, "fence_low")?;
    let fence_high = bigint_to_u64(fence_high, "fence_high")?;
    daemon::lock_release(
        &paths,
        daemon::ReleaseRequest {
            key: &key,
            principal: &principal,
            session: &session,
            mode,
            fence: loom_core::Fence::from_limbs(fence_low, fence_high),
            now_ms: now_ms(),
        },
    )
    .map_err(reason)?;
    Ok(())
}
