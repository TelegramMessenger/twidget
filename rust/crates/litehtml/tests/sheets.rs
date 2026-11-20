//! The Rust stylesheet parser, walk and media queries against the fork's (`oracle/lhdump`): every rule of a
//! sheet with its prelude, its block and what the block holds (declarations and nested rules, or the rules of
//! an at-rule's block), the declarations as `style::add` takes them, every media query list's structure with
//! its values converted and its answer on a grid of media features, and the walk of whole sheets: each
//! selector's order, `@layer` flag, rule and `@media` lists, each list over the grid, the sheet read as it is
//! and with the renderer's light and dark colour-scheme rewrite. The walk's selector lists and `@supports`
//! declarations are later stages' (L4, L5): the run asks lhdump for them first. The sheets: widget-sandbox's
//! corpus, litehtml's master CSS and the renderer's UA layers, generated sheets of every rule kind and media
//! query lists of every syntax, and random changes of them. Skipped without `TWIDGET_ORACLE_DIR`.

mod common;

use std::collections::BTreeMap;

use common::*;
use twidget_litehtml::css_parser::{f_componentize, normalize, parse_stylesheet};
use twidget_litehtml::css_tokenizer::*;
use twidget_litehtml::html::lowcase;
use twidget_litehtml::stylesheet::{RawRule, RuleType};
use twidget_litehtml::types::MediaFeatures;
use twidget_litehtml::{Names, Px};

/// One lhdump run of stylesheet cases.
struct SheetRun {
    lh: Lh,
    later: Later,
    grid: Vec<MediaFeatures>,
    c: Comparison,
    sheets: Vec<Vec<u8>>,
    cases: Vec<(u8, Vec<u8>, String)>,
    counts: BTreeMap<String, usize>,
}

impl SheetRun {
    fn new() -> SheetRun {
        SheetRun {
            lh: Lh::new(),
            later: Later::default(),
            grid: media_grid(),
            c: Comparison::new(),
            sheets: Vec::new(),
            cases: Vec::new(),
            counts: BTreeMap::new(),
        }
    }

    fn count(&mut self, what: impl Into<String>, n: usize) {
        *self.counts.entry(what.into()).or_default() += n;
    }

    /// A case to answer once the run has learnt what the later stages say of its sheets.
    fn case(&mut self, op: u8, input: Vec<u8>, what: impl Into<String>) {
        if op == b'w' {
            self.sheets.push(input[1..].to_vec());
        }
        self.cases.push((op, input, what.into()));
    }

    /// A sheet: its rules top-level and not, and its walk as it is and in both colour schemes.
    fn sheet(&mut self, text: &[u8], what: &str) {
        for top_level in [1u8, 0] {
            self.case(b'r', [&[top_level][..], text].concat(), format!("rules {top_level} of {what}"));
        }
        for scheme in 0..3u8 {
            self.case(b'w', [&[scheme][..], text].concat(), format!("walk {scheme} of {what}"));
        }
        let rules = parse_stylesheet(text, true);
        self.media_preludes(&rules, what);
    }

    /// The `@media` preludes of the rules, at any depth, as media query lists of their own.
    fn media_preludes(&mut self, rules: &[RawRule], what: &str) {
        for rule in rules {
            if rule.ty == RuleType::at && lowcase(&rule.name) == b"media" {
                let text = get_repr(&rule.prelude, 0, -1, false);
                let (text, _) = without_overlong_ascii(&text);
                self.case(b'e', text, format!("media of {what}"));
            }
            if rule.block.ty == CURLY_BLOCK && rule.ty == RuleType::at {
                let inner = twidget_litehtml::css_parser::parse_stylesheet_tokens(&rule.block.value, false);
                self.media_preludes(&inner, what);
            }
        }
    }

    fn check(mut self, name: &str) {
        let Some(oracle) = Oracle::find("lhdump") else { return };
        self.later.learn(&oracle, &self.sheets);
        self.c.setting(&grid_setting(&self.grid));
        let cases = std::mem::take(&mut self.cases);
        for (op, input, what) in cases {
            let answer = self.lh.sheet_answer(&self.later, &self.grid, op, &input);
            self.tally(op, &input, &answer);
            self.c.case(&framed(op, &input), answer, what);
        }
        let total = self.c.len();
        let learnt = (self.later.selector_counts.len(), self.later.supported.len());
        self.c.check(&oracle);
        let counts: Vec<String> = self.counts.iter().map(|(k, n)| format!("{k} {n}")).collect();
        eprintln!("{name}: {total} answers alike ({} preludes and {} declarations learnt); {}", learnt.0, learnt.1, counts.join(", "));
    }

    fn tally(&mut self, op: u8, input: &[u8], answer: &(u8, Vec<u8>)) {
        match op {
            b'r' if input[0] == 1 => {
                let rules = parse_stylesheet(&input[1..], true);
                self.tally_rules(&rules);
            }
            b'w' => {
                let n = u32::from_le_bytes(answer.1[..4].try_into().expect("a count")) as usize;
                self.count("selectors walked", n);
                let mut layered = 0;
                let mut in_media = 0;
                for k in 0..n {
                    let at = 4 + k * 13;
                    layered += usize::from(answer.1[at + 4]);
                    in_media += usize::from(i32::from_le_bytes(answer.1[at + 9..at + 13].try_into().expect("a list")) >= 0);
                }
                self.count("selectors layered", layered);
                self.count("selectors under @media", in_media);
                let lists_at = 4 + n * 13;
                let lists = u32::from_le_bytes(answer.1[lists_at..lists_at + 4].try_into().expect("a count")) as usize;
                self.count("media lists", lists);
                let used = answer.1[lists_at + 4..].iter().filter(|b| **b == 1).count();
                self.count("media lists × grid used", used);
                self.count("media lists × grid unused", lists * self.grid.len() - used);
            }
            b'e' => self.count("media query lists", 1),
            b'B' => self.count("declaration blocks", 1),
            _ => {}
        }
    }

    fn tally_rules(&mut self, rules: &[RawRule]) {
        for rule in rules {
            if rule.ty == RuleType::qualified {
                self.count("style rules", 1);
                let mut decls = Vec::new();
                let mut nested = Vec::new();
                twidget_litehtml::css_parser::CssParser::new(rule.block.value.clone())
                    .consume_style_block_contents(&mut decls, &mut nested);
                self.count("declarations", decls.len());
                self.count("!important", decls.iter().filter(|d| d.important).count());
                self.count("nested rules", nested.len());
            } else {
                let name = String::from_utf8_lossy(&lowcase(&rule.name)).into_owned();
                let known = ["media", "supports", "layer", "import", "charset"].contains(&name.as_str());
                self.count(format!("@{}", if known { &name } else { "other" }), 1);
                if rule.block.ty == CURLY_BLOCK {
                    let inner = twidget_litehtml::css_parser::parse_stylesheet_tokens(&rule.block.value, false);
                    self.tally_rules(&inner);
                }
            }
        }
    }
}

#[test]
fn corpus_sheets_alike() {
    let styles = corpus_styles();
    let mut run = SheetRun::new();
    let mut random = Lcg::new(0x11_2001);
    let mut blocks = 0;
    for (i, style) in styles.iter().enumerate() {
        // A style attribute is a declaration list; a <style> block is a sheet.
        let (style, _) = without_overlong_ascii(style);
        run.case(b'B', style.clone(), format!("declarations of style {i}"));
        if !style.contains(&b'{') {
            continue;
        }
        blocks += 1;
        run.sheet(&style, &format!("style {i}"));
        for k in 0..3 {
            let mut changed = style.clone();
            let edits = mutate(&mut random, &mut changed);
            run.case(b'r', [&[1u8][..], &changed].concat(), format!("rules of style {i} changed {k}{edits}"));
            run.case(b'w', [&[k as u8][..], &changed].concat(), format!("walk of style {i} changed {k}{edits}"));
        }
    }
    assert!(blocks > 100, "found {blocks} style blocks in widget-sandbox's corpus");
    eprintln!("corpus: {} style texts, {blocks} sheets", styles.len());
    run.check("corpus sheets");
}

#[test]
fn master_sheets_alike() {
    let master = master_css();
    let extra = ua_extra();
    let ua = ua_stylesheet();
    let whole = [&master[..], &extra, &ua].concat();
    let mut run = SheetRun::new();
    let mut random = Lcg::new(0x11_2002);
    for (name, text) in [("master_css", &master), ("UA_EXTRA", &extra), ("the profile's UA layer", &ua), ("the renderer's master", &whole)]
    {
        run.sheet(text, name);
        for rule in parse_stylesheet(text, true) {
            if rule.ty == RuleType::qualified {
                let block = get_repr(&rule.block.value, 0, -1, false);
                run.case(b'B', block, format!("declarations of a rule of {name}"));
            }
        }
        for k in 0..500 {
            let mut changed = text.clone();
            let edits = mutate(&mut random, &mut changed);
            run.case(b'r', [&[1u8][..], &changed].concat(), format!("rules of {name} changed {k}{edits}"));
            run.case(b'w', [&[(k % 3) as u8][..], &changed].concat(), format!("walk of {name} changed {k}{edits}"));
        }
    }
    run.check("master sheets");
}

#[test]
fn generated_sheets_alike() {
    let mut run = SheetRun::new();
    let mut random = Lcg::new(0x11_2003);
    for i in 0..12000 {
        let mut text = any_stylesheet(&mut random, 0).into_bytes();
        if i % 4 == 3 {
            mutate(&mut random, &mut text);
        }
        if i % 3 == 0 {
            run.sheet(&text, &format!("generated {i}: {:?}", String::from_utf8_lossy(&text)));
        } else {
            run.case(b'r', [&[1u8][..], &text].concat(), format!("rules of generated {i}: {:?}", String::from_utf8_lossy(&text)));
            let scheme = random.below(3) as u8;
            run.case(
                b'w',
                [&[scheme][..], &text].concat(),
                format!("walk {scheme} of generated {i}: {:?}", String::from_utf8_lossy(&text)),
            );
        }
    }
    run.check("generated sheets");
}

#[test]
fn media_queries_alike() {
    let mut run = SheetRun::new();
    let mut random = Lcg::new(0x11_2004);
    for i in 0..30000 {
        let mut text = any_media_query_list(&mut random).into_bytes();
        if i % 5 == 4 {
            mutate(&mut random, &mut text);
        }
        let (text, _) = without_overlong_ascii(&text);
        run.case(b'e', text.clone(), format!("media query list {i}: {:?}", String::from_utf8_lossy(&text)));
        if i % 10 == 0 {
            let sheet = [&b"@media "[..], &text, b"{a{b:c}} d{e:f}"].concat();
            run.case(b'w', [&[0u8][..], &sheet].concat(), format!("walk of media query list {i}"));
        }
    }
    run.check("media queries");
}

#[test]
fn declarations_and_values_alike() {
    let mut run = SheetRun::new();
    let mut random = Lcg::new(0x11_2005);
    for i in 0..20000 {
        let text = any_stylesheet(&mut random, 3);
        let block = match text.find('{') {
            Some(at) if random.chance(2) => text[at + 1..].to_string(),
            _ => text,
        };
        let (block, _) = without_overlong_ascii(block.as_bytes());
        run.case(b'B', block, format!("declarations {i}"));
    }
    for i in 0..10000 {
        let tok = match random.below(4) {
            0 => any_token(&mut random, 0),
            1 => {
                let mut t = CssToken::with_str(URL, pick(&mut random, &[" a.css ", "x", "", "\t"]).as_bytes());
                t.repr = b"url(x)".to_vec();
                t
            }
            _ => {
                let mut f = CssToken::with_str(CV_FUNCTION, pick(&mut random, &["url", "URL", "src", "image"]).as_bytes());
                let mut s = CssToken::with_str(STRING, pick(&mut random, &[" a.css", "b ", ""]).as_bytes());
                s.repr = b"'s'".to_vec();
                f.value = if random.chance(4) { vec![s.clone(), CssToken::new(WHITESPACE)] } else { vec![s] };
                f
            }
        };
        let mut o = Out::default();
        o.token(&tok);
        run.case(b'U', o.take(), format!("url {i}: {tok:?}"));
    }
    for i in 0..10000 {
        let mut text = match random.below(5) {
            0 => [&b"\xef\xbb\xbf"[..], &any_utf8(&mut random)].concat(),
            1 => {
                let units: Vec<u8> = (0..random.below(12))
                    .flat_map(|_| {
                        let u = [0x0041u16, 0x00e9, 0xd83d, 0xde00, 0xdc00, 0xd800, 0x0000, 0xfffd, 0x2028][random.below(9)];
                        u.to_le_bytes()
                    })
                    .collect();
                [&b"\xff\xfe"[..], &units].concat()
            }
            2 => {
                let units: Vec<u8> = (0..random.below(12))
                    .flat_map(|_| [0x0041u16, 0xd83d, 0xde00, 0xdbff, 0x0a0a][random.below(5)].to_be_bytes())
                    .collect();
                [&b"\xfe\xff"[..], &units].concat()
            }
            _ => any_utf8(&mut random),
        };
        if random.chance(5) {
            text.push(random.next() as u8);
        }
        run.case(b'X', text.clone(), format!("decode {i}: {}", hex(&text)));
    }
    for i in 0..20000 {
        let tok = match random.below(3) {
            0 => math_function(&mut random, 0),
            _ => {
                let mut d = dimension(&mut random);
                if random.chance(2) {
                    d.n.number = random.below(400) as f32 / 4.0 - 20.0;
                }
                d
            }
        };
        let mut o = Out::default();
        o.token(&tok);
        run.case(b'V', o.take(), format!("units {i}: {tok:?}"));
    }
    run.check("declarations and values");
}

#[test]
fn color_scheme_rewrite_alike() {
    let mut run = SheetRun::new();
    let mut random = Lcg::new(0x11_2006);
    for style in corpus_styles() {
        for dark in [0u8, 1] {
            run.case(b'o', [&[dark][..], &style].concat(), "rewrite of a corpus style");
        }
    }
    for i in 0..20000 {
        let mut text = match random.below(3) {
            0 => any_stylesheet(&mut random, 2).into_bytes(),
            _ => {
                let pieces = [
                    "prefers-color-scheme",
                    "PREFERS-COLOR-SCHEME",
                    "(prefers-color-scheme: dark)",
                    "(prefers-color-scheme)",
                    ": light",
                    ":dark",
                    "\"",
                    "'",
                    "/*",
                    "*/",
                    "\\",
                    "url(",
                    ")",
                    "x-",
                    "-",
                    " ",
                    "\n",
                    "a",
                    "prefers-color-schemes",
                    "@media ",
                    "{",
                    "}",
                ];
                let n = random.below(10);
                let mut text = String::new();
                for _ in 0..n {
                    text.push_str(pick(&mut random, &pieces));
                }
                text.into_bytes()
            }
        };
        if random.chance(4) {
            mutate(&mut random, &mut text);
        }
        let dark = random.below(2) as u8;
        run.case(b'o', [&[dark][..], &text].concat(), format!("rewrite {i}: {:?}", String::from_utf8_lossy(&text)));
    }
    run.check("colour-scheme rewrite");
}

#[test]
fn declarations_keep_their_parts() {
    let mut decls = Vec::new();
    let mut rules = Vec::new();
    let tokens = normalize(
        b"  Color : red !IMPORTANT ; --Var:{a} ;x;@media print{}; & b{c:d} 1px; margin: 0 auto ! important",
        f_componentize,
        None,
    );
    twidget_litehtml::css_parser::CssParser::new(tokens).consume_style_block_contents(&mut decls, &mut rules);
    let names: Vec<&[u8]> = decls.iter().map(|d| &d.name[..]).collect();
    assert_eq!(names, [&b"Color"[..], b"--Var", b"margin"]);
    // `!` must touch `important`: with whitespace between them the fork keeps both in the value.
    assert!(decls[0].important && !decls[2].important);
    assert_eq!(get_repr(&decls[2].value, 0, -1, false), b"0 auto ! important");
    assert_eq!(rules.iter().map(|r| r.ty).collect::<Vec<_>>(), [RuleType::at, RuleType::qualified]);
}

fn media(text: &str) -> twidget_litehtml::media_query::MediaQueryList {
    let lh = Lh::new();
    let later = Later::default();
    let sheets = Sheets { lh: &lh, later: &later, container: TestContainer };
    use twidget_litehtml::stylesheet::StyleSheetHost;
    twidget_litehtml::media_query::parse_media_query_list_text(text.as_bytes(), &sheets.media_document())
}

#[test]
fn media_features_take_their_values_in_px_and_dpi() {
    use twidget_litehtml::media_query::{MediaInParens, OP_GE, OP_LE, OP_LT};
    let feature = |text: &str| match &media(text).m_queries[0].m_conditions[0].m_conditions[0] {
        MediaInParens::Feature(f) => Some((f.name.clone(), f.value, f.op, f.value2, f.op2)),
        _ => None,
    };
    assert_eq!(feature("(min-width: 40em)"), Some((b"width".to_vec(), 640.0, OP_GE, 0.0, 0)));
    assert_eq!(feature("(max-height: 50vh)"), Some((b"height".to_vec(), 333.5, OP_LE, 0.0, 0)));
    assert_eq!(feature("(400px < width <= 1in)"), Some((b"width".to_vec(), 400.0, OP_LT, 96.0, OP_LE)));
    assert_eq!(feature("(aspect-ratio: 16/9)").map(|f| f.1), Some(16.0 / 9.0));
    // The fork refuses the resolution units it lists and takes any other number as dpi.
    assert_eq!(feature("(min-resolution: 2dppx)"), None);
    assert_eq!(feature("(min-resolution: 2foo)").map(|f| f.1), Some(2.0));
    assert_eq!(feature("(resolution: infinite)").map(|f| f.1), Some(f32::INFINITY));
}

#[test]
fn media_queries_answer_in_three_values() {
    use twidget_litehtml::media_query::Trilean;
    let names = Names::new();
    let narrow = MediaFeatures { ty: 3, width: Px(320.0), height: Px(480.0), ..MediaFeatures::default() };
    let answer = |text: &str| media(text).m_queries.iter().map(|q| q.check(&names, &narrow)).collect::<Vec<_>>();
    assert_eq!(answer("screen and (max-width: 600px), print"), [Trilean::True, Trilean::False]);
    assert_eq!(answer("(hover) or (width < 1px)"), [Trilean::Unknown]);
    assert_eq!(answer("not (hover)"), [Trilean::Unknown]);
    assert_eq!(answer("tv, not tv, bogus"), [Trilean::False, Trilean::True, Trilean::False]);
    assert_eq!(answer("(orientation: portrait)"), [Trilean::True]);
    assert!(media("").empty() && media("screen,").m_queries.len() == 2);
}

#[test]
fn a_walk_orders_selectors_under_their_rules() {
    use twidget_litehtml::stylesheet::{Css, StyleSheetInput};
    let lh = Lh::new();
    let mut later = Later::default();
    let sheet = b"@import 'a.css' print; a, b {c:d} @media screen{e{f:g}} @layer{h{}} @import 'm.css'; @supports not (x:y){i{}}";
    for (prelude, count) in [(&b"a, b "[..], 2), (b"e", 1), (b"h", 1), (b"i", 1), (b"i", 1)] {
        let tokens = parse_stylesheet(&[prelude, b"{}"].concat(), true)[0].prelude.clone();
        later.selector_counts.insert(token_list_bytes(&tokens), count);
    }
    later.supported.insert(token_list_bytes(&normalize(b"x:y", f_componentize, None)), false);
    let sheets = Sheets { lh: &lh, later: &later, container: TestContainer };
    let mut css = Css::default();
    css.parse_css_stylesheet(StyleSheetInput::Text(sheet), b"", &sheets, None, true);
    let walked: Vec<(i32, bool, bool, usize)> =
        css.selectors().iter().map(|s| (s.order, s.layered, s.media_query.is_some(), s.index)).collect();
    // a.css's i, then a and b, e under @media, h layered, i under @supports; m.css comes after a rule: not imported.
    assert_eq!(
        walked,
        [(0, false, true, 0), (1, false, false, 0), (2, false, false, 1), (3, false, true, 0), (4, true, false, 0), (5, false, false, 0)]
    );
    assert!(std::sync::Arc::ptr_eq(&css.selectors()[1].rule, &css.selectors()[2].rule));
    assert_eq!(css.selectors()[0].rule.baseurl, b"imported/a.css");
}
