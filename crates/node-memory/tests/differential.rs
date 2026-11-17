//! Equivalence oracle: random batches of stored `Operation`s applied to the lib and to
//! `Candidate::from_snapshot` + `apply` + `prune_links`. Accepted batches must leave
//! identical rows (nodes with sort keys, tokens, styles, notes) and identical changed
//! ids, with indexes equal to a rebuild; rejections must carry the same error. The only
//! allowed difference is `oracle::lib_only`.
mod support;

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
