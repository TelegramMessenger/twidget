//! The Rust JSON forms against the C++ (`oracle/encode_oracle`, built from core/src/encode.cpp and wire.cpp):
//! the same values reach both sides in their wire forms (lists and batches) or member by member
//! (compositions), and the JSON each writes is compared byte for byte. The values: wire_tests' samples with
//! every member set, generated display lists as a renderer paints them (the compositor tests' generator),
//! canvas batches the canvas interpreter makes of generated command lists (the canvas tests' generator),
//! batches with every member changed, and generated compositions. Skipped without `TWIDGET_ORACLE_DIR`;
//! `tools/gate.py` sets it.

#[path = "../../canvas/tests/common/mod.rs"]
mod canvas_common;
#[path = "../../compositor/tests/common/mod.rs"]
mod compositor_common;
#[path = "../../wire/tests/common/mod.rs"]
mod wire_common;

use std::sync::Arc;

use compositor_common::{any_list, any_matrix, any_radii, any_rect, any_track, special};
use twidget_canvas::{Canvas, CanvasHost};
use twidget_compositor::{Composition, Layer};
use twidget_encode::*;
use twidget_json::Json;
use twidget_testkit::{Comparison, Fields, Lcg, Oracle, framed};
use twidget_wire::*;

fn list_case(c: &mut Comparison, list: &[Command], what: String) {
    let mut bytes = Vec::new();
    assert!(encode_list(list, &Caps::default(), &mut bytes), "{what}: the list encodes");
    c.case(&framed(b'l', &bytes), (1, list_json(list)), what);
}

fn batch_case(c: &mut Comparison, ops: &[CanvasOp], what: String) {
    let mut bytes = Vec::new();
    assert!(encode_batch(ops, &Caps::default(), &mut bytes), "{what}: the batch encodes");
    c.case(&framed(b'b', &bytes), (1, ops_json(ops)), what);
}

fn composition_case(c: &mut Comparison, composition: &Composition, what: String) {
    let mut o = Fields::new();
    o.u64(composition.version).u32(composition.layers.len() as u32);
    for l in &composition.layers {
        o.i64(l.id);
        for v in [l.bounds.x, l.bounds.y, l.bounds.w, l.bounds.h] {
            o.u32(v.to_bits());
        }
        o.u64(l.content_version).u32(l.raster_scale.to_bits()).flag(l.has_clip);
        for v in [l.clip.x, l.clip.y, l.clip.w, l.clip.h] {
            o.u32(v.to_bits());
        }
        let q = &l.clip_radii;
        for v in [q.tlx, q.tly, q.trx, q.try_, q.brx, q.bry, q.blx, q.bly] {
            o.u32(v.to_bits());
        }
        o.u64(l.track_version).f64(l.track.start).f64(l.track.duration).flag(l.track.repeat).u32(l.track.keys.len() as u32);
        for k in &l.track.keys {
            for v in [k.t, k.matrix.a, k.matrix.b, k.matrix.c, k.matrix.d, k.matrix.e, k.matrix.f, k.opacity] {
                o.u32(v.to_bits());
            }
        }
        o.i64(l.parent);
    }
    c.case(&o.framed(b'c'), (1, composition_json(composition)), what);
}

/// A float as painters meet them, with values at the hundredths' rounding edges and special ones.
fn any_float(random: &mut Lcg) -> f32 {
    match random.below(16) {
        0 => special(random),
        1 => f32::from_bits(random.next() ^ (random.next() << 1)),
        2 => (random.below(2000) as f32 - 1000.0) / 1000.0 + 0.005,
        3 => 1e15 + random.below(1000) as f32,
        4 => -0.0,
        _ => (random.below(64000) as f32 - 32000.0) / 64.0,
    }
}

fn any_floats(random: &mut Lcg, n: usize) -> Vec<f32> {
    (0..n).map(|_| any_float(random)).collect()
}

fn any_color(random: &mut Lcg) -> Color {
    Color::rgba(random.next() as u8, random.next() as u8, random.next() as u8, random.next() as u8)
}

fn any_stops(random: &mut Lcg) -> Vec<Stop> {
    (0..random.below(5)).map(|_| Stop::new(any_float(random), any_color(random))).collect()
}

fn any_geometry(random: &mut Lcg) -> Geometry {
    let mut g = Geometry::default();
    for _ in 0..random.below(6) {
        let op = PathOp::ALL[random.below(PathOp::ALL.len())];
        let n = match op {
            PathOp::Move | PathOp::Line => 2,
            PathOp::Quad => 4,
            PathOp::Cubic => 6,
            PathOp::Close => 0,
        };
        g.ops.push(op);
        g.points.extend(any_floats(random, n));
    }
    g
}

fn any_text(random: &mut Lcg) -> Vec<u8> {
    (0..random.below(8)).map(|_| [b'a', b'"', b'\\', b'\n', 0, 0x1f, 0xc3, 0xa9, 0xff, b'/', 0x7f][random.below(11)]).collect()
}

/// A canvas op with any of its members changed, of any kind.
fn any_op(random: &mut Lcg) -> CanvasOp {
    let mut op = CanvasOp { kind: OpKind::ALL[random.below(OpKind::ALL.len())], ..CanvasOp::default() };
    if random.chance(2) {
        op.path = any_geometry(random);
    }
    op.evenodd = random.chance(3);
    if random.chance(2) {
        op.paint.kind = PaintKind::ALL[random.below(PaintKind::ALL.len())];
        op.paint.color = any_color(random);
        for (i, v) in any_floats(random, 6).into_iter().enumerate() {
            op.paint.args[i] = v;
        }
        op.paint.stops = any_stops(random);
    }
    if random.chance(2) {
        op.matrix = any_matrix(random);
    }
    if random.chance(2) {
        op.line_width = any_float(random);
        op.line_cap = random.below(3) as i32;
        op.line_join = random.below(3) as i32;
        op.miter_limit = any_float(random);
        let n = random.below(4);
        op.dashes = any_floats(random, n);
        op.dash_offset = any_float(random);
    }
    if random.chance(3) {
        op.alpha = if random.chance(2) { random.below(11) as f32 / 10.0 } else { any_float(random) };
    }
    if random.chance(3) {
        op.composite = Composite::ALL[random.below(Composite::ALL.len())];
    }
    if random.chance(3) {
        op.shadow_color = any_color(random);
        op.shadow_blur = any_float(random);
        op.shadow_x = any_float(random);
        op.shadow_y = any_float(random);
    }
    if random.chance(3) {
        op.clips =
            Some(Arc::new((0..random.below(3)).map(|_| CanvasClip { path: any_geometry(random), evenodd: random.chance(2) }).collect()));
    }
    op.font = random.below(5) as i32;
    op.text = any_text(random);
    op.x = any_float(random);
    op.y = any_float(random);
    op.scale_x = any_float(random);
    op.text_dx = any_float(random);
    op.stroke_text = random.chance(2);
    op.image = random.below(4) as i32;
    op.canvas = if random.chance(2) { 0 } else { random.below(4) as i32 };
    for v in [&mut op.sx, &mut op.sy, &mut op.sw, &mut op.sh, &mut op.dx, &mut op.dy, &mut op.dw, &mut op.dh] {
        *v = any_float(random);
    }
    op.smoothing = random.chance(2);
    if op.kind == OpKind::PutImage && random.chance(4) {
        op.w = random.below(3) as i32;
        op.h = random.below(3) as i32;
        op.rgba = Some(Arc::new((0..op.w * op.h * 4).map(|_| random.next() as u8).collect()));
    }
    op
}

/// The canvas interpreter's batches of generated command lists, run through one canvas.
fn interpreted(random: &mut Lcg, runs: usize) -> Vec<Batch> {
    let mut host = canvas_common::ScriptHost { allowance: Canvas::MAX_PIXELS, ..canvas_common::ScriptHost::default() };
    let mut canvas = Canvas::new(7, 300, 150, &mut host as &mut dyn CanvasHost);
    let mut out = Vec::new();
    for _ in 0..runs {
        let n = 1 + random.below(12);
        let Some(cmds) = Json::parse(canvas_common::commands(random, n)) else { continue };
        let mut ops = Vec::new();
        canvas.run(&mut host, &cmds, &mut ops);
        out.push(ops);
    }
    out
}

fn any_composition(random: &mut Lcg) -> Composition {
    let mut layers = Vec::new();
    for i in 0..random.below(5) {
        let has_clip = random.chance(2);
        layers.push(Layer {
            id: [10, 0, -1, i64::MAX, i64::MIN][random.below(5)].wrapping_add(i as i64),
            bounds: any_rect(random),
            content_version: [0, 1, u64::MAX, 1 << 60][random.below(4)],
            raster_scale: if random.chance(4) { special(random) } else { 1.0 + random.below(3) as f32 / 2.0 },
            has_clip,
            clip: any_rect(random),
            clip_radii: any_radii(random),
            track_version: random.next() as u64 * u64::from(random.next()),
            track: any_track(random),
            parent: if random.chance(2) { 0 } else { [10, -5, i64::MAX][random.below(3)] },
            ..Layer::default()
        });
    }
    Composition { layers, version: [0, 3, u64::MAX][random.below(3)] }
}

#[test]
fn lists_alike() {
    let Some(oracle) = Oracle::find("encode_oracle") else { return };
    let mut random = Lcg::new(0xe7c0_0001);
    let mut c = Comparison::new();
    for sample in wire_common::list_samples() {
        list_case(&mut c, &sample.value, format!("sample {}", sample.name));
    }
    list_case(&mut c, &wire_common::full_list(), "the full list".into());
    for i in 0..6000 {
        let list = any_list(&mut random);
        list_case(&mut c, &list, format!("generated {i}"));
    }
    // Every command kind with its members drawn at the rounding edges and among the special floats.
    for i in 0..4000 {
        let mut cmd = Command { kind: Cmd::ALL[random.below(Cmd::ALL.len())], ..Command::default() };
        cmd.rect = Rect::new(any_float(&mut random), any_float(&mut random), any_float(&mut random), any_float(&mut random));
        if random.chance(2) {
            let r = any_floats(&mut random, 8);
            cmd.radii = Radii { tlx: r[0], tly: r[1], trx: r[2], try_: r[3], brx: r[4], bry: r[5], blx: r[6], bly: r[7] };
        }
        cmd.has_clip = random.chance(3);
        cmd.clip = Rect::new(any_float(&mut random), any_float(&mut random), any_float(&mut random), any_float(&mut random));
        cmd.color = any_color(&mut random);
        cmd.width = any_float(&mut random);
        cmd.font = random.below(9) as i32 - 1;
        cmd.text = any_text(&mut random);
        cmd.x = any_float(&mut random);
        cmd.y = any_float(&mut random);
        cmd.image = random.below(3) as i32;
        cmd.canvas = if random.chance(2) { 0 } else { random.below(3) as i32 };
        for v in [&mut cmd.ax, &mut cmd.ay, &mut cmd.bx, &mut cmd.by, &mut cmd.opacity] {
            *v = any_float(&mut random);
        }
        cmd.stops = any_stops(&mut random);
        if random.chance(2) {
            let dashes = random.below(3);
            let g = any_geometry(&mut random);
            let sg = any_floats(&mut random, 7);
            let path = Path {
                ops: g.ops,
                points: g.points,
                fill: random.chance(2),
                evenodd: random.chance(2),
                fill_color: any_color(&mut random),
                gradient: random.below(3) as i32,
                x1: any_float(&mut random),
                r: any_float(&mut random),
                stops: any_stops(&mut random),
                stroke: random.chance(2),
                stroke_color: any_color(&mut random),
                stroke_width: any_float(&mut random),
                line_cap: random.below(3) as i32,
                line_join: random.below(3) as i32,
                miter_limit: any_float(&mut random),
                dashes: any_floats(&mut random, dashes),
                dash_offset: any_float(&mut random),
                stroke_gradient: random.below(3) as i32,
                sg: [sg[0], sg[1], sg[2], sg[3], sg[4], sg[5], sg[6]],
                stroke_stops: any_stops(&mut random),
                ..Path::default()
            };
            cmd.path = Some(Arc::new(path));
        }
        if random.chance(2) {
            cmd.filter = Some(Arc::new(
                (0..random.below(4))
                    .map(|_| Filter {
                        kind: FilterKind::ALL[random.below(FilterKind::ALL.len())],
                        amount: any_float(&mut random),
                        x: any_float(&mut random),
                        y: any_float(&mut random),
                        color: any_color(&mut random),
                    })
                    .collect(),
            ));
        }
        cmd.matrix = any_matrix(&mut random);
        cmd.hidden = random.chance(2);
        cmd.inset = random.chance(2);
        list_case(&mut c, &[cmd], format!("command {i}"));
    }
    let tally = c.check(&oracle);
    eprintln!("display lists: {} alike", tally[1]);
}

#[test]
fn batches_alike() {
    let Some(oracle) = Oracle::find("encode_oracle") else { return };
    let mut random = Lcg::new(0xe7c0_0002);
    let mut c = Comparison::new();
    for sample in wire_common::batch_samples() {
        batch_case(&mut c, &sample.value, format!("sample {}", sample.name));
    }
    let mut interpreted_ops = 0;
    for script in 0..300 {
        for (k, batch) in interpreted(&mut random, 20).iter().enumerate() {
            interpreted_ops += batch.len();
            batch_case(&mut c, batch, format!("script {script}, run {k}"));
        }
    }
    for i in 0..6000 {
        let ops: Vec<CanvasOp> = (0..1 + random.below(4)).map(|_| any_op(&mut random)).collect();
        batch_case(&mut c, &ops, format!("generated {i}"));
    }
    let tally = c.check(&oracle);
    eprintln!("canvas batches: {} alike ({interpreted_ops} ops from the interpreter)", tally[1]);
}

#[test]
fn compositions_alike() {
    let Some(oracle) = Oracle::find("encode_oracle") else { return };
    let mut random = Lcg::new(0xe7c0_0003);
    let mut c = Comparison::new();
    composition_case(&mut c, &Composition::default(), "empty".into());
    for i in 0..6000 {
        let composition = any_composition(&mut random);
        composition_case(&mut c, &composition, format!("generated {i}"));
    }
    let tally = c.check(&oracle);
    eprintln!("compositions: {} alike", tally[1]);
}
