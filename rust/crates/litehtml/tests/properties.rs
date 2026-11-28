//! The Rust `style.cpp` and the value parsers of `css_effects.cpp` against the fork's (`oracle/lhdump`,
//! records `f` and `y`): declaration blocks through `style::add` of text (with the declaration cache) and of
//! tokens, single declarations through `add_property`, every property each leaves with its typed value,
//! `!important`, order and `var()` flag, the name table's size after it; and the values a style keeps as
//! tokens through the parsers the computed style runs on them, with `fold_math` on every value. The
//! declarations: widget-sandbox's corpus (style blocks and attributes), litehtml's master CSS and the
//! renderer's UA layers, generated declarations of every property the fork knows, and random changes of
//! them, through four containers (none, lhdump's, the renderer's sanitizing one, one that resolves colour
//! names). Skipped without `TWIDGET_ORACLE_DIR`.

mod common;

use std::collections::BTreeMap;

use common::values::*;
use common::*;
use twidget_litehtml::Names;
use twidget_litehtml::css_parser::{f_componentize, f_remove_whitespace, normalize, parse_stylesheet, parse_stylesheet_tokens};
use twidget_litehtml::css_tokenizer::*;
use twidget_litehtml::css_values::CssUnits;
use twidget_litehtml::gradient::ImageType;
use twidget_litehtml::style::{Style, Value};
use twidget_litehtml::stylesheet::RuleType;

const KINDS: [&str; 14] = [
    "invalid",
    "inherit",
    "css-wide keyword",
    "int",
    "int vector",
    "length",
    "length vector",
    "float",
    "color",
    "images",
    "string",
    "strings",
    "sizes",
    "tokens",
];

/// One lhdump run of declaration cases, with what they reached.
struct StyleRun {
    lh: Lh,
    c: Comparison,
    counts: BTreeMap<String, usize>,
    properties: BTreeMap<String, BTreeMap<&'static str, usize>>,
}

impl StyleRun {
    fn new() -> StyleRun {
        StyleRun { lh: Lh::new(), c: Comparison::new(), counts: BTreeMap::new(), properties: BTreeMap::new() }
    }

    fn count(&mut self, what: impl Into<String>, n: usize) {
        *self.counts.entry(what.into()).or_default() += n;
    }

    /// An `f` case, answered and tallied now: the Rust side keeps the name table and the cache in the order
    /// lhdump meets the cases.
    fn style(&mut self, container: u8, entry: u8, baseurl: &[u8], name: &[u8], text: &[u8], what: impl Into<String>) {
        let (text, _) = without_overlong_ascii(text);
        let input = style_input(container, entry, baseurl, name, &text);
        let (st, answer) = self.lh.style_case(&input);
        self.tally(&st);
        self.count(
            format!("entry {}", ["add(text)", "add(tokens)", "add_property", "add_property !important"][usize::from(entry.min(3))]),
            1,
        );
        self.c.case(&framed(b'f', &input), answer, what);
    }

    /// A `y` case.
    fn effects(&mut self, which: u8, container: u8, text: &[u8], what: impl Into<String>) {
        let (text, _) = without_overlong_ascii(text);
        let input = effects_input(which, container, &text);
        let answer = self.lh.effects_answer(&input);
        let parser = [
            "box-shadow",
            "filter",
            "transform",
            "transform-origin",
            "clip-path",
            "grid track list",
            "grid placement",
            "fast transform",
            "fold_math",
        ][usize::from(which.min(8))];
        self.count(format!("{parser} {}", if answer.0 == 1 { "parsed" } else { "refused" }), 1);
        self.c.case(&framed(b'y', &input), answer, what);
    }

    /// A declaration every way: `add_property` and `!important`, a block of text and of tokens, the
    /// containers taken in turn.
    fn declaration(&mut self, name: &[u8], value: &[u8], k: usize, what: &str) {
        let container = (k % 4) as u8;
        self.style(container, 2 + u8::from(k.is_multiple_of(5)), b"", name, value, format!("add_property {what}"));
        let text = [name, b": ", value].concat();
        match k % 3 {
            0 => self.style((container + 1) % 4, 0, b"", b"", &text, format!("add(text) {what}")),
            1 => self.style((container + 2) % 4, 1, if k % 7 == 1 { b"base/" } else { b"" }, b"", &text, format!("add(tokens) {what}")),
            _ => {}
        }
    }

    fn tally(&mut self, st: &Style) {
        if st.empty() {
            self.count("styles empty", 1);
        }
        for (&id, p) in st.properties() {
            let name = String::from_utf8_lossy(&self.lh.names.s(id)).into_owned();
            let kind = KINDS[usize::from(p.value.index())];
            *self.properties.entry(name).or_default().entry(kind).or_default() += 1;
            self.count(format!("kind {kind}"), 1);
            if p.m_important {
                self.count("!important kept", 1);
            }
            if p.m_has_var {
                self.count("waiting for var()", 1);
            }
            match &p.value {
                Value::Length(len) => {
                    let form = if len.is_predefined() {
                        "keyword".to_string()
                    } else if len.units() == CssUnits::css_units_calc {
                        "calc".to_string()
                    } else {
                        format!("{:?}", len.units()).replace("css_units_", "")
                    };
                    self.count(format!("length {form}"), 1);
                }
                Value::Images(images) => {
                    for img in images {
                        let form = match img.ty {
                            ImageType::type_none => "none".to_string(),
                            ImageType::type_url => "url".to_string(),
                            ImageType::type_gradient => String::from_utf8_lossy(&self.lh.names.s(img.m_gradient.m_type)).into_owned(),
                        };
                        self.count(format!("image {form}"), 1);
                    }
                }
                Value::CssWideKeyword(kind) => self.count(format!("keyword {kind:?}"), 1),
                Value::Color(c) if c.is_current_color => self.count("color currentcolor", 1),
                _ => {}
            }
        }
    }

    fn check(self, name: &str) {
        let Some(oracle) = Oracle::find("lhdump") else { return };
        let total = self.c.len();
        let tally = self.c.check(&oracle);
        let counts: Vec<String> = self.counts.iter().map(|(k, n)| format!("{k} {n}")).collect();
        eprintln!("{name}: {total} answers alike ({} of status 1, {} of 0); {}", tally[1], tally[0], counts.join(", "));
        if self.properties.is_empty() {
            return;
        }
        let properties: Vec<String> = self
            .properties
            .iter()
            .map(|(p, kinds)| format!("{p} [{}]", kinds.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", ")))
            .collect();
        eprintln!("{name}: {} properties held: {}", properties.len(), properties.join("; "));
    }
}

// ---- Cases -----------------------------------------------------------------------------------------------

/// The qualified rules' blocks of a sheet at any depth, as text.
fn rule_blocks(rules: &[twidget_litehtml::stylesheet::RawRule], out: &mut Vec<Vec<u8>>) {
    for rule in rules {
        if rule.ty == RuleType::qualified {
            out.push(get_repr(&rule.block.value, 0, -1, false));
        } else if rule.block.ty == CURLY_BLOCK {
            rule_blocks(&parse_stylesheet_tokens(&rule.block.value, false), out);
        }
    }
}

#[test]
fn corpus_declarations_alike() {
    let styles = corpus_styles();
    let mut run = StyleRun::new();
    let mut random = Lcg::new(0x11_5001);
    let mut blocks = 0;
    let mut attributes = 0;
    let mut k = 0usize;
    for (i, style) in styles.iter().enumerate() {
        let (style, _) = without_overlong_ascii(style);
        if style.contains(&b'{') {
            let mut texts = Vec::new();
            rule_blocks(&parse_stylesheet(&style, true), &mut texts);
            for text in texts {
                blocks += 1;
                run.style(1, 1, b"", b"", &text, format!("rule block of style {i}"));
                run.style(2, 0, b"", b"", &text, format!("rule block of style {i} as text"));
            }
        } else {
            attributes += 1;
            run.style(2, 0, b"", b"", &style, format!("style attribute {i}"));
            run.style(0, 0, b"", b"", &style, format!("style attribute {i} without a container"));
            run.style(3, 1, b"base/", b"", &style, format!("style attribute {i} with a base url"));
        }
        for (name, value) in declarations(&style) {
            k += 1;
            run.style(
                (k % 4) as u8,
                2 + u8::from(k.is_multiple_of(9)),
                b"",
                &twidget_litehtml::html::lowcase(&name),
                &value,
                format!("declaration {k} of style {i}"),
            );
            if k.is_multiple_of(5) {
                let mut changed = value.clone();
                let edits = mutate(&mut random, &mut changed);
                run.style((k % 3) as u8 + 1, 2, b"", &name, &changed, format!("declaration {k} of style {i} changed{edits}"));
            }
        }
    }
    assert!(blocks > 1000 && attributes > 500, "found {blocks} rule blocks and {attributes} style attributes in widget-sandbox's corpus");
    eprintln!("corpus: {} style texts, {blocks} rule blocks, {attributes} style attributes, {k} declarations", styles.len());
    run.check("corpus declarations");
}

#[test]
fn master_declarations_alike() {
    let master = master_css();
    let extra = ua_extra();
    let ua = ua_stylesheet();
    let resolved =
        twidget_css::unwrap_where(&twidget_css::substitute_variables(&ua, &[(b"--tg-theme-bg-color".to_vec(), b"#fff".to_vec())]));
    let mut run = StyleRun::new();
    let mut random = Lcg::new(0x11_5002);
    let mut n = 0;
    for (name, text) in [
        ("master_css", &master),
        ("UA_EXTRA", &extra),
        ("the profile's UA layer", &ua),
        ("the UA layer as the renderer reads it", &resolved),
    ] {
        let mut texts = Vec::new();
        rule_blocks(&parse_stylesheet(text, true), &mut texts);
        for (k, block) in texts.iter().enumerate() {
            n += 1;
            for container in 0..4u8 {
                run.style(container, 1, b"", b"", block, format!("rule {k} of {name} in container {container}"));
            }
            run.style(2, 0, b"", b"", block, format!("rule {k} of {name} as text"));
            if k.is_multiple_of(4) {
                let mut changed = block.clone();
                let edits = mutate(&mut random, &mut changed);
                run.style(1, 1, b"", b"", &changed, format!("rule {k} of {name} changed{edits}"));
            }
        }
    }
    assert!(n > 150, "{n} rules in the master and UA sheets");
    run.check("master declarations");
}

#[test]
fn generated_declarations_alike() {
    let mut run = StyleRun::new();
    let mut random = Lcg::new(0x11_5003);
    let mut k = 0;
    for round in 0..40 {
        for &property in PROPERTIES {
            k += 1;
            let value = any_value(&mut random, property);
            let value = decorated(&mut random, value);
            run.declaration(property.as_bytes(), value.as_bytes(), k, &format!("{property}: {value} (round {round})"));
            if k.is_multiple_of(6) {
                let mut changed = value.clone().into_bytes();
                let edits = mutate(&mut random, &mut changed);
                run.style((k % 4) as u8, 2, b"", property.as_bytes(), &changed, format!("{property}: {value} changed{edits}"));
            }
        }
    }
    // The shorthands and lists whose valid values are rare among random ones, again.
    for round in 0..150 {
        for property in [
            "background",
            "background-image",
            "background-position",
            "background-size",
            "font",
            "flex",
            "border",
            "outline",
            "list-style",
            "text-decoration",
            "text-emphasis",
            "grid",
            "place-items",
            "place-content",
            "border-radius",
            "align-self",
            "flex-flow",
            "margin",
            "border-color",
            "font-feature-settings",
        ] {
            k += 1;
            let value = any_value(&mut random, property);
            run.declaration(property.as_bytes(), value.as_bytes(), k, &format!("{property}: {value} (again {round})"));
        }
    }
    // Blocks of several declarations: the cache, `-lh-anim`'s layer, later declarations over earlier ones,
    // `!important` against them.
    for i in 0..3000usize {
        let n = 1 + random.below(5);
        let mut decls = Vec::new();
        if random.chance(4) {
            decls.push("-lh-anim: 1".to_string());
        }
        for _ in 0..n {
            let property = pick_str(&mut random, PROPERTIES);
            let value = any_value(&mut random, property);
            let value = decorated(&mut random, value);
            decls.push(format!("{property}: {value}"));
        }
        if random.chance(5) {
            decls.insert(random.below(decls.len()), "-lh-anim:x".into());
        }
        let text = decls.join(pick_str(&mut random, &["; ", ";", " ;\n", ";;"]));
        let entry = (i % 2) as u8;
        run.style((i % 4) as u8, entry, if i.is_multiple_of(11) { b"b/" } else { b"" }, b"", text.as_bytes(), format!("block {i}: {text}"));
    }
    // The same declarations again: answers from the cache, keyed by a container's presence alone.
    let mut again = Lcg::new(0x11_5003);
    for i in 0..400 {
        let property = pick_str(&mut again, PROPERTIES);
        let value = any_value(&mut again, property);
        let text = format!("{property}: {value}");
        run.style((i % 4) as u8, 0, b"", b"", text.as_bytes(), format!("again {i}: {text}"));
        run.style(((i + 1) % 4) as u8, 0, b"", b"", text.as_bytes(), format!("again in another container {i}: {text}"));
    }
    // Edges of the parsers, each through every entry.
    for (i, text) in [
        "-lh-anim:1; -lh-raw-opacity: 5; opacity: .5",
        "-lh-anim: 1;transform: none;-lh-raw-transform: x",
        "-LH-ANIM:1; opacity: 1",
        "-lh-animation: 1; color: red",
        "font: 12px",
        "font: \"caption\"",
        "font: Caption",
        "font: italic bold 12px/30px Georgia, serif",
        "font: normal normal normal 1px a",
        "flex: content",
        "flex: unheard",
        "flex: initial",
        "flex: 0",
        "flex: 1 0",
        "flex: 0 0 0",
        "text-decoration: 3px from-font",
        "text-decoration: auto",
        "text-decoration: underline red wavy 2px blink",
        "text-emphasis: 5 red blue 'x'",
        "rotate: 1turn",
        "order: 2147483647",
        "order: -2147483649",
        "background: url(a.png) 10px 20px / cover no-repeat fixed content-box padding-box red",
        "background: red, blue",
        "background: center / 10px",
        "background: left top / 10px 20px, linear-gradient(red, blue)",
        "background: top / x",
        "background-position: top left, 10px bottom",
        "border: 1px solid red; border-color: blue; border-top: thick",
        "margin: 1px 2px 3px",
        "border-radius: 1px 2px / 3px",
        "inset: auto 1px",
        "place-content: normal",
        "place-content: center stretch",
        "place-items: baseline first",
        "align-self: safe center",
        "align-items: last baseline",
        "aspect-ratio: auto 4 / 3",
        "contain: none strict",
        "color-scheme: dark only",
        "accent-color: auto",
        "list-style: none none",
        "list-style: none disc",
        "list-style: inside url(x) none",
        "grid: auto-flow 10px / 1fr",
        "grid-template: none",
        "gap: 1px normal",
        "grid-gap: 10%",
        "width: 10vw",
        "width: calc(100% - 1vw)",
        "font-feature-settings: \"liga\" on, \"tnum\" 2",
        "font-variant-numeric: ordinal ordinal",
        "color: currentcolor",
        "color: inherit",
        "--x: inherit",
        "--x: initial",
        "--: 1",
        "--a: ;",
        "-webkit-backdrop-filter: blur(2px)",
        "word-wrap: anywhere",
        "-webkit-user-select: none",
        "overflow: hidden auto",
        "overflow: hidden auto scroll",
        "background-color: brand",
        "background-color: alias",
        "background-color: twotok",
        "color: Accentx",
        "border-spacing: 1px -1px",
        "border-top-left-radius: 1px 2px 3px",
        "z-index: 1.5",
    ]
    .iter()
    .enumerate()
    {
        for entry in 0..2u8 {
            run.style((i % 4) as u8, entry, b"", b"", text.as_bytes(), format!("edge {text:?} entry {entry}"));
        }
        if let Some((name, value)) = text.split_once(':') {
            run.style(
                ((i + 1) % 4) as u8,
                2,
                b"",
                name.trim().as_bytes(),
                value.split(';').next().unwrap_or("").as_bytes(),
                format!("edge add_property {text:?}"),
            );
        }
    }
    run.check("generated declarations");
}

#[test]
fn effects_alike() {
    let mut run = StyleRun::new();
    let mut random = Lcg::new(0x11_5004);
    // Every value the corpus gives these properties, and generated ones.
    let parsers: [(u8, &[&str]); 7] = [
        (0, &["box-shadow", "text-shadow"]),
        (1, &["filter", "backdrop-filter", "-webkit-backdrop-filter"]),
        (2, &["transform"]),
        (3, &["transform-origin"]),
        (4, &["clip-path"]),
        (5, &["grid-template-columns", "grid-template-rows", "grid-auto-rows"]),
        (6, &["grid-column", "grid-row", "grid-column-start", "grid-column-end", "grid-row-start", "grid-row-end"]),
    ];
    let mut k = 0;
    for style in corpus_styles() {
        let (style, _) = without_overlong_ascii(&style);
        for (name, value) in declarations(&style) {
            k += 1;
            let name = String::from_utf8_lossy(&twidget_litehtml::html::lowcase(&name)).into_owned();
            for (which, properties) in &parsers {
                if properties.contains(&name.as_str()) {
                    run.effects(*which, (k % 4) as u8, &value, format!("{name}: {} (corpus)", String::from_utf8_lossy(&value)));
                    if *which == 2 {
                        run.effects(7, 0, &value, format!("fast {name}: {} (corpus)", String::from_utf8_lossy(&value)));
                    }
                }
            }
            run.effects(8, (k % 4) as u8, &value, format!("fold {name}: {} (corpus)", String::from_utf8_lossy(&value)));
        }
    }
    for round in 0..600 {
        for (which, properties) in &parsers {
            let property = properties[round % properties.len()];
            let mut value = any_value(&mut random, property).into_bytes();
            if round.is_multiple_of(7) {
                mutate(&mut random, &mut value);
            }
            run.effects(*which, (round % 4) as u8, &value, format!("{property}: {} (generated {round})", String::from_utf8_lossy(&value)));
            if *which == 2 {
                run.effects(7, 0, &value, format!("fast transform: {} (generated {round})", String::from_utf8_lossy(&value)));
            }
        }
        let value = (0..1 + random.below(3))
            .map(|_| if random.chance(2) { math(&mut random, 0) } else { color(&mut random) })
            .collect::<Vec<_>>()
            .join(" ");
        run.effects(8, (round % 4) as u8, value.as_bytes(), format!("fold {value}"));
    }
    // Folding's edges: ranges, units, depth, mixes.
    for text in [
        "calc(1e30px * 1e30)",
        "calc(1px / 0)",
        "calc(1px * 2px)",
        "calc(2 / 1px)",
        "min(1px, 2em)",
        "min(1px, 0, 3px)",
        "clamp(1px, 2px)",
        "max()",
        "calc(((((((((((((((((((1px)))))))))))))))))))",
        "calc(1px + 0em)",
        "calc(0 + 5%)",
        "min(1px,2px,3px,4px,5px,6px,7px,8px,9px,10px,11px,12px,13px,14px,15px,16px,17px,18px,19px,20px,21px,22px,23px,24px,25px,26px,27px,28px,29px,30px,31px,32px,33px)",
        "color-mix(in srgb, black, white)",
        "color-mix(in srgb, #000 30%, red)",
        "color-mix(in srgb, red 0%, blue 0%)",
        "color-mix(in srgb, red 60%, blue 60%)",
        "color-mix(in srgb, red 20%, blue 20%)",
        "color-mix(in srgb, transparent, red)",
        "color-mix(in oklab, red, blue)",
        "color-mix(in srgb, currentcolor, red)",
        "env(safe-area-inset-top)",
        "env(foo, 1px 2px)",
        "env(foo, 3px)",
        "env()",
        "f(env(keyboard-inset-bottom))",
        "10vw 5svh 2dvmin 1lvmax",
        "calc(100vw - 10px)",
        "var(--x, calc(1px + 1px))",
        "1e40px",
        "calc(1e38 * 10)",
    ] {
        for container in 0..3u8 {
            run.effects(8, container, text.as_bytes(), format!("fold {text:?} in container {container}"));
        }
    }
    // The fast path's own edges.
    for text in [
        "none",
        " none ",
        "nonex",
        "none(",
        "translate(10px, 20%) rotate(45deg)",
        "scale(1.5)",
        "matrix(1,0,0,1,0,0)",
        "rotate(1e3deg)",
        "translateX(+1.5px)",
        "skewx(.5rad)",
        "rotate(10)",
        "scale(1 2 3 4 5 6 7)",
        "translate(1px 2px",
        "translate()",
        "rotate(1.px)",
        "scale(99999999999)",
        "TRANSLATE(1PX)",
        "rotate(1turn)x",
        "translate(10px)translate(1px)",
        "foo(1)",
        "",
    ] {
        run.effects(7, 0, text.as_bytes(), format!("fast transform {text:?}"));
    }
    run.check("effects");
}

/// What the comparison found, kept as the port's own checks.
#[test]
fn the_forks_quirks_hold() {
    let names = Names::new();
    let cache = twidget_litehtml::style::DeclarationCache::default();
    let add = |text: &str, container: Option<&dyn twidget_litehtml::document_container::DocumentContainer>| {
        let mut st = Style::default();
        st.add_text(&names, &cache, text.as_bytes(), b"", container);
        st
    };
    let id = |name: &str| names.find_id(name.as_bytes()).expect("a known name");
    let value = |st: &Style, name: &str| st.get_property(id(name)).value.clone();

    // A keyword after a length keeps the length's unit (`from_token` sets only the index).
    let st = add("flex: content", None);
    match value(&st, "flex-basis") {
        Value::Length(len) => assert!(len.is_predefined() && len.predef() == 1 && len.units() == CssUnits::css_units_px),
        other => panic!("{other:?}"),
    }
    // `color-mix()` refuses opaque black: it compares equal to currentcolor.
    let mut tokens = normalize(b"color-mix(in srgb, black 50%, white)", f_componentize | f_remove_whitespace, None);
    assert!(!twidget_litehtml::css_effects::fold_math(&names, &mut tokens, None, 0));
    // `font` takes a size without a family, the family empty.
    let st = add("font: 12px", None);
    assert_eq!(value(&st, "font-family"), Value::String(Vec::new()));
    // A malformed rotate3d() is skipped, not refused.
    let mut functions = Vec::new();
    let tokens = normalize(b"rotate3d(1) translate(1px)", f_componentize | f_remove_whitespace, None);
    assert!(twidget_litehtml::css_effects::parse_transform(&names, &tokens, &mut functions));
    assert_eq!(functions.len(), 1);
    // The cache keys by whether there is a container, not which: the renderer's sanitizing is not consulted
    // for a declaration another container left in the cache.
    let st = add("display: list-item", Some(&DeclContainer::Plain));
    assert!(!st.empty());
    let st = add("display: list-item", Some(&DeclContainer::Renderer));
    assert!(!st.empty(), "the cached declaration is replayed");
    let mut fresh = Style::default();
    fresh.add(&names, &normalize(b"display: list-item", f_componentize, None), b"", Some(&DeclContainer::Renderer));
    assert!(fresh.empty(), "the renderer drops list-item when it is asked");
    // Every property name a style meets is interned for good, and so is a lone flex keyword.
    let before = names.usage().names;
    add("made-up-property: 1; flex: unheard", None);
    assert_eq!(names.usage().names, before + 2);
    assert!(names.find_id(b"unheard").is_some() && names.find_id(b"made-up-property").is_some());
    // Past `-lh-anim`, a style text drops a raw name declared outright; a block of tokens keeps it.
    let raw = id("-lh-raw-opacity");
    assert!(add("-lh-anim:1; -lh-raw-opacity: 5", None).get_property(raw).value == Value::Invalid);
    let mut tokens_path = Style::default();
    tokens_path.add(&names, &normalize(b"-lh-anim:1; -lh-raw-opacity: 5", f_componentize, None), b"", None);
    assert!(matches!(tokens_path.get_property(raw).value, Value::Tokens(_)));
    // `auto` in text-decoration is taken as its colour, before its thickness.
    let st = add("text-decoration: auto", None);
    assert!(matches!(value(&st, "text-decoration-color"), Value::Color(c) if c.is_current_color));
    assert_eq!(value(&st, "text-decoration-thickness"), Value::Invalid);
    // Math folds in doubles and narrows to float unchecked: past float's range it is infinite.
    let mut tokens = normalize(b"calc(1e30px * 1e30)", f_componentize | f_remove_whitespace, None);
    assert!(twidget_litehtml::css_effects::fold_math(&names, &mut tokens, None, 0));
    assert!(tokens[0].ty == DIMENSION && tokens[0].n.number == f32::INFINITY);
}

#[test]
fn declaration_cache_limits_alike() {
    let mut run = StyleRun::new();
    let list_item = b"display: list-item";
    // A container's verdict stays in the cache for another: the renderer's sanitizing is not asked again.
    run.style(1, 0, b"", b"", list_item, "list-item in lhdump's container");
    run.style(2, 0, b"", b"", list_item, "list-item in the renderer's, from the cache");
    // Past 512 bytes, or with a viewport length, a declaration is not cached.
    let long = [&b"display:"[..], &[b' '; 600], b"list-item"].concat();
    run.style(1, 0, b"", b"", &long, "a long list-item in lhdump's container");
    run.style(2, 0, b"", b"", &long, "a long list-item in the renderer's");
    for container in [1u8, 2, 1, 2] {
        run.style(
            container,
            0,
            b"",
            b"",
            b"width: 10vw; height: calc(5vh + 1px); left: 1vmax",
            format!("viewport lengths in container {container}"),
        );
    }
    // Past 1 MB of keys the cache is emptied whole, and the renderer is asked.
    for batch in 0..45 {
        let text: Vec<String> = (0..50).map(|j| format!("--b{}: {}", batch * 50 + j, "a".repeat(480))).collect();
        run.style(1, 0, b"", b"", text.join(";").as_bytes(), format!("long fillers {batch}"));
    }
    run.style(2, 0, b"", b"", list_item, "list-item in the renderer's after the cache was emptied");
    run.style(1, 0, b"", b"", list_item, "list-item in lhdump's, from the cache: dropped");
    // Past 50000 declarations likewise.
    for batch in 0..101 {
        let text: Vec<String> = (0..500).map(|j| format!("--e{}:1", batch * 500 + j)).collect();
        run.style(1, 0, b"", b"", text.join(";").as_bytes(), format!("fillers {batch}"));
    }
    run.style(1, 0, b"", b"", list_item, "list-item in lhdump's after the cache was emptied again");
    assert!(run.lh.cache.len() < 50000);
    run.check("declaration cache limits");
}

/// `subst_vars` waits for the cascade's elements to be compared; this checks the port on its own.
#[test]
fn variables_are_substituted() {
    use twidget_litehtml::style::VarElement;
    struct Vars<'a>(&'a Names, Vec<(&'static str, &'static str)>);
    impl VarElement for Vars<'_> {
        fn get_custom_property(&self, name: twidget_litehtml::StringId, result: &mut CssTokenVector, _invalid: &mut bool) -> bool {
            for (n, v) in &self.1 {
                if self.0.id(n.as_bytes()) == name {
                    *result = normalize(v.as_bytes(), f_componentize | f_remove_whitespace, None);
                    return true;
                }
            }
            false
        }
        fn container(&self) -> Option<&dyn twidget_litehtml::document_container::DocumentContainer> {
            None
        }
    }
    let names = Names::new();
    let cache = twidget_litehtml::style::DeclarationCache::default();
    let mut st = Style::default();
    st.add_text(
        &names,
        &cache,
        b"color: var(--a); margin: var(--m, 1px 2px); width: var(--nope); border: var(--b); border-color: var(--a)",
        b"",
        None,
    );
    assert!(st.has_var_property());
    st.subst_vars(&names, &Vars(&names, vec![("--a", "red"), ("--b", "1px solid blue")]));
    let get = |name: &str| st.get_property(names.id(name.as_bytes())).value.clone();
    assert!(matches!(get("color"), Value::Color(c) if c.red == 255 && c.alpha == 255));
    assert!(matches!(get("margin-left"), Value::Length(len) if len.val() == 2.0));
    // A missing variable without fallback leaves the declaration as it was, still waiting for var().
    assert!(matches!(get("width"), Value::Tokens(_)) && st.get_property(names.id(b"width")).m_has_var);
    // border-color after border: its longhands keep the later declaration.
    assert!(matches!(get("border-top-color"), Value::Color(c) if c.red == 255));
    assert!(matches!(get("border-top-width"), Value::Length(len) if len.val() == 1.0));
}
