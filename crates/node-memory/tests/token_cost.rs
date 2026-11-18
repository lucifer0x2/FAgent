//! Cost of `upsert_token`, which revalidates every token and the alias graph per
//! operation (as `Candidate::apply` does). Timing only; run with
//! `cargo test --release -p fagent-node-memory --test token_cost -- --ignored --nocapture`.
mod support;

use fagent_node_memory::{ManualClock, MemoryConfig, NodeMemory, Operation, ProjectState};
use fagent_scene_input::validate::Candidate;
use fagent_scene_input::{Snapshot, Token};
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;
use support::*;

/// Color token `i` of alias chains of 16 (a literal, then aliases of the previous
/// one); every fourth chain head has a `night` branch. `shade` varies the literal.
fn token(i: usize, shade: usize) -> Token {
    let value = match (i % 16, i % 64) {
        (0, 0) => {
            json!({"default": format!("#0000{:02x}", shade % 256), "themes": {"night": "#ffffff"}})
        }
        (0, _) => json!(format!("#0000{:02x}", shade % 256)),
        _ => json!({"aliasId": format!("tk{:05}", i - 1)}),
    };
    Token {
        id: format!("tk{i:05}"),
        name: format!("Token {i}"),
        token_type: "color".into(),
        value,
    }
}

fn project(count: usize) -> Snapshot {
    let mut snapshot = small();
    snapshot.tokens = (0..count).map(|i| token(i, 0)).collect();
    snapshot
}

/// A batch rewriting `ops` existing tokens, spread over the set.
fn batch(count: usize, ops: usize) -> Vec<Operation> {
    (0..ops)
        .map(|n| Operation::UpsertToken {
            token: token(n * count / ops, n + 1),
        })
        .collect()
}

fn millis(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1e3
}

#[test]
#[ignore = "timing; run in release"]
fn upsert_token_revalidates_all_tokens() {
    for (count, ops) in [(500, 200), (2_000, 200), (4_000, 200), (2_000, 1)] {
        let snapshot = project(count);
        let ops = batch(count, ops);

        let state = ProjectState::from_snapshot(snapshot.clone()).unwrap();
        let mut txn = state.clone();
        let start = Instant::now();
        txn.apply(&ops).unwrap();
        let txn_ms = millis(start);

        let memory = NodeMemory::new(MemoryConfig::default(), Arc::new(ManualClock::default()));
        let hot = memory.load(snapshot.clone()).unwrap();
        let start = Instant::now();
        let begin = hot.begin(1, &ops).unwrap();
        let begin_ms = millis(start);
        drop(begin);

        let start = Instant::now();
        let mut candidate = Candidate::from_snapshot(&snapshot);
        let mut changed = BTreeSet::new();
        for op in &ops {
            candidate.apply(op, &mut changed).unwrap();
        }
        let candidate_ms = millis(start);
        eprintln!(
            "{count} tokens, {} upserts: Txn {txn_ms:.1} ms, begin {begin_ms:.1} ms, \
             Candidate apply {candidate_ms:.1} ms",
            ops.len()
        );
    }
}
