//! lhdump's `j` record, Gumbo's parse of a text (oracle/lhdump_gumbo.h), and what the Gumbo cases are made of.
//!
//! The record is `i32 fragment_context, u8 fragment_namespace, i32 max_errors, u8 stop_on_first_error,
//! i32 tab_stop` and the text. The answer, status 1:
//!
//! - `u8 truncated`, `u32` the root's index (`FFFFFFFF` NULL), `u32` node count and the nodes in pre-order from
//!   the document: `u8 type`, `u32` the parent's index (`FFFFFFFF` NULL, `FFFFFFFE` a node outside the tree),
//!   `u64 index_within_parent`, `u32 parse_flags`, then
//!   - the document: `u8 has_doctype`, its name, public and system identifiers (each a `u8` flag for non-NULL
//!     and a string), `u32` quirks mode, `u32` children;
//!   - an element or template: `u32 tag`, `u32` namespace, `original_tag`, `original_end_tag`, start and end
//!     positions, `u32` attributes (each `u32` namespace, name, `original_name`, value, `original_value` and four
//!     positions) and `u32` children;
//!   - text, whitespace, CDATA and comments: the text, `original_text`, the start position.
//! - `u32` error count and each error: `u32 type`, its position, `i64 original_text` (-1 NULL), then what its
//!   kind sets: `u64` code point (UTF-8 and numeric reference kinds but NO_DIGITS, which sets none), a piece (named
//!   references), the name and `u32` indexes (duplicate attribute), `u32` token type, tag, insertion mode and the
//!   open elements' tags (parser kinds), or `i32` code point and `u32` state (the tokenizer's).
//!
//! A piece is `i64` offset into the text (-1 NULL) and `u64` length, a position `u32` line, column and offset,
//! a C string (`const char*`) its bytes up to the first NUL. lhdump allocates zeroed memory, so what Gumbo leaves
//! uninitialised reads as zero, as the port has it.

use std::path::PathBuf;

use twidget_litehtml::gumbo::char_ref::Action;
use twidget_litehtml::gumbo::error::*;
use twidget_litehtml::gumbo::tables::*;
use twidget_litehtml::gumbo::*;

use super::{Lcg, Out, pick};

/// The options a case parses with.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub fragment_context: GumboTag,
    pub fragment_namespace: GumboNamespaceEnum,
    pub max_errors: i32,
    pub stop_on_first_error: bool,
    pub tab_stop: i32,
}

impl Default for Options {
    /// `kGumboDefaultOptions`, what the fork parses a document with.
    fn default() -> Options {
        Options {
            fragment_context: GUMBO_TAG_LAST,
            fragment_namespace: GUMBO_NAMESPACE_HTML,
            max_errors: -1,
            stop_on_first_error: false,
            tab_stop: 8,
        }
    }
}

impl Options {
    /// `append_children_from_string`'s: a fragment in `<body>`.
    pub fn fragment() -> Options {
        Options { fragment_context: GUMBO_TAG_BODY, ..Options::default() }
    }

    pub fn gumbo(&self) -> GumboOptions {
        GumboOptions {
            tab_stop: self.tab_stop,
            stop_on_first_error: self.stop_on_first_error,
            max_errors: self.max_errors,
            fragment_context: self.fragment_context,
            fragment_namespace: self.fragment_namespace,
        }
    }
}

/// The `j` record's bytes for `text` parsed with `options`.
pub fn record(options: &Options, text: &[u8]) -> Vec<u8> {
    let mut o = Out::default();
    o.i32(options.fragment_context as i32)
        .u8(options.fragment_namespace as u8)
        .i32(options.max_errors)
        .u8(u8::from(options.stop_on_first_error));
    o.i32(options.tab_stop);
    o.s.extend_from_slice(text);
    o.take()
}

/// The record's options and text.
pub fn read_record(input: &[u8]) -> (Options, &[u8]) {
    let mut i = super::In::new(input);
    let options = Options {
        fragment_context: i.i32() as GumboTag,
        fragment_namespace: GumboNamespaceEnum::from(i.u8()),
        max_errors: i.i32(),
        stop_on_first_error: i.u8() != 0,
        tab_stop: i.i32(),
    };
    (options, &input[i.at..])
}

/// What the port answers to a `j` record, and the parse it made.
pub fn answer(input: &[u8]) -> ((u8, Vec<u8>), GumboOutput) {
    let (options, text) = read_record(input);
    let output = gumbo_parse_with_options(&options.gumbo(), text);
    ((1, dump(&output)), output)
}

fn piece(o: &mut Out, p: &GumboStringPiece) {
    o.u64(p.data.map_or(u64::MAX, |d| d as u64));
    o.u64(p.length as u64);
}

fn position(o: &mut Out, p: &GumboSourcePosition) {
    o.u32(p.line).u32(p.column).u32(p.offset);
}

fn cstr(o: &mut Out, s: Option<&[u8]>) {
    o.u8(u8::from(s.is_some()));
    o.str(c_str(s.unwrap_or_default()));
}

/// The answer's bytes for a parse.
pub fn dump(output: &GumboOutput) -> Vec<u8> {
    let mut order = Vec::new();
    let mut index = vec![u32::MAX; output.nodes.len()];
    let mut stack = vec![output.document];
    while let Some(node) = stack.pop() {
        index[node] = order.len() as u32;
        order.push(node);
        if let Some(children) = output.nodes[node].children() {
            stack.extend(children.iter().rev());
        }
    }
    let index_of = |node: Option<NodeId>| match node {
        None => u32::MAX,
        Some(n) if index[n] == u32::MAX => 0xFFFF_FFFE,
        Some(n) => index[n],
    };
    let mut o = Out::default();
    o.u8(u8::from(output.truncated));
    o.u32(index_of(output.root));
    o.u32(order.len() as u32);
    for &id in &order {
        let node = &output.nodes[id];
        o.u8(node.ty as u8).u32(index_of(node.parent)).u64(node.index_within_parent as u64).u32(node.parse_flags);
        match &node.v {
            GumboNodeData::Document(d) => {
                o.u8(u8::from(d.has_doctype));
                cstr(&mut o, d.name.as_deref());
                cstr(&mut o, d.public_identifier.as_deref());
                cstr(&mut o, d.system_identifier.as_deref());
                o.u32(d.doc_type_quirks_mode).u32(d.children.len() as u32);
            }
            GumboNodeData::Element(e) => {
                o.u32(e.tag).u32(e.tag_namespace);
                piece(&mut o, &e.original_tag);
                piece(&mut o, &e.original_end_tag);
                position(&mut o, &e.start_pos);
                position(&mut o, &e.end_pos);
                o.u32(e.attributes.len() as u32);
                for a in &e.attributes {
                    o.u32(a.attr_namespace);
                    cstr(&mut o, Some(&a.name));
                    piece(&mut o, &a.original_name);
                    cstr(&mut o, Some(&a.value));
                    piece(&mut o, &a.original_value);
                    for p in [&a.name_start, &a.name_end, &a.value_start, &a.value_end] {
                        position(&mut o, p);
                    }
                }
                o.u32(e.children.len() as u32);
            }
            GumboNodeData::Text(t) => {
                cstr(&mut o, Some(&t.text));
                piece(&mut o, &t.original_text);
                position(&mut o, &t.start_pos);
            }
        }
    }
    o.u32(output.errors.len() as u32);
    for e in &output.errors {
        o.u32(e.ty);
        position(&mut o, &e.position);
        o.u64(e.original_text.map_or(u64::MAX, |d| d as u64));
        match &e.v {
            GumboErrorValue::None => {}
            GumboErrorValue::Codepoint(c) => {
                o.u64(*c);
            }
            GumboErrorValue::Text(p) => piece(&mut o, p),
            GumboErrorValue::DuplicateAttr { name, original_index, new_index } => {
                cstr(&mut o, Some(name));
                o.u32(*original_index).u32(*new_index);
            }
            GumboErrorValue::Parser { input_type, input_tag, parser_state, tag_stack } => {
                o.u32(*input_type).u32(*input_tag).u32(*parser_state).u32(tag_stack.len() as u32);
                for t in tag_stack {
                    o.u32(*t);
                }
            }
            GumboErrorValue::Tokenizer { codepoint, state } => {
                o.i32(*codepoint).u32(*state);
            }
        }
    }
    o.take()
}

/// What the parses of a run reached: tokens by insertion mode and type, characters by tokenizer state, errors by
/// kind, and the parses that stopped at a limit.
#[derive(Clone, Debug, Default)]
pub struct Tally {
    pub cases: usize,
    pub nodes: usize,
    pub truncated: usize,
    pub modes: Vec<[u64; 9]>,
    pub states: Vec<u64>,
    pub errors: Vec<u64>,
}

pub const MODE_NAMES: [&str; 24] = [
    "initial",
    "before html",
    "before head",
    "in head",
    "in head noscript",
    "after head",
    "in body",
    "text",
    "in table",
    "in table text",
    "in caption",
    "in column group",
    "in table body",
    "in row",
    "in cell",
    "in select",
    "in select in table",
    "in template",
    "after body",
    "in frameset",
    "after frameset",
    "after after body",
    "after after frameset",
    "foreign content",
];

pub const TOKEN_NAMES: [&str; 9] = ["doctype", "start tag", "end tag", "comment", "whitespace", "character", "cdata", "null", "eof"];

impl Tally {
    pub fn new() -> Tally {
        Tally { modes: vec![[0; 9]; 24], states: vec![0; 68], errors: vec![0; 42], ..Tally::default() }
    }

    pub fn add(&mut self, output: &GumboOutput) {
        self.cases += 1;
        self.nodes += output.nodes.len();
        self.truncated += usize::from(output.truncated);
        for (row, counts) in output.reached.modes.iter().enumerate() {
            for (column, n) in counts.iter().enumerate() {
                self.modes[row][column] += u64::from(*n);
            }
        }
        for (state, n) in output.reached.states.iter().enumerate() {
            self.states[state] += u64::from(*n);
        }
        for e in &output.errors {
            self.errors[e.ty as usize] += 1;
        }
    }

    pub fn merge(&mut self, other: &Tally) {
        self.cases += other.cases;
        self.nodes += other.nodes;
        self.truncated += other.truncated;
        for (a, b) in self.modes.iter_mut().zip(&other.modes) {
            for (x, y) in a.iter_mut().zip(b) {
                *x += y;
            }
        }
        for (x, y) in self.states.iter_mut().zip(&other.states) {
            *x += y;
        }
        for (x, y) in self.errors.iter_mut().zip(&other.errors) {
            *x += y;
        }
    }

    /// One line per insertion mode with its tokens by type, then the states and error kinds never reached.
    pub fn report(&self) -> String {
        let mut out = format!("{} parses, {} nodes, {} stopped at a limit\n", self.cases, self.nodes, self.truncated);
        for (row, counts) in self.modes.iter().enumerate() {
            let tokens: Vec<String> =
                counts.iter().enumerate().filter(|(_, n)| **n > 0).map(|(t, n)| format!("{} {n}", TOKEN_NAMES[t])).collect();
            out += &format!("  {}: {}\n", MODE_NAMES[row], if tokens.is_empty() { "none".to_string() } else { tokens.join(", ") });
        }
        let states = self.states.iter().filter(|n| **n > 0).count();
        out += &format!("  tokenizer states reached: {states} of {}; unreached: {:?}\n", self.states.len(), self.unreached_states());
        let errors: Vec<String> = self.errors.iter().enumerate().filter(|(_, n)| **n > 0).map(|(k, n)| format!("{k}:{n}")).collect();
        out += &format!("  error kinds: {}; unreached: {:?}\n", errors.join(" "), self.unreached_errors());
        out
    }

    pub fn unreached_modes(&self) -> Vec<&'static str> {
        self.modes.iter().enumerate().filter(|(_, c)| c.iter().all(|n| *n == 0)).map(|(m, _)| MODE_NAMES[m]).collect()
    }

    pub fn unreached_states(&self) -> Vec<usize> {
        self.states.iter().enumerate().filter(|(_, n)| **n == 0).map(|(s, _)| s).collect()
    }

    pub fn unreached_errors(&self) -> Vec<usize> {
        self.errors.iter().enumerate().filter(|(_, n)| **n == 0).map(|(k, _)| k).collect()
    }
}

// ---- The corpus ---------------------------------------------------------------------------------

/// widget-sandbox's corpus: every `corpus/**/*.html`, by path.
pub fn corpus_pages() -> Vec<(String, Vec<u8>)> {
    let root = super::widget_sandbox().join("corpus");
    let mut files: Vec<PathBuf> = Vec::new();
    super::html_files(&root, &mut files);
    files
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read(&path).ok()?;
            Some((path.strip_prefix(&root).unwrap_or(&path).display().to_string(), text))
        })
        .collect()
}

const VOID_TAGS: [&[u8]; 14] =
    [b"area", b"base", b"br", b"col", b"embed", b"hr", b"img", b"input", b"link", b"meta", b"param", b"source", b"track", b"wbr"];

fn alpha(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

/// core/src/css.cpp's `is_tag_name`.
fn is_tag_name(name: &[u8]) -> bool {
    !name.is_empty() && name.len() <= 64 && alpha(name[0]) && name.iter().all(|c| alpha(*c) || c.is_ascii_digit() || *c == b'-')
}

/// core/src/css.cpp's `is_attribute_name`.
fn is_attribute_name(name: &[u8]) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && (alpha(name[0]) || name[0] == b'_' || name[0] == b':')
        && name.iter().all(|c| alpha(*c) || c.is_ascii_digit() || matches!(c, b'_' | b':' | b'.' | b'-'))
}

fn escape_text(s: &[u8], out: &mut Vec<u8>) {
    for &c in s {
        match c {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            _ => out.push(c),
        }
    }
}

fn escape_attr(s: &[u8], out: &mut Vec<u8>) {
    for &c in s {
        match c {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            _ => out.push(c),
        }
    }
}

/// An element of a DOM as the renderer's shadow tree holds one.
#[derive(Clone, Debug, Default)]
pub struct Dom {
    pub tag: Vec<u8>,
    pub svg: bool,
    pub attrs: Vec<(Vec<u8>, Vec<u8>)>,
    /// Text children are `Err(text)`.
    pub children: Vec<Result<Dom, Vec<u8>>>,
}

/// The HTML `Renderer::serialize` writes for a DOM: the doctype, `data-twid` first on each element, the
/// widget's attributes quoted, text and attributes escaped, void elements without an end, the newline a
/// textarea's value starts with doubled, `<script>` and `<template>` dropped, and nothing below level 128.
pub fn serialize(html: &Dom, next_id: &mut u32) -> Vec<u8> {
    fn node(n: &Dom, out: &mut Vec<u8>, level: u32, next_id: &mut u32) {
        if n.tag == b"script" || n.tag == b"template" || level > 128 || !is_tag_name(&n.tag) {
            return;
        }
        out.push(b'<');
        out.extend_from_slice(&n.tag);
        out.extend_from_slice(format!(" data-twid=\"{next_id}\"").as_bytes());
        *next_id += 1;
        for (name, value) in &n.attrs {
            if name.eq_ignore_ascii_case(b"data-twid") || name.eq_ignore_ascii_case(b"data-lh-id") || !is_attribute_name(name) {
                continue;
            }
            out.push(b' ');
            out.extend_from_slice(name);
            out.extend_from_slice(b"=\"");
            escape_attr(value, out);
            out.push(b'"');
        }
        out.push(b'>');
        if !n.svg && VOID_TAGS.contains(&&n.tag[..]) {
            return;
        }
        for c in &n.children {
            match c {
                Ok(e) => node(e, out, level + 1, next_id),
                Err(t) if n.tag == b"style" => out.extend_from_slice(t),
                Err(t) => {
                    if n.tag == b"textarea" && t.first() == Some(&b'\n') {
                        out.push(b'\n');
                    }
                    escape_text(t, out);
                }
            }
        }
        out.extend_from_slice(b"</");
        out.extend_from_slice(&n.tag);
        out.push(b'>');
    }
    let mut out = b"<!DOCTYPE html><html".to_vec();
    out.extend_from_slice(format!(" data-twid=\"{next_id}\">").as_bytes());
    *next_id += 1;
    for e in html.children.iter().flatten() {
        node(e, &mut out, 2, next_id);
    }
    out.extend_from_slice(b"</html>");
    out
}

/// A parse's tree as a [`Dom`], comments, doctypes and whitespace-only text between elements kept as text.
pub fn dom_of(output: &GumboOutput) -> Dom {
    fn convert(output: &GumboOutput, id: NodeId) -> Dom {
        let node = &output.nodes[id];
        let e = node.element();
        let mut tag = gumbo_normalized_tagname(e.tag).to_vec();
        if tag.is_empty() {
            tag = b"x-unknown".to_vec();
        }
        let mut dom = Dom { tag, svg: e.tag_namespace == GUMBO_NAMESPACE_SVG, attrs: Vec::new(), children: Vec::new() };
        for a in &e.attributes {
            dom.attrs.push((c_str(&a.name).to_vec(), c_str(&a.value).to_vec()));
        }
        for &c in &e.children {
            let child = &output.nodes[c];
            match child.ty {
                GUMBO_NODE_ELEMENT | GUMBO_NODE_TEMPLATE => dom.children.push(Ok(convert(output, c))),
                GUMBO_NODE_TEXT | GUMBO_NODE_WHITESPACE | GUMBO_NODE_CDATA => dom.children.push(Err(c_str(&child.text().text).to_vec())),
                _ => {}
            }
        }
        dom
    }
    match output.root {
        Some(root) => convert(output, root),
        None => Dom { tag: b"html".to_vec(), ..Dom::default() },
    }
}

/// What the renderer would serialise for a widget whose DOM is its markup as parsed: its approximation of a
/// corpus snapshot (a widget's scripts change the DOM after that).
pub fn snapshot(text: &[u8]) -> Vec<u8> {
    let output = gumbo_parse_with_options(&kGumboDefaultOptions, text);
    let mut next_id = 2;
    serialize(&dom_of(&output), &mut next_id)
}

// ---- Generated HTML -----------------------------------------------------------------------------

/// Every tag name Gumbo knows.
pub fn tag_names() -> Vec<&'static [u8]> {
    TAG_NAMES[..GUMBO_TAG_UNKNOWN as usize].to_vec()
}

const EXTRA_TAGS: [&str; 24] = [
    "foo",
    "x-widget",
    "my-el",
    "d\u{ed}v",
    "DIV",
    "Svg",
    "foreignObject",
    "clipPath",
    "linearGradient",
    "feGaussianBlur",
    "annotation-xml",
    "mi",
    "mglyph",
    "malignmark",
    "image",
    "isindex",
    "keygen",
    "nobr",
    "rtc",
    "search",
    "dialog",
    "menuitem",
    "TABLE",
    "a1",
];

fn tag_name(random: &mut Lcg) -> Vec<u8> {
    if random.chance(4) {
        return pick(random, &EXTRA_TAGS).as_bytes().to_vec();
    }
    let names = tag_names();
    let name = names[random.below(names.len())].to_vec();
    if random.chance(10) { name.to_ascii_uppercase() } else { name }
}

const ATTR_NAMES: [&str; 30] = [
    "class",
    "id",
    "style",
    "data-twid",
    "data-lh-id",
    "href",
    "src",
    "type",
    "encoding",
    "xmlns",
    "xmlns:xlink",
    "xlink:href",
    "xlink:title",
    "xml:lang",
    "definitionurl",
    "viewbox",
    "viewBox",
    "preserveaspectratio",
    "color",
    "face",
    "size",
    "CLASS",
    "value",
    "selected",
    "x",
    "a-b",
    "_c",
    "\u{e9}",
    "on:x",
    "d",
];

const ATTR_VALUES: [&str; 22] = [
    "",
    "x",
    "hidden",
    "HIDDEN",
    "text/html",
    "application/xhtml+xml",
    "http://www.w3.org/2000/svg",
    "http://www.w3.org/1999/xlink",
    "a b c",
    "0 0 10 10",
    "color: red; width: 10px",
    "&amp;",
    "&amp",
    "&ampx",
    "&amp=",
    "&notit;",
    "&#65;",
    "&#x;",
    "&lt;b&gt;",
    "\u{e9}t\u{e9}",
    "a\"b",
    "a'b",
];

fn attribute(random: &mut Lcg, s: &mut Vec<u8>) {
    s.push(if random.chance(8) { b'\t' } else { b' ' });
    s.extend_from_slice(pick(random, &ATTR_NAMES).as_bytes());
    if random.chance(5) {
        return;
    }
    if random.chance(8) {
        s.push(b' ');
    }
    s.push(b'=');
    if random.chance(8) {
        s.push(b' ');
    }
    let value = if random.chance(6) { any_text(random, 12) } else { pick(random, &ATTR_VALUES).as_bytes().to_vec() };
    match random.below(6) {
        0 => {
            s.push(b'\'');
            s.extend_from_slice(&value);
            s.push(b'\'');
        }
        1 => s.extend(value.iter().filter(|c| !matches!(c, b' ' | b'\t' | b'\n' | b'>'))),
        _ => {
            s.push(b'"');
            s.extend(value.iter().filter(|c| **c != b'"'));
            s.push(b'"');
        }
    }
}

fn start_tag(random: &mut Lcg, s: &mut Vec<u8>, name: &[u8]) {
    s.push(b'<');
    s.extend_from_slice(name);
    let n = if random.chance(3) { random.below(5) } else { 0 };
    for _ in 0..n {
        attribute(random, s);
    }
    if random.chance(12) {
        s.extend_from_slice(pick(random, &["/", " /", "/x", " ", "\n", "\r\n", "\0"]).as_bytes());
    }
    s.push(b'>');
}

fn end_tag(s: &mut Vec<u8>, name: &[u8]) {
    s.extend_from_slice(b"</");
    s.extend_from_slice(name);
    s.push(b'>');
}

const TEXT_PIECES: [&[u8]; 40] = [
    b"text",
    b"word ",
    b" ",
    b"\n",
    b"\r\n",
    b"\r",
    b"\t",
    b"\x0c",
    b"\x0b",
    b"\0",
    b"\xc3\xa9",
    b"\xe2\x82\xac",
    b"\xf0\x9f\x98\x80",
    b"\xc3",
    b"\xff",
    b"\xed\xa0\x80",
    b"\xef\xbf\xbe",
    b"\xef\xb7\x90",
    b"\x7f",
    b"\xc2\x85",
    b"\xe0\x80\xaf",
    b"\xf4\x90\x80\x80",
    b"&amp;",
    b"&amp",
    b"&lt",
    b"&notin;",
    b"&noti",
    b"&NotEqualTilde;",
    b"&#65;",
    b"&#x1F600;",
    b"&#X41",
    b"&#0;",
    b"&#128;",
    b"&#xD800;",
    b"&#x110000;",
    b"&#99999999999;",
    b"&#11;",
    b"&bogus;",
    b"&;",
    b"& ",
];

fn any_text(random: &mut Lcg, most: usize) -> Vec<u8> {
    let mut s = Vec::new();
    for _ in 0..1 + random.below(most) {
        s.extend_from_slice(TEXT_PIECES[random.below(TEXT_PIECES.len())]);
    }
    s
}

const DOCTYPES: [&str; 26] = [
    "<!DOCTYPE html>",
    "<!doctype HTML>",
    "<!DOCTYPE>",
    "<!DOCTYPE html PUBLIC \"-//W3C//DTD HTML 4.01//EN\" \"http://www.w3.org/TR/html4/strict.dtd\">",
    "<!DOCTYPE html PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\">",
    "<!DOCTYPE html PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\" \"x\">",
    "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\" 'x'>",
    "<!DOCTYPE html PUBLIC \"-//IETF//DTD HTML//\">",
    "<!DOCTYPE html PUBLIC \"HTML\">",
    "<!DOCTYPE html SYSTEM \"about:legacy-compat\">",
    "<!DOCTYPE html SYSTEM \"http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd\">",
    "<!DOCTYPE svg>",
    "<!DOCTYPE html PUBLIC>",
    "<!DOCTYPE html PUBLICx>",
    "<!DOCTYPE html SYSTEM'x'y>",
    "<!DOCTYPE html \"x\">",
    "<!DOCTYPE\0x>",
    "<!DOCTYPE html PUBLIC \"a\"'b'>",
    "<!DOCTYPE html PUBLIC 'x'>",
    "<!DOCTYPE html PUBLIC 'x\0y' \"z\">",
    "<!DOCTYPE html PUBLIC \"x>",
    "<!DOCTYPE html PUBLIC >",
    "<!DOCTYPE html SYSTEM >",
    "<!DOCTYPE html SYSTEM \"x>",
    "<!DOCTYPE html PUBLIC \"x\" \"y\" z>",
    "<!DOCTYPE html PUBLIC \"x\"\"y\">",
];

/// Texts a document ends with that the end of input cuts: each state's own end.
const ENDINGS: [&str; 30] = [
    "<a b='x",
    "<a b=\"x",
    "<a b=x",
    "<a b=\"x\"",
    "<a b",
    "<a b=",
    "<a /",
    "<a",
    "</a",
    "</",
    "<",
    "<!--",
    "<!---",
    "<!-- x -",
    "<!-- x --",
    "<!-- x --!",
    "<!x",
    "<!DOCTYPE",
    "<!DOCTYPE ",
    "<!DOCTYPE html",
    "<!DOCTYPE html ",
    "<!DOCTYPE html PUBLIC",
    "<!DOCTYPE html PUBLIC ",
    "<!DOCTYPE html PUBLIC 'x",
    "<!DOCTYPE html PUBLIC \"x\"",
    "<!DOCTYPE html PUBLIC \"x\" ",
    "<!DOCTYPE html SYSTEM",
    "<!DOCTYPE html SYSTEM 'x",
    "<!DOCTYPE html SYSTEM \"x\" ",
    "<svg><![CDATA[x",
];

const ODDITIES: [&str; 28] = [
    "<!-- c -->",
    "<!---->",
    "<!-->",
    "<!--->",
    "<!-- a -- b -->",
    "<!-- a --!>",
    "<!-- a --!-->",
    "<!-- a ---->",
    "<!--\0-->",
    "<!-- open",
    "<?php x ?>",
    "</>",
    "</ x>",
    "</3>",
    "<!x>",
    "<![CDATA[x]]>",
    "< b>",
    "<3",
    "<",
    "</",
    "</a",
    "<a b",
    "<a b=",
    "<a b=\"",
    "<a b='x'c>",
    "<a b c=d e='f'/>",
    "<a b b b=1>",
    "<a\0b>",
];

fn rawtext(random: &mut Lcg, s: &mut Vec<u8>) {
    let tag = pick(random, &["style", "script", "title", "textarea", "xmp", "iframe", "noembed", "noframes", "noscript", "plaintext"]);
    start_tag(random, s, tag.as_bytes());
    for _ in 0..1 + random.below(4) {
        let body: &[u8] = match random.below(12) {
            0 => b"</sty",
            1 => b"<!--",
            2 => b"<!--<script>",
            3 => b"</script >",
            4 => b"-->",
            5 => b"<script>",
            6 => b"</scrip",
            7 => b"<b>&amp;</b>",
            8 => b"\0",
            9 => b"\n",
            10 => b"--",
            _ => b"a<b",
        };
        s.extend_from_slice(body);
        if random.chance(3) {
            s.extend_from_slice(&any_text(random, 3));
        }
    }
    if !random.chance(6) {
        let name = if random.chance(8) { tag.to_ascii_uppercase() } else { tag.to_string() };
        end_tag(s, name.as_bytes());
    }
}

fn table(random: &mut Lcg, s: &mut Vec<u8>) {
    let parts = [
        "<table>",
        "<tr>",
        "<td>",
        "<th>",
        "<tbody>",
        "<thead>",
        "<tfoot>",
        "<caption>",
        "<colgroup>",
        "<col>",
        "</td>",
        "</tr>",
        "</table>",
        "</tbody>",
        "</caption>",
        "</colgroup>",
        "x",
        " ",
        "<div>",
        "<b>",
        "<input type=hidden>",
        "<form>",
        "<select>",
        "<option>",
        "<template>",
        "</template>",
        "<style></style>",
        "<!-- c -->",
        "</th>",
        "</thead>",
    ];
    for _ in 0..2 + random.below(12) {
        s.extend_from_slice(pick(random, &parts).as_bytes());
    }
}

fn foreign(random: &mut Lcg, s: &mut Vec<u8>) {
    let parts = [
        "<svg>",
        "<svg viewbox='0 0 1 1' xlink:href=x>",
        "<math definitionurl=x>",
        "<g>",
        "<path d='M0'/>",
        "<foreignObject>",
        "<foreignobject>",
        "<desc>",
        "<title>",
        "<mi>",
        "<mtext>",
        "<mglyph>",
        "<annotation-xml encoding='text/html'>",
        "<annotation-xml>",
        "<clippath>",
        "<b>",
        "<p>",
        "<font color=red>",
        "<font>",
        "</p>",
        "</br>",
        "<![CDATA[x<y]]>",
        "<![CDATA[",
        "\0",
        "x",
        "</g>",
        "</svg>",
        "</math>",
        "</foreignObject>",
        "<div>",
        "<table>",
        "<svg/>",
        "</G >",
    ];
    for _ in 0..2 + random.below(10) {
        s.extend_from_slice(pick(random, &parts).as_bytes());
    }
}

fn formatting(random: &mut Lcg, s: &mut Vec<u8>) {
    let tags = ["b", "i", "a", "nobr", "em", "strong", "font", "u", "s", "small", "code", "tt", "big", "strike"];
    let blocks = ["div", "p", "table", "blockquote", "address", "td", "li", "h1", "pre", "applet", "marquee", "object"];
    for _ in 0..2 + random.below(14) {
        match random.below(5) {
            0 | 1 => {
                let name = pick(random, &tags);
                if random.chance(2) {
                    s.extend_from_slice(format!("<{name} data-twid=\"{}\">", random.below(9)).as_bytes());
                } else {
                    s.extend_from_slice(format!("<{name}>").as_bytes());
                }
            }
            2 => s.extend_from_slice(format!("</{}>", pick(random, &tags)).as_bytes()),
            3 => s.extend_from_slice(format!("<{}>", pick(random, &blocks)).as_bytes()),
            _ => s.extend_from_slice(if random.chance(2) { format!("</{}>", pick(random, &blocks)) } else { "x".to_string() }.as_bytes()),
        }
    }
}

fn structure(random: &mut Lcg, s: &mut Vec<u8>) {
    let parts = [
        "<html lang=en>",
        "<head>",
        "</head>",
        "<body class=b>",
        "</body>",
        "</html>",
        "<frameset>",
        "<frame>",
        "</frameset>",
        "<noframes>x</noframes>",
        "<template>",
        "</template>",
        "<select>",
        "<option>",
        "<optgroup>",
        "</optgroup>",
        "</option>",
        "</select>",
        "<hr>",
        "<input>",
        "<textarea>",
        "<keygen>",
        "<form>",
        "</form>",
        "<li>",
        "<dd>",
        "<dt>",
        "<ul>",
        "<ol>",
        "<ruby>",
        "<rb>",
        "<rt>",
        "<rp>",
        "<rtc>",
        "<button>",
        "</button>",
        "<image>",
        "<isindex>",
        "<base>",
        "<meta>",
        "<link>",
        "<noscript>",
        "</noscript>",
        "<p>",
        "</p>",
        "</br>",
        "</li>",
        "</dd>",
        "<h2>",
        "</h3>",
        "<main>",
        "</main>",
        "<plaintext>",
        "<listing>",
        "<param>",
        "<area>",
        "<wbr>",
        "<col>",
        "<head>",
        "<!DOCTYPE html>",
    ];
    for _ in 0..1 + random.below(10) {
        s.extend_from_slice(pick(random, &parts).as_bytes());
        if random.chance(4) {
            s.extend_from_slice(&any_text(random, 2));
        }
    }
}

/// HTML of every kind the tokenizer and the tree builder meet: tag soup, misnesting, tables, foreign content,
/// templates, framesets, raw text, references, NULs, invalid UTF-8.
pub fn any_html(random: &mut Lcg) -> Vec<u8> {
    let mut s = Vec::new();
    if random.chance(4) {
        s.extend_from_slice(pick(random, &DOCTYPES).as_bytes());
    }
    let most = if random.chance(6) { 80 } else { 16 };
    let n = 1 + random.below(most);
    for _ in 0..n {
        match random.below(22) {
            0..=4 => {
                let name = tag_name(random);
                start_tag(random, &mut s, &name);
            }
            5..=7 => {
                let name = tag_name(random);
                end_tag(&mut s, &name);
            }
            8..=10 => s.extend_from_slice(&any_text(random, 6)),
            11 => s.extend_from_slice(pick(random, &ODDITIES).as_bytes()),
            12 | 13 => rawtext(random, &mut s),
            14 | 15 => table(random, &mut s),
            16 | 17 => foreign(random, &mut s),
            18 | 19 => formatting(random, &mut s),
            20 => structure(random, &mut s),
            _ => s.extend_from_slice(pick(random, &DOCTYPES).as_bytes()),
        }
    }
    if random.chance(8) {
        s.extend_from_slice(pick(random, &ENDINGS).as_bytes());
    }
    s
}

const PROFILE_TAGS: [&str; 30] = [
    "div", "span", "p", "a", "b", "i", "em", "strong", "ul", "ol", "li", "img", "br", "hr", "table", "tr", "td", "th", "tbody", "h1", "h2",
    "button", "input", "textarea", "select", "option", "label", "section", "style", "canvas",
];

fn random_dom(random: &mut Lcg, depth: u32, svg: bool) -> Dom {
    let tag = if svg {
        pick(random, &["g", "path", "rect", "circle", "text", "foreignObject", "linearGradient", "stop"]).as_bytes().to_vec()
    } else if random.chance(25) {
        b"svg".to_vec()
    } else {
        pick(random, &PROFILE_TAGS).as_bytes().to_vec()
    };
    let svg = svg || tag == b"svg";
    let mut dom = Dom { tag, svg, attrs: Vec::new(), children: Vec::new() };
    for _ in 0..random.below(3) {
        let name = pick(random, &["class", "style", "id", "href", "src", "type", "value", "viewBox", "d", "data-x"]);
        dom.attrs.push((name.as_bytes().to_vec(), any_text(random, 3)));
    }
    if dom.tag == b"style" {
        dom.children.push(Err(b".a{color:red}\n.b>c{x:\"<\\3c \"}".to_vec()));
        return dom;
    }
    let n = if depth > 7 { 0 } else { random.below(5) };
    for _ in 0..n {
        if random.chance(3) {
            dom.children.push(Err(any_text(random, 3)));
        } else {
            dom.children.push(Ok(random_dom(random, depth + 1, svg)));
        }
    }
    dom
}

/// A random shadow tree serialised as the renderer serialises it.
pub fn renderer_html(random: &mut Lcg) -> Vec<u8> {
    let mut body = Dom { tag: b"body".to_vec(), ..Dom::default() };
    for _ in 0..1 + random.below(6) {
        body.children.push(Ok(random_dom(random, 0, false)));
    }
    let head = Dom { tag: b"head".to_vec(), ..Dom::default() };
    let html = Dom { tag: b"html".to_vec(), children: vec![Ok(head), Ok(body)], ..Dom::default() };
    let mut next_id = 2;
    serialize(&html, &mut next_id)
}

/// core_tests.cpp's "formatting elements the parser opens again": a `<p>` holding `formatting` nested `<b>`
/// with a class of `name` characters, then `blocks` `<div>`s with a text each, as the renderer serialises it.
pub fn formatting_blocks(formatting: usize, blocks: usize, name: usize) -> Vec<u8> {
    let class = "x".repeat(name);
    let mut out =
        String::from("<!DOCTYPE html><html data-twid=\"2\"><head data-twid=\"3\"></head><body data-twid=\"4\"><p data-twid=\"10\">");
    let mut id = 11;
    for _ in 0..formatting {
        out += &format!("<b data-twid=\"{id}\" class=\"{class}\">");
        id += 1;
    }
    for _ in 0..blocks {
        out += &format!("<div data-twid=\"{id}\">x</div>");
        id += 2;
    }
    for _ in 0..formatting {
        out += "</b>";
    }
    out += "</p></body></html>";
    out.into_bytes()
}

/// Every named reference char_ref.c's machine knows, found by walking it: the texts after `&` a transition
/// takes whole (`te = p+1`) with the code points it gives.
pub fn entities() -> Vec<(Vec<u8>, i32, i32)> {
    let mut out = Vec::new();
    let mut stack: Vec<(i32, Vec<u8>)> = vec![(CHAR_REF_START, Vec::new())];
    while let Some((cs, text)) = stack.pop() {
        if text.len() > 40 || cs == 0 {
            continue;
        }
        let state = cs as usize;
        let k0 = i32::from(CHAR_REF_TRANS_KEYS[state << 1]);
        let k1 = i32::from(CHAR_REF_TRANS_KEYS[(state << 1) + 1]);
        let slen = i32::from(CHAR_REF_KEY_SPANS[state]);
        if slen <= 0 {
            continue;
        }
        for ch in k0..=k1 {
            let trans = CHAR_REF_INDICIES[usize::from(CHAR_REF_INDEX_OFFSETS[state]) + (ch - k0) as usize] as usize;
            let target = i32::from(CHAR_REF_TRANS_TARGS[trans]);
            let mut next = text.clone();
            next.push(ch as u8);
            // A transition that leaves the machine ends the text: a whole reference where it takes the last byte.
            let mut leaves = None;
            let acts = CHAR_REF_TRANS_ACTIONS[trans] as usize;
            if acts != 0 {
                for k in 0..CHAR_REF_ACTIONS[acts] as usize {
                    let (action, first, second) = CHAR_REF_ACTION_CODE[CHAR_REF_ACTIONS[acts + 1 + k] as usize];
                    if matches!(action, Action::MATCH_NEXT | Action::MATCH_HERE | Action::MATCH_TE) {
                        leaves = Some((action, first, second));
                        break;
                    }
                }
            }
            match leaves {
                Some((Action::MATCH_NEXT, first, second)) => out.push((next, first, second)),
                Some(_) => {}
                None if target != 0 => stack.push((target, next)),
                None => {}
            }
        }
    }
    out.sort();
    out
}
