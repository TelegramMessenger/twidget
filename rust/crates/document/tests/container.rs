//! The container's own parts against renderer.cpp's, through the renderer on `twidget_document`'s engine and
//! `oracle/renderer_oracle`, checked answer by answer and end to end as `oracle.rs` does: written widgets for fonts grown
//! past 4096 and faces past 256, the width cache's entry, key and byte bounds (measured with the stub's widths growing
//! per call, so every hit and miss shows), the text breaker's clusters, images decoded, late, broken, refused, the
//! placeholder, `tw-canvas:` sources and the image table's bound, every paint call into the list's commands, the paint
//! limit's look without effects, media features at scales and widths; then generated widgets mixing them. Skipped
//! without `TWIDGET_ORACLE_DIR`.

#[path = "../../renderer/tests/common/mod.rs"]
mod common;
mod support;

use common::inputs::{document, html_to_ops, pick, quote};
use common::*;
use support::compare_engines;
use twidget_testkit::{Lcg, Oracle};

fn oracle() -> Option<Oracle> {
    Oracle::find("renderer_oracle")
}

/// A widget's HTML as a script: the stub's measuring, its ops in one batch, then a paint.
fn widget(html: &str, width: f64, dpr: f64, measuring: u8) -> (Script, u32) {
    let (ops, last) = html_to_ops(html.as_bytes(), 4);
    let max_id = (last as u32).min(60000);
    let mut s = Script::new(max_id);
    s.width = width;
    s.dpr = dpr;
    s.after(0.0, Step::Backend(measuring));
    s.after(0.0, Step::Ops(document(&ops).into_bytes()));
    s.after(16.0, Step::Paint);
    (s, max_id)
}

fn style_op(id: i64, name: &str, value: &str) -> String {
    format!("{{\"op\":\"style\",\"id\":{id},\"name\":{},\"value\":{}}}", quote(name), quote(value))
}

fn batch(ops: &[String]) -> Step {
    Step::Ops(format!("[{}]", ops.join(",")).into_bytes())
}

fn query(s: &mut Script, q: &str) {
    s.after(0.0, Step::Message(format!("{{\"type\":\"query\",\"qid\":1,\"query\":{q}}}").into_bytes()));
}

/// Eight boxes whose font sizes every batch changes: a font per box and batch, past the 4096 a document keeps before it
/// is built anew, then faces past 256 once the first document's are idle.
fn fonts_past_4096() -> Script {
    let mut ops = Vec::new();
    for i in 0..8 {
        let (e, t) = (10 + i, 20 + i);
        ops.push(format!(
            "{{\"op\":\"el\",\"id\":{e},\"tag\":\"div\",\"attrs\":[[\"style\",\"font-size:10px\"]]}},{{\"op\":\"ins\",\"parent\":4,\"id\":{e},\"before\":null}},{{\"op\":\"text\",\"id\":{t},\"data\":\"words in box {i}\"}},{{\"op\":\"ins\",\"parent\":{e},\"id\":{t},\"before\":null}}"
        ));
    }
    let mut s = Script::new(40);
    s.after(0.0, Step::Backend(1));
    s.after(0.0, Step::Ops(document(&ops).into_bytes()));
    s.after(16.0, Step::Paint);
    let mut k = 0;
    for step in 0..560 {
        let mut b = Vec::new();
        for i in 0..8 {
            k += 1;
            b.push(style_op(10 + i, "font-size", &format!("{:.2}px", 8.0 + f64::from(k) * 0.01)));
        }
        s.after(16.0, batch(&b));
        if step % 40 == 0 || (505..525).contains(&step) {
            s.after(0.0, Step::Paint);
            s.after(0.0, Step::Inspect);
        }
    }
    s.after(16.0, Step::Trim);
    s.after(16.0, Step::Paint);
    s
}

/// Distinct words past a face's 4096 entries, words past the 256-byte key, and two faces whose keys pass the 1 MiB the
/// faces share; laid out, painted, at other widths (hits now), trimmed (measured again).
fn width_cache() -> Script {
    let mut html = String::from("<p style=\"font-size:12px\">");
    for i in 0..4300 {
        html.push_str(&format!("w{i:05} "));
    }
    html.push_str("</p><p style=\"font-size:11px\">");
    for i in 0..6 {
        html.push_str(&format!("{}{i} ", "L".repeat(300)));
    }
    for (size, words) in [(13, 2700), (14, 2700)] {
        html.push_str(&format!("</p><p style=\"font-size:{size}px\">"));
        for i in 0..words {
            html.push_str(&format!("{i:06}{} ", "k".repeat(194)));
        }
    }
    html.push_str("</p>");
    let (mut s, _) = widget(&html, 320.0, 1.0, 1);
    s.after(16.0, Step::Width(280.0));
    s.after(16.0, Step::Paint);
    s.after(16.0, Step::Trim);
    s.after(16.0, Step::Width(300.0));
    s.after(16.0, Step::Paint);
    s
}

/// Words the breaker cuts in boxes 0, 1, 13 and 37 px wide: ASCII, combining marks, ZWJ sequences, flags, skin tones,
/// tags, variation selectors, Indic, Hebrew and Arabic marks, CJK; overflow-wrap, word-break and pre-wrap.
fn breaker(width: f64, measuring: u8) -> Script {
    let texts = [
        "supercalifragilisticexpialidocious-and-then-some-more-letters",
        "e\u{301}e\u{301}e\u{301}a\u{308}o\u{302}u\u{30a}e\u{301}e\u{301}",
        "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}",
        "\u{1f1fa}\u{1f1f8}\u{1f1eb}\u{1f1f7}\u{1f1e9}\u{1f1ea}\u{1f1ef}\u{1f1f5}\u{1f1fa}",
        "\u{1f44d}\u{1f3fd}\u{1f44d}\u{1f3ff}\u{1f44b}\u{1f3fb}",
        "\u{1f3f4}\u{e0067}\u{e0062}\u{e0073}\u{e0063}\u{e0074}\u{e007f}\u{1f3f4}",
        "\u{270c}\u{fe0f}\u{2764}\u{fe0f}\u{263a}\u{fe0e}x",
        "\u{928}\u{92e}\u{938}\u{94d}\u{924}\u{947}\u{928}\u{92e}\u{938}\u{94d}\u{924}\u{947}",
        "\u{5e9}\u{5b8}\u{5c1}\u{5dc}\u{5d5}\u{5b9}\u{5dd}\u{5e9}\u{5b8}\u{5c1}",
        "\u{645}\u{64e}\u{631}\u{652}\u{62d}\u{64e}\u{628}\u{64b}\u{627}",
        "\u{6f22}\u{5b57}\u{6f22}\u{5b57}\u{304b}\u{306a}\u{304b}\u{306a}",
        "mixed\u{301}\u{1f600}words\u{200d}\u{1f525}and\u{fe0f}more",
    ];
    let mut html = String::new();
    for (i, w) in [0, 1, 13, 37].into_iter().enumerate() {
        let wrap =
            ["overflow-wrap:break-word", "word-break:break-all", "overflow-wrap:anywhere", "overflow-wrap:break-word;letter-spacing:1px"]
                [i];
        for t in &texts {
            html.push_str(&format!("<div style=\"width:{w}px;{wrap}\">{t}</div>"));
        }
    }
    html.push_str(
        "<textarea style=\"width:40px;white-space:pre-wrap\">aaaaaaaaaaaaaaaaaaaaaaaa \u{1f600}\u{1f600}\u{1f600} bbbb</textarea>",
    );
    let (mut s, max_id) = widget(&html, width, 1.0, measuring);
    query(&mut s, "{\"q\":\"rects\",\"id\":6}");
    for id in [6, 30, 60, 90] {
        query(&mut s, &format!("{{\"q\":\"box\",\"id\":{}}}", id.min(max_id)));
    }
    s.after(16.0, Step::Width(width / 2.0));
    s.after(16.0, Step::Paint);
    s
}

const PNG: &str = "data:image/png;base64,iVBORw0KGgo=";

/// Images decoded, late, broken, refused and the SVG placeholder, by `<img>` and as backgrounds tiled every way, a
/// `<canvas>`'s own source and a forged one; late sizes, a canvas resized, a repaint.
fn images() -> Script {
    let srcs = [
        PNG,
        "data:image/png;base64,async1",
        "data:image/jpeg;base64,async2",
        "data:image/png;base64,fail",
        "data:image/svg+xml,%3Csvg%2F%3E",
        "data:text/plain,hi",
        "DATA:IMAGE/PNG;BASE64,QUJD",
        "data:image/png,raw",
        "data:image/png",
        "  data:image/gif;base64,R0lG  ",
        "data:\timage/webp;base64,UklG",
        "tw-canvas:99/10x10",
        "tw-canvas:x",
    ];
    let mut html = String::new();
    for (i, src) in srcs.iter().enumerate() {
        let size = if i % 3 == 0 { " width=\"30\" height=\"20\"" } else { "" };
        html.push_str(&format!("<img src=\"{src}\"{size}>"));
    }
    let tiles = [
        "repeat;background-size:10px 10px",
        "repeat-x;background-size:7px 5px",
        "repeat-y;background-size:5px 7px",
        "no-repeat;background-position:center",
        "repeat;background-size:1px 1px",
        "repeat;background-size:3px 3px",
        "repeat;background-size:0.5px 4px",
        "repeat;background-size:10px 10px;background-position:-3px -4px",
        "round;background-size:10px",
    ];
    for (i, t) in tiles.iter().enumerate() {
        let src = [PNG, "data:image/svg+xml;utf8,x", "data:image/png;base64,async3"][i % 3];
        html.push_str(&format!(
            "<div style=\"width:200px;height:200px;border-radius:8px;border:3px solid red;padding:4px;background-image:url('{src}');background-repeat:{t};background-clip:padding-box\"></div>"
        ));
    }
    html.push_str(
        "<canvas width=\"30\" height=\"20\"></canvas><div style=\"background:url(data:image/png;base64,QUJD) 0 0/40px 40px\">x</div>",
    );
    let (mut s, max_id) = widget(&html, 300.0, 2.0, 0);
    for (id, w, h) in [(1, 30.0, 10.0), (2, 12.0, 9.0), (3, 50.0, 60.0), (2, 12.0, 9.0), (7, 4.0, 4.0)] {
        s.after(16.0, Step::ImageReady { id, w, h });
        s.after(0.0, Step::Paint);
    }
    // The canvas resized: its source names its new size.
    let canvas = (max_id as i64) - 3;
    s.after(16.0, batch(&[format!("{{\"op\":\"attr\",\"id\":{canvas},\"name\":\"width\",\"value\":\"44\"}}")]));
    s.after(16.0, Step::Paint);
    s.after(16.0, Step::Trim);
    s.after(16.0, Step::Paint);
    s
}

/// More image sources than the table takes: the limit's violation once, the rest failing as images do.
fn image_limit() -> Script {
    let mut html = String::new();
    for i in 0..4110 {
        html.push_str(&format!("<img src=\"data:image/png;base64,QU{i}\">"));
    }
    let (mut s, _) = widget(&html, 300.0, 1.0, 0);
    s.after(
        16.0,
        batch(&[
            r#"{"op":"el","id":90000,"tag":"img","attrs":[["src","data:image/png;base64,late"]]}"#.to_string(),
            r#"{"op":"ins","parent":4,"id":90000,"before":null}"#.to_string(),
        ]),
    );
    s.after(16.0, Step::Paint);
    s
}

/// Every paint call the container turns into commands: texts spaced and decorated, list markers of every kind and
/// range, fills, gradients, borders of every style and corner, outlines, clips and clip paths, fixed boxes escaping
/// clips, shadows, groups and flips, filters with currentColor, backdrops, SVG paths, gradients and cached groups.
const DRAW_KINDS: &str = r##"
<style>
.t{font-size:14px} .ls{letter-spacing:2px} .ws{word-spacing:5px} .u{text-decoration:underline} .o{text-decoration:overline red}
.lt{text-decoration:line-through;color:blue} .all{text-decoration:underline overline line-through;text-decoration-color:green}
.b1{border:1px dashed red;border-radius:6px} .b2{border:4px dashed #00f;border-radius:10px} .b3{border:2px dotted red;border-radius:5px}
.b4{border:5px dotted green;border-radius:12px} .b5{border:6px double #333} .b6{border:2px double #333} .b7{border:3px dashed red}
.b8{border-left:2px dotted red;border-top:5px dotted blue;border-right:1px solid green;border-bottom:3px dashed black}
.b9{border:4px solid red;border-top-color:blue;border-radius:10px 4px 20px 0} .b10{border-left:6px dashed red;border-right:2px dotted blue;border-radius:12px}
.b11{border:3px solid rgba(0,0,0,0);border-bottom:3px solid red} .b12{border:1px solid red;border-radius:50%}
.ol{outline:2px dashed orange;outline-offset:3px}
.g1{background:linear-gradient(45deg,red,blue 40%,green)} .g2{background:radial-gradient(circle at 20% 30%,red,transparent)}
.g3{background:conic-gradient(from 30deg,red,yellow,blue)} .g4{background:repeating-linear-gradient(90deg,red 0 5px,blue 5px 10px)}
.sh{box-shadow:0 2px 4px rgba(0,0,0,.5),inset 0 0 3px 2px red} .sh0{box-shadow:0 0 0 3px #0f0} .sht{box-shadow:0 0 4px transparent}
.rot{transform:rotate(20deg)} .flip{transform:scaleX(-1);backface-visibility:hidden} .op{opacity:.4}
.flt{filter:blur(2px) drop-shadow(2px 3px 1px currentColor);color:purple} .hue{filter:hue-rotate(90deg) saturate(2) sepia(.3) invert(.2) grayscale(1) brightness(1.2) contrast(.8) opacity(.9)}
.bd{backdrop-filter:blur(4px)} .cp{clip-path:circle(40%)} .cp2{clip-path:polygon(0 0,100% 0,50% 100%)}
.clip{overflow:hidden;border-radius:10px;height:40px;border:2px solid #000} .fx{position:fixed;top:5px;left:5px;width:30px;height:10px;background:red}
</style>
<p class="t ls">spaced letters e&#x301; &#x1F468;&#x200D;&#x1F469;</p><p class="t ws">spaced&nbsp;words and more</p>
<p class="u">under</p><p class="o">over</p><p class="lt">through</p><p class="all ls">all three</p>
<ul><li>disc</li><li style="list-style:circle">circle</li><li style="list-style:square">square</li><li style="list-style:none">none</li></ul>
<ol start="-2"><li>a</li><li>b</li><li>c</li></ol><ol start="3998" style="list-style:upper-roman"><li>x</li><li>y</li><li>z</li></ol>
<ol style="list-style:lower-roman" start="0"><li>r</li></ol><ol style="list-style:lower-alpha" start="26"><li>z</li><li>aa</li></ol>
<ol style="list-style:upper-latin" start="0"><li>q</li></ol><ol style="list-style:decimal-leading-zero" start="8"><li>8</li><li>9</li><li>10</li></ol>
<ol style="list-style:georgian"><li>g</li></ol><ol start="2000000000" style="list-style:upper-alpha"><li>big</li></ol>
<details><summary>closed</summary>hidden</details><details open><summary>open</summary>shown</details>
<div class="b1">b1</div><div class="b2">b2</div><div class="b3">b3</div><div class="b4">b4</div><div class="b5">b5</div><div class="b6">b6</div>
<div class="b7">b7</div><div class="b8">b8</div><div class="b9">b9</div><div class="b10">b10</div><div class="b11">b11</div><div class="b12" style="width:30px;height:20px">b12</div>
<div class="ol">outline</div><div class="g1" style="height:20px"></div><div class="g2" style="height:20px"></div><div class="g3" style="height:20px"></div><div class="g4" style="height:20px"></div>
<div class="sh" style="height:20px;margin:6px"></div><div class="sh0" style="height:10px"></div><div class="sht" style="height:10px"></div>
<div class="rot" style="width:40px;height:20px;background:red">r</div><div class="flip" style="width:40px;height:20px;background:blue">f</div>
<div class="flip"><div class="flip" style="background:green">double flip</div></div><div class="op">faint<div class="op">fainter</div></div>
<div class="flt">filtered</div><div class="hue" style="background:red">hued</div><div class="bd" style="height:20px">backdrop</div>
<div class="cp" style="width:40px;height:40px;background:red"></div><div class="cp2" style="width:40px;height:40px;background:blue"></div>
<div class="clip"><div style="height:80px;background:linear-gradient(red,blue)">clipped</div><div class="fx">fixed</div></div>
<svg width="120" height="80" viewBox="0 0 120 80"><defs><linearGradient id="lg"><stop offset="0" stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient></defs>
<g><rect x="1" y="1" width="10" height="10" fill="url(#lg)"/><circle cx="20" cy="10" r="5" stroke="black" stroke-dasharray="2 1"/><ellipse cx="40" cy="10" rx="8" ry="4" fill="none" stroke="red" stroke-width="2"/>
<line x1="0" y1="20" x2="50" y2="25" stroke="green" stroke-linecap="round"/><polyline points="0,30 10,40 20,30" fill="none" stroke="blue" stroke-linejoin="bevel"/>
<polygon points="30,30 40,40 50,30" fill-rule="evenodd"/><path d="M60 10 Q70 0 80 10 T100 10 C100 20 110 20 115 30 A5 5 0 0 1 100 40 Z" fill="orange"/>
<rect x="60" y="50" width="20" height="10" rx="3" opacity=".5"/></g><text x="5" y="70">svg text</text></svg>
"##;

fn draw_kinds(width: f64, measuring: u8, dark: bool) -> Script {
    let (mut s, _) = widget(DRAW_KINDS, width, 2.0, measuring);
    s.dark = dark;
    s.after(16.0, Step::Dark(!dark));
    s.after(16.0, Step::Paint);
    s.after(16.0, Step::Width(width + 37.0));
    s.after(16.0, Step::Paint);
    s.after(16.0, Step::Compositing(false));
    s.after(16.0, Step::Paint);
    s
}

/// Blurred shadows, filters and translucent boxes past the paint limit: the stopped widget's look without effects (a
/// disabled button keeps its dimming, an invisible box stays hidden).
fn no_effects(policy: &str) -> Script {
    let mut html = String::from("<button disabled style=\"opacity:.5\">dim</button><div style=\"opacity:0\">gone</div>");
    for i in 0..120 {
        let look = ["box-shadow:0 0 6px #000", "filter:blur(1px)", "opacity:.5", "backdrop-filter:blur(2px)"][i % 4];
        html.push_str(&format!("<div style=\"{look};width:20px;height:6px;display:inline-block\">{i}</div>"));
    }
    let (ops, last) = html_to_ops(html.as_bytes(), 4);
    let mut s = Script::new(last as u32);
    s.after(0.0, Step::Message(policy.as_bytes().to_vec()));
    s.after(0.0, Step::Ops(document(&ops).into_bytes()));
    for _ in 0..4 {
        s.after(16.0, Step::Tick);
        s.after(0.0, Step::Paint);
    }
    s
}

/// Media features at scales and widths: resolution queries (which the fork reads inverted), widths, heights,
/// orientation, viewport lengths, the colour scheme; resizes, themes and the viewport query.
fn media(width: f64, dpr: f64) -> Script {
    let html = r#"<style>
.r{height:10px;background:red} @media (min-resolution:2dppx){.r{background:blue}} @media (min-resolution:150dpi){.r{height:20px}}
@media (resolution:96dpi){.r{width:50px}} @media (max-resolution:1x){.r{border:1px solid}} @media (min-width:200px){.w{height:30px}}
@media (max-width:1px){.w{height:1px}} @media (min-height:100px){.h{height:33px}} @media (orientation:portrait){.o{color:green}}
@media (prefers-color-scheme:dark){.d{background:#000}} .v{width:10vw;height:5vh;min-height:2vmin}
</style><div class="r w h o d v">media</div><div style="width:50vw">half</div>"#;
    let (mut s, _) = widget(html, width, dpr, 0);
    query(&mut s, "{\"q\":\"viewport\"}");
    query(&mut s, "{\"q\":\"rect\",\"id\":7}");
    query(&mut s, "{\"q\":\"cs\",\"id\":7}");
    s.after(16.0, Step::Width(width * 2.0 + 1.0));
    s.after(16.0, Step::Paint);
    s.after(16.0, Step::MaxHeight(900.0));
    s.after(16.0, Step::Dark(true));
    s.after(16.0, Step::Paint);
    query(&mut s, "{\"q\":\"rect\",\"id\":7}");
    s
}

#[test]
fn written_widgets() {
    let Some(oracle) = oracle() else { return };
    let mut scripts = vec![(fonts_past_4096(), "fonts past 4096".to_string()), (width_cache(), "the width cache".to_string())];
    for (w, m) in [(300.0, 1), (120.0, 1), (300.0, 0)] {
        scripts.push((breaker(w, m), format!("the breaker at {w} measuring {m}")));
    }
    scripts.push((images(), "images".to_string()));
    scripts.push((image_limit(), "the image table's bound".to_string()));
    for (w, m, dark) in [(320.0, 0, false), (250.0, 1, true), (412.0, 1, false)] {
        scripts.push((draw_kinds(w, m, dark), format!("every draw kind at {w}")));
    }
    for policy in ["{\"type\":\"policy\",\"policy\":\"strict\"}", "{\"type\":\"policy\",\"policy\":\"strict\",\"paintReports\":true}"] {
        scripts.push((no_effects(policy), format!("no effects under {policy}")));
    }
    for (w, dpr) in [(0.0, 1.0), (1.0, 0.25), (320.0, 2.0), (320.0, 64.0), (99.5, 1.5), (1048576.0, 3.0)] {
        scripts.push((media(w, dpr), format!("media at {w} × {dpr}")));
    }
    let out = compare_engines(&oracle, &scripts, "written widgets");
    let seen: Vec<String> = out
        .whole
        .0
        .iter()
        .filter(|(k, _)| k.starts_with("violation") || k.starts_with("message limit"))
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    eprintln!("written widgets reached: {}", seen.join(", "));
    assert!(out.whole.count("violation image limit") == 1, "the image table's bound is crossed once");
    assert!(out.whole.count("violation image refused") >= 4, "sources are refused");
    assert!(out.checked.count("fonts past 4096") > 0, "a document holds more than 4096 fonts");
}

// ---- Generated widgets ----------------------------------------------------------------------------------------

fn container_page(r: &mut Lcg) -> String {
    let fonts = [
        "font:12px serif",
        "font:bold 15px/1.2 sans-serif",
        "font-size:9.5px;font-style:italic",
        "font-size:0",
        "font-size:22px;font-weight:300",
        "font-family:'A B',monospace;font-size:13px",
        "font-size:13px;font-variant-numeric:tabular-nums",
    ];
    let spacing = ["", "letter-spacing:1px", "letter-spacing:-0.5px", "word-spacing:4px", "letter-spacing:.3em;word-spacing:2px"];
    let deco = [
        "",
        "text-decoration:underline",
        "text-decoration:overline line-through",
        "text-decoration:underline wavy red",
        "text-transform:uppercase",
        "text-transform:capitalize",
    ];
    let wraps = [
        "",
        "overflow-wrap:break-word",
        "word-break:break-all",
        "white-space:nowrap",
        "white-space:pre-wrap",
        "text-overflow:ellipsis;overflow:hidden;white-space:nowrap",
    ];
    let sides = ["solid", "dashed", "dotted", "double", "none", "groove"];
    let backs = [
        "",
        "background:#fc0",
        "background:linear-gradient(red,blue)",
        "background:radial-gradient(red,blue)",
        "background:conic-gradient(red,blue)",
        "background:url(data:image/png;base64,QUJD) repeat 0 0/7px 5px",
        "background:url(data:image/png;base64,async) no-repeat center",
        "background:url('data:image/svg+xml,x') repeat-x 0 0/9px 9px",
    ];
    let effects = [
        "",
        "box-shadow:1px 1px 3px #000",
        "opacity:.6",
        "transform:rotate(7deg)",
        "filter:drop-shadow(0 0 2px currentColor)",
        "overflow:hidden;border-radius:9px",
        "position:relative;top:3px",
        "clip-path:inset(2px round 4px)",
    ];
    let words = [
        "a",
        "words of text",
        "longwordlongwordlongword",
        "e\u{301}e\u{301}",
        "\u{1f600}\u{1f600}",
        "\u{6f22}\u{5b57}",
        "mixed text here and there",
    ];
    let mut html = String::new();
    for _ in 0..(3 + r.below(8)) {
        let tag = *pick(r, &["div", "p", "span", "li", "button", "h3"]);
        let w = pick(r, &["", "width:40px;", "width:90px;", "width:0;", "display:inline-block;width:25px;"]);
        let border = format!(
            "border-width:{}px {}px {}px {}px;border-style:{} {} {} {};border-color:red blue #0a0 rgba(0,0,0,.5);border-radius:{}px",
            r.below(6),
            r.below(6),
            r.below(6),
            r.below(6),
            pick(r, &sides),
            pick(r, &sides),
            pick(r, &sides),
            pick(r, &sides),
            pick(r, &[0usize, 0, 3, 8, 30])
        );
        let style = format!(
            "{w}{};{};{};{};{};{};{}",
            pick(r, &fonts),
            pick(r, &spacing),
            pick(r, &deco),
            pick(r, &wraps),
            if r.chance(2) { border } else { String::new() },
            pick(r, &backs),
            pick(r, &effects)
        );
        let text = (0..1 + r.below(4)).map(|_| *pick(r, &words)).collect::<Vec<_>>().join(" ");
        if tag == "li" {
            let list = pick(r, &["disc", "circle", "square", "decimal", "lower-roman", "upper-alpha", "decimal-leading-zero", "none"]);
            html.push_str(&format!(
                "<ol start=\"{}\" style=\"list-style:{list}\"><li style=\"{style}\">{text}</li></ol>",
                r.below(30) as i64 - 3
            ));
        } else {
            html.push_str(&format!("<{tag} style=\"{style}\">{text}</{tag}>"));
        }
    }
    if r.chance(3) {
        html.push_str(&format!(
            "<img src=\"{}\">",
            pick(r, &[PNG, "data:image/png;base64,async", "data:image/png;base64,fail", "data:image/svg+xml,s"])
        ));
    }
    if r.chance(3) {
        html.push_str("<svg width=\"60\" height=\"30\"><g><rect width=\"9\" height=\"9\" fill=\"red\"/><circle cx=\"20\" cy=\"5\" r=\"4\"/><rect x=\"30\" width=\"5\" height=\"5\"/><rect x=\"40\" width=\"5\" height=\"5\"/><rect x=\"50\" width=\"5\" height=\"5\"/><rect y=\"10\" width=\"5\" height=\"5\"/><rect y=\"20\" width=\"5\" height=\"5\"/><path d=\"M0 0 L9 9\" stroke=\"blue\"/></g></svg>");
    }
    html
}

fn container_script(r: &mut Lcg) -> Script {
    let html = container_page(r);
    let width = *pick(r, &[0.0, 37.0, 200.0, 320.0, 412.5]);
    let dpr = *pick(r, &[0.25, 1.0, 2.0, 3.0]);
    let (mut s, max_id) = widget(&html, width, dpr, u8::from(r.chance(2)));
    s.dark = r.chance(3);
    for _ in 0..(3 + r.below(6)) {
        let id = 5 + r.below(max_id.max(6) as usize - 4) as i64;
        match r.below(9) {
            0 => s.after(16.0, batch(&[style_op(id, "font-size", &format!("{}px", 8 + r.below(20)))])),
            1 => s.after(16.0, batch(&[style_op(id, "letter-spacing", &format!("{}px", r.below(4)))])),
            2 => s.after(16.0, batch(&[style_op(id, "border", &format!("{}px {} red", r.below(7), pick(r, &sides_all())))])),
            3 => s.after(16.0, Step::Width(*pick(r, &[100.0, 250.0, 330.0]))),
            4 => s.after(16.0, Step::ImageReady { id: 1 + r.below(4) as i32, w: 10.0, h: 7.0 }),
            5 => s.after(16.0, Step::Trim),
            6 => s.after(16.0, Step::Dark(r.chance(2))),
            7 => {
                query(&mut s, &format!("{{\"q\":\"box\",\"id\":{id}}}"));
                &mut s
            }
            _ => s.after(16.0, Step::Paint),
        };
    }
    s.after(16.0, Step::Paint);
    s
}

fn sides_all() -> [&'static str; 8] {
    ["solid", "dashed", "dotted", "double", "none", "hidden", "groove", "inset"]
}

#[test]
fn generated_widgets() {
    let Some(oracle) = oracle() else { return };
    let mut r = Lcg::new(0x4207);
    let scripts: Vec<(Script, String)> = (0..600).map(|i| (container_script(&mut r), format!("container widget {i}"))).collect();
    compare_engines(&oracle, &scripts, "generated widgets");
}
