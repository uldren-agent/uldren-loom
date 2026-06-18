//! Native Polars executor for the dataframe logical plan.

use loom_dataframe::{
    DataframeBatch, DataframeColumn, DataframeExecutor, DataframeOperation, DataframePlan,
    dataframe_coerce_value, dataframe_parse_literal, dataframe_value_type,
    execute_loaded_dataframe_plan,
};
use loom_types::{Code, ColumnType, LoomError, Result, Value as DataValue};
use polars::prelude::*;
use std::collections::BTreeMap;

pub const POLARS_DATAFRAME_EXECUTOR: PolarsDataframeExecutor = PolarsDataframeExecutor;

#[derive(Debug, Clone, Copy, Default)]
pub struct PolarsDataframeExecutor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolarsDataframeExecutionMode {
    Native,
    PortableFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolarsDataframeExecutionReport {
    pub mode: PolarsDataframeExecutionMode,
    pub reason: Option<&'static str>,
}

impl DataframeExecutor for PolarsDataframeExecutor {
    fn execute(
        &self,
        plan: &DataframePlan,
        sources: &BTreeMap<String, DataframeBatch>,
    ) -> Result<DataframeBatch> {
        match execute_polars_subset(plan, sources)? {
            PolarsExecution::Native(batch) => Ok(batch),
            PolarsExecution::Fallback(_) => execute_loaded_dataframe_plan(plan, sources),
        }
    }
}

pub fn polars_dataframe_execution_report(
    plan: &DataframePlan,
    sources: &BTreeMap<String, DataframeBatch>,
) -> Result<PolarsDataframeExecutionReport> {
    match execute_polars_subset(plan, sources)? {
        PolarsExecution::Native(_) => Ok(PolarsDataframeExecutionReport {
            mode: PolarsDataframeExecutionMode::Native,
            reason: None,
        }),
        PolarsExecution::Fallback(reason) => Ok(PolarsDataframeExecutionReport {
            mode: PolarsDataframeExecutionMode::PortableFallback,
            reason: Some(reason),
        }),
    }
}

enum PolarsExecution {
    Native(DataframeBatch),
    Fallback(&'static str),
}

struct PolarsFrame {
    df: DataFrame,
    columns: Vec<DataframeColumn>,
}

fn execute_polars_subset(
    plan: &DataframePlan,
    sources: &BTreeMap<String, DataframeBatch>,
) -> Result<PolarsExecution> {
    let mut current: Option<PolarsFrame> = None;
    for operation in &plan.operations {
        current = Some(match operation {
            DataframeOperation::Scan { source } => {
                let batch = sources.get(source).ok_or_else(|| {
                    LoomError::invalid(format!("unknown dataframe source {source:?}"))
                })?;
                let Some(frame) = batch_to_polars(batch)? else {
                    return Ok(PolarsExecution::Fallback("unsupported source scalar type"));
                };
                frame
            }
            DataframeOperation::Select { columns } => {
                let current = current_polars_frame(&current)?;
                let selected = columns
                    .iter()
                    .map(|name| {
                        current
                            .columns
                            .iter()
                            .find(|column| column.name == *name)
                            .cloned()
                            .ok_or_else(|| {
                                LoomError::invalid(format!("unknown dataframe column {name:?}"))
                            })
                    })
                    .collect::<Result<Vec<_>>>()?;
                PolarsFrame {
                    df: current
                        .df
                        .select(columns.iter().map(String::as_str))
                        .map_err(polars_err)?,
                    columns: selected,
                }
            }
            DataframeOperation::Rename { from, to } => {
                let current = current_polars_frame(&current)?;
                let mut df = current.df.clone();
                df.rename(from, to.clone().into()).map_err(polars_err)?;
                let mut columns = current.columns.clone();
                let column = columns
                    .iter_mut()
                    .find(|column| column.name == *from)
                    .ok_or_else(|| {
                        LoomError::invalid(format!("unknown dataframe column {from:?}"))
                    })?;
                column.name.clone_from(to);
                PolarsFrame { df, columns }
            }
            DataframeOperation::Sort {
                columns,
                descending,
            } => {
                let current = current_polars_frame(&current)?;
                let options = SortMultipleOptions::new().with_order_descending(*descending);
                PolarsFrame {
                    df: current
                        .df
                        .sort(columns.iter().map(String::as_str), options)
                        .map_err(polars_err)?,
                    columns: current.columns.clone(),
                }
            }
            DataframeOperation::Limit { rows } => {
                let current = current_polars_frame(&current)?;
                let rows = usize::try_from(*rows)
                    .map_err(|_| LoomError::invalid("dataframe limit does not fit usize"))?;
                PolarsFrame {
                    df: current.df.head(Some(rows)),
                    columns: current.columns.clone(),
                }
            }
            DataframeOperation::Cast {
                column,
                column_type,
            } => {
                let current = current_polars_frame(&current)?;
                let Some(frame) = cast_column(current, column, *column_type)? else {
                    return Ok(PolarsExecution::Fallback("unsupported cast target type"));
                };
                frame
            }
            DataframeOperation::WithColumn { column, expression } => {
                let current = current_polars_frame(&current)?;
                let Some(frame) = with_literal_column(current, column, expression)? else {
                    return Ok(PolarsExecution::Fallback("unsupported literal scalar type"));
                };
                frame
            }
            DataframeOperation::Union { source } => {
                let current = current_polars_frame(&current)?;
                let right = sources.get(source).ok_or_else(|| {
                    LoomError::invalid(format!("unknown dataframe source {source:?}"))
                })?;
                let Some(right) = batch_to_polars(right)? else {
                    return Ok(PolarsExecution::Fallback(
                        "unsupported union source scalar type",
                    ));
                };
                union_polars(current, &right)?
            }
            DataframeOperation::Filter { .. }
            | DataframeOperation::Sample { .. }
            | DataframeOperation::Aggregate { .. }
            | DataframeOperation::Join { .. } => {
                return Ok(PolarsExecution::Fallback("unsupported dataframe operation"));
            }
        });
    }
    let current =
        current.ok_or_else(|| LoomError::invalid("dataframe plan has no scan operation"))?;
    Ok(PolarsExecution::Native(polars_to_batch(current)?))
}

fn current_polars_frame(current: &Option<PolarsFrame>) -> Result<&PolarsFrame> {
    current
        .as_ref()
        .ok_or_else(|| LoomError::invalid("dataframe operation requires a prior scan"))
}

fn cast_column(
    frame: &PolarsFrame,
    column: &str,
    column_type: ColumnType,
) -> Result<Option<PolarsFrame>> {
    if !is_polars_supported_type(column_type) {
        return Ok(None);
    }
    let index = frame
        .columns
        .iter()
        .position(|current| current.name == column)
        .ok_or_else(|| LoomError::invalid(format!("unknown dataframe column {column:?}")))?;
    let mut batch = polars_frame_to_batch(frame)?;
    for row in &mut batch.rows {
        row[index] = dataframe_coerce_value(row[index].clone(), column_type)?;
    }
    batch.columns[index].column_type = column_type;
    batch_to_polars(&batch)
}

fn with_literal_column(
    frame: &PolarsFrame,
    column: &str,
    expression: &str,
) -> Result<Option<PolarsFrame>> {
    let value = dataframe_parse_literal(expression)?;
    let Some(polars_column) = literal_polars_column(column, &value, frame.df.height())? else {
        return Ok(None);
    };
    let mut df = frame.df.clone();
    df.with_column(polars_column).map_err(polars_err)?;
    let mut columns = frame.columns.clone();
    columns.push(DataframeColumn::new(
        column,
        dataframe_value_type(&value),
        true,
    ));
    Ok(Some(PolarsFrame { df, columns }))
}

fn literal_polars_column(name: &str, value: &DataValue, rows: usize) -> Result<Option<Column>> {
    Ok(Some(match value {
        DataValue::Int(value) => Column::new(name.into(), vec![Some(*value); rows]),
        DataValue::Float(value) => Column::new(name.into(), vec![Some(*value); rows]),
        DataValue::Text(value) => Column::new(name.into(), vec![Some(value.as_str()); rows]),
        DataValue::Bool(value) => Column::new(name.into(), vec![Some(*value); rows]),
        DataValue::Null => return Ok(None),
        _ => return Ok(None),
    }))
}

fn is_polars_supported_type(column_type: ColumnType) -> bool {
    matches!(
        column_type,
        ColumnType::Int | ColumnType::Float | ColumnType::Text | ColumnType::Bool
    )
}

fn union_polars(left: &PolarsFrame, right: &PolarsFrame) -> Result<PolarsFrame> {
    if left.columns != right.columns {
        return Err(LoomError::invalid("dataframe union schema mismatch"));
    }
    Ok(PolarsFrame {
        df: left.df.vstack(&right.df).map_err(polars_err)?,
        columns: left.columns.clone(),
    })
}

fn batch_to_polars(batch: &DataframeBatch) -> Result<Option<PolarsFrame>> {
    let mut columns = Vec::with_capacity(batch.columns.len());
    for (index, column) in batch.columns.iter().enumerate() {
        columns.push(match column.column_type {
            ColumnType::Int => {
                let values = batch
                    .rows
                    .iter()
                    .map(|row| match &row[index] {
                        DataValue::Null => Ok(None),
                        DataValue::Int(value) => Ok(Some(*value)),
                        _ => Err(type_mismatch(column)),
                    })
                    .collect::<Result<Vec<_>>>()?;
                Column::new(column.name.clone().into(), values)
            }
            ColumnType::Float => {
                let values = batch
                    .rows
                    .iter()
                    .map(|row| match &row[index] {
                        DataValue::Null => Ok(None),
                        DataValue::Float(value) => Ok(Some(*value)),
                        _ => Err(type_mismatch(column)),
                    })
                    .collect::<Result<Vec<_>>>()?;
                Column::new(column.name.clone().into(), values)
            }
            ColumnType::Text => {
                let values = batch
                    .rows
                    .iter()
                    .map(|row| match &row[index] {
                        DataValue::Null => Ok(None),
                        DataValue::Text(value) => Ok(Some(value.as_str())),
                        _ => Err(type_mismatch(column)),
                    })
                    .collect::<Result<Vec<_>>>()?;
                Column::new(column.name.clone().into(), values)
            }
            ColumnType::Bool => {
                let values = batch
                    .rows
                    .iter()
                    .map(|row| match &row[index] {
                        DataValue::Null => Ok(None),
                        DataValue::Bool(value) => Ok(Some(*value)),
                        _ => Err(type_mismatch(column)),
                    })
                    .collect::<Result<Vec<_>>>()?;
                Column::new(column.name.clone().into(), values)
            }
            _ => return Ok(None),
        });
    }
    Ok(Some(PolarsFrame {
        df: DataFrame::new_infer_height(columns).map_err(polars_err)?,
        columns: batch.columns.clone(),
    }))
}

fn polars_to_batch(frame: PolarsFrame) -> Result<DataframeBatch> {
    polars_frame_to_batch(&frame)
}

fn polars_frame_to_batch(frame: &PolarsFrame) -> Result<DataframeBatch> {
    let mut rows = Vec::with_capacity(frame.df.height());
    for row_index in 0..frame.df.height() {
        let values = frame
            .df
            .get(row_index)
            .ok_or_else(|| LoomError::invalid("Polars row index missing"))?;
        rows.push(
            values
                .into_iter()
                .map(polars_value)
                .collect::<Result<Vec<_>>>()?,
        );
    }
    DataframeBatch::new(frame.columns.clone(), rows)
}

fn polars_value(value: AnyValue<'_>) -> Result<DataValue> {
    Ok(match value {
        AnyValue::Null => DataValue::Null,
        AnyValue::Boolean(value) => DataValue::Bool(value),
        AnyValue::Int64(value) => DataValue::Int(value),
        AnyValue::Float64(value) => DataValue::Float(value),
        AnyValue::String(value) => DataValue::Text(value.to_string()),
        AnyValue::StringOwned(value) => DataValue::Text(value.to_string()),
        other => {
            return Err(LoomError::new(
                Code::Unsupported,
                format!("unsupported Polars dataframe value {other:?}"),
            ));
        }
    })
}

fn type_mismatch(column: &DataframeColumn) -> LoomError {
    LoomError::invalid(format!(
        "dataframe column {:?} does not match {:?}",
        column.name, column.column_type
    ))
}

fn polars_err(error: PolarsError) -> LoomError {
    LoomError::invalid(format!("Polars dataframe execution failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_dataframe::{
        DataframeAggregation, DataframeInputFormat, DataframeSourceBinding, DataframeSourceKind,
    };

    #[test]
    fn polars_executor_runs_supported_cast_projection_union_with_column_sort_and_limit() {
        let batch = DataframeBatch::new(
            vec![
                DataframeColumn::new("tenant", ColumnType::Text, false),
                DataframeColumn::new("score", ColumnType::Float, false),
                DataframeColumn::new("active", ColumnType::Bool, false),
            ],
            vec![
                vec![
                    DataValue::Text("a".into()),
                    DataValue::Float(2.0),
                    DataValue::Bool(true),
                ],
                vec![
                    DataValue::Text("b".into()),
                    DataValue::Float(5.0),
                    DataValue::Bool(false),
                ],
                vec![
                    DataValue::Text("c".into()),
                    DataValue::Float(3.0),
                    DataValue::Bool(true),
                ],
            ],
        )
        .unwrap();
        let extra = DataframeBatch::new(
            vec![
                DataframeColumn::new("tenant", ColumnType::Text, false),
                DataframeColumn::new("score", ColumnType::Float, false),
                DataframeColumn::new("active", ColumnType::Bool, false),
                DataframeColumn::new("label", ColumnType::Text, true),
                DataframeColumn::new("rank_text", ColumnType::Int, true),
            ],
            vec![vec![
                DataValue::Text("d".into()),
                DataValue::Float(4.0),
                DataValue::Bool(true),
                DataValue::Text("extra".into()),
                DataValue::Int(7),
            ]],
        )
        .unwrap();
        let plan = DataframePlan::new(vec![
            DataframeSourceBinding::new(
                "scores",
                DataframeSourceKind::Cas,
                "unused",
                DataframeInputFormat::Csv,
            ),
            DataframeSourceBinding::new(
                "extra",
                DataframeSourceKind::Cas,
                "unused",
                DataframeInputFormat::Csv,
            ),
        ])
        .unwrap()
        .with_operations(vec![
            DataframeOperation::Scan {
                source: "scores".into(),
            },
            DataframeOperation::WithColumn {
                column: "label".into(),
                expression: "\"native\"".into(),
            },
            DataframeOperation::WithColumn {
                column: "rank_text".into(),
                expression: "\"7\"".into(),
            },
            DataframeOperation::Cast {
                column: "rank_text".into(),
                column_type: ColumnType::Int,
            },
            DataframeOperation::Union {
                source: "extra".into(),
            },
            DataframeOperation::Select {
                columns: vec![
                    "tenant".into(),
                    "score".into(),
                    "label".into(),
                    "rank_text".into(),
                ],
            },
            DataframeOperation::Rename {
                from: "score".into(),
                to: "points".into(),
            },
            DataframeOperation::Sort {
                columns: vec!["points".into()],
                descending: true,
            },
            DataframeOperation::Limit { rows: 3 },
        ])
        .unwrap();
        let sources = BTreeMap::from([("scores".to_string(), batch), ("extra".to_string(), extra)]);

        let report = polars_dataframe_execution_report(&plan, &sources).unwrap();
        assert_eq!(report.mode, PolarsDataframeExecutionMode::Native);
        assert_eq!(report.reason, None);

        let out = POLARS_DATAFRAME_EXECUTOR.execute(&plan, &sources).unwrap();
        assert_eq!(
            out.columns,
            vec![
                DataframeColumn::new("tenant", ColumnType::Text, false),
                DataframeColumn::new("points", ColumnType::Float, false),
                DataframeColumn::new("label", ColumnType::Text, true),
                DataframeColumn::new("rank_text", ColumnType::Int, true),
            ]
        );
        assert_eq!(
            out.rows,
            vec![
                vec![
                    DataValue::Text("b".into()),
                    DataValue::Float(5.0),
                    DataValue::Text("native".into()),
                    DataValue::Int(7)
                ],
                vec![
                    DataValue::Text("d".into()),
                    DataValue::Float(4.0),
                    DataValue::Text("extra".into()),
                    DataValue::Int(7)
                ],
                vec![
                    DataValue::Text("c".into()),
                    DataValue::Float(3.0),
                    DataValue::Text("native".into()),
                    DataValue::Int(7)
                ],
            ]
        );
    }

    #[test]
    fn polars_executor_falls_back_for_portable_aggregate() {
        let batch = DataframeBatch::new(
            vec![
                DataframeColumn::new("tenant", ColumnType::Text, false),
                DataframeColumn::new("score", ColumnType::Float, false),
            ],
            vec![
                vec![DataValue::Text("a".into()), DataValue::Float(2.0)],
                vec![DataValue::Text("a".into()), DataValue::Float(3.0)],
                vec![DataValue::Text("b".into()), DataValue::Float(5.0)],
            ],
        )
        .unwrap();
        let plan = DataframePlan::new(vec![DataframeSourceBinding::new(
            "scores",
            DataframeSourceKind::Cas,
            "unused",
            DataframeInputFormat::Csv,
        )])
        .unwrap()
        .with_operations(vec![
            DataframeOperation::Scan {
                source: "scores".into(),
            },
            DataframeOperation::Aggregate {
                group_by: vec!["tenant".into()],
                aggregations: vec![DataframeAggregation::new(
                    "score_sum",
                    "sum",
                    Some("score".into()),
                )],
            },
        ])
        .unwrap();
        let sources = BTreeMap::from([("scores".to_string(), batch)]);

        let report = polars_dataframe_execution_report(&plan, &sources).unwrap();
        assert_eq!(
            report,
            PolarsDataframeExecutionReport {
                mode: PolarsDataframeExecutionMode::PortableFallback,
                reason: Some("unsupported dataframe operation"),
            }
        );

        let out = POLARS_DATAFRAME_EXECUTOR.execute(&plan, &sources).unwrap();
        assert_eq!(out.row_count(), 2);
        assert_eq!(
            out.rows[0],
            vec![DataValue::Text("a".into()), DataValue::Float(5.0)]
        );
    }
}
