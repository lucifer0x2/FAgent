//! Process memory. RSS (via `ps`) misses pages the OS compressed or swapped, so on
//! macOS the physical footprint (resident + compressed, what Activity Monitor shows)
//! is reported too. Peaks come from the kernel, not from sampling.
use std::process::Command;

const MIB: f64 = 1024.0 * 1024.0;

pub fn mib(bytes: usize) -> f64 {
    bytes as f64 / MIB
}

/// Current and peak memory of this process in MiB.
#[derive(Clone, Copy)]
pub struct Sample {
    pub rss: f64,
    pub peak_rss: f64,
    /// macOS physical footprint; RSS elsewhere.
    pub footprint: f64,
    pub peak_footprint: f64,
}

pub fn sample() -> Sample {
    let peak_rss = peak_rss_mib();
    let rss = rss_mib();
    let (footprint, peak_footprint) = footprint_mib().unwrap_or((rss, peak_rss));
    Sample {
        rss,
        peak_rss,
        footprint,
        peak_footprint,
    }
}

/// Resident set size via `ps` (macOS and Linux).
fn rss_mib() -> f64 {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps");
    let kib: f64 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0.0);
    kib * 1024.0 / MIB
}

/// `getrusage` `ru_maxrss`: bytes on macOS, KiB on Linux.
fn peak_rss_mib() -> f64 {
    // SAFETY: `getrusage` only writes the zeroed struct we pass.
    let usage = unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        assert_eq!(libc::getrusage(libc::RUSAGE_SELF, &mut usage), 0);
        usage
    };
    let unit = if cfg!(target_os = "macos") {
        1.0
    } else {
        1024.0
    };
    usage.ru_maxrss as f64 * unit / MIB
}

/// macOS `proc_pid_rusage`: current and lifetime-max physical footprint.
#[cfg(target_os = "macos")]
fn footprint_mib() -> Option<(f64, f64)> {
    // SAFETY: the kernel fills the zeroed v4 struct whose address we pass.
    let info = unsafe {
        let mut info: libc::rusage_info_v4 = std::mem::zeroed();
        let buffer = &mut info as *mut libc::rusage_info_v4 as *mut libc::rusage_info_t;
        let pid = std::process::id() as libc::c_int;
        (libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V4, buffer) == 0).then_some(info)
    }?;
    let current = info.ri_phys_footprint as f64 / MIB;
    Some((current, info.ri_lifetime_max_phys_footprint as f64 / MIB))
}

#[cfg(not(target_os = "macos"))]
fn footprint_mib() -> Option<(f64, f64)> {
    None
}

pub fn header() {
    println!("| memory stage | RSS MiB | footprint MiB | note |\n| --- | ---: | ---: | --- |");
}

pub fn row(stage: &str, s: Sample, note: &str) {
    println!("| {stage} | {:.0} | {:.0} | {note} |", s.rss, s.footprint);
}

pub fn peak_row(stage: &str, s: Sample) {
    let note = "ru_maxrss / lifetime max footprint";
    println!(
        "| {stage} | {:.0} | {:.0} | {note} |",
        s.peak_rss, s.peak_footprint
    );
}
