//! The renderer on `twidget_document`'s engine against renderer.cpp (`oracle/renderer_oracle`), on every script set
//! the renderer's own comparisons replay (stage 4.3's: corpus.twrec and its changes, the corpus widgets, patch scripts,
//! documents with input, themes, animations, resizes, flushes, frozen, scripted layers, past the budget or the name
//! table; stage 4.5's: layer pages, heavy boxes, the paint-policy fixture, damage, the throttle, the corpus and stress
//! widgets composited; the bot's profile page and a bubble's place): every answer the engine gives checked against the
//! C++'s, then every transcript end to end. Skipped without `TWIDGET_ORACLE_DIR`; the last tests are the port's own,
//! where it differs from renderer.cpp: a page's ceiling and the host's bound across its viewport, the host's names in
//! the root's style attribute, a size held back before a place's.

#[path = "../../renderer/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;
use std::sync::Arc;

use common::inputs::{corpus_files, document, html_to_ops, recorded_sessions, widget_sandbox};
use common::layer_scripts::{damage_script, fixture_scripts, heavy_script, layer_script, throttle_script, widget_frames};
use common::scripts::{patch_script, query};
use common::*;
use support::compare_engines;
use twidget_document::{DocumentEngine, Process};
use twidget_json::Json;
use twidget_testkit::{Lcg, Oracle};

fn oracle() -> Option<Oracle> {
    Oracle::find("renderer_oracle")
}

/// How many of a set to run: all, unless `TWIDGET_DOCUMENT_SAMPLE` asks for fewer while iterating.
fn sample<T>(mut v: Vec<T>) -> Vec<T> {
    if let Ok(n) = std::env::var("TWIDGET_DOCUMENT_SAMPLE")
        && let Ok(n) = n.parse::<usize>()
    {
        v.truncate(n);
    }
    v
}

#[test]
fn smoke() {
    let Some(oracle) = oracle() else { return };
    let mut s = Script::new(40);
    s.ops(0.0, r#"[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},{"op":"el","id":10,"tag":"div","attrs":[["style","width:100px;height:30px"]]},{"op":"ins","parent":4,"id":10,"before":null}]"#);
    s.query(0.0, 1, r#"{"q":"rect","id":10}"#);
    s.ops(16.0, r#"[{"op":"style","id":10,"name":"height","value":"40px"}]"#);
    s.after(16.0, Step::Tick);
    s.after(16.0, Step::Paint);
    s.after(16.0, Step::LastHtml);
    compare_engines(&oracle, &[(s, "smoke".to_string())], "smoke");
}

#[test]
fn generated_patches() {
    let Some(oracle) = oracle() else { return };
    let mut r = Lcg::new(0x4301);
    let scripts: Vec<(Script, String)> = (0..600).map(|i| (patch_script(&mut r, 40), format!("patch script {i}"))).collect();
    compare_engines(&oracle, &sample(scripts), "generated patches");
}

#[test]
fn recorded() {
    let Some(oracle) = oracle() else { return };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/recordings/corpus.twrec");
    let bytes = std::fs::read(&path).expect("corpus.twrec");
    let scripts: Vec<(Script, String)> =
        recorded_sessions(&bytes).into_iter().enumerate().map(|(i, s)| (s, format!("corpus.twrec session {i}"))).collect();
    compare_engines(&oracle, &sample(scripts), "corpus.twrec");
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
    compare_engines(&oracle, &sample(scripts), "corpus.twrec changed");
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
    compare_engines(&oracle, &sample(scripts), "corpus widgets");
}

#[test]
fn budget_and_names() {
    let Some(oracle) = oracle() else { return };
    let mut r = Lcg::new(0x4303);
    let mut scripts: Vec<(Script, String)> =
        (0..48).map(|i| (common::scripts::budget_script(&mut r, i), format!("budget script {i}"))).collect();
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
    compare_engines(&oracle, &sample(scripts), "budget and names");
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
    compare_engines(&oracle, &sample(scripts), "themes, animations, resizes, flushes, frozen, scripted layers");
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
    compare_engines(&oracle, &sample(scripts), "generated documents with input");
}

// ---- Stage 4.5's sets: the paint, layers and heavy boxes ------------------------------------------------------

#[test]
fn generated_layer_pages() {
    let Some(oracle) = oracle() else { return };
    let mut r = Lcg::new(0x4501);
    let scripts: Vec<(Script, String)> = (0..400).map(|i| (layer_script(&mut r), format!("layer script {i}"))).collect();
    compare_engines(&oracle, &sample(scripts), "generated layer pages");
}

#[test]
fn heavy_boxes_across_the_limits() {
    let Some(oracle) = oracle() else { return };
    let mut r = Lcg::new(0x4502);
    let scripts: Vec<(Script, String)> = (0..400).map(|i| (heavy_script(&mut r), format!("heavy script {i}"))).collect();
    compare_engines(&oracle, &sample(scripts), "heavy boxes across the limits");
}

#[test]
fn paint_policy_fixture_as_documents() {
    let Some(oracle) = oracle() else { return };
    let mut scripts = Vec::new();
    for policy in ["", "{\"type\":\"policy\",\"policy\":\"compat\"}", "{\"type\":\"policy\",\"policy\":\"strict\",\"paintReports\":true}"] {
        for compositing in [false, true] {
            scripts.extend(fixture_scripts(policy, compositing));
        }
    }
    compare_engines(&oracle, &sample(scripts), "paint-policy fixture as documents");
}

#[test]
fn damage_between_paints() {
    let Some(oracle) = oracle() else { return };
    let mut r = Lcg::new(0x4503);
    let scripts: Vec<(Script, String)> = (0..200).map(|i| (damage_script(&mut r), format!("damage script {i}"))).collect();
    compare_engines(&oracle, &sample(scripts), "damage between paints");
}

#[test]
fn the_render_throttle() {
    let Some(oracle) = oracle() else { return };
    let mut r = Lcg::new(0x4504);
    let scripts: Vec<(Script, String)> = (0..150).map(|i| (throttle_script(&mut r), format!("throttle script {i}"))).collect();
    compare_engines(&oracle, &sample(scripts), "the render throttle");
}

#[test]
fn corpus_and_stress_widgets_composited() {
    let Some(oracle) = oracle() else { return };
    let _ = widget_sandbox();
    let mut files = corpus_files(&["corpus"]);
    files.extend(corpus_files(&["packages/stress/widgets"]));
    let mut r = Lcg::new(0x4505);
    let mut scripts = Vec::new();
    for (i, path) in files.iter().enumerate() {
        let source = std::fs::read(path).expect("a corpus file");
        let (ops, last) = html_to_ops(&source, 4);
        let max_id = (last as u32).min(60000);
        let policy = match i % 3 {
            0 => "",
            1 => "{\"type\":\"policy\",\"policy\":\"compat\"}",
            _ => "{\"type\":\"policy\",\"policy\":\"strict\",\"paintReports\":true}",
        };
        scripts.push((widget_frames(&mut r, &ops, max_id, policy, i % 5 != 4), path.display().to_string()));
    }
    compare_engines(&oracle, &sample(scripts), "corpus and stress widgets composited");
}

#[test]
fn the_bots_profile_page() {
    let Some(oracle) = oracle() else { return };
    compare_engines(&oracle, &common::scripts::profile_scripts(), "the bot's profile page and a bubble's place");
}

/// The script on the port's engine alone.
fn on_the_port(script: &Script) -> Run {
    let shared: common::tape::Tape = Rc::default();
    let log = shared.borrow().log.clone();
    let engine = DocumentEngine::new(Arc::new(Process::default()), FontStub::logging(log));
    run_on(script, Some(shared), engine, &mut Tally::default())
}

#[test]
fn a_hosts_bound_and_the_page_ceiling() {
    for page in [true, false] {
        let m = common::scripts::host_bounds(page);
        let r = on_the_port(&m.script);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let viewport = |name: &str| r.viewports[m.at(name)];
        let laid_out = |name: &str| r.layouts[m.at(name)] - r.layouts[m.at(name) - 1];
        let reported = |name: &str| sizes(&r.outs[m.at(name)]);
        let got = ["load", "raised", "endless", "lowered", "between"].map(viewport);
        if page {
            assert_eq!(
                got,
                [4000, 4000, 4000, 844, 3000],
                "a page stays within its ceiling whatever the host's bound, and at least its viewport"
            );
            for name in ["raised", "endless"] {
                assert!(laid_out(name) == 0 && reported(name).is_empty(), "{name}: a bound past the page's ceiling changes nothing");
            }
            let got = ["bound", "same viewport", "lower viewport", "higher viewport"].map(viewport);
            assert_eq!(got, [1200, 1200, 1200, 1500], "the host's bound holds across the page's viewport, which stays its floor");
            assert_eq!(reported("bound"), [(9000.0, 1200.0)]);
            assert!(
                laid_out("same viewport") == 0 && reported("same viewport").is_empty(),
                "the same viewport again lays out nothing and reports nothing"
            );
            assert!(reported("lower viewport").is_empty(), "a lower viewport leaves the bound and the size as they were");
            assert_eq!(reported("higher viewport"), [(9000.0, 1500.0)], "one above the bound is the page's height");
        } else {
            assert_eq!(got, [600, 8000, 9000, 100, 3000], "a bubble keeps the host's own bound, up to the document");
        }
    }
}

#[test]
fn the_hosts_names_in_the_roots_style_attribute() {
    for page in [false, true] {
        let m = common::scripts::root_style(page);
        let r = on_the_port(&m.script);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let text = |name: &str| String::from_utf8_lossy(result(&r.outs[m.at(name)]).as_str()).into_owned();
        let f = |state: &str| result(&r.outs[m.at(&format!("{state} f"))]).get("height").as_number(-1.0);
        let host = if page { "844px" } else { "" };
        for (state, var, own, height) in [
            ("load", host, "", if page { 844.0 } else { 30.0 }),
            ("adopted", host, "4px", if page { 844.0 } else { 30.0 }),
            ("set", host, "5px", if page { 844.0 } else { 30.0 }),
            ("withdrawn", "", "5px", 30.0),
        ] {
            if state == "withdrawn" && !page {
                continue;
            }
            assert_eq!(
                (text(&format!("{state} var")), text(&format!("{state} own")), f(state)),
                (var.to_string(), own.to_string(), height),
                "{state} on a page: {page}: the host's value or none, never the widget's, and the widget's own variable"
            );
        }
    }
}

#[test]
fn a_held_size_goes_before_a_places() {
    let size = |height: u32, inner: u32| {
        format!("{{\"type\":\"size\",\"width\":390,\"height\":{height},\"innerWidth\":390,\"innerHeight\":{inner},\"dpr\":1}}")
    };
    let resize = |height: u32| format!("{{\"type\":\"resize\",\"entries\":[{{\"id\":10,\"width\":390,\"height\":{height}}}]}}");
    let ended = || {
        "{\"type\":\"event\",\"ev\":{\"type\":\"animationend\",\"targetId\":10,\"animationName\":\"fade\",\"elapsedTime\":0.06},\"trusted\":true}".to_string()
    };
    let content = |bottom: u32, corner_left: u32| {
        common::scripts::safe_area(&format!(
            "{{\"top\":0,\"right\":0,\"bottom\":{bottom},\"left\":0,\"cornerLeft\":{corner_left},\"cornerRight\":0}}"
        ))
    };
    let device = common::scripts::device_safe_area("{\"top\":20,\"right\":0,\"bottom\":0,\"left\":0}");
    let viewport = common::scripts::surface("profile", "1500");
    let mut failed = Vec::new();
    for (page, late, update, inner, reply) in [
        (true, false, device, vec![1000.0, 1020.0], vec![size(1000, 1000), size(1020, 1020), resize(1000), ended()]),
        (true, false, content(30, 0), vec![1000.0, 1030.0], vec![size(1000, 1000), size(1030, 1030), resize(1000), ended()]),
        (true, false, viewport.clone(), vec![1000.0, 1500.0], vec![size(1000, 1000), size(1000, 1500), resize(1000), ended()]),
        (false, false, content(0, 12), vec![200.0], vec![resize(200), size(200, 200), ended()]),
        (true, true, viewport, vec![1000.0, 1500.0], vec![size(1000, 1000), size(1000, 1500), resize(1000), ended()]),
        (false, true, content(10, 0), vec![200.0, 210.0], vec![size(200, 200), size(210, 210), resize(200), ended()]),
        (false, true, content(0, 12), vec![200.0], vec![resize(200), ended(), size(200, 200)]),
    ] {
        let m = common::scripts::held_size(page, late, &update);
        let r = on_the_port(&m.script);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let steps = ["claim", "update", "tick"];
        let got: Vec<f64> = steps.iter().flat_map(|name| sizes(&r.outs[m.at(name)])).map(|s| s.1).collect();
        let replies = steps.map(|name| r.outs[m.at(name)].iter().map(Json::dump_lossy).collect::<Vec<_>>());
        if got != inner || replies != [Vec::new(), reply.clone(), Vec::new()] {
            failed.push(format!(
                "{update} (page: {page}, late claim: {late}): innerHeights {got:?}, expected {inner:?}; claim, update, tick \
                 {replies:?}, expected the update {reply:?}"
            ));
        }
    }
    assert!(
        failed.is_empty(),
        "the claim's size first, the update's last and once; a new size after the held sizes, then the resize entries and \
         every other message in its order; an unchanged one leaves them all to the flush, as renderer.cpp does:\n{}",
        failed.join("\n")
    );
}
