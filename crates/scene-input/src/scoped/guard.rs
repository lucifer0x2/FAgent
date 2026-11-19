//! Exact page dependency closure over persistent authored/resource adjacency.
//! Expanded props and effective fonts are scoped work for visited pages.
use super::{snapshot_source::references_of, ReadScope, ReadView};
use crate::{validate::FontReferences, Font, NodeKind, Result, Snapshot, StorageError};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

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
        let mut resources: Option<Resources<'_>> = None;
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
                _ => {
                    let resources = resources.get_or_insert_with(|| Resources::new(&self.snapshot));
                    queue.extend(resources.dependencies(kind, id));
                }
            }
        }
        Ok(keys)
    }

    /// Keys a token, style or font depends on; nothing for other resources.
    #[cfg(test)]
    pub(super) fn resource_dependencies(&self, kind: &str, id: &str) -> BTreeSet<String> {
        Resources::new(&self.snapshot).dependencies(kind, id)
    }
}

/// Tokens, styles and fonts by id, built once per closure walk.
struct Resources<'a> {
    tokens: HashMap<&'a str, &'a Value>,
    styles: HashMap<&'a str, &'a Value>,
    fonts: HashMap<&'a str, &'a Font>,
}

impl<'a> Resources<'a> {
    fn new(meta: &'a Snapshot) -> Self {
        Self {
            tokens: meta
                .tokens
                .iter()
                .map(|t| (t.id.as_str(), &t.value))
                .collect(),
            styles: meta
                .styles
                .iter()
                .map(|s| (s.id.as_str(), &s.value))
                .collect(),
            fonts: meta.fonts.iter().map(|f| (f.id.as_str(), f)).collect(),
        }
    }

    fn dependencies(&self, kind: &str, id: &str) -> BTreeSet<String> {
        match kind {
            "token" => self.tokens.get(id).copied(),
            "style" => self.styles.get(id).copied(),
            "font" => {
                let Some(font) = self.fonts.get(id) else {
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
