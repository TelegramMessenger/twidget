//! `document.cpp`, as far as the ported stages need it: a document built from Gumbo's tree as the renderer builds
//! one (`createUpdatableFromString`: `create_node`, the element classes by tag, text split into words and spaces,
//! the build limits, the source template with its holes), `finalize_from_external_root`'s cascade (the master
//! sheet, the presentational attributes, the document's sheets under their media, the user sheet, then every
//! element's computed style), `media_changed`, the fonts the computed styles ask the container for, style
//! sharing's cache, and lengths in pixels (`to_pixels`, `cvt_units`, through a [`Units`] of what they read of the
//! document).
//!
//! What the C++ keeps per thread or per process for the cascade is explicit: the selector budget's steps and its
//! scopes are the document's (each scope that opens first starts them at 0, so nothing carries over from another
//! document), the `LH_NO_*` switches it reads once per process are a [`Switches`], and the caches of parsed master
//! and document sheets, which give what a parse gives, are not kept.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::Names;
use crate::Px;
use crate::css_length::{CssLength, calc_evaluate, calc_resolve_units, clamp_length};
use crate::css_properties::CssProperties;
use crate::css_selector::DocumentMode;
use crate::css_values::CssUnits;
use crate::cxx;
use crate::document_container::DocumentContainer;
use crate::element::{ElementId, ElementKind, StringMap};
use crate::font_description::FontDescription;
use crate::gumbo::*;
use crate::media_query::{MediaDocument, MediaQueryListList, parse_media_query_list_text};
use crate::style::DeclarationCache;
use crate::stylesheet::{Css, StyleSheetHost, StyleSheetInput};
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

/// The `LH_NO_*` switches the cascade reads, each once per process in the C++ (a function-local static): all on
/// unless the variable is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Switches {
    /// `style_sharing_enabled()`: not `LH_NO_STYLE_SHARING`.
    pub style_sharing: bool,
    /// `ids_selectable()` looks at the sheets: not `LH_NO_ID_SHARE` (which makes ids always selectable).
    pub id_share: bool,
    /// `find_adjacent_sibling`'s index of a long child list while a sheet is applied: not `LH_NO_SIBLING_INDEX`.
    pub sibling_index: bool,
    /// `prune_enabled()`: not `LH_NO_DESCENDANT_PRUNE`.
    pub descendant_prune: bool,
    /// `layout_cache_enabled()`: not `LH_NO_LAYOUT_CACHE`.
    pub layout_cache: bool,
    /// `same_resolved_layout_input` may reuse a fixed flex box's layout: not `LH_NO_FIXED_LAYOUT_REUSE` (read on
    /// every call in the C++).
    pub fixed_layout_reuse: bool,
    /// `render` keeps the positioned boxes it fetched while styles and tree stay: not `LH_NO_POSITIONED_REUSE`.
    pub positioned_reuse: bool,
    /// `layout_cache_check()`: `LH_LAYOUT_CACHE_CHECK`, a cache hit laid out again and compared.
    pub layout_cache_check: bool,
    /// `LHReuseColumnMeasure()`: a column flex item's auto or content basis is the height its min-height measured,
    /// not measured again: not `LH_NO_COLUMN_REUSE`.
    pub column_reuse: bool,
    /// `draw_group` culls a transformed group's content against the clip mapped into it: not
    /// `LH_NO_TRANSFORMED_CULLING`.
    pub transformed_culling: bool,
}

impl Default for Switches {
    fn default() -> Switches {
        Switches {
            style_sharing: true,
            id_share: true,
            sibling_index: true,
            descendant_prune: true,
            layout_cache: true,
            fixed_layout_reuse: true,
            positioned_reuse: true,
            layout_cache_check: false,
            column_reuse: true,
            transformed_culling: true,
        }
    }
}

/// What the cascade asks besides the document: the name table, the container, the declaration cache `style::add`
/// keeps of style texts (the process's), and the switches.
#[derive(Clone, Copy)]
pub struct StyleCx<'a> {
    pub names: &'a Names,
    pub container: &'a dyn DocumentContainer,
    pub cache: &'a DeclarationCache,
    pub switches: Switches,
}

impl std::fmt::Debug for StyleCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StyleCx").field("switches", &self.switches).finish_non_exhaustive()
    }
}

impl<'a> StyleCx<'a> {
    pub fn env(&self) -> Env<'a> {
        Env { names: self.names, container: self.container }
    }
}

/// `font_item`: a font the container made and its metrics.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FontItem {
    pub font: u64,
    pub metrics: FontMetrics,
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
/// `max_table_cells`: the cells a document's tables may make, spans and filled-out rows included.
pub const max_table_cells: i32 = 200000;

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
    /// `m_master_css` (sheet 0), `m_styles` (1), `m_user_css` (2).
    pub master_css: Css,
    pub styles: Css,
    pub user_css: Css,
    /// `m_media`: the media features the document last had from its container.
    pub media: MediaFeatures,
    /// `m_media_lists`: every media list a sheet registered, each once.
    pub media_lists: Vec<Arc<MediaQueryListList>>,
    /// `m_finalized`.
    pub finalized: bool,
    /// `m_fonts`: the fonts by their description's hash.
    pub fonts: RefCell<BTreeMap<Vec<u8>, FontItem>>,
    /// `m_share_cache` and `m_next_share_id`: computed styles by sharing key.
    pub share_cache: BTreeMap<Vec<u8>, CssProperties>,
    pub next_share_id: i32,
    /// `m_ids_selectable`: not looked at yet, or whether a sheet selects by id.
    pub ids_selectable: Cell<Option<bool>>,
    /// `has_backdrop`: an element has a backdrop filter.
    pub has_backdrop: Cell<bool>,
    /// The selector budget: `g_selector_steps`, `g_apply_depth` and `g_budget_depth`.
    pub selector_steps: Cell<i64>,
    pub apply_depth: Cell<i32>,
    pub budget_depth: Cell<i32>,
    /// The render tree (`m_root_render`, `m_tabular_elements`, the table cell budget).
    pub render: crate::render_item::RenderTree,
    /// Slots of `elements` the render tree's anonymous elements left, to be used again by the next ones.
    pub free_elements: Vec<ElementId>,
    /// What layout keeps of the document.
    pub layout: crate::layout::DocLayout,
    /// What painting keeps of the document, and what the renderer sets for a paint (stage L13).
    pub paint: crate::paint::DocPaint,
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
        Document { styles: Css::with_sheet(1), user_css: Css::with_sheet(2), ..Document::default() }
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

/// The document as `css::parse_css_stylesheet` reads it: its container, its media and root font size as they are,
/// its mode; the media lists the sheet registers are kept for the document to add.
struct SheetHost<'a> {
    doc: &'a Document,
    cx: &'a StyleCx<'a>,
    lists: RefCell<Vec<Arc<MediaQueryListList>>>,
}

impl StyleSheetHost for SheetHost<'_> {
    fn names(&self) -> &Names {
        self.cx.names
    }
    fn container(&self) -> &dyn DocumentContainer {
        self.cx.container
    }
    fn media_document(&self) -> MediaDocument<'_> {
        MediaDocument { names: self.cx.names, units: self.doc.units(self.cx) }
    }
    fn mode(&self) -> DocumentMode {
        self.doc.mode
    }
    fn add_media_list(&self, list: &Arc<MediaQueryListList>) {
        self.lists.borrow_mut().push(Arc::clone(list));
    }
}

impl Document {
    /// What `to_pixels` and `cvt_units` read of the document now: its container, its media and its root's font
    /// size.
    pub fn units<'a>(&self, cx: &StyleCx<'a>) -> Units<'a> {
        let root_font_size = self.root.map_or(Px(0.0), |root| self.el(root).css.get_font_size());
        Units { container: cx.container, media: self.media, root_font_size }
    }

    /// `add_media_list(list)`: a list is kept once.
    pub fn add_media_list(&mut self, list: &Arc<MediaQueryListList>) {
        if !self.media_lists.iter().any(|l| Arc::ptr_eq(l, list)) {
            self.media_lists.push(Arc::clone(list));
        }
    }

    fn sheet_mut(&mut self, sheet: i32) -> &mut Css {
        match sheet {
            0 => &mut self.master_css,
            1 => &mut self.styles,
            _ => &mut self.user_css,
        }
    }

    /// `css::parse_css_stylesheet(text, baseurl, doc, media)` into one of the document's sheets (0 the master, 1 the
    /// document's, 2 the user's), registering the media lists it meets.
    fn parse_sheet(&mut self, cx: &StyleCx, sheet: i32, text: &[u8], baseurl: &[u8], media: Option<&Arc<MediaQueryListList>>) {
        let mut css = std::mem::take(self.sheet_mut(sheet));
        let host = SheetHost { doc: self, cx, lists: RefCell::new(Vec::new()) };
        css.parse_css_stylesheet(StyleSheetInput::Text(text), baseurl, &host, media, true);
        let lists = host.lists.into_inner();
        for list in &lists {
            self.add_media_list(list);
        }
        *self.sheet_mut(sheet) = css;
    }

    /// `createUpdatableFromString(html, container, master_styles)`: the document built ([`Document::build`]),
    /// finalized with its cascade, and its template.
    pub fn create_updatable(html: &[u8], master_styles: &[u8], cx: &StyleCx) -> Document {
        let (mut doc, root) = Document::build(html, &cx.env());
        doc.finalize_from_external_root(cx, root, master_styles, b"");
        doc.build_template();
        doc
    }

    /// `finalize_from_external_root(root, master_styles, user_styles)`, once: the master sheet (in quirks mode with
    /// tables' fonts reset) and the user sheet parsed as trusted, then for a root: the media features, `:root`, the
    /// master sheet applied, the attributes parsed, the document's sheets parsed under their media, the media
    /// applied, the document's and the user's sheets applied, every element's computed style, and the render tree
    /// ([`Document::create_render_tree`]).
    pub fn finalize_from_external_root(&mut self, cx: &StyleCx, root: Option<ElementId>, base_master_styles: &[u8], user_styles: &[u8]) {
        self.invalidate_paint();
        if self.finalized {
            return;
        }
        self.finalized = true;

        self.root = root;

        let master_styles = if self.mode == DocumentMode::quirks_mode && !base_master_styles.is_empty() {
            [base_master_styles, b"\ntable { font-size: medium; font-weight: normal; font-style: normal; }\n"].concat()
        } else {
            base_master_styles.to_vec()
        };
        if !master_styles.is_empty() {
            cx.container.trusted_styles(true);
            self.parse_sheet(cx, 0, &master_styles, b"", None);
            cx.container.trusted_styles(false);
            self.master_css.sort_selectors();
        }
        if !user_styles.is_empty() {
            cx.container.trusted_styles(true);
            self.parse_sheet(cx, 2, user_styles, b"", None);
            cx.container.trusted_styles(false);
            self.user_css.sort_selectors();
        }

        let Some(root) = root else { return };
        self.media = cx.container.get_media_features();

        self.set_pseudo_class(root, crate::string_id::_root_, true);

        // apply master CSS
        let master = self.master_css.selectors().to_vec();
        self.apply_stylesheet(cx, root, &master);

        // parse elements attributes
        self.parse_attributes(&cx.env(), root);
        self.flush_declarations(cx);

        // parse style sheets linked in document
        for css in self.css.clone() {
            let media = if css.media.is_empty() {
                None
            } else {
                let mq_list = parse_media_query_list_text(&css.media, &MediaDocument { names: cx.names, units: self.units(cx) });
                let mut list = MediaQueryListList::default();
                list.add(mq_list);
                Some(Arc::new(list))
            };
            self.parse_sheet(cx, 1, &css.text, &css.baseurl, media.as_ref());
        }
        // Sort css selectors using CSS rules.
        self.styles.sort_selectors();

        // Apply media features.
        let media = self.media;
        self.update_media_lists(cx, &media);

        // Apply parsed styles.
        let styles = self.styles.selectors().to_vec();
        self.apply_stylesheet(cx, root, &styles);

        // Apply user styles if any
        let user = self.user_css.selectors().to_vec();
        self.apply_stylesheet(cx, root, &user);

        // Initialize element::m_css
        self.compute_styles(cx, root, true);

        // Create rendering tree
        self.create_render_tree(cx);
    }

    /// The declarations `parse_attributes` recorded, added to each element's style in their order, and the master
    /// sheet applied to the boxes the form controls made as the C++ applies it then (nothing reads a style in
    /// between, and the boxes' matching reads nothing those calls change).
    pub fn flush_declarations(&mut self, cx: &StyleCx) {
        for index in 0..self.elements.len() {
            let el = &mut self.elements[index];
            let pending = el.declarations[el.declarations_applied..].to_vec();
            el.declarations_applied = el.declarations.len();
            for d in pending {
                let container = if d.with_container { Some(cx.container) } else { None };
                let style = &mut self.elements[index].style;
                match &d.value {
                    crate::element::AttributeValue::Text(text) => {
                        style.add_property_text(cx.names, d.name, text, &d.baseurl, d.important, container)
                    }
                    crate::element::AttributeValue::Tokens(tokens) => {
                        style.add_property(cx.names, d.name, tokens, &d.baseurl, d.important, container)
                    }
                }
            }
            if self.elements[index].master_sheet && !self.elements[index].master_applied {
                self.elements[index].master_applied = true;
                let master = self.master_css.selectors().to_vec();
                self.apply_stylesheet(cx, ElementId(index as u32), &master);
            }
        }
    }

    /// `update_media_lists(features)`: every list's answer under the features; whether one changed.
    pub fn update_media_lists(&mut self, cx: &StyleCx, features: &MediaFeatures) -> bool {
        self.invalidate_paint();
        let mut update_styles = false;
        for media_list in &self.media_lists {
            if media_list.apply_media_features(cx.names, features) {
                update_styles = true;
            }
        }
        update_styles
    }

    /// `media_changed()`: the container's media features now; when a list's answer changed, every style again and
    /// the render tree made again (without `fix_tables_layout`, its tabular list kept: see
    /// [`Document::recreate_render_tree_on_media_change`]).
    pub fn media_changed(&mut self, cx: &StyleCx) -> bool {
        self.invalidate_paint();
        self.media = cx.container.get_media_features();
        let media = self.media;
        if self.update_media_lists(cx, &media)
            && let Some(root) = self.root
        {
            self.refresh_styles(cx, root);
            self.compute_styles(cx, root, true);
            self.recreate_render_tree_on_media_change(cx);
            return true;
        }
        false
    }

    /// `get_font(descr, fm)`: the font of the description, made by the container the first time (`add_font`), and
    /// its metrics; no font and no metrics for a size of 0.
    pub fn get_font(&self, cx: &StyleCx, descr: &FontDescription) -> (u64, Option<FontMetrics>) {
        if descr.size == Px(0.0) {
            return (0, None);
        }
        let key = descr.hash();
        if let Some(item) = self.fonts.borrow().get(&key) {
            return (item.font, Some(item.metrics));
        }
        let (font, metrics) = cx.container.create_font(descr);
        self.fonts.borrow_mut().insert(key, FontItem { font, metrics });
        (font, Some(metrics))
    }

    /// `next_share_id()`.
    pub fn next_share_id(&mut self) -> i32 {
        self.next_share_id += 1;
        self.next_share_id
    }

    /// `ids_selectable()`: whether a selector of the three sheets reads an id (an id selector, or an `id` attribute
    /// selector in any case, nested lists included); always with `LH_NO_ID_SHARE`. Looked at once.
    pub fn ids_selectable(&self, cx: &StyleCx) -> bool {
        if let Some(seen) = self.ids_selectable.get() {
            return seen;
        }
        fn sees_ids(names: &Names, selector: &crate::css_selector::CssSelector) -> bool {
            let mut sel = Some(selector);
            while let Some(s) = sel {
                for attr in &s.m_right.m_attrs {
                    if attr.ty == crate::css_selector::AttrSelectType::select_id
                        || (attr.ty == crate::css_selector::AttrSelectType::select_attr
                            && crate::html::t_strcasecmp(&names.s(attr.name), b"id") == 0)
                    {
                        return true;
                    }
                    if attr.selector_list.iter().any(|inner| sees_ids(names, inner)) {
                        return true;
                    }
                }
                sel = s.m_left.as_deref();
            }
            false
        }
        let mut seen = !cx.switches.id_share;
        for sheet in [&self.master_css, &self.styles, &self.user_css] {
            for sel in sheet.selectors() {
                if seen {
                    break;
                }
                seen = sees_ids(cx.names, sel);
            }
        }
        self.ids_selectable.set(Some(seen));
        seen
    }
}

// ---- The render tree's table fix-up ------------------------------------------------------------------------

impl Document {
    /// `fix_tables_layout()`: each tabular item in the list (those it adds too, up to eight times the list's length
    /// and 64 more) given the parent and the children its display needs, in anonymous boxes (CSS 2.1 17.2.1 as the
    /// fork does it: a table's children in row groups, a row group's in rows and its parent a table, a row's in cells
    /// and its parent a row group, a cell's parent a row; captions and columns left alone).
    pub fn fix_tables_layout(&mut self, cx: &StyleCx) {
        use crate::render_item::*;
        self.invalidate_paint();
        let limit = self.render.tabular.len() * 8 + 64;
        let mut processed = 0;
        let mut k = 0;
        while k < self.render.tabular.len() {
            processed += 1;
            if processed > limit {
                break;
            }
            let el_ptr = self.render.tabular[k];
            k += 1;
            let d = self.render_display(el_ptr);
            if d == display_inline_table || d == display_table {
                self.fix_table_children(cx, el_ptr, display_table_row_group, b"table-row-group");
            } else if d == display_table_footer_group || d == display_table_row_group || d == display_table_header_group {
                if let Some(parent) = self.render.parent(el_ptr)
                    && self.render_display(parent) != display_inline_table
                {
                    self.fix_table_parent(cx, el_ptr, display_table, b"table");
                }
                self.fix_table_children(cx, el_ptr, display_table_row, b"table-row");
            } else if d == display_table_row {
                self.fix_table_parent(cx, el_ptr, display_table_row_group, b"table-row-group");
                self.fix_table_children(cx, el_ptr, display_table_cell, b"table-cell");
            } else if d == display_table_cell {
                self.fix_table_parent(cx, el_ptr, display_table_row, b"table-row");
            }
        }
    }

    /// The anonymous box of `display:<disp_str>` the fix-up makes for an item of `element`: a cell (its tag
    /// `table_cell`) a `render_item_block` unless `table` (the parent's fix-up) is asked, a table a
    /// `render_item_table` when it is, a row a `render_item_table_row`, anything else a `render_item_table_part`.
    #[inline(never)]
    fn anonymous_table_box(&mut self, cx: &StyleCx, element: ElementId, disp_str: &[u8], table: bool) -> crate::render_item::RenderId {
        use crate::render_item::*;
        let annon_tag = self.new_anonymous_tag(cx, element, &[b"display:", disp_str].concat());
        let d = self.el(annon_tag).css.m_display;
        let kind = if !table && d == display_table_cell {
            self.set_tag_name(cx.names, annon_tag, b"table_cell");
            RenderKind::render_item_block
        } else if table && (d == display_table || d == display_inline_table) {
            RenderKind::render_item_table
        } else if d == display_table_row {
            RenderKind::render_item_table_row
        } else {
            RenderKind::render_item_table_part
        };
        self.new_render_item(cx, kind, annon_tag)
    }

    /// `fix_table_children(el_ptr, disp, disp_str)`: each run of children that are not of `disp` (not starting with
    /// a space, a comment or a box not displayed; a caption neither starting nor joining one in a table) wrapped in
    /// an anonymous box of `disp`, which goes in the run's place; the run's children after the box leave the list up
    /// to the first one whose parent is still this item.
    pub fn fix_table_children(&mut self, cx: &StyleCx, el_ptr: crate::render_item::RenderId, disp: i32, disp_str: &[u8]) {
        use crate::render_item::*;
        let mut tmp: Vec<RenderId> = Vec::new();
        let mut first = 0;
        let mut cur = 0;

        loop {
            let at_end = cur >= self.render.children(el_ptr).len();
            if at_end && tmp.is_empty() {
                break;
            }
            if !at_end {
                let child = self.render.children(el_ptr)[cur];
                let child_display = self.render_display(child);
                if child_display != disp {
                    let skip = self.is_table_skip(self.render.item(child).element);
                    if (!skip || !tmp.is_empty()) && (disp != display_table_row_group || child_display != display_table_caption) {
                        if tmp.is_empty() {
                            first = cur;
                        }
                        tmp.push(child);
                    }
                    cur += 1;
                    continue;
                }
                if tmp.is_empty() {
                    cur += 1;
                    continue;
                }
            }
            // flush_elements
            let element = self.render.item(el_ptr).element;
            let annon_ri = self.anonymous_table_box(cx, element, disp_str, false);
            for &el in &tmp {
                self.add_render_child(annon_ri, el);
            }
            // add annon item as tabular for future processing
            self.add_tabular(annon_ri);
            self.render.set_parent(annon_ri, Some(el_ptr));
            self.render.hold(annon_ri);
            self.render_children_insert(el_ptr, first, annon_ri);
            cur = first + 1;
            let mut end = cur;
            while end < self.render.children(el_ptr).len() && self.render.parent(self.render.children(el_ptr)[end]) != Some(el_ptr) {
                end += 1;
            }
            for gone in self.render_children_drain(el_ptr, cur, end) {
                self.release_render(gone);
            }
            first = cur;
            tmp.clear();
            if at_end {
                break;
            }
        }
    }

    /// `fix_table_parent(el_ptr, disp, disp_str)`: unless the parent is of `disp`, the item and its neighbours of
    /// the same display (spaces, comments and boxes not displayed among them) wrapped in an anonymous box of `disp`
    /// in their place.
    pub fn fix_table_parent(&mut self, cx: &StyleCx, el_ptr: crate::render_item::RenderId, disp: i32, disp_str: &[u8]) {
        // The C++ reads the parent without a check; every tabular item it meets has one.
        let Some(parent) = self.render.parent(el_ptr) else { return };

        if self.render_display(parent) != disp {
            let Some(this_element) = self.render.children(parent).iter().position(|&el| el == el_ptr) else { return };
            let el_disp = self.render_display(el_ptr);
            let mut first = this_element;
            let mut last = this_element;
            let same = |doc: &Document, item: crate::render_item::RenderId| {
                doc.is_table_skip(doc.render.item(item).element) || doc.render_display(item) == el_disp
            };

            // find first element with same display
            let mut cur = this_element;
            while cur > 0 {
                cur -= 1;
                if same(self, self.render.children(parent)[cur]) {
                    first = cur;
                } else {
                    break;
                }
            }

            // find last element with same display
            cur = this_element;
            loop {
                cur += 1;
                if cur >= self.render.children(parent).len() {
                    break;
                }
                if same(self, self.render.children(parent)[cur]) {
                    last = cur;
                } else {
                    break;
                }
            }

            // extract elements with the same display and wrap them with anonymous object
            let element = self.render.item(parent).element;
            let annon_ri = self.anonymous_table_box(cx, element, disp_str, true);
            for k in first..=last {
                let el = self.render.children(parent)[k];
                self.add_render_child(annon_ri, el);
            }
            let gone = self.render_children_drain(parent, first, last + 1);
            self.render.hold(annon_ri);
            self.render_children_insert(parent, first, annon_ri);
            for el in gone {
                self.release_render(el);
            }
            self.add_tabular(annon_ri);
            self.render.set_parent(annon_ri, Some(parent));
        }
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
