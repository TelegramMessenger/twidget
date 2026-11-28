//! The Rust messages against the C++ ones (`oracle/remote_oracle`, built from core/src/remote.cpp and
//! wire.cpp): every sample and its cuts, mutated samples, generated messages and pixel answers, under the
//! default caps and under tight ones, decoded by both and encoded again by both; widths requests and answers, the texts
//! one request holds, frames with their list placed and viewed, and frame deltas rebuilt and scanned. Skipped without
//! `TWIDGET_ORACLE_DIR`; `tools/gate.py` sets it.

mod common;

use common::*;
use twidget_testkit::{Comparison, Oracle, framed};

fn tight_caps() -> Vec<Caps> {
    let mut out = Vec::new();
    for (i, set) in [
        (|c: &mut Caps| c.message = 40) as fn(&mut Caps),
        |c| c.text = 3,
        |c| c.url = 4,
        |c| c.messages = 1,
        |c| c.values.commands = 1,
        |c| c.values.canvas_ops = 2,
        |c| c.values.pixels = 8,
        |c| c.values.text = 2,
    ]
    .iter()
    .enumerate()
    {
        let mut caps = Caps::default();
        set(&mut caps);
        out.push(caps);
        let _ = i;
    }
    out
}

fn same(c: &mut Comparison, caps: &Caps, message: &[u8], what: String) {
    c.case(&framed(b'm', message), answer(message, caps, caps), what);
}

#[test]
fn samples_and_mutations_alike() {
    let Some(oracle) = Oracle::find("remote_oracle") else { return };
    let mut c = Comparison::new();
    let mut all_caps = vec![Caps::default()];
    all_caps.extend(tight_caps());
    for (k, caps) in all_caps.iter().enumerate() {
        c.setting(&caps_record(caps));
        for (index, (name, bytes)) in samples().iter().enumerate() {
            for n in 0..=bytes.len() {
                same(&mut c, caps, &bytes[..n], format!("{name} cut to {n}, caps {k}"));
            }
            let mut random = Lcg::new(0x7e31_0000 + (k as u64) * 0x100 + index as u64);
            for round in 0..(if k == 0 { 1500 } else { 200 }) {
                let mut changed = bytes.clone();
                let edits = mutate(&mut random, &mut changed);
                same(&mut c, caps, &changed, format!("{name}, caps {k}, round {round}:{edits}"));
            }
        }
    }
    let tally = c.check(&oracle);
    eprintln!("samples and mutations: {} refused, {} taken by both, {} encodes refused by both", tally[0], tally[1], tally[2]);
    assert!(tally[0] > 0 && tally[1] > 0);
}

#[test]
fn generated_messages_alike() {
    let Some(oracle) = Oracle::find("remote_oracle") else { return };
    let mut random = Lcg::new(0x7e32_0001);
    let mut c = Comparison::new();
    c.setting(&caps_record(&Caps::default()));
    let mut messages = Vec::new();
    for i in 0..20000 {
        let bytes = any_message(&mut random);
        same(&mut c, &Caps::default(), &bytes, format!("generated {i}"));
        let mut changed = bytes.clone();
        let edits = mutate(&mut random, &mut changed);
        same(&mut c, &Caps::default(), &changed, format!("generated {i}:{edits}"));
        messages.push(bytes);
    }
    for (k, caps) in tight_caps().iter().enumerate() {
        c.setting(&caps_record(caps));
        for (i, bytes) in messages.iter().take(3000).enumerate() {
            c.case(&framed(b'M', bytes), answer(bytes, &Caps::default(), caps), format!("generated {i} encoded under caps {k}"));
        }
    }
    for byte in 0..=255u8 {
        for message in [vec![byte, 1], vec![byte, 1, 0], vec![byte, 2], vec![byte]] {
            let rust = kind_of(&message).map_or((0, Vec::new()), |k| (1, vec![k as u8]));
            c.case(&framed(b'k', &message), rust, format!("kind_of {}", hex(&message)));
        }
    }
    let tally = c.check(&oracle);
    eprintln!("generated: {} refused, {} taken, {} encodes refused", tally[0], tally[1], tally[2]);
}

#[test]
fn pixel_answers_alike() {
    let Some(oracle) = Oracle::find("remote_oracle") else { return };
    let mut random = Lcg::new(0x7e33_0001);
    let mut c = Comparison::new();
    let mut tight = Caps::default();
    tight.values.pixels = 24;
    tight.message = 40;
    for caps in [Caps::default(), tight] {
        c.setting(&caps_record(&caps));
        for i in 0..6000 {
            let (w, h) = match random.below(4) {
                0 => (random.below(5) as i32, random.below(5) as i32),
                1 => {
                    ([-1, 0, 1, 8192, 8193, 4096, 4097, i32::MIN][random.below(8)], [-1, 0, 1, 2048, 2049, 4096, i32::MAX][random.below(7)])
                }
                _ => (random.below(8) as i32, random.below(8) as i32),
            };
            let asked = PixelsRequest { id: 1, x: 0, y: 0, w, h };
            let mut message = vec![0x23, 1];
            let ok = !random.chance(5);
            message.push(u8::from(ok));
            if ok {
                let n = if pixel_ok(w, h) && random.chance(2) { (w * h * 4) as u32 } else { random.below(80) as u32 };
                message.extend_from_slice(&n.to_le_bytes());
                let have = if random.chance(4) { n as usize + random.below(3) } else { n as usize };
                message.extend((0..have.min(4096)).map(|k| k as u8));
            }
            let mut changed = message.clone();
            if random.chance(3) {
                mutate(&mut random, &mut changed);
            }
            for (tag, decode_caps) in [(b'x', caps), (b'X', Caps::default())] {
                c.case(
                    &pixels_record(tag, &asked, &changed),
                    answer_pixels(&changed, &asked, &decode_caps, &caps),
                    format!("pixels {i} for {w} × {h}"),
                );
            }
        }
    }
    let tally = c.check(&oracle);
    eprintln!("pixel answers: {} refused, {} taken, {} encodes refused", tally[0], tally[1], tally[2]);
    assert!(tally[0] > 0 && tally[1] > 0);
}

/// The widths caps and the frame delta caps, tight, one at a time and together.
fn freeze_caps() -> Vec<Caps> {
    let sets: [fn(&mut Caps); 8] = [
        |c| c.widths = 2,
        |c| c.widths_text = 7,
        |c| c.text = 3,
        |c| c.message = 60,
        |c| c.frame = 120,
        |c| c.delta_base = 64,
        |c| c.values.commands = 3,
        |c| {
            c.message = 4096;
            c.text = 64;
            c.widths = 4;
            c.widths_text = 32;
            c.frame = 4096;
            c.delta_base = 64;
        },
    ];
    let mut out = vec![Caps::default()];
    for set in sets {
        let mut caps = Caps::default();
        set(&mut caps);
        out.push(caps);
    }
    out
}

fn any_texts(random: &mut Lcg, most: usize) -> Vec<TextInFont> {
    (0..random.below(most + 1))
        .map(|_| {
            let n = if random.chance(8) { 30 + random.below(40) } else { random.below(10) };
            TextInFont { font: random.below(5) as i32 - 1, text: (0..n).map(|k| b'a' + (k % 26) as u8).collect() }
        })
        .collect()
}

/// Widths requests and the answers to them, their cuts and mutations, and widths_in_request from every text, under the
/// default and tight caps, through remote.cpp and the port.
#[test]
fn widths_alike() {
    let Some(oracle) = Oracle::find("remote_oracle") else { return };
    let mut random = Lcg::new(0x7e34_0001);
    let mut c = Comparison::new();
    for (k, caps) in freeze_caps().iter().enumerate() {
        c.setting(&caps_record(caps));
        for i in 0..1500 {
            let asked = WidthsRequest { texts: any_texts(&mut random, 8) };
            let mut request = Vec::new();
            WidthsRequest::encode(&asked, &Caps::default(), &mut request);
            same(&mut c, caps, &request, format!("widths request {i}, caps {k}"));
            let mut changed = request.clone();
            let edits = mutate(&mut random, &mut changed);
            same(&mut c, caps, &changed, format!("widths request {i}, caps {k}:{edits}"));
            let n = if random.chance(4) { random.below(10) } else { asked.texts.len() };
            let answer = WidthsAnswer { widths: (0..n).map(|_| f32::from_bits(random.next())).collect() };
            let mut bytes = Vec::new();
            answer.encode(&Caps::default(), &mut bytes);
            if random.chance(3) {
                mutate(&mut random, &mut bytes);
            }
            for (tag, decode_caps) in [(b'y', *caps), (b'Y', Caps::default())] {
                c.case(
                    &widths_record(tag, &asked, &bytes),
                    answer_widths(&bytes, &asked, &decode_caps, caps),
                    format!("widths answer {i}, caps {k}"),
                );
            }
            for from in 0..=asked.texts.len() as u32 + 1 {
                c.case(
                    &widths_in_request_record(&asked.texts, from),
                    answer_widths_in_request(&asked.texts, from, caps),
                    format!("widths_in_request {i} from {from}, caps {k}"),
                );
            }
        }
        for (name, value, asked) in widths_answers() {
            let mut bytes = Vec::new();
            value.encode(&Caps::default(), &mut bytes);
            for n in 0..=bytes.len() {
                c.case(
                    &widths_record(b'y', &asked, &bytes[..n]),
                    answer_widths(&bytes[..n], &asked, caps, caps),
                    format!("{name} cut to {n}, caps {k}"),
                );
            }
        }
    }
    let tally = c.check(&oracle);
    eprintln!(
        "widths: {} refused, {} taken, {} encodes refused, {} answers without their request",
        tally[0],
        tally[1],
        tally[2],
        tally.get(3).copied().unwrap_or(0)
    );
    assert!(tally[0] > 0 && tally[1] > 0);
}

/// A list of commands drawn from a few, so that one list shares runs with the next.
fn drawn_list(random: &mut Lcg, palette: &[twidget_wire::Command]) -> twidget_wire::DisplayList {
    let n = random.below(12);
    (0..n)
        .map(|_| {
            let mut c = palette[random.below(palette.len())].clone();
            if random.chance(5) {
                c.rect.x = random.below(100) as f32;
            }
            c
        })
        .collect()
}

/// Frames with their list's place, frames with their list's view, delta_ids, and frame deltas rebuilt on their base:
/// written deltas, generated ones between lists that share runs, their cuts and mutations, on the right base, another
/// base and a cut one, under the default and tight caps.
#[test]
fn frame_deltas_alike() {
    let Some(oracle) = Oracle::find("remote_oracle") else { return };
    let mut random = Lcg::new(0x7e35_0001);
    let mut palette = cpp_compact_list();
    palette.extend(compact_list());
    let mut c = Comparison::new();
    for (k, caps) in freeze_caps().iter().enumerate() {
        c.setting(&caps_record(caps));
        let mut previous = cpp_compact_list();
        for i in 0..1200 {
            let list = drawn_list(&mut random, &palette);
            let damage = if random.chance(2) { Some(vec![Rect::new(1.0, 2.0, 3.0, 4.0); random.below(3)]) } else { None };
            let frame = Frame {
                seq: i as u64 + 2,
                state: any_state_for(&mut random),
                painted: !random.chance(6),
                list: list.clone(),
                damage: damage.clone(),
            };
            let frame = if frame.painted { frame } else { Frame { list: Vec::new(), damage: None, ..frame } };
            let mut bytes = Vec::new();
            frame.encode(&Caps::default(), &mut bytes);
            let n = bytes.len() as u64;
            for most in [u64::MAX, n, n.saturating_sub(40), 6, 0] {
                c.case(
                    &placed_record(most, &bytes),
                    answer_placed(&bytes, caps, most),
                    format!("frame {i} placed within {most}, caps {k}"),
                );
            }
            c.case(&framed(b'v', &bytes), answer_viewed(&bytes, caps), format!("frame {i} viewed, caps {k}"));
            let (base, now) = (list_bytes(&previous), list_bytes(&list));
            let runs = if random.chance(3) { ends_alike(&base, &now) } else { mixed_runs(&mut random, &base, &now) };
            let delta = FrameDelta { seq: i as u64 + 2, state: frame.state, base: i as u64 + 1, runs, damage };
            let mut message = Vec::new();
            if !delta.encode(&Caps::default(), &mut message) {
                previous = list;
                continue;
            }
            same(&mut c, caps, &message, format!("delta {i}, caps {k}"));
            c.case(&framed(b'i', &message), answer_ids(&message), format!("delta {i} ids"));
            for (seq, on) in [(i as u64 + 1, &base[..]), (i as u64, &base[..]), (i as u64 + 1, &base[..base.len() / 2])] {
                c.case(
                    &rebuilt_record(seq, on, &message),
                    answer_rebuilt(&message, caps, seq, on),
                    format!("delta {i} rebuilt on {seq} of {} bytes, caps {k}", on.len()),
                );
            }
            let mut changed = message.clone();
            let edits = mutate(&mut random, &mut changed);
            c.case(
                &rebuilt_record(i as u64 + 1, &base, &changed),
                answer_rebuilt(&changed, caps, i as u64 + 1, &base),
                format!("delta {i} changed, caps {k}:{edits}"),
            );
            same(&mut c, caps, &changed, format!("delta {i} changed and decoded, caps {k}:{edits}"));
            c.case(&framed(b'i', &changed), answer_ids(&changed), format!("delta {i} changed ids:{edits}"));
            if i % 50 == 0 {
                for n in 0..message.len() {
                    c.case(
                        &rebuilt_record(i as u64 + 1, &base, &message[..n]),
                        answer_rebuilt(&message[..n], caps, i as u64 + 1, &base),
                        format!("delta {i} cut to {n}, caps {k}"),
                    );
                }
            }
            previous = list;
        }
    }
    let tally = c.check(&oracle);
    eprintln!("frame deltas: {} refused, {} taken, {} encodes refused", tally[0], tally[1], tally[2]);
    assert!(tally[0] > 0 && tally[1] > 0);
}

fn any_state_for(random: &mut Lcg) -> State {
    if random.chance(4) { special_state() } else { ordinary_state() }
}

/// Runs copied from anywhere in the base and literals, which may rebuild the list or not.
fn mixed_runs(random: &mut Lcg, base: &[u8], list: &[u8]) -> Vec<DeltaRun> {
    let mut runs = Vec::new();
    let mut at = 0;
    while at < list.len() && runs.len() < 6 {
        let n = (1 + random.below(12)).min(list.len() - at);
        if !base.is_empty() && random.chance(2) {
            let offset = random.below(base.len());
            runs.push(DeltaRun { offset: offset as u32, length: n.min(base.len() - offset) as u32, literal: Vec::new() });
        } else {
            runs.push(DeltaRun { offset: 0, length: 0, literal: list[at..at + n].to_vec() });
        }
        at += n;
    }
    if at < list.len() {
        runs.push(DeltaRun { offset: 0, length: 0, literal: list[at..].to_vec() });
    }
    if runs.is_empty() {
        runs.push(DeltaRun { offset: 0, length: 0, literal: vec![1] });
    }
    runs
}

/// delta_scan as `FrameDelta::decode` sees the same message: whatever decode takes the scan takes, with the same seq and
/// the length of its runs, and what the scan takes decode takes unless the damage it views does not decode, which is the
/// caller's to check; whether it is such a delta, or what differs.
fn scan_as_decode(message: &[u8], caps: &Caps) -> Result<bool, &'static str> {
    let decoded = FrameDelta::decode(message, caps);
    let Some((seq, length, damage)) = delta_scan(message, caps) else {
        return if decoded.is_none() { Ok(false) } else { Err("decoded but not scanned") };
    };
    let damage_decodes = twidget_wire::decode_damage(damage, &caps.values).is_some();
    let Some(delta) = decoded else {
        return if damage_decodes { Err("scanned with a damage that decodes, but not decoded") } else { Ok(true) };
    };
    let runs: u64 = delta.runs.iter().map(|r| if r.literal.is_empty() { u64::from(r.length) } else { r.literal.len() as u64 }).sum();
    if delta.seq == seq && runs == u64::from(length) && damage_decodes { Ok(false) } else { Err("scanned otherwise than decoded") }
}

/// delta_scan, the checks of a FrameDelta that make nothing: generated deltas between lists that share runs, with no,
/// empty, small and large damage, each changed three times, one in ten cut at every byte, one in twenty with a byte after
/// it, under a Frame's header and in version 2, under the default and tight caps, and one in twenty-five under message
/// and frame caps around its own size and its Frame's; remote_tests' cases. Every answer also against
/// `FrameDelta::decode` (`scan_as_decode`).
#[test]
fn frame_delta_scans_alike() {
    let Some(oracle) = Oracle::find("remote_oracle") else { return };
    let mut random = Lcg::new(0x7e35_0002);
    let mut palette = cpp_compact_list();
    palette.extend(compact_list());
    let mut c = Comparison::new();
    let mut damage_left = 0usize;
    let mut unlike_decode = Vec::new();
    let mut scan = |c: &mut Comparison, caps: &Caps, message: &[u8], what: String| {
        match scan_as_decode(message, caps) {
            Ok(left) => damage_left += usize::from(left),
            Err(why) => unlike_decode.push(format!("{what}: {why}")),
        }
        c.case(&framed(b's', message), answer_scanned(message, caps), what);
    };
    let caps_with = |set: &dyn Fn(&mut Caps)| {
        let mut caps = Caps::default();
        set(&mut caps);
        caps
    };
    for (k, caps) in freeze_caps().iter().enumerate() {
        c.setting(&caps_record(caps));
        let mut previous = cpp_compact_list();
        for i in 0..1200 {
            let list = drawn_list(&mut random, &palette);
            let damage = match random.below(4) {
                0 => None,
                1 => Some(Vec::new()),
                2 => Some(vec![Rect::new(1.0, 2.0, 3.0, 4.0); 1 + random.below(3)]),
                _ => Some(vec![Rect::new(random.below(50) as f32, 0.5, 8.0, 2.0); 1 + random.below(80)]),
            };
            let (base, now) = (list_bytes(&previous), list_bytes(&list));
            let runs = if random.chance(3) { ends_alike(&base, &now) } else { mixed_runs(&mut random, &base, &now) };
            let seq = if random.chance(8) { u64::MAX - random.below(3) as u64 } else { i as u64 + 2 };
            let delta = FrameDelta { seq, state: any_state_for(&mut random), base: i as u64 + 1, runs, damage };
            previous = list;
            let mut message = Vec::new();
            if !delta.encode(&Caps::default(), &mut message) {
                continue;
            }
            scan(&mut c, caps, &message, format!("delta {i}, caps {k}"));
            for round in 0..3 {
                let mut changed = message.clone();
                let edits = mutate(&mut random, &mut changed);
                scan(&mut c, caps, &changed, format!("delta {i} changed ({round}), caps {k}:{edits}"));
            }
            if i % 10 == 0 {
                for n in 0..message.len() {
                    scan(&mut c, caps, &message[..n], format!("delta {i} cut to {n}, caps {k}"));
                }
            }
            if i % 20 == 0 {
                scan(&mut c, caps, &[message.as_slice(), &[0]].concat(), format!("delta {i} with a byte after it, caps {k}"));
                let mut other = message.clone();
                other[0] = Kind::Frame as u8;
                scan(&mut c, caps, &other, format!("delta {i} under a Frame's header, caps {k}"));
                other = message.clone();
                other[1] = 2;
                scan(&mut c, caps, &other, format!("delta {i} in version 2, caps {k}"));
            }
            if k == 0 && i % 25 == 0 {
                let Some((frame, _)) = rebuild(&message, &Caps::default(), i as u64 + 1, &base) else { continue };
                let whole = encoded(&frame).len();
                for n in [message.len() - 1, message.len(), message.len() + 1, whole - 1, whole, whole + 1] {
                    for (which, at) in [
                        ("message", caps_with(&|c| c.message = n)),
                        ("frame", caps_with(&|c| c.frame = n)),
                        (
                            "message and frame",
                            caps_with(&|c| {
                                c.message = n;
                                c.frame = n;
                            }),
                        ),
                    ] {
                        c.setting(&caps_record(&at));
                        scan(
                            &mut c,
                            &at,
                            &message,
                            format!("delta {i} under a {which} cap of {n} ({} bytes, its Frame {whole})", message.len()),
                        );
                    }
                }
                c.setting(&caps_record(caps));
            }
        }
    }

    // remote_tests' cases: the documented delta, its cuts and changes, and delta_bounds' deltas at each cap and a byte past it.
    let defaults = Caps::default();
    c.setting(&caps_record(&defaults));
    let delta = encoded(&FrameDelta {
        seq: u64::MAX,
        state: special_state(),
        base: 0x0102030405060708,
        runs: vec![DeltaRun { offset: 0, length: 0, literal: b"x".to_vec() }],
        damage: None,
    });
    for n in 0..=delta.len() {
        scan(&mut c, &defaults, &delta[..n], format!("the documented delta cut to {n}"));
    }
    scan(&mut c, &defaults, &[delta.as_slice(), &[0]].concat(), "the documented delta with a byte after it".into());
    let mut other = delta.clone();
    other[0] = Kind::Frame as u8;
    scan(&mut c, &defaults, &other, "the documented delta under a Frame's header".into());
    other = delta.clone();
    other[1] = 2;
    scan(&mut c, &defaults, &other, "the documented delta in version 2".into());
    let painted = encoded(&Frame { seq: 5, state: ordinary_state(), painted: true, list: compact_list(), damage: None });
    scan(&mut c, &defaults, &painted, "a Frame".into());
    let (base, list) = (list_bytes(&cpp_compact_list()), list_bytes(&moved_list()));
    let bytes = encoded(&FrameDelta { seq: 2, state: State::default(), base: 1, runs: ends_alike(&base, &list), damage: None });
    let whole = encoded(&Frame { seq: 2, state: State::default(), painted: true, list: moved_list(), damage: None });
    let tiny = list_bytes(&vec![twidget_wire::Command::default()]);
    let to_tiny = encoded(&FrameDelta {
        seq: 2,
        state: State::default(),
        base: 1,
        runs: vec![DeltaRun { offset: 0, length: 0, literal: tiny }],
        damage: None,
    });
    let rects = Some(vec![Rect::new(1.0, 2.0, 3.0, 4.0); 64]);
    let damaged = encoded(&FrameDelta { seq: 2, state: State::default(), base: 1, runs: ends_alike(&base, &list), damage: rects.clone() });
    let damaged_whole = encoded(&Frame { seq: 2, state: State::default(), painted: true, list: moved_list(), damage: rects });
    let longest = base.len().max(list.len());
    let message_whole = whole.len().max(bytes.len());
    let cases: [(&str, &[u8], Caps); 15] = [
        (
            "the short list's delta under caps of its own size",
            &to_tiny,
            caps_with(&|c| {
                c.message = to_tiny.len();
                c.frame = to_tiny.len();
            }),
        ),
        ("the short list's delta a byte past the message cap", &to_tiny, caps_with(&|c| c.message = to_tiny.len() - 1)),
        ("the short list's delta a byte past the frame cap", &to_tiny, caps_with(&|c| c.frame = to_tiny.len() - 1)),
        ("the delta under a delta_base as long as its list or base", &bytes, caps_with(&|c| c.delta_base = longest)),
        ("the delta under a delta_base a byte shorter", &bytes, caps_with(&|c| c.delta_base = longest - 1)),
        ("the delta under a delta_base of 0", &bytes, caps_with(&|c| c.delta_base = 0)),
        ("the delta whose Frame is as long as the frame cap", &bytes, caps_with(&|c| c.frame = whole.len())),
        ("the delta whose Frame is a byte past the frame cap", &bytes, caps_with(&|c| c.frame = whole.len() - 1)),
        ("the delta whose Frame is as long as the message cap", &bytes, caps_with(&|c| c.message = message_whole)),
        ("the delta whose Frame is a byte past the message cap", &bytes, caps_with(&|c| c.message = message_whole - 1)),
        ("the delta itself a byte past the frame cap", &bytes, caps_with(&|c| c.frame = bytes.len() - 1)),
        ("the delta with damage whose Frame is as long as the frame cap", &damaged, caps_with(&|c| c.frame = damaged_whole.len())),
        ("the delta with damage whose Frame is a byte past the frame cap", &damaged, caps_with(&|c| c.frame = damaged_whole.len() - 1)),
        ("the delta with damage under a damage cap of 63 rects", &damaged, caps_with(&|c| c.values.rects = 63)),
        ("the delta with damage under the default caps", &damaged, Caps::default()),
    ];
    for (what, message, caps) in &cases {
        c.setting(&caps_record(caps));
        scan(&mut c, caps, message, (*what).into());
    }
    let cases = c.len();
    let tally = c.check(&oracle);
    assert!(
        unlike_decode.is_empty(),
        "{} scans unlike decode, the first:\n{}",
        unlike_decode.len(),
        unlike_decode.iter().take(10).cloned().collect::<Vec<_>>().join("\n")
    );
    eprintln!(
        "frame delta scans: {cases} cases, {} refused, {} taken, {damage_left} of them with a damage decode refuses",
        tally[0], tally[1]
    );
    assert!(tally[0] > 0 && tally[1] > 0 && damage_left > 0);
}
