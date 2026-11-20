//! The document's life as renderer.cpp's `Container` leads it: a build that patches the live document from the
//! serialised HTML where the fork can and parses a new one where it cannot, the layout budget, in-place patches of
//! holes and style attributes, the master CSS a new document is parsed with, and whether a new viewport height
//! needs a second layout. The fork's calls go through [`Engine`]; past the budget the renderer stops
//! (`Renderer::stop_layout`), which the caller does when [`Container::layout`] says so.

use crate::engine::{Engine, HoleChange};

/// Boxes one layout may lay out afresh: nested flex, grid and table boxes measure their children several times
/// each, so a few hundred bytes of markup ask for 2^depth layouts. The heaviest widget of widget-sandbox's corpus
/// lays out 2,600 boxes in a pass; the base is 40 times that, and a document of many elements gets more.
pub const LAYOUT_BUDGET_BASE: u64 = 100000;
pub const LAYOUT_BUDGET_PER_ELEMENT: u64 = 50;
/// A document keeps every font it was ever styled with: one that made this many since its build is built anew.
pub const MAX_PATCHED_FONTS: usize = 4096;

/// What `Container` keeps of the document's life; the document itself is the engine's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Container {
    /// There is a document (`doc_`).
    pub(crate) doc: bool,
    pub(crate) master: Vec<u8>,
    pub(crate) vp_height: i32,
    pub(crate) rendered_width: f32,
    pub(crate) viewport_units: bool,
    pub(crate) patched: bool,
    pub(crate) layouts: u64,
    pub(crate) built_fonts: usize,
}

/// A layout ran past its budget: the boxes it laid out, for the renderer's stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exhausted(pub u64);

impl Container {
    pub fn has_document(&self) -> bool {
        self.doc
    }
    pub fn master(&self) -> &[u8] {
        &self.master
    }
    pub fn vp_height(&self) -> i32 {
        self.vp_height
    }
    pub fn rendered_width(&self) -> f32 {
        self.rendered_width
    }
    pub fn viewport_units(&self) -> bool {
        self.viewport_units
    }
    /// True when the last build patched the previous document instead of parsing a new one.
    pub fn patched(&self) -> bool {
        self.patched
    }
    /// Counts layouts: boxes, and so what can cover a layer, change only with one.
    pub fn layouts(&self) -> u64 {
        self.layouts
    }
    pub fn built_fonts(&self) -> usize {
        self.built_fonts
    }

    /// Lays the document out within the layout budget. Past it the document stays as far as it got and lays out
    /// nothing again, and the renderer stops.
    pub fn layout(&mut self, engine: &mut dyn Engine, width: f32) -> Option<Exhausted> {
        let elements = engine.element_count();
        engine.render(width, LAYOUT_BUDGET_BASE.wrapping_add(LAYOUT_BUDGET_PER_ELEMENT.wrapping_mul(elements)));
        if engine.layout_exhausted() { Some(Exhausted(engine.layout_spent())) } else { None }
    }

    /// The last layout ran past the budget: the document is laid out no more.
    pub fn exhausted(&self, engine: &mut dyn Engine) -> bool {
        self.doc && engine.layout_exhausted()
    }

    /// Drops the document: the next build parses a new one.
    pub fn drop_document(&mut self, engine: &mut dyn Engine) {
        engine.drop_document();
        self.doc = false;
    }

    /// The viewport is now `vp_height` tall, without a layout: the document does not depend on it.
    pub fn viewport_resized(&mut self, vp_height: i32) {
        self.vp_height = vp_height;
    }

    /// The master CSS of the next document. A change drops the current one: a patch keeps the master it was
    /// parsed with.
    pub fn set_master(&mut self, engine: &mut dyn Engine, css: Vec<u8>) {
        if css == self.master {
            return;
        }
        self.master = css;
        engine.drop_document();
        self.doc = false;
    }

    fn fonts_grown(&self, engine: &mut dyn Engine) -> bool {
        engine.font_count() > self.built_fonts.wrapping_add(MAX_PATCHED_FONTS)
    }

    /// False when the document is exactly as it was: same HTML, viewport and width. A layout past the budget
    /// goes to `exhausted` as it happens, for the renderer to stop there.
    pub fn build(
        &mut self,
        engine: &mut dyn Engine,
        html: &[u8],
        vp_height: i32,
        width: f64,
        exhausted: &mut dyn FnMut(Exhausted),
    ) -> bool {
        let viewport_changed = vp_height != self.vp_height;
        self.vp_height = vp_height;
        self.patched = false;
        // The fork folds viewport lengths to px as it parses, so a resize needs a fresh parse of a document that
        // has one. Media queries need no such care: a patched document re-evaluates them.
        let viewport_units = engine.has_viewport_units(html);
        let stale = (viewport_changed || width as f32 != self.rendered_width) && (viewport_units || self.viewport_units);
        self.viewport_units = viewport_units;
        // twidget's litehtml fork patches a document made by createUpdatableFromString in place when the new HTML
        // only changes what it can patch (styles, text, classes, attributes, child lists), and refuses otherwise,
        // leaving the document as it was.
        if self.doc
            && !stale
            && !self.fonts_grown(engine)
            && let Some(changed) = engine.update_from_string(html)
        {
            self.patched = true;
            if viewport_changed {
                engine.media_changed();
            }
            let width = width as f32;
            if !changed && !viewport_changed && width == self.rendered_width {
                return false;
            }
            if !engine.last_patch_paint_only() || viewport_changed || width != self.rendered_width {
                if let Some(e) = self.layout(engine, width) {
                    exhausted(e);
                }
                self.rendered_width = width;
                self.layouts = self.layouts.wrapping_add(1);
            }
            // A child-list patch replaces elements; a paint-only one cannot.
            if !engine.last_patch_paint_only() {
                engine.index();
            }
            return true;
        }
        self.doc = engine.create(html, &self.master);
        if !self.doc {
            return true;
        }
        self.rendered_width = width as f32;
        if let Some(e) = self.layout(engine, self.rendered_width) {
            exhausted(e);
        }
        self.layouts = self.layouts.wrapping_add(1);
        engine.index();
        self.built_fonts = engine.font_count();
        true
    }

    pub fn height(&self, engine: &mut dyn Engine) -> f64 {
        if self.doc { engine.height() } else { 0.0 }
    }

    /// Whether the layout can change with the viewport's height, which follows the document's: fixed boxes,
    /// percentage heights on html or body, viewport lengths, height media queries, and absolute boxes placed
    /// against the viewport. Otherwise a new height needs no second layout.
    pub fn depends_on_viewport_height(&self, engine: &mut dyn Engine, html: &[u8]) -> bool {
        if !self.doc || !engine.has_root() {
            return false;
        }
        if self.viewport_units {
            return true;
        }
        // Width queries do not care (the theme's rewrite makes every widget have some): only those that read the
        // height, its ratio to the width or the orientation.
        let mut at = find(html, b"@media", 0);
        while let Some(start) = at {
            let end = html[start..].iter().position(|&c| c == b'{').map_or(html.len(), |e| start + e);
            let query = &html[start..end];
            if find(query, b"height", 0).is_some() || find(query, b"aspect", 0).is_some() || find(query, b"orientation", 0).is_some() {
                return true;
            }
            at = find(html, b"@media", start + 6);
        }
        engine.layout_reads_viewport_height()
    }

    /// The document was laid out at the current width and viewport: a patch can go straight in.
    pub fn can_patch(&self, engine: &mut dyn Engine, width: f64) -> bool {
        self.doc && width as f32 == self.rendered_width && !self.fonts_grown(engine)
    }

    /// Attribute values and sole texts into the document in place (the fork's holes), relaying out only when a
    /// value can move boxes. False, leaving the document as it was, when a change has no hole.
    pub fn patch(&mut self, engine: &mut dyn Engine, changes: &[HoleChange], exhausted: &mut dyn FnMut(Exhausted)) -> bool {
        let Some(changed) = engine.update_holes(changes) else { return false };
        self.patched = true;
        if changed && !engine.last_patch_paint_only() {
            if let Some(e) = self.layout(engine, self.rendered_width) {
                exhausted(e);
            }
            self.layouts = self.layouts.wrapping_add(1);
        }
        true
    }

    /// Replaces the style attribute of elements in place (the fork's style holes), relaying out only when a value
    /// can move boxes. False when the document cannot patch them.
    pub fn restyle(
        &mut self,
        engine: &mut dyn Engine,
        patches: &[(i64, Vec<u8>)],
        width: f64,
        exhausted: &mut dyn FnMut(Exhausted),
    ) -> bool {
        if !self.doc || self.fonts_grown(engine) {
            return false;
        }
        let Some(changed) = engine.update_styles(patches) else { return false };
        self.patched = true;
        if changed && !engine.last_patch_paint_only() {
            self.rendered_width = width as f32;
            if let Some(e) = self.layout(engine, self.rendered_width) {
                exhausted(e);
            }
            self.layouts = self.layouts.wrapping_add(1);
        }
        true
    }
}

/// `std::string::find` from `from`.
pub(crate) fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return (from <= hay.len()).then_some(from);
    }
    if from >= hay.len() || hay.len() - from < needle.len() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|p| from + p)
}
