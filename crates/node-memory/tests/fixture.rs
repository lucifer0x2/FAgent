//! Projects from `fixtures/generate.mjs`: accepted by the scene-input validator,
//! loaded with correct indexes, round-tripped, and edited like `Candidate` would.
//! Needs Node.js >= 20 on PATH.
mod support;

use fagent_node_memory::{ManualClock, ProjectState, Registry, RegistryConfig};
use fagent_scene_input::Snapshot;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use support::fixture::{generate, validate_with_scene_input};
use support::oracle::{round, Tally};
use support::random_ops::Gen;
use support::*;

fn check_loaded(snapshot: Snapshot, stats: &Value) {
    let registry = Registry::new(
        RegistryConfig {
            ttl: Duration::from_secs(60),
            budget_bytes: usize::MAX,
        },
        Arc::new(ManualClock::default()),
    );
    let (handle, _) = registry.load(snapshot).expect("fixture loads");
    let state = handle.read();
    let stat = |key: &str| stats[key].as_u64().expect("stat") as usize;
    assert_eq!(state.len(), stat("authored"));
    assert_eq!(state.pages().count(), 1 + stat("pages"));
    assert_eq!(
        state.children("frame_wide").unwrap().len(),
        stat("wideChildren")
    );
    let instances: usize = (0..8)
        .map(|m| state.instances_of(&format!("master_{m}")).count())
        .sum();
    assert_eq!(instances, stat("instances"));
    let id = |n: Option<&fagent_scene_input::Node>| n.map(|n| n.id.clone());
    assert_eq!(id(state.root_frame_of("wide_1")), Some("frame_wide".into()));
    assert_eq!(id(state.page_of("wide_1")), Some("page_1".into()));
    assert_eq!(
        id(state.master_of("master_3_body")),
        Some("master_3".into())
    );
    let subtree = state.subtree("page_library").unwrap().count();
    assert_eq!(subtree, 1 + 8 * 4);
    assert_rebuild_matches(&state);
}

/// Loading then `to_snapshot` keeps every row; only node order may differ.
fn assert_round_trip(snapshot: &Snapshot, state: &ProjectState) {
    let mut expected = snapshot.clone();
    let mut actual = state.to_snapshot();
    for s in [&mut expected, &mut actual] {
        s.nodes.sort_by(|a, b| a.id.cmp(&b.id));
    }
    assert!(actual == expected, "round trip changed the snapshot");
}

#[test]
fn small_fixture_is_valid_loads_and_round_trips() {
    let (snapshot, stats) = generate(5_000, 7);
    assert_eq!(stats["expanded"], 5_000);
    validate_with_scene_input(&snapshot);
    assert_round_trip(&snapshot, &load(snapshot.clone()));
    check_loaded(snapshot, &stats);
}

#[test]
fn random_batches_on_generated_fixture_match_candidate() {
    let (mut snapshot, _) = generate(2_000, 3);
    // Generated text uses the test font id.
    let mut font = snapshot.fonts[0].clone();
    font.id = "font".into();
    snapshot.fonts.push(font);
    let mut state = load(snapshot);
    let mut rng = Rng::new(11);
    let mut gen = Gen {
        rng: &mut rng,
        counter: 0,
        extended: false,
    };
    let mut tally = Tally::default();
    for n in 0..60 {
        let ops = gen.batch(&state.to_snapshot());
        round(&mut state, &ops, &mut tally, &format!("round {n}"));
    }
    assert!(tally.accepted > 10 && tally.rejected() > 10);
}

#[test]
#[ignore = "200k nodes; run with --release -- --ignored"]
fn large_fixture_is_valid_and_loads() {
    let (snapshot, stats) = generate(200_000, 1);
    assert_eq!(stats["expanded"], 200_000);
    validate_with_scene_input(&snapshot);
    check_loaded(snapshot, &stats);
}
