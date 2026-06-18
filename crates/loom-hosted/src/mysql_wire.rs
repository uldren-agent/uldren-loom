use std::collections::BTreeMap;
use std::future::Future;
use std::io;

use gluesql_core::ast::DataType;
use loom_core::error::Code;
use loom_core::{WorkspaceId, tabular};
use loom_result::result_view::{ResultPayload, ShowVariable, Statement};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[cfg(feature = "tls")]
use crate::HostedTlsConfig;
use crate::{HostedAuth, HostedError, HostedKernel};

const CLIENT_CONNECT_WITH_DB: u32 = 0x0000_0008;
const CLIENT_SSL: u32 = 0x0000_0800;
const CLIENT_PROTOCOL_41: u32 = 0x0000_0200;
const CLIENT_SECURE_CONNECTION: u32 = 0x0000_8000;
const CLIENT_PLUGIN_AUTH: u32 = 0x0008_0000;
const CLIENT_PLUGIN_AUTH_LENENC_CLIENT_DATA: u32 = 0x0020_0000;
const SERVER_STATUS_AUTOCOMMIT: u16 = 0x0002;
const MYSQL_TYPE_NULL: u8 = 0x06;
const MYSQL_TYPE_TINY: u8 = 0x01;
const MYSQL_TYPE_SHORT: u8 = 0x02;
const MYSQL_TYPE_LONG: u8 = 0x03;
const MYSQL_TYPE_FLOAT: u8 = 0x04;
const MYSQL_TYPE_DOUBLE: u8 = 0x05;
const MYSQL_TYPE_LONGLONG: u8 = 0x08;
const MYSQL_TYPE_VAR_STRING: u8 = 0xfd;
const MYSQL_TYPE_STRING: u8 = 0xfe;
const MYSQL_CLEARTEXT_PASSWORD: &str = "mysql_clear_password";
const MYSQL_NATIVE_PASSWORD: &str = "mysql_native_password";
const MYSQL_SALT_LEN: usize = 20;
const COM_QUIT: u8 = 0x01;
const COM_INIT_DB: u8 = 0x02;
const COM_QUERY: u8 = 0x03;
const COM_PING: u8 = 0x0e;
const COM_STMT_PREPARE: u8 = 0x16;
const COM_STMT_EXECUTE: u8 = 0x17;
const COM_STMT_CLOSE: u8 = 0x19;
const COM_STMT_RESET: u8 = 0x1a;

type MysqlTextRows = (Vec<String>, Vec<Vec<tabular::Value>>);

#[cfg(feature = "tls")]
type MysqlTls = Option<HostedTlsConfig>;
#[cfg(not(feature = "tls"))]
type MysqlTls = ();

enum MysqlIo {
    Plain(TcpStream),
    #[cfg(feature = "tls")]
    Tls(Box<tokio_rustls::server::TlsStream<TcpStream>>),
}

impl AsyncRead for MysqlIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut *self {
            MysqlIo::Plain(stream) => Pin::new(stream).poll_read(cx, buf),
            #[cfg(feature = "tls")]
            MysqlIo::Tls(stream) => Pin::new(stream.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for MysqlIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match &mut *self {
            MysqlIo::Plain(stream) => Pin::new(stream).poll_write(cx, buf),
            #[cfg(feature = "tls")]
            MysqlIo::Tls(stream) => Pin::new(stream.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut *self {
            MysqlIo::Plain(stream) => Pin::new(stream).poll_flush(cx),
            #[cfg(feature = "tls")]
            MysqlIo::Tls(stream) => Pin::new(stream.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut *self {
            MysqlIo::Plain(stream) => Pin::new(stream).poll_shutdown(cx),
            #[cfg(feature = "tls")]
            MysqlIo::Tls(stream) => Pin::new(stream.as_mut()).poll_shutdown(cx),
        }
    }
}

struct MysqlConnection {
    stream: Option<MysqlIo>,
    kernel: HostedKernel,
    workspace: String,
    database: String,
    auth: Option<HostedAuth>,
    client_capabilities: u32,
    salt: [u8; MYSQL_SALT_LEN],
    prepared_statements: BTreeMap<u32, MysqlPreparedStatement>,
    next_statement_id: u32,
    #[cfg(feature = "tls")]
    tls: Option<HostedTlsConfig>,
}

struct MysqlPreparedStatement {
    sql: String,
    parameter_count: usize,
    parameter_types: Vec<Option<DataType>>,
}

impl MysqlConnection {
    async fn run(mut self) -> io::Result<()> {
        self.write_handshake().await?;
        let mut response = self.read_packet().await?;
        if self.packet_requests_tls(&response.payload) {
            self.upgrade_tls().await?;
            response = self.read_packet().await?;
        }
        self.authenticate(&response.payload).await?;
        self.write_ok(2, 0).await?;
        loop {
            let packet = match self.read_packet().await {
                Ok(packet) => packet,
                Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(err) => return Err(err),
            };
            if packet.payload.is_empty() {
                self.write_err(1, 1105, "empty MySQL command").await?;
                continue;
            }
            match packet.payload[0] {
                COM_QUIT => return Ok(()),
                COM_INIT_DB => self.handle_init_db(&packet.payload[1..]).await?,
                COM_QUERY => self.handle_query(&packet.payload[1..]).await?,
                COM_PING => self.write_ok(1, 0).await?,
                COM_STMT_PREPARE => self.handle_stmt_prepare(&packet.payload[1..]).await?,
                COM_STMT_EXECUTE => self.handle_stmt_execute(&packet.payload[1..]).await?,
                COM_STMT_CLOSE => self.handle_stmt_close(&packet.payload[1..]),
                COM_STMT_RESET => self.handle_stmt_reset(&packet.payload[1..]).await?,
                _ => {
                    self.write_err(1, 1235, "MySQL command is not supported yet")
                        .await?;
                }
            }
        }
    }

    fn stream_mut(&mut self) -> io::Result<&mut MysqlIo> {
        self.stream
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "MySQL stream unavailable"))
    }

    fn packet_requests_tls(&self, payload: &[u8]) -> bool {
        payload.len() == 32
            && u32::from_le_bytes(payload[0..4].try_into().unwrap()) & CLIENT_SSL != 0
    }

    #[cfg(feature = "tls")]
    async fn upgrade_tls(&mut self) -> io::Result<()> {
        let Some(tls) = self.tls.as_ref() else {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "MySQL TLS is not enabled for this listener",
            ));
        };
        let stream = self.stream.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "MySQL stream unavailable")
        })?;
        let MysqlIo::Plain(stream) = stream else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "MySQL stream is already TLS",
            ));
        };
        let stream = tls.acceptor().accept(stream).await?;
        self.stream = Some(MysqlIo::Tls(Box::new(stream)));
        Ok(())
    }

    #[cfg(not(feature = "tls"))]
    async fn upgrade_tls(&mut self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "MySQL TLS is not enabled for this build",
        ))
    }

    async fn write_handshake(&mut self) -> io::Result<()> {
        let mut payload = Vec::new();
        payload.push(10);
        payload.extend_from_slice(b"8.0.0-loom\0");
        payload.extend_from_slice(&8_u32.to_le_bytes());
        payload.extend_from_slice(&self.salt[..8]);
        payload.push(0);
        let caps = CLIENT_PROTOCOL_41
            | CLIENT_SECURE_CONNECTION
            | CLIENT_PLUGIN_AUTH
            | CLIENT_CONNECT_WITH_DB;
        #[cfg(feature = "tls")]
        let caps = if self.tls.is_some() {
            caps | CLIENT_SSL
        } else {
            caps
        };
        payload.extend_from_slice(&(caps as u16).to_le_bytes());
        payload.push(33);
        payload.extend_from_slice(&SERVER_STATUS_AUTOCOMMIT.to_le_bytes());
        payload.extend_from_slice(&((caps >> 16) as u16).to_le_bytes());
        payload.push(21);
        payload.extend_from_slice(&[0; 10]);
        payload.extend_from_slice(&self.salt[8..]);
        payload.push(0);
        payload.extend_from_slice(MYSQL_NATIVE_PASSWORD.as_bytes());
        payload.push(0);
        self.write_packet(0, &payload).await
    }

    async fn authenticate(&mut self, payload: &[u8]) -> io::Result<()> {
        if payload.len() < 32 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "short MySQL handshake response",
            ));
        }
        self.client_capabilities = u32::from_le_bytes(payload[0..4].try_into().unwrap());
        let mut cursor = 32;
        let username_end = payload[cursor..]
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "missing username"))?
            + cursor;
        let username = std::str::from_utf8(&payload[cursor..username_end]).map_err(|_| {
            io::Error::new(io::ErrorKind::PermissionDenied, "username is not UTF-8")
        })?;
        cursor = username_end + 1;
        let auth_data = if self.client_capabilities & CLIENT_PLUGIN_AUTH_LENENC_CLIENT_DATA != 0 {
            let (len, used) = read_lenenc_int(&payload[cursor..]).ok_or_else(|| {
                io::Error::new(io::ErrorKind::PermissionDenied, "bad auth length")
            })?;
            cursor += used;
            let end = cursor + len as usize;
            if end > payload.len() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "auth data exceeds packet",
                ));
            }
            let data = &payload[cursor..end];
            cursor = end;
            data
        } else if self.client_capabilities & CLIENT_SECURE_CONNECTION != 0 {
            let Some(len) = payload.get(cursor).copied() else {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "missing auth length",
                ));
            };
            cursor += 1;
            let end = cursor + len as usize;
            if end > payload.len() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "auth data exceeds packet",
                ));
            }
            let data = &payload[cursor..end];
            cursor = end;
            data
        } else {
            let end = payload[cursor..]
                .iter()
                .position(|byte| *byte == 0)
                .map(|idx| cursor + idx)
                .unwrap_or(payload.len());
            let data = &payload[cursor..end];
            cursor = end;
            data
        };
        if self.client_capabilities & CLIENT_CONNECT_WITH_DB != 0 && cursor < payload.len() {
            let database_end = payload[cursor..]
                .iter()
                .position(|byte| *byte == 0)
                .map(|idx| cursor + idx)
                .unwrap_or(payload.len());
            let database = std::str::from_utf8(&payload[cursor..database_end]).unwrap_or_default();
            cursor = database_end.saturating_add(1);
            if !database.is_empty() && database != self.database {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "unknown Loom SQL database",
                ));
            }
        }
        let plugin = if self.client_capabilities & CLIENT_PLUGIN_AUTH != 0 && cursor < payload.len()
        {
            let plugin_end = payload[cursor..]
                .iter()
                .position(|byte| *byte == 0)
                .map(|idx| cursor + idx)
                .unwrap_or(payload.len());
            std::str::from_utf8(&payload[cursor..plugin_end]).unwrap_or(MYSQL_NATIVE_PASSWORD)
        } else {
            MYSQL_NATIVE_PASSWORD
        };
        let auth = match plugin {
            MYSQL_NATIVE_PASSWORD => self.authenticate_native_password(username, auth_data)?,
            MYSQL_CLEARTEXT_PASSWORD => {
                let password = String::from_utf8_lossy(auth_data)
                    .trim_end_matches('\0')
                    .to_string();
                self.authenticate_secret(username, password)?
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "unsupported MySQL authentication plugin",
                ));
            }
        };
        self.auth = Some(auth);
        Ok(())
    }

    fn authenticate_native_password(
        &self,
        username: &str,
        scramble: &[u8],
    ) -> io::Result<HostedAuth> {
        let principal = WorkspaceId::parse(username)
            .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "bad principal"))?;
        self.kernel
            .read_mut(&HostedAuth::unauthenticated(), |loom| {
                let identity = loom.identity_store_mut().ok_or_else(|| {
                    loom_core::LoomError::new(Code::AuthenticationFailed, "identity store missing")
                })?;
                let session = identity.authenticate_mysql_native_password(
                    principal,
                    scramble,
                    &self.salt,
                    format!("mysql-wire-native:{principal}"),
                )?;
                loom.set_session(session.id);
                Ok(())
            })
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "mysql_native_password authentication failed",
                )
            })?;
        Ok(HostedAuth::preauthenticated(
            principal,
            format!("mysql-wire-native:{principal}"),
        ))
    }

    fn authenticate_secret(&self, username: &str, secret: String) -> io::Result<HostedAuth> {
        if secret.starts_with("loom_app_") {
            let auth = HostedAuth::app_credential(secret, format!("mysql-wire-app:{username}"));
            let principal = self.kernel.read(&auth, |loom| loom.effective_principal());
            let principal = principal.map_err(|_| {
                io::Error::new(io::ErrorKind::PermissionDenied, "authentication failed")
            })?;
            if let Ok(username_principal) = WorkspaceId::parse(username)
                && principal != Some(username_principal)
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "app credential principal does not match MySQL user",
                ));
            }
            return Ok(auth);
        }
        let principal = WorkspaceId::parse(username)
            .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "bad principal"))?;
        let auth = HostedAuth::passphrase(principal, secret, format!("mysql-wire:{principal}"));
        self.kernel.read(&auth, |_| Ok(())).map_err(|_| {
            io::Error::new(io::ErrorKind::PermissionDenied, "authentication failed")
        })?;
        Ok(auth)
    }

    async fn handle_init_db(&mut self, payload: &[u8]) -> io::Result<()> {
        let database = std::str::from_utf8(payload).unwrap_or_default();
        if database == self.database {
            self.write_ok(1, 0).await
        } else {
            self.write_err(1, 1049, "unknown Loom SQL database").await
        }
    }

    async fn handle_query(&mut self, payload: &[u8]) -> io::Result<()> {
        let query = std::str::from_utf8(payload)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "query is not UTF-8"))?;
        if mysql_transaction_boundary(query) {
            return self
                .write_err(
                    1,
                    1235,
                    "MySQL-wire multi-statement transactions are not supported yet",
                )
                .await;
        }
        let Some(auth) = self.auth.as_ref() else {
            return self.write_err(1, 1045, "unauthenticated").await;
        };
        if mysql_noop_statement(query) {
            return self.write_ok(1, 0).await;
        }
        if let Some((labels, rows)) = self.mysql_metadata_query(auth, query)? {
            return self.write_resultset(1, labels, rows).await;
        }
        let bytes = match self
            .kernel
            .sql()
            .exec_cbor(auth, &self.workspace, &self.database, query)
        {
            Ok(bytes) => bytes,
            Err(err) => return self.write_hosted_error(1, err).await,
        };
        self.write_results_from_cbor(1, &bytes).await
    }

    async fn handle_stmt_prepare(&mut self, payload: &[u8]) -> io::Result<()> {
        let sql = std::str::from_utf8(payload)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "query is not UTF-8"))?
            .to_string();
        if mysql_transaction_boundary(&sql) {
            return self
                .write_err(
                    1,
                    1235,
                    "MySQL-wire multi-statement transactions are not supported yet",
                )
                .await;
        }
        let Some(auth) = self.auth.as_ref() else {
            return self.write_err(1, 1045, "unauthenticated").await;
        };
        let parameter_count = count_mysql_parameter_markers(&sql);
        let inference_sql = mysql_parameter_markers_to_postgres(&sql)?;
        let parameter_types = self
            .kernel
            .sql()
            .infer_parameter_types(auth, &self.workspace, &self.database, &inference_sql)
            .unwrap_or_default();
        let result_columns = self.prepare_result_columns(auth, &sql)?;
        let statement_id = self.next_statement_id;
        self.next_statement_id = self.next_statement_id.wrapping_add(1).max(1);
        self.prepared_statements.insert(
            statement_id,
            MysqlPreparedStatement {
                sql,
                parameter_count,
                parameter_types,
            },
        );
        self.write_stmt_prepare_ok(statement_id, parameter_count, &result_columns)
            .await
    }

    fn prepare_result_columns(&self, auth: &HostedAuth, sql: &str) -> io::Result<Vec<String>> {
        let normalized = normalize_sql(sql);
        if normalized.starts_with("SELECT ") {
            return Ok(mysql_select_projection_labels(sql).unwrap_or_default());
        }
        if !(normalized.starts_with("SHOW ")
            || normalized.starts_with("DESCRIBE ")
            || normalized.starts_with("DESC "))
        {
            return Ok(Vec::new());
        }
        let describe_sql = replace_mysql_parameter_markers_with_null(sql);
        if let Some((labels, _)) = self.mysql_metadata_query(auth, &describe_sql)? {
            return Ok(labels);
        }
        Ok(Vec::new())
    }

    async fn handle_stmt_execute(&mut self, payload: &[u8]) -> io::Result<()> {
        let Some(statement_id) = read_u32_le(payload, 0) else {
            return self
                .write_err(1, 1064, "malformed MySQL prepared statement execute")
                .await;
        };
        let Some(statement) = self.prepared_statements.get(&statement_id) else {
            return self
                .write_err(1, 1243, "unknown MySQL prepared statement")
                .await;
        };
        let parameters = match parse_mysql_execute_parameters(payload, statement) {
            Ok(parameters) => parameters,
            Err(err) => return self.write_err(1, 1064, &err).await,
        };
        let query = match rewrite_mysql_parameter_markers(&statement.sql, &parameters) {
            Ok(query) => query,
            Err(err) => return self.write_err(1, 1064, &err).await,
        };
        self.execute_prepared_query(&query).await
    }

    async fn execute_prepared_query(&mut self, query: &str) -> io::Result<()> {
        if mysql_transaction_boundary(query) {
            return self
                .write_err(
                    1,
                    1235,
                    "MySQL-wire multi-statement transactions are not supported yet",
                )
                .await;
        }
        let Some(auth) = self.auth.as_ref() else {
            return self.write_err(1, 1045, "unauthenticated").await;
        };
        if mysql_noop_statement(query) {
            return self.write_ok(1, 0).await;
        }
        if let Some((labels, rows)) = self.mysql_metadata_query(auth, query)? {
            return self.write_binary_resultset(1, labels, rows).await;
        }
        let bytes = match self
            .kernel
            .sql()
            .exec_cbor(auth, &self.workspace, &self.database, query)
        {
            Ok(bytes) => bytes,
            Err(err) => return self.write_hosted_error(1, err).await,
        };
        self.write_binary_results_from_cbor(1, &bytes).await
    }

    fn handle_stmt_close(&mut self, payload: &[u8]) {
        if let Some(statement_id) = read_u32_le(payload, 0) {
            self.prepared_statements.remove(&statement_id);
        }
    }

    async fn handle_stmt_reset(&mut self, payload: &[u8]) -> io::Result<()> {
        if let Some(statement_id) = read_u32_le(payload, 0)
            && self.prepared_statements.contains_key(&statement_id)
        {
            return self.write_ok(1, 0).await;
        }
        self.write_err(1, 1243, "unknown MySQL prepared statement")
            .await
    }

    fn mysql_metadata_query(
        &self,
        auth: &HostedAuth,
        query: &str,
    ) -> io::Result<Option<MysqlTextRows>> {
        let normalized = normalize_sql(query);
        if normalized == "SELECT @@VERSION_COMMENT LIMIT 1" {
            return Ok(Some(single_text_result(
                "@@version_comment",
                "Loom MySQL-wire compatibility profile",
            )));
        }
        if normalized == "SELECT VERSION()" || normalized.starts_with("SELECT VERSION() ") {
            return Ok(Some(single_text_result("VERSION()", "8.0.0-loom")));
        }
        if normalized == "SELECT DATABASE()" || normalized.starts_with("SELECT DATABASE() ") {
            return Ok(Some(single_text_result("DATABASE()", &self.database)));
        }
        if normalized == "SELECT USER()"
            || normalized.starts_with("SELECT USER() ")
            || normalized == "SELECT CURRENT_USER()"
            || normalized.starts_with("SELECT CURRENT_USER() ")
        {
            let user = auth
                .principal
                .map(|principal| principal.to_string())
                .unwrap_or_else(|| "loom_app".to_string());
            return Ok(Some(single_text_result(
                "USER()",
                &format!("{user}@localhost"),
            )));
        }
        if normalized == "SHOW DATABASES" {
            return Ok(Some((
                vec!["Database".to_string()],
                vec![vec![tabular::Value::Text(self.database.clone())]],
            )));
        }
        if normalized.starts_with("SHOW VARIABLES") {
            return Ok(Some(mysql_show_variables_response(&normalized)));
        }
        if normalized == "SHOW TABLES" || normalized == "SHOW FULL TABLES" {
            let tables = self.list_sql_tables(auth)?;
            let full = normalized == "SHOW FULL TABLES";
            let mut labels = vec![format!("Tables_in_{}", self.database)];
            if full {
                labels.push("Table_type".to_string());
            }
            let rows = tables
                .into_iter()
                .map(|table| {
                    let mut row = vec![tabular::Value::Text(table)];
                    if full {
                        row.push(tabular::Value::Text("BASE TABLE".to_string()));
                    }
                    row
                })
                .collect();
            return Ok(Some((labels, rows)));
        }
        if let Some(table) = mysql_describe_table(query) {
            let columns = self.list_sql_columns(auth, table)?;
            let rows = columns
                .into_iter()
                .enumerate()
                .map(|(idx, column)| {
                    let key = if idx == 0 { "PRI" } else { "" };
                    vec![
                        tabular::Value::Text(column.name),
                        tabular::Value::Text(mysql_type_name(&column.type_name)),
                        tabular::Value::Text("YES".to_string()),
                        tabular::Value::Text(key.to_string()),
                        tabular::Value::Null,
                        tabular::Value::Text(String::new()),
                    ]
                })
                .collect();
            return Ok(Some((
                vec![
                    "Field".to_string(),
                    "Type".to_string(),
                    "Null".to_string(),
                    "Key".to_string(),
                    "Default".to_string(),
                    "Extra".to_string(),
                ],
                rows,
            )));
        }
        if normalized.contains(" INFORMATION_SCHEMA.TABLES") {
            let labels = mysql_information_schema_table_labels(&normalized);
            let rows = self
                .list_sql_tables(auth)?
                .into_iter()
                .map(|table| {
                    labels
                        .iter()
                        .map(|label| {
                            mysql_information_schema_table_value(label, &self.database, &table)
                        })
                        .collect()
                })
                .collect();
            return Ok(Some((labels, rows)));
        }
        if normalized.contains(" INFORMATION_SCHEMA.COLUMNS") {
            let labels = mysql_information_schema_column_labels(&normalized);
            let table_filter = mysql_table_name_filter(&normalized);
            let mut rows = Vec::new();
            for table in self.list_sql_tables(auth)? {
                if table_filter.as_ref().is_some_and(|filter| filter != &table) {
                    continue;
                }
                for (idx, column) in self.list_sql_columns(auth, &table)?.into_iter().enumerate() {
                    rows.push(
                        labels
                            .iter()
                            .map(|label| {
                                mysql_information_schema_column_value(
                                    label,
                                    &self.database,
                                    &table,
                                    idx + 1,
                                    &column,
                                )
                            })
                            .collect(),
                    );
                }
            }
            return Ok(Some((labels, rows)));
        }
        Ok(None)
    }

    fn list_sql_tables(&self, auth: &HostedAuth) -> io::Result<Vec<String>> {
        let bytes = self
            .kernel
            .sql()
            .exec_cbor(auth, &self.workspace, &self.database, "SHOW TABLES")
            .map_err(|err| io::Error::other(err.message))?;
        let payload = loom_result::result_view::decode(&bytes).map_err(io::Error::other)?;
        match payload {
            ResultPayload::Statements(statements) => {
                for statement in statements {
                    if let Statement::ShowVariable(ShowVariable::Tables(values)) = statement {
                        return Ok(values);
                    }
                }
                Ok(Vec::new())
            }
            ResultPayload::Reader(_) => Err(io::Error::other(
                "MySQL-wire catalog reader payloads are not supported",
            )),
        }
    }

    fn list_sql_columns(
        &self,
        auth: &HostedAuth,
        table: &str,
    ) -> io::Result<Vec<loom_result::result_view::Column>> {
        let sql = format!("SHOW COLUMNS FROM {table}");
        let bytes = self
            .kernel
            .sql()
            .exec_cbor(auth, &self.workspace, &self.database, &sql)
            .map_err(|err| io::Error::other(err.message))?;
        let payload = loom_result::result_view::decode(&bytes).map_err(io::Error::other)?;
        match payload {
            ResultPayload::Statements(statements) => {
                for statement in statements {
                    if let Statement::ShowColumns(columns) = statement {
                        return Ok(columns);
                    }
                }
                Ok(Vec::new())
            }
            ResultPayload::Reader(_) => Err(io::Error::other(
                "MySQL-wire catalog column reader payloads are not supported",
            )),
        }
    }

    async fn write_results_from_cbor(&mut self, seq: u8, bytes: &[u8]) -> io::Result<()> {
        let payload = loom_result::result_view::decode(bytes).map_err(io::Error::other)?;
        match payload {
            ResultPayload::Statements(mut statements) => {
                if statements.len() != 1 {
                    return self
                        .write_err(
                            seq,
                            1235,
                            "MySQL-wire supports one result per query in the current profile",
                        )
                        .await;
                }
                self.write_statement(seq, statements.remove(0)).await
            }
            ResultPayload::Reader(_) => {
                self.write_err(seq, 1235, "MySQL-wire reader payloads are not supported")
                    .await
            }
        }
    }

    async fn write_binary_results_from_cbor(&mut self, seq: u8, bytes: &[u8]) -> io::Result<()> {
        let payload = loom_result::result_view::decode(bytes).map_err(io::Error::other)?;
        match payload {
            ResultPayload::Statements(mut statements) => {
                if statements.len() != 1 {
                    return self
                        .write_err(
                            seq,
                            1235,
                            "MySQL-wire supports one result per query in the current profile",
                        )
                        .await;
                }
                self.write_binary_statement(seq, statements.remove(0)).await
            }
            ResultPayload::Reader(_) => {
                self.write_err(seq, 1235, "MySQL-wire reader payloads are not supported")
                    .await
            }
        }
    }

    async fn write_statement(&mut self, seq: u8, statement: Statement) -> io::Result<()> {
        match statement {
            Statement::Select { labels, rows } => self.write_resultset(seq, labels, rows).await,
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
                self.write_resultset(seq, labels, rows).await
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
                self.write_resultset(seq, vec!["name".to_string(), "type".to_string()], rows)
                    .await
            }
            Statement::ShowVariable(ShowVariable::Tables(values))
            | Statement::ShowVariable(ShowVariable::Functions(values)) => {
                let rows = values
                    .into_iter()
                    .map(|value| vec![tabular::Value::Text(value)])
                    .collect();
                self.write_resultset(seq, vec!["value".to_string()], rows)
                    .await
            }
            Statement::ShowVariable(ShowVariable::Version(value)) => {
                self.write_resultset(
                    seq,
                    vec!["version".to_string()],
                    vec![vec![tabular::Value::Text(value)]],
                )
                .await
            }
            Statement::Insert(rows)
            | Statement::Delete(rows)
            | Statement::Update(rows)
            | Statement::DropTable(rows) => self.write_ok(seq, rows).await,
            Statement::Create
            | Statement::DropFunction
            | Statement::AlterTable
            | Statement::CreateIndex
            | Statement::DropIndex => self.write_ok(seq, 0).await,
            Statement::StartTransaction | Statement::Commit | Statement::Rollback => {
                self.write_err(
                    seq,
                    1235,
                    "MySQL-wire multi-statement transactions are not supported yet",
                )
                .await
            }
        }
    }

    async fn write_binary_statement(&mut self, seq: u8, statement: Statement) -> io::Result<()> {
        match statement {
            Statement::Select { labels, rows } => {
                self.write_binary_resultset(seq, labels, rows).await
            }
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
                self.write_binary_resultset(seq, labels, rows).await
            }
            other => self.write_statement(seq, other).await,
        }
    }

    async fn write_resultset(
        &mut self,
        mut seq: u8,
        labels: Vec<String>,
        rows: Vec<Vec<tabular::Value>>,
    ) -> io::Result<()> {
        self.write_packet(seq, &lenenc_int(labels.len() as u64))
            .await?;
        seq = seq.wrapping_add(1);
        for label in &labels {
            self.write_packet(seq, &column_definition(&self.database, label))
                .await?;
            seq = seq.wrapping_add(1);
        }
        self.write_eof(seq).await?;
        seq = seq.wrapping_add(1);
        for row in rows {
            let mut payload = Vec::new();
            for value in row {
                match value {
                    tabular::Value::Null => payload.push(0xfb),
                    other => {
                        let text = mysql_text_value(&other);
                        payload.extend_from_slice(&lenenc_int(text.len() as u64));
                        payload.extend_from_slice(text.as_bytes());
                    }
                }
            }
            self.write_packet(seq, &payload).await?;
            seq = seq.wrapping_add(1);
        }
        self.write_eof(seq).await
    }

    async fn write_binary_resultset(
        &mut self,
        mut seq: u8,
        labels: Vec<String>,
        rows: Vec<Vec<tabular::Value>>,
    ) -> io::Result<()> {
        self.write_packet(seq, &lenenc_int(labels.len() as u64))
            .await?;
        seq = seq.wrapping_add(1);
        for label in &labels {
            self.write_packet(seq, &column_definition(&self.database, label))
                .await?;
            seq = seq.wrapping_add(1);
        }
        self.write_eof(seq).await?;
        seq = seq.wrapping_add(1);
        for row in rows {
            let mut payload = vec![0x00];
            let null_bitmap_len = (labels.len() + 7 + 2) / 8;
            let bitmap_start = payload.len();
            payload.resize(payload.len() + null_bitmap_len, 0);
            for (idx, value) in row.into_iter().enumerate() {
                if matches!(value, tabular::Value::Null) {
                    let bit = idx + 2;
                    payload[bitmap_start + (bit / 8)] |= 1 << (bit % 8);
                } else {
                    let text = mysql_text_value(&value);
                    payload.extend_from_slice(&lenenc_int(text.len() as u64));
                    payload.extend_from_slice(text.as_bytes());
                }
            }
            self.write_packet(seq, &payload).await?;
            seq = seq.wrapping_add(1);
        }
        self.write_eof(seq).await
    }

    async fn write_stmt_prepare_ok(
        &mut self,
        statement_id: u32,
        parameter_count: usize,
        result_columns: &[String],
    ) -> io::Result<()> {
        let mut payload = vec![0x00];
        payload.extend_from_slice(&statement_id.to_le_bytes());
        payload.extend_from_slice(&(result_columns.len() as u16).to_le_bytes());
        payload.extend_from_slice(&(parameter_count as u16).to_le_bytes());
        payload.push(0);
        payload.extend_from_slice(&0_u16.to_le_bytes());
        self.write_packet(1, &payload).await?;
        let mut seq = 2;
        for idx in 0..parameter_count {
            let label = "?".to_string();
            let label = if parameter_count == 1 {
                label
            } else {
                format!("?{}", idx + 1)
            };
            self.write_packet(seq, &column_definition(&self.database, &label))
                .await?;
            seq = seq.wrapping_add(1);
        }
        if parameter_count > 0 {
            self.write_eof(seq).await?;
            seq = seq.wrapping_add(1);
        }
        for label in result_columns {
            self.write_packet(seq, &column_definition(&self.database, label))
                .await?;
            seq = seq.wrapping_add(1);
        }
        if !result_columns.is_empty() {
            self.write_eof(seq).await?;
        }
        Ok(())
    }

    async fn write_ok(&mut self, seq: u8, affected_rows: u64) -> io::Result<()> {
        let mut payload = vec![0x00];
        payload.extend_from_slice(&lenenc_int(affected_rows));
        payload.push(0x00);
        payload.extend_from_slice(&SERVER_STATUS_AUTOCOMMIT.to_le_bytes());
        payload.extend_from_slice(&0_u16.to_le_bytes());
        self.write_packet(seq, &payload).await
    }

    async fn write_eof(&mut self, seq: u8) -> io::Result<()> {
        let mut payload = vec![0xfe, 0x00, 0x00];
        payload.extend_from_slice(&SERVER_STATUS_AUTOCOMMIT.to_le_bytes());
        self.write_packet(seq, &payload).await
    }

    async fn write_hosted_error(&mut self, seq: u8, err: HostedError) -> io::Result<()> {
        self.write_err(seq, mysql_error_number(err.code), &err.message)
            .await
    }

    async fn write_err(&mut self, seq: u8, code: u16, message: &str) -> io::Result<()> {
        let mut payload = vec![0xff];
        payload.extend_from_slice(&code.to_le_bytes());
        payload.extend_from_slice(b"#HY000");
        payload.extend_from_slice(message.as_bytes());
        self.write_packet(seq, &payload).await
    }

    async fn read_packet(&mut self) -> io::Result<MysqlPacket> {
        let mut header = [0_u8; 4];
        self.stream_mut()?.read_exact(&mut header).await?;
        let len = header[0] as usize | ((header[1] as usize) << 8) | ((header[2] as usize) << 16);
        let mut payload = vec![0_u8; len];
        self.stream_mut()?.read_exact(&mut payload).await?;
        Ok(MysqlPacket { payload })
    }

    async fn write_packet(&mut self, sequence: u8, payload: &[u8]) -> io::Result<()> {
        let len = payload.len();
        if len > 0x00ff_ffff {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "MySQL packet exceeds 24-bit length",
            ));
        }
        self.stream_mut()?.write_all(&[(len & 0xff) as u8]).await?;
        self.stream_mut()?
            .write_all(&[((len >> 8) & 0xff) as u8])
            .await?;
        self.stream_mut()?
            .write_all(&[((len >> 16) & 0xff) as u8])
            .await?;
        self.stream_mut()?.write_all(&[sequence]).await?;
        self.stream_mut()?.write_all(payload).await?;
        self.stream_mut()?.flush().await
    }
}

struct MysqlPacket {
    payload: Vec<u8>,
}

pub async fn serve_sql_mysql_wire<S>(
    listener: TcpListener,
    kernel: HostedKernel,
    workspace: impl Into<String>,
    database: impl Into<String>,
    shutdown: S,
) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    serve_sql_mysql_wire_inner(
        listener,
        kernel,
        workspace,
        database,
        mysql_no_tls_config(),
        shutdown,
    )
    .await
}

#[cfg(feature = "tls")]
pub async fn serve_sql_mysql_wire_with_tls<S>(
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
    serve_sql_mysql_wire_inner(listener, kernel, workspace, database, Some(tls), shutdown).await
}

async fn serve_sql_mysql_wire_inner<S>(
    listener: TcpListener,
    kernel: HostedKernel,
    workspace: impl Into<String>,
    database: impl Into<String>,
    #[cfg_attr(not(feature = "tls"), allow(unused_variables))] tls: MysqlTls,
    shutdown: S,
) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    let workspace = workspace.into();
    let database = database.into();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => return Ok(()),
            incoming = listener.accept() => {
                let (stream, _) = incoming?;
                let connection = MysqlConnection {
                    stream: Some(MysqlIo::Plain(stream)),
                    kernel: kernel.clone(),
                    workspace: workspace.clone(),
                    database: database.clone(),
                    auth: None,
                    client_capabilities: 0,
                    salt: mysql_random_salt()?,
                    prepared_statements: BTreeMap::new(),
                    next_statement_id: 1,
                    #[cfg(feature = "tls")]
                    tls: tls.clone(),
                };
                tokio::spawn(async move {
                    let _ = connection.run().await;
                });
            }
        }
    }
}

#[cfg(feature = "tls")]
fn mysql_no_tls_config() -> MysqlTls {
    None
}

#[cfg(not(feature = "tls"))]
fn mysql_no_tls_config() -> MysqlTls {}

fn mysql_transaction_boundary(query: &str) -> bool {
    let normalized = normalize_sql(query);
    matches!(
        normalized.as_str(),
        "BEGIN" | "START TRANSACTION" | "COMMIT" | "ROLLBACK"
    )
}

fn mysql_noop_statement(query: &str) -> bool {
    let normalized = normalize_sql(query);
    normalized.starts_with("SET ")
        || normalized == "DO 0"
        || normalized == "SELECT 1 FROM DUAL WHERE FALSE"
}

fn mysql_select_projection_labels(query: &str) -> Option<Vec<String>> {
    let trimmed = query.trim().trim_end_matches(';').trim();
    if !trimmed
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("SELECT"))
    {
        return None;
    }
    let projection = split_sql_before_from(trimmed.get(6..)?.trim())?;
    if projection.trim() == "*" {
        return None;
    }
    let labels = split_sql_projection(projection)
        .into_iter()
        .map(mysql_projection_label)
        .collect::<Vec<_>>();
    Some(labels)
}

fn split_sql_before_from(input: &str) -> Option<&str> {
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;
    for (idx, _) in input.char_indices() {
        let rest = &input[idx..];
        let ch = rest.chars().next()?;
        match ch {
            '\'' if !in_double && !in_backtick => in_single = !in_single,
            '"' if !in_single && !in_backtick => in_double = !in_double,
            '`' if !in_single && !in_double => in_backtick = !in_backtick,
            _ if !in_single
                && !in_double
                && !in_backtick
                && rest
                    .get(..6)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(" FROM ")) =>
            {
                return Some(input[..idx].trim());
            }
            _ => {}
        }
    }
    Some(input.trim())
}

fn split_sql_projection(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;
    for (idx, ch) in input.char_indices() {
        match ch {
            '\'' if !in_double && !in_backtick => in_single = !in_single,
            '"' if !in_single && !in_backtick => in_double = !in_double,
            '`' if !in_single && !in_double => in_backtick = !in_backtick,
            ',' if !in_single && !in_double && !in_backtick => {
                parts.push(input[start..idx].trim());
                start = idx + 1;
            }
            _ => {}
        }
    }
    parts.push(input[start..].trim());
    parts
}

fn mysql_projection_label(expression: &str) -> String {
    let trimmed = expression.trim();
    let upper = trimmed.to_ascii_uppercase();
    if let Some((_, alias)) = upper.rsplit_once(" AS ") {
        let start = trimmed.len() - alias.len();
        return first_identifier(&trimmed[start..]).to_string();
    }
    first_identifier(
        trimmed
            .rsplit_once('.')
            .map(|(_, last)| last)
            .unwrap_or(trimmed),
    )
    .to_string()
}

fn count_mysql_parameter_markers(query: &str) -> usize {
    mysql_parameter_marker_positions(query).len()
}

fn mysql_parameter_markers_to_postgres(query: &str) -> io::Result<String> {
    rewrite_mysql_markers(query, |idx| Ok(format!("${idx}")))
}

fn replace_mysql_parameter_markers_with_null(query: &str) -> String {
    let result: Result<String, std::convert::Infallible> =
        rewrite_mysql_markers(query, |_| Ok("NULL".to_string()));
    result.unwrap_or_else(|_| query.to_string())
}

fn rewrite_mysql_parameter_markers(query: &str, parameters: &[String]) -> Result<String, String> {
    rewrite_mysql_markers(query, |idx| {
        parameters
            .get(idx - 1)
            .cloned()
            .ok_or_else(|| format!("MySQL parameter marker ?{idx} has no bound value"))
    })
}

fn rewrite_mysql_markers<E>(
    query: &str,
    mut replacement: impl FnMut(usize) -> Result<String, E>,
) -> Result<String, E> {
    let mut out = String::with_capacity(query.len());
    let mut marker_index = 0;
    let mut chars = query.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;
    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double && !in_backtick => {
                out.push(ch);
                if in_single && chars.peek().is_some_and(|next| *next == '\'') {
                    out.push(chars.next().unwrap());
                } else {
                    in_single = !in_single;
                }
            }
            '"' if !in_single && !in_backtick => {
                out.push(ch);
                in_double = !in_double;
            }
            '`' if !in_single && !in_double => {
                out.push(ch);
                in_backtick = !in_backtick;
            }
            '?' if !in_single && !in_double && !in_backtick => {
                marker_index += 1;
                out.push_str(&replacement(marker_index)?);
            }
            _ => out.push(ch),
        }
    }
    Ok(out)
}

fn mysql_parameter_marker_positions(query: &str) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;
    let mut chars = query.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        match ch {
            '\'' if !in_double && !in_backtick => {
                if in_single && chars.peek().is_some_and(|(_, next)| *next == '\'') {
                    chars.next();
                } else {
                    in_single = !in_single;
                }
            }
            '"' if !in_single && !in_backtick => in_double = !in_double,
            '`' if !in_single && !in_double => in_backtick = !in_backtick,
            '?' if !in_single && !in_double && !in_backtick => positions.push(idx),
            _ => {}
        }
    }
    positions
}

fn mysql_random_salt() -> io::Result<[u8; MYSQL_SALT_LEN]> {
    let mut salt = [0u8; MYSQL_SALT_LEN];
    getrandom::fill(&mut salt).map_err(|err| io::Error::other(format!("rng: {err}")))?;
    for byte in &mut salt {
        if *byte == 0 {
            *byte = 1;
        }
    }
    Ok(salt)
}

fn normalize_sql(query: &str) -> String {
    query
        .trim()
        .trim_end_matches(';')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase()
}

fn single_text_result(label: &str, value: &str) -> (Vec<String>, Vec<Vec<tabular::Value>>) {
    (
        vec![label.to_string()],
        vec![vec![tabular::Value::Text(value.to_string())]],
    )
}

fn mysql_show_variables_response(normalized: &str) -> (Vec<String>, Vec<Vec<tabular::Value>>) {
    let candidates = [
        ("version_comment", "Loom MySQL-wire compatibility profile"),
        ("version", "8.0.0-loom"),
        ("autocommit", "ON"),
        ("character_set_client", "utf8mb4"),
        ("character_set_connection", "utf8mb4"),
        ("character_set_results", "utf8mb4"),
        ("lower_case_table_names", "0"),
        ("sql_mode", ""),
    ];
    let rows = candidates
        .into_iter()
        .filter(|(name, _)| {
            !normalized.contains(" LIKE ")
                || normalized.contains(&name.to_ascii_uppercase())
                || normalized.contains("CHARACTER_SET_%")
        })
        .map(|(name, value)| {
            vec![
                tabular::Value::Text(name.to_string()),
                tabular::Value::Text(value.to_string()),
            ]
        })
        .collect();
    (vec!["Variable_name".to_string(), "Value".to_string()], rows)
}

fn mysql_describe_table(normalized: &str) -> Option<&str> {
    let trimmed = normalized.trim().trim_end_matches(';').trim();
    let upper = trimmed.to_ascii_uppercase();
    for prefix in ["SHOW COLUMNS FROM ", "DESCRIBE ", "DESC "] {
        if upper.starts_with(prefix) {
            return Some(first_identifier(&trimmed[prefix.len()..]));
        }
    }
    None
}

fn first_identifier(input: &str) -> &str {
    input
        .split([' ', '\t', '\r', '\n'])
        .next()
        .unwrap_or("")
        .trim_matches('`')
        .trim_matches('"')
        .trim_matches('\'')
}

fn mysql_table_name_filter(normalized: &str) -> Option<String> {
    for marker in [
        "TABLE_NAME = '",
        "TABLE_NAME='",
        "TABLE_NAME = \"",
        "TABLE_NAME=\"",
    ] {
        if let Some(rest) = normalized.split_once(marker).map(|(_, rest)| rest) {
            let quote = if marker.ends_with('"') { '"' } else { '\'' };
            let end = rest.find(quote).unwrap_or(rest.len());
            return Some(rest[..end].to_ascii_lowercase());
        }
    }
    None
}

fn mysql_information_schema_table_labels(normalized: &str) -> Vec<String> {
    let select = normalized
        .split_once(" FROM ")
        .map(|(select, _)| select)
        .unwrap_or(normalized);
    let mut labels = Vec::new();
    for (needle, label) in [
        ("TABLE_SCHEMA", "table_schema"),
        ("TABLE_NAME", "table_name"),
        ("TABLE_TYPE", "table_type"),
        ("ENGINE", "engine"),
    ] {
        if select.contains(needle) || select == "SELECT *" {
            labels.push(label.to_string());
        }
    }
    if labels.is_empty() {
        labels.push("table_name".to_string());
    }
    labels
}

fn mysql_information_schema_column_labels(normalized: &str) -> Vec<String> {
    let select = normalized
        .split_once(" FROM ")
        .map(|(select, _)| select)
        .unwrap_or(normalized);
    let mut labels = Vec::new();
    for (needle, label) in [
        ("TABLE_SCHEMA", "table_schema"),
        ("TABLE_NAME", "table_name"),
        ("COLUMN_NAME", "column_name"),
        ("ORDINAL_POSITION", "ordinal_position"),
        ("DATA_TYPE", "data_type"),
        ("COLUMN_TYPE", "column_type"),
        ("IS_NULLABLE", "is_nullable"),
        ("COLUMN_DEFAULT", "column_default"),
    ] {
        if select.contains(needle) || select == "SELECT *" {
            labels.push(label.to_string());
        }
    }
    if labels.is_empty() {
        labels.push("column_name".to_string());
    }
    labels
}

fn mysql_information_schema_table_value(
    label: &str,
    database: &str,
    table: &str,
) -> tabular::Value {
    match label {
        "table_schema" => tabular::Value::Text(database.to_string()),
        "table_name" => tabular::Value::Text(table.to_string()),
        "table_type" => tabular::Value::Text("BASE TABLE".to_string()),
        "engine" => tabular::Value::Text("Loom".to_string()),
        _ => tabular::Value::Null,
    }
}

fn mysql_information_schema_column_value(
    label: &str,
    database: &str,
    table: &str,
    ordinal: usize,
    column: &loom_result::result_view::Column,
) -> tabular::Value {
    match label {
        "table_schema" => tabular::Value::Text(database.to_string()),
        "table_name" => tabular::Value::Text(table.to_string()),
        "column_name" => tabular::Value::Text(column.name.clone()),
        "ordinal_position" => tabular::Value::Int(ordinal as i64),
        "data_type" => tabular::Value::Text(mysql_type_name(&column.type_name)),
        "column_type" => tabular::Value::Text(mysql_type_name(&column.type_name)),
        "is_nullable" => tabular::Value::Text("YES".to_string()),
        "column_default" => tabular::Value::Null,
        _ => tabular::Value::Null,
    }
}

fn mysql_type_name(type_name: &str) -> String {
    let upper = type_name.to_ascii_uppercase();
    if upper.contains("INT") {
        "bigint".to_string()
    } else if upper.contains("FLOAT") || upper.contains("DOUBLE") || upper.contains("REAL") {
        "double".to_string()
    } else if upper.contains("BOOL") {
        "tinyint(1)".to_string()
    } else if upper.contains("BLOB") || upper.contains("BYTE") {
        "blob".to_string()
    } else {
        "text".to_string()
    }
}

fn mysql_error_number(code: Code) -> u16 {
    match code {
        Code::AuthenticationFailed | Code::E2eKeyInvalid => 1045,
        Code::PermissionDenied => 1142,
        Code::InvalidArgument | Code::CorruptObject => 1064,
        Code::NotFound => 1049,
        Code::Unsupported => 1235,
        _ => 1105,
    }
}

fn parse_mysql_execute_parameters(
    payload: &[u8],
    statement: &MysqlPreparedStatement,
) -> Result<Vec<String>, String> {
    if payload.len() < 9 {
        return Err("malformed MySQL prepared statement execute".to_string());
    }
    let count = statement.parameter_count;
    let null_bitmap_len = count.div_ceil(8);
    let null_bitmap_start = 9;
    let null_bitmap_end = null_bitmap_start + null_bitmap_len;
    if payload.len() < null_bitmap_end + usize::from(count > 0) {
        return Err("malformed MySQL prepared statement execute parameters".to_string());
    }
    let null_bitmap = &payload[null_bitmap_start..null_bitmap_end];
    let mut cursor = null_bitmap_end;
    let new_params_bound = if count == 0 {
        false
    } else {
        let flag = payload[cursor] != 0;
        cursor += 1;
        flag
    };
    let mut mysql_types = vec![MysqlParameterType::default(); count];
    if new_params_bound {
        for ty in &mut mysql_types {
            if cursor + 2 > payload.len() {
                return Err("malformed MySQL prepared statement parameter types".to_string());
            }
            *ty = MysqlParameterType {
                type_code: payload[cursor],
                unsigned: payload[cursor + 1] & 0x80 != 0,
            };
            cursor += 2;
        }
    } else {
        for (idx, ty) in mysql_types.iter_mut().enumerate() {
            *ty = mysql_type_for_data_type(
                statement
                    .parameter_types
                    .get(idx)
                    .and_then(|ty| ty.as_ref()),
            );
        }
    }
    let mut out = Vec::with_capacity(count);
    for (idx, mysql_type) in mysql_types.iter().copied().enumerate().take(count) {
        if mysql_parameter_is_null(null_bitmap, idx) {
            out.push("NULL".to_string());
            continue;
        }
        let literal = read_mysql_binary_parameter(payload, &mut cursor, mysql_type)?;
        out.push(literal);
    }
    Ok(out)
}

fn mysql_parameter_is_null(bitmap: &[u8], idx: usize) -> bool {
    let byte = bitmap.get(idx / 8).copied().unwrap_or(0);
    byte & (1 << (idx % 8)) != 0
}

#[derive(Clone, Copy)]
struct MysqlParameterType {
    type_code: u8,
    unsigned: bool,
}

impl Default for MysqlParameterType {
    fn default() -> Self {
        Self {
            type_code: MYSQL_TYPE_VAR_STRING,
            unsigned: false,
        }
    }
}

fn mysql_type_for_data_type(ty: Option<&DataType>) -> MysqlParameterType {
    let (type_code, unsigned) = match ty {
        Some(DataType::Uint8) => (MYSQL_TYPE_TINY, true),
        Some(DataType::Uint16) => (MYSQL_TYPE_SHORT, true),
        Some(DataType::Uint32) => (MYSQL_TYPE_LONG, true),
        Some(DataType::Uint64) | Some(DataType::Uint128) => (MYSQL_TYPE_LONGLONG, true),
        Some(DataType::Boolean) | Some(DataType::Int8) => (MYSQL_TYPE_TINY, false),
        Some(DataType::Int16) => (MYSQL_TYPE_SHORT, false),
        Some(DataType::Int32) => (MYSQL_TYPE_LONG, false),
        Some(DataType::Int) | Some(DataType::Int128) => (MYSQL_TYPE_LONGLONG, false),
        Some(DataType::Float32) => (MYSQL_TYPE_FLOAT, false),
        Some(DataType::Float) => (MYSQL_TYPE_DOUBLE, false),
        _ => (MYSQL_TYPE_VAR_STRING, false),
    };
    MysqlParameterType {
        type_code,
        unsigned,
    }
}

fn read_mysql_binary_parameter(
    payload: &[u8],
    cursor: &mut usize,
    mysql_type: MysqlParameterType,
) -> Result<String, String> {
    match mysql_type.type_code {
        MYSQL_TYPE_NULL => Ok("NULL".to_string()),
        MYSQL_TYPE_TINY => {
            let byte = *payload
                .get(*cursor)
                .ok_or_else(|| "short MySQL tinyint parameter".to_string())?;
            *cursor += 1;
            let value = if mysql_type.unsigned {
                u8::from_le_bytes([byte]).to_string()
            } else {
                i8::from_le_bytes([byte]).to_string()
            };
            Ok(value)
        }
        MYSQL_TYPE_SHORT => {
            let bytes =
                read_exact_parameter_bytes(payload, cursor, 2, "short MySQL smallint parameter")?;
            if mysql_type.unsigned {
                Ok(u16::from_le_bytes(bytes.try_into().unwrap()).to_string())
            } else {
                Ok(i16::from_le_bytes(bytes.try_into().unwrap()).to_string())
            }
        }
        MYSQL_TYPE_LONG => {
            let bytes =
                read_exact_parameter_bytes(payload, cursor, 4, "short MySQL int parameter")?;
            if mysql_type.unsigned {
                Ok(u32::from_le_bytes(bytes.try_into().unwrap()).to_string())
            } else {
                Ok(i32::from_le_bytes(bytes.try_into().unwrap()).to_string())
            }
        }
        MYSQL_TYPE_LONGLONG => {
            let bytes =
                read_exact_parameter_bytes(payload, cursor, 8, "short MySQL bigint parameter")?;
            if mysql_type.unsigned {
                Ok(u64::from_le_bytes(bytes.try_into().unwrap()).to_string())
            } else {
                Ok(i64::from_le_bytes(bytes.try_into().unwrap()).to_string())
            }
        }
        MYSQL_TYPE_FLOAT => {
            let bytes =
                read_exact_parameter_bytes(payload, cursor, 4, "short MySQL float parameter")?;
            Ok(f32::from_bits(u32::from_le_bytes(bytes.try_into().unwrap())).to_string())
        }
        MYSQL_TYPE_DOUBLE => {
            let bytes =
                read_exact_parameter_bytes(payload, cursor, 8, "short MySQL double parameter")?;
            Ok(f64::from_bits(u64::from_le_bytes(bytes.try_into().unwrap())).to_string())
        }
        MYSQL_TYPE_VAR_STRING | MYSQL_TYPE_STRING => {
            let (len, used) = read_lenenc_int(&payload[*cursor..])
                .ok_or_else(|| "bad MySQL string parameter length".to_string())?;
            *cursor += used;
            let end = *cursor + len as usize;
            if end > payload.len() {
                return Err("short MySQL string parameter".to_string());
            }
            let value = std::str::from_utf8(&payload[*cursor..end])
                .map_err(|_| "MySQL string parameter is not valid UTF-8".to_string())?;
            *cursor = end;
            Ok(sql_quoted(value))
        }
        _ => Err("MySQL parameter type is not supported yet".to_string()),
    }
}

fn read_exact_parameter_bytes<'a>(
    payload: &'a [u8],
    cursor: &mut usize,
    len: usize,
    err: &str,
) -> Result<&'a [u8], String> {
    let end = *cursor + len;
    if end > payload.len() {
        return Err(err.to_string());
    }
    let bytes = &payload[*cursor..end];
    *cursor = end;
    Ok(bytes)
}

fn read_u32_le(payload: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        payload.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn sql_quoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            out.push('\'');
        }
        out.push(ch);
    }
    out.push('\'');
    out
}

fn column_definition(database: &str, label: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    lenenc_str(&mut payload, "def");
    lenenc_str(&mut payload, database);
    lenenc_str(&mut payload, "");
    lenenc_str(&mut payload, "");
    lenenc_str(&mut payload, label);
    lenenc_str(&mut payload, label);
    payload.push(0x0c);
    payload.extend_from_slice(&33_u16.to_le_bytes());
    payload.extend_from_slice(&1024_u32.to_le_bytes());
    payload.push(MYSQL_TYPE_VAR_STRING);
    payload.extend_from_slice(&0_u16.to_le_bytes());
    payload.push(0);
    payload.extend_from_slice(&[0, 0]);
    payload
}

fn lenenc_str(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&lenenc_int(value.len() as u64));
    out.extend_from_slice(value.as_bytes());
}

fn lenenc_int(value: u64) -> Vec<u8> {
    if value < 251 {
        vec![value as u8]
    } else if value <= 0xffff {
        let mut out = vec![0xfc];
        out.extend_from_slice(&(value as u16).to_le_bytes());
        out
    } else if value <= 0x00ff_ffff {
        vec![
            0xfd,
            (value & 0xff) as u8,
            ((value >> 8) & 0xff) as u8,
            ((value >> 16) & 0xff) as u8,
        ]
    } else {
        let mut out = vec![0xfe];
        out.extend_from_slice(&value.to_le_bytes());
        out
    }
}

fn read_lenenc_int(input: &[u8]) -> Option<(u64, usize)> {
    match *input.first()? {
        value @ 0x00..=0xfa => Some((value as u64, 1)),
        0xfc if input.len() >= 3 => Some((u16::from_le_bytes([input[1], input[2]]) as u64, 3)),
        0xfd if input.len() >= 4 => Some((
            input[1] as u64 | ((input[2] as u64) << 8) | ((input[3] as u64) << 16),
            4,
        )),
        0xfe if input.len() >= 9 => Some((u64::from_le_bytes(input[1..9].try_into().ok()?), 9)),
        _ => None,
    }
}

fn mysql_text_value(value: &tabular::Value) -> String {
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

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use std::env;
    use std::fs;
    use std::process::Command;

    use super::*;
    use crate::test_support::{init, nid, temp_path};
    use sha1::{Digest as Sha1Digest, Sha1};

    #[test]
    fn mysql_lenenc_round_trips_profile_lengths() {
        for value in [0, 250, 251, 65_535, 65_536, 16_777_215, 16_777_216] {
            let encoded = lenenc_int(value);
            let (decoded, used) = read_lenenc_int(&encoded).unwrap();
            assert_eq!(decoded, value);
            assert_eq!(used, encoded.len());
        }
    }

    #[test]
    fn mysql_binary_execute_decodes_unsigned_integer_parameters() {
        let statement = MysqlPreparedStatement {
            sql: "SELECT ?".to_string(),
            parameter_count: 1,
            parameter_types: Vec::new(),
        };
        let mut payload = Vec::new();
        payload.extend_from_slice(&1_u32.to_le_bytes());
        payload.push(0);
        payload.extend_from_slice(&1_u32.to_le_bytes());
        payload.push(0);
        payload.push(1);
        payload.push(MYSQL_TYPE_LONG);
        payload.push(0x80);
        payload.extend_from_slice(&u32::MAX.to_le_bytes());
        let parameters = parse_mysql_execute_parameters(&payload, &statement).unwrap();
        assert_eq!(parameters, vec!["4294967295"]);
    }

    #[test]
    fn mysql_wire_raw_protocol_transcript() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let path = temp_path("mysql-wire-transcript");
            init(&path, None);
            let kernel = HostedKernel::new(&path);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(serve_sql_mysql_wire(
                listener,
                kernel,
                "main",
                "db",
                async move {
                    let _ = shutdown_rx.await;
                },
            ));

            let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
            let handshake = read_client_packet(&mut client).await;
            assert_eq!(handshake.payload[0], 10);
            write_client_packet(&mut client, 1, &client_handshake_response()).await;
            let auth_ok = read_client_packet(&mut client).await;
            assert_eq!(auth_ok.payload[0], 0x00);

            write_client_query(
                &mut client,
                "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)",
            )
            .await;
            let create = read_client_packet(&mut client).await;
            assert_eq!(create.payload[0], 0x00);

            write_client_query(&mut client, "INSERT INTO t VALUES (1, 'a')").await;
            let insert = read_client_packet(&mut client).await;
            assert_eq!(insert.payload[0], 0x00);

            write_client_query(&mut client, "SELECT id, name FROM t").await;
            let columns = read_client_packet(&mut client).await;
            assert_eq!(columns.payload[0], 2);
            let _id_column = read_client_packet(&mut client).await;
            let _name_column = read_client_packet(&mut client).await;
            let _columns_eof = read_client_packet(&mut client).await;
            let row = read_client_packet(&mut client).await;
            assert!(row.payload.windows(1).any(|window| window == b"1"));
            assert!(row.payload.windows(1).any(|window| window == b"a"));
            let _rows_eof = read_client_packet(&mut client).await;

            write_client_query(&mut client, "SHOW TABLES").await;
            let tables = read_mysql_text_result(&mut client).await;
            assert_eq!(tables.0, vec!["Tables_in_db"]);
            assert_eq!(tables.1, vec![vec!["t".to_string()]]);

            write_client_query(&mut client, "DESCRIBE t").await;
            let describe = read_mysql_text_result(&mut client).await;
            assert_eq!(
                describe.0,
                vec!["Field", "Type", "Null", "Key", "Default", "Extra"]
            );
            assert!(
                describe
                    .1
                    .iter()
                    .any(|row| row.first().is_some_and(|v| v == "id"))
            );

            write_client_query(
                &mut client,
                "SELECT table_name FROM information_schema.tables WHERE table_schema = DATABASE()",
            )
            .await;
            let info_tables = read_mysql_text_result(&mut client).await;
            assert_eq!(info_tables.0, vec!["table_name"]);
            assert_eq!(info_tables.1, vec![vec!["t".to_string()]]);

            write_client_packet(&mut client, 0, &[COM_PING]).await;
            let ping = read_client_packet(&mut client).await;
            assert_eq!(ping.payload[0], 0x00);

            let mut prepare = vec![COM_STMT_PREPARE];
            prepare.extend_from_slice(b"SELECT id, name FROM t WHERE id = ?");
            write_client_packet(&mut client, 0, &prepare).await;
            let prepare_ok = read_client_packet(&mut client).await;
            assert_eq!(prepare_ok.payload[0], 0x00);
            let statement_id = u32::from_le_bytes(prepare_ok.payload[1..5].try_into().unwrap());
            assert_eq!(
                u16::from_le_bytes(prepare_ok.payload[5..7].try_into().unwrap()),
                2
            );
            assert_eq!(
                u16::from_le_bytes(prepare_ok.payload[7..9].try_into().unwrap()),
                1
            );
            let _param = read_client_packet(&mut client).await;
            let _params_eof = read_client_packet(&mut client).await;
            let _id_column = read_client_packet(&mut client).await;
            let _name_column = read_client_packet(&mut client).await;
            let _columns_eof = read_client_packet(&mut client).await;

            let mut execute = vec![COM_STMT_EXECUTE];
            execute.extend_from_slice(&statement_id.to_le_bytes());
            execute.push(0);
            execute.extend_from_slice(&1_u32.to_le_bytes());
            execute.push(0);
            execute.push(1);
            execute.push(MYSQL_TYPE_LONG);
            execute.push(0);
            execute.extend_from_slice(&1_i32.to_le_bytes());
            write_client_packet(&mut client, 0, &execute).await;
            let binary = read_mysql_binary_result(&mut client).await;
            assert_eq!(binary.0, 2);
            assert_eq!(binary.1, vec![vec!["1".to_string(), "a".to_string()]]);

            let mut reset = vec![COM_STMT_RESET];
            reset.extend_from_slice(&statement_id.to_le_bytes());
            write_client_packet(&mut client, 0, &reset).await;
            let reset_ok = read_client_packet(&mut client).await;
            assert_eq!(reset_ok.payload[0], 0x00);

            let mut close = vec![COM_STMT_CLOSE];
            close.extend_from_slice(&statement_id.to_le_bytes());
            write_client_packet(&mut client, 0, &close).await;

            let _ = shutdown_tx.send(());
            server.await.unwrap().unwrap();
            fs::remove_file(path).unwrap();
        });
    }

    #[test]
    fn mysql_wire_app_credential_authenticates() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let path = temp_path("mysql-wire-app-credential");
            let user = nid(2);
            init(&path, Some(user));
            let kernel = HostedKernel::new(&path);
            let root = HostedAuth::passphrase(nid(1), "root-pass", "mysql-app-admin");
            let credential = kernel
                .admin()
                .create_app_credential(&root, user, "mysql".to_string())
                .unwrap();
            let secret = json_field(&credential, "secret");
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(serve_sql_mysql_wire(
                listener,
                kernel,
                "main",
                "db",
                async move {
                    let _ = shutdown_rx.await;
                },
            ));

            let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
            let _handshake = read_client_packet(&mut client).await;
            write_client_packet(
                &mut client,
                1,
                &client_handshake_response_with_secret(user, &secret),
            )
            .await;
            let auth_ok = read_client_packet(&mut client).await;
            assert_eq!(auth_ok.payload[0], 0x00);

            let _ = shutdown_tx.send(());
            server.await.unwrap().unwrap();
            fs::remove_file(path).unwrap();
        });
    }

    #[test]
    fn mysql_wire_native_password_app_credential_authenticates() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let path = temp_path("mysql-wire-native-app-credential");
            let user = nid(2);
            init(&path, Some(user));
            let kernel = HostedKernel::new(&path);
            let root = HostedAuth::passphrase(nid(1), "root-pass", "mysql-native-app-admin");
            let credential = kernel
                .admin()
                .create_app_credential(&root, user, "mysql".to_string())
                .unwrap();
            let secret = json_field(&credential, "secret");
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(serve_sql_mysql_wire(
                listener,
                kernel,
                "main",
                "db",
                async move {
                    let _ = shutdown_rx.await;
                },
            ));

            let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
            let handshake = read_client_packet(&mut client).await;
            let (salt, plugin) = mysql_handshake_auth(&handshake.payload);
            assert_eq!(plugin, MYSQL_NATIVE_PASSWORD);
            assert!(salt.iter().all(|byte| *byte != 0));
            write_client_packet(
                &mut client,
                1,
                &client_native_password_response(user, &secret, &salt),
            )
            .await;
            let auth_ok = read_client_packet(&mut client).await;
            assert_eq!(auth_ok.payload[0], 0x00);

            write_client_query(&mut client, "SELECT USER()").await;
            let user_result = read_mysql_text_result(&mut client).await;
            assert_eq!(user_result.0, vec!["USER()"]);
            assert_eq!(user_result.1, vec![vec!["loom_app@localhost".to_string()]]);

            let _ = shutdown_tx.send(());
            server.await.unwrap().unwrap();
            fs::remove_file(path).unwrap();
        });
    }

    #[cfg(feature = "tls")]
    #[test]
    fn mysql_wire_client_ssl_upgrades_before_authentication() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let path = temp_path("mysql-wire-client-ssl");
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
            let server = tokio::spawn(serve_sql_mysql_wire_with_tls(
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
            let handshake = read_client_packet(&mut client).await;
            assert_ne!(handshake.payload[0], 0);
            let caps = mysql_handshake_capabilities(&handshake.payload);
            assert_ne!(caps & CLIENT_SSL, 0);
            write_client_packet(&mut client, 1, &client_ssl_request()).await;
            let tls_client = mysql_test_tls_connector(cert.der().as_ref().to_vec());
            let server_name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
            let mut client = tls_client.connect(server_name, client).await.unwrap();
            write_client_packet(&mut client, 2, &client_handshake_response()).await;
            let auth_ok = read_client_packet(&mut client).await;
            assert_eq!(auth_ok.payload[0], 0x00);

            write_client_query(&mut client, "SELECT USER()").await;
            let user_result = read_mysql_text_result(&mut client).await;
            assert_eq!(user_result.0, vec!["USER()"]);
            assert_eq!(user_result.1, vec![vec![format!("{}@localhost", nid(1))]]);

            let _ = shutdown_tx.send(());
            server.await.unwrap().unwrap();
            fs::remove_file(path).unwrap();
        });
    }

    #[test]
    fn mysql_cli_transcript_covers_metadata_when_available() {
        let Some(mysql) = mysql_client_path() else {
            return;
        };
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let path = temp_path("mysql-cli-transcript");
            init(&path, None);
            let kernel = HostedKernel::new(&path);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let port = addr.port().to_string();
            let user = nid(1).to_string();
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(serve_sql_mysql_wire(
                listener,
                kernel,
                "main",
                "db",
                async move {
                    let _ = shutdown_rx.await;
                },
            ));

            mysql_cli(&mysql, &port, &user, "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)");
            mysql_cli(&mysql, &port, &user, "INSERT INTO t VALUES (1, 'a')");
            let selected = mysql_cli(&mysql, &port, &user, "SELECT id, name FROM t");
            assert!(selected.contains("1\ta"), "{selected}");
            let tables = mysql_cli(&mysql, &port, &user, "SHOW TABLES");
            assert!(tables.lines().any(|line| line == "t"), "{tables}");
            let describe = mysql_cli(&mysql, &port, &user, "DESCRIBE t");
            assert!(describe.contains("id\tbigint"), "{describe}");
            let info_tables = mysql_cli(
                &mysql,
                &port,
                &user,
                "SELECT table_name FROM information_schema.tables WHERE table_schema = DATABASE()",
            );
            assert!(info_tables.lines().any(|line| line == "t"), "{info_tables}");
            let info_columns = mysql_cli(
                &mysql,
                &port,
                &user,
                "SELECT column_name, data_type FROM information_schema.columns WHERE table_name = 't'",
            );
            assert!(info_columns.contains("id\tbigint"), "{info_columns}");

            let _ = shutdown_tx.send(());
            server.await.unwrap().unwrap();
            fs::remove_file(path).unwrap();
        });
    }

    #[test]
    fn mysql_node_mysql2_transcript_when_available() {
        let Some(node) = node_mysql2_client() else {
            return;
        };
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let path = temp_path("mysql-node-mysql2-transcript");
            init(&path, None);
            let kernel = HostedKernel::new(&path);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let port = addr.port().to_string();
            let user = nid(1).to_string();
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(serve_sql_mysql_wire(
                listener,
                kernel,
                "main",
                "db",
                async move {
                    let _ = shutdown_rx.await;
                },
            ));

            node_mysql2(&node, &port, &user);

            let _ = shutdown_tx.send(());
            server.await.unwrap().unwrap();
            fs::remove_file(path).unwrap();
        });
    }

    #[test]
    fn mysql_python_driver_transcript_when_available() {
        let Some(python) = python_mysql_client() else {
            return;
        };
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let path = temp_path("mysql-python-driver-transcript");
            init(&path, None);
            let kernel = HostedKernel::new(&path);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let port = addr.port().to_string();
            let user = nid(1).to_string();
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(serve_sql_mysql_wire(
                listener,
                kernel,
                "main",
                "db",
                async move {
                    let _ = shutdown_rx.await;
                },
            ));

            python_mysql(&python, &port, &user);

            let _ = shutdown_tx.send(());
            server.await.unwrap().unwrap();
            fs::remove_file(path).unwrap();
        });
    }

    #[test]
    fn mysql_jdbc_connectorj_transcript_when_available() {
        let Some(jdbc) = jdbc_mysql_client() else {
            return;
        };
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let path = temp_path("mysql-jdbc-connectorj-transcript");
            init(&path, None);
            let kernel = HostedKernel::new(&path);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let port = addr.port().to_string();
            let user = nid(1).to_string();
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(serve_sql_mysql_wire(
                listener,
                kernel,
                "main",
                "db",
                async move {
                    let _ = shutdown_rx.await;
                },
            ));

            jdbc_mysql(&jdbc, &port, &user);

            let _ = shutdown_tx.send(());
            server.await.unwrap().unwrap();
            fs::remove_file(path).unwrap();
        });
    }

    fn client_handshake_response() -> Vec<u8> {
        client_handshake_response_with_secret(nid(1), "root-pass")
    }

    fn client_handshake_response_with_secret(user: WorkspaceId, secret: &str) -> Vec<u8> {
        let mut payload = Vec::new();
        let capabilities = CLIENT_PROTOCOL_41
            | CLIENT_SECURE_CONNECTION
            | CLIENT_PLUGIN_AUTH
            | CLIENT_CONNECT_WITH_DB;
        payload.extend_from_slice(&capabilities.to_le_bytes());
        payload.extend_from_slice(&0_u32.to_le_bytes());
        payload.push(33);
        payload.extend_from_slice(&[0; 23]);
        payload.extend_from_slice(user.to_string().as_bytes());
        payload.push(0);
        payload.push((secret.len() + 1) as u8);
        payload.extend_from_slice(secret.as_bytes());
        payload.push(0);
        payload.extend_from_slice(b"db\0");
        payload.extend_from_slice(MYSQL_CLEARTEXT_PASSWORD.as_bytes());
        payload.push(0);
        payload
    }

    fn client_native_password_response(user: WorkspaceId, secret: &str, salt: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        let capabilities = CLIENT_PROTOCOL_41
            | CLIENT_SECURE_CONNECTION
            | CLIENT_PLUGIN_AUTH
            | CLIENT_CONNECT_WITH_DB;
        payload.extend_from_slice(&capabilities.to_le_bytes());
        payload.extend_from_slice(&0_u32.to_le_bytes());
        payload.push(33);
        payload.extend_from_slice(&[0; 23]);
        payload.extend_from_slice(user.to_string().as_bytes());
        payload.push(0);
        let scramble = mysql_native_password_scramble(secret.as_bytes(), salt);
        payload.push(scramble.len() as u8);
        payload.extend_from_slice(&scramble);
        payload.extend_from_slice(b"db\0");
        payload.extend_from_slice(MYSQL_NATIVE_PASSWORD.as_bytes());
        payload.push(0);
        payload
    }

    #[cfg(feature = "tls")]
    fn client_ssl_request() -> Vec<u8> {
        let mut payload = Vec::new();
        let capabilities = CLIENT_PROTOCOL_41
            | CLIENT_SSL
            | CLIENT_SECURE_CONNECTION
            | CLIENT_PLUGIN_AUTH
            | CLIENT_CONNECT_WITH_DB;
        payload.extend_from_slice(&capabilities.to_le_bytes());
        payload.extend_from_slice(&0_u32.to_le_bytes());
        payload.push(33);
        payload.extend_from_slice(&[0; 23]);
        payload
    }

    fn mysql_native_password_scramble(password: &[u8], salt: &[u8]) -> [u8; 20] {
        let stage1 = Sha1::digest(password);
        let stage2 = Sha1::digest(stage1);
        let mut hasher = Sha1::new();
        hasher.update(salt);
        hasher.update(stage2);
        let challenge = hasher.finalize();
        let mut out = [0u8; 20];
        for (idx, byte) in out.iter_mut().enumerate() {
            *byte = stage1[idx] ^ challenge[idx];
        }
        out
    }

    fn mysql_handshake_auth(payload: &[u8]) -> (Vec<u8>, String) {
        let server_end = payload[1..].iter().position(|byte| *byte == 0).unwrap() + 1;
        let mut cursor = server_end + 1 + 4;
        let mut salt = payload[cursor..cursor + 8].to_vec();
        cursor += 8 + 1 + 2 + 1 + 2 + 2;
        let auth_len = payload[cursor] as usize;
        cursor += 1 + 10;
        let second_len = auth_len.saturating_sub(9).max(12);
        salt.extend_from_slice(&payload[cursor..cursor + second_len]);
        cursor += second_len + 1;
        let plugin_end = payload[cursor..]
            .iter()
            .position(|byte| *byte == 0)
            .map(|idx| cursor + idx)
            .unwrap_or(payload.len());
        (
            salt,
            String::from_utf8(payload[cursor..plugin_end].to_vec()).unwrap(),
        )
    }

    #[cfg(feature = "tls")]
    fn mysql_handshake_capabilities(payload: &[u8]) -> u32 {
        let server_end = payload[1..].iter().position(|byte| *byte == 0).unwrap() + 1;
        let cursor = server_end + 1 + 4 + 8 + 1;
        let lower = u16::from_le_bytes(payload[cursor..cursor + 2].try_into().unwrap()) as u32;
        let upper = u16::from_le_bytes(payload[cursor + 5..cursor + 7].try_into().unwrap()) as u32;
        lower | (upper << 16)
    }

    #[cfg(feature = "tls")]
    fn mysql_test_tls_connector(cert: Vec<u8>) -> tokio_rustls::TlsConnector {
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

    async fn write_client_query<S>(stream: &mut S, query: &str)
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let mut payload = vec![0x03];
        payload.extend_from_slice(query.as_bytes());
        write_client_packet(stream, 0, &payload).await;
    }

    async fn read_client_packet<S>(stream: &mut S) -> MysqlPacket
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let mut header = [0_u8; 4];
        stream.read_exact(&mut header).await.unwrap();
        let len = header[0] as usize | ((header[1] as usize) << 8) | ((header[2] as usize) << 16);
        let mut payload = vec![0_u8; len];
        stream.read_exact(&mut payload).await.unwrap();
        MysqlPacket { payload }
    }

    async fn read_mysql_text_result<S>(stream: &mut S) -> (Vec<String>, Vec<Vec<String>>)
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let columns = read_client_packet(stream).await;
        let column_count = columns.payload[0] as usize;
        let mut labels = Vec::new();
        for _ in 0..column_count {
            let column = read_client_packet(stream).await;
            labels.push(column_label(&column.payload));
        }
        let _columns_eof = read_client_packet(stream).await;
        let mut rows = Vec::new();
        loop {
            let packet = read_client_packet(stream).await;
            if packet.payload.first() == Some(&0xfe) {
                break;
            }
            rows.push(read_lenenc_strings(&packet.payload, column_count));
        }
        (labels, rows)
    }

    async fn read_mysql_binary_result<S>(stream: &mut S) -> (usize, Vec<Vec<String>>)
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let columns = read_client_packet(stream).await;
        let column_count = columns.payload[0] as usize;
        for _ in 0..column_count {
            let _column = read_client_packet(stream).await;
        }
        let _columns_eof = read_client_packet(stream).await;
        let mut rows = Vec::new();
        loop {
            let packet = read_client_packet(stream).await;
            if packet.payload.first() == Some(&0xfe) {
                break;
            }
            assert_eq!(packet.payload.first(), Some(&0x00));
            let null_bitmap_len = (column_count + 7 + 2) / 8;
            let values_start = 1 + null_bitmap_len;
            rows.push(read_lenenc_strings(
                &packet.payload[values_start..],
                column_count,
            ));
        }
        (column_count, rows)
    }

    fn read_lenenc_strings(payload: &[u8], count: usize) -> Vec<String> {
        let mut cursor = 0;
        let mut out = Vec::new();
        for _ in 0..count {
            if payload.get(cursor) == Some(&0xfb) {
                cursor += 1;
                out.push(String::new());
                continue;
            }
            let (len, used) = read_lenenc_int(&payload[cursor..]).unwrap();
            cursor += used;
            let end = cursor + len as usize;
            out.push(String::from_utf8(payload[cursor..end].to_vec()).unwrap());
            cursor = end;
        }
        out
    }

    fn column_label(payload: &[u8]) -> String {
        let mut cursor = 0;
        let mut values = Vec::new();
        for _ in 0..6 {
            let (len, used) = read_lenenc_int(&payload[cursor..]).unwrap();
            cursor += used;
            let end = cursor + len as usize;
            values.push(String::from_utf8_lossy(&payload[cursor..end]).to_string());
            cursor = end;
        }
        values.get(4).cloned().unwrap_or_default()
    }

    async fn write_client_packet<S>(stream: &mut S, sequence: u8, payload: &[u8])
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let len = payload.len();
        stream.write_all(&[(len & 0xff) as u8]).await.unwrap();
        stream
            .write_all(&[((len >> 8) & 0xff) as u8])
            .await
            .unwrap();
        stream
            .write_all(&[((len >> 16) & 0xff) as u8])
            .await
            .unwrap();
        stream.write_all(&[sequence]).await.unwrap();
        stream.write_all(payload).await.unwrap();
        stream.flush().await.unwrap();
    }

    fn mysql_client_path() -> Option<String> {
        for candidate in ["mysql", "/opt/homebrew/opt/mysql@8.4/bin/mysql"] {
            if Command::new(candidate).arg("--version").output().is_ok() {
                return Some(candidate.to_string());
            }
        }
        None
    }

    fn mysql_cli(mysql: &str, port: &str, user: &str, query: &str) -> String {
        let output = Command::new(mysql)
            .args([
                "--protocol=TCP",
                "--host=127.0.0.1",
                "--port",
                port,
                "--user",
                user,
                "--password=root-pass",
                "--database=db",
                "--enable-cleartext-plugin",
                "--ssl-mode=DISABLED",
                "--batch",
                "--raw",
                "--skip-column-names",
                "--skip-auto-rehash",
                "--execute",
                query,
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "mysql failed: status={:?}\nstdout={}\nstderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn node_mysql2_client() -> Option<String> {
        let node = command_path("node")?;
        let status = Command::new(&node)
            .args(["-e", "require.resolve('mysql2/promise')"])
            .status()
            .ok()?;
        status.success().then_some(node)
    }

    fn node_mysql2(node: &str, port: &str, user: &str) {
        let script = r#"
const mysql = require('mysql2/promise');
(async () => {
  const conn = await mysql.createConnection({
    host: '127.0.0.1',
    port: Number(process.argv[1]),
    user: process.argv[2],
    password: 'root-pass',
    database: 'db',
    ssl: false
  });
  await conn.query('CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)');
  await conn.query("INSERT INTO t VALUES (1, 'a')");
  const [rows] = await conn.query('SELECT id, name FROM t');
  if (String(rows[0].id) !== '1' || rows[0].name !== 'a') {
    throw new Error(JSON.stringify(rows));
  }
  const [tables] = await conn.query('SHOW TABLES');
  if (!Object.values(tables[0]).includes('t')) {
    throw new Error(JSON.stringify(tables));
  }
  const [columns] = await conn.query('DESCRIBE t');
  if (!columns.some((row) => row.Field === 'id')) {
    throw new Error(JSON.stringify(columns));
  }
  await conn.end();
})().catch((err) => {
  console.error(err && err.stack ? err.stack : err);
  process.exit(1);
});
"#;
        let output = Command::new(node)
            .args(["-e", script, port, user])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "node mysql2 failed: status={:?}\nstdout={}\nstderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn python_mysql_client() -> Option<String> {
        let python = command_path("python3")?;
        let status = Command::new(&python)
            .args([
                "-c",
                "import importlib.util as u; raise SystemExit(0 if (u.find_spec('pymysql') or u.find_spec('MySQLdb')) else 1)",
            ])
            .status()
            .ok()?;
        status.success().then_some(python)
    }

    fn python_mysql(python: &str, port: &str, user: &str) {
        let script = r#"
import sys
try:
    import pymysql
    conn = pymysql.connect(
        host='127.0.0.1',
        port=int(sys.argv[1]),
        user=sys.argv[2],
        password='root-pass',
        database='db',
        ssl=None,
    )
except ImportError:
    import MySQLdb
    conn = MySQLdb.connect(
        host='127.0.0.1',
        port=int(sys.argv[1]),
        user=sys.argv[2],
        passwd='root-pass',
        db='db',
        ssl={},
    )
cur = conn.cursor()
cur.execute('CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)')
cur.execute(\"INSERT INTO t VALUES (1, 'a')\")
cur.execute('SELECT id, name FROM t')
rows = cur.fetchall()
assert str(rows[0][0]) == '1' and rows[0][1] == 'a', rows
cur.execute('SHOW TABLES')
tables = cur.fetchall()
assert any(row[0] == 't' for row in tables), tables
cur.execute('DESCRIBE t')
columns = cur.fetchall()
assert any(row[0] == 'id' for row in columns), columns
conn.close()
"#;
        let output = Command::new(python)
            .args(["-c", script, port, user])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "python MySQL client failed: status={:?}\nstdout={}\nstderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    struct JdbcClient {
        java: String,
        javac: String,
        jar: String,
    }

    fn jdbc_mysql_client() -> Option<JdbcClient> {
        let jar = env::var("MYSQL_CONNECTOR_J_JAR").ok()?;
        Some(JdbcClient {
            java: command_path("java")?,
            javac: command_path("javac")?,
            jar,
        })
    }

    fn jdbc_mysql(jdbc: &JdbcClient, port: &str, user: &str) {
        let dir = env::temp_dir().join(format!("loom-mysql-jdbc-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("LoomMysqlTranscript.java");
        fs::write(
            &source,
            r#"
import java.sql.*;

public final class LoomMysqlTranscript {
  public static void main(String[] args) throws Exception {
    String url = "jdbc:mysql://127.0.0.1:" + args[0] + "/db?useSSL=false&allowPublicKeyRetrieval=false";
    try (Connection conn = DriverManager.getConnection(url, args[1], "root-pass")) {
      try (Statement stmt = conn.createStatement()) {
        stmt.executeUpdate("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)");
        stmt.executeUpdate("INSERT INTO t VALUES (1, 'a')");
        try (ResultSet rows = stmt.executeQuery("SELECT id, name FROM t")) {
          if (!rows.next() || rows.getLong(1) != 1L || !"a".equals(rows.getString(2))) {
            throw new IllegalStateException("unexpected select row");
          }
        }
        try (ResultSet tables = stmt.executeQuery("SHOW TABLES")) {
          if (!tables.next() || !"t".equals(tables.getString(1))) {
            throw new IllegalStateException("unexpected table listing");
          }
        }
        try (ResultSet columns = stmt.executeQuery("DESCRIBE t")) {
          boolean found = false;
          while (columns.next()) {
            found |= "id".equals(columns.getString(1));
          }
          if (!found) {
            throw new IllegalStateException("missing id column");
          }
        }
      }
    }
  }
}
"#,
        )
        .unwrap();
        let compile = Command::new(&jdbc.javac)
            .args(["-cp", &jdbc.jar, source.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "javac failed: status={:?}\nstdout={}\nstderr={}",
            compile.status.code(),
            String::from_utf8_lossy(&compile.stdout),
            String::from_utf8_lossy(&compile.stderr)
        );
        let classpath = format!("{}:{}", jdbc.jar, dir.to_string_lossy());
        let output = Command::new(&jdbc.java)
            .args(["-cp", &classpath, "LoomMysqlTranscript", port, user])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Connector/J failed: status={:?}\nstdout={}\nstderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let _ = fs::remove_dir_all(dir);
    }

    fn command_path(command: &str) -> Option<String> {
        let path = env::var_os("PATH")?;
        for dir in env::split_paths(&path) {
            let candidate = dir.join(command);
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
        None
    }

    fn json_field(body: &str, field: &str) -> String {
        let marker = format!("\"{field}\":\"");
        let start = body.find(&marker).unwrap() + marker.len();
        let end = body[start..].find('"').unwrap() + start;
        body[start..end].to_string()
    }
}
