//! Scripts aimed at one part of the input each: text selection (inside transforms, clips and layers), pointer
//! capture, and the editor.

use twidget_testkit::Lcg;

use super::inputs::{document, html_to_ops, pick, quote};
use super::{Script, Step};

fn some_id(r: &mut Lcg, max_id: u32) -> i64 {
    (2 + r.below(max_id.max(3) as usize - 1)) as i64
}

const PROSE: [&str; 8] = [
    "Hello world",
    "The quick brown fox jumps over the lazy dog again and again until the line wraps",
    "Ünïcödé — text with 漢字 and emoji 😀👍🏽 in it",
    "a<br>b<br><br>c",
    "<b>bold</b> and <i>italic</i> and <span class=\"s\">span</span>",
    "tab\tand  spaces   kept",
    "x",
    "nowrap nowrap nowrap nowrap nowrap nowrap nowrap nowrap nowrap nowrap nowrap",
];

const SELECTION_RULES: [&str; 12] = [
    "user-select:text",
    "user-select:none",
    "user-select:auto",
    "transform:translateX(50px)",
    "transform:rotate(90deg)",
    "transform:scale(2)",
    "overflow:hidden;width:40px",
    "animation:spin 2s linear infinite",
    "animation:slide 1s linear infinite alternate",
    "white-space:nowrap",
    "display:none",
    "letter-spacing:3px;word-spacing:4px",
];

/// A document of text to select: blocks under user-select text, none and auto, moved, rotated, scaled and clipped,
/// some animated (layers once compositing is on), and now and then a run too long to measure.
pub fn selection_html(r: &mut Lcg) -> String {
    let mut css = String::from(
        "@keyframes spin{from{transform:rotate(0)}to{transform:rotate(360deg)}} @keyframes slide{from{transform:translateX(0)}to{transform:translateX(40px)}} ",
    );
    css.push_str(pick(r, &["body{margin:0} ", "body{margin:8px} ", ""]));
    css.push_str(pick(r, &["p{margin:0;font:10px/20px sans-serif} ", "p{margin:4px 0} ", ""]));
    for (c, decl) in SELECTION_RULES.iter().enumerate() {
        css.push_str(&format!(".k{c}{{{decl}}} "));
    }
    if r.chance(2) {
        css.push_str("body{user-select:text} ");
    }
    let mut html = format!("<style>{css}</style>");
    for _ in 0..2 + r.below(6) {
        let classes: Vec<String> = (0..r.below(3)).map(|_| format!("k{}", r.below(12))).collect();
        let tag = *pick(r, &["p", "div", "span", "h1", "li"]);
        let mut text = pick(r, &PROSE).to_string();
        if r.chance(25) {
            text = "a".repeat(*pick(r, &[3000, 4097, 5000]));
        }
        if r.chance(3) {
            let inner = format!("k{}", r.below(12));
            text = format!("<span class=\"{inner}\">{text}</span> tail");
        }
        html.push_str(&format!("<{tag} class=\"{}\">{text}</{tag}>", classes.join(" ")));
    }
    html
}

/// Mouse drags over the text, the frames and paints between them, what a host reads of the selection, and presses that
/// drop it.
pub fn selection_gestures(r: &mut Lcg, s: &mut Script, n: usize) {
    let width = s.width;
    for _ in 0..n {
        let (x, y) = (r.below(width as usize) as f64 + 0.5, r.below(160) as f64);
        match r.below(8) {
            0..=3 => {
                let pointer = r.below(2) as i32;
                let mouse = !r.chance(6);
                s.after(*pick(r, &[16.0, 400.0]), Step::Touch { action: 0, x, y, pointer, primary: true, mouse, modifiers: 0, button: 0 });
                let (mut px, mut py) = (x, y);
                for _ in 0..1 + r.below(5) {
                    px += r.below(120) as f64 - 30.0;
                    py += *pick(r, &[0.0, 0.0, 6.0, 20.0, -10.0]);
                    s.after(16.0, Step::Touch { action: 1, x: px, y: py, pointer, primary: true, mouse, modifiers: 0, button: 0 });
                    if r.chance(3) {
                        s.after(0.0, Step::Paint);
                    }
                    if r.chance(4) {
                        s.after(0.0, Step::Observe);
                    }
                }
                s.after(
                    16.0,
                    Step::Touch {
                        action: if r.chance(8) { 3 } else { 2 },
                        x: px,
                        y: py,
                        pointer,
                        primary: true,
                        mouse,
                        modifiers: 0,
                        button: 0,
                    },
                );
                s.after(0.0, Step::Observe);
                if r.chance(2) {
                    s.after(16.0, Step::Paint);
                }
            }
            4 => {
                s.after(16.0, Step::ClearSelection);
            }
            5 => {
                s.after(*pick(r, &[16.0, 250.0, 700.0]), Step::Tick);
                s.after(0.0, Step::Paint);
                s.after(0.0, Step::Observe);
            }
            6 => {
                let id = some_id(r, s.max_id);
                match r.below(4) {
                    0 => s.after(
                        16.0,
                        Step::Ops(
                            format!("[{{\"op\":\"attr\",\"id\":{id},\"name\":\"class\",\"value\":\"k{}\"}}]", r.below(12)).into_bytes(),
                        ),
                    ),
                    1 => s.after(
                        16.0,
                        Step::Ops(
                            format!(
                                "[{{\"op\":\"style\",\"id\":{id},\"name\":\"user-select\",\"value\":{}}}]",
                                quote(pick(r, &["text", "none", "auto", ""]))
                            )
                            .into_bytes(),
                        ),
                    ),
                    2 => s.after(16.0, Step::Ops(format!("[{{\"op\":\"rm\",\"id\":{id}}}]").into_bytes())),
                    _ => {
                        s.after(16.0, Step::Message(format!("{{\"type\":\"preventDefault\",\"keys\":[\"{id}:mousedown\"]}}").into_bytes()))
                    }
                };
            }
            _ => {
                s.after(16.0, Step::Hover { action: 0, x, y, modifiers: 0 });
            }
        }
    }
    s.after(16.0, Step::Paint);
    s.after(0.0, Step::Observe);
}

/// Presses captured by script: setPointerCapture and releasePointerCapture on the pressed element or another, by the
/// pointer id the widget was given or another, the captured element removed or hidden, moves, releases and cancels.
pub fn capture_gestures(r: &mut Lcg, s: &mut Script, n: usize) {
    let width = s.width;
    for _ in 0..n {
        let (x, y) = (r.below(width as usize) as f64, r.below(200) as f64);
        let pointer = r.below(3) as i32;
        let mouse = r.chance(2);
        let id_given = if mouse { 1 } else { pointer + 2 };
        s.after(*pick(r, &[16.0, 350.0]), Step::Touch { action: 0, x, y, pointer, primary: !r.chance(5), mouse, modifiers: 0, button: 0 });
        let (mut px, mut py) = (x, y);
        for _ in 0..1 + r.below(5) {
            match r.below(6) {
                0 | 1 => {
                    let id = if r.chance(2) { some_id(r, s.max_id) } else { 2 + r.below(8) as i64 };
                    let given = *pick(r, &[id_given, id_given, 1, 2, 3, 7]);
                    let method = if r.chance(4) { "releasePointerCapture" } else { "setPointerCapture" };
                    s.after(
                        8.0,
                        Step::Ops(format!("[{{\"op\":\"call\",\"id\":{id},\"method\":\"{method}\",\"args\":[{given}]}}]").into_bytes()),
                    );
                }
                2 => {
                    let id = some_id(r, s.max_id);
                    let op = if r.chance(2) {
                        format!("{{\"op\":\"rm\",\"id\":{id}}}")
                    } else {
                        format!("{{\"op\":\"style\",\"id\":{id},\"name\":\"display\",\"value\":\"none\"}}")
                    };
                    s.after(8.0, Step::Ops(format!("[{op}]").into_bytes()));
                }
                3 => {
                    s.after(16.0, Step::Tick);
                }
                _ => {
                    px += r.below(60) as f64 - 20.0;
                    py += r.below(30) as f64 - 10.0;
                    s.after(16.0, Step::Touch { action: 1, x: px, y: py, pointer, primary: true, mouse, modifiers: 0, button: 0 });
                }
            }
        }
        s.after(
            16.0,
            Step::Touch { action: if r.chance(4) { 3 } else { 2 }, x: px, y: py, pointer, primary: true, mouse, modifiers: 0, button: 0 },
        );
        if r.chance(2) {
            s.after(16.0, Step::Tick);
        }
    }
}

/// A document of text controls: inputs of every type, disabled, read-only, with placeholders and limits, textareas,
/// controls inside labels (a tap on their inner text) and inside a clip, styled.
pub fn editor_html(r: &mut Lcg) -> String {
    let mut html = String::from(
        "<style>body{margin:0} .w{overflow:hidden;width:120px;height:30px} input,textarea{width:150px} .big{font:20px serif;text-align:center} .r{text-align:right;color:#f00}</style>",
    );
    for _ in 0..2 + r.below(5) {
        let class = *pick(r, &["", " class=\"big\"", " class=\"r\""]);
        let control = match r.below(5) {
            0 | 1 => {
                let kind = *pick(r, &["", "text", "search", "number", "email", "url", "tel", " Text ", "checkbox", "password", "date"]);
                let mut attrs = if kind.is_empty() { String::new() } else { format!(" type=\"{kind}\"") };
                for (chance, a) in [
                    (2, " value=\"ab\""),
                    (2, " placeholder=\"Name\""),
                    (6, " disabled"),
                    (5, " readonly"),
                    (4, " maxlength=\"5\""),
                    (8, " maxlength=\"x\""),
                ] {
                    if r.chance(chance) {
                        attrs.push_str(a);
                    }
                }
                format!("<input{class}{attrs}>")
            }
            2 => format!("<textarea{class}{}>line one\nline two</textarea>", if r.chance(2) { " placeholder=\"Say\"" } else { "" }),
            3 => format!("<label{class}>Name <input value=\"in a label\"></label>"),
            _ => format!("<div class=\"w\"><input{class} value=\"clipped\"></div>"),
        };
        html.push_str(&format!("<div>{control}</div>"));
    }
    html
}

/// Taps on the controls and off them, the platform's edits (current and stale), a script's focus inside and past its
/// second, select and setSelectionRange, values set while editing, the edited control restyled and removed, and the
/// editor let go.
pub fn editor_gestures(r: &mut Lcg, s: &mut Script, n: usize) {
    let width = s.width;
    for _ in 0..n {
        let (x, y) = (r.below(width as usize) as f64, r.below(200) as f64);
        let id = some_id(r, s.max_id);
        match r.below(10) {
            0..=2 => {
                let mouse = r.chance(2);
                s.after(
                    *pick(r, &[16.0, 300.0, 1500.0]),
                    Step::Touch { action: 0, x, y, pointer: 0, primary: true, mouse, modifiers: 0, button: 0 },
                );
                s.after(60.0, Step::Touch { action: 2, x, y, pointer: 0, primary: true, mouse, modifiers: 0, button: 0 });
            }
            3 | 4 => {
                let text = pick(r, &["abc", "", "héllo", "1\n2", "😀😀", "x"]).as_bytes().to_vec();
                s.after(16.0, Step::Edit { current: !r.chance(5), generation: r.below(4) as u64, text, kind: b"insertText".to_vec() });
            }
            5 => {
                s.after(
                    *pick(r, &[100.0, 900.0, 1100.0]),
                    Step::Ops(format!("[{{\"op\":\"call\",\"id\":{id},\"method\":\"focus\",\"args\":[]}}]").into_bytes()),
                );
            }
            6 => {
                let (a, b) = (r.below(8) as i64 - 2, r.below(8) as i64 - 2);
                let method = *pick(r, &["select", "setSelectionRange", "blur"]);
                s.after(
                    16.0,
                    Step::Ops(format!("[{{\"op\":\"call\",\"id\":{id},\"method\":\"{method}\",\"args\":[{a},{b}]}}]").into_bytes()),
                );
            }
            7 => {
                let op = match r.below(4) {
                    0 => format!(
                        "{{\"op\":\"prop\",\"id\":{id},\"name\":\"value\",\"value\":{}}}",
                        pick(r, &["\"new\"", "null", "12", "\"ab\""])
                    ),
                    1 => format!("{{\"op\":\"style\",\"id\":{id},\"name\":\"width\",\"value\":\"{}px\"}}", 50 + r.below(100)),
                    2 => format!("{{\"op\":\"rm\",\"id\":{id}}}"),
                    _ => format!("{{\"op\":\"attr\",\"id\":{id},\"name\":\"class\",\"value\":\"big\"}}"),
                };
                s.after(16.0, Step::Ops(format!("[{op}]").into_bytes()));
            }
            8 => {
                s.after(16.0, Step::EndEditing);
            }
            _ => {
                let step = match r.below(4) {
                    0 => Step::Tick,
                    1 => Step::LastHtml,
                    2 => Step::Paint,
                    _ => Step::Observe,
                };
                s.after(16.0, step);
            }
        }
    }
}

/// A script of `gestures` over the document `html` makes: compositing on or off, every event listened for, maybe the
/// client's arbitration, a first paint.
pub fn targeted(
    seeds: std::ops::Range<u64>,
    html: fn(&mut Lcg) -> String,
    gestures: fn(&mut Lcg, &mut Script, usize),
    n: usize,
    what: &str,
) -> Vec<(Script, String)> {
    let mut scripts = Vec::new();
    for seed in seeds {
        let mut r = Lcg::new(seed);
        let page = html(&mut r);
        let (ops, last) = html_to_ops(page.as_bytes(), 10);
        let mut s = Script::new(last as u32);
        s.width = *pick(&mut r, &[300.0, 240.0]);
        if r.chance(2) {
            s.after(0.0, Step::Compositing(true));
        }
        s.after(0.0, Step::Ops(document(&ops).into_bytes()));
        let all = twidget_events::PROFILE_EVENTS.iter().map(|t| quote(&String::from_utf8_lossy(t))).collect::<Vec<_>>().join(",");
        s.after(0.0, Step::Message(format!("{{\"type\":\"listen\",\"types\":[{all}]}}").into_bytes()));
        if r.chance(3) {
            s.after(
                0.0,
                Step::Message(
                    format!("{{\"type\":\"arbitrate\",\"bubble\":{},\"touch\":{},\"rtl\":{}}}", r.chance(2), r.chance(2), r.chance(4))
                        .into_bytes(),
                ),
            );
        }
        s.after(16.0, Step::Paint);
        gestures(&mut r, &mut s, n);
        scripts.push((s, format!("{what} {seed}")));
    }
    scripts
}
