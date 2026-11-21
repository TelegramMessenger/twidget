//! `tests/bench.cpp` on the Rust renderer (T9): the renderer on `twidget_document` under a busy widget, in the
//! scenarios of the Swift renderer's FrameCostBenchTests, sent as the same protocol ops (the same message strings),
//! through the same frame loop, on the same stub backend (text at 8 px a byte, nothing rastered), with the same report,
//! so that the two can be timed run for run.
//!
//! [`traced`] runs a scenario on a step clock (1000 ms, a 60th of a second more at each tick) and writes each call of
//! the loop as a line of hashes, as `oracle/bench_oracle` writes them for the C++: `<call>[ in=<message>][
//! out=<count>:<messages>][ <paint>] be=<Backend calls>:<their hash> L=<layouts> B=<builds> P=<patched> I=<in place>
//! S=<scripted> R=<reused> T=<throttled> N=<passes> H=<HTML bytes> M=<names> Y=<name bytes>`, where a paint is `n=<commands>
//! list=<encoded>:<its hash> damage=<hash> layers=<version>:<count>:<hash of the composition's JSON and contents>`; every
//! hash is FNV-1a 64 over little-endian fields. Alike traces show the two benches do the same work.

#![forbid(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use twidget_cstd::{atoi, format_fixed};
use twidget_document::{DocumentEngine, Process};
use twidget_encode::composition_json;
use twidget_ends::TextInFont;
use twidget_json::{Json, number};
use twidget_renderer::{Backend, BuildStats, Clock, Renderer};
use twidget_wire::{CanvasOp, Caps, DisplayList, FontMetrics, encode_batch, encode_list};

// ---- the scenarios ---------------------------------------------------------------------------------------------

/// What the bench sends: the setup batch, a batch per frame, and the queries sent after every frame's ops.
#[derive(Clone, Debug, Default)]
pub struct Scenario {
    pub setup: String,
    pub frames: Vec<String>,
    pub queries: Vec<String>,
    pub hover: bool,
}

fn quoted(s: &str) -> String {
    String::from_utf8(Json::from(s).dump()).expect("a string's JSON is UTF-8")
}

/// Ops of `body` as a message; each op a JSON object without its braces.
fn message(ops: &[String]) -> String {
    let mut out = String::from("{\"type\":\"ops\",\"ops\":[");
    for (i, op) in ops.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('{');
        out.push_str(op);
        out.push('}');
    }
    out + "]}"
}

fn el(id: i32, tag: &str, attrs: &str) -> String {
    format!("\"op\":\"el\",\"id\":{id},\"tag\":\"{tag}\",\"attrs\":[{attrs}]")
}
fn ins(id: i32, parent: i32) -> String {
    format!("\"op\":\"ins\",\"id\":{id},\"parent\":{parent},\"before\":null")
}
fn text(id: i32, data: &str) -> String {
    format!("\"op\":\"text\",\"id\":{id},\"data\":{}", quoted(data))
}
fn data(id: i32, value: &str) -> String {
    format!("\"op\":\"data\",\"id\":{id},\"data\":{}", quoted(value))
}
fn style(id: i32, name: &str, value: &str) -> String {
    format!("\"op\":\"style\",\"id\":{id},\"name\":\"{name}\",\"value\":{}", quoted(value))
}
fn attr(name: &str, value: &str) -> String {
    format!("[\"{name}\",{}]", quoted(value))
}

fn document(css: &str) -> Vec<String> {
    vec![
        el(2, "html", "") + ",\"adopt\":\"html\"",
        el(3, "head", "") + ",\"adopt\":\"head\"",
        el(1, "body", "") + ",\"adopt\":\"body\"",
        el(4, "style", ""),
        format!("\"op\":\"css\",\"id\":4,\"text\":{}", quoted(css)),
        ins(4, 3),
    ]
}

/// `snprintf("%.*f")`.
fn fixed(value: f64, precision: i32) -> String {
    format_fixed(value, precision)
}

fn busy(mode: &str, count: i32, frames: i32) -> Scenario {
    let mut s = Scenario::default();
    let mut ops = document(
        "body { margin: 0; font: 12px sans-serif; } .box { position: absolute; width: 18px; height: 18px; border-radius: 4px; background: #3a7; } \
         .label { position: absolute; left: 0; color: #222; } canvas { position: absolute; left: 200px; top: 0; width: 200px; height: 200px; }",
    );
    let mut next = 100;
    let (mut boxes, mut texts) = (Vec::new(), Vec::new());
    for i in 0..count {
        let id = next;
        next += 1;
        let place = format!("left:{}px;top:{}px", (i % 20) * 20, (i / 20) * 20);
        ops.push(el(id, "div", &(attr("class", "box") + "," + &attr("style", &place))));
        ops.push(ins(id, 1));
        boxes.push(id);
    }
    for i in 0..30 {
        let (id, t) = (next, next + 1);
        next += 2;
        ops.push(el(id, "div", &(attr("class", "label") + "," + &attr("style", &format!("top:{}px", i * 16)))));
        ops.push(ins(id, 1));
        ops.push(text(t, &format!("label {i}")));
        ops.push(ins(t, id));
        texts.push(t);
    }
    let canvas = next;
    ops.push(el(canvas, "canvas", &(attr("width", "200") + "," + &attr("height", "200"))));
    ops.push(ins(canvas, 1));
    s.setup = message(&ops);

    let drag = mode.starts_with("drag");
    let n = boxes.len();
    for f in 0..frames {
        let mut frame = Vec::new();
        let moving = (n as i32).min(if drag { 6 } else { 300 });
        for k in 0..moving {
            if drag {
                // The same few, in the last row where nothing paints over them, as a drag would have it.
                let id = boxes[n - 1 - k as usize % n];
                frame.push(style(id, "transform", &format!("translate({}px, {}px)", f % 13, (f + k) % 9)));
                continue;
            }
            let id = boxes[((k * 7 + f) % n as i32) as usize];
            let transform = format!("translate({}px, {}px) rotate({}deg)", f % 13, (f + k) % 9, (f * 3 + k) % 360);
            frame.push(style(id, "transform", &transform));
        }
        if mode != "transform" && !drag {
            for k in 0..20 {
                let colour = format!("rgb({}, {}, 120)", f % 255, k * 10);
                frame.push(style(boxes[((k * 31 + f) % n as i32) as usize], "background-color", &colour));
            }
            for (k, &t) in texts.iter().enumerate() {
                frame.push(data(t, &format!("label {k} frame {f}")));
            }
            frame.push(format!(
                "\"op\":\"canvas\",\"id\":{canvas},\"cmds\":[[\"s\",\"fillStyle\",\"#f80\"],[\"c\",\"fillRect\",{},10,40,40]]",
                f % 100
            ));
        }
        s.frames.push(message(&frame));
    }
    if mode != "transform" && !drag {
        for q in 0..3 {
            s.queries.push(format!("{{\"type\":\"query\",\"qid\":{q},\"query\":{{\"q\":\"rect\",\"id\":{}}}}}", boxes[q * 50 % n]));
        }
    }
    s.hover = mode == "mixed";
    s
}

struct Tile {
    id: i32,
    hx: f64,
    hy: f64,
    ph: f64,
    hue: i32,
}

fn swarm(count: i32, frames: i32) -> Scenario {
    let mut s = Scenario::default();
    let mut ops = document(
        "body { margin: 0; font: 14px sans-serif; } .card { padding: 14px 16px 16px; background: #fff; overflow: hidden; } .stage { position: relative; height: 260px; \
         border-radius: 14px; overflow: hidden; background: #eef0f6; border: 1px solid rgba(0,0,0,.08); } .t { position: absolute; left: 0; top: 0; width: 9px; \
         height: 9px; border-radius: 3px; } .orb { position: absolute; left: 0; top: 0; width: 92px; height: 92px; border-radius: 46px; background: \
         rgba(255,255,255,.16); border: 1px solid rgba(255,255,255,.35); box-shadow: 0 8px 30px rgba(0,0,0,.25); } .log { position: relative; height: 128px; \
         margin-top: 12px; overflow: auto; border-radius: 12px; background: #f2f2f7; } .sh { position: sticky; top: 0; display: flex; justify-content: \
         space-between; padding: 6px 10px; font-size: 11px; font-weight: 700; color: #6e6e73; background: #f2f2f7; } .r { display: flex; justify-content: \
         space-between; padding: 5px 10px; font-size: 12px; border-bottom: 1px solid rgba(0,0,0,.08); }",
    );
    ops.push(el(5, "div", &attr("class", "card")));
    ops.push(ins(5, 1));
    ops.push(el(6, "div", &attr("class", "stage")));
    ops.push(ins(6, 5));
    ops.push(el(7, "div", &attr("class", "log")));
    ops.push(ins(7, 5));
    ops.push(el(8, "div", &attr("class", "sh")));
    ops.push(ins(8, 7));
    ops.push(text(9, "Event Frame"));
    ops.push(ins(9, 8));

    let mut next = 100;
    let (w, h) = (366.0, 260.0);
    let cols = (f64::from(count) * w / h).sqrt().ceil() as i32;
    let rows = (count + cols - 1) / cols;
    let mut tiles = Vec::new();
    for i in 0..count {
        let (id, c, r, hue) = (next, i % cols, i / cols, 360 * i / count);
        next += 1;
        let colour = format!("background-color: hsl({hue}, 80%, 58%)");
        ops.push(el(id, "div", &(attr("class", "t") + "," + &attr("style", &colour))));
        ops.push(ins(id, 6));
        tiles.push(Tile {
            id,
            hx: (f64::from(c) + 0.5) * w / f64::from(cols) - 4.5,
            hy: (f64::from(r) + 0.5) * h / f64::from(rows) - 4.5,
            ph: f64::from(c) * 0.35 + f64::from(r) * 0.22,
            hue,
        });
    }
    let mut orbs = Vec::new();
    for _ in 0..3 {
        ops.push(el(next, "div", &attr("class", "orb")));
        ops.push(ins(next, 6));
        orbs.push(next);
        next += 1;
    }
    let mut logs = Vec::new();
    for i in 0..40 {
        let (row, a, at, b, bt) = (next, next + 1, next + 2, next + 3, next + 4);
        next += 5;
        ops.push(el(row, "div", &attr("class", "r")));
        ops.push(ins(row, 7));
        ops.push(el(a, "span", ""));
        ops.push(ins(a, row));
        ops.push(text(at, &format!("tile {} re-coloured", i * 37 % 800)));
        ops.push(ins(at, a));
        ops.push(el(b, "span", ""));
        ops.push(ins(b, row));
        ops.push(text(bt, &format!("#{i}")));
        ops.push(ins(bt, b));
        logs.push((at, bt));
    }
    s.setup = message(&ops);

    let log_max = 40.0 * 24.0 + 24.0 - 128.0;
    let n = tiles.len() as i32;
    let mut px = vec![0.0f64; tiles.len()];
    let mut py = vec![0.0f64; tiles.len()];
    let mut hues: Vec<i32> = tiles.iter().map(|t| t.hue).collect();
    let mut orb = [0.0f64, 2.1, 4.2];
    for f in 1..=frames {
        let t = f64::from(f) / 60.0;
        let mut frame = Vec::new();
        for (i, p) in tiles.iter().enumerate() {
            let x = p.hx + (t * 1.3 + p.ph * 0.7).cos() * 6.0;
            let y = p.hy + (t * 2.2 + p.ph).sin() * 14.0;
            let sc = 0.75 + 0.35 * (t * 3.0 + p.ph).sin();
            let rot = (t + p.ph).sin() * 40.0;
            px[i] += (x - px[i]) * 0.35;
            py[i] += (y - py[i]) * 0.35;
            let transform =
                format!("translate({}px, {}px) rotate({}deg) scale({})", fixed(px[i], 1), fixed(py[i], 1), fixed(rot, 1), fixed(sc, 2));
            frame.push(style(p.id, "transform", &transform));
        }
        for k in 0..16 {
            let i = ((f * 16 + k * 97) % n) as usize;
            hues[i] = (hues[i] + 47) % 360;
            frame.push(style(tiles[i].id, "background-color", &format!("hsl({}, 80%, 58%)", hues[i])));
        }
        for (i, o) in orb.iter_mut().enumerate() {
            *o += 0.012 * (1.0 + i as f64 * 0.4);
            let transform = format!(
                "translate({}px, {}px) scale({})",
                fixed(w / 2.0 - 46.0 + o.cos() * w * 0.33, 1),
                fixed(h / 2.0 - 46.0 + (*o * 1.3).sin() * 70.0, 1),
                fixed(0.8 + 0.25 * (*o * 2.0).sin(), 2)
            );
            frame.push(style(orbs[i], "transform", &transform));
        }
        if f % 12 == 0 {
            let row = logs[((f / 12) % logs.len() as i32) as usize];
            frame.push(data(row.0, &format!("tile {} re-coloured", (f * 16) % n)));
            frame.push(data(row.1, &format!("#{f}")));
        }
        frame.push(format!("\"op\":\"prop\",\"id\":7,\"name\":\"scrollTop\",\"value\":{}", number((f64::from(f) * 0.6) % log_max)));
        s.frames.push(message(&frame));
    }
    s
}

/// A chart a script redraws on a canvas every frame: `count` points as line segments, bars and labels.
fn canvas_chart(count: i32, frames: i32) -> Scenario {
    let mut s = Scenario::default();
    let mut ops = document("body { margin: 0; } canvas { width: 400px; height: 300px; }");
    ops.push(el(100, "canvas", &(attr("width", "800") + "," + &attr("height", "600"))));
    ops.push(ins(100, 1));
    s.setup = message(&ops);
    for f in 0..frames {
        let mut cmds = String::from(
            "[\"c\",\"clearRect\",0,0,800,600],[\"s\",\"lineWidth\",2],[\"s\",\"strokeStyle\",\"#3a7\"],[\"c\",\"beginPath\"]",
        );
        for i in 0..count {
            let x = 800.0 * f64::from(i) / f64::from(count);
            let y = 300.0 + 200.0 * (f64::from(i + f) * 0.05).sin();
            let _ = write!(cmds, ",[\"c\",\"{}\",{},{}]", if i > 0 { "lineTo" } else { "moveTo" }, number(x), number(y));
        }
        cmds += ",[\"c\",\"stroke\"],[\"s\",\"fillStyle\",\"#f80\"]";
        for i in 0..count / 10 {
            // The height is an int negated before it is a double: never -0.
            let height = f64::from(-((i * 37 + f) % 200));
            let _ =
                write!(cmds, ",[\"c\",\"fillRect\",{},580,6,{}]", number(800.0 * f64::from(i) * 10.0 / f64::from(count)), number(height));
        }
        cmds += ",[\"s\",\"fillStyle\",\"#222\"],[\"s\",\"font\",\"12px sans-serif\"]";
        for i in 0..10 {
            let _ = write!(cmds, ",[\"c\",\"fillText\",\"{}\",{},20]", i * 10 + f % 10, number(f64::from(i * 80)));
        }
        s.frames.push(message(&[format!("\"op\":\"canvas\",\"id\":100,\"cmds\":[{cmds}]")]));
    }
    s
}

/// CSS animations the core runs itself, composited: frames are ticks, with no ops at all. Rows of spinners, every
/// tenth row with a pulsing background behind it.
fn css_animations(count: i32) -> Scenario {
    let mut s = Scenario::default();
    let mut ops = document(
        "body { margin: 0; } @keyframes spin { to { transform: rotate(360deg) } } @keyframes glow { to { background-color: #fc0 } } \
         .row { padding: 2px; } .glow { animation: glow 2s infinite alternate; } \
         .s { display: inline-block; width: 10px; height: 10px; margin: 4px; background: #39f; animation: spin 1s linear infinite; }",
    );
    let (mut next, mut row) = (100, 0);
    for i in 0..count {
        if i % 20 == 0 {
            row = next;
            next += 1;
            ops.push(el(row, "div", &attr("class", if (i / 20) % 10 == 0 { "row glow" } else { "row" })));
            ops.push(ins(row, 1));
        }
        let id = next;
        next += 1;
        ops.push(el(id, "div", &(attr("class", "s") + "," + &attr("style", &format!("animation-delay:-{}0ms", i % 17)))));
        ops.push(ins(id, row));
    }
    s.setup = message(&ops);
    s
}

/// bench.cpp's arguments: `[mixed|transform|swarm|css|drag|drag-doc|canvas] [count] [frames]`, the numbers as `atoi`
/// reads them.
#[derive(Clone, Debug)]
pub struct Options {
    pub mode: String,
    pub count: i32,
    pub frames: i32,
}

impl Options {
    pub fn parse(args: &[String]) -> Options {
        let mode = args.first().cloned().unwrap_or_else(|| "mixed".to_string());
        let count = args.get(1).map_or(if mode == "swarm" { 800 } else { 1000 }, |a| atoi(a.as_bytes()));
        let frames = args.get(2).map_or(300, |a| atoi(a.as_bytes()));
        Options { mode, count, frames }
    }
}

pub fn scenario(o: &Options) -> Scenario {
    match o.mode.as_str() {
        "swarm" => swarm(o.count, o.frames),
        "css" => css_animations(o.count),
        "canvas" => canvas_chart(o.count, o.frames),
        _ => busy(&o.mode, o.count, o.frames),
    }
}

// ---- the loop --------------------------------------------------------------------------------------------------

/// bench.cpp's `StubBackend`: fonts with metrics from their size, text at 8 px a byte, no image, no raster. The
/// renderer and its document's container share one, as the C++'s do, so font handles count up across both.
#[derive(Clone, Debug, Default)]
pub struct StubBackend {
    next: Rc<Cell<i32>>,
}

impl Backend for StubBackend {
    fn create_font(&mut self, _families: &[u8], size: f32, _weight: i32, _italic: bool, _features: &[u8], out: &mut FontMetrics) -> i32 {
        out.ascent = size * 0.8;
        out.descent = size * 0.2;
        out.height = size * 1.2;
        out.x_height = size * 0.5;
        out.zero_width = 8.0;
        self.next.set(self.next.get() + 1);
        self.next.get()
    }
    fn delete_font(&mut self, _font: i32) {}
    fn text_width(&mut self, _font: i32, text: &[u8]) -> f32 {
        8.0 * text.len() as f32
    }
    fn load_image(&mut self, _id: i32, _url: &[u8], _w: &mut f32, _h: &mut f32) -> bool {
        false
    }
}

/// What bench.cpp's loop calls of its renderer: the renderer itself when timing, or the traced one.
pub trait Bench {
    fn receive(&mut self, msg: &Json) -> Vec<Json>;
    fn hover(&mut self, action: i32, x: f64, y: f64) -> Vec<Json>;
    fn tick(&mut self) -> Vec<Json>;
    fn dirty(&self) -> bool;
    fn paint(&mut self) -> DisplayList;
    fn set_compositing(&mut self, enabled: bool);
    fn stats(&self) -> &BuildStats;
    fn layouts(&self) -> u64;
    fn layers(&self) -> usize;
}

impl<B: Backend + Clone> Bench for Renderer<DocumentEngine<B>, B> {
    fn receive(&mut self, msg: &Json) -> Vec<Json> {
        Renderer::receive(self, msg)
    }
    fn hover(&mut self, action: i32, x: f64, y: f64) -> Vec<Json> {
        Renderer::hover(self, action, x, y, 0)
    }
    fn tick(&mut self) -> Vec<Json> {
        Renderer::tick(self)
    }
    fn dirty(&self) -> bool {
        Renderer::dirty(self)
    }
    fn paint(&mut self) -> DisplayList {
        Renderer::paint(self)
    }
    fn set_compositing(&mut self, enabled: bool) {
        Renderer::set_compositing(self, enabled)
    }
    fn stats(&self) -> &BuildStats {
        Renderer::stats(self)
    }
    fn layouts(&self) -> u64 {
        Renderer::layouts(self)
    }
    fn layers(&self) -> usize {
        self.composition().layers.len()
    }
}

fn parse(s: &str) -> Json {
    Json::parse(s).unwrap_or_else(|| {
        eprintln!("bad json");
        std::process::exit(2);
    })
}

/// bench.cpp's `main` after the renderer is made: the setup, the frames, and the three lines of its report.
pub fn run(o: &Options, scenario: &Scenario, r: &mut impl Bench) -> String {
    let frames = o.frames;
    // drag: a few elements a script moves, as scripted layers; drag-doc: the same, painted in the document.
    if o.mode == "css" || o.mode == "drag" {
        r.set_compositing(true);
    }
    let epoch = Instant::now();
    let now_us = || epoch.elapsed().as_secs_f64() * 1e6;
    let t = now_us();
    r.receive(&parse(&scenario.setup));
    r.paint();
    let setup = now_us() - t;

    let (mut receive, mut tick, mut paint, mut serialize, mut build) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let layouts = r.layouts();
    let (builds, patched, in_place) = (r.stats().builds, r.stats().patched, r.stats().in_place);
    let mut commands = 0usize;
    for f in 0..frames {
        let a = now_us();
        let before = r.stats().builds;
        if let Some(frame) = scenario.frames.get(f as usize) {
            r.receive(&parse(frame));
        }
        if r.stats().builds != before {
            serialize += r.stats().serialize_ms * 1000.0;
            build += r.stats().parse_ms * 1000.0;
        }
        for q in &scenario.queries {
            r.receive(&parse(q));
        }
        if scenario.hover {
            r.hover(0, f64::from(f % 400), 300.0);
        }
        let b = now_us();
        r.tick();
        let c = now_us();
        if r.dirty() {
            commands += r.paint().len();
        }
        let d = now_us();
        receive += b - a;
        tick += c - b;
        paint += d - c;
    }
    let per = f64::from(frames);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "BENCH mode={} n={} frames={frames} setup={}ms total={}us receive={}us tick={}us paint={}us",
        o.mode,
        o.count,
        fixed(setup / 1000.0, 0),
        fixed((receive + tick + paint) / per, 1),
        fixed(receive / per, 1),
        fixed(tick / per, 1),
        fixed(paint / per, 1)
    );
    let _ = writeln!(
        out,
        "BENCH stages serialize={}us build={}us (the last build of each frame)",
        fixed(serialize / per, 1),
        fixed(build / per, 1)
    );
    let _ = writeln!(
        out,
        "BENCH layouts={}/frame builds={}/frame patched={} in-place={} commands={}/frame layers={}",
        fixed((r.layouts() - layouts) as f64 / per, 2),
        fixed(f64::from(r.stats().builds - builds) / per, 2),
        r.stats().patched - patched,
        r.stats().in_place - in_place,
        fixed(commands as f64 / per, 0),
        r.layers()
    );
    out
}

/// The bench as bench.cpp runs it: its report.
pub fn timed(o: &Options) -> String {
    let scenario = scenario(o);
    let backend = StubBackend::default();
    let engine = DocumentEngine::new(Arc::new(Process::default()), backend.clone());
    let mut r = Renderer::new(400.0, 2.0, false, engine, backend, None);
    run(o, &scenario, &mut r)
}

// ---- the trace -------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Fnv(u64);

impl Fnv {
    const START: Fnv = Fnv(0xcbf2_9ce4_8422_2325);

    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn u8(&mut self, v: u8) {
        self.bytes(&[v]);
    }
    fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.bytes(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    fn str(&mut self, s: &[u8]) {
        self.u64(s.len() as u64);
        self.bytes(s);
    }
    fn metrics(&mut self, m: &FontMetrics) {
        for v in [m.ascent, m.descent, m.height, m.x_height, m.zero_width, m.em_ascent, m.em_descent] {
            self.f32(v);
        }
    }
}

fn outs(out: &[Json]) -> String {
    let mut h = Fnv::START;
    for m in out {
        h.str(&m.dump());
    }
    format!(" out={}:{:016x}", out.len(), h.0)
}

#[derive(Debug)]
struct TapLog {
    hash: Fnv,
    calls: u64,
}

impl Default for TapLog {
    fn default() -> TapLog {
        TapLog { hash: Fnv::START, calls: 0 }
    }
}

/// The bench's backend behind one that writes down each call, its arguments and its answers.
#[derive(Clone, Debug)]
struct Tap {
    inner: StubBackend,
    log: Rc<RefCell<TapLog>>,
}

impl Tap {
    fn note(&self, write: impl FnOnce(&mut Fnv)) {
        let mut log = self.log.borrow_mut();
        log.calls += 1;
        write(&mut log.hash);
    }
}

impl Backend for Tap {
    fn create_font(&mut self, families: &[u8], size_px: f32, weight: i32, italic: bool, features: &[u8], out: &mut FontMetrics) -> i32 {
        let font = self.inner.create_font(families, size_px, weight, italic, features, out);
        self.note(|h| {
            h.u8(b'F');
            h.str(families);
            h.f32(size_px);
            h.i32(weight);
            h.u8(u8::from(italic));
            h.str(features);
            h.i32(font);
            h.metrics(out);
        });
        font
    }
    fn create_canvas_font(&mut self, families: &[u8], size_px: f32, weight: i32, italic: bool, out: &mut FontMetrics) -> i32 {
        let font = self.inner.create_canvas_font(families, size_px, weight, italic, out);
        self.note(|h| {
            h.u8(b'C');
            h.str(families);
            h.f32(size_px);
            h.i32(weight);
            h.u8(u8::from(italic));
            h.i32(font);
            h.metrics(out);
        });
        font
    }
    fn delete_font(&mut self, font: i32) {
        self.inner.delete_font(font);
        self.note(|h| {
            h.u8(b'D');
            h.i32(font);
        });
    }
    fn text_width(&mut self, font: i32, text: &[u8]) -> f32 {
        let width = self.inner.text_width(font, text);
        self.note(|h| {
            h.u8(b'W');
            h.i32(font);
            h.str(text);
            h.f32(width);
        });
        width
    }
    fn text_widths(&mut self, texts: &[TextInFont]) -> Vec<f32> {
        let widths = self.inner.text_widths(texts);
        self.note(|h| {
            h.u8(b'T');
            h.u64(texts.len() as u64);
            for t in texts {
                h.i32(t.font);
                h.str(&t.text);
            }
            h.u64(widths.len() as u64);
            for &w in &widths {
                h.f32(w);
            }
        });
        widths
    }
    fn load_image(&mut self, id: i32, url: &[u8], w: &mut f32, h: &mut f32) -> bool {
        let ok = self.inner.load_image(id, url, w, h);
        let (w, hh) = (*w, *h);
        self.note(|h| {
            h.u8(b'I');
            h.i32(id);
            h.str(url);
            h.u8(u8::from(ok));
            h.f32(w);
            h.f32(hh);
        });
        ok
    }
    fn canvas_run(&mut self, id: i32, w: i32, h: i32, ops: &[CanvasOp]) {
        self.inner.canvas_run(id, w, h, ops);
        let mut bytes = Vec::new();
        let ok = encode_batch(ops, &Caps::default(), &mut bytes);
        let height = h;
        self.note(|h| {
            h.u8(b'R');
            h.i32(id);
            h.i32(w);
            h.i32(height);
            h.u8(u8::from(ok));
            h.str(&bytes);
        });
    }
    fn canvas_read(&mut self, id: i32, x: i32, y: i32, w: i32, h: i32, rgba: &mut Vec<u8>) -> bool {
        let ok = self.inner.canvas_read(id, x, y, w, h, rgba);
        let height = h;
        self.note(|h| {
            h.u8(b'Q');
            for v in [id, x, y, w, height] {
                h.i32(v);
            }
            h.u8(u8::from(ok));
            h.str(rgba);
        });
        ok
    }
    fn canvas_release(&mut self, id: i32) {
        self.inner.canvas_release(id);
        self.note(|h| {
            h.u8(b'X');
            h.i32(id);
        });
    }
}

/// A time only the loop moves; the renderer's clock reads it from its own handle (a renderer's clock is `Send`).
struct StepClock(Arc<AtomicU64>);

impl Clock for StepClock {
    fn now_ms(&mut self) -> f64 {
        f64::from_bits(self.0.load(Ordering::Relaxed))
    }
}

/// The renderer on the step clock: 1000 ms when made, a 60th of a second more at each tick.
struct Stepped<B: Backend + Clone> {
    r: Renderer<DocumentEngine<B>, B>,
    clock: Arc<AtomicU64>,
}

impl<B: Backend + Clone> Stepped<B> {
    fn new(width: f64, dpr: f64, dark: bool, backend: B) -> Stepped<B> {
        let clock = Arc::new(AtomicU64::new(1000.0f64.to_bits()));
        let engine = DocumentEngine::new(Arc::new(Process::default()), backend.clone());
        let r = Renderer::new(width, dpr, dark, engine, backend, Some(Box::new(StepClock(Arc::clone(&clock)))));
        Stepped { r, clock }
    }
}

impl<B: Backend + Clone> Bench for Stepped<B> {
    fn receive(&mut self, msg: &Json) -> Vec<Json> {
        self.r.receive(msg)
    }
    fn hover(&mut self, action: i32, x: f64, y: f64) -> Vec<Json> {
        self.r.hover(action, x, y, 0)
    }
    fn tick(&mut self) -> Vec<Json> {
        let now = f64::from_bits(self.clock.load(Ordering::Relaxed)) + 1000.0 / 60.0;
        self.clock.store(now.to_bits(), Ordering::Relaxed);
        self.r.tick()
    }
    fn dirty(&self) -> bool {
        self.r.dirty()
    }
    fn paint(&mut self) -> DisplayList {
        self.r.paint()
    }
    fn set_compositing(&mut self, enabled: bool) {
        self.r.set_compositing(enabled)
    }
    fn stats(&self) -> &BuildStats {
        self.r.stats()
    }
    fn layouts(&self) -> u64 {
        self.r.layouts()
    }
    fn layers(&self) -> usize {
        self.r.composition().layers.len()
    }
}

/// The bench on the step clock without the trace: the same work in every run whatever each frame takes, where bench.cpp's
/// steady clock lets the time decide some (the CSS animations' progress at each tick, the render throttle holding back
/// what follows a pass over 16 ms). `bench_oracle --step` runs the C++ so.
pub fn stepped(o: &Options) -> String {
    let scenario = scenario(o);
    let mut r = Stepped::new(400.0, 2.0, false, StubBackend::default());
    run(o, &scenario, &mut r)
}

/// bench_oracle.cpp's `TracingRenderer`: the renderer on the tap and the step clock, each call a line.
struct Traced {
    stepped: Stepped<Tap>,
    log: Rc<RefCell<TapLog>>,
    lines: String,
}

impl Traced {
    fn new(width: f64, dpr: f64, dark: bool, backend: StubBackend) -> Traced {
        let log = Rc::new(RefCell::new(TapLog::default()));
        let tap = Tap { inner: backend, log: Rc::clone(&log) };
        let mut traced = Traced { stepped: Stepped::new(width, dpr, dark, tap), log, lines: String::new() };
        traced.line("open", "");
        traced
    }

    fn line(&mut self, what: &str, rest: &str) {
        let mut log = self.log.borrow_mut();
        let r = &self.stepped.r;
        let s = r.stats();
        let _ = writeln!(
            self.lines,
            "{what}{rest} be={}:{:016x} L={} B={} P={} I={} S={} R={} T={} N={} H={} M={} Y={}",
            log.calls,
            log.hash.0,
            r.layouts(),
            s.builds,
            s.patched,
            s.in_place,
            s.scripted,
            s.reused,
            s.throttled,
            s.passes,
            s.html_bytes,
            s.names,
            s.name_bytes
        );
        *log = TapLog::default();
    }
}

impl Bench for Traced {
    fn receive(&mut self, msg: &Json) -> Vec<Json> {
        let out = self.stepped.receive(msg);
        let mut h = Fnv::START;
        h.str(&msg.dump());
        self.line("receive", &format!(" in={:016x}{}", h.0, outs(&out)));
        out
    }
    fn hover(&mut self, action: i32, x: f64, y: f64) -> Vec<Json> {
        let out = self.stepped.hover(action, x, y);
        self.line("hover", &outs(&out));
        out
    }
    fn tick(&mut self) -> Vec<Json> {
        let out = self.stepped.tick();
        self.line("tick", &outs(&out));
        out
    }
    fn dirty(&self) -> bool {
        self.stepped.dirty()
    }
    fn paint(&mut self) -> DisplayList {
        let list = self.stepped.paint();
        let r = &self.stepped.r;
        let mut bytes = Vec::new();
        let ok = encode_list(&list, &Caps::default(), &mut bytes);
        let mut l = Fnv::START;
        l.bytes(&bytes);
        let mut rects = Vec::new();
        let damaged = r.damage(&mut rects);
        let mut d = Fnv::START;
        d.u8(u8::from(damaged));
        d.u64(rects.len() as u64);
        for r in &rects {
            for v in [r.x, r.y, r.w, r.h] {
                d.f32(v);
            }
        }
        let comp = r.composition();
        let mut c = Fnv::START;
        c.str(&composition_json(comp));
        for layer in &comp.layers {
            let mut content = Vec::new();
            let encoded = encode_list(&layer.content, &Caps::default(), &mut content);
            c.u8(u8::from(encoded));
            c.str(&content);
        }
        let rest = format!(
            " n={} list={}:{:016x} damage={:016x} layers={}:{}:{:016x}",
            list.len(),
            u8::from(ok),
            l.0,
            d.0,
            comp.version,
            comp.layers.len(),
            c.0
        );
        self.line("paint", &rest);
        list
    }
    fn set_compositing(&mut self, enabled: bool) {
        self.stepped.set_compositing(enabled);
        self.line("compositing", "");
    }
    fn stats(&self) -> &BuildStats {
        self.stepped.stats()
    }
    fn layouts(&self) -> u64 {
        self.stepped.layouts()
    }
    fn layers(&self) -> usize {
        self.stepped.layers()
    }
}

/// The bench on a step clock with every call traced: the trace's lines, then the report (whose counts are the C++'s
/// too; its timings are not).
pub fn traced(o: &Options) -> String {
    let scenario = scenario(o);
    let mut r = Traced::new(400.0, 2.0, false, StubBackend::default());
    let report = run(o, &scenario, &mut r);
    r.lines + &report
}
