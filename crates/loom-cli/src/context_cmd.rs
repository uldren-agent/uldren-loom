//! First-class CLI context configuration commands.
//!
//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use crate::{ContextCmd, locator_cx};
use loom_locator::{ContextEntry, ContextFile, Target};

pub(crate) fn run_context(action: ContextCmd) -> Result<(), String> {
    match action {
        ContextCmd::List { format } => {
            let resolver = locator_cx::current().resolver()?;
            let names = resolver.context_names();
            if format == "json" {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "contexts": names,
                        "current_context": resolver.current_context()
                    }))
                    .map_err(|err| err.to_string())?
                );
            } else if names.is_empty() {
                println!("(no contexts)");
            } else {
                let current = resolver.current_context();
                for name in names {
                    let marker = if current == Some(name.as_str()) {
                        "*"
                    } else {
                        " "
                    };
                    println!("{marker}\t{name}");
                }
            }
            Ok(())
        }
        ContextCmd::Get { name, format } => {
            let resolver = locator_cx::current().resolver()?;
            let context = resolver
                .context(&name)
                .ok_or_else(|| format!("context not found: {name}"))?;
            print_context(&name, context, &format)
        }
        ContextCmd::Add {
            name,
            target,
            default_workspace,
            auth,
            tls,
            discovery,
            discovery_path,
            connect_timeout_ms,
            request_timeout_ms,
            description,
            format,
        } => {
            let mut file = read_project_context_file()?;
            if file.contexts.contains_key(&name) {
                return Err(format!("context already exists: {name}"));
            }
            file.contexts.insert(
                name.clone(),
                ContextEntry {
                    target,
                    default_workspace,
                    auth,
                    tls,
                    discovery,
                    discovery_path,
                    connect_timeout_ms,
                    request_timeout_ms,
                    description,
                },
            );
            write_project_context_file(&file)?;
            let resolver = locator_cx::current().resolver()?;
            let context = resolver
                .context(&name)
                .ok_or_else(|| format!("context not found after write: {name}"))?;
            print_context(&name, context, &format)
        }
        ContextCmd::Update {
            name,
            target,
            default_workspace,
            auth,
            tls,
            discovery,
            discovery_path,
            connect_timeout_ms,
            request_timeout_ms,
            description,
            format,
        } => {
            let mut file = read_project_context_file()?;
            let context = file
                .contexts
                .get_mut(&name)
                .ok_or_else(|| format!("context not found: {name}"))?;
            if let Some(value) = target {
                context.target = value;
            }
            if default_workspace.is_some() {
                context.default_workspace = default_workspace;
            }
            if auth.is_some() {
                context.auth = auth;
            }
            if tls.is_some() {
                context.tls = tls;
            }
            if discovery.is_some() {
                context.discovery = discovery;
            }
            if discovery_path.is_some() {
                context.discovery_path = discovery_path;
            }
            if connect_timeout_ms.is_some() {
                context.connect_timeout_ms = connect_timeout_ms;
            }
            if request_timeout_ms.is_some() {
                context.request_timeout_ms = request_timeout_ms;
            }
            if description.is_some() {
                context.description = description;
            }
            write_project_context_file(&file)?;
            let resolver = locator_cx::current().resolver()?;
            let context = resolver
                .context(&name)
                .ok_or_else(|| format!("context not found after write: {name}"))?;
            print_context(&name, context, &format)
        }
        ContextCmd::Remove { name } => {
            let mut file = read_project_context_file()?;
            if file.contexts.remove(&name).is_none() {
                return Err(format!("context not found: {name}"));
            }
            if file.cli.current_context.as_deref() == Some(&name) {
                file.cli.current_context = None;
            }
            write_project_context_file(&file)?;
            println!("removed\t{name}");
            Ok(())
        }
        ContextCmd::Test { name, format } => {
            let resolver = locator_cx::current().resolver()?;
            let target = resolver
                .resolve_context(&name)
                .map_err(|err| err.to_string())?;
            print_target(&name, &target, &format)
        }
        ContextCmd::Use { name } => {
            let resolver = locator_cx::current().resolver()?;
            if !resolver.has_context(&name) {
                return Err(format!("context not found: {name}"));
            }
            let mut file = read_project_context_file()?;
            file.cli.current_context = Some(name.clone());
            write_project_context_file(&file)?;
            println!("current_context\t{name}");
            Ok(())
        }
        ContextCmd::Current { format } => {
            let resolver = locator_cx::current().resolver()?;
            let current = resolver.current_context();
            if format == "json" {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "current_context": current
                    }))
                    .map_err(|err| err.to_string())?
                );
            } else {
                println!("current_context\t{}", current.unwrap_or(""));
            }
            Ok(())
        }
    }
}

fn read_project_context_file() -> Result<ContextFile, String> {
    let path = locator_cx::current().project_context_file()?;
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(|err| err.to_string()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(ContextFile::default()),
        Err(err) => Err(format!("cannot read {}: {err}", path.display())),
    }
}

fn write_project_context_file(file: &ContextFile) -> Result<(), String> {
    let path = locator_cx::current().project_context_file()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let text = toml::to_string_pretty(file).map_err(|err| err.to_string())?;
    std::fs::write(&path, text).map_err(|err| format!("cannot write {}: {err}", path.display()))
}

fn print_context(
    name: &str,
    context: &loom_locator::ContextDef,
    format: &str,
) -> Result<(), String> {
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "name": name,
                "target": &context.target,
                "default_workspace": &context.default_workspace
            }))
            .map_err(|err| err.to_string())?
        );
    } else {
        println!("name\t{name}");
        println!("target\t{}", context.target);
        println!(
            "default_workspace\t{}",
            context.default_workspace.as_deref().unwrap_or("")
        );
    }
    Ok(())
}

fn print_target(name: &str, target: &Target, format: &str) -> Result<(), String> {
    let (kind, value) = match target {
        Target::Local(path) => ("local", path.display().to_string()),
        Target::Remote(remote) => ("remote", remote.url.clone()),
    };
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "name": name,
                "target_kind": kind,
                "target": value
            }))
            .map_err(|err| err.to_string())?
        );
    } else {
        println!("name\t{name}");
        println!("target_kind\t{kind}");
        println!("target\t{value}");
    }
    Ok(())
}
