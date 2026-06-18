use std::{collections::BTreeMap, env, path::Path};

use loom_compute::{
    Capability, DirectExecRequest, ExecContext, ExecStep, Grant, GrantSet, Manifest, Mode, Scope,
    direct, program_get, program_inspect, program_list, program_put_cel, program_put_template,
    program_put_wasm, render_template_program,
};
use loom_core::{AclRight, AclSubject, FacetKind, Loom, PrincipalId, WorkspaceId, WsSelector};
use loom_store::{FileStore, open_loom, open_loom_read, save_loom};

const WORKSPACE_NAME: &str = "programs";
const WORKSPACE_ID: WorkspaceId = WorkspaceId::from_bytes([42; 16]);
const PRINCIPAL: PrincipalId = PrincipalId::from_bytes([9; 16]);
const WASM_PROGRAM: &str = "wasm-file-writer";
const TEMPLATE_PROGRAM: &str = "template-card";
const CEL_PROGRAM: &str = "cel-threshold";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "all".to_string());
    let store = args.next().unwrap_or_else(|| "program.loom".to_string());

    match command.as_str() {
        "build" => build(&store)?,
        "call-wasm" => call_wasm(&store)?,
        "call-template" => call_template(&store)?,
        "call-cel" => call_cel(&store)?,
        "list" => list(&store)?,
        "all" => {
            build(&store)?;
            call_wasm(&store)?;
            call_template(&store)?;
            call_cel(&store)?;
            list(&store)?;
        }
        _ => {
            return Err(format!(
                "usage: program_engines [build|call-wasm|call-template|call-cel|list|all] [program.loom]"
            )
            .into());
        }
    }

    Ok(())
}

fn build(store: &str) -> Result<(), Box<dyn std::error::Error>> {
    if Path::new(store).exists() {
        std::fs::remove_file(store)?;
    }

    let mut loom = open_demo_loom(store)?;
    let ns = ensure_workspace(&mut loom)?;
    seed_acl_and_base(&mut loom, ns)?;

    let wasm = wasm_program()?;
    let wasm_manifest = Manifest::for_wasm(WASM_PROGRAM, &wasm, program_grants());
    program_put_wasm(&mut loom, ns, WASM_PROGRAM, wasm_manifest, &wasm)?;

    let template = template_program();
    let template_manifest = Manifest::for_template(TEMPLATE_PROGRAM, template, program_grants());
    program_put_template(&mut loom, ns, TEMPLATE_PROGRAM, template_manifest, template)?;

    let cel = cel_program();
    let cel_manifest = Manifest::for_cel(CEL_PROGRAM, cel, program_grants());
    program_put_cel(&mut loom, ns, CEL_PROGRAM, cel_manifest, cel)?;

    loom.commit(ns, "program-engines", "store program engine records", 2)?;
    save_loom(&mut loom)?;
    println!("built {store} with wasm, template, and cel programs");
    Ok(())
}

fn call_wasm(store: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut loom = open_demo_loom(store)?;
    let ns = open_workspace(&loom)?;
    let program = program_get(&loom, ns, WASM_PROGRAM)?.ok_or("missing wasm program")?;
    let report = direct(
        &mut loom,
        DirectExecRequest {
            context: exec_context(ns),
            step: ExecStep {
                manifest: program.record.manifest,
                wasm: &program.body,
                inputs: BTreeMap::new(),
                fuel: 10_000,
            },
            author: "program-engines".to_string(),
            message: "call stored wasm program".to_string(),
            timestamp_ms: 1,
        },
    )?;
    let output = loom.read_file(ns, "/wasm-output")?;
    save_loom(&mut loom)?;
    println!(
        "called wasm: logs={:?} output={}",
        report.logs,
        String::from_utf8(output)?
    );
    Ok(())
}

fn call_template(store: &str) -> Result<(), Box<dyn std::error::Error>> {
    let loom = open_demo_loom_read(store)?;
    let ns = open_workspace(&loom)?;
    let program = program_get(&loom, ns, TEMPLATE_PROGRAM)?.ok_or("missing template program")?;
    let source = String::from_utf8(program.body)?;
    let mut inputs = BTreeMap::new();
    inputs.insert("loom.title".to_string(), br#""Program engines""#.to_vec());
    inputs.insert(
        "program.summary".to_string(),
        b"stored template rendered".to_vec(),
    );
    inputs.insert(
        "request.path".to_string(),
        b"/programs/template-card".to_vec(),
    );
    let rendered = render_template_program(&program.record.manifest, &source, &inputs)?;
    println!(
        "called template: outputs={} logs={:?}",
        serde_json::to_string(&rendered.outputs)?,
        rendered.logs
    );
    Ok(())
}

fn call_cel(store: &str) -> Result<(), Box<dyn std::error::Error>> {
    let loom = open_demo_loom_read(store)?;
    let ns = open_workspace(&loom)?;
    let program = program_get(&loom, ns, CEL_PROGRAM)?.ok_or("missing cel program")?;
    let source = String::from_utf8(program.body)?;
    println!(
        "called cel: inspected source={} execution=target",
        serde_json::to_string(&source)?
    );
    Ok(())
}

fn list(store: &str) -> Result<(), Box<dyn std::error::Error>> {
    let loom = open_demo_loom_read(store)?;
    let ns = open_workspace(&loom)?;
    for program in program_list(&loom, ns)? {
        let inspected = program_inspect(&loom, ns, &program.name)?.ok_or("missing program")?;
        println!(
            "program name={} engine={} entry={} body_len={}",
            inspected.name, inspected.manifest.engine, inspected.manifest.entry, inspected.body_len
        );
    }
    Ok(())
}

fn open_demo_loom(store: &str) -> Result<Loom<FileStore>, Box<dyn std::error::Error>> {
    let mut loom = open_loom(store)?;
    if let Some(acl) = loom.store().acl_store()? {
        loom.set_acl_store(acl);
    }
    Ok(loom)
}

fn open_demo_loom_read(store: &str) -> Result<Loom<FileStore>, Box<dyn std::error::Error>> {
    let mut loom = open_loom_read(store)?;
    if let Some(acl) = loom.store().acl_store()? {
        loom.set_acl_store(acl);
    }
    Ok(loom)
}

fn ensure_workspace(loom: &mut Loom<FileStore>) -> Result<WorkspaceId, Box<dyn std::error::Error>> {
    let ns = match loom
        .registry()
        .open(&WsSelector::Name(WORKSPACE_NAME.to_string()))
    {
        Ok(ns) => ns,
        Err(_) => {
            loom.registry_mut()
                .create(FacetKind::Program, Some(WORKSPACE_NAME), WORKSPACE_ID)?
        }
    };
    for facet in [FacetKind::Program, FacetKind::Files, FacetKind::Kv] {
        if !loom.registry().has_facet(ns, facet)? {
            loom.registry_mut().add_facet(ns, facet)?;
        }
    }
    Ok(ns)
}

fn open_workspace(loom: &Loom<FileStore>) -> Result<WorkspaceId, Box<dyn std::error::Error>> {
    Ok(loom
        .registry()
        .open(&WsSelector::Name(WORKSPACE_NAME.to_string()))?)
}

fn seed_acl_and_base(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
) -> Result<(), Box<dyn std::error::Error>> {
    loom.acl_store_mut()
        .allow(
            AclSubject::Principal(PRINCIPAL),
            Some(ns),
            None,
            [AclRight::Execute],
        )
        .ok();
    loom.store().save_acl_store(loom.acl_store())?;
    loom.write_file(ns, "/seed", b"ready", 0o100644)?;
    loom.commit(ns, "program-engines", "seed program demo", 1)?;
    Ok(())
}

fn exec_context(ns: WorkspaceId) -> ExecContext {
    ExecContext {
        workspace: ns,
        principal: PRINCIPAL,
        roles: Vec::new(),
        authenticated: true,
        base_branch: "main".to_string(),
        grants: program_grants(),
    }
}

fn program_grants() -> GrantSet {
    GrantSet::new(vec![
        Grant {
            facet: Capability::Files,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::All],
        },
        Grant {
            facet: Capability::Kv,
            mode: Mode::ReadWrite,
            scopes: vec![Scope::All],
        },
    ])
}

fn wasm_program() -> Result<Vec<u8>, wat::Error> {
    wat::parse_str(
        r#"(module
             (import "env" "file_write" (func $file_write (param i32 i32 i32 i32)))
             (import "env" "log" (func $log (param i32 i32)))
             (memory (export "memory") 1)
             (data (i32.const 0) "/wasm-output")
             (data (i32.const 32) "stored wasm ran")
             (data (i32.const 64) "wasm-called")
             (func (export "run")
               (call $file_write (i32.const 0) (i32.const 12) (i32.const 32) (i32.const 15))
               (call $log (i32.const 64) (i32.const 11))))"#,
    )
}

fn template_program() -> &'static str {
    r#"{"outputs":{"html":"<section>{{ loom.title }}</section>","summary":{{ loom.program("summary") | tojson }},"path":{{ request.path | tojson }}},"logs":["template-called"]}"#
}

fn cel_program() -> &'static str {
    "request.amount < 100"
}
