//! tests/core_tests.cpp's canvas checks, on a host that answers as the renderer does there over its stub backend
//! (fonts made from the parsed shorthand with the em box the renderer gives them, 8 px per byte of text, the widget's
//! canvas pixels and path floats together), then what the interpreter promises besides.

use std::collections::{HashMap, HashSet};

use twidget_canvas::*;
use twidget_json::Json;
use twidget_wire::{CanvasOp, Color, FontMetrics, Geometry, OpKind, PaintKind, PathOp, set_em_box};

/// The renderer's side of a canvas over core_tests.cpp's StubBackend (or its EmBox variant).
#[derive(Default)]
struct StubHost {
    next: i32,
    fonts: HashMap<Vec<u8>, (i32, FontMetrics)>,
    em_box: bool,
    measured_in: Vec<i32>,
    sources: HashMap<i64, CanvasSource>,
    other_pixels: i64,
    other_floats: usize,
}

impl CanvasHost for StubHost {
    fn canvas_font(&mut self, css: &[u8], metrics: &mut FontMetrics) -> i32 {
        if let Some((handle, m)) = self.fonts.get(css) {
            *metrics = *m;
            return *handle;
        }
        let f = twidget_css::parse_font(css);
        let size = f.size;
        if self.em_box {
            *metrics = FontMetrics {
                ascent: size * 0.9,
                descent: size * 0.3,
                height: size * 1.2,
                em_ascent: size * 0.7,
                em_descent: size * 0.3,
                ..FontMetrics::default()
            };
        } else {
            *metrics = FontMetrics {
                ascent: size * 0.8,
                descent: size * 0.2,
                height: size * 1.2,
                x_height: size * 0.5,
                zero_width: 8.0,
                ..FontMetrics::default()
            };
        }
        self.next += 1;
        if metrics.em_ascent == 0.0 && metrics.em_descent == 0.0 {
            let (a, d) = (metrics.ascent, metrics.descent);
            set_em_box(metrics, a, d, size);
        }
        self.fonts.insert(css.to_vec(), (self.next, *metrics));
        self.next
    }

    fn canvas_text_width(&mut self, font: i32, text: &[u8]) -> f32 {
        self.measured_in.push(font);
        8.0 * text.len() as f32
    }

    fn canvas_source(&mut self, node: i64) -> Option<CanvasSource> {
        self.sources.get(&node).copied()
    }

    fn canvas_allowance(&mut self, _id: i32) -> i64 {
        Canvas::MAX_PIXELS - self.other_pixels
    }

    fn canvas_path_floats(&mut self, _id: i32) -> usize {
        self.other_floats
    }
}

fn run(canvas: &mut Canvas, host: &mut StubHost, cmds: &str) -> Vec<CanvasOp> {
    let cmds = Json::parse(cmds).expect("commands are JSON");
    let mut ops = Vec::new();
    canvas.run(host, &cmds, &mut ops);
    ops
}

fn call(canvas: &Canvas, method: &str, args: &str) -> Json {
    canvas.call(method.as_bytes(), &Json::parse(args).expect("arguments are JSON"))
}

#[test]
fn a_batch_draws_a_rect_an_arc_text_and_a_path2d() {
    let mut host = StubHost::default();
    let mut c = Canvas::new(30, 100, 50, &mut host);
    let ops = run(
        &mut c,
        &mut host,
        r##"[["s","fillStyle","#ff0000"],["c","translate",10,5],["c","fillRect",0,0,20,10],["c","beginPath"],["c","arc",50,20,10,0,6.2832],["s","lineWidth",4],["c","stroke"],
        ["pn",1,"M0 0 L40 0 L40 40 Z"],["c","fillText","hi",0,0],["g",1,"linear",0,0,10,0],["gs",1,0,"red"],["gs",1,1,"blue"],["s","fillStyle",{"g":1}],["c","fill",{"p":1}]]"##,
    );
    assert_eq!(ops.len(), 4, "fillRect, stroke, fillText and fill(Path2D): 4 ops");
    assert!(
        ops[0].kind == OpKind::Fill
            && ops[0].paint.color.r == 255
            && ops[0].path.points.len() == 8
            && ops[0].path.points[0] == 10.0
            && ops[0].path.points[1] == 5.0,
        "fillRect: red, translated into canvas px"
    );
    assert!(ops[1].kind == OpKind::Stroke && ops[1].line_width == 4.0 && ops[1].path.ops.len() >= 5, "arc stroke: béziers, width 4");
    assert!(ops[2].kind == OpKind::Text && ops[2].text == b"hi" && ops[2].matrix[4] == 10.0, "fillText under the transform");
    assert!(
        ops[3].kind == OpKind::Fill
            && ops[3].paint.kind == PaintKind::Linear
            && ops[3].paint.stops.len() == 2
            && ops[3].path.points[0] == 10.0,
        "Path2D filled with a gradient, transformed at fill time"
    );
    assert_eq!(call(&c, "isPointInPath", r#"[{"p":1},30,15]"#), Json::Bool(true), "isPointInPath on a Path2D, under the transform");
    let m = call(&c, "getTransform", "[]");
    assert!(m.arr().len() == 6 && m[4].as_number(0.0) == 10.0, "getTransform");
    let mut sevens = |_: i32, _: i32, w: i32, h: i32, out: &mut Vec<u8>| {
        out.resize(w as usize * h as usize * 4, 7);
        true
    };
    let (answer, binary) = image_data(&Json::parse(r#"{"q":"imageData","id":30,"x":0,"y":0,"w":2,"h":1}"#).unwrap(), Some(&mut sevens));
    assert!(
        binary.as_deref() == Some("BwcHBwcHBwc=") && answer["width"].as_number(0.0) == 2.0,
        "getImageData returns the backend's pixels as base64"
    );
    assert_eq!(call(&c, "toDataURL", "[]").dump_lossy(), r#"{"error":"unsupported"}"#);
}

#[test]
fn the_newest_gradient_still_paints_past_the_object_cap() {
    let mut host = StubHost::default();
    let mut c = Canvas::new(30, 100, 50, &mut host);
    let mut cmds: Vec<String> = (2..=5001).map(|g| format!(r#"["g",{g},"linear",0,0,10,0],["gs",{g},0,"red"]"#)).collect();
    cmds.push(r#"["s","fillStyle",{"g":5001}],["c","fillRect",0,0,5,5]"#.into());
    let ops = run(&mut c, &mut host, &format!("[{}]", cmds.join(",")));
    assert!(
        ops.last().is_some_and(|o| o.kind == OpKind::Fill && o.paint.kind == PaintKind::Linear),
        "the newest gradient still paints past the object cap"
    );
    let ops = run(
        &mut c,
        &mut host,
        r#"[["s","fillStyle",{"g":2}],["c","fillRect",0,0,5,5],["s","fillStyle",{"g":1000}],["c","fillRect",0,0,5,5]]"#,
    );
    assert_eq!(ops.len(), 1, "the oldest went, a newer one stays");
}

#[test]
fn a_size_past_the_widgets_canvas_pixels_is_not_taken() {
    let mut host = StubHost { other_pixels: Canvas::MAX_PIXELS - 2000 * 2000, ..StubHost::default() };
    let mut c = Canvas::new(1, 2000, 2000, &mut host);
    let mut ops = Vec::new();
    c.resize(&mut host, 2000, 2400, &mut ops);
    assert!(
        c.width() == 2000 && c.height() == 2000 && ops.last().is_some_and(|o| o.kind == OpKind::Reset),
        "a size past the widget's canvas pixels keeps the canvas's own, cleared"
    );
    host.other_pixels = Canvas::MAX_PIXELS;
    let fresh = Canvas::new(2, 300, 150, &mut host);
    assert!(fresh.width() == 0 && fresh.height() == 0, "a new canvas past them is 0 x 0");
}

#[test]
fn measure_text_measures_in_the_font_filltext_drew_with() {
    let mut host = StubHost::default();
    let mut c = Canvas::new(30, 50, 20, &mut host);
    let ops = run(&mut c, &mut host, r#"[["s","font","bold 12px serif"],["c","fillText","hi",0,10]]"#);
    let drawn = ops.iter().find(|o| o.kind == OpKind::Text).map_or(0, |o| o.font);
    host.measured_in.clear();
    let answer = measure_text(&mut host, b"bold 12px serif", b"hi");
    assert!(drawn != 0 && host.measured_in == [drawn], "measureText measures in the font fillText drew with");
    assert_eq!(answer["width"].as_number(0.0), 16.0);
    assert_eq!(answer["fontBoundingBoxAscent"].as_number(0.0), f64::from(12.0f32 * 0.8));
}

#[test]
fn a_gradient_with_no_length_paints_nothing() {
    let mut host = StubHost::default();
    let mut c = Canvas::new(50, 10, 10, &mut host);
    let ops = run(&mut c, &mut host, r#"[["g",1,"linear",5,5,5,5],["gs",1,0,"red"],["s","fillStyle",{"g":1}],["c","fillRect",0,0,10,10]]"#);
    assert!(
        ops.last().is_some_and(|o| o.paint.kind == PaintKind::Solid && o.paint.color.a == 0),
        "a gradient with no length paints nothing"
    );
}

#[test]
fn canvas_text_sits_on_the_em_box() {
    let mut host = StubHost { em_box: true, ..StubHost::default() };
    let mut c = Canvas::new(40, 40, 40, &mut host);
    let baselines: Vec<String> = ["top", "hanging", "middle", "alphabetic", "bottom", "ideographic"]
        .iter()
        .map(|b| format!(r#"["s","textBaseline","{b}"],["c","fillText","x",0,0]"#))
        .collect();
    let ops = run(&mut c, &mut host, &format!(r#"[["s","font","10px sans-serif"],{}]"#, baselines.join(",")));
    let ys: String = ops.iter().filter(|o| o.kind == OpKind::Text).map(|o| twidget_css::format_number(f64::from(o.y), 3) + " ").collect();
    assert_eq!(ys, "7 7.2 2 0 -3 -3 ", "canvas text sits on the em box");
}

#[test]
fn draw_image_draws_a_canvas_and_a_refused_image_as_the_placeholder() {
    let mut host = StubHost::default();
    host.sources.insert(41, CanvasSource { image: 0, canvas: 41, w: 10.0, h: 10.0 });
    host.sources.insert(50, CanvasSource { image: 0, canvas: 0, w: 48.0, h: 48.0 });
    let mut c = Canvas::new(40, 40, 40, &mut host);
    let ops = run(&mut c, &mut host, r#"[["c","drawImage",{"n":41},0,0]]"#);
    assert!(ops.iter().any(|o| o.kind == OpKind::Image && o.canvas == 41), "drawImage of a canvas draws it");

    let mut c = Canvas::new(42, 48, 48, &mut host);
    let ops = run(&mut c, &mut host, r#"[["c","drawImage",{"n":50},0,0,24,24]]"#);
    let images = ops.iter().filter(|o| o.kind == OpKind::Image).count();
    let fills: Vec<&CanvasOp> = ops.iter().filter(|o| o.kind == OpKind::Fill).collect();
    let inside = fills.iter().all(|o| {
        o.paint.color.r == 136
            && o.clips.as_ref().is_some_and(|c| !c.is_empty())
            && o.path.points.iter().all(|&v| (-0.01..=24.01).contains(&v))
    });
    assert!(images == 0 && fills.len() == 3 && inside, "drawImage of the placeholder draws its drawing into the destination");
    assert!(
        fills.windows(2).all(|w| std::sync::Arc::ptr_eq(w[0].clips.as_ref().unwrap(), w[1].clips.as_ref().unwrap())),
        "its three fills share one clip list"
    );
}

#[test]
fn past_4096_paths_the_first_still_fills_and_the_refused_one_does_not() {
    let mut host = StubHost::default();
    let mut c = Canvas::new(43, 8, 8, &mut host);
    let mut cmds = vec![r##"["s","fillStyle","#ff0000"],["pn",1,"M1 1 L3 1 L3 3 Z"]"##.to_string()];
    cmds.extend((2..=4097).map(|i| format!(r#"["pn",{i},"M0 0 L4 0 L4 4 Z"]"#)));
    cmds.push(r#"["c","fill",{"p":1}],["c","fill",{"p":4097}]"#.into());
    let ops = run(&mut c, &mut host, &format!("[{}]", cmds.join(",")));
    let fills: Vec<&CanvasOp> = ops.iter().filter(|o| o.kind == OpKind::Fill).collect();
    assert!(fills.len() == 1 && fills[0].path.points[0] == 1.0, "past 4,096 paths the first still fills and the refused one does not");
}

fn draw(c: &mut Canvas, host: &mut StubHost, cmds: &str) -> Vec<CanvasOp> {
    run(c, host, &format!(r#"[["r",100,100],{cmds}]"#))
}

#[test]
fn geometry_a_few_commands_multiply_is_bounded() {
    let mut host = StubHost::default();
    let mut c = Canvas::new(40, 100, 100, &mut host);
    let mut cmds = r#"["pn",1,"M0 0L10 10L20 0Z"],["pn",2,"M0 0L10 10L20 0Z"]"#.to_string();
    for _ in 0..14 {
        cmds += r#",["pc",1,"addPath",{"p":2}],["pc",2,"addPath",{"p":1}]"#;
    }
    let ops = draw(&mut c, &mut host, &(cmds + r#",["c","fill",{"p":1}]"#));
    let longest = ops.iter().map(|o| o.path.points.len()).max().unwrap_or(0);
    assert!(longest > 0 && longest <= 1 << 20, "two Path2D objects added to each other stop growing ({longest} floats)");

    let mut cmds = r#"["c","beginPath"]"#.to_string();
    for i in 0..500 {
        cmds += &format!(r#",["c","lineTo",{},{}]"#, i % 100, i * 7 % 100);
    }
    for _ in 0..300 {
        cmds += r#",["c","clip"],["c","fillRect",0,0,1,1]"#;
    }
    let ops = draw(&mut c, &mut host, &cmds);
    let clipped = ops.iter().map(|o| o.clips.as_ref().map_or(0, |l| l.iter().map(|c| c.path.points.len()).sum())).max().unwrap_or(0);
    let fills = ops.iter().filter(|o| o.kind == OpKind::Fill).count();
    assert!(fills == 300 && clipped <= 1 << 14, "a clip list of long paths stops growing ({clipped} floats in one)");

    let mut d = "M0 0".to_string();
    for i in 0..50000 {
        d += &format!(" {} {}", i % 100, i * 7 % 100);
    }
    let mut cmds = format!(r#"["pn",3,"{d}"]"#);
    for _ in 0..400 {
        cmds += r#",["c","fill",{"p":3}]"#;
    }
    let ops = draw(&mut c, &mut host, &cmds);
    let drawn: usize = ops.iter().map(|o| o.path.points.len()).sum();
    assert!((100000..=1 << 24).contains(&drawn), "one canvas op hands the backend a bounded amount of geometry ({drawn} floats)");
}

#[test]
fn the_clip_lists_a_stack_keeps_stop_growing() {
    let mut host = StubHost::default();
    let mut c = Canvas::new(40, 100, 100, &mut host);
    let mut cmds = r#"["c","rect",0,0,9,9]"#.to_string();
    for _ in 0..512 {
        cmds += r#",["c","clip"],["c","save"]"#;
    }
    for _ in 0..512 {
        cmds += r#",["c","restore"],["c","fillRect",0,0,1,1]"#;
    }
    let ops = run(&mut c, &mut host, &format!("[{cmds}]"));
    let mut lists = HashSet::new();
    let (mut kept, mut nested) = (0, 0);
    for o in &ops {
        let Some(l) = &o.clips else { continue };
        if !lists.insert(std::sync::Arc::as_ptr(l)) {
            continue;
        }
        nested = nested.max(l.len());
        kept += l.iter().map(|c| c.path.points.len() + 16).sum::<usize>();
    }
    let fills = ops.iter().filter(|o| o.kind == OpKind::Fill).count();
    assert!(
        fills == 512 && nested >= 100 && kept <= (1 << 18) + (1 << 14),
        "a stack of 512 states keeps a bounded amount of clip geometry ({kept} floats, {nested} clips nested)"
    );
}

#[test]
fn a_canvas_keeps_short_strings() {
    let mut host = StubHost::default();
    let mut c = Canvas::new(40, 100, 100, &mut host);
    let big = "x".repeat(32000);
    let ops = run(
        &mut c,
        &mut host,
        &format!(
            r#"[["s","font","20px serif"],["s","textAlign","center"],["s","textBaseline","top"],["c","fillText","ab",50,50],["s","font","20px {big}"],["s","textAlign","{big}"],["s","textBaseline","{big}"],["c","fillText","ab",50,50]]"#
        ),
    );
    let texts: Vec<&CanvasOp> = ops.iter().filter(|o| o.kind == OpKind::Text).collect();
    assert!(
        texts.len() == 2
            && texts[1].font == texts[0].font
            && texts[0].text_dx == -8.0
            && texts[1].text_dx == -8.0
            && texts[0].y != 50.0
            && texts[1].y == texts[0].y,
        "a canvas ignores a font of 32 K and a textAlign and a textBaseline that are no keyword"
    );
}

#[test]
fn placeholders_under_a_long_clip_copy_a_bounded_amount_of_it() {
    let mut host = StubHost::default();
    host.sources.insert(50, CanvasSource { image: 0, canvas: 0, w: 48.0, h: 48.0 });
    let mut c = Canvas::new(40, 100, 100, &mut host);
    let mut d = "M0 0".to_string();
    for i in 0..8000 {
        d += &format!("L{} {}", i % 9, i % 7);
    }
    let mut cmds = format!(r#"["pn",1,"{d}"],["c","clip",{{"p":1}}]"#);
    for _ in 0..1200 {
        cmds += r#",["c","drawImage",{"n":50},0,0]"#;
    }
    let ops = run(&mut c, &mut host, &format!("[{cmds}]"));
    let mut lists = HashSet::new();
    let mut copied = 0;
    for o in &ops {
        if let Some(l) = &o.clips
            && lists.insert(std::sync::Arc::as_ptr(l))
        {
            copied += l.iter().map(|c| c.path.points.len()).sum::<usize>();
        }
    }
    let fills = ops.iter().filter(|o| o.kind == OpKind::Fill).count();
    assert!(fills >= 3 && copied <= 1 << 24, "placeholders drawn under a long clip copy a bounded amount of it ({copied} floats)");
}

#[test]
fn the_paths_a_widgets_canvases_keep_are_bounded_together() {
    let mut host = StubHost::default();
    let mut canvases: Vec<Canvas> = (0..8).map(|i| Canvas::new(40 + i, 16, 16, &mut host)).collect();
    let mut cmds = r#"["pn",1,"M0 0L1 1L2 2L3 3"],["pn",2,null],["pn",3,null],["pn",4,null],["pn",5,null]"#.to_string();
    for _ in 0..13 {
        cmds += r#",["pc",2,"addPath",{"p":1}],["pc",1,"addPath",{"p":2}]"#;
    }
    cmds += r#",["pc",3,"addPath",{"p":2}],["pc",4,"addPath",{"p":2}],["pc",5,"addPath",{"p":1}],["c","addPath",{"p":2}]"#;
    // The renderer's canvas_path_floats: what every other canvas keeps.
    let others =
        |canvases: &[Canvas], i: usize| canvases.iter().enumerate().filter(|&(k, _)| k != i).map(|(_, c)| c.path_floats()).sum::<usize>();
    for i in 0..8 {
        host.other_floats = others(&canvases, i);
        run(&mut canvases[i], &mut host, &format!("[{cmds}]"));
    }
    let mut kept = 0;
    for i in 0..8 {
        host.other_floats = others(&canvases, i);
        let ops = run(
            &mut canvases[i],
            &mut host,
            r#"[["c","fill",{"p":1}],["c","fill",{"p":2}],["c","fill",{"p":3}],["c","fill",{"p":4}],["c","fill",{"p":5}],["c","fill"]]"#,
        );
        kept += ops.iter().map(|o| o.path.points.len()).sum::<usize>();
    }
    assert!((1 << 20..=5 << 20).contains(&kept), "eight canvases keep the paths of one together ({kept} floats)");
}

#[test]
fn an_arc_without_a_finite_sweep_adds_no_curve() {
    let mut host = StubHost::default();
    let mut c = Canvas::new(40, 100, 100, &mut host);
    let ops = run(
        &mut c,
        &mut host,
        r#"[["c","moveTo",1,1],["c","arc",50,50,10,1e308,-1e308],["c","lineTo",2,2],["c","stroke"],["pn",1,"M1e308 0A5 5 0 0 1 -1e308 5"],["c","fill",{"p":1}]]"#,
    );
    let strokes = ops.iter().filter(|o| o.kind == OpKind::Stroke).count();
    let fills = ops.iter().filter(|o| o.kind == OpKind::Fill).count();
    let curves: usize = ops.iter().map(|o| o.path.ops.iter().filter(|&&op| op == PathOp::Cubic).count()).sum();
    let finite = ops.iter().filter(|o| o.kind == OpKind::Stroke).all(|o| o.path.points.iter().all(|v| v.is_finite()));
    assert!(
        strokes == 1 && fills == 1 && curves == 0 && finite,
        "an arc from 1e308 to -1e308 radians, and an SVG arc between such points, add no curve of NaN"
    );
}

#[test]
fn objects_made_under_numbers_past_every_int_are_the_ones_those_numbers_name() {
    let mut host = StubHost::default();
    let mut c = Canvas::new(40, 100, 100, &mut host);
    let ops = run(
        &mut c,
        &mut host,
        r##"[["pn",1e300,"M0 0L5 5L0 5Z"],["c","fill",{"p":1e300}],["g",-1e300,"linear",0,0,10,0],["gs",-1e300,0,"#f00"],["s","fillStyle",{"g":-1e300}],
        ["c","fillRect",0,0,10,10],["c","drawImage",{"n":1e300},0,0]]"##,
    );
    let paths = ops.iter().filter(|o| o.kind == OpKind::Fill && o.paint.kind == PaintKind::Solid && o.path.points.len() == 6).count();
    let gradients = ops.iter().filter(|o| o.kind == OpKind::Fill && o.paint.kind == PaintKind::Linear).count();
    assert!(paths == 1 && gradients == 1, "a path and a gradient made under 1e300 and -1e300 are the ones those numbers name");
}

#[test]
fn a_canvas_of_no_size_draws_nothing_but_keeps_its_objects() {
    let mut host = StubHost::default();
    let mut c = Canvas::new(1, 0, 10, &mut host);
    let ops = run(
        &mut c,
        &mut host,
        r#"[["s","fillStyle","red"],["c","translate",5,5],["c","fillRect",0,0,5,5],["pn",1,"M0 0L1 1"],["r",10,10],["c","fill",{"p":1}]]"#,
    );
    assert_eq!(
        ops.iter().map(|o| o.kind).collect::<Vec<_>>(),
        [OpKind::Reset, OpKind::Fill],
        "a 0-wide canvas runs no method; a resize resets the state but not the Path2D"
    );
    assert_eq!(ops[1].paint.color, Color::BLACK, "the fillStyle went with the resize");
}

#[test]
fn svg_paths_read_as_the_c_plus_plus_reads_them() {
    let mut g = Geometry::default();
    append_svg_path(b"M10-5l5.5.5h3v-2Q1 2 3 4t5 5z m1 1", &mut g);
    assert_eq!(g.ops, [PathOp::Move, PathOp::Line, PathOp::Line, PathOp::Line, PathOp::Quad, PathOp::Quad, PathOp::Close, PathOp::Move]);
    assert_eq!(&g.points[..10], &[10.0, -5.0, 15.5, -4.5, 18.5, -4.5, 18.5, -6.5, 1.0, 2.0]);
    assert_eq!(&g.points[g.points.len() - 2..], &[11.0, -4.0], "after z the next relative move counts from the subpath's start");
    let mut arc = Geometry::default();
    append_svg_path(b"M0 0A10 10 0 0 1 20 0", &mut arc);
    assert!(arc.ops[1..].iter().all(|&op| op == PathOp::Cubic) && arc.ops.len() == 3, "a half turn is two quarter béziers");
    let mut bad = Geometry::default();
    append_svg_path(b"M0 0L1x", &mut bad);
    assert_eq!(bad.ops, [PathOp::Move], "a bad number ends the path");
    let square = Geometry {
        ops: vec![PathOp::Move, PathOp::Line, PathOp::Line, PathOp::Line, PathOp::Close],
        points: vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0],
    };
    assert!(geometry_contains(&square, 5.0, 5.0, false) && !geometry_contains(&square, 15.0, 5.0, true));
}
