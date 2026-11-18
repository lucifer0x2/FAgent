//! Spike 1b of Task 41: the engine (a, since phase 2 one persistent state over a radix
//! trie) against the spike prototypes, one persistent state with `imbl` (b) or a
//! hand-written radix trie (c). Measurement only; the prototypes are not wired into the
//! engine or storage. Run one option per process so memory rows are not mixed:
//! `cargo run --release -p fagent-node-memory --example bench -- --spike a|ord|hash|radix`.
mod common;
mod engine;
mod load;
mod ops;
mod persistent;
mod pmap;
mod radix;
mod read;
mod state;

use crate::args::Args;
use std::path::Path;

pub fn run(option: &str, args: &Args, path: &Path) {
    match option {
        "a" => engine::run(args, path),
        "ord" => persistent::run::<pmap::Ord>(args, path),
        "hash" => persistent::run::<pmap::Hash>(args, path),
        "radix" => persistent::run::<pmap::Trie>(args, path),
        _ => panic!("--spike a|ord|hash|radix"),
    }
}
