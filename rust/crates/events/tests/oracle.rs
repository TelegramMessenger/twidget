//! The Rust input against the C++ renderer (`oracle/events_oracle`: renderer.cpp with probes at its input code, on
//! the whole core and litehtml): scripts of messages, pointers, hovers, frames, paints, edits and selections through
//! one renderer on both sides. The C++'s document answers every question the input asks of it (a tape the port
//! replays, question for question, each checked to be the C++'s with its arguments); after every step the messages, what
//! the step returned and the input's whole state are compared byte for byte. Skipped without `TWIDGET_ORACLE_DIR`;
//! `tools/gate.py` sets it.

mod common;

use common::cases::{compare, report};
use common::inputs::*;
use common::targeted::*;
use common::*;
use twidget_testkit::{Lcg, Oracle};

fn corpus_recording() -> Vec<Script> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/recordings/corpus.twrec");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    recorded_sessions(&bytes)
}

#[test]
fn recorded_sessions_alike() {
    let Some(oracle) = Oracle::find("events_oracle") else { return };
    let sessions = corpus_recording();
    assert_eq!(sessions.len(), 4, "corpus.twrec holds four sessions");
    let mut scripts: Vec<(Script, String)> = sessions.iter().enumerate().map(|(i, s)| (s.clone(), format!("session {i}"))).collect();
    // Each session again with gestures after its own steps.
    for seed in 0..200u64 {
        let mut r = Lcg::new(seed + 1);
        let i = seed as usize % sessions.len();
        let mut s = sessions[i].clone();
        let listen = format!(
            "{{\"type\":\"listen\",\"types\":[{}]}}",
            twidget_events::PROFILE_EVENTS.iter().map(|t| quote(&String::from_utf8_lossy(t))).collect::<Vec<_>>().join(",")
        );
        s.after(0.0, Step::Message(listen.into_bytes()));
        gestures(&mut r, &mut s, 30);
        scripts.push((s, format!("session {i} and gestures of seed {seed}")));
    }
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("recorded sessions", scripts.len(), &tally);
    assert!(tally.count("step touch") > 1000 && tally.count("step hover") > 100, "the sessions press and hover");
}

fn corpus_scripts(dirs: &[&str], seeds: u64, gestures_n: usize) -> Vec<(Script, String)> {
    let mut scripts = Vec::new();
    for (k, path) in corpus_files(dirs).iter().enumerate() {
        let source = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let (ops, last) = html_to_ops(&source, 10);
        let name = path.strip_prefix(widget_sandbox()).unwrap_or(path).display().to_string();
        for seed in 0..seeds {
            let mut r = Lcg::new((k as u64) << 8 | seed);
            let s = script_over(&mut r, &ops, last.min(60000) as u32, gestures_n);
            scripts.push((s, format!("{name}, seed {seed}")));
        }
    }
    scripts
}

#[test]
fn gesture_and_interactive_widgets_alike() {
    let Some(oracle) = Oracle::find("events_oracle") else { return };
    let scripts = corpus_scripts(&["corpus/gestures", "corpus/interactive", "corpus/showcase"], 12, 60);
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("gesture, interactive and showcase widgets", scripts.len(), &tally);
}

#[test]
fn attack_conformance_and_stress_widgets_alike() {
    let Some(oracle) = Oracle::find("events_oracle") else { return };
    let scripts = corpus_scripts(&["corpus/attacks", "corpus/out-of-profile", "packages/conformance", "packages/stress"], 1, 24);
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("attack, conformance and stress widgets", scripts.len(), &tally);
}

#[test]
fn corpus_widgets_alike() {
    let Some(oracle) = Oracle::find("events_oracle") else { return };
    let scripts = corpus_scripts(
        &[
            "corpus/compat",
            "corpus/common",
            "corpus/generated",
            "corpus/htmlbubbles-ported",
            "corpus/bot-demos",
            "corpus/defaults",
            "corpus/resize",
            "corpus/safe-area",
        ],
        1,
        30,
    );
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("corpus widgets", scripts.len(), &tally);
}

#[test]
fn selection_scripts_alike() {
    let Some(oracle) = Oracle::find("events_oracle") else { return };
    let scripts = targeted(0x5E1E0..0x5E1E0 + 400, selection_html, selection_gestures, 24, "selection script");
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("selection scripts", scripts.len(), &tally);
    assert!(
        tally.count("selection observed") > 500 && tally.count("tints painted") > 200 && tally.count("asked layer_shown") > 50,
        "the scripts select, paint and select in layers"
    );
}

#[test]
fn capture_scripts_alike() {
    let Some(oracle) = Oracle::find("events_oracle") else { return };
    let scripts = targeted(0xCA9_0000..0xCA9_0000 + 300, generated_html, capture_gestures, 20, "capture script");
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("capture scripts", scripts.len(), &tally);
    assert!(tally.count("asked has_element") > 300, "the scripts capture pointers");
}

#[test]
fn editor_scripts_alike() {
    let Some(oracle) = Oracle::find("events_oracle") else { return };
    let scripts = targeted(0xED17_0000..0xED17_0000 + 300, editor_html, editor_gestures, 40, "editor script");
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("editor scripts", scripts.len(), &tally);
    assert!(tally.count("asked editor_style") > 1000 && tally.count("editor edit") > 1000, "the scripts edit");
}

#[test]
fn generated_documents_alike() {
    let Some(oracle) = Oracle::find("events_oracle") else { return };
    let mut scripts = Vec::new();
    for seed in 0..1500u64 {
        let mut r = Lcg::new(0xE7E0 + seed);
        let html = generated_html(&mut r);
        let (ops, last) = html_to_ops(html.as_bytes(), 10);
        let s = script_over(&mut r, &ops, last as u32, 40);
        scripts.push((s, format!("generated document {seed}")));
    }
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("generated documents", scripts.len(), &tally);
}

/// Twenty nested grids under an element, displayed when `shown` matches it: a layout past the budget.
fn nested_grids(parent: i64, first: i64) -> String {
    let mut ops = String::new();
    let mut parent = parent;
    let mut id = first;
    for _ in 0..20 {
        let (g, c, t) = (id, id + 1, id + 2);
        ops.push_str(&format!(
            r#",{{"op":"el","id":{g},"tag":"div","attrs":[["class","g"]]}},{{"op":"el","id":{c},"tag":"div","attrs":[]}},{{"op":"text","id":{t},"data":"x"}},{{"op":"ins","parent":{c},"id":{t},"before":null}},{{"op":"ins","parent":{g},"id":{c},"before":null}},{{"op":"ins","parent":{parent},"id":{g},"before":null}}"#
        ));
        parent = id;
        id += 3;
    }
    ops
}

#[test]
fn stops_inside_the_input_alike() {
    let Some(oracle) = Oracle::find("events_oracle") else { return };
    let mut scripts = Vec::new();
    for seed in 0..160u64 {
        let mut r = Lcg::new(0x5709 + seed);
        let mut s = Script::new(200);
        let hovered = r.chance(2);
        let css = if hovered {
            "body{margin:0}#t{width:100px;min-height:40px;transition:width 1s linear}#t:hover{width:200px}#t:active{width:150px}#b{display:none}#t:hover #b,#t:active #b{display:block}.g{display:grid}@keyframes spin{to{transform:rotate(360deg)}}.s{animation:spin 1s infinite}"
        } else {
            "body{margin:0}#t{width:100px;min-height:40px}.g{display:grid}@keyframes spin{to{transform:rotate(360deg)}}.s{animation:spin 1s infinite}"
        };
        let mut ops = format!(
            r#"[{ROOTS},{{"op":"el","id":5,"tag":"style","attrs":[]}},{{"op":"css","id":5,"text":"{css}"}},{{"op":"ins","parent":3,"id":5,"before":null}},{{"op":"el","id":10,"tag":"div","attrs":[["id","t"]]}},{{"op":"ins","parent":4,"id":10,"before":null}},{{"op":"el","id":11,"tag":"div","attrs":[["id","b"]]}},{{"op":"ins","parent":10,"id":11,"before":null}},{{"op":"el","id":12,"tag":"div","attrs":[["class","s"],["style","width:20px;height:20px"]]}},{{"op":"ins","parent":4,"id":12,"before":null}},{{"op":"el","id":13,"tag":"input","attrs":[["value","v"]]}},{{"op":"ins","parent":4,"id":13,"before":null}}"#
        );
        if hovered {
            ops.push_str(&nested_grids(11, 100));
        }
        ops.push(']');
        s.after(0.0, Step::Ops(ops.into_bytes()));
        let all = twidget_events::PROFILE_EVENTS.iter().map(|t| quote(&String::from_utf8_lossy(t))).collect::<Vec<_>>().join(",");
        s.after(0.0, Step::Message(format!("{{\"type\":\"listen\",\"types\":[{all}]}}").into_bytes()));
        // Something to hold: a press, a hover, an editor, coalesced moves.
        gestures(&mut r, &mut s, 6);
        if !hovered {
            // A batch the animation makes wait for its frame, built by the next input's settle.
            let grids = format!("[{}]", &nested_grids(11, 100)[1..]);
            s.after(600.0, Step::Ops(grids.into_bytes()));
        }
        let mouse = r.chance(2);
        let x = 20.0 + r.below(60) as f64;
        match r.below(4) {
            0 => {
                s.after(16.0, Step::Hover { action: 0, x, y: 20.0, modifiers: 0 });
            }
            1 => {
                s.after(16.0, Step::Touch { action: 0, x: 250.0, y: 300.0, pointer: 0, primary: true, mouse, modifiers: 0, button: 0 });
                s.after(16.0, Step::Touch { action: 1, x, y: 20.0, pointer: 0, primary: true, mouse, modifiers: 0, button: 0 });
            }
            2 => {
                s.after(16.0, Step::Touch { action: 0, x, y: 20.0, pointer: 0, primary: true, mouse, modifiers: 0, button: 0 });
            }
            _ => {
                s.after(16.0, Step::Claim { x, y: 20.0 });
            }
        }
        gestures(&mut r, &mut s, 8);
        scripts.push((s, format!("stop script {seed}")));
    }
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("stops inside the input", scripts.len(), &tally);
    assert!(tally.count("message violation") > 50, "the scripts cross the layout limit");
}

/// What the generators seldom reach: a capture whose element leaves the document mid-press, a summary outside any
/// details, an image node replaced in its own batch, a long unbroken run of multibyte text dragged over.
#[test]
fn edge_cases_alike() {
    let Some(oracle) = Oracle::find("events_oracle") else { return };
    let page = |body: &str| format!(r#"[{ROOTS},{{"op":"attr","id":4,"name":"style","value":"margin:0;user-select:text"}}{body}]"#);
    let mut scripts = Vec::new();
    for mouse in [true, false] {
        for remove in ["rm", "hide"] {
            for end in [1, 2, 3] {
                let mut s = Script::new(20);
                s.ops(0.0, &page(r#",{"op":"el","id":10,"tag":"div","attrs":[["style","height:40px"]]},{"op":"ins","parent":4,"id":10,"before":null},{"op":"el","id":11,"tag":"div","attrs":[["style","height:40px"]]},{"op":"ins","parent":4,"id":11,"before":null}"#));
                s.message(0.0, r#"{"type":"listen","types":["pointermove","pointerup","pointercancel","mousemove"]}"#);
                s.touch(16.0, 0, 10.0, 10.0, 0, mouse);
                let id = if mouse { 1 } else { 2 };
                s.ops(16.0, &format!(r#"[{{"op":"call","id":10,"method":"setPointerCapture","args":[{id}]}}]"#));
                let op =
                    if remove == "rm" { r#"[{"op":"rm","id":10}]"# } else { r#"[{"op":"style","id":10,"name":"display","value":"none"}]"# };
                s.ops(16.0, op);
                s.touch(16.0, 1, 40.0, 50.0, 0, mouse);
                s.touch(16.0, end, 45.0, 50.0, 0, mouse);
                s.after(16.0, Step::Tick);
                scripts.push((s, format!("a capture removed ({remove}, mouse {mouse}, ending {end})")));
            }
        }
    }
    let mut s = Script::new(20);
    s.ops(0.0, &page(r#",{"op":"el","id":10,"tag":"summary","attrs":[]},{"op":"ins","parent":4,"id":10,"before":null},{"op":"text","id":11,"data":"loose"},{"op":"ins","parent":10,"id":11,"before":null}"#));
    s.touch(16.0, 0, 5.0, 5.0, 0, true);
    s.touch(16.0, 2, 5.0, 5.0, 0, true);
    scripts.push((s, "a summary outside details".into()));
    let mut s = Script::new(20);
    s.ops(0.0, &page(r#",{"op":"el","id":10,"tag":"img","attrs":[["src","data:image/png;base64,QUJD"]]},{"op":"rm","id":10},{"op":"el","id":10,"tag":"div","attrs":[]},{"op":"ins","parent":4,"id":10,"before":null}"#));
    scripts.push((s, "an image replaced in its batch".into()));
    for (text, nowrap) in [("é".repeat(2600), true), ("漢".repeat(1500), true), ("ab".repeat(2100), false)] {
        let mut s = Script::new(20);
        let style = if nowrap { "white-space:nowrap" } else { "" };
        s.ops(0.0, &page(&format!(r#",{{"op":"el","id":10,"tag":"p","attrs":[["style","{style}"]]}},{{"op":"ins","parent":4,"id":10,"before":null}},{{"op":"text","id":11,"data":"{text}"}},{{"op":"ins","parent":10,"id":11,"before":null}}"#)));
        s.after(16.0, Step::Paint);
        for (x0, x1) in [(-50.0, 900.0), (3.0, 37.0), (5000.0, 2.0), (301.0, 299.0), (3.0, 90000.0), (290.0, -50.0)] {
            s.touch(16.0, 0, x0, 10.0, 1, true);
            s.touch(16.0, 1, x1, 12.0, 1, true);
            s.touch(16.0, 2, x1, 12.0, 1, true);
            s.after(0.0, Step::Paint);
            s.after(0.0, Step::Observe);
        }
        scripts.push((s, format!("a long run of {} bytes", text.len())));
    }
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("edge cases", scripts.len(), &tally);
}
