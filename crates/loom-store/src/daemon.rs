//! Native local coordinator daemon client.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use loom_core::error::{LoomError, Result};
use loom_core::{Code, Fence, LockMode, LockOwner, LockToken, Object};

pub const PROTOCOL: u32 = 1;
pub const TRANSPORT: &str = "tcp";
pub const UNIX_SOCKET_TRANSPORT: &str = "unix_socket";
pub const WINDOWS_NAMED_PIPE_TRANSPORT: &str = "windows_named_pipe";
pub const DEFAULT_LOCK_WAIT_MS: u64 = 30_000;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const RESPONSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const LOCK_WAIT_POLL: std::time::Duration = std::time::Duration::from_millis(25);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaemonTransport {
    TcpLoopback,
    UnixSocket,
    WindowsNamedPipe,
}

impl DaemonTransport {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::TcpLoopback => TRANSPORT,
            Self::UnixSocket => UNIX_SOCKET_TRANSPORT,
            Self::WindowsNamedPipe => WINDOWS_NAMED_PIPE_TRANSPORT,
        }
    }

    pub fn security(self) -> DaemonTransportSecurity {
        match self {
            Self::TcpLoopback => DaemonTransportSecurity::DegradedLoopback,
            Self::UnixSocket => DaemonTransportSecurity::PeerCredentials,
            Self::WindowsNamedPipe => DaemonTransportSecurity::OwnerOnlyNamedPipe,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaemonTransportSecurity {
    DegradedLoopback,
    OwnerRuntimeDirectory,
    PeerCredentials,
    OwnerOnlyNamedPipe,
}

impl DaemonTransportSecurity {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::DegradedLoopback => "degraded_loopback",
            Self::OwnerRuntimeDirectory => "owner_runtime_directory",
            Self::PeerCredentials => "peer_credentials",
            Self::OwnerOnlyNamedPipe => "owner_only_named_pipe",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaemonTransportCapabilityStatus {
    Supported,
    Degraded,
    Target,
    Unsupported,
}

impl DaemonTransportCapabilityStatus {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Degraded => "degraded",
            Self::Target => "target",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonTransportCapability {
    pub transport: DaemonTransport,
    pub status: DaemonTransportCapabilityStatus,
    pub security: DaemonTransportSecurity,
    pub reason: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonEndpointEnvelope {
    pub protocol: u32,
    pub transport: DaemonTransport,
    pub security: DaemonTransportSecurity,
    pub identity: String,
    pub endpoint: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DaemonEndpoint {
    TcpLoopback(std::net::SocketAddr),
    #[cfg(unix)]
    UnixSocket(PathBuf),
    #[cfg(windows)]
    WindowsNamedPipe(String),
}

impl DaemonEndpoint {
    pub fn transport(&self) -> DaemonTransport {
        match self {
            Self::TcpLoopback(_) => DaemonTransport::TcpLoopback,
            #[cfg(unix)]
            Self::UnixSocket(_) => DaemonTransport::UnixSocket,
            #[cfg(windows)]
            Self::WindowsNamedPipe(_) => DaemonTransport::WindowsNamedPipe,
        }
    }

    pub fn security(&self) -> DaemonTransportSecurity {
        match self {
            Self::TcpLoopback(_) => DaemonTransportSecurity::DegradedLoopback,
            #[cfg(unix)]
            Self::UnixSocket(_) => DaemonTransportSecurity::PeerCredentials,
            #[cfg(windows)]
            Self::WindowsNamedPipe(_) => DaemonTransportSecurity::OwnerOnlyNamedPipe,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::TcpLoopback(addr) => addr.to_string(),
            #[cfg(unix)]
            Self::UnixSocket(path) => path.display().to_string(),
            #[cfg(windows)]
            Self::WindowsNamedPipe(name) => name.clone(),
        }
    }
}

impl DaemonEndpointEnvelope {
    pub fn tcp_loopback(paths: &DaemonPaths, addr: std::net::SocketAddr) -> Self {
        Self {
            protocol: PROTOCOL,
            transport: DaemonTransport::TcpLoopback,
            security: DaemonTransportSecurity::DegradedLoopback,
            identity: paths.store_id.clone(),
            endpoint: addr.to_string(),
        }
    }

    #[cfg(unix)]
    pub fn unix_socket(paths: &DaemonPaths) -> Self {
        Self {
            protocol: PROTOCOL,
            transport: DaemonTransport::UnixSocket,
            security: DaemonTransportSecurity::PeerCredentials,
            identity: paths.store_id.clone(),
            endpoint: paths.sock_file.display().to_string(),
        }
    }

    #[cfg(windows)]
    pub fn windows_named_pipe(paths: &DaemonPaths) -> Self {
        Self {
            protocol: PROTOCOL,
            transport: DaemonTransport::WindowsNamedPipe,
            security: DaemonTransportSecurity::OwnerOnlyNamedPipe,
            identity: paths.store_id.clone(),
            endpoint: paths.pipe_name.clone(),
        }
    }

    pub fn to_addr_file_contents(&self) -> String {
        format!(
            "protocol={}\ntransport={}\nsecurity={}\nidentity={}\naddr={}\n",
            self.protocol,
            self.transport.wire_name(),
            self.security.wire_name(),
            self.identity,
            self.endpoint
        )
    }
}

pub fn transport_capabilities() -> Vec<DaemonTransportCapability> {
    vec![
        DaemonTransportCapability {
            transport: DaemonTransport::TcpLoopback,
            status: DaemonTransportCapabilityStatus::Degraded,
            security: DaemonTransportSecurity::DegradedLoopback,
            reason: "portable fallback; does not authenticate hostile same-user peers",
        },
        unix_socket_capability(),
        windows_named_pipe_capability(),
    ]
}

#[cfg(unix)]
fn unix_socket_capability() -> DaemonTransportCapability {
    DaemonTransportCapability {
        transport: DaemonTransport::UnixSocket,
        status: DaemonTransportCapabilityStatus::Supported,
        security: DaemonTransportSecurity::PeerCredentials,
        reason: "runtime Unix socket with peer credential owner checks",
    }
}

#[cfg(not(unix))]
fn unix_socket_capability() -> DaemonTransportCapability {
    DaemonTransportCapability {
        transport: DaemonTransport::UnixSocket,
        status: DaemonTransportCapabilityStatus::Unsupported,
        security: DaemonTransportSecurity::OwnerRuntimeDirectory,
        reason: "not a Unix-family platform",
    }
}

#[cfg(windows)]
fn windows_named_pipe_capability() -> DaemonTransportCapability {
    DaemonTransportCapability {
        transport: DaemonTransport::WindowsNamedPipe,
        status: DaemonTransportCapabilityStatus::Supported,
        security: DaemonTransportSecurity::OwnerOnlyNamedPipe,
        reason: "runtime named pipe with owner-only security descriptor",
    }
}

#[cfg(not(windows))]
fn windows_named_pipe_capability() -> DaemonTransportCapability {
    DaemonTransportCapability {
        transport: DaemonTransport::WindowsNamedPipe,
        status: DaemonTransportCapabilityStatus::Unsupported,
        security: DaemonTransportSecurity::OwnerOnlyNamedPipe,
        reason: "not a Windows platform",
    }
}

#[derive(Clone, Debug)]
pub struct DaemonPaths {
    pub store: String,
    pub store_id: String,
    pub addr_file: PathBuf,
    pub pid_file: PathBuf,
    pub lock_file: PathBuf,
    pub sock_file: PathBuf,
    pub pipe_name: String,
}

#[derive(Debug, Clone)]
pub struct DaemonStatus {
    pub transport: DaemonTransport,
    pub pid: String,
    pub store: String,
    pub store_id: String,
    pub sessions: u64,
    pub pins: u64,
    pub permanent_pins: u64,
    pub leased_pins: u64,
    pub pin_details: Vec<DaemonPinStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonPinStatus {
    pub id: String,
    pub kind: DaemonPinKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonPinKind {
    Permanent,
    Leased { deadline_ms: u64 },
}

#[derive(Clone, Debug, Default)]
pub struct DaemonAuth {
    pub principal: Option<String>,
    pub passphrase: Option<String>,
    pub session: Option<String>,
}

pub fn paths(store: impl AsRef<Path>) -> Result<DaemonPaths> {
    let path = store.as_ref();
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| LoomError::invalid(format!("canonicalize {}: {e}", path.display())))?;
    let store = canonical.to_string_lossy().into_owned();
    let store_id = file_identity(&canonical)?;
    let digest = Object::Blob(store_id.as_bytes().to_vec())
        .digest()
        .to_string()
        .replace(':', "_");
    let mut dir = runtime_dir();
    dir.push("uldren-loom-daemon");
    ensure_runtime_dir(&dir)?;
    validate_runtime_dir_owner(&dir, &canonical)?;
    let addr_file = dir.join(format!("{digest}.addr"));
    let pid_file = dir.join(format!("{digest}.pid"));
    let lock_file = dir.join(format!("{digest}.lock"));
    let sock_digest = digest.get(..32).unwrap_or(&digest);
    let sock_file = dir.join(format!("{sock_digest}.sock"));
    let pipe_name = format!("uldren-loom-daemon-{digest}");
    Ok(DaemonPaths {
        store,
        store_id,
        addr_file,
        pid_file,
        lock_file,
        sock_file,
        pipe_name,
    })
}

#[cfg(unix)]
fn file_identity(path: &Path) -> Result<String> {
    use std::os::unix::fs::MetadataExt;

    let meta = std::fs::metadata(path)
        .map_err(|e| LoomError::invalid(format!("metadata {}: {e}", path.display())))?;
    Ok(format!("unix:{}:{}", meta.dev(), meta.ino()))
}

#[cfg(windows)]
fn file_identity(path: &Path) -> Result<String> {
    let handle = winapi_util::Handle::from_path(path).map_err(|e| {
        LoomError::invalid(format!(
            "open {} for Windows daemon identity: {e}",
            path.display()
        ))
    })?;
    let info = winapi_util::file::information(&handle).map_err(|e| {
        LoomError::invalid(format!(
            "read Windows daemon identity for {}: {e}",
            path.display()
        ))
    })?;
    Ok(windows_file_identity(
        info.volume_serial_number(),
        info.file_index(),
    ))
}

#[cfg(any(windows, test))]
fn windows_file_identity(volume_serial_number: u64, file_index: u64) -> String {
    format!("windows:{volume_serial_number}:{file_index}")
}

#[cfg(not(any(unix, windows)))]
fn file_identity(path: &Path) -> Result<String> {
    Ok(format!("path:{}", path.to_string_lossy()))
}

pub fn runtime_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/private/tmp")
    } else {
        std::env::temp_dir()
    }
}

#[cfg(unix)]
fn ensure_runtime_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    match std::fs::symlink_metadata(dir) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(LoomError::invalid(format!(
                "daemon runtime dir {} must not be a symlink",
                dir.display()
            )));
        }
        Ok(meta) if !meta.is_dir() => {
            return Err(LoomError::invalid(format!(
                "daemon runtime dir {} is not a directory",
                dir.display()
            )));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(dir)
                .map_err(|e| LoomError::invalid(format!("create daemon runtime dir: {e}")))?;
        }
        Err(e) => return Err(LoomError::invalid(format!("stat daemon runtime dir: {e}"))),
    }
    let mut perms = std::fs::metadata(dir)
        .map_err(|e| LoomError::invalid(format!("stat daemon runtime dir: {e}")))?
        .permissions();
    perms.set_mode(0o700);
    std::fs::set_permissions(dir, perms)
        .map_err(|e| LoomError::invalid(format!("secure daemon runtime dir: {e}")))?;
    let mode = std::fs::metadata(dir)
        .map_err(|e| LoomError::invalid(format!("stat daemon runtime dir: {e}")))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return Err(LoomError::invalid(format!(
            "daemon runtime dir {} must not be accessible by group or other",
            dir.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_runtime_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)
        .map_err(|e| LoomError::invalid(format!("create daemon runtime dir: {e}")))
}

#[cfg(unix)]
fn validate_runtime_dir_owner(dir: &Path, store: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    let dir_meta = std::fs::metadata(dir)
        .map_err(|e| LoomError::invalid(format!("stat daemon runtime dir: {e}")))?;
    let store_meta = std::fs::metadata(store)
        .map_err(|e| LoomError::invalid(format!("metadata {}: {e}", store.display())))?;
    if dir_meta.uid() != store_meta.uid() {
        return Err(LoomError::invalid(format!(
            "daemon runtime dir {} owner does not match store owner",
            dir.display()
        )));
    }
    Ok(())
}

#[cfg(unix)]
pub fn unix_peer_credentials_supported() -> bool {
    cfg!(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
        target_os = "freebsd",
        target_os = "dragonfly"
    ))
}

#[cfg(unix)]
pub fn validate_unix_peer_owner(
    stream: &std::os::unix::net::UnixStream,
    paths: &DaemonPaths,
) -> Result<()> {
    let peer_uid = unix_peer_uid(stream)?;
    let expected_uid = store_owner_uid(Path::new(&paths.store))?;
    if peer_uid != 0 && peer_uid != expected_uid {
        return Err(LoomError::new(
            Code::PermissionDenied,
            format!(
                "daemon Unix peer uid {peer_uid} does not match store owner uid {expected_uid}"
            ),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn store_owner_uid(store: &Path) -> Result<u64> {
    use std::os::unix::fs::MetadataExt;

    let meta = std::fs::metadata(store)
        .map_err(|e| LoomError::invalid(format!("metadata {}: {e}", store.display())))?;
    Ok(u64::from(meta.uid()))
}

#[cfg(unix)]
pub fn align_runtime_artifact_owner(path: &Path, label: &str, paths: &DaemonPaths) -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(LoomError::new(
                Code::Io,
                format!("stat daemon {label} file: {e}"),
            ));
        }
    };
    if meta.file_type().is_symlink() {
        return Err(LoomError::invalid(format!(
            "daemon {label} file {} must not be a symlink",
            path.display()
        )));
    }
    let uid = store_owner_uid(Path::new(&paths.store))?;
    if u64::from(meta.uid()) == uid {
        return Ok(());
    }
    let uid = u32::try_from(uid).map_err(|_| {
        LoomError::invalid(format!(
            "daemon {label} file {} store owner uid is out of range",
            path.display()
        ))
    })?;
    std::os::unix::fs::chown(path, Some(uid), None).map_err(|e| {
        LoomError::new(
            Code::Io,
            format!(
                "set daemon {label} file {} owner to store owner: {e}",
                path.display()
            ),
        )
    })
}

#[cfg(not(unix))]
pub fn align_runtime_artifact_owner(
    _path: &Path,
    _label: &str,
    _paths: &DaemonPaths,
) -> Result<()> {
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn unix_peer_uid(stream: &std::os::unix::net::UnixStream) -> Result<u64> {
    let credentials =
        nix::sys::socket::getsockopt(stream, nix::sys::socket::sockopt::PeerCredentials)
            .map_err(|e| LoomError::new(Code::Io, format!("read Unix peer credentials: {e}")))?;
    Ok(u64::from(credentials.uid()))
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos",
    target_os = "freebsd",
    target_os = "dragonfly"
))]
fn unix_peer_uid(stream: &std::os::unix::net::UnixStream) -> Result<u64> {
    let credentials =
        nix::sys::socket::getsockopt(stream, nix::sys::socket::sockopt::LocalPeerCred)
            .map_err(|e| LoomError::new(Code::Io, format!("read Unix peer credentials: {e}")))?;
    Ok(u64::from(credentials.uid()))
}

#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
        target_os = "freebsd",
        target_os = "dragonfly"
    ))
))]
fn unix_peer_uid(_stream: &std::os::unix::net::UnixStream) -> Result<u64> {
    Err(LoomError::unsupported(
        "Unix peer credentials are not supported on this platform",
    ))
}

#[cfg(not(unix))]
fn validate_runtime_dir_owner(_dir: &Path, _store: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn validate_runtime_artifact(path: &Path, label: &str) -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(LoomError::new(
                Code::Io,
                format!("stat daemon {label} file: {e}"),
            ));
        }
    };
    if meta.file_type().is_symlink() {
        return Err(LoomError::invalid(format!(
            "daemon {label} file {} must not be a symlink",
            path.display()
        )));
    }
    if !meta.is_file() {
        return Err(LoomError::invalid(format!(
            "daemon {label} file {} is not a regular file",
            path.display()
        )));
    }
    let Some(parent) = path.parent() else {
        return Err(LoomError::invalid(format!(
            "daemon {label} file {} has no parent directory",
            path.display()
        )));
    };
    let parent_meta = std::fs::metadata(parent)
        .map_err(|e| LoomError::invalid(format!("stat daemon runtime dir: {e}")))?;
    if meta.uid() != parent_meta.uid() {
        return Err(LoomError::invalid(format!(
            "daemon {label} file {} owner does not match runtime dir owner",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_runtime_artifact(_path: &Path, _label: &str) -> Result<()> {
    Ok(())
}

pub fn validate_runtime_artifacts(paths: &DaemonPaths) -> Result<()> {
    validate_runtime_artifact(&paths.addr_file, "address")?;
    validate_runtime_artifact(&paths.pid_file, "pid")?;
    validate_runtime_artifact(&paths.lock_file, "lock")?;
    validate_runtime_socket_artifact(&paths.sock_file)
}

#[cfg(unix)]
pub fn prepare_unix_socket_artifact(path: &Path) -> Result<()> {
    use std::os::unix::fs::FileTypeExt;

    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(LoomError::invalid(format!(
            "daemon socket file {} must not be a symlink",
            path.display()
        ))),
        Ok(meta) if meta.file_type().is_socket() => std::fs::remove_file(path)
            .map_err(|e| LoomError::new(Code::Io, format!("remove stale daemon socket: {e}"))),
        Ok(_) => Err(LoomError::invalid(format!(
            "daemon socket file {} is not a socket",
            path.display()
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(LoomError::new(
            Code::Io,
            format!("stat daemon socket file: {e}"),
        )),
    }
}

#[cfg(unix)]
pub fn validate_runtime_socket_artifact(path: &Path) -> Result<()> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(LoomError::new(
                Code::Io,
                format!("stat daemon socket file: {e}"),
            ));
        }
    };
    if meta.file_type().is_symlink() {
        return Err(LoomError::invalid(format!(
            "daemon socket file {} must not be a symlink",
            path.display()
        )));
    }
    if !meta.file_type().is_socket() {
        return Err(LoomError::invalid(format!(
            "daemon socket file {} is not a socket",
            path.display()
        )));
    }
    let Some(parent) = path.parent() else {
        return Err(LoomError::invalid(format!(
            "daemon socket file {} has no parent directory",
            path.display()
        )));
    };
    let parent_meta = std::fs::metadata(parent)
        .map_err(|e| LoomError::invalid(format!("stat daemon runtime dir: {e}")))?;
    if meta.uid() != parent_meta.uid() {
        return Err(LoomError::invalid(format!(
            "daemon socket file {} owner does not match runtime dir owner",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn validate_runtime_socket_artifact(_path: &Path) -> Result<()> {
    Ok(())
}

pub fn addr_file_contents(paths: &DaemonPaths, addr: std::net::SocketAddr) -> String {
    DaemonEndpointEnvelope::tcp_loopback(paths, addr).to_addr_file_contents()
}

pub fn request(addr_file: &Path, request: &str) -> Result<String> {
    let endpoint = daemon_transport_endpoint_from_file(addr_file, None)?;
    request_endpoint(endpoint, request)
}

fn request_endpoint(endpoint: DaemonEndpoint, request: &str) -> Result<String> {
    let mut stream = connect_endpoint(&endpoint)?;
    stream.configure()?;
    stream
        .write_all(request.as_bytes())
        .map_err(|e| LoomError::new(Code::Io, format!("write daemon request: {e}")))?;
    stream
        .shutdown_write()
        .map_err(|e| LoomError::new(Code::Io, format!("finish daemon request: {e}")))?;
    read_daemon_response(&mut stream)
}

fn read_daemon_response(stream: &mut impl Read) -> Result<String> {
    let deadline = std::time::Instant::now() + RESPONSE_TIMEOUT;
    let mut response = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => {
                return String::from_utf8(response)
                    .map_err(|e| LoomError::corrupt(format!("daemon response is not UTF-8: {e}")));
            }
            Ok(n) => response.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Err(LoomError::new(
                        Code::Io,
                        format!(
                            "read daemon response: timed out after {:?}",
                            RESPONSE_TIMEOUT
                        ),
                    ));
                }
                std::thread::sleep(LOCK_WAIT_POLL);
            }
            Err(e) => {
                return Err(LoomError::new(
                    Code::Io,
                    format!("read daemon response: {e}"),
                ));
            }
        }
    }
}

enum DaemonConnection {
    Tcp(std::net::TcpStream),
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixStream),
    #[cfg(windows)]
    WindowsNamedPipe(interprocess::local_socket::Stream),
}

impl DaemonConnection {
    fn configure(&self) -> Result<()> {
        match self {
            Self::Tcp(stream) => configure_tcp_daemon_stream(stream),
            #[cfg(unix)]
            Self::Unix(stream) => configure_unix_daemon_stream(stream),
            #[cfg(windows)]
            Self::WindowsNamedPipe(stream) => configure_windows_daemon_stream(stream),
        }
    }

    fn shutdown_write(&self) -> std::io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.shutdown(std::net::Shutdown::Write),
            #[cfg(unix)]
            Self::Unix(stream) => stream.shutdown(std::net::Shutdown::Write),
            #[cfg(windows)]
            Self::WindowsNamedPipe(_) => Ok(()),
        }
    }
}

impl Read for DaemonConnection {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(buf),
            #[cfg(unix)]
            Self::Unix(stream) => stream.read(buf),
            #[cfg(windows)]
            Self::WindowsNamedPipe(stream) => stream.read(buf),
        }
    }
}

impl Write for DaemonConnection {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(buf),
            #[cfg(unix)]
            Self::Unix(stream) => stream.write(buf),
            #[cfg(windows)]
            Self::WindowsNamedPipe(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            #[cfg(unix)]
            Self::Unix(stream) => stream.flush(),
            #[cfg(windows)]
            Self::WindowsNamedPipe(stream) => stream.flush(),
        }
    }
}

fn connect_endpoint(endpoint: &DaemonEndpoint) -> Result<DaemonConnection> {
    match endpoint {
        DaemonEndpoint::TcpLoopback(addr) => Ok(DaemonConnection::Tcp(
            std::net::TcpStream::connect(addr).map_err(|e| daemon_connect_error(endpoint, e))?,
        )),
        #[cfg(unix)]
        DaemonEndpoint::UnixSocket(path) => Ok(DaemonConnection::Unix(
            std::os::unix::net::UnixStream::connect(path)
                .map_err(|e| daemon_connect_error(endpoint, e))?,
        )),
        #[cfg(windows)]
        DaemonEndpoint::WindowsNamedPipe(name) => {
            use interprocess::local_socket::{GenericWorkspaced, prelude::*};

            let name = name
                .as_str()
                .to_ns_name::<GenericWorkspaced>()
                .map_err(|e| {
                    LoomError::invalid(format!("invalid daemon named pipe endpoint: {e}"))
                })?;
            Ok(DaemonConnection::WindowsNamedPipe(
                interprocess::local_socket::Stream::connect(name)
                    .map_err(|e| daemon_connect_error(endpoint, e))?,
            ))
        }
    }
}

fn configure_tcp_daemon_stream(stream: &std::net::TcpStream) -> Result<()> {
    stream
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|e| LoomError::new(Code::Io, format!("set daemon read timeout: {e}")))?;
    stream
        .set_write_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|e| LoomError::new(Code::Io, format!("set daemon write timeout: {e}")))
}

#[cfg(all(test, feature = "integration-tests"))]
fn configure_daemon_stream(stream: &std::net::TcpStream) -> Result<()> {
    configure_tcp_daemon_stream(stream)
}

#[cfg(unix)]
fn configure_unix_daemon_stream(stream: &std::os::unix::net::UnixStream) -> Result<()> {
    stream
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|e| LoomError::new(Code::Io, format!("set daemon read timeout: {e}")))?;
    stream
        .set_write_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|e| LoomError::new(Code::Io, format!("set daemon write timeout: {e}")))
}

#[cfg(windows)]
fn configure_windows_daemon_stream(stream: &interprocess::local_socket::Stream) -> Result<()> {
    use interprocess::local_socket::prelude::*;

    stream
        .set_recv_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|e| LoomError::new(Code::Io, format!("set daemon read timeout: {e}")))?;
    stream
        .set_send_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|e| LoomError::new(Code::Io, format!("set daemon write timeout: {e}")))
}

fn read_daemon_addr_file(addr_file: &Path) -> Result<String> {
    match std::fs::symlink_metadata(addr_file) {
        Ok(meta) if meta.file_type().is_symlink() => Err(LoomError::invalid(format!(
            "daemon address file {} must not be a symlink",
            addr_file.display()
        ))),
        Ok(meta) if !meta.is_file() => Err(LoomError::invalid(format!(
            "daemon address file {} is not a regular file",
            addr_file.display()
        ))),
        Ok(_) => {
            validate_runtime_artifact(addr_file, "address")?;
            std::fs::read_to_string(addr_file)
                .map_err(|e| LoomError::new(Code::Io, format!("read daemon address file: {e}")))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(daemon_not_running(format!(
            "daemon is not running: address file {} is missing",
            addr_file.display()
        ))),
        Err(e) => Err(LoomError::new(
            Code::Io,
            format!("stat daemon address file: {e}"),
        )),
    }
}

fn daemon_addr_from_file(
    addr_file: &Path,
    expected_store_id: Option<&str>,
) -> Result<std::net::SocketAddr> {
    match daemon_transport_endpoint_from_file(addr_file, expected_store_id)? {
        DaemonEndpoint::TcpLoopback(addr) => Ok(addr),
        #[cfg(unix)]
        DaemonEndpoint::UnixSocket(_) => Err(LoomError::invalid(
            "daemon endpoint is not a TCP loopback address",
        )),
        #[cfg(windows)]
        DaemonEndpoint::WindowsNamedPipe(_) => Err(LoomError::invalid(
            "daemon endpoint is not a TCP loopback address",
        )),
    }
}

pub fn daemon_endpoint(paths: &DaemonPaths) -> Result<std::net::SocketAddr> {
    daemon_addr_from_file(&paths.addr_file, Some(&paths.store_id))
}

pub fn daemon_transport_endpoint(paths: &DaemonPaths) -> Result<DaemonEndpoint> {
    daemon_transport_endpoint_from_file(&paths.addr_file, Some(&paths.store_id))
}

fn daemon_transport_endpoint_from_file(
    addr_file: &Path,
    expected_store_id: Option<&str>,
) -> Result<DaemonEndpoint> {
    let contents = read_daemon_addr_file(addr_file)?;
    parse_daemon_endpoint_file(&contents, expected_store_id)
}

#[cfg(test)]
fn parse_daemon_addr_file(
    contents: &str,
    expected_store_id: Option<&str>,
) -> Result<std::net::SocketAddr> {
    match parse_daemon_endpoint_file(contents, expected_store_id)? {
        DaemonEndpoint::TcpLoopback(addr) => Ok(addr),
        #[cfg(unix)]
        DaemonEndpoint::UnixSocket(_) => Err(LoomError::invalid(
            "daemon endpoint is not a TCP loopback address",
        )),
        #[cfg(windows)]
        DaemonEndpoint::WindowsNamedPipe(_) => Err(LoomError::invalid(
            "daemon endpoint is not a TCP loopback address",
        )),
    }
}

fn parse_daemon_endpoint_file(
    contents: &str,
    expected_store_id: Option<&str>,
) -> Result<DaemonEndpoint> {
    let trimmed = contents.trim();
    if !trimmed.contains('\n') && !trimmed.starts_with("protocol=") {
        return Ok(DaemonEndpoint::TcpLoopback(parse_daemon_addr(trimmed)?));
    }
    let mut protocol = None;
    let mut transport = None;
    let mut security = None;
    let mut identity = None;
    let mut addr = None;
    for line in contents.lines() {
        let Some((key, value)) = line.split_once('=') else {
            return Err(LoomError::invalid("invalid daemon address envelope line"));
        };
        match key {
            "protocol" => protocol = Some(value),
            "transport" => transport = Some(value),
            "security" => security = Some(value),
            "identity" => identity = Some(value),
            "addr" => addr = Some(value),
            _ => {}
        }
    }
    if protocol != Some("1") {
        return Err(LoomError::invalid("unsupported daemon address protocol"));
    }
    let identity = identity.ok_or_else(|| LoomError::invalid("missing daemon address identity"))?;
    if let Some(expected) = expected_store_id
        && identity != expected
    {
        return Err(LoomError::invalid(format!(
            "daemon address identity is {identity}, not expected identity {expected}"
        )));
    }
    let addr = addr.ok_or_else(|| LoomError::invalid("missing daemon address"))?;
    match transport.ok_or_else(|| LoomError::invalid("missing daemon address transport"))? {
        TRANSPORT => {
            if security != Some(DaemonTransportSecurity::DegradedLoopback.wire_name()) {
                return Err(LoomError::invalid(
                    "daemon TCP endpoint must use degraded_loopback security",
                ));
            }
            Ok(DaemonEndpoint::TcpLoopback(parse_daemon_addr(addr)?))
        }
        UNIX_SOCKET_TRANSPORT => parse_unix_daemon_endpoint(addr, security),
        WINDOWS_NAMED_PIPE_TRANSPORT => parse_windows_daemon_endpoint(addr, security),
        _ => Err(LoomError::invalid("unsupported daemon address transport")),
    }
}

#[cfg(unix)]
fn parse_unix_daemon_endpoint(addr: &str, security: Option<&str>) -> Result<DaemonEndpoint> {
    if security != Some(DaemonTransportSecurity::PeerCredentials.wire_name()) {
        return Err(LoomError::invalid(
            "daemon Unix socket endpoint must use peer_credentials security",
        ));
    }
    let path = PathBuf::from(addr);
    if !path.is_absolute() {
        return Err(LoomError::invalid(
            "daemon Unix socket endpoint must be an absolute path",
        ));
    }
    Ok(DaemonEndpoint::UnixSocket(path))
}

#[cfg(not(unix))]
fn parse_unix_daemon_endpoint(_addr: &str, _security: Option<&str>) -> Result<DaemonEndpoint> {
    Err(LoomError::invalid(
        "daemon Unix socket endpoint is unsupported on this platform",
    ))
}

#[cfg(windows)]
fn parse_windows_daemon_endpoint(addr: &str, security: Option<&str>) -> Result<DaemonEndpoint> {
    if security != Some(DaemonTransportSecurity::OwnerOnlyNamedPipe.wire_name()) {
        return Err(LoomError::invalid(
            "daemon Windows named-pipe endpoint must use owner_only_named_pipe security",
        ));
    }
    validate_windows_pipe_name(addr)?;
    Ok(DaemonEndpoint::WindowsNamedPipe(addr.to_string()))
}

#[cfg(not(windows))]
fn parse_windows_daemon_endpoint(_addr: &str, _security: Option<&str>) -> Result<DaemonEndpoint> {
    Err(LoomError::invalid(
        "daemon Windows named-pipe endpoint is unsupported on this platform",
    ))
}

pub fn validate_windows_pipe_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= 220
        && name.starts_with("uldren-loom-daemon-")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'));
    if valid {
        Ok(())
    } else {
        Err(LoomError::invalid("invalid daemon named pipe name"))
    }
}

fn parse_daemon_addr(addr: &str) -> Result<std::net::SocketAddr> {
    let parsed = addr
        .parse::<std::net::SocketAddr>()
        .map_err(|e| LoomError::invalid(format!("invalid daemon address {addr:?}: {e}")))?;
    if !parsed.ip().is_loopback() {
        return Err(LoomError::invalid(format!(
            "daemon address {parsed} is not loopback"
        )));
    }
    Ok(parsed)
}

fn daemon_connect_error(endpoint: &DaemonEndpoint, err: std::io::Error) -> LoomError {
    let addr = endpoint.label();
    match err.kind() {
        std::io::ErrorKind::ConnectionRefused
        | std::io::ErrorKind::NotFound
        | std::io::ErrorKind::TimedOut => {
            daemon_not_running(format!("daemon is not running at {addr}: {err}"))
        }
        _ => LoomError::new(Code::Io, format!("connect daemon at {addr}: {err}")),
    }
}

fn daemon_not_running(message: impl Into<String>) -> LoomError {
    LoomError::not_found(message)
}

pub fn request_checked(paths: &DaemonPaths, request_text: &str) -> Result<String> {
    let response = request_for_paths(paths, request_text)?;
    if let Some(err) = response.strip_prefix("error\t") {
        Err(parse_daemon_error(err))
    } else {
        Ok(response)
    }
}

pub fn request_checked_addr(addr_file: &Path, request_text: &str) -> Result<String> {
    let response = request(addr_file, request_text)?;
    if let Some(err) = response.strip_prefix("error\t") {
        Err(parse_daemon_error(err))
    } else {
        Ok(response)
    }
}

fn parse_daemon_error(err: &str) -> LoomError {
    let err = err.trim_end();
    if let Some((code, message)) = err.split_once(": ")
        && let Some(code) = code_from_wire(code)
    {
        return LoomError::new(code, message.to_string());
    }
    LoomError::invalid(err)
}

fn code_from_wire(code: &str) -> Option<Code> {
    Some(match code {
        "NOT_FOUND" => Code::NotFound,
        "ALREADY_EXISTS" => Code::AlreadyExists,
        "CORRUPT_OBJECT" => Code::CorruptObject,
        "INTEGRITY_FAILURE" => Code::IntegrityFailure,
        "UNSUPPORTED" => Code::Unsupported,
        "INVALID_ARGUMENT" => Code::InvalidArgument,
        "IO" => Code::Io,
        "INTERNAL" => Code::Internal,
        "CROSS_WORKSPACE" => Code::CrossWorkspace,
        "CAS_MISMATCH" => Code::CasMismatch,
        "NOT_FAST_FORWARD" => Code::NotFastForward,
        "DIMENSION_MISMATCH" => Code::DimensionMismatch,
        "PERMISSION_DENIED" => Code::PermissionDenied,
        "AUTHENTICATION_FAILED" => Code::AuthenticationFailed,
        "IDENTITY_NO_ROOT_CREDENTIAL" => Code::IdentityNoRootCredential,
        "TRIGGER_NOT_FOUND" => Code::TriggerNotFound,
        "TRIGGER_DENIED" => Code::TriggerDenied,
        "CURSOR_INVALID" => Code::CursorInvalid,
        "E2E_LOCKED" => Code::E2eLocked,
        "E2E_KEY_INVALID" => Code::E2eKeyInvalid,
        "CONFLICT" => Code::Conflict,
        "LOCKED" => Code::Locked,
        "LOCK_LEASE_EXPIRED" => Code::LockLeaseExpired,
        "FENCING_STALE" => Code::FencingStale,
        "LOCK_NOT_HELD" => Code::LockNotHeld,
        "NO_SUCH_FIELD" => Code::NoSuchField,
        "QUERY_PARSE_ERROR" => Code::QueryParseError,
        "SQL_SYNTAX" => Code::SqlSyntax,
        "SQL_CONSTRAINT_VIOLATION" => Code::SqlConstraintViolation,
        "SQL_TABLE_NOT_FOUND" => Code::SqlTableNotFound,
        "SQL_TYPE_MISMATCH" => Code::SqlTypeMismatch,
        "SQL_EXECUTION_FAILED" => Code::SqlExecutionFailed,
        "RESOURCE_EXHAUSTED" => Code::ResourceExhausted,
        "INDEX_NOT_READY" => Code::IndexNotReady,
        "DOCUMENT_NOT_TEXT" => Code::DocumentNotText,
        "UNAVAILABLE" => Code::Unavailable,
        "RETAINED_GAP" => Code::RetainedGap,
        _ => return None,
    })
}

fn request_for_paths(paths: &DaemonPaths, request_text: &str) -> Result<String> {
    request_endpoint(daemon_transport_endpoint(paths)?, request_text)
}

pub fn field(value: &str) -> Result<&str> {
    if value.is_empty() || value.contains('\t') || value.contains('\n') || value.contains('\r') {
        Err(LoomError::invalid(
            "daemon fields must be non-empty and cannot contain tab or newline",
        ))
    } else {
        Ok(value)
    }
}

pub fn field_bytes(value: &str) -> Result<&str> {
    if value.contains('\t') || value.contains('\n') || value.contains('\r') {
        Err(LoomError::invalid(
            "daemon fields cannot contain tab or newline",
        ))
    } else {
        Ok(value)
    }
}

pub fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub fn hex_decode(value: &str) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return Err(LoomError::invalid("hex field has odd length"));
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    for idx in (0..value.len()).step_by(2) {
        out.push(
            u8::from_str_radix(&value[idx..idx + 2], 16)
                .map_err(|_| LoomError::invalid("hex field is not valid lowercase hex"))?,
        );
    }
    Ok(out)
}

fn auth_fields(auth: &DaemonAuth) -> Result<String> {
    let mut out = String::new();
    if let Some(principal) = &auth.principal {
        out.push_str("\tauth-principal=");
        out.push_str(field(principal)?);
    }
    if let Some(passphrase) = &auth.passphrase {
        out.push_str("\tauth-passphrase-hex=");
        out.push_str(&hex_encode(passphrase.as_bytes()));
    }
    if let Some(session) = &auth.session {
        out.push_str("\tauth-session=");
        out.push_str(field(session)?);
    }
    Ok(out)
}

pub fn session_attach(paths: &DaemonPaths, session: &str) -> Result<String> {
    session_attach_auth(paths, session, &DaemonAuth::default())
}

pub fn session_attach_auth(
    paths: &DaemonPaths,
    session: &str,
    auth: &DaemonAuth,
) -> Result<String> {
    request_checked(
        paths,
        &format!(
            "session-attach\t{}{}\n",
            field(session)?,
            auth_fields(auth)?
        ),
    )
}

pub fn session_detach(paths: &DaemonPaths, session: &str) -> Result<String> {
    session_detach_auth(paths, session, &DaemonAuth::default())
}

pub fn session_detach_auth(
    paths: &DaemonPaths,
    session: &str,
    auth: &DaemonAuth,
) -> Result<String> {
    request_checked(
        paths,
        &format!(
            "session-detach\t{}{}\n",
            field(session)?,
            auth_fields(auth)?
        ),
    )
}

pub fn session_check_auth(paths: &DaemonPaths, session: &str, auth: &DaemonAuth) -> Result<String> {
    request_checked(
        paths,
        &format!("session-check\t{}{}\n", field(session)?, auth_fields(auth)?),
    )
}

pub fn pin_add(paths: &DaemonPaths, pin: &str) -> Result<String> {
    pin_add_auth(paths, pin, &DaemonAuth::default())
}

pub fn pin_add_auth(paths: &DaemonPaths, pin: &str, auth: &DaemonAuth) -> Result<String> {
    request_checked(
        paths,
        &format!("pin-add\t{}{}\n", field(pin)?, auth_fields(auth)?),
    )
}

pub fn pin_add_lease(paths: &DaemonPaths, pin: &str, lease_ms: u64, now_ms: u64) -> Result<String> {
    pin_add_lease_auth(paths, pin, lease_ms, now_ms, &DaemonAuth::default())
}

pub fn pin_add_lease_auth(
    paths: &DaemonPaths,
    pin: &str,
    lease_ms: u64,
    now_ms: u64,
    auth: &DaemonAuth,
) -> Result<String> {
    request_checked(
        paths,
        &format!(
            "pin-add\t{}\t{}\t{}{}\n",
            field(pin)?,
            lease_ms,
            now_ms,
            auth_fields(auth)?
        ),
    )
}

pub fn pin_remove(paths: &DaemonPaths, pin: &str) -> Result<String> {
    pin_remove_auth(paths, pin, &DaemonAuth::default())
}

pub fn pin_remove_auth(paths: &DaemonPaths, pin: &str, auth: &DaemonAuth) -> Result<String> {
    request_checked(
        paths,
        &format!("pin-remove\t{}{}\n", field(pin)?, auth_fields(auth)?),
    )
}

pub struct FtsRequest<'a> {
    pub workspace: &'a str,
    pub collection: &'a str,
    pub engine_version: &'a str,
}

pub fn fts_status(paths: &DaemonPaths, req: FtsRequest<'_>) -> Result<String> {
    fts_status_auth(paths, req, &DaemonAuth::default())
}

pub fn fts_status_auth(
    paths: &DaemonPaths,
    req: FtsRequest<'_>,
    auth: &DaemonAuth,
) -> Result<String> {
    request_checked(
        paths,
        &format!(
            "fts-status\t{}\t{}\t{}{}\n",
            field(req.workspace)?,
            field(req.collection)?,
            field(req.engine_version)?,
            auth_fields(auth)?
        ),
    )
}

pub fn fts_rebuild(paths: &DaemonPaths, req: FtsRequest<'_>) -> Result<String> {
    fts_rebuild_auth(paths, req, &DaemonAuth::default())
}

pub fn fts_rebuild_auth(
    paths: &DaemonPaths,
    req: FtsRequest<'_>,
    auth: &DaemonAuth,
) -> Result<String> {
    request_checked(
        paths,
        &format!(
            "fts-rebuild\t{}\t{}\t{}{}\n",
            field(req.workspace)?,
            field(req.collection)?,
            field(req.engine_version)?,
            auth_fields(auth)?
        ),
    )
}

pub fn stop(paths: &DaemonPaths, force: bool) -> Result<String> {
    stop_auth(paths, force, &DaemonAuth::default())
}

pub fn stop_auth(paths: &DaemonPaths, force: bool, auth: &DaemonAuth) -> Result<String> {
    stop_auth_with_options(
        paths,
        StopOptions {
            force,
            ..StopOptions::default()
        },
        auth,
    )
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StopOptions {
    pub force: bool,
    pub hard: bool,
    pub wait_ms: Option<u64>,
}

pub fn stop_with_options(paths: &DaemonPaths, options: StopOptions) -> Result<String> {
    stop_auth_with_options(paths, options, &DaemonAuth::default())
}

pub fn stop_auth_with_options(
    paths: &DaemonPaths,
    options: StopOptions,
    auth: &DaemonAuth,
) -> Result<String> {
    let command = if options.force { "stop-force" } else { "stop" };
    let mut fields = String::new();
    if options.hard {
        fields.push_str("\thard=true");
    }
    if let Some(wait_ms) = options.wait_ms {
        fields.push_str("\twait-ms=");
        fields.push_str(&wait_ms.to_string());
    }
    request_checked(paths, &format!("{command}{fields}{}\n", auth_fields(auth)?))
}

pub fn parse_response(response: &str, expected_store: &str) -> Result<DaemonStatus> {
    parse_response_expected(response, expected_store, expected_store)
}

pub fn parse_response_expected(
    response: &str,
    expected_store: &str,
    expected_store_id: &str,
) -> Result<DaemonStatus> {
    let mut parts = response.trim_end().split('\t');
    let state = parts
        .next()
        .ok_or_else(|| LoomError::invalid("empty daemon response"))?;
    let protocol = parts
        .next()
        .ok_or_else(|| LoomError::invalid("missing daemon protocol"))?;
    let transport = parts
        .next()
        .ok_or_else(|| LoomError::invalid("missing daemon transport"))?;
    let pid = parts
        .next()
        .ok_or_else(|| LoomError::invalid("missing daemon pid"))?;
    let store = parts
        .next()
        .ok_or_else(|| LoomError::invalid("missing daemon store"))?;
    if state != "running" {
        return Err(LoomError::invalid(format!("daemon state is {state:?}")));
    }
    if protocol != "protocol=1" {
        return Err(LoomError::invalid(format!("unsupported daemon {protocol}")));
    }
    let transport = parse_daemon_status_transport(transport)?;
    let mut sessions = 0;
    let mut pins = 0;
    let mut permanent_pins = None;
    let mut leased_pins = None;
    let mut pin_details = Vec::new();
    let mut store_id = None;
    for part in parts {
        if let Some(value) = part.strip_prefix("sessions=") {
            sessions = value
                .parse()
                .map_err(|_| LoomError::invalid(format!("invalid daemon sessions {value:?}")))?;
        } else if let Some(value) = part.strip_prefix("pins=") {
            pins = value
                .parse()
                .map_err(|_| LoomError::invalid(format!("invalid daemon pins {value:?}")))?;
        } else if let Some(value) = part.strip_prefix("permanent_pins=") {
            permanent_pins = Some(value.parse().map_err(|_| {
                LoomError::invalid(format!("invalid daemon permanent pins {value:?}"))
            })?);
        } else if let Some(value) = part.strip_prefix("leased_pins=") {
            leased_pins = Some(value.parse().map_err(|_| {
                LoomError::invalid(format!("invalid daemon leased pins {value:?}"))
            })?);
        } else if let Some(value) = part.strip_prefix("pin=") {
            pin_details.push(parse_pin_status(value)?);
        } else if let Some(value) = part.strip_prefix("identity=") {
            store_id = Some(value.to_string());
        }
    }
    let derived_permanent = pin_details
        .iter()
        .filter(|pin| matches!(&pin.kind, DaemonPinKind::Permanent))
        .count() as u64;
    let derived_leased = pin_details
        .iter()
        .filter(|pin| matches!(&pin.kind, DaemonPinKind::Leased { .. }))
        .count() as u64;
    let permanent_pins = permanent_pins.unwrap_or(derived_permanent);
    let leased_pins = leased_pins.unwrap_or(derived_leased);
    let store_id = store_id.unwrap_or_else(|| store.to_string());
    if store_id != expected_store_id {
        return Err(LoomError::invalid(format!(
            "daemon identity is {store_id}, not expected identity {expected_store_id}"
        )));
    }
    if store_id == store && store != expected_store {
        return Err(LoomError::invalid(format!(
            "daemon is for {store}, not expected store {expected_store}"
        )));
    }
    Ok(DaemonStatus {
        transport,
        pid: pid.to_string(),
        store: store.to_string(),
        store_id,
        sessions,
        pins,
        permanent_pins,
        leased_pins,
        pin_details,
    })
}

fn parse_daemon_status_transport(field: &str) -> Result<DaemonTransport> {
    let Some(transport) = field.strip_prefix("transport=") else {
        return Err(LoomError::invalid(format!("unsupported daemon {field}")));
    };
    match transport {
        TRANSPORT => Ok(DaemonTransport::TcpLoopback),
        UNIX_SOCKET_TRANSPORT => Ok(DaemonTransport::UnixSocket),
        WINDOWS_NAMED_PIPE_TRANSPORT => Ok(DaemonTransport::WindowsNamedPipe),
        _ => Err(LoomError::invalid(format!(
            "unsupported daemon transport={transport}"
        ))),
    }
}

fn parse_pin_status(value: &str) -> Result<DaemonPinStatus> {
    if let Some(hex) = value.strip_prefix("permanent:") {
        return Ok(DaemonPinStatus {
            id: decode_pin_id(hex)?,
            kind: DaemonPinKind::Permanent,
        });
    }
    if let Some(rest) = value.strip_prefix("leased:") {
        let (deadline, hex) = rest
            .split_once(':')
            .ok_or_else(|| LoomError::invalid(format!("invalid daemon pin status {value:?}")))?;
        let deadline_ms = deadline.parse().map_err(|_| {
            LoomError::invalid(format!("invalid daemon pin lease deadline {deadline:?}"))
        })?;
        return Ok(DaemonPinStatus {
            id: decode_pin_id(hex)?,
            kind: DaemonPinKind::Leased { deadline_ms },
        });
    }
    Err(LoomError::invalid(format!(
        "invalid daemon pin status {value:?}"
    )))
}

fn decode_pin_id(hex: &str) -> Result<String> {
    let bytes = hex_decode(hex)?;
    String::from_utf8(bytes).map_err(|_| LoomError::invalid("daemon pin id is not valid utf-8"))
}

pub fn status_response(paths: &DaemonPaths) -> Result<DaemonStatus> {
    validate_runtime_artifacts(paths)?;
    let response = request_for_paths(paths, "status\n")?;
    parse_response_expected(&response, &paths.store, &paths.store_id)
}

pub fn is_running(paths: &DaemonPaths) -> bool {
    status_response(paths).is_ok()
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub fn status_json(paths: &DaemonPaths) -> String {
    match status_response(paths) {
        Ok(status) => format!(
            "{{\"state\":\"RUNNING\",\"protocol\":{},\"transport\":\"{}\",\"security\":\"{}\",\"pid\":\"{}\",\"store\":\"{}\",\"identity\":\"{}\",\"sessions\":{},\"pins\":{},\"permanent_pins\":{},\"leased_pins\":{},\"pin_details\":[{}]}}",
            PROTOCOL,
            json_escape(status.transport.wire_name()),
            json_escape(status.transport.security().wire_name()),
            json_escape(&status.pid),
            json_escape(&status.store),
            json_escape(&status.store_id),
            status.sessions,
            status.pins,
            status.permanent_pins,
            status.leased_pins,
            pin_details_json(&status.pin_details)
        ),
        Err(e) => format!(
            "{{\"state\":\"STOPPED\",\"protocol\":{},\"transport\":\"{}\",\"security\":\"{}\",\"pid\":null,\"store\":\"{}\",\"identity\":\"{}\",\"sessions\":0,\"pins\":0,\"permanent_pins\":0,\"leased_pins\":0,\"pin_details\":[],\"reason\":\"{}\"}}",
            PROTOCOL,
            json_escape(TRANSPORT),
            json_escape(DaemonTransport::TcpLoopback.security().wire_name()),
            json_escape(&paths.store),
            json_escape(&paths.store_id),
            json_escape(&e.to_string())
        ),
    }
}

fn pin_details_json(pins: &[DaemonPinStatus]) -> String {
    pins.iter()
        .map(|pin| match &pin.kind {
            DaemonPinKind::Permanent => format!(
                "{{\"id\":\"{}\",\"kind\":\"PERMANENT\"}}",
                json_escape(&pin.id)
            ),
            DaemonPinKind::Leased { deadline_ms } => format!(
                "{{\"id\":\"{}\",\"kind\":\"LEASED\",\"lease_deadline_ms\":{deadline_ms}}}",
                json_escape(&pin.id)
            ),
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub fn parse_lock_mode(mode: &str, permits: u32, capacity: u32) -> Result<LockMode> {
    match mode {
        "exclusive" => Ok(LockMode::Exclusive),
        "shared" => Ok(LockMode::Shared),
        "semaphore" => Ok(LockMode::Semaphore { permits, capacity }),
        other => Err(LoomError::invalid(format!(
            "unknown lock mode {other:?} (expected `exclusive`, `shared`, or `semaphore`)"
        ))),
    }
}

pub fn lock_mode_wire(mode: LockMode) -> String {
    match mode {
        LockMode::Exclusive => "exclusive".to_string(),
        LockMode::Shared => "shared".to_string(),
        LockMode::Semaphore { permits, capacity } => format!("semaphore:{permits}:{capacity}"),
    }
}

pub fn parse_lock_mode_wire(value: &str) -> Result<LockMode> {
    if let Some(rest) = value.strip_prefix("semaphore:") {
        let (permits, capacity) = rest
            .split_once(':')
            .ok_or_else(|| LoomError::invalid(format!("invalid semaphore mode {value:?}")))?;
        let permits = permits
            .parse()
            .map_err(|_| LoomError::invalid(format!("invalid semaphore permits in {value:?}")))?;
        let capacity = capacity
            .parse()
            .map_err(|_| LoomError::invalid(format!("invalid semaphore capacity in {value:?}")))?;
        return Ok(LockMode::Semaphore { permits, capacity });
    }
    parse_lock_mode(value, 1, 1)
}

pub fn lock_token(
    key: &str,
    principal: &str,
    session: &str,
    mode: LockMode,
    fence: u64,
) -> LockToken {
    LockToken {
        key: key.as_bytes().to_vec(),
        owner: LockOwner {
            principal: principal.to_string(),
            session: session.to_string(),
        },
        mode,
        fence: Fence::embedded(fence),
        lease_deadline_ms: 0,
    }
}

pub fn lock_token_response(token: &LockToken) -> String {
    format!(
        "lock\t{}\t{}\t{}\t{}\t{}\t{}\n",
        String::from_utf8_lossy(&token.key),
        token.owner.principal,
        token.owner.session,
        lock_mode_wire(token.mode),
        token.fence.sequence(),
        token.lease_deadline_ms
    )
}

pub fn lock_response_json(response: &str) -> Result<String> {
    let mut parts = response.trim_end().split('\t');
    let kind = parts
        .next()
        .ok_or_else(|| LoomError::invalid("empty lock response"))?;
    if kind != "lock" {
        return Err(LoomError::invalid(format!(
            "unexpected lock response {kind:?}"
        )));
    }
    let key = parts
        .next()
        .ok_or_else(|| LoomError::invalid("missing lock key"))?;
    let principal = parts
        .next()
        .ok_or_else(|| LoomError::invalid("missing lock principal"))?;
    let session = parts
        .next()
        .ok_or_else(|| LoomError::invalid("missing lock session"))?;
    let mode_wire = parts
        .next()
        .ok_or_else(|| LoomError::invalid("missing lock mode"))?;
    let fence = parts
        .next()
        .ok_or_else(|| LoomError::invalid("missing lock fence"))?;
    let lease_deadline_ms = parts
        .next()
        .ok_or_else(|| LoomError::invalid("missing lock lease deadline"))?;
    let mode = parse_lock_mode_wire(mode_wire)?;
    let (mode_name, permits, capacity) = match mode {
        LockMode::Exclusive => ("EXCLUSIVE", 1, 1),
        LockMode::Shared => ("SHARED", 1, 1),
        LockMode::Semaphore { permits, capacity } => ("SEMAPHORE", permits, capacity),
    };
    Ok(format!(
        "{{\"key\":\"{}\",\"principal\":\"{}\",\"session\":\"{}\",\"mode\":\"{}\",\"permits\":{},\"capacity\":{},\"fence\":{{\"authority\":0,\"epoch\":0,\"sequence\":{}}},\"lease_deadline_ms\":{}}}",
        json_escape(key),
        json_escape(principal),
        json_escape(session),
        mode_name,
        permits,
        capacity,
        fence,
        lease_deadline_ms
    ))
}

pub struct AcquireRequest<'a> {
    pub key: &'a str,
    pub principal: &'a str,
    pub session: &'a str,
    pub mode: LockMode,
    pub lease_ms: u64,
    pub wait_ms: u64,
    pub now_ms: u64,
}

pub fn lock_acquire(paths: &DaemonPaths, req: AcquireRequest<'_>) -> Result<String> {
    lock_acquire_auth(paths, req, &DaemonAuth::default())
}

pub fn lock_acquire_auth(
    paths: &DaemonPaths,
    req: AcquireRequest<'_>,
    auth: &DaemonAuth,
) -> Result<String> {
    let key = field(req.key)?;
    let principal = field(req.principal)?;
    let session = field(req.session)?;
    let deadline = req
        .now_ms
        .checked_add(req.wait_ms)
        .ok_or_else(|| LoomError::invalid("lock wait deadline overflows"))?;
    let mut attempt_now = req.now_ms;
    loop {
        match lock_acquire_once(
            paths,
            LockAcquireWire {
                key,
                principal,
                session,
                mode: req.mode,
                lease_ms: req.lease_ms,
                now_ms: attempt_now,
            },
            auth,
        ) {
            Ok(response) => return Ok(response),
            Err(e) if e.code == Code::Locked && attempt_now < deadline => {
                let now = daemon_now_ms();
                if now >= deadline {
                    return Err(e);
                }
                std::thread::sleep(LOCK_WAIT_POLL.min(std::time::Duration::from_millis(
                    deadline.saturating_sub(now),
                )));
                attempt_now = daemon_now_ms();
            }
            Err(e) => return Err(e),
        }
    }
}

struct LockAcquireWire<'a> {
    key: &'a str,
    principal: &'a str,
    session: &'a str,
    mode: LockMode,
    lease_ms: u64,
    now_ms: u64,
}

fn lock_acquire_once(
    paths: &DaemonPaths,
    req: LockAcquireWire<'_>,
    auth: &DaemonAuth,
) -> Result<String> {
    request_checked(
        paths,
        &format!(
            "lock-acquire\t{}\t{}\t{}\t{}\t{}\t{}{}\n",
            req.key,
            req.principal,
            req.session,
            lock_mode_wire(req.mode),
            req.lease_ms,
            req.now_ms,
            auth_fields(auth)?
        ),
    )
}

fn daemon_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

pub struct RefreshRequest<'a> {
    pub key: &'a str,
    pub principal: &'a str,
    pub session: &'a str,
    pub mode: LockMode,
    pub fence: Fence,
    pub lease_ms: u64,
    pub now_ms: u64,
}

pub fn lock_refresh(paths: &DaemonPaths, req: RefreshRequest<'_>) -> Result<String> {
    lock_refresh_auth(paths, req, &DaemonAuth::default())
}

pub fn lock_refresh_auth(
    paths: &DaemonPaths,
    req: RefreshRequest<'_>,
    auth: &DaemonAuth,
) -> Result<String> {
    request_checked(
        paths,
        &format!(
            "lock-refresh\t{}\t{}\t{}\t{}\t{}\t{}\t{}{}\n",
            field(req.key)?,
            field(req.principal)?,
            field(req.session)?,
            lock_mode_wire(req.mode),
            req.fence.sequence(),
            req.lease_ms,
            req.now_ms,
            auth_fields(auth)?
        ),
    )
}

pub struct ReleaseRequest<'a> {
    pub key: &'a str,
    pub principal: &'a str,
    pub session: &'a str,
    pub mode: LockMode,
    pub fence: Fence,
    pub now_ms: u64,
}

pub fn lock_release(paths: &DaemonPaths, req: ReleaseRequest<'_>) -> Result<String> {
    lock_release_auth(paths, req, &DaemonAuth::default())
}

pub fn lock_release_auth(
    paths: &DaemonPaths,
    req: ReleaseRequest<'_>,
    auth: &DaemonAuth,
) -> Result<String> {
    request_checked(
        paths,
        &format!(
            "lock-release\t{}\t{}\t{}\t{}\t{}\t{}{}\n",
            field(req.key)?,
            field(req.principal)?,
            field(req.session)?,
            lock_mode_wire(req.mode),
            req.fence.sequence(),
            req.now_ms,
            auth_fields(auth)?
        ),
    )
}

pub struct BreakRequest<'a> {
    pub key: &'a str,
    pub now_ms: u64,
}

pub fn lock_break(paths: &DaemonPaths, req: BreakRequest<'_>) -> Result<String> {
    lock_break_auth(paths, req, &DaemonAuth::default())
}

pub fn lock_break_auth(
    paths: &DaemonPaths,
    req: BreakRequest<'_>,
    auth: &DaemonAuth,
) -> Result<String> {
    request_checked(
        paths,
        &format!(
            "lock-break\t{}\t{}{}\n",
            field(req.key)?,
            req.now_ms,
            auth_fields(auth)?
        ),
    )
}

pub struct ApplyFenceRequest<'a> {
    pub key: &'a str,
    pub principal: &'a str,
    pub session: &'a str,
    pub mode: LockMode,
    pub fence: Fence,
    pub now_ms: u64,
}

pub fn lock_apply_fence(paths: &DaemonPaths, req: ApplyFenceRequest<'_>) -> Result<String> {
    lock_apply_fence_auth(paths, req, &DaemonAuth::default())
}

pub fn lock_apply_fence_auth(
    paths: &DaemonPaths,
    req: ApplyFenceRequest<'_>,
    auth: &DaemonAuth,
) -> Result<String> {
    request_checked(
        paths,
        &format!(
            "lock-apply-fence\t{}\t{}\t{}\t{}\t{}\t{}{}\n",
            field(req.key)?,
            field(req.principal)?,
            field(req.session)?,
            lock_mode_wire(req.mode),
            req.fence.sequence(),
            req.now_ms,
            auth_fields(auth)?
        ),
    )
}

pub fn kv_ephemeral_put(
    paths: &DaemonPaths,
    session: &str,
    workspace: &str,
    name: &str,
    key_cbor: &[u8],
    value: &[u8],
    now_ms: u64,
) -> Result<()> {
    kv_ephemeral_put_auth(
        paths,
        KvPutRequest {
            session,
            workspace,
            name,
            key_cbor,
            value,
            now_ms,
        },
        &DaemonAuth::default(),
    )
}

pub struct KvPutRequest<'a> {
    pub session: &'a str,
    pub workspace: &'a str,
    pub name: &'a str,
    pub key_cbor: &'a [u8],
    pub value: &'a [u8],
    pub now_ms: u64,
}

pub fn kv_ephemeral_put_auth(
    paths: &DaemonPaths,
    req: KvPutRequest<'_>,
    auth: &DaemonAuth,
) -> Result<()> {
    let response = request_checked(
        paths,
        &format!(
            "kv-put\t{}\t{}\t{}\t{}\t{}\t{}{}\n",
            field(req.session)?,
            field(req.workspace)?,
            field(req.name)?,
            hex_encode(req.key_cbor),
            field_bytes(&hex_encode(req.value))?,
            req.now_ms,
            auth_fields(auth)?
        ),
    )?;
    if response.trim_end() == "ok" {
        Ok(())
    } else {
        Err(LoomError::invalid(format!(
            "unexpected daemon kv put response {response:?}"
        )))
    }
}

pub fn kv_ephemeral_get(
    paths: &DaemonPaths,
    session: &str,
    workspace: &str,
    name: &str,
    key_cbor: &[u8],
    now_ms: u64,
) -> Result<Option<Vec<u8>>> {
    kv_ephemeral_get_auth(
        paths,
        session,
        workspace,
        name,
        key_cbor,
        now_ms,
        &DaemonAuth::default(),
    )
}

pub fn kv_ephemeral_get_auth(
    paths: &DaemonPaths,
    session: &str,
    workspace: &str,
    name: &str,
    key_cbor: &[u8],
    now_ms: u64,
    auth: &DaemonAuth,
) -> Result<Option<Vec<u8>>> {
    let response = request_checked(
        paths,
        &format!(
            "kv-get\t{}\t{}\t{}\t{}\t{}{}\n",
            field(session)?,
            field(workspace)?,
            field(name)?,
            hex_encode(key_cbor),
            now_ms,
            auth_fields(auth)?
        ),
    )?;
    let mut parts = response.trim_end().split('\t');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("kv"), Some("0"), None) => Ok(None),
        (Some("kv"), Some("1"), Some(hex)) => Ok(Some(hex_decode(hex)?)),
        _ => Err(LoomError::invalid(format!(
            "unexpected daemon kv get response {response:?}"
        ))),
    }
}

pub fn kv_ephemeral_delete(
    paths: &DaemonPaths,
    session: &str,
    workspace: &str,
    name: &str,
    key_cbor: &[u8],
) -> Result<bool> {
    kv_ephemeral_delete_auth(
        paths,
        session,
        workspace,
        name,
        key_cbor,
        &DaemonAuth::default(),
    )
}

pub fn kv_ephemeral_delete_auth(
    paths: &DaemonPaths,
    session: &str,
    workspace: &str,
    name: &str,
    key_cbor: &[u8],
    auth: &DaemonAuth,
) -> Result<bool> {
    let response = request_checked(
        paths,
        &format!(
            "kv-delete\t{}\t{}\t{}\t{}{}\n",
            field(session)?,
            field(workspace)?,
            field(name)?,
            hex_encode(key_cbor),
            auth_fields(auth)?
        ),
    )?;
    let mut parts = response.trim_end().split('\t');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("deleted"), Some("0"), None) => Ok(false),
        (Some("deleted"), Some("1"), None) => Ok(true),
        _ => Err(LoomError::invalid(format!(
            "unexpected daemon kv delete response {response:?}"
        ))),
    }
}

pub fn kv_ephemeral_list(
    paths: &DaemonPaths,
    session: &str,
    workspace: &str,
    name: &str,
    now_ms: u64,
) -> Result<Vec<u8>> {
    kv_ephemeral_list_auth(
        paths,
        session,
        workspace,
        name,
        now_ms,
        &DaemonAuth::default(),
    )
}

pub fn kv_ephemeral_list_auth(
    paths: &DaemonPaths,
    session: &str,
    workspace: &str,
    name: &str,
    now_ms: u64,
    auth: &DaemonAuth,
) -> Result<Vec<u8>> {
    let response = request_checked(
        paths,
        &format!(
            "kv-list\t{}\t{}\t{}\t{}{}\n",
            field(session)?,
            field(workspace)?,
            field(name)?,
            now_ms,
            auth_fields(auth)?
        ),
    )?;
    kv_map_response(&response, "list")
}

pub fn kv_ephemeral_range(
    paths: &DaemonPaths,
    session: &str,
    workspace: &str,
    name: &str,
    lo_cbor: &[u8],
    hi_cbor: &[u8],
    now_ms: u64,
) -> Result<Vec<u8>> {
    kv_ephemeral_range_auth(
        paths,
        KvRangeRequest {
            session,
            workspace,
            name,
            lo_cbor,
            hi_cbor,
            now_ms,
        },
        &DaemonAuth::default(),
    )
}

pub struct KvRangeRequest<'a> {
    pub session: &'a str,
    pub workspace: &'a str,
    pub name: &'a str,
    pub lo_cbor: &'a [u8],
    pub hi_cbor: &'a [u8],
    pub now_ms: u64,
}

pub fn kv_ephemeral_range_auth(
    paths: &DaemonPaths,
    req: KvRangeRequest<'_>,
    auth: &DaemonAuth,
) -> Result<Vec<u8>> {
    let response = request_checked(
        paths,
        &format!(
            "kv-range\t{}\t{}\t{}\t{}\t{}\t{}{}\n",
            field(req.session)?,
            field(req.workspace)?,
            field(req.name)?,
            hex_encode(req.lo_cbor),
            hex_encode(req.hi_cbor),
            req.now_ms,
            auth_fields(auth)?
        ),
    )?;
    kv_map_response(&response, "range")
}

fn kv_map_response(response: &str, op: &str) -> Result<Vec<u8>> {
    let mut parts = response.trim_end().split('\t');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("kv-map"), Some(hex), None) => hex_decode(hex),
        _ => Err(LoomError::invalid(format!(
            "unexpected daemon kv {op} response {response:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_response_is_bound_to_protocol_and_store() {
        let response =
            "running\tprotocol=1\ttransport=tcp\t123\t/private/tmp/a.loom\tsessions=0\tpins=0\n";
        let status = parse_response(response, "/private/tmp/a.loom").unwrap();
        assert_eq!(status.pid, "123");
        assert_eq!(status.store, "/private/tmp/a.loom");
        assert_eq!(status.store_id, "/private/tmp/a.loom");
        assert_eq!(status.sessions, 0);
        assert_eq!(status.pins, 0);
        assert_eq!(status.permanent_pins, 0);
        assert_eq!(status.leased_pins, 0);
        assert!(status.pin_details.is_empty());
        assert!(parse_response(response, "/private/tmp/b.loom").is_err());
        let response = "running\tprotocol=1\ttransport=tcp\t123\t/private/tmp/a.loom\tidentity=unix:1:2\tsessions=1\tpins=2\tpermanent_pins=1\tleased_pins=1\tpin=permanent:6d616e75616c\tpin=leased:600:6d6f756e74\n";
        let status = parse_response_expected(response, "/private/tmp/b.loom", "unix:1:2").unwrap();
        assert_eq!(status.pid, "123");
        assert_eq!(status.store, "/private/tmp/a.loom");
        assert_eq!(status.store_id, "unix:1:2");
        assert_eq!(status.sessions, 1);
        assert_eq!(status.pins, 2);
        assert_eq!(status.permanent_pins, 1);
        assert_eq!(status.leased_pins, 1);
        assert_eq!(
            status.pin_details,
            vec![
                DaemonPinStatus {
                    id: "manual".to_string(),
                    kind: DaemonPinKind::Permanent,
                },
                DaemonPinStatus {
                    id: "mount".to_string(),
                    kind: DaemonPinKind::Leased { deadline_ms: 600 },
                },
            ]
        );
        assert!(parse_response_expected(response, "/private/tmp/a.loom", "unix:9:9").is_err());
        assert!(
            parse_response(
                "running\tprotocol=2\ttransport=tcp\t123\t/private/tmp/a.loom\n",
                "/private/tmp/a.loom",
            )
            .is_err()
        );
        assert!(
            parse_response(
                "running\tprotocol=1\ttransport=unix\t123\t/private/tmp/a.loom\n",
                "/private/tmp/a.loom",
            )
            .is_err()
        );
    }

    #[test]
    fn daemon_transport_capabilities_record_secure_ipc_targets() {
        let capabilities = transport_capabilities();
        let tcp = capabilities
            .iter()
            .find(|cap| cap.transport == DaemonTransport::TcpLoopback)
            .unwrap();
        assert_eq!(tcp.status, DaemonTransportCapabilityStatus::Degraded);
        assert_eq!(tcp.security, DaemonTransportSecurity::DegradedLoopback);

        let unix_socket = capabilities
            .iter()
            .find(|cap| cap.transport == DaemonTransport::UnixSocket)
            .unwrap();
        if cfg!(unix) {
            assert_eq!(
                unix_socket.status,
                DaemonTransportCapabilityStatus::Supported
            );
            assert_eq!(
                unix_socket.security,
                DaemonTransportSecurity::PeerCredentials
            );
        } else {
            assert_eq!(
                unix_socket.status,
                DaemonTransportCapabilityStatus::Unsupported
            );
        }

        let named_pipe = capabilities
            .iter()
            .find(|cap| cap.transport == DaemonTransport::WindowsNamedPipe)
            .unwrap();
        if cfg!(windows) {
            assert_eq!(
                named_pipe.status,
                DaemonTransportCapabilityStatus::Supported
            );
        } else {
            assert_eq!(
                named_pipe.status,
                DaemonTransportCapabilityStatus::Unsupported
            );
        }
        assert_eq!(
            named_pipe.security,
            DaemonTransportSecurity::OwnerOnlyNamedPipe
        );
    }

    #[test]
    fn daemon_paths_are_scoped_by_canonical_store_path() {
        let root = std::env::temp_dir().join(format!("loom-daemon-paths-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        let a = root.join("a.loom");
        let b = root.join("b.loom");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let pa = paths(&a).unwrap();
        let pb = paths(&b).unwrap();
        assert_ne!(pa.addr_file, pb.addr_file);
        assert_ne!(pa.pid_file, pb.pid_file);
        assert_ne!(pa.lock_file, pb.lock_file);
        assert_ne!(pa.sock_file, pb.sock_file);
        assert_ne!(pa.store_id, pb.store_id);
        assert!(pa.addr_file.ends_with(pa.addr_file.file_name().unwrap()));
        let _ = std::fs::remove_file(a);
        let _ = std::fs::remove_file(b);
        let _ = std::fs::remove_dir(root);
    }

    #[test]
    fn windows_file_identity_uses_volume_and_index() {
        assert_eq!(windows_file_identity(7, 42), "windows:7:42");
    }

    #[cfg(unix)]
    #[test]
    fn daemon_paths_are_scoped_by_file_identity_for_hard_links() {
        let root =
            std::env::temp_dir().join(format!("loom-daemon-hardlink-paths-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        let primary = root.join("primary.loom");
        let alias = root.join("alias.loom");
        std::fs::write(&primary, b"store").unwrap();
        std::fs::hard_link(&primary, &alias).unwrap();
        let primary_paths = paths(&primary).unwrap();
        let alias_paths = paths(&alias).unwrap();
        assert_eq!(primary_paths.store_id, alias_paths.store_id);
        assert_eq!(primary_paths.addr_file, alias_paths.addr_file);
        assert_eq!(primary_paths.pid_file, alias_paths.pid_file);
        assert_eq!(primary_paths.lock_file, alias_paths.lock_file);
        assert_eq!(primary_paths.sock_file, alias_paths.sock_file);
        assert_ne!(primary_paths.store, alias_paths.store);
        let _ = std::fs::remove_file(alias);
        let _ = std::fs::remove_file(primary);
        let _ = std::fs::remove_dir(root);
    }

    #[cfg(unix)]
    #[test]
    fn daemon_runtime_dir_is_private() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let root =
            std::env::temp_dir().join(format!("loom-daemon-private-dir-{}", std::process::id()));
        let dir = root.join("runtime");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        ensure_runtime_dir(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0);

        let link = root.join("runtime-link");
        symlink(&dir, &link).unwrap();
        assert!(ensure_runtime_dir(&link).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_artifacts_must_be_regular_files() {
        let root = std::env::temp_dir().join(format!(
            "loom-daemon-runtime-artifacts-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = root.join("store.loom");
        std::fs::write(&store, b"store").unwrap();
        let paths = paths(&store).unwrap();
        std::fs::write(&paths.addr_file, "127.0.0.1:1").unwrap();
        std::fs::write(&paths.lock_file, "lock").unwrap();
        std::fs::create_dir(&paths.pid_file).unwrap();
        let err = validate_runtime_artifacts(&paths).unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
        assert!(err.message.contains("pid file"));
        assert!(err.message.contains("is not a regular file"));
        let _ = std::fs::remove_dir(&paths.pid_file);

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            std::fs::write(&paths.pid_file, "123").unwrap();
            let target = root.join("target.lock");
            std::fs::write(&target, "lock").unwrap();
            let _ = std::fs::remove_file(&paths.lock_file);
            symlink(&target, &paths.lock_file).unwrap();
            let err = validate_runtime_artifacts(&paths).unwrap_err();
            assert_eq!(err.code, Code::InvalidArgument);
            assert!(err.message.contains("lock file"));
            assert!(err.message.contains("must not be a symlink"));
        }

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(paths.addr_file);
        let _ = std::fs::remove_file(paths.pid_file);
        let _ = std::fs::remove_file(paths.lock_file);
    }

    #[test]
    fn missing_daemon_address_is_not_found() {
        let root = std::env::temp_dir().join(format!(
            "loom-daemon-missing-address-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let err = request(&root.join("missing.addr"), "status\n").unwrap_err();
        assert_eq!(err.code, Code::NotFound);
        assert!(err.message.contains("daemon is not running"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn daemon_address_file_must_be_regular_file() {
        let root = std::env::temp_dir().join(format!(
            "loom-daemon-bad-address-file-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let err = read_daemon_addr_file(&root).unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
        assert!(err.message.contains("is not a regular file"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let target = root.join("target.addr");
            let link = root.join("link.addr");
            std::fs::write(&target, "127.0.0.1:1").unwrap();
            symlink(&target, &link).unwrap();
            let err = read_daemon_addr_file(&link).unwrap_err();
            assert_eq!(err.code, Code::InvalidArgument);
            assert!(err.message.contains("must not be a symlink"));
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn daemon_address_file_must_be_loopback_socket_addr() {
        assert!(parse_daemon_addr("127.0.0.1:1234").is_ok());
        assert!(parse_daemon_addr("[::1]:1234").is_ok());

        let err = parse_daemon_addr("localhost:1234").unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
        assert!(err.message.contains("invalid daemon address"));

        let err = parse_daemon_addr("192.0.2.1:1234").unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
        assert!(err.message.contains("is not loopback"));
    }

    #[test]
    fn daemon_address_envelope_is_bound_to_identity() {
        let addr: std::net::SocketAddr = "127.0.0.1:3210".parse().unwrap();
        let paths = DaemonPaths {
            store: "/tmp/store.loom".to_string(),
            store_id: "unix:1:2".to_string(),
            addr_file: PathBuf::from("/tmp/store.addr"),
            pid_file: PathBuf::from("/tmp/store.pid"),
            lock_file: PathBuf::from("/tmp/store.lock"),
            sock_file: PathBuf::from("/tmp/store.sock"),
            pipe_name: "uldren-loom-daemon-test".to_string(),
        };
        let envelope = addr_file_contents(&paths, addr);
        assert_eq!(
            parse_daemon_addr_file(&envelope, Some("unix:1:2")).unwrap(),
            addr
        );
        assert!(envelope.contains("protocol=1\n"));
        assert!(envelope.contains("transport=tcp\n"));
        assert!(envelope.contains("security=degraded_loopback\n"));
        assert!(envelope.contains("identity=unix:1:2\n"));
        assert!(envelope.contains("addr=127.0.0.1:3210\n"));

        let err = parse_daemon_addr_file(&envelope, Some("unix:9:9")).unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
        assert!(err.message.contains("not expected identity"));

        assert_eq!(
            parse_daemon_addr_file("127.0.0.1:3210", Some("unix:9:9")).unwrap(),
            addr
        );
        assert!(
            parse_daemon_addr_file(
                "protocol=2\ntransport=tcp\nsecurity=degraded_loopback\nidentity=unix:1:2\naddr=127.0.0.1:3210\n",
                Some("unix:1:2"),
            )
            .is_err()
        );
        assert!(
            parse_daemon_addr_file(
                "protocol=1\ntransport=unix\nsecurity=degraded_loopback\nidentity=unix:1:2\naddr=127.0.0.1:3210\n",
                Some("unix:1:2"),
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_daemon_address_envelope_uses_peer_credentials_security() {
        let paths = DaemonPaths {
            store: "/tmp/store.loom".to_string(),
            store_id: "unix:1:2".to_string(),
            addr_file: PathBuf::from("/tmp/store.addr"),
            pid_file: PathBuf::from("/tmp/store.pid"),
            lock_file: PathBuf::from("/tmp/store.lock"),
            sock_file: PathBuf::from("/tmp/store.sock"),
            pipe_name: "uldren-loom-daemon-test".to_string(),
        };
        let envelope = DaemonEndpointEnvelope::unix_socket(&paths).to_addr_file_contents();
        assert!(envelope.contains("transport=unix_socket\n"));
        assert!(envelope.contains("security=peer_credentials\n"));
        assert!(envelope.contains("addr=/tmp/store.sock\n"));
        let endpoint = parse_daemon_endpoint_file(&envelope, Some("unix:1:2")).unwrap();
        assert_eq!(endpoint, DaemonEndpoint::UnixSocket(paths.sock_file));
        assert_eq!(endpoint.transport(), DaemonTransport::UnixSocket);
        assert_eq!(
            endpoint.security(),
            DaemonTransportSecurity::PeerCredentials
        );

        let err = parse_daemon_endpoint_file(
            "protocol=1\ntransport=unix_socket\nsecurity=owner_runtime_directory\nidentity=unix:1:2\naddr=/tmp/store.sock\n",
            Some("unix:1:2"),
        )
        .unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
        assert!(err.message.contains("peer_credentials"));

        let err = parse_daemon_endpoint_file(
            "protocol=1\ntransport=unix_socket\nsecurity=peer_credentials\nidentity=unix:1:2\naddr=relative.sock\n",
            Some("unix:1:2"),
        )
        .unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
        assert!(err.message.contains("absolute path"));
    }

    #[cfg(unix)]
    #[test]
    fn unix_peer_owner_validation_accepts_same_uid_peer() {
        let root = std::env::temp_dir().join(format!(
            "loom-daemon-peer-credentials-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = root.join("store.loom");
        std::fs::write(&store, b"store").unwrap();
        let paths = paths(&store).unwrap();
        let (server, client) = std::os::unix::net::UnixStream::pair().unwrap();
        assert!(validate_unix_peer_owner(&server, &paths).is_ok());
        assert!(validate_unix_peer_owner(&client, &paths).is_ok());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn windows_pipe_names_are_validated() {
        assert!(validate_windows_pipe_name("uldren-loom-daemon-abc_123").is_ok());
        assert!(validate_windows_pipe_name("other").is_err());
        assert!(validate_windows_pipe_name("uldren-loom-daemon-../x").is_err());
    }

    #[test]
    fn daemon_endpoint_reads_envelope_for_expected_identity() {
        let root =
            std::env::temp_dir().join(format!("loom-daemon-endpoint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = root.join("store.loom");
        std::fs::write(&store, b"store").unwrap();
        let paths = paths(&store).unwrap();
        let addr: std::net::SocketAddr = "127.0.0.1:3210".parse().unwrap();
        std::fs::write(&paths.addr_file, addr_file_contents(&paths, addr)).unwrap();
        assert_eq!(daemon_endpoint(&paths).unwrap(), addr);
        let endpoint = daemon_transport_endpoint(&paths).unwrap();
        assert_eq!(endpoint, DaemonEndpoint::TcpLoopback(addr));
        assert_eq!(endpoint.transport(), DaemonTransport::TcpLoopback);
        assert_eq!(
            endpoint.security(),
            DaemonTransportSecurity::DegradedLoopback
        );
        assert_eq!(endpoint.label(), "127.0.0.1:3210");

        let wrong_paths = DaemonPaths {
            store: paths.store.clone(),
            store_id: "wrong-store".to_string(),
            addr_file: paths.addr_file.clone(),
            pid_file: paths.pid_file.clone(),
            lock_file: paths.lock_file.clone(),
            sock_file: paths.sock_file.clone(),
            pipe_name: paths.pipe_name.clone(),
        };
        let err = daemon_endpoint(&wrong_paths).unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
        assert!(err.message.contains("not expected identity"));

        let _ = std::fs::remove_file(paths.addr_file);
        let _ = std::fs::remove_file(paths.pid_file);
        let _ = std::fs::remove_file(paths.lock_file);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn daemon_error_frames_preserve_stable_codes() {
        let err = parse_daemon_error("LOCK_NOT_HELD: lock is not held by this token\n");
        assert_eq!(err.code, Code::LockNotHeld);
        assert_eq!(err.message, "lock is not held by this token");

        let err = parse_daemon_error("legacy daemon error\n");
        assert_eq!(err.code, Code::InvalidArgument);
        assert_eq!(err.message, "legacy daemon error");
    }

    #[cfg(feature = "integration-tests")]
    #[test]
    fn daemon_request_stream_is_bounded() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let join = std::thread::spawn(move || listener.accept().unwrap().0);
        let stream = std::net::TcpStream::connect(addr).unwrap();
        configure_daemon_stream(&stream).unwrap();
        assert_eq!(stream.read_timeout().unwrap(), Some(REQUEST_TIMEOUT));
        assert_eq!(stream.write_timeout().unwrap(), Some(REQUEST_TIMEOUT));
        drop(stream);
        let _ = join.join().unwrap();
    }

    #[test]
    fn lock_response_json_preserves_token_fields() {
        let json =
            lock_response_json("lock\tresource\talice\ts1\tsemaphore:2:5\t9\t1234\n").unwrap();
        assert!(json.contains("\"key\":\"resource\""));
        assert!(json.contains("\"principal\":\"alice\""));
        assert!(json.contains("\"mode\":\"SEMAPHORE\""));
        assert!(json.contains("\"permits\":2"));
        assert!(json.contains("\"capacity\":5"));
        assert!(json.contains("\"fence\":{\"authority\":0,\"epoch\":0,\"sequence\":9}"));
    }
}
