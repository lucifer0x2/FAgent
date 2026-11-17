//! Load-path timings and the memory profile of the first load.
use crate::args::Args;
use crate::memory::{self, mib, sample};
use crate::support::{each, median, row};
use fagent_node_memory::{Limits, ProjectState};
use fagent_scene_input::{Snapshot, MAX_NODES};
use std::path::Path;
use std::time::{Duration, Instant};

/// A cap that admits the fixture plus the benchmark's own creates.
pub fn limits(authored: usize) -> Limits {
    Limits {
        max_nodes: MAX_NODES.max(authored + 10_000),
    }
}

fn parse(bytes: &[u8]) -> (Snapshot, Duration) {
    let start = Instant::now();
    let snapshot = serde_json::from_slice(bytes).expect("fixture JSON");
    (snapshot, start.elapsed())
}

/// Parses straight from the file through a 1 MiB buffer: no whole-file copy.
fn parse_reader(path: &Path) -> (Snapshot, Duration) {
    let start = Instant::now();
    let file = std::fs::File::open(path).expect("fixture");
    let reader = std::io::BufReader::with_capacity(1 << 20, file);
    let snapshot = serde_json::from_reader(reader).expect("fixture JSON");
    (snapshot, start.elapsed())
}

fn load(snapshot: Snapshot) -> (ProjectState, Duration) {
    let limits = limits(snapshot.nodes.len());
    let start = Instant::now();
    let state = ProjectState::from_snapshot_with(snapshot, limits).expect("fixture loads");
    (state, start.elapsed())
}

/// First load (read, parse, drop the bytes, load) with memory at each stage; returns
/// the state and this run's parse and load times.
pub fn first_load(path: &Path, reader: bool) -> (ProjectState, Duration, Duration) {
    let file = std::fs::metadata(path).expect("fixture").len() as usize;
    let start = sample();
    let bytes = match reader {
        true => Vec::new(),
        false => std::fs::read(path).expect("fixture"),
    };
    let read = sample();
    let (snapshot, parse_time) = match reader {
        true => parse_reader(path),
        false => parse(&bytes),
    };
    let authored = snapshot.nodes.len();
    let parsed = sample();
    drop(bytes);
    let dropped = sample();
    let (state, load_time) = load(snapshot);
    let loaded = sample();
    let estimated = state.estimated_bytes();
    let per_node = |mib: f64| format!("{:.0} B/node", mib * 1024.0 * 1024.0 / authored as f64);
    memory::header();
    println!(
        "| fixture file on disk | {:.0} | | {file} bytes |",
        mib(file)
    );
    memory::row("before read", start, "");
    memory::row("after file read", read, "");
    let note = match reader {
        true => "Snapshot (--reader: parsed through a 1 MiB buffer)",
        false => "file bytes + Snapshot",
    };
    memory::row("after parse", parsed, note);
    memory::row("after dropping file bytes", dropped, "Snapshot only");
    let note = format!(
        "RSS {}, footprint {}",
        per_node(loaded.rss),
        per_node(loaded.footprint)
    );
    memory::row("after load (snapshot moved in)", loaded, &note);
    memory::peak_row("peak through first load", loaded);
    println!(
        "| estimated_bytes (lib) | {:.0} | | {} |",
        mib(estimated),
        per_node(mib(estimated))
    );
    println!("\n{authored} authored nodes; parse {parse_time:?}, load + index {load_time:?} (first run)\n");
    (state, parse_time, load_time)
}

/// Further fresh read -> parse -> load runs, one copy alive at a time; prints medians
/// including the first run.
pub fn repeat_loads(args: &Args, path: &Path, first: (Duration, Duration)) {
    let (mut parses, mut loads) = (vec![first.0], vec![first.1]);
    for _ in 1..args.load_runs {
        let bytes = std::fs::read(path).expect("fixture");
        let (snapshot, parse_time) = parse(&bytes);
        drop(bytes);
        let (state, load_time) = load(snapshot);
        drop(state);
        parses.push(parse_time);
        loads.push(load_time);
    }
    let note = format!("{} runs, fresh read each", args.load_runs);
    row("JSON parse (serde_json -> Snapshot)", median(parses), &note);
    row("load + index build", median(loads), &note);
}

/// Today's per-read baseline: a full `Snapshot` clone. Holds two copies.
pub fn clone_baseline(args: &Args, state: &ProjectState) {
    let snapshot = state.to_snapshot();
    let clone = each(args.load_runs, |_| (), |_| snapshot.clone());
    row("Snapshot clone (today's per-read baseline)", clone, "");
}
