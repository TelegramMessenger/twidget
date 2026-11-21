//! [`DocumentEngine`]: `twidget_renderer::Engine` on a `twidget_litehtml::Document` and renderer.cpp's container. Each
//! call is what the C++ renderer asks of `Container`, of the document it holds and of the free helpers, answered as the
//! C++ answers it: the document's life (a new document through `createUpdatableFromString`, patches, the layout budget,
//! the element index), the declaration scans by style epoch, the queries' reads of the render tree, the input's
//! questions (hit tests with the renderer's `hittable` check, the mouse state, the editor's style, the text runs, the
//! selectable text), and stage 4.5's (the heavy boxes' walk, layer candidates and clips, shimmer tiles, the paint trace
//! and what covers what, the document drawn whole, without layers or as one layer).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use twidget_animations::{Declared, StyleValues};
use twidget_computed_style as computed;
use twidget_ends::Backend;
use twidget_events::{EditorStyle, TextRun};
use twidget_json::Json;
use twidget_litehtml::Px;
use twidget_litehtml::css_selector::CssSelector;
use twidget_litehtml::css_values::*;
use twidget_litehtml::document::{Document, StyleCx, Switches};
use twidget_litehtml::el_svg::{DocSvg, svg_geometry, svg_path_length};
use twidget_litehtml::element::{ElementId, ElementKind};
use twidget_litehtml::html_tag::ElementVars;
use twidget_litehtml::paint::DrawFlag;
use twidget_litehtml::patch::HoleChange as LhHoleChange;
use twidget_litehtml::render_item::{RenderId, RenderKind};
use twidget_litehtml::string_id::{
    __tag_after_, __tag_before_, _active_, _body_, _button_, _fieldset_, _input_, _meter_, _progress_, _select_, _textarea_,
};
use twidget_litehtml::style::DeclarationCache;
use twidget_litehtml::types::Position;
use twidget_litehtml::{Names, StringId};
use twidget_renderer::{BoxStyle, Clip, Engine, HoleChange, ScrollExtent, TextContent, Tile};
use twidget_wire::{DisplayList, Radii, Rect};

use crate::boxes::*;
use crate::container::{Container, color_of, radii_of, rect_of};
use crate::cxx::{cmax, strtoll};

/// renderer.cpp's `LAYER_MARGIN`: the redraw box leaves out shadows and anti-aliasing.
const LAYER_MARGIN: f32 = 48.0;

/// What litehtml keeps once per process in the C++ (the name table, the declaration cache, the `LH_*` switches): the
/// helper makes one and every renderer's engine shares it.
#[derive(Debug, Default)]
pub struct Process {
    pub names: Names,
    pub cache: DeclarationCache,
    pub switches: Switches,
}

/// `Scan`: one `declarations()` answer, kept for its style epoch.
#[derive(Clone, Debug)]
struct Scan {
    names: Vec<Vec<u8>>,
    gates: Vec<Vec<u8>>,
    always: Vec<i64>,
    epoch: u64,
    generation: u64,
    result: BTreeMap<i64, Declared>,
}

/// `Cover`: what a traced item can show: its border box inside its clipping ancestors, and whether it or an ancestor is
/// hidden or fully transparent.
#[derive(Clone, Copy, Debug)]
struct Cover {
    rect: Rect,
    invisible: bool,
}

/// `widget_id(el, placement)`'s walk: the widget id of an element or its nearest ancestor with one (0 when none), and
/// that ancestor's border box.
fn widget_id(doc: &Document, cx: &StyleCx, el: Option<ElementId>, placement: Option<&mut Rect>) -> i64 {
    let mut cur = el;
    while let Some(e) = cur {
        if let Some(a) = doc.get_attr(e, b"data-twid") {
            let id = strtoll(crate::container::c_str(a));
            if let Some(p) = placement {
                *p = border_box_of(doc, cx, e);
            }
            return id;
        }
        cur = doc.parent(e);
    }
    0
}

/// The renderer's document and container as `twidget_renderer::Engine`. `K` is the backend the container makes fonts,
/// measures text and decodes images with.
pub struct DocumentEngine<K: Backend> {
    process: Arc<Process>,
    container: Container<K>,
    doc: Option<Document>,
    /// The document's identity in the keys (`doc_.get()`): 0 for none, else unique per document made.
    doc_id: u64,
    next_doc: u64,
    /// Identities to give the next documents, in order, before counting: what a comparison sets to the C++'s.
    given_ids: std::collections::VecDeque<u64>,
    elements: BTreeMap<i64, ElementId>,
    scans: Vec<Scan>,
    scan_generation: u64,
    /// `layouts_`: each `render` is one.
    layouts: u64,
    trace: Vec<RenderId>,
    trace_key: Option<(u64, u32, u64)>,
    cover: Vec<Cover>,
    cover_index: HashMap<RenderId, usize>,
    cover_key: Option<(u64, u32, u64)>,
}

impl<K: Backend> std::fmt::Debug for DocumentEngine<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentEngine").field("doc_id", &self.doc_id).field("layouts", &self.layouts).finish_non_exhaustive()
    }
}

impl<K: Backend> Drop for DocumentEngine<K> {
    fn drop(&mut self) {
        // The document releases its fonts through the container: dropped while the font table still exists.
        self.drop_doc();
        self.container.delete_faces();
    }
}

impl<K: Backend> DocumentEngine<K> {
    pub fn new(process: Arc<Process>, backend: K) -> DocumentEngine<K> {
        DocumentEngine {
            process,
            container: Container::new(backend),
            doc: None,
            doc_id: 0,
            next_doc: 0,
            given_ids: std::collections::VecDeque::new(),
            elements: BTreeMap::new(),
            scans: Vec::new(),
            scan_generation: 0,
            layouts: 0,
            trace: Vec::new(),
            trace_key: None,
            cover: Vec::new(),
            cover_index: HashMap::new(),
            cover_key: None,
        }
    }

    pub fn container(&self) -> &Container<K> {
        &self.container
    }
    pub fn document(&self) -> Option<&Document> {
        self.doc.as_ref()
    }
    pub fn process(&self) -> &Arc<Process> {
        &self.process
    }

    /// The identities the next documents take in the keys, in order (a comparison gives the C++'s addresses); past
    /// them each new document takes one of its own.
    pub fn give_document_ids(&mut self, ids: impl IntoIterator<Item = u64>) {
        self.given_ids.extend(ids);
    }

    fn cx(&self) -> StyleCx<'_> {
        StyleCx { names: &self.process.names, container: &self.container, cache: &self.process.cache, switches: self.process.switches }
    }

    /// The document dropped (`doc_ = nullptr`): it gives its fonts back through the container, in its table's order.
    fn drop_doc(&mut self) {
        self.elements.clear();
        if let Some(doc) = self.doc.take() {
            let fonts: Vec<u64> = doc.fonts.borrow().values().map(|f| f.font).collect();
            drop(doc);
            for font in fonts {
                self.container.delete_font(font);
            }
        }
        self.doc_id = 0;
    }

    fn element(&self, id: i64) -> Option<ElementId> {
        self.elements.get(&id).copied()
    }

    /// `paint_key()`'s tuple with the layouts.
    fn key(&self) -> (u64, u32, u64) {
        (self.doc_id, self.doc.as_ref().map_or(0, Document::paint_generation), self.layouts)
    }

    /// `draw()`: the document clipped to the viewport into a new list.
    fn draw_plain(&mut self) -> DisplayList {
        self.container.begin_list();
        let cx =
            StyleCx { names: &self.process.names, container: &self.container, cache: &self.process.cache, switches: self.process.switches };
        if let Some(doc) = self.doc.as_mut() {
            self.container.prepare_paint(doc);
            let clip = Position {
                x: Px(0.0),
                y: Px(0.0),
                width: Px(self.container.width() as f32),
                height: Px(self.container.vp_height() as f32),
            };
            doc.draw(&cx, 0, Px(0.0), Px(0.0), Some(&clip), false);
        }
        self.container.take_list()
    }

    /// `clip_of`: the ancestor that clips `el`, if one does: the innermost one's padding box and inner radii; false when
    /// an outer one could cut what the inner one lets through. `has`, `rect` and `radii` as the C++ leaves them.
    fn clip_of(doc: &Document, cx: &StyleCx, el: ElementId, has: &mut bool, rect: &mut Rect, radii: &mut Radii) -> bool {
        *has = false;
        let Some(ri) = doc.get_render_item(el) else { return false };
        let mut p = doc.render.parent(ri);
        while let Some(item) = p {
            let pel = doc.render.item(item).element;
            if !doc.el(pel).css.clips_contents() {
                p = doc.render.parent(item);
                continue;
            }
            let border = border_box_of(doc, cx, pel);
            let r = doc.el(pel).css.m_css_borders.radius.calc_percents(Px(border.w), Px(border.h));
            if *has {
                // Past its rounded corners an outer clip is a rectangle: it is moot when that holds the inner one.
                let mut corner = r.top_left_x.value();
                for v in [r.top_left_y, r.top_right_x, r.top_right_y, r.bottom_left_x, r.bottom_left_y, r.bottom_right_x, r.bottom_right_y]
                {
                    corner = cmax(corner, v.value());
                }
                let outer = padding_box_of(doc, cx, item);
                if rect.x < outer.x + corner
                    || rect.y < outer.y + corner
                    || rect.x + rect.w > outer.x + outer.w - corner
                    || rect.y + rect.h > outer.y + outer.h - corner
                {
                    return false;
                }
                p = doc.render.parent(item);
                continue;
            }
            *has = true;
            let b = &doc.render.item(item).borders;
            *rect = padding_box_of(doc, cx, item);
            *radii = Radii {
                tlx: cmax(0.0, r.top_left_x.value() - b.left.value()),
                tly: cmax(0.0, r.top_left_y.value() - b.top.value()),
                trx: cmax(0.0, r.top_right_x.value() - b.right.value()),
                try_: cmax(0.0, r.top_right_y.value() - b.top.value()),
                brx: cmax(0.0, r.bottom_right_x.value() - b.right.value()),
                bry: cmax(0.0, r.bottom_right_y.value() - b.bottom.value()),
                blx: cmax(0.0, r.bottom_left_x.value() - b.left.value()),
                bly: cmax(0.0, r.bottom_left_y.value() - b.bottom.value()),
            };
            p = doc.render.parent(item);
        }
        true
    }

    /// `box_paint_of`'s reads of an element's computed values.
    fn box_style_of(doc: &Document, el: ElementId, pseudo: bool) -> BoxStyle {
        let css = &doc.el(el).css;
        let mut blurred = false;
        for shadow in &css.m_box_shadows {
            if !matches!(shadow.blur.val().partial_cmp(&0.0), Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)) {
                blurred = true;
                break;
            }
        }
        let dimmed = !pseudo
            && doc.tag(el) == _button_
            && (doc.get_attr(el, b"disabled").is_some() || (doc.kind(el).is_html_tag() && doc.el(el).pseudo_classes.contains(&_active_)));
        BoxStyle { backdrop: !css.m_backdrop_filter.is_empty(), filter: !css.m_filter.is_empty(), blurred, dimmed, opacity: css.m_opacity }
    }

    /// `scan`: the items painted after `el` that meet `area`, skipping its descendants, what `ignore` holds and what
    /// shows nothing: 1 at the first when `ids` is None, otherwise their widget ids into `ids` (-1 past `limit` or for
    /// one without an id). 0 when nothing covers it; 1 too when `el` is not painted.
    fn scan(
        &mut self,
        el: Option<ElementId>,
        area: Rect,
        ignore: &BTreeSet<ElementId>,
        mut ids: Option<&mut Vec<i64>>,
        limit: usize,
    ) -> i32 {
        let Some(doc) = self.doc.as_ref() else { return if ids.is_some() { -1 } else { 1 } };
        let cx =
            StyleCx { names: &self.process.names, container: &self.container, cache: &self.process.cache, switches: self.process.switches };
        if self.cover_key != self.trace_key || self.cover_key.is_none() {
            self.cover.clear();
            self.cover_index.clear();
            self.cover.reserve(self.trace.len());
            for (i, &item) in self.trace.iter().enumerate() {
                let Some(it) = doc.render.get(item) else {
                    self.cover.push(Cover { rect: Rect::default(), invisible: true });
                    continue;
                };
                let e = it.element;
                let mut c = Cover {
                    rect: clipped_border_box(doc, &cx, item),
                    invisible: doc.is_space(e) || doc.el(e).css.m_visibility != Visibility::visibility_visible.index(),
                };
                let mut walk = Some(item);
                while let Some(w) = walk {
                    if c.invisible {
                        break;
                    }
                    c.invisible = doc.ri_css(w).m_opacity <= 0.0;
                    walk = doc.render.parent(w);
                }
                self.cover.push(c);
                self.cover_index.entry(item).or_insert(i);
            }
            self.cover_key = self.trace_key;
        }
        let ri = el.and_then(|e| doc.get_render_item(e));
        let Some(&self_index) = ri.and_then(|r| self.cover_index.get(&r)) else { return if ids.is_some() { -1 } else { 1 } };
        for i in self_index + 1..self.trace.len() {
            let c = self.cover[i];
            // The box test first: most items are nowhere near the layer.
            let b = c.rect;
            if c.invisible || b.w <= 0.0 || b.h <= 0.0 {
                continue;
            }
            if !(b.x < area.x + area.w && area.x < b.x + b.w && b.y < area.y + area.h && area.y < b.y + b.h) {
                continue;
            }
            let mut skip = false;
            let mut walk = Some(self.trace[i]);
            while let Some(w) = walk {
                if skip {
                    break;
                }
                skip = Some(w) == ri || ignore.contains(&doc.render.item(w).element);
                walk = doc.render.parent(w);
            }
            if skip {
                continue;
            }
            let Some(list) = ids.as_deref_mut() else { return 1 };
            let id = widget_id(doc, &cx, Some(doc.render.item(self.trace[i]).element), None);
            if id == 0 {
                return -1;
            }
            if list.contains(&id) {
                continue;
            }
            if list.len() == limit {
                return -1;
            }
            list.push(id);
        }
        if ids.is_some_and(|l| !l.is_empty()) { 1 } else { 0 }
    }

    /// The elements of widget ids, as a set (what the C++ keeps as element pointers).
    fn element_set(&self, ids: &BTreeSet<i64>) -> BTreeSet<ElementId> {
        ids.iter().filter_map(|id| self.element(*id)).collect()
    }

    /// `positioned_z`.
    fn positioned_z(doc: &Document, item: RenderId, z: &mut BTreeSet<i32>) {
        let mut stack = vec![item];
        while let Some(i) = stack.pop() {
            for &child in doc.render.children(i).iter().rev() {
                let e = doc.render.item(child).element;
                if doc.is_positioned(e) {
                    z.insert(doc.el(e).css.get_z_index());
                } else if !doc.el(e).css.is_paint_group() {
                    stack.push(child);
                }
            }
        }
    }

    /// `draw_stacking_context`: an element's children in stacking order, as if it formed a stacking context.
    fn draw_stacking_context(doc: &mut Document, cx: &StyleCx, ri: RenderId, x: Px, y: Px) {
        let mut z = BTreeSet::new();
        Self::positioned_z(doc, ri, &mut z);
        for &index in &z {
            if index < 0 {
                doc.draw_children(cx, ri, 0, x, y, None, DrawFlag::draw_positioned, index);
            }
        }
        doc.draw_children(cx, ri, 0, x, y, None, DrawFlag::draw_block, 0);
        doc.draw_children(cx, ri, 0, x, y, None, DrawFlag::draw_floats, 0);
        doc.draw_children(cx, ri, 0, x, y, None, DrawFlag::draw_inlines, 0);
        for &index in &z {
            if index >= 0 {
                doc.draw_children(cx, ri, 0, x, y, None, DrawFlag::draw_positioned, index);
            }
        }
    }

    /// `element_at`: what the document has at (x, y), through the renderer's `hittable`.
    fn element_at(doc: &Document, cx: &StyleCx, x: f64, y: f64) -> Option<ElementId> {
        let root = doc.render.root?;
        let px = Px(x as f32);
        let py = Px(y as f32);
        let check = |d: &Document, ri: RenderId| pointer_events(d, cx, d.render.item(ri).element) != b"none";
        doc.get_element_by_point(cx, root, px, py, px, py, Some(&check))
    }
}

impl<K: Backend> Engine for DocumentEngine<K> {
    // ── what the container reads of the renderer ──

    fn viewport(&mut self, width: f64, dpr: f64) {
        self.container.set_width(width, dpr);
    }
    fn viewport_height(&mut self, vp_height: i32) {
        self.container.set_vp_height(vp_height);
    }
    fn canvases(&mut self, held: &[(i32, i32, i32)]) {
        self.container.set_canvases(held);
    }

    // ── the document's life ──

    fn has_viewport_units(&mut self, html: &[u8]) -> bool {
        twidget_litehtml::css_effects::has_viewport_units_text(html)
    }

    fn update_from_string(&mut self, html: &[u8]) -> Option<bool> {
        let cx =
            StyleCx { names: &self.process.names, container: &self.container, cache: &self.process.cache, switches: self.process.switches };
        let doc = self.doc.as_mut()?;
        let mut changed = true;
        doc.update_from_string(&cx, html, &mut changed).then_some(changed)
    }

    fn media_changed(&mut self) {
        let cx =
            StyleCx { names: &self.process.names, container: &self.container, cache: &self.process.cache, switches: self.process.switches };
        if let Some(doc) = self.doc.as_mut() {
            doc.media_changed(&cx);
        }
    }

    fn last_patch_paint_only(&mut self) -> bool {
        self.doc.as_ref().is_some_and(Document::last_patch_paint_only)
    }

    fn create(&mut self, html: &[u8], master: &[u8]) -> bool {
        self.drop_doc();
        self.scans.clear();
        // The fork's keys are only good within a document: a new one may resolve colours differently.
        self.container.drop_cached();
        let doc = Document::create_updatable(html, master, &self.cx());
        self.doc = Some(doc);
        self.next_doc += 1;
        self.doc_id = self.given_ids.pop_front().unwrap_or(self.next_doc);
        true
    }

    fn drop_document(&mut self) {
        self.drop_doc();
    }

    fn element_count(&mut self) -> u64 {
        let Some(doc) = self.doc.as_ref() else { return 0 };
        let mut elements = 0u64;
        let mut pending: Vec<ElementId> = doc.root.into_iter().collect();
        while let Some(el) = pending.pop() {
            elements += 1;
            pending.extend_from_slice(doc.children(el));
        }
        elements
    }

    fn render(&mut self, width: f32, budget: u64) {
        let cx =
            StyleCx { names: &self.process.names, container: &self.container, cache: &self.process.cache, switches: self.process.switches };
        if let Some(doc) = self.doc.as_mut() {
            doc.set_layout_budget(budget);
            doc.layout_document(&cx, Px(width));
        }
        self.layouts += 1;
    }

    fn layout_exhausted(&mut self) -> bool {
        self.doc.as_ref().is_some_and(|d| d.layout.exhausted)
    }

    fn layout_spent(&mut self) -> u64 {
        self.doc.as_ref().map_or(0, |d| d.layout.spent)
    }

    fn index(&mut self) {
        self.elements.clear();
        let Some(doc) = self.doc.as_ref() else { return };
        let Some(root) = doc.root else { return };
        let cx = self.cx();
        let Some(sel) = CssSelector::parse(cx.names, b"[data-twid]", doc.mode) else { return };
        let mut found = Vec::new();
        doc.matcher(&cx).select_all(root, &sel, &mut found);
        for el in found {
            if let Some(a) = doc.get_attr(el, b"data-twid") {
                self.elements.insert(strtoll(crate::container::c_str(a)), el);
            }
        }
    }

    fn height(&mut self) -> f64 {
        self.doc.as_ref().map_or(0.0, |d| f64::from(d.layout.size.height.value()))
    }

    fn has_root(&mut self) -> bool {
        self.doc.as_ref().is_some_and(|d| d.root.is_some())
    }

    fn layout_reads_viewport_height(&mut self) -> bool {
        let Some(doc) = self.doc.as_ref() else { return false };
        if !doc.get_fixed_boxes().is_empty() {
            return true;
        }
        let percent = |el: ElementId| {
            let css = &doc.el(el).css;
            css.m_css_height.units() == CssUnits::css_units_percentage || css.m_css_min_height.units() == CssUnits::css_units_percentage
        };
        let Some(root) = doc.root else { return false };
        if percent(root) {
            return true;
        }
        for &child in doc.children(root) {
            if doc.tag(child) == _body_ && percent(child) {
                return true;
            }
        }
        for &el in self.elements.values() {
            if doc.el(el).css.m_el_position != ElementPosition::element_position_absolute.index() {
                continue;
            }
            let mut contained = false;
            let mut p = doc.parent(el);
            while let Some(e) = p {
                if contained {
                    break;
                }
                contained = doc.el(e).css.m_el_position != ElementPosition::element_position_static.index();
                p = doc.parent(e);
            }
            if !contained {
                return true;
            }
        }
        false
    }

    fn font_count(&mut self) -> usize {
        self.container.font_count()
    }

    fn update_holes(&mut self, changes: &[HoleChange]) -> Option<bool> {
        let changes: Vec<LhHoleChange> = changes
            .iter()
            .map(|c| LhHoleChange { owner: self.element(c.owner), name: c.name.clone(), text: c.text, value: c.value.clone() })
            .collect();
        let cx =
            StyleCx { names: &self.process.names, container: &self.container, cache: &self.process.cache, switches: self.process.switches };
        let doc = self.doc.as_mut()?;
        let mut changed = false;
        doc.update_holes(&cx, &changes, &mut changed).then_some(changed)
    }

    fn update_styles(&mut self, patches: &[(i64, Vec<u8>)]) -> Option<bool> {
        let patches: Vec<(Option<ElementId>, Vec<u8>)> = patches.iter().map(|(id, s)| (self.element(*id), s.clone())).collect();
        let cx =
            StyleCx { names: &self.process.names, container: &self.container, cache: &self.process.cache, switches: self.process.switches };
        let doc = self.doc.as_mut()?;
        let mut changed = false;
        doc.update_styles(&cx, &patches, &mut changed).then_some(changed)
    }

    fn name_usage(&mut self) -> (u64, u64) {
        let u = self.process.names.usage();
        (u.names as u64, u.bytes as u64)
    }

    fn trim(&mut self) {
        self.container.trim();
        self.trace = Vec::new();
        self.trace_key = None;
        self.cover = Vec::new();
        self.cover_index = HashMap::new();
        self.cover_key = None;
        self.scans.clear();
    }

    fn set_no_effects(&mut self) {
        if self.container.set_no_effects()
            && let Some(doc) = self.doc.as_mut()
        {
            doc.invalidate_paint();
        }
    }

    fn take_violations(&mut self) -> Vec<Vec<u8>> {
        self.container.take_violations()
    }

    // ── elements ──

    fn has_element(&mut self, id: i64) -> bool {
        self.elements.contains_key(&id)
    }

    fn declarations(&mut self, names: &[Vec<u8>], gates: &[Vec<u8>], always: &[i64]) -> (BTreeMap<i64, Declared>, u64) {
        let Some(doc) = self.doc.as_ref() else { return (BTreeMap::new(), 0) };
        let Some(root) = doc.root else { return (BTreeMap::new(), 0) };
        let epoch = doc.layout.style_epoch;
        // A frame asks the same few questions of a document whose styles have not changed: the walk is done once per
        // style epoch for each.
        for i in 0..self.scans.len() {
            let s = &self.scans[i];
            if s.epoch != epoch || s.names != names || s.gates != gates || s.always != always {
                continue;
            }
            self.scans[..=i].rotate_right(1);
            return (self.scans[0].result.clone(), self.scans[0].generation);
        }
        let mut out: BTreeMap<i64, Declared> = BTreeMap::new();
        let names_table = &self.process.names;
        let mut pending = vec![root];
        while let Some(el) = pending.pop() {
            if doc.el(el).css.m_display == StyleDisplay::display_none.index() {
                continue;
            }
            if doc.kind(el).is_html_tag()
                && let Some(a) = doc.get_attr(el, b"data-twid")
            {
                let id = strtoll(crate::container::c_str(a));
                let listed = always.contains(&id);
                let mut open = listed;
                let mut i = 0;
                while !open && i < gates.len() {
                    open = !doc.raw_declaration(names_table, el, &gates[i]).is_empty();
                    i += 1;
                }
                if open {
                    let mut found = Declared::new();
                    for name in names {
                        let raw = doc.raw_declaration(names_table, el, name);
                        if !raw.is_empty() {
                            found.entry(name.clone()).or_insert(raw);
                        }
                    }
                    if !found.is_empty() || listed {
                        out.insert(id, found);
                    }
                }
            }
            pending.extend(doc.children(el).iter().rev());
        }
        self.scan_generation += 1;
        self.scans.insert(
            0,
            Scan {
                names: names.to_vec(),
                gates: gates.to_vec(),
                always: always.to_vec(),
                epoch,
                generation: self.scan_generation,
                result: out,
            },
        );
        if self.scans.len() > 4 {
            self.scans.pop();
        }
        (self.scans[0].result.clone(), self.scans[0].generation)
    }

    fn edges(&mut self) -> Vec<(i64, Rect)> {
        let Some(doc) = self.doc.as_ref() else { return Vec::new() };
        let cx = self.cx();
        self.elements.iter().map(|(&id, &el)| (id, border_box_of(doc, &cx, el))).collect()
    }

    // ── what the queries read ──

    fn visual_box(&mut self, id: i64) -> Rect {
        let (Some(doc), Some(el)) = (self.doc.as_ref(), self.element(id)) else { return Rect::default() };
        visual_box_of(doc, &self.cx(), el)
    }

    fn inline_boxes(&mut self, id: i64) -> Vec<Rect> {
        let (Some(doc), Some(el)) = (self.doc.as_ref(), self.element(id)) else { return Vec::new() };
        inline_boxes_of(doc, &self.cx(), el)
    }

    fn transformed_box(&mut self, id: i64, r: Rect) -> Rect {
        let (Some(doc), Some(el)) = (self.doc.as_ref(), self.element(id)) else { return r };
        transformed_box(doc, &self.cx(), el, r)
    }

    fn border_box(&mut self, id: i64) -> Rect {
        let (Some(doc), Some(el)) = (self.doc.as_ref(), self.element(id)) else { return Rect::default() };
        border_box_of(doc, &self.cx(), el)
    }

    fn borders(&mut self, id: i64) -> Option<[f32; 4]> {
        let (doc, el) = (self.doc.as_ref()?, self.element(id)?);
        let ri = doc.get_render_item(el)?;
        let b = &doc.render.item(ri).borders;
        Some([b.left.value(), b.top.value(), b.right.value(), b.bottom.value()])
    }

    fn offsets(&mut self, id: i64) -> (f64, f64, i64) {
        let (Some(doc), Some(el)) = (self.doc.as_ref(), self.element(id)) else { return (0.0, 0.0, 0) };
        let Some(ri) = doc.get_render_item(el) else { return (0.0, 0.0, 0) };
        offsets_of(doc, ri)
    }

    fn scroll_extent(&mut self, id: i64) -> ScrollExtent {
        let (Some(doc), Some(el)) = (self.doc.as_ref(), self.element(id)) else { return ScrollExtent::default() };
        let Some(ri) = doc.get_render_item(el) else { return ScrollExtent::default() };
        let cx = self.cx();
        let scroller = doc.ri_css(ri).m_overflow != Overflow::overflow_visible.index();
        let pad = &doc.render.item(ri).padding;
        let (mut right, mut bottom) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        content_extent(doc, &cx, ri, &mut right, &mut bottom, scroller);
        ScrollExtent { scroller, pad_right: pad.right.value(), pad_bottom: pad.bottom.value(), right, bottom }
    }

    fn textarea_content(&mut self, id: i64) -> Option<TextContent> {
        let (doc, el) = (self.doc.as_ref()?, self.element(id)?);
        let ri = doc.get_render_item(el)?;
        if doc.render.item(ri).kind != RenderKind::render_item_inline_context {
            return None;
        }
        let pad = &doc.render.item(ri).padding;
        let content = doc.text_content_size(ri);
        Some(TextContent {
            width: content.width.value(),
            height: content.height.value(),
            pad: [pad.left.value(), pad.top.value(), pad.right.value(), pad.bottom.value()],
        })
    }

    fn computed_style(&mut self, id: i64, pseudo: &[u8], animated: &StyleValues) -> Json {
        let (Some(doc), Some(el)) = (self.doc.as_ref(), self.element(id)) else { return Json::object() };
        let cx = self.cx();
        let element = computed_element(doc, &cx, el);
        computed::computed_style(&element, pseudo, Some(animated))
    }

    fn custom_property(&mut self, id: i64, name: &[u8]) -> Vec<u8> {
        let (Some(doc), Some(el)) = (self.doc.as_ref(), self.element(id)) else { return Vec::new() };
        let cx = self.cx();
        if !doc.kind(el).is_html_tag() {
            return Vec::new();
        }
        let Some(name_id) = cx.names.find_id(name) else { return Vec::new() };
        let vars = ElementVars { doc, cx: &cx, start: Some(el) };
        let mut tokens = Vec::new();
        let mut invalid = false;
        if !twidget_litehtml::style::VarElement::get_custom_property(&vars, name_id, &mut tokens, &mut invalid) {
            return Vec::new();
        }
        computed::custom_property_text(&tokens)
    }

    fn svg_text_box(&mut self, id: i64) -> Option<[f32; 4]> {
        let (doc, el) = (self.doc.as_ref()?, self.element(id)?);
        let b = doc.svg_text_box(&self.cx(), el)?;
        Some([b.x.value(), b.y.value(), b.width.value(), b.height.value()])
    }

    fn svg_geometry(&mut self, id: i64) -> bool {
        let (Some(doc), Some(el)) = (self.doc.as_ref(), self.element(id)) else { return false };
        let cx = self.cx();
        svg_geometry(&DocSvg { doc, cx: &cx }, el).is_some()
    }

    fn svg_path_length(&mut self, id: i64, at: f32) -> (f32, f32, f32) {
        let (Some(doc), Some(el)) = (self.doc.as_ref(), self.element(id)) else { return (0.0, 0.0, 0.0) };
        let cx = self.cx();
        let Some(path) = svg_geometry(&DocSvg { doc, cx: &cx }, el) else { return (0.0, 0.0, 0.0) };
        if at < 0.0 {
            return (svg_path_length(&path, -1.0).0, 0.0, 0.0);
        }
        let (x, y) = svg_path_length(&path, at).1.unwrap_or((0.0, 0.0));
        (0.0, x, y)
    }

    fn svg_path_bounds(&mut self, id: i64) -> Option<[f32; 4]> {
        let (doc, el) = (self.doc.as_ref()?, self.element(id)?);
        let cx = self.cx();
        let path = svg_geometry(&DocSvg { doc, cx: &cx }, el)?;
        path_bounds(&path)
    }

    // ── what the input asks ──

    fn hit(&mut self, x: f64, y: f64, placement: Option<&mut Rect>) -> i64 {
        let Some(doc) = self.doc.as_ref() else { return 0 };
        let cx = self.cx();
        let el = Self::element_at(doc, &cx, x, y);
        widget_id(doc, &cx, el, placement)
    }

    fn box_of(&mut self, id: i64) -> Rect {
        self.border_box(id)
    }

    fn touch_action(&mut self, id: i64) -> Option<Vec<u8>> {
        let (doc, el) = (self.doc.as_ref()?, self.element(id)?);
        let raw = if doc.kind(el).is_html_tag() { Some(doc.raw_declaration(&self.process.names, el, b"touch-action")) } else { None };
        Some(computed::raw(raw.as_deref(), b"auto"))
    }

    fn user_select_text(&mut self, id: i64) -> bool {
        let Some(doc) = self.doc.as_ref() else { return false };
        let mut cur = self.element(id);
        while let Some(el) = cur {
            let value = doc.el(el).css.m_user_select;
            if value != UserSelect::user_select_auto.index() {
                return value == UserSelect::user_select_text.index();
            }
            cur = doc.parent(el);
        }
        false
    }

    fn mouse(&mut self, action: i32, x: f64, y: f64, hovers: bool) -> bool {
        let cx =
            StyleCx { names: &self.process.names, container: &self.container, cache: &self.process.cache, switches: self.process.switches };
        let Some(doc) = self.doc.as_mut() else { return false };
        let mut redraw = false;
        let mut bx = |_: Position| redraw = true;
        let px = Px(x as f32);
        let py = Px(y as f32);
        match action {
            0 => {
                doc.doc_on_mouse_over(&cx, px, py, px, py, &mut bx);
                doc.doc_on_lbutton_down(&cx, px, py, px, py, &mut bx);
            }
            1 => {
                doc.doc_on_mouse_over(&cx, px, py, px, py, &mut bx);
            }
            2 => {
                doc.doc_on_lbutton_up(&cx, px, py, px, py, &mut bx);
                if !hovers {
                    doc.doc_on_mouse_leave(&cx, &mut bx);
                }
            }
            _ => {
                doc.doc_on_button_cancel(&cx, &mut bx);
                doc.doc_on_mouse_leave(&cx, &mut bx);
            }
        }
        redraw
    }

    fn leave(&mut self) -> bool {
        self.container.clear_cursor();
        let cx =
            StyleCx { names: &self.process.names, container: &self.container, cache: &self.process.cache, switches: self.process.switches };
        let Some(doc) = self.doc.as_mut() else { return false };
        let mut redraw = false;
        doc.doc_on_mouse_leave(&cx, &mut |_: Position| redraw = true);
        redraw
    }

    fn cursor(&mut self) -> Vec<u8> {
        self.container.cursor()
    }

    fn editor_style(&mut self, id: i64, clip: Rect) -> Option<EditorStyle> {
        let (doc, el) = (self.doc.as_ref()?, self.element(id)?);
        let ri = doc.get_render_item(el)?;
        if !doc.is_visible(ri) {
            return None;
        }
        let cx = self.cx();
        let css = &doc.el(el).css;
        let align = css.m_text_align;
        let mut style = EditorStyle {
            frame: rect_of(&doc.element_placement(&cx, el)),
            font_family: css.m_font_family.clone(),
            font_size: css.get_font_size().value(),
            font_weight: twidget_litehtml::cxx::f32_to_int(css.m_font_weight.val()),
            italic: css.m_font_style == FontStyle::font_style_italic.index(),
            color: color_of(&css.m_color),
            align: if align == TextAlign::text_align_center.index() {
                1
            } else if align == TextAlign::text_align_right.index() {
                2
            } else {
                0
            },
            font: self.container.font_handle(css.m_font),
            has_clip: false,
            clip,
        };
        let mut radii = Radii::default();
        if !Self::clip_of(doc, &cx, el, &mut style.has_clip, &mut style.clip, &mut radii) {
            style.has_clip = false;
        }
        Some(style)
    }

    fn image_of(&mut self, src: &[u8]) -> Option<(i32, f32, f32)> {
        self.container.image_of(src)
    }

    fn image_ready(&mut self, id: i32, w: f32, h: f32) -> bool {
        self.container.image_ready(id, w, h)
    }

    fn content_size(&mut self, id: i64) -> (f32, f32) {
        let (Some(doc), Some(el)) = (self.doc.as_ref(), self.element(id)) else { return (0.0, 0.0) };
        let content = doc.element_placement(&self.cx(), el);
        (content.width.value(), content.height.value())
    }

    fn runs_key(&mut self) -> [u64; 4] {
        let (doc, generation, layouts) = self.key();
        [doc, u64::from(generation), layouts, self.doc.as_ref().map_or(0, |d| d.layout.style_epoch)]
    }

    fn text_runs(&mut self) -> Vec<TextRun> {
        let mut runs = Vec::new();
        let Some(doc) = self.doc.as_ref() else { return runs };
        let Some(root) = doc.root else { return runs };
        let cx = self.cx();
        let mut lengths: HashMap<i64, usize> = HashMap::new();
        let mut blocks = 0i32;
        // user-select is not inherited, but `auto` follows the parent: the nearest element that says text or none decides.
        let selectable = |el: ElementId| {
            let mut cur = Some(el);
            while let Some(e) = cur {
                let value = doc.el(e).css.m_user_select;
                if value != UserSelect::user_select_auto.index() {
                    return value == UserSelect::user_select_text.index();
                }
                cur = doc.parent(e);
            }
            false
        };
        let mut add = |runs: &mut Vec<TextRun>, owner: i64, block: i32, text: Vec<u8>, bx: Option<Position>, font: u64, can: bool| {
            if text.is_empty() {
                return;
            }
            let length = lengths.entry(owner).or_insert(0);
            let mut r = TextRun { owner, offset: *length, block, font, selectable: can, line_break: bx.is_none(), ..TextRun::default() };
            if let Some(b) = bx {
                r.rect = Rect::new(b.x.value(), b.y.value(), b.width.value(), b.height.value());
            }
            *length += text.len();
            r.text = text;
            runs.push(r);
        };
        let mut pending: Vec<(ElementId, i64, i32)> = vec![(root, 0, 0)];
        while let Some((el, mut owner, mut block)) = pending.pop() {
            if !doc.is_text(el) {
                if doc.el(el).css.m_display == StyleDisplay::display_none.index() {
                    continue;
                }
                if let Some(id) = doc.get_attr(el, b"data-twid") {
                    owner = strtoll(crate::container::c_str(id));
                }
                if !doc.is_inline(el) {
                    blocks += 1;
                    block = blocks;
                }
                if doc.kind(el) == ElementKind::el_break {
                    add(&mut runs, owner, block, b"\n".to_vec(), None, 0, selectable(el));
                    continue;
                }
                for &child in doc.children(el).iter().rev() {
                    pending.push((child, owner, block));
                }
                continue;
            }
            let Some(parent) = doc.parent(el) else { continue };
            let can = selectable(parent);
            let font = doc.el(parent).css.m_font;
            let shown = doc.display_text(el);
            for ri in doc.live_renders(el) {
                let placed = doc.get_placement(&cx, ri);
                let it = doc.render.item(ri);
                let text_item = if it.kind == RenderKind::render_text { it.text.as_deref() } else { None };
                if let Some(t) = text_item.filter(|t| !t.fragments.is_empty()) {
                    // A fragment's box is in its item's frame.
                    for &fragment in &t.fragments {
                        let f = doc.render.item(fragment);
                        let mut b = f.pos;
                        b.x += placed.x - it.pos.x;
                        b.y += placed.y - it.pos.y;
                        let fragment_text = f.text.as_ref().map_or(Vec::new(), |ft| ft.fragment_text.clone());
                        add(&mut runs, owner, block, fragment_text, Some(b), font, can);
                    }
                } else {
                    let text = match text_item {
                        Some(t) if t.cut >= 0 => shown[..(t.cut as usize).min(shown.len())].to_vec(),
                        _ => shown.to_vec(),
                    };
                    add(&mut runs, owner, block, text, Some(placed), font, can);
                }
            }
        }
        runs
    }

    fn text_width(&mut self, text: &[u8], font: u64) -> f32 {
        self.container.measure(crate::container::c_str(text), font)
    }

    fn font_info(&mut self, font: u64) -> Option<(i32, f32, f32, f32)> {
        self.container.font_info(font)
    }

    fn selectable_key(&mut self) -> [u64; 2] {
        [self.doc_id, self.doc.as_ref().map_or(0, |d| d.layout.style_epoch)]
    }

    fn any_selectable(&mut self) -> bool {
        let Some(doc) = self.doc.as_ref() else { return false };
        let Some(root) = doc.root else { return false };
        let mut pending = vec![root];
        while let Some(el) = pending.pop() {
            if doc.is_text(el) || doc.el(el).css.m_display == StyleDisplay::display_none.index() {
                continue;
            }
            if doc.el(el).css.m_user_select == UserSelect::user_select_text.index() {
                return true;
            }
            pending.extend(doc.children(el).iter().rev());
        }
        false
    }

    // ── stage 4.5 ──

    fn boxes(&mut self) -> Vec<(i64, bool, BoxStyle)> {
        let mut out = Vec::new();
        let Some(doc) = self.doc.as_ref() else { return out };
        let Some(root) = doc.root else { return out };
        // The widget's elements carry their id; the renderer's own (the stopped badge) build no box of the widget's.
        let mut pending: Vec<(ElementId, i64)> = vec![(root, 0)];
        while let Some((el, parent)) = pending.pop() {
            if doc.is_text(el) || doc.is_comment(el) {
                continue;
            }
            let tag: StringId = doc.tag(el);
            if tag == __tag_before_ || tag == __tag_after_ {
                if parent != 0 && doc.el(el).css.m_display != StyleDisplay::display_none.index() {
                    let key = parent.wrapping_mul(4).wrapping_add(if tag == __tag_before_ { 1 } else { 2 });
                    out.push((key, true, Self::box_style_of(doc, el, true)));
                }
                continue;
            }
            if doc.el(el).css.m_display == StyleDisplay::display_none.index() {
                continue;
            }
            let id = doc.get_attr(el, b"data-twid").map_or(0, |a| strtoll(crate::container::c_str(a)));
            if id != 0 {
                out.push((id.wrapping_mul(4), false, Self::box_style_of(doc, el, false)));
            }
            for &child in doc.children(el).iter().rev() {
                pending.push((child, id));
            }
        }
        out
    }

    fn box_style(&mut self, id: i64) -> BoxStyle {
        let (Some(doc), Some(el)) = (self.doc.as_ref(), self.element(id)) else { return BoxStyle::default() };
        Self::box_style_of(doc, el, false)
    }

    fn background_is_local(&mut self, id: i64) -> bool {
        let (Some(doc), Some(el)) = (self.doc.as_ref(), self.element(id)) else { return false };
        if !doc.kind(el).is_html_tag() || !doc.background_color_is_local(el) {
            return false;
        }
        let tag = doc.tag(el);
        if [_input_, _select_, _textarea_, _button_, _meter_, _progress_, _fieldset_].contains(&tag) {
            return false;
        }
        let mut pending = vec![el];
        while let Some(e) = pending.pop() {
            if !doc.el(e).css.m_backdrop_filter.is_empty() {
                return false;
            }
            pending.extend_from_slice(doc.children(e));
        }
        true
    }

    fn layer_candidate(&mut self, id: i64, movers: Option<&BTreeSet<i64>>) -> Option<Rect> {
        let movers = movers.map(|m| self.element_set(m));
        let (doc, el) = (self.doc.as_ref()?, self.element(id)?);
        let ri = doc.get_render_item(el)?;
        if !doc.is_visible(ri) || (doc.is_inline(el) && !doc.is_inline_box(el)) {
            return None;
        }
        let css = &doc.el(el).css;
        if !css.m_filter.is_empty() || !css.m_backdrop_filter.is_empty() {
            return None;
        }
        let mut p = doc.render.parent(ri);
        while let Some(item) = p {
            let pel = doc.render.item(item).element;
            if doc.el(pel).css.is_paint_group() && !movers.as_ref().is_some_and(|m| m.contains(&pel)) {
                return None;
            }
            p = doc.render.parent(item);
        }
        let b = border_box_of(doc, &self.cx(), el);
        (b.w > 0.0 && b.h > 0.0 && b.w * b.h < 2_000_000.0).then_some(b)
    }

    fn clip_of(&mut self, id: i64) -> Option<Clip> {
        let (doc, el) = (self.doc.as_ref()?, self.element(id)?);
        let mut c = Clip::default();
        Self::clip_of(doc, &self.cx(), el, &mut c.has, &mut c.rect, &mut c.radii).then_some(c)
    }

    fn background_tile(&mut self, id: i64) -> Option<Tile> {
        let cx =
            StyleCx { names: &self.process.names, container: &self.container, cache: &self.process.cache, switches: self.process.switches };
        let el = self.element(id)?;
        let doc = self.doc.as_ref()?;
        let ri = doc.get_render_item(el)?;
        if doc.el_is_root(el) || doc.is_inline(el) || !doc.children(el).is_empty() || doc.el(el).css.is_paint_group() {
            return None;
        }
        // The layer goes over the document, and would cover the borders the background lies under.
        if doc.el(el).css.m_css_borders.is_visible() {
            return None;
        }
        let bg = doc.get_background(el, false)?;
        if bg.m_image.len() != 1 || bg.m_repeat.is_empty() || bg.m_repeat[0] != BackgroundRepeat::background_repeat_no_repeat.index() {
            return None;
        }
        use twidget_litehtml::background::LayerType;
        let ty = bg.get_layer_type(0);
        if ty != LayerType::type_linear_gradient && ty != LayerType::type_radial_gradient && ty != LayerType::type_conic_gradient {
            return None;
        }
        if !bg.m_origin.is_empty() && bg.m_origin[0] != BackgroundBox::background_box_padding.index() {
            return None;
        }
        if !bg.m_attachment.is_empty() && bg.m_attachment[0] != BackgroundAttachment::background_attachment_scroll.index() {
            return None;
        }
        let mut p = doc.render.parent(ri);
        while let Some(item) = p {
            let css = doc.ri_css(item);
            if css.is_paint_group() || css.m_el_position == ElementPosition::element_position_fixed.index() {
                return None;
            }
            p = doc.render.parent(item);
        }
        let bx = doc.get_placement(&cx, ri);
        let mut layer = twidget_litehtml::background::BackgroundLayer::default();
        if !bg.get_layer(doc, &cx, 0, bx, el, ri, &mut layer) {
            return None;
        }
        let mut tile = Tile {
            tile: rect_of(&layer.origin_box),
            clip: rect_of(&layer.clip_box),
            radii: radii_of(&layer.border_radius),
            ..Tile::default()
        };
        let mut padding = bx;
        padding.grow(&doc.render.item(ri).padding);
        tile.area = rect_of(&padding);
        if tile.tile.empty() || tile.clip.empty() {
            return None;
        }
        let mut whole = layer;
        whole.clip_box = layer.origin_box;
        whole.border_box = layer.origin_box;
        whole.border_radius = Default::default();
        self.container.begin_list();
        self.container.prepare_paint(doc);
        bg.draw_layer(0, 0, &whole, &self.container);
        tile.content = self.container.take_list();
        (!tile.content.is_empty()).then_some(tile)
    }

    fn declared(&mut self, id: i64, names: &[&str]) -> Declared {
        let mut out = Declared::new();
        let (Some(doc), Some(el)) = (self.doc.as_ref(), self.element(id)) else { return out };
        if !doc.kind(el).is_html_tag() {
            return out;
        }
        for name in names {
            let raw = doc.raw_declaration(&self.process.names, el, name.as_bytes());
            if !raw.is_empty() {
                out.entry(name.as_bytes().to_vec()).or_insert(raw);
            }
        }
        out
    }

    fn paint_order(&mut self) {
        if self.doc.is_none() {
            self.trace.clear();
            return;
        }
        let key = self.key();
        if Some(key) == self.trace_key {
            return;
        }
        self.trace.clear();
        if let Some(doc) = self.doc.as_mut() {
            doc.paint.trace = Some(Vec::new());
        }
        self.container.set_tracing(true);
        let _ = self.draw_plain();
        self.container.set_tracing(false);
        if let Some(doc) = self.doc.as_mut() {
            self.trace = doc.paint.trace.take().unwrap_or_default();
        }
        self.trace_key = Some(key);
    }

    fn paint_index(&mut self, id: i64) -> usize {
        let ri = match (self.doc.as_ref(), self.element(id)) {
            (Some(doc), Some(el)) => doc.get_render_item(el),
            _ => None,
        };
        match ri {
            Some(r) => self.trace.iter().position(|&t| t == r).unwrap_or(self.trace.len()),
            None => self.trace.len(),
        }
    }

    fn covered(&mut self, id: i64, area: Rect, ignore: &BTreeSet<i64>) -> bool {
        let ignore = self.element_set(ignore);
        let el = self.element(id);
        self.scan(el, area, &ignore, None, 0) != 0
    }

    fn obscurers(&mut self, id: i64, area: Rect, ignore: &BTreeSet<i64>, limit: usize) -> Option<Vec<i64>> {
        let ignore = self.element_set(ignore);
        let el = self.element(id);
        let mut out = Vec::new();
        if self.scan(el, area, &ignore, Some(&mut out), limit) < 0 {
            return None;
        }
        Some(out)
    }

    fn draw(&mut self) -> DisplayList {
        self.draw_plain()
    }

    fn draw_skipping(&mut self, skip: &[i64], backgrounds: &[i64]) -> DisplayList {
        if self.doc.is_none() {
            return DisplayList::new();
        }
        let skip: Vec<ElementId> = skip.iter().filter_map(|id| self.element(*id)).collect();
        let backgrounds: Vec<ElementId> = backgrounds.iter().filter_map(|id| self.element(*id)).collect();
        if let Some(doc) = self.doc.as_mut() {
            doc.paint.skip_paint.extend(skip);
            doc.paint.skip_background_images.extend(backgrounds);
        }
        let list = self.draw_plain();
        if let Some(doc) = self.doc.as_mut() {
            doc.paint.skip_paint.clear();
            doc.paint.skip_background_images.clear();
        }
        list
    }

    fn draw_layer(&mut self, id: i64, skip: &[i64]) -> (DisplayList, Rect) {
        self.container.begin_list();
        let el = self.element(id);
        let skip: Vec<ElementId> = skip.iter().filter_map(|id| self.element(*id)).collect();
        let cx =
            StyleCx { names: &self.process.names, container: &self.container, cache: &self.process.cache, switches: self.process.switches };
        let Some(doc) = self.doc.as_mut() else { return (DisplayList::new(), Rect::default()) };
        let Some(el) = el else { return (DisplayList::new(), Rect::default()) };
        let Some(ri) = doc.get_render_item(el) else { return (DisplayList::new(), Rect::default()) };
        for other in skip {
            if other != el {
                doc.paint.skip_paint.insert(other);
            }
        }
        self.container.prepare_paint(doc);
        let placement = doc.get_placement(&cx, ri);
        let pos = doc.render.item(ri).pos;
        let x = placement.x - pos.x;
        let y = placement.y - pos.y;
        doc.draw_element(&cx, el, 0, x, y, None, ri);
        if doc.is_visible(ri) {
            Self::draw_stacking_context(doc, &cx, ri, x, y);
        }
        doc.paint.skip_paint.clear();
        let b = border_box_of(doc, &cx, el);
        let mut redraw = Position { x: Px(b.x), y: Px(b.y), width: Px(b.w), height: Px(b.h) };
        doc.get_redraw_box(ri, &mut redraw, x, y);
        // The redraw box leaves out shadows and anti-aliasing: a margin keeps them.
        let x0 = crate::cxx::cmin(b.x, redraw.x.value()) - LAYER_MARGIN;
        let y0 = crate::cxx::cmin(b.y, redraw.y.value()) - LAYER_MARGIN;
        let x1 = cmax(b.x + b.w, redraw.x.value() + redraw.width.value()) + LAYER_MARGIN;
        let y1 = cmax(b.y + b.h, redraw.y.value() + redraw.height.value()) + LAYER_MARGIN;
        let bounds = Rect::new(x0.floor(), y0.floor(), x1.ceil() - x0.floor(), y1.ceil() - y0.floor());
        (self.container.take_list(), bounds)
    }

    fn paint_key(&mut self) -> (u64, u32) {
        let (doc, generation, _) = self.key();
        (doc, generation)
    }

    fn hit_within(&mut self, id: i64, x: f32, y: f32, placement: Option<&mut Rect>) -> i64 {
        let Some(doc) = self.doc.as_ref() else { return 0 };
        let Some(el) = self.element(id) else { return 0 };
        let Some(ri) = doc.get_render_item(el) else { return 0 };
        let cx = self.cx();
        // get_element_by_point takes the point relative to the item's parent content box.
        let p = doc.get_placement(&cx, ri);
        let sticky = doc.sticky_offset(ri);
        let pos = doc.render.item(ri).pos;
        let px = Px(x) - (p.x - pos.x - sticky.x);
        let py = Px(y) - (p.y - pos.y - sticky.y);
        let check = |d: &Document, r: RenderId| pointer_events(d, &cx, d.render.item(r).element) != b"none";
        let hit = doc.get_element_by_point(&cx, ri, px, py, Px(x), Px(y), Some(&check));
        widget_id(doc, &cx, hit, placement)
    }
}

/// What computed_style.cpp reads of an element: its cascade, and the layout boxes of its first render item and its
/// placement, its `::before` and `::after` alike.
fn computed_element(doc: &Document, cx: &StyleCx, el: ElementId) -> computed::Element {
    let mut e = computed::Element::of_cascade(doc, cx, el);
    fill_layout(doc, cx, el, &mut e);
    if doc.kind(el).is_html_tag() {
        let mut before = None;
        let mut after = None;
        for &child in doc.children(el) {
            if before.is_none() && doc.kind(child) == ElementKind::el_before {
                before = Some(child);
            }
            if after.is_none() && doc.kind(child) == ElementKind::el_after {
                after = Some(child);
            }
        }
        if let (Some(b), Some(child)) = (e.before.as_deref_mut(), before) {
            fill_layout(doc, cx, child, b);
        }
        if let (Some(a), Some(child)) = (e.after.as_deref_mut(), after) {
            fill_layout(doc, cx, child, a);
        }
    }
    e
}

fn fill_layout(doc: &Document, cx: &StyleCx, el: ElementId, e: &mut computed::Element) {
    if let Some(ri) = doc.get_render_item(el) {
        let it = doc.render.item(ri);
        let side = |m: &twidget_litehtml::types::Margins| [m.top.value(), m.right.value(), m.bottom.value(), m.left.value()];
        e.layout = Some(computed::Layout { margins: side(&it.margins), paddings: side(&it.padding), borders: side(&it.borders) });
    }
    let p = doc.element_placement(cx, el);
    e.placement = (p.width.value(), p.height.value());
}
