use std::path::Path;

use loom_codec::{Value as CborValue, encode as cbor_encode};
use loom_interchange::{ArchiveKind, ExportReport};
use loom_interchange_io::{
    ArchiveExportOptions, ArchiveImportOptions, CarExportOptions, CarImportOptions,
    export_archive_bytes, export_car_bytes, import_archive_bytes, import_car_bytes,
};
use wasm_bindgen::prelude::*;

use super::{LoomStore, le, resolve_workspace_arg, save_loom};

fn parse_archive_kind(kind: &str) -> Result<ArchiveKind, JsError> {
    match kind {
        "zip" => Ok(ArchiveKind::Zip),
        "tar" => Ok(ArchiveKind::Tar),
        "tar-gzip" | "tar.gz" | "tgz" => Ok(ArchiveKind::TarGzip),
        "gzip" | "gz" => Ok(ArchiveKind::Gzip),
        other => Err(JsError::new(&format!(
            "unsupported archive kind {other:?}; expected tar, tar-gzip, zip, or gzip"
        ))),
    }
}

fn export_package(bytes: Vec<u8>, report: ExportReport) -> Result<Vec<u8>, JsError> {
    cbor_encode(&CborValue::Array(vec![
        CborValue::Bytes(bytes),
        report.to_value(),
    ]))
    .map_err(|e| JsError::new(&format!("encode export package: {e}")))
}

#[wasm_bindgen]
impl LoomStore {
    pub fn archive_import_bytes(
        &mut self,
        workspace: String,
        source_name: String,
        kind: String,
        bytes: Vec<u8>,
        dry_run: bool,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let archive_kind = parse_archive_kind(&kind)?;
        let mut options = ArchiveImportOptions::new(&source_name);
        options.archive_id = source_name.clone();
        options.dry_run = dry_run;
        let result = import_archive_bytes(
            &mut self.loom,
            ns,
            &bytes,
            Path::new(&source_name),
            archive_kind,
            &options,
        )
        .map_err(le)?;
        if !dry_run {
            save_loom(&mut self.loom).map_err(le)?;
        }
        result.report.encode().map_err(le)
    }

    pub fn archive_export_bytes(
        &self,
        workspace: String,
        destination_name: String,
        kind: String,
        revision: Option<String>,
        dry_run: bool,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let archive_kind = parse_archive_kind(&kind)?;
        let mut options = ArchiveExportOptions::new(destination_name);
        options.revision = revision;
        options.dry_run = dry_run;
        let result = export_archive_bytes(&self.loom, ns, archive_kind, &options).map_err(le)?;
        export_package(result.bytes, result.report)
    }

    pub fn car_import_bytes(&mut self, bytes: Vec<u8>, dry_run: bool) -> Result<Vec<u8>, JsError> {
        let mut options = CarImportOptions::new("wasm-car-bytes");
        options.dry_run = dry_run;
        let result = import_car_bytes(&mut self.loom, &bytes, &options).map_err(le)?;
        if !dry_run {
            save_loom(&mut self.loom).map_err(le)?;
        }
        result.report.encode().map_err(le)
    }

    pub fn car_export_bytes(
        &self,
        workspace: String,
        destination_name: String,
        dry_run: bool,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let mut options = CarExportOptions::new(destination_name);
        options.dry_run = dry_run;
        let result = export_car_bytes(&self.loom, ns, &options).map_err(le)?;
        export_package(result.bytes, result.report)
    }
}
