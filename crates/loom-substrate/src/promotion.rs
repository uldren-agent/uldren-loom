use loom_types::{LoomError, Result};

use crate::validate_text;

pub const STUDIO_PROMOTION_TARGET_SCHEMA: &str = "loom.studio.promotion-target.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioPromotionTargetProfile {
    Tickets,
    Pages,
    Lifecycle,
    References,
    DecisionLog,
}

impl StudioPromotionTargetProfile {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "tickets" => Ok(Self::Tickets),
            "pages" => Ok(Self::Pages),
            "lifecycle" => Ok(Self::Lifecycle),
            "references" => Ok(Self::References),
            "decision-log" => Ok(Self::DecisionLog),
            _ => Err(LoomError::invalid(
                "studio promotion target_profile must be tickets, pages, lifecycle, references, or decision-log",
            )),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tickets => "tickets",
            Self::Pages => "pages",
            Self::Lifecycle => "lifecycle",
            Self::References => "references",
            Self::DecisionLog => "decision-log",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioPromotionTarget {
    pub profile: StudioPromotionTargetProfile,
    pub entity_ref: String,
}

impl StudioPromotionTarget {
    pub fn new(target_profile: &str, target_entity_ref: &str) -> Result<Self> {
        validate_text("studio promotion target_profile", target_profile)?;
        validate_text("studio promotion target_entity_ref", target_entity_ref)?;
        let profile = StudioPromotionTargetProfile::parse(target_profile)?;
        let target = Self {
            profile,
            entity_ref: target_entity_ref.to_string(),
        };
        target.validate_ref()?;
        Ok(target)
    }

    fn validate_ref(&self) -> Result<()> {
        match self.profile {
            StudioPromotionTargetProfile::Tickets => require_prefix(&self.entity_ref, "ticket:"),
            StudioPromotionTargetProfile::Pages => require_prefix(&self.entity_ref, "page:"),
            StudioPromotionTargetProfile::Lifecycle => {
                require_prefix(&self.entity_ref, "lifecycle:")
            }
            StudioPromotionTargetProfile::References => {
                if self.entity_ref.starts_with("reference:")
                    || self.entity_ref.starts_with("artifact:")
                {
                    Ok(())
                } else {
                    Err(LoomError::invalid(
                        "references promotion target_entity_ref must start with reference: or artifact:",
                    ))
                }
            }
            StudioPromotionTargetProfile::DecisionLog => {
                require_prefix(&self.entity_ref, "decision:")
            }
        }
    }
}

pub fn validate_studio_promotion(
    source_kind: &str,
    operation_kind: &str,
    target_profile: &str,
    target_entity_ref: &str,
) -> Result<StudioPromotionTarget> {
    validate_text("studio promotion source_kind", source_kind)?;
    validate_text("studio promotion operation_kind", operation_kind)?;
    let target = StudioPromotionTarget::new(target_profile, target_entity_ref)?;
    match (source_kind, operation_kind, target.profile) {
        ("Task", "task.promoted", StudioPromotionTargetProfile::Tickets)
        | ("Decision", "decision.promoted", StudioPromotionTargetProfile::DecisionLog)
        | ("Question", "question.promoted", StudioPromotionTargetProfile::Lifecycle)
        | ("Artifact", "artifact.promoted", StudioPromotionTargetProfile::References)
        | ("Reference", "reference.promoted", StudioPromotionTargetProfile::References) => {
            Ok(target)
        }
        _ => Err(LoomError::invalid(
            "studio promotion operation_kind does not match source kind and target profile",
        )),
    }
}

fn require_prefix(value: &str, prefix: &str) -> Result<()> {
    if value.starts_with(prefix) {
        Ok(())
    } else {
        Err(LoomError::invalid(format!(
            "studio promotion target_entity_ref must start with {prefix}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn promotion_target_validates_profile_and_reference_shape() {
        assert_eq!(
            StudioPromotionTarget::new("tickets", "ticket:core-1")
                .unwrap()
                .profile
                .as_str(),
            "tickets"
        );
        assert!(StudioPromotionTarget::new("tickets", "page:one").is_err());
        assert!(StudioPromotionTarget::new("unknown", "ticket:one").is_err());
    }

    #[test]
    fn promotion_contract_validates_source_operation_and_target() {
        assert!(
            validate_studio_promotion("Task", "task.promoted", "tickets", "ticket:core-1").is_ok()
        );
        assert!(
            validate_studio_promotion(
                "Decision",
                "decision.promoted",
                "decision-log",
                "decision:d1"
            )
            .is_ok()
        );
        assert!(
            validate_studio_promotion("Decision", "task.promoted", "tickets", "ticket:core-1")
                .is_err()
        );
    }
}
