use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use loom_core::{Value, key_to_cbor};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::{HostedAuth, HostedKernel};

const RECORD_MAGIC: &[u8; 4] = b"LMC1";

pub async fn serve_memcached_text<F>(
    listener: TcpListener,
    cache_name: String,
    shutdown: F,
) -> io::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let state = Arc::new(Mutex::new(MemcachedCache::new(cache_name)));
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => return Ok(()),
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    let _ = MemcachedConnection { stream, state }.run().await;
                });
            }
        }
    }
}

pub async fn serve_memcached_text_backed<F>(
    listener: TcpListener,
    kernel: HostedKernel,
    workspace: String,
    display_name: String,
    collection: String,
    mode: MemcachedCacheMode,
    shutdown: F,
) -> io::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let state = Arc::new(Mutex::new(MemcachedCache::new_backed(
        display_name,
        kernel,
        workspace,
        collection,
        mode,
    )));
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => return Ok(()),
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    let _ = MemcachedConnection { stream, state }.run().await;
                });
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemcachedCacheMode {
    Versioned,
    ReadThrough,
    WriteThrough,
    WriteAround,
    WriteBehind,
}

impl MemcachedCacheMode {
    pub fn from_profile(profile: Option<&str>) -> Option<Self> {
        match profile {
            Some("versioned") => Some(Self::Versioned),
            Some("read-through") => Some(Self::ReadThrough),
            Some("write-through") => Some(Self::WriteThrough),
            Some("write-around") => Some(Self::WriteAround),
            Some("write-behind") => Some(Self::WriteBehind),
            _ => None,
        }
    }
}

struct MemcachedConnection<S> {
    stream: S,
    state: Arc<Mutex<MemcachedCache>>,
}

impl<S> MemcachedConnection<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    async fn run(mut self) -> io::Result<()> {
        loop {
            let line = match read_line(&mut self.stream).await {
                Ok(Some(line)) => line,
                Ok(None) => return Ok(()),
                Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(err) => return Err(err),
            };
            if line.is_empty() {
                continue;
            }
            let parts = split_ascii_words(&line)?;
            let Some(command) = parts.first() else {
                continue;
            };
            match command.to_ascii_lowercase().as_str() {
                "get" => self.get(&parts[1..], false).await?,
                "gets" => self.get(&parts[1..], true).await?,
                "gat" => self.gat(&parts[1..], false).await?,
                "gats" => self.gat(&parts[1..], true).await?,
                "set" | "add" | "replace" | "append" | "prepend" => {
                    self.storage(command, &parts[1..], None).await?
                }
                "cas" => {
                    let Some(token) = parts.get(5) else {
                        self.write_line("CLIENT_ERROR bad command line format")
                            .await?;
                        continue;
                    };
                    let token = parse_u64(token)?;
                    self.storage(command, &parts[1..], Some(token)).await?;
                }
                "incr" => self.counter(&parts[1..], CounterOp::Increment).await?,
                "decr" => self.counter(&parts[1..], CounterOp::Decrement).await?,
                "delete" => self.delete(&parts[1..]).await?,
                "touch" => self.touch(&parts[1..]).await?,
                "flush_all" => self.flush_all(&parts[1..]).await?,
                "verbosity" => self.verbosity(&parts[1..]).await?,
                "stats" => self.stats().await?,
                "version" => self.write_line("VERSION loom-memcached").await?,
                "quit" => return Ok(()),
                _ => self.write_line("ERROR").await?,
            }
        }
    }

    async fn get(&mut self, keys: &[String], include_cas: bool) -> io::Result<()> {
        let entries = {
            let mut state = self.state.lock().await;
            keys.iter()
                .filter_map(|key| state.get(key).map(|entry| (key.clone(), entry)))
                .collect::<Vec<_>>()
        };
        for (key, entry) in entries {
            if include_cas {
                self.write_line(&format!(
                    "VALUE {} {} {} {}",
                    key,
                    entry.flags,
                    entry.value.len(),
                    entry.cas
                ))
                .await?;
            } else {
                self.write_line(&format!(
                    "VALUE {} {} {}",
                    key,
                    entry.flags,
                    entry.value.len()
                ))
                .await?;
            }
            self.stream.write_all(&entry.value).await?;
            self.stream.write_all(b"\r\n").await?;
        }
        self.write_line("END").await
    }

    async fn gat(&mut self, args: &[String], include_cas: bool) -> io::Result<()> {
        if args.len() < 2 {
            return self
                .write_line("CLIENT_ERROR bad command line format")
                .await;
        }
        let expires_at_ms = parse_exptime(&args[0])?;
        let entries = {
            let mut state = self.state.lock().await;
            args[1..]
                .iter()
                .filter_map(|key| {
                    state
                        .get_and_touch(key, expires_at_ms)
                        .map(|entry| (key.clone(), entry))
                })
                .collect::<Vec<_>>()
        };
        for (key, entry) in entries {
            if include_cas {
                self.write_line(&format!(
                    "VALUE {} {} {} {}",
                    key,
                    entry.flags,
                    entry.value.len(),
                    entry.cas
                ))
                .await?;
            } else {
                self.write_line(&format!(
                    "VALUE {} {} {}",
                    key,
                    entry.flags,
                    entry.value.len()
                ))
                .await?;
            }
            self.stream.write_all(&entry.value).await?;
            self.stream.write_all(b"\r\n").await?;
        }
        self.write_line("END").await
    }

    async fn storage(
        &mut self,
        command: &str,
        args: &[String],
        cas_token: Option<u64>,
    ) -> io::Result<()> {
        if args.len() < 4 {
            return self
                .write_line("CLIENT_ERROR bad command line format")
                .await;
        }
        let key = args[0].clone();
        let flags = parse_u32(&args[1])?;
        let expires_at_ms = parse_exptime(&args[2])?;
        let bytes = parse_usize(&args[3])?;
        let noreply = args.iter().any(|part| part == "noreply");
        let mut value = vec![0; bytes];
        self.stream.read_exact(&mut value).await?;
        expect_crlf(&mut self.stream).await?;

        let outcome = {
            let mut state = self.state.lock().await;
            match command.to_ascii_lowercase().as_str() {
                "set" => state.set(key, flags, expires_at_ms, value),
                "add" => state.add(key, flags, expires_at_ms, value),
                "replace" => state.replace(key, flags, expires_at_ms, value),
                "append" => state.append(key, value),
                "prepend" => state.prepend(key, value),
                "cas" => state.cas(
                    key,
                    flags,
                    expires_at_ms,
                    value,
                    cas_token.unwrap_or_default(),
                ),
                _ => StorageOutcome::NotStored,
            }
        };
        if noreply {
            return Ok(());
        }
        match outcome {
            StorageOutcome::Stored => self.write_line("STORED").await,
            StorageOutcome::NotStored => self.write_line("NOT_STORED").await,
            StorageOutcome::Exists => self.write_line("EXISTS").await,
            StorageOutcome::NotFound => self.write_line("NOT_FOUND").await,
        }
    }

    async fn counter(&mut self, args: &[String], op: CounterOp) -> io::Result<()> {
        if args.len() < 2 {
            return self
                .write_line("CLIENT_ERROR bad command line format")
                .await;
        }
        let delta = parse_u64(&args[1])?;
        let noreply = args.iter().any(|part| part == "noreply");
        let result = {
            let mut state = self.state.lock().await;
            state.counter(&args[0], delta, op)
        };
        if noreply {
            return Ok(());
        }
        match result {
            CounterOutcome::Value(value) => self.write_line(&value.to_string()).await,
            CounterOutcome::NotFound => self.write_line("NOT_FOUND").await,
            CounterOutcome::NonNumeric => {
                self.write_line("CLIENT_ERROR cannot increment or decrement non-numeric value")
                    .await
            }
        }
    }

    async fn delete(&mut self, args: &[String]) -> io::Result<()> {
        if args.is_empty() {
            return self
                .write_line("CLIENT_ERROR bad command line format")
                .await;
        }
        let noreply = args.iter().any(|part| part == "noreply");
        let deleted = {
            let mut state = self.state.lock().await;
            state.delete(&args[0])
        };
        if noreply {
            return Ok(());
        }
        if deleted {
            self.write_line("DELETED").await
        } else {
            self.write_line("NOT_FOUND").await
        }
    }

    async fn touch(&mut self, args: &[String]) -> io::Result<()> {
        if args.len() < 2 {
            return self
                .write_line("CLIENT_ERROR bad command line format")
                .await;
        }
        let expires_at_ms = parse_exptime(&args[1])?;
        let noreply = args.iter().any(|part| part == "noreply");
        let touched = {
            let mut state = self.state.lock().await;
            state.touch(&args[0], expires_at_ms)
        };
        if noreply {
            return Ok(());
        }
        if touched {
            self.write_line("TOUCHED").await
        } else {
            self.write_line("NOT_FOUND").await
        }
    }

    async fn flush_all(&mut self, args: &[String]) -> io::Result<()> {
        let noreply = args.iter().any(|part| part == "noreply");
        let delay = args
            .iter()
            .find(|part| part.as_str() != "noreply")
            .map(|part| parse_u64(part))
            .transpose()?;
        {
            let mut state = self.state.lock().await;
            state.flush_all(delay);
        }
        if noreply {
            return Ok(());
        }
        self.write_line("OK").await
    }

    async fn verbosity(&mut self, args: &[String]) -> io::Result<()> {
        if args.is_empty() {
            return self
                .write_line("CLIENT_ERROR bad command line format")
                .await;
        }
        let _ = parse_u32(&args[0])?;
        let noreply = args.iter().any(|part| part == "noreply");
        if noreply {
            return Ok(());
        }
        self.write_line("OK").await
    }

    async fn stats(&mut self) -> io::Result<()> {
        let (name, count) = {
            let mut state = self.state.lock().await;
            state.sweep();
            (state.name.clone(), state.entries.len())
        };
        self.write_line("STAT version loom-memcached").await?;
        self.write_line(&format!("STAT cache {}", name)).await?;
        self.write_line(&format!("STAT curr_items {}", count))
            .await?;
        self.write_line("END").await
    }

    async fn write_line(&mut self, value: &str) -> io::Result<()> {
        self.stream.write_all(value.as_bytes()).await?;
        self.stream.write_all(b"\r\n").await
    }
}

struct MemcachedCache {
    name: String,
    next_cas: u64,
    entries: BTreeMap<String, MemcachedEntry>,
    backing: Option<MemcachedBacking>,
    flush_at_ms: Option<u64>,
}

struct MemcachedBacking {
    kernel: HostedKernel,
    workspace: String,
    collection: String,
    mode: MemcachedCacheMode,
}

impl MemcachedCache {
    fn new(name: String) -> Self {
        Self {
            name,
            next_cas: 1,
            entries: BTreeMap::new(),
            backing: None,
            flush_at_ms: None,
        }
    }

    fn new_backed(
        name: String,
        kernel: HostedKernel,
        workspace: String,
        collection: String,
        mode: MemcachedCacheMode,
    ) -> Self {
        Self {
            name,
            next_cas: 1,
            entries: BTreeMap::new(),
            backing: Some(MemcachedBacking {
                kernel,
                workspace,
                collection,
                mode,
            }),
            flush_at_ms: None,
        }
    }

    fn get(&mut self, key: &str) -> Option<MemcachedEntry> {
        self.sweep_key(key);
        if let Some(entry) = self.entries.get(key).cloned() {
            return Some(entry);
        }
        let entry = self.load_backing(key).ok().flatten()?;
        self.entries.insert(key.to_string(), entry.clone());
        Some(entry)
    }

    fn get_and_touch(&mut self, key: &str, expires_at_ms: Option<u64>) -> Option<MemcachedEntry> {
        if !self.touch(key, expires_at_ms) {
            return None;
        }
        self.entries.get(key).cloned()
    }

    fn set(
        &mut self,
        key: String,
        flags: u32,
        expires_at_ms: Option<u64>,
        value: Vec<u8>,
    ) -> StorageOutcome {
        self.put(key, flags, expires_at_ms, value);
        StorageOutcome::Stored
    }

    fn add(
        &mut self,
        key: String,
        flags: u32,
        expires_at_ms: Option<u64>,
        value: Vec<u8>,
    ) -> StorageOutcome {
        self.sweep_key(&key);
        if !self.entries.contains_key(&key) && self.load_backing(&key).ok().flatten().is_some() {
            return StorageOutcome::NotStored;
        }
        if self.entries.contains_key(&key) {
            return StorageOutcome::NotStored;
        }
        self.put(key, flags, expires_at_ms, value);
        StorageOutcome::Stored
    }

    fn replace(
        &mut self,
        key: String,
        flags: u32,
        expires_at_ms: Option<u64>,
        value: Vec<u8>,
    ) -> StorageOutcome {
        self.sweep_key(&key);
        if !self.entries.contains_key(&key)
            && let Ok(Some(entry)) = self.load_backing(&key)
        {
            self.entries.insert(key.clone(), entry);
        }
        if !self.entries.contains_key(&key) {
            return StorageOutcome::NotStored;
        }
        self.put(key, flags, expires_at_ms, value);
        StorageOutcome::Stored
    }

    fn append(&mut self, key: String, value: Vec<u8>) -> StorageOutcome {
        self.join_bytes(key, value, ByteJoin::Append)
    }

    fn prepend(&mut self, key: String, value: Vec<u8>) -> StorageOutcome {
        self.join_bytes(key, value, ByteJoin::Prepend)
    }

    fn join_bytes(&mut self, key: String, value: Vec<u8>, join: ByteJoin) -> StorageOutcome {
        self.sweep_key(&key);
        if !self.entries.contains_key(&key)
            && let Ok(Some(entry)) = self.load_backing(&key)
        {
            self.entries.insert(key.clone(), entry);
        }
        let Some(existing) = self.entries.get(&key).cloned() else {
            return StorageOutcome::NotStored;
        };
        let mut joined = Vec::with_capacity(existing.value.len().saturating_add(value.len()));
        match join {
            ByteJoin::Append => {
                joined.extend_from_slice(&existing.value);
                joined.extend_from_slice(&value);
            }
            ByteJoin::Prepend => {
                joined.extend_from_slice(&value);
                joined.extend_from_slice(&existing.value);
            }
        }
        self.put(key, existing.flags, existing.expires_at_ms, joined);
        StorageOutcome::Stored
    }

    fn cas(
        &mut self,
        key: String,
        flags: u32,
        expires_at_ms: Option<u64>,
        value: Vec<u8>,
        token: u64,
    ) -> StorageOutcome {
        self.sweep_key(&key);
        if !self.entries.contains_key(&key)
            && let Ok(Some(entry)) = self.load_backing(&key)
        {
            self.entries.insert(key.clone(), entry);
        }
        let Some(existing) = self.entries.get(&key) else {
            return StorageOutcome::NotFound;
        };
        if existing.cas != token {
            return StorageOutcome::Exists;
        }
        self.put(key, flags, expires_at_ms, value);
        StorageOutcome::Stored
    }

    fn counter(&mut self, key: &str, delta: u64, op: CounterOp) -> CounterOutcome {
        self.sweep_key(key);
        if !self.entries.contains_key(key)
            && let Ok(Some(entry)) = self.load_backing(key)
        {
            self.entries.insert(key.to_string(), entry);
        }
        let Some(existing) = self.entries.get(key).cloned() else {
            return CounterOutcome::NotFound;
        };
        let Ok(current) = parse_counter_value(&existing.value) else {
            return CounterOutcome::NonNumeric;
        };
        let next = match op {
            CounterOp::Increment => current.saturating_add(delta),
            CounterOp::Decrement => current.saturating_sub(delta),
        };
        self.put(
            key.to_string(),
            existing.flags,
            existing.expires_at_ms,
            next.to_string().into_bytes(),
        );
        CounterOutcome::Value(next)
    }

    fn delete(&mut self, key: &str) -> bool {
        self.sweep_key(key);
        let removed = self.entries.remove(key).is_some();
        let backed = self.delete_backing(key).unwrap_or(false);
        removed || backed
    }

    fn touch(&mut self, key: &str, expires_at_ms: Option<u64>) -> bool {
        self.sweep_key(key);
        if !self.entries.contains_key(key)
            && let Ok(Some(entry)) = self.load_backing(key)
        {
            self.entries.insert(key.to_string(), entry);
        }
        let Some(entry) = self.entries.get_mut(key) else {
            return false;
        };
        entry.expires_at_ms = expires_at_ms;
        let entry = entry.clone();
        let _ = self.persist_backing(key, &entry);
        true
    }

    fn flush_all(&mut self, delay_secs: Option<u64>) {
        match delay_secs {
            Some(delay) if delay > 0 => {
                self.flush_at_ms = Some(now_ms().saturating_add(delay.saturating_mul(1000)));
            }
            _ => self.flush_now(),
        }
    }

    fn flush_now(&mut self) {
        self.entries.clear();
        self.flush_at_ms = None;
        let _ = self.flush_backing();
    }

    fn put(&mut self, key: String, flags: u32, expires_at_ms: Option<u64>, value: Vec<u8>) {
        let cas = self.next_cas;
        self.next_cas = self.next_cas.saturating_add(1);
        let entry = MemcachedEntry {
            flags,
            value,
            expires_at_ms,
            cas,
        };
        match self.backing.as_ref().map(|backing| backing.mode) {
            Some(MemcachedCacheMode::WriteAround) => {
                let _ = self.persist_backing(&key, &entry);
                self.entries.remove(&key);
            }
            Some(MemcachedCacheMode::Versioned)
            | Some(MemcachedCacheMode::WriteThrough)
            | Some(MemcachedCacheMode::WriteBehind) => {
                let _ = self.persist_backing(&key, &entry);
                self.entries.insert(key, entry);
            }
            Some(MemcachedCacheMode::ReadThrough) | None => {
                self.entries.insert(key, entry);
            }
        }
    }

    fn load_backing(&mut self, key: &str) -> io::Result<Option<MemcachedEntry>> {
        let Some(backing) = self.backing.as_ref() else {
            return Ok(None);
        };
        let auth = HostedAuth::unauthenticated();
        let bytes = backing
            .kernel
            .data()
            .kv_get(
                &auth,
                &backing.workspace,
                &backing.collection,
                &memcached_key_cbor(key),
            )
            .map_err(|err| io::Error::other(err.message))?;
        let Some(bytes) = bytes else {
            return Ok(None);
        };
        let Some(entry) = decode_record(&bytes)? else {
            let _ = self.delete_backing(key);
            return Ok(None);
        };
        self.next_cas = self.next_cas.max(entry.cas.saturating_add(1));
        if is_expired(entry.expires_at_ms, now_ms()) {
            let _ = self.delete_backing(key);
            return Ok(None);
        }
        Ok(Some(entry))
    }

    fn persist_backing(&self, key: &str, entry: &MemcachedEntry) -> io::Result<()> {
        let Some(backing) = self.backing.as_ref() else {
            return Ok(());
        };
        if backing.mode == MemcachedCacheMode::ReadThrough {
            return Ok(());
        }
        let auth = HostedAuth::unauthenticated();
        backing
            .kernel
            .data()
            .kv_put(
                &auth,
                &backing.workspace,
                &backing.collection,
                &memcached_key_cbor(key),
                encode_record(entry),
            )
            .map_err(|err| io::Error::other(err.message))
    }

    fn delete_backing(&self, key: &str) -> io::Result<bool> {
        let Some(backing) = self.backing.as_ref() else {
            return Ok(false);
        };
        let auth = HostedAuth::unauthenticated();
        backing
            .kernel
            .data()
            .kv_delete(
                &auth,
                &backing.workspace,
                &backing.collection,
                &memcached_key_cbor(key),
            )
            .map_err(|err| io::Error::other(err.message))
    }

    fn flush_backing(&self) -> io::Result<()> {
        let Some(backing) = self.backing.as_ref() else {
            return Ok(());
        };
        let auth = HostedAuth::unauthenticated();
        let entries = backing
            .kernel
            .data()
            .kv_list(&auth, &backing.workspace, &backing.collection)
            .map_err(|err| io::Error::other(err.message))?;
        for entry in entries {
            let _ = backing.kernel.data().kv_delete(
                &auth,
                &backing.workspace,
                &backing.collection,
                &entry.key_cbor,
            );
        }
        Ok(())
    }

    fn sweep(&mut self) {
        self.apply_flush_deadline();
        let now = now_ms();
        self.entries
            .retain(|_, entry| !is_expired(entry.expires_at_ms, now));
    }

    fn sweep_key(&mut self, key: &str) {
        self.apply_flush_deadline();
        let Some(entry) = self.entries.get(key) else {
            return;
        };
        if is_expired(entry.expires_at_ms, now_ms()) {
            self.entries.remove(key);
        }
    }

    fn apply_flush_deadline(&mut self) {
        if self
            .flush_at_ms
            .is_some_and(|flush_at_ms| flush_at_ms <= now_ms())
        {
            self.flush_now();
        }
    }
}

#[derive(Clone)]
struct MemcachedEntry {
    flags: u32,
    value: Vec<u8>,
    expires_at_ms: Option<u64>,
    cas: u64,
}

enum StorageOutcome {
    Stored,
    NotStored,
    Exists,
    NotFound,
}

#[derive(Clone, Copy)]
enum CounterOp {
    Increment,
    Decrement,
}

enum CounterOutcome {
    Value(u64),
    NotFound,
    NonNumeric,
}

enum ByteJoin {
    Append,
    Prepend,
}

fn memcached_key_cbor(key: &str) -> Vec<u8> {
    key_to_cbor(&Value::Bytes(key.as_bytes().to_vec()))
}

fn parse_counter_value(bytes: &[u8]) -> Result<u64, ()> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return Err(());
    }
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(())
}

fn encode_record(entry: &MemcachedEntry) -> Vec<u8> {
    let mut out = Vec::with_capacity(28 + entry.value.len());
    out.extend_from_slice(RECORD_MAGIC);
    out.extend_from_slice(&entry.flags.to_be_bytes());
    out.extend_from_slice(&entry.cas.to_be_bytes());
    out.extend_from_slice(&entry.expires_at_ms.unwrap_or(0).to_be_bytes());
    out.extend_from_slice(&(entry.value.len() as u64).to_be_bytes());
    out.extend_from_slice(&entry.value);
    out
}

fn decode_record(bytes: &[u8]) -> io::Result<Option<MemcachedEntry>> {
    if bytes.len() < 32 || &bytes[..4] != RECORD_MAGIC {
        return Ok(None);
    }
    let flags = u32::from_be_bytes(bytes[4..8].try_into().unwrap_or_default());
    let cas = u64::from_be_bytes(bytes[8..16].try_into().unwrap_or_default());
    let expires = u64::from_be_bytes(bytes[16..24].try_into().unwrap_or_default());
    let value_len = u64::from_be_bytes(bytes[24..32].try_into().unwrap_or_default());
    let value_len = usize::try_from(value_len)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "record value too large"))?;
    let end = 32usize
        .checked_add(value_len)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "record value too large"))?;
    if bytes.len() != end {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid Memcached backing record",
        ));
    }
    Ok(Some(MemcachedEntry {
        flags,
        value: bytes[32..].to_vec(),
        expires_at_ms: (expires != 0).then_some(expires),
        cas,
    }))
}

async fn read_line<S>(stream: &mut S) -> io::Result<Option<Vec<u8>>>
where
    S: AsyncRead + Unpin,
{
    let mut out = Vec::new();
    loop {
        let mut byte = [0];
        match stream.read_exact(&mut byte).await {
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof && out.is_empty() => {
                return Ok(None);
            }
            Err(err) => return Err(err),
        }
        if byte[0] == b'\r' {
            let mut next = [0];
            stream.read_exact(&mut next).await?;
            if next[0] == b'\n' {
                return Ok(Some(out));
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Memcached line missing LF",
            ));
        }
        out.push(byte[0]);
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
            "Memcached payload missing CRLF",
        ))
    }
}

fn split_ascii_words(line: &[u8]) -> io::Result<Vec<String>> {
    let line = std::str::from_utf8(line)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "line is not UTF-8"))?;
    Ok(line.split_whitespace().map(str::to_string).collect())
}

fn parse_u32(value: &str) -> io::Result<u32> {
    value
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid integer"))
}

fn parse_u64(value: &str) -> io::Result<u64> {
    value
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid integer"))
}

fn parse_usize(value: &str) -> io::Result<usize> {
    value
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid integer"))
}

fn parse_exptime(value: &str) -> io::Result<Option<u64>> {
    let seconds = parse_u64(value)?;
    if seconds == 0 {
        return Ok(None);
    }
    if seconds <= 60 * 60 * 24 * 30 {
        return Ok(Some(now_ms().saturating_add(seconds.saturating_mul(1000))));
    }
    Ok(Some(seconds.saturating_mul(1000)))
}

fn is_expired(expires_at_ms: Option<u64>, now_ms: u64) -> bool {
    expires_at_ms.is_some_and(|expires_at_ms| expires_at_ms <= now_ms)
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
    use std::process::Command;

    fn memcached_client(tool: &str) -> Option<String> {
        if let Ok(dir) = std::env::var("LOOM_MEMCACHED_CLIENT_DIR") {
            let candidate = format!("{dir}/{tool}");
            if Command::new(&candidate).arg("--version").output().is_ok() {
                return Some(candidate);
            }
        }
        if Command::new(tool).arg("--version").output().is_ok() {
            return Some(tool.to_string());
        }
        None
    }

    #[test]
    fn guarded_memcached_client_transcript_covers_volatile_profile() {
        let (Some(memcp), Some(memcat), Some(memrm)) = (
            memcached_client("memcp"),
            memcached_client("memcat"),
            memcached_client("memrm"),
        ) else {
            return;
        };

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(serve_memcached_text(
                listener,
                "guarded".to_string(),
                async move {
                    let _ = shutdown_rx.await;
                },
            ));

            let servers = format!("--servers=127.0.0.1:{}", addr.port());
            let dir = std::env::temp_dir().join(format!("loom-memcached-{}", addr.port()));
            std::fs::create_dir_all(&dir).unwrap();
            let key_file = dir.join("loomkey");
            std::fs::write(&key_file, b"loomvalue").unwrap();

            let set = Command::new(&memcp)
                .arg(&servers)
                .arg(&key_file)
                .output()
                .unwrap();
            assert!(set.status.success(), "memcp SET must succeed: {set:?}");

            let got = Command::new(&memcat)
                .arg(&servers)
                .arg("loomkey")
                .output()
                .unwrap();
            assert!(got.status.success(), "memcat GET must succeed");
            assert_eq!(
                String::from_utf8_lossy(&got.stdout).trim(),
                "loomvalue",
                "GET returns the SET value over the memcached text protocol"
            );

            let _ = Command::new(&memrm)
                .arg(&servers)
                .arg("loomkey")
                .output()
                .unwrap();
            let after = Command::new(&memcat)
                .arg(&servers)
                .arg("loomkey")
                .output()
                .unwrap();
            assert!(
                String::from_utf8_lossy(&after.stdout).trim().is_empty(),
                "GET after DELETE returns no value"
            );

            let _ = shutdown_tx.send(());
            server.await.unwrap().unwrap();
            std::fs::remove_dir_all(&dir).ok();
        });
    }
}
