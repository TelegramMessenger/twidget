//! lhdump's `i` and `I` records (oracle/lhdump_dom.h): the fork's DOM as the renderer's documents build it, and
//! the declarations its attributes add to each tag's style.
//!
//! `i` is `u8 flags` (bit 0: parse the attributes, as `finalize_from_external_root` does after the master sheet)
//! and the html. The document is built as `createUpdatableFromString` builds it up to `finalize_from_external_root`
//! (Gumbo's parse, `create_node` with the node map, the attributes when asked), then `build_template`. The answer,
//! status 1:
//!
//! - `u8` mode, `u8` Gumbo's truncated, `u8 m_build_limited`, `i32 m_build_elements`, `i32 m_build_depth`, `u8`
//!   has_svg, `i32` the root's index;
//! - `u32` element count and each element, numbered in tree order from the root (an `el_style`'s own list after its
//!   children), then those the node map holds that the tree does not reach, in Gumbo's order: `u8` class, `i32`
//!   parent (-1 none), `tag()`, `get_tagName()`, `u8` each of is_text, is_space, is_comment, is_body, is_break,
//!   is_replaced, `u8` display; an `html_tag` its id, `u32` str_classes and each, `u32` classes and each name,
//!   `u32` attributes as names and values, `u32` pseudo-classes of root, hover, active, link; then `get_text`, a
//!   script's `m_text`, `u32` children and each index, a style's `u32` own children and each index, an image's
//!   src and `u8` loaded, an input's type, `u8` checked, `f32` fraction and its swatch colour;
//! - `u32` node map entries in Gumbo's pre-order: `u32` node, `u32` count and each element index;
//! - `u8` template ok, `u32` literals and each, `u32` holes and each: `u8` text, src, cls, `u32` node, name, attr,
//!   `u8` close, value, `i32` owner, `u32` text elements and each index; `u32` node holes entries (`u32` node,
//!   `u32` count and each hole index);
//! - `u32` style sheets the elements brought, each text, base url, media;
//! - `u32` container calls in order, each `u8` kind (`L` sanitizer_limit, `B` set_base_url, `C` set_caption, `K`
//!   link, `M` import_css), `u8` NULL, two strings, `i32` element index (`K`; else -1).
//!
//! `I` is `u8` flags, the html and `u32` declarations, each `u32` element index (as `i` numbers it), `u8` form (0
//! text, 1 tokens), the property's name, the text or `u32` tokens and each, the base url, `u8` important and `u8`
//! with the container. lhdump builds the document with its attributes parsed and answers `u32` tags and, for each
//! tag in order, `u8` 1 when the style the fork's `parse_attributes` gave it equals an empty style with the
//! declarations for it added in order through the fork's `style::add_property`.

use std::cell::RefCell;
use std::collections::BTreeMap;

use twidget_litehtml::css_tokenizer::CssToken;
use twidget_litehtml::document::{Document, Env};
use twidget_litehtml::document_container::DocumentContainer;
use twidget_litehtml::element::{AttributeValue, ElementId, ElementKind, ElementState};
use twidget_litehtml::gumbo::{GumboOutput, NodeId};
use twidget_litehtml::{Names, Px, string_id};

use super::{In, Lcg, Out, imported, pick};

/// What lhdump's logging container records.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub kind: u8,
    pub null: bool,
    pub a: Vec<u8>,
    pub b: Vec<u8>,
    pub el: Option<ElementId>,
}

/// lhdump's `LoggingContainer<OracleContainer>`.
#[derive(Debug, Default)]
pub struct DomContainer {
    pub calls: RefCell<Vec<Call>>,
}

impl DomContainer {
    fn log(&self, kind: u8, null: bool, a: &[u8], b: &[u8], el: Option<ElementId>) {
        self.calls.borrow_mut().push(Call { kind, null, a: a.to_vec(), b: b.to_vec(), el });
    }
}

impl twidget_litehtml::web_color::ResolveColor for DomContainer {
    fn resolve_color(&self, _name: &[u8]) -> Vec<u8> {
        Vec::new()
    }
}

impl DocumentContainer for DomContainer {
    fn pt_to_px(&self, pt: f32) -> Px {
        Px(pt * 96.0 / 72.0)
    }
    fn get_default_font_size(&self) -> Px {
        Px(16.0)
    }
    fn get_media_features(&self) -> twidget_litehtml::types::MediaFeatures {
        super::document_media()
    }
    fn import_css(&self, url: &[u8], baseurl: &mut Vec<u8>) -> Vec<u8> {
        let before = baseurl.clone();
        let text = match imported(url) {
            Some(text) => {
                *baseurl = [&b"imported/"[..], url].concat();
                text.to_vec()
            }
            None => Vec::new(),
        };
        self.log(b'M', false, url, &before, None);
        text
    }
    fn sanitizer_limit(&self, what: &[u8]) {
        self.log(b'L', false, what, b"", None);
    }
    fn set_base_url(&self, base_url: Option<&[u8]>) {
        self.log(b'B', base_url.is_none(), base_url.unwrap_or(b""), b"", None);
    }
    fn set_caption(&self, caption: &[u8]) {
        self.log(b'C', false, caption, b"", None);
    }
    fn link(&self, el: ElementId) {
        self.log(b'K', false, b"", b"", Some(el));
    }
}

/// A document built as lhdump's `build` builds one, with what its container was asked.
pub struct Built {
    pub doc: Document,
    pub root: Option<ElementId>,
    pub calls: Vec<Call>,
}

pub fn build(names: &Names, html: &[u8], parse_attributes: bool) -> Built {
    let container = DomContainer::default();
    let env = Env { names, container: &container };
    let (mut doc, root) = Document::build(html, &env);
    if parse_attributes && let Some(root) = root {
        doc.parse_attributes(&env, root);
    }
    doc.build_template();
    Built { doc, root, calls: container.calls.into_inner() }
}

/// Gumbo's nodes in pre-order from the document.
pub fn gumbo_order(output: &GumboOutput) -> Vec<NodeId> {
    let mut order = Vec::new();
    let mut stack = vec![output.document];
    while let Some(node) = stack.pop() {
        order.push(node);
        if let Some(children) = output.node(node).children() {
            stack.extend(children.iter().rev());
        }
    }
    order
}

/// The elements as the dump numbers them.
pub struct Walk {
    pub order: Vec<ElementId>,
    pub index: BTreeMap<ElementId, i32>,
}

impl Walk {
    fn visit(&mut self, doc: &Document, el: ElementId) {
        if self.index.contains_key(&el) {
            return;
        }
        self.index.insert(el, self.order.len() as i32);
        self.order.push(el);
        for &child in doc.children(el) {
            self.visit(doc, child);
        }
        if doc.kind(el) == ElementKind::el_style {
            for &child in &doc.el(el).own_children {
                self.visit(doc, child);
            }
        }
    }

    pub fn of(&self, el: Option<ElementId>) -> i32 {
        match el {
            None => -1,
            Some(el) => self.index.get(&el).copied().unwrap_or(-2),
        }
    }
}

pub fn walk(doc: &Document, root: Option<ElementId>, nodes: &[NodeId]) -> Walk {
    let mut w = Walk { order: Vec::new(), index: BTreeMap::new() };
    if let Some(root) = root {
        w.visit(doc, root);
    }
    for node in nodes {
        if let Some(made) = doc.node_elements.get(node) {
            for &el in made {
                w.visit(doc, el);
            }
        }
    }
    w
}

fn names_out(o: &mut Out, names: &Names, ids: &[string_id::StringId]) {
    o.u32(ids.len() as u32);
    for &id in ids {
        o.str(&names.s(id));
    }
}

fn element_out(o: &mut Out, names: &Names, doc: &Document, w: &Walk, id: ElementId) {
    let el = doc.el(id);
    o.u8(el.kind as u8);
    o.i32(w.of(el.parent));
    o.str(&names.s(doc.tag(id)));
    o.str(&doc.get_tag_name(names, id));
    for flag in [doc.is_text(id), doc.is_space(id), doc.is_comment(id), doc.is_body(id), doc.is_break(id), doc.is_replaced(id)] {
        o.u8(u8::from(flag));
    }
    o.u8(el.css.m_display as u8);
    if el.kind.is_html_tag() {
        o.str(&names.s(el.id));
        o.u32(el.str_classes.len() as u32);
        for cls in &el.str_classes {
            o.str(cls);
        }
        names_out(o, names, &el.classes);
        o.u32(el.attrs.len() as u32);
        for (name, value) in &el.attrs {
            o.str(name).str(value);
        }
        let pseudo: Vec<_> = [&b"root"[..], b"hover", b"active", b"link"]
            .into_iter()
            .map(|n| names.id(n))
            .filter(|cls| el.pseudo_classes.contains(cls))
            .collect();
        names_out(o, names, &pseudo);
    }
    let mut text = Vec::new();
    doc.get_text(id, &mut text);
    o.str(&text);
    if el.kind == ElementKind::el_script {
        o.str(&el.text);
    }
    o.u32(el.children.len() as u32);
    for &child in &el.children {
        o.i32(w.of(Some(child)));
    }
    if el.kind == ElementKind::el_style {
        o.u32(el.own_children.len() as u32);
        for &child in &el.own_children {
            o.i32(w.of(Some(child)));
        }
    }
    match &el.state {
        ElementState::Image { src, loaded } => {
            o.str(src).u8(u8::from(*loaded));
        }
        ElementState::Input { ty, checked, fraction, swatch } => {
            o.str(ty).u8(u8::from(*checked)).f32(*fraction).color(swatch);
        }
        _ => {}
    }
}

/// The template's literals, holes and node holes.
fn holes_out(o: &mut Out, doc: &Document, w: &Walk, node_of: &dyn Fn(Option<NodeId>) -> u32, nodes: &[NodeId]) {
    o.u32(doc.literals.len() as u32);
    for literal in &doc.literals {
        o.str(literal);
    }
    o.u32(doc.holes.len() as u32);
    for hole in &doc.holes {
        o.u8(u8::from(hole.text)).u8(u8::from(hole.src)).u8(u8::from(hole.cls)).u32(node_of(hole.node));
        o.str(&hole.name).str(&hole.attr).u8(hole.close).str(&hole.value).i32(w.of(hole.owner));
        o.u32(hole.text_elements.len() as u32);
        for &el in &hole.text_elements {
            o.i32(w.of(Some(el)));
        }
    }
    let holed: Vec<NodeId> = nodes.iter().copied().filter(|n| doc.node_holes.contains_key(n)).collect();
    o.u32(holed.len() as u32);
    for node in holed {
        let holes = &doc.node_holes[&node];
        o.u32(node_of(Some(node))).u32(holes.len() as u32);
        for &h in holes {
            o.u32(h as u32);
        }
    }
}

/// What the Rust side answers to an `i` record, and the document it built.
pub fn dom_answer(names: &Names, input: &[u8]) -> ((u8, Vec<u8>), Built) {
    let mut i = In::new(input);
    let flags = i.u8();
    let parse_attributes = flags & 1 != 0;
    let html = i.rest();
    let built = build(names, &html, parse_attributes);
    let doc = &built.doc;
    let output = doc.gumbo.as_ref().expect("the tree is kept");
    let nodes = gumbo_order(output);
    let node_index: BTreeMap<NodeId, u32> = nodes.iter().enumerate().map(|(k, &n)| (n, k as u32)).collect();
    let node_of = |node: Option<NodeId>| node.and_then(|n| node_index.get(&n).copied()).unwrap_or(u32::MAX);
    let w = walk(doc, built.root, &nodes);

    let mut o = Out::default();
    o.u8(doc.mode as u8).u8(u8::from(output.truncated)).u8(u8::from(doc.build_limited));
    o.i32(doc.build_elements).i32(doc.build_depth).u8(u8::from(doc.has_svg)).i32(w.of(built.root));
    o.u32(w.order.len() as u32);
    for &el in &w.order {
        element_out(&mut o, names, doc, &w, el);
    }
    let mapped: Vec<NodeId> = nodes.iter().copied().filter(|n| doc.node_elements.contains_key(n)).collect();
    o.u32(mapped.len() as u32);
    for node in mapped {
        let made = &doc.node_elements[&node];
        o.u32(node_of(Some(node))).u32(made.len() as u32);
        for &el in made {
            o.i32(w.of(Some(el)));
        }
    }
    o.u8(u8::from(doc.template_ok));
    if flags & 2 == 0 {
        holes_out(&mut o, doc, &w, &node_of, &nodes);
    }
    o.u32(doc.css.len() as u32);
    for sheet in &doc.css {
        o.str(&sheet.text).str(&sheet.baseurl).str(&sheet.media);
    }
    o.u32(built.calls.len() as u32);
    for call in &built.calls {
        o.u8(call.kind).u8(u8::from(call.null)).str(&call.a).str(&call.b);
        o.i32(if call.kind == b'K' { w.of(call.el) } else { -1 });
    }
    ((1, o.take()), built)
}

/// An `i` record; without `holes`, the answer leaves out the literals and holes.
pub fn dom_record(parse_attributes: bool, holes: bool, html: &[u8]) -> Vec<u8> {
    let mut o = Out::default();
    o.u8(u8::from(parse_attributes) | if holes { 0 } else { 2 });
    o.s.extend_from_slice(html);
    o.take()
}

/// An `I` record of the declarations the Rust side's attributes gave, and the answer that says they are the fork's.
pub fn replay_case(names: &Names, html: &[u8]) -> (Vec<u8>, (u8, Vec<u8>), usize) {
    let built = build(names, html, true);
    let doc = &built.doc;
    let nodes = gumbo_order(doc.gumbo.as_ref().expect("the tree is kept"));
    let w = walk(doc, built.root, &nodes);
    let mut entries = Out::default();
    let mut count = 0;
    let mut tags = 0;
    for (k, &el) in w.order.iter().enumerate() {
        if doc.kind(el).is_html_tag() {
            tags += 1;
        }
        for d in &doc.el(el).declarations {
            count += 1;
            entries.u32(k as u32);
            match &d.value {
                AttributeValue::Text(text) => {
                    entries.u8(0).str(&names.s(d.name)).str(text);
                }
                AttributeValue::Tokens(tokens) => {
                    entries.u8(1).str(&names.s(d.name)).u32(tokens.len() as u32);
                    for t in tokens {
                        entries.token(t);
                    }
                }
            }
            entries.str(&d.baseurl).u8(u8::from(d.important)).u8(u8::from(d.with_container));
        }
    }
    let mut o = Out::default();
    o.u8(1).str(html).u32(count as u32);
    o.s.extend_from_slice(&entries.s);
    let mut answer = Out::default();
    answer.u32(tags);
    for _ in 0..tags {
        answer.u8(1);
    }
    (o.take(), (1, answer.take()), count)
}

/// What the DOM cases reached: elements by class, holes by kind, limits, declarations by property, input
/// types, container calls, templates made and refused.
#[derive(Debug, Default)]
pub struct DomTally {
    pub counts: BTreeMap<String, usize>,
}

impl DomTally {
    pub fn add(&mut self, names: &Names, built: &Built) {
        let doc = &built.doc;
        let mut bump = |k: String| *self.counts.entry(k).or_default() += 1;
        for (k, el) in doc.elements.iter().enumerate() {
            bump(format!("class {:?}", el.kind));
            for d in &el.declarations {
                bump(format!("declares {}", String::from_utf8_lossy(&names.s(d.name))));
            }
            if let ElementState::Input { ty, .. } = &el.state
                && !ty.is_empty()
            {
                bump(format!("input {}", String::from_utf8_lossy(ty)));
            }
            if el.master_sheet {
                bump("generated box with the master sheet".into());
            }
            if el.parent.is_none() && Some(ElementId(k as u32)) != built.root {
                bump("elements no parent keeps".into());
            }
        }
        bump(if doc.template_ok { "template made".into() } else { "template refused".into() });
        for hole in &doc.holes {
            let kind = if hole.text {
                "text"
            } else if hole.src {
                "src"
            } else if hole.cls {
                "class"
            } else if !hole.attr.is_empty() {
                "svg geometry"
            } else {
                "style"
            };
            bump(format!("hole {kind}"));
        }
        for call in &built.calls {
            bump(format!(
                "call {}{}",
                char::from(call.kind),
                if call.kind == b'L' { String::from_utf8_lossy(&call.a).into_owned() } else { String::new() }
            ));
        }
        if !doc.css.is_empty() {
            bump("documents with sheets".into());
        }
        if doc.gumbo.as_ref().is_some_and(|g| g.truncated) {
            bump("Gumbo truncated".into());
        }
        if doc.mode != Default::default() {
            bump(format!("mode {:?}", doc.mode));
        }
    }

    pub fn report(&self) -> String {
        self.counts.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", ")
    }
}

// ---- Generators ---------------------------------------------------------------------------------------------

fn attr_value(random: &mut Lcg) -> String {
    pick(
        random,
        &[
            "",
            "0",
            "1",
            "3",
            "+2",
            "-1",
            "+9",
            "-9",
            "7",
            " 4",
            "12px",
            "50%",
            "0.5",
            "1.25%",
            "00",
            "0.00001",
            "2147483648",
            "99999999999",
            "-0",
            " 12",
            "3.",
            "3.x",
            "4e2",
            "0x10",
            "abc",
            "red",
            "#f00",
            "rgb(1,2,3)",
            "center",
            "left",
            "top",
            "a b",
            "  x  ",
            "\"q\"",
            "url(x)",
            "2024-01-05",
            "2024-13-05",
            " 024-01-05",
            "+202-01-05",
            "2024-1-5x",
            "2024-01-5 ",
            "0000-01-01",
            "a.css",
            "nested.css",
            "none.css",
            "stylesheet",
            "Stylesheet",
            "print",
            "text",
            "number",
            "password",
            "date",
            "range",
            "color",
            "checkbox",
            "submit",
            "reset",
            "button",
            "hidden",
            "TEXT",
            "RANGE",
            "1e40",
            "nan",
            "-5",
            "150",
            "1000",
            "1001",
            "é",
            "日本",
            "x\u{1}y",
        ],
    )
    .to_string()
}

const DOM_ATTRS: [&str; 34] = [
    "id",
    "class",
    "style",
    "align",
    "valign",
    "width",
    "height",
    "bgcolor",
    "background",
    "border",
    "cellspacing",
    "color",
    "face",
    "size",
    "src",
    "href",
    "rel",
    "media",
    "type",
    "value",
    "checked",
    "selected",
    "placeholder",
    "min",
    "max",
    "font-family",
    "font-weight",
    "font-style",
    "font-size",
    "x",
    "d",
    "fill",
    "lang",
    "data-x",
];

const DOM_TAGS: [&str; 52] = [
    "div",
    "p",
    "span",
    "a",
    "b",
    "font",
    "table",
    "tbody",
    "thead",
    "tfoot",
    "caption",
    "colgroup",
    "tr",
    "td",
    "th",
    "img",
    "canvas",
    "link",
    "base",
    "title",
    "style",
    "script",
    "br",
    "body",
    "input",
    "select",
    "option",
    "optgroup",
    "button",
    "textarea",
    "fieldset",
    "legend",
    "progress",
    "meter",
    "svg",
    "g",
    "text",
    "tspan",
    "textPath",
    "circle",
    "path",
    "foreignObject",
    "math",
    "template",
    "noscript",
    "x-item",
    "lh-input-text",
    "ul",
    "li",
    "pre",
    "form",
    "label",
];

fn dom_element(random: &mut Lcg, depth: u32, out: &mut String) {
    let tag = pick(random, &DOM_TAGS);
    out.push('<');
    out.push_str(tag);
    for _ in 0..random.below(5) {
        let name = pick(random, &DOM_ATTRS);
        let value = attr_value(random);
        match random.below(5) {
            0 => out.push_str(&format!(" {name}")),
            1 => out.push_str(&format!(" {name}='{}'", value.replace('\'', ""))),
            2 => out.push_str(&format!(
                " {name}={}",
                if value.is_empty() || value.contains([' ', '"', '\'']) { "v".to_string() } else { value }
            )),
            _ => out.push_str(&format!(" {name}=\"{}\"", value.replace('"', "&quot;"))),
        }
    }
    out.push('>');
    let n = if depth > 4 { 0 } else { random.below(5) };
    for _ in 0..n {
        match random.below(10) {
            0 => out.push_str(pick(random, &["text", " ", "\n  ", "a b", "x-y z?w", "日本語", "\t", "\u{ff}", "&amp;", "&nbsp;x"])),
            1 => out.push_str("<!-- c -->"),
            2 => out.push_str("<![CDATA[d]]>"),
            3 => out.push_str(pick(random, &["p{}", "[x] {}", "[fill]{}", "@media print{}", "a{color:red}"])),
            _ => dom_element(random, depth + 1, out),
        }
    }
    if random.below(6) != 0 {
        out.push_str(&format!("</{tag}>"));
    }
}

/// A document of every element class and the attributes each reads, in HTML, tables and SVG, with comments, CDATA,
/// raw text and missing end tags; quirks or not.
pub fn any_dom_html(random: &mut Lcg) -> Vec<u8> {
    let mut out = String::new();
    if !random.chance(4) {
        out.push_str("<!DOCTYPE html>");
    }
    if random.chance(2) {
        out.push_str("<html><head>");
        if random.chance(2) {
            out.push_str("<title>T i</title><base href=\"b/\"><link rel=stylesheet href=a.css media=screen><style>[d]{} p{}</style>");
        }
        out.push_str("</head><body>");
    }
    for _ in 0..1 + random.below(4) {
        dom_element(random, 0, &mut out);
    }
    if random.chance(2) {
        out.push_str("</body></html>");
    }
    out.into_bytes()
}

/// Nesting past the build's depth limit, and elements past its element limit (and Gumbo's open elements).
pub fn limit_documents() -> Vec<(Vec<u8>, String)> {
    let mut cases = Vec::new();
    for depth in [126, 127, 128, 129, 130, 200, 401] {
        let mut s = String::from("<!DOCTYPE html><body>");
        for k in 0..depth {
            s.push_str(if k % 3 == 0 { "<div class=a>" } else { "<span>" });
        }
        s.push('x');
        cases.push((s.into_bytes(), format!("nesting {depth}")));
    }
    for count in [19_996, 19_997, 19_998, 20_000, 20_050] {
        let mut s = String::from("<!DOCTYPE html><body>");
        for k in 0..count {
            s.push_str(if k % 2 == 0 { "<i></i>" } else { "<b>t</b>" });
        }
        cases.push((s.into_bytes(), format!("{count} elements")));
    }
    cases
}

/// Every attribute of every class with values of every shape, one element each.
pub fn attribute_documents() -> Vec<Vec<u8>> {
    let mut docs = Vec::new();
    let mut random = Lcg::new(0x13_d0a1);
    for tag in DOM_TAGS {
        for name in DOM_ATTRS {
            let mut s = format!("<!DOCTYPE html><body><{tag}");
            for _ in 0..3 {
                s.push_str(&format!(" {name}=\"{}\"", attr_value(&mut random).replace('"', "")));
            }
            s.push_str(&format!(">t</{tag}><svg><{tag} {name}=\"{}\">u</{tag}></svg>", attr_value(&mut random).replace('"', "")));
            docs.push(s.into_bytes());
        }
    }
    docs
}

/// Every input type with values of every shape for what it reads (dates, numbers, colours, placeholders,
/// sizes, ranges), and selects and text areas with and without what they show.
pub fn control_documents() -> Vec<Vec<u8>> {
    let mut docs = Vec::new();
    let types = [
        "text", "TEXT", "number", "password", "date", "range", "color", "checkbox", "radio", "submit", "reset", "button", "hidden",
        "image", "file", "", "email", "search",
    ];
    let values = [
        None,
        Some(""),
        Some("v"),
        Some("2024-01-05"),
        Some(" 024-01-05"),
        Some("2024-02-31"),
        Some("2024-13-01"),
        Some("0000-01-01"),
        Some("+202-01-05"),
        Some("2024-01-5x"),
        Some("2024- 1- 5"),
        Some("50"),
        Some("-3"),
        Some("1e3"),
        Some("0x10"),
        Some("nan"),
        Some("red"),
        Some("#0f0"),
        Some("rgb(1, 2, 3)"),
        Some("red blue"),
        Some("p\u{e9}ss"),
        Some("12.5"),
        Some("1e39"),
    ];
    let extras = [
        "",
        " size=5",
        " size=0",
        " size=1001",
        " size=7.5",
        " placeholder=hint",
        " placeholder=''",
        " min=10 max=0",
        " min=-5 max=5",
        " max=1e39 min=-1e39",
        " checked",
    ];
    for (k, ty) in types.iter().enumerate() {
        for (m, value) in values.iter().enumerate() {
            let extra = extras[(k + m) % extras.len()];
            let value = value.map(|v| format!(" value=\"{v}\"")).unwrap_or_default();
            docs.push(format!("<!DOCTYPE html><body><input type=\"{ty}\"{value}{extra}><input type={ty}{extra}></body>").into_bytes());
        }
    }
    for options in [
        "",
        "<option>a</option>",
        "<option> b </option><option selected>c<!--x--></option>",
        "<optgroup><option>d</option></optgroup>",
        "<optgroup><option>e</option><option selected> f\n</option></optgroup><option selected>g</option>",
        "<option></option>",
        "<lh-select-value>old</lh-select-value><option>h</option>",
    ] {
        docs.push(
            format!("<!DOCTYPE html><select>{options}</select><svg><select><option selected>s</option><lh-x>y</lh-x></select></svg>")
                .into_bytes(),
        );
    }
    for (attrs, text) in [("", ""), (" placeholder=p", ""), (" placeholder=p", "t"), (" placeholder=''", ""), (" placeholder=p", "\n")] {
        docs.push(
            format!("<!DOCTYPE html><textarea{attrs}>{text}</textarea><svg><textarea{attrs}><lh-a>z</lh-a>{text}</textarea></svg>")
                .into_bytes(),
        );
    }
    docs
}

/// The quirks-mode forms of a few documents: ids and classes lower-case.
pub fn quirks(html: &[u8]) -> Vec<u8> {
    let start = html.windows(15).position(|w| w.eq_ignore_ascii_case(b"<!DOCTYPE html>")).map(|at| at + 15).unwrap_or(0);
    html[start..].to_vec()
}

/// A token list as a test writes it.
pub fn tokens_text(tokens: &[CssToken]) -> String {
    tokens.iter().map(|t| format!("{}{}", t.n.number, String::from_utf8_lossy(&t.str))).collect::<Vec<_>>().join(" ")
}
