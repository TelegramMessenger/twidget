//! Gumbo, the fork's HTML parser, ported: its tables against the C sources they come from, what the C does that
//! the port keeps (bugs included), core_tests.cpp's check of the clone limit at the parser, and the parse of
//! every corpus widget, its serialised snapshot, generated and mutated HTML, every named reference, the limits
//! and the options through `oracle/lhdump` and the port, every field of every node and every error alike. Each
//! run reports what it reached: tokens by insertion mode and type, tokenizer states, error kinds.

mod common;

use common::gumbo::*;
use common::*;
use twidget_litehtml::gumbo::error::*;
use twidget_litehtml::gumbo::tables::*;
use twidget_litehtml::gumbo::*;

/// `j` cases through lhdump and the port, checked in batches of about 24 MB.
struct Run {
    name: &'static str,
    batch: Comparison,
    bytes: usize,
    cases: usize,
    tally: Tally,
    oracle: Option<Oracle>,
}

impl Run {
    fn new(name: &'static str) -> Run {
        Run { name, batch: Comparison::new(), bytes: 0, cases: 0, tally: Tally::new(), oracle: Oracle::find("lhdump") }
    }

    fn case(&mut self, options: &Options, text: &[u8], what: impl Into<String>) -> GumboOutput {
        let input = record(options, text);
        let (rust, output) = answer(&input);
        self.tally.add(&output);
        self.bytes += input.len() + rust.1.len();
        self.cases += 1;
        if self.oracle.is_some() {
            self.batch.case(&framed(b'j', &input), rust, what);
        }
        if self.bytes > 24 << 20 {
            self.flush();
        }
        output
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

    fn finish(mut self) -> Tally {
        self.flush();
        let compared = if self.oracle.is_some() { "alike with lhdump" } else { "parsed (no oracle)" };
        eprintln!("{}: {} cases {compared}; {}", self.name, self.cases, self.tally.report());
        self.tally
    }
}

fn mutated(random: &mut Lcg, text: &[u8]) -> (Vec<u8>, String) {
    let mut bytes = text.to_vec();
    let what = mutate(random, &mut bytes);
    (bytes, what)
}

// ---- The tables -------------------------------------------------------------------------------------------

fn c_source(name: &str) -> String {
    let path = twidget().join("litehtml/src/gumbo").join(name);
    // A checkout may have CRLF; the compiler reads one newline either way.
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())).replace("\r\n", "\n")
}

fn c_array(source: &str, name: &str) -> Vec<i64> {
    let start = source.find(&format!(" {name}[] = {{")).unwrap_or_else(|| panic!("{name}"));
    let body = &source[start..];
    let body = &body[body.find('{').expect("{") + 1..body.find("};").expect("};")];
    body.split(',').map(str::trim).filter(|v| !v.is_empty()).map(|v| v.parse::<i64>().unwrap_or_else(|_| panic!("{name}: {v}"))).collect()
}

/// tools/gen_gumbo_tables.py wrote tables.rs from these files; each entry is read here again.
#[test]
fn the_tables_are_char_ref_c_and_the_tag_headers() {
    let char_ref = c_source("char_ref.c");
    let check = |name: &str, rust: Vec<i64>| assert_eq!(c_array(&char_ref, name), rust, "{name}");
    check("_char_ref_actions", CHAR_REF_ACTIONS.iter().map(|v| i64::from(*v)).collect());
    check("_char_ref_trans_keys", CHAR_REF_TRANS_KEYS.iter().map(|v| i64::from(*v)).collect());
    check("_char_ref_key_spans", CHAR_REF_KEY_SPANS.iter().map(|v| i64::from(*v)).collect());
    check("_char_ref_index_offsets", CHAR_REF_INDEX_OFFSETS.iter().map(|v| i64::from(*v)).collect());
    check("_char_ref_indicies", CHAR_REF_INDICIES.iter().map(|v| i64::from(*v)).collect());
    check("_char_ref_trans_targs", CHAR_REF_TRANS_TARGS.iter().map(|v| i64::from(*v)).collect());
    check("_char_ref_trans_actions", CHAR_REF_TRANS_ACTIONS.iter().map(|v| i64::from(*v)).collect());
    check("_char_ref_to_state_actions", CHAR_REF_TO_STATE_ACTIONS.iter().map(|v| i64::from(*v)).collect());
    check("_char_ref_from_state_actions", CHAR_REF_FROM_STATE_ACTIONS.iter().map(|v| i64::from(*v)).collect());
    check("_char_ref_eof_trans", CHAR_REF_EOF_TRANS.iter().map(|v| i64::from(*v)).collect());
    assert!(char_ref.contains(&format!("static const int char_ref_start = {CHAR_REF_START};")));
    // Every case of the transition actions' switch: what it sets and how it leaves.
    let switch = &char_ref[char_ref.find("switch ( *(_acts++) )").expect("the switch")..];
    let switch = &switch[..switch.find("_again:").expect("its end")];
    let mut cases = 0;
    for (case, action) in CHAR_REF_ACTION_CODE.iter().enumerate() {
        let (kind, first, second) = *action;
        let Some(at) = switch.find(&format!("\tcase {case}:\n")) else {
            assert_eq!(kind, twidget_litehtml::gumbo::char_ref::Action::NONE, "case {case}");
            continue;
        };
        cases += 1;
        let line = switch[at..].lines().nth(2).expect("the action");
        // The code points as numbers: the C writes them with leading zeros at times.
        let number = |key: &str| {
            line.find(key).map_or(-1, |at| {
                let digits: String = line[at + key.len()..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
                i32::from_str_radix(&digits, 16).expect("hex digits")
            })
        };
        use twidget_litehtml::gumbo::char_ref::Action as A;
        let shape = match kind {
            A::TE_NEXT => line == "\t{te = p+1;}",
            A::MATCH_NEXT => line.starts_with("\t{te = p+1;{ output->first = 0x"),
            A::MATCH_HERE => line.starts_with("\t{te = p;p--;{ output->first = 0x"),
            _ => line.starts_with("\t{{p = ((te))-1;}{ output->first = 0x"),
        };
        assert!(shape && (kind == A::TE_NEXT || line.ends_with(" {p++; goto _out; } }}")), "case {case}: {line}");
        assert_eq!((number("output->first = 0x"), number("output->second = 0x")), (first, second), "case {case}: {line}");
    }
    assert_eq!(cases, 2240);

    let replacements = &char_ref[char_ref.find("kCharReplacements[] = {").expect("kCharReplacements")..];
    for (from, to) in CHAR_REPLACEMENTS.iter().filter(|(f, _)| *f != -1) {
        assert!(replacements.to_ascii_lowercase().contains(&format!("{{{from:#04x}, {to:#06x}}}")), "{from:#x} to {to:#x}");
    }

    let names: Vec<String> = c_source("tag_strings.h")
        .lines()
        .filter(|l| l.starts_with('"'))
        .map(|l| l.trim_end_matches(',').trim_matches('"').to_string())
        .collect();
    assert_eq!(names.len(), GUMBO_TAG_UNKNOWN as usize);
    let sizes: Vec<i64> = c_source("tag_sizes.h")
        .lines()
        .filter(|l| !l.starts_with("//"))
        .flat_map(|l| l.split(',').map(str::trim).filter(|v| !v.is_empty()).map(|v| v.parse().expect("a size")).collect::<Vec<i64>>())
        .collect();
    let enums: Vec<String> = std::fs::read_to_string(twidget().join("litehtml/gumbo-include/gumbo/tag_enum.h"))
        .expect("tag_enum.h")
        .lines()
        .filter(|l| l.starts_with("GUMBO_TAG_"))
        .map(|l| l.trim_end_matches(',').to_string())
        .collect();
    for (i, name) in names.iter().enumerate() {
        assert_eq!(TAG_NAMES[i], name.as_bytes());
        assert_eq!(i64::from(TAG_SIZES[i]), sizes[i]);
        assert_eq!(enums[i], format!("GUMBO_TAG_{}", name.to_ascii_uppercase().replace('-', "_")));
    }
    let gperf = c_source("tag_gperf.h");
    let asso = &gperf[gperf.find("asso_values[] =").expect("asso_values")..];
    let asso: Vec<i64> = asso[asso.find('{').expect("{") + 1..asso.find("};").expect("};")]
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.parse().expect("a value"))
        .collect();
    assert_eq!(asso, TAG_ASSO_VALUES.iter().map(|v| i64::from(*v)).collect::<Vec<_>>());
    let map = &gperf[gperf.find("kGumboTagMap[] = {").expect("kGumboTagMap")..];
    let map: Vec<&str> =
        map[map.find('{').expect("{") + 1..map.find("};").expect("};")].split(',').map(str::trim).filter(|v| !v.is_empty()).collect();
    assert_eq!(map.len(), TAG_MAP.len());
    for (entry, tag) in map.iter().zip(TAG_MAP.iter()) {
        let expected =
            if *entry == "GUMBO_TAG_LAST" { GUMBO_TAG_LAST } else { enums.iter().position(|e| e == entry).expect("a tag") as GumboTag };
        assert_eq!(*tag, expected, "{entry}");
    }
}

#[test]
fn every_tag_is_found_by_its_name_in_any_case() {
    for (i, name) in TAG_NAMES[..GUMBO_TAG_UNKNOWN as usize].iter().enumerate() {
        assert_eq!(gumbo_tagn_enum(name), i as GumboTag);
        assert_eq!(gumbo_tagn_enum(&name.to_ascii_uppercase()), i as GumboTag);
        assert_eq!(gumbo_normalized_tagname(i as GumboTag), *name);
    }
    for unknown in [&b""[..], b"x", b"divv", b"di", b"d\xc3\xadv", b"h7", b"\0"] {
        assert_eq!(gumbo_tagn_enum(unknown), GUMBO_TAG_UNKNOWN);
    }
    assert_eq!(gumbo_tag_enum(b"svg\0x"), GUMBO_TAG_SVG);
    let text = b"<my-el a=b></my-el ><x/>";
    let mut start = GumboStringPiece { data: Some(0), length: 11 };
    gumbo_tag_from_original_text(text, &mut start);
    assert_eq!(start.bytes(text), b"my-el");
    let mut end = GumboStringPiece { data: Some(11), length: 9 };
    gumbo_tag_from_original_text(text, &mut end);
    assert_eq!(end.bytes(text), b"my-el ");
    let mut slash = GumboStringPiece { data: Some(20), length: 4 };
    gumbo_tag_from_original_text(text, &mut slash);
    assert_eq!(slash.bytes(text), b"x");
    assert_eq!(twidget_litehtml::gumbo::tag::gumbo_normalize_svg_tagname(b"FOREIGNOBJECT"), Some(&b"foreignObject"[..]));
}

// ---- What the C does, kept ----------------------------------------------------------------------------------

fn parse(text: &[u8]) -> GumboOutput {
    gumbo_parse_with_options(&kGumboDefaultOptions, text)
}

/// The elements of a parse in document order, as `tag` or `ns:tag` for foreign ones.
fn elements(output: &GumboOutput) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![output.document];
    while let Some(id) = stack.pop() {
        let node = output.node(id);
        if node.is_element() {
            let e = node.element();
            let mut name = String::from_utf8_lossy(gumbo_normalized_tagname(e.tag)).to_string();
            if name.is_empty() {
                let mut piece = e.original_tag;
                gumbo_tag_from_original_text(&[], &mut piece);
                name = "?".to_string();
            }
            out.push(match e.tag_namespace {
                GUMBO_NAMESPACE_SVG => format!("svg:{name}"),
                GUMBO_NAMESPACE_MATHML => format!("math:{name}"),
                _ => name,
            });
        }
        if let Some(children) = node.children() {
            stack.extend(children.iter().rev());
        }
    }
    out
}

fn body(output: &GumboOutput) -> &GumboElement {
    let html = output.node(output.root.expect("a root")).element();
    output.node(*html.children.last().expect("a body")).element()
}

#[test]
fn a_misnested_document_is_mended_as_the_spec_says() {
    let output = parse(b"<p>a<b>b<i>c</b>d</i>e<table><tr><td>f</table>g");
    assert_eq!(elements(&output), ["html", "head", "body", "p", "b", "i", "i", "table", "tbody", "tr", "td"]);
    assert!(!output.truncated);
    assert_eq!(output.node(output.document).document().doc_type_quirks_mode, GUMBO_DOCTYPE_QUIRKS);
    let output = parse(b"<!DOCTYPE html><table>x<tr>y</table>");
    assert_eq!(output.node(output.document).document().doc_type_quirks_mode, GUMBO_DOCTYPE_NO_QUIRKS);
    // Foster parenting: each text goes before the table, a node of its own.
    let body = body(&output);
    assert_eq!(output.node(body.children[0]).text().text, b"x");
    assert_eq!(output.node(body.children[1]).text().text, b"y");
    assert_eq!(output.node(body.children[2]).element().tag, GUMBO_TAG_TABLE);
}

/// gumbo's quirks lists are prefixes, but parser.c compares whole identifiers (`strcasecmp`), so a prefix
/// alone makes the quirks mode and a longer identifier does not.
#[test]
fn a_quirks_prefix_counts_only_as_the_whole_identifier() {
    let quirks = |public: &str| {
        let output = parse(format!("<!DOCTYPE html PUBLIC \"{public}\">").as_bytes());
        output.node(output.document).document().doc_type_quirks_mode
    };
    assert_eq!(quirks("-//IETF//DTD HTML//"), GUMBO_DOCTYPE_QUIRKS);
    assert_eq!(quirks("-//ietf//dtd html//"), GUMBO_DOCTYPE_QUIRKS);
    assert_eq!(quirks("-//IETF//DTD HTML//EN"), GUMBO_DOCTYPE_NO_QUIRKS);
    assert_eq!(quirks("-//W3C//DTD XHTML 1.0 Transitional//"), GUMBO_DOCTYPE_LIMITED_QUIRKS);
    assert_eq!(quirks("-//W3C//DTD XHTML 1.0 Transitional//EN"), GUMBO_DOCTYPE_NO_QUIRKS);
}

/// A NUL before an attribute's name goes to the tokenizer's temporary buffer, not to the name.
#[test]
fn a_nul_before_an_attribute_name_is_lost() {
    let output = parse(b"<p \0x=1>");
    let p = output.node(body(&output).children[0]).element();
    assert_eq!(p.attributes.len(), 1);
    assert_eq!(p.attributes[0].name, b"x");
    assert!(output.errors.iter().any(|e| e.ty == GUMBO_ERR_UTF8_NULL));
}

/// In foreign content a NUL becomes U+FFFD in a text that stays whitespace: a NULL token does not make the text
/// node a text one.
#[test]
fn a_nul_in_foreign_content_is_whitespace_text() {
    let output = parse(b"<svg>\0</svg>");
    let svg = output.node(body(&output).children[0]).element();
    let text = output.node(svg.children[0]);
    assert_eq!(text.ty, GUMBO_NODE_WHITESPACE);
    assert_eq!(text.text().text, "\u{fffd}".as_bytes());
}

/// `<image>` becomes `<img>` before the check that would flag it, which then never holds.
#[test]
fn an_image_is_an_img_without_its_flag() {
    let output = parse(b"<image src=x>");
    let img = output.node(body(&output).children[0]);
    assert_eq!(img.element().tag, GUMBO_TAG_IMG);
    assert_eq!(img.parse_flags & GUMBO_INSERTION_FROM_IMAGE, 0);
    assert_eq!(output.errors.iter().filter(|e| e.ty == GUMBO_ERR_PARSER).count(), 2);
}

/// insert_foreign_element checks `xmlns` on the token after the element took its attributes: never an error.
#[test]
fn a_wrong_xmlns_is_no_error() {
    let errors = |text: &[u8]| parse(text).errors.len();
    assert_eq!(errors(b"<!DOCTYPE html><svg xmlns=\"wrong\"></svg>"), errors(b"<!DOCTYPE html><svg></svg>"));
}

/// A character reference in an attribute that ends the text looks at the byte after it: the NUL that ends a
/// C string.
#[test]
fn a_reference_that_ends_an_attribute_reads_the_terminator() {
    let output = parse(b"<a href=x&amp");
    assert!(output.errors.iter().any(|e| e.ty == GUMBO_ERR_ATTR_UNQUOTED_EOF));
    let output = parse(b"<a href=\"x&amp\">");
    let a = output.node(body(&output).children[0]).element();
    assert_eq!(a.attributes[0].value, b"x&");
    let output = parse(b"<a href=\"&ampx\">");
    let a = output.node(body(&output).children[0]).element();
    assert_eq!(a.attributes[0].value, b"&ampx");
}

/// With no room for errors (`max_errors`), add_duplicate_attr_error returns before it reinitialises the tag
/// buffer: a duplicate attribute's name runs into the next one's. Bounding the errors is not neutral.
#[test]
fn with_no_room_for_errors_a_duplicate_name_runs_into_the_next() {
    let names = |max_errors: i32| {
        let options = GumboOptions { max_errors, ..kGumboDefaultOptions };
        let output = gumbo_parse_with_options(&options, b"<a b b c=1>");
        let a = output.node(body(&output).children[0]).element();
        a.attributes
            .iter()
            .map(|attr| (String::from_utf8_lossy(&attr.name).to_string(), String::from_utf8_lossy(&attr.value).to_string()))
            .collect::<Vec<_>>()
    };
    assert_eq!(names(-1), [("b".to_string(), String::new()), ("c".to_string(), "1".to_string())]);
    assert_eq!(names(0), [("b".to_string(), String::new()), ("bc".to_string(), "1".to_string())]);
}

/// The source positions litehtml reads: a start tag's text, an attribute's quoted value, a text's span.
#[test]
fn source_spans_cover_what_litehtml_reads() {
    let text = b"<div class=\"a b\" id=x>one &amp; two</div>";
    let output = parse(text);
    let div = output.node(body(&output).children[0]).element();
    assert_eq!(div.original_tag.bytes(text), b"<div class=\"a b\" id=x>");
    assert_eq!(div.attributes[0].original_value.bytes(text), b"\"a b\"");
    assert_eq!(div.attributes[1].original_value.bytes(text), b"x");
    assert_eq!(div.original_end_tag.bytes(text), b"</div>");
    let t = output.node(div.children[0]).text();
    assert_eq!(t.text, b"one & two");
    assert_eq!(t.original_text.bytes(text), b"one &amp; two");
}

// ---- core_tests.cpp ----------------------------------------------------------------------------------------

/// core_tests.cpp, "formatting elements the parser opens again in every block that follows", at the parser: the
/// small case parses whole and opens each `<b>` again in each `<div>`, the large one stops once its clones pass
/// the input's length and 1 MB.
#[test]
fn formatting_elements_the_parser_opens_again_in_every_block_that_follows() {
    let mut run = Run::new("formatting clones");
    let few = run.case(&Options::default(), &formatting_blocks(3, 20, 1), "3 formatting, 20 blocks");
    assert!(!few.truncated);
    let divs = few.nodes.iter().filter(|n| n.is_element() && n.element().tag == GUMBO_TAG_DIV && n.parent.is_some()).count();
    let clones = few.nodes.iter().filter(|n| n.parse_flags & GUMBO_INSERTION_RECONSTRUCTED_FORMATTING_ELEMENT != 0).count();
    assert_eq!((divs, clones), (20, 60));
    let many = run.case(&Options::default(), &formatting_blocks(124, 925, 2000), "124 formatting, 925 blocks");
    assert!(many.truncated);
    let made = many.nodes.iter().filter(|n| n.is_element()).count();
    assert!(made < 2000, "{made} elements before the parse stopped");
    run.finish();
}

// ---- The comparison ----------------------------------------------------------------------------------------

#[test]
fn the_corpus_parses_alike() {
    let mut run = Run::new("corpus");
    let mut random = Lcg::new(0x63_0001);
    let pages = corpus_pages();
    assert!(pages.len() > 300, "widget-sandbox's corpus: {} pages", pages.len());
    for (name, text) in &pages {
        run.case(&Options::default(), text, name.clone());
        run.case(&Options::fragment(), text, format!("{name} as a fragment"));
        let snapshot = snapshot(text);
        run.case(&Options::default(), &snapshot, format!("{name}'s snapshot"));
        for k in 0..24 {
            let (bytes, what) = mutated(&mut random, text);
            run.case(&Options::default(), &bytes, format!("{name} mutated {k}:{what}"));
            let (bytes, what) = mutated(&mut random, &snapshot);
            run.case(&Options::default(), &bytes, format!("{name}'s snapshot mutated {k}:{what}"));
        }
    }
    let tally = run.finish();
    assert!(tally.cases > 20000);
}

/// Tag soup, each text alone, changed and as a fragment.
fn tag_soup(name: &'static str, seed: u64) {
    let mut run = Run::new(name);
    let mut random = Lcg::new(seed);
    for k in 0..50000 {
        let text = any_html(&mut random);
        run.case(&Options::default(), &text, format!("{name} {k}: {}", hex(&text)));
        if k % 2 == 0 {
            let (bytes, what) = mutated(&mut random, &text);
            run.case(&Options::default(), &bytes, format!("{name} {k} mutated{what}"));
        }
        if k % 8 == 0 {
            run.case(&Options::fragment(), &text, format!("{name} {k} as a fragment"));
        }
    }
    let tally = run.finish();
    assert!(tally.cases > 80000);
}

#[test]
fn generated_html_parses_alike() {
    tag_soup("generated", 0x63_0002);
}

#[test]
fn more_generated_html_parses_alike() {
    tag_soup("more generated", 0x63_0005);
}

/// Shadow trees serialised as the renderer serialises them, and changed.
#[test]
fn serialised_trees_parse_alike() {
    let mut run = Run::new("serialised");
    let mut random = Lcg::new(0x63_0006);
    for k in 0..25000 {
        let text = renderer_html(&mut random);
        run.case(&Options::default(), &text, format!("serialised {k}: {}", String::from_utf8_lossy(&text)));
        let (bytes, what) = mutated(&mut random, &text);
        run.case(&Options::default(), &bytes, format!("serialised {k} mutated{what}"));
    }
    let tally = run.finish();
    assert!(tally.cases == 50000);
}

/// Every named reference the machine knows, in text and in attributes, with and without its semicolon, before
/// the end, a letter, `=` and a space.
#[test]
fn every_named_reference_parses_alike() {
    let mut run = Run::new("references");
    let entities = entities();
    assert!(entities.len() > 2000, "{} references", entities.len());
    for (name, _, _) in &entities {
        let bare = name.strip_suffix(b";").unwrap_or(name);
        for text in [
            [b"&", &name[..]].concat(),
            [b"&", bare].concat(),
            [b"x&", bare, b"y z"].concat(),
            [b"&", bare, b"=1"].concat(),
            [b"<a title=\"&", bare, b"\">"].concat(),
            [b"<a title=\"&", bare, b"x\">"].concat(),
            [b"<a title=\"&", bare, b"=\">"].concat(),
            [b"<a title=&", bare, b">"].concat(),
            [b"<a title='&", &name[..], b"'>"].concat(),
            [b"<a title=&", bare].concat(),
            [b"<textarea>&", bare, b" </textarea>"].concat(),
        ] {
            run.case(&Options::default(), &text, String::from_utf8_lossy(&text).to_string());
        }
    }
    run.finish();
}

/// The open elements' limit of 400 and the clones' around where they stop a parse, and depths past them.
#[test]
fn the_limits_are_met_alike() {
    let mut run = Run::new("limits");
    let wrap = |open: &str, n: usize, inner: &str| -> Vec<u8> { [open.repeat(n), inner.to_string()].concat().into_bytes() };
    for n in [1, 2, 126, 127, 128, 129, 253, 254, 255, 396, 397, 398, 399, 400, 401, 402, 403, 500, 800, 2000] {
        for (open, inner) in [
            ("<div>", "x"),
            ("<b>", "x"),
            ("<span class=c>", "x<p>y"),
            ("<b data-twid=1>", "<div>x</div><div>y</div>"),
            ("<svg>", "x"),
            ("<g>", "</g>"),
            ("<table><tr><td>", "x"),
            ("<select><option>", "x"),
            ("<ul><li>", "x"),
            ("<template>", "x"),
            ("<a>", "x"),
            ("<math><mi>", "x"),
            ("<form>", "</form>"),
        ] {
            let text = if open == "<g>" { [b"<svg>".to_vec(), wrap(open, n, inner)].concat() } else { wrap(open, n, inner) };
            run.case(&Options::default(), &text, format!("{n} times {open}"));
        }
    }
    for (formatting, blocks, name) in
        [(3, 20, 1), (10, 100, 50), (40, 300, 300), (60, 400, 1000), (124, 925, 2000), (30, 2000, 8000), (124, 300, 8000)]
    {
        run.case(
            &Options::default(),
            &formatting_blocks(formatting, blocks, name),
            format!("{formatting} formatting, {blocks} blocks, {name}"),
        );
    }
    let mut random = Lcg::new(0x63_0003);
    for k in 0..400 {
        let mut text = Vec::new();
        for _ in 0..390 + random.below(20) {
            text.extend_from_slice(
                pick(&mut random, &["<div>", "<b>", "<i>", "<p>", "<svg>", "<table>", "<td>", "<a>", "x", "</b>", "</div>"]).as_bytes(),
            );
        }
        let (bytes, what) = mutated(&mut random, &text);
        run.case(&Options::default(), &text, format!("deep {k}"));
        run.case(&Options::default(), &bytes, format!("deep {k} mutated{what}"));
    }
    let tally = run.finish();
    assert!(tally.truncated > 100);
}

/// Fragments in every context and namespace, a bounded and a stopping error list, other tab stops.
#[test]
fn the_options_parse_alike() {
    let mut run = Run::new("options");
    let mut random = Lcg::new(0x63_0004);
    let samples: Vec<Vec<u8>> = (0..6).map(|_| any_html(&mut random)).collect();
    for tag in 0..GUMBO_TAG_LAST {
        for ns in [GUMBO_NAMESPACE_HTML, GUMBO_NAMESPACE_SVG, GUMBO_NAMESPACE_MATHML] {
            let options = Options { fragment_context: tag, fragment_namespace: ns, ..Options::default() };
            for text in &samples {
                run.case(&options, text, format!("fragment in {tag} ns {ns}"));
            }
            run.case(&options, b"a</title><b>&amp;<tr><td>x</template></html>y", format!("fragment end tags in {tag} ns {ns}"));
        }
    }
    for max_errors in [-1, 0, 1, 2] {
        for text in [&b"<a b b c=1>"[..], b"<a b=1 b=2 c d>", b"<p x x\0y=1 x>"] {
            let options = Options { max_errors, ..Options::default() };
            run.case(&options, text, format!("duplicates with max_errors {max_errors}: {}", String::from_utf8_lossy(text)));
        }
    }
    for k in 0..4000 {
        let text = any_html(&mut random);
        let options = Options {
            max_errors: [-1, 0, 1, 2, 5][random.below(5)],
            stop_on_first_error: random.chance(3),
            tab_stop: [8, 1, 4, 3][random.below(4)],
            ..Options::default()
        };
        run.case(&options, &text, format!("options {k}: {options:?}"));
    }
    run.finish();
}

/// The generated cases reach every insertion mode, every tokenizer state and every error kind parser.c,
/// tokenizer.c, char_ref.c and utf8.c can record (GUMBO_ERR_COMMENT_SPACE_AFTER_DOUBLE_DASH is recorded nowhere).
#[test]
fn generated_html_reaches_every_mode_state_and_error() {
    let mut random = Lcg::new(0x63_0002);
    let mut tally = Tally::new();
    for _ in 0..30000 {
        tally.add(&parse(&any_html(&mut random)));
    }
    tally.add(&parse(&formatting_blocks(124, 925, 2000)));
    eprintln!("reached: {}", tally.report());
    assert!(tally.unreached_modes().is_empty(), "modes never reached: {:?}", tally.unreached_modes());
    assert!(tally.unreached_states().is_empty(), "states never reached: {:?}", tally.unreached_states());
    assert_eq!(tally.unreached_errors(), [GUMBO_ERR_COMMENT_SPACE_AFTER_DOUBLE_DASH as usize]);
    assert!(tally.truncated > 0);
}
