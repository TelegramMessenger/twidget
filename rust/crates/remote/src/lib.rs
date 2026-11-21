//! The messages between TwidgetCore's renderer end and its platform end (`core/src/remote.h`,
//! `remote.cpp`): 28 kinds in four ranges, each a kind byte, a version byte and its fields, little-endian,
//! the paint values inside in their wire forms. `docs/TWIDGET_CORE.md`, "Two ends over a byte channel",
//! lays them out. Decoders take hostile bytes; encoders refuse what the decoder would.

#![forbid(unsafe_code)]

use twidget_wire as wire;
use twidget_wire::{CanvasOp, DisplayList, FontMetrics};

pub const VERSION: u8 = 1;

/// canvas.h's `Canvas::MAX_SIDE` and `MAX_PIXELS`: the most a pixels request may ask for.
pub const CANVAS_MAX_SIDE: i32 = 8192;
pub const CANVAS_MAX_PIXELS: i64 = 4096 * 4096;

/// Kinds from 1 go from the platform to the renderer and kinds from 64 back; those from 32 answer the
/// requests from 96.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Kind {
    FrameRequest = 1,
    SetWidth = 2,
    SetMaxHeight = 3,
    SetTheme = 4,
    Touch = 5,
    Hover = 6,
    ClearSelection = 7,
    ClaimQuery = 8,
    FontAnswer = 32,
    WidthAnswer = 33,
    ImageAnswer = 34,
    PixelsAnswer = 35,
    WidthsAnswer = 36,
    Frame = 64,
    State = 65,
    Messages = 66,
    Pointer = 67,
    Selection = 68,
    ClaimAnswer = 69,
    FontRelease = 70,
    CanvasRun = 71,
    CanvasRelease = 72,
    FrameDelta = 73,
    FontRequest = 96,
    WidthRequest = 97,
    ImageRequest = 98,
    PixelsRequest = 99,
    WidthsRequest = 100,
}

impl Kind {
    pub const ALL: [Kind; 28] = [
        Kind::FrameRequest,
        Kind::SetWidth,
        Kind::SetMaxHeight,
        Kind::SetTheme,
        Kind::Touch,
        Kind::Hover,
        Kind::ClearSelection,
        Kind::ClaimQuery,
        Kind::FontAnswer,
        Kind::WidthAnswer,
        Kind::ImageAnswer,
        Kind::PixelsAnswer,
        Kind::WidthsAnswer,
        Kind::Frame,
        Kind::State,
        Kind::Messages,
        Kind::Pointer,
        Kind::Selection,
        Kind::ClaimAnswer,
        Kind::FontRelease,
        Kind::CanvasRun,
        Kind::CanvasRelease,
        Kind::FrameDelta,
        Kind::FontRequest,
        Kind::WidthRequest,
        Kind::ImageRequest,
        Kind::PixelsRequest,
        Kind::WidthsRequest,
    ];

    pub fn from_u8(value: u8) -> Option<Kind> {
        Kind::ALL.iter().copied().find(|k| *k as u8 == value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    pub values: wire::Caps,
    /// Bytes of one message either way: widget-sandbox's frames hold 32 MiB and 64 KiB, which leaves
    /// room for their framing.
    pub message: usize,
    pub text: usize,
    pub url: usize,
    pub messages: usize,
    /// Texts of one WidthsRequest, and bytes of their texts together.
    pub widths: usize,
    pub widths_text: usize,
    /// Bytes of one Frame, and of the Frame a FrameDelta stands for once rebuilt; `usize::MAX` leaves Frames to
    /// `message` alone.
    pub frame: usize,
    /// Bytes of the painted list each end keeps for a FrameDelta to copy from, and of a list a FrameDelta rebuilds;
    /// both ends take the same, and 0 sends every painted Frame whole.
    pub delta_base: usize,
}

impl Default for Caps {
    fn default() -> Caps {
        Caps {
            values: wire::Caps::default(),
            message: 32 << 20,
            text: 4 << 20,
            url: 8 << 20,
            messages: 1024,
            widths: 256,
            widths_text: 16 << 10,
            frame: usize::MAX,
            delta_base: 256 << 10,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Paint {
    Skip = 0,
    #[default]
    IfDirty = 1,
    Force = 2,
}

/// widget-sandbox's `WidgetGestureClaim` (gesture_policy.h): what of a press the widget claims.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GestureClaim {
    /// Horizontal drags.
    pub x: bool,
    /// Always false since v0.1.3: vertical drags are the chat's.
    pub y: bool,
    /// Holds, and a right click with a mouse.
    pub hold: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameRequest {
    pub seq: u64,
    pub tick: bool,
    pub paint: Paint,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SetWidth {
    pub width: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SetMaxHeight {
    pub height: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SetTheme {
    pub dark: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Touch {
    pub action: i32,
    pub x: f64,
    pub y: f64,
    pub pointer: i32,
    pub primary: bool,
    pub mouse: bool,
    pub modifiers: i32,
    pub button: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hover {
    pub action: i32,
    pub x: f64,
    pub y: f64,
    pub modifiers: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClearSelection;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClaimQuery {
    pub seq: u64,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FontAnswer {
    pub font: i32,
    pub metrics: FontMetrics,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WidthAnswer {
    pub width: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ImageAnswer {
    pub ok: bool,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PixelsAnswer {
    pub ok: bool,
    pub rgba: Vec<u8>,
}

/// The widths of a WidthsRequest's texts, in their order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WidthsAnswer {
    pub widths: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub width: f64,
    pub height: i32,
    pub dirty: bool,
    pub wants_tick: bool,
    pub next_tick_in: f64,
    pub frame_asked: bool,
}

impl Default for State {
    fn default() -> State {
        State { width: 0.0, height: 0, dirty: false, wants_tick: false, next_tick_in: f64::INFINITY, frame_asked: false }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub seq: u64,
    pub state: State,
    pub painted: bool,
    pub list: DisplayList,
    pub damage: wire::Damage,
}

/// Where [`encode_frame_placed`] wrote a painted Frame's list: `size` bytes from byte `at` of its output, and, while the
/// list kept within the bytes it was given, where each command starts and, last, where the list ends, counted from the
/// list's first byte; no starts for a longer list or an unpainted Frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WrittenList {
    pub at: usize,
    pub size: usize,
    pub starts: Vec<usize>,
}

/// `length` bytes of a FrameDelta's base from `offset`, or, when `literal` is not empty, those bytes, `offset` and
/// `length` then 0.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeltaRun {
    pub offset: u32,
    pub length: u32,
    pub literal: Vec<u8>,
}

/// A painted Frame whose list is rebuilt from the list of the last painted Frame, its base, by copies and bytes of
/// its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameDelta {
    pub seq: u64,
    pub state: State,
    pub base: u64,
    pub runs: Vec<DeltaRun>,
    pub damage: wire::Damage,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Messages {
    pub texts: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pointer {
    pub cursor: Vec<u8>,
    pub claim: Option<GestureClaim>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    pub text: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClaimAnswer {
    pub seq: u64,
    pub claim: GestureClaim,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FontRelease {
    pub font: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CanvasRun {
    pub id: i32,
    pub w: i32,
    pub h: i32,
    pub ops: Vec<CanvasOp>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CanvasRelease {
    pub id: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FontRequest {
    pub canvas: bool,
    pub families: Vec<u8>,
    pub size: f32,
    pub weight: i32,
    pub italic: bool,
    pub features: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WidthRequest {
    pub font: i32,
    pub text: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageRequest {
    pub id: i32,
    pub url: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PixelsRequest {
    pub id: i32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// One text in one backend font (backend.h's `TextInFont`), as a WidthsRequest carries it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextInFont {
    pub font: i32,
    pub text: Vec<u8>,
}

/// Texts a renderer asks the widths of in one request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WidthsRequest {
    pub texts: Vec<TextInFont>,
}

/// The JSON message types a renderer end reports to the platform (in `Messages`).
const PLATFORM_TYPES: [&[u8]; 5] = [b"size", b"gesture", b"limit", b"violation", b"bubble"];

/// The fewest bytes a str takes: its length.
const STR_BYTES: usize = 4;
/// A WidthsRequest's header and count, and the fewest bytes one of its texts takes: its font and its length.
const WIDTHS_HEAD: usize = 2 + 4;
const WIDTHS_ITEM_BYTES: usize = 4 + STR_BYTES;
/// A painted Frame's bytes before its list: header, seq, state and the flag.
const STATE_BYTES: usize = 8 + 4 + 1 + 1 + 8 + 1;
const FRAME_HEAD: usize = 2 + 8 + STATE_BYTES + 1;
/// A FrameDelta run's tag, and the fewest bytes a run takes: a literal of one byte.
const COPY_RUN: u8 = 0;
const LITERAL_RUN: u8 = 1;
const RUN_BYTES: usize = 1 + 4 + 1;

/// The kind of a message of version 1, from its header alone.
pub fn kind_of(message: &[u8]) -> Option<Kind> {
    if message.len() < 2 || message[1] != VERSION {
        return None;
    }
    Kind::from_u8(message[0])
}

pub fn is_platform_message(kind: &[u8]) -> bool {
    PLATFORM_TYPES.contains(&kind)
}

/// How many of `texts`, from `from` on, keep within the widths caps in order: their count, each text and all of them,
/// and, when `sized`, the message cap as a WidthsRequest's, which bounds its WidthsAnswer too.
fn widths_within(texts: &[TextInFont], from: usize, caps: &Caps, sized: bool) -> usize {
    let most = caps.widths.min(u32::MAX as usize);
    let longest = caps.text.min(u32::MAX as usize);
    let (mut bytes, mut text, mut n) = (WIDTHS_HEAD, 0usize, 0usize);
    let mut i = from;
    while i < texts.len() && n < most {
        let size = texts[i].text.len();
        if size > longest || size > caps.widths_text - text {
            break;
        }
        if sized && (caps.message < bytes || caps.message - bytes < WIDTHS_ITEM_BYTES || size > caps.message - bytes - WIDTHS_ITEM_BYTES) {
            break;
        }
        bytes += WIDTHS_ITEM_BYTES + size;
        text += size;
        i += 1;
        n += 1;
    }
    n
}

fn widths_fit(request: &WidthsRequest, caps: &Caps) -> bool {
    widths_within(&request.texts, 0, caps, false) == request.texts.len()
}

/// How many of `texts`, from `from` on, one WidthsRequest holds within `caps`, together with its WidthsAnswer: as
/// many as encode takes in order, 0 when even the first is past them.
pub fn widths_in_request(texts: &[TextInFont], from: usize, caps: &Caps) -> usize {
    widths_within(texts, from, caps, true)
}

/// The bytes of a w × h RGBA region a canvas can hold, or None.
fn pixel_bytes(w: i32, h: i32) -> Option<u32> {
    if w < 0 || h < 0 || w > CANVAS_MAX_SIDE || h > CANVAS_MAX_SIDE || i64::from(w) * i64::from(h) > CANVAS_MAX_PIXELS {
        return None;
    }
    Some((w as u32) * (h as u32) * 4)
}

struct Writer<'a> {
    out: &'a mut Vec<u8>,
    start: usize,
    limit: usize,
}

impl Writer<'_> {
    fn written(&self) -> usize {
        self.out.len() - self.start
    }

    fn u8(&mut self, v: u8) {
        self.out.push(v);
    }

    fn u32(&mut self, v: u32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    fn i32(&mut self, v: i32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }

    fn f64(&mut self, v: f64) {
        self.u64(v.to_bits());
    }

    fn flag(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    fn count(&mut self, n: usize, cap: usize) -> bool {
        let Ok(n) = u32::try_from(n) else {
            return false;
        };
        if n as usize > cap {
            return false;
        }
        self.u32(n);
        true
    }

    /// Whether a u32 length and `n` bytes after it keep the message within its cap.
    fn room(&self, n: usize) -> bool {
        let used = self.written();
        used <= self.limit && STR_BYTES <= self.limit - used && n <= self.limit - used - STR_BYTES
    }

    fn bytes(&mut self, v: &[u8], cap: usize) -> bool {
        if !self.room(v.len()) || !self.count(v.len(), cap) {
            return false;
        }
        self.out.extend_from_slice(v);
        true
    }

    fn floats(&mut self, v: &[f32], cap: usize) -> bool {
        if v.len() > usize::MAX / 4 || !self.room(v.len() * 4) || !self.count(v.len(), cap) {
            return false;
        }
        for f in v {
            self.f32(*f);
        }
        true
    }

    /// A value of another codec after its u32 length.
    fn value(&mut self, write: impl FnOnce(&mut Vec<u8>) -> bool) -> bool {
        let at = self.out.len();
        self.u32(0);
        if !write(self.out) {
            return false;
        }
        let Ok(n) = u32::try_from(self.out.len() - at - 4) else {
            return false;
        };
        self.out[at..at + 4].copy_from_slice(&n.to_le_bytes());
        true
    }

    fn claim(&mut self, c: &GestureClaim) {
        self.flag(c.x);
        self.flag(c.y);
        self.flag(c.hold);
    }

    fn state(&mut self, s: &State) {
        self.f64(s.width);
        self.i32(s.height);
        self.flag(s.dirty);
        self.flag(s.wants_tick);
        self.f64(s.next_tick_in);
        self.flag(s.frame_asked);
    }
}

#[derive(Clone, Copy)]
struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    fn done(&self) -> bool {
        self.rest.is_empty()
    }

    fn view(&mut self, n: usize) -> Option<&'a [u8]> {
        if n > self.rest.len() {
            return None;
        }
        let (head, tail) = self.rest.split_at(n);
        self.rest = tail;
        Some(head)
    }

    /// The bytes of a value of another codec after its u32 length.
    fn value_view(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        self.view(n)
    }

    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, tail) = self.rest.split_first_chunk::<N>()?;
        self.rest = tail;
        Some(*head)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take::<1>().map(|b| b[0])
    }

    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_le_bytes)
    }

    fn u64(&mut self) -> Option<u64> {
        self.take().map(u64::from_le_bytes)
    }

    fn i32(&mut self) -> Option<i32> {
        self.take().map(i32::from_le_bytes)
    }

    fn f32(&mut self) -> Option<f32> {
        self.u32().map(f32::from_bits)
    }

    fn f64(&mut self) -> Option<f64> {
        self.u64().map(f64::from_bits)
    }

    fn flag(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    fn head(&mut self, kind: Kind) -> Option<()> {
        (self.u8()? == kind as u8 && self.u8()? == VERSION).then_some(())
    }

    /// A count of items of at least `item_bytes` each, checked by division so that nothing wraps.
    fn count(&mut self, cap: usize, item_bytes: usize) -> Option<usize> {
        let c = self.u32()? as usize;
        (c <= cap && c <= self.rest.len() / item_bytes).then_some(c)
    }

    fn str(&mut self, cap: usize) -> Option<Vec<u8>> {
        let n = self.count(cap, 1)?;
        self.bytes(n)
    }

    fn bytes(&mut self, n: usize) -> Option<Vec<u8>> {
        if n > self.rest.len() {
            return None;
        }
        let (head, tail) = self.rest.split_at(n);
        self.rest = tail;
        Some(head.to_vec())
    }

    /// A value of another codec after its u32 length, read from those bytes alone.
    fn value<T>(&mut self, read: impl FnOnce(&[u8]) -> Option<T>) -> Option<T> {
        let n = self.u32()? as usize;
        if n > self.rest.len() {
            return None;
        }
        let value = read(&self.rest[..n])?;
        self.rest = &self.rest[n..];
        Some(value)
    }

    fn claim(&mut self) -> Option<GestureClaim> {
        Some(GestureClaim { x: self.flag()?, y: self.flag()?, hold: self.flag()? })
    }

    fn state(&mut self) -> Option<State> {
        Some(State {
            width: self.f64()?,
            height: self.i32()?,
            dirty: self.flag()?,
            wants_tick: self.flag()?,
            next_tick_in: self.f64()?,
            frame_asked: self.flag()?,
        })
    }
}

/// Runs `write` after the header, cutting `out` back to its size on entry when it refuses or the
/// message passes its cap.
fn appended(out: &mut Vec<u8>, caps: &Caps, kind: Kind, write: impl FnOnce(&mut Writer) -> bool) -> bool {
    let start = out.len();
    let mut w = Writer { out, start, limit: caps.message };
    w.u8(kind as u8);
    w.u8(VERSION);
    if write(&mut w) && w.written() <= caps.message {
        return true;
    }
    out.truncate(start);
    false
}

/// What `read` takes from all of `message` after its header.
fn taken<'a, T>(message: &'a [u8], caps: &Caps, kind: Kind, read: impl FnOnce(&mut Reader<'a>) -> Option<T>) -> Option<T> {
    if message.len() > caps.message {
        return None;
    }
    let mut r = Reader { rest: message };
    r.head(kind)?;
    let value = read(&mut r)?;
    r.rest.is_empty().then_some(value)
}

/// A message of one kind: appended to `out` (false, `out` as on entry, for what the decoder would
/// refuse) and taken from all of a message's bytes.
pub trait Message: Sized {
    const KIND: Kind;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool;
    fn decode(message: &[u8], caps: &Caps) -> Option<Self>;
}

impl Message for FrameRequest {
    const KIND: Kind = Kind::FrameRequest;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.u64(self.seq);
            w.flag(self.tick);
            w.u8(self.paint as u8);
            true
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| {
            let seq = r.u64()?;
            let tick = r.flag()?;
            let paint = match r.u8()? {
                0 => Paint::Skip,
                1 => Paint::IfDirty,
                2 => Paint::Force,
                _ => return None,
            };
            Some(FrameRequest { seq, tick, paint })
        })
    }
}

impl Message for SetWidth {
    const KIND: Kind = Kind::SetWidth;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.f64(self.width);
            true
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| Some(SetWidth { width: r.f64()? }))
    }
}

impl Message for SetMaxHeight {
    const KIND: Kind = Kind::SetMaxHeight;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.f64(self.height);
            true
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| Some(SetMaxHeight { height: r.f64()? }))
    }
}

impl Message for SetTheme {
    const KIND: Kind = Kind::SetTheme;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.flag(self.dark);
            true
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| Some(SetTheme { dark: r.flag()? }))
    }
}

impl Message for Touch {
    const KIND: Kind = Kind::Touch;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.i32(self.action);
            w.f64(self.x);
            w.f64(self.y);
            w.i32(self.pointer);
            w.flag(self.primary);
            w.flag(self.mouse);
            w.i32(self.modifiers);
            w.i32(self.button);
            true
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| {
            Some(Touch {
                action: r.i32()?,
                x: r.f64()?,
                y: r.f64()?,
                pointer: r.i32()?,
                primary: r.flag()?,
                mouse: r.flag()?,
                modifiers: r.i32()?,
                button: r.i32()?,
            })
        })
    }
}

impl Message for Hover {
    const KIND: Kind = Kind::Hover;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.i32(self.action);
            w.f64(self.x);
            w.f64(self.y);
            w.i32(self.modifiers);
            true
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| Some(Hover { action: r.i32()?, x: r.f64()?, y: r.f64()?, modifiers: r.i32()? }))
    }
}

impl Message for ClearSelection {
    const KIND: Kind = Kind::ClearSelection;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |_| true)
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |_| Some(ClearSelection))
    }
}

impl Message for ClaimQuery {
    const KIND: Kind = Kind::ClaimQuery;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.u64(self.seq);
            w.f64(self.x);
            w.f64(self.y);
            true
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| Some(ClaimQuery { seq: r.u64()?, x: r.f64()?, y: r.f64()? }))
    }
}

impl Message for FontAnswer {
    const KIND: Kind = Kind::FontAnswer;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.i32(self.font);
            w.value(|s| {
                wire::encode_metrics(&self.metrics, s);
                true
            })
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| Some(FontAnswer { font: r.i32()?, metrics: r.value(wire::decode_metrics)? }))
    }
}

impl Message for WidthAnswer {
    const KIND: Kind = Kind::WidthAnswer;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.f32(self.width);
            true
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| Some(WidthAnswer { width: r.f32()? }))
    }
}

impl Message for ImageAnswer {
    const KIND: Kind = Kind::ImageAnswer;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.flag(self.ok);
            w.f32(self.w);
            w.f32(self.h);
            true
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| Some(ImageAnswer { ok: r.flag()?, w: r.f32()?, h: r.f32()? }))
    }
}

impl PixelsAnswer {
    pub const KIND: Kind = Kind::PixelsAnswer;

    /// A failed answer has no pixels; a taken one has them all, within the pixel cap.
    pub fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.flag(self.ok);
            if !self.ok {
                return self.rgba.is_empty();
            }
            w.bytes(&self.rgba, caps.values.pixels)
        })
    }

    /// The answer to `asked`: exactly its w × h × 4 bytes when it is taken, within the pixel cap.
    pub fn decode(message: &[u8], caps: &Caps, asked: &PixelsRequest) -> Option<PixelsAnswer> {
        let expected = pixel_bytes(asked.w, asked.h);
        taken(message, caps, Self::KIND, |r| {
            let expected = expected?;
            if !r.flag()? {
                return Some(PixelsAnswer { ok: false, rgba: Vec::new() });
            }
            let n = r.u32()?;
            if n != expected || n as usize > caps.values.pixels {
                return None;
            }
            Some(PixelsAnswer { ok: true, rgba: r.bytes(n as usize)? })
        })
    }
}

/// Whether the painted Frame of a list and a damage of these lengths keeps within the frame and the message caps.
fn fits_frame(caps: &Caps, list: u64, damage: u64) -> bool {
    let bytes = (FRAME_HEAD as u64 + 4).wrapping_add(list).wrapping_add(4).wrapping_add(damage);
    bytes <= caps.frame as u64 && bytes <= caps.message as u64
}

/// A Frame after its header; for a painted one, `list` where its list went, starts noted within `most` bytes, when
/// given.
fn put_frame(w: &mut Writer, message: &Frame, caps: &Caps, most: usize, list: Option<&mut WrittenList>) -> bool {
    w.u64(message.seq);
    w.state(&message.state);
    w.flag(message.painted);
    if !message.painted {
        return message.list.is_empty() && message.damage.is_none() && w.written() <= caps.frame;
    }
    let placed = list.is_some();
    let mut written = WrittenList::default();
    let mut damage = 0;
    let put = w.value(|s| {
        written.at = s.len();
        let ok = if placed {
            wire::encode_list_with_starts(&message.list, &caps.values, s, &mut written.starts, most)
        } else {
            wire::encode_list(&message.list, &caps.values, s)
        };
        if !ok {
            return false;
        }
        written.size = s.len() - written.at;
        true
    }) && w.value(|s| {
        let at = s.len();
        if !wire::encode_damage(&message.damage, &caps.values, s) {
            return false;
        }
        damage = s.len() - at;
        true
    }) && fits_frame(caps, written.size as u64, damage as u64);
    if put && let Some(list) = list {
        *list = written;
    }
    put
}

impl Message for Frame {
    const KIND: Kind = Kind::Frame;

    /// An unpainted frame carries no list and no damage; the whole Frame keeps within the frame cap.
    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| put_frame(w, self, caps, 0, None))
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        decode_frame_viewed(message, caps).map(|(frame, _)| frame)
    }
}

/// As `Frame::encode`, and where it wrote the list, its starts noted while the list keeps within `most` bytes; `list`
/// untouched on false.
pub fn encode_frame_placed(message: &Frame, caps: &Caps, out: &mut Vec<u8>, most: usize, list: &mut WrittenList) -> bool {
    let mut written = WrittenList::default();
    if !appended(out, caps, Kind::Frame, |w| put_frame(w, message, caps, most, Some(&mut written))) {
        return false;
    }
    *list = written;
    true
}

/// As `Frame::decode`, and the bytes of a painted Frame's list where they lie in `message`, empty for an unpainted one.
pub fn decode_frame_viewed<'a>(message: &'a [u8], caps: &Caps) -> Option<(Frame, &'a [u8])> {
    if message.len() > caps.frame {
        return None;
    }
    let mut painted_list: &[u8] = &[];
    let frame = taken(message, caps, Kind::Frame, |r| {
        let seq = r.u64()?;
        let state = r.state()?;
        let painted = r.flag()?;
        if !painted {
            return Some(Frame { seq, state, painted, list: Vec::new(), damage: None });
        }
        let bytes = r.value_view()?;
        let list = wire::decode_list(bytes, &caps.values)?;
        painted_list = bytes;
        let damage = r.value(|b| wire::decode_damage(b, &caps.values))?;
        Some(Frame { seq, state, painted, list, damage })
    })?;
    Some((frame, painted_list))
}

/// One run for the list's head and one for each command: what an encoder that copies whole commands may need.
fn most_runs(caps: &Caps) -> usize {
    caps.values.commands.min(u32::MAX as usize - 1) + 1
}

/// A FrameDelta's fields before its runs.
struct DeltaHead {
    seq: u64,
    state: State,
    base: u64,
    length: u32,
    runs: usize,
}

fn read_delta_head(r: &mut Reader, caps: &Caps) -> Option<DeltaHead> {
    let seq = r.u64()?;
    let state = r.state()?;
    let base = r.u64()?;
    let length = r.u32()?;
    if length == 0 || length as usize > caps.delta_base {
        return None;
    }
    let runs = r.count(most_runs(caps).min(length as usize), RUN_BYTES)?;
    Some(DeltaHead { seq, state, base, length, runs })
}

/// Takes runs that rebuild exactly `length` bytes, each copy within a base of `base` bytes and all of them copying no
/// more than it holds, and gives each to `run(offset, length, literal)`, the literal empty for a copy.
fn read_runs<'a>(r: &mut Reader<'a>, count: usize, length: u32, base: usize, mut run: impl FnMut(u32, u32, &'a [u8])) -> bool {
    let (mut built, mut copied) = (0usize, 0usize);
    for _ in 0..count {
        let Some(tag) = r.u8() else { return false };
        let mut offset = 0u32;
        let mut literal: &'a [u8] = &[];
        let n: u32;
        if tag == COPY_RUN {
            let (Some(o), Some(m)) = (r.u32(), r.u32()) else { return false };
            if o as usize > base || m as usize > base - o as usize || m as usize > base - copied {
                return false;
            }
            offset = o;
            n = m;
            copied += n as usize;
        } else {
            if tag != LITERAL_RUN {
                return false;
            }
            let Some(m) = r.u32() else { return false };
            let Some(bytes) = r.view(m as usize) else { return false };
            literal = bytes;
            n = m;
        }
        if n == 0 || n as usize > length as usize - built {
            return false;
        }
        built += n as usize;
        run(offset, n, literal);
    }
    built == length as usize
}

/// A FrameDelta's runs on a base of `base` bytes and its damage, read as views up to its last byte, and the Frame it
/// stands for within the caps; nothing made yet, the damage's bytes given back.
fn scan_delta<'a>(r: &mut Reader<'a>, caps: &Caps, h: &DeltaHead, base: usize) -> Option<&'a [u8]> {
    if !read_runs(r, h.runs, h.length, base, |_, _, _| {}) {
        return None;
    }
    let damage = r.value_view()?;
    (r.done() && fits_frame(caps, u64::from(h.length), damage.len() as u64)).then_some(damage)
}

impl Message for FrameDelta {
    const KIND: Kind = Kind::FrameDelta;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            let (mut length, mut copied) = (0u64, 0u64);
            for run in &self.runs {
                if !run.literal.is_empty() {
                    if run.offset != 0 || run.length != 0 {
                        return false;
                    }
                    length += run.literal.len() as u64;
                } else {
                    if run.length == 0
                        || run.offset as usize > caps.delta_base
                        || run.length as usize > caps.delta_base - run.offset as usize
                    {
                        return false;
                    }
                    copied += u64::from(run.length);
                    length += u64::from(run.length);
                }
            }
            if self.runs.is_empty()
                || self.runs.len() > most_runs(caps)
                || length > caps.delta_base as u64
                || length > u64::from(u32::MAX)
                || copied > caps.delta_base as u64
            {
                return false;
            }
            w.u64(self.seq);
            w.state(&self.state);
            w.u64(self.base);
            w.u32(length as u32);
            w.u32(self.runs.len() as u32);
            for run in &self.runs {
                if !run.literal.is_empty() {
                    w.u8(LITERAL_RUN);
                    if !w.bytes(&run.literal, caps.delta_base) {
                        return false;
                    }
                    continue;
                }
                w.u8(COPY_RUN);
                w.u32(run.offset);
                w.u32(run.length);
            }
            let before = w.written();
            w.value(|s| wire::encode_damage(&self.damage, &caps.values, s))
                && w.written() <= caps.frame
                && fits_frame(caps, length, (w.written() - before - 4) as u64)
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        if message.len() > caps.frame {
            return None;
        }
        taken(message, caps, Self::KIND, |r| {
            let h = read_delta_head(r, caps)?;
            let mut runs = *r;
            let damage = scan_delta(r, caps, &h, caps.delta_base)?;
            let mut delta = FrameDelta { seq: h.seq, state: h.state, base: h.base, runs: Vec::with_capacity(h.runs), damage: None };
            read_runs(&mut runs, h.runs, h.length, caps.delta_base, |offset, n, literal| {
                if literal.is_empty() {
                    delta.runs.push(DeltaRun { offset, length: n, literal: Vec::new() });
                } else {
                    delta.runs.push(DeltaRun { offset: 0, length: 0, literal: literal.to_vec() });
                }
            });
            delta.damage = wire::decode_damage(damage, &caps.values)?;
            Some(delta)
        })
    }
}

/// A FrameDelta's seq and base, read from the fields after its header alone; it never stands in for decoding.
pub fn delta_ids(message: &[u8]) -> Option<(u64, u64)> {
    let mut r = Reader { rest: message };
    r.head(Kind::FrameDelta)?;
    let seq = r.u64()?;
    r.state()?;
    let base = r.u64()?;
    Some((seq, base))
}

/// The painted Frame a FrameDelta stands for, rebuilt on `base`, the list of painted Frame `base_seq`, with the
/// rebuilt list's bytes.
pub fn rebuild(message: &[u8], caps: &Caps, base_seq: u64, base: &[u8]) -> Option<(Frame, Vec<u8>)> {
    if message.len() > caps.message || message.len() > caps.frame || base.len() > caps.delta_base {
        return None;
    }
    let mut r = Reader { rest: message };
    r.head(Kind::FrameDelta)?;
    let h = read_delta_head(&mut r, caps)?;
    if h.base != base_seq {
        return None;
    }
    let mut runs = r;
    let damage = scan_delta(&mut r, caps, &h, base.len())?;
    let damage = wire::decode_damage(damage, &caps.values)?;
    let mut bytes = Vec::with_capacity(h.length as usize);
    read_runs(&mut runs, h.runs, h.length, base.len(), |offset, n, literal| {
        if literal.is_empty() {
            bytes.extend_from_slice(&base[offset as usize..offset as usize + n as usize]);
        } else {
            bytes.extend_from_slice(literal);
        }
    });
    let list = wire::decode_list(&bytes, &caps.values)?;
    Some((Frame { seq: h.seq, state: h.state, painted: true, list, damage }, bytes))
}

impl Message for State {
    const KIND: Kind = Kind::State;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.state(self);
            true
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| r.state())
    }
}

impl Message for Messages {
    const KIND: Kind = Kind::Messages;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| w.count(self.texts.len(), caps.messages) && self.texts.iter().all(|t| w.bytes(t, caps.text)))
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| {
            let n = r.count(caps.messages, STR_BYTES)?;
            let mut texts = Vec::with_capacity(n);
            for _ in 0..n {
                texts.push(r.str(caps.text)?);
            }
            Some(Messages { texts })
        })
    }
}

impl Message for Pointer {
    const KIND: Kind = Kind::Pointer;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            if !w.bytes(&self.cursor, caps.text) {
                return false;
            }
            w.flag(self.claim.is_some());
            if let Some(claim) = &self.claim {
                w.claim(claim);
            }
            true
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| {
            let cursor = r.str(caps.text)?;
            let claim = if r.flag()? { Some(r.claim()?) } else { None };
            Some(Pointer { cursor, claim })
        })
    }
}

impl Message for Selection {
    const KIND: Kind = Kind::Selection;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| w.bytes(&self.text, caps.text))
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| Some(Selection { text: r.str(caps.text)? }))
    }
}

impl Message for ClaimAnswer {
    const KIND: Kind = Kind::ClaimAnswer;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.u64(self.seq);
            w.claim(&self.claim);
            true
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| Some(ClaimAnswer { seq: r.u64()?, claim: r.claim()? }))
    }
}

impl Message for FontRelease {
    const KIND: Kind = Kind::FontRelease;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.i32(self.font);
            true
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| Some(FontRelease { font: r.i32()? }))
    }
}

/// What a CanvasRun of these members encodes to, without making one.
pub fn encode_canvas_run(id: i32, w: i32, h: i32, ops: &[CanvasOp], caps: &Caps, out: &mut Vec<u8>) -> bool {
    appended(out, caps, Kind::CanvasRun, |writer| {
        writer.i32(id);
        writer.i32(w);
        writer.i32(h);
        writer.value(|s| wire::encode_batch(ops, &caps.values, s))
    })
}

impl Message for CanvasRun {
    const KIND: Kind = Kind::CanvasRun;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        encode_canvas_run(self.id, self.w, self.h, &self.ops, caps, out)
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| {
            Some(CanvasRun { id: r.i32()?, w: r.i32()?, h: r.i32()?, ops: r.value(|b| wire::decode_batch(b, &caps.values))? })
        })
    }
}

impl Message for CanvasRelease {
    const KIND: Kind = Kind::CanvasRelease;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.i32(self.id);
            true
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| Some(CanvasRelease { id: r.i32()? }))
    }
}

impl Message for FontRequest {
    const KIND: Kind = Kind::FontRequest;

    /// A canvas's font has no features, and none are written.
    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.flag(self.canvas);
            if !w.bytes(&self.families, caps.text) {
                return false;
            }
            w.f32(self.size);
            w.i32(self.weight);
            w.flag(self.italic);
            if self.canvas { self.features.is_empty() } else { w.bytes(&self.features, caps.text) }
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| {
            let canvas = r.flag()?;
            let families = r.str(caps.text)?;
            let size = r.f32()?;
            let weight = r.i32()?;
            let italic = r.flag()?;
            let features = if canvas { Vec::new() } else { r.str(caps.text)? };
            Some(FontRequest { canvas, families, size, weight, italic, features })
        })
    }
}

impl Message for WidthRequest {
    const KIND: Kind = Kind::WidthRequest;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.i32(self.font);
            w.bytes(&self.text, caps.text)
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| Some(WidthRequest { font: r.i32()?, text: r.str(caps.text)? }))
    }
}

impl Message for ImageRequest {
    const KIND: Kind = Kind::ImageRequest;

    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.i32(self.id);
            w.bytes(&self.url, caps.url)
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| Some(ImageRequest { id: r.i32()?, url: r.str(caps.url)? }))
    }
}

impl WidthsAnswer {
    pub const KIND: Kind = Kind::WidthsAnswer;

    /// At most the widths cap of widths, within the message cap.
    pub fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| w.floats(&self.widths, caps.widths))
    }

    /// The answer to `asked`, which keeps within the widths caps: exactly one width for each of its texts.
    pub fn decode(message: &[u8], caps: &Caps, asked: &WidthsRequest) -> Option<WidthsAnswer> {
        let fits = widths_fit(asked, caps);
        taken(message, caps, Self::KIND, |r| {
            if !fits {
                return None;
            }
            let n = r.count(caps.widths, 4)?;
            if n != asked.texts.len() {
                return None;
            }
            let mut widths = Vec::with_capacity(n);
            for _ in 0..n {
                widths.push(r.f32()?);
            }
            Some(WidthsAnswer { widths })
        })
    }
}

impl Message for WidthsRequest {
    const KIND: Kind = Kind::WidthsRequest;

    /// The texts keep within the widths caps and, as one request, the message cap.
    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            if widths_in_request(&self.texts, 0, caps) != self.texts.len() || !w.count(self.texts.len(), caps.widths) {
                return false;
            }
            for t in &self.texts {
                w.i32(t.font);
                if !w.bytes(&t.text, caps.text) {
                    return false;
                }
            }
            true
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| {
            let n = r.count(caps.widths, WIDTHS_ITEM_BYTES)?;
            let mut texts = Vec::with_capacity(n);
            let mut left = caps.widths_text;
            for _ in 0..n {
                let font = r.i32()?;
                let text = r.str(caps.text.min(left))?;
                left -= text.len();
                texts.push(TextInFont { font, text });
            }
            Some(WidthsRequest { texts })
        })
    }
}

impl Message for PixelsRequest {
    const KIND: Kind = Kind::PixelsRequest;

    /// A request asks for no more than a canvas holds.
    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            if pixel_bytes(self.w, self.h).is_none() {
                return false;
            }
            w.i32(self.id);
            w.i32(self.x);
            w.i32(self.y);
            w.i32(self.w);
            w.i32(self.h);
            true
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| {
            let m = PixelsRequest { id: r.i32()?, x: r.i32()?, y: r.i32()?, w: r.i32()?, h: r.i32()? };
            pixel_bytes(m.w, m.h).map(|_| m)
        })
    }
}
