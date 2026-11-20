//! What the renderer comparison shares with `oracle/renderer_oracle`: scripts of messages, settings and input through one
//! renderer, the tape of the C++ document's and stage 4.5's answers the port replays, the transcript both sides write,
//! and the inputs (the input stage's, `inputs.rs` of twidget_events' tests, and this stage's own in `scripts.rs`).
//!
//! A script is 'S', a u32 length and: f64 width, f64 dpr, u8 dark, f64 the clock at construction (ms), u32 the largest
//! node id the tree digest walks to, u32 steps, each a u8 kind, f64 the clock (ms) and its fields ([`Step`]).
//!
//! The answer is status 1 and: u32 tape length, the tape ([`tape`]), the transcript. The transcript is the backend's
//! log of the construction (str) and the renderer's state ([`build_state`]), then per step its kind, every message
//! it gave (u32 count, each its JSON), what the step returned, the backend's log (str: 'R' canvas_run i32 canvas,
//! i32 width, i32 height, u32 ops, u8 encoded, u64 FNV-1a of the batch in wire form with every NaN as one; 'X' i32
//! canvas_release; 'G' i32 canvas_read's five; 'F' create_canvas_font str family, f32 size, i32 weight, u8 italic, i32
//! handle; 'D' i32 delete_font; 'W' i32 font, str text_width: the renderer's own calls, its canvas fonts numbered from
//! 2^20), the input's state ([`input_state`]), the renderer's ([`build_state`]) and the tree's digest ([`tree`]).

#![allow(dead_code)]

pub mod cases;
#[path = "../../../events/tests/common/inputs.rs"]
pub mod inputs;
pub mod scripts;
pub mod tape;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;
use std::rc::Rc;

use twidget_json::Json;
use twidget_renderer::{Backend, Clock, Renderer};
pub use twidget_testkit::Fields;
use twidget_wire::{CanvasOp, Caps, FontMetrics, encode_batch};

use tape::{Entry, Shared, TapeEngine, TapeLayers};

pub fn fnv64(v: &[u8]) -> u64 {
    fnv64_from(1469598103934665603u64, v)
}

pub fn fnv64_from(mut h: u64, v: &[u8]) -> u64 {
    for &c in v {
        h = (h ^ u64::from(c)).wrapping_mul(1099511628211);
    }
    h
}

#[derive(Debug, Default)]
pub struct Out(pub Vec<u8>);

impl Out {
    pub fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    pub fn flag(&mut self, v: bool) {
        self.0.push(u8::from(v));
    }
    pub fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn str(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v);
    }
    pub fn rect(&mut self, r: &twidget_wire::Rect) {
        self.f32(r.x);
        self.f32(r.y);
        self.f32(r.w);
        self.f32(r.h);
    }
}

/// One call into the renderer. The input stage's steps by its names (its `inputs.rs` makes them), and this stage's.
#[derive(Clone, Debug)]
pub enum Step {
    /// An `ops` message: the array's JSON text.
    Ops(Vec<u8>),
    /// Any other message as JSON text.
    Message(Vec<u8>),
    Dark(bool),
    Width(f64),
    MaxHeight(f64),
    Compositing(bool),
    Touch {
        action: i32,
        x: f64,
        y: f64,
        pointer: i32,
        primary: bool,
        mouse: bool,
        modifiers: i32,
        button: i32,
    },
    Hover {
        action: i32,
        x: f64,
        y: f64,
        modifiers: i32,
    },
    Claim {
        x: f64,
        y: f64,
    },
    /// `current`: the editor's generation as it is, else `generation`.
    Edit {
        current: bool,
        generation: u64,
        text: Vec<u8>,
        kind: Vec<u8>,
    },
    EndEditing,
    ClearSelection,
    ImageReady {
        id: i32,
        w: f32,
        h: f32,
    },
    Wheel {
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
        mode: i32,
        modifiers: i32,
    },
    Key {
        kind: Vec<u8>,
        key: Vec<u8>,
        code: Vec<u8>,
        repeat: bool,
        modifiers: i32,
    },
    Tick,
    /// A paint is stage 4.5's: its build is `last_html`'s, which this runs.
    Paint,
    LastHtml,
    /// cursor(), has_selection(), selected_text(), selectable_text_json().
    Observe,
    Trim,
    ResizeTracking {
        enabled: bool,
        prefers_bottom: bool,
    },
    Patching(bool),
    /// wants_tick(), next_tick_in(), master_css(), theme_variables_json(), editor_json(), layouts(), px_width(),
    /// px_height().
    Inspect,
    /// charge(seconds), the render throttle's.
    Charge(f64),
}

impl Step {
    pub fn kind(&self) -> u8 {
        match self {
            Step::Ops(_) => b'o',
            Step::Message(_) => b'm',
            Step::Dark(_) => b'd',
            Step::Width(_) => b'w',
            Step::MaxHeight(_) => b'M',
            Step::Compositing(_) => b'C',
            Step::Touch { .. } => b't',
            Step::Hover { .. } => b'h',
            Step::Claim { .. } => b'c',
            Step::Edit { .. } => b'e',
            Step::EndEditing => b'E',
            Step::ClearSelection => b'x',
            Step::ImageReady { .. } => b'i',
            Step::Wheel { .. } => b'W',
            Step::Key { .. } => b'k',
            Step::Tick => b'f',
            Step::Paint | Step::LastHtml => b'l',
            Step::Observe => b'q',
            Step::Trim => b'T',
            Step::ResizeTracking { .. } => b'r',
            Step::Patching(_) => b'P',
            Step::Inspect => b'n',
            Step::Charge(_) => b'g',
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Step::Ops(_) => "ops",
            Step::Message(_) => "message",
            Step::Dark(_) => "set_theme",
            Step::Width(_) => "set_width",
            Step::MaxHeight(_) => "set_max_height",
            Step::Compositing(_) => "set_compositing",
            Step::Touch { .. } => "touch",
            Step::Hover { .. } => "hover",
            Step::Claim { .. } => "claim_at",
            Step::Edit { .. } => "edit",
            Step::EndEditing => "end_editing",
            Step::ClearSelection => "clear_selection",
            Step::ImageReady { .. } => "image_ready",
            Step::Wheel { .. } => "wheel",
            Step::Key { .. } => "key",
            Step::Tick => "tick",
            Step::Paint | Step::LastHtml => "last_html",
            Step::Observe => "observe",
            Step::Trim => "trim",
            Step::ResizeTracking { .. } => "set_resize_tracking",
            Step::Patching(_) => "set_patching",
            Step::Inspect => "inspect",
            Step::Charge(_) => "charge",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Script {
    pub width: f64,
    pub dpr: f64,
    pub dark: bool,
    pub t0: f64,
    pub max_id: u32,
    /// Each step with the clock (ms) it runs at.
    pub steps: Vec<(f64, Step)>,
}

impl Script {
    pub fn new(max_id: u32) -> Script {
        Script { width: 300.0, dpr: 1.0, dark: false, t0: 1000.0, max_id, steps: Vec::new() }
    }

    /// The clock of the last step, or of the construction.
    pub fn now(&self) -> f64 {
        self.steps.last().map_or(self.t0, |s| s.0)
    }

    /// `step` `ms` after the last one.
    pub fn after(&mut self, ms: f64, step: Step) -> &mut Script {
        let t = self.now() + ms;
        self.steps.push((t, step));
        self
    }

    pub fn ops(&mut self, ms: f64, ops: &str) -> &mut Script {
        self.after(ms, Step::Ops(ops.as_bytes().to_vec()))
    }

    pub fn message(&mut self, ms: f64, msg: &str) -> &mut Script {
        self.after(ms, Step::Message(msg.as_bytes().to_vec()))
    }

    pub fn touch(&mut self, ms: f64, action: i32, x: f64, y: f64, pointer: i32, mouse: bool) -> &mut Script {
        self.after(ms, Step::Touch { action, x, y, pointer, primary: true, mouse, modifiers: 0, button: 0 })
    }

    /// A query message: `{"type":"query","qid":qid,"query":query}`.
    pub fn query(&mut self, ms: f64, qid: i64, query: &str) -> &mut Script {
        self.after(ms, Step::Message(format!("{{\"type\":\"query\",\"qid\":{qid},\"query\":{query}}}").into_bytes()))
    }

    pub fn record(&self) -> Vec<u8> {
        let mut f = Fields::new();
        f.f64(self.width).f64(self.dpr).flag(self.dark).f64(self.t0).u32(self.max_id).u32(self.steps.len() as u32);
        for (t, step) in &self.steps {
            f.u8(step.kind()).f64(*t);
            match step {
                Step::Ops(text) | Step::Message(text) => {
                    f.str(text);
                }
                Step::Dark(d) | Step::Compositing(d) | Step::Patching(d) => {
                    f.flag(*d);
                }
                Step::Width(w) | Step::MaxHeight(w) => {
                    f.f64(*w);
                }
                Step::Touch { action, x, y, pointer, primary, mouse, modifiers, button } => {
                    f.i32(*action).f64(*x).f64(*y).i32(*pointer).flag(*primary).flag(*mouse).i32(*modifiers).i32(*button);
                }
                Step::Hover { action, x, y, modifiers } => {
                    f.i32(*action).f64(*x).f64(*y).i32(*modifiers);
                }
                Step::Claim { x, y } => {
                    f.f64(*x).f64(*y);
                }
                Step::Edit { current, generation, text, kind } => {
                    f.u8(if *current { 0 } else { 1 }).u64(*generation).str(text).str(kind);
                }
                Step::ImageReady { id, w, h } => {
                    f.i32(*id).u32(w.to_bits()).u32(h.to_bits());
                }
                Step::Wheel { x, y, dx, dy, mode, modifiers } => {
                    f.f64(*x).f64(*y).f64(*dx).f64(*dy).i32(*mode).i32(*modifiers);
                }
                Step::Key { kind, key, code, repeat, modifiers } => {
                    f.str(kind).str(key).str(code).flag(*repeat).i32(*modifiers);
                }
                Step::ResizeTracking { enabled, prefers_bottom } => {
                    f.flag(*enabled).flag(*prefers_bottom);
                }
                Step::Charge(seconds) => {
                    f.f64(*seconds);
                }
                Step::EndEditing
                | Step::ClearSelection
                | Step::Tick
                | Step::Paint
                | Step::LastHtml
                | Step::Observe
                | Step::Trim
                | Step::Inspect => {}
            }
        }
        f.framed(b'S')
    }
}

/// What the tallies count.
#[derive(Debug, Default)]
pub struct Tally(pub BTreeMap<String, usize>);

impl Tally {
    pub fn add(&mut self, kind: impl Into<String>) {
        *self.0.entry(kind.into()).or_default() += 1;
    }
    pub fn add_n(&mut self, kind: impl Into<String>, n: usize) {
        *self.0.entry(kind.into()).or_default() += n;
    }
    pub fn show(&self) -> String {
        let mut s = String::new();
        for (k, v) in &self.0 {
            let _ = write!(s, " {k}={v}");
        }
        s
    }
    pub fn count(&self, kind: &str) -> usize {
        self.0.get(kind).copied().unwrap_or(0)
    }
    pub fn merge(&mut self, other: Tally) {
        for (k, v) in other.0 {
            *self.0.entry(k).or_default() += v;
        }
    }
}

// ---- The backend and the clock ------------------------------------------------------------------------------

/// The handles of canvas fonts start past this one, as the oracle's stub numbers them.
pub const CANVAS_FONTS: i32 = 1 << 20;

/// A batch's floats with every NaN as one, as the oracle writes them (canvas_oracle.cpp's `canonical`).
fn canonical(ops: &[CanvasOp]) -> Vec<CanvasOp> {
    use std::collections::HashMap as Map;
    use std::sync::Arc;
    fn one(v: &mut f32) {
        if v.is_nan() {
            *v = f32::NAN;
        }
    }
    let mut clips: Map<*const Vec<twidget_wire::CanvasClip>, Arc<Vec<twidget_wire::CanvasClip>>> = Map::new();
    let mut out = ops.to_vec();
    for op in &mut out {
        op.path.points.iter_mut().for_each(one);
        op.paint.args.iter_mut().for_each(one);
        op.paint.stops.iter_mut().for_each(|s| one(&mut s.offset));
        op.matrix.iter_mut().for_each(one);
        op.dashes.iter_mut().for_each(one);
        for v in [
            &mut op.line_width,
            &mut op.miter_limit,
            &mut op.dash_offset,
            &mut op.alpha,
            &mut op.shadow_blur,
            &mut op.shadow_x,
            &mut op.shadow_y,
            &mut op.x,
            &mut op.y,
            &mut op.scale_x,
            &mut op.text_dx,
            &mut op.sx,
            &mut op.sy,
            &mut op.sw,
            &mut op.sh,
            &mut op.dx,
            &mut op.dy,
            &mut op.dw,
            &mut op.dh,
        ] {
            one(v);
        }
        if let Some(list) = &op.clips {
            let key = Arc::as_ptr(list);
            let made = clips
                .entry(key)
                .or_insert_with(|| {
                    let mut copy = (**list).clone();
                    for c in &mut copy {
                        c.path.points.iter_mut().for_each(one);
                    }
                    Arc::new(copy)
                })
                .clone();
            op.clips = Some(made);
        }
    }
    out
}

/// The oracle's stub backend: core_tests.cpp's fonts, canvas fonts numbered on their own, 8 px a byte of text, and
/// every call the renderer makes itself written down.
#[derive(Debug, Default)]
pub struct Stub {
    next_canvas_font: i32,
    pub log: Out,
    /// Each canvas run's ops, for the tests.
    pub runs: Vec<(i32, Vec<CanvasOp>)>,
    pub released: Vec<i32>,
    pub deleted: Vec<i32>,
    pub made: Vec<i32>,
    pub measured_in: Vec<i32>,
}

impl Stub {
    pub fn new() -> Stub {
        Stub { next_canvas_font: CANVAS_FONTS, ..Stub::default() }
    }
    fn metrics(size: f32, out: &mut FontMetrics) {
        out.ascent = size * 0.8;
        out.descent = size * 0.2;
        out.height = size * 1.2;
        out.x_height = size * 0.5;
        out.zero_width = 8.0;
    }
}

impl Backend for Stub {
    fn create_font(&mut self, _families: &[u8], size_px: f32, _weight: i32, _italic: bool, _features: &[u8], out: &mut FontMetrics) -> i32 {
        Stub::metrics(size_px, out);
        0
    }
    fn create_canvas_font(&mut self, families: &[u8], size_px: f32, weight: i32, italic: bool, out: &mut FontMetrics) -> i32 {
        Stub::metrics(size_px, out);
        self.next_canvas_font += 1;
        let handle = self.next_canvas_font;
        self.made.push(handle);
        self.log.u8(b'F');
        self.log.str(families);
        self.log.f32(size_px);
        self.log.i32(weight);
        self.log.flag(italic);
        self.log.i32(handle);
        handle
    }
    fn delete_font(&mut self, font: i32) {
        if font > CANVAS_FONTS {
            self.deleted.push(font);
            self.log.u8(b'D');
            self.log.i32(font);
        }
    }
    fn text_width(&mut self, font: i32, text: &[u8]) -> f32 {
        if font > CANVAS_FONTS {
            self.measured_in.push(font);
            self.log.u8(b'W');
            self.log.i32(font);
            self.log.str(text);
        }
        8.0 * text.len() as f32
    }
    fn load_image(&mut self, _id: i32, _url: &[u8], _w: &mut f32, _h: &mut f32) -> bool {
        false
    }
    fn canvas_run(&mut self, id: i32, w: i32, h: i32, ops: &[CanvasOp]) {
        let mut bytes = Vec::new();
        let encoded = encode_batch(&canonical(ops), &Caps::default(), &mut bytes);
        self.log.u8(b'R');
        self.log.i32(id);
        self.log.i32(w);
        self.log.i32(h);
        self.log.u32(ops.len() as u32);
        self.log.flag(encoded);
        self.log.u64(fnv64(&bytes));
        self.runs.push((id, ops.to_vec()));
    }
    fn canvas_read(&mut self, id: i32, x: i32, y: i32, w: i32, h: i32, _rgba: &mut Vec<u8>) -> bool {
        self.log.u8(b'G');
        for v in [id, x, y, w, h] {
            self.log.i32(v);
        }
        false
    }
    fn canvas_release(&mut self, id: i32) {
        self.released.push(id);
        self.log.u8(b'X');
        self.log.i32(id);
    }
}

/// The script's clock: each step's time.
#[derive(Debug)]
pub struct StepClock(pub Rc<Cell<f64>>);

impl Clock for StepClock {
    fn now_ms(&mut self) -> f64 {
        self.0.get()
    }
}

pub type Port = Renderer<TapeEngine, Stub, TapeLayers>;

// ---- The transcript -----------------------------------------------------------------------------------------

/// The input's state, as the oracle's RendererOracle::input writes the C++'s.
pub fn input_state(r: &mut Port, o: &mut Out) {
    let events = r.events();
    let doc = r.document();
    o.u32(events.touches().len() as u32);
    for (&pointer, t) in events.touches() {
        o.i32(pointer);
        o.f64(t.x0);
        o.f64(t.y0);
        o.f64(t.x);
        o.f64(t.y);
        o.i64(t.target);
        o.rect(&t.target_rect);
        o.flag(t.moved);
        o.flag(t.primary);
        o.i32(t.pointer);
        o.flag(t.mouse);
        o.i32(t.modifiers);
        o.i32(t.button);
        o.flag(t.held);
        o.i64(t.capture);
        o.f64(t.sent_x);
        o.f64(t.sent_y);
        o.flag(t.finger);
    }
    let (hover, hx, hy) = events.hover_state();
    o.i64(hover);
    o.f64(hx);
    o.f64(hy);
    o.i32(events.click_count());
    o.i32(events.arbiter_pointer());
    let deadline = events.hold_deadline(doc);
    o.flag(deadline.is_some());
    o.f64(deadline.unwrap_or(0.0));
    let (bubble, bubble_touch, rtl) = events.arbitration();
    o.flag(bubble);
    o.flag(bubble_touch);
    o.flag(rtl);
    o.u32(events.pending_moves().len() as u32);
    for (key, ev) in events.pending_moves() {
        o.str(key);
        o.str(&ev.dump());
    }
    for set in [events.interest(), events.prevented()] {
        let mut sorted: Vec<&Vec<u8>> = set.iter().collect();
        sorted.sort();
        o.u32(sorted.len() as u32);
        for k in sorted {
            o.str(k);
        }
    }
    o.str(&events.editor_json());
    o.str(events.editing_initial());
    o.u64(events.editor_generation());
    let s = events.selection();
    o.i64(s.anchor.owner);
    o.u64(s.anchor.offset as u64);
    o.i64(s.focus.owner);
    o.u64(s.focus.offset as u64);
    o.i32(s.pointer);
    o.f64(events.last_input);
    o.u32(events.resize_watch().len() as u32);
    for (&id, &(w, h)) in events.resize_watch() {
        o.i64(id);
        o.f32(w);
        o.f32(h);
    }
    let mut waiting: Vec<(&i32, &Vec<i64>)> = events.waiting_images().iter().collect();
    waiting.sort();
    o.u32(waiting.len() as u32);
    for (&image, ids) in waiting {
        o.i32(image);
        o.u32(ids.len() as u32);
        for &id in ids {
            o.i64(id);
        }
    }
    for k in events.runs_key() {
        o.u64(k);
    }
    o.u32(events.runs().len() as u32);
    for run in events.runs() {
        o.flag(run.drawn);
        o.i64(run.layer);
        for v in [run.shown.a, run.shown.b, run.shown.c, run.shown.d, run.shown.e, run.shown.f] {
            o.f32(v);
        }
        o.u32(run.advances.len() as u32);
    }
    let (key, selectable) = events.selectable_cache();
    o.u64(key[0]);
    o.u64(key[1]);
    o.flag(selectable);
}

/// What the builds, the reports, the animations and the canvas fonts left, as RendererOracle::build writes it.
pub fn build_state(r: &mut Port, o: &mut Out) {
    let theme = r.theme_variables_json();
    let master = fnv64(r.master_css());
    let (width, dpr, vp) = (r.width(), r.dpr(), r.viewport_height());
    let doc = r.document();
    let c = r.core();
    o.f64(width);
    o.f64(dpr);
    o.i32(vp);
    o.f64(c.doc_height());
    o.f64(c.max_height());
    o.flag(doc.dark());
    o.flag(r.dirty());
    o.flag(doc.stale);
    o.flag(doc.document_changed);
    let (html, html_stale) = c.html_state();
    o.flag(html_stale);
    o.u64(fnv64(html));
    o.u32(html.len() as u32);
    for v in c.last_size() {
        o.f64(v);
    }
    let (track, prefers) = c.resize_tracking();
    o.flag(track);
    o.flag(prefers);
    let (edges, edges_height, edges_layouts) = c.edges();
    o.u32(edges.len() as u32);
    for (&id, e) in edges {
        o.i64(id);
        o.f32(e.top);
        o.f32(e.bottom);
    }
    o.f64(edges_height);
    o.u64(edges_layouts);
    let (has_resize, d, resize_at, serial, reported) = c.resize_state();
    o.flag(has_resize);
    o.f64(d.old_height);
    o.f64(d.new_height);
    o.f64(d.unchanged_top);
    o.f64(d.unchanged_bottom);
    o.flag(d.has_pointer);
    o.f64(d.pointer_from);
    o.f64(d.pointer_to);
    o.flag(d.keeps_bottom);
    o.f64(resize_at);
    o.u64(serial);
    o.u64(reported);
    let (press_id, press_y, press_at) = c.press();
    o.i64(press_id);
    o.f64(press_y);
    o.f64(press_at);
    let log = &doc.patch_log;
    o.flag(log.valid);
    o.u32(log.attributes.len() as u32);
    for (r, names) in log.attributes.values() {
        o.i64(doc.get(*r).id);
        o.u32(names.len() as u32);
        for name in names {
            o.str(name);
        }
    }
    o.u32(log.texts.len() as u32);
    for r in log.texts.values() {
        o.i64(doc.get(*r).id);
    }
    let (touched, touched_all) = c.touched();
    o.u32(touched.len() as u32);
    for &id in touched {
        o.i64(id);
    }
    o.flag(touched_all);
    o.flag(c.patching());
    let s = r.stats();
    o.f64(s.serialize_ms);
    o.f64(s.parse_ms);
    o.i32(s.passes);
    o.u64(s.html_bytes as u64);
    for v in [s.builds, s.patched, s.throttled, s.in_place, s.scripted, s.reused] {
        o.i32(v);
    }
    o.u64(s.names);
    o.u64(s.name_bytes);
    o.u32(doc.pending_violations.len() as u32);
    for v in &doc.pending_violations {
        o.str(v);
    }
    o.flag(doc.frozen());
    o.str(doc.frozen_reason());
    o.flag(doc.frozen_fatal());
    o.flag(doc.layout_stopped());
    o.flag(doc.paint_stopped);
    o.str(&theme);
    o.u64(master);
    let k = c.container();
    o.flag(k.has_document());
    o.i32(k.vp_height());
    o.f32(k.rendered_width());
    o.flag(k.viewport_units());
    o.flag(k.patched());
    o.u64(k.layouts());
    o.u64(k.built_fonts() as u64);
    o.u32(c.overrides().len() as u32);
    for (&id, values) in c.overrides() {
        o.i64(id);
        o.u32(values.len() as u32);
        for (k, v) in values {
            o.str(k);
            o.str(v);
        }
    }
    o.u64(fnv64(c.sheets_key()));
    o.u64(c.declared_generation());
    o.flag(c.animating());
    o.u32(c.pending_events().len() as u32);
    for m in c.pending_events() {
        o.str(&m.dump());
    }
    let (fonts, order) = c.canvas_fonts();
    let sorted: BTreeMap<&Vec<u8>, i32> = fonts.iter().map(|(k, v)| (k, v.0)).collect();
    o.u32(sorted.len() as u32);
    for (font, handle) in sorted {
        o.str(font);
        o.i32(handle);
    }
    o.u32(order.len() as u32);
    for &h in order {
        o.i32(h);
    }
    let (first, until, charges) = c.throttle();
    o.f64(first);
    o.f64(until);
    o.u32(charges as u32);
    let (meter, names) = c.meters();
    o.i32(meter);
    o.i32(names);
    o.flag(c.scripted_pending());
    o.u32(doc.canvases().len() as u32);
    for (&node, canvas) in doc.canvases() {
        o.i64(node);
        o.i32(canvas.id());
        o.i32(canvas.width());
        o.i32(canvas.height());
    }
}

/// The tree's nodes up to `max_id`: kind, tag, attributes and data, as RendererOracle::tree digests them.
pub fn tree(doc: &twidget_shadow::Document, max_id: u32) -> u64 {
    let mut h = 1469598103934665603u64;
    for id in 1..=max_id {
        let Some(r) = doc.node(i64::from(id)) else { continue };
        let n = doc.get(r);
        let mut b = Vec::new();
        b.extend_from_slice(&id.to_le_bytes());
        b.push(match n.kind {
            twidget_shadow::NodeKind::Element => 0,
            twidget_shadow::NodeKind::Text => 1,
            twidget_shadow::NodeKind::Comment => 2,
        });
        b.extend_from_slice(&n.tag);
        b.push(0);
        for (name, value) in &n.attrs {
            b.extend_from_slice(name);
            b.push(b'=');
            b.extend_from_slice(value);
            b.push(0);
        }
        b.push(1);
        b.extend_from_slice(&n.data);
        h = fnv64_from(h, &b);
    }
    h
}

fn messages(out: &[Json], o: &mut Out, tally: &mut Tally) {
    o.u32(out.len() as u32);
    for m in out {
        let t = m.get("type").as_str();
        match t {
            b"result" => {
                let q = m.get("result");
                if q.find("error").is_some() {
                    tally.add(format!("result error {}", String::from_utf8_lossy(q.get("error").as_str())));
                }
            }
            b"violation" => {
                let detail = m.get("detail").as_str();
                let head = detail.split(|&c| c == b':').next().unwrap_or(b"");
                tally.add(format!("violation {}", String::from_utf8_lossy(head)));
            }
            b"event" => tally.add(format!("event {}", String::from_utf8_lossy(m.get("ev").get("type").as_str()))),
            _ => tally.add(format!("message {}", String::from_utf8_lossy(t))),
        }
        o.str(&m.dump());
    }
}

/// What one run of a script through the port gives: its transcript, what went wrong against the tape, and what the
/// tests look at.
#[derive(Debug, Default)]
pub struct Run {
    pub transcript: Vec<u8>,
    pub errors: Vec<String>,
    /// Each step's messages.
    pub outs: Vec<Vec<Json>>,
    /// Each step's returned bytes (the transcript's `extra`).
    pub extras: Vec<Vec<u8>>,
    /// The tree's HTML after each last_html step.
    pub html: Vec<Vec<u8>>,
    /// Each step's stats and viewport height, after it.
    pub stats: Vec<twidget_renderer::BuildStats>,
    pub viewports: Vec<i32>,
    pub layouts: Vec<u64>,
    pub frozen: Vec<(bool, Vec<u8>, bool)>,
    pub wants_tick: Vec<bool>,
    /// The backend's own calls the renderer made, in order, for the tests: (canvas releases, fonts deleted, fonts made,
    /// fonts measured in, canvas runs).
    pub stub: Stub,
}

/// The query step's result in a run's messages.
pub fn result(out: &[Json]) -> Json {
    out.iter().find(|m| m.get("type").as_str() == b"result").map(|m| m.get("result").clone()).unwrap_or(Json::Null)
}

/// The script through the port, on the tape the oracle recorded of the C++ running it.
pub fn run(script: &Script, tape_bytes: &[u8], tally: &mut Tally) -> Run {
    let entries = match tape::parse(tape_bytes) {
        Ok(e) => e,
        Err(e) => return Run { errors: vec![format!("the tape does not parse: {e}")], ..Run::default() },
    };
    let shared = Rc::new(RefCell::new(Shared { entries: VecDeque::from(entries), ..Shared::default() }));
    let clock = Rc::new(Cell::new(script.t0));
    let mut r: Port = Renderer::new(
        script.width,
        script.dpr,
        script.dark,
        TapeEngine { tape: shared.clone() },
        Stub::new(),
        TapeLayers::new(shared.clone()),
        Some(Box::new(StepClock(clock.clone()))),
    );
    let mut run = Run::default();
    let mut o = Out::default();
    let log = std::mem::take(&mut r.core_mut().backend.log);
    o.str(&log.0);
    build_state(&mut r, &mut o);
    for (i, (t, step)) in script.steps.iter().enumerate() {
        clock.set(*t);
        shared.borrow_mut().step = i;
        match shared.borrow_mut().entries.pop_front() {
            Some(Entry::Step(n)) if n as usize == i => {}
            other => {
                shared.borrow_mut().error(format!("step {i} ({}): expected its start, got {other:?}", step.name()));
                break;
            }
        }
        tally.add(format!("step {}", step.name()));
        o.u8(step.kind());
        let mut out = Vec::new();
        let mut extra = Out::default();
        match step {
            Step::Ops(text) | Step::Message(text) => {
                let parsed = Json::parse(text);
                extra.flag(parsed.is_some());
                if let Some(parsed) = parsed {
                    let msg = if matches!(step, Step::Ops(_)) {
                        let mut m = Json::object();
                        m.set("type", "ops").set("ops", parsed);
                        m
                    } else {
                        parsed
                    };
                    tally.add(format!("message in {}", String::from_utf8_lossy(msg.get("type").as_str())));
                    if msg.get("type").as_str() == b"query" {
                        tally.add(format!("query {}", String::from_utf8_lossy(msg.get("query").get("q").as_str())));
                    }
                    out = r.receive(&msg);
                }
            }
            Step::Dark(d) => r.set_theme(*d, &mut out),
            Step::Width(w) => r.set_width(*w, &mut out),
            Step::MaxHeight(h) => r.set_max_height(*h, &mut out),
            Step::Compositing(c) => r.set_compositing(*c),
            Step::Touch { action, x, y, pointer, primary, mouse, modifiers, button } => {
                out = r.touch(*action, *x, *y, *pointer, *primary, *mouse, *modifiers, *button);
            }
            Step::Hover { action, x, y, modifiers } => out = r.hover(*action, *x, *y, *modifiers),
            Step::Claim { x, y } => {
                let claim = r.claim_at(*x, *y);
                extra.u8(u8::from(claim.x) | (u8::from(claim.y) << 1) | (u8::from(claim.hold) << 2));
            }
            Step::Edit { current, generation, text, kind } => {
                let generation = if *current { r.editor().map_or(0, |e| e.generation) } else { *generation };
                out = r.edit(generation, text, kind);
            }
            Step::EndEditing => out = r.end_editing(),
            Step::ClearSelection => {
                let had = r.clear_selection();
                extra.flag(had);
            }
            Step::ImageReady { id, w, h } => out = r.image_ready(*id, *w, *h),
            Step::Wheel { x, y, dx, dy, mode, modifiers } => out = r.wheel(*x, *y, *dx, *dy, *mode, *modifiers),
            Step::Key { kind, key, code, repeat, modifiers } => out = r.key(kind, key, code, *repeat, *modifiers),
            Step::Tick => out = r.tick(),
            Step::Paint | Step::LastHtml => {
                let html = r.last_html().to_vec();
                extra.str(&html);
                run.html.push(html);
            }
            Step::Observe => {
                let cursor = r.cursor();
                let has = r.has_selection();
                let text = r.selected_text();
                let json = r.selectable_text_json();
                extra.str(&cursor);
                extra.flag(has);
                extra.str(&text);
                extra.str(&json);
            }
            Step::Trim => r.trim(),
            Step::ResizeTracking { enabled, prefers_bottom } => r.set_resize_tracking(*enabled, *prefers_bottom),
            Step::Patching(p) => r.set_patching(*p),
            Step::Inspect => {
                extra.flag(r.wants_tick());
                extra.f64(r.next_tick_in());
                extra.str(r.master_css());
                extra.str(&r.theme_variables_json());
                extra.str(&r.editor_json());
                extra.u64(r.layouts());
                extra.u32(r.px_width());
                extra.u32(r.px_height());
            }
            Step::Charge(seconds) => r.core_mut().charge(*seconds),
        }
        messages(&out, &mut o, tally);
        o.0.extend_from_slice(&extra.0);
        let log = std::mem::take(&mut r.core_mut().backend.log);
        o.str(&log.0);
        input_state(&mut r, &mut o);
        build_state(&mut r, &mut o);
        o.u64(tree(r.document(), script.max_id));
        run.extras.push(extra.0);
        run.stats.push(r.stats().clone());
        run.viewports.push(r.viewport_height());
        run.layouts.push(r.layouts());
        let doc = r.document();
        run.frozen.push((doc.frozen(), doc.frozen_reason().to_vec(), doc.layout_stopped()));
        run.outs.push(out);
        if !shared.borrow().errors.is_empty() {
            break;
        }
    }
    {
        let mut s = shared.borrow_mut();
        if s.errors.is_empty()
            && let Some(e) = s.entries.front().cloned()
        {
            s.error(format!("the tape goes on past the script: {e:?}"));
        }
    }
    run.wants_tick.push(r.wants_tick());
    let mut s = shared.borrow_mut();
    tally.merge(std::mem::take(&mut s.tally));
    run.errors = std::mem::take(&mut s.errors);
    drop(s);
    run.transcript = o.0;
    let stub = std::mem::replace(&mut r.core_mut().backend, Stub::new());
    run.stub = stub;
    run
}

/// An oracle answer: the tape, then the transcript.
pub fn split(answer: &[u8]) -> (&[u8], &[u8]) {
    let n = answer.get(..4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap_or([0; 4])) as usize);
    let n = n.min(answer.len().saturating_sub(4));
    (&answer[4..4 + n], &answer[4 + n..])
}
