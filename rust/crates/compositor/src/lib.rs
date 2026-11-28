//! What a platform compositor runs on its own (`core/src/compositor.h`, `compositor.cpp`): elements whose
//! animations touch only transform and opacity, lifted out of the document's paint into layers, each with a
//! track of keyframes the core has already resolved. Easing, steps(), direction, iteration and Web Animations
//! timing are all baked into linear keys, so any compositor with linear keyframe animation can play them. And
//! what the renderer keeps of a paint between frames: its digest, its summary for damage, and the clips that
//! change no pixel. Floats and doubles are where the C++ has them.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use twidget_wire::{Affine, Cmd, Color, Command, DisplayList, Radii, Rect, Stop};

const GEOMETRY_TOLERANCE: f32 = 0.05;
const OPACITY_TOLERANCE: f32 = 0.002;
const INITIAL_SPANS: i32 = 8;
// A discontinuity (steps(), a flip at 0.5) never meets the tolerance: it stops splitting here and is played
// as a ramp this short.
const MAX_DEPTH: i32 = 9;
const MAX_KEYS: usize = 1024;
// More changed areas than this are merged into the one around them all.
const MAX_DAMAGE_RECTS: usize = 32;

const FNV_OFFSET: u64 = 1469598103934665603;
const FNV_PRIME: u64 = 1099511628211;

/// One key of a track: the layer's full transform and opacity at `t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerKey {
    /// 0..1 across the track's duration, ascending; values between keys are linear in every matrix
    /// component and in opacity.
    pub t: f32,
    /// Maps the layer's content, which is in document coordinates, to where it shows, in document
    /// coordinates: the CSS transform already anchored at the transform origin.
    pub matrix: Affine,
    pub opacity: f32,
}

impl Default for LayerKey {
    fn default() -> LayerKey {
        LayerKey { t: 0.0, matrix: Affine::IDENTITY, opacity: 1.0 }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerTrack {
    /// When the first key applies, on the renderer clock, in seconds. Usually a moment ago: a platform
    /// starts the animation that far in.
    pub start: f64,
    pub duration: f64,
    /// Loops forever; otherwise the last key holds until the core changes the composition.
    pub repeat: bool,
    pub keys: Vec<LayerKey>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    /// The widget node id of the element.
    pub id: i64,
    /// The area `content` covers, in document coordinates. Draw the content with this rectangle's top left
    /// at the layer's origin.
    pub bounds: Rect,
    /// Changes whenever `content` paints differently; the same value means the same pixels.
    pub content_version: u64,
    /// The element and its descendants without the element's own transform and opacity, in document
    /// coordinates.
    pub content: DisplayList,
    /// Rasterise the content this much finer than the document (the largest scale the track reaches, at
    /// most 3), so a scaled-up layer stays sharp.
    pub raster_scale: f32,
    /// A clipping ancestor (overflow), in document coordinates. It clips the transformed layer and does not
    /// move with it.
    pub has_clip: bool,
    pub clip: Rect,
    pub clip_radii: Radii,
    /// Changes whenever `track` changes; the same value means keep the running animation.
    pub track_version: u64,
    pub track: LayerTrack,
    /// The layer this one moves with (an animated ancestor's), or 0 for one over the document. A child comes
    /// after its parent, with nothing but the parent's other descendants in between; its keys and its clip
    /// are in the parent's content space, so it shows at the parent's matrix times its own, inside the
    /// parent's clip and opacity.
    pub parent: i64,
}

impl Default for Layer {
    fn default() -> Layer {
        Layer {
            id: 0,
            bounds: Rect::default(),
            content_version: 0,
            content: DisplayList::new(),
            raster_scale: 1.0,
            has_clip: false,
            clip: Rect::default(),
            clip_radii: Radii::default(),
            track_version: 0,
            track: LayerTrack::default(),
            parent: 0,
        }
    }
}

/// Layers in paint order, bottom first, all above the document's own paint.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Composition {
    pub layers: Vec<Layer>,
    /// Changes whenever anything above changes.
    pub version: u64,
}

/// One drawing command as damage tracking keeps it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Entry {
    /// What it draws, with the clips and groups around it.
    pub key: u64,
    /// Where its pixels can land, in document coordinates.
    pub ink: Rect,
    pub canvas: i32,
    /// It paints from what is under it: damage there repaints all of it.
    pub backdrop: bool,
}

/// A paint as damage tracking needs to keep it: an entry per drawing command.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaintSummary {
    pub entries: Vec<Entry>,
}

/// FNV-1a over the bytes the C++ hashes: each value as its object lies in memory on the 64-bit,
/// little-endian targets the helper runs on (an enum class as its `int`, a `bool` as one byte, a size as
/// eight), none of the structs it hashes whole having padding.
struct Digest {
    h: u64,
}

impl Digest {
    fn new() -> Digest {
        Digest { h: FNV_OFFSET }
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.h = (self.h ^ u64::from(b)).wrapping_mul(FNV_PRIME);
        }
    }

    fn byte(&mut self, v: u8) {
        self.h = (self.h ^ u64::from(v)).wrapping_mul(FNV_PRIME);
    }

    fn flag(&mut self, v: bool) {
        self.byte(u8::from(v));
    }

    fn i32(&mut self, v: i32) {
        self.bytes(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }

    fn size(&mut self, n: usize) {
        self.u64(n as u64);
    }

    fn f32(&mut self, v: f32) {
        self.bytes(&v.to_bits().to_le_bytes());
    }

    fn f32s(&mut self, v: &[f32]) {
        for &x in v {
            self.f32(x);
        }
    }

    fn color(&mut self, c: Color) {
        self.bytes(&[c.r, c.g, c.b, c.a]);
    }

    fn rect(&mut self, r: &Rect) {
        self.f32s(&[r.x, r.y, r.w, r.h]);
    }

    fn text(&mut self, s: &[u8]) {
        self.size(s.len());
        self.bytes(s);
    }

    fn stops(&mut self, list: &[Stop]) {
        self.size(list.len());
        for s in list {
            self.f32(s.offset);
            self.color(s.color);
        }
    }

    /// Everything a command paints with, in the C++'s order; its `key` is not.
    fn command(&mut self, c: &Command) {
        self.i32(i32::from(c.kind.as_u8()));
        self.rect(&c.rect);
        let q = &c.radii;
        self.f32s(&[q.tlx, q.tly, q.trx, q.try_, q.brx, q.bry, q.blx, q.bly]);
        self.rect(&c.clip);
        self.flag(c.has_clip);
        self.color(c.color);
        self.f32(c.width);
        self.i32(c.font);
        self.text(&c.text);
        self.f32(c.x);
        self.f32(c.y);
        self.i32(c.image);
        self.i32(c.canvas);
        self.f32s(&[c.ax, c.ay, c.bx, c.by]);
        self.stops(&c.stops);
        self.f32(c.opacity);
        self.f32s(&c.matrix);
        self.flag(c.hidden);
        self.flag(c.inset);
        if let Some(filters) = &c.filter {
            for step in filters.iter() {
                self.byte(step.kind.as_u8());
                self.f32s(&[step.amount, step.x, step.y]);
                self.color(step.color);
            }
        }
        if let Some(p) = &c.path {
            for op in &p.ops {
                self.byte(op.as_u8());
            }
            self.f32s(&p.points);
            self.flag(p.fill);
            self.flag(p.evenodd);
            self.color(p.fill_color);
            self.i32(p.gradient);
            self.f32s(&[p.x1, p.y1, p.x2, p.y2, p.r, p.fx, p.fy]);
            self.stops(&p.stops);
            self.flag(p.stroke);
            self.color(p.stroke_color);
            self.f32(p.stroke_width);
            self.i32(p.line_cap);
            self.i32(p.line_join);
            self.f32(p.miter_limit);
            self.f32s(&p.dashes);
            self.f32(p.dash_offset);
            self.i32(p.stroke_gradient);
            self.f32s(&p.sg);
            self.stops(&p.stroke_stops);
        }
    }
}

/// `std::min(a, b)`: `a` unless `b` is less.
fn min(a: f32, b: f32) -> f32 {
    if b < a { b } else { a }
}

/// `std::max(a, b)`: `a` unless it is less than `b`.
fn max(a: f32, b: f32) -> f32 {
    if a < b { b } else { a }
}

/// `std::max` of an initializer list: the first of the largest.
fn max_of(values: &[f32]) -> f32 {
    let mut m = values[0];
    for &v in &values[1..] {
        if m < v {
            m = v;
        }
    }
    m
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

/// The keys of `track` as they continue `elapsed` seconds after its start, for a compositor that cannot
/// start an animation part way through: a loop turned to its current phase, a one-shot cut to what is left.
/// With them, the span the returned keys cover; one key means hold it.
pub fn resume_track(track: &LayerTrack, elapsed: f64) -> (Vec<LayerKey>, f64) {
    let keys = &track.keys;
    let duration = track.duration;
    if keys.len() < 2 || elapsed <= 0.0 || track.duration <= 0.0 {
        return (keys.clone(), duration);
    }
    let p = if track.repeat { (elapsed % track.duration) / track.duration } else { elapsed / track.duration };
    if p >= 1.0 {
        let mut last = keys[keys.len() - 1];
        last.t = 0.0;
        return (vec![last], duration);
    }
    let at = |t: f64| {
        let mut i = 0;
        while i + 2 < keys.len() && f64::from(keys[i + 1].t) < t {
            i += 1;
        }
        let (a, b) = (&keys[i], &keys[i + 1]);
        let u = if b.t > a.t { clamp((t - f64::from(a.t)) / f64::from(b.t - a.t), 0.0, 1.0) as f32 } else { 0.0 };
        let lerp = |x: f32, y: f32| x + (y - x) * u;
        let (m, n) = (&a.matrix, &b.matrix);
        LayerKey {
            t: 0.0,
            matrix: Affine {
                a: lerp(m.a, n.a),
                b: lerp(m.b, n.b),
                c: lerp(m.c, n.c),
                d: lerp(m.d, n.d),
                e: lerp(m.e, n.e),
                f: lerp(m.f, n.f),
            },
            opacity: lerp(a.opacity, b.opacity),
        }
    };
    let now = at(p);
    let mut out = vec![now];
    if !track.repeat {
        for k in keys {
            if f64::from(k.t) <= p {
                continue;
            }
            out.push(LayerKey { t: ((f64::from(k.t) - p) / (1.0 - p)) as f32, ..*k });
        }
        return (out, track.duration * (1.0 - p));
    }
    // A loop's last key is its first again: the keys after the phase, then those before it.
    for k in keys {
        if f64::from(k.t) <= p {
            continue;
        }
        out.push(LayerKey { t: (f64::from(k.t) - p) as f32, ..*k });
    }
    for k in keys {
        if k.t <= 0.0 || f64::from(k.t) >= p {
            continue;
        }
        out.push(LayerKey { t: (f64::from(k.t + 1.0) - p) as f32, ..*k });
    }
    out.push(LayerKey { t: 1.0, ..now });
    (out, duration)
}

fn grow(r: Rect, by: f32) -> Rect {
    Rect::new(r.x - by, r.y - by, r.w + 2.0 * by, r.h + 2.0 * by)
}

/// Where a drawing command can put pixels, in its own space; empty for the commands that only set up clips
/// and groups.
fn ink_of(c: &Command) -> Rect {
    match c.kind {
        Cmd::FillRect | Cmd::FillEllipse | Cmd::Image | Cmd::Linear | Cmd::Radial | Cmd::Conic => grow(c.rect, 1.0),
        Cmd::StrokeRect | Cmd::StrokeEllipse => grow(c.rect, c.width / 2.0 + 1.0),
        Cmd::Text => {
            // Glyphs overhang their advance (italics, swashes): a quarter of the line on each side.
            let size = max(c.rect.h, 1.0);
            grow(c.rect, size / 4.0 + 1.0)
        }
        Cmd::Shadow => grow(c.rect, c.bx * 1.5 + c.by.abs() + max(c.ax.abs(), c.ay.abs()) + 2.0),
        Cmd::FilterPush | Cmd::Backdrop => grow(c.rect, 1.0),
        Cmd::Path => {
            let Some(p) = c.path.as_deref().filter(|p| p.points.len() >= 2) else { return Rect::default() };
            let (mut x0, mut y0) = (p.points[0], p.points[1]);
            let (mut x1, mut y1) = (x0, y0);
            for xy in p.points.chunks_exact(2) {
                x0 = min(x0, xy[0]);
                x1 = max(x1, xy[0]);
                y0 = min(y0, xy[1]);
                y1 = max(y1, xy[1]);
            }
            let reach = if p.stroke { p.stroke_width * max(p.miter_limit, 1.0) / 2.0 } else { 0.0 };
            grow(Rect::new(x0, y0, x1 - x0, y1 - y0), reach + 1.0)
        }
        _ => Rect::default(),
    }
}

fn transformed(r: Rect, m: &Affine) -> Rect {
    if *m == Affine::IDENTITY {
        return r;
    }
    let xs = [r.x, r.x + r.w, r.x, r.x + r.w];
    let ys = [r.y, r.y, r.y + r.h, r.y + r.h];
    let (mut x0, mut y0, mut x1, mut y1) = (1e30f32, 1e30f32, -1e30f32, -1e30f32);
    for i in 0..4 {
        let x = m.apply_x(xs[i], ys[i]);
        let y = m.apply_y(xs[i], ys[i]);
        x0 = min(x0, x);
        y0 = min(y0, y);
        x1 = max(x1, x);
        y1 = max(y1, y);
    }
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

fn own_matrix(c: &Command) -> Affine {
    let m = &c.matrix;
    Affine { a: m[0], b: m[1], c: m[2], d: m[3], e: m[4], f: m[5] }
}

/// A digest of everything `list` paints: equal digests paint equal pixels.
pub fn paint_digest(list: &[Command]) -> u64 {
    let mut d = Digest::new();
    for c in list {
        d.command(c);
    }
    d.h
}

/// Drops the rectangular clips whose content stays inside them (inset by their corner radii): they change
/// no pixel, and a rounded clip costs a painter a layer. Clip paths and clips under a rotation or skew stay.
/// Returns how many clips went.
pub fn drop_inert_clips(list: &mut DisplayList) -> usize {
    // Per open clip: where it starts, its area (inset by its corners) in document coordinates, and whether
    // everything drawn inside has stayed within it so far.
    struct Open {
        at: usize,
        area: Rect,
        keep: bool,
    }
    struct Frame {
        matrix: Affine,
        // A ClipPush, else a GroupPush.
        clip: bool,
    }
    let mut frames = vec![Frame { matrix: Affine::IDENTITY, clip: false }];
    let mut open: Vec<Open> = Vec::new();
    let mut drop = vec![false; list.len()];
    let axis_aligned = |m: &Affine| m.b == 0.0 && m.c == 0.0 && m.a > 0.0 && m.d > 0.0;
    let inside = |r: &Rect, area: &Rect| r.x >= area.x && r.y >= area.y && r.x + r.w <= area.x + area.w && r.y + r.h <= area.y + area.h;
    for (i, c) in list.iter().enumerate() {
        let m = frames[frames.len() - 1].matrix;
        match c.kind {
            Cmd::ClipPush => {
                let mut o = Open { at: i, area: Rect::default(), keep: false };
                // A clip path, or one under a rotation or skew, stays.
                if c.path.is_none() && axis_aligned(&m) {
                    let q = &c.radii;
                    let corner = max_of(&[q.tlx, q.tly, q.trx, q.try_, q.brx, q.bry, q.blx, q.bly]);
                    let area = Rect::new(c.rect.x + corner, c.rect.y + corner, c.rect.w - 2.0 * corner, c.rect.h - 2.0 * corner);
                    o.area = transformed(area, &m);
                    o.keep = area.w > 0.0 && area.h > 0.0;
                }
                open.push(o);
                frames.push(Frame { matrix: m, clip: true });
                continue;
            }
            Cmd::ClipPop => {
                if frames.len() > 1 && frames[frames.len() - 1].clip {
                    frames.pop();
                    if let Some(o) = open.pop()
                        && o.keep
                    {
                        drop[o.at] = true;
                        drop[i] = true;
                    }
                }
                continue;
            }
            Cmd::GroupPush => {
                frames.push(Frame { matrix: m * own_matrix(c), clip: false });
                continue;
            }
            Cmd::GroupPop => {
                if frames.len() > 1 && !frames[frames.len() - 1].clip {
                    frames.pop();
                }
                continue;
            }
            _ => {}
        }
        if open.is_empty() {
            continue;
        }
        let ink = transformed(ink_of(c), &m);
        if ink.w <= 0.0 && ink.h <= 0.0 {
            continue;
        }
        // Within the innermost open clip is within every enclosing one that it is within.
        for o in &mut open {
            if o.keep && !inside(&ink, &o.area) {
                o.keep = false;
            }
        }
    }
    // One pass: a removal per dropped command makes many sibling clips quadratic.
    let before = list.len();
    let mut at = 0;
    list.retain(|_| {
        at += 1;
        !drop[at - 1]
    });
    (before - list.len()) / 2
}

/// What damage tracking keeps of `list`: for each drawing command, a key for what it draws (with the clips
/// and groups around it) and where its pixels can land, in document coordinates.
pub fn summarize(list: &[Command]) -> PaintSummary {
    let mut out = PaintSummary { entries: Vec::with_capacity(list.len()) };
    // What a command draws depends on the clips and groups around it: its key carries them.
    #[derive(Clone, Copy)]
    struct Frame {
        context: u64,
        matrix: Affine,
        hidden: bool,
        // Inside a filter: where its result paints, which a change inside may all change.
        filtered: Rect,
    }
    let mut stack = vec![Frame { context: FNV_OFFSET, matrix: Affine::IDENTITY, hidden: false, filtered: Rect::default() }];
    for c in list {
        let top = stack[stack.len() - 1];
        let mut d = Digest::new();
        d.u64(top.context);
        d.command(c);
        match c.kind {
            Cmd::ClipPush => {
                stack.push(Frame { context: d.h, ..top });
                continue;
            }
            Cmd::GroupPush => {
                let hidden = top.hidden || c.hidden || c.opacity <= 0.0;
                stack.push(Frame { context: d.h, matrix: top.matrix * own_matrix(c), hidden, filtered: top.filtered });
                continue;
            }
            Cmd::FilterPush => {
                stack.push(Frame { context: d.h, filtered: transformed(ink_of(c), &top.matrix), ..top });
                continue;
            }
            Cmd::ClipPop | Cmd::GroupPop | Cmd::FilterPop => {
                if stack.len() > 1 {
                    stack.pop();
                }
                continue;
            }
            _ => {}
        }
        let mut e = Entry { key: d.h, ink: Rect::default(), canvas: c.canvas, backdrop: c.kind == Cmd::Backdrop };
        if !top.hidden {
            e.ink = if top.filtered.empty() { transformed(ink_of(c), &top.matrix) } else { top.filtered };
        }
        out.entries.push(e);
    }
    out
}

fn add(out: &mut Vec<Rect>, r: Rect) {
    if r.w > 0.0 && r.h > 0.0 {
        out.push(r);
    }
}

/// The areas where `after` paints differently from `before`, into `out`: commands one has and the other not,
/// commands whose paint order changed, and canvases `canvas_changed` says drew since. Merged into one area
/// when there are many.
pub fn damage_between(
    before: &PaintSummary,
    after: &PaintSummary,
    mut canvas_changed: Option<&mut dyn FnMut(i32) -> bool>,
    out: &mut Vec<Rect>,
) -> bool {
    out.clear();
    let a = &before.entries;
    let b = &after.entries;
    // What both start and end with paints the same, unless a canvas behind it drew again.
    let mut head = 0;
    while head < a.len() && head < b.len() && a[head].key == b[head].key {
        head += 1;
    }
    let mut tail = 0;
    while tail < a.len() - head && tail < b.len() - head && a[a.len() - 1 - tail].key == b[b.len() - 1 - tail].key {
        tail += 1;
    }
    // The middle: commands matched by key, in order where they can be; the rest painted anew.
    let mut waiting: HashMap<u64, Vec<usize>> = HashMap::new();
    for i in (head..a.len() - tail).rev() {
        waiting.entry(a[i].key).or_default().push(i);
    }
    let mut matched = vec![false; a.len()];
    // (after, before)
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for j in head..b.len() - tail {
        match waiting.get_mut(&b[j].key).and_then(|w| w.pop()) {
            None => add(out, b[j].ink),
            Some(i) => {
                matched[i] = true;
                pairs.push((j, i));
            }
        }
    }
    for i in head..a.len() - tail {
        if !matched[i] {
            add(out, a[i].ink);
        }
    }
    // Matched commands whose paint order changed: those off the longest run kept in order. run[k] is the
    // smallest `before` index ending a run of k + 1, at[k] its pair.
    let mut run: Vec<usize> = Vec::new();
    let mut at: Vec<usize> = Vec::new();
    let mut back: Vec<i64> = vec![-1; pairs.len()];
    for (p, &(_, i)) in pairs.iter().enumerate() {
        let k = run.partition_point(|&x| x < i);
        if k == run.len() {
            run.push(i);
            at.push(p);
        } else {
            run[k] = i;
            at[k] = p;
        }
        back[p] = if k > 0 { at[k - 1] as i64 } else { -1 };
    }
    let mut kept = vec![false; pairs.len()];
    let mut p = at.last().map_or(-1, |&p| p as i64);
    while p >= 0 {
        kept[p as usize] = true;
        p = back[p as usize];
    }
    for (p, &(j, _)) in pairs.iter().enumerate() {
        if !kept[p] {
            add(out, b[j].ink);
        }
    }
    for e in b {
        if e.canvas != 0
            && let Some(changed) = canvas_changed.as_mut()
            && changed(e.canvas)
        {
            add(out, e.ink);
        }
    }
    // A backdrop filters what is under it: a change there changes all of it.
    let touches = |a: &Rect, r: &Rect| a.x < r.x + r.w && r.x < a.x + a.w && a.y < r.y + r.h && r.y < a.y + a.h;
    for e in b {
        if !e.backdrop {
            continue;
        }
        let n = out.len();
        for k in 0..n {
            if touches(&e.ink, &out[k]) {
                add(out, e.ink);
                break;
            }
        }
    }
    if out.len() > MAX_DAMAGE_RECTS {
        let mut u = out[0];
        for r in out.iter() {
            let x0 = min(u.x, r.x);
            let y0 = min(u.y, r.y);
            u = Rect::new(x0, y0, max(u.x + u.w, r.x + r.w) - x0, max(u.y + u.h, r.y + r.h) - y0);
        }
        out.clear();
        out.push(u);
    }
    true
}

struct Baker<'a, F> {
    sample: F,
    corners: [[f32; 2]; 4],
    keys: &'a mut Vec<LayerKey>,
    failed: bool,
}

impl<F: FnMut(f64, &mut LayerKey) -> bool> Baker<'_, F> {
    fn straight(&self, a: &LayerKey, b: &LayerKey, mid: &LayerKey) -> bool {
        let mut worst = 0.0f32;
        for p in &self.corners {
            let (ax, ay) = (a.matrix.apply_x(p[0], p[1]), a.matrix.apply_y(p[0], p[1]));
            let (bx, by) = (b.matrix.apply_x(p[0], p[1]), b.matrix.apply_y(p[0], p[1]));
            let (mx, my) = (mid.matrix.apply_x(p[0], p[1]), mid.matrix.apply_y(p[0], p[1]));
            worst = max(worst, (mx - (ax + bx) / 2.0).hypot(my - (ay + by) / 2.0));
        }
        worst <= GEOMETRY_TOLERANCE && (mid.opacity - (a.opacity + b.opacity) / 2.0).abs() <= OPACITY_TOLERANCE
    }

    fn split(&mut self, t0: f64, k0: &LayerKey, t1: f64, k1: &LayerKey, depth: i32) {
        if self.failed {
            return;
        }
        let tm = (t0 + t1) / 2.0;
        let mut km = LayerKey::default();
        if !(self.sample)(tm, &mut km) {
            self.failed = true;
            return;
        }
        km.t = tm as f32;
        if depth >= MAX_DEPTH || self.straight(k0, k1, &km) {
            self.keys.push(*k1);
            self.failed = self.keys.len() > MAX_KEYS;
            return;
        }
        self.split(t0, k0, tm, &km, depth + 1);
        self.split(tm, &km, t1, k1, depth + 1);
    }
}

/// Bakes `sample` (the layer's key at 0..1 of the track) into linear keys: halves each span until the box's
/// corners stray at most 0.05 px and the opacity 0.002 from the straight line. False when a sample fails or
/// the track needs more keys than a compositor should get; `keys` then holds what was baked before.
pub fn bake_track(sample: impl FnMut(f64, &mut LayerKey) -> bool, bounds: &Rect, keys: &mut Vec<LayerKey>) -> bool {
    let b = bounds;
    let corners = [[b.x, b.y], [b.x + b.w, b.y], [b.x, b.y + b.h], [b.x + b.w, b.y + b.h]];
    keys.clear();
    let mut baker = Baker { sample, corners, keys, failed: false };
    let mut previous = LayerKey::default();
    if !(baker.sample)(0.0, &mut previous) {
        return false;
    }
    previous.t = 0.0;
    baker.keys.push(previous);
    let mut i = 1;
    while i <= INITIAL_SPANS && !baker.failed {
        let t0 = f64::from(i - 1) / f64::from(INITIAL_SPANS);
        let t1 = f64::from(i) / f64::from(INITIAL_SPANS);
        let mut next = LayerKey::default();
        if !(baker.sample)(t1, &mut next) {
            return false;
        }
        next.t = t1 as f32;
        baker.split(t0, &previous, t1, &next, 0);
        previous = next;
        i += 1;
    }
    if baker.failed {
        return false;
    }
    let keys = baker.keys;
    // A run of equal keys needs only its ends.
    let mut kept: Vec<LayerKey> = Vec::with_capacity(keys.len());
    for i in 0..keys.len() {
        let n = kept.len();
        let same = n >= 2
            && kept[n - 1].matrix == kept[n - 2].matrix
            && kept[n - 1].opacity == kept[n - 2].opacity
            && keys[i].matrix == kept[n - 1].matrix
            && keys[i].opacity == kept[n - 1].opacity;
        if same && i + 1 < keys.len() {
            kept[n - 1].t = keys[i].t;
        } else {
            kept.push(keys[i]);
        }
    }
    *keys = kept;
    true
}
