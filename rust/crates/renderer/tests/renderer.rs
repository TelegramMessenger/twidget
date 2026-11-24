//! core_tests.cpp's checks of builds, patches, the viewport, the layout budget and the name table, `frozen`, the theme
//! and the safe area, the bot's profile page and a bubble's place (auto/46's probes, a page's heights, a relayout per
//! host message), the size reports and resize diffs, and the queries, by their names, on the port. What the document
//! answers comes from the C++ renderer running the same script (`oracle/renderer_oracle`) as a tape the port replays;
//! the port's own messages and state are checked against what core_tests expects of the C++, and against the C++'s
//! transcript. core_tests' sleeps are steps that far apart. Those checks are skipped without `TWIDGET_ORACLE_DIR`; the
//! last tests are the port's own, on a document that is never built.

mod common;

use std::collections::BTreeMap;

use common::cases::compare;
use common::*;
use twidget_animations::{Declared, StyleValues};
use twidget_events::{EditorStyle, TextRun};
use twidget_json::Json;
use twidget_renderer::{BoxStyle, Clip, Engine, HoleChange, Renderer, ScrollExtent, TextContent, Tile};
use twidget_testkit::Oracle;
use twidget_wire::{DisplayList, Rect};

/// html(2) head(3) body(4); A(10) > A1(11); B(12) at the right; a style element (20).
const PAGE: &str = r##"[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
 {"op":"el","id":20,"tag":"style","attrs":[]},
 {"op":"css","id":20,"text":"#a{width:100px;height:100px;float:left;color:light-dark(#010203,#040506)} #a1{width:50px;height:50px} #b{width:100px;height:100px;float:left} @keyframes spin{from{transform:rotate(0deg)}to{transform:rotate(360deg)}} .spin{animation:spin 1s linear infinite} .t{transition:opacity 200ms linear}"},
 {"op":"ins","parent":3,"id":20,"before":null},
 {"op":"el","id":10,"tag":"div","attrs":[["id","a"]]},
 {"op":"el","id":11,"tag":"div","attrs":[["id","a1"]]},
 {"op":"el","id":12,"tag":"div","attrs":[["id","b"]]},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null},
 {"op":"ins","parent":4,"id":12,"before":null}
]"##;

const ROOTS: &str = r#"{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]}"#;

/// The scripts through the oracle and the port, alike; None without the oracle.
fn replay_all(scripts: Vec<Script>) -> Option<Vec<Run>> {
    let oracle = Oracle::find("renderer_oracle")?;
    let mut tally = Tally::default();
    let named: Vec<(Script, String)> = scripts.into_iter().enumerate().map(|(i, s)| (s, format!("script {i}"))).collect();
    Some(compare(&oracle, &named, &mut tally))
}

fn replay(script: Script) -> Option<Run> {
    replay_all(vec![script]).and_then(|mut r| r.pop())
}

fn has(out: &[Json], kind: &[u8]) -> bool {
    out.iter().any(|m| m.get("type").as_str() == kind)
}

/// An Inspect step's wants_tick() and next_tick_in().
fn inspected(extra: &[u8]) -> (bool, f64) {
    (extra[0] != 0, f64::from_le_bytes(extra[1..9].try_into().expect("eight bytes")))
}

#[test]
fn coalescing() {
    let mut s = Script::new(30);
    s.ops(0.0, PAGE);
    s.ops(16.0, r#"[{"op":"attr","id":12,"name":"class","value":"spin"}]"#);
    let before = s.steps.len() - 1;
    for i in 0..3 {
        s.ops(16.0, &format!(r#"[{{"op":"style","id":4,"name":"padding-top","value":"{}px"}}]"#, 1 + i));
    }
    s.after(0.0, Step::Inspect);
    let waited = s.steps.len() - 1;
    s.after(16.0, Step::Tick);
    let ticked = s.steps.len() - 1;
    s.touch(16.0, 0, 250.0, 250.0, 0, true);
    let touched = s.steps.len() - 1;
    s.ops(16.0, r#"[{"op":"style","id":10,"name":"width","value":"90px"}]"#);
    let batch = s.steps.len() - 1;
    s.query(0.0, 1, r#"{"q":"rect","id":10}"#);
    let Some(r) = replay(s) else { return };
    let builds = r.stats[before].builds;
    assert!(r.stats[waited].builds == builds && inspected(&r.extras[waited]).0, "while something animates, batches wait for the frame");
    let (patched, serialised) = (r.stats[waited].in_place, r.stats[waited].builds - r.stats[waited].patched);
    let t = &r.stats[ticked];
    assert!(t.in_place == patched + 1 && t.builds - t.patched == serialised, "and the frame builds them once, in place");
    assert!(r.stats[batch].builds > r.stats[touched].builds, "a batch right after input builds at once");
    let width = result(r.outs.last().expect("the query")).get("width").as_number(0.0);
    assert_eq!(width, 90.0, "and a query sees it");
}

#[test]
fn render_throttle() {
    let mut s = Script::new(30);
    s.ops(0.0, PAGE);
    // Past the load grace: a costly paint, three times as long held back.
    s.after(1100.0, Step::Charge(0.05));
    let charged = s.steps.len() - 1;
    s.ops(0.0, r#"[{"op":"style","id":4,"name":"padding-top","value":"3px"}]"#);
    s.after(0.0, Step::Tick);
    s.after(0.0, Step::Inspect);
    let held = s.steps.len() - 1;
    s.after(200.0, Step::Tick);
    let later = s.steps.len() - 1;
    let Some(r) = replay(s) else { return };
    let builds = r.stats[charged].builds;
    assert!(r.stats[held].builds == builds && inspected(&r.extras[held]).0, "after a costly pass, a batch waits out the throttle");
    assert!(r.stats[later].builds > builds, "and builds once it is over");
    assert!(r.stats[later].throttled >= 1, "the pass counted as throttled");
}

#[test]
fn height_bound() {
    let mut s = Script::new(10);
    s.ops(0.0, &format!(r#"[{ROOTS},{{"op":"el","id":5,"tag":"div","attrs":[["style","height:2000px"]]}},{{"op":"ins","parent":4,"id":5,"before":null}}]"#));
    s.after(16.0, Step::MaxHeight(3000.0));
    let Some(r) = replay(s) else { return };
    assert_eq!(r.viewports[0], 600, "a widget stops at 600 px unless the host says otherwise");
    assert!(r.viewports[1] == 2000 && !r.outs[1].is_empty(), "a host's own bound lets it grow to its content");
}

#[test]
fn font_cache() {
    let mut s = Script::new(10);
    s.ops(0.0, &format!(r#"[{ROOTS},{{"op":"text","id":5,"data":"hello"}},{{"op":"ins","parent":4,"id":5,"before":null}}]"#));
    for _ in 0..2 {
        s.query(0.0, 1, r#"{"q":"measureText","font":"12px serif","text":"abc"}"#);
    }
    let Some(r) = replay(s) else { return };
    assert_eq!(r.stub.made.len(), 1, "measureText keeps its font for the next query");
    assert_eq!(result(&r.outs[1]).get("width").as_number(0.0), 24.0, "and measures in it");
}

#[test]
fn viewport_height() {
    let mut s = Script::new(40);
    s.ops(0.0, &format!(r#"[{ROOTS}]"#).replace("margin:0", "margin:0;line-height:20px"));
    for i in 0..5 {
        let (e, t) = (10 + i, 20 + i);
        s.ops(
            16.0,
            &format!(r#"[{{"op":"el","id":{e},"tag":"div"}},{{"op":"ins","parent":4,"id":{e},"before":null}},{{"op":"text","id":{t},"data":"row"}},{{"op":"ins","parent":{e},"id":{t},"before":null}}]"#),
        );
    }
    // A box sized by the viewport's height follows it.
    s.ops(16.0, r#"[{"op":"el","id":30,"tag":"div","attrs":[["style","position:absolute;top:0;height:100%;width:5px"]]},{"op":"ins","parent":4,"id":30,"before":null}]"#);
    s.query(0.0, 1, r#"{"q":"rect","id":30}"#);
    let Some(r) = replay(s) else { return };
    assert_eq!(r.layouts[5] - r.layouts[0], 5, "a growing document lays out once per batch");
    let tall = result(&r.outs[7]).get("height").as_number(0.0);
    assert_eq!(tall, f64::from(r.viewports[7]), "a box the viewport's height sizes matches it");
}

/// core_tests' in-place patches: random batches into a patching renderer and one that builds every time.
fn patch_rounds(patching: bool) -> Script {
    let mut setup = format!(
        r#"[{ROOTS},{{"op":"el","id":5,"tag":"style"}},{{"op":"css","id":5,"text":".a{{color:red;padding:2px}}.b{{border:1px solid blue;width:40px}}"}},{{"op":"ins","parent":3,"id":5,"before":null}}"#
    );
    for i in 0..20 {
        let (e, t) = (100 + i, 200 + i);
        setup.push_str(&format!(
            r#",{{"op":"el","id":{e},"tag":"div","attrs":[["class","a"],["style","height:10px"]]}},{{"op":"ins","parent":4,"id":{e},"before":null}},{{"op":"text","id":{t},"data":"item"}},{{"op":"ins","parent":{e},"id":{t},"before":null}}"#
        ));
    }
    setup.push(']');
    let mut s = Script::new(230);
    if !patching {
        s.after(0.0, Step::Patching(false));
    }
    s.ops(0.0, &setup);
    let mut seed: u32 = 7;
    let mut next = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        seed >> 8
    };
    for _ in 0..200 {
        let mut ops = Vec::new();
        // Mostly what a busy widget sends; now and then what has no hole (a new height, markup in text) builds.
        for _ in 0..3 {
            let (e, t) = (100 + next() % 20, 200 + next() % 20);
            let pick = next() % 20;
            ops.push(if pick < 6 {
                format!(r#"{{"op":"style","id":{e},"name":"transform","value":"translateX({}px)"}}"#, next() % 30)
            } else if pick < 11 {
                format!(r#"{{"op":"style","id":{e},"name":"background-color","value":"rgb({},0,0)"}}"#, next() % 255)
            } else if pick < 13 {
                format!(r#"{{"op":"attr","id":{e},"name":"class","value":"{}"}}"#, if next() % 2 == 1 { "a" } else { "a b" })
            } else if pick < 18 {
                format!(r#"{{"op":"data","id":{t},"data":"text {}"}}"#, next() % 1000)
            } else if pick < 19 {
                format!(r#"{{"op":"data","id":{t},"data":"text &<> {}"}}"#, next() % 1000)
            } else {
                format!(r#"{{"op":"style","id":{e},"name":"height","value":"{}px"}}"#, 5 + next() % 20)
            });
        }
        s.ops(16.0, &format!("[{}]", ops.join(",")));
        s.after(0.0, Step::LastHtml);
    }
    s
}

#[test]
fn in_place_patches() {
    let Some(runs) = replay_all(vec![patch_rounds(true), patch_rounds(false)]) else { return };
    let (patching, building) = (&runs[0], &runs[1]);
    assert_eq!(patching.html.len(), 200);
    let same = patching.html.iter().zip(&building.html).filter(|(a, b)| a == b).count();
    assert_eq!(same, 200, "patched and rebuilt documents write the same HTML");
    let first = patching.stats.iter().position(|_| true).expect("steps");
    let patched = patching.stats.last().expect("steps").in_place - patching.stats[first + 1].in_place;
    assert!(patched > 100, "most batches go into the document in place ({patched} of 200)");
    assert_eq!(building.stats.last().expect("steps").in_place, 0, "and none of the other's");
}

#[test]
fn layout_queries() {
    let mut s = Script::new(10);
    s.ops(
        0.0,
        r#"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font-size:10px;line-height:10px"]]},
        {"op":"el","id":5,"tag":"div","attrs":[["style","position:relative;margin-left:20px;padding:5px;border:1px solid red"]]},{"op":"ins","parent":4,"id":5,"before":null},
        {"op":"el","id":6,"tag":"div","attrs":[["style","margin-left:7px;width:50px;height:10px"]]},{"op":"ins","parent":5,"id":6,"before":null},
        {"op":"el","id":7,"tag":"div","attrs":[["style","width:50px;height:10px;transform:scale(2)"]]},{"op":"ins","parent":4,"id":7,"before":null},
        {"op":"el","id":8,"tag":"textarea","attrs":[["style","height:20px;padding:0;border:0;min-height:0;font-size:10px;line-height:10px"]]},{"op":"ins","parent":4,"id":8,"before":null},
        {"op":"prop","id":8,"name":"value","value":"\na\nb\nc\nd"}]"#,
    );
    for (q, id) in [("box", 6), ("rect", 7), ("box", 7), ("box", 8)] {
        s.query(0.0, 1, &format!(r#"{{"q":"{q}","id":{id}}}"#));
    }
    s.after(0.0, Step::LastHtml);
    let Some(r) = replay(s) else { return };
    let b = result(&r.outs[1]);
    assert!(
        b.get("offsetLeft").as_number(0.0) == 12.0
            && b.get("offsetTop").as_number(0.0) == 5.0
            && b.get("offsetParent").as_number(0.0) == 5.0,
        "offsets are from the positioned parent's padding edge: {}",
        b.dump_lossy()
    );
    assert!(
        result(&r.outs[2]).get("width").as_number(0.0) == 100.0 && result(&r.outs[3]).get("offsetWidth").as_number(0.0) == 50.0,
        "a transform scales the rect but not the layout size"
    );
    let area = result(&r.outs[4]);
    assert!(
        area.get("clientHeight").as_number(0.0) == 20.0 && area.get("scrollHeight").as_number(0.0) == 50.0,
        "a textarea's scrollHeight is its text's: {}",
        area.dump_lossy()
    );
    let html = &r.html[0];
    assert!(html.windows(5).any(|w| w == b"\n\na\nb"), "a textarea value starting with a newline keeps it through the parser");
}

#[test]
fn viewport_units() {
    let mut s = Script::new(10);
    s.width = 400.0;
    s.ops(
        0.0,
        &format!(
            r#"[{ROOTS},{{"op":"el","id":5,"tag":"div","attrs":[["style","width:50vw;height:10px"]]}},{{"op":"ins","parent":4,"id":5,"before":null}},
            {{"op":"el","id":6,"tag":"style"}},{{"op":"css","id":6,"text":".q{{width:25vw;height:10px}}"}},{{"op":"ins","parent":3,"id":6,"before":null}},
            {{"op":"el","id":7,"tag":"div","attrs":[["class","q"]]}},{{"op":"ins","parent":4,"id":7,"before":null}}]"#
        ),
    );
    s.query(0.0, 1, r#"{"q":"rect","id":5}"#).query(0.0, 1, r#"{"q":"rect","id":7}"#);
    s.after(16.0, Step::Width(200.0));
    s.query(0.0, 1, r#"{"q":"rect","id":5}"#).query(0.0, 1, r#"{"q":"rect","id":7}"#);
    let Some(r) = replay(s) else { return };
    let w = |i: usize| result(&r.outs[i]).get("width").as_number(0.0);
    assert!(w(1) == 200.0 && w(2) == 100.0, "50vw and 25vw of 400 are 200 and 100");
    assert_eq!(w(4), 100.0, "a resize refolds viewport units in a style attribute");
    assert_eq!(w(5), 50.0, "and in a style sheet");
}

#[test]
fn safe_area() {
    let mut s = Script::new(12);
    s.ops(
        0.0,
        r##"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},{"op":"el","id":4,"tag":"body","adopt":"body"},
        {"op":"el","id":5,"tag":"style"},{"op":"css","id":5,"text":"#own{padding-bottom:var(--tg-content-safe-area-inset-bottom,9px)}"},{"op":"ins","parent":3,"id":5,"before":null},
        {"op":"el","id":10,"tag":"div","attrs":[["id","own"],["style","height:20px"]]},{"op":"ins","parent":4,"id":10,"before":null}]"##,
    );
    let cs = |s: &mut Script, id: i64, prop: &str| {
        if prop.starts_with('-') {
            s.query(0.0, 1, &format!(r#"{{"q":"cs","id":{id},"prop":"{prop}"}}"#));
        } else {
            s.query(0.0, 1, &format!(r#"{{"q":"cs","id":{id}}}"#));
        }
    };
    cs(&mut s, 2, "--tg-content-safe-area-inset-bottom");
    cs(&mut s, 4, "padding-bottom");
    cs(&mut s, 10, "padding-bottom");
    s.message(16.0, r#"{"type":"safeArea","inset":{"top":0,"right":0,"bottom":24.567,"left":0,"cornerLeft":0,"cornerRight":300}}"#);
    cs(&mut s, 2, "--tg-content-safe-area-inset-bottom");
    cs(&mut s, 2, "--tg-content-safe-area-corner-right");
    cs(&mut s, 4, "padding-bottom");
    cs(&mut s, 10, "padding-bottom");
    s.ops(16.0, r#"[{"op":"style","id":2,"name":"--tg-content-safe-area-inset-bottom","value":"1px"}]"#);
    cs(&mut s, 2, "--tg-content-safe-area-inset-bottom");
    s.message(16.0, r#"{"type":"deviceSafeArea","inset":{"top":47,"right":0,"bottom":34,"left":0}}"#);
    cs(&mut s, 2, "--tg-safe-area-inset-top");
    cs(&mut s, 4, "padding-top");
    let Some(r) = replay(s) else { return };
    let text = |i: usize| result(&r.outs[i]).as_str().to_vec();
    let prop = |i: usize, p: &str| result(&r.outs[i]).get(p).as_str().to_vec();
    assert!(
        text(1) == b"0px" && prop(2, "padding-bottom") == b"0px" && prop(3, "padding-bottom") == b"0px",
        "before the host says otherwise the safe area is 0px, set on the root"
    );
    assert!(text(5) == b"24.57px" && text(6) == b"0px", "an inset is rounded to two decimals, a corner past 200 is 0");
    assert!(
        prop(7, "padding-bottom") == b"24.57px" && prop(8, "padding-bottom") == b"24.57px" && has(&r.outs[4], b"size"),
        "the UA layer pads the body by the band, a widget's var() reads it, and the height is reported again"
    );
    assert_eq!(text(10), b"24.57px", "the widget cannot set the safe area on the root");
    assert!(
        text(12) == b"0px" && prop(13, "padding-top") == b"0px" && !has(&r.outs[11], b"size"),
        "in a bubble the device's edges are always 0px, and they change nothing"
    );
}

/// The text an Inspect step's returned bytes hold at `at` (a u32 length, the bytes), and where the next field starts.
fn inspected_text(extra: &[u8], at: usize) -> (&[u8], usize) {
    let n = extra.get(at..at + 4).and_then(|b| b.try_into().ok()).map_or(0, |b| u32::from_le_bytes(b) as usize);
    (extra.get(at + 4..at + 4 + n).unwrap_or_default(), at + 4 + n)
}

/// An Inspect step's theme_variables_json() (after wants_tick(), next_tick_in() and master_css()) and px_height().
fn inspected_root(extra: &[u8]) -> (Json, u32) {
    let (_, at) = inspected_text(extra, 9);
    let (vars, _) = inspected_text(extra, at);
    let px = extra.len().checked_sub(4).and_then(|n| extra[n..].try_into().ok()).map_or(0, u32::from_le_bytes);
    (Json::parse(vars).unwrap_or(Json::Null), px)
}

/// The root's last `n` variables, names and values.
fn last_variables(root: &Json, n: usize) -> Vec<(String, String)> {
    let all = root.obj();
    all[all.len().saturating_sub(n)..]
        .iter()
        .map(|(k, v)| (String::from_utf8_lossy(k).into_owned(), String::from_utf8_lossy(v.as_str()).into_owned()))
        .collect()
}

/// The names `PLACE_VARIABLES` with `values`.
fn place(values: &[&str]) -> Vec<(String, String)> {
    scripts::PLACE_VARIABLES.iter().zip(values).map(|(n, v)| (n.to_string(), v.to_string())).collect()
}

/// The body's computed padding, top, right, bottom and left, in a `cs` answer.
fn padding(body: &Json) -> Vec<String> {
    ["top", "right", "bottom", "left"]
        .iter()
        .map(|side| String::from_utf8_lossy(body.get(format!("padding-{side}")).as_str()).into_owned())
        .collect()
}

#[test]
fn profile_reference_page() {
    let m = scripts::reference_profile();
    let Some(r) = replay(m.script.clone()) else { return };
    let answer = |name: &str| result(&r.outs[m.at(name)]);
    let reference = ["47px", "0px", "34px", "0px", "56px", "0px", "64px", "0px", "0px", "0px", "844px"];
    let (root, _) = inspected_root(&r.extras[m.at("start inspect")]);
    assert_eq!(
        last_variables(&root, 11),
        place(&reference),
        "before any ops the root ends with the device's edges, the client's chrome and the viewport, in renderer.cpp's order"
    );
    let sides = ["47px", "13px", "34px", "11px", "56px", "7px", "64px", "5px", "0px", "0px", "844px"];
    let sides_padding = ["103px", "20px", "98px", "16px"];
    let sides_widths = [103.0, 98.0, 16.0, 113.0, 20.0, 47.0, 34.0, 56.0];
    for (state, values, body, widths) in [
        ("reference", reference, ["103px", "0px", "98px", "0px"], [103.0, 98.0, 0.0, 113.0, 4.0, 47.0, 34.0, 56.0]),
        ("sides", sides, sides_padding, sides_widths),
        ("theme", sides, sides_padding, sides_widths),
        ("overwritten", sides, sides_padding, sides_widths),
    ] {
        let got: Vec<(String, String)> = scripts::PLACE_VARIABLES
            .iter()
            .map(|n| (n.to_string(), String::from_utf8_lossy(answer(&format!("{state} {n}")).as_str()).into_owned()))
            .collect();
        assert_eq!(got, place(&values), "{state}: the root's place variables are the host's, a page's corners 0");
        assert_eq!(
            padding(&answer(&format!("{state} body"))),
            body,
            "{state}: the UA layer pads the unstyled body by the sum of both layers"
        );
        let got: Vec<f64> = (1..=8).map(|e| answer(&format!("{state} e{e}")).get("width").as_number(-1.0)).collect();
        assert_eq!(got, widths, "{state}: auto/46's probes e1–e8, env() the sum and var() one layer");
        assert_eq!(
            answer(&format!("{state} viewport")).get("innerHeight").as_number(0.0),
            844.0,
            "{state}: the short page is its viewport tall"
        );
    }
    let load = sizes(&r.outs[m.at("load")]);
    assert!(
        load.len() == 1 && load[0].0 < 844.0 && load[0].1 == 844.0,
        "the short page reports its own height and the viewport's: {load:?}"
    );
}

#[test]
fn profile_page() {
    let m = scripts::profile_page();
    let Some(r) = replay(m.script.clone()) else { return };
    let answer = |name: &str| result(&r.outs[m.at(name)]);
    let text = |name: &str| answer(name).as_str().to_vec();
    let surface = sizes(&r.outs[m.at("surface")]);
    assert!(surface.len() == 1 && surface[0].1 >= 844.57, "a page is at least its viewport tall: {surface:?}");
    assert!(
        text("page --tg-viewport-height") == b"844.57px" && answer("hero").get("height").as_str() == b"844.57px",
        "a page sets --tg-viewport-height at two decimals"
    );
    assert!(
        text("page --tg-safe-area-inset-top") == b"47px"
            && text("page --tg-safe-area-inset-right") == b"0px"
            && text("page --tg-content-safe-area-inset-bottom") == b"120px"
            && text("page --tg-content-safe-area-corner-right") == b"0px",
        "on a page the device's edges are set, a negative one is 0, a content side past 64 stays and a corner is 0"
    );
    let body = answer("body");
    assert!(
        body.get("padding-top").as_str() == b"103px" && body.get("padding-bottom").as_str() == b"154px",
        "the UA layer pads the body by the sum of both layers: {}",
        body.dump_lossy()
    );
    assert!(
        text("overwritten --tg-safe-area-inset-top") == b"47px" && text("overwritten --tg-viewport-height") == b"844.57px",
        "the widget cannot set the device's edges or the viewport on the root"
    );
    let tall = sizes(&r.outs[m.at("tall")]);
    assert!(
        tall.len() == 1 && tall[0].0 > 9000.0 && tall[0].1 == 4000.0 && sizes(&r.outs[m.at("tall tick")]).is_empty(),
        "a page is clipped at the 4000 px ceiling, its own height reported whole: {tall:?}"
    );
    let (_, px) = inspected_root(&r.extras[m.at("tall inspect")]);
    assert!(
        px == 4000 && answer("tall viewport").get("innerHeight").as_number(0.0) == 4000.0,
        "and so are its pixels and its viewport ({px})"
    );
}

#[test]
fn profile_heights() {
    let m = scripts::profile_heights();
    let Some(r) = replay(m.script.clone()) else { return };
    let answer = |name: &str| result(&r.outs[m.at(name)]);
    let inner = |name: &str| answer(name).get("innerHeight").as_number(0.0);
    let px = |name: &str| inspected_root(&r.extras[m.at(name)]).1;
    let reported = |name: &str| sizes(&r.outs[m.at(name)]);
    assert!(
        reported("short") == [(522.0, 844.0)]
            && answer("short v").get("height").as_number(0.0) == 422.0
            && inner("short viewport") == 844.0
            && px("short inspect") == 844,
        "a short page is laid out at its viewport's height and reports its own: {:?}",
        reported("short")
    );
    assert!(
        reported("midrange") == [(2000.0, 2000.0)] && inner("midrange viewport") == 2000.0 && px("midrange inspect") == 2000,
        "a taller one is its own height: {:?}",
        reported("midrange")
    );
    assert!(
        reported("clipped") == [(9000.0, 4000.0)]
            && reported("clipped tick").is_empty()
            && inner("clipped viewport") == 4000.0
            && px("clipped inspect") == 4000,
        "past the ceiling it is cut off at 4000 px, its own height reported whole: {:?}",
        reported("clipped")
    );
    assert!(
        r.viewports[m.at("lowered")] == 844 && reported("lowered") == [(9000.0, 844.0)],
        "a host's bound below the viewport cannot take a page below it: {:?}",
        reported("lowered")
    );
    assert_eq!(r.viewports[m.at("between")], 3000, "one between the viewport and the ceiling holds");
}

#[test]
fn profile_without_a_viewport() {
    let m = scripts::profile_without_viewport();
    let Some(r) = replay(m.script.clone()) else { return };
    let answer = |name: &str| result(&r.outs[m.at(name)]);
    for (state, var, f, inner) in [
        ("none", "", 30.0, 50.0),
        ("fraction", "844.57px", 844.57, 845.0),
        ("ceiling", "4000px", 4000.0, 4000.0),
        ("string", "", 30.0, 50.0),
        ("named", "", 30.0, 50.0),
    ] {
        let got = (
            String::from_utf8_lossy(answer(&format!("{state} var")).as_str()).into_owned(),
            answer(&format!("{state} f")).get("height").as_number(0.0),
            answer(&format!("{state} viewport")).get("innerHeight").as_number(0.0),
        );
        assert!(
            got.0 == var && (got.1 - f).abs() < 0.01 && got.2 == inner,
            "{state}: --tg-viewport-height {var:?}, #f {f}, the viewport {inner} (got {got:?})"
        );
    }
}

#[test]
fn profile_relayout() {
    let m = scripts::profile_relayout();
    let Some(r) = replay(m.script.clone()) else { return };
    let laid_out = |name: &str| {
        let at = m.at(name);
        r.layouts[at] - r.layouts[at - 1]
    };
    let reported = |name: &str| {
        let mut all = sizes(&r.outs[m.at(name)]);
        all.extend(sizes(&r.outs[m.at(&format!("{name} tick"))]));
        all
    };
    assert_eq!(sizes(&r.outs[m.at("load")]), [(1000.0, 1000.0)], "a page taller than its viewport is its own height");
    for (name, size) in [("device", (1020.0, 1020.0)), ("content", (1050.0, 1050.0)), ("viewport", (1050.0, 1500.0))] {
        assert!(
            laid_out(name) > 0 && reported(name) == [size],
            "{name}: laid out again ({} layouts) and its new size reported once: {:?}",
            laid_out(name),
            reported(name)
        );
        let again = format!("{name} again");
        assert!(
            laid_out(&again) == 0 && reported(&again).is_empty(),
            "{again}: the same input at the same bounds lays out nothing and reports nothing: {:?}",
            reported(&again)
        );
    }
    let (before, after) = (result(&r.outs[m.at("before sides")]), result(&r.outs[m.at("after sides")]));
    let at = |j: &Json, k: &str| j.get(k).as_number(-1.0);
    assert!(
        laid_out("sides") > 0
            && reported("sides").is_empty()
            && at(&before, "x") == 0.0
            && at(&after, "x") == 10.0
            && at(&after, "width") == at(&before, "width") - 10.0,
        "a left edge lays the page out again without a size report while no reported number moves: {} / {}",
        before.dump_lossy(),
        after.dump_lossy()
    );
}

#[test]
fn bubble_place() {
    let m = scripts::bubble_place();
    let Some(r) = replay(m.script.clone()) else { return };
    let answer = |name: &str| result(&r.outs[m.at(name)]);
    let text = |name: &str| String::from_utf8_lossy(answer(name).as_str()).into_owned();
    let laid_out = |name: &str| {
        let at = m.at(name);
        r.layouts[at] - r.layouts[at - 1]
    };
    let (root, _) = inspected_root(&r.extras[m.at("start inspect")]);
    let chrome = ["0px", "0px", "0px", "0px", "0px", "0px", "24px", "0px", "16px", "20px"];
    assert!(
        last_variables(&root, 10) == place(&chrome) && root.find("--tg-viewport-height").is_none(),
        "a bubble's root: the device's edges 0px whatever the host says, the client's chrome, no viewport: {}",
        root.dump_lossy()
    );
    for state in ["load", "side"] {
        let got: Vec<String> = scripts::PLACE_VARIABLES[..4].iter().map(|n| text(&format!("{state} {n}"))).collect();
        assert_eq!(got, ["0px"; 4], "{state}: the device's edges are 0px in a bubble");
        assert!(
            text(&format!("{state} --tg-viewport-height")).is_empty() && answer(&format!("{state} f")).get("height").as_number(0.0) == 5.0,
            "{state}: no viewport variable in a bubble, the widget's fallback applies"
        );
        assert_eq!(answer(&format!("{state} viewport")).get("innerHeight").as_number(0.0), 50.0, "{state}: a short bubble is 50 px tall");
    }
    assert_eq!(padding(&answer("load body")), ["0px", "0px", "24px", "0px"], "the client's bottom band pads the body's bottom alone");
    assert_eq!(padding(&answer("side body")), ["0px", "10px", "24px", "0px"], "and a side within its range that side once");
    for name in ["device", "surface"] {
        assert!(laid_out(name) == 0 && sizes(&r.outs[m.at(name)]).is_empty(), "{name}: nothing changes in a bubble");
    }
    assert!(sizes(&r.outs[m.at("inert tick")]).is_empty(), "not at the frame after either");
    assert_eq!(r.viewports[m.at("tall")], 600, "a tall bubble stops at 600 px");
    let over = sizes(&r.outs[m.at("override")]);
    assert!(
        over.len() == 1 && over[0].1 == over[0].0 && over[0].1 > 600.0,
        "and the host's own bound lets it grow to its content: {over:?}"
    );
}

#[test]
fn measure_text_in_the_canvas_font() {
    let mut s = Script::new(40);
    s.ops(
        0.0,
        r#"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
        {"op":"el","id":4,"tag":"body","adopt":"body"},{"op":"el","id":30,"tag":"canvas","attrs":[["width","50"],["height","20"]]},{"op":"ins","parent":4,"id":30,"before":null},
        {"op":"canvas","id":30,"cmds":[["s","font","bold 12px serif"],["c","fillText","hi",0,10]]}]"#,
    );
    s.query(0.0, 1, r#"{"q":"measureText","font":"bold 12px serif","text":"hi"}"#);
    let Some(r) = replay(s) else { return };
    let drawn = r
        .stub
        .runs
        .iter()
        .flat_map(|(_, ops)| ops.iter())
        .filter(|op| op.kind == twidget_wire::OpKind::Text)
        .map(|op| op.font)
        .next_back()
        .unwrap_or(0);
    let measured: Vec<i32> = r.stub.measured_in.clone();
    assert!(drawn != 0 && measured.last() == Some(&drawn), "measureText measures in the font fillText drew with ({drawn}, {measured:?})");
}

#[test]
fn a_build_in_paint_waits_for_the_next_tick() {
    let mut s = Script::new(20);
    s.ops(
        0.0,
        &format!(
            r#"[{ROOTS},{{"op":"el","id":5,"tag":"style"}},{{"op":"css","id":5,"text":"@keyframes f{{from{{opacity:0}}to{{opacity:1}}}} .a{{animation:f 0.05s}}"}},{{"op":"ins","parent":3,"id":5,"before":null}},
            {{"op":"el","id":10,"tag":"div","attrs":[["class","a"],["style","height:20px"]]}},{{"op":"ins","parent":4,"id":10,"before":null}}]"#
        ),
    );
    s.after(0.0, Step::Tick);
    // While the animation runs, a batch waits for the frame: its build is the next paint's.
    s.ops(0.0, r#"[{"op":"el","id":11,"tag":"div","attrs":[["style","height:100px"]]},{"op":"ins","parent":4,"id":11,"before":null}]"#);
    let batch = s.steps.len() - 1;
    s.after(120.0, Step::Paint);
    s.after(0.0, Step::Inspect);
    let inspect = s.steps.len() - 1;
    s.after(0.0, Step::Tick);
    let tick = s.steps.len() - 1;
    let Some(r) = replay(s) else { return };
    let (waits, due) = inspected(&r.extras[inspect]);
    assert!(
        !has(&r.outs[batch], b"size") && waits && due == 0.0 && has(&r.outs[tick], b"size"),
        "a size from a build in paint() keeps wants_tick() until the next tick hands it out"
    );
}

#[test]
fn resize_diffs() {
    let run = |grow: i64, press_y: f64| {
        let mut s = Script::new(12);
        s.after(0.0, Step::ResizeTracking { enabled: true, prefers_bottom: false });
        s.ops(
            0.0,
            &format!(
                r#"[{ROOTS},{{"op":"el","id":10,"tag":"div","attrs":[["style","height:50px"]]}},{{"op":"ins","parent":4,"id":10,"before":null}},
                {{"op":"el","id":11,"tag":"div","attrs":[["style","height:50px"]]}},{{"op":"ins","parent":4,"id":11,"before":null}}]"#
            ),
        );
        s.after(16.0, Step::Paint);
        if press_y >= 0.0 {
            s.touch(16.0, 0, 10.0, press_y, 1, false);
            s.touch(16.0, 2, 10.0, press_y, 1, false);
        }
        s.ops(16.0, &format!(r#"[{{"op":"style","id":{grow},"name":"height","value":"80px"}}]"#));
        s.after(16.0, Step::Paint);
        s.after(0.0, Step::Tick);
        s
    };
    let Some(runs) = replay_all(vec![run(11, -1.0), run(10, -1.0), run(10, 75.0), run(10, 25.0)]) else { return };
    let diff = |r: &Run| -> Json {
        r.outs
            .iter()
            .flatten()
            .filter(|m| m.get("type").as_str() == b"size")
            .filter_map(|m| m.find("resize").cloned())
            .next_back()
            .unwrap_or(Json::Null)
    };
    let below = diff(&runs[0]);
    assert!(
        below.get("oldHeight").as_number(0.0) == 100.0
            && below.get("newHeight").as_number(0.0) == 130.0
            && below.get("unchangedTop").as_number(0.0) == 100.0
            && !below.get("keepsBottom").as_bool(true),
        "growing at the bottom keeps the top: {}",
        below.dump_lossy()
    );
    let above = diff(&runs[1]);
    assert!(
        above.get("unchangedTop").as_number(0.0) == 50.0
            && above.get("unchangedBottom").as_number(0.0) == 100.0
            && above.get("keepsBottom").as_bool(false),
        "growing at the top keeps the bottom: {}",
        above.dump_lossy()
    );
    let pressed_below = diff(&runs[2]);
    assert!(
        pressed_below.get("pointerTo").as_number(0.0) - pressed_below.get("pointerFrom").as_number(0.0) == 30.0
            && pressed_below.get("keepsBottom").as_bool(false),
        "a press below what grew keeps the pressed element still: {}",
        pressed_below.dump_lossy()
    );
    let pressed_on = diff(&runs[3]);
    assert!(
        pressed_on.find("pointerTo").is_some()
            && pressed_on.get("pointerTo").as_number(1.0) == pressed_on.get("pointerFrom").as_number(0.0)
            && !pressed_on.get("keepsBottom").as_bool(true),
        "a press on what grew keeps its top still: {}",
        pressed_on.dump_lossy()
    );
}

#[test]
fn a_long_flush_in_parts() {
    let mut s = Script::new(30);
    s.ops(0.0, PAGE);
    let part: Vec<String> = (0..1000).map(|i| format!(r#"{{"op":"style","id":10,"name":"width","value":"{}px"}}"#, i % 90 + 1)).collect();
    s.ops(16.0, &format!("[{}]", part.join(",")));
    s.ops(0.0, r#"[{"op":"style","id":10,"name":"width","value":"70px"}]"#);
    s.after(0.0, Step::Inspect);
    s.after(16.0, Step::Tick);
    s.query(0.0, 1, r#"{"q":"rect","id":10}"#);
    let Some(r) = replay(s) else { return };
    let builds = r.stats[0].builds;
    assert_eq!(r.stats[1].builds, builds, "a part of 1000 ops waits for the rest of its task");
    assert!(r.stats[2].builds == builds && inspected(&r.extras[3]).0, "and so does the last, to the frame");
    assert!(r.stats[4].builds - builds == 1 && result(&r.outs[5]).get("width").as_number(0.0) == 70.0, "which builds the task once");
}

#[test]
fn a_hosts_sizes_that_are_no_sizes() {
    let mut s = Script::new(12);
    s.width = f64::NAN;
    s.dpr = -3.0;
    s.ops(0.0, &format!(r#"[{ROOTS},{{"op":"el","id":10,"tag":"div","attrs":[["style","height:5000px"]]}},{{"op":"ins","parent":4,"id":10,"before":null}}]"#));
    s.after(0.0, Step::Inspect);
    s.after(0.0, Step::Width(-300.0));
    s.after(0.0, Step::Inspect);
    s.after(0.0, Step::Width(1e30));
    s.after(0.0, Step::Inspect);
    s.after(0.0, Step::Width(300.0));
    s.after(0.0, Step::MaxHeight(f64::NAN));
    s.after(0.0, Step::Paint);
    s.after(0.0, Step::MaxHeight(f64::INFINITY));
    s.after(0.0, Step::Paint);
    s.after(0.0, Step::Inspect);
    let Some(r) = replay(s) else { return };
    let px = |extra: &[u8]| {
        let n = extra.len();
        (u32::from_le_bytes(extra[n - 8..n - 4].try_into().expect("four")), u32::from_le_bytes(extra[n - 4..].try_into().expect("four")))
    };
    let (w, h) = px(&r.extras[1]);
    assert!(w == 0 && h > 0, "a width that is no number is no width, and a scale below any screen's is the least one");
    assert_eq!(px(&r.extras[3]).0, 0, "a negative width is none");
    assert_eq!(px(&r.extras[5]).0, 262144, "a width past any view is the widest one, at the least scale");
    assert_eq!(r.viewports[8], 50, "a height limit that is no number is the least one");
    assert!(r.viewports[10] == 5000 && px(&r.extras[11]).1 > 0, "and an endless one the ceiling's");
}

#[test]
fn names_stop_a_widget_that_keeps_inventing_them() {
    let page = r##"[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
        {"op":"el","id":20,"tag":"style","attrs":[]},{"op":"css","id":20,"text":"#a{width:100px;height:10px;--accent:red}"},{"op":"ins","parent":3,"id":20,"before":null},
        {"op":"el","id":10,"tag":"div","attrs":[["id","a"]]},{"op":"ins","parent":4,"id":10,"before":null}]"##;
    // Each in a process of its own, as each core_tests renderer met a name table without these names.
    let stopped_after = |tag: &str, classes: usize, length: usize, properties: usize| -> Option<(usize, Vec<u8>)> {
        let mut s = Script::new(30);
        s.ops(0.0, page);
        s.after(16.0, Step::Tick);
        let mut rounds = Vec::new();
        for round in 1..=12 {
            let mut list = String::new();
            let mut sheet = String::new();
            for i in 0..classes {
                let name = format!("{tag}{round}c{i}");
                list.push_str(&name);
                list.push_str(&"x".repeat(length.saturating_sub(name.len())));
                list.push(' ');
            }
            for i in 0..properties {
                sheet.push_str(&format!("--{tag}{round}p{i}:1;"));
            }
            s.ops(16.0, &format!(r##"[{{"op":"attr","id":10,"name":"class","value":"{list}"}},{{"op":"css","id":20,"text":"#a{{width:100px;height:10px;{sheet}}}"}}]"##));
            s.after(16.0, Step::Tick);
            rounds.push(s.steps.len() - 1);
        }
        s.ops(16.0, r#"[{"op":"attr","id":10,"name":"class","value":"after-the-stop"}]"#);
        s.after(16.0, Step::Tick);
        s.after(16.0, Step::LastHtml);
        let r = replay(s)?;
        let violation = |out: &[Json]| {
            out.iter().any(|m| m.get("type").as_str() == b"violation" && m.get("detail").as_str().starts_with(b"name limit"))
        };
        let stopped = rounds.iter().position(|&tick| violation(&r.outs[tick - 1]) || violation(&r.outs[tick])).map_or(0, |i| i + 1);
        Some((stopped, r.html.last().cloned().unwrap_or_default()))
    };
    let Some((by_count, many)) = stopped_after("nm", 2500, 0, 500) else { return };
    let Some((by_bytes, long_ones)) = stopped_after("nl", 100, 4000, 0) else { return };
    let contains = |h: &[u8], n: &[u8]| h.windows(n.len()).any(|w| w == n);
    assert!(
        by_count == 7
            && by_bytes == 3
            && contains(&many, b"data-tw-badge=\"fatal\"")
            && !contains(&many, b"after-the-stop")
            && contains(&long_ones, b"data-tw-badge=\"fatal\""),
        "a widget that invents 3000 names, or 400 K of names, a batch is stopped past 20000 names or 1 MB, and its names reach the document no more (stopped after {by_count} and {by_bytes} batches)"
    );
}

#[test]
fn hostile_markup_nested_layout_is_bounded() {
    let mut s = Script::new(80);
    let mut ops = vec![ROOTS.to_string()];
    let mut parent = 4;
    for i in 0..40 {
        let id = 10 + i;
        ops.push(format!(r#"{{"op":"el","id":{id},"tag":"div","attrs":[["style","display:flex"]]}},{{"op":"ins","parent":{parent},"id":{id},"before":null}}"#));
        parent = id;
    }
    s.ops(0.0, &format!("[{}]", ops.join(",")));
    s.after(16.0, Step::Tick);
    s.ops(16.0, r#"[{"op":"style","id":10,"name":"color","value":"red"}]"#);
    s.after(16.0, Step::LastHtml);
    let Some(r) = replay(s) else { return };
    let violations: Vec<&Json> = r.outs[0].iter().filter(|m| m.get("type").as_str() == b"violation").collect();
    assert!(
        violations.iter().any(|m| m.get("detail").as_str().starts_with(b"layout limit: more than ")),
        "forty nested flex boxes run past the layout budget, which the host hears of"
    );
    let (frozen, reason, stopped) = &r.frozen[0];
    assert!(*frozen && reason == b"Layout limit exceeded" && *stopped, "and the widget is frozen with the badge alone");
    assert!(r.outs[2].is_empty(), "its ops are refused after the stop");
    let html = &r.html[0];
    assert!(
        html.starts_with(b"<!DOCTYPE html><html data-tw-frozen=\"fatal\">") && html.windows(22).any(|w| w == b"Layout limit exceeded<"),
        "the document is the badge alone"
    );
}

#[test]
fn frozen_paused_and_fatal() {
    let mut s = Script::new(30);
    s.ops(0.0, PAGE);
    s.message(16.0, r#"{"type":"frozen","reason":"Paused <b>","fatal":false}"#);
    s.after(0.0, Step::LastHtml);
    s.ops(16.0, r#"[{"op":"style","id":10,"name":"width","value":"70px"}]"#);
    s.query(0.0, 1, r#"{"q":"rect","id":10}"#);
    s.message(16.0, r#"{"type":"frozen","reason":"Stopped","fatal":true}"#);
    s.after(0.0, Step::LastHtml);
    s.after(0.0, Step::Inspect);
    let Some(r) = replay(s) else { return };
    let paused = String::from_utf8_lossy(&r.html[0]).into_owned();
    assert!(
        paused.contains("data-tw-frozen=\"paused\"") && paused.contains("<div data-tw-badge=\"paused\">Paused &lt;b&gt;</div></body>"),
        "a paused widget wears its badge at the end of the body: {paused}"
    );
    assert_eq!(result(&r.outs[4]).get("width").as_number(0.0), 70.0, "and still takes its ops");
    let fatal = String::from_utf8_lossy(&r.html[1]).into_owned();
    assert!(fatal.contains("data-tw-frozen=\"paused\""), "a second freeze changes nothing: {fatal}");
    assert!(!inspected(&r.extras[7]).0 && inspected(&r.extras[7]).1 == f64::INFINITY, "a frozen widget wants no frame");
}

#[test]
fn every_query_kind_answers() {
    let mut r = twidget_testkit::Lcg::new(0x4310);
    let (ops, max_id) = common::scripts::page(&mut r);
    let mut s = Script::new(max_id);
    s.after(0.0, Step::Ops(common::inputs::document(&ops).into_bytes()));
    let queries = [
        r#"{"q":"rect","id":10}"#,
        r#"{"q":"rects","id":30}"#,
        r#"{"q":"box","id":41}"#,
        r#"{"q":"cs","id":10}"#,
        r#"{"q":"cs","id":10,"prop":"--none"}"#,
        r#"{"q":"elementFromPoint","x":5,"y":5}"#,
        r#"{"q":"measureText","font":"10px serif","text":"abc"}"#,
        r#"{"q":"validity","id":32}"#,
        r#"{"q":"matchMedia","query":"(max-width: 500px)"}"#,
        r#"{"q":"viewport"}"#,
        r#"{"q":"anims","id":4,"subtree":true}"#,
        r#"{"q":"imageData","id":33,"x":0,"y":0,"w":2,"h":2}"#,
        r#"{"q":"imageData","id":33,"x":0,"y":0,"w":8192,"h":8192}"#,
        r#"{"q":"canvasCall","id":33,"method":"getTransform","args":[]}"#,
        r#"{"q":"canvasCall","id":10,"method":"getTransform","args":[]}"#,
        r#"{"q":"canvasCall","id":33,"method":"toDataURL","args":[]}"#,
        r#"{"q":"svg","id":35,"method":"getBBox","args":[]}"#,
        r#"{"q":"svg","id":36,"method":"getTotalLength","args":[]}"#,
        r#"{"q":"svg","id":36,"method":"getPointAtLength","args":[10]}"#,
        r#"{"q":"svg","id":37,"method":"getComputedTextLength","args":[]}"#,
        r#"{"q":"rect","id":999}"#,
        r#"{"q":"nope"}"#,
    ];
    for q in queries {
        s.query(0.0, 5, q);
    }
    s.message(0.0, r#"{"type":"query","qid":5}"#);
    let Some(run) = replay(s) else { return };
    let answers: Vec<Json> = run.outs[1..].iter().map(|out| result(out)).collect();
    let error = |i: usize| answers[i].get("error").as_str().to_vec();
    assert!(answers[0].get("width").as_number(0.0) > 0.0 && answers[0].find("bottom").is_some(), "rect");
    assert!(!answers[1].arr().is_empty(), "rects: a line box per line");
    assert!(answers[2].find("scrollHeight").is_some() && answers[2].find("offsetParent").is_some(), "box");
    assert!(answers[3].find("width").is_some(), "cs");
    assert_eq!(answers[4].as_str(), b"", "cs of a custom property nobody declared");
    assert!(answers[5].is_number(), "elementFromPoint");
    assert_eq!(answers[6].get("width").as_number(0.0), 24.0, "measureText");
    assert!(answers[7].get("valid").as_bool(false), "validity");
    assert!(answers[8].get("matches").as_bool(false), "matchMedia");
    assert_eq!(answers[9].get("innerWidth").as_number(0.0), 300.0, "viewport");
    assert!(answers[10].get("list").is_array(), "anims");
    assert_eq!(answers[11].get("width").as_number(0.0), 2.0, "imageData");
    assert!(run.outs[12].iter().any(|m| m.get("binaryBase64").as_str() == b"AAAAAAAAAAAAAAAAAAAAAA=="), "imageData's pixels, base64");
    assert_eq!(error(12), b"too large", "imageData past the canvas's pixels");
    assert!(answers[13].is_array() && answers[14].arr().len() == 6, "canvasCall's transform, with a canvas or without one");
    assert_eq!(error(15), b"toDataURL unsupported by the litehtml renderer");
    assert!(
        answers[16].find("width").is_some() && answers[17].is_number() && answers[18].find("x").is_some() && answers[19].is_number(),
        "svg"
    );
    assert_eq!(error(20), b"no such element");
    assert_eq!(error(21), b"unknown query");
    assert_eq!(error(22), b"no query");
}

// ---- The port's own, on a document that is never built ----------------------------------------------------------

/// An engine with no document: every build makes none, every element is missing.
#[derive(Debug, Default)]
struct NoDocument;

impl Engine for NoDocument {
    fn measure_words(&mut self) {}
    fn has_viewport_units(&mut self, _html: &[u8]) -> bool {
        false
    }
    fn update_from_string(&mut self, _html: &[u8]) -> Option<bool> {
        None
    }
    fn media_changed(&mut self) {}
    fn last_patch_paint_only(&mut self) -> bool {
        false
    }
    fn create(&mut self, _html: &[u8], _master: &[u8]) -> bool {
        false
    }
    fn drop_document(&mut self) {}
    fn element_count(&mut self) -> u64 {
        0
    }
    fn render(&mut self, _width: f32, _budget: u64) {}
    fn layout_exhausted(&mut self) -> bool {
        false
    }
    fn layout_spent(&mut self) -> u64 {
        0
    }
    fn index(&mut self) {}
    fn height(&mut self) -> f64 {
        0.0
    }
    fn has_root(&mut self) -> bool {
        false
    }
    fn layout_reads_viewport_height(&mut self) -> bool {
        false
    }
    fn font_count(&mut self) -> usize {
        0
    }
    fn update_holes(&mut self, _changes: &[HoleChange]) -> Option<bool> {
        None
    }
    fn update_styles(&mut self, _patches: &[(i64, Vec<u8>)]) -> Option<bool> {
        None
    }
    fn name_usage(&mut self) -> (u64, u64) {
        (0, 0)
    }
    fn trim(&mut self) {}
    fn set_no_effects(&mut self) {}
    fn take_violations(&mut self) -> Vec<Vec<u8>> {
        Vec::new()
    }
    fn has_element(&mut self, _id: i64) -> bool {
        false
    }
    fn declarations(&mut self, _names: &[Vec<u8>], _gates: &[Vec<u8>], _always: &[i64]) -> (BTreeMap<i64, Declared>, u64) {
        (BTreeMap::new(), 0)
    }
    fn edges(&mut self) -> Vec<(i64, Rect)> {
        Vec::new()
    }
    fn visual_box(&mut self, _id: i64) -> Rect {
        Rect::default()
    }
    fn inline_boxes(&mut self, _id: i64) -> Vec<Rect> {
        Vec::new()
    }
    fn transformed_box(&mut self, _id: i64, r: Rect) -> Rect {
        r
    }
    fn border_box(&mut self, _id: i64) -> Rect {
        Rect::default()
    }
    fn borders(&mut self, _id: i64) -> Option<[f32; 4]> {
        None
    }
    fn offsets(&mut self, _id: i64) -> (f64, f64, i64) {
        (0.0, 0.0, 0)
    }
    fn scroll_extent(&mut self, _id: i64) -> ScrollExtent {
        ScrollExtent::default()
    }
    fn textarea_content(&mut self, _id: i64) -> Option<TextContent> {
        None
    }
    fn computed_style(&mut self, _id: i64, _pseudo: &[u8], _animated: &StyleValues) -> Json {
        Json::object()
    }
    fn custom_property(&mut self, _id: i64, _name: &[u8]) -> Vec<u8> {
        Vec::new()
    }
    fn svg_text_box(&mut self, _id: i64) -> Option<[f32; 4]> {
        None
    }
    fn svg_geometry(&mut self, _id: i64) -> bool {
        false
    }
    fn svg_path_length(&mut self, _id: i64, _at: f32) -> (f32, f32, f32) {
        (0.0, 0.0, 0.0)
    }
    fn svg_path_bounds(&mut self, _id: i64) -> Option<[f32; 4]> {
        None
    }
    fn hit(&mut self, _x: f64, _y: f64, _placement: Option<&mut Rect>) -> i64 {
        0
    }
    fn box_of(&mut self, _id: i64) -> Rect {
        Rect::default()
    }
    fn touch_action(&mut self, _id: i64) -> Option<Vec<u8>> {
        None
    }
    fn user_select_text(&mut self, _id: i64) -> bool {
        false
    }
    fn mouse(&mut self, _action: i32, _x: f64, _y: f64, _hovers: bool) -> bool {
        false
    }
    fn leave(&mut self) -> bool {
        false
    }
    fn cursor(&mut self) -> Vec<u8> {
        Vec::new()
    }
    fn editor_style(&mut self, _id: i64, _clip: Rect) -> Option<EditorStyle> {
        None
    }
    fn image_of(&mut self, _src: &[u8]) -> Option<(i32, f32, f32)> {
        None
    }
    fn image_ready(&mut self, _id: i32, _w: f32, _h: f32) -> bool {
        false
    }
    fn content_size(&mut self, _id: i64) -> (f32, f32) {
        (0.0, 0.0)
    }
    fn runs_key(&mut self) -> [u64; 4] {
        [0; 4]
    }
    fn text_runs(&mut self) -> Vec<TextRun> {
        Vec::new()
    }
    fn text_width(&mut self, _text: &[u8], _font: u64) -> f32 {
        0.0
    }
    fn font_info(&mut self, _font: u64) -> Option<(i32, f32, f32, f32)> {
        None
    }
    fn selectable_key(&mut self) -> [u64; 2] {
        [0; 2]
    }
    fn any_selectable(&mut self) -> bool {
        false
    }
    fn boxes(&mut self) -> Vec<(i64, bool, BoxStyle)> {
        Vec::new()
    }
    fn box_style(&mut self, _id: i64) -> BoxStyle {
        BoxStyle::default()
    }
    fn background_is_local(&mut self, _id: i64) -> bool {
        false
    }
    type Elements = ();
    fn elements(&mut self, _ids: impl IntoIterator<Item = i64>) {}
    fn insert_element(&mut self, _set: &mut (), _id: i64) {}
    fn layer_candidate(&mut self, _id: i64, _movers: Option<&()>) -> Option<Rect> {
        None
    }
    fn clip_of(&mut self, _id: i64) -> Option<Clip> {
        None
    }
    fn background_tile(&mut self, _id: i64) -> Option<Tile> {
        None
    }
    fn declared(&mut self, _id: i64, _names: &[&str]) -> Declared {
        Declared::new()
    }
    fn paint_order(&mut self) {}
    fn paint_index(&mut self, _id: i64) -> usize {
        0
    }
    fn covered(&mut self, _id: i64, _area: Rect, _ignore: &()) -> bool {
        true
    }
    fn obscurers(&mut self, _id: i64, _area: Rect, _ignore: &(), _limit: usize) -> Option<Vec<i64>> {
        None
    }
    fn draw(&mut self) -> DisplayList {
        DisplayList::new()
    }
    fn draw_skipping(&mut self, _skip: &[i64], _backgrounds: &[i64]) -> DisplayList {
        DisplayList::new()
    }
    fn draw_layer(&mut self, _id: i64, _skip: &[i64]) -> (DisplayList, Rect) {
        (DisplayList::new(), Rect::default())
    }
    fn paint_key(&mut self) -> (u64, u32) {
        (0, 0)
    }
    fn hit_within(&mut self, _id: i64, _x: f32, _y: f32, _placement: Option<&mut Rect>) -> i64 {
        0
    }
}

fn parse(text: &str) -> Json {
    Json::parse(text).expect("JSON")
}

#[test]
fn canvases_and_canvas_fonts_released_in_order() {
    let nodes = [70i64, 31, 95, 12, 58];
    let mut page = format!("[{ROOTS}");
    for id in nodes {
        page.push_str(&format!(r#",{{"op":"el","id":{id},"tag":"canvas","attrs":[["width","40"],["height","20"]]}},{{"op":"ins","parent":4,"id":{id},"before":null}}"#));
    }
    page.push(']');
    let draw = |id: i64, from: i32, count: i32| {
        let mut cmds = String::from(r#"["s","textBaseline","top"]"#);
        for i in from..from + count {
            cmds.push_str(&format!(r#",["s","font","{i}px serif"],["c","fillText","hi",0,10]"#));
        }
        let mut m = Json::object();
        m.set("type", "ops").set("ops", parse(&format!(r#"[{{"op":"canvas","id":{id},"cmds":[{cmds}]}}]"#)));
        m
    };
    let mut ops = Json::object();
    ops.set("type", "ops").set("ops", parse(&page));
    let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut r = Renderer::new(300.0, 1.0, false, NoDocument, Recorder { next: 0, calls: calls.clone() }, None);
    r.receive(&ops);
    r.receive(&draw(95, 30, 40));
    r.receive(&draw(12, 1, 25));
    let first: Vec<i32> = (1..=65).collect();
    r.receive(&draw(70, 100, 1));
    let deleted =
        |calls: &[String]| -> Vec<i32> { calls.iter().filter_map(|c| c.strip_prefix("delete ").and_then(|v| v.parse().ok())).collect() };
    assert_eq!(deleted(&calls.borrow()), first, "past 64 canvas fonts the renderer lets them all go in the order it made them");
    calls.borrow_mut().clear();
    r.receive(&draw(31, 200, 3));
    r.receive(&draw(58, 300, 0));
    let made = r.core().backend.next;
    drop(r);
    let calls = calls.borrow();
    let released: Vec<i32> = calls.iter().filter_map(|c| c.strip_prefix("release ").and_then(|v| v.parse().ok())).collect();
    assert_eq!(released, vec![12, 31, 58, 70, 95], "a renderer that goes releases its canvases in node order");
    assert!(made == 69 && deleted(&calls) == vec![66, 67, 68, 69], "and its canvas fonts in the order it made them ({made} made)");
}

/// A backend that writes its calls where the test still reads them after the renderer went.
#[derive(Debug, Default)]
struct Recorder {
    next: i32,
    calls: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
}

impl twidget_renderer::Backend for Recorder {
    fn create_font(&mut self, _f: &[u8], _s: f32, _w: i32, _i: bool, _x: &[u8], _o: &mut twidget_wire::FontMetrics) -> i32 {
        0
    }
    fn create_canvas_font(&mut self, _f: &[u8], size: f32, _w: i32, _i: bool, out: &mut twidget_wire::FontMetrics) -> i32 {
        out.ascent = size * 0.8;
        out.descent = size * 0.2;
        self.next += 1;
        self.next
    }
    fn delete_font(&mut self, font: i32) {
        self.calls.borrow_mut().push(format!("delete {font}"));
    }
    fn text_width(&mut self, _font: i32, text: &[u8]) -> f32 {
        8.0 * text.len() as f32
    }
    fn load_image(&mut self, _id: i32, _url: &[u8], _w: &mut f32, _h: &mut f32) -> bool {
        false
    }
    fn canvas_release(&mut self, id: i32) {
        self.calls.borrow_mut().push(format!("release {id}"));
    }
}

#[test]
fn queries_without_a_document() {
    let mut r = Renderer::new(300.0, 2.0, true, NoDocument, Stub::new(), None);
    let ask = |r: &mut Renderer<NoDocument, Stub>, q: &str| -> Json {
        let out = r.receive(&parse(&format!(r#"{{"type":"query","qid":3,"query":{q}}}"#)));
        assert_eq!(out[0].get("qid").as_number(0.0), 3.0);
        result(&out)
    };
    assert_eq!(ask(&mut r, r#"{"q":"rect","id":5}"#).get("error").as_str(), b"no such element");
    assert_eq!(ask(&mut r, r#"{"q":"rect","id":2}"#).get("error").as_str(), b"no such element", "html is no node before it is adopted");
    assert_eq!(ask(&mut r, r#"{"q":"rect"}"#).get("width").as_number(1.0), 0.0, "a rect of nothing is empty");
    assert!(
        ask(&mut r, r#"{"q":"matchMedia","query":"(prefers-color-scheme: dark)"}"#).get("matches").as_bool(false),
        "the scheme is the theme's"
    );
    assert!(ask(&mut r, r#"{"q":"matchMedia","query":"(max-width: 300px)"}"#).get("matches").as_bool(false));
    assert!(!ask(&mut r, r#"{"q":"matchMedia","query":"(min-width: 300.5px)"}"#).get("matches").as_bool(true));
    assert!(ask(&mut r, r#"{"q":"matchMedia","query":"(hover: none)"}"#).get("matches").as_bool(false));
    assert!(!ask(&mut r, r#"{"q":"matchMedia","query":"(pointer: fine)"}"#).get("matches").as_bool(true));
    let v = ask(&mut r, r#"{"q":"viewport"}"#);
    assert!(
        v.get("innerWidth").as_number(0.0) == 300.0 && v.get("innerHeight").as_number(0.0) == 50.0 && v.get("dpr").as_number(0.0) == 2.0
    );
    assert_eq!(ask(&mut r, r#"{"q":"imageData","w":4097,"h":4097}"#).get("error").as_str(), b"too large");
    let m = ask(&mut r, r#"{"q":"measureText","font":5,"text":"abcd"}"#);
    assert!(
        m.get("width").as_number(0.0) == 32.0 && m.get("actualBoundingBoxAscent").as_number(0.0) == f64::from(10.0f32 * 0.8 * 0.8),
        "10px sans-serif when the font is no string"
    );
    assert_eq!(r.core().backend.made.len(), 1);
}
