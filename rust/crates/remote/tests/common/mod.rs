//! What the remote tests share: the Rust side of `oracle/remote_oracle` (a message decoded by the kind its
//! first byte names and encoded again), samples of every kind, and a generator.

#![allow(dead_code, unused_imports)]

use std::sync::Arc;

pub use twidget_remote::*;
pub use twidget_testkit::{Lcg, hex, mutate, unhex};
use twidget_wire as wire;
pub use twidget_wire::Rect;
use twidget_wire::{Batch, CanvasClip, CanvasOp, Cmd, Color, Command, DisplayList, FontMetrics, Geometry, OpKind, Path, PathOp};

fn again<T: Message>(message: &[u8], decode_caps: &Caps, encode_caps: &Caps) -> (u8, Vec<u8>) {
    let Some(value) = T::decode(message, decode_caps) else {
        return (0, Vec::new());
    };
    let mut out = Vec::new();
    if value.encode(encode_caps, &mut out) { (1, out) } else { (2, Vec::new()) }
}

/// What the oracle answers for 'm' (and 'M' with the default caps to decode).
pub fn answer(message: &[u8], decode_caps: &Caps, encode_caps: &Caps) -> (u8, Vec<u8>) {
    let Some(first) = message.first() else {
        return (0, Vec::new());
    };
    let (d, e) = (decode_caps, encode_caps);
    match Kind::from_u8(*first) {
        None => (0, Vec::new()),
        Some(Kind::PixelsAnswer) => (3, Vec::new()),
        Some(Kind::FrameRequest) => again::<FrameRequest>(message, d, e),
        Some(Kind::SetWidth) => again::<SetWidth>(message, d, e),
        Some(Kind::SetMaxHeight) => again::<SetMaxHeight>(message, d, e),
        Some(Kind::SetTheme) => again::<SetTheme>(message, d, e),
        Some(Kind::Touch) => again::<Touch>(message, d, e),
        Some(Kind::Hover) => again::<Hover>(message, d, e),
        Some(Kind::ClearSelection) => again::<ClearSelection>(message, d, e),
        Some(Kind::ClaimQuery) => again::<ClaimQuery>(message, d, e),
        Some(Kind::FontAnswer) => again::<FontAnswer>(message, d, e),
        Some(Kind::WidthAnswer) => again::<WidthAnswer>(message, d, e),
        Some(Kind::ImageAnswer) => again::<ImageAnswer>(message, d, e),
        Some(Kind::Frame) => again::<Frame>(message, d, e),
        Some(Kind::State) => again::<State>(message, d, e),
        Some(Kind::Messages) => again::<Messages>(message, d, e),
        Some(Kind::Pointer) => again::<Pointer>(message, d, e),
        Some(Kind::Selection) => again::<Selection>(message, d, e),
        Some(Kind::ClaimAnswer) => again::<ClaimAnswer>(message, d, e),
        Some(Kind::FontRelease) => again::<FontRelease>(message, d, e),
        Some(Kind::CanvasRun) => again::<CanvasRun>(message, d, e),
        Some(Kind::CanvasRelease) => again::<CanvasRelease>(message, d, e),
        Some(Kind::FontRequest) => again::<FontRequest>(message, d, e),
        Some(Kind::WidthRequest) => again::<WidthRequest>(message, d, e),
        Some(Kind::ImageRequest) => again::<ImageRequest>(message, d, e),
        Some(Kind::PixelsRequest) => again::<PixelsRequest>(message, d, e),
    }
}

/// What the oracle answers for 'x' (and 'X').
pub fn answer_pixels(message: &[u8], asked: &PixelsRequest, decode_caps: &Caps, encode_caps: &Caps) -> (u8, Vec<u8>) {
    let Some(value) = PixelsAnswer::decode(message, decode_caps, asked) else {
        return (0, Vec::new());
    };
    let mut out = Vec::new();
    if value.encode(encode_caps, &mut out) { (1, out) } else { (2, Vec::new()) }
}

/// The oracle's 'C' record for `caps`.
pub fn caps_record(caps: &Caps) -> Vec<u8> {
    let v = &caps.values;
    let mut out = vec![b'C'];
    for x in [
        v.commands,
        v.canvas_ops,
        v.path_ops,
        v.floats,
        v.stops,
        v.filters,
        v.clips,
        v.text,
        v.pixels,
        v.rects,
        caps.message,
        caps.text,
        caps.url,
        caps.messages,
    ] {
        out.extend_from_slice(&(x as u64).to_le_bytes());
    }
    out
}

pub fn pixels_record(tag: u8, asked: &PixelsRequest, message: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    for v in [asked.id, asked.x, asked.y, asked.w, asked.h] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&(message.len() as u32).to_le_bytes());
    out.extend_from_slice(message);
    out
}

pub fn encoded<T: Message>(value: &T) -> Vec<u8> {
    let mut out = Vec::new();
    assert!(value.encode(&Caps::default(), &mut out), "{:?} encodes", T::KIND);
    out
}

pub fn ordinary_metrics() -> FontMetrics {
    FontMetrics { ascent: 12.5, descent: 3.25, height: 17.0, x_height: 6.5, zero_width: 7.125, em_ascent: 12.0, em_descent: 4.0 }
}

/// A list of a few commands with text, a path and a filter.
pub fn compact_list() -> DisplayList {
    let path = Arc::new(Path {
        ops: vec![PathOp::Move, PathOp::Line, PathOp::Close],
        points: vec![1.0, 2.0, 3.0, 4.0],
        fill: true,
        ..Path::default()
    });
    vec![
        Command { kind: Cmd::FillRect, rect: Rect::new(0.0, 0.0, 10.0, 10.0), color: Color::rgba(1, 2, 3, 4), ..Command::default() },
        Command { kind: Cmd::Text, text: b"hello".to_vec(), font: 3, x: 1.0, y: 12.0, ..Command::default() },
        Command { kind: Cmd::Path, path: Some(path), ..Command::default() },
    ]
}

/// A batch of a few ops with a clip and pixels.
pub fn compact_batch() -> Batch {
    let clips =
        Arc::new(vec![CanvasClip { path: Geometry { ops: vec![PathOp::Move, PathOp::Close], points: vec![0.0, 0.0] }, evenodd: false }]);
    vec![
        CanvasOp { kind: OpKind::Reset, ..CanvasOp::default() },
        CanvasOp {
            kind: OpKind::Fill,
            path: Geometry { ops: vec![PathOp::Move, PathOp::Line], points: vec![0.0, 0.0, 5.0, 5.0] },
            clips: Some(clips),
            ..CanvasOp::default()
        },
        CanvasOp { kind: OpKind::PutImage, rgba: Some(Arc::new((0..16).collect())), w: 2, h: 2, ..CanvasOp::default() },
    ]
}

const SPECIAL_DOUBLES: [u64; 8] = [
    0x7ff8_0000_0000_1234,
    0xfff4_0000_0000_4321,
    0x8000_0000_0000_0000,
    0x7ff0_0000_0000_0000,
    0xfff0_0000_0000_0000,
    1,
    0x7fef_ffff_ffff_ffff,
    0,
];

pub fn special_double(i: usize) -> f64 {
    f64::from_bits(SPECIAL_DOUBLES[i % SPECIAL_DOUBLES.len()])
}

/// Every kind once or more, its members at ordinary values and at their extremes.
pub fn samples() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut add = |name: &str, bytes: Vec<u8>| out.push((name.to_string(), bytes));
    add("a forced tick", encoded(&FrameRequest { seq: u64::MAX, tick: true, paint: Paint::Force }));
    add("a skipped frame", encoded(&FrameRequest { seq: 0, tick: false, paint: Paint::Skip }));
    for i in 0..8 {
        add("a width", encoded(&SetWidth { width: special_double(i) }));
        add("a max height", encoded(&SetMaxHeight { height: special_double(i + 3) }));
        add("a claim query", encoded(&ClaimQuery { seq: i as u64, x: special_double(i), y: special_double(i + 1) }));
    }
    add("a dark theme", encoded(&SetTheme { dark: true }));
    add("a light theme", encoded(&SetTheme { dark: false }));
    add(
        "a touch",
        encoded(&Touch {
            action: i32::MIN,
            x: special_double(0),
            y: -1.5,
            pointer: i32::MAX,
            primary: true,
            mouse: false,
            modifiers: -1,
            button: 2,
        }),
    );
    add("a hover", encoded(&Hover { action: 3, x: 1e300, y: special_double(4), modifiers: 7 }));
    add("clearing the selection", encoded(&ClearSelection));
    add("a font answer", encoded(&FontAnswer { font: -2, metrics: ordinary_metrics() }));
    add("a width answer", encoded(&WidthAnswer { width: f32::from_bits(0x7fc0_1234) }));
    add("an image answer", encoded(&ImageAnswer { ok: true, w: 3.5, h: -0.0 }));
    add("a failed image answer", encoded(&ImageAnswer { ok: false, w: 0.0, h: 0.0 }));
    let state = State { width: 320.5, height: -7, dirty: true, wants_tick: false, next_tick_in: f64::INFINITY, frame_asked: true };
    add("a state", encoded(&state));
    add(
        "a painted frame",
        encoded(&Frame { seq: 9, state, painted: true, list: compact_list(), damage: Some(vec![Rect::new(1.0, 2.0, 3.0, 4.0)]) }),
    );
    add("a painted frame of nothing", encoded(&Frame { seq: 2, state: State::default(), painted: true, list: Vec::new(), damage: None }));
    add("an unpainted frame", encoded(&Frame { seq: 3, state: State::default(), painted: false, list: Vec::new(), damage: None }));
    add("messages", encoded(&Messages { texts: vec![b"{\"type\":\"size\"}".to_vec(), Vec::new(), b"\xff\x00".to_vec()] }));
    add("no messages", encoded(&Messages { texts: Vec::new() }));
    add(
        "a claimed pointer",
        encoded(&Pointer { cursor: b"pointer".to_vec(), claim: Some(GestureClaim { x: true, y: false, hold: true }) }),
    );
    add("an unclaimed pointer", encoded(&Pointer { cursor: Vec::new(), claim: None }));
    add("a selection", encoded(&Selection { text: "selected é".as_bytes().to_vec() }));
    add("a claim answer", encoded(&ClaimAnswer { seq: 5, claim: GestureClaim { x: false, y: true, hold: false } }));
    add("a font release", encoded(&FontRelease { font: i32::MIN }));
    add("a canvas run", encoded(&CanvasRun { id: 1, w: 2, h: 3, ops: compact_batch() }));
    add("an empty canvas run", encoded(&CanvasRun { id: -1, w: 0, h: 0, ops: Vec::new() }));
    add("a canvas release", encoded(&CanvasRelease { id: 7 }));
    add(
        "a canvas font request",
        encoded(&FontRequest { canvas: true, families: b"a".to_vec(), size: 1.0, weight: 2, italic: false, features: Vec::new() }),
    );
    add(
        "a document font request",
        encoded(&FontRequest {
            canvas: false,
            families: b"\"Inter\", sans-serif".to_vec(),
            size: 13.5,
            weight: 700,
            italic: true,
            features: b"tnum=1".to_vec(),
        }),
    );
    add("a width request", encoded(&WidthRequest { font: 4, text: b"word ".to_vec() }));
    add("an image request", encoded(&ImageRequest { id: 2, url: b"data:image/png;base64,AAAA".to_vec() }));
    add("a pixels request", encoded(&PixelsRequest { id: 1, x: -1, y: 2, w: 3, h: 4 }));
    add("the largest pixels request", encoded(&PixelsRequest { id: 1, x: 0, y: 0, w: 8192, h: 2048 }));
    out
}

fn any_f64(random: &mut Lcg) -> f64 {
    match random.below(5) {
        0 => special_double(random.below(8)),
        1 => f64::from_bits((u64::from(random.next()) << 32) | u64::from(random.next())),
        _ => (random.below(100000) as f64 - 50000.0) / 4.0,
    }
}

fn any_i32(random: &mut Lcg) -> i32 {
    match random.below(3) {
        0 => [i32::MIN, i32::MAX, -1, 0][random.below(4)],
        1 => random.next() as i32 ^ (random.next() << 1) as i32,
        _ => random.below(100) as i32,
    }
}

fn any_bytes(random: &mut Lcg, most: usize) -> Vec<u8> {
    let n = random.below(most + 1);
    (0..n).map(|_| random.next() as u8).collect()
}

fn any_claim(random: &mut Lcg) -> GestureClaim {
    GestureClaim { x: random.chance(2), y: random.chance(2), hold: random.chance(2) }
}

fn any_state(random: &mut Lcg) -> State {
    State {
        width: any_f64(random),
        height: any_i32(random),
        dirty: random.chance(2),
        wants_tick: random.chance(2),
        next_tick_in: any_f64(random),
        frame_asked: random.chance(2),
    }
}

/// A message of any kind but PixelsAnswer with members drawn at random, encoded.
pub fn any_message(random: &mut Lcg) -> Vec<u8> {
    let kinds = Kind::ALL;
    loop {
        let kind = kinds[random.below(kinds.len())];
        let bytes = match kind {
            Kind::FrameRequest => encoded(&FrameRequest {
                seq: u64::from(random.next()) << 20,
                tick: random.chance(2),
                paint: [Paint::Skip, Paint::IfDirty, Paint::Force][random.below(3)],
            }),
            Kind::SetWidth => encoded(&SetWidth { width: any_f64(random) }),
            Kind::SetMaxHeight => encoded(&SetMaxHeight { height: any_f64(random) }),
            Kind::SetTheme => encoded(&SetTheme { dark: random.chance(2) }),
            Kind::Touch => encoded(&Touch {
                action: any_i32(random),
                x: any_f64(random),
                y: any_f64(random),
                pointer: any_i32(random),
                primary: random.chance(2),
                mouse: random.chance(2),
                modifiers: any_i32(random),
                button: any_i32(random),
            }),
            Kind::Hover => encoded(&Hover { action: any_i32(random), x: any_f64(random), y: any_f64(random), modifiers: any_i32(random) }),
            Kind::ClearSelection => encoded(&ClearSelection),
            Kind::ClaimQuery => encoded(&ClaimQuery { seq: u64::from(random.next()), x: any_f64(random), y: any_f64(random) }),
            Kind::FontAnswer => encoded(&FontAnswer {
                font: any_i32(random),
                metrics: FontMetrics { ascent: any_f64(random) as f32, height: f32::from_bits(random.next()), ..ordinary_metrics() },
            }),
            Kind::WidthAnswer => encoded(&WidthAnswer { width: f32::from_bits(random.next()) }),
            Kind::ImageAnswer => {
                encoded(&ImageAnswer { ok: random.chance(2), w: f32::from_bits(random.next()), h: f32::from_bits(random.next()) })
            }
            Kind::PixelsAnswer => continue,
            Kind::Frame => {
                let painted = random.chance(2);
                let list = if painted && random.chance(2) { compact_list() } else { Vec::new() };
                let damage = if painted && random.chance(2) { Some(vec![Rect::new(0.0, 0.0, any_f64(random) as f32, 1.0)]) } else { None };
                encoded(&Frame { seq: u64::from(random.next()), state: any_state(random), painted, list, damage })
            }
            Kind::State => encoded(&any_state(random)),
            Kind::Messages => {
                let n = random.below(4);
                encoded(&Messages { texts: (0..n).map(|_| any_bytes(random, 20)).collect() })
            }
            Kind::Pointer => {
                let cursor = any_bytes(random, 12);
                let claim = if random.chance(2) { Some(any_claim(random)) } else { None };
                encoded(&Pointer { cursor, claim })
            }
            Kind::Selection => encoded(&Selection { text: any_bytes(random, 30) }),
            Kind::ClaimAnswer => encoded(&ClaimAnswer { seq: u64::from(random.next()), claim: any_claim(random) }),
            Kind::FontRelease => encoded(&FontRelease { font: any_i32(random) }),
            Kind::CanvasRun => {
                let ops = if random.chance(2) { compact_batch() } else { Vec::new() };
                encoded(&CanvasRun { id: any_i32(random), w: any_i32(random), h: any_i32(random), ops })
            }
            Kind::CanvasRelease => encoded(&CanvasRelease { id: any_i32(random) }),
            Kind::FontRequest => {
                let canvas = random.chance(2);
                let families = any_bytes(random, 16);
                let features = if canvas { Vec::new() } else { any_bytes(random, 8) };
                encoded(&FontRequest {
                    canvas,
                    families,
                    size: f32::from_bits(random.next()),
                    weight: any_i32(random),
                    italic: random.chance(2),
                    features,
                })
            }
            Kind::WidthRequest => encoded(&WidthRequest { font: any_i32(random), text: any_bytes(random, 24) }),
            Kind::ImageRequest => encoded(&ImageRequest { id: any_i32(random), url: any_bytes(random, 24) }),
            Kind::PixelsRequest => {
                let w = random.below(9000) as i32 - 100;
                let h = random.below(3000) as i32 - 100;
                if pixel_ok(w, h) {
                    encoded(&PixelsRequest { id: any_i32(random), x: any_i32(random), y: any_i32(random), w, h })
                } else {
                    continue;
                }
            }
        };
        return bytes;
    }
}

pub fn pixel_ok(w: i32, h: i32) -> bool {
    (0..=CANVAS_MAX_SIDE).contains(&w) && (0..=CANVAS_MAX_SIDE).contains(&h) && i64::from(w) * i64::from(h) <= CANVAS_MAX_PIXELS
}

pub fn wire_caps() -> wire::Caps {
    wire::Caps::default()
}
