//! tests/remote_tests.cpp's message checks, ported: the documented bytes, the kinds and their ranges,
//! pixels answered exactly as asked, the carried values' caps, lengths of carried values, an encoder's
//! room under the message cap, and cuts and random changes of every sample. Its platform end's session is
//! the main process's and stays with the C++.

mod common;

use common::*;
use twidget_wire as wire;

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

/// The kinds: 25 of them in four ranges, kind_of naming exactly those of version 1 from the header alone.
#[test]
fn kinds() {
    let ranges = [(1u8, 8u8), (32, 35), (64, 72), (96, 99)];
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
    assert_eq!(known, 25);
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
