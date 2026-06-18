use loom_core::Loom;
use loom_core::error::Result;
use loom_core::provider::ObjectStore;
use loom_core::workspace::WorkspaceId;

use crate::metadata::ProjectionMetadata;
use crate::overlay::{self, Facet, FacetFile, WriteOutcome};

/// Backend-neutral facet projection behavior.
pub trait ProjectionFacet {
    fn classify(&self, path: &str) -> Option<FacetFile>;
    fn classify_collection(&self, path: &str) -> Option<(Facet, String, String)>;
    fn ensure_collection<S: ObjectStore>(
        &self,
        loom: &mut Loom<S>,
        ns: WorkspaceId,
        facet: Facet,
        principal: &str,
        collection: &str,
    ) -> Result<()>;
    fn list_projected<S: ObjectStore>(
        &self,
        loom: &Loom<S>,
        ns: WorkspaceId,
        facet: Facet,
        principal: &str,
        collection: &str,
    ) -> Result<Vec<String>>;
    fn project<S: ObjectStore>(
        &self,
        loom: &Loom<S>,
        ns: WorkspaceId,
        file: &FacetFile,
    ) -> Result<Option<Vec<u8>>>;
    fn ingest<S: ObjectStore>(
        &self,
        loom: &mut Loom<S>,
        ns: WorkspaceId,
        file: &FacetFile,
        bytes: &[u8],
    ) -> Result<WriteOutcome>;
    fn delete_record<S: ObjectStore>(
        &self,
        loom: &mut Loom<S>,
        ns: WorkspaceId,
        file: &FacetFile,
    ) -> Result<bool>;
    fn metadata<S: ObjectStore>(
        &self,
        loom: &Loom<S>,
        ns: WorkspaceId,
        file: &FacetFile,
    ) -> Result<ProjectionMetadata>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BuiltInFacetProjection;

impl ProjectionFacet for BuiltInFacetProjection {
    fn classify(&self, path: &str) -> Option<FacetFile> {
        overlay::classify(path)
    }

    fn classify_collection(&self, path: &str) -> Option<(Facet, String, String)> {
        overlay::classify_collection(path)
    }

    fn ensure_collection<S: ObjectStore>(
        &self,
        loom: &mut Loom<S>,
        ns: WorkspaceId,
        facet: Facet,
        principal: &str,
        collection: &str,
    ) -> Result<()> {
        overlay::ensure_collection(loom, ns, facet, principal, collection)
    }

    fn list_projected<S: ObjectStore>(
        &self,
        loom: &Loom<S>,
        ns: WorkspaceId,
        facet: Facet,
        principal: &str,
        collection: &str,
    ) -> Result<Vec<String>> {
        overlay::list_projected(loom, ns, facet, principal, collection)
    }

    fn project<S: ObjectStore>(
        &self,
        loom: &Loom<S>,
        ns: WorkspaceId,
        file: &FacetFile,
    ) -> Result<Option<Vec<u8>>> {
        overlay::project(loom, ns, file)
    }

    fn ingest<S: ObjectStore>(
        &self,
        loom: &mut Loom<S>,
        ns: WorkspaceId,
        file: &FacetFile,
        bytes: &[u8],
    ) -> Result<WriteOutcome> {
        overlay::ingest(loom, ns, file, bytes)
    }

    fn delete_record<S: ObjectStore>(
        &self,
        loom: &mut Loom<S>,
        ns: WorkspaceId,
        file: &FacetFile,
    ) -> Result<bool> {
        overlay::delete(loom, ns, file)
    }

    fn metadata<S: ObjectStore>(
        &self,
        loom: &Loom<S>,
        ns: WorkspaceId,
        file: &FacetFile,
    ) -> Result<ProjectionMetadata> {
        overlay::processing(loom, ns, file).map(ProjectionMetadata::from_processing)
    }
}
