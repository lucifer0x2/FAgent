//! Timing, RSS and fixture helpers for the benchmark. Output files go under target/.
use std::hint::black_box;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

pub fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

/// Median over `runs` of one call each; `setup` output is passed in untimed.
pub fn each<S, T>(
    runs: usize,
    mut setup: impl FnMut(usize) -> S,
    mut f: impl FnMut(S) -> T,
) -> Duration {
    let samples = (0..runs)
        .map(|run| {
            let input = setup(run);
            let start = Instant::now();
            black_box(f(input));
            start.elapsed()
        })
        .collect();
    median(samples)
}

/// Median per-iteration time of `iters` calls, over `runs` repetitions.
pub fn per_op<T>(runs: usize, iters: usize, mut f: impl FnMut(usize) -> T) -> Duration {
    let samples = (0..runs)
        .map(|_| {
            let start = Instant::now();
            for i in 0..iters {
                black_box(f(i));
            }
            start.elapsed() / iters as u32
        })
        .collect();
    median(samples)
}

/// Generates the fixture under target/node-memory-bench and returns its path.
pub fn fixture(nodes: usize, seed: u64) -> PathBuf {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = crate_dir.join("../../target/node-memory-bench");
    std::fs::create_dir_all(&dir).expect("bench dir");
    let out = dir.join(format!("fixture-{nodes}-{seed}.json"));
    let status = Command::new("node")
        .arg(crate_dir.join("fixtures/generate.mjs"))
        .args(["--nodes", &nodes.to_string(), "--seed", &seed.to_string()])
        .arg("--out")
        .arg(&out)
        .status()
        .unwrap_or_else(|e| panic!("`node` (Node.js >= 20) must be on PATH: {e}"));
    assert!(status.success(), "generate.mjs failed");
    out
}

pub fn row(name: &str, median: Duration, note: &str) {
    let micros = median.as_secs_f64() * 1e6;
    let value = match micros {
        m if m >= 1000.0 => format!("{:.2} ms", m / 1000.0),
        m if m >= 1.0 => format!("{m:.2} µs"),
        m => format!("{:.0} ns", m * 1000.0),
    };
    println!("| {name:<44} | {value:>10} | {note} |");
}

/// Every sample of `runs` calls; `setup` output is passed in untimed.
pub fn samples<S, T>(
    runs: usize,
    mut setup: impl FnMut(usize) -> S,
    mut f: impl FnMut(S) -> T,
) -> Vec<Duration> {
    (0..runs)
        .map(|run| {
            let input = setup(run);
            let start = Instant::now();
            black_box(f(input));
            start.elapsed()
        })
        .collect()
}

/// The `q` quantile (0..=1) by nearest rank.
pub fn quantile(samples: &[Duration], q: f64) -> Duration {
    let mut sorted = samples.to_vec();
    sorted.sort();
    let rank = ((sorted.len() as f64 * q).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

fn fmt(d: Duration) -> String {
    let micros = d.as_secs_f64() * 1e6;
    match micros {
        m if m >= 1000.0 => format!("{:.2} ms", m / 1000.0),
        m if m >= 1.0 => format!("{m:.2} µs"),
        m => format!("{:.0} ns", m * 1000.0),
    }
}

/// A row with median and p95.
pub fn row_p95(name: &str, samples: &[Duration], note: &str) {
    let (median, p95) = (quantile(samples, 0.5), quantile(samples, 0.95));
    println!(
        "| {name:<44} | {:>10} | {:>10} | {note} |",
        fmt(median),
        fmt(p95)
    );
}

pub fn header_p95() {
    println!("| operation | median | p95 | note |\n| --- | ---: | ---: | --- |");
}
