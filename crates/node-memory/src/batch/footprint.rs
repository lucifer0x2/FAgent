//! History footprint of a batch: which keys it touched, so a guarded write can tell
//! whether a concurrent commit conflicts. Keys are `node:`, `page:`, `note:`,
//! `token:` and `style:` entries plus `*` when navigation defaults may have moved.
use crate::Delta;
use fagent_scene_input::read_scope::ReadView;
use fagent_scene_input::{Node, NodeKind, Snapshot};
use std::collections::BTreeSet;

pub(super) fn build<'m>(
    before: &'m ReadView,
    after: &'m ReadView,
    delta: &Delta,
    ids: &[String],
) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    let global = navigation_changed(before, after, ids);
    if global {
        keys.insert("*".to_owned());
    }
    for id in ids {
        let (old, new) = (before.node(id), after.node(id));
        if !crate::props::same(old, new) {
            keys.insert(format!("node:{id}"));
            // Appending a page may compact other pages' sort keys; with order
            // preserved (no wildcard) their content and guards are unaffected.
            if !global && only_page_sort_key(old, new) {
                continue;
            }
            for view in [before, after] {
                if let Some(page) = view.page(id) {
                    keys.insert(format!("page:{page}"));
                }
            }
        }
        let (old, new) = (before.note(id), after.note(id));
        if !crate::props::same(old, new) {
            keys.insert(format!("note:{id}"));
            for (note, view) in [(old, before), (new, after)] {
                if let Some(page) = note.and_then(|n| view.page(&n.node_id)) {
                    keys.insert(format!("page:{page}"));
                }
            }
        }
    }
    let (old, new) = (before.metadata(), after.metadata());
    resources(&delta.tokens, "token", &mut keys, |id| {
        let find = |m: &'m Snapshot| m.tokens.iter().find(|t| t.id == id);
        (find(old), find(new))
    });
    resources(&delta.styles, "style", &mut keys, |id| {
        let find = |m: &'m Snapshot| m.styles.iter().find(|s| s.id == id);
        (find(old), find(new))
    });
    keys
}

/// Adds `kind:id` for every written resource whose row differs between the views.
fn resources<'m, T: PartialEq + serde::Serialize + 'm>(
    ids: &[String],
    kind: &str,
    keys: &mut BTreeSet<String>,
    pair: impl Fn(&str) -> (Option<&'m T>, Option<&'m T>),
) {
    for id in ids {
        let (old, new) = pair(id);
        if !crate::props::same(old, new) {
            keys.insert(format!("{kind}:{id}"));
        }
    }
}

fn only_page_sort_key(old: Option<&Node>, new: Option<&Node>) -> bool {
    match (old, new) {
        (Some(old), Some(new)) => old.kind == NodeKind::Page && same_page(Some(old), Some(new)),
        _ => false,
    }
}

/// Whether an existing page is unchanged apart from its numeric sort key.
fn same_page(old: Option<&Node>, new: Option<&Node>) -> bool {
    match (old, new) {
        (Some(a), Some(b)) => {
            a.id == b.id
                && a.kind == b.kind
                && a.parent_id == b.parent_id
                && crate::props::same(Some(&a.props), Some(&b.props))
        }
        _ => false,
    }
}

/// Default navigation depends on page and root-frame order and on flow starts: the
/// viewer opens the first page, preview the first root frame across pages in order.
/// A change that can move either, or any `flowStart`, is global.
fn navigation_changed(before: &ReadView, after: &ReadView, ids: &[String]) -> bool {
    let order =
        |view: &ReadView| -> Vec<String> { view.scoped().roots().map(|n| n.id.clone()).collect() };
    let (old, new) = (order(before), order(after));
    // Existing pages keep content and relative order; new pages may only be
    // appended to a project that already had pages.
    if !new.starts_with(&old)
        || old.first() != new.first()
        || old
            .iter()
            .any(|id| !same_page(before.node(id), after.node(id)))
    {
        return true;
    }
    let mut affected = BTreeSet::new();
    for id in ids {
        let flow = |view: &ReadView| {
            view.node(id)
                .and_then(|n| n.props.get("flowStart"))
                .cloned()
        };
        if flow(before) != flow(after) {
            return true;
        }
        for view in [before, after] {
            if let Some(page) = view.page(id) {
                affected.insert(page.to_owned());
            }
        }
    }
    for page in &affected {
        // Relative order matters; placement may compact harmless gaps.
        if !roots(after, page).starts_with(&roots(before, page)) {
            return true;
        }
    }
    // A first root on an empty page matters only when it becomes the project's
    // first root frame, i.e. no earlier page has one.
    first_root(before, &old) != first_root(after, &new)
}

fn roots(view: &ReadView, page: &str) -> Vec<String> {
    view.scoped()
        .children(page)
        .filter(|n| n.kind == NodeKind::Frame)
        .map(|n| n.id.clone())
        .collect()
}

fn first_root(view: &ReadView, order: &[String]) -> Option<String> {
    order
        .iter()
        .find_map(|page| roots(view, page).into_iter().next())
}
