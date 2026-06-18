use loom_core::error::Result;
use loom_core::provider::ObjectStore;
use loom_core::workspace::WorkspaceId;
use loom_core::{AclRight, Loom};

use crate::Mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionOperation {
    Lookup,
    Getattr,
    Readdir,
    Read,
    Readlink,
    MetadataRead,
    Write,
    Create,
    Mkdir,
    Unlink,
    Rmdir,
    Rename,
    Truncate,
    Symlink,
    FlushOverlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProjectionPolicy;

impl ProjectionPolicy {
    pub fn authorize<S: ObjectStore>(
        &self,
        loom: &Loom<S>,
        ns: WorkspaceId,
        mode: Mode,
        op: ProjectionOperation,
        path: &str,
    ) -> Result<()> {
        self.check_mode(mode, op)?;
        loom.authorize_file_path(ns, path, op.right())
    }

    pub fn authorize_rename<S: ObjectStore>(
        &self,
        loom: &Loom<S>,
        ns: WorkspaceId,
        mode: Mode,
        src: &str,
        dst: &str,
    ) -> Result<()> {
        self.check_mode(mode, ProjectionOperation::Rename)?;
        loom.authorize_file_path(ns, src, AclRight::Write)?;
        loom.authorize_file_path(ns, dst, AclRight::Write)
    }

    fn check_mode(&self, mode: Mode, op: ProjectionOperation) -> Result<()> {
        if op.is_mutating() {
            mode.check_writable()
        } else {
            Ok(())
        }
    }
}

impl ProjectionOperation {
    fn right(self) -> AclRight {
        match self {
            ProjectionOperation::Lookup
            | ProjectionOperation::Getattr
            | ProjectionOperation::Readdir
            | ProjectionOperation::Read
            | ProjectionOperation::Readlink
            | ProjectionOperation::MetadataRead => AclRight::Read,
            ProjectionOperation::Write
            | ProjectionOperation::Create
            | ProjectionOperation::Mkdir
            | ProjectionOperation::Unlink
            | ProjectionOperation::Rmdir
            | ProjectionOperation::Rename
            | ProjectionOperation::Truncate
            | ProjectionOperation::Symlink
            | ProjectionOperation::FlushOverlay => AclRight::Write,
        }
    }

    fn is_mutating(self) -> bool {
        matches!(
            self,
            ProjectionOperation::Write
                | ProjectionOperation::Create
                | ProjectionOperation::Mkdir
                | ProjectionOperation::Unlink
                | ProjectionOperation::Rmdir
                | ProjectionOperation::Rename
                | ProjectionOperation::Truncate
                | ProjectionOperation::Symlink
                | ProjectionOperation::FlushOverlay
        )
    }
}
