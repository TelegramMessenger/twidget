//! tdesktop's `tgw/tgw_hosted_wire.h`: the glue's own platform messages, the `0xFF` controls, beside the
//! `twidget_remote` messages a hosted renderer's platform carries. A control is the tag `0xFF`, its kind and a body
//! of fixed numbers and length-prefixed texts, little-endian; Telegram's platform end decodes them as these decoders
//! do. To the helper: `Open` (the device ratio's bits), the press controls, the tap's `Mirror` and `MirrorWork`, and
//! `CanvasRefill`; from it: `Content`, `Loaded`, `Stopped` (a reason), `Mirrored` (a tap record), `Cursor`,
//! `CanvasRefused` and `Clicked`.

use twidget_remote::Caps;

/// A control's kind, its second byte: those below 64 go to the helper, those from 64 come from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Kind {
    Open = 1,
    MenuDeliver = 2,
    FingerHold = 3,
    FingerRelease = 4,
    Mirror = 5,
    MirrorWork = 6,
    CanvasRefill = 7,

    Content = 64,
    Loaded = 65,
    Stopped = 66,
    Mirrored = 67,
    Cursor = 68,
    CanvasRefused = 69,
    Clicked = 70,
}

impl Kind {
    pub const ALL: [Kind; 14] = [
        Kind::Open,
        Kind::MenuDeliver,
        Kind::FingerHold,
        Kind::FingerRelease,
        Kind::Mirror,
        Kind::MirrorWork,
        Kind::CanvasRefill,
        Kind::Content,
        Kind::Loaded,
        Kind::Stopped,
        Kind::Mirrored,
        Kind::Cursor,
        Kind::CanvasRefused,
        Kind::Clicked,
    ];

    /// The kind a byte names, None for a byte that names none (the C++ makes any byte a `Kind` and `ToHelper` and
    /// `FromHelper` refuse the others).
    pub fn from_u8(value: u8) -> Option<Kind> {
        Kind::ALL.iter().copied().find(|k| *k as u8 == value)
    }
}

pub const TAG: u8 = 0xFF;
pub const HEADER_BYTES: usize = 2;
pub const NUMBER_BYTES: usize = 8;
pub const LENGTH_BYTES: usize = 4;
pub const MAX_MESSAGE_BYTES: usize = 32 << 20;
pub const MAX_LIST_COMMANDS: usize = 1 << 16;
pub const MAX_FRAME_BYTES: usize = 8 << 20;
pub const MAX_CANVAS_RUN_OPS: usize = 1 << 13;
pub const MAX_WIDTHS_TEXTS: usize = 1024;
pub const MAX_WIDTHS_TEXT_BYTES: usize = 64 << 10;
pub const MAX_DELTA_BASE_BYTES: usize = 256 << 10;
pub const MAX_STOP_REASON_BYTES: usize = 4096;
pub const MAX_CURSOR_BYTES: usize = 256;

pub const MIRRORED_BUILT: u8 = b'b';
pub const MIRRORED_HOST: u8 = b'h';
pub const MIRRORED_RENDERER: u8 = b'r';
pub const MIRRORED_THEME: u8 = b't';
pub const MIRRORED_WORK: u8 = b'w';

pub const MIRRORED_OVERHEAD_BYTES: usize = HEADER_BYTES + 1 + LENGTH_BYTES;
pub const MAX_MIRRORED_TEXT_BYTES: usize = MAX_MESSAGE_BYTES - MIRRORED_OVERHEAD_BYTES;

/// A `Mirrored` record: its way and its text, in the message it was decoded from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MirroredRecord<'a> {
    pub way: u8,
    pub text: &'a [u8],
}

/// A `Cursor` report: the pointer messages the glue had taken when it changed, and the CSS cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorReport<'a> {
    pub pointers: u64,
    pub cursor: &'a [u8],
}

/// The caps of the renderer channel a hosted renderer's platform carries.
pub fn embedding_caps() -> Caps {
    let mut result = Caps::default();
    result.message = MAX_MESSAGE_BYTES;
    result.values.commands = MAX_LIST_COMMANDS;
    result.values.canvas_ops = MAX_CANVAS_RUN_OPS;
    result.widths = MAX_WIDTHS_TEXTS;
    result.widths_text = MAX_WIDTHS_TEXT_BYTES;
    result.frame = MAX_FRAME_BYTES;
    result.delta_base = MAX_DELTA_BASE_BYTES;
    result
}

pub fn is_control(message: &[u8]) -> bool {
    message.first() == Some(&TAG)
}

pub fn to_helper(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Open | Kind::MenuDeliver | Kind::FingerHold | Kind::FingerRelease | Kind::Mirror | Kind::MirrorWork | Kind::CanvasRefill
    )
}

pub fn is_tap_control(kind: Kind) -> bool {
    kind == Kind::Mirror || kind == Kind::MirrorWork
}

pub fn from_helper(kind: Kind) -> bool {
    matches!(kind, Kind::Content | Kind::Loaded | Kind::Stopped | Kind::Mirrored | Kind::Cursor | Kind::CanvasRefused | Kind::Clicked)
}

pub fn kind_of(message: &[u8]) -> Option<Kind> {
    if message.len() < HEADER_BYTES || !is_control(message) {
        return None;
    }
    let kind = Kind::from_u8(message[1])?;
    if !to_helper(kind) && !from_helper(kind) {
        return None;
    }
    Some(kind)
}

pub fn is_exactly(message: &[u8], kind: Kind) -> bool {
    message.len() == HEADER_BYTES && kind_of(message) == Some(kind)
}

pub fn is_mirrored_way(way: u8) -> bool {
    way == MIRRORED_BUILT || way == MIRRORED_HOST || way == MIRRORED_RENDERER || way == MIRRORED_THEME || way == MIRRORED_WORK
}

/// The header's own helpers, which the tap's work record reuses (`HostedWire::details`).
pub mod details {
    use super::{HEADER_BYTES, Kind, LENGTH_BYTES, NUMBER_BYTES, TAG, kind_of};

    pub fn header(kind: Kind, body: usize) -> Vec<u8> {
        let mut result = Vec::with_capacity(HEADER_BYTES + body);
        result.push(TAG);
        result.push(kind as u8);
        result
    }

    /// `value`'s low `bytes` bytes, little-endian (`bytes` at most 8).
    pub fn put_number(to: &mut Vec<u8>, value: u64, bytes: usize) {
        for i in 0..bytes {
            to.push((value >> (8 * i)) as u8);
        }
    }

    /// The first `bytes` bytes of `from` as a little-endian number, taken off its front; the callers checked
    /// that `from` holds them (`bytes` at most 8).
    pub fn take_number(from: &mut &[u8], bytes: usize) -> u64 {
        let mut result = 0u64;
        for i in 0..bytes {
            result |= u64::from(from[i]) << (8 * i);
        }
        *from = &from[bytes..];
        result
    }

    pub fn with_number(kind: Kind, value: u64) -> Vec<u8> {
        let mut result = header(kind, NUMBER_BYTES);
        put_number(&mut result, value, NUMBER_BYTES);
        result
    }

    pub fn number_of(message: &[u8], kind: Kind) -> Option<u64> {
        if message.len() != HEADER_BYTES + NUMBER_BYTES || kind_of(message) != Some(kind) {
            return None;
        }
        let mut body = &message[HEADER_BYTES..];
        Some(take_number(&mut body, NUMBER_BYTES))
    }

    /// The rest of `body` after its length, when that length is all of the rest and at most `limit`.
    pub fn text_of<'a>(body: &mut &'a [u8], limit: usize) -> Option<&'a [u8]> {
        if body.len() < LENGTH_BYTES {
            return None;
        }
        let length = take_number(body, LENGTH_BYTES);
        if length > limit as u64 || length != body.len() as u64 {
            return None;
        }
        Some(body)
    }
}

pub fn encode_open(dpr: f64) -> Vec<u8> {
    details::with_number(Kind::Open, dpr.to_bits())
}

/// The device ratio, when finite.
pub fn decode_open(message: &[u8]) -> Option<f64> {
    let bits = details::number_of(message, Kind::Open)?;
    let result = f64::from_bits(bits);
    if !result.is_finite() {
        return None;
    }
    Some(result)
}

pub fn encode_menu_deliver(press: u64) -> Vec<u8> {
    details::with_number(Kind::MenuDeliver, press)
}

pub fn encode_finger_hold(press: u64) -> Vec<u8> {
    details::with_number(Kind::FingerHold, press)
}

pub fn encode_finger_release(press: u64) -> Vec<u8> {
    details::with_number(Kind::FingerRelease, press)
}

/// The press of a `MenuDeliver`, `FingerHold` or `FingerRelease`.
pub fn decode_press(message: &[u8]) -> Option<u64> {
    let kind = kind_of(message);
    if kind != Some(Kind::MenuDeliver) && kind != Some(Kind::FingerHold) && kind != Some(Kind::FingerRelease) {
        return None;
    }
    details::number_of(message, kind?)
}

pub fn encode_clicked(press: u64) -> Vec<u8> {
    details::with_number(Kind::Clicked, press)
}

pub fn decode_clicked(message: &[u8]) -> Option<u64> {
    details::number_of(message, Kind::Clicked)
}

pub fn encode_mirror() -> Vec<u8> {
    details::header(Kind::Mirror, 0)
}

pub fn encode_mirror_work() -> Vec<u8> {
    details::header(Kind::MirrorWork, 0)
}

pub fn encode_canvas_refill() -> Vec<u8> {
    details::header(Kind::CanvasRefill, 0)
}

pub fn encode_canvas_refused() -> Vec<u8> {
    details::header(Kind::CanvasRefused, 0)
}

pub fn encode_content() -> Vec<u8> {
    details::header(Kind::Content, 0)
}

pub fn encode_loaded() -> Vec<u8> {
    details::header(Kind::Loaded, 0)
}

/// The reason's first `MAX_STOP_REASON_BYTES` bytes (a UTF-8 sequence may be cut).
pub fn encode_stopped(reason: &[u8]) -> Vec<u8> {
    let text = &reason[..reason.len().min(MAX_STOP_REASON_BYTES)];
    let mut result = details::header(Kind::Stopped, LENGTH_BYTES + text.len());
    details::put_number(&mut result, text.len() as u64, LENGTH_BYTES);
    result.extend_from_slice(text);
    result
}

pub fn decode_stopped(message: &[u8]) -> Option<&[u8]> {
    if message.len() > HEADER_BYTES + LENGTH_BYTES + MAX_STOP_REASON_BYTES || kind_of(message) != Some(Kind::Stopped) {
        return None;
    }
    let mut body = &message[HEADER_BYTES..];
    details::text_of(&mut body, MAX_STOP_REASON_BYTES)
}

/// The cursor's first `MAX_CURSOR_BYTES` bytes.
pub fn encode_cursor(pointers: u64, cursor: &[u8]) -> Vec<u8> {
    let text = &cursor[..cursor.len().min(MAX_CURSOR_BYTES)];
    let mut result = details::header(Kind::Cursor, NUMBER_BYTES + LENGTH_BYTES + text.len());
    details::put_number(&mut result, pointers, NUMBER_BYTES);
    details::put_number(&mut result, text.len() as u64, LENGTH_BYTES);
    result.extend_from_slice(text);
    result
}

pub fn decode_cursor(message: &[u8]) -> Option<CursorReport<'_>> {
    let fixed = HEADER_BYTES + NUMBER_BYTES + LENGTH_BYTES;
    if message.len() < fixed || message.len() > fixed + MAX_CURSOR_BYTES || kind_of(message) != Some(Kind::Cursor) {
        return None;
    }
    let mut body = &message[HEADER_BYTES..];
    let pointers = details::take_number(&mut body, NUMBER_BYTES);
    let text = details::text_of(&mut body, MAX_CURSOR_BYTES)?;
    Some(CursorReport { pointers, cursor: text })
}

/// None for a way that is none of the five and for a text past `MAX_MIRRORED_TEXT_BYTES`.
pub fn encode_mirrored(way: u8, text: &[u8]) -> Option<Vec<u8>> {
    if !is_mirrored_way(way) || text.len() > MAX_MIRRORED_TEXT_BYTES {
        return None;
    }
    let mut result = details::header(Kind::Mirrored, MIRRORED_OVERHEAD_BYTES - HEADER_BYTES + text.len());
    result.push(way);
    details::put_number(&mut result, text.len() as u64, LENGTH_BYTES);
    result.extend_from_slice(text);
    Some(result)
}

pub fn decode_mirrored(message: &[u8]) -> Option<MirroredRecord<'_>> {
    if message.len() < MIRRORED_OVERHEAD_BYTES || message.len() > MAX_MESSAGE_BYTES || kind_of(message) != Some(Kind::Mirrored) {
        return None;
    }
    let mut body = &message[HEADER_BYTES..];
    let way = body[0];
    body = &body[1..];
    let text = details::text_of(&mut body, MAX_MIRRORED_TEXT_BYTES);
    if !is_mirrored_way(way) {
        return None;
    }
    Some(MirroredRecord { way, text: text? })
}
