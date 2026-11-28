//! Painting (stage L13): `render_item.cpp`'s paint half (`draw_stacking_context`, `draw_children` with its passes,
//! overflow clips, paint groups and fixed boxes, `draw_group` with opacity, transforms and their culling, filters,
//! backdrop filters and clip paths, `draw_borders`, the ink each item reaches and its cache by paint generation,
//! `get_visual_transform`), `document.cpp`'s `draw` and the paint state the renderer sets on the document (the
//! elements it skips or paints bare, the scroller it leaves unclipped, the paint trace), and the element classes'
//! `draw` dispatch. Each class's own drawing is in its module: `html_tag.rs` (backgrounds, shadows, borders,
//! outlines, list markers), `el_text.rs`, `el_form.rs`, `render_inline_context.rs` (ellipses), `render_table.rs`
//! (rows, cells and collapsed borders), `background.rs` (the layers), `el_svg.rs` (what an `<svg>` draws, L14),
//! `el_image.rs` (an `<img>`'s or `<canvas>`'s image over its box, L15).

use std::collections::BTreeSet;

use crate::Px;
use crate::borders::Borders;
use crate::css_effects::{FilterFunction, FilterPaint, PaintGroup, TransformKind, TransformMatrix, filter_reach};
use crate::css_properties::CssProperties;
use crate::css_values::*;
use crate::cxx;
use crate::document::{Document, StyleCx};
use crate::element::{ElementId, ElementKind};
use crate::render_item::{RenderId, RenderKind};
use crate::types::{Margins, Position};

/// What `document` keeps for painting, and what the renderer sets on it around a paint: `m_paint_generation` (from
/// 1, never 0, a new one whenever layout, styles or media change), `m_cull_paint`, `skip_paint`,
/// `skip_background_images`, `bare_effects`, `bare_shadows`, `skip_scroll_content`, `unclipped_scroll`; the
/// thread's `pixel_snap_scale()` and `g_paint_trace` of the C++, which are a document's here.
#[derive(Clone, Debug)]
pub struct DocPaint {
    pub generation: u32,
    pub cull: bool,
    /// Elements not drawn, nor what they hold.
    pub skip_paint: BTreeSet<ElementId>,
    /// Elements drawn without their background layers that are not the colour.
    pub skip_background_images: BTreeSet<ElementId>,
    /// The element whose filter, backdrop filter (and with `bare_shadows`, outer box shadows) are left out.
    pub bare_effects: Option<ElementId>,
    pub bare_shadows: bool,
    /// Elements whose children are not drawn.
    pub skip_scroll_content: BTreeSet<ElementId>,
    /// The element whose overflow does not clip.
    pub unclipped_scroll: Option<ElementId>,
    /// `pixel_snap_scale()`: what `position::round` snaps to (1 but where the Apple host sets it).
    pub snap_scale: f32,
    /// `g_paint_trace`: while set, every render item drawn, in order, and no culling.
    pub trace: Option<Vec<RenderId>>,
    /// `m_svg_use_generation` and `m_svg_use_budget`: the paint generation the `<use>` budget the document's
    /// `<svg>`s share was last filled in, and what is left of it.
    pub svg_use_generation: u32,
    pub svg_use_budget: i32,
}

impl Default for DocPaint {
    fn default() -> DocPaint {
        DocPaint {
            generation: 1,
            cull: false,
            skip_paint: BTreeSet::new(),
            skip_background_images: BTreeSet::new(),
            bare_effects: None,
            bare_shadows: false,
            skip_scroll_content: BTreeSet::new(),
            unclipped_scroll: None,
            snap_scale: 1.0,
            trace: None,
            svg_use_generation: 0,
            svg_use_budget: 0,
        }
    }
}

/// `paint_ink`: the area an item's paint reaches, or none, or no bound.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintInk {
    pub l: f32,
    pub t: f32,
    pub r: f32,
    pub b: f32,
    pub empty: bool,
    pub unbounded: bool,
}

impl Default for PaintInk {
    fn default() -> PaintInk {
        PaintInk { l: 0.0, t: 0.0, r: 0.0, b: 0.0, empty: true, unbounded: false }
    }
}

impl PaintInk {
    /// `add(left, top, right, bottom)`: the area grown over the rectangle; a NaN edge adds nothing.
    pub fn add(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        if left.is_nan() || top.is_nan() || right.is_nan() || bottom.is_nan() {
            return;
        }
        if self.empty {
            self.l = left;
            self.t = top;
            self.r = right;
            self.b = bottom;
            self.empty = false;
            return;
        }
        self.l = cxx::min_f32(self.l, left);
        self.t = cxx::min_f32(self.t, top);
        self.r = cxx::max_f32(self.r, right);
        self.b = cxx::max_f32(self.b, bottom);
    }

    /// `add(box, reach)`: the box grown by `reach` on every side.
    pub fn add_box(&mut self, bx: &Position, reach: f32) {
        self.add(
            bx.x.value() - reach,
            bx.y.value() - reach,
            bx.x.value() + bx.width.value() + reach,
            bx.y.value() + bx.height.value() + reach,
        );
    }

    /// `add(other, dx, dy)`: another area moved by `dx`, `dy`.
    pub fn add_ink(&mut self, other: &PaintInk, dx: f32, dy: f32) {
        if other.unbounded {
            self.unbounded = true;
        } else if !other.empty {
            self.add(other.l + dx, other.t + dy, other.r + dx, other.b + dy);
        }
    }

    /// `misses(clip, dx, dy)`: the area moved by `dx`, `dy` is wholly outside the clip.
    pub fn misses(&self, clip: &Position, dx: f32, dy: f32) -> bool {
        if self.unbounded {
            return false;
        }
        if self.empty {
            return true;
        }
        self.r + dx < clip.x.value()
            || self.l + dx > clip.x.value() + clip.width.value()
            || self.b + dy < clip.y.value()
            || self.t + dy > clip.y.value() + clip.height.value()
    }
}

/// A render item's `m_ink`, `m_drawn_ink` (the ink through its transform) and `m_ink_generation`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InkCache {
    pub ink: PaintInk,
    pub drawn: PaintInk,
    pub generation: u32,
}

/// `draw_flag`: which of a stacking context's passes `draw_children` draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawFlag {
    draw_root,
    draw_block,
    draw_floats,
    draw_inlines,
    draw_positioned,
}

/// `group_transform(el_css, border_box)`: the element's `transform` about its origin over the border box.
pub fn group_transform(el_css: &CssProperties, border_box: &Position) -> TransformMatrix {
    let ox = border_box.x + el_css.m_transform_origin_x.calc_percent(border_box.width);
    let oy = border_box.y + el_css.m_transform_origin_y.calc_percent(border_box.height);
    let mut m = TransformMatrix::new(1.0, 0.0, 0.0, 1.0, -ox.value(), -oy.value());
    for it in el_css.m_transform.iter().rev() {
        let mut f = TransformMatrix::default();
        match it.kind {
            TransformKind::translate => {
                f.tx = it.x.calc_percent(border_box.width).value();
                f.ty = it.y.calc_percent(border_box.height).value();
            }
            TransformKind::scale => {
                f.a = it.v[0];
                f.d = it.v[1];
            }
            TransformKind::rotate => {
                f.a = it.v[0].cos();
                f.b = it.v[0].sin();
                f.c = -f.b;
                f.d = f.a;
            }
            TransformKind::skew => {
                f.b = it.v[1].tan();
                f.c = it.v[0].tan();
            }
            TransformKind::matrix => {
                f = TransformMatrix::new(it.v[0], it.v[1], it.v[2], it.v[3], it.v[4], it.v[5]);
            }
            TransformKind::rotate_x => {
                f.d = it.v[0].cos();
            }
            TransformKind::rotate_y => {
                f.a = it.v[0].cos();
            }
        }
        m = m.then(&f);
    }
    m.then(&TransformMatrix::new(1.0, 0.0, 0.0, 1.0, ox.value(), oy.value()))
}

/// `shadow_reach(shadows)`: how far the outer shadows reach past their box.
pub fn shadow_reach(shadows: &[crate::css_effects::BoxShadow]) -> f32 {
    let mut reach = 0.0f32;
    for shadow in shadows {
        if !shadow.inset {
            reach = cxx::max_f32(
                reach,
                shadow.blur.val().abs() * 2.0 + shadow.spread.val().abs() + cxx::max_f32(shadow.x.val().abs(), shadow.y.val().abs()),
            );
        }
    }
    reach
}

impl Document {
    // ---- The document's paint state ------------------------------------------------------------------------------

    /// `invalidate_paint()`: a new paint generation (0 skipped).
    pub fn invalidate_paint(&mut self) {
        let g = self.paint.generation.wrapping_add(1);
        self.paint.generation = if g == 0 { 1 } else { g };
    }

    /// `paint_generation()`.
    pub fn paint_generation(&self) -> u32 {
        self.paint.generation
    }

    /// `culls_paint()`.
    pub fn culls_paint(&self) -> bool {
        self.paint.cull
    }

    /// `svg_use_budget(refill)`: the `<use>` budget the document's `<svg>`s share in this paint generation,
    /// refilled to `refill` in a new one.
    pub fn svg_use_budget(&mut self, refill: i32) -> i32 {
        if self.paint.svg_use_generation != self.paint.generation {
            self.paint.svg_use_generation = self.paint.generation;
            self.paint.svg_use_budget = refill;
        }
        self.paint.svg_use_budget
    }

    /// `g_paint_trace->push_back(ri)` when tracing.
    fn trace_paint(&mut self, ri: RenderId) {
        if let Some(trace) = self.paint.trace.as_mut() {
            trace.push(ri);
        }
    }

    // ---- What painting asks of render items ----------------------------------------------------------------------

    /// `for_inline_boxes(process)` of the item's class: an inline's boxes (first and last marked), a table row's cells'
    /// boxes; false (and no call) for the classes that have none (the base class, a text). `process` stops the walk
    /// by answering false.
    pub fn for_inline_boxes(&self, id: RenderId, process: &mut dyn FnMut(&Position, bool, bool) -> bool) -> bool {
        let it = self.render.item(id);
        match it.kind {
            RenderKind::render_item_inline => {
                let n = it.inline_boxes.len();
                for (k, b) in it.inline_boxes.iter().enumerate() {
                    if !process(b, k == 0, k + 1 == n) {
                        break;
                    }
                }
                true
            }
            RenderKind::render_item_table_row => self.table_row_for_inline_boxes(id, process),
            _ => false,
        }
    }

    /// `get_draw_vertical_offset()` of the item's class: a table's top captions' height, 0 for the others.
    pub fn get_draw_vertical_offset(&self, id: RenderId) -> Px {
        if self.render.item(id).kind == RenderKind::render_item_table {
            return self.table_draw_vertical_offset(id);
        }
        Px(0.0)
    }

    /// `get_visual_transform()`: the element's transform over its placed border box; none for an inline that is not
    /// an inline box or replaced.
    pub fn get_visual_transform(&self, cx: &StyleCx, id: RenderId) -> TransformMatrix {
        let el = self.render.item(id).element;
        let css = self.ri_css(id);
        if css.m_transform.is_empty() || (self.is_inline(el) && !self.is_inline_box(el) && !self.is_replaced(el)) {
            return TransformMatrix::default();
        }
        let mut bx = self.get_placement(cx, id);
        let it = self.render.item(id);
        bx.grow(&it.padding);
        bx.grow(&it.borders);
        group_transform(self.ri_css(id), &bx)
    }

    // ---- Ink ----------------------------------------------------------------------------------------------------

    /// `add_self_ink(ink, dx, dy)`: the item's own paint moved by `dx`, `dy`: a text's box grown by half its height
    /// (at least 4) and its parent's text shadows; the border box (and an inline's boxes) grown by the outer box
    /// shadows and the outline, 2 px more.
    pub fn add_self_ink(&self, id: RenderId, ink: &mut PaintInk, dx: f32, dy: f32) {
        let slack = 2.0f32;
        let it = self.render.item(id);
        let el = it.element;
        let el_css = &self.el(el).css;
        let mut bx = it.pos;
        bx.x += dx;
        bx.y += dy;
        if self.is_text(el) {
            let mut reach = cxx::max_f32(4.0, bx.height.value() / 2.0) + slack;
            if let Some(parent_el) = self.parent(el) {
                reach += shadow_reach(&self.el(parent_el).css.m_text_shadows);
            }
            ink.add_box(&bx, reach);
        }
        bx.grow(&it.padding);
        bx.grow(&it.borders);
        let mut reach = shadow_reach(&el_css.m_box_shadows);
        let stroke = el_css.m_outline.width.val() + cxx::max_f32(0.0, el_css.m_outline_offset.val());
        if stroke > 0.0 {
            reach += stroke;
        }
        ink.add_box(&bx, reach + slack);
        self.for_inline_boxes(id, &mut |inline_box, _, _| {
            let mut shifted = *inline_box;
            shifted.x += dx;
            shifted.y += dy;
            ink.add_box(&shifted, reach + slack);
            true
        });
    }

    /// `add_children_ink(generation, ink)` of the item's class: each child's drawn ink at the item's place less its
    /// scroll position (stopping once unbounded); a table's captions, rows and cells instead.
    fn add_children_ink(&mut self, id: RenderId, generation: u32, ink: &mut PaintInk) {
        if self.render.item(id).kind == RenderKind::render_item_table {
            self.table_add_children_ink(id, generation, ink);
            return;
        }
        let dx = (self.render.item(id).pos.x - self.get_scroll_left(id)).value();
        let dy = (self.render.item(id).pos.y - self.get_scroll_top(id)).value();
        let n = self.render.item(id).children.len();
        for k in 0..n {
            let el = self.render.item(id).children[k];
            let child = self.ink(el, generation, true);
            ink.add_ink(&child, dx, dy);
            if ink.unbounded {
                return;
            }
        }
    }

    /// `ink(generation, drawn)`: what the item and its children reach, made once a paint generation: its own and its
    /// children's, grown by its filter's reach; no bound for a fixed or sticky box, nor for a list item's image
    /// marker, a list item's marker room on its left otherwise; the drawn ink is that through its transform.
    pub fn ink(&mut self, id: RenderId, generation: u32, drawn: bool) -> PaintInk {
        if self.render.item(id).ink.generation != generation {
            self.render.item_mut(id).ink.generation = generation;
            let mut ink = PaintInk::default();
            self.add_self_ink(id, &mut ink, 0.0, 0.0);
            self.add_children_ink(id, generation, &mut ink);
            let cache = self.finish_ink(id, ink);
            let item = self.render.item_mut(id);
            item.ink.ink = cache.0;
            item.ink.drawn = cache.1;
        }
        let cache = &self.render.item(id).ink;
        if drawn { cache.drawn } else { cache.ink }
    }

    /// The rest of `ink()` after the children: the filter, fixed and sticky boxes, list markers, the transform.
    #[inline(never)]
    fn finish_ink(&self, id: RenderId, mut ink: PaintInk) -> (PaintInk, PaintInk) {
        let it = self.render.item(id);
        let el_css = self.ri_css(id);
        if !el_css.m_filter.is_empty() && !ink.empty && !ink.unbounded {
            let reach = filter_reach(&el_css.m_filter);
            ink.add(ink.l - reach, ink.t - reach, ink.r + reach, ink.b + reach);
        }
        if el_css.m_el_position == ElementPosition::element_position_fixed.index()
            || el_css.m_el_position == ElementPosition::element_position_sticky.index()
        {
            ink.unbounded = true;
        }
        if !ink.unbounded
            && !ink.empty
            && el_css.m_display == StyleDisplay::display_list_item.index()
            && (el_css.m_list_style_type != ListStyleType::list_style_type_none.index() || !el_css.m_list_style_image.is_empty())
        {
            if !el_css.m_list_style_image.is_empty() {
                ink.unbounded = true;
            } else {
                let font = cxx::max_f32(el_css.get_font_size().value(), el_css.m_font_metrics.height.value());
                let pad = cxx::max_f32(font, el_css.m_line_height.computed_value.value()) * 2.0 + 4.0;
                ink.add(f32::NEG_INFINITY, ink.t - pad, it.pos.x.value() + font + 4.0, ink.b + pad);
            }
        }
        let mut drawn = ink;
        if !ink.unbounded && !ink.empty && !el_css.m_transform.is_empty() {
            let mut border_box = it.pos;
            border_box.grow(&it.padding);
            border_box.grow(&it.borders);
            let m = group_transform(el_css, &border_box);
            drawn = PaintInk::default();
            for x in [ink.l, ink.r] {
                for y in [ink.t, ink.b] {
                    let px = m.a * x + m.c * y + m.tx;
                    let py = m.b * x + m.d * y + m.ty;
                    if !px.is_finite() || !py.is_finite() {
                        drawn.unbounded = true;
                    }
                    drawn.add(px, py, px, py);
                }
            }
        }
        (ink, drawn)
    }

    // ---- Drawing ------------------------------------------------------------------------------------------------

    /// `document::draw(hdc, x, y, clip, cull)`: the root's group when it has a clip path, else the root drawn and its
    /// stacking context; culling against the clip while asked for (and not tracing).
    pub fn draw(&mut self, cx: &StyleCx, hdc: u64, x: Px, y: Px, clip: Option<&Position>, cull: bool) {
        let (Some(root), Some(root_render)) = (self.root, self.render.root) else { return };
        let saved = self.paint.cull;
        self.paint.cull = cull && clip.is_some() && self.paint.trace.is_none();
        if !self.el(root).css.m_clip_path.empty() {
            self.draw_group(cx, hdc, x, y, clip, root_render);
        } else {
            self.draw_element(cx, root, hdc, x, y, clip, root_render);
            self.draw_stacking_context(cx, root_render, hdc, x, y, clip, true);
        }
        self.paint.cull = saved;
    }

    /// `element::draw(hdc, x, y, clip, ri)` of the element's class.
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    pub fn draw_element(&mut self, cx: &StyleCx, el: ElementId, hdc: u64, x: Px, y: Px, clip: Option<&Position>, ri: RenderId) {
        match self.kind(el) {
            ElementKind::el_text | ElementKind::el_space => self.el_text_draw(cx, el, hdc, x, y, clip, ri),
            ElementKind::el_input => self.el_input_draw(cx, el, hdc, x, y, clip, ri),
            ElementKind::el_select => self.el_select_draw(cx, el, hdc, x, y, clip, ri),
            ElementKind::el_button => self.el_button_draw(cx, el, hdc, x, y, clip, ri),
            ElementKind::el_textarea => self.el_textarea_draw(cx, el, hdc, x, y, clip, ri),
            ElementKind::el_meter => self.el_meter_draw(cx, el, hdc, x, y, clip, ri),
            ElementKind::el_fieldset => self.el_fieldset_draw(cx, el, hdc, x, y, clip, ri),
            ElementKind::el_image => self.el_image_draw(cx, el, hdc, x, y, clip, ri),
            ElementKind::el_svg => self.el_svg_draw(cx, el, hdc, x, y, clip, ri),
            kind if kind.is_html_tag() => self.html_tag_draw(cx, el, hdc, x, y, clip, ri),
            _ => {}
        }
    }

    /// `element::draw_background(hdc, x, y, clip, ri)` of the element's class: an `html_tag`'s, nothing for the others.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_element_background(&self, cx: &StyleCx, el: ElementId, hdc: u64, x: Px, y: Px, clip: Option<&Position>, ri: RenderId) {
        if self.kind(el).is_html_tag() {
            self.draw_background(cx, el, hdc, x, y, clip, ri);
        }
    }

    /// `render_item::draw_borders(hdc, border_box)`: the element's borders but the sides a collapsed table left to
    /// a neighbour (their style none), on the rounded box, when a side has a width.
    pub fn draw_borders(&self, cx: &StyleCx, id: RenderId, hdc: u64, mut border_box: Position) {
        let it = self.render.item(id);
        let el = it.element;
        let css = &self.el(el).css;
        let mut bdr = Borders::from(&css.m_css_borders);
        let none = BorderStyle::border_style_none.index();
        if it.collapse_skip & 1 != 0 {
            bdr.left.style = none;
        }
        if it.collapse_skip & 2 != 0 {
            bdr.top.style = none;
        }
        if it.collapse_skip & 4 != 0 {
            bdr.right.style = none;
        }
        if it.collapse_skip & 8 != 0 {
            bdr.bottom.style = none;
        }
        if bdr.is_visible() {
            border_box.round(self.paint.snap_scale);
            bdr.radius = css.m_css_borders.radius.calc_percents(border_box.width, border_box.height);
            cx.container.draw_borders(hdc, &bdr, &border_box, self.el_is_root(el));
        }
    }

    /// `draw_stacking_context(hdc, x, y, clip, with_positioned)`: the item's children that are not a stacking
    /// context of their own: its positioned ones of negative `z-index` (by z-index), its blocks, floats and inlines,
    /// then its positioned ones of `z-index` 0, then positive.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_stacking_context(
        &mut self,
        cx: &StyleCx,
        id: RenderId,
        hdc: u64,
        x: Px,
        y: Px,
        clip: Option<&Position>,
        with_positioned: bool,
    ) {
        if !self.is_visible(id) {
            return;
        }
        let z_indexes: BTreeSet<i32> = if with_positioned {
            self.render.item(id).positioned.iter().map(|&p| self.ri_css(p).get_z_index()).collect()
        } else {
            BTreeSet::new()
        };
        for &z in z_indexes.iter().filter(|&&z| z < 0) {
            self.draw_children(cx, id, hdc, x, y, clip, DrawFlag::draw_positioned, z);
        }
        self.draw_children(cx, id, hdc, x, y, clip, DrawFlag::draw_block, 0);
        self.draw_children(cx, id, hdc, x, y, clip, DrawFlag::draw_floats, 0);
        self.draw_children(cx, id, hdc, x, y, clip, DrawFlag::draw_inlines, 0);
        if z_indexes.contains(&0) {
            self.draw_children(cx, id, hdc, x, y, clip, DrawFlag::draw_positioned, 0);
        }
        for &z in z_indexes.iter().filter(|&&z| z > 0) {
            self.draw_children(cx, id, hdc, x, y, clip, DrawFlag::draw_positioned, z);
        }
    }

    /// `group_z_indexes(item, z)`: the z-indexes of a paint group's positioned descendants, short of those inside
    /// another positioned element or paint group, found in the tree.
    fn group_z_indexes(&self, item: RenderId, z: &mut BTreeSet<i32>) {
        for &child in self.render.children(item) {
            let el = self.render.item(child).element;
            if self.is_positioned(el) {
                z.insert(self.el(el).css.get_z_index());
            } else if !self.el(el).css.is_paint_group() {
                self.group_z_indexes(child, z);
            }
        }
    }

    /// `draw_group_contents(el, hdc, x, y, clip)`: a paint group's stacking context, its positioned descendants of
    /// negative z-index first and the others after its blocks, floats and inlines.
    fn draw_group_contents(&mut self, cx: &StyleCx, el: RenderId, hdc: u64, x: Px, y: Px, clip: Option<&Position>) {
        if !self.is_visible(el) {
            return;
        }
        let mut z = BTreeSet::new();
        self.group_z_indexes(el, &mut z);
        for &index in z.iter().filter(|&&index| index < 0) {
            self.draw_children(cx, el, hdc, x, y, clip, DrawFlag::draw_positioned, index);
        }
        self.draw_children(cx, el, hdc, x, y, clip, DrawFlag::draw_block, 0);
        self.draw_children(cx, el, hdc, x, y, clip, DrawFlag::draw_floats, 0);
        self.draw_children(cx, el, hdc, x, y, clip, DrawFlag::draw_inlines, 0);
        for &index in z.iter().filter(|&&index| index >= 0) {
            self.draw_children(cx, el, hdc, x, y, clip, DrawFlag::draw_positioned, index);
        }
    }

    /// `draw_group(hdc, x, y, clip, el)`: a paint group (opacity, transform, filter, backdrop filter, clip path,
    /// sticky, backface) as one: its bounds (its redraw box, grown to its ink under opacity and by its filter's
    /// reach), `begin_group`, its clip path, the clip mapped into a transformed group for culling, its backdrop,
    /// then the element and its contents (through `draw_filtered` with a filter), and `end_group`.
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    pub fn draw_group(&mut self, cx: &StyleCx, hdc: u64, x: Px, y: Px, clip: Option<&Position>, el: RenderId) {
        let sticky = self.sticky_offset(el);
        let x = x + sticky.x;
        let y = y + sticky.y;
        let it = self.render.item(el);
        let element = it.element;
        let mut border_box = it.pos;
        border_box.x += x;
        border_box.y += y;
        border_box.grow(&it.padding);
        border_box.grow(&it.borders);
        let el_css = &self.el(element).css;

        let mut group = PaintGroup {
            opacity: el_css.m_opacity,
            backface_hidden: el_css.m_backface_hidden != 0,
            owner: Some(element),
            bounds: border_box,
            ..PaintGroup::default()
        };
        self.get_redraw_box(el, &mut group.bounds, x, y);
        if group.opacity < 1.0 {
            let generation = self.paint_generation();
            let group_ink = self.ink(el, generation, false);
            if !group_ink.empty && !group_ink.unbounded {
                let l = cxx::min_f32(group.bounds.left().value(), group_ink.l + x.value());
                let t = cxx::min_f32(group.bounds.top().value(), group_ink.t + y.value());
                let r = cxx::max_f32(group.bounds.right().value(), group_ink.r + x.value());
                let b = cxx::max_f32(group.bounds.bottom().value(), group_ink.b + y.value());
                group.bounds = Position::new(Px(l), Px(t), Px(r - l), Px(b - t));
                group.exact_bounds = true;
            }
        }
        let el_css = &self.el(element).css;
        if !el_css.m_transform.is_empty() {
            group.matrix = group_transform(el_css, &border_box);
        }

        let filter: Vec<FilterFunction> = el_css.m_filter.clone();
        let backdrop: Vec<FilterFunction> = el_css.m_backdrop_filter.clone();
        let bare = self.paint.bare_effects == Some(element);
        if !filter.is_empty() && !bare {
            let reach = filter_reach(&filter);
            group.bounds.x -= reach;
            group.bounds.y -= reach;
            group.bounds.width += reach * 2.0;
            group.bounds.height += reach * 2.0;
        }

        cx.container.begin_group(hdc, &group);
        let shaped = !el_css.m_clip_path.empty();
        if shaped {
            cx.container.set_clip_path(&el_css.m_clip_path.resolve(&border_box));
        }
        let mut inner_clip = if group.matrix.is_identity() { clip } else { None };
        let transformed_clip;
        if let Some(clip) = clip
            && cx.switches.transformed_culling
            && inner_clip.is_none()
            && self.culls_paint()
            && filter.is_empty()
            && backdrop.is_empty()
            && let Some(mapped) = transformed_clip_of(&group.matrix, clip)
        {
            transformed_clip = mapped;
            inner_clip = Some(&transformed_clip);
        }
        self.trace_paint(el);
        if !backdrop.is_empty() && !bare {
            let el_css = &self.el(element).css;
            let mut radius = el_css.m_css_borders.radius.calc_percents(border_box.width, border_box.height);
            radius.fix_values_in(border_box.width, border_box.height);
            let paint = FilterPaint { steps: &backdrop, bounds: border_box, r#box: border_box, radius, owner: Some(element) };
            cx.container.draw_backdrop(hdc, &paint);
        }
        if !filter.is_empty() && !bare {
            let generation = self.paint_generation();
            let ink = self.ink(el, generation, false);
            let mut paint = FilterPaint {
                steps: &filter,
                bounds: border_box,
                r#box: border_box,
                radius: crate::borders::BorderRadiuses::default(),
                owner: Some(element),
            };
            if !ink.empty && !ink.unbounded {
                paint.bounds = Position::new(Px(ink.l) + x, Px(ink.t) + y, Px(ink.r - ink.l), Px(ink.b - ink.t));
            }
            cx.container.draw_filtered(hdc, &paint, &mut |target| {
                self.draw_element(cx, element, target, x, y, None, el);
                self.draw_group_contents(cx, el, target, x, y, None);
            });
        } else {
            self.draw_element(cx, element, hdc, x, y, inner_clip, el);
            self.draw_group_contents(cx, el, hdc, x, y, inner_clip);
        }
        if shaped {
            cx.container.del_clip();
        }
        cx.container.end_group(hdc);
    }

    /// `draw_children(hdc, x, y, clip, flag, zindex)` of the item's class: an inline context's adds its ellipses, a
    /// table's draws its captions, rows and cells.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_children(
        &mut self,
        cx: &StyleCx,
        id: RenderId,
        hdc: u64,
        x: Px,
        y: Px,
        clip: Option<&Position>,
        flag: DrawFlag,
        zindex: i32,
    ) {
        match self.render.item(id).kind {
            RenderKind::render_item_inline_context => self.inline_context_draw_children(cx, id, hdc, x, y, clip, flag, zindex),
            RenderKind::render_item_table => self.table_draw_children(cx, id, hdc, x, y, clip, flag, zindex),
            _ => self.render_item_draw_children(cx, id, hdc, x, y, clip, flag, zindex),
        }
    }

    /// `render_item::draw_children(hdc, x, y, clip, flag, zindex)`: the children of the pass at the item's place less
    /// its scroll position, inside its overflow clip (its padding box, radii less its borders; none for an inline,
    /// nor for the scroller the renderer leaves unclipped): what the renderer skips and what culling finds outside
    /// the clip left out; a paint group drawn whole in its own pass (a fixed one at 0, 0 and outside the clips unless
    /// a group holds it); a positioned child of the z-index drawn with its stacking context (a fixed one at 0, 0,
    /// escaping the clips likewise), a block drawn, a float and an inline box drawn with their stacking contexts, an
    /// inline drawn; the children of what was not drawn whole looked into for the same pass.
    #[allow(clippy::too_many_arguments)]
    pub fn render_item_draw_children(
        &mut self,
        cx: &StyleCx,
        id: RenderId,
        hdc: u64,
        x: Px,
        y: Px,
        clip: Option<&Position>,
        flag: DrawFlag,
        zindex: i32,
    ) {
        let element = self.render.item(id).element;
        if !self.paint.skip_scroll_content.is_empty() && self.paint.skip_scroll_content.contains(&element) {
            return;
        }
        let mut pos = self.render.item(id).pos;
        pos.x += x - self.get_scroll_left(id);
        pos.y += y - self.get_scroll_top(id);

        let clips = self.el(element).css.clips_contents() && Some(element) != self.paint.unclipped_scroll;
        let inline = self.el(element).css.m_display == StyleDisplay::display_inline.index();
        if clips && !inline {
            self.set_overflow_clip(cx, id, x, y);
        }

        let cull = clip.is_some() && self.culls_paint();
        let generation = self.paint_generation();
        let n = self.render.item(id).children.len();
        for k in 0..n {
            let el = self.render.item(id).children[k];
            let child_element = self.render.item(el).element;
            if !self.paint.skip_paint.is_empty() && self.paint.skip_paint.contains(&child_element) {
                continue;
            }
            if let Some(clip) = clip
                && cull
                && self.ink(el, generation, true).misses(clip, pos.x.value(), pos.y.value())
            {
                continue;
            }
            if self.is_visible(el) {
                self.draw_child(cx, el, hdc, &pos, clip, flag, zindex);
            }
        }

        if clips && !inline {
            cx.container.del_clip();
        }
    }

    /// The overflow clip of `draw_children`: the padding box at `x`, `y`, the radii less the borders.
    #[inline(never)]
    fn set_overflow_clip(&self, cx: &StyleCx, id: RenderId, x: Px, y: Px) {
        let it = self.render.item(id);
        let mut clip_box = it.pos;
        clip_box.x += x;
        clip_box.y += y;
        clip_box.grow(&it.padding);
        let mut border_box = clip_box;
        border_box.grow(&it.borders);
        let mut bdr_radius = self.ri_css(id).m_css_borders.radius.calc_percents(border_box.width, border_box.height);
        bdr_radius.shrink(&it.borders);
        cx.container.set_clip(&clip_box, &bdr_radius);
    }

    /// One visible child of `draw_children` at `pos`, its parent's place.
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    fn draw_child(&mut self, cx: &StyleCx, el: RenderId, hdc: u64, pos: &Position, clip: Option<&Position>, flag: DrawFlag, zindex: i32) {
        let element = self.render.item(el).element;
        let el_css = &self.el(element).css;
        let positioned = self.is_positioned(element);
        let floats = el_css.m_float != ElementFloat::float_none.index();
        let inline = self.is_inline(element);
        let fixed = el_css.m_el_position == ElementPosition::element_position_fixed.index();
        let display = el_css.m_display;
        if el_css.is_paint_group() {
            let own_pass = match flag {
                DrawFlag::draw_positioned => positioned && el_css.get_z_index() == zindex,
                DrawFlag::draw_block => !positioned && !inline && !floats,
                DrawFlag::draw_floats => !positioned && floats,
                DrawFlag::draw_inlines => !positioned && inline && !floats,
                DrawFlag::draw_root => false,
            };
            if own_pass {
                let escape = fixed && !self.inside_paint_group(el);
                if escape {
                    cx.container.suspend_clips();
                }
                let (gx, gy) = if fixed { (Px(0.0), Px(0.0)) } else { (pos.x, pos.y) };
                self.draw_group(cx, hdc, gx, gy, clip, el);
                if escape {
                    cx.container.resume_clips();
                }
            }
            // The group draws its positioned descendants itself, inside its opacity, filter and transform.
            return;
        }
        let mut process = true;
        match flag {
            DrawFlag::draw_positioned => {
                if positioned && el_css.get_z_index() == zindex {
                    if fixed {
                        // Fixed elements position is always relative to the (0,0)
                        self.trace_paint(el);
                        let escape = !self.inside_paint_group(el);
                        if escape {
                            cx.container.suspend_clips();
                        }
                        self.draw_element(cx, element, hdc, Px(0.0), Px(0.0), clip, el);
                        self.draw_stacking_context(cx, el, hdc, Px(0.0), Px(0.0), clip, true);
                        if escape {
                            cx.container.resume_clips();
                        }
                    } else {
                        self.trace_paint(el);
                        self.draw_element(cx, element, hdc, pos.x, pos.y, clip, el);
                        self.draw_stacking_context(cx, el, hdc, pos.x, pos.y, clip, true);
                    }
                    process = false;
                }
            }
            DrawFlag::draw_block => {
                if !inline && !floats && !positioned {
                    self.trace_paint(el);
                    self.draw_element(cx, element, hdc, pos.x, pos.y, clip, el);
                }
            }
            DrawFlag::draw_floats => {
                if floats && !positioned {
                    self.trace_paint(el);
                    self.draw_element(cx, element, hdc, pos.x, pos.y, clip, el);
                    self.draw_stacking_context(cx, el, hdc, pos.x, pos.y, clip, false);
                    process = false;
                }
            }
            DrawFlag::draw_inlines => {
                if inline && !floats && !positioned {
                    self.trace_paint(el);
                    self.draw_element(cx, element, hdc, pos.x, pos.y, clip, el);
                    if display == StyleDisplay::display_inline_block.index()
                        || display == StyleDisplay::display_inline_flex.index()
                        || display == StyleDisplay::display_inline_grid.index()
                    {
                        self.draw_stacking_context(cx, el, hdc, pos.x, pos.y, clip, false);
                        process = false;
                    }
                }
            }
            DrawFlag::draw_root => {}
        }

        if process {
            if flag == DrawFlag::draw_positioned {
                if !positioned {
                    self.draw_children(cx, el, hdc, pos.x, pos.y, clip, flag, zindex);
                }
            } else if !floats && display != StyleDisplay::display_inline_block.index() && !positioned {
                self.draw_children(cx, el, hdc, pos.x, pos.y, clip, flag, zindex);
            }
        }
    }

    /// `inside_paint_group(el)`: an ancestor of the item is a paint group.
    pub fn inside_paint_group(&self, el: RenderId) -> bool {
        let mut p = self.render.parent(el);
        while let Some(item) = p {
            if self.ri_css(item).is_paint_group() {
                return true;
            }
            p = self.render.parent(item);
        }
        false
    }
}

/// `draw_group`'s culling clip for a transformed group: the clip (one pixel larger) mapped back through the matrix,
/// floored and ceiled, when the matrix inverts and the result is finite and within a million pixels.
fn transformed_clip_of(m: &TransformMatrix, clip: &Position) -> Option<Position> {
    let determinant = f64::from(m.a) * f64::from(m.d) - f64::from(m.b) * f64::from(m.c);
    if !(determinant.is_finite() && determinant.abs() > 1e-8) {
        return None;
    }
    let (mut left, mut top, mut right, mut bottom) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for cx in [f64::from(clip.left().value()) - 1.0, f64::from(clip.right().value()) + 1.0] {
        for cy in [f64::from(clip.top().value()) - 1.0, f64::from(clip.bottom().value()) + 1.0] {
            let px = (f64::from(m.d) * (cx - f64::from(m.tx)) - f64::from(m.c) * (cy - f64::from(m.ty))) / determinant;
            let py = (f64::from(m.a) * (cy - f64::from(m.ty)) - f64::from(m.b) * (cx - f64::from(m.tx))) / determinant;
            left = cxx::min_f64(left, px);
            top = cxx::min_f64(top, py);
            right = cxx::max_f64(right, px);
            bottom = cxx::max_f64(bottom, py);
        }
    }
    if left.is_finite()
        && top.is_finite()
        && right.is_finite()
        && bottom.is_finite()
        && left.abs() < 1e6
        && top.abs() < 1e6
        && right.abs() < 1e6
        && bottom.abs() < 1e6
    {
        let left = left.floor();
        let top = top.floor();
        return Some(Position::new(Px(left as f32), Px(top as f32), Px((right.ceil() - left) as f32), Px((bottom.ceil() - top) as f32)));
    }
    None
}

/// `margins{v, v, v, v}`.
pub fn margins_of(v: Px) -> Margins {
    Margins { left: v, right: v, top: v, bottom: v }
}
