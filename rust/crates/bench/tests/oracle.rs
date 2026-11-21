//! The two benches do the same work: every mode of tests/bench.cpp through `oracle/bench_oracle` (the C++ bench on a
//! step clock, each call of its loop written down with the Backend calls it made and the renderer's counters) and
//! through `twidget_bench`'s traced run, line for line, then the counts each reports.

use std::path::PathBuf;
use std::process::Command;

use twidget_bench::{Options, traced};

/// `oracle/bench_oracle`, found as `twidget_testkit::Oracle` finds the oracles.
fn oracle() -> Option<PathBuf> {
    let required = std::env::var_os("TWIDGET_ORACLE_REQUIRED").is_some();
    let Some(dir) = std::env::var_os("TWIDGET_ORACLE_DIR") else {
        assert!(!required, "TWIDGET_ORACLE_REQUIRED is set but TWIDGET_ORACLE_DIR is not");
        eprintln!("TWIDGET_ORACLE_DIR is not set: the comparison with the C++ is skipped");
        return None;
    };
    let program = PathBuf::from(dir).join(if cfg!(windows) { "bench_oracle.exe" } else { "bench_oracle" });
    assert!(program.is_file(), "no oracle at {}", program.display());
    Some(program)
}

/// What a run writes that does not depend on the time it took: the trace and the counts, without the timings.
fn steady(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.starts_with("BENCH mode=") && !l.starts_with("BENCH stages"))
        .map(str::to_string)
        .collect()
}

/// Every mode at twidget_bench_record's sizes, with frames enough for the swarm's log rows (every 12th frame), and the CSS
/// animations past the 256 layers too, where the rest paint in the document every frame.
const CASES: [(&str, &str, &str); 8] = [
    ("mixed", "60", "26"),
    ("transform", "60", "26"),
    ("swarm", "60", "26"),
    ("css", "60", "26"),
    ("css", "300", "26"),
    ("drag", "60", "26"),
    ("drag-doc", "60", "26"),
    ("canvas", "50", "26"),
];

#[test]
fn same_work_as_the_cpp_bench() {
    let Some(program) = oracle() else { return };
    for (mode, count, frames) in CASES {
        let run = Command::new(&program).args([mode, count, frames]).output().expect("bench_oracle starts");
        assert!(run.status.success(), "bench_oracle {mode} {count} {frames} ended with {}", run.status);
        let theirs = steady(&String::from_utf8_lossy(&run.stdout));
        let ours = steady(&traced(&Options::parse(&[mode.to_string(), count.to_string(), frames.to_string()])));
        if let Some(at) = (0..theirs.len().max(ours.len())).find(|&i| theirs.get(i) != ours.get(i)) {
            panic!(
                "{mode} {count} {frames}: line {} of {} in C++, {} in Rust differs:\n  C++  {}\n  Rust {}",
                at + 1,
                theirs.len(),
                ours.len(),
                theirs.get(at).map_or("(none)", String::as_str),
                ours.get(at).map_or("(none)", String::as_str)
            );
        }
        let paints = theirs.iter().filter(|l| l.starts_with("paint ")).count();
        assert!(paints > 0 && theirs.len() > 26, "{mode}: {} lines, {paints} paints", theirs.len());
        eprintln!("{mode} {count} {frames}: {} lines alike, {paints} paints", theirs.len());
    }
}
