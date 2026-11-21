//! What the wire tests share: tests/wire_tests.cpp's samples, its byte builder and its seeded mutator,
//! a dump of every member with float bits and payload identity (the comparison that does not go through
//! the codec), and the client of `oracle/wire_oracle`.

#![allow(dead_code, unused_imports)]

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

pub use twidget_testkit::{EDGES, Lcg, hex, mutate, unhex};

use twidget_wire::*;

/// Float bits a copy through a float could change: NaNs with payloads and either sign, a signalling
/// NaN, -0, both infinities, a subnormal, the largest float.
pub const SPECIALS: [u32; 8] = [0x7fc12345, 0xffc54321, 0x7f800abc, 0x80000000, 0x7f800000, 0xff800000, 0x00000003, 0x7f7fffff];
pub const SPECIAL_COUNT: usize = SPECIALS.len();

pub fn special(i: usize) -> f32 {
    f32::from_bits(SPECIALS[i])
}

/// tests/wire_tests.cpp's `Bytes`: fields appended little-endian.
#[derive(Clone, Default)]
pub struct Bytes {
    pub s: Vec<u8>,
}

impl Bytes {
    pub fn new() -> Bytes {
        Bytes::default()
    }
    pub fn from(s: &[u8]) -> Bytes {
        Bytes { s: s.to_vec() }
    }
    pub fn head(mut self, form: u32) -> Bytes {
        self.s.push(form as u8);
        self.s.push(1);
        self
    }
    pub fn u8(mut self, v: u32) -> Bytes {
        self.s.push(v as u8);
        self
    }
    pub fn u32(mut self, v: u32) -> Bytes {
        self.s.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u64(mut self, v: u64) -> Bytes {
        self.s.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn i32(self, v: i32) -> Bytes {
        self.u32(v as u32)
    }
    pub fn f32(self, v: f32) -> Bytes {
        self.u32(v.to_bits())
    }
    pub fn color(self, r: u32, g: u32, b: u32, a: u32) -> Bytes {
        self.u8(r).u8(g).u8(b).u8(a)
    }
    /// `n` floats from `first`, one apart.
    pub fn run(mut self, first: f32, n: usize) -> Bytes {
        for i in 0..n {
            self = self.f32(first + i as f32);
        }
        self
    }
    pub fn bytes(mut self, more: &[u8]) -> Bytes {
        self.s.extend_from_slice(more);
        self
    }
}

pub const ROUNDS: usize = 3000;

pub fn tight() -> Caps {
    Caps { commands: 18, canvas_ops: 26, path_ops: 5, floats: 14, stops: 8, filters: 10, clips: 2, text: 300, pixels: 1024, rects: 5 }
}

/// Each cap, by name, to set one alone.
pub const CAP_NAMES: [&str; 10] = ["commands", "canvas_ops", "path_ops", "floats", "stops", "filters", "clips", "text", "pixels", "rects"];

pub fn cap_mut<'a>(caps: &'a mut Caps, name: &str) -> &'a mut usize {
    match name {
        "commands" => &mut caps.commands,
        "canvas_ops" => &mut caps.canvas_ops,
        "path_ops" => &mut caps.path_ops,
        "floats" => &mut caps.floats,
        "stops" => &mut caps.stops,
        "filters" => &mut caps.filters,
        "clips" => &mut caps.clips,
        "text" => &mut caps.text,
        "pixels" => &mut caps.pixels,
        "rects" => &mut caps.rects,
        _ => panic!("no cap {name}"),
    }
}

// ---- Dumps: every member with float bits, payloads by identity ----------------------------------

fn bits(f: f32) -> String {
    format!("{:08x}", f.to_bits())
}

fn floats(v: &[f32]) -> String {
    v.iter().map(|f| bits(*f)).collect::<Vec<_>>().join(",")
}

fn color(c: Color) -> String {
    format!("{},{},{},{}", c.r, c.g, c.b, c.a)
}

fn rect(r: &Rect) -> String {
    floats(&[r.x, r.y, r.w, r.h])
}

fn stops(v: &[Stop]) -> String {
    v.iter().map(|s| format!("{}@{}", bits(s.offset), color(s.color))).collect::<Vec<_>>().join(";")
}

fn ops(v: &[PathOp]) -> String {
    v.iter().map(|o| o.as_u8().to_string()).collect::<Vec<_>>().join("")
}

/// Numbers each payload of one kind by the first place its address appears in a value.
#[derive(Default)]
struct Places {
    seen: HashMap<usize, usize>,
}

impl Places {
    /// The payload's number, and whether this is its first appearance.
    fn place<T>(&mut self, p: &Arc<T>) -> (usize, bool) {
        let key = Arc::as_ptr(p) as *const u8 as usize;
        let next = self.seen.len();
        let mut fresh = false;
        let at = *self.seen.entry(key).or_insert_with(|| {
            fresh = true;
            next
        });
        (at, fresh)
    }
}

fn dump_path(p: &Path) -> String {
    format!(
        "ops={} pts={} fill={} eo={} fc={} g={} xy={} st=[{}] stroke={} sc={} sw={} cap={} join={} ml={} d=[{}] do={} sg={} sgv={} ss=[{}]",
        ops(&p.ops),
        floats(&p.points),
        p.fill,
        p.evenodd,
        color(p.fill_color),
        p.gradient,
        floats(&[p.x1, p.y1, p.x2, p.y2, p.r, p.fx, p.fy]),
        stops(&p.stops),
        p.stroke,
        color(p.stroke_color),
        bits(p.stroke_width),
        p.line_cap,
        p.line_join,
        bits(p.miter_limit),
        floats(&p.dashes),
        bits(p.dash_offset),
        p.stroke_gradient,
        floats(&p.sg),
        stops(&p.stroke_stops),
    )
}

fn dump_filters(list: &Filters) -> String {
    list.iter().map(|f| format!("{}:{}:{}", f.kind.as_u8(), floats(&[f.amount, f.x, f.y]), color(f.color))).collect::<Vec<_>>().join(";")
}

pub fn dump_list(list: &[Command]) -> String {
    let mut paths = Places::default();
    let mut filters = Places::default();
    let mut out = String::new();
    for c in list {
        let path = match &c.path {
            None => "none".to_string(),
            Some(p) => match paths.place(p) {
                (at, true) => format!("#{at}{{{}}}", dump_path(p)),
                (at, false) => format!("#{at}"),
            },
        };
        let filter = match &c.filter {
            None => "none".to_string(),
            Some(f) => match filters.place(f) {
                (at, true) => format!("#{at}{{{}}}", dump_filters(f)),
                (at, false) => format!("#{at}"),
            },
        };
        let _ = writeln!(
            out,
            "k={} r={} q={} cl={} hc={} c={} w={} f={} t={} xy={} i={} cv={} ab={} st=[{}] p={} fl={} o={} m={} h={} in={} key={:016x}",
            c.kind.as_u8(),
            rect(&c.rect),
            floats(&[c.radii.tlx, c.radii.tly, c.radii.trx, c.radii.try_, c.radii.brx, c.radii.bry, c.radii.blx, c.radii.bly]),
            rect(&c.clip),
            c.has_clip,
            color(c.color),
            bits(c.width),
            c.font,
            hex(&c.text),
            floats(&[c.x, c.y]),
            c.image,
            c.canvas,
            floats(&[c.ax, c.ay, c.bx, c.by]),
            stops(&c.stops),
            path,
            filter,
            bits(c.opacity),
            floats(&c.matrix),
            c.hidden,
            c.inset,
            c.key,
        );
    }
    out
}

fn dump_geometry(g: &Geometry) -> String {
    format!("{}/{}", ops(&g.ops), floats(&g.points))
}

fn dump_paint(p: &CanvasPaint) -> String {
    format!("{}:{}:{}:[{}]", p.kind.as_u8(), color(p.color), floats(&p.args), stops(&p.stops))
}

pub fn dump_batch(batch: &[CanvasOp]) -> String {
    let mut clips = Places::default();
    let mut pixels = Places::default();
    let mut out = String::new();
    for o in batch {
        let clip = match &o.clips {
            None => "none".to_string(),
            Some(c) => match clips.place(c) {
                (at, true) => format!(
                    "#{at}{{{}}}",
                    c.iter().map(|c| format!("{}:{}", dump_geometry(&c.path), c.evenodd)).collect::<Vec<_>>().join(";")
                ),
                (at, false) => format!("#{at}"),
            },
        };
        let rgba = match &o.rgba {
            None => "none".to_string(),
            Some(p) => match pixels.place(p) {
                (at, true) => format!("#{at}{{{}}}", hex(p)),
                (at, false) => format!("#{at}"),
            },
        };
        let _ = writeln!(
            out,
            "k={} p={} eo={} paint={} m={} lw={} cap={} join={} ml={} d=[{}] do={} a={} cm={} shc={} sh={} cl={} f={} t={} txt={} st={} i={} cv={} src={} sm={} px={} wh={},{}",
            o.kind.as_u8(),
            dump_geometry(&o.path),
            o.evenodd,
            dump_paint(&o.paint),
            floats(&o.matrix),
            bits(o.line_width),
            o.line_cap,
            o.line_join,
            bits(o.miter_limit),
            floats(&o.dashes),
            bits(o.dash_offset),
            bits(o.alpha),
            o.composite.as_u8(),
            color(o.shadow_color),
            floats(&[o.shadow_blur, o.shadow_x, o.shadow_y]),
            clip,
            o.font,
            hex(&o.text),
            floats(&[o.x, o.y, o.scale_x, o.text_dx]),
            o.stroke_text,
            o.image,
            o.canvas,
            floats(&[o.sx, o.sy, o.sw, o.sh, o.dx, o.dy, o.dw, o.dh]),
            o.smoothing,
            rgba,
            o.w,
            o.h,
        );
    }
    out
}

pub fn dump_damage(damage: &Damage) -> String {
    match damage {
        None => "whole".to_string(),
        Some(rects) => rects.iter().map(rect).collect::<Vec<_>>().join(";"),
    }
}

pub fn dump_metrics(m: &FontMetrics) -> String {
    floats(&[m.ascent, m.descent, m.height, m.x_height, m.zero_width, m.em_ascent, m.em_descent])
}

/// The bytes a decoded value holds, a bound on what decoding it asked the allocator for.
pub fn held_list(list: &[Command]) -> usize {
    let mut n = std::mem::size_of_val(list);
    for c in list {
        n += c.text.capacity() + c.stops.capacity() * std::mem::size_of::<Stop>();
        if let Some(p) = &c.path {
            n += std::mem::size_of::<Path>()
                + p.ops.capacity()
                + 4 * (p.points.capacity() + p.dashes.capacity())
                + std::mem::size_of::<Stop>() * (p.stops.capacity() + p.stroke_stops.capacity());
        }
        if let Some(f) = &c.filter {
            n += f.capacity() * std::mem::size_of::<Filter>();
        }
    }
    n
}

pub fn held_batch(batch: &[CanvasOp]) -> usize {
    let mut n = std::mem::size_of_val(batch);
    for o in batch {
        n += o.path.ops.capacity() + 4 * (o.path.points.capacity() + o.dashes.capacity()) + o.text.capacity();
        n += o.paint.stops.capacity() * std::mem::size_of::<Stop>();
        if let Some(c) = &o.clips {
            n += c.capacity() * std::mem::size_of::<CanvasClip>();
            for clip in c.iter() {
                n += clip.path.ops.capacity() + 4 * clip.path.points.capacity();
            }
        }
        if let Some(p) = &o.rgba {
            n += p.capacity();
        }
    }
    n
}

/// tests/decoder_checks.h's budget: 128 bytes for each input byte and 1024 more. Rust's structs are
/// laid out differently from the C++ ones, so the per-byte figure is the C++ one doubled.
pub fn budget(input: usize) -> usize {
    256 * input + 2048
}

// ---- One form at a time -------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormKind {
    List,
    Damage,
    Batch,
    Metrics,
}

impl FormKind {
    pub const ALL: [FormKind; 4] = [FormKind::List, FormKind::Damage, FormKind::Batch, FormKind::Metrics];

    pub fn number(self) -> u32 {
        match self {
            FormKind::List => 1,
            FormKind::Damage => 2,
            FormKind::Batch => 3,
            FormKind::Metrics => 4,
        }
    }

    pub fn of(number: u32) -> FormKind {
        FormKind::ALL[number as usize - 1]
    }

    /// The oracle's case letter: decode and encode under the same caps.
    pub fn letter(self) -> u8 {
        match self {
            FormKind::List => b'l',
            FormKind::Damage => b'd',
            FormKind::Batch => b'b',
            FormKind::Metrics => b'm',
        }
    }

    /// What `bytes` decode to under `caps`, as a dump, and the bytes it holds.
    pub fn decode(self, bytes: &[u8], caps: &Caps) -> Option<(String, usize)> {
        match self {
            FormKind::List => decode_list(bytes, caps).map(|v| (dump_list(&v), held_list(&v))),
            FormKind::Damage => decode_damage(bytes, caps).map(|v| {
                let held = v.as_ref().map_or(0, |r| r.capacity() * 16);
                (dump_damage(&v), held)
            }),
            FormKind::Batch => decode_batch(bytes, caps).map(|v| (dump_batch(&v), held_batch(&v))),
            FormKind::Metrics => decode_metrics(bytes).map(|v| (dump_metrics(&v), 0)),
        }
    }

    /// The oracle's answer computed by the Rust codec: decode under `decode_caps`, encode again under
    /// `encode_caps`.
    pub fn answer(self, bytes: &[u8], decode_caps: &Caps, encode_caps: &Caps) -> (u8, Vec<u8>) {
        let mut out = Vec::new();
        let encoded = match self {
            FormKind::List => decode_list(bytes, decode_caps).map(|v| encode_list(&v, encode_caps, &mut out)),
            FormKind::Damage => decode_damage(bytes, decode_caps).map(|v| encode_damage(&v, encode_caps, &mut out)),
            FormKind::Batch => decode_batch(bytes, decode_caps).map(|v| encode_batch(&v, encode_caps, &mut out)),
            FormKind::Metrics => decode_metrics(bytes).map(|v| {
                encode_metrics(&v, &mut out);
                true
            }),
        };
        match encoded {
            None => (0, Vec::new()),
            Some(false) => (2, Vec::new()),
            Some(true) => (1, out),
        }
    }
}

// ---- Samples: tests/wire_tests.cpp's, member for member ------------------------------------------

pub struct Named<T> {
    pub name: &'static str,
    pub value: T,
}

pub const EVERY_MEMBER: &str = "every member, on a kind that paints none of them";
pub const SPECIAL_FLOATS: &str = "special floats in scalars, arrays and lists";
pub const PAYLOADS: &str = "payloads: none, empty, shared, equal but apart, referred back to";

pub fn named<'a, T>(samples: &'a [Named<T>], name: &str) -> &'a T {
    &samples.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("a sample named {name}")).value
}

fn spread<const N: usize>(first: f32, step: f32) -> [f32; N] {
    std::array::from_fn(|i| first + step * i as f32)
}

/// A Path with each of its members set and every op, its numbers counted from `base`.
pub fn full_path(base: f32) -> Arc<Path> {
    Arc::new(Path {
        ops: vec![PathOp::Move, PathOp::Line, PathOp::Quad, PathOp::Cubic, PathOp::Close],
        points: (0..14).map(|i| base + 0.5 * i as f32).collect(),
        fill: true,
        evenodd: true,
        fill_color: Color::rgba(1, 2, 3, 4),
        gradient: 2,
        x1: base + 1.0,
        y1: base + 2.0,
        x2: base + 3.0,
        y2: base + 4.0,
        r: base + 5.0,
        fx: base + 6.0,
        fy: base + 7.0,
        stops: vec![Stop::new(0.25, Color::rgba(5, 6, 7, 8)), Stop::new(0.75, Color::rgba(9, 10, 11, 12))],
        stroke: true,
        stroke_color: Color::rgba(13, 14, 15, 16),
        stroke_width: base + 0.25,
        line_cap: 1,
        line_join: 2,
        miter_limit: base + 8.0,
        dashes: vec![base + 9.0, base + 10.0],
        dash_offset: base + 11.0,
        stroke_gradient: 1,
        sg: spread(base + 12.0, 1.0),
        stroke_stops: vec![Stop::new(0.5, Color::rgba(17, 18, 19, 20))],
    })
}

/// A Path whose floats are the special patterns, in its scalars, its sg slots and its lists alike.
pub fn special_path() -> Arc<Path> {
    let mut p = Path {
        ops: vec![PathOp::Move, PathOp::Cubic],
        points: (0..SPECIAL_COUNT).map(special).collect(),
        dashes: (0..SPECIAL_COUNT).map(|i| special(SPECIAL_COUNT - 1 - i)).collect(),
        stops: (0..SPECIAL_COUNT).map(|i| Stop { offset: special(i), color: Color::BLACK }).collect(),
        stroke_stops: (0..SPECIAL_COUNT).map(|i| Stop { offset: special(SPECIAL_COUNT - 1 - i), color: Color::BLACK }).collect(),
        ..Path::default()
    };
    {
        let scalars = [&mut p.x1, &mut p.y1, &mut p.x2, &mut p.y2, &mut p.r, &mut p.fx, &mut p.fy, &mut p.stroke_width];
        for (i, s) in scalars.into_iter().enumerate() {
            *s = special(i);
        }
    }
    p.miter_limit = special(2);
    p.dash_offset = special(3);
    for i in 0..p.sg.len() {
        p.sg[i] = special(SPECIAL_COUNT - 1 - i);
    }
    Arc::new(p)
}

/// One filter of each kind, every member set, numbers from `base`.
pub fn every_filter(base: f32) -> Arc<Filters> {
    Arc::new(
        (0..=FilterKind::LAST.as_u8() as usize)
            .map(|k| Filter {
                kind: FilterKind::from_u8(k as u8).unwrap(),
                amount: base + k as f32,
                x: base - k as f32,
                y: base + 0.5 * k as f32 + 0.25,
                color: Color::rgba(k as u8 + 1, k as u8 + 2, k as u8 + 3, 100 + k as u8),
            })
            .collect(),
    )
}

pub fn special_filters() -> Arc<Filters> {
    Arc::new(
        (0..SPECIAL_COUNT)
            .map(|i| Filter {
                kind: FilterKind::from_u8(i as u8).unwrap(),
                amount: special(i),
                x: special((i + 3) % SPECIAL_COUNT),
                y: special((i + 5) % SPECIAL_COUNT),
                color: Color::BLACK,
            })
            .collect(),
    )
}

/// Every member of a command set, on ClipPop, which paints none of them.
pub fn every_member_command() -> Command {
    Command {
        kind: Cmd::ClipPop,
        rect: Rect::new(1.25, 2.5, 3.75, 5.0),
        radii: Radii { tlx: 0.5, tly: 1.0, trx: 1.5, try_: 2.0, brx: 2.5, bry: 3.0, blx: 3.5, bly: 4.0 },
        clip: Rect::new(6.0, 7.0, 8.0, 9.0),
        has_clip: true,
        color: Color::rgba(10, 20, 30, 40),
        width: 1.0 / 3.0,
        font: 11,
        text: b"text a ClipPop does not paint".to_vec(),
        x: 12.0,
        y: 13.0,
        image: 14,
        canvas: 15,
        ax: 16.0,
        ay: 17.0,
        bx: 18.0,
        by: 19.0,
        stops: vec![Stop::new(0.25, Color::rgba(1, 2, 3, 4)), Stop::new(0.75, Color::rgba(5, 6, 7, 8))],
        path: Some(full_path(100.0)),
        filter: Some(every_filter(200.0)),
        opacity: 0.5,
        matrix: spread(2.0, 0.5),
        hidden: true,
        inset: true,
        key: 0x0123456789abcdef,
    }
}

/// A clip list of two clips with every op between them, numbers from `base`.
pub fn clip_list(base: f32) -> Arc<Clips> {
    Arc::new(vec![
        CanvasClip {
            path: Geometry {
                ops: vec![PathOp::Move, PathOp::Line, PathOp::Quad, PathOp::Cubic, PathOp::Close],
                points: (0..14).map(|i| base + i as f32).collect(),
            },
            evenodd: false,
        },
        CanvasClip {
            path: Geometry { ops: vec![PathOp::Move, PathOp::Line, PathOp::Close], points: vec![base, -base, base + 0.125, base] },
            evenodd: true,
        },
    ])
}

/// 1024 pixel bytes holding every byte value, in an order `step` (odd) sets.
pub fn every_byte(step: usize) -> Arc<Pixels> {
    Arc::new((0..1024).map(|i| (i * step + 3) as u8).collect())
}

/// Every member of an op set, on Reset, which paints none of them.
pub fn every_member_op() -> CanvasOp {
    CanvasOp {
        kind: OpKind::Reset,
        path: Geometry {
            ops: vec![PathOp::Move, PathOp::Line, PathOp::Quad, PathOp::Cubic, PathOp::Close],
            points: (0..14).map(|i| 0.5 + i as f32).collect(),
        },
        evenodd: true,
        paint: CanvasPaint {
            kind: PaintKind::Conic,
            color: Color::rgba(1, 2, 3, 4),
            args: spread(0.25, 1.5),
            stops: vec![Stop::new(0.125, Color::rgba(5, 6, 7, 8)), Stop::new(0.875, Color::rgba(9, 10, 11, 12))],
        },
        matrix: spread(2.0, 0.5),
        line_width: 1.0 / 3.0,
        line_cap: 2,
        line_join: 1,
        miter_limit: 3.5,
        dashes: vec![4.0, 2.0, 1.0],
        dash_offset: 0.75,
        alpha: 0.5,
        composite: Composite::Xor,
        shadow_color: Color::rgba(13, 14, 15, 16),
        shadow_blur: 5.0,
        shadow_x: 6.0,
        shadow_y: -7.0,
        clips: Some(clip_list(20.0)),
        font: 8,
        text: b"text a Reset does not paint".to_vec(),
        x: 9.0,
        y: 10.0,
        scale_x: 0.25,
        text_dx: 11.0,
        stroke_text: true,
        image: 12,
        canvas: 13,
        sx: 14.0,
        sy: 15.0,
        sw: 16.0,
        sh: 17.0,
        dx: 18.0,
        dy: 19.0,
        dw: 20.0,
        dh: 21.0,
        smoothing: false,
        rgba: Some(every_byte(7)),
        w: 32,
        h: 32,
    }
}

pub fn list_samples() -> Vec<Named<DisplayList>> {
    let mut out = vec![Named { name: "empty list", value: Vec::new() }];

    let kinds = (0..=Cmd::LAST.as_u8())
        .map(|k| Command {
            kind: Cmd::from_u8(k).unwrap(),
            rect: Rect::new(k as f32 + 0.5, 1.0, 2.0, 3.0),
            key: u64::from(k) + 1,
            ..Command::default()
        })
        .collect();
    out.push(Named { name: "every command kind", value: kinds });

    out.push(Named { name: EVERY_MEMBER, value: vec![every_member_command()] });

    let mut s = Command { kind: Cmd::Text, ..Command::default() };
    {
        let scalars = [&mut s.width, &mut s.x, &mut s.y, &mut s.ax, &mut s.ay, &mut s.bx, &mut s.by, &mut s.opacity];
        for (i, f) in scalars.into_iter().enumerate() {
            *f = special(i);
        }
    }
    {
        let rects =
            [&mut s.rect.x, &mut s.rect.y, &mut s.rect.w, &mut s.rect.h, &mut s.clip.x, &mut s.clip.y, &mut s.clip.w, &mut s.clip.h];
        for (i, f) in rects.into_iter().enumerate() {
            *f = special(SPECIAL_COUNT - 1 - i);
        }
    }
    {
        let r = &mut s.radii;
        let radii = [&mut r.tlx, &mut r.tly, &mut r.trx, &mut r.try_, &mut r.brx, &mut r.bry, &mut r.blx, &mut r.bly];
        for (i, f) in radii.into_iter().enumerate() {
            *f = special((i + 4) % SPECIAL_COUNT);
        }
    }
    s.stops = (0..SPECIAL_COUNT).map(|i| Stop { offset: special(i), color: Color::rgba(i as u8, 2 * i as u8, 0, 0) }).collect();
    for i in 0..s.matrix.len() {
        s.matrix[i] = special(i);
    }
    s.path = Some(special_path());
    let specials = vec![s, Command { kind: Cmd::FilterPush, filter: Some(special_filters()), ..Command::default() }];
    out.push(Named { name: SPECIAL_FLOATS, value: specials });

    let enums = Arc::new(Path { gradient: i32::MIN, line_cap: i32::MAX, line_join: -1, stroke_gradient: 3, ..Path::default() });
    let ints = vec![
        Command { kind: Cmd::Image, font: i32::MIN, image: i32::MAX, canvas: -1, key: u64::MAX, ..Command::default() },
        Command { kind: Cmd::Text, font: i32::MAX, image: i32::MIN, canvas: i32::MIN, key: 0x8000000000000001, ..Command::default() },
        Command { kind: Cmd::Image, font: -1, image: -2, canvas: i32::MAX, key: 0, ..Command::default() },
        Command { kind: Cmd::Path, path: Some(enums), key: 1, ..Command::default() },
    ];
    out.push(Named { name: "ints and keys at their extremes", value: ints });

    let text = vec![
        Command { kind: Cmd::Text, text: b"nul\0inside".to_vec(), ..Command::default() },
        Command { kind: Cmd::Text, text: b"\xff\xfe\xc3\x28 is not UTF-8".to_vec(), ..Command::default() },
        Command { kind: Cmd::Text, text: (0..300).map(|i| (255 - i % 256) as u8).collect(), ..Command::default() },
    ];
    out.push(Named { name: "text as bytes: a NUL, invalid UTF-8, a long run", value: text });

    let empty_path = Arc::new(Path::default());
    let path = full_path(1.0);
    let equal_path = full_path(1.0);
    let other_path = full_path(50.0);
    let paths =
        [None, Some(&empty_path), Some(&path), Some(&path), Some(&equal_path), Some(&other_path), Some(&empty_path), Some(&other_path)];
    let empty_filters: Arc<Filters> = Arc::new(Vec::new());
    let filters = every_filter(1.0);
    let equal_filters = every_filter(1.0);
    let other_filters = every_filter(9.0);
    let filter_users = [
        Some(&filters),
        None,
        Some(&empty_filters),
        Some(&filters),
        Some(&equal_filters),
        Some(&other_filters),
        Some(&other_filters),
        Some(&empty_filters),
    ];
    let shared = (0..8)
        .map(|i| Command {
            kind: if i % 2 == 1 { Cmd::Path } else { Cmd::FilterPush },
            path: paths[i].cloned(),
            filter: filter_users[i].cloned(),
            ..Command::default()
        })
        .collect();
    out.push(Named { name: PAYLOADS, value: shared });
    out
}

pub fn batch_samples() -> Vec<Named<Batch>> {
    let mut out = vec![Named { name: "empty batch", value: Vec::new() }];

    let kinds = (0..=OpKind::LAST.as_u8())
        .map(|k| CanvasOp { kind: OpKind::from_u8(k).unwrap(), x: k as f32 + 0.25, ..CanvasOp::default() })
        .collect();
    out.push(Named { name: "every op kind", value: kinds });

    out.push(Named { name: EVERY_MEMBER, value: vec![every_member_op()] });

    let paints = (0..=PaintKind::LAST.as_u8())
        .map(|k| CanvasOp {
            paint: CanvasPaint {
                kind: PaintKind::from_u8(k).unwrap(),
                color: Color::rgba(k, 50, 60, 70),
                args: spread(k as f32 + 1.0, 0.5),
                stops: vec![Stop::new(0.5, Color::rgba(1, 2, 3, k))],
            },
            ..CanvasOp::default()
        })
        .collect();
    out.push(Named { name: "every paint kind", value: paints });

    let composites = Composite::ALL.iter().map(|c| CanvasOp { composite: *c, ..CanvasOp::default() }).collect();
    out.push(Named { name: "every composite", value: composites });

    let mut settings = vec![CanvasOp::default(); 6];
    settings[1] = CanvasOp {
        kind: OpKind::Stroke,
        line_width: 3.0,
        line_cap: 1,
        line_join: 2,
        miter_limit: 2.0,
        dashes: vec![1.0, 2.0, 3.0],
        dash_offset: 0.5,
        ..CanvasOp::default()
    };
    settings[2] = CanvasOp {
        kind: OpKind::Image,
        image: 3,
        sx: 1.0,
        sy: 2.0,
        sw: 30.0,
        sh: 40.0,
        dx: 5.0,
        dy: 6.0,
        dw: 70.0,
        dh: 80.0,
        smoothing: false,
        ..CanvasOp::default()
    };
    settings[3] = CanvasOp { kind: OpKind::Image, canvas: 4, matrix: spread(0.5, 0.25), ..CanvasOp::default() };
    settings[4] =
        CanvasOp { kind: OpKind::PutImage, rgba: Some(Arc::new(vec![0x80; 16])), dx: 1.0, dy: 1.0, w: 2, h: 2, ..CanvasOp::default() };
    settings[5] = CanvasOp {
        shadow_color: Color::rgba(0, 0, 0, 128),
        shadow_blur: 4.0,
        shadow_x: 2.0,
        shadow_y: -2.0,
        alpha: 0.25,
        ..CanvasOp::default()
    };
    out.push(Named { name: "line, image, shadow and matrix settings beside their defaults", value: settings });

    let mut s = CanvasOp { kind: OpKind::Stroke, ..CanvasOp::default() };
    s.paint.kind = PaintKind::Linear;
    s.path.ops = vec![PathOp::Move, PathOp::Line, PathOp::Line, PathOp::Line];
    s.path.points = (0..SPECIAL_COUNT).map(special).collect();
    s.dashes = (0..SPECIAL_COUNT).map(|i| special(SPECIAL_COUNT - 1 - i)).collect();
    s.paint.stops = (0..SPECIAL_COUNT).map(|i| Stop { offset: special(i), color: Color::BLACK }).collect();
    let clip = CanvasClip {
        path: Geometry {
            ops: vec![PathOp::Move, PathOp::Line, PathOp::Line, PathOp::Close],
            points: (0..SPECIAL_COUNT).map(|i| special((i + 2) % SPECIAL_COUNT)).collect(),
        },
        evenodd: false,
    };
    {
        let scalars = [
            &mut s.line_width,
            &mut s.miter_limit,
            &mut s.dash_offset,
            &mut s.alpha,
            &mut s.shadow_blur,
            &mut s.shadow_x,
            &mut s.shadow_y,
            &mut s.x,
        ];
        for (i, f) in scalars.into_iter().enumerate() {
            *f = special(i);
        }
    }
    {
        let image = [&mut s.sx, &mut s.sy, &mut s.sw, &mut s.sh, &mut s.dx, &mut s.dy, &mut s.dw, &mut s.dh];
        for (i, f) in image.into_iter().enumerate() {
            *f = special(SPECIAL_COUNT - 1 - i);
        }
    }
    for i in 0..s.matrix.len() {
        s.matrix[i] = special(i);
        s.paint.args[i] = special(i + 2);
    }
    s.y = special(1);
    s.scale_x = special(2);
    s.text_dx = special(3);
    s.clips = Some(Arc::new(vec![clip]));
    out.push(Named { name: SPECIAL_FLOATS, value: vec![s] });

    let empty_clips: Arc<Clips> = Arc::new(Vec::new());
    let clip = clip_list(1.0);
    let equal_clip = clip_list(1.0);
    let other_clip = clip_list(40.0);
    let clip_users =
        [None, Some(&empty_clips), Some(&clip), Some(&clip), Some(&equal_clip), Some(&other_clip), Some(&empty_clips), Some(&other_clip)];
    let empty_pixels: Arc<Pixels> = Arc::new(Vec::new());
    let pixels = every_byte(1);
    let equal_pixels = every_byte(1);
    let other_pixels = every_byte(5);
    let pixel_users = [
        Some(&pixels),
        None,
        Some(&empty_pixels),
        Some(&pixels),
        Some(&equal_pixels),
        Some(&other_pixels),
        Some(&other_pixels),
        Some(&empty_pixels),
    ];
    let shared = (0..8)
        .map(|i| CanvasOp {
            kind: if i % 2 == 1 { OpKind::PutImage } else { OpKind::Fill },
            clips: clip_users[i].cloned(),
            rgba: pixel_users[i].cloned(),
            ..CanvasOp::default()
        })
        .collect();
    out.push(Named { name: PAYLOADS, value: shared });
    out
}

pub fn damage_samples() -> Vec<Named<Damage>> {
    let mut rects = vec![
        Rect::new(1.5, 2.25, 30.0, 40.0),
        Rect::new(-8.0, 0.125, 1.0 / 3.0, 7.0),
        Rect::new(100.0, 200.0, 0.5, 0.75),
        Rect::default(),
        Rect::default(),
    ];
    for i in 0..SPECIAL_COUNT {
        let r = &mut rects[3 + i / 4];
        let field = match i % 4 {
            0 => &mut r.x,
            1 => &mut r.y,
            2 => &mut r.w,
            _ => &mut r.h,
        };
        *field = special(i);
    }
    vec![
        Named { name: "the whole frame", value: None },
        Named { name: "nothing moved", value: Some(Vec::new()) },
        Named { name: "rects, special floats among them", value: Some(rects) },
    ]
}

pub fn metrics_samples() -> Vec<Named<FontMetrics>> {
    let mut out = vec![
        Named { name: "default metrics", value: FontMetrics::default() },
        Named {
            name: "ordinary metrics",
            value: FontMetrics {
                ascent: 12.5,
                descent: 3.25,
                height: 17.0,
                x_height: 6.5,
                zero_width: 7.125,
                em_ascent: 1.0 / 3.0,
                em_descent: 4.75,
            },
        },
    ];
    for (name, shift) in [("special metrics", 0), ("special metrics from the largest float", 7)] {
        let f = |i: usize| special((i + shift) % SPECIAL_COUNT);
        out.push(Named {
            name,
            value: FontMetrics {
                ascent: f(0),
                descent: f(1),
                height: f(2),
                x_height: f(3),
                zero_width: f(4),
                em_ascent: f(5),
                em_descent: f(6),
            },
        });
    }
    out
}

/// Every member of a command, its path and its filters, with a second command that refers back to both.
pub fn full_list() -> DisplayList {
    let path = Arc::new(Path {
        ops: vec![PathOp::Cubic, PathOp::Close],
        points: vec![35.0, 36.0],
        fill: true,
        evenodd: true,
        fill_color: Color::rgba(37, 38, 39, 40),
        gradient: 2,
        x1: 41.0,
        y1: 42.0,
        x2: 43.0,
        y2: 44.0,
        r: 45.0,
        fx: 46.0,
        fy: 47.0,
        stops: vec![Stop::new(0.25, Color::rgba(48, 49, 50, 51))],
        stroke: true,
        stroke_color: Color::rgba(52, 53, 54, 55),
        stroke_width: 56.0,
        line_cap: 1,
        line_join: 2,
        miter_limit: 57.0,
        dashes: vec![58.0],
        dash_offset: 59.0,
        stroke_gradient: 1,
        sg: spread(60.0, 1.0),
        stroke_stops: vec![Stop::new(0.75, Color::rgba(67, 68, 69, 70))],
    });
    let filter =
        Arc::new(vec![Filter { kind: FilterKind::DropShadow, amount: 71.0, x: 72.0, y: 73.0, color: Color::rgba(74, 75, 76, 77) }]);
    let first = Command {
        kind: Cmd::Backdrop,
        rect: Rect::new(1.0, 2.0, 3.0, 4.0),
        radii: Radii { tlx: 5.0, tly: 6.0, trx: 7.0, try_: 8.0, brx: 9.0, bry: 10.0, blx: 11.0, bly: 12.0 },
        clip: Rect::new(13.0, 14.0, 15.0, 16.0),
        has_clip: true,
        color: Color::rgba(17, 18, 19, 20),
        width: 21.0,
        font: -22,
        text: b"ab".to_vec(),
        x: 23.0,
        y: 24.0,
        image: -25,
        canvas: 26,
        ax: 27.0,
        ay: 28.0,
        bx: 29.0,
        by: 30.0,
        stops: vec![Stop::new(0.5, Color::rgba(31, 32, 33, 34))],
        path: Some(Arc::clone(&path)),
        filter: Some(Arc::clone(&filter)),
        opacity: 0.125,
        matrix: spread(78.0, 1.0),
        hidden: true,
        inset: true,
        key: 0x0123456789abcdef,
    };
    let second = Command { kind: Cmd::Path, path: Some(path), filter: Some(filter), ..Command::default() };
    vec![first, second]
}

fn joined(fields: &[Vec<u8>]) -> Vec<u8> {
    fields.concat()
}

/// full_list()'s path as the documentation lays out each member: bit i's bytes at i, none for a flag.
fn full_path_fields() -> Vec<Vec<u8>> {
    vec![
        Bytes::new().u32(2).u8(3).u8(4).s,
        Bytes::new().u32(2).f32(35.0).f32(36.0).s,
        vec![],
        vec![],
        Bytes::new().color(37, 38, 39, 40).s,
        Bytes::new().i32(2).s,
        Bytes::new().f32(41.0).s,
        Bytes::new().f32(42.0).s,
        Bytes::new().f32(43.0).s,
        Bytes::new().f32(44.0).s,
        Bytes::new().f32(45.0).s,
        Bytes::new().f32(46.0).s,
        Bytes::new().f32(47.0).s,
        Bytes::new().u32(1).f32(0.25).color(48, 49, 50, 51).s,
        vec![],
        Bytes::new().color(52, 53, 54, 55).s,
        Bytes::new().f32(56.0).s,
        Bytes::new().i32(1).s,
        Bytes::new().i32(2).s,
        Bytes::new().f32(57.0).s,
        Bytes::new().u32(1).f32(58.0).s,
        Bytes::new().f32(59.0).s,
        Bytes::new().i32(1).s,
        Bytes::new().run(60.0, 7).s,
        Bytes::new().u32(1).f32(0.75).color(67, 68, 69, 70).s,
    ]
}

/// full_list()'s first command, each member's bytes at its bit, the path and the filter list new payloads.
fn full_command_fields() -> Vec<Vec<u8>> {
    vec![
        Bytes::new().run(1.0, 4).s,
        Bytes::new().run(5.0, 8).s,
        Bytes::new().run(13.0, 4).s,
        vec![],
        Bytes::new().color(17, 18, 19, 20).s,
        Bytes::new().f32(21.0).s,
        Bytes::new().i32(-22).s,
        Bytes::new().u32(2).u8(u32::from(b'a')).u8(u32::from(b'b')).s,
        Bytes::new().f32(23.0).s,
        Bytes::new().f32(24.0).s,
        Bytes::new().i32(-25).s,
        Bytes::new().i32(26).s,
        Bytes::new().f32(27.0).s,
        Bytes::new().f32(28.0).s,
        Bytes::new().f32(29.0).s,
        Bytes::new().f32(30.0).s,
        Bytes::new().u32(1).f32(0.5).color(31, 32, 33, 34).s,
        Bytes::new().u32(0).u32(0x01ffffff).bytes(&joined(&full_path_fields())).s,
        Bytes::new().u32(0).u32(1).u8(9).f32(71.0).f32(72.0).f32(73.0).color(74, 75, 76, 77).s,
        Bytes::new().f32(0.125).s,
        Bytes::new().run(78.0, 6).s,
        vec![],
        vec![],
        Bytes::new().u64(0x0123456789abcdef).s,
    ]
}

pub fn full_list_bytes() -> Vec<u8> {
    Bytes::new().head(1).u32(2).u8(17).u32(0x00ffffff).bytes(&joined(&full_command_fields())).u8(11).u32(0x00060000).u32(1).u32(1).s
}

/// Every member of an op, its paint, clips and pixels, with a second op that refers back to both payloads.
pub fn full_batch() -> Batch {
    let clips =
        Arc::new(vec![CanvasClip { path: Geometry { ops: vec![PathOp::Move, PathOp::Close], points: vec![37.0, 38.0] }, evenodd: true }]);
    let rgba = Arc::new(vec![0x00, 0x7f, 0x80, 0xff]);
    let first = CanvasOp {
        kind: OpKind::PutImage,
        path: Geometry { ops: vec![PathOp::Move, PathOp::Line], points: vec![1.0, 2.0, 3.0, 4.0] },
        evenodd: true,
        paint: CanvasPaint {
            kind: PaintKind::Radial,
            color: Color::rgba(5, 6, 7, 8),
            args: spread(9.0, 1.0),
            stops: vec![Stop::new(0.5, Color::rgba(15, 16, 17, 18))],
        },
        matrix: spread(19.0, 1.0),
        line_width: 25.0,
        line_cap: 1,
        line_join: 2,
        miter_limit: 26.0,
        dashes: vec![27.0, 28.0],
        dash_offset: 29.0,
        alpha: 0.5,
        composite: Composite::Luminosity,
        shadow_color: Color::rgba(30, 31, 32, 33),
        shadow_blur: 34.0,
        shadow_x: 35.0,
        shadow_y: 36.0,
        clips: Some(Arc::clone(&clips)),
        font: -39,
        text: b"hi".to_vec(),
        x: 40.0,
        y: 41.0,
        scale_x: 42.0,
        text_dx: 43.0,
        stroke_text: true,
        image: 44,
        canvas: -45,
        sx: 46.0,
        sy: 47.0,
        sw: 48.0,
        sh: 49.0,
        dx: 50.0,
        dy: 51.0,
        dw: 52.0,
        dh: 53.0,
        smoothing: false,
        rgba: Some(Arc::clone(&rgba)),
        w: 1,
        h: 1,
    };
    let second = CanvasOp { clips: Some(clips), rgba: Some(rgba), ..CanvasOp::default() };
    vec![first, second]
}

fn full_paint_fields() -> Vec<Vec<u8>> {
    vec![Bytes::new().color(5, 6, 7, 8).s, Bytes::new().run(9.0, 6).s, Bytes::new().u32(1).f32(0.5).color(15, 16, 17, 18).s]
}

/// full_batch()'s first op, each member's bytes at its bit, the clip list and the pixels new payloads.
fn full_op_fields() -> Vec<Vec<u8>> {
    let mut f = vec![
        Bytes::new().u32(2).u8(0).u8(1).u32(4).run(1.0, 4).s,
        vec![],
        Bytes::new().u8(2).u8(7).bytes(&joined(&full_paint_fields())).s,
        Bytes::new().run(19.0, 6).s,
        Bytes::new().f32(25.0).s,
        Bytes::new().i32(1).s,
        Bytes::new().i32(2).s,
        Bytes::new().f32(26.0).s,
        Bytes::new().u32(2).f32(27.0).f32(28.0).s,
        Bytes::new().f32(29.0).s,
        Bytes::new().f32(0.5).s,
        Bytes::new().u8(25).s,
        Bytes::new().color(30, 31, 32, 33).s,
        Bytes::new().f32(34.0).s,
        Bytes::new().f32(35.0).s,
        Bytes::new().f32(36.0).s,
        Bytes::new().u32(0).u32(1).u32(2).u8(0).u8(4).u32(2).f32(37.0).f32(38.0).u8(1).s,
        Bytes::new().i32(-39).s,
        Bytes::new().u32(2).u8(u32::from(b'h')).u8(u32::from(b'i')).s,
        Bytes::new().f32(40.0).s,
        Bytes::new().f32(41.0).s,
        Bytes::new().f32(42.0).s,
        Bytes::new().f32(43.0).s,
        vec![],
        Bytes::new().i32(44).s,
        Bytes::new().i32(-45).s,
    ];
    for i in 0..8 {
        f.push(Bytes::new().f32((46 + i) as f32).s);
    }
    f.push(vec![]);
    f.push(Bytes::new().u32(0).u32(4).u8(0x00).u8(0x7f).u8(0x80).u8(0xff).s);
    f.push(Bytes::new().i32(1).s);
    f.push(Bytes::new().i32(1).s);
    f
}

pub fn full_batch_bytes() -> Vec<u8> {
    Bytes::new().head(3).u32(2).u8(6).u64(0x3fffffffff).bytes(&joined(&full_op_fields())).u8(1).u64((1 << 16) | (1 << 35)).u32(1).u32(1).s
}

/// Every sample of every form, encoded under `caps`, with its form and name.
pub fn encoded_samples(caps: &Caps) -> Vec<(FormKind, String, Vec<u8>)> {
    let mut out = Vec::new();
    for s in list_samples() {
        let mut b = Vec::new();
        assert!(encode_list(&s.value, caps, &mut b), "list {} encodes", s.name);
        out.push((FormKind::List, s.name.to_string(), b));
    }
    for s in batch_samples() {
        let mut b = Vec::new();
        assert!(encode_batch(&s.value, caps, &mut b), "batch {} encodes", s.name);
        out.push((FormKind::Batch, s.name.to_string(), b));
    }
    for s in damage_samples() {
        let mut b = Vec::new();
        assert!(encode_damage(&s.value, caps, &mut b), "damage {} encodes", s.name);
        out.push((FormKind::Damage, s.name.to_string(), b));
    }
    for s in metrics_samples() {
        let mut b = Vec::new();
        encode_metrics(&s.value, &mut b);
        out.push((FormKind::Metrics, s.name.to_string(), b));
    }
    out
}

// ---- Generated values: seeded, every member, sharing ------------------------------------------

fn any_f32(random: &mut Lcg) -> f32 {
    match random.below(8) {
        0 => special(random.below(SPECIAL_COUNT)),
        1 => 0.0,
        2 => f32::from_bits(random.next() ^ (random.next() << 1)),
        _ => (random.below(4000) as f32 - 2000.0) / 8.0,
    }
}

fn maybe_f32(random: &mut Lcg, default: f32) -> f32 {
    if random.chance(2) { default } else { any_f32(random) }
}

fn any_i32(random: &mut Lcg) -> i32 {
    match random.below(4) {
        0 => [i32::MIN, i32::MAX, -1, 0][random.below(4)],
        1 => random.next() as i32 ^ (random.next() << 1) as i32,
        _ => random.below(8) as i32,
    }
}

fn any_color(random: &mut Lcg) -> Color {
    if random.chance(2) {
        Color::BLACK
    } else {
        Color::rgba(random.next() as u8, random.next() as u8, random.next() as u8, random.next() as u8)
    }
}

fn any_floats(random: &mut Lcg, most: usize) -> Vec<f32> {
    let n = random.below(most + 1);
    (0..n).map(|_| any_f32(random)).collect()
}

fn any_stops(random: &mut Lcg, most: usize) -> Vec<Stop> {
    let n = random.below(most + 1);
    (0..n).map(|_| Stop { offset: any_f32(random), color: any_color(random) }).collect()
}

fn any_ops(random: &mut Lcg, most: usize) -> Vec<PathOp> {
    let n = random.below(most + 1);
    (0..n).map(|_| PathOp::from_u8(random.below(5) as u8).unwrap()).collect()
}

fn any_text(random: &mut Lcg) -> Vec<u8> {
    let n = if random.chance(2) { 0 } else { random.below(40) };
    (0..n).map(|_| random.next() as u8).collect()
}

fn any_array<const N: usize>(random: &mut Lcg, default: [f32; N]) -> [f32; N] {
    if random.chance(2) { default } else { std::array::from_fn(|_| any_f32(random)) }
}

pub fn any_path(random: &mut Lcg) -> Path {
    Path {
        ops: any_ops(random, 6),
        points: any_floats(random, 8),
        fill: random.chance(2),
        evenodd: random.chance(2),
        fill_color: any_color(random),
        gradient: if random.chance(2) { 0 } else { any_i32(random) },
        x1: maybe_f32(random, 0.0),
        y1: maybe_f32(random, 0.0),
        x2: maybe_f32(random, 0.0),
        y2: maybe_f32(random, 0.0),
        r: maybe_f32(random, 0.0),
        fx: maybe_f32(random, 0.0),
        fy: maybe_f32(random, 0.0),
        stops: any_stops(random, 3),
        stroke: random.chance(2),
        stroke_color: any_color(random),
        stroke_width: maybe_f32(random, 1.0),
        line_cap: if random.chance(2) { 0 } else { any_i32(random) },
        line_join: if random.chance(2) { 0 } else { any_i32(random) },
        miter_limit: maybe_f32(random, 4.0),
        dashes: any_floats(random, 3),
        dash_offset: maybe_f32(random, 0.0),
        stroke_gradient: if random.chance(2) { 0 } else { any_i32(random) },
        sg: any_array(random, [0.0; 7]),
        stroke_stops: any_stops(random, 2),
    }
}

fn any_filters(random: &mut Lcg) -> Filters {
    let n = random.below(4);
    (0..n)
        .map(|_| Filter {
            kind: FilterKind::from_u8(random.below(10) as u8).unwrap(),
            amount: any_f32(random),
            x: any_f32(random),
            y: any_f32(random),
            color: any_color(random),
        })
        .collect()
}

/// A payload that is absent, shared with an earlier one, or fresh.
fn any_payload<T>(random: &mut Lcg, pool: &mut Vec<Arc<T>>, fresh: impl FnOnce(&mut Lcg) -> T) -> Option<Arc<T>> {
    match random.below(4) {
        0 => None,
        1 if !pool.is_empty() => Some(Arc::clone(&pool[random.below(pool.len())])),
        _ => {
            let p = Arc::new(fresh(random));
            pool.push(Arc::clone(&p));
            Some(p)
        }
    }
}

pub fn any_list(random: &mut Lcg) -> DisplayList {
    let n = random.below(12);
    let mut paths = Vec::new();
    let mut filters = Vec::new();
    (0..n)
        .map(|_| {
            let sparse = random.chance(2);
            let mut c = Command { kind: Cmd::from_u8(random.below(18) as u8).unwrap(), ..Command::default() };
            if sparse && random.chance(2) {
                c.rect = Rect::new(any_f32(random), any_f32(random), any_f32(random), any_f32(random));
                c.color = any_color(random);
                return c;
            }
            if random.chance(2) {
                c.rect = Rect::new(any_f32(random), any_f32(random), any_f32(random), any_f32(random));
            }
            if random.chance(3) {
                let v = any_array(random, [0.0; 8]);
                c.radii = Radii { tlx: v[0], tly: v[1], trx: v[2], try_: v[3], brx: v[4], bry: v[5], blx: v[6], bly: v[7] };
            }
            if random.chance(3) {
                c.clip = Rect::new(any_f32(random), any_f32(random), any_f32(random), any_f32(random));
            }
            c.has_clip = random.chance(3);
            c.color = any_color(random);
            c.width = maybe_f32(random, 0.0);
            c.font = if random.chance(2) { 0 } else { any_i32(random) };
            c.text = any_text(random);
            c.x = maybe_f32(random, 0.0);
            c.y = maybe_f32(random, 0.0);
            c.image = if random.chance(2) { 0 } else { any_i32(random) };
            c.canvas = if random.chance(2) { 0 } else { any_i32(random) };
            c.ax = maybe_f32(random, 0.0);
            c.ay = maybe_f32(random, 0.0);
            c.bx = maybe_f32(random, 0.0);
            c.by = maybe_f32(random, 0.0);
            c.stops = any_stops(random, 3);
            c.path = any_payload(random, &mut paths, any_path);
            c.filter = any_payload(random, &mut filters, any_filters);
            c.opacity = maybe_f32(random, 1.0);
            c.matrix = any_array(random, IDENTITY_MATRIX);
            c.hidden = random.chance(4);
            c.inset = random.chance(4);
            c.key = if random.chance(2) { 0 } else { u64::from(random.next()) << 32 | u64::from(random.next()) };
            c
        })
        .collect()
}

fn any_geometry(random: &mut Lcg) -> Geometry {
    Geometry { ops: any_ops(random, 5), points: any_floats(random, 8) }
}

pub fn any_batch(random: &mut Lcg) -> Batch {
    let n = random.below(10);
    let mut clips = Vec::new();
    let mut pixels = Vec::new();
    (0..n)
        .map(|_| {
            let mut o = CanvasOp { kind: OpKind::from_u8(random.below(7) as u8).unwrap(), ..CanvasOp::default() };
            if random.chance(2) {
                o.path = any_geometry(random);
            }
            o.evenodd = random.chance(3);
            if random.chance(2) {
                o.paint = CanvasPaint {
                    kind: PaintKind::from_u8(random.below(4) as u8).unwrap(),
                    color: any_color(random),
                    args: any_array(random, [0.0; 6]),
                    stops: any_stops(random, 3),
                };
            }
            o.matrix = any_array(random, IDENTITY_MATRIX);
            o.line_width = maybe_f32(random, 1.0);
            o.line_cap = if random.chance(2) { 0 } else { any_i32(random) };
            o.line_join = if random.chance(2) { 0 } else { any_i32(random) };
            o.miter_limit = maybe_f32(random, 10.0);
            o.dashes = any_floats(random, 3);
            o.dash_offset = maybe_f32(random, 0.0);
            o.alpha = maybe_f32(random, 1.0);
            o.composite = if random.chance(2) { Composite::SourceOver } else { Composite::from_u8(random.below(26) as u8).unwrap() };
            o.shadow_color = if random.chance(2) { Color::TRANSPARENT } else { any_color(random) };
            o.shadow_blur = maybe_f32(random, 0.0);
            o.shadow_x = maybe_f32(random, 0.0);
            o.shadow_y = maybe_f32(random, 0.0);
            o.clips = any_payload(random, &mut clips, |random| {
                let n = random.below(3);
                (0..n).map(|_| CanvasClip { path: any_geometry(random), evenodd: random.chance(2) }).collect()
            });
            o.font = if random.chance(2) { 0 } else { any_i32(random) };
            o.text = any_text(random);
            o.x = maybe_f32(random, 0.0);
            o.y = maybe_f32(random, 0.0);
            o.scale_x = maybe_f32(random, 1.0);
            o.text_dx = maybe_f32(random, 0.0);
            o.stroke_text = random.chance(4);
            o.image = if random.chance(2) { 0 } else { any_i32(random) };
            o.canvas = if random.chance(2) { 0 } else { any_i32(random) };
            o.sx = maybe_f32(random, 0.0);
            o.sy = maybe_f32(random, 0.0);
            o.sw = maybe_f32(random, -1.0);
            o.sh = maybe_f32(random, -1.0);
            o.dx = maybe_f32(random, 0.0);
            o.dy = maybe_f32(random, 0.0);
            o.dw = maybe_f32(random, 0.0);
            o.dh = maybe_f32(random, 0.0);
            o.smoothing = !random.chance(4);
            o.rgba = any_payload(random, &mut pixels, |random| {
                let n = random.below(64);
                (0..n).map(|_| random.next() as u8).collect()
            });
            o.w = if random.chance(2) { 0 } else { any_i32(random) };
            o.h = if random.chance(2) { 0 } else { any_i32(random) };
            o
        })
        .collect()
}

pub fn any_damage(random: &mut Lcg) -> Damage {
    if random.chance(4) {
        return None;
    }
    let n = random.below(7);
    Some((0..n).map(|_| Rect::new(any_f32(random), any_f32(random), any_f32(random), any_f32(random))).collect())
}

pub fn any_metrics(random: &mut Lcg) -> FontMetrics {
    FontMetrics {
        ascent: any_f32(random),
        descent: any_f32(random),
        height: any_f32(random),
        x_height: any_f32(random),
        zero_width: any_f32(random),
        em_ascent: any_f32(random),
        em_descent: any_f32(random),
    }
}

// ---- The C++ oracle -----------------------------------------------------------------------------

pub enum Case {
    /// Sets the caps of the cases after it.
    Caps(Caps),
    /// Decode under the set caps and encode again under them.
    Same(FormKind, Vec<u8>),
    /// Decode under the default caps, encode under the set ones.
    Encode(FormKind, Vec<u8>),
    /// A list decoded under the set caps and encoded after "xy" with its command starts within `most`.
    Starts(u64, Vec<u8>),
}

/// What the oracle answers for a `Starts` case: the bytes written after "xy", the starts' count and each start.
pub fn starts_answer(bytes: &[u8], caps: &Caps, most: u64) -> (u8, Vec<u8>) {
    let Some(list) = decode_list(bytes, caps) else { return (0, Vec::new()) };
    let mut out = b"xy".to_vec();
    let mut starts = vec![7, 8, 9];
    if !encode_list_with_starts(&list, caps, &mut out, &mut starts, most as usize) {
        out.extend_from_slice(starts.len().to_string().as_bytes());
        return (2, out);
    }
    out.extend_from_slice(&(starts.len() as u32).to_le_bytes());
    for s in starts {
        out.extend_from_slice(&(s as u64).to_le_bytes());
    }
    (1, out)
}

pub struct Oracle {
    program: twidget_testkit::Oracle,
}

/// `oracle/wire_oracle`, when `TWIDGET_ORACLE_DIR` names where it is.
pub fn oracle() -> Option<Oracle> {
    twidget_testkit::Oracle::find("wire_oracle").map(|program| Oracle { program })
}

impl Oracle {
    /// The C++'s answer to every case but `Caps`, in order: a status (0 the decode refused, 1 encoded,
    /// 2 the encode refused) and the bytes encoded.
    pub fn run(&self, cases: &[Case]) -> Vec<(u8, Vec<u8>)> {
        let mut input = Vec::new();
        let mut expected = 0;
        for case in cases {
            match case {
                Case::Caps(c) => {
                    input.push(b'C');
                    for v in [c.commands, c.canvas_ops, c.path_ops, c.floats, c.stops, c.filters, c.clips, c.text, c.pixels, c.rects] {
                        input.extend_from_slice(&(v as u64).to_le_bytes());
                    }
                }
                Case::Same(form, bytes) => {
                    input.extend(twidget_testkit::framed(form.letter(), bytes));
                    expected += 1;
                }
                Case::Encode(form, bytes) => {
                    input.extend(twidget_testkit::framed(form.letter().to_ascii_uppercase(), bytes));
                    expected += 1;
                }
                Case::Starts(most, bytes) => {
                    input.push(b's');
                    input.extend_from_slice(&most.to_le_bytes());
                    input.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                    input.extend_from_slice(bytes);
                    expected += 1;
                }
            }
        }
        self.program.run(input, expected)
    }
}
