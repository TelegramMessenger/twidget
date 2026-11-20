//! The messages between TwidgetCore's renderer end and its platform end (`core/src/remote.h`,
//! `remote.cpp`): 25 kinds in four ranges, each a kind byte, a version byte and its fields, little-endian,
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
    Frame = 64,
    State = 65,
    Messages = 66,
    Pointer = 67,
    Selection = 68,
    ClaimAnswer = 69,
    FontRelease = 70,
    CanvasRun = 71,
    CanvasRelease = 72,
    FontRequest = 96,
    WidthRequest = 97,
    ImageRequest = 98,
    PixelsRequest = 99,
}

impl Kind {
    pub const ALL: [Kind; 25] = [
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
        Kind::Frame,
        Kind::State,
        Kind::Messages,
        Kind::Pointer,
        Kind::Selection,
        Kind::ClaimAnswer,
        Kind::FontRelease,
        Kind::CanvasRun,
        Kind::CanvasRelease,
        Kind::FontRequest,
        Kind::WidthRequest,
        Kind::ImageRequest,
        Kind::PixelsRequest,
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
}

impl Default for Caps {
    fn default() -> Caps {
        Caps { values: wire::Caps::default(), message: 32 << 20, text: 4 << 20, url: 8 << 20, messages: 1024 }
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

/// The JSON message types a renderer end reports to the platform (in `Messages`).
const PLATFORM_TYPES: [&[u8]; 5] = [b"size", b"gesture", b"limit", b"violation", b"bubble"];

/// The fewest bytes a str takes: its length.
const STR_BYTES: usize = 4;

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

struct Reader<'a> {
    rest: &'a [u8],
}

impl Reader<'_> {
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
fn taken<T>(message: &[u8], caps: &Caps, kind: Kind, read: impl FnOnce(&mut Reader) -> Option<T>) -> Option<T> {
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

impl Message for Frame {
    const KIND: Kind = Kind::Frame;

    /// An unpainted frame carries no list and no damage.
    fn encode(&self, caps: &Caps, out: &mut Vec<u8>) -> bool {
        appended(out, caps, Self::KIND, |w| {
            w.u64(self.seq);
            w.state(&self.state);
            w.flag(self.painted);
            if !self.painted {
                return self.list.is_empty() && self.damage.is_none();
            }
            w.value(|s| wire::encode_list(&self.list, &caps.values, s)) && w.value(|s| wire::encode_damage(&self.damage, &caps.values, s))
        })
    }

    fn decode(message: &[u8], caps: &Caps) -> Option<Self> {
        taken(message, caps, Self::KIND, |r| {
            let seq = r.u64()?;
            let state = r.state()?;
            let painted = r.flag()?;
            if !painted {
                return Some(Frame { seq, state, painted, list: Vec::new(), damage: None });
            }
            let list = r.value(|b| wire::decode_list(b, &caps.values))?;
            let damage = r.value(|b| wire::decode_damage(b, &caps.values))?;
            Some(Frame { seq, state, painted, list, damage })
        })
    }
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
