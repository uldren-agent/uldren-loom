use crate::{
    BoardCardPlacement, ExternalTicketIdentity, Ticket, TicketAttachment, TicketBoard,
    TicketComment, TicketKey, TicketKeyResolution, TicketKeyStatus, TicketOperationRecord,
    TicketProfileState, TicketProject, TicketRelation, ticket_profile_state_key,
};
use loom_core::acl::{AclResource, AclResourceScope, AclScopeKind};
use loom_core::graph::{
    GraphValue, Props, graph_remove_edge, graph_upsert_edge, graph_upsert_node,
};
use loom_core::tabular::{ColumnType, Predicate, Row, Schema, Table, Value};
use loom_core::workspace::WorkspaceId;
use loom_core::{Code, Digest, Loom, LoomError, Result};
use loom_store::FileStore;

const STORAGE_ROOT: &str = ".loom/substrate/tickets/v2";
const PROJECTS_TABLE: &str = "projects";
const PREFIXES_TABLE: &str = "prefixes";
const TICKETS_TABLE: &str = "tickets";
const TICKET_NUMBERS_TABLE: &str = "ticket-numbers";
const EXTERNAL_IDS_TABLE: &str = "external-ids";
const OPERATIONS_TABLE: &str = "operations";
const COMMENTS_TABLE: &str = "comments";
const ATTACHMENTS_TABLE: &str = "attachments";
const WATCHERS_TABLE: &str = "watchers";
const RANKS_TABLE: &str = "ranks";
const BOARDS_TABLE: &str = "boards";
const BOARD_CARDS_TABLE: &str = "board-cards";
const TICKET_RELATION_GRAPH: &str = "ticket-relations";

pub fn profile_table_prefix(workspace_id: &str) -> Result<String> {
    ticket_profile_state_key(workspace_id)?;
    Ok(format!("{STORAGE_ROOT}/{workspace_id}"))
}

pub fn reconcile_reference_candidates(
    loom: &mut Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
    now_ms: u64,
    max: usize,
    resolver_principal: &str,
) -> Result<Vec<loom_reference::ResolvedReference>> {
    let target = loom_reference::ReferenceTarget {
        source_profile: "tickets".to_string(),
        source_scope: workspace_id.to_string(),
        next_attempt_ms: 0,
        pending: 0,
    };
    let mut resolutions = std::collections::BTreeMap::new();
    if let Some(profile) = TicketProfileReader::open(loom, namespace, workspace_id)? {
        for candidate in loom_reference::due(loom, namespace, &target, now_ms, max)? {
            let alias = candidate
                .alias_text
                .strip_prefix("!ticket:")
                .unwrap_or(&candidate.alias_text);
            let entity = profile
                .resolve_ticket_key(alias)?
                .map(|resolution| {
                    loom_substrate::refs::EntityRef::parse(&format!(
                        "ticket:{}",
                        resolution.ticket_id
                    ))
                })
                .transpose()?;
            resolutions.insert(candidate.candidate_id, entity);
        }
    }
    loom_reference::reconcile(
        loom,
        namespace,
        &target,
        now_ms,
        max,
        resolver_principal,
        |_, candidate| Ok(resolutions.get(&candidate.candidate_id).cloned().flatten()),
    )
}

pub struct IndexedTicketProfile<'a> {
    loom: &'a mut Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: String,
    state: TicketProfileState,
}

pub struct TicketProfileReader<'a> {
    loom: &'a Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: String,
    state: TicketProfileState,
}

impl<'a> TicketProfileReader<'a> {
    pub fn open(
        loom: &'a Loom<FileStore>,
        namespace: WorkspaceId,
        workspace_id: &str,
    ) -> Result<Option<Self>> {
        let prefix = profile_table_prefix(workspace_id)?;
        loom.authorize_resource(
            AclResource::scoped(
                namespace,
                loom_core::FacetKind::Vcs,
                None,
                AclResourceScope::Prefix {
                    kind: AclScopeKind::Table,
                    value: prefix.as_bytes(),
                },
            ),
            loom_core::AclRight::Read,
        )?;
        let Some(bytes) = loom
            .store()
            .control_get(&ticket_profile_state_key(workspace_id)?)?
        else {
            return Ok(None);
        };
        let state = TicketProfileState::decode(&bytes)?;
        if state.workspace_id != workspace_id {
            return Err(LoomError::corrupt(
                "ticket profile state workspace does not match key",
            ));
        }
        verify_state_roots(loom, namespace, workspace_id, &state)?;
        Ok(Some(Self {
            loom,
            namespace,
            workspace_id: workspace_id.to_string(),
            state,
        }))
    }

    pub fn profile_root(&self) -> Result<Digest> {
        Ok(Digest::hash(
            self.loom.store().digest_algo(),
            &self.state.encode()?,
        ))
    }

    pub fn project(&self, project_id: &str) -> Result<Option<TicketProject>> {
        get_project(self.loom, self.namespace, &self.workspace_id, project_id)
    }

    pub fn ticket(&self, ticket_id: &str) -> Result<Option<Ticket>> {
        get_ticket(self.loom, self.namespace, &self.workspace_id, ticket_id)
    }

    pub fn comments(&self, ticket_id: &str) -> Result<Vec<TicketComment>> {
        list_comments(self.loom, self.namespace, &self.workspace_id, ticket_id)
    }

    pub fn comment(&self, ticket_id: &str, comment_id: &str) -> Result<Option<TicketComment>> {
        get_comment(
            self.loom,
            self.namespace,
            &self.workspace_id,
            ticket_id,
            comment_id,
        )
    }

    pub fn attachments(&self, ticket_id: &str) -> Result<Vec<TicketAttachment>> {
        list_attachments(self.loom, self.namespace, &self.workspace_id, ticket_id)
    }

    pub fn watchers(&self, ticket_id: &str) -> Result<Vec<String>> {
        list_watchers(self.loom, self.namespace, &self.workspace_id, ticket_id)
    }

    pub fn rank_token(&self, ticket_id: &str) -> Result<Option<String>> {
        rank_token(self.loom, self.namespace, &self.workspace_id, ticket_id)
    }

    pub fn ticket_by_external_identity(
        &self,
        identity: &ExternalTicketIdentity,
    ) -> Result<Option<Ticket>> {
        get_ticket_by_external_identity(self.loom, self.namespace, &self.workspace_id, identity)
    }

    pub fn resolve_ticket_key(&self, value: &str) -> Result<Option<TicketKeyResolution>> {
        resolve_ticket_key(self.loom, self.namespace, &self.workspace_id, value)
    }

    pub fn prefix_exists(&self, prefix: &str) -> Result<bool> {
        prefix_exists(self.loom, self.namespace, &self.workspace_id, prefix)
    }

    pub fn tickets(&self) -> Result<Vec<Ticket>> {
        list_tickets(self.loom, self.namespace, &self.workspace_id)
    }

    pub fn projects(&self) -> Result<Vec<TicketProject>> {
        list_projects(self.loom, self.namespace, &self.workspace_id)
    }

    pub fn operations(&self) -> Result<Vec<TicketOperationRecord>> {
        list_operations(self.loom, self.namespace, &self.workspace_id)
    }

    pub fn board(&self, board_id: &str) -> Result<Option<TicketBoard>> {
        get_board(self.loom, self.namespace, &self.workspace_id, board_id)
    }

    pub fn boards(&self) -> Result<Vec<TicketBoard>> {
        list_boards(self.loom, self.namespace, &self.workspace_id)
    }

    pub fn board_cards(&self, board_id: &str) -> Result<Vec<BoardCardPlacement>> {
        list_board_cards(self.loom, self.namespace, &self.workspace_id, board_id)
    }
}

impl<'a> IndexedTicketProfile<'a> {
    pub fn open(
        loom: &'a mut Loom<FileStore>,
        namespace: WorkspaceId,
        workspace_id: &str,
    ) -> Result<Self> {
        let state = match loom
            .store()
            .control_get(&ticket_profile_state_key(workspace_id)?)?
        {
            Some(bytes) => TicketProfileState::decode(&bytes)?,
            None => {
                // Initial profile creation must publish the empty indexed tables and the initial
                // profile-state record atomically: flush the staged tables to a reference root, then
                // commit that reference root and the state in one superblock. A plain control_set
                // here would leave a stored state whose table roots were never published as the
                // reference root, so an interruption before the first mutation could make reopen
                // observe a stored state without its indexed tables.
                stage_empty_tables(loom, namespace, workspace_id)?;
                let state = state_from_tables(loom, namespace, workspace_id, 1)?;
                let reference_root = loom.save_state()?;
                loom.store().control_set_with_reference(
                    &ticket_profile_state_key(workspace_id)?,
                    state.encode()?,
                    Some(reference_root),
                )?;
                state
            }
        };
        if state.workspace_id != workspace_id {
            return Err(LoomError::corrupt(
                "ticket profile state workspace does not match key",
            ));
        }
        verify_state_roots(loom, namespace, workspace_id, &state)?;
        Ok(Self {
            loom,
            namespace,
            workspace_id: workspace_id.to_string(),
            state,
        })
    }

    pub fn profile_root(&self) -> Result<Digest> {
        Ok(Digest::hash(
            self.loom.store().digest_algo(),
            &self.state.encode()?,
        ))
    }

    pub fn enforce_expected_root(&self, expected_root: Option<&str>) -> Result<()> {
        let Some(expected_root) = expected_root else {
            return Ok(());
        };
        if Digest::parse(expected_root)? != self.profile_root()? {
            return Err(LoomError::new(
                Code::Conflict,
                "ticket profile root does not match expected_root",
            ));
        }
        Ok(())
    }

    pub fn next_sequence(&self) -> u64 {
        self.state.next_sequence
    }

    pub fn effective_principal(&self) -> Result<Option<WorkspaceId>> {
        self.loom.effective_principal()
    }

    pub fn digest_algo(&self) -> loom_types::Algo {
        self.loom.store().digest_algo()
    }

    pub fn next_profile_root(&self) -> Result<Digest> {
        let state = state_from_tables(
            self.loom,
            self.namespace,
            &self.workspace_id,
            self.state
                .next_sequence
                .checked_add(1)
                .ok_or_else(|| LoomError::invalid("ticket operation sequence overflow"))?,
        )?;
        Ok(Digest::hash(
            self.loom.store().digest_algo(),
            &state.encode()?,
        ))
    }

    pub fn project(&self, project_id: &str) -> Result<Option<TicketProject>> {
        get_project(self.loom, self.namespace, &self.workspace_id, project_id)
    }

    pub fn ticket(&self, ticket_id: &str) -> Result<Option<Ticket>> {
        get_ticket(self.loom, self.namespace, &self.workspace_id, ticket_id)
    }

    pub fn ticket_by_external_identity(
        &self,
        identity: &ExternalTicketIdentity,
    ) -> Result<Option<Ticket>> {
        get_ticket_by_external_identity(self.loom, self.namespace, &self.workspace_id, identity)
    }

    pub fn resolve_ticket_key(&self, value: &str) -> Result<Option<TicketKeyResolution>> {
        resolve_ticket_key(self.loom, self.namespace, &self.workspace_id, value)
    }

    pub fn prefix_exists(&self, prefix: &str) -> Result<bool> {
        prefix_exists(self.loom, self.namespace, &self.workspace_id, prefix)
    }

    pub fn put_project(&mut self, project: &TicketProject) -> Result<()> {
        self.loom.insert_row_reserved(
            self.namespace,
            &table_path(&self.workspace_id, PROJECTS_TABLE),
            vec![
                Value::Text(project.project_id.clone()),
                Value::Bytes(project.encode()?),
            ],
        )?;
        self.loom.insert_row_reserved(
            self.namespace,
            &table_path(&self.workspace_id, PREFIXES_TABLE),
            vec![
                Value::Text(project.key_prefix.clone()),
                Value::Text(project.project_id.clone()),
                Value::Text("active".to_string()),
            ],
        )?;
        for prefix in &project.retired_prefixes {
            self.loom.insert_row_reserved(
                self.namespace,
                &table_path(&self.workspace_id, PREFIXES_TABLE),
                vec![
                    Value::Text(prefix.clone()),
                    Value::Text(project.project_id.clone()),
                    Value::Text("retired".to_string()),
                ],
            )?;
        }
        Ok(())
    }

    pub fn remove_prefix(&mut self, prefix: &str) -> Result<()> {
        self.loom.delete_row_reserved(
            self.namespace,
            &table_path(&self.workspace_id, PREFIXES_TABLE),
            &[Value::Text(prefix.to_string())],
        )
    }

    pub fn put_ticket(&mut self, ticket: &Ticket) -> Result<()> {
        self.loom.insert_row_reserved(
            self.namespace,
            &table_path(&self.workspace_id, TICKETS_TABLE),
            vec![
                Value::Text(ticket.ticket_id.clone()),
                Value::Bytes(ticket.encode()?),
            ],
        )?;
        self.loom.insert_row_reserved(
            self.namespace,
            &table_path(&self.workspace_id, TICKET_NUMBERS_TABLE),
            vec![
                Value::Text(ticket.project_id.clone()),
                Value::U64(ticket.ticket_number),
                Value::Text(ticket.ticket_id.clone()),
            ],
        )?;
        if let Some(identity) = &ticket.external_identity {
            self.loom.insert_row_reserved(
                self.namespace,
                &table_path(&self.workspace_id, EXTERNAL_IDS_TABLE),
                vec![
                    Value::Text(identity.source.clone()),
                    Value::Text(identity.id.clone()),
                    Value::Text(ticket.ticket_id.clone()),
                ],
            )?;
        }
        Ok(())
    }

    pub fn append_operation(&mut self, record: &TicketOperationRecord) -> Result<()> {
        self.loom.insert_row_reserved(
            self.namespace,
            &table_path(&self.workspace_id, OPERATIONS_TABLE),
            vec![Value::U64(record.sequence), Value::Bytes(record.encode()?)],
        )
    }

    pub fn put_comment(&mut self, ticket_id: &str, comment: &TicketComment) -> Result<()> {
        self.ensure_table(COMMENTS_TABLE)?;
        self.loom.insert_row_reserved(
            self.namespace,
            &table_path(&self.workspace_id, COMMENTS_TABLE),
            vec![
                Value::Text(ticket_id.to_string()),
                Value::Text(comment.comment_id.clone()),
                Value::Bytes(comment.encode()?),
            ],
        )
    }

    pub fn comments(&self, ticket_id: &str) -> Result<Vec<TicketComment>> {
        list_comments(self.loom, self.namespace, &self.workspace_id, ticket_id)
    }

    pub fn delete_comment(&mut self, ticket_id: &str, comment_id: &str) -> Result<()> {
        self.loom.delete_row_reserved(
            self.namespace,
            &table_path(&self.workspace_id, COMMENTS_TABLE),
            &[
                Value::Text(ticket_id.to_string()),
                Value::Text(comment_id.to_string()),
            ],
        )
    }

    pub fn comment(&self, ticket_id: &str, comment_id: &str) -> Result<Option<TicketComment>> {
        get_comment(
            self.loom,
            self.namespace,
            &self.workspace_id,
            ticket_id,
            comment_id,
        )
    }

    pub fn comment_exists(&self, ticket_id: &str, comment_id: &str) -> Result<bool> {
        Ok(point_row(
            self.loom,
            self.namespace,
            &table_path(&self.workspace_id, COMMENTS_TABLE),
            &[
                Value::Text(ticket_id.to_string()),
                Value::Text(comment_id.to_string()),
            ],
        )?
        .is_some())
    }

    pub fn put_attachment(&mut self, ticket_id: &str, attachment: &TicketAttachment) -> Result<()> {
        self.ensure_table(ATTACHMENTS_TABLE)?;
        self.loom.insert_row_reserved(
            self.namespace,
            &table_path(&self.workspace_id, ATTACHMENTS_TABLE),
            vec![
                Value::Text(ticket_id.to_string()),
                Value::Text(attachment.attachment_id.clone()),
                Value::Bytes(attachment.encode()?),
            ],
        )
    }

    pub fn attachment_exists(&self, ticket_id: &str, attachment_id: &str) -> Result<bool> {
        Ok(point_row(
            self.loom,
            self.namespace,
            &table_path(&self.workspace_id, ATTACHMENTS_TABLE),
            &[
                Value::Text(ticket_id.to_string()),
                Value::Text(attachment_id.to_string()),
            ],
        )?
        .is_some())
    }

    pub fn set_watcher(&mut self, ticket_id: &str, principal: &str, watch: bool) -> Result<()> {
        self.ensure_table(WATCHERS_TABLE)?;
        let path = table_path(&self.workspace_id, WATCHERS_TABLE);
        let key = [
            Value::Text(ticket_id.to_string()),
            Value::Text(principal.to_string()),
        ];
        if watch {
            self.loom.insert_row_reserved(
                self.namespace,
                &path,
                vec![key[0].clone(), key[1].clone()],
            )
        } else {
            self.loom.delete_row_reserved(self.namespace, &path, &key)
        }
    }

    pub fn set_rank_token(&mut self, ticket_id: &str, rank_token: &str) -> Result<()> {
        self.ensure_table(RANKS_TABLE)?;
        self.loom.insert_row_reserved(
            self.namespace,
            &table_path(&self.workspace_id, RANKS_TABLE),
            vec![
                Value::Text(ticket_id.to_string()),
                Value::Text(rank_token.to_string()),
            ],
        )
    }

    fn ensure_table(&mut self, table_name: &str) -> Result<()> {
        let path = table_path(&self.workspace_id, table_name);
        if self
            .loom
            .table_reader_reserved(self.namespace, &path)?
            .is_some()
        {
            return Ok(());
        }
        self.loom
            .stage_table_reserved(self.namespace, &path, &Table::new(schema_for(table_name)?))
    }

    pub fn upsert_relation_projection(
        &mut self,
        source_ticket_id: &str,
        relation: &TicketRelation,
    ) -> Result<String> {
        let source = ticket_relation_node_id("ticket", source_ticket_id);
        let target = ticket_relation_node_id(relation.target_type.as_str(), &relation.target_id);
        graph_upsert_node(
            self.loom,
            self.namespace,
            TICKET_RELATION_GRAPH,
            &source,
            relation_node_props("ticket", source_ticket_id),
        )?;
        graph_upsert_node(
            self.loom,
            self.namespace,
            TICKET_RELATION_GRAPH,
            &target,
            relation_node_props(relation.target_type.as_str(), &relation.target_id),
        )?;
        let edge_id = ticket_relation_edge_id(source_ticket_id, relation);
        graph_upsert_edge(
            self.loom,
            self.namespace,
            TICKET_RELATION_GRAPH,
            &edge_id,
            &source,
            &target,
            relation.kind.as_str(),
            relation_edge_props(source_ticket_id, relation),
        )?;
        Ok(edge_id)
    }

    pub fn remove_relation_projection(
        &mut self,
        source_ticket_id: &str,
        relation: &TicketRelation,
    ) -> Result<bool> {
        graph_remove_edge(
            self.loom,
            self.namespace,
            TICKET_RELATION_GRAPH,
            &ticket_relation_edge_id(source_ticket_id, relation),
        )
    }

    pub fn operations(&self) -> Result<Vec<TicketOperationRecord>> {
        list_operations(self.loom, self.namespace, &self.workspace_id)
    }

    pub fn tickets(&self) -> Result<Vec<Ticket>> {
        list_tickets(self.loom, self.namespace, &self.workspace_id)
    }

    pub fn projects(&self) -> Result<Vec<TicketProject>> {
        list_projects(self.loom, self.namespace, &self.workspace_id)
    }

    pub fn board(&self, board_id: &str) -> Result<Option<TicketBoard>> {
        get_board(self.loom, self.namespace, &self.workspace_id, board_id)
    }

    pub fn boards(&self) -> Result<Vec<TicketBoard>> {
        list_boards(self.loom, self.namespace, &self.workspace_id)
    }

    pub fn board_cards(&self, board_id: &str) -> Result<Vec<BoardCardPlacement>> {
        list_board_cards(self.loom, self.namespace, &self.workspace_id, board_id)
    }

    pub fn put_board(&mut self, board: &TicketBoard) -> Result<()> {
        self.loom.insert_row_reserved(
            self.namespace,
            &table_path(&self.workspace_id, BOARDS_TABLE),
            vec![
                Value::Text(board.board_id.clone()),
                Value::Text(board.board_key.clone()),
                Value::Bytes(board.encode()?),
            ],
        )
    }

    pub fn put_board_card(&mut self, placement: &BoardCardPlacement) -> Result<()> {
        self.loom.insert_row_reserved(
            self.namespace,
            &table_path(&self.workspace_id, BOARD_CARDS_TABLE),
            vec![
                Value::Text(placement.board_id.clone()),
                Value::Text(placement.ticket_id.clone()),
                Value::Bytes(placement.encode()?),
            ],
        )
    }

    pub fn remove_board_card(&mut self, board_id: &str, ticket_id: &str) -> Result<()> {
        self.loom.delete_row_reserved(
            self.namespace,
            &table_path(&self.workspace_id, BOARD_CARDS_TABLE),
            &[
                Value::Text(board_id.to_string()),
                Value::Text(ticket_id.to_string()),
            ],
        )
    }

    pub fn finish_operation(&mut self) -> Result<Digest> {
        self.finish_operation_with_audit(None, None, None)
    }

    pub fn finish_operation_with_audit(
        &mut self,
        principal: Option<WorkspaceId>,
        action: Option<&str>,
        target: Option<&str>,
    ) -> Result<Digest> {
        self.state = state_from_tables(
            self.loom,
            self.namespace,
            &self.workspace_id,
            self.state
                .next_sequence
                .checked_add(1)
                .ok_or_else(|| LoomError::invalid("ticket operation sequence overflow"))?,
        )?;
        let key = ticket_profile_state_key(&self.workspace_id)?;
        let value = self.state.encode()?;
        // Flush the staged indexed tables to a reference (engine working-tree) root, then commit
        // that reference root AND the new TicketProfileState control record in ONE superblock
        // transaction. This is the prevention invariant: a successful ticket mutation can never
        // leave the indexed-table roots and the stored profile state mismatched, because both are
        // published by a single atomic commit rather than two separate ones. An interruption
        // exposes either the fully-old or fully-new committed state.
        let reference_root = self.loom.save_state()?;
        if let Some(action) = action {
            self.loom.store().control_set_audited_with_reference(
                &key,
                value,
                principal,
                action,
                target,
                Some(reference_root),
            )?;
        } else {
            self.loom
                .store()
                .control_set_with_reference(&key, value, Some(reference_root))?;
        }
        self.profile_root()
    }
}

pub fn ticket_relation_edge_id(source_ticket_id: &str, relation: &TicketRelation) -> String {
    format!(
        "ticket_relation:{source_ticket_id}:{}:{}:{}:{}",
        relation.kind.as_str(),
        relation.target_type.as_str(),
        relation.target_id,
        relation.relation_id
    )
}

fn ticket_relation_node_id(target_type: &str, target_id: &str) -> String {
    format!("{target_type}:{target_id}")
}

fn relation_node_props(target_type: &str, target_id: &str) -> Props {
    std::collections::BTreeMap::from([
        (
            "target_type".to_string(),
            GraphValue::Text(target_type.to_string()),
        ),
        (
            "target_id".to_string(),
            GraphValue::Text(target_id.to_string()),
        ),
    ])
}

fn relation_edge_props(source_ticket_id: &str, relation: &TicketRelation) -> Props {
    std::collections::BTreeMap::from([
        (
            "derived_from".to_string(),
            GraphValue::Text("tickets".to_string()),
        ),
        (
            "source_ticket_id".to_string(),
            GraphValue::Text(source_ticket_id.to_string()),
        ),
        (
            "relation_id".to_string(),
            GraphValue::Text(relation.relation_id.clone()),
        ),
        (
            "target_type".to_string(),
            GraphValue::Text(relation.target_type.as_str().to_string()),
        ),
        (
            "target_id".to_string(),
            GraphValue::Text(relation.target_id.clone()),
        ),
    ])
}

fn resolve_ticket_key(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
    value: &str,
) -> Result<Option<TicketKeyResolution>> {
    let requested_key = TicketKey::parse(value)?;
    let Some(prefix_row) = point_row(
        loom,
        namespace,
        &table_path(workspace_id, PREFIXES_TABLE),
        &[Value::Text(requested_key.prefix.clone())],
    )?
    else {
        return Ok(None);
    };
    let project_id = row_text(&prefix_row, 1, "ticket prefix project")?;
    let status = match row_text(&prefix_row, 2, "ticket prefix status")?.as_str() {
        "active" => TicketKeyStatus::Active,
        "retired" => TicketKeyStatus::Retired,
        _ => return Err(LoomError::corrupt("ticket prefix status is invalid")),
    };
    let Some(number_row) = point_row(
        loom,
        namespace,
        &table_path(workspace_id, TICKET_NUMBERS_TABLE),
        &[
            Value::Text(project_id.clone()),
            Value::U64(requested_key.number),
        ],
    )?
    else {
        return Ok(None);
    };
    let ticket_id = row_text(&number_row, 2, "ticket number target")?;
    let ticket = get_ticket(loom, namespace, workspace_id, &ticket_id)?
        .ok_or_else(|| LoomError::corrupt("ticket number resolved to a missing ticket"))?;
    let project = get_project(loom, namespace, workspace_id, &project_id)?
        .ok_or_else(|| LoomError::corrupt("ticket prefix resolved to a missing project"))?;
    Ok(Some(TicketKeyResolution {
        ticket_id,
        requested_key,
        current_key: project.ticket_key(ticket.ticket_number)?,
        status,
    }))
}

fn prefix_exists(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
    prefix: &str,
) -> Result<bool> {
    Ok(point_row(
        loom,
        namespace,
        &table_path(workspace_id, PREFIXES_TABLE),
        &[Value::Text(prefix.to_string())],
    )?
    .is_some())
}

fn table_path(workspace_id: &str, table: &str) -> String {
    format!("{STORAGE_ROOT}/{workspace_id}/{table}")
}

fn stage_empty_tables(
    loom: &mut Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
) -> Result<()> {
    for (name, schema) in schemas() {
        loom.stage_table_reserved(
            namespace,
            &table_path(workspace_id, name),
            &Table::new(schema),
        )?;
    }
    Ok(())
}

fn schemas() -> Vec<(&'static str, Schema)> {
    vec![
        (
            PROJECTS_TABLE,
            Schema::new(
                vec![
                    ("project_id".to_string(), ColumnType::Text),
                    ("payload".to_string(), ColumnType::Bytes),
                ],
                vec![0],
            )
            .expect("ticket projects schema is valid"),
        ),
        (
            PREFIXES_TABLE,
            Schema::new(
                vec![
                    ("prefix".to_string(), ColumnType::Text),
                    ("project_id".to_string(), ColumnType::Text),
                    ("status".to_string(), ColumnType::Text),
                ],
                vec![0],
            )
            .expect("ticket prefixes schema is valid"),
        ),
        (
            TICKETS_TABLE,
            Schema::new(
                vec![
                    ("ticket_id".to_string(), ColumnType::Text),
                    ("payload".to_string(), ColumnType::Bytes),
                ],
                vec![0],
            )
            .expect("tickets schema is valid"),
        ),
        (
            TICKET_NUMBERS_TABLE,
            Schema::new(
                vec![
                    ("project_id".to_string(), ColumnType::Text),
                    ("ticket_number".to_string(), ColumnType::U64),
                    ("ticket_id".to_string(), ColumnType::Text),
                ],
                vec![0, 1],
            )
            .expect("ticket numbers schema is valid"),
        ),
        (
            EXTERNAL_IDS_TABLE,
            Schema::new(
                vec![
                    ("source".to_string(), ColumnType::Text),
                    ("external_id".to_string(), ColumnType::Text),
                    ("ticket_id".to_string(), ColumnType::Text),
                ],
                vec![0, 1],
            )
            .expect("ticket external identity schema is valid"),
        ),
        (
            EXTERNAL_IDS_TABLE,
            Schema::new(
                vec![
                    ("source".to_string(), ColumnType::Text),
                    ("external_id".to_string(), ColumnType::Text),
                    ("ticket_id".to_string(), ColumnType::Text),
                ],
                vec![0, 1],
            )
            .expect("ticket external ids schema is valid"),
        ),
        (
            OPERATIONS_TABLE,
            Schema::new(
                vec![
                    ("sequence".to_string(), ColumnType::U64),
                    ("payload".to_string(), ColumnType::Bytes),
                ],
                vec![0],
            )
            .expect("ticket operations schema is valid"),
        ),
        (
            COMMENTS_TABLE,
            Schema::new(
                vec![
                    ("ticket_id".to_string(), ColumnType::Text),
                    ("comment_id".to_string(), ColumnType::Text),
                    ("payload".to_string(), ColumnType::Bytes),
                ],
                vec![0, 1],
            )
            .expect("ticket comments schema is valid"),
        ),
        (
            ATTACHMENTS_TABLE,
            Schema::new(
                vec![
                    ("ticket_id".to_string(), ColumnType::Text),
                    ("attachment_id".to_string(), ColumnType::Text),
                    ("payload".to_string(), ColumnType::Bytes),
                ],
                vec![0, 1],
            )
            .expect("ticket attachments schema is valid"),
        ),
        (
            WATCHERS_TABLE,
            Schema::new(
                vec![
                    ("ticket_id".to_string(), ColumnType::Text),
                    ("principal".to_string(), ColumnType::Text),
                ],
                vec![0, 1],
            )
            .expect("ticket watchers schema is valid"),
        ),
        (
            RANKS_TABLE,
            Schema::new(
                vec![
                    ("ticket_id".to_string(), ColumnType::Text),
                    ("rank_token".to_string(), ColumnType::Text),
                ],
                vec![0],
            )
            .expect("ticket ranks schema is valid"),
        ),
        (
            BOARDS_TABLE,
            Schema::new(
                vec![
                    ("board_id".to_string(), ColumnType::Text),
                    ("board_key".to_string(), ColumnType::Text),
                    ("payload".to_string(), ColumnType::Bytes),
                ],
                vec![0],
            )
            .expect("ticket boards schema is valid"),
        ),
        (
            BOARD_CARDS_TABLE,
            Schema::new(
                vec![
                    ("board_id".to_string(), ColumnType::Text),
                    ("ticket_id".to_string(), ColumnType::Text),
                    ("payload".to_string(), ColumnType::Bytes),
                ],
                vec![0, 1],
            )
            .expect("ticket board cards schema is valid"),
        ),
    ]
}

fn state_from_tables(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
    next_sequence: u64,
) -> Result<TicketProfileState> {
    TicketProfileState::new(
        workspace_id,
        next_sequence,
        table_root(loom, namespace, workspace_id, PROJECTS_TABLE)?,
        table_root(loom, namespace, workspace_id, PREFIXES_TABLE)?,
        table_root(loom, namespace, workspace_id, TICKETS_TABLE)?,
        table_root(loom, namespace, workspace_id, TICKET_NUMBERS_TABLE)?,
        table_root(loom, namespace, workspace_id, EXTERNAL_IDS_TABLE)?,
        table_root(loom, namespace, workspace_id, BOARDS_TABLE)?,
        table_root(loom, namespace, workspace_id, BOARD_CARDS_TABLE)?,
    )
}

fn mismatched_state_tables(
    actual: &TicketProfileState,
    state: &TicketProfileState,
) -> Vec<&'static str> {
    let mut mismatches = Vec::new();
    if actual.projects_root != state.projects_root {
        mismatches.push(PROJECTS_TABLE);
    }
    if actual.prefixes_root != state.prefixes_root {
        mismatches.push(PREFIXES_TABLE);
    }
    if actual.tickets_root != state.tickets_root {
        mismatches.push(TICKETS_TABLE);
    }
    if actual.ticket_numbers_root != state.ticket_numbers_root {
        mismatches.push(TICKET_NUMBERS_TABLE);
    }
    if actual.external_ids_root != state.external_ids_root {
        mismatches.push(EXTERNAL_IDS_TABLE);
    }
    if state.board_roots_present {
        if actual.boards_root != state.boards_root {
            mismatches.push(BOARDS_TABLE);
        }
        if actual.board_cards_root != state.board_cards_root {
            mismatches.push(BOARD_CARDS_TABLE);
        }
    }
    mismatches
}

fn verify_state_roots(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
    state: &TicketProfileState,
) -> Result<()> {
    let actual = state_from_tables(loom, namespace, workspace_id, state.next_sequence)?;
    let mismatches = mismatched_state_tables(&actual, state);
    if !mismatches.is_empty() {
        return Err(LoomError::corrupt(format!(
            "ticket profile state does not match indexed tables: {}",
            mismatches.join(", ")
        )));
    }
    Ok(())
}

fn table_root(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
    table: &str,
) -> Result<Digest> {
    loom.staged_table_root(namespace, &table_path(workspace_id, table))
        .ok_or_else(|| LoomError::corrupt("ticket indexed table is missing"))
}

fn point_row(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    table: &str,
    key: &[Value],
) -> Result<Option<Row>> {
    let Some((schema, Some(root))) = loom.table_reader_reserved(namespace, table)? else {
        return Ok(None);
    };
    Table::get_row(loom.store(), &schema, &root, key)
}

fn get_project(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
    project_id: &str,
) -> Result<Option<TicketProject>> {
    let Some(row) = point_row(
        loom,
        namespace,
        &table_path(workspace_id, PROJECTS_TABLE),
        &[Value::Text(project_id.to_string())],
    )?
    else {
        return Ok(None);
    };
    TicketProject::decode(row_bytes(&row, 1, "ticket project")?).map(Some)
}

fn get_ticket(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
    ticket_id: &str,
) -> Result<Option<Ticket>> {
    let Some(row) = point_row(
        loom,
        namespace,
        &table_path(workspace_id, TICKETS_TABLE),
        &[Value::Text(ticket_id.to_string())],
    )?
    else {
        return Ok(None);
    };
    Ticket::decode(row_bytes(&row, 1, "ticket")?).map(Some)
}

fn get_board(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
    board_id: &str,
) -> Result<Option<TicketBoard>> {
    let Some(row) = point_row(
        loom,
        namespace,
        &table_path(workspace_id, BOARDS_TABLE),
        &[Value::Text(board_id.to_string())],
    )?
    else {
        return Ok(None);
    };
    TicketBoard::decode(row_bytes(&row, 2, "ticket board")?).map(Some)
}

fn get_ticket_by_external_identity(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
    identity: &ExternalTicketIdentity,
) -> Result<Option<Ticket>> {
    let Some(row) = point_row(
        loom,
        namespace,
        &table_path(workspace_id, EXTERNAL_IDS_TABLE),
        &[
            Value::Text(identity.source.clone()),
            Value::Text(identity.id.clone()),
        ],
    )?
    else {
        return Ok(None);
    };
    let ticket_id = row_text(&row, 2, "ticket external identity target")?;
    get_ticket(loom, namespace, workspace_id, &ticket_id)?
        .map(Some)
        .ok_or_else(|| LoomError::corrupt("ticket external identity resolved to a missing ticket"))
}

fn list_tickets(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
) -> Result<Vec<Ticket>> {
    rows(loom, namespace, &table_path(workspace_id, TICKETS_TABLE))?
        .into_iter()
        .map(|row| Ticket::decode(row_bytes(&row, 1, "ticket")?))
        .collect()
}

fn list_projects(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
) -> Result<Vec<TicketProject>> {
    rows(loom, namespace, &table_path(workspace_id, PROJECTS_TABLE))?
        .into_iter()
        .map(|row| TicketProject::decode(row_bytes(&row, 1, "ticket project")?))
        .collect()
}

fn list_operations(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
) -> Result<Vec<TicketOperationRecord>> {
    rows(loom, namespace, &table_path(workspace_id, OPERATIONS_TABLE))?
        .into_iter()
        .map(|row| TicketOperationRecord::decode(row_bytes(&row, 1, "ticket operation")?))
        .collect()
}

fn list_boards(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
) -> Result<Vec<TicketBoard>> {
    rows(loom, namespace, &table_path(workspace_id, BOARDS_TABLE))?
        .into_iter()
        .map(|row| TicketBoard::decode(row_bytes(&row, 2, "ticket board")?))
        .collect()
}

fn list_board_cards(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
    board_id: &str,
) -> Result<Vec<BoardCardPlacement>> {
    rows(
        loom,
        namespace,
        &table_path(workspace_id, BOARD_CARDS_TABLE),
    )?
    .into_iter()
    .filter(|row| row_text(row, 0, "board card source").ok().as_deref() == Some(board_id))
    .map(|row| BoardCardPlacement::decode(row_bytes(&row, 2, "board card")?))
    .collect()
}

fn list_comments(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
    ticket_id: &str,
) -> Result<Vec<TicketComment>> {
    optional_rows(loom, namespace, &table_path(workspace_id, COMMENTS_TABLE))?
        .into_iter()
        .filter(|row| row_text(row, 0, "ticket comment source").ok().as_deref() == Some(ticket_id))
        .map(|row| TicketComment::decode(row_bytes(&row, 2, "ticket comment")?))
        .collect()
}

fn get_comment(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
    ticket_id: &str,
    comment_id: &str,
) -> Result<Option<TicketComment>> {
    point_row(
        loom,
        namespace,
        &table_path(workspace_id, COMMENTS_TABLE),
        &[
            Value::Text(ticket_id.to_string()),
            Value::Text(comment_id.to_string()),
        ],
    )?
    .map(|row| TicketComment::decode(row_bytes(&row, 2, "ticket comment")?))
    .transpose()
}

fn list_attachments(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
    ticket_id: &str,
) -> Result<Vec<TicketAttachment>> {
    optional_rows(
        loom,
        namespace,
        &table_path(workspace_id, ATTACHMENTS_TABLE),
    )?
    .into_iter()
    .filter(|row| row_text(row, 0, "ticket attachment source").ok().as_deref() == Some(ticket_id))
    .map(|row| TicketAttachment::decode(row_bytes(&row, 2, "ticket attachment")?))
    .collect()
}

fn list_watchers(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
    ticket_id: &str,
) -> Result<Vec<String>> {
    optional_rows(loom, namespace, &table_path(workspace_id, WATCHERS_TABLE))?
        .into_iter()
        .filter(|row| row_text(row, 0, "ticket watcher source").ok().as_deref() == Some(ticket_id))
        .map(|row| row_text(&row, 1, "ticket watcher"))
        .collect()
}

fn rank_token(
    loom: &Loom<FileStore>,
    namespace: WorkspaceId,
    workspace_id: &str,
    ticket_id: &str,
) -> Result<Option<String>> {
    let Some(row) = point_row(
        loom,
        namespace,
        &table_path(workspace_id, RANKS_TABLE),
        &[Value::Text(ticket_id.to_string())],
    )?
    else {
        return Ok(None);
    };
    row_text(&row, 1, "ticket rank token").map(Some)
}

fn optional_rows(loom: &Loom<FileStore>, namespace: WorkspaceId, table: &str) -> Result<Vec<Row>> {
    if loom.table_reader_reserved(namespace, table)?.is_none() {
        return Ok(Vec::new());
    }
    rows(loom, namespace, table)
}

fn rows(loom: &Loom<FileStore>, namespace: WorkspaceId, table: &str) -> Result<Vec<Row>> {
    Ok(loom
        .read_table_reserved(namespace, table)?
        .scan(&Predicate::All)
        .into_iter()
        .cloned()
        .collect())
}

fn schema_for(table_name: &str) -> Result<Schema> {
    schemas()
        .into_iter()
        .find_map(|(name, schema)| (name == table_name).then_some(schema))
        .ok_or_else(|| LoomError::corrupt("ticket indexed table schema is missing"))
}

fn row_bytes<'a>(row: &'a Row, index: usize, description: &str) -> Result<&'a [u8]> {
    match row.get(index) {
        Some(Value::Bytes(value)) => Ok(value),
        _ => Err(LoomError::corrupt(format!(
            "{description} payload is invalid"
        ))),
    }
}

fn row_text(row: &Row, index: usize, description: &str) -> Result<String> {
    match row.get(index) {
        Some(Value::Text(value)) => Ok(value.clone()),
        _ => Err(LoomError::corrupt(format!("{description} is invalid"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{Algo, Loom};

    fn temp_path() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let uniq = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "loom-ticket-indexed-{seq}-{uniq}-{}.loom",
            std::process::id()
        ))
    }

    #[test]
    fn reader_reports_mismatched_indexed_table_roots() {
        let path = temp_path();
        let namespace = WorkspaceId::v4_from_bytes([19; 16]);
        let workspace_id = namespace.to_string();
        let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
        stage_empty_tables(&mut loom, namespace, &workspace_id).unwrap();
        let state = state_from_tables(&loom, namespace, &workspace_id, 1).unwrap();
        let _ = loom.save_state().unwrap();
        loom.store()
            .control_set(
                &ticket_profile_state_key(&workspace_id).unwrap(),
                state.encode().unwrap(),
            )
            .unwrap();
        let project = TicketProject::new("matrix", "MX", "Matrix").unwrap();
        loom.insert_row_reserved(
            namespace,
            &table_path(&workspace_id, PROJECTS_TABLE),
            vec![
                Value::Text(project.project_id.clone()),
                Value::Bytes(project.encode().unwrap()),
            ],
        )
        .unwrap();

        let error = match TicketProfileReader::open(&loom, namespace, &workspace_id) {
            Ok(_) => panic!("mismatched ticket profile state must fail closed"),
            Err(error) => error,
        };
        assert_eq!(error.code, Code::CorruptObject);
        assert!(error.message.contains(PROJECTS_TABLE));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reads_fail_closed_on_synthetic_tickets_table_drift() {
        let path = temp_path();
        let namespace = WorkspaceId::v4_from_bytes([23; 16]);
        let workspace_id = namespace.to_string();
        let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
        stage_empty_tables(&mut loom, namespace, &workspace_id).unwrap();
        let state = state_from_tables(&loom, namespace, &workspace_id, 1).unwrap();
        loom.store()
            .control_set(
                &ticket_profile_state_key(&workspace_id).unwrap(),
                state.encode().unwrap(),
            )
            .unwrap();

        // Drift the `tickets` table without updating the stored profile-state record: simulates an
        // interrupted write that committed table rows through the tabular engine but not the
        // separate control-plane state root, the drift class the atomic commit path prevents.
        let ticket_uuid = WorkspaceId::v4_from_bytes([24; 16]).to_string();
        let ticket = crate::Ticket::new(crate::TicketInput {
            ticket_id: &ticket_uuid,
            project_id: "matrix",
            ticket_number: 1,
            ticket_type: crate::TicketType::Task,
            external_identity: None,
            fields: std::collections::BTreeMap::new(),
            policy_labels: &[],
        })
        .unwrap();
        loom.insert_row_reserved(
            namespace,
            &table_path(&workspace_id, TICKETS_TABLE),
            vec![
                Value::Text(ticket.ticket_id.clone()),
                Value::Bytes(ticket.encode().unwrap()),
            ],
        )
        .unwrap();

        // Ordinary reads must fail closed and name the drifted table.
        let error = match TicketProfileReader::open(&loom, namespace, &workspace_id) {
            Ok(_) => panic!("mismatched ticket profile state must fail closed"),
            Err(error) => error,
        };
        assert_eq!(error.code, Code::CorruptObject);
        assert!(error.message.contains(TICKETS_TABLE));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn initial_profile_creation_publishes_tables_and_state_atomically_across_reopen() {
        let path = temp_path();
        let namespace = WorkspaceId::v4_from_bytes([31; 16]);
        let workspace_id = namespace.to_string();
        {
            let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
            // First open with no stored state creates the profile. This must publish the empty
            // indexed tables (reference root) and the initial profile-state record in one atomic
            // commit. No explicit save follows here, approximating an interruption right after
            // initial creation and before any ticket mutation.
            IndexedTicketProfile::open(&mut loom, namespace, &workspace_id).unwrap();
        }

        // Reopen from disk: the atomic creation must have published the indexed tables together with
        // the profile state, so a fresh reader is consistent (fail-closed verification passes) and
        // never observes a stored state whose tables were not published.
        let reopened = loom_store::open_loom(&path).unwrap();
        let reader = TicketProfileReader::open(&reopened, namespace, &workspace_id)
            .unwrap()
            .expect("ticket profile reader present after reopen");
        assert!(reader.tickets().unwrap().is_empty());
        assert!(reader.projects().unwrap().is_empty());

        let _ = std::fs::remove_file(path);
    }
}
