//! The Rust litehtml against the fork itself (`oracle/lhdump`, built from litehtml/src as twidget builds it):
//! every record kind of tests/common/mod.rs on generated values, their edges and their random changes, with
//! the name table carried across a run in the C++ order. Skipped without `TWIDGET_ORACLE_DIR`;
//! `tools/gate.py` sets it.

mod common;

use std::collections::BTreeMap;

use common::*;
use twidget_litehtml::css_length::*;
use twidget_litehtml::css_tokenizer::*;

/// Cases for one lhdump run and how many answers each kind gave by status.
struct Run {
    lh: Lh,
    c: Comparison,
    kinds: BTreeMap<String, [usize; 2]>,
}

impl Run {
    fn new() -> Run {
        Run { lh: Lh::new(), c: Comparison::new(), kinds: BTreeMap::new() }
    }

    fn case(&mut self, kind: &str, op: u8, input: &[u8], what: impl Into<String>) {
        let answer = self.lh.answer(op, input);
        self.kinds.entry(kind.to_string()).or_default()[usize::from(answer.0 != 0)] += 1;
        self.c.case(&framed(op, input), answer, what);
    }

    fn check(self, name: &str) {
        let Some(oracle) = Oracle::find("lhdump") else { return };
        let total = self.c.len();
        self.c.check(&oracle);
        let kinds: Vec<String> = self.kinds.iter().map(|(k, [no, yes])| format!("{k} {yes}/{}", yes + no)).collect();
        eprintln!("{name}: {total} answers alike ({})", kinds.join(", "));
    }
}

fn token_bytes(t: &CssToken) -> Vec<u8> {
    let mut o = Out::default();
    o.token(t);
    o.take()
}

#[test]
fn names_alike() {
    let mut run = Run::new();
    run.case("table", b'N', b"", "the table at start");
    let mut random = Lcg::new(0x11_0001);
    let initial = run.lh.names.usage().names as u32;
    let known: Vec<Vec<u8>> = (0..initial).map(|i| run.lh.names.s(twidget_litehtml::StringId(i)).to_vec()).collect();
    for script in 0..3000 {
        let mut steps = Out::default();
        let n = 1 + random.below(12);
        steps.u32(n as u32);
        for _ in 0..n {
            let name: Vec<u8> = match random.below(6) {
                0 | 1 => known[random.below(known.len())].clone(),
                2 => known[random.below(known.len())].to_ascii_uppercase(),
                3 => format!("n{}", random.below(400)).into_bytes(),
                4 => any_text(&mut random),
                _ => pick(&mut random, &["", "*", "-", "--x", "border-color", "Border-Color", "\u{e9}", "px", "deg"]).as_bytes().to_vec(),
            };
            match random.below(4) {
                0 => {
                    steps.u8(b'i').str(&name);
                }
                1 => {
                    steps.u8(b'f').str(&name);
                }
                2 => {
                    let usage = run.lh.names.usage().names as u32;
                    steps.u8(b's').u32(random.below(usage as usize + 40) as u32 % usage);
                }
                _ => {
                    steps.u8(b'u');
                }
            }
        }
        // The ids `s` asks for must be in the table when the script runs: the generator above asks for those
        // already there, and `i` steps only add.
        run.case("script", b'n', &steps.take(), format!("name script {script}"));
        if script % 500 == 0 {
            run.case("table", b'N', b"", format!("the table after script {script}"));
        }
    }
    // Angles add their units to the table.
    for i in 0..2000 {
        let mut t = CssToken::with_number(DIMENSION, any_f32(&mut random), CssNumberType::css_number_integer);
        t.str = if random.chance(2) { any_unit(&mut random).to_vec() } else { format!("u{}", random.below(300)).into_bytes() };
        let input = [&[u8::from(random.chance(2))][..], &token_bytes(&t)].concat();
        run.case("angle", b'A', &input, format!("angle {i}"));
    }
    run.case("table", b'N', b"", "the table at the end");
    run.check("names");
}

#[test]
fn tables_alike() {
    let mut run = Run::new();
    run.case("tables", b't', b"", "the tables");
    let mut random = Lcg::new(0x11_0002);
    for (t, (name, entries)) in TABLES.iter().enumerate() {
        let mut texts: Vec<Vec<u8>> = entries.iter().map(|e| e.as_bytes().to_vec()).collect();
        texts.extend(entries.iter().map(|e| e.to_ascii_uppercase().into_bytes()));
        texts.extend([b"".to_vec(), b"none".to_vec(), b"auto ".to_vec(), b"\0".to_vec()]);
        for _ in 0..20 {
            let mut e = entries[random.below(entries.len())].as_bytes().to_vec();
            mutate(&mut random, &mut e);
            texts.push(e);
        }
        for (k, text) in texts.iter().enumerate() {
            let index = (k % (entries.len() + 3)) as u32;
            let input = [&[t as u8][..], &index.to_le_bytes(), text].concat();
            run.case("lookup", b'v', &input, format!("{name} {:?} at {index}", String::from_utf8_lossy(text)));
        }
    }
    for k in [53u8, 200, 255] {
        run.case("lookup", b'v', &[&[k][..], &0u32.to_le_bytes(), b"none"].concat(), format!("no table {k}"));
    }
    run.check("tables");
}

#[test]
fn numbers_alike() {
    let mut run = Run::new();
    let mut random = Lcg::new(0x11_0003);
    for i in 0..60000 {
        let mut text = any_number_text(&mut random);
        if i % 4 == 3 {
            mutate(&mut random, &mut text);
        }
        run.case("strtod", b'd', &text, format!("t_strtod {i}: {:?}", String::from_utf8_lossy(&text)));
        run.case("convert", b'c', &text, format!("convert_string_to_number {i}: {:?}", String::from_utf8_lossy(&text)));
    }
    run.check("numbers");
}

#[test]
fn pixels_alike() {
    let mut run = Run::new();
    let mut random = Lcg::new(0x11_0004);
    for i in 0..40000 {
        let a = any_f32(&mut random);
        let mut b =
            if random.chance(4) { a + [0.0, 5e-5, -5e-5, 1e-4, -1e-4, 2e-4, 9.9e-5][random.below(7)] } else { any_f32(&mut random) };
        // Two NaNs in one operation give the payload of the compiler's operand order: one at a time.
        if a.is_nan() && b.is_nan() {
            b = 1.0;
        }
        let k = match random.below(4) {
            0 => 0,
            1 => [1, -1, 2, 3, 100, i32::MAX, i32::MIN][random.below(7)],
            2 => random.next() as i32,
            _ => random.below(200) as i32 - 100,
        };
        let input = [a.to_bits().to_le_bytes(), b.to_bits().to_le_bytes(), k.to_le_bytes()].concat();
        run.case("pixel", b'p', &input, format!("pixel_t {i}: {a:e} {b:e} {k}"));
    }
    run.check("pixels");
}

#[test]
fn colors_alike() {
    let mut run = Run::new();
    let mut random = Lcg::new(0x11_0005);
    for i in 0..60000 {
        let which = random.below(4) as u8;
        let tok = match (which, random.below(10)) {
            (_, 0..=2) => any_token(&mut random, 0),
            (0, _) => hash_token(&mut random),
            (1, _) => color_function(&mut random, false),
            (2, _) => color_function(&mut random, true),
            _ => {
                let hsl = random.chance(2);
                color_function(&mut random, hsl)
            }
        };
        let input = [&[which][..], &token_bytes(&tok)].concat();
        let kind = ["hash", "rgb", "hsl", "function"][usize::from(which)];
        run.case(kind, b'C', &input, format!("colour {i} ({kind}): {tok:?}"));
    }
    for i in 0..20000 {
        let input = [any_f32(&mut random), any_f32(&mut random), any_f32(&mut random)].map(f32::to_bits).map(u32::to_le_bytes).concat();
        run.case("hsl_to_rgb", b'H', &input, format!("hsl_to_rgb {i}"));
    }
    for i in 0..20000 {
        let (x, lo, hi, value) = (any_f32(&mut random), any_f32(&mut random), any_f32(&mut random), any_f32(&mut random));
        let units = random.below(17) as u8;
        let max = if random.chance(2) {
            255.0
        } else if random.chance(2) {
            1.0
        } else {
            any_f32(&mut random)
        };
        let input =
            [&x.to_le_bytes()[..], &lo.to_le_bytes(), &hi.to_le_bytes(), &value.to_le_bytes(), &[units], &max.to_le_bytes()].concat();
        run.case("clamp", b'q', &input, format!("clamp {i}"));
    }
    for i in 0..10000 {
        let rgba = [random.next() as u8, random.next() as u8, random.next() as u8, random.next() as u8];
        let fraction = match random.below(4) {
            0 => [0.0, 0.33, 0.5, 1.0, -1.0, 2.0, f64::NAN, f64::INFINITY, -1e300, 1e300][random.below(10)],
            _ => f64::from(any_f32(&mut random)),
        };
        run.case("darken", b'D', &[&rgba[..], &fraction.to_le_bytes()].concat(), format!("darken {i}"));
        run.case("to_string", b'S', &rgba, format!("to_string {i}"));
    }
    for (name, _) in twidget_litehtml::web_color::DEF_COLORS {
        for text in [name.to_string(), name.to_ascii_lowercase(), name.to_ascii_uppercase(), format!("{name} "), name[1..].to_string()] {
            run.case("name", b'R', text.as_bytes(), format!("resolve_name {text:?}"));
        }
    }
    for i in 0..2000 {
        let text = any_text(&mut random);
        run.case("name", b'R', &text, format!("resolve_name {i}"));
    }
    run.check("colours");
}

fn kind_of(t: &CssToken) -> &'static str {
    match t.ty {
        NUMBER => "number",
        PERCENTAGE => "percentage",
        DIMENSION => "dimension",
        IDENT => "ident",
        HASH => "hash",
        CV_FUNCTION => "function",
        ROUND_BLOCK | SQUARE_BLOCK | CURLY_BLOCK => "block",
        _ => "other",
    }
}

#[test]
fn lengths_alike() {
    let mut run = Run::new();
    let mut random = Lcg::new(0x11_0006);
    let realistic = [
        f_length,
        f_length | f_positive,
        f_length_percentage,
        f_length_percentage | f_positive,
        f_number,
        f_number | f_positive,
        f_integer,
        f_number | f_percentage,
        f_length | f_number,
        f_length_percentage | f_number,
        f_percentage,
        0,
    ];
    for i in 0..60000 {
        let words = if random.chance(3) { 255 } else { random.below(TABLES.len()) as u8 };
        let tok = match random.below(20) {
            0..=5 => {
                let mut d = dimension(&mut random);
                if random.chance(3) {
                    d.n.number = random.below(400) as f32 / 4.0 - 20.0;
                }
                d
            }
            6..=8 => number_token(&mut random, NUMBER),
            9..=10 => number_token(&mut random, PERCENTAGE),
            11..=12 => {
                let entries = TABLES.get(usize::from(words)).map_or(&["none", "auto"][..], |t| t.1);
                let word = entries[random.below(entries.len())];
                let word = if random.chance(4) { word.to_ascii_uppercase() } else { word.to_string() };
                CssToken::with_str(IDENT, word.as_bytes())
            }
            13 | 14 => calc_function(&mut random, 0),
            15..=17 => math_function(&mut random, 0),
            _ => any_token(&mut random, 0),
        };
        let start = random.below(4) as u8;
        let options = if tok.ty == CV_FUNCTION && !random.chance(4) {
            f_length_percentage | if random.chance(2) { f_positive } else { 0 }
        } else if random.chance(4) {
            random.next() as i32
        } else {
            realistic[random.below(realistic.len())]
        };
        let input = [&[start][..], &options.to_le_bytes(), &[words], &token_bytes(&tok)].concat();
        let kind = kind_of(&tok);
        run.case(kind, b'L', &input, format!("length {i} from {start}, options {options:#x}, table {words}: {tok:?}"));
    }
    run.check("lengths");
}

#[test]
fn angles_alike() {
    let mut run = Run::new();
    let mut random = Lcg::new(0x11_0009);
    for i in 0..20000 {
        let tok = match random.below(4) {
            0 => any_token(&mut random, 0),
            1 => {
                let ty = if random.chance(2) { NUMBER } else { PERCENTAGE };
                number_token(&mut random, ty)
            }
            _ => {
                let mut d = dimension(&mut random);
                d.str = pick(&mut random, &["deg", "grad", "rad", "turn", "DEG", "Turn", "px", ""]).as_bytes().to_vec();
                d
            }
        };
        let percents = random.chance(2);
        let input = [&[u8::from(percents)][..], &token_bytes(&tok)].concat();
        run.case(kind_of(&tok), b'A', &input, format!("angle {i} ({percents}): {tok:?}"));
    }
    run.check("angles");
}

#[test]
fn markers_alike() {
    let mut run = Run::new();
    let mut random = Lcg::new(0x11_0007);
    for i in 0..30000 {
        let value = match random.below(4) {
            0 => [0, -1, 1, 26, 27, 52, 702, 703, 3999, 4000, 18278, 18279, i32::MAX, i32::MIN, 24, 25, 576, 577][random.below(18)],
            1 => random.next() as i32,
            _ => random.below(5000) as i32 - 100,
        };
        let style = (i % 5) as u8;
        run.case("marker", b'k', &[&[style][..], &value.to_le_bytes()].concat(), format!("marker {style} of {value}"));
    }
    run.check("markers");
}

#[test]
fn strings_alike() {
    let mut run = Run::new();
    let mut random = Lcg::new(0x11_0008);
    for i in 0..20000 {
        let text = any_utf8(&mut random);
        run.case("utf8", b'u', &text, format!("utf8 {i}: {}", hex(&text)));
    }
    for i in 0..10000 {
        let mut input = Vec::new();
        for _ in 0..random.below(8) {
            let cp = match random.below(4) {
                0 => [0, 0x7f, 0x80, 0x7ff, 0x800, 0xd7ff, 0xd800, 0xdfff, 0xe000, 0xfffd, 0xffff, 0x10000, 0x10ffff, 0x110000, u32::MAX]
                    [random.below(15)],
                1 => random.next(),
                _ => random.below(0x110000) as u32,
            };
            input.extend_from_slice(&cp.to_le_bytes());
        }
        run.case("append_char", b'a', &input, format!("append_char {i}"));
    }
    for i in 0..20000 {
        let delims = pick(&mut random, &[" ", ",", ";", " \t\r\n\x0c", "", ",;", "("]).as_bytes();
        let preserve = pick(&mut random, &["", "", "/", ",", "()"]).as_bytes();
        let quote = pick(&mut random, &["\"", "\"", "\"'", "", "(", "([{", "'"]).as_bytes();
        let text = any_text(&mut random);
        let mut input = Out::default();
        input.str(delims).str(preserve).str(quote);
        input.s.extend_from_slice(&text);
        run.case("split", b's', &input.take(), format!("split_string {i}: {:?}", String::from_utf8_lossy(&text)));
    }
    for i in 0..20000 {
        let a = any_text(&mut random);
        let b = if random.chance(3) { a.to_ascii_uppercase() } else { any_text(&mut random) };
        let mut input = Out::default();
        input.str(&a).str(&b);
        run.case(
            "helpers",
            b'm',
            &input.take(),
            format!("helpers {i}: {:?} {:?}", String::from_utf8_lossy(&a), String::from_utf8_lossy(&b)),
        );
    }
    run.case("url classes", b'x', b"", "the URL classes");
    run.check("strings");
}

#[test]
fn switches_are_settings() {
    // A setting gets no answer and changes nothing these stages read.
    let Some(oracle) = Oracle::find("lhdump") else { return };
    let lh = Lh::new();
    let mut c = Comparison::new();
    c.setting(&framed(b'E', b"LH_NO_LAYOUT_CACHE=1"));
    lh.case(&mut c, b'c', b"1.5e3", "a number after a setting");
    c.check(&oracle);
}
