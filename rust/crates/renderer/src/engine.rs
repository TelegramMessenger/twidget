//! The renderer's seam [`Engine`]: litehtml's document as the renderer's container holds it (what the fork
//! parses, patches, lays out, paints and answers, with the container's own parts of stage 4.2 behind it). Each
//! method is one call the C++ makes there, asked where and in the order renderer.cpp asks it.

use std::collections::{BTreeMap, BTreeSet};

use twidget_animations::{Declared, StyleValues};
use twidget_events::{EditorStyle, TextRun};
use twidget_json::Json;
use twidget_wire::{DisplayList, Radii, Rect};

/// A hole of the fork's document template patched in place (`litehtml::document::hole_change`): an attribute
/// value of element `owner` (by its widget id), or with `text` the text it holds, byte for byte what the
/// writer puts there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HoleChange {
    pub owner: i64,
    pub name: Vec<u8>,
    pub value: Vec<u8>,
    pub text: bool,
}

/// What a `box` query reads of a scroll container: whether the item clips (`overflow` is not `visible`), its
/// end paddings, and how far its content reaches (`content_extent`, -infinity when nothing does).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollExtent {
    pub scroller: bool,
    pub pad_right: f32,
    pub pad_bottom: f32,
    pub right: f32,
    pub bottom: f32,
}

/// A textarea's text as laid out (`render_item_inline_context::text_content_size`) and its paddings, left,
/// top, right, bottom.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextContent {
    pub width: f32,
    pub height: f32,
    pub pad: [f32; 4],
}

/// litehtml's document as `Container` holds it, and what the renderer's free helpers read of its render tree.
///
/// The document's life is ported in this crate (`Container::build`, `layout` with the layout budget, `patch`,
/// `restyle`, `depends_on_viewport_height`); these are the fork's calls it makes. The container's parts of stage
/// 4.2 (fonts, images, the declaration scans, the caches a new document drops) answer here too. What the C++
/// container reads of the renderer while it parses or lays out (the width and the scale for media features, the
/// canvases a `tw-canvas:` source names) comes with that stage.
pub trait Engine {
    // ── the document's life ──

    /// `litehtml::has_viewport_units`: the HTML has a viewport length the fork folds to px as it parses.
    fn has_viewport_units(&mut self, html: &[u8]) -> bool;
    /// `document::update_from_string`: the live document patched to `html`; None when the fork refuses (the
    /// document as it was), else whether anything changed.
    fn update_from_string(&mut self, html: &[u8]) -> Option<bool>;
    /// `document::media_changed`: media queries evaluated again after a viewport change.
    fn media_changed(&mut self);
    /// `document::last_patch_paint_only`: the last patch can move no box.
    fn last_patch_paint_only(&mut self) -> bool;
    /// The old document and its element index dropped, the container's per-document caches with them, and
    /// `document::createUpdatableFromString(html, master)`; false when it made none.
    fn create(&mut self, html: &[u8], master: &[u8]) -> bool;
    /// The document and its element index dropped (`Container::drop`, a new master).
    fn drop_document(&mut self);
    /// The elements under the root, it included (the walk of `Container::layout`), 0 without one.
    fn element_count(&mut self) -> u64;
    /// `document::set_layout_budget(budget)` and `document::render(width)`.
    fn render(&mut self, width: f32, budget: u64);
    /// `document::layout_exhausted`: the last layout ran past its budget.
    fn layout_exhausted(&mut self) -> bool;
    /// `document::layout_spent`: the boxes the last layout laid out.
    fn layout_spent(&mut self) -> u64;
    /// `Container::index`: the elements with a `data-twid` by their id, again.
    fn index(&mut self);
    /// `document::height`.
    fn height(&mut self) -> f64;
    /// `document::root` is there.
    fn has_root(&mut self) -> bool;
    /// What `depends_on_viewport_height` reads of the laid-out document: fixed boxes, a percentage height or
    /// min-height on the root or the body, an absolute box no positioned ancestor contains.
    fn layout_reads_viewport_height(&mut self) -> bool;
    /// The fonts the container made for documents so far (`fonts_.size()`).
    fn font_count(&mut self) -> usize;
    /// `document::update_holes`: None when a change has no hole (the document as it was), else whether anything
    /// changed.
    fn update_holes(&mut self, changes: &[HoleChange]) -> Option<bool>;
    /// `document::update_styles` with the style attributes of elements by widget id: None when the document
    /// cannot patch them, else whether anything changed.
    fn update_styles(&mut self, patches: &[(i64, Vec<u8>)]) -> Option<bool>;
    /// `litehtml::_id_usage`: the names in the process's name table and their bytes.
    fn name_usage(&mut self) -> (u64, u64);
    /// `Container::trim`: fonts no document holds, measured widths, the paint trace, the declaration scans.
    fn trim(&mut self);
    /// `Container::set_no_effects`: past the paint limit, drawn without blurs, filters and transparency.
    fn set_no_effects(&mut self);
    /// The violations the container made during the last call (an image source it refused or one past its
    /// table's bounds), in order.
    fn take_violations(&mut self) -> Vec<Vec<u8>>;

    // ── elements ──

    /// `Container::element`: element `id` is in the document as built.
    fn has_element(&mut self, id: i64) -> bool;
    /// `Container::declarations`: the declared values of `names` on every displayed element with a widget id
    /// that declares one of `gates` or is in `always`, and the scan's generation (the same number, the same
    /// declarations; 0 for none).
    fn declarations(&mut self, names: &[Vec<u8>], gates: &[Vec<u8>], always: &[i64]) -> (BTreeMap<i64, Declared>, u64);
    /// The border box of every element of the index (`note_resize`'s walk), by id.
    fn edges(&mut self) -> Vec<(i64, Rect)>;

    // ── what the queries read ──

    /// `visual_box_of`: where element `id`'s border box shows, its transforms and its ancestors' included.
    fn visual_box(&mut self, id: i64) -> Rect;
    /// `inline_boxes_of`: the line boxes of an inline element, none for another.
    fn inline_boxes(&mut self, id: i64) -> Vec<Rect>;
    /// `transformed_box`: where `r`, in element `id`'s layout coordinates, shows.
    fn transformed_box(&mut self, id: i64, r: Rect) -> Rect;
    /// `border_box_of`.
    fn border_box(&mut self, id: i64) -> Rect;
    /// The border widths of element `id`'s render item, left, top, right, bottom; None when it has none.
    fn borders(&mut self, id: i64) -> Option<[f32; 4]>;
    /// `offsets_of`: offsetLeft, offsetTop and the offsetParent's widget id (0 for none).
    fn offsets(&mut self, id: i64) -> (f64, f64, i64);
    /// What scrollWidth and scrollHeight read of element `id`'s render item.
    fn scroll_extent(&mut self, id: i64) -> ScrollExtent;
    /// A textarea's text as laid out; None when its render item lays out no inline content.
    fn textarea_content(&mut self, id: i64) -> Option<TextContent>;
    /// `computed_style` (computed_style.cpp) of element `id` or its `pseudo`, with its animated values.
    fn computed_style(&mut self, id: i64, pseudo: &[u8], animated: &StyleValues) -> Json;
    /// `computed_custom_property`: a custom property as the cascade left it, "" when unset.
    fn custom_property(&mut self, id: i64, name: &[u8]) -> Vec<u8>;
    /// `svg_text_box`: an SVG text element's box, x, y, width, height.
    fn svg_text_box(&mut self, id: i64) -> Option<[f32; 4]>;
    /// `svg_geometry`: element `id` has an outline.
    fn svg_geometry(&mut self, id: i64) -> bool;
    /// `svg_path_length(path, at)` of its outline: the length, and for `at` ≥ 0 the point there.
    fn svg_path_length(&mut self, id: i64, at: f32) -> (f32, f32, f32);
    /// `path_bounds` of its outline, x0, y0, x1, y1; None when it has none.
    fn svg_path_bounds(&mut self, id: i64) -> Option<[f32; 4]>;

    // ── what the input asks (twidget_events::EventHost's questions of the document) ──

    /// `widget_id(element_at(x, y))`: the widget id of what the document has at (x, y), 0 for none; its border
    /// box into `placement` when asked for and there is one. The layers above it are the renderer's.
    fn hit(&mut self, x: f64, y: f64, placement: Option<&mut Rect>) -> i64;
    fn box_of(&mut self, id: i64) -> Rect;
    fn touch_action(&mut self, id: i64) -> Option<Vec<u8>>;
    fn user_select_text(&mut self, id: i64) -> bool;
    fn mouse(&mut self, action: i32, x: f64, y: f64, hovers: bool) -> bool;
    fn leave(&mut self) -> bool;
    fn cursor(&mut self) -> Vec<u8>;
    fn editor_style(&mut self, id: i64) -> Option<EditorStyle>;
    fn image_of(&mut self, src: &[u8]) -> Option<(i32, f32, f32)>;
    fn image_ready(&mut self, id: i32, w: f32, h: f32) -> bool;
    fn content_size(&mut self, id: i64) -> (f32, f32);
    fn runs_key(&mut self) -> [u64; 4];
    fn text_runs(&mut self) -> Vec<TextRun>;
    fn text_width(&mut self, text: &[u8], font: u64) -> f32;
    fn font_info(&mut self, font: u64) -> Option<(i32, f32, f32, f32)>;
    fn selectable_key(&mut self) -> [u64; 2];
    fn any_selectable(&mut self) -> bool;

    // ── stage 4.5: heavy boxes, layers, the paint ──

    /// `scan_boxes`' walk of the document: every box it builds, keyed as the paint policy counts them (a widget
    /// element's id × 4; the ::before of an element with an id + 1, its ::after + 2), whether it is a
    /// pseudo-element's, and what `box_paint_of` reads of it; in the walk's order, none without a document.
    fn boxes(&mut self) -> Vec<(i64, bool, BoxStyle)>;
    /// What `box_paint_of` reads of element `id`'s own box.
    fn box_style(&mut self, id: i64) -> BoxStyle;
    /// `Container::background_is_local`: an animated background-color on `id` repaints `id` alone.
    fn background_is_local(&mut self, id: i64) -> bool;
    /// `Container::layer_candidate`: element `id`'s border box when it can be painted on its own, inside no paint
    /// group but those of `movers` (when given).
    fn layer_candidate(&mut self, id: i64, movers: Option<&BTreeSet<i64>>) -> Option<Rect>;
    /// `Container::clip_of`: the ancestor that clips element `id`; None when an outer clip could cut what the
    /// inner one lets through.
    fn clip_of(&mut self, id: i64) -> Option<Clip>;
    /// `Container::background_tile`: a shimmer's tile, None when element `id`'s background is no lone tile.
    fn background_tile(&mut self, id: i64) -> Option<Tile>;
    /// `Container::declared`: what element `id` declares for `names` after the cascade.
    fn declared(&mut self, id: i64, names: &[&str]) -> Declared;
    /// `Container::paint_order`: the render items in the order a full paint draws them, traced again when the
    /// document laid out or its paint changed; `paint_index`, `covered` and `obscurers` read that trace.
    fn paint_order(&mut self);
    /// Where element `id`'s render item is in the trace; the trace's length when it is not there.
    fn paint_index(&mut self, id: i64) -> usize;
    /// `Container::covered`: something painted after element `id` meets `area`, but its descendants and the
    /// elements of `ignore`.
    fn covered(&mut self, id: i64, area: Rect, ignore: &BTreeSet<i64>) -> bool;
    /// `Container::obscurers`: the widget elements painted after `id` that meet `area`, as `covered` finds them;
    /// None past `limit` or for one without a widget id.
    fn obscurers(&mut self, id: i64, area: Rect, ignore: &BTreeSet<i64>, limit: usize) -> Option<Vec<i64>>;
    /// `Container::draw()`: the document's paint.
    fn draw(&mut self) -> DisplayList;
    /// `Container::draw(skip, backgrounds)`: the document without the elements of `skip` and without the
    /// background images of `backgrounds`, each in the order given.
    fn draw_skipping(&mut self, skip: &[i64], backgrounds: &[i64]) -> DisplayList;
    /// `Container::draw_layer`: element `id` and its descendants alone, without the others of `skip`; with the
    /// area the paint can reach.
    fn draw_layer(&mut self, id: i64, skip: &[i64]) -> (DisplayList, Rect);
    /// The document's half of `Container::paint_key`: which document, and its paint generation.
    fn paint_key(&mut self) -> (u64, u32);
    /// `widget_id(element_at_within(element(id), x, y))`: what element `id` has at (x, y) in document
    /// coordinates, as `hit` answers.
    fn hit_within(&mut self, id: i64, x: f32, y: f32, placement: Option<&mut Rect>) -> i64;
}

/// What `box_paint_of` reads of a box's computed values (paint_policy.h, rules 1 to 4).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoxStyle {
    /// `backdrop-filter` is set.
    pub backdrop: bool,
    /// `filter` is set.
    pub filter: bool,
    /// A box shadow is blurred.
    pub blurred: bool,
    /// A disabled or pressed `<button>`'s own box: its opacity is its own dimming.
    pub dimmed: bool,
    pub opacity: f32,
}

/// The ancestor that clips an element (`clip_of`): when there is one, the innermost one's padding box and inner
/// radii.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Clip {
    pub has: bool,
    pub rect: Rect,
    pub radii: Radii,
}

/// A shimmer's tile (`background_tile`): where it is, the padding box it slides in, the clip and radii over it,
/// and its content drawn whole.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tile {
    pub tile: Rect,
    pub area: Rect,
    pub clip: Rect,
    pub radii: Radii,
    pub content: DisplayList,
}
