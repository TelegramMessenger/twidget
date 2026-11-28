//! The 2D canvas interpreter (`core/src/canvas.h`, `canvas.cpp`): the `canvas` op's command list
//! (widget-sandbox's docs/protocol.md, "Canvas commands") becomes the CanvasOps a backend draws. All state
//! lives here (the transform, the state stack, styles, paths, Path2D and gradient objects, clips), so a
//! backend only executes resolved geometry; what the interpreter needs of the renderer it asks a
//! [`CanvasHost`]. Also the renderer's answers that are the canvas's: `measureText` and `getImageData`. Text
//! is bytes, as the C++ `std::string`s hold it; floats and doubles are where the C++ has them.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashMap};
use std::f64::consts::PI;
use std::sync::Arc;

use twidget_json::Json;
use twidget_wire::{CanvasClip, CanvasOp, CanvasPaint, Color, Composite, FontMetrics, Geometry, OpKind, PaintKind, PathOp, Stop};

const MAX_OBJECTS: usize = 4096;
const MAX_STOPS: usize = 256;
const MAX_STACK: usize = 512;
const MAX_TEXT: usize = 4096;
const MAX_COLORS: usize = 256;
const MAX_FONT: usize = 512;
// In floats: a path, a canvas's Path2D objects, a state's clips (CLIP_COST each besides their points), one
// op's copies.
const MAX_PATH_FLOATS: usize = 1 << 20;
const MAX_PATHS_FLOATS: usize = 1 << 22;
const MAX_CLIP_FLOATS: usize = 1 << 14;
const MAX_STACK_CLIP_FLOATS: usize = 1 << 18;
const CLIP_COST: usize = 16;
const MAX_RUN_FLOATS: usize = 1 << 24;
// The longest SVG path text a Path2D is made from.
const MAX_SVG_PATH: usize = 1 << 20;

/// Canvas method names, looked up once a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Method {
    Unknown,
    BeginPath,
    ClearRect,
    Clip,
    DrawImage,
    Fill,
    FillRect,
    FillText,
    PutImageData,
    ResetTransform,
    Restore,
    Rotate,
    Save,
    Scale,
    SetLineDash,
    SetTransform,
    Stroke,
    StrokeRect,
    StrokeText,
    Transform,
    Translate,
    AddPath,
    Arc,
    ArcTo,
    BezierCurveTo,
    ClosePath,
    Ellipse,
    LineTo,
    MoveTo,
    QuadraticCurveTo,
    Rect,
    RoundRect,
}

fn method_of(name: &[u8]) -> Method {
    match name {
        b"beginPath" => Method::BeginPath,
        b"clearRect" => Method::ClearRect,
        b"clip" => Method::Clip,
        b"drawImage" => Method::DrawImage,
        b"fill" => Method::Fill,
        b"fillRect" => Method::FillRect,
        b"fillText" => Method::FillText,
        b"putImageData" => Method::PutImageData,
        b"resetTransform" => Method::ResetTransform,
        b"restore" => Method::Restore,
        b"rotate" => Method::Rotate,
        b"save" => Method::Save,
        b"scale" => Method::Scale,
        b"setLineDash" => Method::SetLineDash,
        b"setTransform" => Method::SetTransform,
        b"stroke" => Method::Stroke,
        b"strokeRect" => Method::StrokeRect,
        b"strokeText" => Method::StrokeText,
        b"transform" => Method::Transform,
        b"translate" => Method::Translate,
        b"addPath" => Method::AddPath,
        b"arc" => Method::Arc,
        b"arcTo" => Method::ArcTo,
        b"bezierCurveTo" => Method::BezierCurveTo,
        b"closePath" => Method::ClosePath,
        b"ellipse" => Method::Ellipse,
        b"lineTo" => Method::LineTo,
        b"moveTo" => Method::MoveTo,
        b"quadraticCurveTo" => Method::QuadraticCurveTo,
        b"rect" => Method::Rect,
        b"roundRect" => Method::RoundRect,
        _ => Method::Unknown,
    }
}

/// Canvas state property names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Property {
    Unknown,
    FillStyle,
    Font,
    GlobalAlpha,
    GlobalCompositeOperation,
    ImageSmoothingEnabled,
    LineCap,
    LineDashOffset,
    LineJoin,
    LineWidth,
    MiterLimit,
    ShadowBlur,
    ShadowColor,
    ShadowOffsetX,
    ShadowOffsetY,
    StrokeStyle,
    TextAlign,
    TextBaseline,
}

fn property_of(name: &[u8]) -> Property {
    match name {
        b"fillStyle" => Property::FillStyle,
        b"font" => Property::Font,
        b"globalAlpha" => Property::GlobalAlpha,
        b"globalCompositeOperation" => Property::GlobalCompositeOperation,
        b"imageSmoothingEnabled" => Property::ImageSmoothingEnabled,
        b"lineCap" => Property::LineCap,
        b"lineDashOffset" => Property::LineDashOffset,
        b"lineJoin" => Property::LineJoin,
        b"lineWidth" => Property::LineWidth,
        b"miterLimit" => Property::MiterLimit,
        b"shadowBlur" => Property::ShadowBlur,
        b"shadowColor" => Property::ShadowColor,
        b"shadowOffsetX" => Property::ShadowOffsetX,
        b"shadowOffsetY" => Property::ShadowOffsetY,
        b"strokeStyle" => Property::StrokeStyle,
        b"textAlign" => Property::TextAlign,
        b"textBaseline" => Property::TextBaseline,
        _ => Property::Unknown,
    }
}

/// globalCompositeOperation's names, in `Composite`'s order.
const COMPOSITES: [&[u8]; 26] = [
    b"source-over",
    b"source-in",
    b"source-out",
    b"source-atop",
    b"destination-over",
    b"destination-in",
    b"destination-out",
    b"destination-atop",
    b"lighter",
    b"copy",
    b"xor",
    b"multiply",
    b"screen",
    b"overlay",
    b"darken",
    b"lighten",
    b"color-dodge",
    b"color-burn",
    b"hard-light",
    b"soft-light",
    b"difference",
    b"exclusion",
    b"hue",
    b"saturation",
    b"color",
    b"luminosity",
];

fn composite_of(name: &[u8]) -> Option<Composite> {
    COMPOSITES.iter().position(|n| *n == name).and_then(|i| Composite::from_u8(i as u8))
}

/// The pixels of an ImageData: the base64 alphabet's characters read, any other skipped.
fn base64(text: &[u8]) -> Vec<u8> {
    fn digit(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut value: u32 = 0;
    let mut bits: i32 = -8;
    for &c in text {
        let Some(d) = digit(c) else { continue };
        value = (value << 6) | d;
        bits += 6;
        if bits >= 0 {
            out.push((value >> bits) as u8);
            bits -= 8;
        }
    }
    out
}

/// `std::clamp(v, lo, hi)`.
fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}

/// `std::max(0.0, v)`: 0 unless 0 is less than `v`.
fn at_least_zero(v: f64) -> f64 {
    if 0.0 < v { v } else { 0.0 }
}

/// Appends an elliptical arc as cubic béziers (at most a quarter turn each). Angles in radians, y down,
/// clockwise positive.
fn arc_segments(
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    rotation: f64,
    start: f64,
    sweep: f64,
    mut emit: impl FnMut(f64, f64, f64, f64, f64, f64),
) {
    if !sweep.is_finite() {
        return;
    }
    let segments = 1.max((sweep.abs() / (PI / 2.0) - 1e-9).ceil() as i32);
    let step = sweep / f64::from(segments);
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let (cr, sr) = (rotation.cos(), rotation.sin());
    let point = |x: f64, y: f64| (cx + rx * x * cr - ry * y * sr, cy + rx * x * sr + ry * y * cr);
    let mut a = start;
    for _ in 0..segments {
        let b = a + step;
        let (ca, sa, cb, sb) = (a.cos(), a.sin(), b.cos(), b.sin());
        let (p1x, p1y) = point(ca - k * sa, sa + k * ca);
        let (p2x, p2y) = point(cb + k * sb, sb - k * cb);
        let (p3x, p3y) = point(cb, sb);
        emit(p1x, p1y, p2x, p2y, p3x, p3y);
        a = b;
    }
}

fn sweep_of(start: f64, end: f64, anticlockwise: bool) -> f64 {
    let mut sweep = end - start;
    if !anticlockwise {
        if sweep >= 2.0 * PI {
            return 2.0 * PI;
        }
        sweep %= 2.0 * PI;
        if sweep < 0.0 {
            sweep += 2.0 * PI;
        }
    } else {
        if -sweep >= 2.0 * PI {
            return -2.0 * PI;
        }
        sweep %= 2.0 * PI;
        if sweep > 0.0 {
            sweep -= 2.0 * PI;
        }
    }
    sweep
}

/// Polylines of a geometry, curves flattened, for hit testing.
fn flatten(g: &Geometry) -> Vec<Vec<(f64, f64)>> {
    let mut out: Vec<Vec<(f64, f64)>> = Vec::new();
    let p = &g.points;
    let n = p.len();
    let mut i = 0;
    let (mut cx, mut cy, mut sx, mut sy) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let line = |out: &mut Vec<Vec<(f64, f64)>>, cx: &mut f64, cy: &mut f64, x: f64, y: f64| {
        if out.is_empty() {
            out.push(vec![(*cx, *cy)]);
        }
        if let Some(last) = out.last_mut() {
            last.push((x, y));
        }
        *cx = x;
        *cy = y;
    };
    for &op in &g.ops {
        let need = match op {
            PathOp::Cubic => 6,
            PathOp::Quad => 4,
            PathOp::Close => 0,
            _ => 2,
        };
        if i + need > n {
            break;
        }
        let at = |k: usize| f64::from(p[i + k]);
        match op {
            PathOp::Move => {
                out.push(vec![(at(0), at(1))]);
                cx = at(0);
                sx = cx;
                cy = at(1);
                sy = cy;
            }
            PathOp::Line => line(&mut out, &mut cx, &mut cy, at(0), at(1)),
            PathOp::Quad => {
                let (x0, y0) = (cx, cy);
                for s in 1..=16 {
                    let t = f64::from(s) / 16.0;
                    let u = 1.0 - t;
                    let x = u * u * x0 + 2.0 * u * t * at(0) + t * t * at(2);
                    let y = u * u * y0 + 2.0 * u * t * at(1) + t * t * at(3);
                    line(&mut out, &mut cx, &mut cy, x, y);
                }
            }
            PathOp::Cubic => {
                let (x0, y0) = (cx, cy);
                for s in 1..=16 {
                    let t = f64::from(s) / 16.0;
                    let u = 1.0 - t;
                    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    let x = a * x0 + b * at(0) + c * at(2) + d * at(4);
                    let y = a * y0 + b * at(1) + c * at(3) + d * at(5);
                    line(&mut out, &mut cx, &mut cy, x, y);
                }
            }
            PathOp::Close => line(&mut out, &mut cx, &mut cy, sx, sy),
        }
        i += need;
    }
    out
}

fn contains(g: &Geometry, x: f64, y: f64, evenodd: bool) -> bool {
    let (mut winding, mut crossings) = (0i32, 0i32);
    for poly in flatten(g) {
        if poly.len() < 2 {
            continue;
        }
        // Subpaths fill as if closed.
        for k in 0..poly.len() {
            let (x0, y0) = poly[k];
            let (x1, y1) = poly[(k + 1) % poly.len()];
            if (y0 <= y) != (y1 <= y) {
                let at = x0 + (y - y0) * (x1 - x0) / (y1 - y0);
                if at > x {
                    crossings += 1;
                    winding += if y1 > y0 { 1 } else { -1 };
                }
            }
        }
    }
    if evenodd { (crossings & 1) != 0 } else { winding != 0 }
}

fn near_stroke(g: &Geometry, x: f64, y: f64, half: f64) -> bool {
    for poly in flatten(g) {
        for w in poly.windows(2) {
            let ((x0, y0), (x1, y1)) = (w[0], w[1]);
            let (dx, dy) = (x1 - x0, y1 - y0);
            let len = dx * dx + dy * dy;
            let t = if len > 0.0 { clamp(((x - x0) * dx + (y - y0) * dy) / len, 0.0, 1.0) } else { 0.0 };
            if (x - (x0 + t * dx)).hypot(y - (y0 + t * dy)) <= half {
                return true;
            }
        }
    }
    false
}

/// Whether a point is inside a geometry filled with the nonzero or even-odd rule, curves flattened.
pub fn geometry_contains(g: &Geometry, x: f64, y: f64, evenodd: bool) -> bool {
    contains(g, x, y, evenodd)
}

/// A command argument as the interpreter reads it.
#[derive(Clone, Debug)]
enum Value<'a> {
    Null,
    /// A number, or a boolean as 1 or 0.
    Number(f64),
    String(&'a [u8]),
    /// A flat array, each item a float (0 for one that is no number).
    List(Vec<f32>),
    /// `{g: id}`.
    Gradient(i64),
    /// `{p: id}`.
    PathRef(i64),
    /// `{n: id}`: an image source.
    Node(i64),
    /// `{w, h, b64}`: an ImageData.
    Pixels {
        pixels: Arc<Vec<u8>>,
        w: i32,
        h: i32,
    },
}

impl<'a> Value<'a> {
    fn of(j: &'a Json) -> Value<'a> {
        match j {
            Json::Null => Value::Null,
            Json::Number(d) => Value::Number(*d),
            Json::Bool(b) => Value::Number(if *b { 1.0 } else { 0.0 }),
            Json::String(s) => Value::String(s),
            Json::Array(items) => Value::List(items.iter().map(|x| x.as_number(0.0) as f32).collect()),
            Json::Object(_) => {
                if let Some(g) = j.find("g") {
                    Value::Gradient(i64::from(g.as_int(0)))
                } else if let Some(p) = j.find("p") {
                    Value::PathRef(i64::from(p.as_int(0)))
                } else if let Some(n) = j.find("n") {
                    Value::Node(i64::from(n.as_int(0)))
                } else if let Some(b) = j.find("b64")
                    && b.is_string()
                {
                    Value::Pixels { pixels: Arc::new(base64(b.as_str())), w: j.get("w").as_int(0), h: j.get("h").as_int(0) }
                } else {
                    Value::Null
                }
            }
        }
    }

    /// The string's bytes; none for any other kind.
    fn string(&self) -> &[u8] {
        match self {
            Value::String(s) => s,
            _ => b"",
        }
    }
}

/// The arguments of a command from its item `from` on.
fn rest<'a>(args: &mut Vec<Value<'a>>, list: &'a [Json], from: usize) {
    args.extend(list[from.min(list.len())..].iter().map(Value::of));
}

/// Argument `i` when it is a finite number, else `fallback`.
fn number_at(a: &[Value], i: usize, fallback: f64) -> f64 {
    match a.get(i) {
        Some(Value::Number(v)) if v.is_finite() => *v,
        _ => fallback,
    }
}

/// The canvas transform, in doubles: x' = a·x + c·y + e, y' = b·x + d·y + f.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Matrix {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Matrix {
    const IDENTITY: Matrix = Matrix { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    fn apply(&self, x: f64, y: f64) -> (f32, f32) {
        ((self.a * x + self.c * y + self.e) as f32, (self.b * x + self.d * y + self.f) as f32)
    }

    /// `self`, then `m`.
    fn then(&self, m: &Matrix) -> Matrix {
        Matrix {
            a: self.a * m.a + self.b * m.c,
            b: self.a * m.b + self.b * m.d,
            c: self.c * m.a + self.d * m.c,
            d: self.c * m.b + self.d * m.d,
            e: self.e * m.a + self.f * m.c + m.e,
            f: self.e * m.b + self.f * m.d + m.f,
        }
    }

    fn invert(&self) -> Option<Matrix> {
        let det = self.a * self.d - self.b * self.c;
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        Some(Matrix {
            a: self.d / det,
            b: -self.b / det,
            c: -self.c / det,
            d: self.a / det,
            e: (self.c * self.f - self.d * self.e) / det,
            f: (self.b * self.e - self.a * self.f) / det,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Paint {
    gradient: bool,
    color: Color,
    id: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Align {
    Start,
    End,
    Left,
    Right,
    Center,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Baseline {
    Alphabetic,
    Top,
    Hanging,
    Middle,
    Ideographic,
    Bottom,
}

#[derive(Clone, Debug)]
struct State {
    matrix: Matrix,
    fill: Paint,
    stroke: Paint,
    font: Vec<u8>,
    align: Align,
    baseline: Baseline,
    dash: Vec<f32>,
    dash_offset: f32,
    line_width: f32,
    line_cap: i32,
    line_join: i32,
    miter_limit: f32,
    alpha: f32,
    composite: Composite,
    shadow_color: Color,
    shadow_blur: f32,
    shadow_x: f32,
    shadow_y: f32,
    smoothing: bool,
    clips: Option<Arc<Vec<CanvasClip>>>,
    clip_floats: usize,
}

impl Default for State {
    fn default() -> State {
        let black = Paint { gradient: false, color: Color::BLACK, id: 0 };
        State {
            matrix: Matrix::IDENTITY,
            fill: black,
            stroke: black,
            font: b"10px sans-serif".to_vec(),
            align: Align::Start,
            baseline: Baseline::Alphabetic,
            dash: Vec::new(),
            dash_offset: 0.0,
            line_width: 1.0,
            line_cap: 0,
            line_join: 0,
            miter_limit: 10.0,
            alpha: 1.0,
            composite: Composite::SourceOver,
            shadow_color: Color::TRANSPARENT,
            shadow_blur: 0.0,
            shadow_x: 0.0,
            shadow_y: 0.0,
            smoothing: true,
            clips: None,
            clip_floats: 0,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Gradient {
    kind: Vec<u8>,
    args: Vec<f32>,
    stops: Vec<Stop>,
}

/// A path being built: points already in canvas px (the current path) or in its own space (Path2D).
#[derive(Clone, Debug, Default)]
struct Builder {
    geometry: Geometry,
    /// A figure is open: segments append to it.
    open: bool,
    /// There is a current point; after closePath it is the closed figure's start.
    has_point: bool,
    /// Subpath start and current point, as stored.
    sx: f32,
    sy: f32,
    cx: f32,
    cy: f32,
}

/// An image source the host resolves: a decoded <img> (`image`) or another canvas (`canvas`), with its size
/// in px; neither for a refused image, which draws as the sandbox's placeholder.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CanvasSource {
    pub image: i32,
    pub canvas: i32,
    pub w: f32,
    pub h: f32,
}

/// What the interpreter asks of the renderer.
pub trait CanvasHost {
    /// A backend font for a CSS `font` shorthand, `metrics` filled, or 0 for none; kept by the host for the
    /// canvas's lifetime.
    fn canvas_font(&mut self, css: &[u8], metrics: &mut FontMetrics) -> i32;
    fn canvas_text_width(&mut self, font: i32, text: &[u8]) -> f32;
    /// An image source `{n: id}`.
    fn canvas_source(&mut self, node: i64) -> Option<CanvasSource>;
    /// Pixels the canvas may still allocate, given every other canvas of the widget.
    fn canvas_allowance(&mut self, id: i32) -> i64;
    /// Floats the widget's other canvases keep in their paths: one bound holds for all of them together.
    fn canvas_path_floats(&mut self, _id: i32) -> usize {
        0
    }
}

/// One canvas element's interpreter.
#[derive(Debug)]
pub struct Canvas {
    id: i32,
    width: i32,
    height: i32,
    state: State,
    stack: Vec<State>,
    path: Builder,
    // By id, which the sandbox only ever increases: at the cap the oldest object goes, as a browser collects
    // the ones a widget dropped, rather than the newest being refused.
    gradients: BTreeMap<i32, Gradient>,
    colors: HashMap<Vec<u8>, Option<Color>>,
    paths: BTreeMap<i32, Builder>,
    paths_floats: usize,
    run_floats: usize,
}

/// Takes `floats` of one run's allowance, or refuses them all.
fn spend(run_floats: &mut usize, floats: usize) -> bool {
    if floats > MAX_RUN_FLOATS - *run_floats {
        return false;
    }
    *run_floats += floats;
    true
}

fn transformed(path: &Geometry, m: &Matrix) -> Geometry {
    let mut points = vec![0.0f32; path.points.len()];
    for (to, from) in points.chunks_exact_mut(2).zip(path.points.chunks_exact(2)) {
        let (x, y) = m.apply(f64::from(from[0]), f64::from(from[1]));
        to[0] = x;
        to[1] = y;
    }
    Geometry { ops: path.ops.clone(), points }
}

fn rect_geometry(paths: &BTreeMap<i32, Builder>, m: &Matrix, x: f64, y: f64, w: f64, h: f64) -> Geometry {
    let mut b = Builder::default();
    path_method(paths, &mut b, m, Method::Rect, &[Value::Number(x), Value::Number(y), Value::Number(w), Value::Number(h)]);
    b.geometry
}

/// A colour of the canvas's cache: a widget sets the same few colours frame after frame.
fn color_of(colors: &mut HashMap<Vec<u8>, Option<Color>>, text: &[u8]) -> Option<Color> {
    if let Some(known) = colors.get(text) {
        return *known;
    }
    if colors.len() >= MAX_COLORS {
        colors.clear();
    }
    let parsed = twidget_css::parse_color(text);
    colors.insert(text.to_vec(), parsed);
    parsed
}

impl Canvas {
    pub const MAX_SIDE: i32 = 8192;
    pub const MAX_PIXELS: i64 = 4096 * 4096;

    pub fn new(id: i32, width: i32, height: i32, host: &mut dyn CanvasHost) -> Canvas {
        let mut canvas = Canvas {
            id,
            width: 0,
            height: 0,
            state: State::default(),
            stack: Vec::new(),
            path: Builder::default(),
            gradients: BTreeMap::new(),
            colors: HashMap::new(),
            paths: BTreeMap::new(),
            paths_floats: 0,
            run_floats: 0,
        };
        let mut ignored = Vec::new();
        canvas.resize(host, width, height, &mut ignored);
        canvas
    }

    pub fn id(&self) -> i32 {
        self.id
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn pixels(&self) -> i64 {
        i64::from(self.width) * i64::from(self.height)
    }

    /// Floats the canvas keeps in its Path2D objects and its current path.
    pub fn path_floats(&self) -> usize {
        self.paths_floats + self.path.geometry.points.len()
    }

    /// Forgets the colours it has parsed.
    pub fn trim(&mut self) {
        self.colors.clear();
    }

    /// The bitmap size changed (`r`, or the element's width/height): cleared, state reset. A size past the
    /// widget's canvas pixels is not applied but still clears the bitmap: the canvas keeps the size it had, a
    /// new one stays 0 × 0 (widget-sandbox v0.3.0).
    pub fn resize(&mut self, host: &mut dyn CanvasHost, width: i32, height: i32, out: &mut Vec<CanvasOp>) {
        let width = width.clamp(0, Canvas::MAX_SIDE);
        let height = height.clamp(0, Canvas::MAX_SIDE);
        if i64::from(width) * i64::from(height) <= Canvas::MAX_PIXELS.min(host.canvas_allowance(self.id)) {
            self.width = width;
            self.height = height;
        }
        self.state = State::default();
        self.stack.clear();
        self.path = Builder::default();
        out.push(CanvasOp { kind: OpKind::Reset, ..CanvasOp::default() });
    }

    fn path_room(&self, host: &mut dyn CanvasHost) -> usize {
        let held = host.canvas_path_floats(self.id).wrapping_add(self.path_floats());
        MAX_PATHS_FLOATS.saturating_sub(held)
    }

    /// One `canvas` op. `out` receives what the backend must run, in order.
    pub fn run(&mut self, host: &mut dyn CanvasHost, cmds: &Json, out: &mut Vec<CanvasOp>) {
        let mut args: Vec<Value> = Vec::new();
        self.run_floats = 0;
        for cmd in cmds.arr() {
            let list = cmd.arr();
            if list.len() < 2 || !list[0].is_string() {
                continue;
            }
            args.clear();
            match list[0].as_str() {
                b"c" => {
                    rest(&mut args, list, 2);
                    if list[1].is_string() {
                        self.call_method(host, list[1].as_str(), &args, out);
                    }
                }
                b"s" => {
                    rest(&mut args, list, 2);
                    if list[1].is_string()
                        && let Some(value) = args.first()
                    {
                        self.set(list[1].as_str(), value);
                    }
                }
                b"r" => {
                    let height = if list.len() > 2 { list[2].as_int(0) } else { 0 };
                    self.resize(host, list[1].as_int(0), height, out);
                }
                b"g" => {
                    let gid = list[1].as_int(0);
                    if list.len() < 3 || !list[2].is_string() {
                        continue;
                    }
                    if !self.gradients.contains_key(&gid) && self.gradients.len() >= MAX_OBJECTS {
                        self.gradients.pop_first();
                    }
                    let g = Gradient {
                        kind: list[2].as_str().to_vec(),
                        args: list[3..].iter().map(|x| x.as_number(0.0) as f32).collect(),
                        stops: Vec::new(),
                    };
                    self.gradients.insert(gid, g);
                }
                b"gs" => {
                    let Some(g) = self.gradients.get_mut(&list[1].as_int(0)) else { continue };
                    if list.len() < 4 || g.stops.len() >= MAX_STOPS {
                        continue;
                    }
                    let Some(color) = color_of(&mut self.colors, list[3].as_str()) else { continue };
                    let offset = list[2].as_number(0.0) as f32;
                    let stop = Stop::new(offset.clamp(0.0, 1.0), color);
                    // upper_bound's own comparison.
                    #[allow(clippy::neg_cmp_op_on_partial_ord)]
                    let at = g.stops.partition_point(|s| !(stop.offset < s.offset));
                    g.stops.insert(at, stop);
                }
                b"pn" => {
                    let pid = list[1].as_int(0);
                    // At the cap a new path is refused: a Path2D is sent once, so evicting one a widget still holds
                    // would make it vanish.
                    if !self.paths.contains_key(&pid) && self.paths.len() >= MAX_OBJECTS {
                        continue;
                    }
                    let mut b = Builder::default();
                    if list.len() > 2 && list[2].is_string() && list[2].as_str().len() <= MAX_SVG_PATH {
                        append_svg_path(list[2].as_str(), &mut b.geometry);
                        // An SVG path ends where its last command did; later Path2D calls continue from there.
                        let pts = &b.geometry.points;
                        if pts.len() >= 2 {
                            b.has_point = true;
                            b.open = b.geometry.ops.last() != Some(&PathOp::Close);
                            b.cx = pts[pts.len() - 2];
                            b.cy = pts[pts.len() - 1];
                        }
                    }
                    let before = self.paths.get(&pid).map_or(0, |held| held.geometry.points.len());
                    let n = b.geometry.points.len();
                    if n > MAX_PATH_FLOATS || n > before + self.path_room(host) {
                        continue;
                    }
                    self.paths_floats = self.paths_floats.wrapping_add(n.wrapping_sub(before));
                    self.paths.insert(pid, b);
                }
                b"pc" => {
                    let pid = list[1].as_int(0);
                    if !self.paths.contains_key(&pid) || list.len() < 3 || !list[2].is_string() || self.path_room(host) == 0 {
                        continue;
                    }
                    rest(&mut args, list, 3);
                    // Out of the map while it changes: addPath finds no path to add to itself.
                    let Some(mut b) = self.paths.remove(&pid) else { continue };
                    let before = b.geometry.points.len();
                    path_method(&self.paths, &mut b, &Matrix::IDENTITY, method_of(list[2].as_str()), &args);
                    self.paths_floats = self.paths_floats.wrapping_add(b.geometry.points.len().wrapping_sub(before));
                    self.paths.insert(pid, b);
                }
                _ => {}
            }
        }
    }

    fn set(&mut self, property: &[u8], v: &Value) {
        let s = &mut self.state;
        let number = match v {
            Value::Number(n) => Some(*n),
            _ => None,
        };
        let text = v.string();
        match property_of(property) {
            q @ (Property::FillStyle | Property::StrokeStyle) => {
                let paint = match v {
                    Value::String(t) => color_of(&mut self.colors, t).map(|color| Paint { gradient: false, color, id: 0 }),
                    Value::Gradient(id) => Some(Paint { gradient: true, color: Color::BLACK, id: *id as i32 }),
                    _ => None,
                };
                if let Some(paint) = paint {
                    if q == Property::FillStyle {
                        s.fill = paint;
                    } else {
                        s.stroke = paint;
                    }
                }
            }
            Property::LineWidth => {
                if let Some(n) = number
                    && n > 0.0
                    && n.is_finite()
                {
                    s.line_width = n as f32;
                }
            }
            Property::LineCap => {
                s.line_cap = match text {
                    b"round" => 1,
                    b"square" => 2,
                    b"butt" => 0,
                    _ => s.line_cap,
                };
            }
            Property::LineJoin => {
                s.line_join = match text {
                    b"round" => 1,
                    b"bevel" => 2,
                    b"miter" => 0,
                    _ => s.line_join,
                };
            }
            Property::MiterLimit => {
                if let Some(n) = number
                    && n > 0.0
                    && n.is_finite()
                {
                    s.miter_limit = n as f32;
                }
            }
            Property::LineDashOffset => {
                if let Some(n) = number
                    && n.is_finite()
                {
                    s.dash_offset = n as f32;
                }
            }
            Property::Font => {
                if let Value::String(t) = v
                    && !t.is_empty()
                    && t.len() <= MAX_FONT
                {
                    s.font = t.to_vec();
                }
            }
            Property::TextAlign => {
                s.align = match text {
                    b"start" => Align::Start,
                    b"end" => Align::End,
                    b"left" => Align::Left,
                    b"right" => Align::Right,
                    b"center" => Align::Center,
                    _ => s.align,
                };
            }
            Property::TextBaseline => {
                s.baseline = match text {
                    b"alphabetic" => Baseline::Alphabetic,
                    b"top" => Baseline::Top,
                    b"hanging" => Baseline::Hanging,
                    b"middle" => Baseline::Middle,
                    b"ideographic" => Baseline::Ideographic,
                    b"bottom" => Baseline::Bottom,
                    _ => s.baseline,
                };
            }
            Property::GlobalAlpha => {
                if let Some(n) = number
                    && (0.0..=1.0).contains(&n)
                {
                    s.alpha = n as f32;
                }
            }
            Property::GlobalCompositeOperation => {
                if let Some(c) = composite_of(text) {
                    s.composite = c;
                }
            }
            Property::ShadowBlur => {
                if let Some(n) = number
                    && n >= 0.0
                    && n.is_finite()
                {
                    s.shadow_blur = (if 100.0 < n { 100.0 } else { n }) as f32;
                }
            }
            Property::ShadowColor => {
                if let Value::String(t) = v
                    && let Some(color) = color_of(&mut self.colors, t)
                {
                    s.shadow_color = color;
                }
            }
            Property::ShadowOffsetX => {
                if let Some(n) = number
                    && n.is_finite()
                {
                    s.shadow_x = n as f32;
                }
            }
            Property::ShadowOffsetY => {
                if let Some(n) = number
                    && n.is_finite()
                {
                    s.shadow_y = n as f32;
                }
            }
            Property::ImageSmoothingEnabled => {
                if let Some(n) = number {
                    s.smoothing = n != 0.0;
                }
            }
            Property::Unknown => {}
        }
    }

    /// An op of `kind` with the state's transform and compositing.
    fn op(&self, kind: OpKind) -> CanvasOp {
        let s = &self.state;
        let m = &s.matrix;
        CanvasOp {
            kind,
            matrix: [m.a as f32, m.b as f32, m.c as f32, m.d as f32, m.e as f32, m.f as f32],
            alpha: s.alpha,
            composite: s.composite,
            shadow_color: s.shadow_color,
            shadow_blur: s.shadow_blur,
            shadow_x: s.shadow_x,
            shadow_y: s.shadow_y,
            clips: s.clips.clone(),
            smoothing: s.smoothing,
            ..CanvasOp::default()
        }
    }

    /// A paint's CanvasPaint; None for a gradient the canvas does not hold or does not draw (a pattern).
    fn paint_of(&self, paint: &Paint) -> Option<CanvasPaint> {
        if !paint.gradient {
            return Some(CanvasPaint { kind: PaintKind::Solid, color: paint.color, ..CanvasPaint::default() });
        }
        let g = self.gradients.get(&paint.id)?;
        let mut out = CanvasPaint { stops: g.stops.clone(), ..CanvasPaint::default() };
        let mut args = |n: usize| {
            if g.args.len() < n {
                return false;
            }
            for i in 0..n {
                out.args[i] = if g.args[i].is_finite() { g.args[i] } else { 0.0 };
            }
            true
        };
        let kind = if g.kind == b"linear" && args(4) {
            PaintKind::Linear
        } else if g.kind == b"radial" && args(6) {
            PaintKind::Radial
        } else if g.kind == b"conic" && args(3) {
            PaintKind::Conic
        } else {
            return None;
        };
        out.kind = kind;
        // A gradient without stops paints nothing, as in browsers; so does one with no length (the canvas spec),
        // which is what widget-sandbox's validator makes of a conic gradient or a pattern.
        let a = &out.args;
        let degenerate = (kind == PaintKind::Linear && a[0] == a[2] && a[1] == a[3])
            || (kind == PaintKind::Radial && a[0] == a[3] && a[1] == a[4] && a[2] == a[5]);
        if out.stops.is_empty() || degenerate {
            out = CanvasPaint { kind: PaintKind::Solid, color: Color::TRANSPARENT, ..CanvasPaint::default() };
        }
        Some(out)
    }

    /// A Fill op for a path of `floats`, before its path is set; None when nothing is drawn.
    fn fill_op(&mut self, empty: bool, floats: usize, evenodd: bool) -> Option<CanvasOp> {
        if empty || !spend(&mut self.run_floats, floats) {
            return None;
        }
        let mut o = self.op(OpKind::Fill);
        o.paint = self.paint_of(&self.state.fill)?;
        o.evenodd = evenodd;
        Some(o)
    }

    fn stroke_op(&mut self, empty: bool, floats: usize) -> Option<CanvasOp> {
        if empty || !spend(&mut self.run_floats, floats) {
            return None;
        }
        let mut o = self.op(OpKind::Stroke);
        o.paint = self.paint_of(&self.state.stroke)?;
        let s = &self.state;
        o.line_width = s.line_width;
        o.line_cap = s.line_cap;
        o.line_join = s.line_join;
        o.miter_limit = s.miter_limit;
        o.dashes = s.dash.clone();
        o.dash_offset = s.dash_offset;
        Some(o)
    }

    fn fill(&mut self, path: Geometry, evenodd: bool, out: &mut Vec<CanvasOp>) {
        if let Some(mut o) = self.fill_op(path.empty(), path.points.len(), evenodd) {
            o.path = path;
            out.push(o);
        }
    }

    fn stroke(&mut self, path: Geometry, out: &mut Vec<CanvasOp>) {
        if let Some(mut o) = self.stroke_op(path.empty(), path.points.len()) {
            o.path = path;
            out.push(o);
        }
    }

    fn text(
        &mut self,
        host: &mut dyn CanvasHost,
        full: &[u8],
        x: f64,
        y: f64,
        max_width: Option<&Value>,
        stroke_text: bool,
        out: &mut Vec<CanvasOp>,
    ) {
        // Truncated at a UTF-8 boundary.
        let mut cut = full.len().min(MAX_TEXT);
        while cut < full.len() && cut > 0 && (full[cut] & 0xC0) == 0x80 {
            cut -= 1;
        }
        let s = &full[..cut];
        let mut m = FontMetrics::default();
        let font = host.canvas_font(&self.state.font, &mut m);
        if font == 0 || s.is_empty() {
            return;
        }
        let width = host.canvas_text_width(font, s);
        let mut o = self.op(OpKind::Text);
        let Some(paint) = self.paint_of(if stroke_text { &self.state.stroke } else { &self.state.fill }) else { return };
        o.paint = paint;
        let st = &self.state;
        o.text_dx = match st.align {
            Align::Center => -width / 2.0,
            Align::Right | Align::End => -width,
            _ => 0.0,
        };
        // As Chromium places them: top, middle and bottom on the em box, hanging and ideographic on the font's
        // own ascent and descent.
        let shift = match st.baseline {
            Baseline::Top => m.em_ascent,
            Baseline::Hanging => m.ascent * 0.8,
            Baseline::Middle => (m.em_ascent - m.em_descent) / 2.0,
            Baseline::Bottom => -m.em_descent,
            Baseline::Ideographic => -m.descent,
            Baseline::Alphabetic => 0.0,
        };
        o.x = x as f32;
        o.y = y as f32 + shift;
        if let Some(Value::Number(limit)) = max_width {
            if *limit > 0.0 && f64::from(width) > *limit {
                o.scale_x = (limit / f64::from(width)) as f32;
            } else if *limit <= 0.0 {
                return;
            }
        }
        o.font = font;
        o.text = s.to_vec();
        o.stroke_text = stroke_text;
        o.line_width = st.line_width;
        if stroke_text {
            o.line_cap = st.line_cap;
            o.line_join = st.line_join;
            o.miter_limit = st.miter_limit;
            o.dashes = st.dash.clone();
            o.dash_offset = st.dash_offset;
        }
        out.push(o);
    }

    fn image(&mut self, host: &mut dyn CanvasHost, a: &[Value], out: &mut Vec<CanvasOp>) {
        let Some(Value::Node(node)) = a.first() else { return };
        let Some(source) = host.canvas_source(*node) else { return };
        let (w, h) = (source.w, source.h);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let n = |i: usize| match a.get(i) {
            Some(Value::Number(v)) if v.is_finite() => *v as f32,
            _ => 0.0,
        };
        let mut o = self.op(OpKind::Image);
        o.image = source.image;
        o.canvas = source.canvas;
        match a.len() {
            3 => {
                (o.dx, o.dy, o.dw, o.dh) = (n(1), n(2), w, h);
            }
            5 => {
                (o.dx, o.dy, o.dw, o.dh) = (n(1), n(2), n(3), n(4));
            }
            9 => {
                (o.sx, o.sy, o.sw, o.sh) = (n(1), n(2), n(3), n(4));
                (o.dx, o.dy, o.dw, o.dh) = (n(5), n(6), n(7), n(8));
            }
            _ => return,
        }
        let nine = a.len() == 9;
        if o.dw == 0.0 || o.dh == 0.0 || (nine && (o.sw == 0.0 || o.sh == 0.0)) {
            return;
        }
        if o.image != 0 || o.canvas != 0 {
            out.push(o);
            return;
        }
        // A refused image is widget-sandbox's placeholder (url-policy.ts, PLACEHOLDER_IMAGE), which no backend
        // holds: its drawing, as a document draws it (draw_placeholder), from the source rect into the
        // destination.
        let (sx, sy, sw, sh) = if nine {
            (f64::from(o.sx), f64::from(o.sy), f64::from(o.sw), f64::from(o.sh))
        } else {
            (0.0, 0.0, f64::from(w), f64::from(h))
        };
        let (dx, dy, dw, dh) = (f64::from(o.dx), f64::from(o.dy), f64::from(o.dw), f64::from(o.dh));
        let (fw, fh) = (f64::from(w), f64::from(h));
        let kx = dw / sw * fw / 48.0;
        let ky = dh / sh * fh / 48.0;
        let at = |px: f64, py: f64| (dx + (px * fw / 48.0 - sx) * dw / sw, dy + (py * fh / 48.0 - sy) * dh / sh);
        let mut held = CLIP_COST;
        if let Some(clips) = &self.state.clips {
            for c in clips.iter() {
                held += c.path.points.len() + CLIP_COST;
            }
        }
        if !spend(&mut self.run_floats, held) {
            return;
        }
        let m = self.state.matrix;
        let mut list: Vec<CanvasClip> = self.state.clips.as_deref().cloned().unwrap_or_default();
        list.push(CanvasClip { path: rect_geometry(&self.paths, &m, dx, dy, dw, dh), evenodd: false });
        let clips = Arc::new(list);
        let mut draw = |canvas: &Canvas, g: Geometry, color: Color| {
            let mut f = canvas.op(OpKind::Fill);
            f.paint.kind = PaintKind::Solid;
            f.paint.color = color;
            f.path = g;
            f.clips = Some(Arc::clone(&clips));
            out.push(f);
        };
        let faint = Color::rgba(136, 136, 136, 34);
        let mark = Color::rgba(136, 136, 136, 102);
        let (x0, y0) = at(0.0, 0.0);
        draw(self, rect_geometry(&self.paths, &m, x0, y0, 48.0 * kx, 48.0 * ky), faint);
        let mut hills = Builder::default();
        const POINTS: [f64; 10] = [8.0, 36.0, 18.0, 24.0, 26.0, 33.0, 32.0, 26.0, 40.0, 36.0];
        for i in (0..POINTS.len()).step_by(2) {
            let (x, y) = at(POINTS[i], POINTS[i + 1]);
            let verb = if i != 0 { Method::LineTo } else { Method::MoveTo };
            path_method(&self.paths, &mut hills, &m, verb, &[Value::Number(x), Value::Number(y)]);
        }
        path_method(&self.paths, &mut hills, &m, Method::ClosePath, &[]);
        draw(self, hills.geometry, mark);
        let mut sun = Builder::default();
        let (cx, cy) = at(33.0, 15.0);
        let sun_args = [
            Value::Number(cx),
            Value::Number(cy),
            Value::Number(4.0 * kx.abs()),
            Value::Number(4.0 * ky.abs()),
            Value::Number(0.0),
            Value::Number(0.0),
            Value::Number(2.0 * PI),
        ];
        path_method(&self.paths, &mut sun, &m, Method::Ellipse, &sun_args);
        draw(self, sun.geometry, mark);
    }

    fn call_method(&mut self, host: &mut dyn CanvasHost, method: &[u8], a: &[Value], out: &mut Vec<CanvasOp>) {
        if self.width == 0 || self.height == 0 {
            return;
        }
        let verb = method_of(method);
        let n = |i: usize, fallback: f64| number_at(a, i, fallback);
        let evenodd = |i: usize| a.get(i).is_some_and(|v| matches!(v, Value::String(s) if *s == b"evenodd"));
        let path_arg = |paths: &BTreeMap<i32, Builder>| match a.first() {
            Some(Value::PathRef(r)) => paths.get(&(*r as i32)).map(|b| b.geometry.points.len()),
            _ => None,
        };
        match verb {
            Method::Save => {
                if self.stack.len() < MAX_STACK {
                    self.stack.push(self.state.clone());
                }
            }
            Method::Restore => {
                if let Some(saved) = self.stack.pop() {
                    self.state = saved;
                }
            }
            Method::Scale => {
                self.state.matrix = Matrix { a: n(0, 1.0), b: 0.0, c: 0.0, d: n(1, 1.0), e: 0.0, f: 0.0 }.then(&self.state.matrix);
            }
            Method::Rotate => {
                let r = n(0, 0.0);
                let (c, si) = (r.cos(), r.sin());
                self.state.matrix = Matrix { a: c, b: si, c: -si, d: c, e: 0.0, f: 0.0 }.then(&self.state.matrix);
            }
            Method::Translate => {
                self.state.matrix = Matrix { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: n(0, 0.0), f: n(1, 0.0) }.then(&self.state.matrix);
            }
            Method::Transform => {
                let m = Matrix { a: n(0, 1.0), b: n(1, 0.0), c: n(2, 0.0), d: n(3, 1.0), e: n(4, 0.0), f: n(5, 0.0) };
                self.state.matrix = m.then(&self.state.matrix);
            }
            Method::SetTransform => {
                self.state.matrix = match a.first() {
                    Some(Value::List(l)) if l.len() >= 6 => {
                        let v = |i: usize| f64::from(l[i]);
                        Matrix { a: v(0), b: v(1), c: v(2), d: v(3), e: v(4), f: v(5) }
                    }
                    _ => Matrix { a: n(0, 1.0), b: n(1, 0.0), c: n(2, 0.0), d: n(3, 1.0), e: n(4, 0.0), f: n(5, 0.0) },
                };
            }
            Method::ResetTransform => self.state.matrix = Matrix::IDENTITY,
            Method::ClearRect => {
                let mut o = self.op(OpKind::Clear);
                o.path = rect_geometry(&self.paths, &self.state.matrix, n(0, 0.0), n(1, 0.0), n(2, 0.0), n(3, 0.0));
                out.push(o);
            }
            Method::FillRect => {
                let g = rect_geometry(&self.paths, &self.state.matrix, n(0, 0.0), n(1, 0.0), n(2, 0.0), n(3, 0.0));
                self.fill(g, false, out);
            }
            Method::StrokeRect => {
                let g = rect_geometry(&self.paths, &self.state.matrix, n(0, 0.0), n(1, 0.0), n(2, 0.0), n(3, 0.0));
                self.stroke(g, out);
            }
            Method::BeginPath => self.path = Builder::default(),
            Method::Fill => match path_arg(&self.paths) {
                Some(floats) => {
                    if floats <= MAX_RUN_FLOATS - self.run_floats
                        && let Some(Value::PathRef(r)) = a.first()
                        && let Some(p) = self.paths.get(&(*r as i32))
                    {
                        let g = transformed(&p.geometry, &self.state.matrix);
                        self.fill(g, evenodd(1), out);
                    }
                }
                None => {
                    let g = &self.path.geometry;
                    if let Some(mut o) = self.fill_op(g.empty(), g.points.len(), evenodd(0)) {
                        o.path = self.path.geometry.clone();
                        out.push(o);
                    }
                }
            },
            Method::Stroke => match path_arg(&self.paths) {
                Some(floats) => {
                    if floats <= MAX_RUN_FLOATS - self.run_floats
                        && let Some(Value::PathRef(r)) = a.first()
                        && let Some(p) = self.paths.get(&(*r as i32))
                    {
                        let g = transformed(&p.geometry, &self.state.matrix);
                        self.stroke(g, out);
                    }
                }
                None => {
                    let g = &self.path.geometry;
                    if let Some(mut o) = self.stroke_op(g.empty(), g.points.len()) {
                        o.path = self.path.geometry.clone();
                        out.push(o);
                    }
                }
            },
            Method::Clip => {
                let p = match a.first() {
                    Some(Value::PathRef(r)) => self.paths.get(&(*r as i32)),
                    _ => None,
                };
                let path = p.map_or(&self.path.geometry, |b| &b.geometry);
                let s = &self.state;
                let mut held = path.points.len() + CLIP_COST + s.clip_floats;
                // States saved with no clip() between them share one list.
                let mut stacked = 0;
                let mut last: *const Vec<CanvasClip> = std::ptr::null();
                for saved in &self.stack {
                    let list = saved.clips.as_ref().map_or(std::ptr::null(), Arc::as_ptr);
                    if list != last {
                        stacked += saved.clip_floats;
                    }
                    last = list;
                }
                let mut clips: Vec<CanvasClip> = Vec::new();
                // An empty path clips everything away.
                if held <= MAX_CLIP_FLOATS && stacked + held <= MAX_STACK_CLIP_FLOATS && spend(&mut self.run_floats, held) {
                    if let Some(kept) = &s.clips {
                        clips.reserve(kept.len() + 1);
                        clips.extend_from_slice(kept);
                    }
                    let path = if p.is_some() { transformed(path, &s.matrix) } else { path.clone() };
                    clips.push(CanvasClip { path, evenodd: evenodd(if p.is_some() { 1 } else { 0 }) });
                } else {
                    clips.push(CanvasClip::default());
                    held = CLIP_COST;
                }
                self.state.clips = Some(Arc::new(clips));
                self.state.clip_floats = held;
            }
            Method::FillText | Method::StrokeText => {
                if let Some(Value::String(s)) = a.first() {
                    self.text(host, s, n(1, 0.0), n(2, 0.0), a.get(3), verb == Method::StrokeText, out);
                }
            }
            Method::SetLineDash => {
                if let Some(Value::List(v)) = a.first()
                    && v.len() <= 64
                    && v.iter().all(|x| x.is_finite() && *x >= 0.0)
                {
                    let mut dash = v.clone();
                    if v.len() % 2 == 1 {
                        dash.extend_from_slice(v);
                    }
                    self.state.dash = dash;
                }
            }
            Method::PutImageData => {
                let Some(Value::Pixels { pixels, w, h }) = a.first() else { return };
                let (w, h) = (*w, *h);
                if w <= 0
                    || h <= 0
                    || w > Canvas::MAX_SIDE
                    || h > Canvas::MAX_SIDE
                    || i64::from(w) * i64::from(h) > Canvas::MAX_PIXELS
                    || pixels.len() < w as usize * h as usize * 4
                {
                    return;
                }
                out.push(CanvasOp {
                    kind: OpKind::PutImage,
                    rgba: Some(Arc::clone(pixels)),
                    w,
                    h,
                    dx: n(1, 0.0) as f32,
                    dy: n(2, 0.0) as f32,
                    ..CanvasOp::default()
                });
            }
            Method::DrawImage => self.image(host, a, out),
            _ => {
                if self.path_room(host) != 0 {
                    path_method(&self.paths, &mut self.path, &self.state.matrix, verb, a);
                }
            }
        }
    }

    /// `canvasCall` queries that the state answers: isPointInPath, isPointInStroke, getTransform, getLineDash.
    pub fn call(&self, method: &[u8], args: &Json) -> Json {
        let a: Vec<Value> = args.arr().iter().map(Value::of).collect();
        let m = &self.state.matrix;
        match method {
            b"getTransform" => {
                let mut out = Json::array();
                for v in [m.a, m.b, m.c, m.d, m.e, m.f] {
                    out.push(v);
                }
                out
            }
            b"getLineDash" => {
                let mut out = Json::array();
                for &v in &self.state.dash {
                    out.push(f64::from(v));
                }
                out
            }
            b"isPointInPath" | b"isPointInStroke" => {
                let mut held = None;
                let mut at = 0;
                if let Some(Value::PathRef(r)) = a.first() {
                    let Some(p) = self.paths.get(&(*r as i32)) else { return Json::Bool(false) };
                    held = Some(transformed(&p.geometry, m));
                    at = 1;
                }
                let path = held.as_ref().unwrap_or(&self.path.geometry);
                let n = |i: usize| match a.get(i) {
                    Some(Value::Number(v)) => *v,
                    _ => 0.0,
                };
                let (x, y) = (n(at), n(at + 1));
                if method == b"isPointInPath" {
                    let evenodd = a.get(at + 2).is_some_and(|v| v.string() == b"evenodd");
                    return Json::Bool(contains(path, x, y, evenodd));
                }
                let scale = (m.a * m.d - m.b * m.c).abs().sqrt();
                Json::Bool(near_stroke(path, x, y, f64::from(self.state.line_width) * scale / 2.0))
            }
            _ => {
                let mut out = Json::object();
                out.set("error", "unsupported");
                out
            }
        }
    }
}

/// A path being drawn, its points mapped by `m` as they are added.
struct Pen<'a> {
    b: &'a mut Builder,
    m: &'a Matrix,
}

impl Pen<'_> {
    fn point(&mut self, x: f64, y: f64) -> (f32, f32) {
        let (px, py) = self.m.apply(x, y);
        self.b.geometry.points.push(px);
        self.b.geometry.points.push(py);
        (px, py)
    }

    fn move_to(&mut self, x: f64, y: f64) {
        self.b.geometry.ops.push(PathOp::Move);
        let (px, py) = self.point(x, y);
        let b = &mut *self.b;
        b.sx = px;
        b.cx = px;
        b.sy = py;
        b.cy = py;
        b.open = true;
        b.has_point = true;
    }

    /// Canvas: a segment with no current point starts its subpath at its own first point; after closePath the
    /// next one starts where the closed figure did.
    fn ensure(&mut self, x: f64, y: f64) {
        if self.b.open {
            return;
        }
        if !self.b.has_point {
            self.move_to(x, y);
            return;
        }
        let b = &mut *self.b;
        b.geometry.ops.push(PathOp::Move);
        b.geometry.points.push(b.cx);
        b.geometry.points.push(b.cy);
        b.sx = b.cx;
        b.sy = b.cy;
        b.open = true;
    }

    fn close(&mut self) {
        let b = &mut *self.b;
        if !b.open {
            return;
        }
        b.geometry.ops.push(PathOp::Close);
        b.cx = b.sx;
        b.cy = b.sy;
        b.open = false;
    }

    fn set_current(&mut self, x: f64, y: f64) {
        let (cx, cy) = self.m.apply(x, y);
        self.b.cx = cx;
        self.b.cy = cy;
        self.b.has_point = true;
    }

    fn line(&mut self, x: f64, y: f64) {
        self.ensure(x, y);
        self.b.geometry.ops.push(PathOp::Line);
        let (px, py) = self.point(x, y);
        self.b.cx = px;
        self.b.cy = py;
    }

    fn cubic(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64) {
        self.b.geometry.ops.push(PathOp::Cubic);
        self.point(x1, y1);
        self.point(x2, y2);
        let (px, py) = self.point(x, y);
        self.b.cx = px;
        self.b.cy = py;
    }

    fn arc(&mut self, cx: f64, cy: f64, rx: f64, ry: f64, rotation: f64, start: f64, end: f64, anticlockwise: bool) {
        let sweep = sweep_of(start, end, anticlockwise);
        let (cr, sr) = (rotation.cos(), rotation.sin());
        let x0 = cx + rx * start.cos() * cr - ry * start.sin() * sr;
        let y0 = cy + rx * start.cos() * sr + ry * start.sin() * cr;
        if self.b.has_point {
            self.line(x0, y0);
        } else {
            self.move_to(x0, y0);
        }
        if sweep == 0.0 {
            return;
        }
        arc_segments(cx, cy, rx, ry, rotation, start, sweep, |x1, y1, x2, y2, x, y| self.cubic(x1, y1, x2, y2, x, y));
    }

    /// The current point in this path's own space.
    fn current(&self) -> Option<(f64, f64)> {
        let inverse = self.m.invert()?;
        let (fx, fy) = inverse.apply(f64::from(self.b.cx), f64::from(self.b.cy));
        Some((f64::from(fx), f64::from(fy)))
    }

    fn rounded_corner(&mut self, cx: f64, cy: f64, r: f64, start: f64) {
        arc_segments(cx, cy, r, r, 0.0, start, PI / 2.0, |a1, b1, a2, b2, a3, b3| self.cubic(a1, b1, a2, b2, a3, b3));
    }
}

/// Draws the path-building method `verb` into `b`, its points mapped by `m`; `paths` are the canvas's Path2D
/// objects, for addPath.
fn path_method(paths: &BTreeMap<i32, Builder>, b: &mut Builder, m: &Matrix, verb: Method, a: &[Value]) {
    if b.geometry.points.len() >= MAX_PATH_FLOATS {
        return;
    }
    let n = |i: usize, fallback: f64| number_at(a, i, fallback);
    let truthy = |i: usize| match a.get(i) {
        Some(Value::Number(v)) => *v != 0.0,
        Some(Value::String(s)) => !s.is_empty() && *s != b"false",
        _ => false,
    };
    let mut pen = Pen { b, m };
    match verb {
        Method::MoveTo => pen.move_to(n(0, 0.0), n(1, 0.0)),
        Method::LineTo => pen.line(n(0, 0.0), n(1, 0.0)),
        Method::ClosePath => pen.close(),
        Method::BezierCurveTo => {
            pen.ensure(n(0, 0.0), n(1, 0.0));
            pen.cubic(n(0, 0.0), n(1, 0.0), n(2, 0.0), n(3, 0.0), n(4, 0.0), n(5, 0.0));
        }
        Method::QuadraticCurveTo => {
            pen.ensure(n(0, 0.0), n(1, 0.0));
            pen.b.geometry.ops.push(PathOp::Quad);
            pen.point(n(0, 0.0), n(1, 0.0));
            let (px, py) = pen.point(n(2, 0.0), n(3, 0.0));
            pen.b.cx = px;
            pen.b.cy = py;
        }
        Method::Arc => {
            let r = at_least_zero(n(2, 0.0));
            pen.arc(n(0, 0.0), n(1, 0.0), r, at_least_zero(n(2, 0.0)), 0.0, n(3, 0.0), n(4, 0.0), truthy(5));
        }
        Method::Ellipse => {
            if n(2, 0.0) < 0.0 || n(3, 0.0) < 0.0 {
                return;
            }
            pen.arc(n(0, 0.0), n(1, 0.0), n(2, 0.0), n(3, 0.0), n(4, 0.0), n(5, 0.0), n(6, 0.0), truthy(7));
        }
        Method::ArcTo => {
            let (x1, y1, x2, y2) = (n(0, 0.0), n(1, 0.0), n(2, 0.0), n(3, 0.0));
            let r = at_least_zero(n(4, 0.0));
            if !pen.b.has_point {
                pen.move_to(x1, y1);
                return;
            }
            let Some((x0, y0)) = pen.current() else { return };
            let (ax, ay, bx, by) = (x0 - x1, y0 - y1, x2 - x1, y2 - y1);
            let (la, lb) = (ax.hypot(ay), bx.hypot(by));
            let cross = ax * by - ay * bx;
            if r == 0.0 || la == 0.0 || lb == 0.0 || cross.abs() < 1e-9 * la * lb {
                pen.line(x1, y1);
                return;
            }
            let cosine = (ax * bx + ay * by) / (la * lb);
            let angle = clamp(cosine, -1.0, 1.0).acos();
            let tangent = r / (angle / 2.0).tan();
            let (t1x, t1y) = (x1 + ax / la * tangent, y1 + ay / la * tangent);
            let (t2x, t2y) = (x1 + bx / lb * tangent, y1 + by / lb * tangent);
            let (mx, my) = (ax / la + bx / lb, ay / la + by / lb);
            let ml = mx.hypot(my);
            let distance = r / (angle / 2.0).sin();
            let (cx, cy) = (x1 + mx / ml * distance, y1 + my / ml * distance);
            let (start, end) = ((t1y - cy).atan2(t1x - cx), (t2y - cy).atan2(t2x - cx));
            pen.line(t1x, t1y);
            pen.arc(cx, cy, r, r, 0.0, start, end, cross > 0.0);
        }
        Method::Rect => {
            let (x, y, w, h) = (n(0, 0.0), n(1, 0.0), n(2, 0.0), n(3, 0.0));
            pen.move_to(x, y);
            pen.line(x + w, y);
            pen.line(x + w, y + h);
            pen.line(x, y + h);
            pen.close();
        }
        Method::RoundRect => {
            let (mut x, mut y, mut w, mut h) = (n(0, 0.0), n(1, 0.0), n(2, 0.0), n(3, 0.0));
            if w < 0.0 {
                x += w;
                w = -w;
            }
            if h < 0.0 {
                y += h;
                h = -h;
            }
            let mut r = 0.0;
            match a.get(4) {
                Some(Value::Number(_)) => r = n(4, 0.0),
                Some(Value::List(l)) if !l.is_empty() && l[0].is_finite() => r = f64::from(l[0]),
                _ => {}
            }
            // std::min({r, w / 2, h / 2}): the first of the smallest.
            let mut least = r;
            for v in [w / 2.0, h / 2.0] {
                if v < least {
                    least = v;
                }
            }
            let r = at_least_zero(least);
            if r == 0.0 {
                let corners = [Value::Number(x), Value::Number(y), Value::Number(w), Value::Number(h)];
                path_method(paths, pen.b, m, Method::Rect, &corners);
                return;
            }
            pen.move_to(x + r, y);
            pen.line(x + w - r, y);
            pen.rounded_corner(x + w - r, y + r, r, -PI / 2.0);
            pen.line(x + w, y + h - r);
            pen.rounded_corner(x + w - r, y + h - r, r, 0.0);
            pen.line(x + r, y + h);
            pen.rounded_corner(x + r, y + h - r, r, PI / 2.0);
            pen.line(x, y + r);
            pen.rounded_corner(x + r, y + r, r, PI);
            pen.close();
            // The spec starts the next subpath at the rectangle's origin, not at the first corner arc.
            pen.set_current(x, y);
        }
        Method::AddPath => {
            let Some(Value::PathRef(r)) = a.first() else { return };
            let Some(src) = paths.get(&(*r as i32)) else { return };
            let b = pen.b;
            if b.geometry.points.len() + src.geometry.points.len() > MAX_PATH_FLOATS {
                return;
            }
            let g = transformed(&src.geometry, m);
            b.geometry.ops.extend_from_slice(&g.ops);
            b.geometry.points.extend_from_slice(&g.points);
            b.open = src.open;
            b.has_point = src.has_point;
            if g.points.len() >= 2 {
                b.cx = g.points[g.points.len() - 2];
                b.cy = g.points[g.points.len() - 1];
            }
        }
        _ => {}
    }
}

/// Appends an SVG path (`d`) to a geometry, arcs as cubic béziers. Shared with Path2D.
pub fn append_svg_path(d: &[u8], out: &mut Geometry) {
    // Each token is a run of `d`: a command letter, or a number as strtod may read it.
    let mut tokens: Vec<(usize, usize)> = Vec::new();
    let (mut start, mut len, mut dot) = (0usize, 0usize, false);
    let flush = |tokens: &mut Vec<(usize, usize)>, start: usize, len: &mut usize, dot: &mut bool| {
        if *len > 0 {
            tokens.push((start, start + *len));
        }
        *len = 0;
        *dot = false;
    };
    for (i, &ch) in d.iter().enumerate() {
        let letter = ch.is_ascii_alphabetic();
        if letter && ch != b'e' && ch != b'E' {
            flush(&mut tokens, start, &mut len, &mut dot);
            tokens.push((i, i + 1));
        } else if matches!(ch, b',' | b' ' | b'\n' | b'\t' | b'\r') {
            flush(&mut tokens, start, &mut len, &mut dot);
        } else if ch == b'-' && len > 0 && d[start + len - 1] != b'e' && d[start + len - 1] != b'E' {
            flush(&mut tokens, start, &mut len, &mut dot);
            (start, len) = (i, 1);
        } else if ch == b'.' && dot {
            flush(&mut tokens, start, &mut len, &mut dot);
            (start, len, dot) = (i, 1, true);
        } else {
            if len == 0 {
                start = i;
            }
            len += 1;
            dot |= ch == b'.';
        }
    }
    flush(&mut tokens, start, &mut len, &mut dot);

    let token = |i: usize| &d[tokens[i].0..tokens[i].1];
    let mut i = 0;
    let next = |i: &mut usize| -> Option<f64> {
        if *i >= tokens.len() {
            return None;
        }
        let v = twidget_css::number(token(*i))?;
        *i += 1;
        Some(v)
    };
    let push = |out: &mut Geometry, op: PathOp, pts: &[f64]| {
        out.ops.push(op);
        out.points.extend(pts.iter().map(|v| *v as f32));
    };
    let mut command = b'M';
    let (mut px, mut py, mut sx, mut sy) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut has_control = false;
    let (mut lcx, mut lcy) = (0.0f64, 0.0f64);
    let mut open = false;
    let ensure = |out: &mut Geometry, open: &mut bool, px: f64, py: f64| {
        if !*open {
            push(out, PathOp::Move, &[px, py]);
            *open = true;
        }
    };
    while i < tokens.len() {
        let t = token(i);
        if t.len() == 1 && t[0].is_ascii_alphabetic() {
            command = t[0];
            i += 1;
            if command == b'Z' || command == b'z' {
                if open {
                    push(out, PathOp::Close, &[]);
                }
                open = false;
                px = sx;
                py = sy;
                has_control = false;
                continue;
            }
        }
        let relative = command.is_ascii_lowercase();
        let (ox, oy) = if relative { (px, py) } else { (0.0, 0.0) };
        match command & !0x20 {
            b'M' => {
                let Some(x) = next(&mut i) else { return };
                let Some(y) = next(&mut i) else { return };
                px = ox + x;
                py = oy + y;
                push(out, PathOp::Move, &[px, py]);
                open = true;
                sx = px;
                sy = py;
                command = if relative { b'l' } else { b'L' };
                has_control = false;
            }
            b'L' => {
                let Some(x) = next(&mut i) else { return };
                let Some(y) = next(&mut i) else { return };
                ensure(out, &mut open, px, py);
                px = ox + x;
                py = oy + y;
                push(out, PathOp::Line, &[px, py]);
                has_control = false;
            }
            b'H' => {
                let Some(x) = next(&mut i) else { return };
                ensure(out, &mut open, px, py);
                px = (if relative { px } else { 0.0 }) + x;
                push(out, PathOp::Line, &[px, py]);
                has_control = false;
            }
            b'V' => {
                let Some(y) = next(&mut i) else { return };
                ensure(out, &mut open, px, py);
                py = (if relative { py } else { 0.0 }) + y;
                push(out, PathOp::Line, &[px, py]);
                has_control = false;
            }
            b'C' => {
                let mut v = [0.0f64; 6];
                for x in &mut v {
                    let Some(n) = next(&mut i) else { return };
                    *x = n;
                }
                let [x1, y1, x2, y2, x, y] = v;
                ensure(out, &mut open, px, py);
                push(out, PathOp::Cubic, &[ox + x1, oy + y1, ox + x2, oy + y2, ox + x, oy + y]);
                lcx = ox + x2;
                lcy = oy + y2;
                has_control = true;
                px = ox + x;
                py = oy + y;
            }
            b'S' => {
                let mut v = [0.0f64; 4];
                for x in &mut v {
                    let Some(n) = next(&mut i) else { return };
                    *x = n;
                }
                let [x2, y2, x, y] = v;
                ensure(out, &mut open, px, py);
                let (c1x, c1y) = if has_control { (2.0 * px - lcx, 2.0 * py - lcy) } else { (px, py) };
                push(out, PathOp::Cubic, &[c1x, c1y, ox + x2, oy + y2, ox + x, oy + y]);
                lcx = ox + x2;
                lcy = oy + y2;
                has_control = true;
                px = ox + x;
                py = oy + y;
            }
            b'Q' => {
                let mut v = [0.0f64; 4];
                for x in &mut v {
                    let Some(n) = next(&mut i) else { return };
                    *x = n;
                }
                let [x1, y1, x, y] = v;
                ensure(out, &mut open, px, py);
                push(out, PathOp::Quad, &[ox + x1, oy + y1, ox + x, oy + y]);
                lcx = ox + x1;
                lcy = oy + y1;
                has_control = true;
                px = ox + x;
                py = oy + y;
            }
            b'T' => {
                let Some(x) = next(&mut i) else { return };
                let Some(y) = next(&mut i) else { return };
                ensure(out, &mut open, px, py);
                let (cx, cy) = if has_control { (2.0 * px - lcx, 2.0 * py - lcy) } else { (px, py) };
                push(out, PathOp::Quad, &[cx, cy, ox + x, oy + y]);
                lcx = cx;
                lcy = cy;
                has_control = true;
                px = ox + x;
                py = oy + y;
            }
            b'A' => {
                let mut v = [0.0f64; 7];
                for x in &mut v {
                    let Some(n) = next(&mut i) else { return };
                    *x = n;
                }
                let [mut rx, mut ry, rotation, large, sweep_flag, x, y] = v;
                ensure(out, &mut open, px, py);
                let (x2, y2) = (ox + x, oy + y);
                rx = rx.abs();
                ry = ry.abs();
                if rx == 0.0 || ry == 0.0 || (x2 == px && y2 == py) {
                    push(out, PathOp::Line, &[x2, y2]);
                } else {
                    // SVG 1.1 F.6.5: endpoint to centre parameterisation.
                    let phi = rotation * PI / 180.0;
                    let (c, s) = (phi.cos(), phi.sin());
                    let (dx, dy) = ((px - x2) / 2.0, (py - y2) / 2.0);
                    let (x1p, y1p) = (c * dx + s * dy, -s * dx + c * dy);
                    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
                    if lambda > 1.0 {
                        rx *= lambda.sqrt();
                        ry *= lambda.sqrt();
                    }
                    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
                    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
                    let k = at_least_zero(num / den).sqrt() * if (large != 0.0) == (sweep_flag != 0.0) { -1.0 } else { 1.0 };
                    let (cxp, cyp) = (k * rx * y1p / ry, -k * ry * x1p / rx);
                    let (cx, cy) = (c * cxp - s * cyp + (px + x2) / 2.0, s * cxp + c * cyp + (py + y2) / 2.0);
                    let angle = |ux: f64, uy: f64, vx: f64, vy: f64| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
                    let theta = angle(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
                    let mut delta = angle((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
                    if sweep_flag == 0.0 && delta > 0.0 {
                        delta -= 2.0 * PI;
                    } else if sweep_flag != 0.0 && delta < 0.0 {
                        delta += 2.0 * PI;
                    }
                    arc_segments(cx, cy, rx, ry, phi, theta, delta, |a1, b1, a2, b2, a3, b3| {
                        push(out, PathOp::Cubic, &[a1, b1, a2, b2, a3, b3]);
                    });
                }
                px = x2;
                py = y2;
                has_control = false;
            }
            _ => return,
        }
    }
}

/// The renderer's `measureText` answer: `text` in the canvas font `font` names, the one fillText draws it
/// with. The renderer flushes its canvas fonts before, as it does before a batch.
pub fn measure_text(host: &mut dyn CanvasHost, font: &[u8], text: &[u8]) -> Json {
    let mut fm = FontMetrics::default();
    let handle = host.canvas_font(font, &mut fm);
    let width = if handle != 0 { host.canvas_text_width(handle, text) } else { 0.0 };
    let mut o = Json::object();
    o.set("width", f64::from(width));
    o.set("actualBoundingBoxLeft", 0).set("actualBoundingBoxRight", f64::from(width));
    o.set("actualBoundingBoxAscent", f64::from(fm.ascent * 0.8)).set("actualBoundingBoxDescent", f64::from(fm.descent * 0.6));
    o.set("fontBoundingBoxAscent", f64::from(fm.ascent)).set("fontBoundingBoxDescent", f64::from(fm.descent));
    o.set("emHeightAscent", f64::from(fm.em_ascent)).set("emHeightDescent", f64::from(fm.em_descent));
    o.set("hangingBaseline", f64::from(fm.ascent * 0.8)).set("alphabeticBaseline", 0).set("ideographicBaseline", f64::from(-fm.descent));
    o
}

/// The backend's read of a canvas's pixels: x, y, w, h into straight RGBA; false when it cannot.
pub type PixelRead<'a> = &'a mut dyn FnMut(i32, i32, i32, i32, &mut Vec<u8>) -> bool;

/// The renderer's `imageData` answer for a `{w, h, x, y}` query: the area's size, and its pixels as base64,
/// those `read` gives (the backend's `canvas_read` of the canvas, None without one) when they are all of
/// them, else transparent. No pixels past the canvas pixel bound.
pub fn image_data(query: &Json, read: Option<PixelRead>) -> (Json, Option<String>) {
    let w = query.get("w").as_int(0).clamp(0, Canvas::MAX_SIDE);
    let h = query.get("h").as_int(0).clamp(0, Canvas::MAX_SIDE);
    if i64::from(w) * i64::from(h) > Canvas::MAX_PIXELS {
        let mut error = Json::object();
        error.set("error", "too large");
        return (error, None);
    }
    let size = w as usize * h as usize * 4;
    let mut rgba = Vec::new();
    let read = match read {
        Some(read) => read(query.get("x").as_int(0), query.get("y").as_int(0), w, h, &mut rgba) && rgba.len() == size,
        None => false,
    };
    if !read {
        rgba.clear();
        rgba.resize(size, 0);
    }
    let mut o = Json::object();
    o.set("width", w).set("height", h);
    (o, Some(twidget_base64::encode(&rgba)))
}
