//! Every limit and budget of the fork (stage L17), each crossed: just under it, at it and just past it, through
//! `oracle/lhdump` and the port. `LIMITS.md` lists them all with where each lives in the C++ and the Rust and which
//! test crosses it; of the limits other stages' tests already crossed, only those whose boundary they missed are
//! crossed again here.
//!
//! A limit's cases run as one lhdump process (a fresh name table and declaration cache, as a fresh Rust `Lh`), each a
//! record of the stage that reaches the limit: `Q` (a document cascaded, built, laid out and painted, every
//! container call compared, `sanitizer_limit`'s among them), `j` (Gumbo's parse), `b` (patches) or `W` (hit testing). Each test also
//! checks, where the dumps or the port's counters show it, that the limit was met on the far side only, so a case that
//! misses its limit fails.

mod common;

use common::cascade::*;
use common::hit::*;
use common::paint::*;
use common::patch::*;
use common::*;
use twidget_litehtml::document::Switches;

/// One limit's cases through lhdump and the port, in one process.
struct Limit {
    name: &'static str,
    lh: Lh,
    input: Vec<u8>,
    cases: Vec<((u8, Vec<u8>), String)>,
    paint: PaintTally,
    patch: PatchTally,
    hit: HitTally,
    oracle: Option<Oracle>,
}

impl Limit {
    fn new(name: &'static str) -> Limit {
        Limit {
            name,
            lh: Lh::new(),
            input: Vec::new(),
            cases: Vec::new(),
            paint: PaintTally::default(),
            patch: PatchTally::default(),
            hit: HitTally::default(),
            oracle: Oracle::find("lhdump"),
        }
    }

    fn push(&mut self, op: u8, record: &[u8], answer: (u8, Vec<u8>), what: String) -> (u8, Vec<u8>) {
        self.input.extend(framed(op, record));
        self.cases.push((answer.clone(), what));
        answer
    }

    /// A `Q` record: a document under the master sheet at 375 × 667, laid out, dumped and painted.
    fn document(&mut self, html: &[u8], steps: &[Step], what: impl Into<String>) -> (u8, Vec<u8>) {
        let record = paint_record(375.0, 667.0, 2.0, &master_css(), b"", html, steps);
        let answer = paint_answer(&self.lh, Switches::default(), &record, &mut self.paint);
        self.push(b'Q', &record, answer, what.into())
    }

    /// A document with the default steps: a layout, a dump and a paint of the viewport.
    fn page(&mut self, html: &[u8], what: impl Into<String>) -> (u8, Vec<u8>) {
        self.document(html, &default_steps(), what)
    }

    /// A `j` record: Gumbo's parse with the default options.
    fn gumbo(&mut self, text: &[u8], what: impl Into<String>) -> (u8, Vec<u8>) {
        let record = common::gumbo::record(&common::gumbo::Options::default(), text);
        let answer = common::gumbo::answer(&record).0;
        self.push(b'j', &record, answer, what.into())
    }

    /// A `b` record: a document under the master sheet, then patches.
    fn patches(&mut self, html: &[u8], steps: &[PatchStep], what: impl Into<String>) -> (u8, Vec<u8>) {
        let record = patch_record(375.0, 667.0, 2.0, false, &master_css(), html, steps);
        let answer = patch_answer(&self.lh, Switches::default(), &record, &mut self.patch);
        self.push(b'b', &record, answer, what.into())
    }

    /// A `W` record: a document under the master sheet, then pointer steps (stage L16).
    fn hits(&mut self, html: &[u8], steps: &[HitStep], what: impl Into<String>) -> (u8, Vec<u8>) {
        let record = hit_record(375.0, 667.0, 2.0, false, &master_css(), html, steps);
        let answer = hit_answer(&self.lh, Switches::default(), &record, &mut self.hit);
        self.push(b'W', &record, answer, what.into())
    }

    /// Runs the cases through lhdump (when it is there) and fails on the first that differs.
    fn finish(self) -> (PaintTally, PatchTally) {
        let n = self.cases.len();
        if let Some(oracle) = &self.oracle {
            let answers = oracle.run(self.input, n);
            let mut differences = Vec::new();
            for (cpp, (rust, what)) in answers.iter().zip(&self.cases) {
                if cpp != rust {
                    let at = cpp.1.iter().zip(&rust.1).position(|(a, b)| a != b).unwrap_or(cpp.1.len().min(rust.1.len()));
                    let near = |b: &[u8]| hex(&b[at.saturating_sub(16).min(b.len())..(at + 32).min(b.len())]);
                    differences.push(format!(
                        "{what}: status C++ {} / Rust {}, {} / {} bytes, first difference at byte {at}:\n  C++  {}\n  Rust {}",
                        cpp.0,
                        rust.0,
                        cpp.1.len(),
                        rust.1.len(),
                        near(&cpp.1),
                        near(&rust.1)
                    ));
                }
            }
            assert!(
                differences.is_empty(),
                "{}: {} of {n} answers differ, the first:\n{}",
                self.name,
                differences.len(),
                differences[..differences.len().min(3)].join("\n")
            );
            eprintln!("{}: {n} cases alike with lhdump", self.name);
        } else {
            eprintln!("{}: {n} cases (no oracle)", self.name);
        }
        (self.paint, self.patch)
    }
}

fn default_steps() -> Vec<Step> {
    vec![Step::Layout(375.0, None), Step::Dump, Step::Paint(Box::new(PaintStep::viewport(375.0, 667.0)))]
}

fn page(css: &str, body: &str) -> Vec<u8> {
    format!("<!DOCTYPE html><html><head><style>{css}</style></head><body>{body}</body></html>").into_bytes()
}

/// The container's calls of an answer, every phase.
fn calls(answer: &(u8, Vec<u8>)) -> Vec<Call> {
    split_answer(&answer.1).1.into_iter().flatten().collect()
}

/// Whether the document met a limit the container is told of (`sanitizer_limit(what)`).
fn met(answer: &(u8, Vec<u8>), what: &[u8]) -> bool {
    calls(answer).iter().any(|c| c.0 == b'L' && c.1 == what)
}

/// The answer's body (the dumps), without the container's calls.
fn body(answer: &(u8, Vec<u8>)) -> Vec<u8> {
    split_answer(&answer.1).0
}

/// A document as a `Q` record makes it (finalized under the master sheet at 375 × 667), for `inspect` to read what the
/// port counted: where a limit's boundary falls for a shape of document.
fn with_document<T>(html: &[u8], inspect: impl FnOnce(&mut twidget_litehtml::Document, &twidget_litehtml::document::StyleCx) -> T) -> T {
    let lh = Lh::new();
    let container = CascadeContainer::new(375.0, 667.0, 2.0);
    let cx =
        twidget_litehtml::document::StyleCx { names: &lh.names, container: &container, cache: &lh.cache, switches: Switches::default() };
    let (mut doc, root) = twidget_litehtml::Document::build(html, &cx.env());
    doc.finalize_from_external_root(&cx, root, &master_css(), b"");
    inspect(&mut doc, &cx)
}

/// `{open}` × n, the text, `{close}` × n.
fn nest(open: &str, n: usize, inner: &str, close: &str) -> String {
    [open.repeat(n), inner.to_string(), close.repeat(n)].concat()
}

// ---- CSS ---------------------------------------------------------------------------------------------------------

/// `kMaxNesting` (css_parser.cpp): a block or function 128 deep is consumed, one deeper is skipped whole, in a style
/// attribute (blocks from its top level) and in a sheet (inside the rule's block).
#[test]
fn css_nesting_crosses_128() {
    let mut limit = Limit::new("CSS nesting (128)");
    let mut bodies = Vec::new();
    for n in 125..=130 {
        let value = nest("(", n, "a", ")");
        let a = limit.page(&page("", &format!("<div style=\"--x:{value};width:5px\">a</div>")), format!("a style attribute {n} deep"));
        bodies.push(body(&a));
        let f = nest("calc(", n, "1px", ")");
        limit.page(&page(&format!("div{{--y:{value};--z:{f}}}"), "<div>b</div>"), format!("a sheet's value {n} deep"));
        limit.page(&page(&format!("div{{--w:{}}}", nest("[", n, "b", "]")), "<div>c</div>"), format!("square blocks {n} deep"));
    }
    limit.finish();
    assert!(bodies.windows(2).all(|w| w[0] != w[1]), "each level shows in the dump");
}

/// `kMaxCompounds` (css_selector.cpp): a selector of 128 compounds is kept, of 129 refused, counted across the lists
/// nested in `:is()` and `:not()`.
#[test]
fn selector_compounds_cross_128() {
    let mut limit = Limit::new("selector compounds (128)");
    let siblings = "<p>s</p>".repeat(140);
    let mut reached = Vec::new();
    for n in 126..=130 {
        let chain = vec!["p"; n].join(" + ");
        let a = limit.page(&page(&format!("{chain}{{color:red}}"), &siblings), format!("{n} compounds"));
        reached.push(body(&a));
        let nested = nest(":is(", n - 1, "p", ")");
        limit.page(&page(&format!("{nested}{{color:blue}}"), &siblings), format!("{n} compounds through :is()"));
        let nots = nest("p:not(", n - 1, "q", ")");
        limit.page(&page(&format!("{nots}{{color:green}}"), &siblings), format!("{n} compounds through :not()"));
        let list = (0..n).map(|k| format!("x{k}")).collect::<Vec<_>>().join(",");
        limit.page(&page(&format!(":is({list}),p{{color:navy}}"), &siblings), format!("a list of {n}"));
    }
    limit.finish();
    assert!(reached[2] != reached[3], "the 129th compound refuses the selector");
    assert!(reached[0] != reached[1] && reached[1] != reached[2], "128 compounds still match");
}

/// calc()'s `MAX_NODES` (css_length.cpp, 128 nodes in the tree) and its atoms' depth (more than 16 refused), and
/// `css_effects.cpp`'s depth of 16 in the math functions and the values that hold them.
#[test]
fn calc_nodes_and_depths_cross_their_limits() {
    let mut limit = Limit::new("calc nodes (128) and depths (16)");
    let mut widths = Vec::new();
    for n in 60..=68 {
        let sum = vec!["1px"; n].join(" + ");
        let a = limit.page(&page("", &format!("<div style=\"width:calc({sum})\">a</div>")), format!("calc of {n} terms"));
        widths.push(body(&a));
        let product = format!("1px{}", " * 1".repeat(n));
        limit.page(&page("", &format!("<div style=\"width:calc({product})\">a</div>")), format!("calc of a product of {n}"));
        let mixed = format!("{} + 2px * 2", vec!["(1px - 0px)"; n / 3].join(" + "));
        limit.page(&page("", &format!("<div style=\"height:calc({mixed})\">a</div>")), format!("calc of {} groups", n / 3));
    }
    for d in 12..=20 {
        let deep = nest("(", d, "1px", ")");
        limit.page(&page("", &format!("<div style=\"width:calc({deep} + 1px)\">a</div>")), format!("calc {d} deep"));
        let fns = nest("min(", d, "3px", ", 9px)");
        limit.page(&page("", &format!("<div style=\"width:calc({fns});margin-left:{fns}\">a</div>")), format!("min() {d} deep"));
        let rotate = format!("rotate(calc({}))", nest("(", d, "10deg", ")"));
        let blur = nest("calc(", d, "1px", ")");
        limit.page(
            &page("", &format!("<div style=\"transform:{rotate};filter:blur({blur})\">a</div>")),
            format!("a transform's math {d} deep"),
        );
        let clamp = nest("clamp(1px, ", d, "5px", ", 9px)");
        let half = nest("calc(", d, "0.5", ")");
        limit.page(&page("", &format!("<div style=\"padding:{clamp};opacity:{half}\">a</div>")), format!("clamp() {d} deep"));
    }
    limit.finish();
    assert!(widths.first() != widths.last(), "the node limit refuses a calc()");
}

/// `max_var_substitution_tokens` (16384 tokens an element's declarations may expand to) and the depth of a var()
/// chain (more than 32 invalid), style.cpp.
#[test]
fn var_tokens_and_depth_cross_their_limits() {
    let mut limit = Limit::new("var() tokens (16384) and depth (32)");
    let mut bodies = Vec::new();
    for n in 16382..=16386 {
        let big = vec!["a"; n].join(" ");
        let a = limit.page(&page("", &format!("<div style=\"--big:{big};--x:var(--big);width:5px\">a</div>")), format!("{n} tokens"));
        bodies.push(body(&a));
    }
    // Two declarations share the element's budget.
    for n in [8191usize, 8192, 8193] {
        let big = vec!["b"; n].join(" ");
        limit.page(&page("", &format!("<div style=\"--big:{big};--x:var(--big);--y:var(--big)\">a</div>")), format!("twice {n} tokens"));
    }
    let chain: String = (0..44).map(|k| format!("--v{k}:var(--v{});", k + 1)).collect::<String>() + "--v44:7px";
    let mut divs = String::new();
    for k in 0..16 {
        divs += &format!("<div style=\"width:var(--v{k});--c{k}:var(--v{k})\">{k}</div>");
    }
    limit.page(&page(&format!("body{{{chain}}}"), &divs), "var() chains 44 to 29 deep");
    limit.finish();
    assert!(bodies[2] != bodies[3], "16385 tokens are past the budget");
}

/// `max_tracks` (css_effects.cpp): a track list holds 1000 tracks at most, written out or from `repeat()`.
#[test]
fn grid_tracks_cross_1000() {
    let mut limit = Limit::new("grid tracks (1000)");
    let mut bodies = Vec::new();
    for n in 998..=1002 {
        let grid = |columns: &str| format!("div.g{{display:grid;grid-template-columns:{columns}}}");
        let a = limit.page(&page(&grid(&format!("repeat({n},1px)")), "<div class=\"g\"><i>a</i><b>b</b></div>"), format!("repeat({n})"));
        bodies.push(body(&a));
        let list = vec!["1px"; n].join(" ");
        limit.page(&page(&grid(&list), "<div class=\"g\"><i>a</i></div>"), format!("{n} tracks written"));
        let half = vec!["2px"; n / 2].join(" ");
        let rows = format!("div.g{{display:grid;grid-template-columns:3px repeat(2,{half}) 4px;grid-template-rows:{list}}}");
        limit.page(&page(&rows, "<div class=\"g\"><i>a</i></div>"), format!("a repeat of {} and more", n / 2));
        limit.page(
            &page(&grid(&format!("{list} repeat(auto-fill,5px)")), "<div class=\"g\"><i>a</i></div>"),
            format!("{n} tracks and auto-fill"),
        );
    }
    limit.finish();
    assert!(bodies[1] != bodies[2], "a 1000th track is kept");
}

/// `max_used_styles` (html_tag.cpp): an element keeps 256 matched selectors at most; the 257th and later are not
/// applied.
#[test]
fn used_styles_cross_256() {
    let mut limit = Limit::new("used styles (256)");
    let mut bodies = Vec::new();
    for n in 248..=262 {
        let rules: String = (0..n).map(|k| format!("div{{--a{k}:{k}}}")).collect();
        let a = limit.page(&page(&rules, "<div>a</div><div class=\"x\">b</div>"), format!("{n} rules"));
        bodies.push(body(&a));
    }
    limit.finish();
    let _ = bodies;
    let used = |n: usize| {
        let rules: String = (0..n).map(|k| format!("div{{--a{k}:{k}}}")).collect();
        with_document(&page(&rules, "<div>a</div>"), |doc, cx| {
            let div = (0..doc.elements.len() as u32)
                .map(twidget_litehtml::ElementId)
                .find(|&e| &cx.names.s(doc.el(e).tag)[..] == b"div")
                .unwrap();
            doc.el(div).used_styles.len()
        })
    };
    let counts: Vec<usize> = [248, 262].iter().map(|&n| used(n)).collect();
    assert!(counts[0] < 256 && counts[1] == 256, "the used selectors stop at 256: {counts:?}");
}

/// `max_names` (element.cpp): a `counter-reset`, `counter-set` or `counter-increment` takes 16 names at most.
#[test]
fn counter_names_cross_16() {
    let mut limit = Limit::new("counter names (16)");
    let mut bodies = Vec::new();
    for n in 14..=18 {
        let names: String = (1..=n).map(|k| format!(" c{k} {k}")).collect();
        let content: String = (1..=n).map(|k| format!(" counter(c{k})")).collect();
        let increments: String = (1..=n).map(|k| format!(" c{k}")).collect();
        let css = format!(
            "p{{counter-reset:{names}}}p::before{{content:{content}}}li{{counter-increment:{increments}}}li::after{{content:{content}}}"
        );
        let a = limit.page(&page(&css, "<p>x</p><ul><li>a</li><li>b</li></ul>"), format!("{n} counter names"));
        bodies.push(body(&a));
        let set = format!("p{{counter-set:{names}}}p::after{{content:{content}}}");
        limit.page(&page(&set, "<p>y</p>"), format!("counter-set of {n} names"));
    }
    limit.finish();
    let _ = bodies;
}

// ---- Generated content ---------------------------------------------------------------------------------------

/// `max_generations` (el_before_after.h): a box makes its content 8 times at most between two styles, so the ninth
/// rule that gives it content changes nothing.
#[test]
fn content_generations_cross_8() {
    let mut limit = Limit::new("content generations (8)");
    let mut texts = Vec::new();
    for n in 6..=10 {
        let rules: String = (1..=n).map(|k| format!("p::before{{content:\"{} \"}}", "k".repeat(k))).collect();
        let a = limit.page(&page(&rules, "<p>x</p>"), format!("{n} rules with content"));
        texts.push(body(&a));
        let images: String = (1..=n).map(|k| format!("p::after{{content:url(i{k}.png) \"t\"}}")).collect();
        limit.page(&page(&images, "<p>y</p>"), format!("{n} rules with images"));
    }
    limit.finish();
    let generated = |n: usize| {
        let rules: String = (1..=n).map(|k| format!("p::before{{content:\"{} \"}}", "k".repeat(k))).collect();
        with_document(&page(&rules, "<p>x</p>"), |doc, _| doc.generated_bytes)
    };
    assert_eq!(generated(8), generated(9), "a ninth generation is not made");
    assert_ne!(generated(7), generated(8), "an eighth is");
    let _ = texts;
}

/// `max_generated_box` (32 KiB a word or image url of generated content), `max_generated_bytes` (1 MiB a document)
/// and `max_generated_boxes` (20000 a document), document.h: past one, `sanitizer_limit("content")` and no more
/// content in the document.
#[test]
fn generated_content_crosses_its_limits() {
    let mut limit = Limit::new("generated content (32 KiB, 1 MiB, 20000 boxes)");
    let mut box_met = Vec::new();
    for n in [32766usize, 32767, 32768, 32769, 32770] {
        let word = "w".repeat(n);
        let a = limit.page(&page(&format!("p::before{{content:\"{word}\"}}"), "<p>x</p><p>y</p>"), format!("a word of {n} bytes"));
        box_met.push(met(&a, b"content"));
        let url = format!("u{}.png", "v".repeat(n - 6));
        limit.page(&page(&format!("p::before{{content:url({url})}}"), "<p>x</p>"), format!("an image url of {n} bytes"));
    }
    // 32 words of 32 KiB are 1 MiB; one byte more is past it.
    let mut bytes_met = Vec::new();
    for extra in [0usize, 1, 2] {
        let words: String = (0..32).map(|k| format!("p.k{k}::before{{content:\"{}\"}}", "b".repeat(32768))).collect();
        let tail = if extra > 0 { format!("p.e::after{{content:\"{}\"}}", "e".repeat(extra)) } else { String::new() };
        let paragraphs: String = (0..32).map(|k| format!("<p class=\"k{k}\">{k}</p>")).collect::<String>() + "<p class=\"e\">e</p>";
        let a = limit.page(&page(&(words + &tail + "p.e::before{content:\"\"}"), &paragraphs), format!("1 MiB and {extra} bytes"));
        bytes_met.push(met(&a, b"content"));
    }
    // 100 paragraphs of 199 boxes (100 words and the spaces between) and one of 99, 100 or 101 boxes.
    let mut boxes_met = Vec::new();
    for last in [99usize, 100, 101] {
        let words = |n: usize| vec!["a"; n.div_ceil(2)].join(" ");
        let css = format!(
            "p:not(.l)::before{{content:\"{}\"}}p.l::before{{content:\"{}\"}}",
            words(199),
            if last % 2 == 1 { words(last) } else { words(last - 1) + " " }
        );
        let paragraphs = "<p>p</p>".repeat(100) + "<p class=\"l\">l</p>";
        let a = limit.page(&page(&css, &paragraphs), format!("19900 boxes and {last}"));
        boxes_met.push(met(&a, b"content"));
    }
    limit.finish();
    assert_eq!(box_met, [false, false, false, true, true], "a word of 32 KiB is made, one byte more is not");
    assert_eq!(bytes_met, [false, true, true], "1 MiB is made, one byte more is not");
    assert_eq!(boxes_met, [false, false, true], "20000 boxes are made, one more is not");
}

// ---- Tables, grids and layout ----------------------------------------------------------------------------------

/// `max_table_cells` (document.h): the cells, spans and filled-out rows a document's tables take, 200000 at most; past
/// them a cell spans one column and row and `sanitizer_limit("table")`. One row of cells spanning 1000 columns, and
/// one more cell of a span that makes 199999, 200000 or 200001; two tables sharing the budget.
#[test]
fn table_cells_cross_200000() {
    let table = |wide: usize, colspan: u32| {
        page("", &format!("<table><tr>{}<td colspan={colspan}>b</td></tr></table>", "<td colspan=1000>a</td>".repeat(wide)))
    };
    let taken =
        |wide: usize, colspan: u32| with_document(&table(wide, colspan), |doc, _| (doc.render.table_cells, doc.render.table_limited));
    let mut limit = Limit::new("table cells (200000)");
    let mut seen = Vec::new();
    for (wide, colspan) in [(199, 999), (199, 1000), (200, 1)] {
        let (cells, limited) = taken(wide, colspan);
        seen.push((cells, limited));
        let a = limit.page(&table(wide, colspan), format!("{cells} cells ({wide} of 1000 and {colspan})"));
        assert_eq!(met(&a, b"table"), limited);
    }
    let two = |rows: u32| {
        page(
            "",
            &format!(
                "<table><tr><td colspan=1000 rowspan=100>a</td></tr></table><table><tr><td colspan=1000 rowspan={rows}>b</td><td>c</td></tr></table>"
            ),
        )
    };
    for rows in [99, 100] {
        let (cells, limited) = with_document(&two(rows), |doc, _| (doc.render.table_cells, doc.render.table_limited));
        let a = limit.page(&two(rows), format!("two tables, {cells} cells"));
        assert_eq!(met(&a, b"table"), limited);
    }
    // A span is clamped to 1 to 1000 (table.cpp) before it is charged.
    let mut spans = Vec::new();
    for span in [0, 1, 999, 1000, 1001, 5000] {
        let cells =
            page("", &format!("<table><tr><td colspan={span}>a</td><td>b</td></tr><tr><td rowspan={span}>c</td><td>d</td></tr></table>"));
        let a = limit.page(&cells, format!("spans of {span}"));
        spans.push(body(&a));
    }
    limit.finish();
    assert_eq!(seen, [(199999, false), (200000, false), (200000, true)], "the limit is met past 200000");
    let columns = |span: u32| {
        let cells = page("", &format!("<table><tr><td colspan={span}>a</td><td>b</td></tr></table>"));
        with_document(&cells, |doc, _| doc.render.table_cells)
    };
    assert_eq!([columns(0), columns(1), columns(999), columns(1000), columns(1001)], [2, 2, 1000, 1001, 1001], "spans clamp to 1 and 1000");
    let _ = spans;
}

/// The grid's placement budget (render_grid.cpp, 200000 cells examined or taken): past it the items left stack in
/// rows of their own. The column count is found where the port's budget runs out on the last item.
#[test]
fn grid_placement_budget_crosses_200000() {
    let grid = |columns: usize, items: usize| {
        page(
            &format!("div.g{{display:grid;grid-template-columns:repeat({columns},1px)}}i{{grid-column:span 1000}}"),
            &format!("<div class=\"g\">{}</div>", "<i>x</i>".repeat(items)),
        )
    };
    let mut limit = Limit::new("grid placement budget (200000)");
    let mut bodies = Vec::new();
    for items in 198..=202 {
        let a = limit.page(&grid(1000, items), format!("{items} items spanning 1000 columns"));
        bodies.push(body(&a));
    }
    for (rows, columns) in [(199, 1000), (200, 1000), (201, 1000)] {
        let css = format!(
            "div.g{{display:grid;grid-template-columns:repeat({columns},1px)}}i{{grid-row:span {rows};grid-column:span {columns}}}"
        );
        limit.page(&page(&css, "<div class=\"g\"><i>a</i><i>b</i><b>c</b></div>"), format!("an item of {rows} × {columns}"));
    }
    limit.finish();
    assert!(bodies.windows(2).any(|w| w[0] != w[1]), "the items past the budget stack");
}

/// The layout budget (document.h, `set_layout_budget`): one layout lays out that many boxes afresh; the next one is
/// refused and every box returns at once. The budget is found where the port spends exactly all of it.
#[test]
fn the_layout_budget_crosses_exactly() {
    let mut html = String::from("<!DOCTYPE html><html><body>");
    for k in 0..30 {
        html +=
            ["<div style=\"margin:1px\">a ", "<p>b <span style=\"display:inline-block\">c</span> ", "<div style=\"float:left\">d "][k % 3];
    }
    for k in (0..30).rev() {
        html += if k % 3 == 1 { "</p>" } else { "</div>" };
    }
    html += "<div style=\"display:flex\"><i>f</i><b style=\"flex:1\">g</b></div><table><tr><td>t</td></tr></table></body></html>";
    let spent = with_document(html.as_bytes(), |doc, cx| {
        doc.set_layout_budget(0);
        doc.layout_document_as(cx, twidget_litehtml::Px(300.0), twidget_litehtml::types::RenderType::render_all);
        doc.layout.spent
    });
    let mut limit = Limit::new("layout budget");
    let mut bodies = Vec::new();
    for budget in [spent - 1, spent, spent + 1] {
        let steps = vec![Step::Layout(300.0, Some(budget)), Step::Dump, Step::Layout(500.0, Some(budget)), Step::Dump];
        let a = limit.document(html.as_bytes(), &steps, format!("a budget of {budget} for {spent} boxes"));
        bodies.push(body(&a));
    }
    limit.finish();
    assert!(bodies[0] != bodies[1] && bodies[1] == bodies[2], "a budget of all the boxes lays them all out");
}

// ---- Patching ------------------------------------------------------------------------------------------------

/// `update_structure`'s limits (document.cpp): new HTML of more than 20000 elements, or 128 levels deep, is refused
/// and the document stays as it was.
#[test]
fn a_structural_patch_crosses_the_build_limits() {
    let mut limit = Limit::new("structural patches (20000 elements, 128 levels)");
    let start = page("", "<div>start</div>");
    for depth in 125..=129 {
        let html = page("", &nest("<div>", depth, "x", "</div>"));
        let steps = [PatchStep::Html(html), PatchStep::Layout(375.0), PatchStep::Dump];
        limit.patches(&start, &steps, format!("a patch {depth} divs deep"));
    }
    for count in 19996..=20001 {
        let html = page("", &"<i></i>".repeat(count));
        let steps = [PatchStep::Html(html), PatchStep::Layout(375.0), PatchStep::Dump];
        limit.patches(&start, &steps, format!("a patch of {count} elements"));
    }
    let (_, tally) = limit.finish();
    eprintln!("structural patches: {}", tally.report());
}

// ---- Painting and SVG ----------------------------------------------------------------------------------------

/// The stops of a repeating gradient's layer once its period is unrolled (`max_repeated_color_points`, background.cpp,
/// 4096): a list that would be longer is drawn as its average colour. Periods swept across the boundary.
#[test]
fn repeated_gradient_stops_cross_4096() {
    let mut limit = Limit::new("repeating gradient stops (4096)");
    let mut stops = Vec::new();
    for k in 0..=24 {
        let period = 0.0478 + f64::from(k) * 0.0001;
        let css = format!(
            "div{{width:200px;height:40px;background:repeating-linear-gradient(red 0,blue {period:.4}%)}}div.r{{background:repeating-radial-gradient(red 0,blue {period:.4}%,green {:.4}%)}}div.c{{background:repeating-conic-gradient(red 0,blue {:.4}%)}}",
            period * 1.5,
            period
        );
        let a =
            limit.page(&page(&css, "<div>a</div><div class=\"r\">b</div><div class=\"c\">c</div>"), format!("a period of {period:.4}%"));
        for call in calls(&a).iter().filter(|c| c.0 == b'l') {
            stops.push(i32::from_le_bytes([call.2[89], call.2[90], call.2[91], call.2[92]]));
        }
    }
    limit.finish();
    let most = stops.iter().copied().max().unwrap_or(0);
    assert!(most > 4000 && most <= 4096 && stops.iter().any(|&s| s < 8), "both sides of the limit: {stops:?}");
}

/// An SVG reference id (`svg_reference_id`, el_svg.cpp): 1 to 128 bytes; a longer one names nothing, in a `<use>`'s
/// `href` and in a paint's `url(#…)`.
#[test]
fn svg_reference_ids_cross_128_bytes() {
    let mut limit = Limit::new("SVG reference ids (128 bytes)");
    let mut bodies = Vec::new();
    for n in 126..=130 {
        let id = format!("i{}", "d".repeat(n - 1));
        let svg = format!(
            "<svg width=\"60\" height=\"30\"><defs><rect id=\"{id}\" width=\"5\" height=\"5\"/><linearGradient id=\"g{id}\"><stop stop-color=\"red\"/></linearGradient></defs><use href=\"#{id}\" x=\"3\"/><rect x=\"20\" width=\"9\" height=\"9\" fill=\"url(#g{id})\"/></svg>"
        );
        let a = limit.page(&page("", &svg), format!("an id of {n} bytes"));
        bodies.push(calls(&a).into_iter().filter(|c| c.0 == b'p').count());
    }
    limit.finish();
    assert!(bodies[2] != bodies[3], "an id of 129 bytes names nothing: {bodies:?}");
}

// ---- Patching thresholds -------------------------------------------------------------------------------------

/// `fast_transform_tokens` (css_effects.cpp): a transform patched in through `update_styles` takes the fast path up to
/// 512 bytes of value and the tokenizer past them; both must give the same.
#[test]
fn a_patched_transform_crosses_512_bytes() {
    let mut limit = Limit::new("patched transforms (512 bytes)");
    let start = page("", "<div style=\"transform:rotate(1deg)\">a</div><p>b</p>");
    for n in [510usize, 511, 512, 513, 514] {
        let head = "translate(1px, 2px) rotate(3deg)";
        let value = format!("{head}{}", " ".repeat(n - head.len()));
        let patch = format!("transform:{value};").into_bytes();
        let steps = [PatchStep::Styles(vec![(4, patch.clone()), (5, patch)]), PatchStep::Layout(375.0), PatchStep::Dump];
        limit.patches(&start, &steps, format!("a transform of {n} bytes"));
    }
    limit.finish();
}

/// Match memos (html_tag.cpp): a tag keeps the last two; a third class drops the first, so a flip back to it is
/// matched again.
#[test]
fn match_memos_keep_two() {
    let mut limit = Limit::new("match memos (2)");
    let css = ".a{color:red}.b{color:blue}.c{color:green}.d{color:navy}";
    let html = |class: &str| page(css, &format!("<div class=\"{class}\">x</div><div class=\"z\">y</div>"));
    let mut steps = Vec::new();
    for class in ["b", "c", "a", "d", "c", "b", "a"] {
        steps.extend([PatchStep::Html(html(class)), PatchStep::Layout(375.0), PatchStep::Dump]);
    }
    limit.patches(&html("a"), &steps, "class flips over four classes");
    limit.finish();
}

/// Thresholds the selector budget's charges turn on (html_tag.cpp): the sibling index of a sheet's pass serves a parent
/// of more than 16 children, and a walk over a child list costs one step per 64 children. Both must match alike on
/// either side.
#[test]
fn sibling_thresholds_match_alike() {
    let mut limit = Limit::new("sibling index (16 children) and walk charges (64)");
    let css = "q + p{color:red}q ~ p{background:blue}p:nth-child(3n+1){margin:1px}p:nth-last-of-type(2){padding:1px}p:only-child{border:1px solid}";
    for n in [15usize, 16, 17, 63, 64, 65, 127, 128, 129] {
        let children = format!("<q>q</q>{}", "<p>p</p>".repeat(n - 1));
        let steps = [Step::Layout(375.0, None), Step::Dump, Step::Hover(n as u32 / 2), Step::Dump];
        limit.document(&page(css, &format!("<div>{children}</div>")), &steps, format!("{n} children"));
    }
    limit.finish();
}

/// `layout_dirty_regions` (document.cpp): a relayout after a patch dirties the boxes that moved, before and after,
/// 128 at most; one more pair and it reports none (the whole document repaints). A word grows, so it and the N inline
/// blocks after it on its line move.
#[test]
fn dirty_regions_cross_128_boxes() {
    let mut limit = Limit::new("layout dirty regions (128 boxes)");
    for n in 60..=66 {
        let blocks = "<b style=\"display:inline-block;width:2px;height:2px\"></b>".repeat(n);
        let doc = |word: &str| {
            page("", &format!("<p style=\"width:3000px;white-space:nowrap\">{word}{blocks}</p><div style=\"height:4000px\"></div>"))
        };
        let steps = [
            PatchStep::Layout(375.0),
            PatchStep::Html(doc("bbbbbbbbbbbbbb")),
            PatchStep::Layout(375.0),
            PatchStep::Html(doc("cc")),
            PatchStep::Layout(375.0),
        ];
        limit.patches(&doc("aaa"), &steps, format!("a word and {n} blocks moved"));
    }
    let (_, tally) = limit.finish();
    let with = tally.count("layouts with dirty regions");
    assert!(with > 0 && with < 14, "some relayouts report their regions, the larger ones none: {}", tally.report());
}

// ---- Values clamped ------------------------------------------------------------------------------------------

/// Values the fork clamps: a filter's radius to 256 px and its shadow offsets to ±1024 px (css_properties.cpp), a font
/// feature's value to 65535 (style.cpp), a table cell's CSS height to 2^20 px in its row (render_table.cpp).
#[test]
fn clamped_values_cross_their_bounds() {
    let mut limit = Limit::new("clamped values (256 px, 1024 px, 65535, 2^20 px)");
    for r in ["255px", "256px", "256.01px", "257px", "16em", "1e30px"] {
        limit
            .page(&page("", &format!("<div style=\"filter:blur({r}) drop-shadow({r} -{r} 2px red)\">a</div>")), format!("a filter of {r}"));
    }
    for d in ["1023px", "1024px", "1024.5px", "1025px", "-1025px", "64em"] {
        limit.page(&page("", &format!("<div style=\"filter:drop-shadow({d} {d} {d} blue)\">a</div>")), format!("a shadow offset of {d}"));
    }
    for v in ["65534", "65535", "65536", "4294967296", "1e10"] {
        limit
            .page(&page("", &format!("<p style=\"font-feature-settings:'liga' {v}, 'tnum' {v}\">a</p>")), format!("a font feature of {v}"));
    }
    for h in ["1048575px", "1048576px", "1048577px", "1e30px", "65536em"] {
        limit.page(&page("", &format!("<table><tr><td style=\"height:{h}\">a</td><td>b</td></tr></table>")), format!("a cell {h} high"));
    }
    limit.finish();
}

/// css_effects.cpp's counts: a math function of 32 arguments at most, a filter list of 32 functions at most, and the
/// fast transform path (`update_styles`) of 32 functions, 6 numbers each, numbers of 32 characters and up to 1e9.
#[test]
fn effect_counts_cross_32() {
    let mut limit = Limit::new("effect counts (32 arguments, 32 filters, 32 transforms, 6 numbers)");
    for n in 31..=33 {
        let args = vec!["2px"; n].join(", ");
        limit.page(
            &page("", &format!("<div style=\"width:min({args});transform:translateX(max({args}))\">a</div>")),
            format!("min() of {n}"),
        );
        let filters = vec!["blur(1px)"; n].join(" ");
        limit.page(&page("", &format!("<div style=\"filter:{filters};backdrop-filter:{filters}\">a</div>")), format!("{n} filters"));
    }
    let start = page("", "<div style=\"transform:rotate(1deg)\">a</div><p>b</p>");
    let mut patch = |value: String, what: String| {
        let style = format!("transform:{value};").into_bytes();
        let steps = [PatchStep::Styles(vec![(4, style.clone()), (5, style)]), PatchStep::Layout(375.0), PatchStep::Dump];
        limit.patches(&start, &steps, what);
    };
    for n in 31..=33 {
        patch(vec!["rotate(1deg)"; n].join(" "), format!("{n} transform functions"));
        patch(format!("translate(1.{}px)", "0".repeat(n - 2)), format!("a number of {n} characters"));
    }
    for n in 5..=7 {
        patch(format!("matrix({})", vec!["1"; n].join(",")), format!("a matrix of {n} numbers"));
    }
    for v in ["999999999", "1000000000", "1000000001"] {
        patch(format!("translateX({v}px)"), format!("a translation of {v}"));
    }
    limit.finish();
}

/// Integer and length clamps of the cascade and the attributes: `order` and `z-index` to `int` (style.cpp,
/// css_properties.h), `font-weight` 1 to 1000, `clamp_length`'s ±2^25 px (css_length.h), flex factors ×1000 to
/// INT_MAX and their sums against 1000 (flex_item.cpp, flex_line.cpp), counter increments to `int` and two changes a
/// pass (element.cpp, html_tag.cpp), `color-mix()` percentages, grid lines to ±1000 and auto repeats to 1000 tracks
/// (css_effects.cpp, render_grid.cpp), `<input size>` 1 to 1000, `<ol start>` below INT_MAX, `<textarea rows>` and
/// `<font size>` (el_form.cpp, render_block.cpp, el_font.cpp).
#[test]
fn clamps_cross_their_bounds() {
    let mut limit = Limit::new("integer and length clamps");
    for v in ["2147483646", "2147483647", "2147483648", "-2147483648", "-2147483649", "1e20"] {
        let flex = format!("<div style=\"display:flex\"><i style=\"order:{v}\">a</i><b style=\"order:1\">b</b></div>");
        let z = format!("<div style=\"position:relative;z-index:{v}\">z</div><div style=\"position:relative;z-index:1\">y</div>");
        limit.page(&page("", &(flex + &z)), format!("order and z-index {v}"));
    }
    for w in ["0", "1", "999", "1000", "1001", "1000.5"] {
        limit.page(&page("", &format!("<p style=\"font-weight:{w}\">a<b>b</b></p>")), format!("font-weight {w}"));
    }
    for px in ["33554431px", "33554432px", "33554433px", "-33554433px", "1e10px"] {
        let css = format!(
            "div.a{{width:calc({px})}}div.b{{margin-left:calc({px} * 2);width:100px}}div.c{{width:{px}}}div.d{{padding-left:1e8%}}"
        );
        limit.page(
            &page(&css, "<div class=\"a\">a</div><div class=\"b\">b</div><div class=\"c\">c</div><div class=\"d\">d</div>"),
            format!("lengths of {px}"),
        );
    }
    for f in ["2147483.646", "2147483.647", "2147483.648", "1e10", "0.999", "0.0004", "0.0005"] {
        let html = format!(
            "<div style=\"display:flex;width:300px\"><i style=\"flex:{f} {f} 10px\">a</i><b style=\"flex:0.0005 1 20px\">b</b></div><div style=\"display:flex;width:50px\"><i style=\"flex:0 {f} 100px\">c</i></div>"
        );
        limit.page(&page("", &html), format!("flex factors of {f}"));
    }
    for v in ["2147483646", "2147483647", "2147483648"] {
        let css = format!(
            "p{{counter-increment:c {v}}}p::before{{content:counter(c)}}p.r{{counter-reset:c {v}}}li{{counter-increment:d 1}}li:hover{{counter-increment:d 2}}"
        );
        limit.document(
            &page(&css, "<p class=\"r\">r</p><p>a</p><p>b</p><ol><li>x</li><li>y</li></ol>"),
            &[Step::Layout(375.0, None), Step::Dump, Step::Hover(8), Step::Dump, Step::Refresh, Step::Dump],
            format!("counter increments of {v}"),
        );
    }
    for p in ["0%", "100%", "-1%", "101%", "50%, blue 50%", "60%, blue 60%", "0%, blue 0%", "30%, blue 30%"] {
        limit.page(
            &page("", &format!("<p style=\"color:color-mix(in srgb, red {p});background:color-mix(in srgb, red, blue {p})\">a</p>")),
            format!("color-mix {p}"),
        );
    }
    for line in ["999", "1000", "1001", "-1001", "span 1000", "span 1001", "1 / 1002", "-1000 / span 3"] {
        let css = format!("div.g{{display:grid;grid-template-columns:repeat(3,10px)}}i{{grid-column:{line}}}b{{grid-row:{line}}}");
        limit.page(&page(&css, "<div class=\"g\"><i>a</i><b>b</b><u>c</u></div>"), format!("grid line {line}"));
    }
    for width in ["999px", "1000px", "1001px", "2000px"] {
        let css = format!(
            "div.g{{display:grid;width:{width};grid-template-columns:repeat(auto-fill,1px)}}div.h{{display:grid;width:{width};grid-template-columns:repeat(auto-fit,1px 1px) 3px}}"
        );
        limit.page(&page(&css, "<div class=\"g\"><i>a</i></div><div class=\"h\"><b>b</b></div>"), format!("auto repeats in {width}"));
    }
    for size in ["0", "1", "1000", "1001", "-5", "4294967297"] {
        limit.page(&page("", &format!("<input size=\"{size}\"><textarea rows=\"{size}\"></textarea><font size=\"+{size}\">f</font><font size=\"-{size}\">g</font>")), format!("sizes of {size}"));
    }
    for start in ["2147483646", "2147483647", "2147483648", "-2147483648", "-2147483649"] {
        limit.page(
            &page(
                "ol{list-style-type:armenian}ol.d{list-style-type:decimal}",
                &format!("<ol start=\"{start}\"><li>a</li><li>b</li></ol><ol class=\"d\" start=\"{start}\"><li>c</li><li>d</li></ol>"),
            ),
            format!("<ol start={start}>"),
        );
    }
    for rows in ["2147483647", "2147483648", "4294967296"] {
        limit.page(&page("", &format!("<textarea rows=\"{rows}\" style=\"height:auto\"></textarea>")), format!("<textarea rows={rows}>"));
    }
    limit.finish();
}

/// Layout and paint thresholds: a fixed-size flex reused for up to 256 items (render_item.cpp), transformed culling
/// only with a determinant past 1e-8 and corners within 1e6 (render_item.cpp).
#[test]
fn layout_and_culling_thresholds_cross_alike() {
    let mut limit = Limit::new("flex reuse (256) and culling (1e-8, 1e6)");
    for n in [254usize, 255, 256, 257, 258] {
        let items = "<i style=\"width:3px;height:3px;flex:none\">a</i>".repeat(n);
        let html = page("", &format!("<div style=\"display:flex;flex-wrap:wrap;width:200px;height:300px\">{items}</div><p>after</p>"));
        let steps = [Step::Layout(375.0, None), Step::Dump, Step::Layout(375.0, None), Step::Layout(300.0, None), Step::Dump];
        limit.document(&html, &steps, format!("a fixed flex of {n} items"));
    }
    for t in [
        "scale(0.0001)",
        "scale(0.00009)",
        "scale(0.0001, 0.0001)",
        "scale(1e-5)",
        "translate(999999px)",
        "translate(1000001px)",
        "translate(-1000001px, 5px)",
        "matrix(1e-4,0,0,1e-4,0,0)",
    ] {
        let mut culled = PaintStep::viewport(375.0, 667.0);
        culled.cull = true;
        let html = page(
            "",
            &format!("<div style=\"transform:{t};width:100px;overflow:hidden\"><p>a</p><p style=\"margin-top:900px\">b</p></div>"),
        );
        limit.document(
            &html,
            &[Step::Layout(375.0, None), Step::Paint(Box::new(culled)), Step::Dump],
            format!("a transform of {t}, culled"),
        );
    }
    limit.finish();
}

/// Gumbo's numeric character references: the code point wraps past 2^32 and is replaced past 0x10FFFF and for
/// surrogates (char_ref.c).
#[test]
fn numeric_references_cross_their_bounds() {
    let mut limit = Limit::new("numeric references (0x10FFFF, 2^32)");
    for r in [
        "&#1114111;",
        "&#1114112;",
        "&#x10FFFF;",
        "&#x110000;",
        "&#xD7FF;",
        "&#xD800;",
        "&#xDFFF;",
        "&#xE000;",
        "&#4294967295;",
        "&#4294967296;",
        "&#4294967361;",
        "&#x100000041;",
        "&#0;",
        "&#128;",
        "&#x9F;",
    ] {
        limit.gumbo(format!("<p title=\"{r}\">{r}</p>").as_bytes(), format!("the reference {r}"));
        limit.page(&page("", &format!("<p title=\"{r}\">{r}</p>")), format!("the reference {r} built"));
    }
    limit.finish();
}

// ---- Hit testing ---------------------------------------------------------------------------------------------

/// Hit testing inverts a transformed child's matrix only when its determinant is at least 1e-12 (render_item.cpp's
/// `get_child_by_point`); under it the child is passed over. Scales whose determinant falls either side, asked at
/// points inside and outside the box, with the pointer moved over them.
#[test]
fn hit_transforms_cross_1e_12() {
    let mut limit = Limit::new("hit transforms (determinant 1e-12)");
    for t in [
        "scale(1e-6)",
        "scale(1.01e-6)",
        "scale(0.99e-6)",
        "scale(1e-6, 1.1e-6)",
        "scale(1e-3, 1e-9)",
        "scale(1e-3, 0.9e-9)",
        "matrix(1e-6,0,0,1e-6,0,0)",
        "scale(0)",
    ] {
        let html = page(
            "",
            &format!(
                "<div style=\"width:200px;height:200px;transform:{t}\"><p style=\"margin:0;height:200px\">a</p></div><span>after</span>"
            ),
        );
        let mut steps = vec![HitStep::Layout(375.0)];
        for (x, y) in [(100.0, 100.0), (100.0001, 100.0), (0.0, 0.0), (300.0, 300.0)] {
            steps.push(HitStep::At(At::root(0, x, y)));
            steps.push(HitStep::Over([x, y, x, y]));
        }
        limit.hits(&html, &steps, format!("a hit under {t}"));
    }
    limit.finish();
}
