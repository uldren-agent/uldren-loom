//! Dataframe logical plans and source bindings.
//!
//! The dataframe facet stores Loom-readable plan state. Execution engines consume these records, but
//! engine-native state is not part of identity.

use crate::AclRight;
use crate::cas::{cas_get, cas_put};
use crate::cbor::{self, Value};
use crate::columnar::{ColumnarSet, get_columnar, put_columnar};
use crate::digest::Digest;
use crate::error::{Code, LoomError, Result};
use crate::provider::ObjectStore;
use crate::tabular::{ColumnType, Value as DataValue, cell_from};
use crate::vcs::Loom;
use crate::workspace::{FacetKind, WorkspaceId, facet_path, facet_root};
pub use loom_dataframe::{
    DataframeAggregation, DataframeBatch, DataframeColumn, DataframeExecutor, DataframeInputFormat,
    DataframeMaterialization, DataframeMaterializationTarget, DataframeOperation, DataframePlan,
    DataframeSchema, DataframeSourceBinding, DataframeSourceKind,
};
use loom_dataframe::{
    dataframe_coerce_rows, dataframe_infer_columns, dataframe_parse_portable_bytes,
    dataframe_plan_digest as canonical_dataframe_plan_digest,
};
use std::collections::{BTreeMap, BTreeSet};

pub fn dataframe_load_source<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    source: &DataframeSourceBinding,
    schema: Option<&DataframeSchema>,
) -> Result<DataframeBatch> {
    match source.kind {
        DataframeSourceKind::Columnar => {
            if source.format != DataframeInputFormat::Native {
                return Err(LoomError::invalid(
                    "columnar dataframe sources use native format",
                ));
            }
            let set = get_columnar(loom, ns, &source.target)?;
            let columns = set
                .columns()
                .iter()
                .map(|(name, column_type)| DataframeColumn::new(name.clone(), *column_type, true))
                .collect();
            DataframeBatch::new(columns, set.scan().cloned().collect())
        }
        DataframeSourceKind::Files => {
            let bytes = loom.read_file(ns, &source.target)?;
            dataframe_parse_bytes(source.format, &bytes, schema, &source.options)
        }
        DataframeSourceKind::Cas => {
            let digest = Digest::parse(&source.target)?;
            let bytes = cas_get(loom, ns, &digest)?
                .ok_or_else(|| LoomError::not_found(format!("cas blob {digest}")))?;
            dataframe_parse_bytes(source.format, &bytes, schema, &source.options)
        }
        DataframeSourceKind::SqlResult => {
            if source.format != DataframeInputFormat::Native {
                return Err(LoomError::invalid(
                    "SQL-result dataframe sources use native format",
                ));
            }
            let digest = Digest::parse(&source.target)?;
            let bytes = cas_get(loom, ns, &digest)?
                .ok_or_else(|| LoomError::not_found(format!("cas blob {digest}")))?;
            dataframe_parse_sql_result(&bytes, schema)
        }
    }
}

pub fn dataframe_load_plan_sources<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    plan: &DataframePlan,
) -> Result<BTreeMap<String, DataframeBatch>> {
    plan.sources
        .iter()
        .map(|source| {
            Ok((
                source.alias.clone(),
                dataframe_load_source(loom, ns, source, plan.schema.as_ref())?,
            ))
        })
        .collect()
}

pub fn dataframe_parse_bytes(
    format: DataframeInputFormat,
    bytes: &[u8],
    schema: Option<&DataframeSchema>,
    options: &BTreeMap<String, String>,
) -> Result<DataframeBatch> {
    match format {
        DataframeInputFormat::Csv | DataframeInputFormat::Json | DataframeInputFormat::Ndjson => {
            dataframe_parse_portable_bytes(format, bytes, schema, options)
        }
        DataframeInputFormat::Native => Err(LoomError::new(
            Code::Unsupported,
            "native dataframe batch bytes are not promoted",
        )),
        DataframeInputFormat::ArrowIpc => parse_arrow_ipc(bytes, schema, options),
        DataframeInputFormat::Parquet => parse_parquet(bytes, schema, options),
    }
}

pub fn dataframe_parse_sql_result(
    bytes: &[u8],
    schema: Option<&DataframeSchema>,
) -> Result<DataframeBatch> {
    match cbor::decode(bytes)? {
        Value::Map(entries) => sql_reader_rows(entries, schema),
        Value::Array(items) => sql_statement_rows(items, schema),
        _ => Err(LoomError::corrupt(
            "dataframe SQL result is neither a reader map nor statement array",
        )),
    }
}

pub fn dataframe_collect<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<DataframeBatch> {
    dataframe_collect_auto(loom, ns, name, default_dataframe_executor())
}

pub fn dataframe_collect_auto<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    exec: Option<&dyn DataframeExecutor>,
) -> Result<DataframeBatch> {
    let plan = get_dataframe_plan(loom, ns, name)?;
    dataframe_execute_auto(loom, ns, &plan, exec)
}

pub fn dataframe_preview<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    rows: u64,
) -> Result<DataframeBatch> {
    dataframe_preview_auto(loom, ns, name, rows, default_dataframe_executor())
}

pub fn dataframe_preview_auto<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    rows: u64,
    exec: Option<&dyn DataframeExecutor>,
) -> Result<DataframeBatch> {
    dataframe_collect_auto(loom, ns, name, exec)?.limit(rows)
}

pub fn dataframe_materialize<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<Option<Digest>> {
    dataframe_materialize_auto(loom, ns, name, default_dataframe_executor())
}

pub fn dataframe_materialize_auto<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    exec: Option<&dyn DataframeExecutor>,
) -> Result<Option<Digest>> {
    let plan = get_dataframe_plan(loom, ns, name)?;
    let Some(materialization) = &plan.materialization else {
        return Err(LoomError::invalid(
            "dataframe plan has no materialization policy",
        ));
    };
    let batch = dataframe_execute_auto(loom, ns, &plan, exec)?;
    match materialization.target {
        DataframeMaterializationTarget::Columnar => {
            let destination = materialization.destination.as_deref().ok_or_else(|| {
                LoomError::invalid("columnar materialization requires destination")
            })?;
            let mut set = ColumnarSet::new(
                batch
                    .columns
                    .iter()
                    .map(|column| (column.name.clone(), column.column_type))
                    .collect(),
                0,
            )?;
            for row in batch.rows {
                set.append_row(row)?;
            }
            let columnar_path = facet_path(FacetKind::Columnar, destination);
            if let Some((parent, _)) = columnar_path.rsplit_once('/') {
                loom.create_directory_reserved(ns, parent, true)?;
            }
            put_columnar(loom, ns, destination, &set)?;
            Ok(None)
        }
        DataframeMaterializationTarget::Files => {
            let destination = materialization
                .destination
                .as_deref()
                .ok_or_else(|| LoomError::invalid("file materialization requires destination"))?;
            let bytes = encode_batch_for_format(&batch, materialization.format)?;
            ensure_public_parent(loom, ns, destination)?;
            loom.write_file(ns, destination, &bytes, 0o100644)?;
            Ok(None)
        }
        DataframeMaterializationTarget::Cas => {
            let bytes = encode_batch_for_format(&batch, materialization.format)?;
            Ok(Some(cas_put(loom, ns, &bytes)?))
        }
        DataframeMaterializationTarget::EphemeralPreview => Ok(None),
    }
}

fn execute_dataframe_plan<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    plan: &DataframePlan,
) -> Result<DataframeBatch> {
    let sources = dataframe_load_plan_sources(loom, ns, plan)?;
    loom_dataframe::execute_loaded_dataframe_plan(plan, &sources)
}

fn dataframe_execute_auto<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    plan: &DataframePlan,
    exec: Option<&dyn DataframeExecutor>,
) -> Result<DataframeBatch> {
    match exec {
        Some(exec) => {
            let sources = dataframe_load_plan_sources(loom, ns, plan)?;
            exec.execute(plan, &sources)
        }
        None => execute_dataframe_plan(loom, ns, plan),
    }
}

fn default_dataframe_executor() -> Option<&'static dyn DataframeExecutor> {
    #[cfg(all(feature = "dataframe-polars", not(target_arch = "wasm32")))]
    {
        Some(&loom_polars::POLARS_DATAFRAME_EXECUTOR)
    }
    #[cfg(not(all(feature = "dataframe-polars", not(target_arch = "wasm32"))))]
    {
        None
    }
}

fn frame_path(name: &str) -> String {
    facet_path(FacetKind::Dataframe, name)
}

pub fn put_dataframe_plan<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    plan: &DataframePlan,
) -> Result<()> {
    plan.validate()?;
    loom.authorize_collection(ns, FacetKind::Dataframe, name, AclRight::Write)?;
    write_dataframe_plan_reserved(loom, ns, name, plan)
}

pub fn get_dataframe_plan<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<DataframePlan> {
    loom.authorize_collection(ns, FacetKind::Dataframe, name, AclRight::Read)?;
    DataframePlan::decode(&loom.read_file_reserved(ns, &frame_path(name))?)
}

pub fn dataframe_create<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    plan: &DataframePlan,
) -> Result<()> {
    plan.validate()?;
    loom.authorize_collection(ns, FacetKind::Dataframe, name, AclRight::Write)?;
    match loom.read_file_reserved(ns, &frame_path(name)) {
        Ok(_) => Err(LoomError::new(
            Code::Conflict,
            format!("dataframe {name:?} already exists"),
        )),
        Err(e) if e.code == Code::NotFound => write_dataframe_plan_reserved(loom, ns, name, plan),
        Err(e) => Err(e),
    }
}

pub fn dataframe_plan_digest<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<Digest> {
    let plan = get_dataframe_plan(loom, ns, name)?;
    Ok(canonical_dataframe_plan_digest(
        &plan,
        loom.store().digest_algo(),
    ))
}

pub fn dataframe_source_digests<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<Vec<Digest>> {
    Ok(get_dataframe_plan(loom, ns, name)?.source_digests())
}

fn ensure_frame_parent<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<()> {
    let path = frame_path(name);
    let root = facet_root(FacetKind::Dataframe);
    let parent = path
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or(root.as_str());
    loom.create_directory_reserved(ns, parent, true)
}

fn write_dataframe_plan_reserved<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    plan: &DataframePlan,
) -> Result<()> {
    ensure_frame_parent(loom, ns, name)?;
    loom.write_file_reserved(ns, &frame_path(name), &plan.encode(), 0o100644)
}

fn ensure_public_parent<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    path: &str,
) -> Result<()> {
    if let Some((parent, _)) = path.rsplit_once('/') {
        loom.create_directory(ns, parent, true)?;
    }
    Ok(())
}

fn json_err(error: serde_json::Error) -> LoomError {
    LoomError::invalid(format!("dataframe JSON parse error: {error}"))
}

fn sql_reader_rows(
    entries: Vec<(Value, Value)>,
    schema: Option<&DataframeSchema>,
) -> Result<DataframeBatch> {
    match sql_text_field(&entries, "kind")?.as_str() {
        "Rows" => {
            let result_columns = sql_columns(sql_field(&entries, "columns")?)?;
            let rows = sql_rows(sql_field(&entries, "rows")?)?;
            let names = result_columns
                .iter()
                .map(|column| column.name.clone())
                .collect::<Vec<_>>();
            let columns = result_columns
                .into_iter()
                .map(|column| {
                    sql_column_type(&column.type_name)
                        .map(|ty| DataframeColumn::new(column.name, ty, true))
                })
                .collect::<Result<Vec<_>>>()?;
            sql_batch_from_rows(names, columns, rows, schema)
        }
        other => Err(LoomError::new(
            Code::Unsupported,
            format!("dataframe SQL reader kind {other:?} is not row-shaped"),
        )),
    }
}

fn sql_statement_rows(
    statements: Vec<Value>,
    schema: Option<&DataframeSchema>,
) -> Result<DataframeBatch> {
    for statement in statements {
        let entries = cbor::as_map(statement)?;
        match sql_text_field(&entries, "kind")?.as_str() {
            "Select" => {
                let names = sql_strings(sql_field(&entries, "labels")?)?;
                let rows = sql_rows(sql_field(&entries, "rows")?)?;
                let columns = dataframe_infer_columns(&names, &rows);
                return sql_batch_from_rows(names, columns, rows, schema);
            }
            "SelectMap" => {
                return sql_select_map_rows(sql_field(&entries, "rows")?, schema);
            }
            "ShowColumns" | "Insert" | "Delete" | "Update" | "DropTable" | "Create"
            | "DropFunction" | "AlterTable" | "CreateIndex" | "DropIndex" | "StartTransaction"
            | "Commit" | "Rollback" | "ShowVariable" => {}
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown SQL result statement kind {other:?}"
                )));
            }
        }
    }
    Err(LoomError::new(
        Code::Unsupported,
        "dataframe SQL result has no row-shaped statement",
    ))
}

fn sql_select_map_rows(value: Value, schema: Option<&DataframeSchema>) -> Result<DataframeBatch> {
    let row_maps = cbor::as_array(value)?
        .into_iter()
        .map(|row| {
            cbor::as_map(row)?
                .into_iter()
                .map(|(key, value)| Ok((cbor::as_text(key)?, cell_from(value)?)))
                .collect::<Result<BTreeMap<_, _>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    let names = schema.map_or_else(
        || {
            row_maps
                .iter()
                .flat_map(|row| row.keys().cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        },
        |schema| {
            schema
                .columns
                .iter()
                .map(|column| column.name.clone())
                .collect()
        },
    );
    let rows = row_maps
        .into_iter()
        .map(|row| {
            names
                .iter()
                .map(|name| row.get(name).cloned().unwrap_or(DataValue::Null))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let columns = schema
        .map(|schema| schema.columns.clone())
        .unwrap_or_else(|| dataframe_infer_columns(&names, &rows));
    sql_batch_from_rows(names, columns, rows, schema)
}

fn sql_batch_from_rows(
    names: Vec<String>,
    columns: Vec<DataframeColumn>,
    rows: Vec<Vec<DataValue>>,
    schema: Option<&DataframeSchema>,
) -> Result<DataframeBatch> {
    if rows.iter().any(|row| row.len() != names.len()) {
        return Err(LoomError::corrupt("SQL result row arity mismatch"));
    }
    let schema = if let Some(schema) = schema {
        if schema.columns.len() != names.len()
            || schema
                .columns
                .iter()
                .zip(&names)
                .any(|(column, name)| column.name != *name)
        {
            return Err(LoomError::invalid(
                "dataframe schema does not match SQL result columns",
            ));
        }
        schema.clone()
    } else {
        DataframeSchema::new(columns, true)?
    };
    DataframeBatch::new(
        schema.columns.clone(),
        dataframe_coerce_rows(rows, Some(&schema))?,
    )
}

fn sql_columns(value: Value) -> Result<Vec<SqlResultColumn>> {
    cbor::as_array(value)?
        .into_iter()
        .map(|column| {
            let entries = cbor::as_map(column)?;
            Ok(SqlResultColumn {
                name: sql_text_field(&entries, "name")?,
                type_name: sql_text_field(&entries, "type")?,
            })
        })
        .collect()
}

fn sql_rows(value: Value) -> Result<Vec<Vec<DataValue>>> {
    cbor::as_array(value)?
        .into_iter()
        .map(|row| {
            cbor::as_array(row)?
                .into_iter()
                .map(cell_from)
                .collect::<Result<Vec<_>>>()
        })
        .collect()
}

fn sql_strings(value: Value) -> Result<Vec<String>> {
    cbor::as_array(value)?
        .into_iter()
        .map(cbor::as_text)
        .collect()
}

fn sql_text_field(entries: &[(Value, Value)], name: &str) -> Result<String> {
    cbor::as_text(sql_field(entries, name)?)
}

fn sql_field(entries: &[(Value, Value)], name: &str) -> Result<Value> {
    entries
        .iter()
        .find_map(|(key, value)| match key {
            Value::Text(key) if key == name => Some(value.clone()),
            _ => None,
        })
        .ok_or_else(|| LoomError::corrupt(format!("SQL result missing field {name:?}")))
}

fn sql_column_type(type_name: &str) -> Result<ColumnType> {
    Ok(match type_name {
        "Int" | "INTEGER" | "BIGINT" | "int" | "integer" | "bigint" => ColumnType::Int,
        "Float" | "DOUBLE" | "REAL" | "FLOAT" | "float" | "double" | "real" => ColumnType::Float,
        "Text" | "TEXT" | "STRING" | "VARCHAR" | "text" | "string" | "varchar" => ColumnType::Text,
        "Bool" | "BOOLEAN" | "bool" | "boolean" => ColumnType::Bool,
        "Bytes" | "BLOB" | "BYTES" | "bytes" | "blob" => ColumnType::Bytes,
        "I8" | "i8" => ColumnType::I8,
        "I16" | "i16" => ColumnType::I16,
        "I32" | "i32" => ColumnType::I32,
        "I128" | "i128" => ColumnType::I128,
        "U8" | "u8" => ColumnType::U8,
        "U16" | "u16" => ColumnType::U16,
        "U32" | "u32" => ColumnType::U32,
        "U64" | "u64" => ColumnType::U64,
        "U128" | "u128" => ColumnType::U128,
        "F32" | "f32" => ColumnType::F32,
        "Decimal" | "DECIMAL" | "decimal" => ColumnType::Decimal,
        "Date" | "DATE" | "date" => ColumnType::Date,
        "Time" | "TIME" | "time" => ColumnType::Time,
        "Timestamp" | "TIMESTAMP" | "timestamp" => ColumnType::Timestamp,
        "Interval" | "INTERVAL" | "interval" => ColumnType::Interval,
        "Uuid" | "UUID" | "uuid" => ColumnType::Uuid,
        "Inet" | "INET" | "inet" => ColumnType::Inet,
        "Point" | "POINT" | "point" => ColumnType::Point,
        "List" | "LIST" | "list" => ColumnType::List,
        "Map" | "MAP" | "map" => ColumnType::Map,
        other => {
            return Err(LoomError::corrupt(format!(
                "unknown SQL result column type {other:?}"
            )));
        }
    })
}

struct SqlResultColumn {
    name: String,
    type_name: String,
}

fn encode_batch_for_format(
    batch: &DataframeBatch,
    format: DataframeInputFormat,
) -> Result<Vec<u8>> {
    match format {
        DataframeInputFormat::Csv => Ok(encode_csv(batch).into_bytes()),
        DataframeInputFormat::Json => encode_json(batch, false),
        DataframeInputFormat::Ndjson => encode_json(batch, true),
        DataframeInputFormat::ArrowIpc => encode_arrow_ipc(batch),
        DataframeInputFormat::Parquet => encode_parquet(batch),
        DataframeInputFormat::Native => Err(LoomError::new(
            Code::Unsupported,
            "dataframe export format is not promoted",
        )),
    }
}

#[cfg(feature = "columnar-arrow")]
fn parse_arrow_ipc(
    bytes: &[u8],
    schema: Option<&DataframeSchema>,
    options: &BTreeMap<String, String>,
) -> Result<DataframeBatch> {
    apply_schema(
        columnar_to_batch(&crate::columnar_arrow::columnar_from_arrow_ipc(
            bytes,
            target_segment_rows(options)?,
        )?)?,
        schema,
    )
}

#[cfg(not(feature = "columnar-arrow"))]
fn parse_arrow_ipc(
    _bytes: &[u8],
    _schema: Option<&DataframeSchema>,
    _options: &BTreeMap<String, String>,
) -> Result<DataframeBatch> {
    Err(LoomError::new(
        Code::Unsupported,
        "Arrow IPC dataframe adapters require the columnar-arrow feature",
    ))
}

#[cfg(feature = "columnar-arrow")]
fn parse_parquet(
    bytes: &[u8],
    schema: Option<&DataframeSchema>,
    options: &BTreeMap<String, String>,
) -> Result<DataframeBatch> {
    apply_schema(
        columnar_to_batch(&crate::columnar_arrow::columnar_from_parquet(
            bytes,
            target_segment_rows(options)?,
        )?)?,
        schema,
    )
}

#[cfg(not(feature = "columnar-arrow"))]
fn parse_parquet(
    _bytes: &[u8],
    _schema: Option<&DataframeSchema>,
    _options: &BTreeMap<String, String>,
) -> Result<DataframeBatch> {
    Err(LoomError::new(
        Code::Unsupported,
        "Parquet dataframe adapters require the columnar-arrow feature",
    ))
}

#[cfg(feature = "columnar-arrow")]
fn apply_schema(batch: DataframeBatch, schema: Option<&DataframeSchema>) -> Result<DataframeBatch> {
    let Some(schema) = schema else {
        return Ok(batch);
    };
    if schema.columns.len() != batch.columns.len()
        || schema
            .columns
            .iter()
            .zip(&batch.columns)
            .any(|(expected, actual)| expected.name != actual.name)
    {
        return Err(LoomError::invalid(
            "dataframe schema does not match decoded batch columns",
        ));
    }
    DataframeBatch::new(
        schema.columns.clone(),
        dataframe_coerce_rows(batch.rows, Some(schema))?,
    )
}

#[cfg(feature = "columnar-arrow")]
fn encode_arrow_ipc(batch: &DataframeBatch) -> Result<Vec<u8>> {
    crate::columnar_arrow::columnar_to_arrow_ipc(&batch_to_columnar(batch)?)
}

#[cfg(not(feature = "columnar-arrow"))]
fn encode_arrow_ipc(_batch: &DataframeBatch) -> Result<Vec<u8>> {
    Err(LoomError::new(
        Code::Unsupported,
        "Arrow IPC dataframe export requires the columnar-arrow feature",
    ))
}

#[cfg(feature = "columnar-arrow")]
fn encode_parquet(batch: &DataframeBatch) -> Result<Vec<u8>> {
    crate::columnar_arrow::columnar_to_parquet(&batch_to_columnar(batch)?)
}

#[cfg(not(feature = "columnar-arrow"))]
fn encode_parquet(_batch: &DataframeBatch) -> Result<Vec<u8>> {
    Err(LoomError::new(
        Code::Unsupported,
        "Parquet dataframe export requires the columnar-arrow feature",
    ))
}

#[cfg(feature = "columnar-arrow")]
fn target_segment_rows(options: &BTreeMap<String, String>) -> Result<usize> {
    options
        .get("target_segment_rows")
        .map(|value| {
            value.parse::<usize>().map_err(|e| {
                LoomError::invalid(format!("invalid dataframe target_segment_rows option: {e}"))
            })
        })
        .unwrap_or(Ok(0))
}

#[cfg(feature = "columnar-arrow")]
fn columnar_to_batch(dataset: &ColumnarSet) -> Result<DataframeBatch> {
    DataframeBatch::new(
        dataset
            .columns()
            .iter()
            .map(|(name, column_type)| DataframeColumn::new(name.clone(), *column_type, true))
            .collect(),
        dataset.scan().cloned().collect(),
    )
}

#[cfg(feature = "columnar-arrow")]
fn batch_to_columnar(batch: &DataframeBatch) -> Result<ColumnarSet> {
    let mut dataset = ColumnarSet::new(
        batch
            .columns
            .iter()
            .map(|column| (column.name.clone(), column.column_type))
            .collect(),
        0,
    )?;
    for row in &batch.rows {
        dataset.append_row(row.clone())?;
    }
    Ok(dataset)
}

fn encode_csv(batch: &DataframeBatch) -> String {
    let mut out = String::new();
    out.push_str(
        &batch
            .columns
            .iter()
            .map(|column| csv_escape(&column.name))
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push('\n');
    for row in &batch.rows {
        out.push_str(
            &row.iter()
                .map(|value| csv_escape(&data_value_text(value)))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push('\n');
    }
    out
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn encode_json(batch: &DataframeBatch, ndjson: bool) -> Result<Vec<u8>> {
    let rows = batch
        .rows
        .iter()
        .map(|row| {
            let mut object = serde_json::Map::new();
            for (column, value) in batch.columns.iter().zip(row) {
                object.insert(column.name.clone(), data_value_json(value));
            }
            serde_json::Value::Object(object)
        })
        .collect::<Vec<_>>();
    if ndjson {
        let mut out = String::new();
        for row in rows {
            out.push_str(&serde_json::to_string(&row).map_err(json_err)?);
            out.push('\n');
        }
        Ok(out.into_bytes())
    } else {
        serde_json::to_vec(&rows).map_err(json_err)
    }
}

fn data_value_text(value: &DataValue) -> String {
    match value {
        DataValue::Null => String::new(),
        DataValue::Bool(value) => value.to_string(),
        DataValue::Int(value) => value.to_string(),
        DataValue::Float(value) => value.to_string(),
        DataValue::Text(value) => value.clone(),
        DataValue::U64(value) => value.to_string(),
        other => format!("{other:?}"),
    }
}

fn data_value_json(value: &DataValue) -> serde_json::Value {
    match value {
        DataValue::Null => serde_json::Value::Null,
        DataValue::Bool(value) => serde_json::Value::Bool(*value),
        DataValue::Int(value) => serde_json::Value::Number((*value).into()),
        DataValue::Text(value) => serde_json::Value::String(value.clone()),
        DataValue::U64(value) => serde_json::Value::Number((*value).into()),
        DataValue::Float(value) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        DataValue::List(values) => {
            serde_json::Value::Array(values.iter().map(data_value_json).collect())
        }
        DataValue::Map(values) => serde_json::Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), data_value_json(value)))
                .collect(),
        ),
        other => serde_json::Value::String(format!("{other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::memory::MemoryStore;
    use crate::workspace::WorkspaceId;

    fn source() -> DataframeSourceBinding {
        DataframeSourceBinding::new(
            "events",
            DataframeSourceKind::Files,
            "/inputs/events.csv",
            DataframeInputFormat::Csv,
        )
        .with_option("has_header", "true")
    }

    fn plan() -> DataframePlan {
        DataframePlan::new(vec![source()])
            .unwrap()
            .with_schema(
                DataframeSchema::new(
                    vec![
                        DataframeColumn::new("id", ColumnType::U64, false),
                        DataframeColumn::new("kind", ColumnType::Text, true),
                    ],
                    true,
                )
                .unwrap(),
            )
            .unwrap()
            .with_operations(vec![
                DataframeOperation::Scan {
                    source: "events".into(),
                },
                DataframeOperation::Filter {
                    expression: "kind == \"purchase\"".into(),
                },
                DataframeOperation::Aggregate {
                    group_by: vec!["kind".into()],
                    aggregations: vec![DataframeAggregation::new("count", "count", None)],
                },
            ])
            .unwrap()
            .with_materialization(DataframeMaterialization::new(
                DataframeMaterializationTarget::Columnar,
                Some("analytics/purchases".into()),
                DataframeInputFormat::Parquet,
            ))
            .unwrap()
    }

    #[test]
    fn plan_encode_round_trips() {
        let plan = plan();
        let decoded = DataframePlan::decode(&plan.encode()).unwrap();
        assert_eq!(decoded, plan);
        assert_eq!(decoded.sources[0].format.as_str(), "csv");
    }

    #[test]
    fn plan_validation_rejects_unknown_sources_and_duplicate_columns() {
        assert!(
            DataframePlan::new(vec![source()])
                .unwrap()
                .with_operations(vec![DataframeOperation::Scan {
                    source: "missing".into(),
                }])
                .is_err()
        );
        assert!(
            DataframeSchema::new(
                vec![
                    DataframeColumn::new("id", ColumnType::U64, false),
                    DataframeColumn::new("id", ColumnType::Text, true),
                ],
                false,
            )
            .is_err()
        );
    }

    #[test]
    fn facade_create_put_get_and_digest() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Dataframe,
                None,
                WorkspaceId::from_bytes([33; 16]),
            )
            .unwrap();
        let plan = plan();
        dataframe_create(&mut loom, ns, "etl/purchases", &plan).unwrap();
        assert_eq!(
            dataframe_create(&mut loom, ns, "etl/purchases", &plan)
                .unwrap_err()
                .code,
            Code::Conflict
        );
        assert_eq!(
            get_dataframe_plan(&loom, ns, "etl/purchases").unwrap(),
            plan
        );
        let digest = dataframe_plan_digest(&loom, ns, "etl/purchases").unwrap();
        put_dataframe_plan(
            &mut loom,
            ns,
            "etl/purchases",
            &DataframePlan::new(vec![source()])
                .unwrap()
                .with_operations(vec![DataframeOperation::Limit { rows: 1 }])
                .unwrap(),
        )
        .unwrap();
        assert_ne!(
            digest,
            dataframe_plan_digest(&loom, ns, "etl/purchases").unwrap()
        );
    }

    #[test]
    fn dataframe_operations_honor_collection_scopes() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Dataframe,
                None,
                WorkspaceId::from_bytes([34; 16]),
            )
            .unwrap();
        let root = WorkspaceId::from_bytes([1; 16]);
        let mut identity = crate::IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);
        loom.acl_store_mut()
            .grant(crate::AclGrant {
                subject: crate::AclSubject::Principal(root),
                workspace: Some(ns),
                domain: Some(FacetKind::Dataframe.into()),
                ref_glob: None,
                scopes: vec![crate::AclScope::Prefix {
                    kind: crate::AclScopeKind::Collection,
                    prefix: b"etl/".to_vec(),
                }],
                rights: [crate::AclRight::Write, crate::AclRight::Read]
                    .into_iter()
                    .collect(),
                effect: crate::AclEffect::Allow,
                predicate: None,
            })
            .unwrap();

        dataframe_create(&mut loom, ns, "etl/purchases", &plan()).unwrap();
        assert_eq!(
            dataframe_create(&mut loom, ns, "private/purchases", &plan())
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
    }

    #[test]
    fn csv_source_executes_and_materializes_to_columnar() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Dataframe,
                None,
                WorkspaceId::from_bytes([35; 16]),
            )
            .unwrap();
        loom.registry_mut().add_facet(ns, FacetKind::Files).unwrap();
        loom.registry_mut()
            .add_facet(ns, FacetKind::Columnar)
            .unwrap();
        loom.create_directory(ns, "inputs", true).unwrap();
        loom.write_file(
            ns,
            "inputs/events.csv",
            b"id,kind,total\n1,purchase,10.5\n2,view,0\n3,purchase,7.5\n",
            0o100644,
        )
        .unwrap();

        let plan = DataframePlan::new(vec![DataframeSourceBinding::new(
            "events",
            DataframeSourceKind::Files,
            "inputs/events.csv",
            DataframeInputFormat::Csv,
        )])
        .unwrap()
        .with_operations(vec![
            DataframeOperation::Scan {
                source: "events".into(),
            },
            DataframeOperation::Filter {
                expression: "kind == \"purchase\"".into(),
            },
            DataframeOperation::Select {
                columns: vec!["id".into(), "total".into()],
            },
            DataframeOperation::Sort {
                columns: vec!["id".into()],
                descending: true,
            },
        ])
        .unwrap()
        .with_materialization(DataframeMaterialization::new(
            DataframeMaterializationTarget::Columnar,
            Some("analytics/purchases".into()),
            DataframeInputFormat::Parquet,
        ))
        .unwrap();
        dataframe_create(&mut loom, ns, "etl/purchases", &plan).unwrap();

        let preview = dataframe_preview(&loom, ns, "etl/purchases", 1).unwrap();
        assert_eq!(preview.row_count(), 1);
        assert_eq!(preview.rows[0][0], DataValue::Int(3));
        dataframe_materialize(&mut loom, ns, "etl/purchases").unwrap();
        let materialized = get_columnar(&loom, ns, "analytics/purchases").unwrap();
        assert_eq!(materialized.rows(), 2);
        assert_eq!(
            materialized.scan().next().unwrap()[1],
            DataValue::Float(7.5)
        );
    }

    #[test]
    fn ndjson_source_executes_aggregate_and_exports_to_cas() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Dataframe,
                None,
                WorkspaceId::from_bytes([36; 16]),
            )
            .unwrap();
        loom.registry_mut().add_facet(ns, FacetKind::Cas).unwrap();
        let digest = cas_put(
            &mut loom,
            ns,
            br#"{"tenant":"a","score":2}
{"tenant":"a","score":3}
{"tenant":"b","score":5}
"#,
        )
        .unwrap();
        let plan = DataframePlan::new(vec![DataframeSourceBinding::new(
            "scores",
            DataframeSourceKind::Cas,
            digest.to_string(),
            DataframeInputFormat::Ndjson,
        )])
        .unwrap()
        .with_operations(vec![
            DataframeOperation::Scan {
                source: "scores".into(),
            },
            DataframeOperation::Aggregate {
                group_by: vec!["tenant".into()],
                aggregations: vec![
                    DataframeAggregation::new("rows", "count", None),
                    DataframeAggregation::new("score_sum", "sum", Some("score".into())),
                ],
            },
        ])
        .unwrap()
        .with_materialization(DataframeMaterialization::new(
            DataframeMaterializationTarget::Cas,
            None,
            DataframeInputFormat::Ndjson,
        ))
        .unwrap();
        dataframe_create(&mut loom, ns, "etl/scores", &plan).unwrap();

        let batch = dataframe_collect(&loom, ns, "etl/scores").unwrap();
        assert_eq!(batch.row_count(), 2);
        assert_eq!(batch.rows[0][0], DataValue::Text("a".into()));
        assert_eq!(batch.rows[0][1], DataValue::U64(2));
        assert_eq!(batch.rows[0][2], DataValue::Float(5.0));
        let exported = dataframe_materialize(&mut loom, ns, "etl/scores")
            .unwrap()
            .unwrap();
        let bytes = cas_get(&loom, ns, &exported).unwrap().unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("\"tenant\":\"a\""));
        assert!(text.contains("\"score_sum\":5.0"));
    }

    #[test]
    fn sql_result_reader_rows_load_from_cas_source() {
        let bytes = cbor::encode(&Value::Map(vec![
            (Value::Text("kind".into()), Value::Text("Rows".into())),
            (
                Value::Text("columns".into()),
                Value::Array(vec![
                    Value::Map(vec![
                        (Value::Text("name".into()), Value::Text("id".into())),
                        (Value::Text("type".into()), Value::Text("Int".into())),
                    ]),
                    Value::Map(vec![
                        (Value::Text("name".into()), Value::Text("name".into())),
                        (Value::Text("type".into()), Value::Text("Text".into())),
                    ]),
                ]),
            ),
            (
                Value::Text("rows".into()),
                Value::Array(vec![Value::Array(vec![
                    crate::tabular::cell_value(&DataValue::Int(7)),
                    crate::tabular::cell_value(&DataValue::Text("ada".into())),
                ])]),
            ),
        ]));
        let schema = DataframeSchema::new(
            vec![
                DataframeColumn::new("id", ColumnType::Int, false),
                DataframeColumn::new("name", ColumnType::Text, true),
            ],
            false,
        )
        .unwrap();
        let parsed = dataframe_parse_sql_result(&bytes, Some(&schema)).unwrap();
        assert_eq!(parsed.columns, schema.columns);
        assert_eq!(
            parsed.rows,
            vec![vec![DataValue::Int(7), DataValue::Text("ada".into())]]
        );

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Dataframe,
                None,
                WorkspaceId::from_bytes([38; 16]),
            )
            .unwrap();
        loom.registry_mut().add_facet(ns, FacetKind::Cas).unwrap();
        let digest = cas_put(&mut loom, ns, &bytes).unwrap();
        let plan = DataframePlan::new(vec![DataframeSourceBinding::new(
            "sql",
            DataframeSourceKind::SqlResult,
            digest.to_string(),
            DataframeInputFormat::Native,
        )])
        .unwrap()
        .with_schema(schema)
        .unwrap()
        .with_operations(vec![DataframeOperation::Scan {
            source: "sql".into(),
        }])
        .unwrap();
        dataframe_create(&mut loom, ns, "etl/sql", &plan).unwrap();

        let collected = dataframe_collect(&loom, ns, "etl/sql").unwrap();
        assert_eq!(collected.row_count(), 1);
        assert_eq!(collected.rows[0][0], DataValue::Int(7));
        assert_eq!(collected.rows[0][1], DataValue::Text("ada".into()));
    }

    #[test]
    fn sql_result_statement_select_and_select_map_are_row_shaped() {
        let select = cbor::encode(&Value::Array(vec![Value::Map(vec![
            (Value::Text("kind".into()), Value::Text("Select".into())),
            (
                Value::Text("labels".into()),
                Value::Array(vec![Value::Text("id".into()), Value::Text("ok".into())]),
            ),
            (
                Value::Text("rows".into()),
                Value::Array(vec![Value::Array(vec![
                    crate::tabular::cell_value(&DataValue::Int(1)),
                    crate::tabular::cell_value(&DataValue::Bool(true)),
                ])]),
            ),
        ])]));
        let select_batch = dataframe_parse_sql_result(&select, None).unwrap();
        assert_eq!(
            select_batch.columns,
            vec![
                DataframeColumn::new("id", ColumnType::Int, true),
                DataframeColumn::new("ok", ColumnType::Bool, true),
            ]
        );
        assert_eq!(
            select_batch.rows,
            vec![vec![DataValue::Int(1), DataValue::Bool(true)]]
        );

        let select_map = cbor::encode(&Value::Array(vec![Value::Map(vec![
            (Value::Text("kind".into()), Value::Text("SelectMap".into())),
            (
                Value::Text("rows".into()),
                Value::Array(vec![Value::Map(vec![
                    (
                        Value::Text("name".into()),
                        crate::tabular::cell_value(&DataValue::Text("grace".into())),
                    ),
                    (
                        Value::Text("score".into()),
                        crate::tabular::cell_value(&DataValue::Float(9.5)),
                    ),
                ])]),
            ),
        ])]));
        let map_batch = dataframe_parse_sql_result(&select_map, None).unwrap();
        assert_eq!(
            map_batch.columns,
            vec![
                DataframeColumn::new("name", ColumnType::Text, true),
                DataframeColumn::new("score", ColumnType::Float, true),
            ]
        );
        assert_eq!(
            map_batch.rows,
            vec![vec![DataValue::Text("grace".into()), DataValue::Float(9.5)]]
        );
    }

    #[cfg(feature = "columnar-arrow")]
    #[test]
    fn arrow_and_parquet_dataframe_batches_round_trip_through_columnar_profile() {
        let batch = DataframeBatch::new(
            vec![
                DataframeColumn::new("id", ColumnType::Int, false),
                DataframeColumn::new("kind", ColumnType::Text, true),
                DataframeColumn::new("score", ColumnType::Float, true),
            ],
            vec![
                vec![
                    DataValue::Int(1),
                    DataValue::Text("purchase".into()),
                    DataValue::Float(10.5),
                ],
                vec![DataValue::Int(2), DataValue::Null, DataValue::Float(0.0)],
            ],
        )
        .unwrap();
        let options = BTreeMap::from([("target_segment_rows".to_string(), "1".to_string())]);
        let schema = batch.schema(false).unwrap();

        let arrow = encode_batch_for_format(&batch, DataframeInputFormat::ArrowIpc).unwrap();
        let arrow_batch = dataframe_parse_bytes(
            DataframeInputFormat::ArrowIpc,
            &arrow,
            Some(&schema),
            &options,
        )
        .unwrap();
        assert_eq!(arrow_batch.columns, batch.columns);
        assert_eq!(arrow_batch.rows, batch.rows);

        let parquet = encode_batch_for_format(&batch, DataframeInputFormat::Parquet).unwrap();
        let parquet_batch = dataframe_parse_bytes(
            DataframeInputFormat::Parquet,
            &parquet,
            Some(&schema),
            &options,
        )
        .unwrap();
        assert_eq!(parquet_batch.columns, batch.columns);
        assert_eq!(parquet_batch.rows, batch.rows);
    }

    struct FirstSourceExecutor;

    impl DataframeExecutor for FirstSourceExecutor {
        fn execute(
            &self,
            plan: &DataframePlan,
            sources: &BTreeMap<String, DataframeBatch>,
        ) -> Result<DataframeBatch> {
            let source = plan
                .sources
                .first()
                .ok_or_else(|| LoomError::invalid("missing source"))?;
            sources
                .get(&source.alias)
                .cloned()
                .ok_or_else(|| LoomError::invalid("source not loaded"))
        }
    }

    #[test]
    fn collect_auto_uses_injected_executor_over_loom_loaded_sources() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Dataframe,
                None,
                WorkspaceId::from_bytes([37; 16]),
            )
            .unwrap();
        loom.registry_mut().add_facet(ns, FacetKind::Files).unwrap();
        loom.create_directory(ns, "inputs", true).unwrap();
        loom.write_file(ns, "inputs/simple.csv", b"id\n1\n2\n", 0o100644)
            .unwrap();
        let plan = DataframePlan::new(vec![DataframeSourceBinding::new(
            "simple",
            DataframeSourceKind::Files,
            "inputs/simple.csv",
            DataframeInputFormat::Csv,
        )])
        .unwrap()
        .with_operations(vec![DataframeOperation::Scan {
            source: "simple".into(),
        }])
        .unwrap();
        dataframe_create(&mut loom, ns, "etl/simple", &plan).unwrap();

        let native =
            dataframe_collect_auto(&loom, ns, "etl/simple", Some(&FirstSourceExecutor)).unwrap();
        assert_eq!(native.row_count(), 2);
        assert_eq!(native.rows[1][0], DataValue::Int(2));
    }
}
