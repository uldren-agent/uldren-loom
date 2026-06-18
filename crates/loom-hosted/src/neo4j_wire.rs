//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use std::collections::BTreeMap;
use std::future::Future;
use std::io;

use loom_core::error::{Code, LoomError};
use loom_core::{GraphQueryValue, GraphReturn, GraphValue, WorkspaceId, graph};
use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::{HostedAuth, HostedKernel};

const BOLT_MAGIC: [u8; 4] = [0x60, 0x60, 0xb0, 0x17];
const BOLT_5_1: u32 = 0x0000_0105;
const BOLT_MAX_CHUNK: usize = 65_535;
const BOLT_MAX_MESSAGE: usize = 1024 * 1024;

const MSG_HELLO: u8 = 0x01;
const MSG_GOODBYE: u8 = 0x02;
const MSG_RESET: u8 = 0x0f;
const MSG_RUN: u8 = 0x10;
const MSG_BEGIN: u8 = 0x11;
const MSG_COMMIT: u8 = 0x12;
const MSG_ROLLBACK: u8 = 0x13;
const MSG_PULL: u8 = 0x3f;
const MSG_LOGON: u8 = 0x6a;
const MSG_LOGOFF: u8 = 0x6b;
const MSG_TELEMETRY: u8 = 0x54;

const MSG_SUCCESS: u8 = 0x70;
const MSG_RECORD: u8 = 0x71;
const MSG_FAILURE: u8 = 0x7f;

const STRUCT_NODE: u8 = 0x4e;
const STRUCT_RELATIONSHIP: u8 = 0x52;
const STRUCT_UNBOUND_RELATIONSHIP: u8 = 0x72;
const STRUCT_PATH: u8 = 0x50;

pub async fn serve_neo4j_tcp<S>(
    listener: TcpListener,
    kernel: HostedKernel,
    workspace: impl Into<String>,
    graph: impl Into<String>,
    shutdown: S,
) -> io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    let state = Neo4jState {
        kernel,
        workspace: workspace.into(),
        graph: graph.into(),
    };
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => return Ok(()),
            incoming = listener.accept() => {
                let (stream, _) = incoming?;
                let connection = Neo4jConnection {
                    stream,
                    state: state.clone(),
                    auth: None,
                    pending: None,
                };
                tokio::spawn(async move {
                    let _ = connection.run().await;
                });
            }
        }
    }
}

#[derive(Clone)]
struct Neo4jState {
    kernel: HostedKernel,
    workspace: String,
    graph: String,
}

struct Neo4jConnection {
    stream: TcpStream,
    state: Neo4jState,
    auth: Option<HostedAuth>,
    pending: Option<PendingResult>,
}

struct PendingResult {
    rows: Vec<Vec<BoltValue>>,
    next: usize,
}

impl Neo4jConnection {
    async fn run(mut self) -> io::Result<()> {
        let Some(version) = self.handshake().await? else {
            return Ok(());
        };
        if version != BOLT_5_1 {
            return Ok(());
        }
        loop {
            let Some(message) = self.read_message().await? else {
                return Ok(());
            };
            match parse_request(&message) {
                Ok(BoltRequest::Hello) => {
                    self.write_success(BTreeMap::from([
                        (
                            "server".to_string(),
                            BoltValue::String("Neo4j/5.1".to_string()),
                        ),
                        (
                            "connection_id".to_string(),
                            BoltValue::String(format!("loom-neo4j:{}", self.state.graph)),
                        ),
                    ]))
                    .await?;
                }
                Ok(BoltRequest::Logon(tokens)) => match neo4j_auth(tokens) {
                    Ok(auth) => match self.state.kernel.read(&auth, |loom| {
                        let ns = loom
                            .registry()
                            .open(&loom_core::WsSelector::Name(self.state.workspace.clone()))?;
                        loom.authorize(ns, loom_core::FacetKind::Graph, loom_core::AclRight::Read)
                    }) {
                        Ok(()) => {
                            self.auth = Some(auth);
                            self.write_success(BTreeMap::new()).await?;
                        }
                        Err(err) => {
                            self.state.kernel.audit_security_failure(&auth, &err);
                            self.write_failure(
                                "Neo.ClientError.Security.Unauthorized",
                                &err.message,
                            )
                            .await?;
                        }
                    },
                    Err(err) => {
                        self.write_failure(
                            "Neo.ClientError.Security.Unauthorized",
                            &err.to_string(),
                        )
                        .await?;
                    }
                },
                Ok(BoltRequest::Logoff) => {
                    self.auth = None;
                    self.pending = None;
                    self.write_success(BTreeMap::new()).await?;
                }
                Ok(BoltRequest::Reset) | Ok(BoltRequest::Telemetry) => {
                    self.pending = None;
                    self.write_success(BTreeMap::new()).await?;
                }
                Ok(BoltRequest::Goodbye) => return Ok(()),
                Ok(BoltRequest::Run { query, parameters }) => {
                    self.run_query(&query, parameters).await?;
                }
                Ok(BoltRequest::Pull(extra)) => {
                    self.pull(extra).await?;
                }
                Ok(BoltRequest::UnsupportedExecution(name)) => {
                    self.write_execution_unsupported(name).await?;
                }
                Err(err) => {
                    self.write_failure("Neo.ClientError.Request.Invalid", &err)
                        .await?;
                }
            }
        }
    }

    async fn run_query(
        &mut self,
        query: &str,
        parameters: BTreeMap<String, BoltValue>,
    ) -> io::Result<()> {
        let Some(auth) = self.auth.clone() else {
            self.write_failure(
                "Neo.ClientError.Security.Unauthorized",
                "Neo4j command requires LOGON",
            )
            .await?;
            return Ok(());
        };
        let substituted = match substitute_parameters(query, &parameters) {
            Ok(query) => query,
            Err(err) => {
                self.write_failure("Neo.ClientError.Statement.SyntaxError", &err.message)
                    .await?;
                return Ok(());
            }
        };
        if neo4j_bounded_write_keyword(&substituted).is_some() {
            self.run_write_query(&auth, &substituted).await?;
            return Ok(());
        }
        let query = match graph::GraphQuery::parse_opencypher(&substituted) {
            Ok(query) => query,
            Err(err) => {
                self.write_failure(code_to_neo4j(err.code), &err.message)
                    .await?;
                return Ok(());
            }
        };
        let fields = graph_result_fields(&query);
        match self.state.kernel.data().graph_query(
            &auth,
            &self.state.workspace,
            &self.state.graph,
            &query,
        ) {
            Ok(result) => {
                let fields = if fields.is_empty() {
                    result
                        .rows
                        .first()
                        .map(|row| row.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default()
                } else {
                    fields
                };
                let rows = result
                    .rows
                    .into_iter()
                    .map(|row| {
                        fields
                            .iter()
                            .map(|field| {
                                row.get(field)
                                    .cloned()
                                    .map(graph_query_value_to_bolt)
                                    .unwrap_or(BoltValue::Null)
                            })
                            .collect()
                    })
                    .collect();
                self.pending = Some(PendingResult { rows, next: 0 });
                self.write_success(BTreeMap::from([(
                    "fields".to_string(),
                    BoltValue::List(fields.into_iter().map(BoltValue::String).collect()),
                )]))
                .await?;
            }
            Err(err) => {
                self.write_failure(code_to_neo4j(err.code), &err.message)
                    .await?;
            }
        }
        Ok(())
    }

    async fn run_write_query(&mut self, auth: &HostedAuth, query: &str) -> io::Result<()> {
        let identity = match graph::GraphMutationIdentity::deterministic_opencypher(query) {
            Ok(identity) => identity,
            Err(err) => {
                self.write_failure(code_to_neo4j(err.code), &err.message)
                    .await?;
                return Ok(());
            }
        };
        let plan = match graph::GraphMutationPlan::parse_opencypher(query, &identity) {
            Ok(plan) => plan,
            Err(err) => {
                self.write_failure(code_to_neo4j(err.code), &err.message)
                    .await?;
                return Ok(());
            }
        };
        match self.state.kernel.data().graph_apply_mutations(
            auth,
            &self.state.workspace,
            &self.state.graph,
            &plan,
        ) {
            Ok(_) => {
                self.pending = Some(PendingResult {
                    rows: Vec::new(),
                    next: 0,
                });
                self.write_success(BTreeMap::from([(
                    "fields".to_string(),
                    BoltValue::List(Vec::new()),
                )]))
                .await?;
            }
            Err(err) => {
                self.write_failure(code_to_neo4j(err.code), &err.message)
                    .await?;
            }
        }
        Ok(())
    }

    async fn pull(&mut self, extra: BTreeMap<String, BoltValue>) -> io::Result<()> {
        if self.auth.is_none() {
            self.write_failure(
                "Neo.ClientError.Security.Unauthorized",
                "Neo4j command requires LOGON",
            )
            .await?;
            return Ok(());
        }
        let Some(mut pending) = self.pending.take() else {
            self.write_failure(
                "Neo.ClientError.Request.Invalid",
                "PULL requires an open auto-commit query stream",
            )
            .await?;
            return Ok(());
        };
        let remaining = pending.rows.len().saturating_sub(pending.next);
        let take = pull_count(&extra).map_or(remaining, |count| count.min(remaining));
        for row in pending.rows[pending.next..pending.next + take]
            .iter()
            .cloned()
        {
            self.write_message(&bolt_struct(MSG_RECORD, vec![BoltValue::List(row)]))
                .await?;
        }
        pending.next += take;
        let has_more = pending.next < pending.rows.len();
        if has_more {
            self.pending = Some(pending);
        }
        self.write_success(BTreeMap::from([(
            "has_more".to_string(),
            BoltValue::Bool(has_more),
        )]))
        .await
    }

    async fn write_execution_unsupported(&mut self, name: &'static str) -> io::Result<()> {
        if self.auth.is_none() {
            self.write_failure(
                "Neo.ClientError.Security.Unauthorized",
                "Neo4j command requires LOGON",
            )
            .await
        } else {
            self.write_failure(
                "Neo.ClientError.Statement.Unsupported",
                &format!("{name} is not implemented by the bounded Loom Neo4j surface"),
            )
            .await
        }
    }

    async fn handshake(&mut self) -> io::Result<Option<u32>> {
        let mut magic = [0u8; 4];
        self.stream.read_exact(&mut magic).await?;
        if magic != BOLT_MAGIC {
            self.stream.write_all(&0u32.to_be_bytes()).await?;
            return Ok(None);
        }
        let mut offers = [0u8; 16];
        self.stream.read_exact(&mut offers).await?;
        let selected = if offers
            .chunks_exact(4)
            .map(|chunk| u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .any(|version| bolt_version_offer_matches(version, BOLT_5_1))
        {
            BOLT_5_1
        } else {
            0
        };
        self.stream.write_all(&selected.to_be_bytes()).await?;
        if selected == 0 {
            return Ok(None);
        }
        Ok(Some(selected))
    }

    async fn read_message(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut message = Vec::new();
        loop {
            let mut header = [0u8; 2];
            match self.stream.read_exact(&mut header).await {
                Ok(_) => {}
                Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(err) => return Err(err),
            }
            let len = u16::from_be_bytes(header) as usize;
            if len == 0 {
                return Ok(Some(message));
            }
            if message.len() + len > BOLT_MAX_MESSAGE {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Bolt message exceeds size limit",
                ));
            }
            let mut chunk = vec![0u8; len];
            self.stream.read_exact(&mut chunk).await?;
            message.extend_from_slice(&chunk);
        }
    }

    async fn write_success(&mut self, metadata: BTreeMap<String, BoltValue>) -> io::Result<()> {
        self.write_message(&bolt_struct(MSG_SUCCESS, vec![BoltValue::Map(metadata)]))
            .await
    }

    async fn write_failure(&mut self, code: &str, message: &str) -> io::Result<()> {
        let metadata = BTreeMap::from([
            ("code".to_string(), BoltValue::String(code.to_string())),
            (
                "message".to_string(),
                BoltValue::String(message.to_string()),
            ),
        ]);
        self.write_message(&bolt_struct(MSG_FAILURE, vec![BoltValue::Map(metadata)]))
            .await
    }

    async fn write_message(&mut self, payload: &[u8]) -> io::Result<()> {
        for chunk in payload.chunks(BOLT_MAX_CHUNK) {
            let len = u16::try_from(chunk.len()).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Bolt chunk exceeds 16-bit length",
                )
            })?;
            self.stream.write_all(&len.to_be_bytes()).await?;
            self.stream.write_all(chunk).await?;
        }
        self.stream.write_all(&0u16.to_be_bytes()).await?;
        self.stream.flush().await
    }
}

fn bolt_version_offer_matches(offer: u32, supported: u32) -> bool {
    if offer == supported {
        return true;
    }
    let [reserved, range, minor, major] = offer.to_be_bytes();
    let [supported_reserved, _, supported_minor, supported_major] = supported.to_be_bytes();
    reserved == supported_reserved
        && major == supported_major
        && supported_minor <= minor
        && minor.saturating_sub(range) <= supported_minor
}

fn neo4j_auth(tokens: BTreeMap<String, BoltValue>) -> loom_core::Result<HostedAuth> {
    let scheme = bolt_string(&tokens, "scheme")
        .ok_or_else(|| loom_core::LoomError::invalid("missing Neo4j auth scheme"))?;
    if scheme != "basic" {
        return Err(loom_core::LoomError::unsupported(format!(
            "unsupported Neo4j auth scheme {scheme:?}"
        )));
    }
    let principal = bolt_string(&tokens, "principal")
        .ok_or_else(|| loom_core::LoomError::invalid("missing Neo4j auth principal"))?;
    let credentials = bolt_string(&tokens, "credentials")
        .ok_or_else(|| loom_core::LoomError::invalid("missing Neo4j auth credentials"))?;
    if credentials.starts_with("loom_app_") {
        return Ok(HostedAuth::app_credential(
            credentials,
            format!("neo4j-bolt-app:{principal}"),
        ));
    }
    Ok(HostedAuth::passphrase(
        WorkspaceId::parse(principal)?,
        credentials,
        format!("neo4j-bolt:{principal}"),
    ))
}

fn bolt_string<'a>(map: &'a BTreeMap<String, BoltValue>, key: &str) -> Option<&'a str> {
    match map.get(key) {
        Some(BoltValue::String(value)) => Some(value),
        _ => None,
    }
}

fn pull_count(extra: &BTreeMap<String, BoltValue>) -> Option<usize> {
    match extra.get("n") {
        Some(BoltValue::Int(value)) if *value >= 0 => usize::try_from(*value).ok(),
        _ => None,
    }
}

fn neo4j_bounded_write_keyword(query: &str) -> Option<&'static str> {
    let keyword = query
        .trim_start()
        .split(|ch: char| !ch.is_ascii_alphabetic())
        .next()?;
    if keyword.eq_ignore_ascii_case("CREATE") {
        Some("CREATE")
    } else if keyword.eq_ignore_ascii_case("MERGE") {
        Some("MERGE")
    } else {
        None
    }
}

fn graph_result_fields(query: &graph::GraphQuery) -> Vec<String> {
    query
        .returns
        .iter()
        .map(|item| match item {
            GraphReturn::Binding(binding) => binding.clone(),
            GraphReturn::Property { binding, property } => format!("{binding}.{property}"),
            GraphReturn::Count { alias, .. } => alias.clone(),
            GraphReturn::PathLength { alias, .. } => alias.clone(),
            GraphReturn::Function { alias, .. } => alias.clone(),
        })
        .collect()
}

fn graph_query_value_to_bolt(value: GraphQueryValue) -> BoltValue {
    match value {
        GraphQueryValue::Null => BoltValue::Null,
        GraphQueryValue::Scalar(value) => graph_value_to_bolt(value),
        GraphQueryValue::Node(node) => neo4j_node(node),
        GraphQueryValue::Edge(edge) => neo4j_relationship(edge),
        GraphQueryValue::Path(path) => neo4j_path(path),
        GraphQueryValue::List(values) => {
            BoltValue::List(values.into_iter().map(graph_query_value_to_bolt).collect())
        }
        GraphQueryValue::Map(values) => BoltValue::Map(
            values
                .into_iter()
                .map(|(key, value)| (key, graph_query_value_to_bolt(value)))
                .collect(),
        ),
    }
}

fn graph_value_to_bolt(value: GraphValue) -> BoltValue {
    match value {
        GraphValue::Null => BoltValue::Null,
        GraphValue::Bool(value) => BoltValue::Bool(value),
        GraphValue::Int(value) => BoltValue::Int(value),
        GraphValue::Float(value) => BoltValue::Float(value),
        GraphValue::Text(value) => BoltValue::String(value),
        GraphValue::Bytes(value) => BoltValue::Bytes(value),
        GraphValue::List(values) => {
            BoltValue::List(values.into_iter().map(graph_value_to_bolt).collect())
        }
        GraphValue::Map(values) => BoltValue::Map(
            values
                .into_iter()
                .map(|(key, value)| (key, graph_value_to_bolt(value)))
                .collect(),
        ),
        GraphValue::Geometry(graph::GraphGeometry::Point(point)) => {
            BoltValue::Map(BTreeMap::from([
                ("type".to_string(), BoltValue::String("point".to_string())),
                (
                    "crs".to_string(),
                    BoltValue::String(point.crs.as_str().to_string()),
                ),
                ("x".to_string(), BoltValue::Float(point.x)),
                ("y".to_string(), BoltValue::Float(point.y)),
                (
                    "z".to_string(),
                    point.z.map(BoltValue::Float).unwrap_or(BoltValue::Null),
                ),
            ]))
        }
    }
}

fn neo4j_node(node: graph::GraphQueryNode) -> BoltValue {
    BoltValue::Struct {
        signature: STRUCT_NODE,
        fields: vec![
            BoltValue::Int(stable_element_id("node", &node.id)),
            BoltValue::List(node.labels.into_iter().map(BoltValue::String).collect()),
            graph_value_to_bolt(GraphValue::Map(node.props)),
            BoltValue::String(node.id),
        ],
    }
}

fn neo4j_relationship(edge: graph::GraphQueryEdge) -> BoltValue {
    BoltValue::Struct {
        signature: STRUCT_RELATIONSHIP,
        fields: vec![
            BoltValue::Int(stable_element_id("edge", &edge.id)),
            BoltValue::Int(stable_element_id("node", &edge.src)),
            BoltValue::Int(stable_element_id("node", &edge.dst)),
            BoltValue::String(edge.label),
            graph_value_to_bolt(GraphValue::Map(edge.props)),
            BoltValue::String(edge.id),
            BoltValue::String(edge.src),
            BoltValue::String(edge.dst),
        ],
    }
}

fn neo4j_path(path: graph::GraphPath) -> BoltValue {
    let node_count = path.nodes.len();
    let edge_count = path.edges.len();
    let nodes = path.nodes.into_iter().map(neo4j_node).collect::<Vec<_>>();
    let rels = path
        .edges
        .into_iter()
        .map(|edge| BoltValue::Struct {
            signature: STRUCT_UNBOUND_RELATIONSHIP,
            fields: vec![
                BoltValue::Int(stable_element_id("edge", &edge.id)),
                BoltValue::String(edge.label),
                graph_value_to_bolt(GraphValue::Map(edge.props)),
                BoltValue::String(edge.id),
            ],
        })
        .collect::<Vec<_>>();
    let mut indices = Vec::new();
    for idx in 0..edge_count {
        indices.push(BoltValue::Int(i64::try_from(idx + 1).unwrap_or(i64::MAX)));
        indices.push(BoltValue::Int(
            i64::try_from((idx + 1).min(node_count.saturating_sub(1))).unwrap_or(i64::MAX),
        ));
    }
    BoltValue::Struct {
        signature: STRUCT_PATH,
        fields: vec![
            BoltValue::List(nodes),
            BoltValue::List(rels),
            BoltValue::List(indices),
        ],
    }
}

fn stable_element_id(kind: &str, id: &str) -> i64 {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update([0]);
    hasher.update(id.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(bytes) & i64::MAX
}

fn substitute_parameters(
    query: &str,
    parameters: &BTreeMap<String, BoltValue>,
) -> loom_core::Result<String> {
    let mut out = String::with_capacity(query.len());
    let mut chars = query.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\'' || ch == '"' {
            out.push(ch);
            while let Some(inner) = chars.next() {
                out.push(inner);
                if inner == '\\' {
                    if let Some(escaped) = chars.next() {
                        out.push(escaped);
                    }
                } else if inner == ch {
                    break;
                }
            }
        } else if ch == '$' {
            let mut name = String::new();
            while let Some(next) = chars.peek().copied() {
                if next == '_' || next.is_ascii_alphanumeric() {
                    name.push(next);
                    let _ = chars.next();
                } else {
                    break;
                }
            }
            if name.is_empty() {
                out.push(ch);
                continue;
            }
            let value = parameters
                .get(&name)
                .ok_or_else(|| LoomError::invalid(format!("missing Cypher parameter ${name}")))?;
            out.push_str(&bolt_value_to_cypher_literal(value)?);
        } else {
            out.push(ch);
        }
    }
    Ok(out)
}

fn bolt_value_to_cypher_literal(value: &BoltValue) -> loom_core::Result<String> {
    match value {
        BoltValue::Null => Ok("null".to_string()),
        BoltValue::Bool(value) => Ok(value.to_string()),
        BoltValue::Int(value) => Ok(value.to_string()),
        BoltValue::Float(value) if value.is_finite() => Ok(value.to_string()),
        BoltValue::Float(_) => Err(LoomError::invalid("Cypher parameter float must be finite")),
        BoltValue::String(value) => Ok(format!("'{}'", cypher_escape(value))),
        BoltValue::List(values) => Ok(format!(
            "[{}]",
            values
                .iter()
                .map(bolt_value_to_cypher_literal)
                .collect::<loom_core::Result<Vec<_>>>()?
                .join(", ")
        )),
        BoltValue::Map(values) => Ok(format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| {
                    Ok(format!("{}: {}", key, bolt_value_to_cypher_literal(value)?))
                })
                .collect::<loom_core::Result<Vec<_>>>()?
                .join(", ")
        )),
        BoltValue::Bytes(_) | BoltValue::Struct { .. } => Err(LoomError::unsupported(
            "Cypher parameter type is not supported by the bounded Neo4j surface",
        )),
    }
}

fn cypher_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            _ => out.push(ch),
        }
    }
    out
}

fn code_to_neo4j(code: Code) -> &'static str {
    match code {
        Code::AuthenticationFailed | Code::PermissionDenied => {
            "Neo.ClientError.Security.Unauthorized"
        }
        Code::InvalidArgument => "Neo.ClientError.Statement.SyntaxError",
        Code::Conflict => "Neo.ClientError.Schema.ConstraintValidationFailed",
        Code::NotFound => "Neo.ClientError.Statement.EntityNotFound",
        Code::Unsupported => "Neo.ClientError.Statement.Unsupported",
        _ => "Neo.DatabaseError.General.UnknownError",
    }
}

enum BoltRequest {
    Hello,
    Logon(BTreeMap<String, BoltValue>),
    Logoff,
    Reset,
    Goodbye,
    Telemetry,
    Run {
        query: String,
        parameters: BTreeMap<String, BoltValue>,
    },
    Pull(BTreeMap<String, BoltValue>),
    UnsupportedExecution(&'static str),
}

fn parse_request(bytes: &[u8]) -> Result<BoltRequest, String> {
    let mut input = BoltInput::new(bytes);
    let (signature, fields) = input.read_struct()?;
    match signature {
        MSG_HELLO => Ok(BoltRequest::Hello),
        MSG_LOGON => {
            let [BoltValue::Map(tokens)] = fields.as_slice() else {
                return Err("LOGON expects one metadata map".to_string());
            };
            Ok(BoltRequest::Logon(tokens.clone()))
        }
        MSG_LOGOFF => Ok(BoltRequest::Logoff),
        MSG_RESET => Ok(BoltRequest::Reset),
        MSG_GOODBYE => Ok(BoltRequest::Goodbye),
        MSG_TELEMETRY => Ok(BoltRequest::Telemetry),
        MSG_RUN => {
            let query = match fields.first() {
                Some(BoltValue::String(query)) => query.clone(),
                _ => return Err("RUN expects query string".to_string()),
            };
            let parameters = match fields.get(1) {
                Some(BoltValue::Map(parameters)) => parameters.clone(),
                Some(_) => return Err("RUN expects parameter map".to_string()),
                None => BTreeMap::new(),
            };
            if !matches!(fields.get(2), None | Some(BoltValue::Map(_))) {
                return Err("RUN expects metadata map".to_string());
            }
            Ok(BoltRequest::Run { query, parameters })
        }
        MSG_PULL => match fields.as_slice() {
            [] => Ok(BoltRequest::Pull(BTreeMap::new())),
            [BoltValue::Map(extra)] => Ok(BoltRequest::Pull(extra.clone())),
            _ => Err("PULL expects metadata map".to_string()),
        },
        MSG_BEGIN => Ok(BoltRequest::UnsupportedExecution("BEGIN")),
        MSG_COMMIT => Ok(BoltRequest::UnsupportedExecution("COMMIT")),
        MSG_ROLLBACK => Ok(BoltRequest::UnsupportedExecution("ROLLBACK")),
        other => Err(format!("unsupported Bolt message signature 0x{other:02x}")),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum BoltValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<BoltValue>),
    Map(BTreeMap<String, BoltValue>),
    Struct {
        signature: u8,
        fields: Vec<BoltValue>,
    },
}

struct BoltInput<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> BoltInput<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn read_struct(&mut self) -> Result<(u8, Vec<BoltValue>), String> {
        let marker = self.read_u8()?;
        if !(0xb0..=0xbf).contains(&marker) {
            return Err("Bolt request must be a tiny struct".to_string());
        }
        let fields = usize::from(marker & 0x0f);
        let signature = self.read_u8()?;
        let mut values = Vec::with_capacity(fields);
        for _ in 0..fields {
            values.push(self.read_value()?);
        }
        Ok((signature, values))
    }

    fn read_value(&mut self) -> Result<BoltValue, String> {
        let marker = self.read_u8()?;
        match marker {
            0xc0 => Ok(BoltValue::Null),
            0xc1 => Ok(BoltValue::Float(f64::from_be_bytes(
                self.read_array::<8>()?,
            ))),
            0xc2 => Ok(BoltValue::Bool(false)),
            0xc3 => Ok(BoltValue::Bool(true)),
            0xc8 => Ok(BoltValue::Int(i64::from(self.read_i8()?))),
            0xc9 => Ok(BoltValue::Int(i64::from(self.read_i16()?))),
            0xca => Ok(BoltValue::Int(i64::from(self.read_i32()?))),
            0xcb => Ok(BoltValue::Int(self.read_i64()?)),
            0xcc => {
                let len = usize::from(self.read_u8()?);
                Ok(BoltValue::Bytes(self.read_bytes(len)?.to_vec()))
            }
            0xcd => {
                let len = usize::from(self.read_u16()?);
                Ok(BoltValue::Bytes(self.read_bytes(len)?.to_vec()))
            }
            0xd0 => {
                let len = usize::from(self.read_u8()?);
                self.read_string(len)
            }
            0xd1 => {
                let len = usize::from(self.read_u16()?);
                self.read_string(len)
            }
            0xd4 => {
                let len = usize::from(self.read_u8()?);
                self.read_list(len)
            }
            0xd5 => {
                let len = usize::from(self.read_u16()?);
                self.read_list(len)
            }
            0xd8 => {
                let len = usize::from(self.read_u8()?);
                self.read_map(len)
            }
            0xd9 => {
                let len = usize::from(self.read_u16()?);
                self.read_map(len)
            }
            0x00..=0x7f => Ok(BoltValue::Int(i64::from(marker))),
            0x80..=0x8f => self.read_string(usize::from(marker & 0x0f)),
            0x90..=0x9f => self.read_list(usize::from(marker & 0x0f)),
            0xa0..=0xaf => self.read_map(usize::from(marker & 0x0f)),
            0xb0..=0xbf => {
                let fields = usize::from(marker & 0x0f);
                let signature = self.read_u8()?;
                let mut values = Vec::with_capacity(fields);
                for _ in 0..fields {
                    values.push(self.read_value()?);
                }
                Ok(BoltValue::Struct {
                    signature,
                    fields: values,
                })
            }
            0xf0..=0xff => Ok(BoltValue::Int(i64::from(marker as i8))),
            _ => Err(format!("unsupported PackStream marker 0x{marker:02x}")),
        }
    }

    fn read_string(&mut self, len: usize) -> Result<BoltValue, String> {
        let bytes = self.read_bytes(len)?;
        let value = std::str::from_utf8(bytes)
            .map_err(|_| "invalid PackStream UTF-8 string".to_string())?;
        Ok(BoltValue::String(value.to_string()))
    }

    fn read_list(&mut self, len: usize) -> Result<BoltValue, String> {
        let mut values = Vec::with_capacity(len);
        for _ in 0..len {
            values.push(self.read_value()?);
        }
        Ok(BoltValue::List(values))
    }

    fn read_map(&mut self, len: usize) -> Result<BoltValue, String> {
        let mut values = BTreeMap::new();
        for _ in 0..len {
            let key = match self.read_value()? {
                BoltValue::String(key) => key,
                _ => return Err("PackStream map key must be a string".to_string()),
            };
            values.insert(key, self.read_value()?);
        }
        Ok(BoltValue::Map(values))
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        let Some(value) = self.bytes.get(self.offset).copied() else {
            return Err("unexpected end of PackStream input".to_string());
        };
        self.offset += 1;
        Ok(value)
    }

    fn read_i8(&mut self) -> Result<i8, String> {
        Ok(self.read_u8()? as i8)
    }

    fn read_u16(&mut self) -> Result<u16, String> {
        let bytes = self.read_array::<2>()?;
        Ok(u16::from_be_bytes(bytes))
    }

    fn read_i16(&mut self) -> Result<i16, String> {
        let bytes = self.read_array::<2>()?;
        Ok(i16::from_be_bytes(bytes))
    }

    fn read_i32(&mut self) -> Result<i32, String> {
        let bytes = self.read_array::<4>()?;
        Ok(i32::from_be_bytes(bytes))
    }

    fn read_i64(&mut self) -> Result<i64, String> {
        let bytes = self.read_array::<8>()?;
        Ok(i64::from_be_bytes(bytes))
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let bytes = self.read_bytes(N)?;
        let mut out = [0u8; N];
        out.copy_from_slice(bytes);
        Ok(out)
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| "PackStream length overflow".to_string())?;
        if end > self.bytes.len() {
            return Err("unexpected end of PackStream input".to_string());
        }
        let bytes = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(bytes)
    }
}

fn bolt_struct(signature: u8, fields: Vec<BoltValue>) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0xb0 | u8::try_from(fields.len()).unwrap_or(0x0f));
    out.push(signature);
    for field in fields {
        encode_value(&mut out, &field);
    }
    out
}

fn encode_value(out: &mut Vec<u8>, value: &BoltValue) {
    match value {
        BoltValue::Null => out.push(0xc0),
        BoltValue::Bool(false) => out.push(0xc2),
        BoltValue::Bool(true) => out.push(0xc3),
        BoltValue::Float(value) => {
            out.push(0xc1);
            out.extend_from_slice(&value.to_be_bytes());
        }
        BoltValue::Int(value) if (0..=127).contains(value) => out.push(*value as u8),
        BoltValue::Int(value) if (-16..=-1).contains(value) => out.push(*value as i8 as u8),
        BoltValue::Int(value) if i8::try_from(*value).is_ok() => {
            out.push(0xc8);
            out.push(*value as i8 as u8);
        }
        BoltValue::Int(value) if i16::try_from(*value).is_ok() => {
            out.push(0xc9);
            out.extend_from_slice(&(*value as i16).to_be_bytes());
        }
        BoltValue::Int(value) if i32::try_from(*value).is_ok() => {
            out.push(0xca);
            out.extend_from_slice(&(*value as i32).to_be_bytes());
        }
        BoltValue::Int(value) => {
            out.push(0xcb);
            out.extend_from_slice(&value.to_be_bytes());
        }
        BoltValue::String(value) => encode_string(out, value),
        BoltValue::Bytes(bytes) => {
            out.push(0xcd);
            out.extend_from_slice(&u16::try_from(bytes.len()).unwrap_or(u16::MAX).to_be_bytes());
            out.extend_from_slice(bytes);
        }
        BoltValue::List(values) => {
            encode_len(out, 0x90, 0xd4, 0xd5, values.len());
            for value in values {
                encode_value(out, value);
            }
        }
        BoltValue::Map(values) => {
            encode_len(out, 0xa0, 0xd8, 0xd9, values.len());
            for (key, value) in values {
                encode_string(out, key);
                encode_value(out, value);
            }
        }
        BoltValue::Struct { signature, fields } => {
            out.push(0xb0 | u8::try_from(fields.len()).unwrap_or(0x0f));
            out.push(*signature);
            for field in fields {
                encode_value(out, field);
            }
        }
    }
}

fn encode_string(out: &mut Vec<u8>, value: &str) {
    let bytes = value.as_bytes();
    encode_len(out, 0x80, 0xd0, 0xd1, bytes.len());
    out.extend_from_slice(bytes);
}

fn encode_len(out: &mut Vec<u8>, tiny_base: u8, marker8: u8, marker16: u8, len: usize) {
    if len < 16 {
        out.push(tiny_base | u8::try_from(len).unwrap_or(0));
    } else if u8::try_from(len).is_ok() {
        out.push(marker8);
        out.push(len as u8);
    } else {
        out.push(marker16);
        out.extend_from_slice(&u16::try_from(len).unwrap_or(u16::MAX).to_be_bytes());
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use super::*;

    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::oneshot;
    use tokio::task::JoinHandle;

    #[test]
    fn bolt_version_offer_accepts_exact_and_range() {
        assert!(bolt_version_offer_matches(0x0000_0105, BOLT_5_1));
        assert!(bolt_version_offer_matches(0x0002_0305, BOLT_5_1));
        assert!(!bolt_version_offer_matches(0x0000_0004, BOLT_5_1));
        assert!(!bolt_version_offer_matches(0x0002_0805, BOLT_5_1));
    }

    #[test]
    fn packstream_logon_parses_basic_auth() {
        let mut auth = BTreeMap::new();
        auth.insert("scheme".into(), BoltValue::String("basic".into()));
        auth.insert("principal".into(), BoltValue::String(nid(1).to_string()));
        auth.insert("credentials".into(), BoltValue::String("secret".into()));
        let bytes = bolt_struct(MSG_LOGON, vec![BoltValue::Map(auth)]);
        match parse_request(&bytes).unwrap() {
            BoltRequest::Logon(tokens) => {
                assert_eq!(bolt_string(&tokens, "scheme"), Some("basic"));
            }
            _ => panic!("expected LOGON"),
        }
    }

    #[test]
    fn neo4j_tcp_transcript_covers_handshake_logon_run_and_pull() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let fixture = Neo4jTestFixture::start("neo4j-bolt-transcript").await;

            let mut client = TcpStream::connect(fixture.addr).await.unwrap();
            client.write_all(&BOLT_MAGIC).await.unwrap();
            client.write_all(&BOLT_5_1.to_be_bytes()).await.unwrap();
            client.write_all(&0u32.to_be_bytes()).await.unwrap();
            client.write_all(&0u32.to_be_bytes()).await.unwrap();
            client.write_all(&0u32.to_be_bytes()).await.unwrap();
            let mut selected = [0u8; 4];
            client.read_exact(&mut selected).await.unwrap();
            assert_eq!(u32::from_be_bytes(selected), BOLT_5_1);

            write_client_message(
                &mut client,
                &bolt_struct(
                    MSG_HELLO,
                    vec![BoltValue::Map(BTreeMap::from([(
                        "user_agent".to_string(),
                        BoltValue::String("loom-test/1".to_string()),
                    )]))],
                ),
            )
            .await;
            assert_eq!(
                read_response_signature(&mut client).await,
                MSG_SUCCESS,
                "HELLO succeeds"
            );

            write_client_message(
                &mut client,
                &bolt_struct(
                    MSG_LOGON,
                    vec![BoltValue::Map(BTreeMap::from([
                        ("scheme".to_string(), BoltValue::String("basic".to_string())),
                        (
                            "principal".to_string(),
                            BoltValue::String(nid(1).to_string()),
                        ),
                        (
                            "credentials".to_string(),
                            BoltValue::String("root-pass".to_string()),
                        ),
                    ]))],
                ),
            )
            .await;
            assert_eq!(
                read_response_signature(&mut client).await,
                MSG_SUCCESS,
                "LOGON succeeds"
            );

            write_client_message(
                &mut client,
                &bolt_struct(
                    MSG_RUN,
                    vec![
                        BoltValue::String("MERGE (k:Person {name: $new_name})".to_string()),
                        BoltValue::Map(BTreeMap::from([(
                            "new_name".to_string(),
                            BoltValue::String("Katherine".to_string()),
                        )])),
                        BoltValue::Map(BTreeMap::new()),
                    ],
                ),
            )
            .await;
            assert_eq!(
                read_response_signature(&mut client).await,
                MSG_SUCCESS,
                "write RUN succeeds"
            );

            write_client_message(
                &mut client,
                &bolt_struct(
                    MSG_PULL,
                    vec![BoltValue::Map(BTreeMap::from([(
                        "n".to_string(),
                        BoltValue::Int(-1),
                    )]))],
                ),
            )
            .await;
            assert_eq!(
                read_response_signature(&mut client).await,
                MSG_SUCCESS,
                "write PULL closes empty stream"
            );

            write_client_message(
                &mut client,
                &bolt_struct(
                    MSG_RUN,
                    vec![
                        BoltValue::String(
                            "MATCH (k:Person) WHERE k.name = $new_name RETURN k.name".to_string(),
                        ),
                        BoltValue::Map(BTreeMap::from([(
                            "new_name".to_string(),
                            BoltValue::String("Katherine".to_string()),
                        )])),
                        BoltValue::Map(BTreeMap::new()),
                    ],
                ),
            )
            .await;
            assert_eq!(
                read_response_signature(&mut client).await,
                MSG_SUCCESS,
                "read-back RUN succeeds"
            );
            write_client_message(
                &mut client,
                &bolt_struct(
                    MSG_PULL,
                    vec![BoltValue::Map(BTreeMap::from([(
                        "n".to_string(),
                        BoltValue::Int(-1),
                    )]))],
                ),
            )
            .await;
            let record = read_response(&mut client).await;
            let (signature, fields) = parse_response(&record);
            assert_eq!(signature, MSG_RECORD, "read-back PULL yields a record");
            assert!(
                format!("{fields:?}").contains("Katherine"),
                "read-back record includes written node: {fields:?}"
            );
            assert_eq!(
                read_response_signature(&mut client).await,
                MSG_SUCCESS,
                "read-back PULL closes stream"
            );

            write_client_message(
                &mut client,
                &bolt_struct(
                    MSG_RUN,
                    vec![
                        BoltValue::String(
                            "MATCH p = (a:Person)-[r:KNOWS]->(b:Person) \
                             WHERE a.name = $name RETURN p, r, a.name, b.name"
                                .to_string(),
                        ),
                        BoltValue::Map(BTreeMap::from([(
                            "name".to_string(),
                            BoltValue::String("Ada".to_string()),
                        )])),
                        BoltValue::Map(BTreeMap::new()),
                    ],
                ),
            )
            .await;
            assert_eq!(
                read_response_signature(&mut client).await,
                MSG_SUCCESS,
                "RUN succeeds"
            );

            write_client_message(
                &mut client,
                &bolt_struct(
                    MSG_PULL,
                    vec![BoltValue::Map(BTreeMap::from([(
                        "n".to_string(),
                        BoltValue::Int(-1),
                    )]))],
                ),
            )
            .await;
            let record = read_response(&mut client).await;
            let (signature, fields) = parse_response(&record);
            assert_eq!(signature, MSG_RECORD, "PULL yields a record");
            let fields = format!("{fields:?}");
            assert!(
                fields.contains("Ada") && fields.contains("Grace"),
                "record includes projected scalar fields: {fields}"
            );
            assert!(
                fields.contains("KNOWS") && fields.contains("ada-knows-grace"),
                "record includes projected relationship and path fields: {fields}"
            );
            let success = read_response(&mut client).await;
            let (signature, fields) = parse_response(&success);
            assert_eq!(signature, MSG_SUCCESS, "PULL closes stream");
            assert!(
                format!("{fields:?}").contains("has_more"),
                "success includes stream metadata: {fields:?}"
            );

            fixture.shutdown().await;
        });
    }

    #[test]
    fn neo4j_python_driver_transcript_covers_bounded_read_write() {
        if !python_driver_available() {
            eprintln!(
                "skipping Neo4j Python driver transcript: python neo4j package is not installed"
            );
            return;
        }
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let fixture = Neo4jTestFixture::start("neo4j-python-driver-transcript").await;
            let script = r#"
from neo4j import GraphDatabase
import sys

uri, principal, password = sys.argv[1], sys.argv[2], sys.argv[3]
driver = GraphDatabase.driver(uri, auth=(principal, password))
try:
    with driver.session() as session:
        session.run("MERGE (p:Person {name: $name})", name="Python Driver").consume()
        record = session.run("MATCH (p:Person) WHERE p.name = $name RETURN p.name", name="Python Driver").single()
        assert record is not None
        assert record[0] == "Python Driver"
        rel = session.run(
            "MATCH p = (a:Person)-[r:KNOWS]->(b:Person) WHERE a.name = $name RETURN p, r, b.name",
            name="Ada",
        ).single()
        assert rel is not None
        assert rel["b.name"] == "Grace"
finally:
    driver.close()
"#;
            run_external_driver(
                python_driver_command()
                    .arg("-c")
                    .arg(script)
                    .arg(format!("bolt://{}", fixture.addr))
                    .arg(nid(1).to_string())
                    .arg("root-pass"),
            );
            fixture.shutdown().await;
        });
    }

    #[test]
    fn neo4j_javascript_driver_transcript_covers_bounded_read_write() {
        if !javascript_driver_available() {
            eprintln!(
                "skipping Neo4j JavaScript driver transcript: neo4j-driver package is not installed"
            );
            return;
        }
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let fixture = Neo4jTestFixture::start("neo4j-javascript-driver-transcript").await;
            let script = r#"
const neo4j = require('neo4j-driver');

async function main() {
  const [uri, principal, password] = process.argv.slice(1);
  const driver = neo4j.driver(uri, neo4j.auth.basic(principal, password));
  try {
    const session = driver.session();
    try {
      await session.run('MERGE (p:Person {name: $name})', {name: 'JavaScript Driver'});
      const writeBack = await session.run(
        'MATCH (p:Person) WHERE p.name = $name RETURN p.name',
        {name: 'JavaScript Driver'}
      );
      if (writeBack.records.length !== 1 || writeBack.records[0].get('p.name') !== 'JavaScript Driver') {
        throw new Error('write-back record mismatch');
      }
      const rel = await session.run(
        'MATCH p = (a:Person)-[r:KNOWS]->(b:Person) WHERE a.name = $name RETURN p, r, b.name',
        {name: 'Ada'}
      );
      if (rel.records.length !== 1 || rel.records[0].get('b.name') !== 'Grace') {
        throw new Error('relationship record mismatch');
      }
    } finally {
      await session.close();
    }
  } finally {
    await driver.close();
  }
}

main().catch((err) => {
  console.error(err && err.stack ? err.stack : err);
  process.exit(1);
});
"#;
            run_external_driver(
                javascript_driver_command()
                    .arg("-e")
                    .arg(script)
                    .arg(format!("bolt://{}", fixture.addr))
                    .arg(nid(1).to_string())
                    .arg("root-pass"),
            );
            fixture.shutdown().await;
        });
    }

    struct Neo4jTestFixture {
        path: PathBuf,
        addr: SocketAddr,
        shutdown_tx: oneshot::Sender<()>,
        server: JoinHandle<io::Result<()>>,
    }

    impl Neo4jTestFixture {
        async fn start(name: &str) -> Self {
            let path = crate::test_support::temp_path(name);
            crate::test_support::init(&path, None);
            loom_coordination::with_local_store_write_lock(&path, || {
                let mut loom = loom_store::open_loom_unlocked(&path, None)?;
                let ns = loom
                    .registry()
                    .open(&loom_core::WsSelector::Name("main".into()))?;
                loom.registry_mut()
                    .add_facet(ns, loom_core::FacetKind::Graph)?;
                loom_store::save_loom(&mut loom)?;
                Ok(())
            })
            .unwrap();

            let auth = HostedAuth::passphrase(nid(1), "root-pass", format!("{name}:seed"));
            let plan = graph::GraphMutationPlan::new(vec![
                graph::GraphMutation::CreateNode {
                    id: "ada".to_string(),
                    labels: std::collections::BTreeSet::from(["Person".to_string()]),
                    props: BTreeMap::from([(
                        "name".to_string(),
                        GraphValue::Text("Ada".to_string()),
                    )]),
                },
                graph::GraphMutation::CreateNode {
                    id: "grace".to_string(),
                    labels: std::collections::BTreeSet::from(["Person".to_string()]),
                    props: BTreeMap::from([(
                        "name".to_string(),
                        GraphValue::Text("Grace".to_string()),
                    )]),
                },
                graph::GraphMutation::CreateEdge {
                    id: "ada-knows-grace".to_string(),
                    src: "ada".to_string(),
                    dst: "grace".to_string(),
                    label: "KNOWS".to_string(),
                    props: BTreeMap::new(),
                },
            ]);
            HostedKernel::new(&path)
                .data()
                .graph_apply_mutations(&auth, "main", "people", &plan)
                .unwrap();

            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let kernel = HostedKernel::new(&path);
            let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
            let server = tokio::spawn(serve_neo4j_tcp(
                listener,
                kernel,
                "main",
                "people",
                async move {
                    let _ = shutdown_rx.await;
                },
            ));
            Self {
                path,
                addr,
                shutdown_tx,
                server,
            }
        }

        async fn shutdown(self) {
            let _ = self.shutdown_tx.send(());
            self.server.await.unwrap().unwrap();
            std::fs::remove_file(self.path).unwrap();
        }
    }

    fn python_driver_available() -> bool {
        python_driver_command()
            .arg("-c")
            .arg("import neo4j")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn javascript_driver_available() -> bool {
        javascript_driver_command()
            .arg("-e")
            .arg("require('neo4j-driver')")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn python_driver_command() -> Command {
        Command::new(std::env::var("LOOM_NEO4J_PYTHON").unwrap_or_else(|_| "python3".to_string()))
    }

    fn javascript_driver_command() -> Command {
        let mut command = Command::new("node");
        if let Ok(node_path) = std::env::var("LOOM_NEO4J_NODE_MODULES") {
            command.env("NODE_PATH", node_path);
        }
        command
    }

    fn run_external_driver(command: &mut Command) {
        let output = command
            .output()
            .expect("external Neo4j driver command runs");
        assert!(
            output.status.success(),
            "external Neo4j driver transcript failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    async fn write_client_message(stream: &mut TcpStream, payload: &[u8]) {
        let len = u16::try_from(payload.len()).unwrap();
        stream.write_all(&len.to_be_bytes()).await.unwrap();
        stream.write_all(payload).await.unwrap();
        stream.write_all(&0u16.to_be_bytes()).await.unwrap();
        stream.flush().await.unwrap();
    }

    async fn read_response_signature(stream: &mut TcpStream) -> u8 {
        let payload = read_response(stream).await;
        let (signature, _) = parse_response(&payload);
        signature
    }

    async fn read_response(stream: &mut TcpStream) -> Vec<u8> {
        let mut payload = Vec::new();
        loop {
            let mut len = [0u8; 2];
            stream.read_exact(&mut len).await.unwrap();
            let len = u16::from_be_bytes(len) as usize;
            if len == 0 {
                break;
            }
            let mut chunk = vec![0u8; len];
            stream.read_exact(&mut chunk).await.unwrap();
            payload.extend_from_slice(&chunk);
        }
        payload
    }

    fn parse_response(payload: &[u8]) -> (u8, Vec<BoltValue>) {
        let mut input = BoltInput::new(payload);
        input.read_struct().unwrap()
    }

    fn nid(byte: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([byte; 16])
    }
}
