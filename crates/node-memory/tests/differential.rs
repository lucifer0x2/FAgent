//! Equivalence oracle: random batches of stored `Operation`s applied to the lib and to
//! `Candidate::from_snapshot` + `apply` + `prune_links`. Accepted batches must leave
//! identical rows (nodes with sort keys, tokens, styles, notes) and identical changed
//! ids, with indexes equal to a rebuild; rejections must carry the same error. The only
//! allowed difference is `oracle::lib_only`.
mod support;

use fagent_node_memory::{MemoryConfig, MemoryStats, NodeMemory, SystemClock};
use std::sync::Arc;
use support::batch_oracle::{round as batch_round, Totals};
use support::oracle::{round, Tally};
use support::random_ops::Gen;
use support::*;

/// `ProjectState::apply` (the `Txn` alone) against `Candidate` for `seeds`.
fn state_run(seeds: std::ops::RangeInclusive<u64>, invalid: bool) -> Tally {
    let mut tally = Tally::default();
    for seed in seeds {
        let mut rng = Rng::new(seed);
        let mut gen = Gen {
            rng: &mut rng,
            counter: 0,
            extended: false,
            invalid,
        };
        let mut state = load(small());
        for n in 0..300 {
            let ops = gen.batch(&state.to_snapshot());
            round(
                &mut state,
                &ops,
                &mut tally,
                &format!("seed {seed} round {n}"),
            );
        }
    }
    eprintln!(
        "accepted {}, rejected {}, lib-only {}",
        tally.accepted,
        tally.rejected(),
        tally.lib_only
    );
    for (kind, count) in &tally.rejected {
        eprintln!("  {count:>4} {kind}");
    }
    tally
}

#[test]
fn random_batches_match_candidate() {
    let tally = state_run(1..=8, false);
    assert!(tally.accepted > 600 && tally.rejected() > 300);
    assert!(tally.rejected.len() >= 8, "rejection kinds covered");
}

/// Per-operation property errors come from the `Txn` itself and equal `Candidate`'s.
#[test]
fn random_property_batches_match_candidate() {
    let tally = state_run(1..=16, true);
    assert!(tally.accepted > 600 && tally.rejected() > 1500);
    let property = |field: &str| {
        tally
            .rejected
            .keys()
            .any(|kind| kind.contains(&format!("Some(\"{field}")))
    };
    for field in [
        "props", "width", "color", "tokenId", "styleId", "token.", "style.",
    ] {
        assert!(property(field), "no rejection on {field}");
    }
}

/// `HotProject::begin` against `Candidate` apply + `validate()` for `seeds`.
fn begin_run(seeds: std::ops::RangeInclusive<u64>, invalid: bool) -> (Totals, MemoryStats) {
    let memory = NodeMemory::new(MemoryConfig::default(), Arc::new(SystemClock::new()));
    let mut totals = Totals::default();
    for seed in seeds {
        let mut rng = Rng::new(seed);
        let mut pick = Rng::new(seed + 1000);
        let mut gen = Gen {
            rng: &mut rng,
            counter: 0,
            extended: true,
            invalid,
        };
        let mut snapshot = small();
        snapshot.project_id = format!("diff{seed}");
        let project = memory.load(snapshot.clone()).expect("project loads");
        let (batches, accepted) = (totals.batches, totals.accepted);
        for n in 0..300 {
            let ops = gen.batch(&snapshot);
            let at = format!("seed {seed} batch {n}");
            batch_round(
                &memory,
                &project,
                &mut snapshot,
                &ops,
                &mut pick,
                &mut totals,
                &at,
            );
        }
        eprintln!(
            "seed {seed}: {} batches, {} accepted",
            totals.batches - batches,
            totals.accepted - accepted
        );
        assert!(!project.diverged(), "seed {seed}: diverged");
    }
    report(&totals);
    let stats = memory.stats();
    eprintln!(
        "per-op rejections: {} direct, {} routed; engine: {} direct, {} routed",
        totals.per_op_direct, totals.per_op_routed, stats.rejected_direct, stats.rejected_routed
    );
    assert!(totals.differences.is_empty(), "differences found");
    assert_eq!(stats.divergences, 0);
    assert_eq!(stats.commits as usize, totals.committed);
    assert_eq!(stats.rollbacks as usize, totals.dropped);
    (totals, stats)
}

fn report(totals: &Totals) {
    eprintln!(
        "{} batches: accepted {} (committed {}, dropped {}; local {}, full {}), rejected {}",
        totals.batches,
        totals.accepted,
        totals.committed,
        totals.dropped,
        totals.local,
        totals.full,
        totals.rejected.values().sum::<usize>()
    );
    for (kind, count) in &totals.rejected {
        eprintln!("  {count:>4} {kind}");
    }
    eprintln!(
        "end-of-batch error order differences: {}",
        totals.order.len()
    );
    for line in &totals.order {
        eprintln!("  {line}");
    }
    eprintln!("differences: {}", totals.differences.len());
    for line in &totals.differences {
        eprintln!("  {line}");
    }
}

/// Generated batches that cover pages, root frames, tokens with themes and aliases,
/// styles, links and flow starts besides the structural and note operations.
#[test]
fn begin_matches_candidate() {
    let (totals, _) = begin_run(1..=24, false);
    assert!(
        totals.local > 100 && totals.full > 100,
        "both paths covered"
    );
    assert!(totals.accepted > 2000 && totals.rejected.len() >= 12);
}

/// The same with property errors: per-operation rejections are `Txn`'s own errors,
/// returned without the reference, and equal `Candidate`'s.
#[test]
fn begin_property_errors_match_candidate() {
    let (totals, stats) = begin_run(101..=116, true);
    assert!(totals.accepted > 600 && totals.rejected.len() >= 20);
    assert!(
        totals.per_op_direct > 1000,
        "per-op errors returned directly"
    );
    // Only a reparent moving a subtree too deep is per-op and still routed.
    assert!(totals.per_op_routed * 100 <= totals.per_op_direct);
    assert!(stats.rejected_direct >= totals.per_op_direct as u64);
}
