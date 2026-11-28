//! tests/core_tests.cpp's checks of the JSON forms: a display list's (a group's exact matrix, an SVG path,
//! a path clip, a shadow), a canvas batch's (a fill's paint, blend mode and clip, putImageData's pixels in
//! base64) and a composition's parent; each parses with widget-sandbox's JSON.

use std::f32::consts::FRAC_1_SQRT_2;
use std::sync::Arc;

use twidget_compositor::{Composition, Layer};
use twidget_encode::*;
use twidget_json::Json;
use twidget_wire::*;

#[test]
fn the_display_list_json_parses() {
    let mut list = DisplayList::new();
    let group = Command {
        kind: Cmd::GroupPush,
        matrix: [FRAC_1_SQRT_2, FRAC_1_SQRT_2, -FRAC_1_SQRT_2, FRAC_1_SQRT_2, 10.0, 20.0],
        opacity: 0.5,
        ..Command::default()
    };
    list.push(group);
    let p = Arc::new(Path {
        ops: vec![PathOp::Move, PathOp::Line, PathOp::Close],
        points: vec![0.0, 0.0, 10.0, 0.0],
        fill: true,
        fill_color: Color::rgba(255, 0, 0, 255),
        stroke: true,
        stroke_width: 2.0,
        dashes: vec![4.0, 2.0],
        ..Path::default()
    });
    list.push(Command { kind: Cmd::Path, path: Some(p.clone()), ..Command::default() });
    list.push(Command { kind: Cmd::ClipPush, path: Some(p), ..Command::default() });
    list.push(Command { kind: Cmd::Shadow, rect: Rect::new(1.0, 2.0, 3.0, 4.0), inset: true, bx: 6.0, ..Command::default() });
    list.push(Command { kind: Cmd::ClipPop, ..Command::default() });
    list.push(Command { kind: Cmd::GroupPop, ..Command::default() });
    let json = Json::parse(list_json(&list)).expect("the display list JSON parses");
    let j = json.arr();
    assert_eq!(j.len(), 6);
    assert!(
        j[0].get("k").as_int(-1) == i32::from(Cmd::GroupPush.as_u8())
            && (j[0].get("m").at(0).as_number(0.0) - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6
            && j[0].get("o").as_number(0.0) == 0.5,
        "a group keeps its matrix exact and its opacity"
    );
    let path = j[1].get("p");
    assert!(
        path.get("o").arr().len() == 3
            && path.get("v").arr().len() == 4
            && path.get("f").as_number(0.0) == f64::from(0xffff0000u32)
            && path.get("sw").as_number(0.0) == 2.0
            && path.get("d").arr().len() == 2,
        "an SVG path carries its geometry, fill and dashed stroke"
    );
    assert!(j[2].get("p").get("o").arr().len() == 3 && j[2].get("p").find("f").is_none(), "a path clip carries the geometry only");
    assert!(
        j[3].get("n").as_int(0) == 1 && j[3].get("a").at(2).as_number(0.0) == 6.0 && j[3].get("r").at(3).as_number(0.0) == 4.0,
        "a shadow carries inset, blur and box"
    );
}

#[test]
fn the_canvas_json_parses() {
    let mut ops = vec![CanvasOp::default(), CanvasOp::default()];
    ops[0].kind = OpKind::Fill;
    ops[0].path.ops = vec![PathOp::Move, PathOp::Line];
    ops[0].path.points = vec![0.0, 0.0, 5.0, 5.0];
    ops[0].paint.kind = PaintKind::Linear;
    ops[0].paint.stops = vec![Stop::new(0.0, Color::BLACK), Stop::new(1.0, Color::rgba(255, 255, 255, 255))];
    ops[0].composite = Composite::Multiply;
    ops[0].clips = Some(Arc::new(vec![CanvasClip::default()]));
    ops[1].kind = OpKind::PutImage;
    ops[1].w = 1;
    ops[1].h = 1;
    ops[1].rgba = Some(Arc::new(vec![1, 2, 3, 4]));
    let canvas = Json::parse(ops_json(&ops)).expect("the canvas JSON parses");
    let c = canvas.arr();
    assert_eq!(c.len(), 2);
    assert!(
        c[0].get("pk").as_int(-1) == 1
            && c[0].get("s").arr().len() == 4
            && c[0].get("cm").as_int(-1) == i32::from(Composite::Multiply.as_u8())
            && c[0].get("cl").arr().len() == 1
            && c[0].find("a").is_none(),
        "a fill carries its paint, blend mode and clip, and no alpha when opaque"
    );
    assert_eq!(c[1].get("px").as_str(), b"AQIDBA==", "putImageData pixels are base64");
}

#[test]
fn the_composition_json_names_the_parent() {
    let layer = |id: i64, parent: i64| Layer { id, parent, ..Layer::default() };
    let composition = Composition { layers: vec![layer(30, 0), layer(31, 30)], version: 4 };
    let text = composition_json(&composition);
    assert!(String::from_utf8_lossy(&text).contains("\"p\":30"), "the JSON names the parent");
    let json = Json::parse(&text).expect("the composition JSON parses");
    assert_eq!(json.get("v").as_int(0), 4);
    assert!(json.get("l").at(0).find("p").is_none(), "a layer over the document has no parent");
}

#[test]
fn numbers_are_hundredths_or_exact_and_null_when_not_finite() {
    let list = vec![Command { kind: Cmd::FillRect, rect: Rect::new(0.125, 1.0 / 3.0, f32::NAN, f32::INFINITY), ..Command::default() }];
    assert_eq!(list_json(&list), b"[{\"k\":0,\"r\":[0.13,0.33,null,null],\"c\":4278190080}]");
    let text = vec![Command { kind: Cmd::Text, text: b"a\"\xff\n".to_vec(), font: 3, ..Command::default() }];
    assert_eq!(
        list_json(&text),
        b"[{\"k\":4,\"c\":4278190080,\"f\":3,\"x\":0,\"y\":0,\"t\":\"a\\\"\xff\\n\"}]",
        "text bytes go in quoted as they are"
    );
}
