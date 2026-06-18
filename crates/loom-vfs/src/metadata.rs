use crate::overlay;

/// Backend-neutral metadata for projected files.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectionMetadata {
    pub status: ProjectionStatus,
    pub error: Option<String>,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectionStatus {
    #[default]
    Unknown,
    Ok,
    Quarantined,
}

impl ProjectionMetadata {
    pub fn from_processing(processing: overlay::Processing) -> Self {
        let status = match processing.status.as_str() {
            "ok" => ProjectionStatus::Ok,
            "quarantined" => ProjectionStatus::Quarantined,
            _ => ProjectionStatus::Unknown,
        };
        Self {
            status,
            error: processing.error,
            etag: processing.etag,
        }
    }

    pub fn xattrs(&self) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        if let Some(status) = self.status.as_str() {
            out.push(("user.loom.status".to_string(), status.as_bytes().to_vec()));
        }
        if let Some(error) = &self.error {
            out.push(("user.loom.error".to_string(), error.as_bytes().to_vec()));
        }
        if let Some(etag) = &self.etag {
            out.push(("user.loom.etag".to_string(), etag.as_bytes().to_vec()));
        }
        out
    }
}

impl ProjectionStatus {
    fn as_str(self) -> Option<&'static str> {
        match self {
            ProjectionStatus::Unknown => None,
            ProjectionStatus::Ok => Some("ok"),
            ProjectionStatus::Quarantined => Some("quarantined"),
        }
    }
}
