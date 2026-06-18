use super::*;

#[derive(Default)]
struct SearchDiffSnapshot {
    mapping: Option<Vec<u8>>,
    docs: BTreeMap<Vec<u8>, Digest>,
}

impl<S: ObjectStore> Loom<S> {
    /// First-parent commit list for `branch`, newest first.
    pub fn log(&self, ns: WorkspaceId, branch: &str) -> Result<Vec<Digest>> {
        self.authorize_ref(ns, &format!("branch/{branch}"), AclRight::Read)?;
        self.log_unchecked(ns, branch)
    }

    pub(crate) fn log_unchecked(&self, ns: WorkspaceId, branch: &str) -> Result<Vec<Digest>> {
        let mut out = Vec::new();
        let mut cur = self.registry.branch_tip(ns, branch)?;
        while let Some(d) = cur {
            out.push(d);
            cur = self.get_commit(d)?.parents.first().copied();
        }
        Ok(out)
    }

    /// Path-level diff between two commits visible from `ns`, sorted by path.
    pub fn diff(&self, ns: WorkspaceId, from: Digest, to: Digest) -> Result<Vec<Change>> {
        self.authorize(ns, FacetKind::Vcs, AclRight::Read)?;
        self.require_commit_visible_in_workspace(ns, from)?;
        self.require_commit_visible_in_workspace(ns, to)?;
        let (f, _) = self.flatten_commit(from)?;
        let (t, _) = self.flatten_commit(to)?;
        let mut paths: BTreeSet<String> = BTreeSet::new();
        paths.extend(f.keys().cloned());
        paths.extend(t.keys().cloned());
        let mut changes = Vec::new();
        for path in paths {
            let kind = match (f.get(&path), t.get(&path)) {
                (None, Some(_)) => ChangeKind::Added,
                (Some(_), None) => ChangeKind::Deleted,
                (Some(a), Some(b)) if a != b => ChangeKind::Modified,
                _ => continue,
            };
            changes.push(Change { path, kind });
        }
        Ok(changes)
    }

    /// Structural commit diff grouped by facet and collection.
    pub fn diff_commits(&self, ns: WorkspaceId, from: Digest, to: Digest) -> Result<Vec<u8>> {
        self.authorize(ns, FacetKind::Vcs, AclRight::Read)?;
        self.require_commit_visible_in_workspace(ns, from)?;
        self.require_commit_visible_in_workspace(ns, to)?;
        let (before, _) = self.flatten_commit(from)?;
        let (after, _) = self.flatten_commit(to)?;
        let mut paths: BTreeSet<String> = BTreeSet::new();
        paths.extend(before.keys().cloned());
        paths.extend(after.keys().cloned());

        let mut envelope = DiffEnvelope::default();
        for path in paths {
            let lhs = before.get(&path);
            let rhs = after.get(&path);
            if lhs == rhs {
                continue;
            }
            let diff_path = classify_diff_path(&path, self.store.digest_algo());
            if let Some(facet) = diff_path.facet_kind()
                && !self.facet_diff_details_allowed(ns, facet)?
            {
                envelope.add_coarse(facet.as_str(), Vec::new(), simple_change(lhs, rhs), false);
                continue;
            }
            match diff_path {
                DiffPath::SqlTable { db, table } => {
                    self.add_table_diff(&mut envelope, ns, from, to, &path, db, table)?;
                }
                DiffPath::QueueStream { stream } => {
                    self.add_queue_diff(&mut envelope, &stream, lhs, rhs)?;
                }
                DiffPath::KvCollection { collection } => {
                    self.add_kv_diff(&mut envelope, &before, &after, &collection, lhs, rhs)?;
                }
                DiffPath::DocumentCollection { collection } => {
                    self.add_document_diff(&mut envelope, &before, &after, &collection, lhs, rhs)?;
                }
                DiffPath::SearchCollection { collection } => {
                    self.add_search_diff(&mut envelope, &before, &after, &collection, lhs, rhs)?;
                }
                DiffPath::CasDigest { digest } => {
                    self.add_simple_unit_diff(
                        &mut envelope,
                        "cas",
                        Vec::new(),
                        "digest",
                        key_digest(digest),
                        lhs,
                        rhs,
                    );
                }
                DiffPath::Calendar {
                    principal,
                    collection,
                    uid,
                } => {
                    self.add_simple_unit_diff(
                        &mut envelope,
                        "calendar",
                        vec![principal, collection],
                        "event",
                        key_text(&uid),
                        lhs,
                        rhs,
                    );
                }
                DiffPath::Contacts {
                    principal,
                    book,
                    uid,
                } => {
                    self.add_simple_unit_diff(
                        &mut envelope,
                        "contacts",
                        vec![principal, book],
                        "contact",
                        key_text(&uid),
                        lhs,
                        rhs,
                    );
                }
                DiffPath::Mail {
                    principal,
                    mailbox,
                    unit_kind,
                    uid,
                } => {
                    self.add_simple_unit_diff(
                        &mut envelope,
                        "mail",
                        vec![principal, mailbox],
                        unit_kind,
                        key_text(&uid),
                        lhs,
                        rhs,
                    );
                }
                DiffPath::VectorEntry { set, id } => {
                    self.add_simple_unit_diff(
                        &mut envelope,
                        "vector",
                        set,
                        "vector",
                        key_text(&id),
                        lhs,
                        rhs,
                    );
                }
                DiffPath::CoarseFacet { facet, collection } => {
                    envelope.add_coarse(&facet, collection, simple_change(lhs, rhs), false);
                }
                DiffPath::Ignored => {}
                DiffPath::File { collection } => {
                    self.add_simple_unit_diff(
                        &mut envelope,
                        "files",
                        collection,
                        "path",
                        key_text(&path),
                        lhs,
                        rhs,
                    );
                }
            }
        }
        Ok(envelope.encode(ns, from, to))
    }

    fn facet_diff_details_allowed(&self, ns: WorkspaceId, facet: FacetKind) -> Result<bool> {
        match self.authorize(ns, facet, AclRight::Read) {
            Ok(()) => Ok(true),
            Err(err) if err.code == Code::PermissionDenied => Ok(false),
            Err(err) => Err(err),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add_table_diff(
        &self,
        envelope: &mut DiffEnvelope,
        ns: WorkspaceId,
        from: Digest,
        to: Digest,
        path: &str,
        db: String,
        table: String,
    ) -> Result<()> {
        let from_parts = self.table_parts_at(from, path)?;
        let to_parts = self.table_parts_at(to, path)?;
        let schema_addr = to_parts
            .map(|(schema, _)| schema)
            .or(from_parts.map(|(schema, _)| schema))
            .ok_or_else(|| LoomError::not_found(format!("table {path:?} in neither commit")))?;
        let schema = tabular::Schema::decode(&self.load_content(schema_addr)?)?;
        for diff in self.diff_table(ns, path, from, to)? {
            match diff {
                RowDiff::Added(row) => {
                    let after = Some(self.row_payload_digest(&row));
                    envelope.add_unit(
                        "sql",
                        vec![db.clone(), table.clone()],
                        DiffUnitChange::new("row", row_key(&schema, &row), "added", None, after),
                    );
                }
                RowDiff::Removed(row) => {
                    let before = Some(self.row_payload_digest(&row));
                    envelope.add_unit(
                        "sql",
                        vec![db.clone(), table.clone()],
                        DiffUnitChange::new("row", row_key(&schema, &row), "removed", before, None),
                    );
                }
                RowDiff::Updated { from, to } => {
                    let before = Some(self.row_payload_digest(&from));
                    let after = Some(self.row_payload_digest(&to));
                    envelope.add_unit(
                        "sql",
                        vec![db.clone(), table.clone()],
                        DiffUnitChange::new("row", row_key(&schema, &to), "changed", before, after),
                    );
                }
            }
        }
        Ok(())
    }

    fn add_queue_diff(
        &self,
        envelope: &mut DiffEnvelope,
        stream: &str,
        lhs: Option<&StagedEntry>,
        rhs: Option<&StagedEntry>,
    ) -> Result<()> {
        let left = match lhs {
            Some(StagedEntry::Stream(root)) => Some(self.stream_root_parts(*root)?),
            Some(_) => return Err(LoomError::corrupt("queue path is not a stream")),
            None => None,
        };
        let right = match rhs {
            Some(StagedEntry::Stream(root)) => Some(self.stream_root_parts(*root)?),
            Some(_) => return Err(LoomError::corrupt("queue path is not a stream")),
            None => None,
        };
        match (left, right) {
            (Some((before_len, before_root)), Some((after_len, after_root)))
                if after_len >= before_len =>
            {
                let diff =
                    crate::prolly::diff(&self.store, before_root.as_ref(), after_root.as_ref())?;
                let mut pure_append = true;
                for (key, old, new) in diff {
                    let seq = stream_seq_key(&key)?;
                    if seq < before_len || old.is_some() || new.is_none() {
                        pure_append = false;
                        break;
                    }
                    let record = new.expect("checked above");
                    let payload = self.decode_stream_record(&record)?;
                    let after = Some(content_address_with(self.store.digest_algo(), &payload));
                    envelope.add_unit(
                        "queue",
                        vec![stream.to_string()],
                        DiffUnitChange::new("entry", key_uint(seq), "appended", None, after),
                    );
                }
                if !pure_append {
                    envelope.add_coarse("queue", vec![stream.to_string()], "changed", false);
                }
            }
            (None, Some((_, root))) => {
                let entries = match root {
                    Some(root) => crate::prolly::entries(&self.store, &root)?,
                    None => Vec::new(),
                };
                for (key, record) in entries {
                    let seq = stream_seq_key(&key)?;
                    let payload = self.decode_stream_record(&record)?;
                    let after = Some(content_address_with(self.store.digest_algo(), &payload));
                    envelope.add_unit(
                        "queue",
                        vec![stream.to_string()],
                        DiffUnitChange::new("entry", key_uint(seq), "appended", None, after),
                    );
                }
            }
            (Some(_), None) => {
                envelope.add_coarse("queue", vec![stream.to_string()], "removed", false);
            }
            (Some(_), Some(_)) => {
                envelope.add_coarse("queue", vec![stream.to_string()], "changed", false);
            }
            (None, None) => {}
        }
        Ok(())
    }

    fn add_kv_diff(
        &self,
        envelope: &mut DiffEnvelope,
        before_flat: &BTreeMap<String, StagedEntry>,
        after_flat: &BTreeMap<String, StagedEntry>,
        collection: &str,
        lhs: Option<&StagedEntry>,
        rhs: Option<&StagedEntry>,
    ) -> Result<()> {
        let left = self.kv_snapshot(before_flat, collection, lhs)?;
        let right = self.kv_snapshot(after_flat, collection, rhs)?;
        let mut keys = BTreeSet::new();
        keys.extend(left.iter().map(|(key, _)| key.clone()));
        keys.extend(right.iter().map(|(key, _)| key.clone()));
        for key in keys {
            let before = left
                .get(&key)
                .map(|value| content_address_with(self.store.digest_algo(), value));
            let after = right
                .get(&key)
                .map(|value| content_address_with(self.store.digest_algo(), value));
            if before == after {
                continue;
            }
            envelope.add_unit(
                "kv",
                vec![collection.to_string()],
                DiffUnitChange::new(
                    "key",
                    crate::kv::key_to_cbor(&key),
                    change_kind(before, after),
                    before,
                    after,
                ),
            );
        }
        Ok(())
    }

    fn add_document_diff(
        &self,
        envelope: &mut DiffEnvelope,
        before_flat: &BTreeMap<String, StagedEntry>,
        after_flat: &BTreeMap<String, StagedEntry>,
        collection: &str,
        lhs: Option<&StagedEntry>,
        rhs: Option<&StagedEntry>,
    ) -> Result<()> {
        if matches!(lhs, Some(StagedEntry::Document(_)) | None)
            && matches!(rhs, Some(StagedEntry::Document(_)) | None)
        {
            return self.add_prolly_document_diff(envelope, collection, lhs, rhs);
        }
        let left = self.document_snapshot(before_flat, collection, lhs)?;
        let right = self.document_snapshot(after_flat, collection, rhs)?;
        let mut ids = BTreeSet::new();
        ids.extend(left.ids().map(str::to_string));
        ids.extend(right.ids().map(str::to_string));
        for id in ids {
            let before = left
                .get(&id)
                .map(|value| content_address_with(self.store.digest_algo(), value));
            let after = right
                .get(&id)
                .map(|value| content_address_with(self.store.digest_algo(), value));
            if before == after {
                continue;
            }
            envelope.add_unit(
                "document",
                vec![collection.to_string()],
                DiffUnitChange::new(
                    "document",
                    key_text(&id),
                    change_kind(before, after),
                    before,
                    after,
                ),
            );
        }
        Ok(())
    }

    fn add_search_diff(
        &self,
        envelope: &mut DiffEnvelope,
        before_flat: &BTreeMap<String, StagedEntry>,
        after_flat: &BTreeMap<String, StagedEntry>,
        collection: &str,
        lhs: Option<&StagedEntry>,
        rhs: Option<&StagedEntry>,
    ) -> Result<()> {
        let left = self.search_snapshot(before_flat, collection, lhs)?;
        let right = self.search_snapshot(after_flat, collection, rhs)?;
        if left.mapping != right.mapping {
            let before = left
                .mapping
                .as_ref()
                .map(|value| content_address_with(self.store.digest_algo(), value));
            let after = right
                .mapping
                .as_ref()
                .map(|value| content_address_with(self.store.digest_algo(), value));
            envelope.add_unit(
                "search",
                vec![collection.to_string()],
                DiffUnitChange::new(
                    "mapping",
                    key_text("mapping"),
                    change_kind(before, after),
                    before,
                    after,
                ),
            );
        }
        let mut ids = BTreeSet::new();
        ids.extend(left.docs.keys().cloned());
        ids.extend(right.docs.keys().cloned());
        for id in ids {
            let before = left.docs.get(&id).copied();
            let after = right.docs.get(&id).copied();
            if before == after {
                continue;
            }
            envelope.add_unit(
                "search",
                vec![collection.to_string()],
                DiffUnitChange::new(
                    "document",
                    key_bytes(&id),
                    change_kind(before, after),
                    before,
                    after,
                ),
            );
        }
        Ok(())
    }

    fn add_prolly_document_diff(
        &self,
        envelope: &mut DiffEnvelope,
        collection: &str,
        lhs: Option<&StagedEntry>,
        rhs: Option<&StagedEntry>,
    ) -> Result<()> {
        let before_root = match lhs {
            Some(StagedEntry::Document(root)) => self.document_root_documents(*root)?,
            Some(_) => return Err(LoomError::corrupt("document path is not a document")),
            None => None,
        };
        let after_root = match rhs {
            Some(StagedEntry::Document(root)) => self.document_root_documents(*root)?,
            Some(_) => return Err(LoomError::corrupt("document path is not a document")),
            None => None,
        };
        for (key, before, after) in
            crate::prolly::diff(&self.store, before_root.as_ref(), after_root.as_ref())?
        {
            let document_id = DocumentId::decode(&key)?;
            let DocumentId::String(id) = document_id else {
                return Err(LoomError::corrupt("document map id is not string-backed"));
            };
            let before = before
                .as_deref()
                .map(|bytes| self.document_record_content_digest(bytes))
                .transpose()?;
            let after = after
                .as_deref()
                .map(|bytes| self.document_record_content_digest(bytes))
                .transpose()?;
            envelope.add_unit(
                "document",
                vec![collection.to_string()],
                DiffUnitChange::new(
                    "document",
                    key_text(&id),
                    change_kind(before, after),
                    before,
                    after,
                ),
            );
        }
        Ok(())
    }

    fn document_root_documents(&self, root: Digest) -> Result<Option<Digest>> {
        let Object::Tree(entries) = self.get_object(&root)? else {
            return Err(LoomError::corrupt("document root is not a Tree"));
        };
        let mut documents_root = None;
        for entry in entries {
            match entry.name.as_str() {
                "manifest" if entry.kind == EntryKind::Blob => {}
                "documents" if entry.kind == EntryKind::ProllyMap => {
                    documents_root = Some(entry.target);
                }
                _ => return Err(LoomError::corrupt("invalid document root entry")),
            }
        }
        Ok(documents_root)
    }

    fn kv_snapshot(
        &self,
        flat: &BTreeMap<String, StagedEntry>,
        collection: &str,
        entry: Option<&StagedEntry>,
    ) -> Result<KvMap> {
        match entry {
            Some(StagedEntry::File(file)) => crate::kv::decode_kv_storage_with_store(
                &self.store,
                self.store.digest_algo(),
                collection,
                &self.load_content(file.content_addr)?,
                |digest| {
                    self.content_at_path(
                        flat,
                        &crate::kv::structured_value_path(collection, digest),
                    )
                },
            ),
            Some(_) => Err(LoomError::corrupt("kv path is not a file")),
            None => Ok(KvMap::new()),
        }
    }

    fn document_snapshot(
        &self,
        flat: &BTreeMap<String, StagedEntry>,
        collection: &str,
        entry: Option<&StagedEntry>,
    ) -> Result<DocumentCollection> {
        match entry {
            Some(StagedEntry::File(file)) => self.structured_document_snapshot(
                flat,
                collection,
                &self.load_content(file.content_addr)?,
            ),
            Some(StagedEntry::Document(root)) => {
                self.prolly_document_snapshot(flat, collection, *root)
            }
            Some(_) => Err(LoomError::corrupt("document path is not a document")),
            None => Ok(DocumentCollection::new()),
        }
    }

    fn search_snapshot(
        &self,
        flat: &BTreeMap<String, StagedEntry>,
        collection: &str,
        entry: Option<&StagedEntry>,
    ) -> Result<SearchDiffSnapshot> {
        match entry {
            Some(StagedEntry::File(file)) => {
                let root = self.load_content(file.content_addr)?;
                parse_search_diff_snapshot(
                    &root,
                    collection,
                    flat,
                    self.store.digest_algo(),
                    |digest| self.load_content(digest),
                )
            }
            Some(_) => Err(LoomError::corrupt("search path is not a file")),
            None => Ok(SearchDiffSnapshot::default()),
        }
    }

    fn structured_document_snapshot(
        &self,
        flat: &BTreeMap<String, StagedEntry>,
        collection: &str,
        manifest_bytes: &[u8],
    ) -> Result<DocumentCollection> {
        let manifest = DocumentCollectionManifest::decode(manifest_bytes)?;
        if manifest.collection_id != collection {
            return Err(LoomError::corrupt(
                "document collection manifest id mismatch",
            ));
        }
        let map_bytes = self.content_at_path(flat, &document_map_diff_path(collection))?;
        if Digest::blake3(&map_bytes) != manifest.document_map_root {
            return Err(LoomError::corrupt("document map root mismatch"));
        }
        let mut fields = cbor::Fields::new(cbor::decode_array(&map_bytes)?);
        let schema = fields.text()?;
        if schema != "loom.document.map.v1" {
            return Err(LoomError::corrupt("unknown document map schema"));
        }
        let raw_entries = fields.array()?;
        fields.end()?;
        let mut out = DocumentCollection::new();
        for raw_entry in raw_entries {
            let mut entry = cbor::Fields::new(cbor::as_array(raw_entry)?);
            let document_id = DocumentId::from_cbor(entry.next_field()?)?;
            let record = DocumentRecord::from_cbor(entry.next_field()?)?;
            entry.end()?;
            if record.document_id != document_id {
                return Err(LoomError::corrupt("document map id mismatch"));
            }
            let id = match document_id {
                DocumentId::String(id) => id,
                _ => return Err(LoomError::corrupt("document map id is not string-backed")),
            };
            let digest = match record.body_ref {
                DocumentBodyRef::Direct { digest } => digest,
                DocumentBodyRef::Chunked { .. } => {
                    return Err(LoomError::unsupported(
                        "chunked document bodies are not supported by this storage engine",
                    ));
                }
            };
            let body = self.content_at_path(flat, &document_body_diff_path(collection, &digest))?;
            if body.len() as u64 != record.byte_length {
                return Err(LoomError::corrupt("document body length mismatch"));
            }
            if Digest::hash(self.store.digest_algo(), &body) != digest {
                return Err(LoomError::corrupt("document body digest mismatch"));
            }
            out.put(id, body);
        }
        Ok(out)
    }

    fn prolly_document_snapshot(
        &self,
        flat: &BTreeMap<String, StagedEntry>,
        collection: &str,
        root: Digest,
    ) -> Result<DocumentCollection> {
        let Object::Tree(entries) = self.get_object(&root)? else {
            return Err(LoomError::corrupt("document root is not a Tree"));
        };
        let mut manifest_addr = None;
        let mut documents_root = None;
        for entry in entries {
            match entry.name.as_str() {
                "manifest" if entry.kind == EntryKind::Blob => manifest_addr = Some(entry.target),
                "documents" if entry.kind == EntryKind::ProllyMap => {
                    documents_root = Some(entry.target);
                }
                _ => return Err(LoomError::corrupt("invalid document root entry")),
            }
        }
        let manifest = DocumentCollectionManifest::decode(&self.load_content(
            manifest_addr.ok_or_else(|| LoomError::corrupt("document root has no manifest"))?,
        )?)?;
        if manifest.collection_id != collection {
            return Err(LoomError::corrupt(
                "document collection manifest id mismatch",
            ));
        }
        let Some(documents_root) = documents_root else {
            return Ok(DocumentCollection::new());
        };
        if manifest.document_map_root != documents_root {
            return Err(LoomError::corrupt("document map root mismatch"));
        }
        let mut out = DocumentCollection::new();
        for (key, value) in crate::prolly::entries(&self.store, &documents_root)? {
            let document_id = DocumentId::decode(&key)?;
            let record = DocumentRecord::decode(&value)?;
            if record.document_id != document_id {
                return Err(LoomError::corrupt("document map id mismatch"));
            }
            let id = match document_id {
                DocumentId::String(id) => id,
                _ => return Err(LoomError::corrupt("document map id is not string-backed")),
            };
            let digest = match record.body_ref {
                DocumentBodyRef::Direct { digest } => digest,
                DocumentBodyRef::Chunked { .. } => {
                    return Err(LoomError::unsupported(
                        "chunked document bodies are not supported by this storage engine",
                    ));
                }
            };
            let body = self.content_at_path(flat, &document_body_diff_path(collection, &digest))?;
            if body.len() as u64 != record.byte_length {
                return Err(LoomError::corrupt("document body length mismatch"));
            }
            if Digest::hash(self.store.digest_algo(), &body) != digest {
                return Err(LoomError::corrupt("document body digest mismatch"));
            }
            out.put(id, body);
        }
        Ok(out)
    }

    fn content_at_path(&self, flat: &BTreeMap<String, StagedEntry>, path: &str) -> Result<Vec<u8>> {
        match flat.get(path) {
            Some(StagedEntry::File(file)) => self.load_content(file.content_addr),
            Some(_) => Err(LoomError::corrupt("document component path is not a file")),
            None => Err(LoomError::not_found(format!("document component {path:?}"))),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add_simple_unit_diff(
        &self,
        envelope: &mut DiffEnvelope,
        facet: &str,
        collection: Vec<String>,
        unit_kind: &str,
        unit_key: Vec<u8>,
        lhs: Option<&StagedEntry>,
        rhs: Option<&StagedEntry>,
    ) {
        envelope.add_unit(
            facet,
            collection,
            DiffUnitChange::new(
                unit_kind,
                unit_key,
                simple_change(lhs, rhs),
                entry_digest(lhs),
                entry_digest(rhs),
            ),
        );
    }

    fn row_payload_digest(&self, row: &Row) -> Digest {
        content_address_with(self.store.digest_algo(), &tabular::encode_row(row))
    }

    fn document_record_content_digest(&self, bytes: &[u8]) -> Result<Digest> {
        Digest::parse(&DocumentRecord::decode(bytes)?.record_revision)
    }

    /// Path-level **blame** for `branch`: each current path paired with the commit that last set its
    /// value. Walks first-parent history newest-first, diffing each commit's flattened tree against its
    /// parent's, and attributes every still-unattributed current path to the newest commit that changed
    /// it; paths present since the first commit are attributed to it. Returned in path order. This is
    /// the workspace/entry-level counterpart of [`Self::blame_table`] (which blames table rows).
    pub fn blame(&self, ns: WorkspaceId, branch: &str) -> Result<Vec<(String, Digest)>> {
        let log = self.log(ns, branch)?; // newest first; also authorizes Read
        let Some(&tip) = log.first() else {
            return Ok(Vec::new());
        };
        let (current, _) = self.flatten_commit(tip)?;
        let mut remaining: BTreeSet<String> = current.keys().cloned().collect();
        let mut blame: BTreeMap<String, Digest> = BTreeMap::new();
        for (i, &c) in log.iter().enumerate() {
            if remaining.is_empty() {
                break;
            }
            let (c_files, _) = self.flatten_commit(c)?;
            let p_files = match log.get(i + 1) {
                Some(&p) => self.flatten_commit(p)?.0,
                None => BTreeMap::new(),
            };
            // Paths whose value on `c` differs from its parent (added or modified) and that no newer
            // commit has claimed: `c` is the newest commit that set their current value.
            let mut claimed: Vec<String> = Vec::new();
            for (path, c_entry) in &c_files {
                if remaining.contains(path) && p_files.get(path) != Some(c_entry) {
                    claimed.push(path.clone());
                }
            }
            for path in claimed {
                remaining.remove(&path);
                blame.insert(path, c);
            }
        }
        let oldest = *log.last().expect("log is non-empty");
        let mut out = Vec::with_capacity(current.len());
        for path in current.keys() {
            let commit = blame.get(path).copied().unwrap_or(oldest);
            out.push((path.clone(), commit));
        }
        Ok(out)
    }

    // ---- merge ----------------------------------------------------------------------------------
}

fn parse_search_diff_snapshot(
    root: &[u8],
    collection: &str,
    flat: &BTreeMap<String, StagedEntry>,
    algo: crate::Algo,
    mut load_content: impl FnMut(Digest) -> Result<Vec<u8>>,
) -> Result<SearchDiffSnapshot> {
    let mut fields = cbor::Fields::new(cbor::decode_array(root)?);
    let schema = fields.text()?;
    if schema != crate::search::STRUCTURED_SEARCH_ROOT_SCHEMA {
        return Err(LoomError::corrupt("unknown search structured root schema"));
    }
    let root_algo = crate::digest::Algo::from_code(cbor::u8_from(fields.uint()?)?)?;
    if root_algo != algo {
        return Err(LoomError::corrupt(
            "search structured root digest profile mismatch",
        ));
    }
    let mapping = cbor::encode(&fields.next_field()?);
    let docs_raw = fields.array()?;
    fields.end()?;
    let mut docs = BTreeMap::new();
    for item in docs_raw {
        let mut entry = cbor::Fields::new(cbor::as_array(item)?);
        let id = entry.bytes()?;
        let digest = digest_from_search_root_bytes(algo, entry.bytes()?)?;
        let len = entry.uint()?;
        entry.end()?;
        let component = search_document_diff_path(collection, &digest);
        let Some(StagedEntry::File(file)) = flat.get(&component) else {
            return Err(LoomError::not_found(format!(
                "search document component {component:?}"
            )));
        };
        let bytes = load_content(file.content_addr)?;
        if bytes.len() as u64 != len {
            return Err(LoomError::integrity_failure(
                "search document length mismatch",
            ));
        }
        let actual = content_address_with(algo, &bytes);
        if actual != digest {
            return Err(LoomError::integrity_failure(
                "search document digest mismatch",
            ));
        }
        if docs.insert(id, digest).is_some() {
            return Err(LoomError::corrupt(
                "duplicate search structured root document id",
            ));
        }
    }
    Ok(SearchDiffSnapshot {
        mapping: Some(mapping),
        docs,
    })
}

fn digest_from_search_root_bytes(algo: crate::Algo, bytes: Vec<u8>) -> Result<Digest> {
    let bytes: [u8; crate::digest::DIGEST_LEN] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| LoomError::corrupt("search document digest field is not 32 bytes"))?;
    Ok(Digest::of(algo, bytes))
}

fn search_collection_key(collection: &str) -> String {
    hex::encode(collection.as_bytes())
}

fn search_document_diff_path(collection: &str, digest: &Digest) -> String {
    crate::workspace::facet_path(
        crate::workspace::FacetKind::Search,
        &format!(
            ".documents/{}/{}",
            search_collection_key(collection),
            digest.to_hex()
        ),
    )
}
