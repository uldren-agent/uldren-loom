use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationEnvelope<T> {
    pub resource: T,
    pub receipt: MutationReceipt,
}

impl<T> MutationEnvelope<T> {
    pub fn new(resource: T, receipt: MutationReceipt) -> Self {
        Self { resource, receipt }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationReceipt {
    pub operation: String,
    pub resource_kind: String,
    pub resource_id: String,
    pub operation_id: Option<String>,
    pub root_before: Option<String>,
    pub root_after: Option<String>,
    pub changes: Vec<MutationChange>,
}

impl MutationReceipt {
    pub fn new(
        operation: impl Into<String>,
        resource_kind: impl Into<String>,
        resource_id: impl Into<String>,
    ) -> Self {
        Self {
            operation: operation.into(),
            resource_kind: resource_kind.into(),
            resource_id: resource_id.into(),
            operation_id: None,
            root_before: None,
            root_after: None,
            changes: Vec::new(),
        }
    }

    pub fn operation_id(mut self, operation_id: Option<impl Into<String>>) -> Self {
        self.operation_id = operation_id.map(Into::into);
        self
    }

    pub fn roots(
        mut self,
        root_before: Option<impl Into<String>>,
        root_after: Option<impl Into<String>>,
    ) -> Self {
        self.root_before = root_before.map(Into::into);
        self.root_after = root_after.map(Into::into);
        self
    }

    pub fn change(mut self, change: MutationChange) -> Self {
        self.changes.push(change);
        self
    }

    pub fn changes(mut self, changes: impl IntoIterator<Item = MutationChange>) -> Self {
        self.changes.extend(changes);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MutationChange {
    FieldChanged {
        field: String,
        before: Option<String>,
        after: Option<String>,
    },
    FieldSet {
        field: String,
        after: String,
    },
    FieldDeleted {
        field: String,
        before: Option<String>,
    },
    RelationSet {
        relation_id: String,
        relation_kind: String,
        target_id: String,
    },
    RelationRemoved {
        relation_id: String,
        relation_kind: String,
        target_id: String,
    },
    OrderChanged {
        field: String,
        before: Vec<String>,
        after: Vec<String>,
    },
    ResourceCreated,
    ResourceDeleted,
}

impl MutationChange {
    pub fn field_changed(
        field: impl Into<String>,
        before: Option<impl Into<String>>,
        after: Option<impl Into<String>>,
    ) -> Self {
        Self::FieldChanged {
            field: field.into(),
            before: before.map(Into::into),
            after: after.map(Into::into),
        }
    }

    pub fn field_set(field: impl Into<String>, after: impl Into<String>) -> Self {
        Self::FieldSet {
            field: field.into(),
            after: after.into(),
        }
    }

    pub fn field_deleted(field: impl Into<String>, before: Option<impl Into<String>>) -> Self {
        Self::FieldDeleted {
            field: field.into(),
            before: before.map(Into::into),
        }
    }

    pub fn relation_set(
        relation_id: impl Into<String>,
        relation_kind: impl Into<String>,
        target_id: impl Into<String>,
    ) -> Self {
        Self::RelationSet {
            relation_id: relation_id.into(),
            relation_kind: relation_kind.into(),
            target_id: target_id.into(),
        }
    }

    pub fn relation_removed(
        relation_id: impl Into<String>,
        relation_kind: impl Into<String>,
        target_id: impl Into<String>,
    ) -> Self {
        Self::RelationRemoved {
            relation_id: relation_id.into(),
            relation_kind: relation_kind.into(),
            target_id: target_id.into(),
        }
    }

    pub fn order_changed(
        field: impl Into<String>,
        before: Vec<String>,
        after: Vec<String>,
    ) -> Self {
        Self::OrderChanged {
            field: field.into(),
            before,
            after,
        }
    }
}
