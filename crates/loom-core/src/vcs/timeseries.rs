use super::*;
use std::collections::BTreeMap;

impl<S: ObjectStore> Loom<S> {
    pub(crate) fn stage_time_series_reserved(
        &mut self,
        ns: WorkspaceId,
        path: &str,
        metadata: &[u8],
        points_root: Option<Digest>,
        rollup_roots: BTreeMap<String, Digest>,
    ) -> Result<()> {
        let path = normalize_path(path)?;
        let metadata_addr = self.store_content(ns, metadata)?;
        let mut entries = vec![TreeEntry {
            name: "metadata".to_string(),
            kind: EntryKind::Blob,
            target: metadata_addr,
            mode: 0,
        }];
        if let Some(root) = points_root {
            entries.push(TreeEntry {
                name: "points".to_string(),
                kind: EntryKind::ProllyMap,
                target: root,
                mode: 0,
            });
        }
        if !rollup_roots.is_empty() {
            let rollups = rollup_roots
                .into_iter()
                .map(|(name, root)| TreeEntry {
                    name,
                    kind: EntryKind::ProllyMap,
                    target: root,
                    mode: 0,
                })
                .collect();
            entries.push(TreeEntry {
                name: "rollups".to_string(),
                kind: EntryKind::Tree,
                target: self.put_object(&Object::tree(rollups)?)?,
                mode: 0,
            });
        }
        let root = self.put_object(&Object::tree(entries)?)?;
        self.work
            .entry(ns)
            .or_default()
            .insert(path, StagedEntry::TimeSeries(root));
        Ok(())
    }

    pub(crate) fn time_series_parts_reserved(
        &self,
        ns: WorkspaceId,
        path: &str,
    ) -> Result<(Digest, Option<Digest>, BTreeMap<String, Digest>)> {
        let path = normalize_path(path)?;
        let root = match self.work.get(&ns).and_then(|work| work.get(&path)) {
            Some(StagedEntry::TimeSeries(root)) => *root,
            Some(_) => {
                return Err(LoomError::invalid(format!(
                    "{path:?} is not a time-series collection"
                )));
            }
            None => {
                return Err(LoomError::not_found(format!(
                    "time-series collection {path:?} not staged"
                )));
            }
        };
        let Object::Tree(entries) = self.get_object(&root)? else {
            return Err(LoomError::corrupt("time-series entry target is not a Tree"));
        };
        let mut metadata = None;
        let mut points = None;
        let mut rollups = BTreeMap::new();
        for entry in entries {
            match entry.name.as_str() {
                "metadata" if entry.kind == EntryKind::Blob => metadata = Some(entry.target),
                "points" if entry.kind == EntryKind::ProllyMap => points = Some(entry.target),
                "rollups" if entry.kind == EntryKind::Tree => {
                    let Object::Tree(entries) = self.get_object(&entry.target)? else {
                        return Err(LoomError::corrupt(
                            "time-series rollups target is not a Tree",
                        ));
                    };
                    for rollup in entries {
                        if rollup.kind == EntryKind::ProllyMap {
                            rollups.insert(rollup.name, rollup.target);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok((
            metadata.ok_or_else(|| LoomError::corrupt("time-series Tree has no metadata entry"))?,
            points,
            rollups,
        ))
    }
}
