//! `render_block_context.h` and `render_block_context.cpp`: a block formatting context's content (stage L7). Its
//! boxes go one below another: floats placed beside the flow, absolute and fixed boxes given their in-flow pass,
//! each other box below what it clears, its top margin collapsed with the previous box's bottom margin (or, the
//! first, with the block's own top margin), moved below the floats a formatting context or a replaced box or a table
//! cannot share a line with, centred or pushed by `auto` margins, shifted when relatively positioned; the block as
//! high as its boxes, its last box's bottom margin collapsed through. Its baselines, and `vertical-align` in a table
//! cell (stage L12 asks it).

use crate::Px;
use crate::document::{Document, StyleCx};
use crate::formatting_context::{ElPosition, FormattingContext};
use crate::render_item::*;
use crate::types::*;

impl Document {
    /// `render_item_block_context::_render_content(x, y, second_pass, self_size, fmt_ctx)`.
    pub(crate) fn block_context_render_content(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        second_pass: bool,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) -> RenderedWidth {
        self.render.item_mut(this).va_applied = Px(0.0);
        let mut last_margin_el: Option<RenderId> = None;

        let mut ret_width = RenderedWidth::default();
        let mut child_top = Px(0.0);
        let mut last_margin = Px(0.0);
        let mut is_first = true;
        let mut k = 0;
        while k < self.render.children(this).len() {
            let el = self.render.children(this)[k];
            k += 1;
            let (el_position, el_float, el_display) = {
                let css = self.ri_css(el);
                (css.m_el_position, css.m_float, css.m_display)
            };
            // we don't need to process absolute and fixed positioned element on the second pass
            if second_pass && (el_position == element_position_absolute || el_position == element_position_fixed) {
                continue;
            }

            if el_float != float_none {
                let rw = self.place_float(cx, this, None, el, child_top, self_size, fmt_ctx);
                ret_width.merge(&rw);
            } else if el_display != display_none {
                if el_position == element_position_absolute || el_position == element_position_fixed {
                    self.render_out_of_flow(cx, el, child_top, self_size, fmt_ctx);
                } else {
                    self.block_context_place_child(
                        cx,
                        this,
                        el,
                        self_size,
                        fmt_ctx,
                        &mut ChildFlow { child_top, last_margin, is_first, ret_width },
                    )
                    .apply(&mut child_top, &mut last_margin, &mut is_first, &mut ret_width);
                    last_margin_el = Some(el);
                }
            }
        }

        let collapse = self.collapse_bottom_margin(this);
        let it = self.render.item_mut(this);
        if self_size.height.ty != CbcValueType::auto && self_size.height.value > Px(0.0) {
            it.pos.height = self_size.height.px();
        } else {
            it.pos.height = child_top;
            if collapse {
                it.pos.height -= last_margin;
                it.margins.bottom = it.margins.bottom.max(last_margin);
                if let Some(last) = last_margin_el {
                    self.render.item_mut(last).margins.bottom = Px(0.0);
                }
            }
        }

        ret_width
    }

    /// The loop's body for a box in the flow: kept out of the loop's frame, which every level of a deep document
    /// keeps on the stack.
    #[inline(never)]
    fn block_context_place_child(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        el: RenderId,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
        flow: &mut ChildFlow,
    ) -> ChildFlow {
        let last_margin = flow.last_margin;
        let is_first = flow.is_first;
        let mut child_top = fmt_ctx.get_cleared_top(self, el, flow.child_top);
        let child_x = Px(0.0);
        let child_width = self_size.render_width.px();
        let mut top_margin = self.render.item(this).margins.top;

        self.calc_outlines(el, self_size.width.px());

        let base_child_top = child_top;
        let base_top_margin = top_margin;
        let collapse_top = self.collapse_top_margin(this);
        let collapse = |doc: &Document, margin: Px, child_top: &mut Px, top_margin: &mut Px| {
            *child_top = base_child_top;
            *top_margin = base_top_margin;
            if is_first && collapse_top {
                if margin > Px(0.0) {
                    *child_top -= margin;
                    if margin > doc.render.item(this).margins.top {
                        *top_margin = margin;
                    }
                }
            } else if margin > Px(0.0) {
                *child_top -= last_margin.min(margin);
            }
        };
        let css_top = self.render.item(el).margins.top;
        collapse(self, css_top, &mut child_top, &mut top_margin);

        let mut rw = self.render(cx, el, child_x, child_top, &self_size.new_width(child_width, size_mode_normal), Some(fmt_ctx), false);
        let margin_top = self.render.item(el).margins.top;
        if margin_top != css_top {
            let before = child_top;
            collapse(self, margin_top, &mut child_top, &mut top_margin);
            if child_top != before {
                rw = self.render(cx, el, child_x, child_top, &self_size.new_width(child_width, size_mode_normal), Some(fmt_ctx), false);
            }
        }
        // Render table with "width: auto" into returned width
        if self.ri_css(el).m_display == display_table && rw.natural_width < child_width && self.ri_css(el).m_css_width.is_predefined() {
            self.render(cx, el, child_x, child_top, &self_size.new_width(rw.natural_width, size_mode_normal), Some(fmt_ctx), false);
        }

        // Check if we need to move block to the next line
        let el_element = self.render.item(el).element;
        if self.is_replaced(el_element) || self.is_block_formatting_context(el_element) || self.ri_css(el).m_display == display_table {
            let mut el_pos = ElPosition::default();
            {
                let it = self.render.item(el);
                el_pos.el_pos = it.pos;
                el_pos.el_pos.grow(&it.margins);
                el_pos.el_pos.grow(&it.padding);
                el_pos.el_pos.grow(&it.borders);
                el_pos.el_pos.width = rw.min_width;
                el_pos.container_width = self_size.render_width.px();
                el_pos.el_margins = it.margins;
            }
            let new_pos = fmt_ctx.place_to_left(&el_pos);
            if new_pos.found {
                child_top = new_pos.top;
                let it = self.render.item_mut(el);
                it.pos.x = new_pos.left + it.content_offset_left();
                it.pos.y = child_top + it.content_offset_top();
                //  Rollback top margin collapse if the block was moved to the new line
                if is_first && new_pos.new_line && self.collapse_top_margin(this) {
                    top_margin = self.render.item(this).margins.top;
                }
            }
            if new_pos.width != self_size.render_width.px() && self.ri_css(el).m_css_width.is_predefined() {
                rw = self.render(
                    cx,
                    el,
                    new_pos.left,
                    new_pos.top,
                    &self_size.new_width(new_pos.width, size_mode_normal),
                    Some(fmt_ctx),
                    false,
                );
            }
            rw.natural_width = self_size.render_width.value - new_pos.width + rw.natural_width;
        }

        let auto_margin = self.calc_auto_margins(el, child_width);
        if auto_margin != Px(0.0) {
            self.render.item_mut(el).pos.x += auto_margin;
        } else if self.ri_css(this).m_direction == direction_rtl
            && !self.ri_css(el).m_css_margins.left.is_predefined()
            && !self.ri_css(el).m_css_margins.right.is_predefined()
        {
            // Right to left, a block narrower than its container keeps its right margin and sits on the right.
            let shift = self_size.render_width.px() - self.render.item(el).right();
            self.render.item_mut(el).pos.x += shift;
        }
        let mut ret_width = flow.ret_width;
        ret_width.merge(&rw);
        self.render.item_mut(this).margins.top = top_margin;
        child_top += self.render.item(el).height();
        let last_margin = self.render.item(el).margins.bottom;

        if self.ri_css(el).m_el_position == element_position_relative {
            self.apply_relative_shift(el, self_size);
        }
        ChildFlow { child_top, last_margin, is_first: false, ret_width }
    }

    /// `get_first_baseline()`: the first box in the flow's, else the bottom without the bottom margin.
    pub fn block_context_first_baseline(&self, this: RenderId) -> Px {
        let it = self.render.item(this);
        for &item in &it.children {
            if self.block_in_flow(item) {
                return it.content_offset_top() + self.render.item(item).top() + self.get_first_baseline(item);
            }
        }
        it.height() - it.margins.bottom
    }

    /// `get_last_baseline()`: the last box in the flow's.
    pub fn block_context_last_baseline(&self, this: RenderId) -> Px {
        let it = self.render.item(this);
        for &item in it.children.iter().rev() {
            if self.block_in_flow(item) {
                return it.content_offset_top() + self.render.item(item).top() + self.get_last_baseline(item);
            }
        }
        it.height() - it.margins.bottom
    }

    /// `is_in_flow(item)`.
    fn block_in_flow(&self, item: RenderId) -> bool {
        let css = self.ri_css(item);
        css.m_display != display_none
            && css.m_float == float_none
            && css.m_el_position != element_position_absolute
            && css.m_el_position != element_position_fixed
    }

    /// `apply_vertical_align()`: the boxes moved down by `vertical-align`'s middle or bottom share of the room below
    /// them, from where an earlier call left them.
    pub fn block_context_apply_vertical_align(&mut self, this: RenderId) {
        let va = self.ri_css(this).m_vertical_align;
        let mut any = false;
        let mut content_bottom = Px(0.0);
        for &el in self.render.children(this) {
            let css = self.ri_css(el);
            if css.m_display == display_none
                || css.m_el_position == element_position_absolute
                || css.m_el_position == element_position_fixed
            {
                continue;
            }
            let bottom = self.render.item(el).bottom();
            content_bottom = if any { content_bottom.max(bottom) } else { bottom };
            any = true;
        }
        if !any {
            return;
        }
        let it = self.render.item(this);
        content_bottom -= it.va_applied;
        let mut add = Px(0.0);
        if it.pos.height > content_bottom {
            if va == va_middle {
                add = (it.pos.height - content_bottom) / Px(2.0);
            } else if va == va_bottom {
                add = it.pos.height - content_bottom;
            }
        }
        let shift = add - it.va_applied;
        if shift != Px(0.0) {
            for k in 0..self.render.children(this).len() {
                let el = self.render.children(this)[k];
                let position = self.ri_css(el).m_el_position;
                if position != element_position_absolute && position != element_position_fixed {
                    self.render.item_mut(el).pos.y += shift;
                }
            }
        }
        self.render.item_mut(this).va_applied = add;
    }
}

/// What the loop carries from one box in the flow to the next.
#[derive(Clone, Copy, Debug)]
struct ChildFlow {
    child_top: Px,
    last_margin: Px,
    is_first: bool,
    ret_width: RenderedWidth,
}

impl ChildFlow {
    fn apply(self, child_top: &mut Px, last_margin: &mut Px, is_first: &mut bool, ret_width: &mut RenderedWidth) {
        *child_top = self.child_top;
        *last_margin = self.last_margin;
        *is_first = self.is_first;
        *ret_width = self.ret_width;
    }
}
