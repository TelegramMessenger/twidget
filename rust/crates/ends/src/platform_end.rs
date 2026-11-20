//! `PlatformEnd` (`core/src/platform_end.cpp`): a message is decoded and checked whole before the Backend or
//! the host sees any of it, and the first refusal stops the end for good. The C++ holds its Backend, host and
//! channel by reference; here it owns them, and lends them out.

use std::collections::VecDeque;

use twidget_json::{Json, Surrogates};
use twidget_remote::{
    CanvasRelease, CanvasRun, Caps, ClaimAnswer, ClaimQuery, ClearSelection, FontAnswer, FontRelease, FontRequest, Frame, FrameRequest,
    Hover, ImageAnswer, ImageRequest, Kind, Message, Messages, Paint, PixelsAnswer, PixelsRequest, Pointer, Selection, SetMaxHeight,
    SetTheme, SetWidth, State, Touch, WidthAnswer, WidthRequest, is_platform_message, kind_of,
};
use twidget_wire::FontMetrics;

use crate::{Backend, Channel};

const JSON_DEPTH: i32 = 4;
const PIXELS_ANSWER_HEAD: usize = 2 + 1 + 4;

/// What the platform end gives its host.
pub trait Host {
    /// The answer to one `request_frame()`, unchanged ones included; the host paints its fonts, images and
    /// canvases before it passes on later traffic.
    fn frame(&mut self, frame: Frame);
    fn state(&mut self, state: &State);
    fn messages(&mut self, messages: Vec<Json>);
    fn pointer(&mut self, pointer: &Pointer);
    fn selection(&mut self, selection: &Selection);
    fn claim(&mut self, answer: &ClaimAnswer);
    /// Called once, when the end stops for good: the host stops the widget.
    fn refused(&mut self, reason: &str);
}

#[derive(Clone, Copy, Debug)]
struct Expected {
    kind: Kind,
    seq: u64,
}

fn platform_message(json: &Json) -> bool {
    json.find("type").is_some_and(|t| t.is_string() && is_platform_message(t.as_str()))
}

fn named(kind: Kind) -> String {
    format!("kind {}", kind as u8)
}

/// The platform end: it checks every message of a renderer end it does not trust, calls the host's Backend,
/// and gives the host frames and reports.
#[derive(Debug)]
pub struct PlatformEnd<B: Backend, H: Host, C: Channel> {
    backend: B,
    host: H,
    channel: C,
    caps: Caps,
    state: State,
    expected: VecDeque<Expected>,
    frames: u64,
    claims: u64,
    refused: bool,
}

impl<B: Backend, H: Host, C: Channel> PlatformEnd<B, H, C> {
    pub fn new(backend: B, host: H, channel: C, caps: Caps) -> PlatformEnd<B, H, C> {
        PlatformEnd {
            backend,
            host,
            channel,
            caps,
            state: State::default(),
            expected: VecDeque::new(),
            frames: 0,
            claims: 0,
            refused: false,
        }
    }

    pub fn backend(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn host(&mut self) -> &mut H {
        &mut self.host
    }

    pub fn channel(&mut self) -> &mut C {
        &mut self.channel
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn refusing(&self) -> bool {
        self.refused
    }

    fn send<M: Message>(&mut self, message: &M, expected: Option<Expected>) -> bool {
        if self.refused {
            return false;
        }
        let mut bytes = Vec::new();
        if !message.encode(&self.caps, &mut bytes) {
            return self.refuse("an input did not encode within the caps");
        }
        if let Some(expected) = expected {
            self.expected.push_back(expected);
        }
        self.channel.send(bytes);
        true
    }

    fn take<M: Message>(&mut self, message: &[u8], kind: Kind) -> Option<M> {
        let decoded = M::decode(message, &self.caps);
        if decoded.is_none() {
            self.refuse(&format!("a message of {} did not decode", named(kind)));
        }
        decoded
    }

    fn respond(&mut self, encode_answer: impl FnOnce(&Caps, &mut Vec<u8>) -> bool, reply: &mut Vec<u8>) -> bool {
        if self.refused {
            return false;
        }
        let mut bytes = Vec::new();
        if !encode_answer(&self.caps, &mut bytes) {
            return self.refuse("an answer did not encode within the caps");
        }
        *reply = bytes;
        true
    }

    fn solicited(&mut self, kind: Kind, seq: u64) -> bool {
        let Some(front) = self.expected.front().copied() else {
            return self.refuse(&format!("an unsolicited message of {}", named(kind)));
        };
        if front.kind != kind {
            return self.refuse(&format!("a message of {} where {} was due", named(kind), named(front.kind)));
        }
        if front.seq != seq {
            return self.refuse(&format!("a message of {} for request {seq} where {} was due", named(kind), front.seq));
        }
        self.expected.pop_front();
        true
    }

    fn refuse(&mut self, reason: &str) -> bool {
        if self.refused {
            return false;
        }
        self.refused = true;
        self.expected.clear();
        self.host.refused(reason);
        false
    }

    /// Asks the renderer end for a frame; its seq, or 0 once the end refuses.
    pub fn request_frame(&mut self, tick: bool, paint: Paint) -> u64 {
        let next = Expected { kind: Kind::Frame, seq: self.frames + 1 };
        if !self.send(&FrameRequest { seq: next.seq, tick, paint }, Some(next)) {
            return 0;
        }
        self.frames = next.seq;
        self.frames
    }

    pub fn set_width(&mut self, width: f64) {
        self.send(&SetWidth { width }, None);
    }

    pub fn set_max_height(&mut self, height: f64) {
        self.send(&SetMaxHeight { height }, None);
    }

    pub fn set_theme(&mut self, dark: bool) {
        self.send(&SetTheme { dark }, None);
    }

    pub fn touch(&mut self, action: i32, x: f64, y: f64, pointer: i32, primary: bool, mouse: bool, modifiers: i32, button: i32) {
        let report = Expected { kind: Kind::Pointer, seq: 0 };
        self.send(&Touch { action, x, y, pointer, primary, mouse, modifiers, button }, Some(report));
    }

    pub fn hover(&mut self, action: i32, x: f64, y: f64, modifiers: i32) {
        let report = Expected { kind: Kind::Pointer, seq: 0 };
        self.send(&Hover { action, x, y, modifiers }, Some(report));
    }

    pub fn clear_selection(&mut self) {
        self.send(&ClearSelection, None);
    }

    pub fn ask_claim(&mut self, x: f64, y: f64) -> u64 {
        let next = Expected { kind: Kind::ClaimAnswer, seq: self.claims + 1 };
        if !self.send(&ClaimQuery { seq: next.seq, x, y }, Some(next)) {
            return 0;
        }
        self.claims = next.seq;
        self.claims
    }

    /// A one-way message of the renderer end; false once the end refuses.
    pub fn receive(&mut self, message: &[u8]) -> bool {
        if self.refused {
            return false;
        }
        let Some(kind) = kind_of(message) else { return self.refuse("a message of no known kind or version") };
        match kind {
            Kind::Frame => {
                let Some(frame) = self.take::<Frame>(message, kind) else { return false };
                if !self.solicited(kind, frame.seq) {
                    return false;
                }
                self.state = frame.state;
                self.host.frame(frame);
            }
            Kind::State => {
                let Some(state) = self.take::<State>(message, kind) else { return false };
                self.state = state;
                self.host.state(&self.state);
            }
            Kind::Messages => {
                if !self.take_messages(message) {
                    return false;
                }
            }
            Kind::Pointer => {
                let Some(pointer) = self.take::<Pointer>(message, kind) else { return false };
                if !self.solicited(kind, 0) {
                    return false;
                }
                self.host.pointer(&pointer);
            }
            Kind::Selection => {
                let Some(selection) = self.take::<Selection>(message, kind) else { return false };
                self.host.selection(&selection);
            }
            Kind::ClaimAnswer => {
                let Some(claim) = self.take::<ClaimAnswer>(message, kind) else { return false };
                if !self.solicited(kind, claim.seq) {
                    return false;
                }
                self.host.claim(&claim);
            }
            Kind::FontRelease => {
                let Some(release) = self.take::<FontRelease>(message, kind) else { return false };
                self.backend.delete_font(release.font);
            }
            Kind::CanvasRun => {
                let Some(run) = self.take::<CanvasRun>(message, kind) else { return false };
                self.backend.canvas_run(run.id, run.w, run.h, &run.ops);
            }
            Kind::CanvasRelease => {
                let Some(release) = self.take::<CanvasRelease>(message, kind) else { return false };
                self.backend.canvas_release(release.id);
            }
            Kind::FontRequest | Kind::WidthRequest | Kind::ImageRequest | Kind::PixelsRequest => {
                return self.refuse(&format!("a request of {} where a one-way message belongs", named(kind)));
            }
            _ => return self.refuse(&format!("a message of {}, which only the platform end sends", named(kind))),
        }
        !self.refused
    }

    fn take_messages(&mut self, message: &[u8]) -> bool {
        let Some(batch) = self.take::<Messages>(message, Kind::Messages) else { return false };
        let mut parsed = Vec::with_capacity(batch.texts.len());
        for text in &batch.texts {
            match Json::parse_with(text, Surrogates::Replace, JSON_DEPTH) {
                Some(json) if platform_message(&json) => parsed.push(json),
                _ => return self.refuse("a report that is not one of the five platform messages"),
            }
        }
        self.host.messages(parsed);
        true
    }

    /// A request of the renderer end, answered into `reply`; false once the end refuses.
    pub fn answer(&mut self, request: &[u8], reply: &mut Vec<u8>) -> bool {
        if self.refused {
            return false;
        }
        let Some(kind) = kind_of(request) else { return self.refuse("a request of no known kind or version") };
        match kind {
            Kind::FontRequest => {
                let Some(m) = self.take::<FontRequest>(request, kind) else { return false };
                let mut metrics = FontMetrics::default();
                let font = if m.canvas {
                    self.backend.create_canvas_font(&m.families, m.size, m.weight, m.italic, &mut metrics)
                } else {
                    self.backend.create_font(&m.families, m.size, m.weight, m.italic, &m.features, &mut metrics)
                };
                self.respond(|caps, bytes| FontAnswer { font, metrics }.encode(caps, bytes), reply)
            }
            Kind::WidthRequest => {
                let Some(m) = self.take::<WidthRequest>(request, kind) else { return false };
                let width = self.backend.text_width(m.font, &m.text);
                self.respond(|caps, bytes| WidthAnswer { width }.encode(caps, bytes), reply)
            }
            Kind::ImageRequest => {
                let Some(m) = self.take::<ImageRequest>(request, kind) else { return false };
                let (mut w, mut h) = (0.0, 0.0);
                let ok = self.backend.load_image(m.id, &m.url, &mut w, &mut h);
                self.respond(|caps, bytes| ImageAnswer { ok, w, h }.encode(caps, bytes), reply)
            }
            Kind::PixelsRequest => self.answer_pixels(request, reply),
            Kind::Frame
            | Kind::State
            | Kind::Messages
            | Kind::Pointer
            | Kind::Selection
            | Kind::ClaimAnswer
            | Kind::FontRelease
            | Kind::CanvasRun
            | Kind::CanvasRelease => self.refuse(&format!("a one-way message of {} where a request belongs", named(kind))),
            _ => self.refuse(&format!("a message of {}, which only the platform end sends", named(kind))),
        }
    }

    fn answer_pixels(&mut self, request: &[u8], reply: &mut Vec<u8>) -> bool {
        let Some(m) = self.take::<PixelsRequest>(request, Kind::PixelsRequest) else { return false };
        let bytes = (m.w as u64).wrapping_mul(m.h as u64).wrapping_mul(4);
        let fits = bytes <= self.caps.values.pixels as u64
            && self.caps.message >= PIXELS_ANSWER_HEAD
            && bytes <= (self.caps.message - PIXELS_ANSWER_HEAD) as u64;
        let mut pixels = PixelsAnswer::default();
        if fits {
            let mut rgba = Vec::new();
            if self.backend.canvas_read(m.id, m.x, m.y, m.w, m.h, &mut rgba) && rgba.len() as u64 == bytes {
                pixels.ok = true;
                pixels.rgba = rgba;
            }
        }
        self.respond(|caps, bytes| pixels.encode(caps, bytes), reply)
    }
}
