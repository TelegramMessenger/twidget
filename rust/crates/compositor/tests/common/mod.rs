//! What the compositor tests share: seeded display lists as a renderer paints them (nested clips, groups
//! and filters around drawing commands, canvases, backdrops) with odd values and stray pops among them,
//! their edits for damage, crafted summaries, tracks, and the scripted layer the oracle bakes too.

#![allow(dead_code)]

use std::sync::Arc;

use twidget_compositor::*;
pub use twidget_testkit::Lcg;
use twidget_wire::*;

/// Floats a paint can hold besides ordinary ones: NaNs of either sign, -0, both infinities, a subnormal,
/// the largest float.
pub const SPECIALS: [u32; 8] = [0x7fc12345, 0xffc54321, 0x7fc00000, 0x80000000, 0x7f800000, 0xff800000, 0x00000003, 0x7f7fffff];

pub fn special(random: &mut Lcg) -> f32 {
    f32::from_bits(SPECIALS[random.below(SPECIALS.len())])
}

/// A coordinate: mostly on a 1/8 px grid within a few hundred px, sometimes odd.
pub fn coord(random: &mut Lcg) -> f32 {
    match random.below(40) {
        0 => special(random),
        1 => f32::from_bits(random.next() ^ (random.next() << 1)),
        2 => 0.0,
        _ => (random.below(4000) as f32 - 800.0) / 8.0,
    }
}

pub fn extent(random: &mut Lcg) -> f32 {
    match random.below(30) {
        0 => special(random),
        1 => -(random.below(80) as f32) / 8.0,
        2 => 0.0,
        _ => random.below(1600) as f32 / 8.0,
    }
}

pub fn any_color(random: &mut Lcg) -> Color {
    match random.below(4) {
        0 => Color::BLACK,
        1 => Color::rgba(255, 0, 0, 255),
        _ => Color::rgba(random.next() as u8, random.next() as u8, random.next() as u8, random.next() as u8),
    }
}

pub fn any_rect(random: &mut Lcg) -> Rect {
    Rect::new(coord(random), coord(random), extent(random), extent(random))
}

/// A rectangle inside `outer` (when it has room), the content a clip holds.
pub fn rect_inside(random: &mut Lcg, outer: &Rect) -> Rect {
    if !(outer.w > 2.0 && outer.h > 2.0 && outer.w < 1e6 && outer.h < 1e6) {
        return any_rect(random);
    }
    let w = outer.w * random.below(9) as f32 / 10.0;
    let h = outer.h * random.below(9) as f32 / 10.0;
    let x = outer.x + (outer.w - w) * random.below(11) as f32 / 10.0;
    let y = outer.y + (outer.h - h) * random.below(11) as f32 / 10.0;
    Rect::new(x, y, w, h)
}

pub fn any_radii(random: &mut Lcg) -> Radii {
    match random.below(6) {
        0..=2 => Radii::default(),
        3 => {
            let r = random.below(80) as f32 / 4.0;
            Radii { tlx: r, tly: r, trx: r, try_: r, brx: r, bry: r, blx: r, bly: r }
        }
        _ => {
            let mut v = [0.0f32; 8];
            for x in &mut v {
                *x = if random.chance(12) { special(random) } else { random.below(100) as f32 / 4.0 - 2.0 };
            }
            Radii { tlx: v[0], tly: v[1], trx: v[2], try_: v[3], brx: v[4], bry: v[5], blx: v[6], bly: v[7] }
        }
    }
}

pub fn any_matrix(random: &mut Lcg) -> [f32; 6] {
    match random.below(12) {
        0..=3 => IDENTITY_MATRIX,
        4 | 5 => [1.0, 0.0, 0.0, 1.0, coord(random), coord(random)],
        6 => {
            let s = random.below(40) as f32 / 8.0;
            [s, 0.0, 0.0, s * (random.below(3) as f32 + 1.0) / 2.0, coord(random), coord(random)]
        }
        7 => {
            let angle = random.below(360) as f32 * std::f32::consts::PI / 180.0;
            [angle.cos(), angle.sin(), -angle.sin(), angle.cos(), coord(random), coord(random)]
        }
        8 => [1.0, 0.0, random.below(9) as f32 / 8.0 - 0.5, 1.0, 0.0, 0.0],
        9 => [-1.0, 0.0, 0.0, 1.0, coord(random), 0.0],
        10 => [0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        _ => std::array::from_fn(|_| if random.chance(3) { special(random) } else { coord(random) / 50.0 }),
    }
}

pub fn any_stops(random: &mut Lcg) -> Vec<Stop> {
    (0..random.below(4)).map(|_| Stop::new(random.below(11) as f32 / 10.0, any_color(random))).collect()
}

pub fn any_path(random: &mut Lcg) -> Path {
    let n = random.below(8);
    let mut p = Path::default();
    for _ in 0..n {
        let op = PathOp::from_u8(random.below(5) as u8).unwrap_or(PathOp::Line);
        p.ops.push(op);
        let floats = match op {
            PathOp::Move | PathOp::Line => 2,
            PathOp::Quad => 4,
            PathOp::Cubic => 6,
            PathOp::Close => 0,
        };
        for _ in 0..floats {
            p.points.push(coord(random));
        }
    }
    if random.chance(5) {
        p.points.push(coord(random));
    }
    p.fill = random.chance(2);
    p.evenodd = random.chance(3);
    p.fill_color = any_color(random);
    if random.chance(4) {
        p.gradient = random.below(3) as i32;
        p.x1 = coord(random);
        p.y2 = coord(random);
        p.r = extent(random);
        p.stops = any_stops(random);
    }
    p.stroke = random.chance(2);
    p.stroke_color = any_color(random);
    p.stroke_width = if random.chance(6) { special(random) } else { random.below(40) as f32 / 4.0 };
    p.line_cap = random.below(3) as i32;
    p.line_join = random.below(3) as i32;
    p.miter_limit = [4.0, 0.5, 10.0, 1.0][random.below(4)];
    if random.chance(4) {
        p.dashes = (0..random.below(4)).map(|_| random.below(20) as f32).collect();
        p.dash_offset = random.below(5) as f32;
    }
    if random.chance(5) {
        p.stroke_gradient = 1;
        p.sg = std::array::from_fn(|_| coord(random));
        p.stroke_stops = any_stops(random);
    }
    p
}

pub fn any_filters(random: &mut Lcg) -> Filters {
    (0..random.below(3))
        .map(|_| Filter {
            kind: FilterKind::from_u8(random.below(10) as u8).unwrap_or_default(),
            amount: random.below(40) as f32 / 4.0,
            x: coord(random),
            y: coord(random),
            color: any_color(random),
        })
        .collect()
}

/// Paths and filter lists already made, shared by later commands as a renderer shares them.
#[derive(Default)]
pub struct Pools {
    paths: Vec<Arc<Path>>,
    filters: Vec<Arc<Filters>>,
}

impl Pools {
    fn path(&mut self, random: &mut Lcg) -> Arc<Path> {
        if !self.paths.is_empty() && random.chance(3) {
            return Arc::clone(&self.paths[random.below(self.paths.len())]);
        }
        let p = Arc::new(any_path(random));
        self.paths.push(Arc::clone(&p));
        p
    }

    fn filters(&mut self, random: &mut Lcg) -> Option<Arc<Filters>> {
        if random.chance(5) {
            return None;
        }
        if !self.filters.is_empty() && random.chance(3) {
            return Some(Arc::clone(&self.filters[random.below(self.filters.len())]));
        }
        let f = Arc::new(any_filters(random));
        self.filters.push(Arc::clone(&f));
        Some(f)
    }
}

const DRAWING: [Cmd; 13] = [
    Cmd::FillRect,
    Cmd::FillRect,
    Cmd::StrokeRect,
    Cmd::FillEllipse,
    Cmd::StrokeEllipse,
    Cmd::Text,
    Cmd::Image,
    Cmd::Linear,
    Cmd::Radial,
    Cmd::Conic,
    Cmd::Path,
    Cmd::Shadow,
    Cmd::Backdrop,
];

/// One drawing command, inside `within` when it is given and the draw says so.
pub fn drawing(random: &mut Lcg, pools: &mut Pools, within: Option<&Rect>) -> Command {
    let kind = DRAWING[random.below(DRAWING.len())];
    let mut c = Command { kind, ..Command::default() };
    c.rect = match within {
        Some(r) if random.chance(2) => rect_inside(random, r),
        _ => any_rect(random),
    };
    c.radii = any_radii(random);
    c.color = any_color(random);
    if random.chance(6) {
        c.has_clip = true;
        c.clip = any_rect(random);
    }
    match kind {
        Cmd::StrokeRect | Cmd::StrokeEllipse => c.width = if random.chance(8) { special(random) } else { random.below(40) as f32 / 4.0 },
        Cmd::Text => {
            c.font = 1 + random.below(4) as i32;
            c.text = (0..random.below(12)).map(|_| b"abc xyz\xd0\x96"[random.below(9)]).collect();
            c.x = c.rect.x;
            c.y = c.rect.y + c.rect.h;
        }
        Cmd::Image => {
            if random.chance(2) {
                c.canvas = 1 + random.below(5) as i32;
            } else {
                c.image = 1 + random.below(5) as i32;
            }
        }
        Cmd::Linear | Cmd::Radial | Cmd::Conic => {
            c.ax = coord(random);
            c.ay = coord(random);
            c.bx = coord(random);
            c.by = coord(random);
            c.stops = any_stops(random);
        }
        Cmd::Path => {
            if !random.chance(10) {
                c.path = Some(pools.path(random));
            }
        }
        Cmd::Shadow => {
            c.ax = coord(random) / 8.0;
            c.ay = coord(random) / 8.0;
            c.bx = if random.chance(8) { special(random) } else { random.below(40) as f32 / 2.0 };
            c.by = coord(random) / 16.0;
            c.inset = random.chance(3);
        }
        Cmd::Backdrop => c.filter = pools.filters(random),
        _ => {}
    }
    if random.chance(25) {
        c.canvas = 1 + random.below(5) as i32;
    }
    if random.chance(3) {
        c.key = u64::from(random.next());
    }
    c
}

fn scope(random: &mut Lcg, pools: &mut Pools, list: &mut DisplayList, depth: usize, within: Option<&Rect>) {
    let items = random.below(if depth == 0 { 14 } else { 6 });
    for _ in 0..items {
        let nest = depth < 4 && random.chance(3);
        if !nest {
            list.push(drawing(random, pools, within));
            continue;
        }
        let (open, close, inner) = match random.below(3) {
            0 => {
                let mut c = Command { kind: Cmd::ClipPush, ..Command::default() };
                c.rect = match within {
                    Some(r) if random.chance(2) => rect_inside(random, r),
                    _ => any_rect(random),
                };
                c.radii = any_radii(random);
                if random.chance(6) {
                    c.path = Some(pools.path(random));
                }
                let inner = c.rect;
                (c, Cmd::ClipPop, Some(inner))
            }
            1 => {
                let mut c = Command { kind: Cmd::GroupPush, ..Command::default() };
                c.matrix = any_matrix(random);
                c.opacity = [1.0, 0.5, 0.0, -0.0, 1.0][random.below(5)];
                c.hidden = random.chance(8);
                if random.chance(3) {
                    c.key = 1 + u64::from(random.next());
                    c.rect = any_rect(random);
                }
                (c, Cmd::GroupPop, within.copied())
            }
            _ => {
                let mut c = Command { kind: Cmd::FilterPush, ..Command::default() };
                c.rect = any_rect(random);
                c.filter = pools.filters(random);
                (c, Cmd::FilterPop, within.copied())
            }
        };
        list.push(open);
        scope(random, pools, list, depth + 1, inner.as_ref());
        if !random.chance(30) {
            list.push(Command { kind: close, ..Command::default() });
        }
    }
    if random.chance(40) {
        let stray = [Cmd::ClipPop, Cmd::GroupPop, Cmd::FilterPop][random.below(3)];
        list.push(Command { kind: stray, ..Command::default() });
    }
}

/// A display list as a renderer paints one, with odd values and a stray pop or a missing one now and then.
pub fn any_list(random: &mut Lcg) -> DisplayList {
    let mut pools = Pools::default();
    let mut list = DisplayList::new();
    scope(random, &mut pools, &mut list, 0, None);
    list
}

/// What the next paint of `list` could be: colours changed, commands gone, new and moved, a group's
/// matrix or visibility changed, as a few style changes repaint a document.
pub fn edited(random: &mut Lcg, list: &DisplayList) -> DisplayList {
    let mut out = list.clone();
    let mut pools = Pools::default();
    for _ in 0..random.below(5) {
        let n = out.len();
        match random.below(7) {
            0 if n > 0 => {
                let i = random.below(n);
                out[i].color = any_color(random);
            }
            1 if n > 0 => {
                out.remove(random.below(n));
            }
            2 => {
                let at = random.below(n + 1);
                out.insert(at, drawing(random, &mut pools, None));
            }
            3 if n > 1 => {
                let i = random.below(n - 1);
                out.swap(i, i + 1);
            }
            4 if n > 1 => {
                let c = out.remove(random.below(n));
                let at = random.below(n);
                out.insert(at, c);
            }
            5 if n > 0 => {
                let i = random.below(n);
                if out[i].kind == Cmd::GroupPush {
                    out[i].matrix = any_matrix(random);
                    out[i].hidden = random.chance(2);
                } else {
                    out[i].rect = any_rect(random);
                }
            }
            _ if n > 0 => {
                let i = random.below(n);
                out[i].canvas = random.below(4) as i32;
            }
            _ => {}
        }
    }
    out
}

/// A summary of keys from a few values, so that commands repeat and reorder.
pub fn any_summary(random: &mut Lcg, keys: usize) -> PaintSummary {
    let n = random.below(48);
    PaintSummary {
        entries: (0..n)
            .map(|_| Entry {
                key: 1 + random.below(keys) as u64,
                ink: if random.chance(10) { Rect::new(coord(random), coord(random), -1.0, extent(random)) } else { any_rect(random) },
                canvas: if random.chance(4) { random.below(4) as i32 } else { 0 },
                backdrop: random.chance(10),
            })
            .collect(),
    }
}

/// The scripted layer the oracle bakes too: a family of key functions of t, each `kind` its own.
pub fn sample_of(kind: u8, p: &[f64; 8], t: f64, key: &mut LayerKey) -> bool {
    key.t = p[7] as f32;
    match kind % 8 {
        0 => {
            let angle = p[2] + p[3] * t;
            let (c, s) = (angle.cos(), angle.sin());
            key.matrix = Affine {
                a: c as f32,
                b: s as f32,
                c: -s as f32,
                d: c as f32,
                e: (p[0] - c * p[0] + s * p[1]) as f32,
                f: (p[1] - s * p[0] - c * p[1]) as f32,
            };
        }
        1 => {
            key.matrix = Affine::translate((p[0] + p[1] * t) as f32, (p[2] * t * t) as f32);
            key.opacity = (p[3] + p[4] * t) as f32;
        }
        2 => {
            let n = if p[0] < 1.0 { 1.0 } else { p[0] };
            let v = (t * n).floor() / n;
            key.matrix = Affine { a: (1.0 + p[1] * v) as f32, b: 0.0, c: 0.0, d: (1.0 + p[1] * v) as f32, e: (p[2] * v) as f32, f: 0.0 };
        }
        3 => {
            let u = t * t * (3.0 - 2.0 * t);
            key.matrix = Affine { a: (1.0 + p[0] * u) as f32, b: 0.0, c: 0.0, d: (1.0 + p[1] * u) as f32, e: 0.0, f: 0.0 };
            key.opacity = (1.0 - u * p[2]) as f32;
        }
        4 => {
            if t >= p[0] {
                key.matrix = Affine::translate(p[1] as f32, p[2] as f32);
                key.opacity = p[3] as f32;
            }
        }
        5 => key.matrix = Affine::translate(if t > p[0] { f32::NAN } else { (t * p[1]) as f32 }, 0.0),
        6 => {
            let mut h = 1469598103934665603u64;
            for i in 0..8 {
                h = (h ^ ((t.to_bits() >> (8 * i)) & 0xff)).wrapping_mul(1099511628211);
            }
            key.matrix = Affine::translate((h % 1000) as f32 / 10.0, ((h >> 10) % 1000) as f32 / 10.0);
            key.opacity = ((h >> 20) % 1000) as f32 / 1000.0;
        }
        _ => key.matrix = Affine::translate(p[0] as f32, p[1] as f32),
    }
    // Its NaNs as one: two NaNs met in the arithmetic above keep the payload the compiler's operand order picks.
    let m = &mut key.matrix;
    for v in [&mut m.a, &mut m.b, &mut m.c, &mut m.d, &mut m.e, &mut m.f, &mut key.opacity] {
        if v.is_nan() {
            *v = f32::NAN;
        }
    }
    true
}

pub fn any_key(random: &mut Lcg, t: f32) -> LayerKey {
    let m = any_matrix(random);
    LayerKey {
        t,
        matrix: Affine { a: m[0], b: m[1], c: m[2], d: m[3], e: m[4], f: m[5] },
        opacity: if random.chance(10) { special(random) } else { random.below(11) as f32 / 10.0 },
    }
}

/// A track as the compositor bakes them, its keys ascending from 0 to 1, or an odd one.
pub fn any_track(random: &mut Lcg) -> LayerTrack {
    let n = random.below(12);
    let mut keys = Vec::with_capacity(n);
    for i in 0..n {
        let t = match random.below(12) {
            0 => special(random),
            1 => random.below(11) as f32 / 10.0,
            _ if n > 1 => i as f32 / (n - 1) as f32,
            _ => 0.0,
        };
        keys.push(any_key(random, t));
    }
    LayerTrack {
        start: [0.0, -1.5, 3.25, 1e9][random.below(4)],
        duration: match random.below(10) {
            0 => 0.0,
            1 => -1.0,
            2 => f64::NAN,
            3 => f64::INFINITY,
            4 => 1e-9,
            _ => (1 + random.below(40)) as f64 / 8.0,
        },
        repeat: random.chance(2),
        keys,
    }
}

pub fn any_elapsed(random: &mut Lcg, track: &LayerTrack) -> f64 {
    match random.below(12) {
        0 => 0.0,
        1 => -0.5,
        2 => [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300, f64::MIN_POSITIVE][random.below(5)],
        3 => track.duration * random.below(5) as f64,
        _ => track.duration * random.below(4000) as f64 / 1000.0 + random.below(3) as f64,
    }
}
