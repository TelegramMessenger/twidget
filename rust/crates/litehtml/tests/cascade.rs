//! The cascade, ported (stage L5): documents finalized as the renderer builds them (the master sheet and the UA
//! layers, presentational attributes, the documents' sheets under their media, inline styles, `var()`, the
//! CSS-wide keywords, `!important`, layers, `::before`/`::after` with their content and counters, style sharing,
//! the selector budget), every element's computed style after each step (media changes, `:hover` and `:active`
//! flips through `find_styles_changes`, refreshes), through `oracle/lhdump` (`l`) and the port, field by field.
//! The documents: widget-sandbox's corpus snapshots in the light and the dark theme at two viewports, every tag
//! under the master sheet alone and with the UA layers, generated documents and sheets, the selector budget crossed,
//! and the `LH_NO_*` switches each in a process of its own. Each run reports what it reached.

mod common;

use common::cascade::*;
use common::gumbo::{corpus_pages, snapshot};
use common::values::*;
use common::*;
use twidget_litehtml::document::Switches;

/// A case: its record, the port's answer and what it is.
type Case = (Vec<u8>, (u8, Vec<u8>), String);

/// `l` cases through lhdump and the port, checked in batches of about 24 MB; settings (`E`) open every batch.
struct Run {
    name: &'static str,
    lh: Lh,
    switches: Switches,
    settings: Vec<u8>,
    batch: Vec<Case>,
    bytes: usize,
    cases: usize,
    tally: CascadeTally,
    oracle: Option<Oracle>,
}

impl Run {
    fn new(name: &'static str) -> Run {
        Run {
            name,
            lh: Lh::new(),
            switches: Switches::default(),
            settings: Vec::new(),
            batch: Vec::new(),
            bytes: 0,
            cases: 0,
            tally: CascadeTally::default(),
            oracle: Oracle::find("lhdump"),
        }
    }

    /// A run with `LH_NO_<switch>` set on both sides.
    fn with_switch(name: &'static str, switch: &str) -> Run {
        let mut run = Run::new(name);
        run.settings = framed(b'E', format!("LH_NO_{switch}=1").as_bytes());
        match switch {
            "STYLE_SHARING" => run.switches.style_sharing = false,
            "ID_SHARE" => run.switches.id_share = false,
            "SIBLING_INDEX" => run.switches.sibling_index = false,
            "DESCENDANT_PRUNE" => run.switches.descendant_prune = false,
            other => panic!("no switch {other}"),
        }
        run
    }

    fn case(&mut self, input: Vec<u8>, what: impl Into<String>) {
        self.tally.documents += 1;
        let rust = cascade_answer(&self.lh, self.switches, &input, &mut self.tally);
        self.bytes += input.len() + rust.1.len();
        self.cases += 1;
        if self.oracle.is_some() {
            self.batch.push((input, rust, what.into()));
        }
        if self.bytes > 24 << 20 {
            self.flush();
        }
    }

    fn flush(&mut self) {
        let batch = std::mem::take(&mut self.batch);
        self.bytes = 0;
        let Some(oracle) = &self.oracle else {
            self.lh = Lh::new();
            return;
        };
        if batch.is_empty() {
            return;
        }
        let mut input = self.settings.clone();
        for (case, _, _) in &batch {
            input.extend(framed(b'l', case));
        }
        let answers = oracle.run(input, batch.len());
        // Every batch is a process of its own: the next one starts with a fresh name table and declaration cache.
        let lh = std::mem::take(&mut self.lh);
        let mut differences = Vec::new();
        for ((case, rust, what), cpp) in batch.iter().zip(&answers) {
            if *cpp == *rust {
                continue;
            }
            let (cpp_body, _) = split_answer(&cpp.1);
            let (rust_body, _) = split_answer(&rust.1);
            if cpp.0 != rust.0 || cpp_body != rust_body {
                differences.push(explain(&lh, self.switches, case, &(cpp.0, cpp_body), &(rust.0, rust_body), what));
            } else if let Some(calls) = compare_calls(&cpp.1, &rust.1) {
                differences.push(format!("{calls}\n  in {}", what.chars().take(4000).collect::<String>()));
            }
            if differences.len() >= 3 {
                break;
            }
        }
        assert!(differences.is_empty(), "{}: answers differ, the first:\n{}", self.name, differences.join("\n"));
    }

    fn finish(mut self) -> CascadeTally {
        self.flush();
        let compared = if self.oracle.is_some() { "alike with lhdump" } else { "cascaded (no oracle)" };
        eprintln!("{}: {} cases {compared}; {}", self.name, self.cases, self.tally.report());
        self.tally
    }
}

/// Where two answers part: the field the Rust answer was writing there.
fn explain(lh: &Lh, switches: Switches, case: &[u8], cpp: &(u8, Vec<u8>), rust: &(u8, Vec<u8>), what: &str) -> String {
    let at = cpp.1.iter().zip(&rust.1).position(|(a, b)| a != b).unwrap_or(cpp.1.len().min(rust.1.len()));
    let marks = {
        start_marks();
        let mut tally = CascadeTally::default();
        // Again with the names as they are now: the same fields at the same places.
        cascade_answer(lh, switches, case, &mut tally);
        take_marks()
    };
    let element = marks.iter().rfind(|m| m.0 <= at && m.2 >= 0).map_or(-1, |m| m.2);
    let field = marks.iter().rfind(|m| m.0 <= at).map_or("", |m| m.1);
    let near = |b: &[u8]| {
        let piece = &b[at.saturating_sub(4).min(b.len())..(at + 80).min(b.len())];
        format!("{} {:?}", hex(&piece[..piece.len().min(24)]), String::from_utf8_lossy(piece))
    };
    format!(
        "status C++ {} / Rust {}, {} / {} bytes, first difference at byte {at}, element {element}, in {field}: C++ {} / Rust {}
  in {}",
        cpp.0,
        rust.0,
        cpp.1.len(),
        rust.1.len(),
        near(&cpp.1),
        near(&rust.1),
        what.chars().take(4000).collect::<String>()
    )
}

// ---- Inputs ------------------------------------------------------------------------------------------------

fn pick<'a>(random: &mut Lcg, from: &[&'a str]) -> &'a str {
    from[random.below(from.len())]
}

/// A declaration of a property style.cpp reads, its value of any form the property takes, decorated.
fn any_declaration(random: &mut Lcg) -> String {
    match random.below(14) {
        0 => format!(
            "--{}:{}",
            pick(random, &["x", "y", "z", "c"]),
            pick(random, &["red", "2px", "1em 2px", "var(--y)", "calc(1px + 2px)", "", "#0a0"])
        ),
        1 => format!(
            "{}:var(--{}{})",
            pick(
                random,
                &["color", "margin", "padding-left", "width", "font-size", "background", "border", "display", "position", "opacity"]
            ),
            pick(random, &["x", "y", "z", "c", "missing"]),
            pick(random, &["", ", 3px", ", blue", ", var(--x)", ","])
        ),
        2 => format!(
            "{}:{}",
            pick(
                random,
                &["color", "margin", "font-size", "display", "border-color", "line-height", "font-weight", "background-color", "opacity"]
            ),
            pick(random, &["inherit", "initial", "unset", "revert", "revert !important"])
        ),
        3 => format!(
            "content:{}",
            pick(
                random,
                &[
                    "'a'",
                    "\"x y\"",
                    "counter(n)",
                    "counters(n, '.')",
                    "attr(title)",
                    "open-quote",
                    "close-quote",
                    "none",
                    "url(i.png)",
                    "'\\41 b' counter(m) \"!\"",
                    "''",
                    "normal",
                    "'q' open-quote close-quote",
                ]
            )
        ),
        4 => format!(
            "{}:{}",
            pick(random, &["counter-reset", "counter-increment"]),
            pick(random, &["n", "n 2", "m -1 n", "none", "n x", "n 3000000000"])
        ),
        5 => format!("quotes:{}", pick(random, &["'<' '>'", "none", "auto", "'\"' '\"' \"'\" \"'\"", "inherit"])),
        6 => format!(
            "{}:{}",
            pick(random, &["width", "margin-left", "padding", "font-size", "top", "line-height", "border-width", "text-indent", "gap"]),
            pick(
                random,
                &["calc(10px + 5%)", "calc(1em * 2)", "min(10px, 2rem)", "1rem", "2ch", "3ex", "10vw", "5vmin", "1in", "3pt", "50%", "0"]
            )
        ),
        _ => {
            let property = pick(random, PROPERTIES);
            let value = any_value(random, property);
            let value = decorated(random, value);
            format!("{property}:{value}")
        }
    }
}

/// A selector the generated documents match: types, classes, ids, attributes, pseudo-classes and
/// pseudo-elements, combinators.
fn any_cascade_selector(random: &mut Lcg) -> String {
    let compound = |random: &mut Lcg| -> String {
        let mut s = pick(random, &["", "div", "p", "span", "li", "*", "a", "b", "input", "section", "td", "html", "body"]).to_string();
        for _ in 0..random.below(3) {
            s += pick(
                random,
                &[
                    ".a",
                    ".b",
                    "#i",
                    "[title]",
                    "[data-x=\"1\"]",
                    ":hover",
                    ":active",
                    ":first-child",
                    ":nth-child(2n+1)",
                    ":not(.a)",
                    ":is(p,.b)",
                    ":where(div)",
                    ":empty",
                    ":root",
                    ":checked",
                    ":disabled",
                    ":last-of-type",
                    ":lang(en)",
                ],
            );
        }
        if s.is_empty() { "*".into() } else { s }
    };
    let mut s = compound(random);
    for _ in 0..random.below(3) {
        s = format!("{}{}{}", compound(random), pick(random, &[" ", " > ", " + ", " ~ "]), s);
    }
    if random.chance(4) {
        s += pick(random, &["::before", "::after", ":before", "::after"]);
    }
    s
}

/// A sheet of rules of declarations, with `@media`, `@layer` and `@supports` blocks.
fn any_cascade_sheet(random: &mut Lcg) -> String {
    let mut out = String::new();
    for _ in 0..1 + random.below(12) {
        let decls: Vec<String> = (0..1 + random.below(5)).map(|_| any_declaration(random)).collect();
        let rule = format!("{}{{{}}}", any_cascade_selector(random), decls.join(";"));
        out += &match random.below(10) {
            0 => format!("@media (min-width:{}px){{{rule}}}", pick(random, &["300", "400", "600", "1000"])),
            1 => format!("@media (max-width:500px) and (orientation:portrait){{{rule}}}"),
            2 => format!("@layer l{{{rule}}}"),
            3 => format!("@supports (display:grid){{{rule}}}"),
            4 => format!("@media print{{{rule}}}"),
            _ => rule,
        };
    }
    out
}

/// A generated document: nested elements of many classes with ids, classes, attributes the fork maps, inline
/// styles of every property, sheets (one under a media attribute), text in every direction.
fn any_cascade_document(random: &mut Lcg) -> Vec<u8> {
    fn element(random: &mut Lcg, depth: u32, out: &mut String) {
        let tags = [
            "div", "p", "span", "a", "li", "ul", "ol", "b", "section", "table", "tr", "td", "img", "input", "button", "select", "textarea",
            "fieldset", "legend", "meter", "h1", "pre", "q", "font", "svg", "em", "label",
        ];
        let tag = pick(random, &tags);
        out.push('<');
        out.push_str(tag);
        for _ in 0..random.below(4) {
            let attr = match random.below(12) {
                0 => format!(" id=\"{}\"", pick(random, &["i", "j"])),
                1 | 2 => format!(" class=\"{}\"", pick(random, &["a", "b", "a b", "c"])),
                3 => format!(" title=\"{}\"", pick(random, &["t", "x y", ""])),
                4 => format!(" data-x=\"{}\"", pick(random, &["1", "2"])),
                5 => format!(" dir=\"{}\"", pick(random, &["auto", "rtl", "ltr"])),
                6 => format!(" type=\"{}\"", pick(random, &["text", "checkbox", "range", "number", "button", "color"])),
                7 => format!(" width=\"{}\"", pick(random, &["10", "50%", "0", "x"])),
                8 => format!(" color=\"{}\"", pick(random, &["red", "#123"])),
                9 => " disabled".to_string(),
                10 => format!(" lang=\"{}\"", pick(random, &["en", "fr"])),
                _ => format!(" placeholder=\"{}\"", pick(random, &["p", "hint"])),
            };
            out.push_str(&attr);
        }
        if random.chance(2) {
            let decls: Vec<String> = (0..1 + random.below(4)).map(|_| any_declaration(random)).collect();
            out.push_str(&format!(" style=\"{}\"", decls.join(";").replace('"', "'")));
        }
        out.push('>');
        if matches!(tag, "img" | "input") {
            return;
        }
        let n = if depth > 4 { 0 } else { random.below(4) };
        for _ in 0..n {
            match random.below(7) {
                0 => out.push_str(pick(random, &["text", " ", "\n  ", "a b", "\u{5d0}\u{5d1}", "\t", "UPPER lower"])),
                1 => out.push_str("<!-- c -->"),
                _ => element(random, depth + 1, out),
            }
        }
        out.push_str(&format!("</{tag}>"));
    }
    let mut out = String::new();
    if !random.chance(5) {
        out.push_str("<!DOCTYPE html>");
    }
    out.push_str("<html><head>");
    for _ in 0..random.below(3) {
        let media = if random.chance(4) {
            format!(" media=\"{}\"", pick(random, &["screen", "print", "(min-width:500px)"]))
        } else {
            String::new()
        };
        out.push_str(&format!("<style{media}>{}</style>", any_cascade_sheet(random)));
    }
    out.push_str("</head><body>");
    for _ in 0..1 + random.below(5) {
        element(random, 0, &mut out);
    }
    out.push_str("</body></html>");
    out.into_bytes()
}

/// Steps for a generated document: a dump, flips on two tags, a media change, a refresh.
fn generated_steps(random: &mut Lcg) -> Vec<Step> {
    let (k, j) = (random.below(40) as u32, random.below(40) as u32);
    let mut steps = vec![Step::Dump, Step::Hover(k), Step::Active(k), Step::Dump, Step::Leave(k), Step::Hover(j)];
    steps.push(Step::Media([320.0, 480.0, 700.0, 1200.0][random.below(4)], 600.0));
    steps.push(Step::Dump);
    if random.chance(3) {
        steps.push(Step::Refresh);
        steps.push(Step::Dump);
    }
    steps
}

/// A deep chain of divs and descendant selectors that walk up it: the selector budget runs out mid-sheet.
fn budget_chain_document() -> (Vec<u8>, Vec<u8>) {
    let mut html = String::from("<!DOCTYPE html><html><head><style>");
    for k in 0..700 {
        html += &format!("x{k} y z div{{color:red}}");
    }
    html += "div.m{color:rgb(1,2,3)}</style></head><body>";
    for _ in 0..120 {
        html += "<div class=\"m\">";
    }
    for _ in 0..120 {
        html += "</div>";
    }
    html += "</body></html>";
    (html.into_bytes(), Vec::new())
}

/// A long list of siblings and sibling selectors: the walks are charged per 64 children, an adjacent one not while
/// the index of a sheet's pass serves it.
fn budget_siblings_document() -> Vec<u8> {
    let mut html = String::from("<!DOCTYPE html><html><head><style>");
    for k in 0..24 {
        html += &format!("q{k} ~ p{{color:red}} q{k} + p{{color:blue}} q{k} + p:hover{{color:green}}");
    }
    html += "p.m{color:rgb(1,2,3)}</style></head><body>";
    for k in 0..600 {
        html += &format!("<p class=\"m\">{k}</p>");
    }
    html += "</body></html>";
    html.into_bytes()
}

// ---- The comparison ----------------------------------------------------------------------------------------

#[test]
fn the_corpus_cascades_alike() {
    let mut run = Run::new("corpus");
    let pages = corpus_pages();
    assert!(pages.len() > 300, "widget-sandbox's corpus: {} pages", pages.len());
    let light = themed_master(false);
    let dark = themed_master(true);
    for (k, (name, text)) in pages.iter().enumerate() {
        let snap = snapshot(text);
        for (dark_theme, (master, vars)) in [(false, &light), (true, &dark)] {
            let html = themed_html(&snap, dark_theme, vars);
            let (width, height, other) = if dark_theme { (412.0, 915.0, (800.0, 600.0)) } else { (375.0, 667.0, (1024.0, 768.0)) };
            let mut steps = vec![Step::Dump, Step::Media(other.0, other.1), Step::Dump];
            if k % 3 == 0 {
                let t = k as u32 * 7;
                steps.extend([Step::Hover(t), Step::Active(t), Step::Dump, Step::Leave(t), Step::Dump]);
            }
            let theme = if dark_theme { "dark" } else { "light" };
            run.case(cascade_record(width, height, 2.0, master, b"", &html, &steps), format!("{name} ({theme})"));
        }
    }
    let tally = run.finish();
    assert!(tally.elements > 100_000);
}

#[test]
fn the_master_sheet_and_ua_layers_cascade_alike() {
    let mut run = Run::new("master sheet and UA layers");
    let master = master_css();
    let (light, _) = themed_master(false);
    let (dark, vars) = themed_master(true);
    let steps = [Step::Dump, Step::Hover(5), Step::Active(9), Step::Dump, Step::Media(1200.0, 800.0), Step::Refresh, Step::Dump];
    for quirks in [false, true] {
        let html = every_tag_document(quirks);
        let what = if quirks { "every tag in quirks mode" } else { "every tag" };
        run.case(cascade_record(375.0, 667.0, 1.0, &master, b"", &html, &steps), format!("{what}, master CSS"));
        run.case(cascade_record(375.0, 667.0, 2.0, &light, b"", &html, &steps), format!("{what}, light UA layers"));
        run.case(
            cascade_record(412.0, 915.0, 3.0, &dark, b"", &themed_html(&html, true, &vars), &steps),
            format!("{what}, dark UA layers"),
        );
        run.case(cascade_record(375.0, 667.0, 2.0, b"", &master, &html, &steps), format!("{what}, master CSS as the user sheet"));
        run.case(cascade_record(375.0, 667.0, 2.0, b"", b"", &html, &steps), format!("{what}, no sheet"));
    }
    run.finish();
}

#[test]
fn generated_documents_cascade_alike() {
    let mut run = Run::new("generated documents");
    let mut random = Lcg::new(0x15_0001);
    let (light, _) = themed_master(false);
    let only: Option<usize> = std::env::var("CASCADE_ONLY").ok().and_then(|v| v.parse().ok());
    for k in 0..1500 {
        let html = any_cascade_document(&mut random);
        let steps = generated_steps(&mut random);
        let master: &[u8] = if k % 2 == 0 { &light } else { &[] };
        let master = if master.is_empty() { master_css() } else { master.to_vec() };
        let user = if k % 7 == 0 { any_cascade_sheet(&mut random).into_bytes() } else { Vec::new() };
        let width = [320.0, 375.0, 600.0, 1000.0][k % 4];
        if only.is_some_and(|only| only != k) {
            continue;
        }
        run.case(
            cascade_record(width, 640.0, 2.0, &master, &user, &html, &steps),
            format!("generated {k}: {}", String::from_utf8_lossy(&html)),
        );
    }
    run.finish();
}

#[test]
fn the_selector_budget_runs_out_alike() {
    let mut run = Run::new("selector budget");
    let master = master_css();
    let (chain, _) = budget_chain_document();
    let siblings = budget_siblings_document();
    let steps = [Step::Dump, Step::Hover(40), Step::Dump, Step::Refresh, Step::Dump];
    run.case(cascade_record(375.0, 667.0, 2.0, &master, b"", &chain, &steps), "a deep chain under descendant selectors");
    run.case(cascade_record(375.0, 667.0, 2.0, &master, b"", &siblings, &steps), "600 siblings under sibling selectors");
    run.finish();
    // The budget did run out: the rule sorted last reached some of the elements it selects and not the others.
    for html in [&chain, &siblings] {
        let lh = Lh::new();
        let container = CascadeContainer::new(375.0, 667.0, 2.0);
        let cx = twidget_litehtml::document::StyleCx {
            names: &lh.names,
            container: &container,
            cache: &lh.cache,
            switches: Switches::default(),
        };
        let doc = twidget_litehtml::Document::create_updatable(html, &master, &cx);
        let (mut reached, mut missed) = (0, 0);
        for el in &doc.elements {
            if el.classes.iter().any(|c| &lh.names.s(*c)[..] == b"m") {
                // the class rule is the document sheet's only one with a class
                if el.used_styles.iter().any(|u| u.selector.m_sheet == 1 && u.selector.m_specificity.c == 1) {
                    reached += 1;
                } else {
                    missed += 1;
                }
            }
        }
        eprintln!("selector budget: the last rule reached {reached} elements and missed {missed}");
        assert!(reached > 0 && missed > 0, "the budget runs out mid-document ({reached} reached, {missed} missed)");
    }
}

#[test]
fn the_switches_cascade_alike() {
    for switch in ["STYLE_SHARING", "ID_SHARE", "SIBLING_INDEX", "DESCENDANT_PRUNE"] {
        let name: &'static str = match switch {
            "STYLE_SHARING" => "LH_NO_STYLE_SHARING",
            "ID_SHARE" => "LH_NO_ID_SHARE",
            "SIBLING_INDEX" => "LH_NO_SIBLING_INDEX",
            _ => "LH_NO_DESCENDANT_PRUNE",
        };
        let mut run = Run::with_switch(name, switch);
        let mut random = Lcg::new(0x15_0002);
        let master = master_css();
        for k in 0..150 {
            let html = any_cascade_document(&mut random);
            let steps = generated_steps(&mut random);
            run.case(
                cascade_record(375.0, 667.0, 2.0, &master, b"", &html, &steps),
                format!("generated {k}: {}", String::from_utf8_lossy(&html)),
            );
        }
        if switch == "SIBLING_INDEX" || switch == "DESCENDANT_PRUNE" {
            let steps = [Step::Dump, Step::Refresh, Step::Dump];
            let (chain, _) = budget_chain_document();
            run.case(cascade_record(375.0, 667.0, 2.0, &master, b"", &chain, &steps), "a deep chain under descendant selectors");
            run.case(cascade_record(375.0, 667.0, 2.0, &master, b"", &budget_siblings_document(), &steps), "600 siblings");
        }
        run.finish();
    }
}
