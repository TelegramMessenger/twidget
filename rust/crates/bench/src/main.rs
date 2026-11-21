//! twidget_bench: `twidget_core_bench` on the Rust renderer, with its arguments and its report.
//!   twidget_bench [--step|--trace] [mixed|transform|swarm|css|drag|drag-doc|canvas] [count] [frames]
//! `--step` runs on a step clock (1000 ms, a 60th of a second more at each tick), so that every run does the same work;
//! `--trace` does too and writes a line of hashes for each call of the loop before the report. `oracle/bench_oracle`
//! takes the same flags for the C++.

#![forbid(unsafe_code)]

use twidget_bench::{Options, stepped, timed, traced};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let flag = args.first().filter(|a| *a == "--step" || *a == "--trace").cloned();
    if flag.is_some() {
        args.remove(0);
    }
    let options = Options::parse(&args);
    let report = match flag.as_deref() {
        Some("--step") => stepped(&options),
        Some(_) => traced(&options),
        None => timed(&options),
    };
    print!("{report}");
}
