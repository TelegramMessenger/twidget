//! The Rust ends against the C++ (`oracle/ends_oracle`: renderer_end.cpp, platform_end.cpp, remote.cpp and
//! wire.cpp, with tests/log_backend.h, and renderer_end.cpp's quoted `renderer.h` resolving to
//! `oracle/ends_seam/renderer.h`, a scripted renderer that does what tests/common/scripted.rs does). Both sides
//! run the same sessions through a renderer end and a platform end over an in-memory channel that can spoil
//! answers, and write the same transcript: after each step every message and request on the wire and every
//! answer, every call the end made of its renderer and every Backend call that renderer's script made, with
//! what each returned, every Backend call the platform end made, every report its host got, the step's
//! results, and where both ends stand; then the renderer's destruction. The transcripts are compared byte for
//! byte.
//!
//! A session is `{"caps": {…}, "script": <the scripted renderer's script>, "steps": [step, …]}`, a step one
//! of `["open", width, dpr, dark]`, `["receive", message]`, `["want_frame"]`, `["report", [message, …]]`,
//! `["frame", tick, paint]`, `["width", w]`, `["max", h]`, `["theme", dark]`, `["touch", action, x, y,
//! pointer, primary, mouse, modifiers, button]`, `["hover", action, x, y, modifiers]`, `["clear"]`,
//! `["claim", x, y]`, `["pump", steps]`, `["deliver", hex]`, `["fault", spoil, at, kind, hex reply]`,
//! `["backend", images load, reads]`, `["push", method, reaction]`, `["helper", message]`,
//! `["platform_receive", hex]`, `["platform_answer", hex]`, `["platform_close"]`, `["close"]`. Skipped without
//! `TWIDGET_ORACLE_DIR`; `tools/gate.py` sets it.

mod common;

use std::fmt::Write as _;

use common::scripted::hex;
use common::*;
use twidget_ends::Output;
use twidget_json::{Json, Surrogates};
use twidget_remote::*;
use twidget_testkit::{Comparison, Lcg, Oracle, framed, mutate, unhex};
use twidget_wire::{CanvasOp, Color, Command, OpKind, Rect};

fn cap(caps: &Json, name: &str, fallback: usize) -> usize {
    match caps.find(name) {
        Some(v) if v.is_number() => v.as_number(0.0) as usize,
        _ => fallback,
    }
}

fn caps_of(j: &Json) -> Caps {
    let mut c = Caps::default();
    c.message = cap(j, "message", c.message);
    c.text = cap(j, "text", c.text);
    c.url = cap(j, "url", c.url);
    c.messages = cap(j, "messages", c.messages);
    c.values.commands = cap(j, "commands", c.values.commands);
    c.values.canvas_ops = cap(j, "canvas_ops", c.values.canvas_ops);
    c.values.pixels = cap(j, "pixels", c.values.pixels);
    c.values.text = cap(j, "value_text", c.values.text);
    c.values.rects = cap(j, "rects", c.values.rects);
    c.values.clips = cap(j, "clips", c.values.clips);
    c.values.floats = cap(j, "floats", c.values.floats);
    c.widths = cap(j, "widths", c.widths);
    c.widths_text = cap(j, "widths_text", c.widths_text);
    c.frame = cap(j, "frame", c.frame);
    c.delta_base = cap(j, "delta_base", c.delta_base);
    c
}

fn joined(messages: &[Json]) -> String {
    messages.iter().map(|m| m.dump_lossy()).collect::<Vec<_>>().join(" ")
}

fn flag(b: bool) -> u8 {
    u8::from(b)
}

/// The session through the Rust ends, written as ends_oracle writes it.
fn run_session(session: &Json) -> Vec<u8> {
    let caps = caps_of(session.get("caps"));
    let script = session.get("script").clone();
    let mut lp = Loopback::new(caps);
    let mut t: Vec<u8> = Vec::new();
    let mut seen = [0usize; 4];
    let mut out = Output::default();
    let drain = |lp: &Loopback, t: &mut Vec<u8>, seen: &mut [usize; 4]| {
        let traffic = lp.wire.borrow().traffic.clone();
        for line in &traffic[seen[0]..] {
            t.extend_from_slice(line.as_bytes());
            t.push(b'\n');
        }
        seen[0] = traffic.len();
        let log = lp.log.borrow().clone();
        for line in &log[seen[1]..] {
            t.extend_from_slice(format!("R {line}\n").as_bytes());
        }
        seen[1] = log.len();
        let calls = lp.backend.calls();
        for call in &calls[seen[2]..] {
            t.extend_from_slice(b"B ");
            t.extend_from_slice(&call.line());
            t.push(b'\n');
        }
        seen[2] = calls.len();
        let lines = lp.host.borrow().lines.clone();
        for line in &lines[seen[3]..] {
            t.extend_from_slice(b"H ");
            t.extend_from_slice(line);
            t.push(b'\n');
        }
        seen[3] = lines.len();
    };
    for (index, step) in session.get("steps").arr().iter().enumerate() {
        let what = String::from_utf8_lossy(step.at(0).as_str()).into_owned();
        let mut result = format!("= {what}");
        let open = lp.end.is_some();
        match what.as_str() {
            "open" => lp.open(step.at(1).as_number(0.0), step.at(2).as_number(0.0), step.at(3).as_bool(false), &script),
            "receive" | "want_frame" | "report" | "deliver" | "push" | "helper" | "close" if !open => result.push_str(" none"),
            "receive" => {
                let got = lp.receive(step.at(1));
                let _ = write!(result, " {}", joined(&got));
            }
            "want_frame" => lp.want_frame(),
            "report" => lp.report(step.at(1).arr()),
            "deliver" => {
                let ok = lp.end().deliver(&unhex(&String::from_utf8_lossy(step.at(1).as_str())), &mut out);
                lp.flush();
                let _ = write!(result, " {} {} {}", flag(ok), flag(out.frame), joined(&out.messages));
            }
            "push" => lp.end().renderer().push(&String::from_utf8_lossy(step.at(1).as_str()), step.at(2).clone()),
            "helper" => {
                if lp.end().stopped() {
                    result.push_str(" skipped");
                } else {
                    let got = twidget_ends::Renderer::receive(lp.end().renderer(), step.at(1));
                    lp.report(&got);
                    let _ = write!(result, " {}", joined(&got));
                }
            }
            "close" => lp.close(),
            "frame" => {
                let paint = [Paint::Skip, Paint::IfDirty, Paint::Force][step.at(2).as_int(1).clamp(0, 2) as usize];
                let seq = lp.platform.borrow_mut().request_frame(step.at(1).as_bool(false), paint);
                let _ = write!(result, " {seq}");
            }
            "width" => lp.platform.borrow_mut().set_width(step.at(1).as_number(0.0)),
            "max" => lp.platform.borrow_mut().set_max_height(step.at(1).as_number(0.0)),
            "theme" => lp.platform.borrow_mut().set_theme(step.at(1).as_bool(false)),
            "touch" => lp.platform.borrow_mut().touch(
                step.at(1).as_int(0),
                step.at(2).as_number(0.0),
                step.at(3).as_number(0.0),
                step.at(4).as_int(0),
                step.at(5).as_bool(false),
                step.at(6).as_bool(false),
                step.at(7).as_int(0),
                step.at(8).as_int(0),
            ),
            "hover" => lp.platform.borrow_mut().hover(
                step.at(1).as_int(0),
                step.at(2).as_number(0.0),
                step.at(3).as_number(0.0),
                step.at(4).as_int(0),
            ),
            "clear" => lp.platform.borrow_mut().clear_selection(),
            "claim" => {
                let seq = lp.platform.borrow_mut().ask_claim(step.at(1).as_number(0.0), step.at(2).as_number(0.0));
                let _ = write!(result, " {seq}");
            }
            "pump" => {
                let steps = step.at(1).as_int(64).max(0) as usize;
                lp.flush();
                let mut ok = true;
                let mut done = Vec::new();
                let mut i = 0;
                loop {
                    if lp.queue.0.borrow().is_empty() {
                        break;
                    }
                    if i == steps || lp.end.is_none() {
                        ok = false;
                        break;
                    }
                    let message = lp.queue.0.borrow_mut().pop_front().expect("a message");
                    let delivered = lp.end().deliver(&message, &mut out);
                    done.push((delivered, out.clone()));
                    lp.flush();
                    i += 1;
                }
                let _ = write!(result, " {}", flag(ok));
                for (delivered, output) in done {
                    let _ = write!(result, "\n= deliver {} {} {}", flag(delivered), flag(output.frame), joined(&output.messages));
                }
            }
            "fault" => {
                let how = [
                    Spoil::None,
                    Spoil::Unanswered,
                    Spoil::Empty,
                    Spoil::WrongKind,
                    Spoil::WrongVersion,
                    Spoil::Cut,
                    Spoil::Trailing,
                    Spoil::Replaced,
                ][step.at(1).as_int(0).clamp(0, 7) as usize];
                let kind = step.at(3).as_int(0);
                lp.wire.borrow_mut().fault = Fault {
                    how,
                    at: step.at(2).as_int(0).max(0) as usize,
                    kind: if kind != 0 { Kind::from_u8(kind as u8) } else { None },
                    reply: unhex(&String::from_utf8_lossy(step.at(4).as_str())),
                };
            }
            "backend" => {
                let mut b = lp.backend.0.borrow_mut();
                b.images_load = step.at(1).as_bool(true);
                b.reads = [Read::Pixels, Read::Unsupported, Read::WrongSize][step.at(2).as_int(0).clamp(0, 2) as usize];
            }
            "platform_receive" => {
                let ok = lp.platform.borrow_mut().receive(&unhex(&String::from_utf8_lossy(step.at(1).as_str())));
                let _ = write!(result, " {}", flag(ok));
            }
            "platform_close" => lp.platform.borrow_mut().close(),
            "platform_answer" => {
                let mut reply = Vec::new();
                let ok = lp.platform.borrow_mut().answer(&unhex(&String::from_utf8_lossy(step.at(1).as_str())), &mut reply);
                let _ = write!(result, " {} {}", flag(ok), hex(&reply));
            }
            _ => result.push_str(" unknown"),
        }
        t.extend_from_slice(format!("step {index} ").as_bytes());
        t.extend_from_slice(&step.dump());
        t.push(b'\n');
        drain(&lp, &mut t, &mut seen);
        t.extend_from_slice(result.as_bytes());
        t.push(b'\n');
        let status = match lp.end.as_ref() {
            Some(end) => format!("{} {}", flag(end.stopped()), common::scripted::quoted(end.stop_reason().as_bytes())),
            None => "-".to_string(),
        };
        t.extend_from_slice(
            format!(
                "S {} {status} {} {} {}\n",
                flag(lp.end.is_some()),
                flag(lp.refusing()),
                state_line(&lp.platform_state()),
                lp.queue.0.borrow().len()
            )
            .as_bytes(),
        );
    }
    lp.close();
    drain(&lp, &mut t, &mut seen);
    t
}

/// What the sessions came to: the ends' stop reasons and the platform end's refusals, and how many of each.
#[derive(Default)]
struct Seen(std::collections::BTreeMap<String, usize>);

impl Seen {
    fn note(&mut self, transcript: &[u8]) {
        let text = String::from_utf8_lossy(transcript);
        let mut last_stop = String::new();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("S 1 1 ") {
                // The reason is the quoted text before the platform's flag and state.
                let end = rest.rfind('"').map_or(0, |at| at + 1);
                last_stop = format!("stop {}", &rest[..end]);
            }
            if let Some(reason) = line.strip_prefix("H refused ") {
                *self.0.entry(format!("refused {reason}")).or_default() += 1;
            }
            for (prefix, what) in
                [("> 4901", "traffic: frame deltas"), ("> 4001", "traffic: frames whole"), ("? 6401", "traffic: widths requests")]
            {
                if line.starts_with(prefix) {
                    *self.0.entry(what.to_string()).or_default() += 1;
                }
            }
        }
        if !last_stop.is_empty() {
            *self.0.entry(last_stop).or_default() += 1;
        }
    }
}

fn case(c: &mut Comparison, session: &Json, what: String, seen: &mut Seen) {
    let text = session.dump();
    let parsed = Json::parse_with(&text, Surrogates::Replace, 1024).expect("the session parses again");
    let transcript = run_session(&parsed);
    seen.note(&transcript);
    c.case(&framed(b'c', &text), (1, transcript), what);
}

// ---- Sessions written out -------------------------------------------------------------------------

fn session(caps: &str, script: &str, steps: &str) -> Json {
    parse(&format!(r#"{{"caps":{caps},"script":{script},"steps":{steps}}}"#))
}

const PAGE: &str = r#"{"type":"ops","ops":[],"react":{"calls":[["font","sans-serif",16,400,false,""],["widths",[[0,"two"],[0," "],[1,"ends"]]],["image",12,"data:image/png;base64,iVBORw0KGgo="],["cfont","serif",12,700,false],["width",2,"hi"],["run",13,40,20,[{"k":0},{"k":4,"r":[0,10,0,0],"t":"hi","f":3},{"k":1,"r":[0,0,5,5],"c":4281558681,"cl":0},{"k":1,"r":[6,0,5,5],"cl":0},{"k":6,"r":[1,1,0,0],"px":[2,1,0]}]]],"out":[{"type":"size","width":300,"height":40},{"type":"event","ev":{"type":"load"}}],"set":{"height":40,"dirty":true}}}"#;

const SCRIPT: &str = r#"{"ctor":{"calls":[["font","sans-serif",16,400,false,""]]},"on":{"paint":[{"list":[{"k":0,"r":[0,0,300,40],"c":4294967295},{"k":4,"t":"two ends","f":1},{"k":11,"p":0},{"k":15,"fl":2},{"k":11,"p":0},{"k":16}]}],"damage":[{"damage":[[0,0,300,40]]}],"tick":[{"out":[{"type":"event","ev":{"type":"frame"}}]}],"touch":[{"out":[{"type":"gesture","wants":true,"x":true,"y":false,"hold":true}],"set":{"selection":"Hello"}}],"selected_text":[{"calls":[["width",0,"Hello"]]}]}}"#;

/// The C++ suite's sessions as steps: a healthy one, construction and later failures, bad input, caps.
fn written_sessions() -> Vec<(String, Json)> {
    let mut out = Vec::new();
    let pixels = r#"{"type":"query","qid":1,"react":{"calls":[["read",13,0,0,2,1]],"out":[{"type":"result","qid":1}]}}"#;
    let healthy = format!(
        r#"[["open",300,2,false],["receive",{PAGE}],["receive",{pixels}],["want_frame"],["frame",true,1],["pump",64],["frame",false,1],["pump",64],["clear"],["pump",64],["touch",0,5,5,1,true,false,0,0],["pump",64],["hover",0,5,5,0],["claim",5,5],["pump",64],["width",200],["max",100],["theme",true],["pump",64],["helper",{{"type":"trim"}}],["report",[{{"type":"size"}},{{"type":"event"}},{{"type":"bubble","action":"menu"}}]],["close"]]"#
    );
    out.push(("a healthy session".to_string(), session("{}", SCRIPT, &healthy)));
    let moved = |x: i32| {
        format!(
            r#"{{"list":[{{"k":12,"r":[{x},0,20,20]}},{{"k":0,"r":[0,0,300,40],"c":4294967295}},{{"k":4,"t":"two ends","f":1}},{{"k":11,"p":0}},{{"k":15,"fl":2}},{{"k":11,"p":0}},{{"k":16}},{{"k":13}}]}}"#
        )
    };
    let paints: Vec<String> = (0..6).map(moved).collect();
    let deltas = format!(r#"{{"ctor":{{"calls":[["font","sans-serif",16,400,false,""]]}},"on":{{"paint":[{}]}}}}"#, paints.join(","));
    let frames = r#"[["open",300,2,false],["frame",true,2],["pump",64],["frame",true,2],["pump",64],["frame",false,0],["pump",64],["frame",true,2],["pump",64],["frame",true,2],["pump",64],["platform_close"],["frame",true,2],["pump",64],["close"]]"#;
    out.push(("frames as deltas, an unpainted one between, and a platform end closed".to_string(), session("{}", &deltas, frames)));
    for (caps, name) in
        [(r#"{"delta_base":0}"#, "no deltas"), (r#"{"delta_base":40}"#, "a base too small"), (r#"{"frame":120}"#, "a frame cap")]
    {
        out.push((format!("frames under {name}"), session(caps, &deltas, frames)));
    }
    for how in 1..=6 {
        out.push((
            format!("request 1 spoiled {how}"),
            session(
                "{}",
                SCRIPT,
                &format!(r#"[["fault",{how},1,0,""],["open",300,1,false],["receive",{PAGE}],["frame",true,2],["pump",64],["close"]]"#),
            ),
        ));
        for kind in 96..=99 {
            let first = if kind == 99 { format!(r#"["receive",{PAGE}],"#) } else { String::new() };
            let then = if kind == 99 { pixels.to_string() } else { PAGE.to_string() };
            out.push((
                format!("kind {kind} spoiled {how}"),
                session("{}", SCRIPT, &format!(r#"[["open",300,1,false],{first}["fault",{how},0,{kind},""],["receive",{then}],["want_frame"],["touch",0,1,1,0,true,false,0,0],["pump",64],["close"]]"#)),
            ));
        }
    }
    let mut bad: Vec<String> = vec![String::new(), "01".into(), "0901".into(), "0102".into()];
    let mut frame_request = Vec::new();
    FrameRequest { seq: 1, tick: true, paint: Paint::Force }.encode(&Caps::default(), &mut frame_request);
    bad.push(hex(&frame_request[..frame_request.len() - 1]));
    bad.push(hex(&frame_request) + "00");
    for kind in [Kind::State, Kind::CanvasRelease, Kind::FontAnswer, Kind::PixelsAnswer, Kind::FontRequest] {
        bad.push(format!("{:02x}01", kind as u8));
    }
    for (i, bytes) in bad.iter().enumerate() {
        out.push((
            format!("bad input {i}"),
            session(
                "{}",
                SCRIPT,
                &format!(r#"[["open",300,1,false],["deliver","{bytes}"],["receive",{PAGE}],["frame",true,2],["pump",64],["close"]]"#),
            ),
        ));
    }
    for (name, caps) in [
        ("commands", r#"{"commands":1}"#),
        ("messages", r#"{"messages":0}"#),
        ("url", r#"{"url":16}"#),
        ("pixels", r#"{"pixels":4}"#),
        ("text", r#"{"text":3}"#),
        ("message", r#"{"message":64}"#),
    ] {
        out.push((
            format!("caps {name}"),
            session(caps, SCRIPT, &format!(r#"[["open",300,1,false],["receive",{PAGE}],["receive",{pixels}],["want_frame"],["frame",true,2],["pump",64],["touch",0,1,1,0,true,false,0,0],["pump",64],["close"]]"#)),
        ));
    }
    out
}

// ---- Generated sessions ---------------------------------------------------------------------------

fn pick<'a>(random: &mut Lcg, items: &[&'a str]) -> &'a str {
    items[random.below(items.len())]
}

fn number(random: &mut Lcg) -> Json {
    Json::from(match random.below(10) {
        0 => 0.0,
        1 => -1.5,
        2 => 1e300,
        3 => 0.25,
        _ => random.below(400) as f64,
    })
}

fn text(random: &mut Lcg) -> Json {
    Json::from(pick(
        random,
        &["", "two ends", "hi", "w", "Hello world\nSecond", "\"quoted\"", "é ü 中", "a\tb", "wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww"],
    ))
}

fn message(random: &mut Lcg) -> Json {
    let pool = [
        r#"{"type":"size","width":200,"height":300,"innerWidth":200,"innerHeight":100,"dpr":2}"#,
        r#"{"type":"event","ev":{"type":"click","targetId":10}}"#,
        r#"{"type":"gesture","wants":true,"x":true,"y":false,"hold":true}"#,
        r#"{"type":"gesture","wants":false,"x":false,"y":true,"hold":false}"#,
        r#"{"type":"gesture"}"#,
        r#"{"type":"result","qid":9,"result":{}}"#,
        r#"{"type":"limit","detail":"paint"}"#,
        r#"{"type":"resize","entries":[]}"#,
        r#"{"type":"violation","detail":"ops limit"}"#,
        r#"{"type":"bubble","action":"menu","gesture":"hold"}"#,
        r#"{"type":"size","deep":{"a":{"b":{"c":{"d":1}}}}}"#,
        r#"{"type":5}"#,
        r#"[1,2]"#,
        r#""size""#,
        r#"{"type":"size","text":"é"}"#,
    ];
    parse(pool[random.below(pool.len())])
}

fn messages(random: &mut Lcg, n: usize) -> Json {
    Json::from((0..random.below(n + 1)).map(|_| message(random)).collect::<Vec<_>>())
}

fn rect(random: &mut Lcg) -> Json {
    Json::from((0..4).map(|_| number(random)).collect::<Vec<_>>())
}

/// A widths call of up to 300 texts in the fonts made so far, some long, some repeated, some empty.
fn widths_call(random: &mut Lcg) -> Json {
    let mut texts = Json::array();
    let n = if random.chance(4) { random.below(300) } else { random.below(12) };
    for _ in 0..n {
        let t = if random.chance(10) { Json::from("x".repeat(random.below(70))) } else { text(random) };
        texts.push(Json::from(vec![Json::from(random.below(8) as i32), t]));
    }
    let mut c = Json::array();
    c.push("widths").push(texts);
    c
}

fn call(random: &mut Lcg) -> Json {
    let mut c = Json::array();
    if random.chance(6) {
        return widths_call(random);
    }
    match random.below(12) {
        0..=2 => {
            c.push("font")
                .push(pick(random, &["sans-serif", "serif", "\"Inter\", system-ui", ""]))
                .push(number(random))
                .push([400, 700, 100][random.below(3)]);
            c.push(random.chance(3)).push(pick(random, &["", "tnum=1", "zero=1,tnum=1"]));
        }
        3 => {
            c.push("cfont").push(pick(random, &["monospace", "serif"])).push(number(random)).push(400).push(random.chance(2));
        }
        4 => {
            c.push("del").push(random.below(8) as i32);
        }
        5 | 6 => {
            c.push("width").push(random.below(8) as i32).push(text(random));
        }
        7 => {
            let url = if random.chance(4) {
                "data:image/png;base64,".to_string() + &"A".repeat(random.below(64))
            } else {
                "data:image/png;base64,iVBORw0KGgo=".to_string()
            };
            c.push("image").push(random.below(20) as i32).push(url);
        }
        8 => {
            let mut ops = Json::array();
            for _ in 0..random.below(5) {
                let mut op = Json::object();
                op.set("k", random.below(7) as i32)
                    .set("r", rect(random))
                    .set("c", random.next() as f64)
                    .set("t", text(random))
                    .set("f", random.below(4) as i32);
                if random.chance(2) {
                    op.set("cl", random.below(3) as i32);
                }
                if random.chance(4) {
                    op.set(
                        "px",
                        Json::from(vec![
                            Json::from(random.below(3) as i32),
                            Json::from(random.below(3) as i32),
                            Json::from(random.below(2) as i32),
                        ]),
                    );
                }
                ops.push(op);
            }
            c.push("run").push(1 + random.below(4) as i32).push([40, 0, 9000][random.below(3)]).push(20).push(ops);
        }
        9 | 10 => {
            let (w, h) = [(2, 1), (0, 0), (120, 140), (-1, 3), (1, 2), (9000, 1)][random.below(6)];
            c.push("read").push(1 + random.below(4) as i32).push(random.below(3) as i32).push(0).push(w).push(h);
        }
        _ => {
            c.push("release").push(1 + random.below(4) as i32);
        }
    }
    c
}

/// A command of a few, or one drawn at random.
fn painted_command(random: &mut Lcg) -> Json {
    let palette = [
        r#"{"k":0,"r":[0,0,300,40],"c":4294967295}"#,
        r#"{"k":4,"t":"two ends","f":1}"#,
        r#"{"k":11,"p":0}"#,
        r#"{"k":15,"fl":2}"#,
        r#"{"k":16}"#,
        r#"{"k":2,"r":[10,10,20,20],"c":4281558681}"#,
    ];
    let mut c = parse(palette[random.below(palette.len())]);
    if random.chance(4) {
        c.set("r", rect(random)).set("t", text(random));
    }
    c
}

fn reaction(random: &mut Lcg, paint: bool) -> Json {
    let mut r = Json::object();
    if random.chance(2) {
        r.set("calls", Json::from((0..random.below(5)).map(|_| call(random)).collect::<Vec<_>>()));
    }
    if random.chance(2) {
        r.set("out", messages(random, 3));
    }
    if random.chance(2) {
        let mut set = Json::object();
        for _ in 0..random.below(4) {
            match random.below(8) {
                0 => set.set("width", number(random)),
                1 => set.set("height", number(random)),
                2 => set.set("dirty", random.chance(2)),
                3 => set.set("tick", random.chance(2)),
                4 => set.set("next", if random.chance(2) { Json::Null } else { number(random) }),
                5 => set.set("cursor", pick(random, &["auto", "pointer", "text", ""])),
                6 => set.set("selection", text(random)),
                _ => set.set(
                    "claim",
                    Json::from(vec![Json::from(random.chance(2)), Json::from(random.chance(4)), Json::from(random.chance(2))]),
                ),
            };
        }
        r.set("set", set);
    }
    if paint && random.chance(2) {
        // Commands from a few, so that one painted list shares runs with the last: what a FrameDelta copies.
        let palette = [
            r#"{"k":0,"r":[0,0,300,40],"c":4294967295}"#,
            r#"{"k":4,"t":"two ends","f":1}"#,
            r#"{"k":11,"p":0}"#,
            r#"{"k":15,"fl":2}"#,
            r#"{"k":16}"#,
            r#"{"k":2,"r":[10,10,20,20],"c":4281558681}"#,
        ];
        let mut list = Json::array();
        for _ in 0..random.below(24) {
            let mut c = parse(palette[random.below(palette.len())]);
            if random.chance(8) {
                c.set("r", rect(random));
            }
            list.push(c);
        }
        r.set("list", list);
    } else if paint {
        let mut list = Json::array();
        for _ in 0..random.below(6) {
            let mut c = Json::object();
            c.set("k", random.below(18) as i32)
                .set("r", rect(random))
                .set("c", random.next() as f64)
                .set("t", text(random))
                .set("f", random.below(3) as i32);
            if random.chance(3) {
                c.set("p", random.below(2) as i32);
            }
            if random.chance(4) {
                c.set("fl", random.below(2) as i32);
            }
            list.push(c);
        }
        r.set("list", list);
    }
    if random.chance(3) {
        r.set(
            "damage",
            if random.chance(4) { Json::Null } else { Json::from((0..random.below(3)).map(|_| rect(random)).collect::<Vec<_>>()) },
        );
    }
    r
}

const METHODS: [&str; 19] = [
    "receive",
    "set_width",
    "set_max_height",
    "set_theme",
    "touch",
    "hover",
    "cursor",
    "claim_at",
    "clear_selection",
    "may_have_selection",
    "selected_text",
    "tick",
    "dirty",
    "paint",
    "damage",
    "width",
    "viewport_height",
    "wants_tick",
    "next_tick_in",
];

fn script(random: &mut Lcg) -> Json {
    let mut s = Json::object();
    if !random.chance(10) {
        let mut ctor = reaction(random, false);
        let mut calls = Json::from(vec![parse(r#"["font","sans-serif",16,400,false,""]"#)]);
        for c in ctor.get("calls").arr() {
            calls.push(c.clone());
        }
        ctor.set("calls", calls);
        s.set("ctor", ctor);
    }
    if random.chance(8) {
        s.set("drop", reaction(random, false));
    }
    let mut on = Json::object();
    for _ in 0..random.below(8) {
        let method = METHODS[random.below(METHODS.len())];
        let list = Json::from((0..1 + random.below(3)).map(|_| reaction(random, method == "paint")).collect::<Vec<_>>());
        on.set(method, list);
    }
    if !random.chance(3) {
        // Several paints in a row, each the last list with a command changed, added or removed, now and then another:
        // frames as deltas.
        let mut base: Vec<Json> = (0..4 + random.below(20)).map(|_| painted_command(random)).collect();
        let mut paints = Vec::new();
        for _ in 0..3 + random.below(10) {
            match random.below(5) {
                0 if !base.is_empty() => {
                    let at = random.below(base.len());
                    base.remove(at);
                }
                1 => {
                    let at = random.below(base.len() + 1);
                    base.insert(at, painted_command(random));
                }
                2 if !base.is_empty() => {
                    let at = random.below(base.len());
                    base[at] = painted_command(random);
                }
                3 if random.chance(4) => base = (0..random.below(20)).map(|_| painted_command(random)).collect(),
                _ => {}
            }
            let mut r = if random.chance(3) { reaction(random, false) } else { Json::object() };
            r.set("list", Json::from(base.clone()));
            paints.push(r);
        }
        on.set("paint", Json::from(paints));
    }
    s.set("on", on);
    s
}

fn encoded_message<M: Message>(m: &M) -> Vec<u8> {
    let mut out = Vec::new();
    m.encode(&Caps::default(), &mut out);
    out
}

fn any_list(random: &mut Lcg) -> Vec<Command> {
    (0..random.below(3))
        .map(|_| Command {
            kind: twidget_wire::Cmd::FillRect,
            rect: Rect::new(1.0, 2.0, random.below(9) as f32, 4.0),
            color: Color::rgba(1, 2, 3, 4),
            ..Command::default()
        })
        .collect()
}

/// A platform message (to the renderer end) or one of any other kind, in its wire form; changed now and then.
fn platform_bytes(random: &mut Lcg) -> Vec<u8> {
    let seq = random.below(4) as u64;
    let mut bytes = match random.below(14) {
        0 => encoded_message(&FrameRequest {
            seq,
            tick: random.chance(2),
            paint: [Paint::Skip, Paint::IfDirty, Paint::Force][random.below(3)],
        }),
        1 => encoded_message(&SetWidth { width: 120.0 }),
        2 => encoded_message(&SetMaxHeight { height: 50.0 }),
        3 => encoded_message(&SetTheme { dark: true }),
        4 => encoded_message(&Touch {
            action: random.below(4) as i32,
            x: 3.0,
            y: 4.0,
            pointer: 1,
            primary: true,
            mouse: random.chance(2),
            modifiers: 0,
            button: 0,
        }),
        5 => encoded_message(&Hover { action: 0, x: 3.0, y: 4.0, modifiers: 2 }),
        6 => encoded_message(&ClearSelection),
        7 => encoded_message(&ClaimQuery { seq, x: 1.0, y: 2.0 }),
        8 => encoded_message(&FontAnswer { font: 3, metrics: Default::default() }),
        9 => encoded_message(&State::default()),
        10 => encoded_message(&FontRequest {
            canvas: false,
            families: b"serif".to_vec(),
            size: 12.0,
            weight: 400,
            italic: false,
            features: Vec::new(),
        }),
        11 => encoded_message(&CanvasRelease { id: 1 }),
        12 => (0..random.below(6)).map(|_| random.next() as u8).collect(),
        _ => encoded_message(&WidthAnswer { width: 3.0 }),
    };
    if random.chance(3) {
        mutate(random, &mut bytes);
    }
    bytes
}

/// A renderer end's message or request (to the platform end), or a platform one, in its wire form; changed now
/// and then.
fn renderer_bytes(random: &mut Lcg, seq: u64) -> Vec<u8> {
    let state = State { width: 300.0, height: 40, dirty: random.chance(2), ..State::default() };
    let mut bytes = match random.below(19) {
        0 => encoded_message(&Frame { seq, state, painted: false, ..Frame::default() }),
        1 => encoded_message(&Frame {
            seq,
            state,
            painted: true,
            list: any_list(random),
            damage: if random.chance(2) { None } else { Some(vec![Rect::new(0.0, 0.0, 1.0, 1.0)]) },
        }),
        2 => encoded_message(&state),
        3 => {
            let texts = (0..random.below(3)).map(|_| message(random).dump()).collect();
            encoded_message(&Messages { texts })
        }
        4 => encoded_message(&Messages { texts: vec![b"{\"type\":\"size\"".to_vec()] }),
        5 => encoded_message(&Pointer {
            cursor: b"pointer".to_vec(),
            claim: if random.chance(2) { None } else { Some(GestureClaim { x: true, y: false, hold: true }) },
        }),
        6 => encoded_message(&Selection { text: b"Hello".to_vec() }),
        7 => encoded_message(&ClaimAnswer { seq, claim: GestureClaim::default() }),
        8 => encoded_message(&FontRelease { font: random.below(4) as i32 }),
        9 => {
            let ops = vec![CanvasOp { kind: OpKind::Reset, ..CanvasOp::default() }, CanvasOp::default()];
            let mut out = Vec::new();
            encode_canvas_run(2, 10, 10, &ops, &Caps::default(), &mut out);
            out
        }
        10 => encoded_message(&CanvasRelease { id: 2 }),
        11 => encoded_message(&FontRequest {
            canvas: random.chance(2),
            families: b"serif".to_vec(),
            size: 12.0,
            weight: 700,
            italic: true,
            features: b"tnum=1".to_vec(),
        }),
        12 => encoded_message(&WidthRequest { font: random.below(4) as i32, text: b"hello".to_vec() }),
        13 => encoded_message(&ImageRequest { id: 4, url: b"data:image/png;base64,AAAA".to_vec() }),
        14 => {
            let (w, h) = [(2, 1), (0, 0), (120, 140), (1, 3)][random.below(4)];
            encoded_message(&PixelsRequest { id: 1, x: 0, y: 0, w, h })
        }
        15 => encoded_message(&FrameRequest { seq, tick: true, paint: Paint::Force }),
        16 => (0..random.below(6)).map(|_| random.next() as u8).collect(),
        17 if random.chance(2) => {
            let texts = (0..random.below(5)).map(|k| TextInFont { font: k as i32, text: b"word".to_vec() }).collect();
            encoded_message(&WidthsRequest { texts })
        }
        17 => {
            let base = random.below(4) as u64;
            let runs = vec![
                DeltaRun { offset: 0, length: 1 + random.below(8) as u32, literal: Vec::new() },
                DeltaRun { offset: 0, length: 0, literal: vec![1, 1, 0, 0, 0, 0] },
            ];
            encoded_message(&FrameDelta { seq, state, base, runs, damage: None })
        }
        _ => encoded_message(&WidthAnswer { width: 3.0 }),
    };
    if random.chance(3) {
        mutate(random, &mut bytes);
    }
    bytes
}

fn answer_bytes(random: &mut Lcg) -> Vec<u8> {
    let mut bytes = match random.below(7) {
        0 => encoded_message(&FontAnswer { font: 7, metrics: Default::default() }),
        1 => encoded_message(&WidthAnswer { width: 8.0 }),
        2 => encoded_message(&ImageAnswer { ok: true, w: 3.0, h: 4.0 }),
        3 => {
            let mut out = Vec::new();
            PixelsAnswer { ok: true, rgba: vec![9; 8] }.encode(&Caps::default(), &mut out);
            out
        }
        4 => Vec::new(),
        5 => {
            let mut out = Vec::new();
            WidthsAnswer { widths: (0..random.below(4)).map(|k| k as f32 + 0.5).collect() }.encode(&Caps::default(), &mut out);
            out
        }
        _ => (0..random.below(9)).map(|_| random.next() as u8).collect(),
    };
    if random.chance(3) {
        mutate(random, &mut bytes);
    }
    bytes
}

fn any_caps(random: &mut Lcg) -> Json {
    let mut caps = Json::object();
    if random.chance(4) {
        match random.below(4) {
            0 => caps.set("widths", [0, 1, 4][random.below(3)]),
            1 => caps.set("widths_text", [0, 3, 16][random.below(3)]),
            2 => caps.set("frame", [60, 200, 4096][random.below(3)]),
            _ => caps.set("delta_base", [0, 1, 64, 4096][random.below(4)]),
        };
    }
    if random.chance(3) {
        match random.below(8) {
            0 => caps.set("message", [40, 64, 200, 4096, 65536][random.below(5)]),
            1 => caps.set("text", [3, 8, 64][random.below(3)]),
            // The page's URL is 34 bytes: a cap at it, either side of it, and far from it.
            2 => caps.set("url", [4, 16, 33, 34, 35, 40][random.below(6)]),
            3 => caps.set("messages", [0, 1, 2][random.below(3)]),
            4 => caps.set("commands", [1, 2][random.below(2)]),
            5 => caps.set("canvas_ops", [1, 2][random.below(2)]),
            6 => caps.set("pixels", [0, 4, 8, 64][random.below(4)]),
            _ => caps.set("value_text", [2, 8][random.below(2)]),
        };
    }
    caps
}

fn step(random: &mut Lcg) -> Json {
    let mut s = Json::array();
    match random.below(30) {
        0..=4 => {
            let mut m = parse(r#"{"type":"ops","ops":[]}"#);
            if random.chance(4) {
                m = message(random);
            }
            if random.below(10) < 7 {
                m.set("react", reaction(random, false));
            }
            s.push("receive").push(m);
        }
        5 => {
            s.push("want_frame");
        }
        6 => {
            s.push("report").push(messages(random, 4));
        }
        7..=9 => {
            s.push("frame").push(random.chance(2)).push(random.below(4) as i32);
        }
        10 => {
            s.push("width").push(number(random));
        }
        11 => {
            s.push("max").push(number(random));
        }
        12 => {
            s.push("theme").push(random.chance(2));
        }
        13 | 14 => {
            s.push("touch")
                .push(random.below(4) as i32)
                .push(number(random))
                .push(number(random))
                .push(random.below(3) as i32)
                .push(random.chance(2))
                .push(random.chance(2))
                .push(0)
                .push(random.below(3) as i32);
        }
        15 => {
            s.push("hover").push(random.below(2) as i32).push(number(random)).push(number(random)).push(random.below(16) as i32);
        }
        16 => {
            s.push("clear");
        }
        17 => {
            s.push("claim").push(number(random)).push(number(random));
        }
        18..=20 => {
            s.push("pump").push(if random.chance(6) { random.below(2) as i32 } else { 64 });
        }
        21 => {
            s.push("deliver").push(hex(&platform_bytes(random)));
        }
        22 => {
            let kind = if random.chance(2) { 0 } else { 96 + random.below(5) as i32 };
            let reply = if random.chance(3) { hex(&answer_bytes(random)) } else { String::new() };
            s.push("fault").push(random.below(8) as i32).push(if kind == 0 { random.below(8) as i32 } else { 0 }).push(kind).push(reply);
        }
        23 => {
            s.push("backend").push(!random.chance(4)).push(random.below(3) as i32);
        }
        24 | 25 => {
            let method = METHODS[random.below(METHODS.len())];
            s.push("push").push(method).push(reaction(random, method == "paint"));
        }
        26 => {
            let mut m = parse(r#"{"type":"trim"}"#);
            m.set("react", reaction(random, false));
            s.push("helper").push(m);
        }
        27 => {
            let seq = random.below(4) as u64;
            s.push("platform_receive").push(hex(&renderer_bytes(random, seq)));
        }
        28 if random.chance(8) => {
            s.push("platform_close");
        }
        28 => {
            let seq = random.below(4) as u64;
            s.push("platform_answer").push(hex(&renderer_bytes(random, seq)));
        }
        _ => {
            s.push("close");
        }
    }
    s
}

fn any_session(random: &mut Lcg) -> Json {
    let mut steps = Json::array();
    if random.chance(6) {
        steps.push(step(random));
    }
    let open = Json::from(vec![
        Json::from("open"),
        Json::from([300.0, 0.5, 1e300, -1.0, 0.0][random.below(5)]),
        Json::from([1.0, 2.0, 3.0][random.below(3)]),
        Json::from(random.chance(2)),
    ]);
    steps.push(open);
    for _ in 0..5 + random.below(36) {
        steps.push(step(random));
    }
    if random.chance(2) {
        steps.push(parse(r#"["pump",64]"#));
    }
    let mut s = Json::object();
    s.set("caps", any_caps(random)).set("script", script(random)).set("steps", steps);
    s
}

#[test]
fn written_sessions_alike() {
    let Some(oracle) = Oracle::find("ends_oracle") else { return };
    let mut c = Comparison::new();
    let sessions = written_sessions();
    for (name, session) in &sessions {
        case(&mut c, session, name.clone(), &mut Seen::default());
    }
    let tally = c.check(&oracle);
    eprintln!("written sessions: {} transcripts alike", tally[1]);
}

#[test]
fn generated_sessions_alike() {
    let Some(oracle) = Oracle::find("ends_oracle") else { return };
    let mut random = Lcg::new(0xe0d5_0001);
    let mut c = Comparison::new();
    let mut steps = 0;
    let mut seen = Seen::default();
    for i in 0..4000 {
        let session = any_session(&mut random);
        steps += session.get("steps").size();
        case(&mut c, &session, format!("session {i}"), &mut seen);
    }
    let tally = c.check(&oracle);
    eprintln!("generated sessions: {} transcripts alike, {steps} steps", tally[1]);
    for (what, n) in &seen.0 {
        eprintln!("  {n} sessions: {what}");
    }
    let stops: Vec<&String> = seen.0.keys().filter(|k| k.starts_with("stop")).collect();
    assert!(stops.len() >= 6, "the sessions stop the end every way ({stops:?})");
}

/// Sessions that paint many frames of lists sharing runs, under delta_base and frame caps of every size, with widths
/// batches, a delta changed on its way now and then, and a platform end closed by its embedder: frames whole and as
/// deltas, rebuilt by the platform end, through both ends.
#[test]
fn delta_sessions_alike() {
    let Some(oracle) = Oracle::find("ends_oracle") else { return };
    let mut random = Lcg::new(0xe0d5_0002);
    let mut c = Comparison::new();
    let mut seen = Seen::default();
    for i in 0..1500 {
        let mut caps = Json::object();
        match random.below(6) {
            0 => {
                caps.set("delta_base", [0, 1, 6, 64, 256, 4096][random.below(6)]);
            }
            1 => {
                caps.set("frame", [100, 200, 400, 4096][random.below(4)]);
            }
            2 => {
                caps.set("commands", [4, 16][random.below(2)]);
            }
            3 => {
                caps.set("widths", [1, 4][random.below(2)]).set("widths_text", [8, 64][random.below(2)]);
            }
            _ => {}
        }
        let frames = 4 + random.below(24);
        let mut base: Vec<Json> = (0..2 + random.below(30)).map(|_| painted_command(&mut random)).collect();
        let mut paints = Vec::new();
        for _ in 0..frames {
            for _ in 0..1 + random.below(2) {
                match random.below(6) {
                    0 if !base.is_empty() => {
                        let at = random.below(base.len());
                        base.remove(at);
                    }
                    1 | 2 => {
                        let at = random.below(base.len() + 1);
                        base.insert(at, painted_command(&mut random));
                    }
                    3 if !base.is_empty() => {
                        let at = random.below(base.len());
                        base[at] = painted_command(&mut random);
                    }
                    _ => {}
                }
            }
            let mut r = Json::object();
            r.set("list", Json::from(base.clone()));
            if random.chance(4) {
                r.set("calls", Json::from(vec![widths_call(&mut random)]));
            }
            paints.push(r);
        }
        let mut on = Json::object();
        on.set("paint", Json::from(paints));
        let mut script = Json::object();
        script.set("ctor", parse(r#"{"calls":[["font","sans-serif",16,400,false,""],["font","serif",12,700,false,""]]}"#));
        script.set("on", on);
        let mut steps = Json::array();
        steps.push(parse(r#"["open",300,2,false]"#));
        for k in 0..frames {
            match random.below(12) {
                0 => {
                    let mut m = parse(r#"{"type":"ops","ops":[]}"#);
                    let mut react = Json::object();
                    react.set("calls", Json::from(vec![widths_call(&mut random)])).set("set", parse(r#"{"dirty":true}"#));
                    m.set("react", react);
                    steps.push(Json::from(vec![Json::from("receive"), m]));
                }
                1 => {
                    let seq = (k + 1) as u64;
                    let mut bytes = renderer_bytes(&mut random, seq);
                    if random.chance(2) {
                        bytes = encoded_message(&FrameDelta {
                            seq,
                            state: State::default(),
                            base: seq.saturating_sub(1),
                            runs: vec![DeltaRun {
                                offset: random.below(40) as u32,
                                length: 1 + random.below(30) as u32,
                                literal: Vec::new(),
                            }],
                            damage: None,
                        });
                        mutate(&mut random, &mut bytes);
                    }
                    steps.push(Json::from(vec![Json::from("platform_receive"), Json::from(hex(&bytes))]));
                }
                2 if random.chance(10) => {
                    steps.push(parse(r#"["platform_close"]"#));
                }
                3 => {
                    steps.push(parse(r#"["frame",false,1]"#));
                }
                _ => {}
            }
            steps.push(Json::from(vec![Json::from("frame"), Json::from(random.chance(2)), Json::from([1, 2, 2][random.below(3)])]));
            steps.push(parse(r#"["pump",64]"#));
        }
        steps.push(parse(r#"["close"]"#));
        let mut session = Json::object();
        session.set("caps", caps).set("script", script).set("steps", steps);
        case(&mut c, &session, format!("delta session {i}"), &mut seen);
    }
    let tally = c.check(&oracle);
    eprintln!("delta sessions: {} transcripts alike", tally[1]);
    for (what, n) in &seen.0 {
        eprintln!("  {n}: {what}");
    }
    assert!(seen.0.get("traffic: frame deltas").is_some_and(|n| *n > 1000), "frames cross as deltas");
}
