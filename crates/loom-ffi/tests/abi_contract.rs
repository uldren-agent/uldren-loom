use std::ffi::{CStr, CString, c_char, c_uchar, c_void};
use std::path::PathBuf;

use libloading::Library;
use loom_codec::Value as CborValue;
use loom_core::tabular::Value;
use loom_core::{Code, WorkspaceId};

const LOOM_VALUE_INT: i32 = 2;
const LOOM_VALUE_TEXT: i32 = 4;

#[repr(C)]
struct LoomValue {
    tag: i32,
    scale: u32,
    int_val: i64,
    int_val2: i64,
    uint_val: u64,
    float_val: f64,
    float_val2: f64,
    bits: u64,
    bits2: u64,
    bytes16: [u8; 16],
    data: *const c_uchar,
    data_len: usize,
}

impl LoomValue {
    fn zeroed() -> Self {
        Self {
            tag: 0,
            scale: 0,
            int_val: 0,
            int_val2: 0,
            uint_val: 0,
            float_val: 0.0,
            float_val2: 0.0,
            bits: 0,
            bits2: 0,
            bytes16: [0; 16],
            data: std::ptr::null(),
            data_len: 0,
        }
    }
}

fn library_path() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target = manifest
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .join("target")
        .join("debug");
    let name = if cfg!(target_os = "macos") {
        "libuldren_loom.dylib"
    } else if cfg!(target_os = "windows") {
        "uldren_loom.dll"
    } else {
        "libuldren_loom.so"
    };
    target.join(name)
}

fn temp_loom(name: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "loom-ffi-abi-{name}-{}-{nonce}.loom",
        std::process::id()
    ))
}

fn cbor_pair(key: &str, value: CborValue) -> (CborValue, CborValue) {
    (CborValue::Text(key.to_string()), value)
}

fn cbor_get<'a>(map: &'a [(CborValue, CborValue)], key: &str) -> &'a CborValue {
    map.iter()
        .find_map(|(key_value, value)| match key_value {
            CborValue::Text(found) if found == key => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing key {key}"))
}

fn exec_program() -> Vec<u8> {
    wat::parse_str(
        r#"(module
             (import "env" "file_write" (func $write (param i32 i32 i32 i32)))
             (memory (export "memory") 1)
             (data (i32.const 0) "/out")
             (data (i32.const 16) "v")
             (func (export "run")
               (call $write (i32.const 0) (i32.const 4) (i32.const 16) (i32.const 1))))"#,
    )
    .expect("assemble exec program")
}

fn exec_request(workspace: WorkspaceId, wasm: &[u8]) -> Vec<u8> {
    let grants = loom_compute::GrantSet::all_facets();
    let manifest = loom_compute::Manifest::for_wasm("ffi-abi", wasm, grants.clone());
    let grants = CborValue::Array(
        grants
            .grants
            .iter()
            .map(|grant| {
                CborValue::Array(vec![
                    CborValue::Uint(u64::from(grant.facet.stable_tag())),
                    CborValue::Uint(u64::from(grant.mode.as_u8())),
                    CborValue::Array(
                        grant
                            .scopes
                            .iter()
                            .map(|scope| match scope {
                                loom_compute::Scope::All => {
                                    CborValue::Array(vec![CborValue::Uint(0)])
                                }
                                loom_compute::Scope::Prefix(prefix) => CborValue::Array(vec![
                                    CborValue::Uint(1),
                                    CborValue::Text(prefix.clone()),
                                ]),
                            })
                            .collect(),
                    ),
                ])
            })
            .collect(),
    );
    loom_codec::encode(&CborValue::Map(vec![
        cbor_pair(
            "schema",
            CborValue::Text("loom.exec.request.v1".to_string()),
        ),
        cbor_pair("mode", CborValue::Text("direct".to_string())),
        cbor_pair("workspace", CborValue::Bytes(workspace.as_bytes().to_vec())),
        cbor_pair("principal", CborValue::Bytes([9_u8; 16].to_vec())),
        cbor_pair("roles", CborValue::Array(Vec::new())),
        cbor_pair("authenticated", CborValue::Bool(false)),
        cbor_pair("base_branch", CborValue::Text("main".to_string())),
        cbor_pair("grants", grants),
        cbor_pair("fork_branch", CborValue::Null),
        cbor_pair(
            "steps",
            CborValue::Array(vec![CborValue::Map(vec![
                cbor_pair("manifest", CborValue::Bytes(manifest.encode())),
                cbor_pair("wasm", CborValue::Bytes(wasm.to_vec())),
                cbor_pair(
                    "inputs",
                    CborValue::Map(vec![cbor_pair(
                        "nk",
                        CborValue::Bytes(loom_core::key_to_cbor(&Value::Text("k".to_string()))),
                    )]),
                ),
                cbor_pair("fuel", CborValue::Uint(1_000_000)),
            ])]),
        ),
        cbor_pair("author", CborValue::Text("program".to_string())),
        cbor_pair("message", CborValue::Text("ffi exec".to_string())),
        cbor_pair("timestamp_ms", CborValue::Uint(42)),
    ]))
    .expect("encode exec request")
}

unsafe fn symbol<'a, T>(library: &'a Library, name: &[u8]) -> libloading::Symbol<'a, T> {
    unsafe { library.get::<T>(name) }.expect("required C ABI symbol")
}

fn c_string(value: &str) -> CString {
    CString::new(value).expect("C string")
}

fn c_path(path: &std::path::Path) -> CString {
    CString::new(path.to_string_lossy().as_bytes()).expect("path C string")
}

unsafe fn read_c_string(ptr: *mut c_char) -> String {
    assert!(!ptr.is_null());
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .expect("UTF-8 C string")
        .to_owned()
}

fn json_string_field(json: &str, field: &str) -> String {
    let needle = format!("\"{field}\":\"");
    let start = json.find(&needle).expect("field exists") + needle.len();
    let rest = &json[start..];
    rest[..rest.find('"').expect("field ends")].to_string()
}

fn last_error(library: &Library) -> Option<(i32, String)> {
    let last_error = unsafe {
        symbol::<unsafe extern "C" fn(*mut i32, *mut *mut c_char, *mut usize)>(
            library,
            b"loom_last_error\0",
        )
    };
    let string_free =
        unsafe { symbol::<unsafe extern "C" fn(*mut c_char)>(library, b"loom_string_free\0") };
    let mut code = 0;
    let mut message = std::ptr::null_mut();
    let mut len = 0usize;
    unsafe { last_error(&mut code, &mut message, &mut len) };
    if message.is_null() {
        return None;
    }
    let text = unsafe { read_c_string(message) };
    assert_eq!(len, text.len());
    unsafe { string_free(message) };
    Some((code, text))
}

fn render_result(library: &Library, status: i32, ptr: *mut c_uchar, len: usize) -> String {
    assert_eq!(status, 0, "status {status}, err {:?}", last_error(library));
    assert!(
        !ptr.is_null() && len > 0,
        "success must write a non-empty result buffer"
    );
    let result_to_json = unsafe {
        symbol::<unsafe extern "C" fn(*const c_uchar, usize, *mut *mut c_char) -> i32>(
            library,
            b"loom_result_to_json\0",
        )
    };
    let bytes_free = unsafe {
        symbol::<unsafe extern "C" fn(*mut c_uchar, usize)>(library, b"loom_bytes_free\0")
    };
    let string_free =
        unsafe { symbol::<unsafe extern "C" fn(*mut c_char)>(library, b"loom_string_free\0") };
    let mut json = std::ptr::null_mut();
    assert_eq!(unsafe { result_to_json(ptr, len, &mut json) }, 0);
    let rendered = unsafe { read_c_string(json) };
    unsafe { string_free(json) };
    unsafe { bytes_free(ptr, len) };
    rendered
}

fn open_sql(library: &Library, path: &std::path::Path) -> *mut c_void {
    let open = unsafe {
        symbol::<
            unsafe extern "C" fn(
                *const c_char,
                *const c_char,
                *const c_char,
                *mut *mut c_void,
            ) -> i32,
        >(library, b"loom_sql_open\0")
    };
    let path = c_path(path);
    let ns = c_string("app");
    let db = c_string("main");
    let mut session = std::ptr::null_mut();
    assert_eq!(
        unsafe { open(path.as_ptr(), ns.as_ptr(), db.as_ptr(), &mut session) },
        0,
        "open SQL session: {:?}",
        last_error(library)
    );
    assert!(!session.is_null());
    session
}

fn exec_sql(library: &Library, session: *mut c_void, sql: &str) -> String {
    let exec = unsafe {
        symbol::<
            unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut c_uchar, *mut usize) -> i32,
        >(library, b"loom_sql_exec\0")
    };
    let sql = c_string(sql);
    let mut ptr = std::ptr::null_mut();
    let mut len = 0usize;
    let status = unsafe { exec(session, sql.as_ptr(), &mut ptr, &mut len) };
    render_result(library, status, ptr, len)
}

#[test]
fn exported_abi_reports_version_and_canonical_blob_digest() {
    let path = library_path();
    assert!(
        path.exists(),
        "missing built C ABI library at {}",
        path.display()
    );
    let library = unsafe { Library::new(&path) }.expect("load built C ABI library");
    let version = unsafe {
        library
            .get::<unsafe extern "C" fn() -> *mut c_char>(b"loom_version\0")
            .expect("loom_version symbol")
    };
    let string_free = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut c_char)>(b"loom_string_free\0")
            .expect("loom_string_free symbol")
    };
    let blob_digest = unsafe {
        library
            .get::<unsafe extern "C" fn(*const c_uchar, usize) -> *mut c_char>(
                b"loom_blob_digest\0",
            )
            .expect("loom_blob_digest symbol")
    };

    let version_ptr = unsafe { version() };
    assert!(!version_ptr.is_null());
    let version_text = unsafe { CStr::from_ptr(version_ptr) }
        .to_str()
        .expect("UTF-8 version")
        .to_owned();
    unsafe { string_free(version_ptr) };
    assert!(!version_text.is_empty());

    let digest_ptr = unsafe { blob_digest(b"abc".as_ptr(), 3) };
    assert!(!digest_ptr.is_null());
    let digest = unsafe { CStr::from_ptr(digest_ptr) }
        .to_str()
        .expect("UTF-8 digest")
        .to_owned();
    unsafe { string_free(digest_ptr) };
    assert_eq!(
        digest,
        "blake3:7c953cb883974e24e76125db985052ecdfb77d40386cd699ecd952a314915b07"
    );
}

#[test]
fn exported_abi_transfers_and_releases_capability_result_buffers() {
    let library = unsafe { Library::new(library_path()) }.expect("load built C ABI library");
    let capabilities = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut *mut c_uchar, *mut usize) -> i32>(
                b"loom_capabilities\0",
            )
            .expect("loom_capabilities symbol")
    };
    let result_to_json = unsafe {
        library
            .get::<unsafe extern "C" fn(*const c_uchar, usize, *mut *mut c_char) -> i32>(
                b"loom_result_to_json\0",
            )
            .expect("loom_result_to_json symbol")
    };
    let bytes_free = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut c_uchar, usize)>(b"loom_bytes_free\0")
            .expect("loom_bytes_free symbol")
    };
    let string_free = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut c_char)>(b"loom_string_free\0")
            .expect("loom_string_free symbol")
    };

    let mut bytes = std::ptr::null_mut();
    let mut len = 0usize;
    assert_eq!(unsafe { capabilities(&mut bytes, &mut len) }, 0);
    assert!(!bytes.is_null());
    assert!(len > 0);

    let mut json = std::ptr::null_mut();
    assert_eq!(unsafe { result_to_json(bytes, len, &mut json) }, 0);
    assert!(!json.is_null());
    let rendered = unsafe { CStr::from_ptr(json) }
        .to_str()
        .expect("UTF-8 capability JSON")
        .to_owned();
    unsafe { string_free(json) };
    unsafe { bytes_free(bytes, len) };

    for capability in [
        "object-store",
        "workspace",
        "sql",
        "single-file-store",
        "rekey",
        "lanes",
    ] {
        assert!(rendered.contains(capability), "{rendered}");
    }
    assert!(rendered.contains("capability_id"), "{rendered}");
    assert!(rendered.contains("operational_state"), "{rendered}");
    assert!(rendered.contains("executable"), "{rendered}");
    assert!(rendered.contains("supported"), "{rendered}");
    assert!(!rendered.contains("\"supported\":"), "{rendered}");
}

#[test]
fn exported_abi_reports_runtime_profile_with_owned_result_buffers() {
    let library = unsafe { Library::new(library_path()) }.expect("load built C ABI library");
    let runtime_profile = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut *mut c_uchar, *mut usize) -> i32>(
                b"loom_runtime_profile\0",
            )
            .expect("loom_runtime_profile symbol")
    };
    let result_to_json = unsafe {
        library
            .get::<unsafe extern "C" fn(*const c_uchar, usize, *mut *mut c_char) -> i32>(
                b"loom_result_to_json\0",
            )
            .expect("loom_result_to_json symbol")
    };
    let bytes_free = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut c_uchar, usize)>(b"loom_bytes_free\0")
            .expect("loom_bytes_free symbol")
    };
    let string_free = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut c_char)>(b"loom_string_free\0")
            .expect("loom_string_free symbol")
    };

    let mut bytes = std::ptr::null_mut();
    let mut len = 0usize;
    assert_eq!(unsafe { runtime_profile(&mut bytes, &mut len) }, 0);
    let mut json = std::ptr::null_mut();
    assert_eq!(unsafe { result_to_json(bytes, len, &mut json) }, 0);
    let rendered = unsafe { CStr::from_ptr(json) }
        .to_str()
        .expect("UTF-8 runtime profile JSON")
        .to_owned();
    unsafe { string_free(json) };
    unsafe { bytes_free(bytes, len) };

    for field in [
        "binary_channel",
        "runtime_policy",
        "default_identity_profile",
        "crypto_provider",
        "tls_provider",
        "fips_capable",
        "fips_tls_claim",
    ] {
        assert!(rendered.contains(field), "{rendered}");
    }
}

#[test]
fn exported_abi_creates_writes_reads_and_closes_a_loom() {
    let library = unsafe { Library::new(library_path()) }.expect("load built C ABI library");
    let create = unsafe {
        library
            .get::<unsafe extern "C" fn(
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_uchar,
                usize,
            ) -> i32>(b"loom_create\0")
            .expect("loom_create symbol")
    };
    let open = unsafe {
        library
            .get::<unsafe extern "C" fn(*const c_char, *mut *mut c_void) -> i32>(b"loom_open\0")
            .expect("loom_open symbol")
    };
    let close = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut c_void)>(b"loom_close\0")
            .expect("loom_close symbol")
    };
    let workspace_create = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *mut *mut c_char) -> i32>(b"loom_workspace_create\0")
            .expect("loom_workspace_create symbol")
    };
    let write_file = unsafe {
        library
            .get::<unsafe extern "C" fn(
                *mut c_void,
                *const c_char,
                *const c_char,
                *const c_uchar,
                usize,
                u32,
            ) -> i32>(b"loom_write_file\0")
            .expect("loom_write_file symbol")
    };
    let read_file = unsafe {
        library
            .get::<unsafe extern "C" fn(
                *mut c_void,
                *const c_char,
                *const c_char,
                *mut *mut c_uchar,
                *mut usize,
            ) -> i32>(b"loom_read_file\0")
            .expect("loom_read_file symbol")
    };
    let string_free = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut c_char)>(b"loom_string_free\0")
            .expect("loom_string_free symbol")
    };
    let bytes_free = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut c_uchar, usize)>(b"loom_bytes_free\0")
            .expect("loom_bytes_free symbol")
    };

    let path = temp_loom("filesystem");
    let path_c = CString::new(path.to_string_lossy().as_bytes()).expect("path C string");
    let profile = CString::new("default").unwrap();
    assert_eq!(
        unsafe {
            create(
                path_c.as_ptr(),
                profile.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        },
        0
    );

    let mut handle = std::ptr::null_mut();
    assert_eq!(unsafe { open(path_c.as_ptr(), &mut handle) }, 0);
    let workspace = CString::new("files").unwrap();
    let facet = CString::new("files").unwrap();
    let mut workspace_id = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            workspace_create(
                handle,
                workspace.as_ptr(),
                facet.as_ptr(),
                &mut workspace_id,
            )
        },
        0
    );
    assert!(!workspace_id.is_null());
    unsafe { string_free(workspace_id) };

    let file = CString::new("/note.txt").unwrap();
    assert_eq!(
        unsafe {
            write_file(
                handle,
                workspace.as_ptr(),
                file.as_ptr(),
                b"loom".as_ptr(),
                4,
                0,
            )
        },
        0
    );
    let mut bytes = std::ptr::null_mut();
    let mut len = 0usize;
    assert_eq!(
        unsafe {
            read_file(
                handle,
                workspace.as_ptr(),
                file.as_ptr(),
                &mut bytes,
                &mut len,
            )
        },
        0
    );
    let content = unsafe { std::slice::from_raw_parts(bytes, len) }.to_vec();
    unsafe { bytes_free(bytes, len) };
    assert_eq!(content, b"loom");
    unsafe { close(handle) };
    let _ = std::fs::remove_file(path);
}

#[test]
fn exported_abi_round_trips_cas_blobs() {
    let library = unsafe { Library::new(library_path()) }.expect("load built C ABI library");
    let create = unsafe {
        symbol::<
            unsafe extern "C" fn(
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_uchar,
                usize,
            ) -> i32,
        >(&library, b"loom_create\0")
    };
    let open = unsafe {
        symbol::<unsafe extern "C" fn(*const c_char, *mut *mut c_void) -> i32>(
            &library,
            b"loom_open\0",
        )
    };
    let close = unsafe { symbol::<unsafe extern "C" fn(*mut c_void)>(&library, b"loom_close\0") };
    let string_free =
        unsafe { symbol::<unsafe extern "C" fn(*mut c_char)>(&library, b"loom_string_free\0") };
    let bytes_free = unsafe {
        symbol::<unsafe extern "C" fn(*mut c_uchar, usize)>(&library, b"loom_bytes_free\0")
    };
    let blob_digest = unsafe {
        symbol::<unsafe extern "C" fn(*const c_uchar, usize) -> *mut c_char>(
            &library,
            b"loom_blob_digest\0",
        )
    };
    let cas_put = unsafe {
        symbol::<
            unsafe extern "C" fn(
                *mut c_void,
                *const c_char,
                *const c_uchar,
                usize,
                *mut *mut c_char,
            ) -> i32,
        >(&library, b"loom_cas_put\0")
    };
    let cas_get = unsafe {
        symbol::<
            unsafe extern "C" fn(
                *mut c_void,
                *const c_char,
                *const c_char,
                *mut *mut c_uchar,
                *mut usize,
                *mut i32,
            ) -> i32,
        >(&library, b"loom_cas_get\0")
    };
    let cas_has = unsafe {
        symbol::<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *mut i32) -> i32>(
            &library,
            b"loom_cas_has\0",
        )
    };
    let cas_delete = unsafe {
        symbol::<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *mut i32) -> i32>(
            &library,
            b"loom_cas_delete\0",
        )
    };
    let cas_list_json = unsafe {
        symbol::<unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut c_char) -> i32>(
            &library,
            b"loom_cas_list_json\0",
        )
    };

    let path = temp_loom("cas");
    let path_c = c_path(&path);
    let profile = c_string("default");
    assert_eq!(
        unsafe {
            create(
                path_c.as_ptr(),
                profile.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        },
        0,
        "create failed: {:?}",
        last_error(&library)
    );

    let mut handle = std::ptr::null_mut();
    assert_eq!(unsafe { open(path_c.as_ptr(), &mut handle) }, 0);
    assert!(!handle.is_null());
    let workspace = c_string("blobs");
    let content = b"hello cas";

    let mut digest_ptr = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            cas_put(
                handle,
                workspace.as_ptr(),
                content.as_ptr(),
                content.len(),
                &mut digest_ptr,
            )
        },
        0,
        "put failed: {:?}",
        last_error(&library)
    );
    let digest = unsafe { read_c_string(digest_ptr) };
    unsafe { string_free(digest_ptr) };
    assert!(digest.starts_with("blake3:"), "{digest}");
    let digest_c = c_string(&digest);

    let mut found_content = std::ptr::null_mut();
    let mut found_len = 0usize;
    let mut found = -1i32;
    assert_eq!(
        unsafe {
            cas_get(
                handle,
                workspace.as_ptr(),
                digest_c.as_ptr(),
                &mut found_content,
                &mut found_len,
                &mut found,
            )
        },
        0,
        "get failed: {:?}",
        last_error(&library)
    );
    assert_eq!(found, 1);
    let bytes = unsafe { std::slice::from_raw_parts(found_content, found_len) }.to_vec();
    unsafe { bytes_free(found_content, found_len) };
    assert_eq!(bytes, content);

    let mut has = -1i32;
    assert_eq!(
        unsafe { cas_has(handle, workspace.as_ptr(), digest_c.as_ptr(), &mut has) },
        0,
        "has failed: {:?}",
        last_error(&library)
    );
    assert_eq!(has, 1);

    let mut list = std::ptr::null_mut();
    assert_eq!(
        unsafe { cas_list_json(handle, workspace.as_ptr(), &mut list) },
        0,
        "list failed: {:?}",
        last_error(&library)
    );
    let list_json = unsafe { read_c_string(list) };
    unsafe { string_free(list) };
    assert!(list_json.contains(&digest), "{list_json}");

    let other = b"never stored";
    let other_digest_ptr = unsafe { blob_digest(other.as_ptr(), other.len()) };
    let other_digest = unsafe { read_c_string(other_digest_ptr) };
    unsafe { string_free(other_digest_ptr) };
    let other_digest = c_string(&other_digest);
    let mut absent_ptr = std::ptr::null_mut();
    let mut absent_len = 0usize;
    let mut absent = -1i32;
    assert_eq!(
        unsafe {
            cas_get(
                handle,
                workspace.as_ptr(),
                other_digest.as_ptr(),
                &mut absent_ptr,
                &mut absent_len,
                &mut absent,
            )
        },
        0,
        "absent get failed: {:?}",
        last_error(&library)
    );
    assert_eq!(absent, 0);
    assert!(absent_ptr.is_null());
    assert_eq!(absent_len, 0);

    let bad = c_string("not-a-digest");
    let mut bad_has = -1i32;
    assert_eq!(
        unsafe { cas_has(handle, workspace.as_ptr(), bad.as_ptr(), &mut bad_has) },
        Code::InvalidArgument.as_i32(),
        "invalid digest was not rejected: {:?}",
        last_error(&library)
    );

    let mut deleted = -1i32;
    assert_eq!(
        unsafe { cas_delete(handle, workspace.as_ptr(), digest_c.as_ptr(), &mut deleted) },
        0,
        "delete failed: {:?}",
        last_error(&library)
    );
    assert_eq!(deleted, 1);
    let mut deleted_again = -1i32;
    assert_eq!(
        unsafe {
            cas_delete(
                handle,
                workspace.as_ptr(),
                digest_c.as_ptr(),
                &mut deleted_again,
            )
        },
        0
    );
    assert_eq!(deleted_again, 0);

    let mut has_after = -1i32;
    assert_eq!(
        unsafe {
            cas_has(
                handle,
                workspace.as_ptr(),
                digest_c.as_ptr(),
                &mut has_after,
            )
        },
        0,
        "has after delete failed: {:?}",
        last_error(&library)
    );
    assert_eq!(has_after, 0);

    unsafe { close(handle) };
    let _ = std::fs::remove_file(path);
}

#[test]
fn exported_abi_round_trips_tickets_json() {
    let library = unsafe { Library::new(library_path()) }.expect("load built C ABI library");
    let create = unsafe {
        symbol::<
            unsafe extern "C" fn(
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_uchar,
                usize,
            ) -> i32,
        >(&library, b"loom_create\0")
    };
    let open = unsafe {
        symbol::<unsafe extern "C" fn(*const c_char, *mut *mut c_void) -> i32>(
            &library,
            b"loom_open\0",
        )
    };
    let close = unsafe { symbol::<unsafe extern "C" fn(*mut c_void)>(&library, b"loom_close\0") };
    let string_free =
        unsafe { symbol::<unsafe extern "C" fn(*mut c_char)>(&library, b"loom_string_free\0") };
    let workspace_create = unsafe {
        symbol::<
            unsafe extern "C" fn(
                *mut c_void,
                *const c_char,
                *const c_char,
                *mut *mut c_char,
            ) -> i32,
        >(&library, b"loom_workspace_create\0")
    };
    let project_create = unsafe {
        symbol::<
            unsafe extern "C" fn(
                *mut c_void,
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_char,
                *mut *mut c_char,
            ) -> i32,
        >(&library, b"loom_tickets_project_create_json\0")
    };
    let create_ticket = unsafe {
        symbol::<
            unsafe extern "C" fn(
                *mut c_void,
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_char,
                *mut *mut c_char,
            ) -> i32,
        >(&library, b"loom_tickets_create_json\0")
    };
    let get_ticket = unsafe {
        symbol::<
            unsafe extern "C" fn(
                *mut c_void,
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_char,
                *mut *mut c_char,
            ) -> i32,
        >(&library, b"loom_tickets_get_json\0")
    };
    let list_tickets = unsafe {
        symbol::<
            unsafe extern "C" fn(
                *mut c_void,
                *const c_char,
                *const c_char,
                *const c_char,
                *mut *mut c_char,
            ) -> i32,
        >(&library, b"loom_tickets_list_json\0")
    };
    let ticket_history = unsafe {
        symbol::<
            unsafe extern "C" fn(
                *mut c_void,
                *const c_char,
                *const c_char,
                *const c_char,
                *mut *mut c_char,
            ) -> i32,
        >(&library, b"loom_tickets_history_json\0")
    };

    let path = temp_loom("tickets");
    let path_c = c_path(&path);
    let profile = c_string("default");
    assert_eq!(
        unsafe {
            create(
                path_c.as_ptr(),
                profile.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        },
        0,
        "create failed: {:?}",
        last_error(&library)
    );

    let mut handle = std::ptr::null_mut();
    assert_eq!(unsafe { open(path_c.as_ptr(), &mut handle) }, 0);
    assert!(!handle.is_null());

    let workspace = c_string("studio");
    let workspace_facet = c_string("files");
    let ticket_workspace = c_string("product");
    let project_id = c_string("core");
    let key_prefix = c_string("CORE");
    let project_name = c_string("Core");
    let empty = c_string("");
    let ticket_type = c_string("task");
    let fields = c_string(r#"{"status":"planned","title":"Binding parity"}"#);
    let labels = c_string(r#"["release"]"#);
    let projection = c_string("native");
    let list_request = c_string(r#"{"projection":"native"}"#);

    let mut out = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            workspace_create(
                handle,
                workspace.as_ptr(),
                workspace_facet.as_ptr(),
                &mut out,
            )
        },
        0,
        "workspace create failed: {:?}",
        last_error(&library)
    );
    unsafe { string_free(out) };

    assert_eq!(
        unsafe {
            project_create(
                handle,
                workspace.as_ptr(),
                ticket_workspace.as_ptr(),
                project_id.as_ptr(),
                key_prefix.as_ptr(),
                project_name.as_ptr(),
                empty.as_ptr(),
                &mut out,
            )
        },
        0,
        "project create failed: {:?}",
        last_error(&library)
    );
    let project = unsafe { read_c_string(out) };
    unsafe { string_free(out) };
    assert!(project.contains(r#""project_id":"core""#), "{project}");

    assert_eq!(
        unsafe {
            create_ticket(
                handle,
                workspace.as_ptr(),
                ticket_workspace.as_ptr(),
                project_id.as_ptr(),
                ticket_type.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                fields.as_ptr(),
                labels.as_ptr(),
                empty.as_ptr(),
                &mut out,
            )
        },
        0,
        "ticket create failed: {:?}",
        last_error(&library)
    );
    let created = unsafe { read_c_string(out) };
    unsafe { string_free(out) };
    assert!(created.contains(r#""title":"Binding parity""#), "{created}");
    let ticket_id = json_string_field(&created, "ticket_id");
    let ticket_id = c_string(&ticket_id);

    assert_eq!(
        unsafe {
            get_ticket(
                handle,
                workspace.as_ptr(),
                ticket_workspace.as_ptr(),
                ticket_id.as_ptr(),
                projection.as_ptr(),
                &mut out,
            )
        },
        0,
        "ticket get failed: {:?}",
        last_error(&library)
    );
    let fetched = unsafe { read_c_string(out) };
    unsafe { string_free(out) };
    assert!(fetched.contains(r#""status":"planned""#), "{fetched}");

    assert_eq!(
        unsafe {
            list_tickets(
                handle,
                workspace.as_ptr(),
                ticket_workspace.as_ptr(),
                list_request.as_ptr(),
                &mut out,
            )
        },
        0,
        "ticket list failed: {:?}",
        last_error(&library)
    );
    let listed = unsafe { read_c_string(out) };
    unsafe { string_free(out) };
    assert!(listed.contains(r#""title":"Binding parity""#), "{listed}");

    assert_eq!(
        unsafe {
            ticket_history(
                handle,
                workspace.as_ptr(),
                ticket_workspace.as_ptr(),
                ticket_id.as_ptr(),
                &mut out,
            )
        },
        0,
        "ticket history failed: {:?}",
        last_error(&library)
    );
    let history = unsafe { read_c_string(out) };
    unsafe { string_free(out) };
    assert!(
        history.contains(r#""operation_kind":"ticket.created""#),
        "{history}"
    );

    unsafe { close(handle) };
    let _ = std::fs::remove_file(path);
}

#[test]
fn exported_abi_manages_workspace_lifecycle() {
    let library = unsafe { Library::new(library_path()) }.expect("load built C ABI library");
    let create = unsafe {
        symbol::<
            unsafe extern "C" fn(
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_uchar,
                usize,
            ) -> i32,
        >(&library, b"loom_create\0")
    };
    let open = unsafe {
        symbol::<unsafe extern "C" fn(*const c_char, *mut *mut c_void) -> i32>(
            &library,
            b"loom_open\0",
        )
    };
    let close = unsafe { symbol::<unsafe extern "C" fn(*mut c_void)>(&library, b"loom_close\0") };
    let string_free =
        unsafe { symbol::<unsafe extern "C" fn(*mut c_char)>(&library, b"loom_string_free\0") };
    let workspace_create = unsafe {
        symbol::<
            unsafe extern "C" fn(
                *mut c_void,
                *const c_char,
                *const c_char,
                *mut *mut c_char,
            ) -> i32,
        >(&library, b"loom_workspace_create\0")
    };
    let workspace_list = unsafe {
        symbol::<unsafe extern "C" fn(*mut c_void, *mut *mut c_char) -> i32>(
            &library,
            b"loom_workspace_list_json\0",
        )
    };
    let workspace_rename = unsafe {
        symbol::<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> i32>(
            &library,
            b"loom_workspace_rename\0",
        )
    };
    let workspace_delete = unsafe {
        symbol::<unsafe extern "C" fn(*mut c_void, *const c_char) -> i32>(
            &library,
            b"loom_workspace_delete\0",
        )
    };

    let path = temp_loom("workspace-lifecycle");
    let path_c = c_path(&path);
    let profile = c_string("default");
    assert_eq!(
        unsafe {
            create(
                path_c.as_ptr(),
                profile.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        },
        0,
        "create failed: {:?}",
        last_error(&library)
    );

    let mut handle = std::ptr::null_mut();
    assert_eq!(unsafe { open(path_c.as_ptr(), &mut handle) }, 0);
    let name = c_string("work");
    let facet = c_string("files");
    let mut workspace_id = std::ptr::null_mut();
    assert_eq!(
        unsafe { workspace_create(handle, name.as_ptr(), facet.as_ptr(), &mut workspace_id) },
        0,
        "workspace create failed: {:?}",
        last_error(&library)
    );
    let id = unsafe { read_c_string(workspace_id) };
    unsafe { string_free(workspace_id) };
    assert!(id.contains('-'), "{id}");

    let mut list = std::ptr::null_mut();
    assert_eq!(unsafe { workspace_list(handle, &mut list) }, 0);
    let json = unsafe { read_c_string(list) };
    unsafe { string_free(list) };
    assert!(
        json.contains("\"name\":\"work\"") && json.contains("\"files\""),
        "{json}"
    );

    let renamed = c_string("client");
    assert_eq!(
        unsafe { workspace_rename(handle, name.as_ptr(), renamed.as_ptr()) },
        0,
        "rename failed: {:?}",
        last_error(&library)
    );
    unsafe { close(handle) };

    let mut fresh = std::ptr::null_mut();
    assert_eq!(unsafe { open(path_c.as_ptr(), &mut fresh) }, 0);
    let mut list_after_rename = std::ptr::null_mut();
    assert_eq!(unsafe { workspace_list(fresh, &mut list_after_rename) }, 0);
    let renamed_json = unsafe { read_c_string(list_after_rename) };
    unsafe { string_free(list_after_rename) };
    assert!(
        renamed_json.contains("\"name\":\"client\""),
        "{renamed_json}"
    );

    let id = c_string(&id);
    assert_eq!(
        unsafe { workspace_delete(fresh, id.as_ptr()) },
        0,
        "delete failed: {:?}",
        last_error(&library)
    );
    let mut list_after_delete = std::ptr::null_mut();
    assert_eq!(unsafe { workspace_list(fresh, &mut list_after_delete) }, 0);
    let deleted_json = unsafe { read_c_string(list_after_delete) };
    unsafe { string_free(list_after_delete) };
    assert_eq!(deleted_json, "[]");

    unsafe { close(fresh) };
    let _ = std::fs::remove_file(path);
}

#[test]
fn exported_abi_executes_a_canonical_program_request() {
    let library = unsafe { Library::new(library_path()) }.expect("load built C ABI library");
    let create = unsafe {
        library
            .get::<unsafe extern "C" fn(
                *const c_char,
                *const c_char,
                *const c_char,
                *const c_uchar,
                usize,
            ) -> i32>(b"loom_create\0")
            .expect("loom_create symbol")
    };
    let open = unsafe {
        library
            .get::<unsafe extern "C" fn(*const c_char, *mut *mut c_void) -> i32>(b"loom_open\0")
            .expect("loom_open symbol")
    };
    let close = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut c_void)>(b"loom_close\0")
            .expect("loom_close symbol")
    };
    let workspace_create = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *mut *mut c_char) -> i32>(b"loom_workspace_create\0")
            .expect("loom_workspace_create symbol")
    };
    let write_file = unsafe {
        library
            .get::<unsafe extern "C" fn(
                *mut c_void,
                *const c_char,
                *const c_char,
                *const c_uchar,
                usize,
                u32,
            ) -> i32>(b"loom_write_file\0")
            .expect("loom_write_file symbol")
    };
    let commit = unsafe {
        library
            .get::<unsafe extern "C" fn(
                *mut c_void,
                *const c_char,
                *const c_char,
                *const c_char,
                *mut *mut c_char,
            ) -> i32>(b"loom_commit\0")
            .expect("loom_commit symbol")
    };
    let exec = unsafe {
        library
            .get::<unsafe extern "C" fn(
                *mut c_void,
                *const c_uchar,
                usize,
                *mut *mut c_uchar,
                *mut usize,
            ) -> i32>(b"loom_exec_cbor\0")
            .expect("loom_exec_cbor symbol")
    };
    let read_file = unsafe {
        library
            .get::<unsafe extern "C" fn(
                *mut c_void,
                *const c_char,
                *const c_char,
                *mut *mut c_uchar,
                *mut usize,
            ) -> i32>(b"loom_read_file\0")
            .expect("loom_read_file symbol")
    };
    let string_free = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut c_char)>(b"loom_string_free\0")
            .expect("loom_string_free symbol")
    };
    let bytes_free = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut c_uchar, usize)>(b"loom_bytes_free\0")
            .expect("loom_bytes_free symbol")
    };

    let path = temp_loom("exec");
    let path_c = CString::new(path.to_string_lossy().as_bytes()).unwrap();
    let profile = CString::new("default").unwrap();
    assert_eq!(
        unsafe {
            create(
                path_c.as_ptr(),
                profile.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        },
        0
    );
    let mut handle = std::ptr::null_mut();
    assert_eq!(unsafe { open(path_c.as_ptr(), &mut handle) }, 0);
    let workspace_name = CString::new("exec").unwrap();
    let facet = CString::new("files").unwrap();
    let mut workspace_id = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            workspace_create(
                handle,
                workspace_name.as_ptr(),
                facet.as_ptr(),
                &mut workspace_id,
            )
        },
        0
    );
    let workspace = WorkspaceId::parse(
        unsafe { CStr::from_ptr(workspace_id) }
            .to_str()
            .expect("workspace ID"),
    )
    .expect("valid workspace ID");
    unsafe { string_free(workspace_id) };
    let seed = CString::new("/seed").unwrap();
    assert_eq!(
        unsafe {
            write_file(
                handle,
                workspace_name.as_ptr(),
                seed.as_ptr(),
                b"base".as_ptr(),
                4,
                0,
            )
        },
        0
    );
    let author = CString::new("setup").unwrap();
    let message = CString::new("base").unwrap();
    let mut commit_id = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            commit(
                handle,
                workspace_name.as_ptr(),
                author.as_ptr(),
                message.as_ptr(),
                &mut commit_id,
            )
        },
        0
    );
    unsafe { string_free(commit_id) };

    let request = exec_request(workspace, &exec_program());
    let mut response = std::ptr::null_mut();
    let mut response_len = 0usize;
    assert_eq!(
        unsafe {
            exec(
                handle,
                request.as_ptr(),
                request.len(),
                &mut response,
                &mut response_len,
            )
        },
        0
    );
    let response_bytes = unsafe { std::slice::from_raw_parts(response, response_len) }.to_vec();
    unsafe { bytes_free(response, response_len) };
    let CborValue::Map(fields) = loom_codec::decode(&response_bytes).expect("exec response") else {
        panic!("exec response is not a map");
    };
    assert_eq!(
        cbor_get(&fields, "schema"),
        &CborValue::Text("loom.exec.result.v1".to_string())
    );
    assert_eq!(cbor_get(&fields, "committed"), &CborValue::Bool(true));

    let output = CString::new("/out").unwrap();
    let mut content = std::ptr::null_mut();
    let mut content_len = 0usize;
    assert_eq!(
        unsafe {
            read_file(
                handle,
                workspace_name.as_ptr(),
                output.as_ptr(),
                &mut content,
                &mut content_len,
            )
        },
        0
    );
    assert_eq!(
        unsafe { std::slice::from_raw_parts(content, content_len) },
        b"v"
    );
    unsafe { bytes_free(content, content_len) };
    unsafe { close(handle) };
    let _ = std::fs::remove_file(path);
}

#[test]
fn exported_abi_rejects_null_sql_sessions_with_stable_errors() {
    let library = unsafe { Library::new(library_path()) }.expect("load built C ABI library");
    let exec = unsafe {
        symbol::<
            unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut c_uchar, *mut usize) -> i32,
        >(&library, b"loom_sql_exec\0")
    };
    let close =
        unsafe { symbol::<unsafe extern "C" fn(*mut c_void)>(&library, b"loom_sql_close\0") };

    let sql = c_string("SELECT 1");
    let mut ptr = std::ptr::null_mut();
    let mut len = 0usize;
    let status = unsafe { exec(std::ptr::null_mut(), sql.as_ptr(), &mut ptr, &mut len) };
    assert_eq!(status, Code::InvalidArgument.as_i32());
    assert!(ptr.is_null());
    assert_eq!(len, 0);
    let (code, message) = last_error(&library).expect("stable SQL error");
    assert_eq!(code, status);
    assert!(message.contains("null session"), "{message}");
    unsafe { close(std::ptr::null_mut()) };
}

#[test]
fn exported_abi_allows_two_sql_sessions_to_share_a_loom_path() {
    let library = unsafe { Library::new(library_path()) }.expect("load built C ABI library");
    let close =
        unsafe { symbol::<unsafe extern "C" fn(*mut c_void)>(&library, b"loom_sql_close\0") };
    let path = temp_loom("two-sql-sessions");
    let first = open_sql(&library, &path);
    let second = open_sql(&library, &path);

    let create = exec_sql(&library, first, "CREATE TABLE t (id INTEGER PRIMARY KEY)");
    assert!(create.contains("Create"), "{create}");
    let select = exec_sql(&library, second, "SELECT id FROM t");
    assert!(select.contains("Select"), "{select}");

    unsafe { close(first) };
    unsafe { close(second) };
    let _ = std::fs::remove_file(path);
}

#[test]
fn exported_abi_sql_session_round_trips_and_reports_errors() {
    let library = unsafe { Library::new(library_path()) }.expect("load built C ABI library");
    let exec = unsafe {
        symbol::<
            unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut c_uchar, *mut usize) -> i32,
        >(&library, b"loom_sql_exec\0")
    };
    let commit = unsafe {
        symbol::<
            unsafe extern "C" fn(
                *mut c_void,
                *const c_char,
                *const c_char,
                *mut *mut c_char,
            ) -> i32,
        >(&library, b"loom_sql_commit\0")
    };
    let close =
        unsafe { symbol::<unsafe extern "C" fn(*mut c_void)>(&library, b"loom_sql_close\0") };
    let string_free =
        unsafe { symbol::<unsafe extern "C" fn(*mut c_char)>(&library, b"loom_string_free\0") };

    let path = temp_loom("sql-session");
    let session = open_sql(&library, &path);
    let _ = exec_sql(
        &library,
        session,
        "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)",
    );
    let _ = exec_sql(&library, session, "INSERT INTO t VALUES (1, 'hello')");
    let selected = exec_sql(&library, session, "SELECT id, v FROM t");
    assert!(
        selected.contains("Select") && selected.contains("hello"),
        "{selected}"
    );

    let message = c_string("seed");
    let author = c_string("tester");
    let mut commit_ptr = std::ptr::null_mut();
    assert_eq!(
        unsafe { commit(session, message.as_ptr(), author.as_ptr(), &mut commit_ptr) },
        0,
        "commit failed: {:?}",
        last_error(&library)
    );
    let commit_id = unsafe { read_c_string(commit_ptr) };
    unsafe { string_free(commit_ptr) };
    assert!(commit_id.starts_with("blake3:"), "{commit_id}");
    unsafe { close(session) };

    let fresh = open_sql(&library, &path);
    let selected_after_reopen = exec_sql(&library, fresh, "SELECT id, v FROM t");
    assert!(
        selected_after_reopen.contains("hello"),
        "{selected_after_reopen}"
    );
    unsafe { close(fresh) };

    let failing = open_sql(&library, &path);
    let bad = c_string("SELECT * FROM does_not_exist");
    let mut ptr = std::ptr::null_mut();
    let mut len = 0usize;
    let status = unsafe { exec(failing, bad.as_ptr(), &mut ptr, &mut len) };
    assert_eq!(status, Code::SqlTableNotFound.as_i32());
    assert!(ptr.is_null());
    assert_eq!(len, 0);
    assert_eq!(last_error(&library).expect("SQL table error").0, status);
    unsafe { close(failing) };
    let _ = std::fs::remove_file(path);
}

#[test]
fn exported_abi_rejects_dangling_per_operation_sql_transactions() {
    let library = unsafe { Library::new(library_path()) }.expect("load built C ABI library");
    let exec = unsafe {
        symbol::<
            unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut c_uchar, *mut usize) -> i32,
        >(&library, b"loom_sql_exec\0")
    };
    let close =
        unsafe { symbol::<unsafe extern "C" fn(*mut c_void)>(&library, b"loom_sql_close\0") };
    let path = temp_loom("dangling-begin");
    let session = open_sql(&library, &path);
    let begin = c_string("BEGIN");
    let mut ptr = std::ptr::null_mut();
    let mut len = 0usize;
    let status = unsafe { exec(session, begin.as_ptr(), &mut ptr, &mut len) };
    assert_ne!(status, 0);
    assert!(ptr.is_null());
    assert_eq!(len, 0);
    assert!(
        last_error(&library)
            .expect("dangling BEGIN error")
            .1
            .contains("LoomSqlBatch")
    );
    unsafe { close(session) };
    let _ = std::fs::remove_file(path);
}

#[test]
fn exported_abi_sql_query_streams_rows_one_at_a_time() {
    let library = unsafe { Library::new(library_path()) }.expect("load built C ABI library");
    let query = unsafe {
        symbol::<unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut c_void) -> i32>(
            &library,
            b"loom_sql_query\0",
        )
    };
    let iter_next = unsafe {
        symbol::<unsafe extern "C" fn(*mut c_void, *mut *mut c_uchar, *mut usize, *mut i32) -> i32>(
            &library,
            b"loom_iter_next\0",
        )
    };
    let iter_free =
        unsafe { symbol::<unsafe extern "C" fn(*mut c_void)>(&library, b"loom_iter_free\0") };
    let close =
        unsafe { symbol::<unsafe extern "C" fn(*mut c_void)>(&library, b"loom_sql_close\0") };
    let bytes_free = unsafe {
        symbol::<unsafe extern "C" fn(*mut c_uchar, usize)>(&library, b"loom_bytes_free\0")
    };
    let path = temp_loom("sql-query");
    let session = open_sql(&library, &path);
    let _ = exec_sql(
        &library,
        session,
        "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)",
    );
    let _ = exec_sql(
        &library,
        session,
        "INSERT INTO t VALUES (1,'a'),(2,'b'),(3,'c')",
    );

    let select = c_string("SELECT id, v FROM t ORDER BY id");
    let mut iter = std::ptr::null_mut();
    assert_eq!(unsafe { query(session, select.as_ptr(), &mut iter) }, 0);
    assert!(!iter.is_null());

    let mut count = 0;
    loop {
        let mut ptr = std::ptr::null_mut();
        let mut len = 0usize;
        let mut done = 0i32;
        assert_eq!(
            unsafe { iter_next(iter, &mut ptr, &mut len, &mut done) },
            0,
            "iter_next failed: {:?}",
            last_error(&library)
        );
        if done == 1 {
            assert!(ptr.is_null());
            assert_eq!(len, 0);
            break;
        }
        assert!(!ptr.is_null() && len > 0);
        unsafe { bytes_free(ptr, len) };
        count += 1;
        assert!(count <= 3);
    }
    assert_eq!(count, 3);

    unsafe { iter_free(iter) };
    unsafe { close(session) };
    let _ = std::fs::remove_file(path);
}

#[test]
fn exported_abi_sql_query_rejects_mutating_statements() {
    let library = unsafe { Library::new(library_path()) }.expect("load built C ABI library");
    let query = unsafe {
        symbol::<unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut c_void) -> i32>(
            &library,
            b"loom_sql_query\0",
        )
    };
    let close =
        unsafe { symbol::<unsafe extern "C" fn(*mut c_void)>(&library, b"loom_sql_close\0") };
    let path = temp_loom("sql-query-rejects-mutating-statements");
    let session = open_sql(&library, &path);
    let statement = c_string("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
    let mut iter = std::ptr::null_mut();
    assert_eq!(
        unsafe { query(session, statement.as_ptr(), &mut iter) },
        Code::PermissionDenied.as_i32()
    );
    assert!(iter.is_null());
    assert_eq!(
        last_error(&library).expect("query mutation error").0,
        Code::PermissionDenied.as_i32()
    );
    unsafe { close(session) };
    let _ = std::fs::remove_file(path);
}

#[test]
fn exported_abi_decodes_streamed_sql_rows_with_result_views() {
    let library = unsafe { Library::new(library_path()) }.expect("load built C ABI library");
    let query = unsafe {
        symbol::<unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut c_void) -> i32>(
            &library,
            b"loom_sql_query\0",
        )
    };
    let iter_next = unsafe {
        symbol::<unsafe extern "C" fn(*mut c_void, *mut *mut c_uchar, *mut usize, *mut i32) -> i32>(
            &library,
            b"loom_iter_next\0",
        )
    };
    let iter_free =
        unsafe { symbol::<unsafe extern "C" fn(*mut c_void)>(&library, b"loom_iter_free\0") };
    let close =
        unsafe { symbol::<unsafe extern "C" fn(*mut c_void)>(&library, b"loom_sql_close\0") };
    let bytes_free = unsafe {
        symbol::<unsafe extern "C" fn(*mut c_uchar, usize)>(&library, b"loom_bytes_free\0")
    };
    let row_open = unsafe {
        symbol::<unsafe extern "C" fn(*const c_uchar, usize, *mut *mut c_void) -> i32>(
            &library,
            b"loom_row_open\0",
        )
    };
    let row_len = unsafe {
        symbol::<unsafe extern "C" fn(*const c_void, usize, usize) -> usize>(
            &library,
            b"loom_result_row_len\0",
        )
    };
    let cell = unsafe {
        symbol::<unsafe extern "C" fn(*const c_void, usize, usize, usize, *mut LoomValue) -> i32>(
            &library,
            b"loom_result_cell\0",
        )
    };
    let result_close =
        unsafe { symbol::<unsafe extern "C" fn(*mut c_void)>(&library, b"loom_result_close\0") };

    let path = temp_loom("sql-row-view");
    let session = open_sql(&library, &path);
    let _ = exec_sql(
        &library,
        session,
        "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)",
    );
    let _ = exec_sql(&library, session, "INSERT INTO t VALUES (1,'a'),(2,'b')");
    let select = c_string("SELECT id, v FROM t ORDER BY id");
    let mut iter = std::ptr::null_mut();
    assert_eq!(unsafe { query(session, select.as_ptr(), &mut iter) }, 0);

    let mut ids = Vec::new();
    let mut texts = Vec::new();
    loop {
        let mut ptr = std::ptr::null_mut();
        let mut len = 0usize;
        let mut done = 0i32;
        assert_eq!(unsafe { iter_next(iter, &mut ptr, &mut len, &mut done) }, 0);
        if done == 1 {
            break;
        }
        let mut row = std::ptr::null_mut();
        assert_eq!(unsafe { row_open(ptr, len, &mut row) }, 0);
        unsafe { bytes_free(ptr, len) };
        assert_eq!(unsafe { row_len(row, 0, 0) }, 2);

        let mut id = LoomValue::zeroed();
        let mut text = LoomValue::zeroed();
        assert_eq!(unsafe { cell(row, 0, 0, 0, &mut id) }, 0);
        assert_eq!(unsafe { cell(row, 0, 0, 1, &mut text) }, 0);
        assert_eq!(id.tag, LOOM_VALUE_INT);
        ids.push(id.int_val);
        assert_eq!(text.tag, LOOM_VALUE_TEXT);
        let bytes = unsafe { std::slice::from_raw_parts(text.data, text.data_len) };
        texts.push(String::from_utf8(bytes.to_vec()).expect("UTF-8 text cell"));
        unsafe { result_close(row) };
    }
    assert_eq!(ids, vec![1, 2]);
    assert_eq!(texts, vec!["a".to_string(), "b".to_string()]);

    unsafe { iter_free(iter) };
    unsafe { close(session) };
    let _ = std::fs::remove_file(path);
}

#[test]
fn exported_abi_reports_stable_errors_without_allocating_output() {
    let library = unsafe { Library::new(library_path()) }.expect("load built C ABI library");
    let open = unsafe {
        library
            .get::<unsafe extern "C" fn(*const c_char, *mut *mut c_void) -> i32>(b"loom_open\0")
            .expect("loom_open symbol")
    };
    let last_error = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut i32, *mut *mut c_char, *mut usize)>(
                b"loom_last_error\0",
            )
            .expect("loom_last_error symbol")
    };
    let string_free = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut c_char)>(b"loom_string_free\0")
            .expect("loom_string_free symbol")
    };

    let mut handle = std::ptr::null_mut();
    let status = unsafe { open(std::ptr::null(), &mut handle) };
    assert_ne!(status, 0);
    assert!(handle.is_null());

    let mut code = 0;
    let mut message = std::ptr::null_mut();
    let mut len = 0usize;
    unsafe { last_error(&mut code, &mut message, &mut len) };
    assert_eq!(code, status);
    assert!(!message.is_null());
    let text = unsafe { CStr::from_ptr(message) }
        .to_str()
        .expect("UTF-8 error")
        .to_owned();
    assert_eq!(len, text.len());
    unsafe { string_free(message) };
    assert!(text.contains("loom_open"));
}

#[test]
fn exported_abi_reports_a_stopped_daemon_for_an_unserved_store() {
    let library = unsafe { Library::new(library_path()) }.expect("load built C ABI library");
    let daemon_status = unsafe {
        library
            .get::<unsafe extern "C" fn(*const c_char, *mut *mut c_char) -> i32>(
                b"loom_daemon_status_json\0",
            )
            .expect("loom_daemon_status_json symbol")
    };
    let string_free = unsafe {
        library
            .get::<unsafe extern "C" fn(*mut c_char)>(b"loom_string_free\0")
            .expect("loom_string_free symbol")
    };

    let path = temp_loom("stopped-daemon");
    std::fs::write(&path, b"not-a-store").expect("write store fixture");
    let path_c = CString::new(path.to_string_lossy().as_bytes()).expect("path C string");
    let mut json = std::ptr::null_mut();
    assert_eq!(unsafe { daemon_status(path_c.as_ptr(), &mut json) }, 0);
    assert!(!json.is_null());
    let rendered = unsafe { CStr::from_ptr(json) }
        .to_str()
        .expect("UTF-8 daemon status")
        .to_owned();
    unsafe { string_free(json) };
    assert!(rendered.contains("\"state\":\"STOPPED\""), "{rendered}");
    assert!(rendered.contains("\"pid\":null"), "{rendered}");
    let _ = std::fs::remove_file(path);
}
