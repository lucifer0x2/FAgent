//! `HotProject::begin`: staleness, rows, footprint, rollback and counters.
mod support;

use fagent_node_memory::{
    Begin, HotProject, Limits, ManualClock, MemoryConfig, NodeMemory, Prepared,
};
use fagent_scene_input::{NodeKind, Token};
use serde_json::json;
use std::sync::Arc;
use support::*;

fn setup() -> (NodeMemory, HotProject) {
    let memory = NodeMemory::new(MemoryConfig::default(), Arc::new(ManualClock::default()));
    let project = memory.load(small()).unwrap();
    (memory, project)
}

fn ready(begin: Begin<'_>) -> Prepared<'_> {
    match begin {
        Begin::Ready(prepared) => prepared,
        Begin::Stale => panic!("stale"),
    }
}

fn keys(prepared: &Prepared<'_>) -> Vec<String> {
    prepared.footprint().iter().cloned().collect()
}

#[test]
fn stale_when_the_project_is_at_another_revision() {
    let (_, project) = setup();
    let ops = [update("a", json!({"name":"A"}), true)];
    assert!(matches!(project.begin(0, &ops).unwrap(), Begin::Stale));
    assert!(matches!(project.begin(2, &ops).unwrap(), Begin::Stale));
    let prepared = ready(project.begin(1, &ops).unwrap());
    assert_eq!((prepared.base_revision(), prepared.revision()), (1, 2));
    prepared.commit();
    assert!(matches!(project.begin(1, &ops).unwrap(), Begin::Stale));
    assert!(matches!(project.begin(2, &ops).unwrap(), Begin::Ready(_)));
}

#[test]
fn a_batch_exposes_rows_footprint_and_both_views() {
    let (memory, project) = setup();
    let ops = [update("a", json!({"name":"A"}), true)];
    let prepared = ready(project.begin(1, &ops).unwrap());
    assert_eq!(prepared.changed_ids(), ["a"]);
    let rows = prepared.rows();
    assert_eq!(rows.nodes.len(), 1);
    assert_eq!(rows.nodes[0].props["name"], "A");
    assert!(rows.deleted_nodes.is_empty() && rows.tokens.is_empty());
    assert_eq!(keys(&prepared), ["node:a", "page:p1"]);
    assert_eq!(prepared.before().node("a").unwrap().props.get("name"), None);
    assert_eq!(prepared.after().node("a").unwrap().props["name"], "A");
    // Readers keep the base until the commit.
    assert_eq!(project.view().node("a").unwrap().props.get("name"), None);
    let delta = prepared.commit();
    assert_eq!(delta.revision, 2);
    assert_eq!(project.view().node("a").unwrap().props["name"], "A");
    let stats = memory.stats();
    assert_eq!((stats.commits, stats.rollbacks), (1, 0));
}

#[test]
fn update_that_changes_nothing_has_no_row_but_is_a_changed_id() {
    let (_, project) = setup();
    let ops = [update("a", json!({"name":"A"}), true)];
    ready(project.begin(1, &ops).unwrap()).commit();
    let again = ready(project.begin(2, &ops).unwrap());
    assert_eq!(again.changed_ids(), ["a"]);
    assert!(again.rows().is_empty());
    assert!(again.footprint().is_empty());
}

#[test]
fn page_token_and_note_batches_report_their_rows() {
    let (_, project) = setup();
    let token = Token {
        id: "tk".into(),
        name: "T".into(),
        token_type: "color".into(),
        value: json!("#000000"),
    };
    let ops = [
        new("p3", NodeKind::Page, None, json!({})),
        fagent_node_memory::Operation::UpsertToken { token },
        fagent_node_memory::Operation::CreateNote {
            id: "n1".into(),
            node_id: "a".into(),
            text: "hi".into(),
            author: None,
        },
    ];
    let prepared = ready(project.begin(1, &ops).unwrap());
    assert_eq!(prepared.changed_ids(), ["n1", "p2", "p3", "tk"]);
    let rows = prepared.rows();
    assert_eq!(rows.tokens.len(), 1);
    assert_eq!(rows.notes.len(), 1);
    // A new page is appended and renumbers its tied sibling p2: order is preserved,
    // so no wildcard and no page key for p2.
    assert_eq!(
        keys(&prepared),
        ["node:p2", "node:p3", "note:n1", "page:p1", "page:p3", "token:tk"]
    );
}

#[test]
fn navigation_changes_are_global() {
    let (_, project) = setup();
    let ops = [update("f1", json!({"flowStart":"main"}), true)];
    let prepared = ready(project.begin(1, &ops).unwrap());
    assert!(prepared.footprint().contains("*"));
    drop(prepared);
    // A new root frame before the others moves the first screen.
    let front = fagent_node_memory::Operation::CreateNode {
        id: "front".into(),
        kind: NodeKind::Frame,
        parent_id: Some("p1".into()),
        props: json!({"width":10,"height":10}),
        index: Some(0),
    };
    let prepared = ready(project.begin(1, &[front]).unwrap());
    assert!(prepared.footprint().contains("*"));
    // A plain edit deep in a frame is not.
    drop(prepared);
    let edit = [update("c", json!({"name":"C"}), true)];
    assert!(!ready(project.begin(1, &edit).unwrap())
        .footprint()
        .contains("*"));
}

#[test]
fn dropping_a_prepared_batch_rolls_back_exactly() {
    let (memory, project) = setup();
    let before = project.view();
    let ops = [delete("g"), update("a", json!({"name":"A"}), true)];
    let prepared = ready(project.begin(1, &ops).unwrap());
    assert_eq!(prepared.rows().deleted_nodes, ["g", "gt", "i1"]);
    drop(prepared);
    assert!(Arc::ptr_eq(&before, &project.view()));
    assert_eq!(memory.stats().rollbacks, 1);
    // The same batch applies again at the same revision.
    ready(project.begin(1, &ops).unwrap()).commit();
    assert_eq!(memory.stats().commits, 1);
}

#[test]
fn rejections_carry_the_public_error_and_count() {
    let (memory, project) = setup();
    let ops = [update("a", json!({"name":"A"}), true), delete("missing")];
    let error = project.begin(1, &ops).err().expect("rejected");
    assert_eq!(
        (error.code.as_str(), error.op_index),
        ("VALIDATION_FAILED", Some(1))
    );
    // End-of-batch rule: a link to a node that is not a root frame.
    let ops = [update("a", json!({"links":[{"to":"b"}]}), true)];
    let error = project.begin(1, &ops).err().expect("rejected");
    assert_eq!(error.field.as_deref(), Some("links[0].to"));
    assert_eq!(error.op_index, None);
    let stats = memory.stats();
    assert_eq!(stats.rejected, 2);
    // Nothing leaked into the project.
    assert_eq!(project.view().metadata().revision, 1);
}

#[test]
fn a_node_cap_below_the_scene_input_cap_is_a_rejection() {
    let config = MemoryConfig {
        limits: Limits { max_nodes: 18 },
        ..MemoryConfig::default()
    };
    let memory = NodeMemory::new(config, Arc::new(ManualClock::default()));
    let project = memory.load(small()).unwrap();
    let ops = [create("x", NodeKind::Rectangle, "f1", json!({}))];
    let error = project.begin(1, &ops).err().expect("over the cap");
    assert_eq!(error.field.as_deref(), Some("nodes"));
}

#[test]
fn settled_view_waits_for_a_staged_batch() {
    let (_, project) = setup();
    let ops = [update("a", json!({"name":"A"}), true)];
    let prepared = ready(project.begin(1, &ops).unwrap());
    let reader = project.clone();
    let waiter = std::thread::spawn(move || reader.settled_view(2).metadata().revision);
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(!waiter.is_finished(), "waits for the write lock");
    prepared.commit();
    assert_eq!(waiter.join().unwrap(), 2);
}

fn color(id: &str, value: serde_json::Value) -> fagent_node_memory::Operation {
    fagent_node_memory::Operation::UpsertToken {
        token: Token {
            id: id.into(),
            name: id.into(),
            token_type: "color".into(),
            value,
        },
    }
}

/// `Candidate` validates every token after each upsert; `Txn` checks what the write can
/// affect, right after it. A batch whose token set is invalid after one upsert and
/// repaired by the next one, and an error after a token upsert, get `Candidate`'s
/// per-operation error straight from `Txn`.
#[test]
fn token_errors_are_candidates_per_upsert() {
    let (memory, project) = setup();
    let repaired = [
        color("alias", json!({"aliasId":"later"})),
        color("later", json!("#ffffff")),
    ];
    let error = project.begin(1, &repaired).err().expect("rejected");
    assert_eq!(error.op_index, Some(0));
    assert_eq!(error.field.as_deref(), Some("token.aliasId"));

    let then_bad_node = [
        color("alias", json!({"aliasId":"missing"})),
        update("a", json!({"width":false}), true),
    ];
    let error = project.begin(1, &then_bad_node).err().expect("rejected");
    assert_eq!(error.op_index, Some(0));
    assert_eq!(error.field.as_deref(), Some("token.aliasId"));

    let stats = memory.stats();
    assert_eq!(stats.rejected, 2);
}
