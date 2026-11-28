//! Stage 4.5 against renderer.cpp: core_tests.cpp's checks of compositor layers, damage, scripted, shimmer, SVG,
//! canvas, Web Animation and nested layers, coverage and heavy boxes, by their names, on the port, with what the
//! document answers taken from the C++ renderer running the same script (`oracle/renderer_oracle`) and the port's
//! transcript compared with the C++'s; then this stage's comparisons: generated layer pages over frames, boxes across
//! the paint limits, the paint-policy fixture's cases as documents, damage between paints, the throttle, and the corpus
//! and stress widgets composited under both policies. Skipped without `TWIDGET_ORACLE_DIR`.

mod common;

use common::cases::{compare, report};
use common::inputs::{corpus_files, html_to_ops, widget_sandbox};
use common::layer_scripts::{damage_script, fixture_scripts, heavy_script, layer_script, throttle_script, widget_frames};
use common::*;
use twidget_json::Json;
use twidget_renderer::{Composition, Layer, LayerTrack};
use twidget_testkit::{Lcg, Oracle};
use twidget_wire::{Caps, Cmd, DisplayList, Rect, encode_list};

fn oracle() -> Option<Oracle> {
    Oracle::find("renderer_oracle")
}

/// The scripts through the oracle and the port, alike; None without the oracle.
fn replay_all(scripts: Vec<Script>) -> Option<Vec<Run>> {
    let oracle = oracle()?;
    let mut tally = Tally::default();
    let named: Vec<(Script, String)> = scripts
        .into_iter()
        .enumerate()
        .map(|(i, mut s)| {
            s.keep_paints = true;
            (s, format!("script {i}"))
        })
        .collect();
    Some(compare(&oracle, &named, &mut tally))
}

fn replay(script: Script) -> Option<Run> {
    replay_all(vec![script]).and_then(|mut r| r.pop())
}

fn ops_message(ops: &str) -> Step {
    Step::Ops(ops.as_bytes().to_vec())
}

/// An Inspect step's wants_tick() and next_tick_in().
fn inspected(extra: &[u8]) -> (bool, f64) {
    (extra[0] != 0, f64::from_le_bytes(extra[1..9].try_into().expect("eight bytes")))
}

fn layer(c: &Composition, id: i64) -> Option<&Layer> {
    c.layers.iter().find(|l| l.id == id)
}

fn red(list: &DisplayList) -> bool {
    list.iter().any(|c| c.kind == Cmd::FillRect && c.color.r == 255 && c.color.g == 0)
}

fn wire(list: &DisplayList) -> Vec<u8> {
    let mut out = Vec::new();
    encode_list(list, &Caps::default(), &mut out);
    out
}

/// The step index of the last step added.
fn last(s: &Script) -> usize {
    s.steps.len() - 1
}

/// The paint a step made.
fn painted(run: &Run, step: usize) -> &Painted {
    run.paints.iter().find(|p| p.step == step).expect("a paint at that step")
}

/// Where a track's key at `t` takes `(x, y)`, between the keys around it.
fn track_at(track: &LayerTrack, t: f32, x: f32, y: f32) -> (f32, f32) {
    let keys = &track.keys;
    for i in 0..keys.len().saturating_sub(1) {
        if t < keys[i].t || t > keys[i + 1].t {
            continue;
        }
        let u = (t - keys[i].t) / (keys[i + 1].t - keys[i].t);
        let (a, b) = (&keys[i].matrix, &keys[i + 1].matrix);
        return ((1.0 - u) * a.apply_x(x, y) + u * b.apply_x(x, y), (1.0 - u) * a.apply_y(x, y) + u * b.apply_y(x, y));
    }
    (0.0, 0.0)
}

const ROOTS: &str = r#"{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]}"#;

/// The element with `data-twid="id"` in the HTML, up to its tag's end.
fn element(html: &[u8], id: i64) -> String {
    let html = String::from_utf8_lossy(html);
    let key = format!("data-twid=\"{id}\"");
    let Some(start) = html.find(&key) else { return String::new() };
    let end = html[start..].find('>').map_or(html.len(), |e| start + e);
    html[start..end].to_string()
}

// ---- core_tests.cpp's checks, by their names ----------------------------------------------------------------

#[test]
fn compositor_layers() {
    let mut s = Script::new(80);
    s.dpr = 2.0;
    s.after(0.0, Step::Compositing(true));
    s.ops(0.0, &format!(r##"[{ROOTS},
     {{"op":"el","id":20,"tag":"style","attrs":[]}},
     {{"op":"css","id":20,"text":"#s{{width:40px;height:40px;margin:20px;background:#ff0000;animation:spin 1s linear infinite}} #t{{height:30px;background:#00ff00;transition:opacity 400ms linear}} #w{{height:10px;background:#0000ff;animation:grow 1s infinite}} @keyframes spin{{from{{transform:rotate(0deg)}}to{{transform:rotate(360deg)}}}} @keyframes grow{{from{{width:10px}}to{{width:90px}}}}"}},
     {{"op":"ins","parent":3,"id":20,"before":null}},
     {{"op":"el","id":40,"tag":"div","attrs":[["id","s"]]}},
     {{"op":"el","id":41,"tag":"div","attrs":[["id","t"]]}},
     {{"op":"ins","parent":4,"id":40,"before":null}},
     {{"op":"ins","parent":4,"id":41,"before":null}}]"##));
    s.after(0.0, Step::Paint);
    let first = last(&s);
    s.after(0.0, Step::LastHtml);
    s.after(0.0, Step::Inspect);
    let calm = last(&s);
    s.after(30.0, Step::Tick);
    s.after(0.0, Step::Paint);
    let later = last(&s);
    s.message(0.0, r#"{"type":"listen","types":["animationiteration"]}"#);
    s.ops(0.0, r#"[{"op":"style","id":41,"name":"opacity","value":"1"}]"#);
    s.ops(0.0, r#"[{"op":"style","id":41,"name":"opacity","value":"0"}]"#);
    s.after(0.0, Step::Paint);
    let faded = last(&s);
    s.after(0.0, Step::Inspect);
    let woken = last(&s);
    let mut cover = Vec::new();
    for k in 0..4 {
        cover.push(format!(r#"{{"op":"el","id":{},"tag":"div","attrs":[["style","position:absolute;left:{}px;top:0;width:30px;height:30px;background:#000"]]}},{{"op":"ins","parent":4,"id":{},"before":null}}"#, 42 + k * 10, k * 8, 42 + k * 10));
    }
    s.ops(0.0, &format!("[{}]", cover.join(",")));
    s.after(0.0, Step::Paint);
    let covered = last(&s);
    s.after(0.0, Step::Inspect);
    let ticking = last(&s);
    s.ops(0.0, r#"[{"op":"rm","id":42},{"op":"rm","id":52},{"op":"rm","id":62},{"op":"rm","id":72},{"op":"el","id":43,"tag":"div","attrs":[["id","w"]]},{"op":"ins","parent":4,"id":43,"before":null}]"#);
    s.after(0.0, Step::Paint);
    let grown = last(&s);
    s.after(0.0, Step::LastHtml);
    s.after(0.0, Step::Compositing(false));
    s.after(0.0, Step::Paint);
    let flat = last(&s);
    s.after(0.0, Step::LastHtml);
    let Some(run) = replay(s) else { return };

    let p = painted(&run, first);
    let spin = (p.composition.layers.len() == 1 && p.composition.layers[0].id == 40).then(|| &p.composition.layers[0]);
    assert!(spin.is_some(), "the endless rotation becomes the one layer ({} layers)", p.composition.layers.len());
    assert!(!red(&p.list), "the document's paint leaves the layer's element out");
    let spin = spin.expect("the layer");
    assert!(red(&spin.content), "the layer's content paints the element");
    assert!(
        spin.track.repeat && (spin.track.duration - 1.0).abs() < 1e-6 && spin.track.keys.len() >= 3,
        "one second, looping, {} keys",
        spin.track.keys.len()
    );
    let (x, y) = track_at(&spin.track, 0.25, 20.0, 20.0);
    assert!((x - 60.0).abs() < 0.2 && (y - 20.0).abs() < 0.2, "a quarter of the track is a quarter turn about the centre ({x}, {y})");
    assert!(spin.bounds.x <= 20.0 && spin.bounds.y <= 20.0 && spin.bounds.x + spin.bounds.w >= 60.0, "the layer's bounds hold its box");
    assert!(!spin.has_clip && spin.raster_scale == 1.0, "no clip, no extra raster scale");
    let (resumed, span) = twidget_compositor::resume_track(&spin.track, 0.25 + 3.0);
    assert!(
        span == 1.0
            && resumed[0].t == 0.0
            && resumed[resumed.len() - 1].t == 1.0
            && (resumed[0].matrix.apply_x(20.0, 20.0) - 60.0).abs() < 0.2
            && resumed[resumed.len() - 1].matrix == resumed[0].matrix,
        "resumed 3.25 s in, the loop starts and ends at the quarter turn"
    );
    let mut once = spin.track.clone();
    once.repeat = false;
    let (rest, span) = twidget_compositor::resume_track(&once, 0.5);
    assert!(
        (span - 0.5).abs() < 1e-9 && rest[0].t == 0.0 && (rest[0].matrix.apply_x(20.0, 20.0) - 60.0).abs() < 0.2,
        "a one-shot resumed half way keeps its second half"
    );
    assert!(
        !element(&run.html[0], 40).contains("transform"),
        "the document gets no per-frame transform for a layer ({})",
        element(&run.html[0], 40)
    );
    let (wants, next) = inspected(&run.extras[calm]);
    assert!(!wants, "a layer alone needs no frames");
    assert!(next.is_infinite(), "nor a wake-up, with no animationiteration listener");
    assert_eq!(painted(&run, later).composition.version, p.composition.version, "a frame later the composition is the same");

    let fade = layer(&painted(&run, faded).composition, 41);
    assert!(fade.is_some(), "the opacity transition becomes a layer too");
    let keys = &fade.expect("the fade").track.keys;
    let fade = fade.expect("the fade");
    assert!(
        !fade.track.repeat && (fade.track.duration - 0.4).abs() < 0.05 && keys[0].opacity > 0.9 && keys[keys.len() - 1].opacity < 0.1,
        "it plays once, 1 to 0, over what is left of 400 ms"
    );
    let (_, wake) = inspected(&run.extras[woken]);
    assert!(wake > 0.2 && wake < 1.01, "the core wakes for the next animationiteration or the transition's end ({wake} s)");
    assert!(layer(&painted(&run, covered).composition, 40).is_none(), "content painted over the sweep keeps the element in the document");
    assert!(inspected(&run.extras[ticking]).0, "which then needs frames again");
    assert!(
        layer(&painted(&run, grown).composition, 43).is_none() && String::from_utf8_lossy(&run.html[1]).contains("width:"),
        "a width animation stays in the document"
    );
    assert!(
        painted(&run, flat).composition.layers.is_empty() && element(&run.html[2], 40).contains("transform:rotate("),
        "compositing off: no layers, the transform is sampled again"
    );
}

#[test]
fn a_finished_animation_inside_an_element_does_not_keep_it_out_of_layers() {
    let mut s = Script::new(90);
    s.width = 360.0;
    s.after(0.0, Step::Compositing(true));
    s.ops(0.0, &format!(r#"[{ROOTS},
     {{"op":"el","id":20,"tag":"style","attrs":[]}},
     {{"op":"css","id":20,"text":".card{{margin:30px;height:60px}} .like{{width:20px;height:20px}} .pop{{animation:pop 100ms}} .slide{{animation:slide 1s}} @keyframes pop{{from{{transform:scale(1.7)}}}} @keyframes slide{{from{{opacity:0;transform:translateX(28px)}}}}"}},
     {{"op":"ins","parent":3,"id":20,"before":null}},
     {{"op":"el","id":80,"tag":"div","attrs":[["class","card"]]}},
     {{"op":"el","id":81,"tag":"div","attrs":[["class","like pop"]]}},
     {{"op":"ins","parent":4,"id":80,"before":null}},
     {{"op":"ins","parent":80,"id":81,"before":null}}]"#));
    s.after(150.0, Step::Tick);
    s.ops(0.0, r#"[{"op":"attr","id":80,"name":"class","value":"card slide"}]"#);
    s.after(0.0, Step::Paint);
    let p = last(&s);
    let Some(run) = replay(s) else { return };
    let c = &painted(&run, p).composition;
    assert!(
        c.layers.len() == 1 && c.layers[0].id == 80,
        "a pop that has ended inside the card does not keep the card's slide out of layers"
    );
}

const SEEK_PAGE: &str = r#"
 {"op":"el","id":20,"tag":"style","attrs":[]},
 {"op":"css","id":20,"text":".card{padding:12px 14px;border:1px solid #ccc;animation:slide 1s linear infinite} .art{height:64px} .seek{position:relative;height:14px} .prog{position:absolute;left:0;top:5px;width:300px;height:4px;background-color:#7f7f7f} .times{margin-top:2px} @keyframes slide{from{opacity:0;transform:translateX(28px)}}"},
 {"op":"ins","parent":3,"id":20,"before":null},
 {"op":"el","id":70,"tag":"div","attrs":[["class","card"]]},
 {"op":"el","id":71,"tag":"div","attrs":[["class","art"]]},
 {"op":"el","id":72,"tag":"div","attrs":[["class","seek"]]},
 {"op":"el","id":73,"tag":"div","attrs":[["class","prog"]]},
 {"op":"el","id":74,"tag":"div","attrs":[["class","times"]]},
 {"op":"text","id":75,"data":"0:00"},
 {"op":"ins","parent":4,"id":70,"before":null},
 {"op":"ins","parent":70,"id":71,"before":null},
 {"op":"ins","parent":70,"id":72,"before":null},
 {"op":"ins","parent":72,"id":73,"before":null},
 {"op":"ins","parent":70,"id":74,"before":null},
 {"op":"ins","parent":74,"id":75,"before":null}"#;

#[test]
fn a_layer_paints_positioned_descendants_where_the_document_does() {
    let bar = |list: &DisplayList| list.iter().find(|c| c.kind == Cmd::FillRect && c.color.r == 0x7f && c.color.g == 0x7f).map(|c| c.rect);
    let page = |card: &str| format!("[{ROOTS},{}]", SEEK_PAGE.replace("animation:slide 1s linear infinite", card));
    let mut flat = Script::new(80);
    flat.width = 360.0;
    flat.ops(0.0, &page("animation:slide 1s linear infinite"));
    flat.after(0.0, Step::Paint);
    let mut layered = flat.clone();
    layered.steps.insert(0, (layered.t0, Step::Compositing(true)));
    let mut scripts = vec![flat, layered];
    let groups = ["opacity:0.5", "transform:translateX(28px)", "opacity:0.5;transform:translateX(28px)"];
    for group in groups {
        let mut s = Script::new(80);
        s.width = 360.0;
        s.ops(0.0, &page(group));
        s.after(0.0, Step::Paint);
        scripts.push(s);
    }
    let Some(runs) = replay_all(scripts) else { return };
    let expected = bar(&runs[0].paints[0].list);
    let c = &runs[1].paints[0].composition;
    let got = (c.layers.len() == 1).then(|| bar(&c.layers[0].content)).flatten();
    assert!(expected.is_some() && got.is_some(), "the card is a layer and its seek bar is in the layer's content");
    let (expected, got) = (expected.unwrap_or_default(), got.unwrap_or_default());
    assert!(
        got.x == expected.x && got.y == 82.0 && expected.y == 82.0,
        "the layer and the document both draw the absolutely positioned bar at 82 (layer {}, document {})",
        got.y,
        expected.y
    );
    for (group, run) in groups.iter().zip(&runs[2..]) {
        let list = &run.paints[0].list;
        let mut depth = 0;
        let mut inside = -1;
        for c in list {
            match c.kind {
                Cmd::GroupPush => depth += 1,
                Cmd::GroupPop => depth -= 1,
                Cmd::FillRect if c.color.r == 0x7f && c.color.g == 0x7f => inside = depth,
                _ => {}
            }
        }
        assert_eq!(inside, 1, "a card with a static {group} paints the bar inside its group");
        assert!(bar(list).is_some_and(|r| r.y == 82.0), "a card with a static {group} draws the bar at 82");
    }
}

#[test]
fn clip_path_and_fixed_elements_under_an_overflow_clip() {
    let mut s = Script::new(90);
    s.width = 360.0;
    s.ops(0.0, &format!(r#"[{ROOTS},
     {{"op":"el","id":20,"tag":"style","attrs":[]}},
     {{"op":"css","id":20,"text":".box{{width:200px;height:200px;overflow:hidden}} .round{{width:100px;height:100px;clip-path:circle(50%);background-color:#ff0000}} .wide{{width:300px;height:20px;background-color:#00ff00}} .fixed{{position:fixed;left:0;top:0;width:300px;height:10px;background-color:#0000ff}}"}},
     {{"op":"ins","parent":3,"id":20,"before":null}},
     {{"op":"el","id":80,"tag":"div","attrs":[["class","box"]]}},
     {{"op":"el","id":81,"tag":"div","attrs":[["class","round"]]}},
     {{"op":"el","id":82,"tag":"div","attrs":[["class","wide"]]}},
     {{"op":"el","id":83,"tag":"div","attrs":[["class","fixed"]]}},
     {{"op":"ins","parent":4,"id":80,"before":null}},
     {{"op":"ins","parent":80,"id":81,"before":null}},
     {{"op":"ins","parent":80,"id":82,"before":null}},
     {{"op":"ins","parent":80,"id":83,"before":null}}]"#));
    s.after(0.0, Step::Paint);
    let p = last(&s);
    s.query(0.0, 9, r#"{"q":"elementFromPoint","x":50,"y":50}"#);
    let inside = last(&s);
    s.query(0.0, 9, r#"{"q":"elementFromPoint","x":4,"y":14}"#);
    let corner = last(&s);
    let Some(run) = replay(s) else { return };
    let (mut depth, mut low, mut wide, mut fixed, mut paths) = (0, 0, -1, -1, 0);
    for c in &painted(&run, p).list {
        match c.kind {
            Cmd::ClipPush => {
                depth += 1;
                if c.path.is_some() {
                    paths += 1;
                }
            }
            Cmd::ClipPop => {
                depth -= 1;
                low = low.min(depth);
            }
            Cmd::FillRect if c.color.g == 255 => wide = depth,
            Cmd::FillRect if c.color.b == 255 && c.color.r == 0 => fixed = depth,
            _ => {}
        }
    }
    assert_eq!(paths, 1, "clip-path pushes a path clip");
    assert!(depth == 0 && low == 0, "clip pushes and pops balance, so clip-path does not pop its parent's clip");
    assert!(wide >= 1, "the element after the clip-path one is still clipped by the overflow box (depth {wide})");
    assert!(fixed >= 0 && fixed < wide, "a fixed element escapes its ancestor's overflow clip (depth {fixed})");
    let at = |step: usize| result(&run.outs[step]).as_number(0.0);
    assert!(at(inside) == 81.0 && at(corner) != 81.0, "hits inside the circle reach it, the corner outside does not");
}

#[test]
fn damage() {
    let mut s = Script::new(20);
    s.ops(0.0, r#"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
        {"op":"el","id":5,"tag":"div","attrs":[["style","position:absolute;left:10px;top:10px;width:20px;height:20px;background:red"]]},{"op":"ins","parent":4,"id":5,"before":null},
        {"op":"el","id":6,"tag":"div","attrs":[["style","position:absolute;left:100px;top:10px;width:20px;height:20px;background:blue"]]},{"op":"ins","parent":4,"id":6,"before":null},
        {"op":"el","id":7,"tag":"div","attrs":[["style","position:absolute;left:105px;top:15px;width:20px;height:20px;background:green"]]},{"op":"ins","parent":4,"id":7,"before":null},
        {"op":"el","id":8,"tag":"canvas","attrs":[["width","10"],["height","10"],["style","position:absolute;left:200px;top:10px"]]},{"op":"ins","parent":4,"id":8,"before":null}]"#);
    let mut paints = Vec::new();
    let mut paint = |s: &mut Script| {
        s.after(0.0, Step::Paint);
        paints.push(last(s));
    };
    paint(&mut s);
    paint(&mut s);
    s.ops(0.0, r#"[{"op":"style","id":5,"name":"background-color","value":"yellow"}]"#);
    paint(&mut s);
    s.ops(0.0, r#"[{"op":"canvas","id":8,"cmds":[["c","fillRect",0,0,5,5]]}]"#);
    paint(&mut s);
    s.ops(0.0, r#"[{"op":"style","id":6,"name":"z-index","value":"2"},{"op":"style","id":7,"name":"z-index","value":"1"}]"#);
    paint(&mut s);
    paint(&mut s);
    s.after(0.0, Step::Trim);
    paint(&mut s);
    let Some(run) = replay(s) else { return };
    let damage = |i: usize| painted(&run, paints[i]).damage.clone();
    let covers = |rects: &[Rect], x: f32, y: f32, w: f32, h: f32| {
        rects.iter().any(|r| r.x <= x && r.y <= y && r.x + r.w >= x + w && r.y + r.h >= y + h)
    };
    let within = |rects: &[Rect], x: f32, y: f32, w: f32, h: f32| {
        rects.iter().all(|r| r.x >= x && r.y >= y && r.x + r.w <= x + w && r.y + r.h <= y + h)
    };
    assert!(damage(0).is_none(), "the first paint is all damage");
    assert!(damage(1).is_some_and(|d| d.is_empty()), "an unchanged paint damages nothing");
    assert!(
        damage(2).is_some_and(|d| covers(&d, 10.0, 10.0, 20.0, 20.0) && within(&d, 5.0, 5.0, 30.0, 30.0)),
        "a new colour damages its box and nothing else"
    );
    assert!(
        damage(3).is_some_and(|d| covers(&d, 200.0, 10.0, 10.0, 10.0) && within(&d, 195.0, 5.0, 20.0, 20.0)),
        "a canvas that drew damages itself"
    );
    assert!(damage(4).is_some_and(|d| covers(&d, 105.0, 15.0, 15.0, 15.0)), "boxes that swap paint order damage where they overlap");
    assert!(
        damage(6).is_none() && wire(&painted(&run, paints[6]).list) == wire(&painted(&run, paints[5]).list),
        "after a trim the next paint is whole, and the same"
    );
}

/// The layers of a composited document of a spinner sheet and `body`'s ops, after a paint.
fn layers_of(body: &str) -> Script {
    let mut s = Script::new(420);
    s.after(0.0, Step::Compositing(true));
    s.ops(0.0, &format!(r#"[{{"op":"el","id":2,"tag":"html","adopt":"html"}},{{"op":"el","id":3,"tag":"head","adopt":"head"}},
        {{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]}},{{"op":"el","id":20,"tag":"style"}},
        {{"op":"css","id":20,"text":"@keyframes spin{{from{{transform:rotate(0deg)}}to{{transform:rotate(360deg)}}}} .s{{width:40px;height:40px;background:red;animation:spin 1s linear infinite}}"}},
        {{"op":"ins","parent":3,"id":20,"before":null}},{body}]"#));
    s.after(0.0, Step::Paint);
    s
}

#[test]
fn compositor_coverage() {
    let mut scripts = vec![
        layers_of(
            r#"{"op":"el","id":30,"tag":"div","attrs":[["style","overflow:hidden;width:300px;height:300px"]]},{"op":"ins","parent":4,"id":30,"before":null},
            {"op":"el","id":31,"tag":"div","attrs":[["style","overflow:hidden;margin:50px;width:100px;height:100px"]]},{"op":"ins","parent":30,"id":31,"before":null},
            {"op":"el","id":40,"tag":"div","attrs":[["class","s"],["style","margin:0"]]},{"op":"ins","parent":31,"id":40,"before":null}"#,
        ),
        layers_of(
            r#"{"op":"el","id":40,"tag":"div","attrs":[["class","s"],["style","margin:20px"]]},{"op":"ins","parent":4,"id":40,"before":null},
            {"op":"el","id":41,"tag":"div","attrs":[["style","height:150px"]]},{"op":"ins","parent":4,"id":41,"before":null},
            {"op":"el","id":42,"tag":"div","attrs":[["style","overflow:hidden;height:10px"]]},{"op":"ins","parent":4,"id":42,"before":null},
            {"op":"el","id":43,"tag":"div","attrs":[["style","position:relative;top:-200px;height:60px;background:blue"]]},{"op":"ins","parent":42,"id":43,"before":null}"#,
        ),
        layers_of(
            r#"{"op":"el","id":21,"tag":"style"},{"op":"css","id":21,"text":"@keyframes glow{from{background-color:#000}to{background-color:#fff}} .g{padding:20px;animation:glow 1s infinite}"},
            {"op":"ins","parent":3,"id":21,"before":null},
            {"op":"el","id":30,"tag":"div","attrs":[["class","g"]]},{"op":"ins","parent":4,"id":30,"before":null},
            {"op":"el","id":40,"tag":"div","attrs":[["class","s"]]},{"op":"ins","parent":30,"id":40,"before":null}"#,
        ),
        layers_of(
            r#"{"op":"el","id":40,"tag":"div","attrs":[["class","s"],["style","margin:20px"]]},{"op":"ins","parent":4,"id":40,"before":null},
            {"op":"el","id":41,"tag":"div","attrs":[["style","position:absolute;left:25px;top:30px;width:30px;height:12px;background:white"]]},{"op":"ins","parent":4,"id":41,"before":null}"#,
        ),
        layers_of(
            r#"{"op":"el","id":40,"tag":"div","attrs":[["class","s"],["style","margin:20px"]]},{"op":"ins","parent":4,"id":40,"before":null},
            {"op":"el","id":41,"tag":"div","attrs":[["style","position:absolute;left:25px;top:30px;width:30px;height:12px;background:white;opacity:.5;transform:translateX(4px)"]]},{"op":"ins","parent":4,"id":41,"before":null}"#,
        ),
    ];
    let mut many = Vec::new();
    for i in 0..300 {
        let id = 100 + i;
        many.push(format!(r#"{{"op":"el","id":{id},"tag":"div","attrs":[["class","s"],["style","display:inline-block;width:10px;height:10px;margin:10px"]]}},{{"op":"ins","parent":4,"id":{id},"before":null}}"#));
    }
    scripts.push(layers_of(&many.join(",")));
    // A layer keeps its content while what changes is elsewhere, and records it again when it is inside.
    let mut k = Script::new(60);
    k.after(0.0, Step::Compositing(true));
    k.ops(0.0, r#"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},{"op":"el","id":20,"tag":"style"},
        {"op":"css","id":20,"text":"@keyframes spin{to{transform:rotate(360deg)}} .s{width:40px;height:40px;animation:spin 1s linear infinite}"},{"op":"ins","parent":3,"id":20,"before":null},
        {"op":"el","id":40,"tag":"div","attrs":[["class","s"]]},{"op":"ins","parent":4,"id":40,"before":null},
        {"op":"el","id":41,"tag":"span","attrs":[["style","color:red"]]},{"op":"ins","parent":40,"id":41,"before":null},{"op":"text","id":42,"data":"x"},{"op":"ins","parent":41,"id":42,"before":null},
        {"op":"el","id":50,"tag":"div","attrs":[["style","height:10px;background:blue"]]},{"op":"ins","parent":4,"id":50,"before":null}]"#);
    k.after(0.0, Step::Paint);
    k.ops(0.0, r#"[{"op":"style","id":50,"name":"background-color","value":"green"}]"#);
    k.after(0.0, Step::Paint);
    k.ops(0.0, r#"[{"op":"style","id":41,"name":"color","value":"blue"}]"#);
    k.after(0.0, Step::Paint);
    scripts.push(k);
    // A layer is hit where its track shows it: half way through a 200 px slide, 100 px along.
    let mut h = Script::new(60);
    h.after(0.0, Step::Compositing(true));
    h.ops(0.0, r#"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},{"op":"el","id":20,"tag":"style"},
        {"op":"css","id":20,"text":"@keyframes slide{from{transform:translateX(0)}to{transform:translateX(200px)}} .m{width:40px;height:40px;background:red;animation:slide 1s linear infinite}"},
        {"op":"ins","parent":3,"id":20,"before":null},
        {"op":"el","id":40,"tag":"div","attrs":[["class","m"]]},{"op":"ins","parent":4,"id":40,"before":null}]"#);
    h.after(0.0, Step::Paint);
    h.query(500.0, 1, r#"{"q":"elementFromPoint","x":120,"y":20}"#);
    scripts.push(h);
    let Some(runs) = replay_all(scripts) else { return };
    let layers = |i: usize| &runs[i].paints[0].composition.layers;
    let nested = layers(0);
    assert!(
        nested.len() == 1 && nested[0].has_clip && nested[0].clip.x == 50.0 && nested[0].clip.w == 100.0,
        "a layer inside two clips takes the inner one ({} layers)",
        nested.len()
    );
    let clipped = layers(1);
    assert!(
        clipped.len() == 1 && clipped[0].id == 40,
        "a box clipped away by its own parent does not cover a layer ({} layers)",
        clipped.len()
    );
    let glowing = layers(2);
    assert!(
        glowing.len() == 1 && glowing[0].id == 40,
        "a parent's background animation leaves its child a layer ({} layers)",
        glowing.len()
    );
    let labelled = layers(3);
    let spinner = labelled.iter().any(|l| l.id == 40 && l.track.keys.len() > 2);
    let label = labelled.iter().any(|l| l.id == 41 && l.track.keys.len() == 1);
    assert!(
        labelled.len() == 2 && spinner && label && labelled[1].id == 41,
        "a label over a spinner becomes a still layer above it ({} layers)",
        labelled.len()
    );
    let turned = layers(4);
    let kept = turned.len() == 2
        && turned[1].id == 41
        && turned[1].track.keys.len() == 1
        && (turned[1].track.keys[0].opacity - 0.5).abs() < 1e-3
        && (turned[1].track.keys[0].matrix.e - 4.0).abs() < 1e-3;
    assert!(kept, "a still layer keeps its element's own transform and opacity");
    let capped = layers(5);
    assert!(
        capped.len() == 256 && capped.iter().any(|l| l.id == 399) && !capped.iter().any(|l| l.id == 100),
        "past the cap, the topmost layers win ({} layers)",
        capped.len()
    );
    let version = |p: &Painted| p.composition.layers.first().map_or(0, |l| l.content_version);
    let first = version(&runs[6].paints[0]);
    assert!(first != 0 && version(&runs[6].paints[1]) == first, "a layer keeps its content when something outside it changes");
    assert!(version(&runs[6].paints[2]) != first, "and records it again when something inside it changes");
    let hit = result(&runs[7].outs[3]);
    assert!(
        runs[7].paints[0].composition.layers.len() == 1 && hit.as_number(0.0) == 40.0,
        "a moving layer is hit where it shows ({})",
        String::from_utf8_lossy(&hit.dump())
    );
}

#[test]
fn scripted_layers() {
    let mut s = Script::new(20);
    s.width = 400.0;
    s.after(0.0, Step::Compositing(true));
    s.ops(0.0, r#"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
        {"op":"el","id":10,"tag":"div","attrs":[["style","width:40px;height:40px;background-color:#f00"]]},{"op":"ins","parent":4,"id":10,"before":null},
        {"op":"el","id":11,"tag":"div","attrs":[["style","margin-top:200px;width:300px;height:100px;background-color:#00f"]]},{"op":"ins","parent":4,"id":11,"before":null}]"#);
    s.after(0.0, Step::Paint);
    let mv = |s: &mut Script, x: i32| {
        s.ops(0.0, &format!(r#"[{{"op":"style","id":10,"name":"transform","value":"translateX({x}px)"}}]"#));
        last(s)
    };
    mv(&mut s, 1);
    s.after(0.0, Step::Paint);
    mv(&mut s, 2);
    s.after(0.0, Step::Paint);
    let two = last(&s);
    mv(&mut s, 3);
    s.after(0.0, Step::Paint);
    let three = last(&s);
    let four = mv(&mut s, 4);
    s.after(0.0, Step::Paint);
    let reused = last(&s);
    let far = mv(&mut s, 200);
    s.after(0.0, Step::Paint);
    let replanned = last(&s);
    let settled = mv(&mut s, 201);
    s.query(0.0, 1, r#"{"q":"rect","id":10}"#);
    let asked = last(&s);
    s.after(0.0, Step::LastHtml);
    let Some(run) = replay(s) else { return };
    let layer_x = |c: &Composition| {
        if c.layers.len() == 1 && c.layers[0].id == 10 && c.layers[0].track.keys.len() == 1 {
            c.layers[0].track.keys[0].matrix.e
        } else {
            -1.0
        }
    };
    assert!(painted(&run, two).composition.layers.is_empty(), "two moves are not a streak yet");
    assert_eq!(layer_x(&painted(&run, three).composition), 3.0, "a third makes the element a layer, where the script put it");
    let (builds, reused_before) = (run.stats[three].builds, run.stats[three].reused);
    assert!(run.stats[four].scripted == 1 && run.stats[four].builds == builds, "the next move goes to the layer without a build");
    assert!(run.dirty[four] && layer_x(&run.compositions[four]) == 4.0, "its track holds the new transform");
    let p = painted(&run, reused);
    assert!(
        run.stats[reused].reused == reused_before + 1 && p.damage.as_ref().is_some_and(|d| d.is_empty()),
        "the paint hands out the last list: the document did not change"
    );
    assert!(run.stats[far].builds > builds, "a move past the room the layer has builds");
    assert_eq!(layer_x(&painted(&run, replanned).composition), 200.0, "and plans the layer again where it went");
    assert!(run.stats[asked].builds > run.stats[settled].builds, "a query first builds what only the layer showed");
    assert!(String::from_utf8_lossy(&run.html[0]).contains("translateX(201px)"), "the document has it");
}

#[test]
fn a_crowd_moved_by_script_stays_in_the_document() {
    let mut s = Script::new(400);
    s.width = 400.0;
    s.after(0.0, Step::Compositing(true));
    let mut ops = vec![r#"{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]}"#.to_string()];
    for i in 0..260 {
        let id = 100 + i;
        ops.push(format!(r#"{{"op":"el","id":{id},"tag":"div","attrs":[["style","display:inline-block;width:4px;height:4px"]]}},{{"op":"ins","parent":4,"id":{id},"before":null}}"#));
    }
    s.ops(0.0, &format!("[{}]", ops.join(",")));
    for f in 0..4 {
        let frame: Vec<String> =
            (0..260).map(|i| format!(r#"{{"op":"style","id":{},"name":"transform","value":"translateY({f}px)"}}"#, 100 + i)).collect();
        s.ops(0.0, &format!("[{}]", frame.join(",")));
        s.after(0.0, Step::Paint);
    }
    let end = last(&s);
    let Some(run) = replay(s) else { return };
    assert!(
        painted(&run, end).composition.layers.is_empty() && run.stats[end].scripted == 0,
        "more than the layer budget moving together: no scripted layers"
    );
}

#[test]
fn shimmer_layers() {
    let mut s = Script::new(40);
    s.width = 400.0;
    s.after(0.0, Step::Compositing(true));
    s.ops(0.0, r#"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
        {"op":"el","id":20,"tag":"style"},{"op":"css","id":20,"text":".sk{margin:10px;width:200px;height:20px;border-radius:6px;background-color:#ddd;background-image:linear-gradient(90deg,#ddd,#fff,#ddd);background-size:50% 100%;background-repeat:no-repeat;animation:shimmer 1s linear infinite} @keyframes shimmer{from{background-position:-100% 0}to{background-position:200% 0}}"},
        {"op":"ins","parent":3,"id":20,"before":null},
        {"op":"el","id":30,"tag":"div","attrs":[["class","sk"]]},{"op":"ins","parent":4,"id":30,"before":null}]"#);
    s.after(0.0, Step::Paint);
    let p = last(&s);
    s.after(0.0, Step::Inspect);
    let Some(run) = replay(s) else { return };
    let p = painted(&run, p);
    let layers = &p.composition.layers;
    assert!(layers.len() == 1 && layers[0].id == 30, "a background-position animation of a lone gradient tile is a layer");
    let l = &layers[0];
    let gradient = |d: &DisplayList| d.iter().filter(|c| c.kind == Cmd::Linear).count();
    assert!(gradient(&l.content) == 1 && gradient(&p.list) == 0, "the layer holds the tile, the document the rest of the element");
    assert!(
        l.bounds.w == 100.0 && l.bounds.h == 20.0 && l.has_clip && l.clip.x == 10.0 && l.clip.w == 200.0 && l.clip_radii.tlx == 6.0,
        "the tile, under the element's rounded box"
    );
    assert!(l.track.repeat && l.track.keys.len() >= 2, "sliding forever");
    let (first, end) = (l.track.keys[0].matrix.e, l.track.keys[l.track.keys.len() - 1].matrix.e);
    assert!(
        (l.bounds.x + first - (10.0 - 100.0)).abs() < 0.5 && (l.bounds.x + end - (10.0 + 200.0)).abs() < 0.5,
        "from -100% to 200% of the room (got {} to {})",
        l.bounds.x + first,
        l.bounds.x + end
    );
    assert!(!inspected(&run.extras[run.extras.len() - 1]).0, "and the document needs no frames for it");
}

#[test]
fn svg_layers() {
    let mut s = Script::new(60);
    s.width = 400.0;
    s.after(0.0, Step::Compositing(true));
    s.ops(0.0, r##"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
        {"op":"el","id":40,"tag":"svg","attrs":[["width","60"],["height","60"]]},{"op":"ins","parent":4,"id":40,"before":null},
        {"op":"el","id":41,"tag":"circle","attrs":[["cx","30"],["cy","30"],["r","10"],["fill","#f00"]]},{"op":"ins","parent":40,"id":41,"before":null},
        {"op":"el","id":50,"tag":"div","attrs":[["style","margin-top:100px;height:20px;background-color:#00f"]]},{"op":"ins","parent":4,"id":50,"before":null}]"##);
    s.after(0.0, Step::Paint);
    let mut grown = Vec::new();
    for r in [11, 12, 14] {
        s.ops(0.0, &format!(r#"[{{"op":"attr","id":41,"name":"r","value":"{r}"}}]"#));
        s.after(0.0, Step::Paint);
        grown.push(last(&s));
    }
    let Some(run) = replay(s) else { return };
    assert!(painted(&run, grown[0]).composition.layers.is_empty(), "two changes are not a streak yet");
    let c = &painted(&run, grown[1]).composition;
    assert!(c.layers.len() == 1 && c.layers[0].id == 40, "a third makes the SVG a layer");
    let content = c.layers[0].content_version;
    let p = painted(&run, grown[2]);
    assert!(p.damage.as_ref().is_some_and(|d| d.is_empty()), "its next change leaves the document as it was");
    assert!(p.composition.layers.len() == 1 && p.composition.layers[0].content_version != content, "and repaints the layer");
}

#[test]
fn canvas_layers() {
    let mut s = Script::new(70);
    s.width = 400.0;
    s.after(0.0, Step::Compositing(true));
    s.ops(0.0, r##"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
        {"op":"el","id":60,"tag":"canvas","attrs":[["width","100"],["height","50"]]},{"op":"ins","parent":4,"id":60,"before":null},
        {"op":"el","id":61,"tag":"div","attrs":[["style","height:20px;background-color:#00f"]]},{"op":"ins","parent":4,"id":61,"before":null}]"##);
    s.after(0.0, Step::Paint);
    let mut drawn = Vec::new();
    for x in [0, 10, 20, 30] {
        s.ops(0.0, &format!(r##"[{{"op":"canvas","id":60,"cmds":[["s","fillStyle","#f00"],["c","fillRect",{x},0,10,10]]}}]"##));
        s.after(0.0, Step::Paint);
        drawn.push(last(&s));
    }
    let Some(run) = replay(s) else { return };
    assert!(painted(&run, drawn[1]).composition.layers.is_empty(), "a canvas drawn twice is in the document");
    let c = &painted(&run, drawn[2]).composition;
    assert!(c.layers.len() == 1 && c.layers[0].id == 60, "drawn a third time, it is a layer");
    let content = c.layers[0].content_version;
    let (builds, reused) = (run.stats[drawn[2]].builds, run.stats[drawn[2]].reused);
    let p = painted(&run, drawn[3]);
    let in_document = p.list.iter().any(|c| c.canvas != 0);
    assert!(
        !in_document
            && run.stats[drawn[3]].reused == reused + 1
            && run.stats[drawn[3]].builds == builds
            && p.damage.as_ref().is_some_and(|d| d.is_empty()),
        "its next drawing leaves the document's list as it was"
    );
    assert!(
        p.composition.layers.len() == 1 && p.composition.layers[0].content_version != content,
        "and gives the layer a new content version"
    );
}

#[test]
fn web_animations_on_the_compositor() {
    let mut s = Script::new(80);
    s.width = 400.0;
    s.after(0.0, Step::Compositing(true));
    s.ops(0.0, r##"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
        {"op":"el","id":70,"tag":"div","attrs":[["style","width:40px;height:40px;background-color:#f00"]]},{"op":"ins","parent":4,"id":70,"before":null}]"##);
    s.message(0.0, r#"{"type":"anim","now":0,"anims":[{"aid":1,"target":70,"keyframes":[{"offset":0,"values":{"transform":"translateX(0px)"}},{"offset":1,"values":{"transform":"translateX(100px)"}}],"timing":{"duration":1000,"iterations":null,"easing":"linear"},"startTime":0}]}"#);
    s.after(0.0, Step::Paint);
    let p = last(&s);
    s.after(0.0, Step::Inspect);
    let Some(run) = replay(s) else { return };
    let layers = &painted(&run, p).composition.layers;
    assert!(layers.len() == 1 && layers[0].id == 70, "a Web Animation of transform is a layer");
    let t = &layers[0].track;
    let rising = t.keys.windows(2).all(|w| w[0].matrix.e <= w[1].matrix.e);
    assert!(
        t.repeat
            && (t.duration - 1.0).abs() < 1e-6
            && rising
            && t.keys[0].matrix.e.abs() < 0.01
            && (t.keys[t.keys.len() - 1].matrix.e - 100.0).abs() < 0.05,
        "its loop is baked from 0 to 100 px ({} keys)",
        t.keys.len()
    );
    assert!(!inspected(&run.extras[run.extras.len() - 1]).0, "and the document needs no frames for it");
}

#[test]
fn nested_layers() {
    let mut s = Script::new(40);
    s.width = 400.0;
    s.after(0.0, Step::Compositing(true));
    s.ops(0.0, r#"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
        {"op":"el","id":20,"tag":"style"},{"op":"css","id":20,"text":"@keyframes spin{to{transform:rotate(360deg)}} .orbit{margin:40px;width:100px;height:100px;animation:spin 4s linear infinite} .moon{width:20px;height:20px;background:red;animation:spin 1s linear infinite}"},
        {"op":"ins","parent":3,"id":20,"before":null},
        {"op":"el","id":30,"tag":"div","attrs":[["class","orbit"]]},{"op":"ins","parent":4,"id":30,"before":null},
        {"op":"el","id":31,"tag":"div","attrs":[["class","moon"]]},{"op":"ins","parent":30,"id":31,"before":null}]"#);
    s.after(0.0, Step::Paint);
    let p = last(&s);
    let Some(run) = replay(s) else { return };
    let c = &painted(&run, p).composition;
    let layers = &c.layers;
    let nested = layers.len() == 2 && layers[0].id == 30 && layers[0].parent == 0 && layers[1].id == 31 && layers[1].parent == 30;
    assert!(nested, "a spinner inside a spinner is a layer inside its layer ({} layers)", layers.len());
    assert!(!red(&layers[0].content) && red(&layers[1].content), "the parent's content leaves the child out");
    assert!(String::from_utf8_lossy(&twidget_encode::composition_json(c)).contains("\"p\":30"), "the JSON names the parent");
}

/// A body of boxes, one per style, and an optional style sheet (core_tests' heavy-box `page`).
fn heavy_page(styles: &[&str], css: &str, attrs: &str, tag: &str) -> String {
    let mut ops = String::from(
        r#"[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]}"#,
    );
    if !css.is_empty() {
        ops.push_str(&format!(
            r#",{{"op":"el","id":20,"tag":"style"}},{{"op":"css","id":20,"text":{}}},{{"op":"ins","parent":3,"id":20,"before":null}}"#,
            common::inputs::quote(css)
        ));
    }
    for (i, style) in styles.iter().enumerate() {
        let id = 30 + i;
        ops.push_str(&format!(
            r#",{{"op":"el","id":{id},"tag":"{tag}","attrs":[["style","width:10px;height:10px;{style}"]{attrs}]}},{{"op":"ins","parent":4,"id":{id},"before":null}}"#
        ));
    }
    ops.push(']');
    ops
}

/// The `limit` a step and the tick after it reported: "code count", or "".
fn limit_of(run: &Run, step: usize) -> String {
    let mut limit = String::new();
    for out in &run.outs[step..=step + 1] {
        for m in out {
            if m.get("type").as_str() == b"limit" {
                limit = format!("{} {}", String::from_utf8_lossy(m.get("code").as_str()), m.get("count").as_int(0));
            }
        }
    }
    limit
}

/// core_tests' `run`: the message, then a frame; the index of the message's step.
fn run_step(s: &mut Script, step: Step) -> usize {
    s.after(16.0, step);
    let at = last(s);
    s.after(16.0, Step::Tick);
    at
}

#[test]
fn heavy_boxes() {
    let shadows: Vec<&str> = vec!["box-shadow:0 2px 4px red"; 16];
    let mut many = shadows.clone();
    many.push("filter:brightness(1)");
    let mut waiting = shadows.clone();
    waiting.push("");
    let mut scripts = Vec::new();
    let mut at: Vec<Vec<usize>> = Vec::new();

    let mut h = Script::new(80);
    at.push(vec![run_step(&mut h, ops_message(&heavy_page(&shadows, "", "", "div")))]);
    scripts.push(h);
    let mut h17 = Script::new(80);
    at.push(vec![run_step(&mut h17, ops_message(&heavy_page(&many, "", "", "div")))]);
    scripts.push(h17);
    let mut c = Script::new(80);
    c.message(0.0, r#"{"type":"policy","policy":"compat"}"#);
    at.push(vec![run_step(&mut c, ops_message(&heavy_page(&many, "", "", "div")))]);
    scripts.push(c);
    let mut b = Script::new(80);
    let dimmed = vec!["opacity:0.5"; 20];
    let first = run_step(&mut b, ops_message(&heavy_page(&dimmed, "", r#",["disabled",""]"#, "button")));
    let more: Vec<String> = (0..17)
        .map(|i| {
            format!(
                r#"{{"op":"el","id":{},"tag":"div","attrs":[["style","opacity:0.5"]]}},{{"op":"ins","parent":4,"id":{},"before":null}}"#,
                60 + i,
                60 + i
            )
        })
        .collect();
    let second = run_step(&mut b, ops_message(&format!("[{}]", more.join(","))));
    at.push(vec![first, second]);
    scripts.push(b);
    let mut d = Script::new(80);
    at.push(vec![run_step(
        &mut d,
        ops_message(&heavy_page(
            &["backdrop-filter:blur(4px)", "backdrop-filter:blur(4px)", "-webkit-backdrop-filter:blur(4px)"],
            "",
            "",
            "div",
        )),
    )]);
    scripts.push(d);
    let fade = "@keyframes fade{from{opacity:1}to{opacity:1}} .late{animation:fade 1s linear 10s} .now{animation:fade 1s linear infinite}";
    let mut a = Script::new(80);
    at.push(vec![
        run_step(&mut a, ops_message(&heavy_page(&waiting, fade, "", "div"))),
        run_step(&mut a, ops_message(r#"[{"op":"attr","id":46,"name":"class","value":"late"}]"#)),
        run_step(&mut a, ops_message(r#"[{"op":"attr","id":46,"name":"class","value":"now"}]"#)),
    ]);
    scripts.push(a);
    let mut w = Script::new(80);
    at.push(vec![
        run_step(&mut w, ops_message(&heavy_page(&waiting, "", "", "div"))),
        run_step(&mut w, Step::Message(br#"{"type":"anim","now":0,"anims":[{"aid":1,"target":46,"keyframes":[{"offset":0,"values":{"opacity":"1"}},{"offset":1,"values":{"opacity":"1"}}],"timing":{"duration":1000,"fill":"forwards"},"startTime":-5000}]}"#.to_vec())),
        run_step(&mut w, Step::Message(br#"{"type":"anim","now":0,"anims":[{"aid":2,"target":46,"keyframes":[{"offset":0,"values":{"opacity":"1"}},{"offset":1,"values":{"opacity":"1"}}],"timing":{"duration":1000,"iterations":null},"startTime":0}]}"#.to_vec())),
    ]);
    scripts.push(w);
    let restyles = [
        ("@keyframes p{to{opacity:.55}} .a{animation:p 1.2s ease-in-out infinite alternate}", "a", "an animation with a to frame only"),
        (".p::before{content:\"\";display:block;width:6px;height:6px;filter:blur(1px)}", "p", "a ::before with a filter"),
        (".g{-webkit-backdrop-filter:blur(3px);backdrop-filter:blur(3px)}", "g", "a backdrop-filter"),
    ];
    for (css, class, _) in restyles {
        let all = format!(".h{{box-shadow:0 2px 6px rgba(0,0,0,.4)}} {css}");
        let mut x = Script::new(80);
        at.push(vec![
            run_step(&mut x, ops_message(&heavy_page(&shadows, &all, r#",["class","h"]"#, "div"))),
            run_step(&mut x, ops_message(&heavy_page(&waiting, &all, "", "div"))),
            run_step(&mut x, ops_message(&format!(r#"[{{"op":"attr","id":46,"name":"class","value":"{class}"}}]"#))),
        ]);
        scripts.push(x);
    }
    let mut heavy = shadows.clone();
    heavy[0] = "box-shadow:0 2px 4px red, 1px 1px 0 blue";
    heavy[1] = "filter:blur(2px)";
    heavy[2] = "backdrop-filter:blur(4px)";
    heavy[3] = "opacity:0.5";
    heavy[4] = "opacity:0";
    heavy.push("opacity:0.25");
    let mut st = Script::new(80);
    let stopped = run_step(&mut st, ops_message(&heavy_page(&heavy, "", "", "div")));
    st.after(0.0, Step::Paint);
    let p = last(&st);
    st.after(0.0, Step::Inspect);
    let quiet = last(&st);
    let again = run_step(&mut st, ops_message(&heavy_page(&heavy, "", "", "div")));
    at.push(vec![stopped, p, quiet, again]);
    scripts.push(st);

    let Some(runs) = replay_all(scripts) else { return };
    assert!(limit_of(&runs[0], at[0][0]).is_empty(), "16 boxes with a blurred shadow run");
    assert_eq!(limit_of(&runs[1], at[1][0]), "paint-limit 17", "a 17th heavy box reports the limit");
    assert!(limit_of(&runs[2], at[2][0]).is_empty(), "under compat nothing is counted");
    assert!(limit_of(&runs[3], at[3][0]).is_empty(), "a disabled button's own opacity is not heavy");
    assert_eq!(limit_of(&runs[3], at[3][1]), "paint-limit 17", "while a box's opacity is");
    assert_eq!(limit_of(&runs[4], at[4][0]), "backdrop-limit 3", "a third backdrop-filter reports its own limit");
    assert!(limit_of(&runs[5], at[5][0]).is_empty(), "a plain 17th box is not heavy");
    assert!(limit_of(&runs[5], at[5][1]).is_empty(), "nor while its animation waits out the delay");
    assert_eq!(limit_of(&runs[5], at[5][2]), "paint-limit 17", "but it is once the animation runs");
    assert!(limit_of(&runs[6], at[6][0]).is_empty(), "a page of 16 heavy boxes runs");
    assert!(limit_of(&runs[6], at[6][1]).is_empty(), "a finished Web Animation that holds its end is not counted");
    assert_eq!(limit_of(&runs[6], at[6][2]), "paint-limit 17", "a running one is");
    for (k, (_, _, what)) in restyles.iter().enumerate() {
        let (run, at) = (&runs[7 + k], &at[7 + k]);
        assert!(limit_of(run, at[0]).is_empty() && limit_of(run, at[1]).is_empty(), "{what}: runs before");
        assert!(!limit_of(run, at[2]).is_empty(), "{what}: stops");
    }
    let (run, at) = (&runs[10], &at[10]);
    assert_eq!(limit_of(run, at[0]), "paint-limit 17", "a mixed page stops");
    let (mut blurred, mut effects, mut translucent, mut hidden, mut sharp) = (0, 0, 0, 0, 0);
    for c in &painted(run, at[1]).list {
        match c.kind {
            Cmd::Shadow if c.bx > 0.0 => blurred += 1,
            Cmd::Shadow => sharp += 1,
            Cmd::FilterPush | Cmd::Backdrop => effects += 1,
            Cmd::GroupPush if c.hidden => hidden += 1,
            Cmd::GroupPush if c.opacity < 1.0 => translucent += 1,
            _ => {}
        }
    }
    assert!(
        blurred == 0 && sharp == 1 && effects == 0 && translucent == 0 && hidden == 1,
        "the stopped paint keeps sharp shadows only, opaque, the transparent box hidden (blurred {blurred}, sharp {sharp}, effects {effects}, translucent {translucent}, hidden {hidden})"
    );
    assert!(!inspected(&run.extras[at[2]]).0 && limit_of(run, at[3]).is_empty(), "and takes no more ops or frames");
}

// ---- This stage's comparisons ---------------------------------------------------------------------------------

#[test]
fn generated_layer_pages() {
    let Some(oracle) = oracle() else { return };
    let mut r = Lcg::new(0x4501);
    let scripts: Vec<(Script, String)> = (0..400).map(|i| (layer_script(&mut r), format!("layer script {i}"))).collect();
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("generated layer pages", scripts.len(), &tally);
    for kind in
        ["layer animated", "layer still", "layer scripted", "layer shimmer", "layer svg", "layer canvas", "layer nested", "layer clipped"]
    {
        assert!(tally.count(kind) > 0, "{kind} reached");
    }
}

#[test]
fn heavy_boxes_across_the_limits() {
    let Some(oracle) = oracle() else { return };
    let mut r = Lcg::new(0x4502);
    let scripts: Vec<(Script, String)> = (0..400).map(|i| (heavy_script(&mut r), format!("heavy script {i}"))).collect();
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("heavy boxes across the limits", scripts.len(), &tally);
    assert!(tally.count("message limit") >= 40, "the limits are crossed ({})", tally.count("message limit"));
    assert!(tally.count("message paint") >= 40, "paint reports go out ({})", tally.count("message paint"));
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
    assert!(scripts.len() >= 120, "the fixture's cases ({})", scripts.len());
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("paint-policy fixture as documents", scripts.len(), &tally);
}

#[test]
fn damage_between_paints() {
    let Some(oracle) = oracle() else { return };
    let mut r = Lcg::new(0x4503);
    let scripts: Vec<(Script, String)> = (0..200).map(|i| (damage_script(&mut r), format!("damage script {i}"))).collect();
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("damage between paints", scripts.len(), &tally);
    for kind in ["damage whole", "damage none", "damage one rect", "damage rects"] {
        assert!(tally.count(kind) > 0, "{kind} reached");
    }
}

#[test]
fn the_render_throttle() {
    let Some(oracle) = oracle() else { return };
    let mut r = Lcg::new(0x4504);
    let scripts: Vec<(Script, String)> = (0..150).map(|i| (throttle_script(&mut r), format!("throttle script {i}"))).collect();
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("the render throttle", scripts.len(), &tally);
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
    let mut tally = Tally::default();
    compare(&oracle, &scripts, &mut tally);
    report("corpus and stress widgets composited", scripts.len(), &tally);
}

#[test]
fn the_json_of_a_paint_policy_report() {
    // The `paint` and `limit` messages are JSON objects with their counts as numbers.
    let mut s = Script::new(80);
    s.message(0.0, r#"{"type":"policy","policy":"strict","paintReports":true}"#);
    let shadows = vec!["box-shadow:0 2px 4px red"; 17];
    s.ops(0.0, &heavy_page(&shadows, "", "", "div"));
    let ops = last(&s);
    let Some(run) = replay(s) else { return };
    let reports: Vec<&Json> = run.outs[ops].iter().filter(|m| m.get("type").as_str() == b"paint").collect();
    assert!(reports.iter().all(|m| m.get("heavy").is_number() && m.get("backdrop").is_number()), "paint reports carry numbers");
}
