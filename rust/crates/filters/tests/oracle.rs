//! The Rust filters against the C++ (`oracle/filters_oracle`, built from core/src/filters.cpp): every colour
//! matrix, every chain's reach and every filtered image, bit for bit and byte for byte, on generated filters
//! (odd amounts, offsets and colours among them), the filter lists of widget-sandbox's corpus style texts,
//! and generated images in both byte orders, with strides, scales and contents of every kind. Skipped
//! without `TWIDGET_ORACLE_DIR`; `tools/gate.py` sets it.

use std::path::{Path, PathBuf};

use twidget_filters::*;
use twidget_testkit::{Comparison, Fields, Lcg, Oracle};
use twidget_wire::{Color, Filter, FilterKind};

/// Floats a filter can hold besides ordinary ones: NaNs of either sign, -0, both infinities, a subnormal,
/// the largest float.
const SPECIALS: [u32; 8] = [0x7fc12345, 0xffc54321, 0x7fc00000, 0x80000000, 0x7f800000, 0xff800000, 0x00000003, 0x7f7fffff];

fn special(random: &mut Lcg) -> f32 {
    f32::from_bits(SPECIALS[random.below(SPECIALS.len())])
}

fn any_kind(random: &mut Lcg) -> FilterKind {
    FilterKind::ALL[random.below(FilterKind::ALL.len())]
}

fn amount(random: &mut Lcg, kind: FilterKind) -> f32 {
    match random.below(24) {
        0 => special(random),
        1 => -(random.below(40) as f32) / 8.0,
        2 => 0.0,
        3 => f32::from_bits(random.next() ^ (random.next() << 1)),
        4 => 1e6 * random.below(100) as f32,
        _ => match kind {
            FilterKind::Blur | FilterKind::DropShadow => random.below(240) as f32 / 8.0,
            FilterKind::HueRotate => random.below(1440) as f32 - 720.0 + random.below(8) as f32 / 8.0,
            _ => random.below(300) as f32 / 100.0,
        },
    }
}

fn offset(random: &mut Lcg) -> f32 {
    match random.below(20) {
        0 => special(random),
        1 => 1e9,
        _ => (random.below(320) as f32 - 160.0) / 8.0,
    }
}

fn any_color(random: &mut Lcg) -> Color {
    match random.below(4) {
        0 => Color::BLACK,
        1 => Color::rgba(0, 0, 0, 0),
        _ => Color::rgba(random.next() as u8, random.next() as u8, random.next() as u8, random.next() as u8),
    }
}

fn any_filter(random: &mut Lcg) -> Filter {
    let kind = any_kind(random);
    let mut f = Filter { kind, amount: amount(random, kind), ..Filter::default() };
    if kind == FilterKind::DropShadow || random.chance(8) {
        f.x = offset(random);
        f.y = offset(random);
        f.color = any_color(random);
    }
    f
}

fn put_filter(o: &mut Fields, f: &Filter) {
    o.u8(f.kind.as_u8()).u32(f.amount.to_bits()).u32(f.x.to_bits()).u32(f.y.to_bits());
    o.u8(f.color.r).u8(f.color.g).u8(f.color.b).u8(f.color.a);
}

fn put_list(o: &mut Fields, list: &[Filter]) {
    o.u32(list.len() as u32);
    for f in list {
        put_filter(o, f);
    }
}

/// A float's bits, every NaN as 0x7fc00000 (as the oracle writes them): which NaN an operation gives is the
/// compiler's choice, and LLVM and MSVC choose differently where two meet.
fn bits(out: &mut Vec<u8>, v: f32) {
    let v = if v.is_nan() { f32::NAN } else { v };
    out.extend_from_slice(&v.to_bits().to_le_bytes());
}

fn matrix_case(c: &mut Comparison, f: &Filter, what: String) {
    let mut o = Fields::new();
    put_filter(&mut o, f);
    let mut m = ColorMatrix { offset: 7.0, ..ColorMatrix::default() };
    let ok = color_matrix(f, &mut m);
    let mut out = Vec::new();
    for v in m.rgb {
        bits(&mut out, v);
    }
    bits(&mut out, m.offset);
    bits(&mut out, m.alpha);
    c.case(&o.framed(b'm'), (u8::from(ok), out), what);
}

fn reach_case(c: &mut Comparison, list: &[Filter], what: String) {
    let mut o = Fields::new();
    put_list(&mut o, list);
    let r = filter_reach(list);
    let mut out = Vec::new();
    for v in [r.left, r.top, r.right, r.bottom] {
        bits(&mut out, v);
    }
    c.case(&o.framed(b'r'), (1, out), what);
}

struct Image {
    w: i32,
    h: i32,
    stride: usize,
    bgra: bool,
    scale: f32,
    pixels: Vec<u8>,
}

fn pixels_case(c: &mut Comparison, image: &Image, list: &[Filter], what: String) {
    let mut o = Fields::new();
    o.i32(image.w).i32(image.h).u32(image.stride as u32).flag(image.bgra).u32(image.scale.to_bits());
    put_list(&mut o, list);
    o.0.extend_from_slice(&image.pixels);
    let mut px = image.pixels.clone();
    filter_pixels(&mut px, image.w, image.h, image.stride, image.bgra, list, image.scale);
    c.case(&o.framed(b'p'), (1, px), what);
}

/// An image of up to 20 × 20 px, its rows `stride` bytes apart with a few bytes past the last, its content
/// premultiplied noise, straight noise, a dot, an edge or flat; now and then no image at all.
fn any_image(random: &mut Lcg) -> Image {
    let side = |random: &mut Lcg| if random.chance(30) { -(random.below(2) as i32) } else { 1 + random.below(20) as i32 };
    let w = side(random);
    let h = side(random);
    let stride = w.max(0) as usize * 4 + if random.chance(3) { random.below(9) } else { 0 };
    let bytes = if w > 0 && h > 0 { (h as usize - 1) * stride + w as usize * 4 } else { random.below(16) } + random.below(5);
    let mut pixels = vec![0u8; bytes];
    let content = random.below(6);
    for i in 0..bytes / 4 {
        let px = &mut pixels[i * 4..i * 4 + 4];
        match content {
            0 => {
                let a = random.next() as u8;
                for v in px.iter_mut().take(3) {
                    *v = random.below(usize::from(a) + 1) as u8;
                }
                px[3] = a;
            }
            1 => px.iter_mut().for_each(|v| *v = random.next() as u8),
            2 => {
                if i == bytes / 8 {
                    px.copy_from_slice(&[255, 128, 0, 255]);
                }
            }
            3 => {
                if i % 7 < 3 {
                    px.copy_from_slice(&[200, 30, 90, 255]);
                }
            }
            4 => px.copy_from_slice(&[40, 80, 120, 160]),
            _ => {}
        }
    }
    for v in pixels[bytes / 4 * 4..].iter_mut() {
        *v = random.next() as u8;
    }
    let scale = match random.below(12) {
        0 => special(random),
        1 => 0.0,
        2 => -1.0,
        3 => 2.0,
        4 => 0.5,
        5 => 3.0,
        _ => 1.0,
    };
    Image { w, h, stride, bgra: random.chance(3), scale, pixels }
}

// ---- The corpus's filter lists -----------------------------------------------------------------

fn widget_sandbox() -> PathBuf {
    std::env::var_os("TWIDGET_WIDGET_SANDBOX")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../widget-sandbox"))
}

fn html_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            html_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "html") {
            out.push(path);
        }
    }
}

/// Every `filter:` and `backdrop-filter:` value of the corpus, conformance and stress widgets, as written.
fn corpus_filter_texts() -> Vec<String> {
    let mut files = Vec::new();
    for dir in ["corpus", "packages/conformance", "packages/stress"] {
        html_files(&widget_sandbox().join(dir), &mut files);
    }
    let mut out = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        for (at, _) in text.match_indices("filter") {
            let rest = text[at + "filter".len()..].trim_start();
            let Some(value) = rest.strip_prefix(':') else { continue };
            let end = value.find([';', '"', '}', '\n']).unwrap_or(value.len());
            let value = value[..end].trim();
            if !value.is_empty() && !out.iter().any(|v| v == value) {
                out.push(value.to_string());
            }
        }
    }
    out
}

/// A number and its unit, `.5` and `-3px` alike.
fn dimension(text: &str) -> Option<(f32, &str)> {
    let end = text.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e')).unwrap_or(text.len());
    let end = if text[..end].ends_with('e') { end - 1 } else { end };
    Some((text[..end].parse().ok()?, &text[end..]))
}

/// What a widget's filter text comes to as the renderer hands it to a painter, near enough to give both sides
/// the corpus's values: functions in order, lengths in px, angles in degrees, percentages as factors.
fn parse_filters(text: &str) -> Vec<Filter> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim();
        let mut depth = 0;
        let mut close = rest.len();
        for (i, c) in rest[open..].char_indices() {
            depth += i32::from(c == '(') - i32::from(c == ')');
            if depth == 0 {
                close = open + i;
                break;
            }
        }
        let args = &rest[open + 1..close.min(rest.len())];
        rest = &rest[(close + 1).min(rest.len())..];
        let kind = match name {
            "blur" => FilterKind::Blur,
            "brightness" => FilterKind::Brightness,
            "contrast" => FilterKind::Contrast,
            "grayscale" => FilterKind::Grayscale,
            "hue-rotate" => FilterKind::HueRotate,
            "invert" => FilterKind::Invert,
            "opacity" => FilterKind::Opacity,
            "saturate" => FilterKind::Saturate,
            "sepia" => FilterKind::Sepia,
            "drop-shadow" => FilterKind::DropShadow,
            _ => continue,
        };
        let mut f =
            Filter { kind, amount: if kind == FilterKind::Blur || kind == FilterKind::HueRotate { 0.0 } else { 1.0 }, ..Filter::default() };
        let mut lengths = Vec::new();
        let mut depth = 0;
        let mut word = String::new();
        for c in args.chars().chain([' ']) {
            depth += i32::from(c == '(') - i32::from(c == ')');
            if c == ' ' && depth == 0 {
                if let Some((v, unit)) = dimension(&word) {
                    let v = match unit {
                        "%" => v / 100.0,
                        "rad" => v * 180.0 / std::f32::consts::PI,
                        "turn" => v * 360.0,
                        "grad" => v * 0.9,
                        _ => v,
                    };
                    lengths.push(v);
                } else if let Some(color) = twidget_css::parse_color(word.as_bytes()) {
                    f.color = Color::rgba(color.r, color.g, color.b, color.a);
                }
                word.clear();
            } else {
                word.push(c);
            }
        }
        if kind == FilterKind::DropShadow {
            f.x = lengths.first().copied().unwrap_or(0.0);
            f.y = lengths.get(1).copied().unwrap_or(0.0);
            f.amount = lengths.get(2).copied().unwrap_or(0.0);
        } else if let Some(&v) = lengths.first() {
            f.amount = v;
        }
        out.push(f);
    }
    out
}

#[test]
fn matrices_alike() {
    let Some(oracle) = Oracle::find("filters_oracle") else { return };
    let mut random = Lcg::new(0xf117_0001);
    let mut c = Comparison::new();
    for kind in FilterKind::ALL {
        for i in -400..=400 {
            matrix_case(
                &mut c,
                &Filter { kind: *kind, amount: i as f32 / 100.0, ..Filter::default() },
                format!("{kind:?} {}", i as f32 / 100.0),
            );
        }
        for i in 0..=1440 {
            matrix_case(&mut c, &Filter { kind: *kind, amount: i as f32 - 720.0, ..Filter::default() }, format!("{kind:?} {}", i - 720));
        }
        for bits in SPECIALS {
            matrix_case(
                &mut c,
                &Filter { kind: *kind, amount: f32::from_bits(bits), ..Filter::default() },
                format!("{kind:?} bits {bits:#x}"),
            );
        }
    }
    for i in 0..20000 {
        let f = any_filter(&mut random);
        matrix_case(&mut c, &f, format!("generated {i}: {f:?}"));
    }
    let tally = c.check(&oracle);
    eprintln!("colour matrices: {} matrices, {} refused alike", tally[1], tally[0]);
}

#[test]
fn reaches_alike() {
    let Some(oracle) = Oracle::find("filters_oracle") else { return };
    let mut random = Lcg::new(0xf117_0002);
    let mut c = Comparison::new();
    for (i, text) in corpus_filter_texts().iter().enumerate() {
        reach_case(&mut c, &parse_filters(text), format!("corpus {i}: {text}"));
    }
    for i in 0..20000 {
        let list: Vec<Filter> = (0..random.below(6)).map(|_| any_filter(&mut random)).collect();
        reach_case(&mut c, &list, format!("generated {i}: {list:?}"));
    }
    let tally = c.check(&oracle);
    eprintln!("reaches: {} alike", tally[1]);
}

#[test]
fn filtered_pixels_alike() {
    let Some(oracle) = Oracle::find("filters_oracle") else { return };
    let mut random = Lcg::new(0xf117_0003);
    let mut c = Comparison::new();
    let corpus: Vec<(String, Vec<Filter>)> =
        corpus_filter_texts().into_iter().map(|t| (parse_filters(&t), t)).map(|(l, t)| (t, l)).collect();
    assert!(corpus.len() >= 20 && corpus.iter().filter(|(_, l)| !l.is_empty()).count() >= 20, "the corpus has its filter lists");
    for (i, (text, list)) in corpus.iter().enumerate() {
        for k in 0..20 {
            let image = any_image(&mut random);
            pixels_case(&mut c, &image, list, format!("corpus {i} ({text}), image {k} {}x{}", image.w, image.h));
        }
    }
    // A NaN pixel: brightness(inf) of black is inf · 0; lroundf leaves 0 in the low byte on both sides.
    let black = Image { w: 2, h: 1, stride: 8, bgra: false, scale: 1.0, pixels: vec![0, 0, 0, 255, 10, 20, 30, 255] };
    let mut nan = black.pixels.clone();
    filter_pixels(&mut nan, 2, 1, 8, false, &[Filter { kind: FilterKind::Brightness, amount: f32::INFINITY, ..Filter::default() }], 1.0);
    assert_eq!(nan, vec![0, 0, 0, 255, 255, 255, 255, 255], "a NaN channel is byte 0");
    pixels_case(
        &mut c,
        &black,
        &[Filter { kind: FilterKind::Brightness, amount: f32::INFINITY, ..Filter::default() }],
        "brightness(inf)".into(),
    );
    for kind in FilterKind::ALL {
        for i in 0..200 {
            let image = any_image(&mut random);
            let f = Filter {
                kind: *kind,
                amount: amount(&mut random, *kind),
                x: offset(&mut random),
                y: offset(&mut random),
                color: any_color(&mut random),
            };
            pixels_case(&mut c, &image, &[f], format!("{kind:?} {i}: {f:?} on {}x{}", image.w, image.h));
        }
    }
    for i in 0..6000 {
        let image = any_image(&mut random);
        let list: Vec<Filter> = (0..random.below(5)).map(|_| any_filter(&mut random)).collect();
        pixels_case(&mut c, &image, &list, format!("generated {i}: {list:?} on {}x{} stride {}", image.w, image.h, image.stride));
    }
    let tally = c.check(&oracle);
    eprintln!("filtered pixels: {} images alike ({} corpus filter lists)", tally[1], corpus.len());
}
