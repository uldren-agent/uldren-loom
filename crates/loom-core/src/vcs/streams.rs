use super::*;

impl<S: ObjectStore> Loom<S> {
    /// Append `entry` to the structured stream `name` in `ns`, storing the payload through the
    /// file-content path and inserting an entry record at the next sequence into the entry map. Returns
    /// the assigned zero-based sequence. Prior entry payloads are not decoded or re-encoded.
    pub fn stream_append(&mut self, ns: WorkspaceId, name: &str, entry: &[u8]) -> Result<usize> {
        self.authorize_collection(ns, FacetKind::Queue, name, AclRight::Write)?;
        let path = normalize_path(&stream_facet_path(name))?;
        let (length, entries_root) = match self.work.get(&ns).and_then(|w| w.get(&path)) {
            Some(StagedEntry::Stream(root)) => self.stream_root_parts(*root)?,
            Some(_) => return Err(LoomError::invalid(format!("{path:?} is not a stream"))),
            None => (0, None),
        };
        let payload_addr = self.store_content(ns, entry)?;
        let key = length.to_be_bytes();
        let record = encode_stream_record(payload_addr, entry.len() as u64);
        let new_entries =
            crate::prolly::insert(&mut self.store, entries_root.as_ref(), &key, &record)?;
        let root = self.build_stream_root(ns, length + 1, Some(new_entries))?;
        self.create_directory_reserved(
            ns,
            &crate::workspace::facet_root(crate::workspace::FacetKind::Queue),
            true,
        )?;
        self.work
            .entry(ns)
            .or_default()
            .insert(path, StagedEntry::Stream(root));
        Ok(length as usize)
    }

    /// The number of entries in the structured stream `name` in `ns`.
    pub fn stream_len(&self, ns: WorkspaceId, name: &str) -> Result<usize> {
        self.authorize_collection(ns, FacetKind::Queue, name, AclRight::Read)?;
        let length = self.stream_len_unchecked(ns, name)?;
        Ok(length as usize)
    }

    /// The payload at `seq` in the structured stream `name` in `ns`, or `None` if out of range.
    pub fn stream_get(&self, ns: WorkspaceId, name: &str, seq: usize) -> Result<Option<Vec<u8>>> {
        self.authorize_collection(ns, FacetKind::Queue, name, AclRight::Read)?;
        let (length, entries_root) = self.stream_root_parts(self.stream_root(ns, name)?)?;
        if seq as u64 >= length {
            return Ok(None);
        }
        let Some(root) = entries_root else {
            return Ok(None);
        };
        match crate::prolly::get(&self.store, &root, &(seq as u64).to_be_bytes())? {
            Some(record) => Ok(Some(self.decode_stream_record(&record)?)),
            None => Ok(None),
        }
    }

    /// The payloads with `lo <= seq < hi` (clamped to the stream) in the structured stream `name`,
    /// oldest first. Reads only the requested range from the entry map.
    pub fn stream_range(
        &self,
        ns: WorkspaceId,
        name: &str,
        lo: usize,
        hi: usize,
    ) -> Result<Vec<Vec<u8>>> {
        self.authorize_collection(ns, FacetKind::Queue, name, AclRight::Read)?;
        let (length, entries_root) = self.stream_root_parts(self.stream_root(ns, name)?)?;
        let hi = (hi as u64).min(length);
        let lo = (lo as u64).min(hi);
        let Some(root) = entries_root else {
            return Ok(Vec::new());
        };
        let lo_key = lo.to_be_bytes();
        let hi_key = hi.to_be_bytes();
        let mut cur = crate::prolly::ProllyCursor::open_range(
            &self.store,
            &root,
            Some(lo_key.as_slice()),
            Some(hi_key.to_vec()),
        )?;
        let mut out = Vec::new();
        while let Some((_, record)) = cur.next()? {
            out.push(self.decode_stream_record(&record)?);
        }
        Ok(out)
    }

    // ---- queue consumer offsets (operational metadata) ------------------------------------------

    /// The next sequence the named consumer should read from `stream` in `ns`. A consumer with no
    /// stored offset starts at `0`. Reading a position does not create or change any offset.
    pub fn consumer_position(
        &self,
        ns: WorkspaceId,
        stream: &str,
        consumer_id: &str,
    ) -> Result<u64> {
        validate_queue_stream_name(stream)?;
        self.consumer_position_internal(ns, stream, consumer_id)
    }

    /// Internal variant of [`Self::consumer_position`] for structurally-trusted, non-user-facing
    /// stream paths (e.g. chat profile operation logs, whose keys contain `/`). Enforces ACLs and
    /// consumer-id validity but skips the user-queue name-format policy, consistent with
    /// `stream_len` / `stream_range`, which already operate on such paths without that policy.
    #[doc(hidden)]
    pub fn consumer_position_internal(
        &self,
        ns: WorkspaceId,
        stream: &str,
        consumer_id: &str,
    ) -> Result<u64> {
        self.authorize_collection(ns, FacetKind::Queue, stream, AclRight::Read)?;
        validate_consumer_id(consumer_id)?;
        let next = self.consumer_offset(ns, stream, consumer_id);
        self.ensure_stream_position_retained(ns, stream, next)?;
        Ok(next)
    }

    /// Read up to `max` entries from `stream` in `ns` starting at the consumer's stored next sequence,
    /// oldest first. Does not advance the consumer's progress.
    pub fn consumer_read(
        &self,
        ns: WorkspaceId,
        stream: &str,
        consumer_id: &str,
        max: usize,
    ) -> Result<Vec<Vec<u8>>> {
        self.authorize_collection(ns, FacetKind::Queue, stream, AclRight::Read)?;
        validate_queue_stream_name(stream)?;
        validate_consumer_id(consumer_id)?;
        let next = self.consumer_offset(ns, stream, consumer_id);
        self.ensure_stream_position_retained(ns, stream, next)?;
        let lo = usize::try_from(next).unwrap_or(usize::MAX);
        let hi = lo.saturating_add(max);
        self.stream_range(ns, stream, lo, hi)
    }

    /// Advance the named consumer's next sequence to `next_seq`. Monotonic: a `next_seq` below the
    /// stored position is rejected, and a `next_seq` past the stream length is rejected.
    pub fn consumer_advance(
        &mut self,
        ns: WorkspaceId,
        stream: &str,
        consumer_id: &str,
        next_seq: u64,
    ) -> Result<()> {
        validate_queue_stream_name(stream)?;
        self.consumer_advance_internal(ns, stream, consumer_id, next_seq)
    }

    /// Internal variant of [`Self::consumer_advance`] for structurally-trusted, non-user-facing
    /// stream paths (e.g. chat profile operation logs). Enforces ACLs and consumer-id validity but
    /// skips the user-queue name-format policy, consistent with `stream_len` / `stream_range`.
    #[doc(hidden)]
    pub fn consumer_advance_internal(
        &mut self,
        ns: WorkspaceId,
        stream: &str,
        consumer_id: &str,
        next_seq: u64,
    ) -> Result<()> {
        self.authorize_collection(ns, FacetKind::Queue, stream, AclRight::Advance)?;
        validate_consumer_id(consumer_id)?;
        let length = self.stream_len_unchecked(ns, stream)?;
        self.ensure_stream_position_retained(ns, stream, next_seq)?;
        if next_seq > length {
            return Err(LoomError::invalid(format!(
                "next_seq {next_seq} is past the stream length {length}"
            )));
        }
        if next_seq < self.consumer_offset(ns, stream, consumer_id) {
            return Err(LoomError::invalid(
                "consumer_advance cannot move a consumer offset backward",
            ));
        }
        self.consumer_offsets
            .insert((ns, stream.to_string(), consumer_id.to_string()), next_seq);
        Ok(())
    }

    /// Set the named consumer's next sequence to `next_seq`, which may move backward. Rejects a
    /// `next_seq` past the stream length.
    pub fn consumer_reset(
        &mut self,
        ns: WorkspaceId,
        stream: &str,
        consumer_id: &str,
        next_seq: u64,
    ) -> Result<()> {
        self.authorize_collection(ns, FacetKind::Queue, stream, AclRight::Advance)?;
        validate_queue_stream_name(stream)?;
        validate_consumer_id(consumer_id)?;
        let length = self.stream_len_unchecked(ns, stream)?;
        self.ensure_stream_position_retained(ns, stream, next_seq)?;
        if next_seq > length {
            return Err(LoomError::invalid(format!(
                "next_seq {next_seq} is past the stream length {length}"
            )));
        }
        self.consumer_offsets
            .insert((ns, stream.to_string(), consumer_id.to_string()), next_seq);
        Ok(())
    }

    /// The stored next sequence for a consumer, or `0` when none is recorded.
    fn consumer_offset(&self, ns: WorkspaceId, stream: &str, consumer_id: &str) -> u64 {
        self.consumer_offsets
            .get(&(ns, stream.to_string(), consumer_id.to_string()))
            .copied()
            .unwrap_or(0)
    }

    /// The retained low-water mark for a stream. Consumer cursors below this value are stale.
    pub fn stream_retained_low_water_mark(&self, ns: WorkspaceId, stream: &str) -> Result<u64> {
        self.authorize_collection(ns, FacetKind::Queue, stream, AclRight::Read)?;
        validate_queue_stream_name(stream)?;
        Ok(self.stream_low_water_mark(ns, stream))
    }

    /// Advance the retained low-water mark for a stream. The mark is monotonic and may not pass the
    /// current stream length.
    pub fn stream_set_retained_low_water_mark(
        &mut self,
        ns: WorkspaceId,
        stream: &str,
        mark: u64,
    ) -> Result<()> {
        self.authorize_collection(ns, FacetKind::Queue, stream, AclRight::Advance)?;
        validate_queue_stream_name(stream)?;
        let length = self.stream_len_unchecked(ns, stream)?;
        if mark > length {
            return Err(LoomError::invalid(format!(
                "retained low-water mark {mark} is past the stream length {length}"
            )));
        }
        let current = self.stream_low_water_mark(ns, stream);
        if mark < current {
            return Err(LoomError::invalid(
                "retained low-water mark cannot move backward",
            ));
        }
        self.stream_low_water_marks
            .insert((ns, stream.to_string()), mark);
        Ok(())
    }

    fn stream_low_water_mark(&self, ns: WorkspaceId, stream: &str) -> u64 {
        self.stream_low_water_marks
            .get(&(ns, stream.to_string()))
            .copied()
            .unwrap_or(0)
    }

    fn ensure_stream_position_retained(
        &self,
        ns: WorkspaceId,
        stream: &str,
        next_seq: u64,
    ) -> Result<()> {
        let low_water = self.stream_low_water_mark(ns, stream);
        if next_seq < low_water {
            return Err(LoomError::retained_gap(format!(
                "queue stream {stream:?} cursor {next_seq} predates retained low-water mark {low_water}"
            )));
        }
        Ok(())
    }

    /// Stage `stream` under `name` in `ns` as a structured stream slot, building the entry map and
    /// stream root from its entries.
    pub fn stage_stream(
        &mut self,
        ns: WorkspaceId,
        name: &str,
        stream: &crate::log::Stream,
    ) -> Result<()> {
        self.authorize_collection(ns, FacetKind::Queue, name, AclRight::Write)?;
        let path = normalize_path(&stream_facet_path(name))?;
        let mut kv: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(stream.len());
        for (seq, payload) in stream.iter() {
            let payload_addr = self.store_content(ns, payload)?;
            kv.push((
                (seq as u64).to_be_bytes().to_vec(),
                encode_stream_record(payload_addr, payload.len() as u64),
            ));
        }
        let entries_root = crate::prolly::build(&mut self.store, &kv)?;
        let root = self.build_stream_root(ns, stream.len() as u64, entries_root)?;
        self.create_directory_reserved(
            ns,
            &crate::workspace::facet_root(crate::workspace::FacetKind::Queue),
            true,
        )?;
        self.work
            .entry(ns)
            .or_default()
            .insert(path, StagedEntry::Stream(root));
        Ok(())
    }

    /// Load the structured stream `name` from `ns` into an in-memory [`crate::log::Stream`], reading
    /// every entry in sequence order.
    pub fn load_stream(&self, ns: WorkspaceId, name: &str) -> Result<crate::log::Stream> {
        self.authorize_collection(ns, FacetKind::Queue, name, AclRight::Read)?;
        let (length, entries_root) = self.stream_root_parts(self.stream_root(ns, name)?)?;
        let mut stream = crate::log::Stream::new();
        if let Some(root) = entries_root {
            let mut cur = crate::prolly::ProllyCursor::open(&self.store, &root)?;
            while let Some((_, record)) = cur.next()? {
                stream.append(self.decode_stream_record(&record)?);
            }
        }
        if stream.len() as u64 != length {
            return Err(LoomError::corrupt(
                "stream length does not match entry-map count",
            ));
        }
        Ok(stream)
    }

    fn stream_len_unchecked(&self, ns: WorkspaceId, name: &str) -> Result<u64> {
        let (length, _) = self.stream_root_parts(self.stream_root(ns, name)?)?;
        Ok(length)
    }

    /// The stream-root `Tree` digest of the stream staged at `name` in `ns`.
    pub(crate) fn stream_root(&self, ns: WorkspaceId, name: &str) -> Result<Digest> {
        let path = normalize_path(&stream_facet_path(name))?;
        match self.work.get(&ns).and_then(|w| w.get(&path)) {
            Some(StagedEntry::Stream(root)) => Ok(*root),
            Some(_) => Err(LoomError::invalid(format!("{path:?} is not a stream"))),
            None => Err(LoomError::not_found(format!("stream {name:?} not staged"))),
        }
    }

    /// Build a stream-root `Tree` from the metadata (length plus the entry-map root) and return its
    /// digest. An empty stream omits the `entries` entry.
    fn build_stream_root(
        &mut self,
        ns: WorkspaceId,
        length: u64,
        entries_root: Option<Digest>,
    ) -> Result<Digest> {
        let entries_value = match entries_root {
            Some(d) => crate::cbor::digest_value(&d),
            None => crate::cbor::Value::Null,
        };
        let meta = crate::cbor::encode(&crate::cbor::Value::Array(vec![
            crate::cbor::Value::Uint(STREAM_META_VERSION),
            crate::cbor::Value::Uint(length),
            entries_value,
            crate::cbor::Value::Null,
        ]));
        let meta_addr = self.store_content(ns, &meta)?;
        let mut entries = vec![TreeEntry {
            name: "meta".to_string(),
            kind: EntryKind::Blob,
            target: meta_addr,
            mode: 0,
        }];
        if let Some(root) = entries_root {
            entries.push(TreeEntry {
                name: "entries".to_string(),
                kind: EntryKind::ProllyMap,
                target: root,
                mode: 0,
            });
        }
        self.put_object(&Object::tree(entries)?)
    }

    /// The `(length, entry-map root)` of a stream root `Tree`. Validates that `meta` is a Blob entry and
    /// that the `entries` entry's presence and kind agree with the metadata's entry-map root.
    pub(crate) fn stream_root_parts(&self, root: Digest) -> Result<(u64, Option<Digest>)> {
        let Object::Tree(entries) = self.get_object(&root)? else {
            return Err(LoomError::corrupt("stream root is not a Tree"));
        };
        let meta_entry = entries
            .iter()
            .find(|e| e.name == "meta")
            .ok_or_else(|| LoomError::corrupt("stream root has no meta entry"))?;
        if meta_entry.kind != EntryKind::Blob {
            return Err(LoomError::corrupt("stream meta entry is not a Blob"));
        }
        let entries_entry = entries.iter().find(|e| e.name == "entries");
        let meta = self.load_content(meta_entry.target)?;
        let mut f = crate::cbor::Fields::new(crate::cbor::decode_array(&meta)?);
        if f.uint()? != STREAM_META_VERSION {
            return Err(LoomError::corrupt("unsupported stream metadata version"));
        }
        let length = f.uint()?;
        let entries_root = match f.next_field()? {
            crate::cbor::Value::Null => None,
            v => Some(crate::cbor::as_digest(v)?),
        };
        let _consumers = f.next_field()?;
        f.end()?;
        match (entries_root, entries_entry) {
            (Some(meta_root), Some(entry)) => {
                if entry.kind != EntryKind::ProllyMap {
                    return Err(LoomError::corrupt(
                        "stream entries entry is not a ProllyMap",
                    ));
                }
                if entry.target != meta_root {
                    return Err(LoomError::corrupt(
                        "stream entries entry does not match the metadata entry-map root",
                    ));
                }
            }
            (Some(_), None) => {
                return Err(LoomError::corrupt(
                    "stream metadata has an entry-map root but the root Tree has no entries entry",
                ));
            }
            (None, Some(_)) => {
                return Err(LoomError::corrupt(
                    "empty stream metadata but the root Tree has an entries entry",
                ));
            }
            (None, None) => {}
        }
        Ok((length, entries_root))
    }

    /// Decode and verify one entry record, returning the payload bytes. Rejects a length or
    /// content-digest mismatch.
    pub(crate) fn decode_stream_record(&self, record: &[u8]) -> Result<Vec<u8>> {
        let mut f = crate::cbor::Fields::new(crate::cbor::decode_array(record)?);
        if f.uint()? != STREAM_ENTRY_VERSION {
            return Err(LoomError::corrupt(
                "unsupported stream entry-record version",
            ));
        }
        let payload_addr = f.digest()?;
        let payload_len = f.uint()?;
        f.end()?;
        let bytes = self.load_content(payload_addr)?;
        if bytes.len() as u64 != payload_len {
            return Err(LoomError::corrupt(
                "stream payload length does not match record",
            ));
        }
        if content_address_with(self.store.digest_algo(), &bytes) != payload_addr {
            return Err(LoomError::corrupt(
                "stream payload content digest does not match record",
            ));
        }
        Ok(bytes)
    }

    /// Every object a structured stream reaches: the root `Tree`, its metadata blob, the entry-map
    /// prolly nodes, and each entry's payload content (blobs or chunk lists). Pruned by `have`.
    pub(crate) fn stream_reachable(
        &self,
        root: Digest,
        have: &BTreeSet<Digest>,
    ) -> Result<BTreeSet<Digest>> {
        let mut out = BTreeSet::new();
        if have.contains(&root) {
            return Ok(out);
        }
        out.insert(root);
        let Object::Tree(entries) = self.get_object(&root)? else {
            return Err(LoomError::corrupt("stream root is not a Tree"));
        };
        for e in entries {
            match e.name.as_str() {
                "meta" => self.collect_content_objects(e.target, have, &mut out)?,
                "entries" => {
                    let reach = crate::prolly::reachable_with_leaves(&self.store, &e.target, have)?;
                    for node in reach.nodes {
                        out.insert(node);
                    }
                    for value in reach.leaf_values {
                        let mut f = crate::cbor::Fields::new(crate::cbor::decode_array(&value)?);
                        if f.uint()? != STREAM_ENTRY_VERSION {
                            return Err(LoomError::corrupt(
                                "unsupported stream entry-record version",
                            ));
                        }
                        let payload_addr = f.digest()?;
                        let _len = f.uint()?;
                        f.end()?;
                        self.collect_content_objects(payload_addr, have, &mut out)?;
                    }
                }
                "consumers" => {
                    for node in
                        crate::prolly::reachable_with_leaves(&self.store, &e.target, have)?.nodes
                    {
                        out.insert(node);
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }

    /// Resolve a content address to its stored object (blob or chunk list) and fold the object, plus any
    /// chunk blobs, into `out`, pruned by `have`.
    fn collect_content_objects(
        &self,
        addr: Digest,
        have: &BTreeSet<Digest>,
        out: &mut BTreeSet<Digest>,
    ) -> Result<()> {
        let obj = self
            .content
            .get(&addr)
            .copied()
            .ok_or_else(|| LoomError::not_found(format!("content {addr}")))?;
        if have.contains(&obj) || !out.insert(obj) {
            return Ok(());
        }
        if let Object::ChunkList { entries, .. } = self.get_object(&obj)? {
            for c in entries {
                if !have.contains(&c.target) {
                    out.insert(c.target);
                }
            }
        }
        Ok(())
    }
}
