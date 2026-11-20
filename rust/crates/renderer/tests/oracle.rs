//! The renderer against renderer.cpp (`oracle/renderer_oracle`): scripts through the C++ renderer with probes at its
//! calls into the document and stage 4.5's code, then through the port on the tape of those answers, every message,
//! returned value, backend call and the whole state compared after every step. Skipped without `TWIDGET_ORACLE_DIR`.

mod common;

use common::cases::{compare, report};
use common::inputs::{corpus_files, document, html_to_ops, recorded_sessions, widget_sandbox};
use common::scripts::{patch_script, query};
use common::*;
use twidget_testkit::{Lcg, Oracle};

fn oracle() -> Option<Oracle> {
    Oracle::find("renderer_oracle")
}

#[test]
fn smoke() {
    let Some(oracle) = oracle() else { return };
    let mut scripts = Vec::new();
    let mut s = Script::new(40);
    s.ops(0.0, r#"[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},{"op":"el","id":10,"tag":"div","attrs":[["style","width:100px;height:30px"]]},{"op":"ins","parent":4,"id":10,"before":null}]"#);
    s.query(0.0, 1, r#"{"q":"rect","id":10}"#);
    s.ops(16.0, r#"[{"op":"style","id":10,"name":"height","value":"40px"}]"#);
    s.after(16.0, Step::Tick);
    s.after(16.0, Step::LastHtml);
    scripts.push((s, "smoke".to_string()));
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("smoke", scripts.len(), &tally);
}

#[test]
fn generated_patches() {
    let Some(oracle) = oracle() else { return };
    let mut r = Lcg::new(0x4301);
    let scripts: Vec<(Script, String)> = (0..600).map(|i| (patch_script(&mut r, 40), format!("patch script {i}"))).collect();
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("generated patches", scripts.len(), &tally);
}

#[test]
fn recorded() {
    let Some(oracle) = oracle() else { return };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/recordings/corpus.twrec");
    let bytes = std::fs::read(&path).expect("corpus.twrec");
    let scripts: Vec<(Script, String)> =
        recorded_sessions(&bytes).into_iter().enumerate().map(|(i, s)| (s, format!("corpus.twrec session {i}"))).collect();
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("corpus.twrec", scripts.len(), &tally);
}

#[test]
fn recorded_and_changed() {
    let Some(oracle) = oracle() else { return };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/recordings/corpus.twrec");
    let sessions = recorded_sessions(&std::fs::read(&path).expect("corpus.twrec"));
    let mut r = Lcg::new(0x4306);
    let mut scripts = Vec::new();
    for (n, session) in sessions.iter().enumerate() {
        for v in 0..40 {
            // The session with queries, batches, host messages, settings and frames put in between its steps.
            let mut s = session.clone();
            let max_id = s.max_id.max(3);
            let mut steps = Vec::with_capacity(s.steps.len() + 40);
            for (t, step) in std::mem::take(&mut s.steps) {
                steps.push((t, step));
                if r.chance(4) {
                    let extra = match r.below(7) {
                        0 | 1 => Step::Message(query(&mut r, max_id).into_bytes()),
                        2 => Step::Message(common::scripts::host_message(&mut r).into_bytes()),
                        3 => Step::Ops(common::scripts::patch_batch(&mut r).into_bytes()),
                        4 => Step::Inspect,
                        5 => Step::Width(*common::inputs::pick(&mut r, &[280.0, 320.0, 200.0])),
                        _ => Step::Tick,
                    };
                    steps.push((t, extra));
                }
            }
            s.steps = steps;
            scripts.push((s, format!("corpus.twrec session {n} changed {v}")));
        }
    }
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("corpus.twrec changed", scripts.len(), &tally);
}

#[test]
fn corpus_widgets() {
    let Some(oracle) = oracle() else { return };
    let _ = widget_sandbox();
    let files = corpus_files(&["corpus"]);
    let mut r = Lcg::new(0x4302);
    let mut scripts = Vec::new();
    for path in &files {
        let source = std::fs::read(path).expect("a corpus file");
        let (ops, last) = html_to_ops(&source, 4);
        let max_id = (last as u32).min(60000);
        let mut s = Script::new(max_id);
        s.width = *common::inputs::pick(&mut r, &[300.0, 320.0, 412.5]);
        s.after(0.0, Step::Ops(document(&ops).into_bytes()));
        for _ in 0..6 {
            match r.below(5) {
                0 => {
                    s.after(16.0, Step::Tick);
                }
                1 => {
                    s.after(300.0, Step::Tick);
                }
                2 => {
                    let q = query(&mut r, max_id.max(3));
                    s.after(0.0, Step::Message(q.into_bytes()));
                }
                3 => {
                    s.after(16.0, Step::LastHtml);
                }
                _ => {
                    s.after(16.0, Step::Inspect);
                }
            }
        }
        scripts.push((s, path.display().to_string()));
    }
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("corpus widgets", scripts.len(), &tally);
}

#[test]
fn budget_and_names() {
    let Some(oracle) = oracle() else { return };
    let mut r = Lcg::new(0x4303);
    let mut scripts: Vec<(Script, String)> =
        (0..48).map(|i| (common::scripts::budget_script(&mut r, i), format!("budget script {i}"))).collect();
    // A process-wide name table: each script's names are its own.
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    for i in 0..6 {
        scripts.push((common::scripts::names_script(&mut r, &format!("z{stamp:x}x{i}")), format!("names script {i}")));
    }
    for name in
        ["render-nested-flex", "render-nested-grid", "render-nested-table", "deep-nesting", "layout-query-loop", "render-var-expansion"]
    {
        let path = widget_sandbox().join(format!("corpus/attacks/{name}.html"));
        let Ok(source) = std::fs::read(&path) else { continue };
        let (ops, last) = html_to_ops(&source, 4);
        let mut s = Script::new((last as u32).min(60000));
        s.after(0.0, Step::Ops(document(&ops).into_bytes()));
        s.after(16.0, Step::Tick);
        s.query(0.0, 1, "{\"q\":\"rect\",\"id\":5}");
        s.after(16.0, Step::LastHtml);
        s.after(16.0, Step::Inspect);
        scripts.push((s, name.to_string()));
    }
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("budget and names", scripts.len(), &tally);
    assert!(tally.count("violation layout limit") >= 10, "the layout budget is crossed ({})", tally.count("violation layout limit"));
    assert!(tally.count("violation name limit") >= 3, "the name table's bounds are crossed ({})", tally.count("violation name limit"));
}

#[test]
fn themes_animations_resizes_flushes_frozen() {
    let Some(oracle) = oracle() else { return };
    let mut r = Lcg::new(0x4304);
    let mut scripts = Vec::new();
    for i in 0..150 {
        scripts.push((common::scripts::theme_script(&mut r), format!("theme script {i}")));
        scripts.push((common::scripts::animation_script(&mut r), format!("animation script {i}")));
        scripts.push((common::scripts::resize_script(&mut r), format!("resize script {i}")));
        scripts.push((common::scripts::frozen_script(&mut r), format!("frozen script {i}")));
    }
    for i in 0..12 {
        scripts.push((common::scripts::flush_script(&mut r), format!("flush script {i}")));
    }
    for i in 0..120 {
        scripts.push((common::scripts::scripted_script(&mut r), format!("scripted layers script {i}")));
    }
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("themes, animations, resizes, flushes, frozen, scripted layers", scripts.len(), &tally);
}

#[test]
fn generated_documents_with_input() {
    let Some(oracle) = oracle() else { return };
    let mut r = Lcg::new(0x4305);
    let mut scripts = Vec::new();
    for i in 0..600 {
        let html = common::inputs::generated_html(&mut r);
        let (ops, last) = html_to_ops(html.as_bytes(), 4);
        let mut s = common::inputs::script_over(&mut r, &ops, last as u32, 12);
        for _ in 0..4 {
            let q = query(&mut r, (last as u32).max(3));
            s.after(16.0, Step::Message(q.into_bytes()));
            let b = common::scripts::patch_batch(&mut r);
            s.after(16.0, Step::Ops(b.into_bytes()));
        }
        s.after(16.0, Step::Inspect);
        scripts.push((s, format!("generated document {i}")));
    }
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("generated documents with input", scripts.len(), &tally);
}
