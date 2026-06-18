use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use loom_types::error::LoomError;

pub type RedisResult<T> = Result<T, LoomError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisPersistenceMode {
    Versioned,
    Ephemeral,
    BackedCache,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisValueKind {
    String,
    Hash,
    Set,
    List,
    SortedSet,
    Stream,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RedisTtl {
    NoKey,
    Persistent,
    RemainingMs(u64),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisKeyMeta {
    pub kind: RedisValueKind,
    pub expires_at_ms: Option<u64>,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RedisListPush {
    pub index: i64,
    pub len: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisListPop {
    pub index: i64,
    pub value: Vec<u8>,
    pub empty_after: bool,
}

#[derive(Clone, Debug)]
pub struct RedisKeyspace {
    mode: RedisPersistenceMode,
    next_revision: u64,
    catalog: BTreeMap<Vec<u8>, RedisKeyMeta>,
    expiry_index: BTreeMap<(u64, Vec<u8>), ()>,
    strings: BTreeMap<Vec<u8>, Vec<u8>>,
    hashes: BTreeMap<Vec<u8>, BTreeMap<Vec<u8>, Vec<u8>>>,
    sets: BTreeMap<Vec<u8>, BTreeSet<Vec<u8>>>,
    lists: BTreeMap<Vec<u8>, RedisList>,
    sorted_sets: BTreeMap<Vec<u8>, RedisSortedSet>,
}

impl RedisKeyspace {
    pub fn new(mode: RedisPersistenceMode) -> Self {
        Self {
            mode,
            next_revision: 1,
            catalog: BTreeMap::new(),
            expiry_index: BTreeMap::new(),
            strings: BTreeMap::new(),
            hashes: BTreeMap::new(),
            sets: BTreeMap::new(),
            lists: BTreeMap::new(),
            sorted_sets: BTreeMap::new(),
        }
    }

    pub fn mode(&self) -> RedisPersistenceMode {
        self.mode
    }

    pub fn is_empty(&self, now_ms: u64) -> bool {
        self.key_count_live(now_ms) == 0
    }

    pub fn key_count_live(&self, now_ms: u64) -> usize {
        self.catalog
            .values()
            .filter(|meta| !is_expired_at(meta.expires_at_ms, now_ms))
            .count()
    }

    pub fn live_key_kind(&self, key: &[u8], now_ms: u64) -> Option<RedisValueKind> {
        self.catalog
            .get(key)
            .filter(|meta| !is_expired_at(meta.expires_at_ms, now_ms))
            .map(|meta| meta.kind)
    }

    pub fn set_string(
        &mut self,
        key: impl Into<Vec<u8>>,
        value: impl Into<Vec<u8>>,
        expires_at_ms: Option<u64>,
    ) -> u64 {
        let key = key.into();
        let revision = self.replace_key(key.clone(), RedisValueKind::String, expires_at_ms);
        self.strings.insert(key, value.into());
        revision
    }

    pub fn get_string(&self, key: &[u8], now_ms: u64) -> RedisResult<Option<&[u8]>> {
        match self.live_meta(key, now_ms) {
            Some(meta) if meta.kind == RedisValueKind::String => {
                Ok(self.strings.get(key).map(Vec::as_slice))
            }
            Some(_) => Err(wrong_type()),
            None => Ok(None),
        }
    }

    pub fn delete(&mut self, key: &[u8]) -> bool {
        let Some(meta) = self.catalog.remove(key) else {
            return false;
        };
        self.remove_expiry_index(key, meta.expires_at_ms);
        self.clear_value_for_key(key, meta.kind);
        true
    }

    pub fn expire_at(&mut self, key: &[u8], expires_at_ms: u64, now_ms: u64) -> bool {
        let Some(previous_expires_at_ms) =
            self.live_meta(key, now_ms).map(|meta| meta.expires_at_ms)
        else {
            return false;
        };
        self.remove_expiry_index(key, previous_expires_at_ms);
        self.expiry_index.insert((expires_at_ms, key.to_vec()), ());
        let revision = self.next_revision();
        if let Some(meta) = self.catalog.get_mut(key) {
            meta.expires_at_ms = Some(expires_at_ms);
            meta.revision = revision;
        }
        true
    }

    pub fn persist(&mut self, key: &[u8], now_ms: u64) -> bool {
        let Some(previous_expires_at_ms) =
            self.live_meta(key, now_ms).map(|meta| meta.expires_at_ms)
        else {
            return false;
        };
        let Some(expires_at_ms) = previous_expires_at_ms else {
            return false;
        };
        self.remove_expiry_index(key, Some(expires_at_ms));
        let revision = self.next_revision();
        if let Some(meta) = self.catalog.get_mut(key) {
            meta.expires_at_ms = None;
            meta.revision = revision;
        }
        true
    }

    pub fn ttl_ms(&self, key: &[u8], now_ms: u64) -> RedisTtl {
        match self
            .live_meta(key, now_ms)
            .and_then(|meta| meta.expires_at_ms)
        {
            Some(expires_at_ms) => RedisTtl::RemainingMs(expires_at_ms.saturating_sub(now_ms)),
            None if self.live_meta(key, now_ms).is_some() => RedisTtl::Persistent,
            None => RedisTtl::NoKey,
        }
    }

    pub fn sweep_expired(&mut self, now_ms: u64) -> usize {
        let expired_keys: Vec<Vec<u8>> = self
            .expiry_index
            .iter()
            .take_while(|((expires_at_ms, _), ())| *expires_at_ms <= now_ms)
            .map(|((_, key), ())| key.clone())
            .collect();
        let mut removed = 0;
        for key in expired_keys {
            if self.delete(&key) {
                removed += 1;
            }
        }
        removed
    }

    pub fn hset(
        &mut self,
        key: impl Into<Vec<u8>>,
        field: impl Into<Vec<u8>>,
        value: impl Into<Vec<u8>>,
        now_ms: u64,
    ) -> RedisResult<bool> {
        let key = key.into();
        self.ensure_missing_or_kind(&key, now_ms, RedisValueKind::Hash)?;
        if !matches!(self.live_key_kind(&key, now_ms), Some(RedisValueKind::Hash)) {
            self.replace_key(key.clone(), RedisValueKind::Hash, None);
        }
        let fields = self.hashes.entry(key).or_default();
        let inserted = fields.insert(field.into(), value.into()).is_none();
        Ok(inserted)
    }

    pub fn hget(&self, key: &[u8], field: &[u8], now_ms: u64) -> RedisResult<Option<&[u8]>> {
        match self.live_meta(key, now_ms) {
            Some(meta) if meta.kind == RedisValueKind::Hash => Ok(self
                .hashes
                .get(key)
                .and_then(|fields| fields.get(field))
                .map(Vec::as_slice)),
            Some(_) => Err(wrong_type()),
            None => Ok(None),
        }
    }

    pub fn hlen(&self, key: &[u8], now_ms: u64) -> RedisResult<usize> {
        match self.live_meta(key, now_ms) {
            Some(meta) if meta.kind == RedisValueKind::Hash => {
                Ok(self.hashes.get(key).map(BTreeMap::len).unwrap_or_default())
            }
            Some(_) => Err(wrong_type()),
            None => Ok(0),
        }
    }

    pub fn sadd(
        &mut self,
        key: impl Into<Vec<u8>>,
        member: impl Into<Vec<u8>>,
        now_ms: u64,
    ) -> RedisResult<bool> {
        let key = key.into();
        self.ensure_missing_or_kind(&key, now_ms, RedisValueKind::Set)?;
        if !matches!(self.live_key_kind(&key, now_ms), Some(RedisValueKind::Set)) {
            self.replace_key(key.clone(), RedisValueKind::Set, None);
        }
        let members = self.sets.entry(key).or_default();
        Ok(members.insert(member.into()))
    }

    pub fn sismember(&self, key: &[u8], member: &[u8], now_ms: u64) -> RedisResult<bool> {
        match self.live_meta(key, now_ms) {
            Some(meta) if meta.kind == RedisValueKind::Set => Ok(self
                .sets
                .get(key)
                .is_some_and(|members| members.contains(member))),
            Some(_) => Err(wrong_type()),
            None => Ok(false),
        }
    }

    pub fn scard(&self, key: &[u8], now_ms: u64) -> RedisResult<usize> {
        match self.live_meta(key, now_ms) {
            Some(meta) if meta.kind == RedisValueKind::Set => {
                Ok(self.sets.get(key).map(BTreeSet::len).unwrap_or_default())
            }
            Some(_) => Err(wrong_type()),
            None => Ok(0),
        }
    }

    pub fn lpush(
        &mut self,
        key: impl Into<Vec<u8>>,
        value: impl Into<Vec<u8>>,
        now_ms: u64,
    ) -> RedisResult<usize> {
        Ok(self.lpush_indexed(key, value, now_ms)?.len)
    }

    pub fn lpush_indexed(
        &mut self,
        key: impl Into<Vec<u8>>,
        value: impl Into<Vec<u8>>,
        now_ms: u64,
    ) -> RedisResult<RedisListPush> {
        let key = key.into();
        self.ensure_missing_or_kind(&key, now_ms, RedisValueKind::List)?;
        if !matches!(self.live_key_kind(&key, now_ms), Some(RedisValueKind::List)) {
            self.replace_key(key.clone(), RedisValueKind::List, None);
        }
        let list = self.lists.entry(key).or_default();
        Ok(list.push_front(value.into()))
    }

    pub fn rpush(
        &mut self,
        key: impl Into<Vec<u8>>,
        value: impl Into<Vec<u8>>,
        now_ms: u64,
    ) -> RedisResult<usize> {
        Ok(self.rpush_indexed(key, value, now_ms)?.len)
    }

    pub fn rpush_indexed(
        &mut self,
        key: impl Into<Vec<u8>>,
        value: impl Into<Vec<u8>>,
        now_ms: u64,
    ) -> RedisResult<RedisListPush> {
        let key = key.into();
        self.ensure_missing_or_kind(&key, now_ms, RedisValueKind::List)?;
        if !matches!(self.live_key_kind(&key, now_ms), Some(RedisValueKind::List)) {
            self.replace_key(key.clone(), RedisValueKind::List, None);
        }
        let list = self.lists.entry(key).or_default();
        Ok(list.push_back(value.into()))
    }

    pub fn lpop(&mut self, key: &[u8], now_ms: u64) -> RedisResult<Option<Vec<u8>>> {
        Ok(self.lpop_indexed(key, now_ms)?.map(|popped| popped.value))
    }

    pub fn lpop_indexed(&mut self, key: &[u8], now_ms: u64) -> RedisResult<Option<RedisListPop>> {
        match self.live_meta(key, now_ms) {
            Some(meta) if meta.kind == RedisValueKind::List => {
                let Some(list) = self.lists.get_mut(key) else {
                    return Ok(None);
                };
                let Some((index, value)) = list.pop_front() else {
                    return Ok(None);
                };
                let empty_after = list.is_empty();
                if empty_after {
                    self.delete(key);
                }
                Ok(Some(RedisListPop {
                    index,
                    value,
                    empty_after,
                }))
            }
            Some(_) => Err(wrong_type()),
            None => Ok(None),
        }
    }

    pub fn rpop(&mut self, key: &[u8], now_ms: u64) -> RedisResult<Option<Vec<u8>>> {
        Ok(self.rpop_indexed(key, now_ms)?.map(|popped| popped.value))
    }

    pub fn rpop_indexed(&mut self, key: &[u8], now_ms: u64) -> RedisResult<Option<RedisListPop>> {
        match self.live_meta(key, now_ms) {
            Some(meta) if meta.kind == RedisValueKind::List => {
                let Some(list) = self.lists.get_mut(key) else {
                    return Ok(None);
                };
                let Some((index, value)) = list.pop_back() else {
                    return Ok(None);
                };
                let empty_after = list.is_empty();
                if empty_after {
                    self.delete(key);
                }
                Ok(Some(RedisListPop {
                    index,
                    value,
                    empty_after,
                }))
            }
            Some(_) => Err(wrong_type()),
            None => Ok(None),
        }
    }

    pub fn llen(&self, key: &[u8], now_ms: u64) -> RedisResult<usize> {
        match self.live_meta(key, now_ms) {
            Some(meta) if meta.kind == RedisValueKind::List => {
                Ok(self.lists.get(key).map(RedisList::len).unwrap_or_default())
            }
            Some(_) => Err(wrong_type()),
            None => Ok(0),
        }
    }

    pub fn zadd(
        &mut self,
        key: impl Into<Vec<u8>>,
        score: f64,
        member: impl Into<Vec<u8>>,
        now_ms: u64,
    ) -> RedisResult<bool> {
        if score.is_nan() {
            return Err(LoomError::invalid("Redis sorted-set score must not be NaN"));
        }
        let key = key.into();
        self.ensure_missing_or_kind(&key, now_ms, RedisValueKind::SortedSet)?;
        if !matches!(
            self.live_key_kind(&key, now_ms),
            Some(RedisValueKind::SortedSet)
        ) {
            self.replace_key(key.clone(), RedisValueKind::SortedSet, None);
        }
        let member = member.into();
        let set = self.sorted_sets.entry(key).or_default();
        let inserted = set.scores.insert(member.clone(), score).is_none();
        if !inserted {
            set.by_score
                .retain(|entry| entry.member.as_slice() != member.as_slice());
        }
        set.by_score.insert(RedisScoreMember { score, member });
        Ok(inserted)
    }

    pub fn zscore(&self, key: &[u8], member: &[u8], now_ms: u64) -> RedisResult<Option<f64>> {
        match self.live_meta(key, now_ms) {
            Some(meta) if meta.kind == RedisValueKind::SortedSet => Ok(self
                .sorted_sets
                .get(key)
                .and_then(|set| set.scores.get(member))
                .copied()),
            Some(_) => Err(wrong_type()),
            None => Ok(None),
        }
    }

    pub fn zcard(&self, key: &[u8], now_ms: u64) -> RedisResult<usize> {
        match self.live_meta(key, now_ms) {
            Some(meta) if meta.kind == RedisValueKind::SortedSet => Ok(self
                .sorted_sets
                .get(key)
                .map(|set| set.scores.len())
                .unwrap_or_default()),
            Some(_) => Err(wrong_type()),
            None => Ok(0),
        }
    }

    pub fn zentries(&self, key: &[u8], now_ms: u64) -> RedisResult<Vec<(Vec<u8>, f64)>> {
        match self.live_meta(key, now_ms) {
            Some(meta) if meta.kind == RedisValueKind::SortedSet => Ok(self
                .sorted_sets
                .get(key)
                .map(RedisSortedSet::entries)
                .unwrap_or_default()),
            Some(_) => Err(wrong_type()),
            None => Ok(Vec::new()),
        }
    }

    pub fn ensure_stream(&mut self, key: impl Into<Vec<u8>>, now_ms: u64) -> RedisResult<bool> {
        let key = key.into();
        self.ensure_missing_or_kind(&key, now_ms, RedisValueKind::Stream)?;
        if matches!(
            self.live_key_kind(&key, now_ms),
            Some(RedisValueKind::Stream)
        ) {
            return Ok(false);
        }
        self.replace_key(key, RedisValueKind::Stream, None);
        Ok(true)
    }

    pub fn put_list_node(
        &mut self,
        key: impl Into<Vec<u8>>,
        index: i64,
        value: impl Into<Vec<u8>>,
        now_ms: u64,
    ) -> RedisResult<()> {
        let key = key.into();
        self.ensure_missing_or_kind(&key, now_ms, RedisValueKind::List)?;
        if !matches!(self.live_key_kind(&key, now_ms), Some(RedisValueKind::List)) {
            self.replace_key(key.clone(), RedisValueKind::List, None);
        }
        self.lists
            .entry(key)
            .or_default()
            .insert(index, value.into());
        Ok(())
    }

    pub fn list_nodes(&self, key: &[u8], now_ms: u64) -> RedisResult<Vec<(i64, Vec<u8>)>> {
        match self.live_meta(key, now_ms) {
            Some(meta) if meta.kind == RedisValueKind::List => Ok(self
                .lists
                .get(key)
                .map(RedisList::nodes)
                .unwrap_or_default()),
            Some(_) => Err(wrong_type()),
            None => Ok(Vec::new()),
        }
    }

    fn live_meta(&self, key: &[u8], now_ms: u64) -> Option<&RedisKeyMeta> {
        self.catalog
            .get(key)
            .filter(|meta| !is_expired_at(meta.expires_at_ms, now_ms))
    }

    fn ensure_missing_or_kind(
        &self,
        key: &[u8],
        now_ms: u64,
        kind: RedisValueKind,
    ) -> RedisResult<()> {
        match self.live_key_kind(key, now_ms) {
            Some(live_kind) if live_kind != kind => Err(wrong_type()),
            _ => Ok(()),
        }
    }

    fn replace_key(
        &mut self,
        key: Vec<u8>,
        kind: RedisValueKind,
        expires_at_ms: Option<u64>,
    ) -> u64 {
        if let Some(previous) = self.catalog.remove(&key) {
            self.remove_expiry_index(&key, previous.expires_at_ms);
            self.clear_value_for_key(&key, previous.kind);
        }
        let revision = self.next_revision();
        if let Some(expires_at_ms) = expires_at_ms {
            self.expiry_index.insert((expires_at_ms, key.clone()), ());
        }
        self.catalog.insert(
            key,
            RedisKeyMeta {
                kind,
                expires_at_ms,
                revision,
            },
        );
        revision
    }

    fn clear_value_for_key(&mut self, key: &[u8], kind: RedisValueKind) {
        match kind {
            RedisValueKind::String => {
                self.strings.remove(key);
            }
            RedisValueKind::Hash => {
                self.hashes.remove(key);
            }
            RedisValueKind::Set => {
                self.sets.remove(key);
            }
            RedisValueKind::List => {
                self.lists.remove(key);
            }
            RedisValueKind::SortedSet => {
                self.sorted_sets.remove(key);
            }
            RedisValueKind::Stream => {}
        }
    }

    fn remove_expiry_index(&mut self, key: &[u8], expires_at_ms: Option<u64>) {
        if let Some(expires_at_ms) = expires_at_ms {
            self.expiry_index.remove(&(expires_at_ms, key.to_vec()));
        }
    }

    fn next_revision(&mut self) -> u64 {
        let revision = self.next_revision;
        self.next_revision = self.next_revision.saturating_add(1);
        revision
    }
}

fn is_expired_at(expires_at_ms: Option<u64>, now_ms: u64) -> bool {
    expires_at_ms.is_some_and(|expires_at_ms| expires_at_ms <= now_ms)
}

fn wrong_type() -> LoomError {
    LoomError::invalid("WRONGTYPE Operation against a key holding the wrong kind of value")
}

#[derive(Clone, Debug, Default)]
struct RedisList {
    items: BTreeMap<i64, Vec<u8>>,
    next_head: i64,
    next_tail: i64,
}

impl RedisList {
    fn push_front(&mut self, value: Vec<u8>) -> RedisListPush {
        self.next_head = self.next_head.saturating_sub(1);
        let index = self.next_head;
        self.items.insert(index, value);
        RedisListPush {
            index,
            len: self.len(),
        }
    }

    fn push_back(&mut self, value: Vec<u8>) -> RedisListPush {
        let index = self.next_tail;
        self.items.insert(index, value);
        self.next_tail = self.next_tail.saturating_add(1);
        RedisListPush {
            index,
            len: self.len(),
        }
    }

    fn pop_front(&mut self) -> Option<(i64, Vec<u8>)> {
        let index = self.items.first_key_value().map(|(index, _)| *index)?;
        let value = self.items.remove(&index)?;
        if self.items.is_empty() {
            self.next_head = 0;
            self.next_tail = 0;
        }
        Some((index, value))
    }

    fn pop_back(&mut self) -> Option<(i64, Vec<u8>)> {
        let index = self.items.last_key_value().map(|(index, _)| *index)?;
        let value = self.items.remove(&index)?;
        if self.items.is_empty() {
            self.next_head = 0;
            self.next_tail = 0;
        }
        Some((index, value))
    }

    fn insert(&mut self, index: i64, value: Vec<u8>) {
        let was_empty = self.items.is_empty();
        self.items.insert(index, value);
        if was_empty {
            self.next_head = index;
            self.next_tail = index.saturating_add(1);
        } else {
            self.next_head = self.next_head.min(index);
            self.next_tail = self.next_tail.max(index.saturating_add(1));
        }
    }

    fn len(&self) -> usize {
        self.items.len()
    }

    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn nodes(&self) -> Vec<(i64, Vec<u8>)> {
        self.items
            .iter()
            .map(|(index, value)| (*index, value.clone()))
            .collect()
    }
}

#[derive(Clone, Debug, Default)]
struct RedisSortedSet {
    scores: BTreeMap<Vec<u8>, f64>,
    by_score: BTreeSet<RedisScoreMember>,
}

impl RedisSortedSet {
    fn entries(&self) -> Vec<(Vec<u8>, f64)> {
        self.scores
            .iter()
            .map(|(member, score)| (member.clone(), *score))
            .collect()
    }
}

#[derive(Clone, Debug)]
struct RedisScoreMember {
    score: f64,
    member: Vec<u8>,
}

impl PartialEq for RedisScoreMember {
    fn eq(&self, other: &Self) -> bool {
        self.score.total_cmp(&other.score) == Ordering::Equal && self.member == other.member
    }
}

impl Eq for RedisScoreMember {}

impl PartialOrd for RedisScoreMember {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RedisScoreMember {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| self.member.cmp(&other.member))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_expiry_filters_reads_counts_and_sweeps_later() {
        let mut keyspace = RedisKeyspace::new(RedisPersistenceMode::Versioned);
        keyspace.set_string(b"live".to_vec(), b"one".to_vec(), Some(200));
        keyspace.set_string(b"expired".to_vec(), b"two".to_vec(), Some(100));

        assert_eq!(keyspace.key_count_live(99), 2);
        assert_eq!(keyspace.key_count_live(100), 1);
        assert_eq!(keyspace.get_string(b"expired", 100).unwrap(), None);
        assert!(!keyspace.catalog.is_empty());

        assert_eq!(keyspace.sweep_expired(100), 1);
        assert!(!keyspace.catalog.contains_key(b"expired".as_slice()));
        assert_eq!(keyspace.key_count_live(100), 1);
    }

    #[test]
    fn ttl_persist_and_expire_report_live_view() {
        let mut keyspace = RedisKeyspace::new(RedisPersistenceMode::Versioned);
        keyspace.set_string(b"k".to_vec(), b"v".to_vec(), None);

        assert_eq!(keyspace.ttl_ms(b"k", 10), RedisTtl::Persistent);
        assert!(keyspace.expire_at(b"k", 110, 10));
        assert_eq!(keyspace.ttl_ms(b"k", 10), RedisTtl::RemainingMs(100));
        assert!(keyspace.persist(b"k", 10));
        assert_eq!(keyspace.ttl_ms(b"k", 110), RedisTtl::Persistent);
        assert!(!keyspace.persist(b"k", 10));
        assert!(!keyspace.expire_at(b"missing", 120, 10));

        assert!(keyspace.expire_at(b"k", 110, 10));
        assert_eq!(keyspace.ttl_ms(b"k", 110), RedisTtl::NoKey);
        assert!(!keyspace.persist(b"k", 110));
    }

    #[test]
    fn hash_fields_are_independent_and_wrong_type_is_rejected() {
        let mut keyspace = RedisKeyspace::new(RedisPersistenceMode::Versioned);

        assert!(
            keyspace
                .hset(b"user".to_vec(), b"name".to_vec(), b"ada".to_vec(), 0)
                .unwrap()
        );
        assert!(
            keyspace
                .hset(b"user".to_vec(), b"role".to_vec(), b"admin".to_vec(), 0)
                .unwrap()
        );
        assert!(
            !keyspace
                .hset(b"user".to_vec(), b"role".to_vec(), b"owner".to_vec(), 0)
                .unwrap()
        );
        assert_eq!(keyspace.hlen(b"user", 0).unwrap(), 2);
        assert_eq!(
            keyspace.hget(b"user", b"name", 0).unwrap(),
            Some(b"ada".as_slice())
        );
        assert_eq!(
            keyspace.hget(b"user", b"role", 0).unwrap(),
            Some(b"owner".as_slice())
        );

        keyspace.set_string(b"name".to_vec(), b"ada".to_vec(), None);
        assert!(
            keyspace
                .hset(b"name".to_vec(), b"field".to_vec(), b"value".to_vec(), 0)
                .is_err()
        );
    }

    #[test]
    fn set_members_are_independent_and_counts_are_live() {
        let mut keyspace = RedisKeyspace::new(RedisPersistenceMode::Versioned);

        assert!(keyspace.sadd(b"tags".to_vec(), b"red".to_vec(), 0).unwrap());
        assert!(
            keyspace
                .sadd(b"tags".to_vec(), b"blue".to_vec(), 0)
                .unwrap()
        );
        assert!(
            !keyspace
                .sadd(b"tags".to_vec(), b"blue".to_vec(), 0)
                .unwrap()
        );
        assert!(keyspace.sismember(b"tags", b"red", 0).unwrap());
        assert!(!keyspace.sismember(b"tags", b"green", 0).unwrap());
        assert_eq!(keyspace.scard(b"tags", 0).unwrap(), 2);

        assert!(keyspace.expire_at(b"tags", 5, 0));
        assert_eq!(keyspace.scard(b"tags", 5).unwrap(), 0);
        assert_eq!(keyspace.key_count_live(5), 0);
    }

    #[test]
    fn setting_new_type_clears_old_structure() {
        let mut keyspace = RedisKeyspace::new(RedisPersistenceMode::Versioned);

        keyspace
            .hset(b"k".to_vec(), b"field".to_vec(), b"value".to_vec(), 0)
            .unwrap();
        assert_eq!(keyspace.hlen(b"k", 0).unwrap(), 1);
        keyspace.set_string(b"k".to_vec(), b"value".to_vec(), None);

        assert_eq!(
            keyspace.get_string(b"k", 0).unwrap(),
            Some(b"value".as_slice())
        );
        assert!(keyspace.hget(b"k", b"field", 0).is_err());
        assert!(!keyspace.hashes.contains_key(b"k".as_slice()));
    }

    #[test]
    fn list_push_pop_and_counts_are_live() {
        let mut keyspace = RedisKeyspace::new(RedisPersistenceMode::Versioned);

        assert_eq!(
            keyspace.rpush(b"jobs".to_vec(), b"b".to_vec(), 0).unwrap(),
            1
        );
        assert_eq!(
            keyspace.lpush(b"jobs".to_vec(), b"a".to_vec(), 0).unwrap(),
            2
        );
        assert_eq!(keyspace.llen(b"jobs", 0).unwrap(), 2);
        assert_eq!(keyspace.lpop(b"jobs", 0).unwrap(), Some(b"a".to_vec()));
        assert_eq!(keyspace.rpop(b"jobs", 0).unwrap(), Some(b"b".to_vec()));
        assert_eq!(keyspace.rpop(b"jobs", 0).unwrap(), None);

        keyspace.set_string(b"name".to_vec(), b"ada".to_vec(), None);
        assert!(keyspace.lpush(b"name".to_vec(), b"x".to_vec(), 0).is_err());
    }

    #[test]
    fn sorted_set_tracks_member_lookup_and_score_order() {
        let mut keyspace = RedisKeyspace::new(RedisPersistenceMode::Versioned);

        assert!(
            keyspace
                .zadd(b"scores".to_vec(), 2.0, b"b".to_vec(), 0)
                .unwrap()
        );
        assert!(
            keyspace
                .zadd(b"scores".to_vec(), 1.0, b"a".to_vec(), 0)
                .unwrap()
        );
        assert!(
            !keyspace
                .zadd(b"scores".to_vec(), 3.0, b"a".to_vec(), 0)
                .unwrap()
        );
        assert_eq!(keyspace.zcard(b"scores", 0).unwrap(), 2);
        assert_eq!(keyspace.zscore(b"scores", b"a", 0).unwrap(), Some(3.0));
        assert!(
            keyspace
                .zadd(b"scores".to_vec(), f64::NAN, b"x".to_vec(), 0)
                .is_err()
        );

        keyspace.set_string(b"name".to_vec(), b"ada".to_vec(), None);
        assert!(
            keyspace
                .zadd(b"name".to_vec(), 1.0, b"x".to_vec(), 0)
                .is_err()
        );
    }
}
