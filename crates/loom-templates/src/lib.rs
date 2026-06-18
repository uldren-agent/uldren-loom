//! Jinja-compatible template planning for Loom-authored content.
//!
//! `loom-templates` owns parsing, planning, and the template binding surface. Callers supply
//! authorized environment values, serving, storage, and host operation execution.

use loom_core::digest::Digest;
use minijinja::machinery::ast::{self, CallArg, Expr, Stmt};
use minijinja::machinery::parse;
use minijinja::value::{Kwargs, Object, Value, from_args};
use minijinja::{Environment, Error as MiniJinjaError, ErrorKind, State};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use thiserror::Error;

pub const SYNTAX_VERSION: &str = "loom-templates/0.1.0";

#[derive(Debug, Default, Clone)]
pub struct TemplateProcessor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplatePlan {
    pub source_path: String,
    pub source_digest: Digest,
    pub ast_digest: Digest,
    pub syntax_version: &'static str,
    pub dependencies: Vec<TemplateDependency>,
    pub host_calls: Vec<HostCall>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedTemplate {
    pub plan: TemplatePlan,
    pub html: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateBindings {
    pub loom: LoomBindings,
    /// App/route metadata exposed to templates under the `meta.*` root (parallel to `loom.*`).
    /// Populated from the `_meta.md` front matter so templates can read any declared setting.
    pub meta: BTreeMap<String, serde_json::Value>,
    pub request: BTreeMap<String, String>,
    pub response: BTreeMap<String, String>,
    pub session: BTreeMap<String, String>,
    pub cookie: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoomBindings {
    pub programs: BTreeMap<String, String>,
    pub values: BTreeMap<String, serde_json::Value>,
}

impl TemplateBindings {
    pub fn with_program_output(
        mut self,
        name: impl Into<String>,
        output: impl Into<String>,
    ) -> Self {
        self.loom.programs.insert(name.into(), output.into());
        self
    }

    pub fn with_loom_value(mut self, name: impl Into<String>, value: serde_json::Value) -> Self {
        self.loom.values.insert(name.into(), value);
        self
    }

    /// Expose an object's top-level fields under the `meta.*` template root. Non-object values are
    /// ignored. Typically fed the serialized app/route `_meta.md`.
    pub fn with_meta(mut self, value: serde_json::Value) -> Self {
        if let serde_json::Value::Object(map) = value {
            for (key, val) in map {
                self.meta.insert(key, val);
            }
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateCacheInput {
    pub source_digest: Digest,
    pub syntax_version: String,
    pub consumer: TemplateConsumer,
    pub metadata_digest: Option<Digest>,
    pub program_bindings: Vec<ProgramBinding>,
    pub grants_profile_digest: Option<Digest>,
    pub render_options: Vec<RenderOption>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TemplateConsumer {
    App,
    Route,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProgramBinding {
    pub name: String,
    pub manifest_digest: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RenderOption {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateCacheKey {
    pub digest: Digest,
}

impl TemplateCacheInput {
    pub fn from_plan(plan: &TemplatePlan, consumer: TemplateConsumer) -> Self {
        Self {
            source_digest: plan.source_digest,
            syntax_version: plan.syntax_version.to_string(),
            consumer,
            metadata_digest: None,
            program_bindings: Vec::new(),
            grants_profile_digest: None,
            render_options: Vec::new(),
        }
    }

    pub fn cache_key(&self) -> TemplateCacheKey {
        let mut program_bindings = self.program_bindings.clone();
        program_bindings.sort();
        let mut render_options = self.render_options.clone();
        render_options.sort();

        let mut bytes = Vec::new();
        push_field(&mut bytes, "syntax", self.syntax_version.as_bytes());
        push_field(&mut bytes, "source", self.source_digest.to_hex().as_bytes());
        push_field(
            &mut bytes,
            "consumer",
            match self.consumer {
                TemplateConsumer::App => b"app",
                TemplateConsumer::Route => b"route",
            },
        );
        push_digest_opt(&mut bytes, "metadata", self.metadata_digest);
        push_digest_opt(&mut bytes, "grants", self.grants_profile_digest);

        for binding in program_bindings {
            push_field(&mut bytes, "program.name", binding.name.as_bytes());
            push_field(
                &mut bytes,
                "program.manifest",
                binding.manifest_digest.to_hex().as_bytes(),
            );
        }
        for option in render_options {
            push_field(&mut bytes, "render.key", option.key.as_bytes());
            push_field(&mut bytes, "render.value", option.value.as_bytes());
        }

        TemplateCacheKey {
            digest: Digest::blake3(&bytes),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TemplateDependency {
    pub kind: TemplateDependencyKind,
    pub target: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TemplateDependencyKind {
    Include,
    Extends,
    Import,
    FromImport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCall {
    pub kind: HostCallKind,
    pub target: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCallKind {
    Program,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub code: &'static str,
    pub message: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct SourceSpan {
    pub start_line: u16,
    pub start_col: u16,
    pub start_offset: u32,
    pub end_line: u16,
    pub end_col: u16,
    pub end_offset: u32,
}

#[derive(Debug, Error)]
pub enum TemplateError {
    #[error("template parse failed for {path}: {message}")]
    Parse { path: String, message: String },
    #[error("template render failed for {path}: {message}")]
    Render { path: String, message: String },
    #[error("unknown Loom template call {name}")]
    UnknownLoomCall { name: String },
    #[error("loom.program requires a static string name")]
    InvalidProgramCall,
}

impl TemplateProcessor {
    pub fn new() -> Self {
        Self
    }

    pub fn process(&self, source_path: impl Into<String>, source: &str) -> Result<TemplatePlan> {
        let source_path = source_path.into();
        compile_template(&source_path, source)?;
        let ast =
            parse(source, &source_path, Default::default(), Default::default()).map_err(|err| {
                TemplateError::Parse {
                    path: source_path.clone(),
                    message: error_message(&err),
                }
            })?;

        let mut visitor = PlanVisitor::default();
        visitor.visit_stmt(&ast)?;

        let mut dependencies = visitor.dependencies.into_iter().collect::<Vec<_>>();
        dependencies.sort();
        let ast_digest = plan_digest(source, &dependencies, &visitor.host_calls);

        Ok(TemplatePlan {
            source_path,
            source_digest: Digest::blake3(source.as_bytes()),
            ast_digest,
            syntax_version: SYNTAX_VERSION,
            dependencies,
            host_calls: visitor.host_calls,
            diagnostics: visitor.diagnostics,
        })
    }

    pub fn render(
        &self,
        source_path: impl Into<String>,
        source: &str,
        bindings: &TemplateBindings,
    ) -> Result<RenderedTemplate> {
        let source_path = source_path.into();
        let plan = self.process(source_path.clone(), source)?;
        let html = render_template(&source_path, source, bindings)?;
        Ok(RenderedTemplate { plan, html })
    }
}

pub type Result<T> = std::result::Result<T, TemplateError>;

#[derive(Default)]
struct PlanVisitor {
    dependencies: BTreeSet<TemplateDependency>,
    host_calls: Vec<HostCall>,
    diagnostics: Vec<Diagnostic>,
}

impl PlanVisitor {
    fn visit_stmt(&mut self, stmt: &Stmt<'_>) -> Result<()> {
        match stmt {
            Stmt::Template(node) => self.visit_stmts(&node.children),
            Stmt::EmitExpr(node) => self.visit_expr(&node.expr),
            Stmt::EmitRaw(_) => Ok(()),
            Stmt::ForLoop(node) => {
                self.visit_expr(&node.target)?;
                self.visit_expr(&node.iter)?;
                if let Some(expr) = &node.filter_expr {
                    self.visit_expr(expr)?;
                }
                self.visit_stmts(&node.body)?;
                self.visit_stmts(&node.else_body)
            }
            Stmt::IfCond(node) => {
                self.visit_expr(&node.expr)?;
                self.visit_stmts(&node.true_body)?;
                self.visit_stmts(&node.false_body)
            }
            Stmt::WithBlock(node) => {
                for (target, expr) in &node.assignments {
                    self.visit_expr(target)?;
                    self.visit_expr(expr)?;
                }
                self.visit_stmts(&node.body)
            }
            Stmt::Set(node) => {
                self.visit_expr(&node.target)?;
                self.visit_expr(&node.expr)
            }
            Stmt::SetBlock(node) => {
                self.visit_expr(&node.target)?;
                if let Some(filter) = &node.filter {
                    self.visit_expr(filter)?;
                }
                self.visit_stmts(&node.body)
            }
            Stmt::AutoEscape(node) => {
                self.visit_expr(&node.enabled)?;
                self.visit_stmts(&node.body)
            }
            Stmt::FilterBlock(node) => {
                self.visit_expr(&node.filter)?;
                self.visit_stmts(&node.body)
            }
            Stmt::Block(node) => self.visit_stmts(&node.body),
            Stmt::Import(node) => {
                self.record_template_dependency(
                    TemplateDependencyKind::Import,
                    &node.expr,
                    node.span(),
                );
                self.visit_expr(&node.name)
            }
            Stmt::FromImport(node) => {
                self.record_template_dependency(
                    TemplateDependencyKind::FromImport,
                    &node.expr,
                    node.span(),
                );
                for (name, alias) in &node.names {
                    self.visit_expr(name)?;
                    if let Some(alias) = alias {
                        self.visit_expr(alias)?;
                    }
                }
                Ok(())
            }
            Stmt::Extends(node) => {
                self.record_template_dependency(
                    TemplateDependencyKind::Extends,
                    &node.name,
                    node.span(),
                );
                Ok(())
            }
            Stmt::Include(node) => {
                self.record_template_dependency(
                    TemplateDependencyKind::Include,
                    &node.name,
                    node.span(),
                );
                Ok(())
            }
            Stmt::Macro(node) => {
                for arg in &node.args {
                    self.visit_expr(arg)?;
                }
                for default in &node.defaults {
                    self.visit_expr(default)?;
                }
                self.visit_stmts(&node.body)
            }
            Stmt::CallBlock(node) => {
                self.visit_call(&node.call)?;
                for arg in &node.macro_decl.args {
                    self.visit_expr(arg)?;
                }
                for default in &node.macro_decl.defaults {
                    self.visit_expr(default)?;
                }
                self.visit_stmts(&node.macro_decl.body)
            }
            Stmt::Do(node) => self.visit_call(&node.call),
        }
    }

    fn visit_stmts(&mut self, stmts: &[Stmt<'_>]) -> Result<()> {
        for stmt in stmts {
            self.visit_stmt(stmt)?;
        }
        Ok(())
    }

    fn visit_expr(&mut self, expr: &Expr<'_>) -> Result<()> {
        match expr {
            Expr::Var(_) | Expr::Const(_) => Ok(()),
            Expr::Slice(node) => {
                self.visit_expr(&node.expr)?;
                self.visit_expr_opt(&node.start)?;
                self.visit_expr_opt(&node.stop)?;
                self.visit_expr_opt(&node.step)
            }
            Expr::UnaryOp(node) => self.visit_expr(&node.expr),
            Expr::BinOp(node) => {
                self.visit_expr(&node.left)?;
                self.visit_expr(&node.right)
            }
            Expr::Compare(node) => {
                self.visit_expr(&node.expr)?;
                for op in &node.ops {
                    self.visit_expr(&op.expr)?;
                }
                Ok(())
            }
            Expr::IfExpr(node) => {
                self.visit_expr(&node.test_expr)?;
                self.visit_expr(&node.true_expr)?;
                self.visit_expr_opt(&node.false_expr)
            }
            Expr::Filter(node) => {
                self.visit_expr_opt(&node.expr)?;
                self.visit_args(&node.args)
            }
            Expr::Test(node) => {
                self.visit_expr(&node.expr)?;
                self.visit_args(&node.args)
            }
            Expr::GetAttr(node) => self.visit_expr(&node.expr),
            Expr::GetItem(node) => {
                self.visit_expr(&node.expr)?;
                self.visit_expr(&node.subscript_expr)
            }
            Expr::Call(node) => self.visit_call(node),
            Expr::List(node) => {
                for item in &node.items {
                    self.visit_expr(item)?;
                }
                Ok(())
            }
            Expr::Map(node) => {
                for (key, value) in node.keys.iter().zip(node.values.iter()) {
                    self.visit_expr(key)?;
                    self.visit_expr(value)?;
                }
                Ok(())
            }
        }
    }

    fn visit_expr_opt(&mut self, expr: &Option<Expr<'_>>) -> Result<()> {
        if let Some(expr) = expr {
            self.visit_expr(expr)?;
        }
        Ok(())
    }

    fn visit_args(&mut self, args: &[CallArg<'_>]) -> Result<()> {
        for arg in args {
            self.visit_arg(arg)?;
        }
        Ok(())
    }

    fn visit_arg(&mut self, arg: &CallArg<'_>) -> Result<()> {
        match arg {
            CallArg::Pos(expr)
            | CallArg::Kwarg(_, expr)
            | CallArg::PosSplat(expr)
            | CallArg::KwargSplat(expr) => self.visit_expr(expr),
        }
    }

    fn visit_call(&mut self, call: &ast::Call<'_>) -> Result<()> {
        if let Some(path) = expr_path(&call.expr)
            && path.first().is_some_and(|part| *part == "loom")
        {
            return self.visit_loom_call(path, call);
        }

        self.visit_expr(&call.expr)?;
        self.visit_args(&call.args)
    }

    fn visit_loom_call(&mut self, path: Vec<&str>, call: &ast::Call<'_>) -> Result<()> {
        if path.as_slice() != ["loom", "program"] {
            return Err(TemplateError::UnknownLoomCall {
                name: path.join("."),
            });
        }

        let Some(name) = program_name(call) else {
            return Err(TemplateError::InvalidProgramCall);
        };

        self.host_calls.push(HostCall {
            kind: HostCallKind::Program,
            target: name,
            span: span(call.expr.span()),
        });
        self.visit_args(&call.args)
    }

    fn record_template_dependency(
        &mut self,
        kind: TemplateDependencyKind,
        expr: &Expr<'_>,
        span: minijinja::machinery::Span,
    ) {
        if let Some(target) = static_string(expr) {
            self.dependencies.insert(TemplateDependency {
                kind,
                target,
                span: self::span(span),
            });
        } else {
            self.diagnostics.push(Diagnostic {
                severity: DiagnosticSeverity::Warning,
                code: "dynamic-template-reference",
                message: "dynamic template references are not dependency tracked".to_string(),
                span: self::span(span),
            });
        }
    }
}

fn compile_template(path: &str, source: &str) -> Result<()> {
    let mut env = Environment::new();
    env.add_template(path, source)
        .map_err(|err| TemplateError::Parse {
            path: path.to_string(),
            message: error_message(&err),
        })?;
    env.get_template(path).map_err(|err| TemplateError::Parse {
        path: path.to_string(),
        message: error_message(&err),
    })?;
    Ok(())
}

fn render_template(path: &str, source: &str, bindings: &TemplateBindings) -> Result<String> {
    let mut env = Environment::new();
    add_binding_globals(&mut env, bindings);
    env.add_template(path, source)
        .map_err(|err| TemplateError::Parse {
            path: path.to_string(),
            message: error_message(&err),
        })?;
    let template = env.get_template(path).map_err(|err| TemplateError::Parse {
        path: path.to_string(),
        message: error_message(&err),
    })?;
    template.render(()).map_err(|err| TemplateError::Render {
        path: path.to_string(),
        message: error_message(&err),
    })
}

fn add_binding_globals(env: &mut Environment<'_>, bindings: &TemplateBindings) {
    env.add_global(
        "loom",
        Value::from_object(LoomBindingObject {
            programs: bindings.loom.programs.clone(),
            values: bindings.loom.values.clone(),
        }),
    );
    env.add_global("meta", Value::from_serialize(&bindings.meta));
    env.add_global("request", Value::from_serialize(&bindings.request));
    env.add_global("response", Value::from_serialize(&bindings.response));
    env.add_global("session", Value::from_serialize(&bindings.session));
    env.add_global("cookie", Value::from_serialize(&bindings.cookie));
}

#[derive(Debug)]
struct LoomBindingObject {
    programs: BTreeMap<String, String>,
    values: BTreeMap<String, serde_json::Value>,
}

impl Object for LoomBindingObject {
    fn get_value_by_str(self: &Arc<Self>, key: &str) -> Option<Value> {
        self.values.get(key).map(Value::from_serialize)
    }

    fn call_method(
        self: &Arc<Self>,
        _state: &State<'_, '_>,
        method: &str,
        args: &[Value],
    ) -> std::result::Result<Value, MiniJinjaError> {
        match method {
            "program" => Ok(Value::from_safe_string(self.program_output(args)?)),
            _ => Err(MiniJinjaError::from(ErrorKind::UnknownMethod)),
        }
    }
}

impl LoomBindingObject {
    fn program_output(&self, args: &[Value]) -> std::result::Result<String, MiniJinjaError> {
        let (positional, kwargs): (&[Value], Kwargs) = from_args(args)?;
        let positional_name = match positional {
            [] => None,
            [value] => Some(value.as_str().ok_or_else(invalid_program_name)?.to_string()),
            _ => return Err(MiniJinjaError::from(ErrorKind::TooManyArguments)),
        };
        let named_name = if kwargs.has("name") {
            Some(kwargs.get::<String>("name")?)
        } else {
            None
        };
        kwargs.assert_all_used()?;

        let name = named_name.or(positional_name).ok_or_else(|| {
            MiniJinjaError::new(ErrorKind::MissingArgument, "missing program name")
        })?;
        Ok(self.programs.get(&name).cloned().unwrap_or_default())
    }
}

fn invalid_program_name() -> MiniJinjaError {
    MiniJinjaError::new(ErrorKind::InvalidOperation, "program name must be a string")
}

fn program_name(call: &ast::Call<'_>) -> Option<String> {
    let mut positional = None;
    let mut named = None;

    for arg in &call.args {
        match arg {
            CallArg::Pos(expr) => positional = static_string(expr),
            CallArg::Kwarg("name", expr) => named = static_string(expr),
            CallArg::Kwarg(_, _) | CallArg::PosSplat(_) | CallArg::KwargSplat(_) => return None,
        }
    }

    named.or(positional)
}

fn expr_path<'a>(expr: &'a Expr<'a>) -> Option<Vec<&'a str>> {
    match expr {
        Expr::Var(var) => Some(vec![var.id]),
        Expr::GetAttr(attr) => {
            let mut path = expr_path(&attr.expr)?;
            path.push(attr.name);
            Some(path)
        }
        _ => None,
    }
}

fn static_string(expr: &Expr<'_>) -> Option<String> {
    expr.as_const()
        .and_then(|value| value.as_str().map(str::to_string))
}

fn span(span: minijinja::machinery::Span) -> SourceSpan {
    SourceSpan {
        start_line: span.start_line,
        start_col: span.start_col,
        start_offset: span.start_offset,
        end_line: span.end_line,
        end_col: span.end_col,
        end_offset: span.end_offset,
    }
}

fn plan_digest(
    source: &str,
    dependencies: &[TemplateDependency],
    host_calls: &[HostCall],
) -> Digest {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(SYNTAX_VERSION.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(source.as_bytes());
    bytes.push(0);

    for dependency in dependencies {
        bytes
            .extend_from_slice(format!("{:?}:{}\n", dependency.kind, dependency.target).as_bytes());
    }
    bytes.push(0);
    for call in host_calls {
        bytes.extend_from_slice(format!("{:?}:{}\n", call.kind, call.target).as_bytes());
    }

    Digest::blake3(&bytes)
}

fn push_digest_opt(bytes: &mut Vec<u8>, label: &str, digest: Option<Digest>) {
    match digest {
        Some(digest) => push_field(bytes, label, digest.to_hex().as_bytes()),
        None => push_field(bytes, label, b""),
    }
}

fn push_field(bytes: &mut Vec<u8>, label: &str, value: &[u8]) {
    bytes.extend_from_slice(label.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(value.len().to_string().as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(value);
    bytes.push(0);
}

fn error_message(err: &MiniJinjaError) -> String {
    match (err.line(), err.detail()) {
        (Some(line), Some(detail)) => format!("line {line}: {detail}"),
        (Some(line), None) => format!("line {line}: {}", err.kind()),
        (None, Some(detail)) => detail.to_string(),
        (None, None) => err.kind().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_program_call_and_static_include() {
        let plan = TemplateProcessor::new()
            .process(
                "index.html",
                r#"{% include "partial.html" %}{{ loom.program(name="dashboard/load") }}"#,
            )
            .unwrap();

        assert_eq!(
            plan.dependencies,
            vec![TemplateDependency {
                kind: TemplateDependencyKind::Include,
                target: "partial.html".to_string(),
                span: plan.dependencies[0].span,
            }]
        );
        assert_eq!(
            plan.host_calls,
            vec![HostCall {
                kind: HostCallKind::Program,
                target: "dashboard/load".to_string(),
                span: plan.host_calls[0].span,
            }]
        );
        assert!(plan.diagnostics.is_empty());
    }

    #[test]
    fn rejects_unknown_loom_call() {
        let err = TemplateProcessor::new()
            .process("index.html", "{{ loom.tool(name=\"x\") }}")
            .unwrap_err();

        assert!(matches!(err, TemplateError::UnknownLoomCall { name } if name == "loom.tool"));
    }

    #[test]
    fn rejects_dynamic_program_name() {
        let err = TemplateProcessor::new()
            .process("index.html", "{{ loom.program(name=program_name) }}")
            .unwrap_err();

        assert!(matches!(err, TemplateError::InvalidProgramCall));
    }

    #[test]
    fn records_dynamic_include_as_diagnostic() {
        let plan = TemplateProcessor::new()
            .process("index.html", "{% include partial_name %}")
            .unwrap();

        assert!(plan.dependencies.is_empty());
        assert_eq!(plan.diagnostics.len(), 1);
        assert_eq!(plan.diagnostics[0].code, "dynamic-template-reference");
    }

    #[test]
    fn plan_digest_is_stable_for_same_source() {
        let processor = TemplateProcessor::new();
        let first = processor
            .process("index.html", "{{ loom.program('dashboard/load') }}")
            .unwrap();
        let second = processor
            .process("index.html", "{{ loom.program('dashboard/load') }}")
            .unwrap();

        assert_eq!(first.source_digest, second.source_digest);
        assert_eq!(first.ast_digest, second.ast_digest);
    }

    #[test]
    fn syntax_errors_are_reported() {
        let err = TemplateProcessor::new()
            .process("index.html", "{% if user %}")
            .unwrap_err();

        assert!(matches!(err, TemplateError::Parse { .. }));
    }

    #[test]
    fn cache_key_is_stable_for_reordered_inputs() {
        let plan = TemplateProcessor::new()
            .process("index.html", "{{ loom.program('dashboard/load') }}")
            .unwrap();
        let manifest_a = Digest::blake3(b"program-a");
        let manifest_b = Digest::blake3(b"program-b");

        let mut first = TemplateCacheInput::from_plan(&plan, TemplateConsumer::App);
        first.metadata_digest = Some(Digest::blake3(b"app-meta"));
        first.grants_profile_digest = Some(Digest::blake3(b"grants"));
        first.program_bindings = vec![
            ProgramBinding {
                name: "b".to_string(),
                manifest_digest: manifest_b,
            },
            ProgramBinding {
                name: "a".to_string(),
                manifest_digest: manifest_a,
            },
        ];
        first.render_options = vec![
            RenderOption {
                key: "locale".to_string(),
                value: "en-US".to_string(),
            },
            RenderOption {
                key: "theme".to_string(),
                value: "light".to_string(),
            },
        ];

        let mut second = first.clone();
        second.program_bindings.reverse();
        second.render_options.reverse();

        assert_eq!(first.cache_key(), second.cache_key());
    }

    #[test]
    fn cache_key_changes_for_metadata_digest() {
        let plan = TemplateProcessor::new()
            .process("index.html", "{{ loom.program('dashboard/load') }}")
            .unwrap();
        let mut first = TemplateCacheInput::from_plan(&plan, TemplateConsumer::App);
        first.metadata_digest = Some(Digest::blake3(b"app-meta-a"));
        let mut second = TemplateCacheInput::from_plan(&plan, TemplateConsumer::App);
        second.metadata_digest = Some(Digest::blake3(b"app-meta-b"));

        assert_ne!(first.cache_key(), second.cache_key());
    }

    #[test]
    fn render_replaces_program_call_with_bound_output() {
        let bindings =
            TemplateBindings::default().with_program_output("dashboard/load", "<span>ready</span>");
        let rendered = TemplateProcessor::new()
            .render(
                "index.html",
                r#"<section>{{ loom.program(name="dashboard/load") }}</section>"#,
                &bindings,
            )
            .unwrap();

        assert_eq!(rendered.html, "<section><span>ready</span></section>");
        assert_eq!(rendered.plan.host_calls[0].target, "dashboard/load");
    }

    #[test]
    fn render_uses_empty_output_for_unbound_program() {
        let rendered = TemplateProcessor::new()
            .render(
                "index.html",
                r#"<section>{{ loom.program("dashboard/load") }}</section>"#,
                &TemplateBindings::default(),
            )
            .unwrap();

        assert_eq!(rendered.html, "<section></section>");
    }

    #[test]
    fn render_exposes_environment_workspaces() {
        let mut bindings = TemplateBindings::default();
        bindings.request.insert("path".into(), "apps".into());
        bindings.session.insert("principal".into(), "alice".into());
        let rendered = TemplateProcessor::new()
            .render(
                "index.html",
                "{{ request.path }} {{ session.principal }}{{ response.status }}{{ cookie.sid }}",
                &bindings,
            )
            .unwrap();

        assert_eq!(rendered.html, "apps alice");
    }

    #[test]
    fn render_exposes_loom_data_values() {
        let bindings = TemplateBindings::default().with_loom_value(
            "vcs",
            serde_json::json!({
                "workspace": "repo",
                "staged": 2
            }),
        );
        let rendered = TemplateProcessor::new()
            .render(
                "index.html",
                r#"<script>const data = {{ loom.vcs | tojson }};</script>"#,
                &bindings,
            )
            .unwrap();

        let json = rendered
            .html
            .strip_prefix("<script>const data = ")
            .and_then(|value| value.strip_suffix(";</script>"))
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
            serde_json::json!({
                "workspace": "repo",
                "staged": 2
            })
        );
    }

    #[test]
    fn render_exposes_meta_root() {
        let bindings = TemplateBindings::default().with_meta(serde_json::json!({
            "name": "VCS",
            "availableDisplayModes": ["inline", "fullscreen"],
            "visibility": ["model", "app"]
        }));
        let rendered = TemplateProcessor::new()
            .render(
                "index.html",
                r#"<script>const m = {{ meta.availableDisplayModes | tojson }};const n = "{{ meta.name }}";</script>"#,
                &bindings,
            )
            .unwrap();

        assert_eq!(
            rendered.html,
            r#"<script>const m = ["inline","fullscreen"];const n = "VCS";</script>"#
        );
    }
}
