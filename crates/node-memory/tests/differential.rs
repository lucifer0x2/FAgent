//! Equivalence oracle: random batches of stored `Operation`s applied to the lib and to
//! the reference, `Candidate::from_snapshot` + `apply` + `prune_links` + a full
//! `validate()`. Accepted batches must leave identical rows (nodes with sort keys,
//! tokens, styles, notes) and identical changed ids, with indexes equal to a rebuild;
//! per-operation rejections must carry the same error. End-of-batch rejections must
//! agree on accept/reject and carry no operation index; when several rules fail the
//! first error may differ, and those cases are counted and printed.
mod support;

use fagent_node_memory::{MemoryConfig, MemoryStats, NodeMemory, SystemClock};
use std::sync::Arc;
use support::batch_oracle::{round as batch_round, Totals};
use support::oracle::{round, Tally};
use support::random_ops::Gen;
use support::*;

/// Generator switches of one run.
#[derive(Clone, Copy, Default)]
struct Mix {
    /// Pages, root frames, tokens, styles, themes, flows and links.
    extended: bool,
    /// Property errors.
    invalid: bool,
    /// Overrides, animations with triggers, deep chains, dimension tokens with themes,
    /// theme-scoped instances and master edits (`random_cases`).
    cases: bool,
}

fn gen<'a>(rng: &'a mut Rng, mix: Mix) -> Gen<'a> {
    Gen {
        rng,
        counter: 0,
        extended: mix.extended,
        invalid: mix.invalid,
        cases: mix.cases,
    }
}

/// `ProjectState::apply` (`Txn` and the delta rules) against the reference for `seeds`.
fn state_run(seeds: std::ops::RangeInclusive<u64>, mix: Mix) -> Tally {
    let mut tally = Tally::default();
    for seed in seeds {
        let mut rng = Rng::new(seed);
        let mut gen = gen(&mut rng, mix);
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
        "{} batches: accepted {}, rejected {}",
        tally.accepted + tally.rejected(),
        tally.accepted,
        tally.rejected()
    );
    for (kind, count) in &tally.rejected {
        eprintln!("  {count:>4} {kind}");
    }
    eprintln!(
        "end-of-batch error order differences: {}",
        tally.order.len()
    );
    for line in &tally.order {
        eprintln!("  {line}");
    }
    tally
}

#[test]
fn random_batches_match_candidate() {
    let tally = state_run(1..=8, Mix::default());
    assert!(tally.accepted > 600 && tally.rejected() > 300);
    assert!(tally.rejected.len() >= 8, "rejection kinds covered");
}

/// Per-operation property errors come from the `Txn` itself and equal `Candidate`'s.
#[test]
fn random_property_batches_match_candidate() {
    let mix = Mix {
        invalid: true,
        ..Mix::default()
    };
    let tally = state_run(1..=16, mix);
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

/// The delta-rule cases on the state alone.
#[test]
fn random_case_batches_match_candidate() {
    let mix = Mix {
        extended: true,
        cases: true,
        ..Mix::default()
    };
    let tally = state_run(201..=212, mix);
    assert!(tally.accepted > 600 && tally.rejected.len() >= 15);
}

/// `HotProject::begin` against the reference for `seeds`.
fn begin_run(seeds: std::ops::RangeInclusive<u64>, mix: Mix) -> (Totals, MemoryStats) {
    let memory = NodeMemory::new(MemoryConfig::default(), Arc::new(SystemClock::new()));
    let mut totals = Totals::default();
    for seed in seeds {
        let mut rng = Rng::new(seed);
        let mut pick = Rng::new(seed + 1000);
        let mut gen = gen(&mut rng, mix);
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
    }
    report(&totals);
    let stats = memory.stats();
    assert!(totals.differences.is_empty(), "differences found");
    assert_eq!(stats.commits as usize, totals.committed);
    assert_eq!(stats.rollbacks as usize, totals.dropped);
    (totals, stats)
}

fn report(totals: &Totals) {
    eprintln!(
        "{} batches: accepted {} (committed {}, dropped {}), rejected {}",
        totals.batches,
        totals.accepted,
        totals.committed,
        totals.dropped,
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
    let mix = Mix {
        extended: true,
        ..Mix::default()
    };
    let (totals, _) = begin_run(1..=24, mix);
    assert!(totals.accepted > 2000 && totals.rejected.len() >= 12);
}

/// The same with property errors.
#[test]
fn begin_property_errors_match_candidate() {
    let mix = Mix {
        extended: true,
        invalid: true,
        ..Mix::default()
    };
    let (totals, stats) = begin_run(101..=116, mix);
    assert!(totals.accepted > 600 && totals.rejected.len() >= 20);
    assert_eq!(
        stats.rejected as usize,
        totals.rejected.values().sum::<usize>()
    );
}

/// Overrides, animation triggers, deep chains repaired later, dimension tokens with
/// theme branches, theme-scoped instances and master edits (`random_cases`).
#[test]
fn begin_cases_match_candidate() {
    let mix = Mix {
        extended: true,
        invalid: false,
        cases: true,
    };
    let (totals, _) = begin_run(201..=224, mix);
    assert!(totals.accepted > 1500 && totals.rejected.len() >= 50);
}
