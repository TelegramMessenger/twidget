//! `flex_item.h` and `flex_item.cpp`: a flex item (stage L10), one per box in a flex container's flow, of the row
//! or the column class: its flex factors (thousandths), outer base, minimum, maximum and hypothetical main sizes,
//! `order`, `auto` margins and alignment as `init()` finds them, and `place()`, which puts its render item on its
//! line: `auto` margins, the main position, then the cross position by `align-self` (laid out again to stretch).

use crate::Px;
use crate::css_values::*;
use crate::document::{Document, StyleCx};
use crate::flex_line::FlexLine;
use crate::formatting_context::FormattingContext;
use crate::render_item::*;
use crate::types::*;

const flex_align_items_auto: i32 = FlexAlignItems::flex_align_items_auto.index();
const flex_align_items_baseline: i32 = FlexAlignItems::flex_align_items_baseline.index();
const flex_align_items_center: i32 = FlexAlignItems::flex_align_items_center.index();
const flex_align_items_start: i32 = FlexAlignItems::flex_align_items_start.index();
const flex_align_items_end: i32 = FlexAlignItems::flex_align_items_end.index();
const flex_align_items_flex_start: i32 = FlexAlignItems::flex_align_items_flex_start.index();
const flex_align_items_flex_end: i32 = FlexAlignItems::flex_align_items_flex_end.index();
pub(crate) const flex_align_items_last: i32 = FlexAlignItems::flex_align_items_last.index();
const flex_basis_auto: i32 = FlexBasis::flex_basis_auto.index();
const flex_basis_content: i32 = FlexBasis::flex_basis_content.index();
const flex_basis_fit_content: i32 = FlexBasis::flex_basis_fit_content.index();
const flex_basis_min_content: i32 = FlexBasis::flex_basis_min_content.index();
const flex_basis_max_content: i32 = FlexBasis::flex_basis_max_content.index();

/// `flex_clamp_state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FlexClampState {
    flex_clamp_state_unclamped,
    flex_clamp_state_inflexible,
    flex_clamp_state_min_violation,
    flex_clamp_state_max_violation,
}

/// `flex_item`, `flex_item_row_direction` (`row`) or `flex_item_column_direction`. Sizes are outer (margin box)
/// sizes; `main_size` is the hypothetical main size until the line distributes its free space, the used one after.
#[derive(Clone, Debug)]
pub struct FlexItem {
    pub el: RenderId,
    pub row: bool,
    pub base_size: Px,
    pub min_size: Px,
    pub max_size: DefValue<Px>,
    pub main_size: Px,
    pub grow: i32,
    pub shrink: i32,
    pub scaled_flex_shrink_factor: Px,
    pub frozen: bool,
    pub clamp_state: FlexClampState,
    pub order: i32,
    pub src_order: i32,
    pub auto_margin_main_start: DefValue<Px>,
    pub auto_margin_main_end: DefValue<Px>,
    pub auto_margin_cross_start: bool,
    pub auto_margin_cross_end: bool,
    /// A `flex_align_items` value with its `first`/`last`/`safe`/`unsafe` flags.
    pub align: i32,
}

impl FlexItem {
    /// `flex_item_row_direction(el)` or `flex_item_column_direction(el)`.
    pub fn new(el: RenderId, row: bool) -> FlexItem {
        FlexItem {
            el,
            row,
            base_size: Px(0.0),
            min_size: Px(0.0),
            max_size: DefValue::new(Px(0.0)),
            main_size: Px(0.0),
            grow: 0,
            shrink: 0,
            scaled_flex_shrink_factor: Px(0.0),
            frozen: false,
            clamp_state: FlexClampState::flex_clamp_state_unclamped,
            order: 0,
            src_order: 0,
            auto_margin_main_start: DefValue::new(Px(0.0)),
            auto_margin_main_end: DefValue::new(Px(0.0)),
            auto_margin_cross_start: false,
            auto_margin_cross_end: false,
            align: flex_align_items_auto,
        }
    }
}

/// `static_cast<int>(std::fmin(std::fmax(std::nearbyint(factor * 1000.0), low), 2147483647.0))`: `fmax` and `fmin`
/// drop a NaN where `clamp` would keep it.
#[allow(clippy::manual_clamp)]
fn thousandths(factor: f32, low: f64) -> i32 {
    (f64::from(factor) * 1000.0).round_ties_even().max(low).min(2147483647.0) as i32
}

impl Document {
    /// `render_offset_width()`: what lies outside the box `width` sizes (margins, and padding and borders under
    /// `content-box`), added as margin, border, padding.
    pub fn render_offset_width(&self, id: RenderId) -> Px {
        self.render_offset_left(id) + self.render_offset_right(id)
    }

    /// `render_offset_height()`.
    pub fn render_offset_height(&self, id: RenderId) -> Px {
        self.render_offset_top(id) + self.render_offset_bottom(id)
    }

    fn content_box_sizing(&self, id: RenderId) -> bool {
        self.ri_css(id).m_box_sizing == BoxSizing::box_sizing_content_box.index()
    }

    fn render_offset_left(&self, id: RenderId) -> Px {
        let it = self.render.item(id);
        if self.content_box_sizing(id) { it.margins.left + it.borders.left + it.padding.left } else { it.margins.left }
    }

    fn render_offset_right(&self, id: RenderId) -> Px {
        let it = self.render.item(id);
        if self.content_box_sizing(id) { it.margins.right + it.borders.right + it.padding.right } else { it.margins.right }
    }

    fn render_offset_top(&self, id: RenderId) -> Px {
        let it = self.render.item(id);
        if self.content_box_sizing(id) { it.margins.top + it.borders.top + it.padding.top } else { it.margins.top }
    }

    fn render_offset_bottom(&self, id: RenderId) -> Px {
        let it = self.render.item(id);
        if self.content_box_sizing(id) { it.margins.bottom + it.borders.bottom + it.padding.bottom } else { it.margins.bottom }
    }

    /// `flex_item::init(self_size, fmt_ctx, align_items)`: the flex factors in thousandths (a negative grow is 0, a
    /// negative shrink 1000), the outlines against the container's width, `order`, the class's sizes, the
    /// alignment, the hypothetical main size (the base size clamped by the minimum, then the maximum).
    #[inline(never)]
    pub(crate) fn flex_item_init(
        &mut self,
        cx: &StyleCx,
        item: &mut FlexItem,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
        align_items: i32,
    ) {
        let el = item.el;
        let (flex_grow, flex_shrink) = {
            let css = self.ri_css(el);
            (css.m_flex_grow, css.m_flex_shrink)
        };
        item.grow = thousandths(flex_grow, 0.0);
        // Negative numbers are invalid.
        // https://www.w3.org/TR/css-flexbox-1/#valdef-flex-grow-number
        item.grow = item.grow.max(0);

        item.shrink = thousandths(flex_shrink, -1.0);
        // Negative numbers are invalid.
        // https://www.w3.org/TR/css-flexbox-1/#valdef-flex-shrink-number
        if item.shrink < 0 {
            item.shrink = 1000;
        }

        self.calc_outlines(el, self_size.render_width.px());
        item.order = self.ri_css(el).m_order;

        if item.row {
            self.flex_item_row_init(cx, item, self_size, fmt_ctx);
        } else {
            self.flex_item_column_init(cx, item, self_size, fmt_ctx);
        }

        let align_self = self.ri_css(el).m_flex_align_self;
        item.align = if align_self == flex_align_items_auto { align_items } else { align_self };

        if item.base_size < item.min_size {
            item.main_size = item.min_size;
        } else if !item.max_size.is_default() && item.base_size > item.max_size.get() {
            item.main_size = item.max_size.get();
        } else {
            item.main_size = item.base_size;
        }

        item.frozen = false;
    }

    /// `flex_item_row_direction::direction_specific_init`: `auto` margins; the minimum (`min-width`, or the
    /// min-content width, no more than a set `width`, for a box that does not clip); the maximum; the base size
    /// from `flex-basis` (`auto` with no `width` the content's), the scaled shrink factor.
    #[inline(never)]
    fn flex_item_row_init(
        &mut self,
        cx: &StyleCx,
        item: &mut FlexItem,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) {
        let el = item.el;
        let render_width = self_size.render_width.px();
        let css = self.ri_css(el);
        if css.m_css_margins.left.is_predefined() {
            item.auto_margin_main_start.set(Px(0.0));
        }
        if css.m_css_margins.right.is_predefined() {
            item.auto_margin_main_end.set(Px(0.0));
        }
        if css.m_css_margins.top.is_predefined() {
            item.auto_margin_cross_start = true;
        }
        if css.m_css_margins.bottom.is_predefined() {
            item.auto_margin_cross_end = true;
        }
        let mut content_size = DefValue::new(Px(0.0));
        if css.m_css_min_width.is_predefined() {
            let overflow = css.m_overflow;
            if !(overflow == overflow_visible || overflow == overflow_clip) {
                item.min_size = self.render_offset_width(el);
            } else {
                let cb = self_size.new_width(self.render.item(el).content_offset_width(), size_mode_content);
                item.min_size = self.measure(cx, el, Px(0.0), Px(0.0), &cb, Some(&mut *fmt_ctx), false, None).natural_width;
                content_size.set(item.min_size);
                let width = &self.ri_css(el).m_css_width;
                if !width.is_predefined() {
                    let specified = width.calc_percent(render_width) + self.render_offset_width(el);
                    if specified >= Px(0.0) && specified < item.min_size {
                        item.min_size = specified;
                    }
                }
            }
        } else {
            item.min_size = css.m_css_min_width.calc_percent(render_width) + self.render_offset_width(el);
        }
        let css = self.ri_css(el);
        if !css.m_css_max_width.is_predefined() {
            item.max_size.set(css.m_css_max_width.calc_percent(render_width) + self.render_offset_width(el));
        }
        let mut flex_basis_predefined = css.m_flex_basis.is_predefined();
        let mut predef = flex_basis_auto;
        if flex_basis_predefined {
            predef = css.m_flex_basis.predef();
        } else if css.m_flex_basis.val() < 0.0 {
            flex_basis_predefined = true;
        }

        if flex_basis_predefined {
            if predef == flex_basis_auto && css.m_css_width.is_predefined() {
                // if width is not predefined, use content size as base size
                predef = flex_basis_content;
            }

            if predef == flex_basis_auto {
                item.base_size = css.m_css_width.calc_percent(render_width) + self.render_offset_width(el);
            } else if predef == flex_basis_fit_content || predef == flex_basis_content {
                let cb = self_size.new_width(
                    self_size.render_width.value + self.render.item(el).content_offset_width(),
                    size_mode_content | size_mode_exact_width,
                );
                item.base_size = self.measure(cx, el, Px(0.0), Px(0.0), &cb, Some(&mut *fmt_ctx), false, None).natural_width;
            } else if predef == flex_basis_min_content {
                if content_size.is_default() {
                    let cb = self_size.new_width(self.render.item(el).content_offset_width(), size_mode_content);
                    content_size.set(self.measure(cx, el, Px(0.0), Px(0.0), &cb, Some(&mut *fmt_ctx), false, None).natural_width);
                }
                item.base_size = content_size.get();
            } else if predef == flex_basis_max_content {
                self.render(cx, el, Px(0.0), Px(0.0), self_size, Some(&mut *fmt_ctx), false);
                item.base_size = self.render.item(el).width();
            } else {
                item.base_size = Px(0.0);
            }
        } else {
            item.base_size = css.m_flex_basis.calc_percent(render_width) + self.render_offset_width(el);
        }

        item.scaled_flex_shrink_factor = (item.base_size - self.render_offset_width(el)) * Px::from_int(item.shrink);
    }

    /// `flex_item_column_direction::direction_specific_init`: as the row's on the vertical axis, the minimum the
    /// content's height measured (the render offsets for a clipping box in a container of a set height), the base
    /// size `auto` with no `height` the content's (the same measure again unless `LH_NO_COLUMN_REUSE`), a % basis
    /// only against a container's absolute height.
    #[inline(never)]
    fn flex_item_column_init(
        &mut self,
        cx: &StyleCx,
        item: &mut FlexItem,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) {
        let el = item.el;
        let height = self_size.height.px();
        let css = self.ri_css(el);
        if css.m_css_margins.top.is_predefined() {
            item.auto_margin_main_start.set(Px(0.0));
        }
        if css.m_css_margins.bottom.is_predefined() {
            item.auto_margin_main_end.set(Px(0.0));
        }
        if css.m_css_margins.left.is_predefined() {
            item.auto_margin_cross_start = true;
        }
        if css.m_css_margins.right.is_predefined() {
            item.auto_margin_cross_end = true;
        }
        let mut content_height = DefValue::new(Px(0.0));
        if css.m_css_min_height.is_predefined() {
            let cb = self_size.new_width(self_size.render_width.px(), size_mode_content);
            content_height.set(self.measure_height(cx, el, Px(0.0), Px(0.0), &cb, Some(&mut *fmt_ctx), false));
            let overflow = self.ri_css(el).m_overflow;
            item.min_size = if !(overflow == overflow_visible || overflow == overflow_clip) && self_size.height.ty != CbcValueType::auto {
                self.render_offset_height(el)
            } else {
                content_height.get()
            };
        } else {
            item.min_size = css.m_css_min_height.calc_percent(height) + self.render_offset_height(el);
        }
        let css = self.ri_css(el);
        if !css.m_css_max_height.is_predefined() {
            item.max_size.set(css.m_css_max_height.calc_percent(height) + self.render_offset_height(el));
        }

        let mut flex_basis_predefined = css.m_flex_basis.is_predefined();
        let mut predef = flex_basis_auto;
        if flex_basis_predefined {
            predef = css.m_flex_basis.predef();
        } else if css.m_flex_basis.val() < 0.0 {
            flex_basis_predefined = true;
        }

        if flex_basis_predefined {
            if predef == flex_basis_auto && css.m_css_height.is_predefined() {
                predef = flex_basis_fit_content;
            }
            if predef == flex_basis_auto {
                item.base_size = css.m_css_height.calc_percent(height) + self.render_offset_height(el);
            } else if predef == flex_basis_max_content || predef == flex_basis_fit_content {
                if !content_height.is_default() && cx.switches.column_reuse {
                    item.base_size = content_height.get();
                } else {
                    item.base_size = self.measure_height(cx, el, Px(0.0), Px(0.0), self_size, Some(&mut *fmt_ctx), false);
                }
            } else if predef == flex_basis_min_content {
                item.base_size = item.min_size;
            } else {
                item.base_size = Px(0.0);
            }
        } else if css.m_flex_basis.units() == CssUnits::css_units_percentage {
            if self_size.height.ty == CbcValueType::absolute {
                item.base_size = css.m_flex_basis.calc_percent(height) + self.render_offset_height(el);
            } else {
                item.base_size = Px(0.0);
            }
        } else {
            item.base_size = Px(css.m_flex_basis.val()) + self.render_offset_height(el);
        }

        item.scaled_flex_shrink_factor = (item.base_size - self.render_offset_height(el)) * Px::from_int(item.shrink);
    }

    /// `flex_item::place(ln, main_pos, self_size, fmt_ctx)`: the main `auto` margins and position, then the cross
    /// position: the cross `auto` margins (the row class's take it all; the column class's set the left margin and
    /// let alignment go on), else by the alignment, stretching (laid out again) for `normal`, `stretch`,
    /// `self-start`, `self-end` and `auto`.
    #[inline(never)]
    pub(crate) fn flex_item_place(
        &mut self,
        cx: &StyleCx,
        item: &FlexItem,
        ln: &FlexLine,
        main_pos: Px,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) {
        self.flex_item_apply_main_auto_margins(item);
        self.flex_item_set_main_position(item, main_pos);
        if !self.flex_item_apply_cross_auto_margins(item, ln.cross_size) {
            let align = item.align & 0xFF;
            if align == flex_align_items_baseline {
                self.flex_item_align_baseline(item, ln);
            } else if align == flex_align_items_flex_end {
                if ln.reverse_cross {
                    // If cross axis is reversed position item from start
                    self.flex_item_set_cross_position(item, ln.cross_start);
                } else {
                    let pos = ln.cross_start + ln.cross_size - self.flex_item_el_cross_size(item);
                    self.flex_item_set_cross_position(item, pos);
                }
            } else if align == flex_align_items_end {
                let pos = ln.cross_start + ln.cross_size - self.flex_item_el_cross_size(item);
                self.flex_item_set_cross_position(item, pos);
            } else if align == flex_align_items_center {
                let pos = ln.cross_start + ln.cross_size / Px(2.0) - self.flex_item_el_cross_size(item) / Px(2.0);
                self.flex_item_set_cross_position(item, pos);
            } else if align == flex_align_items_flex_start {
                if ln.reverse_cross {
                    // If cross axis is reversed position item from end
                    let pos = ln.cross_start + ln.cross_size - self.flex_item_el_cross_size(item);
                    self.flex_item_set_cross_position(item, pos);
                } else {
                    self.flex_item_set_cross_position(item, ln.cross_start);
                }
            } else if align == flex_align_items_start {
                self.flex_item_set_cross_position(item, ln.cross_start);
            } else {
                self.flex_item_align_stretch(cx, item, ln, self_size, fmt_ctx);
            }
        }
    }

    /// `flex_item::get_last_baseline(type)`: the item's last baseline from its top or bottom edge (nothing in the
    /// fork calls it).
    pub fn flex_item_get_last_baseline(&self, item: &FlexItem, ty: BaselineType) -> Px {
        if ty == BaselineType::baseline_type_top {
            return self.get_last_baseline(item.el);
        }
        if ty == BaselineType::baseline_type_bottom {
            return self.render.item(item.el).height() - self.get_last_baseline(item.el);
        }
        Px(0.0)
    }

    /// `flex_item::get_first_baseline(type)` (nothing in the fork calls it).
    pub fn flex_item_get_first_baseline(&self, item: &FlexItem, ty: BaselineType) -> Px {
        if ty == BaselineType::baseline_type_top {
            return self.get_first_baseline(item.el);
        }
        if ty == BaselineType::baseline_type_bottom {
            return self.render.item(item.el).height() - self.get_first_baseline(item.el);
        }
        Px(0.0)
    }

    /// `apply_main_auto_margins()`: the main `auto` margins the line gave, set and the box moved by the start one.
    pub(crate) fn flex_item_apply_main_auto_margins(&mut self, item: &FlexItem) {
        let it = self.render.item_mut(item.el);
        // apply auto margins to item
        if item.row {
            if !item.auto_margin_main_start.is_default() {
                it.margins.left = item.auto_margin_main_start.get();
                it.pos.x += item.auto_margin_main_start.get();
            }
            if !item.auto_margin_main_end.is_default() {
                it.margins.right = item.auto_margin_main_end.get();
            }
        } else {
            if !item.auto_margin_main_start.is_default() {
                it.margins.top = item.auto_margin_main_start.get();
                it.pos.y += item.auto_margin_main_start.get();
            }
            if !item.auto_margin_main_end.is_default() {
                it.margins.bottom = item.auto_margin_main_end.get();
            }
        }
    }

    /// `apply_cross_auto_margins(cross_size)`: the line's free cross size shared by the cross `auto` margins. The row
    /// class puts the box at its new top margin (not below the line's start) and is done; the column class moves the
    /// box right by its offset and answers that it is not.
    fn flex_item_apply_cross_auto_margins(&mut self, item: &FlexItem, cross_size: Px) -> bool {
        if item.auto_margin_cross_end || item.auto_margin_cross_start {
            let mut margins_num = 0;
            if item.auto_margin_cross_end {
                margins_num += 1;
            }
            if item.auto_margin_cross_start {
                margins_num += 1;
            }
            let it = self.render.item_mut(item.el);
            if item.row {
                let margin = (cross_size - it.height()) / Px::from_int(margins_num);
                if item.auto_margin_cross_start {
                    it.margins.top = margin;
                    it.pos.y = it.content_offset_top();
                }
                if item.auto_margin_cross_end {
                    it.margins.bottom = margin;
                }
                return true;
            }
            let margin = (cross_size - it.width()) / Px::from_int(margins_num);
            if item.auto_margin_cross_start {
                it.margins.left = margin;
                it.pos.x += it.content_offset_left();
            }
            if item.auto_margin_cross_end {
                it.margins.right = margin;
            }
        }
        false
    }

    /// `set_main_position(pos)`.
    fn flex_item_set_main_position(&mut self, item: &FlexItem, pos: Px) {
        let it = self.render.item_mut(item.el);
        if item.row {
            it.pos.x = pos + it.content_offset_left();
        } else {
            it.pos.y = pos + it.content_offset_top();
        }
    }

    /// `set_cross_position(pos)`.
    fn flex_item_set_cross_position(&mut self, item: &FlexItem, pos: Px) {
        let it = self.render.item_mut(item.el);
        if item.row {
            it.pos.y = pos + it.content_offset_top();
        } else {
            it.pos.x = pos + it.content_offset_left();
        }
    }

    /// `get_el_main_size()`: the box's outer width (row) or height (column).
    pub(crate) fn flex_item_el_main_size(&self, item: &FlexItem) -> Px {
        let it = self.render.item(item.el);
        if item.row { it.width() } else { it.height() }
    }

    /// `get_el_cross_size()`.
    fn flex_item_el_cross_size(&self, item: &FlexItem) -> Px {
        let it = self.render.item(item.el);
        if item.row { it.height() } else { it.width() }
    }

    /// `align_stretch(ln, self_size, fmt_ctx)`: a row's item at the line's top, laid out again as high as the line
    /// when its `height` is `auto`; a column's item laid out again as wide as the line (its width exact unless set)
    /// at the line's start. The main `auto` margins the layout dropped set again.
    fn flex_item_align_stretch(
        &mut self,
        cx: &StyleCx,
        item: &FlexItem,
        ln: &FlexLine,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) {
        let el = item.el;
        if item.row {
            self.flex_item_set_cross_position(item, ln.cross_start);
            if self.ri_css(el).m_css_height.is_predefined() {
                let it = self.render.item(el);
                let (x, y) = (it.left(), it.top());
                let cb = self_size.new_width_height(
                    it.pos.width + self.box_sizing_width(el),
                    ln.cross_size - it.content_offset_height() + self.box_sizing_height(el),
                    size_mode_exact_width | size_mode_exact_height,
                );
                self.render(cx, el, x, y, &cb, Some(fmt_ctx), false);
                self.flex_item_apply_main_auto_margins(item);
            }
            return;
        }
        // MAIN:  Y
        // CROSS: X
        let size_mode = if !self.ri_css(el).m_css_width.is_predefined() {
            size_mode_exact_height
        } else {
            size_mode_exact_width | size_mode_exact_height
        };
        let it = self.render.item(el);
        let y = it.pos.y - it.content_offset_top();
        let cb = self_size.new_width_height(
            ln.cross_size - it.content_offset_width() + self.box_sizing_width(el),
            it.pos.height + self.box_sizing_height(el),
            size_mode,
        );
        self.render(cx, el, ln.cross_start, y, &cb, Some(fmt_ctx), false);
        // Apply auto margins after rendering
        self.flex_item_apply_main_auto_margins(item);
    }

    /// `align_baseline(ln, self_size, fmt_ctx)`: a row's item with its first (or, `last baseline`, last) baseline on
    /// the line's; a column's at the start (the end for `last baseline`), swapped when the cross axis is reversed.
    fn flex_item_align_baseline(&mut self, item: &FlexItem, ln: &FlexLine) {
        let el = item.el;
        if item.row {
            let pos = if (item.align & flex_align_items_last) != 0 {
                ln.cross_start + ln.last_baseline.get_offset_from_top(ln.cross_size) - self.get_last_baseline(el)
            } else {
                ln.cross_start + ln.first_baseline.get_offset_from_top(ln.cross_size) - self.get_first_baseline(el)
            };
            self.flex_item_set_cross_position(item, pos);
            return;
        }
        // The fallback alignment for first baseline is start, the one for last baseline is end.
        let at_end = if (item.align & flex_align_items_last) != 0 { !ln.reverse_cross } else { ln.reverse_cross };
        if at_end {
            let pos = ln.cross_start + ln.cross_size - self.flex_item_el_cross_size(item);
            self.flex_item_set_cross_position(item, pos);
        } else {
            self.flex_item_set_cross_position(item, ln.cross_start);
        }
    }
}
