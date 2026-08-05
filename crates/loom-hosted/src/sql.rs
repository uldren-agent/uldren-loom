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
        let out = self.kernel.write_current_state(auth, |loom| {
            loom_client::local::execute_generated_sql_result(
                self.kernel.path(),
                loom,
                workspace,
                db,
                sql,
            )
        });
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
    let selector = match WorkspaceId::parse(name) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Name(name.to_string()),
    };
    match loom.registry().open(&selector) {
        Ok(id) => Ok(id),
        Err(err) if err.code == Code::NotFound => loom.registry().open(&WsSelector::Typed {
            ty: FacetKind::Sql,
            name: name.to_string(),
        }),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use loom_client::LocalLoomClient;
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

    #[test]
    fn mu_6h_l_b_hosted_sql_exec_delegates_to_generated_owner() {
        let hosted_path = temp_path("sql-generated-hosted");
        let local_path = temp_path("sql-generated-local");
        init(&hosted_path, None);
        init(&local_path, None);

        let sql_text =
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'a')";
        let hosted = HostedKernel::new(&hosted_path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "sql-generated-hosted");
        let hosted_bytes = hosted
            .sql()
            .exec_cbor(&auth, "main", "db", sql_text)
            .expect("hosted exec");

        let local = LocalLoomClient::new(&local_path);
        let session = local.open().expect("local open");
        local
            .authenticate_passphrase(&session, nid(1), b"root-pass")
            .expect("local auth");
        let local_bytes = local
            .sql_exec_result(&session, "main", "db", sql_text)
            .expect("local generated exec");
        assert_eq!(hosted_bytes, local_bytes);
        local.close(&session);

        let selected = hosted
            .sql()
            .query_cbor(&auth, "main", "db", "SELECT id, v FROM t")
            .expect("hosted query after exec");
        assert!(!selected.is_empty());
        fs::remove_file(hosted_path).unwrap();
        fs::remove_file(local_path).unwrap();
    }
}
