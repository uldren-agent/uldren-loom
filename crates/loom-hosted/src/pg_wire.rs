use std::fmt::Debug;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::sink::{Sink, SinkExt};
use futures_util::stream;
use gluesql_core::ast::DataType;
use loom_core::error::{Code, LoomError};
use loom_core::{WorkspaceId, tabular};
use loom_result::result_view::{ResultPayload, ShowVariable, Statement};
use pgwire::api::auth::{
    DefaultServerParameterProvider, StartupHandler, finish_authentication, protocol_negotiation,
    save_startup_parameters_to_metadata,
};
use pgwire::api::portal::{Format, Portal};
use pgwire::api::query::{ExtendedQueryHandler, SimpleQueryHandler};
use pgwire::api::results::{
    DataRowEncoder, DescribePortalResponse, DescribeStatementResponse, FieldFormat, FieldInfo,
    QueryResponse, Response, Tag,
};
use pgwire::api::stmt::{NoopQueryParser, StoredStatement};
use pgwire::api::{
    ClientInfo, ClientPortalStore, PgWireConnectionState, PgWireServerHandlers, Type,
};
use pgwire::error::{ErrorInfo, PgWireError, PgWireResult};
use pgwire::messages::PgWireBackendMessage;
use pgwire::messages::PgWireFrontendMessage;
use pgwire::messages::startup::Authentication;
use pgwire::tokio::process_socket;
use tokio::net::TcpListener;

#[cfg(feature = "tls")]
use crate::HostedTlsConfig;
use crate::{HostedAuth, HostedError, HostedKernel};

#[cfg(feature = "tls")]
type PgWireTls = Option<HostedTlsConfig>;
#[cfg(not(feature = "tls"))]
type PgWireTls = ();

const PG_TRANSACTION_REJECTION_SQLSTATE: &str = "0A000";
const PG_TRANSACTION_REJECTION_MESSAGE: &str =
    "PostgreSQL-wire transactions require engine atomicity and are not supported by this facade";

#[derive(Clone)]
struct PgWireHostedAuth(HostedAuth);

#[derive(Clone)]
struct LoomPgWireStartup {
    kernel: HostedKernel,
    parameter_provider: Arc<DefaultServerParameterProvider>,
}

#[async_trait]
impl StartupHandler for LoomPgWireStartup {
    async fn on_startup<C>(
        &self,
        client: &mut C,
        message: PgWireFrontendMessage,
    ) -> PgWireResult<()>
    where
        C: ClientInfo + Sink<PgWireBackendMessage> + Unpin + Send + Sync,
        C::Error: Debug,
        PgWireError: From<<C as Sink<PgWireBackendMessage>>::Error>,
    {
        match message {
            PgWireFrontendMessage::Startup(ref startup) => {
                protocol_negotiation(client, startup).await?;
                save_startup_parameters_to_metadata(client, startup);
                client.set_state(PgWireConnectionState::AuthenticationInProgress);
                client
                    .send(PgWireBackendMessage::Authentication(
                        Authentication::CleartextPassword,
                    ))
                    .await?;
            }
            PgWireFrontendMessage::PasswordMessageFamily(pwd) => {
                let password = pwd.into_password()?;
                let auth = pg_auth_from_client(client, password.password.as_bytes())?;
                self.kernel
                    .read(&auth, |_| Ok(()))
                    .map_err(pg_error_from_loom)?;
                client.session_extensions().insert(PgWireHostedAuth(auth));
                finish_authentication(client, self.parameter_provider.as_ref()).await?;
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone)]
struct LoomPgWireQuery {
    kernel: HostedKernel,
    workspace: String,
    database: String,
    query_parser: Arc<NoopQueryParser>,
}

#[async_trait]
impl SimpleQueryHandler for LoomPgWireQuery {
    async fn do_query<C>(&self, client: &mut C, query: &str) -> PgWireResult<Vec<Response>>
    where
        C: ClientInfo + ClientPortalStore + Sink<PgWireBackendMessage> + Unpin + Send + Sync,
        C::Error: Debug,
        PgWireError: From<<C as Sink<PgWireBackendMessage>>::Error>,
    {
        reject_transaction_boundary(query)?;
        let auth = pg_auth_from_session(client)?;
        if let Some(response) = self.catalog_responses(&auth.0, query)? {
            return Ok(response);
        }
        if let Some(response) = self.pgvector_responses(&auth.0, query)? {
            return Ok(response);
        }
        if let Some(response) = self.analytical_responses(&auth.0, query)? {
            return Ok(response);
        }
        self.execute_statement(&auth.0, query)
    }
}

#[async_trait]
impl ExtendedQueryHandler for LoomPgWireQuery {
    type Statement = String;
    type QueryParser = NoopQueryParser;

    fn query_parser(&self) -> Arc<Self::QueryParser> {
        self.query_parser.clone()
    }

    async fn do_query<C>(
        &self,
        client: &mut C,
        portal: &Portal<Self::Statement>,
        _max_rows: usize,
    ) -> PgWireResult<Response>
    where
        C: ClientInfo + ClientPortalStore + Sink<PgWireBackendMessage> + Unpin + Send + Sync,
        C::Error: Debug,
        PgWireError: From<<C as Sink<PgWireBackendMessage>>::Error>,
    {
        let query = &portal.statement.statement;
        reject_transaction_boundary(query)?;
        let auth = pg_auth_from_session(client)?;
        let query = if portal.parameter_len() == 0 {
            reject_parameter_markers(query)?;
            query.clone()
        } else {
            self.rewrite_bound_query(&auth.0, portal)?
        };
        if let Some(mut responses) = self.catalog_responses(&auth.0, &query)? {
            if responses.len() != 1 {
                return Err(pg_user_error(
                    "0A000",
                    "PostgreSQL-wire extended query supports one statement per portal",
                ));
            }
            return Ok(responses.remove(0));
        }
        if let Some(mut responses) = self.pgvector_responses(&auth.0, &query)? {
            if responses.len() != 1 {
                return Err(pg_user_error(
                    "0A000",
                    "PostgreSQL-wire extended query supports one statement per portal",
                ));
            }
            return Ok(responses.remove(0));
        }
        if let Some(mut responses) = self.analytical_responses(&auth.0, &query)? {
            if responses.len() != 1 {
                return Err(pg_user_error(
                    "0A000",
                    "PostgreSQL-wire extended query supports one statement per portal",
                ));
            }
            return Ok(responses.remove(0));
        }
        let mut responses = self.execute_statement(&auth.0, &query)?;
        if responses.len() != 1 {
            return Err(pg_user_error(
                "0A000",
                "PostgreSQL-wire extended query supports one statement per portal",
            ));
        }
        Ok(responses.remove(0))
    }

    async fn do_describe_statement<C>(
        &self,
        client: &mut C,
        stmt: &StoredStatement<Self::Statement>,
    ) -> PgWireResult<DescribeStatementResponse>
    where
        C: ClientInfo + ClientPortalStore + Sink<PgWireBackendMessage> + Unpin + Send + Sync,
        C::Error: Debug,
        PgWireError: From<<C as Sink<PgWireBackendMessage>>::Error>,
    {
        let auth = pg_auth_from_session(client)?;
        let param_types = self.resolve_statement_parameter_types(&auth.0, stmt)?;
        let describe_query = if query_has_parameter_markers(&stmt.statement) {
            replace_parameter_markers_with_null(&stmt.statement)
        } else {
            stmt.statement.clone()
        };
        let fields = self.describe_query_fields(&auth.0, &describe_query, None)?;
        Ok(DescribeStatementResponse::new(param_types, fields))
    }

    async fn do_describe_portal<C>(
        &self,
        client: &mut C,
        portal: &Portal<Self::Statement>,
    ) -> PgWireResult<DescribePortalResponse>
    where
        C: ClientInfo + ClientPortalStore + Sink<PgWireBackendMessage> + Unpin + Send + Sync,
        C::Error: Debug,
        PgWireError: From<<C as Sink<PgWireBackendMessage>>::Error>,
    {
        let auth = pg_auth_from_session(client)?;
        let query = if portal.parameter_len() == 0 {
            reject_parameter_markers(&portal.statement.statement)?;
            portal.statement.statement.clone()
        } else {
            self.rewrite_bound_query(&auth.0, portal)?
        };
        Ok(DescribePortalResponse::new(self.describe_query_fields(
            &auth.0,
            &query,
            Some(&portal.result_column_format),
        )?))
    }
}

impl LoomPgWireQuery {
    fn execute_statement(&self, auth: &HostedAuth, query: &str) -> PgWireResult<Vec<Response>> {
        if let Some(response) = self.catalog_responses(auth, query)? {
            return Ok(response);
        }
        let bytes = self
            .kernel
            .sql()
            .exec_cbor(auth, &self.workspace, &self.database, query)
            .map_err(pg_error_from_hosted)?;
        pg_responses_from_cbor(&bytes)
    }

    fn resolve_statement_parameter_types(
        &self,
        auth: &HostedAuth,
        stmt: &StoredStatement<String>,
    ) -> PgWireResult<Vec<Type>> {
        if stmt.parameter_types.is_empty() && !query_has_parameter_markers(&stmt.statement) {
            return Ok(Vec::new());
        }
        let inferred = self.infer_parameter_types(auth, &stmt.statement)?;
        let len = stmt.parameter_types.len().max(inferred.len());
        let mut out = Vec::with_capacity(len);
        for index in 0..len {
            out.push(
                stmt.parameter_types
                    .get(index)
                    .cloned()
                    .flatten()
                    .or_else(|| inferred.get(index).cloned().flatten())
                    .unwrap_or(Type::UNKNOWN),
            );
        }
        Ok(out)
    }

    fn infer_parameter_types(
        &self,
        auth: &HostedAuth,
        query: &str,
    ) -> PgWireResult<Vec<Option<Type>>> {
        self.kernel
            .sql()
            .infer_parameter_types(auth, &self.workspace, &self.database, query)
            .map_err(pg_error_from_hosted)
            .map(|types| {
                types
                    .into_iter()
                    .map(|ty| ty.map(pg_type_from_sql))
                    .collect()
            })
    }

    fn rewrite_bound_query(
        &self,
        auth: &HostedAuth,
        portal: &Portal<String>,
    ) -> PgWireResult<String> {
        let parameter_types = self.resolve_statement_parameter_types(auth, &portal.statement)?;
        rewrite_parameter_markers(&portal.statement.statement, portal, &parameter_types)
    }

    fn describe_query_fields(
        &self,
        auth: &HostedAuth,
        query: &str,
        format: Option<&Format>,
    ) -> PgWireResult<Vec<FieldInfo>> {
        if let Some(response) = self.catalog_responses(auth, query)? {
            return response_fields(response);
        }
        if let Some(response) = self.pgvector_responses(auth, query)? {
            return response_fields(response);
        }
        if let Some(response) = self.analytical_responses(auth, query)? {
            return response_fields(response);
        }
        if !query_returns_rows(query) {
            return Ok(Vec::new());
        }
        reject_transaction_boundary(query)?;
        let responses = self.execute_statement(auth, query)?;
        if responses.len() != 1 {
            return Err(pg_user_error(
                "0A000",
                "PostgreSQL-wire extended query supports one statement per portal",
            ));
        }
        let Some(response) = responses.into_iter().next() else {
            return Ok(Vec::new());
        };
        match response {
            Response::Query(query) => Ok(query
                .row_schema
                .iter()
                .enumerate()
                .map(|(idx, field)| {
                    FieldInfo::new(
                        field.name().to_owned(),
                        field.table_id(),
                        field.column_id(),
                        field.datatype().clone(),
                        format
                            .map(|format| format.format_for(idx))
                            .unwrap_or(FieldFormat::Text),
                    )
                })
                .collect()),
            Response::EmptyQuery | Response::Execution(_) => Ok(Vec::new()),
            _ => Err(pg_user_error(
                "0A000",
                "PostgreSQL-wire describe is not supported for this statement",
            )),
        }
    }

    fn catalog_responses(
        &self,
        auth: &HostedAuth,
        query: &str,
    ) -> PgWireResult<Option<Vec<Response>>> {
        if psql_list_tables_catalog_query(query) {
            return Ok(Some(vec![self.psql_list_tables_response(auth)?]));
        }
        if psql_describe_relation_lookup_catalog_query(query) {
            return Ok(Some(vec![
                self.psql_describe_relation_lookup_response(auth, query)?,
            ]));
        }
        if psql_relation_flags_catalog_query(query) {
            return Ok(Some(vec![psql_relation_flags_response()?]));
        }
        if psql_attribute_catalog_query(query) {
            return Ok(Some(vec![self.psql_attribute_response(auth, query)?]));
        }
        if psql_policy_catalog_query(query) {
            return Ok(Some(vec![psql_policy_response()?]));
        }
        if psql_statistic_ext_catalog_query(query) {
            return Ok(Some(vec![psql_statistic_ext_response()?]));
        }
        if psql_publication_catalog_query(query) {
            return Ok(Some(vec![psql_publication_response()?]));
        }
        if psql_inherits_catalog_query(query) {
            return Ok(Some(vec![psql_inherits_response()?]));
        }
        Ok(None)
    }

    fn psql_list_tables_response(&self, auth: &HostedAuth) -> PgWireResult<Response> {
        let owner = auth
            .principal
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "loom".to_string());
        let rows = self
            .list_sql_tables(auth)?
            .into_iter()
            .map(|table| {
                vec![
                    tabular::Value::Text("public".to_string()),
                    tabular::Value::Text(table),
                    tabular::Value::Text("table".to_string()),
                    tabular::Value::Text(owner.clone()),
                ]
            })
            .collect();
        query_response(
            vec![
                "Schema".to_string(),
                "Name".to_string(),
                "Type".to_string(),
                "Owner".to_string(),
            ],
            rows,
        )
    }

    fn list_sql_tables(&self, auth: &HostedAuth) -> PgWireResult<Vec<String>> {
        let bytes = self
            .kernel
            .sql()
            .exec_cbor(auth, &self.workspace, &self.database, "SHOW TABLES")
            .map_err(pg_error_from_hosted)?;
        let payload = loom_result::result_view::decode(&bytes).map_err(pg_error_from_loom)?;
        match payload {
            ResultPayload::Statements(statements) => {
                for statement in statements {
                    if let Statement::ShowVariable(ShowVariable::Tables(values)) = statement {
                        return Ok(values);
                    }
                }
                Ok(Vec::new())
            }
            ResultPayload::Reader(_) => Err(pg_user_error(
                "0A000",
                "PostgreSQL-wire catalog reader payloads are not supported",
            )),
        }
    }

    fn list_sql_columns(
        &self,
        auth: &HostedAuth,
        table: &str,
    ) -> PgWireResult<Vec<loom_result::result_view::Column>> {
        let sql = format!("SHOW COLUMNS FROM {table}");
        let bytes = self
            .kernel
            .sql()
            .exec_cbor(auth, &self.workspace, &self.database, &sql)
            .map_err(pg_error_from_hosted)?;
        let payload = loom_result::result_view::decode(&bytes).map_err(pg_error_from_loom)?;
        match payload {
            ResultPayload::Statements(statements) => {
                for statement in statements {
                    if let Statement::ShowColumns(columns) = statement {
                        return Ok(columns);
                    }
                }
                Ok(Vec::new())
            }
            ResultPayload::Reader(_) => Err(pg_user_error(
                "0A000",
                "PostgreSQL-wire catalog column reader payloads are not supported",
            )),
        }
    }

    fn psql_describe_relation_lookup_response(
        &self,
        auth: &HostedAuth,
        query: &str,
    ) -> PgWireResult<Response> {
        let filter = psql_relation_name_filter(query);
        let rows = self
            .list_sql_tables(auth)?
            .into_iter()
            .enumerate()
            .filter(|(_, table)| filter.as_ref().is_none_or(|name| name == table))
            .map(|(idx, table)| {
                vec![
                    tabular::Value::Int(psql_table_oid(idx)),
                    tabular::Value::Text("public".to_string()),
                    tabular::Value::Text(table),
                ]
            })
            .collect();
        query_response(
            vec![
                "oid".to_string(),
                "nspname".to_string(),
                "relname".to_string(),
            ],
            rows,
        )
    }

    fn psql_attribute_response(&self, auth: &HostedAuth, query: &str) -> PgWireResult<Response> {
        let verbose = psql_verbose_attribute_catalog_query(query);
        let Some(table) = self.table_for_oid(auth, psql_oid_filter(query)?)? else {
            return query_response(psql_attribute_labels(verbose), Vec::new());
        };
        let rows = self
            .list_sql_columns(auth, &table)?
            .into_iter()
            .map(|column| {
                let mut row = vec![
                    tabular::Value::Text(column.name),
                    tabular::Value::Text(pg_catalog_type_name(&column.type_name).to_string()),
                    tabular::Value::Null,
                    tabular::Value::Bool(false),
                    tabular::Value::Null,
                    tabular::Value::Text(String::new()),
                    tabular::Value::Text(String::new()),
                ];
                if verbose {
                    row.push(tabular::Value::Text("plain".to_string()));
                    row.push(tabular::Value::Null);
                    row.push(tabular::Value::Null);
                    row.push(tabular::Value::Null);
                }
                row
            })
            .collect();
        query_response(psql_attribute_labels(verbose), rows)
    }

    fn table_for_oid(&self, auth: &HostedAuth, oid: i64) -> PgWireResult<Option<String>> {
        Ok(self
            .list_sql_tables(auth)?
            .into_iter()
            .enumerate()
            .find_map(|(idx, table)| (psql_table_oid(idx) == oid).then_some(table)))
    }

    fn pgvector_responses(
        &self,
        auth: &HostedAuth,
        query: &str,
    ) -> PgWireResult<Option<Vec<Response>>> {
        let Some(parsed) = parse_pgvector_query(query)? else {
            return Ok(None);
        };
        let info = self
            .kernel
            .data()
            .vector_info(auth, &self.workspace, &parsed.collection)
            .map_err(pg_error_from_hosted)?;
        if info.metric != parsed.operator.metric() {
            return Err(pg_user_error(
                "0A000",
                "pgvector-style operator does not match the Loom vector set metric",
            ));
        }
        let hits = self
            .kernel
            .data()
            .vector_search(
                auth,
                &self.workspace,
                &parsed.collection,
                &parsed.query,
                parsed.limit,
            )
            .map_err(pg_error_from_hosted)?;
        let rows = hits
            .into_iter()
            .map(|hit| {
                vec![
                    tabular::Value::Text(hit.id),
                    tabular::Value::Float(parsed.operator.distance(hit.score) as f64),
                ]
            })
            .collect();
        Ok(Some(vec![query_response(
            vec!["id".to_string(), "distance".to_string()],
            rows,
        )?]))
    }

    fn analytical_responses(
        &self,
        auth: &HostedAuth,
        query: &str,
    ) -> PgWireResult<Option<Vec<Response>>> {
        let Some(parsed) = parse_columnar_analytical_query(query)? else {
            return Ok(None);
        };
        match parsed.projection {
            AnalyticalProjection::Count => {
                let count = self
                    .kernel
                    .data()
                    .columnar_rows(auth, &self.workspace, &parsed.dataset)
                    .map_err(pg_error_from_hosted)?;
                Ok(Some(vec![query_response(
                    vec!["count".to_string()],
                    vec![vec![tabular::Value::U64(count as u64)]],
                )?]))
            }
            AnalyticalProjection::Columns(columns) => {
                let schema = self
                    .kernel
                    .data()
                    .columnar_columns(auth, &self.workspace, &parsed.dataset)
                    .map_err(pg_error_from_hosted)?;
                let labels = match columns {
                    ColumnarProjection::All => {
                        schema.into_iter().map(|(name, _)| name).collect::<Vec<_>>()
                    }
                    ColumnarProjection::Selected(columns) => {
                        let available = schema
                            .into_iter()
                            .map(|(name, _)| name)
                            .collect::<std::collections::BTreeSet<_>>();
                        for column in &columns {
                            if !available.contains(column) {
                                return Err(pg_user_error(
                                    "42703",
                                    "columnar analytical query selected an unknown column",
                                ));
                            }
                        }
                        columns
                    }
                };
                let refs = labels.iter().map(String::as_str).collect::<Vec<_>>();
                let mut rows = self
                    .kernel
                    .data()
                    .columnar_select(auth, &self.workspace, &parsed.dataset, &refs, None)
                    .map_err(pg_error_from_hosted)?;
                if let Some(limit) = parsed.limit {
                    rows.truncate(limit);
                }
                Ok(Some(vec![query_response(labels, rows)?]))
            }
        }
    }
}

#[derive(Clone)]
struct LoomPgWireHandlers {
    startup: Arc<LoomPgWireStartup>,
    query: Arc<LoomPgWireQuery>,
}

impl PgWireServerHandlers for LoomPgWireHandlers {
    fn startup_handler(&self) -> Arc<impl StartupHandler> {
        self.startup.clone()
    }

    fn simple_query_handler(&self) -> Arc<impl SimpleQueryHandler> {
        self.query.clone()
    }

    fn extended_query_handler(&self) -> Arc<impl ExtendedQueryHandler> {
        self.query.clone()
    }
}

pub async fn serve_sql_pg_wire<S>(
    listener: TcpListener,
    kernel: HostedKernel,
    workspace: impl Into<String>,
    database: impl Into<String>,
    shutdown: S,
) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    serve_sql_pg_wire_inner(
        listener,
        kernel,
        workspace,
        database,
        pg_wire_no_tls_config(),
        shutdown,
    )
    .await
}

#[cfg(feature = "tls")]
pub async fn serve_sql_pg_wire_with_tls<S>(
    listener: TcpListener,
    tls: HostedTlsConfig,
    kernel: HostedKernel,
    workspace: impl Into<String>,
    database: impl Into<String>,
    shutdown: S,
) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    serve_sql_pg_wire_inner(listener, kernel, workspace, database, Some(tls), shutdown).await
}

async fn serve_sql_pg_wire_inner<S>(
    listener: TcpListener,
    kernel: HostedKernel,
    workspace: impl Into<String>,
    database: impl Into<String>,
    #[cfg_attr(not(feature = "tls"), allow(unused_variables))] tls: PgWireTls,
    shutdown: S,
) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    let mut parameters = DefaultServerParameterProvider::default();
    parameters.server_version = "16.6-loom".to_string();
    parameters.is_superuser = false;
    let handlers = Arc::new(LoomPgWireHandlers {
        startup: Arc::new(LoomPgWireStartup {
            kernel: kernel.clone(),
            parameter_provider: Arc::new(parameters),
        }),
        query: Arc::new(LoomPgWireQuery {
            kernel,
            workspace: workspace.into(),
            database: database.into(),
            query_parser: Arc::new(NoopQueryParser::new()),
        }),
    });
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => return Ok(()),
            incoming = listener.accept() => {
                let (socket, _) = incoming?;
                let handlers = handlers.clone();
                #[cfg(feature = "tls")]
                let tls_acceptor = tls.as_ref().map(HostedTlsConfig::acceptor);
                #[cfg(not(feature = "tls"))]
                let tls_acceptor = None;
                tokio::spawn(async move {
                    let _ = process_socket(socket, tls_acceptor, handlers).await;
                });
            }
        }
    }
}

#[cfg(feature = "tls")]
fn pg_wire_no_tls_config() -> PgWireTls {
    None
}

#[cfg(not(feature = "tls"))]
fn pg_wire_no_tls_config() -> PgWireTls {}

fn pg_auth_from_client<C>(client: &C, password: &[u8]) -> PgWireResult<HostedAuth>
where
    C: ClientInfo,
{
    let Some(user) = client.metadata().get("user") else {
        return Err(PgWireError::UserNameRequired);
    };
    let principal = WorkspaceId::parse(user).map_err(pg_error_from_loom)?;
    let password = std::str::from_utf8(password)
        .map_err(|_| pg_user_error("28P01", "password is not valid UTF-8"))?;
    let session = format!("pg-wire:{principal}:{}", client.socket_addr());
    Ok(HostedAuth::passphrase(principal, password, session))
}

fn pg_auth_from_session<C>(client: &C) -> PgWireResult<Arc<PgWireHostedAuth>>
where
    C: ClientInfo,
{
    client
        .session_extensions()
        .get::<PgWireHostedAuth>()
        .ok_or_else(|| pg_user_error("28000", "PostgreSQL-wire session is not authenticated"))
}

fn reject_transaction_boundary(query: &str) -> PgWireResult<()> {
    if pg_transaction_boundary(query) {
        return Err(pg_user_error(
            PG_TRANSACTION_REJECTION_SQLSTATE,
            PG_TRANSACTION_REJECTION_MESSAGE,
        ));
    }
    Ok(())
}

fn pg_transaction_boundary(query: &str) -> bool {
    let normalized = query.trim_start().to_ascii_uppercase();
    starts_with_sql_command(&normalized, "BEGIN")
        || starts_with_sql_command(&normalized, "COMMIT")
        || starts_with_sql_command(&normalized, "ROLLBACK")
        || starts_with_sql_phrase(&normalized, "START TRANSACTION")
        || starts_with_sql_command(&normalized, "SAVEPOINT")
        || starts_with_sql_phrase(&normalized, "RELEASE SAVEPOINT")
        || starts_with_sql_phrase(&normalized, "ROLLBACK TO")
}

fn starts_with_sql_command(value: &str, command: &str) -> bool {
    value.strip_prefix(command).is_some_and(sql_boundary_after)
}

fn starts_with_sql_phrase(value: &str, phrase: &str) -> bool {
    value.strip_prefix(phrase).is_some_and(sql_boundary_after)
}

fn sql_boundary_after(rest: &str) -> bool {
    rest.is_empty()
        || rest
            .chars()
            .next()
            .is_some_and(|ch| ch.is_whitespace() || ch == ';')
}

fn reject_parameter_markers(query: &str) -> PgWireResult<()> {
    if query_has_parameter_markers(query) {
        return Err(pg_user_error(
            "0A000",
            "PostgreSQL-wire parameter binding is not supported yet",
        ));
    }
    Ok(())
}

fn query_has_parameter_markers(query: &str) -> bool {
    let mut chars = query.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double => {
                if in_single && chars.peek().is_some_and(|next| *next == '\'') {
                    chars.next();
                } else {
                    in_single = !in_single;
                }
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            '$' if !in_single
                && !in_double
                && chars.peek().is_some_and(|next| next.is_ascii_digit()) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn rewrite_parameter_markers(
    query: &str,
    portal: &Portal<String>,
    parameter_types: &[Type],
) -> PgWireResult<String> {
    let mut out = String::with_capacity(query.len());
    let mut chars = query.char_indices().peekable();
    let mut in_single = false;
    let mut in_double = false;
    while let Some((_, ch)) = chars.next() {
        match ch {
            '\'' if !in_double => {
                out.push(ch);
                if in_single && chars.peek().is_some_and(|(_, next)| *next == '\'') {
                    let (_, next) = chars.next().unwrap();
                    out.push(next);
                } else {
                    in_single = !in_single;
                }
            }
            '"' if !in_single => {
                out.push(ch);
                in_double = !in_double;
            }
            '$' if !in_single
                && !in_double
                && chars.peek().is_some_and(|(_, next)| next.is_ascii_digit()) =>
            {
                let mut marker = String::new();
                while let Some((_, next)) = chars.peek() {
                    if next.is_ascii_digit() {
                        marker.push(*next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let index = marker
                    .parse::<usize>()
                    .ok()
                    .and_then(|value| value.checked_sub(1))
                    .ok_or_else(|| pg_user_error("08P01", "invalid PostgreSQL parameter marker"))?;
                let parameter = portal.parameters.get(index).ok_or_else(|| {
                    pg_user_error("08P01", "PostgreSQL parameter marker has no bound value")
                })?;
                let ty = parameter_types.get(index).unwrap_or(&Type::UNKNOWN);
                let binary = portal.parameter_format.is_binary(index);
                out.push_str(&pg_parameter_literal(parameter.as_ref(), ty, binary)?);
            }
            _ => out.push(ch),
        }
    }
    Ok(out)
}

fn replace_parameter_markers_with_null(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    let mut chars = query.char_indices().peekable();
    let mut in_single = false;
    let mut in_double = false;
    while let Some((_, ch)) = chars.next() {
        match ch {
            '\'' if !in_double => {
                out.push(ch);
                if in_single && chars.peek().is_some_and(|(_, next)| *next == '\'') {
                    let (_, next) = chars.next().unwrap();
                    out.push(next);
                } else {
                    in_single = !in_single;
                }
            }
            '"' if !in_single => {
                out.push(ch);
                in_double = !in_double;
            }
            '$' if !in_single
                && !in_double
                && chars.peek().is_some_and(|(_, next)| next.is_ascii_digit()) =>
            {
                while let Some((_, next)) = chars.peek() {
                    if next.is_ascii_digit() {
                        chars.next();
                    } else {
                        break;
                    }
                }
                out.push_str("NULL");
            }
            _ => out.push(ch),
        }
    }
    out
}

fn pg_parameter_literal(
    parameter: Option<&bytes::Bytes>,
    ty: &Type,
    binary: bool,
) -> PgWireResult<String> {
    let Some(parameter) = parameter else {
        return Ok("NULL".to_string());
    };
    if binary {
        return pg_binary_parameter_literal(parameter, ty);
    }
    let value = std::str::from_utf8(parameter)
        .map_err(|_| pg_user_error("22021", "PostgreSQL text parameter is not valid UTF-8"))?;
    pg_text_parameter_literal(value, ty)
}

fn pg_binary_parameter_literal(parameter: &[u8], ty: &Type) -> PgWireResult<String> {
    if *ty == Type::BOOL {
        return match parameter {
            [0] => Ok("FALSE".to_string()),
            [1] => Ok("TRUE".to_string()),
            _ => Err(pg_user_error(
                "22P03",
                "PostgreSQL binary boolean parameter has invalid length",
            )),
        };
    }
    if *ty == Type::INT2 {
        let bytes = parameter.try_into().map_err(|_| {
            pg_user_error(
                "22P03",
                "PostgreSQL binary int2 parameter has invalid length",
            )
        })?;
        return Ok(i16::from_be_bytes(bytes).to_string());
    }
    if *ty == Type::INT4 {
        let bytes = parameter.try_into().map_err(|_| {
            pg_user_error(
                "22P03",
                "PostgreSQL binary int4 parameter has invalid length",
            )
        })?;
        return Ok(i32::from_be_bytes(bytes).to_string());
    }
    if *ty == Type::INT8 {
        let bytes = parameter.try_into().map_err(|_| {
            pg_user_error(
                "22P03",
                "PostgreSQL binary int8 parameter has invalid length",
            )
        })?;
        return Ok(i64::from_be_bytes(bytes).to_string());
    }
    if *ty == Type::FLOAT4 {
        let bytes: [u8; 4] = parameter.try_into().map_err(|_| {
            pg_user_error(
                "22P03",
                "PostgreSQL binary float4 parameter has invalid length",
            )
        })?;
        return Ok(f32::from_bits(u32::from_be_bytes(bytes)).to_string());
    }
    if *ty == Type::FLOAT8 {
        let bytes: [u8; 8] = parameter.try_into().map_err(|_| {
            pg_user_error(
                "22P03",
                "PostgreSQL binary float8 parameter has invalid length",
            )
        })?;
        return Ok(f64::from_bits(u64::from_be_bytes(bytes)).to_string());
    }
    if *ty == Type::TEXT || *ty == Type::VARCHAR || *ty == Type::BPCHAR || *ty == Type::UNKNOWN {
        let value = std::str::from_utf8(parameter).map_err(|_| {
            pg_user_error(
                "22021",
                "PostgreSQL binary text parameter is not valid UTF-8",
            )
        })?;
        return Ok(sql_quoted(value));
    }
    Err(pg_user_error(
        "0A000",
        "PostgreSQL binary parameter type is not supported yet",
    ))
}

fn pg_text_parameter_literal(value: &str, ty: &Type) -> PgWireResult<String> {
    if *ty == Type::BOOL {
        return match value.to_ascii_lowercase().as_str() {
            "t" | "true" | "1" => Ok("TRUE".to_string()),
            "f" | "false" | "0" => Ok("FALSE".to_string()),
            _ => Err(pg_user_error(
                "22P02",
                "invalid PostgreSQL boolean parameter",
            )),
        };
    }
    if matches_numeric_pg_type(ty) {
        if value
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '+' | '-' | '.'))
        {
            return Ok(value.to_string());
        }
        return Err(pg_user_error(
            "22P02",
            "invalid PostgreSQL numeric parameter",
        ));
    }
    Ok(sql_quoted(value))
}

fn matches_numeric_pg_type(ty: &Type) -> bool {
    *ty == Type::INT2
        || *ty == Type::INT4
        || *ty == Type::INT8
        || *ty == Type::FLOAT4
        || *ty == Type::FLOAT8
        || *ty == Type::NUMERIC
}

fn sql_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn pg_type_from_sql(data_type: DataType) -> Type {
    match data_type {
        DataType::Boolean => Type::BOOL,
        DataType::Int8 | DataType::Int16 => Type::INT2,
        DataType::Int32 => Type::INT4,
        DataType::Int | DataType::Int128 => Type::INT8,
        DataType::Uint8 | DataType::Uint16 | DataType::Uint32 => Type::INT4,
        DataType::Uint64 | DataType::Uint128 => Type::INT8,
        DataType::Float32 => Type::FLOAT4,
        DataType::Float => Type::FLOAT8,
        DataType::Bytea => Type::BYTEA,
        DataType::Date => Type::DATE,
        DataType::Timestamp => Type::TIMESTAMP,
        DataType::Time => Type::TIME,
        DataType::Uuid => Type::UUID,
        DataType::Decimal => Type::NUMERIC,
        DataType::Inet => Type::INET,
        DataType::Text | DataType::Interval | DataType::Map | DataType::List | DataType::Point => {
            Type::TEXT
        }
    }
}

fn query_returns_rows(query: &str) -> bool {
    matches!(
        first_statement_keyword(query).as_deref(),
        Some("SELECT" | "SHOW")
    )
}

fn psql_list_tables_catalog_query(query: &str) -> bool {
    let normalized = normalized_catalog_query(query);
    normalized.contains("from pg_catalog.pg_class c")
        && normalized.contains("pg_catalog.pg_namespace n")
        && normalized.contains("pg_catalog.pg_get_userbyid")
        && normalized.contains("pg_catalog.pg_table_is_visible")
}

fn psql_describe_relation_lookup_catalog_query(query: &str) -> bool {
    let normalized = normalized_catalog_query(query);
    normalized.contains("select c.oid, n.nspname, c.relname")
        && normalized.contains("from pg_catalog.pg_class c")
        && normalized.contains("pg_catalog.pg_namespace n")
        && normalized.contains("pg_table_is_visible")
}

fn psql_relation_flags_catalog_query(query: &str) -> bool {
    let normalized = normalized_catalog_query(query);
    normalized.contains("select c.relchecks, c.relkind")
        && normalized.contains("from pg_catalog.pg_class c")
        && normalized.contains("where c.oid =")
}

fn psql_attribute_catalog_query(query: &str) -> bool {
    let normalized = normalized_catalog_query(query);
    normalized.contains("select a.attname")
        && normalized.contains("from pg_catalog.pg_attribute a")
        && normalized.contains("where a.attrelid =")
}

fn psql_verbose_attribute_catalog_query(query: &str) -> bool {
    let normalized = normalized_catalog_query(query);
    normalized.contains("a.attstorage") && normalized.contains("pg_catalog.col_description")
}

fn psql_policy_catalog_query(query: &str) -> bool {
    normalized_catalog_query(query).contains("from pg_catalog.pg_policy pol")
}

fn psql_policy_response() -> PgWireResult<Response> {
    query_response(
        vec![
            "polname".to_string(),
            "polpermissive".to_string(),
            "array_to_string".to_string(),
            "pg_get_expr".to_string(),
            "pg_get_expr".to_string(),
            "cmd".to_string(),
        ],
        Vec::new(),
    )
}

fn psql_statistic_ext_catalog_query(query: &str) -> bool {
    normalized_catalog_query(query).contains("from pg_catalog.pg_statistic_ext")
}

fn psql_statistic_ext_response() -> PgWireResult<Response> {
    query_response(
        vec![
            "oid".to_string(),
            "stxrelid".to_string(),
            "nsp".to_string(),
            "stxname".to_string(),
            "columns".to_string(),
            "ndist_enabled".to_string(),
            "deps_enabled".to_string(),
            "mcv_enabled".to_string(),
            "stxstattarget".to_string(),
        ],
        Vec::new(),
    )
}

fn psql_publication_catalog_query(query: &str) -> bool {
    normalized_catalog_query(query).contains("pg_catalog.pg_publication")
}

fn psql_publication_response() -> PgWireResult<Response> {
    query_response(
        vec![
            "pubname".to_string(),
            "?column?".to_string(),
            "?column?".to_string(),
        ],
        Vec::new(),
    )
}

fn psql_inherits_catalog_query(query: &str) -> bool {
    normalized_catalog_query(query).contains("pg_catalog.pg_inherits")
}

fn psql_inherits_response() -> PgWireResult<Response> {
    query_response(vec!["regclass".to_string()], Vec::new())
}

fn psql_relation_flags_response() -> PgWireResult<Response> {
    query_response(
        vec![
            "relchecks".to_string(),
            "relkind".to_string(),
            "relhasindex".to_string(),
            "relhasrules".to_string(),
            "relhastriggers".to_string(),
            "relrowsecurity".to_string(),
            "relforcerowsecurity".to_string(),
            "relhasoids".to_string(),
            "relispartition".to_string(),
            "?column?".to_string(),
            "reltablespace".to_string(),
            "case".to_string(),
            "relpersistence".to_string(),
            "relreplident".to_string(),
            "amname".to_string(),
        ],
        vec![vec![
            tabular::Value::Int(0),
            tabular::Value::Text("r".to_string()),
            tabular::Value::Bool(false),
            tabular::Value::Bool(false),
            tabular::Value::Bool(false),
            tabular::Value::Bool(false),
            tabular::Value::Bool(false),
            tabular::Value::Bool(false),
            tabular::Value::Bool(false),
            tabular::Value::Text(String::new()),
            tabular::Value::Int(0),
            tabular::Value::Text(String::new()),
            tabular::Value::Text("p".to_string()),
            tabular::Value::Text("d".to_string()),
            tabular::Value::Text("heap".to_string()),
        ]],
    )
}

fn psql_attribute_labels(verbose: bool) -> Vec<String> {
    let mut labels = vec![
        "attname".to_string(),
        "format_type".to_string(),
        "pg_get_expr".to_string(),
        "attnotnull".to_string(),
        "attcollation".to_string(),
        "attidentity".to_string(),
        "attgenerated".to_string(),
    ];
    if verbose {
        labels.push("attstorage".to_string());
        labels.push("attcompression".to_string());
        labels.push("attstattarget".to_string());
        labels.push("col_description".to_string());
    }
    labels
}

fn psql_relation_name_filter(query: &str) -> Option<String> {
    let marker = "c.relname OPERATOR(pg_catalog.~) '^(";
    let start = query.find(marker)? + marker.len();
    let rest = &query[start..];
    let end = rest.find(")$'")?;
    Some(rest[..end].to_string())
}

fn psql_table_oid(index: usize) -> i64 {
    16_384 + index as i64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PgVectorOperator {
    L2,
    Cosine,
    Dot,
}

impl PgVectorOperator {
    fn token(self) -> &'static str {
        match self {
            Self::L2 => "<->",
            Self::Cosine => "<=>",
            Self::Dot => "<#>",
        }
    }

    fn metric(self) -> loom_core::Metric {
        match self {
            Self::L2 => loom_core::Metric::L2,
            Self::Cosine => loom_core::Metric::Cosine,
            Self::Dot => loom_core::Metric::Dot,
        }
    }

    fn distance(self, score: f32) -> f32 {
        match self {
            Self::L2 => (-score).max(0.0).sqrt(),
            Self::Cosine => 1.0 - score,
            Self::Dot => -score,
        }
    }
}

struct PgVectorQuery {
    collection: String,
    operator: PgVectorOperator,
    query: Vec<f32>,
    limit: usize,
}

fn parse_pgvector_query(query: &str) -> PgWireResult<Option<PgVectorQuery>> {
    let Some(operator) = pgvector_operator(query) else {
        return Ok(None);
    };
    let normalized = normalized_catalog_query(query);
    if !normalized.starts_with("select ") || !normalized.contains(" order by ") {
        return Ok(None);
    }
    let collection = parse_relation_after_from(query).ok_or_else(|| {
        pg_user_error(
            "42601",
            "pgvector-style query must select from a Loom vector set",
        )
    })?;
    let vector_literal = parse_pgvector_literal(query, operator)?;
    let limit = parse_limit(query).unwrap_or(10);
    Ok(Some(PgVectorQuery {
        collection,
        operator,
        query: vector_literal,
        limit,
    }))
}

fn pgvector_operator(query: &str) -> Option<PgVectorOperator> {
    if query.contains("<->") {
        Some(PgVectorOperator::L2)
    } else if query.contains("<=>") {
        Some(PgVectorOperator::Cosine)
    } else if query.contains("<#>") {
        Some(PgVectorOperator::Dot)
    } else {
        None
    }
}

fn parse_relation_after_from(query: &str) -> Option<String> {
    let lower = query.to_ascii_lowercase();
    let start = lower.find(" from ")? + " from ".len();
    let rest = query[start..].trim_start();
    let ident = rest
        .split(|ch: char| ch.is_ascii_whitespace() || ch == ';')
        .next()?
        .trim_matches('"');
    (!ident.is_empty()).then(|| ident.to_string())
}

fn parse_pgvector_literal(query: &str, operator: PgVectorOperator) -> PgWireResult<Vec<f32>> {
    let op_at = query.find(operator.token()).ok_or_else(|| {
        pg_user_error(
            "42601",
            "pgvector-style query is missing a supported vector operator",
        )
    })?;
    let after = &query[op_at + operator.token().len()..];
    let quote_at = after.find('\'').ok_or_else(|| {
        pg_user_error("42601", "pgvector-style query is missing a vector literal")
    })? + 1;
    let rest = &after[quote_at..];
    let end = rest.find('\'').ok_or_else(|| {
        pg_user_error(
            "42601",
            "pgvector-style query has an unterminated vector literal",
        )
    })?;
    parse_vector_literal(&rest[..end])
}

fn parse_vector_literal(value: &str) -> PgWireResult<Vec<f32>> {
    let trimmed = value.trim();
    let Some(body) = trimmed.strip_prefix('[').and_then(|v| v.strip_suffix(']')) else {
        return Err(pg_user_error(
            "42601",
            "pgvector-style vector literal must use [..]",
        ));
    };
    if body.trim().is_empty() {
        return Err(pg_user_error(
            "42601",
            "pgvector-style vector literal must not be empty",
        ));
    }
    body.split(',')
        .map(|part| {
            part.trim().parse::<f32>().map_err(|_| {
                pg_user_error(
                    "42601",
                    "pgvector-style vector literal contains an invalid float",
                )
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ColumnarAnalyticalQuery {
    dataset: String,
    projection: AnalyticalProjection,
    limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AnalyticalProjection {
    Count,
    Columns(ColumnarProjection),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ColumnarProjection {
    All,
    Selected(Vec<String>),
}

fn parse_columnar_analytical_query(query: &str) -> PgWireResult<Option<ColumnarAnalyticalQuery>> {
    let trimmed = query.trim().trim_end_matches(';').trim();
    let lower = trimmed.to_ascii_lowercase();
    let normalized = normalized_catalog_query(trimmed);
    let Some(from_at) = lower.find(" from columnar.") else {
        if normalized.contains("columnar.") {
            return Err(pg_user_error(
                "42601",
                "columnar analytical queries must select from columnar.<dataset>",
            ));
        }
        return Ok(None);
    };
    if !normalized.starts_with("select ") {
        return Err(pg_user_error(
            "42601",
            "columnar analytical queries must be SELECT statements",
        ));
    }
    for clause in [" where ", " join ", " group by ", " order by ", " having "] {
        if normalized.contains(clause) {
            return Err(pg_user_error(
                "0A000",
                "columnar analytical queries support projection, count, and limit only",
            ));
        }
    }

    let projection = parse_columnar_projection(trimmed, from_at)?;
    let dataset = parse_columnar_dataset(trimmed, from_at)?;
    let limit = parse_limit(trimmed);
    Ok(Some(ColumnarAnalyticalQuery {
        dataset,
        projection,
        limit,
    }))
}

fn parse_columnar_projection(query: &str, from_at: usize) -> PgWireResult<AnalyticalProjection> {
    let projection = query["select ".len()..from_at].trim();
    if projection == "*" {
        return Ok(AnalyticalProjection::Columns(ColumnarProjection::All));
    }
    if projection.eq_ignore_ascii_case("count(*)") {
        return Ok(AnalyticalProjection::Count);
    }
    let columns = projection
        .split(',')
        .map(|part| part.trim().trim_matches('"').to_string())
        .collect::<Vec<_>>();
    if columns.is_empty() || columns.iter().any(String::is_empty) {
        return Err(pg_user_error(
            "42601",
            "columnar analytical query projection is invalid",
        ));
    }
    Ok(AnalyticalProjection::Columns(ColumnarProjection::Selected(
        columns,
    )))
}

fn parse_columnar_dataset(query: &str, from_at: usize) -> PgWireResult<String> {
    let from_token = " from columnar.";
    let after = &query[from_at + from_token.len()..];
    let dataset = after
        .trim_start()
        .split(|ch: char| ch.is_ascii_whitespace() || ch == ';')
        .next()
        .unwrap_or_default()
        .trim_matches('"');
    if dataset.is_empty() {
        return Err(pg_user_error(
            "42601",
            "columnar analytical query is missing a dataset",
        ));
    }
    Ok(dataset.to_string())
}

fn parse_limit(query: &str) -> Option<usize> {
    let lower = query.to_ascii_lowercase();
    let start = lower.rfind(" limit ")? + " limit ".len();
    let rest = query[start..].trim_start();
    rest.split(|ch: char| !ch.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

fn psql_oid_filter(query: &str) -> PgWireResult<i64> {
    let Some(start) = query.find("= '").map(|idx| idx + 3) else {
        return Err(pg_user_error(
            "42601",
            "PostgreSQL catalog query is missing an oid filter",
        ));
    };
    let Some(end) = query[start..].find('\'').map(|idx| start + idx) else {
        return Err(pg_user_error(
            "42601",
            "PostgreSQL catalog query has an unterminated oid filter",
        ));
    };
    query[start..end]
        .parse()
        .map_err(|_| pg_user_error("42601", "PostgreSQL catalog query oid is invalid"))
}

fn pg_catalog_type_name(type_name: &str) -> &'static str {
    match type_name.to_ascii_uppercase().as_str() {
        "INT" | "INTEGER" | "INT4" => "integer",
        "BIGINT" | "INT8" => "bigint",
        "TEXT" | "STRING" => "text",
        "BOOLEAN" | "BOOL" => "boolean",
        "FLOAT" | "DOUBLE" | "REAL" => "double precision",
        _ => "text",
    }
}

fn normalized_catalog_query(query: &str) -> String {
    query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn response_fields(response: Vec<Response>) -> PgWireResult<Vec<FieldInfo>> {
    if response.len() != 1 {
        return Err(pg_user_error(
            "0A000",
            "PostgreSQL-wire extended query supports one statement per portal",
        ));
    }
    let Some(response) = response.into_iter().next() else {
        return Ok(Vec::new());
    };
    match response {
        Response::Query(query) => Ok(query.row_schema.iter().cloned().collect()),
        Response::EmptyQuery | Response::Execution(_) => Ok(Vec::new()),
        _ => Err(pg_user_error(
            "0A000",
            "PostgreSQL-wire describe is not supported for this statement",
        )),
    }
}

fn first_statement_keyword(query: &str) -> Option<String> {
    query
        .trim_start()
        .trim_start_matches('(')
        .split(|ch: char| ch.is_ascii_whitespace() || ch == ';')
        .find(|part| !part.is_empty())
        .map(str::to_ascii_uppercase)
}

fn pg_responses_from_cbor(bytes: &[u8]) -> PgWireResult<Vec<Response>> {
    let payload = loom_result::result_view::decode(bytes).map_err(pg_error_from_loom)?;
    match payload {
        ResultPayload::Statements(statements) => statements.into_iter().map(pg_response).collect(),
        ResultPayload::Reader(_) => Err(pg_user_error(
            "0A000",
            "PostgreSQL-wire reader payloads are not supported",
        )),
    }
}

fn pg_response(statement: Statement) -> PgWireResult<Response> {
    match statement {
        Statement::Select { labels, rows } => query_response(labels, rows),
        Statement::SelectMap(rows) => {
            let labels = rows
                .first()
                .map(|row| row.keys().cloned().collect())
                .unwrap_or_else(Vec::new);
            let rows = rows
                .into_iter()
                .map(|row| {
                    labels
                        .iter()
                        .map(|label| row.get(label).cloned().unwrap_or(tabular::Value::Null))
                        .collect()
                })
                .collect();
            query_response(labels, rows)
        }
        Statement::ShowColumns(columns) => {
            let rows = columns
                .into_iter()
                .map(|column| {
                    vec![
                        tabular::Value::Text(column.name),
                        tabular::Value::Text(column.type_name),
                    ]
                })
                .collect();
            query_response(vec!["name".to_string(), "type".to_string()], rows)
        }
        Statement::ShowVariable(var) => show_variable_response(var),
        Statement::Insert(rows) => Ok(Response::Execution(
            Tag::new("INSERT").with_oid(0).with_rows(rows as usize),
        )),
        Statement::Delete(rows) => Ok(Response::Execution(
            Tag::new("DELETE").with_rows(rows as usize),
        )),
        Statement::Update(rows) => Ok(Response::Execution(
            Tag::new("UPDATE").with_rows(rows as usize),
        )),
        Statement::DropTable(rows) => Ok(Response::Execution(
            Tag::new("DROP TABLE").with_rows(rows as usize),
        )),
        Statement::Create => Ok(Response::Execution(Tag::new("CREATE TABLE"))),
        Statement::DropFunction => Ok(Response::Execution(Tag::new("DROP FUNCTION"))),
        Statement::AlterTable => Ok(Response::Execution(Tag::new("ALTER TABLE"))),
        Statement::CreateIndex => Ok(Response::Execution(Tag::new("CREATE INDEX"))),
        Statement::DropIndex => Ok(Response::Execution(Tag::new("DROP INDEX"))),
        Statement::StartTransaction | Statement::Commit | Statement::Rollback => {
            Err(pg_user_error(
                "0A000",
                "PostgreSQL-wire multi-statement transactions are not supported yet",
            ))
        }
    }
}

fn show_variable_response(var: ShowVariable) -> PgWireResult<Response> {
    match var {
        ShowVariable::Tables(values) | ShowVariable::Functions(values) => {
            let rows = values
                .into_iter()
                .map(|value| vec![tabular::Value::Text(value)])
                .collect();
            query_response(vec!["value".to_string()], rows)
        }
        ShowVariable::Version(value) => query_response(
            vec!["version".to_string()],
            vec![vec![tabular::Value::Text(value)]],
        ),
    }
}

fn query_response(labels: Vec<String>, rows: Vec<Vec<tabular::Value>>) -> PgWireResult<Response> {
    let fields = Arc::new(
        labels
            .into_iter()
            .map(|label| FieldInfo::new(label, None, None, Type::TEXT, FieldFormat::Text))
            .collect::<Vec<_>>(),
    );
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let mut encoder = DataRowEncoder::new(fields.clone());
        for value in row {
            match value {
                tabular::Value::Null => encoder.encode_field(&None::<String>)?,
                other => encoder.encode_field(&Some(pg_text_value(&other)))?,
            }
        }
        out.push(Ok(encoder.take_row()));
    }
    Ok(Response::Query(QueryResponse::new(
        fields,
        stream::iter(out),
    )))
}

fn pg_text_value(value: &tabular::Value) -> String {
    match value {
        tabular::Value::Null => String::new(),
        tabular::Value::Bool(v) => v.to_string(),
        tabular::Value::Int(v) => v.to_string(),
        tabular::Value::Float(v) => v.to_string(),
        tabular::Value::Text(v) => v.clone(),
        tabular::Value::Bytes(v) => hex_lower(v),
        tabular::Value::I8(v) => v.to_string(),
        tabular::Value::I16(v) => v.to_string(),
        tabular::Value::I32(v) => v.to_string(),
        tabular::Value::I128(v) => v.to_string(),
        tabular::Value::U8(v) => v.to_string(),
        tabular::Value::U16(v) => v.to_string(),
        tabular::Value::U32(v) => v.to_string(),
        tabular::Value::U64(v) => v.to_string(),
        tabular::Value::U128(v) => v.to_string(),
        tabular::Value::F32(v) => v.to_string(),
        tabular::Value::Decimal { mantissa, scale } => format!("{mantissa}e-{scale}"),
        tabular::Value::Date(v) => v.to_string(),
        tabular::Value::Time(v) => v.to_string(),
        tabular::Value::Timestamp(v) => v.to_string(),
        tabular::Value::Interval { months, micros } => format!("{months}:{micros}"),
        tabular::Value::Uuid(v) => v.to_string(),
        tabular::Value::Inet(v) => v.to_string(),
        tabular::Value::Point { x, y } => format!("{x},{y}"),
        tabular::Value::List(v) => format!("{v:?}"),
        tabular::Value::Map(v) => format!("{v:?}"),
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn pg_error_from_loom(err: LoomError) -> PgWireError {
    PgWireError::UserError(Box::new(ErrorInfo::new(
        "ERROR".to_string(),
        pg_sqlstate(err.code).to_string(),
        err.to_string(),
    )))
}

fn pg_error_from_hosted(err: HostedError) -> PgWireError {
    PgWireError::UserError(Box::new(ErrorInfo::new(
        "ERROR".to_string(),
        pg_sqlstate(err.code).to_string(),
        err.message,
    )))
}

fn pg_user_error(code: &str, message: &str) -> PgWireError {
    PgWireError::UserError(Box::new(ErrorInfo::new(
        "ERROR".to_string(),
        code.to_string(),
        message.to_string(),
    )))
}

fn pg_sqlstate(code: Code) -> &'static str {
    match code {
        Code::AuthenticationFailed => "28P01",
        Code::PermissionDenied => "42501",
        Code::NotFound | Code::SqlTableNotFound => "42P01",
        Code::AlreadyExists | Code::Conflict | Code::SqlConstraintViolation => "23505",
        Code::InvalidArgument | Code::SqlSyntax => "42601",
        Code::SqlTypeMismatch => "42804",
        Code::Unsupported => "0A000",
        _ => "XX000",
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use std::process::Command;

    use crate::test_support::{init, nid, temp_path};
    #[cfg(feature = "tls")]
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
    #[cfg(feature = "tls")]
    use tokio::net::TcpStream;

    #[test]
    fn pg_wire_rejects_transaction_boundaries() {
        for query in [
            "BEGIN",
            "commit;",
            "ROLLBACK",
            "START TRANSACTION",
            "BEGIN; SELECT 1",
            "SAVEPOINT s",
            "RELEASE SAVEPOINT s",
            "ROLLBACK TO s",
        ] {
            let err = reject_transaction_boundary(query).unwrap_err();
            let err = err.to_string();
            assert!(err.contains(PG_TRANSACTION_REJECTION_SQLSTATE), "{err}");
            assert!(err.contains(PG_TRANSACTION_REJECTION_MESSAGE), "{err}");
        }
        assert!(reject_transaction_boundary("SELECT 1").is_ok());
        assert!(reject_transaction_boundary("SELECT 'BEGIN'").is_ok());
    }

    #[test]
    fn psql_catalog_queries_recognize_postgres_namespace_relation() {
        assert!(psql_list_tables_catalog_query(
            "SELECT n.nspname FROM pg_catalog.pg_class c LEFT JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE pg_catalog.pg_get_userbyid(c.relowner) IS NOT NULL AND pg_catalog.pg_table_is_visible(c.oid)"
        ));
        assert!(psql_describe_relation_lookup_catalog_query(
            "SELECT c.oid, n.nspname, c.relname FROM pg_catalog.pg_class c LEFT JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE pg_table_is_visible(c.oid)"
        ));
    }

    #[test]
    fn pg_wire_maps_sql_select_and_execution_results() {
        let mut store = loom_sql::LoomSqlStore::default();
        let bytes = store
            .exec_cbor(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'a'); SELECT id, name FROM t",
            )
            .unwrap();
        let responses = pg_responses_from_cbor(&bytes).unwrap();
        assert_eq!(responses.len(), 3);
        match &responses[0] {
            Response::Execution(_) => {}
            _ => panic!("CREATE should map to an execution response"),
        }
        match &responses[1] {
            Response::Execution(_) => {}
            _ => panic!("INSERT should map to an execution response"),
        }
        match &responses[2] {
            Response::Query(query) => assert_eq!(query.row_schema.len(), 2),
            _ => panic!("SELECT should map to a query response"),
        }
    }

    #[test]
    fn tokio_postgres_simple_query_transcript_covers_pg_wire_profile() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let path = temp_path("pg-wire-transcript");
            init(&path, None);
            let kernel = HostedKernel::new(&path);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(serve_sql_pg_wire(
                listener,
                kernel,
                "main",
                "db",
                async move {
                    let _ = shutdown_rx.await;
                },
            ));

            let (client, connection) = tokio_postgres::Config::new()
                .host("127.0.0.1")
                .port(addr.port())
                .user(nid(1).to_string())
                .password("root-pass")
                .dbname("db")
                .connect(tokio_postgres::NoTls)
                .await
                .unwrap();
            let connection = tokio::spawn(async move {
                let _ = connection.await;
            });

            client
                .simple_query(
                    "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'a')",
                )
                .await
                .unwrap();
            let rows = client
                .simple_query("SELECT id, name FROM t")
                .await
                .unwrap();
            let selected = rows
                .iter()
                .find_map(|message| match message {
                    tokio_postgres::SimpleQueryMessage::Row(row) => row.get("name"),
                    _ => None,
                })
                .unwrap();
            assert_eq!(selected, "a");

            let extended_rows = client
                .query("SELECT id, name FROM t", &[])
                .await
                .unwrap();
            assert_eq!(extended_rows.len(), 1);
            assert_eq!(extended_rows[0].get::<_, &str>("name"), "a");

            let inserted = client
                .execute("INSERT INTO t VALUES (2, 'b')", &[])
                .await
                .unwrap();
            assert_eq!(inserted, 1);
            let extended_count = client
                .query("SELECT id, name FROM t", &[])
                .await
                .unwrap()
                .len();
            assert_eq!(extended_count, 2);

            let parameterized = client.query("SELECT $1", &[&"1"]).await.unwrap();
            assert_eq!(parameterized.len(), 1);
            assert_eq!(parameterized[0].get::<_, &str>(0), "1");
            let parameterized_insert = client
                .execute("INSERT INTO t VALUES ($1, $2)", &[&3_i64, &"c"])
                .await
                .unwrap();
            assert_eq!(parameterized_insert, 1);
            let parameterized_rows = client
                .query("SELECT id, name FROM t", &[])
                .await
                .unwrap();
            assert_eq!(parameterized_rows.len(), 3);

            for query in ["BEGIN", "COMMIT", "ROLLBACK", "BEGIN; SELECT 1"] {
                let txn = client.simple_query(query).await.unwrap_err();
                let db_error = txn.as_db_error().unwrap();
                assert_eq!(
                    db_error.code().code(),
                    PG_TRANSACTION_REJECTION_SQLSTATE
                );
                assert_eq!(db_error.message(), PG_TRANSACTION_REJECTION_MESSAGE);
            }

            drop(client);
            let _ = connection.await;

            let bad_auth = match tokio_postgres::Config::new()
                .host("127.0.0.1")
                .port(addr.port())
                .user(nid(1).to_string())
                .password("wrong-pass")
                .dbname("db")
                .connect(tokio_postgres::NoTls)
                .await
            {
                Ok(_) => panic!("wrong passphrase should fail PostgreSQL-wire authentication"),
                Err(err) => err,
            };
            assert_eq!(bad_auth.code().map(|code| code.code()), Some("28P01"));

            let _ = shutdown_tx.send(());
            server.await.unwrap().unwrap();
            fs::remove_file(path).unwrap();
        });
    }

    #[cfg(feature = "tls")]
    #[test]
    fn pg_wire_sslrequest_upgrades_before_authentication() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let path = temp_path("pg-wire-tls-transcript");
            init(&path, None);
            let kernel = HostedKernel::new(&path);
            let rcgen::CertifiedKey { cert, signing_key } =
                rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
            let cert_pem = cert.pem();
            let key_pem = signing_key.serialize_pem();
            let tls = HostedTlsConfig::from_pem_bytes_with_client_trust(
                "test-cert",
                cert_pem.as_bytes(),
                "test-key",
                key_pem.as_bytes(),
                None,
            )
            .unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(serve_sql_pg_wire_with_tls(
                listener,
                tls,
                kernel,
                "main",
                "db",
                async move {
                    let _ = shutdown_rx.await;
                },
            ));

            let mut client = TcpStream::connect(addr).await.unwrap();
            client
                .write_all(&[0, 0, 0, 8, 0x04, 0xd2, 0x16, 0x2f])
                .await
                .unwrap();
            client.flush().await.unwrap();
            let mut ssl_response = [0_u8; 1];
            client.read_exact(&mut ssl_response).await.unwrap();
            assert_eq!(ssl_response, [b'S']);

            let tls_client = pg_wire_test_tls_connector(cert.der().as_ref().to_vec());
            let server_name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
            let mut client = tls_client.connect(server_name, client).await.unwrap();
            write_pg_startup(&mut client, &nid(1).to_string(), "db").await;
            let auth = read_pg_backend_message(&mut client).await;
            assert_eq!(auth.0, b'R');
            assert_eq!(i32::from_be_bytes(auth.1[0..4].try_into().unwrap()), 3);

            write_pg_password(&mut client, "root-pass").await;
            assert_pg_ready_after_auth(&mut client).await;
            write_pg_terminate(&mut client).await;

            let _ = shutdown_tx.send(());
            server.await.unwrap().unwrap();
            fs::remove_file(path).unwrap();
        });
    }

    #[test]
    fn psql_transcript_covers_catalog_profile_when_available() {
        if Command::new("psql").arg("--version").output().is_err() {
            return;
        }

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let path = temp_path("pg-wire-psql-transcript");
            init(&path, None);
            let kernel = HostedKernel::new(&path);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(serve_sql_pg_wire(
                listener,
                kernel,
                "main",
                "db",
                async move {
                    let _ = shutdown_rx.await;
                },
            ));

            run_psql(
                addr.port(),
                "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES (1, 'a'); SELECT id, name FROM t;",
            );
            run_psql(addr.port(), "\\dt");
            run_psql(addr.port(), "\\d t");
            run_psql(addr.port(), "\\d+ t");

            let _ = shutdown_tx.send(());
            server.await.unwrap().unwrap();
            fs::remove_file(path).unwrap();
        });
    }

    #[test]
    fn tokio_postgres_pgvector_style_query_transcript() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let path = temp_path("pg-wire-pgvector-transcript");
            init(&path, None);
            let kernel = HostedKernel::new(&path);
            let setup_auth = HostedAuth::passphrase(nid(1), "root-pass", "setup");
            kernel
                .data()
                .vector_create(&setup_auth, "main", "embeddings", 2, loom_core::Metric::L2)
                .unwrap();
            kernel
                .data()
                .vector_upsert(
                    &setup_auth,
                    "main",
                    "embeddings",
                    "a",
                    vec![1.0, 0.0],
                    BTreeMap::new(),
                )
                .unwrap();
            kernel
                .data()
                .vector_create(&setup_auth, "main", "cosines", 2, loom_core::Metric::Cosine)
                .unwrap();
            kernel
                .data()
                .vector_upsert(
                    &setup_auth,
                    "main",
                    "cosines",
                    "a",
                    vec![1.0, 0.0],
                    BTreeMap::new(),
                )
                .unwrap();
            kernel
                .data()
                .vector_upsert(
                    &setup_auth,
                    "main",
                    "cosines",
                    "b",
                    vec![0.0, 1.0],
                    BTreeMap::new(),
                )
                .unwrap();
            kernel
                .data()
                .vector_create(&setup_auth, "main", "dots", 2, loom_core::Metric::Dot)
                .unwrap();
            kernel
                .data()
                .vector_upsert(
                    &setup_auth,
                    "main",
                    "dots",
                    "a",
                    vec![1.0, 0.0],
                    BTreeMap::new(),
                )
                .unwrap();
            kernel
                .data()
                .vector_upsert(
                    &setup_auth,
                    "main",
                    "dots",
                    "b",
                    vec![0.0, 1.0],
                    BTreeMap::new(),
                )
                .unwrap();
            kernel
                .data()
                .vector_upsert(
                    &setup_auth,
                    "main",
                    "embeddings",
                    "b",
                    vec![0.0, 1.0],
                    BTreeMap::new(),
                )
                .unwrap();

            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(serve_sql_pg_wire(
                listener,
                kernel,
                "main",
                "db",
                async move {
                    let _ = shutdown_rx.await;
                },
            ));

            let (client, connection) = tokio_postgres::Config::new()
                .host("127.0.0.1")
                .port(addr.port())
                .user(nid(1).to_string())
                .password("root-pass")
                .dbname("db")
                .connect(tokio_postgres::NoTls)
                .await
                .unwrap();
            let connection = tokio::spawn(async move {
                let _ = connection.await;
            });

            let rows = client
                .query(
                    "SELECT id, embedding <-> '[1.0,0.0]' AS distance FROM embeddings ORDER BY embedding <-> '[1.0,0.0]' LIMIT 2",
                    &[],
                )
                .await
                .unwrap();
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].get::<_, &str>("id"), "a");
            assert_eq!(rows[0].get::<_, &str>("distance"), "0");

            let cosine_rows = client
                .query(
                    "SELECT id, embedding <=> '[1.0,0.0]' AS distance FROM cosines ORDER BY embedding <=> '[1.0,0.0]' LIMIT 1",
                    &[],
                )
                .await
                .unwrap();
            assert_eq!(cosine_rows[0].get::<_, &str>("id"), "a");
            assert_eq!(cosine_rows[0].get::<_, &str>("distance"), "0");

            let dot_rows = client
                .query(
                    "SELECT id, embedding <#> '[1.0,0.0]' AS distance FROM dots ORDER BY embedding <#> '[1.0,0.0]' LIMIT 1",
                    &[],
                )
                .await
                .unwrap();
            assert_eq!(dot_rows[0].get::<_, &str>("id"), "a");
            assert_eq!(dot_rows[0].get::<_, &str>("distance"), "-1");

            let mismatch = client
                .query(
                    "SELECT id, embedding <=> '[1.0,0.0]' AS distance FROM embeddings ORDER BY embedding <=> '[1.0,0.0]' LIMIT 2",
                    &[],
                )
                .await
                .unwrap_err();
            assert_eq!(mismatch.code().map(|code| code.code()), Some("0A000"));

            drop(client);
            let _ = connection.await;
            let _ = shutdown_tx.send(());
            server.await.unwrap().unwrap();
            fs::remove_file(path).unwrap();
        });
    }

    #[test]
    fn tokio_postgres_columnar_analytical_query_transcript() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let path = temp_path("pg-wire-columnar-transcript");
            init(&path, None);
            let kernel = HostedKernel::new(&path);
            let setup_auth = HostedAuth::passphrase(nid(1), "root-pass", "setup");
            kernel
                .data()
                .columnar_create(
                    &setup_auth,
                    "main",
                    "sales",
                    vec![
                        ("id".to_string(), loom_core::ColumnType::Int),
                        ("region".to_string(), loom_core::ColumnType::Text),
                        ("amount".to_string(), loom_core::ColumnType::Float),
                    ],
                    100,
                )
                .unwrap();
            kernel
                .data()
                .columnar_append(
                    &setup_auth,
                    "main",
                    "sales",
                    vec![
                        tabular::Value::Int(1),
                        tabular::Value::Text("west".to_string()),
                        tabular::Value::Float(12.5),
                    ],
                )
                .unwrap();
            kernel
                .data()
                .columnar_append(
                    &setup_auth,
                    "main",
                    "sales",
                    vec![
                        tabular::Value::Int(2),
                        tabular::Value::Text("east".to_string()),
                        tabular::Value::Float(7.0),
                    ],
                )
                .unwrap();

            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(serve_sql_pg_wire(
                listener,
                kernel,
                "main",
                "db",
                async move {
                    let _ = shutdown_rx.await;
                },
            ));

            let (client, connection) = tokio_postgres::Config::new()
                .host("127.0.0.1")
                .port(addr.port())
                .user(nid(1).to_string())
                .password("root-pass")
                .dbname("db")
                .connect(tokio_postgres::NoTls)
                .await
                .unwrap();
            let connection = tokio::spawn(async move {
                let _ = connection.await;
            });

            let projected = client
                .query("SELECT id, amount FROM columnar.sales LIMIT 1", &[])
                .await
                .unwrap();
            assert_eq!(projected.len(), 1);
            assert_eq!(projected[0].get::<_, &str>("id"), "1");
            assert_eq!(projected[0].get::<_, &str>("amount"), "12.5");

            let all = client
                .query("SELECT * FROM columnar.sales LIMIT 2", &[])
                .await
                .unwrap();
            assert_eq!(all.len(), 2);
            assert_eq!(all[1].get::<_, &str>("region"), "east");

            let count = client
                .query("SELECT count(*) FROM columnar.sales", &[])
                .await
                .unwrap();
            assert_eq!(count[0].get::<_, &str>("count"), "2");

            let unsupported = client
                .query("SELECT id FROM columnar.sales WHERE id = 1", &[])
                .await
                .unwrap_err();
            assert_eq!(unsupported.code().map(|code| code.code()), Some("0A000"));

            drop(client);
            let _ = connection.await;
            let _ = shutdown_tx.send(());
            server.await.unwrap().unwrap();
            fs::remove_file(path).unwrap();
        });
    }

    #[cfg(feature = "tls")]
    fn pg_wire_test_tls_connector(cert: Vec<u8>) -> tokio_rustls::TlsConnector {
        use std::sync::Arc;

        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(rustls::pki_types::CertificateDer::from(cert))
            .unwrap();
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        tokio_rustls::TlsConnector::from(Arc::new(config))
    }

    #[cfg(feature = "tls")]
    async fn write_pg_startup<S>(stream: &mut S, user: &str, database: &str)
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let mut body = Vec::new();
        body.extend_from_slice(&196_608_i32.to_be_bytes());
        body.extend_from_slice(b"user\0");
        body.extend_from_slice(user.as_bytes());
        body.push(0);
        body.extend_from_slice(b"database\0");
        body.extend_from_slice(database.as_bytes());
        body.push(0);
        body.push(0);
        let len = (body.len() + 4) as i32;
        stream.write_all(&len.to_be_bytes()).await.unwrap();
        stream.write_all(&body).await.unwrap();
        stream.flush().await.unwrap();
    }

    #[cfg(feature = "tls")]
    async fn write_pg_password<S>(stream: &mut S, password: &str)
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let mut body = password.as_bytes().to_vec();
        body.push(0);
        stream.write_all(&[b'p']).await.unwrap();
        stream
            .write_all(&((body.len() + 4) as i32).to_be_bytes())
            .await
            .unwrap();
        stream.write_all(&body).await.unwrap();
        stream.flush().await.unwrap();
    }

    #[cfg(feature = "tls")]
    async fn write_pg_terminate<S>(stream: &mut S)
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        stream.write_all(&[b'X']).await.unwrap();
        stream.write_all(&4_i32.to_be_bytes()).await.unwrap();
        stream.flush().await.unwrap();
    }

    #[cfg(feature = "tls")]
    async fn read_pg_backend_message<S>(stream: &mut S) -> (u8, Vec<u8>)
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let tag = stream.read_u8().await.unwrap();
        let len = stream.read_i32().await.unwrap();
        let mut payload = vec![0_u8; len as usize - 4];
        stream.read_exact(&mut payload).await.unwrap();
        (tag, payload)
    }

    #[cfg(feature = "tls")]
    async fn assert_pg_ready_after_auth<S>(stream: &mut S)
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let mut saw_auth_ok = false;
        loop {
            let message = read_pg_backend_message(stream).await;
            match message.0 {
                b'R' => {
                    let code = i32::from_be_bytes(message.1[0..4].try_into().unwrap());
                    assert_eq!(code, 0);
                    saw_auth_ok = true;
                }
                b'Z' => {
                    assert!(saw_auth_ok);
                    return;
                }
                _ => {}
            }
        }
    }

    fn run_psql(port: u16, command: &str) {
        let output = Command::new("psql")
            .env("PGPASSWORD", "root-pass")
            .arg("-h")
            .arg("127.0.0.1")
            .arg("-p")
            .arg(port.to_string())
            .arg("-U")
            .arg(nid(1).to_string())
            .arg("-d")
            .arg("db")
            .arg("-E")
            .arg("-v")
            .arg("ON_ERROR_STOP=1")
            .arg("-c")
            .arg(command)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "psql command failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
