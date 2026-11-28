//! The Rust CSS tokenizer and normalizer against the fork's (`oracle/lhdump`): every token with every field
//! (type or code point, number bits and type flag, hash type, string, representation), the component
//! values `normalize` makes of them with and without whitespace and their representations, and colours
//! and lengths read from text. The texts: the style text of every widget of widget-sandbox's corpus and
//! its declarations, litehtml's master CSS, the renderer's UA rules and the profile's UA layer, generated
//! CSS of every token kind and its broken forms, and random changes of all of these. Each run counts the
//! token kinds it reaches. Skipped without `TWIDGET_ORACLE_DIR`; `tools/gate.py` sets it.

mod common;

use std::collections::BTreeMap;

use common::*;
use twidget_litehtml::css_length::*;
use twidget_litehtml::css_parser::{f_componentize, normalize};
use twidget_litehtml::css_tokenizer::*;

/// One lhdump run and what its tokens reached.
struct Run {
    lh: Lh,
    c: Comparison,
    kinds: BTreeMap<&'static str, usize>,
    results: BTreeMap<String, [usize; 2]>,
    repaired: usize,
}

fn kind_name(t: &CssToken) -> &'static str {
    match t.ty {
        WHITESPACE => "whitespace",
        IDENT => "ident",
        FUNCTION => "function",
        AT_KEYWORD => "at-keyword",
        HASH if t.hash_type == CssHashType::css_hash_id => "hash id",
        HASH => "hash unrestricted",
        STRING => "string",
        BAD_STRING => "bad string",
        URL => "url",
        BAD_URL => "bad url",
        NUMBER if t.n.number_type == CssNumberType::css_number_integer => "number integer",
        NUMBER => "number",
        PERCENTAGE => "percentage",
        DIMENSION => "dimension",
        CDO => "CDO",
        CDC => "CDC",
        CV_FUNCTION => "cv function",
        CURLY_BLOCK | ROUND_BLOCK | SQUARE_BLOCK => "block",
        0x3a | 0x3b | 0x2c | 0x28 | 0x29 | 0x5b | 0x5d | 0x7b | 0x7d => ":;,()[]{}",
        c if c >= 0x80 => "delim past ASCII",
        _ => "delim",
    }
}

impl Run {
    fn new() -> Run {
        Run { lh: Lh::new(), c: Comparison::new(), kinds: BTreeMap::new(), results: BTreeMap::new(), repaired: 0 }
    }

    /// The text as the comparison may give it to the tokenizer (see `without_overlong_ascii`).
    fn tokenizable(&mut self, text: &[u8]) -> Vec<u8> {
        let (text, changed) = without_overlong_ascii(text);
        self.repaired += usize::from(changed);
        text
    }

    fn count(&mut self, list: &[CssToken]) {
        for t in list {
            *self.kinds.entry(kind_name(t)).or_default() += 1;
            self.count(&t.value);
        }
    }

    fn case(&mut self, op: u8, input: &[u8], what: impl Into<String>) {
        let answer = self.lh.answer(op, input);
        self.c.case(&framed(op, input), answer, what);
    }

    /// The text through the tokenizer, the filter, the four normalizings and the list checks.
    fn text(&mut self, text: &[u8], what: &str) {
        let text = &self.tokenizable(text);
        self.case(b'T', text, format!("tokens of {what}"));
        self.count(&tokenize(text));
        self.case(b'F', text, format!("filtered {what}"));
        for (options, keep) in [(0u8, 0u8), (1, 0), (2, 0), (3, 0), (3, 1)] {
            let input = [&[options, keep][..], text].concat();
            self.case(b'Z', &input, format!("normalized {options}/{keep} {what}"));
        }
        self.count(&normalize(text, f_componentize, None));
        self.case(b'P', text, format!("lists of {what}"));
    }

    /// A declaration's value as a colour and as lengths.
    fn value(&mut self, value: &[u8], what: &str) {
        let value = &self.tokenizable(value);
        let colour = self.lh.answer(b'K', value);
        self.results.entry("colour from text".into()).or_default()[usize::from(colour.0)] += 1;
        self.c.case(&framed(b'K', value), colour, format!("colour of {what}"));
        for (options, table) in [(f_length_percentage, 255u8), (f_length | f_positive, 14), (f_number | f_percentage, 255), (f_integer, 0)]
        {
            let input = [&options.to_le_bytes()[..], &[table], value].concat();
            let length = self.lh.answer(b'G', &input);
            self.results.entry("length from text".into()).or_default()[usize::from(length.0)] += 1;
            self.c.case(&framed(b'G', &input), length, format!("length {options:#x} of {what}"));
        }
    }

    fn check(self, name: &str) {
        let Some(oracle) = Oracle::find("lhdump") else { return };
        let total = self.c.len();
        self.c.check(&oracle);
        let kinds: Vec<String> = self.kinds.iter().map(|(k, n)| format!("{k} {n}")).collect();
        let results: Vec<String> = self.results.iter().map(|(k, [no, yes])| format!("{k} {yes}/{}", yes + no)).collect();
        eprintln!(
            "{name}: {total} answers alike ({} texts with an overlong ASCII form made 0xFF); tokens: {}; {}",
            self.repaired,
            kinds.join(", "),
            results.join(", ")
        );
    }
}

#[test]
fn master_css_is_the_forks() {
    let Some(oracle) = Oracle::find("lhdump") else { return };
    let lh = Lh::new();
    let mut c = Comparison::new();
    lh.case(&mut c, b'M', b"", "litehtml's master_css as the test reads it from master_css.h");
    c.check(&oracle);
}

#[test]
fn corpus_tokens_alike() {
    let styles = corpus_styles();
    assert!(styles.len() > 400, "found {} style texts in widget-sandbox's corpus", styles.len());
    let mut run = Run::new();
    let mut random = Lcg::new(0x11_1001);
    let mut values = 0;
    for (i, style) in styles.iter().enumerate() {
        run.text(style, &format!("style {i}"));
        for (name, value) in declarations(style) {
            values += 1;
            run.value(&value, &format!("{} in style {i}", String::from_utf8_lossy(&name)));
        }
        for k in 0..3 {
            let mut changed = style.clone();
            let edits = mutate(&mut random, &mut changed);
            let changed = run.tokenizable(&changed);
            run.case(b'T', &changed, format!("tokens of style {i} changed {k}{edits}"));
            run.case(b'Z', &[&[3u8, 0][..], &changed].concat(), format!("normalized style {i} changed {k}{edits}"));
        }
    }
    eprintln!("corpus: {} style texts, {values} declarations", styles.len());
    run.check("corpus");
}

#[test]
fn master_tokens_alike() {
    let master = master_css();
    let extra = ua_extra();
    let ua = ua_stylesheet();
    let whole = [&master[..], &extra, &ua].concat();
    let mut run = Run::new();
    let mut random = Lcg::new(0x11_1002);
    for (name, text) in [("master_css", &master), ("UA_EXTRA", &extra), ("the profile's UA layer", &ua), ("the renderer's master", &whole)]
    {
        run.text(text, name);
        for (k, rule) in text.split(|c| *c == b'}').enumerate() {
            run.text(rule, &format!("rule {k} of {name}"));
            for (prop, value) in declarations(rule) {
                run.value(&value, &format!("{} in rule {k} of {name}", String::from_utf8_lossy(&prop)));
            }
        }
        for k in 0..1000 {
            let mut changed = text.clone();
            let edits = mutate(&mut random, &mut changed);
            let changed = run.tokenizable(&changed);
            run.case(b'T', &changed, format!("tokens of {name} changed {k}{edits}"));
            if k % 4 == 0 {
                run.case(b'Z', &[&[3u8, 1][..], &changed].concat(), format!("normalized {name} changed {k}{edits}"));
            }
        }
    }
    run.check("master");
}

#[test]
fn generated_tokens_alike() {
    let mut run = Run::new();
    let mut random = Lcg::new(0x11_1003);
    for i in 0..60000 {
        let mut text = any_css(&mut random);
        if i % 5 == 4 {
            mutate(&mut random, &mut text);
        }
        let text = run.tokenizable(&text);
        if i % 6 == 0 {
            run.text(&text, &format!("generated {i}: {:?}", String::from_utf8_lossy(&text)));
        } else {
            run.case(b'T', &text, format!("tokens of generated {i}: {:?}", String::from_utf8_lossy(&text)));
            run.count(&tokenize(&text));
            let options = random.below(4) as u8;
            let keep = random.below(2) as u8;
            run.case(b'Z', &[&[options, keep][..], &text].concat(), format!("normalized {options}/{keep} generated {i}"));
        }
    }
    run.check("generated");
}

#[test]
fn values_from_text_alike() {
    let mut run = Run::new();
    let mut random = Lcg::new(0x11_1004);
    let mut texts: Vec<Vec<u8>> = Vec::new();
    for (name, rgb) in twidget_litehtml::web_color::DEF_COLORS {
        texts.extend([name.as_bytes().to_vec(), name.to_ascii_lowercase().into_bytes(), rgb.as_bytes().to_vec()]);
    }
    texts.extend(
        ["currentColor", "CURRENTCOLOR", "--currentcolor", "transparent ", " red", "red blue", "#ABC", "#abcd", "#aabbccdd", "#aabbcc80 "]
            .map(|t| t.as_bytes().to_vec()),
    );
    for _ in 0..30000 {
        let text = match random.below(8) {
            0 => {
                let hex: String =
                    (0..[3, 4, 6, 8, 5][random.below(5)]).map(|_| "0123456789abcdefABCDEFg".as_bytes()[random.below(23)] as char).collect();
                format!("#{hex}")
            }
            1 | 2 => {
                let f = pick(&mut random, &["rgb", "rgba", "hsl", "hsla", "RGB", "Hsl"]);
                let n = 3 + random.below(2);
                let comma = random.chance(2);
                let parts: Vec<String> = (0..n)
                    .map(|k| {
                        let unit = pick(&mut random, &["", "", "%", "deg", "turn", "rad", "grad", "px"]);
                        let number = pick(
                            &mut random,
                            &["0", "1", "50", "100", "255", "256", "-5", "0.5", "1e2", "1e39", "0e999", ".5", "none", "360", "720"],
                        );
                        let sep = if k == 3 && !comma { " / " } else { "" };
                        format!("{sep}{number}{unit}")
                    })
                    .collect();
                format!("{f}({})", parts.join(if comma { ", " } else { " " }))
            }
            3 => {
                let (name, _) = twidget_litehtml::web_color::DEF_COLORS[random.below(149)];
                name.to_string()
            }
            4 => {
                let unit = pick(&mut random, &["px", "em", "%", "", "rem", "vw", "PX", "deg", "x"]);
                format!("{}{unit}", pick(&mut random, &["0", "1", "-2", "1.5", "1e3", "+.5", "10", "1e39", "0e999"]))
            }
            5 => {
                let a = pick(&mut random, &["10px", "50%", "2em", "3", "1vw", "-4px"]);
                let b = pick(&mut random, &["10px", "50%", "2em", "3", "1vw", "(1px + 2%)"]);
                let op = pick(&mut random, &[" + ", " - ", " * ", " / ", "+", ", "]);
                let f = pick(&mut random, &["calc", "min", "max", "clamp", "CALC"]);
                format!("{f}({a}{op}{b})")
            }
            _ => String::from_utf8_lossy(&any_css(&mut random)).into_owned(),
        };
        let mut text = text.into_bytes();
        if random.chance(6) {
            mutate(&mut random, &mut text);
        }
        texts.push(text);
    }
    for (i, text) in texts.iter().enumerate() {
        run.value(text, &format!("value {i}: {:?}", String::from_utf8_lossy(text)));
    }
    run.check("values");
}

#[test]
fn whitespace_kept_by_list_length_alike() {
    // A list of up to 50 tokens decides each whitespace against what is left of it, a longer one against
    // the tokens as they were: media queries' `< /**/ /**/ =` keeps a space in a short block, none in a long.
    let mut run = Run::new();
    let long = [&b"(a < /**/ /**/ = b "[..], &b"x ".repeat(30), b")"].concat();
    for text in [&b"(a < /**/ /**/ = b)"[..], &long] {
        for keep in [0u8, 1] {
            run.case(b'Z', &[&[3u8, keep][..], text].concat(), format!("keep {keep} in {:?}", String::from_utf8_lossy(text)));
        }
    }
    let short = normalize(b"(a < /**/ /**/ = b)", 3, keeper(1));
    let long = normalize(&long, 3, keeper(1));
    assert_eq!(get_repr(&short[0].value, 0, 3, false), b"a< ");
    assert_eq!(get_repr(&long[0].value, 0, 3, false), b"a<=");
    run.check("whitespace");
}
