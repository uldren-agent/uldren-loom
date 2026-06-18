//! The Wasmtime execution substrate (files facet).
//!
//! Native JIT (Cranelift); the same host ABI as the wasmi engine, so the gate and the rest of
//! the crate are engine-agnostic. Native-only - Wasmtime cannot target `wasm32`, so this module is
//! compiled only under the `engine-wasmtime` feature on non-`wasm32` targets.

use super::{FileSet, RunResult};
use crate::capability::{Capability, GrantSet, Mode};
use crate::error::ExecError;
use std::collections::BTreeMap;
use wasmtime::{Caller, Config, Engine, Extern, Linker, Memory, Module, Store};

struct HostCtx {
    files: FileSet,
    grants: GrantSet,
    inputs: BTreeMap<String, Vec<u8>>,
}

/// Run `wasm` over `files` with `fuel` units of budget under `grants`, with `inputs` available
/// read-only. Same contract as the wasmi engine's `run`.
pub fn run(
    wasm: &[u8],
    files: FileSet,
    fuel: u64,
    grants: GrantSet,
    inputs: BTreeMap<String, Vec<u8>>,
) -> Result<RunResult, ExecError> {
    let mut config = Config::new();
    config.consume_fuel(true);
    let engine =
        Engine::new(&config).map_err(|e| ExecError::Program(format!("wasmtime engine: {e}")))?;
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
                let mem = memory(&mut caller);
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
                let mem = memory(&mut caller);
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
                let mem = memory(&mut caller);
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
                let mem = memory(&mut caller);
                let name = read_string(&caller, &mem, np, nl);
                let found = caller.data().inputs.get(&name).cloned();
                write_out(&mut caller, &mem, op, oc, found)
            },
        )
        .map_err(|e| ExecError::Program(format!("link input_get: {e}")))?;

    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| ExecError::Program(format!("instantiate: {e}")))?;
    let run = instance
        .get_typed_func::<(), ()>(&mut store, "run")
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
        Err(err) => {
            let out_of_fuel = matches!(
                err.downcast_ref::<wasmtime::Trap>(),
                Some(wasmtime::Trap::OutOfFuel)
            ) || err.to_string().to_lowercase().contains("fuel");
            if out_of_fuel {
                Err(ExecError::BudgetExceeded { budget: fuel })
            } else {
                Err(ExecError::Program(format!("trap: {err}")))
            }
        }
    }
}

fn memory(caller: &mut Caller<'_, HostCtx>) -> Memory {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .expect("program must export `memory`")
}

fn read_string(caller: &Caller<'_, HostCtx>, mem: &Memory, ptr: i32, len: i32) -> String {
    let mut buf = vec![0u8; len.max(0) as usize];
    mem.read(caller, ptr as usize, &mut buf)
        .expect("read string");
    String::from_utf8_lossy(&buf).into_owned()
}

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

#[cfg(test)]
mod tests {
    use crate::capability::{Capability, Grant, GrantSet, Mode, Scope};
    use std::collections::BTreeMap;

    fn writer_wasm() -> Vec<u8> {
        wat::parse_str(
            r#"(module
                 (import "env" "file_write" (func $fw (param i32 i32 i32 i32)))
                 (memory (export "memory") 1)
                 (data (i32.const 0) "/greeting")
                 (data (i32.const 16) "hello world")
                 (func (export "run")
                   (call $fw (i32.const 0) (i32.const 9) (i32.const 16) (i32.const 11))))"#,
        )
        .expect("assemble writer wasm")
    }

    /// Wasmtime and wasmi produce the same file set (the same state root) for the same program -
    /// the cross-engine equivalence vector.
    #[test]
    fn wasmtime_matches_wasmi() {
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Files,
            scopes: vec![Scope::All],
            mode: Mode::ReadWrite,
        }]);
        let wasm = writer_wasm();
        let by_wasmtime = super::run(
            &wasm,
            BTreeMap::new(),
            100_000,
            grants.clone(),
            BTreeMap::new(),
        )
        .unwrap();
        let by_wasmi = super::super::engine_wasmi::run(
            &wasm,
            BTreeMap::new(),
            100_000,
            grants,
            BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(by_wasmtime.files, by_wasmi.files);
    }
}
