//! The hosted wire against tdesktop's (`oracle/hosted_wire_oracle`, tgw_hosted_wire.h and the helper work record
//! compiled from a tdesktop checkout): every sample control and its cuts, mutated samples, generated controls and
//! bodies, the limits on their boundaries, decoded by both; every encoder on generated arguments; the constants and
//! the embedding caps. Skipped without `TWIDGET_ORACLE_DIR` or without tdesktop's sources.

mod common;

use common::*;
use twidget_helper::wire::{self, Kind};
use twidget_helper::{HelperWorkKind, HelperWorkRecord, decode_helper_work, encode_helper_work};
use twidget_testkit::Answer;

fn put_record(out: &mut Out, record: &HelperWorkRecord) {
    out.u8(record.kind as u8).i64(record.started).i64(record.finished).i64(record.waited);
    out.u64(record.seq).u32(record.waits).u32(record.frame_bytes);
}

/// What the oracle's `Decode` answers.
fn decoded(message: &[u8]) -> Answer {
    let mut out = Out::default();
    out.u8(wire::kind_of(message).map_or(0, |k| k as u8));
    out.flag(wire::is_control(message));
    for kind in Kind::ALL {
        out.flag(wire::is_exactly(message, kind));
    }
    if message.len() >= 2 {
        let named = Kind::from_u8(message[1]);
        out.flag(named.is_some_and(wire::to_helper));
        out.flag(named.is_some_and(wire::from_helper));
        out.flag(named.is_some_and(wire::is_tap_control));
    }
    if message.len() >= 3 {
        out.flag(wire::is_mirrored_way(message[2]));
    }
    let open = wire::decode_open(message);
    out.flag(open.is_some()).f64(open.unwrap_or(0.0));
    let press = wire::decode_press(message);
    out.flag(press.is_some()).u64(press.unwrap_or(0));
    let clicked = wire::decode_clicked(message);
    out.flag(clicked.is_some()).u64(clicked.unwrap_or(0));
    let stopped = wire::decode_stopped(message);
    out.flag(stopped.is_some()).str(stopped.unwrap_or_default());
    let cursor = wire::decode_cursor(message);
    out.flag(cursor.is_some());
    out.u64(cursor.map_or(0, |c| c.pointers)).str(cursor.map_or(&[][..], |c| c.cursor));
    let mirrored = wire::decode_mirrored(message);
    out.flag(mirrored.is_some());
    out.u8(mirrored.map_or(0, |m| m.way)).str(mirrored.map_or(&[][..], |m| m.text));
    let work = decode_helper_work(message);
    out.flag(work.is_some());
    put_record(&mut out, &work.unwrap_or_default());
    (1, out.0)
}

fn decode_case(c: &mut Comparison, message: &[u8], what: String) {
    c.case(&framed(b'd', message), decoded(message), what);
}

/// An encoder's call: its record for the oracle and the Rust answer.
enum Encode {
    Open(f64),
    Press(u8, u64),
    Bare(u8),
    Stopped(Vec<u8>),
    Cursor(u64, Vec<u8>),
    Mirrored(u8, Vec<u8>),
    Work(HelperWorkRecord),
}

fn encode_case(c: &mut Comparison, call: &Encode, what: String) {
    let mut record = Out::default();
    let answer = match call {
        Encode::Open(dpr) => {
            record.u8(0).f64(*dpr);
            (1, wire::encode_open(*dpr))
        }
        Encode::Press(which, press) => {
            record.u8(*which).u64(*press);
            let bytes = match which {
                1 => wire::encode_menu_deliver(*press),
                2 => wire::encode_finger_hold(*press),
                3 => wire::encode_finger_release(*press),
                _ => wire::encode_clicked(*press),
            };
            (1, bytes)
        }
        Encode::Bare(which) => {
            record.u8(*which);
            let bytes = match which {
                5 => wire::encode_mirror(),
                6 => wire::encode_mirror_work(),
                7 => wire::encode_canvas_refill(),
                8 => wire::encode_canvas_refused(),
                9 => wire::encode_content(),
                _ => wire::encode_loaded(),
            };
            (1, bytes)
        }
        Encode::Stopped(reason) => {
            record.u8(11).str(reason);
            (1, wire::encode_stopped(reason))
        }
        Encode::Cursor(pointers, cursor) => {
            record.u8(12).u64(*pointers).str(cursor);
            (1, wire::encode_cursor(*pointers, cursor))
        }
        Encode::Mirrored(way, text) => {
            record.u8(13).u8(*way).str(text);
            match wire::encode_mirrored(*way, text) {
                Some(bytes) => (1, bytes),
                None => (0, Vec::new()),
            }
        }
        Encode::Work(work) => {
            record.u8(14);
            put_record(&mut record, work);
            (1, encode_helper_work(work))
        }
    };
    c.case(&framed(b'e', &record.0), answer, what);
}

const EDGE_BITS: [u64; 14] = [
    0,
    0x8000_0000_0000_0000,
    0x3ff0_0000_0000_0000,
    0x7ff0_0000_0000_0000,
    0xfff0_0000_0000_0000,
    0x7ff8_0000_0000_0000,
    0xfff8_0000_0000_0000,
    0x7ff0_0000_0000_0001,
    0x7fff_ffff_ffff_ffff,
    0x0000_0000_0000_0001,
    0x000f_ffff_ffff_ffff,
    0x7fef_ffff_ffff_ffff,
    0x3fd0_0000_0000_0000,
    0xffff_ffff_ffff_ffff,
];

fn any_number(random: &mut Lcg) -> u64 {
    match random.below(4) {
        0 => EDGE_BITS[random.below(EDGE_BITS.len())],
        1 => u64::from(random.next()),
        2 => (u64::from(random.next()) << 32) | u64::from(random.next()),
        _ => random.below(100) as u64,
    }
}

fn any_text(random: &mut Lcg, longest: usize) -> Vec<u8> {
    let n = match random.below(5) {
        0 => 0,
        1 => random.below(8),
        2 => longest.saturating_sub(2) + random.below(5),
        _ => random.below(longest + 8),
    };
    (0..n).map(|_| if random.chance(3) { random.next() as u8 } else { b'a' + random.below(26) as u8 }).collect()
}

fn any_record(random: &mut Lcg) -> HelperWorkRecord {
    let kinds = [
        HelperWorkKind::Message,
        HelperWorkKind::Theme,
        HelperWorkKind::Tick,
        HelperWorkKind::Paint,
        HelperWorkKind::Platform,
        HelperWorkKind::Control,
    ];
    let started = match random.below(4) {
        0 => -(random.below(1000) as i64),
        1 => any_number(random) as i64,
        _ => 1_000_000 + random.below(1 << 30) as i64,
    };
    let finished = match random.below(5) {
        0 => started.wrapping_sub(random.below(3) as i64),
        1 => started.wrapping_add(3_600_000_000 + random.below(3) as i64 - 1),
        2 => any_number(random) as i64,
        _ => started.wrapping_add(random.below(100_000) as i64),
    };
    let interval = finished.wrapping_sub(started);
    let waited = match random.below(4) {
        0 => interval.wrapping_add(random.below(3) as i64 - 1),
        1 => -(random.below(3) as i64),
        2 => any_number(random) as i64,
        _ => {
            if interval > 0 {
                random.below(interval.min(1 << 30) as usize) as i64
            } else {
                0
            }
        }
    };
    HelperWorkRecord {
        started,
        finished,
        waited,
        seq: any_number(random),
        waits: random.next(),
        frame_bytes: random.next(),
        kind: kinds[random.below(kinds.len())],
    }
}

fn samples() -> Vec<(String, Vec<u8>)> {
    let mut out = vec![
        ("open".to_string(), wire::encode_open(2.0)),
        ("open nan".to_string(), wire::encode_open(f64::NAN)),
        ("menu".to_string(), wire::encode_menu_deliver(3)),
        ("hold".to_string(), wire::encode_finger_hold(u64::MAX)),
        ("release".to_string(), wire::encode_finger_release(0)),
        ("clicked".to_string(), wire::encode_clicked(77)),
        ("mirror".to_string(), wire::encode_mirror()),
        ("mirror work".to_string(), wire::encode_mirror_work()),
        ("refill".to_string(), wire::encode_canvas_refill()),
        ("refused".to_string(), wire::encode_canvas_refused()),
        ("content".to_string(), wire::encode_content()),
        ("loaded".to_string(), wire::encode_loaded()),
        ("stopped".to_string(), wire::encode_stopped(b"the widget stopped: a layout past its budget")),
        ("stopped empty".to_string(), wire::encode_stopped(b"")),
        ("cursor".to_string(), wire::encode_cursor(12, b"pointer")),
        ("cursor empty".to_string(), wire::encode_cursor(0, b"")),
    ];
    for way in [b'b', b'h', b'r', b't'] {
        out.push((format!("mirrored {}", way as char), wire::encode_mirrored(way, br#"{"type":"ready"}"#).expect("a way")));
    }
    let record =
        HelperWorkRecord { started: 100, finished: 250, waited: 40, seq: 9, waits: 2, frame_bytes: 4096, kind: HelperWorkKind::Paint };
    out.push(("mirrored w".to_string(), wire::encode_mirrored(b'w', &encode_helper_work(&record)).expect("a way")));
    out.push(("work record".to_string(), encode_helper_work(&record)));
    out
}

#[test]
fn samples_cuts_and_mutations_alike() {
    let Some(oracle) = helper_oracle("hosted_wire_oracle") else { return };
    let mut c = Comparison::new();
    for (index, (name, bytes)) in samples().iter().enumerate() {
        for n in 0..=bytes.len() {
            decode_case(&mut c, &bytes[..n], format!("{name} cut to {n}"));
        }
        let mut random = Lcg::new(0x4857_0000 + index as u64);
        for round in 0..1500 {
            let mut changed = bytes.clone();
            let edits = mutate(&mut random, &mut changed);
            decode_case(&mut c, &changed, format!("{name}, round {round}:{edits}"));
        }
    }
    let cases = c.len();
    c.check(&oracle);
    eprintln!("hosted wire: {cases} samples, cuts and mutations alike");
}

/// Controls of every kind byte, bodies of fixed numbers and length-prefixed texts whose lengths agree or not, near
/// each limit.
#[test]
fn generated_controls_alike() {
    let Some(oracle) = helper_oracle("hosted_wire_oracle") else { return };
    let mut random = Lcg::new(0x4857_1001);
    let mut c = Comparison::new();
    for i in 0..60000 {
        let mut message = Vec::new();
        message.push(if random.chance(8) { random.next() as u8 } else { wire::TAG });
        let kind = if random.chance(6) { random.next() as u8 } else { Kind::ALL[random.below(Kind::ALL.len())] as u8 };
        message.push(kind);
        match random.below(6) {
            0 => {}
            1 => message.extend_from_slice(&any_number(&mut random).to_le_bytes()),
            2 => {
                if random.chance(2) {
                    message.extend_from_slice(&any_number(&mut random).to_le_bytes());
                }
                let longest = if random.chance(2) { wire::MAX_STOP_REASON_BYTES } else { wire::MAX_CURSOR_BYTES };
                let text = any_text(&mut random, longest);
                let length =
                    if random.chance(5) { random.next() } else { (text.len() as u32).wrapping_add(random.below(3) as u32).wrapping_sub(1) };
                message.extend_from_slice(&if random.chance(2) { text.len() as u32 } else { length }.to_le_bytes());
                message.extend_from_slice(&text);
            }
            3 => {
                message.push([b'b', b'h', b'r', b't', b'w', random.next() as u8][random.below(6)]);
                let text = if random.chance(2) { encode_helper_work(&any_record(&mut random)) } else { any_text(&mut random, 64) };
                let length = if random.chance(4) { (text.len() as u32).wrapping_add(1) } else { text.len() as u32 };
                message.extend_from_slice(&length.to_le_bytes());
                message.extend_from_slice(&text);
            }
            4 => message = encode_helper_work(&any_record(&mut random)),
            _ => {
                let n = random.below(24);
                message.extend((0..n).map(|_| random.next() as u8));
            }
        }
        decode_case(&mut c, &message, format!("generated {i}"));
    }
    let cases = c.len();
    c.check(&oracle);
    eprintln!("hosted wire: {cases} generated controls alike");
}

#[test]
fn encoders_and_constants_alike() {
    let Some(oracle) = helper_oracle("hosted_wire_oracle") else { return };
    let mut random = Lcg::new(0x4857_2002);
    let mut c = Comparison::new();
    for i in 0..30000 {
        let call = match random.below(7) {
            0 => Encode::Open(f64::from_bits(any_number(&mut random))),
            1 => Encode::Press(1 + random.below(4) as u8, any_number(&mut random)),
            2 => Encode::Bare(5 + random.below(6) as u8),
            3 => Encode::Stopped(any_text(&mut random, wire::MAX_STOP_REASON_BYTES)),
            4 => Encode::Cursor(any_number(&mut random), any_text(&mut random, wire::MAX_CURSOR_BYTES)),
            5 => Encode::Mirrored(random.next() as u8, any_text(&mut random, 40)),
            _ => Encode::Work(any_record(&mut random)),
        };
        encode_case(&mut c, &call, format!("encoder {i}"));
    }
    for way in 0..=255u8 {
        encode_case(&mut c, &Encode::Mirrored(way, b"x".to_vec()), format!("way {way}"));
    }
    let caps = wire::embedding_caps();
    let v = caps.values;
    let mut constants = Out::default();
    for value in [
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
        constants.u64(value as u64);
    }
    for value in [
        wire::HEADER_BYTES,
        wire::NUMBER_BYTES,
        wire::LENGTH_BYTES,
        wire::MAX_MESSAGE_BYTES,
        wire::MAX_LIST_COMMANDS,
        wire::MAX_FRAME_BYTES,
        wire::MAX_CANVAS_RUN_OPS,
        wire::MAX_WIDTHS_TEXTS,
        wire::MAX_WIDTHS_TEXT_BYTES,
        wire::MAX_DELTA_BASE_BYTES,
        wire::MAX_STOP_REASON_BYTES,
        wire::MAX_CURSOR_BYTES,
        wire::MIRRORED_OVERHEAD_BYTES,
        wire::MAX_MIRRORED_TEXT_BYTES,
        twidget_helper::HELPER_WORK_RECORD_BYTES,
    ] {
        constants.u64(value as u64);
    }
    constants.u8(wire::TAG);
    for way in [wire::MIRRORED_BUILT, wire::MIRRORED_HOST, wire::MIRRORED_RENDERER, wire::MIRRORED_THEME, wire::MIRRORED_WORK] {
        constants.u8(way);
    }
    constants.i64(twidget_helper::MAX_HELPER_INTERVAL);
    c.case(&framed(b'k', &[]), (1, constants.0), "constants and caps");
    let cases = c.len();
    c.check(&oracle);
    eprintln!("hosted wire: {cases} encoder calls and the constants alike");
}

/// Each length limit on its boundary and past it: a reason of 4096 bytes and 4097, a cursor of 256 and 257, a
/// mirrored text that fills a 32 MiB message and one byte more, a message of 32 MiB and one byte more.
#[test]
fn limits_alike() {
    let Some(oracle) = helper_oracle("hosted_wire_oracle") else { return };
    let mut c = Comparison::new();
    let body = |kind: Kind, number: Option<u64>, text_len: usize, length: u32| {
        let mut m = vec![wire::TAG, kind as u8];
        if let Some(n) = number {
            m.extend_from_slice(&n.to_le_bytes());
        }
        m.extend_from_slice(&length.to_le_bytes());
        m.extend(std::iter::repeat_n(b'q', text_len));
        m
    };
    for n in [4095usize, 4096, 4097] {
        decode_case(&mut c, &body(Kind::Stopped, None, n, n as u32), format!("reason of {n}"));
        encode_case(&mut c, &Encode::Stopped(vec![b'r'; n]), format!("encoded reason of {n}"));
    }
    for n in [255usize, 256, 257] {
        decode_case(&mut c, &body(Kind::Cursor, Some(4), n, n as u32), format!("cursor of {n}"));
        encode_case(&mut c, &Encode::Cursor(4, vec![b'c'; n]), format!("encoded cursor of {n}"));
    }
    let mut cases = c.len();
    c.check(&oracle);
    // One 32 MiB case a run, so that at most one such message and its answers are held at a time.
    for n in [wire::MAX_MIRRORED_TEXT_BYTES, wire::MAX_MIRRORED_TEXT_BYTES + 1] {
        let mut c = Comparison::new();
        encode_case(&mut c, &Encode::Mirrored(b'r', vec![b'm'; n]), format!("mirrored text of {n}"));
        c.check(&oracle);
        let mut c = Comparison::new();
        let mut m = vec![wire::TAG, Kind::Mirrored as u8, b'r'];
        m.extend_from_slice(&(n as u32).to_le_bytes());
        m.resize(m.len() + n, b'm');
        decode_case(&mut c, &m, format!("mirrored message of {}", m.len()));
        drop(m);
        c.check(&oracle);
        cases += 2;
    }
    eprintln!("hosted wire: {cases} limit cases alike");
}
