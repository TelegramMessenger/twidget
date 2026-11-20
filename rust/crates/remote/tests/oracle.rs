//! The Rust messages against the C++ ones (`oracle/remote_oracle`, built from core/src/remote.cpp and
//! wire.cpp): every sample and its cuts, mutated samples, generated messages and pixel answers, under the
//! default caps and under tight ones, decoded by both and encoded again by both. Skipped without
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
