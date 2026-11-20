//! Runs one measured action `RUNS` times after `WARMUP` unmeasured runs and reduces
//! the wall times, the per-phase times (`bench_phase` spans, summed per name and run)
//! and the work counts to median, p95 and min.
use fagent_storage::bench_phase;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Instant;

pub const RUNS: usize = 30;
pub const WARMUP: usize = 2;

#[derive(Default)]
pub struct Samples {
    totals: Vec<f64>,
    phases: BTreeMap<&'static str, Vec<f64>>,
    counts: BTreeMap<&'static str, Vec<f64>>,
}

impl Samples {
    /// Times `action(run)` for every run; warm-up runs are recorded nowhere.
    pub fn collect(mut action: impl FnMut(usize)) -> Self {
        Self::collect_prepared(|_| (), |run, _| action(run), drop)
    }

    /// As `collect`, timing only `action(run, &state)`: `setup(run)` builds the state
    /// before the clock starts and `finish(state)` checks and drops it after it stops.
    pub fn collect_prepared<S>(
        mut setup: impl FnMut(usize) -> S,
        mut action: impl FnMut(usize, &S),
        mut finish: impl FnMut(S),
    ) -> Self {
        let mut samples = Self::default();
        for run in 0..WARMUP + RUNS {
            let state = setup(run);
            bench_phase::take();
            bench_phase::take_counts();
            let start = Instant::now();
            action(run, &state);
            let total = start.elapsed().as_secs_f64() * 1e3;
            let (phases, counts) = (bench_phase::take(), bench_phase::take_counts());
            finish(state);
            if run >= WARMUP {
                samples.push(total, phases, counts);
            }
        }
        samples
    }

    fn push(
        &mut self,
        total: f64,
        phases: Vec<(&'static str, u64)>,
        counts: Vec<(&'static str, usize)>,
    ) {
        self.totals.push(total);
        let mut sums: BTreeMap<&'static str, f64> = BTreeMap::new();
        for (name, micros) in phases {
            *sums.entry(name).or_default() += micros as f64 / 1e3;
        }
        for (name, ms) in sums {
            self.phases.entry(name).or_default().push(ms);
        }
        let mut sums: BTreeMap<&'static str, f64> = BTreeMap::new();
        for (name, value) in counts {
            *sums.entry(name).or_default() += value as f64;
        }
        for (name, value) in sums {
            self.counts.entry(name).or_default().push(value);
        }
    }

    /// `{"totalMs":{median,p95,min},"phasesMs":{name:median},"counts":{name:median}}`;
    /// a phase that ran in only some runs reports the median of those runs.
    pub fn report(self) -> Value {
        let medians = |map: BTreeMap<&str, Vec<f64>>| -> BTreeMap<String, f64> {
            map.into_iter()
                .map(|(name, values)| (name.to_owned(), round(stat(values).0)))
                .collect()
        };
        let (median, p95, min) = stat(self.totals);
        json!({"totalMs": {"median": round(median), "p95": round(p95), "min": round(min)},
            "phasesMs": medians(self.phases), "counts": medians(self.counts)})
    }
}

/// Median, nearest-rank p95 and min.
fn stat(mut values: Vec<f64>) -> (f64, f64, f64) {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let rank = ((values.len() as f64 * 0.95).ceil() as usize).clamp(1, values.len());
    (values[values.len() / 2], values[rank - 1], values[0])
}

fn round(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}
