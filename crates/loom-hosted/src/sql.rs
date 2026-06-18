use std::sync::atomic::{AtomicU32, Ordering};

use gluesql_core::ast::DataType;
use loom_core::error::{Code, LoomError, Result};
use loom_core::{FacetKind, Loom, WorkspaceId, WsSelector};
use loom_sql::LoomSqlStore;
use loom_store::FileStore;

use crate::{HostedAuth, HostedKernel, HostedOutcome, hosted_outcome};

pub struct HostedSqlAdapter<'a> {
    kernel: &'a HostedKernel,
}

impl HostedKernel {
    pub fn sql(&self) -> HostedSqlAdapter<'_> {
        HostedSqlAdapter { kernel: self }
    }
}

impl HostedSqlAdapter<'_> {
    pub fn query_cbor(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        db: &str,
        sql: &str,
    ) -> HostedOutcome<Vec<u8>> {
        let out = self.kernel.with_read_loom(auth, |read| {
            let ns = resolve_sql_ns(&read, workspace)?;
            let mut store = LoomSqlStore::open_read(read, ns, db)?;
            read_query_cbor(&mut store, sql)
        });
        hosted_outcome(out)
    }

    pub fn exec_cbor(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        db: &str,
        sql: &str,
    ) -> HostedOutcome<Vec<u8>> {
        let out = (|| {
            self.kernel
                .write(auth, |loom| ensure_sql_ns(loom, workspace).map(|_| ()))?;
            let (out, mut store) = self.kernel.with_read_loom(auth, |read| {
                let ns = resolve_sql_ns(&read, workspace)?;
                let mut store = LoomSqlStore::open_write(read, ns, db)?;
                let out = store.exec_cbor(sql)?;
                if store.in_transaction() {
                    return Err(LoomError::invalid(
                        "BEGIN without a matching COMMIT/ROLLBACK in one exec",
                    ));
                }
                Ok((out, store))
            })?;
            if store.is_dirty() {
                self.kernel.write(auth, |loom| {
                    let ns = resolve_sql_ns(loom, workspace)?;
                    store.persist(loom, ns, db)
                })?;
            }
            Ok(out)
        })();
        hosted_outcome(out)
    }

    pub fn infer_parameter_types(
        &self,
        auth: &HostedAuth,
        workspace: &str,
        db: &str,
        sql: &str,
    ) -> HostedOutcome<Vec<Option<DataType>>> {
        let out = self.kernel.with_read_loom(auth, |read| {
            let ns = resolve_sql_ns(&read, workspace)?;
            let store = LoomSqlStore::open_read(read, ns, db)?;
            store.infer_parameter_types(sql)
        });
        hosted_outcome(out)
    }
}

fn read_query_cbor(store: &mut LoomSqlStore, sql: &str) -> Result<Vec<u8>> {
    let out = store.exec_cbor(sql)?;
    if store.in_transaction() {
        return Err(LoomError::invalid(
            "BEGIN without a matching COMMIT/ROLLBACK in one query",
        ));
    }
    if store.is_dirty() {
        return Err(LoomError::new(
            Code::PermissionDenied,
            "sql.query is read-only; use sql.exec for statements that mutate state",
        ));
    }
    Ok(out)
}

fn resolve_sql_ns(loom: &Loom<FileStore>, name: &str) -> Result<WorkspaceId> {
    loom.registry().open(&WsSelector::Typed {
        ty: FacetKind::Sql,
        name: name.to_string(),
    })
}

fn ensure_sql_ns(loom: &mut Loom<FileStore>, name: &str) -> Result<WorkspaceId> {
    match resolve_sql_ns(loom, name) {
        Ok(ns) => Ok(ns),
        Err(err) if err.code == Code::NotFound => {
            loom.authorize_global_admin()?;
            loom.registry_mut().ensure_for_write(
                &WsSelector::Typed {
                    ty: FacetKind::Sql,
                    name: name.to_string(),
                },
                fresh_workspace_id(),
            )
        }
        Err(err) => Err(err),
    }
}

fn fresh_workspace_id() -> WorkspaceId {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let mut bytes = [0u8; 16];
    bytes[0..8].copy_from_slice(&nanos.to_be_bytes());
    bytes[8..12].copy_from_slice(&pid.to_be_bytes());
    bytes[12..16].copy_from_slice(&seq.to_be_bytes());
    WorkspaceId::v4_from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use loom_core::Code;

    use crate::test_support::{init, nid, temp_path};
    use crate::{HostedAuth, HostedKernel};

    #[test]
    fn hosted_sql_exec_and_query_attach_auth() {
        let path = temp_path("sql");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let sql = kernel.sql();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "sql-1");

        sql.exec_cbor(
            &auth,
            "main",
            "db",
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'a')",
        )
        .unwrap();
        let result = sql
            .query_cbor(&auth, "main", "db", "SELECT id, v FROM t")
            .unwrap();
        assert!(!result.is_empty());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn hosted_sql_missing_auth_fails_before_query() {
        let path = temp_path("sql-auth");
        init(&path, None);
        let err = HostedKernel::new(&path)
            .sql()
            .query_cbor(&HostedAuth::unauthenticated(), "main", "db", "SELECT 1")
            .unwrap_err();
        assert_eq!(err.code, Code::AuthenticationFailed);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn hosted_sql_query_rejects_mutation() {
        let path = temp_path("sql-query");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "sql-1");
        let err = kernel
            .sql()
            .query_cbor(
                &auth,
                "main",
                "db",
                "CREATE TABLE t (id INTEGER PRIMARY KEY)",
            )
            .unwrap_err();
        assert_eq!(err.code, Code::PermissionDenied);
        fs::remove_file(path).unwrap();
    }
}
