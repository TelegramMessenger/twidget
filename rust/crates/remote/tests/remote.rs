//! tests/remote_tests.cpp's message checks, ported: the documented bytes, the kinds and their ranges,
//! pixels answered exactly as asked, the carried values' caps, lengths of carried values, an encoder's
//! room under the message cap, and cuts and random changes of every sample; the widths messages, the texts
//! one request holds, and frame deltas rebuilt and scanned, their bytes and their bounds. Its platform end's session
//! is the main process's: the checks of the platform end's own code are in twidget_ends' tests.

mod common;

use common::*;
use twidget_wire as wire;
use twidget_wire::DisplayList;

const EARLIER: &[u8] = b"earlier bytes";

fn golden<T: Message + std::fmt::Debug + PartialEq>(what: &str, value: T, expected: &[u8]) {
    let mut out = Vec::new();
    assert!(value.encode(&Caps::default(), &mut out), "{what}: encodes");
    assert_eq!(hex(&out), hex(expected), "{what}: the documented bytes");
    let back = T::decode(expected, &Caps::default()).unwrap_or_else(|| panic!("{what}: the documented bytes decode"));
    let mut again = Vec::new();
    assert!(back.encode(&Caps::default(), &mut again) && again == expected, "{what}: decodes to what encodes to them");
}

#[test]
fn golden_bytes() {
    golden(
        "a forced tick",
        FrameRequest { seq: 0x0102030405060708, tick: true, paint: Paint::Force },
        &unhex("01 01  08 07 06 05 04 03 02 01  01  02"),
    );
    let state = State { width: 320.5, height: -7, dirty: true, wants_tick: false, next_tick_in: f64::INFINITY, frame_asked: true };
    golden("a state", state, &unhex("41 01  00 00 00 00 00 08 74 40  f9 ff ff ff  01  00  00 00 00 00 00 00 f0 7f  01"));
    golden(
        "a claimed pointer",
        Pointer { cursor: b"pointer".to_vec(), claim: Some(GestureClaim { x: true, y: false, hold: true }) },
        &unhex("43 01  07 00 00 00 70 6f 69 6e 74 65 72  01  01 00 01"),
    );
    golden("an unclaimed pointer", Pointer { cursor: Vec::new(), claim: None }, &unhex("43 01  00 00 00 00  00"));
    golden("clearing the selection", ClearSelection, &unhex("07 01"));
    golden(
        "a painted frame of nothing",
        Frame { seq: 2, state: State::default(), painted: true, list: Vec::new(), damage: None },
        &unhex(
            "40 01  02 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  00 00 00 00  00  00  00 00 00 00 00 00 f0 7f  00  01  06 00 00 00 01 01 00 00 00 00  03 00 00 00 02 01 00",
        ),
    );
    golden(
        "an unpainted frame",
        Frame { seq: 3, state: State::default(), painted: false, list: Vec::new(), damage: None },
        &unhex("40 01  03 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  00 00 00 00  00  00  00 00 00 00 00 00 f0 7f  00  00"),
    );
    golden(
        "a canvas font request",
        FontRequest { canvas: true, families: b"a".to_vec(), size: 1.0, weight: 2, italic: false, features: Vec::new() },
        &unhex("60 01  01  01 00 00 00 61  00 00 80 3f  02 00 00 00  00"),
    );
    golden(
        "a document font request",
        FontRequest { canvas: false, families: b"a".to_vec(), size: 1.0, weight: 2, italic: true, features: b"b".to_vec() },
        &unhex("60 01  00  01 00 00 00 61  00 00 80 3f  02 00 00 00  01  01 00 00 00 62"),
    );
    golden(
        "a pixels request",
        PixelsRequest { id: 1, x: -1, y: 2, w: 3, h: 4 },
        &unhex("63 01  01 00 00 00  ff ff ff ff  02 00 00 00  03 00 00 00  04 00 00 00"),
    );
    golden("messages", Messages { texts: vec![b"ab".to_vec(), Vec::new()] }, &unhex("42 01  02 00 00 00  02 00 00 00 61 62  00 00 00 00"));

    let mut metrics = Vec::new();
    wire::encode_metrics(&ordinary_metrics(), &mut metrics);
    let mut expected = unhex("20 01  fe ff ff ff");
    expected.extend_from_slice(&(metrics.len() as u32).to_le_bytes());
    expected.extend_from_slice(&metrics);
    golden("a font answer", FontAnswer { font: -2, metrics: ordinary_metrics() }, &expected);

    let mut batch = Vec::new();
    assert!(wire::encode_batch(&compact_batch(), &wire_caps(), &mut batch));
    let mut expected = unhex("47 01  01 00 00 00  02 00 00 00  03 00 00 00");
    expected.extend_from_slice(&(batch.len() as u32).to_le_bytes());
    expected.extend_from_slice(&batch);
    golden("a canvas run", CanvasRun { id: 1, w: 2, h: 3, ops: compact_batch() }, &expected);

    let mut run = Vec::new();
    assert!(encode_canvas_run(1, 2, 3, &compact_batch(), &Caps::default(), &mut run));
    assert_eq!(run, expected, "encode_canvas_run writes what a CanvasRun encodes to");
    let mut small = Caps::default();
    small.values.canvas_ops = 2;
    let mut kept = EARLIER.to_vec();
    assert!(
        !encode_canvas_run(1, 2, 3, &compact_batch(), &small, &mut kept) && kept == EARLIER,
        "a batch past its cap is refused, the earlier bytes kept"
    );
}

/// The kinds: 28 of them in four ranges, kind_of naming exactly those of version 1 from the header alone.
#[test]
fn kinds() {
    let ranges = [(1u8, 8u8), (32, 36), (64, 73), (96, 100)];
    let mut known = 0;
    for byte in 0..=255u8 {
        let in_range = ranges.iter().any(|(a, b)| (*a..=*b).contains(&byte));
        assert_eq!(Kind::from_u8(byte).is_some(), in_range, "kind {byte}");
        assert_eq!(kind_of(&[byte, 1]).map(|k| k as u8), in_range.then_some(byte), "kind_of [{byte}, 1]");
        assert_eq!(kind_of(&[byte, 1, 0xff, 0xff]).map(|k| k as u8), in_range.then_some(byte), "kind_of looks at the header alone");
        for version in [0u8, 2, 0xff] {
            assert!(kind_of(&[byte, version]).is_none(), "kind_of [{byte}, {version}]");
        }
        known += usize::from(in_range);
    }
    assert_eq!(known, 28);
    assert!(kind_of(&[]).is_none() && kind_of(&[64]).is_none());
    for t in ["size", "gesture", "limit", "violation", "bubble"] {
        assert!(is_platform_message(t.as_bytes()), "{t}");
    }
    for t in ["", "Size", "event", "result", "sizes", "size "] {
        assert!(!is_platform_message(t.as_bytes()), "{t:?}");
    }
}

fn pixels_answer(declared: u32, bytes: usize) -> Vec<u8> {
    let mut b = unhex("23 01 01");
    b.extend_from_slice(&declared.to_le_bytes());
    b.extend((0..bytes).map(|i| i as u8));
    b
}

/// A pixels answer holds exactly the w × h × 4 bytes of the request it answers, within the pixel cap; a
/// request asks for no more than a canvas holds.
#[test]
fn pixels() {
    let six = PixelsRequest { id: 1, x: 0, y: 0, w: 2, h: 3 };
    let failed = unhex("23 01 00");
    let caps = Caps::default();
    let out = PixelsAnswer::decode(&pixels_answer(24, 24), &caps, &six).expect("a 2 × 3 answer of 24 bytes is taken");
    assert!(out.ok && out.rgba.len() == 24);
    for n in [0u32, 4, 20, 23, 25, 28, 48] {
        assert!(PixelsAnswer::decode(&pixels_answer(n, n as usize), &caps, &six).is_none(), "a 2 × 3 answer of {n} bytes");
    }
    assert!(PixelsAnswer::decode(&pixels_answer(24, 23), &caps, &six).is_none());
    assert!(PixelsAnswer::decode(&pixels_answer(24, 25), &caps, &six).is_none());
    for n in [0x7fffffffu32, 0x80000000, 0xffffffff, 0x40000000] {
        assert!(PixelsAnswer::decode(&pixels_answer(n, 24), &caps, &six).is_none(), "declaring {n:#x}");
    }
    assert!(PixelsAnswer::decode(&[failed.clone(), vec![0; 4]].concat(), &caps, &six).is_none(), "a failed answer with a count after it");
    assert!(PixelsAnswer::decode(&unhex("23 01 01"), &caps, &six).is_none(), "an answer without its count");
    let mut pixel_cap = Caps::default();
    pixel_cap.values.pixels = 24;
    let out = PixelsAnswer::decode(&pixels_answer(24, 24), &pixel_cap, &six).expect("taken under a pixel cap of 24");
    pixel_cap.values.pixels = 23;
    assert!(PixelsAnswer::decode(&pixels_answer(24, 24), &pixel_cap, &six).is_none());
    let mut kept = EARLIER.to_vec();
    assert!(!out.encode(&pixel_cap, &mut kept) && kept == EARLIER, "an answer past the pixel cap is not encoded");
    let mut message_cap = Caps::default();
    message_cap.message = 3 + 4 + 24;
    let mut written = Vec::new();
    assert!(
        PixelsAnswer::decode(&pixels_answer(24, 24), &message_cap, &six).is_some() && out.encode(&message_cap, &mut written),
        "31 bytes"
    );
    message_cap.message = 30;
    assert!(PixelsAnswer::decode(&pixels_answer(24, 24), &message_cap, &six).is_none());
    let empty = PixelsRequest { id: 1, x: 0, y: 0, w: 0, h: 3 };
    let none = PixelsAnswer::decode(&pixels_answer(0, 0), &caps, &empty).expect("no pixels for a request of none");
    assert!(none.ok && none.rgba.is_empty());
    assert!(PixelsAnswer::decode(&pixels_answer(4, 4), &caps, &empty).is_none());
    let bad = [(-1, 1), (1, -1), (8193, 1), (1, 8193), (8192, 2049), (4097, 4096), (i32::MIN, i32::MIN), (i32::MAX, 0)];
    for (w, h) in bad {
        let asked = PixelsRequest { id: 0, x: 0, y: 0, w, h };
        assert!(PixelsAnswer::decode(&failed, &caps, &asked).is_none(), "a failed answer to {w} × {h}");
        assert!(PixelsAnswer::decode(&pixels_answer(0, 0), &caps, &asked).is_none(), "an empty answer to {w} × {h}");
        let mut request = unhex("63 01 01 00 00 00 00 00 00 00 00 00 00 00");
        request.extend_from_slice(&w.to_le_bytes());
        request.extend_from_slice(&h.to_le_bytes());
        assert!(PixelsRequest::decode(&request, &caps).is_none(), "a request of {w} × {h}");
        let mut none = EARLIER.to_vec();
        assert!(!asked.encode(&caps, &mut none) && none == EARLIER, "a request of {w} × {h} is not encoded");
    }
    for (w, h) in [(8192, 2048), (4096, 4096), (8192, 0), (0, 8192), (0, 0)] {
        let asked = PixelsRequest { id: 0, x: 0, y: 0, w, h };
        assert!(PixelsAnswer::decode(&failed, &caps, &asked).is_some_and(|a| !a.ok), "a failed answer to {w} × {h} is taken");
        assert!(PixelsRequest::decode(&encoded(&asked), &caps).is_some_and(|r| r.w == w && r.h == h), "a request of {w} × {h} is taken");
    }
    let unfailed = PixelsAnswer { ok: false, rgba: vec![1] };
    let mut kept = EARLIER.to_vec();
    assert!(!unfailed.encode(&caps, &mut kept) && kept == EARLIER, "a failed answer carries no pixels");
}

/// A cap of the carried values bounds them inside a message both ways.
#[test]
fn nested_caps() {
    let frame = Frame {
        seq: 1,
        state: State::default(),
        painted: true,
        list: compact_list(),
        damage: Some(vec![Rect::new(1.0, 2.0, 3.0, 4.0), Rect::new(5.0, 6.0, 7.0, 8.0)]),
    };
    let run = CanvasRun { id: 1, w: 2, h: 3, ops: compact_batch() };
    type Set = fn(&mut wire::Caps, usize);
    let frame_caps: [(&str, Set, usize); 4] = [
        ("commands", |c, v| c.commands = v, 3),
        ("rects", |c, v| c.rects = v, 2),
        ("text", |c, v| c.text = v, 5),
        ("path_ops", |c, v| c.path_ops = v, 3),
    ];
    let run_caps: [(&str, Set, usize); 3] =
        [("canvas_ops", |c, v| c.canvas_ops = v, 3), ("pixels", |c, v| c.pixels = v, 16), ("clips", |c, v| c.clips = v, 1)];
    let check = |what: &str,
                 bytes: &[u8],
                 decode: &dyn Fn(&[u8], &Caps) -> bool,
                 encode: &dyn Fn(&Caps, &mut Vec<u8>) -> bool,
                 set: Set,
                 at: usize| {
        let mut fits = Caps::default();
        set(&mut fits.values, at);
        let mut short = Caps::default();
        set(&mut short.values, at - 1);
        assert!(decode(bytes, &fits), "{what}: taken at its cap");
        assert!(!decode(bytes, &short), "{what}: refused one past its cap");
        let mut kept = EARLIER.to_vec();
        assert!(!encode(&short, &mut kept) && kept == EARLIER, "{what}: not encoded one past its cap");
    };
    let frame_bytes = encoded(&frame);
    for (name, set, at) in frame_caps {
        check(&format!("a frame's {name}"), &frame_bytes, &|b, c| Frame::decode(b, c).is_some(), &|c, o| frame.encode(c, o), set, at);
    }
    let run_bytes = encoded(&run);
    for (name, set, at) in run_caps {
        check(&format!("a canvas run's {name}"), &run_bytes, &|b, c| CanvasRun::decode(b, c).is_some(), &|c, o| run.encode(c, o), set, at);
    }
}

/// An encoder checks a counted field's room before it appends: past the message cap, nothing is
/// appended and the earlier bytes stay.
#[test]
fn encoder_room() {
    let mut caps = Caps::default();
    caps.message = 1000;
    let big = vec![b'x'; 1 << 20];
    let refuse = |what: &str, encode: &dyn Fn(&mut Vec<u8>) -> bool| {
        let mut out = EARLIER.to_vec();
        assert!(!encode(&mut out) && out == EARLIER, "{what}: refused past the message cap");
    };
    refuse("a selection of 1 MiB", &|o| Selection { text: big.clone() }.encode(&caps, o));
    refuse("an image URL of 1 MiB", &|o| ImageRequest { id: 1, url: big.clone() }.encode(&caps, o));
    refuse("a width request's text of 1 MiB", &|o| WidthRequest { font: 1, text: big.clone() }.encode(&caps, o));
    refuse("a report of a 1 MiB text", &|o| Messages { texts: vec![b"a".to_vec(), big.clone()] }.encode(&caps, o));
    refuse("pixels of 1 MiB", &|o| PixelsAnswer { ok: true, rgba: big.clone() }.encode(&caps, o));
    let mut exact = Caps::default();
    exact.message = 2 + 4 + 3;
    let mut out = Vec::new();
    assert!(Selection { text: b"abc".to_vec() }.encode(&exact, &mut out), "a selection exactly at the message cap");
    assert!(Selection::decode(&out, &exact).is_some());
    exact.message -= 1;
    assert!(!Selection { text: b"abc".to_vec() }.encode(&exact, &mut Vec::new()), "a byte past it");
    assert!(Selection::decode(&out, &exact).is_none(), "and refused by the decoder too");
}

/// A carried value's length must be its own: a byte shorter or longer, empty, of another form or
/// version, or vast, and the message is refused.
#[test]
fn value_lengths() {
    let mut metrics = Vec::new();
    wire::encode_metrics(&ordinary_metrics(), &mut metrics);
    let mut list = Vec::new();
    assert!(wire::encode_list(&compact_list(), &wire_caps(), &mut list));
    let mut damage = Vec::new();
    assert!(wire::encode_damage(&Some(vec![Rect::new(1.0, 2.0, 3.0, 4.0)]), &wire_caps(), &mut damage));
    let mut batch = Vec::new();
    assert!(wire::encode_batch(&compact_batch(), &wire_caps(), &mut batch));
    let frame_head = unhex("40 01  01 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  00 00 00 00  00  00  00 00 00 00 00 00 f0 7f  00  01");
    let with_len = |n: u32, value: &[u8]| [&n.to_le_bytes()[..], value].concat();
    type Build = Box<dyn Fn(u32, &[u8]) -> Vec<u8>>;
    let sites: Vec<(&str, Vec<u8>, Vec<u8>, Build)> = vec![
        (
            "a font answer's metrics",
            metrics.clone(),
            damage.clone(),
            Box::new(move |n, v| [unhex("20 01 01 00 00 00"), with_len(n, v)].concat()),
        ),
        (
            "a canvas run's batch",
            batch.clone(),
            list.clone(),
            Box::new(move |n, v| [unhex("47 01 01 00 00 00 02 00 00 00 03 00 00 00"), with_len(n, v)].concat()),
        ),
        ("a frame's list", list.clone(), damage.clone(), {
            let (head, damage) = (frame_head.clone(), damage.clone());
            Box::new(move |n, v| [head.clone(), with_len(n, v), with_len(damage.len() as u32, &damage)].concat())
        }),
        ("a frame's damage", damage.clone(), list.clone(), {
            let (head, list) = (frame_head.clone(), list.clone());
            Box::new(move |n, v| [head.clone(), with_len(list.len() as u32, &list), with_len(n, v)].concat())
        }),
    ];
    let caps = Caps::default();
    for (name, value, other, build) in &sites {
        let n = value.len() as u32;
        assert_eq!(answer(&build(n, value), &caps, &caps).0, 1, "{name} at its length");
        let refused = |what: &str, bytes: Vec<u8>| assert_eq!(answer(&bytes, &caps, &caps).0, 0, "{name} {what}");
        refused("a byte shorter", build(n - 1, value));
        refused("a byte longer", build(n + 1, value));
        refused("a byte shorter, cut to it", build(n - 1, &value[..value.len() - 1]));
        refused("with a byte after it inside its length", build(n + 1, &[value.as_slice(), &[0]].concat()));
        refused("empty", build(0, &[]));
        refused("of another form", build(other.len() as u32, other));
        let mut version = value.clone();
        version[1] = 2;
        refused("of version 2", build(n, &version));
        let mut form = value.clone();
        form[0] = 0x7f;
        refused("of no form", build(n, &form));
        for vast in [0x7fffffffu32, 0x80000000, 0xffffffff] {
            refused("declaring a vast length", build(vast, value));
        }
    }
}

/// Every sample comes back to its bytes; every cut of it, a byte after it and it twice over are refused.
#[test]
fn round_trips_and_cuts() {
    let caps = Caps::default();
    for (name, bytes) in samples() {
        assert_eq!(answer(&bytes, &caps, &caps), (1, bytes.clone()), "{name} comes back to its bytes");
        for n in 0..bytes.len() {
            assert_eq!(answer(&bytes[..n], &caps, &caps).0, 0, "{name} cut to {n} bytes");
        }
        for extra in [0u8, 0xff] {
            assert_eq!(answer(&[bytes.as_slice(), &[extra]].concat(), &caps, &caps).0, 0, "{name} with {extra:#x} after it");
        }
        assert_eq!(answer(&[bytes.clone(), bytes.clone()].concat(), &caps, &caps).0, 0, "{name} twice over");
        let mut tight = Caps::default();
        tight.message = bytes.len();
        assert_eq!(answer(&bytes, &tight, &tight).0, 1, "{name} at a message cap of its size");
        tight.message = bytes.len() - 1;
        assert_eq!(answer(&bytes, &tight, &tight).0, 0, "{name} past a message cap of a byte less");
    }
}

/// Random changes to every sample: what is taken encodes again to what decodes the same.
#[test]
fn mutations() {
    let caps = Caps::default();
    let mut taken = 0;
    let mut refused = 0;
    for (index, (name, bytes)) in samples().iter().enumerate() {
        let mut random = Lcg::new(0x7e30_0000 + index as u64);
        for round in 0..2000 {
            let mut changed = bytes.clone();
            let edits = mutate(&mut random, &mut changed);
            match answer(&changed, &caps, &caps) {
                (1, again) => {
                    taken += 1;
                    assert_eq!(answer(&again, &caps, &caps), (1, again.clone()), "{name} round {round}:{edits}: comes back the same");
                }
                (0 | 3, _) => refused += 1,
                (status, _) => panic!("{name} round {round}:{edits}: status {status}"),
            }
        }
    }
    assert!(taken > 0 && refused > 0);
    eprintln!("mutations: {taken} taken, {refused} refused");
}

// ---- Widths and frame deltas (twidget f0a070d) ----------------------------------------------------------------

/// Bytes as docs/TWIDGET_CORE.md lays them out, put together without the codec (remote_tests.cpp's `Bytes`).
#[derive(Clone, Default)]
struct B(Vec<u8>);

impl B {
    fn head(mut self, kind: Kind) -> B {
        self.0.extend([kind as u8, VERSION]);
        self
    }
    fn u8(mut self, v: u8) -> B {
        self.0.push(v);
        self
    }
    fn u32(mut self, v: u32) -> B {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn i32(self, v: i32) -> B {
        self.u32(v as u32)
    }
    fn u64(mut self, v: u64) -> B {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn f32(self, v: f32) -> B {
        self.u32(v.to_bits())
    }
    fn f64(self, v: f64) -> B {
        self.u64(v.to_bits())
    }
    fn raw(mut self, v: &[u8]) -> B {
        self.0.extend_from_slice(v);
        self
    }
    fn str(self, v: &[u8]) -> B {
        self.u32(v.len() as u32).raw(v)
    }
    fn value(self, v: &[u8]) -> B {
        self.str(v)
    }
    fn state(self, s: &State) -> B {
        self.f64(s.width).i32(s.height).u8(u8::from(s.dirty)).u8(u8::from(s.wants_tick)).f64(s.next_tick_in).u8(u8::from(s.frame_asked))
    }
}

/// remote_tests.cpp's `EARLIER`: bytes a value is appended after, which an encoder keeps.
const EARLIER_CPP: &[u8] = b"an earlier value\0\xff";

fn widths_golden(what: &str, value: &WidthsAnswer, expected: &[u8], asked: &WidthsRequest) {
    let mut out = Vec::new();
    assert!(value.encode(&Caps::default(), &mut out) && out == expected, "{what}: encodes to the documented bytes");
    let back = WidthsAnswer::decode(expected, &Caps::default(), asked).unwrap_or_else(|| panic!("{what}: the documented bytes decode"));
    assert_eq!(back.widths.iter().map(|w| w.to_bits()).collect::<Vec<_>>(), value.widths.iter().map(|w| w.to_bits()).collect::<Vec<_>>());
}

/// The widths messages' and a frame delta's documented bytes; a Frame encoded with its list's place and decoded with
/// its list's view; delta_ids and delta_scan.
#[test]
fn golden_widths_and_deltas() {
    let two = WidthsRequest { texts: vec![text(1, b"ab"), text(-1, b"")] };
    golden("a widths request", two.clone(), &unhex("64 01  02 00 00 00  01 00 00 00  02 00 00 00 61 62  ff ff ff ff  00 00 00 00"));
    widths_golden(
        "a widths answer",
        &WidthsAnswer { widths: vec![1.0, -0.0] },
        &unhex("24 01  02 00 00 00  00 00 80 3f  00 00 00 80"),
        &two,
    );
    golden("an empty widths request", WidthsRequest::default(), &unhex("64 01  00 00 00 00"));
    widths_golden("an empty widths answer", &WidthsAnswer::default(), &unhex("24 01  00 00 00 00"), &WidthsRequest::default());

    golden(
        "a frame delta",
        FrameDelta {
            seq: 2,
            state: State::default(),
            base: 1,
            runs: vec![DeltaRun { offset: 6, length: 4, literal: Vec::new() }, DeltaRun { offset: 0, length: 0, literal: b"ab".to_vec() }],
            damage: None,
        },
        &unhex(concat!(
            "49 01  02 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  00 00 00 00  00  00  00 00 00 00 00 00 f0 7f  00  01 00 00 00 00 00 00 00  06 00 00 00  02 00 00 00",
            "  00  06 00 00 00  04 00 00 00  01  02 00 00 00 61 62  03 00 00 00 02 01 00"
        )),
    );
    let mut list = Vec::new();
    let mut starts = Vec::new();
    assert!(
        wire::encode_list_with_starts(&cpp_compact_list(), &wire_caps(), &mut list, &mut starts, usize::MAX),
        "the compact list encodes with its starts"
    );
    let frame = Frame {
        seq: 5,
        state: ordinary_state(),
        painted: true,
        list: cpp_compact_list(),
        damage: Some(vec![Rect::new(1.0, 2.0, 3.0, 4.0)]),
    };
    let untouched = WrittenList { at: 1, size: 2, starts: vec![3] };
    let mut place = untouched.clone();
    let mut written = EARLIER_CPP.to_vec();
    assert!(
        encode_frame_placed(&frame, &Caps::default(), &mut written, list.len(), &mut place)
            && written == [EARLIER_CPP, &encoded(&frame)].concat(),
        "a painted Frame written with its list's place is the Frame's own bytes"
    );
    assert!(
        place.at == EARLIER_CPP.len() + 2 + 8 + 23 + 1 + 4
            && place.size == list.len()
            && written[place.at..place.at + place.size] == list[..]
            && place.starts == starts,
        "its list lies where the place says, after the header, seq, state, flag and length, with the list's own starts"
    );
    written.clear();
    assert!(
        encode_frame_placed(&frame, &Caps::default(), &mut written, list.len() - 1, &mut place)
            && written == encoded(&frame)
            && place.size == list.len()
            && place.starts.is_empty(),
        "a list a byte past the bytes given is written the same, with no starts"
    );
    let mut one_short = Caps::default();
    one_short.frame = encoded(&frame).len() - 1;
    written = EARLIER_CPP.to_vec();
    place = untouched.clone();
    assert!(
        !encode_frame_placed(&frame, &one_short, &mut written, usize::MAX, &mut place) && written == EARLIER_CPP && place == untouched,
        "a Frame past the frame cap is refused, the earlier value and the place kept"
    );
    let unpainted = Frame { seq: 6, state: State::default(), painted: false, list: Vec::new(), damage: None };
    written.clear();
    assert!(
        encode_frame_placed(&unpainted, &Caps::default(), &mut written, usize::MAX, &mut place)
            && written == encoded(&unpainted)
            && place.size == 0
            && place.starts.is_empty(),
        "an unpainted Frame has no list to place"
    );
    let bytes = encoded(&frame);
    let (_, viewed) = decode_frame_viewed(&bytes, &Caps::default()).expect("a painted Frame decodes with its view");
    let start = bytes.as_ptr() as usize;
    assert!(
        viewed == list.as_slice() && viewed.as_ptr() as usize >= start && (viewed.as_ptr() as usize) < start + bytes.len(),
        "a painted Frame's list is viewed where it lies in the message"
    );
    assert!(
        decode_frame_viewed(&encoded(&unpainted), &Caps::default()).is_some_and(|(_, v)| v.is_empty()),
        "an unpainted Frame views no list"
    );
    assert!(decode_frame_viewed(&bytes, &one_short).is_none(), "a Frame past the frame cap is refused");
    let delta = encoded(&FrameDelta {
        seq: u64::MAX,
        state: special_state(),
        base: 0x0102030405060708,
        runs: vec![DeltaRun { offset: 0, length: 0, literal: b"x".to_vec() }],
        damage: None,
    });
    assert_eq!(delta_ids(&delta), Some((u64::MAX, 0x0102030405060708)), "delta_ids reads a FrameDelta's seq and base");
    assert_eq!(delta_ids(&delta[..40]), None, "delta_ids needs both fields whole");
    assert_eq!(delta_ids(&bytes), None, "delta_ids reads no other kind");
    let no_rects = unhex("02 01 00");
    let scan = delta_scan(&delta, &Caps::default());
    assert!(
        scan.is_some_and(|(seq, length, damage)| seq == u64::MAX
            && length == 1
            && damage == no_rects.as_slice()
            && damage.as_ptr() as usize == delta.as_ptr() as usize + delta.len() - no_rects.len()),
        "delta_scan reads a FrameDelta's whole seq and its list's length and views its damage value where it lies, at the message's end"
    );
    assert!(FrameDelta::decode(&delta, &Caps::default()).is_some_and(|d| d.seq == u64::MAX), "delta_scan's seq is the one decode gives");
    for n in 0..delta.len() {
        not_scanned(&format!("delta_scan of a delta cut to {n} bytes"), &delta[..n], &Caps::default());
    }
    assert!(delta_ids(&delta[..41]).is_some(), "delta_ids reads a delta cut right after its base, a cut delta_scan refuses");
    not_scanned("delta_scan of a delta with a byte after it", &[delta.as_slice(), &[0]].concat(), &Caps::default());
    let mut other = delta.clone();
    other[0] = Kind::Frame as u8;
    not_scanned("delta_scan of a delta's bytes under a Frame's header", &other, &Caps::default());
    other = delta.clone();
    other[1] = 2;
    not_scanned("delta_scan of a delta in version 2", &other, &Caps::default());
    not_scanned("delta_scan of a Frame", &bytes, &Caps::default());
}

/// `message` refused by delta_scan.
fn not_scanned(what: &str, message: &[u8], caps: &Caps) {
    assert!(delta_scan(message, caps).is_none(), "{what}: scanned");
}

/// The kinds by direction and role, each answer its request's kind less 64.
#[test]
fn kinds_by_role() {
    let ranges = [(1u8, 8u8), (32, 36), (64, 73), (96, 100)];
    let (mut platform, mut answers, mut renderer, mut requests) = (0, 0, 0, 0);
    for k in Kind::ALL {
        let v = k as u8;
        platform += usize::from((1..=8).contains(&v));
        answers += usize::from((32..=36).contains(&v));
        renderer += usize::from((64..=73).contains(&v));
        requests += usize::from((96..=100).contains(&v));
    }
    assert!(
        Kind::ALL.len() == 28 && platform == 8 && answers == 5 && renderer == 10 && requests == 5,
        "28 kinds: 8 from the platform, 5 answers, 10 from the renderer, 5 requests"
    );
    assert_eq!(
        Kind::WidthsAnswer as u8 - Kind::FontAnswer as u8,
        Kind::WidthsRequest as u8 - Kind::FontRequest as u8,
        "each answer is its request's kind less 64"
    );
    for byte in 0..=255u8 {
        let in_range = ranges.iter().any(|(a, b)| (*a..=*b).contains(&byte));
        assert_eq!(kind_of(&[byte, 1]).map(|k| k as u8), in_range.then_some(byte), "kind_of [{byte}, 1]");
    }
}

fn widths_answer_bytes(declared: u32, widths: usize) -> Vec<u8> {
    let mut b = B::default().head(Kind::WidthsAnswer).u32(declared);
    for i in 0..widths {
        b = b.f32(i as f32 + 0.5);
    }
    b.0
}

/// remote_tests.cpp's `texts_of`: `n` texts of `bytes` bytes each, in three fonts.
fn texts_of(n: usize, bytes: usize) -> WidthsRequest {
    WidthsRequest { texts: (0..n).map(|i| TextInFont { font: (i % 3) as i32 + 1, text: vec![b'a' + (i % 26) as u8; bytes] }).collect() }
}

fn encoded_under<T: Message>(value: &T, caps: &Caps) -> Vec<u8> {
    let mut out = Vec::new();
    assert!(value.encode(caps, &mut out), "{:?} encodes", T::KIND);
    out
}

/// A widths answer holds exactly one width for each text of the request it answers, which must itself be within the
/// caps; a request's texts are bounded by count, one by one and together, both ways.
#[test]
fn widths() {
    let three = texts_of(3, 1);
    let d = Caps::default();
    let out = WidthsAnswer::decode(&widths_answer_bytes(3, 3), &d, &three).expect("an answer of 3 widths to 3 texts is taken in order");
    assert!(out.widths.len() == 3 && out.widths[0] == 0.5 && out.widths[2] == 2.5);
    for n in [0u32, 1, 2, 4, 6] {
        assert!(WidthsAnswer::decode(&widths_answer_bytes(n, n as usize), &d, &three).is_none(), "an answer of {n} widths to 3 texts");
    }
    assert!(WidthsAnswer::decode(&widths_answer_bytes(3, 2), &d, &three).is_none(), "an answer declaring 3 widths with 2");
    assert!(WidthsAnswer::decode(&widths_answer_bytes(3, 4), &d, &three).is_none(), "an answer declaring 3 widths with 4");
    assert!(
        WidthsAnswer::decode(&[widths_answer_bytes(3, 2), vec![0]].concat(), &d, &three).is_none(),
        "declaring 3 widths with 2 and a byte"
    );
    for n in [0x7fffffffu32, 0x80000000, 0xffffffff, 0x40000000] {
        assert!(WidthsAnswer::decode(&widths_answer_bytes(n, 3), &d, &three).is_none(), "an answer declaring {n:#x} widths");
    }
    assert!(
        WidthsAnswer::decode(&widths_answer_bytes(0, 0), &d, &WidthsRequest::default()).is_some_and(|a| a.widths.is_empty()),
        "no widths to no texts"
    );
    assert!(WidthsAnswer::decode(&widths_answer_bytes(1, 1), &d, &WidthsRequest::default()).is_none(), "one width to no texts");

    let mut caps = Caps::default();
    caps.widths = 3;
    assert!(WidthsAnswer::decode(&widths_answer_bytes(3, 3), &caps, &three).is_some(), "3 widths under a widths cap of 3");
    caps.widths = 2;
    assert!(WidthsAnswer::decode(&widths_answer_bytes(3, 3), &caps, &three).is_none(), "3 widths under a widths cap of 2");
    let mut kept = EARLIER_CPP.to_vec();
    assert!(!out.encode(&caps, &mut kept) && kept == EARLIER_CPP, "an answer past the widths cap is not encoded, the earlier value kept");
    caps = Caps::default();
    caps.message = 2 + 4 + 3 * 4;
    assert!(
        WidthsAnswer::decode(&widths_answer_bytes(3, 3), &caps, &three).is_some() && out.encode(&caps, &mut kept),
        "an answer of 3 widths takes 18 bytes"
    );
    caps.message = 17;
    assert!(WidthsAnswer::decode(&widths_answer_bytes(3, 3), &caps, &three).is_none(), "under a message cap of 17");
    kept = EARLIER_CPP.to_vec();
    assert!(!out.encode(&caps, &mut kept) && kept == EARLIER_CPP, "an answer past the message cap is not encoded, the earlier value kept");
    caps = Caps::default();
    caps.text = 0;
    assert!(
        WidthsAnswer::decode(&widths_answer_bytes(3, 3), &caps, &three).is_none(),
        "an answer to a request whose texts pass the text cap"
    );
    caps = Caps::default();
    caps.widths_text = 2;
    assert!(
        WidthsAnswer::decode(&widths_answer_bytes(3, 3), &caps, &three).is_none(),
        "an answer to a request whose texts together pass their cap"
    );
    caps.widths_text = 3;
    assert!(
        WidthsAnswer::decode(&widths_answer_bytes(3, 3), &caps, &three).is_some(),
        "an answer to a request whose texts together are at their cap"
    );
    caps = Caps::default();
    caps.widths = 2;
    assert!(WidthsAnswer::decode(&widths_answer_bytes(2, 2), &caps, &three).is_none(), "an answer to a request past the widths cap");

    let pair = WidthsRequest { texts: vec![text(1, b"abc"), text(2, b"de")] };
    let over = WidthsRequest { texts: vec![text(1, b"abc"), text(2, b"def")] };
    let mut total = Caps::default();
    total.widths_text = 5;
    let request = WidthsRequest::decode(&encoded_under(&pair, &total), &total).expect("two texts of 5 bytes together under a cap of 5");
    assert!(request.texts.len() == 2 && request.texts[1].text == b"de");
    let mut open = total;
    open.widths_text = 6;
    assert!(WidthsRequest::decode(&encoded_under(&over, &open), &total).is_none(), "two texts of 6 bytes together under a cap of 5");
    kept = EARLIER_CPP.to_vec();
    assert!(!over.encode(&total, &mut kept) && kept == EARLIER_CPP, "not encoded under a cap of 5, the earlier value kept");
    let mut none = Caps::default();
    none.widths = 0;
    assert!(
        WidthsRequest::decode(&encoded_under(&WidthsRequest::default(), &none), &none).is_some_and(|r| r.texts.is_empty()),
        "an empty request under a widths cap of 0"
    );
    assert!(WidthsRequest::decode(&encoded(&texts_of(1, 1)), &none).is_none(), "a request of one text under a widths cap of 0");
    kept = EARLIER_CPP.to_vec();
    assert!(!texts_of(1, 1).encode(&none, &mut kept) && kept == EARLIER_CPP, "not encoded under a widths cap of 0");
    none = Caps::default();
    none.widths_text = 0;
    assert!(
        WidthsRequest::decode(&encoded_under(&texts_of(3, 0), &none), &none).is_some_and(|r| r.texts.len() == 3),
        "empty texts under a texts cap of 0"
    );
    assert!(WidthsRequest::decode(&encoded(&texts_of(1, 1)), &none).is_none(), "a 1-byte text under a texts cap of 0");
    caps = Caps::default();
    caps.widths = 1024;
    caps.widths_text = 64 << 10;
    assert!(
        WidthsRequest::decode(&encoded_under(&texts_of(1024, 64), &caps), &caps).is_some_and(|r| r.texts.len() == 1024),
        "1,024 texts of 64 KiB together"
    );
    let mut wider = caps;
    wider.widths = 1025;
    assert!(WidthsRequest::decode(&encoded_under(&texts_of(1025, 1), &wider), &caps).is_none(), "1,025 texts under a widths cap of 1,024");
    wider = caps;
    wider.widths_text = (64 << 10) + 1;
    let mut heavy = texts_of(1024, 64);
    heavy.texts.last_mut().unwrap().text.push(b'x');
    assert!(
        WidthsRequest::decode(&encoded_under(&heavy, &wider), &caps).is_none(),
        "1,024 texts of 64 KiB and a byte under a texts cap of 64 KiB"
    );
    // remote_tests.cpp's count sites of a WidthsRequest: its count, a text and its texts together, at their caps and past them.
    for (what, set) in [("count", 0), ("text", 1), ("texts together", 2)] {
        for at in [0usize, 1, 2, 5] {
            let request = match set {
                0 => texts_of(at, 0),
                _ => WidthsRequest { texts: vec![text(1, b""), text(2, &vec![b'q'; at])] },
            };
            let bytes = encoded(&request);
            let mut fits = Caps::default();
            let mut short = Caps::default();
            match set {
                0 => {
                    fits.widths = at;
                    short.widths = at.wrapping_sub(1);
                }
                1 => {
                    fits.text = at;
                    short.text = at.wrapping_sub(1);
                }
                _ => {
                    fits.widths_text = at;
                    short.widths_text = at.wrapping_sub(1);
                }
            }
            assert!(WidthsRequest::decode(&bytes, &fits).is_some(), "a WidthsRequest's {what} of {at} at its cap");
            if at > 0 {
                assert!(WidthsRequest::decode(&bytes, &short).is_none(), "a WidthsRequest's {what} of {at} past its cap");
            }
        }
    }
}

/// The texts one request takes are exactly those encode takes from there, in order, under every cap, and the answer
/// to them encodes as well.
#[test]
fn widths_in_requests() {
    let mut texts = WidthsRequest::default();
    for t in ["a", "bb", "", "dddd", "eeeeeeee", "f", "gg", "hhhhhhhhhhhhhhhh", "i"] {
        let font = (texts.texts.len() % 3) as i32 + 1;
        texts.texts.push(text(font, t.as_bytes()));
    }
    let tight = || {
        let mut c = Caps::default();
        c.message = 4096;
        c.text = 64;
        c.url = 128;
        c.messages = 4;
        c.widths = 4;
        c.widths_text = 32;
        c.frame = 4096;
        c.delta_base = 64;
        c
    };
    let mut cases: Vec<(String, Caps)> = vec![("the default caps".into(), Caps::default()), ("tight caps".into(), tight())];
    let mut with = |name: String, set: &dyn Fn(&mut Caps)| {
        let mut caps = Caps::default();
        set(&mut caps);
        cases.push((name, caps));
    };
    with("a widths cap of 0".into(), &|c| c.widths = 0);
    with("a widths cap of 4".into(), &|c| c.widths = 4);
    with("a text cap of 4".into(), &|c| c.text = 4);
    with("texts of 7 bytes together".into(), &|c| c.widths_text = 7);
    with("texts of 15 bytes together".into(), &|c| c.widths_text = 15);
    for message in 0..=80usize {
        with(format!("a message cap of {message}"), &|c| c.message = message);
    }
    with("caps at SIZE_MAX".into(), &|c| {
        c.widths = usize::MAX;
        c.text = usize::MAX;
        c.widths_text = usize::MAX;
        c.message = usize::MAX;
    });
    let mut prefixes = 0;
    for (name, caps) in &cases {
        for from in 0..=texts.texts.len() {
            let n = widths_in_request(&texts.texts, from, caps);
            for k in 0..=texts.texts.len() - from {
                let request = WidthsRequest { texts: texts.texts[from..from + k].to_vec() };
                let taken = request.encode(caps, &mut Vec::new());
                let answered = WidthsAnswer { widths: vec![1.5; k] }.encode(caps, &mut Vec::new());
                prefixes += 1;
                assert!(
                    k == 0 || taken == (k <= n),
                    "{name}: from text {from}, {n} in a request, but encode {} {k}",
                    if taken { "takes" } else { "refuses" }
                );
                assert!(!taken || answered, "{name}: the answer to {k} texts from {from} encodes too");
            }
        }
    }
    assert!(prefixes > 1000, "every prefix of nine texts is checked under {} caps ({prefixes})", cases.len());
    assert_eq!(widths_in_request(&texts.texts, texts.texts.len() + 1, &Caps::default()), 0, "nothing past the last text");
}

/// remote_tests.cpp's `suite` for the WidthsAnswer: each sample comes back to its bytes against its request; every
/// cut, a byte after it and a tight message cap refuse it.
#[test]
fn widths_answers_round_trip() {
    let caps = Caps::default();
    for (name, value, asked) in widths_answers() {
        let mut bytes = Vec::new();
        assert!(value.encode(&caps, &mut bytes), "{name} encodes");
        assert_eq!(answer_widths(&bytes, &asked, &caps, &caps), (1, bytes.clone()), "{name} comes back to its bytes");
        for n in 0..bytes.len() {
            assert_eq!(answer_widths(&bytes[..n], &asked, &caps, &caps).0, 0, "{name} cut to {n} bytes");
        }
        assert_eq!(answer_widths(&[bytes.as_slice(), &[0]].concat(), &asked, &caps, &caps).0, 0, "{name} with a byte after it");
        let mut tight = Caps::default();
        tight.message = bytes.len();
        assert_eq!(answer_widths(&bytes, &asked, &tight, &tight).0, 1, "{name} at a message cap of its size");
        tight.message = bytes.len() - 1;
        assert_eq!(answer_widths(&bytes, &asked, &tight, &tight).0, 0, "{name} past a message cap of a byte less");
    }
}

/// A widths answer of 1 MiB and a widths request's text of 1 MiB are refused past the message cap, also under widths
/// caps of SIZE_MAX, the earlier bytes kept (remote_tests.cpp's `encoder_room` additions).
#[test]
fn widths_encoder_room() {
    let mut caps = Caps::default();
    caps.message = 1000;
    let big = vec![b'x'; 1 << 20];
    let answer = WidthsAnswer { widths: vec![1.0; big.len() / 4] };
    let mut out = EARLIER_CPP.to_vec();
    assert!(!answer.encode(&caps, &mut out) && out == EARLIER_CPP, "a widths answer of 1 MiB");
    caps.widths = usize::MAX;
    caps.widths_text = usize::MAX;
    assert!(!answer.encode(&caps, &mut out) && out == EARLIER_CPP, "a widths answer of 1 MiB under widths caps of SIZE_MAX");
    assert!(
        !WidthsRequest { texts: vec![TextInFont { font: 1, text: big }] }.encode(&caps, &mut out) && out == EARLIER_CPP,
        "a widths request's text of 1 MiB under widths caps of SIZE_MAX"
    );
}

/// Which commands of a list share their path or filter with an earlier one, by index.
fn sharing(list: &DisplayList) -> Vec<(Option<usize>, Option<usize>)> {
    (0..list.len())
        .map(|i| {
            let path =
                list[i].path.as_ref().and_then(|p| (0..i).find(|&j| list[j].path.as_ref().is_some_and(|q| std::sync::Arc::ptr_eq(p, q))));
            let filter = list[i]
                .filter
                .as_ref()
                .and_then(|p| (0..i).find(|&j| list[j].filter.as_ref().is_some_and(|q| std::sync::Arc::ptr_eq(p, q))));
            (path, filter)
        })
        .collect()
}

fn same_frame(a: &Frame, b: &Frame) -> bool {
    let mut x = Vec::new();
    let mut y = Vec::new();
    a.encode(&Caps::default(), &mut x) && b.encode(&Caps::default(), &mut y) && x == y && sharing(&a.list) == sharing(&b.list)
}

/// `message` is not rebuilt on `base`.
fn not_rebuilt(what: &str, message: &[u8], caps: &Caps, base_seq: u64, base: &[u8]) {
    assert!(rebuild(message, caps, base_seq, base).is_none(), "{what}: rebuilt");
}

struct DeltaCase {
    name: &'static str,
    base: DisplayList,
    list: DisplayList,
    damage: twidget_wire::Damage,
}

fn delta_cases() -> Vec<DeltaCase> {
    let mut longer = cpp_compact_list();
    longer.push(longer[3].clone());
    let mut shorter = cpp_compact_list();
    shorter.remove(2);
    vec![
        DeltaCase {
            name: "one member changed",
            base: cpp_compact_list(),
            list: moved_list(),
            damage: Some(vec![Rect::new(1.0, 2.0, 3.0, 4.0)]),
        },
        DeltaCase { name: "the same list", base: cpp_compact_list(), list: cpp_compact_list(), damage: Some(Vec::new()) },
        DeltaCase { name: "a command added that refers back to a shared path", base: cpp_compact_list(), list: longer, damage: None },
        DeltaCase { name: "a shared path's first user removed", base: cpp_compact_list(), list: shorter, damage: None },
        DeltaCase { name: "from an empty list", base: Vec::new(), list: cpp_compact_list(), damage: None },
        DeltaCase { name: "to an empty list", base: cpp_compact_list(), list: Vec::new(), damage: Some(Vec::new()) },
    ]
}

/// Each case's delta rebuilds its Frame member by member and its list's own bytes, wherever the base lies; every cut,
/// a byte after it and another base are refused.
#[test]
fn deltas_rebuilt() {
    for c in delta_cases() {
        let what = format!("a delta with {}", c.name);
        let (base, list) = (list_bytes(&c.base), list_bytes(&c.list));
        let delta = FrameDelta { seq: 77, state: ordinary_state(), base: 76, runs: ends_alike(&base, &list), damage: c.damage.clone() };
        let bytes = encoded(&delta);
        let expected = Frame { seq: 77, state: ordinary_state(), painted: true, list: c.list.clone(), damage: c.damage.clone() };
        let (out, again) = rebuild(&bytes, &Caps::default(), 76, &base).unwrap_or_else(|| panic!("{what}: rebuilds"));
        assert!(same_frame(&expected, &out), "{what}: rebuilds the Frame member by member, its payloads shared alike");
        assert_eq!(again, list, "{what}: the rebuilt bytes are the list's own");
        let mut whole = Vec::new();
        let mut place = WrittenList::default();
        assert!(
            encode_frame_placed(&expected, &Caps::default(), &mut whole, usize::MAX, &mut place)
                && place.size == again.len()
                && whole[place.at..place.at + place.size] == again[..],
            "{what}: the rebuilt list is the one the Frame it stands for holds"
        );
        let padded = [EARLIER_CPP, &base, EARLIER_CPP].concat();
        let (inside, _) =
            rebuild(&bytes, &Caps::default(), 76, &padded[EARLIER_CPP.len()..EARLIER_CPP.len() + base.len()]).expect("rebuilds inside");
        assert!(same_frame(&expected, &inside), "{what}: rebuilds on a base inside a larger buffer");
        assert_eq!(FrameDelta::decode(&bytes, &Caps::default()), Some(delta.clone()), "{what}: decodes as a FrameDelta member by member");
        for n in 0..bytes.len() {
            not_rebuilt(&format!("{what} cut to {n} bytes"), &bytes[..n], &Caps::default(), 76, &base);
        }
        not_rebuilt(&format!("{what} with a byte after it"), &[bytes.as_slice(), &[0]].concat(), &Caps::default(), 76, &base);
        not_rebuilt(&format!("{what} twice over"), &[bytes.as_slice(), &bytes].concat(), &Caps::default(), 76, &base);
        not_rebuilt(&format!("{what} on frame 75"), &bytes, &Caps::default(), 75, &base);
        not_rebuilt(&format!("{what} on frame 77"), &bytes, &Caps::default(), 77, &base);
        if !base.is_empty() && delta.runs.last().unwrap().literal.is_empty() {
            not_rebuilt(&format!("{what} on a base a byte short"), &bytes, &Caps::default(), 76, &base[..base.len() - 1]);
        }
    }
}

/// remote_tests.cpp's `delta_head`: a FrameDelta's bytes up to its runs: seq 2, a default state, base 1, the declared
/// length and run count.
fn delta_head(length: u32, runs: u32, base: u64) -> B {
    B::default().head(Kind::FrameDelta).u64(2).state(&State::default()).u64(base).u32(length).u32(runs)
}

fn no_damage() -> Vec<u8> {
    let mut out = Vec::new();
    wire::encode_damage(&None, &wire_caps(), &mut out);
    out
}

fn with_damage(b: B, damage: &[u8]) -> Vec<u8> {
    b.value(damage).0
}

/// Delta bytes taken and refused against its compact list's, every refusal of a run before the delta's list is made.
#[test]
fn delta_bytes() {
    let (base, list, damage) = (list_bytes(&cpp_compact_list()), list_bytes(&moved_list()), no_damage());
    let with = |b: B| with_damage(b, &damage);
    let d = Caps::default();
    let n = list.len() as u32;
    assert!(
        rebuild(&with(delta_head(n, 1, 1).u8(1).str(&list)), &d, 1, &base).is_some_and(|(_, l)| l == list),
        "one literal of the whole list"
    );
    let keep = list.len() - 4;
    assert!(
        rebuild(&with(delta_head(n, 2, 1).u8(1).str(&list[..keep]).u8(1).str(&list[keep..])), &d, 1, &base).is_some_and(|(_, l)| l == list),
        "two literals that follow each other"
    );
    for tag in [2u8, 3, 0x80, 0xff] {
        not_rebuilt(&format!("a run of tag {tag}"), &with(delta_head(n, 1, 1).u8(tag).str(&list)), &d, 1, &base);
    }
    not_rebuilt("a copy of no bytes", &with(delta_head(n, 2, 1).u8(0).u32(0).u32(0).u8(1).str(&list)), &d, 1, &base);
    not_rebuilt("a literal of no bytes", &with(delta_head(n, 2, 1).u8(1).str(b"").u8(1).str(&list)), &d, 1, &base);
    not_rebuilt("runs a byte short of the declared length", &with(delta_head(n + 1, 1, 1).u8(1).str(&list)), &d, 1, &base);
    not_rebuilt("runs a byte past the declared length", &with(delta_head(n - 1, 1, 1).u8(1).str(&list)), &d, 1, &base);
    not_rebuilt("a second run past the declared length", &with(delta_head(n, 2, 1).u8(1).str(&list).u8(0).u32(0).u32(1)), &d, 1, &base);
    not_rebuilt("a run count past the runs given", &with(delta_head(n, 2, 1).u8(1).str(&list)), &d, 1, &base);
    not_rebuilt("a run count past what the bytes hold", &with(delta_head(n, 0xffffffff, 1).u8(1).str(&list)), &d, 1, &base);
    not_rebuilt("no runs", &with(delta_head(n, 0, 1)), &d, 1, &base);
    not_rebuilt("a declared length of 0", &with(delta_head(0, 0, 1)), &d, 1, &base);
    not_rebuilt("a literal longer than the bytes left", &delta_head(n, 1, 1).u8(1).u32(n + 100).raw(&list).value(&damage).0, &d, 1, &base);
    let size = base.len() as u32;
    for (what, offset, length) in [
        ("a copy one byte past the base", size - 3, 4u32),
        ("a copy from the base's end", size, 1),
        ("a copy whose end wraps 32 bits", 0xffffffff, 2),
        ("a copy of 2^32 - 1 bytes", 0, 0xffffffff),
        ("a copy from 2^31", 0x80000000, 1),
    ] {
        let declared = if length > 0x10000 { n } else { n + length };
        not_rebuilt(what, &with(delta_head(declared, 2, 1).u8(0).u32(offset).u32(length).u8(1).str(&list)), &d, 1, &base);
    }
    not_rebuilt(
        "the base's last 4 bytes after the list, which make no list",
        &with(delta_head(n + 4, 2, 1).u8(1).str(&list).u8(0).u32(size - 4).u32(4)),
        &d,
        1,
        &base,
    );
    not_rebuilt("a list that does not decode", &with(delta_head(3, 1, 1).u8(1).str(b"xyz")), &d, 1, &base);
    let mut scratch = Vec::new();
    let mut starts = Vec::new();
    assert!(wire::encode_list_with_starts(&cpp_compact_list(), &wire_caps(), &mut scratch, &mut starts, usize::MAX));
    let one = list_bytes(&vec![twidget_wire::Command::default()])[..6].to_vec();
    not_rebuilt(
        "a copied reference to a payload the list has not made",
        &with(
            delta_head((6 + starts[4] - starts[3]) as u32, 2, 1)
                .u8(1)
                .str(&one)
                .u8(0)
                .u32(starts[3] as u32)
                .u32((starts[4] - starts[3]) as u32),
        ),
        &d,
        1,
        &base,
    );
    not_rebuilt("a damage that does not decode", &delta_head(n, 1, 1).u8(1).str(&list).value(b"xx").0, &d, 1, &base);
    not_rebuilt(
        "a damage longer than the bytes left",
        &delta_head(n, 1, 1).u8(1).str(&list).u32(damage.len() as u32 + 1).raw(&damage).0,
        &d,
        1,
        &base,
    );
    not_rebuilt("a byte after the damage", &[with(delta_head(n, 1, 1).u8(1).str(&list)), vec![0]].concat(), &d, 1, &base);
    let other = with(delta_head(n, 1, 1).u8(1).str(&list));
    not_rebuilt("another kind", &B::default().head(Kind::Frame).raw(&other[2..]).0, &d, 1, &base);
    let mut version = other.clone();
    version[1] = 2;
    not_rebuilt("version 2", &version, &d, 1, &base);
}

/// The declared length, the run count, the copy bound, the frame and message caps and the base cap, each taken at its
/// bound and refused one past it.
#[test]
fn delta_bounds() {
    let (base, list, damage) = (list_bytes(&cpp_compact_list()), list_bytes(&moved_list()), no_damage());
    let delta = FrameDelta { seq: 2, state: State::default(), base: 1, runs: ends_alike(&base, &list), damage: None };
    let bytes = encoded(&delta);
    let whole = encoded(&Frame { seq: 2, state: State::default(), painted: true, list: moved_list(), damage: None });
    let caps_with = |set: &dyn Fn(&mut Caps)| {
        let mut c = Caps::default();
        set(&mut c);
        c
    };
    let at = caps_with(&|c| c.delta_base = base.len().max(list.len()));
    let below = caps_with(&|c| c.delta_base = base.len().max(list.len()) - 1);
    assert!(rebuild(&bytes, &at, 1, &base).is_some(), "a list and a base as long as delta_base are taken");
    not_rebuilt("a list or a base a byte past delta_base", &bytes, &below, 1, &base);
    not_rebuilt("a delta under a delta_base of 0", &bytes, &caps_with(&|c| c.delta_base = 0), 1, &base);
    let tiny = list_bytes(&vec![twidget_wire::Command::default()]);
    let to_tiny = encoded(&FrameDelta {
        seq: 2,
        state: State::default(),
        base: 1,
        runs: vec![DeltaRun { offset: 0, length: 0, literal: tiny.clone() }],
        damage: None,
    });
    assert!(
        rebuild(&to_tiny, &caps_with(&|c| c.delta_base = base.len()), 1, &base).is_some_and(|(_, l)| l == tiny),
        "a short list on a base as long as delta_base"
    );
    not_rebuilt("a short list on a base a byte past delta_base", &to_tiny, &caps_with(&|c| c.delta_base = base.len() - 1), 1, &base);
    let tiny_whole =
        encoded(&Frame { seq: 2, state: State::default(), painted: true, list: vec![twidget_wire::Command::default()], damage: None });
    assert!(to_tiny.len() > tiny_whole.len(), "the short list's one literal makes a delta longer than the Frame it stands for");
    let own_size = caps_with(&|c| {
        c.message = to_tiny.len();
        c.frame = to_tiny.len();
    });
    assert!(
        delta_scan(&to_tiny, &own_size).is_some_and(|(seq, length, _)| seq == 2 && length as usize == tiny.len()),
        "delta_scan takes the short list's delta under message and frame caps of its own size"
    );
    let tiny_message_below = caps_with(&|c| c.message = to_tiny.len() - 1);
    let tiny_frame_below = caps_with(&|c| c.frame = to_tiny.len() - 1);
    not_scanned("delta_scan of a delta a byte past the message cap, its Frame within it", &to_tiny, &tiny_message_below);
    not_scanned("delta_scan of a delta a byte past the frame cap, its Frame within it", &to_tiny, &tiny_frame_below);
    assert!(FrameDelta::decode(&to_tiny, &tiny_message_below).is_none(), "the short list's FrameDelta a byte past the message cap");
    assert!(FrameDelta::decode(&to_tiny, &tiny_frame_below).is_none(), "the short list's FrameDelta a byte past the frame cap");
    assert!(FrameDelta::decode(&bytes, &at).is_some(), "the FrameDelta decodes under the same delta_base");
    assert!(FrameDelta::decode(&bytes, &below).is_none(), "the FrameDelta under a delta_base a byte shorter");
    let mut kept = EARLIER_CPP.to_vec();
    assert!(
        !delta.encode(&caps_with(&|c| c.delta_base = list.len() - 1), &mut kept) && kept == EARLIER_CPP,
        "a delta whose list passes delta_base is not encoded"
    );
    let most = Caps::default().delta_base;
    not_rebuilt(
        "a declared length past delta_base",
        &with_damage(delta_head(most as u32 + 1, 1, 1).u8(1).str(&vec![b'x'; most + 1]), &damage),
        &Caps::default(),
        1,
        &base,
    );
    not_rebuilt(
        "a declared length of 2^32 - 1",
        &with_damage(delta_head(0xffffffff, 1, 1).u8(1).str(&list), &damage),
        &Caps::default(),
        1,
        &base,
    );

    let frame_at = caps_with(&|c| c.frame = whole.len());
    let frame_below = caps_with(&|c| c.frame = whole.len() - 1);
    let message_at = caps_with(&|c| c.message = whole.len().max(bytes.len()));
    let message_below = caps_with(&|c| c.message = whole.len().max(bytes.len()) - 1);
    assert!(whole.len() > bytes.len(), "the delta is shorter than the Frame it stands for");
    assert!(rebuild(&bytes, &frame_at, 1, &base).is_some(), "a delta whose Frame is as long as the frame cap");
    not_rebuilt("a delta whose Frame is a byte past the frame cap", &bytes, &frame_below, 1, &base);
    assert!(rebuild(&bytes, &message_at, 1, &base).is_some(), "a delta whose Frame is as long as the message cap");
    not_rebuilt("a delta whose Frame is a byte past the message cap", &bytes, &message_below, 1, &base);
    assert!(
        FrameDelta::decode(&bytes, &frame_at).is_some() && FrameDelta::decode(&bytes, &message_at).is_some(),
        "the FrameDelta decodes under the same caps"
    );
    assert!(
        FrameDelta::decode(&bytes, &frame_below).is_none() && FrameDelta::decode(&bytes, &message_below).is_none(),
        "refused a byte past them"
    );
    kept = EARLIER_CPP.to_vec();
    assert!(!delta.encode(&frame_below, &mut kept) && kept == EARLIER_CPP, "a delta whose Frame passes the frame cap is not encoded");
    not_rebuilt("a delta itself a byte past the frame cap", &bytes, &caps_with(&|c| c.frame = bytes.len() - 1), 1, &base);
    let rects = Some(vec![Rect::new(1.0, 2.0, 3.0, 4.0); 64]);
    let damaged = FrameDelta { seq: 2, state: State::default(), base: 1, runs: ends_alike(&base, &list), damage: rects.clone() };
    let damaged_bytes = encoded(&damaged);
    let damaged_whole = encoded(&Frame { seq: 2, state: State::default(), painted: true, list: moved_list(), damage: rects });
    let damaged_at = caps_with(&|c| c.frame = damaged_whole.len());
    let damaged_below = caps_with(&|c| c.frame = damaged_whole.len() - 1);
    assert!(
        damaged_bytes.len() <= damaged_below.frame && damaged.runs.iter().any(|r| !r.literal.is_empty()),
        "a delta with a literal and 64 damage rects is shorter than its Frame"
    );
    assert!(
        rebuild(&damaged_bytes, &damaged_at, 1, &base).is_some_and(|(f, l)| l == list && f.damage.is_some_and(|d| d.len() == 64)),
        "a delta with damage whose Frame is as long as the frame cap"
    );
    not_rebuilt("a delta with damage whose Frame is a byte past the frame cap", &damaged_bytes, &damaged_below, 1, &base);
    assert!(
        FrameDelta::decode(&damaged_bytes, &damaged_at).is_some_and(|d| d.damage.is_some_and(|d| d.len() == 64)),
        "decodes under the same cap"
    );
    assert!(FrameDelta::decode(&damaged_bytes, &damaged_below).is_none(), "refused a byte past it");
    assert!(
        [&frame_at, &message_at]
            .iter()
            .all(|caps| delta_scan(&bytes, caps).is_some_and(|(seq, length, _)| seq == 2 && length as usize == list.len())),
        "delta_scan takes the delta under the same caps"
    );
    not_scanned("delta_scan of the delta whose Frame is a byte past the frame cap", &bytes, &frame_below);
    not_scanned("delta_scan of the delta whose Frame is a byte past the message cap", &bytes, &message_below);
    let mut rects_value = Vec::new();
    assert!(wire::encode_damage(&damaged.damage, &wire::Caps::default(), &mut rects_value), "64 damage rects encode");
    assert!(
        delta_scan(&damaged_bytes, &damaged_at).is_some_and(|(seq, length, viewed)| seq == 2
            && length as usize == list.len()
            && viewed == rects_value.as_slice()
            && viewed.as_ptr() as usize == damaged_bytes.as_ptr() as usize + damaged_bytes.len() - rects_value.len()),
        "delta_scan takes a delta with a literal and 64 damage rects, making neither, and views the damage value where it lies"
    );
    not_scanned("delta_scan of the delta with damage whose Frame is a byte past the frame cap", &damaged_bytes, &damaged_below);

    let all = cpp_compact_list();
    let short_list = list_bytes(&all[..3].to_vec());
    let commands = caps_with(&|c| c.values.commands = 3);
    let pieces = |n: usize| {
        let runs = (0..n)
            .map(|i| {
                let (from, to) = (short_list.len() * i / n, short_list.len() * (i + 1) / n);
                DeltaRun { offset: 0, length: 0, literal: short_list[from..to].to_vec() }
            })
            .collect();
        FrameDelta { seq: 2, state: State::default(), base: 1, runs, damage: None }
    };
    assert!(rebuild(&encoded(&pieces(4)), &commands, 1, &base).is_some_and(|(_, l)| l == short_list), "a command cap of 3 takes 4 runs");
    not_rebuilt("5 runs under a command cap of 3", &encoded(&pieces(5)), &commands, 1, &base);
    assert!(FrameDelta::decode(&encoded(&pieces(5)), &commands).is_none(), "the FrameDelta of 5 runs under a command cap of 3");
    kept = EARLIER_CPP.to_vec();
    assert!(!pieces(5).encode(&commands, &mut kept) && kept == EARLIER_CPP, "encoding 5 runs under a command cap of 3 is refused");

    let painted = |n: usize| {
        (0..n)
            .map(|_| twidget_wire::Command {
                rect: Rect::new(1.0, 2.0, 3.0, 4.0),
                color: twidget_wire::Color::rgba(9, 8, 7, 255),
                ..Default::default()
            })
            .collect::<Vec<_>>()
    };
    let (four, more) = (painted(4), painted(5));
    let (repeated_base, longer) = (list_bytes(&four), list_bytes(&more));
    let command = ((repeated_base.len() - 6) / 4) as u32;
    let copied = |n: usize| {
        let mut b = delta_head(6 + n as u32 * command, 1 + n as u32, 1).u8(1).str(&list_bytes(if n == 4 { &four } else { &more })[..6]);
        for _ in 0..n {
            b = b.u8(0).u32(6).u32(command);
        }
        b.value(&damage).0
    };
    assert!(
        rebuild(&copied(4), &Caps::default(), 1, &repeated_base).is_some_and(|(_, l)| l == repeated_base),
        "one command copied 4 times"
    );
    not_rebuilt("one command copied 5 times, past what the base holds", &copied(5), &Caps::default(), 1, &repeated_base);
    assert!(
        rebuild(&with_damage(delta_head(longer.len() as u32, 1, 1).u8(1).str(&longer), &damage), &Caps::default(), 1, &repeated_base)
            .is_some_and(|(_, l)| l == longer),
        "the same list as one literal"
    );
}
