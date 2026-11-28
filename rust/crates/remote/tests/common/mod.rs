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
        Some(Kind::PixelsAnswer) | Some(Kind::WidthsAnswer) => (3, Vec::new()),
        Some(Kind::FrameDelta) => again::<FrameDelta>(message, d, e),
        Some(Kind::WidthsRequest) => again::<WidthsRequest>(message, d, e),
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
        caps.widths,
        caps.widths_text,
        caps.frame,
        caps.delta_base,
    ] {
        out.extend_from_slice(&(x as u64).to_le_bytes());
    }
    out
}

fn put_texts(out: &mut Vec<u8>, texts: &[TextInFont]) {
    out.extend_from_slice(&(texts.len() as u32).to_le_bytes());
    for t in texts {
        out.extend_from_slice(&t.font.to_le_bytes());
        out.extend_from_slice(&(t.text.len() as u32).to_le_bytes());
        out.extend_from_slice(&t.text);
    }
}

fn framed_after(mut head: Vec<u8>, message: &[u8]) -> Vec<u8> {
    head.extend_from_slice(&(message.len() as u32).to_le_bytes());
    head.extend_from_slice(message);
    head
}

/// The oracle's 'y' (or 'Y') record: a WidthsAnswer and the request it answers.
pub fn widths_record(tag: u8, asked: &WidthsRequest, message: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    put_texts(&mut out, &asked.texts);
    framed_after(out, message)
}

/// What the oracle answers for 'y' (and 'Y').
pub fn answer_widths(message: &[u8], asked: &WidthsRequest, decode_caps: &Caps, encode_caps: &Caps) -> (u8, Vec<u8>) {
    let Some(value) = WidthsAnswer::decode(message, decode_caps, asked) else {
        return (0, Vec::new());
    };
    let mut out = Vec::new();
    if value.encode(encode_caps, &mut out) { (1, out) } else { (2, Vec::new()) }
}

pub fn placed_record(most: u64, message: &[u8]) -> Vec<u8> {
    let mut out = vec![b'p'];
    out.extend_from_slice(&most.to_le_bytes());
    framed_after(out, message)
}

/// What the oracle answers for 'p': the Frame encoded again after "xy" with its list's place.
pub fn answer_placed(message: &[u8], caps: &Caps, most: u64) -> (u8, Vec<u8>) {
    let Some(frame) = Frame::decode(message, caps) else { return (0, Vec::new()) };
    let mut out = b"xy".to_vec();
    let mut list = WrittenList { at: 1, size: 2, starts: vec![3] };
    if !encode_frame_placed(&frame, caps, &mut out, most as usize, &mut list) {
        out.extend_from_slice(format!("{},{},{}", list.at, list.size, list.starts.len()).as_bytes());
        return (2, out);
    }
    out.extend_from_slice(&(list.at as u64).to_le_bytes());
    out.extend_from_slice(&(list.size as u64).to_le_bytes());
    out.extend_from_slice(&(list.starts.len() as u32).to_le_bytes());
    for s in list.starts {
        out.extend_from_slice(&(s as u64).to_le_bytes());
    }
    (1, out)
}

/// What the oracle answers for 'v': where the list's view lies in the message.
pub fn answer_viewed(message: &[u8], caps: &Caps) -> (u8, Vec<u8>) {
    let Some((_, list)) = decode_frame_viewed(message, caps) else { return (0, Vec::new()) };
    let offset = if list.is_empty() { 0 } else { list.as_ptr() as usize - message.as_ptr() as usize };
    let mut out = Vec::new();
    out.extend_from_slice(&(offset as u64).to_le_bytes());
    out.extend_from_slice(&(list.len() as u64).to_le_bytes());
    (1, out)
}

/// What the oracle answers for 'i'.
pub fn answer_ids(message: &[u8]) -> (u8, Vec<u8>) {
    let Some((seq, base)) = delta_ids(message) else { return (0, Vec::new()) };
    let mut out = Vec::new();
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&base.to_le_bytes());
    (1, out)
}

/// What the oracle answers for 's': the seq, the list's length and the damage view's offset and length.
pub fn answer_scanned(message: &[u8], caps: &Caps) -> (u8, Vec<u8>) {
    let Some((seq, length, damage)) = delta_scan(message, caps) else { return (0, Vec::new()) };
    let mut out = Vec::new();
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&((damage.as_ptr() as usize - message.as_ptr() as usize) as u64).to_le_bytes());
    out.extend_from_slice(&(damage.len() as u64).to_le_bytes());
    (1, out)
}

pub fn rebuilt_record(base_seq: u64, base: &[u8], message: &[u8]) -> Vec<u8> {
    let mut out = vec![b'r'];
    out.extend_from_slice(&base_seq.to_le_bytes());
    out.extend_from_slice(&(base.len() as u32).to_le_bytes());
    out.extend_from_slice(base);
    framed_after(out, message)
}

/// What the oracle answers for 'r': the rebuilt Frame's seq, state and damage, and the list's bytes.
pub fn answer_rebuilt(message: &[u8], caps: &Caps, base_seq: u64, base: &[u8]) -> (u8, Vec<u8>) {
    let Some((frame, list)) = rebuild(message, caps, base_seq, base) else { return (0, Vec::new()) };
    let mut out = Vec::new();
    out.extend_from_slice(&frame.seq.to_le_bytes());
    out.extend_from_slice(&frame.state.width.to_bits().to_le_bytes());
    out.extend_from_slice(&frame.state.height.to_le_bytes());
    out.push(u8::from(frame.state.dirty));
    out.push(u8::from(frame.state.wants_tick));
    out.extend_from_slice(&frame.state.next_tick_in.to_bits().to_le_bytes());
    out.push(u8::from(frame.state.frame_asked));
    out.push(u8::from(frame.painted));
    let mut damage = Vec::new();
    wire::encode_damage(&frame.damage, &wire::Caps::default(), &mut damage);
    out.extend_from_slice(&(damage.len() as u32).to_le_bytes());
    out.extend_from_slice(&damage);
    out.extend_from_slice(&list);
    (1, out)
}

pub fn widths_in_request_record(texts: &[TextInFont], from: u32) -> Vec<u8> {
    let mut out = vec![b'w'];
    put_texts(&mut out, texts);
    out.extend_from_slice(&from.to_le_bytes());
    out
}

pub fn answer_widths_in_request(texts: &[TextInFont], from: u32, caps: &Caps) -> (u8, Vec<u8>) {
    (1, (widths_in_request(texts, from as usize, caps) as u64).to_le_bytes().to_vec())
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
    for (name, request) in widths_requests() {
        add(&name, encoded(&request));
    }
    for (name, delta) in frame_deltas() {
        add(&name, encoded(&delta));
    }
    out
}

/// remote_tests.cpp's `ordinary_state()`.
pub fn ordinary_state() -> State {
    State { width: 320.5, height: 480, dirty: true, wants_tick: true, next_tick_in: 0.016, frame_asked: true }
}

/// remote_tests.cpp's `special_state()`: special doubles in both doubles.
pub fn special_state() -> State {
    State {
        width: f64::from_bits(0x7ff0_0000_000a_bcde),
        height: i32::MIN,
        dirty: false,
        wants_tick: true,
        next_tick_in: f64::from_bits(0x7ff8_0000_0001_2345),
        frame_asked: false,
    }
}

pub fn text(font: i32, text: &[u8]) -> TextInFont {
    TextInFont { font, text: text.to_vec() }
}

/// remote_tests.cpp's `widths_requests()`.
pub fn widths_requests() -> Vec<(String, WidthsRequest)> {
    let several = WidthsRequest { texts: vec![text(3, b"hello"), text(-1, b"\0\xff\n"), text(i32::MIN, b""), text(3, b"hello")] };
    vec![
        ("no widths request".into(), WidthsRequest::default()),
        ("a widths request of one text".into(), WidthsRequest { texts: vec![text(7, b"word")] }),
        ("a widths request repeated, empty and bytes".into(), several),
        ("a widths request at the extremes".into(), WidthsRequest { texts: vec![text(i32::MAX, b" "), text(0, &[b'x'; 28])] }),
    ]
}

/// remote_tests.cpp's `widths_answers()`, with the requests they answer.
pub fn widths_answers() -> Vec<(String, WidthsAnswer, WidthsRequest)> {
    let four = WidthsRequest { texts: vec![text(1, b"a"), text(1, b"bb"), text(2, b"a"), text(2, b"")] };
    let specials: Vec<f32> = [0x7fc1_2345u32, 0xffc5_4321, 0x7f80_0abc, 0x8000_0000, 0x7f80_0000, 0xff80_0000, 0x0000_0003, 0x7f7f_ffff]
        .iter()
        .map(|&b| f32::from_bits(b))
        .collect();
    vec![
        ("no widths".into(), WidthsAnswer::default(), WidthsRequest::default()),
        ("one width".into(), WidthsAnswer { widths: vec![42.25] }, WidthsRequest { texts: vec![text(3, b"word")] }),
        ("special widths".into(), WidthsAnswer { widths: specials[..4].to_vec() }, four.clone()),
        ("more special widths".into(), WidthsAnswer { widths: specials[4..].to_vec() }, four.clone()),
        ("ordinary widths".into(), WidthsAnswer { widths: vec![8.0, 0.0, 16.5, 1e-3] }, four),
    ]
}

/// remote_tests.cpp's `frame_deltas()`: within its tight caps, each no shorter than the Frame it stands for.
pub fn frame_deltas() -> Vec<(String, FrameDelta)> {
    let literal = FrameDelta {
        seq: 10,
        state: ordinary_state(),
        base: 9,
        runs: vec![DeltaRun { offset: 0, length: 0, literal: b"\x01\x01\0\0\0\0".to_vec() }],
        damage: None,
    };
    let mixed = FrameDelta {
        seq: u64::MAX,
        state: special_state(),
        base: u64::MAX - 1,
        runs: vec![
            DeltaRun { offset: 0, length: 6, literal: Vec::new() },
            DeltaRun { offset: 0, length: 0, literal: b"abc".to_vec() },
            DeltaRun { offset: 55, length: 9, literal: Vec::new() },
            DeltaRun { offset: 0, length: 0, literal: b"\0\xff".to_vec() },
        ],
        damage: Some(vec![Rect::new(1.0, 2.0, 3.0, 4.0), Rect::new(0.0, 0.0, f32::from_bits(0x7fc1_2345), 0.0)]),
    };
    let still = FrameDelta {
        seq: 1,
        state: State::default(),
        base: 0,
        runs: vec![DeltaRun { offset: 3, length: 1, literal: Vec::new() }],
        damage: Some(Vec::new()),
    };
    vec![
        ("a frame delta of one literal".into(), literal),
        ("a frame delta of copies and literals".into(), mixed),
        ("a frame delta of one byte copied".into(), still),
    ]
}

/// remote_tests.cpp's `ends_alike`: runs that rebuild `list` from `base` without looking at commands: a copy of the
/// bytes they begin with alike, the rest of `list` as it is, then a copy of the bytes they end with alike.
pub fn ends_alike(base: &[u8], list: &[u8]) -> Vec<DeltaRun> {
    let (mut head, mut tail) = (0, 0);
    while head < base.len() && head < list.len() && base[head] == list[head] {
        head += 1;
    }
    while tail < base.len() - head && tail < list.len() - head && base[base.len() - 1 - tail] == list[list.len() - 1 - tail] {
        tail += 1;
    }
    let mut runs = Vec::new();
    if head > 0 {
        runs.push(DeltaRun { offset: 0, length: head as u32, literal: Vec::new() });
    }
    if list.len() > head + tail {
        runs.push(DeltaRun { offset: 0, length: 0, literal: list[head..list.len() - tail].to_vec() });
    }
    if tail > 0 {
        runs.push(DeltaRun { offset: (base.len() - tail) as u32, length: tail as u32, literal: Vec::new() });
    }
    runs
}

pub fn list_bytes(list: &DisplayList) -> Vec<u8> {
    let mut out = Vec::new();
    assert!(wire::encode_list(list, &wire::Caps::default(), &mut out), "a list the checks build encodes");
    out
}

/// remote_tests.cpp's `compact_list()`, member for member: five commands, two of them sharing a path, one with a
/// filter.
pub fn cpp_compact_list() -> DisplayList {
    let path = Arc::new(Path {
        ops: vec![PathOp::Move, PathOp::Line, PathOp::Close],
        points: vec![0.0, 0.0, 10.0, 0.0],
        fill: true,
        ..Path::default()
    });
    let filter = Arc::new(vec![twidget_wire::Filter { kind: twidget_wire::FilterKind::Blur, amount: 2.5, ..Default::default() }]);
    let mut list = vec![Command::default(); 5];
    list[0].rect = Rect::new(f32::from_bits(0x7f80_0abc), 2.0, 30.0, 40.0);
    list[0].color = Color::rgba(10, 20, 30, 255);
    list[0].key = 7;
    list[1].kind = Cmd::Text;
    list[1].text = b"hello".to_vec();
    list[1].font = 3;
    list[1].x = 1.5;
    list[1].y = 12.0;
    list[2].kind = Cmd::Path;
    list[2].path = Some(Arc::clone(&path));
    list[3].kind = Cmd::Path;
    list[3].path = Some(path);
    list[3].opacity = 0.5;
    list[4].kind = Cmd::FilterPush;
    list[4].filter = Some(filter);
    list[4].rect = Rect::new(0.0, 0.0, 100.0, 100.0);
    list
}

/// remote_tests.cpp's `moved_list()`: its compact list with one member changed.
pub fn moved_list() -> DisplayList {
    let mut list = cpp_compact_list();
    list[1].x = 9.5;
    list
}

/// remote_tests.cpp's `third_list()`.
pub fn third_list() -> DisplayList {
    let mut list = moved_list();
    list.push(list[3].clone());
    list
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
            Kind::PixelsAnswer | Kind::WidthsAnswer => continue,
            Kind::WidthsRequest => {
                let n = random.below(5);
                encoded(&WidthsRequest {
                    texts: (0..n).map(|_| TextInFont { font: any_i32(random), text: any_bytes(random, 12) }).collect(),
                })
            }
            Kind::FrameDelta => {
                let n = 1 + random.below(4);
                let runs = (0..n)
                    .map(|_| {
                        if random.chance(2) {
                            DeltaRun { offset: random.below(300) as u32, length: 1 + random.below(50) as u32, literal: Vec::new() }
                        } else {
                            let mut literal = any_bytes(random, 12);
                            literal.push(random.next() as u8);
                            DeltaRun { offset: 0, length: 0, literal }
                        }
                    })
                    .collect();
                let damage = if random.chance(2) { Some(vec![Rect::new(0.0, 0.0, any_f64(random) as f32, 1.0)]) } else { None };
                encoded(&FrameDelta {
                    seq: u64::from(random.next()),
                    state: any_state(random),
                    base: u64::from(random.next()),
                    runs,
                    damage,
                })
            }
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
