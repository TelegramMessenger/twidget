//! What the shadow tree comparison shares with `oracle/shadow_oracle`: scripts of messages through one renderer, the
//! answer both sides write after each, the `.twrec` sessions they come from, and seeded op batches.
//!
//! A record is 'S', a u32 length and: f64 width, f64 dpr, u8 dark, u8 flags (1: each document as its u32 length and
//! u64 FNV-1a), u32 steps, each a u8 kind and its input: 'o' an ops array as JSON text (the `ops` message), 'm' any
//! message as JSON text, 'd' u8 dark (`set_theme`). The answer is status 1 and, for the renderer as made ('c') and
//! after each step, its kind (and for 'o' and 'm' a u8, the text parsed), then: the violations the tree reports, a
//! u32 count of (detail); the backend's canvas calls, a u32 byte length of records 'R' i32 canvas, i32 width, i32
//! height, u32 ops, u8 encoded, u64 FNV-1a of the batch in wire form with every NaN as one, or 'X' i32 canvas; a u8
//! count of the renderer's own stops (its layout and name limits, which the tree does not decide); and `last_html()`.
//! The C++ renderer runs with the paint policy off (`compat`): heavy boxes are stage 4.5's.

#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt::Write as _;

use twidget_json::Json;
pub use twidget_shadow::*;
pub use twidget_testkit::{Fields, Lcg};
use twidget_wire::{CanvasOp, Caps, FontMetrics, encode_batch, set_em_box};

pub fn fnv64(v: &[u8]) -> u64 {
    let mut h = 1469598103934665603u64;
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
    pub fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn str(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v);
    }
}

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

/// The renderer's side of the document over the oracle's stub backend: core_tests.cpp's fonts, canvas fonts numbered
/// on their own from 1001 and kept as `Renderer::canvas_font` keeps them, 8 px a byte of text, images that never
/// decode, and every canvas call written down.
#[derive(Debug, Default)]
pub struct StubHost {
    pub log: Out,
    next_canvas_font: i32,
    fonts: HashMap<Vec<u8>, (i32, FontMetrics)>,
}

impl StubHost {
    pub fn new() -> StubHost {
        StubHost { next_canvas_font: 1000, ..StubHost::default() }
    }
}

/// widget-sandbox's checkUrl reading of a data: URL's media type (renderer.cpp, `data_mime`).
fn data_mime(raw: &[u8]) -> Vec<u8> {
    let url: Vec<u8> = raw.iter().copied().filter(|&c| c != b'\t' && c != b'\n' && c != b'\r').collect();
    let (mut from, mut to) = (0, url.len());
    while from < to && url[from] <= 0x20 {
        from += 1;
    }
    while to > from && url[to - 1] <= 0x20 {
        to -= 1;
    }
    let u = &url[from..to];
    if u.len() < 5 || twidget_css::lower(&u[..5]) != b"data:" {
        return Vec::new();
    }
    let Some(end) = u[5..].iter().position(|&c| c == b',' || c == b';').map(|p| p + 5) else { return Vec::new() };
    if !u[5..].contains(&b',') {
        return Vec::new();
    }
    twidget_css::trim(&twidget_css::lower(&u[5..end]))
}

impl DocumentHost for StubHost {
    fn canvas_run(&mut self, canvas: i32, width: i32, height: i32, ops: &[CanvasOp]) {
        let mut bytes = Vec::new();
        let encoded = encode_batch(&canonical(ops), &Caps::default(), &mut bytes);
        let log = &mut self.log;
        log.u8(b'R');
        log.i32(canvas);
        log.i32(width);
        log.i32(height);
        log.u32(ops.len() as u32);
        log.u8(u8::from(encoded));
        log.u64(fnv64(&bytes));
    }

    fn canvas_release(&mut self, canvas: i32) {
        self.log.u8(b'X');
        self.log.i32(canvas);
    }

    fn canvas_run_begins(&mut self) {
        if self.fonts.len() > 64 {
            self.fonts.clear();
        }
    }

    fn canvas_font(&mut self, css: &[u8], metrics: &mut FontMetrics) -> i32 {
        if let Some((handle, m)) = self.fonts.get(css) {
            *metrics = *m;
            return *handle;
        }
        let f = twidget_css::parse_font(css);
        let size = f.size;
        metrics.ascent = size * 0.8;
        metrics.descent = size * 0.2;
        metrics.height = size * 1.2;
        metrics.x_height = size * 0.5;
        metrics.zero_width = 8.0;
        self.next_canvas_font += 1;
        let handle = self.next_canvas_font;
        if metrics.em_ascent == 0.0 && metrics.em_descent == 0.0 {
            let (a, d) = (metrics.ascent, metrics.descent);
            set_em_box(metrics, a, d, size);
        }
        self.fonts.insert(css.to_vec(), (handle, *metrics));
        handle
    }

    fn canvas_text_width(&mut self, _font: i32, text: &[u8]) -> f32 {
        8.0 * text.len() as f32
    }

    fn image_of(&mut self, src: &[u8]) -> Option<(i32, f32, f32)> {
        // The validator's placeholder; every other image fails to decode on the stub backend.
        (data_mime(src) == b"image/svg+xml").then_some((0, 48.0, 48.0))
    }
}

/// One message to the renderer.
#[derive(Clone, Debug)]
pub enum Step {
    /// An `ops` message: the array's JSON text.
    Ops(Vec<u8>),
    /// Any other message as JSON text.
    Message(Vec<u8>),
    /// `set_theme`.
    Dark(bool),
}

#[derive(Clone, Debug)]
pub struct Script {
    pub width: f64,
    pub dpr: f64,
    pub dark: bool,
    pub digest: bool,
    pub steps: Vec<Step>,
}

impl Script {
    pub fn new(steps: Vec<Step>) -> Script {
        Script { width: 300.0, dpr: 1.0, dark: false, digest: false, steps }
    }

    pub fn record(&self) -> Vec<u8> {
        let mut f = Fields::new();
        f.f64(self.width).f64(self.dpr).flag(self.dark).u8(u8::from(self.digest)).u32(self.steps.len() as u32);
        for step in &self.steps {
            match step {
                Step::Ops(text) => {
                    f.u8(b'o').str(text);
                }
                Step::Message(text) => {
                    f.u8(b'm').str(text);
                }
                Step::Dark(dark) => {
                    f.u8(b'd').flag(*dark);
                }
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
}

fn is_outside(detail: &[u8]) -> bool {
    detail.starts_with(b"layout limit") || detail.starts_with(b"name limit")
}

/// The renderer's message dispatch as far as the tree takes part in it (renderer.cpp, `Renderer::receive`).
pub fn receive(doc: &mut Document, host: &mut StubHost, msg: &Json) -> Vec<Json> {
    let mut out = Vec::new();
    match msg.get("type").as_str() {
        b"ops" => doc.receive_ops(msg.get("ops").arr(), host, &mut out),
        b"theme" => {
            doc.receive_theme(msg.get("theme"));
        }
        b"safeArea" => {
            doc.apply_safe_area(msg.get("inset"));
        }
        b"frozen" => doc.freeze(msg.get("reason").as_str(), msg.get("fatal").as_bool(false), host),
        _ => {}
    }
    // flush_pending: what the renderer held back goes out after the message's own answers.
    for v in std::mem::take(&mut doc.pending_violations) {
        let mut m = Json::object();
        m.set("type", "violation").set("detail", v);
        out.push(m);
    }
    out
}

/// What the renderer decided after a step that the tree does not: the details of its layout and name limit violations,
/// and the reason its badge gives when it stopped there.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub stops: Vec<Vec<u8>>,
    pub stopped: Option<Vec<u8>>,
}

struct Take<'a> {
    s: &'a [u8],
    at: usize,
}

impl Take<'_> {
    fn u8(&mut self) -> u8 {
        let v = self.s.get(self.at).copied().unwrap_or(0);
        self.at += 1;
        v
    }
    fn u32(&mut self) -> usize {
        let v = self.s.get(self.at..self.at + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap_or([0; 4])) as usize);
        self.at += 4;
        v
    }
    fn bytes(&mut self, n: usize) -> &[u8] {
        let v = self.s.get(self.at..(self.at + n).min(self.s.len())).unwrap_or_default();
        self.at += n;
        v
    }
    fn str(&mut self) -> Vec<u8> {
        let n = self.u32();
        self.bytes(n).to_vec()
    }
}

/// The facts of every answer step (the renderer as made first) in an answer of the oracle.
pub fn facts_of(answer: &[u8], digest: bool) -> Vec<Facts> {
    let mut t = Take { s: answer, at: 0 };
    let mut out = Vec::new();
    while t.at < answer.len() {
        let kind = t.u8();
        if (kind == b'o' || kind == b'm') && t.u8() == 0 {
            out.push(Facts::default());
            continue;
        }
        for _ in 0..t.u32() {
            t.str();
        }
        let calls = t.u32();
        t.bytes(calls);
        let stops: Vec<Vec<u8>> = (0..t.u32()).map(|_| t.str()).collect();
        let stopped = (t.u8() == 1).then(|| t.str());
        if digest {
            t.bytes(12);
        } else {
            t.str();
        }
        out.push(Facts { stops, stopped });
    }
    out
}

/// The script through the port, writing what the oracle writes, with what `facts` say the renderer decided itself
/// after each step: its stops past its layout and name limits.
pub fn run(script: &Script, facts: &[Facts], tally: &mut Tally) -> Vec<u8> {
    let mut doc = Document::new(script.dark);
    let mut host = StubHost::new();
    let mut o = Out::default();
    let overrides = StyleOverrides::new();
    let chrome = Chrome { overrides: &overrides, editing: 0 };
    let mut step = 0usize;
    let tail = |o: &mut Out, doc: &mut Document, host: &mut StubHost, out: &[Json], step: &mut usize, tally: &mut Tally| {
        let fact = facts.get(*step).cloned().unwrap_or_default();
        *step += 1;
        if let Some(reason) = &fact.stopped
            && !doc.layout_stopped()
        {
            tally.add("stopped by the renderer");
            doc.stop(b"", reason, host);
            doc.pending_violations.clear();
        }
        let tree: Vec<&[u8]> = out
            .iter()
            .filter(|m| m.get("type").as_str() == b"violation")
            .map(|m| m.get("detail").as_str())
            .filter(|d| !is_outside(d) && !d.starts_with(b"image "))
            .collect();
        o.u32(tree.len() as u32);
        for d in &tree {
            let words: Vec<&[u8]> = d.split(|&c| c == b' ').take(2).collect();
            tally.add(format!("violation {}", String::from_utf8_lossy(&words.join(&b' '))));
            o.str(d);
        }
        let html = doc.serialize(&chrome);
        let calls = std::mem::take(&mut host.log.0);
        o.u32(calls.len() as u32);
        o.0.extend_from_slice(&calls);
        o.u32(fact.stops.len() as u32);
        for d in &fact.stops {
            o.str(d);
        }
        o.u8(u8::from(doc.layout_stopped()));
        if doc.layout_stopped() {
            let mut reason = Vec::new();
            escape_text(doc.frozen_reason(), &mut reason);
            o.str(&reason);
        }
        if script.digest {
            o.u32(html.len() as u32);
            o.u64(fnv64(&html));
        } else {
            o.str(&html);
        }
    };
    o.u8(b'c');
    tail(&mut o, &mut doc, &mut host, &[], &mut step, tally);
    for s in &script.steps {
        match s {
            Step::Ops(text) | Step::Message(text) => {
                let ops = matches!(s, Step::Ops(_));
                o.u8(if ops { b'o' } else { b'm' });
                let parsed = Json::parse(text);
                o.u8(u8::from(parsed.is_some()));
                let Some(parsed) = parsed else {
                    tally.add("not JSON");
                    step += 1;
                    continue;
                };
                let msg = if ops {
                    let mut m = Json::object();
                    m.set("type", "ops").set("ops", parsed);
                    m
                } else {
                    parsed
                };
                tally.add(format!("message {}", String::from_utf8_lossy(msg.get("type").as_str())));
                let out = receive(&mut doc, &mut host, &msg);
                tail(&mut o, &mut doc, &mut host, &out, &mut step, tally);
                // last_html() builds, and every build takes the log of the ops before it.
                let log = doc.take_patch_log();
                tally.add(if !log.valid {
                    "patch log invalid"
                } else if log.attributes.is_empty() && log.texts.is_empty() {
                    "patch log empty"
                } else {
                    "patch log in place"
                });
            }
            Step::Dark(dark) => {
                o.u8(b'd');
                doc.set_dark(*dark);
                tail(&mut o, &mut doc, &mut host, &[], &mut step, tally);
            }
        }
    }
    if doc.frozen() {
        tally.add(if doc.layout_stopped() { "ends stopped" } else { "ends frozen" });
    }
    tally.add(format!("canvases {}", doc.canvases().len().min(3)));
    o.0
}

// ---- Recordings --------------------------------------------------------------------------------------

struct Reader<'a> {
    s: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let b = *self.s.get(self.at)?;
        self.at += 1;
        Some(b)
    }
    fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }
    fn bytes(&mut self) -> Option<&[u8]> {
        let n = usize::try_from(self.varint()?).ok()?;
        let out = self.s.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(out)
    }
    fn f64(&mut self) -> Option<f64> {
        let b = self.s.get(self.at..self.at + 8)?;
        self.at += 8;
        Some(f64::from_le_bytes(b.try_into().ok()?))
    }
    fn json(&mut self, depth: usize) -> Option<Json> {
        if depth > 1024 {
            return None;
        }
        Some(match self.byte()? {
            0 => Json::Null,
            1 => Json::Bool(false),
            2 => Json::Bool(true),
            3 => Json::Number(self.f64()?),
            4 => Json::String(self.bytes()?.to_vec()),
            5 => {
                let n = self.varint()?;
                let mut items = Vec::new();
                for _ in 0..n {
                    items.push(self.json(depth + 1)?);
                }
                Json::Array(items)
            }
            6 => {
                let n = self.varint()?;
                let mut members = Vec::new();
                for _ in 0..n {
                    let key = self.bytes()?.to_vec();
                    members.push((key, self.json(depth + 1)?));
                }
                Json::Object(members)
            }
            _ => return None,
        })
    }
}

/// Every session of a `.twrec` recording (tests/recording.h) as the messages that reach the tree: the renderer as
/// made, every `ops`, `theme`, `safeArea` and `frozen` message, and `set_theme`, in their order.
pub fn recorded_sessions(bytes: &[u8]) -> Vec<Script> {
    let mut r = Reader { s: bytes, at: 0 };
    assert!(bytes.starts_with(b"TWREC") && bytes.get(5) == Some(&1), "a version 1 recording");
    r.at = 6;
    r.bytes().expect("the origin");
    let mut sessions: Vec<(u64, Script)> = Vec::new();
    while r.at < bytes.len() {
        let n = usize::try_from(r.varint().expect("a frame")).expect("a frame length");
        let frame = &bytes[r.at..r.at + n];
        r.at += n;
        let mut f = Reader { s: frame, at: 0 };
        let kind = f.byte().expect("a kind");
        if kind == 0 {
            break;
        }
        let session = f.varint().expect("a session");
        f.bytes().expect("a label");
        f.varint().expect("names");
        f.varint().expect("name bytes");
        let numbers: Vec<f64> = (0..f.varint().expect("numbers")).map(|_| f.f64().expect("a number")).collect();
        for _ in 0..f.varint().expect("texts") {
            f.bytes().expect("a text");
        }
        let message = if f.byte().expect("a flag") == 1 { f.json(0) } else { None };
        match kind {
            1 => sessions
                .push((session, Script { width: numbers[0], dpr: numbers[1], dark: numbers[2] != 0.0, digest: false, steps: Vec::new() })),
            3 | 9 => {
                let Some((_, script)) = sessions.iter_mut().rev().find(|(s, _)| *s == session) else { continue };
                if kind == 9 {
                    script.steps.push(Step::Dark(numbers[0] != 0.0));
                    continue;
                }
                let Some(m) = message else { continue };
                match m.get("type").as_str() {
                    b"ops" => script.steps.push(Step::Ops(m.get("ops").dump())),
                    b"theme" | b"safeArea" | b"frozen" => script.steps.push(Step::Message(m.dump())),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    sessions.into_iter().map(|(_, s)| s).collect()
}

// ---- Generators --------------------------------------------------------------------------------------

pub fn pick<'a, T>(r: &mut Lcg, items: &'a [T]) -> &'a T {
    &items[r.below(items.len())]
}

pub fn quote(s: &str) -> String {
    String::from_utf8(twidget_json::quote(s)).unwrap_or_default()
}

/// The page every widget starts with: its html, head and body adopted, a style element, a few elements.
pub const PAGE: &str = r##"[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},{"op":"el","id":20,"tag":"style","attrs":[]},
{"op":"css","id":20,"text":"#a{width:100px;height:100px;float:left;color:light-dark(#010203,#040506)} #a1{width:50px;height:50px} #b{width:100px;height:100px;float:left}"},
{"op":"ins","parent":3,"id":20,"before":null},{"op":"el","id":10,"tag":"div","attrs":[["id","a"]]},{"op":"el","id":11,"tag":"div","attrs":[["id","a1"]]},
{"op":"el","id":12,"tag":"div","attrs":[["id","b"]]},{"op":"ins","parent":4,"id":10,"before":null},{"op":"ins","parent":10,"id":11,"before":null},
{"op":"ins","parent":4,"id":12,"before":null}]"##;

const TAGS: [&str; 30] = [
    "div", "div", "span", "p", "b", "a", "ul", "li", "img", "canvas", "canvas", "style", "textarea", "input", "select", "option",
    "details", "summary", "dialog", "br", "svg", "g", "rect", "script", "template", "table", "td", "DIV", "x-y", "1bad",
];

const ATTRS: [&str; 22] = [
    "class",
    "class",
    "id",
    "title",
    "src",
    "width",
    "height",
    "open",
    "value",
    "placeholder",
    "data-x",
    "style",
    "STYLE",
    "Data-TwId",
    "data-twid",
    "data-lh-id",
    "onclick",
    "ONload",
    "a b",
    "x\"y",
    "href",
    "aria-label",
];

const PROPERTIES: [&str; 18] = [
    "color",
    "background-color",
    "width",
    "height",
    "margin",
    "display",
    "opacity",
    "transform",
    "--x",
    "--tg-theme-bg-color",
    "COLOR",
    "color:red;x",
    "1bad",
    "",
    "padding-left",
    "font-size",
    "border",
    "--Y",
];

/// Text the way widgets write it, with what HTML would read as markup.
pub fn text(r: &mut Lcg) -> String {
    match r.below(10) {
        0 => {
            pick(r, &["", " ", "a < b", "x & y", "</div>", "<b>bold</b>", "&amp;", "\"q\"", "Ж ё 日本 😀", "\u{0}", "\n\nlead"]).to_string()
        }
        1 => (0..r.below(40)).map(|_| *pick(r, &["word", " ", "  ", "\t", "é", "\u{e000}"])).collect(),
        _ => format!("{} {}", pick(r, &["Hello", "Total", "42", "Weekly budget", "x", "Привет"]), r.below(1000)),
    }
}

pub fn value(r: &mut Lcg) -> String {
    match r.below(12) {
        0 => color(r),
        1 => format!("{}px", r.below(400)),
        2 => pick(
            r,
            &["", "none", "block", "inline", "flex", "light-dark(red, blue)", "light-dark(#000,light-dark(#111,#222))", "red !important"],
        )
        .to_string(),
        3 => "data:image/svg+xml;base64,PHN2Zy8+".to_string(),
        4 => "data:image/png;base64,AA==".to_string(),
        5 => pick(r, &["a b", "x\"y<z>&", "300", "  20  ", "-5", "1e3", "abc", "2147483648", "0"]).to_string(),
        _ => pick(r, &["card", "row", "big", "x", "a", "b", "s", "t"]).to_string(),
    }
}

pub fn color(r: &mut Lcg) -> String {
    match r.below(5) {
        0 => format!("#{:06x}", r.next() & 0xffffff),
        1 => format!("rgb({}, {}, {})", r.below(256), r.below(256), r.below(256)),
        2 => "light-dark(#010203, #040506)".to_string(),
        _ => pick(r, &["red", "blue", "transparent", "black"]).to_string(),
    }
}

pub fn sheet(r: &mut Lcg) -> String {
    let mut s = String::new();
    for _ in 0..r.below(6) {
        let _ = match r.below(14) {
            0 => write!(s, "@media (prefers-color-scheme: {}){{.x{{color:{}}}}}", pick(r, &["dark", "light", "DARK"]), color(r)),
            1 => write!(s, "@media (prefers-color-scheme){{.y{{width:{}px}}}}", r.below(99)),
            2 => write!(s, "a::before{{content:\"<div data-twid='10'>\\<&</b>\"}}"),
            3 => write!(s, "/* <div> */ <!-- .c{{color:red}} --> "),
            4 => write!(s, ".z{{color:light-dark({},{})}}", color(r), color(r)),
            5 => write!(s, "@media (100px<width){{body>#a{{margin-left:9px}}}}"),
            6 => write!(s, "{}", pick(r, &["a{}", "<", "<=", "\\<", "\\", "'<'", "\"\\3c\"", "url(<x>)", "{", "}", "@import 'x';"])),
            _ => write!(
                s,
                ".{}{{{}:{}}}",
                pick(r, &["card", "row", "big", "x", "a"]),
                pick(r, &["color", "width", "margin", "display"]),
                value(r)
            ),
        };
    }
    s
}

/// What the generator knows of the tree it builds, to name nodes that exist most of the time.
#[derive(Debug, Default)]
pub struct Model {
    next: i64,
    elements: Vec<i64>,
    texts: Vec<i64>,
    styles: Vec<i64>,
    canvases: Vec<i64>,
}

impl Model {
    pub fn new() -> Model {
        Model { next: 100, elements: vec![2, 3, 4, 10, 11, 12, 20], texts: Vec::new(), styles: vec![20], canvases: Vec::new() }
    }

    fn any(&self, r: &mut Lcg) -> i64 {
        match r.below(12) {
            0 => *pick(r, &[0, -1, 99999, 1 << 40, 2147483647]),
            1 if !self.texts.is_empty() => *pick(r, &self.texts),
            2 if !self.canvases.is_empty() => *pick(r, &self.canvases),
            _ => *pick(r, &self.elements),
        }
    }

    /// A batch that moves every node made under <body>, one after another: what is detached, or deeper than the
    /// document is written, shows in the HTML.
    pub fn reveal(&self, r: &mut Lcg) -> Vec<u8> {
        let mut ids: Vec<i64> = self.elements.iter().chain(&self.texts).chain(&self.canvases).copied().filter(|&id| id >= 100).collect();
        for i in (1..ids.len()).rev() {
            ids.swap(i, r.below(i + 1));
        }
        let ops: Vec<String> = ids.iter().map(|id| format!("{{\"op\":\"ins\",\"parent\":4,\"id\":{id},\"before\":null}}")).collect();
        format!("[{}]", ops.join(",")).into_bytes()
    }

    fn fresh(&mut self, r: &mut Lcg) -> i64 {
        if r.chance(15) {
            return self.any(r);
        }
        self.next += 1;
        self.next
    }
}

fn attrs(r: &mut Lcg, canvas: bool) -> String {
    let mut list = Vec::new();
    for _ in 0..r.below(4) {
        let name = if canvas && r.chance(2) { *pick(r, &["width", "height"]) } else { *pick(r, &ATTRS) };
        let value = if name.eq_ignore_ascii_case("style") {
            format!("{}:{};{}:{}", pick(r, &PROPERTIES), value(r), pick(r, &PROPERTIES), value(r))
        } else if canvas && (name == "width" || name == "height") {
            pick(r, &["0", "10", "300", "  64", "abc", "5000", "8193", "-3", "2048"]).to_string()
        } else {
            value(r)
        };
        list.push(format!("[{},{}]", quote(name), quote(&value)));
    }
    if r.chance(20) {
        list.push("[1,\"x\"]".to_string());
    }
    format!("[{}]", list.join(","))
}

fn canvas_cmds(r: &mut Lcg, m: &Model) -> String {
    let mut cmds = Vec::new();
    for _ in 0..1 + r.below(6) {
        cmds.push(match r.below(8) {
            0 => format!("[\"s\",\"fillStyle\",{}]", quote(&color(r))),
            1 => format!("[\"c\",\"fillRect\",{},{},{},{}]", r.below(50), r.below(50), r.below(80), r.below(80)),
            2 => format!("[\"s\",\"font\",{}]", quote(pick(r, &["12px sans-serif", "bold 20px serif", "10px x", "italic 9px y"]))),
            3 => format!("[\"c\",\"fillText\",{},{},{}]", quote(&text(r)), r.below(50), r.below(50)),
            4 if !m.canvases.is_empty() || !m.elements.is_empty() => {
                let n = if r.chance(2) && !m.canvases.is_empty() { *pick(r, &m.canvases) } else { *pick(r, &m.elements) };
                format!("[\"c\",\"drawImage\",{{\"n\":{n}}},{},{}]", r.below(30), r.below(30))
            }
            5 => format!("[\"c\",\"arc\",{},{},{},0,6.3]", r.below(50), r.below(50), r.below(30)),
            6 => "[\"c\",\"fill\"]".to_string(),
            _ => format!("[\"c\",\"strokeRect\",1,1,{},{}]", r.below(40), r.below(40)),
        });
    }
    format!("[{}]", cmds.join(","))
}

/// One op as the validator sends them, and past them.
pub fn op(r: &mut Lcg, m: &mut Model) -> String {
    match r.below(40) {
        0..=5 => {
            let id = m.fresh(r);
            let tag = *pick(r, &TAGS);
            let svg = matches!(tag, "svg" | "g" | "rect") || r.chance(30);
            let ns = if svg { ",\"ns\":\"svg\"" } else { "" };
            let canvas = tag == "canvas";
            if canvas {
                m.canvases.push(id);
            } else {
                m.elements.push(id);
            }
            if tag == "style" {
                m.styles.push(id);
            }
            format!("{{\"op\":\"el\",\"id\":{id},\"tag\":{}{ns},\"attrs\":{}}}", quote(tag), attrs(r, canvas))
        }
        6..=8 => {
            let id = m.fresh(r);
            m.texts.push(id);
            let kind = if r.chance(8) { "comment" } else { "text" };
            format!("{{\"op\":\"{kind}\",\"id\":{id},\"data\":{}}}", quote(&text(r)))
        }
        9..=15 => {
            let id = m.any(r);
            let parent = if r.chance(12) { *pick(r, &[0, 99999, 2]) } else { *pick(r, &m.elements) };
            let before = match r.below(4) {
                0 => m.any(r).to_string(),
                1 => "\"x\"".to_string(),
                _ => "null".to_string(),
            };
            format!("{{\"op\":\"ins\",\"parent\":{parent},\"id\":{id},\"before\":{before}}}")
        }
        16 | 17 => format!("{{\"op\":\"rm\",\"id\":{}}}", m.any(r)),
        18..=21 => {
            let id = m.any(r);
            let canvas = m.canvases.contains(&id);
            let name = if canvas && r.chance(2) { *pick(r, &["width", "height"]) } else { *pick(r, &ATTRS) };
            let value = if canvas { pick(r, &["0", "10", "300", "  64", "abc", "5000", "8193", "2048"]).to_string() } else { value(r) };
            format!("{{\"op\":\"attr\",\"id\":{id},\"name\":{},\"value\":{}}}", quote(name), quote(&value))
        }
        22 => format!("{{\"op\":\"rattr\",\"id\":{},\"name\":{}}}", m.any(r), quote(pick(r, &ATTRS))),
        23..=26 => {
            let id = if r.chance(10) { 2 } else { m.any(r) };
            let important = if r.chance(4) { ",\"important\":true" } else { "" };
            format!("{{\"op\":\"style\",\"id\":{id},\"name\":{},\"value\":{}{important}}}", quote(pick(r, &PROPERTIES)), quote(&value(r)))
        }
        27 | 28 => {
            let id = if !m.styles.is_empty() && !r.chance(5) { *pick(r, &m.styles) } else { m.any(r) };
            format!("{{\"op\":\"css\",\"id\":{id},\"text\":{}}}", quote(&sheet(r)))
        }
        29 | 30 => format!("{{\"op\":\"data\",\"id\":{},\"data\":{}}}", m.any(r), quote(&text(r))),
        31 | 32 => {
            let name = *pick(r, &["value", "value", "checked", "selected", "indeterminate", "selectedIndex", "scrollTop", "other"]);
            let v = match r.below(6) {
                0 => "true".to_string(),
                1 => "false".to_string(),
                2 => format!("{}", r.below(4)),
                3 => "null".to_string(),
                4 => "{\"a\":[1,\"<x>\"]}".to_string(),
                _ => quote(&text(r)),
            };
            format!("{{\"op\":\"prop\",\"id\":{},\"name\":\"{name}\",\"value\":{v}}}", m.any(r))
        }
        33 => {
            let method = *pick(
                r,
                &[
                    "showModal",
                    "show",
                    "showPopover",
                    "close",
                    "hidePopover",
                    "focus",
                    "blur",
                    "observeResize",
                    "setPointerCapture",
                    "scrollTo",
                    "x",
                ],
            );
            format!("{{\"op\":\"call\",\"id\":{},\"method\":\"{method}\",\"args\":[1]}}", m.any(r))
        }
        _ => {
            let id = if !m.canvases.is_empty() && !r.chance(6) { *pick(r, &m.canvases) } else { m.any(r) };
            format!("{{\"op\":\"canvas\",\"id\":{id},\"cmds\":{}}}", canvas_cmds(r, m))
        }
    }
}

pub fn batch(r: &mut Lcg, m: &mut Model, n: usize) -> Vec<u8> {
    let ops: Vec<String> = (0..n).map(|_| op(r, m)).collect();
    format!("[{}]", ops.join(",")).into_bytes()
}

pub fn message(r: &mut Lcg) -> Vec<u8> {
    match r.below(6) {
        0 => format!(
            "{{\"type\":\"theme\",\"theme\":{{\"colorScheme\":\"{}\",\"bg_color\":{}}}}}",
            pick(r, &["dark", "light"]),
            quote(&color(r))
        )
        .into_bytes(),
        1 => format!("{{\"type\":\"safeArea\",\"inset\":{{\"top\":{},\"bottom\":{}}}}}", r.below(40), r.below(40)).into_bytes(),
        2 => format!("{{\"type\":\"frozen\",\"reason\":{},\"fatal\":{}}}", quote(pick(r, &["Paused", "<b>x</b> & y", ""])), r.chance(2))
            .into_bytes(),
        3 => b"{\"type\":\"listen\",\"types\":[\"click\"]}".to_vec(),
        _ => format!("{{\"type\":\"theme\",\"theme\":{{\"colorScheme\":\"{}\"}}}}", pick(r, &["dark", "light"])).into_bytes(),
    }
}

/// A generated script: the page, then batches of ops and now and then a theme, a safe area, a freeze or a scheme.
pub fn any_script(r: &mut Lcg) -> Script {
    let mut m = Model::new();
    let mut steps = vec![Step::Ops(PAGE.as_bytes().to_vec())];
    if r.chance(10) {
        steps.clear();
    }
    for _ in 0..1 + r.below(8) {
        match r.below(10) {
            0 => steps.push(Step::Message(message(r))),
            1 => steps.push(Step::Dark(r.chance(2))),
            _ => {
                let most = if r.chance(5) { 60 } else { 12 };
                let n = 1 + r.below(most);
                steps.push(Step::Ops(batch(r, &mut m, n)));
            }
        }
    }
    if !r.chance(3) {
        steps.push(Step::Ops(m.reveal(r)));
    }
    let mut script = Script::new(steps);
    script.dark = r.chance(4);
    script
}

// ---- The corpus ---------------------------------------------------------------------------------------

pub fn widget_sandbox() -> std::path::PathBuf {
    std::env::var_os("TWIDGET_WIDGET_SANDBOX")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../widget-sandbox"))
}

fn html_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            html_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "html") {
            out.push(path);
        }
    }
}

/// Every corpus widget of widget-sandbox (its corpus, conformance and stress sets), by path.
pub fn corpus_files() -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for dir in ["corpus", "packages/conformance", "packages/stress"] {
        html_files(&widget_sandbox().join(dir), &mut files);
    }
    files.sort();
    files
}

fn decode_entities(s: &[u8]) -> Vec<u8> {
    let named: [(&[u8], &[u8]); 6] =
        [(b"&amp;", b"&"), (b"&lt;", b"<"), (b"&gt;", b">"), (b"&quot;", b"\""), (b"&#39;", b"'"), (b"&nbsp;", "\u{a0}".as_bytes())];
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'&'
            && let Some((k, v)) = named.iter().find(|(k, _)| s[i..].starts_with(k))
        {
            out.extend_from_slice(v);
            i += k.len();
            continue;
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

fn find_ci(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    (from..=hay.len().checked_sub(needle.len())?).find(|&i| hay[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

const VOID: [&[u8]; 14] =
    [b"area", b"base", b"br", b"col", b"embed", b"hr", b"img", b"input", b"link", b"meta", b"param", b"source", b"track", b"wbr"];

fn text_op(ops: &mut Vec<String>, id: i64, parent: i64, data: &[u8]) {
    ops.push(format!("{{\"op\":\"text\",\"id\":{id},\"data\":{}}}", quote(&String::from_utf8_lossy(data))));
    ops.push(format!("{{\"op\":\"ins\",\"parent\":{parent},\"id\":{id},\"before\":null}}"));
}

/// The attributes of a start tag from `j`, and where its name list ends.
/// (name, value) of each attribute of a tag.
type Attributes = Vec<(Vec<u8>, Vec<u8>)>;

fn tag_attributes(html: &[u8], mut j: usize) -> (Attributes, usize) {
    let mut attrs = Vec::new();
    loop {
        while j < html.len() && html[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= html.len() || html[j] == b'>' || html[j..].starts_with(b"/>") {
            return (attrs, j);
        }
        let start = j;
        while j < html.len() && !html[j].is_ascii_whitespace() && html[j] != b'=' && html[j] != b'>' && !html[j..].starts_with(b"/>") {
            j += 1;
        }
        let name = html[start..j].to_vec();
        if name.is_empty() {
            j += 1;
            continue;
        }
        let mut value = Vec::new();
        if j < html.len() && html[j] == b'=' {
            j += 1;
            if j < html.len() && (html[j] == b'"' || html[j] == b'\'') {
                let q = html[j];
                let e = html[j + 1..].iter().position(|&c| c == q).map_or(html.len(), |e| j + 1 + e);
                value = decode_entities(&html[j + 1..e]);
                j = (e + 1).min(html.len());
            } else {
                let s = j;
                while j < html.len() && !html[j].is_ascii_whitespace() && html[j] != b'>' {
                    j += 1;
                }
                value = decode_entities(&html[s..j]);
            }
        }
        attrs.push((name, value));
    }
}

/// A widget's static HTML as the ops the sandbox's validator sends for it, roughly: its elements with their
/// attributes, its texts and its style sheets under the adopted roots, scripts left out. CSS animations and
/// transitions are renamed so that none runs: animated values are not the tree's.
pub fn html_to_ops(source: &[u8], first_id: i64) -> Vec<String> {
    let html = String::from_utf8_lossy(source).replace("animation", "x-anim").replace("transition", "x-trans").into_bytes();
    let mut ops = Vec::new();
    let mut next = first_id;
    // (tag, node id, in svg)
    let mut stack: Vec<(Vec<u8>, i64, bool)> = vec![(b"body".to_vec(), 4, false)];
    let mut i = 0;
    while i < html.len() {
        let rest = &html[i..];
        if rest.starts_with(b"<!--") {
            i = find_ci(&html, b"-->", i + 4).map_or(html.len(), |e| e + 3);
            continue;
        }
        if rest.len() > 1 && rest[0] == b'<' && (rest[1] == b'!' || rest[1] == b'?') {
            i = rest.iter().position(|&c| c == b'>').map_or(html.len(), |e| i + e + 1);
            continue;
        }
        if rest.starts_with(b"</") {
            let end = rest.iter().position(|&c| c == b'>').map_or(html.len(), |e| i + e);
            let name = html[i + 2..end].trim_ascii().to_ascii_lowercase();
            if let Some(at) = stack.iter().rposition(|(n, _, _)| *n == name)
                && at > 0
            {
                stack.truncate(at);
            }
            i = (end + 1).min(html.len());
            continue;
        }
        if rest.len() > 1 && rest[0] == b'<' && rest[1].is_ascii_alphabetic() {
            let mut j = i + 1;
            while j < html.len() && (html[j].is_ascii_alphanumeric() || html[j] == b'-') {
                j += 1;
            }
            let tag = html[i + 1..j].to_ascii_lowercase();
            let (attrs, j) = tag_attributes(&html, j);
            let closed = html[j..].starts_with(b"/>");
            i = html[j..].iter().position(|&c| c == b'>').map_or(html.len(), |e| j + e + 1);
            if matches!(tag.as_slice(), b"html" | b"head" | b"body") {
                continue;
            }
            if tag == b"script" {
                i = find_ci(&html, b"</script", i).unwrap_or(html.len());
                continue;
            }
            let svg = tag == b"svg" || stack.last().is_some_and(|s| s.2);
            next += 1;
            let id = next;
            let list: Vec<String> = attrs
                .iter()
                .map(|(k, v)| format!("[{},{}]", quote(&String::from_utf8_lossy(k)), quote(&String::from_utf8_lossy(v))))
                .collect();
            let ns = if svg { ",\"ns\":\"svg\"" } else { "" };
            ops.push(format!(
                "{{\"op\":\"el\",\"id\":{id},\"tag\":{}{ns},\"attrs\":[{}]}}",
                quote(&String::from_utf8_lossy(&tag)),
                list.join(",")
            ));
            let head = matches!(tag.as_slice(), b"style" | b"meta" | b"title" | b"link");
            let parent = if head { 3 } else { stack.last().map_or(4, |s| s.1) };
            ops.push(format!("{{\"op\":\"ins\",\"parent\":{parent},\"id\":{id},\"before\":null}}"));
            if matches!(tag.as_slice(), b"style" | b"textarea" | b"title") {
                let close = [&b"</"[..], &tag].concat();
                let end = find_ci(&html, &close, i).unwrap_or(html.len());
                let body = &html[i..end];
                if tag == b"style" {
                    ops.push(format!("{{\"op\":\"css\",\"id\":{id},\"text\":{}}}", quote(&String::from_utf8_lossy(body))));
                } else if !body.is_empty() {
                    next += 1;
                    text_op(&mut ops, next, id, &decode_entities(body));
                }
                i = html[end..].iter().position(|&c| c == b'>').map_or(html.len(), |e| end + e + 1);
                continue;
            }
            if !closed && !VOID.contains(&tag.as_slice()) {
                stack.push((tag, id, svg));
            }
            continue;
        }
        let end = html[i + 1..].iter().position(|&c| c == b'<').map_or(html.len(), |e| i + 1 + e);
        let data = decode_entities(&html[i..end]);
        if !data.trim_ascii().is_empty() || data.contains(&b' ') {
            next += 1;
            text_op(&mut ops, next, stack.last().map_or(4, |s| s.1), &data);
        }
        i = end;
    }
    ops
}

/// The adopted roots alone.
pub const ROOTS: &str = r#"[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[]}]"#;
