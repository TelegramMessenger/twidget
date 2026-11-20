//! Layout (stages L7 and L8): `render_item.cpp`'s layout half (`render` with the layout cache and its marks,
//! `measure`, `calc_outlines`, `calc_auto_margins`, `apply_relative_shift`, the containing block,
//! `fetch_positioned`, the out-of-flow pass, `y_shift`, baselines, the document's size and the scroll views),
//! `element.cpp`'s questions layout asks (`is_block_formatting_context`, `is_inline_box`, `in_normal_flow`,
//! `get_content_size` of each class, `mark_layout_dirty`), and `document.cpp`'s `render` with the layout budget.
//! Each class's `_render` is in its module: `render_block.rs` (and the block and inline contexts' contents in
//! `render_block_context.rs` and `render_inline_context.rs`), `render_image.rs`, `render_table.rs` (L12).
//!
//! Positioned layout (stage L9) is here too: `render_positioned` places the absolute and fixed boxes after the flow
//! (their in-flow pass, `render_out_of_flow`, leaves the static position), each item's positioned list sorted by
//! `z-index` (painting and hit testing take the stacking passes from its z-indexes), the fixed boxes' redraw boxes the
//! document keeps, sticky offsets, placements and the scroll views `calc_document_size` makes.
//!
//! Grid layout (L11) is in `render_grid.rs`, flex layout (L10) in `render_flex.rs`, `flex_line.rs` and
//! `flex_item.rs`, table layout (L12) in `render_table.rs`.

use crate::Px;
use crate::css_properties::CssProperties;
use crate::css_values::*;
use crate::document::{Document, StyleCx};
use crate::element::{ElementId, ElementKind};
use crate::formatting_context::FormattingContext;
use crate::render_item::*;
use crate::types::*;

/// What `document` keeps for layout: `m_layout_epoch` (from 1), the budget (`m_layout_budget`, `m_layout_spent`,
/// `m_layout_exhausted`), the viewport it last laid out in, the relayout boundaries to lay out again, the style and
/// tree the positioned boxes were fetched for, the fixed boxes' redraw boxes (`m_fixed_boxes`), the document's size, the
/// cache's counters (process-wide in the C++), and, for the port, the grid passes a comparison asks to see.
#[derive(Clone, Debug)]
pub struct DocLayout {
    pub epoch: u64,
    pub budget: u64,
    pub spent: u64,
    pub exhausted: bool,
    pub viewport: Position,
    pub relayout_roots: Vec<RenderId>,
    /// `m_style_epoch`: style computations so far.
    pub style_epoch: u64,
    pub positioned_epoch: u64,
    pub positioned_tree: u64,
    pub has_positioned: bool,
    /// `m_layout_after_render`.
    pub after_render: bool,
    /// `m_fixed_boxes`: each fixed box's redraw box, as `render_positioned` placed them.
    pub fixed_boxes: Vec<Position>,
    /// `m_size`.
    pub size: Size,
    pub hits: u64,
    pub misses: u64,
    pub mismatches: u64,
    /// Each grid's `_render_content` as it ends, kept while this is `Some` (the C++ keeps none of it; lhdump's probe
    /// records the same).
    pub grid_trace: Option<Vec<crate::render_grid::GridPass>>,
}

impl Default for DocLayout {
    fn default() -> DocLayout {
        DocLayout {
            epoch: 1,
            budget: 0,
            spent: 0,
            exhausted: false,
            viewport: Position::default(),
            relayout_roots: Vec::new(),
            style_epoch: 0,
            positioned_epoch: u64::MAX,
            positioned_tree: u64::MAX,
            has_positioned: false,
            after_render: false,
            fixed_boxes: Vec::new(),
            size: Size::default(),
            hits: 0,
            misses: 0,
            mismatches: 0,
            grid_trace: None,
        }
    }
}

/// `same_layout_input(entry, cb, second_pass)`: the same containing block (kinds and values) and pass.
pub fn same_layout_input(entry: &LayoutEntry, cb: &ContainingBlockContext, second_pass: bool) -> bool {
    let same = |a: &TypedPixel, b: &TypedPixel| a.ty == b.ty && a.value == b.value;
    let c = &entry.cb;
    entry.valid
        && entry.second_pass == second_pass
        && same(&c.width, &cb.width)
        && same(&c.render_width, &cb.render_width)
        && same(&c.min_width, &cb.min_width)
        && same(&c.max_width, &cb.max_width)
        && same(&c.height, &cb.height)
        && same(&c.min_height, &cb.min_height)
        && same(&c.max_height, &cb.max_height)
        && c.context_idx == cb.context_idx
        && c.size_mode == cb.size_mode
}

fn fixed_length(value: &crate::css_length::CssLength) -> bool {
    !value.is_predefined() && value.units() == CssUnits::css_units_px && value.val().is_finite()
}

impl Document {
    // ---- What layout asks of elements -----------------------------------------------------------------------------

    /// `element::is_root()`: no parent.
    pub fn el_is_root(&self, id: ElementId) -> bool {
        self.parent(id).is_none()
    }

    /// `in_normal_flow()`.
    pub fn in_normal_flow(&self, id: ElementId) -> bool {
        let css = &self.el(id).css;
        css.m_el_position != element_position_absolute && css.m_display != display_none && css.m_el_position != element_position_fixed
    }

    /// `is_inline_box()`: an inline-table, inline-block, inline-flex or inline-grid.
    pub fn is_inline_box(&self, id: ElementId) -> bool {
        let d = self.el(id).css.m_display;
        d == display_inline_table || d == display_inline_block || d == display_inline_flex || d == display_inline_grid
    }

    /// `is_positioned()`.
    pub fn is_positioned(&self, id: ElementId) -> bool {
        self.el(id).css.m_el_position > element_position_static
    }

    /// `has_external_intrinsic_size()`: a replaced element's, but an `<svg>`'s.
    pub fn has_external_intrinsic_size(&self, id: ElementId) -> bool {
        self.kind(id) == ElementKind::el_image
    }

    /// `is_ancestor(el)`: `el` is above the element.
    pub fn is_ancestor(&self, id: ElementId, el: ElementId) -> bool {
        let mut el_parent = self.parent(id);
        while let Some(p) = el_parent {
            if p == el {
                break;
            }
            el_parent = self.parent(p);
        }
        el_parent.is_some()
    }

    /// `is_block_formatting_context()`.
    pub fn is_block_formatting_context(&self, id: ElementId) -> bool {
        let css = &self.el(id).css;
        if css.m_display == display_block
            && let Some(par) = self.parent(id)
        {
            let d = self.el(par).css.m_display;
            if d == display_inline_flex || d == display_flex || d == display_grid || d == display_inline_grid {
                return true;
            }
        }
        let d = css.m_display;
        d == display_inline_block
            || d == display_table_cell
            || d == display_inline_flex
            || d == display_flex
            || d == display_inline_grid
            || d == display_grid
            || d == display_table_caption
            || self.el_is_root(id)
            || css.m_float != ElementFloat::float_none.index()
            || css.m_el_position == element_position_absolute
            || css.m_el_position == element_position_fixed
            || !(css.m_overflow == overflow_visible || css.m_overflow == overflow_clip)
            || (css.m_contain & (contain_layout | contain_paint)) != 0
    }

    /// `get_content_size(sz, max_width)` of the element's class: a text's measure, an image's size as the container
    /// knows it, an `<svg>`'s from its attributes, an `html_tag`'s nothing (a block the width), others' nothing.
    pub fn get_content_size(&self, cx: &StyleCx, id: ElementId, max_width: Px) -> Size {
        match self.kind(id) {
            ElementKind::el_text | ElementKind::el_space => {
                let size = self.el(id).text_state.size;
                Size::new(size.0, size.1)
            }
            ElementKind::el_image => {
                let src = match &self.el(id).state {
                    crate::element::ElementState::Image { src, .. } => src.clone(),
                    _ => Vec::new(),
                };
                cx.container.get_image_size(crate::gumbo::c_str(&src), None)
            }
            ElementKind::el_svg => crate::el_svg::content_size_of(
                self.get_attr(id, b"viewbox"),
                self.get_attr(id, b"width"),
                self.get_attr(id, b"height"),
                max_width,
            ),
            kind if kind.is_html_tag() => Size::new(if self.el(id).css.m_display == display_block { max_width } else { Px(0.0) }, Px(0.0)),
            _ => Size::default(),
        }
    }

    /// `element::mark_layout_dirty()`: each of the element's render items that is alive marked, with what holds it.
    pub fn mark_element_layout_dirty(&mut self, cx: &StyleCx, id: ElementId) {
        let renders: Vec<RenderId> = self.live_renders(id).collect();
        for ri in renders {
            self.mark_layout_dirty(cx, ri);
        }
    }

    // ---- What layout asks of render items -------------------------------------------------------------------------

    /// The render item's element's computed style.
    pub fn ri_css(&self, id: RenderId) -> &CssProperties {
        &self.el(self.render.item(id).element).css
    }

    /// `render_item::is_root()`: no parent, or a gone one.
    pub fn render_is_root(&self, id: RenderId) -> bool {
        self.render.parent(id).is_none()
    }

    /// `is_flex_item()`: the parent is a flex or grid container.
    pub fn is_flex_item(&self, id: RenderId) -> bool {
        match self.render.parent(id) {
            Some(par) => {
                let d = self.ri_css(par).m_display;
                d == display_inline_flex || d == display_flex || d == display_grid || d == display_inline_grid
            }
            None => false,
        }
    }

    /// `collapse_top_margin()`.
    pub fn collapse_top_margin(&self, id: RenderId) -> bool {
        let it = self.render.item(id);
        let css = self.ri_css(id);
        it.borders.top == Px(0.0)
            && it.padding.top == Px(0.0)
            && self.in_normal_flow(it.element)
            && css.m_float == ElementFloat::float_none.index()
            && it.margins.top >= Px(0.0)
            && !self.is_flex_item(id)
            && !self.render_is_root(id)
            && !(css.m_overflow == overflow_hidden || css.m_overflow == overflow_scroll || css.m_overflow == overflow_auto)
            && (css.m_contain & (contain_layout | contain_paint)) == 0
    }

    /// `collapse_bottom_margin()`.
    pub fn collapse_bottom_margin(&self, id: RenderId) -> bool {
        let it = self.render.item(id);
        let css = self.ri_css(id);
        it.borders.bottom == Px(0.0)
            && it.padding.bottom == Px(0.0)
            && self.in_normal_flow(it.element)
            && css.m_float == ElementFloat::float_none.index()
            && it.margins.bottom >= Px(0.0)
            && !self.render_is_root(id)
            && !(css.m_overflow == overflow_hidden || css.m_overflow == overflow_scroll || css.m_overflow == overflow_auto)
            && (css.m_contain & (contain_layout | contain_paint)) == 0
    }

    /// `is_visible()`.
    pub fn is_visible(&self, id: RenderId) -> bool {
        let css = self.ri_css(id);
        !self.render.item(id).skip && css.m_display != display_none && css.m_visibility == Visibility::visibility_visible.index()
    }

    fn border_box_sizing(&self, id: RenderId) -> bool {
        self.ri_css(id).m_box_sizing == BoxSizing::box_sizing_border_box.index()
    }

    /// `box_sizing_width()`: padding and borders a `border-box` width holds.
    pub fn box_sizing_width(&self, id: RenderId) -> Px {
        let it = self.render.item(id);
        let (left, right) = if self.border_box_sizing(id) {
            (it.padding.left + it.borders.left, it.padding.right + it.borders.right)
        } else {
            (Px(0.0), Px(0.0))
        };
        left + right
    }

    /// `box_sizing_height()`.
    pub fn box_sizing_height(&self, id: RenderId) -> Px {
        let it = self.render.item(id);
        let (top, bottom) = if self.border_box_sizing(id) {
            (it.padding.top + it.borders.top, it.padding.bottom + it.borders.bottom)
        } else {
            (Px(0.0), Px(0.0))
        };
        top + bottom
    }

    // ---- The layout cache -----------------------------------------------------------------------------------------

    /// `layout_cacheable(fmt_ctx)`: the cache is on, the item is in the flow and not positioned out of it, and
    /// either it starts a formatting context or none is given.
    fn layout_cacheable(&self, cx: &StyleCx, id: RenderId, no_fmt: bool) -> bool {
        let it = self.render.item(id);
        if !cx.switches.layout_cache || it.layout.out_of_flow {
            return false;
        }
        let position = self.ri_css(id).m_el_position;
        if position == element_position_absolute || position == element_position_fixed {
            return false;
        }
        no_fmt || self.is_block_formatting_context(it.element)
    }

    /// `layout_reusable()`: laid out in this epoch and not marked since.
    fn layout_reusable(&self, id: RenderId) -> bool {
        let l = &self.render.item(id).layout;
        !l.layout_dirty && l.layout_epoch == self.layout.epoch
    }

    /// `same_resolved_layout_input(entry, cb, second_pass)`: a static, unfloated flex box of fixed px sizes, its
    /// outlines fixed, with no min or max, whose tree (256 items at most) has no positioned, floated or externally
    /// sized item, gets the same containing block from both inputs.
    #[inline(never)]
    fn same_resolved_layout_input(&mut self, cx: &StyleCx, id: RenderId, cb: &ContainingBlockContext, second_pass: bool) -> bool {
        let entry = self.render.item(id).layout.last_layout;
        if !entry.valid
            || entry.second_pass != second_pass
            || entry.cb.size_mode != size_mode_normal
            || cb.size_mode != size_mode_normal
            || entry.cb.context_idx != cb.context_idx
        {
            return false;
        }
        let element = self.render.item(id).element;
        let style = &self.el(element).css;
        if style.m_display != display_flex
            || style.m_el_position != element_position_static
            || style.m_float != ElementFloat::float_none.index()
            || !self.is_block_formatting_context(element)
            || !fixed_length(&style.m_css_width)
            || !fixed_length(&style.m_css_height)
            || style.m_css_width.val() <= 0.0
            || style.m_css_height.val() <= 0.0
        {
            return false;
        }
        let m = &style.m_css_margins;
        let p = &style.m_css_padding;
        let b = &style.m_css_borders;
        for value in [
            &m.left,
            &m.right,
            &m.top,
            &m.bottom,
            &p.left,
            &p.right,
            &p.top,
            &p.bottom,
            &b.left.width,
            &b.right.width,
            &b.top.width,
            &b.bottom.width,
        ] {
            if !fixed_length(value) {
                return false;
            }
        }
        for value in [&style.m_css_min_width, &style.m_css_max_width, &style.m_css_min_height, &style.m_css_max_height] {
            if !value.is_predefined() {
                return false;
            }
        }
        if !cx.switches.fixed_layout_reuse {
            return false;
        }
        for context in [&entry.cb, cb] {
            for value in [&context.width, &context.render_width, &context.height] {
                if !value.value.value().is_finite() {
                    return false;
                }
            }
        }
        if self.render.item(id).layout.resolved_layout_safe < 0 {
            let mut safe = 1;
            let mut pending = vec![id];
            let mut count: usize = 0;
            while let Some(item) = pending.pop() {
                count += 1;
                let css = self.ri_css(item);
                if count > 256
                    || css.m_el_position != element_position_static
                    || css.m_float != ElementFloat::float_none.index()
                    || self.has_external_intrinsic_size(self.render.item(item).element)
                {
                    safe = 0;
                    break;
                }
                for &child in self.render.children(item) {
                    if pending.len() + count >= 256 {
                        safe = 0;
                        break;
                    }
                    pending.push(child);
                }
                if safe == 0 {
                    break;
                }
            }
            self.render.item_mut(id).layout.resolved_layout_safe = safe;
        }
        if self.render.item(id).layout.resolved_layout_safe == 0 {
            return false;
        }
        let mut before = self.calculate_containing_block_context(cx, id, &entry.cb);
        let mut after = self.calculate_containing_block_context(cx, id, cb);
        let normalize = |context: &mut ContainingBlockContext| {
            for value in [&mut context.min_width, &mut context.max_width, &mut context.min_height, &mut context.max_height] {
                if value.ty == CbcValueType::none {
                    value.value = Px(0.0);
                }
            }
        };
        normalize(&mut before);
        normalize(&mut after);
        same_layout_input(
            &LayoutEntry { cb: before, ret: RenderedWidth::default(), second_pass, valid: true, outer_height: Px(0.0) },
            &after,
            second_pass,
        )
    }

    /// `relayout_boundary()`: a cached, size-contained formatting context in the flow, not replaced nor positioned
    /// out of it, laid out in this epoch (or waiting to be laid out again).
    pub fn relayout_boundary(&self, cx: &StyleCx, id: RenderId) -> bool {
        let it = self.render.item(id);
        let css = self.ri_css(id);
        if !cx.switches.layout_cache
            || (css.m_contain & contain_size) == 0
            || it.layout.out_of_flow
            || !self.is_block_formatting_context(it.element)
            || self.is_replaced(it.element)
        {
            return false;
        }
        let position = css.m_el_position;
        if position == element_position_absolute || position == element_position_fixed {
            return false;
        }
        if it.layout.layout_dirty {
            return it.layout.relayout_pending;
        }
        it.layout.last_layout.valid && it.layout.layout_epoch == self.layout.epoch
    }

    /// `relayout()`: a boundary laid out again at its last input, keeping its place and boxes.
    pub fn relayout(&mut self, cx: &StyleCx, id: RenderId) {
        self.render.item_mut(id).layout.relayout_pending = false;
        let it = self.render.item(id);
        if !it.layout.layout_dirty || !it.layout.last_layout.valid {
            return;
        }
        let (placed, margin, padding, border) = (it.pos, it.margins, it.padding, it.borders);
        let (x, y, cb, second_pass) = (it.layout.last_x, it.layout.last_y, it.layout.last_layout.cb, it.layout.last_layout.second_pass);
        self.render(cx, id, x, y, &cb, None, second_pass);
        let it = self.render.item_mut(id);
        it.pos = placed;
        it.margins = margin;
        it.padding = padding;
        it.borders = border;
    }

    /// `mark_layout_dirty()`: the item and the items above it up to a relayout boundary (which is then marked
    /// pending and noted as a root to lay out again) lose their cached layouts.
    pub fn mark_layout_dirty(&mut self, cx: &StyleCx, id: RenderId) {
        let mut item = Some(id);
        while let Some(cur) = item {
            if cur != id && self.relayout_boundary(cx, cur) {
                let it = self.render.item_mut(cur);
                let pending = it.layout.relayout_pending;
                it.layout.relayout_pending = true;
                it.layout.layout_dirty = true;
                it.layout.measured[0].valid = false;
                it.layout.measured[1].valid = false;
                if !pending {
                    self.layout.relayout_roots.push(cur);
                }
                return;
            }
            let it = self.render.item_mut(cur);
            it.layout.layout_dirty = true;
            it.layout.last_layout.valid = false;
            it.layout.measured[0].valid = false;
            it.layout.measured[1].valid = false;
            item = self.render.parent(cur);
        }
    }

    /// `spend_layout()`: one more box laid out afresh, or none once the budget is spent.
    pub fn spend_layout(&mut self) -> bool {
        if self.layout.exhausted {
            return false;
        }
        self.layout.spent += 1;
        if self.layout.spent > self.layout.budget && self.layout.budget != 0 {
            self.layout.exhausted = true;
            return false;
        }
        true
    }

    /// `set_layout_budget(boxes)`.
    pub fn set_layout_budget(&mut self, boxes: u64) {
        self.layout.budget = boxes;
    }

    // ---- render and measure ---------------------------------------------------------------------------------------

    /// `render(x, y, containing_block_size, fmt_ctx, second_pass)`: the item laid out at `x`, `y` (its margin box's
    /// corner) in the containing block, or its cached layout moved there; its own formatting context when it starts
    /// one or none is given, else the given one shifted to its content box.
    pub fn render(
        &mut self,
        cx: &StyleCx,
        id: RenderId,
        x: Px,
        y: Px,
        containing_block_size: &ContainingBlockContext,
        fmt_ctx: Option<&mut FormattingContext>,
        second_pass: bool,
    ) -> RenderedWidth {
        let cacheable = self.layout_cacheable(cx, id, fmt_ctx.is_none());
        if self.render.item(id).layout.layout_epoch != self.layout.epoch {
            let l = &mut self.render.item_mut(id).layout;
            l.measured[0].valid = false;
            l.measured[1].valid = false;
        }
        if cacheable
            && self.layout_reusable(id)
            && (same_layout_input(&self.render.item(id).layout.last_layout, containing_block_size, second_pass)
                || self.same_resolved_layout_input(cx, id, containing_block_size, second_pass))
        {
            return self.render_cached(cx, id, x, y, containing_block_size, fmt_ctx, second_pass);
        }
        if cacheable {
            self.layout.misses += 1;
        }
        if !self.spend_layout() {
            return RenderedWidth::default();
        }

        let it = self.render.item_mut(id);
        it.layout.resolved_layout_safe = -1;
        self.calc_outlines(id, containing_block_size.width.px());

        let it = self.render.item_mut(id);
        it.pos.clear();
        it.pos.move_to(x, y);

        let content_left = it.content_offset_left();
        let content_top = it.content_offset_top();

        it.pos.x += content_left;
        it.pos.y += content_top;

        let own_context = self.is_block_formatting_context(self.render.item(id).element);
        let ret = match fmt_ctx {
            Some(fmt_ctx) if !own_context => {
                fmt_ctx.push_position(x + content_left, y + content_top);
                let ret = self.render_class(cx, id, x, y, containing_block_size, fmt_ctx, second_pass);
                fmt_ctx.pop_position(x + content_left, y + content_top);
                ret
            }
            _ => {
                let mut fmt = FormattingContext::default();
                let ret = self.render_class(cx, id, x, y, containing_block_size, &mut fmt, second_pass);
                fmt.apply_relative_shift(self, containing_block_size);
                ret
            }
        };
        let epoch = self.layout.epoch;
        let it = self.render.item_mut(id);
        it.layout.layout_dirty = false;
        if cacheable {
            let height = it.height();
            let l = &mut it.layout;
            l.layout_epoch = epoch;
            l.last_layout = LayoutEntry { cb: *containing_block_size, ret, second_pass, valid: true, outer_height: height };
            l.last_pos = it.pos;
            l.last_x = x;
            l.last_y = y;
            l.last_margins = it.margins;
            l.last_padding = it.padding;
            l.last_borders = it.borders;
        } else {
            it.layout.last_layout.valid = false;
        }
        ret
    }

    /// `render`'s cache hit: the last layout's place and boxes, moved to `x`, `y`; with `LH_LAYOUT_CACHE_CHECK` laid
    /// out again and compared.
    #[inline(never)]
    fn render_cached(
        &mut self,
        cx: &StyleCx,
        id: RenderId,
        x: Px,
        y: Px,
        containing_block_size: &ContainingBlockContext,
        fmt_ctx: Option<&mut FormattingContext>,
        second_pass: bool,
    ) -> RenderedWidth {
        let it = self.render.item_mut(id);
        it.pos = it.layout.last_pos;
        it.margins = it.layout.last_margins;
        it.padding = it.layout.last_padding;
        it.borders = it.layout.last_borders;
        if x != it.layout.last_x || y != it.layout.last_y {
            it.pos.x = x + it.content_offset_left();
            it.pos.y = y + it.content_offset_top();
        }
        self.layout.hits += 1;
        let it = self.render.item_mut(id);
        if !cx.switches.layout_cache_check {
            return it.layout.last_layout.ret;
        }
        let cached_pos = it.pos;
        let cached = it.layout.last_layout.ret;
        it.layout.layout_dirty = true;
        let fresh = self.render(cx, id, x, y, containing_block_size, fmt_ctx, second_pass);
        if fresh.natural_width != cached.natural_width || fresh.min_width != cached.min_width || !self.render.item(id).pos.same(&cached_pos)
        {
            self.layout.mismatches += 1;
        }
        fresh
    }

    /// `_render(x, y, containing_block_size, fmt_ctx, second_pass)` of the item's class.
    fn render_class(
        &mut self,
        cx: &StyleCx,
        id: RenderId,
        x: Px,
        y: Px,
        containing_block_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
        second_pass: bool,
    ) -> RenderedWidth {
        match self.render.item(id).kind {
            RenderKind::render_item_block
            | RenderKind::render_item_block_context
            | RenderKind::render_item_inline_context
            | RenderKind::render_item_flex
            | RenderKind::render_item_grid => self.render_block_render(cx, id, x, y, containing_block_size, fmt_ctx, second_pass),
            RenderKind::render_item_image => self.render_image_render(cx, id, x, y, containing_block_size),
            RenderKind::render_item_table => self.render_table_render(cx, id, x, y, containing_block_size, fmt_ctx),
            _ => RenderedWidth::default(),
        }
    }

    /// `measure(x, y, cb, fmt_ctx, second_pass, measured_height)` of the item's class: the last layout or one of the
    /// two last measures when the input is the same, else a layout, kept as a measure. Only flex, grid and table
    /// layout measure (stages L10–L12; L10's flex items measure their minimum and base sizes).
    pub fn measure(
        &mut self,
        cx: &StyleCx,
        id: RenderId,
        x: Px,
        y: Px,
        cb: &ContainingBlockContext,
        fmt_ctx: Option<&mut FormattingContext>,
        second_pass: bool,
        measured_height: Option<&mut Px>,
    ) -> RenderedWidth {
        if self.render.item(id).kind == RenderKind::render_item_inline_context {
            return self.inline_context_measure(cx, id, x, y, cb, fmt_ctx, second_pass, measured_height);
        }
        self.render_item_measure(cx, id, x, y, cb, fmt_ctx, second_pass, measured_height)
    }

    /// `render_item::measure`.
    pub(crate) fn render_item_measure(
        &mut self,
        cx: &StyleCx,
        id: RenderId,
        x: Px,
        y: Px,
        cb: &ContainingBlockContext,
        fmt_ctx: Option<&mut FormattingContext>,
        second_pass: bool,
        measured_height: Option<&mut Px>,
    ) -> RenderedWidth {
        let cacheable = self.layout_cacheable(cx, id, fmt_ctx.is_none());
        if cacheable && self.layout_reusable(id) && !cx.switches.layout_cache_check {
            let l = &self.render.item(id).layout;
            if same_layout_input(&l.last_layout, cb, second_pass) {
                self.layout.hits += 1;
                if let Some(h) = measured_height {
                    *h = l.last_layout.outer_height;
                }
                return l.last_layout.ret;
            }
            for entry in &l.measured {
                if same_layout_input(entry, cb, second_pass) {
                    self.layout.hits += 1;
                    if let Some(h) = measured_height {
                        *h = entry.outer_height;
                    }
                    return entry.ret;
                }
            }
        }
        let ret = self.render(cx, id, x, y, cb, fmt_ctx, second_pass);
        let height = self.render.item(id).height();
        if let Some(h) = measured_height {
            *h = height;
        }
        if cacheable {
            let l = &mut self.render.item_mut(id).layout;
            l.measured[l.measured_next as usize] = LayoutEntry { cb: *cb, ret, second_pass, valid: true, outer_height: height };
            l.measured_next ^= 1;
        }
        ret
    }

    /// `measure_height(x, y, cb, fmt_ctx, second_pass)`.
    pub fn measure_height(
        &mut self,
        cx: &StyleCx,
        id: RenderId,
        x: Px,
        y: Px,
        cb: &ContainingBlockContext,
        fmt_ctx: Option<&mut FormattingContext>,
        second_pass: bool,
    ) -> Px {
        let mut measured_height = Px(0.0);
        self.measure(cx, id, x, y, cb, fmt_ctx, second_pass, Some(&mut measured_height));
        measured_height
    }

    // ---- Outlines and shifts --------------------------------------------------------------------------------------

    /// `calc_outlines(parent_width)`: padding, the side borders and margins against the parent's width.
    pub fn calc_outlines(&mut self, id: RenderId, parent_width: Px) {
        let element = self.render.item(id).element;
        let css = &self.el(element).css;
        let padding_left = css.m_css_padding.left.calc_percent(parent_width);
        let padding_right = css.m_css_padding.right.calc_percent(parent_width);
        let borders_left = css.m_css_borders.left.width.calc_percent(parent_width);
        let borders_right = css.m_css_borders.right.width.calc_percent(parent_width);
        let margins_left = css.m_css_margins.left.calc_percent(parent_width);
        let margins_right = css.m_css_margins.right.calc_percent(parent_width);
        let margins_top = css.m_css_margins.top.calc_percent(parent_width);
        let margins_bottom = css.m_css_margins.bottom.calc_percent(parent_width);
        let padding_top = css.m_css_padding.top.calc_percent(parent_width);
        let padding_bottom = css.m_css_padding.bottom.calc_percent(parent_width);
        let it = self.render.item_mut(id);
        it.padding.left = padding_left;
        it.padding.right = padding_right;
        it.borders.left = borders_left;
        it.borders.right = borders_right;
        it.margins.left = margins_left;
        it.margins.right = margins_right;
        it.margins.top = margins_top;
        it.margins.bottom = margins_bottom;
        it.padding.top = padding_top;
        it.padding.bottom = padding_bottom;
    }

    /// `calc_auto_margins(parent_width)`: a block-level box in the flow with `auto` side margins centred or pushed;
    /// the left margin when both are `auto` or the left one is, else 0.
    pub fn calc_auto_margins(&mut self, id: RenderId, parent_width: Px) -> Px {
        let element = self.render.item(id).element;
        let css = &self.el(element).css;
        let d = css.m_display;
        if (d == display_block || d == display_table || d == display_flex || d == display_grid || d == display_list_item)
            && css.m_el_position != element_position_absolute
            && css.m_float == ElementFloat::float_none.index()
        {
            let left_auto = css.m_css_margins.left.is_predefined();
            let right_auto = css.m_css_margins.right.is_predefined();
            let it = self.render.item_mut(id);
            if left_auto && right_auto {
                let el_width = it.pos.width + it.borders.left + it.borders.right + it.padding.left + it.padding.right;
                if el_width <= parent_width {
                    it.margins.left = (parent_width - el_width) / Px(2.0);
                    it.margins.right = (parent_width - el_width) - it.margins.left;
                } else {
                    it.margins.left = Px(0.0);
                    it.margins.right = Px(0.0);
                }
                return it.margins.left;
            }
            if left_auto && !right_auto {
                let el_width = it.pos.width + it.borders.left + it.borders.right + it.padding.left + it.padding.right + it.margins.right;
                it.margins.left = parent_width - el_width;
                it.margins.left = it.margins.left.max(Px(0.0));
                return it.margins.left;
            }
            if !left_auto && right_auto {
                let el_width = it.pos.width + it.borders.left + it.borders.right + it.padding.left + it.padding.right + it.margins.left;
                it.margins.right = parent_width - el_width;
                it.margins.right = it.margins.right.max(Px(0.0));
            }
        }
        Px(0.0)
    }

    /// `apply_relative_shift(containing_block_size)`: a relatively positioned box moved by its offsets.
    pub fn apply_relative_shift(&mut self, id: RenderId, containing_block_size: &ContainingBlockContext) {
        let element = self.render.item(id).element;
        let css = &self.el(element).css;
        if css.m_el_position != element_position_relative {
            return;
        }
        let offsets = &css.m_css_offsets;
        let mut dx = None;
        let mut dy = None;
        if !offsets.left.is_predefined() {
            dx = Some((true, offsets.left.calc_percent(containing_block_size.width.px())));
        } else if !offsets.right.is_predefined() {
            dx = Some((false, offsets.right.calc_percent(containing_block_size.width.px())));
        }
        if !offsets.top.is_predefined() {
            dy = Some((true, offsets.top.calc_percent(containing_block_size.height.px())));
        } else if !offsets.bottom.is_predefined() {
            dy = Some((false, offsets.bottom.calc_percent(containing_block_size.height.px())));
        }
        let it = self.render.item_mut(id);
        match dx {
            Some((true, v)) => it.pos.x += v,
            Some((false, v)) => it.pos.x -= v,
            None => {}
        }
        match dy {
            Some((true, v)) => it.pos.y += v,
            Some((false, v)) => it.pos.y -= v,
            None => {}
        }
    }

    /// `y_shift(delta)` of the item's class: an inline moves its boxes (and, as text, itself), a text its pieces too.
    pub fn y_shift(&mut self, id: RenderId, delta: Px) {
        let kind = self.render.item(id).kind;
        match kind {
            RenderKind::render_item_inline | RenderKind::render_text => {
                let inline_text = self.ri_css(id).m_display == display_inline_text;
                let it = self.render.item_mut(id);
                if inline_text {
                    it.pos.y += delta;
                }
                for b in it.inline_boxes.iter_mut() {
                    b.y += delta;
                }
                if kind == RenderKind::render_text {
                    let fragments = it.text.as_ref().map(|t| t.fragments.clone()).unwrap_or_default();
                    for fragment in fragments {
                        self.y_shift(fragment, delta);
                    }
                }
            }
            _ => self.render.item_mut(id).pos.y += delta,
        }
    }

    /// `get_first_baseline()` of the item's class.
    pub fn get_first_baseline(&self, id: RenderId) -> Px {
        match self.render.item(id).kind {
            RenderKind::render_item_block_context => self.block_context_first_baseline(id),
            RenderKind::render_item_inline_context => self.inline_context_first_baseline(id),
            RenderKind::render_item_inline | RenderKind::render_text => {
                let fm = &self.ri_css(id).m_font_metrics;
                fm.height - fm.base_line()
            }
            RenderKind::render_item_flex => self.flex_first_baseline(id),
            _ => {
                let it = self.render.item(id);
                it.height() - it.margins.bottom
            }
        }
    }

    /// `get_last_baseline()` of the item's class.
    pub fn get_last_baseline(&self, id: RenderId) -> Px {
        match self.render.item(id).kind {
            RenderKind::render_item_block_context => self.block_context_last_baseline(id),
            RenderKind::render_item_inline_context => self.inline_context_last_baseline(id),
            RenderKind::render_item_inline | RenderKind::render_text => {
                let fm = &self.ri_css(id).m_font_metrics;
                fm.height - fm.base_line()
            }
            RenderKind::render_item_flex => self.flex_last_baseline(id),
            _ => {
                let it = self.render.item(id);
                it.height() - it.margins.bottom
            }
        }
    }

    /// `apply_vertical_align()` of the item's class (a table cell's, stage L12, asks it).
    pub fn apply_vertical_align(&mut self, cx: &StyleCx, id: RenderId) {
        match self.render.item(id).kind {
            RenderKind::render_item_block_context => self.block_context_apply_vertical_align(id),
            RenderKind::render_item_inline_context => self.inline_context_apply_vertical_align(cx, id),
            _ => {}
        }
    }

    // ---- The containing block ------------------------------------------------------------------------------------

    /// `calc_cb_length(len, percent_base, out_value)`: a % (or a calc with one) against the base, else px.
    fn calc_cb_length(&self, cx: &StyleCx, id: RenderId, len: &crate::css_length::CssLength, percent_base: Px, out_value: &mut TypedPixel) {
        if !len.is_predefined() {
            // A calc length still has a % part after unit conversion: it depends on the base as one does.
            if len.units() == CssUnits::css_units_percentage || len.units() == CssUnits::css_units_calc {
                out_value.value = len.calc_percent(percent_base);
                out_value.ty = CbcValueType::percentage;
            } else {
                out_value.value = self.units(cx).to_pixels(len, &self.ri_css(id).m_font_metrics, Px(0.0));
                out_value.ty = CbcValueType::absolute;
            }
        }
    }

    /// `calculate_containing_block_context(cb_context)`: the block the item's content is laid out in: its width and
    /// height (`auto` unless set, a flex item's basis taking the main axis's), min and max, box-sizing applied.
    pub fn calculate_containing_block_context(
        &self,
        cx: &StyleCx,
        id: RenderId,
        cb_context: &ContainingBlockContext,
    ) -> ContainingBlockContext {
        let it = self.render.item(id);
        let element = it.element;
        let css = &self.el(element).css;
        let mut ret = ContainingBlockContext { context_idx: cb_context.context_idx + 1, ..ContainingBlockContext::default() };
        let v = cb_context.width.value - it.content_offset_width();
        ret.max_width.value = v;
        ret.width.value = v;
        if css.m_el_position != element_position_absolute && css.m_el_position != element_position_fixed {
            ret.height.value = cb_context.height.value - it.content_offset_height();
        }

        // Calculate width if css property is not auto
        // We have to use aut value for display_table_cell also.
        if css.m_display != display_table_cell {
            let par = self.render.parent(id);
            let par_flex = par.map(|p| self.ri_css(p)).filter(|p| p.m_display == display_flex || p.m_display == display_inline_flex);
            if (cb_context.size_mode & size_mode_exact_width) != 0 {
                ret.width.value = cb_context.width.px();
                ret.width.ty = CbcValueType::absolute;
            } else {
                let mut width = Some(&css.m_css_width);
                if let Some(p) = par_flex
                    && !css.m_flex_basis.is_predefined()
                    && css.m_flex_basis.val() >= 0.0
                    && (p.m_flex_direction == FlexDirection::flex_direction_row.index()
                        || p.m_flex_direction == FlexDirection::flex_direction_row_reverse.index())
                {
                    ret.width.ty = CbcValueType::auto;
                    ret.width.value = Px(0.0);
                    width = None;
                }
                if let Some(width) = width {
                    self.calc_cb_length(cx, id, width, cb_context.width.px(), &mut ret.width);
                }
            }
            if (cb_context.size_mode & size_mode_exact_height) != 0 {
                ret.height.value = cb_context.height.px();
                ret.height.ty = CbcValueType::absolute;
            } else {
                let mut height = Some(&css.m_css_height);
                if let Some(p) = par_flex
                    && !css.m_flex_basis.is_predefined()
                    && css.m_flex_basis.val() >= 0.0
                    && (p.m_flex_direction == FlexDirection::flex_direction_column.index()
                        || p.m_flex_direction == FlexDirection::flex_direction_column_reverse.index())
                {
                    ret.height.ty = CbcValueType::auto;
                    ret.height.value = Px(0.0);
                    height = None;
                }
                if let Some(height) = height {
                    self.calc_cb_length(cx, id, height, cb_context.height.px(), &mut ret.height);
                }
            }
            if ret.width.ty != CbcValueType::auto && (css.m_display == display_table || self.el_is_root(element)) {
                ret.width.value -= it.content_offset_width();
            }
            if ret.height.ty != CbcValueType::auto && (css.m_display == display_table || self.el_is_root(element)) {
                ret.height.value -= it.content_offset_height();
            }
        }
        ret.render_width = ret.width;

        self.calc_cb_length(cx, id, &css.m_css_min_width, cb_context.width.px(), &mut ret.min_width);
        self.calc_cb_length(cx, id, &css.m_css_max_width, cb_context.width.px(), &mut ret.max_width);

        self.calc_cb_length(cx, id, &css.m_css_min_height, cb_context.height.px(), &mut ret.min_height);
        self.calc_cb_length(cx, id, &css.m_css_max_height, cb_context.height.px(), &mut ret.max_height);

        // Fix box sizing
        if ret.width.ty != CbcValueType::auto {
            ret.render_width.set(ret.width.value - self.box_sizing_width(id));
        }
        if ret.min_width.ty != CbcValueType::none {
            ret.min_width.value -= self.box_sizing_width(id);
        }
        if ret.max_width.ty != CbcValueType::none {
            ret.max_width.value -= self.box_sizing_width(id);
        }
        if ret.min_height.ty != CbcValueType::none {
            ret.min_height.value -= self.box_sizing_height(id);
        }
        if ret.max_height.ty != CbcValueType::none {
            ret.max_height.value -= self.box_sizing_height(id);
        }

        // Issue #459: a flex item's exact width is its resolved main size: the clamp by its min/max-width was the
        // flex algorithm's, against the container (render_item.cpp says why at length).
        if (cb_context.size_mode & size_mode_exact_width) != 0 {
            let par = self.render.parent(id);
            let row_flex_parent = par.map(|p| self.ri_css(p)).is_some_and(|p| {
                (p.m_display == display_flex || p.m_display == display_inline_flex)
                    && (p.m_flex_direction == FlexDirection::flex_direction_row.index()
                        || p.m_flex_direction == FlexDirection::flex_direction_row_reverse.index())
            });
            if (cb_context.size_mode & size_mode_resolved_width) != 0 || row_flex_parent {
                ret.min_width.ty = CbcValueType::none;
                ret.max_width.ty = CbcValueType::none;
            }
        }

        ret
    }

    // ---- Positioned boxes -----------------------------------------------------------------------------------------

    /// `fetch_positioned()`: each item's positioned children listed on the item that positions them, and whether it
    /// is out of the flow's caching (an absolute or fixed box below, a replaced box, an image list marker).
    pub fn fetch_positioned(&mut self, id: RenderId) -> bool {
        let mut ret = false;
        let mut replaced = false;

        for old in std::mem::take(&mut self.render.item_mut(id).positioned) {
            self.release_render(old);
        }

        let mut k = 0;
        while k < self.render.children(id).len() {
            let el = self.render.children(id)[k];
            k += 1;
            let el_element = self.render.item(el).element;
            replaced = replaced || self.has_external_intrinsic_size(el_element);
            let el_pos = self.el(el_element).css.m_el_position;
            if el_pos != element_position_static {
                self.add_positioned(id, el);
            }
            if !ret && (el_pos == element_position_absolute || el_pos == element_position_fixed) {
                ret = true;
            }
            if self.fetch_positioned(el) {
                ret = true;
            }
            replaced = replaced || self.render.item(el).layout.out_of_flow;
        }
        let element = self.render.item(id).element;
        let css = &self.el(element).css;
        let out_of_flow = ret
            || replaced
            || self.has_external_intrinsic_size(element)
            || (css.m_display == display_list_item && !css.m_list_style_image.is_empty());
        self.render.item_mut(id).layout.out_of_flow = out_of_flow;
        ret
    }

    /// `add_positioned(el)`: on the first item from this one up that is positioned, the root, transformed or
    /// contained.
    pub fn add_positioned(&mut self, id: RenderId, el: RenderId) {
        let mut cur = Some(id);
        while let Some(item) = cur {
            let css = self.ri_css(item);
            if css.m_el_position != element_position_static
                || self.render_is_root(item)
                || !css.m_transform.is_empty()
                || (css.m_contain & (contain_layout | contain_paint)) != 0
            {
                self.render.hold(el);
                self.render.item_mut(item).positioned.push(el);
                return;
            }
            cur = self.render.parent(item);
        }
    }

    /// `render_out_of_flow(y, self_size, fmt_ctx)`: an absolute or fixed box's in-flow pass, shrunk to fit:
    /// min(max(min-content, available), max-content), which leaves its static position; `render_positioned` places
    /// it.
    pub fn render_out_of_flow(
        &mut self,
        cx: &StyleCx,
        id: RenderId,
        y: Px,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) {
        let rw = self.render(cx, id, Px(0.0), y, self_size, Some(fmt_ctx), false);
        if !self.ri_css(id).m_css_width.is_predefined() {
            return;
        }
        let width = self.render.item(id).width();
        if rw.natural_width < width {
            self.render(cx, id, Px(0.0), y, &self_size.new_width(rw.natural_width, size_mode_normal), Some(fmt_ctx), false);
        } else if rw.min_width > width {
            // A zero-width parent (a 'left: 0; right: 0' box before it is placed) still fits the words.
            self.render(cx, id, Px(0.0), y, &self_size.new_width(rw.min_width, size_mode_normal), Some(fmt_ctx), false);
        }
    }

    /// `render_positioned(rt)`: the absolute and fixed boxes the item positions placed in their containing block (the
    /// viewport for a fixed box and for the root's when the root is not positioned, else the item's padding box) as
    /// CSS 2.2 10.3.7 and 10.6.4 solve them, laid out again at their size when it changed; each fixed box's redraw box
    /// kept by the document; each positioned box's own placed in turn (all of them), then those of the static boxes
    /// below that hold some (transformed or contained ones); the list sorted by `z-index`, ties in their order.
    pub fn render_positioned(&mut self, cx: &StyleCx, id: RenderId, rt: RenderType) {
        let view_port = cx.container.get_viewport();
        let mut k = 0;
        while k < self.render.item(id).positioned.len() {
            let el = self.render.item(id).positioned[k];
            k += 1;
            self.place_positioned(cx, id, el, rt, &view_port);
            self.render_positioned(cx, el, RenderType::render_all);
        }
        self.render_positioned_holders(cx, id, rt);
        if !self.render.item(id).positioned.is_empty() {
            let mut positioned = std::mem::take(&mut self.render.item_mut(id).positioned);
            // std::stable_sort by get_z_index(): a stable sort by the key.
            positioned.sort_by_key(|&p| self.ri_css(p).get_z_index());
            self.render.item_mut(id).positioned = positioned;
        }
    }

    /// `render_positioned`'s `holders`: the static boxes below the item, in order and short of the positioned ones,
    /// that hold positioned boxes place them; the others are looked into.
    fn render_positioned_holders(&mut self, cx: &StyleCx, item: RenderId, rt: RenderType) {
        let mut k = 0;
        while k < self.render.children(item).len() {
            let child = self.render.children(item)[k];
            k += 1;
            if self.ri_css(child).m_el_position != element_position_static {
                continue;
            }
            if !self.render.item(child).positioned.is_empty() {
                self.render_positioned(cx, child, rt);
            } else {
                self.render_positioned_holders(cx, child, rt);
            }
        }
    }

    /// `render_positioned`'s height in the box's sizing: min and max against the containing block, then the
    /// padding, borders and margins a content-box height leaves out (`fix_height_min_max`).
    fn fix_height_min_max(&self, el: RenderId, containing_block_size: &ContainingBlockContext, mut height: Px) -> Px {
        let css = self.ri_css(el);
        let max_height = &css.m_css_max_height;
        let min_height = &css.m_css_min_height;
        if !max_height.is_predefined() {
            let max_height_value = max_height.calc_percent(containing_block_size.height.px());
            height = height.min(max_height_value);
        }
        if !min_height.is_predefined() {
            let min_height_value = min_height.calc_percent(containing_block_size.height.px());
            height = height.max(min_height_value);
        }
        height += self.render.item(el).content_offset_height() - self.box_sizing_height(el);
        height
    }

    /// `fix_width_min_max`, the same across.
    fn fix_width_min_max(&self, el: RenderId, containing_block_size: &ContainingBlockContext, mut width: Px) -> Px {
        let css = self.ri_css(el);
        let max_width = &css.m_css_max_width;
        let min_width = &css.m_css_min_width;
        if !max_width.is_predefined() {
            let max_width_value = max_width.calc_percent(containing_block_size.width.px());
            width = width.min(max_width_value);
        }
        if !min_width.is_predefined() {
            let min_width_value = min_width.calc_percent(containing_block_size.width.px());
            width = width.max(min_width_value);
        }
        width += self.render.item(el).content_offset_width() - self.box_sizing_width(el);
        width
    }

    /// One box of `render_positioned`'s list placed: kept out of the recursion's frames.
    #[inline(never)]
    fn place_positioned(&mut self, cx: &StyleCx, id: RenderId, el: RenderId, rt: RenderType, view_port: &Position) {
        let el_element = self.render.item(el).element;
        let el_position = self.el(el_element).css.m_el_position;

        let mut process = false;
        if self.el(el_element).css.m_display != display_none {
            if el_position == element_position_absolute {
                if rt != RenderType::render_fixed_only {
                    process = true;
                }
            } else if el_position == element_position_fixed && rt != RenderType::render_no_fixed {
                process = true;
            }
        }
        if !process {
            return;
        }

        let mut containing_block_size = ContainingBlockContext::default();
        if el_position == element_position_fixed || (self.render_is_root(id) && !self.is_positioned(self.render.item(id).element)) {
            containing_block_size.height.set(view_port.height);
            containing_block_size.width.set(view_port.width);
        } else {
            let it = self.render.item(id);
            containing_block_size.height.set(it.pos.height + it.padding.height());
            containing_block_size.width.set(it.pos.width + it.padding.width());
        }
        let cb_width = containing_block_size.width.px();
        let cb_height = containing_block_size.height.px();

        let css = &self.el(el_element).css;
        let css_left = css.m_css_offsets.left.clone();
        let css_right = css.m_css_offsets.right.clone();
        let css_top = css.m_css_offsets.top.clone();
        let css_bottom = css.m_css_offsets.bottom.clone();
        let el_width = css.m_css_width.clone();
        let el_height = css.m_css_height.clone();
        let margin_top_auto = css.m_css_margins.top.is_predefined();
        let margin_bottom_auto = css.m_css_margins.bottom.is_predefined();
        let margin_left_auto = css.m_css_margins.left.is_predefined();
        let margin_right_auto = css.m_css_margins.right.is_predefined();
        let display = css.m_display;

        let mut need_render = false;

        if (display == display_flex || display == display_inline_flex) && (el_width.is_predefined() || el_height.is_predefined()) {
            let mut available_width = cb_width;
            if !css_left.is_predefined() {
                available_width -= css_left.calc_percent(cb_width);
            }
            if !css_right.is_predefined() {
                available_width -= css_right.calc_percent(cb_width);
            }
            available_width = Px(0.0).max(available_width);
            let (left, top) = (self.render.item(el).left(), self.render.item(el).top());
            let measured = self.render(cx, el, left, top, &containing_block_size.new_width(available_width, size_mode_normal), None, false);
            if el_width.is_predefined() && (css_left.is_predefined() || css_right.is_predefined()) {
                let fitted_width = measured.min_width.max(available_width).min(measured.natural_width);
                if fitted_width != self.render.item(el).width() {
                    let (left, top) = (self.render.item(el).left(), self.render.item(el).top());
                    self.render(cx, el, left, top, &containing_block_size.new_width(fitted_width, size_mode_normal), None, false);
                }
            }
        }

        let bottom;
        let top;
        let mut height;

        let (el_static_offset_x, el_static_offset_y) = self.element_static_offset(id, el);

        let el_static_x = self.render.item(el).pos.x + el_static_offset_x;
        let el_static_y = self.render.item(el).pos.y + el_static_offset_y;
        // The auto vertical margins set to 0, as every rule but the last does first.
        let zero_vertical_auto_margins = |doc: &mut Document| {
            let it = doc.render.item_mut(el);
            if margin_top_auto {
                it.margins.top = Px(0.0);
            }
            if margin_bottom_auto {
                it.margins.bottom = Px(0.0);
            }
        };
        // Calculate vertical position
        // https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height
        // 10.6.4 Absolutely positioned, non-replaced elements
        if css_top.is_predefined() && !css_bottom.is_predefined() && el_height.is_predefined() {
            // 1. 'top' and 'height' are 'auto' and 'bottom' is not 'auto', then the height is based on the
            // content per 10.6.7, set 'auto' values for 'margin-top' and 'margin-bottom' to 0, and solve for 'top'
            zero_vertical_auto_margins(self);
            height = self.render.item(el).height();
            bottom = css_bottom.calc_percent(cb_height);
            top = containing_block_size.height.value - height - bottom;
        } else if css_top.is_predefined() && css_bottom.is_predefined() && !el_height.is_predefined() {
            // 2. 'top' and 'bottom' are 'auto' and 'height' is not 'auto', then set 'top' to the static position,
            // set 'auto' values for 'margin-top' and 'margin-bottom' to 0, and solve for 'bottom'
            zero_vertical_auto_margins(self);
            top = el_static_y - self.render.item(el).content_offset_top();
            height = self.fix_height_min_max(el, &containing_block_size, el_height.calc_percent(cb_height));
        } else if !css_top.is_predefined() && css_bottom.is_predefined() && el_height.is_predefined() {
            // 3. 'height' and 'bottom' are 'auto' and 'top' is not 'auto', then the height is based on the
            // content per 10.6.7, set 'auto' values for 'margin-top' and 'margin-bottom' to 0,
            // and solve for 'bottom'
            zero_vertical_auto_margins(self);
            height = self.render.item(el).height();
            top = css_top.calc_percent(cb_height);
        } else if css_top.is_predefined() && !css_bottom.is_predefined() && !el_height.is_predefined() {
            // 4. 'top' is 'auto', 'height' and 'bottom' are not 'auto', then set 'auto' values for 'margin-top'
            // and 'margin-bottom' to 0, and solve for 'top'
            zero_vertical_auto_margins(self);
            height = self.fix_height_min_max(el, &containing_block_size, el_height.calc_percent(cb_height));
            bottom = css_bottom.calc_percent(cb_height);
            top = containing_block_size.height.value - height - bottom;
        } else if !css_top.is_predefined() && !css_bottom.is_predefined() && el_height.is_predefined() {
            // 5. 'height' is 'auto', 'top' and 'bottom' are not 'auto', then 'auto' values for 'margin-top' and
            // 'margin-bottom' are set to 0 and solve for 'height'
            zero_vertical_auto_margins(self);
            bottom = css_bottom.calc_percent(cb_height);
            top = css_top.calc_percent(cb_height);
            if self.is_replaced(el_element) {
                let it = self.render.item_mut(el);
                height = it.height() - it.content_offset_height();
                let reminded = (containing_block_size.height.value - top - bottom) - height - it.content_offset_height();
                if reminded > Px(0.0) {
                    let divider = i32::from(margin_top_auto) + i32::from(margin_bottom_auto);
                    if divider != 0 {
                        if margin_top_auto {
                            it.margins.top = reminded / Px::from_int(divider);
                        }
                        if margin_bottom_auto {
                            it.margins.bottom = reminded / Px::from_int(divider);
                        }
                    }
                }
                height += it.content_offset_height();
            } else {
                height = containing_block_size.height.value - top - bottom;
            }
            let max_height_css = &self.ri_css(el).m_css_max_height;
            if !max_height_css.is_predefined() {
                let max_height = max_height_css.calc_percent(cb_height);
                let it = self.render.item_mut(el);
                if height - it.content_offset_height() > max_height {
                    let reminded = height - it.content_offset_height() - max_height;
                    height = max_height;
                    let divider = i32::from(margin_top_auto) + i32::from(margin_bottom_auto);
                    if divider != 0 {
                        if margin_top_auto {
                            it.margins.top = reminded / Px::from_int(divider);
                        }
                        if margin_bottom_auto {
                            it.margins.bottom = reminded / Px::from_int(divider);
                        }
                    }
                    height += it.content_offset_height();
                }
            }
        } else if !css_top.is_predefined() && css_bottom.is_predefined() && !el_height.is_predefined() {
            // 6. 'bottom' is 'auto', 'top' and 'height' are not 'auto', then set 'auto' values for 'margin-top'
            // and 'margin-bottom' to 0 and solve for 'bottom'
            zero_vertical_auto_margins(self);
            height = self.fix_height_min_max(el, &containing_block_size, el_height.calc_percent(cb_height));
            top = css_top.calc_percent(cb_height);
        } else if css_top.is_predefined() && css_bottom.is_predefined() && el_height.is_predefined() {
            // If all three of 'top', 'height', and 'bottom' are auto, set 'top' to the static position and
            // apply rule number three.
            zero_vertical_auto_margins(self);
            height = self.render.item(el).height();
            top = el_static_y - self.render.item(el).content_offset_top();
        } else {
            // If none of the three are 'auto':
            height = self.fix_height_min_max(el, &containing_block_size, el_height.calc_percent(cb_height));
            top = css_top.calc_percent(cb_height);
            bottom = css_bottom.calc_percent(cb_height);
            let remained = containing_block_size.height.value - height - top - bottom;

            let it = self.render.item_mut(el);
            if margin_top_auto && margin_bottom_auto {
                // If both 'margin-top' and 'margin-bottom' are 'auto', solve the equation under the extra
                // constraint that the two margins get equal values.
                it.margins.bottom = remained / Px(2.0);
                it.margins.top = it.margins.bottom;
                height += it.margins.top + it.margins.bottom;
            } else {
                // If one of 'margin-top' or 'margin-bottom' is 'auto', solve the equation for that value.
                if margin_top_auto {
                    it.margins.top = remained;
                    height += it.margins.top;
                }
                if margin_bottom_auto {
                    it.margins.bottom = remained;
                    height += it.margins.bottom;
                }
            }
        }
        let it = self.render.item_mut(el);
        it.pos.y = top + it.content_offset_top();
        if it.pos.height != height - it.content_offset_height() {
            it.pos.height = height - it.content_offset_height();
            need_render = true;
        }

        // Calculate horizontal position
        let right;
        let left;
        let mut width;
        let zero_horizontal_auto_margins = |doc: &mut Document| {
            let it = doc.render.item_mut(el);
            if margin_left_auto {
                it.margins.left = Px(0.0);
            }
            if margin_right_auto {
                it.margins.right = Px(0.0);
            }
        };
        // https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width
        // 10.3.7 Absolutely positioned, non-replaced elements
        if css_left.is_predefined() && !css_right.is_predefined() && el_width.is_predefined() {
            // 1. 'left' and 'width' are 'auto' and 'right' is not 'auto', then the width is shrink-to-fit.
            // Then solve for 'left'
            zero_horizontal_auto_margins(self);
            width = self.render.item(el).width();
            right = css_right.calc_percent(cb_width);
            left = containing_block_size.width.value - width - right;
        } else if css_left.is_predefined() && css_right.is_predefined() && !el_width.is_predefined() {
            // 2. 'left' and 'right' are 'auto' and 'width' is not 'auto', then if the 'direction' property of
            // the element establishing the static-position containing block is 'ltr' set 'left' to the
            // static position, otherwise set 'right' to the static position. Then solve for 'left'
            // (if 'direction is 'rtl') or 'right' (if 'direction' is 'ltr').
            zero_horizontal_auto_margins(self);
            left = el_static_x - self.render.item(el).content_offset_left();
            width = self.fix_width_min_max(el, &containing_block_size, el_width.calc_percent(cb_width));
        } else if !css_left.is_predefined() && css_right.is_predefined() && el_width.is_predefined() {
            // 3. 'width' and 'right' are 'auto' and 'left' is not 'auto', then the width is shrink-to-fit .
            // Then solve for 'right'
            zero_horizontal_auto_margins(self);
            width = self.render.item(el).width();
            left = css_left.calc_percent(cb_width);
        } else if css_left.is_predefined() && !css_right.is_predefined() && !el_width.is_predefined() {
            // 4. 'left' is 'auto', 'width' and 'right' are not 'auto', then solve for 'left'
            zero_horizontal_auto_margins(self);
            right = css_right.calc_percent(cb_width);
            width = self.fix_width_min_max(el, &containing_block_size, el_width.calc_percent(cb_width));
            left = containing_block_size.width.value - right - width;
        } else if !css_left.is_predefined() && !css_right.is_predefined() && el_width.is_predefined() {
            // 5. 'width' is 'auto', 'left' and 'right' are not 'auto', then solve for 'width'
            zero_horizontal_auto_margins(self);
            left = css_left.calc_percent(cb_width);
            right = css_right.calc_percent(cb_width);
            if self.is_replaced(el_element) {
                let it = self.render.item_mut(el);
                width = it.width() - it.content_offset_width();
                let reminded = (containing_block_size.width.value - left - right) - width - it.content_offset_width();
                if reminded != Px(0.0) {
                    let divider = i32::from(margin_left_auto) + i32::from(margin_right_auto);
                    if divider != 0 {
                        if margin_left_auto {
                            it.margins.left = reminded / Px::from_int(divider);
                        }
                        if margin_right_auto {
                            it.margins.right = reminded / Px::from_int(divider);
                        }
                    }
                }
                width += it.content_offset_width();
            } else {
                width = containing_block_size.width.value - left - right;
            }
            let max_width_css = &self.ri_css(el).m_css_max_width;
            if !max_width_css.is_predefined() {
                // The C++ resolves a % max-width here against the containing block's height.
                let max_width = max_width_css.calc_percent(cb_height);
                let it = self.render.item_mut(el);
                if width - it.content_offset_width() > max_width {
                    let reminded = width - it.content_offset_width() - max_width;
                    width = max_width;
                    let divider = i32::from(margin_left_auto) + i32::from(margin_right_auto);
                    if divider != 0 {
                        if margin_left_auto {
                            it.margins.left = reminded / Px::from_int(divider);
                        }
                        if margin_right_auto {
                            it.margins.right = reminded / Px::from_int(divider);
                        }
                    }
                    width += it.content_offset_width();
                }
            }
        } else if !css_left.is_predefined() && css_right.is_predefined() && !el_width.is_predefined() {
            // 6. 'right' is 'auto', 'left' and 'width' are not 'auto', then solve for 'right'
            zero_horizontal_auto_margins(self);
            left = css_left.calc_percent(cb_width);
            width = self.fix_width_min_max(el, &containing_block_size, el_width.calc_percent(cb_width));
        } else if css_left.is_predefined() && css_right.is_predefined() && el_width.is_predefined() {
            // If all three of 'left', 'width', and 'right' are 'auto': First set any 'auto' values for
            // 'margin-left' and 'margin-right' to 0. Then, if the 'direction' property of the element
            // establishing the static-position containing block is 'ltr' set 'left' to the static position
            // and apply rule number three below; otherwise, set 'right' to the static position and apply
            // rule number one below.
            zero_horizontal_auto_margins(self);
            width = self.render.item(el).width();
            left = el_static_x - self.render.item(el).content_offset_left();
        } else {
            // If none of the three is 'auto':
            width = self.fix_width_min_max(el, &containing_block_size, el_width.calc_percent(cb_width));
            left = css_left.calc_percent(cb_width);
            right = css_right.calc_percent(cb_width);
            let remained = containing_block_size.width.value - width - left - right;

            let it = self.render.item_mut(el);
            if margin_left_auto && margin_right_auto {
                // If both 'margin-left' and 'margin-right' are 'auto', solve the equation under the extra
                // constraint that the two margins get equal values, unless this would make them negative,
                // in which case when direction of the containing block is 'ltr' ('rtl'), set 'margin-left'
                // ('margin-right') to zero and solve for 'margin-right' ('margin-left').
                it.margins.right = remained / Px(2.0);
                it.margins.left = it.margins.right;
                if it.margins.left < Px(0.0) {
                    it.margins.left = Px(0.0);
                    it.margins.right = remained;
                }
                width += it.margins.left + it.margins.right;
            } else {
                // If one of 'margin-left' or 'margin-right' is 'auto', solve the equation
                // for that value. If the values are over-constrained, ignore the value for 'left'
                // (in case the 'direction' property of the containing block is 'rtl') or 'right' (in case
                // 'direction' is 'ltr') and solve for that value.
                if margin_left_auto {
                    it.margins.left = remained;
                    width += it.margins.left;
                }
                if margin_right_auto {
                    it.margins.right = remained;
                    width += it.margins.right;
                }
            }
        }
        let it = self.render.item_mut(el);
        it.pos.x = left + it.content_offset_left();
        if it.pos.width != width - it.content_offset_width() {
            it.pos.width = width - it.content_offset_width();
            need_render = true;
        }

        if el_position != element_position_fixed {
            it.pos.x -= el_static_offset_x;
            it.pos.y -= el_static_offset_y;
        }

        if need_render {
            let pos = self.render.item(el).pos;
            let (left, top) = (self.render.item(el).left(), self.render.item(el).top());
            if self.ri_css(el).m_display == display_table || self.is_replaced(el_element) {
                let cb = containing_block_size.new_width(self.render.item(el).width(), size_mode_normal);
                self.render(cx, el, left, top, &cb, None, true);
            } else {
                let cb = containing_block_size.new_width_height(
                    pos.width + self.box_sizing_width(el),
                    pos.height + self.box_sizing_height(el),
                    size_mode_exact_width | size_mode_exact_height,
                );
                self.render(cx, el, left, top, &cb, None, true);
            }
            self.render.item_mut(el).pos = pos;
        }

        if el_position == element_position_fixed {
            let mut fixed_pos = self.render.item(el).pos;
            self.get_redraw_box(el, &mut fixed_pos, Px(0.0), Px(0.0));
            self.layout.fixed_boxes.push(fixed_pos);
        }
    }

    /// `element_static_offset(el)`: from the box's parent to this item (to the root for a fixed box), the places
    /// of the boxes between, then this item's place for a fixed box or a root that is not positioned, else its
    /// padding: what turns a static position into one in the containing block.
    pub fn element_static_offset(&self, id: RenderId, el: RenderId) -> (Px, Px) {
        let mut offset_x = Px(0.0);
        let mut offset_y = Px(0.0);
        let fixed = self.ri_css(el).m_el_position == element_position_fixed;
        let mut cur_el = self.render.parent(el);
        let this_el = if !fixed { Some(id) } else { self.render.root };
        while let Some(cur) = cur_el {
            if Some(cur) == this_el {
                break;
            }
            let pos = self.render.item(cur).pos;
            offset_x += pos.x;
            offset_y += pos.y;
            cur_el = self.render.parent(cur);
        }

        if fixed || (self.render_is_root(id) && !self.is_positioned(self.render.item(id).element)) {
            // The C++ reads the document's root render item here unchecked; layout runs only with one.
            if let Some(this_el) = this_el {
                let pos = self.render.item(this_el).pos;
                offset_x += pos.x;
                offset_y += pos.y;
            }
        } else {
            let it = self.render.item(id);
            offset_x += it.padding.left;
            offset_y += it.padding.top;
        }
        (offset_x, offset_y)
    }

    /// `get_redraw_box(pos, x, y)`: the box grown over the item's border box at `x`, `y` (its right edge grown by its
    /// left padding and border, as the C++ has it), and over its children's that are not fixed unless it clips them.
    pub fn get_redraw_box(&self, id: RenderId, pos: &mut Position, x: Px, y: Px) {
        if !self.is_visible(id) {
            return;
        }
        let it = self.render.item(id);
        let p_left = pos.left().min(x + it.pos.left() - it.padding.left - it.borders.left);
        let p_right = pos.right().max(x + it.pos.right() + it.padding.left + it.borders.left);
        let p_top = pos.top().min(y + it.pos.top() - it.padding.top - it.borders.top);
        let p_bottom = pos.bottom().max(y + it.pos.bottom() + it.padding.bottom + it.borders.bottom);

        pos.x = p_left;
        pos.y = p_top;
        pos.width = p_right - p_left;
        pos.height = p_bottom - p_top;

        let css = self.ri_css(id);
        let clips_contents = css.m_overflow > overflow_visible || (css.m_contain & contain_paint) != 0;
        if !clips_contents {
            let (cx, cy) = (x + it.pos.x, y + it.pos.y);
            for &el in &it.children {
                if self.ri_css(el).m_el_position != element_position_fixed {
                    self.get_redraw_box(el, pos, cx, cy);
                }
            }
        }
    }

    /// `document::get_fixed_boxes(fixed_boxes)`: the fixed boxes' redraw boxes the last placement kept.
    pub fn get_fixed_boxes(&self) -> Vec<Position> {
        self.layout.fixed_boxes.clone()
    }

    // ---- The document -------------------------------------------------------------------------------------------

    /// `document::render(max_width)`: [`Document::layout_document_as`] with `render_all`, as the renderer lays out.
    pub fn layout_document(&mut self, cx: &StyleCx, max_width: Px) -> Px {
        self.layout_document_as(cx, max_width, RenderType::render_all)
    }

    /// `document::render(max_width, rt)`: the root laid out at the width, the viewport's height its containing
    /// block's (a new viewport starts a new layout epoch), within the budget; the relayout boundaries laid out again;
    /// the positioned boxes placed when there are absolute or fixed ones; the document's size. Gives the root's
    /// natural width. `render_fixed_only` places the fixed boxes again and nothing else (it gives 0).
    pub fn layout_document_as(&mut self, cx: &StyleCx, max_width: Px, rt: RenderType) -> Px {
        let mut ret = Px(0.0);
        self.layout.spent = 0;
        let Some(root_render) = self.render.root else { return ret };
        if self.root.is_none() || self.layout.exhausted {
            return ret;
        }
        let viewport = cx.container.get_viewport();
        if !viewport.same(&self.layout.viewport) {
            self.layout.viewport = viewport;
            self.layout.epoch += 1;
        }
        let mut cb_context = ContainingBlockContext::default();
        cb_context.width.set(max_width);
        cb_context.width.ty = CbcValueType::absolute;
        cb_context.height.set(viewport.height);
        cb_context.height.ty = CbcValueType::absolute;

        if rt == RenderType::render_fixed_only {
            self.layout.fixed_boxes.clear();
            self.render_positioned(cx, root_render, rt);
            return ret;
        }
        let refetch = !cx.switches.positioned_reuse;
        if refetch || self.layout.positioned_epoch != self.layout.style_epoch || self.layout.positioned_tree != self.render.serial {
            self.layout.has_positioned = self.fetch_positioned(root_render);
            self.layout.positioned_epoch = self.layout.style_epoch;
            self.layout.positioned_tree = self.render.serial;
        }
        ret = self.render(cx, root_render, Px(0.0), Px(0.0), &cb_context, None, false).natural_width;
        for item in std::mem::take(&mut self.layout.relayout_roots) {
            if self.render.alive(item) {
                self.relayout(cx, item);
            }
        }
        self.layout.after_render = true;
        if self.layout.has_positioned {
            self.layout.fixed_boxes.clear();
            self.render_positioned(cx, root_render, rt);
        }
        self.layout.size = Size::default();
        let mut size = Size::default();
        self.calc_document_size(root_render, &mut size, Px(0.0), Px(0.0));
        self.layout.size = size;
        ret
    }

    /// `for_inline_boxes(process)` of the item's class, for `calc_document_size`: an inline's boxes, a table row's
    /// cells' boxes; `None` where the class has none (the base class, a text).
    fn inline_boxes_of(&self, id: RenderId) -> Option<Vec<Position>> {
        let it = self.render.item(id);
        match it.kind {
            RenderKind::render_item_inline => Some(it.inline_boxes.clone()),
            RenderKind::render_item_table_row => {
                let mut boxes = Vec::new();
                for &el in &it.children {
                    if self.ri_css(el).m_display == display_table_cell {
                        let c = self.render.item(el);
                        let mut pos = Position::default();
                        pos.x = c.left() + c.margins.left;
                        pos.y = c.top() - it.padding.top - it.borders.top;
                        pos.width = c.right() - pos.x - c.margins.right - c.margins.left;
                        pos.height = c.height() + it.padding.top + it.padding.bottom + it.borders.top + it.borders.bottom;
                        boxes.push(pos);
                    }
                }
                Some(boxes)
            }
            _ => None,
        }
    }

    /// `get_scroll_left()`.
    pub fn get_scroll_left(&self, id: RenderId) -> Px {
        self.render.item(id).scroll_view.as_ref().map_or(Px(0.0), |v| v.left)
    }

    /// `get_scroll_top()`.
    pub fn get_scroll_top(&self, id: RenderId) -> Px {
        self.render.item(id).scroll_view.as_ref().map_or(Px(0.0), |v| v.top)
    }

    /// `sticky_offset()`: how far a sticky box is moved to stay inside its scroller's viewport (its offsets from the
    /// viewport's edges), within its parent's box.
    pub fn sticky_offset(&self, id: RenderId) -> Position {
        let mut shift = Position::default();
        if self.ri_css(id).m_el_position != element_position_sticky {
            return shift;
        }
        let Some(parent_item) = self.render.parent(id) else { return shift };
        let it = self.render.item(id);
        let mut scroller = Some(parent_item);
        let (mut local_x, mut local_y) = (it.pos.x, it.pos.y);
        while let Some(s) = scroller {
            let overflow = self.ri_css(s).m_overflow;
            if !(overflow == overflow_visible || overflow == overflow_clip) {
                break;
            }
            local_x += self.render.item(s).pos.x;
            local_y += self.render.item(s).pos.y;
            scroller = self.render.parent(s);
        }
        let Some(scroller) = scroller else { return shift };
        let si = self.render.item(scroller);
        let mut viewport = si.pos;
        viewport.grow(&si.padding);
        let offsets = &self.ri_css(id).m_css_offsets;
        #[allow(clippy::too_many_arguments)]
        fn axis(
            start: Px,
            size: Px,
            scroll: Px,
            viewport_start: Px,
            viewport_size: Px,
            parent_start: Px,
            parent_size: Px,
            before: &crate::css_length::CssLength,
            after: &crate::css_length::CssLength,
        ) -> Px {
            if before.is_predefined() && after.is_predefined() {
                return Px(0.0);
            }
            let mut delta = Px(0.0);
            if !after.is_predefined() {
                delta = delta.min(scroll + viewport_start + viewport_size - after.calc_percent(viewport_size) - start - size);
            }
            if !before.is_predefined() {
                delta = delta.max(scroll + viewport_start + before.calc_percent(viewport_size) - start);
            }
            Px(0.0).min(-parent_start).max(delta.min(Px(0.0).max(parent_size - parent_start - size)))
        }
        let left = it.pos.x - it.padding.left - it.borders.left;
        let top = it.pos.y - it.padding.top - it.borders.top;
        let pi = self.render.item(parent_item);
        let mut parent_width = pi.pos.width;
        let mut parent_height = pi.pos.height;
        if let Some(view) = pi.scroll_view.as_ref() {
            parent_width = parent_width.max(view.scroll_size.width - pi.padding.width());
            parent_height = parent_height.max(view.scroll_size.height - pi.padding.height());
        }
        shift.x = axis(
            local_x - it.padding.left - it.borders.left,
            it.pos.width + it.padding.width() + it.borders.width(),
            self.get_scroll_left(scroller),
            -si.padding.left,
            viewport.width,
            left,
            parent_width,
            &offsets.left,
            &offsets.right,
        );
        shift.y = axis(
            local_y - it.padding.top - it.borders.top,
            it.pos.height + it.padding.height() + it.borders.height(),
            self.get_scroll_top(scroller),
            -si.padding.top,
            viewport.height,
            top,
            parent_height,
            &offsets.top,
            &offsets.bottom,
        );
        shift
    }

    /// `calc_placement(x, y)`: the item's content box moved by `x`, `y` less its scroll position, plus its sticky
    /// shift: where its children's places start from.
    pub fn calc_placement(&self, id: RenderId, x: i32, y: i32) -> Position {
        let mut pos = self.render.item(id).pos;
        let sticky = self.sticky_offset(id);
        pos.x += Px::from_int(x) - self.get_scroll_left(id) + sticky.x;
        pos.y += Px::from_int(y) - self.get_scroll_top(id) + sticky.y;
        pos
    }

    /// `get_placement()`: the item's content box in the document, its sticky shift in, through its ancestors'
    /// placements up to the first fixed one (or itself), which adds the viewport's corner.
    pub fn get_placement(&self, cx: &StyleCx, id: RenderId) -> Position {
        let mut pos = self.render.item(id).pos;
        let sticky = self.sticky_offset(id);
        pos.x += sticky.x;
        pos.y += sticky.y;
        if self.ri_css(id).m_el_position == element_position_fixed {
            let view_port = cx.container.get_viewport();
            pos.x += view_port.left();
            pos.y += view_port.top();
            return pos;
        }
        let mut cur_el = self.render.parent(id);
        while let Some(cur) = cur_el {
            let p = self.calc_placement(cur, 0, 0);
            pos.x += p.x;
            pos.y += p.y;

            if self.ri_css(cur).m_el_position == element_position_fixed {
                let view_port = cx.container.get_viewport();
                pos.x += view_port.left();
                pos.y += view_port.top();
                return pos;
            }

            cur_el = self.render.parent(cur);
        }
        pos
    }

    /// `get_rendering_boxes(redraw_box)`: the item's boxes (an inline's boxes, else its border box) in the
    /// document, through its ancestors' places, scroll positions and sticky shifts (a fixed box's from the viewport).
    pub fn get_rendering_boxes(&self, cx: &StyleCx, id: RenderId, redraw_box: &mut dyn FnMut(Position)) {
        let sticky = self.sticky_offset(id);
        let mut add_x = sticky.x;
        let mut add_y = sticky.y;

        if self.ri_css(id).m_el_position != element_position_fixed {
            let mut cur_el = self.render.parent(id);
            while let Some(cur) = cur_el {
                let pos = self.render.item(cur).pos;
                if self.ri_css(cur).m_el_position == element_position_fixed {
                    let view_port = cx.container.get_viewport();
                    add_x += pos.x + view_port.left() - self.get_scroll_left(cur);
                    add_y += pos.y + view_port.top() - self.get_scroll_top(cur);
                    break;
                }
                let parent_sticky = self.sticky_offset(cur);
                add_x += pos.x - self.get_scroll_left(cur) + parent_sticky.x;
                add_y += pos.y - self.get_scroll_top(cur) + parent_sticky.y;
                cur_el = self.render.parent(cur);
            }
        } else {
            let view_port = cx.container.get_viewport();
            add_x = view_port.left();
            add_y = view_port.top();
        }

        match self.inline_boxes_of(id) {
            Some(boxes) => {
                let view = self.render.item(id).scroll_view;
                for b in boxes {
                    let mut b = b;
                    if let Some(view) = view {
                        b.x -= view.left;
                        b.y -= view.top;
                    }
                    b.x += add_x;
                    b.y += add_y;
                    redraw_box(b);
                }
            }
            None => {
                let it = self.render.item(id);
                let mut pos = it.pos;
                pos.grow(&it.padding);
                pos.grow(&it.borders);
                pos.x += add_x;
                pos.y += add_y;
                redraw_box(pos);
            }
        }
    }

    /// `calc_document_size(sz, x, y)`: what the item and the items it does not clip reach, and a scroll view for a
    /// box that scrolls or hides its overflow.
    pub fn calc_document_size(&mut self, id: RenderId, sz: &mut Size, x: Px, y: Px) {
        if let Some(boxes) = self.inline_boxes_of(id) {
            for b in boxes {
                sz.width = sz.width.max(x + b.x + b.width);
                sz.height = sz.height.max(y + b.y + b.height);
            }
            return;
        }
        if !self.is_visible(id) {
            return;
        }
        let css = self.ri_css(id);
        let (position, overflow, display, clips) =
            (css.m_el_position, css.m_overflow, css.m_display, css.m_overflow > overflow_visible || (css.m_contain & contain_paint) != 0);
        if position != element_position_fixed {
            let it = self.render.item(id);
            sz.width = sz.width.max(x + it.right());
            sz.height = sz.height.max(y + it.bottom());
        }

        let children = self.render.children(id).to_vec();
        if overflow == overflow_scroll || overflow == overflow_auto || overflow == overflow_hidden {
            let mut child_size = Size::default();
            let element = self.render.item(id).element;
            if self.tag(element) == crate::string_id::_textarea_ && self.render.item(id).kind == RenderKind::render_item_inline_context {
                child_size = self.text_content_size(id);
            } else {
                for &el in &children {
                    self.calc_document_size(el, &mut child_size, Px(0.0), Px(0.0));
                }
            }
            let it = self.render.item_mut(id);
            let mut padding_box = it.pos;
            padding_box.grow(&it.padding);
            let scroll_size =
                Size::new(child_size.width + it.padding.left + it.padding.right, child_size.height + it.padding.top + it.padding.bottom);
            match it.scroll_view.as_mut() {
                None => it.scroll_view = Some(ScrollView { viewport: padding_box, scroll_size, left: Px(0.0), top: Px(0.0) }),
                Some(view) => view.set(padding_box, scroll_size),
            }
        } else {
            // All children of tables and blocks with style other than "overflow: visible" are inside element.
            // We can skip calculating the size of children
            let pos = self.render.item(id).pos;
            if !clips && display != display_table {
                for &el in &children {
                    self.calc_document_size(el, sz, x + pos.x, y + pos.y);
                }
            } else {
                let mut child_size = Size::default();
                for &el in &children {
                    self.calc_document_size(el, &mut child_size, Px(0.0), Px(0.0));
                }
            }
        }
    }
}
