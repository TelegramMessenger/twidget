//! The Rust compositor functions against the C++ (`oracle/compositor_oracle`, built from core/src/compositor.cpp):
//! generated display lists through `paint_digest`, `summarize` and `drop_inert_clips`, list pairs and crafted
//! summaries through `damage_between` with the canvases it asks about, the scripted layers through
//! `bake_track` with the times it samples, and tracks through `resume_track`. Every float is compared by its
//! bits, but a NaN the functions compute is any NaN: when two NaNs meet in one operation, which payload the
//! result keeps is the compiler's operand order (IEEE leaves it open), and MSVC's and LLVM's differ. Skipped
//! without `TWIDGET_ORACLE_DIR`; `tools/gate.py` sets it.

mod common;

use std::collections::HashSet;

use common::*;
use twidget_compositor::*;
use twidget_testkit::{Answer, Comparison, Oracle, framed};
use twidget_wire::{Caps, DisplayList, Rect, encode_list};

fn list_bytes(list: &DisplayList) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    encode_list(list, &Caps::default(), &mut out).then_some(out)
}

fn put_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_bits().to_le_bytes());
}

fn put_computed(out: &mut Vec<u8>, v: f32) {
    put_f32(out, if v.is_nan() { f32::from_bits(0x7fc00000) } else { v });
}

/// Input rectangles keep their bits; ones the functions give have their NaNs as one.
fn put_rect(out: &mut Vec<u8>, r: &Rect, computed: bool) {
    for v in [r.x, r.y, r.w, r.h] {
        if computed { put_computed(out, v) } else { put_f32(out, v) }
    }
}

fn put_keys(out: &mut Vec<u8>, keys: &[LayerKey], computed: bool) {
    out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
    for k in keys {
        let m = &k.matrix;
        for v in [k.t, m.a, m.b, m.c, m.d, m.e, m.f, k.opacity] {
            if computed { put_computed(out, v) } else { put_f32(out, v) }
        }
    }
}

fn put_summary(out: &mut Vec<u8>, s: &PaintSummary, computed: bool) {
    out.extend_from_slice(&(s.entries.len() as u32).to_le_bytes());
    for e in &s.entries {
        out.extend_from_slice(&e.key.to_le_bytes());
        put_rect(out, &e.ink, computed);
        out.extend_from_slice(&e.canvas.to_le_bytes());
        out.push(u8::from(e.backdrop));
    }
}

fn put_changed(out: &mut Vec<u8>, mode: u8, changed: &[i32]) {
    out.push(mode);
    out.extend_from_slice(&(changed.len() as u32).to_le_bytes());
    for c in changed {
        out.extend_from_slice(&c.to_le_bytes());
    }
}

fn damage_answer(mode: u8, changed: &[i32], a: &PaintSummary, b: &PaintSummary) -> Answer {
    let changed: HashSet<i32> = changed.iter().copied().collect();
    let mut asked = Vec::new();
    let mut callback = |canvas: i32| {
        asked.push(canvas);
        changed.contains(&canvas)
    };
    let mut rects = vec![Rect::new(1.0, 2.0, 3.0, 4.0)];
    let result = damage_between(a, b, if mode != 0 { Some(&mut callback) } else { None }, &mut rects);
    let mut out = vec![u8::from(result)];
    out.extend_from_slice(&(rects.len() as u32).to_le_bytes());
    for r in &rects {
        put_rect(&mut out, r, true);
    }
    out.extend_from_slice(&(asked.len() as u32).to_le_bytes());
    for c in &asked {
        out.extend_from_slice(&c.to_le_bytes());
    }
    (1, out)
}

fn any_changed(random: &mut Lcg) -> (u8, Vec<i32>) {
    let mode = u8::from(!random.chance(5));
    let changed = (0..random.below(4)).map(|_| random.below(6) as i32).collect();
    (mode, changed)
}

/// Every list function on one list; false when the list has no binary form.
fn list_cases(c: &mut Comparison, list: &DisplayList, what: &str, dropped: &mut usize) -> bool {
    let Some(bytes) = list_bytes(list) else { return false };
    c.case(&framed(b'd', &bytes), (1, paint_digest(list).to_le_bytes().to_vec()), format!("paint_digest of {what}"));
    let mut summary = Vec::new();
    put_summary(&mut summary, &summarize(list), true);
    c.case(&framed(b's', &bytes), (1, summary), format!("summarize of {what}"));
    let mut dropped_list = list.clone();
    let gone = drop_inert_clips(&mut dropped_list);
    *dropped += usize::from(gone > 0);
    let mut answer = (gone as u64).to_le_bytes().to_vec();
    let encoded = encode_list(&dropped_list, &Caps::default(), &mut answer);
    c.case(&framed(b'i', &bytes), if encoded { (1, answer) } else { (2, Vec::new()) }, format!("drop_inert_clips of {what}"));
    true
}

#[test]
fn lists_alike() {
    let Some(oracle) = Oracle::find("compositor_oracle") else { return };
    let mut random = Lcg::new(0xc0_0001);
    let mut c = Comparison::new();
    let (mut lists, mut dropped) = (0, 0);
    for i in 0..6000 {
        let list = any_list(&mut random);
        lists += usize::from(list_cases(&mut c, &list, &format!("list {i}"), &mut dropped));
        let changed = edited(&mut random, &list);
        lists += usize::from(list_cases(&mut c, &changed, &format!("list {i} edited"), &mut dropped));
    }
    let tally = c.check(&oracle);
    eprintln!("{lists} lists ({dropped} with clips dropped): {} answers alike", tally[0] + tally[1] + tally[2]);
}

#[test]
fn damage_alike() {
    let Some(oracle) = Oracle::find("compositor_oracle") else { return };
    let mut random = Lcg::new(0xc0_0002);
    let mut c = Comparison::new();
    for i in 0..8000 {
        let before = any_list(&mut random);
        let after = if random.chance(10) { any_list(&mut random) } else { edited(&mut random, &before) };
        let (Some(a), Some(b)) = (list_bytes(&before), list_bytes(&after)) else { continue };
        let (mode, changed) = any_changed(&mut random);
        let mut input = Vec::new();
        put_changed(&mut input, mode, &changed);
        for bytes in [&a, &b] {
            input.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            input.extend_from_slice(bytes);
        }
        let rust = damage_answer(mode, &changed, &summarize(&before), &summarize(&after));
        c.case(&framed(b'L', &input), rust, format!("damage of list pair {i}"));
    }
    for i in 0..8000 {
        let keys = [2, 4, 8, 64][random.below(4)];
        let before = any_summary(&mut random, keys);
        let after = if random.chance(3) {
            let mut s = before.clone();
            for _ in 0..random.below(6) {
                let n = s.entries.len();
                if n > 1 {
                    let (x, y) = (random.below(n), random.below(n));
                    s.entries.swap(x, y);
                }
            }
            s
        } else {
            any_summary(&mut random, keys)
        };
        let (mode, changed) = any_changed(&mut random);
        let mut input = Vec::new();
        put_changed(&mut input, mode, &changed);
        put_summary(&mut input, &before, false);
        put_summary(&mut input, &after, false);
        c.case(&framed(b'D', &input), damage_answer(mode, &changed, &before, &after), format!("damage of summaries {i}"));
    }
    let tally = c.check(&oracle);
    eprintln!("damage: {} answers alike", tally[1]);
}

#[test]
fn tracks_alike() {
    let Some(oracle) = Oracle::find("compositor_oracle") else { return };
    let mut random = Lcg::new(0xc0_0003);
    let mut c = Comparison::new();
    for i in 0..6000 {
        let bounds = if random.chance(10) { any_rect(&mut random) } else { Rect::new(0.0, 0.0, extent(&mut random), extent(&mut random)) };
        let kind = random.below(8) as u8;
        let mut p = [0.0f64; 8];
        for v in &mut p {
            *v = match random.below(10) {
                0 => [f64::NAN, f64::INFINITY, 1e300, -0.0][random.below(4)],
                1 => 0.0,
                _ => (random.below(4000) as f64 - 1000.0) / 64.0,
            };
        }
        if kind == 0 && random.chance(2) {
            p[2] = 0.0;
            p[3] = std::f64::consts::TAU * random.below(4) as f64;
        }
        if kind == 4 {
            p[0] = random.below(9) as f64 / 8.0 + if random.chance(2) { 0.0 } else { 0.03 };
        }
        let fail_at = if random.chance(4) { 1 + random.below(300) as u32 } else { 0 };
        let mut input = Vec::new();
        put_rect(&mut input, &bounds, false);
        input.push(kind);
        for v in p {
            input.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        input.extend_from_slice(&fail_at.to_le_bytes());
        let mut asked = Vec::new();
        let mut keys = vec![LayerKey::default()];
        let ok = bake_track(
            |t, key| {
                asked.push(t);
                if fail_at != 0 && asked.len() == fail_at as usize {
                    return false;
                }
                sample_of(kind, &p, t, key)
            },
            &bounds,
            &mut keys,
        );
        let mut out = vec![u8::from(ok)];
        put_keys(&mut out, &keys, false);
        out.extend_from_slice(&(asked.len() as u32).to_le_bytes());
        for t in &asked {
            out.extend_from_slice(&t.to_bits().to_le_bytes());
        }
        c.case(&framed(b'b', &input), (1, out), format!("bake {i}: kind {kind}, {p:?}, failing at {fail_at}"));
    }
    for i in 0..12000 {
        let track = any_track(&mut random);
        let elapsed = any_elapsed(&mut random, &track);
        let mut input = Vec::new();
        input.extend_from_slice(&track.start.to_bits().to_le_bytes());
        input.extend_from_slice(&track.duration.to_bits().to_le_bytes());
        input.push(u8::from(track.repeat));
        put_keys(&mut input, &track.keys, false);
        input.extend_from_slice(&elapsed.to_bits().to_le_bytes());
        let (keys, span) = resume_track(&track, elapsed);
        let mut out = Vec::new();
        put_keys(&mut out, &keys, true);
        out.extend_from_slice(&span.to_bits().to_le_bytes());
        c.case(&framed(b'r', &input), (1, out), format!("resume {i}: {elapsed} s into {track:?}"));
    }
    let tally = c.check(&oracle);
    eprintln!("tracks: {} answers alike", tally[1]);
}
