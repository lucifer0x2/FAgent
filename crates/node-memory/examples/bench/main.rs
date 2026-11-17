//! Node-memory benchmark on a generated fixture (default 200k expanded nodes). Run:
//! `cargo run --release -p fagent-node-memory --example bench -- [--nodes N]
//!  [--seed S] [--runs R] [--load-runs L] [--low-copy] [--reader]`
//! Prints Markdown tables (memory stages, then medians); writes only the fixture under
//! target/. Use `--low-copy` for multi-million fixtures: no snapshot clone baseline.
mod args;
mod load;
mod memory;
mod reads;
mod support;
mod txn;
mod writes;

use memory::{mib, sample};

fn main() {
    let args = args::Args::parse();
    let path = support::fixture(args.nodes, args.seed);
    // Memory first, before repeated runs leave freed pages in the allocator.
    let (mut state, parse, load) = load::first_load(&path, args.reader);

    println!("| operation | median | note |\n| --- | ---: | --- |");
    if !args.low_copy {
        load::clone_baseline(&args, &state);
    }
    reads::reads(&state, args.runs);
    reads::slice(&state, args.runs);
    writes::writes(&mut state, args.runs);
    txn::staged(&mut state, args.runs);
    txn::stamp_memory(&mut state);
    let ops = sample();
    let estimated = mib(state.estimated_bytes());
    let (authored, revision) = (state.len(), state.revision());
    drop(state);
    load::repeat_loads(&args, &path, (parse, load));
    let end = sample();
    println!(
        "\nAfter operations: RSS {:.0} MiB, footprint {:.0} MiB, peak RSS {:.0} MiB, peak \
         footprint {:.0} MiB; estimated {estimated:.0} MiB, {authored} nodes, revision \
         {revision}. Peak at exit (incl. repeated loads): RSS {:.0} MiB, footprint {:.0} MiB.",
        ops.rss, ops.footprint, ops.peak_rss, ops.peak_footprint, end.peak_rss, end.peak_footprint
    );
}
