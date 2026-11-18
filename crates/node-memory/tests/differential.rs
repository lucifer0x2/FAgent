//! Equivalence oracle: random batches of stored `Operation`s applied to the lib and to
//! `Candidate::from_snapshot` + `apply` + `prune_links`. Accepted batches must leave
//! identical rows (nodes with sort keys, tokens, styles, notes) and identical changed
//! ids, with indexes equal to a rebuild; rejections must carry the same error. The only
//! allowed difference is `oracle::lib_only`.
mod support;

use fagent_node_memory::{MemoryConfig, NodeMemory, SystemClock};
use std::sync::Arc;
use support::batch_oracle::{round as batch_round, Totals};
use support::oracle::{round, Tally};
use support::random_ops::Gen;
use support::*;

#[test]
fn random_batches_match_candidate() {
    let mut tally = Tally::default();
    for seed in 1..=8 {
        let mut rng = Rng::new(seed);
        let mut gen = Gen {
            rng: &mut rng,
            counter: 0,
            extended: false,
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
    assert!(tally.accepted > 600 && tally.rejected() > 300);
    assert!(tally.rejected.len() >= 8, "rejection kinds covered");
}

/// `HotProject::begin` against `Candidate` apply + `validate()` over generated batches
/// that cover pages, root frames, tokens with themes and aliases, styles, links and
/// flow starts besides the structural and note operations.
#[test]
fn begin_matches_candidate() {
    let memory = NodeMemory::new(MemoryConfig::default(), Arc::new(SystemClock::new()));
    let mut totals = Totals::default();
    for seed in 1..=12u64 {
        let mut rng = Rng::new(seed);
        let mut pick = Rng::new(seed + 1000);
        let mut gen = Gen {
            rng: &mut rng,
            counter: 0,
            extended: true,
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
    assert!(totals.differences.is_empty(), "differences found");
    let stats = memory.stats();
    assert_eq!(stats.divergences, 0);
    assert_eq!(stats.commits as usize, totals.committed);
    assert_eq!(stats.rollbacks as usize, totals.dropped);
    assert!(
        totals.local > 100 && totals.full > 100,
        "both paths covered"
    );
    assert!(totals.accepted > 1000 && totals.rejected.len() >= 12);
}
