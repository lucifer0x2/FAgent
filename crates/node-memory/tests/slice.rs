//! Scoped slices: what they hold, and that the existing scoped readers
//! (`read_scope::ScopedSnapshot`, used by `get_design_context`/`get_subtree`) answer
//! on a slice exactly as on the full snapshot.
mod support;

use fagent_node_memory::{Operation, ProjectState};
use fagent_scene_input::read_scope::{ReadScope, ScopeIndex, ScopedSnapshot};
use fagent_scene_input::{NodeKind, Snapshot, StorageError};
use serde_json::json;
use std::sync::Arc;
use support::fixture::{generate, validate_with_scene_input};
use support::*;

fn ids(snapshot: &Snapshot) -> Vec<&str> {
    snapshot.nodes.iter().map(|n| n.id.as_str()).collect()
}

fn note(id: &str, node: &str) -> Operation {
    Operation::CreateNote {
        id: id.into(),
        node_id: node.into(),
        text: "check".into(),
        author: None,
    }
}

#[test]
fn slice_holds_subtree_masters_navigation_and_notes() {
    let mut state = load(small());
    state
        .apply(&[
            update("t2", json!({"links":[{"to":"f1"}]}), false),
            update("b", json!({"links":[{"to":"f2"}]}), false),
            note("on_a", "a"),
            note("on_f2", "f2"),
        ])
        .unwrap();
    let slice = state.slice("f1").unwrap();
    // Library page and m1 (with nested master m1n) through i1; f2 as a link target
    // and t2 as a link source, each with ancestors only; p1 as f1's ancestor.
    let expected = [
        "lib", "m1", "m1t", "m1n", "m1nt", "p1", "f1", "b", "a", "c", "g", "gt", "i1", "p2", "f2",
        "t2",
    ];
    assert_eq!(ids(&slice), expected);
    let notes: Vec<_> = slice.notes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(notes, ["on_a", "on_f2"]);
    assert_eq!(slice.fonts.len(), 1);
    assert_eq!(slice.revision, state.revision());
    assert!(state.slice("missing").is_none());
}

fn scoped(snapshot: &Snapshot) -> ScopedSnapshot<'_> {
    ScopedSnapshot::from_validated(snapshot, Arc::new(ScopeIndex::new(snapshot)))
}

fn same_error(a: &StorageError, b: &StorageError) -> bool {
    a.code == b.code && a.message == b.message
}

/// Every scoped read the tree readers make for `target`, compared on both snapshots.
fn compare(full: &Snapshot, slice: &Snapshot, target: &str) -> Result<(), String> {
    let (a, b) = (scoped(full), scoped(slice));
    for definitions in [true, false] {
        let fail = |what: &str| Err(format!("{target} definitions={definitions}: {what}"));
        let (mut x, mut y) = match (
            a.expand(ReadScope::Target(target), definitions),
            b.expand(ReadScope::Target(target), definitions),
        ) {
            (Ok(x), Ok(y)) => (x, y),
            (Err(e), Err(f)) if same_error(&e, &f) => continue,
            _ => return fail("expand outcome"),
        };
        if x.authored != y.authored || x.dependencies != y.dependencies {
            return fail("authored/dependencies");
        }
        if x.expanded != y.expanded || x.stats != y.stats {
            return fail("expanded");
        }
        let deps =
            |n: &Vec<&fagent_scene_input::Node>| n.iter().map(|n| n.id.clone()).collect::<Vec<_>>();
        let (dx, dy) = (deps(&x.dependencies), deps(&y.dependencies));
        let notes_x = a.notes_for_nodes(dx.iter().map(String::as_str));
        if notes_x != b.notes_for_nodes(dy.iter().map(String::as_str)) {
            return fail("notes");
        }
        if a.navigation_nodes(&x.authored, target) != b.navigation_nodes(&y.authored, target) {
            return fail("navigation");
        }
        match (a.resolve(&mut x, "default"), b.resolve(&mut y, "default")) {
            (Ok(rx), Ok(ry)) if rx == ry => {}
            (Err(e), Err(f)) if same_error(&e, &f) => {}
            _ => return fail("resolve"),
        }
    }
    let nodes = |s: &ScopedSnapshot| s.expand_nodes(ReadScope::Target(target)).ok();
    if nodes(&a) != nodes(&b) {
        return Err(format!("{target}: expand_nodes"));
    }
    match (a.resolved_tokens("default"), b.resolved_tokens("default")) {
        (Ok(x), Ok(y)) if x == y => Ok(()),
        (Err(e), Err(f)) if same_error(&e, &f) => Ok(()),
        _ => Err(format!("{target}: tokens")),
    }
}

/// Links between root frames, a nested instance inside a master (recursive closure),
/// notes on frames, masters and inner nodes, and a token.
fn enrich(state: &mut ProjectState) {
    let first = |state: &ProjectState, root: &str, kind: NodeKind| {
        let mut nodes = state.subtree(root).unwrap().skip(1);
        nodes.find(|n| n.kind == kind).unwrap().id.clone()
    };
    let source_1 = first(state, "root_1", NodeKind::Rectangle);
    let source_3 = first(state, "root_3", NodeKind::Rectangle);
    let inner_4 = first(state, "root_4", NodeKind::Text);
    let nested = json!({"componentId":"master_0","name":"Nested"});
    let token = json!({"id":"brand","name":"Brand","type":"color","value":"#ff0000"});
    state
        .apply(&[
            update(&source_1, json!({"links":[{"to":"root_2"}]}), false),
            update(&source_3, json!({"links":[{"to":"root_1"}]}), false),
            create("nested_0", NodeKind::Instance, "master_1", nested),
            note("note_root_1", "root_1"),
            note("note_inner_4", &inner_4),
            note("note_master_0", "master_0_title"),
            Operation::UpsertToken {
                token: serde_json::from_value(token).unwrap(),
            },
        ])
        .unwrap();
}

#[test]
fn scoped_reads_on_slices_match_full_snapshot_for_every_root_frame() {
    let (snapshot, _) = generate(5_000, 7);
    let mut state = load(snapshot);
    enrich(&mut state);
    let full = state.to_snapshot();
    validate_with_scene_input(&full);
    let roots: Vec<String> = state
        .pages()
        .flat_map(|p| state.children(&p.id).unwrap())
        .map(|n| n.id.clone())
        .collect();
    let mut sizes = Vec::new();
    for root in &roots {
        let slice = state.slice(root).unwrap();
        sizes.push(slice.nodes.len());
        let mut targets = vec![root.clone()];
        let inner = state.subtree(root).unwrap().skip(1);
        let picks = inner.filter(|n| matches!(n.kind, NodeKind::Frame | NodeKind::Instance));
        for node in picks.take(3) {
            if node.kind == NodeKind::Instance {
                let master = node.props["componentId"].as_str().unwrap();
                targets.push(format!("{}/{master}_title", node.id));
            }
            targets.push(node.id.clone());
        }
        for target in &targets {
            if let Err(diff) = compare(&full, &slice, target) {
                panic!("slice of {root} differs: {diff}");
            }
        }
    }
    sizes.sort_unstable();
    let largest = *sizes.last().unwrap();
    eprintln!(
        "{} root frames; slice nodes median {}, max {largest} of {}",
        roots.len(),
        sizes[sizes.len() / 2],
        full.nodes.len()
    );
    assert!(roots.len() > 10 && largest < full.nodes.len());
}
