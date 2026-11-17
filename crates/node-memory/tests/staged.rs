//! Two-phase batches: a staged batch is visible only through `Staged`, commit keeps
//! it, abort or drop (also on panic) restores the exact previous state, and readers of
//! a shared handle wait while a batch is staged.
mod support;

use fagent_node_memory::{ManualClock, Registry, RegistryConfig};
use fagent_scene_input::NodeKind;
use serde_json::json;
use std::sync::{mpsc, Arc};
use std::time::Duration;
use support::*;

fn batch() -> Vec<fagent_node_memory::Operation> {
    vec![
        create_at("n", NodeKind::Rectangle, "f1", 0),
        update("a", json!({"name":"A"}), false),
        delete("g"),
    ]
}

#[test]
fn commit_keeps_and_abort_restores() {
    let mut state = load(small());
    let before = observe(&state);
    let staged = state.begin(&batch()).unwrap();
    assert_eq!(staged.state().revision(), 2);
    assert!(staged.state().node("n").is_some());
    assert_eq!(staged.delta().nodes.deleted, ["g", "gt", "i1"]);
    staged.abort();
    assert_unchanged(&state, &before);

    drop(state.begin(&batch()).unwrap());
    assert_unchanged(&state, &before);

    let delta = state.begin(&batch()).unwrap().commit();
    assert_eq!(delta.revision, 2);
    assert_eq!(state.revision(), 2);
    assert_eq!(kids(&state, "f1"), ["n", "b", "a", "c"]);
    assert_rebuild_matches(&state);
}

#[test]
fn panic_while_staged_rolls_back() {
    let mut state = load(small());
    let before = observe(&state);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _staged = state.begin(&batch()).unwrap();
        panic!("persistence failed");
    }));
    assert!(result.is_err());
    assert_unchanged(&state, &before);
}

#[test]
fn readers_wait_for_a_staged_batch() {
    let registry = Registry::new(
        RegistryConfig {
            ttl: Duration::from_secs(60),
            budget_bytes: usize::MAX,
        },
        Arc::new(ManualClock::default()),
    );
    let (handle, _) = registry.load(small()).unwrap();
    let bytes = handle.estimated_bytes();
    for commit in [false, true] {
        let staged = handle
            .begin(&[create("x", NodeKind::Text, "f1", text("x"))])
            .unwrap();
        let (sent, received) = mpsc::channel();
        let reader = {
            let handle = handle.clone();
            std::thread::spawn(move || {
                let state = handle.read();
                sent.send((state.revision(), state.node("x").is_some()))
                    .unwrap();
            })
        };
        let early = received.recv_timeout(Duration::from_millis(100));
        assert!(early.is_err(), "reader saw a staged batch");
        match commit {
            true => drop(staged.commit()),
            false => staged.abort(),
        }
        let seen = received.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(seen, (1 + u64::from(commit), commit));
        reader.join().unwrap();
        assert_eq!(handle.estimated_bytes() > bytes, commit);
    }
}
