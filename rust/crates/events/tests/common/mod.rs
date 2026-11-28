//! What the events comparison shares with `oracle/events_oracle`: scripts of messages and input through one renderer,
//! the tape of the C++ document's answers the port replays, the transcript both sides write after each step, and the
//! inputs (`.twrec` sessions, corpus widgets, generated documents and gestures).
//!
//! A script is 'S', a u32 length and: f64 width, f64 dpr, u8 dark, f64 the clock at construction (ms), u32 the largest
//! node id the tree digest walks to, u32 steps, each a u8 kind, f64 the clock (ms) and its fields ([`Step`]).
//!
//! The answer is status 1 and: u32 tape length, the tape, the transcript. The tape is what renderer.cpp's input code
//! asked of the rest of the renderer, in order, as the probes of `oracle/events_probe.h` wrote it ([`tape`]); the
//! transcript is, per step, its kind, the messages it gave but `size` and `result` (u32 count, each its JSON), what
//! the step returned, and the input's state after it ([`state`]).

#![allow(dead_code)]

pub mod cases;
pub mod inputs;
pub mod tape;
pub mod targeted;

use std::collections::VecDeque;
use std::fmt::Write as _;

use twidget_events::*;
use twidget_json::Json;
use twidget_shadow::{Document, DocumentHost, NodeKind};
pub use twidget_testkit::Fields;
use twidget_wire::{CanvasOp, Caps, FontMetrics, decode_list, encode_list};

pub use tape::{Entry, Sync};

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

/// One call into the renderer.
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
    Paint,
    LastHtml,
    /// The selection and cursor as a host reads them: cursor(), has_selection(), selected_text(), selectable_text_json().
    Observe,
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
            Step::Paint => b'p',
            Step::LastHtml => b'l',
            Step::Observe => b'q',
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
            Step::Paint => "paint",
            Step::LastHtml => "last_html",
            Step::Observe => "observe",
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

    pub fn record(&self) -> Vec<u8> {
        let mut f = Fields::new();
        f.f64(self.width).f64(self.dpr).flag(self.dark).f64(self.t0).u32(self.max_id).u32(self.steps.len() as u32);
        for (t, step) in &self.steps {
            f.u8(step.kind()).f64(*t);
            match step {
                Step::Ops(text) | Step::Message(text) => {
                    f.str(text);
                }
                Step::Dark(d) | Step::Compositing(d) => {
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
                Step::EndEditing | Step::ClearSelection | Step::Tick | Step::Paint | Step::LastHtml | Step::Observe => {}
            }
        }
        f.framed(b'S')
    }
}

/// What the tallies count.
#[derive(Debug, Default)]
pub struct Tally(pub std::collections::BTreeMap<String, usize>);

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

// ---- The renderer around the port, on the tape ----------------------------------------------------------

/// The tree's calls into the rest of the renderer: canvases are not the input's (stage 4.1 compares them), the input
/// takes what is its own.
pub struct Bridge<'a> {
    pub events: &'a mut Events,
    pub host: &'a mut tape::TapeHost,
}

impl DocumentHost for Bridge<'_> {
    fn canvas_run(&mut self, _canvas: i32, _width: i32, _height: i32, _ops: &[CanvasOp]) {}
    fn canvas_release(&mut self, _canvas: i32) {}
    fn canvas_font(&mut self, _css: &[u8], _metrics: &mut FontMetrics) -> i32 {
        0
    }
    fn canvas_text_width(&mut self, _font: i32, text: &[u8]) -> f32 {
        8.0 * text.len() as f32
    }
    fn image_of(&mut self, _src: &[u8]) -> Option<(i32, f32, f32)> {
        None
    }
    fn node_forgotten(&mut self, id: i64) {
        self.events.node_forgotten(id);
    }
    fn value_set(&mut self, id: i64, value: &[u8]) {
        self.events.value_set(id, value);
    }
    fn call(&mut self, document: &mut Document, id: i64, method: &[u8], args: &Json, out: &mut Vec<Json>) {
        self.events.call(document, self.host, id, method, args, out);
    }
    fn frozen(&mut self) {
        self.events.on_frozen();
    }
}

/// The renderer's state the tape gives at a step or a marker: what builds, stops and freezes left, which the port
/// follows here as the renderer would have told it.
fn follow(doc: &mut Document, events: &mut Events, host: &mut tape::TapeHost, s: &Sync) {
    let was = doc.paint_stopped;
    let mut bridge = Bridge { events, host };
    tape::apply(doc, s, &mut bridge);
    if !was && doc.paint_stopped {
        bridge.events.drop_moves();
    }
}

/// The markers the tape holds where the rest of the renderer called the input's code itself.
fn drain(doc: &mut Document, events: &mut Events, host: &mut tape::TapeHost, out: &mut Vec<Json>, tally: &mut Tally) {
    loop {
        match host.peek() {
            Some(Entry::Marker(tag, s)) => {
                let (tag, s) = (*tag, s.clone());
                host.pop();
                follow(doc, events, host, &s);
                tally.add(format!("marker {}", tag as char));
                match tag {
                    b'E' => events.refresh_editor(doc, host),
                    b'F' => events.flush_pending(doc, host, out),
                    b'M' => events.flush_moves(out),
                    b'H' => events.check_hold(doc, host, out),
                    b'G' => events.load_images(doc, host, out),
                    b'D' => events.paint_begins(host),
                    _ => host.error(format!("unknown marker {tag}")),
                }
            }
            Some(Entry::Paint { .. }) => {
                let Some(Entry::Paint { sync, layer, tint, list }) = host.pop() else { unreachable!() };
                follow(doc, events, host, &sync);
                tally.add("marker Q");
                let Some(mut decoded) = decode_list(&list, &Caps::default()) else {
                    host.error("a painted list that does not decode".into());
                    continue;
                };
                let theme = doc.theme_variables().clone();
                let before = decoded.len();
                events.paint_selection(host, &theme, &mut decoded, layer, tint);
                if decoded.len() > before {
                    tally.add_n("tints painted", decoded.len() - before);
                }
                let mut bytes = Vec::new();
                let encoded = encode_list(&decoded, &Caps::default(), &mut bytes);
                host.painted.push(decoded.clone());
                match host.pop() {
                    Some(Entry::PaintOut { encoded: e, len, digest }) => {
                        if e != encoded || len as usize != decoded.len() || digest != fnv64(&bytes) {
                            host.error(format!(
                                "paint_selection on layer {layer}: {len} commands in C++, {} in Rust, or other ones",
                                decoded.len()
                            ));
                        }
                    }
                    other => host.error(format!("paint_selection's list: expected its digest, got {other:?}")),
                }
            }
            _ => return,
        }
    }
}

fn messages(out: &[Json], o: &mut Out, tally: &mut Tally) {
    let kept: Vec<&Json> = out.iter().filter(|m| !matches!(m.get("type").as_str(), b"size" | b"result")).collect();
    o.u32(kept.len() as u32);
    for m in kept {
        let t = m.get("type").as_str();
        if t == b"event" {
            let ev = m.get("ev").get("type").as_str();
            tally.add(format!("event {}", String::from_utf8_lossy(ev)));
            if !m.get("trusted").as_bool(true) {
                tally.add("event untrusted");
            }
        } else if t == b"gesture" {
            let claim = (m.get("x").as_bool(false), m.get("hold").as_bool(false));
            tally.add(format!("claim x={} hold={}", claim.0, claim.1));
        } else if t == b"bubble" {
            tally.add(format!(
                "bubble {} {}",
                String::from_utf8_lossy(m.get("action").as_str()),
                String::from_utf8_lossy(m.get("gesture").as_str())
            ));
        } else {
            tally.add(format!("message {}", String::from_utf8_lossy(t)));
        }
        o.str(&m.dump());
    }
}

/// The input's state after a step, as the oracle's EventsOracle::state writes the C++'s.
pub fn state(doc: &Document, events: &Events, o: &mut Out, max_id: u32) {
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
    for r in events.runs() {
        o.flag(r.drawn);
        o.i64(r.layer);
        for v in [r.shown.a, r.shown.b, r.shown.c, r.shown.d, r.shown.e, r.shown.f] {
            o.f32(v);
        }
        o.u32(r.advances.len() as u32);
    }
    let (key, selectable) = events.selectable_cache();
    o.u64(key[0]);
    o.u64(key[1]);
    o.flag(selectable);
    o.u64(tree(doc, max_id));
}

/// The tree's nodes up to `max_id`: kind, tag, attributes and data, as EventsOracle::tree digests them.
pub fn tree(doc: &Document, max_id: u32) -> u64 {
    let mut h = 1469598103934665603u64;
    for id in 1..=max_id {
        let Some(r) = doc.node(i64::from(id)) else { continue };
        let n = doc.get(r);
        let mut b = Vec::new();
        b.extend_from_slice(&id.to_le_bytes());
        b.push(match n.kind {
            NodeKind::Element => 0,
            NodeKind::Text => 1,
            NodeKind::Comment => 2,
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

/// What an observe step read: cursor(), has_selection(), selected_text(), selectable_text_json().
pub type Observed = (Vec<u8>, bool, Vec<u8>, Vec<u8>);

/// What one run of a script through the port gives: its transcript, what went wrong against the tape, and the
/// results the tests look at.
#[derive(Debug, Default)]
pub struct Run {
    pub transcript: Vec<u8>,
    pub errors: Vec<String>,
    /// Each step's messages.
    pub outs: Vec<Vec<Json>>,
    /// Each observe step's answers.
    pub observed: Vec<Observed>,
    pub claims: Vec<twidget_gesture::WidgetGestureClaim>,
    pub cleared: Vec<bool>,
    /// The editor after each step.
    pub editors: Vec<Option<EditorSpec>>,
    /// The tree's HTML after each last_html step.
    pub html: Vec<Vec<u8>>,
    /// The lists paint_selection left in each step.
    pub painted: Vec<Vec<Vec<twidget_wire::Command>>>,
    /// The coalesced moves waiting after each step.
    pub pending: Vec<usize>,
}

fn parse_ops(text: &[u8]) -> Option<Json> {
    Json::parse(text)
}

/// The script through the port, on the tape the oracle recorded of the C++ running it.
pub fn run(script: &Script, tape_bytes: &[u8], tally: &mut Tally) -> Run {
    let entries = match tape::parse(tape_bytes) {
        Ok(e) => e,
        Err(e) => return Run { errors: vec![format!("the tape does not parse: {e}")], ..Run::default() },
    };
    let mut host = tape::TapeHost::new(VecDeque::from(entries), script.t0);
    let mut doc = Document::new(script.dark);
    let mut events = Events::new();
    let mut run = Run::default();
    let mut o = Out::default();
    // The renderer as made and its `policy` message.
    let mut ignored = Vec::new();
    drain(&mut doc, &mut events, &mut host, &mut ignored, tally);
    for (t, step) in &script.steps {
        host.now = *t;
        match host.pop() {
            Some(Entry::Step(s)) => follow(&mut doc, &mut events, &mut host, &s),
            other => {
                host.error(format!("step {} ({}): expected its start, got {other:?}", run.outs.len(), step.name()));
                break;
            }
        }
        tally.add(format!("step {}", step.name()));
        o.u8(step.kind());
        let mut out = Vec::new();
        let mut extra = Out::default();
        match step {
            Step::Ops(text) | Step::Message(text) => {
                if let Some(parsed) = parse_ops(text) {
                    let msg = if matches!(step, Step::Ops(_)) {
                        let mut m = Json::object();
                        m.set("type", "ops").set("ops", parsed);
                        m
                    } else {
                        parsed
                    };
                    let mut bridge = Bridge { events: &mut events, host: &mut host };
                    match msg.get("type").as_str() {
                        b"ops" => doc.receive_ops(msg.get("ops").arr(), &mut bridge, &mut out),
                        b"listen" | b"preventDefault" | b"arbitrate" => bridge.events.receive(&msg),
                        b"theme" => {
                            doc.receive_theme(msg.get("theme"));
                        }
                        b"safeArea" => {
                            doc.apply_safe_area(msg.get("inset"));
                        }
                        b"frozen" => doc.freeze(msg.get("reason").as_str(), msg.get("fatal").as_bool(false), &mut bridge),
                        _ => {}
                    }
                }
            }
            Step::Dark(d) => {
                doc.set_dark(*d);
            }
            Step::Width(_) | Step::MaxHeight(_) | Step::Compositing(_) | Step::Tick | Step::Paint | Step::LastHtml => {}
            Step::Touch { action, x, y, pointer, primary, mouse, modifiers, button } => {
                out = events.touch(&mut doc, &mut host, *action, *x, *y, *pointer, *primary, *mouse, *modifiers, *button);
            }
            Step::Hover { action, x, y, modifiers } => out = events.hover(&mut doc, &mut host, *action, *x, *y, *modifiers),
            Step::Claim { x, y } => {
                let claim = events.claim_at(&mut doc, &mut host, *x, *y);
                run.claims.push(claim);
                extra.u8(u8::from(claim.x) | (u8::from(claim.y) << 1) | (u8::from(claim.hold) << 2));
            }
            Step::Edit { current, generation, text, kind } => {
                let generation = if *current { events.editor().map_or(0, |e| e.generation) } else { *generation };
                tally.add("editor edit");
                out = events.edit(&mut doc, &mut host, generation, text, kind);
            }
            Step::EndEditing => out = events.end_editing(&mut doc, &mut host),
            Step::ClearSelection => {
                let had = events.clear_selection(&mut host);
                run.cleared.push(had);
                extra.flag(had);
            }
            Step::ImageReady { id, w, h } => out = events.image_ready(&mut doc, &mut host, *id, *w, *h),
            Step::Wheel { x, y, dx, dy, mode, modifiers } => out = events.wheel(*x, *y, *dx, *dy, *mode, *modifiers),
            Step::Key { kind, key, code, repeat, modifiers } => out = events.key(kind, key, code, *repeat, *modifiers),
            Step::Observe => {
                let cursor = events.cursor(&doc, &mut host);
                let has = events.has_selection(&mut host);
                let text = events.selected_text(&mut host);
                let json = events.selectable_text_json(&mut host);
                extra.str(&cursor);
                extra.flag(has);
                extra.str(&text);
                extra.str(&json);
                if has {
                    tally.add("selection observed");
                }
                run.observed.push((cursor, has, text, json));
            }
        }
        drain(&mut doc, &mut events, &mut host, &mut out, tally);
        if matches!(step, Step::LastHtml) {
            let overrides = twidget_shadow::StyleOverrides::new();
            run.html.push(doc.serialize(&twidget_shadow::Chrome { overrides: &overrides, editing: events.editing_id() }));
        }
        messages(&out, &mut o, tally);
        o.0.extend_from_slice(&extra.0);
        state(&doc, &events, &mut o, script.max_id);
        run.editors.push(events.editor().cloned());
        run.painted.push(std::mem::take(&mut host.painted));
        run.pending.push(events.pending_moves().len());
        run.outs.push(out);
        if !host.errors.is_empty() {
            break;
        }
    }
    if host.errors.is_empty()
        && let Some(e) = host.peek()
    {
        host.error(format!("the tape goes on past the script: {e:?}"));
    }
    tally.merge(std::mem::take(&mut host.tally));
    if events.editor().is_some() {
        tally.add("ends editing");
    }
    run.errors = std::mem::take(&mut host.errors);
    run.transcript = o.0;
    run
}

/// An oracle answer: the tape, then the transcript.
pub fn split(answer: &[u8]) -> (&[u8], &[u8]) {
    let n = answer.get(..4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap_or([0; 4])) as usize);
    let n = n.min(answer.len().saturating_sub(4));
    (&answer[4..4 + n], &answer[4 + n..])
}
