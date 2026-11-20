//! The renderer's seams: [`Engine`], litehtml's document as the renderer's container holds it (what the fork
//! parses, patches, lays out and answers, with the container's own parts of stage 4.2 behind it), and [`Layers`],
//! what stage 4.5 ports (heavy boxes, compositor layers, damage, the paint). Each method is one call the C++
//! makes there, asked where and in the order renderer.cpp asks it.

use std::collections::{BTreeMap, HashSet};

use twidget_animations::{Declared, StyleOverrides, StyleValues};
use twidget_events::{EditorStyle, TextRun};
use twidget_json::Json;
use twidget_shadow::Document;
use twidget_wire::{Affine, Command, Rect};

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

    /// `widget_id_at`: the widget id of what is at (x, y), the layers' tracks included; its border box into
    /// `placement` when asked for and there is one.
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
}

/// What a count of the heavy boxes left: the messages it queued (`paint`, `limit`) and whether it stopped the
/// widget past the paint limit.
#[derive(Clone, Debug, Default)]
pub struct PaintCount {
    pub messages: Vec<Json>,
    pub stopped: bool,
}

/// What stage 4.5 ports: the paint policy's heavy boxes, compositor and scripted layers, damage and the paint.
/// The renderer calls it where renderer.cpp calls that code; `dirty` and `touched_all` are the renderer's,
/// which that code sets too.
pub trait Layers {
    /// `compositing_`.
    fn compositing(&self) -> bool;
    /// `set_compositing`: false when it was so already; the composition is planned again.
    fn set_compositing(&mut self, enabled: bool) -> bool;
    /// `scripted_only`: the batch only moves scripted layers through their layer properties; the ids it moves.
    fn scripted_only(&mut self, doc: &Document, ops: &[Json], still: bool) -> Option<Vec<i64>>;
    /// `apply_scripted`: the layers moved without a build; false when one moved past what its layer allows.
    fn apply_scripted(&mut self, ids: &[i64], now: f64, dirty: &mut bool) -> bool;
    /// `recount_boxes`: the boxes of scripted layers counted with their new opacity.
    fn recount_boxes(&mut self, ids: &[i64], now: f64, still: bool) -> PaintCount;
    /// `note_scripted`: the streaks of elements moved by style ops.
    fn note_scripted(&mut self, ops: &[Json], now: f64);
    /// `note_svg`: the streaks of SVGs changed batch after batch.
    fn note_svg(&mut self, doc: &Document, ops: &[Json]);
    /// `composition_key_ = 0`.
    fn forget_key(&mut self);
    /// `composition_key_ == 0`.
    fn key_forgotten(&self) -> bool;
    /// What a rebuild does first: scripted layers follow what the build declares.
    fn rebuild_begins(&mut self);
    /// `paint_scan_ = true`: every box is read again at the next count.
    fn rescan(&mut self);
    /// `update_composition`.
    fn update_composition(&mut self, now: f64, still: bool, dirty: &mut bool, touched_all: &mut bool);
    /// `strip_layers`: what the layers animate leaves the sampled values.
    fn strip_layers(&mut self, sampled: &mut StyleOverrides);
    /// `count_paint`.
    fn count_paint(&mut self, now: f64, still: bool) -> PaintCount;
    /// What `stop_paint` does past the container's `set_no_effects`: the layers back in the document.
    fn paint_stopped(&mut self, now: f64);
    /// `wake_at_`: when a layer's track needs a frame, in seconds; infinity for never.
    fn wake_at(&self) -> f64;
    /// `layer_ids_`: the elements the compositor animates.
    fn layer_ids(&self) -> HashSet<i64>;
    /// `layer_shown`.
    fn layer_shown(&mut self, id: i64, now: f64) -> Option<(Affine, f32)>;
    /// The `policy` message: counted unless compat, and `paint` reports.
    fn policy(&mut self, on: bool, reports: bool);
    /// A canvas drew (`run_canvas` after the backend's run); `clean` when the paint was not dirty before.
    fn canvas_drew(&mut self, canvas: i32, clean: bool);
    /// A canvas was released.
    fn canvas_released(&mut self, canvas: i32);
    /// `trim`'s part: the paint summary and the last list.
    fn trim(&mut self);
    /// The rest of `paint()` after its build.
    fn paint(&mut self) -> Vec<Command>;
    /// `damage`.
    fn damage(&self, out: &mut Vec<Rect>) -> bool;
}

/// The layers of a renderer that composites nothing and counts no box: every call as the C++ makes it with
/// `compositing_` off and the policy `compat`.
#[derive(Debug, Default)]
pub struct NoLayers {
    compositing: bool,
    key_forgotten: bool,
}

impl Layers for NoLayers {
    fn compositing(&self) -> bool {
        self.compositing
    }
    fn set_compositing(&mut self, enabled: bool) -> bool {
        if enabled == self.compositing {
            return false;
        }
        self.compositing = enabled;
        self.key_forgotten = true;
        true
    }
    fn scripted_only(&mut self, _doc: &Document, _ops: &[Json], _still: bool) -> Option<Vec<i64>> {
        None
    }
    fn apply_scripted(&mut self, _ids: &[i64], _now: f64, _dirty: &mut bool) -> bool {
        true
    }
    fn recount_boxes(&mut self, _ids: &[i64], _now: f64, _still: bool) -> PaintCount {
        PaintCount::default()
    }
    fn note_scripted(&mut self, _ops: &[Json], _now: f64) {}
    fn note_svg(&mut self, _doc: &Document, _ops: &[Json]) {}
    fn forget_key(&mut self) {
        self.key_forgotten = true;
    }
    fn key_forgotten(&self) -> bool {
        self.key_forgotten
    }
    fn rebuild_begins(&mut self) {}
    fn rescan(&mut self) {}
    fn update_composition(&mut self, _now: f64, _still: bool, _dirty: &mut bool, _touched_all: &mut bool) {}
    fn strip_layers(&mut self, _sampled: &mut StyleOverrides) {}
    fn count_paint(&mut self, _now: f64, _still: bool) -> PaintCount {
        PaintCount::default()
    }
    fn paint_stopped(&mut self, _now: f64) {}
    fn wake_at(&self) -> f64 {
        f64::INFINITY
    }
    fn layer_ids(&self) -> HashSet<i64> {
        HashSet::new()
    }
    fn layer_shown(&mut self, _id: i64, _now: f64) -> Option<(Affine, f32)> {
        None
    }
    fn policy(&mut self, _on: bool, _reports: bool) {}
    fn canvas_drew(&mut self, _canvas: i32, _clean: bool) {}
    fn canvas_released(&mut self, _canvas: i32) {}
    fn trim(&mut self) {}
    fn paint(&mut self) -> Vec<Command> {
        Vec::new()
    }
    fn damage(&self, _out: &mut Vec<Rect>) -> bool {
        false
    }
}
