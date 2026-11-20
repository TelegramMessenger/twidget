//! tests/ends_tests.cpp's renderer-end tests, by name: the renderer end against a platform end over an
//! in-memory channel. Focused sequences match a renderer called directly step by step, and a missing or bad
//! answer, bad platform input or a value past the caps stops the end for good with nothing sent after.
//!
//! The renderer is the scripted one (tests/common/scripted.rs) until stage 4 brings the Rust renderer: each
//! session scripts the Backend calls, messages and state the C++ session's real renderer makes at that step,
//! and both arms run the same script, so what the C++ checks of the end holds here as it does there; the
//! renderer's own answers (its zero-filled pixels, its throttle, its clock) wait for stage 4.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::scripted::{Log, ScriptedRenderer};
use common::*;
use twidget_ends::{Output, Renderer};
use twidget_json::Json;
use twidget_remote::*;

/// What the C++ suite's PAGE makes the renderer do: a document font and its widths, a PNG, a canvas with a
/// font of its own and a batch; the size, a load event and a dirty document.
const PAGE: &str = r#"{"type":"ops","ops":[],"react":{"calls":[
 ["font","sans-serif",16,400,false,""],["width",0,"two"],["width",0,"ends"],
 ["image",12,"data:image/png;base64,iVBORw0KGgo="],
 ["cfont","serif",12,700,false],["width",1,"hi"],
 ["run",13,40,20,[{"k":4,"r":[0,10,0,0],"t":"hi","f":2},{"k":1,"r":[0,0,5,5],"c":4281558681,"cl":0},{"k":1,"r":[6,0,5,5],"cl":0}]]],
 "out":[{"type":"size","width":300,"height":40,"innerWidth":300,"innerHeight":40,"dpr":2},{"type":"event","ev":{"type":"load","targetId":12}}],
 "set":{"height":40,"dirty":true}}}"#;

/// The imageData query: the canvas read, and the result (helper-only).
const PIXELS: &str = r#"{"type":"query","qid":1,"query":{"q":"imageData","id":13,"x":0,"y":0,"w":2,"h":1},
 "react":{"calls":[["read",13,0,0,2,1]],"out":[{"type":"result","qid":1,"result":{"binaryBase64":"pixels"}}]}}"#;

/// A batch that waits for the next frame: it wants a tick, and makes no Backend call yet.
const SPINNING: &str = r#"{"type":"ops","ops":[],"react":{"calls":[["font","sans-serif",16,400,false,""]],"set":{"tick":true,"next":0,"height":20,"dirty":true}}}"#;

/// The construction every session makes: the default font.
const SCRIPT: &str = r#"{"ctor":{"calls":[["font","sans-serif",16,400,false,""]],"set":{"height":0}}}"#;

fn script() -> Json {
    parse(SCRIPT)
}

fn stale_output() -> Output {
    Output { messages: vec![parse(r#"{"type":"event","ev":{"type":"stale"}}"#)], frame: true }
}

fn empty(out: &Output) -> bool {
    out.messages.is_empty() && !out.frame
}

fn requests_made(backend: &LogHandle) -> usize {
    [Op::Font, Op::CanvasFont, Op::TextWidth, Op::Image, Op::CanvasRead].iter().map(|op| backend.count(*op)).sum()
}

/// Once stopped the end answers nothing, sends nothing and calls nothing, and goes without a word.
fn inert(lp: &mut Loopback, what: &str) {
    let reason = lp.end().stop_reason();
    let sent = lp.kinds().len();
    let calls = lp.backend.len();
    let heard = lp.events().len();
    let logged = lp.log.borrow().len();
    assert!(lp.end().stopped() && !reason.is_empty(), "{what}: the end says it stopped");
    assert!(lp.receive(&parse(PAGE)).is_empty(), "{what}: an engine message gets nothing back");
    lp.want_frame();
    lp.report(&[parse(r#"{"type":"size","width":1,"height":2}"#)]);
    let mut out = stale_output();
    assert!(
        !lp.end().deliver(&encoded(&FrameRequest { seq: 9, tick: true, paint: Paint::Force }), &mut out) && empty(&out),
        "{what}: a frame request gets false and an empty output"
    );
    {
        let mut p = lp.platform.borrow_mut();
        p.set_width(200.0);
        p.touch(0, 5.0, 5.0, 1, true, false, 0, 0);
        p.request_frame(true, Paint::Force);
        p.ask_claim(5.0, 5.0);
    }
    let delivered = lp.delivered.len();
    lp.out = stale_output();
    assert!(lp.pump(), "{what}: the platform input drains");
    let refused = lp.delivered.len() == delivered + 4 && (delivered..lp.delivered.len()).all(|i| !lp.delivered[i] && empty(&lp.outputs[i]));
    assert!(refused, "{what}: each platform input gets false and an empty output");
    assert!(
        lp.kinds().len() == sent && lp.wire.borrow().outbox.is_empty() && lp.backend.len() == calls && lp.events().len() == heard,
        "{what}: nothing goes out once stopped"
    );
    assert_eq!(lp.log.borrow().len(), logged, "{what}: the renderer is not called once the end stopped");
    assert!(lp.end().stopped() && lp.end().stop_reason() == reason, "{what}: the first reason stays");
    lp.close();
    assert!(lp.kinds().len() == sent && lp.events().len() == heard, "{what}: the end goes without a word");
    assert!(lp.backend.len() == calls, "{what}: the renderer's releases go nowhere");
    assert!(
        lp.host.borrow().refusals.is_empty() && lp.wire.borrow().refused == 0 && !lp.refusing(),
        "{what}: the platform end took all it was sent"
    );
}

/// The spoiled request was the last message of the call, and the end gives the expected reason.
fn stopped_at_spoil(lp: &mut Loopback, kind: Kind, reason: &str, what: &str) {
    let at = lp.wire.borrow().spoiled;
    let kinds = lp.kinds();
    assert!(at > 0 && kinds[at - 1] == kind as u8, "{what}: the spoiled request is the one named");
    assert_eq!(kinds.len(), at, "{what}: nothing goes out after the spoiled request");
    assert!(lp.end().stopped() && lp.end().stop_reason() == reason, "{what}: stops with \"{reason}\", not \"{}\"", lp.end().stop_reason());
}

const SPOILS: [Spoil; 6] = [Spoil::Unanswered, Spoil::Empty, Spoil::WrongKind, Spoil::WrongVersion, Spoil::Cut, Spoil::Trailing];

fn spoil_name(how: Spoil) -> &'static str {
    match how {
        Spoil::Unanswered => "unanswered",
        Spoil::Empty => "answered empty",
        Spoil::WrongKind => "answered as another request",
        Spoil::WrongVersion => "answered in another version",
        Spoil::Cut => "answered one byte short",
        Spoil::Trailing => "answered with a byte after",
        Spoil::Replaced => "answered otherwise",
        Spoil::None => "answered",
    }
}

fn reason_for(how: Spoil) -> &'static str {
    if how == Spoil::Unanswered { "a request got no answer" } else { "an answer did not decode" }
}

fn state_of<B: twidget_ends::Backend>(r: &mut ScriptedRenderer<B>, frame_asked: bool) -> State {
    let width = r.width();
    let height = r.viewport_height();
    let dirty = r.dirty();
    let wants_tick = r.wants_tick();
    let next_tick_in = r.next_tick_in();
    State { width, height, dirty, wants_tick, next_tick_in, frame_asked }
}

fn direct_renderer(script: &Json) -> (ScriptedRenderer<LogHandle>, LogHandle, Log) {
    let backend = LogHandle::new();
    let log = Log::default();
    (ScriptedRenderer::new(300.0, 2.0, false, backend.clone(), script, Rc::clone(&log)), backend, log)
}

/// A healthy session against a renderer called directly: construction, a page, pixels, a frame the engine
/// asked for, an unchanged frame and destruction.
#[test]
fn session() {
    let mut script = script();
    // As the renderer goes: the canvas, its font, then the document's fonts.
    script.set("drop", parse(r#"{"calls":[["release",13],["del",2],["del",0],["del",1]]}"#));
    script.set(
        "on",
        parse(
            r#"{"paint":[{"list":[{"k":0,"r":[0,0,300,40],"c":4294967295},{"k":4,"t":"two ends","f":1,"r":[0,16,0,0]},{"k":5,"r":[0,20,8,4],"i":12},{"k":5,"r":[0,24,40,20],"cv":13}]}],
            "damage":[{"damage":[[0,0,300,40]]}],
            "tick":[{"out":[{"type":"event","ev":{"type":"frame"}}]}]}"#,
        ),
    );
    let (mut direct, direct_backend, direct_log) = direct_renderer(&script);
    let mut lp = Loopback::new(Caps::default());
    lp.open(300.0, 2.0, false, &script);
    assert!(!lp.end().stopped() && lp.host.borrow().refusals.is_empty(), "the end starts healthy");
    assert!(
        lp.backend.len() > 0 && same_calls(&direct_backend.calls(), &lp.backend.calls()).is_ok(),
        "construction makes the direct renderer's Backend calls through the platform end"
    );
    assert!(
        lp.wire.borrow().requests > 0 && lp.wire.borrow().requests == requests_made(&lp.backend),
        "construction's requests are answered, one Backend call each"
    );
    let first = state_of(&mut direct, false);
    assert!(
        lp.host.borrow().states.len() == 1 && same_state(&lp.host.borrow().states[0], &first) && same_state(&lp.platform_state(), &first),
        "the first State is the renderer's state"
    );
    assert!(
        *lp.kinds().last().expect("a message") == Kind::State as u8 && lp.events() == vec![Kind::State],
        "the State is the constructor's last message and its only report"
    );

    let direct_page = direct.receive(&parse(PAGE));
    let page = lp.receive(&parse(PAGE));
    assert!(!page.is_empty() && dumps(&page) == dumps(&direct_page), "receive gives every renderer message of the page");
    assert!(
        lp.host.borrow().batches.len() == 1 && dumps(&lp.host.borrow().batches[0]) == dumps(&platform_only(&direct_page)),
        "the platform gets the page's platform messages alone"
    );
    same_calls(&direct_backend.calls(), &lp.backend.calls()).expect("the page's Backend calls arrive in order");
    assert!(
        lp.backend.count(Op::Font) > 0
            && lp.backend.count(Op::CanvasFont) == 1
            && lp.backend.count(Op::TextWidth) > 0
            && lp.backend.count(Op::Image) == 1
            && lp.backend.count(Op::CanvasRun) > 0,
        "the page makes document fonts, a marked canvas font, widths, an image and canvas batches"
    );

    let direct_pixels = direct.receive(&parse(PIXELS));
    let pixels = lp.receive(&parse(PIXELS));
    assert!(pixels.len() == 1 && dumps(&pixels) == dumps(&direct_pixels), "an imageData query answers");
    assert!(lp.backend.count(Op::CanvasRead) == 1, "the pixels are read once");
    same_calls(&direct_backend.calls(), &lp.backend.calls()).expect("the pixels are read in order");
    let read = |log: &Log| log.borrow().iter().find(|l| l.starts_with("  read")).cloned();
    assert!(
        read(&lp.log).is_some() && read(&lp.log) == read(&direct_log) && !read(&lp.log).unwrap_or_default().ends_with("-> 0 "),
        "the renderer gets the platform's pixels"
    );

    lp.want_frame();
    assert!(
        *lp.events().last().expect("an event") == Kind::State && lp.host.borrow().states.last().expect("a state").frame_asked,
        "the engine's want of a frame goes out as State"
    );
    let seq = lp.platform.borrow_mut().request_frame(true, Paint::IfDirty);
    lp.out = stale_output();
    assert!(lp.pump() && lp.delivered == vec![true], "the frame request is delivered");
    let ticked = direct.tick();
    let dirty = direct.dirty();
    let list = direct.paint();
    let mut rects = Vec::new();
    let damage = direct.damage(&mut rects).then_some(rects);
    direct_backend.0.borrow_mut().frame(true);
    assert!(
        lp.outputs.len() == 1 && lp.outputs[0].frame && dumps(&lp.outputs[0].messages) == dumps(&ticked),
        "the ticking frame gives the tick's messages and says the engine's frame is due"
    );
    {
        let host = lp.host.borrow();
        assert!(
            dirty && host.frames.len() == 1 && host.frames[0].seq == seq && host.frames[0].painted && !list.is_empty(),
            "the dirty page is painted"
        );
        let frame = &host.frames[0];
        assert_eq!(list_bytes(&frame.list), list_bytes(&list), "the frame's list is the direct paint");
        assert_eq!(sharing(&frame.list), sharing(&list), "the frame's list shares its payloads as the direct paint does");
        assert_eq!(damage_bytes(&frame.damage), damage_bytes(&damage), "the frame's damage is the direct damage");
        let after = state_of(&mut direct, false);
        assert!(
            !frame.state.frame_asked && same_state(&frame.state, &after),
            "the frame carries the state after it, the engine's want taken"
        );
    }
    assert!(same_state(&lp.platform_state(), &lp.host.borrow().frames[0].state), "the platform end keeps the frame's state");
    assert_eq!(*lp.events().last().expect("an event"), Kind::Frame, "no State follows the frame that carried it");
    same_calls(&direct_backend.calls(), &lp.backend.calls()).expect("the frame's Backend calls come before its marker");

    lp.platform.borrow_mut().request_frame(false, Paint::IfDirty);
    assert!(
        lp.pump() && lp.delivered.len() == 2 && lp.delivered[1] && empty(&lp.outputs[1]),
        "an unchanged frame reuses the Output with nothing of the frame before"
    );
    let _ = direct.dirty();
    direct_backend.0.borrow_mut().frame(false);
    {
        let host = lp.host.borrow();
        assert!(
            host.frames.len() == 2 && !host.frames[1].painted && host.frames[1].list.is_empty() && host.frames[1].damage.is_none(),
            "an unchanged frame is answered unpainted"
        );
    }
    same_calls(&direct_backend.calls(), &lp.backend.calls()).expect("the unchanged frame makes no Backend call");
    let heard = lp.events().len();
    lp.platform.borrow_mut().clear_selection();
    assert!(
        lp.pump() && lp.delivered.len() == 3 && lp.delivered[2] && lp.events().len() == heard,
        "after the frames a call that changes nothing sends no report"
    );

    let before = lp.backend.len();
    drop(direct);
    lp.close();
    same_calls(&direct_backend.calls(), &lp.backend.calls()).expect("destruction releases what the direct renderer releases, in its order");
    let calls = lp.backend.calls();
    let canvas_font = calls.iter().rev().find(|c| c.op == Op::CanvasFont).map_or(0, |c| c.font);
    let mut tail = Vec::new();
    let mut first_deleted = 0;
    for c in &calls[before..] {
        if c.op == Op::DeleteFont && first_deleted == 0 {
            first_deleted = c.font;
        }
        if tail.last() != Some(&c.op) {
            tail.push(c.op);
        }
    }
    assert!(
        tail == vec![Op::CanvasRelease, Op::DeleteFont] && first_deleted == canvas_font && lp.backend.0.borrow().live_fonts() == 0,
        "destruction releases the canvas, then its font, then every document font"
    );
    assert!(
        lp.host.borrow().refusals.is_empty() && lp.wire.borrow().refused == 0 && !lp.refusing(),
        "the platform end takes the whole session"
    );
}

/// The first request, made while the renderer is constructed, fails every way: the end stops before its
/// first State.
#[test]
fn construction_failures() {
    for how in SPOILS {
        let what = format!("request 1 {}", spoil_name(how));
        let mut lp = Loopback::new(Caps::default());
        lp.wire.borrow_mut().fault = Fault { how, at: 1, kind: None, reply: Vec::new() };
        lp.open(300.0, 1.0, false, &script());
        stopped_at_spoil(&mut lp, Kind::FontRequest, reason_for(how), &what);
        assert!(lp.kinds().len() == 1 && lp.events().is_empty(), "{what}: the constructor sends nothing after it");
        inert(&mut lp, &what);
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::FontRequest => "a font request",
        Kind::WidthRequest => "a width request",
        Kind::ImageRequest => "an image request",
        _ => "a pixels request",
    }
}

/// A later request of each kind fails every way inside an engine message, which then gets nothing back.
#[test]
fn later_failures() {
    let kinds = [Kind::FontRequest, Kind::WidthRequest, Kind::ImageRequest, Kind::PixelsRequest];
    let mut faults = Vec::new();
    for kind in kinds {
        for how in SPOILS {
            faults.push((Fault { how, at: 0, kind: Some(kind), reply: Vec::new() }, format!("{} {}", kind_name(kind), spoil_name(how))));
        }
    }
    faults.push((
        Fault {
            how: Spoil::Replaced,
            at: 0,
            kind: Some(Kind::PixelsRequest),
            reply: encoded_pixels(&PixelsAnswer { ok: true, rgba: vec![9; 4] }),
        },
        "a pixels request answered with 4 bytes for 8".to_string(),
    ));
    faults.push((
        Fault { how: Spoil::Replaced, at: 0, kind: Some(Kind::FontRequest), reply: encoded(&WidthAnswer { width: 8.0 }) },
        "a font request answered with a width".to_string(),
    ));
    for (fault, what) in faults {
        let mut lp = Loopback::new(Caps::default());
        lp.open(300.0, 1.0, false, &script());
        let pixels = fault.kind == Some(Kind::PixelsRequest);
        if pixels {
            lp.receive(&parse(PAGE));
        }
        let states = lp.host.borrow().states.len();
        let how = fault.how;
        let kind = fault.kind.expect("a kind");
        lp.wire.borrow_mut().fault = fault;
        assert!(lp.receive(&parse(if pixels { PIXELS } else { PAGE })).is_empty(), "{what}: the engine message gets nothing back");
        stopped_at_spoil(&mut lp, kind, reason_for(how), &what);
        assert!(
            lp.host.borrow().states.len() == states && lp.host.borrow().batches.len() == usize::from(pixels),
            "{what}: no report of the failed call goes out"
        );
        inert(&mut lp, &what);
    }
}

fn encoded_pixels(answer: &PixelsAnswer) -> Vec<u8> {
    let mut bytes = Vec::new();
    assert!(answer.encode(&Caps::default(), &mut bytes));
    bytes
}

/// A frame, a touch, a hover and a hold query each settle a batch that waited for the frame, whose first
/// width request goes unanswered.
#[test]
fn mid_call_failures() {
    let names = ["a ticking frame", "a touch", "a hover", "a hold query"];
    let settle = r#"{"calls":[["width",1,"words measured at the next frame"],["width",1,"more"]],"set":{"tick":false,"next":null}}"#;
    for (call, name) in names.iter().enumerate() {
        let what = format!("{name} whose build loses its answer");
        let mut lp = Loopback::new(Caps::default());
        lp.open(300.0, 1.0, false, &script());
        lp.receive(&parse(SPINNING));
        let widths = lp.count_kind(Kind::WidthRequest);
        lp.receive(&parse(r#"{"type":"ops","ops":[],"react":{"set":{"dirty":true}}}"#));
        assert!(
            lp.count_kind(Kind::WidthRequest) == widths && lp.end().renderer().wants_tick() && !lp.end().stopped(),
            "{what}: the batch waits for the frame"
        );
        lp.want_frame();
        let method = ["tick", "touch", "hover", "claim_at"][call];
        lp.end().renderer().push(method, parse(settle));
        lp.wire.borrow_mut().fault = Fault { how: Spoil::Unanswered, at: 0, kind: Some(Kind::WidthRequest), reply: Vec::new() };
        {
            let mut p = lp.platform.borrow_mut();
            match call {
                0 => {
                    p.request_frame(true, Paint::Force);
                }
                1 => p.touch(0, 5.0, 5.0, 1, true, false, 0, 0),
                2 => p.hover(0, 5.0, 5.0, 0),
                _ => {
                    p.ask_claim(5.0, 5.0);
                }
            }
        }
        lp.out = stale_output();
        let frames = lp.backend.count(Op::Frame);
        let runs = lp.backend.count(Op::CanvasRun);
        assert!(
            lp.pump() && lp.delivered == vec![false] && empty(&lp.outputs[0]) && empty(&lp.out),
            "{what}: the delivery gets false and an empty output, the engine's frame not due"
        );
        stopped_at_spoil(&mut lp, Kind::WidthRequest, "a request got no answer", &what);
        {
            let host = lp.host.borrow();
            assert!(
                host.frames.is_empty()
                    && host.pointers.is_empty()
                    && host.claims.is_empty()
                    && lp.backend.count(Op::Frame) == frames
                    && lp.backend.count(Op::CanvasRun) == runs,
                "{what}: no paint, frame, pointer or claim follows"
            );
        }
        if call == 0 {
            assert!(lp.end().renderer().dirty(), "{what}: the frame leaves its paint undone");
            assert!(!lp.log.borrow().iter().any(|l| l == "paint"), "{what}: the end paints nothing once stopped");
        }
        inert(&mut lp, &what);
    }
}

/// What a platform end never sends, or sends broken, stops the end at once.
#[test]
fn bad_input() {
    let frame_request = encoded(&FrameRequest { seq: 1, tick: true, paint: Paint::Force });
    let none = "a platform message of no known kind or version";
    let broken = "a platform message that did not decode";
    let mut version = frame_request.clone();
    version[1] = VERSION + 1;
    let mut paint = frame_request.clone();
    *paint.last_mut().expect("a byte") = 3;
    let mut theme = encoded(&SetTheme { dark: true });
    *theme.last_mut().expect("a byte") = 2;
    let mut trailing_clear = encoded(&ClearSelection);
    trailing_clear.push(0);
    let mut trailing_frame = frame_request.clone();
    trailing_frame.push(0);
    let cases: Vec<(&str, Vec<u8>, String)> = vec![
        ("an empty message", Vec::new(), none.into()),
        ("a kind byte alone", frame_request[..1].to_vec(), none.into()),
        ("an unknown kind", vec![9, 1], none.into()),
        ("a frame request of another version", version, none.into()),
        ("a frame request one byte short", frame_request[..frame_request.len() - 1].to_vec(), broken.into()),
        ("a frame request with a byte after it", trailing_frame, broken.into()),
        ("a frame request painting past Force", paint, broken.into()),
        ("a theme flag of 2", theme, broken.into()),
        ("a clear selection with a byte after it", trailing_clear, broken.into()),
        ("a State", encoded(&State::default()), "a message of kind 65 where platform input belongs".into()),
        ("a CanvasRelease", encoded(&CanvasRelease { id: 1 }), "a message of kind 72 where platform input belongs".into()),
        ("a FontAnswer outside a request", encoded(&FontAnswer::default()), "a message of kind 32 where platform input belongs".into()),
        (
            "a PixelsAnswer outside a request",
            encoded_pixels(&PixelsAnswer::default()),
            "a message of kind 35 where platform input belongs".into(),
        ),
        ("a FontRequest", encoded(&FontRequest::default()), "a message of kind 96 where platform input belongs".into()),
    ];
    for (what, bytes, reason) in cases {
        let mut lp = Loopback::new(Caps::default());
        lp.open(300.0, 1.0, false, &script());
        let sent = lp.kinds().len();
        let mut out = stale_output();
        assert!(!lp.end().deliver(&bytes, &mut out) && empty(&out), "{what}: false and an empty output");
        assert_eq!(lp.end().stop_reason(), reason, "{what}: stops with the reason");
        assert_eq!(lp.kinds().len(), sent, "{what}: nothing goes out");
        inert(&mut lp, what);
    }
    let mut lp = Loopback::new(Caps::default());
    lp.open(300.0, 1.0, false, &script());
    let mut out = stale_output();
    assert!(
        lp.end().deliver(&frame_request, &mut out) && !out.frame && out.messages.is_empty() && !lp.end().stopped(),
        "the same frame request whole is taken, into a fresh Output"
    );
}

/// A value past the caps stops the end: a painted list during a frame the engine asked for, a report, a
/// request's text; a URL past its cap and pixels past theirs do not.
#[test]
fn cap_failures() {
    {
        let what = "a painted list past the command cap";
        let mut caps = Caps::default();
        caps.values.commands = 1;
        let mut lp = Loopback::new(caps);
        lp.open(300.0, 1.0, false, &script());
        lp.receive(&parse(PAGE));
        lp.end().renderer().push("paint", parse(r#"{"list":[{"k":0,"r":[0,0,1,1]},{"k":0,"r":[1,1,1,1]}]}"#));
        lp.want_frame();
        lp.platform.borrow_mut().request_frame(true, Paint::Force);
        lp.out = stale_output();
        assert!(
            !lp.end().stopped() && lp.pump() && lp.delivered == vec![false] && empty(&lp.outputs[0]),
            "{what}: the frame gets false and an empty output, the engine's frame not due"
        );
        assert!(
            lp.end().stop_reason() == "a message past the caps" && lp.count_kind(Kind::Frame) == 0 && lp.host.borrow().frames.is_empty(),
            "{what}: stops past the caps with no frame sent"
        );
        inert(&mut lp, what);
    }
    {
        let what = "a report past the count cap";
        let caps = Caps { messages: 0, ..Caps::default() };
        let mut lp = Loopback::new(caps);
        lp.open(300.0, 1.0, false, &script());
        assert!(!lp.end().stopped() && lp.receive(&parse(PAGE)).is_empty(), "{what}: the page gets nothing back");
        assert!(
            lp.end().stop_reason() == "a message past the caps"
                && lp.host.borrow().batches.is_empty()
                && lp.count_kind(Kind::Messages) == 0,
            "{what}: stops past the caps with no report sent"
        );
        inert(&mut lp, what);
    }
    {
        let caps = Caps { url: 16, ..Caps::default() };
        let mut lp = Loopback::new(caps);
        lp.open(300.0, 1.0, false, &script());
        lp.receive(&parse(PAGE));
        assert!(
            !lp.end().stopped() && lp.count_kind(Kind::ImageRequest) == 0 && lp.backend.count(Op::Image) == 0,
            "an image URL past the cap fails to load without a request, and the end goes on"
        );
        assert!(
            lp.log.borrow().iter().any(|l| l.starts_with("  image 12 -> 0 00000000 00000000")),
            "the renderer gets false and its sizes as they were"
        );
        let at = Caps { url: "data:image/png;base64,iVBORw0KGgo=".len(), ..Caps::default() };
        let mut lp = Loopback::new(at);
        lp.open(300.0, 1.0, false, &script());
        lp.receive(&parse(PAGE));
        assert!(lp.count_kind(Kind::ImageRequest) == 1 && lp.backend.count(Op::Image) == 1, "an image URL as long as the cap is requested");
    }
    {
        let mut pixels = Caps::default();
        pixels.values.pixels = 4;
        let room = Caps { message: 64 << 10, ..Caps::default() };
        let big = r#"{"type":"query","qid":1,"query":{"q":"imageData"},"react":{"calls":[["read",13,0,0,120,140]],"out":[{"type":"result","qid":1}]}}"#;
        for (what, caps, query) in [("pixels past the pixel cap", pixels, PIXELS), ("pixels past the message room", room, big)] {
            let mut lp = Loopback::new(caps);
            lp.open(300.0, 1.0, false, &script());
            lp.receive(&parse(PAGE));
            let asked = lp.count_kind(Kind::PixelsRequest);
            let answer = lp.receive(&parse(query));
            assert!(!lp.end().stopped() && !lp.refusing() && lp.host.borrow().refusals.is_empty(), "{what}: both ends go on");
            assert!(
                lp.count_kind(Kind::PixelsRequest) == asked + 1 && lp.backend.count(Op::CanvasRead) == 0,
                "{what}: the request goes out and the platform reads nothing"
            );
            assert!(
                answer.len() == 1 && lp.log.borrow().iter().any(|l| l == "  read 13 -> 0 "),
                "{what}: the renderer is told it got no pixels"
            );
        }
    }
    {
        let what = "a width request past the text cap";
        let caps = Caps { text: 64, ..Caps::default() };
        let mut lp = Loopback::new(caps);
        lp.open(300.0, 1.0, false, &script());
        let word = "w".repeat(100);
        assert!(!lp.end().stopped(), "{what}: the end starts");
        let ops = format!(
            r#"{{"type":"ops","ops":[],"react":{{"calls":[["width",0,"{word}"]],"out":[{{"type":"size","width":300,"height":20}}]}}}}"#
        );
        assert!(lp.receive(&parse(&ops)).is_empty(), "{what}: the batch gets nothing back");
        let measured = lp.backend.calls().iter().any(|c| c.text == word.as_bytes());
        assert!(
            lp.end().stop_reason() == "a request past the caps" && !measured,
            "{what}: stops past the caps before the platform measures it"
        );
        inert(&mut lp, what);
    }
}

// ---- a renderer called directly against the two ends ----------------------------------------------

/// A check of a session step: the condition is evaluated before the check borrows the session.
macro_rules! ok {
    ($a:expr, $cond:expr, $what:expr $(,)?) => {{
        let cond = $cond;
        let what = $what;
        $a.ok(cond, &what)
    }};
}

const NO_CLAIM: GestureClaim = GestureClaim { x: false, y: false, hold: false };
const HOLD_CLAIM: GestureClaim = GestureClaim { x: true, y: false, hold: true };
const DRAG_CLAIM: GestureClaim = GestureClaim { x: true, y: false, hold: false };

fn has_event(messages: &[Json], kind: &str) -> bool {
    messages.iter().any(|m| m.get("type").as_str() == b"event" && m.get("ev").get("type").as_str() == kind.as_bytes())
}

/// What a session made happen, by kind.
#[derive(Debug, Default)]
struct Tally {
    steps: usize,
    construction: usize,
    destruction: usize,
    painted: usize,
    unchanged: usize,
    helper_only: usize,
    calls: [usize; 8],
    between: [usize; 8],
    messages: usize,
    pointers: usize,
    selections: usize,
    frames: usize,
    claims: usize,
    states: usize,
}

/// One focused sequence made of calls, each on a renderer called directly, which gives what is expected, and
/// then through the two ends; every step compares the two at once: the reports, the Backend calls and the
/// renderer's own log of the calls the end made of it.
struct Arms {
    name: String,
    label: String,
    direct: Option<ScriptedRenderer<LogHandle>>,
    direct_backend: LogHandle,
    direct_log: Log,
    lp: Loopback,
    asked: bool,
    selection: Vec<u8>,
    sent: State,
    requested: usize,
    tally: Tally,
}

type Step = Expected;

impl Arms {
    fn new(name: &str, caps: Caps) -> Arms {
        let script = script();
        let (direct, direct_backend, direct_log) = direct_renderer(&script);
        let mut lp = Loopback::new(caps);
        lp.open(300.0, 2.0, false, &script);
        let mut a = Arms {
            name: name.to_string(),
            label: "construction".to_string(),
            direct: Some(direct),
            direct_backend,
            direct_log,
            lp,
            asked: false,
            selection: Vec::new(),
            sent: State::default(),
            requested: 0,
            tally: Tally::default(),
        };
        a.tally.construction = a.direct_backend.len();
        let calls_ok = same_calls(&a.direct_backend.calls(), &a.lp.backend.calls());
        ok!(a, a.lp.backend.len() > 0 && calls_ok.is_ok(), &format!("construction makes the direct renderer's Backend calls {calls_ok:?}"));
        // The reads the end's first finish() made, in its order.
        let _ = a.direct().may_have_selection();
        a.sent = state_of(a.direct.as_mut().expect("a renderer"), false);
        let sent = a.sent;
        ok!(
            a,
            a.lp.events() == vec![Kind::State]
                && same_state(a.lp.host.borrow().states.last().expect("a state"), &sent)
                && same_state(&a.lp.platform_state(), &sent),
            "the constructor's one report is the direct renderer's State",
        );
        a.healthy();
        a
    }

    fn ok(&self, cond: bool, what: &str) {
        assert!(cond, "{} step {} ({}): {what}", self.name, self.tally.steps, self.label);
    }

    fn direct(&mut self) -> &mut ScriptedRenderer<LogHandle> {
        self.direct.as_mut().expect("a renderer")
    }

    /// The next call of `method` reacts so, in both arms.
    fn react(&mut self, method: &str, reaction: &str) {
        let r = parse(reaction);
        self.direct().push(method, r.clone());
        self.lp.end().renderer().push(method, r);
    }

    fn healthy(&mut self) {
        let stopped = self.lp.end().stopped();
        let reason = self.lp.end().stop_reason();
        let refusals = self.lp.host.borrow().refusals.clone();
        let quiet = self.lp.wire.borrow().refused == 0 && !self.lp.refusing() && self.lp.queue.0.borrow().is_empty();
        ok!(self, !stopped && refusals.is_empty() && quiet, &format!("both ends stay healthy: {reason} {refusals:?}"));
    }

    /// The reads finish() makes after the call, in its order, then the frame's marker.
    fn expect(&mut self, s: &mut Step) {
        let asked = self.asked;
        let d = self.direct();
        s.selection = if d.may_have_selection() { d.selected_text() } else { Vec::new() };
        s.state = state_of(d, asked);
        predict(s, &self.selection, &self.sent);
        if let Some(frame) = &s.frame {
            self.direct_backend.0.borrow_mut().frame(frame.painted);
        }
    }

    fn compare(&mut self, s: &Step, heard_before: usize, returned: &[Json], out: Option<&Output>) {
        self.healthy();
        ok!(self, dumps(returned) == dumps(&s.messages), "the helper gets every message the direct renderer gave");
        if let Some(out) = out {
            ok!(self, out.frame == s.engine, &format!("the engine's frame is {}", if s.engine { "due" } else { "not due" }));
        }
        let heard: Vec<Kind> = self.lp.events()[heard_before..].to_vec();
        ok!(self, heard == s.events, &format!("the host hears {:?}, not {heard:?}", s.events));
        {
            let host = self.lp.host.borrow();
            for kind in &heard {
                match kind {
                    Kind::Messages => ok!(
                        self,
                        dumps(host.batches.last().expect("a batch")) == dumps(&platform_only(&s.messages)),
                        "the Messages report holds the call's platform messages alone"
                    ),
                    Kind::Pointer => {
                        let p = host.pointers.last().expect("a pointer");
                        let e = s.pointer.as_ref().expect("a pointer");
                        ok!(self, p.cursor == e.cursor && p.claim == e.claim, &format!("the Pointer report is {e:?}, not {p:?}"));
                    }
                    Kind::Selection => ok!(
                        self,
                        host.selections.last().expect("a selection").text == s.selection,
                        "the Selection report is the direct selected text"
                    ),
                    Kind::Frame => {
                        let f = host.frames.last().expect("a frame");
                        let e = s.frame.as_ref().expect("a frame");
                        ok!(self, f.seq == s.seq && f.painted == e.painted, &format!("the Frame answers request {}", s.seq));
                        ok!(self, list_bytes(&f.list) == list_bytes(&e.list), "the frame's list is the direct paint");
                        ok!(self, sharing(&f.list) == sharing(&e.list), "the frame's list shares its payloads as the direct paint does");
                        ok!(self, damage_bytes(&f.damage) == damage_bytes(&e.damage), "the frame's damage is the direct damage");
                        ok!(self, same_state(&f.state, &s.state), "the Frame carries the direct state");
                    }
                    Kind::ClaimAnswer => {
                        let c = host.claims.last().expect("a claim");
                        ok!(
                            self,
                            c.seq == s.seq && Some(c.claim) == s.claim,
                            &format!("the ClaimAnswer is {:?}, not {:?}", s.claim, c.claim)
                        );
                    }
                    Kind::State => {
                        ok!(self, same_state(host.states.last().expect("a state"), &s.state), "the State report is the direct state")
                    }
                    _ => {}
                }
            }
        }
        ok!(self, same_state(&self.lp.platform_state(), &s.state), "the platform end's state is the direct renderer's");
        let calls = same_calls(&self.direct_backend.calls(), &self.lp.backend.calls());
        ok!(self, calls.is_ok(), &format!("the Backend calls are the direct renderer's, in order: {calls:?}"));
        let logs_alike = *self.direct_log.borrow() == *self.lp.log.borrow();
        ok!(self, logs_alike, "the end calls its renderer as the step called the direct one, in the same order");
        self.selection = s.selection.clone();
        self.sent = s.state;
        self.tally.helper_only += s.messages.iter().filter(|m| !platform_type(m)).count();
    }

    fn begin(&mut self, what: &str) {
        self.tally.steps += 1;
        self.label = what.to_string();
    }

    /// Platform input: `call` makes the direct call and its reads into the step, `send` hands the same input to
    /// the platform end and returns its seq.
    fn input(&mut self, what: &str, call: impl FnOnce(&mut Arms, &mut Step), send: impl FnOnce(&mut Platform) -> u64) -> Step {
        self.begin(what);
        let mut s = Step::default();
        call(self, &mut s);
        self.expect(&mut s);
        let heard = self.lp.events().len();
        let delivered = self.lp.delivered.len();
        s.seq = send(&mut self.lp.platform.borrow_mut());
        let pumped = self.lp.pump();
        ok!(
            self,
            pumped && self.lp.delivered.len() == delivered + 1 && *self.lp.delivered.last().expect("a delivery"),
            "the platform input is delivered and taken"
        );
        let out = self.lp.outputs.last().cloned().unwrap_or_default();
        self.compare(&s, heard, &out.messages, Some(&out));
        s
    }

    fn receive(&mut self, json: &str, what: &str) -> Step {
        self.begin(what);
        let message = parse(json);
        let mut s = Step { messages: self.direct().receive(&message), ..Step::default() };
        self.expect(&mut s);
        let heard = self.lp.events().len();
        let got = self.lp.receive(&message);
        self.compare(&s, heard, &got, None);
        s
    }

    fn want_frame(&mut self, what: &str) -> Step {
        self.begin(what);
        let mut s = Step::default();
        self.asked = true;
        self.expect(&mut s);
        let heard = self.lp.events().len();
        let frames = self.lp.host.borrow().frames.len();
        self.lp.want_frame();
        self.compare(&s, heard, &[], None);
        ok!(self, self.lp.host.borrow().frames.len() == frames, "the engine's want of a frame makes no frame for the host");
        s
    }

    fn frame(&mut self, what: &str, tick: bool, paint: Paint) -> Step {
        self.requested += 1;
        self.input(
            what,
            |a, s| {
                if tick {
                    s.messages = a.direct().tick();
                    s.engine = std::mem::replace(&mut a.asked, false);
                }
                let mut f = Frame::default();
                if paint == Paint::Force || (paint == Paint::IfDirty && a.direct().dirty()) {
                    f.list = a.direct().paint();
                    let mut rects = Vec::new();
                    if a.direct().damage(&mut rects) {
                        f.damage = Some(rects);
                    }
                    f.painted = true;
                }
                s.frame = Some(f);
            },
            |p| p.request_frame(tick, paint),
        )
    }

    fn touch(&mut self, what: &str, action: i32, x: f64, y: f64, pointer: i32, primary: bool, mouse: bool, button: i32) -> Step {
        self.input(
            what,
            |a, s| {
                s.messages = a.direct().touch(action, x, y, pointer, primary, mouse, 0, button);
                s.pointer = Some(Pointer { cursor: a.direct().cursor(), claim: gesture_of(&s.messages) });
            },
            |p| {
                p.touch(action, x, y, pointer, primary, mouse, 0, button);
                0
            },
        )
    }

    fn hover(&mut self, what: &str, action: i32, x: f64, y: f64) -> Step {
        self.input(
            what,
            |a, s| {
                s.messages = a.direct().hover(action, x, y, 0);
                s.pointer = Some(Pointer { cursor: a.direct().cursor(), claim: None });
            },
            |p| {
                p.hover(action, x, y, 0);
                0
            },
        )
    }

    fn set_width(&mut self, what: &str, width: f64) -> Step {
        self.input(
            what,
            |a, s| a.direct().set_width(width, &mut s.messages),
            |p| {
                p.set_width(width);
                0
            },
        )
    }

    fn set_max_height(&mut self, what: &str, height: f64) -> Step {
        self.input(
            what,
            |a, s| a.direct().set_max_height(height, &mut s.messages),
            |p| {
                p.set_max_height(height);
                0
            },
        )
    }

    fn set_theme(&mut self, what: &str, dark: bool) -> Step {
        self.input(
            what,
            |a, s| a.direct().set_theme(dark, &mut s.messages),
            |p| {
                p.set_theme(dark);
                0
            },
        )
    }

    fn clear_selection(&mut self, what: &str) -> Step {
        self.input(
            what,
            |a, _| {
                a.direct().clear_selection();
            },
            |p| {
                p.clear_selection();
                0
            },
        )
    }

    fn claim(&mut self, what: &str, x: f64, y: f64) -> Step {
        self.input(what, |a, s| s.claim = Some(a.direct().claim_at(x, y)), |p| p.ask_claim(x, y))
    }

    /// A call the channel does not carry, made on each renderer by the helper, which then hands the end's
    /// renderer messages to report().
    fn helper(&mut self, what: &str, f: impl Fn(&mut dyn RendererCalls) -> Vec<Json>) -> Step {
        self.begin(what);
        let mut s = Step { messages: f(self.direct()), ..Step::default() };
        self.expect(&mut s);
        let heard = self.lp.events().len();
        ok!(self, !self.lp.end().stopped(), "the helper calls the renderer of a healthy end");
        let got = f(self.lp.end().renderer());
        self.lp.report(&got);
        self.compare(&s, heard, &got, None);
        s
    }

    /// Both arms' time moves on; that alone is no call, so neither does or reports anything.
    fn advance(&mut self) {
        let sent = self.lp.kinds().len();
        let heard = self.lp.events().len();
        let calls = self.lp.backend.len();
        let direct_calls = self.direct_backend.len();
        ok!(
            self,
            self.lp.kinds().len() == sent
                && self.lp.events().len() == heard
                && self.lp.backend.len() == calls
                && self.direct_backend.len() == direct_calls,
            "time moving on alone sends, reports and calls nothing",
        );
    }

    /// Drops both renderers while their Backends live, compares what they released, and counts the session.
    fn close(&mut self) {
        self.begin("destruction");
        let before = self.direct_backend.len();
        let heard = self.lp.events().len();
        self.direct = None;
        self.lp.close();
        self.tally.destruction = self.direct_backend.len() - before;
        let calls = same_calls(&self.direct_backend.calls(), &self.lp.backend.calls());
        ok!(self, calls.is_ok(), &format!("destruction releases what the direct renderer releases, in its order: {calls:?}"));
        let quiet = self.lp.events().len() == heard
            && self.lp.host.borrow().refusals.is_empty()
            && self.lp.wire.borrow().refused == 0
            && !self.lp.refusing();
        ok!(self, quiet, "destruction reports nothing and the platform end takes all of it");
        ok!(
            self,
            self.lp.backend.0.borrow().live_fonts() == 0 && self.direct_backend.0.borrow().live_fonts() == 0,
            "no font outlives its renderer"
        );
        let frames = self.lp.host.borrow().frames.len();
        ok!(
            self,
            frames == self.requested,
            &format!("the host got a frame for each request and no other ({frames} for {})", self.requested)
        );
        let calls = self.lp.backend.calls();
        let marks: Vec<usize> = (0..calls.len()).filter(|&i| calls[i].op == Op::Frame).collect();
        for (i, c) in calls.iter().enumerate() {
            if c.op == Op::Frame {
                continue;
            }
            self.tally.calls[c.op as usize] += 1;
            if matches!(c.op, Op::DeleteFont | Op::CanvasRun | Op::CanvasRelease)
                && marks.first().is_some_and(|f| i > *f)
                && marks.last().is_some_and(|l| i < *l)
            {
                self.tally.between[c.op as usize] += 1;
            }
        }
        for f in &self.lp.host.borrow().frames {
            if f.painted {
                self.tally.painted += 1;
            } else {
                self.tally.unchanged += 1;
            }
        }
        for kind in self.lp.events() {
            match kind {
                Kind::Messages => self.tally.messages += 1,
                Kind::Pointer => self.tally.pointers += 1,
                Kind::Selection => self.tally.selections += 1,
                Kind::Frame => self.tally.frames += 1,
                Kind::ClaimAnswer => self.tally.claims += 1,
                Kind::State => self.tally.states += 1,
                _ => {}
            }
        }
        eprintln!("{}: {:?}", self.name, self.tally);
    }

    fn need(&self, n: usize, what: &str) {
        assert!(n > 0, "{}: the session has {what}", self.name);
    }
}

/// The renderer calls a helper makes, on either arm's renderer.
trait RendererCalls {
    fn receive_json(&mut self, message: &Json) -> Vec<Json>;
}

impl<B: twidget_ends::Backend> RendererCalls for ScriptedRenderer<B> {
    fn receive_json(&mut self, message: &Json) -> Vec<Json> {
        self.receive(message)
    }
}

/// Text, a PNG and two canvases (13 and 14), the first with a clip its two fills share and a putImageData.
const RESOURCES: &str = r#"{"type":"ops","ops":[],"react":{"calls":[
 ["font","sans-serif",16,400,false,""],["width",1,"two"],["width",1,"ends"],
 ["image",12,"data:image/png;base64,iVBORw0KGgo="],
 ["cfont","serif",12,700,false],["width",2,"hi"],
 ["run",13,40,20,[{"k":0},{"k":4,"r":[0,10,0,0],"t":"hi","f":3},{"k":1,"r":[0,0,5,5],"c":4281558681,"cl":0},{"k":1,"r":[6,0,5,5],"c":4281558681,"cl":0},{"k":6,"r":[1,1,0,0],"px":[2,1,0]}]],
 ["cfont","monospace",10,400,false],
 ["run",14,30,10,[{"k":0},{"k":4,"r":[0,8,0,0],"t":"ok","f":4}]]],
 "out":[{"type":"size","width":300,"height":60,"innerWidth":300,"innerHeight":60,"dpr":2}],
 "set":{"height":60,"dirty":true}}}"#;

fn many_fonts(canvas: i32, count: usize) -> String {
    let mut calls = Vec::new();
    for i in 1..=count {
        calls.push(format!(r#"["cfont","serif",{i},400,false],["width",{},"f"]"#, 4 + i));
    }
    format!(
        r#"{{"type":"ops","ops":[],"react":{{"calls":[{},["run",{canvas},40,20,[{{"k":4,"t":"f","f":5}}]]],"set":{{"dirty":true}}}}}}"#,
        calls.join(",")
    )
}

/// Fonts, widths, an image, canvases, pixels, the canvas-font purge between frames, a canvas removed and one
/// kept until destruction, and false or wrong-sized answers.
#[test]
fn resources() {
    let mut a = Arms::new("resources", Caps::default());
    let page = a.receive(RESOURCES, "a page with a text, a PNG and two canvases");
    ok!(a, !platform_only(&page.messages).is_empty(), "the page reports its size");
    a.receive(
        r#"{"type":"query","qid":1,"query":{"q":"measureText"},"react":{"calls":[["cfont","serif",14,400,true],["width",5,"wide"]],"out":[{"type":"result","qid":1}]}}"#,
        "measureText in a font of its own",
    );
    a.receive(
        r#"{"type":"query","qid":2,"query":{"q":"imageData"},"react":{"calls":[["read",13,0,0,2,1]],"out":[{"type":"result","qid":2}]}}"#,
        "imageData of the first canvas",
    );
    a.react("paint", r#"{"list":[{"k":0,"r":[0,0,300,60],"c":4294967295},{"k":4,"t":"two ends","f":1},{"k":5,"r":[0,16,8,4],"i":12},{"k":5,"r":[0,20,40,20],"cv":13},{"k":5,"r":[0,40,30,10],"cv":14}]}"#);
    ok!(a, a.frame("the first frame", true, Paint::IfDirty).frame.as_ref().expect("a frame").painted, "the page is painted");
    a.receive(&many_fonts(13, 65), "65 more canvas font spellings in one batch");
    a.react("paint", r#"{"list":[{"k":5,"r":[0,20,40,20],"cv":13}]}"#);
    a.react("damage", r#"{"damage":[[0,20,40,20]]}"#);
    a.frame("the frame that draws them", true, Paint::IfDirty);
    let released = a.lp.backend.count(Op::DeleteFont);
    let mut purge: Vec<String> = (2..71).map(|k| format!(r#"["del",{k}]"#)).collect();
    purge.push(r#"["cfont","monospace",9,400,false]"#.to_string());
    purge.push(r#"["run",14,30,10,[{"k":4,"t":"again","f":71}]]"#.to_string());
    a.receive(
        &format!(r#"{{"type":"ops","ops":[],"react":{{"calls":[{}],"set":{{"dirty":true}}}}}}"#, purge.join(",")),
        "the next batch, past 64 cached canvas fonts",
    );
    ok!(a, a.lp.backend.count(Op::DeleteFont) >= released + 65, "the batch first releases every cached canvas font");
    a.frame("the frame after the purge", true, Paint::IfDirty);
    a.receive(
        r#"{"type":"ops","ops":[{"op":"rm","id":14}],"react":{"calls":[["release",14]],"set":{"dirty":true}}}"#,
        "the second canvas removed",
    );
    ok!(a, a.lp.backend.count(Op::CanvasRelease) == 1, "its canvas is released at once");
    a.frame("the frame without it", true, Paint::IfDirty);
    ok!(a, !a.frame("a frame with nothing new", false, Paint::IfDirty).frame.expect("a frame").painted, "an unchanged frame is unpainted");
    a.direct_backend.0.borrow_mut().images_load = false;
    a.lp.backend.0.borrow_mut().images_load = false;
    a.receive(
        r#"{"type":"ops","ops":[],"react":{"calls":[["image",15,"data:image/png;base64,iVBORw0KGgoAAAA="]],"set":{"dirty":true}}}"#,
        "a PNG the platform does not load",
    );
    a.direct_backend.0.borrow_mut().reads = Read::WrongSize;
    a.lp.backend.0.borrow_mut().reads = Read::WrongSize;
    a.receive(
        r#"{"type":"query","qid":4,"react":{"calls":[["read",13,0,0,2,1]],"out":[{"type":"result","qid":4}]}}"#,
        "imageData answered with the wrong size",
    );
    ok!(a, a.lp.log.borrow().iter().filter(|l| *l == "  read 13 -> 0 ").count() == 1, "wrong-sized pixels reach the renderer as none");
    a.direct_backend.0.borrow_mut().reads = Read::Unsupported;
    a.lp.backend.0.borrow_mut().reads = Read::Unsupported;
    a.receive(
        r#"{"type":"query","qid":5,"react":{"calls":[["read",13,1,0,1,2]],"out":[{"type":"result","qid":5}]}}"#,
        "imageData the platform cannot read",
    );
    a.frame("the last frame", true, Paint::IfDirty);

    let calls = a.lp.backend.calls();
    let images_failed =
        calls.iter().any(|c| c.op == Op::Image && !c.ok && c.w_in == 0.0 && c.h_in == 0.0 && c.image_w == 2.0 && c.image_h == 3.0);
    let shared_clip = calls.iter().any(|c| {
        c.op == Op::CanvasRun
            && c.ops.len() > 3
            && match (&c.ops[2].clips, &c.ops[3].clips) {
                (Some(x), Some(y)) => std::sync::Arc::ptr_eq(x, y),
                _ => false,
            }
    });
    let put = calls.iter().any(|c| c.op == Op::CanvasRun && c.ops.iter().any(|o| o.rgba.as_ref().is_some_and(|p| p.len() == 8)));
    ok!(a, shared_clip && put, "a batch's fills share its clip and its putImageData carries its pixels");
    ok!(a, images_failed, "a false image answer leaves the sizes the platform wrote into initialized outputs");
    ok!(a, a.lp.log.borrow().iter().any(|l| l == "  image 15 -> 0 40000000 40400000"), "and the renderer gets them");
    a.close();
    for (op, name) in
        ["document fonts", "canvas fonts", "font releases", "widths", "images", "canvas runs", "pixel reads", "canvas releases"]
            .iter()
            .enumerate()
    {
        a.need(a.tally.calls[op], name);
    }
    a.need(a.tally.construction, "Backend calls in construction");
    a.need(a.tally.destruction, "Backend calls in destruction");
    a.need(a.tally.between[Op::DeleteFont as usize], "font releases between frames");
    a.need(a.tally.between[Op::CanvasRun as usize], "canvas runs between frames");
    a.need(a.tally.between[Op::CanvasRelease as usize], "a canvas release between frames");
    a.need(a.tally.painted, "painted frames");
    a.need(a.tally.unchanged, "unchanged frames");
    a.need(a.tally.messages, "Messages reports");
    a.need(a.tally.states, "State reports");
}

/// Frames as the host asks for them: ticking or not, painted if dirty, forced or skipped, over a finite
/// animation, and the engine's own frame request.
#[test]
fn frames() {
    let mut a = Arms::new("frames", Caps::default());
    a.receive(
        r#"{"type":"ops","ops":[],"react":{"calls":[["font","sans-serif",16,400,false,""]],"set":{"dirty":true,"tick":true,"height":20}}}"#,
        "a box that fades in for 100 ms",
    );
    a.receive(r#"{"type":"listen","types":["animationstart","animationend"]}"#, "the widget listens to its animation");
    a.react("tick", r#"{"out":[{"type":"event","ev":{"type":"animationstart","targetId":10}}],"set":{"dirty":true}}"#);
    a.react("paint", r#"{"list":[{"k":12},{"k":0,"r":[0,0,20,20],"c":4281558681},{"k":13}]}"#);
    let first = a.frame("the first ticking frame", true, Paint::IfDirty);
    ok!(
        a,
        first.frame.as_ref().expect("a frame").painted && !first.engine && first.state.wants_tick,
        "it is painted and the animation wants more"
    );
    let clean = a.frame("a clean frame without a tick", false, Paint::IfDirty);
    ok!(a, !clean.frame.expect("a frame").painted, "a clean IfDirty frame is unpainted");
    let wanted = a.want_frame("the engine asks for a frame");
    ok!(a, wanted.state.frame_asked && wanted.events == vec![Kind::State], "the want goes out as State alone");
    let forced = a.frame("a forced frame without a tick", false, Paint::Force);
    ok!(
        a,
        forced.frame.as_ref().expect("a frame").painted && !forced.engine && forced.state.frame_asked,
        "Force paints and the engine's want survives a frame that does not tick"
    );
    a.react("tick", r#"{"set":{"dirty":true}}"#);
    let skipped = a.frame("a ticking frame that skips the paint", true, Paint::Skip);
    ok!(
        a,
        !skipped.frame.as_ref().expect("a frame").painted && skipped.engine && !skipped.state.frame_asked,
        "Skip paints nothing and the tick takes the engine's want once"
    );
    let mut painted = 0;
    for frame in 0..6 {
        a.advance();
        let last = frame == 5;
        let reaction = if last {
            r#"{"out":[{"type":"event","ev":{"type":"animationend","targetId":10}}],"set":{"dirty":true,"tick":false}}"#
        } else {
            r#"{"set":{"dirty":true}}"#
        };
        a.react("tick", reaction);
        a.react("paint", r#"{"list":[{"k":0,"r":[0,0,20,20],"c":4281558681}]}"#);
        a.react("damage", r#"{"damage":[[0,0,20,20]]}"#);
        let f = a.frame(&format!("an animation frame at +{} ms", 16 * (frame + 1)), true, Paint::IfDirty);
        painted += usize::from(f.frame.as_ref().expect("a frame").painted);
        ok!(a, !f.engine, "a tick with no engine request owes the engine no frame");
    }
    ok!(a, painted > 3 && !a.direct().wants_tick(), "the animation runs over several painted frames and ends");
    let _ = a.lp.end().renderer().wants_tick();
    let ended = a.frame("a frame after the animation ended", true, Paint::IfDirty);
    ok!(
        a,
        !ended.frame.as_ref().expect("a frame").painted && !ended.state.wants_tick && ended.state.next_tick_in.is_infinite(),
        "nothing is left to tick or paint"
    );
    ok!(
        a,
        a.clear_selection("a call that changes nothing after the frame").events.is_empty(),
        "no State repeats the one the frame carried"
    );
    a.advance();
    ok!(a, a.clear_selection("the same call later").events.is_empty(), "with nothing scheduled, time changes nothing to report");
    let again = a.want_frame("the engine asks again");
    ok!(a, again.events == vec![Kind::State], "a changed state is reported again");
    ok!(a, a.frame("the ticking frame that takes it", true, Paint::IfDirty).engine, "the engine's frame is due after it");
    ok!(a, !a.frame("the next ticking frame", true, Paint::IfDirty).engine, "and only once");
    a.close();
    a.need(a.tally.painted, "painted frames");
    a.need(a.tally.unchanged, "unchanged frames");
    a.need(a.tally.frames, "Frame reports");
    a.need(a.tally.states, "State reports");
    a.need(a.tally.helper_only, "animation events for the helper alone");
}

/// Cursor, press claims, holds, hold queries against real presses, input while frozen, and the selection
/// reported exactly when its text changes.
#[test]
fn pointing() {
    let mut a = Arms::new("pointer", Caps::default());
    a.receive(r#"{"type":"ops","ops":[],"react":{"calls":[["font","sans-serif",10,400,false,""],["width",1,"Hello"],["width",1,"world"]],"set":{"dirty":true,"height":150}}}"#, "a page to point at");
    a.receive(
        r#"{"type":"listen","types":["pointerdown","pointerup","pointercancel","contextmenu","click"]}"#,
        "the widget listens to presses",
    );
    a.frame("the first frame", true, Paint::IfDirty);
    a.react("cursor", r#"{"set":{"cursor":"pointer"}}"#);
    ok!(
        a,
        a.hover("the mouse over cursor:pointer", 0, 5.0, 10.0).pointer.expect("a pointer").cursor == b"pointer",
        "the cursor there is pointer"
    );
    a.react("cursor", r#"{"set":{"cursor":"auto"}}"#);
    ok!(a, a.hover("the mouse over a plain div", 0, 5.0, 30.0).pointer.expect("a pointer").cursor == b"auto", "the cursor there is auto");
    a.react("cursor", r#"{"set":{"cursor":"pointer"}}"#);
    a.hover("the mouse back over cursor:pointer", 0, 5.0, 10.0);
    a.react("cursor", r#"{"set":{"cursor":"auto"}}"#);
    ok!(
        a,
        a.hover("the mouse leaves", 1, 0.0, 0.0).pointer.expect("a pointer").cursor == b"auto",
        "the cursor is auto once the mouse left"
    );

    for (spot, x, y, claim) in [
        ("cursor:pointer", 60.0, 10.0, NO_CLAIM),
        ("a plain div", 60.0, 30.0, NO_CLAIM),
        ("touch-action:none", 60.0, 60.0, HOLD_CLAIM),
        ("selectable text", 60.0, 90.0, NO_CLAIM),
    ] {
        let c = format!(r#"{{"set":{{"claim":[{},{},{}]}}}}"#, claim.x, claim.y, claim.hold);
        a.react("claim_at", &c);
        let asked = a.claim(&format!("a hold query on {spot}"), x, y);
        ok!(
            a,
            asked.claim == Some(claim) && asked.messages.is_empty() && asked.selection.is_empty(),
            "the query answers and selects nothing"
        );
        let gesture = format!(
            r#"{{"out":[{{"type":"event","ev":{{"type":"pointerdown"}}}},{{"type":"gesture","wants":true,"x":{},"y":{},"hold":{}}}],"set":{{"tick":{},"next":{}}}}}"#,
            claim.x,
            claim.y,
            claim.hold,
            false,
            if claim.hold { "0.5" } else { "null" }
        );
        a.react("touch", &gesture);
        let pressed = a.touch(&format!("a finger pressed on {spot}"), 0, x, y, 0, true, false, 0);
        ok!(a, pressed.pointer.expect("a pointer").claim == asked.claim, "the press claims what the query answered");
        a.react("touch", r#"{"out":[{"type":"event","ev":{"type":"pointercancel"}}],"set":{"next":null}}"#);
        a.touch(&format!("the press cancelled on {spot}"), 3, x, y, 0, true, false, 0);
    }
    a.react("touch", r#"{"out":[{"type":"gesture","wants":true,"x":true,"y":false,"hold":true}],"set":{"tick":false,"next":0.5}}"#);
    let held = a.touch("a finger held on touch-action:none", 0, 60.0, 60.0, 0, true, false, 0);
    ok!(
        a,
        held.pointer.expect("a pointer").claim == Some(HOLD_CLAIM) && !held.state.wants_tick && held.state.next_tick_in == 0.5,
        "the press claims the hold, due at the next tick"
    );
    a.react("wants_tick", r#"{"set":{"tick":true,"next":0}}"#);
    let due = a.claim("a hold query past the hold's deadline", 60.0, 60.0);
    ok!(a, due.state.wants_tick && due.state.next_tick_in == 0.0, "the next call reports the hold due");
    a.react("tick", r#"{"out":[{"type":"event","ev":{"type":"contextmenu"}}],"set":{"tick":false,"next":null}}"#);
    ok!(
        a,
        has_event(&a.frame("the frame that takes the hold", true, Paint::Skip).messages, "contextmenu"),
        "the held finger gets the widget's contextmenu"
    );
    a.touch("the held finger lifted", 2, 60.0, 60.0, 0, true, false, 0);
    a.react("touch", r#"{"out":[{"type":"event","ev":{"type":"contextmenu"}}]}"#);
    ok!(
        a,
        has_event(&a.touch("a right click on touch-action:none", 0, 60.0, 60.0, 1, true, true, 2).messages, "contextmenu"),
        "a right click there is the widget's contextmenu"
    );
    a.touch("the right button released", 2, 60.0, 60.0, 1, true, true, 0);
    ok!(
        a,
        a.touch("a second finger, not primary", 0, 100.0, 30.0, 5, false, false, 0).pointer.expect("a pointer").claim.is_none(),
        "a press that is not primary claims nothing"
    );

    a.react("touch", r#"{"out":[{"type":"gesture","wants":true,"x":true,"y":false,"hold":false}]}"#);
    let start = a.touch("a mouse press on selectable text", 0, 1.0, 90.0, 1, true, true, 0);
    ok!(
        a,
        start.pointer.expect("a pointer").claim == Some(DRAG_CLAIM) && start.selection.is_empty(),
        "a press that starts a selection claims horizontal drags and selects nothing yet"
    );
    a.react("touch", r#"{"calls":[["width",1,"Hello world"]],"set":{"selection":"Hello world"}}"#);
    ok!(
        a,
        a.touch("the mouse dragged over the line", 1, 200.0, 90.0, 1, true, true, 0).selection == b"Hello world",
        "the drag selects the line"
    );
    let released = a.touch("the mouse released", 2, 200.0, 90.0, 1, true, true, 0);
    ok!(a, released.selection == b"Hello world" && !released.events.contains(&Kind::Selection), "the same selection is not reported again");
    let over = a.claim("a hold query over the selection", 60.0, 90.0);
    ok!(
        a,
        over.selection == b"Hello world" && over.messages.is_empty() && !over.events.contains(&Kind::Selection),
        "the query keeps the selection and emits nothing"
    );
    a.react("touch", r#"{"set":{"selection":"Hel"}}"#);
    a.react("selected_text", r#"{"calls":[["width",1,"Hel"],["width",1,"lo"]]}"#);
    let widths = a.lp.backend.count(Op::TextWidth);
    let part = a.touch("dragged part of the way", 1, 33.0, 90.0, 1, true, true, 0);
    ok!(a, part.selection == b"Hel" && part.events.contains(&Kind::Selection), "part of the line is selected, and reported");
    ok!(a, a.lp.backend.count(Op::TextWidth) == widths + 2, "the selection's text is laid out over the channel before it is reported");
    a.react("touch", r#"{"set":{"selection":"Hello world\nSecond"}}"#);
    ok!(
        a,
        a.touch("dragged into the next block", 1, 200.0, 110.0, 1, true, true, 0).selection == b"Hello world\nSecond",
        "a selection across blocks keeps its line break"
    );
    a.touch("released over the next block", 2, 200.0, 110.0, 1, true, true, 0);
    ok!(a, a.clear_selection("the host clears the selection").selection.is_empty(), "the cleared selection is reported");
    ok!(a, a.clear_selection("cleared again").events.is_empty(), "clearing nothing reports nothing");

    a.receive(r#"{"type":"frozen","reason":"paused","fatal":false}"#, "the widget is frozen");
    ok!(
        a,
        a.hover("the mouse over cursor:pointer while frozen", 0, 5.0, 10.0).pointer.expect("a pointer").cursor == b"auto",
        "a frozen widget's cursor is auto"
    );
    let frozen = a.touch("a finger on touch-action:none while frozen", 0, 60.0, 60.0, 0, true, false, 0);
    ok!(
        a,
        frozen.pointer.expect("a pointer").claim.is_none() && !has_event(&frozen.messages, "pointerdown"),
        "a frozen widget takes no press"
    );
    a.react("claim_at", r#"{"set":{"claim":[false,false,false]}}"#);
    ok!(a, a.claim("a hold query while frozen", 60.0, 60.0).claim == Some(NO_CLAIM), "and claims nothing");
    a.close();
    a.need(a.tally.pointers, "Pointer reports");
    a.need(a.tally.selections, "Selection reports");
    a.need(a.tally.claims, "ClaimAnswer reports");
    a.need(a.tally.messages, "Messages reports");
}

fn mixture() -> Vec<Json> {
    [
        r#"{"type":"size","width":200,"height":300,"innerWidth":200,"innerHeight":100,"dpr":2}"#,
        r#"{"type":"event","ev":{"type":"click","targetId":10}}"#,
        r#"{"type":"gesture","wants":false,"x":true,"y":false,"hold":true}"#,
        r#"{"type":"result","qid":9,"result":{}}"#,
        r#"{"type":"limit","detail":"paint"}"#,
        r#"{"type":"resize","entries":[]}"#,
        r#"{"type":"violation","detail":"ops limit"}"#,
        r#"{"type":"bubble","action":"menu","gesture":"hold"}"#,
    ]
    .iter()
    .map(|m| parse(m))
    .collect()
}

/// Size and theme input, the viewport against the document's height, work left unpainted until the host
/// asks, and which messages cross to the platform.
#[test]
fn routing() {
    let mut a = Arms::new("routing", Caps::default());
    let page = a.receive(
        r#"{"type":"ops","ops":[],"react":{"calls":[["font","sans-serif",16,400,false,""],["width",1,"tall"]],"out":[{"type":"size","width":300,"height":300}],"set":{"height":300,"dirty":true}}}"#,
        "a page 300 px tall",
    );
    ok!(a, page.state.height == 300, "the viewport follows the document");
    a.receive(r#"{"type":"listen","types":["pointerdown","pointerup"]}"#, "the widget listens to presses");
    a.react("set_max_height", r#"{"out":[{"type":"size","width":300,"height":300,"innerHeight":100}],"set":{"height":100}}"#);
    let capped = a.set_max_height("the height capped at 100 px", 100.0);
    ok!(
        a,
        capped.state.height == 100 && capped.events.contains(&Kind::Messages),
        "State has the viewport's height and the size message the document's as well"
    );
    a.react("set_width", r#"{"calls":[["width",1,"tall"]],"out":[{"type":"size","width":200,"height":300}],"set":{"dirty":true}}"#);
    let narrow = a.set_width("a narrower view", 200.0);
    ok!(a, narrow.state.width == 200.0, "the width is in State and the size message");
    a.receive(
        r#"{"type":"ops","ops":[],"react":{"calls":[["font","serif",18,400,false,""]],"set":{"dirty":true}}}"#,
        "a batch that changes the body's font",
    );
    a.react("set_theme", r#"{"calls":[["font","serif",18,400,false,""],["width",3,"tall"]],"set":{"dirty":true}}"#);
    a.set_theme("the dark theme, which builds the document anew", true);
    a.hover("the mouse over the page", 0, 5.0, 50.0);
    a.want_frame("the engine asks for a frame");
    ok!(
        a,
        a.lp.host.borrow().frames.is_empty() && a.direct().dirty() && a.lp.end().renderer().dirty() && a.lp.platform_state().dirty,
        "without request_frame nothing is painted or framed"
    );
    let released = a.lp.backend.count(Op::DeleteFont);
    a.react("receive", r#"{"calls":[["del",0],["del",2]]}"#);
    a.helper("the helper trims", |r| r.receive_json(&parse(r#"{"type":"trim"}"#)));
    ok!(a, a.lp.backend.count(Op::DeleteFont) > released, "trim releases the fonts no document holds");
    let mixed = a.helper("the helper reports a mixture of messages", |_| mixture());
    let crossed: String =
        a.lp.host
            .borrow()
            .batches
            .last()
            .map(|b| b.iter().map(|m| String::from_utf8_lossy(m.get("type").as_str()).into_owned() + " ").collect())
            .unwrap_or_default();
    ok!(
        a,
        mixed.events == vec![Kind::Messages] && crossed == "size gesture limit violation bubble ",
        &format!("the five platform messages cross alone (got {crossed})")
    );
    let result = a.receive(
        r#"{"type":"query","qid":1,"query":{"q":"rect","id":10},"react":{"out":[{"type":"result","qid":1,"result":{}}]}}"#,
        "a query",
    );
    ok!(a, result.events.is_empty(), "its result is the helper's alone");
    a.react(
        "touch",
        r#"{"out":[{"type":"event","ev":{"type":"pointerdown"}},{"type":"gesture","wants":true,"x":false,"y":false,"hold":false}]}"#,
    );
    let press = a.touch("a mouse press", 0, 5.0, 50.0, 1, true, true, 0);
    ok!(
        a,
        has_event(&press.messages, "pointerdown") && gesture_of(&press.messages).is_some() && press.events.first() == Some(&Kind::Messages),
        "the helper gets the press's event, the platform its gesture"
    );
    a.touch("the mouse released", 2, 5.0, 50.0, 1, true, true, 0);
    a.react("paint", r#"{"list":[{"k":0,"r":[0,0,200,100],"c":4278190080},{"k":4,"t":"tall","f":3}]}"#);
    let painted = a.frame("the frame the host asks for", true, Paint::IfDirty);
    let f = painted.frame.as_ref().expect("a frame");
    ok!(
        a,
        painted.engine && f.painted && f.damage.is_none(),
        "the requested frame paints the waiting work, whole, and owes the engine its frame"
    );
    ok!(a, !a.frame("an unchanged frame", false, Paint::IfDirty).frame.expect("a frame").painted, "the next is unpainted");
    a.close();
    a.need(a.tally.messages, "Messages reports");
    a.need(a.tally.helper_only, "messages for the helper alone");
}

/// The time seam, as far as the end is concerned: the end reads no clock of its own and hands its renderer
/// the one the embedder gives (here the scripted renderer's own), and time moving alone makes no traffic.
#[test]
fn clocks() {
    let clock = Rc::new(RefCell::new(1000.0f64));
    let seen = Rc::clone(&clock);
    let lp = Loopback::new(Caps::default());
    let log = Rc::clone(&lp.log);
    let mut end = twidget_ends::RendererEnd::new(300.0, 1.0, false, lp.channel(), Caps::default(), |w, d, k, proxy| {
        let r = ScriptedRenderer::new(w, d, k, proxy, &script(), log);
        r.log.borrow_mut().push(format!("clock {}", seen.borrow()));
        r
    });
    lp.flush();
    assert!(!end.stopped() && lp.log.borrow().iter().any(|l| l == "clock 1000"), "the renderer is made with the embedder's clock");
    let sent = lp.kinds().len();
    *clock.borrow_mut() += 600.0;
    assert_eq!(lp.kinds().len(), sent, "time moving on alone sends nothing");
    end.renderer().push("next_tick_in", parse(r#"{"set":{"next":0.25}}"#));
    end.want_frame();
    lp.flush();
    assert_eq!(lp.platform_state().next_tick_in, 0.25, "what the renderer reads of its clock goes out in State as it says");
    drop(end);
}
