#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use loom_native::NativeLibrary;
use loom_types::Code;

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("loom-native-{name}-{}-{nanos}", std::process::id()))
}

fn fixture_library(dir: &Path) -> Option<PathBuf> {
    fs::create_dir_all(dir).unwrap();
    let source = dir.join("fixture.c");
    fs::write(
        &source,
        b"#include <stdint.h>\nuint32_t loom_native_fixture_abi_version(void) { return 7u; }\nconst char *loom_native_fixture_runtime_info(void) { return \"{\\\"schema_version\\\":1}\"; }\n",
    )
    .unwrap();
    let output = if cfg!(target_os = "macos") {
        dir.join("libloom_native_fixture.dylib")
    } else {
        dir.join("libloom_native_fixture.so")
    };
    let mut command = Command::new("cc");
    if cfg!(target_os = "macos") {
        command.arg("-dynamiclib");
    } else {
        command.args(["-shared", "-fPIC"]);
    }
    let output_result = command.arg(&source).arg("-o").arg(&output).output();
    let output_result = match output_result {
        Ok(output_result) => output_result,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => panic!("failed to run cc: {error}"),
    };
    assert!(
        output_result.status.success(),
        "cc failed: {}",
        String::from_utf8_lossy(&output_result.stderr)
    );
    Some(output)
}

#[test]
fn loads_fixture_library_and_calls_u32_symbol() {
    let dir = temp_dir("load");
    let Some(path) = fixture_library(&dir) else {
        return;
    };

    let library = NativeLibrary::open(&path).unwrap();
    library
        .require_symbol(c"loom_native_fixture_abi_version")
        .unwrap();
    let version = library
        .load_u32_function(c"loom_native_fixture_abi_version")
        .unwrap();
    let runtime_info = library
        .load_static_utf8_function(c"loom_native_fixture_runtime_info")
        .unwrap();

    assert_eq!(library.path(), path.as_path());
    assert_eq!(version.name(), "loom_native_fixture_abi_version");
    assert_eq!(version.call(), 7);
    assert_eq!(runtime_info.name(), "loom_native_fixture_runtime_info");
    assert_eq!(
        runtime_info.call_string().unwrap(),
        "{\"schema_version\":1}"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn reports_missing_required_symbol() {
    let dir = temp_dir("missing-symbol");
    let Some(path) = fixture_library(&dir) else {
        return;
    };

    let library = NativeLibrary::open(&path).unwrap();
    let error = library
        .require_symbol(c"loom_native_fixture_missing")
        .unwrap_err();

    assert_eq!(error.code, Code::Unsupported);
    assert!(error.message.contains("loom_native_fixture_missing"));
    fs::remove_dir_all(dir).unwrap();
}
