//! Command-line options. `--low-copy` keeps at most one snapshot copy alive at a
//! time (no clone baseline, no `to_snapshot`), as needed for multi-million projects.
pub struct Args {
    pub nodes: usize,
    pub seed: u64,
    /// Repeats for read and write operations.
    pub runs: usize,
    /// Repeats of read file -> parse -> load, each from a fresh file read.
    pub load_runs: usize,
    pub low_copy: bool,
    /// First load parses from a buffered file reader instead of whole-file bytes.
    pub reader: bool,
    /// Spike 1b option to run instead of the default benchmark: a, ord, hash, radix.
    pub spike: Option<String>,
}

const USAGE: &str =
    "usage: bench [--nodes N] [--seed S] [--runs R] [--load-runs L] [--low-copy] [--reader] [--spike a|ord|hash|radix]";

impl Args {
    pub fn parse() -> Self {
        let mut args = Self {
            nodes: 200_000,
            seed: 1,
            runs: 21,
            load_runs: 7,
            low_copy: false,
            reader: false,
            spike: None,
        };
        let mut it = std::env::args().skip(1);
        while let Some(flag) = it.next() {
            if flag == "--spike" {
                args.spike = Some(it.next().expect(USAGE));
                continue;
            }
            let mut value = || it.next().expect(USAGE).parse::<usize>().expect(USAGE);
            match flag.as_str() {
                "--nodes" => args.nodes = value(),
                "--seed" => args.seed = value() as u64,
                "--runs" => args.runs = value(),
                "--load-runs" => args.load_runs = value(),
                "--low-copy" => args.low_copy = true,
                "--reader" => args.reader = true,
                _ => panic!("{USAGE}"),
            }
        }
        assert!(args.runs > 0 && args.load_runs > 0, "{USAGE}");
        args
    }
}
