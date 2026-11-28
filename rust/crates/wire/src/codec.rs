//! `core/src/wire.cpp`: a Writer appends fields to the caller's bytes, a Reader takes them from what is
//! left, checking every count against its cap and against what is left before making room for it.
//! The layouts are `docs/TWIDGET_CORE.md`, "Binary forms".

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;

use crate::values::*;

/// What a value is: its first byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Form {
    List = 1,
    Damage = 2,
    Batch = 3,
    Metrics = 4,
}

/// A value's second byte, the layout of every form: a changed layout takes a new version.
pub const VERSION: u8 = 1;

/// The caller's bounds, each on one counted field of a value and not on a total (the length of the
/// bytes bounds that); a count travels as a u32 whatever its cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    /// Commands of one display list.
    pub commands: usize,
    /// CanvasOps of one batch.
    pub canvas_ops: usize,
    /// Ops of one Path or Geometry.
    pub path_ops: usize,
    /// Points of one Path or Geometry; dashes of one Path or CanvasOp.
    pub floats: usize,
    /// Stops of one gradient: a Command's, a Path's fill or stroke, a CanvasPaint's.
    pub stops: usize,
    /// Functions of one filter list.
    pub filters: usize,
    /// Clips of one clip list.
    pub clips: usize,
    /// Bytes of one string: Command::text, CanvasOp::text.
    pub text: usize,
    /// Bytes of one rgba payload.
    pub pixels: usize,
    /// Rectangles of one damage.
    pub rects: usize,
}

impl Default for Caps {
    fn default() -> Caps {
        Caps {
            commands: 1 << 20,
            canvas_ops: 1 << 20,
            path_ops: 1 << 22,
            floats: 1 << 22,
            stops: 1 << 16,
            filters: 1 << 12,
            clips: 1 << 16,
            text: 4 << 20,
            pixels: 64 << 20,
            rects: 1 << 12,
        }
    }
}

/// What moved since the last paint: None when the whole frame must be painted, an empty vector when
/// nothing moved.
pub type Damage = Option<Vec<Rect>>;

const CMD_RECT: u32 = 0;
const CMD_RADII: u32 = 1;
const CMD_CLIP: u32 = 2;
const CMD_HAS_CLIP: u32 = 3;
const CMD_COLOR: u32 = 4;
const CMD_WIDTH: u32 = 5;
const CMD_FONT: u32 = 6;
const CMD_TEXT: u32 = 7;
const CMD_X: u32 = 8;
const CMD_Y: u32 = 9;
const CMD_IMAGE: u32 = 10;
const CMD_CANVAS: u32 = 11;
const CMD_AX: u32 = 12;
const CMD_AY: u32 = 13;
const CMD_BX: u32 = 14;
const CMD_BY: u32 = 15;
const CMD_STOPS: u32 = 16;
const CMD_PATH: u32 = 17;
const CMD_FILTER: u32 = 18;
const CMD_OPACITY: u32 = 19;
const CMD_MATRIX: u32 = 20;
const CMD_HIDDEN: u32 = 21;
const CMD_INSET: u32 = 22;
const CMD_KEY: u32 = 23;
const COMMAND_BITS: u32 = 24;

const PATH_OPS: u32 = 0;
const PATH_POINTS: u32 = 1;
const PATH_FILL: u32 = 2;
const PATH_EVENODD: u32 = 3;
const PATH_FILL_COLOR: u32 = 4;
const PATH_GRADIENT: u32 = 5;
const PATH_X1: u32 = 6;
const PATH_Y1: u32 = 7;
const PATH_X2: u32 = 8;
const PATH_Y2: u32 = 9;
const PATH_R: u32 = 10;
const PATH_FX: u32 = 11;
const PATH_FY: u32 = 12;
const PATH_STOPS: u32 = 13;
const PATH_STROKE: u32 = 14;
const PATH_STROKE_COLOR: u32 = 15;
const PATH_STROKE_WIDTH: u32 = 16;
const PATH_LINE_CAP: u32 = 17;
const PATH_LINE_JOIN: u32 = 18;
const PATH_MITER_LIMIT: u32 = 19;
const PATH_DASHES: u32 = 20;
const PATH_DASH_OFFSET: u32 = 21;
const PATH_STROKE_GRADIENT: u32 = 22;
const PATH_SG: u32 = 23;
const PATH_STROKE_STOPS: u32 = 24;
const PATH_BITS: u32 = 25;

const OP_PATH: u32 = 0;
const OP_EVENODD: u32 = 1;
const OP_PAINT: u32 = 2;
const OP_MATRIX: u32 = 3;
const OP_LINE_WIDTH: u32 = 4;
const OP_LINE_CAP: u32 = 5;
const OP_LINE_JOIN: u32 = 6;
const OP_MITER_LIMIT: u32 = 7;
const OP_DASHES: u32 = 8;
const OP_DASH_OFFSET: u32 = 9;
const OP_ALPHA: u32 = 10;
const OP_COMPOSITE: u32 = 11;
const OP_SHADOW_COLOR: u32 = 12;
const OP_SHADOW_BLUR: u32 = 13;
const OP_SHADOW_X: u32 = 14;
const OP_SHADOW_Y: u32 = 15;
const OP_CLIPS: u32 = 16;
const OP_FONT: u32 = 17;
const OP_TEXT: u32 = 18;
const OP_X: u32 = 19;
const OP_Y: u32 = 20;
const OP_SCALE_X: u32 = 21;
const OP_TEXT_DX: u32 = 22;
const OP_STROKE_TEXT: u32 = 23;
const OP_IMAGE: u32 = 24;
const OP_CANVAS: u32 = 25;
const OP_SX: u32 = 26;
const OP_SY: u32 = 27;
const OP_SW: u32 = 28;
const OP_SH: u32 = 29;
const OP_DX: u32 = 30;
const OP_DY: u32 = 31;
const OP_DW: u32 = 32;
const OP_DH: u32 = 33;
const OP_SMOOTHING: u32 = 34;
const OP_RGBA: u32 = 35;
const OP_W: u32 = 36;
const OP_H: u32 = 37;
const CANVAS_OP_BITS: u32 = 38;

const PAINT_COLOR: u32 = 0;
const PAINT_ARGS: u32 = 1;
const PAINT_STOPS: u32 = 2;
const PAINT_BITS: u32 = 3;

/// The fewest bytes one item of a counted list takes.
const STOP_BYTES: usize = 8;
const FLOAT_BYTES: usize = 4;
const OP_BYTES: usize = 1;
const FILTER_BYTES: usize = 17;
const COMMAND_BYTES: usize = 5;
const CANVAS_OP_BYTES: usize = 9;
const CLIP_BYTES: usize = 9;
const RECT_BYTES: usize = 16;
/// A list's form, version and command count.
const LIST_HEAD_BYTES: usize = 2 + 4;

// Version 1's defaults, written out rather than taken from the structs, so that old bytes keep their meaning.
const ZERO_RECT: Rect = Rect::new(0.0, 0.0, 0.0, 0.0);
const ZERO_RADII: Radii = Radii { tlx: 0.0, tly: 0.0, trx: 0.0, try_: 0.0, brx: 0.0, bry: 0.0, blx: 0.0, bly: 0.0 };

static V1_COMMAND: Command = Command {
    kind: Cmd::FillRect,
    rect: ZERO_RECT,
    radii: ZERO_RADII,
    clip: ZERO_RECT,
    has_clip: false,
    color: Color::BLACK,
    width: 0.0,
    font: 0,
    text: Vec::new(),
    x: 0.0,
    y: 0.0,
    image: 0,
    canvas: 0,
    ax: 0.0,
    ay: 0.0,
    bx: 0.0,
    by: 0.0,
    stops: Vec::new(),
    path: None,
    filter: None,
    opacity: 1.0,
    matrix: IDENTITY_MATRIX,
    hidden: false,
    inset: false,
    key: 0,
};

static V1_PATH: Path = Path {
    ops: Vec::new(),
    points: Vec::new(),
    fill: false,
    evenodd: false,
    fill_color: Color::BLACK,
    gradient: 0,
    x1: 0.0,
    y1: 0.0,
    x2: 0.0,
    y2: 0.0,
    r: 0.0,
    fx: 0.0,
    fy: 0.0,
    stops: Vec::new(),
    stroke: false,
    stroke_color: Color::BLACK,
    stroke_width: 1.0,
    line_cap: 0,
    line_join: 0,
    miter_limit: 4.0,
    dashes: Vec::new(),
    dash_offset: 0.0,
    stroke_gradient: 0,
    sg: [0.0; 7],
    stroke_stops: Vec::new(),
};

static V1_PAINT: CanvasPaint = CanvasPaint { kind: PaintKind::Solid, color: Color::BLACK, args: [0.0; 6], stops: Vec::new() };

static V1_OP: CanvasOp = CanvasOp {
    kind: OpKind::Fill,
    path: Geometry { ops: Vec::new(), points: Vec::new() },
    evenodd: false,
    paint: CanvasPaint { kind: PaintKind::Solid, color: Color::BLACK, args: [0.0; 6], stops: Vec::new() },
    matrix: IDENTITY_MATRIX,
    line_width: 1.0,
    line_cap: 0,
    line_join: 0,
    miter_limit: 10.0,
    dashes: Vec::new(),
    dash_offset: 0.0,
    alpha: 1.0,
    composite: Composite::SourceOver,
    shadow_color: Color::TRANSPARENT,
    shadow_blur: 0.0,
    shadow_x: 0.0,
    shadow_y: 0.0,
    clips: None,
    font: 0,
    text: Vec::new(),
    x: 0.0,
    y: 0.0,
    scale_x: 1.0,
    text_dx: 0.0,
    stroke_text: false,
    image: 0,
    canvas: 0,
    sx: 0.0,
    sy: 0.0,
    sw: -1.0,
    sh: -1.0,
    dx: 0.0,
    dy: 0.0,
    dw: 0.0,
    dh: 0.0,
    smoothing: true,
    rgba: None,
    w: 0,
    h: 0,
};

/// Version 1's defaults of each value, which an absent member decodes to.
pub fn version_1_command() -> Command {
    V1_COMMAND.clone()
}

pub fn version_1_path() -> Path {
    V1_PATH.clone()
}

pub fn version_1_paint() -> CanvasPaint {
    V1_PAINT.clone()
}

pub fn version_1_op() -> CanvasOp {
    V1_OP.clone()
}

fn same(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits()
}

fn same_all(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same(*x, *y))
}

fn same_rect(a: &Rect, b: &Rect) -> bool {
    same(a.x, b.x) && same(a.y, b.y) && same(a.w, b.w) && same(a.h, b.h)
}

fn same_radii(a: &Radii, b: &Radii) -> bool {
    same(a.tlx, b.tlx)
        && same(a.tly, b.tly)
        && same(a.trx, b.trx)
        && same(a.try_, b.try_)
        && same(a.brx, b.brx)
        && same(a.bry, b.bry)
        && same(a.blx, b.blx)
        && same(a.bly, b.bly)
}

fn has(mask: u64, bit: u32) -> bool {
    (mask >> bit) & 1 != 0
}

struct Writer<'a> {
    out: &'a mut Vec<u8>,
}

impl Writer<'_> {
    fn u8(&mut self, v: u8) {
        self.out.push(v);
    }

    fn u32(&mut self, v: u32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    fn i32(&mut self, v: i32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }

    fn f32s(&mut self, v: &[f32]) {
        for f in v {
            self.f32(*f);
        }
    }

    fn color(&mut self, c: Color) {
        self.out.extend_from_slice(&[c.r, c.g, c.b, c.a]);
    }

    fn rect(&mut self, r: &Rect) {
        self.f32s(&[r.x, r.y, r.w, r.h]);
    }

    fn radii(&mut self, r: &Radii) {
        self.f32s(&[r.tlx, r.tly, r.trx, r.try_, r.brx, r.bry, r.blx, r.bly]);
    }

    fn flag(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    fn head(&mut self, form: Form) {
        self.u8(form as u8);
        self.u8(VERSION);
    }

    fn count(&mut self, n: usize, cap: usize) -> bool {
        let Ok(n) = u32::try_from(n) else {
            return false;
        };
        if n as usize > cap {
            return false;
        }
        self.u32(n);
        true
    }

    fn bytes(&mut self, v: &[u8], cap: usize) -> bool {
        if !self.count(v.len(), cap) {
            return false;
        }
        self.out.extend_from_slice(v);
        true
    }

    fn stops(&mut self, v: &[Stop], cap: usize) -> bool {
        if !self.count(v.len(), cap) {
            return false;
        }
        for s in v {
            self.f32(s.offset);
            self.color(s.color);
        }
        true
    }

    fn floats(&mut self, v: &[f32], cap: usize) -> bool {
        if !self.count(v.len(), cap) {
            return false;
        }
        self.f32s(v);
        true
    }

    fn path_ops(&mut self, v: &[PathOp], cap: usize) -> bool {
        if !self.count(v.len(), cap) {
            return false;
        }
        for op in v {
            self.u8(op.as_u8());
        }
        true
    }

    fn geometry(&mut self, g: &Geometry, caps: &Caps) -> bool {
        self.path_ops(&g.ops, caps.path_ops) && self.floats(&g.points, caps.floats)
    }
}

/// A presence mask of `N` bytes put as 0 before the members it marks and patched after them, so
/// `if m.set(BIT, ...) { w.field(...) }` writes each in its place.
struct Mask<const N: usize> {
    at: usize,
    bits: u64,
}

impl<const N: usize> Mask<N> {
    fn start(w: &mut Writer) -> Mask<N> {
        let at = w.out.len();
        w.out.extend_from_slice(&[0; N]);
        Mask { at, bits: 0 }
    }

    fn set(&mut self, bit: u32, present: bool) -> bool {
        if present {
            self.bits |= 1 << bit;
        }
        present
    }

    /// A bool of no bytes: its bit says it is true.
    fn flag(&mut self, bit: u32, value: bool) {
        self.set(bit, value);
    }

    fn finish(self, w: &mut Writer) {
        w.out[self.at..self.at + N].copy_from_slice(&self.bits.to_le_bytes()[..N]);
    }
}

/// Reads fields, each only when all of it is left.
struct Reader<'a> {
    rest: &'a [u8],
}

impl Reader<'_> {
    fn done(&self) -> bool {
        self.rest.is_empty()
    }

    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, tail) = self.rest.split_first_chunk::<N>()?;
        self.rest = tail;
        Some(*head)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take::<1>().map(|b| b[0])
    }

    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_le_bytes)
    }

    fn u64(&mut self) -> Option<u64> {
        self.take().map(u64::from_le_bytes)
    }

    fn i32(&mut self) -> Option<i32> {
        self.take().map(i32::from_le_bytes)
    }

    fn f32(&mut self) -> Option<f32> {
        self.u32().map(f32::from_bits)
    }

    fn f32s<const N: usize>(&mut self) -> Option<[f32; N]> {
        let mut out = [0.0; N];
        for f in &mut out {
            *f = self.f32()?;
        }
        Some(out)
    }

    fn color(&mut self) -> Option<Color> {
        let [r, g, b, a] = self.take()?;
        Some(Color { r, g, b, a })
    }

    fn rect(&mut self) -> Option<Rect> {
        let [x, y, w, h] = self.f32s()?;
        Some(Rect { x, y, w, h })
    }

    fn radii(&mut self) -> Option<Radii> {
        let [tlx, tly, trx, try_, brx, bry, blx, bly] = self.f32s()?;
        Some(Radii { tlx, tly, trx, try_, brx, bry, blx, bly })
    }

    fn flag(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    fn head(&mut self, form: Form) -> Option<()> {
        (self.u8()? == form as u8 && self.u8()? == VERSION).then_some(())
    }

    /// A count of items of at least `item_bytes` each, checked by division so that nothing wraps.
    fn count(&mut self, cap: usize, item_bytes: usize) -> Option<usize> {
        let c = self.u32()? as usize;
        (c <= cap && c <= self.rest.len() / item_bytes).then_some(c)
    }

    fn bytes(&mut self, cap: usize) -> Option<Vec<u8>> {
        let n = self.count(cap, 1)?;
        let (head, tail) = self.rest.split_at(n);
        self.rest = tail;
        Some(head.to_vec())
    }

    fn stops(&mut self, cap: usize) -> Option<Vec<Stop>> {
        let n = self.count(cap, STOP_BYTES)?;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            let offset = self.f32()?;
            let color = self.color()?;
            v.push(Stop { offset, color });
        }
        Some(v)
    }

    fn floats(&mut self, cap: usize) -> Option<Vec<f32>> {
        let n = self.count(cap, FLOAT_BYTES)?;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(self.f32()?);
        }
        Some(v)
    }

    fn path_ops(&mut self, cap: usize) -> Option<Vec<PathOp>> {
        let n = self.count(cap, OP_BYTES)?;
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(PathOp::from_u8(self.u8()?)?);
        }
        Some(v)
    }

    fn geometry(&mut self, caps: &Caps) -> Option<Geometry> {
        let ops = self.path_ops(caps.path_ops)?;
        let points = self.floats(caps.floats)?;
        Some(Geometry { ops, points })
    }
}

/// The payloads of one kind a value has written, by address: the first user puts 0 and the payload, a
/// later one its place from 1.
struct WrittenPayloads<T> {
    places: HashMap<*const T, u32>,
}

impl<T> WrittenPayloads<T> {
    fn new() -> WrittenPayloads<T> {
        WrittenPayloads { places: HashMap::new() }
    }

    fn put(&mut self, w: &mut Writer, p: &Arc<T>, write: impl FnOnce(&mut Writer, &T) -> bool) -> bool {
        let next = self.places.len() as u32 + 1;
        match self.places.entry(Arc::as_ptr(p)) {
            Entry::Occupied(at) => {
                w.u32(*at.get());
                true
            }
            Entry::Vacant(place) => {
                place.insert(next);
                w.u32(0);
                write(w, p)
            }
        }
    }
}

/// The payloads of one kind a value has read, in order; a new one joins them only once it is whole.
struct ReadPayloads<T> {
    payloads: Vec<Arc<T>>,
}

impl<T> ReadPayloads<T> {
    fn new() -> ReadPayloads<T> {
        ReadPayloads { payloads: Vec::new() }
    }

    fn get(&mut self, r: &mut Reader, read: impl FnOnce(&mut Reader) -> Option<T>) -> Option<Arc<T>> {
        let place = r.u32()? as usize;
        if place != 0 {
            return self.payloads.get(place - 1).cloned();
        }
        let fresh = Arc::new(read(r)?);
        self.payloads.push(Arc::clone(&fresh));
        Some(fresh)
    }
}

/// Runs `write` over `out`, cutting `out` back to its size on entry when it refuses.
fn appended(out: &mut Vec<u8>, write: impl FnOnce(&mut Writer) -> bool) -> bool {
    let start = out.len();
    if write(&mut Writer { out }) {
        return true;
    }
    out.truncate(start);
    false
}

fn write_path(w: &mut Writer, caps: &Caps, p: &Path) -> bool {
    let d = &V1_PATH;
    let mut m = Mask::<4>::start(w);
    if m.set(PATH_OPS, !p.ops.is_empty()) && !w.path_ops(&p.ops, caps.path_ops) {
        return false;
    }
    if m.set(PATH_POINTS, !p.points.is_empty()) && !w.floats(&p.points, caps.floats) {
        return false;
    }
    m.flag(PATH_FILL, p.fill);
    m.flag(PATH_EVENODD, p.evenodd);
    if m.set(PATH_FILL_COLOR, p.fill_color != d.fill_color) {
        w.color(p.fill_color);
    }
    if m.set(PATH_GRADIENT, p.gradient != d.gradient) {
        w.i32(p.gradient);
    }
    if m.set(PATH_X1, !same(p.x1, d.x1)) {
        w.f32(p.x1);
    }
    if m.set(PATH_Y1, !same(p.y1, d.y1)) {
        w.f32(p.y1);
    }
    if m.set(PATH_X2, !same(p.x2, d.x2)) {
        w.f32(p.x2);
    }
    if m.set(PATH_Y2, !same(p.y2, d.y2)) {
        w.f32(p.y2);
    }
    if m.set(PATH_R, !same(p.r, d.r)) {
        w.f32(p.r);
    }
    if m.set(PATH_FX, !same(p.fx, d.fx)) {
        w.f32(p.fx);
    }
    if m.set(PATH_FY, !same(p.fy, d.fy)) {
        w.f32(p.fy);
    }
    if m.set(PATH_STOPS, !p.stops.is_empty()) && !w.stops(&p.stops, caps.stops) {
        return false;
    }
    m.flag(PATH_STROKE, p.stroke);
    if m.set(PATH_STROKE_COLOR, p.stroke_color != d.stroke_color) {
        w.color(p.stroke_color);
    }
    if m.set(PATH_STROKE_WIDTH, !same(p.stroke_width, d.stroke_width)) {
        w.f32(p.stroke_width);
    }
    if m.set(PATH_LINE_CAP, p.line_cap != d.line_cap) {
        w.i32(p.line_cap);
    }
    if m.set(PATH_LINE_JOIN, p.line_join != d.line_join) {
        w.i32(p.line_join);
    }
    if m.set(PATH_MITER_LIMIT, !same(p.miter_limit, d.miter_limit)) {
        w.f32(p.miter_limit);
    }
    if m.set(PATH_DASHES, !p.dashes.is_empty()) && !w.floats(&p.dashes, caps.floats) {
        return false;
    }
    if m.set(PATH_DASH_OFFSET, !same(p.dash_offset, d.dash_offset)) {
        w.f32(p.dash_offset);
    }
    if m.set(PATH_STROKE_GRADIENT, p.stroke_gradient != d.stroke_gradient) {
        w.i32(p.stroke_gradient);
    }
    if m.set(PATH_SG, !same_all(&p.sg, &d.sg)) {
        w.f32s(&p.sg);
    }
    if m.set(PATH_STROKE_STOPS, !p.stroke_stops.is_empty()) && !w.stops(&p.stroke_stops, caps.stops) {
        return false;
    }
    m.finish(w);
    true
}

fn read_path(r: &mut Reader, caps: &Caps) -> Option<Path> {
    let mask = u64::from(r.u32()?);
    if mask >> PATH_BITS != 0 {
        return None;
    }
    let mut p = V1_PATH.clone();
    if has(mask, PATH_OPS) {
        p.ops = r.path_ops(caps.path_ops)?;
    }
    if has(mask, PATH_POINTS) {
        p.points = r.floats(caps.floats)?;
    }
    if has(mask, PATH_FILL) {
        p.fill = true;
    }
    if has(mask, PATH_EVENODD) {
        p.evenodd = true;
    }
    if has(mask, PATH_FILL_COLOR) {
        p.fill_color = r.color()?;
    }
    if has(mask, PATH_GRADIENT) {
        p.gradient = r.i32()?;
    }
    if has(mask, PATH_X1) {
        p.x1 = r.f32()?;
    }
    if has(mask, PATH_Y1) {
        p.y1 = r.f32()?;
    }
    if has(mask, PATH_X2) {
        p.x2 = r.f32()?;
    }
    if has(mask, PATH_Y2) {
        p.y2 = r.f32()?;
    }
    if has(mask, PATH_R) {
        p.r = r.f32()?;
    }
    if has(mask, PATH_FX) {
        p.fx = r.f32()?;
    }
    if has(mask, PATH_FY) {
        p.fy = r.f32()?;
    }
    if has(mask, PATH_STOPS) {
        p.stops = r.stops(caps.stops)?;
    }
    if has(mask, PATH_STROKE) {
        p.stroke = true;
    }
    if has(mask, PATH_STROKE_COLOR) {
        p.stroke_color = r.color()?;
    }
    if has(mask, PATH_STROKE_WIDTH) {
        p.stroke_width = r.f32()?;
    }
    if has(mask, PATH_LINE_CAP) {
        p.line_cap = r.i32()?;
    }
    if has(mask, PATH_LINE_JOIN) {
        p.line_join = r.i32()?;
    }
    if has(mask, PATH_MITER_LIMIT) {
        p.miter_limit = r.f32()?;
    }
    if has(mask, PATH_DASHES) {
        p.dashes = r.floats(caps.floats)?;
    }
    if has(mask, PATH_DASH_OFFSET) {
        p.dash_offset = r.f32()?;
    }
    if has(mask, PATH_STROKE_GRADIENT) {
        p.stroke_gradient = r.i32()?;
    }
    if has(mask, PATH_SG) {
        p.sg = r.f32s()?;
    }
    if has(mask, PATH_STROKE_STOPS) {
        p.stroke_stops = r.stops(caps.stops)?;
    }
    Some(p)
}

fn write_filters(w: &mut Writer, caps: &Caps, list: &Filters) -> bool {
    if !w.count(list.len(), caps.filters) {
        return false;
    }
    for f in list {
        w.u8(f.kind.as_u8());
        w.f32(f.amount);
        w.f32(f.x);
        w.f32(f.y);
        w.color(f.color);
    }
    true
}

fn read_filters(r: &mut Reader, caps: &Caps) -> Option<Filters> {
    let n = r.count(caps.filters, FILTER_BYTES)?;
    let mut list = Vec::with_capacity(n);
    for _ in 0..n {
        let kind = FilterKind::from_u8(r.u8()?)?;
        let amount = r.f32()?;
        let x = r.f32()?;
        let y = r.f32()?;
        let color = r.color()?;
        list.push(Filter { kind, amount, x, y, color });
    }
    Some(list)
}

fn write_command(
    w: &mut Writer,
    caps: &Caps,
    paths: &mut WrittenPayloads<Path>,
    filters: &mut WrittenPayloads<Filters>,
    c: &Command,
) -> bool {
    let d = &V1_COMMAND;
    w.u8(c.kind.as_u8());
    let mut m = Mask::<4>::start(w);
    if m.set(CMD_RECT, !same_rect(&c.rect, &d.rect)) {
        w.rect(&c.rect);
    }
    if m.set(CMD_RADII, !same_radii(&c.radii, &d.radii)) {
        w.radii(&c.radii);
    }
    if m.set(CMD_CLIP, !same_rect(&c.clip, &d.clip)) {
        w.rect(&c.clip);
    }
    m.flag(CMD_HAS_CLIP, c.has_clip);
    if m.set(CMD_COLOR, c.color != d.color) {
        w.color(c.color);
    }
    if m.set(CMD_WIDTH, !same(c.width, d.width)) {
        w.f32(c.width);
    }
    if m.set(CMD_FONT, c.font != d.font) {
        w.i32(c.font);
    }
    if m.set(CMD_TEXT, !c.text.is_empty()) && !w.bytes(&c.text, caps.text) {
        return false;
    }
    if m.set(CMD_X, !same(c.x, d.x)) {
        w.f32(c.x);
    }
    if m.set(CMD_Y, !same(c.y, d.y)) {
        w.f32(c.y);
    }
    if m.set(CMD_IMAGE, c.image != d.image) {
        w.i32(c.image);
    }
    if m.set(CMD_CANVAS, c.canvas != d.canvas) {
        w.i32(c.canvas);
    }
    if m.set(CMD_AX, !same(c.ax, d.ax)) {
        w.f32(c.ax);
    }
    if m.set(CMD_AY, !same(c.ay, d.ay)) {
        w.f32(c.ay);
    }
    if m.set(CMD_BX, !same(c.bx, d.bx)) {
        w.f32(c.bx);
    }
    if m.set(CMD_BY, !same(c.by, d.by)) {
        w.f32(c.by);
    }
    if m.set(CMD_STOPS, !c.stops.is_empty()) && !w.stops(&c.stops, caps.stops) {
        return false;
    }
    if let Some(path) = &c.path {
        m.set(CMD_PATH, true);
        if !paths.put(w, path, |w, p| write_path(w, caps, p)) {
            return false;
        }
    }
    if let Some(filter) = &c.filter {
        m.set(CMD_FILTER, true);
        if !filters.put(w, filter, |w, f| write_filters(w, caps, f)) {
            return false;
        }
    }
    if m.set(CMD_OPACITY, !same(c.opacity, d.opacity)) {
        w.f32(c.opacity);
    }
    if m.set(CMD_MATRIX, !same_all(&c.matrix, &d.matrix)) {
        w.f32s(&c.matrix);
    }
    m.flag(CMD_HIDDEN, c.hidden);
    m.flag(CMD_INSET, c.inset);
    if m.set(CMD_KEY, c.key != d.key) {
        w.u64(c.key);
    }
    m.finish(w);
    true
}

fn read_command(r: &mut Reader, caps: &Caps, paths: &mut ReadPayloads<Path>, filters: &mut ReadPayloads<Filters>) -> Option<Command> {
    let kind = Cmd::from_u8(r.u8()?)?;
    let mask = u64::from(r.u32()?);
    if mask >> COMMAND_BITS != 0 {
        return None;
    }
    let mut c = V1_COMMAND.clone();
    c.kind = kind;
    if has(mask, CMD_RECT) {
        c.rect = r.rect()?;
    }
    if has(mask, CMD_RADII) {
        c.radii = r.radii()?;
    }
    if has(mask, CMD_CLIP) {
        c.clip = r.rect()?;
    }
    if has(mask, CMD_HAS_CLIP) {
        c.has_clip = true;
    }
    if has(mask, CMD_COLOR) {
        c.color = r.color()?;
    }
    if has(mask, CMD_WIDTH) {
        c.width = r.f32()?;
    }
    if has(mask, CMD_FONT) {
        c.font = r.i32()?;
    }
    if has(mask, CMD_TEXT) {
        c.text = r.bytes(caps.text)?;
    }
    if has(mask, CMD_X) {
        c.x = r.f32()?;
    }
    if has(mask, CMD_Y) {
        c.y = r.f32()?;
    }
    if has(mask, CMD_IMAGE) {
        c.image = r.i32()?;
    }
    if has(mask, CMD_CANVAS) {
        c.canvas = r.i32()?;
    }
    if has(mask, CMD_AX) {
        c.ax = r.f32()?;
    }
    if has(mask, CMD_AY) {
        c.ay = r.f32()?;
    }
    if has(mask, CMD_BX) {
        c.bx = r.f32()?;
    }
    if has(mask, CMD_BY) {
        c.by = r.f32()?;
    }
    if has(mask, CMD_STOPS) {
        c.stops = r.stops(caps.stops)?;
    }
    if has(mask, CMD_PATH) {
        c.path = Some(paths.get(r, |r| read_path(r, caps))?);
    }
    if has(mask, CMD_FILTER) {
        c.filter = Some(filters.get(r, |r| read_filters(r, caps))?);
    }
    if has(mask, CMD_OPACITY) {
        c.opacity = r.f32()?;
    }
    if has(mask, CMD_MATRIX) {
        c.matrix = r.f32s()?;
    }
    if has(mask, CMD_HIDDEN) {
        c.hidden = true;
    }
    if has(mask, CMD_INSET) {
        c.inset = true;
    }
    if has(mask, CMD_KEY) {
        c.key = r.u64()?;
    }
    Some(c)
}

/// Whether `p` differs from version 1's paint, so that its op carries it.
fn painted(p: &CanvasPaint) -> bool {
    let d = &V1_PAINT;
    p.kind != d.kind || p.color != d.color || !same_all(&p.args, &d.args) || !p.stops.is_empty()
}

fn write_paint(w: &mut Writer, caps: &Caps, p: &CanvasPaint) -> bool {
    let d = &V1_PAINT;
    w.u8(p.kind.as_u8());
    let mut m = Mask::<1>::start(w);
    if m.set(PAINT_COLOR, p.color != d.color) {
        w.color(p.color);
    }
    if m.set(PAINT_ARGS, !same_all(&p.args, &d.args)) {
        w.f32s(&p.args);
    }
    if m.set(PAINT_STOPS, !p.stops.is_empty()) && !w.stops(&p.stops, caps.stops) {
        return false;
    }
    m.finish(w);
    true
}

fn read_paint(r: &mut Reader, caps: &Caps) -> Option<CanvasPaint> {
    let kind = PaintKind::from_u8(r.u8()?)?;
    let mask = u64::from(r.u8()?);
    if mask >> PAINT_BITS != 0 {
        return None;
    }
    let mut p = V1_PAINT.clone();
    p.kind = kind;
    if has(mask, PAINT_COLOR) {
        p.color = r.color()?;
    }
    if has(mask, PAINT_ARGS) {
        p.args = r.f32s()?;
    }
    if has(mask, PAINT_STOPS) {
        p.stops = r.stops(caps.stops)?;
    }
    Some(p)
}

fn write_clips(w: &mut Writer, caps: &Caps, list: &Clips) -> bool {
    if !w.count(list.len(), caps.clips) {
        return false;
    }
    for c in list {
        if !w.geometry(&c.path, caps) {
            return false;
        }
        w.flag(c.evenodd);
    }
    true
}

fn read_clips(r: &mut Reader, caps: &Caps) -> Option<Clips> {
    let n = r.count(caps.clips, CLIP_BYTES)?;
    let mut list = Vec::with_capacity(n);
    for _ in 0..n {
        let path = r.geometry(caps)?;
        let evenodd = r.flag()?;
        list.push(CanvasClip { path, evenodd });
    }
    Some(list)
}

fn write_op(w: &mut Writer, caps: &Caps, clips: &mut WrittenPayloads<Clips>, pixels: &mut WrittenPayloads<Pixels>, o: &CanvasOp) -> bool {
    let d = &V1_OP;
    w.u8(o.kind.as_u8());
    let mut m = Mask::<8>::start(w);
    if m.set(OP_PATH, !o.path.ops.is_empty() || !o.path.points.is_empty()) && !w.geometry(&o.path, caps) {
        return false;
    }
    m.flag(OP_EVENODD, o.evenodd);
    if m.set(OP_PAINT, painted(&o.paint)) && !write_paint(w, caps, &o.paint) {
        return false;
    }
    if m.set(OP_MATRIX, !same_all(&o.matrix, &d.matrix)) {
        w.f32s(&o.matrix);
    }
    if m.set(OP_LINE_WIDTH, !same(o.line_width, d.line_width)) {
        w.f32(o.line_width);
    }
    if m.set(OP_LINE_CAP, o.line_cap != d.line_cap) {
        w.i32(o.line_cap);
    }
    if m.set(OP_LINE_JOIN, o.line_join != d.line_join) {
        w.i32(o.line_join);
    }
    if m.set(OP_MITER_LIMIT, !same(o.miter_limit, d.miter_limit)) {
        w.f32(o.miter_limit);
    }
    if m.set(OP_DASHES, !o.dashes.is_empty()) && !w.floats(&o.dashes, caps.floats) {
        return false;
    }
    if m.set(OP_DASH_OFFSET, !same(o.dash_offset, d.dash_offset)) {
        w.f32(o.dash_offset);
    }
    if m.set(OP_ALPHA, !same(o.alpha, d.alpha)) {
        w.f32(o.alpha);
    }
    if m.set(OP_COMPOSITE, o.composite != d.composite) {
        w.u8(o.composite.as_u8());
    }
    if m.set(OP_SHADOW_COLOR, o.shadow_color != d.shadow_color) {
        w.color(o.shadow_color);
    }
    if m.set(OP_SHADOW_BLUR, !same(o.shadow_blur, d.shadow_blur)) {
        w.f32(o.shadow_blur);
    }
    if m.set(OP_SHADOW_X, !same(o.shadow_x, d.shadow_x)) {
        w.f32(o.shadow_x);
    }
    if m.set(OP_SHADOW_Y, !same(o.shadow_y, d.shadow_y)) {
        w.f32(o.shadow_y);
    }
    if let Some(list) = &o.clips {
        m.set(OP_CLIPS, true);
        if !clips.put(w, list, |w, l| write_clips(w, caps, l)) {
            return false;
        }
    }
    if m.set(OP_FONT, o.font != d.font) {
        w.i32(o.font);
    }
    if m.set(OP_TEXT, !o.text.is_empty()) && !w.bytes(&o.text, caps.text) {
        return false;
    }
    if m.set(OP_X, !same(o.x, d.x)) {
        w.f32(o.x);
    }
    if m.set(OP_Y, !same(o.y, d.y)) {
        w.f32(o.y);
    }
    if m.set(OP_SCALE_X, !same(o.scale_x, d.scale_x)) {
        w.f32(o.scale_x);
    }
    if m.set(OP_TEXT_DX, !same(o.text_dx, d.text_dx)) {
        w.f32(o.text_dx);
    }
    m.flag(OP_STROKE_TEXT, o.stroke_text);
    if m.set(OP_IMAGE, o.image != d.image) {
        w.i32(o.image);
    }
    if m.set(OP_CANVAS, o.canvas != d.canvas) {
        w.i32(o.canvas);
    }
    if m.set(OP_SX, !same(o.sx, d.sx)) {
        w.f32(o.sx);
    }
    if m.set(OP_SY, !same(o.sy, d.sy)) {
        w.f32(o.sy);
    }
    if m.set(OP_SW, !same(o.sw, d.sw)) {
        w.f32(o.sw);
    }
    if m.set(OP_SH, !same(o.sh, d.sh)) {
        w.f32(o.sh);
    }
    if m.set(OP_DX, !same(o.dx, d.dx)) {
        w.f32(o.dx);
    }
    if m.set(OP_DY, !same(o.dy, d.dy)) {
        w.f32(o.dy);
    }
    if m.set(OP_DW, !same(o.dw, d.dw)) {
        w.f32(o.dw);
    }
    if m.set(OP_DH, !same(o.dh, d.dh)) {
        w.f32(o.dh);
    }
    m.flag(OP_SMOOTHING, !o.smoothing);
    if let Some(px) = &o.rgba {
        m.set(OP_RGBA, true);
        if !pixels.put(w, px, |w, p| w.bytes(p, caps.pixels)) {
            return false;
        }
    }
    if m.set(OP_W, o.w != d.w) {
        w.i32(o.w);
    }
    if m.set(OP_H, o.h != d.h) {
        w.i32(o.h);
    }
    m.finish(w);
    true
}

fn read_op(r: &mut Reader, caps: &Caps, clips: &mut ReadPayloads<Clips>, pixels: &mut ReadPayloads<Pixels>) -> Option<CanvasOp> {
    let kind = OpKind::from_u8(r.u8()?)?;
    let mask = r.u64()?;
    if mask >> CANVAS_OP_BITS != 0 {
        return None;
    }
    let mut o = V1_OP.clone();
    o.kind = kind;
    if has(mask, OP_PATH) {
        o.path = r.geometry(caps)?;
    }
    if has(mask, OP_EVENODD) {
        o.evenodd = true;
    }
    if has(mask, OP_PAINT) {
        o.paint = read_paint(r, caps)?;
    }
    if has(mask, OP_MATRIX) {
        o.matrix = r.f32s()?;
    }
    if has(mask, OP_LINE_WIDTH) {
        o.line_width = r.f32()?;
    }
    if has(mask, OP_LINE_CAP) {
        o.line_cap = r.i32()?;
    }
    if has(mask, OP_LINE_JOIN) {
        o.line_join = r.i32()?;
    }
    if has(mask, OP_MITER_LIMIT) {
        o.miter_limit = r.f32()?;
    }
    if has(mask, OP_DASHES) {
        o.dashes = r.floats(caps.floats)?;
    }
    if has(mask, OP_DASH_OFFSET) {
        o.dash_offset = r.f32()?;
    }
    if has(mask, OP_ALPHA) {
        o.alpha = r.f32()?;
    }
    if has(mask, OP_COMPOSITE) {
        o.composite = Composite::from_u8(r.u8()?)?;
    }
    if has(mask, OP_SHADOW_COLOR) {
        o.shadow_color = r.color()?;
    }
    if has(mask, OP_SHADOW_BLUR) {
        o.shadow_blur = r.f32()?;
    }
    if has(mask, OP_SHADOW_X) {
        o.shadow_x = r.f32()?;
    }
    if has(mask, OP_SHADOW_Y) {
        o.shadow_y = r.f32()?;
    }
    if has(mask, OP_CLIPS) {
        o.clips = Some(clips.get(r, |r| read_clips(r, caps))?);
    }
    if has(mask, OP_FONT) {
        o.font = r.i32()?;
    }
    if has(mask, OP_TEXT) {
        o.text = r.bytes(caps.text)?;
    }
    if has(mask, OP_X) {
        o.x = r.f32()?;
    }
    if has(mask, OP_Y) {
        o.y = r.f32()?;
    }
    if has(mask, OP_SCALE_X) {
        o.scale_x = r.f32()?;
    }
    if has(mask, OP_TEXT_DX) {
        o.text_dx = r.f32()?;
    }
    if has(mask, OP_STROKE_TEXT) {
        o.stroke_text = true;
    }
    if has(mask, OP_IMAGE) {
        o.image = r.i32()?;
    }
    if has(mask, OP_CANVAS) {
        o.canvas = r.i32()?;
    }
    if has(mask, OP_SX) {
        o.sx = r.f32()?;
    }
    if has(mask, OP_SY) {
        o.sy = r.f32()?;
    }
    if has(mask, OP_SW) {
        o.sw = r.f32()?;
    }
    if has(mask, OP_SH) {
        o.sh = r.f32()?;
    }
    if has(mask, OP_DX) {
        o.dx = r.f32()?;
    }
    if has(mask, OP_DY) {
        o.dy = r.f32()?;
    }
    if has(mask, OP_DW) {
        o.dw = r.f32()?;
    }
    if has(mask, OP_DH) {
        o.dh = r.f32()?;
    }
    if has(mask, OP_SMOOTHING) {
        o.smoothing = false;
    }
    if has(mask, OP_RGBA) {
        o.rgba = Some(pixels.get(r, |r| r.bytes(caps.pixels))?);
    }
    if has(mask, OP_W) {
        o.w = r.i32()?;
    }
    if has(mask, OP_H) {
        o.h = r.i32()?;
    }
    Some(o)
}

/// Appends `list` to `out` without the members equal to their version-1 defaults. False, `out` cut back
/// to its size on entry, for what the decoder would refuse under `caps`: a count or a length past its
/// cap or u32::MAX.
pub fn encode_list(list: &[Command], caps: &Caps, out: &mut Vec<u8>) -> bool {
    appended(out, |w| write_list(w, caps, list, None, usize::MAX))
}

/// Writes a list, noting in `starts`, when given, where each command starts and where the list ends, from the list's
/// first byte, until one of them passes `most`: then `starts` is left empty, its room given back.
fn write_list(w: &mut Writer, caps: &Caps, list: &[Command], mut starts: Option<&mut Vec<usize>>, most: usize) -> bool {
    let origin = w.out.len();
    let mut note = |w: &Writer| {
        let Some(s) = starts.as_deref_mut() else { return };
        let at = w.out.len() - origin;
        if at <= most {
            s.push(at);
            return;
        }
        *s = Vec::new();
        starts = None;
    };
    w.head(Form::List);
    if !w.count(list.len(), caps.commands) {
        return false;
    }
    let mut paths = WrittenPayloads::new();
    let mut filters = WrittenPayloads::new();
    for c in list {
        note(w);
        if !write_command(w, caps, &mut paths, &mut filters, c) {
            return false;
        }
    }
    note(w);
    true
}

/// As [`encode_list`], and replaces `starts` with where each command starts and, last, where the list ends, counted
/// from the list's first byte whatever `out` held before, while the list keeps within `most` bytes: for a longer one
/// `starts` is left empty. `starts` untouched on false.
pub fn encode_list_with_starts(list: &[Command], caps: &Caps, out: &mut Vec<u8>, starts: &mut Vec<usize>, most: usize) -> bool {
    let mut at = Vec::new();
    let may_fit = most >= LIST_HEAD_BYTES && list.len() <= (most - LIST_HEAD_BYTES) / COMMAND_BYTES;
    if may_fit && list.len() <= caps.commands {
        at.reserve(list.len() + 1);
    }
    if !appended(out, |w| write_list(w, caps, list, if may_fit { Some(&mut at) } else { None }, most)) {
        return false;
    }
    *starts = at;
    true
}

/// The one list all of `bytes` hold, an absent member at its version-1 default; None for anything else.
/// It checks structure, not what the host does (finite floats, UTF-8, ids, paint cost).
pub fn decode_list(bytes: &[u8], caps: &Caps) -> Option<DisplayList> {
    let mut r = Reader { rest: bytes };
    r.head(Form::List)?;
    let n = r.count(caps.commands, COMMAND_BYTES)?;
    let mut list = Vec::with_capacity(n);
    let mut paths = ReadPayloads::new();
    let mut filters = ReadPayloads::new();
    for _ in 0..n {
        list.push(read_command(&mut r, caps, &mut paths, &mut filters)?);
    }
    r.done().then_some(list)
}

/// Appends `damage` to `out`; false, `out` as on entry, for more rectangles than `caps.rects` or u32::MAX.
pub fn encode_damage(damage: &Damage, caps: &Caps, out: &mut Vec<u8>) -> bool {
    appended(out, |w| {
        w.head(Form::Damage);
        w.flag(damage.is_some());
        let Some(rects) = damage else {
            return true;
        };
        if !w.count(rects.len(), caps.rects) {
            return false;
        }
        for rect in rects {
            w.rect(rect);
        }
        true
    })
}

/// The damage all of `bytes` hold, as for a list.
pub fn decode_damage(bytes: &[u8], caps: &Caps) -> Option<Damage> {
    let mut r = Reader { rest: bytes };
    r.head(Form::Damage)?;
    let damage = if r.flag()? {
        let n = r.count(caps.rects, RECT_BYTES)?;
        let mut list = Vec::with_capacity(n);
        for _ in 0..n {
            list.push(r.rect()?);
        }
        Some(list)
    } else {
        None
    };
    r.done().then_some(damage)
}

/// Appends `ops` to `out`; false, `out` as on entry, for what the decoder would refuse, as for a list.
pub fn encode_batch(ops: &[CanvasOp], caps: &Caps, out: &mut Vec<u8>) -> bool {
    appended(out, |w| {
        w.head(Form::Batch);
        if !w.count(ops.len(), caps.canvas_ops) {
            return false;
        }
        let mut clips = WrittenPayloads::new();
        let mut pixels = WrittenPayloads::new();
        ops.iter().all(|o| write_op(w, caps, &mut clips, &mut pixels, o))
    })
}

/// The canvas ops all of `bytes` hold, as for a list.
pub fn decode_batch(bytes: &[u8], caps: &Caps) -> Option<Batch> {
    let mut r = Reader { rest: bytes };
    r.head(Form::Batch)?;
    let n = r.count(caps.canvas_ops, CANVAS_OP_BYTES)?;
    let mut ops = Vec::with_capacity(n);
    let mut clips = ReadPayloads::new();
    let mut pixels = ReadPayloads::new();
    for _ in 0..n {
        ops.push(read_op(&mut r, caps, &mut clips, &mut pixels)?);
    }
    r.done().then_some(ops)
}

/// Appends `metrics` to `out`: seven floats, nothing to refuse.
pub fn encode_metrics(metrics: &FontMetrics, out: &mut Vec<u8>) {
    let mut w = Writer { out };
    w.head(Form::Metrics);
    w.f32s(&[metrics.ascent, metrics.descent, metrics.height, metrics.x_height, metrics.zero_width, metrics.em_ascent, metrics.em_descent]);
}

/// The metrics all of `bytes` hold, as for a list.
pub fn decode_metrics(bytes: &[u8]) -> Option<FontMetrics> {
    let mut r = Reader { rest: bytes };
    r.head(Form::Metrics)?;
    let [ascent, descent, height, x_height, zero_width, em_ascent, em_descent] = r.f32s()?;
    r.done().then_some(FontMetrics { ascent, descent, height, x_height, zero_width, em_ascent, em_descent })
}
