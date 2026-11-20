//! `render_block.h` and `render_block.cpp`: `render_item_block::init()`, which numbers a list item, splits the
//! inline children around the block boxes inside them, and makes the block its formatting context: a
//! `render_item_block_context` with the runs of inline children wrapped in anonymous blocks when it has a
//! block-level child, else a `render_item_inline_context` (stage L6); and a block's layout (stage L7): `_render`,
//! which sizes the block around its content, `place_float`, and the class's `_render_content` and
//! `shift_content`.

use crate::Px;
use crate::document::{Document, StyleCx};
use crate::formatting_context::{ElPosition, FormattingContext, NewPosition};
use crate::render_item::*;
use crate::types::*;

impl Document {
    /// `render_item_block::init()`: what the block becomes.
    pub(crate) fn render_block_init(&mut self, cx: &StyleCx, this: RenderId) -> RenderId {
        let element = self.render.item(this).element;

        // Initialize indexes for list items
        if self.el(element).css.m_display == display_list_item && self.el(element).css.m_list_style_type >= list_style_type_armenian {
            self.number_list_item(cx, element);
        }
        // Split inline blocks with box blocks inside
        let mut k = 0;
        while k < self.render.children(this).len() {
            let child = self.render.children(this)[k];
            if self.render_display(child) == display_inline
                && !self.render.children(child).is_empty()
                && let Some((first, block, last)) = self.split_inlines(cx, child)
            {
                for part in [first, block, last] {
                    self.render.hold(part);
                }
                self.render_children_splice(this, k, &[first, block, last]);
                self.release_render(child);
                for part in [first, block, last] {
                    self.render.set_parent(part, Some(this));
                }
                continue;
            }
            k += 1;
        }

        let mut has_block_level = false;
        let mut has_inlines = false;
        for &child in self.render.children(this) {
            let el = self.render.item(child).element;
            if !self.is_float(el) {
                if self.is_block_box(el) {
                    has_block_level = true;
                } else if self.is_inline(el) {
                    has_inlines = true;
                }
            }
            if has_block_level && has_inlines {
                break;
            }
        }
        let parent = self.render.parent(this);
        let ret = if has_block_level {
            let ret = self.new_render_item(cx, RenderKind::render_item_block_context, element);
            self.render.set_parent(ret, parent);
            let mut new_children = Vec::new();
            let mut inlines = Vec::new();
            let mut not_ws_added = false;
            for child in self.render.children(this).to_vec() {
                let el = self.render.item(child).element;
                if self.is_inline(el) {
                    inlines.push(child);
                    if !self.is_white_space(el) {
                        not_ws_added = true;
                    }
                } else {
                    if not_ws_added {
                        let anon = self.anonymous_block(cx, element, &inlines, ret);
                        not_ws_added = false;
                        new_children.push(anon);
                    }
                    new_children.push(child);
                    self.render.set_parent(child, Some(ret));
                    inlines.clear();
                }
            }
            if !inlines.is_empty() && not_ws_added {
                let anon = self.anonymous_block(cx, element, &inlines, ret);
                new_children.push(anon);
            }
            self.set_render_children(ret, new_children);
            ret
        } else {
            let ret = self.new_render_item(cx, RenderKind::render_item_inline_context, element);
            self.render.set_parent(ret, parent);
            let children = self.render.children(this).to_vec();
            self.set_render_children(ret, children.clone());
            for child in children {
                self.render.set_parent(child, Some(ret));
            }
            ret
        };

        self.add_render(element, ret);

        for k in 0..self.render.children(ret).len() {
            self.init_render_child(cx, ret, k);
        }
        ret
    }

    /// A list item's number as an attribute, `list_index`: the parent's `start` (1 without one) and one more for each
    /// list item before it among its parent's children, up to `INT_MAX`. An item its parent does not list gets none.
    #[inline(never)]
    fn number_list_item(&mut self, cx: &StyleCx, element: crate::element::ElementId) {
        let Some(p) = self.parent(element) else { return };
        let start = self.get_attr(p, b"start").unwrap_or(b"1");
        let mut val = twidget_cstd::atoi(crate::gumbo::c_str(start));
        let mut index = None;
        for &child in self.children(p) {
            if child == element {
                index = Some(val);
                break;
            }
            if self.el(child).css.m_display == display_list_item && val < i32::MAX {
                val += 1;
            }
        }
        if let Some(val) = index {
            self.set_attr(cx.names, element, b"list_index", val.to_string().as_bytes());
        }
    }

    /// An anonymous block (`html_tag(src_el())`, `display: block`) around `inlines`, its parent `parent`.
    #[inline(never)]
    fn anonymous_block(&mut self, cx: &StyleCx, element: crate::element::ElementId, inlines: &[RenderId], parent: RenderId) -> RenderId {
        let anon_el = self.new_anonymous_tag(cx, element, b"display: block");
        let anon_ri = self.new_render_item(cx, RenderKind::render_item_block, anon_el);
        for &inl in inlines {
            self.add_render_child(anon_ri, inl);
        }
        self.render.set_parent(anon_ri, Some(parent));
        anon_ri
    }
}

// ---- Layout (stage L7) -----------------------------------------------------------------------------------------

impl Document {
    /// `render_item_block::place_float(el, top, self_size, fmt_ctx)`: a float laid out below what it clears, shrunk
    /// to its natural width when its width is `auto`, placed at the left or right past the floats there, and added
    /// to the context; then the block's last line made to fit around it (`fix_line_width`, an inline context's).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn place_float(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        st: Option<&mut InlineState>,
        el: RenderId,
        top: Px,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) -> RenderedWidth {
        let line_top = fmt_ctx.get_cleared_top(self, el, top);
        let line_left = Px(0.0);

        let mut nv = self.render(
            cx,
            el,
            line_left,
            line_top,
            &self_size.new_width(self_size.render_width.px(), size_mode_normal),
            Some(fmt_ctx),
            false,
        );
        if nv.natural_width < self.render.item(el).width() && self.ri_css(el).m_css_width.is_predefined() {
            self.render(cx, el, line_left, line_top, &self_size.new_width(nv.natural_width, size_mode_normal), Some(fmt_ctx), false);
        }

        let mut new_pos = NewPosition::default();
        let mut el_pos = ElPosition::default();
        {
            let it = self.render.item(el);
            el_pos.el_pos = it.pos;
            el_pos.el_pos.grow(&it.margins);
            el_pos.el_pos.grow(&it.padding);
            el_pos.el_pos.grow(&it.borders);
        }
        el_pos.container_width = self_size.render_width.px();
        let float = self.ri_css(el).m_float;
        if float == float_left {
            new_pos = fmt_ctx.place_to_left(&el_pos);
        } else if float == float_right {
            let it = self.render.item_mut(el);
            it.pos.x = self_size.render_width.value - el_pos.el_pos.width + it.content_offset_left();
            el_pos.el_pos.x = self_size.render_width.value - el_pos.el_pos.width;
            new_pos = fmt_ctx.place_to_right(&el_pos);
        }
        nv.min_width = nv.natural_width;
        if new_pos.found {
            let it = self.render.item_mut(el);
            it.pos.x = new_pos.left + it.content_offset_left();
            it.pos.y = new_pos.top + it.content_offset_top();
        }
        nv.natural_width = self_size.render_width.value - new_pos.width + nv.natural_width;
        fmt_ctx.add_float(self, el, nv.min_width, self_size.context_idx);
        if let Some(st) = st {
            self.fix_line_width(cx, this, st, float, self_size, fmt_ctx);
        }

        nv
    }

    /// `render_item_block::_render(x, y, containing_block_size, fmt_ctx, second_pass)`: the containing block (with
    /// `aspect-ratio`'s height as a minimum), the content laid out, the width (`min-content`, `max-content`,
    /// `fit-content` and the min and max laying the content out again), the height (`rows` of a `<textarea>`,
    /// the floats', min and max, centred content), the place, a list image's height; the widths it reports.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_block_render(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        x: Px,
        y: Px,
        containing_block_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
        second_pass: bool,
    ) -> RenderedWidth {
        let mut self_size = self.calculate_containing_block_context(cx, this, containing_block_size);
        let aspect_ratio = self.ri_css(this).m_aspect_ratio;
        if aspect_ratio > 0.0 && self_size.height.ty == CbcValueType::auto && (containing_block_size.size_mode & size_mode_content) == 0 {
            // From the width it gets: max-width and min-width apply first.
            let mut ratio_width = self_size.render_width.px();
            if self_size.max_width.ty != CbcValueType::none {
                ratio_width = ratio_width.min(self_size.max_width.px());
            }
            if self_size.min_width.ty != CbcValueType::none {
                ratio_width = ratio_width.max(self_size.min_width.px());
            }
            let ratio_height = Px(crate::css_length::clamp_length(
                ((ratio_width + self.box_sizing_width(this)) / Px(aspect_ratio) - self.box_sizing_height(this)).value(),
            ));
            if self_size.min_height.ty == CbcValueType::none || self_size.min_height.value < ratio_height {
                self_size.min_height = TypedPixel::new(ratio_height, CbcValueType::absolute);
            }
        }

        //*****************************************
        // Render content
        //*****************************************
        let content = self.render_content(cx, this, second_pass, &self_size, fmt_ctx);
        let (mut ret_width, mut ret_min_width) = (content.natural_width, content.min_width);
        //*****************************************
        let element = self.render.item(this).element;
        let contain = self.ri_css(this).m_contain;
        let size_contained = (contain & contain_size) != 0;
        if size_contained || (contain & contain_inline_size) != 0 {
            ret_width = Px(0.0);
            ret_min_width = Px(0.0);
        }

        if self.ri_css(this).m_display == display_list_item {
            let line_height = self.ri_css(this).m_line_height.computed_value;
            let it = self.render.item_mut(this);
            if it.pos.height == Px(0.0) {
                it.pos.height = line_height;
            }
        }

        let mut requires_rerender = false; // when true, the second pass for content rendering is required

        // Set block width
        if (containing_block_size.size_mode & size_mode_content) == 0 {
            if self_size.width.ty == CbcValueType::absolute {
                self.render.item_mut(this).pos.width = self_size.render_width.px();
                ret_width = self_size.render_width.px();
            } else {
                self.render.item_mut(this).pos.width = self_size.render_width.px();
                // width: min-content, max-content or fit-content
                let width = &self.ri_css(this).m_css_width;
                if width.is_predefined() && width.predef() != 0 && !self.render_is_root(this) {
                    let content = if width.predef() == 1 {
                        ret_min_width
                    } else if width.predef() == 2 {
                        ret_width
                    } else {
                        ret_width.min(self_size.render_width.px())
                    };
                    let it = self.render.item_mut(this);
                    if content != it.pos.width {
                        it.pos.width = content;
                        requires_rerender = true;
                    }
                }
            }
        } else {
            self.render.item_mut(this).pos.width = ret_width;
            if self_size.width.ty == CbcValueType::absolute && ret_width > self_size.width.px() {
                ret_width = self_size.width.px();
            }
        }

        let it = self.render.item_mut(this);
        // Fix width with max-width attribute
        if self_size.max_width.ty != CbcValueType::none && it.pos.width > self_size.max_width.px() {
            it.pos.width = self_size.max_width.px();
            requires_rerender = true;
        }

        // Fix width with min-width attribute
        if self_size.min_width.ty != CbcValueType::none {
            if it.pos.width < self_size.min_width.px() {
                it.pos.width = self_size.min_width.px();
                requires_rerender = true;
            }
        } else if it.pos.width < Px(0.0) {
            it.pos.width = Px(0.0);
        }

        // re-render content with new width if required
        if requires_rerender && !second_pass && !self.render_is_root(this) {
            if self.is_block_formatting_context(element) {
                fmt_ctx.clear_floats(-1);
            } else {
                fmt_ctx.clear_floats(self_size.context_idx);
            }

            let width = self.render.item(this).pos.width + (self_size.width.px() - self_size.render_width.px());
            self.render_content(cx, this, true, &self_size.new_width(width, size_mode_normal), fmt_ctx);
        }

        let content_height = self.render.item(this).pos.height;
        if size_contained {
            self.render.item_mut(this).pos.height = Px(0.0);
        }

        if self.tag(element) == crate::string_id::_textarea_ && self_size.height.ty == CbcValueType::auto && !size_contained {
            let attribute = self.get_attr(element, b"rows").unwrap_or(b"2");
            let (value, end) = twidget_cstd::strtol(crate::gumbo::c_str(attribute));
            let rows = if end == 0 || value <= 0 || value > i64::from(i32::MAX) { 2 } else { value };
            let line_height = self.ri_css(this).m_line_height.computed_value;
            self.render.item_mut(this).pos.height = line_height * Px(rows as f32);
        }

        // Set block height
        if self_size.height.ty != CbcValueType::auto && (containing_block_size.size_mode & size_mode_content) == 0 {
            let bsh = self.box_sizing_height(this);
            let it = self.render.item_mut(this);
            // TODO: Something wrong here
            // Percentage height from undefined containing block height is usually <= 0
            if self_size.height.ty == CbcValueType::percentage {
                if self_size.height.value > Px(0.0) {
                    it.pos.height = self_size.height.px();
                }
            } else {
                it.pos.height = self_size.height.px();
            }
            it.pos.height -= bsh;
        } else if self.is_block_formatting_context(element) && !size_contained {
            // add the floats' height to the block height
            let floats_height = fmt_ctx.get_floats_height(float_none);
            let it = self.render.item_mut(this);
            it.pos.height = floats_height.max(it.pos.height);
        }
        let it = self.render.item_mut(this);
        if (containing_block_size.size_mode & size_mode_content) != 0 && self_size.height.ty == CbcValueType::absolute {
            it.pos.height = it.pos.height.min(self_size.height.px());
        }

        // Fix height with min-height attribute
        if self_size.min_height.ty != CbcValueType::none {
            it.pos.height = it.pos.height.max(self_size.min_height.px());
        } else if it.pos.height < Px(0.0) {
            it.pos.height = Px(0.0);
        }

        // Fix width with max-width attribute
        if self_size.max_height.ty != CbcValueType::none {
            it.pos.height = it.pos.height.min(self_size.max_height.px());
        }

        let css = self.ri_css(this);
        let d = css.m_display;
        if css.m_center_content
            && !(d == display_flex || d == display_inline_flex || d == display_grid || d == display_inline_grid)
            && self.render.item(this).pos.height > content_height + Px(0.5)
        {
            let shift = (self.render.item(this).pos.height - content_height) / Px(2.0);
            self.shift_content(this, shift);
        }

        // calculate the final position
        let it = self.render.item_mut(this);
        it.pos.move_to(x, y);
        it.pos.x += it.content_offset_left();
        it.pos.y += it.content_offset_top();

        let css = self.ri_css(this);
        if css.m_display == display_list_item {
            let list_image = css.m_list_style_image.clone();
            if !list_image.is_empty() {
                let list_image_baseurl = css.m_list_style_image_baseurl.clone();
                let sz = cx.container.get_image_size(&list_image, Some(&list_image_baseurl));
                let it = self.render.item_mut(this);
                it.pos.height = it.pos.height.max(sz.height);
            }
        }

        let it = self.render.item(this);
        let css = self.ri_css(this);
        let mut ret = RenderedWidth { natural_width: ret_width + it.content_offset_width(), min_width: Px(0.0) };
        if !(css.m_overflow == overflow_visible || css.m_overflow == overflow_clip)
            && css.m_css_width.is_predefined()
            && css.m_display != display_table_cell
        {
            ret.min_width = it.content_offset_left() + it.content_offset_right();
        } else if css.m_css_width.is_predefined() {
            ret.min_width = ret_min_width + it.content_offset_width();
        } else {
            ret.min_width = it.width();
        }

        ret
    }

    /// `_render_content(x, y, second_pass, self_size, fmt_ctx)` of the block's class (which reads neither `x` nor
    /// `y`): a block context's boxes, an inline context's lines; a flex's (L10) and a grid's (L11) are not ported
    /// yet; a plain block lays out nothing.
    pub(crate) fn render_content(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        second_pass: bool,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) -> RenderedWidth {
        match self.render.item(this).kind {
            RenderKind::render_item_block_context => self.block_context_render_content(cx, this, second_pass, self_size, fmt_ctx),
            RenderKind::render_item_inline_context => self.inline_context_render_content(cx, this, self_size, fmt_ctx),
            RenderKind::render_item_flex => {
                self.unported("flex");
                RenderedWidth::default()
            }
            RenderKind::render_item_grid => {
                self.unported("grid");
                RenderedWidth::default()
            }
            _ => RenderedWidth::default(),
        }
    }

    /// `shift_content(delta)` of the block's class: an inline context's lines (and the bounds of its cut texts), any
    /// other block's children.
    pub(crate) fn shift_content(&mut self, this: RenderId, delta: Px) {
        if self.render.item(this).kind == RenderKind::render_item_inline_context {
            let mut st = self.take_inline_state(this);
            for lb in st.line_boxes.iter_mut() {
                self.line_box_y_shift(lb, delta);
            }
            for &text in &st.fragmented_texts {
                self.update_fragment_bounds(text);
            }
            self.put_inline_state(this, st);
            return;
        }
        for k in 0..self.render.children(this).len() {
            let el = self.render.children(this)[k];
            self.y_shift(el, delta);
        }
    }
}
