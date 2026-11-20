//! The Rust CSS helpers and C library behaviours against the C++ (`oracle/css_oracle`, built from
//! core/src/css.cpp with the C library's strtod): generated text of every kind the helpers read, the
//! corpus's style text and its declarations' values, and their random changes, through every helper.
//! Skipped without `TWIDGET_ORACLE_DIR`; `tools/gate.py` sets it.

mod common;

use common::*;
use twidget_testkit::{Comparison, Oracle};

fn case(c: &mut Comparison, op: u8, input: &[u8], what: impl Into<String>) {
    c.case(&framed(op, input), answer(op, input), what);
}

fn with_u8(prefix: u8, text: &[u8]) -> Vec<u8> {
    [&[prefix][..], text].concat()
}

fn transform_input(which: u8, w: f32, h: f32, text: &[u8]) -> Vec<u8> {
    [&[which][..], &w.to_bits().to_le_bytes(), &h.to_bits().to_le_bytes(), text].concat()
}

/// Every helper that reads a value, on one value.
fn value_cases(c: &mut Comparison, text: &[u8], what: &str) {
    for op in [b'N', b't', b'l', b'Z', b'W', b'D', b'T', b'C', b'f', b's', b'n', b'a', b'p', b'v'] {
        case(c, op, text, format!("{} of {what}", char::from(op)));
    }
    for dark in [0, 1] {
        case(c, b'L', &with_u8(dark, text), format!("light-dark {dark} of {what}"));
    }
    for sep in [b',', b';', b' '] {
        case(c, b'S', &with_u8(sep, text), format!("split at {} of {what}", char::from(sep)));
    }
    for which in 0..6 {
        for (w, h) in [(100.0f32, 50.0f32), (0.0, 0.0), (33.3, 1e9)] {
            case(c, b'X', &transform_input(which, w, h, text), format!("transform {which} in {w} × {h} of {what}"));
        }
    }
    for at in [0u32, 1, 2, 5] {
        let input = [&at.to_le_bytes()[..], text].concat();
        case(c, b'O', &input, format!("skip_opaque at {at} of {what}"));
    }
    let vars: [(&[u8], &[u8]); 3] = [(b"--a", b"red"), (b"--b", b"var(--a)"), (b"--tg-theme-bg-color", b"#fff")];
    let mut input = (vars.len() as u32).to_le_bytes().to_vec();
    for (k, v) in vars {
        input.extend_from_slice(&(k.len() as u32).to_le_bytes());
        input.extend_from_slice(k);
        input.extend_from_slice(&(v.len() as u32).to_le_bytes());
        input.extend_from_slice(v);
    }
    input.extend_from_slice(text);
    case(c, b'V', &input, format!("substitute_variables of {what}"));
}

#[test]
fn generated_values_alike() {
    let Some(oracle) = Oracle::find("css_oracle") else { return };
    let mut random = Lcg::new(0xc55_0001);
    let mut c = Comparison::new();
    for i in 0..3000 {
        let text = match i % 6 {
            0 => any_color(&mut random),
            1 => any_transform(&mut random),
            2 => any_font(&mut random),
            3 => any_time(&mut random),
            4 => any_calc(&mut random, 0),
            _ => any_css_text(&mut random, 0),
        };
        value_cases(&mut c, text.as_bytes(), &format!("generated {i}: {text:?}"));
        let mut changed = text.into_bytes();
        let edits = mutate(&mut random, &mut changed);
        value_cases(&mut c, &changed, &format!("generated {i} changed{edits}"));
    }
    let tally = c.check(&oracle);
    eprintln!("generated values: {} answers alike ({} refused, {} taken)", tally[0] + tally[1] + tally[2], tally[0], tally[1]);
}

#[test]
fn numbers_alike() {
    let Some(oracle) = Oracle::find("css_oracle") else { return };
    let mut random = Lcg::new(0xc55_0002);
    let mut c = Comparison::new();
    for i in 0..30000 {
        let text = any_number(&mut random);
        let mut with_tail = text.clone();
        if random.chance(3) {
            with_tail.push_str(pick_tail(&mut random));
        }
        for t in [&text, &with_tail] {
            case(&mut c, b's', t.as_bytes(), format!("strtod {i}: {t:?}"));
            case(&mut c, b'N', t.as_bytes(), format!("number {i}: {t:?}"));
        }
    }
    let mut digits = Vec::new();
    for _ in 0..20000 {
        let d = match random.below(4) {
            0 => f64::from_bits((u64::from(random.next()) << 32) | u64::from(random.next())),
            1 => (random.next() as f64 - 2147483648.0) / 10f64.powi(random.below(9) as i32),
            2 => random.below(2000) as f64 / 8.0 - 125.0,
            _ => [1e9, 999999999.0, -1e9, 0.5, -0.5, 1e9 + 0.5, 1e300, -0.0, 0.00005, -0.00004, 2.5e-5][random.below(11)],
        };
        let n = [4i32, 0, 1, 2, 6, -1, 17][random.below(7)];
        digits.push((d, n));
    }
    for (d, n) in digits {
        let input = [&d.to_bits().to_le_bytes()[..], &n.to_le_bytes()].concat();
        case(&mut c, b'F', &input, format!("format_number({d:e}, {n})"));
    }
    let tally = c.check(&oracle);
    eprintln!("numbers: {} answers alike", tally[0] + tally[1]);
}

fn pick_tail(random: &mut Lcg) -> &'static str {
    ["px", " ", "e", "e+", "x", "\0", ".5", "p3", "(", ")"][random.below(10)]
}

#[test]
fn themes_alike() {
    let Some(oracle) = Oracle::find("css_oracle") else { return };
    let mut random = Lcg::new(0xc55_0003);
    let mut c = Comparison::new();
    for i in 0..6000 {
        let theme = any_theme(&mut random);
        case(&mut c, b'H', theme.as_bytes(), format!("theme {i}: {theme}"));
        let inset = any_safe_area(&mut random);
        case(&mut c, b'A', inset.as_bytes(), format!("safe area {i}: {inset}"));
    }
    let tally = c.check(&oracle);
    eprintln!("themes: {} answers alike", tally[1]);
    assert!(tally[1] > 10000);
}

#[test]
fn corpus_styles_alike() {
    let Some(oracle) = Oracle::find("css_oracle") else { return };
    let styles = corpus_styles();
    assert!(styles.len() > 100, "found {} style texts in widget-sandbox's corpus", styles.len());
    let mut random = Lcg::new(0xc55_0004);
    let mut c = Comparison::new();
    let mut values = 0;
    for (i, style) in styles.iter().enumerate() {
        for op in [b'D', b'Z', b'W', b'f'] {
            case(&mut c, op, style, format!("{} of style {i}", char::from(op)));
        }
        for dark in [0, 1] {
            case(&mut c, b'L', &with_u8(dark, style), format!("light-dark {dark} of style {i}"));
        }
        for d in twidget_css::parse_declarations(style) {
            values += 1;
            if values % 3 == 0 {
                value_cases(&mut c, &d.value, &format!("value of {} in style {i}", String::from_utf8_lossy(&d.name)));
            } else {
                for op in [b'C', b'T', b'Z', b'N'] {
                    case(&mut c, op, &d.value, format!("{} of {} in style {i}", char::from(op), String::from_utf8_lossy(&d.name)));
                }
                case(
                    &mut c,
                    b'X',
                    &transform_input(0, 120.0, 40.0, &d.value),
                    format!("transform of {} in style {i}", String::from_utf8_lossy(&d.name)),
                );
            }
        }
        let mut changed = style.clone();
        let edits = mutate(&mut random, &mut changed);
        case(&mut c, b'D', &changed, format!("declarations of style {i} changed{edits}"));
        case(&mut c, b'L', &with_u8(1, &changed), format!("light-dark of style {i} changed{edits}"));
    }
    let tally = c.check(&oracle);
    eprintln!("corpus: {} style texts, {values} declarations, {} answers alike", styles.len(), tally[0] + tally[1] + tally[2]);
}
