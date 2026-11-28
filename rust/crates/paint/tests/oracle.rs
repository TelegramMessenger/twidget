//! The Rust paint policy against the C++ one (`oracle/paint_oracle`, built from core/src/paint_policy.cpp):
//! generated shadow lists, styles, keyframe sequences and scripts of counter steps through both, beyond
//! the shared fixture's cases. Skipped without `TWIDGET_ORACLE_DIR`; `tools/gate.py` sets it.

use twidget_paint::*;
use twidget_testkit::{Comparison, Lcg, Oracle, framed, mutate};

fn pick<'a>(random: &mut Lcg, items: &[&'a str]) -> &'a str {
    items[random.below(items.len())]
}

fn length(random: &mut Lcg) -> String {
    match random.below(10) {
        0 => pick(random, &["calc(1px + 2px)", "min(1px, 2px)", "MAX(0px,1px)", "clamp(0px, 1px, 2px)", "var(--x)", "env(a)"]).into(),
        1 => pick(random, &["0", "0px", "-0px", "1e-9px", "+.5px", "1e", "1e+px", "nanpx", "0x1px", "5%", "3em", "2px2"]).into(),
        _ => format!("{}{}", random.below(20) as i32 - 5, pick(random, &["px", "", "rem", "PX"])),
    }
}

fn shadow(random: &mut Lcg) -> String {
    let layers = 1 + random.below(3);
    let mut out = Vec::new();
    for _ in 0..layers {
        let mut parts = Vec::new();
        if random.chance(4) {
            parts.push("inset".to_string());
        }
        if random.chance(2) {
            parts.push(pick(random, &["rgba(0, 0, 0, 0.2)", "#000", "red", "rgb(1 2 3 / .5)", "hsl(1,2%,3%)"]).to_string());
        }
        for _ in 0..random.below(5) {
            parts.push(length(random));
        }
        if random.chance(3) {
            parts.push(pick(random, &["black", "currentColor", "rgba(0,0,0,.5)"]).to_string());
        }
        out.push(parts.join(pick(random, &[" ", "  ", "\t", "\u{b}"])));
    }
    match random.below(10) {
        0 => pick(random, &["none", "NONE", " none ", "", "  "]).into(),
        _ => out.join(pick(random, &[", ", ",", " , "])),
    }
}

fn opacity(random: &mut Lcg) -> String {
    pick(random, &["1", "0.999", "100%", "99%", "1.0", "0", "", "nan", "inf", "-1", "1e0", "  0.5 ", "x", "50", "0x1", ".9", "1%"]).into()
}

fn filter(random: &mut Lcg) -> String {
    pick(random, &["none", "", "blur(2px)", "brightness(1)", " None ", "url(#x)", "drop-shadow(0 0 2px red)"]).into()
}

fn style_input(random: &mut Lcg) -> Vec<u8> {
    let mut out = vec![u8::from(random.chance(3)), random.below(3) as u8];
    let fields = [
        pick(random, &["block", "none", "contents", "inline", "", " NONE ", "flex"]).to_string(),
        pick(random, &["", "none", "normal", "\"x\"", "''", "counter(a)", " NORMAL "]).to_string(),
        shadow(random),
        filter(random),
        filter(random),
        opacity(random),
    ];
    for f in fields {
        out.extend_from_slice(&(f.len() as u32).to_le_bytes());
        out.extend_from_slice(f.as_bytes());
    }
    out
}

fn rust_style(input: &[u8]) -> Option<BoxPaint> {
    let pseudo = input[0] != 0;
    let button = match input[1] {
        1 => Button::Disabled,
        2 => Button::Pressed,
        _ => Button::None,
    };
    let mut fields: Vec<&[u8]> = Vec::new();
    let mut at = 2;
    for _ in 0..6 {
        let n = u32::from_le_bytes(input[at..at + 4].try_into().unwrap()) as usize;
        fields.push(&input[at + 4..at + 4 + n]);
        at += 4 + n;
    }
    let style = Style {
        display: fields[0],
        content: fields[1],
        box_shadow: fields[2],
        filter: fields[3],
        backdrop_filter: fields[4],
        opacity: fields[5],
        button,
    };
    box_paint(&style, pseudo)
}

fn paint_answer(p: Option<BoxPaint>) -> (u8, Vec<u8>) {
    match p {
        None => (0, Vec::new()),
        Some(p) => (1, vec![u8::from(p.heavy), u8::from(p.backdrop)]),
    }
}

fn put_str(out: &mut Vec<u8>, s: &[u8]) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s);
}

/// A keyframe sequence as the oracle takes it, and the Rust answer.
fn animation_case(random: &mut Lcg) -> (Vec<u8>, (u8, Vec<u8>)) {
    let mut input = Vec::new();
    let mut paint = AnimationPaint::default();
    let frames = random.below(4);
    input.extend_from_slice(&(frames as u32).to_le_bytes());
    for _ in 0..frames {
        let has = !random.chance(5);
        let offset = [0.0, 1.0, 0.5, -0.0, 1.0000001, 0.25][random.below(6)];
        input.push(u8::from(has));
        input.extend_from_slice(&f64::to_bits(offset).to_le_bytes());
        paint.keyframe(has.then_some(offset));
        let props = random.below(4);
        input.extend_from_slice(&(props as u32).to_le_bytes());
        for _ in 0..props {
            let name = pick(
                random,
                &[
                    "opacity",
                    "Opacity",
                    "filter",
                    "backdropFilter",
                    "webkitBackdropFilter",
                    "-webkit-backdrop-filter",
                    "boxShadow",
                    "box-shadow",
                    "MozBoxShadow",
                    "transform",
                    "offset",
                    "computedOffset",
                    "easing",
                    "COMPOSITE",
                    "color",
                    "oFilter",
                ],
            );
            let valued = !random.chance(6);
            let value = shadow(random);
            put_str(&mut input, name.as_bytes());
            input.push(u8::from(valued));
            put_str(&mut input, value.as_bytes());
            paint.property(name.as_bytes(), valued.then_some(value.as_bytes()));
        }
    }
    (input, paint_answer(paint.result()))
}

/// A script of counter steps as the oracle takes it, and the Rust answer.
fn counter_case(random: &mut Lcg) -> (Vec<u8>, (u8, Vec<u8>)) {
    let mut input = Vec::new();
    let mut counter = PaintCounter::<Vec<u8>>::new();
    let mut out = Vec::new();
    let steps = random.below(80);
    input.extend_from_slice(&(steps as u32).to_le_bytes());
    let keys: Vec<Vec<u8>> = (0..24).map(|k| format!("k{k}").into_bytes()).collect();
    for _ in 0..steps {
        let kind = [0, 0, 0, 1, 2, 2, 3, 4, 4, 5, 6][random.below(11)];
        let key = keys[random.below(keys.len())].clone();
        input.push(kind);
        put_str(&mut input, &key);
        match kind {
            0 => {
                let has = !random.chance(4);
                let p = BoxPaint { heavy: random.chance(2), backdrop: random.chance(6) };
                input.extend_from_slice(&[u8::from(has), u8::from(p.heavy), u8::from(p.backdrop)]);
                counter.set_box(&key, has.then_some(p));
            }
            1 => counter.remove_box(&key),
            2 => {
                let id = pick(random, &["a:x", "a:y", "t:opacity"]);
                let p = BoxPaint { heavy: true, backdrop: random.chance(4) };
                put_str(&mut input, id.as_bytes());
                input.extend_from_slice(&[u8::from(p.heavy), u8::from(p.backdrop)]);
                counter.start_animation(&key, id.as_bytes(), p);
            }
            3 => {
                let id = pick(random, &["a:x", "a:y", "t:opacity", "none"]);
                put_str(&mut input, id.as_bytes());
                counter.end_animation(&key, id.as_bytes());
            }
            4 => {
                let v = counter.check();
                for n in [counter.heavy(), counter.backdrop(), counter.peak_heavy(), counter.peak_backdrop()] {
                    out.extend_from_slice(&n.to_le_bytes());
                }
                out.push(u8::from(v.is_some()));
                out.push(v.map_or(0, |v| v.code as u8));
                out.extend_from_slice(&v.map_or(0, |v| v.count).to_le_bytes());
                out.push(u8::from(counter.has_box(&key)));
            }
            5 => counter.begin_scan(),
            _ => out.push(u8::from(counter.end_scan())),
        }
    }
    (input, (1, out))
}

#[test]
fn rules_and_counters_alike() {
    let Some(oracle) = Oracle::find("paint_oracle") else { return };
    let mut random = Lcg::new(0x9a17_0001);
    let mut c = Comparison::new();
    for i in 0..20000 {
        let value = shadow(&mut random);
        let mut changed = value.clone().into_bytes();
        let edits = mutate(&mut random, &mut changed);
        for (text, what) in [(value.as_bytes().to_vec(), format!("shadow {i}: {value:?}")), (changed, format!("shadow {i} changed{edits}"))]
        {
            c.case(&framed(b'b', &text), (u8::from(box_shadow_blurred(&text)), Vec::new()), format!("blurred of {what}"));
            c.case(&framed(b'w', &text), (1, box_shadow_without_blur(&text)), format!("without blur of {what}"));
            c.case(&framed(b'h', &text), (u8::from(hides_subtree(&text)), Vec::new()), format!("hides of {what}"));
        }
        let style = style_input(&mut random);
        c.case(&framed(b'p', &style), paint_answer(rust_style(&style)), format!("box {i}"));
        let (animation, rust) = animation_case(&mut random);
        c.case(&framed(b'a', &animation), rust, format!("animation {i}"));
        if i % 4 == 0 {
            let (script, rust) = counter_case(&mut random);
            c.case(&framed(b'c', &script), rust, format!("counter script {i}"));
        }
    }
    let tally = c.check(&oracle);
    eprintln!("paint policy: {} answers alike", tally[0] + tally[1]);
}
