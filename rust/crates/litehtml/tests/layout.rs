//! Layout, ported (stages L7–L12): documents cascaded and built as the renderer builds them, laid out at widths
//! (again at the first after another, so the layout cache is used), after media changes and `:hover` restyles, and
//! with the fixed boxes placed alone, through `oracle/lhdump` (`Y`) and the port: every render item's place, margins,
//! padding and borders, its cache and marks, positioned lists in z-order, scroll views, sticky offsets, placements,
//! inline boxes, a text's pieces, an inline context's line boxes and their items, a flex's baselines and lines with
//! their items, a table's grid as laid out and the sides its collapsed borders leave each cell, each grid pass's
//! tracks, items and their areas, the document's size, budget and fixed boxes, each element's line height, with the
//! container's calls in order (fonts, text widths, the viewport, image sizes, text breakers). The documents:
//! widget-sandbox's corpus light and dark at five widths, every tag, written shapes for each branch (margins
//! collapsing, floats and clearance, line breaking, white space, `text-overflow`, bidi, vertical alignment, images,
//! lists, `<textarea>`; flex directions, wrapping, every alignment value, factors and bases, min and max, `order`,
//! gaps, `auto` margins, baselines, form controls; absolute and fixed boxes under each rule of CSS 2.2 10.3.7 and
//! 10.6.4, containing blocks, static positions, `z-index`, sticky boxes, scroll views; tables with spans, captions,
//! fixed layout, %, collapsed borders, heights, anonymous boxes, nesting; grid tracks of every kind, `repeat()` with
//! `auto-fill` and `auto-fit`, placement and auto-placement, spans, gaps, alignment, nested grids), generated block,
//! float, margin and inline documents, generated nested flex, positioned, table and grid documents, the `LH_NO_*`
//! layout switches, the budget crossed, and deep documents on a small stack. Each run reports what it reached.

mod common;

use common::cascade::*;
use common::gumbo::{corpus_pages, snapshot};
use common::layout::*;
use common::*;
use twidget_litehtml::document::Switches;

/// A case: its record, the port's answer and what it is.
type Case = (Vec<u8>, (u8, Vec<u8>), String);

/// `Y` cases through lhdump and the port, checked in batches of about 24 MB; settings (`E`) open every batch.
struct Run {
    name: &'static str,
    lh: Lh,
    switches: Switches,
    settings: Vec<u8>,
    /// The containers let texts owe their widths until read (`TW_MEASURES_TEXT_LATER`).
    later: bool,
    batch: Vec<Case>,
    bytes: usize,
    cases: usize,
    compared: usize,
    tally: LayoutTally,
    oracle: Option<Oracle>,
}

impl Run {
    fn new(name: &'static str) -> Run {
        Run {
            name,
            lh: Lh::new(),
            switches: Switches::default(),
            settings: Vec::new(),
            later: false,
            batch: Vec::new(),
            bytes: 0,
            cases: 0,
            compared: 0,
            tally: LayoutTally::default(),
            oracle: Oracle::find("lhdump"),
        }
    }

    /// A run with a layout switch set on both sides.
    fn with_switch(name: &'static str, switch: &str) -> Run {
        let mut run = Run::new(name);
        run.settings = framed(b'E', format!("{switch}=1").as_bytes());
        match switch {
            "LH_NO_LAYOUT_CACHE" => run.switches.layout_cache = false,
            "LH_NO_POSITIONED_REUSE" => run.switches.positioned_reuse = false,
            "LH_NO_FIXED_LAYOUT_REUSE" => run.switches.fixed_layout_reuse = false,
            "LH_LAYOUT_CACHE_CHECK" => run.switches.layout_cache_check = true,
            "LH_NO_COLUMN_REUSE" => run.switches.column_reuse = false,
            "TW_MEASURES_TEXT_LATER" => run.later = true,
            other => panic!("no switch {other}"),
        }
        run
    }

    fn case(&mut self, input: Vec<u8>, what: impl Into<String>) {
        let mut tally = LayoutTally::default();
        set_measures_text_later(self.later);
        let rust = layout_answer(&self.lh, self.switches, &input, &mut tally);
        self.cases += 1;
        self.tally.documents += 1;
        if tally.grid_passes > 0 {
            self.tally.grid_documents += 1;
        }
        self.compared += 1;
        self.tally.merge(tally);
        self.bytes += input.len() + rust.1.len();
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
            input.extend(framed(b'Y', case));
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
                differences.push(explain(self.switches, case, &(cpp.0, cpp_body), &(rust.0, rust_body), what));
            } else if let Some(calls) = compare_calls(&cpp.1, &rust.1) {
                differences.push(format!("{calls}\n  in {}", what.chars().take(3000).collect::<String>()));
            }
            if differences.len() >= 3 {
                break;
            }
        }
        assert!(differences.is_empty(), "{}: answers differ, the first:\n{}", self.name, differences.join("\n"));
    }

    fn finish(mut self) -> LayoutTally {
        self.flush();
        let compared = if self.oracle.is_some() { "alike with lhdump" } else { "laid out (no oracle)" };
        eprintln!("{}: {} cases, {} {compared}; {}", self.name, self.cases, self.compared, self.tally.report());
        self.tally
    }
}

/// Where two answers part: the field the Rust answer was writing there.
fn explain(switches: Switches, case: &[u8], cpp: &(u8, Vec<u8>), rust: &(u8, Vec<u8>), what: &str) -> String {
    let at = cpp.1.iter().zip(&rust.1).position(|(a, b)| a != b).unwrap_or(cpp.1.len().min(rust.1.len()));
    let marks = {
        start_marks();
        layout_answer(&Lh::new(), switches, case, &mut LayoutTally::default());
        take_marks()
    };
    let item = marks.iter().rfind(|m| m.0 <= at && m.2 >= 0).map_or(-1, |m| m.2);
    let field = marks.iter().rfind(|m| m.0 <= at).map_or("", |m| m.1);
    let near = |b: &[u8]| {
        let piece = &b[at.saturating_sub(16).min(b.len())..(at + 64).min(b.len())];
        hex(&piece[..piece.len().min(48)])
    };
    // The floats around the difference, as numbers.
    let floats = |b: &[u8]| {
        let from = at.saturating_sub(16).min(b.len());
        let to = (from + 48).min(b.len());
        b[from..to]
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .map(|v| format!("{v}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    format!(
        "status C++ {} / Rust {}, {} / {} bytes, first difference at byte {at}, {field} (item or element {item}):\n  C++  {} [{}]\n  Rust {} [{}]\n  in {}",
        cpp.0,
        rust.0,
        cpp.1.len(),
        rust.1.len(),
        near(&cpp.1),
        floats(&cpp.1),
        near(&rust.1),
        floats(&rust.1),
        what.chars().take(3000).collect::<String>()
    )
}

// ---- Inputs ------------------------------------------------------------------------------------------------

fn pick<'a>(random: &mut Lcg, from: &[&'a str]) -> &'a str {
    from[random.below(from.len())]
}

/// A length of the kinds layout resolves.
fn length(random: &mut Lcg) -> &'static str {
    pick(
        random,
        &["0", "4px", "10px", "1.5em", "25%", "50%", "100%", "auto", "-6px", "-12px", "calc(10% + 4px)", "33.3px", "2rem", "0.5px"],
    )
}

/// A declaration block layout reads: the box's lengths, floats and clearance, positions, displays, text and line
/// properties, white space, wrapping, overflow, containment, lists, `aspect-ratio`.
fn layout_declaration(random: &mut Lcg) -> String {
    match random.below(30) {
        0 | 1 => format!("margin:{} {}", length(random), length(random)),
        2 => format!("margin-top:{}", length(random)),
        3 => format!("margin-bottom:{}", length(random)),
        4 => format!("padding:{} {}", pick(random, &["0", "3px", "1em", "10%", "7px"]), pick(random, &["0", "2px", "5%"])),
        5 => format!("border:{} solid", pick(random, &["0", "1px", "3px", "thin", "medium"])),
        6 | 7 => format!(
            "width:{}",
            pick(
                random,
                &["auto", "50px", "120px", "40%", "100%", "min-content", "max-content", "fit-content", "calc(50% - 10px)", "300px"]
            )
        ),
        8 => format!("height:{}", pick(random, &["auto", "20px", "50%", "3em", "0"])),
        9 => format!(
            "{}:{}",
            pick(random, &["min-width", "max-width", "min-height", "max-height"]),
            pick(random, &["10px", "60px", "50%", "none", "200px"])
        ),
        10 | 11 => format!("float:{}", pick(random, &["left", "right", "none", "left", "right"])),
        12 => format!("clear:{}", pick(random, &["left", "right", "both", "none"])),
        13 => format!("position:relative;{}:{}", pick(random, &["left", "right", "top", "bottom"]), pick(random, &["3px", "-4px", "10%"])),
        14 => format!("display:{}", pick(random, &["block", "inline", "inline-block", "list-item", "none", "block", "inline-block"])),
        15 => format!("text-align:{}", pick(random, &["left", "right", "center", "justify", "start", "end"])),
        16 => format!("white-space:{}", pick(random, &["normal", "nowrap", "pre", "pre-line", "pre-wrap"])),
        17 => pick(
            random,
            &[
                "overflow-wrap:anywhere",
                "overflow-wrap:break-word",
                "word-break:break-all",
                "word-break:break-word",
                "overflow-wrap:normal",
            ],
        )
        .to_string(),
        18 => format!("overflow:{}", pick(random, &["hidden", "auto", "scroll", "visible", "clip"])),
        19 => "overflow:hidden;white-space:nowrap;text-overflow:ellipsis".to_string(),
        20 => format!(
            "vertical-align:{}",
            pick(random, &["top", "bottom", "middle", "text-top", "text-bottom", "sub", "super", "baseline", "4px"])
        ),
        21 => format!("line-height:{}", pick(random, &["normal", "1", "1.5", "30px", "0", "200%"])),
        22 => format!("font-size:{}", pick(random, &["10px", "20px", "1.5em", "small", "x-large"])),
        23 => format!("text-indent:{}", pick(random, &["10px", "-5px", "10%", "0"])),
        24 => format!("direction:{}", pick(random, &["rtl", "ltr"])),
        25 => format!("box-sizing:{}", pick(random, &["border-box", "content-box"])),
        26 => format!("contain:{}", pick(random, &["size", "layout", "paint", "inline-size", "strict", "content"])),
        27 => format!("list-style:{}", pick(random, &["inside", "outside", "none", "inside square", "url(m.png)"])),
        28 => format!("aspect-ratio:{}", pick(random, &["1", "2", "16/9", "0.5"])),
        _ => format!("font-weight:{}", pick(random, &["bold", "normal"])),
    }
}

/// Words of every kind the breaker and the lines meet: short, long, punctuation that may not start a line, numbers,
/// right-to-left words, tabs and newlines.
fn words(random: &mut Lcg, out: &mut String) {
    let pool = [
        "word",
        "a",
        "layout",
        "text",
        "of",
        "lines",
        "supercalifragilisticexpialidocious",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ",",
        ".",
        "!",
        ")",
        "123",
        "\u{5e9}\u{5dc}\u{5d5}\u{5dd}",
        "\u{627}\u{644}\u{639}\u{631}\u{628}\u{64a}\u{629}",
        "caf\u{e9}",
        "\u{4e2d}\u{6587}",
        "x-y",
        "\t",
        "\n",
        "  ",
    ];
    for _ in 0..1 + random.below(10) {
        out.push_str(pick(random, &pool));
        if !random.chance(4) {
            out.push(' ');
        }
    }
}

/// A generated document of blocks, floats, margins and inlines.
fn layout_document(random: &mut Lcg) -> Vec<u8> {
    fn element(random: &mut Lcg, depth: u32, out: &mut String) {
        let tags = [
            "div",
            "div",
            "div",
            "p",
            "span",
            "span",
            "b",
            "em",
            "a",
            "img",
            "br",
            "ul",
            "ol",
            "li",
            "pre",
            "section",
            "h2",
            "svg",
            "textarea",
            "input",
            "button",
            "blockquote",
            "small",
            "label",
        ];
        let tag = pick(random, &tags);
        out.push('<');
        out.push_str(tag);
        match tag {
            "img" => {
                out.push_str(&format!(" src=\"{}.png\"", pick(random, &["a", "bb", "photo", "x9", ""])));
                if random.chance(2) {
                    out.push_str(&format!(" width=\"{}\"", pick(random, &["20", "100", "50%"])));
                }
                if random.chance(3) {
                    out.push_str(&format!(" height=\"{}\"", pick(random, &["10", "40"])));
                }
            }
            "svg" => {
                out.push_str(pick(
                    random,
                    &[" width=\"40\" height=\"20\"", " viewBox=\"0 0 10 5\"", " viewBox=\"0 0 4 4\" width=\"30\"", ""],
                ));
            }
            "textarea" => out.push_str(pick(random, &[" rows=\"3\"", " rows=\"x\"", "", " rows=\"0\""])),
            "input" => out.push_str(pick(
                random,
                &[" type=\"checkbox\"", " type=\"radio\"", " type=\"button\" value=\"v\"", " type=\"submit\"", " type=\"reset\""],
            )),
            "ol" => out.push_str(pick(random, &[" start=\"3\"", ""])),
            _ => {}
        }
        if random.chance(3) {
            out.push_str(&format!(" class=\"{}\"", pick(random, &["a", "b", "c", "a b"])));
        }
        if random.chance(2) {
            let decls: Vec<String> = (0..1 + random.below(4)).map(|_| layout_declaration(random)).collect();
            out.push_str(&format!(" style=\"{}\"", decls.join(";")));
        }
        out.push('>');
        if matches!(tag, "img" | "br" | "input") {
            return;
        }
        if tag == "svg" {
            out.push_str("<rect width=\"4\" height=\"4\"/></svg>");
            return;
        }
        let n = if depth > 5 { 0 } else { random.below(5) };
        for _ in 0..n {
            match random.below(7) {
                0..=2 => words(random, out),
                _ => element(random, depth + 1, out),
            }
        }
        out.push_str(&format!("</{tag}>"));
    }
    let mut out = String::new();
    if !random.chance(8) {
        out.push_str("<!DOCTYPE html>");
    }
    out.push_str("<html><head><style>");
    for _ in 0..random.below(5) {
        let selector = pick(random, &["div", "span", ".a", ".b", "p", "li", ".c", "img", "body", "section > div"]);
        let decls: Vec<String> = (0..1 + random.below(3)).map(|_| layout_declaration(random)).collect();
        let rule = format!("{selector}{{{}}}", decls.join(";"));
        if random.chance(3) {
            out.push_str(&format!("@media (min-width:{}px){{{rule}}}", pick(random, &["300", "450", "700"])));
        } else {
            out.push_str(&rule);
        }
    }
    out.push_str("</style></head><body>");
    for _ in 0..1 + random.below(5) {
        element(random, 0, &mut out);
    }
    out.push_str("</body></html>");
    out.into_bytes()
}

/// Shapes for each branch, written out.
fn written_documents() -> Vec<(&'static str, String)> {
    let page = |body: &str| format!("<!DOCTYPE html><html><head></head><body>{body}</body></html>");
    let long = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    vec![
        (
            "first child's margin collapsing into the parent",
            page("<div style=\"margin-top:10px\"><div style=\"margin-top:20px\">a</div></div>"),
        ),
        (
            "negative margins collapsing",
            page("<div style=\"margin-bottom:-10px\">a</div><div style=\"margin-top:15px\">b</div><div style=\"margin-top:-5px\">c</div>"),
        ),
        ("bottom margin collapsing through", page("<div><div style=\"margin-bottom:30px\">a</div></div><p>b</p>")),
        (
            "margin collapsing blocked by padding and border",
            page("<div style=\"padding-top:1px;margin-top:5px\"><p>a</p></div><div style=\"border-top:2px solid\"><p>b</p></div>"),
        ),
        ("empty blocks between margins", page("<p>a</p><div></div><div style=\"height:0\"></div><p>b</p>")),
        (
            "auto margins",
            page(
                "<div style=\"width:100px;margin:0 auto\">c</div><div style=\"width:100px;margin-left:auto\">r</div><div style=\"width:100px;margin-right:auto;margin-left:20px\">l</div>",
            ),
        ),
        ("auto margins wider than the parent", page("<div style=\"width:900px;margin:0 auto\">w</div>")),
        (
            "right to left blocks",
            page(
                "<div dir=\"rtl\" style=\"direction:rtl\"><div style=\"width:50px;margin:1px\">a</div><p>\u{5e9}\u{5dc}\u{5d5}\u{5dd} hello \u{5e2}\u{5d5}\u{5dc}\u{5dd} 123</p></div>",
            ),
        ),
        (
            "floats left and right",
            page(
                "<div style=\"float:left;width:100px;height:50px\">l</div><div style=\"float:right;width:80px;height:30px\">r</div><p>text around the floats, words and words and more words to wrap them</p>",
            ),
        ),
        (
            "floats stacking and wrapping to the next line",
            page(
                "<div style=\"width:300px\"><div style=\"float:left;width:120px;height:20px\"></div><div style=\"float:left;width:120px;height:40px\"></div><div style=\"float:left;width:120px;height:10px\"></div><div style=\"float:right;width:200px;height:5px\"></div></div>",
            ),
        ),
        (
            "clearance",
            page(
                "<div style=\"float:left;width:50px;height:60px\"></div><div style=\"float:right;width:50px;height:80px\"></div><p style=\"clear:left\">l</p><p style=\"clear:right\">r</p><p style=\"clear:both\">b</p>",
            ),
        ),
        (
            "a float in an inline context",
            page(
                "<p>before <span style=\"float:left;width:40px;height:30px\">f</span> after a few words that go around the float and further</p>",
            ),
        ),
        (
            "a float clearing a float",
            page(
                "<div style=\"float:left;width:60px;height:20px\"></div><div style=\"float:left;clear:left;width:40px;height:20px\"></div><div style=\"float:right;clear:both\">x</div>",
            ),
        ),
        (
            "a block formatting context beside a float",
            page(
                "<div style=\"float:left;width:100px;height:100px\"></div><div style=\"overflow:hidden\">bfc text</div><div style=\"display:inline-block\">ib</div>",
            ),
        ),
        ("a br that clears", page("<div style=\"float:left;width:30px;height:40px\"></div>text<br clear=\"all\">after")),
        ("line breaking", page(&format!("<p style=\"width:120px\">some words that wrap over several lines here {long}</p>"))),
        (
            "breaking anywhere",
            page(&format!(
                "<p style=\"width:100px;overflow-wrap:anywhere\">{long} and a b</p><p style=\"width:100px;word-break:break-all\">x{long}</p><p style=\"width:100px;overflow-wrap:break-word\">y{long}.</p>"
            )),
        ),
        (
            "breaking with punctuation after an inline",
            page(&format!("<p style=\"width:150px;overflow-wrap:break-word\"><b>{long}</b>, and <i>this</i>.</p>")),
        ),
        (
            "white space modes",
            page(
                "<pre>a  b\n\tc\n\n</pre><p style=\"white-space:nowrap\">no wrap at all here at all</p><p style=\"white-space:pre-line\">a   b\nc</p><p style=\"white-space:pre-wrap\">a   b\n c  </p>",
            ),
        ),
        (
            "text-align",
            page(
                "<p style=\"text-align:right\">r</p><p style=\"text-align:center\">c c</p><p style=\"text-align:justify;width:150px\">justified words over two lines or three lines here</p><p style=\"text-align:end\">e</p><p dir=\"rtl\" style=\"direction:rtl;text-align:left\">x</p>",
            ),
        ),
        (
            "text-overflow ellipsis",
            page(
                "<div style=\"width:80px;overflow:hidden;white-space:nowrap;text-overflow:ellipsis\">a long line that does not fit <b>bold</b> end</div><div style=\"width:80px;overflow:hidden;text-overflow:ellipsis;overflow-wrap:anywhere\">aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa</div>",
            ),
        ),
        (
            "bidi",
            page(
                "<p>abc \u{5d0}\u{5d1}\u{5d2} \u{5d3}\u{5d4} def <b>\u{5d5}\u{5d6}</b> 12</p><p style=\"direction:rtl\">\u{5d0} abc def \u{5d1}</p>",
            ),
        ),
        (
            "vertical-align",
            page(
                "<p>a<img src=\"x.png\" style=\"vertical-align:top\">b<img src=\"yy.png\" style=\"vertical-align:middle\"><span style=\"vertical-align:super\">sup</span><span style=\"vertical-align:sub\">sub</span><span style=\"display:inline-block;vertical-align:bottom;height:30px\">ib</span><span style=\"vertical-align:text-top\">t</span><span style=\"display:inline-block;vertical-align:text-bottom\">tb</span></p>",
            ),
        ),
        (
            "inline boxes",
            page(
                "<p>a <span style=\"display:inline-block;width:50px;padding:3px;margin:2px;border:1px solid\">ib</span> b <span style=\"display:inline-block\">auto <b>width</b></span></p>",
            ),
        ),
        (
            "inlines with padding, borders and margins",
            page("<p><span style=\"padding:4px;border:2px solid;margin:3px\">a <b style=\"padding:1px\">b c</b> d</span> e</p>"),
        ),
        (
            "relatively positioned inlines",
            page(
                "<p>a <span style=\"position:relative;left:5px;top:-3px\">b <i>c</i></span> d</p><div style=\"position:relative;top:10%;left:3px\">r</div>",
            ),
        ),
        (
            "line height",
            page(
                "<p style=\"line-height:30px\">a <span style=\"line-height:50px\">b</span></p><p style=\"line-height:0\">c</p><p style=\"line-height:2\">d<br>e</p>",
            ),
        ),
        (
            "images",
            page(
                "<img src=\"a.png\"><img src=\"b.png\" width=\"40\"><img src=\"c.png\" style=\"height:20px\"><img src=\"d.png\" style=\"width:50%;height:30%\"><img src=\"e.png\" style=\"max-width:10px\"><img src=\"\" style=\"max-height:5px\"><img src=\"f.png\" style=\"display:block;height:10%\">",
            ),
        ),
        (
            "svgs",
            page(
                "<svg width=\"40\" height=\"20\"></svg><svg viewBox=\"0 0 10 5\"></svg><svg viewBox=\"0 0 4 4\" style=\"width:20px\"></svg><svg></svg>",
            ),
        ),
        (
            "lists",
            page(
                "<ul><li>a</li><li style=\"list-style-position:inside\">b</li></ul><ol start=\"3\"><li>c</li><li style=\"list-style-image:url(m.png)\">d</li><li></li></ol>",
            ),
        ),
        (
            "textareas and inputs",
            page(
                "<textarea rows=\"3\">a\nb</textarea><textarea style=\"white-space:pre-wrap\" rows=\"x\">c</textarea><input type=\"checkbox\"><input type=\"radio\"><input value=\"v\"><button>b</button>",
            ),
        ),
        (
            "percentages, min and max",
            page(
                "<div style=\"width:50%;min-width:200px\">a</div><div style=\"width:50%;max-width:100px\">b</div><div style=\"height:50%;min-height:10px;max-height:5px\">c</div><div style=\"min-height:40px\">d</div>",
            ),
        ),
        (
            "intrinsic widths",
            page(
                "<div style=\"width:min-content\">a bb ccc</div><div style=\"width:max-content\">a bb ccc</div><div style=\"width:fit-content\">a bb</div><div style=\"float:left\">float shrinks to fit</div>",
            ),
        ),
        (
            "box-sizing",
            page(
                "<div style=\"box-sizing:border-box;width:100px;padding:10px;border:5px solid;height:50px\">a</div><div style=\"box-sizing:border-box;min-width:50px;max-width:60px;padding:20px\">b</div>",
            ),
        ),
        (
            "aspect-ratio",
            page(
                "<div style=\"aspect-ratio:2;width:100px\"></div><div style=\"aspect-ratio:1;max-width:40px\"></div><div style=\"aspect-ratio:0.5;min-width:300px;box-sizing:border-box;padding:5px\"></div>",
            ),
        ),
        (
            "containment",
            page(
                "<div style=\"contain:size\">a</div><div style=\"contain:inline-size;float:left\">b</div><div style=\"contain:layout\">c</div><div style=\"contain:paint;height:10px\">d d d</div>",
            ),
        ),
        (
            "overflow and scroll views",
            page(
                "<div style=\"overflow:auto;height:30px\"><p>a</p><p>b</p><p>c</p></div><div style=\"overflow:hidden;width:20px\">wide wide wide</div><div style=\"overflow:scroll\">s</div>",
            ),
        ),
        (
            "text indent",
            page(
                "<p style=\"text-indent:20px;width:100px\">indented words on two lines or more</p><p style=\"text-indent:-10px\">n</p><p style=\"text-indent:10%;list-style:inside;display:list-item\">l</p>",
            ),
        ),
        ("blocks in inlines", page("<span>a<div>b</div>c</span><b>d<p>e</p></b>")),
        (
            "display none and empty",
            page("<div style=\"display:none\">x</div><img src=\"a.png\" style=\"display:none\"><span></span><div></div>"),
        ),
        (
            "nested inline contexts",
            page("<div style=\"display:inline-block;width:100px\"><span style=\"display:inline-block\">a <b>b</b></span> c</div>"),
        ),
        (
            "quirks mode line",
            "<html><body><p><img src=\"a.png\"></p><div><span style=\"display:inline-block\"></span></div></body></html>".to_string(),
        ),
        (
            "percentage heights without a height",
            page("<div style=\"height:50%\">a</div><div style=\"height:100px\"><div style=\"height:50%\">b</div></div>"),
        ),
        ("relative blocks with percentages", page("<div style=\"position:relative;top:10%;bottom:5px;right:10px\">a</div>")),
        ("sticky", page("<div style=\"position:sticky;top:0\">s</div><p>a</p>")),
    ]
}

// ---- Flex inputs (stage L10) --------------------------------------------------------------------------------

/// A container's flex declaration: direction, wrapping, the three alignments with every value the fork parses, gaps,
/// and the container's own sizes.
fn flex_container_declaration(random: &mut Lcg) -> String {
    match random.below(16) {
        0 | 1 => format!("flex-direction:{}", pick(random, &["row", "row-reverse", "column", "column-reverse"])),
        2 | 3 => format!("flex-wrap:{}", pick(random, &["nowrap", "wrap", "wrap-reverse", "wrap"])),
        4 => format!("flex-flow:{} {}", pick(random, &["row", "column", "row-reverse"]), pick(random, &["wrap", "wrap-reverse", "nowrap"])),
        5 | 6 => format!(
            "justify-content:{}",
            pick(
                random,
                &[
                    "normal",
                    "flex-start",
                    "flex-end",
                    "center",
                    "space-between",
                    "space-around",
                    "start",
                    "end",
                    "left",
                    "right",
                    "space-evenly",
                    "stretch",
                    "safe center",
                ]
            )
        ),
        7 | 8 => format!(
            "align-items:{}",
            pick(
                random,
                &[
                    "normal",
                    "stretch",
                    "baseline",
                    "first baseline",
                    "last baseline",
                    "center",
                    "start",
                    "end",
                    "self-start",
                    "self-end",
                    "flex-start",
                    "flex-end",
                    "safe center",
                    "unsafe end",
                ]
            )
        ),
        9 => format!(
            "align-content:{}",
            pick(random, &["flex-start", "start", "flex-end", "end", "center", "space-between", "space-around", "stretch", "normal"])
        ),
        10 => format!("gap:{}", pick(random, &["0", "4px", "10px", "1em", "5%", "10px 3px", "calc(2% + 3px)"])),
        11 => format!("{}:{}", pick(random, &["row-gap", "column-gap"]), pick(random, &["6px", "10%", "0", "0.5em"])),
        12 => format!("width:{}", pick(random, &["300px", "50%", "auto", "120px", "min-content", "max-content", "fit-content"])),
        13 => format!("height:{}", pick(random, &["100px", "auto", "50%", "40px", "0"])),
        14 => format!(
            "{}:{}",
            pick(random, &["min-height", "max-height", "min-width", "max-width"]),
            pick(random, &["50px", "200px", "30%", "none", "10px"])
        ),
        _ => format!("box-sizing:border-box;padding:{};border:2px solid", pick(random, &["3px", "10px", "5%"])),
    }
}

/// A flex item's declaration: the factors and basis (every keyword, lengths, %, negative, calc), `order`,
/// `align-self`, `auto` margins, sizes with min and max, box-sizing, overflow, `aspect-ratio`.
fn flex_item_declaration(random: &mut Lcg) -> String {
    match random.below(22) {
        0 | 1 => format!(
            "flex:{}",
            pick(
                random,
                &["1", "auto", "none", "0 0 auto", "2 1 30%", "1 0 0", "0.5", "1 1 calc(20% + 5px)", "3 2 40px", "0 1 auto", "initial"]
            )
        ),
        2 => format!("flex-grow:{}", pick(random, &["0", "1", "2.5", "0.3", "-1", "0.0005", "1e9"])),
        3 => format!("flex-shrink:{}", pick(random, &["0", "1", "3", "0.2", "-1", "0.0004"])),
        4 | 5 => format!(
            "flex-basis:{}",
            pick(
                random,
                &[
                    "auto",
                    "content",
                    "min-content",
                    "max-content",
                    "fit-content",
                    "0",
                    "50px",
                    "30%",
                    "-10px",
                    "calc(10% + 8px)",
                    "2em",
                    "100%"
                ]
            )
        ),
        6 => format!("order:{}", pick(random, &["-1", "0", "1", "2", "5"])),
        7 | 8 => format!(
            "align-self:{}",
            pick(
                random,
                &[
                    "auto",
                    "normal",
                    "stretch",
                    "baseline",
                    "last baseline",
                    "center",
                    "start",
                    "end",
                    "self-start",
                    "self-end",
                    "flex-start",
                    "flex-end",
                    "safe end",
                ]
            )
        ),
        9 => format!("margin:{}", pick(random, &["auto", "0 auto", "auto 0", "5px auto", "auto 5px auto 0", "10px", "0 0 0 auto", "-5px"])),
        10 => format!("margin-{}:auto", pick(random, &["left", "right", "top", "bottom"])),
        11 | 12 => {
            format!("width:{}", pick(random, &["50px", "30%", "auto", "150px", "min-content", "max-content", "fit-content", "100%"]))
        }
        13 => format!("height:{}", pick(random, &["20px", "auto", "50%", "3em", "100px"])),
        14 | 15 => format!(
            "{}:{}",
            pick(random, &["min-width", "max-width", "min-height", "max-height"]),
            pick(random, &["0", "40px", "80px", "20%", "none", "auto", "min-content"])
        ),
        16 => format!("padding:{}", pick(random, &["2px", "5px 10px", "10%", "1em"])),
        17 => format!("border:{} solid", pick(random, &["1px", "3px", "0"])),
        18 => format!("box-sizing:{}", pick(random, &["border-box", "content-box"])),
        19 => format!("overflow:{}", pick(random, &["hidden", "auto", "visible", "clip", "scroll"])),
        20 => format!("aspect-ratio:{}", pick(random, &["1", "2", "0.5"])),
        _ => pick(
            random,
            &[
                "display:inline-block",
                "display:list-item",
                "position:relative;top:3px",
                "white-space:nowrap",
                "float:left",
                "display:none",
                "font-size:20px",
                "line-height:2",
                "vertical-align:middle",
            ],
        )
        .to_string(),
    }
}

/// A generated document of nested flex containers (row and column, inline-flex too) with items of every kind:
/// blocks, runs of text, inlines, images, controls, inline blocks, nested containers.
fn flex_document(random: &mut Lcg) -> Vec<u8> {
    fn content(random: &mut Lcg, depth: u32, out: &mut String) {
        match random.below(12) {
            0..=2 => words(random, out),
            3 => out.push_str(&format!("<img src=\"{}.png\">", pick(random, &["a", "bb", "photo", "x9"]))),
            4 => out.push_str(pick(
                random,
                &["<span>s <b>t</b></span>", "<b>bold</b> ", "<br>", "<span style=\"display:inline-block;width:30px\">ib</span>"],
            )),
            5 => out.push_str(pick(
                random,
                &[
                    "<input type=\"text\" value=\"v\">",
                    "<select><option>o</option></select>",
                    "<input type=\"number\">",
                    "<button>b</button>",
                ],
            )),
            6 if depth < 4 => container(random, depth + 1, out),
            _ => item(random, depth, out),
        }
    }
    fn item(random: &mut Lcg, depth: u32, out: &mut String) {
        let tag = pick(random, &["div", "div", "p", "span", "section", "li", "a"]);
        out.push_str(&format!("<{tag}"));
        if random.chance(3) {
            out.push_str(&format!(" class=\"{}\"", pick(random, &["i", "j", "i j"])));
        }
        if !random.chance(4) {
            let decls: Vec<String> = (0..1 + random.below(4)).map(|_| flex_item_declaration(random)).collect();
            out.push_str(&format!(" style=\"{}\"", decls.join(";")));
        }
        out.push('>');
        for _ in 0..random.below(3) {
            if depth < 4 && random.chance(5) {
                container(random, depth + 1, out);
            } else {
                words(random, out);
            }
        }
        out.push_str(&format!("</{tag}>"));
    }
    fn container(random: &mut Lcg, depth: u32, out: &mut String) {
        let display = if random.chance(5) { "inline-flex" } else { "flex" };
        let mut decls: Vec<String> = (0..1 + random.below(5)).map(|_| flex_container_declaration(random)).collect();
        decls.insert(0, format!("display:{display}"));
        if random.chance(3) {
            decls.push(flex_item_declaration(random));
        }
        let class = if random.chance(3) { " class=\"f\"" } else { "" };
        out.push_str(&format!("<div{class} style=\"{}\">", decls.join(";")));
        let n = if depth > 3 { random.below(3) } else { random.below(6) };
        for _ in 0..n {
            content(random, depth, out);
        }
        out.push_str("</div>");
    }
    let mut out = String::new();
    if !random.chance(8) {
        out.push_str("<!DOCTYPE html>");
    }
    out.push_str("<html><head><style>");
    for _ in 0..random.below(4) {
        let selector = pick(random, &[".f", ".i", ".j", ".f > div", "div", "p", ".f > :first-child", ".f > :last-child"]);
        let decls: Vec<String> = (0..1 + random.below(3))
            .map(|_| if random.chance(2) { flex_container_declaration(random) } else { flex_item_declaration(random) })
            .collect();
        let rule = format!("{selector}{{{}}}", decls.join(";"));
        if random.chance(3) {
            out.push_str(&format!("@media (min-width:{}px){{{rule}}}", pick(random, &["300", "450", "700"])));
        } else {
            out.push_str(&rule);
        }
    }
    out.push_str("</style></head><body>");
    for _ in 0..1 + random.below(3) {
        if random.chance(4) {
            content(random, 1, &mut out);
        } else {
            container(random, 0, &mut out);
        }
    }
    out.push_str("</body></html>");
    out.into_bytes()
}

/// Flex shapes for each branch, written out.
fn written_flex_documents() -> Vec<(&'static str, String)> {
    let page = |body: &str| format!("<!DOCTYPE html><html><head></head><body>{body}</body></html>");
    let items = "<div>one</div><div>two words</div><div>three more words here</div>";
    let mut docs = vec![
        ("grow", page("<div style=\"display:flex\"><div style=\"flex:1\">a</div><div style=\"flex:2\">b</div><div>c</div></div>")),
        (
            "grow under one in all",
            page("<div style=\"display:flex\"><div style=\"flex-grow:0.2\">a</div><div style=\"flex-grow:0.3\">b</div></div>"),
        ),
        (
            "shrink with content minimums",
            page(
                "<div style=\"display:flex;width:150px\"><div>supercalifragilistic</div><div style=\"flex-shrink:3\">aaaa bbbb cccc dddd</div><div style=\"flex-shrink:0;width:60px\">x</div></div>",
            ),
        ),
        (
            "shrink under one in all",
            page(
                "<div style=\"display:flex;width:100px\"><div style=\"flex:0 0.2 80px\">a</div><div style=\"flex:0 0.3 90px\">b</div></div>",
            ),
        ),
        (
            "min and max clamps",
            page(
                "<div style=\"display:flex;width:400px\"><div style=\"flex:1;max-width:50px\">a</div><div style=\"flex:1;min-width:250px\">b</div><div style=\"flex:1\">c</div></div><div style=\"display:flex;width:100px\"><div style=\"flex:1 1 200px;min-width:90px\">d</div><div style=\"flex:1 1 200px;max-width:20px\">e</div></div>",
            ),
        ),
        ("wrap", page(&format!("<div style=\"display:flex;flex-wrap:wrap;width:120px\">{items}{items}</div>"))),
        (
            "wrap-reverse",
            page(&format!("<div style=\"display:flex;flex-wrap:wrap-reverse;width:120px;height:200px\">{items}{items}</div>")),
        ),
        (
            "row-reverse and column-reverse",
            page(&format!(
                "<div style=\"display:flex;flex-direction:row-reverse\">{items}</div><div style=\"display:flex;flex-direction:column-reverse;height:120px\">{items}</div>"
            )),
        ),
        ("column with an auto height", page(&format!("<div style=\"display:flex;flex-direction:column\">{items}</div>"))),
        (
            "column with a height and wrapping",
            page(&format!("<div style=\"display:flex;flex-direction:column;flex-wrap:wrap;height:60px\">{items}{items}</div>")),
        ),
        (
            "column wrapping under a max-height",
            page(&format!("<div style=\"display:flex;flex-direction:column;flex-wrap:wrap;max-height:50px\">{items}</div>")),
        ),
        (
            "column min and max heights",
            page(&format!(
                "<div style=\"display:flex;flex-direction:column;min-height:200px\">{items}</div><div style=\"display:flex;flex-direction:column;height:300px;max-height:100px\"><div style=\"flex:1\">a</div><div style=\"flex:1;max-height:10px\">b</div><div style=\"min-height:50px;flex:0 1 0\">c</div></div>"
            )),
        ),
        (
            "column bases",
            page(
                "<div style=\"display:flex;flex-direction:column;height:200px\"><div style=\"flex-basis:30%\">a</div><div style=\"flex-basis:20px\">b</div><div style=\"flex-basis:min-content\">c</div><div style=\"flex-basis:max-content;height:10px\">d</div><div style=\"height:40px\">e</div><div style=\"overflow:hidden;flex:1 1 0\">f</div></div><div style=\"display:flex;flex-direction:column\"><div style=\"flex-basis:50%\">g</div></div>",
            ),
        ),
        (
            "row bases",
            page(
                "<div style=\"display:flex\"><div style=\"flex-basis:30%\">a</div><div style=\"flex-basis:content\">b b</div><div style=\"flex-basis:min-content\">c c c</div><div style=\"flex-basis:max-content\">d d</div><div style=\"flex-basis:fit-content\">e</div><div style=\"flex-basis:-5px\">f</div><div style=\"width:40px\">g</div><div style=\"width:20%;overflow:hidden\">h</div></div>",
            ),
        ),
        (
            "order",
            page(
                "<div style=\"display:flex\"><div style=\"order:2\">a</div><div style=\"order:-1\">b</div><div>c</div><div style=\"order:2\">d</div></div>",
            ),
        ),
        (
            "gaps",
            page(&format!(
                "<div style=\"display:flex;gap:10px 5px;flex-wrap:wrap;width:150px\">{items}{items}</div><div style=\"display:flex;flex-direction:column;row-gap:10%;column-gap:2em\">{items}</div><div style=\"display:flex;column-gap:10%;flex-wrap:wrap;row-gap:5%\">{items}</div><div style=\"display:flex;gap:1em\">{items}</div>"
            )),
        ),
        (
            "auto margins on the main axis",
            page(
                "<div style=\"display:flex\"><div style=\"margin-left:auto\">a</div><div style=\"margin:0 auto\">b</div><div style=\"margin-right:auto\">c</div></div><div style=\"display:flex;flex-direction:column;height:200px\"><div style=\"margin-top:auto\">d</div><div style=\"margin-bottom:auto\">e</div></div>",
            ),
        ),
        (
            "auto margins on the cross axis",
            page(
                "<div style=\"display:flex;height:100px;flex-wrap:wrap;width:80px\"><div style=\"margin:auto\">a</div><div style=\"margin-top:auto\">b</div><div style=\"margin-bottom:auto\">c</div><div style=\"margin:auto 0\">dd dd</div></div><div style=\"display:flex;flex-direction:column\"><div style=\"margin:0 auto\">e</div><div style=\"margin-left:auto;align-self:center\">f</div><div style=\"margin-right:auto\">g</div></div>",
            ),
        ),
        (
            "baselines",
            page(
                "<div style=\"display:flex;align-items:baseline\"><div style=\"font-size:30px\">big</div><div>small</div><div style=\"padding-top:20px\">pad</div></div><div style=\"display:flex;align-items:last baseline\"><div>a<br>b</div><div style=\"font-size:24px\">c</div></div><div style=\"display:flex;flex-wrap:wrap-reverse;align-items:baseline\"><div>x</div><div style=\"align-self:last baseline\">y<br>z</div></div><div style=\"display:flex;flex-direction:column;align-items:baseline\"><div>c1</div><div style=\"align-self:last baseline\">c2</div></div><div style=\"display:flex;flex-direction:column;flex-wrap:wrap-reverse;height:40px;align-items:baseline\"><div>c3</div><div style=\"align-self:last baseline\">c4</div></div>",
            ),
        ),
        (
            "inline flex on a line",
            page(
                "<p>before <span style=\"display:inline-flex;align-items:baseline\"><span style=\"font-size:20px\">a</span><span>b</span></span> between <span style=\"display:inline-flex;flex-direction:column\"><span>c</span><span>d</span></span> after <span style=\"display:inline-flex\"></span> end</p>",
            ),
        ),
        (
            "nested flex",
            page(
                "<div style=\"display:flex\"><div style=\"display:flex;flex-direction:column;flex:1\"><div style=\"display:flex\"><div style=\"flex:1\">a</div><div>b</div></div><div>c</div></div><div style=\"display:flex;flex-wrap:wrap;flex:2\"><div style=\"width:60%\">d</div><div style=\"width:60%\">e</div></div></div>",
            ),
        ),
        (
            "texts, inlines and images as items",
            page(
                "<div style=\"display:flex\">text before <b>bold</b> <img src=\"a.png\"> <span>span</span> text after   <svg width=\"20\" height=\"10\"></svg><img src=\"b.png\" style=\"flex:1;min-width:0\"></div>",
            ),
        ),
        (
            "controls",
            page(
                "<input type=\"text\" value=\"a value\"><select><option>one</option><option selected>two</option></select><input type=\"number\" value=\"3\"><input type=\"date\"><input type=\"color\"><input type=\"search\" placeholder=\"s\"><input type=\"password\" style=\"width:40px\"><select multiple size=\"2\"><option>a</option></select>",
            ),
        ),
        ("a range input", page("<input type=\"range\"><input type=\"range\" value=\"10\" min=\"0\" max=\"20\" style=\"width:30px\">")),
        (
            "stretch with fixed sizes",
            page(
                "<div style=\"display:flex;height:80px;align-content:stretch;flex-wrap:wrap;width:100px\"><div style=\"width:60px\">a</div><div style=\"width:60px;height:10px\">b</div></div><div style=\"display:flex;flex-direction:column;width:200px\"><div>c</div><div style=\"width:50px\">d</div><div style=\"align-self:center\">e</div></div>",
            ),
        ),
        (
            "a fixed flex box laid out again",
            page(
                "<div style=\"display:flex;width:200px;height:60px;margin:5px;padding:3px;border:1px solid\"><div style=\"flex:1\">a</div><div>b</div></div><div style=\"display:flex;width:100px;height:30px\"><img src=\"c.png\"></div>",
            ),
        ),
        (
            "a flex box in a measured box",
            page(
                "<div style=\"display:inline-block\"><div style=\"display:flex\"><div>a b c</div><div style=\"flex:1\">d</div></div></div><div style=\"float:left\"><div style=\"display:flex;flex-direction:column\"><div>e e</div></div></div><div style=\"width:max-content\"><div style=\"display:flex;flex-wrap:wrap\"><div>f</div><div>g</div></div></div>",
            ),
        ),
        (
            "box-sizing and percentages",
            page(
                "<div style=\"display:flex;box-sizing:border-box;padding:10px;width:50%;border:5px solid\"><div style=\"box-sizing:border-box;width:50%;padding:5px\">a</div><div style=\"flex-basis:25%;padding:2%;margin:3%\">b</div></div>",
            ),
        ),
        (
            "aspect ratio items",
            page(
                "<div style=\"display:flex\"><div style=\"aspect-ratio:1;flex:1\"></div><div style=\"aspect-ratio:2;width:50px\"></div></div>",
            ),
        ),
        (
            "an empty flex box",
            page(
                "<div style=\"display:flex\"></div><div style=\"display:flex;flex-direction:column;height:20px\"></div><div style=\"display:flex\">   </div>",
            ),
        ),
        (
            "size containment and overflow",
            page(
                "<div style=\"display:flex;contain:size\"><div>a</div></div><div style=\"display:flex;overflow:auto;height:20px;flex-direction:column\"><div>b</div><div>c</div><div>d</div></div>",
            ),
        ),
        (
            "intrinsic container widths",
            page(&format!(
                "<div style=\"display:flex;width:min-content\">{items}</div><div style=\"display:flex;width:max-content;flex-direction:column\">{items}</div><div style=\"display:flex;width:fit-content;flex-wrap:wrap\">{items}</div>"
            )),
        ),
    ];
    for (what, body) in [
        (
            "negative bases through calc",
            "<div style=\"display:flex\"><div style=\"flex-basis:calc(10px - 30px)\">a</div><div style=\"flex-basis:calc(-10%)\">b</div><div style=\"flex-basis:calc(-10% - 5px)\">b2</div></div><div style=\"display:flex;flex-direction:column;height:90px\"><div style=\"flex-basis:calc(5px - 9px)\">c</div><div style=\"flex-basis:calc(-20% + 2px)\">d</div><div style=\"flex-basis:calc(-10% - 5px)\">d2</div></div>",
        ),
        (
            "factors past every int",
            "<div style=\"display:flex\"><div style=\"flex-grow:1e40\">a</div><div style=\"flex-grow:3e9\">b</div></div><div style=\"display:flex;width:50px\"><div style=\"flex-shrink:1e40;width:80px\">c</div><div style=\"flex:0 4e9 60px\">d</div></div>",
        ),
    ] {
        docs.push((what, page(body)));
    }
    // A split of 1794.9px in 3, 6 or 7 leaves a remainder past pixel_t's band, handed out a pixel at a time.
    for (justify, n) in [("space-between", 4), ("space-around", 3), ("space-evenly", 6)] {
        let empty = "<div></div>".repeat(n);
        docs.push((
            "remainders of a split",
            page(&format!(
                "<div style=\"display:flex;justify-content:{justify};width:1794.9px\">{empty}</div><div style=\"display:flex;justify-content:{justify};flex-direction:column;height:1794.9px\">{empty}</div><div style=\"display:flex;justify-content:{justify};width:100003.7px\">{items}</div>"
            )),
        ));
    }
    // Every justify-content, align-items and align-content value in a row and a column.
    for justify in [
        "normal",
        "flex-start",
        "flex-end",
        "center",
        "space-between",
        "space-around",
        "start",
        "end",
        "left",
        "right",
        "space-evenly",
        "stretch",
    ] {
        docs.push((
            "justify-content",
            page(&format!(
                "<div style=\"display:flex;justify-content:{justify};width:300px\">{items}</div><div style=\"display:flex;justify-content:{justify};flex-direction:column-reverse;height:150px\">{items}</div><div style=\"display:flex;justify-content:{justify};flex-direction:row-reverse\"><div>solo</div></div><div style=\"display:flex;justify-content:{justify};width:50px\">{items}</div>"
            )),
        ));
    }
    for align in
        ["normal", "stretch", "baseline", "last baseline", "center", "start", "end", "self-start", "self-end", "flex-start", "flex-end"]
    {
        docs.push((
            "align-items",
            page(&format!(
                "<div style=\"display:flex;align-items:{align};height:80px\">{items}<div style=\"height:20px\">h</div></div><div style=\"display:flex;align-items:{align};flex-wrap:wrap-reverse;width:100px\">{items}</div><div style=\"display:flex;flex-direction:column;align-items:{align}\">{items}<div style=\"width:30px\">w</div></div><div style=\"display:flex;flex-direction:column;flex-wrap:wrap-reverse;height:50px;align-items:{align}\">{items}</div>"
            )),
        ));
    }
    for content in ["flex-start", "start", "flex-end", "end", "center", "space-between", "space-around", "stretch"] {
        docs.push((
            "align-content",
            page(&format!(
                "<div style=\"display:flex;flex-wrap:wrap;align-content:{content};width:100px;height:300px\">{items}{items}</div><div style=\"display:flex;flex-wrap:wrap-reverse;align-content:{content};width:100px;min-height:250px\">{items}</div><div style=\"display:flex;flex-direction:column;flex-wrap:wrap;align-content:{content};height:60px\">{items}{items}</div><div style=\"display:flex;flex-wrap:wrap;align-content:{content};width:100px;height:10px\">{items}</div>"
            )),
        ));
    }
    docs
}

// ---- Positioned inputs (stage L9) ---------------------------------------------------------------------------

/// An offset: lengths, percentages, negative, `calc()`, `auto`.
fn offset(random: &mut Lcg) -> &'static str {
    pick(random, &["0", "5px", "-8px", "10%", "50%", "100%", "auto", "1.5em", "calc(20% - 4px)", "33.3px", "-25%", "auto", "12px"])
}

/// A declaration of a positioned box or of what holds one: the position and offsets, sizes with min and max, `auto`
/// margins, `z-index`, transforms and containment, overflow, displays, padding and borders, box-sizing, direction.
fn positioned_declaration(random: &mut Lcg) -> String {
    match random.below(26) {
        0..=3 => format!("position:{}", pick(random, &["absolute", "absolute", "fixed", "relative", "sticky", "static", "absolute"])),
        4..=7 => format!("{}:{}", pick(random, &["top", "right", "bottom", "left"]), offset(random)),
        8 => format!("inset:{}", pick(random, &["0", "10px", "5% 10%", "auto 0", "0 auto auto 0", "10px 20px 30px 40px"])),
        9 | 10 => format!(
            "width:{}",
            pick(random, &["auto", "50px", "40%", "100%", "min-content", "max-content", "fit-content", "calc(50% - 10px)", "200px"])
        ),
        11 => format!("height:{}", pick(random, &["auto", "30px", "50%", "100%", "0", "2em"])),
        12 => format!(
            "{}:{}",
            pick(random, &["min-width", "max-width", "min-height", "max-height"]),
            pick(random, &["10px", "60px", "50%", "none", "200px", "0"])
        ),
        13 => pick(
            random,
            &[
                "margin:auto",
                "margin:0 auto",
                "margin-left:auto",
                "margin-right:auto",
                "margin-top:auto;margin-bottom:auto",
                "margin-top:auto",
                "margin-bottom:auto",
                "margin:5px",
                "margin:-10px 3px",
                "margin:10%",
            ],
        )
        .to_string(),
        14 => format!("z-index:{}", pick(random, &["-1", "0", "1", "2", "5", "auto", "99999999999", "-3", "1"])),
        15 => format!("transform:{}", pick(random, &["translate(5px,3px)", "rotate(10deg)", "none", "scale(2)"])),
        16 => format!("contain:{}", pick(random, &["layout", "paint", "strict", "none", "size", "content"])),
        17 => format!("overflow:{}", pick(random, &["hidden", "auto", "scroll", "visible", "clip"])),
        18 => format!("display:{}", pick(random, &["block", "inline", "inline-block", "flex", "inline-flex", "none", "list-item"])),
        19 => format!("padding:{}", pick(random, &["0", "3px", "1em", "10%", "7px 2px"])),
        20 => format!("border:{} solid", pick(random, &["0", "1px", "3px"])),
        21 => format!("box-sizing:{}", pick(random, &["border-box", "content-box"])),
        22 => format!("direction:{}", pick(random, &["rtl", "ltr"])),
        23 => format!("float:{}", pick(random, &["left", "right", "none"])),
        24 => "visibility:hidden".to_string(),
        _ => layout_declaration(random),
    }
}

/// A generated document of absolute, fixed, relative and sticky boxes nested in one another, in blocks, inlines,
/// flex containers, transformed, contained and scrolling boxes, with offsets of every kind, sizes, `auto` margins and
/// `z-index`; sometimes the root or the body positioned.
fn positioned_document(random: &mut Lcg) -> Vec<u8> {
    fn element(random: &mut Lcg, depth: u32, out: &mut String) {
        let tag = pick(random, &["div", "div", "div", "span", "p", "img", "section", "b", "svg", "input", "button", "ul", "li", "div"]);
        out.push('<');
        out.push_str(tag);
        match tag {
            "img" => {
                out.push_str(&format!(" src=\"{}.png\"", pick(random, &["a", "bb", "photo", "x9"])));
                if random.chance(3) {
                    out.push_str(&format!(" width=\"{}\"", pick(random, &["20", "100", "50%"])));
                }
            }
            "svg" => out.push_str(pick(random, &[" width=\"40\" height=\"20\"", " viewBox=\"0 0 10 5\"", ""])),
            "input" => out.push_str(pick(random, &[" type=\"range\"", " type=\"checkbox\"", " value=\"v\""])),
            _ => {}
        }
        if random.chance(3) {
            out.push_str(&format!(" class=\"{}\"", pick(random, &["p", "q", "p q", "r"])));
        }
        if !random.chance(4) {
            let mut decls: Vec<String> = (0..1 + random.below(5)).map(|_| positioned_declaration(random)).collect();
            if random.chance(2) {
                decls.insert(0, format!("position:{}", pick(random, &["absolute", "fixed", "relative", "sticky"])));
            }
            out.push_str(&format!(" style=\"{}\"", decls.join(";")));
        }
        out.push('>');
        if matches!(tag, "img" | "input") {
            return;
        }
        if tag == "svg" {
            out.push_str("<rect width=\"4\" height=\"4\"/></svg>");
            return;
        }
        let n = if depth > 5 { random.below(2) } else { random.below(5) };
        for _ in 0..n {
            match random.below(6) {
                0 | 1 => words(random, out),
                _ => element(random, depth + 1, out),
            }
        }
        out.push_str(&format!("</{tag}>"));
    }
    let mut out = String::new();
    if !random.chance(8) {
        out.push_str("<!DOCTYPE html>");
    }
    let root_style = match random.below(6) {
        0 => " style=\"position:relative;padding:7px\"",
        1 => " style=\"position:absolute;top:5px;left:3px\"",
        2 => " style=\"transform:translate(1px,2px)\"",
        _ => "",
    };
    out.push_str(&format!("<html{root_style}><head><style>"));
    for _ in 0..random.below(5) {
        let selector = pick(random, &["div", "span", ".p", ".q", "p", "li", ".r", "img", "body", "section > div", ".p > :first-child"]);
        let decls: Vec<String> = (0..1 + random.below(3)).map(|_| positioned_declaration(random)).collect();
        let rule = format!("{selector}{{{}}}", decls.join(";"));
        if random.chance(3) {
            out.push_str(&format!("@media (min-width:{}px){{{rule}}}", pick(random, &["300", "450", "700"])));
        } else {
            out.push_str(&rule);
        }
    }
    out.push_str("</style></head><body");
    if random.chance(4) {
        out.push_str(pick(random, &[" style=\"position:relative\"", " style=\"overflow:auto;height:300px\"", " style=\"display:flex\""]));
    }
    out.push('>');
    for _ in 0..1 + random.below(5) {
        element(random, 0, &mut out);
    }
    out.push_str("</body></html>");
    out.into_bytes()
}

/// Positioned shapes for each branch, written out.
fn written_positioned_documents() -> Vec<(&'static str, String)> {
    let page = |body: &str| format!("<!DOCTYPE html><html><head></head><body>{body}</body></html>");
    let rel = |inner: &str| {
        page(&format!("<div style=\"position:relative;width:300px;height:200px;padding:10px;border:2px solid\">{inner}</div>"))
    };
    let text = "some words in the box that wrap";
    let mut docs = vec![
        ("top and left", rel(&format!("<div style=\"position:absolute;top:10px;left:20px\">{text}</div>"))),
        ("bottom and right", rel(&format!("<div style=\"position:absolute;bottom:10px;right:20px\">{text}</div>"))),
        (
            "static position in blocks",
            rel(&format!("<p>before</p><div style=\"position:absolute\">{text}</div><p>after</p><div style=\"margin:7px;padding:3px\"><div style=\"position:absolute;width:40px\">s</div></div>")),
        ),
        (
            "static position in inlines",
            rel("<p>text before <span style=\"position:absolute\">abs</span> and after <b style=\"position:absolute;top:2px\">top</b> end</p>"),
        ),
        ("left and right", rel(&format!("<div style=\"position:absolute;left:10px;right:30px\">{text}</div>"))),
        ("top and bottom", rel(&format!("<div style=\"position:absolute;top:10px;bottom:30px\">{text}</div>"))),
        (
            "left, right and a max-width",
            rel(&format!(
                "<div style=\"position:absolute;left:10px;right:10px;max-width:50%;margin:0 auto\">{text}</div><div style=\"position:absolute;left:0;right:0;max-width:100px;margin-left:auto\">x</div>"
            )),
        ),
        (
            "top, bottom and a max-height",
            rel(&format!(
                "<div style=\"position:absolute;top:0;bottom:0;max-height:50px;margin:auto 0\">{text}</div><div style=\"position:absolute;top:0;bottom:0;max-height:30%;margin-bottom:auto\">y</div>"
            )),
        ),
        (
            "every side and auto margins",
            rel(&format!(
                "<div style=\"position:absolute;top:0;bottom:0;left:0;right:0;width:100px;height:50px;margin:auto\">{text}</div><div style=\"position:absolute;top:0;bottom:0;left:0;right:0;width:500px;height:500px;margin:auto\">big</div><div style=\"position:absolute;inset:0;width:80px;height:20px;margin-left:auto;margin-top:auto\">one</div><div style=\"position:absolute;inset:10px;width:80px;height:20px\">over</div>"
            )),
        ),
        (
            "width with auto sides",
            rel(&format!(
                "<p>a</p><div style=\"position:absolute;width:100px;height:40px;margin:auto\">{text}</div><div style=\"position:absolute;width:50%;height:25%\">pct</div>"
            )),
        ),
        (
            "right and width, bottom and height",
            rel(&format!("<div style=\"position:absolute;right:5px;width:120px;bottom:5px;height:30px;margin:auto\">{text}</div>")),
        ),
        (
            "left and width, top and height",
            rel(&format!("<div style=\"position:absolute;left:5%;width:120px;top:5%;height:30px\">{text}</div>")),
        ),
        (
            "min and max",
            rel(&format!(
                "<div style=\"position:absolute;left:0;width:10px;min-width:60px;top:0;height:10px;min-height:20%\">{text}</div><div style=\"position:absolute;right:0;width:400px;max-width:50%;bottom:0;height:400px;max-height:40px\">m</div>"
            )),
        ),
        (
            "box-sizing",
            rel(&format!(
                "<div style=\"position:absolute;box-sizing:border-box;left:0;width:100px;padding:10px;border:3px solid;top:0;height:50px\">{text}</div><div style=\"position:absolute;box-sizing:border-box;left:0;right:0;top:0;bottom:0;padding:20px;max-width:100px\">b</div>"
            )),
        ),
        (
            "absolute flex boxes",
            rel(
                "<div style=\"position:absolute;display:flex;left:10px\"><div>one</div><div>two words</div></div><div style=\"position:absolute;display:flex;left:10px;right:10px;top:40px\"><div style=\"flex:1\">grow</div><div>b</div></div><div style=\"position:absolute;display:inline-flex;right:0;bottom:0;flex-direction:column\"><div>c</div><div>dd</div></div><div style=\"position:absolute;display:flex;width:50px;top:80px\">fixed width but auto height</div>",
            ),
        ),
        (
            "absolute replaced boxes",
            rel(
                "<img src=\"a.png\" style=\"position:absolute;left:0;right:0;margin:0 auto\"><img src=\"bb.png\" style=\"position:absolute;top:0;bottom:0;margin:auto 0\"><img src=\"c.png\" style=\"position:absolute;left:0;right:0;top:0;bottom:0;margin:auto;max-height:10px;max-width:10%\"><svg width=\"40\" height=\"20\" style=\"position:absolute;right:0;left:10px;margin-left:auto\"></svg><img src=\"photo.png\" style=\"position:absolute;inset:0;margin-right:auto\">",
            ),
        ),
        (
            "fixed boxes",
            page(&format!(
                "<div style=\"position:fixed;top:0;left:0;right:0;height:40px\">header</div><div style=\"height:2000px\">{text}</div><div style=\"position:fixed;bottom:10px;right:10px;width:50px;padding:0 2px 0 9px;border-left:4px solid\"><div style=\"position:absolute;left:-30px;width:200px\">wide child</div><div style=\"margin-left:-50px;width:300px\">over</div></div><div style=\"position:fixed;top:50px;left:0;width:40px;padding:0 3px 0 11px;border-left:5px solid\">x</div><div style=\"position:fixed;bottom:0;overflow:hidden;width:20px\"><div style=\"width:400px\">clipped</div></div>"
            )),
        ),
        (
            "fixed boxes inside positioned and transformed boxes",
            page(
                "<div style=\"position:relative;margin:30px;padding:5px\"><p>a</p><div style=\"position:fixed\">static fixed</div><div style=\"position:fixed;top:10%;left:10%\">pct</div></div><div style=\"transform:translate(10px,10px);margin:20px\"><div style=\"position:fixed;right:0\">in a transform</div><div style=\"position:absolute;top:0\">abs in a transform</div></div>",
            ),
        ),
        (
            "nested containing blocks",
            rel(
                "<div style=\"position:absolute;top:10px;left:10px;width:200px;height:150px;padding:4px\"><div style=\"margin:5px\"><div style=\"padding:3px\"><div style=\"position:absolute;bottom:0;right:0\">deep</div><div style=\"position:absolute\">static deep</div></div></div><div style=\"position:relative;top:5px;left:5px;margin:9px\"><span style=\"position:absolute;left:100%;top:50%\">far</span></div></div>",
            ),
        ),
        (
            "the viewport as the containing block",
            page(&format!(
                "<div style=\"position:absolute;bottom:0;right:0\">corner</div><div style=\"margin:20px\"><div style=\"position:absolute;left:50%;top:50%;width:50%;height:50%\">{text}</div></div>"
            )),
        ),
        (
            "a positioned root",
            format!(
                "<!DOCTYPE html><html style=\"position:relative;padding:12px;margin:5px\"><head></head><body style=\"margin:8px\"><div style=\"position:absolute;bottom:0;right:0\">corner</div><div style=\"position:absolute\">static {text}</div></body></html>"
            ),
        ),
        (
            "an absolute root",
            "<!DOCTYPE html><html style=\"position:absolute;top:10px;left:10px;width:300px\"><head></head><body><div style=\"position:absolute;right:0\">r</div></body></html>".to_string(),
        ),
        (
            "transformed and contained holders",
            page(
                "<div style=\"transform:rotate(5deg);margin:10px;padding:5px\"><div style=\"position:absolute;right:0\">t</div></div><div style=\"contain:layout;padding:5px\"><div><div style=\"position:absolute;bottom:0\">c</div></div></div><div style=\"contain:paint;height:40px\"><div style=\"position:absolute;left:0;right:0\">p</div></div><div style=\"contain:strict;width:100px;height:40px\"><div style=\"position:absolute;inset:0\">s</div></div>",
            ),
        ),
        (
            "z-index order",
            rel(
                "<div style=\"position:absolute;z-index:3\">a</div><div style=\"position:relative;z-index:-1\">b</div><div style=\"position:absolute;z-index:0\">c</div><div style=\"position:absolute\">d</div><div style=\"position:absolute;z-index:2\">e</div><div style=\"position:relative;z-index:3\">f</div><div style=\"position:absolute;z-index:-5\">g</div><div style=\"position:sticky;top:0;z-index:1\">h</div><div style=\"position:absolute;z-index:99999999999\">i</div><div style=\"position:absolute;z-index:-99999999999\">j</div>",
            ),
        ),
        ("z-index without absolute boxes", page("<div style=\"position:relative;z-index:2\">a</div><div style=\"position:relative;z-index:1\">b</div>")),
        (
            "scroll views with absolute boxes",
            page(
                "<div style=\"position:relative;overflow:auto;height:60px;width:100px\"><div style=\"position:absolute;top:200px;left:300px\">far</div><p>a</p></div><div style=\"overflow:hidden;position:relative;height:20px\"><div style=\"position:absolute;bottom:-50px\">under</div></div><div style=\"overflow:scroll\"><div style=\"position:fixed;top:0\">fixed out of a scroller</div></div>",
            ),
        ),
        (
            "display none and hidden absolute boxes",
            rel(
                "<div style=\"position:absolute;display:none;top:0\"><div style=\"position:absolute\">inner</div></div><div style=\"position:absolute;visibility:hidden;left:0;right:0\">h</div><div style=\"position:fixed;display:none\">f</div>",
            ),
        ),
        (
            "absolute boxes in flex containers",
            page(
                "<div style=\"display:flex;position:relative;height:100px;align-items:center;justify-content:space-between\"><div>a</div><div style=\"position:absolute;right:0;top:0\">abs</div><div style=\"position:absolute\">static</div><div>b</div></div><div style=\"display:flex;flex-direction:column\"><span style=\"position:absolute;bottom:0\">col</span></div>",
            ),
        ),
        (
            "right to left",
            page(
                "<div dir=\"rtl\" style=\"direction:rtl;position:relative;width:200px;height:50px\"><div style=\"position:absolute\">static rtl</div><div style=\"position:absolute;left:0;right:0;width:50px;margin:0 auto\">c</div><div style=\"position:absolute;left:0;right:0;width:500px;margin:0 auto\">wide</div></div>",
            ),
        ),
        (
            "floats and inline boxes made absolute",
            rel("<div style=\"float:left;position:absolute;right:0\">f</div><span style=\"display:inline-block;position:absolute;bottom:0\">ib</span><li style=\"position:absolute;top:30px\">item</li>"),
        ),
        (
            "intrinsic widths",
            rel(&format!(
                "<div style=\"position:absolute;width:min-content\">{text}</div><div style=\"position:absolute;width:max-content;top:30px\">{text}</div><div style=\"position:absolute;width:fit-content;top:60px;right:0\">{text}</div><div style=\"position:absolute;left:0;right:0;width:calc(50% - 10px);top:90px\">calc</div>"
            )),
        ),
        (
            "a zero-width containing block",
            page(&format!("<div style=\"position:relative;width:0\"><div style=\"position:absolute;left:0;right:0\">{text}</div><div style=\"position:absolute\">{text}</div></div>")),
        ),
        (
            "percentage offsets and margins",
            rel(&format!("<div style=\"position:absolute;top:10%;left:25%;right:calc(5% + 3px);margin:5%;padding:2%\">{text}</div>")),
        ),
        (
            "relative boxes holding absolute ones",
            page(
                "<div style=\"position:relative;top:10px;left:-5px\"><span style=\"position:relative;left:3px\">r <b style=\"position:absolute;right:0\">a</b></span></div><p style=\"position:relative\">x<span style=\"position:absolute;top:100%;left:0\">below</span></p>",
            ),
        ),
        (
            "range inputs",
            page("<input type=\"range\"><input type=\"range\" style=\"width:200px\" value=\"30\"><div style=\"display:flex\"><input type=\"range\" min=\"0\" max=\"10\" value=\"7\"></div>"),
        ),
        (
            "deep absolute chain",
            page("<div style=\"position:absolute;left:1px\"><div style=\"position:absolute;left:2px\"><div style=\"position:absolute;left:3px\"><div style=\"position:fixed;left:4px\"><div style=\"position:absolute;left:5px\">x</div></div></div></div></div>"),
        ),
        (
            "fixed boxes a media change makes static",
            "<!DOCTYPE html><html><head><style>.f{position:fixed;top:0;width:50px}@media (min-width:800px){.f{position:static}}</style></head><body><p>a</p><div class=\"f\">fixed below 800px</div></body></html>".to_string(),
        ),
    ];
    for display in ["block", "inline", "inline-block", "list-item"] {
        docs.push((
            "absolute boxes of every display",
            rel(&format!(
                "<span style=\"display:{display};position:absolute;left:0;right:0;top:0;bottom:0\">all</span><span style=\"display:{display};position:absolute\">none</span><span style=\"display:{display};position:fixed;right:0;bottom:0;width:30px;height:20px\">f</span>"
            )),
        ));
    }
    docs
}

/// Sticky shapes: the renderer refuses `position: sticky` from a widget (`sanitize_declaration`), so it comes from the
/// trusted user sheet here, as from the renderer's own sheets.
fn written_sticky_documents() -> Vec<(&'static str, &'static str, String)> {
    let page = |body: &str| format!("<!DOCTYPE html><html><head></head><body>{body}</body></html>");
    let sheet = ".st{position:sticky;top:0}.sb{position:sticky;bottom:10%}.sl{position:sticky;left:5px}.sr{position:sticky;right:0}.sv{position:sticky;top:-10px;bottom:-10px}.sp{position:sticky;top:20px;left:0;z-index:2}.sx{position:sticky;top:5%;bottom:0;left:1px;right:1px}";
    vec![
        (
            "sticky boxes in scrollers",
            sheet,
            page(
                "<div style=\"overflow:auto;height:100px;padding:5px\"><div class=\"st\">top</div><p>a</p><p>b</p><p>c</p><p>d</p><div class=\"sb\">bottom</div><p>e</p><p>f</p></div><div style=\"overflow:auto;width:100px;white-space:nowrap\"><span class=\"sl\">left</span> wide wide wide wide wide wide <span class=\"sr\" style=\"display:inline-block\">right</span></div><div style=\"overflow:hidden;height:30px;padding-top:40px\"><div class=\"sv\">both</div><p>more</p></div>",
            ),
        ),
        (
            "sticky boxes in the body",
            sheet,
            page(
                "<div class=\"sp\">in the body</div><div style=\"height:900px\"><div class=\"sb\">deep bottom</div><div style=\"height:2000px\"></div><div class=\"st\">late top</div></div>",
            ),
        ),
        (
            "sticky boxes in clipping and positioned boxes",
            sheet,
            page(
                "<div style=\"overflow:clip;height:50px\"><div style=\"position:relative;top:30px;overflow:scroll;height:40px\"><div style=\"height:100px\"></div><div class=\"sx\">x</div></div></div><div style=\"position:absolute;top:0;overflow:auto;height:60px;width:80px\"><div style=\"margin-top:200px\" class=\"st\">in an absolute scroller</div></div><div style=\"position:fixed;bottom:0;overflow:auto;height:20px\"><p>a</p><p class=\"sb\">fixed scroller</p><p>b</p></div>",
            ),
        ),
        (
            "nested sticky boxes",
            sheet,
            page(
                "<div style=\"overflow:auto;height:80px\"><div style=\"height:300px\"></div><div class=\"sb\" style=\"padding:5px;border:2px solid\"><span class=\"sr\">in</span><div class=\"sv\" style=\"margin:3px\">inner</div><div style=\"position:absolute;right:0\">abs in sticky</div></div></div>",
            ),
        ),
    ]
}

/// A trusted user sheet with sticky rules for the generated documents' classes.
fn sticky_user_sheet(random: &mut Lcg) -> String {
    let mut out = String::new();
    for _ in 0..1 + random.below(3) {
        let class = pick(random, &["p", "q", "r"]);
        let decls: Vec<String> =
            (0..1 + random.below(3)).map(|_| format!("{}:{}", pick(random, &["top", "right", "bottom", "left"]), offset(random))).collect();
        out.push_str(&format!(".{class}{{position:sticky;{}}}", decls.join(";")));
    }
    out
}

/// The widths a case is laid out at: its own, another, its own again (the cache).
fn layout_steps(width: f32, other: f32) -> Vec<Step> {
    vec![Step::Layout(width, None), Step::Dump, Step::Layout(other, None), Step::Dump, Step::Layout(width, None), Step::Dump]
}

// ---- Table inputs (stage L12) -------------------------------------------------------------------------------

/// A table's declaration: collapsed or separate borders with spacing, fixed or auto layout, its width and height
/// with min and max, borders and padding of every style, box-sizing, display.
fn table_declaration(random: &mut Lcg) -> String {
    match random.below(16) {
        0 | 1 => format!("border-collapse:{}", pick(random, &["collapse", "separate", "collapse"])),
        2 => format!("border-spacing:{}", pick(random, &["0", "2px", "5px 1px", "1em", "0 7px", "3.3px"])),
        3 | 4 => format!("table-layout:{}", pick(random, &["fixed", "auto", "fixed"])),
        5 | 6 => format!(
            "width:{}",
            pick(random, &["auto", "300px", "100%", "50%", "120px", "calc(50% + 20px)", "1px", "min-content", "max-content", "2000px"])
        ),
        7 => format!("height:{}", pick(random, &["auto", "200px", "50%", "40px", "0"])),
        8 => format!(
            "{}:{}",
            pick(random, &["min-height", "max-height", "min-width", "max-width"]),
            pick(random, &["100px", "30%", "none", "10px", "400px"])
        ),
        9 | 10 => format!(
            "border:{} {} {}",
            pick(random, &["1px", "3px", "0", "medium", "5px"]),
            pick(random, &["solid", "double", "dashed", "dotted", "ridge", "outset", "groove", "inset", "hidden", "none"]),
            pick(random, &["red", "#123", "blue"])
        ),
        11 => format!("padding:{}", pick(random, &["0", "4px", "2px 10px", "5%"])),
        12 => format!("margin:{}", pick(random, &["0", "10px", "0 auto", "auto", "5%"])),
        13 => format!("box-sizing:{}", pick(random, &["border-box", "content-box"])),
        14 => format!("display:{}", pick(random, &["table", "inline-table", "table"])),
        _ => pick(random, &["float:left", "float:right", "vertical-align:middle", "font-size:20px", "direction:rtl"]).to_string(),
    }
}

/// A cell's declaration: its width (lengths, %, calc, keywords) and height, padding and borders of every style,
/// box-sizing, vertical-align, text alignment and wrapping, overflow.
fn cell_declaration(random: &mut Lcg) -> String {
    match random.below(18) {
        0..=3 => format!(
            "width:{}",
            pick(random, &["auto", "50px", "10%", "30%", "60%", "120px", "calc(20% + 5px)", "0", "1000px", "2em", "100%"])
        ),
        4 | 5 => format!("height:{}", pick(random, &["auto", "40px", "50%", "3em", "0", "calc(10px + 1em)", "1e7px", "-5px"])),
        6 | 7 => format!("padding:{}", pick(random, &["0", "5px", "1px 8px", "10%", "3px 0 9px"])),
        8 | 9 => format!(
            "border{}:{} {}",
            pick(random, &["", "-left", "-top", "-right", "-bottom"]),
            pick(random, &["1px", "2px", "4px", "0", "thick"]),
            pick(random, &["solid", "double", "dashed", "dotted", "ridge", "outset", "groove", "inset", "hidden", "none"])
        ),
        10 => format!("box-sizing:{}", pick(random, &["border-box", "content-box"])),
        11 | 12 => format!("vertical-align:{}", pick(random, &["top", "middle", "bottom", "baseline", "text-top", "sub", "4px"])),
        13 => format!("text-align:{}", pick(random, &["left", "center", "right", "justify"])),
        14 => format!("white-space:{}", pick(random, &["nowrap", "pre", "normal"])),
        15 => format!("{}:{}", pick(random, &["min-width", "max-width", "min-height"]), pick(random, &["20px", "50%", "none", "200px"])),
        16 => format!("overflow:{}", pick(random, &["hidden", "auto", "visible"])),
        _ => format!("margin:{}", pick(random, &["5px", "0", "auto"])),
    }
}

/// A generated document of tables (nested, inline, floated, in blocks and flex boxes) with rows in row groups or
/// loose, cells with spans (past the grid too), captions on either side, HTML attributes and declarations, and
/// the content of every kind; some built from `display: table-*` blocks, which take anonymous boxes.
fn table_document(random: &mut Lcg) -> Vec<u8> {
    fn content(random: &mut Lcg, depth: u32, out: &mut String) {
        match random.below(12) {
            0..=4 => words(random, out),
            5 => out.push_str(&format!("<img src=\"{}.png\">", pick(random, &["a", "bb", "photo", "x9"]))),
            6 => out.push_str(pick(
                random,
                &[
                    "<span>s <b>t</b></span>",
                    "<br>",
                    "<span style=\"display:inline-block;width:30px\">ib</span>",
                    "<div>block</div>",
                    "<div style=\"float:left;width:20px\">f</div>",
                    "<p style=\"margin:10px\">p</p>",
                    "<input type=\"text\" value=\"v\">",
                ],
            )),
            7 | 8 if depth < 3 => table(random, depth + 1, out),
            9 if depth < 3 => {
                out.push_str(pick(random, &["<div style=\"display:flex\">", "<div style=\"display:inline-block\">", "<div>"]));
                table(random, depth + 1, out);
                out.push_str("</div>");
            }
            _ => words(random, out),
        }
    }
    fn cell(random: &mut Lcg, depth: u32, out: &mut String) {
        let tag = if random.chance(5) { "th" } else { "td" };
        out.push_str(&format!("<{tag}"));
        if random.chance(4) {
            out.push_str(&format!(" colspan=\"{}\"", pick(random, &["2", "3", "0", "-1", "x", "5", "1000"])));
        }
        if random.chance(5) {
            out.push_str(&format!(" rowspan=\"{}\"", pick(random, &["2", "3", "0", "9", "1"])));
        }
        if random.chance(6) {
            out.push_str(pick(
                random,
                &[" width=\"50\"", " width=\"20%\"", " height=\"30\"", " valign=\"top\"", " align=\"right\"", " width=\"0\""],
            ));
        }
        if random.chance(4) {
            out.push_str(&format!(" class=\"{}\"", pick(random, &["c", "d", "c d"])));
        }
        if random.chance(2) {
            let decls: Vec<String> = (0..1 + random.below(3)).map(|_| cell_declaration(random)).collect();
            out.push_str(&format!(" style=\"{}\"", decls.join(";")));
        }
        out.push('>');
        for _ in 0..random.below(3) {
            content(random, depth, out);
        }
        out.push_str(&format!("</{tag}>"));
    }
    fn row(random: &mut Lcg, depth: u32, out: &mut String) {
        out.push_str("<tr");
        if random.chance(5) {
            out.push_str(&format!(" style=\"height:{}\"", pick(random, &["40px", "20%", "0", "auto", "3em"])));
        }
        out.push('>');
        for _ in 0..random.below(5) {
            cell(random, depth, out);
        }
        out.push_str("</tr>");
    }
    fn table(random: &mut Lcg, depth: u32, out: &mut String) {
        if random.chance(6) {
            // A table of display: table-* blocks, parts left out for the anonymous boxes.
            let mut decls: Vec<String> = (0..random.below(3)).map(|_| table_declaration(random)).collect();
            decls.insert(0, format!("display:{}", pick(random, &["table", "inline-table", "block"])));
            out.push_str(&format!("<div style=\"{}\">", decls.join(";")));
            for _ in 0..1 + random.below(3) {
                match random.below(4) {
                    0 => out.push_str("<div style=\"display:table-cell\">loose cell</div>"),
                    1 => {
                        out.push_str("<div style=\"display:table-row\">");
                        for _ in 0..random.below(3) {
                            out.push_str("<div style=\"display:table-cell;padding:2px\">c ");
                            words(random, out);
                            out.push_str("</div>");
                        }
                        out.push_str("</div>");
                    }
                    2 => out.push_str("<div style=\"display:table-caption\">caption</div>"),
                    _ => {
                        out.push_str("<span>text</span>");
                        words(random, out);
                    }
                }
            }
            out.push_str("</div>");
            return;
        }
        out.push_str("<table");
        if random.chance(4) {
            out.push_str(pick(
                random,
                &[" border=\"1\"", " cellspacing=\"0\"", " cellspacing=\"6\"", " width=\"100%\"", " width=\"200\"", " height=\"150\""],
            ));
        }
        if random.chance(3) {
            out.push_str(&format!(" class=\"{}\"", pick(random, &["t", "u", "t u"])));
        }
        if !random.chance(3) {
            let decls: Vec<String> = (0..1 + random.below(4)).map(|_| table_declaration(random)).collect();
            out.push_str(&format!(" style=\"{}\"", decls.join(";")));
        }
        out.push('>');
        if random.chance(4) {
            let side = pick(random, &["top", "bottom", "top"]);
            out.push_str(&format!("<caption style=\"caption-side:{side}\">"));
            words(random, out);
            out.push_str("</caption>");
        }
        let rows = if depth > 2 { random.below(3) } else { random.below(5) };
        match random.below(4) {
            0 => {
                out.push_str("<thead>");
                row(random, depth, out);
                out.push_str("</thead><tbody>");
                for _ in 0..rows {
                    row(random, depth, out);
                }
                out.push_str("</tbody>");
                if random.chance(2) {
                    out.push_str("<tfoot>");
                    row(random, depth, out);
                    out.push_str("</tfoot>");
                }
            }
            _ => {
                for _ in 0..rows {
                    row(random, depth, out);
                }
            }
        }
        if random.chance(8) {
            out.push_str("<caption style=\"caption-side:bottom\">below</caption>");
        }
        out.push_str("</table>");
    }
    let mut out = String::new();
    if !random.chance(8) {
        out.push_str("<!DOCTYPE html>");
    }
    out.push_str("<html><head><style>");
    for _ in 0..random.below(4) {
        let selector =
            pick(random, &["table", "td", "th", ".t", ".u", ".c", ".d", "tr", "caption", "td:first-child", "tr:nth-child(2) td"]);
        let decls: Vec<String> = (0..1 + random.below(3))
            .map(|_| {
                if selector.starts_with("table") || (selector.starts_with('.') && random.chance(2)) {
                    table_declaration(random)
                } else {
                    cell_declaration(random)
                }
            })
            .collect();
        let rule = format!("{selector}{{{}}}", decls.join(";"));
        if random.chance(3) {
            out.push_str(&format!("@media (min-width:{}px){{{rule}}}", pick(random, &["300", "450", "700"])));
        } else {
            out.push_str(&rule);
        }
    }
    out.push_str("</style></head><body>");
    for _ in 0..1 + random.below(3) {
        if random.chance(4) {
            content(random, 2, &mut out);
        } else {
            table(random, 0, &mut out);
        }
    }
    out.push_str("</body></html>");
    out.into_bytes()
}

/// Table shapes for each branch, written out.
fn written_table_documents() -> Vec<(&'static str, String)> {
    let page = |body: &str| format!("<!DOCTYPE html><html><head></head><body>{body}</body></html>");
    let long = "supercalifragilisticexpialidocious";
    let mut docs = vec![
        ("two by two", page("<table><tr><td>a</td><td>b b b</td></tr><tr><td>c c</td><td>d</td></tr></table>")),
        ("empty table", page("<table></table><table style=\"border:2px solid;padding:3px\"></table><table><tr></tr><tr></tr></table>")),
        ("empty cells", page("<table border=\"1\"><tr><td></td><td>x</td><td></td></tr><tr><td></td></tr></table>")),
        (
            "colspans",
            page(&format!(
                "<table border=\"1\"><tr><td colspan=\"2\">{long} wide</td><td>c</td></tr><tr><td>a</td><td>b</td><td>c</td></tr><tr><td colspan=\"3\">all three columns of text</td></tr></table>"
            )),
        ),
        (
            "rowspans",
            page(
                "<table border=\"1\"><tr><td rowspan=\"2\">tall<br>tall<br>tall<br>tall</td><td>a</td></tr><tr><td>b</td></tr><tr><td>c</td><td rowspan=\"5\">past the end<br>x<br>y</td></tr></table>",
            ),
        ),
        (
            "spans crossing",
            page(
                "<table><tr><td rowspan=\"2\" colspan=\"2\">big</td><td>a</td></tr><tr><td>b</td></tr><tr><td>c</td><td colspan=\"2\">d e f g</td></tr></table>",
            ),
        ),
        (
            "spans of odd values",
            page(
                "<table><tr><td colspan=\"0\">z</td><td colspan=\"-3\">n</td><td colspan=\"x\">x</td><td rowspan=\"0\">r</td></tr><tr><td colspan=\"1000\" rowspan=\"1000\">huge</td></tr></table>",
            ),
        ),
        (
            "captions",
            page(
                "<table border=\"1\"><caption>top caption of some length</caption><caption style=\"caption-side:bottom\">bottom</caption><caption style=\"margin:5px;padding:4px\">second top</caption><tr><td>a</td><td>b</td></tr></table>",
            ),
        ),
        (
            "caption only",
            page(
                "<table style=\"border:4px solid\"><caption>only a caption</caption></table><table><caption style=\"caption-side:bottom\">bottom only</caption></table>",
            ),
        ),
        (
            "row groups",
            page(
                "<table><tfoot><tr><td>foot</td></tr></tfoot><thead><tr><th>head one</th><th>head two</th></tr></thead><tbody><tr><td>a</td><td>b</td></tr></tbody><tbody><tr><td>c</td></tr></tbody></table>",
            ),
        ),
        (
            "fixed layout",
            page(
                "<table style=\"table-layout:fixed;width:300px\"><tr><td style=\"width:50px\">a</td><td>b</td><td style=\"width:20%\">c</td><td>d</td></tr><tr><td style=\"width:200px\">wide in the second row</td><td>e</td></tr></table>",
            ),
        ),
        (
            "fixed layout without auto columns",
            page(
                "<table style=\"table-layout:fixed;width:400px\"><tr><td style=\"width:50px\">a</td><td style=\"width:100px\">b</td></tr></table><table style=\"table-layout:fixed;width:400px\"><tr><td style=\"width:30%\">a</td><td style=\"width:50px\">b</td><td style=\"width:20%\">c</td></tr></table><table style=\"table-layout:fixed;width:100px\"><tr><td style=\"width:80px\">a</td><td style=\"width:90px\">b</td><td>c</td></tr></table>",
            ),
        ),
        (
            "fixed layout past the room",
            page(
                "<table style=\"table-layout:fixed;width:200px\"><tr><td style=\"width:70%\">a</td><td style=\"width:60%\">b</td><td>c</td></tr></table><table style=\"table-layout:fixed;width:200px\"><tr><td style=\"width:150px\">a</td><td style=\"width:40%\">b</td></tr></table><table style=\"table-layout:fixed;width:200px\"><tr><td colspan=\"2\" style=\"width:100px\">span</td><td style=\"width:10px;box-sizing:border-box;padding:4px\">c</td></tr><tr><td>a</td><td>b</td><td>c</td></tr></table>",
            ),
        ),
        (
            "fixed layout of an auto width",
            page("<table style=\"table-layout:fixed\"><tr><td style=\"width:50px\">a</td><td>b</td></tr></table>"),
        ),
        (
            "percentage columns",
            page(
                "<table style=\"width:400px\"><tr><td style=\"width:25%\">a</td><td style=\"width:50%\">b</td><td>c</td></tr></table><table style=\"width:400px\"><tr><td style=\"width:70%\">a</td><td style=\"width:60%\">b</td><td style=\"width:10%\">c</td></tr></table><table><tr><td style=\"width:30%\">a</td><td style=\"width:20%\">b</td></tr></table><table style=\"width:300px\"><tr><td style=\"width:0%\">z</td><td style=\"width:-10%\">n</td><td style=\"width:100%\">all</td></tr></table>",
            ),
        ),
        (
            "fixed width columns",
            page(&format!(
                "<table style=\"width:500px\"><tr><td style=\"width:40px\">a</td><td style=\"width:60px\">b</td></tr></table><table style=\"width:100px\"><tr><td style=\"width:80px\">a</td><td style=\"width:90px\">{long}</td><td>c</td></tr></table><table><tr><td width=\"70\">attr</td><td style=\"width:calc(10% + 20px)\">calc</td><td style=\"width:3em;box-sizing:border-box;padding:5px\">em</td></tr></table>"
            )),
        ),
        (
            "mixed columns with spans",
            page(
                "<table style=\"width:600px\"><tr><td style=\"width:20%\">p</td><td style=\"width:100px\">f</td><td>auto</td><td colspan=\"2\">two auto</td></tr><tr><td colspan=\"3\">a cell over three columns with enough words to need room</td><td>x</td><td style=\"width:15%\">y</td></tr></table>",
            ),
        ),
        (
            "narrow container",
            page(&format!(
                "<div style=\"width:60px\"><table><tr><td>{long}</td><td>another {long}</td></tr></table><table style=\"width:100%\"><tr><td style=\"width:50%\">aaaaaaaaaa</td><td style=\"width:40px\">b</td><td>c</td></tr></table></div>"
            )),
        ),
        (
            "auto columns sharing the room",
            page(
                "<table style=\"width:500px\"><tr><td>a</td><td>bb bb</td><td>ccc ccc ccc</td></tr></table><table style=\"width:500px\"><tr><td></td><td></td><td></td></tr></table><table style=\"width:100%\"><tr><td style=\"width:30px\">x</td><td style=\"width:40px\">y</td></tr></table>",
            ),
        ),
        (
            "one column",
            page(&format!(
                "<table style=\"width:200px\"><tr><td>{long} {long}</td></tr><tr><td>b</td></tr></table><table style=\"width:50%\"><tr><td>c</td></tr></table><table><tr><td>auto one column</td></tr></table>"
            )),
        ),
        (
            "border spacing",
            page(
                "<table style=\"border-spacing:10px 4px;border:1px solid\"><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table><table cellspacing=\"0\" border=\"2\"><tr><td>e</td><td>f</td></tr></table><table style=\"border-spacing:1em\"><tr><td>g</td></tr></table>",
            ),
        ),
        (
            "collapsed borders",
            page(
                "<table style=\"border-collapse:collapse;border:3px solid\"><tr><td style=\"border:1px solid\">a</td><td style=\"border:5px double\">b</td></tr><tr><td style=\"border:2px dashed\">c</td><td style=\"border:2px dotted\">d</td></tr></table>",
            ),
        ),
        (
            "collapsed hidden and none",
            page(
                "<table style=\"border-collapse:collapse;border:2px hidden\"><tr><td style=\"border:4px solid\">a</td><td style=\"border:4px none\">b</td><td style=\"border:1px hidden\">c</td></tr><tr><td colspan=\"2\" style=\"border:3px groove\">d</td><td rowspan=\"2\" style=\"border:3px ridge\">e</td></tr><tr><td style=\"border:3px inset\">f</td><td style=\"border:3px outset\">g</td></tr></table>",
            ),
        ),
        (
            "collapsed ties",
            page(
                "<table style=\"border-collapse:collapse;border:2px solid\"><tr><td style=\"border:2px solid\">a</td><td style=\"border:2px solid\">b</td></tr><tr><td style=\"border:2px dashed\">c</td><td style=\"border:2.00001px solid\">d</td></tr></table><table style=\"border-collapse:collapse\"><tr><td style=\"border-right:3px solid\">a</td><td style=\"border-left:3px double\">b</td></tr></table>",
            ),
        ),
        (
            "table heights",
            page(
                "<table style=\"height:200px;border:1px solid\"><tr><td>a</td></tr><tr style=\"height:30px\"><td>b</td></tr></table><table style=\"height:150px\"><tr style=\"height:20%\"><td>a</td></tr><tr style=\"height:50%\"><td>b</td></tr><tr><td>c</td></tr></table><table style=\"height:100px\"><tr style=\"height:80%\"><td>a</td></tr><tr style=\"height:80%\"><td>b</td></tr></table>",
            ),
        ),
        (
            "rows given back their height",
            page(
                "<table style=\"height:60px\"><tr style=\"height:40%\"><td>a<br>a<br>a</td></tr><tr style=\"height:90%\"><td>b</td></tr><tr style=\"height:30%\"><td>c</td></tr></table><table style=\"height:90px\"><tr style=\"height:30px\"><td>a</td></tr><tr style=\"height:40px\"><td>b</td></tr></table>",
            ),
        ),
        (
            "min heights",
            page(
                "<div style=\"height:300px\"><table style=\"min-height:50%\"><tr><td>a</td></tr></table><table style=\"min-height:120px\"><tr><td>b</td></tr><tr><td>c</td></tr></table></div>",
            ),
        ),
        (
            "cell heights",
            page(
                "<table><tr><td style=\"height:50px\">a</td><td style=\"height:20px;padding:5px\">b</td><td style=\"height:30px;box-sizing:border-box;padding:6px;border:2px solid\">c</td><td style=\"height:50%\">d</td><td style=\"height:1e9px\">e</td><td style=\"height:-4px\">f</td></tr></table>",
            ),
        ),
        (
            "vertical alignment in cells",
            page(
                "<table style=\"height:120px\"><tr><td style=\"vertical-align:top\">t</td><td style=\"vertical-align:middle\">m</td><td style=\"vertical-align:bottom\">b</td><td>base<br>line</td><td valign=\"bottom\"><div>block</div><div>blocks</div></td></tr></table>",
            ),
        ),
        (
            "nested tables",
            page(
                "<table border=\"1\"><tr><td><table border=\"1\" style=\"width:100%\"><tr><td>inner a</td><td>inner b</td></tr></table></td><td>outer</td></tr><tr><td colspan=\"2\"><table style=\"border-collapse:collapse;table-layout:fixed;width:150px\"><tr><td style=\"border:1px solid\">x</td></tr></table></td></tr></table>",
            ),
        ),
        (
            "anonymous table boxes",
            page(
                "<div style=\"display:table\"><div style=\"display:table-cell\">loose cell</div><div style=\"display:table-cell\">and another</div></div><div style=\"display:table-row\"><div style=\"display:table-cell\">row without a table</div></div><div style=\"display:table-cell\">cell alone</div><span style=\"display:table-cell\">inline cell</span>",
            ),
        ),
        (
            "table display on blocks",
            page(
                "<div style=\"display:table;width:300px;border-spacing:4px\"><div style=\"display:table-caption\">cap</div><div style=\"display:table-header-group\"><div style=\"display:table-row\"><div style=\"display:table-cell\">h</div></div></div><div style=\"display:table-row-group\"><div style=\"display:table-row\"><div style=\"display:table-cell\">a</div><div style=\"display:table-cell\">b</div></div></div></div>",
            ),
        ),
        (
            // A media change builds the tree again without the anonymous boxes: the row group is then a cell.
            "a row group inside a row",
            page(
                "<style>@media (min-width:600px){p{margin:1px}}</style><div style=\"display:table\"><div style=\"display:table-row\"><div style=\"display:table-cell\">a</div><div style=\"display:table-row-group\"><div style=\"display:table-cell\">inner</div><div style=\"display:table-row\"><div style=\"display:table-cell\">row</div></div></div></div><div style=\"display:table-row\"><div style=\"display:table-row-group\"><div style=\"display:table-cell\">first</div></div></div></div>",
            ),
        ),
        (
            "inline tables",
            page(
                "<p>before <span style=\"display:inline-table;border:1px solid\"><span style=\"display:table-cell\">x</span><span style=\"display:table-cell\">y</span></span> after <table style=\"display:inline-table\"><tr><td>z</td></tr></table> end</p>",
            ),
        ),
        (
            "tables in a flex box",
            page(
                "<div style=\"display:flex;width:300px\"><table style=\"flex:1\"><tr><td>a</td><td>b</td></tr></table><table><tr><td>c c c</td></tr></table></div><div style=\"display:flex;flex-direction:column\"><table style=\"align-self:center\"><tr><td>d</td></tr></table></div>",
            ),
        ),
        (
            "floated tables",
            page(
                "<table style=\"float:left;width:100px\"><tr><td>left</td></tr></table><table style=\"float:right\"><tr><td>right</td></tr></table><p>text beside the floated tables wraps around them</p>",
            ),
        ),
        (
            "centred and wide tables",
            page(&format!(
                "<table style=\"margin:0 auto;width:50%\"><tr><td>centred</td></tr></table><table style=\"margin-left:auto\"><tr><td>right</td></tr></table><table style=\"width:2000px\"><tr><td>wide</td><td>{long}</td></tr></table>"
            )),
        ),
        (
            "table width keywords",
            page(
                "<table style=\"width:min-content\"><tr><td>a b c d</td></tr></table><table style=\"width:max-content\"><tr><td>a b c d</td></tr></table><table style=\"width:fit-content\"><tr><td>a b c d</td></tr></table><table style=\"width:calc(100% - 50px)\"><tr><td>calc</td></tr></table>",
            ),
        ),
        (
            "padded and bordered cells and tables",
            page(
                "<table style=\"padding:10px;border:3px solid;box-sizing:border-box;width:250px\"><tr><td style=\"padding:8px 2px;border:2px solid\">a</td><td style=\"padding:5%\">b</td></tr></table><table style=\"border-collapse:collapse;padding:6px;border:4px solid\"><tr><td style=\"padding:3px;border:1px solid\">c</td></tr></table>",
            ),
        ),
        (
            "cells of long and unbreakable text",
            page(&format!(
                "<table style=\"width:150px\"><tr><td style=\"white-space:nowrap\">no wrap here at all please</td><td>{long}</td><td><img src=\"photo.png\"></td></tr></table>"
            )),
        ),
        (
            "html attributes",
            page(
                "<table width=\"80%\" height=\"100\" border=\"3\" cellspacing=\"5\" bgcolor=\"red\"><tr><td width=\"30\" height=\"40\" valign=\"middle\" align=\"center\">a</td><td width=\"0\">b</td></tr></table>",
            ),
        ),
        (
            "cells of block content",
            page(
                "<table><tr><td><div style=\"margin:10px\">block with margins</div><p>para</p></td><td><ul><li>one</li><li>two</li></ul></td><td><div style=\"float:right;width:20px\">f</div>text</td></tr></table>",
            ),
        ),
        (
            "right to left",
            page("<table dir=\"rtl\" style=\"direction:rtl\"><tr><td>\u{5e9}\u{5dc}\u{5d5}\u{5dd}</td><td>b</td></tr></table>"),
        ),
        (
            "a table in a narrow float and an inline block",
            page(
                "<div style=\"float:left;width:40px\"><table><tr><td>xxxxxxx</td><td>y</td></tr></table></div><span style=\"display:inline-block\"><table><tr><td>in an inline block</td></tr></table></span>",
            ),
        ),
        ("col and colgroup", page("<table><colgroup><col style=\"width:100px\"><col></colgroup><tr><td>a</td><td>b</td></tr></table>")),
        (
            "hover restyle of a cell",
            page("<style>td:hover{width:200px;padding:9px}tr:hover td{height:60px}</style><table><tr><td>a</td><td>b</td></tr></table>"),
        ),
        (
            "media change of a table",
            page(
                "<style>@media (min-width:600px){table{table-layout:fixed;width:500px;border-collapse:collapse}td{border:2px solid}}</style><table><tr><td>a</td><td>b</td></tr></table>",
            ),
        ),
    ];
    // Many columns, wide enough that the budget at init cuts some.
    let mut wide = String::from("<table>");
    for r in 0..3 {
        wide += "<tr>";
        for c in 0..40 {
            wide += &format!("<td>{r}{c}</td>");
        }
        wide += "</tr>";
    }
    wide += "</table>";
    docs.push(("forty columns", page(&wide)));
    // Tables nested twelve deep: each level lays its cells out three times, so the budget runs out.
    let mut deep = String::new();
    for k in 0..12 {
        deep += &format!("<table><tr><td>{k} </td><td>");
    }
    for _ in 0..12 {
        deep += "</td></tr></table>";
    }
    docs.push(("tables nested twelve deep", page(&deep)));
    docs
}

// ---- Grid inputs (stage L11) --------------------------------------------------------------------------------

/// A track list the fork parses (and some it refuses): lengths, %, `fr`, the content keywords, `minmax()`,
/// `fit-content()`, `repeat()` with a count, `auto-fill` and `auto-fit`, line names, calc.
fn grid_track_list(random: &mut Lcg) -> &'static str {
    pick(
        random,
        &[
            "100px 1fr",
            "repeat(3, 1fr)",
            "repeat(auto-fill, minmax(80px, 1fr))",
            "repeat(auto-fit, minmax(60px, 1fr))",
            "repeat(auto-fill, 100px)",
            "repeat(auto-fit, 50px)",
            "1fr 2fr",
            "auto auto",
            "auto 1fr auto",
            "min-content 1fr max-content",
            "minmax(50px, 200px) auto",
            "minmax(min-content, 1fr) minmax(0, 2fr)",
            "minmax(max-content, auto) 1fr",
            "fit-content(120px) 1fr",
            "20% 30% auto",
            "[a] 50px [b] 1fr [c]",
            "none",
            "repeat(2, 40px 1fr)",
            "minmax(0, 1fr) minmax(0, 1fr)",
            "calc(50% - 10px) 1fr",
            "0.5fr 0.3fr",
            "0.2fr 30px 0.2fr",
            "10px repeat(auto-fit, 30px) 20px",
            "repeat(auto-fill, minmax(10%, 1fr))",
            "1fr",
            "auto",
            "3fr 1fr 1fr 1fr",
            "repeat(4, minmax(20px, auto))",
            "200px 200px 200px",
            "1fr 1fr 1fr 1fr 1fr 1fr",
            "repeat(0, 1fr)",
            "minmax(1fr, 2fr)",
            "-10px 1fr",
        ],
    )
}

fn grid_rows_list(random: &mut Lcg) -> &'static str {
    pick(
        random,
        &[
            "50px auto",
            "repeat(2, 1fr)",
            "1fr 2fr",
            "auto",
            "minmax(20px, auto) 1fr",
            "30% 70%",
            "100px",
            "min-content max-content",
            "repeat(auto-fill, 40px)",
            "auto 1fr auto",
            "40px 40px 40px",
            "fit-content(30px) 1fr",
            "minmax(0, 1fr)",
            "0.5fr 0.5fr",
            "none",
        ],
    )
}

fn grid_alignment(random: &mut Lcg) -> &'static str {
    pick(
        random,
        &[
            "normal",
            "start",
            "end",
            "center",
            "space-between",
            "space-around",
            "space-evenly",
            "stretch",
            "flex-start",
            "flex-end",
            "left",
            "right",
            "safe center",
        ],
    )
}

fn grid_self_alignment(random: &mut Lcg) -> &'static str {
    pick(
        random,
        &[
            "auto",
            "normal",
            "stretch",
            "start",
            "end",
            "center",
            "baseline",
            "self-start",
            "self-end",
            "flex-start",
            "flex-end",
            "left",
            "right",
            "safe end",
            "last baseline",
        ],
    )
}

/// A grid container's declaration: its tracks, the shorthands, gaps, the alignments and its own box.
fn grid_container_declaration(random: &mut Lcg) -> String {
    match random.below(18) {
        0..=3 => format!("grid-template-columns:{}", grid_track_list(random)),
        4 | 5 => format!("grid-template-rows:{}", grid_rows_list(random)),
        6 => format!("grid-auto-rows:{}", pick(random, &["40px", "minmax(30px, auto)", "auto", "1fr", "20px 40px", "min-content", "10%"])),
        7 => pick(
            random,
            &[
                "grid-template:50px auto / 1fr 1fr",
                "grid:auto-flow 30px / repeat(3, 1fr)",
                "grid:auto-flow / 1fr 2fr",
                "grid:auto-flow dense / 1fr 1fr",
                "grid-template:none",
                "grid-template:\"a b\" / 1fr 1fr",
                "grid-auto-flow:column",
                "grid-auto-columns:50px",
            ],
        )
        .to_string(),
        8 | 9 => format!(
            "{}:{}",
            pick(random, &["gap", "row-gap", "column-gap", "grid-gap", "grid-row-gap", "grid-column-gap"]),
            pick(random, &["10px", "5%", "1em 2em", "calc(5% + 2px)", "0", "4px 10%", "normal", "2px"])
        ),
        10 => format!("justify-content:{}", grid_alignment(random)),
        11 => format!("align-content:{}", grid_alignment(random)),
        12 => format!("justify-items:{}", grid_self_alignment(random)),
        13 => format!("align-items:{}", grid_self_alignment(random)),
        14 => format!(
            "{}:{}",
            pick(random, &["place-items", "place-content"]),
            pick(random, &["center", "end start", "stretch", "space-between center"])
        ),
        15 => format!("width:{}", pick(random, &["300px", "50%", "auto", "120px", "min-content", "max-content", "fit-content"])),
        16 => format!(
            "{}:{}",
            pick(random, &["height", "height", "min-height", "max-height"]),
            pick(random, &["200px", "auto", "50%", "60px", "0", "120px"])
        ),
        _ => format!("box-sizing:border-box;padding:{};border:2px solid", pick(random, &["3px", "10px", "5%"])),
    }
}

/// A grid item's declaration: its lines and spans (negative, reversed, past the explicit grid), the
/// self alignments, `order`, sizes with min and max, margins, positioning, display.
fn grid_item_declaration(random: &mut Lcg) -> String {
    match random.below(20) {
        0..=3 => format!(
            "grid-column:{}",
            pick(
                random,
                &[
                    "1",
                    "2",
                    "-1",
                    "1 / 3",
                    "span 2",
                    "2 / span 3",
                    "-2 / -1",
                    "span 5",
                    "3 / 1",
                    "5",
                    "1 / -1",
                    "auto / span 2",
                    "12",
                    "span 12",
                    "auto",
                    "4 / 2",
                    "span 2 / 4",
                    "0",
                    "a",
                ]
            )
        ),
        4 | 5 => format!(
            "grid-row:{}",
            pick(random, &["1", "2 / 4", "span 2", "-1", "3", "1 / -1", "span 3", "2", "4 / 1", "auto", "span 2 / 3", "-3"])
        ),
        6 => format!(
            "{}:{}",
            pick(random, &["grid-column-start", "grid-column-end", "grid-row-start", "grid-row-end"]),
            pick(random, &["1", "2", "-1", "span 2", "auto", "3"])
        ),
        7 => format!("justify-self:{}", grid_self_alignment(random)),
        8 => format!("align-self:{}", grid_self_alignment(random)),
        9 => format!("place-self:{}", pick(random, &["center", "end start", "stretch", "start end"])),
        10 => format!("order:{}", pick(random, &["-1", "1", "2", "0"])),
        11 | 12 => format!(
            "width:{}",
            pick(random, &["50px", "50%", "calc(50% - 5px)", "auto", "min-content", "max-content", "fit-content", "200px", "100%"])
        ),
        13 => format!("height:{}", pick(random, &["30px", "50%", "auto", "100px"])),
        14 => format!(
            "{}:{}",
            pick(random, &["min-width", "max-width", "min-height", "max-height"]),
            pick(random, &["0", "100px", "50%", "none", "20px", "calc(10% + 5px)"])
        ),
        15 => format!("margin:{}", pick(random, &["auto", "5px", "10%", "0 auto", "-4px"])),
        16 => format!("padding:{};border:{} solid", pick(random, &["0", "4px", "5%"]), pick(random, &["0", "1px", "3px"])),
        17 => pick(
            random,
            &[
                "position:absolute;left:5px;top:5px",
                "position:absolute;right:0;bottom:0;width:30%",
                "position:relative;left:3px",
                "position:fixed;top:0",
            ],
        )
        .to_string(),
        18 => format!("display:{}", pick(random, &["none", "flex", "inline-block", "grid", "inline", "list-item"])),
        _ => format!("box-sizing:{}", pick(random, &["border-box", "content-box"])),
    }
}

/// A generated document of nested grids (and some flex boxes between), with items of every kind: blocks, runs of
/// text, inlines, images, controls, nested containers.
fn grid_document(random: &mut Lcg) -> Vec<u8> {
    fn content(random: &mut Lcg, depth: u32, out: &mut String) {
        match random.below(12) {
            0 | 1 => words(random, out),
            2 => out.push_str(&format!(
                "<img src=\"{}.png\"{}>",
                pick(random, &["a", "bb", "photo", "x9"]),
                pick(random, &["", " style=\"width:50%\"", " style=\"width:40px\"", " width=\"30\""])
            )),
            3 => out.push_str(pick(
                random,
                &["<span>s <b>t</b></span>", "<b>bold</b> ", "<br>", "<span style=\"display:inline-block;width:30px\">ib</span>"],
            )),
            4 => out.push_str(pick(
                random,
                &["<input type=\"text\" value=\"v\">", "<select><option>o</option></select>", "<button>b</button>"],
            )),
            5 if depth < 3 => container(random, depth + 1, out),
            6 if depth < 2 => {
                out.push_str(&format!(
                    "<div style=\"display:flex;{}\">",
                    pick(random, &["flex-wrap:wrap", "flex-direction:column", "gap:4px", "justify-content:center"])
                ));
                for _ in 0..1 + random.below(3) {
                    if random.chance(2) {
                        container(random, depth + 1, out);
                    } else {
                        item(random, depth + 1, out);
                    }
                }
                out.push_str("</div>");
            }
            _ => item(random, depth, out),
        }
    }
    fn item(random: &mut Lcg, depth: u32, out: &mut String) {
        let tag = pick(random, &["div", "div", "p", "span", "section", "li", "a"]);
        out.push_str(&format!("<{tag}"));
        if random.chance(3) {
            out.push_str(&format!(" class=\"{}\"", pick(random, &["i", "j", "i j"])));
        }
        if !random.chance(4) {
            let decls: Vec<String> = (0..1 + random.below(4)).map(|_| grid_item_declaration(random)).collect();
            out.push_str(&format!(" style=\"{}\"", decls.join(";")));
        }
        out.push('>');
        for _ in 0..random.below(3) {
            if depth < 3 && random.chance(6) {
                container(random, depth + 1, out);
            } else {
                words(random, out);
            }
        }
        out.push_str(&format!("</{tag}>"));
    }
    fn container(random: &mut Lcg, depth: u32, out: &mut String) {
        let display = if random.chance(5) { "inline-grid" } else { "grid" };
        let mut decls: Vec<String> = (0..1 + random.below(5)).map(|_| grid_container_declaration(random)).collect();
        decls.insert(0, format!("display:{display}"));
        if random.chance(3) {
            decls.push(grid_item_declaration(random));
        }
        if random.chance(10) {
            decls.push("direction:rtl".to_string());
        }
        let class = if random.chance(3) { " class=\"g\"" } else { "" };
        out.push_str(&format!("<div{class} style=\"{}\">", decls.join(";")));
        let n = if depth > 1 { random.below(4) } else { random.below(8) };
        for _ in 0..n {
            content(random, depth, out);
        }
        out.push_str("</div>");
    }
    let mut out = String::new();
    if !random.chance(8) {
        out.push_str("<!DOCTYPE html>");
    }
    out.push_str("<html><head><style>");
    for _ in 0..random.below(4) {
        let selector = pick(random, &[".g", ".i", ".j", ".g > div", "div", "p", ".g > :first-child", ".g > :nth-child(3)"]);
        let decls: Vec<String> = (0..1 + random.below(3))
            .map(|_| if random.chance(2) { grid_container_declaration(random) } else { grid_item_declaration(random) })
            .collect();
        let rule = format!("{selector}{{{}}}", decls.join(";"));
        if random.chance(3) {
            out.push_str(&format!("@media (min-width:{}px){{{rule}}}", pick(random, &["300", "450", "700"])));
        } else {
            out.push_str(&rule);
        }
    }
    out.push_str("</style></head><body>");
    for _ in 0..1 + random.below(3) {
        if random.chance(5) {
            content(random, 1, &mut out);
        } else {
            container(random, 0, &mut out);
        }
    }
    out.push_str("</body></html>");
    out.into_bytes()
}

/// Grid shapes for each branch, written out.
fn written_grid_documents() -> Vec<(&'static str, String)> {
    let page = |body: &str| format!("<!DOCTYPE html><html><head></head><body>{body}</body></html>");
    let six = "<div>one</div><div>two words</div><div>three more words here</div><div>4</div><div>five five</div><div>six</div>";
    let grid = |style: &str, body: &str| page(&format!("<div style=\"display:grid;{style}\">{body}</div>"));
    vec![
        ("explicit columns", grid("grid-template-columns:100px 1fr 2fr", six)),
        ("explicit rows and columns", grid("grid-template-columns:1fr 1fr;grid-template-rows:40px 60px 20px", six)),
        ("implicit rows", grid("grid-template-columns:repeat(4, 1fr)", &format!("{six}{six}"))),
        ("implicit rows from grid-auto-rows", grid("grid-template-columns:1fr 1fr;grid-auto-rows:30px 50px", &format!("{six}{six}"))),
        ("no template at all", grid("", six)),
        ("an empty grid", grid("grid-template-columns:1fr 1fr;height:50px;padding:4px", "")),
        ("an empty grid with only spaces", grid("grid-template-columns:1fr 1fr", "  \n  ")),
        ("text straight in the grid", grid("grid-template-columns:1fr 1fr", "loose text <b>bold</b> more <div>block</div> tail")),
        (
            "auto-placement skipping taken cells",
            grid(
                "grid-template-columns:repeat(3, 1fr)",
                "<div style=\"grid-column:2;grid-row:1\">a</div><div>b</div><div>c</div><div style=\"grid-row:2;grid-column:1 / 3\">d</div><div>e</div><div>f</div>",
            ),
        ),
        (
            "auto-placement after a placed column behind the cursor",
            grid(
                "grid-template-columns:repeat(3, 60px)",
                "<div>a</div><div>b</div><div style=\"grid-column:1\">c</div><div style=\"grid-column:3\">d</div><div style=\"grid-column:2\">e</div>",
            ),
        ),
        (
            "rows given, columns auto",
            grid(
                "grid-template-columns:repeat(3, 1fr)",
                "<div style=\"grid-row:2\">a</div><div style=\"grid-row:2\">b</div><div style=\"grid-row:2 / span 2\">c</div><div style=\"grid-row:2\">d</div><div>e</div>",
            ),
        ),
        (
            "spans past the explicit grid",
            grid(
                "grid-template-columns:50px 50px",
                "<div style=\"grid-column:span 4\">wide</div><div style=\"grid-column:3 / 6\">past</div><div style=\"grid-row:span 3\">tall</div><div>x</div>",
            ),
        ),
        (
            "negative and reversed lines",
            grid(
                "grid-template-columns:1fr 1fr 1fr;grid-template-rows:30px 30px",
                "<div style=\"grid-column:-2 / -1\">a</div><div style=\"grid-column:3 / 1\">b</div><div style=\"grid-row:-1;grid-column:-4\">c</div><div style=\"grid-column-end:3\">d</div><div style=\"grid-column-end:span 2;grid-column-start:2\">e</div>",
            ),
        ),
        (
            "line numbers at the caps",
            grid(
                "grid-template-columns:1fr",
                "<div style=\"grid-column:1000\">a</div><div style=\"grid-row:span 1000\">b</div><div style=\"grid-column:-1000\">c</div>",
            ),
        ),
        (
            "the placement budget spent",
            grid(
                "grid-template-columns:repeat(1000, 1px)",
                "<div style=\"grid-column:span 1000;grid-row:span 300\">a</div><div>b</div><div style=\"grid-row:4\">c</div><div style=\"grid-column:3\">d</div><div>e</div>",
            ),
        ),
        (
            "fr with min-content items",
            grid(
                "grid-template-columns:1fr 1fr 1fr;width:150px",
                "<div>supercalifragilisticexpialidocious</div><div>a</div><div>b c d e f g h</div>",
            ),
        ),
        (
            "fr frozen at their base",
            grid(
                "grid-template-columns:1fr 3fr;width:200px",
                "<div><div style=\"width:150px\">wide</div></div><div>b</div>",
            ),
        ),
        (
            "fractions under one",
            grid("grid-template-columns:0.2fr 0.3fr 50px", "<div>a</div><div>b</div><div>c</div>"),
        ),
        (
            "minmax",
            grid(
                "grid-template-columns:minmax(50px, 200px) minmax(min-content, 1fr) minmax(0, 1fr) minmax(max-content, auto)",
                "<div>a b</div><div>longer words here</div><div>xxxxxxxxxxxxxxx</div><div>tail end</div>",
            ),
        ),
        (
            "fit-content and the content keywords",
            grid(
                "grid-template-columns:fit-content(80px) min-content max-content auto",
                "<div>fit content words</div><div>min content words</div><div>max content words</div><div>auto</div>",
            ),
        ),
        ("repeat with a count", grid("grid-template-columns:repeat(2, 40px 1fr)", six)),
        ("auto-fill", grid("grid-template-columns:repeat(auto-fill, minmax(80px, 1fr))", six)),
        ("auto-fill of fixed tracks", grid("grid-template-columns:repeat(auto-fill, 100px);gap:7px", six)),
        ("auto-fit collapsing empty tracks", grid("grid-template-columns:repeat(auto-fit, minmax(60px, 1fr))", "<div>a</div><div>b</div>")),
        (
            "auto-fit between fixed tracks",
            grid("grid-template-columns:10px repeat(auto-fit, 30px) 20px;column-gap:3px", "<div>a</div><div>b</div><div>c</div>"),
        ),
        ("auto-fill with % tracks", grid("grid-template-columns:repeat(auto-fill, minmax(10%, 1fr))", six)),
        ("auto-fill with no definite size", grid("grid-template-columns:repeat(auto-fill, auto)", six)),
        ("gaps", grid("grid-template-columns:1fr 1fr;gap:10px 20px", six)),
        ("% gaps without a height", grid("grid-template-columns:1fr 1fr;gap:5%", six)),
        ("% gaps with a height", grid("grid-template-columns:1fr 1fr;gap:5% 10%;height:300px", six)),
        ("calc gaps", grid("grid-template-columns:1fr 1fr;gap:calc(5% + 2px)", six)),
        ("legacy gap names", grid("grid-template-columns:1fr 1fr;grid-row-gap:6px;grid-column-gap:9px", six)),
        (
            "justify-content between columns",
            page(&[
                "start", "end", "center", "space-between", "space-around", "space-evenly", "stretch", "flex-end", "right", "normal",
            ]
            .iter()
            .map(|j| format!("<div style=\"display:grid;grid-template-columns:40px 40px 40px;justify-content:{j}\">{six}</div>"))
            .collect::<String>()),
        ),
        (
            "justify-content with auto columns",
            page(&["start", "stretch", "flex-start", "center"]
                .iter()
                .map(|j| format!("<div style=\"display:grid;grid-template-columns:auto auto;justify-content:{j}\">{six}</div>"))
                .collect::<String>()),
        ),
        (
            "align-content between rows",
            page(&["start", "end", "flex-end", "center", "space-between", "space-around", "space-evenly", "stretch", "normal"]
                .iter()
                .map(|a| {
                    format!(
                        "<div style=\"display:grid;grid-template-columns:1fr 1fr;grid-template-rows:30px auto;height:200px;align-content:{a}\">{six}</div>"
                    )
                })
                .collect::<String>()),
        ),
        (
            "fr rows in a definite height",
            grid("grid-template-columns:1fr 1fr;grid-template-rows:1fr 2fr auto;height:240px;row-gap:5px", six),
        ),
        (
            "fr rows frozen at their content",
            grid(
                "grid-template-rows:1fr 5fr;height:100px",
                "<div style=\"height:80px\">tall</div><div>short</div>",
            ),
        ),
        ("fr rows without a height", grid("grid-template-rows:1fr 2fr", six)),
        ("% rows", grid("grid-template-rows:30% 70%;height:150px", "<div>a</div><div>b</div>")),
        (
            "rows spanned by a tall item",
            grid(
                "grid-template-columns:1fr 1fr;grid-template-rows:20px auto 20px",
                "<div style=\"grid-row:1 / 4;height:150px\">tall</div><div>a</div><div>b</div><div>c</div>",
            ),
        ),
        (
            "rows spanned with no content-sized row",
            grid(
                "grid-template-columns:1fr 1fr;grid-template-rows:20px 20px",
                "<div style=\"grid-row:span 2;height:90px\">tall</div><div>a</div>",
            ),
        ),
        (
            "columns spanned by a wide item",
            grid(
                "grid-template-columns:auto 50px auto",
                "<div style=\"grid-column:1 / 4\">a very wide item with many words in it</div><div>a</div><div>b</div><div>c</div>",
            ),
        ),
        (
            "columns spanned across a flexible track",
            grid(
                "grid-template-columns:auto 1fr;width:100px",
                "<div style=\"grid-column:span 2\">aaaaaaaaaaaaaaaaaaaa</div><div style=\"grid-column:span 2;min-width:150px\">b</div><div>c</div>",
            ),
        ),
        (
            "justify-items and justify-self",
            grid(
                "grid-template-columns:100px 100px 100px;justify-items:center",
                "<div>a</div><div style=\"justify-self:end\">b</div><div style=\"justify-self:start\">c</div><div style=\"justify-self:stretch\">d</div><div style=\"justify-self:self-end\">e</div><div style=\"justify-self:flex-end;width:20px\">f</div>",
            ),
        ),
        (
            "align-items and align-self",
            grid(
                "grid-template-columns:repeat(3, 1fr);grid-auto-rows:80px;align-items:center",
                "<div>a</div><div style=\"align-self:end\">b</div><div style=\"align-self:stretch\">c</div><div style=\"align-self:baseline\">d</div><div style=\"align-self:start\">e</div><div style=\"align-self:stretch;height:20px\">f</div>",
            ),
        ),
        (
            "place-items and place-self",
            grid(
                "grid-template-columns:1fr 1fr;grid-auto-rows:60px;place-items:end center",
                "<div>a</div><div style=\"place-self:start end\">b</div><div>c</div>",
            ),
        ),
        (
            "items with widths",
            grid(
                "grid-template-columns:1fr 1fr 1fr",
                "<div style=\"width:50px\">a</div><div style=\"width:50%\">b</div><div style=\"width:calc(50% - 5px)\">c</div><div style=\"width:min-content\">d e f</div><div style=\"width:max-content\">g h</div><div style=\"width:fit-content\">i j</div>",
            ),
        ),
        (
            "items with min and max widths",
            grid(
                "grid-template-columns:auto auto;width:120px",
                "<div style=\"min-width:100px\">a</div><div style=\"min-width:50%;box-sizing:border-box;padding:5px\">b</div><div style=\"max-width:20px\">long words</div><div style=\"min-width:0\">xxxxxxxxxxxxxxxxxxx</div>",
            ),
        ),
        (
            "images with % widths",
            grid(
                "grid-template-columns:1fr 1fr",
                "<img src=\"a.png\" style=\"width:50%\"><img src=\"b.png\"><img src=\"c.png\" style=\"width:30px\"><div>t</div>",
            ),
        ),
        (
            "margins and boxes",
            grid(
                "grid-template-columns:1fr 1fr;box-sizing:border-box;padding:10px;border:3px solid",
                "<div style=\"margin:5px;padding:4px;border:2px solid\">a</div><div style=\"margin:auto\">b</div><div style=\"margin:10%\">c</div><div style=\"box-sizing:border-box;padding:6px;width:80px\">d</div>",
            ),
        ),
        (
            "order",
            grid(
                "grid-template-columns:1fr 1fr 1fr",
                "<div style=\"order:2\">a</div><div style=\"order:-1\">b</div><div>c</div><div style=\"order:1\">d</div><div style=\"order:-1\">e</div>",
            ),
        ),
        (
            "nested grids",
            grid(
                "grid-template-columns:1fr 2fr",
                "<div style=\"display:grid;grid-template-columns:auto auto;gap:3px\"><div>a</div><div>b</div><div>c</div></div><div style=\"display:grid;grid-template-columns:repeat(auto-fill, 30px)\"><div>d</div><div>e</div><div>f</div><div>g</div></div>",
            ),
        ),
        (
            "grid in flex",
            page(
                "<div style=\"display:flex\"><div style=\"display:grid;grid-template-columns:1fr 1fr;flex:1\"><div>a</div><div>b</div></div><div style=\"display:grid;grid-template-columns:auto\">c</div></div>",
            ),
        ),
        (
            "flex in grid",
            grid(
                "grid-template-columns:1fr 1fr",
                "<div style=\"display:flex;flex-wrap:wrap\"><div>a</div><div>b</div><div>c</div></div><div style=\"display:flex;flex-direction:column\"><div>d</div><div>e</div></div>",
            ),
        ),
        (
            "absolute children of a grid",
            grid(
                "grid-template-columns:1fr 1fr;position:relative;height:100px",
                "<div>a</div><div style=\"position:absolute;left:5px;top:5px\">abs</div><div>b</div><div style=\"position:absolute;right:0;bottom:0;width:30%\">abs2</div><div style=\"position:fixed;top:0\">fixed</div><div style=\"position:absolute;display:none\">none</div>",
            ),
        ),
        (
            "relative children of a grid",
            grid("grid-template-columns:1fr 1fr", "<div style=\"position:relative;left:3px;top:4px\">a</div><div>b</div>"),
        ),
        (
            "right to left",
            grid("grid-template-columns:50px 1fr 2fr;direction:rtl", six),
        ),
        (
            "inline grids",
            page(
                "text <div style=\"display:inline-grid;grid-template-columns:auto auto\"><div>a</div><div>b</div></div> more <span style=\"display:inline-grid\">c</span> end",
            ),
        ),
        (
            "a grid of a fixed width and height",
            grid("grid-template-columns:1fr auto;width:200px;height:150px", six),
        ),
        (
            "a grid of min-content and max-content widths",
            page(&format!(
                "<div style=\"display:grid;grid-template-columns:1fr 1fr;width:min-content\">{six}</div><div style=\"display:grid;grid-template-columns:1fr 1fr;width:max-content\">{six}</div><div style=\"display:grid;grid-template-columns:100px 1fr;width:fit-content\">{six}</div>"
            )),
        ),
        (
            "a floated and an absolute grid",
            page(&format!(
                "<div style=\"display:grid;grid-template-columns:1fr 1fr;float:left\">{six}</div><div style=\"display:grid;position:absolute;grid-template-columns:auto auto\">{six}</div>"
            )),
        ),
        (
            "a grid with min and max heights",
            page(&format!(
                "<div style=\"display:grid;grid-template-rows:1fr 1fr;min-height:200px\">{six}</div><div style=\"display:grid;max-height:30px\">{six}</div>"
            )),
        ),
        (
            "items with % heights",
            page(
                "<div style=\"display:grid;height:200px\"><div style=\"height:50%\">a</div><div>b</div></div><div style=\"display:grid\"><div style=\"height:50%\">c</div></div>",
            ),
        ),
        (
            "display none and contents among items",
            grid("grid-template-columns:1fr 1fr", "<div>a</div><div style=\"display:none\">b</div><div>c</div><span>d</span>"),
        ),
        ("the grid shorthand", grid("grid:auto-flow 30px / repeat(3, 1fr)", six)),
        ("grid-template with rows and columns", grid("grid-template:50px auto / 1fr 1fr", six)),
        ("refused shorthands", grid("grid:auto-flow dense / 1fr 1fr;grid-template:\"a b\" / 1fr 1fr;grid-auto-flow:column", six)),
        ("named lines skipped", grid("grid-template-columns:[a] 50px [b] 1fr [c]", "<div style=\"grid-column:a\">a</div><div>b</div>")),
        ("a negative length refused", grid("grid-template-columns:-10px 1fr", six)),
        (
            "controls in a grid",
            grid(
                "grid-template-columns:auto 1fr",
                "<label>name</label><input type=\"text\" value=\"v\"><label>pick</label><select><option>one</option></select><button>go</button><input type=\"range\">",
            ),
        ),
        (
            "a grid laid out at a media width",
            page(
                "<style>.g{display:grid;grid-template-columns:1fr}@media (min-width:600px){.g{grid-template-columns:1fr 1fr 1fr;gap:12px}}</style><div class=\"g\"><div>a</div><div>b</div><div>c</div></div>",
            ),
        ),
        (
            "a grid that changes on hover",
            page(
                "<style>.g:hover{grid-template-columns:1fr 3fr;justify-items:end}</style><div class=\"g\" style=\"display:grid;grid-template-columns:1fr 1fr\"><div>a</div><div>b</div><div>c</div></div>",
            ),
        ),
        (
            "a list as a grid",
            page(
                "<ul style=\"display:grid;grid-template-columns:repeat(2, 1fr);gap:4px\"><li>one</li><li>two</li><li>three</li></ul><ol style=\"display:grid\"><li>a</li><li>b</li></ol>",
            ),
        ),
        (
            "a scrolling grid",
            grid("grid-template-columns:200px 200px 200px;overflow:auto;width:150px;height:60px", six),
        ),
    ]
}

// ---- The comparison ----------------------------------------------------------------------------------------

#[test]
fn the_corpus_lays_out_alike() {
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
            let mut steps = layout_steps(width, if dark_theme { 600.0 } else { 320.0 });
            if k % 3 == 0 {
                let t = k as u32 * 7;
                steps.extend([
                    Step::Media(other.0, other.1),
                    Step::Layout(other.0, None),
                    Step::Dump,
                    Step::Hover(t),
                    Step::Layout(other.0, None),
                    Step::Dump,
                ]);
            }
            let theme = if dark_theme { "dark" } else { "light" };
            run.case(layout_record(width, height, 2.0, master, b"", &html, &steps), format!("{name} ({theme})"));
        }
    }
    let tally = run.finish();
    assert!(tally.grid_documents > 60 && tally.tables > 0);
}

/// The corpus again at narrower and wider viewports (flex lines wrap and stretch differently there): light at 150 and
/// 768 px, dark at 1440 and 240 px, each with a media change between.
#[test]
fn the_corpus_lays_out_alike_at_more_widths() {
    let mut run = Run::new("corpus at more widths");
    let pages = corpus_pages();
    let light = themed_master(false);
    let dark = themed_master(true);
    for (name, text) in pages.iter() {
        let snap = snapshot(text);
        for (dark_theme, (master, vars)) in [(false, &light), (true, &dark)] {
            let html = themed_html(&snap, dark_theme, vars);
            let (width, other) = if dark_theme { (1440.0, 240.0) } else { (150.0, 768.0) };
            let steps = vec![
                Step::Layout(width, None),
                Step::Dump,
                Step::Media(other, 900.0),
                Step::Layout(other, None),
                Step::Dump,
                Step::Layout(width, None),
                Step::Dump,
            ];
            let theme = if dark_theme { "dark" } else { "light" };
            run.case(layout_record(width, 900.0, 2.0, master, b"", &html, &steps), format!("{name} ({theme}, {width} and {other} px)"));
        }
    }
    let tally = run.finish();
    assert!(tally.grid_documents > 60 && tally.tables > 0);
}

/// Every tag, the table ones too, under the master sheet and the UA layers.
#[test]
fn every_tag_lays_out_alike() {
    let mut run = Run::new("every tag");
    let master = master_css();
    let (light, _) = themed_master(false);
    let (dark, vars) = themed_master(true);
    let steps = layout_steps(375.0, 1200.0);
    for quirks in [false, true] {
        let html = String::from_utf8(every_tag_document(quirks)).expect("UTF-8");
        // A range input's thumb is absolutely positioned.
        assert!(html.contains("type=\"range\""));
        let html = html.into_bytes();
        let what = if quirks { "every tag in quirks mode" } else { "every tag" };
        run.case(layout_record(375.0, 667.0, 1.0, &master, b"", &html, &steps), format!("{what}, master CSS"));
        run.case(layout_record(375.0, 667.0, 2.0, &light, b"", &html, &steps), format!("{what}, light UA layers"));
        run.case(layout_record(412.0, 915.0, 3.0, &dark, b"", &themed_html(&html, true, &vars), &steps), format!("{what}, dark UA layers"));
        run.case(layout_record(375.0, 667.0, 2.0, b"", b"", &html, &steps), format!("{what}, no sheet"));
    }
    run.finish();
}

#[test]
fn written_shapes_lay_out_alike() {
    let mut run = Run::new("written shapes");
    let master = master_css();
    let (light, _) = themed_master(false);
    for (what, html) in written_documents() {
        for sheet in [&master, &light] {
            for (width, other) in [(375.0, 200.0), (120.0, 800.0)] {
                let mut steps = layout_steps(width, other);
                steps.extend([
                    Step::Hover(2),
                    Step::Layout(width, None),
                    Step::Dump,
                    Step::Media(900.0, 700.0),
                    Step::Layout(900.0, None),
                    Step::Dump,
                ]);
                run.case(layout_record(375.0, 667.0, 2.0, sheet, b"", html.as_bytes(), &steps), what);
            }
        }
    }
    let tally = run.finish();
    for what in [
        "floats left",
        "floats right",
        "boxes clearing",
        "ellipses",
        "texts cut into pieces",
        "right-to-left lines",
        "white-space pre",
        "scroll views",
    ] {
        assert!(tally.counts.contains_key(what), "no {what}");
    }
}

#[test]
fn written_flex_shapes_lay_out_alike() {
    let mut run = Run::new("written flex shapes");
    let master = master_css();
    let (light, _) = themed_master(false);
    for (what, html) in written_flex_documents() {
        for sheet in [&master, &light] {
            for (width, other) in [(375.0, 200.0), (120.0, 800.0)] {
                let mut steps = layout_steps(width, other);
                steps.extend([
                    Step::Hover(2),
                    Step::Layout(width, None),
                    Step::Dump,
                    Step::Media(900.0, 700.0),
                    Step::Layout(900.0, None),
                    Step::Dump,
                ]);
                run.case(layout_record(375.0, 667.0, 2.0, sheet, b"", html.as_bytes(), &steps), what);
            }
        }
    }
    let tally = run.finish();
    for what in [
        "flex-direction row",
        "flex-direction row-reverse",
        "flex-direction column",
        "flex-direction column-reverse",
        "flex-wrap wrap",
        "flex-wrap wrap-reverse",
        "flex containers of several lines",
        "flex items grown",
        "flex items shrunk",
        "flex items clamped by their minimum",
        "flex items clamped by their maximum",
        "flex items out of source order",
        "flex lines with first baselines",
        "flex lines with last baselines",
        "flex items with main auto margins given room",
        "flex items with cross auto margins",
        "flex main gaps",
        "inline flex containers",
        "fixed flex boxes looked up for their resolved input",
    ] {
        assert!(tally.counts.contains_key(what), "no {what}");
    }
}

#[test]
fn generated_flex_documents_lay_out_alike() {
    let mut run = Run::new("generated flex documents");
    let mut random = Lcg::new(0x1A_0001);
    let master = master_css();
    let (light, _) = themed_master(false);
    let only: Option<usize> = std::env::var("LAYOUT_ONLY").ok().and_then(|v| v.parse().ok());
    for k in 0..2500 {
        let html = flex_document(&mut random);
        if only.is_some_and(|only| only != k) {
            continue;
        }
        let width = [320.0, 375.0, 150.0, 600.0][k % 4];
        let other = [700.0, 240.0, 500.0, 90.0][(k / 4) % 4];
        let sheet = if k % 2 == 0 { &master } else { &light };
        let mut steps = layout_steps(width, other);
        steps.extend([
            Step::Media(other, 640.0),
            Step::Layout(other, None),
            Step::Dump,
            Step::Hover(k as u32),
            Step::Layout(other, None),
            Step::Dump,
        ]);
        run.case(
            layout_record(width, 640.0, 2.0, sheet, b"", &html, &steps),
            format!("generated flex {k}: {}", String::from_utf8_lossy(&html)),
        );
    }
    run.finish();
}

#[test]
fn generated_documents_lay_out_alike() {
    let mut run = Run::new("generated documents");
    let mut random = Lcg::new(0x17_0001);
    let master = master_css();
    let (light, _) = themed_master(false);
    let only: Option<usize> = std::env::var("LAYOUT_ONLY").ok().and_then(|v| v.parse().ok());
    for k in 0..3000 {
        let html = layout_document(&mut random);
        if only.is_some_and(|only| only != k) {
            continue;
        }
        let width = [320.0, 375.0, 150.0, 600.0][k % 4];
        let other = [700.0, 240.0, 500.0, 90.0][(k / 4) % 4];
        let sheet = if k % 2 == 0 { &master } else { &light };
        let mut steps = layout_steps(width, other);
        steps.extend([
            Step::Media(other, 640.0),
            Step::Layout(other, None),
            Step::Dump,
            Step::Hover(k as u32),
            Step::Layout(other, None),
            Step::Dump,
        ]);
        run.case(layout_record(width, 640.0, 2.0, sheet, b"", &html, &steps), format!("generated {k}: {}", String::from_utf8_lossy(&html)));
    }
    run.finish();
}

#[test]
fn written_positioned_shapes_lay_out_alike() {
    let mut run = Run::new("written positioned shapes");
    let master = master_css();
    let (light, _) = themed_master(false);
    for (what, html) in written_positioned_documents() {
        for sheet in [&master, &light] {
            for (width, other) in [(375.0, 200.0), (120.0, 800.0)] {
                let mut steps = layout_steps(width, other);
                steps.extend([
                    Step::Hover(2),
                    Step::Layout(width, None),
                    Step::Dump,
                    Step::Media(900.0, 700.0),
                    Step::Layout(900.0, None),
                    Step::Dump,
                    Step::LayoutAs(900.0, 2),
                    Step::Dump,
                    Step::Media(500.0, 300.0),
                    Step::LayoutAs(900.0, 2),
                    Step::Dump,
                    Step::LayoutAs(500.0, 1),
                    Step::Dump,
                ]);
                run.case(layout_record(375.0, 667.0, 2.0, sheet, b"", html.as_bytes(), &steps), what);
            }
        }
    }
    for (what, user, html) in written_sticky_documents() {
        for sheet in [&master, &light] {
            for (width, height) in [(375.0, 667.0), (120.0, 90.0)] {
                let mut steps = layout_steps(width, 300.0);
                steps.extend([
                    Step::Hover(2),
                    Step::Layout(width, None),
                    Step::Dump,
                    Step::Media(200.0, 50.0),
                    Step::Layout(200.0, None),
                    Step::Dump,
                ]);
                run.case(layout_record(width, height, 2.0, sheet, user.as_bytes(), html.as_bytes(), &steps), what);
            }
        }
    }
    let tally = run.finish();
    for what in [
        "sticky boxes",
        "absolute boxes",
        "fixed boxes",
        "vertical rule 1 (top and height auto)",
        "vertical rule 2 (top and bottom auto)",
        "vertical rule 3 (height and bottom auto)",
        "vertical rule 4 (top auto)",
        "vertical rule 5 (height auto)",
        "vertical rule 6 (bottom auto)",
        "vertical rule 7 (all auto)",
        "vertical rule 8 (none auto)",
        "horizontal rule 1 (left and width auto)",
        "horizontal rule 2 (left and right auto)",
        "horizontal rule 3 (width and right auto)",
        "horizontal rule 4 (left auto)",
        "horizontal rule 5 (width auto)",
        "horizontal rule 6 (right auto)",
        "horizontal rule 7 (all auto)",
        "horizontal rule 8 (none auto)",
        "absolute replaced boxes",
        "absolute flex boxes",
        "fixed boxes' redraw boxes",
        "positioned lists out of tree order",
        "positioned held by static boxes",
        "sticky boxes moved",
        "boxes placed against the viewport",
    ] {
        assert!(tally.counts.contains_key(what), "no {what}");
    }
}

#[test]
fn generated_positioned_documents_lay_out_alike() {
    let mut run = Run::new("generated positioned documents");
    let mut random = Lcg::new(0x19_0001);
    let master = master_css();
    let (light, _) = themed_master(false);
    let only: Option<usize> = std::env::var("LAYOUT_ONLY").ok().and_then(|v| v.parse().ok());
    for k in 0..2500 {
        let html = positioned_document(&mut random);
        let user = if k % 3 == 0 { sticky_user_sheet(&mut random) } else { String::new() };
        if only.is_some_and(|only| only != k) {
            continue;
        }
        let width = [320.0, 375.0, 150.0, 600.0][k % 4];
        let other = [700.0, 240.0, 500.0, 90.0][(k / 4) % 4];
        let sheet = if k % 2 == 0 { &master } else { &light };
        let mut steps = layout_steps(width, other);
        steps.extend([
            Step::Media(other, 640.0),
            Step::Layout(other, None),
            Step::Dump,
            Step::Hover(k as u32),
            Step::Layout(other, None),
            Step::Dump,
        ]);
        if k % 5 == 0 {
            steps.extend([Step::Media(other, 300.0), Step::LayoutAs(other, 2), Step::Dump, Step::LayoutAs(width, 1), Step::Dump]);
        }
        run.case(
            layout_record(width, 640.0, 2.0, sheet, user.as_bytes(), &html, &steps),
            format!("generated positioned {k}: {} with the user sheet {user}", String::from_utf8_lossy(&html)),
        );
    }
    run.finish();
}

#[test]
fn written_table_shapes_lay_out_alike() {
    let mut run = Run::new("written table shapes");
    let master = master_css();
    let (light, _) = themed_master(false);
    for (what, html) in written_table_documents() {
        for sheet in [&master, &light] {
            for (width, other) in [(375.0, 200.0), (120.0, 800.0)] {
                let mut steps = layout_steps(width, other);
                steps.extend([
                    Step::Hover(5),
                    Step::Layout(width, None),
                    Step::Dump,
                    Step::Media(900.0, 700.0),
                    Step::Layout(900.0, None),
                    Step::Dump,
                ]);
                run.case(layout_record(375.0, 667.0, 2.0, sheet, b"", html.as_bytes(), &steps), what);
            }
        }
    }
    let tally = run.finish();
    for what in [
        "tables with collapsed borders",
        "tables with border spacing",
        "cells leaving a collapsed side",
        "fixed tables",
        "inline tables",
        "tables of one column",
        "tables with no cell",
        "captions on top",
        "captions below",
        "tables moved down by their captions",
        "cells spanning columns",
        "cells spanning rows",
        "row groups taken as cells",
        "columns of a % width",
        "columns of a length width",
        "columns wider than their maximum",
        "columns narrower than their maximum",
        "rows of a CSS height",
        "rows of a % height",
        "rows given extra height",
        "cells of a CSS height",
        "places no cell takes",
        "layouts exhausted",
    ] {
        assert!(tally.counts.contains_key(what), "no {what}");
    }
}

#[test]
fn generated_table_documents_lay_out_alike() {
    let mut run = Run::new("generated table documents");
    let mut random = Lcg::new(0x1C_0001);
    let master = master_css();
    let (light, _) = themed_master(false);
    let only: Option<usize> = std::env::var("LAYOUT_ONLY").ok().and_then(|v| v.parse().ok());
    for k in 0..2000 {
        let html = table_document(&mut random);
        if only.is_some_and(|only| only != k) {
            continue;
        }
        let width = [320.0, 375.0, 150.0, 600.0][k % 4];
        let other = [700.0, 240.0, 500.0, 90.0][(k / 4) % 4];
        let sheet = if k % 2 == 0 { &master } else { &light };
        let mut steps = layout_steps(width, other);
        steps.extend([
            Step::Media(other, 640.0),
            Step::Layout(other, None),
            Step::Dump,
            Step::Hover(k as u32),
            Step::Layout(other, None),
            Step::Dump,
        ]);
        run.case(
            layout_record(width, 640.0, 2.0, sheet, b"", &html, &steps),
            format!("generated table {k}: {}", String::from_utf8_lossy(&html)),
        );
    }
    run.finish();
}

#[test]
fn written_grid_shapes_lay_out_alike() {
    let mut run = Run::new("written grid shapes");
    let master = master_css();
    let (light, _) = themed_master(false);
    for (what, html) in written_grid_documents() {
        for sheet in [&master, &light] {
            for (width, other) in [(375.0, 200.0), (120.0, 800.0)] {
                let mut steps = layout_steps(width, other);
                steps.extend([
                    Step::Hover(2),
                    Step::Layout(width, None),
                    Step::Dump,
                    Step::Media(900.0, 700.0),
                    Step::Layout(900.0, None),
                    Step::Dump,
                ]);
                run.case(layout_record(375.0, 667.0, 2.0, sheet, b"", html.as_bytes(), &steps), what);
            }
        }
    }
    let tally = run.finish();
    for what in [
        "empty grids",
        "inline grids",
        "grids of a definite height",
        "grid % gaps",
        "auto-fill repetitions",
        "auto-fit repetitions",
        "collapsed columns",
        "implicit columns",
        "implicit rows",
        "grid-auto-rows",
        "columns spread by justify-content",
        "rows spread by align-content",
        "fr columns frozen at their base",
        "grid items spanning columns",
        "grid items spanning rows",
        "grid items placed by negative lines",
        "grid items with a min-width",
        "grid items with an order",
        "grid items past the explicit columns",
        "column tracks length / fr",
        "column tracks min-content / min-content",
        "column tracks max-content / max-content",
        "column tracks auto / length",
        "row tracks auto / fr",
    ] {
        assert!(tally.counts.contains_key(what), "no {what}");
    }
}

#[test]
fn generated_grid_documents_lay_out_alike() {
    let mut run = Run::new("generated grid documents");
    let mut random = Lcg::new(0x1B_0001);
    let master = master_css();
    let (light, _) = themed_master(false);
    let only: Option<usize> = std::env::var("LAYOUT_ONLY").ok().and_then(|v| v.parse().ok());
    for k in 0..2500 {
        let html = grid_document(&mut random);
        if only.is_some_and(|only| only != k) {
            continue;
        }
        let width = [320.0, 375.0, 150.0, 600.0][k % 4];
        let other = [700.0, 240.0, 500.0, 90.0][(k / 4) % 4];
        let sheet = if k % 2 == 0 { &master } else { &light };
        let mut steps = layout_steps(width, other);
        steps.extend([
            Step::Media(other, 640.0),
            Step::Layout(other, None),
            Step::Dump,
            Step::Hover(k as u32),
            Step::Layout(other, None),
            Step::Dump,
        ]);
        run.case(
            layout_record(width, 640.0, 2.0, sheet, b"", &html, &steps),
            format!("generated grid {k}: {}", String::from_utf8_lossy(&html)),
        );
    }
    run.finish();
}

/// The layout switches, each on both sides over generated documents and the written flex, positioned, table and
/// grid shapes.
#[test]
fn the_layout_switches_lay_out_alike() {
    let master = master_css();
    for switch in
        ["LH_NO_LAYOUT_CACHE", "LH_NO_POSITIONED_REUSE", "LH_NO_FIXED_LAYOUT_REUSE", "LH_LAYOUT_CACHE_CHECK", "LH_NO_COLUMN_REUSE"]
    {
        let mut run = Run::with_switch("switch", switch);
        let mut random = Lcg::new(0x17_0002);
        for k in 0..450 {
            let html = if k % 3 == 2 { flex_document(&mut random) } else { layout_document(&mut random) };
            let mut steps = layout_steps(320.0, 500.0);
            steps.extend([Step::Hover(k as u32), Step::Layout(320.0, None), Step::Dump]);
            run.case(
                layout_record(320.0, 640.0, 2.0, &master, b"", &html, &steps),
                format!("{switch} {k}: {}", String::from_utf8_lossy(&html)),
            );
        }
        let mut random = Lcg::new(0x19_0002);
        for k in 0..200 {
            let html = positioned_document(&mut random);
            let mut steps = layout_steps(320.0, 500.0);
            steps.extend([Step::Hover(k as u32), Step::Layout(320.0, None), Step::Dump]);
            run.case(
                layout_record(320.0, 640.0, 2.0, &master, b"", &html, &steps),
                format!("{switch} positioned {k}: {}", String::from_utf8_lossy(&html)),
            );
        }
        for (what, html) in written_flex_documents().into_iter().chain(written_positioned_documents()) {
            let mut steps = layout_steps(375.0, 200.0);
            steps.extend([Step::Hover(2), Step::Layout(375.0, None), Step::Dump]);
            run.case(layout_record(375.0, 667.0, 2.0, &master, b"", html.as_bytes(), &steps), format!("{switch}: {what}"));
        }
        for (what, html) in written_table_documents() {
            let mut steps = layout_steps(375.0, 200.0);
            steps.extend([Step::Hover(5), Step::Layout(375.0, None), Step::Dump]);
            run.case(layout_record(375.0, 667.0, 2.0, &master, b"", html.as_bytes(), &steps), format!("{switch}: {what}"));
        }
        for (what, html) in written_grid_documents() {
            let mut steps = layout_steps(375.0, 200.0);
            steps.extend([Step::Hover(2), Step::Layout(375.0, None), Step::Dump]);
            run.case(layout_record(375.0, 667.0, 2.0, &master, b"", html.as_bytes(), &steps), format!("{switch}: {what}"));
        }
        let mut random = Lcg::new(0x1B_0002);
        for k in 0..150 {
            let html = grid_document(&mut random);
            let mut steps = layout_steps(320.0, 500.0);
            steps.extend([Step::Hover(k as u32), Step::Layout(320.0, None), Step::Dump]);
            run.case(
                layout_record(320.0, 640.0, 2.0, &master, b"", &html, &steps),
                format!("{switch} grid {k}: {}", String::from_utf8_lossy(&html)),
            );
        }
        let mut random = Lcg::new(0x1C_0002);
        for k in 0..150 {
            let html = table_document(&mut random);
            let mut steps = layout_steps(320.0, 500.0);
            steps.extend([Step::Hover(k as u32), Step::Layout(320.0, None), Step::Dump]);
            run.case(
                layout_record(320.0, 640.0, 2.0, &master, b"", &html, &steps),
                format!("{switch} table {k}: {}", String::from_utf8_lossy(&html)),
            );
        }
        run.finish();
    }
}

/// Layouts past a budget: every box returns at once, the document lays out nothing again.
/// The corpus, generated documents and the written shapes with a container that lets texts owe their widths until they
/// are read, as TwidgetCore's does: the same layout, its widths measured when the layout first reads them.
#[test]
fn documents_lay_out_alike_measuring_later() {
    let mut run = Run::with_switch("measuring later", "TW_MEASURES_TEXT_LATER");
    let light = themed_master(false);
    let dark = themed_master(true);
    for (k, (name, text)) in corpus_pages().iter().enumerate() {
        let snap = snapshot(text);
        let dark_theme = k % 2 == 1;
        let (master, vars) = if dark_theme { &dark } else { &light };
        let html = themed_html(&snap, dark_theme, vars);
        let mut steps = layout_steps(375.0, 320.0);
        steps.extend([
            Step::Media(240.0, 900.0),
            Step::Layout(240.0, None),
            Step::Dump,
            Step::Hover(k as u32 * 7),
            Step::Layout(240.0, None),
            Step::Dump,
        ]);
        run.case(layout_record(375.0, 667.0, 2.0, master, b"", &html, &steps), format!("{name} measuring later"));
    }
    let master = master_css();
    let mut random = Lcg::new(0x17_0004);
    for k in 0..1200 {
        let html = match k % 5 {
            0 => flex_document(&mut random),
            1 => table_document(&mut random),
            2 => grid_document(&mut random),
            3 => positioned_document(&mut random),
            _ => layout_document(&mut random),
        };
        let width = [320.0, 375.0, 150.0, 600.0][k % 4];
        let mut steps = layout_steps(width, 240.0);
        steps.extend([Step::Hover(k as u32), Step::Layout(width, None), Step::Dump]);
        run.case(
            layout_record(width, 640.0, 2.0, &master, b"", &html, &steps),
            format!("generated {k} measuring later: {}", String::from_utf8_lossy(&html)),
        );
    }
    for (what, html) in written_flex_documents()
        .into_iter()
        .chain(written_positioned_documents())
        .chain(written_table_documents())
        .chain(written_grid_documents())
    {
        let mut steps = layout_steps(375.0, 200.0);
        steps.extend([Step::Hover(2), Step::Layout(375.0, None), Step::Dump]);
        run.case(layout_record(375.0, 667.0, 2.0, &master, b"", html.as_bytes(), &steps), format!("{what} measuring later"));
    }
    run.finish();
}

#[test]
fn the_budget_is_crossed_alike() {
    let mut run = Run::new("budget");
    let master = master_css();
    let mut deep = String::from("<!DOCTYPE html><html><body>");
    for k in 0..60 {
        deep += if k % 3 == 0 {
            "<div style=\"margin:1px\">a "
        } else if k % 3 == 1 {
            "<p>b <span style=\"display:inline-block\">c</span> "
        } else {
            "<div style=\"float:left\">d "
        };
    }
    for k in (0..60).rev() {
        deep += if k % 3 == 1 { "</p>" } else { "</div>" };
    }
    deep += "</body></html>";
    for budget in [1u64, 2, 5, 17, 40, 100, 1000] {
        let steps = vec![Step::Layout(300.0, Some(budget)), Step::Dump, Step::Layout(500.0, Some(budget * 10)), Step::Dump];
        run.case(layout_record(375.0, 667.0, 2.0, &master, b"", deep.as_bytes(), &steps), format!("budget {budget}"));
    }
    let tally = run.finish();
    assert!(tally.counts.contains_key("layouts exhausted"));
}

/// A document 124 levels deep of blocks, inline boxes, floats and inlines lays out on a thread whose stack is the size
/// the cascade's measure asked (1 MiB unoptimized, 512 KiB optimized), as the helper's threads will run it.
#[test]
fn a_deep_document_lays_out_on_a_small_stack() {
    let stack = if cfg!(debug_assertions) { 1 << 20 } else { 512 << 10 };
    let thread = std::thread::Builder::new().stack_size(stack).spawn(|| {
        let mut html = String::from("<!DOCTYPE html><html><head></head><body>");
        for k in 0..124 {
            html += match k % 4 {
                0 => "<span>a ",
                1 => "<div style=\"display:inline-block\">b ",
                2 => "<div style=\"float:left\">c ",
                _ => "<div style=\"margin:1px\">",
            };
        }
        html += "<div>deep</div>";
        for k in (0..124).rev() {
            html += match k % 4 {
                0 => "</span>",
                _ => "</div>",
            };
        }
        html += "</body></html>";
        let lh = Lh::new();
        let mut tally = LayoutTally::default();
        let input = layout_record(375.0, 667.0, 2.0, &master_css(), b"", html.as_bytes(), &[Step::Layout(375.0, None), Step::Dump]);
        let answer = layout_answer(&lh, Switches::default(), &input, &mut tally);
        assert_eq!(answer.0, 1);
        tally.items
    });
    let items = thread.expect("a thread").join().expect("the layout ran");
    eprintln!("a deep document: {items} items laid out");
    assert!(items > 200);
}

/// 124 levels of absolute, fixed, relative and transformed boxes, with static blocks between, lay out and are placed
/// on the same stacks.
#[test]
fn a_deep_positioned_document_lays_out_on_a_small_stack() {
    let stack = if cfg!(debug_assertions) { 1 << 20 } else { 512 << 10 };
    let thread = std::thread::Builder::new().stack_size(stack).spawn(|| {
        let mut html = String::from("<!DOCTYPE html><html><head></head><body>");
        for k in 0..124 {
            html += match k % 5 {
                0 => "<div style=\"position:absolute;left:1px;right:2px\">a ",
                1 => "<div style=\"position:relative;top:1px\">b ",
                2 => "<div style=\"transform:translate(1px,1px);margin:1px\">c ",
                3 => "<div style=\"position:fixed;top:2px;width:50%\">d ",
                _ => "<div style=\"padding:1px\">",
            };
        }
        html += "<div style=\"position:absolute;bottom:0\">deep</div>";
        for _ in 0..124 {
            html += "</div>";
        }
        html += "</body></html>";
        let lh = Lh::new();
        let mut tally = LayoutTally::default();
        let input = layout_record(375.0, 667.0, 2.0, &master_css(), b"", html.as_bytes(), &[Step::Layout(375.0, None), Step::Dump]);
        let answer = layout_answer(&lh, Switches::default(), &input, &mut tally);
        assert_eq!(answer.0, 1);
        tally.items
    });
    let items = thread.expect("a thread").join().expect("the layout ran");
    eprintln!("a deep positioned document: {items} items laid out");
    assert!(items > 200);
}

/// 124 levels of flex boxes, row and column, wrapping and not, with blocks and inline blocks between, lay out on the
/// same stacks.
#[test]
fn a_deep_flex_document_lays_out_on_a_small_stack() {
    let stack = if cfg!(debug_assertions) { 1 << 20 } else { 512 << 10 };
    let thread = std::thread::Builder::new().stack_size(stack).spawn(|| {
        let mut html = String::from("<!DOCTYPE html><html><head></head><body>");
        for k in 0..124 {
            html += match k % 4 {
                0 => "<div style=\"display:flex;align-items:baseline\">a ",
                1 => "<div style=\"display:flex;flex-direction:column;flex:1\">b ",
                2 => "<div style=\"display:inline-flex;flex-wrap:wrap;margin:auto\">c ",
                _ => "<div style=\"display:inline-block\">",
            };
        }
        html += "<div>deep</div>";
        for _ in 0..124 {
            html += "</div>";
        }
        html += "</body></html>";
        let lh = Lh::new();
        let mut tally = LayoutTally::default();
        let input = layout_record(375.0, 667.0, 2.0, &master_css(), b"", html.as_bytes(), &[Step::Layout(375.0, None), Step::Dump]);
        let answer = layout_answer(&lh, Switches::default(), &input, &mut tally);
        assert_eq!(answer.0, 1);
        tally.items
    });
    let items = thread.expect("a thread").join().expect("the layout ran");
    eprintln!("a deep flex document: {items} items laid out");
    assert!(items > 200);
}

/// Tables nested 31 deep (126 levels of the DOM with their row groups, rows and cells, under the 128 the DOM keeps)
/// lay out on the same stacks; the renderer's budget stops the layouts each level multiplies.
#[test]
fn a_deep_table_document_lays_out_on_a_small_stack() {
    let stack = if cfg!(debug_assertions) { 1 << 20 } else { 512 << 10 };
    let thread = std::thread::Builder::new().stack_size(stack).spawn(|| {
        let mut html = String::from("<!DOCTYPE html><html><head></head><body>");
        for k in 0..31 {
            html += match k % 3 {
                0 => "<table><tr><td>a ",
                1 => "<table style=\"border-collapse:collapse;width:100%\"><caption>c</caption><tr><td colspan=\"2\">b ",
                _ => "<table style=\"table-layout:fixed;width:300px\"><tr><td rowspan=\"2\">c ",
            };
        }
        html += "deep";
        for _ in 0..31 {
            html += "</td><td>x</td></tr></table>";
        }
        html += "</body></html>";
        let lh = Lh::new();
        let mut tally = LayoutTally::default();
        let input = layout_record(375.0, 667.0, 2.0, &master_css(), b"", html.as_bytes(), &[Step::Layout(375.0, None), Step::Dump]);
        let answer = layout_answer(&lh, Switches::default(), &input, &mut tally);
        assert_eq!(answer.0, 1);
        (tally.items, tally.tables)
    });
    let (items, tables) = thread.expect("a thread").join().expect("the layout ran");
    eprintln!("a deep table document: {items} items laid out, {tables} tables");
    assert!(tables >= 31);
}

/// 124 levels of grids, inline grids and flex boxes, each an item of the one above, lay out on the same stacks (the
/// nested passes cross the layout budget long before they end, after the first has gone all the way down).
#[test]
fn a_deep_grid_document_lays_out_on_a_small_stack() {
    let stack = if cfg!(debug_assertions) { 1 << 20 } else { 512 << 10 };
    let thread = std::thread::Builder::new().stack_size(stack).spawn(|| {
        let mut html = String::from("<!DOCTYPE html><html><head></head><body>");
        for k in 0..124 {
            html += match k % 4 {
                0 => "<div style=\"display:grid;grid-template-columns:1fr auto;gap:2px\">a ",
                1 => "<div style=\"display:inline-grid;grid-template-columns:repeat(auto-fill, 20px);grid-column:span 2\">b ",
                2 => "<div style=\"display:grid;grid-template-rows:1fr;height:50px;align-items:center\">",
                _ => "<div style=\"display:flex\">c ",
            };
        }
        html += "<div>deep</div>";
        for _ in 0..124 {
            html += "</div>";
        }
        html += "</body></html>";
        let lh = Lh::new();
        let mut tally = LayoutTally::default();
        let input = layout_record(375.0, 667.0, 2.0, &master_css(), b"", html.as_bytes(), &[Step::Layout(375.0, None), Step::Dump]);
        let answer = layout_answer(&lh, Switches::default(), &input, &mut tally);
        assert_eq!(answer.0, 1);
        assert!(tally.counts.contains_key("layouts exhausted"));
        tally.grid_passes
    });
    let passes = thread.expect("a thread").join().expect("the layout ran");
    eprintln!("a deep grid document: {passes} grid passes");
    assert!(passes > 100);
}

/// What the containers' `text_width` measured since `from`.
fn measured(container: &CascadeContainer, from: usize) -> Vec<Vec<u8>> {
    container.calls.borrow()[from..].iter().filter(|c| c.0 == b'W').map(|c| c.1.clone()).collect()
}

/// The first text of the first `<p>` of a document.
fn first_text(doc: &twidget_litehtml::document::Document) -> Option<twidget_litehtml::element::ElementId> {
    let mut pending = doc.root.into_iter().collect::<Vec<_>>();
    while let Some(el) = pending.pop() {
        if doc.tag(el) == twidget_litehtml::string_id::_p_ {
            return doc.children(el).first().copied();
        }
        pending.extend(doc.children(el).iter().rev().copied());
    }
    None
}

/// A container that does not opt in measures every text in the style pass, as litehtml always did; one that opts in
/// measures each when it is read, the same texts at the same widths, and set_text drops what a text owed (ends_tests.cpp's
/// `containers`, which drives litehtml alone).
#[test]
fn containers_measure_texts_when_read() {
    use twidget_litehtml::document::{Document, StyleCx};
    let html = b"<html><body><p>alpha beta</p><p style=\"text-transform:uppercase\">gamma</p><pre>a\tb</pre></body></html>";
    let master = master_css();
    let lh = Lh::new();
    let make = |later: bool| {
        set_measures_text_later(later);
        let container = CascadeContainer::new(300.0, 600.0, 1.0);
        let doc = {
            let cx = StyleCx { names: &lh.names, container: &container, cache: &lh.cache, switches: Switches::default() };
            let (mut doc, root) = Document::build(html, &cx.env());
            doc.finalize_from_external_root(&cx, root, &master, b"");
            doc
        };
        (doc, container)
    };
    let (mut a, now) = make(false);
    let (mut b, later) = make(true);
    let styled: std::collections::BTreeSet<Vec<u8>> = measured(&now, 0).into_iter().collect();
    assert!(
        styled.contains(&b"alpha"[..]) && styled.contains(&b"GAMMA"[..]) && styled.contains(&b"    "[..]),
        "a container that does not opt in measures what the texts show in the style pass"
    );
    assert!(measured(&later, 0).is_empty(), "one that opts in measures nothing there");
    let alpha = first_text(&b).expect("a text");
    assert!(b.width_owed(alpha) && !a.width_owed(first_text(&a).expect("a text")), "its texts owe their widths, the other's none");
    let before = measured(&now, 0).len();
    let layout = |doc: &mut Document, container: &CascadeContainer| {
        let cx = StyleCx { names: &lh.names, container, cache: &lh.cache, switches: Switches::default() };
        doc.set_layout_budget(renderer_layout_budget(doc));
        doc.layout_document(&cx, twidget_litehtml::Px(300.0));
    };
    layout(&mut a, &now);
    layout(&mut b, &later);
    assert_eq!(measured(&now, 0).len(), before, "the container that does not opt in measures nothing more in the layout");
    let read: std::collections::BTreeSet<Vec<u8>> = measured(&later, 0).into_iter().collect();
    assert!(
        read == styled && measured(&later, 0).len() == before,
        "the one that opts in measures the same texts once each as the layout reads them"
    );
    assert!(
        a.layout.size.height.value().to_bits() == b.layout.size.height.value().to_bits()
            && a.layout.size.width.value().to_bits() == b.layout.size.width.value().to_bits()
            && !b.width_owed(alpha),
        "both lay out alike, and no width is owed after the layout"
    );

    let (c, early) = make(true);
    let text = first_text(&c).expect("a text");
    let cx = StyleCx { names: &lh.names, container: &early, cache: &lh.cache, switches: Switches::default() };
    let sz = c.get_content_size(&cx, text, twidget_litehtml::Px(0.0));
    assert!(
        measured(&early, 0) == vec![b"alpha".to_vec()] && sz.width.value() == 40.0 && !c.width_owed(text),
        "a size read before the layout measures that text alone, exactly"
    );
    c.get_content_size(&cx, text, twidget_litehtml::Px(0.0));
    assert_eq!(measured(&early, 0).len(), 1, "and reading it again measures nothing");
    let (mut d, reset) = make(true);
    let changed = first_text(&d).expect("a text");
    let cx = StyleCx { names: &lh.names, container: &reset, cache: &lh.cache, switches: Switches::default() };
    d.set_text(&cx, changed, b"omega");
    assert!(!d.width_owed(changed), "set_text drops the width a text owed");
    assert!(
        measured(&reset, 0).is_empty() && d.get_content_size(&cx, changed, twidget_litehtml::Px(0.0)).width.value() == 0.0,
        "and its size reads as nothing until it is styled again"
    );
    set_measures_text_later(false);
}
