//! Exact page dependency closure over persistent authored/resource adjacency.
//! Expanded props and effective fonts are scoped work for visited pages.
use super::{view_indexes::references, ReadScope, ReadView};
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
                        if let Some(refs) =
                            self.index.dependencies.get(&format!("node:{}", node.id))
                        {
                            queue.extend(refs.iter().cloned());
                        }
                        queue.extend(self.notes_on(&node.id).map(|n| format!("note:{}", n.id)));
                    }
                    for node in &nodes.expanded {
                        queue.extend(references(&node.props));
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
                _ => {
                    if let Some(refs) = self.index.dependencies.get(&key) {
                        queue.extend(refs.iter().cloned());
                    }
                }
            }
        }
        Ok(keys)
    }
}
