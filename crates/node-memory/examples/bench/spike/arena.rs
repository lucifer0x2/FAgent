//! Option (a), today's engine: the slot arena with its undo log (`ProjectState`) plus the
//! published `ReadView` (256-shard persistent maps), loaded and written through
//! `Registry`/`ProjectHandle` exactly as `NodeMemory` does: `begin` stages under the
//! write lock, `commit` advances and publishes the reader view, drop rolls back.
use super::common::{self, Workload};
use super::read;
use crate::args::Args;
use crate::memory::sample;
use crate::support::{header_p95, per_op, row, row_p95, samples};
use fagent_node_memory::{Limits, Operation, ProjectHandle, Registry, RegistryConfig, SystemClock};
use fagent_scene_input::read_scope::ReadScope;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Phases {
    apply: Vec<Duration>,
    publish: Vec<Duration>,
    total: Vec<Duration>,
    rows: usize,
}

fn registry(max_nodes: usize) -> Registry {
    let config = RegistryConfig {
        ttl: Duration::from_secs(3600),
        budget_bytes: usize::MAX,
    };
    Registry::with_limits(config, Arc::new(SystemClock::new()), Limits { max_nodes })
}

fn batch(handle: &ProjectHandle, ops: &[Operation], keep: bool, phases: &mut Phases) {
    let start = Instant::now();
    let staged = handle.begin(ops).expect("op applies");
    let applied = Instant::now();
    let delta = staged.delta();
    phases.rows = delta.nodes.upserted.len() + delta.nodes.deleted.len();
    match keep {
        true => drop(staged.commit()),
        false => staged.abort(),
    }
    let end = Instant::now();
    phases.apply.push(applied - start);
    phases.publish.push(end - applied);
    phases.total.push(end - start);
}

pub fn run(args: &Args, path: &Path) {
    let (snapshot, parsed) = common::snapshot(path);
    let (text, section) = common::targets(&snapshot);
    let authored = snapshot.nodes.len();
    let registry = registry(common::max_nodes(&snapshot));
    let start = Instant::now();
    let (handle, _) = registry.load(snapshot).expect("fixture loads");
    let first_load = start.elapsed();
    common::memory_rows("arena + ReadView", parsed, sample(), authored);
    println!(
        "estimated_bytes (lib, arena + view): {:.0} B/node\n",
        handle.estimated_bytes() as f64 / authored as f64
    );

    header_p95();
    let view = handle.read_view();
    let ids: Vec<String> = view
        .to_snapshot()
        .nodes
        .iter()
        .step_by((authored / 65_536).max(1))
        .map(|n| n.id.clone())
        .collect();
    let lookup = per_op(args.runs, 10_000, |i| {
        view.node(&ids[(i * 7919) % ids.len()]).is_some()
    });
    row("get node by id", lookup, "per lookup, median (ReadView)");
    for (target, note) in [
        ("root_10", "normal root frame"),
        ("frame_wide", "14k-node subtree"),
    ] {
        let n = read::frame_view(&view, target).len();
        let times = samples(args.runs, |_| (), |_| read::frame_view(&view, target).len());
        row_p95(
            &format!("read: frame closure {target}"),
            &times,
            &format!("{n} nodes, {note}"),
        );
    }
    let scoped = view.scoped();
    let times = samples(
        args.runs,
        |_| (),
        |_| {
            scoped
                .expand(ReadScope::Target("root_10"), false)
                .map(|n| n.expanded.len())
        },
    );
    row_p95(
        "read: scoped expand root_10 (production)",
        &times,
        "closure + instance expansion; context only",
    );
    let times = samples(
        args.runs,
        |_| (),
        |_| handle.read().slice("root_10").map(|s| s.nodes.len()),
    );
    row_p95(
        "read: arena slice root_10",
        &times,
        "ProjectState::slice (cloned nodes)",
    );
    drop(scoped);
    drop(view);

    for (name, keep) in [
        ("commit: update props (1 op)", true),
        ("reject: update props (1 op, dropped)", false),
    ] {
        let mut phases = Phases::default();
        for run in 0..args.runs {
            batch(&handle, &[common::update(&text, run)], keep, &mut phases);
        }
        report(name, &phases, "begin + apply + commit/publish");
    }
    let workloads: Vec<Workload> = common::workloads(text, section);
    for w in &workloads {
        let mut phases = Phases::default();
        for run in 0..args.runs.min(w.cap) {
            batch(&handle, &(w.ops)(run), true, &mut phases);
        }
        report(
            w.name,
            &phases,
            &format!("{} runs; {}", args.runs.min(w.cap), w.note),
        );
    }
    let view = handle.read_view();
    println!(
        "\nRevision {}, {} nodes after writes.\n",
        view.metadata().revision,
        view.len()
    );
    drop(view);
    let id = handle.id().to_owned();
    drop(handle);
    registry.remove(&id);

    let mut loads = vec![first_load];
    for _ in 1..args.load_runs {
        let (snapshot, _) = common::snapshot(path);
        let start = Instant::now();
        let (handle, _) = registry.load(snapshot).expect("fixture loads");
        loads.push(start.elapsed());
        drop(handle);
        registry.remove(&id);
    }
    header_p95();
    row_p95(
        "load + index build",
        &loads,
        &format!("{} runs (arena + ReadView)", args.load_runs),
    );
}

fn report(name: &str, phases: &Phases, note: &str) {
    row_p95(
        name,
        &phases.total,
        &format!("{} rows; {note}", phases.rows),
    );
    row_p95(
        "  of which apply (begin + ops, undo log, delta)",
        &phases.apply,
        "",
    );
    row_p95(
        "  of which publish (ReadView advance + swap)",
        &phases.publish,
        "",
    );
}
