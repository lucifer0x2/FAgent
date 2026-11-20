//! The nodes and styles a batch can have changed the validity of: the closure
//! `Candidate::validate` would find failing on a valid base after the batch.
//!
//! Seeds are the written nodes whose kind, parent or props changed (a sort-key-only
//! renumbering is no seed), the deleted nodes, the users of every changed token (and of
//! the tokens aliasing it, transitively) and of every changed style or style using a
//! changed token, and the nodes scoped to a theme no token declares any more.
//!
//! From each member the closure adds the instances of every component master that
//! contains it (in either revision), and, when the member *descends*, its children.
//! A node's checks read its ancestors' resolved `width`, `height` and `layout`, their
//! `theme` scope and whether one is a `component` master; nothing reads children. So a
//! member descends when it is new, moved, changed one of those props, uses a changed
//! token inside one of the size props, or names a theme that disappeared. Style values
//! hold only fill or text properties, so style users do not descend.
use super::Batch;
use crate::read_scope::references_of;
use crate::validate::Lookup;
use std::collections::{BTreeMap, BTreeSet};

/// Authored props that descendants consult.
const INHERITED: [&str; 5] = ["width", "height", "layout", "theme", "component"];
/// Props whose resolved values descendants consult.
const SIZES: [&str; 3] = ["width", "height", "layout"];

pub(super) struct Affected {
    /// Ids to recheck, in id order; some may not exist after the batch (deleted).
    pub nodes: BTreeSet<String>,
    /// Styles to recheck under every theme, in id order.
    pub styles: BTreeSet<String>,
}

pub(super) fn collect(before: &dyn Lookup, after: &dyn Lookup, batch: &Batch<'_>) -> Affected {
    let mut seeds: Vec<(String, bool)> = Vec::new();
    for id in batch.nodes {
        let Some(new) = after.node(id) else { continue };
        let descend = match before.node(id) {
            None => true,
            Some(old) if old.kind != new.kind || old.parent_id != new.parent_id => true,
            Some(old) if old.props != new.props => INHERITED
                .iter()
                .any(|key| old.props.get(key) != new.props.get(key)),
            Some(_) => continue,
        };
        seeds.push((id.clone(), descend));
    }
    seeds.extend(batch.deleted.iter().map(|id| (id.clone(), false)));
    let tokens = token_closure(before, after, batch.tokens);
    let mut styles: BTreeSet<String> = batch.styles.iter().cloned().collect();
    if !tokens.is_empty() {
        let keys: BTreeSet<String> = tokens.iter().map(|t| format!("token:{t}")).collect();
        for style in after.styles() {
            if !references_of(&style.value).is_disjoint(&keys) {
                styles.insert(style.id.clone());
            }
        }
        for key in &keys {
            for id in after.referencing(key) {
                let props = after.node(id).map(|n| &n.props);
                let sized = SIZES.iter().any(|size| {
                    let value = props.and_then(|p| p.get(size));
                    value.is_some_and(|v| references_of(v).contains(key))
                });
                seeds.push((id.to_owned(), sized));
            }
        }
    }
    for style in &styles {
        let users = after.referencing(&format!("style:{style}"));
        seeds.extend(users.into_iter().map(|id| (id.to_owned(), false)));
    }
    let declared: BTreeSet<&str> = after.declared_themes().into_iter().collect();
    for name in before.declared_themes() {
        if !declared.contains(name) {
            let scoped = after.referencing(&format!("theme:{name}"));
            seeds.extend(scoped.into_iter().map(|id| (id.to_owned(), true)));
        }
    }
    Affected {
        nodes: closure(before, after, seeds),
        styles,
    }
}

/// The changed tokens and every token aliasing one of them, transitively.
fn token_closure(before: &dyn Lookup, after: &dyn Lookup, changed: &[String]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut pending = changed.to_vec();
    while let Some(token) = pending.pop() {
        if out.contains(&token) {
            continue;
        }
        for view in [before, after] {
            pending.extend(view.aliasers(&token).into_iter().map(str::to_owned));
        }
        out.insert(token);
    }
    out
}

fn closure(
    before: &dyn Lookup,
    after: &dyn Lookup,
    seeds: Vec<(String, bool)>,
) -> BTreeSet<String> {
    // Id -> whether it was processed as descending.
    let mut seen: BTreeMap<String, bool> = BTreeMap::new();
    // Masters whose users (both revisions) are already queued: every descendant of a
    // master reaches it, and its users need queuing once.
    let mut expanded: BTreeSet<String> = BTreeSet::new();
    let mut pending = seeds;
    while let Some((id, descend)) = pending.pop() {
        if seen.get(&id).is_some_and(|&done| done || !descend) {
            continue;
        }
        for view in [before, after] {
            if descend {
                let children = view.children(&id);
                pending.extend(children.into_iter().map(|c| (c.id.clone(), true)));
            }
            for master in component_ancestors(view, &id) {
                if expanded.contains(master) {
                    continue;
                }
                expanded.insert(master.to_owned());
                for users in [before, after] {
                    let ids = users.component_users(master);
                    pending.extend(ids.into_iter().map(|u| (u.to_owned(), false)));
                }
            }
        }
        seen.insert(id, descend);
    }
    seen.into_keys().collect()
}

/// `id` and its ancestors that are component masters, nearest first (bounded walk).
pub(super) fn component_ancestors<'a>(view: &'a dyn Lookup, id: &str) -> Vec<&'a str> {
    let mut current = view.node(id);
    let mut masters = Vec::new();
    for _ in 0..128 {
        let Some(node) = current else { break };
        if node.props.get("component").is_some() {
            masters.push(node.id.as_str());
        }
        current = node.parent_id.as_deref().and_then(|id| view.node(id));
    }
    masters
}
