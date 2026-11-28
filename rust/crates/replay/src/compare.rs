//! `tests/wire_compare.h`, and log_backend.h's `walk` and `same_calls`: two values compared leaf by leaf, floats by
//! their bits, naming the first leaf that differs by its path (`[3].rect.x`); who else holds a shared payload is
//! `sharing()`'s.

use std::collections::BTreeMap;
use std::sync::Arc;

use twidget_wire::{
    CanvasClip, CanvasOp, CanvasPaint, Color, Command, Damage, DisplayList, Filter, FontMetrics, Geometry, Path, Radii, Rect, Stop,
};

use crate::harness::Call;

/// `hex`: `0x` and the lowercase hex digits.
pub fn hex(v: u64) -> String {
    format!("0x{v:x}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AtKind {
    Member,
    Slot,
    Item,
}

/// Where a leaf of a value is: a member, an array slot or a list item under its parent.
#[derive(Clone, Copy, Debug)]
pub struct At<'a> {
    up: Option<&'a At<'a>>,
    name: &'static str,
    index: usize,
    kind: AtKind,
}

impl<'a> At<'a> {
    pub const ROOT: At<'static> = At { up: None, name: "", index: 0, kind: AtKind::Member };

    pub fn named(name: &'static str) -> At<'static> {
        At { up: None, name, index: 0, kind: AtKind::Member }
    }

    fn member(&'a self, name: &'static str) -> At<'a> {
        At { up: Some(self), name, index: 0, kind: AtKind::Member }
    }

    fn slot(&'a self, index: usize) -> At<'a> {
        At { up: Some(self), name: "", index, kind: AtKind::Slot }
    }

    pub fn item(&'a self, index: usize) -> At<'a> {
        At { up: Some(self), name: "", index, kind: AtKind::Item }
    }

    /// `where(at, true)`.
    pub fn path(&self) -> String {
        let up = self.up.map(At::path).unwrap_or_default();
        match self.kind {
            AtKind::Member if up.is_empty() => self.name.to_string(),
            AtKind::Member => format!("{up}.{}", self.name),
            AtKind::Slot | AtKind::Item => format!("{up}[{}]", self.index),
        }
    }
}

/// `Equal`: compares two values leaf by leaf, floats by their bits, and names the first leaf that differs.
#[derive(Debug, Default)]
pub struct Equal {
    pub differences: usize,
    pub first: String,
}

impl Equal {
    fn differ(&mut self, at: &At<'_>, a: String, b: String) {
        if self.differences == 0 {
            self.first = format!("{}: {a} against {b}", at.path());
        }
        self.differences += 1;
    }

    pub fn f32(&mut self, at: &At<'_>, a: f32, b: f32) {
        if a.to_bits() != b.to_bits() {
            self.differ(at, hex(u64::from(a.to_bits())), hex(u64::from(b.to_bits())));
        }
    }

    pub fn integer(&mut self, at: &At<'_>, a: u64, b: u64) {
        if a != b {
            self.differ(at, hex(a), hex(b));
        }
    }

    pub fn raw(&mut self, at: &At<'_>, a: &[u8], b: &[u8]) {
        if a != b {
            self.differ(at, format!("{} bytes", a.len()), format!("{} bytes", b.len()));
        }
    }
}

/// An `int` member as the C++ widens it to `uint64_t`.
fn int(v: i32) -> u64 {
    v as i64 as u64
}

/// An `int` cast to `uint32_t` first, as log_backend.h's walk does.
fn u32_of(v: i32) -> u64 {
    u64::from(v as u32)
}

/// A list's length, then each item against the other's or, past its end, against a default item.
fn list<T: Default>(e: &mut Equal, at: &At<'_>, a: &[T], b: &[T], walk: impl Fn(&mut Equal, &At<'_>, &T, &T)) {
    e.integer(&at.member("size"), a.len() as u64, b.len() as u64);
    let none = T::default();
    for i in 0..a.len().max(b.len()) {
        walk(e, &at.item(i), a.get(i).unwrap_or(&none), b.get(i).unwrap_or(&none));
    }
}

fn floats(e: &mut Equal, at: &At<'_>, a: &[f32], b: &[f32]) {
    list(e, at, a, b, |e, at, x, y| e.f32(at, *x, *y));
}

fn slots(e: &mut Equal, at: &At<'_>, a: &[f32], b: &[f32]) {
    for i in 0..a.len() {
        e.f32(&at.slot(i), a[i], b[i]);
    }
}

/// `Pixels`: its bytes as one leaf.
fn pixels(e: &mut Equal, at: &At<'_>, a: &[u8], b: &[u8]) {
    e.raw(&at.member("bytes"), a, b);
}

/// Whether a payload is there, then what it holds; who else holds it is `sharing()`'s.
fn shared<T: Default>(e: &mut Equal, at: &At<'_>, a: &Option<Arc<T>>, b: &Option<Arc<T>>, walk: impl Fn(&mut Equal, &At<'_>, &T, &T)) {
    e.integer(at, u64::from(a.is_some()), u64::from(b.is_some()));
    let none = T::default();
    walk(e, at, a.as_deref().unwrap_or(&none), b.as_deref().unwrap_or(&none));
}

fn color(e: &mut Equal, at: &At<'_>, a: &Color, b: &Color) {
    e.integer(&at.member("r"), u64::from(a.r), u64::from(b.r));
    e.integer(&at.member("g"), u64::from(a.g), u64::from(b.g));
    e.integer(&at.member("b"), u64::from(a.b), u64::from(b.b));
    e.integer(&at.member("a"), u64::from(a.a), u64::from(b.a));
}

pub fn rect(e: &mut Equal, at: &At<'_>, a: &Rect, b: &Rect) {
    e.f32(&at.member("x"), a.x, b.x);
    e.f32(&at.member("y"), a.y, b.y);
    e.f32(&at.member("w"), a.w, b.w);
    e.f32(&at.member("h"), a.h, b.h);
}

fn radii(e: &mut Equal, at: &At<'_>, a: &Radii, b: &Radii) {
    e.f32(&at.member("tlx"), a.tlx, b.tlx);
    e.f32(&at.member("tly"), a.tly, b.tly);
    e.f32(&at.member("trx"), a.trx, b.trx);
    e.f32(&at.member("try_"), a.try_, b.try_);
    e.f32(&at.member("brx"), a.brx, b.brx);
    e.f32(&at.member("bry"), a.bry, b.bry);
    e.f32(&at.member("blx"), a.blx, b.blx);
    e.f32(&at.member("bly"), a.bly, b.bly);
}

fn stop(e: &mut Equal, at: &At<'_>, a: &Stop, b: &Stop) {
    e.f32(&at.member("offset"), a.offset, b.offset);
    color(e, &at.member("color"), &a.color, &b.color);
}

fn stops(e: &mut Equal, at: &At<'_>, a: &[Stop], b: &[Stop]) {
    list(e, at, a, b, stop);
}

fn filter(e: &mut Equal, at: &At<'_>, a: &Filter, b: &Filter) {
    e.integer(&at.member("kind"), u64::from(a.kind.as_u8()), u64::from(b.kind.as_u8()));
    e.f32(&at.member("amount"), a.amount, b.amount);
    e.f32(&at.member("x"), a.x, b.x);
    e.f32(&at.member("y"), a.y, b.y);
    color(e, &at.member("color"), &a.color, &b.color);
}

fn path_ops(e: &mut Equal, at: &At<'_>, a: &[twidget_wire::PathOp], b: &[twidget_wire::PathOp]) {
    list(e, at, a, b, |e, at, x, y| e.integer(at, u64::from(x.as_u8()), u64::from(y.as_u8())));
}

fn path(e: &mut Equal, at: &At<'_>, a: &Path, b: &Path) {
    path_ops(e, &at.member("ops"), &a.ops, &b.ops);
    floats(e, &at.member("points"), &a.points, &b.points);
    e.integer(&at.member("fill"), u64::from(a.fill), u64::from(b.fill));
    e.integer(&at.member("evenodd"), u64::from(a.evenodd), u64::from(b.evenodd));
    color(e, &at.member("fill_color"), &a.fill_color, &b.fill_color);
    e.integer(&at.member("gradient"), int(a.gradient), int(b.gradient));
    e.f32(&at.member("x1"), a.x1, b.x1);
    e.f32(&at.member("y1"), a.y1, b.y1);
    e.f32(&at.member("x2"), a.x2, b.x2);
    e.f32(&at.member("y2"), a.y2, b.y2);
    e.f32(&at.member("r"), a.r, b.r);
    e.f32(&at.member("fx"), a.fx, b.fx);
    e.f32(&at.member("fy"), a.fy, b.fy);
    stops(e, &at.member("stops"), &a.stops, &b.stops);
    e.integer(&at.member("stroke"), u64::from(a.stroke), u64::from(b.stroke));
    color(e, &at.member("stroke_color"), &a.stroke_color, &b.stroke_color);
    e.f32(&at.member("stroke_width"), a.stroke_width, b.stroke_width);
    e.integer(&at.member("line_cap"), int(a.line_cap), int(b.line_cap));
    e.integer(&at.member("line_join"), int(a.line_join), int(b.line_join));
    e.f32(&at.member("miter_limit"), a.miter_limit, b.miter_limit);
    floats(e, &at.member("dashes"), &a.dashes, &b.dashes);
    e.f32(&at.member("dash_offset"), a.dash_offset, b.dash_offset);
    e.integer(&at.member("stroke_gradient"), int(a.stroke_gradient), int(b.stroke_gradient));
    slots(e, &at.member("sg"), &a.sg, &b.sg);
    stops(e, &at.member("stroke_stops"), &a.stroke_stops, &b.stroke_stops);
}

pub fn command(e: &mut Equal, at: &At<'_>, a: &Command, b: &Command) {
    e.integer(&at.member("kind"), u64::from(a.kind.as_u8()), u64::from(b.kind.as_u8()));
    rect(e, &at.member("rect"), &a.rect, &b.rect);
    radii(e, &at.member("radii"), &a.radii, &b.radii);
    rect(e, &at.member("clip"), &a.clip, &b.clip);
    e.integer(&at.member("has_clip"), u64::from(a.has_clip), u64::from(b.has_clip));
    color(e, &at.member("color"), &a.color, &b.color);
    e.f32(&at.member("width"), a.width, b.width);
    e.integer(&at.member("font"), int(a.font), int(b.font));
    e.raw(&at.member("text"), &a.text, &b.text);
    e.f32(&at.member("x"), a.x, b.x);
    e.f32(&at.member("y"), a.y, b.y);
    e.integer(&at.member("image"), int(a.image), int(b.image));
    e.integer(&at.member("canvas"), int(a.canvas), int(b.canvas));
    e.f32(&at.member("ax"), a.ax, b.ax);
    e.f32(&at.member("ay"), a.ay, b.ay);
    e.f32(&at.member("bx"), a.bx, b.bx);
    e.f32(&at.member("by"), a.by, b.by);
    stops(e, &at.member("stops"), &a.stops, &b.stops);
    shared(e, &at.member("path"), &a.path, &b.path, path);
    shared(e, &at.member("filter"), &a.filter, &b.filter, |e, at, x: &Vec<Filter>, y| list(e, at, x, y, filter));
    e.f32(&at.member("opacity"), a.opacity, b.opacity);
    slots(e, &at.member("matrix"), &a.matrix, &b.matrix);
    e.integer(&at.member("hidden"), u64::from(a.hidden), u64::from(b.hidden));
    e.integer(&at.member("inset"), u64::from(a.inset), u64::from(b.inset));
    e.integer(&at.member("key"), a.key, b.key);
}

fn geometry(e: &mut Equal, at: &At<'_>, a: &Geometry, b: &Geometry) {
    path_ops(e, &at.member("ops"), &a.ops, &b.ops);
    floats(e, &at.member("points"), &a.points, &b.points);
}

fn paint(e: &mut Equal, at: &At<'_>, a: &CanvasPaint, b: &CanvasPaint) {
    e.integer(&at.member("kind"), u64::from(a.kind.as_u8()), u64::from(b.kind.as_u8()));
    color(e, &at.member("color"), &a.color, &b.color);
    slots(e, &at.member("args"), &a.args, &b.args);
    stops(e, &at.member("stops"), &a.stops, &b.stops);
}

fn clip(e: &mut Equal, at: &At<'_>, a: &CanvasClip, b: &CanvasClip) {
    geometry(e, &at.member("path"), &a.path, &b.path);
    e.integer(&at.member("evenodd"), u64::from(a.evenodd), u64::from(b.evenodd));
}

fn op(e: &mut Equal, at: &At<'_>, a: &CanvasOp, b: &CanvasOp) {
    e.integer(&at.member("kind"), u64::from(a.kind.as_u8()), u64::from(b.kind.as_u8()));
    geometry(e, &at.member("path"), &a.path, &b.path);
    e.integer(&at.member("evenodd"), u64::from(a.evenodd), u64::from(b.evenodd));
    paint(e, &at.member("paint"), &a.paint, &b.paint);
    slots(e, &at.member("matrix"), &a.matrix, &b.matrix);
    e.f32(&at.member("line_width"), a.line_width, b.line_width);
    e.integer(&at.member("line_cap"), int(a.line_cap), int(b.line_cap));
    e.integer(&at.member("line_join"), int(a.line_join), int(b.line_join));
    e.f32(&at.member("miter_limit"), a.miter_limit, b.miter_limit);
    floats(e, &at.member("dashes"), &a.dashes, &b.dashes);
    e.f32(&at.member("dash_offset"), a.dash_offset, b.dash_offset);
    e.f32(&at.member("alpha"), a.alpha, b.alpha);
    e.integer(&at.member("composite"), u64::from(a.composite.as_u8()), u64::from(b.composite.as_u8()));
    color(e, &at.member("shadow_color"), &a.shadow_color, &b.shadow_color);
    e.f32(&at.member("shadow_blur"), a.shadow_blur, b.shadow_blur);
    e.f32(&at.member("shadow_x"), a.shadow_x, b.shadow_x);
    e.f32(&at.member("shadow_y"), a.shadow_y, b.shadow_y);
    shared(e, &at.member("clips"), &a.clips, &b.clips, |e, at, x: &Vec<CanvasClip>, y| list(e, at, x, y, clip));
    e.integer(&at.member("font"), int(a.font), int(b.font));
    e.raw(&at.member("text"), &a.text, &b.text);
    e.f32(&at.member("x"), a.x, b.x);
    e.f32(&at.member("y"), a.y, b.y);
    e.f32(&at.member("scale_x"), a.scale_x, b.scale_x);
    e.f32(&at.member("text_dx"), a.text_dx, b.text_dx);
    e.integer(&at.member("stroke_text"), u64::from(a.stroke_text), u64::from(b.stroke_text));
    e.integer(&at.member("image"), int(a.image), int(b.image));
    e.integer(&at.member("canvas"), int(a.canvas), int(b.canvas));
    e.f32(&at.member("sx"), a.sx, b.sx);
    e.f32(&at.member("sy"), a.sy, b.sy);
    e.f32(&at.member("sw"), a.sw, b.sw);
    e.f32(&at.member("sh"), a.sh, b.sh);
    e.f32(&at.member("dx"), a.dx, b.dx);
    e.f32(&at.member("dy"), a.dy, b.dy);
    e.f32(&at.member("dw"), a.dw, b.dw);
    e.f32(&at.member("dh"), a.dh, b.dh);
    e.integer(&at.member("smoothing"), u64::from(a.smoothing), u64::from(b.smoothing));
    shared(e, &at.member("rgba"), &a.rgba, &b.rgba, |e, at, x: &Vec<u8>, y| pixels(e, at, x, y));
    e.integer(&at.member("w"), int(a.w), int(b.w));
    e.integer(&at.member("h"), int(a.h), int(b.h));
}

pub fn metrics(e: &mut Equal, at: &At<'_>, a: &FontMetrics, b: &FontMetrics) {
    e.f32(&at.member("ascent"), a.ascent, b.ascent);
    e.f32(&at.member("descent"), a.descent, b.descent);
    e.f32(&at.member("height"), a.height, b.height);
    e.f32(&at.member("x_height"), a.x_height, b.x_height);
    e.f32(&at.member("zero_width"), a.zero_width, b.zero_width);
    e.f32(&at.member("em_ascent"), a.em_ascent, b.em_ascent);
    e.f32(&at.member("em_descent"), a.em_descent, b.em_descent);
}

fn damage(e: &mut Equal, at: &At<'_>, a: &Damage, b: &Damage) {
    e.integer(&at.member("valid"), u64::from(a.is_some()), u64::from(b.is_some()));
    let none: Vec<Rect> = Vec::new();
    list(e, &at.member("rects"), a.as_deref().unwrap_or(&none), b.as_deref().unwrap_or(&none), rect);
}

/// `same(list)`: whether `b` is `a` leaf by leaf; the first leaf that differs otherwise.
pub fn same_list(a: &DisplayList, b: &DisplayList) -> Result<(), String> {
    let mut e = Equal::default();
    list(&mut e, &At::ROOT, a, b, command);
    if e.differences == 0 { Ok(()) } else { Err(e.first) }
}

/// `same(damage)`.
pub fn same_damage(a: &Damage, b: &Damage) -> Result<(), String> {
    let mut e = Equal::default();
    damage(&mut e, &At::ROOT, a, b);
    if e.differences == 0 { Ok(()) } else { Err(e.first) }
}

/// Each user of a payload as the index of the payload's first user, -1 without one: two values share alike when these
/// match.
fn users<T, P>(values: &[T], get: impl Fn(&T) -> Option<&Arc<P>>) -> Vec<i64> {
    let mut first: BTreeMap<usize, i64> = BTreeMap::new();
    values
        .iter()
        .enumerate()
        .map(|(i, v)| match get(v) {
            Some(p) => *first.entry(Arc::as_ptr(p) as *const () as usize).or_insert(i as i64),
            None => -1,
        })
        .collect()
}

pub type Sharing = Vec<Vec<i64>>;

/// `sharing(list)`: the users of each path and each filter list.
pub fn list_sharing(list: &DisplayList) -> Sharing {
    vec![users(list, |c: &Command| c.path.as_ref()), users(list, |c: &Command| c.filter.as_ref())]
}

/// `sharing(batch)`: the users of each clip list and each pixel payload.
pub fn batch_sharing(ops: &[CanvasOp]) -> Sharing {
    vec![users(ops, |o: &CanvasOp| o.clips.as_ref()), users(ops, |o: &CanvasOp| o.rgba.as_ref())]
}

/// log_backend.h's `walk` of a Call.
fn call(e: &mut Equal, at: &At<'_>, a: &Call, b: &Call) {
    e.integer(&at.member("op"), a.op as u64, b.op as u64);
    e.raw(&at.member("families"), &a.families, &b.families);
    e.raw(&at.member("features"), &a.features, &b.features);
    e.raw(&at.member("text"), &a.text, &b.text);
    e.raw(&at.member("url"), &a.url, &b.url);
    e.f32(&at.member("size"), a.size, b.size);
    e.integer(&at.member("weight"), u32_of(a.weight), u32_of(b.weight));
    e.integer(&at.member("italic"), u64::from(a.italic), u64::from(b.italic));
    e.integer(&at.member("font"), u32_of(a.font), u32_of(b.font));
    metrics(e, &at.member("metrics_in"), &a.metrics_in, &b.metrics_in);
    metrics(e, &at.member("metrics"), &a.metrics, &b.metrics);
    e.f32(&at.member("width"), a.width, b.width);
    e.integer(&at.member("id"), u32_of(a.id), u32_of(b.id));
    e.integer(&at.member("x"), u32_of(a.x), u32_of(b.x));
    e.integer(&at.member("y"), u32_of(a.y), u32_of(b.y));
    e.integer(&at.member("w"), u32_of(a.w), u32_of(b.w));
    e.integer(&at.member("h"), u32_of(a.h), u32_of(b.h));
    e.f32(&at.member("w_in"), a.w_in, b.w_in);
    e.f32(&at.member("h_in"), a.h_in, b.h_in);
    e.f32(&at.member("image_w"), a.image_w, b.image_w);
    e.f32(&at.member("image_h"), a.image_h, b.image_h);
    e.integer(&at.member("ok"), u64::from(a.ok), u64::from(b.ok));
    e.integer(&at.member("painted"), u64::from(a.painted), u64::from(b.painted));
    e.integer(&at.member("rgba_in"), a.rgba_in as u64, b.rgba_in as u64);
    pixels(e, &at.member("rgba"), &a.rgba, &b.rgba);
    list(e, &at.member("ops"), &a.ops, &b.ops, op);
}

/// log_backend.h's `same_calls`: whether two logs hold the same calls in the same order member by member, each batch's
/// payloads shared alike within it; the first difference otherwise.
pub fn same_calls(a: &[Call], b: &[Call]) -> Result<(), String> {
    let log = At::named("call");
    for i in 0..a.len().max(b.len()) {
        if i == a.len() || i == b.len() {
            return Err(format!("call {i} is in one log only, of {} and {}", a.len(), b.len()));
        }
        let mut e = Equal::default();
        call(&mut e, &log.item(i), &a[i], &b[i]);
        if e.differences != 0 {
            return Err(e.first);
        }
        if batch_sharing(&a[i].ops) != batch_sharing(&b[i].ops) {
            return Err(format!("call {i} shares its batch's payloads otherwise"));
        }
    }
    Ok(())
}
