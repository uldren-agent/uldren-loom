//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

//! The 0015 guard layer: read-only **CEL** predicates that gate a transition, graduated from
//! `prototypes/loom-compute/src/guard.rs` onto the real capability model. Behind the `guards` feature.
//!
//! A guard is a CEL (Common Expression Language) boolean expression evaluated against a
//! capability-scoped, read-only view. CEL is non-Turing-complete, so evaluation terminates
//! structurally (no fuel budget) and is side-effect-free. Determinism is enforced by construction: the
//! `cel-interpreter` dependency is built with `default-features = false`, so the `chrono`/`regex`
//! builtins are absent and a guard cannot read a wall clock, generate randomness, or perform host I/O.
//! Capability scoping is enforced by what is placed in the context - only granted-readable KV entries
//! are inserted - so a guard that reaches for ungranted state fails closed (the name does not resolve).
//!
//! Keys use the canonical guard authoring form [`guard_key`] (`{collection}/{rendered_key}`, with
//! reserved-prefix typed keys and percent-escaped text) and values use the lossless [`guard_value`]
//! rendering (valid UTF-8 as a string, invalid UTF-8 as `#bytes:<hex>` - never a lossy replacement).
//!
//! Context variables:
//! - `kv`        - map of granted-readable key-value entries (values rendered losslessly).
//! - `inputs`    - map of the declared transition inputs (string-valued).
//! - `ledger_ok` - bool: whether the ledger chain verifies (false without a Ledger read grant).
//!
//! The caller supplies the KV view, inputs, grants, and the verified-ledger flag; assembling those from
//! a live branch, and folding the guard expression into manifest identity, is the facade's job (the
//! remaining integration step tracked in the queue), kept separate so this layer stays pure and
//! deterministically testable.

use std::collections::BTreeMap;

use cel_interpreter::{Context, Program, Value as CelValue};
use loom_core::key_to_cbor;
use loom_core::tabular::Value;
use loom_core::{
    ACL_PREDICATE_LANGUAGE_CEL, AclEvaluationContext, AclPredicate, AclPredicateEvaluator,
    AclResourceScope, LoomError,
};

use crate::capability::{Capability, GrantSet, Mode};

/// When a guard is evaluated relative to the transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Precondition: evaluated against the base, before the program runs.
    Pre,
    /// Postcondition / invariant: evaluated against the proposed branch, after the program runs.
    Post,
}

/// A guard: a CEL boolean expression plus the phase it is checked in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Guard {
    pub phase: Phase,
    pub expr: String,
}

impl Guard {
    pub fn pre(expr: impl Into<String>) -> Self {
        Self {
            phase: Phase::Pre,
            expr: expr.into(),
        }
    }

    pub fn post(expr: impl Into<String>) -> Self {
        Self {
            phase: Phase::Post,
            expr: expr.into(),
        }
    }
}

/// Why a guard did not evaluate to a clean `true`.
#[derive(Clone, Debug)]
pub enum GuardError {
    /// The CEL expression failed to compile.
    Compile(String),
    /// The CEL expression failed to evaluate - e.g. it referenced a name not in the granted context
    /// (a guard that reaches for ungranted state fails closed this way).
    Eval(String),
    /// The expression did not evaluate to a boolean.
    NotBoolean,
}

/// A capability-scoped, read-only view of the state a guard may observe: granted-readable KV entries
/// (raw value bytes) plus the declared inputs, both keyed by string.
pub type StateView = BTreeMap<String, Vec<u8>>;

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Percent-escape the reserved authoring characters in a text segment (collection name or text key) so
/// no segment can forge a path boundary (`/`), an escape marker (`%`), a typed-key prefix (`#`), or a
/// control character. Non-ASCII characters pass through unchanged (they can never form a reserved byte),
/// so the escaping is minimal and human-readable while collision-safe.
fn escape_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '/' | '%' | '#' => out.push_str(&format!("%{:02X}", ch as u32)),
            c if (c as u32) < 0x20 || c as u32 == 0x7F => {
                out.push_str(&format!("%{:02X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out
}

/// Render a typed KV key into the guard authoring form. The common authorable types get a
/// human-readable, reserved-prefixed rendering; every other typed key falls back to the canonical,
/// injective `#cbor:<hex>` of its `key_to_cbor` bytes, so no two distinct keys can ever alias (for
/// example `U8(1)` and `U64(1)` render to different `#cbor:` forms, not a shared `#uint:1`).
fn render_key(key: &Value) -> String {
    match key {
        Value::Text(s) => escape_segment(s),
        Value::Int(i) => format!("#int:{i}"),
        Value::Bool(b) => format!("#bool:{b}"),
        Value::Null => "#null".to_string(),
        Value::Bytes(b) => format!("#bytes:{}", hex_lower(b)),
        other => format!("#cbor:{}", hex_lower(&key_to_cbor(other))),
    }
}

/// The guard authoring name for a `(collection, typed key)` pair: `{escaped_collection}/{rendered_key}`.
/// The single unescaped `/` is the collection/key boundary; segments cannot contain one. This is an
/// authoring view for guard expressions - the authorization target stays the canonical
/// `collection + hex(key_to_cbor(key))` form the PEP checks, which this does not replace.
pub fn guard_key(collection: &str, key: &Value) -> String {
    format!("{}/{}", escape_segment(collection), render_key(key))
}

/// Render a KV value for the guard context losslessly: valid UTF-8 becomes the string it encodes;
/// invalid UTF-8 becomes a reserved `#bytes:<hex>` form. Never a lossy replacement character, so a guard
/// can distinguish real text from binary rather than silently comparing corrupted input.
pub fn guard_value(value: &[u8]) -> String {
    match std::str::from_utf8(value) {
        Ok(s) => s.to_string(),
        Err(_) => format!("#bytes:{}", hex_lower(value)),
    }
}

/// Evaluate the CEL `expr` against a capability-scoped view of `kv` and `inputs`. Only granted-readable
/// KV entries are placed in the context, so an ungranted reference fails to resolve and the guard fails
/// closed. `ledger_verified` is the caller's chain-verification result; it is exposed as `ledger_ok`
/// only when the grants permit a Ledger read (otherwise `false`, fail closed).
pub fn evaluate(
    expr: &str,
    kv: &StateView,
    inputs: &StateView,
    grants: &GrantSet,
    ledger_verified: bool,
) -> Result<bool, GuardError> {
    let kv_ctx: BTreeMap<String, String> = kv
        .iter()
        .filter(|(k, _)| grants.permits(Capability::Kv, Mode::Read, k))
        .map(|(k, v)| (k.clone(), guard_value(v)))
        .collect();
    let input_ctx: BTreeMap<String, String> = inputs
        .iter()
        .map(|(k, v)| (k.clone(), guard_value(v)))
        .collect();
    let ledger_ok = ledger_verified && grants.permits(Capability::Ledger, Mode::Read, "ledger");
    eval_cel(expr, kv_ctx, input_ctx, ledger_ok)
}

/// Evaluate a guard against an already grant-scoped, [`guard_key`]-rendered view (as produced by
/// [`guard_view_from_collection`]); no further grant filtering is applied here, since scoping was done
/// when the view was built. This is the facade enforcement path: the caller assembles the base view
/// (for `Pre` guards) or the proposed view (for `Post`), and passes the already-resolved `ledger_ok`.
pub fn evaluate_view(
    expr: &str,
    kv: &StateView,
    inputs: &StateView,
    ledger_ok: bool,
) -> Result<bool, GuardError> {
    let kv_ctx: BTreeMap<String, String> = kv
        .iter()
        .map(|(k, v)| (k.clone(), guard_value(v)))
        .collect();
    let input_ctx: BTreeMap<String, String> = inputs
        .iter()
        .map(|(k, v)| (k.clone(), guard_value(v)))
        .collect();
    eval_cel(expr, kv_ctx, input_ctx, ledger_ok)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CelAclPredicateEvaluator;

impl AclPredicateEvaluator for CelAclPredicateEvaluator {
    fn evaluate(
        &self,
        predicate: &AclPredicate,
        context: &AclEvaluationContext<'_>,
    ) -> loom_core::Result<bool> {
        if predicate.language != ACL_PREDICATE_LANGUAGE_CEL {
            return Err(LoomError::invalid("acl predicate language must be cel"));
        }
        evaluate_acl_predicate(&predicate.expression, context).map_err(acl_eval_error)
    }
}

pub fn evaluate_acl_predicate(
    expr: &str,
    context: &AclEvaluationContext<'_>,
) -> Result<bool, GuardError> {
    let program = Program::compile(expr).map_err(|e| GuardError::Compile(format!("{e}")))?;
    let mut ctx = Context::default();
    let roles: Vec<String> = context.roles.iter().map(ToString::to_string).collect();
    let (scope_kind, scope_hex, scope_text) = match context.resource.scope {
        AclResourceScope::All => ("all".to_string(), String::new(), String::new()),
        AclResourceScope::Prefix { kind, value } => (
            format!("{kind:?}").to_ascii_lowercase(),
            hex_lower(value),
            std::str::from_utf8(value).unwrap_or("").to_string(),
        ),
    };
    ctx.add_variable("principal", context.principal.to_string())
        .map_err(|e| GuardError::Eval(format!("principal: {e}")))?;
    ctx.add_variable("roles", roles)
        .map_err(|e| GuardError::Eval(format!("roles: {e}")))?;
    ctx.add_variable("workspace", context.resource.workspace.to_string())
        .map_err(|e| GuardError::Eval(format!("workspace: {e}")))?;
    ctx.add_variable("domain", context.resource.domain.as_str())
        .map_err(|e| GuardError::Eval(format!("facet: {e}")))?;
    ctx.add_variable("right", format!("{:?}", context.right).to_ascii_lowercase())
        .map_err(|e| GuardError::Eval(format!("right: {e}")))?;
    ctx.add_variable("ref", context.resource.ref_name.unwrap_or(""))
        .map_err(|e| GuardError::Eval(format!("ref: {e}")))?;
    ctx.add_variable("scope_kind", scope_kind)
        .map_err(|e| GuardError::Eval(format!("scope_kind: {e}")))?;
    ctx.add_variable("scope_hex", scope_hex)
        .map_err(|e| GuardError::Eval(format!("scope_hex: {e}")))?;
    ctx.add_variable("scope_text", scope_text)
        .map_err(|e| GuardError::Eval(format!("scope_text: {e}")))?;
    match program.execute(&ctx) {
        Ok(CelValue::Bool(b)) => Ok(b),
        Ok(_) => Err(GuardError::NotBoolean),
        Err(e) => Err(GuardError::Eval(format!("{e}"))),
    }
}

fn acl_eval_error(err: GuardError) -> LoomError {
    LoomError::new(
        loom_core::Code::PermissionDenied,
        format!("acl predicate denied: {err:?}"),
    )
}

/// Compile and run a CEL boolean expression against the assembled context.
fn eval_cel(
    expr: &str,
    kv_ctx: BTreeMap<String, String>,
    input_ctx: BTreeMap<String, String>,
    ledger_ok: bool,
) -> Result<bool, GuardError> {
    let program = Program::compile(expr).map_err(|e| GuardError::Compile(format!("{e}")))?;
    let mut ctx = Context::default();
    ctx.add_variable("kv", kv_ctx)
        .map_err(|e| GuardError::Eval(format!("kv: {e}")))?;
    ctx.add_variable("inputs", input_ctx)
        .map_err(|e| GuardError::Eval(format!("inputs: {e}")))?;
    ctx.add_variable("ledger_ok", ledger_ok)
        .map_err(|e| GuardError::Eval(format!("ledger_ok: {e}")))?;
    match program.execute(&ctx) {
        Ok(CelValue::Bool(b)) => Ok(b),
        Ok(_) => Err(GuardError::NotBoolean),
        Err(e) => Err(GuardError::Eval(format!("{e}"))),
    }
}

/// Build the grant-scoped, [`guard_key`]-rendered guard view for one KV collection: include an entry
/// only when the manifest's `Kv` read grant permits it (checked against the canonical
/// `collection/hex(key_to_cbor(key))` PEP target the ACL uses - the guard renderer is only an authoring
/// view, never the authorization target), keyed by the guard authoring name and keeping raw value bytes
/// for lossless [`guard_value`] rendering at evaluation time. Visibility therefore equals the program's
/// read authority: ungranted entries never enter the guard context, so guards fail closed.
pub fn guard_view_from_collection(
    grants: &GrantSet,
    collection: &str,
    entries: &[(Value, Vec<u8>)],
) -> StateView {
    let mut view = StateView::new();
    for (key, value) in entries {
        let pep_target = format!("{collection}/{}", hex_lower(&key_to_cbor(key)));
        if grants.permits(Capability::Kv, Mode::Read, &pep_target) {
            view.insert(guard_key(collection, key), value.clone());
        }
    }
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{Grant, Scope};

    fn view(pairs: &[(&str, &[u8])]) -> StateView {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.to_vec()))
            .collect()
    }

    fn kv_read_all() -> GrantSet {
        GrantSet::new(vec![Grant {
            facet: Capability::Kv,
            mode: Mode::Read,
            scopes: vec![Scope::All],
        }])
    }

    #[test]
    fn text_key_cannot_forge_a_typed_prefix() {
        // A text key that looks like the integer tag must not collide with the actual integer key.
        assert_ne!(
            guard_key("c", &Value::Text("#int:1".to_string())),
            guard_key("c", &Value::Int(1))
        );
        assert_eq!(guard_key("c", &Value::Int(1)), "c/#int:1");
        assert_eq!(
            guard_key("c", &Value::Text("#int:1".to_string())),
            "c/%23int:1"
        );
    }

    #[test]
    fn text_key_cannot_forge_a_collection_boundary() {
        // A text key containing '/' must be escaped so it cannot masquerade as a nested boundary.
        assert_eq!(render_key(&Value::Text("a/b".to_string())), "a%2Fb");
        assert_eq!(guard_key("c", &Value::Text("a/b".to_string())), "c/a%2Fb");
        // The only unescaped '/' is the single collection/key boundary.
        assert_eq!(
            guard_key("c", &Value::Text("a/b".to_string()))
                .matches('/')
                .count(),
            1
        );
    }

    #[test]
    fn bytes_key_is_stable_lowercase_hex() {
        assert_eq!(
            guard_key("c", &Value::Bytes(vec![0xAB, 0x01, 0xff])),
            "c/#bytes:ab01ff"
        );
    }

    #[test]
    fn distinct_integer_widths_do_not_alias() {
        // U8(1) and U64(1) are distinct keys; the #cbor: fallback keeps them distinct.
        assert_ne!(
            guard_key("c", &Value::U8(1)),
            guard_key("c", &Value::U64(1))
        );
    }

    #[test]
    fn guard_value_is_lossless_for_invalid_utf8() {
        assert_eq!(guard_value(b"ready"), "ready");
        // Invalid UTF-8 becomes a reserved hex form, never a lossy replacement character.
        let rendered = guard_value(&[0xff, 0xfe]);
        assert_eq!(rendered, "#bytes:fffe");
        assert!(!rendered.contains('\u{FFFD}'));
    }

    #[test]
    fn guard_view_scopes_by_grant_and_renders_keys() {
        // Read granted only under the "cache/" PEP-target prefix.
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Kv,
            mode: Mode::Read,
            scopes: vec![Scope::Prefix("cache/".into())],
        }]);
        let entries = vec![
            (Value::Text("greeting".to_string()), b"hi".to_vec()),
            (Value::Int(7), b"seven".to_vec()),
        ];
        let view = guard_view_from_collection(&grants, "cache", &entries);
        // Keyed by the authoring form; both entries are under "cache/" so both are granted.
        assert_eq!(
            view.get("cache/greeting").map(|v| v.as_slice()),
            Some(b"hi".as_slice())
        );
        assert_eq!(
            view.get("cache/#int:7").map(|v| v.as_slice()),
            Some(b"seven".as_slice())
        );
        // A different collection is outside the "cache/" grant prefix -> excluded (fail closed).
        let other = guard_view_from_collection(
            &grants,
            "secrets",
            &[(Value::Text("k".to_string()), b"v".to_vec())],
        );
        assert!(other.is_empty());
    }

    #[test]
    fn evaluate_view_reads_the_prebuilt_scoped_view() {
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Kv,
            mode: Mode::Read,
            scopes: vec![Scope::All],
        }]);
        let view = guard_view_from_collection(
            &grants,
            "doc",
            &[(Value::Text("state".to_string()), b"ready".to_vec())],
        );
        assert!(
            evaluate_view(
                r#"kv["doc/state"] == "ready""#,
                &view,
                &StateView::new(),
                false
            )
            .unwrap()
        );
        assert!(
            !evaluate_view(
                r#"kv["doc/state"] == "draft""#,
                &view,
                &StateView::new(),
                false
            )
            .unwrap()
        );
    }

    #[test]
    fn cel_reads_state_and_inputs() {
        let kv = view(&[("greeting", b"hi")]);
        let inputs = view(&[("actor", b"alice")]);
        let grants = kv_read_all();
        assert!(
            evaluate(
                r#"kv.greeting == "hi" && inputs.actor == "alice""#,
                &kv,
                &inputs,
                &grants,
                false
            )
            .unwrap()
        );
        assert!(!evaluate(r#"kv.greeting == "bye""#, &kv, &inputs, &grants, false).unwrap());
        assert!(
            evaluate(
                r#""greeting" in kv && !("missing" in kv)"#,
                &kv,
                &inputs,
                &grants,
                false
            )
            .unwrap()
        );
    }

    #[test]
    fn ungranted_state_is_not_in_the_context() {
        let kv = view(&[("secret", b"x")]);
        // Read granted only under "public:"; "secret" is therefore absent from the context.
        let grants = GrantSet::new(vec![Grant {
            facet: Capability::Kv,
            mode: Mode::Read,
            scopes: vec![Scope::Prefix("public:".into())],
        }]);
        // Membership sees it as absent (fail closed, no leak).
        assert!(!evaluate(r#""secret" in kv"#, &kv, &StateView::new(), &grants, false).unwrap());
        // Direct access on the absent key is an evaluation error (also fail closed).
        assert!(matches!(
            evaluate(
                r#"kv.secret == "x""#,
                &kv,
                &StateView::new(),
                &grants,
                false
            ),
            Err(GuardError::Eval(_))
        ));
    }

    #[test]
    fn ledger_ok_requires_a_ledger_read_grant() {
        let granted = GrantSet::new(vec![Grant {
            facet: Capability::Ledger,
            mode: Mode::Read,
            scopes: vec![Scope::All],
        }]);
        assert!(
            evaluate(
                "ledger_ok",
                &StateView::new(),
                &StateView::new(),
                &granted,
                true
            )
            .unwrap()
        );
        // Verified chain but no Ledger read grant -> ledger_ok is false (fail closed).
        assert!(
            !evaluate(
                "ledger_ok",
                &StateView::new(),
                &StateView::new(),
                &GrantSet::new(vec![]),
                true
            )
            .unwrap()
        );
    }

    #[test]
    fn non_boolean_and_bad_expression_are_errors() {
        let kv = view(&[("greeting", b"hi")]);
        assert!(matches!(
            evaluate("kv.greeting", &kv, &StateView::new(), &kv_read_all(), false),
            Err(GuardError::NotBoolean)
        ));
        assert!(matches!(
            evaluate(
                "this is not cel",
                &kv,
                &StateView::new(),
                &kv_read_all(),
                false
            ),
            Err(GuardError::Compile(_))
        ));
    }

    #[test]
    fn acl_cel_predicate_reads_stable_authorization_context() {
        use loom_core::{
            AclEvaluationContext, AclPredicate, AclPredicateEvaluator, AclResource,
            AclResourceScope, AclRight, AclScopeKind, FacetKind, WorkspaceId,
        };
        use std::collections::BTreeSet;

        let roles = BTreeSet::from([WorkspaceId::from_bytes([4; 16])]);
        let resource = AclResource::scoped(
            WorkspaceId::from_bytes([9; 16]),
            FacetKind::Files,
            Some("branch/main"),
            AclResourceScope::Prefix {
                kind: AclScopeKind::Path,
                value: b"reports/q1.txt",
            },
        );
        let context = AclEvaluationContext {
            principal: WorkspaceId::from_bytes([1; 16]),
            roles: &roles,
            resource,
            right: AclRight::Read,
        };
        let predicate = AclPredicate::cel(
            r#"principal == "01010101-0101-0101-0101-010101010101" &&
               workspace == "09090909-0909-0909-0909-090909090909" &&
               domain == "files" &&
               right == "read" &&
               ref == "branch/main" &&
               scope_kind == "path" &&
               scope_text == "reports/q1.txt" &&
               "04040404-0404-0404-0404-040404040404" in roles"#,
        )
        .unwrap();

        assert!(
            CelAclPredicateEvaluator
                .evaluate(&predicate, &context)
                .unwrap()
        );
    }

    #[test]
    fn acl_cel_predicate_errors_fail_closed() {
        use loom_core::{
            AclEvaluationContext, AclPredicate, AclPredicateEvaluator, AclResource, AclRight,
            FacetKind, WorkspaceId,
        };
        use std::collections::BTreeSet;

        let roles = BTreeSet::new();
        let context = AclEvaluationContext {
            principal: WorkspaceId::from_bytes([1; 16]),
            roles: &roles,
            resource: AclResource::all(WorkspaceId::from_bytes([9; 16]), FacetKind::Files),
            right: AclRight::Read,
        };
        let predicate = AclPredicate::cel("missing.name == true").unwrap();

        assert_eq!(
            CelAclPredicateEvaluator
                .evaluate(&predicate, &context)
                .unwrap_err()
                .code,
            loom_core::Code::PermissionDenied
        );
    }
}
