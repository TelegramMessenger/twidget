//! The scripts the comparison runs: `.twrec` sessions with their input, widget-sandbox's corpus widgets as
//! validator-like ops, generated documents, and seeded gestures, taps, drags, holds, hovers, edits and selections
//! over them.

use twidget_json::Json;
use twidget_testkit::Lcg;

use super::{Script, Step};

pub fn pick<'a, T>(r: &mut Lcg, items: &'a [T]) -> &'a T {
    &items[r.below(items.len())]
}

pub fn quote(s: &str) -> String {
    String::from_utf8_lossy(&twidget_json::quote(s.as_bytes())).into_owned()
}

// ---- Recordings ---------------------------------------------------------------------------------------------

struct Reader<'a> {
    s: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let b = *self.s.get(self.at)?;
        self.at += 1;
        Some(b)
    }
    fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }
    fn bytes(&mut self) -> Option<&[u8]> {
        let n = usize::try_from(self.varint()?).ok()?;
        let out = self.s.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(out)
    }
    fn f64(&mut self) -> Option<f64> {
        let b = self.s.get(self.at..self.at + 8)?;
        self.at += 8;
        Some(f64::from_le_bytes(b.try_into().ok()?))
    }
    fn json(&mut self, depth: usize) -> Option<Json> {
        if depth > 1024 {
            return None;
        }
        Some(match self.byte()? {
            0 => Json::Null,
            1 => Json::Bool(false),
            2 => Json::Bool(true),
            3 => Json::Number(self.f64()?),
            4 => Json::String(self.bytes()?.to_vec()),
            5 => {
                let n = self.varint()?;
                let mut items = Vec::new();
                for _ in 0..n {
                    items.push(self.json(depth + 1)?);
                }
                Json::Array(items)
            }
            6 => {
                let n = self.varint()?;
                let mut members = Vec::new();
                for _ in 0..n {
                    let key = self.bytes()?.to_vec();
                    members.push((key, self.json(depth + 1)?));
                }
                Json::Object(members)
            }
            _ => return None,
        })
    }
}

/// The largest node id an ops array names, for the tree digest (bounded).
fn max_id_of(ops: &Json) -> u32 {
    let mut m = 0.0f64;
    for op in ops.arr() {
        for k in ["id", "parent"] {
            let v = op.get(k).as_number(0.0);
            if v > m {
                m = v;
            }
        }
    }
    m.min(60000.0) as u32
}

/// Every session of a `.twrec` recording (tests/recording.h) as a script: every message, frame, pointer, hover, claim,
/// edit and selection call in its order, at the clock it first read.
pub fn recorded_sessions(bytes: &[u8]) -> Vec<Script> {
    let mut r = Reader { s: bytes, at: 0 };
    assert!(bytes.starts_with(b"TWREC") && bytes.get(5) == Some(&1), "a version 1 recording");
    r.at = 6;
    r.bytes().expect("the origin");
    let mut sessions: Vec<(u64, Script, u64)> = Vec::new();
    while r.at < bytes.len() {
        let n = usize::try_from(r.varint().expect("a frame")).expect("a frame length");
        let frame = &bytes[r.at..r.at + n];
        r.at += n;
        let mut f = Reader { s: frame, at: 0 };
        let kind = f.byte().expect("a kind");
        if kind == 0 {
            break;
        }
        let session = f.varint().expect("a session");
        f.bytes().expect("a label");
        f.varint().expect("names");
        f.varint().expect("name bytes");
        let numbers: Vec<f64> = (0..f.varint().expect("numbers")).map(|_| f.f64().expect("a number")).collect();
        let texts: Vec<Vec<u8>> = (0..f.varint().expect("texts")).map(|_| f.bytes().expect("a text").to_vec()).collect();
        let message = if f.byte().expect("a flag") == 1 { f.json(0) } else { None };
        let reads = f.varint().expect("clock reads");
        let first = if reads > 0 { f.varint() } else { None };
        if kind == 1 {
            let mut s = Script::new(0);
            s.width = numbers[0];
            s.dpr = numbers[1];
            s.dark = numbers[2] != 0.0;
            s.t0 = 1000.0;
            sessions.push((session, s, first.unwrap_or(0)));
            continue;
        }
        let Some((_, script, clock)) = sessions.iter_mut().rev().find(|(s, _, _)| *s == session) else { continue };
        if let Some(delta) = first {
            *clock += delta;
        }
        let t = (*clock as f64 / 1000.0).max(script.now());
        let n = |i: usize| numbers.get(i).copied().unwrap_or(0.0);
        let text = |i: usize| texts.get(i).cloned().unwrap_or_default();
        let step = match kind {
            3 => {
                let Some(m) = message else { continue };
                if m.get("type").as_str() == b"ops" {
                    script.max_id = script.max_id.max(max_id_of(m.get("ops")));
                }
                Step::Message(m.dump())
            }
            4 => {
                if n(0) != 0.0 {
                    script.steps.push((t, Step::Tick));
                }
                if n(1) == 0.0 {
                    continue;
                }
                Step::Paint
            }
            5 => Step::Touch {
                action: n(0) as i32,
                x: n(1),
                y: n(2),
                pointer: n(3) as i32,
                primary: n(4) != 0.0,
                mouse: n(5) != 0.0,
                modifiers: n(6) as i32,
                button: n(7) as i32,
            },
            6 => Step::Hover { action: n(0) as i32, x: n(1), y: n(2), modifiers: n(3) as i32 },
            7 => Step::Width(n(0)),
            8 => Step::MaxHeight(n(0)),
            9 => Step::Dark(n(0) != 0.0),
            10 => Step::ClearSelection,
            11 => Step::Claim { x: n(0), y: n(1) },
            13 => Step::Wheel { x: n(0), y: n(1), dx: n(2), dy: n(3), mode: n(4) as i32, modifiers: n(5) as i32 },
            14 => Step::Key { kind: text(0), key: text(1), code: text(2), repeat: n(0) != 0.0, modifiers: n(1) as i32 },
            15 => Step::Edit { current: false, generation: n(0) as u64, text: text(0), kind: text(1) },
            16 => Step::EndEditing,
            17 => Step::Compositing(n(0) != 0.0),
            18 => Step::ImageReady { id: n(0) as i32, w: n(1) as f32, h: n(2) as f32 },
            23 => Step::LastHtml,
            _ => continue,
        };
        script.steps.push((t, step));
    }
    sessions.into_iter().map(|(_, s, _)| s).collect()
}

// ---- The corpus -------------------------------------------------------------------------------------------

pub fn widget_sandbox() -> std::path::PathBuf {
    std::env::var_os("TWIDGET_WIDGET_SANDBOX")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../widget-sandbox"))
}

fn html_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            html_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "html") {
            out.push(path);
        }
    }
}

/// The corpus widgets of widget-sandbox under `dirs` (relative to its root), by path.
pub fn corpus_files(dirs: &[&str]) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for dir in dirs {
        html_files(&widget_sandbox().join(dir), &mut files);
    }
    files.sort();
    files
}

fn decode_entities(s: &[u8]) -> Vec<u8> {
    let named: [(&[u8], &[u8]); 6] =
        [(b"&amp;", b"&"), (b"&lt;", b"<"), (b"&gt;", b">"), (b"&quot;", b"\""), (b"&#39;", b"'"), (b"&nbsp;", "\u{a0}".as_bytes())];
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'&'
            && let Some((k, v)) = named.iter().find(|(k, _)| s[i..].starts_with(k))
        {
            out.extend_from_slice(v);
            i += k.len();
            continue;
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

fn find_ci(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    (from..=hay.len().checked_sub(needle.len())?).find(|&i| hay[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

const VOID: [&[u8]; 14] =
    [b"area", b"base", b"br", b"col", b"embed", b"hr", b"img", b"input", b"link", b"meta", b"param", b"source", b"track", b"wbr"];

fn text_op(ops: &mut Vec<String>, id: i64, parent: i64, data: &[u8]) {
    ops.push(format!("{{\"op\":\"text\",\"id\":{id},\"data\":{}}}", quote(&String::from_utf8_lossy(data))));
    ops.push(format!("{{\"op\":\"ins\",\"parent\":{parent},\"id\":{id},\"before\":null}}"));
}

type Attributes = Vec<(Vec<u8>, Vec<u8>)>;

fn tag_attributes(html: &[u8], mut j: usize) -> (Attributes, usize) {
    let mut attrs = Vec::new();
    loop {
        while j < html.len() && html[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= html.len() || html[j] == b'>' || html[j..].starts_with(b"/>") {
            return (attrs, j);
        }
        let start = j;
        while j < html.len() && !html[j].is_ascii_whitespace() && html[j] != b'=' && html[j] != b'>' && !html[j..].starts_with(b"/>") {
            j += 1;
        }
        let name = html[start..j].to_vec();
        if name.is_empty() {
            j += 1;
            continue;
        }
        let mut value = Vec::new();
        if j < html.len() && html[j] == b'=' {
            j += 1;
            if j < html.len() && (html[j] == b'"' || html[j] == b'\'') {
                let q = html[j];
                let e = html[j + 1..].iter().position(|&c| c == q).map_or(html.len(), |e| j + 1 + e);
                value = decode_entities(&html[j + 1..e]);
                j = (e + 1).min(html.len());
            } else {
                let s = j;
                while j < html.len() && !html[j].is_ascii_whitespace() && html[j] != b'>' {
                    j += 1;
                }
                value = decode_entities(&html[s..j]);
            }
        }
        attrs.push((name, value));
    }
}

/// A widget's static HTML as the ops the sandbox's validator sends for it, roughly (as stage 4.1's comparison has it):
/// its elements with their attributes, its texts and its style sheets under the adopted roots, scripts left out.
/// Animations and transitions are kept: they run, and the pointer starts some. Returns the ops and the last id.
pub fn html_to_ops(source: &[u8], first_id: i64) -> (Vec<String>, i64) {
    let html = source.to_vec();
    let mut ops = Vec::new();
    let mut next = first_id;
    let mut stack: Vec<(Vec<u8>, i64, bool)> = vec![(b"body".to_vec(), 4, false)];
    let mut i = 0;
    while i < html.len() {
        let rest = &html[i..];
        if rest.starts_with(b"<!--") {
            i = find_ci(&html, b"-->", i + 4).map_or(html.len(), |e| e + 3);
            continue;
        }
        if rest.len() > 1 && rest[0] == b'<' && (rest[1] == b'!' || rest[1] == b'?') {
            i = rest.iter().position(|&c| c == b'>').map_or(html.len(), |e| i + e + 1);
            continue;
        }
        if rest.starts_with(b"</") {
            let end = rest.iter().position(|&c| c == b'>').map_or(html.len(), |e| i + e);
            let name = html[i + 2..end].trim_ascii().to_ascii_lowercase();
            if let Some(at) = stack.iter().rposition(|(n, _, _)| *n == name)
                && at > 0
            {
                stack.truncate(at);
            }
            i = (end + 1).min(html.len());
            continue;
        }
        if rest.len() > 1 && rest[0] == b'<' && rest[1].is_ascii_alphabetic() {
            let mut j = i + 1;
            while j < html.len() && (html[j].is_ascii_alphanumeric() || html[j] == b'-') {
                j += 1;
            }
            let tag = html[i + 1..j].to_ascii_lowercase();
            let (attrs, j) = tag_attributes(&html, j);
            let closed = html[j..].starts_with(b"/>");
            i = html[j..].iter().position(|&c| c == b'>').map_or(html.len(), |e| j + e + 1);
            if matches!(tag.as_slice(), b"html" | b"head" | b"body") {
                continue;
            }
            if tag == b"script" {
                i = find_ci(&html, b"</script", i).unwrap_or(html.len());
                continue;
            }
            let svg = tag == b"svg" || stack.last().is_some_and(|s| s.2);
            next += 1;
            let id = next;
            let list: Vec<String> = attrs
                .iter()
                .map(|(k, v)| format!("[{},{}]", quote(&String::from_utf8_lossy(k)), quote(&String::from_utf8_lossy(v))))
                .collect();
            let ns = if svg { ",\"ns\":\"svg\"" } else { "" };
            ops.push(format!(
                "{{\"op\":\"el\",\"id\":{id},\"tag\":{}{ns},\"attrs\":[{}]}}",
                quote(&String::from_utf8_lossy(&tag)),
                list.join(",")
            ));
            let head = matches!(tag.as_slice(), b"style" | b"meta" | b"title" | b"link");
            let parent = if head { 3 } else { stack.last().map_or(4, |s| s.1) };
            ops.push(format!("{{\"op\":\"ins\",\"parent\":{parent},\"id\":{id},\"before\":null}}"));
            if matches!(tag.as_slice(), b"style" | b"textarea" | b"title") {
                let close = [&b"</"[..], &tag].concat();
                let end = find_ci(&html, &close, i).unwrap_or(html.len());
                let body = &html[i..end];
                if tag == b"style" {
                    ops.push(format!("{{\"op\":\"css\",\"id\":{id},\"text\":{}}}", quote(&String::from_utf8_lossy(body))));
                } else if !body.is_empty() {
                    next += 1;
                    text_op(&mut ops, next, id, &decode_entities(body));
                }
                i = html[end..].iter().position(|&c| c == b'>').map_or(html.len(), |e| end + e + 1);
                continue;
            }
            if !closed && !VOID.contains(&tag.as_slice()) {
                stack.push((tag, id, svg));
            }
            continue;
        }
        let end = html[i + 1..].iter().position(|&c| c == b'<').map_or(html.len(), |e| i + 1 + e);
        let data = decode_entities(&html[i..end]);
        if !data.trim_ascii().is_empty() || data.contains(&b' ') {
            next += 1;
            text_op(&mut ops, next, stack.last().map_or(4, |s| s.1), &data);
        }
        i = end;
    }
    (ops, next)
}

/// The adopted roots alone.
pub const ROOTS: &str = r#"{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[]}"#;

/// A document's ops under the roots, as one batch.
pub fn document(ops: &[String]) -> String {
    let mut s = String::from("[");
    s.push_str(ROOTS);
    for op in ops {
        s.push(',');
        s.push_str(op);
    }
    s.push(']');
    s
}

// ---- Generated documents ------------------------------------------------------------------------------------

const RULES: [&str; 46] = [
    "user-select:text",
    "user-select:none",
    "user-select:auto",
    "user-select:all",
    "touch-action:none",
    "touch-action:pan-y",
    "touch-action:pan-x",
    "touch-action:manipulation",
    "touch-action:pan-left pinch-zoom",
    "touch-action:auto",
    "pointer-events:none",
    "pointer-events:auto",
    "cursor:pointer",
    "cursor:text",
    "cursor:GRAB",
    "display:none",
    "display:block",
    "display:inline-block",
    "display:flex",
    "width:120px",
    "width:40%",
    "height:30px",
    "min-height:24px",
    "padding:6px",
    "margin:4px 10px",
    "font:10px/20px sans-serif",
    "font-size:18px",
    "letter-spacing:2px",
    "word-spacing:5px",
    "white-space:nowrap",
    "white-space:pre",
    "transform:translateX(30px)",
    "transform:rotate(12deg)",
    "transform:scale(1.5)",
    "transform:scale(0)",
    "overflow:hidden",
    "position:absolute;left:20px;top:40px",
    "position:relative;top:-6px",
    "opacity:.5",
    "transition:opacity .2s linear",
    "animation:spin 1s linear infinite",
    "animation:fade .5s ease 2",
    "float:left",
    "text-align:center",
    "visibility:hidden",
    "direction:rtl",
];

const TEXTS: [&str; 12] = [
    "Hello world",
    "Second line of text that wraps around the box when it is narrow enough",
    "short",
    "ünïcödé текст 漢字",
    "a  b\tc",
    "x",
    "😀 emoji 👍🏽",
    "word word word word word word word word word word word word",
    " ",
    "longunbrokenrunlongunbrokenrunlongunbrokenrunlongunbrokenrun",
    "Tap me",
    "1234567890",
];

fn rule(r: &mut Lcg) -> String {
    let n = 1 + r.below(3);
    (0..n).map(|_| *pick(r, &RULES)).collect::<Vec<_>>().join(";")
}

/// A generated document as HTML: a style sheet of the rules above on classes, :hover and :active, and nested boxes,
/// texts, line breaks and the controls the input meets (text fields, textareas, details, images, buttons).
pub fn generated_html(r: &mut Lcg) -> String {
    let mut css = String::from(
        "@keyframes spin{from{transform:rotate(0)}to{transform:rotate(360deg)}} @keyframes fade{from{opacity:1}to{opacity:.2}} ",
    );
    if r.chance(2) {
        css.push_str("body{margin:0} ");
    }
    for c in 0..8 {
        css.push_str(&format!(".c{c}{{{}}} ", rule(r)));
        if r.chance(3) {
            css.push_str(&format!(".c{c}:hover{{{}}} ", rule(r)));
        }
        if r.chance(4) {
            css.push_str(&format!(".c{c}:active{{{}}} ", rule(r)));
        }
    }
    if r.chance(2) {
        css.push_str("p{user-select:text} ");
    }
    let mut html = format!("<style>{css}</style>");
    let blocks = 2 + r.below(8);
    for _ in 0..blocks {
        node(r, &mut html, 0);
    }
    html
}

fn class(r: &mut Lcg) -> String {
    if r.chance(4) {
        return String::new();
    }
    let mut c = format!(" class=\"c{}", r.below(8));
    if r.chance(3) {
        c.push_str(&format!(" c{}", r.below(8)));
    }
    c.push('"');
    if r.chance(5) {
        c.push_str(&format!(" style=\"{}\"", rule(r)));
    }
    c
}

fn node(r: &mut Lcg, html: &mut String, depth: usize) {
    let cls = class(r);
    match r.below(if depth > 3 { 6 } else { 14 }) {
        0 | 1 => html.push_str(pick(r, &TEXTS)),
        2 => html.push_str("<br>"),
        3 => {
            let kind = *pick(r, &["", "text", "search", "number", "checkbox", "radio", "email", "TEL", "date", "hidden", "range", "url"]);
            let mut attrs = String::new();
            if !kind.is_empty() {
                attrs.push_str(&format!(" type=\"{kind}\""));
            }
            for (chance, a) in [
                (2, " value=\"abc\""),
                (3, " placeholder=\"Name\""),
                (5, " disabled"),
                (5, " readonly"),
                (4, " maxlength=\"5\""),
                (6, " checked"),
            ] {
                if r.chance(chance) {
                    attrs.push_str(a);
                }
            }
            html.push_str(&format!("<input{cls}{attrs}>"));
        }
        4 => html.push_str(&format!(
            "<textarea{cls}{}>{}</textarea>",
            if r.chance(3) { " placeholder=\"Say\"" } else { "" },
            pick(r, &TEXTS)
        )),
        5 => {
            let src = *pick(
                r,
                &[
                    "data:image/png;base64,iVBORw0KGgo=",
                    "data:image/png;base64,async",
                    "data:image/png;base64,fail",
                    "data:image/svg+xml,%3Csvg%3E%3C/svg%3E",
                    "",
                ],
            );
            html.push_str(&format!("<img{cls} src=\"{src}\">"));
        }
        6 => {
            html.push_str(&format!("<details{cls}{}><summary>Open</summary>", if r.chance(3) { " open" } else { "" }));
            node(r, html, depth + 1);
            if r.chance(3) {
                html.push_str("<summary>second</summary>");
            }
            html.push_str("</details>");
        }
        7 => html.push_str(&format!("<button{cls}>{}</button>", pick(r, &TEXTS))),
        _ => {
            let tag = *pick(r, &["div", "p", "span", "b", "a", "section", "label", "li", "h1"]);
            html.push_str(&format!("<{tag}{cls}>"));
            for _ in 0..1 + r.below(4) {
                node(r, html, depth + 1);
            }
            html.push_str(&format!("</{tag}>"));
        }
    }
}

// ---- Input ------------------------------------------------------------------------------------------------

/// The events a widget may listen for, and some it may not.
const TYPES: [&str; 30] = [
    "click",
    "pointerdown",
    "pointerup",
    "pointermove",
    "pointerenter",
    "pointerleave",
    "pointerover",
    "pointerout",
    "pointercancel",
    "dblclick",
    "contextmenu",
    "mousedown",
    "mouseup",
    "mousemove",
    "mouseenter",
    "mouseleave",
    "mouseover",
    "mouseout",
    "gotpointercapture",
    "lostpointercapture",
    "touchstart",
    "touchmove",
    "focus",
    "input",
    "change",
    "wheel",
    "keydown",
    "animationiteration",
    "toggle",
    "load",
];

fn listen(r: &mut Lcg) -> String {
    let types: Vec<String> =
        if r.chance(3) { TYPES.iter().map(|t| quote(t)).collect() } else { (0..r.below(12)).map(|_| quote(pick(r, &TYPES))).collect() };
    format!("{{\"type\":\"listen\",\"types\":[{}]}}", types.join(","))
}

fn prevent(r: &mut Lcg, max_id: u32) -> String {
    let keys: Vec<String> = (0..r.below(6))
        .map(|_| {
            let id = 2 + r.below(max_id.max(3) as usize);
            quote(&format!("{id}:{}", pick(r, &["pointerdown", "mousedown", "touchstart", "click", "pointerup"])))
        })
        .collect();
    format!("{{\"type\":\"preventDefault\",\"keys\":[{}]}}", keys.join(","))
}

/// Where a pointer goes: mostly over the content, sometimes off it.
fn point(r: &mut Lcg, width: f64) -> (f64, f64) {
    let x = if r.chance(10) { -5.0 + r.below(20) as f64 } else { r.below(width as usize + 10) as f64 - 4.0 + r.below(4) as f64 * 0.25 };
    let y = if r.chance(3) { r.below(60) as f64 } else { r.below(320) as f64 } + r.below(4) as f64 * 0.5;
    (x, y)
}

fn some_id(r: &mut Lcg, max_id: u32) -> i64 {
    (2 + r.below(max_id.max(3) as usize - 1)) as i64
}

/// An ops batch a script sends between gestures: what it changes is what the input reads (classes and styles the
/// cascade answers from, removed nodes, focus, capture, observers, values, open details).
fn script_ops(r: &mut Lcg, s: &Script, pointer: i32) -> String {
    let id = some_id(r, s.max_id);
    let op = match r.below(14) {
        0 => format!("{{\"op\":\"attr\",\"id\":{id},\"name\":\"class\",\"value\":\"c{} c{}\"}}", r.below(8), r.below(8)),
        1 => format!(
            "{{\"op\":\"style\",\"id\":{id},\"name\":{},\"value\":{}}}",
            quote(pick(r, &["touch-action", "user-select", "pointer-events", "display", "cursor", "transform", "width"])),
            quote(pick(r, &["none", "text", "auto", "pan-y", "block", "pointer", "translateX(20px)", "50px", ""]))
        ),
        2 => format!("{{\"op\":\"rm\",\"id\":{id}}}"),
        3 | 4 => format!("{{\"op\":\"call\",\"id\":{id},\"method\":\"focus\",\"args\":[]}}"),
        5 => format!("{{\"op\":\"call\",\"id\":{id},\"method\":\"blur\",\"args\":[]}}"),
        6 => format!(
            "{{\"op\":\"call\",\"id\":{id},\"method\":{},\"args\":[{}]}}",
            quote(pick(r, &["setPointerCapture", "releasePointerCapture"])),
            pick(r, &[1, pointer + 2, 2, 5, 0])
        ),
        7 => format!("{{\"op\":\"call\",\"id\":{id},\"method\":{},\"args\":[]}}", quote(pick(r, &["observeResize", "unobserveResize"]))),
        8 => format!(
            "{{\"op\":\"call\",\"id\":{id},\"method\":{},\"args\":[{},{}]}}",
            quote(pick(r, &["select", "setSelectionRange"])),
            r.below(8) as i64 - 1,
            r.below(12) as i64 - 2
        ),
        9 => format!("{{\"op\":\"prop\",\"id\":{id},\"name\":\"value\",\"value\":{}}}", quote(pick(r, &TEXTS))),
        10 => format!(
            "{{\"op\":\"attr\",\"id\":{id},\"name\":\"src\",\"value\":{}}}",
            quote(pick(r, &["data:image/png;base64,QUJD", "data:image/png;base64,asyncB", "data:x"]))
        ),
        11 => {
            format!("{{\"op\":\"call\",\"id\":{id},\"method\":{},\"args\":[]}}", quote(pick(r, &["showModal", "close", "scrollIntoView"])))
        }
        12 => {
            let fresh = 50000 + r.below(100) as i64;
            format!(
                "{{\"op\":\"el\",\"id\":{fresh},\"tag\":\"img\",\"attrs\":[[\"src\",{}]]}}",
                quote(pick(r, &["data:image/png;base64,QUJD", "data:image/png;base64,async", "data:image/png;base64,fail"]))
            )
        }
        _ => format!("{{\"op\":\"attr\",\"id\":{id},\"name\":\"open\",\"value\":\"\"}}"),
    };
    format!("[{op}]")
}

/// `n` gestures and calls over a document already in the script: taps and double taps by mouse and finger, holds,
/// drags each way, secondary buttons, a second finger, hovers in and out, claims, frames, paints, selections, edits,
/// and ops batches between them; the clock moves on by a frame to most of a second each step.
pub fn gestures(r: &mut Lcg, s: &mut Script, n: usize) {
    let width = s.width;
    let mut pointer = 0;
    for _ in 0..n {
        let dt = *pick(r, &[0.0, 8.0, 16.0, 16.0, 40.0, 120.0, 250.0, 320.0, 600.0, 1200.0]);
        let mouse = r.chance(2);
        let (x, y) = point(r, width);
        match r.below(30) {
            0..=5 => {
                // A tap, maybe twice.
                let button = if mouse && r.chance(6) { 2 } else { 0 };
                let times = if r.chance(4) { 2 } else { 1 };
                for k in 0..times {
                    let (dx, dy) = (k as f64 * r.below(30) as f64, k as f64 * r.below(30) as f64);
                    s.after(dt, Step::Touch { action: 0, x: x + dx, y: y + dy, pointer, primary: true, mouse, modifiers: 0, button });
                    if r.chance(3) {
                        s.after(
                            16.0,
                            Step::Touch {
                                action: 1,
                                x: x + dx + 3.0,
                                y: y + dy + 2.0,
                                pointer,
                                primary: true,
                                mouse,
                                modifiers: 0,
                                button,
                            },
                        );
                    }
                    s.after(
                        *pick(r, &[30.0, 80.0, 200.0]),
                        Step::Touch { action: 2, x: x + dx, y: y + dy, pointer, primary: true, mouse, modifiers: 1, button },
                    );
                }
            }
            6..=9 => {
                // A drag: horizontal, vertical or both, released, cancelled or left pressed.
                s.after(dt, Step::Touch { action: 0, x, y, pointer, primary: true, mouse, modifiers: 0, button: 0 });
                let (mut px, mut py) = (x, y);
                let (sx, sy) = *pick(r, &[(12.0, 0.0), (0.0, 12.0), (8.0, 8.0), (-15.0, 1.0), (3.0, 40.0), (25.0, -2.0)]);
                for _ in 0..1 + r.below(6) {
                    px += sx;
                    py += sy;
                    s.after(
                        *pick(r, &[8.0, 16.0, 33.0]),
                        Step::Touch { action: 1, x: px, y: py, pointer, primary: true, mouse, modifiers: 0, button: 0 },
                    );
                    if r.chance(3) {
                        s.after(0.0, Step::Tick);
                    }
                }
                match r.below(5) {
                    0 => {
                        s.after(16.0, Step::Touch { action: 3, x: px, y: py, pointer, primary: true, mouse, modifiers: 0, button: 0 });
                    }
                    1 => {}
                    _ => {
                        s.after(16.0, Step::Touch { action: 2, x: px, y: py, pointer, primary: true, mouse, modifiers: 0, button: 0 });
                    }
                }
                if r.chance(2) {
                    s.after(0.0, Step::Observe);
                }
            }
            10 | 11 => {
                // A hold, the frames that settle it, its release.
                s.after(dt, Step::Touch { action: 0, x, y, pointer, primary: true, mouse, modifiers: 0, button: 0 });
                s.after(300.0, Step::Tick);
                s.after(*pick(r, &[150.0, 260.0, 400.0]), Step::Tick);
                if r.chance(3) {
                    s.after(
                        16.0,
                        Step::Touch { action: 1, x: x + 2.0, y: y + 60.0, pointer, primary: true, mouse, modifiers: 0, button: 0 },
                    );
                }
                s.after(
                    40.0,
                    Step::Touch { action: if r.chance(4) { 3 } else { 2 }, x, y, pointer, primary: true, mouse, modifiers: 0, button: 0 },
                );
            }
            12 | 13 => {
                // A mouse hovering over the view, maybe leaving it.
                for _ in 0..1 + r.below(4) {
                    let (hx, hy) = point(r, width);
                    s.after(dt.min(40.0), Step::Hover { action: 0, x: hx, y: hy, modifiers: r.below(16) as i32 });
                }
                if r.chance(2) {
                    s.after(16.0, Step::Tick);
                }
                if r.chance(3) {
                    s.after(16.0, Step::Hover { action: 1, x: 0.0, y: 0.0, modifiers: 0 });
                }
            }
            14 => {
                s.after(dt, Step::Claim { x, y });
            }
            15 => {
                s.after(dt, Step::Tick);
            }
            16 => {
                s.after(dt, Step::Paint);
            }
            17 => {
                s.after(dt, Step::Observe);
            }
            18 | 19 => {
                let text = pick(r, &["abc", "", "héllo wörld", "12", "a\nb", "😀"]).as_bytes().to_vec();
                let current = !r.chance(5);
                s.after(
                    dt,
                    Step::Edit {
                        current,
                        generation: r.below(6) as u64,
                        text,
                        kind: pick(r, &["insertText", "deleteContentBackward"]).as_bytes().to_vec(),
                    },
                );
                if r.chance(3) {
                    s.after(16.0, Step::EndEditing);
                }
            }
            20..=22 => {
                let ops = script_ops(r, s, pointer);
                s.after(dt, Step::Ops(ops.into_bytes()));
            }
            23 => {
                let m = match r.below(4) {
                    0 => listen(r),
                    1 => prevent(r, s.max_id),
                    2 => format!("{{\"type\":\"arbitrate\",\"bubble\":{},\"touch\":{},\"rtl\":{}}}", r.chance(2), r.chance(2), r.chance(3)),
                    _ => "{\"type\":\"query\",\"qid\":1,\"query\":{\"q\":\"elementFromPoint\",\"x\":10,\"y\":10}}".to_string(),
                };
                s.after(dt, Step::Message(m.into_bytes()));
            }
            24 => {
                s.after(dt, Step::ClearSelection);
            }
            25 => {
                s.after(dt, Step::ImageReady { id: 1 + r.below(6) as i32, w: *pick(r, &[0.0, 12.0, 30.5]), h: *pick(r, &[0.0, 7.0]) });
            }
            26 => {
                // A second finger while the first is down.
                s.after(dt, Step::Touch { action: 0, x, y, pointer, primary: true, mouse: false, modifiers: 0, button: 0 });
                let other = pointer + 1 + r.below(3) as i32;
                s.after(
                    16.0,
                    Step::Touch { action: 0, x: x + 40.0, y, pointer: other, primary: false, mouse: false, modifiers: 0, button: 0 },
                );
                s.after(
                    16.0,
                    Step::Touch {
                        action: 1,
                        x: x + 80.0,
                        y: y + 3.0,
                        pointer: other,
                        primary: false,
                        mouse: false,
                        modifiers: 0,
                        button: 0,
                    },
                );
                s.after(
                    16.0,
                    Step::Touch { action: 2, x: x + 80.0, y, pointer: other, primary: false, mouse: false, modifiers: 0, button: 0 },
                );
                s.after(16.0, Step::Touch { action: 2, x, y, pointer, primary: true, mouse: false, modifiers: 0, button: 0 });
            }
            27 => {
                s.after(dt, Step::LastHtml);
            }
            28 => {
                match r.below(6) {
                    0 => s.after(dt, Step::Width(*pick(r, &[200.0, 320.0, 150.5]))),
                    1 => s.after(dt, Step::Dark(r.chance(2))),
                    2 => s.after(dt, Step::Compositing(r.chance(2))),
                    3 => s.after(dt, Step::Wheel { x, y, dx: 0.0, dy: 40.0, mode: 0, modifiers: 0 }),
                    4 => s.after(
                        dt,
                        Step::Key { kind: b"keydown".to_vec(), key: b"a".to_vec(), code: b"KeyA".to_vec(), repeat: false, modifiers: 0 },
                    ),
                    _ => s.after(dt, Step::MaxHeight(*pick(r, &[100.0, 900.0]))),
                };
            }
            _ => {
                if r.chance(12) {
                    s.after(dt, Step::Message(b"{\"type\":\"frozen\",\"reason\":\"Stopped\",\"fatal\":false}".to_vec()));
                }
                pointer = (pointer + 1) % 3;
            }
        }
    }
    s.after(16.0, Step::Observe);
}

/// A script over a document given as ops (under the roots): the document, what the widget listens for and
/// prevents, how the client arbitrates, then gestures.
pub fn script_over(r: &mut Lcg, ops: &[String], max_id: u32, gestures_n: usize) -> Script {
    let mut s = Script::new(max_id);
    s.width = *pick(r, &[300.0, 300.0, 240.0, 412.5]);
    s.dark = r.chance(4);
    if r.chance(4) {
        s.after(0.0, Step::Compositing(true));
    }
    s.after(0.0, Step::Ops(document(ops).into_bytes()));
    s.after(0.0, Step::Message(listen(r).into_bytes()));
    if r.chance(3) {
        let p = prevent(r, max_id);
        s.after(0.0, Step::Message(p.into_bytes()));
    }
    if r.chance(3) {
        let a = format!("{{\"type\":\"arbitrate\",\"bubble\":{},\"touch\":{},\"rtl\":{}}}", r.chance(2), r.chance(2), r.chance(4));
        s.after(0.0, Step::Message(a.into_bytes()));
    }
    if r.chance(2) {
        s.after(16.0, Step::Paint);
    }
    gestures(r, &mut s, gestures_n);
    s
}
