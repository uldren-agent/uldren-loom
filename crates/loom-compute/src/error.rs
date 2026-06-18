//! Errors raised by the execution engine.

use loom_core::{Code, LoomError};

/// An error from compiling, running, or gating a program.
#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    /// Program module or runtime failure (invalid module, link failure, or a guest trap, including a
    /// malformed-key trap from the KV host ABI).
    #[error("program error: {0}")]
    Program(String),
    /// Execution exhausted its fuel budget.
    #[error("metering budget exceeded ({budget} fuel units)")]
    BudgetExceeded {
        /// The fuel budget that was exhausted.
        budget: u64,
    },
    /// A program operation failed the manifest-grant check (or named a non-grantable facet). An ACL
    /// denial surfaces instead as [`ExecError::Core`] carrying `Code::PermissionDenied`.
    #[error("execution denied: {0}")]
    Denied(String),
    /// Underlying Loom engine failure.
    #[error(transparent)]
    Core(#[from] LoomError),
}

impl ExecError {
    /// The stable [`Code`] this error maps to at a public boundary. Both a manifest denial
    /// ([`ExecError::Denied`]) and an ACL denial (a [`ExecError::Core`] carrying
    /// `Code::PermissionDenied`) normalize to `Code::PermissionDenied`; the originating variant and its
    /// message remain available on the `ExecError` itself for audit and debugging.
    ///
    /// `Program` maps to `InvalidArgument` (the submitted program is invalid or trapped);
    /// `BudgetExceeded` maps to `ResourceExhausted`.
    pub fn code(&self) -> Code {
        match self {
            ExecError::Program(_) => Code::InvalidArgument,
            ExecError::BudgetExceeded { .. } => Code::ResourceExhausted,
            ExecError::Denied(_) => Code::PermissionDenied,
            ExecError::Core(err) => err.code,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denials_normalize_to_permission_denied() {
        // Manifest-grant denial.
        assert_eq!(
            ExecError::Denied("nope".to_string()).code(),
            Code::PermissionDenied
        );
        // ACL denial arrives as a Core LoomError already carrying PermissionDenied.
        let acl = ExecError::Core(LoomError::new(Code::PermissionDenied, "acl denied"));
        assert_eq!(acl.code(), Code::PermissionDenied);
    }

    #[test]
    fn other_variants_map_stably() {
        assert_eq!(
            ExecError::Program("trap".to_string()).code(),
            Code::InvalidArgument
        );
        assert_eq!(
            ExecError::BudgetExceeded { budget: 10 }.code(),
            Code::ResourceExhausted
        );
        // A Core error preserves its underlying code.
        assert_eq!(
            ExecError::Core(LoomError::new(Code::NotFound, "x")).code(),
            Code::NotFound
        );
    }
}
