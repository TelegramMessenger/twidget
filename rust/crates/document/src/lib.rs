//! The renderer's document on the Rust litehtml (stage 4.2): renderer.cpp's `Container` as the fork's
//! `document_container` ([`Container`]: fonts through the backend with the width cache, the text breaker, images and
//! their table, media features, every paint call into the display list), and [`DocumentEngine`], the
//! [`twidget_renderer::Engine`] the renderer runs on: a `twidget_litehtml::Document` with that container, the
//! element index, the declaration scans, the paint trace and what covers what, and the free helpers renderer.cpp reads
//! the render tree with for queries, layers and input ([`boxes`]).
//!
//! It is a crate of its own rather than a module of `twidget_renderer` so that the renderer stays generic over its
//! [`twidget_renderer::Engine`] seam (and its tape comparison builds without litehtml), and this crate is where litehtml
//! and the renderer meet.

#![forbid(unsafe_code)]

pub mod boxes;
pub mod container;
mod cxx;
mod engine;

pub use container::{Container, MIN_HEIGHT, data_mime};
pub use engine::{DocumentEngine, Process};
