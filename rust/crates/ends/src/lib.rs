//! TwidgetCore's two ends over a byte channel (`core/src/renderer_end.h`, `renderer_end.cpp`,
//! `platform_end.h`, `platform_end.cpp`; `docs/TWIDGET_CORE.md`, "Two ends over a byte channel"): a
//! renderer in one process whose Backend calls, frames and reports travel as `twidget_remote` messages to
//! a platform end in another, which checks them, calls the host's Backend and hands its host frames and
//! reports.
//!
//! What the C++ takes from `backend.h` and `renderer.h` is a trait here: [`Backend`] (fonts, text widths,
//! images, canvases), [`Clock`], and [`Renderer`], the calls [`RendererEnd`] makes of renderer.h's
//! `Renderer`; the Rust renderer of stage 4 implements it. The channels are [`Channel`] and
//! [`RendererChannel`].

#![forbid(unsafe_code)]

mod platform_end;
mod renderer_end;

pub use platform_end::{Host, PlatformEnd};
pub use renderer_end::{Output, Proxy, RendererEnd};

use twidget_json::Json;
use twidget_remote::GestureClaim;
use twidget_wire::{CanvasOp, DisplayList, FontMetrics, Rect};

/// What the platform provides to the litehtml container (`backend.h`): fonts with metrics, text measurement,
/// image decoding and the 2D canvas. All calls on the renderer's thread. Text is bytes, UTF-8 as the C++
/// `std::string`s hold it.
pub trait Backend {
    /// `families`: the CSS `font-family` list as written (quotes kept, comma separated); the backend resolves
    /// generics and falls back to its default face. `features`: OpenType features as `tag=value` pairs joined
    /// by commas, empty for the font's defaults. Returns a handle > 0, or 0 when no font at all could be made.
    fn create_font(&mut self, families: &[u8], size_px: f32, weight: i32, italic: bool, features: &[u8], out: &mut FontMetrics) -> i32;
    /// A canvas's font, as above: `create_font` with empty `features` unless the backend tells the two apart.
    fn create_canvas_font(&mut self, families: &[u8], size_px: f32, weight: i32, italic: bool, out: &mut FontMetrics) -> i32 {
        self.create_font(families, size_px, weight, italic, b"", out)
    }
    fn delete_font(&mut self, font: i32);
    /// Advance width of `text` in CSS px.
    fn text_width(&mut self, font: i32, text: &[u8]) -> f32;
    /// Decodes a `data:` image and keeps it under `id` for the painter; `w`/`h` get the intrinsic size in CSS
    /// px. False when undecodable.
    fn load_image(&mut self, id: i32, url: &[u8], w: &mut f32, h: &mut f32) -> bool;
    /// Runs `ops` on the w × h px bitmap of canvas `id`, created on the first call. Optional.
    fn canvas_run(&mut self, _id: i32, _w: i32, _h: i32, _ops: &[CanvasOp]) {}
    /// Straight-alpha RGBA of a region; pixels outside the bitmap are transparent. False when unsupported.
    fn canvas_read(&mut self, _id: i32, _x: i32, _y: i32, _w: i32, _h: i32, _rgba: &mut Vec<u8>) -> bool {
        false
    }
    fn canvas_release(&mut self, _id: i32) {}
}

/// The time a renderer reads (`renderer.h`): finite, non-negative and nondecreasing milliseconds, on the
/// renderer's thread.
pub trait Clock {
    fn now_ms(&mut self) -> f64;
}

/// The embedder's channel for one-way messages.
pub trait Channel {
    fn send(&mut self, message: Vec<u8>);
}

/// The renderer end's channel: one-way messages, and requests that wait for their answer.
pub trait RendererChannel: Channel {
    /// False when no answer came.
    fn request(&mut self, message: Vec<u8>, answer: &mut Vec<u8>) -> bool;
}

/// What [`RendererEnd`] asks of a renderer (renderer.h's `Renderer`): host messages in, renderer messages out,
/// the platform's input, frames and the state the end reports. Every call on the renderer's thread; a call
/// may make Backend calls, which the end turns into messages and requests.
pub trait Renderer {
    /// Host → renderer message (parsed JSON). Returns renderer → host messages.
    fn receive(&mut self, message: &Json) -> Vec<Json>;
    fn set_width(&mut self, width: f64, out: &mut Vec<Json>);
    fn set_max_height(&mut self, height: f64, out: &mut Vec<Json>);
    fn set_theme(&mut self, dark: bool, out: &mut Vec<Json>);
    fn touch(&mut self, action: i32, x: f64, y: f64, pointer: i32, primary: bool, mouse: bool, modifiers: i32, button: i32) -> Vec<Json>;
    fn hover(&mut self, action: i32, x: f64, y: f64, modifiers: i32) -> Vec<Json>;
    /// The CSS cursor where the mouse last was.
    fn cursor(&mut self) -> Vec<u8>;
    /// What a press at (x, y) that starts no selection would claim, without pressing.
    fn claim_at(&mut self, x: f64, y: f64) -> GestureClaim;
    fn clear_selection(&mut self) -> bool;
    /// False when nothing is selected, without laying the text out.
    fn may_have_selection(&mut self) -> bool;
    fn selected_text(&mut self) -> Vec<u8>;
    /// One display frame. Returns renderer → host messages.
    fn tick(&mut self) -> Vec<Json>;
    fn dirty(&mut self) -> bool;
    /// Records the current document into a display list.
    fn paint(&mut self) -> DisplayList;
    /// Where the last `paint()` differs from the one before; false when everything must be painted.
    fn damage(&mut self, out: &mut Vec<Rect>) -> bool;
    fn width(&mut self) -> f64;
    fn viewport_height(&mut self) -> i32;
    fn wants_tick(&mut self) -> bool;
    fn next_tick_in(&mut self) -> f64;
}
