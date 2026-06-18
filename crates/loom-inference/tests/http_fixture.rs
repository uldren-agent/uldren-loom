use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use loom_inference::{
    DownloadArtifact, DownloadEvent, DownloadExecutor, DownloadJobManager, DownloadJobPlan,
};
use loom_types::RevisionRef;
use loom_types::{
    Code, DownloadState, InferenceModelKind, LoomError, ModelRef, Result, RuntimeKind,
};

struct HttpFixtureExecutor {
    addr: SocketAddr,
    root: PathBuf,
    calls: u32,
}

impl DownloadExecutor for HttpFixtureExecutor {
    fn download(&mut self, _model: &ModelRef, file: &str) -> Result<DownloadArtifact> {
        self.calls += 1;
        let mut stream = TcpStream::connect(self.addr)?;
        let request = format!(
            "GET /{file} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            self.addr
        );
        stream.write_all(request.as_bytes())?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response)?;
        let separator = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or_else(|| LoomError::corrupt("http fixture response missing header end"))?;
        let headers = String::from_utf8_lossy(&response[..separator]);
        if !headers.starts_with("HTTP/1.1 200") {
            return Err(LoomError::new(
                Code::Io,
                headers.lines().next().unwrap_or("http error"),
            ));
        }
        let body = &response[separator + 4..];
        let path = self.root.join(file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, body)?;
        Ok(DownloadArtifact {
            file: file.to_string(),
            path,
        })
    }
}

fn spawn_http_fixture(
    responses: Vec<(u16, &'static [u8])>,
) -> (SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        for (status, body) in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let reason = if status == 200 {
                "OK"
            } else {
                "Internal Server Error"
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        }
    });
    (addr, handle)
}

fn model() -> ModelRef {
    ModelRef::new(InferenceModelKind::TextEmbedding, "BAAI/bge-small-en-v1.5")
        .with_revision(RevisionRef::Commit("abc123".to_string()))
}

fn temp_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "loom-inference-jobs-{name}-{}-{nanos}",
        std::process::id()
    ))
}

fn manager(root: &Path) -> DownloadJobManager {
    DownloadJobManager::new(root.join("hub"))
}

fn plan() -> DownloadJobPlan {
    DownloadJobPlan::new(
        model(),
        RuntimeKind::CandleSafetensors,
        vec!["snapshots/abc123/model.safetensors".to_string()],
    )
    .unwrap()
}

#[test]
fn run_retries_transient_http_fixture_failure() {
    let root = temp_root("http-retry");
    let manager = manager(&root).with_max_retries(1);
    manager.enqueue_with_id("job-a", plan()).unwrap();
    let (addr, server) = spawn_http_fixture(vec![(500, b"no"), (200, b"http-model")]);
    let mut executor = HttpFixtureExecutor {
        addr,
        root: root.join("hub"),
        calls: 0,
    };
    let mut events = Vec::new();

    let job = manager
        .run("job-a", &mut executor, |event| events.push(event))
        .unwrap();
    server.join().unwrap();

    assert_eq!(job.state, DownloadState::Installed);
    assert_eq!(executor.calls, 2);
    assert!(!root.join("loom").join("installed-models.json").exists());
    assert!(events.iter().any(|event| matches!(
        event,
        DownloadEvent::Retry {
            attempt: 1,
            message,
            ..
        } if message.contains("500")
    )));
}
