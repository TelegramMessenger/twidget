//! The render tree, ported (stage L6): documents finalized and cascaded as the renderer builds them, then their render
//! trees (the items of every class, anonymous blocks and table boxes with the cascade they run, inlines split around
//! blocks, flex items, table grids with spans and the cell budget, list items' `list_index`, cells given their
//! column's width, every element's render items as weak references, the tabular list) after the finalization, a media
//! change that builds the tree again and restyles, through `oracle/lhdump` (`o` with `r`) and the port, item by item.
//! The documents: widget-sandbox's corpus snapshots in the light and the dark theme at two viewports, every tag under
//! the master sheet and the UA layers, generated documents of tables, lists, inline and block mixes, flex and grid
//! containers, `display` values and their changes across a media query, `::before` and `::after` boxes; tables that
//! cross the cell budget; a deep document on a small stack. Each run reports what it reached.

mod common;

use common::cascade::*;
use common::gumbo::{corpus_pages, snapshot};
use common::render::*;
use common::*;
use twidget_litehtml::document::Switches;

/// A case: its record, the port's answer and what it is.
type Case = (Vec<u8>, (u8, Vec<u8>), String);

/// `o` cases through lhdump and the port, checked in batches of about 24 MB.
struct Run {
    name: &'static str,
    lh: Lh,
    batch: Vec<Case>,
    bytes: usize,
    cases: usize,
    tally: RenderTally,
    oracle: Option<Oracle>,
}

impl Run {
    fn new(name: &'static str) -> Run {
        Run { name, lh: Lh::new(), batch: Vec::new(), bytes: 0, cases: 0, tally: RenderTally::default(), oracle: Oracle::find("lhdump") }
    }

    fn case(&mut self, input: Vec<u8>, what: impl Into<String>) {
        let rust = render_answer(&self.lh, Switches::default(), &input, &mut self.tally);
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
        let mut input = Vec::new();
        for (case, _, _) in &batch {
            input.extend(framed(b'o', case));
        }
        let answers = oracle.run(input, batch.len());
        // Every batch is a process of its own: the next one starts with a fresh name table and declaration cache.
        self.lh = Lh::new();
        let mut differences = Vec::new();
        for ((case, rust, what), cpp) in batch.iter().zip(&answers) {
            if *cpp == *rust {
                continue;
            }
            let (cpp_body, _) = split_answer(&cpp.1);
            let (rust_body, _) = split_answer(&rust.1);
            if cpp.0 != rust.0 || cpp_body != rust_body {
                differences.push(explain(case, &(cpp.0, cpp_body), &(rust.0, rust_body), what));
            } else if let Some(calls) = compare_calls(&cpp.1, &rust.1) {
                differences.push(format!("{calls}\n  in {}", what.chars().take(4000).collect::<String>()));
            }
            if differences.len() >= 3 {
                break;
            }
        }
        assert!(differences.is_empty(), "{}: answers differ, the first:\n{}", self.name, differences.join("\n"));
    }

    fn finish(mut self) -> RenderTally {
        self.flush();
        let compared = if self.oracle.is_some() { "alike with lhdump" } else { "built (no oracle)" };
        eprintln!("{}: {} cases {compared}; {}", self.name, self.cases, self.tally.report());
        self.tally
    }
}

/// Where two answers part.
fn explain(case: &[u8], cpp: &(u8, Vec<u8>), rust: &(u8, Vec<u8>), what: &str) -> String {
    let at = cpp.1.iter().zip(&rust.1).position(|(a, b)| a != b).unwrap_or(cpp.1.len().min(rust.1.len()));
    let marks = {
        start_marks();
        // Again with a fresh name table: the same fields at the same places but the names the case interned.
        render_answer(&Lh::new(), Switches::default(), case, &mut RenderTally::default());
        take_marks()
    };
    let item = marks.iter().rfind(|m| m.0 <= at && m.2 >= 0).map_or(-1, |m| m.2);
    let field = marks.iter().rfind(|m| m.0 <= at).map_or("", |m| m.1);
    // The first dump's items, where they differ.
    let (c, r) = (describe_items(cpp.1.get(1..).unwrap_or_default()), describe_items(rust.1.get(1..).unwrap_or_default()));
    let items = match c.iter().zip(&r).position(|(a, b)| a != b) {
        Some(k) => format!("\n  item C++  {}\n  item Rust {}", c[k], r[k]),
        None => String::new(),
    };
    let near = |b: &[u8]| {
        let piece = &b[at.saturating_sub(16).min(b.len())..(at + 80).min(b.len())];
        format!("{} {:?}", hex(&piece[..piece.len().min(48)]), String::from_utf8_lossy(piece))
    };
    format!(
        "status C++ {} / Rust {}, {} / {} bytes, first difference at byte {at}, {field} (item or element {item}): C++ {} / Rust {}{items}\n  in {}",
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

const DISPLAYS: [&str; 21] = [
    "block",
    "inline",
    "inline-block",
    "list-item",
    "table",
    "inline-table",
    "table-row-group",
    "table-header-group",
    "table-footer-group",
    "table-row",
    "table-cell",
    "table-caption",
    "table-column",
    "table-column-group",
    "flex",
    "inline-flex",
    "grid",
    "inline-grid",
    "none",
    "contents",
    "inline-text",
];

/// A declaration the render tree reads: displays, floats, positions, list styles, white space, the box's lengths in
/// every unit, table properties.
fn tree_declaration(random: &mut Lcg) -> String {
    match random.below(12) {
        0..=3 => format!("display:{}", pick(random, &DISPLAYS)),
        4 => format!("float:{}", pick(random, &["left", "right", "none"])),
        5 => format!("position:{}", pick(random, &["absolute", "fixed", "relative", "static", "sticky"])),
        6 => format!(
            "list-style-type:{}",
            pick(random, &["decimal", "disc", "none", "lower-roman", "upper-alpha", "armenian", "square", "georgian"])
        ),
        7 => format!("white-space:{}", pick(random, &["pre", "normal", "nowrap", "pre-wrap", "pre-line"])),
        8 => format!(
            "{}:{}",
            pick(random, &["margin", "padding", "border", "margin-left", "padding-top", "border-left-width", "border-bottom"]),
            pick(
                random,
                &[
                    "1px",
                    "2em",
                    "3pt",
                    "10%",
                    "1rem",
                    "5vw",
                    "calc(1px + 1em)",
                    "auto",
                    "thin",
                    "medium solid",
                    "4px solid red",
                    "0",
                    "2ex",
                    "1in"
                ]
            )
        ),
        9 => format!(
            "{}:{}",
            pick(random, &["border-collapse", "border-spacing", "width", "caption-side", "table-layout"]),
            pick(random, &["collapse", "separate", "2px", "3px 1em", "50px", "40%", "bottom", "fixed", "auto"])
        ),
        10 => format!("font-size:{}", pick(random, &["10px", "2em", "large", "0", "150%"])),
        _ => format!("list-style-image:{}", pick(random, &["url(a.png)", "none"])),
    }
}

/// A generated document: tables (with and without their row groups, rows and cells, captions, columns, spans), lists
/// (`start`, item displays), inlines holding blocks, flex and grid containers of text and inline boxes, images and
/// SVGs, `<details>`, any display on any element, and `::before`/`::after` boxes, under a sheet whose rules change
/// displays across a media query.
fn tree_document(random: &mut Lcg) -> Vec<u8> {
    fn element(random: &mut Lcg, depth: u32, out: &mut String) {
        let tags = [
            "div", "span", "p", "ul", "ol", "li", "table", "caption", "colgroup", "col", "thead", "tbody", "tfoot", "tr", "td", "th",
            "img", "svg", "b", "a", "button", "details", "summary", "section", "pre", "br", "em", "i", "input", "label",
        ];
        let tag = pick(random, &tags);
        out.push('<');
        out.push_str(tag);
        for _ in 0..random.below(3) {
            let attr = match random.below(8) {
                0 => format!(" class=\"{}\"", pick(random, &["a", "b", "c", "a b", "t"])),
                1 => format!(" colspan=\"{}\"", pick(random, &["2", "3", "0", "-1", "1001", "x"])),
                2 => format!(" rowspan=\"{}\"", pick(random, &["2", "3", "0", "1000"])),
                3 => format!(" start=\"{}\"", pick(random, &["3", "-2", "2147483647", "x", "0"])),
                4 => " open".to_string(),
                5 => format!(" width=\"{}\"", pick(random, &["10", "50%", "0"])),
                6 => format!(" border=\"{}\"", pick(random, &["1", "0", "3"])),
                _ => format!(" id=\"{}\"", pick(random, &["i", "j"])),
            };
            out.push_str(&attr);
        }
        if random.chance(2) {
            let decls: Vec<String> = (0..1 + random.below(3)).map(|_| tree_declaration(random)).collect();
            out.push_str(&format!(" style=\"{}\"", decls.join(";")));
        }
        out.push('>');
        if matches!(tag, "img" | "br" | "col" | "input") {
            return;
        }
        let n = if depth > 5 { 0 } else { random.below(5) };
        for _ in 0..n {
            match random.below(9) {
                0 => out.push_str(pick(random, &["text", " ", "\n  ", "a b", "x ", " y", "\t"])),
                1 => out.push_str("<!-- c -->"),
                _ => element(random, depth + 1, out),
            }
        }
        out.push_str(&format!("</{tag}>"));
    }
    let mut out = String::new();
    if !random.chance(6) {
        out.push_str("<!DOCTYPE html>");
    }
    out.push_str("<html><head><style>");
    for _ in 0..random.below(6) {
        let selector = pick(random, &["div", "span", ".a", ".b", "li", "td", "tr", "table", "p", "ul > li", ".c", "*", "section"]);
        let pseudo = pick(random, &["", "", "", "::before", "::after", ":hover"]);
        let mut decls: Vec<String> = (0..1 + random.below(3)).map(|_| tree_declaration(random)).collect();
        if pseudo.starts_with("::") {
            decls.push(format!("content:{}", pick(random, &["'x'", "''", "counter(list-item)", "url(i.png)", "\"a b\""])));
        }
        let rule = format!("{selector}{pseudo}{{{}}}", decls.join(";"));
        if random.chance(3) {
            out.push_str(&format!("@media (min-width:{}px){{{rule}}}", pick(random, &["400", "500", "800"])));
        } else {
            out.push_str(&rule);
        }
    }
    out.push_str("</style></head><body>");
    for _ in 0..1 + random.below(4) {
        element(random, 0, &mut out);
    }
    out.push_str("</body></html>");
    out.into_bytes()
}

/// Shapes the generator meets rarely, each written out.
fn written_documents() -> Vec<(&'static str, String)> {
    let page = |body: &str| format!("<!DOCTYPE html><html><head></head><body>{body}</body></html>");
    let styled = |css: &str, body: &str| format!("<!DOCTYPE html><html><head><style>{css}</style></head><body>{body}</body></html>");
    vec![
        ("a block inside an inline", page("<span>a<div>b</div>c</span>")),
        ("a block deep inside inlines", page("<span>a<b>b<i>c<div>d</div>e</i>f</b>g</span>")),
        ("blocks inside an inline-block inside an inline", page("<span>a<button>b<div>c</div>d</button>e</span>")),
        (
            "a block inside a table inside an inline",
            page("<span>a<table style=\"display:inline-table\"><tr><td>x<div>y</div></td></tr></table>b</span>"),
        ),
        ("two blocks in one inline", page("<span>a<div>b</div>c<p>d</p>e</span>")),
        ("a float in an inline", page("<span>a<div style=\"float:left\">b</div>c</span>")),
        ("text between blocks", page("<div>a</div> text <div>b</div>   <div>c</div>")),
        ("pre spaces between blocks", page("<div style=\"white-space:pre\"><div>a</div>   <div>b</div></div>")),
        ("a flex of text and boxes", page("<div style=\"display:flex\">  a b <span>c</span> d <div>e</div>  <b>f</b>   </div>")),
        ("a flex of pre spaces", page("<div style=\"display:flex;white-space:pre\">   <span>c</span>   </div>")),
        ("an inline flex in a flex", page("<div style=\"display:flex\"><span style=\"display:inline-flex\">a<div>b</div></span></div>")),
        (
            "an inline table in a flex",
            page("<div style=\"display:flex\"><table style=\"display:inline-table\"><tr><td>a</td></tr></table></div>"),
        ),
        (
            "an inline-block in a grid",
            page("<div style=\"display:grid\"><span style=\"display:inline-block\">a<div>b</div></span> t</div>"),
        ),
        (
            "cells without a table",
            page("<div><span style=\"display:table-cell\">a</span><span style=\"display:table-cell\">b</span></div>"),
        ),
        ("rows without a table", page("<div style=\"display:table-row\">a</div><div style=\"display:table-row\">b</div>")),
        ("a row group without a table", page("<div style=\"display:table-row-group\"><div style=\"display:table-row\">x</div></div>")),
        ("a table of loose content", page("<div style=\"display:table\">a<span>b</span><div>c</div></div>")),
        (
            "a table with a caption among its rows",
            page(
                "<div style=\"display:table\"><div style=\"display:table-row\">a</div><div style=\"display:table-caption\">c</div><div style=\"display:table-row\">b</div>x</div>",
            ),
        ),
        (
            "header and footer groups",
            page("<table><thead><tr><td>h</td></tr></thead><tfoot><tr><td>f</td></tr></tfoot><tbody><tr><td>b</td></tr></tbody></table>"),
        ),
        (
            "a row group inside a row",
            page(
                "<div style=\"display:table\"><div style=\"display:table-row\"><div style=\"display:table-row-group\"><div style=\"display:table-cell\">a</div></div><div style=\"display:table-cell\">b</div></div></div>",
            ),
        ),
        (
            "spans",
            page(
                "<table><tr><td rowspan=3>a</td><td colspan=2>b</td></tr><tr><td>c</td></tr><tr><td colspan=3>d</td><td>e</td></tr></table>",
            ),
        ),
        (
            "cell widths",
            page(
                "<table><tr><td width=10>a</td><td style=\"width:30%\">b</td><td colspan=2 width=5>c</td></tr><tr><td>d</td><td width=7>e</td></tr></table>",
            ),
        ),
        (
            "captions and columns",
            page("<table><caption>c</caption><colgroup><col><col></colgroup><tr><td>a</td></tr><caption>d</caption></table>"),
        ),
        (
            "separate and collapsed spacing",
            styled(
                "table{border-spacing:3px 1em} .c{border-collapse:collapse}",
                "<table><tr><td>a</td></tr></table><table class=c><tr><td>b</td></tr></table>",
            ),
        ),
        (
            "cell borders",
            styled(
                "td{border:2px solid} .t td{border-left-width:5px}",
                "<table class=t><tr><td>a</td><td style=\"border:1px solid\">b</td></tr><tr><td>c</td><td>d</td></tr></table>",
            ),
        ),
        (
            "lists",
            page(
                "<ol start=5><li>a</li><li>b</li><li style=\"display:block\">c</li><li>d</li></ol><ul><li>x</li></ul><ol start=2147483646><li>a</li><li>b</li><li>c</li></ol>",
            ),
        ),
        (
            "list styles",
            styled(
                "ol{list-style-type:lower-roman} .n{list-style-type:none} .d li{display:list-item}",
                "<ol><li>a</li><li class=n>b</li></ol><div class=d><li>x</li><li>y</li></div>",
            ),
        ),
        ("list items outside lists", page("<li>a</li><li>b</li><div style=\"display:list-item;list-style-type:decimal\">c</div>")),
        (
            "closed and open details",
            page("<details><summary>s</summary>a<summary>t</summary>b</details><details open><summary>s</summary>c</details>"),
        ),
        (
            "images and svg not displayed",
            page("<img style=\"display:none\" src=a.png><svg style=\"display:none\"><rect/></svg><img src=b.png>"),
        ),
        ("absolute and fixed", page("<span>a<div style=\"position:absolute\">b</div><td style=\"position:fixed\">c</td></span>")),
        (
            "generated boxes",
            styled(
                "p::before{content:'x';display:block} p::after{content:'y';display:table-cell} q::before{content:'z'}",
                "<p>a</p><q>b</q>",
            ),
        ),
        ("a table cell display on everything", styled("*{display:table-cell}", "<div>a<span>b</span></div>")),
        ("a table row display on everything", styled("body *{display:table-row}", "<div>a<span>b</span><p>c</p></div>")),
        (
            "display changes across a media query",
            styled(
                "@media (min-width:500px){.a{display:table-cell}.b{display:none}.c{display:flex}} .b{display:table-row}",
                "<div class=a>1</div><div class=b>2</div><div class=c>3 <span>4</span></div><table><tr><td class=b>x</td></tr></table>",
            ),
        ),
        ("text in a table", page("<table>a<tr>b<td>c</td>d</tr>e</table>")),
        ("a style and a script in the body", page("<div>a<script>x</script><style>p{}</style>b</div>")),
        (
            "a list-style-image on a list item",
            styled("li{list-style-image:url(m.png)}", "<ul><li>a<table><tr><td>b</td></tr></table></li></ul>"),
        ),
    ]
}

/// A table that asks more cells of the budget than there are: spans, and rows filled out to the widest one.
fn budget_documents() -> Vec<(&'static str, String)> {
    let page = |body: String| format!("<!DOCTYPE html><html><head></head><body>{body}</body></html>");
    let mut wide = String::from("<table><tr>");
    for _ in 0..300 {
        wide += "<td colspan=1000>a</td>";
    }
    wide += "</tr><tr><td>b</td></tr></table>";
    let mut rows = String::from("<table>");
    for k in 0..600 {
        rows += "<tr>";
        for _ in 0..(if k == 0 { 400 } else { 1 }) {
            rows += "<td>c</td>";
        }
        rows += "</tr>";
    }
    rows += "</table>";
    vec![
        ("spans past the budget", page(wide)),
        ("rows filled out past the budget", page(rows)),
        ("one huge cell", page("<table><tr><td colspan=1000 rowspan=1000>x</td><td>y</td></tr></table>".to_string())),
    ]
}

fn tree_steps(other: (f32, f32)) -> Vec<Step> {
    vec![Step::Dump, Step::Media(other.0, other.1), Step::Dump, Step::Hover(3), Step::Dump, Step::Media(375.0, 667.0), Step::Dump]
}

// ---- The comparison ----------------------------------------------------------------------------------------

#[test]
fn the_corpus_builds_alike() {
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
                steps.extend([Step::Hover(t), Step::Active(t), Step::Dump, Step::Media(width, height), Step::Dump]);
            }
            let theme = if dark_theme { "dark" } else { "light" };
            run.case(render_record(width, height, 2.0, master, b"", &html, &steps), format!("{name} ({theme})"));
        }
    }
    let tally = run.finish();
    assert!(tally.items > 100_000);
}

#[test]
fn every_tag_builds_alike() {
    let mut run = Run::new("every tag");
    let master = master_css();
    let (light, _) = themed_master(false);
    let (dark, vars) = themed_master(true);
    let steps = tree_steps((1200.0, 800.0));
    for quirks in [false, true] {
        let html = every_tag_document(quirks);
        let what = if quirks { "every tag in quirks mode" } else { "every tag" };
        run.case(render_record(375.0, 667.0, 1.0, &master, b"", &html, &steps), format!("{what}, master CSS"));
        run.case(render_record(375.0, 667.0, 2.0, &light, b"", &html, &steps), format!("{what}, light UA layers"));
        run.case(render_record(412.0, 915.0, 3.0, &dark, b"", &themed_html(&html, true, &vars), &steps), format!("{what}, dark UA layers"));
        run.case(render_record(375.0, 667.0, 2.0, b"", b"", &html, &steps), format!("{what}, no sheet"));
    }
    run.finish();
}

#[test]
fn written_shapes_build_alike() {
    let mut run = Run::new("written shapes");
    let master = master_css();
    let (light, _) = themed_master(false);
    for (what, html) in written_documents().into_iter().chain(budget_documents()) {
        for sheet in [&master, &light] {
            run.case(render_record(375.0, 667.0, 2.0, sheet, b"", html.as_bytes(), &tree_steps((900.0, 700.0))), what);
        }
    }
    let tally = run.finish();
    assert!(tally.counts.contains_key("table limit met"), "the budget was crossed");
}

#[test]
fn generated_documents_build_alike() {
    let mut run = Run::new("generated documents");
    let mut random = Lcg::new(0x16_0001);
    let master = master_css();
    let (light, _) = themed_master(false);
    let only: Option<usize> = std::env::var("RENDER_ONLY").ok().and_then(|v| v.parse().ok());
    for k in 0..2500 {
        let html = tree_document(&mut random);
        let width = [320.0, 375.0, 600.0, 1000.0][k % 4];
        let other = [900.0, 450.0, 300.0, 520.0][(k / 4) % 4];
        if only.is_some_and(|only| only != k) {
            continue;
        }
        let sheet = if k % 2 == 0 { &master } else { &light };
        let steps = vec![
            Step::Dump,
            Step::Media(other, 640.0),
            Step::Dump,
            Step::Hover(k as u32),
            Step::Dump,
            Step::Media(width, 640.0),
            Step::Refresh,
            Step::Dump,
        ];
        run.case(render_record(width, 640.0, 2.0, sheet, b"", &html, &steps), format!("generated {k}: {}", String::from_utf8_lossy(&html)));
    }
    let tally = run.finish();
    for kind in ["block", "block context", "inline context", "inline", "text", "image", "table", "table part", "table row", "flex", "grid"]
    {
        assert!(tally.kinds.contains_key(kind), "no {kind} item was made");
    }
}

/// A document 127 levels deep of inlines around blocks and of tables builds its tree on a thread whose stack is the
/// size the cascade's measure asked (1 MiB unoptimized, 512 KiB optimized), as the helper's threads will run it.
#[test]
fn a_deep_document_builds_on_a_small_stack() {
    let stack = if cfg!(debug_assertions) { 1 << 20 } else { 512 << 10 };
    let thread = std::thread::Builder::new().stack_size(stack).spawn(|| {
        let mut html = String::from("<!DOCTYPE html><html><head></head><body>");
        for k in 0..124 {
            html += match k % 4 {
                0 => "<span>a",
                1 => "<div style=\"display:table-cell\">",
                2 => "<div style=\"display:flex\">b",
                _ => "<em>",
            };
        }
        html += "<div>deep</div>";
        for k in (0..124).rev() {
            html += match k % 4 {
                0 => "</span>",
                1 | 2 => "</div>",
                _ => "</em>",
            };
        }
        html += "</body></html>";
        let lh = Lh::new();
        let mut tally = RenderTally::default();
        let input = render_record(375.0, 667.0, 2.0, &master_css(), b"", html.as_bytes(), &[Step::Dump, Step::Media(900.0, 600.0)]);
        let answer = render_answer(&lh, Switches::default(), &input, &mut tally);
        assert_eq!(answer.0, 1);
        tally.items
    });
    let items = thread.expect("a thread").join().expect("the tree built");
    eprintln!("a deep document: {items} items");
    assert!(items > 300);
}
