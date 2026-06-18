use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::io;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use loom_codec::Value as CborValue;
use loom_core::WorkspaceId;
use loom_core::{Value, key_from_cbor, key_to_cbor};
use loom_redis::{RedisKeyspace, RedisPersistenceMode, RedisTtl, RedisValueKind};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::sync::broadcast;

use crate::{HostedAuth, HostedError, HostedKernel};

const META_MAGIC: &[u8; 4] = b"LRM1";

pub async fn serve_redis_resp<F>(
    listener: TcpListener,
    kernel: HostedKernel,
    workspace: String,
    keyspace_name: String,
    shutdown: F,
) -> io::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let state = Arc::new(RedisServerState {
        kernel,
        target: format!("redis:{workspace}:{keyspace_name}"),
        workspace,
        keyspace_name,
        loaded: Mutex::new(false),
        keyspace: Mutex::new(RedisKeyspace::new(RedisPersistenceMode::Versioned)),
        pubsub: Mutex::new(RedisPubSubState::new()),
    });
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => return Ok(()),
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    let _ = RedisConnection {
                        stream,
                        state,
                        auth: None,
                    }.run().await;
                });
            }
        }
    }
}

struct RedisServerState {
    kernel: HostedKernel,
    target: String,
    workspace: String,
    keyspace_name: String,
    loaded: Mutex<bool>,
    keyspace: Mutex<RedisKeyspace>,
    pubsub: Mutex<RedisPubSubState>,
}

struct RedisConnection<S> {
    stream: S,
    state: Arc<RedisServerState>,
    auth: Option<HostedAuth>,
}

impl<S> RedisConnection<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    async fn run(mut self) -> io::Result<()> {
        loop {
            let command = match read_resp_array(&mut self.stream).await {
                Ok(Some(command)) => command,
                Ok(None) => return Ok(()),
                Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(err) if err.kind() == io::ErrorKind::InvalidData => {
                    self.write_error("ERR protocol error").await?;
                    continue;
                }
                Err(err) => return Err(err),
            };
            self.handle_command(command).await?;
        }
    }

    async fn handle_command(&mut self, command: Vec<Vec<u8>>) -> io::Result<()> {
        let Some(name) = command
            .first()
            .and_then(|value| std::str::from_utf8(value).ok())
        else {
            return self.write_error("ERR empty command").await;
        };
        match name.to_ascii_uppercase().as_str() {
            "AUTH" => self.handle_auth(&command).await,
            "PING" => self.handle_ping(&command).await,
            "QUIT" => {
                self.write_simple("OK").await?;
                Err(io::Error::new(io::ErrorKind::UnexpectedEof, "client quit"))
            }
            "SET" | "GET" | "DEL" | "EXPIRE" | "PEXPIRE" | "TTL" | "PTTL" | "PERSIST"
            | "DBSIZE" | "TYPE" | "HSET" | "HGET" | "HLEN" | "SADD" | "SISMEMBER" | "SCARD"
            | "LPUSH" | "RPUSH" | "LPOP" | "RPOP" | "LLEN" | "ZADD" | "ZSCORE" | "ZCARD"
            | "XADD" | "XLEN" | "XRANGE" | "XREVRANGE" | "XREAD" | "XGROUP" | "XACK" | "XDEL"
            | "XTRIM" | "PUBLISH" | "SUBSCRIBE" | "PSUBSCRIBE" | "UNSUBSCRIBE" | "PUNSUBSCRIBE"
            | "PUBSUB" => {
                if !self.require_auth().await? {
                    return Ok(());
                }
                self.handle_key_command(&command).await
            }
            _ => self.write_error("ERR unsupported Redis command").await,
        }
    }

    async fn handle_auth(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        let (principal, secret) = match command {
            [_, secret] => ("", secret.as_slice()),
            [_, principal, secret] => (bytes_to_str(principal)?, secret.as_slice()),
            _ => {
                return self
                    .write_error("ERR wrong number of arguments for AUTH")
                    .await;
            }
        };
        let secret = bytes_to_string(secret)?;
        let auth = if secret.starts_with("loom_app_") {
            HostedAuth::app_credential(
                secret,
                format!("redis-resp-app:{}:{principal}", self.state.target),
            )
        } else {
            let principal = WorkspaceId::parse(principal)
                .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "bad principal"))?;
            HostedAuth::passphrase(
                principal,
                secret,
                format!("redis-resp:{}:{principal}", self.state.target),
            )
        };
        self.state.kernel.read(&auth, |_| Ok(())).map_err(|_| {
            io::Error::new(io::ErrorKind::PermissionDenied, "authentication failed")
        })?;
        self.ensure_loaded(&auth).await?;
        self.auth = Some(auth);
        self.write_simple("OK").await
    }

    async fn handle_ping(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        match command {
            [_] => self.write_simple("PONG").await,
            [_, value] => self.write_bulk(value).await,
            _ => {
                self.write_error("ERR wrong number of arguments for PING")
                    .await
            }
        }
    }

    async fn handle_key_command(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        let name = std::str::from_utf8(&command[0])
            .map(|value| value.to_ascii_uppercase())
            .unwrap_or_default();
        match name.as_str() {
            "SET" => self.redis_set(command).await,
            "GET" => self.redis_get(command).await,
            "DEL" => self.redis_del(command).await,
            "EXPIRE" | "PEXPIRE" => self.redis_expire(command, name == "PEXPIRE").await,
            "TTL" | "PTTL" => self.redis_ttl(command, name == "PTTL").await,
            "PERSIST" => self.redis_persist(command).await,
            "DBSIZE" => self.redis_dbsize(command).await,
            "TYPE" => self.redis_type(command).await,
            "HSET" => self.redis_hset(command).await,
            "HGET" => self.redis_hget(command).await,
            "HLEN" => self.redis_hlen(command).await,
            "SADD" => self.redis_sadd(command).await,
            "SISMEMBER" => self.redis_sismember(command).await,
            "SCARD" => self.redis_scard(command).await,
            "LPUSH" => self.redis_push(command, true).await,
            "RPUSH" => self.redis_push(command, false).await,
            "LPOP" => self.redis_pop(command, true).await,
            "RPOP" => self.redis_pop(command, false).await,
            "LLEN" => self.redis_llen(command).await,
            "ZADD" => self.redis_zadd(command).await,
            "ZSCORE" => self.redis_zscore(command).await,
            "ZCARD" => self.redis_zcard(command).await,
            "XADD" => self.redis_xadd(command).await,
            "XLEN" => self.redis_xlen(command).await,
            "XRANGE" => self.redis_xrange(command, false).await,
            "XREVRANGE" => self.redis_xrange(command, true).await,
            "XREAD" => self.redis_xread(command).await,
            "XDEL" => self.redis_xdel(command).await,
            "XGROUP" | "XACK" | "XTRIM" => {
                self.write_error("ERR unsupported Redis stream command")
                    .await
            }
            "PUBLISH" => self.redis_publish(command).await,
            "SUBSCRIBE" => self.redis_subscribe(command, false).await,
            "PSUBSCRIBE" => self.redis_subscribe(command, true).await,
            "PUBSUB" => self.redis_pubsub(command).await,
            "UNSUBSCRIBE" | "PUNSUBSCRIBE" => self.write_integer(0).await,
            _ => self.write_error("ERR unsupported Redis command").await,
        }
    }

    async fn redis_set(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() < 3 {
            return self
                .write_error("ERR wrong number of arguments for SET")
                .await;
        }
        let key = command[1].clone();
        let value = command[2].clone();
        let mut expires_at_ms = None;
        let mut cursor = 3;
        while cursor < command.len() {
            let option = bytes_to_str(&command[cursor])?.to_ascii_uppercase();
            match option.as_str() {
                "EX" | "PX" => {
                    let Some(raw) = command.get(cursor + 1) else {
                        return self.write_error("ERR missing expiry value").await;
                    };
                    let amount = parse_u64(raw)?;
                    let ms = if option == "EX" {
                        amount.saturating_mul(1000)
                    } else {
                        amount
                    };
                    expires_at_ms = Some(now_ms().saturating_add(ms));
                    cursor += 2;
                }
                _ => return self.write_error("ERR unsupported SET option").await,
            }
        }
        let auth = self.current_auth()?.clone();
        self.delete_persisted_key(&auth, &command[1])?;
        {
            let mut keyspace = self.lock_keyspace().await;
            keyspace.set_string(key, value, expires_at_ms);
        }
        self.persist_string(&auth, &command[1], &command[2], expires_at_ms)?;
        self.write_simple("OK").await
    }

    async fn redis_get(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() != 2 {
            return self
                .write_error("ERR wrong number of arguments for GET")
                .await;
        }
        let value = {
            let keyspace = self.lock_keyspace().await;
            keyspace
                .get_string(&command[1], now_ms())
                .map_err(loom_error_to_io)?
                .map(|value| value.to_vec())
        };
        match value {
            Some(value) => self.write_bulk(&value).await,
            None => self.write_null_bulk().await,
        }
    }

    async fn redis_del(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() < 2 {
            return self
                .write_error("ERR wrong number of arguments for DEL")
                .await;
        }
        let auth = self.current_auth()?.clone();
        let mut removed = 0_i64;
        {
            let mut keyspace = self.lock_keyspace().await;
            for key in &command[1..] {
                let kind = keyspace.live_key_kind(key, now_ms());
                if keyspace.delete(key) {
                    self.delete_persisted_key(&auth, key)?;
                    if matches!(kind, Some(RedisValueKind::Stream)) {
                        self.delete_stream_queue(&auth, key)?;
                    }
                    removed += 1;
                }
            }
        }
        self.write_integer(removed).await
    }

    async fn redis_expire(&mut self, command: &[Vec<u8>], milliseconds: bool) -> io::Result<()> {
        if command.len() != 3 {
            return self
                .write_error("ERR wrong number of arguments for EXPIRE")
                .await;
        }
        let amount = parse_u64(&command[2])?;
        let delta_ms = if milliseconds {
            amount
        } else {
            amount.saturating_mul(1000)
        };
        let auth = self.current_auth()?.clone();
        let changed = {
            let mut keyspace = self.lock_keyspace().await;
            keyspace.expire_at(&command[1], now_ms().saturating_add(delta_ms), now_ms())
        };
        if changed {
            self.persist_existing_string_meta(&auth, &command[1])?;
        }
        self.write_integer(i64::from(changed)).await
    }

    async fn redis_ttl(&mut self, command: &[Vec<u8>], milliseconds: bool) -> io::Result<()> {
        if command.len() != 2 {
            return self
                .write_error("ERR wrong number of arguments for TTL")
                .await;
        }
        let ttl = {
            let keyspace = self.lock_keyspace().await;
            keyspace.ttl_ms(&command[1], now_ms())
        };
        let value = match ttl {
            RedisTtl::NoKey => -2,
            RedisTtl::Persistent => -1,
            RedisTtl::RemainingMs(ms) if milliseconds => ms.min(i64::MAX as u64) as i64,
            RedisTtl::RemainingMs(ms) => ms.div_ceil(1000).min(i64::MAX as u64) as i64,
        };
        self.write_integer(value).await
    }

    async fn redis_persist(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() != 2 {
            return self
                .write_error("ERR wrong number of arguments for PERSIST")
                .await;
        }
        let auth = self.current_auth()?.clone();
        let changed = {
            let mut keyspace = self.lock_keyspace().await;
            keyspace.persist(&command[1], now_ms())
        };
        if changed {
            self.persist_existing_string_meta(&auth, &command[1])?;
        }
        self.write_integer(i64::from(changed)).await
    }

    async fn redis_dbsize(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() != 1 {
            return self
                .write_error("ERR wrong number of arguments for DBSIZE")
                .await;
        }
        let count = {
            let keyspace = self.lock_keyspace().await;
            keyspace.key_count_live(now_ms())
        };
        self.write_integer(count.min(i64::MAX as usize) as i64)
            .await
    }

    async fn redis_type(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() != 2 {
            return self
                .write_error("ERR wrong number of arguments for TYPE")
                .await;
        }
        let kind = {
            let keyspace = self.lock_keyspace().await;
            keyspace.live_key_kind(&command[1], now_ms())
        };
        let name = match kind {
            Some(loom_redis::RedisValueKind::String) => "string",
            Some(loom_redis::RedisValueKind::Hash) => "hash",
            Some(loom_redis::RedisValueKind::Set) => "set",
            Some(loom_redis::RedisValueKind::List) => "list",
            Some(loom_redis::RedisValueKind::SortedSet) => "zset",
            Some(loom_redis::RedisValueKind::Stream) => "stream",
            None => "none",
        };
        self.write_simple(name).await
    }

    async fn redis_hset(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() < 4 || !command.len().is_multiple_of(2) {
            return self
                .write_error("ERR wrong number of arguments for HSET")
                .await;
        }
        let auth = self.current_auth()?.clone();
        let key = command[1].clone();
        let key_is_new = {
            let keyspace = self.lock_keyspace().await;
            keyspace.live_key_kind(&key, now_ms()).is_none()
        };
        if key_is_new {
            self.delete_persisted_key(&auth, &key)?;
        }
        let mut inserted = 0_i64;
        {
            let mut keyspace = self.lock_keyspace().await;
            let mut cursor = 2;
            while cursor < command.len() {
                if keyspace
                    .hset(
                        key.clone(),
                        command[cursor].clone(),
                        command[cursor + 1].clone(),
                        now_ms(),
                    )
                    .map_err(loom_error_to_io)?
                {
                    inserted += 1;
                }
                cursor += 2;
            }
        }
        self.persist_meta(&auth, RedisValueKind::Hash, &key, None)?;
        let mut cursor = 2;
        while cursor < command.len() {
            self.put_persisted_record(
                &auth,
                redis_subrecord_key("hash-field", &key, &command[cursor]),
                command[cursor + 1].clone(),
            )?;
            cursor += 2;
        }
        self.write_integer(inserted).await
    }

    async fn redis_hget(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() != 3 {
            return self
                .write_error("ERR wrong number of arguments for HGET")
                .await;
        }
        let value = {
            let keyspace = self.lock_keyspace().await;
            keyspace
                .hget(&command[1], &command[2], now_ms())
                .map_err(loom_error_to_io)?
                .map(|value| value.to_vec())
        };
        match value {
            Some(value) => self.write_bulk(&value).await,
            None => self.write_null_bulk().await,
        }
    }

    async fn redis_hlen(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() != 2 {
            return self
                .write_error("ERR wrong number of arguments for HLEN")
                .await;
        }
        let len = {
            let keyspace = self.lock_keyspace().await;
            keyspace
                .hlen(&command[1], now_ms())
                .map_err(loom_error_to_io)?
        };
        self.write_integer(len.min(i64::MAX as usize) as i64).await
    }

    async fn redis_sadd(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() < 3 {
            return self
                .write_error("ERR wrong number of arguments for SADD")
                .await;
        }
        let auth = self.current_auth()?.clone();
        let key = command[1].clone();
        let key_is_new = {
            let keyspace = self.lock_keyspace().await;
            keyspace.live_key_kind(&key, now_ms()).is_none()
        };
        if key_is_new {
            self.delete_persisted_key(&auth, &key)?;
        }
        let mut inserted = 0_i64;
        {
            let mut keyspace = self.lock_keyspace().await;
            for member in &command[2..] {
                if keyspace
                    .sadd(key.clone(), member.clone(), now_ms())
                    .map_err(loom_error_to_io)?
                {
                    inserted += 1;
                }
            }
        }
        self.persist_meta(&auth, RedisValueKind::Set, &key, None)?;
        for member in &command[2..] {
            self.put_persisted_record(
                &auth,
                redis_subrecord_key("set-member", &key, member),
                Vec::new(),
            )?;
        }
        self.write_integer(inserted).await
    }

    async fn redis_sismember(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() != 3 {
            return self
                .write_error("ERR wrong number of arguments for SISMEMBER")
                .await;
        }
        let is_member = {
            let keyspace = self.lock_keyspace().await;
            keyspace
                .sismember(&command[1], &command[2], now_ms())
                .map_err(loom_error_to_io)?
        };
        self.write_integer(i64::from(is_member)).await
    }

    async fn redis_scard(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() != 2 {
            return self
                .write_error("ERR wrong number of arguments for SCARD")
                .await;
        }
        let len = {
            let keyspace = self.lock_keyspace().await;
            keyspace
                .scard(&command[1], now_ms())
                .map_err(loom_error_to_io)?
        };
        self.write_integer(len.min(i64::MAX as usize) as i64).await
    }

    async fn redis_push(&mut self, command: &[Vec<u8>], left: bool) -> io::Result<()> {
        if command.len() < 3 {
            return self
                .write_error("ERR wrong number of arguments for list push")
                .await;
        }
        let auth = self.current_auth()?.clone();
        let key = command[1].clone();
        let key_is_new = {
            let keyspace = self.lock_keyspace().await;
            keyspace.live_key_kind(&key, now_ms()).is_none()
        };
        if key_is_new {
            self.delete_persisted_key(&auth, &key)?;
        }
        let mut len = 0_usize;
        let mut nodes = Vec::new();
        {
            let mut keyspace = self.lock_keyspace().await;
            for value in &command[2..] {
                let pushed = if left {
                    keyspace
                        .lpush_indexed(key.clone(), value.clone(), now_ms())
                        .map_err(loom_error_to_io)?
                } else {
                    keyspace
                        .rpush_indexed(key.clone(), value.clone(), now_ms())
                        .map_err(loom_error_to_io)?
                };
                len = pushed.len;
                nodes.push((pushed.index, value.clone()));
            }
        }
        self.persist_meta(&auth, RedisValueKind::List, &key, None)?;
        for (index, value) in nodes {
            self.put_persisted_record(&auth, redis_list_node_key(&key, index), value)?;
        }
        self.write_integer(len.min(i64::MAX as usize) as i64).await
    }

    async fn redis_pop(&mut self, command: &[Vec<u8>], left: bool) -> io::Result<()> {
        if command.len() != 2 {
            return self
                .write_error("ERR wrong number of arguments for list pop")
                .await;
        }
        let auth = self.current_auth()?.clone();
        let popped = {
            let mut keyspace = self.lock_keyspace().await;
            if left {
                keyspace
                    .lpop_indexed(&command[1], now_ms())
                    .map_err(loom_error_to_io)?
            } else {
                keyspace
                    .rpop_indexed(&command[1], now_ms())
                    .map_err(loom_error_to_io)?
            }
        };
        let Some(popped) = popped else {
            return self.write_null_bulk().await;
        };
        if popped.empty_after {
            self.delete_persisted_key(&auth, &command[1])?;
        } else {
            self.delete_persisted_record(&auth, &redis_list_node_key(&command[1], popped.index))?;
        }
        self.write_bulk(&popped.value).await
    }

    async fn redis_llen(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() != 2 {
            return self
                .write_error("ERR wrong number of arguments for LLEN")
                .await;
        }
        let len = {
            let keyspace = self.lock_keyspace().await;
            keyspace
                .llen(&command[1], now_ms())
                .map_err(loom_error_to_io)?
        };
        self.write_integer(len.min(i64::MAX as usize) as i64).await
    }

    async fn redis_zadd(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() < 4 || !command.len().is_multiple_of(2) {
            return self
                .write_error("ERR wrong number of arguments for ZADD")
                .await;
        }
        let auth = self.current_auth()?.clone();
        let key = command[1].clone();
        let key_is_new = {
            let keyspace = self.lock_keyspace().await;
            keyspace.live_key_kind(&key, now_ms()).is_none()
        };
        if key_is_new {
            self.delete_persisted_key(&auth, &key)?;
        }
        let mut inserted = 0_i64;
        let mut entries = Vec::new();
        {
            let mut keyspace = self.lock_keyspace().await;
            let mut cursor = 2;
            while cursor < command.len() {
                let score = parse_f64(&command[cursor])?;
                let member = command[cursor + 1].clone();
                if keyspace
                    .zadd(key.clone(), score, member.clone(), now_ms())
                    .map_err(loom_error_to_io)?
                {
                    inserted += 1;
                }
                entries.push((member, score));
                cursor += 2;
            }
        }
        self.persist_meta(&auth, RedisValueKind::SortedSet, &key, None)?;
        for (member, score) in entries {
            self.put_persisted_record(
                &auth,
                redis_zset_member_key(&key, &member),
                encode_f64(score),
            )?;
        }
        self.write_integer(inserted).await
    }

    async fn redis_zscore(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() != 3 {
            return self
                .write_error("ERR wrong number of arguments for ZSCORE")
                .await;
        }
        let score = {
            let keyspace = self.lock_keyspace().await;
            keyspace
                .zscore(&command[1], &command[2], now_ms())
                .map_err(loom_error_to_io)?
        };
        match score {
            Some(score) => self.write_bulk(format_redis_score(score).as_bytes()).await,
            None => self.write_null_bulk().await,
        }
    }

    async fn redis_zcard(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() != 2 {
            return self
                .write_error("ERR wrong number of arguments for ZCARD")
                .await;
        }
        let len = {
            let keyspace = self.lock_keyspace().await;
            keyspace
                .zcard(&command[1], now_ms())
                .map_err(loom_error_to_io)?
        };
        self.write_integer(len.min(i64::MAX as usize) as i64).await
    }

    async fn redis_xadd(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() < 5 || command.len().is_multiple_of(2) {
            return self
                .write_error("ERR wrong number of arguments for XADD")
                .await;
        }
        let auth = self.current_auth()?.clone();
        let key = command[1].clone();
        let id_spec = bytes_to_str(&command[2])?;
        let fields = command[3..]
            .chunks_exact(2)
            .map(|pair| (pair[0].clone(), pair[1].clone()))
            .collect::<Vec<_>>();
        let is_new = self.ensure_stream_key(&auth, &key).await?;
        if is_new {
            self.delete_persisted_key(&auth, &key)?;
        }
        let log = self.stream_log(&auth, &key)?;
        let last_id = log.last_add_id();
        let id = RedisStreamId::for_xadd(id_spec, last_id, now_ms())?;
        let record = RedisStreamRecord::Add {
            id,
            fields: fields.clone(),
        };
        self.append_stream_record(&auth, &key, &record)?;
        self.write_bulk(id.to_string().as_bytes()).await
    }

    async fn redis_xlen(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() != 2 {
            return self
                .write_error("ERR wrong number of arguments for XLEN")
                .await;
        }
        self.ensure_stream_readable(&command[1]).await?;
        let auth = self.current_auth()?.clone();
        let len = self.stream_log(&auth, &command[1])?.entries.len();
        self.write_integer(len.min(i64::MAX as usize) as i64).await
    }

    async fn redis_xrange(&mut self, command: &[Vec<u8>], reverse: bool) -> io::Result<()> {
        if command.len() < 4 {
            return self
                .write_error("ERR wrong number of arguments for XRANGE")
                .await;
        }
        let mut count = None;
        let mut cursor = 4;
        while cursor < command.len() {
            match bytes_to_str(&command[cursor])?
                .to_ascii_uppercase()
                .as_str()
            {
                "COUNT" => {
                    let Some(raw) = command.get(cursor + 1) else {
                        return self.write_error("ERR missing COUNT value").await;
                    };
                    count = Some(parse_u64(raw)?.min(usize::MAX as u64) as usize);
                    cursor += 2;
                }
                _ => return self.write_error("ERR unsupported XRANGE option").await,
            }
        }
        self.ensure_stream_readable(&command[1]).await?;
        let auth = self.current_auth()?.clone();
        let log = self.stream_log(&auth, &command[1])?;
        let lower = if reverse {
            RedisRangeBound::parse_lower(&command[3])?
        } else {
            RedisRangeBound::parse_lower(&command[2])?
        };
        let upper = if reverse {
            RedisRangeBound::parse_upper(&command[2])?
        } else {
            RedisRangeBound::parse_upper(&command[3])?
        };
        let mut entries = log
            .entries
            .into_iter()
            .filter(|entry| lower.includes_lower(entry.id) && upper.includes_upper(entry.id))
            .collect::<Vec<_>>();
        if reverse {
            entries.reverse();
        }
        if let Some(count) = count {
            entries.truncate(count);
        }
        self.write_stream_entries(&entries).await
    }

    async fn redis_xread(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() < 4 {
            return self
                .write_error("ERR wrong number of arguments for XREAD")
                .await;
        }
        let mut count = None;
        let mut cursor = 1;
        while cursor < command.len() {
            match bytes_to_str(&command[cursor])?
                .to_ascii_uppercase()
                .as_str()
            {
                "COUNT" => {
                    let Some(raw) = command.get(cursor + 1) else {
                        return self.write_error("ERR missing COUNT value").await;
                    };
                    count = Some(parse_u64(raw)?.min(usize::MAX as u64) as usize);
                    cursor += 2;
                }
                "STREAMS" => {
                    cursor += 1;
                    break;
                }
                "BLOCK" => return self.write_error("ERR unsupported XREAD option").await,
                _ => return self.write_error("ERR unsupported XREAD option").await,
            }
        }
        if cursor >= command.len() {
            return self.write_error("ERR missing STREAMS arguments").await;
        }
        let remaining = command.len() - cursor;
        if remaining == 0 || !remaining.is_multiple_of(2) {
            return self.write_error("ERR invalid STREAMS arguments").await;
        }
        let stream_count = remaining / 2;
        let keys = &command[cursor..cursor + stream_count];
        let ids = &command[cursor + stream_count..];
        let auth = self.current_auth()?.clone();
        let mut responses = Vec::new();
        for (key, raw_id) in keys.iter().zip(ids.iter()) {
            self.ensure_stream_readable(key).await?;
            let log = self.stream_log(&auth, key)?;
            let after = if raw_id.as_slice() == b"$" {
                log.last_add_id()
            } else {
                Some(RedisStreamId::parse(raw_id)?)
            };
            let mut entries = log
                .entries
                .into_iter()
                .filter(|entry| after.is_none_or(|after| entry.id > after))
                .collect::<Vec<_>>();
            if let Some(count) = count {
                entries.truncate(count);
            }
            if !entries.is_empty() {
                responses.push((key.clone(), entries));
            }
        }
        if responses.is_empty() {
            return self.write_null_array().await;
        }
        self.write_array_len(responses.len()).await?;
        for (key, entries) in responses {
            self.write_array_len(2).await?;
            self.write_bulk(&key).await?;
            self.write_stream_entries(&entries).await?;
        }
        Ok(())
    }

    async fn redis_xdel(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() < 3 {
            return self
                .write_error("ERR wrong number of arguments for XDEL")
                .await;
        }
        self.ensure_stream_readable(&command[1]).await?;
        let auth = self.current_auth()?.clone();
        let log = self.stream_log(&auth, &command[1])?;
        let mut removed = 0_i64;
        let mut tombstones = Vec::new();
        for raw_id in &command[2..] {
            let id = RedisStreamId::parse(raw_id)?;
            if log.entries.iter().any(|entry| entry.id == id) {
                removed += 1;
                tombstones.push(id);
            }
        }
        for id in tombstones {
            self.append_stream_record(&auth, &command[1], &RedisStreamRecord::Delete { id })?;
        }
        self.write_integer(removed).await
    }

    async fn redis_publish(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() != 3 {
            return self
                .write_error("ERR wrong number of arguments for PUBLISH")
                .await;
        }
        let delivered = {
            let state = self.state.pubsub.lock().await;
            let delivered = state.delivery_count(&command[1]);
            let _ = state.sender.send(RedisPubSubMessage {
                channel: command[1].clone(),
                payload: command[2].clone(),
            });
            delivered
        };
        self.write_integer(delivered.min(i64::MAX as usize) as i64)
            .await
    }

    async fn redis_pubsub(&mut self, command: &[Vec<u8>]) -> io::Result<()> {
        if command.len() < 2 {
            return self
                .write_error("ERR wrong number of arguments for PUBSUB")
                .await;
        }
        match bytes_to_str(&command[1])?.to_ascii_uppercase().as_str() {
            "NUMSUB" => {
                let pairs = {
                    let state = self.state.pubsub.lock().await;
                    command[2..]
                        .iter()
                        .map(|channel| {
                            (
                                channel.clone(),
                                state.channels.get(channel).copied().unwrap_or_default(),
                            )
                        })
                        .collect::<Vec<_>>()
                };
                self.write_array_len(pairs.len() * 2).await?;
                for (channel, count) in pairs {
                    self.write_bulk(&channel).await?;
                    self.write_integer(count.min(i64::MAX as usize) as i64)
                        .await?;
                }
                Ok(())
            }
            "CHANNELS" => {
                let channels = {
                    let state = self.state.pubsub.lock().await;
                    state.channels.keys().cloned().collect::<Vec<_>>()
                };
                self.write_array_len(channels.len()).await?;
                for channel in channels {
                    self.write_bulk(&channel).await?;
                }
                Ok(())
            }
            "NUMPAT" => {
                let count = {
                    let state = self.state.pubsub.lock().await;
                    state.patterns.values().copied().sum::<usize>()
                };
                self.write_integer(count.min(i64::MAX as usize) as i64)
                    .await
            }
            _ => self.write_error("ERR unsupported PUBSUB subcommand").await,
        }
    }

    async fn redis_subscribe(&mut self, command: &[Vec<u8>], pattern_mode: bool) -> io::Result<()> {
        if command.len() < 2 {
            return self
                .write_error("ERR wrong number of arguments for SUBSCRIBE")
                .await;
        }
        let mut receiver = {
            let mut state = self.state.pubsub.lock().await;
            let receiver = state.sender.subscribe();
            for topic in &command[1..] {
                state.subscribe(topic, pattern_mode);
            }
            receiver
        };
        let mut channels = BTreeSet::new();
        let mut patterns = BTreeSet::new();
        for topic in &command[1..] {
            if pattern_mode {
                patterns.insert(topic.clone());
            } else {
                channels.insert(topic.clone());
            }
            self.write_pubsub_subscription_ack(
                true,
                pattern_mode,
                topic,
                channels.len() + patterns.len(),
            )
            .await?;
        }
        loop {
            tokio::select! {
                message = receiver.recv() => {
                    match message {
                        Ok(message) => {
                            if channels.contains(&message.channel) {
                                self.write_pubsub_message(&message).await?;
                            }
                            for pattern in &patterns {
                                if redis_pattern_matches(pattern, &message.channel) {
                                    self.write_pubsub_pattern_message(pattern, &message).await?;
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {}
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                command = read_resp_array(&mut self.stream) => {
                    let Some(command) = command? else {
                        break;
                    };
                    let Some(name) = command.first().and_then(|value| std::str::from_utf8(value).ok()) else {
                        self.write_error("ERR empty command").await?;
                        continue;
                    };
                    match name.to_ascii_uppercase().as_str() {
                        "SUBSCRIBE" => self.update_subscriptions(&command[1..], false, true, &mut channels, &mut patterns).await?,
                        "PSUBSCRIBE" => self.update_subscriptions(&command[1..], true, true, &mut channels, &mut patterns).await?,
                        "UNSUBSCRIBE" => self.update_subscriptions(&command[1..], false, false, &mut channels, &mut patterns).await?,
                        "PUNSUBSCRIBE" => self.update_subscriptions(&command[1..], true, false, &mut channels, &mut patterns).await?,
                        "PING" => self.handle_ping(&command).await?,
                        "QUIT" => break,
                        _ => self.write_error("ERR only pubsub commands are allowed in subscribed mode").await?,
                    }
                    if channels.is_empty() && patterns.is_empty() {
                        break;
                    }
                }
            }
        }
        self.clear_subscriptions(&channels, &patterns).await;
        Ok(())
    }

    async fn update_subscriptions(
        &mut self,
        topics: &[Vec<u8>],
        pattern_mode: bool,
        subscribe: bool,
        channels: &mut BTreeSet<Vec<u8>>,
        patterns: &mut BTreeSet<Vec<u8>>,
    ) -> io::Result<()> {
        let selected = if topics.is_empty() {
            if pattern_mode {
                patterns.iter().cloned().collect::<Vec<_>>()
            } else {
                channels.iter().cloned().collect::<Vec<_>>()
            }
        } else {
            topics.to_vec()
        };
        for topic in selected {
            let changed = if pattern_mode {
                if subscribe {
                    patterns.insert(topic.clone())
                } else {
                    patterns.remove(&topic)
                }
            } else if subscribe {
                channels.insert(topic.clone())
            } else {
                channels.remove(&topic)
            };
            if changed {
                let mut state = self.state.pubsub.lock().await;
                if subscribe {
                    state.subscribe(&topic, pattern_mode);
                } else {
                    state.unsubscribe(&topic, pattern_mode);
                }
            }
            self.write_pubsub_subscription_ack(
                subscribe,
                pattern_mode,
                &topic,
                channels.len() + patterns.len(),
            )
            .await?;
        }
        Ok(())
    }

    async fn clear_subscriptions(
        &self,
        channels: &BTreeSet<Vec<u8>>,
        patterns: &BTreeSet<Vec<u8>>,
    ) {
        let mut state = self.state.pubsub.lock().await;
        for channel in channels {
            state.unsubscribe(channel, false);
        }
        for pattern in patterns {
            state.unsubscribe(pattern, true);
        }
    }

    async fn require_auth(&mut self) -> io::Result<bool> {
        if self.auth.is_some() {
            Ok(true)
        } else {
            self.write_error("NOAUTH Authentication required").await?;
            Ok(false)
        }
    }

    async fn lock_keyspace(&self) -> tokio::sync::MutexGuard<'_, RedisKeyspace> {
        self.state.keyspace.lock().await
    }

    fn current_auth(&self) -> io::Result<&HostedAuth> {
        self.auth.as_ref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Redis command requires authentication",
            )
        })
    }

    async fn ensure_loaded(&self, auth: &HostedAuth) -> io::Result<()> {
        {
            let loaded = self.state.loaded.lock().await;
            if *loaded {
                return Ok(());
            }
        }
        let loaded = self.load_persisted_keyspace(auth)?;
        {
            let mut keyspace = self.lock_keyspace().await;
            *keyspace = loaded;
        }
        let mut loaded = self.state.loaded.lock().await;
        *loaded = true;
        Ok(())
    }

    fn load_persisted_keyspace(&self, auth: &HostedAuth) -> io::Result<RedisKeyspace> {
        let entries = match self.state.kernel.data().kv_list(
            auth,
            &self.state.workspace,
            &self.state.keyspace_name,
        ) {
            Ok(entries) => entries,
            Err(err) if err.code == loom_core::Code::NotFound => Vec::new(),
            Err(err) => return Err(hosted_error_to_io(err)),
        };
        let mut metas = BTreeMap::<Vec<u8>, (RedisValueKind, Option<u64>)>::new();
        let mut strings = BTreeMap::<Vec<u8>, Vec<u8>>::new();
        let mut hash_fields = BTreeMap::<Vec<u8>, Vec<(Vec<u8>, Vec<u8>)>>::new();
        let mut set_members = BTreeMap::<Vec<u8>, Vec<Vec<u8>>>::new();
        let mut list_nodes = BTreeMap::<Vec<u8>, Vec<(i64, Vec<u8>)>>::new();
        let mut zset_members = BTreeMap::<Vec<u8>, Vec<(Vec<u8>, f64)>>::new();
        for entry in entries {
            let key = key_from_cbor(&entry.key_cbor).map_err(loom_error_to_io)?;
            match redis_record_key_parts(&key) {
                Some(PersistedRedisRecordKey::Meta(redis_key)) => {
                    if let Some(meta) = decode_meta(&entry.value)? {
                        metas.insert(redis_key, meta);
                    }
                }
                Some(PersistedRedisRecordKey::String(redis_key)) => {
                    strings.insert(redis_key, entry.value);
                }
                Some(PersistedRedisRecordKey::HashField { redis_key, field }) => {
                    hash_fields
                        .entry(redis_key)
                        .or_default()
                        .push((field, entry.value));
                }
                Some(PersistedRedisRecordKey::SetMember { redis_key, member }) => {
                    set_members.entry(redis_key).or_default().push(member);
                }
                Some(PersistedRedisRecordKey::ListNode { redis_key, index }) => {
                    list_nodes
                        .entry(redis_key)
                        .or_default()
                        .push((index, entry.value));
                }
                Some(PersistedRedisRecordKey::ZsetMember { redis_key, member }) => {
                    zset_members
                        .entry(redis_key)
                        .or_default()
                        .push((member, decode_f64(&entry.value)?));
                }
                _ => {}
            }
        }
        let mut keyspace = RedisKeyspace::new(RedisPersistenceMode::Versioned);
        for (key, (kind, expires_at_ms)) in metas {
            match kind {
                RedisValueKind::String => {
                    if let Some(value) = strings.remove(&key) {
                        keyspace.set_string(key, value, expires_at_ms);
                    }
                }
                RedisValueKind::Hash => {
                    if let Some(fields) = hash_fields.remove(&key) {
                        for (field, value) in fields {
                            keyspace
                                .hset(key.clone(), field, value, now_ms())
                                .map_err(loom_error_to_io)?;
                        }
                        if let Some(expires_at_ms) = expires_at_ms {
                            keyspace.expire_at(&key, expires_at_ms, now_ms());
                        }
                    }
                }
                RedisValueKind::Set => {
                    if let Some(members) = set_members.remove(&key) {
                        for member in members {
                            keyspace
                                .sadd(key.clone(), member, now_ms())
                                .map_err(loom_error_to_io)?;
                        }
                        if let Some(expires_at_ms) = expires_at_ms {
                            keyspace.expire_at(&key, expires_at_ms, now_ms());
                        }
                    }
                }
                RedisValueKind::List => {
                    if let Some(nodes) = list_nodes.remove(&key) {
                        for (index, value) in nodes {
                            keyspace
                                .put_list_node(key.clone(), index, value, now_ms())
                                .map_err(loom_error_to_io)?;
                        }
                        if let Some(expires_at_ms) = expires_at_ms {
                            keyspace.expire_at(&key, expires_at_ms, now_ms());
                        }
                    }
                }
                RedisValueKind::SortedSet => {
                    if let Some(members) = zset_members.remove(&key) {
                        for (member, score) in members {
                            keyspace
                                .zadd(key.clone(), score, member, now_ms())
                                .map_err(loom_error_to_io)?;
                        }
                        if let Some(expires_at_ms) = expires_at_ms {
                            keyspace.expire_at(&key, expires_at_ms, now_ms());
                        }
                    }
                }
                RedisValueKind::Stream => {
                    keyspace
                        .ensure_stream(key.clone(), now_ms())
                        .map_err(loom_error_to_io)?;
                    if let Some(expires_at_ms) = expires_at_ms {
                        keyspace.expire_at(&key, expires_at_ms, now_ms());
                    }
                }
            }
        }
        Ok(keyspace)
    }

    fn persist_string(
        &self,
        auth: &HostedAuth,
        key: &[u8],
        value: &[u8],
        expires_at_ms: Option<u64>,
    ) -> io::Result<()> {
        self.put_persisted_record(
            auth,
            redis_record_key("meta", key),
            encode_meta(RedisValueKind::String, expires_at_ms),
        )?;
        self.put_persisted_record(auth, redis_record_key("string", key), value.to_vec())
    }

    fn persist_existing_string_meta(&self, auth: &HostedAuth, key: &[u8]) -> io::Result<()> {
        let (kind, expires_at_ms) = {
            let keyspace = self
                .state
                .keyspace
                .try_lock()
                .map_err(|_| io::Error::other("Redis keyspace is busy"))?;
            let Some(kind) = keyspace.live_key_kind(key, now_ms()) else {
                return Ok(());
            };
            let expires_at_ms = match keyspace.ttl_ms(key, now_ms()) {
                RedisTtl::NoKey => return Ok(()),
                RedisTtl::Persistent => None,
                RedisTtl::RemainingMs(ms) => Some(now_ms().saturating_add(ms)),
            };
            (kind, expires_at_ms)
        };
        self.persist_meta(auth, kind, key, expires_at_ms)
    }

    fn persist_meta(
        &self,
        auth: &HostedAuth,
        kind: RedisValueKind,
        key: &[u8],
        expires_at_ms: Option<u64>,
    ) -> io::Result<()> {
        self.put_persisted_record(
            auth,
            redis_record_key("meta", key),
            encode_meta(kind, expires_at_ms),
        )
    }

    fn put_persisted_record(
        &self,
        auth: &HostedAuth,
        key: Value,
        value: Vec<u8>,
    ) -> io::Result<()> {
        self.state
            .kernel
            .data()
            .kv_put(
                auth,
                &self.state.workspace,
                &self.state.keyspace_name,
                &key_to_cbor(&key),
                value,
            )
            .map_err(hosted_error_to_io)
    }

    fn delete_persisted_record(&self, auth: &HostedAuth, key: &Value) -> io::Result<()> {
        self.state
            .kernel
            .data()
            .kv_delete(
                auth,
                &self.state.workspace,
                &self.state.keyspace_name,
                &key_to_cbor(key),
            )
            .map(|_| ())
            .map_err(hosted_error_to_io)
    }

    fn delete_persisted_key(&self, auth: &HostedAuth, key: &[u8]) -> io::Result<()> {
        let entries = match self.state.kernel.data().kv_list(
            auth,
            &self.state.workspace,
            &self.state.keyspace_name,
        ) {
            Ok(entries) => entries,
            Err(err) if err.code == loom_core::Code::NotFound => Vec::new(),
            Err(err) => return Err(hosted_error_to_io(err)),
        };
        for entry in entries {
            let record_key = key_from_cbor(&entry.key_cbor).map_err(loom_error_to_io)?;
            let Some(parts) = redis_record_key_parts(&record_key) else {
                continue;
            };
            if parts.redis_key() != key {
                continue;
            }
            self.state
                .kernel
                .data()
                .kv_delete(
                    auth,
                    &self.state.workspace,
                    &self.state.keyspace_name,
                    &entry.key_cbor,
                )
                .map_err(hosted_error_to_io)?;
        }
        Ok(())
    }

    async fn ensure_stream_key(&self, auth: &HostedAuth, key: &[u8]) -> io::Result<bool> {
        let created = {
            let mut keyspace = self.lock_keyspace().await;
            keyspace
                .ensure_stream(key.to_vec(), now_ms())
                .map_err(loom_error_to_io)?
        };
        if created {
            self.persist_meta(auth, RedisValueKind::Stream, key, None)?;
        }
        Ok(created)
    }

    async fn ensure_stream_readable(&self, key: &[u8]) -> io::Result<()> {
        let kind = {
            let keyspace = self.lock_keyspace().await;
            keyspace.live_key_kind(key, now_ms())
        };
        match kind {
            Some(RedisValueKind::Stream) | None => Ok(()),
            Some(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "WRONGTYPE Operation against a key holding the wrong kind of value",
            )),
        }
    }

    fn append_stream_record(
        &self,
        auth: &HostedAuth,
        key: &[u8],
        record: &RedisStreamRecord,
    ) -> io::Result<usize> {
        self.state
            .kernel
            .data()
            .queue_append(
                auth,
                &self.state.workspace,
                &redis_stream_name(key),
                &record.to_cbor(),
            )
            .map_err(hosted_error_to_io)
    }

    fn stream_log(&self, auth: &HostedAuth, key: &[u8]) -> io::Result<RedisStreamLog> {
        let len = match self.state.kernel.data().queue_len(
            auth,
            &self.state.workspace,
            &redis_stream_name(key),
        ) {
            Ok(len) => len,
            Err(err) if err.code == loom_core::Code::NotFound => return Ok(RedisStreamLog::new()),
            Err(err) => return Err(hosted_error_to_io(err)),
        };
        let entries = if len == 0 {
            Vec::new()
        } else {
            self.state
                .kernel
                .data()
                .queue_range(auth, &self.state.workspace, &redis_stream_name(key), 0, len)
                .map_err(hosted_error_to_io)?
        };
        let mut log = RedisStreamLog::new();
        for entry in entries {
            log.apply(RedisStreamRecord::from_cbor(&entry.payload)?);
        }
        Ok(log)
    }

    fn delete_stream_queue(&self, auth: &HostedAuth, key: &[u8]) -> io::Result<()> {
        self.state
            .kernel
            .data()
            .queue_delete(auth, &self.state.workspace, &redis_stream_name(key))
            .map(|_| ())
            .map_err(hosted_error_to_io)
    }

    async fn write_simple(&mut self, value: &str) -> io::Result<()> {
        self.stream.write_all(b"+").await?;
        self.stream.write_all(value.as_bytes()).await?;
        self.stream.write_all(b"\r\n").await
    }

    async fn write_error(&mut self, value: &str) -> io::Result<()> {
        self.stream.write_all(b"-").await?;
        self.stream.write_all(value.as_bytes()).await?;
        self.stream.write_all(b"\r\n").await
    }

    async fn write_integer(&mut self, value: i64) -> io::Result<()> {
        self.stream.write_all(b":").await?;
        self.stream.write_all(value.to_string().as_bytes()).await?;
        self.stream.write_all(b"\r\n").await
    }

    async fn write_array_len(&mut self, len: usize) -> io::Result<()> {
        self.stream.write_all(b"*").await?;
        self.stream.write_all(len.to_string().as_bytes()).await?;
        self.stream.write_all(b"\r\n").await
    }

    async fn write_bulk(&mut self, value: &[u8]) -> io::Result<()> {
        self.stream.write_all(b"$").await?;
        self.stream
            .write_all(value.len().to_string().as_bytes())
            .await?;
        self.stream.write_all(b"\r\n").await?;
        self.stream.write_all(value).await?;
        self.stream.write_all(b"\r\n").await
    }

    async fn write_null_bulk(&mut self) -> io::Result<()> {
        self.stream.write_all(b"$-1\r\n").await
    }

    async fn write_null_array(&mut self) -> io::Result<()> {
        self.stream.write_all(b"*-1\r\n").await
    }

    async fn write_stream_entries(&mut self, entries: &[RedisStreamEntry]) -> io::Result<()> {
        self.write_array_len(entries.len()).await?;
        for entry in entries {
            self.write_array_len(2).await?;
            self.write_bulk(entry.id.to_string().as_bytes()).await?;
            self.write_array_len(entry.fields.len() * 2).await?;
            for (field, value) in &entry.fields {
                self.write_bulk(field).await?;
                self.write_bulk(value).await?;
            }
        }
        Ok(())
    }

    async fn write_pubsub_subscription_ack(
        &mut self,
        subscribe: bool,
        pattern_mode: bool,
        topic: &[u8],
        count: usize,
    ) -> io::Result<()> {
        self.write_array_len(3).await?;
        let kind = match (subscribe, pattern_mode) {
            (true, true) => b"psubscribe".as_slice(),
            (true, false) => b"subscribe".as_slice(),
            (false, true) => b"punsubscribe".as_slice(),
            (false, false) => b"unsubscribe".as_slice(),
        };
        self.write_bulk(kind).await?;
        self.write_bulk(topic).await?;
        self.write_integer(count.min(i64::MAX as usize) as i64)
            .await
    }

    async fn write_pubsub_message(&mut self, message: &RedisPubSubMessage) -> io::Result<()> {
        self.write_array_len(3).await?;
        self.write_bulk(b"message").await?;
        self.write_bulk(&message.channel).await?;
        self.write_bulk(&message.payload).await
    }

    async fn write_pubsub_pattern_message(
        &mut self,
        pattern: &[u8],
        message: &RedisPubSubMessage,
    ) -> io::Result<()> {
        self.write_array_len(4).await?;
        self.write_bulk(b"pmessage").await?;
        self.write_bulk(pattern).await?;
        self.write_bulk(&message.channel).await?;
        self.write_bulk(&message.payload).await
    }
}

#[derive(Clone, Debug)]
struct RedisPubSubMessage {
    channel: Vec<u8>,
    payload: Vec<u8>,
}

struct RedisPubSubState {
    sender: broadcast::Sender<RedisPubSubMessage>,
    channels: BTreeMap<Vec<u8>, usize>,
    patterns: BTreeMap<Vec<u8>, usize>,
}

impl RedisPubSubState {
    fn new() -> Self {
        let (sender, _) = broadcast::channel(1024);
        Self {
            sender,
            channels: BTreeMap::new(),
            patterns: BTreeMap::new(),
        }
    }

    fn subscribe(&mut self, topic: &[u8], pattern_mode: bool) {
        let counts = if pattern_mode {
            &mut self.patterns
        } else {
            &mut self.channels
        };
        *counts.entry(topic.to_vec()).or_default() += 1;
    }

    fn unsubscribe(&mut self, topic: &[u8], pattern_mode: bool) {
        let counts = if pattern_mode {
            &mut self.patterns
        } else {
            &mut self.channels
        };
        let Some(count) = counts.get_mut(topic) else {
            return;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            counts.remove(topic);
        }
    }

    fn delivery_count(&self, channel: &[u8]) -> usize {
        let direct = self.channels.get(channel).copied().unwrap_or_default();
        let patterns = self
            .patterns
            .iter()
            .filter(|(pattern, _)| redis_pattern_matches(pattern, channel))
            .map(|(_, count)| *count)
            .sum::<usize>();
        direct + patterns
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RedisStreamId {
    ms: u64,
    seq: u64,
}

impl RedisStreamId {
    fn parse(bytes: &[u8]) -> io::Result<Self> {
        let raw = bytes_to_str(bytes)?;
        let Some((ms, seq)) = raw.split_once('-') else {
            let ms = raw.parse::<u64>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "ERR invalid stream ID")
            })?;
            return Ok(Self { ms, seq: 0 });
        };
        Ok(Self {
            ms: ms.parse::<u64>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "ERR invalid stream ID")
            })?,
            seq: seq.parse::<u64>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "ERR invalid stream ID")
            })?,
        })
    }

    fn for_xadd(spec: &str, last: Option<Self>, now_ms: u64) -> io::Result<Self> {
        let id = if spec == "*" {
            Self::next_auto(now_ms, last)
        } else if let Some((ms, seq)) = spec.split_once('-') {
            let ms = ms.parse::<u64>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "ERR invalid stream ID")
            })?;
            let seq = if seq == "*" {
                last.filter(|last| last.ms == ms)
                    .map(|last| last.seq.saturating_add(1))
                    .unwrap_or_default()
            } else {
                seq.parse::<u64>().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "ERR invalid stream ID")
                })?
            };
            Self { ms, seq }
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ERR invalid stream ID",
            ));
        };
        if id.ms == 0 && id.seq == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ERR stream ID must be greater than 0-0",
            ));
        }
        if last.is_some_and(|last| id <= last) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ERR stream ID must be greater than the last generated ID",
            ));
        }
        Ok(id)
    }

    fn next_auto(now_ms: u64, last: Option<Self>) -> Self {
        match last {
            Some(last) if last.ms >= now_ms => Self {
                ms: last.ms,
                seq: last.seq.saturating_add(1),
            },
            _ => Self { ms: now_ms, seq: 0 },
        }
    }
}

impl Ord for RedisStreamId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.ms.cmp(&other.ms).then(self.seq.cmp(&other.seq))
    }
}

impl PartialOrd for RedisStreamId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for RedisStreamId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.ms, self.seq)
    }
}

#[derive(Clone, Debug)]
struct RedisStreamEntry {
    id: RedisStreamId,
    fields: Vec<(Vec<u8>, Vec<u8>)>,
}

enum RedisRangeBound {
    NegInf,
    PosInf,
    Id(RedisStreamId),
}

impl RedisRangeBound {
    fn parse_lower(bytes: &[u8]) -> io::Result<Self> {
        match bytes {
            b"-" => Ok(Self::NegInf),
            b"+" => Ok(Self::PosInf),
            _ => RedisStreamId::parse(bytes).map(Self::Id),
        }
    }

    fn parse_upper(bytes: &[u8]) -> io::Result<Self> {
        match bytes {
            b"-" => Ok(Self::NegInf),
            b"+" => Ok(Self::PosInf),
            _ => RedisStreamId::parse(bytes).map(Self::Id),
        }
    }

    fn includes_lower(&self, id: RedisStreamId) -> bool {
        match self {
            Self::NegInf => true,
            Self::PosInf => false,
            Self::Id(bound) => id >= *bound,
        }
    }

    fn includes_upper(&self, id: RedisStreamId) -> bool {
        match self {
            Self::NegInf => false,
            Self::PosInf => true,
            Self::Id(bound) => id <= *bound,
        }
    }
}

struct RedisStreamLog {
    entries: Vec<RedisStreamEntry>,
    last_add_id: Option<RedisStreamId>,
}

impl RedisStreamLog {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            last_add_id: None,
        }
    }

    fn apply(&mut self, record: RedisStreamRecord) {
        match record {
            RedisStreamRecord::Add { id, fields } => {
                self.last_add_id = Some(id);
                self.entries.push(RedisStreamEntry { id, fields });
            }
            RedisStreamRecord::Delete { id } => {
                self.entries.retain(|entry| entry.id != id);
            }
        }
    }

    fn last_add_id(&self) -> Option<RedisStreamId> {
        self.last_add_id
    }
}

enum RedisStreamRecord {
    Add {
        id: RedisStreamId,
        fields: Vec<(Vec<u8>, Vec<u8>)>,
    },
    Delete {
        id: RedisStreamId,
    },
}

impl RedisStreamRecord {
    fn to_cbor(&self) -> Vec<u8> {
        let value = match self {
            Self::Add { id, fields } => CborValue::Array(vec![
                CborValue::Text("redis-stream-v1".to_string()),
                CborValue::Text("add".to_string()),
                CborValue::Uint(id.ms),
                CborValue::Uint(id.seq),
                CborValue::Array(
                    fields
                        .iter()
                        .map(|(field, value)| {
                            CborValue::Array(vec![
                                CborValue::Bytes(field.clone()),
                                CborValue::Bytes(value.clone()),
                            ])
                        })
                        .collect(),
                ),
            ]),
            Self::Delete { id } => CborValue::Array(vec![
                CborValue::Text("redis-stream-v1".to_string()),
                CborValue::Text("del".to_string()),
                CborValue::Uint(id.ms),
                CborValue::Uint(id.seq),
            ]),
        };
        loom_codec::encode(&value).expect("Redis stream records use finite canonical values")
    }

    fn from_cbor(bytes: &[u8]) -> io::Result<Self> {
        let value = loom_codec::decode(bytes).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid Redis stream record")
        })?;
        let CborValue::Array(parts) = value else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid Redis stream record",
            ));
        };
        match parts.as_slice() {
            [
                CborValue::Text(prefix),
                CborValue::Text(kind),
                CborValue::Uint(ms),
                CborValue::Uint(seq),
                CborValue::Array(fields),
            ] if prefix == "redis-stream-v1" && kind == "add" => {
                let mut decoded = Vec::with_capacity(fields.len());
                for field in fields {
                    let CborValue::Array(pair) = field else {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid Redis stream field",
                        ));
                    };
                    match pair.as_slice() {
                        [CborValue::Bytes(name), CborValue::Bytes(value)] => {
                            decoded.push((name.clone(), value.clone()));
                        }
                        _ => {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "invalid Redis stream field",
                            ));
                        }
                    }
                }
                Ok(Self::Add {
                    id: RedisStreamId { ms: *ms, seq: *seq },
                    fields: decoded,
                })
            }
            [
                CborValue::Text(prefix),
                CborValue::Text(kind),
                CborValue::Uint(ms),
                CborValue::Uint(seq),
            ] if prefix == "redis-stream-v1" && kind == "del" => Ok(Self::Delete {
                id: RedisStreamId { ms: *ms, seq: *seq },
            }),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid Redis stream record",
            )),
        }
    }
}

async fn read_resp_array<S>(stream: &mut S) -> io::Result<Option<Vec<Vec<u8>>>>
where
    S: AsyncRead + Unpin,
{
    let Some(prefix) = read_byte(stream).await? else {
        return Ok(None);
    };
    if prefix != b'*' {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "RESP command must be an array",
        ));
    }
    let count = read_decimal_line(stream).await?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let Some(prefix) = read_byte(stream).await? else {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "unexpected EOF in RESP array",
            ));
        };
        if prefix != b'$' {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "RESP command arguments must be bulk strings",
            ));
        }
        let len = read_decimal_line(stream).await?;
        let mut value = vec![0; len];
        stream.read_exact(&mut value).await?;
        expect_crlf(stream).await?;
        out.push(value);
    }
    Ok(Some(out))
}

async fn read_byte<S>(stream: &mut S) -> io::Result<Option<u8>>
where
    S: AsyncRead + Unpin,
{
    let mut byte = [0];
    match stream.read_exact(&mut byte).await {
        Ok(_) => Ok(Some(byte[0])),
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => Ok(None),
        Err(err) => Err(err),
    }
}

async fn read_decimal_line<S>(stream: &mut S) -> io::Result<usize>
where
    S: AsyncRead + Unpin,
{
    let line = read_line(stream).await?;
    let value = std::str::from_utf8(&line)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "RESP length is not UTF-8"))?;
    value
        .parse::<usize>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "RESP length is not decimal"))
}

async fn read_line<S>(stream: &mut S) -> io::Result<Vec<u8>>
where
    S: AsyncRead + Unpin,
{
    let mut out = Vec::new();
    loop {
        let Some(byte) = read_byte(stream).await? else {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "unexpected EOF in RESP line",
            ));
        };
        if byte == b'\r' {
            let Some(next) = read_byte(stream).await? else {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "unexpected EOF after CR",
                ));
            };
            if next == b'\n' {
                return Ok(out);
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "RESP line is missing LF",
            ));
        }
        out.push(byte);
    }
}

async fn expect_crlf<S>(stream: &mut S) -> io::Result<()>
where
    S: AsyncRead + Unpin,
{
    let mut crlf = [0; 2];
    stream.read_exact(&mut crlf).await?;
    if crlf == *b"\r\n" {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "RESP bulk string missing CRLF",
        ))
    }
}

fn bytes_to_str(bytes: &[u8]) -> io::Result<&str> {
    std::str::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "argument is not UTF-8"))
}

fn bytes_to_string(bytes: &[u8]) -> io::Result<String> {
    String::from_utf8(bytes.to_vec())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "argument is not UTF-8"))
}

fn parse_u64(bytes: &[u8]) -> io::Result<u64> {
    bytes_to_str(bytes)?
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "integer argument is invalid"))
}

fn parse_f64(bytes: &[u8]) -> io::Result<f64> {
    let value = bytes_to_str(bytes)?
        .parse::<f64>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "float argument is invalid"))?;
    if value.is_nan() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "float argument is invalid",
        ));
    }
    Ok(value)
}

fn loom_error_to_io(err: loom_core::error::LoomError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, err.message)
}

fn hosted_error_to_io(err: HostedError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, err.message)
}

fn redis_record_key(kind: &str, redis_key: &[u8]) -> Value {
    Value::List(vec![
        Value::Text("redis-v1".to_string()),
        Value::Text(kind.to_string()),
        Value::Bytes(redis_key.to_vec()),
    ])
}

fn redis_subrecord_key(kind: &str, redis_key: &[u8], subkey: &[u8]) -> Value {
    Value::List(vec![
        Value::Text("redis-v1".to_string()),
        Value::Text(kind.to_string()),
        Value::Bytes(redis_key.to_vec()),
        Value::Bytes(subkey.to_vec()),
    ])
}

fn redis_list_node_key(redis_key: &[u8], index: i64) -> Value {
    redis_subrecord_key("list-node", redis_key, &index.to_be_bytes())
}

fn redis_zset_member_key(redis_key: &[u8], member: &[u8]) -> Value {
    redis_subrecord_key("zset-member", redis_key, member)
}

fn redis_stream_name(redis_key: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity("redis-stream:".len() + redis_key.len() * 2);
    out.push_str("redis-stream:");
    for byte in redis_key {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn redis_pattern_matches(pattern: &[u8], value: &[u8]) -> bool {
    fn matches_inner(pattern: &[u8], value: &[u8]) -> bool {
        if pattern.is_empty() {
            return value.is_empty();
        }
        match pattern[0] {
            b'*' => {
                matches_inner(&pattern[1..], value)
                    || (!value.is_empty() && matches_inner(pattern, &value[1..]))
            }
            b'?' => !value.is_empty() && matches_inner(&pattern[1..], &value[1..]),
            b'[' => {
                let Some(end) = pattern.iter().position(|byte| *byte == b']') else {
                    return !value.is_empty()
                        && pattern[0] == value[0]
                        && matches_inner(&pattern[1..], &value[1..]);
                };
                !value.is_empty()
                    && pattern[1..end].contains(&value[0])
                    && matches_inner(&pattern[end + 1..], &value[1..])
            }
            byte => {
                !value.is_empty() && byte == value[0] && matches_inner(&pattern[1..], &value[1..])
            }
        }
    }
    matches_inner(pattern, value)
}

enum PersistedRedisRecordKey {
    Meta(Vec<u8>),
    String(Vec<u8>),
    HashField { redis_key: Vec<u8>, field: Vec<u8> },
    SetMember { redis_key: Vec<u8>, member: Vec<u8> },
    ListNode { redis_key: Vec<u8>, index: i64 },
    ZsetMember { redis_key: Vec<u8>, member: Vec<u8> },
}

impl PersistedRedisRecordKey {
    fn redis_key(&self) -> &[u8] {
        match self {
            PersistedRedisRecordKey::Meta(key)
            | PersistedRedisRecordKey::String(key)
            | PersistedRedisRecordKey::HashField { redis_key: key, .. }
            | PersistedRedisRecordKey::SetMember { redis_key: key, .. }
            | PersistedRedisRecordKey::ListNode { redis_key: key, .. }
            | PersistedRedisRecordKey::ZsetMember { redis_key: key, .. } => key,
        }
    }
}

fn redis_record_key_parts(key: &Value) -> Option<PersistedRedisRecordKey> {
    let Value::List(parts) = key else {
        return None;
    };
    match parts.as_slice() {
        [
            Value::Text(prefix),
            Value::Text(kind),
            Value::Bytes(redis_key),
        ] if prefix == "redis-v1" => match kind.as_str() {
            "meta" => Some(PersistedRedisRecordKey::Meta(redis_key.clone())),
            "string" => Some(PersistedRedisRecordKey::String(redis_key.clone())),
            _ => None,
        },
        [
            Value::Text(prefix),
            Value::Text(kind),
            Value::Bytes(redis_key),
            Value::Bytes(subkey),
        ] if prefix == "redis-v1" => match kind.as_str() {
            "hash-field" => Some(PersistedRedisRecordKey::HashField {
                redis_key: redis_key.clone(),
                field: subkey.clone(),
            }),
            "set-member" => Some(PersistedRedisRecordKey::SetMember {
                redis_key: redis_key.clone(),
                member: subkey.clone(),
            }),
            "list-node" => decode_i64(subkey).map(|index| PersistedRedisRecordKey::ListNode {
                redis_key: redis_key.clone(),
                index,
            }),
            "zset-member" => Some(PersistedRedisRecordKey::ZsetMember {
                redis_key: redis_key.clone(),
                member: subkey.clone(),
            }),
            _ => None,
        },
        _ => None,
    }
}

fn encode_meta(kind: RedisValueKind, expires_at_ms: Option<u64>) -> Vec<u8> {
    let mut out = Vec::with_capacity(14);
    out.extend_from_slice(META_MAGIC);
    out.push(redis_kind_byte(kind));
    match expires_at_ms {
        Some(expires_at_ms) => {
            out.push(1);
            out.extend_from_slice(&expires_at_ms.to_be_bytes());
        }
        None => {
            out.push(0);
            out.extend_from_slice(&0_u64.to_be_bytes());
        }
    }
    out
}

fn decode_meta(bytes: &[u8]) -> io::Result<Option<(RedisValueKind, Option<u64>)>> {
    if bytes.len() != 14 || &bytes[..4] != META_MAGIC {
        return Ok(None);
    }
    let Some(kind) = redis_kind_from_byte(bytes[4]) else {
        return Ok(None);
    };
    let expires_at_ms = match bytes[5] {
        0 => None,
        1 => Some(u64::from_be_bytes(bytes[6..14].try_into().unwrap())),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid Redis metadata expiry flag",
            ));
        }
    };
    Ok(Some((kind, expires_at_ms)))
}

fn decode_i64(bytes: &[u8]) -> Option<i64> {
    let bytes: [u8; 8] = bytes.try_into().ok()?;
    Some(i64::from_be_bytes(bytes))
}

fn encode_f64(value: f64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn decode_f64(bytes: &[u8]) -> io::Result<f64> {
    let bytes: [u8; 8] = bytes
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid zset score"))?;
    let value = f64::from_be_bytes(bytes);
    if value.is_nan() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid zset score",
        ));
    }
    Ok(value)
}

fn format_redis_score(value: f64) -> String {
    value.to_string()
}

fn redis_kind_byte(kind: RedisValueKind) -> u8 {
    match kind {
        RedisValueKind::String => 1,
        RedisValueKind::Hash => 2,
        RedisValueKind::Set => 3,
        RedisValueKind::List => 4,
        RedisValueKind::SortedSet => 5,
        RedisValueKind::Stream => 6,
    }
}

fn redis_kind_from_byte(byte: u8) -> Option<RedisValueKind> {
    match byte {
        1 => Some(RedisValueKind::String),
        2 => Some(RedisValueKind::Hash),
        3 => Some(RedisValueKind::Set),
        4 => Some(RedisValueKind::List),
        5 => Some(RedisValueKind::SortedSet),
        6 => Some(RedisValueKind::Stream),
        _ => None,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    use crate::test_support::{init, nid, temp_path};

    /// Run one `redis-cli` invocation against the local listener, authenticating as the root
    /// principal (two-argument `AUTH <principal> <secret>` via `--user`). Asserts the client exited
    /// successfully and returns trimmed stdout.
    fn run_redis_cli(port: u16, args: &[&str]) -> String {
        let output = Command::new("redis-cli")
            .arg("-h")
            .arg("127.0.0.1")
            .arg("-p")
            .arg(port.to_string())
            .arg("--no-auth-warning")
            .arg("--user")
            .arg(nid(1).to_string())
            .arg("-a")
            .arg("root-pass")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "redis-cli {args:?} failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn redis_stream_id_auto_advances_within_same_millisecond() {
        let last = Some(RedisStreamId { ms: 42, seq: 7 });
        assert_eq!(
            RedisStreamId::for_xadd("*", last, 41).unwrap(),
            RedisStreamId { ms: 42, seq: 8 }
        );
    }

    #[test]
    fn redis_stream_record_replay_applies_tombstones_without_blob_rewrite() {
        let first = RedisStreamRecord::Add {
            id: RedisStreamId { ms: 10, seq: 0 },
            fields: vec![(b"field".to_vec(), b"one".to_vec())],
        };
        let second = RedisStreamRecord::Add {
            id: RedisStreamId { ms: 11, seq: 0 },
            fields: vec![(b"field".to_vec(), b"two".to_vec())],
        };
        let delete = RedisStreamRecord::Delete {
            id: RedisStreamId { ms: 10, seq: 0 },
        };
        let mut log = RedisStreamLog::new();
        for record in [first, second, delete] {
            log.apply(RedisStreamRecord::from_cbor(&record.to_cbor()).unwrap());
        }
        assert_eq!(log.last_add_id(), Some(RedisStreamId { ms: 11, seq: 0 }));
        assert_eq!(log.entries.len(), 1);
        assert_eq!(log.entries[0].id, RedisStreamId { ms: 11, seq: 0 });
        assert_eq!(log.entries[0].fields[0].1, b"two");
    }

    #[test]
    fn redis_stream_names_are_queue_safe_hex() {
        assert_eq!(redis_stream_name(b"a/b"), "redis-stream:612f62");
    }

    #[test]
    fn redis_pubsub_state_counts_direct_and_pattern_subscribers() {
        let mut state = RedisPubSubState::new();
        state.subscribe(b"orders.created", false);
        state.subscribe(b"orders.*", true);
        assert_eq!(state.delivery_count(b"orders.created"), 2);
        assert_eq!(state.delivery_count(b"orders.deleted"), 1);
        state.unsubscribe(b"orders.created", false);
        assert_eq!(state.delivery_count(b"orders.created"), 1);
    }

    #[test]
    fn redis_pubsub_pattern_matching_supports_basic_globs() {
        assert!(redis_pattern_matches(b"orders.*", b"orders.created"));
        assert!(redis_pattern_matches(b"order?", b"order1"));
        assert!(redis_pattern_matches(b"tenant[12]", b"tenant2"));
        assert!(!redis_pattern_matches(b"tenant[12]", b"tenant3"));
    }

    /// Guarded official-client transcript. Exercises the source-backed R1-R5 command families
    /// (strings/TTL, hashes, sets, lists, sorted-sets) through the real `redis-cli` binary. This is
    /// env-gated: when `redis-cli` is not installed (as in the current build environment, 0019b C1)
    /// the test returns early and is reported as passed/skipped, matching the guarded
    /// `psql`/`mysql`/AWS-CLI transcript pattern. When `redis-cli` is present it provides official
    /// client evidence beyond the in-process raw-socket transcripts.
    #[test]
    fn redis_cli_transcript_covers_command_profile_when_available() {
        if Command::new("redis-cli").arg("--version").output().is_err() {
            return;
        }

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let path = temp_path("redis-cli-transcript");
            init(&path, None);
            let kernel = HostedKernel::new(&path);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(serve_redis_resp(
                listener,
                kernel,
                "main".to_string(),
                "cache".to_string(),
                async move {
                    let _ = shutdown_rx.await;
                },
            ));

            // Strings + TTL (R3)
            assert_eq!(
                run_redis_cli(addr.port(), &["SET", "greeting", "hello"]),
                "OK"
            );
            assert_eq!(run_redis_cli(addr.port(), &["GET", "greeting"]), "hello");
            run_redis_cli(addr.port(), &["EXPIRE", "greeting", "100"]);
            assert_eq!(run_redis_cli(addr.port(), &["TYPE", "greeting"]), "string");
            // Hashes (R4)
            run_redis_cli(addr.port(), &["HSET", "profile", "name", "ada"]);
            assert_eq!(
                run_redis_cli(addr.port(), &["HGET", "profile", "name"]),
                "ada"
            );
            // Sets (R4)
            run_redis_cli(addr.port(), &["SADD", "tags", "x"]);
            assert_eq!(run_redis_cli(addr.port(), &["SISMEMBER", "tags", "x"]), "1");
            // Lists (R5)
            run_redis_cli(addr.port(), &["RPUSH", "queue", "a"]);
            assert_eq!(run_redis_cli(addr.port(), &["LLEN", "queue"]), "1");
            // Sorted sets (R5)
            run_redis_cli(addr.port(), &["ZADD", "board", "1", "alice"]);
            run_redis_cli(addr.port(), &["ZSCORE", "board", "alice"]);
            // Delete (R3)
            assert_eq!(run_redis_cli(addr.port(), &["DEL", "greeting"]), "1");

            let _ = shutdown_tx.send(());
            server.await.unwrap().unwrap();
            fs::remove_file(path).unwrap();
        });
    }
}
