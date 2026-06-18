use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use loom_coordination::local_store_write_lock;
use loom_core::error::{Code, ErrorDetail, LoomError, Result};
use loom_core::keys::KeySpec;
use loom_core::{Algo, Loom, RuntimeProfile, WorkspaceId};
use loom_store::{
    FileStore, LocalOpenAuth, VerifiedExternalCredential, attach_local_auth,
    local_auth_requires_write, open_loom_daemon_authorized_unlocked, open_loom_read_unlocked,
    open_loom_unlocked, save_loom,
};

pub mod generated_dispatch;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "network-access")]
pub mod network_access;
pub mod remote;
#[cfg(all(feature = "http", feature = "tls"))]
pub mod remote_carrier;
pub mod remote_http;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use loom_core::digest::Algo;
    use loom_core::{
        AclRight, AclStore, AclSubject, Digest, FacetKind, IdentityStore, Loom, PrincipalKind,
        WorkspaceId,
    };
    use loom_store::{FileStore, save_loom};

    pub fn nid(byte: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([byte; 16])
    }

    pub fn temp_path(name: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-hosted-{name}-{}-{}-{}.loom",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed),
            nonce,
        ));
        let _ = fs::remove_file(&path);
        path
    }

    pub fn init(path: &Path, user: Option<WorkspaceId>) -> WorkspaceId {
        with_init_loom(path, user, |_, ns| ns)
    }

    fn with_init_loom<T>(
        path: &Path,
        user: Option<WorkspaceId>,
        f: impl FnOnce(&mut Loom<FileStore>, WorkspaceId) -> T,
    ) -> T {
        loom_coordination::with_local_store_write_lock(path, || {
            let (mut loom, ns) = init_loom(path, user);
            let out = f(&mut loom, ns);
            save_loom(&mut loom)?;
            drop(loom);
            Ok(out)
        })
        .unwrap()
    }

    fn init_loom(path: &Path, user: Option<WorkspaceId>) -> (Loom<FileStore>, WorkspaceId) {
        let root = nid(1);
        let ns = nid(9);
        let algo = if cfg!(feature = "fips") {
            Algo::Sha256
        } else {
            Algo::Blake3
        };
        let fs = FileStore::create_with_profile(path, algo).unwrap();
        let mut identity = IdentityStore::new(root);
        identity
            .set_passphrase(root, "root-pass", b"12345678")
            .unwrap();
        if let Some(user) = user {
            identity
                .add_principal(user, "alice", PrincipalKind::User)
                .unwrap();
            identity
                .set_passphrase(user, "alice-pass", b"abcdefgh")
                .unwrap();
        }
        fs.save_identity_store(&identity).unwrap();
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Principal(root),
            None,
            None,
            [
                AclRight::Admin,
                AclRight::Read,
                AclRight::Write,
                AclRight::Advance,
                AclRight::Merge,
                AclRight::Execute,
            ],
        )
        .unwrap();
        fs.save_acl_store(&acl).unwrap();
        let mut loom = Loom::new(fs);
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, Some("main"), ns)
            .unwrap();
        for facet in [
            FacetKind::Cas,
            FacetKind::Sql,
            FacetKind::Calendar,
            FacetKind::Contacts,
            FacetKind::Mail,
        ] {
            loom.registry_mut().add_facet(ns, facet).unwrap();
        }
        (loom, ns)
    }

    pub fn watch_history(path: &Path) -> (WorkspaceId, Digest, Digest) {
        with_init_loom(path, None, |loom, ns| {
            loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
            let c0 = loom.commit(ns, "watch", "c0", 1).unwrap();
            loom.write_file(ns, "b.txt", b"b", 0o100644).unwrap();
            let c1 = loom.commit(ns, "watch", "c1", 2).unwrap();
            (ns, c0, c1)
        })
    }
}

pub type HostedRuntimeProfile = RuntimeProfile;

pub const fn hosted_runtime_profile() -> HostedRuntimeProfile {
    if cfg!(feature = "fips") {
        loom_core::runtime_profile_with_tls("rustls-aws-lc-fips", true)
    } else {
        loom_core::runtime_profile_with_tls("rustls-aws-lc", false)
    }
}

pub fn validate_hosted_store_profile(store_algo: Algo, fips_required: bool) -> Result<()> {
    let profile = hosted_runtime_profile();
    if profile.fips_capable && store_algo != Algo::Sha256 {
        return Err(LoomError::new(
            Code::PermissionDenied,
            "FIPS hosted runtime requires a FIPS-profile store",
        ));
    }
    if !profile.fips_capable && store_algo == Algo::Sha256 {
        return Err(LoomError::new(
            Code::PermissionDenied,
            "FIPS-profile stores cannot be served by the current non-FIPS hosted runtime",
        ));
    }
    if fips_required && !profile.fips_capable {
        return Err(LoomError::new(
            Code::PermissionDenied,
            "FIPS-required stores cannot be served by the current non-FIPS hosted runtime",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostedAuthPolicy {
    OwnerOrPassphrase,
    Passphrase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostedHttpLimits {
    pub request_size_limit: usize,
    pub idle_timeout: Duration,
    pub session_timeout: Duration,
}

impl HostedHttpLimits {
    pub fn new(
        request_size_limit: usize,
        idle_timeout_ms: u64,
        session_timeout_ms: u64,
    ) -> std::io::Result<Self> {
        if request_size_limit == 0 || idle_timeout_ms == 0 || session_timeout_ms == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "hosted HTTP limits must be positive",
            ));
        }
        Ok(Self {
            request_size_limit,
            idle_timeout: Duration::from_millis(idle_timeout_ms),
            session_timeout: Duration::from_millis(session_timeout_ms),
        })
    }
}

impl Default for HostedHttpLimits {
    fn default() -> Self {
        Self {
            request_size_limit: 16 * 1024 * 1024,
            idle_timeout: Duration::from_secs(60),
            session_timeout: Duration::from_secs(60 * 60),
        }
    }
}

#[cfg(feature = "tls")]
#[derive(Clone)]
pub struct HostedTlsConfig {
    config: Arc<rustls::ServerConfig>,
}

#[cfg(feature = "tls")]
impl HostedTlsConfig {
    pub fn from_pem_files(cert_ref: &str, key_ref: &str) -> std::io::Result<Self> {
        Self::from_pem_files_with_client_trust(cert_ref, key_ref, None)
    }

    pub fn from_pem_files_with_client_trust(
        cert_ref: &str,
        key_ref: &str,
        trust_bundle_ref: Option<&str>,
    ) -> std::io::Result<Self> {
        use rustls::pki_types::pem::PemObject;

        ensure_rustls_crypto_provider();
        let certs: Vec<_> = rustls::pki_types::CertificateDer::pem_file_iter(cert_ref)
            .map_err(|e| invalid_tls_config(cert_ref, e))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| invalid_tls_config(cert_ref, e))?;
        if certs.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("TLS certificate file {cert_ref:?} contains no certificates"),
            ));
        }
        let key = rustls::pki_types::PrivateKeyDer::from_pem_file(key_ref)
            .map_err(|e| invalid_tls_config(key_ref, e))?;
        let builder = rustls::ServerConfig::builder();
        let builder = match trust_bundle_ref {
            Some(path) => {
                let mut roots = rustls::RootCertStore::empty();
                let mut count = 0usize;
                for cert in rustls::pki_types::CertificateDer::pem_file_iter(path)
                    .map_err(|e| invalid_tls_config(path, e))?
                {
                    roots
                        .add(cert.map_err(|e| invalid_tls_config(path, e))?)
                        .map_err(|e| invalid_tls_config(path, e))?;
                    count += 1;
                }
                if count == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("TLS trust bundle {path:?} contains no certificates"),
                    ));
                }
                let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots))
                    .build()
                    .map_err(|e| invalid_tls_config(path, e))?;
                builder.with_client_cert_verifier(verifier)
            }
            None => builder.with_no_client_auth(),
        };
        let config = builder
            .with_single_cert(certs, key)
            .map_err(|e| invalid_tls_config(cert_ref, e))?;
        Ok(Self {
            config: Arc::new(config),
        })
    }

    pub fn from_pem_bytes_with_client_trust(
        cert_ref: &str,
        cert_pem: &[u8],
        key_ref: &str,
        key_pem: &[u8],
        trust_bundle_ref: Option<(&str, &[u8])>,
    ) -> std::io::Result<Self> {
        use rustls::pki_types::pem::PemObject;

        ensure_rustls_crypto_provider();
        let certs: Vec<_> = rustls::pki_types::CertificateDer::pem_slice_iter(cert_pem)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| invalid_tls_config(cert_ref, e))?;
        if certs.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("TLS certificate bundle {cert_ref:?} contains no certificates"),
            ));
        }
        let key = rustls::pki_types::PrivateKeyDer::from_pem_slice(key_pem)
            .map_err(|e| invalid_tls_config(key_ref, e))?;
        let builder = rustls::ServerConfig::builder();
        let builder = match trust_bundle_ref {
            Some((path, pem)) => {
                let mut roots = rustls::RootCertStore::empty();
                let mut count = 0usize;
                for cert in rustls::pki_types::CertificateDer::pem_slice_iter(pem) {
                    roots
                        .add(cert.map_err(|e| invalid_tls_config(path, e))?)
                        .map_err(|e| invalid_tls_config(path, e))?;
                    count += 1;
                }
                if count == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("TLS trust bundle {path:?} contains no certificates"),
                    ));
                }
                let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots))
                    .build()
                    .map_err(|e| invalid_tls_config(path, e))?;
                builder.with_client_cert_verifier(verifier)
            }
            None => builder.with_no_client_auth(),
        };
        let config = builder
            .with_single_cert(certs, key)
            .map_err(|e| invalid_tls_config(cert_ref, e))?;
        Ok(Self {
            config: Arc::new(config),
        })
    }

    pub fn acceptor(&self) -> tokio_rustls::TlsAcceptor {
        tokio_rustls::TlsAcceptor::from(self.config.clone())
    }

    /// The shared rustls `ServerConfig`, for carriers (like the remote HTTP/2 server) that build their
    /// own TLS acceptor from it.
    pub fn server_config(&self) -> Arc<rustls::ServerConfig> {
        self.config.clone()
    }
}

#[cfg(feature = "tls")]
fn ensure_rustls_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

#[cfg(feature = "tls")]
fn invalid_tls_config(path: &str, err: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("invalid TLS material {path:?}: {err}"),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedAuth {
    pub principal: Option<WorkspaceId>,
    pub passphrase: Option<String>,
    pub app_credential: Option<String>,
    pub verified_external: Option<VerifiedExternalCredential>,
    pub preauthenticated_principal: Option<WorkspaceId>,
    pub session_id: String,
}

impl HostedAuth {
    pub fn unauthenticated() -> Self {
        Self {
            principal: None,
            passphrase: None,
            app_credential: None,
            verified_external: None,
            preauthenticated_principal: None,
            session_id: "hosted".to_string(),
        }
    }

    pub fn passphrase(
        principal: WorkspaceId,
        passphrase: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            principal: Some(principal),
            passphrase: Some(passphrase.into()),
            app_credential: None,
            verified_external: None,
            preauthenticated_principal: None,
            session_id: session_id.into(),
        }
    }

    pub fn app_credential(
        app_credential: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            principal: None,
            passphrase: None,
            app_credential: Some(app_credential.into()),
            verified_external: None,
            preauthenticated_principal: None,
            session_id: session_id.into(),
        }
    }

    pub fn verified_external(
        credential: VerifiedExternalCredential,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            principal: None,
            passphrase: None,
            app_credential: None,
            verified_external: Some(credential),
            preauthenticated_principal: None,
            session_id: session_id.into(),
        }
    }

    pub fn preauthenticated(principal: WorkspaceId, session_id: impl Into<String>) -> Self {
        Self {
            principal: None,
            passphrase: None,
            app_credential: None,
            verified_external: None,
            preauthenticated_principal: Some(principal),
            session_id: session_id.into(),
        }
    }

    fn local_open_auth(&self, unlock_key: Option<KeySpec>) -> LocalOpenAuth {
        LocalOpenAuth {
            unlock_key,
            principal: self.principal,
            passphrase: self.passphrase.clone(),
            app_credential: self.app_credential.clone(),
            verified_external: self.verified_external.clone(),
            preauthenticated_principal: self.preauthenticated_principal,
            session_id: Some(self.session_id.clone()),
        }
    }
}

impl Default for HostedAuth {
    fn default() -> Self {
        Self::unauthenticated()
    }
}

#[derive(Clone)]
pub struct HostedKernel {
    path: PathBuf,
    unlock_key: Option<KeySpec>,
    write_guard: HostedWriteGuard,
    write_lock: Arc<Mutex<()>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostedWriteGuard {
    DirectFileLock,
    DaemonAuthorized,
}

impl HostedKernel {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let write_lock = local_store_write_lock(&path);
        Self {
            path,
            unlock_key: None,
            write_guard: HostedWriteGuard::DirectFileLock,
            write_lock,
        }
    }

    pub fn with_unlock_key(mut self, unlock_key: KeySpec) -> Self {
        self.unlock_key = Some(unlock_key);
        self
    }

    pub fn with_write_guard(mut self, write_guard: HostedWriteGuard) -> Self {
        self.write_guard = write_guard;
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write_guard(&self) -> HostedWriteGuard {
        self.write_guard
    }

    fn open_read_loom_with_auth(&self, local_auth: LocalOpenAuth) -> Result<Loom<FileStore>> {
        let mut loom = if local_auth_requires_write(&local_auth) {
            match self.write_guard {
                HostedWriteGuard::DirectFileLock => {
                    open_loom_unlocked(&self.path, local_auth.unlock_key.as_ref())?
                }
                HostedWriteGuard::DaemonAuthorized => open_loom_daemon_authorized_unlocked(
                    &self.path,
                    local_auth.unlock_key.as_ref(),
                )?,
            }
        } else {
            open_loom_read_unlocked(&self.path, local_auth.unlock_key.as_ref())?
        };
        loom.set_acl_predicate_evaluator(Arc::new(loom_compute::CelAclPredicateEvaluator));
        attach_local_auth(loom, &local_auth)
    }

    fn open_write_loom(&self, auth: &HostedAuth) -> Result<Loom<FileStore>> {
        let local_auth = auth.local_open_auth(self.unlock_key.clone());
        let mut loom = match self.write_guard {
            HostedWriteGuard::DirectFileLock => {
                open_loom_unlocked(&self.path, local_auth.unlock_key.as_ref())?
            }
            HostedWriteGuard::DaemonAuthorized => {
                open_loom_daemon_authorized_unlocked(&self.path, local_auth.unlock_key.as_ref())?
            }
        };
        loom.set_acl_predicate_evaluator(Arc::new(loom_compute::CelAclPredicateEvaluator));
        attach_local_auth(loom, &local_auth)
    }

    pub fn read<T>(
        &self,
        auth: &HostedAuth,
        f: impl FnOnce(&Loom<FileStore>) -> Result<T>,
    ) -> Result<T> {
        self.with_read_loom(auth, |loom| f(&loom))
    }

    pub fn read_mut<T>(
        &self,
        auth: &HostedAuth,
        f: impl FnOnce(&mut Loom<FileStore>) -> Result<T>,
    ) -> Result<T> {
        self.with_read_loom(auth, |mut loom| f(&mut loom))
    }

    pub fn with_read_loom<T>(
        &self,
        auth: &HostedAuth,
        f: impl FnOnce(Loom<FileStore>) -> Result<T>,
    ) -> Result<T> {
        let local_auth = auth.local_open_auth(self.unlock_key.clone());
        let out = if local_auth_requires_write(&local_auth) {
            let _guard = self
                .write_lock
                .lock()
                .map_err(|_| LoomError::new(Code::Internal, "hosted kernel write lock poisoned"))?;
            self.open_read_loom_with_auth(local_auth).and_then(f)
        } else {
            self.open_read_loom_with_auth(local_auth).and_then(f)
        };
        if let Err(err) = &out {
            self.audit_security_failure(auth, err);
        }
        out
    }

    pub fn write<T>(
        &self,
        auth: &HostedAuth,
        f: impl FnOnce(&mut Loom<FileStore>) -> Result<T>,
    ) -> Result<T> {
        let out = {
            let _guard = self
                .write_lock
                .lock()
                .map_err(|_| LoomError::new(Code::Internal, "hosted kernel write lock poisoned"))?;
            self.open_write_loom(auth).and_then(|mut loom| {
                let out = f(&mut loom)?;
                save_loom(&mut loom)?;
                drop(loom);
                Ok(out)
            })
        };
        if let Err(err) = &out {
            self.audit_security_failure(auth, err);
        }
        out
    }

    pub fn audit_append(
        &self,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| LoomError::new(Code::Internal, "hosted kernel write lock poisoned"))?;
        let store = match self.write_guard {
            HostedWriteGuard::DirectFileLock => FileStore::open(&self.path)?,
            HostedWriteGuard::DaemonAuthorized => FileStore::open_daemon_authorized(&self.path)?,
        };
        store.validate_runtime_policy()?;
        store.audit_append(principal, action, target)
    }

    pub fn audit_security_failure(&self, auth: &HostedAuth, err: &LoomError) {
        let action = match err.code {
            Code::AuthenticationFailed | Code::E2eKeyInvalid => "hosted.auth.failed",
            Code::PermissionDenied => "hosted.auth.denied",
            _ => return,
        };
        let target = format!("session={};code={}", auth.session_id, err.code.as_str());
        let _ = self.audit_append(auth.principal, action, Some(&target));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedError {
    pub code: Code,
    pub code_name: &'static str,
    pub code_number: i32,
    pub message: String,
    pub details: Vec<ErrorDetail>,
}

impl HostedError {
    pub fn from_error(err: LoomError) -> Self {
        Self {
            code: err.code,
            code_name: err.code.as_str(),
            code_number: err.code.as_i32(),
            message: err.message,
            details: err.details,
        }
    }
}

pub type HostedOutcome<T> = std::result::Result<T, HostedError>;

pub fn hosted_outcome<T>(result: Result<T>) -> HostedOutcome<T> {
    result.map_err(HostedError::from_error)
}

#[cfg(test)]
mod hosted_kernel_tests {
    use super::*;

    #[test]
    fn hosted_kernels_for_same_store_share_write_lock() {
        let path = std::env::temp_dir().join(format!(
            "loom-hosted-core-write-lock-{}.loom",
            std::process::id()
        ));
        let first = HostedKernel::new(&path);
        let second = HostedKernel::new(&path);

        assert!(Arc::ptr_eq(&first.write_lock, &second.write_lock));
    }

    #[cfg(unix)]
    #[test]
    fn hosted_write_lock_canonicalizes_parent_before_store_exists() {
        let base = std::env::temp_dir().join(format!(
            "loom-hosted-core-write-lock-parent-{}",
            std::process::id()
        ));
        let real = base.join("real");
        let alias = base.join("alias");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();

        let first = HostedKernel::new(real.join("store.loom"));
        let second = HostedKernel::new(alias.join("store.loom"));

        assert!(Arc::ptr_eq(&first.write_lock, &second.write_lock));
        let _ = std::fs::remove_dir_all(&base);
    }
}
