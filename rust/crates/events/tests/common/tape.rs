//! The tape `oracle/events_probe.h` writes: what renderer.cpp's input code asked the rest of the renderer and what
//! it was told, in order, and where the rest of the renderer called the input's code itself. [`TapeHost`] answers the
//! port's [`EventHost`] from it, checking every question is the C++'s, in its order, with its arguments.
//!
//! Entries: 'S' a step begins, then the renderer's state ([`Sync`]: u8 stale, u8 layout stopped, u8 frozen, str the
//! reason, u8 fatal, u32 count of its pending violations and each, u8 paint stopped); a marker 'E' (refresh_editor),
//! 'F' (flush_pending), 'M' (flush_moves), 'H' (check_hold), 'G' (load_images), 'D' (a paint finds its runs again),
//! each with the state; 'Q' paint_selection with the state, i64 layer, u8 tint, str the list in wire form, and after
//! its questions 'q' u8 encoded, u32 commands, u64 FNV-1a of the list it left; 'A' u8 a build the input asked for
//! ('s' settle into the answer, 'p' settle into the pending messages, 'r' rebuild patching, 'R' rebuild, 'y'
//! restyled_by_input), what the input's code answered inside it, 'Z' and the state after it; 'C' u8 a call without an
//! answer; 'X' before a question asked outside the input's code; a question ([`Q`]) with its arguments and answer.

use std::collections::VecDeque;

use twidget_events::{EditorStyle, EventHost, TextRun};
use twidget_json::Json;
use twidget_shadow::{Document, DocumentHost};
use twidget_wire::{Affine, Color, Rect};

use super::Tally;

#[derive(Clone, Debug, Default)]
pub struct Sync {
    pub stale: bool,
    pub layout_stopped: bool,
    pub frozen: bool,
    pub reason: Vec<u8>,
    pub fatal: bool,
    /// The violations the renderer holds for its next message: the tree's, its own stops' and its container's.
    pub violations: Vec<Vec<u8>>,
    pub paint_stopped: bool,
}

impl Sync {
    /// The violation of a stop past the layout or name limit, the last one pending.
    pub fn stop(&self) -> Vec<u8> {
        self.violations.iter().rev().find(|v| v.starts_with(b"layout limit") || v.starts_with(b"name limit")).cloned().unwrap_or_default()
    }
}

#[derive(Clone, Debug)]
pub enum Q {
    Hit { x: f64, y: f64, placement: bool, id: i64, rect: Rect },
    Box { id: i64, rect: Rect },
    Exists { id: i64, yes: bool },
    TouchAction { id: i64, value: Option<Vec<u8>> },
    UserSelect { id: i64, yes: bool },
    Mouse { action: i32, x: f64, y: f64, hovers: bool, redraw: bool },
    Leave(bool),
    Cursor(Vec<u8>),
    RunsKey([u64; 4]),
    Runs(Vec<TextRun>),
    Width { text: Vec<u8>, font: u64, w: f32 },
    FontInfo { font: u64, info: Option<(i32, f32, f32, f32)> },
    Layer { id: i64, now: f64, shown: Option<(Affine, f32)> },
    SelectableKey([u64; 2]),
    Selectable(bool),
    EditorStyle { id: i64, style: Option<EditorStyle> },
    ImageOf { src: Vec<u8>, answer: Option<(i32, f32, f32)> },
    ImageReady { id: i32, w: f32, h: f32, changed: bool },
    Content { id: i64, size: (f32, f32) },
    Pending(Vec<Vec<u8>>),
    ViewWidth(f64),
}

impl Q {
    pub fn name(&self) -> &'static str {
        match self {
            Q::Hit { .. } => "hit",
            Q::Box { .. } => "box_of",
            Q::Exists { .. } => "has_element",
            Q::TouchAction { .. } => "touch_action",
            Q::UserSelect { .. } => "user_select_text",
            Q::Mouse { .. } => "mouse",
            Q::Leave(_) => "leave",
            Q::Cursor(_) => "cursor",
            Q::RunsKey(_) => "runs_key",
            Q::Runs(_) => "text_runs",
            Q::Width { .. } => "text_width",
            Q::FontInfo { .. } => "font_info",
            Q::Layer { .. } => "layer_shown",
            Q::SelectableKey(_) => "selectable_key",
            Q::Selectable(_) => "any_selectable",
            Q::EditorStyle { .. } => "editor_style",
            Q::ImageOf { .. } => "image_of",
            Q::ImageReady { .. } => "image_ready",
            Q::Content { .. } => "content_size",
            Q::Pending(_) => "take_pending",
            Q::ViewWidth(_) => "width",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Entry {
    Step(Sync),
    Marker(u8, Sync),
    Paint { sync: Sync, layer: i64, tint: bool, list: Vec<u8> },
    PaintOut { encoded: bool, len: u32, digest: u64 },
    Action { tag: u8, nested: Vec<Entry>, sync: Sync },
    Command(u8),
    Unexpected,
    Question(Q),
}

struct Take<'a> {
    s: &'a [u8],
    at: usize,
}

impl Take<'_> {
    fn bytes(&mut self, n: usize) -> Result<&[u8], String> {
        let v =
            self.s.get(self.at..self.at.checked_add(n).ok_or("a length past the tape")?).ok_or_else(|| format!("cut at {}", self.at))?;
        self.at += n;
        Ok(v)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.bytes(1)?[0])
    }
    fn flag(&mut self) -> Result<bool, String> {
        Ok(self.u8()? != 0)
    }
    fn arr<const N: usize>(&mut self) -> Result<[u8; N], String> {
        Ok(self.bytes(N)?.try_into().unwrap_or([0; N]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.arr()?))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.arr()?))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.arr()?))
    }
    fn i64(&mut self) -> Result<i64, String> {
        Ok(i64::from_le_bytes(self.arr()?))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.arr()?))
    }
    fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_le_bytes(self.arr()?))
    }
    fn str(&mut self) -> Result<Vec<u8>, String> {
        let n = self.u32()? as usize;
        Ok(self.bytes(n)?.to_vec())
    }
    fn rect(&mut self) -> Result<Rect, String> {
        Ok(Rect::new(self.f32()?, self.f32()?, self.f32()?, self.f32()?))
    }
    fn sync(&mut self) -> Result<Sync, String> {
        Ok(Sync {
            stale: self.flag()?,
            layout_stopped: self.flag()?,
            frozen: self.flag()?,
            reason: self.str()?,
            fatal: self.flag()?,
            violations: {
                let n = self.u32()? as usize;
                let mut list = Vec::with_capacity(n.min(1 << 12));
                for _ in 0..n {
                    list.push(self.str()?);
                }
                list
            },
            paint_stopped: self.flag()?,
        })
    }

    fn entry(&mut self) -> Result<Entry, String> {
        let tag = self.u8()?;
        Ok(match tag {
            b'S' => Entry::Step(self.sync()?),
            b'E' | b'F' | b'M' | b'H' | b'G' | b'D' => Entry::Marker(tag, self.sync()?),
            b'Q' => Entry::Paint { sync: self.sync()?, layer: self.i64()?, tint: self.flag()?, list: self.str()? },
            b'q' => Entry::PaintOut { encoded: self.flag()?, len: self.u32()?, digest: self.u64()? },
            b'A' => {
                let tag = self.u8()?;
                let mut nested = Vec::new();
                loop {
                    if self.s.get(self.at) == Some(&b'Z') {
                        self.at += 1;
                        break;
                    }
                    nested.push(self.entry()?);
                }
                Entry::Action { tag, nested, sync: self.sync()? }
            }
            b'C' => Entry::Command(self.u8()?),
            b'X' => Entry::Unexpected,
            b'h' => {
                let (x, y, placement, id) = (self.f64()?, self.f64()?, self.flag()?, self.i64()?);
                let rect = if placement { self.rect()? } else { Rect::default() };
                Entry::Question(Q::Hit { x, y, placement, id, rect })
            }
            b'b' => Entry::Question(Q::Box { id: self.i64()?, rect: self.rect()? }),
            b'x' => Entry::Question(Q::Exists { id: self.i64()?, yes: self.flag()? }),
            b'a' => {
                let id = self.i64()?;
                let value = if self.flag()? { Some(self.str()?) } else { None };
                Entry::Question(Q::TouchAction { id, value })
            }
            b'u' => Entry::Question(Q::UserSelect { id: self.i64()?, yes: self.flag()? }),
            b'm' => Entry::Question(Q::Mouse {
                action: self.i32()?,
                x: self.f64()?,
                y: self.f64()?,
                hovers: self.flag()?,
                redraw: self.flag()?,
            }),
            b'v' => Entry::Question(Q::Leave(self.flag()?)),
            b'c' => Entry::Question(Q::Cursor(self.str()?)),
            b'K' => Entry::Question(Q::RunsKey([self.u64()?, self.u64()?, self.u64()?, self.u64()?])),
            b'R' => {
                let n = self.u32()? as usize;
                let mut runs = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    runs.push(TextRun {
                        owner: self.i64()?,
                        offset: self.u64()? as usize,
                        text: self.str()?,
                        rect: self.rect()?,
                        font: self.u64()?,
                        block: self.i32()?,
                        selectable: self.flag()?,
                        line_break: self.flag()?,
                        ..TextRun::default()
                    });
                }
                Entry::Question(Q::Runs(runs))
            }
            b'w' => Entry::Question(Q::Width { text: self.str()?, font: self.u64()?, w: self.f32()? }),
            b'f' => {
                let font = self.u64()?;
                let info = if self.flag()? { Some((self.i32()?, self.f32()?, self.f32()?, self.f32()?)) } else { None };
                Entry::Question(Q::FontInfo { font, info })
            }
            b'L' => {
                let (id, now) = (self.i64()?, self.f64()?);
                let shown = if self.flag()? {
                    let m = Affine { a: self.f32()?, b: self.f32()?, c: self.f32()?, d: self.f32()?, e: self.f32()?, f: self.f32()? };
                    Some((m, self.f32()?))
                } else {
                    None
                };
                Entry::Question(Q::Layer { id, now, shown })
            }
            b'k' => Entry::Question(Q::SelectableKey([self.u64()?, self.u64()?])),
            b'j' => Entry::Question(Q::Selectable(self.flag()?)),
            b'e' => {
                let id = self.i64()?;
                let style = if self.flag()? {
                    Some(EditorStyle {
                        frame: self.rect()?,
                        font_family: self.str()?,
                        font_size: self.f32()?,
                        font_weight: self.i32()?,
                        italic: self.flag()?,
                        color: Color::rgba(self.u8()?, self.u8()?, self.u8()?, self.u8()?),
                        align: self.i32()?,
                        font: self.i32()?,
                        has_clip: self.flag()?,
                        clip: self.rect()?,
                    })
                } else {
                    None
                };
                Entry::Question(Q::EditorStyle { id, style })
            }
            b'i' => {
                let src = self.str()?;
                let answer = if self.flag()? { Some((self.i32()?, self.f32()?, self.f32()?)) } else { None };
                Entry::Question(Q::ImageOf { src, answer })
            }
            b'I' => Entry::Question(Q::ImageReady { id: self.i32()?, w: self.f32()?, h: self.f32()?, changed: self.flag()? }),
            b'p' => Entry::Question(Q::Content { id: self.i64()?, size: (self.f32()?, self.f32()?) }),
            b'P' => {
                let n = self.u32()? as usize;
                let mut list = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    list.push(self.str()?);
                }
                Entry::Question(Q::Pending(list))
            }
            b'W' => Entry::Question(Q::ViewWidth(self.f64()?)),
            other => return Err(format!("entry {other} at {}", self.at - 1)),
        })
    }
}

pub fn parse(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    let mut t = Take { s: bytes, at: 0 };
    let mut out = Vec::new();
    while t.at < bytes.len() {
        out.push(t.entry()?);
    }
    Ok(out)
}

/// A freeze or stop that has no answer of its own.
struct Quiet;

impl DocumentHost for Quiet {
    fn canvas_run(&mut self, _canvas: i32, _width: i32, _height: i32, _ops: &[twidget_wire::CanvasOp]) {}
    fn canvas_release(&mut self, _canvas: i32) {}
    fn canvas_font(&mut self, _css: &[u8], _metrics: &mut twidget_wire::FontMetrics) -> i32 {
        0
    }
    fn canvas_text_width(&mut self, _font: i32, _text: &[u8]) -> f32 {
        0.0
    }
    fn image_of(&mut self, _src: &[u8]) -> Option<(i32, f32, f32)> {
        None
    }
}

/// The renderer's state the C++ had: whether a build is due, a stop past its layout or name limit, which the renderer
/// decides (stage 4.3; the tree's own stop past the tree limit the port decides itself), and the violations it holds,
/// which its container adds to as it loads images (stage 4.2).
pub fn apply(doc: &mut Document, s: &Sync, host: &mut dyn DocumentHost) {
    let renderer_stop = s.reason == b"Layout limit exceeded" || s.reason == b"Name limit exceeded";
    if s.layout_stopped && !doc.layout_stopped() && renderer_stop {
        doc.stop(&s.stop(), &s.reason, host);
    }
    if s.paint_stopped {
        doc.paint_stopped = true;
    }
    doc.stale = s.stale;
    doc.pending_violations = s.violations.clone();
}

/// The rest of the renderer, as the C++ was at each question: the tape's answers in its order.
pub struct TapeHost {
    entries: VecDeque<Entry>,
    /// What the input's code answered inside the last build it asked for, asked again after it.
    deferred: VecDeque<Entry>,
    pub now: f64,
    pub errors: Vec<String>,
    pub tally: Tally,
    /// Questions asked so far, for the errors' sake.
    pub asked: usize,
    /// The lists paint_selection left since the last step.
    pub painted: Vec<Vec<twidget_wire::Command>>,
}

impl TapeHost {
    pub fn new(entries: VecDeque<Entry>, now: f64) -> TapeHost {
        TapeHost { entries, deferred: VecDeque::new(), now, errors: Vec::new(), tally: Tally::default(), asked: 0, painted: Vec::new() }
    }

    pub fn error(&mut self, e: String) {
        if self.errors.len() < 8 {
            self.errors.push(e);
        }
    }

    pub fn peek(&self) -> Option<&Entry> {
        self.deferred.front().or_else(|| self.entries.front())
    }

    pub fn pop(&mut self) -> Option<Entry> {
        self.deferred.pop_front().or_else(|| self.entries.pop_front())
    }

    /// The next question, which must be the one `what` names; None (and an error) when the C++ asked something else.
    fn question(&mut self, what: &'static str) -> Option<Q> {
        self.asked += 1;
        if !self.errors.is_empty() {
            return None;
        }
        if matches!(self.peek(), Some(Entry::Unexpected)) {
            self.pop();
            self.error(format!("the C++ asked {what} outside the input's code"));
            return None;
        }
        match self.peek() {
            Some(Entry::Question(q)) if q.name() == what => {
                self.tally.add(format!("asked {what}"));
                match self.pop() {
                    Some(Entry::Question(q)) => Some(q),
                    _ => None,
                }
            }
            other => {
                let e = format!("question {}: the port asks {what}, the C++ {other:?}", self.asked);
                self.error(e);
                None
            }
        }
    }

    fn mismatch(&mut self, what: &str, port: String, cpp: String) {
        self.error(format!("question {}: {what} asked with {port} by the port, {cpp} by the C++", self.asked));
    }

    /// A build the input asked for: the C++'s, whose inner answers are asked after it.
    fn action(&mut self, tag: u8, doc: &mut Document) -> bool {
        if !self.errors.is_empty() {
            return false;
        }
        match self.peek() {
            Some(Entry::Action { tag: t, .. }) if *t == tag => {}
            other => {
                let e = format!("the port asks for build {}, the C++ {other:?}", tag as char);
                self.error(e);
                return false;
            }
        }
        let Some(Entry::Action { nested, sync, .. }) = self.pop() else { return false };
        self.tally.add(format!("build {}", tag as char));
        let mut built = false;
        let mut rest = VecDeque::new();
        for e in nested {
            if matches!(e, Entry::Marker(b'E', _)) {
                built = true;
            } else {
                rest.push_back(e);
            }
        }
        while let Some(e) = rest.pop_back() {
            self.deferred.push_front(e);
        }
        apply(doc, &sync, &mut Quiet);
        built
    }
}

impl EventHost for TapeHost {
    fn now_ms(&mut self) -> f64 {
        self.now
    }

    fn width(&mut self) -> f64 {
        match self.question("width") {
            Some(Q::ViewWidth(w)) => w,
            _ => 0.0,
        }
    }

    fn settle(&mut self, doc: &mut Document, out: Option<&mut Vec<Json>>) -> bool {
        self.action(if out.is_some() { b's' } else { b'p' }, doc)
    }

    fn rebuild(&mut self, doc: &mut Document, patch: bool) {
        self.action(if patch { b'r' } else { b'R' }, doc);
    }

    fn report_size(&mut self, _out: &mut Vec<Json>) {}

    fn restyled_by_input(&mut self, doc: &mut Document) {
        self.action(b'y', doc);
    }

    fn selection_changed(&mut self) {
        if !self.errors.is_empty() {
            return;
        }
        match self.peek() {
            Some(Entry::Command(b'C')) => {
                self.pop();
                self.tally.add("selection changed");
            }
            other => {
                let e = format!("the port calls selection_changed, the C++ {other:?}");
                self.error(e);
            }
        }
    }

    fn hit(&mut self, x: f64, y: f64, placement: Option<&mut Rect>) -> i64 {
        let has = placement.is_some();
        match self.question("hit") {
            Some(Q::Hit { x: cx, y: cy, placement: cp, id, rect }) => {
                if cx.to_bits() != x.to_bits() || cy.to_bits() != y.to_bits() || cp != has {
                    self.mismatch("hit", format!("({x}, {y}, {has})"), format!("({cx}, {cy}, {cp})"));
                }
                if let Some(p) = placement {
                    *p = rect;
                }
                if id != 0 {
                    self.tally.add("hit an element");
                }
                id
            }
            _ => 0,
        }
    }

    fn box_of(&mut self, id: i64) -> Rect {
        match self.question("box_of") {
            Some(Q::Box { id: cid, rect }) => {
                if cid != id {
                    self.mismatch("box_of", id.to_string(), cid.to_string());
                }
                rect
            }
            _ => Rect::default(),
        }
    }

    fn has_element(&mut self, id: i64) -> bool {
        match self.question("has_element") {
            Some(Q::Exists { id: cid, yes }) => {
                if cid != id {
                    self.mismatch("has_element", id.to_string(), cid.to_string());
                }
                yes
            }
            _ => false,
        }
    }

    fn touch_action(&mut self, id: i64) -> Option<Vec<u8>> {
        match self.question("touch_action") {
            Some(Q::TouchAction { id: cid, value }) => {
                if cid != id {
                    self.mismatch("touch_action", id.to_string(), cid.to_string());
                }
                if let Some(v) = &value {
                    self.tally.add(format!("touch-action {}", String::from_utf8_lossy(v)));
                }
                value
            }
            _ => None,
        }
    }

    fn user_select_text(&mut self, id: i64) -> bool {
        match self.question("user_select_text") {
            Some(Q::UserSelect { id: cid, yes }) => {
                if cid != id {
                    self.mismatch("user_select_text", id.to_string(), cid.to_string());
                }
                yes
            }
            _ => false,
        }
    }

    fn mouse(&mut self, action: i32, x: f64, y: f64, hovers: bool) -> bool {
        match self.question("mouse") {
            Some(Q::Mouse { action: ca, x: cx, y: cy, hovers: ch, redraw }) => {
                if ca != action || cx.to_bits() != x.to_bits() || cy.to_bits() != y.to_bits() || ch != hovers {
                    self.mismatch("mouse", format!("({action}, {x}, {y}, {hovers})"), format!("({ca}, {cx}, {cy}, {ch})"));
                }
                if redraw {
                    self.tally.add("restyled by the pointer");
                }
                redraw
            }
            _ => false,
        }
    }

    fn leave(&mut self) -> bool {
        match self.question("leave") {
            Some(Q::Leave(redraw)) => redraw,
            _ => false,
        }
    }

    fn cursor(&mut self) -> Vec<u8> {
        match self.question("cursor") {
            Some(Q::Cursor(c)) => c,
            _ => Vec::new(),
        }
    }

    fn editor_style(&mut self, id: i64, _clip: Rect) -> Option<EditorStyle> {
        match self.question("editor_style") {
            Some(Q::EditorStyle { id: cid, style }) => {
                if cid != id {
                    self.mismatch("editor_style", id.to_string(), cid.to_string());
                }
                style
            }
            _ => None,
        }
    }

    fn image_of(&mut self, src: &[u8]) -> Option<(i32, f32, f32)> {
        match self.question("image_of") {
            Some(Q::ImageOf { src: csrc, answer }) => {
                if csrc != src {
                    self.mismatch("image_of", String::from_utf8_lossy(src).into(), String::from_utf8_lossy(&csrc).into());
                }
                answer
            }
            _ => None,
        }
    }

    fn image_ready(&mut self, id: i32, w: f32, h: f32) -> bool {
        match self.question("image_ready") {
            Some(Q::ImageReady { id: cid, w: cw, h: ch, changed }) => {
                if cid != id || cw.to_bits() != w.to_bits() || ch.to_bits() != h.to_bits() {
                    self.mismatch("image_ready", format!("({id}, {w}, {h})"), format!("({cid}, {cw}, {ch})"));
                }
                changed
            }
            _ => false,
        }
    }

    fn content_size(&mut self, id: i64) -> (f32, f32) {
        match self.question("content_size") {
            Some(Q::Content { id: cid, size }) => {
                if cid != id {
                    self.mismatch("content_size", id.to_string(), cid.to_string());
                }
                size
            }
            _ => (0.0, 0.0),
        }
    }

    fn take_pending(&mut self, out: &mut Vec<Json>) {
        if let Some(Q::Pending(list)) = self.question("take_pending") {
            for text in list {
                match Json::parse(&text) {
                    Some(m) => out.push(m),
                    None => self.error(format!("a pending message that does not parse: {}", String::from_utf8_lossy(&text))),
                }
            }
        }
    }

    fn runs_key(&mut self) -> [u64; 4] {
        match self.question("runs_key") {
            Some(Q::RunsKey(k)) => k,
            _ => [0; 4],
        }
    }

    fn text_runs(&mut self) -> Vec<TextRun> {
        match self.question("text_runs") {
            Some(Q::Runs(runs)) => runs,
            _ => Vec::new(),
        }
    }

    fn text_width(&mut self, text: &[u8], font: u64) -> f32 {
        match self.question("text_width") {
            Some(Q::Width { text: ct, font: cf, w }) => {
                if ct != text || cf != font {
                    self.mismatch(
                        "text_width",
                        format!("({:?}, {font})", String::from_utf8_lossy(text)),
                        format!("({:?}, {cf})", String::from_utf8_lossy(&ct)),
                    );
                }
                w
            }
            _ => 0.0,
        }
    }

    fn font_info(&mut self, font: u64) -> Option<(i32, f32, f32, f32)> {
        match self.question("font_info") {
            Some(Q::FontInfo { font: cf, info }) => {
                if cf != font {
                    self.mismatch("font_info", font.to_string(), cf.to_string());
                }
                info
            }
            _ => None,
        }
    }

    fn layer_shown(&mut self, id: i64, now: f64) -> Option<(Affine, f32)> {
        match self.question("layer_shown") {
            Some(Q::Layer { id: cid, now: cn, shown }) => {
                if cid != id || cn.to_bits() != now.to_bits() {
                    self.mismatch("layer_shown", format!("({id}, {now})"), format!("({cid}, {cn})"));
                }
                shown
            }
            _ => None,
        }
    }

    fn selectable_key(&mut self) -> [u64; 2] {
        match self.question("selectable_key") {
            Some(Q::SelectableKey(k)) => k,
            _ => [0; 2],
        }
    }

    fn any_selectable(&mut self) -> bool {
        match self.question("any_selectable") {
            Some(Q::Selectable(yes)) => yes,
            _ => false,
        }
    }
}
