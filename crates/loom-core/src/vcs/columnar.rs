use super::*;
use std::collections::BTreeMap;

impl<S: ObjectStore> Loom<S> {
    pub(crate) fn stage_columnar_reserved(
        &mut self,
        ns: WorkspaceId,
        path: &str,
        manifest: &[u8],
        segments: BTreeMap<String, Vec<u8>>,
    ) -> Result<()> {
        let path = normalize_path(path)?;
        let manifest_addr = self.store_content(ns, manifest)?;
        let segment_entries = segments
            .into_iter()
            .map(|(name, bytes)| {
                let target = self.store_content(ns, &bytes)?;
                Ok(TreeEntry {
                    name,
                    kind: EntryKind::Blob,
                    target,
                    mode: 0,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let segments_root = self.put_object(&Object::tree(segment_entries)?)?;
        let root = self.put_object(&Object::tree(vec![
            TreeEntry {
                name: "manifest".to_string(),
                kind: EntryKind::Blob,
                target: manifest_addr,
                mode: 0,
            },
            TreeEntry {
                name: "segments".to_string(),
                kind: EntryKind::Tree,
                target: segments_root,
                mode: 0,
            },
        ])?)?;
        self.work
            .entry(ns)
            .or_default()
            .insert(path, StagedEntry::Columnar(root));
        Ok(())
    }

    pub(crate) fn columnar_parts_reserved(
        &self,
        ns: WorkspaceId,
        path: &str,
    ) -> Result<(Digest, BTreeMap<String, Digest>)> {
        let path = normalize_path(path)?;
        let root = match self.work.get(&ns).and_then(|work| work.get(&path)) {
            Some(StagedEntry::Columnar(root)) => *root,
            Some(_) => {
                return Err(LoomError::invalid(format!(
                    "{path:?} is not a columnar dataset"
                )));
            }
            None => {
                return Err(LoomError::not_found(format!(
                    "columnar dataset {path:?} not staged"
                )));
            }
        };
        let Object::Tree(entries) = self.get_object(&root)? else {
            return Err(LoomError::corrupt("columnar entry target is not a Tree"));
        };
        let mut manifest = None;
        let mut segments_root = None;
        for entry in entries {
            match entry.name.as_str() {
                "manifest" if entry.kind == EntryKind::Blob => manifest = Some(entry.target),
                "segments" if entry.kind == EntryKind::Tree => segments_root = Some(entry.target),
                _ => {}
            }
        }
        let segments_root = segments_root
            .ok_or_else(|| LoomError::corrupt("columnar Tree has no segments entry"))?;
        let Object::Tree(segment_entries) = self.get_object(&segments_root)? else {
            return Err(LoomError::corrupt("columnar segments target is not a Tree"));
        };
        let mut segments = BTreeMap::new();
        for entry in segment_entries {
            if entry.kind == EntryKind::Blob {
                segments.insert(entry.name, entry.target);
            }
        }
        Ok((
            manifest.ok_or_else(|| LoomError::corrupt("columnar Tree has no manifest entry"))?,
            segments,
        ))
    }

    pub(crate) fn columnar_root_reserved(&self, ns: WorkspaceId, path: &str) -> Result<Digest> {
        let path = normalize_path(path)?;
        match self.work.get(&ns).and_then(|work| work.get(&path)) {
            Some(StagedEntry::Columnar(root)) => Ok(*root),
            Some(_) => Err(LoomError::invalid(format!(
                "{path:?} is not a columnar dataset"
            ))),
            None => Err(LoomError::not_found(format!(
                "columnar dataset {path:?} not staged"
            ))),
        }
    }
}
