//! Options (b) and (c): one persistent state. `begin` clones it in O(1), the batch
//! writes on the clone, reject drops it, commit publishes that clone; the delta comes
//! from a pointer diff of the node map (or the write set for `imbl::HashMap`).
use super::common::{self, Workload};
use super::ops::Txn;
use super::pmap::{Family, PMap};
use super::read;
use super::state::{Engine, State};
use crate::args::Args;
use crate::memory::sample;
use crate::support::{header_p95, per_op, row, row_p95, samples};
use fagent_node_memory::Operation;
use std::hint::black_box;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Phases {
    apply: Vec<Duration>,
    delta: Vec<Duration>,
    publish: Vec<Duration>,
    total: Vec<Duration>,
    rows: usize,
}

/// Changed node rows: keys whose node pointer differs (derived-only changes are not rows).
fn rows<F: Family>(base: &State<F>, next: &State<F>, touched: Vec<u32>) -> Vec<u32> {
    let mut keys = next.nodes.diff(&base.nodes).unwrap_or_else(|| {
        let mut keys = touched;
        keys.sort_unstable();
        keys.dedup();
        keys
    });
    keys.retain(|k| match (base.rec(*k), next.rec(*k)) {
        (Some(a), Some(b)) => !Arc::ptr_eq(&a.node, &b.node),
        (None, None) => false,
        _ => true,
    });
    keys
}

fn batch<F: Family>(engine: &mut Engine<F>, ops: &[Operation], keep: bool, phases: &mut Phases) {
    let start = Instant::now();
    let base = engine.view();
    let mut txn = Txn::begin(&base, &mut engine.interner);
    for op in ops {
        txn.apply(op).expect("spike op applies");
    }
    let (next, touched) = txn.finish();
    let applied = Instant::now();
    let changed = black_box(rows(&base, &next, touched));
    let diffed = Instant::now();
    match keep {
        true => engine.publish(next),
        false => drop(next),
    }
    drop(base);
    let end = Instant::now();
    phases.rows = changed.len();
    phases.apply.push(applied - start);
    phases.delta.push(diffed - applied);
    phases.publish.push(end - diffed);
    phases.total.push(end - start);
}

pub fn run<F: Family>(args: &Args, path: &Path) {
    let (snapshot, parsed) = common::snapshot(path);
    let (text, section) = common::targets(&snapshot);
    let authored = snapshot.nodes.len();
    let start = Instant::now();
    let (interner, state) = super::load::build::<F>(snapshot);
    let first_load = start.elapsed();
    let mut engine = Engine::new(interner, state);
    common::memory_rows(F::NAME, parsed, sample(), authored);
    println!("interned ids: {}\n", engine.interner.len());

    header_p95();
    let view = engine.view();
    let ids: Vec<String> = (0..authored as u32)
        .step_by((authored / 65_536).max(1))
        .filter_map(|k| view.rec(k).map(|r| r.node.id.clone()))
        .collect();
    let lookup = per_op(args.runs, 10_000, |i| {
        let id = &ids[(i * 7919) % ids.len()];
        engine.interner.get(id).and_then(|k| view.rec(k)).is_some()
    });
    row("get node by id", lookup, "per lookup, median");
    for (target, note) in [
        ("root_10", "normal root frame"),
        ("frame_wide", "14k-node subtree"),
    ] {
        let n = read::frame(&view, &engine.interner, target).len();
        let times = samples(
            args.runs,
            |_| (),
            |_| read::frame(&view, &engine.interner, target).len(),
        );
        row_p95(
            &format!("read: frame closure {target}"),
            &times,
            &format!("{n} nodes, {note}"),
        );
    }
    drop(view);

    for (name, keep) in [
        ("commit: update props (1 op)", true),
        ("reject: update props (1 op, dropped)", false),
    ] {
        let mut phases = Phases::default();
        for run in 0..args.runs {
            batch(
                &mut engine,
                &[common::update(&text, run)],
                keep,
                &mut phases,
            );
        }
        report(name, &phases, "begin + apply + delta + publish");
    }
    let workloads: Vec<Workload> = common::workloads(text, section);
    for w in &workloads {
        let mut phases = Phases::default();
        for run in 0..args.runs.min(w.cap) {
            batch(&mut engine, &(w.ops)(run), true, &mut phases);
        }
        report(
            w.name,
            &phases,
            &format!("{} runs; {}", args.runs.min(w.cap), w.note),
        );
    }
    let view = engine.view();
    println!(
        "\nRevision {}, {} nodes after writes.\n",
        view.revision, view.len
    );
    drop(view);
    drop(engine);

    let mut loads = vec![first_load];
    for _ in 1..args.load_runs {
        let (snapshot, _) = common::snapshot(path);
        let start = Instant::now();
        let built = super::load::build::<F>(snapshot);
        loads.push(start.elapsed());
        drop(built);
    }
    header_p95();
    row_p95(
        "load + index build",
        &loads,
        &format!("{} runs", args.load_runs),
    );
}

fn report(name: &str, phases: &Phases, note: &str) {
    row_p95(
        name,
        &phases.total,
        &format!("{} rows; {note}", phases.rows),
    );
    row_p95("  of which apply (begin + ops)", &phases.apply, "");
    row_p95("  of which delta (pointer diff)", &phases.delta, "");
    row_p95("  of which publish (swap + release)", &phases.publish, "");
}
