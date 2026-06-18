//! Process-wide locator context: the project path, explicit `--config` files, and selected
//! first-class CLI context that feed every store locator resolution.
//!
//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use loom_locator::{ContextResolver, LayerRoots, Target};
use std::path::PathBuf;
use std::sync::OnceLock;

/// The resolved context layers for one CLI invocation.
pub(crate) struct LocatorContext {
    roots: LayerRoots,
    selected_context: Option<String>,
}

static CONTEXT: OnceLock<LocatorContext> = OnceLock::new();

impl LocatorContext {
    /// Build the context from the global `--project`, `--config`, and `--context` args. The project
    /// path (or, when `--project` is absent, the process working directory) is canonicalized so
    /// `<project>/.loom/contexts.toml` resolves to the same file regardless of the caller's directory.
    /// Explicit `--config` files are carried in command-line order (later overrides earlier).
    pub(crate) fn from_globals(
        project: Option<PathBuf>,
        configs: Vec<PathBuf>,
        selected_context: Option<String>,
    ) -> Result<Self, String> {
        let project = match project {
            Some(path) => path,
            None => std::env::current_dir()
                .map_err(|err| format!("cannot read working directory: {err}"))?,
        };
        let project = project.canonicalize().map_err(|err| {
            format!(
                "cannot canonicalize project path {}: {err}",
                project.display()
            )
        })?;
        Ok(Self {
            roots: LayerRoots {
                explicit_configs: configs,
                project: Some(project),
                user_home: home_dir(),
                system: Some(PathBuf::from("/etc")),
            },
            selected_context,
        })
    }

    /// Resolve a locator string against the context's locator rules.
    fn resolve(&self, locator: &str) -> Result<Target, String> {
        let resolver = ContextResolver::load(&self.roots).map_err(|err| err.to_string())?;
        resolver.resolve_str(locator).map_err(|err| err.to_string())
    }

    fn resolve_selected_context(&self) -> Result<Option<Target>, String> {
        let resolver = ContextResolver::load(&self.roots).map_err(|err| err.to_string())?;
        let Some(name) = self
            .selected_context
            .as_deref()
            .or_else(|| resolver.current_context())
        else {
            return Ok(None);
        };
        resolver
            .resolve_context(name)
            .map(Some)
            .map_err(|err| err.to_string())
    }

    pub(crate) fn project_context_file(&self) -> Result<PathBuf, String> {
        self.roots
            .project
            .as_ref()
            .map(|project| project.join(".loom").join("contexts.toml"))
            .ok_or_else(|| "project root is unavailable".to_string())
    }

    pub(crate) fn resolver(&self) -> Result<ContextResolver, String> {
        ContextResolver::load(&self.roots).map_err(|err| err.to_string())
    }

    /// Resolve a locator string to a [`Target`] (local path or remote endpoint) against the context's
    /// context layers. Used by commands that have been migrated onto the locator-aware `Loom { Local,
    /// Remote }` facade; unmigrated commands keep using [`LocatorContext::resolve_local`], which rejects
    /// remote targets.
    pub(crate) fn resolve_target(&self, locator: &str) -> Result<Target, String> {
        if locator == "context" {
            return self.resolve_selected_context()?.ok_or_else(|| {
                "no CLI context selected; pass --context or run `loom context use <name>`"
                    .to_string()
            });
        }
        self.resolve(locator)
    }

    /// Resolve a locator string to a local `.loom` path. A remote target is rejected here: the in-tree
    /// CLI opens local stores directly, and remote endpoints are served through the separate remote
    /// client surface.
    pub(crate) fn resolve_local(&self, locator: &str) -> Result<String, String> {
        let target = if locator == "context" {
            self.resolve_selected_context()?.ok_or_else(|| {
                "no CLI context selected; pass --context or run `loom context use <name>`"
                    .to_string()
            })?
        } else {
            self.resolve(locator)?
        };
        match target {
            Target::Local(path) => Ok(path.to_string_lossy().into_owned()),
            Target::Remote(target) => Err(format!(
                "locator {locator:?} resolves to remote endpoint {}, which the local CLI cannot open directly",
                target.url
            )),
        }
    }

    /// Enforce the local-store-administration boundary for a command that is local by design
    /// (`specs/0026`): identity authority detach/witness/replication administration operates directly on a
    /// store file's authority-state substrate and is never forwarded to a remote endpoint. A local locator
    /// passes; a remote locator is rejected with one stable `LOCAL_ADMIN_BOUNDARY` error (distinct from the
    /// generic [`LocatorContext::resolve_local`] remote rejection).
    pub(crate) fn require_local_admin(&self, locator: &str) -> Result<(), String> {
        let target = if locator == "context" {
            self.resolve_selected_context()?.ok_or_else(|| {
                "no CLI context selected; pass --context or run `loom context use <name>`"
                    .to_string()
            })?
        } else {
            self.resolve(locator)?
        };
        match target {
            Target::Local(_) => Ok(()),
            Target::Remote(target) => Err(format!(
                "LOCAL_ADMIN_BOUNDARY: locator {locator:?} resolves to remote endpoint {}; identity \
                 authority administration is local-store only (specs/0026) and is never forwarded to a \
                 remote endpoint. Point it at a local `.loom` path.",
                target.url
            )),
        }
    }
}

/// Install the process-wide locator context. Called once from `real_main` after argument parsing; a
/// second call is ignored so the first-installed context wins.
pub(crate) fn install(context: LocatorContext) {
    let _ = CONTEXT.set(context);
}

/// The installed locator context, or a working-directory default when none was installed (for example a
/// unit test that calls an open helper directly). The default reads the standard user and system layers
/// but no explicit `--config` files.
pub(crate) fn current() -> &'static LocatorContext {
    CONTEXT.get_or_init(|| {
        LocatorContext::from_globals(None, Vec::new(), None).unwrap_or_else(|_| LocatorContext {
            roots: LayerRoots::default(),
            selected_context: None,
        })
    })
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("loom-cli-200-{}-{tag}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join(".loom")).expect("create project .loom dir");
        dir
    }

    fn context_for(project: PathBuf, configs: Vec<PathBuf>) -> LocatorContext {
        LocatorContext::from_globals(Some(project), configs, None).expect("build context")
    }

    #[test]
    fn from_globals_carries_project_and_configs_into_roots() {
        let project = temp_project("roots");
        let cfg = project.join("extra.toml");
        std::fs::write(&cfg, "").expect("write config");
        let cx = context_for(project.clone(), vec![cfg.clone()]);
        assert_eq!(cx.roots.project, Some(project.canonicalize().unwrap()));
        assert_eq!(cx.roots.explicit_configs, vec![cfg]);
        assert_eq!(cx.roots.system, Some(PathBuf::from("/etc")));
        std::fs::remove_dir_all(&project).ok();
    }

    #[test]
    fn resolve_local_uses_project_context_layer() {
        let project = temp_project("localcontext");
        std::fs::write(
            project.join(".loom").join("contexts.toml"),
            "[cli]\ncurrent_context = \"mine\"\n\n[contexts.mine]\ntarget = \"file://data/app.loom\"\n",
        )
        .expect("write contexts.toml");
        let cx = context_for(project.clone(), Vec::new());
        assert_eq!(
            cx.resolve_local("context").expect("resolve context"),
            "data/app.loom"
        );
        assert_eq!(cx.resolve_local("mine").expect("resolve bare name"), "mine");
        // A path-like locator passes through unchanged.
        assert_eq!(
            cx.resolve_local("./local.loom").expect("resolve path"),
            "./local.loom"
        );
        std::fs::remove_dir_all(&project).ok();
    }

    #[test]
    fn resolve_local_rejects_remote_context() {
        let project = temp_project("remotecontext");
        std::fs::write(
            project.join(".loom").join("contexts.toml"),
            "[cli]\ncurrent_context = \"prod\"\n\n[contexts.prod]\ntarget = \"https://loom.example.com/prod\"\n",
        )
        .expect("write contexts.toml");
        let cx = context_for(project.clone(), Vec::new());
        let err = cx
            .resolve_local("context")
            .expect_err("remote must be rejected");
        assert!(err.contains("remote endpoint"), "unexpected error: {err}");
        std::fs::remove_dir_all(&project).ok();
    }

    #[test]
    fn mcp_remote_context_selection_prefers_explicit_cli_context() {
        let project = temp_project("mcpremotecontext");
        std::fs::write(
            project.join(".loom").join("contexts.toml"),
            "[cli]\ncurrent_context = \"prod\"\n\n[contexts.prod]\ntarget = \"https://prod.example.com/loom\"\n\n[contexts.staging]\ntarget = \"https://staging.example.com/loom\"\n",
        )
        .expect("write contexts.toml");
        let cx = LocatorContext::from_globals(
            Some(project.clone()),
            Vec::new(),
            Some("staging".to_string()),
        )
        .expect("build context");
        let target = cx
            .resolve_target("context")
            .expect("explicit CLI context resolves");
        let Target::Remote(remote) = target else {
            panic!("explicit CLI context should resolve to remote target");
        };
        assert_eq!(remote.url, "https://staging.example.com/loom");
        std::fs::remove_dir_all(&project).ok();
    }

    #[test]
    fn require_local_admin_rejects_remote_with_stable_boundary_error() {
        // Authority administration is local-store only. A remote context must be rejected with the stable
        // `LOCAL_ADMIN_BOUNDARY` error, and a local path must pass.
        let project = temp_project("localadmin");
        std::fs::write(
            project.join(".loom").join("contexts.toml"),
            "[cli]\ncurrent_context = \"prod\"\n\n[contexts.prod]\ntarget = \"https://loom.example.com/prod\"\n",
        )
        .expect("write contexts.toml");
        let cx = context_for(project.clone(), Vec::new());

        let err = cx
            .require_local_admin("context")
            .expect_err("remote must be rejected for local-admin commands");
        assert!(
            err.starts_with("LOCAL_ADMIN_BOUNDARY:"),
            "stable boundary prefix missing: {err}"
        );
        assert!(err.contains("specs/0026"), "spec citation missing: {err}");

        // A local path passes the boundary.
        cx.require_local_admin("./local.loom")
            .expect("local locator passes the local-admin boundary");
        std::fs::remove_dir_all(&project).ok();
    }
}
