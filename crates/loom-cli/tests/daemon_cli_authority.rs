#![cfg(feature = "daemon-cli-tests")]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use loom_codec::Value as CborValue;
use loom_core::{
    AclRight, AclStore, AclSubject, Algo, ColumnType, ColumnarSet, DataframeInputFormat,
    DataframeMaterialization, DataframeMaterializationTarget, DataframeOperation, DataframePlan,
    DataframeSourceBinding, DataframeSourceKind, Digest, FacetKind, IdentityStore, Loom,
    Value as LoomValue, WorkspaceId,
};
use loom_store::{FileStore, LocalOpenAuth, daemon};
use loom_substrate::drive::{DrivePolicyRegistry, DrivePolicyTarget, drive_policy_registry_key};

struct DaemonStore {
    path: String,
}

impl DaemonStore {
    fn new(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-daemon-cli-authority-{tag}-{}-{}.loom",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = path.to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&path);
        Self { path }
    }

    fn start(&mut self) {
        loom(["daemon", "start", &self.path, "--transport", "tcp"]).unwrap();
    }

    fn spawn_without_start_audit(&self) -> Child {
        let paths = daemon::paths(&self.path).unwrap();
        Command::new(env!("CARGO_BIN_EXE_loom"))
            .args([
                "daemon",
                "run",
                &self.path,
                "--addr-file",
                paths.addr_file.to_str().unwrap(),
                "--pid-file",
                paths.pid_file.to_str().unwrap(),
                "--lock-file",
                paths.lock_file.to_str().unwrap(),
                "--transport",
                "tcp",
                "--startup-mode",
                "persistent",
                "--startup-initiator",
                "cli.daemon.start",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    fn start_without_start_audit(&mut self) {
        drop(self.spawn_without_start_audit());
    }

    fn prewarm_daemon_engine_without_audit(&mut self) {
        let mut child = self.spawn_without_start_audit();
        wait_for_daemon_status(self, "running\t");
        child.kill().unwrap();
        child.wait().unwrap();
        let paths = daemon::paths(&self.path).unwrap();
        for path in [
            &paths.addr_file,
            &paths.pid_file,
            &paths.lock_file,
            &paths.sock_file,
        ] {
            let _ = std::fs::remove_file(path);
        }
        self.assert_runtime_artifacts_removed();
    }

    fn stop(&mut self) {
        loom(["daemon", "stop", "--hard", &self.path]).unwrap();
    }

    fn assert_runtime_artifacts_removed(&self) {
        let paths = daemon::paths(&self.path).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline
            && (paths.addr_file.exists()
                || paths.pid_file.exists()
                || paths.lock_file.exists()
                || paths.sock_file.exists())
        {
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(
            !paths.addr_file.exists(),
            "daemon address artifact remained at {}",
            paths.addr_file.display()
        );
        assert!(
            !paths.pid_file.exists(),
            "daemon pid artifact remained at {}",
            paths.pid_file.display()
        );
        assert!(
            !paths.lock_file.exists(),
            "daemon lock artifact remained at {}",
            paths.lock_file.display()
        );
        assert!(
            !paths.sock_file.exists(),
            "daemon socket artifact remained at {}",
            paths.sock_file.display()
        );
    }

    fn stop_auth(&mut self, root: WorkspaceId) {
        let mut passphrase = std::env::temp_dir();
        passphrase.push(format!(
            "loom-daemon-cli-authority-passphrase-{}-{}.txt",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&passphrase, "root-pass").unwrap();
        let root = root.to_string();
        let passphrase_source = format!("file:{}", passphrase.to_string_lossy());
        loom([
            "--auth-principal",
            &root,
            "--auth-key-source",
            &passphrase_source,
            "daemon",
            "stop",
            "--wait",
            "5000",
            &self.path,
        ])
        .unwrap();
        let _ = std::fs::remove_file(passphrase);
    }

    fn audit_auth(&self, root: WorkspaceId) -> String {
        let mut passphrase = std::env::temp_dir();
        passphrase.push(format!(
            "loom-daemon-cli-authority-passphrase-{}-{}.txt",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&passphrase, "root-pass").unwrap();
        let root = root.to_string();
        let passphrase_source = format!("file:{}", passphrase.to_string_lossy());
        let audit = loom([
            "--auth-principal",
            &root,
            "--auth-key-source",
            &passphrase_source,
            "audit",
            "list",
            &self.path,
        ])
        .unwrap();
        let _ = std::fs::remove_file(passphrase);
        audit
    }
}

impl Drop for DaemonStore {
    fn drop(&mut self) {
        let _ = loom(["daemon", "stop", "--hard", &self.path]);
        let _ = std::fs::remove_file(&self.path);
    }
}

struct RemoteServeStore {
    child: Child,
    store: String,
    cert_path: String,
    key_path: String,
    config_path: String,
    globals: Vec<String>,
}

impl RemoteServeStore {
    #[cfg(all(feature = "serve", feature = "remote-client"))]
    fn start(tag: &str) -> Self {
        let mut store = std::env::temp_dir();
        store.push(format!(
            "loom-daemon-cli-authority-{tag}-{}-{}.loom",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = store.to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&store);
        loom(["store", "init", &store]).unwrap();
        Self::start_existing(tag, store)
    }

    #[cfg(all(feature = "serve", feature = "remote-client"))]
    fn start_existing(tag: &str, store: String) -> Self {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let cert_path = temp_text_file(&format!("{tag}-cert"), &cert.cert.pem());
        let key_path = temp_text_file(&format!("{tag}-key"), &cert.signing_key.serialize_pem());
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let bind = probe.local_addr().unwrap().to_string();
        drop(probe);
        let mut child = Command::new(env!("CARGO_BIN_EXE_loom"))
            .args([
                "serve",
                "remote",
                &store,
                "--bind",
                &bind,
                "--service-root",
                "https://localhost/apps/loom",
                "--tls-cert",
                &cert_path,
                "--tls-key",
                &key_path,
                "--tls-trust",
                "insecure-dev",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| panic!("spawn loom serve remote: {error}"));
        let stdout = child.stdout.take().expect("remote stdout");
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let started = Instant::now();
        while line.is_empty() && started.elapsed() < Duration::from_secs(10) {
            if let Some(status) = child.try_wait().unwrap() {
                let stderr = child
                    .stderr
                    .take()
                    .map(|mut stderr| {
                        let mut text = String::new();
                        let _ = std::io::Read::read_to_string(&mut stderr, &mut text);
                        text
                    })
                    .unwrap_or_default();
                panic!("serve remote exited before listening with {status}: {stderr}");
            }
            reader.read_line(&mut line).expect("read remote listening");
        }
        assert!(
            !line.is_empty(),
            "serve remote did not print listening JSON"
        );
        let value: serde_json::Value =
            serde_json::from_str(line.trim()).expect("remote listening JSON");
        let listening = value
            .get("listening")
            .and_then(|value| value.as_str())
            .expect("listening address");
        let port = listening
            .rsplit_once(':')
            .and_then(|(_, port)| port.parse::<u16>().ok())
            .expect("listening port");
        let target = format!("https://127.0.0.1:{port}/apps/loom");
        let config = format!("[contexts.mu17ga]\ntarget = {target:?}\ntls = \"insecure-dev\"\n");
        let config_path = temp_text_file(&format!("{tag}-contexts"), &config);
        let globals = vec![
            "--config".to_string(),
            config_path.clone(),
            "--context".to_string(),
            "mu17ga".to_string(),
        ];
        Self {
            child,
            store,
            cert_path,
            key_path,
            config_path,
            globals,
        }
    }
}

impl Drop for RemoteServeStore {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.store);
        let _ = std::fs::remove_file(&self.cert_path);
        let _ = std::fs::remove_file(&self.key_path);
        let _ = std::fs::remove_file(&self.config_path);
    }
}

fn loom<const N: usize>(args: [&str; N]) -> Result<String, String> {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(args)
        .output()
        .map_err(|error| format!("spawn loom: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "loom {} failed with {}\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn loom_output<const N: usize>(args: [&str; N]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("spawn loom: {error}"));
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn loom_with_globals(globals: &[String], args: &[&str]) -> Result<String, String> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_loom"));
    command.args(globals);
    command.args(args);
    let output = command
        .output()
        .map_err(|error| format!("spawn loom: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "loom {} {} failed with {}\nstdout:\n{}\nstderr:\n{}",
            globals.join(" "),
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn loom_output_with_globals(globals: &[String], args: &[&str]) -> (bool, String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_loom"));
    command.args(globals);
    command.args(args);
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("spawn loom: {error}"));
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn loom_output_env<const N: usize>(
    args: [&str; N],
    envs: &[(&str, &str)],
) -> (bool, String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_loom"));
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("spawn loom: {error}"));
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn read_cli_lock_token(path: &std::path::Path) -> loom_core::LockToken {
    use base64::Engine as _;

    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).expect("read lock token file"))
            .expect("decode lock token json");
    let encoded = value
        .get("token")
        .and_then(serde_json::Value::as_str)
        .expect("lock token field");
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .expect("decode lock token base64url");
    loom_wire::lock::lock_token_from_cbor(&bytes).expect("decode lock token")
}

#[test]
fn mu17g_f5a_c_cli_lock_session_files_preserve_generated_owner() {
    let mut store = DaemonStore::new("logical-lock-cli");
    loom(["store", "init", &store.path]).unwrap();
    let base = std::path::Path::new(&store.path)
        .parent()
        .unwrap()
        .join(format!("loom-lock-cli-{}", std::process::id()));
    let session = base.with_extension("session.json");
    let foreign_session = base.with_extension("foreign-session.json");
    let foreign_token = base.with_extension("foreign-lock.json");
    let token = base.with_extension("lock.json");
    let stale_session = base.with_extension("stale-session.json");
    for path in [
        &session,
        &foreign_session,
        &foreign_token,
        &token,
        &stale_session,
    ] {
        let _ = std::fs::remove_file(path);
    }
    let session_text = session.to_string_lossy().into_owned();
    let session_ref = format!("@{session_text}");
    let foreign_text = foreign_session.to_string_lossy().into_owned();
    let foreign_ref = format!("@{foreign_text}");
    let foreign_token_text = foreign_token.to_string_lossy().into_owned();
    let token_text = token.to_string_lossy().into_owned();
    let token_ref = format!("@{token_text}");
    let stale_text = stale_session.to_string_lossy().into_owned();
    let stale_ref = format!("@{stale_text}");

    loom([
        "daemon",
        "session",
        "open",
        &store.path,
        "--out",
        &session_text,
    ])
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&session).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    loom([
        "lock",
        "acquire",
        &store.path,
        "resource",
        "--session",
        &session_ref,
        "--out",
        &token_text,
    ])
    .unwrap();
    loom([
        "lock",
        "refresh",
        &store.path,
        "--session",
        &session_ref,
        "--token",
        &token_ref,
        "--out",
        &token_text,
    ])
    .unwrap();
    let first_token = read_cli_lock_token(&token);

    loom([
        "daemon",
        "session",
        "open",
        &store.path,
        "--out",
        &foreign_text,
    ])
    .unwrap();
    let (success, _, stderr) = loom_output([
        "lock",
        "acquire",
        &store.path,
        "resource",
        "--session",
        &foreign_ref,
        "--out",
        &foreign_token_text,
        "--no-wait",
    ]);
    assert!(!success);
    assert!(stderr.contains("Locked"), "{stderr}");
    let (success, _, stderr) = loom_output([
        "lock",
        "release",
        &store.path,
        "--session",
        &foreign_ref,
        "--token",
        &token_ref,
    ]);
    assert!(!success);
    assert!(stderr.contains("PermissionDenied"), "{stderr}");

    loom([
        "lock",
        "release",
        &store.path,
        "--session",
        &session_ref,
        "--token",
        &token_ref,
    ])
    .unwrap();
    assert!(!token.exists());
    loom([
        "lock",
        "acquire",
        &store.path,
        "resource",
        "--session",
        &foreign_ref,
        "--out",
        &token_text,
    ])
    .unwrap();
    let second_token = read_cli_lock_token(&token);
    assert!(second_token.fence > first_token.fence);
    loom([
        "lock",
        "release",
        &store.path,
        "--session",
        &foreign_ref,
        "--token",
        &token_ref,
    ])
    .unwrap();
    loom([
        "daemon",
        "session",
        "close",
        &store.path,
        "--session",
        &session_ref,
    ])
    .unwrap();
    assert!(!session.exists());
    loom([
        "daemon",
        "session",
        "close",
        &store.path,
        "--session",
        &foreign_ref,
    ])
    .unwrap();
    assert!(!foreign_session.exists());

    loom([
        "daemon",
        "session",
        "open",
        &store.path,
        "--out",
        &stale_text,
    ])
    .unwrap();
    store.stop();
    store.start();
    let (success, _, stderr) = loom_output([
        "lock",
        "acquire",
        &store.path,
        "restart-key",
        "--session",
        &stale_ref,
        "--out",
        &token_text,
    ]);
    assert!(!success);
    assert!(stderr.contains("session credential"), "{stderr}");

    let _ = std::fs::remove_file(stale_session);
    store.stop();
}

fn prepare_mu17g_d1_store(path: &str) {
    drop(FileStore::create_with_profile(path, Algo::Blake3).unwrap());
    loom(["workspace", "create", path, "files", "--facet", "files"]).unwrap();
    loom(["workspace", "create", path, "docs", "--facet", "document"]).unwrap();
    loom(["workspace", "create", path, "pages"]).unwrap();
    loom(["workspace", "create", path, "scratch"]).unwrap();
}

fn prepare_mu17g_b_store(path: &str) {
    drop(FileStore::create_with_profile(path, Algo::Blake3).unwrap());
}

fn copy_store_bytes(source: &str, target: &str) {
    let _ = std::fs::remove_file(target);
    std::fs::copy(source, target)
        .unwrap_or_else(|error| panic!("copy prepared store from {source} to {target}: {error}"));
}

fn temp_text_file(tag: &str, body: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "loom-daemon-cli-authority-{tag}-{}-{}.txt",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, body).unwrap();
    path.to_string_lossy().into_owned()
}

fn temp_bytes_file(tag: &str, body: &[u8]) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "loom-daemon-cli-authority-{tag}-{}-{}.bin",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, body).unwrap();
    path.to_string_lossy().into_owned()
}

fn cbor_file(tag: &str, value: CborValue) -> String {
    temp_bytes_file(
        tag,
        &loom_codec::encode(&value).expect("canonical test CBOR"),
    )
}

fn vector_f32_file(tag: &str, values: &[f32]) -> String {
    let bytes = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<_>>();
    temp_bytes_file(tag, &bytes)
}

fn mu17g_b_columnar_bytes() -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
    let columns = loom_wire::columnar::columns_to_cbor(vec![
        ("id".to_string(), ColumnType::Int),
        ("name".to_string(), ColumnType::Text),
    ]);
    let row = loom_wire::columnar::values_to_cbor(vec![
        LoomValue::Int(7),
        LoomValue::Text("ada".to_string()),
    ]);
    let select_columns =
        loom_codec::encode(&CborValue::Array(vec![CborValue::Text("name".to_string())]))
            .expect("select columns CBOR");
    let aggregate = loom_codec::encode(&CborValue::Array(vec![CborValue::Array(vec![
        CborValue::Uint(0),
        CborValue::Null,
    ])]))
    .expect("aggregate CBOR");
    let mut set = ColumnarSet::new(
        vec![
            ("id".to_string(), ColumnType::Int),
            ("name".to_string(), ColumnType::Text),
        ],
        0,
    )
    .unwrap();
    set.append_row(vec![
        LoomValue::Int(9),
        LoomValue::Text("grace".to_string()),
    ])
    .unwrap();
    let arrow = loom_core::columnar_to_arrow_ipc(&set).unwrap();
    let parquet = loom_core::columnar_to_parquet(&set).unwrap();
    (columns, row, select_columns, aggregate, arrow, parquet)
}

fn mu17g_b_dataframe_plan() -> Vec<u8> {
    let source = DataframeSourceBinding::new(
        "events",
        DataframeSourceKind::Columnar,
        "events",
        DataframeInputFormat::Native,
    );
    DataframePlan::new(vec![source])
        .unwrap()
        .with_operations(vec![DataframeOperation::Scan {
            source: "events".to_string(),
        }])
        .unwrap()
        .with_materialization(DataframeMaterialization::new(
            DataframeMaterializationTarget::Cas,
            None,
            DataframeInputFormat::Json,
        ))
        .unwrap()
        .encode()
}

fn mu17g_b_search_mapping_file(tag: &str) -> String {
    cbor_file(
        tag,
        CborValue::Map(vec![(
            CborValue::Text("title".to_string()),
            CborValue::Array(vec![
                CborValue::Uint(0),
                CborValue::Bool(true),
                CborValue::Bool(false),
            ]),
        )]),
    )
}

fn mu17g_b_search_doc_file(tag: &str, title: &str) -> String {
    cbor_file(
        tag,
        CborValue::Map(vec![(
            CborValue::Text("title".to_string()),
            CborValue::Text(title.to_string()),
        )]),
    )
}

fn mu17g_b_search_query_file(tag: &str) -> String {
    cbor_file(
        tag,
        CborValue::Array(vec![
            CborValue::Array(vec![
                CborValue::Uint(0),
                CborValue::Text("title".to_string()),
                CborValue::Text("Ada".to_string()),
            ]),
            CborValue::Uint(10),
            CborValue::Uint(0),
        ]),
    )
}

fn mu17h_normalized_json(label: &str, output: String) -> Vec<u8> {
    fn normalize_string(text: &mut String) {
        if WorkspaceId::parse(text).is_ok() {
            *text = "<uuid>".to_string();
        } else if Digest::parse(text).is_ok() {
            *text = "<digest>".to_string();
        } else if text.starts_with("entity-tag:") {
            *text = "<entity-tag>".to_string();
        } else if let Some((prefix, suffix)) = text.split_once(':')
            && WorkspaceId::parse(prefix).is_ok()
            && suffix.chars().all(|ch| ch.is_ascii_digit())
        {
            *text = "<operation-id>".to_string();
        } else if let Some(rest) = text.strip_prefix("pages.")
            && let Some((workspace, suffix)) = rest.split_once(".structure.")
            && WorkspaceId::parse(workspace).is_ok()
        {
            *text = format!("pages.<uuid>.structure.{suffix}");
        }
    }

    fn normalize(value: &mut serde_json::Value, key: Option<&str>) {
        match value {
            serde_json::Value::String(text) => normalize_string(text),
            serde_json::Value::Number(_)
                if key.is_some_and(|key| {
                    key.ends_with("_at_ms")
                        || key.ends_with("_timestamp_ms")
                        || key == "timestamp_ms"
                }) =>
            {
                *value = serde_json::Value::String("<timestamp>".to_string());
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    normalize(item, None);
                }
            }
            serde_json::Value::Object(map) => {
                for (key, item) in map {
                    normalize(item, Some(key));
                }
            }
            _ => {}
        }
    }

    fn require_fields(value: &serde_json::Value, fields: &[&str], label: &str) {
        let object = value
            .as_object()
            .unwrap_or_else(|| panic!("{label} JSON must be an object: {value}"));
        for field in fields {
            assert!(
                object.contains_key(*field),
                "{label} JSON missing {field:?}: {value}"
            );
        }
    }

    let mut value: serde_json::Value =
        serde_json::from_str(&output).unwrap_or_else(|error| panic!("{label} JSON: {error}"));
    match label {
        "vector.workspace.configure.json" => {
            require_fields(&value, &["workspace", "embedding-instance"], label);
            assert_eq!(value["embedding-instance"], "embedder");
        }
        "fts.status.json" => require_fields(
            &value,
            &[
                "workspace",
                "collection",
                "source_digest",
                "engine_version",
                "status",
            ],
            label,
        ),
        label if label.starts_with("pages.space-list.json") => {
            let spaces = value
                .as_array()
                .unwrap_or_else(|| panic!("{label} JSON must be an array: {value}"));
            assert!(!spaces.is_empty(), "{label} JSON must contain a space");
            require_fields(
                &spaces[0],
                &["space_id", "title", "archived", "profile_root"],
                label,
            );
        }
        label if label.starts_with("pages.space-") => require_fields(
            &value,
            &["space_id", "title", "archived", "profile_root"],
            label,
        ),
        "pages.create.json" | "pages.get.json" => require_fields(
            &value,
            &["page_id", "space_id", "title", "status", "profile_root"],
            label,
        ),
        "pages.update.json" => require_fields(
            &value,
            &["page_id", "status", "updated_at_ms", "profile_root"],
            label,
        ),
        "pages.publish.json" => {
            require_fields(&value, &["page_id", "outcome", "profile_root"], label)
        }
        "pages.history.json" => {
            let history = value
                .as_array()
                .unwrap_or_else(|| panic!("{label} JSON must be an array: {value}"));
            assert!(!history.is_empty(), "{label} JSON must contain history");
        }
        "pages.structure-create.json" | "pages.structure-get.json" => require_fields(
            &value,
            &["structure", "nodes", "edges", "graph_collection"],
            label,
        ),
        "pages.structure-add-node.json"
        | "pages.structure-update-node.json"
        | "pages.structure-bind.json" => require_fields(
            &value,
            &["structure_id", "node_id", "kind", "label", "profile_root"],
            label,
        ),
        "pages.structure-move-node.json" => require_fields(
            &value,
            &["structure_id", "node_id", "label", "profile_root"],
            label,
        ),
        "pages.structure-link-node.json" => require_fields(
            &value,
            &[
                "structure_id",
                "edge_id",
                "src_node_id",
                "dst_node_id",
                "label",
                "profile_root",
            ],
            label,
        ),
        "pages.structure-decompose-to-tickets.json" => require_fields(
            &value,
            &[
                "workspace_id",
                "structure_id",
                "tickets",
                "implemented_by_edges",
                "graph_collection",
            ],
            label,
        ),
        "inference.instance.list.json" | "inference.instance.list.filtered.json" => {
            assert!(value.is_array(), "{label} JSON must be an array: {value}");
        }
        "inference.instance.show.json" | "inference.instance.show.resolved.json" => {
            require_fields(&value, &["instance", "refs"], label)
        }
        _ => panic!("unclassified MU-17h JSON report label {label}"),
    }
    normalize(&mut value, None);
    serde_json::to_vec(&value).expect("normalized MU-17h JSON")
}

fn mu17h_normalized_vector_workspace_text(output: String) -> Vec<u8> {
    let mut fields = output.trim_end().split('\t');
    let kind = fields.next().expect("vector workspace text kind");
    let workspace = fields.next().expect("vector workspace text id");
    let binding = fields.next().expect("vector workspace text binding");
    assert_eq!(kind, "vector_workspace");
    assert!(WorkspaceId::parse(workspace).is_ok());
    assert_eq!(binding, "embedding_instance=embedder");
    format!("{kind}\t<uuid>\t{binding}\n").into_bytes()
}

fn mu17g_c_normalized_ics(bytes: &[u8]) -> Vec<u8> {
    String::from_utf8(bytes.to_vec())
        .expect("ics utf8")
        .lines()
        .filter(|line| !line.starts_with("DTSTAMP:"))
        .collect::<Vec<_>>()
        .join("\r\n")
        .into_bytes()
}

fn document_put_text(store: &str, id: &str, body: &str) -> String {
    let input = temp_text_file(id, body);
    let output = loom(["document", "put-text", store, "main", "mu15d", id, &input]).unwrap();
    let _ = std::fs::remove_file(input);
    output
}

fn mu17g_a_foundational_cli_report(
    globals: &[String],
    store: &str,
    tag: &str,
) -> Vec<(String, Vec<u8>)> {
    let payload = temp_bytes_file(&format!("{tag}-payload"), b"alpha");
    let payload_b = temp_bytes_file(&format!("{tag}-payload-b"), b"beta");
    let key_a = temp_bytes_file(
        &format!("{tag}-key-a"),
        &loom_core::kv::key_to_cbor(&loom_core::Value::Text("a".to_string())),
    );
    let key_z = temp_bytes_file(
        &format!("{tag}-key-z"),
        &loom_core::kv::key_to_cbor(&loom_core::Value::Text("z".to_string())),
    );
    let out = temp_bytes_file(&format!("{tag}-out"), b"");
    let out2 = temp_bytes_file(&format!("{tag}-out2"), b"");
    let mut report = Vec::new();

    let run = |args: &[&str]| loom_with_globals(globals, args).unwrap();
    let capture_error = |label: &str, args: &[&str], report: &mut Vec<(String, Vec<u8>)>| {
        let (ok, stdout, stderr) = loom_output_with_globals(globals, args);
        assert!(!ok, "{label} unexpectedly succeeded:\n{stdout}");
        report.push((format!("{label}.stderr"), stderr.into_bytes()));
    };

    let cas_digest = run(&["cas", "put", store, "mu17g-cas", &payload]);
    let cas_digest = cas_digest.trim().to_string();
    report.push(("cas.put".to_string(), cas_digest.as_bytes().to_vec()));
    report.push((
        "cas.has.present".to_string(),
        run(&["cas", "has", store, "mu17g-cas", &cas_digest]).into_bytes(),
    ));
    report.push((
        "cas.has.absent".to_string(),
        run(&[
            "cas",
            "has",
            store,
            "mu17g-cas",
            "blake3:0000000000000000000000000000000000000000000000000000000000000000",
        ])
        .into_bytes(),
    ));
    run(&["cas", "get", store, "mu17g-cas", &cas_digest, "--out", &out]);
    report.push(("cas.get".to_string(), std::fs::read(&out).unwrap()));
    report.push((
        "cas.list".to_string(),
        run(&["cas", "list", store, "mu17g-cas"]).into_bytes(),
    ));
    report.push((
        "cas.delete".to_string(),
        run(&["cas", "delete", store, "mu17g-cas", &cas_digest]).into_bytes(),
    ));
    report.push((
        "cas.delete.absent".to_string(),
        run(&["cas", "delete", store, "mu17g-cas", &cas_digest]).into_bytes(),
    ));
    capture_error(
        "cas.get.absent",
        &["cas", "get", store, "mu17g-cas", &cas_digest],
        &mut report,
    );

    run(&["kv", "put", store, "mu17g-kv", "settings", &key_a, &payload]);
    run(&[
        "kv", "get", store, "mu17g-kv", "settings", &key_a, "--out", &out,
    ]);
    report.push(("kv.get".to_string(), std::fs::read(&out).unwrap()));
    run(&["kv", "list", store, "mu17g-kv", "settings", "--out", &out]);
    report.push(("kv.list".to_string(), std::fs::read(&out).unwrap()));
    run(&[
        "kv", "range", store, "mu17g-kv", "settings", &key_a, &key_z, "--out", &out2,
    ]);
    report.push(("kv.range".to_string(), std::fs::read(&out2).unwrap()));
    report.push((
        "kv.delete".to_string(),
        run(&["kv", "delete", store, "mu17g-kv", "settings", &key_a]).into_bytes(),
    ));
    capture_error(
        "kv.get.absent",
        &["kv", "get", store, "mu17g-kv", "settings", &key_a],
        &mut report,
    );

    report.push((
        "queue.append.0".to_string(),
        run(&["queue", "append", store, "mu17g-queue", "events", &payload]).into_bytes(),
    ));
    report.push((
        "queue.append.1".to_string(),
        run(&[
            "queue",
            "append",
            store,
            "mu17g-queue",
            "events",
            &payload_b,
        ])
        .into_bytes(),
    ));
    run(&[
        "queue",
        "get",
        store,
        "mu17g-queue",
        "events",
        "0",
        "--out",
        &out,
    ]);
    report.push(("queue.get".to_string(), std::fs::read(&out).unwrap()));
    report.push((
        "queue.len".to_string(),
        run(&["queue", "len", store, "mu17g-queue", "events"]).into_bytes(),
    ));
    run(&[
        "queue",
        "range",
        store,
        "mu17g-queue",
        "events",
        "0",
        "2",
        "--out",
        &out,
    ]);
    report.push(("queue.range".to_string(), std::fs::read(&out).unwrap()));
    run(&[
        "queue",
        "read",
        store,
        "mu17g-queue",
        "events",
        "worker",
        "2",
        "--out",
        &out2,
    ]);
    report.push(("queue.read".to_string(), std::fs::read(&out2).unwrap()));
    run(&[
        "queue",
        "advance",
        store,
        "mu17g-queue",
        "events",
        "worker",
        "1",
    ]);
    report.push((
        "queue.position.advance".to_string(),
        run(&[
            "queue",
            "position",
            store,
            "mu17g-queue",
            "events",
            "worker",
        ])
        .into_bytes(),
    ));
    run(&[
        "queue",
        "reset",
        store,
        "mu17g-queue",
        "events",
        "worker",
        "0",
    ]);
    report.push((
        "queue.position.reset".to_string(),
        run(&[
            "queue",
            "position",
            store,
            "mu17g-queue",
            "events",
            "worker",
        ])
        .into_bytes(),
    ));
    capture_error(
        "queue.get.absent",
        &["queue", "get", store, "mu17g-queue", "events", "9"],
        &mut report,
    );

    run(&[
        "time-series",
        "put",
        store,
        "mu17g-ts",
        "cpu",
        "100",
        &payload,
    ]);
    run(&[
        "time-series",
        "get",
        store,
        "mu17g-ts",
        "cpu",
        "100",
        "--out",
        &out,
    ]);
    report.push(("time-series.get".to_string(), std::fs::read(&out).unwrap()));
    run(&[
        "time-series",
        "latest",
        store,
        "mu17g-ts",
        "cpu",
        "--out",
        &out2,
    ]);
    report.push((
        "time-series.latest".to_string(),
        std::fs::read(&out2).unwrap(),
    ));
    run(&[
        "time-series",
        "range",
        store,
        "mu17g-ts",
        "cpu",
        "0",
        "200",
        "--out",
        &out,
    ]);
    report.push((
        "time-series.range".to_string(),
        std::fs::read(&out).unwrap(),
    ));
    capture_error(
        "time-series.get.absent",
        &["time-series", "get", store, "mu17g-ts", "cpu", "999"],
        &mut report,
    );

    report.push((
        "ledger.append.0".to_string(),
        run(&[
            "ledger",
            "append",
            store,
            "--workspace",
            "mu17g-ledger",
            "audit",
            &payload,
        ])
        .into_bytes(),
    ));
    run(&[
        "ledger",
        "get",
        store,
        "--workspace",
        "mu17g-ledger",
        "audit",
        "0",
        "--out",
        &out,
    ]);
    report.push(("ledger.get".to_string(), std::fs::read(&out).unwrap()));
    report.push((
        "ledger.len".to_string(),
        run(&[
            "ledger",
            "len",
            store,
            "--workspace",
            "mu17g-ledger",
            "audit",
        ])
        .into_bytes(),
    ));
    report.push((
        "ledger.head".to_string(),
        run(&[
            "ledger",
            "head",
            store,
            "--workspace",
            "mu17g-ledger",
            "audit",
        ])
        .into_bytes(),
    ));
    report.push((
        "ledger.verify".to_string(),
        run(&[
            "ledger",
            "verify",
            store,
            "--workspace",
            "mu17g-ledger",
            "audit",
        ])
        .into_bytes(),
    ));
    capture_error(
        "ledger.get.absent",
        &[
            "ledger",
            "get",
            store,
            "--workspace",
            "mu17g-ledger",
            "audit",
            "9",
        ],
        &mut report,
    );

    for path in [payload, payload_b, key_a, key_z, out, out2] {
        let _ = std::fs::remove_file(path);
    }
    report
}

fn mu17g_b_analytical_cli_report(
    globals: &[String],
    store: &str,
    tag: &str,
) -> Vec<(String, Vec<u8>)> {
    let mut report = Vec::new();
    let run = |args: &[&str]| loom_with_globals(globals, args).unwrap();
    let capture_error = |label: &str, args: &[&str], report: &mut Vec<(String, Vec<u8>)>| {
        let (ok, stdout, stderr) = loom_output_with_globals(globals, args);
        assert!(!ok, "{label} unexpectedly succeeded:\n{stdout}");
        report.push((format!("{label}.stderr"), stderr.into_bytes()));
    };
    let out = temp_bytes_file(&format!("{tag}-out"), b"");
    let out2 = temp_bytes_file(&format!("{tag}-out2"), b"");
    let graph_query = "MATCH (n) RETURN n";

    run(&[
        "graph",
        "upsert-node",
        store,
        "--workspace",
        "mu17gb-graph",
        "main",
        "a",
    ]);
    run(&[
        "graph",
        "upsert-node",
        store,
        "--workspace",
        "mu17gb-graph",
        "main",
        "b",
    ]);
    run(&[
        "graph",
        "upsert-edge",
        store,
        "--workspace",
        "mu17gb-graph",
        "main",
        "ab",
        "a",
        "b",
        "knows",
    ]);
    run(&[
        "graph",
        "get-node",
        store,
        "--workspace",
        "mu17gb-graph",
        "main",
        "a",
        "--out",
        &out,
    ]);
    report.push(("graph.get-node".to_string(), std::fs::read(&out).unwrap()));
    run(&[
        "graph",
        "get-edge",
        store,
        "--workspace",
        "mu17gb-graph",
        "main",
        "ab",
        "--out",
        &out,
    ]);
    report.push(("graph.get-edge".to_string(), std::fs::read(&out).unwrap()));
    run(&[
        "graph",
        "neighbors",
        store,
        "--workspace",
        "mu17gb-graph",
        "main",
        "a",
        "--out",
        &out,
    ]);
    report.push(("graph.neighbors".to_string(), std::fs::read(&out).unwrap()));
    run(&[
        "graph",
        "out-edges",
        store,
        "--workspace",
        "mu17gb-graph",
        "main",
        "a",
        "--out",
        &out,
    ]);
    report.push(("graph.out-edges".to_string(), std::fs::read(&out).unwrap()));
    run(&[
        "graph",
        "in-edges",
        store,
        "--workspace",
        "mu17gb-graph",
        "main",
        "b",
        "--out",
        &out,
    ]);
    report.push(("graph.in-edges".to_string(), std::fs::read(&out).unwrap()));
    run(&[
        "graph",
        "reachable",
        store,
        "--workspace",
        "mu17gb-graph",
        "main",
        "a",
        "--out",
        &out,
    ]);
    report.push(("graph.reachable".to_string(), std::fs::read(&out).unwrap()));
    run(&[
        "graph",
        "shortest-path",
        store,
        "--workspace",
        "mu17gb-graph",
        "main",
        "a",
        "b",
        "--out",
        &out,
    ]);
    report.push((
        "graph.shortest-path".to_string(),
        std::fs::read(&out).unwrap(),
    ));
    run(&[
        "graph",
        "query",
        store,
        "--workspace",
        "mu17gb-graph",
        "main",
        graph_query,
        "--out",
        &out,
    ]);
    report.push(("graph.query".to_string(), std::fs::read(&out).unwrap()));
    run(&[
        "graph",
        "explain-query",
        store,
        "--workspace",
        "mu17gb-graph",
        "main",
        graph_query,
        "--out",
        &out,
    ]);
    report.push((
        "graph.explain-query".to_string(),
        std::fs::read(&out).unwrap(),
    ));
    report.push((
        "graph.remove-edge".to_string(),
        run(&[
            "graph",
            "remove-edge",
            store,
            "--workspace",
            "mu17gb-graph",
            "main",
            "ab",
        ])
        .into_bytes(),
    ));
    report.push((
        "graph.remove-edge.absent".to_string(),
        run(&[
            "graph",
            "remove-edge",
            store,
            "--workspace",
            "mu17gb-graph",
            "main",
            "ab",
        ])
        .into_bytes(),
    ));
    report.push((
        "graph.remove-node".to_string(),
        run(&[
            "graph",
            "remove-node",
            store,
            "--workspace",
            "mu17gb-graph",
            "main",
            "a",
            "--cascade",
        ])
        .into_bytes(),
    ));

    let vector = vector_f32_file(&format!("{tag}-vector"), &[1.0, 0.0]);
    let vector_b = vector_f32_file(&format!("{tag}-vector-b"), &[0.0, 1.0]);
    let source_text = temp_text_file(&format!("{tag}-vector-source"), "Ada writes systems.");
    run(&[
        "workspace",
        "create",
        store,
        "mu17gb-vector",
        "--facet",
        "inference",
    ]);
    run(&[
        "vector",
        "create",
        store,
        "--workspace",
        "mu17gb-vector",
        "main",
        "--dim",
        "2",
        "--metric",
        "cosine",
    ]);
    run(&[
        "vector",
        "upsert",
        store,
        "--workspace",
        "mu17gb-vector",
        "main",
        "v1",
        &vector,
    ]);
    run(&[
        "vector",
        "upsert-source",
        store,
        "--workspace",
        "mu17gb-vector",
        "main",
        "v2",
        &vector_b,
        "--source",
        &source_text,
        "--model-id",
        "fixture-model",
    ]);
    run(&[
        "vector",
        "get",
        store,
        "--workspace",
        "mu17gb-vector",
        "main",
        "v1",
        "--out",
        &out,
    ]);
    report.push(("vector.get".to_string(), std::fs::read(&out).unwrap()));
    run(&[
        "vector",
        "source",
        store,
        "--workspace",
        "mu17gb-vector",
        "main",
        "v2",
        "--out",
        &out,
    ]);
    report.push(("vector.source".to_string(), std::fs::read(&out).unwrap()));
    run(&[
        "vector",
        "ids",
        store,
        "--workspace",
        "mu17gb-vector",
        "main",
        "--out",
        &out,
    ]);
    report.push(("vector.ids".to_string(), std::fs::read(&out).unwrap()));
    report.push((
        "vector.create-index".to_string(),
        run(&[
            "vector",
            "create-index",
            store,
            "--workspace",
            "mu17gb-vector",
            "main",
            "kind",
        ])
        .into_bytes(),
    ));
    run(&[
        "vector",
        "index-keys",
        store,
        "--workspace",
        "mu17gb-vector",
        "main",
        "--out",
        &out,
    ]);
    report.push((
        "vector.index-keys".to_string(),
        std::fs::read(&out).unwrap(),
    ));
    run(&[
        "vector",
        "search",
        store,
        "--workspace",
        "mu17gb-vector",
        "main",
        &vector,
        "--k",
        "2",
        "--out",
        &out,
    ]);
    report.push(("vector.search".to_string(), std::fs::read(&out).unwrap()));
    report.push((
        "vector.drop-index".to_string(),
        run(&[
            "vector",
            "drop-index",
            store,
            "--workspace",
            "mu17gb-vector",
            "main",
            "kind",
        ])
        .into_bytes(),
    ));
    report.push((
        "vector.delete".to_string(),
        run(&[
            "vector",
            "delete",
            store,
            "--workspace",
            "mu17gb-vector",
            "main",
            "v1",
        ])
        .into_bytes(),
    ));
    report.push((
        "vector.delete.absent".to_string(),
        run(&[
            "vector",
            "delete",
            store,
            "--workspace",
            "mu17gb-vector",
            "main",
            "v1",
        ])
        .into_bytes(),
    ));
    capture_error(
        "vector.get.absent",
        &[
            "vector",
            "get",
            store,
            "--workspace",
            "mu17gb-vector",
            "main",
            "missing",
        ],
        &mut report,
    );

    run(&[
        "inference",
        "instance",
        "create",
        store,
        "mu17gb-vector",
        "embedder",
        "--model",
        "sentence-transformers/all-MiniLM-L6-v2",
        "--kind",
        "text-embedding",
    ]);
    report.push((
        "vector.workspace.configure.text".to_string(),
        mu17h_normalized_vector_workspace_text(run(&[
            "vector",
            "workspace",
            "configure",
            store,
            "mu17gb-vector",
            "--embedding-instance",
            "embedder",
        ])),
    ));
    report.push((
        "vector.workspace.configure".to_string(),
        mu17h_normalized_json(
            "vector.workspace.configure.json",
            run(&[
                "vector",
                "workspace",
                "configure",
                store,
                "mu17gb-vector",
                "--embedding-instance",
                "embedder",
                "--format",
                "json",
            ]),
        ),
    ));

    let mapping = mu17g_b_search_mapping_file(&format!("{tag}-search-mapping"));
    let doc = mu17g_b_search_doc_file(&format!("{tag}-search-doc"), "Ada writes systems");
    let request = mu17g_b_search_query_file(&format!("{tag}-search-query"));
    run(&[
        "fts",
        "create",
        store,
        "--workspace",
        "mu17gb-search",
        "main",
        &mapping,
    ]);
    run(&[
        "fts",
        "index",
        store,
        "--workspace",
        "mu17gb-search",
        "main",
        "doc-1",
        &doc,
    ]);
    run(&[
        "fts",
        "get",
        store,
        "--workspace",
        "mu17gb-search",
        "main",
        "doc-1",
        "--out",
        &out,
    ]);
    report.push(("fts.get".to_string(), std::fs::read(&out).unwrap()));
    run(&[
        "fts",
        "ids",
        store,
        "--workspace",
        "mu17gb-search",
        "main",
        "--out",
        &out,
    ]);
    report.push(("fts.ids".to_string(), std::fs::read(&out).unwrap()));
    run(&[
        "fts",
        "query",
        store,
        "--workspace",
        "mu17gb-search",
        "main",
        &request,
        "--out",
        &out,
    ]);
    report.push(("fts.query".to_string(), std::fs::read(&out).unwrap()));
    report.push((
        "fts.status.text".to_string(),
        run(&[
            "fts",
            "status",
            store,
            "--workspace",
            "mu17gb-search",
            "main",
            "--engine-version",
            "fixture",
        ])
        .into_bytes(),
    ));
    report.push((
        "fts.status".to_string(),
        mu17h_normalized_json(
            "fts.status.json",
            run(&[
                "fts",
                "status",
                store,
                "--workspace",
                "mu17gb-search",
                "main",
                "--engine-version",
                "fixture",
                "--format",
                "json",
            ]),
        ),
    ));
    run(&[
        "fts",
        "remap",
        store,
        "--workspace",
        "mu17gb-search",
        "main",
        &mapping,
    ]);
    report.push((
        "fts.delete".to_string(),
        run(&[
            "fts",
            "delete",
            store,
            "--workspace",
            "mu17gb-search",
            "main",
            "doc-1",
        ])
        .into_bytes(),
    ));
    report.push((
        "fts.delete.absent".to_string(),
        run(&[
            "fts",
            "delete",
            store,
            "--workspace",
            "mu17gb-search",
            "main",
            "doc-1",
        ])
        .into_bytes(),
    ));
    capture_error(
        "fts.get.absent",
        &[
            "fts",
            "get",
            store,
            "--workspace",
            "mu17gb-search",
            "main",
            "doc-1",
        ],
        &mut report,
    );

    let (columns, row, select_columns, aggregates, arrow, parquet) = mu17g_b_columnar_bytes();
    let columns_path = temp_bytes_file(&format!("{tag}-columns"), &columns);
    let row_path = temp_bytes_file(&format!("{tag}-row"), &row);
    let select_path = temp_bytes_file(&format!("{tag}-select"), &select_columns);
    let aggregates_path = temp_bytes_file(&format!("{tag}-aggregates"), &aggregates);
    let arrow_path = temp_bytes_file(&format!("{tag}-arrow"), &arrow);
    let parquet_path = temp_bytes_file(&format!("{tag}-parquet"), &parquet);
    run(&[
        "columnar",
        "create",
        store,
        "--workspace",
        "mu17gb-columnar",
        "events",
        &columns_path,
    ]);
    run(&[
        "columnar",
        "append",
        store,
        "--workspace",
        "mu17gb-columnar",
        "events",
        &row_path,
    ]);
    run(&[
        "columnar",
        "scan",
        store,
        "--workspace",
        "mu17gb-columnar",
        "events",
        "--out",
        &out,
    ]);
    report.push(("columnar.scan".to_string(), std::fs::read(&out).unwrap()));
    run(&[
        "columnar",
        "columns",
        store,
        "--workspace",
        "mu17gb-columnar",
        "events",
        "--out",
        &out,
    ]);
    report.push(("columnar.columns".to_string(), std::fs::read(&out).unwrap()));
    report.push((
        "columnar.rows".to_string(),
        run(&[
            "columnar",
            "rows",
            store,
            "--workspace",
            "mu17gb-columnar",
            "events",
        ])
        .into_bytes(),
    ));
    run(&[
        "columnar",
        "inspect",
        store,
        "--workspace",
        "mu17gb-columnar",
        "events",
        "--out",
        &out,
    ]);
    report.push(("columnar.inspect".to_string(), std::fs::read(&out).unwrap()));
    report.push((
        "columnar.source-digest".to_string(),
        run(&[
            "columnar",
            "source-digest",
            store,
            "--workspace",
            "mu17gb-columnar",
            "events",
        ])
        .into_bytes(),
    ));
    run(&[
        "columnar",
        "select",
        store,
        "--workspace",
        "mu17gb-columnar",
        "events",
        &select_path,
        "--out",
        &out,
    ]);
    report.push(("columnar.select".to_string(), std::fs::read(&out).unwrap()));
    run(&[
        "columnar",
        "aggregate",
        store,
        "--workspace",
        "mu17gb-columnar",
        "events",
        &aggregates_path,
        "--out",
        &out,
    ]);
    report.push((
        "columnar.aggregate".to_string(),
        std::fs::read(&out).unwrap(),
    ));
    report.push((
        "columnar.compact".to_string(),
        run(&[
            "columnar",
            "compact",
            store,
            "--workspace",
            "mu17gb-columnar",
            "events",
        ])
        .into_bytes(),
    ));
    run(&[
        "columnar",
        "import-arrow",
        store,
        "--workspace",
        "mu17gb-columnar",
        "arrow",
        &arrow_path,
    ]);
    run(&[
        "columnar",
        "scan",
        store,
        "--workspace",
        "mu17gb-columnar",
        "arrow",
        "--out",
        &out,
    ]);
    report.push((
        "columnar.import-arrow.scan".to_string(),
        std::fs::read(&out).unwrap(),
    ));
    run(&[
        "columnar",
        "import-parquet",
        store,
        "--workspace",
        "mu17gb-columnar",
        "parquet",
        &parquet_path,
    ]);
    run(&[
        "columnar",
        "scan",
        store,
        "--workspace",
        "mu17gb-columnar",
        "parquet",
        "--out",
        &out,
    ]);
    report.push((
        "columnar.import-parquet.scan".to_string(),
        std::fs::read(&out).unwrap(),
    ));

    let dataframe_plan =
        temp_bytes_file(&format!("{tag}-dataframe-plan"), &mu17g_b_dataframe_plan());
    run(&[
        "dataframe",
        "create",
        store,
        "--workspace",
        "mu17gb-columnar",
        "df",
        &dataframe_plan,
    ]);
    run(&[
        "dataframe",
        "collect",
        store,
        "--workspace",
        "mu17gb-columnar",
        "df",
        "--out",
        &out,
    ]);
    report.push((
        "dataframe.collect".to_string(),
        std::fs::read(&out).unwrap(),
    ));
    run(&[
        "dataframe",
        "preview",
        store,
        "--workspace",
        "mu17gb-columnar",
        "df",
        "--rows",
        "1",
        "--out",
        &out,
    ]);
    report.push((
        "dataframe.preview".to_string(),
        std::fs::read(&out).unwrap(),
    ));
    report.push((
        "dataframe.plan-digest".to_string(),
        run(&[
            "dataframe",
            "plan-digest",
            store,
            "--workspace",
            "mu17gb-columnar",
            "df",
        ])
        .into_bytes(),
    ));
    run(&[
        "dataframe",
        "source-digests",
        store,
        "--workspace",
        "mu17gb-columnar",
        "df",
        "--out",
        &out,
    ]);
    report.push((
        "dataframe.source-digests".to_string(),
        std::fs::read(&out).unwrap(),
    ));
    report.push((
        "dataframe.materialize".to_string(),
        run(&[
            "dataframe",
            "materialize",
            store,
            "--workspace",
            "mu17gb-columnar",
            "df",
        ])
        .into_bytes(),
    ));

    for path in [
        out,
        out2,
        vector,
        vector_b,
        source_text,
        mapping,
        doc,
        request,
        columns_path,
        row_path,
        select_path,
        aggregates_path,
        arrow_path,
        parquet_path,
        dataframe_plan,
    ] {
        let _ = std::fs::remove_file(path);
    }
    report
}

fn mu17g_c_pim_cli_report(globals: &[String], store: &str, tag: &str) -> Vec<(String, Vec<u8>)> {
    let mut report = Vec::new();
    let run = |args: &[&str]| loom_with_globals(globals, args).unwrap();
    let capture_error = |label: &str, args: &[&str], report: &mut Vec<(String, Vec<u8>)>| {
        let (ok, stdout, stderr) = loom_output_with_globals(globals, args);
        assert!(!ok, "{label} unexpectedly succeeded:\n{stdout}");
        report.push((format!("{label}.stderr"), stderr.into_bytes()));
    };
    let out = temp_bytes_file(&format!("{tag}-pim-out"), b"");
    let out2 = temp_bytes_file(&format!("{tag}-pim-out2"), b"");

    let cal_entry =
        loom_core::calendar::CalendarEntry::event("evt-1", "Standup", "20240115T100000");
    let cal_entry_file = temp_bytes_file(&format!("{tag}-calendar-entry"), &cal_entry.encode());
    let cal_ics_file = temp_text_file(
        &format!("{tag}-calendar-ics"),
        concat!(
            "BEGIN:VCALENDAR\r\n",
            "VERSION:2.0\r\n",
            "BEGIN:VEVENT\r\n",
            "UID:evt-2\r\n",
            "DTSTART:20240116T100000\r\n",
            "SUMMARY:Review\r\n",
            "END:VEVENT\r\n",
            "END:VCALENDAR\r\n"
        ),
    );
    report.push((
        "calendar.create-collection".to_string(),
        run(&[
            "calendar",
            "create-collection",
            store,
            "mu17gc-pim",
            "alice",
            "work",
            "--display-name",
            "Work",
            "--component",
            "event",
        ])
        .into_bytes(),
    ));
    run(&[
        "calendar",
        "get-collection",
        store,
        "mu17gc-pim",
        "alice",
        "work",
        "--out",
        &out,
    ]);
    report.push((
        "calendar.get-collection".to_string(),
        std::fs::read(&out).unwrap(),
    ));
    report.push((
        "calendar.list-collections.stdout".to_string(),
        run(&["calendar", "list-collections", store, "mu17gc-pim", "alice"]).into_bytes(),
    ));
    run(&[
        "calendar",
        "list-collections",
        store,
        "mu17gc-pim",
        "alice",
        "--out",
        &out,
    ]);
    report.push((
        "calendar.list-collections.out".to_string(),
        std::fs::read(&out).unwrap(),
    ));
    report.push((
        "calendar.put-entry".to_string(),
        run(&[
            "calendar",
            "put-entry",
            store,
            "mu17gc-pim",
            "alice",
            "work",
            &cal_entry_file,
        ])
        .into_bytes(),
    ));
    report.push((
        "calendar.put-ics".to_string(),
        run(&[
            "calendar",
            "put-ics",
            store,
            "mu17gc-pim",
            "alice",
            "work",
            &cal_ics_file,
        ])
        .into_bytes(),
    ));
    for (label, args) in [
        (
            "calendar.get-entry",
            vec![
                "calendar",
                "get-entry",
                store,
                "mu17gc-pim",
                "alice",
                "work",
                "evt-1",
                "--out",
                &out,
            ],
        ),
        (
            "calendar.list-entries",
            vec![
                "calendar",
                "list-entries",
                store,
                "mu17gc-pim",
                "alice",
                "work",
                "--out",
                &out,
            ],
        ),
        (
            "calendar.range",
            vec![
                "calendar",
                "range",
                store,
                "mu17gc-pim",
                "alice",
                "work",
                "20240101T000000",
                "20240201T000000",
                "--out",
                &out,
            ],
        ),
        (
            "calendar.search",
            vec![
                "calendar",
                "search",
                store,
                "mu17gc-pim",
                "alice",
                "work",
                "--component",
                "event",
                "--text",
                "Standup",
                "--out",
                &out,
            ],
        ),
        (
            "calendar.to-ics",
            vec![
                "calendar",
                "to-ics",
                store,
                "mu17gc-pim",
                "alice",
                "work",
                "evt-1",
                "--out",
                &out,
            ],
        ),
    ] {
        run(&args);
        let bytes = std::fs::read(&out).unwrap();
        let bytes = if label == "calendar.to-ics" {
            mu17g_c_normalized_ics(&bytes)
        } else {
            bytes
        };
        report.push((label.to_string(), bytes));
    }
    report.push((
        "calendar.delete-entry".to_string(),
        run(&[
            "calendar",
            "delete-entry",
            store,
            "mu17gc-pim",
            "alice",
            "work",
            "evt-1",
        ])
        .into_bytes(),
    ));
    capture_error(
        "calendar.get-entry.absent",
        &[
            "calendar",
            "get-entry",
            store,
            "mu17gc-pim",
            "alice",
            "work",
            "evt-1",
        ],
        &mut report,
    );
    report.push((
        "calendar.delete-collection".to_string(),
        run(&[
            "calendar",
            "delete-collection",
            store,
            "mu17gc-pim",
            "alice",
            "work",
        ])
        .into_bytes(),
    ));

    let contact = loom_core::contacts::ContactEntry::new("c-1", "Bob Jones");
    let contact_file = temp_bytes_file(&format!("{tag}-contact-entry"), &contact.encode());
    let vcard_file = temp_text_file(
        &format!("{tag}-contact-vcard"),
        "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:c-2\r\nFN:Alice Example\r\nEND:VCARD\r\n",
    );
    report.push((
        "contacts.create-book".to_string(),
        run(&[
            "contacts",
            "create-book",
            store,
            "mu17gc-pim",
            "alice",
            "friends",
            "--display-name",
            "Friends",
        ])
        .into_bytes(),
    ));
    run(&[
        "contacts",
        "get-book",
        store,
        "mu17gc-pim",
        "alice",
        "friends",
        "--out",
        &out,
    ]);
    report.push((
        "contacts.get-book".to_string(),
        std::fs::read(&out).unwrap(),
    ));
    report.push((
        "contacts.list-books.stdout".to_string(),
        run(&["contacts", "list-books", store, "mu17gc-pim", "alice"]).into_bytes(),
    ));
    run(&[
        "contacts",
        "list-books",
        store,
        "mu17gc-pim",
        "alice",
        "--out",
        &out,
    ]);
    report.push((
        "contacts.list-books.out".to_string(),
        std::fs::read(&out).unwrap(),
    ));
    report.push((
        "contacts.put-entry".to_string(),
        run(&[
            "contacts",
            "put-entry",
            store,
            "mu17gc-pim",
            "alice",
            "friends",
            &contact_file,
        ])
        .into_bytes(),
    ));
    report.push((
        "contacts.put-vcard".to_string(),
        run(&[
            "contacts",
            "put-vcard",
            store,
            "mu17gc-pim",
            "alice",
            "friends",
            &vcard_file,
        ])
        .into_bytes(),
    ));
    for (label, args) in [
        (
            "contacts.get-entry",
            vec![
                "contacts",
                "get-entry",
                store,
                "mu17gc-pim",
                "alice",
                "friends",
                "c-1",
                "--out",
                &out,
            ],
        ),
        (
            "contacts.list-entries",
            vec![
                "contacts",
                "list-entries",
                store,
                "mu17gc-pim",
                "alice",
                "friends",
                "--out",
                &out,
            ],
        ),
        (
            "contacts.search",
            vec![
                "contacts",
                "search",
                store,
                "mu17gc-pim",
                "alice",
                "friends",
                "Bob",
                "--out",
                &out,
            ],
        ),
        (
            "contacts.to-vcard",
            vec![
                "contacts",
                "to-vcard",
                store,
                "mu17gc-pim",
                "alice",
                "friends",
                "c-1",
                "--out",
                &out,
            ],
        ),
    ] {
        run(&args);
        report.push((label.to_string(), std::fs::read(&out).unwrap()));
    }
    report.push((
        "contacts.delete-entry".to_string(),
        run(&[
            "contacts",
            "delete-entry",
            store,
            "mu17gc-pim",
            "alice",
            "friends",
            "c-1",
        ])
        .into_bytes(),
    ));
    capture_error(
        "contacts.get-entry.absent",
        &[
            "contacts",
            "get-entry",
            store,
            "mu17gc-pim",
            "alice",
            "friends",
            "c-1",
        ],
        &mut report,
    );
    report.push((
        "contacts.delete-book".to_string(),
        run(&[
            "contacts",
            "delete-book",
            store,
            "mu17gc-pim",
            "alice",
            "friends",
        ])
        .into_bytes(),
    ));

    let mail_file = temp_text_file(
        &format!("{tag}-mail-message"),
        "From: alice@example.test\r\nTo: bob@example.test\r\nSubject: Status\r\n\r\nBody\r\n",
    );
    report.push((
        "mail.create-mailbox".to_string(),
        run(&[
            "mail",
            "create-mailbox",
            store,
            "mu17gc-pim",
            "alice",
            "inbox",
            "--display-name",
            "Inbox",
        ])
        .into_bytes(),
    ));
    run(&[
        "mail",
        "get-mailbox",
        store,
        "mu17gc-pim",
        "alice",
        "inbox",
        "--out",
        &out,
    ]);
    report.push(("mail.get-mailbox".to_string(), std::fs::read(&out).unwrap()));
    report.push((
        "mail.list-mailboxes.stdout".to_string(),
        run(&["mail", "list-mailboxes", store, "mu17gc-pim", "alice"]).into_bytes(),
    ));
    run(&[
        "mail",
        "list-mailboxes",
        store,
        "mu17gc-pim",
        "alice",
        "--out",
        &out,
    ]);
    report.push((
        "mail.list-mailboxes.out".to_string(),
        std::fs::read(&out).unwrap(),
    ));
    report.push((
        "mail.ingest-message".to_string(),
        run(&[
            "mail",
            "ingest-message",
            store,
            "mu17gc-pim",
            "alice",
            "inbox",
            "m-1",
            &mail_file,
        ])
        .into_bytes(),
    ));
    for (label, args) in [
        (
            "mail.get-message",
            vec![
                "mail",
                "get-message",
                store,
                "mu17gc-pim",
                "alice",
                "inbox",
                "m-1",
                "--out",
                &out,
            ],
        ),
        (
            "mail.list-messages",
            vec![
                "mail",
                "list-messages",
                store,
                "mu17gc-pim",
                "alice",
                "inbox",
                "--out",
                &out,
            ],
        ),
        (
            "mail.to-eml",
            vec![
                "mail",
                "to-eml",
                store,
                "mu17gc-pim",
                "alice",
                "inbox",
                "m-1",
                "--out",
                &out,
            ],
        ),
        (
            "mail.search",
            vec![
                "mail",
                "search",
                store,
                "mu17gc-pim",
                "alice",
                "inbox",
                "Status",
                "--out",
                &out,
            ],
        ),
    ] {
        run(&args);
        report.push((label.to_string(), std::fs::read(&out).unwrap()));
    }
    report.push((
        "mail.get-flags.empty".to_string(),
        run(&[
            "mail",
            "get-flags",
            store,
            "mu17gc-pim",
            "alice",
            "inbox",
            "m-1",
        ])
        .into_bytes(),
    ));
    report.push((
        "mail.set-flags".to_string(),
        run(&[
            "mail",
            "set-flags",
            store,
            "mu17gc-pim",
            "alice",
            "inbox",
            "m-1",
            "\\Seen",
            "\\Flagged",
        ])
        .into_bytes(),
    ));
    report.push((
        "mail.get-flags.stdout".to_string(),
        run(&[
            "mail",
            "get-flags",
            store,
            "mu17gc-pim",
            "alice",
            "inbox",
            "m-1",
        ])
        .into_bytes(),
    ));
    run(&[
        "mail",
        "get-flags",
        store,
        "mu17gc-pim",
        "alice",
        "inbox",
        "m-1",
        "--out",
        &out2,
    ]);
    report.push((
        "mail.get-flags.out".to_string(),
        std::fs::read(&out2).unwrap(),
    ));
    report.push((
        "mail.delete-message".to_string(),
        run(&[
            "mail",
            "delete-message",
            store,
            "mu17gc-pim",
            "alice",
            "inbox",
            "m-1",
        ])
        .into_bytes(),
    ));
    capture_error(
        "mail.get-message.absent",
        &[
            "mail",
            "get-message",
            store,
            "mu17gc-pim",
            "alice",
            "inbox",
            "m-1",
        ],
        &mut report,
    );
    report.push((
        "mail.delete-mailbox".to_string(),
        run(&[
            "mail",
            "delete-mailbox",
            store,
            "mu17gc-pim",
            "alice",
            "inbox",
        ])
        .into_bytes(),
    ));

    for path in [
        out,
        out2,
        cal_entry_file,
        cal_ics_file,
        contact_file,
        vcard_file,
        mail_file,
    ] {
        let _ = std::fs::remove_file(path);
    }
    report
}

fn mu17g_d1_core_cli_report(globals: &[String], store: &str, tag: &str) -> Vec<(String, Vec<u8>)> {
    fn dynamic_token(token: &str) -> Option<&'static str> {
        if token.starts_with("blake3:") && token.len() == 71 {
            return Some("<digest>");
        }
        if let Some((prefix, suffix)) = token.split_once(':') {
            if prefix.len() == 36
                && prefix.chars().enumerate().all(|(idx, c)| {
                    matches!(idx, 8 | 13 | 18 | 23) && c == '-' || c.is_ascii_hexdigit()
                })
                && suffix.chars().all(|c| c.is_ascii_digit())
            {
                return Some("<operation-id>");
            }
        }
        if token.len() == 36
            && token.chars().enumerate().all(|(idx, c)| {
                matches!(idx, 8 | 13 | 18 | 23) && c == '-' || c.is_ascii_hexdigit()
            })
        {
            return Some("<uuid>");
        }
        if token.len() >= 20
            && token.ends_with('Z')
            && token.as_bytes().get(4) == Some(&b'-')
            && token.as_bytes().get(7) == Some(&b'-')
            && token.as_bytes().get(10) == Some(&b'T')
        {
            return Some("<timestamp>");
        }
        if token.len() >= 13 && token.chars().all(|c| c.is_ascii_digit()) {
            return Some("<timestamp>");
        }
        None
    }

    fn normalized_text(text: String) -> Vec<u8> {
        let text = text
            .replace("FileSystem.read_file failed: NotFound:", "NOT_FOUND:")
            .replace("Document.put_text failed: Conflict:", "CONFLICT:")
            .replace("Pages.pages_update_json failed: Conflict:", "CONFLICT:");
        let mut lines = Vec::new();
        for line in text.lines() {
            let mut normalized = String::new();
            for token in line.split_whitespace() {
                if !normalized.is_empty() {
                    normalized.push(' ');
                }
                if let Some(value) = dynamic_token(token) {
                    normalized.push_str(value);
                } else if token.starts_with("pages.") && token.contains(".structure.") {
                    normalized.push_str("pages.<uuid>.structure.tree");
                } else {
                    normalized.push_str(token);
                }
            }
            lines.push(normalized);
        }
        let mut out = lines.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        out.into_bytes()
    }

    let mut report = Vec::new();
    let run = |args: &[&str]| loom_with_globals(globals, args).unwrap();
    let capture_error = |label: &str, args: &[&str], report: &mut Vec<(String, Vec<u8>)>| {
        let (ok, stdout, stderr) = loom_output_with_globals(globals, args);
        assert!(!ok, "{label} unexpectedly succeeded:\n{stdout}");
        report.push((format!("{label}.stderr"), normalized_text(stderr)));
    };
    let record = |label: &str, output: String, report: &mut Vec<(String, Vec<u8>)>| {
        report.push((label.to_string(), normalized_text(output)));
    };
    let record_json = |label: &str, output: String, report: &mut Vec<(String, Vec<u8>)>| {
        report.push((label.to_string(), mu17h_normalized_json(label, output)));
    };

    let text_input = temp_text_file(
        &format!("{tag}-text"),
        "{\"kind\":\"note\",\"tag\":\"a\"}\n",
    );
    let text_update = temp_text_file(&format!("{tag}-text-update"), "updated\n");
    let binary_input = temp_bytes_file(&format!("{tag}-binary"), b"\x00loom\xff");
    let file_input = temp_text_file(&format!("{tag}-file"), "file body\n");
    let out = temp_bytes_file(&format!("{tag}-out"), b"");
    let query = temp_text_file(
        &format!("{tag}-query"),
        "{\"predicate\":{\"path\":\"kind\",\"op\":\"eq\",\"value\":\"note\"}}",
    );

    record(
        "workspace.list.before",
        run(&["workspace", "list", store]),
        &mut report,
    );
    record(
        "workspace.rename",
        run(&["workspace", "rename", store, "scratch", "scratch2"]),
        &mut report,
    );
    record(
        "workspace.delete",
        run(&["workspace", "delete", store, "scratch2"]),
        &mut report,
    );

    run(&["files", "mkdir", store, "files", "dir", "--parents"]);
    run(&["files", "write", store, "files", "dir/a.txt", &file_input]);
    record(
        "files.ls",
        run(&["files", "ls", store, "files"]),
        &mut report,
    );
    run(&["files", "read", store, "files", "dir/a.txt", "--out", &out]);
    report.push(("files.read.out".to_string(), std::fs::read(&out).unwrap()));
    run(&["files", "delete", store, "files", "dir/a.txt"]);
    capture_error(
        "files.read.absent",
        &["files", "read", store, "files", "dir/a.txt"],
        &mut report,
    );

    run(&[
        "document",
        "index-create",
        store,
        "docs",
        "notes",
        "kind",
        "kind",
    ]);
    record(
        "document.put-text",
        run(&[
            "document",
            "put-text",
            store,
            "docs",
            "notes",
            "one",
            &text_input,
        ]),
        &mut report,
    );
    run(&[
        "document", "get-text", store, "docs", "notes", "one", "--out", &out,
    ]);
    report.push((
        "document.get-text.out".to_string(),
        std::fs::read(&out).unwrap(),
    ));
    capture_error(
        "document.put-text.compare",
        &[
            "document",
            "put-text",
            store,
            "docs",
            "notes",
            "one",
            &text_update,
            "--expected-entity-tag",
            "wrong",
        ],
        &mut report,
    );
    record(
        "document.put-binary",
        run(&[
            "document",
            "put-binary",
            store,
            "docs",
            "binary",
            "raw",
            &binary_input,
        ]),
        &mut report,
    );
    run(&[
        "document",
        "get-binary",
        store,
        "docs",
        "binary",
        "raw",
        "--out",
        &out,
    ]);
    report.push((
        "document.get-binary.out".to_string(),
        std::fs::read(&out).unwrap(),
    ));
    run(&[
        "document",
        "list-binary",
        store,
        "docs",
        "binary",
        "--out",
        &out,
    ]);
    report.push((
        "document.list-binary.out".to_string(),
        std::fs::read(&out).unwrap(),
    ));
    record(
        "document.index-list",
        run(&["document", "index-list", store, "docs", "notes"]),
        &mut report,
    );
    record(
        "document.index-status",
        run(&["document", "index-status", store, "docs", "notes"]),
        &mut report,
    );
    record(
        "document.find",
        run(&[
            "document", "find", store, "docs", "notes", "kind", "\"note\"",
        ]),
        &mut report,
    );
    let query_output = run(&["document", "query", store, "docs", "notes", &query]);
    assert!(
        query_output.contains("\"id\":\"one\""),
        "document.query must return indexed document one: {query_output}"
    );
    record("document.query", query_output, &mut report);
    record(
        "document.index-drop",
        run(&["document", "index-drop", store, "docs", "notes", "kind"]),
        &mut report,
    );
    record(
        "document.delete",
        run(&["document", "delete", store, "docs", "binary", "raw"]),
        &mut report,
    );
    capture_error(
        "document.get-text.absent",
        &["document", "get-text", store, "docs", "notes", "missing"],
        &mut report,
    );

    record(
        "pages.space-create",
        run(&["pages", "space-create", store, "pages", "space", "Space"]),
        &mut report,
    );
    record_json(
        "pages.space-create.json",
        run(&[
            "pages",
            "space-create",
            store,
            "pages",
            "space-json",
            "Space JSON",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "pages.space-list",
        run(&["pages", "space-list", store, "pages"]),
        &mut report,
    );
    record_json(
        "pages.space-list.json",
        run(&["pages", "space-list", store, "pages", "--format", "json"]),
        &mut report,
    );
    record(
        "pages.space-get",
        run(&["pages", "space-get", store, "pages", "space"]),
        &mut report,
    );
    record_json(
        "pages.space-get.json",
        run(&[
            "pages",
            "space-get",
            store,
            "pages",
            "space-json",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "pages.create",
        run(&["pages", "create", store, "pages", "page", "space", "Page"]),
        &mut report,
    );
    record_json(
        "pages.create.json",
        run(&[
            "pages",
            "create",
            store,
            "pages",
            "page-json",
            "space-json",
            "Page JSON",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "pages.update",
        run(&["pages", "update", store, "pages", "page", "plain-body"]),
        &mut report,
    );
    record_json(
        "pages.update.json",
        run(&[
            "pages",
            "update",
            store,
            "pages",
            "page-json",
            "json-body",
            "--format",
            "json",
        ]),
        &mut report,
    );
    capture_error(
        "pages.update.compare",
        &[
            "pages",
            "update",
            store,
            "pages",
            "page",
            "plain-body",
            "--expected-root",
            "blake3:0000000000000000000000000000000000000000000000000000000000000000",
        ],
        &mut report,
    );
    record(
        "pages.publish",
        run(&["pages", "publish", store, "pages", "page"]),
        &mut report,
    );
    record_json(
        "pages.publish.json",
        run(&[
            "pages",
            "publish",
            store,
            "pages",
            "page-json",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "pages.get",
        run(&["pages", "get", store, "pages", "page"]),
        &mut report,
    );
    record_json(
        "pages.get.json",
        run(&[
            "pages",
            "get",
            store,
            "pages",
            "page-json",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "pages.history",
        run(&["pages", "history", store, "pages", "page"]),
        &mut report,
    );
    record_json(
        "pages.history.json",
        run(&[
            "pages",
            "history",
            store,
            "pages",
            "page-json",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "pages.structure-create",
        run(&[
            "pages",
            "structure-create",
            store,
            "pages",
            "tree",
            "space",
            "outline",
            "Tree",
        ]),
        &mut report,
    );
    record_json(
        "pages.structure-create.json",
        run(&[
            "pages",
            "structure-create",
            store,
            "pages",
            "tree-json",
            "space-json",
            "outline",
            "Tree JSON",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "pages.structure-add-node",
        run(&[
            "pages",
            "structure-add-node",
            store,
            "pages",
            "tree",
            "node",
            "page",
            "Node",
        ]),
        &mut report,
    );
    record_json(
        "pages.structure-add-node.json",
        run(&[
            "pages",
            "structure-add-node",
            store,
            "pages",
            "tree-json",
            "node-json",
            "page",
            "Node JSON",
            "--format",
            "json",
        ]),
        &mut report,
    );
    run(&[
        "pages",
        "structure-add-node",
        store,
        "pages",
        "tree",
        "parent",
        "group",
        "Parent",
    ]);
    run(&[
        "pages",
        "structure-add-node",
        store,
        "pages",
        "tree-json",
        "parent-json",
        "group",
        "Parent JSON",
    ]);
    record(
        "pages.structure-update-node",
        run(&[
            "pages",
            "structure-update-node",
            store,
            "pages",
            "tree",
            "node",
            "page",
            "Node Updated",
        ]),
        &mut report,
    );
    record_json(
        "pages.structure-update-node.json",
        run(&[
            "pages",
            "structure-update-node",
            store,
            "pages",
            "tree-json",
            "node-json",
            "page",
            "Node JSON Updated",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "pages.structure-bind",
        run(&[
            "pages",
            "structure-bind",
            store,
            "pages",
            "tree",
            "node",
            "--entity-ref",
            "page:page",
        ]),
        &mut report,
    );
    record_json(
        "pages.structure-bind.json",
        run(&[
            "pages",
            "structure-bind",
            store,
            "pages",
            "tree-json",
            "node-json",
            "--entity-ref",
            "page:page-json",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "pages.structure-move-node",
        run(&[
            "pages",
            "structure-move-node",
            store,
            "pages",
            "tree",
            "parent",
            "--parent-node-id",
            "node",
        ]),
        &mut report,
    );
    record_json(
        "pages.structure-move-node.json",
        run(&[
            "pages",
            "structure-move-node",
            store,
            "pages",
            "tree-json",
            "parent-json",
            "--parent-node-id",
            "node-json",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "pages.structure-link-node",
        run(&[
            "pages",
            "structure-link-node",
            store,
            "pages",
            "tree",
            "related",
            "node",
            "parent",
            "related_to",
        ]),
        &mut report,
    );
    record_json(
        "pages.structure-link-node.json",
        run(&[
            "pages",
            "structure-link-node",
            store,
            "pages",
            "tree-json",
            "related-json",
            "node-json",
            "parent-json",
            "related_to",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "pages.structure-decompose-to-tickets",
        run(&[
            "pages",
            "structure-decompose-to-tickets",
            store,
            "pages",
            "tree",
            "[]",
        ]),
        &mut report,
    );
    record_json(
        "pages.structure-decompose-to-tickets.json",
        run(&[
            "pages",
            "structure-decompose-to-tickets",
            store,
            "pages",
            "tree-json",
            "[]",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "pages.structure-get",
        run(&["pages", "structure-get", store, "pages", "tree"]),
        &mut report,
    );
    record_json(
        "pages.structure-get.json",
        run(&[
            "pages",
            "structure-get",
            store,
            "pages",
            "tree-json",
            "--format",
            "json",
        ]),
        &mut report,
    );

    report
}

fn prepare_mu17g_d2_store(path: &str) {
    loom(["store", "init", path]).unwrap();
    loom(["workspace", "create", path, "repo", "--facet", "vcs"]).unwrap();
}

fn prepare_mu17g_d2_authenticated_non_admin_store(path: &str, tag: &str) -> (String, String) {
    prepare_mu17g_d2_store(path);
    let list = loom(["identity", "list", path]).unwrap();
    let root: serde_json::Value = serde_json::from_str(&list).expect("identity list JSON");
    let root_id = root
        .get("root")
        .and_then(|value| value.as_str())
        .expect("root principal")
        .to_string();
    let root_pass = temp_text_file(&format!("{tag}-root-pass"), "root-passphrase");
    let user_pass = temp_text_file(&format!("{tag}-user-pass"), "user-passphrase");
    loom([
        "identity",
        "set-passphrase",
        path,
        &root_id,
        "--new-key-source",
        &format!("file:{root_pass}"),
    ])
    .unwrap();
    let user_id = loom([
        "--auth-principal",
        &root_id,
        "--auth-key-source",
        &format!("file:{root_pass}"),
        "identity",
        "add",
        path,
        "viewer",
        "Viewer",
    ])
    .unwrap()
    .trim()
    .to_string();
    loom([
        "--auth-principal",
        &root_id,
        "--auth-key-source",
        &format!("file:{root_pass}"),
        "identity",
        "set-passphrase",
        path,
        &user_id,
        "--new-key-source",
        &format!("file:{user_pass}"),
    ])
    .unwrap();
    let _ = std::fs::remove_file(root_pass);
    (user_id, user_pass)
}

fn prepare_mu17g_d3_store(path: &str) {
    loom(["store", "init", path]).unwrap();
    loom(["workspace", "create", path, "main", "--facet", "vcs"]).unwrap();
}

fn prepare_mu17g_d3_authenticated_non_admin_store(path: &str, tag: &str) -> (String, String) {
    prepare_mu17g_d3_store(path);
    let list = loom(["identity", "list", path]).unwrap();
    let root: serde_json::Value = serde_json::from_str(&list).expect("identity list JSON");
    let root_id = root
        .get("root")
        .and_then(|value| value.as_str())
        .expect("root principal")
        .to_string();
    let root_pass = temp_text_file(&format!("{tag}-root-pass"), "root-passphrase");
    let user_pass = temp_text_file(&format!("{tag}-user-pass"), "user-passphrase");
    loom([
        "identity",
        "set-passphrase",
        path,
        &root_id,
        "--new-key-source",
        &format!("file:{root_pass}"),
    ])
    .unwrap();
    let user_id = loom([
        "--auth-principal",
        &root_id,
        "--auth-key-source",
        &format!("file:{root_pass}"),
        "identity",
        "add",
        path,
        "viewer",
        "Viewer",
    ])
    .unwrap()
    .trim()
    .to_string();
    loom([
        "--auth-principal",
        &root_id,
        "--auth-key-source",
        &format!("file:{root_pass}"),
        "identity",
        "set-passphrase",
        path,
        &user_id,
        "--new-key-source",
        &format!("file:{user_pass}"),
    ])
    .unwrap();
    let _ = std::fs::remove_file(root_pass);
    (user_id, user_pass)
}

fn mu17g_d3_tickets_lanes_cli_report(
    globals: &[String],
    store: &str,
    tag: &str,
) -> Vec<(String, Vec<u8>)> {
    fn dynamic_token(token: &str) -> Option<&'static str> {
        if token.starts_with("blake3:") && token.len() == 71 {
            return Some("<digest>");
        }
        if token.len() == 36
            && token.chars().enumerate().all(|(idx, c)| {
                matches!(idx, 8 | 13 | 18 | 23) && c == '-' || c.is_ascii_hexdigit()
            })
        {
            return Some("<uuid>");
        }
        if token.len() >= 13 && token.chars().all(|c| c.is_ascii_digit()) {
            return Some("<timestamp>");
        }
        None
    }

    fn normalize_json_value(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(text) => {
                if let Some(token) = dynamic_token(text) {
                    *text = token.to_string();
                } else if text.contains(':') {
                    let mut changed = false;
                    let normalized = text
                        .split(':')
                        .map(|part| {
                            if dynamic_token(part) == Some("<uuid>") {
                                changed = true;
                                "<uuid>"
                            } else {
                                part
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(":");
                    if changed {
                        *text = normalized;
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    normalize_json_value(item);
                }
            }
            serde_json::Value::Object(map) => {
                for (key, item) in map.iter_mut() {
                    if matches!(
                        key.as_str(),
                        "created_at"
                            | "created_at_ms"
                            | "deleted_at"
                            | "deleted_at_ms"
                            | "updated_at"
                            | "updated_at_ms"
                    ) {
                        match item {
                            serde_json::Value::Number(_) | serde_json::Value::String(_) => {
                                *item = serde_json::Value::String("<timestamp>".to_string());
                                continue;
                            }
                            _ => {}
                        }
                    }
                    normalize_json_value(item);
                }
            }
            _ => {}
        }
    }

    fn normalized_text(text: String) -> Vec<u8> {
        let text = text
            .replace("Tickets.tickets_update_json failed: Conflict:", "CONFLICT:")
            .replace(
                "Tickets.tickets_field_put_json failed: InvalidArgument:",
                "INVALID_ARGUMENT:",
            )
            .replace("Tickets.tickets_get_json failed: NotFound:", "NOT_FOUND:")
            .replace(
                "Tickets.tickets_delete_json failed: NotFound:",
                "NOT_FOUND:",
            )
            .replace("Lanes.get_view_json failed: NotFound:", "NOT_FOUND:")
            .replace("Lanes.delete failed: NotFound:", "NOT_FOUND:")
            .replace("PermissionDenied:", "PERMISSION_DENIED:");
        if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) {
            normalize_json_value(&mut value);
            let mut out = serde_json::to_string(&value).unwrap();
            out.push('\n');
            return out.into_bytes();
        }
        let mut lines = Vec::new();
        for line in text.lines() {
            if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(line) {
                normalize_json_value(&mut value);
                lines.push(serde_json::to_string(&value).unwrap());
                continue;
            }
            let mut normalized = String::new();
            for token in line.split_whitespace() {
                if !normalized.is_empty() {
                    normalized.push(' ');
                }
                if let Some(value) = dynamic_token(token) {
                    normalized.push_str(value);
                } else if let Some((name, value)) = token.split_once('=')
                    && let Some(value) = dynamic_token(value)
                {
                    normalized.push_str(name);
                    normalized.push('=');
                    normalized.push_str(value);
                } else {
                    normalized.push_str(token);
                }
            }
            lines.push(normalized);
        }
        let mut out = lines.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        out.into_bytes()
    }

    fn record_json_root(output: &str) -> String {
        let value: serde_json::Value = serde_json::from_str(output).expect("ticket mutation JSON");
        value
            .pointer("/receipt/root_after")
            .or_else(|| value.pointer("/receipt/new_root"))
            .or_else(|| value.pointer("/root_after"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("blake3:0000000000000000000000000000000000000000000000000000000000000000")
            .to_string()
    }

    fn record_json_ticket_id(output: &str) -> String {
        let value: serde_json::Value = serde_json::from_str(output).expect("ticket mutation JSON");
        value
            .pointer("/resource/ticket_id")
            .or_else(|| value.pointer("/resource/id"))
            .or_else(|| value.pointer("/ticket/id"))
            .and_then(serde_json::Value::as_str)
            .expect("ticket id")
            .to_string()
    }

    fn record_json_workspace_id(output: &str) -> String {
        let value: serde_json::Value = serde_json::from_str(output).expect("ticket project JSON");
        value
            .pointer("/workspace_id")
            .or_else(|| value.pointer("/resource/workspace_id"))
            .and_then(serde_json::Value::as_str)
            .expect("workspace id")
            .to_string()
    }

    let mut report = Vec::new();
    let run = |args: &[&str]| loom_with_globals(globals, args).unwrap();
    let capture_error = |label: &str, args: &[&str], report: &mut Vec<(String, Vec<u8>)>| {
        let (ok, stdout, stderr) = loom_output_with_globals(globals, args);
        assert!(!ok, "{label} unexpectedly succeeded:\n{stdout}");
        report.push((format!("{label}.stderr"), normalized_text(stderr)));
    };
    let capture_stable_error =
        |label: &str, args: &[&str], code: &str, report: &mut Vec<(String, Vec<u8>)>| {
            let (ok, stdout, stderr) = loom_output_with_globals(globals, args);
            assert!(!ok, "{label} unexpectedly succeeded:\n{stdout}");
            assert!(
                stderr
                    .to_ascii_uppercase()
                    .contains(&code.to_ascii_uppercase()),
                "{label} did not preserve {code}:\n{stderr}"
            );
            report.push((format!("{label}.stderr"), normalized_text(stderr)));
        };
    let record = |label: &str, output: String, report: &mut Vec<(String, Vec<u8>)>| {
        report.push((label.to_string(), normalized_text(output)));
    };

    let comment_body = temp_text_file(&format!("{tag}-comment"), "first structured comment\n");
    let comment_body_update = temp_text_file(&format!("{tag}-comment-update"), "updated comment\n");
    let closeout_body = temp_text_file(&format!("{tag}-closeout"), "closed from lane\n");
    let evidence = r#"{"source_anchors":["crates/loom-cli/src/main.rs:3272"]}"#;

    let project_create = run(&[
        "tickets",
        "project-create",
        store,
        "main",
        "core",
        "CORE",
        "Core",
        "--format",
        "json",
    ]);
    let ticket_workspace_id = record_json_workspace_id(&project_create);
    record("tickets.project-create", project_create, &mut report);
    record(
        "tickets.project-settings-get.before",
        run(&[
            "tickets",
            "project-settings-get",
            store,
            "main",
            "core",
            "--include-contracts",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.project-settings-set",
        run(&[
            "tickets",
            "project-settings-set",
            store,
            "main",
            "core",
            "--default-projection",
            "jira",
            "--actor-enforcement",
            "write-access",
            "--acceptance-evidence-enforcement",
            "true",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.projects",
        run(&["tickets", "projects", store, "main", "--format", "json"]),
        &mut report,
    );
    record(
        "tickets.field-put",
        run(&[
            "tickets",
            "field-put",
            store,
            "main",
            "core",
            "component",
            "component",
            "Component",
            "--type",
            "string",
            "--max-length",
            "64",
            "--required",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.fields",
        run(&[
            "tickets",
            "fields",
            store,
            "main",
            "--project-id",
            "core",
            "--projection",
            "native",
            "--operation",
            "write",
            "--format",
            "json",
        ]),
        &mut report,
    );
    capture_error(
        "tickets.field-put.invalid",
        &[
            "tickets",
            "field-put",
            store,
            "main",
            "core",
            "bad",
            "bad",
            "Bad",
            "--type",
            "not-a-type",
            "--format",
            "json",
        ],
        &mut report,
    );
    let create_one = run(&[
        "tickets",
        "create",
        store,
        "main",
        "task",
        "--project-id",
        "core",
        "--title",
        "Build generated CLI",
        "--description",
        "exercise Tickets generated path",
        "--priority",
        "high",
        "--fields",
        r#"{"component":"cli"}"#,
        "--policy-label",
        "mu17g",
        "--format",
        "json",
    ]);
    let create_one_root = record_json_root(&create_one);
    let ticket_one_id = record_json_ticket_id(&create_one);
    record("tickets.create.one", create_one, &mut report);
    record(
        "tickets.create.two",
        run(&[
            "tickets",
            "create",
            store,
            "main",
            "task",
            "--project-id",
            "core",
            "--title",
            "Second ticket",
            "--description",
            "target relation",
            "--priority",
            "low",
            "--fields",
            r#"{"component":"cli"}"#,
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.get.one",
        run(&[
            "tickets", "get", store, "main", "CORE-1", "--format", "json",
        ]),
        &mut report,
    );
    record(
        "tickets.list.before-update",
        run(&[
            "tickets",
            "list",
            store,
            "main",
            "--projection",
            "jira",
            "--status",
            "open",
            "--limit",
            "10",
            "--format",
            "json",
        ]),
        &mut report,
    );
    capture_error(
        "tickets.update.stale-root",
        &[
            "tickets",
            "update",
            store,
            "main",
            "CORE-1",
            "--status",
            "in_progress",
            "--expected-root",
            "blake3:0000000000000000000000000000000000000000000000000000000000000000",
            "--format",
            "json",
        ],
        &mut report,
    );
    record(
        "tickets.update.status",
        run(&[
            "tickets",
            "update",
            store,
            "main",
            "CORE-1",
            "--status",
            "in_progress",
            "--assignee",
            "agent:3",
            "--title",
            "Build generated CLI parity",
            "--field",
            "component=cli",
            "--expected-root",
            &create_one_root,
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.field-retire",
        run(&[
            "tickets",
            "field-retire",
            store,
            "main",
            "core",
            "component",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.comment-add",
        run(&[
            "tickets",
            "comment-add",
            store,
            "main",
            "CORE-1",
            &format!("@{comment_body}"),
            "--comment-id",
            "comment-1",
            "--comment-type",
            "acceptance_evidence",
            "--evidence",
            evidence,
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.comments.after-add",
        run(&[
            "tickets", "comments", store, "main", "CORE-1", "--format", "json",
        ]),
        &mut report,
    );
    record(
        "tickets.comment-update",
        run(&[
            "tickets",
            "comment-update",
            store,
            "main",
            "CORE-1",
            "comment-1",
            "--body",
            &format!("@{comment_body_update}"),
            "--comment-type",
            "review_feedback",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.comment-delete",
        run(&[
            "tickets",
            "comment-delete",
            store,
            "main",
            "CORE-1",
            "comment-1",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.relation-set",
        run(&[
            "tickets",
            "relation-set",
            store,
            "main",
            "CORE-1",
            "blocks",
            "CORE-2",
            "--relation-id",
            "rel-1",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.relations",
        run(&[
            "tickets",
            "relations",
            store,
            "main",
            "CORE-1",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.relation-remove",
        run(&[
            "tickets",
            "relation-remove",
            store,
            "main",
            "CORE-1",
            "rel-1",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.board-create",
        run(&[
            "tickets",
            "board-create",
            store,
            "main",
            "board-1",
            "CORE-BOARD",
            "core",
            "Core Board",
            "--mode",
            "manual",
            "--description",
            "Board for parity",
            "--column",
            "todo:To Do::10",
            "--column",
            "doing:Doing::20",
            "--card-field",
            "title",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.board-get",
        run(&[
            "tickets",
            "board-get",
            store,
            "main",
            "board-1",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.board-list",
        run(&["tickets", "board-list", store, "main", "--format", "json"]),
        &mut report,
    );
    record(
        "tickets.board-update",
        run(&[
            "tickets",
            "board-update",
            store,
            "main",
            "board-1",
            "--name",
            "Core Board Updated",
            "--board-status",
            "active",
            "--card-field",
            "title",
            "--card-field",
            "status",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.board-configure-columns",
        run(&[
            "tickets",
            "board-configure-columns",
            store,
            "main",
            "board-1",
            "--mode",
            "manual",
            "--column",
            "todo:To Do::10",
            "--column",
            "doing:Doing::20",
            "--column",
            "done:Done::30",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.board-move-card",
        run(&[
            "tickets",
            "board-move-card",
            store,
            "main",
            "board-1",
            &ticket_one_id,
            "doing",
            "0001",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "lanes.create.a",
        run(&[
            "lanes",
            "create",
            store,
            "main",
            "lane-a",
            "lane-a",
            "--kind",
            "assignment",
            "--owner-principal",
            "agent:3",
            "--title",
            "Agent lane",
            "--description",
            "Lane parity",
            "--lane-status",
            "ready",
            "--active-ticket-id",
            "CORE-1",
            "--status-report",
            "ready",
            "--updated-at",
            "1",
            "--updated-by",
            "agent:3",
            "--ticket",
            "CORE-1",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "lanes.create.b",
        run(&[
            "lanes",
            "create",
            store,
            "main",
            "lane-b",
            "lane-b",
            "--kind",
            "assignment",
            "--title",
            "Second lane",
            "--lane-status",
            "ready",
            "--updated-at",
            "2",
            "--updated-by",
            "agent:3",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "lanes.get.a",
        run(&[
            "lanes",
            "get",
            store,
            "main",
            "lane-a",
            "--detailed",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "lanes.list",
        run(&[
            "lanes",
            "list",
            store,
            "main",
            "--detailed",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "lanes.update",
        run(&[
            "lanes",
            "update",
            store,
            "main",
            "lane-a",
            "--lane-status",
            "working",
            "--status-report",
            "working CORE-1",
            "--reviewer-feedback",
            "review pending",
            "--updated-by",
            "agent:3",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "lanes.ticket-add",
        run(&[
            "lanes",
            "ticket-add",
            store,
            "main",
            "lane-a",
            "CORE-2",
            "--first",
            "--updated-by",
            "agent:3",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "lanes.ticket-remove",
        run(&[
            "lanes",
            "ticket-remove",
            store,
            "main",
            "lane-a",
            "CORE-1",
            "--updated-by",
            "agent:3",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "lanes.ticket-transfer",
        run(&[
            "lanes",
            "ticket-transfer",
            store,
            "main",
            "lane-a",
            "lane-b",
            "CORE-2",
            "--updated-by",
            "agent:3",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.update.closed",
        run(&[
            "tickets", "update", store, "main", "CORE-1", "--status", "closed", "--format", "json",
        ]),
        &mut report,
    );
    record(
        "lanes.closeout",
        run(&[
            "lanes",
            "closeout",
            store,
            "main",
            "lane-a",
            &ticket_workspace_id,
            "CORE-1",
            "--comment-type",
            "closeout_evidence",
            "--comment-body",
            &format!("@{closeout_body}"),
            "--status-report",
            "closed CORE-1",
            "--updated-by",
            "agent:3",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "lanes.cleanup.dry-run",
        run(&["lanes", "cleanup", store, "main", "--format", "json"]),
        &mut report,
    );
    record(
        "lanes.cleanup.apply",
        run(&[
            "lanes", "cleanup", store, "main", "--apply", "--format", "json",
        ]),
        &mut report,
    );
    record(
        "lanes.update.b.closed",
        run(&[
            "lanes",
            "update",
            store,
            "main",
            "lane-b",
            "--lane-status",
            "closed",
            "--updated-by",
            "agent:3",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "lanes.delete.b",
        run(&[
            "lanes",
            "delete",
            store,
            "main",
            "lane-b",
            "--updated-by",
            "agent:3",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.board-delete",
        run(&[
            "tickets",
            "board-delete",
            store,
            "main",
            "board-1",
            "--format",
            "json",
        ]),
        &mut report,
    );
    record(
        "tickets.delete.two",
        run(&[
            "tickets", "delete", store, "main", "CORE-2", "--format", "json",
        ]),
        &mut report,
    );
    capture_error(
        "tickets.get.absent",
        &[
            "tickets", "get", store, "main", "CORE-99", "--format", "json",
        ],
        &mut report,
    );
    capture_error(
        "lanes.get.absent",
        &["lanes", "get", store, "main", "lane-b", "--format", "json"],
        &mut report,
    );

    record(
        "tickets.text.create",
        run(&[
            "tickets",
            "create",
            store,
            "main",
            "task",
            "--project-id",
            "core",
            "--title",
            "Default text ticket",
            "--description",
            "default presentation parity",
            "--priority",
            "medium",
        ]),
        &mut report,
    );
    record(
        "tickets.text.get",
        run(&["tickets", "get", store, "main", "CORE-3"]),
        &mut report,
    );
    record(
        "tickets.text.update",
        run(&[
            "tickets",
            "update",
            store,
            "main",
            "CORE-3",
            "--status",
            "in_progress",
            "--title",
            "Default text ticket updated",
        ]),
        &mut report,
    );
    record(
        "tickets.text.list",
        run(&[
            "tickets",
            "list",
            store,
            "main",
            "--status",
            "in_progress",
            "--limit",
            "10",
        ]),
        &mut report,
    );
    capture_stable_error(
        "tickets.text.update.stale-root",
        &[
            "tickets",
            "update",
            store,
            "main",
            "CORE-3",
            "--status",
            "closed",
            "--expected-root",
            "blake3:0000000000000000000000000000000000000000000000000000000000000000",
        ],
        "Conflict:",
        &mut report,
    );
    record(
        "tickets.text.delete",
        run(&["tickets", "delete", store, "main", "CORE-3"]),
        &mut report,
    );
    let tickets_after_delete = run(&[
        "tickets",
        "list",
        store,
        "main",
        "--status",
        "in_progress",
        "--limit",
        "10",
    ]);
    assert!(
        !tickets_after_delete.contains("CORE-3"),
        "deleted ticket remained in the default active list:\n{tickets_after_delete}"
    );
    record(
        "tickets.text.list.after-delete",
        tickets_after_delete,
        &mut report,
    );

    record(
        "lanes.text.create",
        run(&[
            "lanes",
            "create",
            store,
            "main",
            "lane-text",
            "lane-text",
            "--kind",
            "assignment",
            "--title",
            "Default text lane",
            "--lane-status",
            "ready",
            "--updated-at",
            "100",
            "--updated-by",
            "agent:3",
        ]),
        &mut report,
    );
    record(
        "lanes.text.get",
        run(&["lanes", "get", store, "main", "lane-text"]),
        &mut report,
    );
    record(
        "lanes.text.update",
        run(&[
            "lanes",
            "update",
            store,
            "main",
            "lane-text",
            "--lane-status",
            "closed",
            "--status-report",
            "default text closed",
            "--updated-by",
            "agent:3",
        ]),
        &mut report,
    );
    record(
        "lanes.text.list",
        run(&["lanes", "list", store, "main"]),
        &mut report,
    );
    record(
        "lanes.text.delete",
        run(&[
            "lanes",
            "delete",
            store,
            "main",
            "lane-text",
            "--updated-by",
            "agent:3",
        ]),
        &mut report,
    );
    capture_error(
        "lanes.text.get.absent",
        &["lanes", "get", store, "main", "lane-text"],
        &mut report,
    );
    for path in [comment_body, comment_body_update, closeout_body] {
        let _ = std::fs::remove_file(path);
    }
    report
}

fn assert_mu17g_d3_reports_eq(
    left: &[(String, Vec<u8>)],
    right: &[(String, Vec<u8>)],
    context: &str,
) {
    if left == right {
        return;
    }
    assert_eq!(left.len(), right.len(), "{context} report lengths diverged");
    for ((left_label, left_bytes), (right_label, right_bytes)) in left.iter().zip(right.iter()) {
        assert_eq!(left_label, right_label, "{context} report labels diverged");
        if left_bytes != right_bytes {
            panic!(
                "{context} report diverged at {left_label}\nleft:\n{}\nright:\n{}",
                String::from_utf8_lossy(left_bytes),
                String::from_utf8_lossy(right_bytes)
            );
        }
    }
    panic!("{context} reports diverged after equal labels and lengths");
}

fn prepare_mu17g_e1_store(path: &str) {
    loom(["store", "init", path]).unwrap();
    loom(["workspace", "create", path, "studio", "--facet", "sql"]).unwrap();
}

fn mu17g_e1_meetings_input(tag: &str) -> String {
    let source_digest = Digest::hash(Algo::Blake3, format!("source-{tag}").as_bytes()).to_string();
    let input = serde_json::json!({
        "snapshot_version": 1,
        "profile": "generic",
        "source_system": "generic",
        "source_scope": "team-notes",
        "observed_at": 200,
        "coverage": "complete",
        "items": [{
            "source_entity_id": "source-a",
            "source_digest": source_digest,
            "source_sidecar": {"raw": "source"},
            "title": "Planning",
            "summary_text": "Planning summary",
            "transcript_spans": [{"text": "Ship the import command."}]
        }]
    });
    temp_text_file(
        &format!("{tag}-meetings-input"),
        &serde_json::to_string(&input).unwrap(),
    )
}

fn mu17g_e1_sql_meetings_cli_report(
    globals: &[String],
    store: &str,
    tag: &str,
    include_readback: bool,
) -> Vec<(String, Vec<u8>)> {
    fn dynamic_token(token: &str) -> Option<&'static str> {
        if token.starts_with("blake3:") && token.len() == 71 {
            return Some("<digest>");
        }
        if token.len() == 36
            && token.chars().enumerate().all(|(idx, c)| {
                matches!(idx, 8 | 13 | 18 | 23) && c == '-' || c.is_ascii_hexdigit()
            })
        {
            return Some("<uuid>");
        }
        if token.len() >= 13 && token.chars().all(|c| c.is_ascii_digit()) {
            return Some("<timestamp>");
        }
        None
    }

    fn normalize_json_value(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(text) => {
                if let Some(token) = dynamic_token(text) {
                    *text = token.to_string();
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    normalize_json_value(item);
                }
            }
            serde_json::Value::Object(map) => {
                for (key, item) in map.iter_mut() {
                    if matches!(
                        key.as_str(),
                        "created_at_ms" | "updated_at_ms" | "observed_at_ms"
                    ) {
                        if item.is_number() || item.is_string() {
                            *item = serde_json::Value::String("<timestamp>".to_string());
                            continue;
                        }
                    }
                    normalize_json_value(item);
                }
            }
            _ => {}
        }
    }

    fn normalized_text(text: String) -> Vec<u8> {
        let text = text
            .replace(
                "Sql.sql_exec_result failed: InvalidArgument:",
                "INVALID_ARGUMENT:",
            )
            .replace("Sql.sql_exec_result failed: SqlSyntax:", "SQL_SYNTAX:")
            .replace(
                "Meetings.meetings_import_snapshot failed: InvalidArgument:",
                "INVALID_ARGUMENT:",
            )
            .replace(
                "Meetings.meetings_source_read failed: InvalidArgument:",
                "INVALID_ARGUMENT:",
            )
            .replace(
                "Meetings.meetings_source_read failed: NotFound:",
                "NOT_FOUND:",
            );
        if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) {
            normalize_json_value(&mut value);
            let mut out = serde_json::to_string(&value).unwrap();
            out.push('\n');
            return out.into_bytes();
        }
        let mut lines = Vec::new();
        for line in text.lines() {
            if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(line) {
                normalize_json_value(&mut value);
                lines.push(serde_json::to_string(&value).unwrap());
                continue;
            }
            let mut normalized = String::new();
            for token in line.split_whitespace() {
                if !normalized.is_empty() {
                    normalized.push(' ');
                }
                if let Some(value) = dynamic_token(token) {
                    normalized.push_str(value);
                } else {
                    normalized.push_str(token);
                }
            }
            lines.push(normalized);
        }
        let mut out = lines.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        out.into_bytes()
    }

    let mut report = Vec::new();
    let run = |args: &[&str]| loom_with_globals(globals, args).unwrap();
    let capture_error = |label: &str, args: &[&str], report: &mut Vec<(String, Vec<u8>)>| {
        let (ok, stdout, stderr) = loom_output_with_globals(globals, args);
        assert!(!ok, "{label} unexpectedly succeeded:\n{stdout}");
        report.push((format!("{label}.stderr"), normalized_text(stderr)));
    };
    let record = |label: &str, output: String, report: &mut Vec<(String, Vec<u8>)>| {
        report.push((label.to_string(), normalized_text(output)));
    };

    let meetings_input = mu17g_e1_meetings_input(tag);
    record(
        "sql.create",
        run(&[
            "sql",
            "exec",
            store,
            "studio",
            "CREATE TABLE notes (id INTEGER, body TEXT)",
            "--db",
            "main",
        ]),
        &mut report,
    );
    record(
        "sql.insert",
        run(&[
            "sql",
            "exec",
            store,
            "studio",
            "INSERT INTO notes VALUES (1, 'alpha')",
            "--db",
            "main",
        ]),
        &mut report,
    );
    record(
        "sql.select",
        run(&[
            "sql",
            "exec",
            store,
            "studio",
            "SELECT id, body FROM notes",
            "--db",
            "main",
        ]),
        &mut report,
    );
    capture_error(
        "sql.malformed",
        &[
            "sql",
            "exec",
            store,
            "studio",
            "THIS IS NOT SQL",
            "--db",
            "main",
        ],
        &mut report,
    );
    record(
        "meetings.import.dry-run",
        run(&[
            "meetings",
            "import",
            store,
            "studio",
            "--input-profile",
            "generic",
            "--input",
            &meetings_input,
            "--dry-run",
            "--report-format",
            "json",
        ]),
        &mut report,
    );
    if include_readback {
        capture_error(
            "meetings.list.before-write",
            &["meetings", "list", store, "studio", "--format", "json"],
            &mut report,
        );
    }
    record(
        "meetings.import.write",
        run(&[
            "meetings",
            "import",
            store,
            "studio",
            "--input-profile",
            "generic",
            "--input",
            &meetings_input,
            "--report-format",
            "json",
        ]),
        &mut report,
    );
    if include_readback {
        record(
            "meetings.list.after-write",
            run(&["meetings", "list", store, "studio", "--format", "json"]),
            &mut report,
        );
        record(
            "meetings.get",
            run(&[
                "meetings",
                "get",
                store,
                "studio",
                "meeting/source-a",
                "--format",
                "json",
            ]),
            &mut report,
        );
    }
    record(
        "meetings.source-read",
        run(&[
            "meetings",
            "source-read",
            store,
            "studio",
            "source-a",
            "summary.txt",
        ]),
        &mut report,
    );
    capture_error(
        "meetings.source-read.invalid-leaf",
        &[
            "meetings",
            "source-read",
            store,
            "studio",
            "source-a",
            "bad.txt",
        ],
        &mut report,
    );
    capture_error(
        "meetings.source-read.absent",
        &[
            "meetings",
            "source-read",
            store,
            "studio",
            "missing-source",
            "summary.txt",
        ],
        &mut report,
    );
    capture_error(
        "meetings.import.bad-profile",
        &[
            "meetings",
            "import",
            store,
            "studio",
            "--input-profile",
            "bad-profile",
            "--input",
            &meetings_input,
            "--report-format",
            "json",
        ],
        &mut report,
    );
    let _ = std::fs::remove_file(meetings_input);
    report
}

fn prepare_mu17g_e2_store(path: &str) -> String {
    loom(["store", "init", path]).unwrap();
    loom(["workspace", "create", path, "chatws"]).unwrap();
    loom(["workspace", "create", path, "drivews", "--facet", "files"]).unwrap();
    let root = loom(["drive", "list", path, "drivews", "root", "--format", "json"]).unwrap();
    serde_json::from_str::<serde_json::Value>(&root).unwrap()["profile_root"]
        .as_str()
        .unwrap()
        .to_string()
}

fn prepare_mu17g_gc_store(path: &str) -> String {
    loom(["store", "init", path]).unwrap();
    loom(["workspace", "create", path, "chatws"]).unwrap();
    for (handle, name, channel_id) in [
        ("general", "General", "71717171-7171-4171-8171-717171717171"),
        ("empty", "Empty", "72727272-7272-4272-8272-727272727272"),
    ] {
        loom([
            "chat",
            "create-channel",
            path,
            "chatws",
            handle,
            name,
            "--channel-id",
            channel_id,
            "--format",
            "json",
        ])
        .unwrap();
    }
    let body = temp_bytes_file("mu17g-gc-initial", b"initial\0body");
    let posted = loom([
        "chat", "post", path, "chatws", "general", "m1", "--input", &body, "--format", "json",
    ])
    .unwrap();
    let _ = std::fs::remove_file(body);
    serde_json::from_str::<serde_json::Value>(&posted).unwrap()["entity_tag"]
        .as_str()
        .unwrap()
        .to_string()
}

fn mu17g_gc_chat_read_and_edit_report(
    globals: &[String],
    store: &str,
    stale_entity_tag: &str,
    tag: &str,
) -> Vec<(String, Vec<u8>)> {
    fn normalize(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(text) => {
                if (text.starts_with("blake3:") && text.len() == 71)
                    || text.starts_with("entity-tag:")
                {
                    *text = "<token>".to_string();
                } else if text.len() == 36
                    && text.chars().enumerate().all(|(idx, c)| {
                        matches!(idx, 8 | 13 | 18 | 23) && c == '-' || c.is_ascii_hexdigit()
                    })
                {
                    *text = "<uuid>".to_string();
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    normalize(item);
                }
            }
            serde_json::Value::Object(map) => {
                for (key, item) in map.iter_mut() {
                    if key.ends_with("_at_ms") && item.is_number() {
                        *item = serde_json::json!(0);
                    } else {
                        normalize(item);
                    }
                }
            }
            _ => {}
        }
    }

    fn normalized_json(output: &str) -> Vec<u8> {
        let mut value = serde_json::from_str::<serde_json::Value>(output).unwrap();
        normalize(&mut value);
        let mut output = serde_json::to_string(&value).unwrap();
        output.push('\n');
        output.into_bytes()
    }

    let run = |args: &[&str]| loom_with_globals(globals, args).unwrap();
    let mut report = Vec::new();
    for (label, args) in [
        (
            "chat.channels",
            vec!["chat", "channels", store, "chatws", "--format", "json"],
        ),
        (
            "chat.messages",
            vec![
                "chat", "messages", store, "chatws", "general", "--format", "json",
            ],
        ),
        (
            "chat.messages.empty",
            vec![
                "chat", "messages", store, "chatws", "empty", "--format", "json",
            ],
        ),
        (
            "chat.events",
            vec![
                "chat", "events", store, "chatws", "general", "--format", "json",
            ],
        ),
        (
            "chat.cursor",
            vec![
                "chat", "cursor", store, "chatws", "general", "--format", "json",
            ],
        ),
        (
            "chat.emoji-list.empty",
            vec!["chat", "emoji-list", store, "chatws", "--format", "json"],
        ),
    ] {
        let output = run(&args);
        if label == "chat.messages.empty" {
            let value = serde_json::from_str::<serde_json::Value>(&output).unwrap();
            assert_eq!(value["messages"], serde_json::json!([]));
        }
        if label == "chat.emoji-list.empty" {
            let value = serde_json::from_str::<serde_json::Value>(&output).unwrap();
            assert_eq!(value["custom"], serde_json::json!([]));
        }
        report.push((label.to_string(), normalized_json(&output)));
    }

    let edit = temp_bytes_file(&format!("{tag}-edit"), b"updated\0body");
    let compatible = run(&[
        "chat", "edit", store, "chatws", "general", "m1", "--input", &edit, "--format", "json",
    ]);
    report.push((
        "chat.edit.omitted-entity-tag".to_string(),
        normalized_json(&compatible),
    ));
    let messages = run(&[
        "chat", "messages", store, "chatws", "general", "--format", "json",
    ]);
    let messages_value = serde_json::from_str::<serde_json::Value>(&messages).unwrap();
    assert_eq!(
        messages_value["messages"][0]["body"],
        serde_json::json!([117, 112, 100, 97, 116, 101, 100, 0, 98, 111, 100, 121])
    );
    report.push((
        "chat.messages.after-compatible-edit".to_string(),
        normalized_json(&messages),
    ));

    let (ok, stdout, stderr) = loom_output_with_globals(
        globals,
        &[
            "chat",
            "edit",
            store,
            "chatws",
            "general",
            "m1",
            "--input",
            &edit,
            "--expected-entity-tag",
            stale_entity_tag,
            "--format",
            "json",
        ],
    );
    assert!(!ok, "stale edit unexpectedly succeeded: {stdout}");
    assert!(
        stderr.to_ascii_uppercase().contains("CONFLICT")
            && stderr.contains("expected_tag_mismatch"),
        "stale edit error: {stderr}"
    );
    report.push((
        "chat.edit.stale-entity-tag".to_string(),
        b"CONFLICT: expected_tag_mismatch\n".to_vec(),
    ));
    let _ = std::fs::remove_file(edit);
    report
}

struct Mu17gF2Fixtures {
    invalid: String,
    csv: String,
    fs_dir: String,
    archive: String,
    markdown_dir: String,
}

fn prepare_mu17g_f3_store(path: &str) -> (String, String) {
    drop(FileStore::create_with_profile(path, Algo::Blake3).unwrap());
    loom(["workspace", "create", path, "repo", "--facet", "vcs"]).unwrap();
    let first = loom(["vcs", "commit", path, "repo", "-m", "first"])
        .unwrap()
        .trim()
        .to_string();
    let second = loom(["vcs", "commit", path, "repo", "-m", "second"])
        .unwrap()
        .trim()
        .to_string();

    loom(["workspace", "create", path, "ai", "--facet", "inference"]).unwrap();
    loom([
        "inference",
        "instance",
        "create",
        path,
        "ai",
        "chat",
        "--model",
        "chat-model",
        "--kind",
        "llm",
        "--preset",
        "balanced",
        "--set",
        "temperature=0.2",
    ])
    .unwrap();
    loom([
        "inference",
        "instance",
        "create",
        path,
        "ai",
        "embed",
        "--model",
        "embedding-model",
        "--kind",
        "text-embedding",
        "--set",
        "batch_size=8",
    ])
    .unwrap();
    (first, second)
}

fn mu17g_f3_studio_vcs_inference_cli_report(
    globals: &[String],
    store: &str,
    first: &str,
    second: &str,
) -> Vec<(String, Vec<u8>)> {
    let run = |args: &[&str]| loom_with_globals(globals, args).unwrap();
    let mut report = Vec::new();
    let record = |label: &str, output: String, report: &mut Vec<(String, Vec<u8>)>| {
        report.push((label.to_string(), output.into_bytes()));
    };

    record(
        "inference.instance.list",
        String::from_utf8(mu17h_normalized_json(
            "inference.instance.list.json",
            run(&[
                "inference",
                "instance",
                "list",
                store,
                "ai",
                "--format",
                "json",
            ]),
        ))
        .unwrap(),
        &mut report,
    );
    record(
        "inference.instance.list.filtered",
        String::from_utf8(mu17h_normalized_json(
            "inference.instance.list.filtered.json",
            run(&[
                "inference",
                "instance",
                "list",
                store,
                "ai",
                "--kind",
                "text-embedding",
                "--format",
                "json",
            ]),
        ))
        .unwrap(),
        &mut report,
    );
    record(
        "inference.instance.show",
        run(&["inference", "instance", "show", store, "ai", "chat"]),
        &mut report,
    );
    report.push((
        "inference.instance.show.json".to_string(),
        mu17h_normalized_json(
            "inference.instance.show.json",
            run(&[
                "inference",
                "instance",
                "show",
                store,
                "ai",
                "chat",
                "--format",
                "json",
            ]),
        ),
    ));
    record(
        "inference.instance.show.resolved",
        run(&[
            "inference",
            "instance",
            "show",
            store,
            "ai",
            "chat",
            "--resolved",
        ]),
        &mut report,
    );
    report.push((
        "inference.instance.show.resolved.json".to_string(),
        mu17h_normalized_json(
            "inference.instance.show.resolved.json",
            run(&[
                "inference",
                "instance",
                "show",
                store,
                "ai",
                "chat",
                "--resolved",
                "--format",
                "json",
            ]),
        ),
    ));
    let (ok, stdout, stderr) = loom_output_with_globals(
        globals,
        &["inference", "instance", "show", store, "ai", "missing"],
    );
    assert!(!ok, "missing instance unexpectedly succeeded: {stdout}");
    assert!(
        stderr.contains("NotFound:") || stderr.contains("NOT_FOUND:"),
        "missing instance did not preserve NotFound: {stderr}"
    );
    let stderr = stderr.replace("NotFound:", "NOT_FOUND:");
    let stable = stderr
        .find("NOT_FOUND:")
        .map(|offset| format!("error: {}\n", stderr[offset..].trim()))
        .expect("stable NotFound error");
    report.push((
        "inference.instance.show.missing".to_string(),
        stable.into_bytes(),
    ));
    record("vcs.log", run(&["vcs", "log", store, "repo"]), &mut report);
    record(
        "vcs.diff.text",
        run(&["vcs", "diff", store, "repo", first, second]),
        &mut report,
    );
    report
}

impl Mu17gF2Fixtures {
    fn new(tag: &str) -> Self {
        let invalid = temp_bytes_file(&format!("{tag}-invalid"), b"not-canonical-input");
        let csv = temp_bytes_file(&format!("{tag}-csv"), b"id,name\n1,alpha\n");

        let mut fs_dir = std::env::temp_dir();
        fs_dir.push(format!(
            "loom-daemon-cli-authority-{tag}-fs-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(fs_dir.join("docs")).unwrap();
        std::fs::write(fs_dir.join("docs/a.txt"), b"alpha").unwrap();

        let mut markdown_dir = std::env::temp_dir();
        markdown_dir.push(format!(
            "loom-daemon-cli-authority-{tag}-markdown-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&markdown_dir).unwrap();
        std::fs::write(markdown_dir.join("page.md"), b"# Page\n\nBody\n").unwrap();

        let archive = temp_bytes_file(&format!("{tag}-archive"), b"");
        let file = std::fs::File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("docs/a.txt", options).unwrap();
        std::io::Write::write_all(&mut zip, b"alpha").unwrap();
        zip.finish().unwrap();

        Self {
            invalid,
            csv,
            fs_dir: fs_dir.to_string_lossy().into_owned(),
            archive,
            markdown_dir: markdown_dir.to_string_lossy().into_owned(),
        }
    }
}

impl Drop for Mu17gF2Fixtures {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.invalid);
        let _ = std::fs::remove_file(&self.csv);
        let _ = std::fs::remove_file(&self.archive);
        let _ = std::fs::remove_dir_all(&self.fs_dir);
        let _ = std::fs::remove_dir_all(&self.markdown_dir);
    }
}

fn prepare_mu17g_f2_store(path: &str) {
    loom(["store", "init", path]).unwrap();
    loom(["workspace", "create", path, "work", "--facet", "vcs"]).unwrap();
}

fn mu17g_f2_cli_report(
    globals: &[String],
    store: &str,
    fixtures: &Mu17gF2Fixtures,
    include_local_path_imports: bool,
) -> Vec<(String, Vec<u8>)> {
    fn normalize_cbor_value(value: &mut loom_codec::Value) {
        match value {
            loom_codec::Value::Uint(number) if *number >= 1_000_000_000_000 => {
                *number = 0;
            }
            loom_codec::Value::Text(text) => {
                if text.starts_with("blake3:") && text.len() == 71 {
                    *text = "<digest>".to_string();
                } else if text.starts_with("entity-tag:") {
                    *text = "<entity-tag>".to_string();
                } else if text.len() == 36
                    && text.chars().enumerate().all(|(index, ch)| {
                        (matches!(index, 8 | 13 | 18 | 23) && ch == '-') || ch.is_ascii_hexdigit()
                    })
                {
                    *text = "<uuid>".to_string();
                }
            }
            loom_codec::Value::Bytes(bytes) => {
                if let Ok(mut nested) = loom_codec::decode(bytes) {
                    normalize_cbor_value(&mut nested);
                    *bytes = loom_codec::encode(&nested).unwrap();
                }
            }
            loom_codec::Value::Array(items) => {
                for item in items {
                    normalize_cbor_value(item);
                }
            }
            loom_codec::Value::Map(items) => {
                for (_, item) in items {
                    normalize_cbor_value(item);
                }
            }
            _ => {}
        }
    }

    fn normalize_cbor_hex(text: &str) -> Option<String> {
        let bytes = hex::decode(text).ok()?;
        let mut value = loom_codec::decode(&bytes).ok()?;
        normalize_cbor_value(&mut value);
        loom_codec::encode(&value).ok().map(hex::encode)
    }

    fn normalize_value(value: &mut serde_json::Value, replace_cbor: bool) {
        match value {
            serde_json::Value::String(text) => {
                if text.starts_with("blake3:") && text.len() == 71 {
                    *text = "<digest>".to_string();
                } else if text.starts_with("entity-tag:") {
                    *text = "<entity-tag>".to_string();
                } else if text.len() == 36
                    && text.chars().enumerate().all(|(index, ch)| {
                        (matches!(index, 8 | 13 | 18 | 23) && ch == '-') || ch.is_ascii_hexdigit()
                    })
                {
                    *text = "<uuid>".to_string();
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    normalize_value(item, replace_cbor);
                }
            }
            serde_json::Value::Object(items) => {
                for (key, item) in items {
                    if key.ends_with("_cbor_hex") {
                        if replace_cbor {
                            *item = serde_json::Value::String("<canonical-cbor>".to_string());
                        } else if let Some(text) = item.as_str()
                            && let Some(normalized) = normalize_cbor_hex(text)
                        {
                            *item = serde_json::Value::String(normalized);
                        }
                    } else if key.ends_with("_at_ms") {
                        *item = serde_json::Value::String("<timestamp>".to_string());
                    } else {
                        normalize_value(item, replace_cbor);
                    }
                }
            }
            _ => {}
        }
    }

    fn normalize(text: String, default_text: bool) -> Vec<u8> {
        let text = text
            .replace("Lifecycle.", "")
            .replace("Refs.", "")
            .replace("Exec.", "")
            .replace("InterchangeProfiles.", "")
            .replace("Car.", "")
            .replace(" failed: ", ": ")
            .replace("lifecycle_define_json: ", "")
            .replace("refs_reconcile_json: ", "")
            .replace("exec_cbor: ", "")
            .replace("apply_cbor: ", "")
            .replace("import_redmine: ", "")
            .replace("import_asana: ", "")
            .replace("import_jira: ", "")
            .replace("import_confluence: ", "")
            .replace("import_slack: ", "")
            .replace("import_drive: ", "")
            .replace("import_notion: ", "")
            .replace("car_import: ", "")
            .replace("CorruptObject", "CORRUPT_OBJECT")
            .replace("InvalidArgument", "INVALID_ARGUMENT")
            .replace("NotFound", "NOT_FOUND")
            .replace("Conflict", "CONFLICT")
            .replace("PermissionDenied", "PERMISSION_DENIED")
            .replace("Internal", "INTERNAL");
        if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) {
            normalize_value(&mut value, !default_text);
            let mut out = if default_text {
                serde_json::to_string_pretty(&value).unwrap()
            } else {
                serde_json::to_string(&value).unwrap()
            };
            out.push('\n');
            return out.into_bytes();
        }
        let mut out = text;
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.into_bytes()
    }

    let mut report = Vec::new();
    let mut capture = |label: &str, args: &[&str]| {
        let (ok, stdout, stderr) = loom_output_with_globals(globals, args);
        let mut bytes = if ok {
            b"ok\n".to_vec()
        } else {
            b"error\n".to_vec()
        };
        bytes.extend(normalize(if ok { stdout } else { stderr }, false));
        report.push((label.to_string(), bytes));
    };

    let completion = Digest::hash(Algo::Blake3, b"mu17g-f2-completion").to_string();
    capture(
        "lifecycle.define-standard",
        &[
            "lifecycle",
            "define-standard",
            store,
            "work",
            "feature",
            "1",
            &completion,
            "--format",
            "json",
        ],
    );
    capture(
        "lifecycle.define",
        &[
            "lifecycle",
            "define",
            store,
            "work",
            &fixtures.invalid,
            "--format",
            "json",
        ],
    );
    capture(
        "lifecycle.instantiate",
        &[
            "lifecycle",
            "instantiate",
            store,
            "work",
            "instance-1",
            "feature",
            "--subject-ref",
            "page:roadmap",
            "--format",
            "json",
        ],
    );
    capture(
        "lifecycle.transition",
        &[
            "lifecycle",
            "transition",
            store,
            "work",
            "instance-1",
            "transition-1",
            "draft",
            "--gate-evaluations",
            r#"[{"gate_id":"enter-draft","passed":true}]"#,
            "--format",
            "json",
        ],
    );
    capture(
        "refs.reconcile",
        &[
            "refs",
            "reconcile",
            store,
            "work",
            "--max",
            "0",
            "--format",
            "json",
        ],
    );
    capture("exec.run", &["exec", "run", store, &fixtures.invalid]);
    capture(
        "exec.apply",
        &["exec", "apply", store, "work", "main", "missing-fork"],
    );

    if include_local_path_imports {
        capture(
            "interchange.import-fs",
            &[
                "interchange",
                "import-fs",
                store,
                "work",
                &fixtures.fs_dir,
                "--dry-run",
                "--format",
                "json",
            ],
        );
        capture(
            "interchange.import-archive",
            &[
                "interchange",
                "import-archive",
                store,
                "work",
                &fixtures.archive,
                "--kind",
                "zip",
                "--dry-run",
                "--format",
                "json",
            ],
        );
    }
    capture(
        "interchange.import-table-csv",
        &[
            "interchange",
            "import-table-csv",
            store,
            "work",
            "main",
            "items",
            &fixtures.csv,
            "--schema",
            "id:int,name:text",
            "--primary-key",
            "id",
            "--dry-run",
            "--format",
            "json",
        ],
    );
    for command in ["import-redmine", "import-asana", "import-jira"] {
        capture(
            &format!("interchange.{command}"),
            &[
                "interchange",
                command,
                store,
                "work",
                "work",
                &fixtures.invalid,
                "--field-policy",
                "strict",
                "--dry-run",
                "--format",
                "json",
            ],
        );
    }
    for command in ["import-slack", "import-drive"] {
        capture(
            &format!("interchange.{command}"),
            &[
                "interchange",
                command,
                store,
                "work",
                "work",
                &fixtures.invalid,
                "--dry-run",
                "--format",
                "json",
            ],
        );
    }
    for command in ["import-confluence", "import-notion"] {
        capture(
            &format!("interchange.{command}"),
            &[
                "interchange",
                command,
                store,
                "work",
                "work",
                &fixtures.invalid,
                "--dry-run",
                "--format",
                "json",
            ],
        );
    }
    capture(
        "interchange.import-markdown",
        &[
            "interchange",
            "import-markdown",
            store,
            "work",
            "work",
            &fixtures.markdown_dir,
            "--dry-run",
            "--format",
            "json",
        ],
    );
    capture(
        "interchange.import-car",
        &[
            "interchange",
            "import-car",
            store,
            &fixtures.invalid,
            "--dry-run",
            "--format",
            "json",
        ],
    );
    drop(capture);

    let mut capture_text = |label: &str, args: &[&str]| {
        let (ok, stdout, stderr) = loom_output_with_globals(globals, args);
        let mut bytes = if ok {
            b"ok\n".to_vec()
        } else {
            b"error\n".to_vec()
        };
        bytes.extend(normalize(if ok { stdout } else { stderr }, true));
        report.push((label.to_string(), bytes));
    };
    capture_text(
        "lifecycle.define-standard.text",
        &[
            "lifecycle",
            "define-standard",
            store,
            "work",
            "feature",
            "1",
            &completion,
        ],
    );
    capture_text(
        "lifecycle.define.invalid.text",
        &["lifecycle", "define", store, "work", &fixtures.invalid],
    );
    capture_text(
        "lifecycle.instantiate.text",
        &[
            "lifecycle",
            "instantiate",
            store,
            "work",
            "instance-text",
            "feature",
            "--subject-ref",
            "page:text-roadmap",
        ],
    );
    capture_text(
        "lifecycle.transition.text",
        &[
            "lifecycle",
            "transition",
            store,
            "work",
            "instance-text",
            "transition-text",
            "draft",
            "--gate-evaluations",
            r#"[{"gate_id":"enter-draft","passed":true}]"#,
        ],
    );
    capture_text(
        "refs.reconcile.text",
        &[
            "refs",
            "reconcile",
            store,
            "--workspace",
            "work",
            "--max",
            "0",
        ],
    );
    capture_text(
        "refs.reconcile.missing.text",
        &[
            "refs",
            "reconcile",
            store,
            "--workspace",
            "missing-workspace",
            "--max",
            "0",
        ],
    );
    report
}

fn mu17g_e2_chat_drive_mutation_report(
    globals: &[String],
    store: &str,
    initial_drive_root: &str,
    tag: &str,
) -> Vec<(String, Vec<u8>)> {
    fn normalize(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(text) => {
                if text.starts_with("blake3:") && text.len() == 71 {
                    *text = "<digest>".to_string();
                } else if text.starts_with("entity-tag:") {
                    *text = "<entity-tag>".to_string();
                } else if text.len() == 36
                    && text.chars().enumerate().all(|(idx, c)| {
                        matches!(idx, 8 | 13 | 18 | 23) && c == '-' || c.is_ascii_hexdigit()
                    })
                {
                    *text = "<uuid>".to_string();
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    normalize(item);
                }
            }
            serde_json::Value::Object(map) => {
                for (key, item) in map.iter_mut() {
                    if key == "timestamp_ms" && item.is_number() {
                        *item = serde_json::Value::Number(0.into());
                        continue;
                    }
                    normalize(item);
                }
            }
            _ => {}
        }
    }

    fn normalized(output: &str) -> Vec<u8> {
        if output.is_empty() {
            return Vec::new();
        }
        if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(output) {
            normalize(&mut value);
            let mut text = serde_json::to_string(&value).unwrap();
            text.push('\n');
            return text.into_bytes();
        }
        let mut text = output
            .replace("Chat.", "")
            .replace("Drive.", "")
            .replace(" failed: ", ": ")
            .replace("chat_edit_message_bytes_json: ", "")
            .replace("drive_stat_json: ", "")
            .replace("NotFound:", "NOT_FOUND:")
            .replace("Conflict:", "CONFLICT:");
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.into_bytes()
    }

    fn json_root(output: &str) -> String {
        serde_json::from_str::<serde_json::Value>(output).unwrap()["profile_root"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn json_conflict(output: &str) -> String {
        serde_json::from_str::<serde_json::Value>(output).unwrap()["conflict_id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    let run = |args: &[&str]| loom_with_globals(globals, args).unwrap();
    let mut report = Vec::new();
    let mut record = |label: &str, output: &str| {
        report.push((label.to_string(), normalized(output)));
    };
    let channel_id = "71717171-7171-4171-8171-717171717171";
    let agent_id = "72727272-7272-4272-8272-727272727272";
    let recipient_id = "73737373-7373-4373-8373-737373737373";
    let body = temp_bytes_file(&format!("{tag}-chat-body"), b"hello\0chat");
    let edit = temp_bytes_file(&format!("{tag}-chat-edit"), b"edited\0chat");
    let prompt = temp_bytes_file(&format!("{tag}-chat-prompt"), b"summarize\0chat");
    let chunk = temp_bytes_file(&format!("{tag}-drive-chunk"), b"drive\0bytes");

    let output = run(&[
        "chat",
        "create-channel",
        store,
        "chatws",
        "general",
        "General",
        "--channel-id",
        channel_id,
        "--format",
        "json",
    ]);
    record("chat.create-channel", &output);
    let output = run(&[
        "chat",
        "rename-channel",
        store,
        "chatws",
        "general",
        "team",
        "--format",
        "json",
    ]);
    record("chat.rename-channel", &output);
    let output = run(&["chat", "messages", store, "chatws", "team"]);
    record("chat.text.messages.empty", &output);
    let output = run(&[
        "chat", "post", store, "chatws", "team", "m1", "--input", &body, "--format", "json",
    ]);
    let stale_entity_tag =
        serde_json::from_str::<serde_json::Value>(&output).unwrap()["entity_tag"]
            .as_str()
            .unwrap()
            .to_string();
    record("chat.post", &output);
    let output = run(&[
        "chat", "edit", store, "chatws", "team", "m1", "--input", &edit, "--format", "json",
    ]);
    record("chat.edit", &output);
    let output = run(&["chat", "messages", store, "chatws", "team"]);
    record("chat.text.messages.after-edit", &output);
    let (ok, stdout, stderr) = loom_output_with_globals(
        globals,
        &[
            "chat",
            "edit",
            store,
            "chatws",
            "team",
            "m1",
            "--input",
            &edit,
            "--expected-entity-tag",
            &stale_entity_tag,
        ],
    );
    assert!(!ok, "stale Chat edit unexpectedly succeeded: {stdout}");
    assert!(
        stderr.to_ascii_uppercase().contains("CONFLICT")
            && stderr.contains("expected_tag_mismatch"),
        "stale Chat edit error: {stderr}"
    );
    record("chat.text.edit.stale", &stderr);
    let output = run(&[
        "chat",
        "create-thread",
        store,
        "chatws",
        "team",
        "thread-1",
        "m1",
        "--format",
        "json",
    ]);
    record("chat.create-thread", &output);
    let output = run(&[
        "chat", "post", store, "chatws", "team", "m2", "--thread", "thread-1", "--input", &body,
        "--format", "json",
    ]);
    record("chat.post-thread", &output);
    let output = run(&[
        "chat", "redact", store, "chatws", "team", "m2", "--reason", "cleanup", "--format", "json",
    ]);
    record("chat.redact", &output);
    let output = run(&[
        "chat",
        "create-task",
        store,
        "chatws",
        "team",
        "task-1",
        "Ship",
        "--message-id",
        "m1",
        "--format",
        "json",
    ]);
    record("chat.create-task", &output);
    let output = run(&[
        "chat",
        "claim-task",
        store,
        "chatws",
        "team",
        "task-1",
        "claim-1",
        "--lease-token",
        "lease-1",
        "--format",
        "json",
    ]);
    record("chat.claim-task", &output);
    let output = run(&[
        "chat",
        "complete-task",
        store,
        "chatws",
        "team",
        "task-1",
        "claim-1",
        "--result-message-id",
        "m1",
        "--format",
        "json",
    ]);
    record("chat.complete-task", &output);
    let output = run(&[
        "chat",
        "invoke-agent",
        store,
        "chatws",
        "team",
        "invoke-1",
        agent_id,
        "--source-message-ids",
        "m1",
        "--input",
        &prompt,
        "--format",
        "json",
    ]);
    record("chat.invoke-agent", &output);
    let output = run(&[
        "chat",
        "agent-reply",
        store,
        "chatws",
        "team",
        "invoke-1",
        "m1",
        "--format",
        "json",
    ]);
    record("chat.agent-reply", &output);
    let output = run(&[
        "chat",
        "request-handoff",
        store,
        "chatws",
        "team",
        "handoff-1",
        agent_id,
        "--to-principal",
        recipient_id,
        "--reason",
        "review",
        "--format",
        "json",
    ]);
    record("chat.request-handoff", &output);
    let output = run(&[
        "chat",
        "emoji-register",
        store,
        "chatws",
        "approved",
        "--format",
        "json",
    ]);
    record("chat.emoji-register", &output);
    let output = run(&[
        "chat",
        "add-reaction",
        store,
        "chatws",
        "team",
        "m1",
        "approved",
        "--format",
        "json",
    ]);
    record("chat.add-reaction", &output);
    let output = run(&[
        "chat",
        "remove-reaction",
        store,
        "chatws",
        "team",
        "m1",
        "approved",
        "--format",
        "json",
    ]);
    record("chat.remove-reaction", &output);
    let output = run(&[
        "chat",
        "emoji-unregister",
        store,
        "chatws",
        "approved",
        "--format",
        "json",
    ]);
    record("chat.emoji-unregister", &output);
    let output = run(&[
        "chat",
        "update-cursor",
        store,
        "chatws",
        "team",
        "2",
        "--format",
        "json",
    ]);
    record("chat.update-cursor", &output);

    let output = run(&[
        "drive",
        "create-folder",
        store,
        "drivews",
        "root",
        "folder-a",
        "A",
        initial_drive_root,
        "--format",
        "json",
    ]);
    let stale_root = json_root(&output);
    record("drive.create-folder-a", &output);
    let output = run(&[
        "drive",
        "create-folder",
        store,
        "drivews",
        "root",
        "folder-b",
        "B",
        &stale_root,
        "--format",
        "json",
    ]);
    let mut root = json_root(&output);
    record("drive.create-folder-b", &output);
    let output = run(&[
        "drive",
        "create-upload",
        store,
        "drivews",
        "upload-1",
        "root",
        "data.bin",
        "file-1",
        &root,
        "--created-at-ms",
        "10",
        "--format",
        "json",
    ]);
    record("drive.create-upload", &output);
    let output = run(&[
        "drive",
        "upload-chunk",
        store,
        "drivews",
        "upload-1",
        "--input",
        &chunk,
        "--format",
        "json",
    ]);
    record("drive.upload-chunk", &output);
    let output = run(&[
        "drive",
        "commit-upload",
        store,
        "drivews",
        "upload-1",
        "--format",
        "json",
    ]);
    root = json_root(&output);
    record("drive.commit-upload", &output);
    let output = run(&[
        "drive", "rename", store, "drivews", "root", "folder-a", "A2", &root, "--format", "json",
    ]);
    root = json_root(&output);
    record("drive.rename", &output);
    let output = run(&[
        "drive", "move", store, "drivews", "root", "folder-b", "folder-a", &root, "--format",
        "json",
    ]);
    root = json_root(&output);
    record("drive.move", &output);

    let output = run(&[
        "drive",
        "grant-share",
        store,
        "drivews",
        "grant-1",
        "folder",
        "folder-b",
        recipient_id,
        "viewer",
        "--granted-at-ms",
        "20",
        "--format",
        "json",
    ]);
    record("drive.grant-share", &output);
    let output = run(&[
        "drive",
        "apply-share-expiry",
        store,
        "drivews",
        "25",
        "--format",
        "json",
    ]);
    record("drive.apply-share-expiry", &output);
    let output = run(&[
        "drive",
        "revoke-share",
        store,
        "drivews",
        "grant-1",
        "--format",
        "json",
    ]);
    record("drive.revoke-share", &output);

    let output = run(&[
        "drive",
        "pin-retention",
        store,
        "drivews",
        "pin-1",
        "current_root",
        &root,
        "--target-entity-id",
        "folder:folder-b",
        "--added-at-ms",
        "30",
        "--format",
        "json",
    ]);
    record("drive.pin-retention", &output);
    let output = run(&[
        "drive",
        "apply-retention",
        store,
        "drivews",
        "35",
        "--format",
        "json",
    ]);
    record("drive.apply-retention", &output);
    let output = run(&[
        "drive",
        "unpin-retention",
        store,
        "drivews",
        "pin-1",
        "--format",
        "json",
    ]);
    record("drive.unpin-retention", &output);

    let output = run(&[
        "drive",
        "delete",
        store,
        "drivews",
        "folder-b",
        "folder-a",
        &stale_root,
        "--format",
        "json",
    ]);
    let conflict_id = json_conflict(&output);
    record("drive.delete-stale", &output);
    let output = run(&["drive", "list-conflicts", store, "drivews"]);
    record("drive.text.conflicts", &output);
    let output = run(&[
        "drive",
        "resolve-conflict",
        store,
        "drivews",
        &conflict_id,
        "keep-current",
        "--format",
        "json",
    ]);
    record("drive.resolve-conflict", &output);

    for (label, args) in [
        (
            "drive.list",
            vec![
                "drive", "list", store, "drivews", "root", "--format", "json",
            ],
        ),
        (
            "drive.stat",
            vec![
                "drive", "stat", store, "drivews", "root", "data.bin", "--format", "json",
            ],
        ),
        (
            "drive.list-versions",
            vec![
                "drive",
                "list-versions",
                store,
                "drivews",
                "file-1",
                "--format",
                "json",
            ],
        ),
        (
            "drive.list-conflicts",
            vec![
                "drive",
                "list-conflicts",
                store,
                "drivews",
                "--format",
                "json",
            ],
        ),
        (
            "drive.list-shares",
            vec!["drive", "list-shares", store, "drivews", "--format", "json"],
        ),
        (
            "drive.list-retention",
            vec![
                "drive",
                "list-retention",
                store,
                "drivews",
                "--format",
                "json",
            ],
        ),
    ] {
        let output = run(&args);
        record(label, &output);
    }
    let drive_out = temp_bytes_file(&format!("{tag}-drive-read"), b"");
    let output = run(&[
        "drive", "read", store, "drivews", "file-1", "--out", &drive_out,
    ]);
    assert!(output.is_empty(), "Drive read --out wrote stdout: {output}");
    let drive_read_bytes = std::fs::read(&drive_out).unwrap();
    let _ = std::fs::remove_file(drive_out);

    for (label, args) in [
        (
            "drive.text.list",
            vec!["drive", "list", store, "drivews", "root"],
        ),
        (
            "drive.text.conflicts.after-resolve",
            vec!["drive", "list-conflicts", store, "drivews"],
        ),
        (
            "drive.text.shares.empty",
            vec!["drive", "list-shares", store, "drivews"],
        ),
        (
            "drive.text.retention.empty",
            vec!["drive", "list-retention", store, "drivews"],
        ),
    ] {
        let output = run(&args);
        record(label, &output);
    }
    let (ok, stdout, stderr) = loom_output_with_globals(
        globals,
        &["drive", "stat", store, "drivews", "root", "missing.txt"],
    );
    assert!(!ok, "absent Drive stat unexpectedly succeeded: {stdout}");
    assert!(
        stderr
            .to_ascii_uppercase()
            .replace('_', "")
            .contains("NOTFOUND"),
        "absent Drive stat error: {stderr}"
    );
    record("drive.text.stat.absent", &stderr);
    drop(record);
    report.push(("drive.read.bytes".to_string(), drive_read_bytes));

    for path in [body, edit, prompt, chunk] {
        let _ = std::fs::remove_file(path);
    }
    report
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
fn mu17g_d3_hosted_auth_denial_report(globals: &[String], store: &str) -> Vec<(String, Vec<u8>)> {
    let (ok, stdout, stderr) = loom_output_with_globals(
        globals,
        &["tickets", "projects", store, "main", "--format", "json"],
    );
    assert!(!ok, "hosted denial unexpectedly succeeded:\n{stdout}");
    vec![(
        "hosted.tickets.projects.unauthenticated-denied.stderr".to_string(),
        stderr
            .replace("PermissionDenied:", "PERMISSION_DENIED:")
            .into_bytes(),
    )]
}

fn mu17g_d2_security_cli_report(
    globals: &[String],
    store: &str,
    tag: &str,
    include_auth_probe: bool,
) -> Vec<(String, Vec<u8>)> {
    fn dynamic_token(token: &str) -> Option<&'static str> {
        if token.starts_with("blake3:") && token.len() == 71 {
            return Some("<digest>");
        }
        if token.len() == 36
            && token.chars().enumerate().all(|(idx, c)| {
                matches!(idx, 8 | 13 | 18 | 23) && c == '-' || c.is_ascii_hexdigit()
            })
        {
            return Some("<uuid>");
        }
        if token.len() >= 13 && token.chars().all(|c| c.is_ascii_digit()) {
            return Some("<timestamp>");
        }
        None
    }

    fn normalized_text(text: String) -> Vec<u8> {
        let text = text
            .replace("Acl.acl_list failed: NotFound:", "NOT_FOUND:")
            .replace(
                "daemon-local generated session open failed: NotFound:",
                "NOT_FOUND:",
            )
            .replace("Acl.acl_revoke failed: NotFound:", "NOT_FOUND:")
            .replace(
                "ProtectedRefs.protected_ref_get failed: NotFound:",
                "NOT_FOUND:",
            );
        let mut lines = Vec::new();
        for line in text.lines() {
            if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(line) {
                normalize_json_value(&mut value);
                lines.push(serde_json::to_string(&value).unwrap());
                continue;
            }
            let mut normalized = String::new();
            for token in line.split_whitespace() {
                if !normalized.is_empty() {
                    normalized.push(' ');
                }
                if let Some(value) = dynamic_token(token) {
                    normalized.push_str(value);
                } else {
                    normalized.push_str(token);
                }
            }
            lines.push(normalized);
        }
        let mut out = lines.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        out.into_bytes()
    }

    fn normalize_json_value(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(text) => {
                if let Some(token) = dynamic_token(text) {
                    *text = token.to_string();
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    normalize_json_value(item);
                }
            }
            serde_json::Value::Object(map) => {
                for item in map.values_mut() {
                    normalize_json_value(item);
                }
            }
            _ => {}
        }
    }

    let mut report = Vec::new();
    let run = |args: &[&str]| loom_with_globals(globals, args).unwrap();
    let capture_error = |label: &str, args: &[&str], report: &mut Vec<(String, Vec<u8>)>| {
        let (ok, stdout, stderr) = loom_output_with_globals(globals, args);
        assert!(!ok, "{label} unexpectedly succeeded:\n{stdout}");
        report.push((format!("{label}.stderr"), normalized_text(stderr)));
    };
    let record = |label: &str, output: String, report: &mut Vec<(String, Vec<u8>)>| {
        report.push((label.to_string(), normalized_text(output)));
    };

    record(
        "identity.list.before",
        run(&["identity", "list", store]),
        &mut report,
    );
    record(
        "identity.public-key.list.before",
        run(&["identity", "public-key", "list", store]),
        &mut report,
    );
    record(
        "identity.authority-witness",
        run(&["identity", "authority-witness", store]),
        &mut report,
    );
    record(
        "identity.list-authority-replication.before",
        run(&["identity", "list-authority-replication", store]),
        &mut report,
    );

    record("acl.list.before", run(&["acl", "list", store]), &mut report);
    record(
        "acl.grant",
        run(&[
            "acl",
            "grant",
            store,
            "--effect",
            "allow",
            "--subject",
            "everyone",
            "--right",
            "read",
        ]),
        &mut report,
    );
    record("acl.list.after", run(&["acl", "list", store]), &mut report);
    record(
        "acl.revoke",
        run(&[
            "acl",
            "revoke",
            store,
            "--effect",
            "allow",
            "--subject",
            "everyone",
            "--right",
            "read",
        ]),
        &mut report,
    );
    record(
        "acl.revoke.absent",
        run(&[
            "acl",
            "revoke",
            store,
            "--effect",
            "allow",
            "--subject",
            "everyone",
            "--right",
            "read",
        ]),
        &mut report,
    );

    record(
        "protected-ref.get.absent",
        run(&["protected-ref", "get", store, "repo", "branch/main"]),
        &mut report,
    );
    record(
        "protected-ref.set",
        run(&[
            "protected-ref",
            "set",
            store,
            "repo",
            "branch/main",
            "--fast-forward-only",
            "--required-review-count",
            "2",
        ]),
        &mut report,
    );
    record(
        "protected-ref.list",
        run(&["protected-ref", "list", store, "repo"]),
        &mut report,
    );
    record(
        "protected-ref.get",
        run(&["protected-ref", "get", store, "repo", "branch/main"]),
        &mut report,
    );
    record(
        "protected-ref.remove",
        run(&["protected-ref", "remove", store, "repo", "branch/main"]),
        &mut report,
    );
    record(
        "protected-ref.remove.absent",
        run(&["protected-ref", "remove", store, "repo", "branch/main"]),
        &mut report,
    );

    if include_auth_probe {
        let bad_auth = temp_text_file(&format!("{tag}-bad-auth"), "wrong-pass");
        capture_error(
            "acl.list.unknown-principal-denied",
            &[
                "--auth-principal",
                "00000000-0000-4000-8000-000000000099",
                "--auth-key-source",
                &format!("file:{bad_auth}"),
                "acl",
                "list",
                store,
            ],
            &mut report,
        );
        let _ = std::fs::remove_file(bad_auth);
    }
    report
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
fn mu17g_d2_hosted_admin_denial_report(globals: &[String], store: &str) -> Vec<(String, Vec<u8>)> {
    let mut report = Vec::new();
    let capture_error = |label: &str, args: &[&str], report: &mut Vec<(String, Vec<u8>)>| {
        let (ok, stdout, stderr) = loom_output_with_globals(globals, args);
        assert!(!ok, "{label} unexpectedly succeeded:\n{stdout}");
        assert!(
            stderr.contains("PermissionDenied:") || stderr.contains("PERMISSION_DENIED:"),
            "{label} did not return PermissionDenied:\n{stderr}"
        );
        report.push((
            format!("{label}.stderr"),
            stderr
                .replace("PermissionDenied:", "PERMISSION_DENIED:")
                .into_bytes(),
        ));
    };
    capture_error(
        "hosted.identity.authority-witness.non-admin-denied",
        &["identity", "authority-witness", store],
        &mut report,
    );
    capture_error(
        "hosted.acl.grant.non-admin-denied",
        &[
            "acl",
            "grant",
            store,
            "--effect",
            "allow",
            "--subject",
            "everyone",
            "--right",
            "read",
        ],
        &mut report,
    );
    capture_error(
        "hosted.protected-ref.set.non-admin-denied",
        &["protected-ref", "set", store, "repo", "branch/main"],
        &mut report,
    );
    report
}

fn mu17g_d4_management_store_cli_report(globals: &[String], store: &str) -> Vec<(String, Vec<u8>)> {
    fn dynamic_token(token: &str) -> Option<&'static str> {
        if WorkspaceId::parse(token).is_ok() {
            return Some("<uuid>");
        }
        if Digest::parse(token).is_ok() {
            return Some("<digest>");
        }
        if token.len() >= 13 && token.chars().all(|c| c.is_ascii_digit()) {
            return Some("<timestamp>");
        }
        None
    }

    fn normalize_json_value(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(text) => {
                if let Some(token) = dynamic_token(text) {
                    *text = token.to_string();
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    normalize_json_value(item);
                }
            }
            serde_json::Value::Object(map) => {
                for item in map.values_mut() {
                    normalize_json_value(item);
                }
            }
            _ => {}
        }
    }

    fn normalize_d4_json(label: &str, value: &mut serde_json::Value) {
        normalize_json_value(value);
        if label.starts_with("store.stat.")
            && let serde_json::Value::Object(map) = value
        {
            for field in [
                "candidate_dead_pages",
                "candidate_segments",
                "generation",
                "last_validated_mark_epoch",
                "object_count",
                "physical_bytes",
                "physical_page_count",
                "reusable_free_pages",
                "segment_overflow",
                "touched_segments",
            ] {
                if map.contains_key(field) {
                    map.insert(
                        field.to_string(),
                        serde_json::Value::String("<stat>".into()),
                    );
                }
            }
        }
        if label.starts_with("store.policy.")
            && let serde_json::Value::Object(map) = value
        {
            if map.contains_key("audit_seq") {
                map.insert(
                    "audit_seq".to_string(),
                    serde_json::Value::String("<seq>".into()),
                );
            }
        }
    }

    fn normalized_text(label: &str, text: String) -> Vec<u8> {
        let mut lines = Vec::new();
        for line in text.lines() {
            if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(line) {
                normalize_d4_json(label, &mut value);
                lines.push(serde_json::to_string(&value).unwrap());
                continue;
            }
            let mut normalized = String::new();
            for token in line.split_whitespace() {
                if !normalized.is_empty() {
                    normalized.push(' ');
                }
                if let Some(value) = dynamic_token(token) {
                    normalized.push_str(value);
                } else {
                    normalized.push_str(token);
                }
            }
            lines.push(normalized);
        }
        let mut out = lines.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        out.into_bytes()
    }

    let mut report = Vec::new();
    let run = |args: &[&str]| loom_with_globals(globals, args).unwrap();
    let record = |label: &str, output: String, report: &mut Vec<(String, Vec<u8>)>| {
        report.push((label.to_string(), normalized_text(label, output)));
    };

    record(
        "store.stat.before",
        run(&["store", "stat", store]),
        &mut report,
    );
    record(
        "store.policy.before",
        run(&["store", "policy", store]),
        &mut report,
    );
    record(
        "store.policy.set-complete",
        run(&[
            "store",
            "policy",
            store,
            "--fips-required",
            "false",
            "--default-durability",
            "relaxed",
            "--facet-durability",
            "document=normal",
        ]),
        &mut report,
    );
    record(
        "store.policy.after-set",
        run(&["store", "policy", store]),
        &mut report,
    );
    record(
        "store.policy.clear-facet",
        run(&[
            "store",
            "policy",
            store,
            "--clear-facet-durability",
            "document",
        ]),
        &mut report,
    );
    record(
        "store.policy.after-clear",
        run(&["store", "policy", store]),
        &mut report,
    );
    record(
        "management.workspace.create",
        run(&[
            "management",
            "workspace",
            "create",
            store,
            "alpha",
            "--facet",
            "kv",
        ]),
        &mut report,
    );
    record(
        "management.workspace.list.after-create",
        run(&["management", "workspace", "list", store]),
        &mut report,
    );
    record(
        "management.workspace.rename",
        run(&["management", "workspace", "rename", store, "alpha", "beta"]),
        &mut report,
    );
    record(
        "management.workspace.list.after-rename",
        run(&["management", "workspace", "list", store]),
        &mut report,
    );
    record(
        "management.workspace.delete",
        run(&["management", "workspace", "delete", store, "beta"]),
        &mut report,
    );
    record(
        "management.workspace.list.after-delete",
        run(&["management", "workspace", "list", store]),
        &mut report,
    );
    record(
        "management.kv.config.set",
        run(&[
            "management",
            "kv",
            "config",
            "set",
            store,
            "--workspace",
            "kvspace",
            "cache",
            "--tier",
            "ephemeral",
            "--default-ttl-ms",
            "42",
        ]),
        &mut report,
    );
    record(
        "management.kv.config.get",
        run(&[
            "management",
            "kv",
            "config",
            "get",
            store,
            "--workspace",
            "kvspace",
            "cache",
        ]),
        &mut report,
    );
    record(
        "store.stat.after",
        run(&["store", "stat", store]),
        &mut report,
    );
    report
}

fn mu17g_d5_metric_descriptor_bytes() -> Vec<u8> {
    loom_core::MetricDescriptor::new(
        "requests".into(),
        String::new(),
        "1".into(),
        loom_core::MetricInstrumentKind::Counter,
        loom_core::MetricTemporality::Cumulative,
        vec!["method".into()],
        64,
        30_000,
    )
    .unwrap()
    .encode()
    .unwrap()
}

fn mu17g_f1_security_admin_cli_report(
    globals: &[String],
    store: &str,
    cert_chain: &str,
    private_key: &str,
) -> Vec<(String, Vec<u8>)> {
    fn normalize(label: &str, output: String) -> Vec<u8> {
        fn normalize_value(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Array(items) => {
                    for item in items {
                        normalize_value(item);
                    }
                }
                serde_json::Value::Object(fields) => {
                    for (name, value) in fields {
                        if name == "seq" || name.ends_with("_seq") {
                            *value = serde_json::Value::String("<seq>".into());
                        } else if name == "hash"
                            || name.ends_with("_hash")
                            || name == "digest"
                            || name.ends_with("_digest")
                        {
                            *value = serde_json::Value::String("<digest>".into());
                        } else {
                            normalize_value(value);
                        }
                    }
                }
                _ => {}
            }
        }

        let mut value: serde_json::Value = serde_json::from_str(output.trim())
            .unwrap_or_else(|error| panic!("{label} must return JSON, got {output:?}: {error}"));
        if label == "audit.list"
            && let Some(records) = value
                .get_mut("records")
                .and_then(serde_json::Value::as_array_mut)
        {
            records.retain(|record| {
                record
                    .get("action")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|action| {
                        action.starts_with("audit.")
                            || action.starts_with("certificate.")
                            || action.starts_with("network-access.")
                    })
            });
        }
        normalize_value(&mut value);
        serde_json::to_vec(&value).unwrap()
    }

    let run = |args: &[&str]| loom_with_globals(globals, args).unwrap();
    let mut report = Vec::new();
    macro_rules! record {
        ($label:expr, $args:expr $(,)?) => {
            report.push(($label.to_string(), normalize($label, run($args))))
        };
    }

    record!("audit.config-show", &["audit", "config", "show", store]);
    record!(
        "audit.config-set",
        &[
            "audit",
            "config",
            "set",
            store,
            "--retention-days",
            "30",
            "--legal-hold",
            "false",
        ],
    );
    let audit_list = run(&["audit", "list", store]);
    let audit_config_set_seq =
        serde_json::from_str::<serde_json::Value>(&audit_list).unwrap()["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["action"] == "audit.config.set")
            .and_then(|record| record["seq"].as_u64())
            .expect("audit.config.set sequence")
            .to_string();
    report.push((
        "audit.list".to_string(),
        normalize("audit.list", audit_list),
    ));
    record!(
        "audit.view",
        &["audit", "view", store, &audit_config_set_seq]
    );

    record!(
        "network-access.set",
        &[
            "network-access",
            "set",
            store,
            "office",
            "--description",
            "office network",
            "--default-action",
            "deny",
            "--allow-source",
            "127.0.0.1/32",
        ],
    );
    record!(
        "network-access.audit",
        &["network-access", "audit", store, "office"],
    );
    record!("network-access.list", &["network-access", "list", store]);
    record!(
        "network-access.remove",
        &["network-access", "remove", store, "office"],
    );

    record!("certificate.list", &["certificate", "list", store]);
    record!(
        "certificate.import",
        &[
            "certificate",
            "import",
            store,
            "imported",
            "--cert-chain",
            cert_chain,
            "--private-key",
            private_key,
            "--trust-bundle",
            cert_chain,
            "--force",
        ],
    );
    record!(
        "certificate.audit",
        &["certificate", "audit", store, "imported"],
    );

    let exported_cert = temp_bytes_file("mu17g-f1-export-cert", b"");
    let exported_key = temp_bytes_file("mu17g-f1-export-key", b"");
    let exported_trust = temp_bytes_file("mu17g-f1-export-trust", b"");
    let _ = std::fs::remove_file(&exported_cert);
    let _ = std::fs::remove_file(&exported_key);
    let _ = std::fs::remove_file(&exported_trust);
    record!(
        "certificate.export",
        &[
            "certificate",
            "export",
            store,
            "imported",
            "--cert-chain",
            &exported_cert,
            "--private-key",
            &exported_key,
            "--trust-bundle",
            &exported_trust,
            "--force",
        ],
    );
    record!(
        "certificate.remove",
        &["certificate", "remove", store, "imported"],
    );
    record!(
        "certificate.generate-self-signed",
        &[
            "certificate",
            "generate",
            "self-signed",
            store,
            "generated",
            "--dns",
            "localhost",
            "--days",
            "1",
            "--force",
        ],
    );
    record!(
        "audit.compact",
        &["audit", "compact", store, "--through-seq", "1"],
    );
    for (label, path) in [
        ("certificate.export.cert-chain", &exported_cert),
        ("certificate.export.private-key", &exported_key),
        ("certificate.export.trust-bundle", &exported_trust),
    ] {
        report.push((label.to_string(), std::fs::read(path).unwrap()));
        let _ = std::fs::remove_file(path);
    }
    report
}

fn assert_mu17g_f1_security_admin_reports_equal(
    expected: &[(String, Vec<u8>)],
    actual: &[(String, Vec<u8>)],
    target: &str,
) {
    assert_eq!(expected.len(), actual.len(), "{target} report length");
    for ((expected_label, expected_bytes), (actual_label, actual_bytes)) in
        expected.iter().zip(actual)
    {
        assert_eq!(expected_label, actual_label, "{target} report label");
        assert_eq!(
            expected_bytes,
            actual_bytes,
            "{target} diverged for {expected_label}: expected {}, actual {}",
            String::from_utf8_lossy(expected_bytes),
            String::from_utf8_lossy(actual_bytes)
        );
    }
}

fn mu17g_f4_serve_daemon_state_cli_report(
    globals: &[String],
    store: &str,
) -> Vec<(String, Vec<u8>)> {
    fn normalized_json(label: &str, output: &str) -> Vec<u8> {
        fn normalize(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Array(items) => {
                    for item in items {
                        normalize(item);
                    }
                }
                serde_json::Value::Object(fields) => {
                    for (name, value) in fields {
                        if name == "seq" || name.ends_with("_seq") {
                            *value = serde_json::Value::String("<seq>".into());
                        } else {
                            normalize(value);
                        }
                    }
                }
                _ => {}
            }
        }

        let mut value: serde_json::Value = serde_json::from_str(output.trim())
            .unwrap_or_else(|error| panic!("{label} must return JSON, got {output:?}: {error}"));
        normalize(&mut value);
        serde_json::to_vec(&value).unwrap()
    }

    fn normalized_maintenance_outcome(output: &str) -> Vec<u8> {
        output
            .trim_end()
            .split('\t')
            .map(|field| {
                if field.starts_with("elapsed_ms=") {
                    "elapsed_ms=<elapsed>"
                } else {
                    field
                }
            })
            .collect::<Vec<_>>()
            .join("\t")
            .into_bytes()
    }

    let run = |args: &[&str]| loom_with_globals(globals, args).unwrap();
    let mut report = Vec::new();
    let maintenance_run = run(&[
        "daemon",
        "maintenance",
        "run",
        store,
        "--max-segments",
        "1",
        "--max-pages",
        "1",
    ]);
    report.push((
        "daemon.maintenance-run".into(),
        normalized_maintenance_outcome(&maintenance_run),
    ));
    let admin = run(&[
        "serve",
        "configure",
        store,
        "admin",
        "--bind",
        "127.0.0.1:19443",
    ]);
    let admin_id = serde_json::from_str::<serde_json::Value>(&admin).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    report.push((
        "serve.configure-admin".into(),
        normalized_json("serve.configure-admin", &admin),
    ));
    report.push((
        "serve.list-after-admin".into(),
        normalized_json("serve.list-after-admin", &run(&["serve", "list", store])),
    ));
    report.push((
        "serve.disable".into(),
        normalized_json(
            "serve.disable",
            &run(&["serve", "disable", store, &admin_id]),
        ),
    ));
    report.push((
        "serve.enable".into(),
        normalized_json("serve.enable", &run(&["serve", "enable", store, &admin_id])),
    ));

    let web = run(&[
        "serve",
        "configure",
        store,
        "web",
        "site",
        "--bind",
        "127.0.0.1:19444",
    ]);
    let web_id = serde_json::from_str::<serde_json::Value>(&web).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    report.push((
        "serve.configure-web".into(),
        normalized_json("serve.configure-web", &web),
    ));
    report.push((
        "serve.route-set".into(),
        normalized_json(
            "serve.route-set",
            &run(&[
                "serve", "route", "set", store, &web_id, "--route", "docs", "--prefix", "/docs",
                "--root", "/",
            ]),
        ),
    ));
    report.push((
        "serve.route-list".into(),
        normalized_json(
            "serve.route-list",
            &run(&["serve", "route", "list", store, &web_id]),
        ),
    ));
    report.push((
        "serve.route-remove".into(),
        normalized_json(
            "serve.route-remove",
            &run(&["serve", "route", "remove", store, &web_id, "docs"]),
        ),
    ));
    for (label, id) in [
        ("serve.remove-web", &web_id),
        ("serve.remove-admin", &admin_id),
    ] {
        report.push((
            label.into(),
            normalized_json(label, &run(&["serve", "remove", store, id])),
        ));
    }

    let policy = run(&[
        "daemon",
        "maintenance",
        "policy",
        store,
        "--min-candidate-pages",
        "3",
        "--max-pages",
        "2",
        "--disallow-full-compaction",
        "--disable-tail-trim",
        "--disable-tail-compaction",
    ]);
    let policy_line = policy
        .lines()
        .find(|line| line.starts_with("maintenance_policy\t"))
        .unwrap_or_else(|| panic!("maintenance policy output missing policy line: {policy}"));
    report.push((
        "daemon.maintenance-policy".into(),
        policy_line.as_bytes().to_vec(),
    ));
    let status = run(&["daemon", "maintenance", "status", store, "--json"]);
    let status: serde_json::Value = serde_json::from_str(&status).unwrap();
    report.push((
        "daemon.maintenance-status-policy".into(),
        serde_json::to_vec(&status["policy"]).unwrap(),
    ));
    report
}

fn prepare_mu17g_f4_template(store: &str) {
    loom(["store", "init", store]).unwrap();
    loom(["workspace", "create", store, "site", "--facet", "files"]).unwrap();
    loom([
        "daemon",
        "maintenance",
        "policy",
        store,
        "--min-candidate-pages",
        "18446744073709551615",
        "--interval-ms",
        "86400000",
    ])
    .unwrap();
    let mut engine = loom_store::open_loom(store).unwrap();
    loom_store::gc_loom(&mut engine).unwrap();
}

fn assert_mu17g_f4_state_reports_equal(
    expected: &[(String, Vec<u8>)],
    actual: &[(String, Vec<u8>)],
    target: &str,
) {
    assert_eq!(expected.len(), actual.len(), "{target} report length");
    for ((expected_label, expected_bytes), (actual_label, actual_bytes)) in
        expected.iter().zip(actual)
    {
        assert_eq!(expected_label, actual_label, "{target} report label");
        assert_eq!(
            expected_bytes,
            actual_bytes,
            "{target} diverged for {expected_label}: expected {}, actual {}",
            String::from_utf8_lossy(expected_bytes),
            String::from_utf8_lossy(actual_bytes)
        );
    }
}

#[test]
fn mu17g_f4_daemon_shared_engine_preserves_ephemeral_kv_across_maintenance() {
    let mut store = DaemonStore::new("mu17g-f4-shared-engine-kv");
    let file_store = FileStore::create_with_profile(&store.path, Algo::Blake3).unwrap();
    let mut engine = Loom::new(file_store);
    let workspace_id = WorkspaceId::v4_from_bytes([0x4f; 16]);
    engine
        .registry_mut()
        .create(FacetKind::Kv, Some("cache"), workspace_id)
        .unwrap();
    engine
        .configure_kv_map(workspace_id, "sessions", loom_core::KvMapConfig::EPHEMERAL)
        .unwrap();
    loom_store::save_loom(&mut engine).unwrap();
    drop(engine);
    let key = loom_core::kv::key_to_cbor(&loom_core::Value::Text("k".into()));
    let key_hex = daemon::hex_encode(&key);
    let value_hex = daemon::hex_encode(b"value");

    store.start();
    wait_for_daemon_status(&store, "running\t");
    let paths = daemon::paths(&store.path).unwrap();
    assert_eq!(
        daemon::request_checked(
            &paths,
            &format!("kv-put\tmu17g-f4\tcache\tsessions\t{key_hex}\t{value_hex}\t1\n"),
        )
        .unwrap(),
        "ok\n"
    );
    assert_eq!(
        daemon::request_checked(
            &paths,
            &format!("kv-get\tmu17g-f4\tcache\tsessions\t{key_hex}\t2\n"),
        )
        .unwrap(),
        format!("kv\t1\t{value_hex}\n")
    );

    let status = loom(["daemon", "maintenance", "status", &store.path, "--json"]).unwrap();
    let status: serde_json::Value = serde_json::from_str(&status).unwrap();
    assert_eq!(
        status["group_commit"]["pinned_reader_blockers"].as_u64(),
        Some(0)
    );
    let maintenance = loom([
        "daemon",
        "maintenance",
        "run",
        &store.path,
        "--max-segments",
        "1",
        "--max-pages",
        "1",
    ])
    .unwrap();
    assert!(maintenance.starts_with("maintenance\t"), "{maintenance}");
    assert_eq!(
        daemon::request_checked(
            &paths,
            &format!("kv-get\tmu17g-f4\tcache\tsessions\t{key_hex}\t3\n"),
        )
        .unwrap(),
        format!("kv\t1\t{value_hex}\n")
    );

    store.stop();
    store.assert_runtime_artifacts_removed();
}

fn mu17g_d5_metric_observation_bytes() -> Vec<u8> {
    let descriptor = loom_core::MetricDescriptor::new(
        "requests".into(),
        String::new(),
        "1".into(),
        loom_core::MetricInstrumentKind::Counter,
        loom_core::MetricTemporality::Cumulative,
        vec!["method".into()],
        64,
        30_000,
    )
    .unwrap();
    loom_core::MetricObservation::new(
        descriptor.digest().unwrap(),
        BTreeMap::from([("method".to_string(), "GET".to_string())]),
        1,
        1.0,
    )
    .unwrap()
    .encode()
    .unwrap()
}

fn mu17g_d5_log_record_bytes() -> Vec<u8> {
    loom_core::LogRecord::new(
        10,
        Some(20),
        loom_core::LogSeverityNumber::new(13).unwrap(),
        "WARN".into(),
        loom_core::LogValue::String("cache miss".into()),
    )
    .unwrap()
    .with_context(
        BTreeMap::from([("cache.hit".into(), loom_core::LogValue::Bool(false))]),
        BTreeMap::from([(
            "service.name".into(),
            loom_core::LogValue::String("api".into()),
        )]),
        BTreeMap::from([("name".into(), loom_core::LogValue::String("loom".into()))]),
        None,
    )
    .unwrap()
    .encode()
    .unwrap()
}

fn mu17g_d5_span_record() -> loom_core::SpanRecord {
    loom_core::SpanRecord::new(
        loom_core::SpanContext::new([1; 16], [2; 8], 1).unwrap(),
        "GET /items".into(),
        loom_core::SpanKind::Server,
        10,
        20,
    )
    .unwrap()
}

fn mu17g_d5_program_metrics_logs_traces_cli_report(
    globals: &[String],
    store: &str,
) -> Vec<(String, Vec<u8>)> {
    let run = |args: &[&str]| loom_with_globals(globals, args).unwrap();
    let mut report = Vec::new();

    fn record_stdout(report: &mut Vec<(String, Vec<u8>)>, label: &str, output: String) {
        report.push((label.to_string(), output.into_bytes()));
    }

    fn record_file(
        report: &mut Vec<(String, Vec<u8>)>,
        run: &impl Fn(&[&str]) -> String,
        label: &str,
        args: &[&str],
    ) {
        let out = temp_bytes_file(label, b"");
        let mut full = args.to_vec();
        full.push("--out");
        full.push(&out);
        let stdout = run(&full);
        assert!(stdout.is_empty(), "{label} should write bytes to --out");
        let bytes = std::fs::read(&out).unwrap();
        let _ = std::fs::remove_file(out);
        report.push((label.to_string(), bytes));
    }

    let wasm = temp_bytes_file("mu17g-d5-wasm", b"\0asm\x01\0\0\0");
    let template = temp_text_file("mu17g-d5-template", "Hello, {{ name }}");
    let cel = temp_text_file("mu17g-d5-cel", "1 < 2");
    let metric_descriptor = temp_bytes_file(
        "mu17g-d5-metric-descriptor",
        &mu17g_d5_metric_descriptor_bytes(),
    );
    let metric_observation = temp_bytes_file(
        "mu17g-d5-metric-observation",
        &mu17g_d5_metric_observation_bytes(),
    );
    let log_record = temp_bytes_file("mu17g-d5-log-record", &mu17g_d5_log_record_bytes());
    let span = mu17g_d5_span_record();
    let span_trace_id = span.trace_id_hex();
    let span_id = span.span_id_hex();
    let span_file = temp_bytes_file("mu17g-d5-span", &span.encode().unwrap());

    record_file(
        &mut report,
        &run,
        "program.put-wasm",
        &[
            "program",
            "put-wasm",
            store,
            "programs",
            "wasm-file",
            "--input",
            &wasm,
        ],
    );
    record_file(
        &mut report,
        &run,
        "program.put-template",
        &[
            "program",
            "put-template",
            store,
            "programs",
            "template-card",
            "--input",
            &template,
        ],
    );
    record_file(
        &mut report,
        &run,
        "program.put-cel",
        &[
            "program",
            "put-cel",
            store,
            "programs",
            "cel-threshold",
            "--input",
            &cel,
        ],
    );
    record_file(
        &mut report,
        &run,
        "program.inspect",
        &["program", "inspect", store, "programs", "template-card"],
    );
    record_file(
        &mut report,
        &run,
        "program.get",
        &["program", "get", store, "programs", "template-card"],
    );
    record_file(
        &mut report,
        &run,
        "program.list",
        &["program", "list", store, "programs"],
    );
    record_stdout(
        &mut report,
        "program.remove",
        run(&["program", "remove", store, "programs", "cel-threshold"]),
    );

    record_stdout(
        &mut report,
        "metrics.put-descriptor",
        run(&[
            "metrics",
            "put-descriptor",
            store,
            "telemetry",
            "--input",
            &metric_descriptor,
        ]),
    );
    record_file(
        &mut report,
        &run,
        "metrics.get-descriptor",
        &["metrics", "get-descriptor", store, "telemetry", "requests"],
    );
    record_stdout(
        &mut report,
        "metrics.put-observation",
        run(&[
            "metrics",
            "put-observation",
            store,
            "telemetry",
            "requests",
            "--input",
            &metric_observation,
        ]),
    );
    record_file(
        &mut report,
        &run,
        "metrics.query",
        &[
            "metrics",
            "query",
            store,
            "telemetry",
            "requests",
            "--from",
            "0",
            "--to",
            "10",
            "--max-series",
            "16",
            "--max-groups",
            "16",
            "--max-samples",
            "64",
            "--max-output-bytes",
            "65536",
            "--now",
            "100",
        ],
    );

    let log_id = run(&[
        "logs",
        "put-record",
        store,
        "telemetry",
        "--input",
        &log_record,
    ]);
    record_stdout(&mut report, "logs.put-record", log_id.clone());
    let log_id = log_id.trim().to_string();
    record_file(
        &mut report,
        &run,
        "logs.get-record",
        &["logs", "get-record", store, "telemetry", &log_id],
    );
    record_file(
        &mut report,
        &run,
        "logs.query",
        &[
            "logs",
            "query",
            store,
            "telemetry",
            "--from",
            "0",
            "--to",
            "40",
            "--max-records",
            "16",
            "--max-output-bytes",
            "65536",
        ],
    );

    record_stdout(
        &mut report,
        "traces.put-span",
        run(&[
            "traces",
            "put-span",
            store,
            "telemetry",
            "--input",
            &span_file,
        ]),
    );
    record_file(
        &mut report,
        &run,
        "traces.get-span",
        &[
            "traces",
            "get-span",
            store,
            "telemetry",
            &span_trace_id,
            &span_id,
        ],
    );
    record_file(
        &mut report,
        &run,
        "traces.trace-spans",
        &[
            "traces",
            "trace-spans",
            store,
            "telemetry",
            &span_trace_id,
            "--max-spans",
            "16",
            "--max-output-bytes",
            "65536",
        ],
    );
    record_file(
        &mut report,
        &run,
        "traces.query",
        &[
            "traces",
            "query",
            store,
            "telemetry",
            "--from",
            "0",
            "--to",
            "40",
            "--max-spans",
            "16",
            "--max-output-bytes",
            "65536",
        ],
    );

    for path in [
        wasm,
        template,
        cel,
        metric_descriptor,
        metric_observation,
        log_record,
        span_file,
    ] {
        let _ = std::fs::remove_file(path);
    }

    report
}

fn document_get_text(store: &str, id: &str) -> String {
    loom(["document", "get-text", store, "main", "mu15d", id]).unwrap()
}

struct PriorDaemonFixture {
    child: Child,
    store: String,
    paths: daemon::DaemonPaths,
}

impl PriorDaemonFixture {
    fn start(store: &str) -> Self {
        Self::start_with_status(store, true, None)
    }

    fn start_unavailable_status(store: &str, transition_file: Option<&std::path::Path>) -> Self {
        Self::start_with_status(store, false, transition_file)
    }

    fn start_with_status(
        store: &str,
        status_available: bool,
        transition_file: Option<&std::path::Path>,
    ) -> Self {
        let paths = daemon::paths(store).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_loom"));
        command
            .env("LOOM_MU17H_PRIOR_DAEMON_STORE", store)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if !status_available {
            command.env("LOOM_MU17H_PRIOR_DAEMON_STATUS_UNAVAILABLE", "1");
        }
        if let Some(path) = transition_file {
            command.env("LOOM_MU17H_PRIOR_DAEMON_TRANSITION_FILE", path);
        }
        let child = command
            .spawn()
            .unwrap_or_else(|error| panic!("spawn prior daemon fixture: {error}"));
        let mut fixture = Self {
            child,
            store: store.to_string(),
            paths,
        };
        if status_available {
            fixture.wait_for_status();
        } else {
            fixture.wait_for_live_lock();
        }
        fixture
    }

    fn wait_for_status(&mut self) {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(10) {
            if let Ok(response) = daemon::request_checked(&self.paths, "status\n")
                && response.starts_with("running\tprotocol=1\ttransport=tcp\t")
            {
                return;
            }
            if let Some(status) = self.child.try_wait().unwrap() {
                let stderr = self
                    .child
                    .stderr
                    .take()
                    .map(|mut stderr| {
                        let mut text = String::new();
                        let _ = std::io::Read::read_to_string(&mut stderr, &mut text);
                        text
                    })
                    .unwrap_or_default();
                panic!("prior daemon fixture exited before status with {status}: {stderr}");
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!(
            "prior daemon fixture did not publish status for {}",
            self.store
        );
    }

    fn wait_for_live_lock(&mut self) {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(10) {
            if self.paths.lock_file.exists()
                && std::fs::read_to_string(&self.paths.lock_file)
                    .is_ok_and(|text| text.contains(&format!("pid={}", self.child.id())))
            {
                return;
            }
            if let Some(status) = self.child.try_wait().unwrap() {
                let stderr = self
                    .child
                    .stderr
                    .take()
                    .map(|mut stderr| {
                        let mut text = String::new();
                        let _ = std::io::Read::read_to_string(&mut stderr, &mut text);
                        text
                    })
                    .unwrap_or_default();
                panic!("prior daemon fixture exited before live lock with {status}: {stderr}");
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!(
            "prior daemon fixture did not publish lock for {}",
            self.store
        );
    }

    fn assert_runtime_artifacts_removed(&self) {
        assert!(
            !self.paths.addr_file.exists(),
            "prior daemon address artifact remained at {}",
            self.paths.addr_file.display()
        );
        assert!(
            !self.paths.pid_file.exists(),
            "prior daemon pid artifact remained at {}",
            self.paths.pid_file.display()
        );
        assert!(
            !self.paths.lock_file.exists(),
            "prior daemon lock artifact remained at {}",
            self.paths.lock_file.display()
        );
        assert!(
            !self.paths.sock_file.exists(),
            "prior daemon socket artifact remained at {}",
            self.paths.sock_file.display()
        );
    }

    fn terminate_and_assert_clean(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let status = self.child.wait().unwrap();
        assert!(
            !status.success(),
            "killed unavailable fixture exited successfully"
        );
        let _ = std::fs::remove_file(&self.paths.addr_file);
        let _ = std::fs::remove_file(&self.paths.pid_file);
        let _ = std::fs::remove_file(&self.paths.lock_file);
        let _ = std::fs::remove_file(&self.paths.sock_file);
        self.assert_runtime_artifacts_removed();
    }
}

impl Drop for PriorDaemonFixture {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = loom(["daemon", "stop", "--hard", &self.store]);
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.paths.addr_file);
        let _ = std::fs::remove_file(&self.paths.pid_file);
        let _ = std::fs::remove_file(&self.paths.lock_file);
        let _ = std::fs::remove_file(&self.paths.sock_file);
    }
}

fn wait_for_daemon_status(store: &DaemonStore, needle: &str) -> String {
    let started = Instant::now();
    let mut last = String::new();
    while started.elapsed() < Duration::from_secs(10) {
        last = loom(["daemon", "status", &store.path]).unwrap();
        if last.contains(needle) {
            return last;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("daemon status did not contain {needle:?}: {last}");
}

#[test]
fn mu17g_cli_smoke_commands_do_not_start_daemon() {
    let store = DaemonStore::new("mu17g-cli-smoke-no-daemon");
    FileStore::create_with_profile(&store.path, Algo::Blake3).unwrap();

    let version = loom(["version"]).unwrap();
    assert!(
        !version.trim().is_empty(),
        "version smoke command returned no output"
    );

    let status = loom(["daemon", "status", &store.path]).unwrap();
    assert!(
        status.starts_with("stopped\t"),
        "CLI smoke command unexpectedly started a daemon: {status}"
    );
}

#[test]
fn mu17h_prior_shape_detached_daemon_is_incompatible_but_stoppable() {
    let store = DaemonStore::new("mu17h-prior-shape");
    FileStore::create_with_profile(&store.path, Algo::Blake3).unwrap();
    let mut fixture = PriorDaemonFixture::start(&store.path);
    let pid = fixture.child.id().to_string();

    let status = loom(["daemon", "status", &store.path]).unwrap();
    assert!(
        status.starts_with("incompatible\t"),
        "prior daemon was not reported incompatible: {status}"
    );
    assert!(status.contains(&format!("pid={pid}")));
    assert!(status.contains(&format!("store={}", fixture.paths.store)));
    assert!(!status.starts_with("starting\t"));
    assert!(!status.contains("starting"));

    let (started, stdout, stderr) = loom_output(["daemon", "start", &store.path]);
    assert!(!started, "start unexpectedly succeeded:\n{stdout}");
    assert!(
        stderr.contains("incompatible daemon runtime contract"),
        "start did not report incompatible runtime:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(!stdout.contains("starting"));
    assert!(!stderr.contains("startup is already in progress"));

    let stopped = loom(["daemon", "stop", "--hard", &store.path]).unwrap();
    assert!(stopped.starts_with("stopped\t"), "stop failed: {stopped}");
    let exit = fixture.child.wait().unwrap();
    assert!(exit.success(), "prior daemon fixture exit was {exit}");
    fixture.assert_runtime_artifacts_removed();
    let status = loom(["daemon", "status", &store.path]).unwrap();
    assert!(
        status.starts_with("stopped\t"),
        "status after prior daemon stop was not stopped: {status}"
    );
}

#[test]
fn mu17h_unavailable_status_live_lock_is_not_starting_and_requires_manual_cleanup() {
    let store = DaemonStore::new("mu17h-unavailable-status");
    FileStore::create_with_profile(&store.path, Algo::Blake3).unwrap();
    let transition = {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-daemon-cli-authority-mu17h-transition-{}-{}.signal",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&path);
        path
    };
    let mut fixture = PriorDaemonFixture::start_unavailable_status(&store.path, Some(&transition));
    let pid = fixture.child.id().to_string();

    let status = loom(["daemon", "status", &store.path]).unwrap();
    assert!(
        status.starts_with("starting\t"),
        "parent-owned startup lock was not reported starting: {status}"
    );

    std::fs::write(&transition, b"running").unwrap();
    let status = wait_for_daemon_status(&store, "incompatible\tprotocol=unresponsive");
    assert!(
        status.starts_with("incompatible\tprotocol=unresponsive\t"),
        "unavailable live runtime was not reported incompatible: {status}"
    );
    assert!(status.contains(&format!("pid={pid}")));
    assert!(status.contains(&format!("store={}", fixture.paths.store)));
    assert!(status.contains(&format!("identity={}", fixture.paths.store_id)));
    assert!(status.contains("phase=running"));
    assert!(status.contains("startup_mode=persistent"));
    assert!(status.contains("startup_initiator=cli.daemon.start"));
    assert!(status.contains("manual_termination_required=true"));
    assert!(!status.starts_with("starting\t"));
    assert!(!status.contains("startup is already in progress"));

    let json = loom(["daemon", "status", "--json", &store.path]).unwrap();
    assert!(json.contains("\"state\":\"INCOMPATIBLE\""));
    assert!(json.contains("\"protocol\":\"unresponsive\""));
    assert!(json.contains(&format!("\"pid\":\"{pid}\"")));
    assert!(json.contains(&format!("\"store\":\"{}\"", fixture.paths.store)));
    assert!(json.contains(&format!("\"identity\":\"{}\"", fixture.paths.store_id)));
    assert!(json.contains("\"phase\":\"running\""));
    assert!(json.contains("\"startup_mode\":\"persistent\""));
    assert!(json.contains("\"startup_initiator\":\"cli.daemon.start\""));
    assert!(json.contains("\"manual_termination_required\":true"));

    let (started, stdout, stderr) = loom_output(["daemon", "start", &store.path]);
    assert!(!started, "start unexpectedly succeeded:\n{stdout}");
    assert!(
        stderr.contains("manual_termination_required=true"),
        "start did not report manual termination:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(!stdout.contains("starting"));
    assert!(!stderr.contains("startup is already in progress"));

    let (stopped, stdout, stderr) = loom_output(["daemon", "stop", "--hard", &store.path]);
    assert!(!stopped, "stop unexpectedly succeeded:\n{stdout}");
    assert!(
        stderr.contains("manual_termination_required=true"),
        "stop did not report manual termination:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    fixture.terminate_and_assert_clean();
    let _ = std::fs::remove_file(transition);
    let status = loom(["daemon", "status", &store.path]).unwrap();
    assert!(
        status.starts_with("stopped\t"),
        "status after unavailable fixture cleanup was not stopped: {status}"
    );
}

#[test]
fn mu15d_daemon_active_cli_routing_generated_read_uses_real_daemon() {
    let mut store = DaemonStore::new("mu15d-generated-read");
    loom(["store", "init", &store.path]).unwrap();
    document_put_text(&store.path, "read-source", "daemon routed read");
    store.start();
    wait_for_daemon_status(&store, "running\t");

    let text = document_get_text(&store.path, "read-source");
    assert_eq!(text, "daemon routed read");

    store.stop();
    store.assert_runtime_artifacts_removed();
}

#[test]
fn mu15d_daemon_active_cli_routing_generated_write_uses_real_daemon() {
    let mut store = DaemonStore::new("mu15d-generated-write");
    loom(["store", "init", &store.path]).unwrap();
    store.start();
    wait_for_daemon_status(&store, "running\t");

    document_put_text(&store.path, "write-target", "daemon routed write");
    let text = document_get_text(&store.path, "write-target");
    assert_eq!(text, "daemon routed write");

    store.stop();
    store.assert_runtime_artifacts_removed();
}

#[test]
fn mu17g_a_foundational_data_cli_parity_direct_local_and_daemon_local() {
    let direct = DaemonStore::new("mu17g-a-direct");
    loom(["store", "init", &direct.path]).unwrap();
    let direct_report = mu17g_a_foundational_cli_report(&[], &direct.path, "direct");

    let mut daemon_store = DaemonStore::new("mu17g-a-daemon");
    loom(["store", "init", &daemon_store.path]).unwrap();
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_a_foundational_cli_report(&[], &daemon_store.path, "daemon");
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    assert_eq!(
        direct_report, daemon_report,
        "MU-17g-a foundational CLI direct-local and daemon-local reports diverged"
    );
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
#[test]
fn mu17g_a_foundational_data_cli_parity_direct_daemon_and_remote() {
    let direct = DaemonStore::new("mu17g-a-all-direct");
    loom(["store", "init", &direct.path]).unwrap();
    let direct_report = mu17g_a_foundational_cli_report(&[], &direct.path, "all-direct");

    let mut daemon_store = DaemonStore::new("mu17g-a-all-daemon");
    loom(["store", "init", &daemon_store.path]).unwrap();
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_a_foundational_cli_report(&[], &daemon_store.path, "all-daemon");
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    let remote = RemoteServeStore::start("mu17g-a-all-remote");
    let remote_report = mu17g_a_foundational_cli_report(&remote.globals, "context", "all-remote");

    assert_eq!(
        direct_report, daemon_report,
        "MU-17g-a foundational CLI direct-local and daemon-local reports diverged"
    );
    assert_eq!(
        direct_report, remote_report,
        "MU-17g-a foundational CLI direct-local and remote reports diverged"
    );
}

fn mu17h_source_item<'a>(source: &'a str, declaration: &str) -> &'a str {
    let start = source
        .find(declaration)
        .unwrap_or_else(|| panic!("missing source declaration {declaration}"));
    let brace = source[start..]
        .find('{')
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("missing opening brace for {declaration}"));
    let mut depth = 0usize;
    for (offset, byte) in source[brace..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[start..=brace + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated source declaration {declaration}");
}

fn mu17h_enum_variant_has_json_selector(source: &str, enum_name: &str, variant: &str) -> bool {
    let enum_body = mu17h_source_item(source, &format!("pub(crate) enum {enum_name}"));
    let variant_body = mu17h_source_item(enum_body, &format!("    {variant} {{"));
    variant_body.contains("format: String")
}

#[test]
fn mu17h_c1_c2_json_inventory_and_report_content_is_complete() {
    let cli = include_str!("../src/cli.rs");
    let report_source = include_str!("daemon_cli_authority.rs");
    let b_report = mu17h_source_item(report_source, "fn mu17g_b_analytical_cli_report");
    let d1_report = mu17h_source_item(report_source, "fn mu17g_d1_core_cli_report");
    let f3_report = mu17h_source_item(report_source, "fn mu17g_f3_studio_vcs_inference_cli_report");

    for (enum_name, required, not_applicable) in [
        (
            "VectorCmd",
            &[][..],
            &[
                "Workspace",
                "Text",
                "Create",
                "Upsert",
                "UpsertSource",
                "Get",
                "Source",
                "Ids",
                "IndexKeys",
                "CreateIndex",
                "DropIndex",
                "Delete",
                "Search",
            ][..],
        ),
        ("VectorWorkspaceCmd", &["Configure"][..], &[][..]),
        ("VectorTextCmd", &["Upsert", "Query"][..], &[][..]),
        (
            "SearchCmd",
            &["Rebuild", "Status"][..],
            &["Create", "Index", "Get", "Delete", "Ids", "Remap", "Query"][..],
        ),
        (
            "WorkspaceCmd",
            &[][..],
            &["Create", "List", "Rename", "Delete"][..],
        ),
        (
            "FilesCmd",
            &[][..],
            &["Delete", "Ls", "Mkdir", "Read", "Write"][..],
        ),
        (
            "DocumentCmd",
            &[][..],
            &[
                "Delete",
                "DeleteCollection",
                "GetText",
                "PutText",
                "GetBinary",
                "PutBinary",
                "ListBinary",
                "Find",
                "Query",
                "IndexCreate",
                "IndexCreateJson",
                "IndexDrop",
                "IndexList",
                "IndexRebuild",
                "IndexStatus",
            ][..],
        ),
        (
            "PagesCmd",
            &[
                "SpaceCreate",
                "SpaceList",
                "SpaceGet",
                "Create",
                "Update",
                "Publish",
                "Get",
                "History",
                "StructureCreate",
                "StructureGet",
                "StructureAddNode",
                "StructureUpdateNode",
                "StructureBind",
                "StructureMoveNode",
                "StructureLinkNode",
                "StructureDecomposeToTickets",
            ][..],
            &[][..],
        ),
        (
            "IdentityCmd",
            &[][..],
            &[
                "List",
                "Add",
                "RenameHandle",
                "SetPassphrase",
                "CreateAppCredential",
                "RevokeAppCredential",
                "CreateExternalCredential",
                "RevokeExternalCredential",
                "PublicKey",
                "ForceDetachAuthority",
                "AuthorityWitness",
                "ReplicateAuthority",
                "ConfigureAuthorityReplication",
                "ListAuthorityReplication",
                "RemoveAuthorityReplication",
                "Remove",
                "AssignRole",
                "RevokeRole",
            ][..],
        ),
        ("AclCmd", &[][..], &["List", "Grant", "Revoke"][..]),
        (
            "ProtectedRefCmd",
            &[][..],
            &["List", "Get", "Set", "Remove"][..],
        ),
        ("ManagementKvConfigCmd", &[][..], &["Set", "Get"][..]),
        (
            "InferenceInstanceCmd",
            &["List", "Show"][..],
            &["Create", "Update", "Delete"][..],
        ),
        (
            "InferenceCmd",
            &["List", "Status", "Show", "Refresh"][..],
            &["Model", "Instance", "Download", "Cancel", "Remove"][..],
        ),
        (
            "InferenceModelCmd",
            &["List", "Show", "Status", "Refresh"][..],
            &["Download", "Cancel", "Remove"][..],
        ),
    ] {
        for variant in required {
            assert!(
                mu17h_enum_variant_has_json_selector(cli, enum_name, variant),
                "{enum_name}::{variant} lost its inventoried JSON selector"
            );
        }
        for variant in not_applicable {
            assert!(
                !mu17h_enum_variant_has_json_selector(cli, enum_name, variant),
                "{enum_name}::{variant} gained a JSON selector; update MU-17h inventory and parity"
            );
        }
    }

    for label in ["vector.workspace.configure", "fts.status"] {
        assert!(
            b_report.contains(&format!("\"{label}\"")),
            "missing B JSON case {label}"
        );
    }
    for label in [
        "pages.space-create.json",
        "pages.space-list.json",
        "pages.space-get.json",
        "pages.create.json",
        "pages.update.json",
        "pages.publish.json",
        "pages.get.json",
        "pages.history.json",
        "pages.structure-create.json",
        "pages.structure-get.json",
        "pages.structure-add-node.json",
        "pages.structure-update-node.json",
        "pages.structure-bind.json",
        "pages.structure-move-node.json",
        "pages.structure-link-node.json",
        "pages.structure-decompose-to-tickets.json",
    ] {
        assert!(
            d1_report.contains(&format!("\"{label}\"")),
            "missing D1 JSON case {label}"
        );
    }
    for label in [
        "inference.instance.list.json",
        "inference.instance.list.filtered.json",
        "inference.instance.show.json",
        "inference.instance.show.resolved.json",
    ] {
        assert!(
            f3_report.contains(&format!("\"{label}\"")),
            "missing F3 JSON case {label}"
        );
    }
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
#[test]
fn mu17g_b_analytical_data_cli_parity_direct_daemon_and_remote() {
    let template = DaemonStore::new("mu17g-b-template");
    prepare_mu17g_b_store(&template.path);

    let direct = DaemonStore::new("mu17g-b-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report = mu17g_b_analytical_cli_report(&[], &direct.path, "b-direct");

    let mut daemon_store = DaemonStore::new("mu17g-b-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_b_analytical_cli_report(&[], &daemon_store.path, "b-daemon");
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    let remote_store = DaemonStore::new("mu17g-b-remote");
    copy_store_bytes(&template.path, &remote_store.path);
    let remote_path = remote_store.path.clone();
    let remote = RemoteServeStore::start_existing("mu17g-b-remote", remote_path);
    let remote_report = mu17g_b_analytical_cli_report(&remote.globals, "context", "b-remote");

    assert_eq!(
        direct_report, daemon_report,
        "MU-17g-b analytical CLI direct-local and daemon-local reports diverged"
    );
    assert_eq!(
        direct_report, remote_report,
        "MU-17g-b analytical CLI direct-local and remote reports diverged"
    );
}

#[test]
fn mu17g_c_pim_cli_parity_direct_local_and_daemon_local() {
    let direct = DaemonStore::new("mu17g-c-direct");
    loom(["store", "init", &direct.path]).unwrap();
    let direct_report = mu17g_c_pim_cli_report(&[], &direct.path, "c-direct");

    let mut daemon_store = DaemonStore::new("mu17g-c-daemon");
    loom(["store", "init", &daemon_store.path]).unwrap();
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_c_pim_cli_report(&[], &daemon_store.path, "c-daemon");
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    assert_eq!(
        direct_report, daemon_report,
        "MU-17g-c PIM CLI direct-local and daemon-local reports diverged"
    );
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
#[test]
fn mu17g_c_pim_cli_parity_direct_daemon_and_remote() {
    let direct = DaemonStore::new("mu17g-c-all-direct");
    loom(["store", "init", &direct.path]).unwrap();
    let direct_report = mu17g_c_pim_cli_report(&[], &direct.path, "c-all-direct");

    let mut daemon_store = DaemonStore::new("mu17g-c-all-daemon");
    loom(["store", "init", &daemon_store.path]).unwrap();
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_c_pim_cli_report(&[], &daemon_store.path, "c-all-daemon");
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    let remote = RemoteServeStore::start("mu17g-c-remote");
    let remote_report = mu17g_c_pim_cli_report(&remote.globals, "context", "c-remote");

    assert_eq!(
        direct_report, daemon_report,
        "MU-17g-c PIM CLI direct-local and daemon-local reports diverged"
    );
    assert_eq!(
        direct_report, remote_report,
        "MU-17g-c PIM CLI direct-local and remote reports diverged"
    );
}

#[test]
fn mu17g_d1_core_cli_parity_direct_local_and_daemon_local() {
    let template = DaemonStore::new("mu17g-d1-template");
    prepare_mu17g_d1_store(&template.path);

    let direct = DaemonStore::new("mu17g-d1-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report = mu17g_d1_core_cli_report(&[], &direct.path, "d1-direct");

    let mut daemon_store = DaemonStore::new("mu17g-d1-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_d1_core_cli_report(&[], &daemon_store.path, "d1-daemon");
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    assert_eq!(
        direct_report, daemon_report,
        "MU-17g-d1 core CLI direct-local and daemon-local reports diverged"
    );
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
#[test]
fn mu17g_d1_core_cli_parity_direct_daemon_and_remote() {
    let template = DaemonStore::new("mu17g-d1-all-template");
    prepare_mu17g_d1_store(&template.path);

    let direct = DaemonStore::new("mu17g-d1-all-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report = mu17g_d1_core_cli_report(&[], &direct.path, "d1-all-direct");

    let mut daemon_store = DaemonStore::new("mu17g-d1-all-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_d1_core_cli_report(&[], &daemon_store.path, "d1-all-daemon");
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    let remote_store = DaemonStore::new("mu17g-d1-remote");
    copy_store_bytes(&template.path, &remote_store.path);
    let remote_path = remote_store.path.clone();
    let remote = RemoteServeStore::start_existing("mu17g-d1-remote", remote_path);
    let remote_report = mu17g_d1_core_cli_report(&remote.globals, "context", "d1-remote");

    assert_eq!(
        direct_report, daemon_report,
        "MU-17g-d1 core CLI direct-local and daemon-local reports diverged"
    );
    assert_eq!(
        direct_report, remote_report,
        "MU-17g-d1 core CLI direct-local and remote reports diverged"
    );
}

#[test]
fn mu17g_d2_security_cli_parity_direct_local_and_daemon_local() {
    let template = DaemonStore::new("mu17g-d2-template");
    prepare_mu17g_d2_store(&template.path);

    let direct = DaemonStore::new("mu17g-d2-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report = mu17g_d2_security_cli_report(&[], &direct.path, "d2-direct", true);

    let mut daemon_store = DaemonStore::new("mu17g-d2-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_d2_security_cli_report(&[], &daemon_store.path, "d2-daemon", true);
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    assert_eq!(
        direct_report, daemon_report,
        "MU-17g-d2 security CLI direct-local and daemon-local reports diverged"
    );
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
#[test]
fn mu17g_d2_security_cli_parity_direct_daemon_and_remote() {
    let template = DaemonStore::new("mu17g-d2-all-template");
    prepare_mu17g_d2_store(&template.path);

    let direct = DaemonStore::new("mu17g-d2-all-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report = mu17g_d2_security_cli_report(&[], &direct.path, "d2-all-direct", false);

    let mut daemon_store = DaemonStore::new("mu17g-d2-all-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report =
        mu17g_d2_security_cli_report(&[], &daemon_store.path, "d2-all-daemon", false);
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    let remote_store = DaemonStore::new("mu17g-d2-remote");
    copy_store_bytes(&template.path, &remote_store.path);
    let remote_path = remote_store.path.clone();
    let remote = RemoteServeStore::start_existing("mu17g-d2-remote", remote_path);
    let remote_report =
        mu17g_d2_security_cli_report(&remote.globals, "context", "d2-remote", false);

    assert_eq!(
        direct_report, daemon_report,
        "MU-17g-d2 security CLI direct-local and daemon-local reports diverged"
    );
    assert_eq!(
        direct_report, remote_report,
        "MU-17g-d2 security CLI direct-local and remote reports diverged"
    );
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
#[test]
fn mu17g_d2_hosted_authenticated_non_admin_is_denied() {
    let remote_store = DaemonStore::new("mu17g-d2-denied");
    let (user_id, user_pass) =
        prepare_mu17g_d2_authenticated_non_admin_store(&remote_store.path, "mu17g-d2-denied");
    let remote_path = remote_store.path.clone();
    let remote = RemoteServeStore::start_existing("mu17g-d2-denied", remote_path);
    let mut denied_globals = remote.globals.clone();
    denied_globals.push("--auth-principal".to_string());
    denied_globals.push(user_id);
    denied_globals.push("--auth-key-source".to_string());
    denied_globals.push(format!("file:{user_pass}"));

    let denied_report = mu17g_d2_hosted_admin_denial_report(&denied_globals, "context");
    assert_eq!(denied_report.len(), 3, "{denied_report:?}");
    let _ = std::fs::remove_file(user_pass);
}

#[test]
fn mu17g_d4_management_store_cli_parity_direct_local_and_daemon_local() {
    let template = DaemonStore::new("mu17g-d4-template");
    loom(["store", "init", &template.path]).unwrap();

    let direct = DaemonStore::new("mu17g-d4-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report = mu17g_d4_management_store_cli_report(&[], &direct.path);

    let mut daemon_store = DaemonStore::new("mu17g-d4-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_d4_management_store_cli_report(&[], &daemon_store.path);
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    assert_eq!(
        direct_report, daemon_report,
        "MU-17g-d4 management/store CLI direct-local and daemon-local reports diverged"
    );
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
#[test]
fn mu17g_d4_management_store_cli_parity_direct_daemon_and_remote() {
    let template = DaemonStore::new("mu17g-d4-all-template");
    loom(["store", "init", &template.path]).unwrap();

    let direct = DaemonStore::new("mu17g-d4-all-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report = mu17g_d4_management_store_cli_report(&[], &direct.path);

    let mut daemon_store = DaemonStore::new("mu17g-d4-all-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_d4_management_store_cli_report(&[], &daemon_store.path);
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    let remote_store = DaemonStore::new("mu17g-d4-remote");
    copy_store_bytes(&template.path, &remote_store.path);
    let remote = RemoteServeStore::start_existing("mu17g-d4-remote", remote_store.path.clone());
    let remote_report = mu17g_d4_management_store_cli_report(&remote.globals, "context");

    assert_eq!(
        direct_report, daemon_report,
        "MU-17g-d4 management/store CLI direct-local and daemon-local reports diverged"
    );
    assert_eq!(
        direct_report, remote_report,
        "MU-17g-d4 management/store CLI direct-local and remote reports diverged"
    );
}

#[test]
fn mu17g_d5_program_metrics_logs_traces_cli_parity_direct_local_and_daemon_local() {
    let template = DaemonStore::new("mu17g-d5-template");
    loom(["store", "init", &template.path]).unwrap();

    let direct = DaemonStore::new("mu17g-d5-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report = mu17g_d5_program_metrics_logs_traces_cli_report(&[], &direct.path);

    let mut daemon_store = DaemonStore::new("mu17g-d5-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_d5_program_metrics_logs_traces_cli_report(&[], &daemon_store.path);
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    assert_eq!(
        direct_report, daemon_report,
        "MU-17g-d5 Program/Metrics/Logs/Traces CLI direct-local and daemon-local reports diverged"
    );
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
#[test]
fn mu17g_d5_program_metrics_logs_traces_cli_parity_direct_daemon_and_remote() {
    let template = DaemonStore::new("mu17g-d5-all-template");
    loom(["store", "init", &template.path]).unwrap();

    let direct = DaemonStore::new("mu17g-d5-all-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report = mu17g_d5_program_metrics_logs_traces_cli_report(&[], &direct.path);

    let mut daemon_store = DaemonStore::new("mu17g-d5-all-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_d5_program_metrics_logs_traces_cli_report(&[], &daemon_store.path);
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    let remote_store = DaemonStore::new("mu17g-d5-remote");
    copy_store_bytes(&template.path, &remote_store.path);
    let remote = RemoteServeStore::start_existing("mu17g-d5-remote", remote_store.path.clone());
    let remote_report = mu17g_d5_program_metrics_logs_traces_cli_report(&remote.globals, "context");

    assert_eq!(
        direct_report, daemon_report,
        "MU-17g-d5 Program/Metrics/Logs/Traces CLI direct-local and daemon-local reports diverged"
    );
    assert_eq!(
        direct_report, remote_report,
        "MU-17g-d5 Program/Metrics/Logs/Traces CLI direct-local and remote reports diverged"
    );
}

#[test]
fn mu17g_f1_security_admin_cli_parity_direct_local_and_daemon_local() {
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let cert_chain = temp_text_file("mu17g-f1-cert", &certificate.cert.pem());
    let private_key = temp_text_file("mu17g-f1-key", &certificate.signing_key.serialize_pem());
    let template = DaemonStore::new("mu17g-f1-template");
    loom(["store", "init", &template.path]).unwrap();

    let direct = DaemonStore::new("mu17g-f1-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report =
        mu17g_f1_security_admin_cli_report(&[], &direct.path, &cert_chain, &private_key);

    let mut daemon_store = DaemonStore::new("mu17g-f1-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report =
        mu17g_f1_security_admin_cli_report(&[], &daemon_store.path, &cert_chain, &private_key);
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    assert_mu17g_f1_security_admin_reports_equal(
        &direct_report,
        &daemon_report,
        "MU-17g-f1 direct-local/daemon-local",
    );
    let _ = std::fs::remove_file(cert_chain);
    let _ = std::fs::remove_file(private_key);
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
#[test]
fn mu17g_f1_security_admin_cli_parity_direct_daemon_and_remote() {
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let cert_chain = temp_text_file("mu17g-f1-all-cert", &certificate.cert.pem());
    let private_key = temp_text_file("mu17g-f1-all-key", &certificate.signing_key.serialize_pem());
    let template = DaemonStore::new("mu17g-f1-all-template");
    loom(["store", "init", &template.path]).unwrap();

    let direct = DaemonStore::new("mu17g-f1-all-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report =
        mu17g_f1_security_admin_cli_report(&[], &direct.path, &cert_chain, &private_key);

    let mut daemon_store = DaemonStore::new("mu17g-f1-all-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report =
        mu17g_f1_security_admin_cli_report(&[], &daemon_store.path, &cert_chain, &private_key);
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    let remote_store = DaemonStore::new("mu17g-f1-remote");
    copy_store_bytes(&template.path, &remote_store.path);
    let remote = RemoteServeStore::start_existing("mu17g-f1-remote", remote_store.path.clone());
    let remote_report =
        mu17g_f1_security_admin_cli_report(&remote.globals, "context", &cert_chain, &private_key);

    assert_mu17g_f1_security_admin_reports_equal(
        &direct_report,
        &daemon_report,
        "MU-17g-f1 direct-local/daemon-local",
    );
    assert_mu17g_f1_security_admin_reports_equal(
        &direct_report,
        &remote_report,
        "MU-17g-f1 direct-local/remote",
    );
    let _ = std::fs::remove_file(cert_chain);
    let _ = std::fs::remove_file(private_key);
}

#[test]
fn mu17g_f3_studio_vcs_inference_cli_parity_direct_local_and_daemon_local() {
    let template = DaemonStore::new("mu17g-f3-template");
    let (first, second) = prepare_mu17g_f3_store(&template.path);

    let direct = DaemonStore::new("mu17g-f3-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report =
        mu17g_f3_studio_vcs_inference_cli_report(&[], &direct.path, &first, &second);

    let mut daemon_store = DaemonStore::new("mu17g-f3-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report =
        mu17g_f3_studio_vcs_inference_cli_report(&[], &daemon_store.path, &first, &second);
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    assert_eq!(
        direct_report, daemon_report,
        "MU-17g-f3 direct-local and daemon-local reports diverged"
    );
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
#[test]
fn mu17g_f3_studio_vcs_inference_cli_parity_direct_daemon_and_remote() {
    let template = DaemonStore::new("mu17g-f3-all-template");
    let (first, second) = prepare_mu17g_f3_store(&template.path);

    let direct = DaemonStore::new("mu17g-f3-all-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report =
        mu17g_f3_studio_vcs_inference_cli_report(&[], &direct.path, &first, &second);

    let mut daemon_store = DaemonStore::new("mu17g-f3-all-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report =
        mu17g_f3_studio_vcs_inference_cli_report(&[], &daemon_store.path, &first, &second);
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    let remote_store = DaemonStore::new("mu17g-f3-remote");
    copy_store_bytes(&template.path, &remote_store.path);
    let remote = RemoteServeStore::start_existing("mu17g-f3-remote", remote_store.path.clone());
    let remote_report =
        mu17g_f3_studio_vcs_inference_cli_report(&remote.globals, "context", &first, &second);

    assert_eq!(
        direct_report, daemon_report,
        "MU-17g-f3 direct-local and daemon-local reports diverged"
    );
    assert_eq!(
        direct_report, remote_report,
        "MU-17g-f3 direct-local and hosted-remote reports diverged"
    );
}

#[test]
fn mu17g_f4_serve_daemon_state_cli_parity_direct_local_and_daemon_local() {
    let template = DaemonStore::new("mu17g-f4-template");
    prepare_mu17g_f4_template(&template.path);

    let direct = DaemonStore::new("mu17g-f4-direct");
    copy_store_bytes(&template.path, &direct.path);
    let mut direct = direct;
    direct.prewarm_daemon_engine_without_audit();
    let direct_report = mu17g_f4_serve_daemon_state_cli_report(&[], &direct.path);

    let mut daemon_store = DaemonStore::new("mu17g-f4-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start_without_start_audit();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_f4_serve_daemon_state_cli_report(&[], &daemon_store.path);
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    assert_mu17g_f4_state_reports_equal(
        &direct_report,
        &daemon_report,
        "MU-17g-f4 direct-local/daemon-local",
    );
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
#[test]
fn mu17g_f4_serve_daemon_state_cli_parity_direct_daemon_and_remote() {
    let template = DaemonStore::new("mu17g-f4-all-template");
    prepare_mu17g_f4_template(&template.path);

    let direct = DaemonStore::new("mu17g-f4-all-direct");
    copy_store_bytes(&template.path, &direct.path);
    let mut direct = direct;
    direct.prewarm_daemon_engine_without_audit();
    let direct_report = mu17g_f4_serve_daemon_state_cli_report(&[], &direct.path);

    let mut daemon_store = DaemonStore::new("mu17g-f4-all-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start_without_start_audit();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_f4_serve_daemon_state_cli_report(&[], &daemon_store.path);
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    let mut remote_store = DaemonStore::new("mu17g-f4-remote");
    copy_store_bytes(&template.path, &remote_store.path);
    remote_store.prewarm_daemon_engine_without_audit();
    let remote = RemoteServeStore::start_existing("mu17g-f4-remote", remote_store.path.clone());
    let remote_report = mu17g_f4_serve_daemon_state_cli_report(&remote.globals, "context");

    assert_mu17g_f4_state_reports_equal(
        &direct_report,
        &daemon_report,
        "MU-17g-f4 direct-local/daemon-local",
    );
    assert_mu17g_f4_state_reports_equal(
        &direct_report,
        &remote_report,
        "MU-17g-f4 direct-local/remote",
    );
}

#[test]
fn mu17h_b1_d3_default_text_report_covers_success_conflict_and_absence() {
    let store = DaemonStore::new("mu17h-b1-d3-text");
    prepare_mu17g_d3_store(&store.path);
    let report = mu17g_d3_tickets_lanes_cli_report(&[], &store.path, "h-b1-text");
    let labels =
        report
            .iter()
            .filter_map(|(label, _)| label.starts_with("tickets.text.").then_some(label.as_str()))
            .chain(report.iter().filter_map(|(label, _)| {
                label.starts_with("lanes.text.").then_some(label.as_str())
            }))
            .collect::<Vec<_>>();
    assert_eq!(
        labels,
        [
            "tickets.text.create",
            "tickets.text.get",
            "tickets.text.update",
            "tickets.text.list",
            "tickets.text.update.stale-root.stderr",
            "tickets.text.delete",
            "tickets.text.list.after-delete",
            "lanes.text.create",
            "lanes.text.get",
            "lanes.text.update",
            "lanes.text.list",
            "lanes.text.delete",
            "lanes.text.get.absent.stderr",
        ]
    );
    let output = |label: &str| {
        String::from_utf8_lossy(
            &report
                .iter()
                .find(|(candidate, _)| candidate == label)
                .unwrap_or_else(|| panic!("missing {label}"))
                .1,
        )
        .into_owned()
    };
    assert!(output("tickets.text.get").contains("Default text ticket"));
    assert!(output("tickets.text.update.stale-root.stderr").contains("CONFLICT:"));
    assert!(!output("tickets.text.list.after-delete").contains("CORE-3"));
    assert!(output("lanes.text.get").contains("Default text lane"));
    assert!(output("lanes.text.get.absent.stderr").contains("lane not found"));
}

#[test]
fn mu17g_d3_tickets_lanes_cli_parity_direct_local_and_daemon_local() {
    let template = DaemonStore::new("mu17g-d3-template");
    prepare_mu17g_d3_store(&template.path);

    let direct = DaemonStore::new("mu17g-d3-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report = mu17g_d3_tickets_lanes_cli_report(&[], &direct.path, "d3-direct");

    let mut daemon_store = DaemonStore::new("mu17g-d3-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_d3_tickets_lanes_cli_report(&[], &daemon_store.path, "d3-daemon");
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    assert_mu17g_d3_reports_eq(
        &direct_report,
        &daemon_report,
        "MU-17g-d3 Tickets and Lanes CLI direct-local and daemon-local",
    );
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
#[test]
fn mu17g_d3_tickets_lanes_cli_parity_direct_daemon_and_remote() {
    let template = DaemonStore::new("mu17g-d3-all-template");
    prepare_mu17g_d3_store(&template.path);

    let direct = DaemonStore::new("mu17g-d3-all-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report = mu17g_d3_tickets_lanes_cli_report(&[], &direct.path, "d3-all-direct");

    let mut daemon_store = DaemonStore::new("mu17g-d3-all-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_d3_tickets_lanes_cli_report(&[], &daemon_store.path, "d3-all-daemon");
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    let remote_store = DaemonStore::new("mu17g-d3-remote");
    copy_store_bytes(&template.path, &remote_store.path);
    let remote = RemoteServeStore::start_existing("mu17g-d3-remote", remote_store.path.clone());
    let remote_report = mu17g_d3_tickets_lanes_cli_report(&remote.globals, "context", "d3-remote");

    assert_mu17g_d3_reports_eq(
        &direct_report,
        &daemon_report,
        "MU-17g-d3 Tickets and Lanes CLI direct-local and daemon-local",
    );
    assert_mu17g_d3_reports_eq(
        &direct_report,
        &remote_report,
        "MU-17g-d3 Tickets and Lanes CLI direct-local and remote",
    );

    let denied_template = DaemonStore::new("mu17g-d3-denied-template");
    let (user_id, user_pass) =
        prepare_mu17g_d3_authenticated_non_admin_store(&denied_template.path, "mu17g-d3-denied");
    let denied_store_path = denied_template.path.clone();
    let denied_store = RemoteServeStore::start_existing("mu17g-d3-denied", denied_store_path);
    let mut denied_globals = denied_store.globals.clone();
    denied_globals.push("--auth-principal".to_string());
    denied_globals.push(user_id);
    denied_globals.push("--auth-key-source".to_string());
    denied_globals.push(format!("file:{user_pass}"));
    let denied_report = mu17g_d3_hosted_auth_denial_report(&denied_globals, "context");
    assert!(
        denied_report.iter().any(|(label, bytes)| label
            == "hosted.tickets.projects.unauthenticated-denied.stderr"
            && String::from_utf8_lossy(bytes).contains("PERMISSION_DENIED")),
        "hosted unauthenticated denial was not captured: {denied_report:?}"
    );
    let _ = std::fs::remove_file(user_pass);
}

#[test]
fn mu17g_e1_sql_meetings_cli_parity_direct_local_and_daemon_local() {
    let template = DaemonStore::new("mu17g-e1-template");
    prepare_mu17g_e1_store(&template.path);

    let direct = DaemonStore::new("mu17g-e1-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report = mu17g_e1_sql_meetings_cli_report(&[], &direct.path, "e1-direct", true);

    let mut daemon_store = DaemonStore::new("mu17g-e1-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report =
        mu17g_e1_sql_meetings_cli_report(&[], &daemon_store.path, "e1-daemon", true);
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    assert_mu17g_d3_reports_eq(
        &direct_report,
        &daemon_report,
        "MU-17g-e1 SQL and Meetings CLI direct-local and daemon-local",
    );
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
#[test]
fn mu17g_e1_sql_meetings_cli_parity_direct_daemon_and_remote() {
    let template = DaemonStore::new("mu17g-e1-all-template");
    prepare_mu17g_e1_store(&template.path);

    let direct = DaemonStore::new("mu17g-e1-all-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report = mu17g_e1_sql_meetings_cli_report(&[], &direct.path, "e1-all-direct", false);

    let mut daemon_store = DaemonStore::new("mu17g-e1-all-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report =
        mu17g_e1_sql_meetings_cli_report(&[], &daemon_store.path, "e1-all-daemon", false);
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    let remote_store = DaemonStore::new("mu17g-e1-remote");
    copy_store_bytes(&template.path, &remote_store.path);
    let remote = RemoteServeStore::start_existing("mu17g-e1-remote", remote_store.path.clone());
    let remote_report =
        mu17g_e1_sql_meetings_cli_report(&remote.globals, "context", "e1-remote", false);

    assert_mu17g_d3_reports_eq(
        &direct_report,
        &daemon_report,
        "MU-17g-e1 SQL and Meetings CLI direct-local and daemon-local",
    );
    assert_mu17g_d3_reports_eq(
        &direct_report,
        &remote_report,
        "MU-17g-e1 SQL and Meetings CLI direct-local and remote",
    );
}

#[test]
fn mu17h_b3_f2_default_text_report_covers_lifecycle_and_refs() {
    let fixtures = Mu17gF2Fixtures::new("mu17h-b3-content");
    let store = DaemonStore::new("mu17h-b3-content");
    prepare_mu17g_f2_store(&store.path);
    let report = mu17g_f2_cli_report(&[], &store.path, &fixtures, false);
    let entry = |label: &str| {
        String::from_utf8(
            report
                .iter()
                .find(|(candidate, _)| candidate == label)
                .unwrap_or_else(|| panic!("missing report entry {label}"))
                .1
                .clone(),
        )
        .unwrap()
    };

    let define = entry("lifecycle.define-standard.text");
    assert!(define.starts_with("ok\n"), "{define}");
    assert!(define.contains(r#""definition_id": "feature""#), "{define}");

    let invalid = entry("lifecycle.define.invalid.text");
    assert!(invalid.starts_with("error\n"), "{invalid}");
    assert!(invalid.contains("CORRUPT_OBJECT:"), "{invalid}");

    let instantiate = entry("lifecycle.instantiate.text");
    assert!(instantiate.starts_with("ok\n"), "{instantiate}");
    assert!(
        instantiate.contains(r#""instance_id": "instance-text""#),
        "{instantiate}"
    );

    let transition = entry("lifecycle.transition.text");
    assert!(transition.starts_with("ok\n"), "{transition}");
    assert!(
        transition.contains(r#""transition_id": "transition-text""#),
        "{transition}"
    );

    assert_eq!(
        entry("refs.reconcile.text"),
        "ok\npending\tresolved\tfailed\tprocessed\n0\t0\t0\t0\n"
    );
    let missing = entry("refs.reconcile.missing.text");
    assert!(missing.starts_with("error\n"), "{missing}");
    assert!(missing.contains("NOT_FOUND:"), "{missing}");
}

#[test]
fn mu17g_f2_lifecycle_refs_exec_interchange_cli_parity_direct_local_and_daemon_local() {
    let fixtures = Mu17gF2Fixtures::new("mu17g-f2-local-daemon");
    let template = DaemonStore::new("mu17g-f2-template");
    prepare_mu17g_f2_store(&template.path);

    let direct = DaemonStore::new("mu17g-f2-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report = mu17g_f2_cli_report(&[], &direct.path, &fixtures, false);

    let mut daemon_store = DaemonStore::new("mu17g-f2-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_f2_cli_report(&[], &daemon_store.path, &fixtures, false);
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    assert_mu17g_d3_reports_eq(
        &direct_report,
        &daemon_report,
        "MU-17g-f2 Lifecycle, Refs, Exec, and Interchange direct-local and daemon-local",
    );
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
#[test]
fn mu17g_f2_lifecycle_refs_exec_interchange_cli_parity_direct_daemon_and_remote() {
    let fixtures = Mu17gF2Fixtures::new("mu17g-f2-all");
    let template = DaemonStore::new("mu17g-f2-all-template");
    prepare_mu17g_f2_store(&template.path);

    let direct = DaemonStore::new("mu17g-f2-all-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report = mu17g_f2_cli_report(&[], &direct.path, &fixtures, false);

    let mut daemon_store = DaemonStore::new("mu17g-f2-all-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_f2_cli_report(&[], &daemon_store.path, &fixtures, false);
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    let remote_store = DaemonStore::new("mu17g-f2-remote");
    copy_store_bytes(&template.path, &remote_store.path);
    let remote = RemoteServeStore::start_existing("mu17g-f2-remote", remote_store.path.clone());
    let remote_report = mu17g_f2_cli_report(&remote.globals, "context", &fixtures, false);

    assert_mu17g_d3_reports_eq(
        &direct_report,
        &daemon_report,
        "MU-17g-f2 Lifecycle, Refs, Exec, and Interchange direct-local and daemon-local",
    );
    assert_mu17g_d3_reports_eq(
        &direct_report,
        &remote_report,
        "MU-17g-f2 Lifecycle, Refs, Exec, and Interchange direct-local and hosted-remote",
    );
}

#[test]
fn mu17g_gb_meetings_source_read_report_covers_bytes_and_stable_errors() {
    let store = DaemonStore::new("mu17g-gb-content");
    prepare_mu17g_e1_store(&store.path);
    let report = mu17g_e1_sql_meetings_cli_report(&[], &store.path, "gb-content", true);
    let value = |label: &str| {
        report
            .iter()
            .find(|(candidate, _)| candidate == label)
            .unwrap_or_else(|| panic!("missing {label}"))
            .1
            .as_slice()
    };
    assert_eq!(value("meetings.source-read"), b"Planning summary\n");
    assert!(
        String::from_utf8_lossy(value("meetings.source-read.invalid-leaf.stderr"))
            .contains("INVALID_ARGUMENT:")
    );
    assert!(
        String::from_utf8_lossy(value("meetings.source-read.absent.stderr")).contains("NOT_FOUND:")
    );
}

#[test]
fn mu17h_b2_e2_default_text_and_drive_reads_cover_required_presentation() {
    let template = DaemonStore::new("mu17h-b2-content");
    let drive_root = prepare_mu17g_e2_store(&template.path);
    let report =
        mu17g_e2_chat_drive_mutation_report(&[], &template.path, &drive_root, "h-b2-content");
    let value = |label: &str| {
        report
            .iter()
            .find(|(candidate, _)| candidate == label)
            .unwrap_or_else(|| panic!("missing {label}"))
            .1
            .as_slice()
    };

    assert!(value("chat.text.messages.empty").is_empty());
    assert!(
        value("chat.text.messages.after-edit")
            .windows(b"edited\0chat".len())
            .any(|window| { window == b"edited\0chat" })
    );
    assert!(
        String::from_utf8_lossy(value("chat.text.edit.stale"))
            .contains("CONFLICT: expected_tag_mismatch")
    );
    assert!(String::from_utf8_lossy(value("drive.text.list")).contains("data.bin"));
    assert!(!value("drive.text.conflicts").is_empty());
    assert!(value("drive.text.shares.empty").is_empty());
    assert!(value("drive.text.retention.empty").is_empty());
    assert!(String::from_utf8_lossy(value("drive.text.stat.absent")).contains("NOT_FOUND:"));
    assert_eq!(value("drive.read.bytes"), b"drive\0bytes");
    for label in [
        "drive.list",
        "drive.stat",
        "drive.list-versions",
        "drive.list-conflicts",
        "drive.list-shares",
        "drive.list-retention",
    ] {
        serde_json::from_slice::<serde_json::Value>(value(label))
            .unwrap_or_else(|error| panic!("{label} is not JSON: {error}"));
    }
}

#[test]
fn mu17g_e2_chat_drive_mutation_cli_parity_direct_local_and_daemon_local() {
    let template = DaemonStore::new("mu17g-e2-template");
    let drive_root = prepare_mu17g_e2_store(&template.path);

    let direct = DaemonStore::new("mu17g-e2-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report =
        mu17g_e2_chat_drive_mutation_report(&[], &direct.path, &drive_root, "e2-direct");

    let mut daemon_store = DaemonStore::new("mu17g-e2-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report =
        mu17g_e2_chat_drive_mutation_report(&[], &daemon_store.path, &drive_root, "e2-daemon");
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    assert_mu17g_d3_reports_eq(
        &direct_report,
        &daemon_report,
        "MU-17g-e2 Chat and Drive mutation CLI direct-local and daemon-local",
    );
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
#[test]
fn mu17g_e2_chat_drive_mutation_cli_parity_direct_daemon_and_remote() {
    let template = DaemonStore::new("mu17g-e2-all-template");
    let drive_root = prepare_mu17g_e2_store(&template.path);

    let direct = DaemonStore::new("mu17g-e2-all-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report =
        mu17g_e2_chat_drive_mutation_report(&[], &direct.path, &drive_root, "e2-all-direct");

    let mut daemon_store = DaemonStore::new("mu17g-e2-all-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report =
        mu17g_e2_chat_drive_mutation_report(&[], &daemon_store.path, &drive_root, "e2-all-daemon");
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    let remote_store = DaemonStore::new("mu17g-e2-remote");
    copy_store_bytes(&template.path, &remote_store.path);
    let remote = RemoteServeStore::start_existing("mu17g-e2-remote", remote_store.path.clone());
    let remote_report =
        mu17g_e2_chat_drive_mutation_report(&remote.globals, "context", &drive_root, "e2-remote");

    assert_mu17g_d3_reports_eq(
        &direct_report,
        &daemon_report,
        "MU-17g-e2 Chat and Drive mutation CLI direct-local and daemon-local",
    );
    assert_mu17g_d3_reports_eq(
        &direct_report,
        &remote_report,
        "MU-17g-e2 Chat and Drive mutation CLI direct-local and remote",
    );
}

#[test]
fn mu17g_gc_chat_read_and_edit_cli_parity_direct_local_and_daemon_local() {
    let template = DaemonStore::new("mu17g-gc-template");
    let stale_entity_tag = prepare_mu17g_gc_store(&template.path);

    let direct = DaemonStore::new("mu17g-gc-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report =
        mu17g_gc_chat_read_and_edit_report(&[], &direct.path, &stale_entity_tag, "gc-direct");

    let mut daemon_store = DaemonStore::new("mu17g-gc-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report =
        mu17g_gc_chat_read_and_edit_report(&[], &daemon_store.path, &stale_entity_tag, "gc-daemon");
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    assert_mu17g_d3_reports_eq(
        &direct_report,
        &daemon_report,
        "MU-17g-g-c Chat read and edit direct-local and daemon-local",
    );
}

#[cfg(all(feature = "serve", feature = "remote-client"))]
#[test]
fn mu17g_gc_chat_read_and_edit_cli_parity_direct_daemon_and_remote() {
    let template = DaemonStore::new("mu17g-gc-all-template");
    let stale_entity_tag = prepare_mu17g_gc_store(&template.path);

    let direct = DaemonStore::new("mu17g-gc-all-direct");
    copy_store_bytes(&template.path, &direct.path);
    let direct_report =
        mu17g_gc_chat_read_and_edit_report(&[], &direct.path, &stale_entity_tag, "gc-all-direct");

    let mut daemon_store = DaemonStore::new("mu17g-gc-all-daemon");
    copy_store_bytes(&template.path, &daemon_store.path);
    daemon_store.start();
    wait_for_daemon_status(&daemon_store, "running\t");
    let daemon_report = mu17g_gc_chat_read_and_edit_report(
        &[],
        &daemon_store.path,
        &stale_entity_tag,
        "gc-all-daemon",
    );
    daemon_store.stop();
    daemon_store.assert_runtime_artifacts_removed();

    let remote_store = DaemonStore::new("mu17g-gc-remote");
    copy_store_bytes(&template.path, &remote_store.path);
    let remote = RemoteServeStore::start_existing("mu17g-gc-remote", remote_store.path.clone());
    let remote_report = mu17g_gc_chat_read_and_edit_report(
        &remote.globals,
        "context",
        &stale_entity_tag,
        "gc-remote",
    );

    assert_mu17g_d3_reports_eq(
        &direct_report,
        &daemon_report,
        "MU-17g-g-c Chat read and edit direct-local and daemon-local",
    );
    assert_mu17g_d3_reports_eq(
        &direct_report,
        &remote_report,
        "MU-17g-g-c Chat read and edit direct-local and hosted-remote",
    );
}

#[test]
fn mu15d_daemon_active_cli_routing_immediate_after_start_uses_daemon() {
    let mut store = DaemonStore::new("mu15d-immediate");
    loom(["store", "init", &store.path]).unwrap();
    store.start();
    wait_for_daemon_status(&store, "running\t");

    document_put_text(&store.path, "immediate", "immediate daemon write");
    let text = document_get_text(&store.path, "immediate");
    assert_eq!(text, "immediate daemon write");

    store.stop();
    store.assert_runtime_artifacts_removed();
}

#[test]
fn mu15d_daemon_active_cli_routing_ownership_failure_fails_closed() {
    let store = DaemonStore::new("mu15d-fail-closed");
    FileStore::create_with_profile(&store.path, Algo::Blake3).unwrap();
    let paths = daemon::paths(&store.path).unwrap();
    let mut lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&paths.lock_file)
        .unwrap();
    use std::io::Write;
    write!(
        lock,
        "store={}\nidentity={}\npid=mu15d-test\nphase=running\nstartup_mode=persistent\nstartup_initiator=cli.daemon.start\n",
        paths.store, paths.store_id
    )
    .unwrap();
    lock.lock().unwrap();

    let (ok, stdout, stderr) = loom_output([
        "document",
        "get-text",
        &store.path,
        "main",
        "mu15d",
        "missing",
    ]);
    assert!(!ok, "read unexpectedly succeeded:\n{stdout}");
    assert!(
        stderr.contains("daemon owns store"),
        "read did not fail closed on daemon ownership:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("generated CLI negotiation failed")
            || stderr.contains("incompatible runtime")
            || stderr.contains("still starting"),
        "read did not report actionable daemon negotiation failure:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    lock.unlock().unwrap();
    drop(lock);
    let _ = std::fs::remove_file(paths.lock_file);
}

#[test]
fn mu15d_daemon_active_cli_routing_direct_local_restored_after_shutdown() {
    let mut store = DaemonStore::new("mu15d-after-shutdown");
    loom(["store", "init", &store.path]).unwrap();
    store.start();
    wait_for_daemon_status(&store, "running\t");
    document_put_text(&store.path, "during-daemon", "daemon before shutdown");
    store.stop();
    store.assert_runtime_artifacts_removed();

    document_put_text(&store.path, "after-shutdown", "direct after shutdown");
    let text = document_get_text(&store.path, "after-shutdown");
    assert_eq!(text, "direct after shutdown");
}

#[test]
fn mu15d_s_slow_progress_start_reports_starting_and_reaches_running() {
    let mut store = DaemonStore::new("mu15d-s-slow-progress");
    loom(["store", "init", &store.path]).unwrap();
    let (ok, stdout, stderr) = loom_output_env(
        ["daemon", "start", "--transport", "tcp", &store.path],
        &[
            ("LOOM_MU15D_S_STARTUP_TEST", "slow-progress"),
            ("ULDREN_LOOM_DAEMON_START_WAIT_MS", "200"),
            ("ULDREN_LOOM_DAEMON_START_NO_PROGRESS_MS", "300"),
        ],
    );
    assert!(
        ok,
        "slow-progress start failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.starts_with("starting\t") || stdout.starts_with("started\t"),
        "start did not report starting or started:\n{stdout}"
    );
    let status = wait_for_daemon_status(&store, "running\t");
    assert!(status.contains("startup_mode=persistent"), "{status}");
    store.stop();
    store.assert_runtime_artifacts_removed();
}

#[test]
fn mu15d_s_stalled_startup_reports_stage_and_cleans_up() {
    let store = DaemonStore::new("mu15d-s-stalled");
    loom(["store", "init", &store.path]).unwrap();
    let (ok, stdout, stderr) = loom_output_env(
        ["daemon", "start", "--transport", "tcp", &store.path],
        &[
            ("LOOM_MU15D_S_STARTUP_TEST", "stalled"),
            ("ULDREN_LOOM_DAEMON_START_WAIT_MS", "5000"),
            ("ULDREN_LOOM_DAEMON_START_NO_PROGRESS_MS", "1000"),
        ],
    );
    assert!(!ok, "stalled startup unexpectedly succeeded:\n{stdout}");
    assert!(stderr.contains("daemon startup stalled"), "{stderr}");
    assert!(
        stderr.contains("stage=store.index") || stderr.contains("stage=spawned"),
        "{stderr}"
    );
    let status = loom(["daemon", "status", &store.path]).unwrap();
    assert!(status.starts_with("stopped\t"), "{status}");
}

#[test]
fn mu15d_s_child_exit_reports_last_stage_and_cleans_up() {
    let store = DaemonStore::new("mu15d-s-exit");
    loom(["store", "init", &store.path]).unwrap();
    let (ok, stdout, stderr) = loom_output_env(
        ["daemon", "start", "--transport", "tcp", &store.path],
        &[
            ("LOOM_MU15D_S_STARTUP_TEST", "exit"),
            ("ULDREN_LOOM_DAEMON_START_WAIT_MS", "5000"),
            ("ULDREN_LOOM_DAEMON_START_NO_PROGRESS_MS", "1000"),
        ],
    );
    assert!(!ok, "exit startup unexpectedly succeeded:\n{stdout}");
    assert!(stderr.contains("daemon exited during startup"), "{stderr}");
    assert!(stderr.contains("stage=store.backing"), "{stderr}");
    let status = loom(["daemon", "status", &store.path]).unwrap();
    assert!(status.starts_with("stopped\t"), "{status}");
}

#[test]
fn mu15d_s_eventual_readiness_starts_and_stops_normally() {
    let mut store = DaemonStore::new("mu15d-s-eventual");
    loom(["store", "init", &store.path]).unwrap();
    let (ok, stdout, stderr) = loom_output_env(
        ["daemon", "start", "--transport", "tcp", &store.path],
        &[
            ("ULDREN_LOOM_DAEMON_START_WAIT_MS", "10000"),
            ("ULDREN_LOOM_DAEMON_START_NO_PROGRESS_MS", "2000"),
        ],
    );
    assert!(
        ok,
        "eventual start failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.starts_with("started\t") || stdout.starts_with("running\t"),
        "eventual start did not reach readiness:\n{stdout}"
    );
    let status = wait_for_daemon_status(&store, "running\t");
    assert!(
        status.contains("startup_initiator=cli.daemon.start"),
        "{status}"
    );
    store.stop();
    store.assert_runtime_artifacts_removed();
}

fn root_auth(root: WorkspaceId, session_id: &str) -> LocalOpenAuth {
    LocalOpenAuth {
        unlock_key: None,
        principal: None,
        passphrase: None,
        app_credential: None,
        verified_external: None,
        preauthenticated_principal: Some(root),
        session_id: Some(session_id.to_string()),
    }
}

fn configure_background_work(path: &str) -> (WorkspaceId, WorkspaceId) {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).unwrap();
    let workspace = WorkspaceId::v4_from_bytes([41; 16]);
    let root = WorkspaceId::v4_from_bytes([42; 16]);
    let recipient = WorkspaceId::v4_from_bytes([43; 16]);
    let mut loom = Loom::new(fs);
    loom.registry_mut()
        .create(FacetKind::Files, Some("repo"), workspace)
        .unwrap();
    loom.registry_mut()
        .add_facet(workspace, FacetKind::Vcs)
        .unwrap();
    let mut identity = IdentityStore::new(root);
    identity
        .set_passphrase(root, "root-pass", b"12345678")
        .unwrap();
    let mut acl = AclStore::new();
    acl.allow(
        AclSubject::Principal(root),
        None,
        None,
        [AclRight::Admin, AclRight::Read, AclRight::Write],
    )
    .unwrap();
    loom.store().save_identity_store(&identity).unwrap();
    loom.store().save_acl_store(&acl).unwrap();
    loom_tickets::create_project(&mut loom, workspace, "repo", "core", "CORE", "Core", None)
        .unwrap();
    let fields = serde_json::json!({});
    loom_tickets::create_ticket(
        &mut loom,
        workspace,
        loom_tickets::TicketCreateRequest {
            workspace_id: "repo",
            project_id: "core",
            ticket_type: "task",
            external_source: None,
            external_id: None,
            fields: &fields,
            policy_labels: &[],
            expected_root: None,
        },
    )
    .unwrap();
    let candidate = loom_substrate::refs::UnresolvedReference::new(
        loom_substrate::refs::UnresolvedReferenceInput {
            candidate_id: "candidate-1".to_string(),
            source: loom_substrate::refs::ReferenceSource::new(
                "tickets",
                "repo",
                "ticket-operation-1",
                "description",
            )
            .unwrap(),
            source_operation_id: "ticket.updated:ticket-operation-1".to_string(),
            source_root: Digest::hash(Algo::Blake3, b"source"),
            alias_text: "CORE-1".to_string(),
            relation: "refers_to".to_string(),
            span_start: 0,
            span_end: 6,
            evidence: "CORE-1".to_string(),
            next_attempt_ms: 1,
        },
    )
    .unwrap();
    loom_reference::enqueue(&mut loom, workspace, &candidate).unwrap();
    loom_store::save_loom(&mut loom).unwrap();
    drop(loom);

    let mut loom = loom_store::open_loom_daemon_authorized_unlocked(path, None).unwrap();
    loom = loom_store::attach_local_auth(loom, &root_auth(root, "root-setup")).unwrap();
    let root_folder = loom_hosted::drive::list_folder(&loom, workspace, "main", "root").unwrap();
    loom_hosted::drive::grant_share(
        &mut loom,
        workspace,
        loom_hosted::drive::HostedDriveGrantShare {
            workspace_id: "main",
            grant_id: "grant-expiring",
            target_kind: "folder",
            target_id: "root",
            principal: &recipient.to_string(),
            role: "viewer",
            granted_at_ms: 100,
            expires_at_ms: Some(200),
        },
    )
    .unwrap();
    loom_hosted::drive::pin_retention(
        &mut loom,
        workspace,
        loom_hosted::drive::HostedDrivePinRetention {
            workspace_id: "main",
            pin_id: "trash-expiring",
            kind: "trash_subtree",
            root: &root_folder.profile_root,
            target_entity_id: Some("folder:trash"),
            added_at_ms: 100,
            expires_at_ms: Some(200),
        },
    )
    .unwrap();
    let mut registry = DrivePolicyRegistry::empty();
    registry
        .upsert_enabled(DrivePolicyTarget::new(workspace, "main", true).unwrap())
        .unwrap();
    loom.store()
        .control_set_audited(
            &drive_policy_registry_key(),
            registry.encode().unwrap(),
            Some(root),
            "drive.policy_registry.configure",
            Some(&format!("workspace={workspace};profile=main;enabled=true")),
        )
        .unwrap();
    loom_store::save_loom(&mut loom).unwrap();
    (workspace, root)
}

fn background_work_completed(path: &str, workspace: WorkspaceId, root: WorkspaceId) -> bool {
    let mut loom = match loom_store::open_loom_daemon_authorized_unlocked(path, None) {
        Ok(loom) => loom,
        Err(_) => return false,
    };
    loom = match loom_store::attach_local_auth(loom, &root_auth(root, "root-verify")) {
        Ok(loom) => loom,
        Err(_) => return false,
    };
    let drive_done = loom_hosted::drive::list_shares(&loom, workspace, "main")
        .map(|shares| shares.is_empty())
        .unwrap_or(false)
        && loom_hosted::drive::list_retention(&loom, workspace, "main")
            .map(|retention| retention.is_empty())
            .unwrap_or(false);
    let references_done = loom_reference::status(&loom, workspace)
        .map(|status| status.pending == 0 && status.resolved == 1)
        .unwrap_or(false);
    drive_done && references_done
}

fn background_work_diagnostics(path: &str, workspace: WorkspaceId, root: WorkspaceId) -> String {
    let mut loom = match loom_store::open_loom_daemon_authorized_unlocked(path, None) {
        Ok(loom) => loom,
        Err(error) => return format!("open={error}"),
    };
    loom = match loom_store::attach_local_auth(loom, &root_auth(root, "root-diagnostics")) {
        Ok(loom) => loom,
        Err(error) => return format!("auth={error}"),
    };
    let share_count = loom_hosted::drive::list_shares(&loom, workspace, "main")
        .map(|shares| shares.len().to_string())
        .unwrap_or_else(|error| format!("error:{error}"));
    let retention_count = loom_hosted::drive::list_retention(&loom, workspace, "main")
        .map(|retention| retention.len().to_string())
        .unwrap_or_else(|error| format!("error:{error}"));
    let reference_status = loom_reference::status(&loom, workspace)
        .map(|status| {
            format!(
                "pending={};resolved={};failed={}",
                status.pending, status.resolved, status.failed
            )
        })
        .unwrap_or_else(|error| format!("error:{error}"));
    let audit = FileStore::open_read(path)
        .and_then(|store| store.audit_records())
        .map(|records| {
            records
                .into_iter()
                .map(|record| {
                    format!(
                        "{}:{}",
                        record.action,
                        record.target.unwrap_or_else(|| "".to_string())
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_else(|error| format!("audit-error:{error}"));
    format!(
        "shares={share_count};retention={retention_count};references={reference_status};audit={audit}"
    )
}

fn wait_for_daemon_background_ticks(path: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        loom(["daemon", "status", path]).unwrap();
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn wait_for_writer_release(path: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match loom_store::open_loom_daemon_authorized_unlocked(path, None) {
            Ok(_) => return,
            Err(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
}

#[test]
fn cli_hot_families_execute_against_active_daemon() {
    let mut store = DaemonStore::new("daemon-active");
    loom(["store", "init", &store.path]).unwrap();
    store.start();

    loom([
        "tickets",
        "project-create",
        &store.path,
        "main",
        "core",
        "CORE",
        "Core",
        "--format",
        "json",
    ])
    .unwrap();
    let ticket_create = loom([
        "tickets",
        "create",
        &store.path,
        "main",
        "task",
        "--project-id",
        "core",
        "--title",
        "Daemon ticket",
        "--fields",
        "{}",
    ])
    .unwrap();
    assert!(ticket_create.contains("CORE-1"), "{ticket_create}");
    assert!(ticket_create.contains("\tnative\t"), "{ticket_create}");

    let stat = loom(["store", "stat", &store.path]).unwrap();
    assert!(stat.contains("\"object_count\":"), "{stat}");

    loom([
        "lanes",
        "create",
        &store.path,
        "main",
        "agent-daemon",
        "agent-daemon",
        "--kind",
        "assignment",
        "--ticket",
        "CORE-1",
        "--status-report",
        "active daemon proof",
        "--updated-by",
        "agent:test",
        "--format",
        "json",
    ])
    .unwrap();
    let lane_get = loom([
        "lanes",
        "get",
        &store.path,
        "main",
        "agent-daemon",
        "--detailed",
        "--format",
        "json",
    ])
    .unwrap();
    assert!(lane_get.contains("agent-daemon"), "{lane_get}");
    assert!(lane_get.contains("CORE-1"), "{lane_get}");
    let lane_list = loom([
        "lanes",
        "list",
        &store.path,
        "main",
        "--detailed",
        "--format",
        "json",
    ])
    .unwrap();
    assert!(lane_list.contains("agent-daemon"), "{lane_list}");
    assert!(lane_list.contains("CORE-1"), "{lane_list}");

    loom([
        "pages",
        "space-create",
        &store.path,
        "main",
        "docs",
        "Docs",
        "--format",
        "json",
    ])
    .unwrap();
    loom([
        "pages",
        "create",
        &store.path,
        "main",
        "intro",
        "docs",
        "Intro",
        "--format",
        "json",
    ])
    .unwrap();
    loom([
        "pages",
        "update",
        &store.path,
        "main",
        "intro",
        "daemon page body",
        "--format",
        "json",
    ])
    .unwrap();
    let page = loom([
        "pages",
        "get",
        &store.path,
        "main",
        "intro",
        "--format",
        "json",
    ])
    .unwrap();
    assert!(page.contains("Intro"), "{page}");

    let mut doc = std::env::temp_dir();
    doc.push(format!(
        "loom-daemon-cli-authority-doc-{}-{}.txt",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&doc, "daemon document body").unwrap();
    let doc_path = doc.to_string_lossy().into_owned();
    loom([
        "document",
        "put-text",
        &store.path,
        "main",
        "notes",
        "d1",
        &doc_path,
    ])
    .unwrap();
    let doc_text = loom(["document", "get-text", &store.path, "main", "notes", "d1"]).unwrap();
    assert_eq!(doc_text, "daemon document body");
    let _ = std::fs::remove_file(doc);

    let tickets = loom(["tickets", "list", &store.path, "main", "--format", "json"]).unwrap();
    assert!(tickets.contains("CORE-1"), "{tickets}");
    assert!(tickets.contains("Daemon ticket"), "{tickets}");

    store.stop();
    let audit = loom(["audit", "list", &store.path]).unwrap();
    assert!(audit.contains("daemon.start"), "{audit}");
}

#[test]
fn daemon_background_mutations_execute_against_owned_engine() {
    let mut store = DaemonStore::new("background-authority");
    let (workspace, root) = configure_background_work(&store.path);
    store.start();
    wait_for_daemon_background_ticks(&store.path);
    store.stop_auth(root);
    wait_for_writer_release(&store.path);

    assert!(
        background_work_completed(&store.path, workspace, root),
        "daemon background work did not persist: {}",
        background_work_diagnostics(&store.path, workspace, root)
    );
    let audit = store.audit_auth(root);
    assert!(audit.contains("daemon.service_principal.ensure"), "{audit}");
    assert!(audit.contains("daemon.service_principal.acl"), "{audit}");
    assert!(audit.contains("drive.share_acl.expire"), "{audit}");
    assert!(
        audit.contains("reference_resolver.principal.ensure"),
        "{audit}"
    );
    assert!(audit.contains("reference_resolver.acl.ensure"), "{audit}");
    assert!(!audit.contains("drive.policy_worker.error"), "{audit}");
    assert!(!audit.contains("reference_resolver.error"), "{audit}");
}
