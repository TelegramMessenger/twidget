//! Stage 4.5's inputs: documents whose animations, scripts, SVGs and canvases make compositor layers (movers,
//! nested ones, shimmers, scripted, SVG, canvas and still layers over the movers, clips, the layer cap), boxes
//! that cross the heavy-box and backdrop limits, the paint-policy fixture's cases as documents, damage between
//! paints, and the render throttle; frames, paints, hits and queries across them.

#![allow(dead_code)]

use twidget_json::Json;
use twidget_testkit::Lcg;

use super::inputs::{document, pick, quote};
use super::scripts::{host_message, patch_batch, query};
use super::{Script, Step};

fn el(id: i64, tag: &str, parent: i64, attrs: &[(&str, &str)]) -> String {
    let attrs: Vec<String> = attrs.iter().map(|(k, v)| format!("[{},{}]", quote(k), quote(v))).collect();
    format!(
        "{{\"op\":\"el\",\"id\":{id},\"tag\":{},\"attrs\":[{}]}},{{\"op\":\"ins\",\"parent\":{parent},\"id\":{id},\"before\":null}}",
        quote(tag),
        attrs.join(",")
    )
}

fn svg_el(id: i64, tag: &str, parent: i64, attrs: &[(&str, &str)]) -> String {
    let attrs: Vec<String> = attrs.iter().map(|(k, v)| format!("[{},{}]", quote(k), quote(v))).collect();
    format!(
        "{{\"op\":\"el\",\"id\":{id},\"tag\":{},\"ns\":\"svg\",\"attrs\":[{}]}},{{\"op\":\"ins\",\"parent\":{parent},\"id\":{id},\"before\":null}}",
        quote(tag),
        attrs.join(",")
    )
}

fn text(id: i64, parent: i64, data: &str) -> String {
    format!("{{\"op\":\"text\",\"id\":{id},\"data\":{}}},{{\"op\":\"ins\",\"parent\":{parent},\"id\":{id},\"before\":null}}", quote(data))
}

fn style_op(id: i64, name: &str, value: &str) -> String {
    format!("{{\"op\":\"style\",\"id\":{id},\"name\":{},\"value\":{}}}", quote(name), quote(value))
}

fn attr_op(id: i64, name: &str, value: &str) -> String {
    format!("{{\"op\":\"attr\",\"id\":{id},\"name\":{},\"value\":{}}}", quote(name), quote(value))
}

fn batch(ops: &[String]) -> Vec<u8> {
    format!("[{}]", ops.join(",")).into_bytes()
}

const LAYER_SHEET: &str = "@keyframes spin{from{transform:rotate(0deg)}to{transform:rotate(360deg)}} \
@keyframes slide{from{transform:translateX(0)}to{transform:translateX(120px)}} \
@keyframes fade{from{opacity:1}to{opacity:0.2}} \
@keyframes pulse{0%{transform:scale(1);opacity:1}50%{transform:scale(1.4);opacity:.6}100%{transform:scale(1);opacity:1}} \
@keyframes grow{from{width:10px}to{width:90px}} \
@keyframes glow{from{background-color:#000}to{background-color:#fff}} \
@keyframes shimmer{from{background-position:-100% 0}to{background-position:200% 0}} \
@keyframes pop{from{transform:scale(1.7)}} \
@keyframes rise{from{opacity:0;transform:translateY(20px)}} \
@keyframes steps{to{transform:translateX(60px)}} \
.s{animation:spin 1s linear infinite} .sl{animation:slide 2s ease-in-out infinite alternate} .f{animation:fade .8s linear 3} \
.p{animation:pulse 1.2s ease-in-out infinite} .g{animation:grow 1s infinite} .gl{animation:glow 1s infinite} \
.pop{animation:pop 100ms} .rise{animation:rise .5s} .st{animation:steps 1s steps(4) infinite} .late{animation:spin 1s linear 2s infinite} \
.sk{background-color:#ddd;background-image:linear-gradient(90deg,#ddd,#fff,#ddd);background-size:50% 100%;background-repeat:no-repeat;animation:shimmer 1s linear infinite} \
.t{transition:opacity .4s linear,transform .3s ease} .box{width:40px;height:40px;background:#c33} \
.clip{overflow:hidden;width:160px;height:90px;border-radius:12px} .round{border-radius:50%} \
.lbl{position:absolute;width:30px;height:12px;background:#fff} .sel{user-select:text}";

/// A document for layers: spinners, sliders and faders in and out of clips, a spinner inside a spinner, a glowing
/// parent, a shimmer, labels over a spinner, an SVG and a canvas a script changes, transformed and translucent
/// boxes, filters, selectable text in a moving card. Ids 10–79, the sheet 5.
pub fn layer_page(r: &mut Lcg) -> (Vec<String>, u32) {
    let mut ops = Vec::new();
    ops.push(format!("{{\"op\":\"el\",\"id\":5,\"tag\":\"style\",\"attrs\":[]}},{{\"op\":\"css\",\"id\":5,\"text\":{}}},{{\"op\":\"ins\",\"parent\":3,\"id\":5,\"before\":null}}", quote(LAYER_SHEET)));
    let movers = ["s", "sl", "f", "p", "g", "pop", "rise", "st", "late", "t", "s f", "", ""];
    let looks = [
        "width:40px;height:40px;background:#c33;margin:10px",
        "width:60px;height:20px;background:#3c3;display:inline-block",
        "width:30px;height:30px;background:#33c;margin:20px",
        "width:80px;height:24px;background:#cc3;border:2px solid #000",
        "width:40px;height:40px;background:#c3c;box-shadow:0 2px 6px rgba(0,0,0,.4)",
        "width:40px;height:40px;background:#3cc;opacity:.5",
        "width:40px;height:40px;background:#888;transform:rotate(10deg)",
        "width:40px;height:40px;background:#444;filter:blur(1px)",
        "width:40px;height:40px;background:#999;position:relative;left:12px",
    ];
    // A clipping card (10) with two movers (11, 12) and a text (13 > 14).
    let card = *pick(r, &["clip", "clip round", "", "clip t"]);
    ops.push(el(10, "div", 4, &[("class", card), ("style", "margin:8px;padding:6px;background:#eee")]));
    for id in [11, 12] {
        ops.push(el(id, "div", 10, &[("class", pick(r, &movers)), ("style", pick(r, &looks))]));
    }
    ops.push(el(13, "span", 10, &[("class", pick(r, &["sel", ""]))]));
    ops.push(text(14, 13, pick(r, &["label text", "selectable words here", "x"])));
    // An orbit (15) with a moon (16): nested movers.
    ops.push(el(15, "div", 4, &[("class", pick(r, &["s", "sl", "p", ""])), ("style", "margin:30px;width:100px;height:100px")]));
    ops.push(el(16, "div", 15, &[("class", pick(r, &["s", "f", "", "p"])), ("style", "width:20px;height:20px;background:red")]));
    // A glowing parent (17) holding a spinner (18).
    ops.push(el(17, "div", 4, &[("class", pick(r, &["gl", "", "gl s"])), ("style", "padding:20px")]));
    ops.push(el(18, "div", 17, &[("class", pick(r, &["s", "box", "sl"])), ("style", "width:30px;height:30px;background:#06c")]));
    // A shimmer (19), alone or with a child.
    ops.push(el(
        19,
        "div",
        4,
        &[
            ("class", pick(r, &["sk", "sk", ""])),
            ("style", pick(r, &["margin:10px;width:200px;height:20px;border-radius:6px", "width:180px;height:16px"])),
        ],
    ));
    if r.chance(4) {
        ops.push(el(20, "span", 19, &[]));
    }
    // A spinner (21) with labels over it (22, 23, 24).
    ops.push(el(21, "div", 4, &[("class", pick(r, &["s", "sl", "p"])), ("style", "width:40px;height:40px;margin:20px;background:red")]));
    let labels = r.below(5);
    for k in 0..labels {
        let style = format!("position:absolute;left:{}px;top:{}px;width:30px;height:12px;background:white", 15 + 8 * k, 400 + 6 * k);
        ops.push(el(22 + k as i64, "div", 4, &[("style", &style)]));
    }
    // An SVG (30) with a circle (31), a canvas (32), a static box (33).
    ops.push(svg_el(30, "svg", 4, &[("width", "60"), ("height", "60")]));
    ops.push(svg_el(31, "circle", 30, &[("cx", "30"), ("cy", "30"), ("r", "10"), ("fill", "#f00")]));
    ops.push(el(32, "canvas", 4, &[("width", "100"), ("height", "50")]));
    ops.push(el(33, "div", 4, &[("style", pick(r, &looks))]));
    // A row of small movers (40..), sometimes past the layer cap.
    let row = if r.chance(12) { 270 } else { *pick(r, &[0usize, 3, 8, 12]) };
    for k in 0..row {
        let id = 100 + k as i64;
        ops.push(el(
            id,
            "div",
            4,
            &[("class", pick(r, &["s", "p", "sl"])), ("style", "display:inline-block;width:10px;height:10px;margin:4px;background:#000")],
        ));
    }
    (ops, 420)
}

/// A move of an element by script: a transform or opacity through a style op.
fn scripted_move(r: &mut Lcg, id: i64, k: usize) -> String {
    match r.below(6) {
        0 => style_op(id, "opacity", &format!("0.{}", 1 + k % 9)),
        1 => style_op(id, "transform", &format!("translateX({}px) rotate({}deg)", k % 7, (k * 13) % 50)),
        2 => style_op(id, "transform", &format!("scale({})", 1.0 + (k % 4) as f64 * 0.3)),
        3 => style_op(id, "transform", &format!("translateX({}px)", if r.chance(6) { 400 } else { k % 9 })),
        4 => style_op(id, "translate", &format!("{}px 2px", k % 5)),
        _ => style_op(id, "transform", &format!("translateY({}px)", k % 6)),
    }
}

/// Frames over the layer page: ticks and paints, scripts that move elements batch after batch, an SVG and a canvas
/// changed again and again, class changes that start and end animations, Web Animations, hits and queries where the
/// layers show, sizes, themes, trims, policies, compositing turned off and on.
pub fn layer_script(r: &mut Lcg) -> Script {
    let (ops, max_id) = layer_page(r);
    let mut s = Script::new(max_id);
    s.dpr = *pick(r, &[1.0, 2.0, 3.0]);
    s.width = *pick(r, &[300.0, 360.0, 412.0]);
    s.after(0.0, Step::Compositing(!r.chance(8)));
    if r.chance(2) {
        s.after(
            0.0,
            Step::Message(b"{\"type\":\"listen\",\"types\":[\"animationiteration\",\"animationend\",\"transitionend\"]}".to_vec()),
        );
    }
    match r.below(4) {
        0 => s.after(0.0, Step::Message(b"{\"type\":\"policy\",\"policy\":\"compat\"}".to_vec())),
        1 => s.after(0.0, Step::Message(b"{\"type\":\"policy\",\"policy\":\"strict\",\"paintReports\":true}".to_vec())),
        _ => &mut s,
    };
    s.after(0.0, Step::Ops(document(&ops).into_bytes()));
    s.after(0.0, Step::Paint);
    let movers = [11i64, 12, 18, 33, *pick(r, &[16i64, 21, 30, 32])];
    let mut k = 0;
    for _ in 0..36 {
        k += 1;
        let dt = *pick(r, &[0.0, 8.0, 16.0, 16.0, 33.0, 120.0, 400.0, 1100.0]);
        match r.below(20) {
            0..=3 => {
                s.after(dt, Step::Tick);
                s.after(0.0, Step::Paint);
            }
            4..=6 => {
                let n = 1 + r.below(2);
                let moved: Vec<String> = (0..n).map(|i| scripted_move(r, movers[(k + i) % movers.len()], k)).collect();
                s.after(dt, Step::Ops(batch(&moved)));
                if r.chance(2) {
                    s.after(0.0, Step::Paint);
                }
            }
            7 => {
                let r2 = 8 + (k % 9);
                s.after(dt, Step::Ops(batch(&[attr_op(31, "r", &r2.to_string())])));
                s.after(0.0, Step::Paint);
            }
            8 => {
                let cmd = format!(
                    "{{\"op\":\"canvas\",\"id\":32,\"cmds\":[[\"s\",\"fillStyle\",\"#f00\"],[\"c\",\"fillRect\",{},0,10,10]]}}",
                    (k * 7) % 90
                );
                s.after(dt, Step::Ops(batch(&[cmd])));
                s.after(0.0, Step::Paint);
            }
            9 => {
                let id = *pick(r, &[11i64, 12, 15, 16, 18, 19, 21, 33]);
                let class = *pick(r, &["s", "sl", "f", "p", "g", "pop", "rise", "st", "sk", "", "box", "t"]);
                s.after(dt, Step::Ops(batch(&[attr_op(id, "class", class)])));
            }
            10 => {
                let id = *pick(r, &[11i64, 12, 18, 21, 33]);
                let (from, to) =
                    *pick(r, &[("translateX(0px)", "translateX(100px)"), ("rotate(0deg)", "rotate(180deg)"), ("scale(1)", "scale(2)")]);
                let prop = if r.chance(3) { "opacity" } else { "transform" };
                let (from, to) = if prop == "opacity" { ("1", "0") } else { (from, to) };
                let m = format!(
                    "{{\"type\":\"anim\",\"now\":{},\"anims\":[{{\"aid\":{},\"target\":{id},\"keyframes\":[{{\"offset\":0,\"values\":{{\"{prop}\":\"{from}\"}}}},{{\"offset\":1,\"values\":{{\"{prop}\":\"{to}\"}}}}],\"timing\":{{\"duration\":{},\"iterations\":{},\"easing\":\"linear\"}},\"startTime\":{}}}]}}",
                    r.below(500),
                    1 + r.below(3),
                    pick(r, &[300, 1000, 2000]),
                    pick(r, &["null", "1", "3"]),
                    pick(r, &["0", "-200", "100"])
                );
                s.after(dt, Step::Message(m.into_bytes()));
            }
            11 => {
                let q = format!("{{\"q\":\"elementFromPoint\",\"x\":{},\"y\":{}}}", r.below(300), r.below(500));
                s.query(dt, 7, &q);
            }
            12 => {
                let q = query(r, 40);
                s.after(dt, Step::Message(q.into_bytes()));
            }
            13 => {
                let b = patch_batch(r);
                s.after(dt, Step::Ops(b.into_bytes()));
                s.after(0.0, Step::Paint);
            }
            14 => {
                let step = match r.below(8) {
                    0 => Step::Width(*pick(r, &[280.0, 320.0, 400.0])),
                    1 => Step::Trim,
                    2 => Step::Compositing(r.chance(3)),
                    3 => Step::Message(host_message(r).into_bytes()),
                    4 => Step::Dark(r.chance(2)),
                    5 => Step::Charge(*pick(r, &[0.01, 0.05, 0.2])),
                    6 => Step::MaxHeight(*pick(r, &[200.0, 600.0, 1200.0])),
                    _ => Step::Inspect,
                };
                s.after(dt, step);
                s.after(0.0, Step::Paint);
            }
            15 => {
                // A drag over the moving card's text: the selection's tint in a layer.
                s.after(dt, Step::Touch { action: 0, x: 20.0, y: 20.0, pointer: 1, primary: true, mouse: true, modifiers: 0, button: 0 });
                s.after(
                    16.0,
                    Step::Touch { action: 1, x: 120.0, y: 30.0, pointer: 1, primary: true, mouse: true, modifiers: 0, button: 0 },
                );
                s.after(
                    16.0,
                    Step::Touch { action: 2, x: 120.0, y: 30.0, pointer: 1, primary: true, mouse: true, modifiers: 0, button: 0 },
                );
                s.after(0.0, Step::Paint);
                s.after(0.0, Step::Observe);
            }
            16 => {
                s.after(dt, Step::Hover { action: 0, x: r.below(300) as f64, y: r.below(300) as f64, modifiers: 0 });
            }
            17 => {
                let id = *pick(r, &[22i64, 23, 11, 33]);
                s.after(dt, Step::Ops(batch(&[format!("{{\"op\":\"rm\",\"id\":{id}}}")])));
                s.after(0.0, Step::Paint);
            }
            18 => {
                s.after(dt, Step::Inspect);
            }
            _ => {
                s.after(dt, Step::Paint);
                s.after(0.0, Step::Paint);
            }
        }
    }
    s.after(16.0, Step::Paint);
    s.after(0.0, Step::Inspect);
    s
}

const HEAVY_STYLES: [&str; 16] = [
    "box-shadow:0 2px 4px red",
    "box-shadow:0 2px 0 red",
    "box-shadow:inset 0 0 0 1px #000",
    "box-shadow:0 0 0 1px #000, 0 3px 6px rgba(0,0,0,.2)",
    "filter:brightness(1)",
    "filter:none",
    "backdrop-filter:blur(4px)",
    "-webkit-backdrop-filter:blur(3px)",
    "opacity:0.5",
    "opacity:0.999",
    "opacity:1",
    "opacity:0",
    "visibility:hidden;opacity:.5",
    "display:none;filter:blur(2px)",
    "display:contents;opacity:.5",
    "",
];

/// Boxes around the paint limits: heavy and plain ones, disabled and pressed buttons, ::before boxes, wrappers that
/// hide them, animations of opacity, filters, shadows and backdrop-filter starting and ending, Web Animations,
/// policies switched, across frames and paints; most cross a limit somewhere.
pub fn heavy_script(r: &mut Lcg) -> Script {
    let mut ops = Vec::new();
    let sheet = ".h{box-shadow:0 2px 6px rgba(0,0,0,.4)} .bf::before{content:\"\";display:block;width:6px;height:6px;filter:blur(1px)} \
.bn::before{content:none;opacity:.5} .ba::after{content:\"x\";opacity:.4} .g{backdrop-filter:blur(3px)} \
@keyframes fade{from{opacity:1}to{opacity:.2}} @keyframes still{from{opacity:1}to{opacity:1}} @keyframes spin{to{transform:rotate(1turn)}} \
@keyframes glow{from{box-shadow:0 0 0 0 rgba(0,0,0,0)}to{box-shadow:0 4px 12px rgba(0,0,0,.4)}} @keyframes ring{from{box-shadow:inset 0 0 0 0 #fff}to{box-shadow:inset 0 0 0 4px #fff}} \
@keyframes frost{from{backdrop-filter:blur(0px)}to{backdrop-filter:blur(8px)}} @keyframes soft{from{filter:blur(0px)}to{filter:blur(2px)}} \
.a1{animation:fade .5s linear} .a2{animation:fade 1s linear infinite} .a3{animation:still 1s 10s} .a4{animation:spin 1s infinite} \
.a5{animation:glow .4s 2} .a6{animation:ring 1s infinite} .a7{animation:frost .6s} .a8{animation:soft .3s} .t{transition:opacity .3s linear,filter .2s}";
    ops.push(format!("{{\"op\":\"el\",\"id\":5,\"tag\":\"style\",\"attrs\":[]}},{{\"op\":\"css\",\"id\":5,\"text\":{}}},{{\"op\":\"ins\",\"parent\":3,\"id\":5,\"before\":null}}", quote(sheet)));
    ops.push(el(6, "div", 4, &[("style", pick(r, &["", "display:none", "display:contents;opacity:.5", "opacity:.5"]))]));
    let n = 8 + r.below(14);
    for i in 0..n {
        let id = 10 + i as i64;
        let parent = if r.chance(6) { 6 } else { 4 };
        let tag = *pick(r, &["div", "div", "div", "button", "span"]);
        let mut style = format!("width:10px;height:10px;{}", pick(r, &HEAVY_STYLES));
        if tag == "button" && r.chance(2) {
            style.push_str(";opacity:.5");
        }
        let class = *pick(r, &["", "", "h", "bf", "bn", "ba", "g", "t", "a1", "a3", ""]);
        let mut attrs = vec![("style", style.as_str()), ("class", class)];
        if tag == "button" && r.chance(2) {
            attrs.push(("disabled", ""));
        }
        ops.push(el(id, tag, parent, &attrs));
    }
    let max_id = 60;
    let mut s = Script::new(max_id);
    s.after(0.0, Step::Compositing(r.chance(3)));
    match r.below(5) {
        0 => s.after(0.0, Step::Message(b"{\"type\":\"policy\",\"policy\":\"compat\"}".to_vec())),
        1 | 2 => s.after(0.0, Step::Message(b"{\"type\":\"policy\",\"policy\":\"strict\",\"paintReports\":true}".to_vec())),
        _ => &mut s,
    };
    s.after(0.0, Step::Ops(document(&ops).into_bytes()));
    s.after(0.0, Step::Tick);
    s.after(0.0, Step::Paint);
    for _ in 0..20 {
        let dt = *pick(r, &[0.0, 16.0, 16.0, 100.0, 350.0, 700.0]);
        let id = 10 + r.below(n) as i64;
        match r.below(14) {
            0..=2 => {
                let class = *pick(r, &["h", "bf", "bn", "ba", "g", "t", "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", ""]);
                s.after(dt, Step::Ops(batch(&[attr_op(id, "class", class)])));
            }
            3 | 4 => {
                s.after(dt, Step::Ops(batch(&[style_op(id, "opacity", pick(r, &["0.5", "1", "0", ".85", ""]))])));
            }
            5 => {
                let fresh = 40 + r.below(10) as i64;
                s.after(
                    dt,
                    Step::Ops(batch(&[el(fresh, "div", 4, &[("style", &format!("width:10px;height:10px;{}", pick(r, &HEAVY_STYLES)))])])),
                );
            }
            6 => {
                s.after(dt, Step::Ops(batch(&[format!("{{\"op\":\"rm\",\"id\":{id}}}")])));
            }
            7 => {
                s.after(dt, Step::Ops(batch(&[attr_op(6, "style", pick(r, &["", "display:none", "display:contents;opacity:.5"]))])));
            }
            8 => {
                let m = format!(
                    "{{\"type\":\"anim\",\"now\":0,\"anims\":[{{\"aid\":{},\"target\":{id},\"keyframes\":[{{\"offset\":0,\"values\":{{\"opacity\":\"1\"}}}},{{\"offset\":1,\"values\":{{\"opacity\":\"1\"}}}}],\"timing\":{{\"duration\":1000,{}}},\"startTime\":{}}}]}}",
                    1 + r.below(2),
                    pick(r, &["\"fill\":\"forwards\"", "\"iterations\":null", "\"iterations\":1"]),
                    pick(r, &["-5000", "0", "-500"])
                );
                s.after(dt, Step::Message(m.into_bytes()));
            }
            9 => {
                let m = *pick(
                    r,
                    &[
                        "{\"type\":\"policy\",\"policy\":\"compat\"}",
                        "{\"type\":\"policy\",\"policy\":\"strict\"}",
                        "{\"type\":\"policy\",\"policy\":\"strict\",\"paintReports\":true}",
                    ],
                );
                s.after(dt, Step::Message(m.as_bytes().to_vec()));
            }
            10 => {
                // A press holds :active on a button.
                s.after(
                    dt,
                    Step::Touch {
                        action: 0,
                        x: 5.0,
                        y: 5.0 + 10.0 * r.below(n) as f64,
                        pointer: 1,
                        primary: true,
                        mouse: true,
                        modifiers: 0,
                        button: 0,
                    },
                );
            }
            11 => {
                s.after(dt, Step::Inspect);
            }
            _ => {
                s.after(dt, Step::Tick);
                s.after(0.0, Step::Paint);
            }
        }
    }
    s.after(16.0, Step::Tick);
    s.after(0.0, Step::Paint);
    s.after(0.0, Step::Inspect);
    s
}

/// Paints between changes: colours, positions, paint order, canvases drawing, sizes and trims that make the next
/// paint whole, with and without layers.
pub fn damage_script(r: &mut Lcg) -> Script {
    let mut ops = Vec::new();
    for i in 0..6i64 {
        let style = format!(
            "position:absolute;left:{}px;top:{}px;width:{}px;height:20px;background:{}",
            10 + 30 * i + r.below(10) as i64,
            10 + (i % 3) * 15,
            20 + r.below(30),
            pick(r, &["red", "blue", "green", "#abc"])
        );
        ops.push(el(10 + i, "div", 4, &[("style", &style)]));
    }
    ops.push(el(20, "canvas", 4, &[("width", "10"), ("height", "10"), ("style", "position:absolute;left:200px;top:60px")]));
    ops.push(el(21, "canvas", 4, &[("width", "30"), ("height", "20")]));
    ops.push(el(22, "div", 4, &[("class", "s"), ("style", "width:20px;height:20px;margin-top:90px;background:#0a0")]));
    ops.push(format!("{{\"op\":\"el\",\"id\":5,\"tag\":\"style\",\"attrs\":[]}},{{\"op\":\"css\",\"id\":5,\"text\":{}}},{{\"op\":\"ins\",\"parent\":3,\"id\":5,\"before\":null}}", quote(LAYER_SHEET)));
    let mut s = Script::new(40);
    s.dpr = *pick(r, &[1.0, 2.0]);
    s.after(0.0, Step::Compositing(r.chance(2)));
    s.after(0.0, Step::Ops(document(&ops).into_bytes()));
    s.after(0.0, Step::Paint);
    s.after(16.0, Step::Paint);
    for k in 0..24 {
        let dt = *pick(r, &[0.0, 16.0, 50.0]);
        let id = 10 + r.below(6) as i64;
        let step = match r.below(12) {
            0 | 1 => Step::Ops(batch(&[style_op(id, "background-color", pick(r, &["yellow", "red", "#123", "transparent"]))])),
            2 => Step::Ops(batch(&[style_op(id, "left", &format!("{}px", r.below(250)))])),
            3 => Step::Ops(batch(&[
                style_op(id, "z-index", pick(r, &["1", "2", "-1", "auto"])),
                style_op(10 + r.below(6) as i64, "z-index", "1"),
            ])),
            4 | 5 => {
                let canvas = *pick(r, &[20i64, 21]);
                Step::Ops(batch(&[format!(
                    "{{\"op\":\"canvas\",\"id\":{canvas},\"cmds\":[[\"s\",\"fillStyle\",{}],[\"c\",\"fillRect\",{},0,5,5]]}}",
                    quote(pick(r, &["#f00", "#0f0"])),
                    k % 6
                )]))
            }
            6 => Step::Ops(batch(&[style_op(id, "width", &format!("{}px", 10 + r.below(40)))])),
            7 => Step::Width(*pick(r, &[300.0, 320.0])),
            8 => Step::Trim,
            9 => Step::Ops(batch(&[style_op(id, "box-shadow", pick(r, &["0 0 6px red", "none", "2px 2px 0 #000"]))])),
            10 => Step::Tick,
            _ => Step::Ops(batch(&[attr_op(22, "class", pick(r, &["s", "", "sl"]))])),
        };
        s.after(dt, step);
        s.after(0.0, Step::Paint);
    }
    s
}

/// The render throttle: costly passes charged after the load grace and before it, batches and frames while it
/// holds the widget back and after, what wants_tick and next_tick_in say meanwhile.
pub fn throttle_script(r: &mut Lcg) -> Script {
    let (ops, max_id) = super::scripts::page(r);
    let mut s = Script::new(max_id);
    s.after(0.0, Step::Compositing(r.chance(2)));
    s.after(0.0, Step::Ops(document(&ops).into_bytes()));
    for _ in 0..28 {
        let dt = *pick(r, &[1.0, 16.0, 16.0, 40.0, 150.0, 600.0, 1100.0]);
        match r.below(9) {
            0 | 1 => {
                s.after(dt, Step::Charge(*pick(r, &[0.005, 0.017, 0.03, 0.06, 0.2, 0.5])));
            }
            2 | 3 => {
                let b = patch_batch(r);
                s.after(dt, Step::Ops(b.into_bytes()));
            }
            4 | 5 => {
                s.after(dt, Step::Tick);
            }
            6 => {
                s.after(dt, Step::Paint);
            }
            7 => {
                s.after(dt, Step::Touch { action: 0, x: 10.0, y: 10.0, pointer: 1, primary: true, mouse: false, modifiers: 0, button: 0 });
                s.after(
                    16.0,
                    Step::Touch { action: 2, x: 10.0, y: 10.0, pointer: 1, primary: true, mouse: false, modifiers: 0, button: 0 },
                );
            }
            _ => {
                s.after(dt, Step::Inspect);
            }
        }
    }
    s
}

/// A corpus widget's static document with compositing on, under a policy, run for frames: ticks, paints, hits and
/// queries over a few seconds.
pub fn widget_frames(r: &mut Lcg, ops: &[String], max_id: u32, policy: &str, compositing: bool) -> Script {
    let mut s = Script::new(max_id);
    s.width = *pick(r, &[300.0, 360.0, 412.5]);
    s.dpr = *pick(r, &[1.0, 2.0]);
    s.after(0.0, Step::Compositing(compositing));
    if !policy.is_empty() {
        s.after(0.0, Step::Message(policy.as_bytes().to_vec()));
    }
    s.after(0.0, Step::Ops(document(ops).into_bytes()));
    s.after(0.0, Step::Paint);
    for _ in 0..8 {
        match r.below(8) {
            0..=3 => {
                s.after(*pick(r, &[16.0, 16.0, 33.0, 250.0, 700.0]), Step::Tick);
                s.after(0.0, Step::Paint);
            }
            4 => {
                let q = format!("{{\"q\":\"elementFromPoint\",\"x\":{},\"y\":{}}}", r.below(320), r.below(400));
                s.query(16.0, 9, &q);
            }
            5 => {
                let q = query(r, max_id.max(3));
                s.after(0.0, Step::Message(q.into_bytes()));
            }
            6 => {
                s.after(
                    16.0,
                    Step::Touch {
                        action: 0,
                        x: r.below(300) as f64,
                        y: r.below(300) as f64,
                        pointer: 1,
                        primary: true,
                        mouse: true,
                        modifiers: 0,
                        button: 0,
                    },
                );
                s.after(
                    16.0,
                    Step::Touch {
                        action: 2,
                        x: r.below(300) as f64,
                        y: r.below(300) as f64,
                        pointer: 1,
                        primary: true,
                        mouse: true,
                        modifiers: 0,
                        button: 0,
                    },
                );
                s.after(0.0, Step::Paint);
            }
            _ => {
                s.after(16.0, Step::Inspect);
            }
        }
    }
    s.after(1000.0, Step::Tick);
    s.after(0.0, Step::Paint);
    s.after(0.0, Step::Inspect);
    s
}

// ---- The paint-policy fixture's cases as documents ----------------------------------------------------------

/// A fixture style (camelCase names, strings or numbers) as declarations; a `button` entry makes the element a
/// `<button>`, disabled or pressed.
fn fixture_style(style: &Json) -> (String, Option<Vec<u8>>) {
    let mut decl = String::new();
    let mut button = None;
    let Json::Object(o) = style else { return (decl, button) };
    for (k, v) in o {
        if k.as_slice() == b"button" {
            button = Some(v.as_str().to_vec());
            continue;
        }
        let mut name = String::new();
        for c in String::from_utf8_lossy(k).chars() {
            if c.is_ascii_uppercase() {
                name.push('-');
                name.push(c.to_ascii_lowercase());
            } else {
                name.push(c);
            }
        }
        let value =
            if v.is_string() { String::from_utf8_lossy(v.as_str()).into_owned() } else { String::from_utf8_lossy(&v.dump()).into_owned() };
        decl.push_str(&format!("{name}:{value};"));
    }
    (decl, button)
}

fn names(v: &Json) -> Vec<String> {
    if v.is_string() {
        return vec![String::from_utf8_lossy(v.as_str()).into_owned()];
    }
    v.arr().iter().map(|n| String::from_utf8_lossy(n.as_str()).into_owned()).collect()
}

/// The fixture's cases (widget-sandbox's paint-policy.json) as scripts: each box an element of its own (positioned so
/// that a press finds a pressed button), each pseudo-element a rule, an animation a keyframes rule, a transition two
/// batches or a Web Animation, each check a frame, a paint and an inspection; under `policy`.
pub fn fixture_scripts(policy: &str, compositing: bool) -> Vec<(Script, String)> {
    let path = super::inputs::widget_sandbox().join("packages/core/tests/fixtures/paint-policy.json");
    let Ok(bytes) = std::fs::read(&path) else { return Vec::new() };
    let Some(fixture) = Json::parse(&bytes) else { return Vec::new() };
    let mut out = Vec::new();
    for case in fixture.get("cases").arr() {
        let mut ids: Vec<String> = Vec::new();
        let mut pseudo_rules: Vec<(String, String)> = Vec::new();
        let mut keyframes: Vec<String> = Vec::new();
        let mut s = Script::new(200);
        s.after(0.0, Step::Compositing(compositing));
        if !policy.is_empty() {
            s.after(0.0, Step::Message(policy.as_bytes().to_vec()));
        }
        let sheet_element = r#"{"op":"el","id":5,"tag":"style","attrs":[]},{"op":"ins","parent":3,"id":5,"before":null}"#;
        s.after(0.0, Step::Ops(document(&[sheet_element.to_string()]).into_bytes()));
        let sheet = |pseudo_rules: &[(String, String)], keyframes: &[String]| -> String {
            let mut css = String::new();
            for (sel, decl) in pseudo_rules {
                css.push_str(&format!("{sel}{{{decl}}} "));
            }
            for k in keyframes {
                css.push_str(k);
                css.push(' ');
            }
            format!("{{\"op\":\"css\",\"id\":5,\"text\":{}}}", quote(&css))
        };
        let mut inline: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
        let mut aid = 1;
        for step in case.get("steps").arr() {
            let mut batch_ops: Vec<String> = Vec::new();
            if let Some(set) = step.find("set") {
                let (decl, button) = fixture_style(step.get("style"));
                for name in names(set) {
                    if step.get("pseudo").as_bool(false) || name.contains("::") {
                        let (owner, part) = name.split_once("::").unwrap_or((&name, "before"));
                        pseudo_rules.retain(|(sel, _)| sel != &format!("#{owner}::{part}"));
                        pseudo_rules.push((format!("#{owner}::{part}"), decl.clone()));
                        if !ids.contains(&owner.to_string()) {
                            ids.push(owner.to_string());
                            let id = 10 + ids.len() as i64;
                            batch_ops.push(el(id, "div", 4, &[("id", owner), ("style", "position:absolute;width:8px;height:8px")]));
                        }
                        batch_ops.push(sheet(&pseudo_rules, &keyframes));
                        continue;
                    }
                    if name == "html" || name == "body" {
                        batch_ops.push(attr_op(if name == "html" { 2 } else { 4 }, "style", &decl));
                        continue;
                    }
                    let index = ids.iter().position(|n| n == &name);
                    let id = 10 + index.map_or(ids.len() + 1, |i| i + 1) as i64;
                    let place =
                        format!("position:absolute;left:{}px;top:{}px;width:8px;height:8px;", 10 * ((id - 10) % 20), 10 * ((id - 10) / 20));
                    let full = format!("{place}{decl}");
                    inline.insert(name.clone(), full.clone());
                    if index.is_none() {
                        ids.push(name.clone());
                        let tag = if button.is_some() { "button" } else { "div" };
                        let mut attrs = vec![("id", name.as_str()), ("style", full.as_str())];
                        if button.as_deref() == Some(b"disabled") {
                            attrs.push(("disabled", ""));
                        }
                        batch_ops.push(el(id, tag, 4, &attrs));
                    } else {
                        batch_ops.push(attr_op(id, "style", &full));
                        if button.as_deref() == Some(b"disabled") {
                            batch_ops.push(attr_op(id, "disabled", ""));
                        }
                    }
                    if button.as_deref() == Some(b"pressed") {
                        let x = 10.0 * ((id - 10) % 20) as f64 + 4.0;
                        let y = 10.0 * ((id - 10) / 20) as f64 + 4.0;
                        s.after(0.0, Step::Ops(batch(&batch_ops)));
                        batch_ops.clear();
                        s.after(0.0, Step::Touch { action: 0, x, y, pointer: 2, primary: true, mouse: true, modifiers: 0, button: 0 });
                    }
                }
            }
            if let Some(remove) = step.find("remove") {
                for name in names(remove) {
                    if let Some(i) = ids.iter().position(|n| n == &name) {
                        batch_ops.push(format!("{{\"op\":\"rm\",\"id\":{}}}", 11 + i as i64));
                    }
                }
            }
            if let Some(start) = step.find("start") {
                let animation = String::from_utf8_lossy(step.get("animation").as_str()).into_owned();
                let frames: Vec<(f64, String)> = step
                    .get("keyframes")
                    .arr()
                    .iter()
                    .map(|k| {
                        let mut values = Json::object();
                        if let Json::Object(o) = k {
                            for (name, v) in o {
                                if name.as_slice() != b"offset" {
                                    values.set(name.clone(), v.clone());
                                }
                            }
                        }
                        (k.get("offset").as_number(0.0), fixture_style(&values).0)
                    })
                    .collect();
                let (kind, label) = animation.split_once(':').unwrap_or(("a", &animation));
                for name in names(start) {
                    let Some(i) = ids.iter().position(|n| n == &name) else { continue };
                    let id = 11 + i as i64;
                    let base = inline.get(&name).cloned().unwrap_or_default();
                    match kind {
                        "w" => {
                            let kf: Vec<String> = step
                                .get("keyframes")
                                .arr()
                                .iter()
                                .map(|k| {
                                    let mut values = Json::object();
                                    if let Json::Object(o) = k {
                                        for (n, v) in o {
                                            if n.as_slice() != b"offset" {
                                                values.set(n.clone(), if v.is_string() { v.clone() } else { Json::from(v.dump()) });
                                            }
                                        }
                                    }
                                    format!(
                                        "{{\"offset\":{},\"values\":{}}}",
                                        k.get("offset").as_number(0.0),
                                        String::from_utf8_lossy(&values.dump())
                                    )
                                })
                                .collect();
                            let m = format!(
                                "{{\"type\":\"anim\",\"now\":0,\"anims\":[{{\"aid\":{aid},\"target\":{id},\"keyframes\":[{}],\"timing\":{{\"duration\":1000,\"iterations\":null}},\"startTime\":0}}]}}",
                                kf.join(",")
                            );
                            aid += 1;
                            s.after(0.0, Step::Ops(batch(&batch_ops)));
                            batch_ops.clear();
                            s.after(0.0, Step::Message(m.into_bytes()));
                        }
                        "t" => {
                            let (first, last) = (
                                frames.first().map(|f| f.1.clone()).unwrap_or_default(),
                                frames.last().map(|f| f.1.clone()).unwrap_or_default(),
                            );
                            batch_ops.push(attr_op(id, "style", &format!("{base}{first}transition:{label} 2s linear;")));
                            s.after(0.0, Step::Ops(batch(&batch_ops)));
                            batch_ops.clear();
                            batch_ops.push(attr_op(id, "style", &format!("{base}{last}transition:{label} 2s linear;")));
                        }
                        _ => {
                            let rule_name = format!("k{}", keyframes.len());
                            let mut rule = format!("@keyframes {rule_name}{{");
                            for (offset, decl) in &frames {
                                rule.push_str(&format!("{}%{{{decl}}}", offset * 100.0));
                            }
                            rule.push('}');
                            keyframes.push(rule);
                            batch_ops.push(sheet(&pseudo_rules, &keyframes));
                            batch_ops.push(attr_op(id, "style", &format!("{base}animation:{rule_name} 2s linear infinite;")));
                        }
                    }
                }
            }
            if let Some(end) = step.find("end") {
                for name in names(end) {
                    let Some(i) = ids.iter().position(|n| n == &name) else { continue };
                    let id = 11 + i as i64;
                    if step.get("animation").as_str().starts_with(b"w:") {
                        s.after(0.0, Step::Ops(batch(&batch_ops)));
                        batch_ops.clear();
                        s.after(
                            0.0,
                            Step::Message(
                                format!("{{\"type\":\"anim\",\"now\":0,\"anims\":[{{\"aid\":{},\"remove\":true}}]}}", aid - 1).into_bytes(),
                            ),
                        );
                    } else {
                        batch_ops.push(attr_op(id, "style", &inline.get(&name).cloned().unwrap_or_default()));
                    }
                }
            }
            if !batch_ops.is_empty() {
                s.after(16.0, Step::Ops(batch(&batch_ops)));
            }
            if step.find("check").is_some() {
                s.after(16.0, Step::Tick);
                s.after(0.0, Step::Paint);
                s.after(0.0, Step::Inspect);
            }
        }
        out.push((s, format!("paint-policy case {:?} {policy}", String::from_utf8_lossy(case.get("name").as_str()))));
    }
    out
}
