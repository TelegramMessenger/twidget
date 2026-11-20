//! Layout, ported (stages L7 and L8): documents cascaded and built as the renderer builds them, laid out at widths
//! (again at the first after another, so the layout cache is used), after media changes and `:hover` restyles,
//! through `oracle/lhdump` (`Y`) and the port: every render item's place, margins, padding and borders, its cache
//! and marks, positioned lists, scroll views, inline boxes, a text's pieces, an inline context's line boxes and their
//! items, the document's size and budget, each element's line height, with the container's calls in order (fonts,
//! text widths, the viewport, image sizes, text breakers). The documents: widget-sandbox's corpus light and dark,
//! every tag but tables, written shapes for each branch (margins collapsing, floats and clearance, line breaking,
//! white space, `text-overflow`, bidi, vertical alignment, images, lists, `<textarea>`), generated block, float,
//! margin and inline documents, the `LH_NO_*` layout switches, the budget crossed, and a deep document on a small
//! stack. A case whose layout reaches a later stage's (absolute and fixed placement, tables, flex, grid) is laid
//! out by the port as far as it goes but not compared, and counted. Each run reports what it reached.

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
            other => panic!("no switch {other}"),
        }
        run
    }

    fn case(&mut self, input: Vec<u8>, what: impl Into<String>) {
        let mut tally = LayoutTally::default();
        let (rust, unported) = layout_answer(&self.lh, self.switches, &input, &mut tally);
        self.cases += 1;
        self.tally.documents += 1;
        if let Some(stage) = unported {
            *self.tally.skipped.entry(stage).or_default() += 1;
            // The case changed the process's name table and declaration cache, which lhdump never sees: the cases
            // before it are compared now, and the next start afresh on both sides.
            self.flush();
            self.lh = Lh::new();
            return;
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

/// The widths a case is laid out at: its own, another, its own again (the cache).
fn layout_steps(width: f32, other: f32) -> Vec<Step> {
    vec![Step::Layout(width, None), Step::Dump, Step::Layout(other, None), Step::Dump, Step::Layout(width, None), Step::Dump]
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
    assert!(run_compared(&tally) > 300);
}

fn run_compared(tally: &LayoutTally) -> usize {
    tally.documents - tally.skipped.values().sum::<usize>()
}

/// Every tag but the table ones, under the master sheet and the UA layers.
#[test]
fn every_tag_lays_out_alike() {
    let mut run = Run::new("every tag");
    let master = master_css();
    let (light, _) = themed_master(false);
    let (dark, vars) = themed_master(true);
    let steps = layout_steps(375.0, 1200.0);
    for quirks in [false, true] {
        let mut html = String::from_utf8(every_tag_document(quirks)).expect("UTF-8");
        for tag in ["table", "caption", "thead", "tbody", "tfoot", "tr", "th", "td"] {
            html = html.replace(&format!("<{tag} "), "<div data-was-table ").replace(&format!("</{tag}>"), "</div>");
        }
        let html = html.into_bytes();
        // Text fields, ranges and selects are flex boxes (stage L10): every input is a checkbox, button or radio here.
        let html = String::from_utf8(html).expect("UTF-8");
        let mut html = html.replace("<select", "<span").replace("</select>", "</span>");
        for ty in ["text", "password", "number", "date", "email", "search", "range", "color", "hidden"] {
            html = html.replace(&format!("type=\"{ty}\""), if ty.len() % 2 == 0 { "type=\"button\"" } else { "type=\"radio\"" });
        }
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
    let tally = run.finish();
    assert!(run_compared(&tally) > 2000, "{} compared", run_compared(&tally));
}

/// The layout switches, each on both sides over generated documents.
#[test]
fn the_layout_switches_lay_out_alike() {
    let master = master_css();
    for switch in ["LH_NO_LAYOUT_CACHE", "LH_NO_POSITIONED_REUSE", "LH_NO_FIXED_LAYOUT_REUSE", "LH_LAYOUT_CACHE_CHECK"] {
        let mut run = Run::with_switch("switch", switch);
        let mut random = Lcg::new(0x17_0002);
        for k in 0..300 {
            let html = layout_document(&mut random);
            let mut steps = layout_steps(320.0, 500.0);
            steps.extend([Step::Hover(k as u32), Step::Layout(320.0, None), Step::Dump]);
            run.case(
                layout_record(320.0, 640.0, 2.0, &master, b"", &html, &steps),
                format!("{switch} {k}: {}", String::from_utf8_lossy(&html)),
            );
        }
        run.finish();
    }
}

/// Layouts past a budget: every box returns at once, the document lays out nothing again.
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
        let (answer, unported) = layout_answer(&lh, Switches::default(), &input, &mut tally);
        assert_eq!(answer.0, 1);
        assert_eq!(unported, None);
        tally.items
    });
    let items = thread.expect("a thread").join().expect("the layout ran");
    eprintln!("a deep document: {items} items laid out");
    assert!(items > 200);
}
