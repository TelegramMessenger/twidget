//! What the canvas comparison shares with `oracle/canvas_oracle`: the scripted host, scripts of steps through one canvas
//! and the transcript both sides write of them (batches with every NaN as one), and seeded command lists as the sandbox sends them (widget-sandbox's
//! canvas.ts shapes, the stress widgets' frames) with hostile ones among them.

#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use twidget_canvas::*;
use twidget_json::Json;
pub use twidget_testkit::Lcg;
use twidget_wire::{CanvasOp, Caps, Clips, FontMetrics, encode_batch};

fn fnv32(s: &[u8]) -> u32 {
    let mut h = 2166136261u32;
    for &c in s {
        h = (h ^ u32::from(c)).wrapping_mul(16777619);
    }
    h
}

/// Where both sides write a script's outputs and the host's calls, in their order.
#[derive(Debug, Default)]
pub struct Transcript {
    pub out: Vec<u8>,
    /// Batches and pixels as their length and FNV-1a, not their bytes.
    pub digest: bool,
}

impl Transcript {
    pub fn u8(&mut self, v: u8) {
        self.out.push(v);
    }
    pub fn u32(&mut self, v: u32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i32(&mut self, v: i32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i64(&mut self, v: i64) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.out.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    pub fn str(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.out.extend_from_slice(v);
    }
    pub fn payload(&mut self, v: &[u8]) {
        if !self.digest {
            return self.str(v);
        }
        let mut h = 1469598103934665603u64;
        for &c in v {
            h = (h ^ u64::from(c)).wrapping_mul(1099511628211);
        }
        self.u32(v.len() as u32);
        self.u64(h);
    }
}

/// The oracle's scripted host: fonts and widths from the bytes they are asked about, sources from the node id, the
/// allowance and the other canvases' floats as the script sets them; it writes down every call.
#[derive(Debug, Default)]
pub struct ScriptHost {
    pub log: Transcript,
    pub allowance: i64,
    pub others: u64,
}

impl CanvasHost for ScriptHost {
    fn canvas_font(&mut self, css: &[u8], m: &mut FontMetrics) -> i32 {
        let h = fnv32(css);
        m.ascent = (h % 17) as f32 * 0.75 + 1.0;
        m.descent = ((h >> 5) % 7) as f32 * 0.5;
        m.height = m.ascent + m.descent + 1.0;
        m.x_height = ((h >> 9) % 5) as f32;
        m.zero_width = ((h >> 12) % 9) as f32 * 0.5;
        if (h >> 3) & 1 != 0 {
            m.em_ascent = m.ascent * 0.875;
            m.em_descent = m.descent * 0.625 + 0.25;
        }
        let handle = if css.is_empty() || h.is_multiple_of(11) { 0 } else { (1 + (h >> 16) % 9) as i32 };
        let log = &mut self.log;
        log.u8(b'F');
        log.str(css);
        log.i32(handle);
        for v in [m.ascent, m.descent, m.height, m.x_height, m.zero_width, m.em_ascent, m.em_descent] {
            log.f32(v);
        }
        handle
    }

    fn canvas_text_width(&mut self, font: i32, text: &[u8]) -> f32 {
        let w = match text.first() {
            Some(b'!') => f32::NAN,
            Some(b'#') => f32::INFINITY,
            Some(b'~') => -5.0,
            first => text.len() as f32 * (font as f32 * 0.5 + 3.0) + first.map_or(0, |c| c % 5) as f32 * 0.25,
        };
        self.log.u8(b'W');
        self.log.i32(font);
        self.log.str(text);
        self.log.f32(w);
        w
    }

    fn canvas_source(&mut self, node: i64) -> Option<CanvasSource> {
        let u = node as u64;
        let mut s = CanvasSource::default();
        let found = match u % 8 {
            0 => false,
            1 => {
                s = CanvasSource { image: 1 + (u % 997) as i32, canvas: 0, w: (u % 50) as f32 + 1.0, h: (u % 30) as f32 + 2.0 };
                true
            }
            2 => {
                s = CanvasSource { image: 0, canvas: 1 + (u % 89) as i32, w: (u % 64) as f32 + 1.0, h: (u % 16) as f32 + 1.0 };
                true
            }
            3 => {
                s = CanvasSource { image: 0, canvas: 0, w: (u % 100) as f32 + 1.0, h: (u % 70) as f32 + 1.0 };
                true
            }
            4 => {
                s = CanvasSource { image: 0, canvas: 0, w: 0.0, h: 10.0 };
                true
            }
            5 => {
                s = CanvasSource { image: 7, canvas: 0, w: f32::NAN, h: 5.0 };
                true
            }
            6 => {
                s = CanvasSource { image: 0, canvas: 0, w: 1e30, h: 0.5 };
                true
            }
            _ => {
                s = CanvasSource { image: 0, canvas: 0, w: 48.0, h: 48.0 };
                true
            }
        };
        let log = &mut self.log;
        log.u8(b'N');
        log.i64(node);
        log.u8(u8::from(found));
        log.i32(s.image);
        log.i32(s.canvas);
        log.f32(s.w);
        log.f32(s.h);
        found.then_some(s)
    }

    fn canvas_allowance(&mut self, id: i32) -> i64 {
        self.log.u8(b'A');
        self.log.i32(id);
        self.log.i64(self.allowance);
        self.allowance
    }

    fn canvas_path_floats(&mut self, id: i32) -> usize {
        self.log.u8(b'P');
        self.log.i32(id);
        self.log.u64(self.others);
        self.others as usize
    }
}

#[derive(Clone, Debug)]
pub enum Step {
    /// A `canvas` op's command list, as JSON text.
    Run(Vec<u8>),
    Resize(i32, i32),
    /// A `canvasCall`: the method and its arguments as JSON text.
    Call(Vec<u8>, Vec<u8>),
    Trim,
    Allowance(i64),
    Others(u64),
    /// `measureText`: the font and the text.
    Measure(Vec<u8>, Vec<u8>),
    /// The `imageData` query as JSON text, and how the backend reads (0 no canvas, 1 pixels, 2 refused, 3 a byte short).
    ImageData(Vec<u8>, u8),
}

/// Steps through one canvas, made with a size, under a host with an allowance and other canvases' path floats.
#[derive(Clone, Debug)]
pub struct Script {
    pub id: i32,
    pub width: i32,
    pub height: i32,
    pub allowance: i64,
    pub others: u64,
    pub digest: bool,
    pub steps: Vec<Step>,
}

impl Script {
    pub fn new(steps: Vec<Step>) -> Script {
        Script { id: 7, width: 300, height: 150, allowance: Canvas::MAX_PIXELS, others: 0, digest: false, steps }
    }

    pub fn runs(cmds: &[Vec<u8>]) -> Script {
        Script::new(cmds.iter().map(|c| Step::Run(c.clone())).collect())
    }

    /// The record `oracle/canvas_oracle` takes.
    pub fn input(&self) -> Vec<u8> {
        let mut t = Transcript::default();
        t.i32(self.id);
        t.i32(self.width);
        t.i32(self.height);
        t.i64(self.allowance);
        t.u64(self.others);
        t.u8(u8::from(self.digest));
        t.u32(self.steps.len() as u32);
        for step in &self.steps {
            match step {
                Step::Run(text) => {
                    t.u8(b'r');
                    t.str(text);
                }
                Step::Resize(w, h) => {
                    t.u8(b'z');
                    t.i32(*w);
                    t.i32(*h);
                }
                Step::Call(method, args) => {
                    t.u8(b'c');
                    t.str(method);
                    t.str(args);
                }
                Step::Trim => t.u8(b't'),
                Step::Allowance(v) => {
                    t.u8(b'a');
                    t.i64(*v);
                }
                Step::Others(v) => {
                    t.u8(b'o');
                    t.u64(*v);
                }
                Step::Measure(font, text) => {
                    t.u8(b'm');
                    t.str(font);
                    t.str(text);
                }
                Step::ImageData(query, mode) => {
                    t.u8(b'i');
                    t.str(query);
                    t.u8(*mode);
                }
            }
        }
        twidget_testkit::framed(b'S', &t.out)
    }

    /// What the Rust canvas writes of the script, as the oracle does.
    pub fn transcript(&self) -> Vec<u8> {
        let mut host =
            ScriptHost { log: Transcript { out: Vec::new(), digest: self.digest }, allowance: self.allowance, others: self.others };
        let mut canvas = Canvas::new(self.id, self.width, self.height, &mut host);
        state(&mut host.log, &canvas);
        for step in &self.steps {
            match step {
                Step::Run(text) => {
                    let cmds = Json::parse(text);
                    host.log.u8(b'R');
                    host.log.u8(u8::from(cmds.is_some()));
                    let Some(cmds) = cmds else { continue };
                    let mut ops = Vec::new();
                    canvas.run(&mut host, &cmds, &mut ops);
                    batch(&mut host.log, &ops);
                    state(&mut host.log, &canvas);
                }
                Step::Resize(w, h) => {
                    let mut ops = Vec::new();
                    host.log.u8(b'Z');
                    canvas.resize(&mut host, *w, *h, &mut ops);
                    batch(&mut host.log, &ops);
                    state(&mut host.log, &canvas);
                }
                Step::Call(method, args) => {
                    let args = Json::parse(args);
                    host.log.u8(b'C');
                    host.log.u8(u8::from(args.is_some()));
                    if let Some(args) = args {
                        let dump = canvas.call(method, &args).dump();
                        host.log.str(&dump);
                    }
                }
                Step::Trim => {
                    canvas.trim();
                    host.log.u8(b'T');
                }
                Step::Allowance(v) => host.allowance = *v,
                Step::Others(v) => host.others = *v,
                Step::Measure(font, text) => {
                    host.log.u8(b'M');
                    let dump = measure_text(&mut host, font, text).dump();
                    host.log.str(&dump);
                }
                Step::ImageData(query, mode) => {
                    let q = Json::parse(query);
                    host.log.u8(b'I');
                    host.log.u8(u8::from(q.is_some()));
                    let Some(q) = q else { continue };
                    let mode = *mode;
                    let mut reads = Vec::new();
                    let mut read = |x: i32, y: i32, w: i32, h: i32, out: &mut Vec<u8>| {
                        let mut n = w as usize * h as usize * 4;
                        if mode == 3 {
                            n = if n > 0 { n - 1 } else { 1 };
                        }
                        if mode != 2 {
                            out.extend((0..n).map(|i| (i * 7).wrapping_add(x as usize).wrapping_add(y as usize) as u8));
                        }
                        reads.push((x, y, w, h, mode != 2));
                        mode != 2
                    };
                    let (answer, binary) = image_data(&q, if mode == 0 { None } else { Some(&mut read) });
                    for (x, y, w, h, ok) in reads {
                        host.log.u8(b'B');
                        for v in [x, y, w, h] {
                            host.log.i32(v);
                        }
                        host.log.u8(u8::from(ok));
                    }
                    host.log.str(&answer.dump());
                    host.log.u8(u8::from(binary.is_some()));
                    if let Some(b) = binary {
                        host.log.payload(b.as_bytes());
                    }
                }
            }
        }
        host.log.out
    }
}

fn state(t: &mut Transcript, c: &Canvas) {
    t.i32(c.width());
    t.i32(c.height());
    t.i64(c.pixels());
    t.u64(c.path_floats() as u64);
}

fn canonical(v: &mut f32) {
    if v.is_nan() {
        *v = f32::NAN;
    }
}

/// `ops` with every NaN as 0x7fc00000, shared clip lists still shared: which NaN an operation gives is the compiler's
/// choice (operand order, and LLVM folds -x / 2 into x / -2, which keeps x's sign), not the source's.
fn canonical_batch(ops: &[CanvasOp]) -> Vec<CanvasOp> {
    let mut clips: HashMap<*const Clips, Arc<Clips>> = HashMap::new();
    let mut out = ops.to_vec();
    for op in &mut out {
        op.path.points.iter_mut().for_each(canonical);
        op.paint.args.iter_mut().for_each(canonical);
        op.paint.stops.iter_mut().for_each(|s| canonical(&mut s.offset));
        op.matrix.iter_mut().for_each(canonical);
        op.dashes.iter_mut().for_each(canonical);
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
            canonical(v);
        }
        if let Some(list) = &op.clips {
            let made = clips.entry(Arc::as_ptr(list)).or_insert_with(|| {
                let mut copy = (**list).clone();
                for c in &mut copy {
                    c.path.points.iter_mut().for_each(canonical);
                }
                Arc::new(copy)
            });
            op.clips = Some(Arc::clone(made));
        }
    }
    out
}

fn batch(t: &mut Transcript, ops: &[CanvasOp]) {
    let mut bytes = Vec::new();
    if !encode_batch(&canonical_batch(ops), &Caps::default(), &mut bytes) {
        return t.u8(0);
    }
    t.u8(1);
    t.payload(&bytes);
}

// ---- Command text ---------------------------------------------------------------------------------------

pub fn pick<'a>(random: &mut Lcg, items: &[&'a str]) -> &'a str {
    items[random.below(items.len())]
}

/// JSON numbers past what an f64 holds read as infinities; past what a float holds become a float's infinity.
pub const ODD_NUMBERS: [&str; 18] = [
    "1e999",
    "-1e999",
    "1e308",
    "-1e308",
    "0",
    "-0",
    "4.9e-324",
    "1e-400",
    "2147483648",
    "-2147483649",
    "1e300",
    "-1e300",
    "3.5e38",
    "1e39",
    "0.1",
    "3.141592653589793",
    "6.283185307179586",
    "-1.5707963267948966",
];

fn number(v: f64) -> String {
    format!("{v}")
}

/// A coordinate in canvas px: mostly a few hundred on an 1/8 grid, sometimes odd.
pub fn coord(random: &mut Lcg) -> String {
    match random.below(30) {
        0 => pick(random, &ODD_NUMBERS).to_string(),
        1 => number((random.below(2_000_000) as f64 - 1_000_000.0) / 7.0),
        _ => number((random.below(4800) as f64 - 800.0) / 8.0),
    }
}

pub fn small(random: &mut Lcg) -> String {
    match random.below(25) {
        0 => pick(random, &ODD_NUMBERS).to_string(),
        _ => number((random.below(800) as f64 - 300.0) / 100.0),
    }
}

pub fn angle(random: &mut Lcg) -> String {
    match random.below(12) {
        0 => pick(random, &ODD_NUMBERS).to_string(),
        1 => {
            pick(random, &["0", "6.283185307179586", "-6.283185307179586", "3.141592653589793", "12.566370614359172", "1.5707963267948966"])
                .to_string()
        }
        _ => number((random.below(1600) as f64 - 400.0) / 100.0),
    }
}

pub fn quoted(s: &str) -> String {
    String::from_utf8(twidget_json::quote(s.as_bytes())).unwrap_or_default()
}

pub fn color(random: &mut Lcg) -> String {
    match random.below(8) {
        0 => format!("hsl({},70%,50%)", random.below(360)),
        1 => format!("rgba({},{},{},{})", random.below(256), random.below(256), random.below(256), random.below(11) as f64 / 10.0),
        2 => format!("#{:06x}", random.next() & 0xffffff),
        _ => pick(
            random,
            &[
                "#f00",
                "#ff000080",
                "red",
                "Blue",
                "rgba(0, 0, 0, 0.5)",
                "rgb(300,-5,20)",
                "transparent",
                "currentColor",
                "bogus",
                "",
                "#12345",
                " Red ",
                "#000000",
                "rgba(0, 0, 0, 0)",
                "hsla(120, 100%, 50%, .3)",
                "rgb(10% 20% 30% / 50%)",
            ],
        )
        .to_string(),
    }
}

pub fn font(random: &mut Lcg) -> String {
    match random.below(12) {
        0 => format!("{}px x{}", random.below(40), "y".repeat(510)),
        1 => format!("{}px f{}", random.below(40), random.below(1000)),
        _ => pick(
            random,
            &[
                "10px sans-serif",
                "bold 12px serif",
                "italic 20px 'Fancy Serif', serif",
                "",
                "12px system-ui,sans-serif",
                "600 14px -apple-system",
                "x",
                "16px monospace",
                "small-caps 9px Arial",
            ],
        )
        .to_string(),
    }
}

pub fn text(random: &mut Lcg) -> String {
    match random.below(14) {
        0 => {
            // Past MAX_TEXT, a multi-byte character across the cut.
            let mut s = "a".repeat(4095 - random.below(3));
            s.push_str("Жжж日本");
            s
        }
        1 => "Ж".repeat(2049),
        _ => pick(random, &["hi", "", "!nan", "#inf", "~neg", "Жжж", "Hello, world", "0", "a\"b\\c", "\u{1F600}", "€uro", " ", "Σ12"])
            .to_string(),
    }
}

fn path_id(random: &mut Lcg) -> String {
    match random.below(10) {
        0 => pick(random, &["0", "-1", "1e300", "-1e300", "2147483648", "\"1\"", "1.5"]).to_string(),
        _ => (1 + random.below(6)).to_string(),
    }
}

fn node_ref(random: &mut Lcg) -> String {
    match random.below(10) {
        0 => format!("{{\"n\":{}}}", pick(random, &["1e300", "-1e300", "-1", "\"5\"", "null", "4294967306"])),
        _ => format!("{{\"n\":{}}}", random.below(64)),
    }
}

fn pixels(random: &mut Lcg) -> String {
    let (w, h) = match random.below(8) {
        0 => (pick(random, &["0", "-1", "9000", "4097", "1e300", "\"2\""]).to_string(), (1 + random.below(3)).to_string()),
        _ => ((1 + random.below(4)).to_string(), (1 + random.below(4)).to_string()),
    };
    let n = (w.parse::<usize>().unwrap_or(1).min(4)) * (h.parse::<usize>().unwrap_or(1).min(4)) * 4;
    let n = match random.below(5) {
        0 => n.saturating_sub(1 + random.below(3)),
        1 => n + random.below(5),
        _ => n,
    };
    let bytes: Vec<u8> = (0..n).map(|_| random.next() as u8).collect();
    let mut b64 = twidget_base64::encode(&bytes);
    if random.chance(6) {
        b64.insert_str(random.below(b64.len() + 1), pick(random, &[" ", "\n", "=", "*", "é"]));
    }
    match random.below(12) {
        0 => format!("{{\"w\":{w},\"h\":{h},\"b64\":5}}"),
        1 => format!("{{\"b64\":{},\"w\":{w}}}", quoted(&b64)),
        _ => format!("{{\"w\":{w},\"h\":{h},\"b64\":{}}}", quoted(&b64)),
    }
}

/// An argument of any kind, for where a method expects one kind.
fn any_arg(random: &mut Lcg) -> String {
    match random.below(12) {
        0 => quoted(&text(random)),
        1 => pick(random, &["true", "false", "null"]).to_string(),
        2 => format!("[{},{}]", coord(random), small(random)),
        3 => format!("{{\"g\":{}}}", path_id(random)),
        4 => format!("{{\"p\":{}}}", path_id(random)),
        5 => node_ref(random),
        6 => "{}".to_string(),
        _ => coord(random),
    }
}

fn args(random: &mut Lcg, mut each: impl FnMut(&mut Lcg) -> String, n: usize) -> Vec<String> {
    let mut out: Vec<String> = (0..n).map(|_| each(random)).collect();
    match random.below(20) {
        0 if !out.is_empty() => {
            out.pop();
        }
        1 => out.push(any_arg(random)),
        2 if !out.is_empty() => {
            let at = random.below(out.len());
            out[at] = any_arg(random);
        }
        _ => {}
    }
    out
}

/// An SVG path text, as a Path2D is made from or a widget writes one.
pub fn svg_path(random: &mut Lcg) -> String {
    let mut d = String::new();
    let num = |random: &mut Lcg| match random.below(30) {
        0 => pick(random, &["1e2", ".5", "1.2.3", "-.5", "+3", "1e", "e5", "0x10", "1e999", "-0", "5-3", "1e-2-1"]).to_string(),
        _ => number((random.below(2400) as f64 - 400.0) / 8.0),
    };
    let sep = |random: &mut Lcg| pick(random, &[" ", ",", " ", ", ", "\n", "\t", ""]).to_string();
    for _ in 0..random.below(9) {
        let letter =
            pick(random, &["M", "L", "H", "V", "C", "S", "Q", "T", "A", "Z", "m", "l", "h", "v", "c", "s", "q", "t", "a", "z", "X", "e"]);
        d.push_str(letter);
        let count = match letter.to_ascii_uppercase().as_str() {
            "M" | "L" | "T" => 2,
            "H" | "V" => 1,
            "C" => 6,
            "S" | "Q" => 4,
            "A" => 7,
            _ => 0,
        };
        let repeats = if random.chance(4) { 2 } else { 1 };
        for _ in 0..count * repeats {
            d.push_str(&sep(random));
            if letter.eq_ignore_ascii_case("A") && random.chance(2) {
                d.push_str(pick(random, &["0", "1", "0", "1", "2"]));
            } else {
                d.push_str(&num(random));
            }
        }
        d.push_str(&sep(random));
    }
    if random.chance(30) {
        d.push_str(pick(random, &["Ж", "\u{1}", "M", "1", "-"]));
    }
    d
}

const METHODS: [&str; 38] = [
    "fillRect",
    "strokeRect",
    "clearRect",
    "rect",
    "roundRect",
    "moveTo",
    "lineTo",
    "bezierCurveTo",
    "quadraticCurveTo",
    "arc",
    "arcTo",
    "ellipse",
    "closePath",
    "beginPath",
    "fill",
    "stroke",
    "clip",
    "save",
    "restore",
    "scale",
    "rotate",
    "translate",
    "transform",
    "setTransform",
    "resetTransform",
    "setLineDash",
    "fillText",
    "strokeText",
    "drawImage",
    "putImageData",
    "addPath",
    "reset",
    "fillrect",
    "",
    "measureText",
    "getTransform",
    "isPointInPath",
    "drawFocusIfNeeded",
];

/// A method call's name and arguments, as canvas.ts writes them (and not quite).
pub fn method_call(random: &mut Lcg, for_path: bool) -> (String, Vec<String>) {
    let name = if for_path {
        pick(
            random,
            &[
                "moveTo",
                "lineTo",
                "bezierCurveTo",
                "quadraticCurveTo",
                "arc",
                "arcTo",
                "ellipse",
                "rect",
                "roundRect",
                "closePath",
                "addPath",
                "fill",
            ],
        )
    } else {
        METHODS[random.below(METHODS.len())]
    };
    let a = match name {
        "fillRect" | "strokeRect" | "clearRect" | "rect" => args(random, coord, 4),
        "roundRect" => {
            let mut a = args(random, coord, 4);
            match random.below(4) {
                0 => {}
                1 => a.push(format!("[{},{}]", small(random), small(random))),
                2 => a.push(pick(random, &["[]", "[1e999]", "\"5\"", "-3"]).to_string()),
                _ => a.push(number(random.below(40) as f64 / 2.0)),
            }
            a
        }
        "moveTo" | "lineTo" => args(random, coord, 2),
        "bezierCurveTo" => args(random, coord, 6),
        "quadraticCurveTo" => args(random, coord, 4),
        "arc" => {
            let mut a = vec![coord(random), coord(random), number(random.below(400) as f64 / 4.0 - 5.0), angle(random), angle(random)];
            if random.chance(2) {
                a.push(pick(random, &["true", "false", "1", "0", "\"false\"", "\"\"", "\"yes\""]).to_string());
            }
            a
        }
        "arcTo" => {
            let mut a = args(random, coord, 4);
            a.push(number(random.below(200) as f64 / 4.0 - 2.0));
            a
        }
        "ellipse" => {
            let mut a = vec![
                coord(random),
                coord(random),
                number(random.below(400) as f64 / 4.0 - 3.0),
                number(random.below(400) as f64 / 4.0 - 3.0),
            ];
            a.extend([angle(random), angle(random), angle(random)]);
            if random.chance(2) {
                a.push(pick(random, &["true", "false", "1"]).to_string());
            }
            a
        }
        "fill" | "clip" => {
            let mut a = Vec::new();
            if random.chance(2) {
                a.push(format!("{{\"p\":{}}}", path_id(random)));
            }
            if random.chance(2) {
                a.push(quoted(pick(random, &["evenodd", "nonzero", "EVENODD"])));
            }
            a
        }
        "stroke" | "addPath" => {
            if random.chance(3) {
                Vec::new()
            } else {
                vec![format!("{{\"p\":{}}}", path_id(random))]
            }
        }
        "scale" => args(random, small, 2),
        "rotate" => args(random, angle, 1),
        "translate" => args(random, coord, 2),
        "transform" => args(random, small, 6),
        "setTransform" => match random.below(4) {
            0 => Vec::new(),
            1 => vec![format!(
                "[{},{},{},{},{},{}]",
                small(random),
                small(random),
                small(random),
                small(random),
                coord(random),
                coord(random)
            )],
            _ => args(random, small, 6),
        },
        "setLineDash" => {
            let n = random.below(7);
            let items: Vec<String> = (0..n)
                .map(|_| {
                    if random.chance(10) {
                        pick(random, &["-1", "1e999", "\"x\""]).to_string()
                    } else {
                        number(random.below(40) as f64 / 2.0)
                    }
                })
                .collect();
            if random.chance(20) { vec![format!("[{}]", vec!["1"; 65].join(","))] } else { vec![format!("[{}]", items.join(","))] }
        }
        "fillText" | "strokeText" => {
            let mut a = vec![quoted(&text(random)), coord(random), coord(random)];
            if random.chance(3) {
                a.push(pick(random, &["0", "-1", "5", "20", "1000", "1e999", "\"5\"", "null"]).to_string());
            }
            a
        }
        "drawImage" => {
            let n = [2, 4, 8, 8, 4, 2, 0, 1, 3, 5, 9][random.below(11)];
            let mut a = vec![node_ref(random)];
            for _ in 0..n {
                a.push(if random.chance(8) {
                    pick(random, &["0", "-10", "1e999", "\"1\""]).to_string()
                } else {
                    number(random.below(200) as f64 / 2.0 - 10.0)
                });
            }
            a
        }
        "putImageData" => vec![pixels(random), coord(random), coord(random)],
        _ => {
            let n = random.below(3);
            args(random, any_arg, n)
        }
    };
    (name.to_string(), a)
}

const PROPERTIES: [&str; 21] = [
    "fillStyle",
    "strokeStyle",
    "lineWidth",
    "lineCap",
    "lineJoin",
    "miterLimit",
    "lineDashOffset",
    "font",
    "textAlign",
    "textBaseline",
    "globalAlpha",
    "globalCompositeOperation",
    "shadowBlur",
    "shadowColor",
    "shadowOffsetX",
    "shadowOffsetY",
    "imageSmoothingEnabled",
    "filter",
    "direction",
    "",
    "FONT",
];

pub fn property(random: &mut Lcg) -> (String, String) {
    let name = PROPERTIES[random.below(PROPERTIES.len())];
    let value = if random.chance(15) {
        any_arg(random)
    } else {
        match name {
            "fillStyle" | "strokeStyle" => {
                if random.chance(3) {
                    format!("{{\"g\":{}}}", path_id(random))
                } else {
                    quoted(&color(random))
                }
            }
            "shadowColor" => quoted(&color(random)),
            "lineWidth" | "miterLimit" | "shadowBlur" => {
                pick(random, &["1", "2.5", "0", "-1", "10", "150", "1e999", "0.001", "4"]).to_string()
            }
            "lineDashOffset" | "shadowOffsetX" | "shadowOffsetY" => small(random),
            "lineCap" => quoted(pick(random, &["butt", "round", "square", "Round", ""])),
            "lineJoin" => quoted(pick(random, &["miter", "round", "bevel", "x"])),
            "font" | "FONT" => quoted(&font(random)),
            "textAlign" => quoted(pick(random, &["start", "end", "left", "right", "center", "middle"])),
            "textBaseline" => quoted(pick(random, &["alphabetic", "top", "hanging", "middle", "ideographic", "bottom", "center"])),
            "globalAlpha" => pick(random, &["1", "0.5", "0", "1.5", "-0.1", "1e999", "0.25"]).to_string(),
            "globalCompositeOperation" => {
                quoted(pick(random, &["source-over", "multiply", "xor", "luminosity", "copy", "lighter", "Copy", "darker"]))
            }
            "imageSmoothingEnabled" => pick(random, &["true", "false", "0", "1", "\"x\""]).to_string(),
            _ => quoted(pick(random, &["none", "ltr", "blur(2px)"])),
        }
    };
    (name.to_string(), value)
}

fn gradient(random: &mut Lcg) -> String {
    let kind = pick(random, &["\"linear\"", "\"linear\"", "\"radial\"", "\"conic\"", "\"pattern\"", "\"LINEAR\"", "5", "\"\""]);
    let n = match random.below(4) {
        0 => random.below(8),
        _ => match kind {
            "\"linear\"" => 4,
            "\"radial\"" => 6,
            "\"conic\"" => 3,
            _ => 2,
        },
    };
    let mut a = vec!["\"g\"".to_string(), path_id(random), kind.to_string()];
    for i in 0..n {
        a.push(if kind == "\"pattern\"" && i == 0 {
            node_ref(random)
        } else if random.chance(4) {
            random.below(5).to_string()
        } else {
            coord(random)
        });
    }
    format!("[{}]", a.join(","))
}

/// A few commands as a widget writes them together: a gradient made and painted, a path built and drawn or clipped
/// to, a Path2D, styled text, a transformed group.
pub fn recipe(random: &mut Lcg) -> String {
    let id = 1 + random.below(6);
    let mut list: Vec<String> = Vec::new();
    match random.below(5) {
        0 => {
            let (kind, args) = match random.below(4) {
                0 => ("linear", vec![coord(random), coord(random), coord(random), coord(random)]),
                1 => ("radial", vec![coord(random), coord(random), small(random), coord(random), coord(random), coord(random)]),
                2 => ("conic", vec![angle(random), coord(random), coord(random)]),
                _ => ("linear", vec!["0".into(), "0".into(), "0".into(), "0".into()]),
            };
            list.push(format!("[\"g\",{id},\"{kind}\",{}]", args.join(",")));
            for _ in 0..random.below(4) {
                list.push(format!("[\"gs\",{id},{},{}]", number(random.below(11) as f64 / 10.0), quoted(&color(random))));
            }
            let style = pick(random, &["fillStyle", "strokeStyle"]);
            list.push(format!("[\"s\",\"{style}\",{{\"g\":{id}}}]"));
            list.push(
                pick(
                    random,
                    &[
                        "[\"c\",\"fillRect\",0,0,50,40]",
                        "[\"c\",\"strokeRect\",5,5,30,30]",
                        "[\"c\",\"fillText\",\"hi\",3,20]",
                        "[\"c\",\"strokeText\",\"hi\",3,20]",
                    ],
                )
                .to_string(),
            );
        }
        1 => {
            list.push("[\"c\",\"beginPath\"]".into());
            for _ in 0..1 + random.below(4) {
                let (name, a) = method_call(random, true);
                if name == "fill" {
                    continue;
                }
                let mut items = vec!["\"c\"".to_string(), quoted(&name)];
                items.extend(a);
                list.push(format!("[{}]", items.join(",")));
            }
            list.push(
                pick(
                    random,
                    &[
                        "[\"c\",\"fill\"]",
                        "[\"c\",\"fill\",\"evenodd\"]",
                        "[\"c\",\"stroke\"]",
                        "[\"c\",\"clip\"]",
                        "[\"c\",\"clip\",\"evenodd\"]",
                    ],
                )
                .to_string(),
            );
            if random.chance(2) {
                list.push("[\"c\",\"fillRect\",0,0,100,100]".into());
            }
        }
        2 => {
            list.push(format!("[\"pn\",{id},{}]", if random.chance(4) { "null".to_string() } else { quoted(&svg_path(random)) }));
            for _ in 0..random.below(4) {
                let (name, a) = method_call(random, true);
                let mut items = vec!["\"pc\"".to_string(), id.to_string(), quoted(&name)];
                items.extend(a);
                list.push(format!("[{}]", items.join(",")));
            }
            list.push(format!("[\"c\",\"{}\",{{\"p\":{id}}}]", pick(random, &["fill", "stroke", "clip", "addPath"])));
        }
        3 => {
            list.push(format!("[\"s\",\"font\",{}]", quoted(&font(random))));
            list.push(format!("[\"s\",\"textAlign\",\"{}\"]", pick(random, &["start", "end", "left", "right", "center"])));
            list.push(format!(
                "[\"s\",\"textBaseline\",\"{}\"]",
                pick(random, &["alphabetic", "top", "hanging", "middle", "ideographic", "bottom"])
            ));
            let max = if random.chance(2) { format!(",{}", random.below(60)) } else { String::new() };
            list.push(format!(
                "[\"c\",\"{}\",{},{},{}{max}]",
                pick(random, &["fillText", "strokeText"]),
                quoted(&text(random)),
                coord(random),
                coord(random)
            ));
        }
        _ => {
            list.push("[\"c\",\"save\"]".into());
            list.push(format!("[\"c\",\"translate\",{},{}]", random.below(100), random.below(100)));
            list.push(format!("[\"c\",\"rotate\",{}]", angle(random)));
            list.push(format!("[\"c\",\"scale\",{},{}]", small(random), small(random)));
            list.push("[\"c\",\"fillRect\",-10,-10,20,20]".into());
            if random.chance(2) {
                list.push("[\"c\",\"restore\"]".into());
            }
        }
    }
    list.join(",")
}

/// One command of a `canvas` op's list, or a few written together.
pub fn command(random: &mut Lcg) -> String {
    if random.chance(6) {
        return recipe(random);
    }
    match random.below(40) {
        0..=21 => {
            let (name, a) = method_call(random, false);
            let mut items = vec!["\"c\"".to_string(), quoted(&name)];
            items.extend(a);
            format!("[{}]", items.join(","))
        }
        22..=29 => {
            let (name, value) = property(random);
            format!("[\"s\",{},{}]", quoted(&name), value)
        }
        30 => format!(
            "[\"r\",{},{}]",
            pick(random, &["300", "100", "0", "-5", "9000", "4096", "50", "1e300"]),
            pick(random, &["150", "100", "0", "4097", "20"])
        ),
        31 | 32 => gradient(random),
        33 | 34 => {
            let offset = pick(random, &["0", "1", "0.5", "-1", "2", "0.25", "0.5", "1e999", "\"x\""]);
            format!("[\"gs\",{},{},{}]", path_id(random), offset, quoted(&color(random)))
        }
        35 | 36 => {
            let d = match random.below(8) {
                0 => "null".to_string(),
                1 => "5".to_string(),
                _ => quoted(&svg_path(random)),
            };
            format!("[\"pn\",{},{}]", path_id(random), d)
        }
        37 | 38 => {
            let (name, a) = method_call(random, true);
            let mut items = vec!["\"pc\"".to_string(), path_id(random), quoted(&name)];
            items.extend(a);
            format!("[{}]", items.join(","))
        }
        _ => pick(
            random,
            &[
                "[]",
                "[\"c\"]",
                "[1,2]",
                "\"x\"",
                "{\"a\":1}",
                "[\"c\",5,1]",
                "[\"s\",\"fillStyle\"]",
                "[\"zz\",1]",
                "[null,1]",
                "[\"g\"]",
                "[\"gs\",1]",
                "[\"pc\",1]",
                "[\"r\"]",
                "[\"r\",\"5\"]",
                "7",
                "null",
                "[\"pn\"]",
            ],
        )
        .to_string(),
    }
}

/// A `canvas` op's list of `n` commands.
pub fn commands(random: &mut Lcg, n: usize) -> Vec<u8> {
    let list: Vec<String> = (0..n).map(|_| command(random)).collect();
    format!("[{}]", list.join(",")).into_bytes()
}

/// A `canvasCall`'s method and arguments.
pub fn query(random: &mut Lcg) -> (Vec<u8>, Vec<u8>) {
    let method = pick(
        random,
        &["getTransform", "getLineDash", "isPointInPath", "isPointInPath", "isPointInStroke", "isPointInStroke", "toDataURL", ""],
    );
    let mut a = Vec::new();
    if random.chance(2) {
        a.push(format!("{{\"p\":{}}}", path_id(random)));
    }
    for _ in 0..2 {
        a.push(if random.chance(3) { coord(random) } else { number(random.below(960) as f64 / 8.0) });
    }
    if random.chance(3) {
        a.push(quoted(pick(random, &["evenodd", "nonzero"])));
    }
    if random.chance(15) {
        a = vec![any_arg(random)];
    }
    let text = if random.chance(30) { "{}".to_string() } else { format!("[{}]", a.join(",")) };
    (method.as_bytes().to_vec(), text.into_bytes())
}

/// A script of runs, queries and size changes on one canvas.
pub fn any_script(random: &mut Lcg) -> Script {
    let mut steps = Vec::new();
    for _ in 0..1 + random.below(6) {
        let step = match random.below(14) {
            0..=7 => {
                let n = 1 + random.below(40);
                Step::Run(commands(random, n))
            }
            8 | 9 => {
                if random.chance(2) {
                    // A shape to hit and a point near it: the current path or a Path2D, maybe under a transform.
                    let id = 1 + random.below(3);
                    let (shape, by_ref) = match random.below(5) {
                        0 => (
                            format!("[\"c\",\"rect\",{},{},{},{}]", random.below(40), random.below(40), random.below(90), random.below(90)),
                            false,
                        ),
                        1 => (format!("[\"c\",\"arc\",50,50,{},0,6.283185307179586]", random.below(60)), false),
                        2 => (format!("[\"pn\",{id},{}]", quoted(&svg_path(random))), true),
                        3 => (format!("[\"pn\",{id},\"M0 0L100 0L{} 100Z M20 20h50v50h-50z\"]", random.below(120)), true),
                        _ => (
                            format!(
                                "[\"c\",\"moveTo\",0,0],[\"c\",\"quadraticCurveTo\",{},0,100,100],[\"c\",\"bezierCurveTo\",0,100,50,{},0,0]",
                                random.below(200),
                                random.below(200)
                            ),
                            false,
                        ),
                    };
                    let transform = pick(
                        random,
                        &[
                            "",
                            "",
                            "[\"c\",\"translate\",10,5],",
                            "[\"c\",\"scale\",2,0.5],",
                            "[\"c\",\"rotate\",0.5],",
                            "[\"s\",\"lineWidth\",6],",
                        ],
                    );
                    steps.push(Step::Run(format!("[[\"c\",\"beginPath\"],{transform}{shape}]").into_bytes()));
                    let method = pick(random, &["isPointInPath", "isPointInStroke"]);
                    let mut a = Vec::new();
                    if by_ref {
                        a.push(format!("{{\"p\":{id}}}"));
                    }
                    for _ in 0..2 {
                        // On a rectangle's edges a stroke is hit.
                        a.push(if method == "isPointInStroke" {
                            random.below(101).to_string()
                        } else {
                            number(random.below(880) as f64 / 8.0)
                        });
                    }
                    if random.chance(2) {
                        a.push(quoted(pick(random, &["evenodd", "nonzero"])));
                    }
                    Step::Call(method.as_bytes().to_vec(), format!("[{}]", a.join(",")).into_bytes())
                } else {
                    let (m, a) = query(random);
                    Step::Call(m, a)
                }
            }
            10 => {
                Step::Resize([300, 0, -5, 100, 9000, 4096, 4097, 64][random.below(8)], [150, 0, 100, 4096, 4097, -1, 64][random.below(7)])
            }
            11 => match random.below(3) {
                0 => Step::Trim,
                1 => Step::Allowance([Canvas::MAX_PIXELS, 0, 10_000, -1, i64::MAX, i64::MIN, 45_000][random.below(7)]),
                _ => Step::Others([0, (1 << 22) - 10, 1 << 22, u64::MAX, 100_000][random.below(5)]),
            },
            12 => Step::Measure(font(random).into_bytes(), text(random).into_bytes()),
            _ => {
                let mut q = String::from("{\"q\":\"imageData\"");
                for key in ["x", "y", "w", "h"] {
                    if !random.chance(10) {
                        let v = match key {
                            "w" | "h" if random.chance(10) => pick(random, &["8192", "9000", "-1", "1e300", "4097", "\"2\""]).to_string(),
                            "w" | "h" => random.below(6).to_string(),
                            _ => (random.below(20) as i32 - 5).to_string(),
                        };
                        let _ = write!(q, ",\"{key}\":{v}");
                    }
                }
                q.push('}');
                Step::ImageData(q.into_bytes(), random.below(4) as u8)
            }
        };
        steps.push(step);
    }
    let mut script = Script::new(steps);
    match random.below(12) {
        0 => (script.width, script.height) = (0, 0),
        1 => (script.width, script.height) = (5000, 5000),
        2 => (script.width, script.height) = (-1, 9000),
        3 => (script.width, script.height) = (100, 100),
        _ => {}
    }
    if random.chance(10) {
        script.allowance = [0, 10_000, -1, i64::MAX, i64::MIN][random.below(5)];
    }
    if random.chance(10) {
        script.others = [(1 << 22) - 100, 1 << 22, u64::MAX][random.below(3)];
    }
    if random.chance(20) {
        script.id = [0, -1, i32::MAX][random.below(3)];
    }
    script
}

/// widget-sandbox's stress widget `canvas-cmds` at `n` commands per frame, frame at `t` ms: clearRect, then a
/// fillStyle and a fillRect per square.
pub fn stress_frame(n: usize, t: f64) -> Vec<u8> {
    let mut list = vec!["[\"c\",\"clearRect\",0,0,600,600]".to_string()];
    for i in 0..n / 2 {
        let i = i as f64;
        let hue = ((i * 7.0 + t / 10.0) % 360.0) as i64;
        list.push(format!("[\"s\",\"fillStyle\",\"hsl({hue},70%,50%)\"]"));
        list.push(format!("[\"c\",\"fillRect\",{},{},16,16]", number((i * 37.0 + t / 8.0) % 580.0), number((i * 53.0) % 580.0)));
    }
    format!("[{}]", list.join(",")).into_bytes()
}

/// A chart as the corpus's canvas widgets draw one: the device pixel ratio's transform, a cleared frame, gridlines,
/// bars with a gradient, a line with dots, labels measured and aligned.
pub fn chart_frame(random: &mut Lcg) -> Vec<u8> {
    let dpr = [1, 2, 3][random.below(3)];
    let mut list = vec![
        format!("[\"r\",{},{}]", 300 * dpr, 220 * dpr),
        format!("[\"c\",\"setTransform\",{dpr},0,0,{dpr},0,0]"),
        "[\"c\",\"clearRect\",0,0,300,220]".to_string(),
        "[\"s\",\"font\",\"12px system-ui,sans-serif\"]".to_string(),
        "[\"s\",\"textBaseline\",\"middle\"]".to_string(),
        "[\"s\",\"strokeStyle\",\"rgba(0,0,0,0.1)\"]".to_string(),
        "[\"s\",\"lineWidth\",1]".to_string(),
    ];
    for k in 0..5 {
        let y = 20 + k * 40;
        list.push("[\"c\",\"beginPath\"]".to_string());
        list.push(format!("[\"c\",\"moveTo\",30,{y}.5]"));
        list.push(format!("[\"c\",\"lineTo\",290,{y}.5]"));
        list.push("[\"c\",\"stroke\"]".to_string());
        list.push(format!("[\"c\",\"fillText\",\"{}\",4,{y}]", 100 - k * 25));
    }
    let gid = 1 + random.below(3);
    list.push(format!("[\"g\",{gid},\"linear\",0,20,0,200]"));
    list.push(format!("[\"gs\",{gid},0,\"#4a90e2\"]"));
    list.push(format!("[\"gs\",{gid},1,\"rgba(74,144,226,0.2)\"]"));
    list.push(format!("[\"s\",\"fillStyle\",{{\"g\":{gid}}}]"));
    let bars = 3 + random.below(6);
    for b in 0..bars {
        let h = random.below(170);
        list.push(format!("[\"c\",\"fillRect\",{},{},{},{}]", 40 + b * 40, 200 - h, 24, h));
    }
    list.push("[\"s\",\"strokeStyle\",\"#e24a4a\"]".to_string());
    list.push("[\"s\",\"lineWidth\",2]".to_string());
    list.push("[\"s\",\"lineJoin\",\"round\"]".to_string());
    list.push("[\"c\",\"beginPath\"]".to_string());
    let mut points = Vec::new();
    for b in 0..bars {
        let (x, y) = (52 + b * 40, 30 + random.below(160));
        points.push((x, y));
        list.push(format!("[\"c\",\"{}\",{x},{y}]", if b == 0 { "moveTo" } else { "lineTo" }));
    }
    list.push("[\"c\",\"stroke\"]".to_string());
    list.push("[\"s\",\"fillStyle\",\"#fff\"]".to_string());
    for (x, y) in points {
        list.push("[\"c\",\"beginPath\"]".to_string());
        list.push(format!("[\"c\",\"arc\",{x},{y},3,0,6.283185307179586]"));
        list.push("[\"c\",\"fill\"]".to_string());
        list.push("[\"c\",\"stroke\"]".to_string());
    }
    list.push("[\"s\",\"textAlign\",\"center\"]".to_string());
    list.push("[\"c\",\"save\"]".to_string());
    list.push("[\"c\",\"translate\",150,110]".to_string());
    list.push(format!("[\"c\",\"rotate\",{}]", number(random.below(628) as f64 / 100.0)));
    list.push("[\"c\",\"fillText\",\"Weekly budget\",0,0,120]".to_string());
    list.push("[\"c\",\"restore\"]".to_string());
    format!("[{}]", list.join(",")).into_bytes()
}

/// The `canvas` ops' command lists in a twidget recording (tests/recordings/*.twrec): each `cmds` member as the
/// recording's value form holds it, as JSON text.
pub fn recorded_streams(bytes: &[u8]) -> Vec<Vec<u8>> {
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
        fn varint(&mut self) -> Option<usize> {
            let mut v = 0usize;
            for shift in (0..64).step_by(7) {
                let b = self.byte()?;
                v |= usize::from(b & 0x7f) << shift;
                if b & 0x80 == 0 {
                    return Some(v);
                }
            }
            None
        }
        fn bytes(&mut self) -> Option<&[u8]> {
            let n = self.varint()?;
            let out = self.s.get(self.at..self.at.checked_add(n)?)?;
            self.at += n;
            Some(out)
        }
        fn json(&mut self, depth: usize) -> Option<Json> {
            if depth > 64 {
                return None;
            }
            Some(match self.byte()? {
                0 => Json::Null,
                1 => Json::Bool(false),
                2 => Json::Bool(true),
                3 => {
                    let b = self.s.get(self.at..self.at + 8)?;
                    self.at += 8;
                    Json::Number(f64::from_le_bytes(b.try_into().ok()?))
                }
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
    let marker = b"\x04cmds\x05";
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = bytes[from..].windows(marker.len()).position(|w| w == marker) {
        let start = from + at + marker.len() - 1;
        let mut r = Reader { s: bytes, at: start };
        if let Some(cmds) = r.json(0) {
            out.push(cmds.dump());
        }
        from = start;
    }
    out
}
