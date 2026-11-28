//! The DOM, ported: documents built from Gumbo's tree as the renderer builds them (`create_node`, the classes by
//! tag, words and spaces, the limits, the node map, the template and its holes, what `parse_attributes` keeps and
//! asks of the container) through `oracle/lhdump` and the port, field by field (`i`), and the declarations the
//! attributes add to each tag's style through the fork's own `style::add_property` (`I`). The documents: every
//! corpus widget, its serialised snapshot and changes of both, generated documents of every class and attribute,
//! tag soup, shadow trees as the renderer serialises them, and the limits. Each run reports what it reached.

mod common;

use common::dom::*;
use common::gumbo::{any_html, corpus_pages, renderer_html, snapshot};
use common::*;
use twidget_litehtml::document::{Document, Env};
use twidget_litehtml::element::{AttributeValue, ElementKind};
use twidget_litehtml::{Names, string_id};

/// `i` and `I` cases through lhdump and the port, checked in batches of about 24 MB.
struct Run {
    name: &'static str,
    names: Names,
    batch: Comparison,
    bytes: usize,
    cases: usize,
    replays: usize,
    declarations: usize,
    tally: DomTally,
    oracle: Option<Oracle>,
}

impl Run {
    fn new(name: &'static str) -> Run {
        Run {
            name,
            names: Names::new(),
            batch: Comparison::new(),
            bytes: 0,
            cases: 0,
            replays: 0,
            declarations: 0,
            tally: DomTally::default(),
            oracle: Oracle::find("lhdump"),
        }
    }

    fn case(&mut self, parse_attributes: bool, html: &[u8], what: impl Into<String>) {
        let mut input = dom_record(parse_attributes, true, html);
        let (mut rust, mut built) = dom_answer(&self.names, &input);
        if built.doc.template_ties != 0 {
            // std::sort orders tied spans as its library does: the holes are left out of the comparison.
            input = dom_record(parse_attributes, false, html);
            (rust, built) = dom_answer(&self.names, &input);
            *self.tally.counts.entry("documents whose hole spans tie".into()).or_default() += 1;
        }
        self.tally.add(&self.names, &built);
        self.bytes += input.len() + rust.1.len();
        self.cases += 1;
        if self.oracle.is_some() {
            self.batch.case(&framed(b'i', &input), rust, what);
        }
        self.flush_if_full();
    }

    fn replay(&mut self, html: &[u8], what: impl Into<String>) {
        let (input, rust, count) = replay_case(&self.names, html);
        self.bytes += input.len() + rust.1.len();
        self.replays += 1;
        self.declarations += count;
        if self.oracle.is_some() {
            self.batch.case(&framed(b'I', &input), rust, what.into() + " (declarations)");
        }
        self.flush_if_full();
    }

    fn flush_if_full(&mut self) {
        if self.bytes > 24 << 20 {
            self.flush();
        }
    }

    fn flush(&mut self) {
        let batch = std::mem::take(&mut self.batch);
        self.bytes = 0;
        if let Some(oracle) = &self.oracle
            && !batch.is_empty()
        {
            batch.check(oracle);
        }
    }

    fn finish(mut self) -> (usize, DomTally) {
        self.flush();
        let compared = if self.oracle.is_some() { "alike with lhdump" } else { "built (no oracle)" };
        eprintln!(
            "{}: {} documents and {} declaration replays ({} declarations) {compared}; {}",
            self.name,
            self.cases,
            self.replays,
            self.declarations,
            self.tally.report()
        );
        (self.cases, self.tally)
    }
}

fn mutated(random: &mut Lcg, text: &[u8]) -> (Vec<u8>, String) {
    let mut bytes = text.to_vec();
    let what = mutate(random, &mut bytes);
    (bytes, what)
}

// ---- The comparison ----------------------------------------------------------------------------------------

#[test]
fn the_corpus_builds_alike() {
    let mut run = Run::new("corpus");
    let mut random = Lcg::new(0x13_0001);
    let pages = corpus_pages();
    assert!(pages.len() > 300, "widget-sandbox's corpus: {} pages", pages.len());
    for (k, (name, text)) in pages.iter().enumerate() {
        run.case(true, text, name.clone());
        let snapshot = snapshot(text);
        run.case(true, &snapshot, format!("{name}'s snapshot"));
        run.replay(text, name.clone());
        run.replay(&snapshot, format!("{name}'s snapshot"));
        if k % 4 == 0 {
            run.case(false, text, format!("{name} without its attributes parsed"));
            run.case(true, &quirks(text), format!("{name} in quirks mode"));
        }
        for m in 0..6 {
            let (bytes, what) = mutated(&mut random, if m % 2 == 0 { text } else { &snapshot });
            run.case(true, &bytes, format!("{name} mutated {m}:{what}"));
        }
    }
    let (cases, tally) = run.finish();
    assert!(cases > 3000);
    assert!(tally.counts.get("template made").copied().unwrap_or(0) > 300);
}

#[test]
fn generated_documents_build_alike() {
    let mut run = Run::new("generated");
    let mut random = Lcg::new(0x13_0002);
    for k in 0..8000 {
        let html = any_dom_html(&mut random);
        run.case(k % 5 != 0, &html, format!("generated {k}: {}", String::from_utf8_lossy(&html)));
        run.replay(&html, format!("generated {k}: {}", String::from_utf8_lossy(&html)));
        if k % 2 == 0 {
            let (bytes, what) = mutated(&mut random, &html);
            run.case(true, &bytes, format!("generated {k} mutated{what}"));
            run.replay(&bytes, format!("generated {k} mutated{what}"));
        }
    }
    let (_, tally) = run.finish();
    // Every class the factory makes, and the boxes the controls make.
    for kind in [
        ElementKind::html_tag,
        ElementKind::el_anchor,
        ElementKind::el_base,
        ElementKind::el_body,
        ElementKind::el_break,
        ElementKind::el_div,
        ElementKind::el_font,
        ElementKind::el_image,
        ElementKind::el_link,
        ElementKind::el_para,
        ElementKind::el_table,
        ElementKind::el_td,
        ElementKind::el_title,
        ElementKind::el_tr,
        ElementKind::el_svg,
        ElementKind::el_input,
        ElementKind::el_select,
        ElementKind::el_button,
        ElementKind::el_textarea,
        ElementKind::el_fieldset,
        ElementKind::el_meter,
        ElementKind::el_script,
        ElementKind::el_style,
        ElementKind::el_text,
        ElementKind::el_space,
        ElementKind::el_comment,
        ElementKind::el_cdata,
    ] {
        assert!(tally.counts.contains_key(&format!("class {kind:?}")), "no {kind:?} made");
    }
    for key in ["hole text", "hole style", "hole class", "hole src", "hole svg geometry", "call K", "call M", "call B", "call C"] {
        assert!(tally.counts.contains_key(key), "nothing reached {key}");
    }
}

#[test]
fn every_attribute_of_every_class_builds_alike() {
    let mut run = Run::new("attributes");
    for (k, html) in attribute_documents().iter().chain(&control_documents()).enumerate() {
        run.case(true, html, format!("attributes {k}: {}", String::from_utf8_lossy(html)));
        run.replay(html, format!("attributes {k}: {}", String::from_utf8_lossy(html)));
        if k % 3 == 0 {
            run.case(true, &quirks(html), format!("attributes {k} in quirks mode"));
        }
    }
    let (_, tally) = run.finish();
    for input in ["text", "number", "password", "date", "range", "color", "checkbox", "submit", "reset", "button"] {
        assert!(tally.counts.contains_key(&format!("input {input}")), "no input of type {input}");
    }
}

/// Number texts of every shape in the attributes read as numbers: a range's `min`, `max` and `value` (C's
/// `strtof`, the place as a float and `%g`), a field's `size` (`%f`), dimensions and pixel lengths (`strtol` and
/// the dimension microsyntax), a font's `size` (`atoi`).
#[test]
fn numbers_in_attributes_read_alike() {
    let mut run = Run::new("numbers");
    let mut random = Lcg::new(0x13_0004);
    for k in 0..4000 {
        let mut n = || String::from_utf8_lossy(&any_number_text(&mut random)).replace(['"', '<', '>', '&'], "");
        let html = format!(
            "<!DOCTYPE html><body><input type=range min=\"{}\" max=\"{}\" value=\"{}\"><input size=\"{}\"><table width=\"{}\" height=\"{}\" cellspacing=\"{}\" border=\"{}\"><tr height=\"{}\"><td width=\"{}\" height=\"{}\"></td></tr></table><img width=\"{}\"><font size=\"{}\">f</font><svg height=\"{}\"></svg>",
            n(),
            n(),
            n(),
            n(),
            n(),
            n(),
            n(),
            n(),
            n(),
            n(),
            n(),
            n(),
            n(),
            n()
        );
        run.case(true, html.as_bytes(), format!("numbers {k}: {html}"));
        run.replay(html.as_bytes(), format!("numbers {k}: {html}"));
    }
    run.finish();
}

#[test]
fn tag_soup_and_serialised_trees_build_alike() {
    let mut run = Run::new("tag soup and serialised trees");
    let mut random = Lcg::new(0x13_0003);
    for k in 0..6000 {
        let soup = any_html(&mut random);
        run.case(k % 3 != 0, &soup, format!("tag soup {k}: {}", hex(&soup)));
        let tree = renderer_html(&mut random);
        run.case(true, &tree, format!("serialised {k}: {}", String::from_utf8_lossy(&tree)));
        if k % 6 == 0 {
            run.replay(&tree, format!("serialised {k}"));
            run.replay(&soup, format!("tag soup {k}"));
        }
    }
    run.finish();
}

#[test]
fn the_limits_build_alike() {
    let mut run = Run::new("limits");
    for (html, what) in limit_documents() {
        run.case(true, &html, what.clone());
        run.case(false, &html, what + " without its attributes parsed");
    }
    let (_, tally) = run.finish();
    for key in ["call Ldepth", "call Lelements", "Gumbo truncated"] {
        assert!(tally.counts.contains_key(key), "nothing reached {key}");
    }
}

// ---- What the port keeps of the C++ ------------------------------------------------------------------------

fn built(names: &Names, html: &str) -> (Document, Option<twidget_litehtml::ElementId>) {
    let container = DomContainer::default();
    let env = Env { names, container: &container };
    let (mut doc, root) = Document::build(html.as_bytes(), &env);
    if let Some(root) = root {
        doc.parse_attributes(&env, root);
    }
    doc.build_template();
    (doc, root)
}

fn find(doc: &Document, names: &Names, tag: &str) -> twidget_litehtml::ElementId {
    let id = names.id(tag.as_bytes());
    (0..doc.elements.len() as u32).map(twidget_litehtml::ElementId).find(|&e| doc.kind(e).is_html_tag() && doc.el(e).tag == id).expect(tag)
}

#[test]
fn text_becomes_words_and_spaces() {
    let names = Names::new();
    let (doc, _) = built(&names, "<!DOCTYPE html><p>a-b c\u{4e00}\u{4e01}d?e f-</p>");
    let p = find(&doc, &names, "p");
    let words: Vec<(bool, String)> =
        doc.children(p).iter().map(|&c| (doc.is_space(c), String::from_utf8_lossy(&doc.el(c).text).into_owned())).collect();
    let expected = [
        (false, "a-"),
        (false, "b"),
        (true, " "),
        (false, "c"),
        (false, "\u{4e00}"),
        (false, "\u{4e01}"),
        (false, "d?"),
        (false, "e"),
        (true, " "),
        (false, "f-"),
    ];
    assert_eq!(words, expected.map(|(s, w)| (s, w.to_string())));
}

#[test]
fn classes_keep_what_the_fork_keeps() {
    let names = Names::new();
    // a table keeps its row groups and caption only; a style keeps its text in a list of its own; a script its text
    let (doc, _) = built(
        &names,
        "<!DOCTYPE html><table> <caption>c</caption><colgroup></colgroup><tr><td>1</td></tr></table><style>p{}</style><script>x<y</script>",
    );
    let table = find(&doc, &names, "table");
    let kept: Vec<_> = doc.children(table).iter().map(|&c| names.s(doc.tag(c)).to_vec()).collect();
    assert_eq!(kept, [b"caption".to_vec(), b"tbody".to_vec()]);
    let style = (0..doc.elements.len() as u32).map(twidget_litehtml::ElementId).find(|&e| doc.kind(e) == ElementKind::el_style).unwrap();
    assert!(doc.children(style).is_empty());
    assert_eq!(doc.el(style).own_children.len(), 1);
    assert_eq!(doc.parent(doc.el(style).own_children[0]), None);
    assert_eq!(doc.css[0].text, b"p{}");
    let script = (0..doc.elements.len() as u32).map(twidget_litehtml::ElementId).find(|&e| doc.kind(e) == ElementKind::el_script).unwrap();
    assert_eq!(doc.el(script).text, b"x<y");
    let mut text = Vec::new();
    doc.get_text(script, &mut text);
    assert!(text.is_empty());
    assert!(doc.template_ok);
}

#[test]
fn attributes_map_to_declarations() {
    let names = Names::new();
    let (doc, _) = built(
        &names,
        "<!DOCTYPE html><table width=0 border=x cellspacing=-3 bgcolor=red><tr><td width=50% background=a.png>x</td></tr></table><font size=+9 color=blue>f</font><input type=range min=0 max=10 value=2.5>",
    );
    let table = find(&doc, &names, "table");
    let declared: Vec<_> = doc.el(table).declarations.iter().map(|d| names.s(d.name).to_vec()).collect();
    // width 0 is ignored, a border that is no number is 1, a negative cellspacing is no length
    assert_eq!(declared, [b"border-width".to_vec(), b"background-color".to_vec()]);
    let td = find(&doc, &names, "td");
    let AttributeValue::Tokens(width) = &doc.el(td).declarations[0].value else { panic!("a token") };
    assert_eq!(tokens_text(width), "50");
    let AttributeValue::Text(url) = &doc.el(td).declarations[1].value else { panic!("text") };
    assert_eq!(url, b"url('a.png')");
    let font = find(&doc, &names, "font");
    let AttributeValue::Text(size) = &doc.el(font).declarations[1].value else { panic!("text") };
    assert_eq!(size, b"xx-large");
    let thumb = find(&doc, &names, "lh-range-thumb");
    assert_eq!(doc.get_attr(thumb, b"style"), Some(&b"display:block;position:absolute;top:0;left:25%;transform:translateX(-25%)"[..]));
    let track = find(&doc, &names, "lh-range-track");
    assert!(doc.el(track).master_sheet);
    assert_eq!(doc.tag(track), names.id(b"lh-range-track"));
    let _ = string_id::empty_id;
}

#[test]
fn quirks_mode_lowercases_ids_and_classes() {
    let names = Names::new();
    let (doc, _) = built(&names, "<p id=Main class='A  b'>x</p>");
    let p = find(&doc, &names, "p");
    assert_eq!(doc.el(p).str_classes, [b"a".to_vec(), b"b".to_vec()]);
    assert_eq!(&*names.s(doc.el(p).id), b"main");
    assert_eq!(doc.get_attr(p, b"class"), Some(&b"A  b"[..]));
}
