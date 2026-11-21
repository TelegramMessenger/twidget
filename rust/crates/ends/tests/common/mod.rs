//! What the ends' tests share, as tests/log_backend.h, loopback.h and reports.h have it: a Backend that
//! answers every call the same way each run and logs it, both ends in one process over an in-memory channel
//! that can spoil an answer, and the reports a step must make. The renderer is the scripted one, or another a test
//! opens the loop on.

#![allow(dead_code)]

pub mod scripted;

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use scripted::{Log, ScriptedRenderer, f32s, hex, metrics_line};
use twidget_ends::{Backend, Channel, Host, Output, PlatformEnd, Proxy, Renderer, RendererChannel, RendererEnd};
use twidget_json::Json;
use twidget_json::quote;
use twidget_remote::*;
use twidget_wire::{CanvasOp, FontMetrics, Rect, encode_batch};

// ---- log_backend.h ------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Op {
    Font,
    CanvasFont,
    DeleteFont,
    TextWidth,
    Image,
    CanvasRun,
    CanvasRead,
    CanvasRelease,
    #[default]
    Frame,
}

/// One Backend call: its arguments, what it found in its outputs, what it gave back.
#[derive(Clone, Debug, Default)]
pub struct Call {
    pub op: Op,
    pub families: Vec<u8>,
    pub features: Vec<u8>,
    pub text: Vec<u8>,
    pub url: Vec<u8>,
    pub size: f32,
    pub weight: i32,
    pub italic: bool,
    pub font: i32,
    pub metrics_in: FontMetrics,
    pub metrics: FontMetrics,
    pub width: f32,
    pub id: i32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub w_in: f32,
    pub h_in: f32,
    pub image_w: f32,
    pub image_h: f32,
    pub ok: bool,
    pub painted: bool,
    pub rgba_in: usize,
    pub rgba: Vec<u8>,
    pub ops: Vec<CanvasOp>,
}

impl Call {
    /// Every member, floats as bits, a canvas run's ops in their wire form (which keeps their sharing); the
    /// strings quoted as json.h quotes them, whatever bytes they hold.
    pub fn line(&self) -> Vec<u8> {
        let mut batch = Vec::new();
        encode_batch(&self.ops, &twidget_wire::Caps::default(), &mut batch);
        let mut out = format!("{:?}", self.op).into_bytes();
        for s in [&self.families, &self.features, &self.text, &self.url] {
            out.push(b' ');
            out.extend_from_slice(&quote(s));
        }
        let rest = format!(
            " {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
            f32s(self.size),
            self.weight,
            u8::from(self.italic),
            self.font,
            metrics_line(&self.metrics_in),
            metrics_line(&self.metrics),
            f32s(self.width),
            self.id,
            self.x,
            self.y,
            self.w,
            self.h,
            f32s(self.w_in),
            f32s(self.h_in),
            f32s(self.image_w),
            f32s(self.image_h),
            u8::from(self.ok),
            u8::from(self.painted),
            self.rgba_in,
            hex(&self.rgba),
            hex(&batch)
        );
        out.extend_from_slice(rest.as_bytes());
        out
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Read {
    #[default]
    Pixels,
    Unsupported,
    WrongSize,
}

pub fn metrics_for(size: f32, weight: i32, italic: bool) -> FontMetrics {
    let mut m = FontMetrics {
        ascent: size * 0.75,
        descent: size * 0.25,
        height: size * 1.25,
        x_height: size * 0.5,
        zero_width: size * if weight >= 600 { 0.625 } else { 0.5 },
        ..FontMetrics::default()
    };
    if !italic {
        m.em_ascent = size * 0.8125;
        m.em_descent = size - m.em_ascent;
    }
    m
}

#[derive(Default)]
pub struct LogBackend {
    pub images_load: bool,
    pub reads: Read,
    /// The Backend that answers in its place, every call logged with what it gave (log_backend.h's `answers`).
    pub answers: Option<Box<dyn Backend + Send>>,
    pub calls: Vec<Call>,
    next_font: i32,
    sizes: BTreeMap<i32, f32>,
}

impl std::fmt::Debug for LogBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogBackend")
            .field("images_load", &self.images_load)
            .field("reads", &self.reads)
            .field("answers", &self.answers.is_some())
            .field("calls", &self.calls.len())
            .finish_non_exhaustive()
    }
}

impl LogBackend {
    pub fn new() -> LogBackend {
        LogBackend { images_load: true, ..LogBackend::default() }
    }

    fn made(&mut self, op: Op, families: &[u8], size: f32, weight: i32, italic: bool, features: &[u8], out: &mut FontMetrics) -> i32 {
        let mut c = Call {
            op,
            families: families.to_vec(),
            features: features.to_vec(),
            size,
            weight,
            italic,
            metrics_in: *out,
            ..Call::default()
        };
        if let Some(answers) = self.answers.as_mut() {
            c.font = if op == Op::CanvasFont {
                answers.create_canvas_font(families, size, weight, italic, out)
            } else {
                answers.create_font(families, size, weight, italic, features, out)
            };
        } else {
            *out = metrics_for(size, weight, italic);
            self.next_font += 1;
            c.font = self.next_font;
        }
        c.metrics = *out;
        if c.font != 0 {
            self.sizes.insert(c.font, size);
        }
        let font = c.font;
        self.calls.push(c);
        font
    }

    pub fn frame(&mut self, painted: bool) {
        self.calls.push(Call { op: Op::Frame, painted, ..Call::default() });
    }

    pub fn count(&self, op: Op) -> usize {
        self.calls.iter().filter(|c| c.op == op).count()
    }

    pub fn live_fonts(&self) -> usize {
        self.sizes.len()
    }
}

impl Backend for LogBackend {
    fn create_font(&mut self, families: &[u8], size_px: f32, weight: i32, italic: bool, features: &[u8], out: &mut FontMetrics) -> i32 {
        self.made(Op::Font, families, size_px, weight, italic, features, out)
    }

    fn create_canvas_font(&mut self, families: &[u8], size_px: f32, weight: i32, italic: bool, out: &mut FontMetrics) -> i32 {
        self.made(Op::CanvasFont, families, size_px, weight, italic, b"", out)
    }

    fn delete_font(&mut self, font: i32) {
        self.sizes.remove(&font);
        self.calls.push(Call { op: Op::DeleteFont, font, ..Call::default() });
        if let Some(answers) = self.answers.as_mut() {
            answers.delete_font(font);
        }
    }

    fn text_width(&mut self, font: i32, text: &[u8]) -> f32 {
        let width = match self.answers.as_mut() {
            Some(answers) => answers.text_width(font, text),
            None => self.sizes.get(&font).map_or(0.0, |size| size * 0.5 * text.len() as f32),
        };
        self.calls.push(Call { op: Op::TextWidth, font, text: text.to_vec(), width, ..Call::default() });
        width
    }

    fn load_image(&mut self, id: i32, url: &[u8], w: &mut f32, h: &mut f32) -> bool {
        let mut c = Call { op: Op::Image, id, url: url.to_vec(), w_in: *w, h_in: *h, ..Call::default() };
        if let Some(answers) = self.answers.as_mut() {
            c.ok = answers.load_image(id, url, w, h);
        } else {
            c.ok = self.images_load;
            *w = if self.images_load { (8 + url.len() % 32) as f32 } else { 2.0 };
            *h = if self.images_load { (4 + url.len() % 16) as f32 } else { 3.0 };
        }
        c.image_w = *w;
        c.image_h = *h;
        let ok = c.ok;
        self.calls.push(c);
        ok
    }

    fn canvas_run(&mut self, id: i32, w: i32, h: i32, ops: &[CanvasOp]) {
        self.calls.push(Call { op: Op::CanvasRun, id, w, h, ops: ops.to_vec(), ..Call::default() });
        if let Some(answers) = self.answers.as_mut() {
            answers.canvas_run(id, w, h, ops);
        }
    }

    fn canvas_read(&mut self, id: i32, x: i32, y: i32, w: i32, h: i32, rgba: &mut Vec<u8>) -> bool {
        let mut c = Call { op: Op::CanvasRead, id, x, y, w, h, rgba_in: rgba.len(), ..Call::default() };
        let bytes = w.max(0) as usize * h.max(0) as usize * 4;
        if let Some(answers) = self.answers.as_mut() {
            c.ok = answers.canvas_read(id, x, y, w, h, rgba);
            c.rgba = rgba.clone();
            let ok = c.ok;
            self.calls.push(c);
            return ok;
        }
        c.ok = self.reads != Read::Unsupported;
        match self.reads {
            Read::Pixels => {
                rgba.resize(bytes, 0);
                for (i, v) in rgba.iter_mut().enumerate() {
                    *v = (id as usize)
                        .wrapping_mul(31)
                        .wrapping_add((x as usize).wrapping_mul(7))
                        .wrapping_add((y as usize).wrapping_mul(13))
                        .wrapping_add(i) as u8;
                }
            }
            Read::WrongSize => *rgba = vec![0x7f; bytes + 4],
            Read::Unsupported => {}
        }
        c.rgba = rgba.clone();
        let ok = c.ok;
        self.calls.push(c);
        ok
    }

    fn canvas_release(&mut self, id: i32) {
        self.calls.push(Call { op: Op::CanvasRelease, id, ..Call::default() });
        if let Some(answers) = self.answers.as_mut() {
            answers.canvas_release(id);
        }
    }
}

/// A LogBackend more than one owner calls.
#[derive(Clone, Debug, Default)]
pub struct LogHandle(pub Rc<RefCell<LogBackend>>);

impl LogHandle {
    pub fn new() -> LogHandle {
        LogHandle(Rc::new(RefCell::new(LogBackend::new())))
    }

    pub fn count(&self, op: Op) -> usize {
        self.0.borrow().count(op)
    }

    pub fn calls(&self) -> Vec<Call> {
        self.0.borrow().calls.clone()
    }

    pub fn len(&self) -> usize {
        self.0.borrow().calls.len()
    }
}

impl Backend for LogHandle {
    fn create_font(&mut self, families: &[u8], size_px: f32, weight: i32, italic: bool, features: &[u8], out: &mut FontMetrics) -> i32 {
        self.0.borrow_mut().create_font(families, size_px, weight, italic, features, out)
    }
    fn create_canvas_font(&mut self, families: &[u8], size_px: f32, weight: i32, italic: bool, out: &mut FontMetrics) -> i32 {
        self.0.borrow_mut().create_canvas_font(families, size_px, weight, italic, out)
    }
    fn delete_font(&mut self, font: i32) {
        self.0.borrow_mut().delete_font(font)
    }
    fn text_width(&mut self, font: i32, text: &[u8]) -> f32 {
        self.0.borrow_mut().text_width(font, text)
    }
    fn load_image(&mut self, id: i32, url: &[u8], w: &mut f32, h: &mut f32) -> bool {
        self.0.borrow_mut().load_image(id, url, w, h)
    }
    fn canvas_run(&mut self, id: i32, w: i32, h: i32, ops: &[CanvasOp]) {
        self.0.borrow_mut().canvas_run(id, w, h, ops)
    }
    fn canvas_read(&mut self, id: i32, x: i32, y: i32, w: i32, h: i32, rgba: &mut Vec<u8>) -> bool {
        self.0.borrow_mut().canvas_read(id, x, y, w, h, rgba)
    }
    fn canvas_release(&mut self, id: i32) {
        self.0.borrow_mut().canvas_release(id)
    }
}

/// Whether two logs hold the same calls in the same order member by member; the first difference otherwise.
pub fn same_calls(a: &[Call], b: &[Call]) -> Result<(), String> {
    for i in 0..a.len().max(b.len()) {
        if i == a.len() || i == b.len() {
            return Err(format!("call {i} is in one log only, of {} and {}", a.len(), b.len()));
        }
        let (x, y) = (a[i].line(), b[i].line());
        if x != y {
            return Err(format!("call {i}: {} / {}", String::from_utf8_lossy(&x), String::from_utf8_lossy(&y)));
        }
    }
    Ok(())
}

// ---- loopback.h ---------------------------------------------------------------------------------

/// The platform end's channel: its input waits here until `pump()`.
#[derive(Clone, Debug, Default)]
pub struct Queue(pub Rc<RefCell<VecDeque<Vec<u8>>>>);

impl Channel for Queue {
    fn send(&mut self, message: Vec<u8>) {
        self.0.borrow_mut().push_back(message);
    }
}

/// What the platform end gives its host, in order; a frame also marks the Backend's log.
#[derive(Debug, Default)]
pub struct Heard {
    pub events: Vec<Kind>,
    pub frames: Vec<Frame>,
    pub states: Vec<State>,
    pub batches: Vec<Vec<Json>>,
    pub pointers: Vec<Pointer>,
    pub selections: Vec<Selection>,
    pub claims: Vec<ClaimAnswer>,
    pub refusals: Vec<String>,
    /// Each as the oracle's transcript writes it.
    pub lines: Vec<Vec<u8>>,
}

pub fn state_line(s: &State) -> String {
    format!(
        "{} {} {} {} {} {}",
        scripted::f64s(s.width),
        s.height,
        u8::from(s.dirty),
        u8::from(s.wants_tick),
        scripted::f64s(s.next_tick_in),
        u8::from(s.frame_asked)
    )
}

pub fn claim_line(c: &GestureClaim) -> String {
    format!("{}{}{}", u8::from(c.x), u8::from(c.y), u8::from(c.hold))
}

#[derive(Clone, Debug)]
pub struct HeardHandle {
    pub heard: Rc<RefCell<Heard>>,
    pub backend: LogHandle,
}

impl Host for HeardHandle {
    fn frame(&mut self, frame: Frame) {
        let mut h = self.heard.borrow_mut();
        h.events.push(Kind::Frame);
        self.backend.0.borrow_mut().frame(frame.painted);
        let line = format!(
            "frame {} {} {} {} {}",
            frame.seq,
            u8::from(frame.painted),
            state_line(&frame.state),
            hex(&list_bytes(&frame.list)),
            hex(&damage_bytes(&frame.damage))
        );
        h.lines.push(line.into_bytes());
        h.frames.push(frame);
    }
    fn state(&mut self, state: &State) {
        let mut h = self.heard.borrow_mut();
        h.events.push(Kind::State);
        h.lines.push(format!("state {}", state_line(state)).into_bytes());
        h.states.push(*state);
    }
    fn messages(&mut self, messages: Vec<Json>) {
        let mut h = self.heard.borrow_mut();
        h.events.push(Kind::Messages);
        let mut line = b"messages ".to_vec();
        for (i, m) in messages.iter().enumerate() {
            if i > 0 {
                line.push(b' ');
            }
            line.extend_from_slice(&m.dump());
        }
        h.lines.push(line);
        h.batches.push(messages);
    }
    fn pointer(&mut self, pointer: &Pointer) {
        let mut h = self.heard.borrow_mut();
        h.events.push(Kind::Pointer);
        let mut line = b"pointer ".to_vec();
        line.extend_from_slice(&quote(&pointer.cursor));
        line.extend_from_slice(format!(" {}", pointer.claim.as_ref().map_or("-".to_string(), claim_line)).as_bytes());
        h.lines.push(line);
        h.pointers.push(pointer.clone());
    }
    fn selection(&mut self, selection: &Selection) {
        let mut h = self.heard.borrow_mut();
        h.events.push(Kind::Selection);
        let mut line = b"selection ".to_vec();
        line.extend_from_slice(&quote(&selection.text));
        h.lines.push(line);
        h.selections.push(selection.clone());
    }
    fn claim(&mut self, answer: &ClaimAnswer) {
        let mut h = self.heard.borrow_mut();
        h.events.push(Kind::ClaimAnswer);
        h.lines.push(format!("claim {} {}", answer.seq, claim_line(&answer.claim)).into_bytes());
        h.claims.push(*answer);
    }
    fn refused(&mut self, reason: &str) {
        let mut h = self.heard.borrow_mut();
        let mut line = b"refused ".to_vec();
        line.extend_from_slice(&quote(reason));
        h.lines.push(line);
        h.refusals.push(reason.to_string());
    }
}

pub type Platform = PlatformEnd<LogHandle, HeardHandle, Queue>;

/// What Wire does to the answer of the request a Fault names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Spoil {
    #[default]
    None,
    Unanswered,
    Empty,
    WrongKind,
    WrongVersion,
    Cut,
    Trailing,
    Replaced,
}

/// The request a Spoil hits: number `at` of the session, from 1, or else the next one of `kind`.
#[derive(Clone, Debug, Default)]
pub struct Fault {
    pub how: Spoil,
    pub at: usize,
    pub kind: Option<Kind>,
    pub reply: Vec<u8>,
}

/// The renderer end's side of the channel.
#[derive(Debug, Default)]
pub struct WireState {
    pub outbox: VecDeque<Vec<u8>>,
    /// Every message of the renderer end, requests included, by kind byte.
    pub kinds: Vec<u8>,
    pub requests: usize,
    /// Messages and requests the platform end refused.
    pub refused: usize,
    pub fault: Fault,
    /// How many messages the renderer end had sent with the spoiled request, 0 before one.
    pub spoiled: usize,
    /// The traffic as the oracle's transcript writes it: `>` a message, `?` a request, `!` its answer.
    pub traffic: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Wire {
    pub state: Rc<RefCell<WireState>>,
    pub platform: Rc<RefCell<Platform>>,
}

impl Wire {
    /// Hands the platform end what the renderer end sent, in order.
    pub fn flush(&self) {
        loop {
            let Some(message) = self.state.borrow_mut().outbox.pop_front() else { break };
            if !self.platform.borrow_mut().receive(&message) {
                self.state.borrow_mut().refused += 1;
            }
        }
    }

    fn spoil(fault: &Fault, reply: &mut Vec<u8>) {
        match fault.how {
            Spoil::Empty => reply.clear(),
            Spoil::WrongKind => {
                reply[0] = (i32::from(Kind::FontAnswer as u8) + (i32::from(reply[0]) - i32::from(Kind::FontAnswer as u8) + 1) % 4) as u8
            }
            Spoil::WrongVersion => reply[1] = VERSION + 1,
            Spoil::Cut => {
                reply.pop();
            }
            Spoil::Trailing => reply.push(0),
            Spoil::Replaced => *reply = fault.reply.clone(),
            Spoil::None | Spoil::Unanswered => {}
        }
    }
}

fn kind_byte(message: &[u8]) -> u8 {
    message.first().copied().unwrap_or(0)
}

impl Channel for Wire {
    fn send(&mut self, message: Vec<u8>) {
        let mut s = self.state.borrow_mut();
        s.kinds.push(kind_byte(&message));
        s.traffic.push(format!("> {}", hex(&message)));
        s.outbox.push_back(message);
    }
}

impl RendererChannel for Wire {
    fn request(&mut self, message: Vec<u8>, answer: &mut Vec<u8>) -> bool {
        let asked = kind_byte(&message);
        let now = {
            let mut s = self.state.borrow_mut();
            s.kinds.push(asked);
            s.traffic.push(format!("? {}", hex(&message)));
            s.requests += 1;
            Fault::default()
        };
        self.flush();
        let now = {
            let mut s = self.state.borrow_mut();
            let hit = s.fault.how != Spoil::None
                && if s.fault.at != 0 { s.fault.at == s.requests } else { s.fault.kind.map(|k| k as u8) == Some(asked) };
            if hit {
                s.spoiled = s.kinds.len();
                std::mem::take(&mut s.fault)
            } else {
                now
            }
        };
        if now.how == Spoil::Unanswered {
            self.state.borrow_mut().traffic.push("! -".to_string());
            return false;
        }
        let mut reply = Vec::new();
        if !self.platform.borrow_mut().answer(&message, &mut reply) {
            let mut s = self.state.borrow_mut();
            s.refused += 1;
            s.traffic.push("! refused".to_string());
            return false;
        }
        Wire::spoil(&now, &mut reply);
        self.state.borrow_mut().traffic.push(format!("! {}", hex(&reply)));
        *answer = reply;
        true
    }
}

pub type End = RendererEnd<Wire, ScriptedRenderer<Proxy<Wire>>>;

/// The backend, the host, the queue, the platform end, the channel and then the renderer end, made and
/// dropped in that order. The renderer is the scripted one unless a test opens the end on another.
pub struct Loopback<R: Renderer = ScriptedRenderer<Proxy<Wire>>> {
    pub caps: Caps,
    pub backend: LogHandle,
    pub host: Rc<RefCell<Heard>>,
    pub queue: Queue,
    pub platform: Rc<RefCell<Platform>>,
    pub wire: Rc<RefCell<WireState>>,
    pub end: Option<RendererEnd<Wire, R>>,
    pub out: Output,
    pub delivered: Vec<bool>,
    pub outputs: Vec<Output>,
    pub log: Log,
}

impl Loopback {
    pub fn new(caps: Caps) -> Loopback {
        Loopback::made(caps)
    }

    pub fn open(&mut self, width: f64, dpr: f64, dark: bool, script: &Json) {
        let log = Rc::clone(&self.log);
        self.open_with(width, dpr, dark, |w, d, k, proxy| ScriptedRenderer::new(w, d, k, proxy, script, log));
    }
}

impl<R: Renderer> Loopback<R> {
    /// The loop before its end opens, on renderers of type `R`.
    pub fn made(caps: Caps) -> Loopback<R> {
        let backend = LogHandle::new();
        let host = Rc::new(RefCell::new(Heard::default()));
        let queue = Queue::default();
        let platform = Rc::new(RefCell::new(PlatformEnd::new(
            backend.clone(),
            HeardHandle { heard: Rc::clone(&host), backend: backend.clone() },
            queue.clone(),
            caps,
        )));
        Loopback {
            caps,
            backend,
            host,
            queue,
            platform,
            wire: Rc::new(RefCell::new(WireState::default())),
            end: None,
            out: Output::default(),
            delivered: Vec::new(),
            outputs: Vec::new(),
            log: Log::default(),
        }
    }

    pub fn channel(&self) -> Wire {
        Wire { state: Rc::clone(&self.wire), platform: Rc::clone(&self.platform) }
    }

    pub fn flush(&self) {
        self.channel().flush();
    }

    /// Opens the end on the renderer `make` builds on its proxy.
    pub fn open_with(&mut self, width: f64, dpr: f64, dark: bool, make: impl FnOnce(f64, f64, bool, Proxy<Wire>) -> R) {
        self.end = Some(RendererEnd::new(width, dpr, dark, self.channel(), self.caps, make));
        self.flush();
    }

    pub fn end(&mut self) -> &mut RendererEnd<Wire, R> {
        self.end.as_mut().expect("an open end")
    }

    /// Drops the renderer end while everything else lives, and hands on what it sent going.
    pub fn close(&mut self) {
        self.end = None;
        self.flush();
    }

    pub fn receive(&mut self, message: &Json) -> Vec<Json> {
        let out = self.end().receive(message);
        self.flush();
        out
    }

    pub fn want_frame(&mut self) {
        self.end().want_frame();
        self.flush();
    }

    pub fn report(&mut self, messages: &[Json]) {
        self.end().report(messages);
        self.flush();
    }

    /// Delivers the queued platform input one message at a time into the same Output, each call's traffic
    /// passed on after it returns; false past `steps` messages.
    pub fn pump_steps(&mut self, steps: usize) -> bool {
        self.flush();
        let mut i = 0;
        loop {
            let Some(message) = self.queue.0.borrow_mut().pop_front() else { return true };
            if i == steps || self.end.is_none() {
                self.queue.0.borrow_mut().push_front(message);
                return false;
            }
            let mut out = std::mem::take(&mut self.out);
            let ok = self.end.as_mut().expect("an open end").deliver(&message, &mut out);
            self.delivered.push(ok);
            self.outputs.push(out.clone());
            self.out = out;
            self.flush();
            i += 1;
        }
    }

    pub fn pump(&mut self) -> bool {
        self.pump_steps(64)
    }

    pub fn kinds(&self) -> Vec<u8> {
        self.wire.borrow().kinds.clone()
    }

    pub fn count_kind(&self, kind: Kind) -> usize {
        self.wire.borrow().kinds.iter().filter(|k| **k == kind as u8).count()
    }

    pub fn events(&self) -> Vec<Kind> {
        self.host.borrow().events.clone()
    }

    pub fn platform_state(&self) -> State {
        *self.platform.borrow().state()
    }

    pub fn refusing(&self) -> bool {
        self.platform.borrow().refusing()
    }
}

// ---- reports.h ----------------------------------------------------------------------------------

pub fn parse(s: &str) -> Json {
    Json::parse(s).unwrap_or_else(|| panic!("bad json: {s}"))
}

pub fn dumps(messages: &[Json]) -> String {
    messages.iter().map(|m| String::from_utf8_lossy(&m.dump()).into_owned() + "\n").collect()
}

pub fn platform_type(message: &Json) -> bool {
    is_platform_message(message.get("type").as_str())
}

pub fn platform_only(messages: &[Json]) -> Vec<Json> {
    messages.iter().filter(|m| platform_type(m)).cloned().collect()
}

pub fn same_state(a: &State, b: &State) -> bool {
    a.width.to_bits() == b.width.to_bits()
        && a.height == b.height
        && a.dirty == b.dirty
        && a.wants_tick == b.wants_tick
        && a.next_tick_in.to_bits() == b.next_tick_in.to_bits()
        && a.frame_asked == b.frame_asked
}

pub fn gesture_of(messages: &[Json]) -> Option<GestureClaim> {
    messages.iter().find(|m| m.get("type").as_str() == b"gesture").map(|m| GestureClaim {
        x: m.get("x").as_bool(false),
        y: m.get("y").as_bool(false),
        hold: m.get("hold").as_bool(false),
    })
}

pub fn encoded<M: Message>(message: &M) -> Vec<u8> {
    let mut bytes = Vec::new();
    assert!(message.encode(&Caps::default(), &mut bytes), "a test message encodes");
    bytes
}

/// A display list as the wire carries it: every member, the payloads' sharing.
pub fn list_bytes(list: &[twidget_wire::Command]) -> Vec<u8> {
    let mut out = Vec::new();
    twidget_wire::encode_list(list, &twidget_wire::Caps::default(), &mut out);
    out
}

pub fn damage_bytes(damage: &Option<Vec<Rect>>) -> Vec<u8> {
    let mut out = Vec::new();
    twidget_wire::encode_damage(damage, &twidget_wire::Caps::default(), &mut out);
    out
}

/// What a step's renderer gave, and the reports the host must hear of it in order.
#[derive(Clone, Debug, Default)]
pub struct Expected {
    pub messages: Vec<Json>,
    pub engine: bool,
    pub seq: u64,
    pub pointer: Option<Pointer>,
    pub frame: Option<Frame>,
    pub claim: Option<GestureClaim>,
    pub selection: Vec<u8>,
    pub state: State,
    pub events: Vec<Kind>,
}

pub fn predict(s: &mut Expected, selection: &[u8], sent: &State) {
    if !platform_only(&s.messages).is_empty() {
        s.events.push(Kind::Messages);
    }
    if s.pointer.is_some() {
        s.events.push(Kind::Pointer);
    }
    if s.selection != selection {
        s.events.push(Kind::Selection);
    }
    if let Some(frame) = &mut s.frame {
        frame.state = s.state;
        s.events.push(Kind::Frame);
    } else {
        if s.claim.is_some() {
            s.events.push(Kind::ClaimAnswer);
        }
        if !same_state(sent, &s.state) {
            s.events.push(Kind::State);
        }
    }
}

/// Keeps a payload's sharing visible to a comparison: the Arc addresses of a list, in first-seen order.
pub fn sharing(list: &[twidget_wire::Command]) -> Vec<usize> {
    let mut seen: Vec<usize> = Vec::new();
    let mut out = Vec::new();
    for c in list {
        for p in
            [c.path.as_ref().map(|p| Arc::as_ptr(p) as usize), c.filter.as_ref().map(|f| Arc::as_ptr(f) as usize)].into_iter().flatten()
        {
            let at = seen.iter().position(|s| *s == p).unwrap_or_else(|| {
                seen.push(p);
                seen.len() - 1
            });
            out.push(at);
        }
    }
    out
}
