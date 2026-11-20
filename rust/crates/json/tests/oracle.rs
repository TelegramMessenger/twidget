//! The Rust JSON against widget-sandbox's json.h (`oracle/json_oracle`): the same texts parsed by both
//! and dumped, the same doubles written by both number forms, as_int and quote. Skipped without
//! `TWIDGET_ORACLE_DIR`; `tools/gate.py` sets it.

mod common;

use std::path::{Path, PathBuf};

use twidget_json::*;
use twidget_testkit::{Comparison, Lcg, Oracle, framed, mutate};

fn parse_case(c: &mut Comparison, text: &[u8], surrogates: Surrogates, depth: i32, what: String) {
    let mut record = vec![b'p', u8::from(surrogates == Surrogates::Keep)];
    record.extend_from_slice(&depth.to_le_bytes());
    record.extend_from_slice(&framed(0, text)[1..]);
    let rust = match Json::parse_with(text, surrogates, depth) {
        Some(v) => (1, v.dump()),
        None => (0, Vec::new()),
    };
    c.case(&record, rust, what);
}

fn number_cases(c: &mut Comparison, d: f64, what: &str) {
    let bits = d.to_bits().to_le_bytes();
    let tagged = |tag: u8| [&[tag][..], &bits[..]].concat();
    c.case(&tagged(b'n'), (1, number(d).into_bytes()), format!("number({what} = {:#018x})", d.to_bits()));
    c.case(&tagged(b'g'), (1, apple_number(d).into_bytes()), format!("the Apple fallback of {what} = {:#018x}", d.to_bits()));
    c.case(&tagged(b'i'), (1, Json::Number(d).as_int(-7).to_le_bytes().to_vec()), format!("as_int({what} = {:#018x})", d.to_bits()));
}

/// json.h's whole Apple branch: the integer form, then the fallback.
fn apple_number(d: f64) -> String {
    if !d.is_finite() || (d == d.floor() && d.abs() < 1e15) { number_integer_or_null(d) } else { shortest_of_15_to_17(d) }
}

fn number_integer_or_null(d: f64) -> String {
    if d.is_finite() { (d as i64).to_string() } else { "null".to_string() }
}

#[test]
fn fuzz_texts_parse_alike() {
    let Some(oracle) = Oracle::find("json_oracle") else { return };
    let mut c = Comparison::new();
    for (i, text) in common::fuzz_texts().iter().enumerate() {
        parse_case(&mut c, text, Surrogates::Replace, DEFAULT_MAX_DEPTH, format!("fuzz text {i}"));
        parse_case(&mut c, text, Surrogates::Keep, DEFAULT_MAX_DEPTH, format!("fuzz text {i}, surrogates kept"));
    }
    let tally = c.check(&oracle);
    eprintln!("fuzz texts: {} refused, {} parsed alike", tally[0], tally[1]);
    assert!(tally[0] > 0 && tally[1] > 0);
}

/// Nesting at and around the bound, and bounds of every sign.
#[test]
fn depths_alike() {
    let Some(oracle) = Oracle::find("json_oracle") else { return };
    let mut c = Comparison::new();
    for depth in [-2, -1, 0, 1, 2, 5, 127, 128, 129, 300] {
        for n in [0usize, 1, 2, 5, 127, 128, 129, 130, 300] {
            let arrays = [vec![b'['; n], b"1".to_vec(), vec![b']'; n]].concat();
            parse_case(&mut c, &arrays, Surrogates::Replace, depth, format!("{n} arrays under a bound of {depth}"));
            let mut objects = Vec::new();
            for _ in 0..n {
                objects.extend_from_slice(b"{\"k\":");
            }
            objects.push(b'0');
            objects.extend(std::iter::repeat_n(b'}', n));
            parse_case(&mut c, &objects, Surrogates::Replace, depth, format!("{n} objects under a bound of {depth}"));
        }
    }
    c.check(&oracle);
}

fn json_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if !matches!(name, "node_modules" | "target" | "build" | ".git" | "dist" | "test262" | "quickjs") {
                json_files(&path, out);
            }
        } else if name.ends_with(".json") {
            out.push(path);
        }
    }
}

/// widget-sandbox's checkout next to twidget's, or `TWIDGET_WIDGET_SANDBOX`.
pub fn widget_sandbox() -> PathBuf {
    std::env::var_os("TWIDGET_WIDGET_SANDBOX")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../widget-sandbox"))
}

/// Every JSON file of widget-sandbox's packages (fixtures, profile, conformance and stress manifests)
/// and of twidget, as they are and changed at random.
#[test]
fn real_files_parse_alike() {
    let Some(oracle) = Oracle::find("json_oracle") else { return };
    let mut files = Vec::new();
    json_files(&widget_sandbox().join("packages"), &mut files);
    json_files(&widget_sandbox().join("VERSION.json").parent().unwrap().join("corpus"), &mut files);
    json_files(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..").as_path(), &mut files);
    assert!(files.len() >= 10, "found {} JSON files", files.len());
    let mut c = Comparison::new();
    let mut random = Lcg::new(0x15_0000);
    let mut bytes = 0;
    for path in &files {
        let Ok(text) = std::fs::read(path) else { continue };
        if text.len() > (8 << 20) {
            continue;
        }
        bytes += text.len();
        let name = path.display().to_string();
        parse_case(&mut c, &text, Surrogates::Replace, DEFAULT_MAX_DEPTH, name.clone());
        parse_case(&mut c, &text, Surrogates::Keep, 512, format!("{name}, surrogates kept"));
        for round in 0..20 {
            let mut changed = text.clone();
            let edits = mutate(&mut random, &mut changed);
            parse_case(&mut c, &changed, Surrogates::Replace, DEFAULT_MAX_DEPTH, format!("{name} round {round}:{edits}"));
        }
    }
    let tally = c.check(&oracle);
    eprintln!("{} files, {bytes} bytes: {} refused, {} parsed alike", files.len(), tally[0], tally[1]);
}

/// Every escape, surrogate pairs and their halves in every order, bytes past ASCII, controls.
#[test]
fn strings_alike() {
    let Some(oracle) = Oracle::find("json_oracle") else { return };
    let pieces: [&[u8]; 24] = [
        b"\\ud800",
        b"\\udbff",
        b"\\udc00",
        b"\\udfff",
        b"\\ud83d",
        b"\\ude00",
        b"\\u0041",
        b"\\u00e9",
        b"\\uFFFD",
        b"\\u0000",
        b"\\u001f",
        b"\\\"",
        b"\\\\",
        b"\\/",
        b"\\b",
        b"\\f",
        b"\\n",
        b"\\r",
        b"\\t",
        b"x",
        b"\xc3\xa9",
        b"\xff",
        b"\\u",
        b"\\uzz12",
    ];
    let mut c = Comparison::new();
    let mut random = Lcg::new(0x5_7212);
    for i in 0..20000 {
        let mut text = vec![b'"'];
        for _ in 0..1 + random.below(5) {
            text.extend_from_slice(pieces[random.below(pieces.len())]);
        }
        if !random.chance(10) {
            text.push(b'"');
        }
        if random.chance(8) {
            text.insert(1 + random.below(text.len() - 1), random.below(0x20) as u8);
        }
        parse_case(&mut c, &text, Surrogates::Replace, DEFAULT_MAX_DEPTH, format!("string {i}"));
        parse_case(&mut c, &text, Surrogates::Keep, DEFAULT_MAX_DEPTH, format!("string {i}, surrogates kept"));
        let raw: Vec<u8> = (0..random.below(12)).map(|_| random.next() as u8).collect();
        c.case(&framed(b'q', &raw), (1, quote(&raw)), format!("quote of bytes {i}"));
    }
    c.check(&oracle);
}

/// Doubles of every kind written by both: specials, integers around 1e15, powers of ten and their
/// neighbours, short decimals, random bits; and number texts parsed by both, long and near halfway.
#[test]
fn numbers_alike() {
    let Some(oracle) = Oracle::find("json_oracle") else { return };
    let mut c = Comparison::new();
    let mut values = vec![
        0.0,
        -0.0,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        f64::MIN_POSITIVE,
        f64::MAX,
        f64::MIN,
        f64::EPSILON,
        5e-324,
        -5e-324,
        1e15,
        -1e15,
        999999999999999.0,
        999999999999999.5,
        1e15 + 1.0,
        9007199254740992.0,
        2147483647.5,
        -2147483648.5,
        2147483648.0,
        -2147483649.0,
        1e21,
        1e-7,
        0.001,
        0.0001,
        123456.789e10,
        0.1 + 0.2,
        1.0 / 3.0,
    ];
    for e in -330..=310 {
        let p = format!("1e{e}").parse::<f64>().unwrap();
        values.extend([p, -p, f64::from_bits(p.to_bits().wrapping_add(1)), f64::from_bits(p.to_bits().wrapping_sub(1)), 1.5 * p, 7.25 * p]);
    }
    let mut random = Lcg::new(0x4e_0001);
    for _ in 0..40000 {
        values.push(match random.below(4) {
            0 => f64::from_bits((u64::from(random.next()) << 33) ^ (u64::from(random.next()) << 2) ^ u64::from(random.next())),
            1 => (random.next() as f64 - 2147483648.0) / 10f64.powi(random.below(12) as i32),
            2 => random.next() as f64 * 1e8 + random.below(1000) as f64 / 8.0,
            _ => f64::from_bits(0x3ff0_0000_0000_0000 | (u64::from(random.next()) << 20) | u64::from(random.next() & 0xfffff)),
        });
    }
    for (i, d) in values.iter().enumerate() {
        number_cases(&mut c, *d, &format!("value {i}"));
    }
    for i in 0..30000 {
        let mut text = String::new();
        if random.chance(2) {
            text.push('-');
        }
        let most = if random.chance(4) { 40 } else { 17 };
        let digits = 1 + random.below(most);
        for k in 0..digits {
            text.push(char::from(b'0' + random.below(10) as u8 + u8::from(k == 0 && digits > 1 && random.chance(2))).min('9'));
        }
        if text.trim_start_matches('-').starts_with('0') && digits > 1 {
            text = text.replacen('0', "1", 1);
        }
        if random.chance(2) {
            text.push('.');
            for _ in 0..1 + random.below(25) {
                text.push(char::from(b'0' + random.below(10) as u8));
            }
        }
        if random.chance(2) {
            text.push(if random.chance(2) { 'e' } else { 'E' });
            match random.below(3) {
                0 => text.push('-'),
                1 => text.push('+'),
                _ => {}
            }
            let most = if random.chance(8) { 400 } else { 30 };
            text.push_str(&random.below(most).to_string());
        }
        parse_case(&mut c, format!("[{text}]").as_bytes(), Surrogates::Replace, DEFAULT_MAX_DEPTH, format!("number text {i}: {text}"));
    }
    for text in [
        "2.2250738585072011e-308",
        "2.2250738585072012e-308",
        "4.9406564584124654e-324",
        "2.4703282292062327e-324",
        "2.4703282292062328e-324",
        "1.7976931348623157e308",
        "1.7976931348623158e308",
        "1.7976931348623159e308",
        "9007199254740993",
        "9007199254740992.5",
        "0.1000000000000000055511151231257827",
        "0.1000000000000000055511151231257826",
        "1e-400",
        "-1e-400",
        "1e400",
        "0e999",
        "-0",
        "-0.0e0",
    ] {
        parse_case(&mut c, format!("[{text}]").as_bytes(), Surrogates::Replace, DEFAULT_MAX_DEPTH, format!("number text {text}"));
    }
    let tally = c.check(&oracle);
    eprintln!("numbers: {} answers alike", tally[1]);
}
