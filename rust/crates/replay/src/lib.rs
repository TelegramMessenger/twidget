//! `tests/replay.cpp` on the Rust renderer end (stage 5.2, T5): a recording of C++ renderers used directly
//! (`tests/recording.h`, made by `tools/record`, `twidget_core_record` or `twidget_bench_record`) replayed through
//! [`twidget_ends::RendererEnd`] and [`twidget_ends::PlatformEnd`] over an in-memory channel, with the Rust renderer
//! on `twidget_document`'s engine behind the end. The platform end's Backend answers from each step's recorded calls
//! and the renderer's clock gives its recorded reads; every step is compared with what the recorded renderer gave:
//! its messages, the reports in their order, lists, damage, the Backend calls in order with their inputs and
//! answers, cursors, claims, selections, states, the name table's size and the clock reads it took.
//!
//! The two ends' test harness is `twidget_ends`' own (`tests/common`, the port of `tests/log_backend.h`,
//! `loopback.h` and `reports.h`), as the C++ replay includes those headers.

#![forbid(unsafe_code)]

#[path = "../../ends/tests/common/mod.rs"]
mod harness;

pub mod compare;
pub mod recording;
mod replay;

pub use harness::{Call, Op};
pub use replay::{Counts, Options, Perturb, Replay, Tree, replay_tree};
