//! `RendererEnd` (`core/src/renderer_end.cpp`): each Backend call of the renderer becomes a message or a
//! request that waits, each call ends with the reports in a fixed order, and the first failure stops the end
//! for good.
//!
//! The C++ end owns a `Proxy` that holds a reference back to it, and the renderer it makes holds the proxy;
//! here what the proxy needs of the end (the channel, the caps and the stop reason) is a [`Link`] the end
//! and its [`Proxy`] share, and the renderer owns the proxy. Neither holds the link across a renderer call.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use twidget_json::Json;
use twidget_remote::{
    CanvasRelease, Caps, ClaimAnswer, ClaimQuery, ClearSelection, DeltaRun, FontAnswer, FontRelease, FontRequest, Frame, FrameDelta,
    FrameRequest, GestureClaim, Hover, ImageAnswer, ImageRequest, Kind, Message, Messages, Paint, PixelsAnswer, PixelsRequest, Pointer,
    Selection, SetMaxHeight, SetTheme, SetWidth, State, TextInFont, Touch, WidthAnswer, WidthRequest, WidthsAnswer, WidthsRequest,
    WrittenList, encode_canvas_run, encode_frame_placed, is_platform_message, kind_of, widths_in_request,
};
use twidget_wire::{CanvasOp, FontMetrics};

use crate::{Backend, Renderer, RendererChannel};

/// What a FrameDelta run costs before its bytes: a copy's tag, offset and length, a literal's tag and length.
const COPY_RUN_BYTES: usize = 1 + 4 + 4;
const LITERAL_RUN_BYTES: usize = 1 + 4;
/// How many commands of the base with a command's hash are compared with it when it does not continue the last copy.
const PROBES: usize = 4;
const NONE: u32 = u32::MAX;

/// The hash a painted list's index keys its commands by: 64-bit FNV-1a over the command's bytes.
pub fn command_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for &c in bytes {
        h ^= u64::from(c);
        h = h.wrapping_mul(1099511628211);
    }
    h
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Entry {
    hash: u64,
    unit: u32,
}

fn before(a: &Entry, b: &Entry) -> bool {
    a.hash < b.hash || (a.hash == b.hash && a.unit < b.unit)
}

/// A painted list as encoded once: unit 0 is its head, unit k from 1 its command k - 1, each bytes
/// [bounds[k], bounds[k + 1]); its commands by hash for the next list to find.
struct Painted {
    seq: u64,
    bytes: Vec<u8>,
    bounds: Vec<u32>,
    index: Vec<Entry>,
}

impl Painted {
    fn units(&self) -> usize {
        self.bounds.len() - 1
    }

    fn unit(&self, u: usize) -> &[u8] {
        &self.bytes[self.bounds[u] as usize..self.bounds[u + 1] as usize]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Back {
    Joins,
    AfterLiteral,
    AfterCopy,
}

/// Copies of what `list`, whose units hash to `hashes`, shares byte for byte with `base`, no more than the base holds
/// in all, and literals for the rest, in as few bytes as such runs take; none when nothing is worth copying.
fn runs_for(base: &Painted, list: &Painted, hashes: &[u64]) -> Vec<DeltaRun> {
    let units = list.units();
    let same_as = |u: usize, b: u32| (b as usize) < base.units() && base.unit(b as usize) == list.unit(u);
    let mut from = vec![NONE; units];
    let mut budget = base.bytes.len();
    for u in 0..units {
        let length = list.unit(u).len();
        if length > budget {
            continue;
        }
        let mut found = NONE;
        if u == 0 {
            if same_as(0, 0) {
                found = 0;
            }
        } else if from[u - 1] != NONE && same_as(u, from[u - 1].wrapping_add(1)) {
            found = from[u - 1].wrapping_add(1);
        } else {
            let key = Entry { hash: hashes[u], unit: 0 };
            let mut it = base.index.partition_point(|e| before(e, &key));
            let mut probe = 0;
            while probe < PROBES && it < base.index.len() && base.index[it].hash == hashes[u] {
                if same_as(u, base.index[it].unit) {
                    found = base.index[it].unit;
                    break;
                }
                probe += 1;
                it += 1;
            }
        }
        if found == NONE {
            continue;
        }
        from[u] = found;
        budget -= length;
    }
    const FAR: usize = usize::MAX / 4;
    let mut literal_back = vec![Back::AfterLiteral; units];
    let mut copy_back = vec![Back::AfterLiteral; units];
    let mut literal = LITERAL_RUN_BYTES + list.unit(0).len();
    let mut copy = if from[0] != NONE { COPY_RUN_BYTES } else { FAR };
    for u in 1..units {
        let length = list.unit(u).len();
        let joined = literal + length;
        let opened = copy + LITERAL_RUN_BYTES + length;
        literal_back[u] = if joined <= opened { Back::Joins } else { Back::AfterCopy };
        let next_literal = joined.min(opened);
        let mut next_copy = FAR;
        if from[u] != NONE {
            let after_literal = literal + COPY_RUN_BYTES;
            let after_copy = copy + COPY_RUN_BYTES;
            let continued = if from[u - 1] != NONE && from[u] == from[u - 1].wrapping_add(1) { copy } else { FAR };
            // std::min of a list: the first of the smallest.
            next_copy = continued;
            if after_literal < next_copy {
                next_copy = after_literal;
            }
            if after_copy < next_copy {
                next_copy = after_copy;
            }
            copy_back[u] = if next_copy == continued {
                Back::Joins
            } else if next_copy == after_literal {
                Back::AfterLiteral
            } else {
                Back::AfterCopy
            };
        }
        literal = next_literal;
        copy = next_copy;
    }
    let mut copies = vec![false; units];
    let mut opens = vec![false; units];
    let mut copying = copy < literal;
    let mut any = false;
    for u in (0..units).rev() {
        let back = if copying { copy_back[u] } else { literal_back[u] };
        copies[u] = copying;
        any = any || copying;
        opens[u] = u == 0 || back != Back::Joins;
        if back != Back::Joins {
            copying = back == Back::AfterCopy;
        }
    }
    let mut runs: Vec<DeltaRun> = Vec::new();
    if !any {
        return runs;
    }
    for u in 0..units {
        let bytes = list.unit(u);
        if opens[u] {
            runs.push(if copies[u] {
                DeltaRun { offset: base.bounds[from[u] as usize], length: 0, literal: Vec::new() }
            } else {
                DeltaRun::default()
            });
        }
        let last = runs.last_mut().expect("the first unit opens a run");
        if copies[u] {
            last.length = last.length.wrapping_add(bytes.len() as u32);
        } else {
            last.literal.extend_from_slice(bytes);
        }
    }
    runs
}

/// Every renderer message of one `deliver()`, and whether the engine's frame message is due after them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    pub messages: Vec<Json>,
    pub frame: bool,
}

/// What the end and its proxy share: the embedder's channel, the caps, why the end stopped (empty while it
/// runs), and the list of the last painted Frame sent while it is within `delta_base`, for the next one to copy
/// from, which a stop drops.
struct Link<C> {
    channel: C,
    caps: Caps,
    reason: String,
    base: Option<Painted>,
}

fn lock<C>(link: &Mutex<Link<C>>) -> MutexGuard<'_, Link<C>> {
    link.lock().unwrap_or_else(PoisonError::into_inner)
}

impl<C: RendererChannel> Link<C> {
    fn stopped(&self) -> bool {
        !self.reason.is_empty()
    }

    /// The first reason stays.
    fn stop(&mut self, reason: &str) -> bool {
        if self.reason.is_empty() {
            self.reason = reason.to_string();
        }
        self.base = None;
        false
    }

    /// A Frame, painted ones as a FrameDelta on the last painted list when that is shorter.
    fn send_frame(&mut self, frame: &Frame) -> bool {
        if self.stopped() {
            return false;
        }
        if !frame.painted {
            return self.send(frame);
        }
        let caps = self.caps;
        let mut bytes = Vec::new();
        let mut written = WrittenList::default();
        if !encode_frame_placed(frame, &caps, &mut bytes, caps.delta_base.min(u32::MAX as usize), &mut written) {
            return self.stop("a message past the caps");
        }
        let mut next = None;
        if !written.starts.is_empty() {
            let mut painted = Painted {
                seq: frame.seq,
                bytes: bytes[written.at..written.at + written.size].to_vec(),
                bounds: Vec::with_capacity(written.starts.len() + 1),
                index: Vec::new(),
            };
            painted.bounds.push(0);
            for &at in &written.starts {
                painted.bounds.push(at as u32);
            }
            let hashes: Vec<u64> = (0..painted.units()).map(|u| command_hash(painted.unit(u))).collect();
            if let Some(base) = &self.base {
                let delta = FrameDelta {
                    seq: frame.seq,
                    state: frame.state,
                    base: base.seq,
                    runs: runs_for(base, &painted, &hashes),
                    damage: frame.damage.clone(),
                };
                let mut smaller = Vec::new();
                if !delta.runs.is_empty() && delta.encode(&caps, &mut smaller) && smaller.len() < bytes.len() {
                    bytes = smaller;
                }
            }
            painted.index.reserve(painted.units() - 1);
            for (u, &hash) in hashes.iter().enumerate().skip(1) {
                painted.index.push(Entry { hash, unit: u as u32 });
            }
            painted.index.sort_by(|a, b| a.hash.cmp(&b.hash).then(a.unit.cmp(&b.unit)));
            next = Some(painted);
        }
        self.channel.send(bytes);
        self.base = next;
        true
    }

    fn post(&mut self, encode_message: impl FnOnce(&Caps, &mut Vec<u8>) -> bool) -> bool {
        if self.stopped() {
            return false;
        }
        let mut bytes = Vec::new();
        if !encode_message(&self.caps, &mut bytes) {
            return self.stop("a message past the caps");
        }
        self.channel.send(bytes);
        true
    }

    fn send<M: Message>(&mut self, message: &M) -> bool {
        self.post(|caps, bytes| message.encode(caps, bytes))
    }

    /// The answer to a request, or None with the end stopped.
    fn ask<A>(
        &mut self,
        encode_request: impl FnOnce(&Caps, &mut Vec<u8>) -> bool,
        decode_answer: impl FnOnce(&[u8], &Caps) -> Option<A>,
    ) -> Option<A> {
        if self.stopped() {
            return None;
        }
        let mut bytes = Vec::new();
        if !encode_request(&self.caps, &mut bytes) {
            self.stop("a request past the caps");
            return None;
        }
        let mut answer = Vec::new();
        if !self.channel.request(bytes, &mut answer) {
            self.stop("a request got no answer");
            return None;
        }
        let decoded = decode_answer(&answer, &self.caps);
        if decoded.is_none() {
            self.stop("an answer did not decode");
        }
        decoded
    }
}

/// The renderer's Backend: every call a message to the platform end, or a request whose answer it waits for.
pub struct Proxy<C> {
    link: Arc<Mutex<Link<C>>>,
}

impl<C> fmt::Debug for Proxy<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Proxy").finish_non_exhaustive()
    }
}

/// Another handle on the same channel: the renderer and its document's container share one Backend, as renderer.h's
/// `Container` holds the renderer's `Backend&`.
impl<C> Clone for Proxy<C> {
    fn clone(&self) -> Self {
        Proxy { link: Arc::clone(&self.link) }
    }
}

impl<C: RendererChannel> Proxy<C> {
    fn answered<A: Message>(&mut self, request: &impl Message) -> Option<A> {
        lock(&self.link).ask(|caps, bytes| request.encode(caps, bytes), |bytes, caps| A::decode(bytes, caps))
    }

    fn requested_font(&mut self, request: &FontRequest, out: &mut FontMetrics) -> i32 {
        let Some(answer) = self.answered::<FontAnswer>(request) else { return 0 };
        *out = answer.metrics;
        answer.font
    }
}

impl<C: RendererChannel> Backend for Proxy<C> {
    fn create_font(&mut self, families: &[u8], size_px: f32, weight: i32, italic: bool, features: &[u8], out: &mut FontMetrics) -> i32 {
        let request =
            FontRequest { canvas: false, families: families.to_vec(), size: size_px, weight, italic, features: features.to_vec() };
        self.requested_font(&request, out)
    }

    fn create_canvas_font(&mut self, families: &[u8], size_px: f32, weight: i32, italic: bool, out: &mut FontMetrics) -> i32 {
        let request = FontRequest { canvas: true, families: families.to_vec(), size: size_px, weight, italic, features: Vec::new() };
        self.requested_font(&request, out)
    }

    fn delete_font(&mut self, font: i32) {
        lock(&self.link).send(&FontRelease { font });
    }

    fn text_width(&mut self, font: i32, text: &[u8]) -> f32 {
        self.answered::<WidthAnswer>(&WidthRequest { font, text: text.to_vec() }).map_or(0.0, |answer| answer.width)
    }

    /// As many texts in each WidthsRequest as it holds within the caps, in order; a text that fits in no request goes
    /// as a WidthRequest. 0 for the texts not measured once the end stops.
    fn text_widths(&mut self, texts: &[TextInFont]) -> Vec<f32> {
        let mut widths = vec![0.0f32; texts.len()];
        let mut i = 0;
        while i < texts.len() && !lock(&self.link).stopped() {
            let n = widths_in_request(texts, i, &lock(&self.link).caps);
            if n == 0 {
                widths[i] = self.text_width(texts[i].font, &texts[i].text);
                i += 1;
                continue;
            }
            let request = WidthsRequest { texts: texts[i..i + n].to_vec() };
            let answer =
                lock(&self.link).ask(|caps, bytes| request.encode(caps, bytes), |bytes, caps| WidthsAnswer::decode(bytes, caps, &request));
            let Some(answer) = answer else { break };
            widths[i..i + answer.widths.len()].copy_from_slice(&answer.widths);
            i += request.texts.len();
        }
        widths
    }

    fn load_image(&mut self, id: i32, url: &[u8], w: &mut f32, h: &mut f32) -> bool {
        if url.len() > lock(&self.link).caps.url {
            return false;
        }
        let Some(answer) = self.answered::<ImageAnswer>(&ImageRequest { id, url: url.to_vec() }) else { return false };
        *w = answer.w;
        *h = answer.h;
        answer.ok
    }

    fn canvas_run(&mut self, id: i32, w: i32, h: i32, ops: &[CanvasOp]) {
        lock(&self.link).post(|caps, bytes| encode_canvas_run(id, w, h, ops, caps, bytes));
    }

    fn canvas_read(&mut self, id: i32, x: i32, y: i32, w: i32, h: i32, rgba: &mut Vec<u8>) -> bool {
        let request = PixelsRequest { id, x, y, w, h };
        let answer =
            lock(&self.link).ask(|caps, bytes| request.encode(caps, bytes), |bytes, caps| PixelsAnswer::decode(bytes, caps, &request));
        let Some(answer) = answer else { return false };
        if answer.ok {
            *rgba = answer.rgba;
        }
        answer.ok
    }

    fn canvas_release(&mut self, id: i32) {
        lock(&self.link).send(&CanvasRelease { id });
    }
}

fn claim_of(messages: &[Json]) -> Option<GestureClaim> {
    messages.iter().find(|m| m.get("type").as_str() == b"gesture").map(|m| GestureClaim {
        x: m.get("x").as_bool(false),
        y: m.get("y").as_bool(false),
        hold: m.get("hold").as_bool(false),
    })
}

/// Bit for bit, as the end decides whether a State changed.
fn same(a: &State, b: &State) -> bool {
    a.width.to_bits() == b.width.to_bits()
        && a.height == b.height
        && a.dirty == b.dirty
        && a.wants_tick == b.wants_tick
        && a.next_tick_in.to_bits() == b.next_tick_in.to_bits()
        && a.frame_asked == b.frame_asked
}

/// The renderer end: a renderer whose Backend calls, frames and reports travel to a platform end over the
/// embedder's channel.
pub struct RendererEnd<C: RendererChannel, R: Renderer> {
    link: Arc<Mutex<Link<C>>>,
    sent_state: Option<State>,
    sent_selection: Vec<u8>,
    frame_asked: bool,
    renderer: R,
}

impl<C: RendererChannel, R: Renderer> fmt::Debug for RendererEnd<C, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RendererEnd")
            .field("stop_reason", &self.stop_reason())
            .field("sent_state", &self.sent_state)
            .field("frame_asked", &self.frame_asked)
            .finish_non_exhaustive()
    }
}

impl<C: RendererChannel, R: Renderer> RendererEnd<C, R> {
    /// The end, with the renderer `make` builds on its proxy: `make` stands for renderer.h's constructor,
    /// `Renderer(width, dpr, dark, backend, clock)`, the clock being the caller's to give it. The renderer's
    /// construction makes its requests over `channel`; then the end reports its first State.
    pub fn new(
        width: f64,
        dpr: f64,
        dark: bool,
        channel: C,
        caps: Caps,
        make: impl FnOnce(f64, f64, bool, Proxy<C>) -> R,
    ) -> RendererEnd<C, R> {
        let link = Arc::new(Mutex::new(Link { channel, caps, reason: String::new(), base: None }));
        let renderer = make(width, dpr, dark, Proxy { link: Arc::clone(&link) });
        let mut end = RendererEnd { link, sent_state: None, sent_selection: Vec::new(), frame_asked: false, renderer };
        end.finish(&[], None, None, None);
        end
    }

    pub fn stopped(&self) -> bool {
        lock(&self.link).stopped()
    }

    pub fn stop_reason(&self) -> String {
        lock(&self.link).reason.clone()
    }

    /// For the helper's own calls the channel does not carry, compositing among them: only while not
    /// `stopped()`, each followed by `report()`.
    pub fn renderer(&mut self) -> &mut R {
        &mut self.renderer
    }

    /// The channel, for an embedder that drives it between calls.
    pub fn with_channel<T>(&self, f: impl FnOnce(&mut C) -> T) -> T {
        f(&mut lock(&self.link).channel)
    }

    fn stop(&self, reason: &str) -> bool {
        lock(&self.link).stop(reason)
    }

    fn send<M: Message>(&self, message: &M) -> bool {
        lock(&self.link).send(message)
    }

    fn take<M: Message>(&self, message: &[u8]) -> Option<M> {
        let mut link = lock(&self.link);
        let decoded = M::decode(message, &link.caps);
        if decoded.is_none() {
            link.stop("a platform message that did not decode");
        }
        decoded
    }

    pub fn receive(&mut self, message: &Json) -> Vec<Json> {
        if self.stopped() {
            return Vec::new();
        }
        let out = self.renderer.receive(message);
        self.finish(&out, None, None, None);
        if self.stopped() {
            return Vec::new();
        }
        out
    }

    pub fn want_frame(&mut self) {
        if self.stopped() {
            return;
        }
        self.frame_asked = true;
        self.finish(&[], None, None, None);
    }

    pub fn report(&mut self, messages: &[Json]) {
        self.finish(messages, None, None, None);
    }

    /// One platform message: false, `out` left empty, once the end stops (now or before).
    pub fn deliver(&mut self, message: &[u8], out: &mut Output) -> bool {
        *out = Output::default();
        if self.stopped() {
            return false;
        }
        let mut result = Output::default();
        if !self.run(message, &mut result) || self.stopped() {
            return false;
        }
        *out = result;
        true
    }

    fn run(&mut self, message: &[u8], out: &mut Output) -> bool {
        let Some(kind) = kind_of(message) else { return self.stop("a platform message of no known kind or version") };
        match kind {
            Kind::FrameRequest => {
                let Some(m) = self.take::<FrameRequest>(message) else { return false };
                self.run_frame(&m, out);
                true
            }
            Kind::SetWidth => {
                let Some(m) = self.take::<SetWidth>(message) else { return false };
                self.renderer.set_width(m.width, &mut out.messages);
                self.finish(&out.messages, None, None, None);
                true
            }
            Kind::SetMaxHeight => {
                let Some(m) = self.take::<SetMaxHeight>(message) else { return false };
                self.renderer.set_max_height(m.height, &mut out.messages);
                self.finish(&out.messages, None, None, None);
                true
            }
            Kind::SetTheme => {
                let Some(m) = self.take::<SetTheme>(message) else { return false };
                self.renderer.set_theme(m.dark, &mut out.messages);
                self.finish(&out.messages, None, None, None);
                true
            }
            Kind::Touch => {
                let Some(m) = self.take::<Touch>(message) else { return false };
                out.messages = self.renderer.touch(m.action, m.x, m.y, m.pointer, m.primary, m.mouse, m.modifiers, m.button);
                self.pointed(&out.messages);
                true
            }
            Kind::Hover => {
                let Some(m) = self.take::<Hover>(message) else { return false };
                out.messages = self.renderer.hover(m.action, m.x, m.y, m.modifiers);
                self.pointed(&out.messages);
                true
            }
            Kind::ClearSelection => {
                if self.take::<ClearSelection>(message).is_none() {
                    return false;
                }
                self.renderer.clear_selection();
                self.finish(&[], None, None, None);
                true
            }
            Kind::ClaimQuery => {
                let Some(m) = self.take::<ClaimQuery>(message) else { return false };
                let answer = ClaimAnswer { seq: m.seq, claim: self.renderer.claim_at(m.x, m.y) };
                self.finish(&[], None, None, Some(&answer));
                true
            }
            other => self.stop(&format!("a message of kind {} where platform input belongs", other as u8)),
        }
    }

    fn run_frame(&mut self, request: &FrameRequest, out: &mut Output) {
        if request.tick {
            out.messages = self.renderer.tick();
            out.frame = std::mem::replace(&mut self.frame_asked, false);
        }
        let mut frame = Frame { seq: request.seq, ..Frame::default() };
        if !self.stopped() && (request.paint == Paint::Force || (request.paint == Paint::IfDirty && self.renderer.dirty())) {
            frame.list = self.renderer.paint();
            let mut damage = Vec::new();
            if self.renderer.damage(&mut damage) {
                frame.damage = Some(damage);
            }
            frame.painted = true;
        }
        self.finish(&out.messages, None, Some(&mut frame), None);
    }

    fn pointed(&mut self, messages: &[Json]) {
        if self.stopped() {
            return;
        }
        let pointer = Pointer { cursor: self.renderer.cursor(), claim: claim_of(messages) };
        self.finish(messages, Some(&pointer), None, None);
    }

    /// The reports of a call, in their order: the platform's messages among `messages`, the pointer, a changed
    /// selection, then the frame (which carries the state), or the claim's answer and a changed state.
    fn finish(&mut self, messages: &[Json], pointer: Option<&Pointer>, frame: Option<&mut Frame>, claim: Option<&ClaimAnswer>) {
        if self.stopped() {
            return;
        }
        let mut reports = Messages::default();
        for m in messages {
            if is_platform_message(m.get("type").as_str()) {
                reports.texts.push(m.dump());
            }
        }
        if !reports.texts.is_empty() && !self.send(&reports) {
            return;
        }
        if let Some(pointer) = pointer
            && !self.send(pointer)
        {
            return;
        }
        let mut selection = Selection::default();
        if self.renderer.may_have_selection() {
            selection.text = self.renderer.selected_text();
        }
        if self.stopped() {
            return;
        }
        if selection.text != self.sent_selection {
            if !self.send(&selection) {
                return;
            }
            self.sent_selection = selection.text;
        }
        let state = self.current_state();
        if let Some(frame) = frame {
            frame.state = state;
            if lock(&self.link).send_frame(frame) {
                self.sent_state = Some(state);
            }
            return;
        }
        if let Some(claim) = claim
            && !self.send(claim)
        {
            return;
        }
        if let Some(sent) = &self.sent_state
            && same(sent, &state)
        {
            return;
        }
        if self.send(&state) {
            self.sent_state = Some(state);
        }
    }

    /// The renderer's state, read in the C++'s order.
    fn current_state(&mut self) -> State {
        let width = self.renderer.width();
        let height = self.renderer.viewport_height();
        let dirty = self.renderer.dirty();
        let wants_tick = self.renderer.wants_tick();
        let next_tick_in = self.renderer.next_tick_in();
        State { width, height, dirty, wants_tick, next_tick_in, frame_asked: self.frame_asked }
    }
}
