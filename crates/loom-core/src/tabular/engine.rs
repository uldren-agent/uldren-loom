//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
impl<S: ObjectStore> Loom<S> {
    /// Stage `table` under `name` in `ns` as a structured table slot: build its `TABLE`-entry `Tree`
    /// (a `schema` Blob plus a `rows` prolly row-map root) and record it in the working tree. Commit,
    /// checkout, merge, diff, sync, and GC then handle the table through the object graph with
    /// row-level structural sharing across commits.
    pub fn stage_table(&mut self, ns: WorkspaceId, name: &str, table: &Table) -> Result<()> {
        let path = normalize_path(name)?;
        self.authorize_table(ns, &path, AclRight::Write)?;
        self.stage_table_reserved(ns, &path, table)
    }

    /// Privileged table staging for a component's reserved canonical storage.
    pub fn stage_table_reserved(
        &mut self,
        ns: WorkspaceId,
        name: &str,
        table: &Table,
    ) -> Result<()> {
        let path = normalize_path(name)?;
        // The schema rides the content-addressed Blob path (like a file), so its TABLE-Tree entry is a
        // normal content-address Blob entry that reachability/GC/sync resolve through the content map.
        let schema_addr = self.store_content(ns, &table.schema().encode())?;
        let rows_root = table.build_rows(self.store_mut())?;
        let index_roots = table.build_index_roots(self.store_mut())?;
        let tree = self.build_table_tree(schema_addr, rows_root, &index_roots)?;
        self.work
            .entry(ns)
            .or_default()
            .insert(path, StagedEntry::Table(tree));
        Ok(())
    }

    /// Insert or replace one `row` in the table staged at `name`, mutating its row map and every
    /// declared secondary index incrementally (`O(log n)` per structure via prolly mutation), then
    /// re-staging the table `Tree`. The result is identical to staging the equivalent full row set, but
    /// the cost scales with the change, not the table size. A `unique` index rejects a
    /// duplicate. Errors if no table is staged at `name`.
    pub fn insert_row(&mut self, ns: WorkspaceId, name: &str, row: Row) -> Result<()> {
        let path = normalize_path(name)?;
        self.authorize_table(ns, &path, AclRight::Write)?;
        self.insert_row_reserved(ns, &path, row)
    }

    /// Privileged incremental row mutation for a component's reserved canonical storage.
    pub fn insert_row_reserved(&mut self, ns: WorkspaceId, name: &str, row: Row) -> Result<()> {
        let path = normalize_path(name)?;
        let (schema_addr, schema, rows_root, index_roots) = self.table_mutation_parts(ns, &path)?;
        let (new_rows, new_index) =
            insert_row(self.store_mut(), &schema, rows_root, &index_roots, &row)?;
        self.restage_table(ns, path, &schema, schema_addr, new_rows, new_index)
    }

    /// Delete the row with primary key `pk` from the table staged at `name`, mutating its row map and
    /// every secondary index incrementally and re-staging the table `Tree`. A no-op if no such row.
    /// Errors if no table is staged at `name`.
    pub fn delete_row(&mut self, ns: WorkspaceId, name: &str, pk: &[Value]) -> Result<()> {
        let path = normalize_path(name)?;
        self.authorize_table(ns, &path, AclRight::Write)?;
        self.delete_row_reserved(ns, &path, pk)
    }

    /// Privileged incremental row deletion for a component's reserved canonical storage.
    pub fn delete_row_reserved(&mut self, ns: WorkspaceId, name: &str, pk: &[Value]) -> Result<()> {
        let path = normalize_path(name)?;
        let (schema_addr, schema, rows_root, index_roots) = self.table_mutation_parts(ns, &path)?;
        let (new_rows, new_index, _existed) =
            delete_row(self.store_mut(), &schema, rows_root, &index_roots, pk)?;
        self.restage_table(ns, path, &schema, schema_addr, new_rows, new_index)
    }

    /// The schema address, decoded schema, row-map root, and per-declared-index roots (aligned to
    /// `schema.indexes`) of the table staged at `path` - the inputs the incremental row mutators need.
    fn table_mutation_parts(&self, ns: WorkspaceId, path: &str) -> Result<TableMutationParts> {
        let tree = match self.work.get(&ns).and_then(|w| w.get(path)) {
            Some(StagedEntry::Table(t)) => *t,
            Some(_) => {
                return Err(LoomError::invalid(format!("{path:?} is not a table")));
            }
            None => return Err(LoomError::not_found(format!("table {path:?} not staged"))),
        };
        let (schema_addr, rows_root) = self.table_tree_parts(tree)?;
        let schema = Schema::decode(&self.load_content(schema_addr)?)?;
        let index_roots = schema
            .indexes
            .iter()
            .map(|idx| self.table_index_root(tree, &idx.name))
            .collect::<Result<Vec<_>>>()?;
        Ok((schema_addr, schema, rows_root, index_roots))
    }

    /// Rebuild a table `Tree` from incrementally-updated roots and record it in the working tree.
    fn restage_table(
        &mut self,
        ns: WorkspaceId,
        path: String,
        schema: &Schema,
        schema_addr: Digest,
        rows_root: Option<Digest>,
        index_roots: Vec<Option<Digest>>,
    ) -> Result<()> {
        let present: Vec<(String, Digest)> = schema
            .indexes
            .iter()
            .zip(index_roots)
            .filter_map(|(idx, root)| root.map(|d| (idx.name.clone(), d)))
            .collect();
        let tree = self.build_table_tree(schema_addr, rows_root, &present)?;
        self.work
            .entry(ns)
            .or_default()
            .insert(path, StagedEntry::Table(tree));
        Ok(())
    }

    /// Build a `TABLE`-entry `Tree` from a schema content address, an optional row-map root, and the
    /// secondary-index roots (`index/<name>`), and return its digest. An empty table omits the `rows`
    /// entry; an empty index omits its `index/<name>` entry. The single place the table-Tree shape is
    /// constructed (stage and merge both use it), so indexes commit/sync/GC with the row map.
    fn build_table_tree(
        &mut self,
        schema_addr: Digest,
        rows_root: Option<Digest>,
        index_roots: &[(String, Digest)],
    ) -> Result<Digest> {
        let mut entries = vec![TreeEntry {
            name: "schema".to_string(),
            kind: EntryKind::Blob,
            target: schema_addr,
            mode: 0,
        }];
        if let Some(root) = rows_root {
            entries.push(TreeEntry {
                name: "rows".to_string(),
                kind: EntryKind::ProllyMap,
                target: root,
                mode: 0,
            });
        }
        for (name, root) in index_roots {
            entries.push(TreeEntry {
                name: format!("index/{name}"),
                kind: EntryKind::ProllyMap,
                target: *root,
                mode: 0,
            });
        }
        self.put_object(&Object::tree(entries)?)
    }

    /// The `(schema content address, optional row-map root)` of a table `Tree`.
    fn table_tree_parts(&self, tree: Digest) -> Result<(Digest, Option<Digest>)> {
        let Object::Tree(entries) = self.get_object(&tree)? else {
            return Err(LoomError::corrupt("table entry target is not a Tree"));
        };
        let mut schema_addr = None;
        let mut rows_root = None;
        for e in entries {
            match e.name.as_str() {
                "schema" => schema_addr = Some(e.target),
                "rows" => rows_root = Some(e.target),
                _ => {}
            }
        }
        Ok((
            schema_addr.ok_or_else(|| LoomError::corrupt("table Tree has no schema entry"))?,
            rows_root,
        ))
    }

    /// Attempt a row-level 3-way merge of a table both sides changed. Returns the merged
    /// table-Tree digest on a clean merge, or `None` when the schemas differ or a row genuinely
    /// conflicts (the caller then reports the path as a conflict).
    pub(crate) fn try_row_merge_table(
        &mut self,
        base: Option<Digest>,
        ours: Digest,
        theirs: Digest,
        cell_level: bool,
    ) -> Result<Option<Digest>> {
        let (ours_schema, ours_rows) = self.table_tree_parts(ours)?;
        let (theirs_schema, theirs_rows) = self.table_tree_parts(theirs)?;
        if ours_schema != theirs_schema {
            return Ok(None); // schema changed on a side: not auto-mergeable here
        }
        let base_rows = match base {
            Some(b) => {
                let (base_schema, br) = self.table_tree_parts(b)?;
                if base_schema != ours_schema {
                    return Ok(None);
                }
                br
            }
            None => None,
        };
        let schema = Schema::decode(&self.load_content(ours_schema)?)?;
        // Row-level by default; cell-level reconciles per-column edits to the same row when requested.
        let outcome = if cell_level {
            merge_rows_cells(
                self.store_mut(),
                &schema,
                base_rows.as_ref(),
                ours_rows.as_ref(),
                theirs_rows.as_ref(),
            )?
        } else {
            merge_rows(
                self.store_mut(),
                &schema,
                base_rows.as_ref(),
                ours_rows.as_ref(),
                theirs_rows.as_ref(),
            )?
        };
        match outcome {
            TableMerge::Merged(root) => {
                // A clean row merge can still violate a `unique` index when both sides independently
                // add rows sharing a unique value; that is a conflict to resolve, not a hard error.
                if rows_violate_unique(self.store(), &schema, root.as_ref())? {
                    return Ok(None);
                }
                // The merged row set differs from either side, so rebuild the secondary indexes from
                // it; they commit in the same table Tree.
                let index_roots =
                    build_indexes_from_rows(self.store_mut(), &schema, root.as_ref())?;
                Ok(Some(self.build_table_tree(
                    ours_schema,
                    root,
                    &index_roots,
                )?))
            }
            TableMerge::Conflicts(_) => Ok(None),
        }
    }

    /// The `(schema content address, row-map root)` of the table at `path` in `commit`, or `None` if
    /// no table is committed there.
    pub(crate) fn table_parts_at(
        &self,
        commit: Digest,
        path: &str,
    ) -> Result<Option<(Digest, Option<Digest>)>> {
        self.table_tree_at(commit, path)?
            .map(|tree| self.table_tree_parts(tree))
            .transpose()
    }

    pub(crate) fn table_tree_at(&self, commit: Digest, path: &str) -> Result<Option<Digest>> {
        let (files, _) = self.flatten_commit(commit)?;
        match files.get(path) {
            Some(StagedEntry::Table(tree)) => Ok(Some(*tree)),
            _ => Ok(None),
        }
    }

    /// Row-level **blame** for the table at `path` on `branch`: each current row paired with
    /// the commit that last set its value. Walks first-parent history newest-first, diffing each
    /// commit's row map against its parent's (`O(changed)` per step), and attributes every
    /// still-unattributed current row to the newest commit that changed it; rows present since the
    /// first commit are attributed to it. Returned in primary-key order.
    pub fn blame_table(
        &self,
        ns: WorkspaceId,
        branch: &str,
        path: &str,
    ) -> Result<Vec<(Row, Digest)>> {
        let path = normalize_path(path)?;
        self.authorize_table(ns, &path, AclRight::Read)?;
        let log = self.log_unchecked(ns, branch)?; // newest first
        let Some(&tip) = log.first() else {
            return Ok(Vec::new());
        };
        let (schema_addr, tip_root) = self.table_parts_at(tip, &path)?.ok_or_else(|| {
            LoomError::not_found(format!("table {path:?} not found on {branch:?}"))
        })?;
        let schema = Schema::decode(&self.load_content(schema_addr)?)?;
        let current: BTreeMap<Vec<u8>, Vec<u8>> = match tip_root {
            Some(r) => crate::prolly::entries(self.store(), &r)?
                .into_iter()
                .collect(),
            None => BTreeMap::new(),
        };

        let mut remaining: BTreeSet<Vec<u8>> = current.keys().cloned().collect();
        let mut blame: BTreeMap<Vec<u8>, Digest> = BTreeMap::new();
        for (i, &c) in log.iter().enumerate() {
            if remaining.is_empty() {
                break;
            }
            let c_root = self.table_parts_at(c, &path)?.and_then(|(_, r)| r);
            let p_root = match log.get(i + 1) {
                Some(&p) => self.table_parts_at(p, &path)?.and_then(|(_, r)| r),
                None => None,
            };
            for (key, _p_val, c_val) in
                crate::prolly::diff(self.store(), p_root.as_ref(), c_root.as_ref())?
            {
                // A row present on `c`'s side that no newer commit has claimed: `c` is the newest
                // commit that set its current value.
                if c_val.is_some() && remaining.remove(&key) {
                    blame.insert(key, c);
                }
            }
        }

        let oldest = *log.last().expect("log is non-empty");
        let mut out = Vec::with_capacity(current.len());
        for (key, value) in current {
            let commit = blame.get(&key).copied().unwrap_or(oldest);
            out.push((decode_row(&schema, &value)?, commit));
        }
        Ok(out)
    }

    /// Row-level diff of the table at `path` between two commits (`from` -> `to`): the rows
    /// added/updated/removed, computed in `O(changed rows)` over the two prolly row maps via
    /// structural sharing. A side missing the table is treated as an empty row map (whole table
    /// added/removed). The schema is taken from `to` when present, else `from`.
    pub fn diff_table(
        &self,
        ns: WorkspaceId,
        path: &str,
        from: Digest,
        to: Digest,
    ) -> Result<Vec<RowDiff>> {
        let path = normalize_path(path)?;
        self.authorize_table(ns, &path, AclRight::Read)?;
        self.require_commit_visible_in_workspace(ns, from)?;
        self.require_commit_visible_in_workspace(ns, to)?;
        let from_parts = self.table_parts_at(from, &path)?;
        let to_parts = self.table_parts_at(to, &path)?;
        let schema_addr = to_parts
            .map(|(s, _)| s)
            .or(from_parts.map(|(s, _)| s))
            .ok_or_else(|| LoomError::not_found(format!("table {path:?} in neither commit")))?;
        let schema = Schema::decode(&self.load_content(schema_addr)?)?;
        let from_root = from_parts.and_then(|(_, r)| r);
        let to_root = to_parts.and_then(|(_, r)| r);
        diff_rows(self.store(), &schema, from_root.as_ref(), to_root.as_ref())
    }

    /// Schema-aware diff of the table at `path` between two commits. When the schema is stable, row
    /// records carry the same row changes as [`Loom::diff_table`]. Table create/drop records include
    /// both the schema change and the row additions/removals. A schema change between two existing
    /// tables reports only the schema record because old rows cannot be decoded with the new schema.
    pub fn diff_table_records(
        &self,
        ns: WorkspaceId,
        path: &str,
        from: Digest,
        to: Digest,
    ) -> Result<Vec<TableDiffRecord>> {
        let path = normalize_path(path)?;
        self.authorize_table(ns, &path, AclRight::Read)?;
        self.require_commit_visible_in_workspace(ns, from)?;
        self.require_commit_visible_in_workspace(ns, to)?;
        let from_parts = self.table_parts_at(from, &path)?;
        let to_parts = self.table_parts_at(to, &path)?;
        if from_parts.is_none() && to_parts.is_none() {
            return Err(LoomError::not_found(format!(
                "table {path:?} in neither commit"
            )));
        }

        let mut records = Vec::new();
        let from_schema = match from_parts {
            Some((addr, _)) => Some(Schema::decode(&self.load_content(addr)?)?),
            None => None,
        };
        let to_schema = match to_parts {
            Some((addr, _)) => Some(Schema::decode(&self.load_content(addr)?)?),
            None => None,
        };
        if from_schema != to_schema {
            records.push(TableDiffRecord::SchemaChanged {
                from: from_schema.clone(),
                to: to_schema.clone(),
            });
        }

        let schema = match (&from_schema, &to_schema) {
            (Some(from), Some(to)) if from != to => return Ok(records),
            (_, Some(schema)) | (Some(schema), None) => schema,
            (None, None) => unreachable!("handled above"),
        };
        let from_root = from_parts.and_then(|(_, r)| r);
        let to_root = to_parts.and_then(|(_, r)| r);
        records.extend(
            diff_rows(self.store(), schema, from_root.as_ref(), to_root.as_ref())?
                .into_iter()
                .map(TableDiffRecord::Row),
        );
        Ok(records)
    }

    pub(crate) fn require_commit_visible_in_workspace(
        &self,
        ns: WorkspaceId,
        commit: Digest,
    ) -> Result<()> {
        let mut tips = Vec::new();
        for branch in self.registry().branch_list(ns)? {
            if let Some(tip) = self.registry().branch_tip(ns, &branch)? {
                tips.push(tip);
            }
        }
        for tag in self.registry().tag_list(ns)? {
            if let Some(target) = self.registry().tag_target(ns, &tag)? {
                tips.push(target);
            }
        }
        if self.reachable(&tips, &BTreeSet::new())?.contains(&commit) {
            Ok(())
        } else {
            Err(LoomError::new(
                Code::PermissionDenied,
                "commit is not reachable from the workspace",
            ))
        }
    }

    /// Read a staged table by `name` from `ns`, materializing it from its `TABLE`-entry `Tree`.
    pub fn read_table(&self, ns: WorkspaceId, name: &str) -> Result<Table> {
        let path = normalize_path(name)?;
        self.authorize_table(ns, &path, AclRight::Read)?;
        self.read_table_reserved(ns, name)
    }

    /// Read a table from a historical commit without changing the workspace working tree.
    pub fn read_table_at(&self, ns: WorkspaceId, name: &str, commit: Digest) -> Result<Table> {
        let path = normalize_path(name)?;
        self.authorize_table(ns, &path, AclRight::Read)?;
        self.require_commit_visible_in_workspace(ns, commit)?;
        let (schema_addr, rows_root) = self
            .table_parts_at(commit, &path)?
            .ok_or_else(|| LoomError::not_found(format!("table {name:?} not found at {commit}")))?;
        let schema = Schema::decode(&self.load_content(schema_addr)?)?;
        match rows_root {
            Some(root) => Table::load_rows(self.store(), schema, &root),
            None => Ok(Table::new(schema)),
        }
    }

    /// Privileged table read for the SQL facet implementation reading its own reserved storage.
    pub fn read_table_reserved(&self, ns: WorkspaceId, name: &str) -> Result<Table> {
        let path = normalize_path(name)?;
        let tree = match self.work.get(&ns).and_then(|w| w.get(&path)) {
            Some(StagedEntry::Table(t)) => *t,
            Some(_) => {
                return Err(LoomError::invalid(format!("{path:?} is not a table")));
            }
            None => return Err(LoomError::not_found(format!("table {name:?} not staged"))),
        };
        let Object::Tree(entries) = self.get_object(&tree)? else {
            return Err(LoomError::corrupt("table entry target is not a Tree"));
        };
        let mut schema_addr = None;
        let mut rows_root = None;
        for e in entries {
            match e.name.as_str() {
                "schema" => schema_addr = Some(e.target),
                "rows" => rows_root = Some(e.target),
                _ => {} // index/<name> entries are ignored here
            }
        }
        let schema_addr =
            schema_addr.ok_or_else(|| LoomError::corrupt("table Tree has no schema entry"))?;
        let schema_bytes = self.load_content(schema_addr)?;
        let schema = Schema::decode(&schema_bytes)?;
        match rows_root {
            Some(root) => Table::load_rows(self.store(), schema, &root),
            None => Ok(Table::new(schema)),
        }
    }

    /// The schema and row-map root of the table staged at `name` in `ns`, **without** materializing
    /// any rows - the cheap read a lazy consumer needs to then stream rows on demand
    /// ([`RowCursor`]) or point-fetch one ([`Table::get_row`]). `Ok(None)`
    /// when no table is staged there; the inner `root` is `None` for a staged-but-empty table. This is
    /// the lazy counterpart of [`Loom::read_table`] (which loads the whole row set): the SQL base
    /// snapshot calls it once per table at open to capture `(schema, root)` and then reads against the
    /// owned store.
    pub fn table_reader(
        &self,
        ns: WorkspaceId,
        name: &str,
    ) -> Result<Option<(Schema, Option<Digest>)>> {
        let path = normalize_path(name)?;
        self.authorize_table(ns, &path, AclRight::Read)?;
        self.table_reader_reserved(ns, name)
    }

    /// Privileged lazy table reader for the SQL facet implementation reading its own reserved storage.
    pub fn table_reader_reserved(
        &self,
        ns: WorkspaceId,
        name: &str,
    ) -> Result<Option<(Schema, Option<Digest>)>> {
        let path = normalize_path(name)?;
        let tree = match self.work.get(&ns).and_then(|w| w.get(&path)) {
            Some(StagedEntry::Table(t)) => *t,
            Some(_) => {
                return Err(LoomError::invalid(format!("{path:?} is not a table")));
            }
            None => return Ok(None),
        };
        let (schema_addr, rows_root) = self.table_tree_parts(tree)?;
        let schema = Schema::decode(&self.load_content(schema_addr)?)?;
        Ok(Some((schema, rows_root)))
    }

    /// The `index/<index_name>` prolly-tree root of the table staged at `name` in `ns`, or `None` if
    /// that index has no entries (an empty table or an undeclared/never-built index). The lazy
    /// counterpart used by the SQL base snapshot to scan a durable secondary index on demand.
    pub fn table_index_reader(
        &self,
        ns: WorkspaceId,
        name: &str,
        index_name: &str,
    ) -> Result<Option<Digest>> {
        let path = normalize_path(name)?;
        self.authorize_table(ns, &path, AclRight::Read)?;
        self.table_index_reader_reserved(ns, name, index_name)
    }

    /// Privileged table-index reader for the SQL facet implementation reading its own reserved storage.
    pub fn table_index_reader_reserved(
        &self,
        ns: WorkspaceId,
        name: &str,
        index_name: &str,
    ) -> Result<Option<Digest>> {
        let path = normalize_path(name)?;
        let tree = match self.work.get(&ns).and_then(|w| w.get(&path)) {
            Some(StagedEntry::Table(t)) => *t,
            Some(_) => {
                return Err(LoomError::invalid(format!("{path:?} is not a table")));
            }
            None => return Ok(None),
        };
        self.table_index_root(tree, index_name)
    }

    /// The `TABLE`-entry `Tree` digest of the table staged at `name` in `ns`, or `None` if no table
    /// is staged there. Two equal tables stage to the same digest (content-addressed dedup); a changed
    /// table re-addresses, so this is the per-table granularity marker.
    pub fn staged_table_root(&self, ns: WorkspaceId, name: &str) -> Option<Digest> {
        let path = normalize_path(name).ok()?;
        match self.work.get(&ns).and_then(|w| w.get(&path)) {
            Some(StagedEntry::Table(t)) => Some(*t),
            _ => None,
        }
    }

    /// The collection names present in `ns` for `facet`, sorted and de-duplicated. A collection is the
    /// first path segment under the facet's reserved directory
    /// (`.loom/facets/<facet>/<collection>/...`), so this serves the flat facets (kv/document/
    /// time-series/ledger/queue) whose collection is a single segment and `sql` whose first segment is
    /// the database. The per-principal facets (calendar/contacts/mail) nest the collection under a
    /// principal and use their own listers instead.
    ///
    /// Reserved segments whose name starts with `.` are excluded: these are facet-internal
    /// implementation roots (for example the document structured root's `.maps`, `.bodies`,
    /// `.indexes`, and `.index-data`), not user-visible collections. This matches the reserved-name
    /// convention used by the per-facet listers (e.g. `document::doc_list_collections`).
    pub fn list_collections(&self, ns: WorkspaceId, facet: FacetKind) -> Vec<String> {
        let prefix = format!(
            "{}/{}/",
            crate::workspace::FACETS_RESERVED_DIR,
            facet.as_str()
        );
        let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        if let Some(work) = self.work.get(&ns) {
            for (path, _) in work.iter() {
                if let Some(rest) = path.strip_prefix(&prefix)
                    && let Some(seg) = rest.split('/').next()
                    && !seg.is_empty()
                    && !seg.starts_with('.')
                {
                    names.insert(seg.to_string());
                }
            }
        }
        names.into_iter().collect()
    }

    /// Names of the structured tables staged in `ns` (the working-tree slots that are tables), sorted.
    pub fn staged_tables(&self, ns: WorkspaceId) -> Vec<String> {
        self.work
            .get(&ns)
            .map(|w| {
                w.iter()
                    .filter(|(_, e)| matches!(e, StagedEntry::Table(_)))
                    .map(|(p, _)| p.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The `index/<index_name>` prolly-tree root of a table `Tree`, or `None` if that index has no
    /// entries (an empty table or an undeclared/never-built index).
    fn table_index_root(&self, tree: Digest, index_name: &str) -> Result<Option<Digest>> {
        let Object::Tree(entries) = self.get_object(&tree)? else {
            return Err(LoomError::corrupt("table entry target is not a Tree"));
        };
        let want = format!("index/{index_name}");
        Ok(entries
            .into_iter()
            .find(|e| e.name == want)
            .map(|e| e.target))
    }

    /// Look up rows of the table staged at `name` in `ns` through a declared secondary index, matching
    /// the index's leading columns against `values`. The index prolly
    /// tree (`index/<index_name>`) is prefix-scanned and each hit's primary key fetched from the row
    /// map, so the cost is `O(matches * log n)` rather than a full scan. `values` shorter than the
    /// index's columns is a leading-column range scan. Returns rows in index (then primary-key) order.
    pub fn index_scan(
        &self,
        ns: WorkspaceId,
        name: &str,
        index_name: &str,
        values: &[Value],
    ) -> Result<Vec<Row>> {
        let path = normalize_path(name)?;
        self.authorize_table(ns, &path, AclRight::Read)?;
        let tree = match self.work.get(&ns).and_then(|w| w.get(&path)) {
            Some(StagedEntry::Table(t)) => *t,
            Some(_) => {
                return Err(LoomError::invalid(format!("{path:?} is not a table")));
            }
            None => return Err(LoomError::not_found(format!("table {name:?} not staged"))),
        };
        let (schema_addr, rows_root) = self.table_tree_parts(tree)?;
        let schema = Schema::decode(&self.load_content(schema_addr)?)?;
        let idx = schema
            .index(index_name)
            .ok_or_else(|| LoomError::not_found(format!("no index {index_name:?} on {name:?}")))?;
        if values.len() > idx.columns.len() {
            return Err(LoomError::invalid(format!(
                "index {index_name:?} has {} columns, got {} lookup values",
                idx.columns.len(),
                values.len()
            )));
        }
        let nidx = idx.columns.len();
        // An empty table (no row map) or an index with no entries yields nothing.
        let (Some(rows_root), Some(index_root)) =
            (rows_root, self.table_index_root(tree, index_name)?)
        else {
            return Ok(Vec::new());
        };
        let prefix = encode_index_prefix(values);
        let mut out = Vec::new();
        for (index_key, _) in crate::prolly::scan_prefix(self.store(), &index_root, &prefix)? {
            let pk_key = index_row_map_key(&index_key, nidx)?;
            if let Some(value) = crate::prolly::get(self.store(), &rows_root, &pk_key)? {
                out.push(decode_row(&schema, &value)?);
            }
        }
        Ok(out)
    }

    /// Read rows through a declared secondary index from a historical commit without changing the
    /// workspace working tree.
    pub fn index_scan_at(
        &self,
        ns: WorkspaceId,
        name: &str,
        index_name: &str,
        values: &[Value],
        commit: Digest,
    ) -> Result<Vec<Row>> {
        let path = normalize_path(name)?;
        self.authorize_table(ns, &path, AclRight::Read)?;
        self.require_commit_visible_in_workspace(ns, commit)?;
        let tree = self
            .table_tree_at(commit, &path)?
            .ok_or_else(|| LoomError::not_found(format!("table {name:?} not found at {commit}")))?;
        let (schema_addr, rows_root) = self.table_tree_parts(tree)?;
        let schema = Schema::decode(&self.load_content(schema_addr)?)?;
        let idx = schema
            .index(index_name)
            .ok_or_else(|| LoomError::not_found(format!("no index {index_name:?} on {name:?}")))?;
        if values.len() > idx.columns.len() {
            return Err(LoomError::invalid(format!(
                "index {index_name:?} has {} columns, got {} lookup values",
                idx.columns.len(),
                values.len()
            )));
        }
        let nidx = idx.columns.len();
        let (Some(rows_root), Some(index_root)) =
            (rows_root, self.table_index_root(tree, index_name)?)
        else {
            return Ok(Vec::new());
        };
        let prefix = encode_index_prefix(values);
        let mut out = Vec::new();
        for (index_key, _) in crate::prolly::scan_prefix(self.store(), &index_root, &prefix)? {
            let pk_key = index_row_map_key(&index_key, nidx)?;
            if let Some(value) = crate::prolly::get(self.store(), &rows_root, &pk_key)? {
                out.push(decode_row(&schema, &value)?);
            }
        }
        Ok(out)
    }
}
