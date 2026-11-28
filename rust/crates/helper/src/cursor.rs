//! tdesktop's `tgw/tgw_cursor.{h,cpp}`: the CSS cursor at a point, from a litehtml document of the renderer's own
//! HTML, master CSS and viewport, its `:hover` and `:active` chains set as the glue's pointer targets say, as
//! `twlite::Renderer::cursor()` is only the last delivered move's. The document is the glue's own, made again when
//! the renderer's master CSS or viewport changed and patched to its HTML when only that did, on the process's
//! litehtml state (its name table and declaration cache), as the C++'s shares the process-wide one.

use std::collections::HashMap;
use std::sync::Arc;

use twidget_litehtml::Px;
use twidget_litehtml::background::{BackgroundLayer, ConicGradient, LinearGradient, RadialGradient};
use twidget_litehtml::borders::{BorderRadiuses, Borders};
use twidget_litehtml::css_selector::CssSelector;
use twidget_litehtml::css_values::MediaType;
use twidget_litehtml::document::{Document, StyleCx};
use twidget_litehtml::document_container::{DocumentContainer, ListMarker};
use twidget_litehtml::element::ElementId;
use twidget_litehtml::font_description::FontDescription;
use twidget_litehtml::string_id::{_active_, _hover_};
use twidget_litehtml::types::{FontMetrics, MediaFeatures, Position, Size};
use twidget_litehtml::web_color::{ResolveColor, WebColor};

use twidget_document::Process;

const FONT_HANDLE: u64 = 1;
const DEFAULT_FONT_SIZE: i32 = 16;
const DEFAULT_FONT_NAME: &[u8] = b"-apple-system, system-ui, sans-serif";
const LANGUAGE: &[u8] = b"en";
const CSS_DPI: f32 = 96.0;
const POINTS_PER_INCH: f32 = 72.0;
const COLOR_BITS: i32 = 8;
const COLOR_INDEX: i32 = 256;
const POINTER_CURSOR: &[u8] = b"pointer";
const ID_ATTRIBUTE: &[u8] = b"data-twid";
const ID_SELECTOR: &[u8] = b"[data-twid]";

/// What the cursor reads of the renderer (`twlite::Renderer`): its builds, and its HTML (built first when a batch
/// waits, as `last_html()` does), master CSS, width and viewport height, read in that order.
pub trait CursorSource {
    fn builds(&self) -> i32;
    fn document(&mut self) -> (&[u8], &[u8], f64, i32);
}

impl<E: twidget_renderer::Engine, B: twidget_renderer::Backend> CursorSource for twidget_renderer::Renderer<E, B> {
    fn builds(&self) -> i32 {
        self.stats().builds
    }

    fn document(&mut self) -> (&[u8], &[u8], f64, i32) {
        self.last_html();
        let (width, height) = (self.width(), self.viewport_height());
        (self.core().html_state().0, self.master_css(), width, height)
    }
}

/// `CursorStyles::Container`: a container that measures nothing, its fonts as tall as their size, its viewport and
/// media the renderer's.
#[derive(Debug)]
struct CursorContainer {
    dpr: f64,
    width: f64,
    height: i32,
}

impl ResolveColor for CursorContainer {
    fn resolve_color(&self, _name: &[u8]) -> Vec<u8> {
        Vec::new()
    }
}

impl DocumentContainer for CursorContainer {
    fn create_font(&self, descr: &FontDescription) -> (u64, FontMetrics) {
        let size = descr.size.value();
        let mut fm = FontMetrics { font_size: descr.size, ..FontMetrics::default() };
        fm.ascent = Px(size);
        fm.height = fm.ascent;
        fm.descent = Px(0.0);
        fm.ch_width = Px(size / 2.0);
        fm.x_height = fm.ch_width;
        (FONT_HANDLE, fm)
    }

    fn text_width(&self, _text: &[u8], _font: u64) -> Px {
        Px(0.0)
    }

    fn draw_text(&self, _hdc: u64, _text: &[u8], _font: u64, _color: WebColor, _pos: &Position) {}

    fn pt_to_px(&self, pt: f32) -> Px {
        Px(pt * CSS_DPI / POINTS_PER_INCH)
    }

    fn get_default_font_size(&self) -> Px {
        Px(DEFAULT_FONT_SIZE as f32)
    }

    fn get_default_font_name(&self) -> Vec<u8> {
        DEFAULT_FONT_NAME.to_vec()
    }

    fn draw_list_marker(&self, _hdc: u64, _marker: &ListMarker) {}

    fn load_image(&self, _src: &[u8], _baseurl: Option<&[u8]>, _redraw_on_ready: bool) {}

    fn get_image_size(&self, _src: &[u8], _baseurl: Option<&[u8]>) -> Size {
        Size::default()
    }

    fn draw_image(&self, _hdc: u64, _layer: &BackgroundLayer, _url: &[u8], _base_url: &[u8]) {}

    fn draw_solid_fill(&self, _hdc: u64, _layer: &BackgroundLayer, _color: &WebColor) {}

    fn draw_linear_gradient(&self, _hdc: u64, _layer: &BackgroundLayer, _gradient: &LinearGradient) {}

    fn draw_radial_gradient(&self, _hdc: u64, _layer: &BackgroundLayer, _gradient: &RadialGradient) {}

    fn draw_conic_gradient(&self, _hdc: u64, _layer: &BackgroundLayer, _gradient: &ConicGradient) {}

    fn draw_borders(&self, _hdc: u64, _borders: &Borders, _draw_pos: &Position, _root: bool) {}

    fn set_caption(&self, _caption: &[u8]) {}

    fn set_base_url(&self, _base_url: Option<&[u8]>) {}

    fn link(&self, _el: ElementId) {}

    fn set_cursor(&self, _cursor: &[u8]) {}

    fn transform_text(&self, _text: &mut Vec<u8>, _tt: i32) {}

    fn import_css(&self, _url: &[u8], _baseurl: &mut Vec<u8>) -> Vec<u8> {
        Vec::new()
    }

    fn set_clip(&self, _pos: &Position, _bdr_radius: &BorderRadiuses) {}

    fn del_clip(&self) {}

    fn get_viewport(&self) -> Position {
        Position { x: Px(0.0), y: Px(0.0), width: Px(self.width as f32), height: Px(self.height as f32) }
    }

    fn get_media_features(&self) -> MediaFeatures {
        let width = Px(self.width as f32);
        let height = Px(self.height as f32);
        MediaFeatures {
            ty: MediaType::media_type_screen.index(),
            width,
            height,
            device_width: width,
            device_height: height,
            color: COLOR_BITS,
            color_index: COLOR_INDEX,
            monochrome: 0,
            resolution: Px((f64::from(CSS_DPI) * self.dpr) as f32),
        }
    }

    fn get_language(&self) -> (Vec<u8>, Vec<u8>) {
        (LANGUAGE.to_vec(), Vec::new())
    }
}

/// `std::strtoll(text, nullptr, 10)` on a C string: leading whitespace, a sign, decimal digits; past `long long`'s
/// range the bound; 0 when no digit.
fn strtoll(text: &[u8]) -> i64 {
    let at = |i: usize| text.get(i).copied().unwrap_or(0);
    let mut i = 0;
    while matches!(at(i), b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r') {
        i += 1;
    }
    let negative = at(i) == b'-';
    if matches!(at(i), b'-' | b'+') {
        i += 1;
    }
    let mut value: i128 = 0;
    while at(i).is_ascii_digit() {
        value = (value * 10 + i128::from(at(i) - b'0')).min(i128::from(i64::MAX) + 1);
        i += 1;
    }
    let value = if negative { -value } else { value };
    value.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

/// An element and its ancestors, the element first (`Chain`).
type Chain = Vec<ElementId>;

/// A point query, as `twlite::Renderer::cursor()` is the last delivered move's (`Tgw::CursorStyles`).
pub struct CursorStyles {
    container: CursorContainer,
    process: Arc<Process>,
    master: Vec<u8>,
    document: Option<Document>,
    elements: HashMap<i64, ElementId>,
    html: Vec<u8>,
    width: f64,
    height: i32,
    builds: i32,
    hover: i64,
    active: i64,
}

impl std::fmt::Debug for CursorStyles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CursorStyles").field("builds", &self.builds).field("hover", &self.hover).field("active", &self.active).finish()
    }
}

/// What the cursor holds, for the comparison.
#[derive(Clone, Debug, PartialEq)]
pub struct CursorState {
    pub hover: i64,
    pub active: i64,
    pub builds: i32,
    pub width: f64,
    pub height: i32,
    pub document: bool,
    pub html_bytes: usize,
    pub master_bytes: usize,
    /// Every indexed element by its id, with its computed `cursor`.
    pub elements: Vec<(i64, Vec<u8>)>,
}

impl CursorStyles {
    pub fn new(dpr: f64, process: Arc<Process>) -> CursorStyles {
        CursorStyles {
            container: CursorContainer { dpr, width: 0.0, height: 0 },
            process,
            master: Vec::new(),
            document: None,
            elements: HashMap::new(),
            html: Vec::new(),
            width: 0.0,
            height: 0,
            builds: 0,
            hover: 0,
            active: 0,
        }
    }

    /// Whether the element `hit` computes `cursor: pointer` once `hover` and `active` (with their ancestors) hover
    /// and are active.
    pub fn pointer(&mut self, renderer: &mut dyn CursorSource, hit: i64, hover: i64, active: i64) -> bool {
        self.sync(renderer, hover, active);
        let Some(&element) = self.elements.get(&hit) else { return false };
        let Some(doc) = &self.document else { return false };
        doc.el(element).css.m_cursor == POINTER_CURSOR
    }

    pub fn state(&self) -> CursorState {
        let mut elements: Vec<(i64, Vec<u8>)> = match &self.document {
            Some(doc) => self.elements.iter().map(|(&id, &el)| (id, doc.el(el).css.m_cursor.clone())).collect(),
            None => Vec::new(),
        };
        elements.sort_by_key(|e| e.0);
        CursorState {
            hover: self.hover,
            active: self.active,
            builds: self.builds,
            width: self.width,
            height: self.height,
            document: self.document.is_some(),
            html_bytes: self.html.len(),
            master_bytes: self.master.len(),
            elements,
        }
    }

    fn sync(&mut self, renderer: &mut dyn CursorSource, hover: i64, active: i64) {
        let reconciled = self.refresh(renderer);
        let CursorStyles { container, process, document, elements, hover: applied_hover, active: applied_active, .. } = self;
        let Some(doc) = document.as_mut() else { return };
        let hover_changed = retarget(doc, elements, applied_hover, hover, _hover_);
        let active_changed = retarget(doc, elements, applied_active, active, _active_);
        if reconciled || hover_changed || active_changed {
            let cx = StyleCx { names: &process.names, container: &*container, cache: &process.cache, switches: process.switches };
            if let Some(root) = doc.root {
                doc.find_styles_changes(&cx, root);
            }
        }
    }

    fn refresh(&mut self, renderer: &mut dyn CursorSource) -> bool {
        let builds = renderer.builds();
        if self.document.is_some() && self.builds == builds {
            return false;
        }
        self.builds = builds;
        let (html, master, width, height) = renderer.document();
        let same = self.master == master && self.width == width && self.height == height;
        if self.document.is_some() && same && self.html == html {
            return false;
        }
        self.html = html.to_vec();
        let master = master.to_vec();
        if self.document.is_some() && same {
            let CursorStyles { container, process, document, elements, hover, active, html, .. } = self;
            let doc = document.as_mut().expect("a document");
            let cx = StyleCx { names: &process.names, container: &*container, cache: &process.cache, switches: process.switches };
            let hover_chain = chain_of(doc, elements.get(hover).copied());
            let active_chain = chain_of(doc, elements.get(active).copied());
            // The C++'s chains hold their elements: an anonymous one outlives a patch that ends its box.
            hold(doc, &hover_chain);
            hold(doc, &active_chain);
            let mut changed = false;
            if doc.update_from_string(&cx, html, &mut changed) {
                let result = if !changed || doc.last_patch_paint_only() {
                    false
                } else {
                    index(doc, &cx, elements);
                    let hover_cleared = reconcile(doc, elements, &hover_chain, hover, _hover_);
                    let active_cleared = reconcile(doc, elements, &active_chain, active, _active_);
                    hover_cleared || active_cleared
                };
                release(doc, &active_chain);
                release(doc, &hover_chain);
                return result;
            }
            release(doc, &active_chain);
            release(doc, &hover_chain);
        }
        self.master = master;
        self.rebuild(width, height);
        false
    }

    fn rebuild(&mut self, width: f64, height: i32) {
        self.hover = 0;
        self.active = 0;
        self.elements.clear();
        self.document = None;
        self.width = width;
        self.height = height;
        self.container.width = width;
        self.container.height = height;
        let CursorStyles { container, process, document, elements, html, master, .. } = self;
        let cx = StyleCx { names: &process.names, container: &*container, cache: &process.cache, switches: process.switches };
        let doc = document.insert(Document::create_updatable(html, master, &cx));
        index(doc, &cx, elements);
    }
}

/// `index()`: every element with a widget id under the root, by that id.
fn index(doc: &Document, cx: &StyleCx, elements: &mut HashMap<i64, ElementId>) {
    elements.clear();
    let Some(root) = doc.root else { return };
    let Some(selector) = CssSelector::parse(cx.names, ID_SELECTOR, doc.mode) else { return };
    let mut found = Vec::new();
    doc.matcher(cx).select_all(root, &selector, &mut found);
    for element in found {
        if let Some(value) = doc.get_attr(element, ID_ATTRIBUTE) {
            let value = &value[..value.iter().position(|&c| c == 0).unwrap_or(value.len())];
            elements.insert(strtoll(value), element);
        }
    }
}

fn chain_of(doc: &Document, element: Option<ElementId>) -> Chain {
    let mut result = Chain::new();
    let mut element = element;
    while let Some(e) = element {
        result.push(e);
        element = doc.parent(e);
    }
    result
}

fn hold(doc: &mut Document, chain: &Chain) {
    for &e in chain {
        doc.hold_anonymous(Some(e));
    }
}

fn release(doc: &mut Document, chain: &Chain) {
    for &e in chain {
        doc.release_anonymous(e);
    }
}

/// `MarkChain`: the pseudo-class set or cleared up the element's chain; whether any changed.
fn mark_chain(doc: &mut Document, element: Option<ElementId>, pseudo: twidget_litehtml::StringId, add: bool) -> bool {
    let mut result = false;
    let mut element = element;
    while let Some(e) = element {
        result = doc.set_pseudo_class(e, pseudo, add) || result;
        element = doc.parent(e);
    }
    result
}

/// `Retarget`: the pseudo-class moved from `applied`'s chain to `wanted`'s.
fn retarget(
    doc: &mut Document,
    elements: &HashMap<i64, ElementId>,
    applied: &mut i64,
    wanted: i64,
    pseudo: twidget_litehtml::StringId,
) -> bool {
    if *applied == wanted {
        return false;
    }
    let cleared = mark_chain(doc, elements.get(applied).copied(), pseudo, false);
    let marked = mark_chain(doc, elements.get(&wanted).copied(), pseudo, true);
    *applied = wanted;
    cleared || marked
}

/// `Reconcile`: after a patch, a chain that is no longer the applied element's loses the pseudo-class, and nothing
/// is applied.
fn reconcile(
    doc: &mut Document,
    elements: &HashMap<i64, ElementId>,
    marked: &Chain,
    applied: &mut i64,
    pseudo: twidget_litehtml::StringId,
) -> bool {
    if chain_of(doc, elements.get(applied).copied()) == *marked {
        return false;
    }
    *applied = 0;
    let mut result = false;
    for &e in marked {
        result = doc.set_pseudo_class(e, pseudo, false) || result;
    }
    result
}
