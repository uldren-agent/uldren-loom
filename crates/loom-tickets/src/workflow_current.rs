use loom_codec::Value;
use loom_core::digest::{Algo, DIGEST_LEN, Digest};
use loom_core::error::{LoomError, Result};
use loom_core::{
    MutableOverlay, ObjectStore, OverlayEntryKind, OverlayKey, OverlayKeyPrefix, OverlayOwnerToken,
    OverlayReadSnapshot, SecondaryIndexWrite, SecondaryIndexWriteOp,
};

const WORKFLOW_CURRENT_SCHEMA: &str = "loom.workflow.current-record.v1";
const WORKSPACE_SCOPE: &[u8] = b"workspace";
const TICKETS_DOMAIN: &[u8] = b"tickets";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkflowCurrentRecordKind {
    Project,
    ProjectContract,
    Ticket,
    Comment,
    Relation,
    Board,
    Lane,
    ActiveAssignment,
}

impl WorkflowCurrentRecordKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::ProjectContract => "project-contract",
            Self::Ticket => "ticket",
            Self::Comment => "comment",
            Self::Relation => "relation",
            Self::Board => "board",
            Self::Lane => "lane",
            Self::ActiveAssignment => "active-assignment",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowCurrentRecord {
    pub workspace_id: String,
    pub project_id: String,
    pub kind: WorkflowCurrentRecordKind,
    pub record_id: String,
    pub payload: Vec<u8>,
    pub operation_root: Option<Digest>,
}

impl WorkflowCurrentRecord {
    pub fn new(
        workspace_id: impl Into<String>,
        project_id: impl Into<String>,
        kind: WorkflowCurrentRecordKind,
        record_id: impl Into<String>,
        payload: impl Into<Vec<u8>>,
        operation_root: Option<Digest>,
    ) -> Result<Self> {
        let record = Self {
            workspace_id: workspace_id.into(),
            project_id: project_id.into(),
            kind,
            record_id: record_id.into(),
            payload: payload.into(),
            operation_root,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn overlay_key(&self) -> Result<OverlayKey> {
        workflow_current_key(
            &self.workspace_id,
            &self.project_id,
            self.kind,
            &self.record_id,
        )
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let operation_root = self
            .operation_root
            .map(|digest| Value::Bytes(digest.bytes().to_vec()))
            .unwrap_or(Value::Null);
        loom_codec::encode(&Value::Array(vec![
            Value::Text(WORKFLOW_CURRENT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Text(self.project_id.clone()),
                Value::Text(self.kind.as_str().to_string()),
                Value::Text(self.record_id.clone()),
                Value::Bytes(self.payload.clone()),
                operation_root,
            ]),
        ]))
        .map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let value = loom_codec::decode(bytes).map_err(codec_error)?;
        let Value::Array(mut outer) = value else {
            return Err(LoomError::corrupt(
                "workflow current record must be an array",
            ));
        };
        if outer.len() != 2 {
            return Err(LoomError::corrupt("workflow current record shape mismatch"));
        }
        if outer.remove(0) != Value::Text(WORKFLOW_CURRENT_SCHEMA.to_string()) {
            return Err(LoomError::corrupt(
                "workflow current record schema mismatch",
            ));
        }
        let Value::Array(fields) = outer.remove(0) else {
            return Err(LoomError::corrupt(
                "workflow current record fields mismatch",
            ));
        };
        let [workspace, project, kind, record_id, payload, operation_root] = fields
            .try_into()
            .map_err(|_| LoomError::corrupt("workflow current record field count mismatch"))?;
        let Value::Text(workspace_id) = workspace else {
            return Err(LoomError::corrupt("workflow current workspace mismatch"));
        };
        let Value::Text(project_id) = project else {
            return Err(LoomError::corrupt("workflow current project mismatch"));
        };
        let Value::Text(kind) = kind else {
            return Err(LoomError::corrupt("workflow current kind mismatch"));
        };
        let Value::Text(record_id) = record_id else {
            return Err(LoomError::corrupt("workflow current id mismatch"));
        };
        let Value::Bytes(payload) = payload else {
            return Err(LoomError::corrupt("workflow current payload mismatch"));
        };
        let operation_root = match operation_root {
            Value::Null => None,
            Value::Bytes(bytes) => {
                let bytes: [u8; DIGEST_LEN] = bytes.as_slice().try_into().map_err(|_| {
                    LoomError::corrupt("workflow current operation root length mismatch")
                })?;
                Some(Digest::of(Algo::Blake3, bytes))
            }
            _ => {
                return Err(LoomError::corrupt(
                    "workflow current operation root mismatch",
                ));
            }
        };
        Self::new(
            workspace_id,
            project_id,
            parse_kind(&kind)?,
            record_id,
            payload,
            operation_root,
        )
    }

    fn validate(&self) -> Result<()> {
        validate_segment("workspace_id", &self.workspace_id)?;
        validate_segment("project_id", &self.project_id)?;
        validate_segment("record_id", &self.record_id)?;
        Ok(())
    }
}

pub fn workflow_current_key(
    workspace_id: &str,
    project_id: &str,
    kind: WorkflowCurrentRecordKind,
    record_id: &str,
) -> Result<OverlayKey> {
    validate_segment("workspace_id", workspace_id)?;
    validate_segment("project_id", project_id)?;
    validate_segment("record_id", record_id)?;
    OverlayKey::from_segments([
        WORKSPACE_SCOPE,
        workspace_id.as_bytes(),
        TICKETS_DOMAIN,
        project_id.as_bytes(),
        kind.as_str().as_bytes(),
        record_id.as_bytes(),
    ])
}

pub fn workflow_current_key_prefix(
    workspace_id: &str,
    project_id: &str,
    kind: WorkflowCurrentRecordKind,
) -> Result<OverlayKeyPrefix> {
    validate_segment("workspace_id", workspace_id)?;
    validate_segment("project_id", project_id)?;
    OverlayKey::prefix_from_segments(
        6,
        [
            WORKSPACE_SCOPE,
            workspace_id.as_bytes(),
            TICKETS_DOMAIN,
            project_id.as_bytes(),
            kind.as_str().as_bytes(),
        ],
    )
}

pub fn list_workflow_current_records_by_prefix(
    store: &impl ObjectStore,
    workspace_id: &str,
    project_id: &str,
    kind: WorkflowCurrentRecordKind,
) -> Result<Vec<WorkflowCurrentRecord>> {
    let prefix = workflow_current_key_prefix(workspace_id, project_id, kind)?;
    let mut records = Vec::new();
    for entry in store.mutable_overlay_current_entries_with_prefix(&prefix)? {
        if entry.kind == OverlayEntryKind::Tombstone {
            continue;
        }
        let record = WorkflowCurrentRecord::decode(&entry.payload)?;
        let key = record.overlay_key()?;
        if key != entry.key
            || record.workspace_id != workspace_id
            || record.project_id != project_id
            || record.kind != kind
        {
            return Err(LoomError::corrupt("workflow current record key mismatch"));
        }
        records.push(record);
    }
    Ok(records)
}

pub fn workflow_current_secondary_index_key(
    workspace_id: &str,
    project_id: &str,
    index_name: &str,
    index_value: &str,
    record_id: &str,
) -> Result<OverlayKey> {
    validate_segment("workspace_id", workspace_id)?;
    validate_segment("project_id", project_id)?;
    validate_segment("index_name", index_name)?;
    validate_segment("index_value", index_value)?;
    validate_segment("record_id", record_id)?;
    let index = format!("{index_name}/{index_value}");
    OverlayKey::from_segments([
        WORKSPACE_SCOPE,
        workspace_id.as_bytes(),
        TICKETS_DOMAIN,
        project_id.as_bytes(),
        index.as_bytes(),
        record_id.as_bytes(),
    ])
}

pub fn workflow_current_secondary_index_put(
    workspace_id: &str,
    project_id: &str,
    index_name: &str,
    index_value: &str,
    record_id: &str,
    payload: impl Into<Vec<u8>>,
) -> Result<SecondaryIndexWrite> {
    Ok(SecondaryIndexWrite {
        index: workflow_current_secondary_index_key(
            workspace_id,
            project_id,
            index_name,
            index_value,
            record_id,
        )?,
        op: SecondaryIndexWriteOp::Put {
            payload: payload.into(),
        },
    })
}

pub fn workflow_current_secondary_index_delete(
    workspace_id: &str,
    project_id: &str,
    index_name: &str,
    index_value: &str,
    record_id: &str,
) -> Result<SecondaryIndexWrite> {
    Ok(SecondaryIndexWrite {
        index: workflow_current_secondary_index_key(
            workspace_id,
            project_id,
            index_name,
            index_value,
            record_id,
        )?,
        op: SecondaryIndexWriteOp::Delete,
    })
}

pub fn put_workflow_current_record(
    overlay: &mut MutableOverlay,
    record: &WorkflowCurrentRecord,
    expected_owner_token: Option<&OverlayOwnerToken>,
) -> Result<OverlayOwnerToken> {
    overlay.put_value(
        record.overlay_key()?,
        expected_owner_token,
        record.encode()?,
    )
}

pub fn delete_workflow_current_record(
    overlay: &mut MutableOverlay,
    workspace_id: &str,
    project_id: &str,
    kind: WorkflowCurrentRecordKind,
    record_id: &str,
    expected_owner_token: Option<&OverlayOwnerToken>,
) -> Result<OverlayOwnerToken> {
    overlay.put_tombstone(
        workflow_current_key(workspace_id, project_id, kind, record_id)?,
        expected_owner_token,
    )
}

pub fn read_workflow_current_record(
    snapshot: &OverlayReadSnapshot,
    workspace_id: &str,
    project_id: &str,
    kind: WorkflowCurrentRecordKind,
    record_id: &str,
    base_read: impl FnOnce(&OverlayKey) -> Result<Option<Vec<u8>>>,
) -> Result<Option<WorkflowCurrentRecord>> {
    let key = workflow_current_key(workspace_id, project_id, kind, record_id)?;
    snapshot
        .read_composite(&key, |_, key| base_read(key))?
        .map(|bytes| WorkflowCurrentRecord::decode(&bytes))
        .transpose()
}

pub fn workflow_lane_current_record(
    workspace_id: &str,
    lane_id: &str,
    payload: impl Into<Vec<u8>>,
    operation_root: Option<Digest>,
) -> Result<WorkflowCurrentRecord> {
    WorkflowCurrentRecord::new(
        workspace_id,
        "lanes",
        WorkflowCurrentRecordKind::Lane,
        lane_id,
        payload,
        operation_root,
    )
}

pub fn workflow_active_assignment_record(
    workspace_id: &str,
    lane_id: &str,
    payload: impl Into<Vec<u8>>,
    operation_root: Option<Digest>,
) -> Result<WorkflowCurrentRecord> {
    WorkflowCurrentRecord::new(
        workspace_id,
        "assignments",
        WorkflowCurrentRecordKind::ActiveAssignment,
        lane_id,
        payload,
        operation_root,
    )
}

fn parse_kind(value: &str) -> Result<WorkflowCurrentRecordKind> {
    match value {
        "project" => Ok(WorkflowCurrentRecordKind::Project),
        "project-contract" => Ok(WorkflowCurrentRecordKind::ProjectContract),
        "ticket" => Ok(WorkflowCurrentRecordKind::Ticket),
        "comment" => Ok(WorkflowCurrentRecordKind::Comment),
        "relation" => Ok(WorkflowCurrentRecordKind::Relation),
        "board" => Ok(WorkflowCurrentRecordKind::Board),
        "lane" => Ok(WorkflowCurrentRecordKind::Lane),
        "active-assignment" => Ok(WorkflowCurrentRecordKind::ActiveAssignment),
        _ => Err(LoomError::invalid(
            "unsupported workflow current record kind",
        )),
    }
}

fn validate_segment(name: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(LoomError::invalid(format!("{name} must not be empty")));
    }
    if value.as_bytes().contains(&0) {
        return Err(LoomError::invalid(format!(
            "{name} must not contain nul bytes"
        )));
    }
    Ok(())
}

fn codec_error(error: loom_codec::CodecError) -> LoomError {
    LoomError::corrupt(format!("workflow current record codec error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn base(
        values: BTreeMap<OverlayKey, Vec<u8>>,
    ) -> impl FnOnce(&OverlayKey) -> Result<Option<Vec<u8>>> {
        move |key| Ok(values.get(key).cloned())
    }

    #[test]
    fn workflow_current_ticket_prefers_overlay_value() {
        let record = WorkflowCurrentRecord::new(
            "workspace",
            "project",
            WorkflowCurrentRecordKind::Ticket,
            "ticket-1",
            b"overlay".to_vec(),
            Some(Digest::blake3(b"operation")),
        )
        .unwrap();
        let key = record.overlay_key().unwrap();
        let mut base_values = BTreeMap::new();
        base_values.insert(
            key.clone(),
            WorkflowCurrentRecord::new(
                "workspace",
                "project",
                WorkflowCurrentRecordKind::Ticket,
                "ticket-1",
                b"base".to_vec(),
                None,
            )
            .unwrap()
            .encode()
            .unwrap(),
        );
        let mut overlay = MutableOverlay::new();
        put_workflow_current_record(&mut overlay, &record, None).unwrap();
        let snapshot = OverlayReadSnapshot::new(overlay.snapshot(), None, None);

        let read = read_workflow_current_record(
            &snapshot,
            "workspace",
            "project",
            WorkflowCurrentRecordKind::Ticket,
            "ticket-1",
            base(base_values),
        )
        .unwrap()
        .unwrap();

        assert_eq!(read, record);
    }

    #[test]
    fn workflow_current_tombstone_masks_base_record() {
        let base_record =
            workflow_lane_current_record("workspace", "agent-2", b"lane", None).unwrap();
        let mut base_values = BTreeMap::new();
        base_values.insert(
            base_record.overlay_key().unwrap(),
            base_record.encode().unwrap(),
        );
        let mut overlay = MutableOverlay::new();
        delete_workflow_current_record(
            &mut overlay,
            "workspace",
            "lanes",
            WorkflowCurrentRecordKind::Lane,
            "agent-2",
            None,
        )
        .unwrap();
        let snapshot = OverlayReadSnapshot::new(overlay.snapshot(), None, None);

        let read = read_workflow_current_record(
            &snapshot,
            "workspace",
            "lanes",
            WorkflowCurrentRecordKind::Lane,
            "agent-2",
            base(base_values),
        )
        .unwrap();

        assert_eq!(read, None);
    }

    #[test]
    fn workflow_current_rejects_stale_owner_token() {
        let first = WorkflowCurrentRecord::new(
            "workspace",
            "project",
            WorkflowCurrentRecordKind::Relation,
            "rel-1",
            b"first".to_vec(),
            None,
        )
        .unwrap();
        let second = WorkflowCurrentRecord::new(
            "workspace",
            "project",
            WorkflowCurrentRecordKind::Relation,
            "rel-1",
            b"second".to_vec(),
            None,
        )
        .unwrap();
        let other = WorkflowCurrentRecord::new(
            "workspace",
            "project",
            WorkflowCurrentRecordKind::Comment,
            "comment-1",
            b"other".to_vec(),
            None,
        )
        .unwrap();
        let mut overlay = MutableOverlay::new();
        let current = put_workflow_current_record(&mut overlay, &first, None).unwrap();
        let stale = put_workflow_current_record(&mut overlay, &other, None).unwrap();
        let error = put_workflow_current_record(&mut overlay, &second, Some(&stale)).unwrap_err();

        assert_eq!(error.code, loom_core::error::Code::Conflict);
        assert_eq!(
            overlay
                .snapshot()
                .owner_token(&first.overlay_key().unwrap())
                .unwrap()
                .map(|token| token.as_bytes().to_owned()),
            Some(*current.as_bytes())
        );
    }

    #[test]
    fn workflow_current_rejects_legacy_unenveloped_payload() {
        let record = WorkflowCurrentRecord::new(
            "workspace",
            "project",
            WorkflowCurrentRecordKind::Ticket,
            "ticket-1",
            b"current".to_vec(),
            None,
        )
        .unwrap();
        let mut base_values = BTreeMap::new();
        base_values.insert(record.overlay_key().unwrap(), b"legacy-current".to_vec());
        let snapshot = OverlayReadSnapshot::new(MutableOverlay::new().snapshot(), None, None);

        let error = read_workflow_current_record(
            &snapshot,
            "workspace",
            "project",
            WorkflowCurrentRecordKind::Ticket,
            "ticket-1",
            base(base_values),
        )
        .unwrap_err();

        assert_eq!(error.code, loom_core::error::Code::CorruptObject);
        assert!(
            error
                .message
                .contains("workflow current record codec error")
        );
    }
}
