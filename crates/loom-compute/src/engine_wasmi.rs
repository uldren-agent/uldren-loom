//! The wasmi execution substrate.
//!
//! Pure-Rust WASM interpreter; the engine used on `wasm32`/browser and the default on native. Runs a
//! program against an in-memory file set under a fuel budget, enforcing the program's [`GrantSet`].
//! The program reaches state only through the host functions below (no clock, randomness, or ambient
//! I/O), so execution is deterministic and metered. wasmi is the 1.x line (`set_fuel`/`get_fuel`).

#[cfg(any(target_arch = "wasm32", not(feature = "engine-wasmtime"), test))]
use super::{FileSet, RunResult};
use super::{
    LogBuffer, RunOutcome, decode_columnar_aggregates, decode_columnar_filter,
    decode_columnar_select_columns, decode_columns, decode_f32_vec, decode_meta, decode_props,
    decode_row, decode_vector_filter, encode_bytes_list, encode_columns, encode_dir_entries,
    encode_edge, encode_edge_list, encode_hits, encode_props, encode_rows, encode_scan_entries,
    encode_string_list, encode_ts_point, encode_ts_points, encode_vector_entry,
};
#[cfg(any(target_arch = "wasm32", not(feature = "engine-wasmtime"), test))]
use crate::capability::{Capability, GrantSet, Mode};
use crate::error::ExecError;
use crate::state_access::StateAccess;
use loom_core::{
    BookMeta, CalendarEntry, CollectionMeta, Component, ContactEntry, DataframePlan, MailboxMeta,
    ObjectStore, key_from_cbor, search_document_cbor, search_document_from_cbor, search_ids_cbor,
    search_mapping_from_cbor, search_request_from_cbor, search_response_cbor,
};
use std::collections::BTreeMap;
use wasmi::{Caller, Config, Engine, Extern, Linker, Memory, Module, Store};

/// Host state carried by the wasmi `Store`: the working file set, the grants that gate it, and the
/// program's declared read-only inputs.
#[cfg(any(target_arch = "wasm32", not(feature = "engine-wasmtime"), test))]
struct HostCtx {
    files: FileSet,
    grants: GrantSet,
    inputs: BTreeMap<String, Vec<u8>>,
}

/// Run `wasm` over `files` with `fuel` units of budget under `grants`, with `inputs` available
/// read-only. Returns the mutated file set plus fuel used, or [`ExecError::BudgetExceeded`] on an
/// out-of-fuel trap.
#[cfg(any(target_arch = "wasm32", not(feature = "engine-wasmtime"), test))]
pub fn run(
    wasm: &[u8],
    files: FileSet,
    fuel: u64,
    grants: GrantSet,
    inputs: BTreeMap<String, Vec<u8>>,
) -> Result<RunResult, ExecError> {
    let mut config = Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module =
        Module::new(&engine, wasm).map_err(|e| ExecError::Program(format!("module: {e}")))?;
    let mut store = Store::new(
        &engine,
        HostCtx {
            files,
            grants,
            inputs,
        },
    );
    store
        .set_fuel(fuel)
        .map_err(|e| ExecError::Program(format!("fuel: {e}")))?;

    let mut linker = <Linker<HostCtx>>::new(&engine);

    linker
        .func_wrap(
            "env",
            "file_write",
            |mut caller: Caller<'_, HostCtx>, pp: i32, pl: i32, vp: i32, vl: i32| {
                let mem = memory(&caller);
                let path = read_string(&caller, &mem, pp, pl);
                let mut value = vec![0u8; vl.max(0) as usize];
                mem.read(&caller, vp as usize, &mut value)
                    .expect("read value");
                if caller
                    .data()
                    .grants
                    .permits(Capability::Files, Mode::Write, &path)
                {
                    caller.data_mut().files.insert(path, value);
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link file_write: {e}")))?;

    linker
        .func_wrap(
            "env",
            "file_remove",
            |mut caller: Caller<'_, HostCtx>, pp: i32, pl: i32| {
                let mem = memory(&caller);
                let path = read_string(&caller, &mem, pp, pl);
                if caller
                    .data()
                    .grants
                    .permits(Capability::Files, Mode::Write, &path)
                {
                    caller.data_mut().files.remove(&path);
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link file_remove: {e}")))?;

    linker
        .func_wrap(
            "env",
            "file_read",
            |mut caller: Caller<'_, HostCtx>, pp: i32, pl: i32, op: i32, oc: i32| -> i32 {
                let mem = memory(&caller);
                let path = read_string(&caller, &mem, pp, pl);
                if !caller
                    .data()
                    .grants
                    .permits(Capability::Files, Mode::Read, &path)
                {
                    return -1;
                }
                let found = caller.data().files.get(&path).cloned();
                write_out(&mut caller, &mem, op, oc, found)
            },
        )
        .map_err(|e| ExecError::Program(format!("link file_read: {e}")))?;

    linker
        .func_wrap(
            "env",
            "input_get",
            |mut caller: Caller<'_, HostCtx>, np: i32, nl: i32, op: i32, oc: i32| -> i32 {
                let mem = memory(&caller);
                let name = read_string(&caller, &mem, np, nl);
                let found = caller.data().inputs.get(&name).cloned();
                write_out(&mut caller, &mem, op, oc, found)
            },
        )
        .map_err(|e| ExecError::Program(format!("link input_get: {e}")))?;

    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|e| ExecError::Program(format!("instantiate: {e}")))?;
    let run = instance
        .get_typed_func::<(), ()>(&store, "run")
        .map_err(|e| ExecError::Program(format!("missing `run` export: {e}")))?;

    match run.call(&mut store, ()) {
        Ok(()) => {
            let remaining = store.get_fuel().unwrap_or(0);
            let fuel_used = fuel.saturating_sub(remaining);
            Ok(RunResult {
                files: store.into_data().files,
                fuel_used,
            })
        }
        Err(trap) => {
            let out_of_fuel = trap.as_trap_code() == Some(wasmi::TrapCode::OutOfFuel)
                || trap.to_string().to_lowercase().contains("fuel");
            if out_of_fuel {
                Err(ExecError::BudgetExceeded { budget: fuel })
            } else {
                Err(ExecError::Program(format!("trap: {trap}")))
            }
        }
    }
}

#[cfg(any(target_arch = "wasm32", not(feature = "engine-wasmtime"), test))]
fn memory(caller: &Caller<'_, HostCtx>) -> Memory {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .expect("program must export `memory`")
}

#[cfg(any(target_arch = "wasm32", not(feature = "engine-wasmtime"), test))]
fn read_string(caller: &Caller<'_, HostCtx>, mem: &Memory, ptr: i32, len: i32) -> String {
    let mut buf = vec![0u8; len.max(0) as usize];
    mem.read(caller, ptr as usize, &mut buf)
        .expect("read string");
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(any(target_arch = "wasm32", not(feature = "engine-wasmtime"), test))]
fn write_out(
    caller: &mut Caller<'_, HostCtx>,
    mem: &Memory,
    ptr: i32,
    cap: i32,
    found: Option<Vec<u8>>,
) -> i32 {
    match found {
        Some(value) => {
            let n = value.len().min(cap.max(0) as usize);
            mem.write(caller, ptr as usize, &value[..n])
                .expect("write out");
            value.len() as i32
        }
        None => -1,
    }
}

/// Host state for a `StateAccess`-backed run: the real facet-backed [`StateAccess`] plus the
/// read-only declared inputs. Files and KV host calls mutate the Loom object graph through
/// `StateAccess`, so every operation is gated by the program's grants and the principal's scoped Exec
/// ACL ([`crate::authz::ExecContext`]).
struct StateCtx<'a, S: ObjectStore> {
    state: StateAccess<'a, S>,
    inputs: BTreeMap<String, Vec<u8>>,
    logs: LogBuffer,
    host_error: Option<ExecError>,
}

/// A host error raised when KV key bytes are not canonical typed-key CBOR. Returning it from a host
/// function traps the guest immediately at the offending call, so the run fails without executing any
/// further instructions. This is invalid ABI input; an authorization denial is captured separately and
/// returned as its stable execution error.
#[derive(Debug)]
struct MalformedKvKey;

impl core::fmt::Display for MalformedKvKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("malformed kv key: not canonical typed-key CBOR")
    }
}

impl wasmi::errors::HostError for MalformedKvKey {}

fn malformed_kv_key() -> wasmi::Error {
    wasmi::Error::host(MalformedKvKey)
}

/// A host error raised when graph property bytes are not the canonical `[key, value]` pair array
/// `encode_props` produces. Like [`MalformedKvKey`], returning it traps the guest at the offending
/// `graph_upsert_node`/`graph_upsert_edge` call, so the run fails without mutating state.
#[derive(Debug)]
struct MalformedGraphProps;

impl core::fmt::Display for MalformedGraphProps {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("malformed graph props: not a canonical [key, value] pair array")
    }
}

impl wasmi::errors::HostError for MalformedGraphProps {}

fn malformed_props() -> wasmi::Error {
    wasmi::Error::host(MalformedGraphProps)
}

/// A host error raised when vector input bytes are malformed: a component blob whose length is not a
/// multiple of 4, a metadata blob that is not the canonical pair array, or an unknown metric tag. Like
/// [`MalformedKvKey`], returning it traps the guest at the offending vector call without mutating state.
#[derive(Debug)]
struct MalformedVectorInput;

impl core::fmt::Display for MalformedVectorInput {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("malformed vector input: bad component bytes, metadata, or metric tag")
    }
}

impl wasmi::errors::HostError for MalformedVectorInput {}

fn malformed_vector() -> wasmi::Error {
    wasmi::Error::host(MalformedVectorInput)
}

/// A host error raised when columnar input bytes are malformed: a row/schema blob that is not the
/// canonical cell/pair array, or an unknown column-type tag. Like [`MalformedKvKey`], returning it
/// traps the guest at the offending columnar call without mutating state.
#[derive(Debug)]
struct MalformedColumnarInput;

impl core::fmt::Display for MalformedColumnarInput {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("malformed columnar input: bad row, schema, or column-type tag")
    }
}

impl wasmi::errors::HostError for MalformedColumnarInput {}

fn malformed_columnar() -> wasmi::Error {
    wasmi::Error::host(MalformedColumnarInput)
}

#[derive(Debug)]
struct MalformedSearchInput;

impl core::fmt::Display for MalformedSearchInput {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("malformed search input: bad mapping, document, or query bytes")
    }
}

impl wasmi::errors::HostError for MalformedSearchInput {}

fn malformed_search() -> wasmi::Error {
    wasmi::Error::host(MalformedSearchInput)
}

#[derive(Debug)]
struct MalformedDataframeInput;

impl core::fmt::Display for MalformedDataframeInput {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("malformed dataframe input: bad plan bytes")
    }
}

impl wasmi::errors::HostError for MalformedDataframeInput {}

fn malformed_dataframe() -> wasmi::Error {
    wasmi::Error::host(MalformedDataframeInput)
}

/// A host error raised when PIM record or list bytes are malformed. The PIM host ABI accepts the same
/// canonical record bytes the PIM crates store, so malformed inputs trap at the boundary before
/// partially mutating state.
#[derive(Debug)]
struct MalformedPimInput;

impl core::fmt::Display for MalformedPimInput {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("malformed PIM input: bad record or list bytes")
    }
}

impl wasmi::errors::HostError for MalformedPimInput {}

fn malformed_pim() -> wasmi::Error {
    wasmi::Error::host(MalformedPimInput)
}

#[derive(Debug)]
struct StoredHostError;

impl core::fmt::Display for StoredHostError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("execution host operation failed")
    }
}

impl wasmi::errors::HostError for StoredHostError {}

fn set_host_error<S: ObjectStore>(
    caller: &mut Caller<'_, StateCtx<S>>,
    err: ExecError,
) -> wasmi::Error {
    caller.data_mut().host_error = Some(err);
    wasmi::Error::host(StoredHostError)
}

/// Run `wasm` with `fuel` units of budget against `state`, with `inputs` available read-only. The host
/// ABI (module `env`) is the state host ABI registered below; the guest exports `memory` and `run`.
/// KV keys are Loom canonical typed-key CBOR (`key_from_cbor`); malformed ABI bytes trap immediately
/// at the offending call. Denied operations fail the run with their stable execution error instead of
/// being hidden as absent reads or ignored writes. The guest may emit ordered, bounded diagnostic
/// lines through the `log` host call. Returns the `StateAccess` (for the caller to commit) and a
/// [`RunOutcome`] (fuel and logs); an out-of-fuel program returns [`ExecError::BudgetExceeded`].
pub fn run_state<'a, S: ObjectStore>(
    wasm: &[u8],
    state: StateAccess<'a, S>,
    fuel: u64,
    inputs: BTreeMap<String, Vec<u8>>,
) -> Result<(StateAccess<'a, S>, RunOutcome), ExecError> {
    let mut config = Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module =
        Module::new(&engine, wasm).map_err(|e| ExecError::Program(format!("module: {e}")))?;
    let mut store = Store::new(
        &engine,
        StateCtx {
            state,
            inputs,
            logs: LogBuffer::default(),
            host_error: None,
        },
    );
    store
        .set_fuel(fuel)
        .map_err(|e| ExecError::Program(format!("fuel: {e}")))?;

    let mut linker = <Linker<StateCtx<S>>>::new(&engine);

    linker
        .func_wrap(
            "env",
            "file_write",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             vp: i32,
             vl: i32|
             -> Result<(), wasmi::Error> {
                let mem = state_memory(&caller);
                let path = state_read_string(&caller, &mem, pp, pl);
                let value = state_read_bytes(&caller, &mem, vp, vl);
                caller
                    .data_mut()
                    .state
                    .file_write(&path, &value)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link file_write: {e}")))?;

    linker
        .func_wrap(
            "env",
            "file_remove",
            |mut caller: Caller<'_, StateCtx<S>>, pp: i32, pl: i32| -> Result<(), wasmi::Error> {
                let mem = state_memory(&caller);
                let path = state_read_string(&caller, &mem, pp, pl);
                caller
                    .data_mut()
                    .state
                    .file_remove(&path)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link file_remove: {e}")))?;

    linker
        .func_wrap(
            "env",
            "file_read",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let path = state_read_string(&caller, &mem, pp, pl);
                match caller.data_mut().state.file_read(&path) {
                    Ok(found) => Ok(state_write_out(&mut caller, &mem, op, oc, Some(found))),
                    Err(ExecError::Core(err)) if err.code == loom_core::Code::NotFound => {
                        Ok(state_write_out(&mut caller, &mem, op, oc, None))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link file_read: {e}")))?;

    linker
        .func_wrap(
            "env",
            "file_list",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let path = state_read_string(&caller, &mem, pp, pl);
                match caller.data_mut().state.file_list(&path) {
                    Ok(entries) => {
                        let blob = encode_dir_entries(&entries);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(ExecError::Core(err)) if err.code == loom_core::Code::NotFound => {
                        Ok(state_write_out(&mut caller, &mem, op, oc, None))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link file_list: {e}")))?;

    linker
        .func_wrap(
            "env",
            "log",
            |mut caller: Caller<'_, StateCtx<S>>, ptr: i32, len: i32| {
                let mem = state_memory(&caller);
                let line = state_read_string(&caller, &mem, ptr, len);
                caller.data_mut().logs.push(line);
            },
        )
        .map_err(|e| ExecError::Program(format!("link log: {e}")))?;

    linker
        .func_wrap(
            "env",
            "input_get",
            |mut caller: Caller<'_, StateCtx<S>>, np: i32, nl: i32, op: i32, oc: i32| -> i32 {
                let mem = state_memory(&caller);
                let name = state_read_string(&caller, &mem, np, nl);
                let found = caller.data().inputs.get(&name).cloned();
                state_write_out(&mut caller, &mem, op, oc, found)
            },
        )
        .map_err(|e| ExecError::Program(format!("link input_get: {e}")))?;

    #[cfg(feature = "sql-state-access")]
    linker
        .func_wrap(
            "env",
            "sql_query",
            |mut caller: Caller<'_, StateCtx<S>>,
             dp: i32,
             dl: i32,
             sp: i32,
             sl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let db = state_read_string(&caller, &mem, dp, dl);
                let sql = state_read_string(&caller, &mem, sp, sl);
                match caller.data_mut().state.sql_query_cbor(&db, &sql) {
                    Ok(bytes) => Ok(state_write_out(&mut caller, &mem, op, oc, Some(bytes))),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link sql_query: {e}")))?;

    #[cfg(feature = "sql-state-access")]
    linker
        .func_wrap(
            "env",
            "sql_exec",
            |mut caller: Caller<'_, StateCtx<S>>,
             dp: i32,
             dl: i32,
             sp: i32,
             sl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let db = state_read_string(&caller, &mem, dp, dl);
                let sql = state_read_string(&caller, &mem, sp, sl);
                match caller.data_mut().state.sql_exec_cbor(&db, &sql) {
                    Ok(bytes) => Ok(state_write_out(&mut caller, &mem, op, oc, Some(bytes))),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link sql_exec: {e}")))?;

    linker
        .func_wrap(
            "env",
            "kv_put",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             kp: i32,
             kl: i32,
             vp: i32,
             vl: i32|
             -> Result<(), wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let key_bytes = state_read_bytes(&caller, &mem, kp, kl);
                let value = state_read_bytes(&caller, &mem, vp, vl);
                let key = key_from_cbor(&key_bytes).map_err(|_| malformed_kv_key())?;
                caller
                    .data_mut()
                    .state
                    .kv_put(&collection, key, value)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link kv_put: {e}")))?;

    linker
        .func_wrap(
            "env",
            "kv_get",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             kp: i32,
             kl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let key_bytes = state_read_bytes(&caller, &mem, kp, kl);
                let key = key_from_cbor(&key_bytes).map_err(|_| malformed_kv_key())?;
                match caller.data_mut().state.kv_get(&collection, &key) {
                    Ok(found) => Ok(state_write_out(&mut caller, &mem, op, oc, found)),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link kv_get: {e}")))?;

    linker
        .func_wrap(
            "env",
            "kv_delete",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             kp: i32,
             kl: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let key_bytes = state_read_bytes(&caller, &mem, kp, kl);
                let key = key_from_cbor(&key_bytes).map_err(|_| malformed_kv_key())?;
                caller
                    .data_mut()
                    .state
                    .kv_delete(&collection, &key)
                    .map(|deleted| if deleted { 1 } else { 0 })
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link kv_delete: {e}")))?;

    linker
        .func_wrap(
            "env",
            "kv_len",
            |mut caller: Caller<'_, StateCtx<S>>, cp: i32, cl: i32| -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                caller
                    .data_mut()
                    .state
                    .kv_list(&collection)
                    .map(|list| i32::try_from(list.len()).unwrap_or(i32::MAX))
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link kv_len: {e}")))?;

    linker
        .func_wrap(
            "env",
            "kv_scan",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             lop: i32,
             lol: i32,
             hip: i32,
             hil: i32,
             limit: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let lo_bytes = state_read_bytes(&caller, &mem, lop, lol);
                let hi_bytes = state_read_bytes(&caller, &mem, hip, hil);
                let lo = key_from_cbor(&lo_bytes).map_err(|_| malformed_kv_key())?;
                let hi = key_from_cbor(&hi_bytes).map_err(|_| malformed_kv_key())?;
                let limit = usize::try_from(limit).unwrap_or(0);
                match caller
                    .data_mut()
                    .state
                    .kv_scan(&collection, &lo, &hi, limit)
                {
                    Ok(entries) => {
                        let blob = encode_scan_entries(&entries);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link kv_scan: {e}")))?;

    linker
        .func_wrap(
            "env",
            "doc_put",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             ip: i32,
             il: i32,
             dp: i32,
             dl: i32|
             -> Result<(), wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let id = state_read_string(&caller, &mem, ip, il);
                let doc = state_read_bytes(&caller, &mem, dp, dl);
                caller
                    .data_mut()
                    .state
                    .doc_put(&collection, &id, doc)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link doc_put: {e}")))?;

    linker
        .func_wrap(
            "env",
            "doc_get",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             ip: i32,
             il: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let id = state_read_string(&caller, &mem, ip, il);
                match caller.data_mut().state.doc_get(&collection, &id) {
                    Ok(found) => Ok(state_write_out(&mut caller, &mem, op, oc, found)),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link doc_get: {e}")))?;

    linker
        .func_wrap(
            "env",
            "doc_delete",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             ip: i32,
             il: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let id = state_read_string(&caller, &mem, ip, il);
                caller
                    .data_mut()
                    .state
                    .doc_delete(&collection, &id)
                    .map(i32::from)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link doc_delete: {e}")))?;

    linker
        .func_wrap(
            "env",
            "ledger_append",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             pp: i32,
             pl: i32|
             -> Result<i64, wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let payload = state_read_bytes(&caller, &mem, pp, pl);
                caller
                    .data_mut()
                    .state
                    .ledger_append(&collection, payload)
                    .map(|seq| i64::try_from(seq).unwrap_or(i64::MAX))
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link ledger_append: {e}")))?;

    linker
        .func_wrap(
            "env",
            "ledger_get",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             seq: i64,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let seq = u64::try_from(seq).unwrap_or(u64::MAX);
                match caller.data_mut().state.ledger_get(&collection, seq) {
                    Ok(found) => Ok(state_write_out(&mut caller, &mem, op, oc, found)),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link ledger_get: {e}")))?;

    linker
        .func_wrap(
            "env",
            "ledger_len",
            |mut caller: Caller<'_, StateCtx<S>>, cp: i32, cl: i32| -> Result<i64, wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                caller
                    .data_mut()
                    .state
                    .ledger_len(&collection)
                    .map(|n| i64::try_from(n).unwrap_or(i64::MAX))
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link ledger_len: {e}")))?;

    linker
        .func_wrap(
            "env",
            "cas_put",
            |mut caller: Caller<'_, StateCtx<S>>,
             dp: i32,
             dl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let data = state_read_bytes(&caller, &mem, dp, dl);
                match caller.data_mut().state.cas_put(&data) {
                    Ok(digest) => Ok(state_write_out(
                        &mut caller,
                        &mem,
                        op,
                        oc,
                        Some(digest.bytes().to_vec()),
                    )),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link cas_put: {e}")))?;

    linker
        .func_wrap(
            "env",
            "cas_get",
            |mut caller: Caller<'_, StateCtx<S>>,
             kp: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let raw = read_digest(&caller, &mem, kp);
                match caller.data_mut().state.cas_get_raw(raw) {
                    Ok(found) => Ok(state_write_out(&mut caller, &mem, op, oc, found)),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link cas_get: {e}")))?;

    linker
        .func_wrap(
            "env",
            "cas_has",
            |mut caller: Caller<'_, StateCtx<S>>, kp: i32| -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let raw = read_digest(&caller, &mem, kp);
                caller
                    .data_mut()
                    .state
                    .cas_has_raw(raw)
                    .map(i32::from)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link cas_has: {e}")))?;

    linker
        .func_wrap(
            "env",
            "cas_delete",
            |mut caller: Caller<'_, StateCtx<S>>, kp: i32| -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let raw = read_digest(&caller, &mem, kp);
                caller
                    .data_mut()
                    .state
                    .cas_delete_raw(raw)
                    .map(i32::from)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link cas_delete: {e}")))?;

    linker
        .func_wrap(
            "env",
            "queue_append",
            |mut caller: Caller<'_, StateCtx<S>>,
             sp: i32,
             sl: i32,
             pp: i32,
             pl: i32|
             -> Result<i64, wasmi::Error> {
                let mem = state_memory(&caller);
                let stream = state_read_string(&caller, &mem, sp, sl);
                let entry = state_read_bytes(&caller, &mem, pp, pl);
                caller
                    .data_mut()
                    .state
                    .queue_append(&stream, &entry)
                    .map(|seq| i64::try_from(seq).unwrap_or(i64::MAX))
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link queue_append: {e}")))?;

    linker
        .func_wrap(
            "env",
            "queue_get",
            |mut caller: Caller<'_, StateCtx<S>>,
             sp: i32,
             sl: i32,
             seq: i64,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let stream = state_read_string(&caller, &mem, sp, sl);
                let seq = usize::try_from(seq).unwrap_or(usize::MAX);
                match caller.data_mut().state.queue_get(&stream, seq) {
                    Ok(found) => Ok(state_write_out(&mut caller, &mem, op, oc, found)),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link queue_get: {e}")))?;

    linker
        .func_wrap(
            "env",
            "queue_range",
            |mut caller: Caller<'_, StateCtx<S>>,
             sp: i32,
             sl: i32,
             lo: i64,
             hi: i64,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let stream = state_read_string(&caller, &mem, sp, sl);
                let lo = usize::try_from(lo).unwrap_or(0);
                let hi = usize::try_from(hi).unwrap_or(usize::MAX);
                match caller.data_mut().state.queue_range(&stream, lo, hi) {
                    Ok(entries) => {
                        let blob = encode_bytes_list(&entries);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link queue_range: {e}")))?;

    linker
        .func_wrap(
            "env",
            "queue_len",
            |mut caller: Caller<'_, StateCtx<S>>, sp: i32, sl: i32| -> Result<i64, wasmi::Error> {
                let mem = state_memory(&caller);
                let stream = state_read_string(&caller, &mem, sp, sl);
                caller
                    .data_mut()
                    .state
                    .queue_len(&stream)
                    .map(|n| i64::try_from(n).unwrap_or(i64::MAX))
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link queue_len: {e}")))?;

    linker
        .func_wrap(
            "env",
            "graph_neighbors",
            |mut caller: Caller<'_, StateCtx<S>>,
             gp: i32,
             gl: i32,
             ip: i32,
             il: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let graph = state_read_string(&caller, &mem, gp, gl);
                let id = state_read_string(&caller, &mem, ip, il);
                match caller.data_mut().state.graph_neighbors(&graph, &id) {
                    Ok(list) => {
                        let blob = encode_string_list(&list);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link graph_neighbors: {e}")))?;

    linker
        .func_wrap(
            "env",
            "graph_get_node",
            |mut caller: Caller<'_, StateCtx<S>>,
             gp: i32,
             gl: i32,
             ip: i32,
             il: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let graph = state_read_string(&caller, &mem, gp, gl);
                let id = state_read_string(&caller, &mem, ip, il);
                match caller.data_mut().state.graph_get_node(&graph, &id) {
                    Ok(Some(props)) => {
                        let blob = encode_props(&props);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Ok(None) => Ok(state_write_out(&mut caller, &mem, op, oc, None)),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link graph_get_node: {e}")))?;

    linker
        .func_wrap(
            "env",
            "graph_get_edge",
            |mut caller: Caller<'_, StateCtx<S>>,
             gp: i32,
             gl: i32,
             ip: i32,
             il: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let graph = state_read_string(&caller, &mem, gp, gl);
                let id = state_read_string(&caller, &mem, ip, il);
                match caller.data_mut().state.graph_get_edge(&graph, &id) {
                    Ok(Some(edge)) => {
                        let blob = encode_edge(&edge);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Ok(None) => Ok(state_write_out(&mut caller, &mem, op, oc, None)),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link graph_get_edge: {e}")))?;

    linker
        .func_wrap(
            "env",
            "graph_out_edges",
            |mut caller: Caller<'_, StateCtx<S>>,
             gp: i32,
             gl: i32,
             ip: i32,
             il: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let graph = state_read_string(&caller, &mem, gp, gl);
                let id = state_read_string(&caller, &mem, ip, il);
                match caller.data_mut().state.graph_out_edges(&graph, &id) {
                    Ok(edges) => {
                        let blob = encode_edge_list(&edges);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link graph_out_edges: {e}")))?;

    linker
        .func_wrap(
            "env",
            "graph_in_edges",
            |mut caller: Caller<'_, StateCtx<S>>,
             gp: i32,
             gl: i32,
             ip: i32,
             il: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let graph = state_read_string(&caller, &mem, gp, gl);
                let id = state_read_string(&caller, &mem, ip, il);
                match caller.data_mut().state.graph_in_edges(&graph, &id) {
                    Ok(edges) => {
                        let blob = encode_edge_list(&edges);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link graph_in_edges: {e}")))?;

    linker
        .func_wrap(
            "env",
            "graph_upsert_node",
            |mut caller: Caller<'_, StateCtx<S>>,
             gp: i32,
             gl: i32,
             ip: i32,
             il: i32,
             pp: i32,
             pl: i32|
             -> Result<(), wasmi::Error> {
                let mem = state_memory(&caller);
                let graph = state_read_string(&caller, &mem, gp, gl);
                let id = state_read_string(&caller, &mem, ip, il);
                let props_bytes = state_read_bytes(&caller, &mem, pp, pl);
                let props = decode_props(&props_bytes).ok_or_else(malformed_props)?;
                caller
                    .data_mut()
                    .state
                    .graph_upsert_node(&graph, &id, props)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link graph_upsert_node: {e}")))?;

    linker
        .func_wrap(
            "env",
            "graph_upsert_edge",
            |mut caller: Caller<'_, StateCtx<S>>,
             gp: i32,
             gl: i32,
             ip: i32,
             il: i32,
             sp: i32,
             sl: i32,
             dp: i32,
             dl: i32,
             lp: i32,
             ll: i32,
             pp: i32,
             pl: i32|
             -> Result<(), wasmi::Error> {
                let mem = state_memory(&caller);
                let graph = state_read_string(&caller, &mem, gp, gl);
                let id = state_read_string(&caller, &mem, ip, il);
                let src = state_read_string(&caller, &mem, sp, sl);
                let dst = state_read_string(&caller, &mem, dp, dl);
                let label = state_read_string(&caller, &mem, lp, ll);
                let props_bytes = state_read_bytes(&caller, &mem, pp, pl);
                let props = decode_props(&props_bytes).ok_or_else(malformed_props)?;
                caller
                    .data_mut()
                    .state
                    .graph_upsert_edge(&graph, &id, &src, &dst, &label, props)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link graph_upsert_edge: {e}")))?;

    linker
        .func_wrap(
            "env",
            "graph_remove_node",
            |mut caller: Caller<'_, StateCtx<S>>,
             gp: i32,
             gl: i32,
             ip: i32,
             il: i32,
             cascade: i32|
             -> Result<(), wasmi::Error> {
                let mem = state_memory(&caller);
                let graph = state_read_string(&caller, &mem, gp, gl);
                let id = state_read_string(&caller, &mem, ip, il);
                caller
                    .data_mut()
                    .state
                    .graph_remove_node(&graph, &id, cascade != 0)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link graph_remove_node: {e}")))?;

    linker
        .func_wrap(
            "env",
            "graph_remove_edge",
            |mut caller: Caller<'_, StateCtx<S>>,
             gp: i32,
             gl: i32,
             ip: i32,
             il: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let graph = state_read_string(&caller, &mem, gp, gl);
                let id = state_read_string(&caller, &mem, ip, il);
                caller
                    .data_mut()
                    .state
                    .graph_remove_edge(&graph, &id)
                    .map(i32::from)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link graph_remove_edge: {e}")))?;

    linker
        .func_wrap(
            "env",
            "graph_reachable",
            |mut caller: Caller<'_, StateCtx<S>>,
             gp: i32,
             gl: i32,
             ip: i32,
             il: i32,
             max_depth: i32,
             lp: i32,
             ll: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let graph = state_read_string(&caller, &mem, gp, gl);
                let start = state_read_string(&caller, &mem, ip, il);
                // Optional-arg convention: `max_depth < 0` means None (unbounded); a via-label length
                // `< 0` means None (an empty label with length 0 is Some("")).
                let depth = if max_depth < 0 {
                    None
                } else {
                    Some(usize::try_from(max_depth).unwrap_or(usize::MAX))
                };
                let via = if ll < 0 {
                    None
                } else {
                    Some(state_read_string(&caller, &mem, lp, ll))
                };
                match caller
                    .data_mut()
                    .state
                    .graph_reachable(&graph, &start, depth, via.as_deref())
                {
                    Ok(list) => {
                        let blob = encode_string_list(&list);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link graph_reachable: {e}")))?;

    linker
        .func_wrap(
            "env",
            "graph_shortest_path",
            |mut caller: Caller<'_, StateCtx<S>>,
             gp: i32,
             gl: i32,
             fp: i32,
             fl: i32,
             tp: i32,
             tl: i32,
             lp: i32,
             ll: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let graph = state_read_string(&caller, &mem, gp, gl);
                let from = state_read_string(&caller, &mem, fp, fl);
                let to = state_read_string(&caller, &mem, tp, tl);
                let via = if ll < 0 {
                    None
                } else {
                    Some(state_read_string(&caller, &mem, lp, ll))
                };
                match caller.data_mut().state.graph_shortest_path(
                    &graph,
                    &from,
                    &to,
                    via.as_deref(),
                ) {
                    // A found path is a canonical text array (endpoints inclusive); no path is the
                    // absent-value sentinel (-1), distinct from an empty array.
                    Ok(Some(path)) => {
                        let blob = encode_string_list(&path);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Ok(None) => Ok(state_write_out(&mut caller, &mem, op, oc, None)),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link graph_shortest_path: {e}")))?;

    linker
        .func_wrap(
            "env",
            "vector_create",
            |mut caller: Caller<'_, StateCtx<S>>,
             sp: i32,
             sl: i32,
             dim: i32,
             metric: i32|
             -> Result<(), wasmi::Error> {
                let mem = state_memory(&caller);
                let set = state_read_string(&caller, &mem, sp, sl);
                let dim = usize::try_from(dim).map_err(|_| malformed_vector())?;
                let tag = u8::try_from(metric).map_err(|_| malformed_vector())?;
                let metric =
                    loom_core::vector::Metric::from_tag(tag).map_err(|_| malformed_vector())?;
                caller
                    .data_mut()
                    .state
                    .vector_create(&set, dim, metric)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link vector_create: {e}")))?;

    linker
        .func_wrap(
            "env",
            "vector_upsert",
            |mut caller: Caller<'_, StateCtx<S>>,
             sp: i32,
             sl: i32,
             ip: i32,
             il: i32,
             vp: i32,
             vl: i32,
             mp: i32,
             ml: i32|
             -> Result<(), wasmi::Error> {
                let mem = state_memory(&caller);
                let set = state_read_string(&caller, &mem, sp, sl);
                let id = state_read_string(&caller, &mem, ip, il);
                let vec_bytes = state_read_bytes(&caller, &mem, vp, vl);
                let meta_bytes = state_read_bytes(&caller, &mem, mp, ml);
                let vector = decode_f32_vec(&vec_bytes).ok_or_else(malformed_vector)?;
                let metadata = decode_meta(&meta_bytes).ok_or_else(malformed_vector)?;
                caller
                    .data_mut()
                    .state
                    .vector_upsert(&set, &id, vector, metadata)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link vector_upsert: {e}")))?;

    linker
        .func_wrap(
            "env",
            "vector_get",
            |mut caller: Caller<'_, StateCtx<S>>,
             sp: i32,
             sl: i32,
             ip: i32,
             il: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let set = state_read_string(&caller, &mem, sp, sl);
                let id = state_read_string(&caller, &mem, ip, il);
                match caller.data_mut().state.vector_get(&set, &id) {
                    Ok(Some((vector, meta))) => {
                        let blob = encode_vector_entry(&vector, &meta);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Ok(None) => Ok(state_write_out(&mut caller, &mem, op, oc, None)),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link vector_get: {e}")))?;

    linker
        .func_wrap(
            "env",
            "vector_delete",
            |mut caller: Caller<'_, StateCtx<S>>,
             sp: i32,
             sl: i32,
             ip: i32,
             il: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let set = state_read_string(&caller, &mem, sp, sl);
                let id = state_read_string(&caller, &mem, ip, il);
                caller
                    .data_mut()
                    .state
                    .vector_delete(&set, &id)
                    .map(i32::from)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link vector_delete: {e}")))?;

    linker
        .func_wrap(
            "env",
            "vector_ids",
            |mut caller: Caller<'_, StateCtx<S>>,
             sp: i32,
             sl: i32,
             pp: i32,
             pl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let set = state_read_string(&caller, &mem, sp, sl);
                // Optional prefix: a length `< 0` means None (an empty prefix with length 0 is Some("")).
                let prefix = if pl < 0 {
                    None
                } else {
                    Some(state_read_string(&caller, &mem, pp, pl))
                };
                match caller.data_mut().state.vector_ids(&set, prefix.as_deref()) {
                    Ok(list) => {
                        let blob = encode_string_list(&list);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link vector_ids: {e}")))?;

    linker
        .func_wrap(
            "env",
            "vector_search",
            |mut caller: Caller<'_, StateCtx<S>>,
             sp: i32,
             sl: i32,
             qp: i32,
             ql: i32,
             k: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let set = state_read_string(&caller, &mem, sp, sl);
                let q_bytes = state_read_bytes(&caller, &mem, qp, ql);
                let query = decode_f32_vec(&q_bytes).ok_or_else(malformed_vector)?;
                let k = usize::try_from(k).unwrap_or(0);
                // Unfiltered nearest-neighbour; the metadata-filter DSL is a separate ABI unit.
                let filter = loom_core::vector::MetaFilter::All;
                match caller
                    .data_mut()
                    .state
                    .vector_search(&set, &query, k, &filter)
                {
                    Ok(hits) => {
                        let blob = encode_hits(&hits);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link vector_search: {e}")))?;

    linker
        .func_wrap(
            "env",
            "vector_search_filtered",
            |mut caller: Caller<'_, StateCtx<S>>,
             sp: i32,
             sl: i32,
             qp: i32,
             ql: i32,
             k: i32,
             fp: i32,
             fl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let set = state_read_string(&caller, &mem, sp, sl);
                let q_bytes = state_read_bytes(&caller, &mem, qp, ql);
                let filter_bytes = state_read_bytes(&caller, &mem, fp, fl);
                let query = decode_f32_vec(&q_bytes).ok_or_else(malformed_vector)?;
                let filter = decode_vector_filter(&filter_bytes).ok_or_else(malformed_vector)?;
                let k = usize::try_from(k).unwrap_or(0);
                match caller
                    .data_mut()
                    .state
                    .vector_search(&set, &query, k, &filter)
                {
                    Ok(hits) => {
                        let blob = encode_hits(&hits);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link vector_search_filtered: {e}")))?;

    linker
        .func_wrap(
            "env",
            "search_create",
            |mut caller: Caller<'_, StateCtx<S>>, cp: i32, cl: i32, mp: i32, ml: i32| {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let mapping_bytes = state_read_bytes(&caller, &mem, mp, ml);
                let mapping =
                    search_mapping_from_cbor(&mapping_bytes).map_err(|_| malformed_search())?;
                caller
                    .data_mut()
                    .state
                    .search_create(&collection, mapping)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link search_create: {e}")))?;

    linker
        .func_wrap(
            "env",
            "search_index",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             ip: i32,
             il: i32,
             dp: i32,
             dl: i32| {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let id = state_read_bytes(&caller, &mem, ip, il);
                let doc_bytes = state_read_bytes(&caller, &mem, dp, dl);
                let doc = search_document_from_cbor(&doc_bytes).map_err(|_| malformed_search())?;
                caller
                    .data_mut()
                    .state
                    .search_index(&collection, id, doc)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link search_index: {e}")))?;

    linker
        .func_wrap(
            "env",
            "search_get",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             ip: i32,
             il: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let id = state_read_bytes(&caller, &mem, ip, il);
                match caller.data_mut().state.search_get(&collection, &id) {
                    Ok(found) => Ok(state_write_out(
                        &mut caller,
                        &mem,
                        op,
                        oc,
                        found.map(|doc| search_document_cbor(&doc)),
                    )),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link search_get: {e}")))?;

    linker
        .func_wrap(
            "env",
            "search_delete",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             ip: i32,
             il: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let id = state_read_bytes(&caller, &mem, ip, il);
                caller
                    .data_mut()
                    .state
                    .search_delete(&collection, &id)
                    .map(i32::from)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link search_delete: {e}")))?;

    linker
        .func_wrap(
            "env",
            "search_ids",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             pp: i32,
             pl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let prefix = if pl < 0 {
                    None
                } else {
                    Some(state_read_bytes(&caller, &mem, pp, pl))
                };
                match caller
                    .data_mut()
                    .state
                    .search_ids(&collection, prefix.as_deref())
                {
                    Ok(ids) => Ok(state_write_out(
                        &mut caller,
                        &mem,
                        op,
                        oc,
                        Some(search_ids_cbor(ids)),
                    )),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link search_ids: {e}")))?;

    linker
        .func_wrap(
            "env",
            "search_query",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             rp: i32,
             rl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let request_bytes = state_read_bytes(&caller, &mem, rp, rl);
                let request =
                    search_request_from_cbor(&request_bytes).map_err(|_| malformed_search())?;
                match caller.data_mut().state.search_query(&collection, &request) {
                    Ok(response) => Ok(state_write_out(
                        &mut caller,
                        &mem,
                        op,
                        oc,
                        Some(search_response_cbor(&response)),
                    )),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link search_query: {e}")))?;

    linker
        .func_wrap(
            "env",
            "columnar_create",
            |mut caller: Caller<'_, StateCtx<S>>,
             dp: i32,
             dl: i32,
             cp: i32,
             cl: i32,
             target: i32|
             -> Result<(), wasmi::Error> {
                let mem = state_memory(&caller);
                let dataset = state_read_string(&caller, &mem, dp, dl);
                let cols_bytes = state_read_bytes(&caller, &mem, cp, cl);
                let columns = decode_columns(&cols_bytes).ok_or_else(malformed_columnar)?;
                let target_segment_rows = usize::try_from(target).unwrap_or(0);
                caller
                    .data_mut()
                    .state
                    .columnar_create(&dataset, columns, target_segment_rows)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link columnar_create: {e}")))?;

    linker
        .func_wrap(
            "env",
            "columnar_append",
            |mut caller: Caller<'_, StateCtx<S>>,
             dp: i32,
             dl: i32,
             rp: i32,
             rl: i32|
             -> Result<(), wasmi::Error> {
                let mem = state_memory(&caller);
                let dataset = state_read_string(&caller, &mem, dp, dl);
                let row_bytes = state_read_bytes(&caller, &mem, rp, rl);
                let row = decode_row(&row_bytes).ok_or_else(malformed_columnar)?;
                caller
                    .data_mut()
                    .state
                    .columnar_append(&dataset, row)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link columnar_append: {e}")))?;

    linker
        .func_wrap(
            "env",
            "columnar_scan",
            |mut caller: Caller<'_, StateCtx<S>>,
             dp: i32,
             dl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let dataset = state_read_string(&caller, &mem, dp, dl);
                match caller.data_mut().state.columnar_scan(&dataset) {
                    Ok(rows) => {
                        let blob = encode_rows(&rows);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link columnar_scan: {e}")))?;

    linker
        .func_wrap(
            "env",
            "columnar_columns",
            |mut caller: Caller<'_, StateCtx<S>>,
             dp: i32,
             dl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let dataset = state_read_string(&caller, &mem, dp, dl);
                match caller.data_mut().state.columnar_columns(&dataset) {
                    Ok(columns) => {
                        let blob = encode_columns(&columns);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link columnar_columns: {e}")))?;

    linker
        .func_wrap(
            "env",
            "columnar_rows",
            |mut caller: Caller<'_, StateCtx<S>>, dp: i32, dl: i32| -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let dataset = state_read_string(&caller, &mem, dp, dl);
                caller
                    .data_mut()
                    .state
                    .columnar_rows(&dataset)
                    .map(|n| i32::try_from(n).unwrap_or(i32::MAX))
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link columnar_rows: {e}")))?;

    linker
        .func_wrap(
            "env",
            "columnar_select",
            |mut caller: Caller<'_, StateCtx<S>>,
             dp: i32,
             dl: i32,
             cp: i32,
             cl: i32,
             fp: i32,
             fl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let dataset = state_read_string(&caller, &mem, dp, dl);
                let columns_bytes = state_read_bytes(&caller, &mem, cp, cl);
                let filter_bytes = state_read_bytes(&caller, &mem, fp, fl);
                let columns = decode_columnar_select_columns(&columns_bytes)
                    .ok_or_else(malformed_columnar)?;
                let filter =
                    decode_columnar_filter(&filter_bytes).ok_or_else(malformed_columnar)?;
                let column_refs = columns.iter().map(String::as_str).collect::<Vec<_>>();
                let filter_ref = filter
                    .as_ref()
                    .map(|(column, op, value)| (column.as_str(), *op, value));
                match caller
                    .data_mut()
                    .state
                    .columnar_select(&dataset, &column_refs, filter_ref)
                {
                    Ok(rows) => {
                        let blob = encode_rows(&rows);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link columnar_select: {e}")))?;

    linker
        .func_wrap(
            "env",
            "columnar_aggregate",
            |mut caller: Caller<'_, StateCtx<S>>,
             dp: i32,
             dl: i32,
             ap: i32,
             al: i32,
             fp: i32,
             fl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let dataset = state_read_string(&caller, &mem, dp, dl);
                let aggregate_bytes = state_read_bytes(&caller, &mem, ap, al);
                let filter_bytes = state_read_bytes(&caller, &mem, fp, fl);
                let aggregates =
                    decode_columnar_aggregates(&aggregate_bytes).ok_or_else(malformed_columnar)?;
                let filter =
                    decode_columnar_filter(&filter_bytes).ok_or_else(malformed_columnar)?;
                let filter_ref = filter
                    .as_ref()
                    .map(|(column, op, value)| (column.as_str(), *op, value));
                match caller
                    .data_mut()
                    .state
                    .columnar_aggregate(&dataset, &aggregates, filter_ref)
                {
                    Ok(values) => {
                        let blob = loom_core::tabular::encode_cells(&values);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link columnar_aggregate: {e}")))?;

    linker
        .func_wrap(
            "env",
            "dataframe_create",
            |mut caller: Caller<'_, StateCtx<S>>, fp: i32, fl: i32, pp: i32, pl: i32| {
                let mem = state_memory(&caller);
                let frame = state_read_string(&caller, &mem, fp, fl);
                let plan_bytes = state_read_bytes(&caller, &mem, pp, pl);
                let plan = DataframePlan::decode(&plan_bytes).map_err(|_| malformed_dataframe())?;
                caller
                    .data_mut()
                    .state
                    .dataframe_create(&frame, &plan)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link dataframe_create: {e}")))?;

    linker
        .func_wrap(
            "env",
            "dataframe_put_plan",
            |mut caller: Caller<'_, StateCtx<S>>, fp: i32, fl: i32, pp: i32, pl: i32| {
                let mem = state_memory(&caller);
                let frame = state_read_string(&caller, &mem, fp, fl);
                let plan_bytes = state_read_bytes(&caller, &mem, pp, pl);
                let plan = DataframePlan::decode(&plan_bytes).map_err(|_| malformed_dataframe())?;
                caller
                    .data_mut()
                    .state
                    .dataframe_put_plan(&frame, &plan)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link dataframe_put_plan: {e}")))?;

    linker
        .func_wrap(
            "env",
            "dataframe_get_plan",
            |mut caller: Caller<'_, StateCtx<S>>,
             fp: i32,
             fl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let frame = state_read_string(&caller, &mem, fp, fl);
                match caller.data_mut().state.dataframe_get_plan(&frame) {
                    Ok(plan) => Ok(state_write_out(
                        &mut caller,
                        &mem,
                        op,
                        oc,
                        Some(plan.encode()),
                    )),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link dataframe_get_plan: {e}")))?;

    linker
        .func_wrap(
            "env",
            "dataframe_collect",
            |mut caller: Caller<'_, StateCtx<S>>,
             fp: i32,
             fl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let frame = state_read_string(&caller, &mem, fp, fl);
                match caller.data_mut().state.dataframe_collect(&frame) {
                    Ok(batch) => Ok(state_write_out(
                        &mut caller,
                        &mem,
                        op,
                        oc,
                        Some(batch.encode()),
                    )),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link dataframe_collect: {e}")))?;

    linker
        .func_wrap(
            "env",
            "dataframe_preview",
            |mut caller: Caller<'_, StateCtx<S>>,
             fp: i32,
             fl: i32,
             rows: i64,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let frame = state_read_string(&caller, &mem, fp, fl);
                let rows = u64::try_from(rows).unwrap_or(0);
                match caller.data_mut().state.dataframe_preview(&frame, rows) {
                    Ok(batch) => Ok(state_write_out(
                        &mut caller,
                        &mem,
                        op,
                        oc,
                        Some(batch.encode()),
                    )),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link dataframe_preview: {e}")))?;

    linker
        .func_wrap(
            "env",
            "dataframe_materialize",
            |mut caller: Caller<'_, StateCtx<S>>,
             fp: i32,
             fl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let frame = state_read_string(&caller, &mem, fp, fl);
                match caller.data_mut().state.dataframe_materialize(&frame) {
                    Ok(digest) => Ok(state_write_out(
                        &mut caller,
                        &mem,
                        op,
                        oc,
                        digest.map(|digest| digest.to_string().into_bytes()),
                    )),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link dataframe_materialize: {e}")))?;

    linker
        .func_wrap(
            "env",
            "dataframe_plan_digest",
            |mut caller: Caller<'_, StateCtx<S>>,
             fp: i32,
             fl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let frame = state_read_string(&caller, &mem, fp, fl);
                match caller.data_mut().state.dataframe_plan_digest(&frame) {
                    Ok(digest) => Ok(state_write_out(
                        &mut caller,
                        &mem,
                        op,
                        oc,
                        Some(digest.to_string().into_bytes()),
                    )),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link dataframe_plan_digest: {e}")))?;

    linker
        .func_wrap(
            "env",
            "dataframe_source_digests",
            |mut caller: Caller<'_, StateCtx<S>>,
             fp: i32,
             fl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let frame = state_read_string(&caller, &mem, fp, fl);
                match caller.data_mut().state.dataframe_source_digests(&frame) {
                    Ok(digests) => {
                        let blob = loom_codec::encode(&loom_codec::Value::Array(
                            digests
                                .into_iter()
                                .map(|digest| loom_codec::Value::Text(digest.to_string()))
                                .collect(),
                        ))
                        .expect("digest string array encodes");
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link dataframe_source_digests: {e}")))?;

    linker
        .func_wrap(
            "env",
            "ts_put",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             ts: i64,
             vp: i32,
             vl: i32|
             -> Result<(), wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let value = state_read_bytes(&caller, &mem, vp, vl);
                caller
                    .data_mut()
                    .state
                    .time_series_put(&collection, ts, value)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link ts_put: {e}")))?;

    linker
        .func_wrap(
            "env",
            "ts_get",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             ts: i64,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                match caller.data_mut().state.time_series_get(&collection, ts) {
                    Ok(found) => Ok(state_write_out(&mut caller, &mem, op, oc, found)),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link ts_get: {e}")))?;

    linker
        .func_wrap(
            "env",
            "ts_latest",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                match caller.data_mut().state.time_series_latest(&collection) {
                    Ok(Some((ts, value))) => {
                        let blob = encode_ts_point(ts, &value);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Ok(None) => Ok(state_write_out(&mut caller, &mem, op, oc, None)),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link ts_latest: {e}")))?;

    linker
        .func_wrap(
            "env",
            "ts_range",
            |mut caller: Caller<'_, StateCtx<S>>,
             cp: i32,
             cl: i32,
             from: i64,
             to: i64,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let collection = state_read_string(&caller, &mem, cp, cl);
                match caller
                    .data_mut()
                    .state
                    .time_series_range(&collection, from, to)
                {
                    Ok(series) => {
                        let points: Vec<(i64, Vec<u8>)> =
                            series.iter().map(|(t, v)| (t, v.to_vec())).collect();
                        let blob = encode_ts_points(&points);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link ts_range: {e}")))?;

    linker
        .func_wrap(
            "env",
            "calendar_create_collection",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             cp: i32,
             cl: i32,
             mp: i32,
             ml: i32|
             -> Result<(), wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let meta_bytes = state_read_bytes(&caller, &mem, mp, ml);
                let meta = CollectionMeta::decode(&meta_bytes).map_err(|_| malformed_pim())?;
                caller
                    .data_mut()
                    .state
                    .calendar_create_collection(&principal, &collection, &meta)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link calendar_create_collection: {e}")))?;

    linker
        .func_wrap(
            "env",
            "calendar_get_collection",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             cp: i32,
             cl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let collection = state_read_string(&caller, &mem, cp, cl);
                match caller
                    .data_mut()
                    .state
                    .calendar_get_collection(&principal, &collection)
                {
                    Ok(found) => Ok(state_write_out(
                        &mut caller,
                        &mem,
                        op,
                        oc,
                        found.map(|meta| meta.encode()),
                    )),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link calendar_get_collection: {e}")))?;

    linker
        .func_wrap(
            "env",
            "calendar_list_collections",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                match caller
                    .data_mut()
                    .state
                    .calendar_list_collections(&principal)
                {
                    Ok(list) => {
                        let blob = encode_string_list(&list);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link calendar_list_collections: {e}")))?;

    linker
        .func_wrap(
            "env",
            "calendar_put_entry",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             cp: i32,
             cl: i32,
             ep: i32,
             el: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let entry_bytes = state_read_bytes(&caller, &mem, ep, el);
                let entry = CalendarEntry::decode(&entry_bytes).map_err(|_| malformed_pim())?;
                match caller
                    .data_mut()
                    .state
                    .calendar_put_entry(&principal, &collection, &entry)
                {
                    Ok(digest) => Ok(state_write_out(
                        &mut caller,
                        &mem,
                        op,
                        oc,
                        Some(digest.bytes().to_vec()),
                    )),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link calendar_put_entry: {e}")))?;

    linker
        .func_wrap(
            "env",
            "calendar_get_entry",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             cp: i32,
             cl: i32,
             up: i32,
             ul: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let uid = state_read_string(&caller, &mem, up, ul);
                match caller
                    .data_mut()
                    .state
                    .calendar_get_entry(&principal, &collection, &uid)
                {
                    Ok(found) => Ok(state_write_out(
                        &mut caller,
                        &mem,
                        op,
                        oc,
                        found.map(|entry| entry.encode()),
                    )),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link calendar_get_entry: {e}")))?;

    linker
        .func_wrap(
            "env",
            "calendar_delete_entry",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             cp: i32,
             cl: i32,
             up: i32,
             ul: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let uid = state_read_string(&caller, &mem, up, ul);
                caller
                    .data_mut()
                    .state
                    .calendar_delete_entry(&principal, &collection, &uid)
                    .map(i32::from)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link calendar_delete_entry: {e}")))?;

    linker
        .func_wrap(
            "env",
            "calendar_list_entries",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             cp: i32,
             cl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let collection = state_read_string(&caller, &mem, cp, cl);
                match caller
                    .data_mut()
                    .state
                    .calendar_list_entries(&principal, &collection)
                {
                    Ok(entries) => {
                        let blob = encode_pim_records(entries.iter().map(CalendarEntry::encode));
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link calendar_list_entries: {e}")))?;

    linker
        .func_wrap(
            "env",
            "calendar_search",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             cp: i32,
             cl: i32,
             component: i32,
             tp: i32,
             tl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let collection = state_read_string(&caller, &mem, cp, cl);
                let component = decode_calendar_component(component).ok_or_else(malformed_pim)?;
                let text = if tl < 0 {
                    None
                } else {
                    Some(state_read_string(&caller, &mem, tp, tl))
                };
                match caller.data_mut().state.calendar_search(
                    &principal,
                    &collection,
                    component,
                    text.as_deref(),
                ) {
                    Ok(entries) => {
                        let blob = encode_pim_records(entries.iter().map(CalendarEntry::encode));
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link calendar_search: {e}")))?;

    linker
        .func_wrap(
            "env",
            "contacts_create_book",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             bp: i32,
             bl: i32,
             mp: i32,
             ml: i32|
             -> Result<(), wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let book = state_read_string(&caller, &mem, bp, bl);
                let meta_bytes = state_read_bytes(&caller, &mem, mp, ml);
                let meta = BookMeta::decode(&meta_bytes).map_err(|_| malformed_pim())?;
                caller
                    .data_mut()
                    .state
                    .contacts_create_book(&principal, &book, &meta)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link contacts_create_book: {e}")))?;

    linker
        .func_wrap(
            "env",
            "contacts_put_entry",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             bp: i32,
             bl: i32,
             ep: i32,
             el: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let book = state_read_string(&caller, &mem, bp, bl);
                let entry_bytes = state_read_bytes(&caller, &mem, ep, el);
                let entry = ContactEntry::decode(&entry_bytes).map_err(|_| malformed_pim())?;
                match caller
                    .data_mut()
                    .state
                    .contacts_put_entry(&principal, &book, &entry)
                {
                    Ok(digest) => Ok(state_write_out(
                        &mut caller,
                        &mem,
                        op,
                        oc,
                        Some(digest.bytes().to_vec()),
                    )),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link contacts_put_entry: {e}")))?;

    linker
        .func_wrap(
            "env",
            "contacts_get_entry",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             bp: i32,
             bl: i32,
             up: i32,
             ul: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let book = state_read_string(&caller, &mem, bp, bl);
                let uid = state_read_string(&caller, &mem, up, ul);
                match caller
                    .data_mut()
                    .state
                    .contacts_get_entry(&principal, &book, &uid)
                {
                    Ok(found) => Ok(state_write_out(
                        &mut caller,
                        &mem,
                        op,
                        oc,
                        found.map(|entry| entry.encode()),
                    )),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link contacts_get_entry: {e}")))?;

    linker
        .func_wrap(
            "env",
            "contacts_list_entries",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             bp: i32,
             bl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let book = state_read_string(&caller, &mem, bp, bl);
                match caller
                    .data_mut()
                    .state
                    .contacts_list_entries(&principal, &book)
                {
                    Ok(entries) => {
                        let blob = encode_pim_records(entries.iter().map(ContactEntry::encode));
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link contacts_list_entries: {e}")))?;

    linker
        .func_wrap(
            "env",
            "contacts_search",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             bp: i32,
             bl: i32,
             tp: i32,
             tl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let book = state_read_string(&caller, &mem, bp, bl);
                let text = state_read_string(&caller, &mem, tp, tl);
                match caller
                    .data_mut()
                    .state
                    .contacts_search(&principal, &book, &text)
                {
                    Ok(entries) => {
                        let blob = encode_pim_records(entries.iter().map(ContactEntry::encode));
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link contacts_search: {e}")))?;

    linker
        .func_wrap(
            "env",
            "mail_create_mailbox",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             mp: i32,
             ml: i32,
             bp: i32,
             bl: i32|
             -> Result<(), wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let mailbox = state_read_string(&caller, &mem, mp, ml);
                let meta_bytes = state_read_bytes(&caller, &mem, bp, bl);
                let meta = MailboxMeta::decode(&meta_bytes).map_err(|_| malformed_pim())?;
                caller
                    .data_mut()
                    .state
                    .mail_create_mailbox(&principal, &mailbox, &meta)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link mail_create_mailbox: {e}")))?;

    linker
        .func_wrap(
            "env",
            "mail_ingest_message",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             mp: i32,
             ml: i32,
             up: i32,
             ul: i32,
             rp: i32,
             rl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let mailbox = state_read_string(&caller, &mem, mp, ml);
                let uid = state_read_string(&caller, &mem, up, ul);
                let raw = state_read_bytes(&caller, &mem, rp, rl);
                match caller
                    .data_mut()
                    .state
                    .mail_ingest_message(&principal, &mailbox, &uid, &raw)
                {
                    Ok(digest) => Ok(state_write_out(
                        &mut caller,
                        &mem,
                        op,
                        oc,
                        Some(digest.bytes().to_vec()),
                    )),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link mail_ingest_message: {e}")))?;

    linker
        .func_wrap(
            "env",
            "mail_get_message",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             mp: i32,
             ml: i32,
             up: i32,
             ul: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let mailbox = state_read_string(&caller, &mem, mp, ml);
                let uid = state_read_string(&caller, &mem, up, ul);
                match caller
                    .data_mut()
                    .state
                    .mail_get_message(&principal, &mailbox, &uid)
                {
                    Ok(found) => Ok(state_write_out(
                        &mut caller,
                        &mem,
                        op,
                        oc,
                        found.map(|message| message.encode()),
                    )),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link mail_get_message: {e}")))?;

    linker
        .func_wrap(
            "env",
            "mail_to_eml",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             mp: i32,
             ml: i32,
             up: i32,
             ul: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let mailbox = state_read_string(&caller, &mem, mp, ml);
                let uid = state_read_string(&caller, &mem, up, ul);
                match caller
                    .data_mut()
                    .state
                    .mail_to_eml(&principal, &mailbox, &uid)
                {
                    Ok(found) => Ok(state_write_out(&mut caller, &mem, op, oc, found)),
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link mail_to_eml: {e}")))?;

    linker
        .func_wrap(
            "env",
            "mail_set_flags",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             mp: i32,
             ml: i32,
             up: i32,
             ul: i32,
             fp: i32,
             fl: i32|
             -> Result<(), wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let mailbox = state_read_string(&caller, &mem, mp, ml);
                let uid = state_read_string(&caller, &mem, up, ul);
                let flags_bytes = state_read_bytes(&caller, &mem, fp, fl);
                let flags = decode_string_list(&flags_bytes).ok_or_else(malformed_pim)?;
                caller
                    .data_mut()
                    .state
                    .mail_set_flags(&principal, &mailbox, &uid, &flags)
                    .map_err(|err| set_host_error(&mut caller, err))
            },
        )
        .map_err(|e| ExecError::Program(format!("link mail_set_flags: {e}")))?;

    linker
        .func_wrap(
            "env",
            "mail_get_flags",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             mp: i32,
             ml: i32,
             up: i32,
             ul: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let mailbox = state_read_string(&caller, &mem, mp, ml);
                let uid = state_read_string(&caller, &mem, up, ul);
                match caller
                    .data_mut()
                    .state
                    .mail_get_flags(&principal, &mailbox, &uid)
                {
                    Ok(flags) => {
                        let blob = encode_string_list(&flags);
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link mail_get_flags: {e}")))?;

    linker
        .func_wrap(
            "env",
            "mail_search",
            |mut caller: Caller<'_, StateCtx<S>>,
             pp: i32,
             pl: i32,
             mp: i32,
             ml: i32,
             tp: i32,
             tl: i32,
             op: i32,
             oc: i32|
             -> Result<i32, wasmi::Error> {
                let mem = state_memory(&caller);
                let principal = state_read_string(&caller, &mem, pp, pl);
                let mailbox = state_read_string(&caller, &mem, mp, ml);
                let text = state_read_string(&caller, &mem, tp, tl);
                match caller
                    .data_mut()
                    .state
                    .mail_search(&principal, &mailbox, &text)
                {
                    Ok(messages) => {
                        let blob =
                            encode_pim_records(messages.iter().map(|message| message.encode()));
                        Ok(state_write_out(&mut caller, &mem, op, oc, Some(blob)))
                    }
                    Err(err) => Err(set_host_error(&mut caller, err)),
                }
            },
        )
        .map_err(|e| ExecError::Program(format!("link mail_search: {e}")))?;

    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|e| ExecError::Program(format!("instantiate: {e}")))?;
    let run = instance
        .get_typed_func::<(), ()>(&store, "run")
        .map_err(|e| ExecError::Program(format!("missing `run` export: {e}")))?;

    match run.call(&mut store, ()) {
        Ok(()) => {
            let remaining = store.get_fuel().unwrap_or(0);
            let fuel_used = fuel.saturating_sub(remaining);
            let data = store.into_data();
            let outcome = RunOutcome {
                fuel_used,
                logs: data.logs.into_entries(),
            };
            Ok((data.state, outcome))
        }
        Err(trap) => {
            if let Some(err) = store.data_mut().host_error.take() {
                return Err(err);
            }
            let out_of_fuel = trap.as_trap_code() == Some(wasmi::TrapCode::OutOfFuel)
                || trap.to_string().to_lowercase().contains("fuel");
            if out_of_fuel {
                Err(ExecError::BudgetExceeded { budget: fuel })
            } else {
                Err(ExecError::Program(format!("trap: {trap}")))
            }
        }
    }
}

fn state_memory<S: ObjectStore>(caller: &Caller<'_, StateCtx<S>>) -> Memory {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .expect("program must export `memory`")
}

fn state_read_bytes<S: ObjectStore>(
    caller: &Caller<'_, StateCtx<S>>,
    mem: &Memory,
    ptr: i32,
    len: i32,
) -> Vec<u8> {
    let mut buf = vec![0u8; len.max(0) as usize];
    mem.read(caller, ptr as usize, &mut buf)
        .expect("read bytes");
    buf
}

fn state_read_string<S: ObjectStore>(
    caller: &Caller<'_, StateCtx<S>>,
    mem: &Memory,
    ptr: i32,
    len: i32,
) -> String {
    String::from_utf8_lossy(&state_read_bytes(caller, mem, ptr, len)).into_owned()
}

/// Read a raw 32-byte content address from guest memory (the digest wire form for the CAS host ABI).
fn read_digest<S: ObjectStore>(
    caller: &Caller<'_, StateCtx<S>>,
    mem: &Memory,
    ptr: i32,
) -> [u8; 32] {
    let bytes = state_read_bytes(caller, mem, ptr, 32);
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    arr
}

fn encode_pim_records(records: impl Iterator<Item = Vec<u8>>) -> Vec<u8> {
    use loom_codec::Value as Cbor;
    let items = records.map(Cbor::Bytes).collect();
    loom_codec::encode(&Cbor::Array(items)).expect("a canonical CBOR PIM record list encodes")
}

fn decode_string_list(bytes: &[u8]) -> Option<Vec<String>> {
    use loom_codec::Value as Cbor;
    let Ok(Cbor::Array(items)) = loom_codec::decode(bytes) else {
        return None;
    };
    items
        .into_iter()
        .map(|item| match item {
            Cbor::Text(text) => Some(text),
            _ => None,
        })
        .collect()
}

fn decode_calendar_component(tag: i32) -> Option<Option<Component>> {
    match tag {
        -1 => Some(None),
        0 => Some(Some(Component::Event)),
        1 => Some(Some(Component::Todo)),
        _ => None,
    }
}

fn state_write_out<S: ObjectStore>(
    caller: &mut Caller<'_, StateCtx<S>>,
    mem: &Memory,
    ptr: i32,
    cap: i32,
    found: Option<Vec<u8>>,
) -> i32 {
    match found {
        Some(value) => {
            let n = value.len().min(cap.max(0) as usize);
            mem.write(caller, ptr as usize, &value[..n])
                .expect("write out");
            value.len() as i32
        }
        None => -1,
    }
}

#[cfg(test)]
mod state_tests {
    use super::*;
    use crate::authz::ExecContext;
    use crate::capability::{Grant, GrantSet, Scope};
    use loom_core::tabular::Value;
    use loom_core::vcs::Loom;
    use loom_core::{
        AclRight, AclSubject, FacetKind, MemoryStore, PrincipalId, WorkspaceId, key_to_cbor,
        kv_get, kv_list,
    };
    use std::collections::BTreeMap;

    fn nid(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    fn pid(seed: u8) -> PrincipalId {
        PrincipalId::from_bytes([seed; 16])
    }

    fn state_loom(seed: u8) -> (Loom<MemoryStore>, WorkspaceId) {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, None, nid(seed))
            .unwrap();
        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(pid(9)),
                Some(ns),
                None,
                [AclRight::Execute],
            )
            .unwrap();
        (loom, ns)
    }

    fn context(ns: WorkspaceId, grants: GrantSet) -> ExecContext {
        ExecContext {
            workspace: ns,
            principal: pid(9),
            roles: Vec::new(),
            authenticated: true,
            base_branch: "main".to_string(),
            grants,
        }
    }

    // Fetches two typed keys from inputs `nt` (Text) and `nb` (Bytes), puts cache/<text>=v1 and
    // cache/<bytes>=v2, reads the text entry into /got, deletes the text entry, and writes kv_len to
    // /count. Keys travel as their Loom canonical CBOR cell form; the guest never constructs them.
    fn typed_key_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "input_get" (func $in (param i32 i32 i32 i32) (result i32)))
                 (import "env" "kv_put" (func $put (param i32 i32 i32 i32 i32 i32)))
                 (import "env" "kv_get" (func $get (param i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "kv_delete" (func $del (param i32 i32 i32 i32) (result i32)))
                 (import "env" "kv_len" (func $len (param i32 i32) (result i32)))
                 (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "cache")
                 (data (i32.const 16) "v1")
                 (data (i32.const 32) "v2")
                 (data (i32.const 48) "nt")
                 (data (i32.const 64) "nb")
                 (data (i32.const 80) "/got")
                 (data (i32.const 96) "/count")
                 (func (export "run") (local $lt i32) (local $lb i32)
                   (local.set $lt (call $in (i32.const 48)(i32.const 2)(i32.const 200)(i32.const 64)))
                   (local.set $lb (call $in (i32.const 64)(i32.const 2)(i32.const 300)(i32.const 64)))
                   (call $put (i32.const 0)(i32.const 5)(i32.const 200)(local.get $lt)(i32.const 16)(i32.const 2))
                   (call $put (i32.const 0)(i32.const 5)(i32.const 300)(local.get $lb)(i32.const 32)(i32.const 2))
                   (drop (call $get (i32.const 0)(i32.const 5)(i32.const 200)(local.get $lt)(i32.const 400)(i32.const 64)))
                   (call $fw (i32.const 80)(i32.const 4)(i32.const 400)(i32.const 2))
                   (drop (call $del (i32.const 0)(i32.const 5)(i32.const 200)(local.get $lt)))
                   (i32.store (i32.const 512) (call $len (i32.const 0)(i32.const 5)))
                   (call $fw (i32.const 96)(i32.const 6)(i32.const 512)(i32.const 1))))"#,
        )
        .expect("assemble typed-key program")
    }

    fn all_kv_files() -> GrantSet {
        GrantSet::new(vec![
            Grant {
                facet: Capability::Kv,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
            Grant {
                facet: Capability::Files,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
        ])
    }

    #[cfg(feature = "sql-state-access")]
    fn sql_files_grants() -> GrantSet {
        GrantSet::new(vec![
            Grant {
                facet: Capability::Sql,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
            Grant {
                facet: Capability::Files,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
        ])
    }

    #[cfg(feature = "sql-state-access")]
    fn sql_roundtrip_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "sql_exec" (func $exec (param i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "sql_query" (func $query (param i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "app")
                 (data (i32.const 16) "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'a')")
                 (data (i32.const 128) "SELECT id, v FROM t ORDER BY id")
                 (data (i32.const 176) "/sql")
                 (func (export "run") (local $n i32)
                   (drop (call $exec (i32.const 0)(i32.const 3)(i32.const 16)(i32.const 78)(i32.const 400)(i32.const 256)))
                   (local.set $n (call $query (i32.const 0)(i32.const 3)(i32.const 128)(i32.const 31)(i32.const 800)(i32.const 512)))
                   (call $fw (i32.const 176)(i32.const 4)(i32.const 800)(local.get $n))))"#,
        )
        .expect("assemble SQL round-trip program")
    }

    #[cfg(feature = "sql-state-access")]
    fn sql_query_mutation_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "sql_query" (func $query (param i32 i32 i32 i32 i32 i32) (result i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "app")
                 (data (i32.const 16) "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
                 (func (export "run")
                   (drop (call $query (i32.const 0)(i32.const 3)(i32.const 16)(i32.const 47)(i32.const 400)(i32.const 256)))))"#,
        )
        .expect("assemble SQL read-only denial program")
    }

    #[cfg(feature = "sql-state-access")]
    #[test]
    fn sql_exec_and_query_round_trip_through_host_abi() {
        let (mut loom, ns) = state_loom(17);
        loom.registry_mut().add_facet(ns, FacetKind::Sql).unwrap();
        let state = StateAccess::new(&mut loom, context(ns, sql_files_grants()));
        let (state, outcome) =
            run_state(&sql_roundtrip_program(), state, 2_000_000, BTreeMap::new()).unwrap();
        assert!(outcome.fuel_used > 0);
        drop(state);

        let result = loom.read_file(ns, "/sql").unwrap();
        let json = loom_result::result_to_json(&result).unwrap();
        assert!(json.contains("\"id\""));
        assert!(json.contains("\"v\""));
        assert!(json.contains("\"a\""));
    }

    #[cfg(feature = "sql-state-access")]
    #[test]
    fn sql_query_host_abi_rejects_mutating_statement() {
        let (mut loom, ns) = state_loom(18);
        loom.registry_mut().add_facet(ns, FacetKind::Sql).unwrap();
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Sql,
            mode: Mode::Read,
            scopes: vec![Scope::All],
        }]);
        let state = StateAccess::new(&mut loom, context(ns, grants));
        let err = match run_state(
            &sql_query_mutation_program(),
            state,
            1_000_000,
            BTreeMap::new(),
        ) {
            Ok(_) => panic!("sql_query must reject mutating statements"),
            Err(err) => err,
        };
        assert_eq!(err.code(), loom_core::Code::PermissionDenied);
    }

    fn queue_files_grants() -> GrantSet {
        GrantSet::new(vec![
            Grant {
                facet: Capability::Queue,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
            Grant {
                facet: Capability::Files,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
        ])
    }

    fn search_files_grants() -> GrantSet {
        GrantSet::new(vec![
            Grant {
                facet: Capability::Search,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
            Grant {
                facet: Capability::Files,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
        ])
    }

    fn search_roundtrip_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "input_get" (func $in (param i32 i32 i32 i32) (result i32)))
                 (import "env" "search_create" (func $create (param i32 i32 i32 i32)))
                 (import "env" "search_index" (func $index (param i32 i32 i32 i32 i32 i32)))
                 (import "env" "search_query" (func $query (param i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "search_get" (func $get (param i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "docs")
                 (data (i32.const 16) "doc-1")
                 (data (i32.const 32) "/hits")
                 (data (i32.const 48) "/doc")
                 (data (i32.const 64) "mapping")
                 (data (i32.const 80) "doc")
                 (data (i32.const 96) "request")
                 (func (export "run") (local $ml i32) (local $dl i32) (local $rl i32) (local $n i32)
                   (local.set $ml (call $in (i32.const 64)(i32.const 7)(i32.const 400)(i32.const 256)))
                   (local.set $dl (call $in (i32.const 80)(i32.const 3)(i32.const 700)(i32.const 256)))
                   (local.set $rl (call $in (i32.const 96)(i32.const 7)(i32.const 1000)(i32.const 256)))
                   (call $create (i32.const 0)(i32.const 4)(i32.const 400)(local.get $ml))
                   (call $index (i32.const 0)(i32.const 4)(i32.const 16)(i32.const 5)(i32.const 700)(local.get $dl))
                   (local.set $n (call $query (i32.const 0)(i32.const 4)(i32.const 1000)(local.get $rl)(i32.const 1300)(i32.const 256)))
                   (call $fw (i32.const 32)(i32.const 5)(i32.const 1300)(local.get $n))
                   (local.set $n (call $get (i32.const 0)(i32.const 4)(i32.const 16)(i32.const 5)(i32.const 1600)(i32.const 256)))
                   (call $fw (i32.const 48)(i32.const 4)(i32.const 1600)(local.get $n))))"#,
        )
        .expect("assemble search round-trip program")
    }

    fn search_mapping_input() -> Vec<u8> {
        loom_codec::encode(&loom_codec::Value::Map(vec![(
            loom_codec::Value::Text("title".to_string()),
            loom_codec::Value::Array(vec![
                loom_codec::Value::Uint(0),
                loom_codec::Value::Bool(true),
                loom_codec::Value::Bool(false),
            ]),
        )]))
        .unwrap()
    }

    fn search_document_input() -> Vec<u8> {
        loom_codec::encode(&loom_codec::Value::Map(vec![(
            loom_codec::Value::Text("title".to_string()),
            loom_codec::Value::Text("hello loom".to_string()),
        )]))
        .unwrap()
    }

    fn search_request_input() -> Vec<u8> {
        loom_codec::encode(&loom_codec::Value::Array(vec![
            loom_codec::Value::Array(vec![
                loom_codec::Value::Uint(0),
                loom_codec::Value::Text("title".to_string()),
                loom_codec::Value::Text("hello".to_string()),
            ]),
            loom_codec::Value::Uint(10),
            loom_codec::Value::Uint(0),
        ]))
        .unwrap()
    }

    #[test]
    fn search_create_index_query_and_get_through_host_abi() {
        use loom_codec::Value as Cbor;
        let (mut loom, ns) = state_loom(19);
        let inputs = BTreeMap::from([
            ("mapping".to_string(), search_mapping_input()),
            ("doc".to_string(), search_document_input()),
            ("request".to_string(), search_request_input()),
        ]);
        let state = StateAccess::new(&mut loom, context(ns, search_files_grants()));
        let (state, outcome) =
            run_state(&search_roundtrip_program(), state, 2_000_000, inputs).unwrap();
        assert!(outcome.fuel_used > 0);
        drop(state);

        let hits = loom.read_file(ns, "/hits").unwrap();
        assert_eq!(
            loom_codec::decode(&hits).unwrap(),
            Cbor::Array(vec![
                Cbor::Bool(true),
                Cbor::Array(vec![Cbor::Array(vec![
                    Cbor::Bytes(b"doc-1".to_vec()),
                    loom_core::tabular::cell_value(&Value::F32(1.0)),
                    Cbor::Map(vec![]),
                ])]),
                Cbor::Map(vec![]),
                Cbor::Map(vec![]),
            ])
        );
        let doc =
            loom_core::search_document_from_cbor(&loom.read_file(ns, "/doc").unwrap()).unwrap();
        assert_eq!(
            doc.get("title"),
            Some(&loom_core::FieldValue::Text("hello loom".to_string()))
        );
    }

    fn dataframe_files_grants() -> GrantSet {
        GrantSet::new(vec![
            Grant {
                facet: Capability::Dataframe,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
            Grant {
                facet: Capability::Files,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
        ])
    }

    fn dataframe_roundtrip_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "input_get" (func $in (param i32 i32 i32 i32) (result i32)))
                 (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
                 (import "env" "dataframe_create" (func $create (param i32 i32 i32 i32)))
                 (import "env" "dataframe_get_plan" (func $get (param i32 i32 i32 i32) (result i32)))
                 (import "env" "dataframe_preview" (func $preview (param i32 i32 i64 i32 i32) (result i32)))
                 (import "env" "dataframe_plan_digest" (func $digest (param i32 i32 i32 i32) (result i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "df")
                 (data (i32.const 16) "/in.csv")
                 (data (i32.const 32) "id,name\0a1,a\0a2,b\0a")
                 (data (i32.const 64) "plan")
                 (data (i32.const 80) "/plan")
                 (data (i32.const 96) "/preview")
                 (data (i32.const 112) "/digest")
                 (func (export "run") (local $pl i32) (local $n i32)
                   (local.set $pl (call $in (i32.const 64)(i32.const 4)(i32.const 400)(i32.const 512)))
                   (call $fw (i32.const 16)(i32.const 7)(i32.const 32)(i32.const 16))
                   (call $create (i32.const 0)(i32.const 2)(i32.const 400)(local.get $pl))
                   (local.set $n (call $get (i32.const 0)(i32.const 2)(i32.const 1000)(i32.const 512)))
                   (call $fw (i32.const 80)(i32.const 5)(i32.const 1000)(local.get $n))
                   (local.set $n (call $preview (i32.const 0)(i32.const 2)(i64.const 1)(i32.const 1600)(i32.const 512)))
                   (call $fw (i32.const 96)(i32.const 8)(i32.const 1600)(local.get $n))
                   (local.set $n (call $digest (i32.const 0)(i32.const 2)(i32.const 2200)(i32.const 128)))
                   (call $fw (i32.const 112)(i32.const 7)(i32.const 2200)(local.get $n))))"#,
        )
        .expect("assemble dataframe round-trip program")
    }

    fn dataframe_plan_input() -> loom_core::DataframePlan {
        loom_core::DataframePlan::new(vec![
            loom_core::DataframeSourceBinding::new(
                "events",
                loom_core::DataframeSourceKind::Files,
                "/in.csv",
                loom_core::DataframeInputFormat::Csv,
            )
            .with_option("has_header", "true"),
        ])
        .unwrap()
        .with_operations(vec![loom_core::DataframeOperation::Scan {
            source: "events".to_string(),
        }])
        .unwrap()
    }

    #[test]
    fn dataframe_plan_preview_and_digest_through_host_abi() {
        let (mut loom, ns) = state_loom(20);
        let plan = dataframe_plan_input();
        let inputs = BTreeMap::from([("plan".to_string(), plan.encode())]);
        let state = StateAccess::new(&mut loom, context(ns, dataframe_files_grants()));
        let (state, outcome) =
            run_state(&dataframe_roundtrip_program(), state, 2_000_000, inputs).unwrap();
        assert!(outcome.fuel_used > 0);
        drop(state);

        assert_eq!(loom.read_file(ns, "/plan").unwrap(), plan.encode());
        let preview =
            loom_core::DataframeBatch::decode(&loom.read_file(ns, "/preview").unwrap()).unwrap();
        assert_eq!(preview.row_count(), 1);
        assert_eq!(preview.rows[0][0], Value::Int(1));
        let digest = String::from_utf8(loom.read_file(ns, "/digest").unwrap()).unwrap();
        assert_eq!(
            digest,
            loom_core::dataframe_plan_digest(&loom, ns, "df")
                .unwrap()
                .to_string()
        );
    }

    // Appends two entries to stream `q`, then reads the whole stream back through `queue_range` and
    // writes the canonical-CBOR array the host returns into /out. The range bound is deliberately wide
    // so the test does not assume a 0- or 1-based first sequence number.
    fn queue_range_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "queue_append" (func $qa (param i32 i32 i32 i32) (result i64)))
                 (import "env" "queue_range" (func $qr (param i32 i32 i64 i64 i32 i32) (result i32)))
                 (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "q")
                 (data (i32.const 16) "aa")
                 (data (i32.const 32) "bb")
                 (data (i32.const 48) "/out")
                 (func (export "run") (local $n i32)
                   (drop (call $qa (i32.const 0)(i32.const 1)(i32.const 16)(i32.const 2)))
                   (drop (call $qa (i32.const 0)(i32.const 1)(i32.const 32)(i32.const 2)))
                   (local.set $n (call $qr (i32.const 0)(i32.const 1)(i64.const 0)(i64.const 1000)(i32.const 200)(i32.const 256)))
                   (call $fw (i32.const 48)(i32.const 4)(i32.const 200)(local.get $n))))"#,
        )
        .expect("assemble queue_range program")
    }

    #[test]
    fn queue_range_returns_entries_in_order_through_host_abi() {
        let (mut loom, ns) = state_loom(9);
        let state = StateAccess::new(&mut loom, context(ns, queue_files_grants()));
        let (state, outcome) =
            run_state(&queue_range_program(), state, 1_000_000, BTreeMap::new()).unwrap();
        assert!(outcome.fuel_used > 0);
        drop(state);
        // The wire form is a canonical CBOR array of the appended byte entries, in sequence order.
        let out = loom.read_file(ns, "/out").unwrap();
        assert_eq!(
            loom_codec::decode(&out).expect("queue_range wire form decodes"),
            loom_codec::Value::Array(vec![
                loom_codec::Value::Bytes(b"aa".to_vec()),
                loom_codec::Value::Bytes(b"bb".to_vec()),
            ])
        );
    }

    fn graph_read_files_grants() -> GrantSet {
        GrantSet::new(vec![
            Grant {
                facet: Capability::Graph,
                mode: Mode::Read,
                scopes: vec![Scope::All],
            },
            Grant {
                facet: Capability::Files,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
        ])
    }

    // Reads node `n1` and its out-edges from graph `g` through the host ABI, writing the canonical-CBOR
    // property array to /node and the canonical-CBOR edge list to /edges.
    fn graph_read_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "graph_get_node" (func $gn (param i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "graph_out_edges" (func $ge (param i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "g")
                 (data (i32.const 16) "n1")
                 (data (i32.const 32) "/node")
                 (data (i32.const 48) "/edges")
                 (func (export "run") (local $n i32)
                   (local.set $n (call $gn (i32.const 0)(i32.const 1)(i32.const 16)(i32.const 2)(i32.const 200)(i32.const 256)))
                   (call $fw (i32.const 32)(i32.const 5)(i32.const 200)(local.get $n))
                   (local.set $n (call $ge (i32.const 0)(i32.const 1)(i32.const 16)(i32.const 2)(i32.const 400)(i32.const 256)))
                   (call $fw (i32.const 48)(i32.const 6)(i32.const 400)(local.get $n))))"#,
        )
        .expect("assemble graph-read program")
    }

    #[test]
    fn graph_get_node_and_out_edges_through_host_abi() {
        use loom_codec::Value as Cbor;
        let (mut loom, ns) = state_loom(11);
        // Seed the graph directly (test Loom binds no identity, so the ACL gate is a no-op); the guest
        // then reads it back through the host ABI under a Graph-Read manifest grant.
        let mut props = BTreeMap::new();
        props.insert("k".to_string(), loom_core::GraphValue::Bytes(vec![9u8]));
        loom_core::graph_upsert_node(&mut loom, ns, "g", "n1", props).unwrap();
        loom_core::graph_upsert_node(&mut loom, ns, "g", "n2", BTreeMap::new()).unwrap();
        loom_core::graph_upsert_edge(&mut loom, ns, "g", "e1", "n1", "n2", "rel", BTreeMap::new())
            .unwrap();

        let state = StateAccess::new(&mut loom, context(ns, graph_read_files_grants()));
        let (state, outcome) =
            run_state(&graph_read_program(), state, 1_000_000, BTreeMap::new()).unwrap();
        assert!(outcome.fuel_used > 0);
        drop(state);

        // Node props: a canonical `[key, value]` pair array.
        assert_eq!(
            loom_codec::decode(&loom.read_file(ns, "/node").unwrap()).expect("node decodes"),
            Cbor::Array(vec![Cbor::Array(vec![
                Cbor::Text("k".to_string()),
                Cbor::Bytes(vec![9]),
            ])])
        );
        // Out-edges: a list of `[edge_id, [src, dst, label, props]]`; e1's props are empty.
        assert_eq!(
            loom_codec::decode(&loom.read_file(ns, "/edges").unwrap()).expect("edges decode"),
            Cbor::Array(vec![Cbor::Array(vec![
                Cbor::Text("e1".to_string()),
                Cbor::Array(vec![
                    Cbor::Text("n1".to_string()),
                    Cbor::Text("n2".to_string()),
                    Cbor::Text("rel".to_string()),
                    Cbor::Array(vec![]),
                ]),
            ])])
        );
    }

    fn graph_rw_grants() -> GrantSet {
        GrantSet::new(vec![Grant {
            facet: Capability::Graph,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::All],
        }])
    }

    fn pim_files_grants() -> GrantSet {
        GrantSet::new(vec![
            Grant {
                facet: Capability::Calendar,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
            Grant {
                facet: Capability::Contacts,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
            Grant {
                facet: Capability::Mail,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
            Grant {
                facet: Capability::Files,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
        ])
    }

    fn pim_roundtrip_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "input_get" (func $in (param i32 i32 i32 i32) (result i32)))
                 (import "env" "calendar_create_collection" (func $cal_create (param i32 i32 i32 i32 i32 i32)))
                 (import "env" "calendar_put_entry" (func $cal_put (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "calendar_get_entry" (func $cal_get (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "contacts_create_book" (func $con_create (param i32 i32 i32 i32 i32 i32)))
                 (import "env" "contacts_put_entry" (func $con_put (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "contacts_get_entry" (func $con_get (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "mail_create_mailbox" (func $mail_create (param i32 i32 i32 i32 i32 i32)))
                 (import "env" "mail_ingest_message" (func $mail_ingest (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "mail_set_flags" (func $mail_set_flags (param i32 i32 i32 i32 i32 i32 i32 i32)))
                 (import "env" "mail_get_flags" (func $mail_get_flags (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "mail_to_eml" (func $mail_eml (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "alice")
                 (data (i32.const 16) "work")
                 (data (i32.const 32) "u1")
                 (data (i32.const 48) "people")
                 (data (i32.const 64) "c1")
                 (data (i32.const 80) "inbox")
                 (data (i32.const 96) "m1")
                 (data (i32.const 112) "cal_meta")
                 (data (i32.const 128) "cal_entry")
                 (data (i32.const 144) "book_meta")
                 (data (i32.const 160) "contact_entry")
                 (data (i32.const 176) "mail_meta")
                 (data (i32.const 192) "raw_mail")
                 (data (i32.const 208) "mail_flags")
                 (data (i32.const 224) "/cal")
                 (data (i32.const 240) "/contact")
                 (data (i32.const 256) "/flags")
                 (data (i32.const 272) "/eml")
                 (func (export "run") (local $n i32)
                   (local.set $n (call $in (i32.const 112)(i32.const 8)(i32.const 1000)(i32.const 256)))
                   (call $cal_create (i32.const 0)(i32.const 5)(i32.const 16)(i32.const 4)(i32.const 1000)(local.get $n))
                   (local.set $n (call $in (i32.const 128)(i32.const 9)(i32.const 1400)(i32.const 512)))
                   (drop (call $cal_put (i32.const 0)(i32.const 5)(i32.const 16)(i32.const 4)(i32.const 1400)(local.get $n)(i32.const 1900)(i32.const 32)))
                   (local.set $n (call $cal_get (i32.const 0)(i32.const 5)(i32.const 16)(i32.const 4)(i32.const 32)(i32.const 2)(i32.const 2000)(i32.const 512)))
                   (call $fw (i32.const 224)(i32.const 4)(i32.const 2000)(local.get $n))

                   (local.set $n (call $in (i32.const 144)(i32.const 9)(i32.const 2600)(i32.const 256)))
                   (call $con_create (i32.const 0)(i32.const 5)(i32.const 48)(i32.const 6)(i32.const 2600)(local.get $n))
                   (local.set $n (call $in (i32.const 160)(i32.const 13)(i32.const 3000)(i32.const 512)))
                   (drop (call $con_put (i32.const 0)(i32.const 5)(i32.const 48)(i32.const 6)(i32.const 3000)(local.get $n)(i32.const 3500)(i32.const 32)))
                   (local.set $n (call $con_get (i32.const 0)(i32.const 5)(i32.const 48)(i32.const 6)(i32.const 64)(i32.const 2)(i32.const 3600)(i32.const 512)))
                   (call $fw (i32.const 240)(i32.const 8)(i32.const 3600)(local.get $n))

                   (local.set $n (call $in (i32.const 176)(i32.const 9)(i32.const 4200)(i32.const 256)))
                   (call $mail_create (i32.const 0)(i32.const 5)(i32.const 80)(i32.const 5)(i32.const 4200)(local.get $n))
                   (local.set $n (call $in (i32.const 192)(i32.const 8)(i32.const 4600)(i32.const 512)))
                   (drop (call $mail_ingest (i32.const 0)(i32.const 5)(i32.const 80)(i32.const 5)(i32.const 96)(i32.const 2)(i32.const 4600)(local.get $n)(i32.const 5200)(i32.const 32)))
                   (local.set $n (call $in (i32.const 208)(i32.const 10)(i32.const 5400)(i32.const 256)))
                   (call $mail_set_flags (i32.const 0)(i32.const 5)(i32.const 80)(i32.const 5)(i32.const 96)(i32.const 2)(i32.const 5400)(local.get $n))
                   (local.set $n (call $mail_get_flags (i32.const 0)(i32.const 5)(i32.const 80)(i32.const 5)(i32.const 96)(i32.const 2)(i32.const 5700)(i32.const 256)))
                   (call $fw (i32.const 256)(i32.const 6)(i32.const 5700)(local.get $n))
                   (local.set $n (call $mail_eml (i32.const 0)(i32.const 5)(i32.const 80)(i32.const 5)(i32.const 96)(i32.const 2)(i32.const 6000)(i32.const 512)))
                   (call $fw (i32.const 272)(i32.const 4)(i32.const 6000)(local.get $n))))"#,
        )
        .expect("assemble PIM round-trip program")
    }

    fn pim_calendar_create_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "input_get" (func $in (param i32 i32 i32 i32) (result i32)))
                 (import "env" "calendar_create_collection" (func $cal_create (param i32 i32 i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "alice")
                 (data (i32.const 16) "work")
                 (data (i32.const 32) "cal_meta")
                 (func (export "run") (local $n i32)
                   (local.set $n (call $in (i32.const 32)(i32.const 8)(i32.const 1000)(i32.const 256)))
                   (call $cal_create (i32.const 0)(i32.const 5)(i32.const 16)(i32.const 4)(i32.const 1000)(local.get $n))))"#,
        )
        .expect("assemble PIM calendar-create program")
    }

    #[test]
    fn pim_domain_records_round_trip_through_host_abi() {
        let (mut loom, ns) = state_loom(16);
        let raw_mail = b"From: bob@example.test\r\nTo: alice@example.test\r\nSubject: Hello\r\nMessage-ID: <m1@example.test>\r\n\r\nbody".to_vec();
        let inputs = BTreeMap::from([
            (
                "cal_meta".to_string(),
                CollectionMeta {
                    display_name: "Work".to_string(),
                    component_set: vec![Component::Event],
                }
                .encode(),
            ),
            (
                "cal_entry".to_string(),
                CalendarEntry::event("u1", "Standup", "20240101T090000").encode(),
            ),
            (
                "book_meta".to_string(),
                BookMeta {
                    display_name: "People".to_string(),
                }
                .encode(),
            ),
            (
                "contact_entry".to_string(),
                ContactEntry::new("c1", "Ada Lovelace").encode(),
            ),
            (
                "mail_meta".to_string(),
                MailboxMeta {
                    display_name: "Inbox".to_string(),
                }
                .encode(),
            ),
            ("raw_mail".to_string(), raw_mail.clone()),
            (
                "mail_flags".to_string(),
                encode_string_list(&["\\Seen".to_string(), "Important".to_string()]),
            ),
        ]);
        let state = StateAccess::new(&mut loom, context(ns, pim_files_grants()));
        let (state, outcome) =
            run_state(&pim_roundtrip_program(), state, 2_000_000, inputs).unwrap();
        assert!(outcome.fuel_used > 0);
        drop(state);

        assert_eq!(
            CalendarEntry::decode(&loom.read_file(ns, "/cal").unwrap())
                .unwrap()
                .summary,
            "Standup"
        );
        assert_eq!(
            ContactEntry::decode(&loom.read_file(ns, "/contact").unwrap())
                .unwrap()
                .full_name,
            "Ada Lovelace"
        );
        assert_eq!(loom.read_file(ns, "/eml").unwrap(), raw_mail);
        assert_eq!(
            loom_codec::decode(&loom.read_file(ns, "/flags").unwrap()).unwrap(),
            loom_codec::Value::Array(vec![
                loom_codec::Value::Text("Important".to_string()),
                loom_codec::Value::Text("\\Seen".to_string()),
            ])
        );
        assert!(
            loom_core::calendar::get_entry(&loom, ns, "alice", "work", "u1")
                .unwrap()
                .is_some()
        );
        assert!(
            loom_core::contacts::get_entry(&loom, ns, "alice", "people", "c1")
                .unwrap()
                .is_some()
        );
        assert!(
            loom_core::mail::get_message(&loom, ns, "alice", "inbox", "m1")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn pim_host_abi_rejects_malformed_records_and_denied_grants() {
        let (mut loom, ns) = state_loom(17);
        let inputs = BTreeMap::from([("cal_meta".to_string(), vec![0xff])]);
        let state = StateAccess::new(&mut loom, context(ns, pim_files_grants()));
        let malformed = match run_state(&pim_calendar_create_program(), state, 1_000_000, inputs) {
            Ok(_) => panic!("malformed PIM input must fail the run"),
            Err(err) => err,
        };
        assert!(matches!(malformed, ExecError::Program(_)));
        assert_eq!(malformed.code(), loom_core::Code::InvalidArgument);
        assert!(
            loom_core::calendar::list_collections(&loom, ns, "alice")
                .unwrap()
                .is_empty()
        );

        let (mut loom, ns) = state_loom(18);
        let inputs = BTreeMap::from([(
            "cal_meta".to_string(),
            CollectionMeta {
                display_name: "Work".to_string(),
                component_set: vec![Component::Event],
            }
            .encode(),
        )]);
        let state = StateAccess::new(&mut loom, context(ns, GrantSet::new(vec![])));
        let denied = match run_state(&pim_calendar_create_program(), state, 1_000_000, inputs) {
            Ok(_) => panic!("denied PIM host call must fail the run"),
            Err(err) => err,
        };
        assert_eq!(denied.code(), loom_core::Code::PermissionDenied);
        assert!(
            loom_core::calendar::list_collections(&loom, ns, "alice")
                .unwrap()
                .is_empty()
        );
    }

    // Reads a property pair array from input `np` and upserts it as node `n1` in graph `g`.
    fn graph_upsert_node_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "input_get" (func $in (param i32 i32 i32 i32) (result i32)))
                 (import "env" "graph_upsert_node" (func $un (param i32 i32 i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "g")
                 (data (i32.const 16) "n1")
                 (data (i32.const 32) "np")
                 (func (export "run") (local $l i32)
                   (local.set $l (call $in (i32.const 32)(i32.const 2)(i32.const 200)(i32.const 256)))
                   (call $un (i32.const 0)(i32.const 1)(i32.const 16)(i32.const 2)(i32.const 200)(local.get $l))))"#,
        )
        .expect("assemble graph_upsert_node program")
    }

    #[test]
    fn graph_upsert_node_through_host_abi_persists_props() {
        let (mut loom, ns) = state_loom(12);
        let mut props = BTreeMap::new();
        props.insert("k".to_string(), loom_core::GraphValue::Bytes(vec![9u8]));
        // The property map travels as the canonical pair array `encode_props` produces.
        let inputs = BTreeMap::from([("np".to_string(), encode_props(&props))]);
        let state = StateAccess::new(&mut loom, context(ns, graph_rw_grants()));
        let (state, outcome) =
            run_state(&graph_upsert_node_program(), state, 1_000_000, inputs).unwrap();
        assert!(outcome.fuel_used > 0);
        drop(state);
        // The node the guest wrote through the ABI is readable directly with the same props.
        assert_eq!(
            loom_core::graph_get_node(&loom, ns, "g", "n1").unwrap(),
            Some(props)
        );
    }

    #[test]
    fn graph_upsert_node_with_malformed_props_traps() {
        let (mut loom, ns) = state_loom(13);
        // `0xFF` is not canonical CBOR, so `decode_props` rejects it and the call traps; nothing is
        // written.
        let inputs = BTreeMap::from([("np".to_string(), vec![0xFFu8])]);
        let state = StateAccess::new(&mut loom, context(ns, graph_rw_grants()));
        let result = run_state(&graph_upsert_node_program(), state, 1_000_000, inputs);
        assert!(matches!(result, Err(ExecError::Program(_))));
        assert_eq!(
            loom_core::graph_get_node(&loom, ns, "g", "n1").unwrap(),
            None
        );
    }

    // Runs `graph_reachable(g, n1, unbounded, any-label)` into /reach and
    // `graph_shortest_path(g, n1, n3, any-label)` into /path. The optional args use the negative
    // sentinel: `max_depth = -1` and via-label length `-1` both mean None.
    fn graph_query_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "graph_reachable" (func $r (param i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "graph_shortest_path" (func $s (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "g")
                 (data (i32.const 16) "n1")
                 (data (i32.const 32) "n3")
                 (data (i32.const 48) "/reach")
                 (data (i32.const 64) "/path")
                 (func (export "run") (local $n i32)
                   (local.set $n (call $r (i32.const 0)(i32.const 1)(i32.const 16)(i32.const 2)(i32.const -1)(i32.const 0)(i32.const -1)(i32.const 200)(i32.const 256)))
                   (call $fw (i32.const 48)(i32.const 6)(i32.const 200)(local.get $n))
                   (local.set $n (call $s (i32.const 0)(i32.const 1)(i32.const 16)(i32.const 2)(i32.const 32)(i32.const 2)(i32.const 0)(i32.const -1)(i32.const 400)(i32.const 256)))
                   (call $fw (i32.const 64)(i32.const 5)(i32.const 400)(local.get $n))))"#,
        )
        .expect("assemble graph-query program")
    }

    #[test]
    fn graph_reachable_and_shortest_path_through_host_abi() {
        use loom_codec::Value as Cbor;
        let (mut loom, ns) = state_loom(14);
        // A directed line n1 -> n2 -> n3 (endpoints must exist as nodes for the queries).
        for id in ["n1", "n2", "n3"] {
            loom_core::graph_upsert_node(&mut loom, ns, "g", id, BTreeMap::new()).unwrap();
        }
        loom_core::graph_upsert_edge(&mut loom, ns, "g", "e1", "n1", "n2", "", BTreeMap::new())
            .unwrap();
        loom_core::graph_upsert_edge(&mut loom, ns, "g", "e2", "n2", "n3", "", BTreeMap::new())
            .unwrap();

        let state = StateAccess::new(&mut loom, context(ns, graph_read_files_grants()));
        let (state, outcome) =
            run_state(&graph_query_program(), state, 1_000_000, BTreeMap::new()).unwrap();
        assert!(outcome.fuel_used > 0);
        drop(state);

        // Reachable-from-n1 is the visited set minus the start, in ascending id order.
        assert_eq!(
            loom_codec::decode(&loom.read_file(ns, "/reach").unwrap()).expect("reach decodes"),
            Cbor::Array(vec![
                Cbor::Text("n2".to_string()),
                Cbor::Text("n3".to_string())
            ])
        );
        // The shortest path is endpoint-inclusive.
        assert_eq!(
            loom_codec::decode(&loom.read_file(ns, "/path").unwrap()).expect("path decodes"),
            Cbor::Array(vec![
                Cbor::Text("n1".to_string()),
                Cbor::Text("n2".to_string()),
                Cbor::Text("n3".to_string()),
            ])
        );
    }

    // Upserts a vector (components from input `nv`, metadata from input `nm`) into set `vs`, then reads
    // it back through `vector_get` and writes the `[vector_bytes, metadata]` result to /out.
    fn vector_roundtrip_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "input_get" (func $in (param i32 i32 i32 i32) (result i32)))
                 (import "env" "vector_upsert" (func $up (param i32 i32 i32 i32 i32 i32 i32 i32)))
                 (import "env" "vector_get" (func $get (param i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "vs")
                 (data (i32.const 16) "id1")
                 (data (i32.const 32) "nv")
                 (data (i32.const 48) "nm")
                 (data (i32.const 64) "/out")
                 (func (export "run") (local $vl i32) (local $ml i32) (local $n i32)
                   (local.set $vl (call $in (i32.const 32)(i32.const 2)(i32.const 200)(i32.const 256)))
                   (local.set $ml (call $in (i32.const 48)(i32.const 2)(i32.const 600)(i32.const 256)))
                   (call $up (i32.const 0)(i32.const 2)(i32.const 16)(i32.const 3)(i32.const 200)(local.get $vl)(i32.const 600)(local.get $ml))
                   (local.set $n (call $get (i32.const 0)(i32.const 2)(i32.const 16)(i32.const 3)(i32.const 1000)(i32.const 256)))
                   (call $fw (i32.const 64)(i32.const 4)(i32.const 1000)(local.get $n))))"#,
        )
        .expect("assemble vector round-trip program")
    }

    #[test]
    fn vector_upsert_and_get_through_host_abi() {
        use super::super::{encode_f32_vec, encode_meta};
        use loom_codec::Value as Cbor;
        use loom_core::tabular::Value;
        let (mut loom, ns) = state_loom(15);
        // Seed the set directly (test Loom binds no identity, so the ACL gate is a no-op).
        loom_core::vector_create(&mut loom, ns, "vs", 2, loom_core::vector::Metric::Cosine)
            .unwrap();

        let vector = vec![0.5f32, 0.25];
        let mut meta = BTreeMap::new();
        meta.insert("k".to_string(), Value::Int(3));
        let inputs = BTreeMap::from([
            ("nv".to_string(), encode_f32_vec(&vector)),
            ("nm".to_string(), encode_meta(&meta)),
        ]);
        let grants = GrantSet::new(vec![
            Grant {
                facet: Capability::Vector,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
            Grant {
                facet: Capability::Files,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
        ]);
        let state = StateAccess::new(&mut loom, context(ns, grants));
        let (state, outcome) =
            run_state(&vector_roundtrip_program(), state, 2_000_000, inputs).unwrap();
        assert!(outcome.fuel_used > 0);
        drop(state);

        // The guest's upsert persisted; a direct read returns the same vector + metadata.
        assert_eq!(
            loom_core::vector_get(&loom, ns, "vs", "id1").unwrap(),
            Some((vector.clone(), meta.clone()))
        );
        // The ABI `vector_get` returned the canonical `[vector_bytes, metadata]` shape.
        let out = loom.read_file(ns, "/out").unwrap();
        let Cbor::Array(parts) = loom_codec::decode(&out).expect("vector entry decodes") else {
            panic!("vector entry is a CBOR array");
        };
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], Cbor::Bytes(encode_f32_vec(&vector)));
        assert_eq!(parts[1], loom_codec::decode(&encode_meta(&meta)).unwrap());
    }

    fn vector_filtered_search_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "input_get" (func $in (param i32 i32 i32 i32) (result i32)))
                 (import "env" "vector_search_filtered" (func $search (param i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "vs")
                 (data (i32.const 16) "nv")
                 (data (i32.const 32) "nf")
                 (data (i32.const 48) "/hits")
                 (func (export "run") (local $vl i32) (local $fl i32) (local $n i32)
                   (local.set $vl (call $in (i32.const 16)(i32.const 2)(i32.const 200)(i32.const 64)))
                   (local.set $fl (call $in (i32.const 32)(i32.const 2)(i32.const 400)(i32.const 128)))
                   (local.set $n (call $search (i32.const 0)(i32.const 2)(i32.const 200)(local.get $vl)(i32.const 10)(i32.const 400)(local.get $fl)(i32.const 800)(i32.const 256)))
                   (call $fw (i32.const 48)(i32.const 5)(i32.const 800)(local.get $n))))"#,
        )
        .expect("assemble vector filtered search program")
    }

    fn vector_filter_eq_lang(lang: &str) -> Vec<u8> {
        loom_codec::encode(&loom_codec::Value::Array(vec![
            loom_codec::Value::Uint(1),
            loom_codec::Value::Text("lang".to_string()),
            loom_core::tabular::cell_value(&Value::Text(lang.to_string())),
        ]))
        .unwrap()
    }

    #[test]
    fn vector_filtered_search_through_host_abi() {
        use super::super::encode_f32_vec;
        use loom_codec::Value as Cbor;
        let (mut loom, ns) = state_loom(21);
        loom_core::vector_create(&mut loom, ns, "vs", 2, loom_core::vector::Metric::Cosine)
            .unwrap();
        loom_core::vector_upsert(
            &mut loom,
            ns,
            "vs",
            "en",
            vec![1.0, 0.0],
            BTreeMap::from([("lang".to_string(), Value::Text("en".to_string()))]),
        )
        .unwrap();
        loom_core::vector_upsert(
            &mut loom,
            ns,
            "vs",
            "fr",
            vec![1.0, 0.0],
            BTreeMap::from([("lang".to_string(), Value::Text("fr".to_string()))]),
        )
        .unwrap();
        let inputs = BTreeMap::from([
            ("nv".to_string(), encode_f32_vec(&[1.0, 0.0])),
            ("nf".to_string(), vector_filter_eq_lang("en")),
        ]);
        let grants = GrantSet::new(vec![
            Grant {
                facet: Capability::Vector,
                mode: Mode::Read,
                scopes: vec![Scope::All],
            },
            Grant {
                facet: Capability::Files,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
        ]);
        let state = StateAccess::new(&mut loom, context(ns, grants));
        let (state, outcome) =
            run_state(&vector_filtered_search_program(), state, 2_000_000, inputs).unwrap();
        assert!(outcome.fuel_used > 0);
        drop(state);

        let Cbor::Array(hits) =
            loom_codec::decode(&loom.read_file(ns, "/hits").unwrap()).expect("hits decode")
        else {
            panic!("vector hits are a CBOR array");
        };
        assert_eq!(hits.len(), 1);
        let Cbor::Array(hit) = &hits[0] else {
            panic!("one hit is a CBOR array");
        };
        assert_eq!(hit[0], Cbor::Text("en".to_string()));
    }

    // Creates dataset `ds` (schema from input `nc`), appends a row (from input `nr`), then scans it
    // back through `columnar_scan` and writes the row array to /out.
    fn columnar_roundtrip_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "input_get" (func $in (param i32 i32 i32 i32) (result i32)))
                 (import "env" "columnar_create" (func $cc (param i32 i32 i32 i32 i32)))
                 (import "env" "columnar_append" (func $ca (param i32 i32 i32 i32)))
                 (import "env" "columnar_scan" (func $cs (param i32 i32 i32 i32) (result i32)))
                 (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "ds")
                 (data (i32.const 16) "nc")
                 (data (i32.const 32) "nr")
                 (data (i32.const 48) "/out")
                 (func (export "run") (local $cl i32) (local $rl i32) (local $n i32)
                   (local.set $cl (call $in (i32.const 16)(i32.const 2)(i32.const 200)(i32.const 256)))
                   (local.set $rl (call $in (i32.const 32)(i32.const 2)(i32.const 600)(i32.const 256)))
                   (call $cc (i32.const 0)(i32.const 2)(i32.const 200)(local.get $cl)(i32.const 16))
                   (call $ca (i32.const 0)(i32.const 2)(i32.const 600)(local.get $rl))
                   (local.set $n (call $cs (i32.const 0)(i32.const 2)(i32.const 1000)(i32.const 256)))
                   (call $fw (i32.const 48)(i32.const 4)(i32.const 1000)(local.get $n))))"#,
        )
        .expect("assemble columnar round-trip program")
    }

    #[test]
    fn columnar_create_append_scan_through_host_abi() {
        use loom_core::tabular::{ColumnType, Value, encode_cells};
        let (mut loom, ns) = state_loom(16);
        // Schema `[("n", Int)]` and a single row `[Int(7)]`, both in their canonical guest wire forms.
        let cols_wire = encode_columns(&[("n".to_string(), ColumnType::Int)]);
        let row_wire = encode_cells(&[Value::Int(7)]);
        let inputs = BTreeMap::from([("nc".to_string(), cols_wire), ("nr".to_string(), row_wire)]);
        let grants = GrantSet::new(vec![
            Grant {
                facet: Capability::Columnar,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
            Grant {
                facet: Capability::Files,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
        ]);
        let state = StateAccess::new(&mut loom, context(ns, grants));
        let (state, outcome) =
            run_state(&columnar_roundtrip_program(), state, 2_000_000, inputs).unwrap();
        assert!(outcome.fuel_used > 0);
        drop(state);

        // The scan returned exactly the appended row, as an array of cell-array rows.
        assert_eq!(
            loom_codec::decode(&loom.read_file(ns, "/out").unwrap()).expect("scan decodes"),
            loom_codec::decode(&encode_rows(&[vec![Value::Int(7)]])).unwrap()
        );
    }

    fn columnar_select_aggregate_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "input_get" (func $in (param i32 i32 i32 i32) (result i32)))
                 (import "env" "columnar_select" (func $select (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "columnar_aggregate" (func $aggregate (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
                 (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "ds")
                 (data (i32.const 16) "cols")
                 (data (i32.const 32) "sf")
                 (data (i32.const 48) "aggs")
                 (data (i32.const 64) "af")
                 (data (i32.const 80) "/sel")
                 (data (i32.const 96) "/agg")
                 (func (export "run") (local $cl i32) (local $sfl i32) (local $al i32) (local $afl i32) (local $n i32)
                   (local.set $cl (call $in (i32.const 16)(i32.const 4)(i32.const 200)(i32.const 128)))
                   (local.set $sfl (call $in (i32.const 32)(i32.const 2)(i32.const 400)(i32.const 128)))
                   (local.set $al (call $in (i32.const 48)(i32.const 4)(i32.const 600)(i32.const 128)))
                   (local.set $afl (call $in (i32.const 64)(i32.const 2)(i32.const 800)(i32.const 16)))
                   (local.set $n (call $select (i32.const 0)(i32.const 2)(i32.const 200)(local.get $cl)(i32.const 400)(local.get $sfl)(i32.const 1000)(i32.const 256)))
                   (call $fw (i32.const 80)(i32.const 4)(i32.const 1000)(local.get $n))
                   (local.set $n (call $aggregate (i32.const 0)(i32.const 2)(i32.const 600)(local.get $al)(i32.const 800)(local.get $afl)(i32.const 1400)(i32.const 256)))
                   (call $fw (i32.const 96)(i32.const 4)(i32.const 1400)(local.get $n))))"#,
        )
        .expect("assemble columnar select/aggregate program")
    }

    fn columnar_select_columns_input() -> Vec<u8> {
        loom_codec::encode(&loom_codec::Value::Array(vec![loom_codec::Value::Text(
            "n".to_string(),
        )]))
        .unwrap()
    }

    fn columnar_filter_gt_five_input() -> Vec<u8> {
        loom_codec::encode(&loom_codec::Value::Array(vec![
            loom_codec::Value::Text("n".to_string()),
            loom_codec::Value::Uint(4),
            loom_core::tabular::cell_value(&Value::Int(5)),
        ]))
        .unwrap()
    }

    fn columnar_sum_aggregate_input() -> Vec<u8> {
        loom_codec::encode(&loom_codec::Value::Array(vec![loom_codec::Value::Array(
            vec![
                loom_codec::Value::Uint(4),
                loom_codec::Value::Text("n".to_string()),
            ],
        )]))
        .unwrap()
    }

    #[test]
    fn columnar_select_and_aggregate_through_host_abi() {
        use loom_core::tabular::{ColumnType, Value, encode_cells};
        let (mut loom, ns) = state_loom(22);
        loom_core::columnar_create(
            &mut loom,
            ns,
            "ds",
            vec![("n".to_string(), ColumnType::Int)],
            0,
        )
        .unwrap();
        loom_core::columnar_append(&mut loom, ns, "ds", vec![Value::Int(3)]).unwrap();
        loom_core::columnar_append(&mut loom, ns, "ds", vec![Value::Int(7)]).unwrap();
        let inputs = BTreeMap::from([
            ("cols".to_string(), columnar_select_columns_input()),
            ("sf".to_string(), columnar_filter_gt_five_input()),
            ("aggs".to_string(), columnar_sum_aggregate_input()),
            ("af".to_string(), Vec::new()),
        ]);
        let grants = GrantSet::new(vec![
            Grant {
                facet: Capability::Columnar,
                mode: Mode::Read,
                scopes: vec![Scope::All],
            },
            Grant {
                facet: Capability::Files,
                mode: Mode::ReadWrite,
                scopes: vec![Scope::All],
            },
        ]);
        let state = StateAccess::new(&mut loom, context(ns, grants));
        let (state, outcome) = run_state(
            &columnar_select_aggregate_program(),
            state,
            2_000_000,
            inputs,
        )
        .unwrap();
        assert!(outcome.fuel_used > 0);
        drop(state);

        assert_eq!(
            loom_codec::decode(&loom.read_file(ns, "/sel").unwrap()).unwrap(),
            loom_codec::decode(&encode_rows(&[vec![Value::Int(7)]])).unwrap()
        );
        assert_eq!(
            loom_codec::decode(&loom.read_file(ns, "/agg").unwrap()).unwrap(),
            loom_codec::decode(&encode_cells(&[Value::Int(10)])).unwrap()
        );
    }

    fn file_list_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "file_list" (func $fl (param i32 i32 i32 i32) (result i32)))
                 (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "dir")
                 (data (i32.const 16) "/listing")
                 (func (export "run") (local $n i32)
                   (local.set $n (call $fl (i32.const 0)(i32.const 3)(i32.const 200)(i32.const 256)))
                   (call $fw (i32.const 16)(i32.const 8)(i32.const 200)(local.get $n))))"#,
        )
        .expect("assemble file list program")
    }

    #[test]
    fn file_list_through_host_abi() {
        let (mut loom, ns) = state_loom(23);
        loom.create_directory(ns, "dir", false).unwrap();
        loom.write_file(ns, "dir/file.txt", b"hello", 0o100644)
            .unwrap();
        loom.create_directory(ns, "dir/sub", false).unwrap();
        let state = StateAccess::new(&mut loom, context(ns, all_kv_files()));
        let (state, outcome) =
            run_state(&file_list_program(), state, 1_000_000, BTreeMap::new()).unwrap();
        assert!(outcome.fuel_used > 0);
        drop(state);

        assert_eq!(
            loom_codec::decode(&loom.read_file(ns, "/listing").unwrap()).unwrap(),
            loom_codec::Value::Array(vec![
                loom_codec::Value::Array(vec![
                    loom_codec::Value::Text("file.txt".to_string()),
                    loom_codec::Value::Uint(0),
                ]),
                loom_codec::Value::Array(vec![
                    loom_codec::Value::Text("sub".to_string()),
                    loom_codec::Value::Uint(1),
                ]),
            ])
        );
    }

    #[test]
    fn kv_put_get_delete_list_through_host_abi() {
        let (mut loom, ns) = state_loom(1);
        // The ABI key wire form is Loom's canonical typed KV key (`key_to_cbor`). Text and Bytes are
        // two of the typed key variants, not the whole model; the host decodes with `key_from_cbor`.
        let text_key = Value::Text("kt".to_string());
        let bytes_key = Value::Bytes(vec![1, 2, 3]);
        let inputs = BTreeMap::from([
            ("nt".to_string(), key_to_cbor(&text_key)),
            ("nb".to_string(), key_to_cbor(&bytes_key)),
        ]);
        let state = StateAccess::new(&mut loom, context(ns, all_kv_files()));
        let (state, outcome) = run_state(&typed_key_program(), state, 1_000_000, inputs).unwrap();
        assert!(outcome.fuel_used > 0);
        drop(state);

        // The Bytes-keyed entry written through the ABI is readable by a direct `kv_get` on the same
        // typed key, and the Text-keyed entry was read (into /got) then deleted (so it is now absent).
        assert_eq!(loom.read_file(ns, "/got").unwrap(), b"v1");
        assert_eq!(kv_get(&loom, ns, "cache", &text_key).unwrap(), None);
        assert_eq!(
            kv_get(&loom, ns, "cache", &bytes_key).unwrap(),
            Some(b"v2".to_vec())
        );
        assert_eq!(loom.read_file(ns, "/count").unwrap(), vec![1u8]);
    }

    // Emits two log lines, "a" then "bb", through the `log` host call.
    fn log_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "log" (func $log (param i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "a")
                 (data (i32.const 16) "bb")
                 (func (export "run")
                   (call $log (i32.const 0)(i32.const 1))
                   (call $log (i32.const 16)(i32.const 2))))"#,
        )
        .expect("assemble log program")
    }

    #[test]
    fn logs_are_captured_in_order() {
        let (mut loom, ns) = state_loom(5);
        let state = StateAccess::new(&mut loom, context(ns, all_kv_files()));
        let (_state, outcome) =
            run_state(&log_program(), state, 1_000_000, BTreeMap::new()).unwrap();
        assert_eq!(outcome.logs, vec!["a".to_string(), "bb".to_string()]);
    }

    // Puts a Text key fetched from input `nt` into the `blocked` collection.
    fn blocked_kv_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "input_get" (func $in (param i32 i32 i32 i32) (result i32)))
                 (import "env" "kv_put" (func $put (param i32 i32 i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "blocked")
                 (data (i32.const 16) "nt")
                 (data (i32.const 32) "v")
                 (func (export "run") (local $l i32)
                   (local.set $l (call $in (i32.const 16)(i32.const 2)(i32.const 200)(i32.const 64)))
                   (call $put (i32.const 0)(i32.const 7)(i32.const 200)(local.get $l)(i32.const 32)(i32.const 1))))"#,
        )
        .expect("assemble blocked kv program")
    }

    #[test]
    fn denied_kv_write_does_not_mutate_state() {
        let (mut loom, ns) = state_loom(2);
        // ACL allows Exec; only the manifest grant (scoped to `cache/`) blocks the `blocked` write, so
        // this isolates a grant denial from a malformed-key rejection.
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Kv,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::Prefix("cache/".to_string())],
        }]);
        let key = Value::Text("k".to_string());
        let inputs = BTreeMap::from([("nt".to_string(), key_to_cbor(&key))]);
        let state = StateAccess::new(&mut loom, context(ns, grants));
        let err = match run_state(&blocked_kv_program(), state, 1_000_000, inputs) {
            Ok(_) => panic!("denied KV write must fail the run"),
            Err(err) => err,
        };
        assert_eq!(err.code(), loom_core::Code::PermissionDenied);
        assert_eq!(kv_get(&loom, ns, "blocked", &key).unwrap(), None);
    }

    // A one-call program over `op`, passing a single `0xFF` byte (offset 16) as the KV key. `0xFF` is
    // not a valid canonical typed-key CBOR cell, so the call must trap.
    fn malformed_key_program(op: &str, import: &str) -> Vec<u8> {
        wat::parse_str(format!(
            r#"(module
                 (import "env" "{op}" (func $op {import}))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "cache")
                 (data (i32.const 16) "\ff")
                 (data (i32.const 32) "v")
                 (func (export "run")
                   {call}))"#,
            call = match op {
                "kv_put" => "(call $op (i32.const 0)(i32.const 5)(i32.const 16)(i32.const 1)(i32.const 32)(i32.const 1))",
                "kv_delete" => "(drop (call $op (i32.const 0)(i32.const 5)(i32.const 16)(i32.const 1)))",
                _ => "(drop (call $op (i32.const 0)(i32.const 5)(i32.const 16)(i32.const 1)(i32.const 64)(i32.const 64)))",
            }
        ))
        .expect("assemble malformed-key program")
    }

    fn assert_malformed_traps(op: &str, import: &str) {
        let (mut loom, ns) = state_loom(3);
        let state = StateAccess::new(&mut loom, context(ns, all_kv_files()));
        let result = run_state(
            &malformed_key_program(op, import),
            state,
            1_000_000,
            BTreeMap::new(),
        );
        // Invalid canonical key bytes trap the call, so the run fails and nothing is written.
        assert!(
            matches!(result, Err(ExecError::Program(_))),
            "{op} must trap"
        );
        assert!(kv_list(&loom, ns, "cache").unwrap().is_empty());
    }

    #[test]
    fn malformed_key_in_kv_put_traps() {
        assert_malformed_traps("kv_put", "(param i32 i32 i32 i32 i32 i32)");
    }

    #[test]
    fn malformed_key_in_kv_get_traps() {
        assert_malformed_traps("kv_get", "(param i32 i32 i32 i32 i32 i32) (result i32)");
    }

    #[test]
    fn malformed_key_in_kv_delete_traps() {
        assert_malformed_traps("kv_delete", "(param i32 i32 i32 i32) (result i32)");
    }

    // Completes a valid put, then a malformed put that traps. The completed put persists; the trap
    // halts execution so no instruction after the malformed call runs.
    fn valid_then_malformed_program() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "input_get" (func $in (param i32 i32 i32 i32) (result i32)))
                 (import "env" "kv_put" (func $put (param i32 i32 i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "cache")
                 (data (i32.const 16) "\ff")
                 (data (i32.const 32) "v")
                 (data (i32.const 48) "nt")
                 (func (export "run") (local $l i32)
                   (local.set $l (call $in (i32.const 48)(i32.const 2)(i32.const 200)(i32.const 64)))
                   (call $put (i32.const 0)(i32.const 5)(i32.const 200)(local.get $l)(i32.const 32)(i32.const 1))
                   (call $put (i32.const 0)(i32.const 5)(i32.const 16)(i32.const 1)(i32.const 32)(i32.const 1))
                   (call $put (i32.const 0)(i32.const 5)(i32.const 200)(local.get $l)(i32.const 32)(i32.const 1))))"#,
        )
        .expect("assemble valid-then-malformed program")
    }

    #[test]
    fn trap_halts_after_completed_ops() {
        let (mut loom, ns) = state_loom(4);
        let key = Value::Text("kt".to_string());
        let inputs = BTreeMap::from([("nt".to_string(), key_to_cbor(&key))]);
        let state = StateAccess::new(&mut loom, context(ns, all_kv_files()));
        let result = run_state(&valid_then_malformed_program(), state, 1_000_000, inputs);
        assert!(matches!(result, Err(ExecError::Program(_))));
        // The op completed before the malformed call persisted; the collection has exactly that one
        // entry with value "v" (the third put, after the trap, never ran, but it would have been
        // idempotent anyway).
        assert_eq!(
            kv_get(&loom, ns, "cache", &key).unwrap(),
            Some(b"v".to_vec())
        );
        assert_eq!(kv_list(&loom, ns, "cache").unwrap().len(), 1);
    }
}
