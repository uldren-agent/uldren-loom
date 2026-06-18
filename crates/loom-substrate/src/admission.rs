use loom_codec::Value;
use loom_types::{LoomError, Result, WorkspaceId};

use crate::{Fields, codec_error, validate_text};

pub const WRITE_ADMISSION_POLICY_SCHEMA: &str = "loom.substrate.write-admission-policy.v1";
pub const WRITE_ADMISSION_CONTROL_PREFIX: &str = "substrate/write-admission/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteAdmissionMode {
    Advisory,
    Mandatory,
}

impl WriteAdmissionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Advisory => "advisory",
            Self::Mandatory => "mandatory",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "advisory" => Ok(Self::Advisory),
            "mandatory" => Ok(Self::Mandatory),
            _ => Err(LoomError::invalid(
                "write admission mode must be advisory or mandatory",
            )),
        }
    }

    const fn tag(self) -> u64 {
        match self {
            Self::Advisory => 0,
            Self::Mandatory => 1,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Advisory),
            1 => Ok(Self::Mandatory),
            _ => Err(LoomError::corrupt("unknown write admission mode")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WriteAdmissionTarget {
    pub target_kind: String,
    pub target_id: String,
}

impl WriteAdmissionTarget {
    pub fn new(target_kind: impl Into<String>, target_id: impl Into<String>) -> Result<Self> {
        let target_kind = target_kind.into();
        let target_id = target_id.into();
        validate_segment("target_kind", &target_kind)?;
        validate_segment("target_id", &target_id)?;
        Ok(Self {
            target_kind,
            target_id,
        })
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.target_kind.clone()),
            Value::Text(self.target_id.clone()),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "write admission target")?;
        let target_kind = fields.text("target_kind")?;
        let target_id = fields.text("target_id")?;
        fields.end("write admission target")?;
        Self::new(target_kind, target_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteAdmissionPolicy {
    pub workspace: WorkspaceId,
    pub surface: String,
    pub scope_id: String,
    pub default_mode: WriteAdmissionMode,
    pub mandatory_targets: Vec<WriteAdmissionTarget>,
}

impl WriteAdmissionPolicy {
    pub fn new(
        workspace: WorkspaceId,
        surface: impl Into<String>,
        scope_id: impl Into<String>,
        default_mode: WriteAdmissionMode,
        mandatory_targets: Vec<WriteAdmissionTarget>,
    ) -> Result<Self> {
        let surface = surface.into();
        let scope_id = scope_id.into();
        validate_segment("surface", &surface)?;
        validate_segment("scope_id", &scope_id)?;
        let mut mandatory_targets = mandatory_targets;
        mandatory_targets.sort();
        mandatory_targets.dedup();
        Ok(Self {
            workspace,
            surface,
            scope_id,
            default_mode,
            mandatory_targets,
        })
    }

    pub fn mode_for(&self, target_kind: &str, target_id: &str) -> WriteAdmissionMode {
        if self
            .mandatory_targets
            .iter()
            .any(|target| target.target_kind == target_kind && target.target_id == target_id)
        {
            WriteAdmissionMode::Mandatory
        } else {
            self.default_mode
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(WRITE_ADMISSION_POLICY_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace.to_string()),
                Value::Text(self.surface.clone()),
                Value::Text(self.scope_id.clone()),
                Value::Uint(self.default_mode.tag()),
                Value::Array(
                    self.mandatory_targets
                        .iter()
                        .map(WriteAdmissionTarget::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "write admission policy")?;
        outer.expect_text(WRITE_ADMISSION_POLICY_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("write admission policy fields")?,
            "write admission policy",
        )?;
        outer.end("write admission policy")?;
        let workspace = WorkspaceId::parse(&fields.text("workspace")?)?;
        let surface = fields.text("surface")?;
        let scope_id = fields.text("scope_id")?;
        let default_mode = WriteAdmissionMode::from_tag(fields.uint("default_mode")?)?;
        let mandatory_targets = match fields.next("mandatory_targets")? {
            Value::Array(items) => items
                .into_iter()
                .map(WriteAdmissionTarget::from_value)
                .collect::<Result<Vec<_>>>()?,
            _ => return Err(LoomError::corrupt("mandatory_targets must be an array")),
        };
        fields.end("write admission policy")?;
        Self::new(
            workspace,
            surface,
            scope_id,
            default_mode,
            mandatory_targets,
        )
    }
}

pub fn write_admission_policy_key(
    workspace: WorkspaceId,
    surface: &str,
    scope_id: &str,
) -> Result<Vec<u8>> {
    validate_segment("surface", surface)?;
    validate_segment("scope_id", scope_id)?;
    Ok(format!("{WRITE_ADMISSION_CONTROL_PREFIX}/{workspace}/{surface}/{scope_id}").into_bytes())
}

fn validate_segment(name: &str, value: &str) -> Result<()> {
    validate_text(name, value)?;
    if value.contains('/') || value.bytes().any(|byte| byte == b'\t' || byte == b'\n') {
        return Err(LoomError::invalid(format!(
            "{name} must not contain path separators or control separators"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(byte: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([byte; 16])
    }

    #[test]
    fn write_admission_policy_round_trips() {
        let policy = WriteAdmissionPolicy::new(
            id(1),
            "drive",
            "main",
            WriteAdmissionMode::Advisory,
            vec![
                WriteAdmissionTarget::new("file", "b").unwrap(),
                WriteAdmissionTarget::new("file", "a").unwrap(),
                WriteAdmissionTarget::new("file", "a").unwrap(),
            ],
        )
        .unwrap();
        assert_eq!(policy.mandatory_targets.len(), 2);
        assert_eq!(policy.mode_for("file", "a"), WriteAdmissionMode::Mandatory);
        assert_eq!(policy.mode_for("file", "c"), WriteAdmissionMode::Advisory);
        let encoded = policy.encode().unwrap();
        let decoded = WriteAdmissionPolicy::decode(&encoded).unwrap();
        assert_eq!(decoded, policy);
        assert_eq!(decoded.encode().unwrap(), encoded);
    }

    #[test]
    fn write_admission_policy_rejects_unsafe_key_segments() {
        assert!(WriteAdmissionTarget::new("file", "bad/id").is_err());
        assert!(write_admission_policy_key(id(1), "drive", "bad/scope").is_err());
    }
}
