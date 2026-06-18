//! Native Arrow IPC and Parquet projections for the columnar facet.

use crate::ColumnarSet;
use arrow_array::{
    Array, ArrayRef, BinaryArray, BooleanArray, Date32Array, Float32Array, Float64Array, Int8Array,
    Int16Array, Int32Array, Int64Array, RecordBatch, StringArray, Time64NanosecondArray,
    TimestampMicrosecondArray, UInt8Array, UInt16Array, UInt32Array, UInt64Array,
};
use arrow_ipc::MetadataVersion;
use arrow_ipc::reader::StreamReader;
use arrow_ipc::writer::{IpcWriteOptions, StreamWriter};
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use bytes::Bytes;
use loom_types::error::{LoomError, Result};
use loom_types::tabular::{ColumnType, Value, cell_from, encode_cell};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::arrow_writer::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

const LOOM_COLUMN_TYPE_METADATA: &str = "loom.column_type";
const LOOM_PARQUET_CREATED_BY: &str = "uldren-loom-columnar-v1";

pub fn columnar_to_arrow_ipc(dataset: &ColumnarSet) -> Result<Vec<u8>> {
    let batch = to_record_batch(dataset)?;
    let options = IpcWriteOptions::try_new(8, false, MetadataVersion::V5).map_err(arrow_error)?;
    let mut out = Vec::new();
    let mut writer =
        StreamWriter::try_new_with_options(&mut out, batch.schema_ref().as_ref(), options)
            .map_err(arrow_error)?;
    writer.write(&batch).map_err(arrow_error)?;
    writer.finish().map_err(arrow_error)?;
    drop(writer);
    Ok(out)
}

pub fn columnar_from_arrow_ipc(bytes: &[u8], target_segment_rows: usize) -> Result<ColumnarSet> {
    let reader = StreamReader::try_new(Cursor::new(bytes), None).map_err(arrow_error)?;
    let mut dataset = None;
    for batch in reader {
        append_batch(
            &mut dataset,
            batch.map_err(arrow_error)?,
            target_segment_rows,
        )?;
    }
    dataset.ok_or_else(|| LoomError::invalid("arrow IPC stream has no record batches"))
}

pub fn columnar_to_parquet(dataset: &ColumnarSet) -> Result<Vec<u8>> {
    let batch = to_record_batch(dataset)?;
    let properties = WriterProperties::builder()
        .set_created_by(LOOM_PARQUET_CREATED_BY.to_owned())
        .set_compression(Compression::UNCOMPRESSED)
        .build();
    let mut out = Vec::new();
    let mut writer =
        ArrowWriter::try_new(&mut out, batch.schema(), Some(properties)).map_err(parquet_error)?;
    writer.write(&batch).map_err(parquet_error)?;
    writer.close().map_err(parquet_error)?;
    Ok(out)
}

pub fn columnar_from_parquet(bytes: &[u8], target_segment_rows: usize) -> Result<ColumnarSet> {
    let reader = ParquetRecordBatchReaderBuilder::try_new(Bytes::copy_from_slice(bytes))
        .map_err(parquet_error)?
        .with_batch_size(target_segment_rows.max(1))
        .build()
        .map_err(parquet_error)?;
    let mut dataset = None;
    for batch in reader {
        append_batch(
            &mut dataset,
            batch.map_err(arrow_error)?,
            target_segment_rows,
        )?;
    }
    dataset.ok_or_else(|| LoomError::invalid("parquet file has no record batches"))
}

fn to_record_batch(dataset: &ColumnarSet) -> Result<RecordBatch> {
    let rows = dataset.scan().collect::<Vec<_>>();
    let fields = dataset
        .columns()
        .iter()
        .map(|(name, ty)| arrow_field(name, *ty))
        .collect::<Result<Vec<_>>>()?;
    let arrays = dataset
        .columns()
        .iter()
        .enumerate()
        .map(|(index, (_, ty))| column_to_array(*ty, &rows, index))
        .collect::<Result<Vec<_>>>()?;
    RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).map_err(arrow_error)
}

fn arrow_field(name: &str, ty: ColumnType) -> Result<Field> {
    let data_type = column_type_to_arrow(ty)?;
    let metadata = HashMap::from([(LOOM_COLUMN_TYPE_METADATA.to_owned(), ty.tag().to_string())]);
    Ok(Field::new(name, data_type, true).with_metadata(metadata))
}

fn column_type_to_arrow(ty: ColumnType) -> Result<DataType> {
    match ty {
        ColumnType::Bool => Ok(DataType::Boolean),
        ColumnType::Int => Ok(DataType::Int64),
        ColumnType::Float => Ok(DataType::Float64),
        ColumnType::Text => Ok(DataType::Utf8),
        ColumnType::Bytes => Ok(DataType::Binary),
        ColumnType::I8 => Ok(DataType::Int8),
        ColumnType::I16 => Ok(DataType::Int16),
        ColumnType::I32 => Ok(DataType::Int32),
        ColumnType::U8 => Ok(DataType::UInt8),
        ColumnType::U16 => Ok(DataType::UInt16),
        ColumnType::U32 => Ok(DataType::UInt32),
        ColumnType::U64 => Ok(DataType::UInt64),
        ColumnType::F32 => Ok(DataType::Float32),
        ColumnType::Date => Ok(DataType::Date32),
        ColumnType::Time => Ok(DataType::Time64(TimeUnit::Nanosecond)),
        ColumnType::Timestamp => Ok(DataType::Timestamp(TimeUnit::Microsecond, None)),
        ColumnType::I128
        | ColumnType::U128
        | ColumnType::Decimal
        | ColumnType::Interval
        | ColumnType::Uuid
        | ColumnType::Inet
        | ColumnType::Point
        | ColumnType::List
        | ColumnType::Map => Ok(DataType::Binary),
    }
}

fn column_to_array(ty: ColumnType, rows: &[&Vec<Value>], index: usize) -> Result<ArrayRef> {
    match ty {
        ColumnType::Bool => Ok(Arc::new(BooleanArray::from(
            rows.iter()
                .map(|row| match &row[index] {
                    Value::Null => Ok(None),
                    Value::Bool(value) => Ok(Some(*value)),
                    _ => Err(type_error(ty)),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        ColumnType::Int => Ok(Arc::new(Int64Array::from(
            rows.iter()
                .map(|row| match &row[index] {
                    Value::Null => Ok(None),
                    Value::Int(value) => Ok(Some(*value)),
                    _ => Err(type_error(ty)),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        ColumnType::Float => Ok(Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| match &row[index] {
                    Value::Null => Ok(None),
                    Value::Float(value) => Ok(Some(*value)),
                    _ => Err(type_error(ty)),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        ColumnType::Text => Ok(Arc::new(StringArray::from(
            rows.iter()
                .map(|row| match &row[index] {
                    Value::Null => Ok(None),
                    Value::Text(value) => Ok(Some(value.as_str())),
                    _ => Err(type_error(ty)),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        ColumnType::Bytes => {
            let values = rows
                .iter()
                .map(|row| match &row[index] {
                    Value::Null => Ok(None),
                    Value::Bytes(value) => Ok(Some(value.as_slice())),
                    _ => Err(type_error(ty)),
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Arc::new(BinaryArray::from_iter(values)))
        }
        ColumnType::I8 => Ok(Arc::new(Int8Array::from(
            rows.iter()
                .map(|row| match &row[index] {
                    Value::Null => Ok(None),
                    Value::I8(value) => Ok(Some(*value)),
                    _ => Err(type_error(ty)),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        ColumnType::I16 => Ok(Arc::new(Int16Array::from(
            rows.iter()
                .map(|row| match &row[index] {
                    Value::Null => Ok(None),
                    Value::I16(value) => Ok(Some(*value)),
                    _ => Err(type_error(ty)),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        ColumnType::I32 => Ok(Arc::new(Int32Array::from(
            rows.iter()
                .map(|row| match &row[index] {
                    Value::Null => Ok(None),
                    Value::I32(value) => Ok(Some(*value)),
                    _ => Err(type_error(ty)),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        ColumnType::U8 => Ok(Arc::new(UInt8Array::from(
            rows.iter()
                .map(|row| match &row[index] {
                    Value::Null => Ok(None),
                    Value::U8(value) => Ok(Some(*value)),
                    _ => Err(type_error(ty)),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        ColumnType::U16 => Ok(Arc::new(UInt16Array::from(
            rows.iter()
                .map(|row| match &row[index] {
                    Value::Null => Ok(None),
                    Value::U16(value) => Ok(Some(*value)),
                    _ => Err(type_error(ty)),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        ColumnType::U32 => Ok(Arc::new(UInt32Array::from(
            rows.iter()
                .map(|row| match &row[index] {
                    Value::Null => Ok(None),
                    Value::U32(value) => Ok(Some(*value)),
                    _ => Err(type_error(ty)),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        ColumnType::U64 => Ok(Arc::new(UInt64Array::from(
            rows.iter()
                .map(|row| match &row[index] {
                    Value::Null => Ok(None),
                    Value::U64(value) => Ok(Some(*value)),
                    _ => Err(type_error(ty)),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        ColumnType::F32 => Ok(Arc::new(Float32Array::from(
            rows.iter()
                .map(|row| match &row[index] {
                    Value::Null => Ok(None),
                    Value::F32(value) => Ok(Some(*value)),
                    _ => Err(type_error(ty)),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        ColumnType::Date => Ok(Arc::new(Date32Array::from(
            rows.iter()
                .map(|row| match &row[index] {
                    Value::Null => Ok(None),
                    Value::Date(value) => Ok(Some(*value)),
                    _ => Err(type_error(ty)),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        ColumnType::Time => Ok(Arc::new(Time64NanosecondArray::from(
            rows.iter()
                .map(|row| match &row[index] {
                    Value::Null => Ok(None),
                    Value::Time(value) => i64::try_from(*value)
                        .map(Some)
                        .map_err(|_| LoomError::invalid("time value exceeds Arrow Time64 range")),
                    _ => Err(type_error(ty)),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        ColumnType::Timestamp => Ok(Arc::new(TimestampMicrosecondArray::from(
            rows.iter()
                .map(|row| match &row[index] {
                    Value::Null => Ok(None),
                    Value::Timestamp(value) => Ok(Some(*value)),
                    _ => Err(type_error(ty)),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        ColumnType::I128
        | ColumnType::U128
        | ColumnType::Decimal
        | ColumnType::Interval
        | ColumnType::Uuid
        | ColumnType::Inet
        | ColumnType::Point
        | ColumnType::List
        | ColumnType::Map => {
            let values = rows
                .iter()
                .map(|row| {
                    let value = &row[index];
                    if matches!(value, Value::Null) {
                        Ok(None)
                    } else if value.matches(ty) {
                        Ok(Some(encode_cell(value)))
                    } else {
                        Err(type_error(ty))
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Arc::new(BinaryArray::from_iter(values)))
        }
    }
}

fn append_batch(
    dataset: &mut Option<ColumnarSet>,
    batch: RecordBatch,
    target_segment_rows: usize,
) -> Result<()> {
    let columns = batch
        .schema()
        .fields()
        .iter()
        .map(|field| {
            let ty = match field.metadata().get(LOOM_COLUMN_TYPE_METADATA) {
                Some(tag) => {
                    let tag = tag
                        .parse::<u8>()
                        .map_err(|_| LoomError::invalid("invalid Loom column type metadata"))?;
                    ColumnType::from_tag(tag)?
                }
                None => arrow_to_column_type(field.data_type())?,
            };
            Ok((field.name().clone(), ty))
        })
        .collect::<Result<Vec<_>>>()?;
    if dataset.is_none() {
        *dataset = Some(ColumnarSet::new(columns.clone(), target_segment_rows)?);
    }
    if dataset.as_ref().map(ColumnarSet::columns) != Some(columns.as_slice()) {
        return Err(LoomError::invalid(
            "arrow record batch schema changed across batches",
        ));
    }
    for row_index in 0..batch.num_rows() {
        let row = columns
            .iter()
            .enumerate()
            .map(|(column_index, (_, ty))| value_at(batch.column(column_index), *ty, row_index))
            .collect::<Result<Vec<_>>>()?;
        dataset
            .as_mut()
            .expect("dataset initialized")
            .append_row(row)?;
    }
    Ok(())
}

fn arrow_to_column_type(data_type: &DataType) -> Result<ColumnType> {
    match data_type {
        DataType::Boolean => Ok(ColumnType::Bool),
        DataType::Int8 => Ok(ColumnType::I8),
        DataType::Int16 => Ok(ColumnType::I16),
        DataType::Int32 => Ok(ColumnType::I32),
        DataType::Int64 => Ok(ColumnType::Int),
        DataType::UInt8 => Ok(ColumnType::U8),
        DataType::UInt16 => Ok(ColumnType::U16),
        DataType::UInt32 => Ok(ColumnType::U32),
        DataType::UInt64 => Ok(ColumnType::U64),
        DataType::Float32 => Ok(ColumnType::F32),
        DataType::Float64 => Ok(ColumnType::Float),
        DataType::Utf8 => Ok(ColumnType::Text),
        DataType::Binary => Ok(ColumnType::Bytes),
        DataType::Date32 => Ok(ColumnType::Date),
        DataType::Time64(TimeUnit::Nanosecond) => Ok(ColumnType::Time),
        DataType::Timestamp(TimeUnit::Microsecond, None) => Ok(ColumnType::Timestamp),
        _ => Err(LoomError::unsupported(format!(
            "Arrow data type {data_type:?} is not a promoted Loom columnar type"
        ))),
    }
}

fn value_at(array: &ArrayRef, ty: ColumnType, index: usize) -> Result<Value> {
    if array.is_null(index) {
        return Ok(Value::Null);
    }
    match ty {
        ColumnType::Bool => {
            typed_value::<BooleanArray, _>(array, |array| Value::Bool(array.value(index)))
        }
        ColumnType::Int => {
            typed_value::<Int64Array, _>(array, |array| Value::Int(array.value(index)))
        }
        ColumnType::Float => {
            typed_value::<Float64Array, _>(array, |array| Value::Float(array.value(index)))
        }
        ColumnType::Text => {
            typed_value::<StringArray, _>(array, |array| Value::Text(array.value(index).to_owned()))
        }
        ColumnType::Bytes => {
            typed_value::<BinaryArray, _>(array, |array| Value::Bytes(array.value(index).to_vec()))
        }
        ColumnType::I8 => typed_value::<Int8Array, _>(array, |array| Value::I8(array.value(index))),
        ColumnType::I16 => {
            typed_value::<Int16Array, _>(array, |array| Value::I16(array.value(index)))
        }
        ColumnType::I32 => {
            typed_value::<Int32Array, _>(array, |array| Value::I32(array.value(index)))
        }
        ColumnType::U8 => {
            typed_value::<UInt8Array, _>(array, |array| Value::U8(array.value(index)))
        }
        ColumnType::U16 => {
            typed_value::<UInt16Array, _>(array, |array| Value::U16(array.value(index)))
        }
        ColumnType::U32 => {
            typed_value::<UInt32Array, _>(array, |array| Value::U32(array.value(index)))
        }
        ColumnType::U64 => {
            typed_value::<UInt64Array, _>(array, |array| Value::U64(array.value(index)))
        }
        ColumnType::F32 => {
            typed_value::<Float32Array, _>(array, |array| Value::F32(array.value(index)))
        }
        ColumnType::Date => {
            typed_value::<Date32Array, _>(array, |array| Value::Date(array.value(index)))
        }
        ColumnType::Time => {
            let array = array
                .as_any()
                .downcast_ref::<Time64NanosecondArray>()
                .ok_or_else(|| {
                    LoomError::invalid("Arrow array type does not match Loom column type")
                })?;
            u64::try_from(array.value(index))
                .map(Value::Time)
                .map_err(|_| LoomError::invalid("negative Arrow Time64 value"))
        }
        ColumnType::Timestamp => typed_value::<TimestampMicrosecondArray, _>(array, |array| {
            Value::Timestamp(array.value(index))
        }),
        ColumnType::I128
        | ColumnType::U128
        | ColumnType::Decimal
        | ColumnType::Interval
        | ColumnType::Uuid
        | ColumnType::Inet
        | ColumnType::Point
        | ColumnType::List
        | ColumnType::Map => {
            let array = array
                .as_any()
                .downcast_ref::<BinaryArray>()
                .ok_or_else(|| {
                    LoomError::invalid("Arrow array type does not match Loom column type")
                })?;
            let value = loom_codec::decode(array.value(index))
                .map_err(|error| LoomError::corrupt(format!("CBOR decode failed: {error}")))
                .and_then(cell_from)?;
            if value.matches(ty) {
                Ok(value)
            } else {
                Err(type_error(ty))
            }
        }
    }
}

fn typed_value<A, F>(array: &ArrayRef, f: F) -> Result<Value>
where
    A: Array + 'static,
    F: FnOnce(&A) -> Value,
{
    array
        .as_any()
        .downcast_ref::<A>()
        .map(f)
        .ok_or_else(|| LoomError::invalid("Arrow array type does not match Loom column type"))
}

fn type_error(ty: ColumnType) -> LoomError {
    LoomError::invalid(format!("row value does not match {ty:?} column"))
}

fn arrow_error(error: arrow_schema::ArrowError) -> LoomError {
    LoomError::invalid(format!("Arrow columnar projection failed: {error}"))
}

fn parquet_error(error: parquet::errors::ParquetError) -> LoomError {
    LoomError::invalid(format!("Parquet columnar projection failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::net::{IpAddr, Ipv4Addr};

    fn dataset() -> ColumnarSet {
        let mut dataset = ColumnarSet::new(
            vec![
                ("id".to_owned(), ColumnType::Int),
                ("name".to_owned(), ColumnType::Text),
                ("active".to_owned(), ColumnType::Bool),
                ("payload".to_owned(), ColumnType::Bytes),
                ("score".to_owned(), ColumnType::Float),
                ("created".to_owned(), ColumnType::Timestamp),
            ],
            2,
        )
        .unwrap();
        dataset
            .append_row(vec![
                Value::Int(1),
                Value::Text("alpha".to_owned()),
                Value::Bool(true),
                Value::Bytes(vec![1, 2, 3]),
                Value::Float(1.5),
                Value::Timestamp(1_700_000_000_000_000),
            ])
            .unwrap();
        dataset
            .append_row(vec![
                Value::Int(2),
                Value::Null,
                Value::Bool(false),
                Value::Bytes(vec![]),
                Value::Float(f64::NAN),
                Value::Timestamp(1_700_000_000_000_100),
            ])
            .unwrap();
        dataset
    }

    #[test]
    fn arrow_ipc_round_trips_supported_scalars() {
        let dataset = dataset();
        let bytes = columnar_to_arrow_ipc(&dataset).unwrap();
        let round_trip = columnar_from_arrow_ipc(&bytes, dataset.target_segment_rows()).unwrap();
        assert_eq!(round_trip.columns(), dataset.columns());
        assert_eq!(
            round_trip.scan().cloned().collect::<Vec<_>>(),
            dataset.scan().cloned().collect::<Vec<_>>()
        );
    }

    #[test]
    fn parquet_round_trips_and_is_deterministic() {
        let dataset = dataset();
        let one = columnar_to_parquet(&dataset).unwrap();
        let two = columnar_to_parquet(&dataset).unwrap();
        assert_eq!(one, two);
        let round_trip = columnar_from_parquet(&one, dataset.target_segment_rows()).unwrap();
        assert_eq!(round_trip.columns(), dataset.columns());
        assert_eq!(
            round_trip.scan().cloned().collect::<Vec<_>>(),
            dataset.scan().cloned().collect::<Vec<_>>()
        );
    }

    fn extended_dataset() -> ColumnarSet {
        let mut dataset = ColumnarSet::new(
            vec![
                ("signed".to_owned(), ColumnType::I128),
                ("unsigned".to_owned(), ColumnType::U128),
                ("amount".to_owned(), ColumnType::Decimal),
                ("duration".to_owned(), ColumnType::Interval),
                ("uuid".to_owned(), ColumnType::Uuid),
                ("addr".to_owned(), ColumnType::Inet),
                ("point".to_owned(), ColumnType::Point),
                ("items".to_owned(), ColumnType::List),
                ("attrs".to_owned(), ColumnType::Map),
            ],
            2,
        )
        .unwrap();
        dataset
            .append_row(vec![
                Value::I128(-(1i128 << 100)),
                Value::U128(u128::MAX),
                Value::Decimal {
                    mantissa: 123_456,
                    scale: 3,
                },
                Value::Interval {
                    months: 14,
                    micros: -250,
                },
                Value::Uuid(0x1234_5678_9abc_def0_1122_3344_5566_7788),
                Value::Inet(IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1))),
                Value::Point { x: 1.0, y: -2.0 },
                Value::List(vec![Value::Int(7), Value::Text("seven".to_owned())]),
                Value::Map(BTreeMap::from([("k".to_owned(), Value::Bool(true))])),
            ])
            .unwrap();
        dataset.append_row(vec![Value::Null; 9]).unwrap();
        dataset
    }

    #[test]
    fn extended_logical_types_round_trip_as_exact_metadata_carriers() {
        let dataset = extended_dataset();
        let arrow = columnar_to_arrow_ipc(&dataset).unwrap();
        let arrow_round_trip =
            columnar_from_arrow_ipc(&arrow, dataset.target_segment_rows()).unwrap();
        assert_eq!(arrow_round_trip.columns(), dataset.columns());
        assert_eq!(
            arrow_round_trip.scan().cloned().collect::<Vec<_>>(),
            dataset.scan().cloned().collect::<Vec<_>>()
        );

        let parquet = columnar_to_parquet(&dataset).unwrap();
        let parquet_round_trip =
            columnar_from_parquet(&parquet, dataset.target_segment_rows()).unwrap();
        assert_eq!(parquet_round_trip.columns(), dataset.columns());
        assert_eq!(
            parquet_round_trip.scan().cloned().collect::<Vec<_>>(),
            dataset.scan().cloned().collect::<Vec<_>>()
        );
    }
}
