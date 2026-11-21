//! The hosted glue against tdesktop's (`oracle/hosted_glue_oracle`: tgw_hosted_glue.cpp and tgw_cursor.cpp on
//! TwidgetCore's renderer end and renderer, the renderer's clock and the frame message's on the session's step clock),
//! end to end over a hosted renderer's platform: widget-sandbox's corpus widgets and generated documents through
//! sessions of what the served host and Telegram send (the kept messages at `ready`, load and Open in either order, the
//! widget's batches, listeners, the post-load query, queries, patches, frames that tick and paint, hovers, mouse and
//! finger presses, the press controls, canvas refills, sizes, themes, claims, `frozen`, bad controls), every byte the
//! glue sends and requests, every answer and every message and diagnostic to the host alike. Skipped without
//! `TWIDGET_ORACLE_DIR` or without tdesktop's sources.

mod common;

use common::docs::{Shadow, pick, widgets};
use common::glue::{GlueSession, GlueStep};
use common::*;
use twidget_helper::wire;
use twidget_remote::{ClaimQuery, ClearSelection, FrameRequest, Hover, Message, Paint, SetMaxHeight, SetTheme, SetWidth, Touch};

fn encode(m: &impl Message) -> Vec<u8> {
    let mut out = Vec::new();
    assert!(m.encode(&wire::embedding_caps(), &mut out));
    out
}

struct Builder<'a> {
    r: &'a mut Lcg,
    clock: f64,
    steps: Vec<GlueStep>,
    width: f64,
    seq: u64,
}

impl Builder<'_> {
    fn call(&mut self, op: u8, text: Vec<u8>) {
        self.clock += *pick(self.r, &[0.0, 8.0, 16.0, 16.0, 16.0, 33.0, 120.0, 400.0, 1200.0]);
        self.steps.push(GlueStep::Call { op, clock: self.clock, text, width: 0.0 });
    }
    fn host(&mut self, json: &str) {
        self.call(b'S', json.as_bytes().to_vec());
    }
    fn platform(&mut self, message: Vec<u8>) {
        self.call(b'P', message);
    }
    fn frame(&mut self, tick: bool, paint: Paint) {
        self.seq += 1;
        let seq = self.seq;
        self.platform(encode(&FrameRequest { seq, tick, paint }));
    }
    fn point(&mut self) -> (f64, f64) {
        let x = if self.r.chance(12) { -4.0 } else { self.r.below(self.width as usize + 6) as f64 + self.r.below(4) as f64 * 0.25 };
        let tall = if self.r.chance(3) { 80 } else { 360 };
        let y = self.r.below(tall) as f64 + self.r.below(2) as f64 * 0.5;
        (x, y)
    }
    fn touch(&mut self, action: i32, mouse: bool, button: i32) {
        let (x, y) = self.point();
        self.platform(encode(&Touch { action, x, y, pointer: if mouse { 0 } else { 1 }, primary: true, mouse, modifiers: 0, button }));
    }
}

const THEME: &str =
    r##"{"type":"theme","theme":{"colorScheme":"light","params":{"bg_color":"#ffffff","text_color":"#000000","button_color":"#3390ec"}}}"##;
const DARK_THEME: &str = r##"{"type":"theme","theme":{"colorScheme":"dark","params":{"bg_color":"#212121","text_color":"#ffffff"}}}"##;
const SAFE_AREA: &str = r#"{"type":"safeArea","inset":{"top":0,"bottom":8,"left":0,"right":0}}"#;
const POLICY: &str = r#"{"type":"policy","policy":"strict"}"#;
const LISTEN: &str = r#"{"type":"listen","types":["click","contextmenu","pointerdown","pointerup","pointermove","mouseover"]}"#;
const ACK: &str = r#"{"type":"query","qid":-1,"query":{"q":"viewport"}}"#;

fn session(r: &mut Lcg, batch: &str, last: i64, steps_n: usize) -> Vec<GlueStep> {
    let mut shadow = Shadow::new(batch, last);
    let width = *pick(r, &[300.0, 320.0, 412.5]);
    let dark = r.chance(4);
    let ready: Vec<Vec<u8>> = if r.chance(8) {
        Vec::new()
    } else {
        [if dark { DARK_THEME } else { THEME }, SAFE_AREA, POLICY].iter().map(|m| m.as_bytes().to_vec()).collect()
    };
    let begin = GlueStep::Begin {
        clock: 1000.0,
        scheme: if dark { b"dark".to_vec() } else { b"light".to_vec() },
        width,
        platform: !r.chance(40),
        images: !r.chance(5),
        ready,
    };
    let mut b = Builder { r, clock: 1000.0, steps: vec![begin], width, seq: 0 };
    let dpr = *pick(b.r, &[1.0, 2.0, 1.5, 3.0, 0.1, 100.0]);
    let open = wire::encode_open(dpr);
    match b.r.below(5) {
        0 => {
            b.platform(open);
            b.call(b'L', Vec::new());
        }
        1 => {
            b.host(&format!(r#"{{"type":"ops","ops":{batch}}}"#));
            b.call(b'L', Vec::new());
            b.platform(open);
        }
        _ => {
            b.call(b'L', Vec::new());
            b.platform(open);
        }
    }
    b.host(LISTEN);
    b.host(&format!(r#"{{"type":"ops","ops":{batch}}}"#));
    if !b.r.chance(6) {
        b.host(ACK);
    }
    let canvas = shadow.next;
    let drawing = b.r.chance(2);
    if drawing {
        b.host(&format!(
            r#"{{"type":"ops","ops":[{{"op":"el","id":{canvas},"tag":"canvas","attrs":[["width","120"],["height","40"]]}},{{"op":"ins","parent":4,"id":{canvas},"before":null}}]}}"#
        ));
    }
    b.frame(true, Paint::IfDirty);
    let mut downs = 0u64;
    for _ in 0..steps_n {
        match b.r.below(46) {
            0..=5 => {
                let (x, y) = b.point();
                let action = if b.r.chance(10) { 1 } else { 0 };
                b.platform(encode(&Hover { action, x, y, modifiers: 0 }));
            }
            6..=8 => {
                downs += 1;
                let button = if b.r.chance(4) { 2 } else { 0 };
                b.touch(0, true, button);
                for _ in 0..b.r.below(3) {
                    b.touch(1, true, button);
                }
                if b.r.chance(4) {
                    b.frame(true, Paint::IfDirty);
                }
                if button == 2 && b.r.chance(2) {
                    b.platform(wire::encode_menu_deliver(downs));
                }
                let end = if b.r.chance(8) { 3 } else { 2 };
                b.touch(end, true, button);
            }
            9 | 10 => {
                downs += 1;
                b.touch(0, false, 0);
                if b.r.chance(2) {
                    b.frame(true, Paint::Skip);
                }
                match b.r.below(3) {
                    0 => b.platform(wire::encode_finger_hold(downs)),
                    1 => b.platform(wire::encode_finger_release(downs)),
                    _ => {}
                }
                b.frame(true, Paint::IfDirty);
                b.touch(2, false, 0);
            }
            11..=17 => {
                let tick = !b.r.chance(4);
                let paint = *pick(b.r, &[Paint::IfDirty, Paint::IfDirty, Paint::Force, Paint::Skip]);
                b.frame(tick, paint);
            }
            18..=21 => {
                let patch = shadow.patch(b.r);
                shadow.apply(&patch);
                b.host(&format!(r#"{{"type":"ops","ops":{patch}}}"#));
            }
            22 => {
                let id = if shadow.ids.is_empty() { 4 } else { shadow.ids[b.r.below(shadow.ids.len())] };
                let qid = 10 + b.r.below(5);
                b.host(&format!(r#"{{"type":"query","qid":{qid},"query":{{"q":"box","id":{id}}}}}"#));
            }
            23 => {
                let (x, y) = b.point();
                b.host(&format!(r#"{{"type":"query","qid":7,"query":{{"q":"elementFromPoint","x":{x},"y":{y}}}}}"#));
            }
            24 => b.host(r#"{"type":"requestFrame"}"#),
            25 => {
                let scheme = if b.r.chance(2) { b"dark".to_vec() } else { b"light".to_vec() };
                b.call(b'T', scheme);
            }
            26 => {
                let w = *pick(b.r, &[280.0, 300.0, 412.5]);
                b.platform(encode(&SetWidth { width: w }));
            }
            27 => {
                let height = *pick(b.r, &[200.0, 600.0, 1200.0]);
                b.platform(encode(&SetMaxHeight { height }));
            }
            28 => {
                let dark = b.r.chance(2);
                b.platform(encode(&SetTheme { dark }));
            }
            29 => {
                let (x, y) = b.point();
                b.platform(encode(&ClaimQuery { seq: 3, x, y }));
            }
            30 => b.platform(encode(&ClearSelection)),
            31 => b.platform(wire::encode_canvas_refill()),
            32 => {
                let theme = if b.r.chance(2) { THEME } else { DARK_THEME };
                b.host(theme);
            }
            33 => {
                let press = b.r.below(4) as u64;
                b.platform(wire::encode_menu_deliver(press));
            }
            34 => {
                if b.r.chance(6) {
                    b.host(r#"{"type":"frozen","reason":"Widget stopped"}"#);
                } else {
                    b.call(b'W', Vec::new());
                }
            }
            35 if b.r.chance(5) => {
                let bad = match b.r.below(6) {
                    0 => wire::encode_mirror(),
                    1 => wire::encode_open(2.0),
                    2 => vec![wire::TAG, 2, 1],
                    3 => vec![wire::TAG, 99],
                    4 => vec![7, 1, 2],
                    _ => [wire::encode_canvas_refill(), vec![0]].concat(),
                };
                b.platform(bad);
            }
            36 => b.host("not json"),
            38..=45 if drawing => {
                let mut cmds = Vec::new();
                for _ in 0..4 + b.r.below(30) {
                    let font = if b.r.chance(30) {
                        format!("{}px {}", 10 + b.r.below(4), "f".repeat(520))
                    } else {
                        format!("{}{}px f{}", if b.r.chance(4) { "bold " } else { "" }, 8 + b.r.below(6), b.r.below(120))
                    };
                    cmds.push(format!(r#"["s","font","{font}"],["c","fillText","hi",0,10]"#));
                }
                b.host(&format!(r#"{{"type":"ops","ops":[{{"op":"canvas","id":{canvas},"cmds":[{}]}}]}}"#, cmds.join(",")));
            }
            37 if b.r.chance(8) => b.call(b'D', Vec::new()),
            _ => b.frame(true, Paint::IfDirty),
        }
    }
    b.call(b'E', Vec::new());
    b.platform(wire::encode_mirror());
    b.steps
}

/// Steps and transcript bytes, the platform's messages and requests, the host's messages and diagnostics, and of the
/// platform's messages the glue's controls: Content, Loaded, Stopped, Mirrored, Cursor, CanvasRefused, Clicked.
#[derive(Default)]
struct Tally {
    steps: usize,
    bytes: usize,
    sends: usize,
    requests: usize,
    messages: usize,
    diags: usize,
    controls: [usize; 7],
}

/// The transcript's entries counted, as hosted_glue_oracle.cpp writes them.
fn count(t: &[u8], tally: &mut Tally) {
    let u32_at = |i: usize| u32::from_le_bytes(t[i..i + 4].try_into().expect("four bytes")) as usize;
    // A logged run of bytes at `i`: its first two bytes when shown, and where it ends.
    let logged = |i: usize| -> (Option<[u8; 2]>, usize) {
        if t[i] == b'#' {
            (None, i + 17)
        } else {
            let n = u32_at(i + 1);
            ((n >= 2).then(|| [t[i + 5], t[i + 6]]), i + 5 + n)
        }
    };
    let mut i = 0;
    while i < t.len() {
        let tag = t[i];
        i += 1;
        match tag {
            b'p' => {
                let (head, end) = logged(i);
                tally.sends += 1;
                if let Some([wire::TAG, kind]) = head
                    && (64..=70).contains(&kind)
                {
                    tally.controls[usize::from(kind - 64)] += 1;
                }
                i = end;
            }
            b'q' => {
                tally.requests += 1;
                i = logged(i).1;
            }
            b'a' => i = logged(i + 1).1,
            b'm' => {
                tally.messages += 1;
                i = logged(i).1;
            }
            b'g' => {
                tally.diags += 1;
                let n = u32_at(i);
                i += 4 + n;
                i += 4 + u32_at(i);
            }
            b'r' => i += 1,
            b'!' => i += 4 + u32_at(i),
            _ => panic!("a transcript entry {tag}"),
        }
    }
}

fn compare(dirs: &[&str], generated: usize, seed: u64, steps_n: usize, what: &str) {
    let Some(oracle) = helper_oracle("hosted_glue_oracle") else { return };
    let mut r = Lcg::new(seed);
    let mut driver = GlueSession::default();
    let mut c = Comparison::new();
    let mut tally = Tally::default();
    let docs = widgets(dirs, generated, seed + 1);
    for (name, batch, last) in &docs {
        let steps = session(&mut r, batch, *last, steps_n);
        for (i, step) in steps.iter().enumerate() {
            let transcript = driver.run(step);
            tally.steps += 1;
            tally.bytes += transcript.len();
            count(&transcript, &mut tally);
            let shown = match step {
                GlueStep::Call { op, text, .. } => format!("{} {}", *op as char, String::from_utf8_lossy(&text[..text.len().min(160)])),
                GlueStep::Begin { .. } => "begin".to_string(),
            };
            c.case(&step.record(), (1, transcript), format!("{name}, step {i}: {shown}"));
        }
    }
    c.check(&oracle);
    eprintln!(
        "glue, {what}: {} sessions, {} steps alike ({} transcript bytes, {} platform messages, {} requests, {} host messages, {} diagnostics)",
        docs.len(),
        tally.steps,
        tally.bytes,
        tally.sends,
        tally.requests,
        tally.messages,
        tally.diags
    );
    eprintln!("glue, {what}: Content, Loaded, Stopped, Mirrored, Cursor, CanvasRefused, Clicked sent {:?}", tally.controls);
}

#[test]
fn corpus_sessions_alike() {
    compare(&["corpus"], 0, 0x474c_0002, 32, "the corpus");
}

#[test]
fn generated_sessions_alike() {
    compare(&[], 800, 0x474c_0001, 40, "generated documents");
}
