//! TwidgetCore's renderer (`core/src/renderer.cpp`, its `Renderer` and the document half of its `Container`): the
//! host's messages in, the document built from the widget's ops, and the answers and reports out. A batch of ops
//! changes the shadow tree (`twidget_shadow`); the build that follows patches the live document in place from the
//! batch's log where it can (the fork's holes), patches it from the serialised HTML where the fork can take that,
//! and parses a new one otherwise, within the layout budget, which stops the widget for good past it, as the name
//! table's bounds do. Around the builds: the coalesced batch that waits for its frame, the viewport that follows
//! the document's height, animation overrides patched into style attributes, the theme and the safe area with the
//! master CSS made from their variables, `frozen` and the badge, the size and resize reports, and every query kind
//! with its `result`. The input is `twidget_events`', driven through this renderer as the C++ drives it.
//!
//! Stage 4.5 is [`Layers`]: the paint policy's heavy boxes counted in the renderer, the compositor layers and the
//! scripted ones, the paint with its damage between frames (`layers.rs`). The seam is [`Engine`], litehtml's
//! document and what the renderer's helpers read of its render tree and its paint; the backend and the clock are
//! `twidget_ends`'. Every call into them is made where and in the order renderer.cpp makes it.

#![forbid(unsafe_code)]

mod container;
mod engine;
mod layers;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::OnceLock;

pub use container::{Container, Exhausted, LAYOUT_BUDGET_BASE, LAYOUT_BUDGET_PER_ELEMENT, MAX_PATCHED_FONTS};
pub use engine::{BoxStyle, Clip, Engine, HoleChange, ScrollExtent, TextContent, Tile};
pub use layers::{
    CountedMotion, LAYER_MARGIN, Layers, MAX_LAYER_PIXELS, MAX_LAYERS, MAX_OBSCURERS, MAX_SCRIPTED_AREA, MIN_LAYER_SPAN, Promotion,
    SCRIPTED_STREAK_BATCHES, SCRIPTED_STREAK_GAP_S, ScriptedHit, is_layer_property,
};
pub use twidget_compositor::{Composition, Layer, LayerKey, LayerTrack};
pub use twidget_ends::{Backend, Clock};

use twidget_animations::{AnimationEvent, Animator, Declared, StyleOverrides, StyleValues};
use twidget_canvas::Canvas;
use twidget_css as css;
use twidget_events::{EditorSpec, EditorStyle, EventHost, Events, TextRun};
use twidget_gesture::WidgetGestureClaim;
use twidget_json::Json;
use twidget_shadow::{Chrome, Document, DocumentHost, NodeKind, NodeRef, escape_attr, escape_text, node_id};
use twidget_wire::{Affine, CanvasOp, DisplayList, FontMetrics, Rect};

/// widget-sandbox's shared bubble bounds are 50 to 600 CSS px; the host may raise the top (set_max_height).
pub const MIN_HEIGHT: f64 = 50.0;
/// The tallest a host may let a widget grow: past this a document is a runaway, not content.
pub const HEIGHT_CEILING: f64 = 16384.0;
/// The widest a host's view and the densest its screen can be: past these a number is a mistake, not a size.
pub const WIDTH_CEILING: f64 = 1048576.0;
pub const MIN_DPR: f64 = 0.25;
pub const MAX_DPR: f64 = 64.0;
pub const MAX_PASSES: i32 = 4;
/// After user input, ops batches build at once: they likely answer it.
pub const INPUT_IMMEDIATE_S: f64 = 0.25;
/// The sandbox hands a long flush over in parts of this many ops (widget-sandbox v0.6.0, capture.ts
/// PARTIAL_BATCH_OPS); only the last part of a task is shorter.
pub const PARTIAL_BATCH_OPS: usize = 1000;
pub const MAX_NAMES: u64 = 20000;
pub const MAX_NAME_BYTES: u64 = 1 << 20;
/// The render throttle (the Swift renderer's): a pass costlier than a frame holds the next one back for
/// THROTTLE_FACTOR times its cost, 25 % of the thread at most; under sustained load, more than SUSTAINED_LIMIT of
/// SUSTAINED_WINDOW with two passes over HEAVY_PASS_S, 5 %. Nothing is charged in the first LOAD_GRACE_S, while a
/// widget sets itself up.
pub const THROTTLE_FREE_S: f64 = 0.016;
pub const THROTTLE_FACTOR: f64 = 3.0;
pub const SUSTAINED_THROTTLE_FACTOR: f64 = 19.0;
pub const SUSTAINED_WINDOW_S: f64 = 2.0;
pub const SUSTAINED_LIMIT: f64 = 0.1;
pub const HEAVY_PASS_S: f64 = 0.05;
pub const LOAD_GRACE_S: f64 = 1.0;
/// How long a press decides the side to keep, and how close height steps come in one burst (a CSS height
/// transition), whose tiny or unclear steps keep the side the burst started with.
pub const RESIZE_PRESS_AGE: f64 = 1.5;
pub const RESIZE_BURST_GAP: f64 = 0.15;
/// Edges that moved less than this did not move.
pub const RESIZE_TOLERANCE: f64 = 0.5;

/// Chromium/WebKit UA rules litehtml's master.css lacks and widgets rely on (renderer.cpp's `UA_EXTRA`). Form
/// controls get a visible box: litehtml has no controls of its own, so they are bordered inline blocks here, with
/// the value as text where the shadow tree knows it. The profile's UA layer comes after these.
pub const UA_EXTRA: &str = r#"
button,input,select,textarea,meter,progress{box-sizing:border-box}
input,select,textarea{display:inline-block;border:1px solid #8f8f8f;border-radius:3px;background:#fff;color:#000;padding:2px 6px;font:13.333px -apple-system,system-ui,sans-serif;min-height:1.8em;min-width:2em;vertical-align:middle}
:where(button){display:inline-block;text-align:center}
input[type=checkbox],input[type=radio]{min-width:13px;width:13px;height:13px;min-height:13px;padding:0;margin:3px 3px 3px 4px;vertical-align:middle}
input[type=radio]{border-radius:50%}
input[type=range]{border:none;background:transparent;padding:0}
input[type=color]{width:50px;height:27px;padding:2px}
input[type=hidden]{display:none}
textarea{display:block;min-height:3em;white-space:pre-wrap;overflow-wrap:break-word}
progress,meter{display:inline-block;width:10em;height:1em;background:#e6e6e6;border-radius:2px}
canvas{display:inline-block;background:transparent}
svg{display:inline-block}
details{display:block}
details>summary:first-of-type{display:list-item;list-style:disclosure-closed inside;cursor:pointer}
details[open]>summary:first-of-type{list-style-type:disclosure-open}
details:not([open])>*:not(summary){display:none}
dialog:not([open]){display:none}
[dir=rtl]{direction:rtl}
[dir=ltr]{direction:ltr}
q::before{content:open-quote}
q::after{content:close-quote}
[hidden]{display:none}
"#;

const MASTER_CSS_H: &[u8] = include_bytes!("../../../../litehtml/include/litehtml/master_css.h");
const PROFILE_JSON: &[u8] = include_bytes!("../../../../../widget-sandbox/packages/core/profile.json");

/// The text of the C++ raw string after `marker` in `source`, its line ends as the compiler reads them.
fn raw_string(source: &[u8], marker: &[u8]) -> Vec<u8> {
    let Some(at) = container::find(source, marker, 0) else { return Vec::new() };
    let Some(open) = container::find(source, b"R\"", at).map(|o| o + 2) else { return Vec::new() };
    let Some(paren) = container::find(source, b"(", open) else { return Vec::new() };
    let close = [b")", &source[open..paren], b"\""].concat();
    let Some(end) = container::find(source, &close, paren) else { return Vec::new() };
    let mut out = Vec::with_capacity(end - paren);
    for (i, &c) in source[paren + 1..end].iter().enumerate() {
        if !(c == b'\r' && source.get(paren + 2 + i) == Some(&b'\n')) {
            out.push(c);
        }
    }
    out
}

/// litehtml's `master_css` (litehtml/include/litehtml/master_css.h).
pub fn litehtml_master_css() -> &'static [u8] {
    static TEXT: OnceLock<Vec<u8>> = OnceLock::new();
    TEXT.get_or_init(|| raw_string(MASTER_CSS_H, b"master_css"))
}

/// The profile's UA layer (`tables.uaStylesheet` of widget-sandbox's packages/core/profile.json, TW_UA_STYLESHEET),
/// its `var()`s as they are.
pub fn ua_stylesheet() -> &'static [u8] {
    static TEXT: OnceLock<Vec<u8>> = OnceLock::new();
    TEXT.get_or_init(|| Json::parse(PROFILE_JSON).map(|j| j.get("tables").get("uaStylesheet").as_str().to_vec()).unwrap_or_default())
}

/// The master CSS documents are parsed with under `vars`: litehtml's, the renderer's own rules, and the profile's
/// UA layer with the theme's values in place, since the fork applies a var() in a shorthand after the cascade
/// (`margin: var(…)` from the master CSS would beat the widget's own `margin: 0`), and without :where().
pub fn master_css_for(vars: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let layer = css::unwrap_where(&css::substitute_variables(ua_stylesheet(), vars));
    [litehtml_master_css(), UA_EXTRA.as_bytes(), &layer].concat()
}

/// Timings of the last rebuild (serialise → parse+layout, per pass) and what the builds did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuildStats {
    pub serialize_ms: f64,
    pub parse_ms: f64,
    pub passes: i32,
    pub html_bytes: usize,
    pub builds: i32,
    /// Builds that patched the previous document instead of parsing a new one.
    pub patched: i32,
    /// Passes after which the render throttle held the widget back.
    pub throttled: i32,
    /// Of those, builds that went into the document without serialising it (ops patched in place).
    pub in_place: i32,
    /// Batches that only moved scripted layers: no build.
    pub scripted: i32,
    /// Paints that handed out the last list again, only layers having moved.
    pub reused: i32,
    pub names: u64,
    pub name_bytes: u64,
}

/// Where the content went when the document changed height, for a host that keeps part of the widget still on
/// screen while it resizes (the Swift renderer's WidgetResizeDiff). Edges are in CSS px of the document before
/// the change.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ResizeDiff {
    pub old_height: f64,
    pub new_height: f64,
    /// How far down from the top, and up from the bottom, nothing moved.
    pub unchanged_top: f64,
    pub unchanged_bottom: f64,
    /// A point of the element pressed last, before and after, when it was pressed recently.
    pub has_pointer: bool,
    pub pointer_from: f64,
    pub pointer_to: f64,
    /// The side to keep still: the bottom (the widget grows or shrinks upward) or the top.
    pub keeps_bottom: bool,
}

/// An element's vertical edges at a layout.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Edges {
    pub top: f32,
    pub bottom: f32,
}

/// The canvas fonts the renderer made, by the CSS `font` they were made for: the backend's handle and its metrics.
pub type CanvasFonts = HashMap<Vec<u8>, (i32, FontMetrics)>;

/// An embedder's own canvas fonts: given the renderer's Backend and the CSS `font` unparsed, it fills `metrics`, the
/// em box included, and returns a handle, or 0 for none. `Send`, as the clock, so that a renderer can move between the
/// helper's threads.
pub type CanvasFontHook = Box<dyn FnMut(&mut dyn Backend, &[u8], &mut FontMetrics) -> i32 + Send>;

fn bounded(v: f64, lo: f64, hi: f64) -> f64 {
    if v.is_nan() { lo } else { clamp(v, lo, hi) }
}

/// `std::clamp` as the C++ library writes it: a NaN comes through.
fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}

/// `std::min` and `std::max`: the first argument unless the second is strictly beyond it.
fn min_f64(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}
fn max_f64(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}
fn min_f32(a: f32, b: f32) -> f32 {
    if b < a { b } else { a }
}
fn max_f32(a: f32, b: f32) -> f32 {
    if a < b { b } else { a }
}

/// `int(v)` of a double on x86-64: a NaN or a value past `int` is INT_MIN.
fn int_of(v: f64) -> i32 {
    if v.is_nan() || v >= 2147483648.0 || v <= -2147483649.0 { i32::MIN } else { v as i32 }
}

/// `int(std::ceil(std::clamp(height, MIN_HEIGHT, max_height)))`: the viewport a document height asks for.
fn viewport_for(height: f64, max_height: f64) -> i32 {
    int_of(clamp(height, MIN_HEIGHT, max_height).ceil())
}

fn rect_json(r: &Rect) -> Json {
    let mut o = Json::object();
    o.set("x", f64::from(r.x)).set("y", f64::from(r.y)).set("width", f64::from(r.w)).set("height", f64::from(r.h));
    o.set("top", f64::from(r.y)).set("left", f64::from(r.x)).set("right", f64::from(r.x + r.w)).set("bottom", f64::from(r.y + r.h));
    o
}

fn error(text: &str) -> Json {
    let mut o = Json::object();
    o.set("error", text);
    o
}

/// The steady clock in ms, from the first time the process asks.
fn steady_ms() -> f64 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64() * 1000.0
}

/// Whether the side of a widget changing height that stays still is the bottom.
fn keeps_bottom(d: &ResizeDiff, press: Option<f64>, prefers_bottom: bool, previous: Option<bool>) -> bool {
    let old_h = d.old_height;
    let delta = d.new_height - d.old_height;
    if let Some(p) = previous
        && (delta.abs() <= 1.5 || (d.unchanged_top >= old_h && d.unchanged_bottom >= old_h))
    {
        return p;
    }
    if d.has_pointer {
        // Whichever side the pressed element stays closer to.
        let shift = d.pointer_to - d.pointer_from;
        if let Some(p) = previous
            && shift.abs() < 1.0
            && (shift - delta).abs() < 1.0
        {
            return p;
        }
        return (shift - delta).abs() < shift.abs();
    }
    if let Some(press) = press {
        let y = clamp(press, 0.0, old_h);
        if y <= d.unchanged_top {
            return false;
        }
        if y >= old_h - d.unchanged_bottom {
            return true;
        }
        return y >= old_h / 2.0;
    }
    prefers_bottom || d.unchanged_bottom > d.unchanged_top
}

/// What the tree's ops ask of the renderer when nothing but the tree's own state is touched: a stop, a freeze.
struct NoHost;

impl DocumentHost for NoHost {
    fn canvas_run(&mut self, _canvas: i32, _width: i32, _height: i32, _ops: &[CanvasOp]) {}
    fn canvas_release(&mut self, _canvas: i32) {}
    fn canvas_font(&mut self, _css: &[u8], _metrics: &mut FontMetrics) -> i32 {
        0
    }
    fn canvas_text_width(&mut self, _font: i32, _text: &[u8]) -> f32 {
        0.0
    }
    fn image_of(&mut self, _src: &[u8]) -> Option<(i32, f32, f32)> {
        None
    }
}

/// The renderer's state before a call that may build: what to follow after it.
#[derive(Clone, Copy)]
struct Before {
    frozen: bool,
    paint_stopped: bool,
}

impl Before {
    fn of(doc: &Document) -> Before {
        Before { frozen: doc.frozen(), paint_stopped: doc.paint_stopped }
    }
}

fn still(doc: &Document) -> bool {
    doc.frozen() || doc.paint_stopped || doc.layout_stopped()
}

/// Everything of the renderer but its tree and its input: the document's life, the meters, the animator, the
/// reports, the canvas fonts, the answers. It is the input's [`EventHost`] and, with the input, the tree's
/// [`DocumentHost`].
pub struct Core<E: Engine, B: Backend> {
    pub engine: E,
    pub backend: B,
    /// Stage 4.5's state: heavy boxes, layers, the last paint.
    pub layers: Layers,
    clock: Option<Box<dyn Clock + Send>>,
    container: Container,
    width: f64,
    dpr: f64,
    vp_height: i32,
    doc_height: f64,
    max_height: f64,
    dirty: bool,
    /// What changed since the last paint, for layers to keep their content: the elements patched, or everything.
    touched: BTreeSet<i64>,
    touched_all: bool,
    patching: bool,
    html: Vec<u8>,
    html_stale: bool,
    stats: BuildStats,
    /// The control the platform's editor shows, as the input last said.
    editing: i64,
    /// The event types the sandbox listens for, as the input keeps them.
    interest: HashSet<Vec<u8>>,
    meter_depth: i32,
    name_meter_depth: i32,
    /// The meters of each rebuild the input asked for, until it says the rebuild ends.
    rebuilds: Vec<(f64, (u64, u64))>,
    charges: VecDeque<(f64, f64)>,
    first_charge: f64,
    throttled_until: f64,
    last_size: [f64; 4],
    track_resize: bool,
    prefers_bottom: bool,
    edges: BTreeMap<i64, Edges>,
    edges_height: f64,
    edges_layouts: u64,
    has_resize: bool,
    resize: ResizeDiff,
    resize_at: f64,
    resize_serial: u64,
    resize_reported: u64,
    press_id: i64,
    press_y: f64,
    press_at: f64,
    pending_events: Vec<Json>,
    animator: Animator,
    overrides: StyleOverrides,
    sheets_key: Vec<u8>,
    animating: bool,
    declared_generation: u64,
    /// DOM changes only the layers show; the document takes them when it builds.
    scripted_pending: bool,
    canvas_fonts: CanvasFonts,
    /// canvas_fonts' handles in the order they were made, the order they are released in.
    canvas_font_order: Vec<i32>,
    canvas_font_hook: Option<CanvasFontHook>,
    /// The binary payload of the query being answered (imageData), base64.
    binary: String,
}

impl<E: Engine, B: Backend> std::fmt::Debug for Core<E, B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core")
            .field("width", &self.width)
            .field("vp_height", &self.vp_height)
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

/// The tree's calls into the rest of the renderer while it applies ops.
struct Bridge<'a, E: Engine, B: Backend> {
    core: &'a mut Core<E, B>,
    events: &'a mut Events,
}

impl<E: Engine, B: Backend> DocumentHost for Bridge<'_, E, B> {
    fn canvas_run(&mut self, canvas: i32, width: i32, height: i32, ops: &[CanvasOp]) {
        self.core.backend.canvas_run(canvas, width, height, ops);
    }
    fn canvas_release(&mut self, canvas: i32) {
        self.core.layers.canvas_released(canvas);
        self.core.backend.canvas_release(canvas);
    }
    fn canvas_run_begins(&mut self) {
        // Fonts are held across one batch at most, so a widget cycling through font strings cannot grow this.
        if self.core.canvas_fonts.len() > 64 {
            self.core.release_canvas_fonts();
        }
    }
    fn canvas_drew(&mut self, canvas: i32) {
        self.core.canvas_drew(canvas);
    }
    fn canvas_font(&mut self, css: &[u8], metrics: &mut FontMetrics) -> i32 {
        self.core.canvas_font(css, metrics)
    }
    fn canvas_text_width(&mut self, font: i32, text: &[u8]) -> f32 {
        self.core.backend.text_width(font, text)
    }
    fn image_of(&mut self, src: &[u8]) -> Option<(i32, f32, f32)> {
        self.core.engine.image_of(src)
    }
    fn node_forgotten(&mut self, id: i64) {
        self.events.node_forgotten(id);
    }
    fn value_set(&mut self, id: i64, value: &[u8]) {
        self.events.value_set(id, value);
    }
    fn call(&mut self, document: &mut Document, id: i64, method: &[u8], args: &Json, out: &mut Vec<Json>) {
        self.events.call(document, self.core, id, method, args, out);
    }
    fn frozen(&mut self) {
        self.events.on_frozen();
    }
    fn take_violations(&mut self) -> Vec<Vec<u8>> {
        self.core.engine.take_violations()
    }
}

impl<E: Engine, B: Backend> Core<E, B> {
    fn now_ms(&mut self) -> f64 {
        match &mut self.clock {
            Some(c) => c.now_ms(),
            None => steady_ms(),
        }
    }

    fn now_s(&mut self) -> f64 {
        self.now_ms() / 1000.0
    }

    /// The engine's violations into the document's list, where the C++ container pushed them.
    fn pushed(&mut self, doc: &mut Document) {
        doc.pending_violations.extend(self.engine.take_violations());
    }

    fn chrome<'a>(overrides: &'a StyleOverrides, editing: i64) -> Chrome<'a> {
        Chrome { overrides, editing }
    }

    fn serialize(&self, doc: &Document) -> Vec<u8> {
        doc.serialize(&Core::<E, B>::chrome(&self.overrides, self.editing))
    }

    /// The master CSS for the document's root variables: a change drops the document.
    fn set_master(&mut self, doc: &Document) {
        let master = master_css_for(doc.theme_variables());
        self.container.set_master(&mut self.engine, master);
    }

    // ── the meters ──

    /// Charges the outermost pass it spans, whatever nests inside.
    fn meter_begin(&mut self) -> f64 {
        let depth = self.meter_depth;
        self.meter_depth += 1;
        if depth != 0 { 0.0 } else { self.now_s() }
    }

    fn meter_end(&mut self, start: f64) {
        self.meter_depth -= 1;
        if self.meter_depth == 0 {
            let spent = self.now_s() - start;
            self.charge(spent);
        }
    }

    /// Charges the names the outermost call it spans left in litehtml's name table, which never gives one back, counted
    /// on its own thread: a renderer that runs on another thread meanwhile pays for the names it adds itself.
    fn names_begin(&mut self) -> (u64, u64) {
        let depth = self.name_meter_depth;
        self.name_meter_depth += 1;
        if depth == 0 { self.engine.name_usage() } else { (0, 0) }
    }

    fn names_end(&mut self, doc: &mut Document, start: (u64, u64)) {
        self.name_meter_depth -= 1;
        if self.name_meter_depth != 0 {
            return;
        }
        let usage = self.engine.name_usage();
        self.stats.names = self.stats.names.wrapping_add(usage.0.wrapping_sub(start.0));
        self.stats.name_bytes = self.stats.name_bytes.wrapping_add(usage.1.wrapping_sub(start.1));
        if self.stats.names > MAX_NAMES || self.stats.name_bytes > MAX_NAME_BYTES {
            Core::<E, B>::stop_names(doc);
        }
    }

    /// Seconds of work done for this widget, toward the render throttle.
    pub fn charge(&mut self, spent: f64) {
        let now = self.now_s();
        if self.first_charge < 0.0 {
            self.first_charge = now;
        }
        if spent <= 0.0 || now - self.first_charge < LOAD_GRACE_S {
            return;
        }
        self.charges.push_back((now, spent));
        while let Some(&(at, _)) = self.charges.front() {
            if now - at > SUSTAINED_WINDOW_S {
                self.charges.pop_front();
            } else {
                break;
            }
        }
        if spent <= THROTTLE_FREE_S {
            return;
        }
        let mut total = 0.0;
        let mut heavy = 0;
        for &(_, cost) in &self.charges {
            total += cost;
            heavy += i32::from(cost > HEAVY_PASS_S);
        }
        let factor = if total > SUSTAINED_LIMIT * SUSTAINED_WINDOW_S && heavy >= 2 { SUSTAINED_THROTTLE_FACTOR } else { THROTTLE_FACTOR };
        self.throttled_until = max_f64(self.throttled_until, now) + spent * factor;
        self.stats.throttled += 1;
    }

    fn throttled(&mut self, last_input: f64) -> bool {
        self.now_s() < self.throttled_until && self.now_s() - last_input > INPUT_IMMEDIATE_S
    }

    // ── stops ──

    /// Past the layout budget, whatever the host's policy: a violation for the host's log, and the renderer
    /// freezes (fatal) with the badge alone, the widget's document laid out no more. A build in progress rebuilds
    /// as the badge itself; a patch or a restyle falls back to one.
    fn stop_layout(doc: &mut Document, boxes: u64) {
        let violation = format!("layout limit: more than {} boxes in one layout", boxes.wrapping_sub(1));
        doc.stop(violation.as_bytes(), b"Layout limit exceeded", &mut NoHost);
    }

    fn stop_names(doc: &mut Document) {
        let violation = format!("name limit: more than {MAX_NAMES} names or {MAX_NAME_BYTES} bytes of names");
        doc.stop(violation.as_bytes(), b"Name limit exceeded", &mut NoHost);
    }

    // ── builds ──

    /// A rebuild up to the editor's refresh, which the caller makes (the C++'s `Refresh` destructor), and
    /// [`Core::rebuild_ends`] after it.
    fn rebuild_body(&mut self, doc: &mut Document, patch: bool) {
        let meter = self.meter_begin();
        let names = self.names_begin();
        self.rebuilds.push((meter, names));
        doc.stale = false;
        // Scripted layers follow what the build declares, which the composition key does not cover.
        if !self.layers.scripted_candidates.is_empty() {
            self.layers.composition_key = 0;
        }
        self.build_document(doc, patch);
        self.layers.paint_scan = true;
        self.animate(doc);
    }

    fn rebuild_ends(&mut self, doc: &mut Document) {
        let Some((meter, names)) = self.rebuilds.pop() else { return };
        self.names_end(doc, names);
        self.meter_end(meter);
    }

    /// The canvases the tree holds, for the container's `tw-canvas:` sources (the C++ container reads `r_.canvases_`).
    fn hold_canvases(&mut self, doc: &Document) {
        let held: Vec<(i32, i32, i32)> = doc.canvases().values().map(|c| (c.id(), c.width(), c.height())).collect();
        self.engine.canvases(&held);
    }

    fn build_document(&mut self, doc: &mut Document, patch: bool) {
        self.hold_canvases(doc);
        self.scripted_pending = false;
        let log = doc.take_patch_log();
        if patch && self.patching && !doc.layout_stopped() {
            let patched = self.patch_document(doc, &log);
            if patched && !doc.layout_stopped() {
                return;
            }
        }
        let t0 = self.now_ms();
        self.html = self.serialize(doc);
        self.html_stale = false;
        let t1 = self.now_ms();
        self.stats.serialize_ms = t1 - t0;
        self.stats.html_bytes = self.html.len();
        self.stats.parse_ms = 0.0;
        self.stats.passes = 0;
        // Like the web iframe: the viewport follows the document height (100vh must not explode), converging in a few passes.
        let mut changed = false;
        for pass in 0..MAX_PASSES {
            let a = self.now_ms();
            let built = {
                let Core { container, engine, html, width, vp_height, .. } = self;
                container.build(engine, html, *vp_height, *width, &mut |e| Core::<E, B>::stop_layout(doc, e.0))
            };
            self.pushed(doc);
            changed = built || changed;
            if doc.layout_stopped() {
                break;
            }
            let b = self.now_ms();
            // litehtml lays out inside createFromString+render; not separable
            self.stats.parse_ms += b - a;
            self.stats.passes += 1;
            if pass == 0 && self.container.patched() {
                self.stats.patched += 1;
            }
            let h = viewport_for(self.container.height(&mut self.engine), self.max_height);
            if h == self.vp_height {
                break;
            }
            let again = self.container.depends_on_viewport_height(&mut self.engine, &self.html);
            self.vp_height = h;
            if !again {
                // Nothing lays out against the viewport's height: the new one only needs recording.
                self.container.viewport_resized(&mut self.engine, self.vp_height);
                break;
            }
        }
        if self.container.exhausted(&mut self.engine) {
            // Past the layout budget (stop_layout): the document as far as it got is no picture of the widget.
            // Built once more, from scratch, as the badge alone (serialize).
            self.container.drop_document(&mut self.engine);
            self.html = self.serialize(doc);
            {
                let Core { container, engine, html, width, vp_height, .. } = self;
                container.build(engine, html, *vp_height, *width, &mut |e| Core::<E, B>::stop_layout(doc, e.0));
            }
            self.pushed(doc);
            self.vp_height = viewport_for(self.container.height(&mut self.engine), self.max_height);
            self.container.viewport_resized(&mut self.engine, self.vp_height);
            changed = true;
        }
        self.doc_height = self.container.height(&mut self.engine).ceil();
        self.stats.builds += 1;
        if changed {
            self.dirty = true;
            self.touched_all = true;
        }
    }

    /// The batch's log into the document in place: which attributes and sole texts the ops changed, through the
    /// fork's `update_holes`, so the document is not serialised again. False, leaving it as it was, when the log
    /// cannot say it all.
    fn patch_document(&mut self, doc: &mut Document, log: &twidget_shadow::PatchLog) -> bool {
        if !log.valid || (log.attributes.is_empty() && log.texts.is_empty()) || !self.container.can_patch(&mut self.engine, self.width) {
            return false;
        }
        let t0 = self.now_ms();
        let mut changes = Vec::new();
        for (r, names) in log.attributes.values() {
            let n = doc.get(*r);
            if !self.engine.has_element(n.id) {
                return false;
            }
            for name in names {
                let mut change = HoleChange { owner: n.id, name: name.clone(), value: Vec::new(), text: false };
                // Byte for byte what serialize_node writes there.
                if name == b"style" {
                    if n.styles.is_empty() && !self.overrides.contains_key(&n.id) && n.id != self.editing {
                        return false;
                    }
                    doc.write_style(*r, &Core::<E, B>::chrome(&self.overrides, self.editing), &mut change.value);
                } else if name == b"src" && n.tag == b"canvas" && !n.svg {
                    let Some(c) = doc.canvas(n.id) else { return false };
                    change.value = format!("tw-canvas:{}/{}x{}", c.id(), c.width(), c.height()).into_bytes();
                } else {
                    let Some((_, value)) = n.attrs.iter().find(|(k, _)| k == name) else { return false };
                    if !css::is_attribute_name(name) {
                        return false;
                    }
                    escape_attr(value, &mut change.value);
                }
                changes.push(change);
            }
        }
        for r in log.texts.values() {
            let n = doc.get(*r);
            if !self.engine.has_element(n.id) || n.tag == b"style" || n.tag == b"script" {
                return false;
            }
            let mut change = HoleChange { owner: n.id, name: Vec::new(), value: Vec::new(), text: true };
            if n.tag == b"textarea" {
                if !n.children.is_empty() || n.data.is_empty() {
                    return false;
                }
                if n.data[0] == b'\n' {
                    change.value.push(b'\n');
                }
                escape_text(&n.data, &mut change.value);
            } else {
                // A hole holds an element's text when the text is all it holds.
                if n.children.len() != 1 {
                    return false;
                }
                let child = doc.get(n.children[0]);
                if child.kind != NodeKind::Text || child.data.is_empty() {
                    return false;
                }
                escape_text(&child.data, &mut change.value);
            }
            changes.push(change);
        }
        let patched = {
            let Core { container, engine, .. } = self;
            container.patch(engine, &changes, &mut |e| Core::<E, B>::stop_layout(doc, e.0))
        };
        self.pushed(doc);
        if !patched {
            return false;
        }
        let h = viewport_for(self.container.height(&mut self.engine), self.max_height);
        // A new height moves the viewport: when the layout depends on it, the full path settles that, with the
        // patch already in. Style sheets only change through that path, so html's are current.
        if h != self.vp_height {
            if self.container.depends_on_viewport_height(&mut self.engine, &self.html) {
                return false;
            }
            self.vp_height = h;
            self.container.viewport_resized(&mut self.engine, h);
        }
        self.doc_height = self.container.height(&mut self.engine).ceil();
        self.stats.parse_ms = self.now_ms() - t0;
        self.stats.serialize_ms = 0.0;
        self.stats.passes = 1;
        for (r, _) in log.attributes.values() {
            self.touched.insert(doc.get(*r).id);
        }
        for r in log.texts.values() {
            self.touched.insert(doc.get(*r).id);
        }
        self.stats.builds += 1;
        self.stats.patched += 1;
        self.stats.in_place += 1;
        self.dirty = true;
        self.html_stale = true;
        true
    }

    // ── animations ──

    fn listened(&self, kind: &[u8]) -> bool {
        if !twidget_events::PROFILE_EVENTS.contains(&kind) {
            return false;
        }
        if twidget_events::ALWAYS_FORWARD.contains(&kind) {
            return true;
        }
        self.interest.contains(kind)
    }

    fn animate(&mut self, doc: &mut Document) {
        if self.animating || still(doc) {
            return;
        }
        self.animating = true;
        let now = self.now_s();
        // The sheets' raw text decides: they are resolved for the theme only when it changed.
        let dark = doc.dark();
        let mut styles: Vec<NodeRef> = Vec::new();
        let mut key = vec![if dark { b'd' } else { b'l' }];
        fn visit(doc: &Document, r: NodeRef, styles: &mut Vec<NodeRef>, key: &mut Vec<u8>) {
            let n = doc.get(r);
            if n.kind != NodeKind::Element {
                return;
            }
            if n.tag == b"style" {
                styles.push(r);
                key.extend_from_slice(&n.data);
                key.push(1);
                return;
            }
            for &c in &n.children {
                visit(doc, c, styles, key);
            }
        }
        visit(doc, doc.html(), &mut styles, &mut key);
        if key != self.sheets_key {
            self.sheets_key = key;
            let sheets: Vec<Vec<u8>> = styles
                .iter()
                .map(|&r| css::resolve_light_dark(&twidget_shadow::rewrite_color_scheme(&doc.get(r).data, dark), dark, 0))
                .collect();
            self.animator.set_style_sheets(&sheets);
        }
        self.animator.prune(|id| doc.node(id).is_some());
        let watched: Vec<Vec<u8>> = Animator::watched().iter().map(|s| s.as_bytes().to_vec()).collect();
        let gates: Vec<Vec<u8>> = Animator::entry_properties().iter().map(|s| s.as_bytes().to_vec()).collect();
        let (found, generation) = self.engine.declarations(&watched, &gates, &[]);
        let targets = self.animator.web_targets();
        if targets.is_empty() {
            // The same declarations as last time start, stop and retarget nothing.
            if generation == 0 || generation != self.declared_generation {
                self.animator.underlying.clear();
                self.animator.update(&found, now);
            }
            self.declared_generation = generation;
        } else {
            self.declared_generation = 0;
            let mut declarations = found;
            self.animator.underlying.clear();
            // Web Animations targets are read whatever they declare: their keyframes compose onto these values.
            let mut names = watched;
            for &id in &targets {
                for p in self.animator.web_properties(id) {
                    if !names.contains(&p) {
                        names.push(p);
                    }
                }
            }
            let (values_by_id, _) = self.engine.declarations(&names, &[], &targets);
            for (id, values) in values_by_id {
                let declared: &mut Declared = declarations.entry(id).or_default();
                for (k, v) in &values {
                    if !k.starts_with(b"animation") && !k.starts_with(b"transition") {
                        declared.entry(k.clone()).or_insert_with(|| v.clone());
                    }
                }
                self.animator.underlying.insert(id, values);
            }
            self.animator.update(&declarations, now);
        }
        self.update_composition(doc, now);
        let mut sampled = self.animator.sample(now);
        self.strip_layers(&mut sampled);
        self.queue_animation_events(doc);
        // Changed overrides patch the style attributes of the document just built; only a change the patch cannot
        // take builds again.
        self.apply_overrides(doc, sampled);
        self.animating = false;
        self.count_paint(doc);
    }

    fn apply_overrides(&mut self, doc: &mut Document, sampled: StyleOverrides) {
        if sampled == self.overrides {
            return;
        }
        let mut changed = Vec::new();
        for (id, values) in &sampled {
            if self.overrides.get(id) != Some(values) {
                changed.push(*id);
            }
        }
        for id in self.overrides.keys() {
            if !sampled.contains_key(id) {
                changed.push(*id);
            }
        }
        self.overrides = sampled;
        // An animation frame changes style attributes only: patch them in place instead of serialising the whole
        // tree. Falls back to a build when an element has no style attribute yet, or the document height moved.
        let mut patches: Vec<(i64, Vec<u8>)> = Vec::new();
        let mut ok = true;
        for &id in &changed {
            let n = doc.node(id);
            let el = self.engine.has_element(id);
            let Some(n) = n.filter(|_| el) else {
                ok = false;
                break;
            };
            let mut style = Vec::new();
            doc.write_style(n, &Core::<E, B>::chrome(&self.overrides, self.editing), &mut style);
            patches.push((id, style));
        }
        let t0 = self.now_ms();
        if ok {
            self.hold_canvases(doc);
            let restyled = {
                let Core { container, engine, width, .. } = self;
                container.restyle(engine, &patches, *width, &mut |e| Core::<E, B>::stop_layout(doc, e.0))
            };
            self.pushed(doc);
            if restyled && !doc.layout_stopped() {
                let h = viewport_for(self.container.height(&mut self.engine), self.max_height);
                if h == self.vp_height {
                    self.doc_height = self.container.height(&mut self.engine).ceil();
                    self.stats.parse_ms = self.now_ms() - t0;
                    self.stats.serialize_ms = 0.0;
                    self.stats.passes = 1;
                    self.stats.builds += 1;
                    self.stats.patched += 1;
                    self.dirty = true;
                    self.html_stale = true;
                    self.touched.extend(changed);
                    return;
                }
            }
        }
        self.build_document(doc, false);
    }

    fn queue_animation_events(&mut self, doc: &Document) {
        for e in self.animator.take_events() {
            let AnimationEvent { kind, target, name, elapsed } = e;
            let kind = kind.name().as_bytes();
            if doc.frozen() || !self.listened(kind) {
                continue;
            }
            let mut ev = Json::object();
            ev.set("type", kind).set("targetId", target as f64);
            ev.set(if kind == b"transitionend" { "propertyName" } else { "animationName" }, name);
            ev.set("elapsedTime", elapsed);
            self.pending_events.push(Events::event_message(ev, true));
        }
    }

    // ── reports ──

    fn report_size(&mut self, out: Option<&mut Vec<Json>>) {
        if self.track_resize {
            self.note_resize();
        }
        // Every frame asks; the message goes out when a number moved.
        let size = [self.width, self.doc_height, f64::from(self.vp_height), self.dpr];
        if size == self.last_size {
            return;
        }
        self.last_size = size;
        let mut msg = Json::object();
        msg.set("type", "size").set("width", self.width).set("height", self.doc_height);
        msg.set("innerWidth", self.width).set("innerHeight", self.vp_height).set("dpr", self.dpr);
        if self.has_resize && self.resize_serial != self.resize_reported {
            self.resize_reported = self.resize_serial;
            let d = &self.resize;
            let mut r = Json::object();
            r.set("oldHeight", d.old_height).set("newHeight", d.new_height);
            r.set("unchangedTop", d.unchanged_top).set("unchangedBottom", d.unchanged_bottom);
            if d.has_pointer {
                r.set("pointerFrom", d.pointer_from).set("pointerTo", d.pointer_to);
            }
            r.set("keepsBottom", d.keeps_bottom);
            msg.set("resize", r);
        }
        match out {
            Some(out) => out.push(msg),
            None => self.pending_events.push(msg),
        }
    }

    /// After a layout, with tracking on: the element edges, and the diff when the height changed.
    fn note_resize(&mut self) {
        let layouts = self.container.layouts();
        if layouts == self.edges_layouts {
            return;
        }
        let mut now: BTreeMap<i64, Edges> = BTreeMap::new();
        for (id, r) in self.engine.edges() {
            if r.w > 0.0 || r.h > 0.0 {
                now.entry(id).or_insert(Edges { top: r.y, bottom: r.y + r.h });
            }
        }
        let old_h = self.edges_height;
        let new_h = self.container.height(&mut self.engine);
        if self.edges_layouts != 0 && (old_h.ceil() - new_h.ceil()).abs() > RESIZE_TOLERANCE {
            // Edges that moved, appeared or went bound the parts that did not, from the top and from the bottom.
            let mut d = ResizeDiff { old_height: old_h, new_height: new_h, ..ResizeDiff::default() };
            let mut top = old_h;
            let mut bottom = old_h;
            let unstable_top = |top: &mut f64, y: f64| *top = min_f64(*top, max_f64(y, 0.0));
            let unstable_bottom = |bottom: &mut f64, distance: f64| *bottom = min_f64(*bottom, max_f64(distance, 0.0));
            for (id, before) in &self.edges {
                let Some(after) = now.get(id) else {
                    unstable_top(&mut top, f64::from(before.top));
                    unstable_bottom(&mut bottom, old_h - f64::from(before.bottom));
                    continue;
                };
                for (a, b) in [(f64::from(before.top), f64::from(after.top)), (f64::from(before.bottom), f64::from(after.bottom))] {
                    if (a - b).abs() > RESIZE_TOLERANCE {
                        unstable_top(&mut top, min_f64(a, b));
                    }
                    if ((old_h - a) - (new_h - b)).abs() > RESIZE_TOLERANCE {
                        unstable_bottom(&mut bottom, min_f64(old_h - a, new_h - b));
                    }
                }
            }
            for (id, after) in &now {
                if self.edges.contains_key(id) {
                    continue;
                }
                unstable_top(&mut top, f64::from(after.top));
                unstable_bottom(&mut bottom, new_h - f64::from(after.bottom));
            }
            d.unchanged_top = min_f64(top, old_h);
            d.unchanged_bottom = min_f64(bottom, old_h);

            let t = self.now_s();
            let recent = t - self.press_at < RESIZE_PRESS_AGE;
            if recent && let (Some(before), Some(after)) = (self.edges.get(&self.press_id), now.get(&self.press_id)) {
                let reach = f64::from(min_f32(before.bottom - before.top, after.bottom - after.top));
                let inset = clamp(self.press_y - f64::from(before.top), 0.0, reach);
                d.has_pointer = true;
                d.pointer_from = f64::from(before.top) + inset;
                d.pointer_to = f64::from(after.top) + inset;
            }
            let previous = (self.has_resize && t - self.resize_at < RESIZE_BURST_GAP).then_some(self.resize.keeps_bottom);
            d.keeps_bottom = keeps_bottom(&d, recent.then_some(self.press_y), self.prefers_bottom, previous);
            // The press stays on the content it was on.
            if d.keeps_bottom {
                self.press_y += new_h - old_h;
            }
            self.resize = d;
            self.resize_at = t;
            self.has_resize = true;
            self.resize_serial = self.resize_serial.wrapping_add(1);
        }
        self.edges = now;
        self.edges_height = new_h;
        self.edges_layouts = layouts;
    }

    // ── canvas fonts ──

    fn release_canvas_fonts(&mut self) {
        for &handle in &self.canvas_font_order {
            self.backend.delete_font(handle);
        }
        self.canvas_font_order.clear();
        self.canvas_fonts.clear();
    }

    fn canvas_font(&mut self, font: &[u8], metrics: &mut FontMetrics) -> i32 {
        if let Some(hook) = &mut self.canvas_font_hook {
            return hook(&mut self.backend, font, metrics);
        }
        if let Some((handle, m)) = self.canvas_fonts.get(font) {
            *metrics = *m;
            return *handle;
        }
        let f = css::parse_font(font);
        let handle = self.backend.create_canvas_font(&f.family, f.size, f.weight, f.italic, metrics);
        if handle != 0 && metrics.em_ascent == 0.0 && metrics.em_descent == 0.0 {
            let (ascent, descent) = (metrics.ascent, metrics.descent);
            twidget_wire::set_em_box(metrics, ascent, descent, f.size);
        }
        if handle != 0 {
            self.canvas_fonts.entry(font.to_vec()).or_insert((handle, *metrics));
            self.canvas_font_order.push(handle);
        }
        handle
    }

    /// `measureText`: the text in the canvas font `font` names, the one fillText draws it with.
    fn measure_text(&mut self, font: &[u8], text: &[u8]) -> Json {
        // In the font fillText draws it with: the canvas's, not a document's.
        if self.canvas_fonts.len() > 64 {
            self.release_canvas_fonts();
        }
        let mut fm = FontMetrics::default();
        let handle = self.canvas_font(font, &mut fm);
        let width = if handle != 0 { self.backend.text_width(handle, text) } else { 0.0 };
        let mut o = Json::object();
        o.set("width", f64::from(width));
        o.set("actualBoundingBoxLeft", 0).set("actualBoundingBoxRight", f64::from(width));
        o.set("actualBoundingBoxAscent", f64::from(fm.ascent * 0.8)).set("actualBoundingBoxDescent", f64::from(fm.descent * 0.6));
        o.set("fontBoundingBoxAscent", f64::from(fm.ascent)).set("fontBoundingBoxDescent", f64::from(fm.descent));
        o.set("emHeightAscent", f64::from(fm.em_ascent)).set("emHeightDescent", f64::from(fm.em_descent));
        o.set("hangingBaseline", f64::from(fm.ascent * 0.8))
            .set("alphabeticBaseline", 0)
            .set("ideographicBaseline", f64::from(-fm.descent));
        o
    }

    // ── queries ──

    fn match_media(&self, dark: bool, query: &[u8]) -> bool {
        let q = css::lower(query);
        let has = |key: &[u8]| container::find(&q, key, 0).is_some();
        if has(b"prefers-color-scheme") {
            return has(b"dark") == dark;
        }
        let px = |key: &[u8]| -> f64 {
            let Some(i) = container::find(&q, key, 0) else { return f64::NAN };
            let mut j = i + key.len();
            while j < q.len() && (q[j] == b' ' || q[j] == b':') {
                j += 1;
            }
            // The C string from there: up to a NUL.
            let rest = &q[j..];
            let rest = rest.iter().position(|&c| c == 0).map_or(rest, |n| &rest[..n]);
            twidget_cstd::atof(rest)
        };
        let v = px(b"max-width");
        if !v.is_nan() {
            return self.width <= v;
        }
        let v = px(b"min-width");
        if !v.is_nan() {
            return self.width >= v;
        }
        if has(b"hover") {
            return has(b"none");
        }
        if has(b"pointer") {
            return has(b"coarse");
        }
        false
    }

    fn answer(&mut self, doc: &Document, q: &Json) -> Json {
        let kind = q.get("q").as_str();
        let mut el = false;
        let mut shadow: Option<NodeRef> = None;
        let mut id = 0;
        if let Some(v) = q.find("id") {
            id = node_id(v);
            shadow = doc.node(id);
            el = self.engine.has_element(id);
            if shadow.is_none_or(|r| doc.get(r).kind != NodeKind::Element) {
                return error("no such element");
            }
        }
        // getBoundingClientRect is where the element shows, transforms included; the offset and client sizes
        // below are layout sizes, which transforms leave alone.
        if kind == b"rect" {
            let r = if el { self.engine.visual_box(id) } else { Rect::default() };
            return rect_json(&r);
        }
        if kind == b"rects" {
            if !el {
                return Json::array();
            }
            // A wrapped inline element has a rect per line.
            let mut rects = Json::array();
            for line in self.engine.inline_boxes(id) {
                let shown = self.engine.transformed_box(id, line);
                rects.push(rect_json(&shown));
            }
            if rects.arr().is_empty() {
                let mut one = Json::array();
                one.push(rect_json(&self.engine.visual_box(id)));
                return one;
            }
            return rects;
        }
        if kind == b"box" {
            let r = if el { self.engine.border_box(id) } else { Rect::default() };
            let borders = if el { self.engine.borders(id) } else { None };
            let [bl, bt, br, bb] = borders.unwrap_or([0.0; 4]);
            let mut o = Json::object();
            let (mut left, mut top, mut parent) = (f64::from(r.x), f64::from(r.y), 0i64);
            if borders.is_some() {
                (left, top, parent) = self.engine.offsets(id);
            }
            o.set("offsetWidth", f64::from(r.w))
                .set("offsetHeight", f64::from(r.h))
                .set("offsetTop", top)
                .set("offsetLeft", left)
                .set("offsetParent", parent as f64);
            let cw = max_f32(0.0, r.w - bl - br);
            let ch = max_f32(0.0, r.h - bt - bb);
            let (mut sw, mut sh) = (cw, ch);
            if borders.is_some() {
                // Only a scroll container counts its children's end margins and its own end padding.
                let e = self.engine.scroll_extent(id);
                if e.right > f32::NEG_INFINITY {
                    sw = max_f32(sw, e.right + (if e.scroller { e.pad_right } else { 0.0 }) - (r.x + bl));
                }
                if e.bottom > f32::NEG_INFINITY {
                    sh = max_f32(sh, e.bottom + (if e.scroller { e.pad_bottom } else { 0.0 }) - (r.y + bt));
                }
            }
            // A textarea's text can outgrow its rows: scrollHeight is the text's, clientHeight the box's.
            if borders.is_some()
                && shadow.is_some_and(|s| doc.get(s).tag == b"textarea")
                && let Some(t) = self.engine.textarea_content(id)
            {
                sw = max_f32(cw, t.width + t.pad[0] + t.pad[2]);
                sh = max_f32(ch, t.height + t.pad[1] + t.pad[3]);
            }
            o.set("clientWidth", f64::from(cw))
                .set("clientHeight", f64::from(ch))
                .set("clientTop", f64::from(bt))
                .set("clientLeft", f64::from(bl));
            o.set("scrollWidth", f64::from(sw)).set("scrollHeight", f64::from(sh)).set("scrollTop", 0).set("scrollLeft", 0);
            return o;
        }
        if kind == b"cs" {
            if !el {
                return if q.find("prop").is_some() { Json::from("") } else { Json::object() };
            }
            // `prop` is only ever a custom property: the shim reads the rest from the whole answer.
            if let Some(prop) = q.find("prop") {
                return Json::from(self.engine.custom_property(id, prop.as_str()));
            }
            // From the animator rather than the overrides: a compositor layer's values never reach those.
            let mut animated: StyleValues = Vec::new();
            if let Some(s) = shadow
                && !still(doc)
            {
                let now = self.now_s();
                animated = self.animator.peek(doc.get(s).id, now);
            }
            return self.engine.computed_style(id, q.get("pseudo").as_str(), &animated);
        }
        if kind == b"elementFromPoint" {
            let hit = self.widget_id_at(q.get("x").as_number(0.0), q.get("y").as_number(0.0), None);
            return Json::from(hit as f64);
        }
        if kind == b"measureText" {
            let font = if q.get("font").is_string() { q.get("font").as_str().to_vec() } else { b"10px sans-serif".to_vec() };
            return self.measure_text(&font, q.get("text").as_str());
        }
        if kind == b"validity" {
            let mut o = Json::object();
            o.set("valid", true).set("message", "");
            return o;
        }
        if kind == b"matchMedia" {
            let mut o = Json::object();
            o.set("matches", self.match_media(doc.dark(), q.get("query").as_str()));
            return o;
        }
        if kind == b"viewport" {
            let mut o = Json::object();
            o.set("innerWidth", self.width).set("innerHeight", f64::from(self.vp_height)).set("dpr", self.dpr);
            return o;
        }
        if kind == b"anims" {
            let mut ids = Vec::new();
            if let Some(s) = shadow {
                fn visit(doc: &Document, r: NodeRef, subtree: bool, ids: &mut Vec<i64>) {
                    let n = doc.get(r);
                    if n.id != 0 && ids.len() < 100000 {
                        ids.push(n.id);
                    }
                    if subtree {
                        for &c in &n.children {
                            visit(doc, c, subtree, ids);
                        }
                    }
                }
                visit(doc, s, q.get("subtree").as_bool(false), &mut ids);
            }
            let now = self.now_s();
            let mut o = Json::object();
            o.set("list", self.animator.css_animations(&ids, now));
            return o;
        }
        if kind == b"imageData" {
            let w = q.get("w").as_int(0).clamp(0, Canvas::MAX_SIDE);
            let h = q.get("h").as_int(0).clamp(0, Canvas::MAX_SIDE);
            if i64::from(w) * i64::from(h) > Canvas::MAX_PIXELS {
                return error("too large");
            }
            let size = w as usize * h as usize * 4;
            let mut rgba = Vec::new();
            let canvas = shadow.and_then(|s| doc.canvas(doc.get(s).id)).map(Canvas::id);
            let read = match canvas {
                Some(c) => self.backend.canvas_read(c, q.get("x").as_int(0), q.get("y").as_int(0), w, h, &mut rgba) && rgba.len() == size,
                None => false,
            };
            if !read {
                rgba = vec![0; size];
            }
            self.binary = twidget_base64::encode(&rgba);
            let mut o = Json::object();
            o.set("width", w).set("height", h);
            return o;
        }
        if kind == b"canvasCall" {
            let canvas = shadow.and_then(|s| doc.canvas(doc.get(s).id));
            let method = q.get("method").as_str();
            let Some(canvas) = canvas else {
                if method == b"getTransform" {
                    let mut a = Json::array();
                    a.push(1).push(0).push(0).push(1).push(0).push(0);
                    return a;
                }
                if method == b"getLineDash" {
                    return Json::array();
                }
                if method == b"isPointInPath" || method == b"isPointInStroke" {
                    return Json::from(false);
                }
                return error("no canvas");
            };
            if method == b"toDataURL" {
                return error("toDataURL unsupported by the litehtml renderer");
            }
            return canvas.call(method, q.get("args"));
        }
        if kind == b"svg" {
            let method = q.get("method").as_str();
            if el && let Some([x, y, w, h]) = self.engine.svg_text_box(id) {
                if method == b"getComputedTextLength" {
                    return Json::from(f64::from(w));
                }
                if method == b"getBBox" {
                    let mut o = Json::object();
                    o.set("x", f64::from(x)).set("y", f64::from(y)).set("width", f64::from(w)).set("height", f64::from(h));
                    return o;
                }
            }
            if !el || !self.engine.svg_geometry(id) {
                return error("unsupported");
            }
            if method == b"getTotalLength" {
                return Json::from(f64::from(self.engine.svg_path_length(id, -1.0).0));
            }
            if method == b"getPointAtLength" {
                let at = q.get("args").at(0).as_number(0.0);
                let at = if at.is_finite() { max_f64(0.0, at) as f32 } else { 0.0 };
                let (_, x, y) = self.engine.svg_path_length(id, at);
                let mut o = Json::object();
                o.set("x", f64::from(x)).set("y", f64::from(y));
                return o;
            }
            if method == b"getBBox" {
                let Some([x0, y0, x1, y1]) = self.engine.svg_path_bounds(id) else { return error("unsupported") };
                let mut o = Json::object();
                o.set("x", f64::from(x0)).set("y", f64::from(y0)).set("width", f64::from(x1 - x0)).set("height", f64::from(y1 - y0));
                return o;
            }
            return error("unsupported");
        }
        error("unknown query")
    }
}

impl<E: Engine, B: Backend> EventHost for Core<E, B> {
    fn now_ms(&mut self) -> f64 {
        Core::now_ms(self)
    }
    fn width(&mut self) -> f64 {
        self.width
    }
    fn settle(&mut self, doc: &mut Document, _out: Option<&mut Vec<Json>>) -> bool {
        if !doc.stale {
            return false;
        }
        self.rebuild_body(doc, true);
        true
    }
    fn settle_ends(&mut self, doc: &mut Document, out: Option<&mut Vec<Json>>) {
        self.rebuild_ends(doc);
        self.report_size(out);
    }
    fn rebuild(&mut self, doc: &mut Document, patch: bool) {
        self.rebuild_body(doc, patch);
    }
    fn rebuild_ends(&mut self, doc: &mut Document) {
        Core::rebuild_ends(self, doc);
    }
    fn report_size(&mut self, out: &mut Vec<Json>) {
        Core::report_size(self, Some(out));
    }
    fn restyled_by_input(&mut self, doc: &mut Document) {
        // :hover and :active restyle without a message: the transitions they start begin now, not with the next batch.
        self.dirty = true;
        self.touched_all = true;
        self.layers.paint_scan = true;
        self.animate(doc);
    }
    fn selection_changed(&mut self) {
        self.dirty = true;
        self.touched_all = true;
    }
    fn pressed(&mut self, id: i64, y: f64, at: f64) {
        self.press_id = id;
        self.press_y = y;
        self.press_at = at;
    }
    fn hit(&mut self, x: f64, y: f64, placement: Option<&mut Rect>) -> i64 {
        self.widget_id_at(x, y, placement)
    }
    fn box_of(&mut self, id: i64) -> Rect {
        self.engine.box_of(id)
    }
    fn has_element(&mut self, id: i64) -> bool {
        self.engine.has_element(id)
    }
    fn touch_action(&mut self, id: i64) -> Option<Vec<u8>> {
        self.engine.touch_action(id)
    }
    fn user_select_text(&mut self, id: i64) -> bool {
        self.engine.user_select_text(id)
    }
    fn mouse(&mut self, action: i32, x: f64, y: f64, hovers: bool) -> bool {
        self.engine.mouse(action, x, y, hovers)
    }
    fn leave(&mut self) -> bool {
        self.engine.leave()
    }
    fn cursor(&mut self) -> Vec<u8> {
        self.engine.cursor()
    }
    fn editor_style(&mut self, id: i64, clip: Rect) -> Option<EditorStyle> {
        self.engine.editor_style(id, clip)
    }
    fn image_of(&mut self, src: &[u8]) -> Option<(i32, f32, f32)> {
        self.engine.image_of(src)
    }
    fn image_ready(&mut self, id: i32, w: f32, h: f32) -> bool {
        self.engine.image_ready(id, w, h)
    }
    fn content_size(&mut self, id: i64) -> (f32, f32) {
        self.engine.content_size(id)
    }
    fn take_pending(&mut self, out: &mut Vec<Json>) {
        out.append(&mut self.pending_events);
    }
    fn runs_key(&mut self) -> [u64; 4] {
        self.engine.runs_key()
    }
    fn text_runs(&mut self) -> Vec<TextRun> {
        self.engine.text_runs()
    }
    fn text_width(&mut self, text: &[u8], font: u64) -> f32 {
        self.engine.text_width(text, font)
    }
    fn font_info(&mut self, font: u64) -> Option<(i32, f32, f32, f32)> {
        self.engine.font_info(font)
    }
    fn layer_shown(&mut self, id: i64, now: f64) -> Option<(Affine, f32)> {
        Core::layer_shown(self, id, now)
    }
    fn selectable_key(&mut self) -> [u64; 2] {
        self.engine.selectable_key()
    }
    fn any_selectable(&mut self) -> bool {
        self.engine.any_selectable()
    }
    fn editing(&mut self, id: i64) {
        self.editing = id;
    }
    fn take_violations(&mut self) -> Vec<Vec<u8>> {
        self.engine.take_violations()
    }
}

/// One widget's renderer (renderer.h's `Renderer`): host messages in, renderer messages out.
pub struct Renderer<E: Engine, B: Backend> {
    doc: Document,
    events: Events,
    core: Core<E, B>,
}

impl<E: Engine, B: Backend> std::fmt::Debug for Renderer<E, B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer").field("doc", &self.doc).field("events", &self.events).field("core", &self.core).finish()
    }
}

impl<E: Engine, B: Backend> Renderer<E, B> {
    /// `clock`, when given (else the steady clock), is what the renderer reads the time from.
    pub fn new(width: f64, dpr: f64, dark: bool, engine: E, backend: B, clock: Option<Box<dyn Clock + Send>>) -> Renderer<E, B> {
        let core = Core {
            engine,
            backend,
            clock,
            container: Container::default(),
            width: bounded(width, 0.0, WIDTH_CEILING),
            dpr: bounded(dpr, MIN_DPR, MAX_DPR),
            vp_height: MIN_HEIGHT as i32,
            doc_height: 0.0,
            max_height: 600.0,
            dirty: true,
            touched: BTreeSet::new(),
            touched_all: true,
            patching: true,
            html: Vec::new(),
            html_stale: false,
            stats: BuildStats::default(),
            editing: 0,
            interest: HashSet::new(),
            meter_depth: 0,
            name_meter_depth: 0,
            rebuilds: Vec::new(),
            charges: VecDeque::new(),
            first_charge: -1.0,
            throttled_until: 0.0,
            last_size: [0.0; 4],
            track_resize: false,
            prefers_bottom: false,
            edges: BTreeMap::new(),
            edges_height: 0.0,
            edges_layouts: 0,
            has_resize: false,
            resize: ResizeDiff::default(),
            resize_at: -1e9,
            resize_serial: 0,
            resize_reported: 0,
            press_id: 0,
            press_y: 0.0,
            press_at: -1e9,
            pending_events: Vec::new(),
            animator: Animator::new(),
            layers: Layers::default(),
            overrides: StyleOverrides::new(),
            sheets_key: Vec::new(),
            animating: false,
            declared_generation: 0,
            scripted_pending: false,
            canvas_fonts: HashMap::new(),
            canvas_font_order: Vec::new(),
            canvas_font_hook: None,
            binary: String::new(),
        };
        let mut r = Renderer { doc: Document::new(dark), events: Events::new(), core };
        r.core.engine.viewport(r.core.width, r.core.dpr);
        r.core.set_master(&r.doc);
        r.rebuild(false);
        r
    }

    /// The tree, for tests and tools.
    pub fn document(&self) -> &Document {
        &self.doc
    }
    /// The input, for tests and tools.
    pub fn events(&self) -> &Events {
        &self.events
    }
    pub fn core(&self) -> &Core<E, B> {
        &self.core
    }
    pub fn core_mut(&mut self) -> &mut Core<E, B> {
        &mut self.core
    }

    fn follow(&mut self, before: Before) {
        if !before.frozen && self.doc.frozen() {
            self.events.on_frozen();
        }
        if !before.paint_stopped && self.doc.paint_stopped {
            self.events.drop_moves();
        }
    }

    /// `rebuild(patch)` from the renderer's own code: the build, the editor brought up to it, the meters.
    fn rebuild(&mut self, patch: bool) {
        let before = Before::of(&self.doc);
        self.core.editing = self.events.editing_id();
        self.core.rebuild_body(&mut self.doc, patch);
        self.events.refresh_editor(&self.doc, &mut self.core);
        self.core.rebuild_ends(&mut self.doc);
        self.follow(before);
    }

    /// Builds a document a coalesced batch left stale, its size into `out` or, for None, the pending messages.
    fn settle(&mut self, out: Option<&mut Vec<Json>>) {
        if !self.doc.stale {
            return;
        }
        self.rebuild(true);
        self.core.report_size(out);
    }

    fn frame_due(&mut self) -> bool {
        if still(&self.doc) {
            return false;
        }
        if self.doc.stale || self.events.has_pending_moves() {
            return true;
        }
        let now = self.core.now_s();
        self.core.animator.active_except(now, &self.core.layers.layer_ids)
    }

    /// Host → renderer message (parsed JSON). Returns renderer → host messages.
    pub fn receive(&mut self, msg: &Json) -> Vec<Json> {
        let names = self.core.names_begin();
        let mut out = Vec::new();
        let before = Before::of(&self.doc);
        match msg.get("type").as_str() {
            b"ops" => {
                // Past the paint limit the host stops the widget: what its task still sent is not drawn.
                if self.doc.paint_stopped || self.doc.layout_stopped() {
                    self.core.names_end(&mut self.doc, names);
                    self.follow(before);
                    return out;
                }
                let ops = msg.get("ops").arr();
                if ops.len() > twidget_shadow::MAX_BATCH_OPS {
                    let mut m = Json::object();
                    m.set("type", "violation").set("detail", format!("ops limit: {} operations in one batch", ops.len()));
                    out.push(m);
                    self.core.names_end(&mut self.doc, names);
                    self.follow(before);
                    return out;
                }
                let scripted = self.core.scripted_only(&self.doc, ops);
                let fast = scripted.is_some();
                // Ticking already (something animates), or held back by the throttle, a batch waits for the frame: a
                // script sending several a frame builds once. Input wants its answer now. A part of a long flush
                // always waits: the rest of the task follows at once, and its last part builds them all.
                let partial = ops.len() >= PARTIAL_BATCH_OPS;
                let last_input = self.events.last_input;
                let coalesce = partial
                    || ((self.frame_due() || self.core.throttled(last_input)) && self.core.now_s() - last_input > INPUT_IMMEDIATE_S);
                self.doc.document_changed = false;
                {
                    let mut bridge = Bridge { core: &mut self.core, events: &mut self.events };
                    self.doc.apply_ops(ops, &mut bridge, &mut out);
                }
                self.core.hold_canvases(&self.doc);
                self.events.load_images(&mut self.doc, &mut self.core, &mut out);
                let mut applied = false;
                if let Some(ids) = &scripted {
                    let now = self.core.now_s();
                    applied = self.core.apply_scripted(&self.doc, ids, now);
                }
                if applied {
                    self.doc.document_changed = false;
                    self.core.scripted_pending = true;
                    self.core.stats.scripted += 1;
                    // The document takes these at its next build; their opacity counts now.
                    let ids = scripted.unwrap_or_default();
                    self.core.recount_boxes(&mut self.doc, &ids);
                } else {
                    // Moved past what its layer allows, a scripted layer is planned again where it is now.
                    if fast {
                        self.core.layers.composition_key = 0;
                    } else {
                        let now = self.core.now_s();
                        self.core.note_scripted(ops, now);
                    }
                    self.core.note_svg(&self.doc, ops);
                    // A batch of canvas drawing, scrolling or focus leaves the document as it was: its canvases
                    // repaint (run_canvas marks the paint dirty) without a build.
                    if self.doc.document_changed && coalesce {
                        self.doc.stale = true;
                    } else if self.doc.document_changed || (self.doc.stale && !partial) {
                        self.rebuild(true);
                        self.core.report_size(Some(&mut out));
                    }
                }
                // A batch that built nothing can still have asked for new layers (a canvas drawn once more).
                if self.core.layers.composition_key == 0 && self.core.layers.compositing && !self.doc.stale {
                    let now = self.core.now_s();
                    self.core.update_composition(&self.doc, now);
                }
            }
            b"query" => {
                // A query reads the document, which must have what only the layers show.
                if self.core.scripted_pending {
                    self.doc.stale = true;
                }
                self.settle(Some(&mut out));
                let mut res = Json::object();
                self.core.binary.clear();
                res.set("type", "result").set("qid", msg.get("qid").clone());
                let result = match msg.find("query") {
                    Some(q) => self.core.answer(&self.doc, q),
                    None => error("no query"),
                };
                res.set("result", result);
                if !self.core.binary.is_empty() {
                    res.set("binaryBase64", std::mem::take(&mut self.core.binary));
                }
                self.core.binary.clear();
                out.push(res);
            }
            b"anim" => {
                self.settle(Some(&mut out));
                if !still(&self.doc) {
                    // `now` is the sandbox clock when the batch was sent: it maps the records' start times onto ours.
                    let now = self.core.now_ms();
                    let offset = now - msg.get("now").as_number(now);
                    self.core.animator.apply_web(msg, offset);
                    self.core.editing = self.events.editing_id();
                    self.core.animate(&mut self.doc);
                    self.core.report_size(Some(&mut out));
                }
            }
            b"listen" => {
                self.events.receive(msg);
                self.core.interest = self.events.interest().clone();
            }
            b"preventDefault" | b"arbitrate" => self.events.receive(msg),
            b"theme" => {
                if self.doc.receive_theme(msg.get("theme")) {
                    self.core.set_master(&self.doc);
                    self.rebuild(false);
                    self.core.report_size(Some(&mut out));
                }
            }
            b"safeArea" => {
                // The client's time over the widget (docs/sdk-v1.md §2.17): the UA layer pads the body by its band.
                if self.doc.apply_safe_area(msg.get("inset")) {
                    self.core.set_master(&self.doc);
                    self.rebuild(false);
                    self.core.report_size(Some(&mut out));
                }
            }
            b"frozen" => {
                {
                    let mut bridge = Bridge { core: &mut self.core, events: &mut self.events };
                    self.doc.freeze(msg.get("reason").as_str(), msg.get("fatal").as_bool(false), &mut bridge);
                }
                self.core.hold_canvases(&self.doc);
                self.rebuild(false);
            }
            b"policy" => {
                // Heavy boxes are counted unless the host says compat (also before this message, and for anything else).
                let on = msg.get("policy").as_str() != b"compat";
                self.core.policy(&mut self.doc, on, msg.get("paintReports").as_bool(false));
            }
            _ => {}
        }
        self.events.flush_pending(&mut self.doc, &mut self.core, &mut out);
        self.core.names_end(&mut self.doc, names);
        self.follow(before);
        out
    }

    pub fn set_width(&mut self, width: f64, out: &mut Vec<Json>) {
        let width = bounded(width, 0.0, WIDTH_CEILING);
        if (width - self.core.width).abs() < 0.01 {
            return;
        }
        self.core.width = width;
        self.core.engine.viewport(width, self.core.dpr);
        self.rebuild(false);
        self.core.report_size(Some(out));
    }

    /// The tallest the widget may grow, in CSS px (600, widget-sandbox's shared bound, unless the host says
    /// otherwise); past it the document is cut off.
    pub fn set_max_height(&mut self, height: f64, out: &mut Vec<Json>) {
        let height = bounded(height, MIN_HEIGHT, HEIGHT_CEILING);
        if height == self.core.max_height {
            return;
        }
        self.core.max_height = height;
        self.rebuild(false);
        self.core.report_size(Some(out));
    }

    /// The chat theme changed: `prefers-color-scheme` inside the document follows.
    pub fn set_theme(&mut self, dark: bool, out: &mut Vec<Json>) {
        if !self.doc.set_dark(dark) {
            return;
        }
        self.core.set_master(&self.doc);
        self.rebuild(false);
        self.core.report_size(Some(out));
    }

    /// Lets the platform's compositor run transform and opacity animations.
    pub fn set_compositing(&mut self, enabled: bool) {
        if enabled == self.core.layers.compositing {
            return;
        }
        self.core.layers.compositing = enabled;
        self.core.layers.composition_key = 0;
        self.rebuild(false);
    }

    /// The layers that go above the display list `paint()` returned last.
    pub fn composition(&self) -> &Composition {
        &self.core.layers.composition
    }

    /// A call of the input's that charges the names it left (the C++'s NameMeter around it).
    fn metered<T>(&mut self, call: impl FnOnce(&mut Events, &mut Document, &mut Core<E, B>) -> T) -> T {
        let names = self.core.names_begin();
        let before = Before::of(&self.doc);
        let out = call(&mut self.events, &mut self.doc, &mut self.core);
        self.core.names_end(&mut self.doc, names);
        self.follow(before);
        out
    }

    pub fn touch(
        &mut self,
        action: i32,
        x: f64,
        y: f64,
        pointer: i32,
        primary: bool,
        mouse: bool,
        modifiers: i32,
        button: i32,
    ) -> Vec<Json> {
        self.metered(|events, doc, core| events.touch(doc, core, action, x, y, pointer, primary, mouse, modifiers, button))
    }

    pub fn hover(&mut self, action: i32, x: f64, y: f64, modifiers: i32) -> Vec<Json> {
        self.metered(|events, doc, core| events.hover(doc, core, action, x, y, modifiers))
    }

    pub fn cursor(&mut self) -> Vec<u8> {
        self.events.cursor(&self.doc, &mut self.core)
    }

    pub fn claim_at(&mut self, x: f64, y: f64) -> WidgetGestureClaim {
        self.metered(|events, doc, core| events.claim_at(doc, core, x, y))
    }

    pub fn wheel(&mut self, x: f64, y: f64, dx: f64, dy: f64, mode: i32, modifiers: i32) -> Vec<Json> {
        self.events.wheel(x, y, dx, dy, mode, modifiers)
    }

    pub fn key(&mut self, kind: &[u8], key: &[u8], code: &[u8], repeat: bool, modifiers: i32) -> Vec<Json> {
        self.events.key(kind, key, code, repeat, modifiers)
    }

    pub fn editor(&self) -> Option<&EditorSpec> {
        self.events.editor()
    }

    pub fn editor_json(&self) -> Vec<u8> {
        self.events.editor_json()
    }

    pub fn edit(&mut self, generation: u64, text: &[u8], input_type: &[u8]) -> Vec<Json> {
        self.metered(|events, doc, core| events.edit(doc, core, generation, text, input_type))
    }

    pub fn end_editing(&mut self) -> Vec<Json> {
        self.metered(|events, doc, core| events.end_editing(doc, core))
    }

    pub fn has_selection(&mut self) -> bool {
        self.events.has_selection(&mut self.core)
    }

    pub fn may_have_selection(&self) -> bool {
        self.events.may_have_selection()
    }

    pub fn selected_text(&mut self) -> Vec<u8> {
        self.events.selected_text(&mut self.core)
    }

    pub fn clear_selection(&mut self) -> bool {
        self.events.clear_selection(&mut self.core)
    }

    pub fn selectable_text_json(&mut self) -> Vec<u8> {
        self.events.selectable_text_json(&mut self.core)
    }

    /// The master CSS documents are parsed with.
    pub fn master_css(&self) -> &[u8] {
        self.core.container.master()
    }

    /// The variables the host sets on the root: the theme's and the content safe area's.
    pub fn theme_variables(&self) -> &css::Variables {
        self.doc.theme_variables()
    }

    pub fn theme_variables_json(&self) -> Vec<u8> {
        let mut out = Json::object();
        for (name, value) in self.doc.theme_variables() {
            out.set(name.clone(), value.clone());
        }
        out.dump()
    }

    /// One display frame: coalesced move events go out and animations advance.
    pub fn tick(&mut self) -> Vec<Json> {
        let names = self.core.names_begin();
        let mut out = Vec::new();
        let before = Before::of(&self.doc);
        self.events.check_hold(&mut self.doc, &mut self.core, &mut out);
        // Held back, a frame only hands out the coalesced moves; the build and the animation step wait.
        if self.core.throttled(self.events.last_input) {
            self.events.flush_moves(&mut out);
            self.events.flush_pending(&mut self.doc, &mut self.core, &mut out);
            self.core.names_end(&mut self.doc, names);
            self.follow(before);
            return out;
        }
        let meter = self.core.meter_begin();
        self.settle(Some(&mut out));
        self.events.flush_moves(&mut out);
        let now = self.core.now_s();
        if !still(&self.doc) && (self.core.animator.active(now) || now >= self.core.layers.wake_at) {
            self.core.editing = self.events.editing_id();
            self.core.update_composition(&self.doc, now);
            let mut sampled = self.core.animator.sample(now);
            self.core.strip_layers(&mut sampled);
            self.core.queue_animation_events(&self.doc);
            self.core.apply_overrides(&mut self.doc, sampled);
            self.core.count_paint(&mut self.doc);
            self.core.report_size(Some(&mut out));
        }
        self.events.flush_pending(&mut self.doc, &mut self.core, &mut out);
        self.core.meter_end(meter);
        self.core.names_end(&mut self.doc, names);
        self.follow(before);
        out
    }

    /// Something waits for the next frame.
    pub fn wants_tick(&mut self) -> bool {
        let hold = self.events.hold_deadline(&self.doc);
        if !self.core.pending_events.is_empty() || !self.doc.pending_violations.is_empty() || self.frame_due() {
            return true;
        }
        hold.is_some_and(|at| self.core.now_ms() >= at)
    }

    /// When `wants_tick()` is false: seconds until `tick()` is due anyway; infinity when nothing is scheduled.
    pub fn next_tick_in(&mut self) -> f64 {
        if !self.core.pending_events.is_empty() || !self.doc.pending_violations.is_empty() {
            return 0.0;
        }
        if still(&self.doc) {
            return f64::INFINITY;
        }
        let mut hold = f64::INFINITY;
        if let Some(at) = self.events.hold_deadline(&self.doc) {
            hold = max_f64(0.0, (at - self.core.now_ms()) / 1000.0);
        }
        if self.doc.stale {
            let until = self.core.throttled_until - self.core.now_s();
            return min_f64(hold, max_f64(0.0, until));
        }
        let until = self.core.layers.wake_at - self.core.now_s();
        min_f64(hold, max_f64(0.0, until))
    }

    /// A backend that decodes images asynchronously reports the size of image `id` here.
    pub fn image_ready(&mut self, id: i32, w: f32, h: f32) -> Vec<Json> {
        self.metered(|events, doc, core| events.image_ready(doc, core, id, w, h))
    }

    /// From now on canvas text takes every font from `hook`; the fonts the renderer had cached are released now.
    /// None gives the cache back.
    pub fn set_canvas_font_hook(&mut self, hook: Option<CanvasFontHook>) {
        if hook.is_some() {
            self.core.release_canvas_fonts();
        }
        self.core.canvas_font_hook = hook;
    }

    /// Diffs each layout that changes the document's height.
    pub fn set_resize_tracking(&mut self, enabled: bool, prefers_bottom: bool) {
        self.core.track_resize = enabled;
        self.core.prefers_bottom = prefers_bottom;
        if !enabled {
            self.core.edges.clear();
            self.core.edges_layouts = 0;
        }
    }

    pub fn resize_diff(&self) -> Option<&ResizeDiff> {
        self.core.has_resize.then_some(&self.core.resize)
    }

    pub fn width(&self) -> f64 {
        self.core.width
    }
    pub fn viewport_height(&self) -> i32 {
        self.core.vp_height
    }
    pub fn dpr(&self) -> f64 {
        self.core.dpr
    }
    pub fn px_width(&self) -> u32 {
        (self.core.width * self.core.dpr).ceil() as i64 as u32
    }
    pub fn px_height(&self) -> u32 {
        (f64::from(self.core.vp_height) * self.core.dpr).ceil() as i64 as u32
    }
    pub fn dirty(&self) -> bool {
        self.core.dirty
    }
    pub fn stats(&self) -> &BuildStats {
        &self.core.stats
    }
    /// Off: every batch serialises the document again. For checks that patching changes nothing.
    pub fn set_patching(&mut self, enabled: bool) {
        self.core.patching = enabled;
    }
    /// Layouts of the document so far.
    pub fn layouts(&self) -> u64 {
        self.core.container.layouts()
    }

    /// Records the current document into a display list (the layers above it in `composition()`).
    pub fn paint(&mut self) -> DisplayList {
        let before = Before::of(&self.doc);
        let meter = self.core.meter_begin();
        let names = self.core.names_begin();
        // Messages from a build here go out with the next ones.
        self.settle(None);
        let list = self.core.paint_frame(&self.doc, &mut self.events);
        self.core.names_end(&mut self.doc, names);
        self.core.meter_end(meter);
        self.follow(before);
        list
    }

    /// Where the last `paint()` differs from the one before, in CSS px; false when everything must be painted (the
    /// first paint, a new size). Empty when nothing moved.
    pub fn damage(&self, out: &mut Vec<Rect>) -> bool {
        self.core.damage(out)
    }

    /// damage() as JSON for the C ABI: `null` (paint everything) or `[[x, y, w, h], ...]`.
    pub fn damage_json(&self) -> Vec<u8> {
        if !self.core.layers.damage_valid {
            return b"null".to_vec();
        }
        let mut out = Json::array();
        for r in &self.core.layers.damage {
            let mut one = Json::array();
            one.push(f64::from(r.x)).push(f64::from(r.y)).push(f64::from(r.w)).push(f64::from(r.h));
            out.push(one);
        }
        out.dump()
    }

    /// Drops caches that are made again on demand; the next paint is a whole one.
    pub fn trim(&mut self) {
        self.core.engine.trim();
        self.doc.trim_canvases();
        self.core.layers.trim();
        self.core.declared_generation = 0;
    }

    /// The document as HTML, built first when a batch is waiting for its frame, and serialised again when style
    /// patches changed it since the last build.
    pub fn last_html(&mut self) -> &[u8] {
        if self.doc.stale {
            self.settle(None);
        }
        if self.core.html_stale {
            self.core.editing = self.events.editing_id();
            self.core.html = self.core.serialize(&self.doc);
            self.core.html_stale = false;
        }
        &self.core.html
    }
}

impl<E: Engine, B: Backend> Drop for Renderer<E, B> {
    fn drop(&mut self) {
        // The canvases in node order, then the canvas fonts in the order they were made.
        let ids: Vec<i32> = self.doc.canvases().values().map(Canvas::id).collect();
        for id in ids {
            self.core.backend.canvas_release(id);
        }
        self.core.release_canvas_fonts();
    }
}

/// What the comparison and the tools read of the renderer's state.
impl<E: Engine, B: Backend> Core<E, B> {
    pub fn container(&self) -> &Container {
        &self.container
    }
    pub fn doc_height(&self) -> f64 {
        self.doc_height
    }
    pub fn max_height(&self) -> f64 {
        self.max_height
    }
    pub fn touched(&self) -> (&BTreeSet<i64>, bool) {
        (&self.touched, self.touched_all)
    }
    pub fn html_state(&self) -> (&[u8], bool) {
        (&self.html, self.html_stale)
    }
    pub fn patching(&self) -> bool {
        self.patching
    }
    pub fn last_size(&self) -> [f64; 4] {
        self.last_size
    }
    pub fn resize_tracking(&self) -> (bool, bool) {
        (self.track_resize, self.prefers_bottom)
    }
    pub fn resize_state(&self) -> (bool, &ResizeDiff, f64, u64, u64) {
        (self.has_resize, &self.resize, self.resize_at, self.resize_serial, self.resize_reported)
    }
    pub fn edges(&self) -> (&BTreeMap<i64, Edges>, f64, u64) {
        (&self.edges, self.edges_height, self.edges_layouts)
    }
    pub fn press(&self) -> (i64, f64, f64) {
        (self.press_id, self.press_y, self.press_at)
    }
    pub fn pending_events(&self) -> &[Json] {
        &self.pending_events
    }
    pub fn overrides(&self) -> &StyleOverrides {
        &self.overrides
    }
    pub fn sheets_key(&self) -> &[u8] {
        &self.sheets_key
    }
    pub fn declared_generation(&self) -> u64 {
        self.declared_generation
    }
    pub fn animating(&self) -> bool {
        self.animating
    }
    pub fn scripted_pending(&self) -> bool {
        self.scripted_pending
    }
    pub fn canvas_fonts(&self) -> (&CanvasFonts, &[i32]) {
        (&self.canvas_fonts, &self.canvas_font_order)
    }
    pub fn throttle(&self) -> (f64, f64, usize) {
        (self.first_charge, self.throttled_until, self.charges.len())
    }
    pub fn meters(&self) -> (i32, i32) {
        (self.meter_depth, self.name_meter_depth)
    }
    pub fn animator(&self) -> &Animator {
        &self.animator
    }
    pub fn editing(&self) -> i64 {
        self.editing
    }
}
