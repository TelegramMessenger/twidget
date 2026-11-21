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
        "overflow-wrap:anywhere",
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
        match r.below(10) {
            9 => s.after(16.0, batch(&[style_op(id, "display", pick(r, &["none", "block", "flex", "inline-block"]))])),
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

// ---- A layout's words in batches (text_widths.cpp's `LayoutWidths`) ---------------------------------------------

/// Ops that put `<tag style>text</tag>` (ids `id` and `id + 1`) at the end of `parent`.
fn insert(parent: i64, id: i64, tag: &str, style: &str, text: &str) -> Vec<String> {
    vec![
        format!("{{\"op\":\"el\",\"id\":{id},\"tag\":{},\"attrs\":[[\"style\",{}]]}}", quote(tag), quote(style)),
        format!("{{\"op\":\"text\",\"id\":{},\"data\":{}}}", id + 1, quote(text)),
        format!("{{\"op\":\"ins\",\"parent\":{id},\"id\":{},\"before\":null}}", id + 1),
        format!("{{\"op\":\"ins\",\"parent\":{parent},\"id\":{id},\"before\":null}}"),
    ]
}

fn data_op(id: i64, text: &str) -> String {
    format!("{{\"op\":\"data\",\"id\":{id},\"data\":{}}}", quote(text))
}

/// `n` different words from `from`, each `pad` letters longer than its number.
fn words(prefix: &str, from: usize, n: usize, pad: usize) -> String {
    (from..from + n).map(|i| format!("{prefix}{i:05}{} ", "z".repeat(pad))).collect()
}

/// Owed words past what a face's cache keeps: a face asked more words than it has room for, one asked again with part
/// of its room taken, one whose words take the bytes every face shares, words longer than a key, a face the trim
/// emptied.
fn owed_past_room(measuring: u8) -> Script {
    let html = format!(
        "<p style=\"font-size:12px\">{}</p><p style=\"font-size:13px\">{}</p><p style=\"font-size:14px\">{}</p><p style=\"font-size:15px\">{} {} short words</p>",
        words("a", 0, 4200, 0),
        words("b", 0, 3000, 0),
        words("c", 0, 4200, 245),
        "L".repeat(300),
        "M".repeat(257)
    );
    let (mut s, _) = widget(&html, 320.0, 1.0, measuring);
    s.after(16.0, batch(&insert(4, 900, "p", "font-size:13px", &words("b", 3000, 2000, 0))));
    s.after(16.0, Step::Paint);
    s.after(16.0, Step::Width(250.0));
    s.after(16.0, Step::Trim);
    s.after(16.0, batch(&insert(4, 902, "p", "font-size:12px", &words("a", 100, 50, 0))));
    s.after(16.0, Step::Paint);
    s
}

/// Runs painted with letter or word spacing, unit by unit: a no-break space, runs of spaces, transforms, a negative
/// spacing, spacing of 0, marks and emoji, a spaced run that breaks, one under a display:none box; then the box shown
/// and the spacings changed.
fn spaced_runs(measuring: u8) -> Script {
    let html = "<p style=\"letter-spacing:2px\">Spaced CAPS</p>\
<p style=\"word-spacing:4px\">10\u{a0}000 apart  and   more</p>\
<div style=\"display:none\"><p style=\"letter-spacing:3px\">QXZ hidden</p></div>\
<p style=\"letter-spacing:1px;text-transform:uppercase\">shout it</p>\
<p style=\"letter-spacing:-0.5px;word-spacing:2px\">tight words</p>\
<pre style=\"word-spacing:3px\">a\tb  c\nd</pre>\
<p style=\"letter-spacing:2px;width:30px;overflow-wrap:anywhere\">spacedbreaking</p>\
<p style=\"word-spacing:0\">no units</p>\
<span style=\"letter-spacing:1px\">e\u{301}e\u{301}\u{1f600}x</span>";
    let (mut s, _) = widget(html, 320.0, 1.0, measuring);
    s.after(16.0, batch(&[style_op(9, "display", "block")]));
    s.after(16.0, Step::Paint);
    s.after(16.0, batch(&[style_op(5, "letter-spacing", "5px"), style_op(7, "word-spacing", "0")]));
    s.after(16.0, Step::Paint);
    s.after(16.0, batch(&insert(4, 900, "p", "letter-spacing:1px;word-spacing:1px", "fresh spaced letters")));
    s.after(16.0, Step::Paint);
    s
}

/// Texts a layout may break at their narrowest or in their line: flex items, table cells, a shrink-to-fit box, a
/// float, preserved tabs and line breaks broken anywhere, spaces alone, a word that may not wrap, break-all, a flex row
/// under a display:none box; then new items, a narrower view, the trim, new letters and the hidden row shown.
fn narrowest_breaks(measuring: u8) -> Script {
    let html = "<div style=\"display:flex\"><div>breakable</div><div>pieces</div></div>\
<p>plain fits</p>\
<div style=\"display:none\"><div style=\"display:flex\"><div>VWK</div></div></div>\
<table><tr><td>cellword</td><td>other cell</td></tr></table>\
<div style=\"display:inline-block\">shrink to fit</div>\
<div style=\"float:left\">floated words</div>\
<div style=\"display:flex\"><div style=\"white-space:pre-wrap;overflow-wrap:anywhere;font:17px serif\">ab\tcd</div></div>\
<div style=\"width:20px;overflow-wrap:anywhere;font:13px monospace\">abcdefgh</div>\
<div style=\"width:300px;white-space:pre-wrap;overflow-wrap:anywhere;font:19px serif\">ab\ncd</div>\
<div style=\"width:300px;white-space:pre-wrap;overflow-wrap:anywhere;font:17px serif\">a\t\t\t\tb</div>\
<div style=\"white-space:pre-wrap;overflow-wrap:anywhere;width:10px\">     </div>\
<div style=\"display:flex\"><div style=\"white-space:nowrap\">no wrap here</div></div>\
<div style=\"width:30px;word-break:break-all\">breakallwords</div>";
    let (mut s, _) = widget(html, 320.0, 1.0, measuring);
    s.after(16.0, batch(&insert(5, 900, "div", "", "QXZ")));
    s.after(16.0, Step::Paint);
    s.after(16.0, Step::Width(200.0));
    let mut row = vec![
        "{\"op\":\"el\",\"id\":910,\"tag\":\"div\",\"attrs\":[[\"style\",\"display:flex\"]]}".to_string(),
        "{\"op\":\"ins\",\"parent\":4,\"id\":910,\"before\":null}".to_string(),
    ];
    row.extend(insert(910, 911, "div", "", "JJ"));
    row.extend(insert(5, 913, "div", "", "YY"));
    s.after(16.0, batch(&row));
    s.after(16.0, Step::Trim);
    s.after(16.0, batch(&insert(4, 920, "div", "width:20px;overflow-wrap:anywhere;font:13px monospace", "mnopqrst")));
    s.after(16.0, batch(&[style_op(12, "display", "block")]));
    s.after(16.0, Step::Paint);
    s
}

/// Texts no box shows: under display:none boxes, nested ones, a list, a title, styles; a hidden text beside a shown
/// one; the boxes shown and hidden again.
fn hidden_texts(measuring: u8) -> Script {
    let html = "<div style=\"display:none\">hidden words here<span>nested hidden</span><p style=\"display:block\">still hidden</p></div>\
<p style=\"visibility:hidden\">invisible but shown</p>\
<style>.x{color:red}</style><title>a title of words</title>\
<div style=\"display:none\"><div style=\"display:none\">doubly hidden</div></div>\
<p>shown words</p>\
<ul style=\"display:none\"><li style=\"letter-spacing:2px\">hidden item</li></ul>";
    let (mut s, _) = widget(html, 320.0, 1.0, measuring);
    s.after(16.0, Step::Paint);
    s.after(16.0, batch(&[style_op(5, "display", "block")]));
    s.after(16.0, Step::Paint);
    s.after(16.0, batch(&[style_op(5, "display", "none"), style_op(12, "visibility", "visible")]));
    s.after(16.0, batch(&insert(5, 900, "p", "letter-spacing:1px", "new hidden words")));
    s.after(16.0, Step::Paint);
    s
}

/// A sole text patched in place, whose new words the document reads before any layout until a size differs, and a text
/// beside another.
fn patched_texts(measuring: u8) -> Script {
    let (mut s, _) = widget("<p>alpha beta</p><p>left <b>bold</b> right</p>", 320.0, 1.0, measuring);
    for text in ["gamma zeta", "epsilons iota", "one two three", "   ", "", "alpha beta"] {
        s.after(16.0, batch(&[data_op(6, text)]));
        s.after(16.0, Step::Paint);
    }
    s.after(16.0, batch(&[data_op(8, "far left "), data_op(11, " far right")]));
    s.after(16.0, Step::Paint);
    s
}

#[test]
fn written_batches() {
    let Some(oracle) = oracle() else { return };
    let mut scripts = Vec::new();
    for measuring in [0, 1] {
        scripts.push((owed_past_room(measuring), format!("owed words past the caches' room measuring {measuring}")));
        scripts.push((spaced_runs(measuring), format!("spaced runs measuring {measuring}")));
        scripts.push((narrowest_breaks(measuring), format!("narrowest breaks measuring {measuring}")));
        scripts.push((hidden_texts(measuring), format!("hidden texts measuring {measuring}")));
        scripts.push((patched_texts(measuring), format!("patched texts measuring {measuring}")));
    }
    let out = compare_engines(&oracle, &scripts, "written batches");
    assert!(out.checked.count("compared text_widths batches") > 0, "the container's batches are compared");
}

// ---- What the sets above leave: written to reach the container's rarer branches -------------------------------

const REACH_SHEET: &str = "@keyframes spin{from{transform:rotate(0deg)}to{transform:rotate(360deg)}} \
@keyframes glow{from{background-color:#000}to{background-color:#fff}} \
@keyframes shimmer{from{background-position:-100% 0}to{background-position:200% 0}} \
.s{animation:spin 1s linear infinite} .gl{animation:glow 1s infinite} \
.sk{background-color:#ddd;background-image:linear-gradient(90deg,#ddd,#fff,#ddd);background-size:50% 100%;background-repeat:no-repeat;animation:shimmer 1s linear infinite}";

/// Layers over clip paths, fixed boxes, groups and cached SVGs (the paint trace walks through them), a layer with a
/// negative z-index child, glowing backgrounds on controls and over backdrops, shimmers that cannot be tiles, hits on
/// a clip path.
fn layered_reach() -> Script {
    let mut html = format!("<style>{REACH_SHEET}</style>");
    html.push_str("<div class=\"s\" style=\"width:40px;height:40px;background:red;position:relative\"><div style=\"position:absolute;z-index:-1;width:20px;height:20px;background:blue\"></div><div style=\"position:relative;z-index:2\">z</div></div>");
    html.push_str("<div style=\"clip-path:circle(40%);width:60px;height:60px;background:green\">clipped</div>");
    html.push_str("<div style=\"overflow:hidden;height:30px\"><div style=\"position:fixed;top:0;left:200px;width:20px;height:20px;background:#00f\"><div style=\"overflow:hidden;width:10px\">in</div></div></div>");
    html.push_str("<div style=\"opacity:.5;transform:rotate(3deg);background:conic-gradient(red,blue)\">group</div>");
    html.push_str("<svg width=\"40\" height=\"40\"><g><rect width=\"4\" height=\"4\"/><rect x=\"5\" width=\"4\" height=\"4\"/><rect x=\"10\" width=\"4\" height=\"4\"/><rect x=\"15\" width=\"4\" height=\"4\"/><rect y=\"5\" width=\"4\" height=\"4\"/><rect y=\"10\" width=\"4\" height=\"4\"/><rect y=\"15\" width=\"4\" height=\"4\"/><circle cx=\"30\" cy=\"30\" r=\"5\"/></g></svg>");
    html.push_str("<button class=\"gl\"><span class=\"s\" style=\"display:inline-block\">x</span></button>");
    html.push_str(
        "<div class=\"gl\"><div style=\"backdrop-filter:blur(1px)\">b</div><div class=\"s\" style=\"width:9px;height:9px\"></div></div>",
    );
    html.push_str(
        "<div class=\"gl\"><div style=\"background-color:inherit\">i</div><div class=\"s\" style=\"width:9px;height:9px\"></div></div>",
    );
    html.push_str(
        "<div class=\"gl\" style=\"border:2px dashed red\"><div class=\"s\" style=\"width:9px;height:9px;border:1px solid\"></div></div>",
    );
    html.push_str("<svg width=\"0\" height=\"10\"><g><rect width=\"1\" height=\"1\"/><rect width=\"1\" height=\"1\"/><rect width=\"1\" height=\"1\"/><rect width=\"1\" height=\"1\"/><rect width=\"1\" height=\"1\"/><rect width=\"1\" height=\"1\"/><rect width=\"1\" height=\"1\"/><rect width=\"1\" height=\"1\"/></g></svg>");
    html.push_str("<span style=\"letter-spacing:-8px;text-decoration:underline\">x</span><div id=\"grow\" style=\"height:5px\"></div>");
    for look in [
        "background-repeat:repeat",
        "background-origin:content-box",
        "background-attachment:fixed",
        "background-size:0 0",
        "background-image:url(data:image/png;base64,QUJD)",
    ] {
        html.push_str(&format!("<div class=\"sk\" style=\"width:100px;height:10px;{look}\"></div>"));
    }
    html.push_str("<div style=\"opacity:.9\"><div class=\"sk\" style=\"width:100px;height:10px\"></div></div>");
    html.push_str("<div style=\"position:fixed;bottom:0\"><div class=\"sk\" style=\"width:100px;height:10px\"></div></div>");
    let (ops, last) = html_to_ops(html.as_bytes(), 4);
    let mut s = Script::new(last as u32);
    s.after(0.0, Step::Compositing(true));
    s.after(0.0, Step::Ops(document(&ops).into_bytes()));
    for _ in 0..3 {
        s.after(16.0, Step::Tick);
        s.after(0.0, Step::Paint);
    }
    for k in 0..12 {
        let (x, y) = (8.0 + f64::from(k % 4) * 15.0, 50.0 + f64::from(k / 4) * 15.0);
        s.after(16.0, Step::Hover { action: 1, x, y, modifiers: 0 });
        s.after(0.0, Step::Touch { action: 0, x, y, pointer: 1, primary: true, mouse: true, modifiers: 0, button: 0 });
        s.after(0.0, Step::Touch { action: 2, x, y, pointer: 1, primary: true, mouse: true, modifiers: 0, button: 0 });
        if k % 3 == 0 {
            // A layout between paints: the trace is taken again over cached SVG paints.
            s.after(0.0, batch(&[style_op(last, "height", &format!("{}px", 5 + k))]));
        }
        s.after(0.0, Step::Tick);
        s.after(0.0, Step::Paint);
    }
    s
}

/// More SVG paints than the container keeps: the oldest go.
fn svg_cache_bound() -> Script {
    let mut html = String::new();
    for i in 0..140 {
        html.push_str(&format!("<svg width=\"12\" height=\"12\"><g fill=\"#{:06x}\">", i * 997 % 0xffffff));
        for k in 0..8 {
            html.push_str(&format!("<rect x=\"{k}\" y=\"{}\" width=\"1\" height=\"1\"/>", i % 10));
        }
        html.push_str("</g></svg>");
    }
    let (mut s, _) = widget(&html, 320.0, 1.0, 0);
    s.after(16.0, Step::Paint);
    s.after(16.0, Step::Width(300.0));
    s.after(16.0, Step::Paint);
    s
}

/// One face whose 256-byte words hold the whole width pool: another face measures uncached.
fn width_pool_one_face() -> Script {
    // 4095 words and the space: the face's 4096 entries hold 1,048,321 of the 1,048,576 bytes.
    let mut html = String::from("<p style=\"font-size:12px\">");
    for i in 0..4095 {
        html.push_str(&format!("{i:06}{} ", "q".repeat(250)));
    }
    html.push_str("</p><p style=\"font-size:15px\">");
    for i in 0..3 {
        html.push_str(&format!("{i:06}{} ", "r".repeat(250)));
    }
    html.push_str("</p>");
    let (mut s, _) = widget(&html, 320.0, 1.0, 1);
    s.after(16.0, Step::Width(310.0));
    s.after(16.0, Step::Paint);
    s
}

/// Sources and units the sets above leave out, focus on controls hidden, right-aligned and clipped twice, a
/// textarea that is no inline context, percentage heights on the root and the body, pseudo-elements' computed
/// styles, offsets inside a fixed box.
fn odds_and_ends() -> Script {
    let html = r#"<style>html{height:100%} body{min-height:50%} p::before{content:"b";padding:2px} p::after{content:"a"}</style>
<img src="data:image/png;base64"><img src="tw-canvas: +5/10x10"><img src="tw-canvas:5-10x10"><img src="tw-canvas:5/10y10"><img src="tw-canvas:-1/2x3">
<div style="width:2in;height:12pt;margin:1pc 1cm 3mm;font-size:10pt">units</div>
<input id="a" style="visibility:hidden"><input id="b" style="text-align:right" value="right">
<div style="overflow:hidden;width:50px;height:30px;border-radius:15px"><div style="overflow:hidden;width:100px;height:60px"><input id="c" value="deep"></div></div>
<textarea style="display:flex">flex text</textarea><p>with pseudo</p>
<div style="position:fixed;top:10px"><div style="position:relative;left:3px"><span>in fixed</span></div></div>"#;
    let (mut s, max_id) = widget(html, 320.0, 1.0, 0);
    for id in 5..=max_id as i64 {
        // A script's focus takes the keyboard only just after the user's input.
        s.after(1500.0, Step::Touch { action: 0, x: 1.0, y: 1.0, pointer: 1, primary: true, mouse: true, modifiers: 0, button: 0 });
        s.after(0.0, Step::Touch { action: 2, x: 1.0, y: 1.0, pointer: 1, primary: true, mouse: true, modifiers: 0, button: 0 });
        s.after(10.0, batch(&[format!("{{\"op\":\"call\",\"id\":{id},\"method\":\"focus\",\"args\":[]}}")]));
        query(&mut s, &format!("{{\"q\":\"box\",\"id\":{id}}}"));
        query(&mut s, &format!("{{\"q\":\"cs\",\"id\":{id},\"pseudo\":\"::before\"}}"));
        query(&mut s, &format!("{{\"q\":\"cs\",\"id\":{id},\"pseudo\":\":after\"}}"));
    }
    s.after(16.0, Step::MaxHeight(1000.0));
    s.after(16.0, Step::Paint);
    s
}

/// Percentage heights on the root or the body: a new viewport height lays the document out again.
fn percent_heights(css: &str) -> Script {
    let html = format!("<style>{css}</style><div style=\"height:700px\">tall</div><div style=\"height:10%\">tenth</div>");
    let (mut s, _) = widget(&html, 320.0, 1.0, 0);
    s.after(16.0, Step::MaxHeight(1200.0));
    s.after(16.0, Step::Paint);
    s
}

#[test]
fn written_reach() {
    let Some(oracle) = oracle() else { return };
    let scripts = vec![
        (percent_heights("html{height:100%}"), "the root's percentage height".to_string()),
        (percent_heights("body{min-height:100%}"), "the body's percentage min-height".to_string()),
        (layered_reach(), "layers over the paint trace's corners".to_string()),
        (svg_cache_bound(), "the SVG paints' bound".to_string()),
        (width_pool_one_face(), "one face holding the width pool".to_string()),
        (odds_and_ends(), "odds and ends".to_string()),
    ];
    compare_engines(&oracle, &scripts, "written reach");
}
