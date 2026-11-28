//! core_tests.cpp's checks of events, gestures, focus, the editor and selection, by their names, on the port. What the
//! document answers (hit tests, boxes, the cascade, the text as laid out) comes from the C++ renderer running the same
//! script (`oracle/events_oracle`) as a tape the port replays; the port's own answers are checked against what
//! core_tests expects of the C++, and against the C++'s transcript. The clock is the script's: core_tests' sleeps are
//! steps that far apart. Those checks are skipped without `TWIDGET_ORACLE_DIR`; the last tests are the port's own.

mod common;

use common::cases::compare;
use common::*;
use twidget_events::{EventHost, Events, PROFILE_EVENTS};
use twidget_json::Json;
use twidget_shadow::Document;
use twidget_testkit::Oracle;
use twidget_wire::{Cmd, Command};

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

const LISTEN: &str = r#"{"type":"listen","types":["mouseover","mouseout","mouseenter","mouseleave","pointerenter","pointermove","dblclick","wheel","keydown","animationstart","animationiteration","transitionend"]}"#;

fn events_of(out: &[Json]) -> Vec<&Json> {
    out.iter().filter(|m| m.get("type").as_str() == b"event").map(|m| m.get("ev")).collect()
}

/// "type:target " for each event, as core_tests' `types`.
fn types(out: &[Json]) -> String {
    events_of(out)
        .iter()
        .map(|ev| format!("{}:{} ", String::from_utf8_lossy(ev.get("type").as_str()), ev.get("targetId").as_number(0.0) as i64))
        .collect()
}

fn kind(ev: &Json) -> &[u8] {
    ev.get("type").as_str()
}

/// The script through the oracle and the port; None without the oracle.
fn replay(script: &Script) -> Option<(Run, Tally)> {
    let oracle = Oracle::find("events_oracle")?;
    let mut tally = Tally::default();
    let mut runs = compare(&oracle, &[(script.clone(), "the script".into())], &mut tally);
    runs.pop().map(|r| (r, tally))
}

/// The renderer of core_tests' main(): PAGE and its listeners.
fn main_page() -> Script {
    let mut s = Script::new(30);
    s.ops(0.0, PAGE);
    s.message(0.0, LISTEN);
    s
}

fn tap(s: &mut Script, x: f64, y: f64, mouse: bool) -> (usize, usize) {
    s.touch(16.0, 0, x, y, 0, mouse);
    let down = s.steps.len() - 1;
    s.touch(16.0, 2, x, y, 0, mouse);
    (down, s.steps.len() - 1)
}

#[test]
fn hover_enter_and_leave_order() {
    let mut s = main_page();
    s.after(16.0, Step::Hover { action: 0, x: 10.0, y: 10.0, modifiers: 0 });
    s.after(16.0, Step::Tick);
    s.after(16.0, Step::Hover { action: 0, x: 150.0, y: 10.0, modifiers: 0 });
    s.after(16.0, Step::Hover { action: 1, x: 0.0, y: 0.0, modifiers: 0 });
    let Some((r, _)) = replay(&s) else { return };
    assert_eq!(
        types(&r.outs[2]),
        "mouseover:11 pointerenter:2 mouseenter:2 pointerenter:4 mouseenter:4 pointerenter:10 mouseenter:10 pointerenter:11 mouseenter:11 ",
        "hover into A1: over on A1, enter on html, body, A, A1, outermost first"
    );
    assert!(r.pending[2] > 0, "the hover's mousemove waits for the frame");
    assert_eq!(types(&r.outs[3]), "pointermove:11 ", "the frame sends the listened move, not the unlistened mousemove");
    assert_eq!(
        types(&r.outs[4]),
        "mouseout:11 mouseleave:11 mouseleave:10 mouseover:12 pointerenter:12 mouseenter:12 ",
        "A1 to B: out, leave A1 then A, over, enter B only"
    );
    assert_eq!(events_of(&r.outs[4])[0].get("relatedId").as_number(0.0), 12.0, "mouseout names the element entered");
    assert_eq!(
        types(&r.outs[5]),
        "pointermove:12 mouseout:12 mouseleave:12 mouseleave:4 mouseleave:2 ",
        "leaving the view: the pending move first, then B, body, html"
    );
}

#[test]
fn clicks() {
    let mut s = main_page();
    tap(&mut s, 150.0, 10.0, true);
    s.touch(16.0, 0, 151.0, 10.0, 0, true);
    s.touch(16.0, 2, 151.0, 10.0, 0, true);
    let second = s.steps.len() - 1;
    s.touch(16.0, 0, 10.0, 10.0, 0, true);
    s.touch(16.0, 2, 150.0, 10.0, 0, true);
    let apart = s.steps.len() - 1;
    s.touch(400.0, 0, 10.0, 10.0, 0, true);
    s.touch(16.0, 2, 80.0, 80.0, 0, true);
    let outer = s.steps.len() - 1;
    let Some((r, _)) = replay(&s) else { return };
    let evs = events_of(&r.outs[second]);
    let dbl = evs.iter().any(|e| kind(e) == b"dblclick");
    let detail = evs.iter().find(|e| kind(e) == b"click").map_or(0, |e| e.get("detail").as_int(0));
    assert!(
        dbl && detail == 2 && types(&r.outs[second]).contains("click:12 dblclick:12"),
        "two quick clicks: click detail 2, then dblclick (got {})",
        types(&r.outs[second])
    );
    assert!(!types(&r.outs[second]).contains("touch"), "a mouse sends no touch events");
    assert!(!types(&r.outs[apart]).contains("click"), "press on A1, release on B: no click");
    let to_a = events_of(&r.outs[outer]).iter().any(|e| kind(e) == b"click" && e.get("targetId").as_number(0.0) == 10.0);
    assert!(to_a, "press on A1, release on A: the click goes to A");
}

#[test]
fn wheel_and_keys() {
    let mut s = main_page();
    s.after(16.0, Step::Wheel { x: 150.0, y: 10.0, dx: 0.0, dy: 50.0, mode: 0, modifiers: 0 });
    s.after(16.0, Step::Tick);
    s.after(16.0, Step::Key { kind: b"keydown".to_vec(), key: b"a".to_vec(), code: b"KeyA".to_vec(), repeat: false, modifiers: 8 });
    let Some((r, _)) = replay(&s) else { return };
    assert!(events_of(&r.outs[3]).is_empty(), "no wheel reaches the widget");
    assert!(events_of(&r.outs[4]).is_empty(), "no key reaches the widget");
}

#[test]
fn pointer_capture() {
    let mut s = Script::new(30);
    s.ops(0.0, PAGE);
    s.message(
        0.0,
        r#"{"type":"listen","types":["pointermove","mousemove","pointerup","mouseup","gotpointercapture","lostpointercapture"]}"#,
    );
    s.touch(16.0, 0, 10.0, 10.0, 0, true);
    s.ops(16.0, r#"[{"op":"call","id":11,"method":"setPointerCapture","args":[1]}]"#);
    let got = s.steps.len() - 1;
    s.touch(16.0, 1, 150.0, 10.0, 0, true);
    s.after(16.0, Step::Tick);
    let moved = s.steps.len() - 1;
    s.touch(16.0, 2, 150.0, 10.0, 0, true);
    let up = s.steps.len() - 1;
    let (_, click) = tap(&mut s, 10.0, 10.0, true);
    let Some((r, _)) = replay(&s) else { return };
    assert_eq!(types(&r.outs[got]), "", "setPointerCapture on the pressed mouse: no gotpointercapture, outside profile 1");
    assert_eq!(
        types(&r.outs[moved]),
        "pointermove:11 mousemove:12 ",
        "captured: pointermove to the capture, mousemove to the element under the mouse"
    );
    assert_eq!(types(&r.outs[up]), "pointerup:11 mouseup:12 ", "release: pointerup to the capture, mouseup under the mouse");
    let actions = events_of(&r.outs[click]).iter().any(|e| kind(e) == b"click" && e.get("defaultActions").as_bool(false));
    assert!(actions, "a click asks the sandbox for its default actions");
}

const FOCUS_PAGE: &str = r#"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
    {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
    {"op":"el","id":10,"tag":"input","attrs":[["style","width:200px;height:20px;padding:4px"],["placeholder","Name"],["value","ab"]]},{"op":"ins","parent":4,"id":10,"before":null},
    {"op":"el","id":11,"tag":"div","attrs":[["style","height:40px"]]},{"op":"ins","parent":4,"id":11,"before":null}]"#;

#[test]
fn focus_and_the_platforms_editor() {
    let mut s = Script::new(20);
    s.width = 400.0;
    s.ops(0.0, FOCUS_PAGE);
    s.after(16.0, Step::Paint);
    let (_, tapped) = tap(&mut s, 50.0, 10.0, true);
    s.after(16.0, Step::LastHtml);
    let hidden = s.steps.len() - 1;
    s.after(16.0, Step::Edit { current: true, generation: 0, text: b"abc".to_vec(), kind: b"insertText".to_vec() });
    let edited = s.steps.len() - 1;
    s.after(16.0, Step::LastHtml);
    let valued = s.steps.len() - 1;
    s.after(16.0, Step::Edit { current: false, generation: 102, text: b"x".to_vec(), kind: b"insertText".to_vec() });
    let stale = s.steps.len() - 1;
    let (_, elsewhere) = tap(&mut s, 50.0, 50.0, true);
    s.after(16.0, Step::LastHtml);
    let shown = s.steps.len() - 1;
    s.ops(16.0, r#"[{"op":"call","id":10,"method":"focus"}]"#);
    let focused = s.steps.len() - 1;
    s.ops(16.0, r#"[{"op":"call","id":10,"method":"setSelectionRange","args":[1,2]}]"#);
    let ranged = s.steps.len() - 1;
    s.after(16.0, Step::EndEditing);
    s.ops(1100.0, r#"[{"op":"call","id":10,"method":"focus"}]"#);
    let late = s.steps.len() - 1;
    let Some((r, _)) = replay(&s) else { return };
    assert!(
        !types(&r.outs[tapped]).contains("focus") && r.editors[tapped].is_some(),
        "a tap on a text input edits it, with no focus event"
    );
    let spec = r.editors[tapped].clone().unwrap_or_default();
    assert!(
        spec.id == 10
            && spec.text == b"ab"
            && spec.placeholder == b"Name"
            && spec.input_type == b"text"
            && (spec.frame.w - 190.0).abs() < 0.5
            && (spec.frame.x - 5.0).abs() < 0.5
            && spec.color.a == 255
            && spec.font != 0,
        "the editor spec has the control's content box (inside its padding and border), font, colour, text and placeholder: {spec:?}"
    );
    let html = |i: usize| String::from_utf8_lossy(&r.html[i]).into_owned();
    let at = |step: usize| s.steps[..=step].iter().filter(|(_, st)| matches!(st, Step::LastHtml)).count() - 1;
    assert!(
        html(at(hidden)).contains("color:transparent") && !html(at(hidden)).contains("placeholder"),
        "the document stops drawing the edited text and placeholder"
    );
    assert!(events_of(&r.outs[edited]).is_empty(), "an edit is no input event");
    assert!(html(at(valued)).contains("value=\"abc\""), "and the value is the document's");
    assert!(r.outs[stale].is_empty(), "a stale generation is ignored");
    assert!(
        !types(&r.outs[elsewhere]).contains("change") && !types(&r.outs[elsewhere]).contains("blur") && r.editors[elsewhere].is_none(),
        "a tap elsewhere ends it, with no change or blur"
    );
    assert!(!html(at(shown)).contains("color:transparent"), "and the text shows again");
    assert!(r.editors[focused].as_ref().is_some_and(|e| e.id == 10), "focus() just after a tap focuses");
    assert!(
        r.editors[ranged].as_ref().is_some_and(|e| e.selection_start == 1 && e.selection_end == 2),
        "setSelectionRange reaches the editor"
    );
    assert!(r.editors[late].is_none(), "focus() long after the user's input does not");
}

#[test]
fn trusted_input() {
    let mut s = Script::new(30);
    s.ops(0.0, PAGE);
    let (_, click) = tap(&mut s, 10.0, 10.0, true);
    s.message(16.0, r#"{"type":"listen","types":["pointerdown","mousedown","pointerup","click"]}"#);
    let (fd, fu) = tap(&mut s, 10.0, 10.0, false);
    let (md, mu) = tap(&mut s, 10.0, 10.0, true);
    let Some((r, _)) = replay(&s) else { return };
    let trusted = r.outs[click]
        .iter()
        .any(|m| m.get("type").as_str() == b"event" && m.get("ev").get("type").as_str() == b"click" && m.get("trusted").as_bool(false));
    assert!(trusted, "a click is trusted, as the hosts require before they count a gesture");
    let trust = |out: &[Json]| -> String {
        out.iter()
            .filter(|m| m.get("type").as_str() == b"event")
            .map(|m| {
                format!(
                    "{}{} ",
                    String::from_utf8_lossy(m.get("ev").get("type").as_str()),
                    if m.get("trusted").as_bool(false) { "+" } else { "-" }
                )
            })
            .collect()
    };
    let finger = trust(&r.outs[fd]) + &trust(&r.outs[fu]);
    let mouse = trust(&r.outs[md]) + &trust(&r.outs[mu]);
    assert_eq!(finger, "pointerdown- mousedown- pointerup+ click+ ", "a finger's press is no gesture, its release and click are");
    assert_eq!(mouse, "pointerdown+ mousedown+ pointerup+ click+ ", "a mouse's press is a gesture");
}

const SELECT_PAGE: &str = r#"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
    {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
    {"op":"el","id":5,"tag":"style"},{"op":"css","id":5,"text":".s{user-select:text} p{margin:0;font:10px/20px sans-serif}"},{"op":"ins","parent":3,"id":5,"before":null},
    {"op":"el","id":10,"tag":"p","attrs":[["class","s"]]},{"op":"ins","parent":4,"id":10,"before":null},{"op":"text","id":11,"data":"Hello world"},{"op":"ins","parent":10,"id":11,"before":null},
    {"op":"el","id":12,"tag":"p","attrs":[["class","s"]]},{"op":"ins","parent":4,"id":12,"before":null},{"op":"text","id":13,"data":"Second"},{"op":"ins","parent":12,"id":13,"before":null},
    {"op":"el","id":14,"tag":"p"},{"op":"ins","parent":4,"id":14,"before":null},{"op":"text","id":15,"data":"Plain"},{"op":"ins","parent":14,"id":15,"before":null}]"#;

fn drag(s: &mut Script, from: (f64, f64), to: (f64, f64), mouse: bool) -> usize {
    let pointer = if mouse { 1 } else { 0 };
    s.touch(16.0, 0, from.0, from.1, pointer, mouse);
    let down = s.steps.len() - 1;
    s.touch(16.0, 1, to.0, to.1, pointer, mouse);
    s.touch(16.0, 2, to.0, to.1, pointer, mouse);
    s.after(0.0, Step::Observe);
    down
}

/// Each observe step's (has_selection, selected_text), in order.
fn observed(r: &Run) -> Vec<(bool, String)> {
    r.observed.iter().map(|(_, has, text, _)| (*has, String::from_utf8_lossy(text).into_owned())).collect()
}

fn tinted(list: &[Command]) -> Vec<&Command> {
    list.iter().filter(|c| c.kind == Cmd::FillRect && c.color.a == 77).collect()
}

#[test]
fn text_selection_by_a_mouse_drag() {
    let mut s = Script::new(20);
    s.ops(0.0, SELECT_PAGE);
    let down = drag(&mut s, (1.0, 10.0), (200.0, 10.0), true);
    drag(&mut s, (17.0, 10.0), (33.0, 10.0), true);
    s.after(16.0, Step::Paint);
    let painted = s.steps.len() - 1;
    drag(&mut s, (1.0, 10.0), (200.0, 30.0), true);
    drag(&mut s, (2.0, 50.0), (2.0, 50.0), true);
    drag(&mut s, (1.0, 50.0), (200.0, 50.0), true);
    drag(&mut s, (1.0, 10.0), (200.0, 10.0), false);
    s.message(16.0, r#"{"type":"preventDefault","keys":["10:pointerdown"]}"#);
    drag(&mut s, (1.0, 10.0), (200.0, 10.0), true);
    s.message(16.0, r#"{"type":"preventDefault","keys":[]}"#);
    drag(&mut s, (1.0, 30.0), (200.0, 30.0), true);
    s.after(16.0, Step::ClearSelection);
    s.after(0.0, Step::Observe);
    s.after(0.0, Step::ClearSelection);
    let Some((r, _)) = replay(&s) else { return };
    let seen = observed(&r);
    let claimed = r.outs[down].iter().any(|m| m.get("type").as_str() == b"gesture" && m.get("x").as_bool(false));
    assert!(seen[0].1 == "Hello world" && claimed, "a mouse drag over selectable text selects it and claims the drag (got {:?})", seen[0]);
    let runs = Json::parse(&r.observed[0].3).expect("selectable_text_json is JSON");
    let mut joined = String::new();
    let mut shaped = !runs.arr().is_empty();
    for run in runs.arr() {
        joined.push_str(&String::from_utf8_lossy(run.at(9).as_str()));
        shaped = shaped && run.arr().len() == 11 && run.at(6).as_int(0) > 0 && run.at(5).as_number(0.0) > run.at(2).as_number(0.0);
    }
    assert!(
        shaped && joined.contains("Hello") && joined.contains("Second") && !joined.contains("Plain"),
        "selectable_text_json lists the selectable runs with font and baseline, not the rest"
    );
    assert_eq!(seen[1].1, "ll", "the ends snap to the nearest character boundaries");
    let tint = r.painted[painted]
        .iter()
        .flat_map(|l| tinted(l))
        .any(|c| c.color.r == 51 && c.color.g == 144 && c.color.b == 236 && c.rect.x == 16.0 && c.rect.w == 16.0);
    assert!(tint, "the selection paints in the link colour over its characters");
    assert_eq!(seen[2].1, "Hello world\nSecond", "a selection across blocks keeps a line break between them");
    assert!(!seen[3].0, "a press elsewhere drops it");
    assert!(!seen[4].0, "text without user-select: text does not select");
    assert!(!seen[5].0, "a finger's drag does not select: touch selection starts from the client's menu");
    assert!(!seen[6].0, "a widget that prevents the press keeps the drag");
    assert!(r.cleared == [true, false] && !seen[8].0, "clear_selection drops it once");
}

#[test]
fn text_selection_inside_transforms_and_clips() {
    let mut s = Script::new(20);
    s.ops(
        0.0,
        r#"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
        {"op":"el","id":5,"tag":"style"},{"op":"css","id":5,"text":".s{user-select:text} p{margin:0;font:10px/20px sans-serif;white-space:nowrap} .m{transform:translateX(50px)} .c{overflow:hidden;width:20px}"},{"op":"ins","parent":3,"id":5,"before":null},
        {"op":"el","id":10,"tag":"p","attrs":[["class","s m"]]},{"op":"ins","parent":4,"id":10,"before":null},{"op":"text","id":11,"data":"Hello"},{"op":"ins","parent":10,"id":11,"before":null},
        {"op":"el","id":12,"tag":"div","attrs":[["class","c"]]},{"op":"ins","parent":4,"id":12,"before":null},
        {"op":"el","id":13,"tag":"p","attrs":[["class","s"]]},{"op":"ins","parent":12,"id":13,"before":null},{"op":"text","id":14,"data":"Clipped text"},{"op":"ins","parent":13,"id":14,"before":null}]"#,
    );
    s.after(16.0, Step::Paint);
    drag(&mut s, (51.0, 10.0), (67.0, 10.0), true);
    s.after(16.0, Step::Paint);
    let grouped = s.steps.len() - 1;
    drag(&mut s, (1.0, 30.0), (200.0, 30.0), true);
    s.after(16.0, Step::Paint);
    let clipped = s.steps.len() - 1;
    let Some((r, _)) = replay(&s) else { return };
    let seen = observed(&r);
    assert_eq!(seen[0].1, "He", "translated text selects where it shows");
    let runs = Json::parse(&r.observed[0].3).expect("selectable_text_json is JSON");
    let moved = !runs.arr().is_empty()
        && runs.arr().iter().all(|run| {
            let e = run.at(10).at(4).as_number(-1.0);
            if run.at(9).as_str() == b"Hello" { e == 50.0 } else { e == 0.0 }
        });
    assert!(moved, "selectable_text_json gives each run the transform it shows under");
    let depth_of_tint = |list: &[Command], push: Cmd, pop: Cmd| -> Option<(i32, Command)> {
        let mut depth = 0;
        let mut found = None;
        for c in list {
            if c.kind == push {
                depth += 1;
            } else if c.kind == pop {
                depth -= 1;
            } else if c.kind == Cmd::FillRect && c.color.a == 77 {
                found = Some((depth, c.clone()));
            }
        }
        found
    };
    let grouped_tint = r.painted[grouped].iter().find_map(|l| depth_of_tint(l, Cmd::GroupPush, Cmd::GroupPop));
    assert!(
        grouped_tint.is_some_and(|(d, c)| d > 0 && c.rect.x == 0.0 && c.rect.w == 16.0),
        "the tint is painted inside the text's transform, in its coordinates"
    );
    assert_eq!(seen[1].1, "Clipped text", "a drag past the clip selects the text under it");
    let clipped_tint = r.painted[clipped].iter().find_map(|l| depth_of_tint(l, Cmd::ClipPush, Cmd::ClipPop));
    assert!(clipped_tint.is_some_and(|(d, _)| d > 0), "the tint stays inside the clip of its text");
}

#[test]
fn a_long_unbroken_run_costs_a_bounded_number_of_measurements() {
    for (length, bound, what) in [
        (3000usize, 64usize, "a press and eight moves over a 3,000-character run search its carets, reusing them"),
        (65536, 0, "past the bound a run's carets are spread over its width, unmeasured"),
    ] {
        let mut s = Script::new(20);
        let text = "a".repeat(length);
        s.ops(
            0.0,
            &format!(
                r#"[{{"op":"el","id":2,"tag":"html","adopt":"html"}},{{"op":"el","id":3,"tag":"head","adopt":"head"}},
                {{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]}},
                {{"op":"el","id":5,"tag":"style"}},{{"op":"css","id":5,"text":"p{{user-select:text;margin:0;font:10px/20px sans-serif;white-space:nowrap}}"}},{{"op":"ins","parent":3,"id":5,"before":null}},
                {{"op":"el","id":10,"tag":"p"}},{{"op":"ins","parent":4,"id":10,"before":null}},{{"op":"text","id":11,"data":"{text}"}},{{"op":"ins","parent":10,"id":11,"before":null}}]"#
            ),
        );
        s.after(16.0, Step::Paint);
        s.touch(16.0, 0, 1.0, 10.0, 1, true);
        for x in (100..=800).step_by(100) {
            s.touch(16.0, 1, f64::from(x), 10.0, 1, true);
        }
        s.touch(16.0, 2, 800.0, 10.0, 1, true);
        s.after(0.0, Step::Observe);
        let Some((r, tally)) = replay(&s) else { return };
        let measured = tally.count("asked text_width");
        assert!(observed(&r)[0].1 == "a".repeat(100) && measured <= bound, "{what} ({measured} measured)");
    }
}

#[test]
fn hover_and_active_start_their_transitions_at_once() {
    let mut s = Script::new(20);
    s.ops(
        0.0,
        r#"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
        {"op":"el","id":5,"tag":"style"},{"op":"css","id":5,"text":".b{height:20px;opacity:1;transition:opacity 1s} .b:hover{opacity:0} .c{height:20px;opacity:1;transition:opacity 1s} .c:active{opacity:0}"},{"op":"ins","parent":3,"id":5,"before":null},
        {"op":"el","id":10,"tag":"div","attrs":[["class","b"]]},{"op":"ins","parent":4,"id":10,"before":null},
        {"op":"el","id":11,"tag":"div","attrs":[["class","c"]]},{"op":"ins","parent":4,"id":11,"before":null}]"#,
    );
    s.after(16.0, Step::Paint);
    s.after(16.0, Step::Hover { action: 0, x: 5.0, y: 10.0, modifiers: 0 });
    s.touch(16.0, 0, 5.0, 30.0, 0, false);
    let Some((_, tally)) = replay(&s) else { return };
    assert!(
        tally.count("build y") >= 2 && tally.count("restyled by the pointer") >= 2,
        "a :hover or :active restyle starts its transition without waiting for a message"
    );
}

#[test]
fn the_cursor_under_the_mouse() {
    let mut s = Script::new(20);
    s.ops(
        0.0,
        r#"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
        {"op":"el","id":10,"tag":"div","attrs":[["style","height:20px;cursor:pointer"]]},{"op":"ins","parent":4,"id":10,"before":null},
        {"op":"el","id":11,"tag":"span","attrs":[["style","display:block;height:20px"]]},{"op":"ins","parent":10,"id":11,"before":null},
        {"op":"el","id":12,"tag":"div","attrs":[["style","height:20px"]]},{"op":"ins","parent":4,"id":12,"before":null}]"#,
    );
    s.after(16.0, Step::Hover { action: 0, x: 5.0, y: 10.0, modifiers: 0 });
    s.after(0.0, Step::Observe);
    s.after(16.0, Step::Hover { action: 0, x: 5.0, y: 30.0, modifiers: 0 });
    s.after(0.0, Step::Observe);
    s.after(16.0, Step::Hover { action: 0, x: 5.0, y: 10.0, modifiers: 0 });
    s.after(16.0, Step::Hover { action: 1, x: 0.0, y: 0.0, modifiers: 0 });
    s.after(0.0, Step::Observe);
    let Some((r, _)) = replay(&s) else { return };
    let cursors: Vec<String> = r.observed.iter().map(|o| String::from_utf8_lossy(&o.0).into_owned()).collect();
    assert_eq!(
        cursors,
        ["pointer", "auto", "auto"],
        "cursor() is the document's cursor under the mouse, inherited, and auto elsewhere or once it left"
    );
}

#[test]
fn img_load_and_error() {
    // The oracle's backend decodes an image at 24 x 16 unless its URL says "fail" (core_tests' Decoding gives 2 x 3).
    let page = |src: &str| {
        format!(
            r#"[{{"op":"el","id":2,"tag":"html","adopt":"html"}},{{"op":"el","id":3,"tag":"head","adopt":"head"}},
            {{"op":"el","id":4,"tag":"body","adopt":"body"}},
            {{"op":"el","id":10,"tag":"img","attrs":[["src","data:image/png;base64,iVBORw0KGgo{src}="]]}},{{"op":"ins","parent":4,"id":10,"before":null}},
            {{"op":"el","id":11,"tag":"img","attrs":[["src","data:image/png;base64,AAAA{src}"]]}}]"#
        )
    };
    let mut s = Script::new(20);
    s.ops(0.0, &page(""));
    s.ops(16.0, r#"[{"op":"attr","id":10,"name":"src","value":"data:image/png;base64,BBBB"}]"#);
    let Some((r, _)) = replay(&s) else { return };
    let loads: String = events_of(&r.outs[0])
        .iter()
        .filter(|e| kind(e) == b"load")
        .map(|e| {
            format!(
                "{}:{}x{} ",
                e.get("targetId").as_number(0.0),
                e.get("naturalWidth").as_number(0.0),
                e.get("naturalHeight").as_number(0.0)
            )
        })
        .collect();
    assert_eq!(loads, "10:24x16 11:24x16 ", "an <img>, attached or not, loads with its natural size");
    let again = events_of(&r.outs[1]);
    assert!(again.len() == 1 && kind(again[0]) == b"load", "a new src loads again");
    let mut f = Script::new(20);
    f.ops(0.0, &page("fail"));
    let Some((r, _)) = replay(&f) else { return };
    assert_eq!(types(&r.outs[0]), "error:10 error:11 ", "an image the backend cannot decode is an error");
}

#[test]
fn movement_of_coalesced_moves() {
    let mut s = Script::new(20);
    s.ops(
        0.0,
        r#"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
        {"op":"el","id":10,"tag":"div","attrs":[["style","height:200px"]]},{"op":"ins","parent":4,"id":10,"before":null}]"#,
    );
    s.message(0.0, r#"{"type":"listen","types":["pointermove"]}"#);
    s.touch(16.0, 0, 100.0, 10.0, 0, true);
    s.touch(16.0, 1, 120.0, 10.0, 0, true);
    s.touch(16.0, 1, 150.0, 14.0, 0, true);
    s.after(16.0, Step::Tick);
    let first = s.steps.len() - 1;
    s.touch(16.0, 1, 160.0, 14.0, 0, true);
    s.after(16.0, Step::Tick);
    let second = s.steps.len() - 1;
    s.touch(16.0, 2, 160.0, 14.0, 0, true);
    let Some((r, _)) = replay(&s) else { return };
    let moves = events_of(&r.outs[first]);
    assert!(
        moves.len() == 1 && moves[0].get("movementX").as_number(0.0) == 50.0 && moves[0].get("movementY").as_number(0.0) == 4.0,
        "coalesced moves add their movementX/Y up"
    );
    let moves = events_of(&r.outs[second]);
    assert!(moves.len() == 1 && moves[0].get("movementX").as_number(0.0) == 10.0, "movementX counts from the last move the widget saw");
}

#[test]
fn resize_observer_and_details() {
    let mut s = Script::new(30);
    s.ops(
        0.0,
        r#"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
        {"op":"el","id":11,"tag":"div","attrs":[["style","width:100px;height:20px"]]},{"op":"ins","parent":4,"id":11,"before":null},
        {"op":"el","id":20,"tag":"details"},{"op":"ins","parent":4,"id":20,"before":null},
        {"op":"el","id":21,"tag":"summary"},{"op":"ins","parent":20,"id":21,"before":null},
        {"op":"text","id":22,"data":"s"},{"op":"ins","parent":21,"id":22,"before":null},
        {"op":"el","id":23,"tag":"div","attrs":[["style","height:20px"]]},{"op":"ins","parent":20,"id":23,"before":null}]"#,
    );
    s.ops(16.0, r#"[{"op":"call","id":11,"method":"observeResize","args":[]}]"#);
    let observed_at = s.steps.len() - 1;
    s.ops(16.0, r#"[{"op":"attr","id":11,"name":"style","value":"width:90px;height:20px"}]"#);
    let changed = s.steps.len() - 1;
    let (_, opened) = tap(&mut s, 4.0, 28.0, false);
    let Some((r, _)) = replay(&s) else { return };
    let resized = |out: &[Json]| {
        out.iter()
            .find(|m| m.get("type").as_str() == b"resize" && !m.get("entries").arr().is_empty())
            .map_or(-1.0, |m| m.get("entries").at(0).get("width").as_number(0.0))
    };
    assert_eq!(resized(&r.outs[observed_at]), 100.0, "observeResize reports the content box at once");
    assert_eq!(resized(&r.outs[changed]), 90.0, "and again when it changes");
    let toggled = events_of(&r.outs[opened])
        .iter()
        .any(|e| kind(e) == b"toggle" && e.get("targetId").as_number(0.0) == 20.0 && e.get("open").as_bool(false));
    assert!(toggled, "tapping the summary opens the <details> and sends toggle");
}

const POLICY_PAGE: &str = r#"[
    {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
    {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
    {"op":"el","id":30,"tag":"div","attrs":[["style","margin-left:40px;width:100px;height:40px;touch-action:none"]]},
    {"op":"el","id":31,"tag":"div","attrs":[["style","margin-left:40px;width:100px;height:40px"]]},
    {"op":"el","id":32,"tag":"div","attrs":[["style","height:40px;touch-action:none"]]},
    {"op":"ins","parent":4,"id":30,"before":null},{"op":"ins","parent":4,"id":31,"before":null},{"op":"ins","parent":4,"id":32,"before":null}]"#;

fn claim_of(out: &[Json]) -> String {
    out.iter().find(|m| m.get("type").as_str() == b"gesture").map_or("none".into(), |m| {
        format!(
            "{}{}{}",
            if m.get("x").as_bool(false) { "x" } else { "" },
            if m.get("y").as_bool(false) { "y" } else { "" },
            if m.get("hold").as_bool(false) { "h" } else { "" }
        )
    })
}

fn bubble_of(out: &[Json]) -> String {
    out.iter()
        .filter(|m| m.get("type").as_str() == b"bubble")
        .map(|m| {
            format!(
                "{}/{}{}",
                String::from_utf8_lossy(m.get("action").as_str()),
                String::from_utf8_lossy(m.get("gesture").as_str()),
                if m.get("edge").as_bool(false) { "/edge" } else { "" }
            )
        })
        .collect()
}

#[test]
fn who_gets_a_press_the_gesture_policy() {
    let mut s = Script::new(40);
    s.ops(0.0, POLICY_PAGE);
    s.message(0.0, r#"{"type":"listen","types":["contextmenu","dblclick","pointerup","pointercancel"]}"#);
    let mark = |s: &Script| s.steps.len() - 1;
    let touch = |s: &mut Script, ms: f64, action: i32, x: f64, y: f64, mouse: bool, button: i32| {
        s.after(ms, Step::Touch { action, x, y, pointer: 0, primary: true, mouse, modifiers: 0, button });
        s.steps.len() - 1
    };
    let d1 = touch(&mut s, 16.0, 0, 60.0, 20.0, false, 0);
    s.after(520.0, Step::Tick);
    let t1 = mark(&s);
    let u1 = touch(&mut s, 16.0, 2, 60.0, 20.0, false, 0);
    let d2 = touch(&mut s, 16.0, 0, 60.0, 60.0, false, 0);
    s.after(520.0, Step::Tick);
    let t2 = mark(&s);
    let u2 = touch(&mut s, 16.0, 3, 60.0, 60.0, false, 0);
    touch(&mut s, 16.0, 0, 5.0, 100.0, false, 0);
    s.after(520.0, Step::Tick);
    let t3 = mark(&s);
    touch(&mut s, 16.0, 3, 5.0, 100.0, false, 0);
    let rd = touch(&mut s, 16.0, 0, 60.0, 60.0, true, 2);
    let ru = touch(&mut s, 16.0, 2, 60.0, 60.0, true, 0);
    let nd = touch(&mut s, 16.0, 0, 60.0, 20.0, true, 2);
    let nu = touch(&mut s, 16.0, 2, 60.0, 20.0, true, 0);
    touch(&mut s, 400.0, 0, 60.0, 60.0, false, 0);
    touch(&mut s, 16.0, 2, 60.0, 60.0, false, 0);
    touch(&mut s, 16.0, 0, 62.0, 61.0, false, 0);
    let dbl = touch(&mut s, 16.0, 2, 62.0, 61.0, false, 0);
    s.message(400.0, r#"{"type":"arbitrate","bubble":true}"#);
    touch(&mut s, 16.0, 0, 60.0, 60.0, false, 0);
    s.after(520.0, Step::Tick);
    let taken = mark(&s);
    let tu = touch(&mut s, 16.0, 2, 60.0, 60.0, false, 0);
    touch(&mut s, 16.0, 0, 5.0, 100.0, false, 0);
    let edge = touch(&mut s, 16.0, 1, 40.0, 100.0, false, 0);
    touch(&mut s, 16.0, 2, 40.0, 100.0, false, 0);
    touch(&mut s, 16.0, 0, 60.0, 20.0, false, 0);
    s.after(520.0, Step::Tick);
    let held = mark(&s);
    let scrolled = touch(&mut s, 16.0, 1, 62.0, 60.0, false, 0);
    touch(&mut s, 16.0, 2, 62.0, 60.0, false, 0);
    let Some((r, _)) = replay(&s) else { return };
    assert_eq!(claim_of(&r.outs[d1]), "xh", "a press on touch-action: none claims horizontal drags and holds");
    assert_eq!(types(&r.outs[t1]), "contextmenu:30 ", "a finger held on none gets the widget's contextmenu");
    assert_eq!(types(&r.outs[u1]), "pointerup:30 ", "and its release is no click");
    assert_eq!(claim_of(&r.outs[d2]), "", "a press on auto claims nothing");
    assert_eq!(types(&r.outs[t2]), "", "a hold on auto is the bubble's: no contextmenu");
    assert_eq!(types(&r.outs[u2]), "pointercancel:31 ", "the client's menu cancels it");
    assert_eq!(types(&r.outs[t3]), "", "the leading 20 px take no claim: no contextmenu on none");
    assert_eq!(types(&r.outs[rd]) + &types(&r.outs[ru]), "pointerup:31 ", "a right click on auto: no contextmenu, no click");
    let ctx = events_of(&r.outs[nd]);
    assert!(
        types(&r.outs[nd]) == "contextmenu:30 " && ctx[0].get("button").as_int(0) == 2 && types(&r.outs[nu]) == "pointerup:30 ",
        "a right click on none: the widget's contextmenu, no click"
    );
    assert_eq!(types(&r.outs[dbl]), "pointerup:31 click:31 dblclick:31 ", "a double tap on auto is the widget's: click, then dblclick");
    assert!(
        types(&r.outs[taken]) == "pointercancel:31 " && bubble_of(&r.outs[taken]) == "menu/hold" && types(&r.outs[tu]).is_empty(),
        "as the bubble, a hold on auto ends with pointercancel and the menu, then nothing"
    );
    assert!(
        types(&r.outs[edge]) == "pointercancel:32 " && bubble_of(&r.outs[edge]) == "back/drag-x/edge",
        "as the bubble, a drag from the leading edge is back"
    );
    assert!(
        types(&r.outs[held]) == "contextmenu:30 "
            && types(&r.outs[scrolled]) == "pointercancel:30 "
            && bubble_of(&r.outs[scrolled]).starts_with("scroll/"),
        "as the bubble, a vertical drag after a hold on none scrolls the chat"
    );
}

#[test]
fn a_pointer_move_whose_restyle_runs_the_layout_past_its_limit() {
    let mut ops = String::from(
        r#"[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[]},{"op":"el","id":5,"tag":"style","attrs":[]},
        {"op":"css","id":5,"text":"body{margin:0}#t{width:100px;min-height:40px;transition:width 1s linear}#t:hover{width:200px}#b{display:none}#t:hover #b{display:block}.g{display:grid}"},
        {"op":"ins","parent":3,"id":5,"before":null},{"op":"el","id":10,"tag":"div","attrs":[["id","t"]]},{"op":"ins","parent":4,"id":10,"before":null},
        {"op":"el","id":11,"tag":"div","attrs":[["id","b"]]},{"op":"ins","parent":10,"id":11,"before":null}"#,
    );
    // Twenty nested grids, not displayed until #t is hovered.
    let mut parent = 11;
    let mut id = 100;
    for _ in 0..20 {
        let (g, c, t) = (id, id + 1, id + 2);
        ops.push_str(&format!(
            r#",{{"op":"el","id":{g},"tag":"div","attrs":[["class","g"]]}},{{"op":"el","id":{c},"tag":"div","attrs":[]}},{{"op":"text","id":{t},"data":"x"}},{{"op":"ins","parent":{c},"id":{t},"before":null}},{{"op":"ins","parent":{g},"id":{c},"before":null}},{{"op":"ins","parent":{parent},"id":{g},"before":null}}"#
        ));
        parent = id;
        id += 3;
    }
    ops.push(']');
    let mut s = Script::new(200);
    s.ops(0.0, &ops);
    s.touch(16.0, 0, 250.0, 300.0, 0, true);
    s.touch(16.0, 1, 50.0, 20.0, 0, true);
    let moved = s.steps.len() - 1;
    s.touch(16.0, 2, 50.0, 20.0, 0, true);
    let up = s.steps.len() - 1;
    let Some((r, _)) = replay(&s) else { return };
    let stopped = r.outs[moved]
        .iter()
        .any(|m| m.get("type").as_str() == b"violation" && String::from_utf8_lossy(m.get("detail").as_str()).contains("layout limit"));
    assert!(
        stopped && events_of(&r.outs[up]).is_empty(),
        "a pointer move that runs the layout past its limit ends with the freeze: its touch is gone"
    );
}

#[test]
fn text_placed_at_1e30_px() {
    let mut s = Script::new(70);
    s.ops(0.0, PAGE);
    s.ops(
        0.0,
        r#"[{"op":"el","id":64,"tag":"svg","ns":"svg","attrs":[["width","40"],["height","40"]]},{"op":"el","id":65,"tag":"text","ns":"svg","attrs":[["y","1e30"]]},
        {"op":"text","id":66,"data":"far"},{"op":"ins","parent":65,"id":66,"before":null},{"op":"ins","parent":64,"id":65,"before":null},{"op":"ins","parent":4,"id":64,"before":null},
        {"op":"el","id":60,"tag":"div","attrs":[["style","user-select:text"]]},{"op":"text","id":61,"data":"selectable"},{"op":"ins","parent":60,"id":61,"before":null},
        {"op":"el","id":62,"tag":"div","attrs":[["style","aspect-ratio:1/1e30"]]},{"op":"ins","parent":60,"id":62,"before":null},
        {"op":"text","id":63,"data":"below"},{"op":"ins","parent":60,"id":63,"before":null},{"op":"ins","parent":4,"id":60,"before":null}]"#,
    );
    s.after(16.0, Step::Paint);
    let painted = s.steps.len() - 1;
    let Some((r, _)) = replay(&s) else { return };
    let far = r.painted[painted].iter().flatten().any(|c| c.kind == Cmd::Text && c.text == b"far" && c.rect.y > 1e29);
    assert!(far, "text drawn at 1e30 px does not hang the paint of a document with selectable text");
}

// ---- The port's own -------------------------------------------------------------------------------------------

/// A host that answers nothing: for the checks that need no document.
struct Nothing;

impl EventHost for Nothing {
    fn now_ms(&mut self) -> f64 {
        0.0
    }
    fn width(&mut self) -> f64 {
        300.0
    }
    fn settle(&mut self, _doc: &mut Document, _out: Option<&mut Vec<Json>>) -> bool {
        false
    }
    fn rebuild(&mut self, _doc: &mut Document, _patch: bool) {}
    fn report_size(&mut self, _out: &mut Vec<Json>) {}
    fn restyled_by_input(&mut self, _doc: &mut Document) {}
    fn selection_changed(&mut self) {}
    fn hit(&mut self, _x: f64, _y: f64, _placement: Option<&mut twidget_wire::Rect>) -> i64 {
        0
    }
    fn box_of(&mut self, _id: i64) -> twidget_wire::Rect {
        twidget_wire::Rect::default()
    }
    fn has_element(&mut self, _id: i64) -> bool {
        false
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
        b" POINTER ".to_vec()
    }
    fn editor_style(&mut self, _id: i64, _clip: twidget_wire::Rect) -> Option<twidget_events::EditorStyle> {
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
    fn take_pending(&mut self, _out: &mut Vec<Json>) {}
    fn runs_key(&mut self) -> [u64; 4] {
        [0; 4]
    }
    fn text_runs(&mut self) -> Vec<twidget_events::TextRun> {
        Vec::new()
    }
    fn text_width(&mut self, _text: &[u8], _font: u64) -> f32 {
        0.0
    }
    fn font_info(&mut self, _font: u64) -> Option<(i32, f32, f32, f32)> {
        None
    }
    fn layer_shown(&mut self, _id: i64, _now: f64) -> Option<(twidget_wire::Affine, f32)> {
        None
    }
    fn selectable_key(&mut self) -> [u64; 2] {
        [0; 2]
    }
    fn any_selectable(&mut self) -> bool {
        false
    }
}

#[test]
fn listened_is_the_profile_with_its_always_forwarded_events() {
    let mut e = Events::new();
    assert!(
        e.listened(b"click") && e.listened(b"load") && e.listened(b"animationstart"),
        "click, load and the one-off animation events go out unasked"
    );
    assert!(!e.listened(b"pointermove") && !e.listened(b"animationiteration"), "the rest wait for a listener");
    e.receive(Json::parse(br#"{"type":"listen","types":["pointermove","touchstart","keydown","focus",7]}"#).as_ref().expect("JSON"));
    assert!(e.listened(b"pointermove"), "a listener opens its type");
    assert!(
        !e.listened(b"touchstart") && !e.listened(b"keydown") && !e.listened(b"focus"),
        "nothing outside profile 1, listened to or not"
    );
    assert_eq!(PROFILE_EVENTS.len(), 25);
}

#[test]
fn the_editor_and_cursor_without_a_document() {
    let mut doc = Document::new(false);
    let mut e = Events::new();
    assert_eq!(e.editor_json(), b"null", "nothing edited is null");
    assert!(e.editor().is_none() && e.hold_deadline(&doc).is_none());
    assert_eq!(e.cursor(&doc, &mut Nothing), b"pointer", "the cursor is trimmed and lowercased");
    assert!(e.edit(&mut doc, &mut Nothing, 0, b"x", b"insertText").is_empty(), "an edit with nothing edited is ignored");
    assert_eq!(e.last_input, 0.0, "but it counts as input");
    assert!(e.end_editing(&mut doc, &mut Nothing).is_empty());
    assert!(!e.clear_selection(&mut Nothing) && !e.has_selection(&mut Nothing), "no selection to clear");
    assert!(e.touch(&mut doc, &mut Nothing, 1, 0.0, 0.0, 0, true, true, 0, 0).is_empty(), "a move of a pointer never pressed is nothing");
    let claim = e.claim_at(&mut doc, &mut Nothing, 0.0, 0.0);
    assert!(!claim.x && !claim.y && !claim.hold, "an empty document claims nothing");
    e.receive(Json::parse(br#"{"type":"arbitrate","bubble":false,"touch":true,"rtl":true}"#).as_ref().expect("JSON"));
    assert_eq!(e.arbitration(), (false, false, true), "a mouse plays a finger only for a renderer playing the bubble");
}
