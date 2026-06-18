//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Local daemon status for `path` as JSON. Missing daemons return a STOPPED JSON payload.
#[pyfunction]
pub(crate) fn daemon_status_json(path: &str) -> PyResult<String> {
    let paths = daemon::paths(path).map_err(py_err)?;
    Ok(daemon::status_json(&paths))
}
/// Attach or detach a named session from a running local daemon.
#[pyfunction]
pub(crate) fn daemon_session_attach(path: &str, session: &str) -> PyResult<()> {
    let paths = daemon::paths(path).map_err(py_err)?;
    daemon::session_attach(&paths, session).map_err(py_err)?;
    Ok(())
}
#[pyfunction]
pub(crate) fn daemon_session_detach(path: &str, session: &str) -> PyResult<()> {
    let paths = daemon::paths(path).map_err(py_err)?;
    daemon::session_detach(&paths, session).map_err(py_err)?;
    Ok(())
}
/// Add or remove a long-lived pin on a running local daemon.
#[pyfunction]
pub(crate) fn daemon_pin_add(path: &str, pin: &str) -> PyResult<()> {
    let paths = daemon::paths(path).map_err(py_err)?;
    daemon::pin_add(&paths, pin).map_err(py_err)?;
    Ok(())
}
#[pyfunction]
pub(crate) fn daemon_pin_remove(path: &str, pin: &str) -> PyResult<()> {
    let paths = daemon::paths(path).map_err(py_err)?;
    daemon::pin_remove(&paths, pin).map_err(py_err)?;
    Ok(())
}
/// Acquire, refresh, or release a daemon-backed lock. Token-returning calls return JSON.
#[pyfunction]
pub(crate) fn lock_acquire_json(
    path: &str,
    key: &str,
    principal: &str,
    session: &str,
    mode: &str,
    permits: u32,
    capacity: u32,
    lease_ms: u64,
    wait_ms: Option<u64>,
) -> PyResult<String> {
    let paths = daemon::paths(path).map_err(py_err)?;
    let mode = daemon::parse_lock_mode(mode, permits, capacity).map_err(py_err)?;
    let response = daemon::lock_acquire(
        &paths,
        daemon::AcquireRequest {
            key,
            principal,
            session,
            mode,
            lease_ms,
            wait_ms: wait_ms.unwrap_or(daemon::DEFAULT_LOCK_WAIT_MS),
            now_ms: now_ms(),
        },
    )
    .map_err(py_err)?;
    daemon::lock_response_json(&response).map_err(py_err)
}
#[pyfunction]
pub(crate) fn lock_refresh_json(
    path: &str,
    key: &str,
    principal: &str,
    session: &str,
    mode: &str,
    permits: u32,
    capacity: u32,
    fence_low: u64,
    fence_high: u64,
    lease_ms: u64,
) -> PyResult<String> {
    let paths = daemon::paths(path).map_err(py_err)?;
    let mode = daemon::parse_lock_mode(mode, permits, capacity).map_err(py_err)?;
    let response = daemon::lock_refresh(
        &paths,
        daemon::RefreshRequest {
            key,
            principal,
            session,
            mode,
            fence: loom_core::Fence::from_limbs(fence_low, fence_high),
            lease_ms,
            now_ms: now_ms(),
        },
    )
    .map_err(py_err)?;
    daemon::lock_response_json(&response).map_err(py_err)
}
#[pyfunction]
pub(crate) fn lock_release(
    path: &str,
    key: &str,
    principal: &str,
    session: &str,
    mode: &str,
    permits: u32,
    capacity: u32,
    fence_low: u64,
    fence_high: u64,
) -> PyResult<()> {
    let paths = daemon::paths(path).map_err(py_err)?;
    let mode = daemon::parse_lock_mode(mode, permits, capacity).map_err(py_err)?;
    daemon::lock_release(
        &paths,
        daemon::ReleaseRequest {
            key,
            principal,
            session,
            mode,
            fence: loom_core::Fence::from_limbs(fence_low, fence_high),
            now_ms: now_ms(),
        },
    )
    .map_err(py_err)?;
    Ok(())
}
