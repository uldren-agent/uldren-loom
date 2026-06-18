//! Dataframe logical plans and source bindings.
//!
//! The dataframe facet stores Loom-readable plan state. Execution engines consume these records, but
//! engine-native state is not part of identity.

use loom_codec::Value;
use loom_types::{
    Algo, Code, ColumnType, Digest, LoomError, Result, Value as DataValue, cell_from, cell_value,
};
use std::collections::{BTreeMap, BTreeSet};

mod cbor {
    use loom_codec::Value;
    use loom_types::{Digest, LoomError, Result};

    pub fn encode(value: &Value) -> Vec<u8> {
        loom_codec::encode(value).expect("dataframe plan values are canonical")
    }

    pub fn decode_array(bytes: &[u8]) -> Result<Vec<Value>> {
        as_array(decode(bytes)?)
    }

    pub fn decode(bytes: &[u8]) -> Result<Value> {
        loom_codec::decode(bytes).map_err(err)
    }

    pub fn err(e: loom_codec::CodecError) -> LoomError {
        LoomError::corrupt(format!("cbor: {e}"))
    }

    pub fn digest_value(d: &Digest) -> Value {
        Value::Bytes(d.bytes().to_vec())
    }

    pub fn as_uint(v: Value) -> Result<u64> {
        match v {
            Value::Uint(n) => Ok(n),
            _ => Err(LoomError::corrupt("expected a uint")),
        }
    }

    pub fn as_text(v: Value) -> Result<String> {
        match v {
            Value::Text(s) => Ok(s),
            _ => Err(LoomError::corrupt("expected a text string")),
        }
    }

    pub fn as_array(v: Value) -> Result<Vec<Value>> {
        match v {
            Value::Array(a) => Ok(a),
            _ => Err(LoomError::corrupt("expected an array")),
        }
    }

    pub fn as_digest(v: Value) -> Result<Digest> {
        let bytes = match v {
            Value::Bytes(b) => b,
            _ => return Err(LoomError::corrupt("expected a digest byte string")),
        };
        let arr: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| LoomError::corrupt("digest field is not 32 bytes"))?;
        Ok(Digest::from_blake3_bytes(arr))
    }

    pub fn u8_from(n: u64) -> Result<u8> {
        u8::try_from(n).map_err(|_| LoomError::corrupt("value out of u8 range"))
    }

    pub struct Fields {
        items: std::vec::IntoIter<Value>,
    }

    impl Fields {
        pub fn new(items: Vec<Value>) -> Self {
            Self {
                items: items.into_iter(),
            }
        }

        pub fn next_field(&mut self) -> Result<Value> {
            self.items
                .next()
                .ok_or_else(|| LoomError::corrupt("missing field"))
        }

        pub fn uint(&mut self) -> Result<u64> {
            as_uint(self.next_field()?)
        }

        pub fn bool(&mut self) -> Result<bool> {
            match self.next_field()? {
                Value::Bool(b) => Ok(b),
                _ => Err(LoomError::corrupt("expected a bool")),
            }
        }

        pub fn text(&mut self) -> Result<String> {
            as_text(self.next_field()?)
        }

        pub fn array(&mut self) -> Result<Vec<Value>> {
            as_array(self.next_field()?)
        }

        pub fn end(mut self) -> Result<()> {
            if self.items.next().is_some() {
                Err(LoomError::corrupt("unexpected extra fields"))
            } else {
                Ok(())
            }
        }
    }
}

const DATAFRAME_PLAN_VERSION: u64 = 1;
const DATAFRAME_PLAN_DIGEST_DOMAIN: &[u8] = b"loom-dataframe-plan-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DataframeSourceKind {
    Files,
    Cas,
    Columnar,
    SqlResult,
}

impl DataframeSourceKind {
    pub const fn tag(self) -> u8 {
        match self {
            DataframeSourceKind::Files => 0,
            DataframeSourceKind::Cas => 1,
            DataframeSourceKind::Columnar => 2,
            DataframeSourceKind::SqlResult => 3,
        }
    }

    pub fn from_tag(tag: u8) -> Result<Self> {
        Ok(match tag {
            0 => DataframeSourceKind::Files,
            1 => DataframeSourceKind::Cas,
            2 => DataframeSourceKind::Columnar,
            3 => DataframeSourceKind::SqlResult,
            _ => return Err(LoomError::corrupt("unknown dataframe source kind tag")),
        })
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            DataframeSourceKind::Files => "files",
            DataframeSourceKind::Cas => "cas",
            DataframeSourceKind::Columnar => "columnar",
            DataframeSourceKind::SqlResult => "sql-result",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DataframeInputFormat {
    Native,
    Csv,
    Json,
    Ndjson,
    ArrowIpc,
    Parquet,
}

impl DataframeInputFormat {
    pub const fn tag(self) -> u8 {
        match self {
            DataframeInputFormat::Native => 0,
            DataframeInputFormat::Csv => 1,
            DataframeInputFormat::Json => 2,
            DataframeInputFormat::Ndjson => 3,
            DataframeInputFormat::ArrowIpc => 4,
            DataframeInputFormat::Parquet => 5,
        }
    }

    pub fn from_tag(tag: u8) -> Result<Self> {
        Ok(match tag {
            0 => DataframeInputFormat::Native,
            1 => DataframeInputFormat::Csv,
            2 => DataframeInputFormat::Json,
            3 => DataframeInputFormat::Ndjson,
            4 => DataframeInputFormat::ArrowIpc,
            5 => DataframeInputFormat::Parquet,
            _ => return Err(LoomError::corrupt("unknown dataframe input format tag")),
        })
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            DataframeInputFormat::Native => "native",
            DataframeInputFormat::Csv => "csv",
            DataframeInputFormat::Json => "json",
            DataframeInputFormat::Ndjson => "ndjson",
            DataframeInputFormat::ArrowIpc => "arrow-ipc",
            DataframeInputFormat::Parquet => "parquet",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataframeSourceBinding {
    pub alias: String,
    pub kind: DataframeSourceKind,
    pub target: String,
    pub format: DataframeInputFormat,
    pub source_digest: Option<Digest>,
    pub options: BTreeMap<String, String>,
}

impl DataframeSourceBinding {
    pub fn new(
        alias: impl Into<String>,
        kind: DataframeSourceKind,
        target: impl Into<String>,
        format: DataframeInputFormat,
    ) -> Self {
        Self {
            alias: alias.into(),
            kind,
            target: target.into(),
            format,
            source_digest: None,
            options: BTreeMap::new(),
        }
    }

    pub fn with_source_digest(mut self, digest: Digest) -> Self {
        self.source_digest = Some(digest);
        self
    }

    pub fn with_option(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.options.insert(key.into(), value.into());
        self
    }

    fn encode_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.alias.clone()),
            Value::Uint(u64::from(self.kind.tag())),
            Value::Text(self.target.clone()),
            Value::Uint(u64::from(self.format.tag())),
            digest_or_null(self.source_digest.as_ref()),
            options_value(&self.options),
        ])
    }

    fn decode_value(value: Value) -> Result<Self> {
        let mut f = cbor::Fields::new(cbor::as_array(value)?);
        let alias = f.text()?;
        let kind = DataframeSourceKind::from_tag(cbor::u8_from(f.uint()?)?)?;
        let target = f.text()?;
        let format = DataframeInputFormat::from_tag(cbor::u8_from(f.uint()?)?)?;
        let source_digest = digest_from_nullable(f.next_field()?)?;
        let options = options_from_value(f.next_field()?)?;
        f.end()?;
        Ok(Self {
            alias,
            kind,
            target,
            format,
            source_digest,
            options,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataframeColumn {
    pub name: String,
    pub column_type: ColumnType,
    pub nullable: bool,
}

impl DataframeColumn {
    pub fn new(name: impl Into<String>, column_type: ColumnType, nullable: bool) -> Self {
        Self {
            name: name.into(),
            column_type,
            nullable,
        }
    }

    fn encode_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.name.clone()),
            Value::Uint(u64::from(self.column_type.tag())),
            Value::Bool(self.nullable),
        ])
    }

    fn decode_value(value: Value) -> Result<Self> {
        let mut f = cbor::Fields::new(cbor::as_array(value)?);
        let name = f.text()?;
        let tag = cbor::u8_from(f.uint()?)?;
        let nullable = f.bool()?;
        f.end()?;
        Ok(Self {
            name,
            column_type: ColumnType::from_tag(tag)?,
            nullable,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataframeSchema {
    pub columns: Vec<DataframeColumn>,
    pub inferred: bool,
}

impl DataframeSchema {
    pub fn new(columns: Vec<DataframeColumn>, inferred: bool) -> Result<Self> {
        let schema = Self { columns, inferred };
        schema.validate()?;
        Ok(schema)
    }

    pub fn validate(&self) -> Result<()> {
        if self.columns.is_empty() {
            return Err(LoomError::invalid("dataframe schema has no columns"));
        }
        let mut names = BTreeSet::new();
        for column in &self.columns {
            validate_nonempty("dataframe column name", &column.name)?;
            if !names.insert(column.name.as_str()) {
                return Err(LoomError::invalid(format!(
                    "duplicate dataframe column {:?}",
                    column.name
                )));
            }
        }
        Ok(())
    }

    fn encode_value(&self) -> Value {
        Value::Array(vec![
            Value::Array(
                self.columns
                    .iter()
                    .map(DataframeColumn::encode_value)
                    .collect(),
            ),
            Value::Bool(self.inferred),
        ])
    }

    fn decode_value(value: Value) -> Result<Self> {
        let mut f = cbor::Fields::new(cbor::as_array(value)?);
        let columns = f
            .array()?
            .into_iter()
            .map(DataframeColumn::decode_value)
            .collect::<Result<Vec<_>>>()?;
        let inferred = f.bool()?;
        f.end()?;
        Self::new(columns, inferred)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataframeAggregation {
    pub output: String,
    pub function: String,
    pub input: Option<String>,
}

impl DataframeAggregation {
    pub fn new(
        output: impl Into<String>,
        function: impl Into<String>,
        input: Option<String>,
    ) -> Self {
        Self {
            output: output.into(),
            function: function.into(),
            input,
        }
    }

    fn encode_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.output.clone()),
            Value::Text(self.function.clone()),
            text_or_null(self.input.as_deref()),
        ])
    }

    fn decode_value(value: Value) -> Result<Self> {
        let mut f = cbor::Fields::new(cbor::as_array(value)?);
        let output = f.text()?;
        let function = f.text()?;
        let input = text_from_nullable(f.next_field()?)?;
        f.end()?;
        Ok(Self {
            output,
            function,
            input,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataframeOperation {
    Scan {
        source: String,
    },
    Select {
        columns: Vec<String>,
    },
    Rename {
        from: String,
        to: String,
    },
    Cast {
        column: String,
        column_type: ColumnType,
    },
    Filter {
        expression: String,
    },
    Sort {
        columns: Vec<String>,
        descending: bool,
    },
    Limit {
        rows: u64,
    },
    Sample {
        rows: u64,
        seed: u64,
    },
    Join {
        right_source: String,
        left_on: Vec<String>,
        right_on: Vec<String>,
        join_type: String,
    },
    Union {
        source: String,
    },
    Aggregate {
        group_by: Vec<String>,
        aggregations: Vec<DataframeAggregation>,
    },
    WithColumn {
        column: String,
        expression: String,
    },
}

impl DataframeOperation {
    fn encode_value(&self) -> Value {
        match self {
            DataframeOperation::Scan { source } => tagged(0, vec![Value::Text(source.clone())]),
            DataframeOperation::Select { columns } => tagged(1, vec![string_array(columns)]),
            DataframeOperation::Rename { from, to } => {
                tagged(2, vec![Value::Text(from.clone()), Value::Text(to.clone())])
            }
            DataframeOperation::Cast {
                column,
                column_type,
            } => tagged(
                3,
                vec![
                    Value::Text(column.clone()),
                    Value::Uint(u64::from(column_type.tag())),
                ],
            ),
            DataframeOperation::Filter { expression } => {
                tagged(4, vec![Value::Text(expression.clone())])
            }
            DataframeOperation::Sort {
                columns,
                descending,
            } => tagged(5, vec![string_array(columns), Value::Bool(*descending)]),
            DataframeOperation::Limit { rows } => tagged(6, vec![Value::Uint(*rows)]),
            DataframeOperation::Sample { rows, seed } => {
                tagged(7, vec![Value::Uint(*rows), Value::Uint(*seed)])
            }
            DataframeOperation::Join {
                right_source,
                left_on,
                right_on,
                join_type,
            } => tagged(
                8,
                vec![
                    Value::Text(right_source.clone()),
                    string_array(left_on),
                    string_array(right_on),
                    Value::Text(join_type.clone()),
                ],
            ),
            DataframeOperation::Union { source } => tagged(9, vec![Value::Text(source.clone())]),
            DataframeOperation::Aggregate {
                group_by,
                aggregations,
            } => tagged(
                10,
                vec![
                    string_array(group_by),
                    Value::Array(
                        aggregations
                            .iter()
                            .map(DataframeAggregation::encode_value)
                            .collect(),
                    ),
                ],
            ),
            DataframeOperation::WithColumn { column, expression } => tagged(
                11,
                vec![Value::Text(column.clone()), Value::Text(expression.clone())],
            ),
        }
    }

    fn decode_value(value: Value) -> Result<Self> {
        let mut f = cbor::Fields::new(cbor::as_array(value)?);
        let tag = cbor::u8_from(f.uint()?)?;
        let op = match tag {
            0 => DataframeOperation::Scan { source: f.text()? },
            1 => DataframeOperation::Select {
                columns: string_vec(f.next_field()?)?,
            },
            2 => DataframeOperation::Rename {
                from: f.text()?,
                to: f.text()?,
            },
            3 => DataframeOperation::Cast {
                column: f.text()?,
                column_type: ColumnType::from_tag(cbor::u8_from(f.uint()?)?)?,
            },
            4 => DataframeOperation::Filter {
                expression: f.text()?,
            },
            5 => DataframeOperation::Sort {
                columns: string_vec(f.next_field()?)?,
                descending: f.bool()?,
            },
            6 => DataframeOperation::Limit { rows: f.uint()? },
            7 => DataframeOperation::Sample {
                rows: f.uint()?,
                seed: f.uint()?,
            },
            8 => DataframeOperation::Join {
                right_source: f.text()?,
                left_on: string_vec(f.next_field()?)?,
                right_on: string_vec(f.next_field()?)?,
                join_type: f.text()?,
            },
            9 => DataframeOperation::Union { source: f.text()? },
            10 => DataframeOperation::Aggregate {
                group_by: string_vec(f.next_field()?)?,
                aggregations: cbor::as_array(f.next_field()?)?
                    .into_iter()
                    .map(DataframeAggregation::decode_value)
                    .collect::<Result<Vec<_>>>()?,
            },
            11 => DataframeOperation::WithColumn {
                column: f.text()?,
                expression: f.text()?,
            },
            _ => return Err(LoomError::corrupt("unknown dataframe operation tag")),
        };
        f.end()?;
        Ok(op)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataframeMaterializationTarget {
    Columnar,
    Files,
    Cas,
    EphemeralPreview,
}

impl DataframeMaterializationTarget {
    pub const fn tag(self) -> u8 {
        match self {
            DataframeMaterializationTarget::Columnar => 0,
            DataframeMaterializationTarget::Files => 1,
            DataframeMaterializationTarget::Cas => 2,
            DataframeMaterializationTarget::EphemeralPreview => 3,
        }
    }

    pub fn from_tag(tag: u8) -> Result<Self> {
        Ok(match tag {
            0 => DataframeMaterializationTarget::Columnar,
            1 => DataframeMaterializationTarget::Files,
            2 => DataframeMaterializationTarget::Cas,
            3 => DataframeMaterializationTarget::EphemeralPreview,
            _ => {
                return Err(LoomError::corrupt(
                    "unknown dataframe materialization target",
                ));
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataframeMaterialization {
    pub target: DataframeMaterializationTarget,
    pub destination: Option<String>,
    pub format: DataframeInputFormat,
}

impl DataframeMaterialization {
    pub fn new(
        target: DataframeMaterializationTarget,
        destination: Option<String>,
        format: DataframeInputFormat,
    ) -> Self {
        Self {
            target,
            destination,
            format,
        }
    }

    fn encode_value(&self) -> Value {
        Value::Array(vec![
            Value::Uint(u64::from(self.target.tag())),
            text_or_null(self.destination.as_deref()),
            Value::Uint(u64::from(self.format.tag())),
        ])
    }

    fn decode_value(value: Value) -> Result<Self> {
        let mut f = cbor::Fields::new(cbor::as_array(value)?);
        let target = DataframeMaterializationTarget::from_tag(cbor::u8_from(f.uint()?)?)?;
        let destination = text_from_nullable(f.next_field()?)?;
        let format = DataframeInputFormat::from_tag(cbor::u8_from(f.uint()?)?)?;
        f.end()?;
        Ok(Self {
            target,
            destination,
            format,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataframePlan {
    pub sources: Vec<DataframeSourceBinding>,
    pub schema: Option<DataframeSchema>,
    pub operations: Vec<DataframeOperation>,
    pub materialization: Option<DataframeMaterialization>,
}

impl DataframePlan {
    pub fn new(sources: Vec<DataframeSourceBinding>) -> Result<Self> {
        let plan = Self {
            sources,
            schema: None,
            operations: Vec::new(),
            materialization: None,
        };
        plan.validate()?;
        Ok(plan)
    }

    pub fn with_schema(mut self, schema: DataframeSchema) -> Result<Self> {
        self.schema = Some(schema);
        self.validate()?;
        Ok(self)
    }

    pub fn with_operations(mut self, operations: Vec<DataframeOperation>) -> Result<Self> {
        self.operations = operations;
        self.validate()?;
        Ok(self)
    }

    pub fn with_materialization(
        mut self,
        materialization: DataframeMaterialization,
    ) -> Result<Self> {
        self.materialization = Some(materialization);
        self.validate()?;
        Ok(self)
    }

    pub fn source_digests(&self) -> Vec<Digest> {
        self.sources
            .iter()
            .filter_map(|source| source.source_digest)
            .collect()
    }

    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Uint(DATAFRAME_PLAN_VERSION),
            Value::Array(
                self.sources
                    .iter()
                    .map(DataframeSourceBinding::encode_value)
                    .collect(),
            ),
            optional_value(self.schema.as_ref().map(DataframeSchema::encode_value)),
            Value::Array(
                self.operations
                    .iter()
                    .map(DataframeOperation::encode_value)
                    .collect(),
            ),
            optional_value(
                self.materialization
                    .as_ref()
                    .map(DataframeMaterialization::encode_value),
            ),
        ]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut f = cbor::Fields::new(cbor::decode_array(bytes)?);
        let version = f.uint()?;
        if version != DATAFRAME_PLAN_VERSION {
            return Err(LoomError::corrupt("unsupported dataframe plan version"));
        }
        let sources = f
            .array()?
            .into_iter()
            .map(DataframeSourceBinding::decode_value)
            .collect::<Result<Vec<_>>>()?;
        let schema = match f.next_field()? {
            Value::Null => None,
            value => Some(DataframeSchema::decode_value(value)?),
        };
        let operations = f
            .array()?
            .into_iter()
            .map(DataframeOperation::decode_value)
            .collect::<Result<Vec<_>>>()?;
        let materialization = match f.next_field()? {
            Value::Null => None,
            value => Some(DataframeMaterialization::decode_value(value)?),
        };
        f.end()?;
        let plan = Self {
            sources,
            schema,
            operations,
            materialization,
        };
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<()> {
        if self.sources.is_empty() {
            return Err(LoomError::invalid("dataframe plan has no sources"));
        }
        let mut source_aliases = BTreeSet::new();
        for source in &self.sources {
            validate_nonempty("dataframe source alias", &source.alias)?;
            validate_nonempty("dataframe source target", &source.target)?;
            if !source_aliases.insert(source.alias.as_str()) {
                return Err(LoomError::invalid(format!(
                    "duplicate dataframe source alias {:?}",
                    source.alias
                )));
            }
            for key in source.options.keys() {
                validate_nonempty("dataframe source option key", key)?;
            }
        }
        if let Some(schema) = &self.schema {
            schema.validate()?;
        }
        for operation in &self.operations {
            validate_operation(operation, &source_aliases)?;
        }
        if let Some(materialization) = &self.materialization {
            validate_materialization(materialization)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataframeBatch {
    pub columns: Vec<DataframeColumn>,
    pub rows: Vec<Vec<DataValue>>,
}

impl DataframeBatch {
    pub fn new(columns: Vec<DataframeColumn>, rows: Vec<Vec<DataValue>>) -> Result<Self> {
        let batch = Self { columns, rows };
        batch.validate()?;
        Ok(batch)
    }

    pub fn schema(&self, inferred: bool) -> Result<DataframeSchema> {
        DataframeSchema::new(self.columns.clone(), inferred)
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn limit(&self, rows: u64) -> Result<Self> {
        let rows = usize::try_from(rows)
            .map_err(|_| LoomError::invalid("dataframe limit is out of range"))?;
        Self::new(
            self.columns.clone(),
            self.rows.iter().take(rows).cloned().collect(),
        )
    }

    pub fn encode(&self) -> Vec<u8> {
        cbor::encode(&Value::Array(vec![
            Value::Array(
                self.columns
                    .iter()
                    .map(DataframeColumn::encode_value)
                    .collect(),
            ),
            Value::Array(
                self.rows
                    .iter()
                    .map(|row| Value::Array(row.iter().map(cell_value).collect()))
                    .collect(),
            ),
        ]))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut f = cbor::Fields::new(cbor::decode_array(bytes)?);
        let columns = f
            .array()?
            .into_iter()
            .map(DataframeColumn::decode_value)
            .collect::<Result<Vec<_>>>()?;
        let rows = f
            .array()?
            .into_iter()
            .map(|row| {
                cbor::as_array(row)?
                    .into_iter()
                    .map(cell_from)
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?;
        f.end()?;
        Self::new(columns, rows)
    }

    fn validate(&self) -> Result<()> {
        DataframeSchema::new(self.columns.clone(), true)?;
        for row in &self.rows {
            if row.len() != self.columns.len() {
                return Err(LoomError::invalid("dataframe row arity mismatch"));
            }
            for (value, column) in row.iter().zip(&self.columns) {
                if !value.matches(column.column_type) {
                    return Err(LoomError::invalid(format!(
                        "dataframe column {:?} expects {:?}",
                        column.name, column.column_type
                    )));
                }
                if !column.nullable && matches!(value, DataValue::Null) {
                    return Err(LoomError::invalid(format!(
                        "dataframe column {:?} is not nullable",
                        column.name
                    )));
                }
            }
        }
        Ok(())
    }
}

pub trait DataframeExecutor {
    fn execute(
        &self,
        plan: &DataframePlan,
        sources: &BTreeMap<String, DataframeBatch>,
    ) -> Result<DataframeBatch>;
}

pub fn execute_loaded_dataframe_plan(
    plan: &DataframePlan,
    sources: &BTreeMap<String, DataframeBatch>,
) -> Result<DataframeBatch> {
    let mut current: Option<DataframeBatch> = None;
    for operation in &plan.operations {
        current = Some(match operation {
            DataframeOperation::Scan { source } => {
                sources.get(source).cloned().ok_or_else(|| {
                    LoomError::invalid(format!("unknown dataframe source {source:?}"))
                })?
            }
            DataframeOperation::Select { columns } => {
                select_columns(current_batch(&current)?, columns)?
            }
            DataframeOperation::Rename { from, to } => {
                rename_column(current_batch(&current)?, from, to)?
            }
            DataframeOperation::Cast {
                column,
                column_type,
            } => cast_column(current_batch(&current)?, column, *column_type)?,
            DataframeOperation::Filter { expression } => {
                filter_rows(current_batch(&current)?, expression)?
            }
            DataframeOperation::Sort {
                columns,
                descending,
            } => sort_rows(current_batch(&current)?, columns, *descending)?,
            DataframeOperation::Limit { rows } => current_batch(&current)?.limit(*rows)?,
            DataframeOperation::Sample { rows, seed } => {
                sample_rows(current_batch(&current)?, *rows, *seed)?
            }
            DataframeOperation::Aggregate {
                group_by,
                aggregations,
            } => aggregate_rows(current_batch(&current)?, group_by, aggregations)?,
            DataframeOperation::WithColumn { column, expression } => {
                with_literal_column(current_batch(&current)?, column, expression)?
            }
            DataframeOperation::Join { .. } => {
                return Err(LoomError::new(
                    Code::Unsupported,
                    "dataframe join execution requires promoted join semantics",
                ));
            }
            DataframeOperation::Union { source } => {
                let right = sources.get(source).ok_or_else(|| {
                    LoomError::invalid(format!("unknown dataframe source {source:?}"))
                })?;
                union_rows(current_batch(&current)?, right)?
            }
        });
    }
    current.ok_or_else(|| LoomError::invalid("dataframe plan has no scan operation"))
}

fn current_batch(current: &Option<DataframeBatch>) -> Result<&DataframeBatch> {
    current
        .as_ref()
        .ok_or_else(|| LoomError::invalid("dataframe operation requires a prior scan"))
}

pub fn dataframe_parse_portable_bytes(
    format: DataframeInputFormat,
    bytes: &[u8],
    schema: Option<&DataframeSchema>,
    options: &BTreeMap<String, String>,
) -> Result<DataframeBatch> {
    match format {
        DataframeInputFormat::Csv => parse_csv(bytes, schema, options),
        DataframeInputFormat::Json => parse_json_records(bytes, schema, false),
        DataframeInputFormat::Ndjson => parse_json_records(bytes, schema, true),
        DataframeInputFormat::Native => Err(LoomError::new(
            Code::Unsupported,
            "native dataframe batch bytes are not promoted",
        )),
        DataframeInputFormat::ArrowIpc | DataframeInputFormat::Parquet => Err(LoomError::new(
            Code::Unsupported,
            "dataframe binary columnar formats require the core columnar adapter",
        )),
    }
}

pub fn dataframe_plan_digest(plan: &DataframePlan, algo: Algo) -> Digest {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(DATAFRAME_PLAN_DIGEST_DOMAIN);
    bytes.extend_from_slice(&plan.encode());
    Digest::hash(algo, &bytes)
}

fn parse_csv(
    bytes: &[u8],
    schema: Option<&DataframeSchema>,
    options: &BTreeMap<String, String>,
) -> Result<DataframeBatch> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| LoomError::invalid(format!("dataframe CSV is not UTF-8: {e}")))?;
    let has_header = options
        .get("has_header")
        .map(|value| value == "true")
        .unwrap_or(true);
    let mut rows = csv_records(text)?;
    if rows.is_empty() {
        return Err(LoomError::invalid("dataframe CSV has no rows"));
    }
    let names = if let Some(schema) = schema {
        schema
            .columns
            .iter()
            .map(|column| column.name.clone())
            .collect::<Vec<_>>()
    } else if has_header {
        rows.remove(0)
    } else {
        (0..rows[0].len())
            .map(|index| format!("column{}", index + 1))
            .collect()
    };
    let raw_rows = rows
        .into_iter()
        .filter(|row| row.iter().any(|cell| !cell.is_empty()))
        .collect::<Vec<_>>();
    if raw_rows.iter().any(|row| row.len() != names.len()) {
        return Err(LoomError::invalid("dataframe CSV row arity mismatch"));
    }
    let parsed_rows = raw_rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| parse_scalar(cell))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let columns = schema
        .map(|schema| schema.columns.clone())
        .unwrap_or_else(|| dataframe_infer_columns(&names, &parsed_rows));
    let inferred_schema = DataframeSchema::new(columns, schema.is_none())?;
    DataframeBatch::new(
        inferred_schema.columns.clone(),
        dataframe_coerce_rows(parsed_rows, Some(&inferred_schema))?,
    )
}

fn csv_records(text: &str) -> Result<Vec<Vec<String>>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && matches!(chars.peek(), Some('"')) => {
                chars.next();
                field.push('"');
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                record.push(std::mem::take(&mut field));
            }
            '\n' if !quoted => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            '\r' if !quoted => {}
            _ => field.push(ch),
        }
    }
    if quoted {
        return Err(LoomError::invalid("unterminated dataframe CSV quote"));
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    Ok(records)
}

fn parse_json_records(
    bytes: &[u8],
    schema: Option<&DataframeSchema>,
    ndjson: bool,
) -> Result<DataframeBatch> {
    let values = if ndjson {
        let text = std::str::from_utf8(bytes)
            .map_err(|e| LoomError::invalid(format!("dataframe NDJSON is not UTF-8: {e}")))?;
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).map_err(json_err))
            .collect::<Result<Vec<serde_json::Value>>>()?
    } else {
        match serde_json::from_slice(bytes).map_err(json_err)? {
            serde_json::Value::Array(items) => items,
            value => vec![value],
        }
    };
    let mut records = Vec::new();
    let mut keys = BTreeSet::new();
    for value in values {
        let serde_json::Value::Object(map) = value else {
            return Err(LoomError::invalid("dataframe JSON records must be objects"));
        };
        for key in map.keys() {
            keys.insert(key.clone());
        }
        records.push(map);
    }
    let names = schema
        .map(|schema| {
            schema
                .columns
                .iter()
                .map(|column| column.name.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| keys.into_iter().collect::<Vec<_>>());
    let parsed_rows = records
        .iter()
        .map(|record| {
            names
                .iter()
                .map(|name| {
                    record
                        .get(name)
                        .map(json_to_data_value)
                        .unwrap_or(Ok(DataValue::Null))
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    let columns = schema
        .map(|schema| schema.columns.clone())
        .unwrap_or_else(|| dataframe_infer_columns(&names, &parsed_rows));
    let inferred_schema = DataframeSchema::new(columns, schema.is_none())?;
    DataframeBatch::new(
        inferred_schema.columns.clone(),
        dataframe_coerce_rows(parsed_rows, Some(&inferred_schema))?,
    )
}

fn json_err(error: serde_json::Error) -> LoomError {
    LoomError::invalid(format!("dataframe JSON parse error: {error}"))
}

fn json_to_data_value(value: &serde_json::Value) -> Result<DataValue> {
    Ok(match value {
        serde_json::Value::Null => DataValue::Null,
        serde_json::Value::Bool(value) => DataValue::Bool(*value),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                DataValue::Int(value)
            } else if let Some(value) = value.as_u64() {
                DataValue::U64(value)
            } else {
                DataValue::Float(
                    value
                        .as_f64()
                        .ok_or_else(|| LoomError::invalid("non-finite dataframe JSON number"))?,
                )
            }
        }
        serde_json::Value::String(value) => DataValue::Text(value.clone()),
        serde_json::Value::Array(items) => DataValue::List(
            items
                .iter()
                .map(json_to_data_value)
                .collect::<Result<Vec<_>>>()?,
        ),
        serde_json::Value::Object(map) => DataValue::Map(
            map.iter()
                .map(|(key, value)| Ok((key.clone(), json_to_data_value(value)?)))
                .collect::<Result<BTreeMap<_, _>>>()?,
        ),
    })
}

pub fn dataframe_infer_columns(names: &[String], rows: &[Vec<DataValue>]) -> Vec<DataframeColumn> {
    names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let values = rows
                .iter()
                .filter_map(|row| row.get(index))
                .collect::<Vec<_>>();
            DataframeColumn::new(name.clone(), infer_column_type(&values), true)
        })
        .collect()
}

fn infer_column_type(values: &[&DataValue]) -> ColumnType {
    let mut out: Option<ColumnType> = None;
    for value in values {
        if matches!(value, DataValue::Null) {
            continue;
        }
        let ty = dataframe_value_type(value);
        out = Some(match (out, ty) {
            (None, ty) => ty,
            (Some(a), b) if a == b => a,
            (Some(ColumnType::Int), ColumnType::Float)
            | (Some(ColumnType::Float), ColumnType::Int)
            | (Some(ColumnType::U64), ColumnType::Float)
            | (Some(ColumnType::Float), ColumnType::U64) => ColumnType::Float,
            _ => ColumnType::Text,
        });
    }
    out.unwrap_or(ColumnType::Text)
}

pub fn dataframe_value_type(value: &DataValue) -> ColumnType {
    match value {
        DataValue::Null => ColumnType::Text,
        DataValue::Bool(_) => ColumnType::Bool,
        DataValue::Int(_) => ColumnType::Int,
        DataValue::Float(_) => ColumnType::Float,
        DataValue::Text(_) => ColumnType::Text,
        DataValue::Bytes(_) => ColumnType::Bytes,
        DataValue::I8(_) => ColumnType::I8,
        DataValue::I16(_) => ColumnType::I16,
        DataValue::I32(_) => ColumnType::I32,
        DataValue::I128(_) => ColumnType::I128,
        DataValue::U8(_) => ColumnType::U8,
        DataValue::U16(_) => ColumnType::U16,
        DataValue::U32(_) => ColumnType::U32,
        DataValue::U64(_) => ColumnType::U64,
        DataValue::U128(_) => ColumnType::U128,
        DataValue::F32(_) => ColumnType::F32,
        DataValue::Decimal { .. } => ColumnType::Decimal,
        DataValue::Date(_) => ColumnType::Date,
        DataValue::Time(_) => ColumnType::Time,
        DataValue::Timestamp(_) => ColumnType::Timestamp,
        DataValue::Interval { .. } => ColumnType::Interval,
        DataValue::Uuid(_) => ColumnType::Uuid,
        DataValue::Inet(_) => ColumnType::Inet,
        DataValue::Point { .. } => ColumnType::Point,
        DataValue::List(_) => ColumnType::List,
        DataValue::Map(_) => ColumnType::Map,
    }
}

pub fn dataframe_coerce_rows(
    rows: Vec<Vec<DataValue>>,
    schema: Option<&DataframeSchema>,
) -> Result<Vec<Vec<DataValue>>> {
    let Some(schema) = schema else {
        return Ok(rows);
    };
    rows.into_iter()
        .map(|row| {
            row.into_iter()
                .zip(&schema.columns)
                .map(|(value, column)| dataframe_coerce_value(value, column.column_type))
                .collect()
        })
        .collect()
}

pub fn dataframe_coerce_value(value: DataValue, ty: ColumnType) -> Result<DataValue> {
    if value.matches(ty) {
        return Ok(value);
    }
    match (value, ty) {
        (DataValue::Text(value), ColumnType::Int) => value
            .parse::<i64>()
            .map(DataValue::Int)
            .map_err(|e| LoomError::invalid(format!("dataframe int parse failed: {e}"))),
        (DataValue::Text(value), ColumnType::Float) => value
            .parse::<f64>()
            .map(DataValue::Float)
            .map_err(|e| LoomError::invalid(format!("dataframe float parse failed: {e}"))),
        (DataValue::Text(value), ColumnType::Bool) => value
            .parse::<bool>()
            .map(DataValue::Bool)
            .map_err(|e| LoomError::invalid(format!("dataframe bool parse failed: {e}"))),
        (DataValue::Int(value), ColumnType::Float) => Ok(DataValue::Float(value as f64)),
        (DataValue::U64(value), ColumnType::Float) => Ok(DataValue::Float(value as f64)),
        (DataValue::U64(value), ColumnType::Int) => i64::try_from(value)
            .map(DataValue::Int)
            .map_err(|_| LoomError::invalid("dataframe u64 does not fit int")),
        (value, _) => Err(LoomError::invalid(format!(
            "dataframe value {:?} cannot be coerced to {:?}",
            value, ty
        ))),
    }
}

fn select_columns(batch: &DataframeBatch, selected: &[String]) -> Result<DataframeBatch> {
    let indexes = selected
        .iter()
        .map(|name| column_index(batch, name))
        .collect::<Result<Vec<_>>>()?;
    DataframeBatch::new(
        indexes
            .iter()
            .map(|index| batch.columns[*index].clone())
            .collect(),
        batch
            .rows
            .iter()
            .map(|row| indexes.iter().map(|index| row[*index].clone()).collect())
            .collect(),
    )
}

fn rename_column(batch: &DataframeBatch, from: &str, to: &str) -> Result<DataframeBatch> {
    let index = column_index(batch, from)?;
    let mut columns = batch.columns.clone();
    columns[index].name = to.to_string();
    DataframeBatch::new(columns, batch.rows.clone())
}

fn cast_column(
    batch: &DataframeBatch,
    column: &str,
    column_type: ColumnType,
) -> Result<DataframeBatch> {
    let index = column_index(batch, column)?;
    let mut columns = batch.columns.clone();
    columns[index].column_type = column_type;
    let rows = batch
        .rows
        .iter()
        .map(|row| {
            let mut row = row.clone();
            row[index] = dataframe_coerce_value(row[index].clone(), column_type)?;
            Ok(row)
        })
        .collect::<Result<Vec<_>>>()?;
    DataframeBatch::new(columns, rows)
}

fn filter_rows(batch: &DataframeBatch, expression: &str) -> Result<DataframeBatch> {
    let (column, op, rhs) = parse_filter_expression(expression)?;
    let index = column_index(batch, &column)?;
    let rhs = dataframe_coerce_value(rhs, batch.columns[index].column_type)?;
    let rows = batch
        .rows
        .iter()
        .filter(|row| cmp_filter(&row[index], op, &rhs))
        .cloned()
        .collect();
    DataframeBatch::new(batch.columns.clone(), rows)
}

fn parse_filter_expression(expression: &str) -> Result<(String, &'static str, DataValue)> {
    for op in ["==", "!=", ">=", "<=", ">", "<"] {
        if let Some((left, right)) = expression.split_once(op) {
            return Ok((
                left.trim().to_string(),
                op,
                dataframe_parse_literal(right.trim())?,
            ));
        }
    }
    Err(LoomError::new(
        Code::Unsupported,
        "dataframe filter expression must use ==, !=, >=, <=, >, or <",
    ))
}

pub fn dataframe_parse_literal(value: &str) -> Result<DataValue> {
    let quoted = (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''));
    if quoted && value.len() >= 2 {
        Ok(DataValue::Text(value[1..value.len() - 1].to_string()))
    } else {
        Ok(parse_scalar(value))
    }
}

fn parse_scalar(cell: &str) -> DataValue {
    let trimmed = cell.trim();
    if trimmed.is_empty() {
        DataValue::Null
    } else if trimmed == "true" {
        DataValue::Bool(true)
    } else if trimmed == "false" {
        DataValue::Bool(false)
    } else if let Ok(value) = trimmed.parse::<i64>() {
        DataValue::Int(value)
    } else if let Ok(value) = trimmed.parse::<f64>() {
        DataValue::Float(value)
    } else {
        DataValue::Text(cell.to_string())
    }
}

fn cmp_filter(left: &DataValue, op: &str, right: &DataValue) -> bool {
    match op {
        "==" => left == right,
        "!=" => left != right,
        ">" => left > right,
        ">=" => left >= right,
        "<" => left < right,
        "<=" => left <= right,
        _ => false,
    }
}

fn sort_rows(
    batch: &DataframeBatch,
    columns: &[String],
    descending: bool,
) -> Result<DataframeBatch> {
    let indexes = columns
        .iter()
        .map(|name| column_index(batch, name))
        .collect::<Result<Vec<_>>>()?;
    let mut rows = batch.rows.clone();
    rows.sort_by(|a, b| {
        let ordering = indexes
            .iter()
            .map(|index| a[*index].cmp(&b[*index]))
            .find(|ordering| !ordering.is_eq())
            .unwrap_or(std::cmp::Ordering::Equal);
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
    DataframeBatch::new(batch.columns.clone(), rows)
}

fn sample_rows(batch: &DataframeBatch, rows: u64, seed: u64) -> Result<DataframeBatch> {
    let rows = usize::try_from(rows)
        .map_err(|_| LoomError::invalid("dataframe sample row count is out of range"))?;
    let mut indexed = batch
        .rows
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, row)| {
            let mut bytes = seed.to_le_bytes().to_vec();
            bytes.extend_from_slice(&(index as u64).to_le_bytes());
            (Digest::blake3(&bytes), row)
        })
        .collect::<Vec<_>>();
    indexed.sort_by_key(|(digest, _)| *digest);
    DataframeBatch::new(
        batch.columns.clone(),
        indexed.into_iter().take(rows).map(|(_, row)| row).collect(),
    )
}

fn aggregate_rows(
    batch: &DataframeBatch,
    group_by: &[String],
    aggregations: &[DataframeAggregation],
) -> Result<DataframeBatch> {
    let group_indexes = group_by
        .iter()
        .map(|name| column_index(batch, name))
        .collect::<Result<Vec<_>>>()?;
    let mut groups: BTreeMap<Vec<DataValue>, Vec<&Vec<DataValue>>> = BTreeMap::new();
    for row in &batch.rows {
        groups
            .entry(
                group_indexes
                    .iter()
                    .map(|index| row[*index].clone())
                    .collect(),
            )
            .or_default()
            .push(row);
    }
    let mut columns = group_indexes
        .iter()
        .map(|index| batch.columns[*index].clone())
        .collect::<Vec<_>>();
    for aggregation in aggregations {
        columns.push(DataframeColumn::new(
            aggregation.output.clone(),
            aggregate_output_type(aggregation),
            true,
        ));
    }
    let rows = groups
        .into_iter()
        .map(|(mut key, rows)| {
            for aggregation in aggregations {
                key.push(evaluate_dataframe_aggregate(batch, &rows, aggregation)?);
            }
            Ok(key)
        })
        .collect::<Result<Vec<_>>>()?;
    DataframeBatch::new(columns, rows)
}

fn aggregate_output_type(aggregation: &DataframeAggregation) -> ColumnType {
    match aggregation.function.as_str() {
        "count" => ColumnType::U64,
        _ => ColumnType::Float,
    }
}

fn evaluate_dataframe_aggregate(
    batch: &DataframeBatch,
    rows: &[&Vec<DataValue>],
    aggregation: &DataframeAggregation,
) -> Result<DataValue> {
    match aggregation.function.as_str() {
        "count" => Ok(DataValue::U64(rows.len() as u64)),
        "sum" | "min" | "max" => {
            let input = aggregation
                .input
                .as_deref()
                .ok_or_else(|| LoomError::invalid("dataframe aggregate input is required"))?;
            let index = column_index(batch, input)?;
            let values = rows
                .iter()
                .map(|row| &row[index])
                .filter(|value| !matches!(value, DataValue::Null))
                .collect::<Vec<_>>();
            match aggregation.function.as_str() {
                "min" => Ok(values.into_iter().min().cloned().unwrap_or(DataValue::Null)),
                "max" => Ok(values.into_iter().max().cloned().unwrap_or(DataValue::Null)),
                "sum" => sum_dataframe_values(values),
                _ => unreachable!(),
            }
        }
        other => Err(LoomError::new(
            Code::Unsupported,
            format!("unsupported dataframe aggregate {other:?}"),
        )),
    }
}

fn sum_dataframe_values(values: Vec<&DataValue>) -> Result<DataValue> {
    let mut sum = 0.0;
    let mut seen = false;
    for value in values {
        seen = true;
        sum += match value {
            DataValue::Int(value) => *value as f64,
            DataValue::U64(value) => *value as f64,
            DataValue::Float(value) => *value,
            DataValue::F32(value) => f64::from(*value),
            _ => return Err(LoomError::invalid("dataframe sum requires numeric values")),
        };
    }
    if seen {
        Ok(DataValue::Float(sum))
    } else {
        Ok(DataValue::Null)
    }
}

fn with_literal_column(
    batch: &DataframeBatch,
    column: &str,
    expression: &str,
) -> Result<DataframeBatch> {
    let value = dataframe_parse_literal(expression)?;
    let mut columns = batch.columns.clone();
    columns.push(DataframeColumn::new(
        column,
        dataframe_value_type(&value),
        true,
    ));
    let rows = batch
        .rows
        .iter()
        .map(|row| {
            let mut row = row.clone();
            row.push(value.clone());
            row
        })
        .collect();
    DataframeBatch::new(columns, rows)
}

fn union_rows(left: &DataframeBatch, right: &DataframeBatch) -> Result<DataframeBatch> {
    if left.columns != right.columns {
        return Err(LoomError::invalid("dataframe union schema mismatch"));
    }
    let mut rows = left.rows.clone();
    rows.extend(right.rows.clone());
    DataframeBatch::new(left.columns.clone(), rows)
}

fn column_index(batch: &DataframeBatch, name: &str) -> Result<usize> {
    batch
        .columns
        .iter()
        .position(|column| column.name == name)
        .ok_or_else(|| LoomError::invalid(format!("unknown dataframe column {name:?}")))
}
fn validate_operation(operation: &DataframeOperation, sources: &BTreeSet<&str>) -> Result<()> {
    match operation {
        DataframeOperation::Scan { source } | DataframeOperation::Union { source } => {
            validate_source_ref(source, sources)
        }
        DataframeOperation::Select { columns } | DataframeOperation::Sort { columns, .. } => {
            validate_nonempty_list("dataframe column list", columns)
        }
        DataframeOperation::Rename { from, to } => {
            validate_nonempty("dataframe rename source", from)?;
            validate_nonempty("dataframe rename target", to)
        }
        DataframeOperation::Cast { column, .. } => {
            validate_nonempty("dataframe cast column", column)
        }
        DataframeOperation::Filter { expression } => {
            validate_nonempty("dataframe filter expression", expression)
        }
        DataframeOperation::Limit { rows } | DataframeOperation::Sample { rows, .. } => {
            if *rows == 0 {
                Err(LoomError::invalid(
                    "dataframe row count must be greater than zero",
                ))
            } else {
                Ok(())
            }
        }
        DataframeOperation::Join {
            right_source,
            left_on,
            right_on,
            join_type,
        } => {
            validate_source_ref(right_source, sources)?;
            validate_nonempty_list("dataframe left join keys", left_on)?;
            validate_nonempty_list("dataframe right join keys", right_on)?;
            validate_nonempty("dataframe join type", join_type)
        }
        DataframeOperation::Aggregate {
            group_by,
            aggregations,
        } => {
            for column in group_by {
                validate_nonempty("dataframe group-by column", column)?;
            }
            if aggregations.is_empty() {
                return Err(LoomError::invalid(
                    "dataframe aggregate has no aggregations",
                ));
            }
            for aggregation in aggregations {
                validate_nonempty("dataframe aggregate output", &aggregation.output)?;
                validate_nonempty("dataframe aggregate function", &aggregation.function)?;
                if let Some(input) = &aggregation.input {
                    validate_nonempty("dataframe aggregate input", input)?;
                }
            }
            Ok(())
        }
        DataframeOperation::WithColumn { column, expression } => {
            validate_nonempty("dataframe with-column name", column)?;
            validate_nonempty("dataframe with-column expression", expression)
        }
    }
}

fn validate_materialization(materialization: &DataframeMaterialization) -> Result<()> {
    match materialization.target {
        DataframeMaterializationTarget::Cas => Ok(()),
        DataframeMaterializationTarget::EphemeralPreview => {
            if materialization.destination.is_some() {
                Err(LoomError::invalid(
                    "ephemeral dataframe materialization has no destination",
                ))
            } else {
                Ok(())
            }
        }
        _ => validate_nonempty(
            "dataframe materialization destination",
            materialization.destination.as_deref().unwrap_or(""),
        ),
    }
}

fn validate_source_ref(source: &str, sources: &BTreeSet<&str>) -> Result<()> {
    validate_nonempty("dataframe source reference", source)?;
    if sources.contains(source) {
        Ok(())
    } else {
        Err(LoomError::invalid(format!(
            "unknown dataframe source reference {source:?}"
        )))
    }
}

fn validate_nonempty(field: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        Err(LoomError::invalid(format!("{field} must not be empty")))
    } else {
        Ok(())
    }
}

fn validate_nonempty_list(field: &str, values: &[String]) -> Result<()> {
    if values.is_empty() {
        return Err(LoomError::invalid(format!("{field} must not be empty")));
    }
    for value in values {
        validate_nonempty(field, value)?;
    }
    Ok(())
}

fn tagged(tag: u8, mut values: Vec<Value>) -> Value {
    let mut out = Vec::with_capacity(values.len() + 1);
    out.push(Value::Uint(u64::from(tag)));
    out.append(&mut values);
    Value::Array(out)
}

fn digest_or_null(digest: Option<&Digest>) -> Value {
    digest.map_or(Value::Null, cbor::digest_value)
}

fn digest_from_nullable(value: Value) -> Result<Option<Digest>> {
    match value {
        Value::Null => Ok(None),
        other => Ok(Some(cbor::as_digest(other)?)),
    }
}

fn text_or_null(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |text| Value::Text(text.to_string()))
}

fn text_from_nullable(value: Value) -> Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        other => Ok(Some(cbor::as_text(other)?)),
    }
}

fn optional_value(value: Option<Value>) -> Value {
    value.unwrap_or(Value::Null)
}

fn string_array(values: &[String]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|value| Value::Text(value.clone()))
            .collect(),
    )
}

fn string_vec(value: Value) -> Result<Vec<String>> {
    cbor::as_array(value)?
        .into_iter()
        .map(cbor::as_text)
        .collect()
}

fn options_value(options: &BTreeMap<String, String>) -> Value {
    Value::Array(
        options
            .iter()
            .map(|(key, value)| {
                Value::Array(vec![Value::Text(key.clone()), Value::Text(value.clone())])
            })
            .collect(),
    )
}

fn options_from_value(value: Value) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for item in cbor::as_array(value)? {
        let mut f = cbor::Fields::new(cbor::as_array(item)?);
        let key = f.text()?;
        let value = f.text()?;
        f.end()?;
        if out.insert(key, value).is_some() {
            return Err(LoomError::corrupt("duplicate dataframe source option key"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_csv_with_header() {
        let batch = dataframe_parse_portable_bytes(
            DataframeInputFormat::Csv,
            b"name,count\nalpha,1\nbeta,2\n",
            None,
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(batch.columns[0].name, "name");
        assert_eq!(batch.columns[1].column_type, ColumnType::Int);
        assert_eq!(batch.rows[1][1], DataValue::Int(2));
    }

    #[test]
    fn parses_json_records_with_nested_values() {
        let batch = dataframe_parse_portable_bytes(
            DataframeInputFormat::Json,
            br#"[{"id":1,"tags":["a"],"meta":{"active":true}}]"#,
            None,
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(batch.columns.len(), 3);
        assert_eq!(batch.rows[0][0], DataValue::Int(1));
        assert!(matches!(batch.rows[0][1], DataValue::Map(_)));
        assert!(matches!(batch.rows[0][2], DataValue::List(_)));
    }

    #[test]
    fn coerces_rows_to_declared_schema() {
        let schema = DataframeSchema::new(
            vec![DataframeColumn::new("enabled", ColumnType::Bool, true)],
            false,
        )
        .unwrap();
        let batch = dataframe_parse_portable_bytes(
            DataframeInputFormat::Csv,
            b"true\n",
            Some(&schema),
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(batch.rows[0][0], DataValue::Bool(true));
    }

    #[test]
    fn plan_digest_uses_canonical_plan_bytes_and_domain() {
        let source = DataframeSourceBinding::new(
            "input",
            DataframeSourceKind::Files,
            "/data.csv",
            DataframeInputFormat::Csv,
        );
        let plan = DataframePlan::new(vec![source]).unwrap();
        let digest = dataframe_plan_digest(&plan, Algo::Blake3);
        let mut expected_bytes = Vec::new();
        expected_bytes.extend_from_slice(DATAFRAME_PLAN_DIGEST_DOMAIN);
        expected_bytes.extend_from_slice(&plan.encode());
        assert_eq!(digest, Digest::hash(Algo::Blake3, &expected_bytes));
    }

    #[test]
    fn batch_bytes_round_trip_canonically() {
        let batch = DataframeBatch::new(
            vec![DataframeColumn::new("id", ColumnType::Int, false)],
            vec![vec![DataValue::Int(7)]],
        )
        .unwrap();
        let encoded = batch.encode();
        assert_eq!(DataframeBatch::decode(&encoded).unwrap(), batch);
        assert_eq!(
            loom_codec::encode(&loom_codec::decode(&encoded).unwrap()).unwrap(),
            encoded
        );
    }
}
