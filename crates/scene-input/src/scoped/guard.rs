//! Exact page dependency closure over persistent authored/resource adjacency.
//! Expanded props and effective fonts are scoped work for visited pages.
use super::{snapshot_source::references_of, ReadScope, ReadView};
use crate::{validate::FontReferences, NodeKind, Result, StorageError};
use std::collections::{BTreeMap, BTreeSet};

impl ReadView {
    pub fn dependency_closure(&self, pages: &[String]) -> Result<BTreeSet<String>> {
        if pages.is_empty()
            || pages.len() > 100
            || pages.iter().collect::<BTreeSet<_>>().len() != pages.len()
        {
            return Err(StorageError::validation(
                "pageIds",
                "Supply 1–100 distinct existing page IDs",
            ));
        }
        if pages
            .iter()
            .any(|id| self.node(id).is_none_or(|n| n.kind != NodeKind::Page))
        {
            return Err(StorageError::validation(
                "pageIds",
                "Guarded page does not exist",
            ));
        }
        let mut keys = BTreeSet::new();
        let mut queue: Vec<_> = pages.iter().map(|p| format!("page:{p}")).collect();
        while let Some(key) = queue.pop() {
            if !keys.insert(key.clone()) {
                continue;
            }
            let (kind, id) = key.split_once(':').unwrap();
            match kind {
                "page" => {
                    let scoped = self.scoped();
                    let nodes = scoped.expand(ReadScope::Page(id), false)?;
                    for node in &nodes.authored {
                        queue.extend(references_of(&node.props));
                        queue.extend(self.notes_on(&node.id).map(|n| format!("note:{}", n.id)));
                    }
                    for node in &nodes.expanded {
                        queue.extend(references_of(&node.props));
                    }
                    let expanded: BTreeMap<_, _> = nodes
                        .expanded
                        .into_iter()
                        .map(|n| (n.id.clone(), n))
                        .collect();
                    let fonts = FontReferences::from_snapshot(&self.snapshot, &expanded)?;
                    queue.extend(
                        fonts
                            .for_nodes(&expanded.values().collect::<Vec<_>>())?
                            .into_iter()
                            .map(|id| format!("font:{id}")),
                    );
                }
                "component" | "link" => {
                    if let Some(page) = self.page(id) {
                        queue.push(format!("page:{page}"));
                    }
                }
                _ => queue.extend(self.resource_dependencies(kind, id)),
            }
        }
        Ok(keys)
    }

    /// Keys a token, style or font depends on; nothing for other resources.
    pub(super) fn resource_dependencies(&self, kind: &str, id: &str) -> BTreeSet<String> {
        let meta = &self.snapshot;
        match kind {
            "token" => meta.tokens.iter().find(|t| t.id == id).map(|t| &t.value),
            "style" => meta.styles.iter().find(|s| s.id == id).map(|s| &s.value),
            "font" => {
                let Some(font) = meta.fonts.iter().find(|f| f.id == id) else {
                    return BTreeSet::new();
                };
                let mut keys = BTreeSet::from([format!("font-family:{}", font.family)]);
                keys.extend(font.asset_id.as_ref().map(|asset| format!("asset:{asset}")));
                return keys;
            }
            _ => None,
        }
        .map(references_of)
        .unwrap_or_default()
    }
}
