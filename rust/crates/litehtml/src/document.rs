//! `document.cpp`, as far as the ported stages need it: a document built from Gumbo's tree as the renderer builds
//! one (`createUpdatableFromString`: `create_node`, the element classes by tag, text split into words and spaces,
//! the build limits, the source template with its holes), and lengths in pixels (`to_pixels`, `cvt_units`). What
//! lengths read of the document (its media features, its root's font size, its container) is a [`Units`] until
//! the cascade is ported.

use std::collections::{BTreeMap, BTreeSet};

use crate::Names;
use crate::Px;
use crate::css_length::{CssLength, calc_evaluate, calc_resolve_units, clamp_length};
use crate::css_selector::DocumentMode;
use crate::css_values::CssUnits;
use crate::cxx;
use crate::document_container::DocumentContainer;
use crate::element::{ElementId, ElementKind, StringMap};
use crate::gumbo::*;
use crate::types::{FontMetrics, MediaFeatures};

/// What `document::to_pixels` reads of its document.
#[derive(Clone, Copy)]
pub struct Units<'a> {
    /// `m_container`.
    pub container: &'a dyn DocumentContainer,
    /// `m_media`: the media features the document last had from its container.
    pub media: MediaFeatures,
    /// `m_root->css().get_font_size()`.
    pub root_font_size: Px,
}

impl std::fmt::Debug for Units<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Units").field("media", &self.media).field("root_font_size", &self.root_font_size).finish()
    }
}

impl Units<'_> {
    /// `document::to_pixels(val, metrics, size)`: a length in px against a font and, for %, a size; capped
    /// at ±2^25 px; 0 for a keyword.
    pub fn to_pixels(&self, val: &CssLength, metrics: &FontMetrics, size: Px) -> Px {
        if val.is_predefined() {
            return Px(0.0);
        }
        let ret = match val.units() {
            CssUnits::css_units_percentage => val.calc_percent(size),
            CssUnits::css_units_calc => {
                let convert = |value: f32, unit: CssUnits| self.to_pixels(&CssLength::new(value, unit), metrics, size).value();
                calc_evaluate(val.calc().map(|c| &**c), size, Some(&convert))
            }
            CssUnits::css_units_em => metrics.font_size * val.val(),

            // https://drafts.csswg.org/css-values-4/#absolute-lengths
            CssUnits::css_units_pt => self.container.pt_to_px(val.val()),
            // 1in = 72pt
            CssUnits::css_units_in => self.container.pt_to_px(val.val() * 72.0),
            // 1pc = (1/6)in = 12pt
            CssUnits::css_units_pc => self.container.pt_to_px(val.val() * 12.0),
            // 1cm = (1/2.54)in = (72/2.54)pt
            CssUnits::css_units_cm => self.container.pt_to_px(val.val() * 72.0 / 2.54),
            CssUnits::css_units_mm => self.container.pt_to_px(val.val() * 72.0 / 2.54 / 10.0),

            CssUnits::css_units_vw => Px(self.media.width.value() * val.val() / 100.0),
            CssUnits::css_units_vh => Px(self.media.height.value() * val.val() / 100.0),
            CssUnits::css_units_vmin => Px(cxx::min_f32(self.media.height.value(), self.media.width.value()) * val.val() / 100.0),
            CssUnits::css_units_vmax => Px(cxx::max_f32(self.media.height.value(), self.media.width.value()) * val.val() / 100.0),
            CssUnits::css_units_rem => Px(self.root_font_size.value() * val.val()),
            CssUnits::css_units_ex => Px(metrics.x_height.value() * val.val()),
            CssUnits::css_units_ch => Px(metrics.ch_width.value() * val.val()),
            _ => Px(val.val()),
        };
        Px(clamp_length(ret.value()))
    }

    /// `document::cvt_units(val, metrics, size)`: the length in px; a % stays one, and a calc keeps only
    /// its % leaves (or becomes px when it has none).
    pub fn cvt_units(&self, val: &mut CssLength, metrics: &FontMetrics, size: Px) {
        if val.is_predefined() {
            return;
        }
        if val.units() == CssUnits::css_units_calc {
            // Every leaf but % in px; with no % left, the length is a plain one.
            let mut px = 0.0;
            let convert = |value: f32, unit: CssUnits| self.to_pixels(&CssLength::new(value, unit), metrics, size).value();
            let expression = calc_resolve_units(val.calc(), &convert, &mut px);
            match expression {
                None => val.set_value(clamp_length(px), CssUnits::css_units_px),
                Some(expression) => val.set_calc(Some(expression)),
            }
        } else if val.units() != CssUnits::css_units_percentage {
            let px = self.to_pixels(val, metrics, size).value();
            val.set_value(px, CssUnits::css_units_px);
        }
    }
}

// ---- The document as Gumbo's tree builds it ---------------------------------------------------------------

/// What building a document asks besides the document: the name table and the container.
#[derive(Clone, Copy)]
pub struct Env<'a> {
    pub names: &'a Names,
    pub container: &'a dyn DocumentContainer,
}

impl std::fmt::Debug for Env<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Env").finish_non_exhaustive()
    }
}

/// `css_text`: a style sheet's text as an element brought it, with its base url and media.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CssText {
    pub text: Vec<u8>,
    pub baseurl: Vec<u8>,
    pub media: Vec<u8>,
}

/// `html_hole`: a place in the source a later frame may change without a new parse: a quoted `style`, `class`
/// (HTML), `src` (`<img>`, `<canvas>`) or SVG geometry attribute value, or a text node.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HtmlHole {
    pub text: bool,
    pub src: bool,
    pub cls: bool,
    /// The Gumbo node it is in.
    pub node: Option<NodeId>,
    /// The attribute's name.
    pub name: Vec<u8>,
    /// The attribute's name for an SVG geometry attribute, else empty.
    pub attr: Vec<u8>,
    /// The quote that ends an attribute's value.
    pub close: u8,
    /// The source text the hole holds.
    pub value: Vec<u8>,
    /// The element an attribute belongs to.
    pub owner: Option<ElementId>,
    /// The elements a text node became.
    pub text_elements: Vec<ElementId>,
}

/// `max_build_depth`: levels of elements built, `<html>` the first.
pub const max_build_depth: i32 = 128;
/// `max_build_elements`.
pub const max_build_elements: i32 = 20000;
/// `max_generated_boxes`, `max_generated_box`, `max_generated_bytes`: what generated content may make.
pub const max_generated_boxes: i32 = 20000;
pub const max_generated_box: usize = 32 * 1024;
pub const max_generated_bytes: usize = 1024 * 1024;

/// `document`: its elements (an arena; an element no parent keeps stays in it), the root, the mode, the parsed
/// text and Gumbo's tree, the map from Gumbo's nodes to the elements they became, the source template, and what
/// its elements brought (style sheets, generated content).
#[derive(Clone, Debug, Default)]
pub struct Document {
    pub elements: Vec<crate::element::Element>,
    /// `m_root`.
    pub root: Option<ElementId>,
    /// `m_mode`.
    pub mode: DocumentMode,
    /// `m_text`: what Gumbo parsed, which the tree's string pieces point into.
    pub text: Vec<u8>,
    /// `m_gumbo`: the tree an updatable document keeps.
    pub gumbo: Option<GumboOutput>,
    /// `m_track_nodes`: while set, `create_node` fills `node_elements`.
    pub track_nodes: bool,
    pub build_depth: i32,
    pub build_elements: i32,
    /// `m_build_limited`: Gumbo or the build stopped at a limit.
    pub build_limited: bool,
    /// `m_node_elements`: each Gumbo element node the element it became, each text node its words and spaces.
    pub node_elements: BTreeMap<NodeId, Vec<ElementId>>,
    /// `m_has_svg`: an `<svg>` was made.
    pub has_svg: bool,
    /// `m_css`: the style sheets `<style>` and `<link>` brought.
    pub css: Vec<CssText>,
    /// `m_literals`, `m_holes`, `m_template_ok`, `m_node_holes`: the source as literals around holes.
    pub literals: Vec<Vec<u8>>,
    pub holes: Vec<HtmlHole>,
    pub template_ok: bool,
    pub node_holes: BTreeMap<NodeId, Vec<usize>>,
    /// Holes whose spans started where another's did (a clone's attributes share the source of the element Gumbo
    /// cloned it from), which the C++'s `std::sort` orders as its library does; here they keep the tree's order. No
    /// counterpart in the C++, for the comparison.
    pub template_ties: usize,
    /// `m_lang`, `m_culture`: empty until `lang_changed`.
    pub lang: Vec<u8>,
    pub culture: Vec<u8>,
    pub generated_boxes: i32,
    pub generated_bytes: usize,
    pub generated_limited: bool,
}

/// `svg_geometry_attribute(name)`: the SVG attributes whose values are holes (unless a selector names them).
pub fn svg_geometry_attribute(name: &[u8]) -> bool {
    const NAMES: [&[u8]; 32] = [
        b"points",
        b"d",
        b"x",
        b"y",
        b"x1",
        b"y1",
        b"x2",
        b"y2",
        b"cx",
        b"cy",
        b"r",
        b"rx",
        b"ry",
        b"dx",
        b"dy",
        b"transform",
        b"fill",
        b"stroke",
        b"stroke-width",
        b"stroke-dasharray",
        b"stroke-dashoffset",
        b"opacity",
        b"fill-opacity",
        b"stroke-opacity",
        b"offset",
        b"stop-color",
        b"stop-opacity",
        b"text-anchor",
        b"width",
        b"height",
        b"display",
        b"visibility",
    ];
    NAMES.contains(&c_str(name))
}

impl Document {
    /// `document(container)`.
    pub fn new() -> Document {
        Document::default()
    }

    /// `set_document_mode(mode)`.
    pub fn set_document_mode(&mut self, mode: DocumentMode) {
        self.mode = mode;
    }

    /// `createUpdatableFromString(html, container, master_styles)`, as far as the DOM goes: [`Document::build`],
    /// the DOM's part of `finalize_from_external_root` ([`Document::finalize_dom`]; the master sheet, the
    /// document's sheets, the cascade and the render tree are later stages'), then the template.
    pub fn create_updatable_from_string(html: &[u8], env: &Env) -> Document {
        let (mut doc, root) = Document::build(html, env);
        doc.finalize_dom(env, root);
        doc.build_template();
        doc
    }

    /// `createUpdatableFromString` up to `finalize_from_external_root`: Gumbo's parse of the text (kept), the mode
    /// from its doctype, and the elements with Gumbo's nodes mapped to them; the root is the last element the
    /// `<html>` node gave.
    pub fn build(html: &[u8], env: &Env) -> (Document, Option<ElementId>) {
        let mut doc = Document::new();
        doc.text = html.to_vec();
        let mut output = gumbo_parse_with_options(&kGumboDefaultOptions, &doc.text);
        if output.truncated {
            env.container.sanitizer_limit(b"elements");
            doc.build_limited = true;
        }
        doc.mode = match output.node(output.document).document().doc_type_quirks_mode {
            GUMBO_DOCTYPE_QUIRKS => DocumentMode::quirks_mode,
            GUMBO_DOCTYPE_LIMITED_QUIRKS => DocumentMode::limited_quirks_mode,
            _ => DocumentMode::no_quirks_mode,
        };
        doc.track_nodes = true;
        let mut root_elements = Vec::new();
        if let Some(root) = output.root {
            doc.create_node(env, &mut output, root, &mut root_elements, true, true);
        }
        doc.track_nodes = false;
        let root = root_elements.last().copied();
        doc.gumbo = Some(output);
        (doc, root)
    }

    /// What `finalize_from_external_root` does to the DOM before the cascade: the root, `:root` on it, and
    /// `parse_attributes` from it.
    pub fn finalize_dom(&mut self, env: &Env, root: Option<ElementId>) {
        self.root = root;
        if let Some(root) = root {
            self.set_pseudo_class(root, crate::string_id::_root_, true);
            self.parse_attributes(env, root);
        }
    }

    /// `create_node(gnode, elements, parseTextNode, process_root)`: the elements a Gumbo node becomes, added to
    /// `elements`. An element node becomes the class its tag names (past the depth and element limits nothing,
    /// and the container may refuse it) with its children appended as its class takes them; without
    /// `process_root` only its children are made. Text becomes words and spaces (one text inside a script),
    /// whitespace one space per byte; templates and the document make nothing. An unknown tag's original text
    /// is cut to its name in the tree, as `gumbo_tag_from_original_text` does in place.
    pub fn create_node(
        &mut self,
        env: &Env,
        output: &mut GumboOutput,
        node: NodeId,
        elements: &mut Vec<ElementId>,
        mut parse_text_node: bool,
        process_root: bool,
    ) {
        match output.node(node).ty {
            GUMBO_NODE_ELEMENT => {
                let children = output.node(node).element().children.clone();
                if !process_root {
                    for child in children {
                        self.create_node(env, output, child, elements, parse_text_node, true);
                    }
                    return;
                }
                let mut attrs = StringMap::new();
                for attr in &output.node(node).element().attributes {
                    attrs.insert(c_str(&attr.name).to_vec(), c_str(&attr.value).to_vec());
                }
                let element = output.node(node).element();
                let tag = gumbo_normalized_tagname(element.tag);
                let svg = element.tag_namespace == GUMBO_NAMESPACE_SVG;
                let mut name = Vec::new();
                if !tag.is_empty() {
                    name = tag.to_vec();
                } else if element.original_tag.data.is_some() && element.original_tag.length != 0 {
                    let mut piece = element.original_tag;
                    gumbo_tag_from_original_text(&self.text, &mut piece);
                    output.nodes[node].element_mut().original_tag = piece;
                    name = piece.bytes(&self.text).to_vec();
                }
                let mut ret = None;
                if !name.is_empty() {
                    if self.build_depth >= max_build_depth || self.build_elements >= max_build_elements {
                        env.container.sanitizer_limit(if self.build_depth >= max_build_depth { b"depth" } else { b"elements" });
                        self.build_limited = true;
                        return;
                    }
                    if !env.container.sanitize_element(&mut name, svg, &mut attrs) {
                        return;
                    }
                    ret = Some(self.create_element(env, c_str(&name), &attrs));
                    self.build_elements += 1;
                }
                if tag == b"script" {
                    parse_text_node = false;
                }
                if let Some(ret) = ret {
                    if self.track_nodes {
                        self.node_elements.insert(node, vec![ret]);
                    }
                    let mut child = Vec::new();
                    self.build_depth += 1;
                    for gchild in children {
                        child.clear();
                        self.create_node(env, output, gchild, &mut child, parse_text_node, true);
                        for &el in &child {
                            self.append_child(ret, el);
                        }
                    }
                    self.build_depth -= 1;
                    elements.push(ret);
                }
            }
            GUMBO_NODE_TEXT => {
                let before = elements.len();
                let text = c_str(&output.node(node).text().text).to_vec();
                if !parse_text_node {
                    elements.push(self.new_text(&text, false));
                } else {
                    env.container.split_text(&text, &mut |piece, space| elements.push(self.new_text(piece, space)));
                }
                if self.track_nodes {
                    self.node_elements.insert(node, elements[before..].to_vec());
                }
            }
            GUMBO_NODE_CDATA | GUMBO_NODE_COMMENT => {
                let ret = self.new_element(if output.node(node).ty == GUMBO_NODE_CDATA {
                    ElementKind::el_cdata
                } else {
                    ElementKind::el_comment
                });
                self.set_data(ret, &output.node(node).text().text);
                elements.push(ret);
            }
            GUMBO_NODE_WHITESPACE => {
                let str = c_str(&output.node(node).text().text).to_vec();
                for i in 0..str.len() {
                    elements.push(self.new_text(&str[i..i + 1], true));
                }
            }
            _ => {}
        }
    }

    /// `create_element(tag_name, attributes)`: the class the tag names (the container's `create_element`, which
    /// both containers answer with nothing, is not asked), its tag and its attributes in name order.
    pub fn create_element(&mut self, env: &Env, tag_name: &[u8], attributes: &StringMap) -> ElementId {
        let kind = match tag_name {
            b"br" => ElementKind::el_break,
            b"p" => ElementKind::el_para,
            b"img" | b"canvas" => ElementKind::el_image,
            b"table" => ElementKind::el_table,
            b"td" | b"th" => ElementKind::el_td,
            b"link" => ElementKind::el_link,
            b"title" => ElementKind::el_title,
            b"a" => ElementKind::el_anchor,
            b"tr" => ElementKind::el_tr,
            b"style" => ElementKind::el_style,
            b"base" => ElementKind::el_base,
            b"body" => ElementKind::el_body,
            b"div" => ElementKind::el_div,
            b"script" => ElementKind::el_script,
            b"font" => ElementKind::el_font,
            b"svg" => ElementKind::el_svg,
            b"input" => ElementKind::el_input,
            b"select" => ElementKind::el_select,
            b"button" => ElementKind::el_button,
            b"textarea" => ElementKind::el_textarea,
            b"fieldset" => ElementKind::el_fieldset,
            b"progress" | b"meter" => ElementKind::el_meter,
            _ => ElementKind::html_tag,
        };
        let id = self.new_element(kind);
        self.set_tag_name(env.names, id, tag_name);
        for (name, value) in attributes {
            self.set_attr(env.names, id, name, value);
        }
        id
    }

    /// `add_stylesheet(str, baseurl, media)`: a sheet with text (up to its first NUL) is kept.
    pub fn add_stylesheet(&mut self, str: &[u8], baseurl: Option<&[u8]>, media: Option<&[u8]>) {
        let text = c_str(str);
        if !text.is_empty() {
            self.css.push(CssText {
                text: text.to_vec(),
                baseurl: baseurl.map(|b| c_str(b).to_vec()).unwrap_or_default(),
                media: media.map(|m| c_str(m).to_vec()).unwrap_or_default(),
            });
        }
    }

    /// `take_generated(bytes)`: room for one more generated box of `bytes`, or the content limit (reported once).
    pub fn take_generated(&mut self, env: &Env, bytes: usize) -> bool {
        if self.generated_limited {
            return false;
        }
        if self.generated_boxes >= max_generated_boxes || bytes > max_generated_box || bytes > max_generated_bytes - self.generated_bytes {
            self.generated_limited = true;
            env.container.sanitizer_limit(b"content");
            return false;
        }
        self.generated_boxes += 1;
        self.generated_bytes += bytes;
        true
    }

    /// `release_generated(boxes, bytes)`.
    pub fn release_generated(&mut self, boxes: i32, bytes: usize) {
        self.generated_boxes -= boxes.min(self.generated_boxes);
        self.generated_bytes -= bytes.min(self.generated_bytes);
    }

    /// `build_template()`: the source as literals around holes, from the tree the document keeps. Every quoted
    /// `style`, HTML `class`, `<img>`/`<canvas>` `src` and SVG geometry attribute (but those a `[name` in the text
    /// may select) of an element that was made is a hole, and every text node outside raw text (style, script,
    /// title, noscript, a textarea whose content starts with a newline) that ends before a `<`. Any other shape
    /// leaves the document without a template: a hole's attribute unquoted or its element not one, a text node that
    /// made nothing or runs to the end. Holes that overlap stop it where they meet, the literals and holes so far
    /// kept.
    pub fn build_template(&mut self) {
        self.template_ok = false;
        self.literals.clear();
        self.holes.clear();
        self.node_holes.clear();
        self.template_ties = 0;
        let Some(output) = &self.gumbo else { return };
        let base = &self.text[..];
        let size = base.len();
        let mut selected = BTreeSet::new();
        let mut p = base.iter().position(|c| *c == b'[');
        while let Some(at) = p {
            let mut q = at + 1;
            while q < size && (base[q].is_ascii_alphanumeric() || base[q] == b'-' || base[q] == b'_' || base[q] == b':') {
                q += 1;
            }
            if q > at + 1 {
                selected.insert(base[at + 1..q].to_vec());
            }
            if q >= size - 1 {
                break;
            }
            p = base[at + 1..].iter().position(|c| *c == b'[').map(|k| at + 1 + k);
        }
        let mut walk = TemplateWalk { base, output, node_elements: &self.node_elements, selected: &selected, spans: Vec::new(), ok: true };
        walk.walk(output.document, false);
        if !walk.ok {
            return;
        }
        let mut spans = walk.spans;
        spans.sort_by_key(|s| s.0);
        self.template_ties = spans.windows(2).filter(|w| w[0].0 == w[1].0).count();
        let mut pos = 0;
        for (start, end, hole) in spans {
            if start < pos {
                return;
            }
            self.literals.push(self.text[pos..start].to_vec());
            self.holes.push(hole);
            pos = end;
        }
        self.literals.push(self.text[pos..].to_vec());
        for (i, hole) in self.holes.iter().enumerate() {
            if let Some(node) = hole.node {
                self.node_holes.entry(node).or_default().push(i);
            }
        }
        self.template_ok = true;
    }
}

/// `build_template`'s walk over the tree: the spans of the holes, or not ok.
struct TemplateWalk<'a> {
    base: &'a [u8],
    output: &'a GumboOutput,
    node_elements: &'a BTreeMap<NodeId, Vec<ElementId>>,
    selected: &'a BTreeSet<Vec<u8>>,
    spans: Vec<(usize, usize, HtmlHole)>,
    ok: bool,
}

impl TemplateWalk<'_> {
    fn walk(&mut self, id: NodeId, raw: bool) {
        if !self.ok {
            return;
        }
        let node = self.output.node(id);
        let size = self.base.len();
        match node.ty {
            GUMBO_NODE_DOCUMENT => {
                for &child in &node.document().children {
                    self.walk(child, false);
                }
            }
            GUMBO_NODE_ELEMENT | GUMBO_NODE_TEMPLATE => {
                let e = node.element();
                let Some(made) = self.node_elements.get(&id) else { return };
                for attr in &e.attributes {
                    let src = (e.tag == GUMBO_TAG_IMG || e.tag == GUMBO_TAG_CANVAS) && strcmp(&attr.name, b"src");
                    let geometry = !src
                        && e.tag_namespace == GUMBO_NAMESPACE_SVG
                        && e.tag != GUMBO_TAG_SVG
                        && svg_geometry_attribute(&attr.name)
                        && !self.selected.contains(c_str(&attr.name));
                    let cls = !src && !geometry && e.tag_namespace == GUMBO_NAMESPACE_HTML && strcmp(&attr.name, b"class");
                    if !src && !geometry && !cls && !strcmp(&attr.name, b"style") {
                        continue;
                    }
                    let v = attr.original_value;
                    let quote = v.data.and_then(|at| self.base.get(at)).copied().unwrap_or(0);
                    let Some(data) =
                        v.data.filter(|&at| v.length >= 2 && (quote == b'"' || quote == b'\'') && at + v.length <= size && made.len() == 1)
                    else {
                        if cls {
                            continue;
                        }
                        self.ok = false;
                        return;
                    };
                    let (start, end) = (data + 1, data + v.length - 1);
                    let hole = HtmlHole {
                        close: quote,
                        src,
                        cls,
                        attr: if geometry { c_str(&attr.name).to_vec() } else { Vec::new() },
                        owner: Some(made[0]),
                        node: Some(id),
                        name: c_str(&attr.name).to_vec(),
                        value: self.base[start..end].to_vec(),
                        ..HtmlHole::default()
                    };
                    self.spans.push((start, end, hole));
                }
                let content = e.original_tag.data.map(|at| at + e.original_tag.length);
                let textarea_newline = e.tag == GUMBO_TAG_TEXTAREA
                    && content.is_some_and(|at| at < size && (self.base[at] == b'\n' || self.base[at] == b'\r'));
                let child_raw = e.tag == GUMBO_TAG_STYLE
                    || e.tag == GUMBO_TAG_SCRIPT
                    || e.tag == GUMBO_TAG_TITLE
                    || textarea_newline
                    || e.tag == GUMBO_TAG_NOSCRIPT;
                for &child in &e.children {
                    self.walk(child, child_raw);
                }
            }
            GUMBO_NODE_TEXT => {
                if raw {
                    return;
                }
                let t = node.text().original_text;
                let made = self.node_elements.get(&id);
                let Some(data) = t.data.filter(|&at| t.length != 0 && at + t.length <= size && made.is_some_and(|m| !m.is_empty())) else {
                    self.ok = false;
                    return;
                };
                let (start, end) = (data, data + t.length);
                let hole = HtmlHole {
                    text: true,
                    node: Some(id),
                    text_elements: made.cloned().unwrap_or_default(),
                    value: self.base[start..end].to_vec(),
                    ..HtmlHole::default()
                };
                if end >= size || self.base[end] != b'<' {
                    self.ok = false;
                    return;
                }
                self.spans.push((start, end, hole));
            }
            _ => {}
        }
    }
}
