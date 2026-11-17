//! Exact-total search, expanding one authored root/page closure at a time.
mod criteria;
mod entry;
mod legacy;
mod page;
#[cfg(test)]
mod tests;

use super::{index::TreeIndex, pagination, ReadRequest};
use crate::error::{ApiError, Result};
use criteria::Criteria;
use fagent_storage::{
    read_scope::{ReadScope, ScopedSnapshot},
    NodeKind, SharedSnapshot,
};
use serde_json::{json, Value};

const RENAME_BYTES: usize = "matches".len() - "nodes".len();

pub(super) fn read_shared(s: &SharedSnapshot, req: ReadRequest) -> Result<Value> {
    dispatch(s, req).map(|(value, _)| value)
}

/// Work is present only when the trusted scoped path served the read.
fn dispatch(s: &SharedSnapshot, req: ReadRequest) -> Result<(Value, Option<Work>)> {
    match s.trusted_scoped()? {
        Some(scoped) => read_scoped(&scoped, req).map(|(value, work)| (value, Some(work))),
        None => legacy::read(&s.snapshot, req).map(|value| (value, None)),
    }
}

/// Work counts exclude SQL loading and cold revision-index construction.
#[derive(Debug, Default)]
struct Work {
    /// Authored nodes selected across roots; master libraries are not re-walked.
    selected: usize,
    visited: usize,
    max_expanded: usize,
    /// Matches inside instances that `instances:"reference"` left out of the results.
    skipped_instance_matches: usize,
    materialized_matches: usize,
    retained_matches: usize,
}

fn read_scoped(scoped: &ScopedSnapshot<'_>, req: ReadRequest) -> Result<(Value, Work)> {
    let criteria = Criteria::new(&req)?;
    if req.node_id.is_some() && req.frame_id.is_some() {
        return Err(ApiError::invalid("Choose one scope ID"));
    }
    let s = scoped.snapshot();
    let (scope, offset) = pagination::scope(s, "find_nodes", &req)?;
    let root = req.node_id.as_deref().or(req.frame_id.as_deref());
    let limit = req.limit.unwrap_or(50);
    let budget = req.budget.unwrap_or(65536);
    let mut matches = page::Matches::new(offset, limit, budget - RENAME_BYTES);
    let mut work = Work::default();
    let in_instance = root.is_some_and(|id| {
        id.contains('/')
            || scoped
                .node(id)
                .is_some_and(|n| n.kind == NodeKind::Instance)
    });
    let virtual_nodes = searches_virtual(&req, in_instance);
    if let Some(id) = root {
        visit(
            scoped,
            id,
            root,
            (&req, &criteria, virtual_nodes),
            &mut matches,
            &mut work,
        )?;
    } else {
        // Source order is the tie breaker, matching TreeIndex's stable root sort.
        for node in scoped.ordered_roots() {
            visit(
                scoped,
                &node.id,
                None,
                (&req, &criteria, virtual_nodes),
                &mut matches,
                &mut work,
            )?;
        }
    }
    work.materialized_matches = matches.materialized;
    work.retained_matches = matches.retained();
    for (name, value) in [
        ("find.selected_nodes", work.selected),
        ("find.visited_nodes", work.visited),
        ("find.max_expanded_nodes", work.max_expanded),
        ("find.materialized_matches", work.materialized_matches),
        ("find.retained_matches", work.retained_matches),
    ] {
        fagent_storage::bench_phase::count(name, value);
    }
    let mut response = json!({"schemaVersion":1,"projectId":s.project_id,"revision":s.revision,"scope":root,"criteria":entry::echo(&req),"scannedNodes":work.visited,"totalMatches":matches.total,"nodes":[],"limits":{"nodes":limit,"bytes":budget}});
    skipped(&mut response, work.skipped_instance_matches);
    let mut response = matches.finish(response, &scope)?;
    let found = response.as_object_mut().unwrap().remove("nodes").unwrap();
    response["matches"] = found;
    Ok((response, work))
}

/// Whether virtual instance descendants (IDs with `/`) are searched. `instances:"reference"`
/// (the default) keeps a page- or project-wide scan to authored nodes, so instances match as
/// themselves; descendants are still searched when the request addresses them: `expand`, a
/// scope that is an instance or inside one, or `ids`/`parentId` (which can name virtual IDs).
fn searches_virtual(req: &ReadRequest, in_instance: bool) -> bool {
    req.instances.as_deref() == Some("expand")
        || in_instance
        || req.ids.is_some()
        || req.parent_id.is_some()
}

/// Reported only when reference mode left matches out, so `totalMatches:0` with this field
/// means "repeat with instances:expand", not "nothing matched".
fn skipped(response: &mut Value, count: usize) {
    if count > 0 {
        response["skippedInstanceMatches"] = json!(count);
    }
}

fn visit(
    scoped: &ScopedSnapshot<'_>,
    id: &str,
    root: Option<&str>,
    (req, criteria, virtual_nodes): (&ReadRequest, &Criteria<'_>, bool),
    matches: &mut page::Matches,
    work: &mut Work,
) -> Result<()> {
    // Instance-free scopes need neither authored payload copies nor a transient
    // tree index. Search still visits every requested node for exact totals.
    if !id.contains('/') {
        let index = TreeIndex::scoped(scoped);
        let ordered = index.preorder(Some(id));
        if !ordered.iter().any(|(n, _)| n.kind == NodeKind::Instance) {
            let target = index
                .get(id)
                .ok_or_else(|| ApiError::invalid("Scope node does not exist"))?;
            if root.is_some() && req.frame_id.is_some() && target.kind != NodeKind::Frame {
                return Err(ApiError::invalid("frameId must identify a frame"));
            }
            work.selected += ordered.len();
            for (node, depth) in ordered {
                work.visited += 1;
                if let Some(on) = criteria.matched_on(node) {
                    matches.push(|| entry::entry(&index, root, node, depth, on, criteria, req))?;
                }
            }
            return Ok(());
        }
    }
    let (mut expanded, stats) = scoped.expand_nodes(ReadScope::Target(id))?;
    work.selected += stats.closure_nodes;
    work.max_expanded = work.max_expanded.max(expanded.len());
    // Authored instances retain their searchable kind/props; virtual children do not.
    for node in &mut expanded {
        if let Some(authored) = scoped
            .node(&node.id)
            .filter(|n| n.kind == NodeKind::Instance)
        {
            *node = authored.clone();
        }
    }
    let index = TreeIndex::new(&expanded);
    let target = index
        .get(id)
        .ok_or_else(|| ApiError::invalid("Scope node does not exist"))?;
    if root.is_some() && req.frame_id.is_some() && target.kind != NodeKind::Frame {
        return Err(ApiError::invalid("frameId must identify a frame"));
    }
    for (node, depth) in index.preorder(Some(id)) {
        if !virtual_nodes && node.id.contains('/') {
            if criteria.matched_on(node).is_some() {
                work.skipped_instance_matches += 1;
            }
            continue;
        }
        work.visited += 1;
        if let Some(on) = criteria.matched_on(node) {
            matches.push(|| entry::entry(&index, root, node, depth, on, criteria, req))?;
        }
    }
    Ok(())
}
