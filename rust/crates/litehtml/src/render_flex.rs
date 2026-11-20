//! `render_flex.h` and `render_flex.cpp`: `render_item_flex::init()` (a `render_item_grid`'s too), which makes
//! every child a flex item: block boxes as they are, a run of text in an anonymous block (leading collapsible spaces
//! dropped, trailing spaces cut), any other child in an anonymous block of its own; and the flex layout (stage L10):
//! `_render_content` splits the items into lines (`get_lines`: absolute and fixed children given their in-flow pass,
//! the items made and sorted by `order`, wrapped at the container's main size), resolves the flexible lengths and
//! the cross sizes line by line, aligns the lines (`align-content`, `flex-wrap: wrap-reverse`) and the items in them
//! (`justify-content`, `align-items`); the container's baselines come from its first line.

use crate::Px;
use crate::css_values::*;
use crate::document::{Document, StyleCx};
use crate::element::ElementId;
use crate::flex_item::FlexItem;
use crate::flex_line::FlexLine;
use crate::formatting_context::FormattingContext;
use crate::render_item::*;
use crate::types::*;

const flex_direction_row: i32 = FlexDirection::flex_direction_row.index();
const flex_direction_row_reverse: i32 = FlexDirection::flex_direction_row_reverse.index();
const flex_direction_column: i32 = FlexDirection::flex_direction_column.index();
const flex_direction_column_reverse: i32 = FlexDirection::flex_direction_column_reverse.index();
const flex_wrap_nowrap: i32 = FlexWrap::flex_wrap_nowrap.index();
const flex_wrap_wrap_reverse: i32 = FlexWrap::flex_wrap_wrap_reverse.index();
const flex_align_content_flex_start: i32 = FlexAlignContent::flex_align_content_flex_start.index();
const flex_align_content_flex_end: i32 = FlexAlignContent::flex_align_content_flex_end.index();
const flex_align_content_end: i32 = FlexAlignContent::flex_align_content_end.index();
const flex_align_content_center: i32 = FlexAlignContent::flex_align_content_center.index();
const flex_align_content_space_between: i32 = FlexAlignContent::flex_align_content_space_between.index();
const flex_align_content_space_around: i32 = FlexAlignContent::flex_align_content_space_around.index();
const flex_align_content_stretch: i32 = FlexAlignContent::flex_align_content_stretch.index();
const flex_justify_content_start: i32 = FlexJustifyContent::flex_justify_content_start.index();
const flex_justify_content_left: i32 = FlexJustifyContent::flex_justify_content_left.index();
const flex_justify_content_right: i32 = FlexJustifyContent::flex_justify_content_right.index();

impl Document {
    /// `render_item_flex::init()`.
    pub(crate) fn render_flex_init(&mut self, cx: &StyleCx, this: RenderId) -> RenderId {
        let element = self.render.item(this).element;
        self.add_render(element, this);
        let mut new_children = Vec::new();
        let mut inlines = Vec::new();

        for el in self.render.children(this).to_vec() {
            let el_element = self.render.item(el).element;
            if self.el(el_element).css.m_display == display_inline_text {
                // Text after text, or text that is not collapsible space
                if !inlines.is_empty() || !self.is_white_space(el_element) {
                    inlines.push(el);
                }
            } else {
                self.convert_inlines(cx, this, element, &mut inlines, &mut new_children);
                if self.is_block_box(el_element) {
                    // Add block boxes as is
                    self.render.set_parent(el, Some(this));
                    let made = self.render_init(cx, el);
                    new_children.push(made);
                } else {
                    // Wrap inlines with anonymous block box
                    let made = self.wrapped_flex_item(cx, this, el, el_element);
                    new_children.push(made);
                }
            }
        }
        self.convert_inlines(cx, this, element, &mut inlines, &mut new_children);
        self.set_render_children(this, new_children);

        this
    }

    /// `convert_inlines`: the run of text so far, without its trailing spaces, as an anonymous block's children,
    /// that block's `init()` a flex item.
    #[inline(never)]
    fn convert_inlines(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        element: ElementId,
        inlines: &mut Vec<RenderId>,
        new_children: &mut Vec<RenderId>,
    ) {
        if inlines.is_empty() {
            return;
        }
        // Find last not space
        if let Some(at) = inlines.iter().rposition(|&el| !self.is_space(self.render.item(el).element)) {
            // Erase all spaces at the end
            inlines.truncate(at + 1);
        }

        let anon_el = self.new_anonymous_tag(cx, element, b"display: block");
        let anon_ri = self.new_render_item(cx, RenderKind::render_item_block, anon_el);
        for &inl in inlines.iter() {
            self.add_render_child(anon_ri, inl);
        }
        self.render.set_parent(anon_ri, Some(this));

        let made = self.render_init(cx, anon_ri);
        new_children.push(made);
        if made != anon_ri {
            self.discard_render(anon_ri);
        }
        inlines.clear();
    }

    /// A child that is neither text nor a block box: an anonymous block of the child's element around what its
    /// `init()` makes it, and that block's `init()`.
    #[inline(never)]
    fn wrapped_flex_item(&mut self, cx: &StyleCx, this: RenderId, el: RenderId, el_element: ElementId) -> RenderId {
        let anon_el = self.new_anonymous_tag(cx, el_element, b"display: block");
        let anon_ri = self.new_render_item(cx, RenderKind::render_item_block, anon_el);
        let inner = self.render_init(cx, el);
        self.add_render_child(anon_ri, inner);
        self.render.set_parent(anon_ri, Some(this));
        let made = self.render_init(cx, anon_ri);
        if made != anon_ri {
            self.discard_render(anon_ri);
        }
        made
    }
}

/// The flex container's gap on an axis: a % only against the width (and only on the axis the width runs along),
/// any other length's number as it is.
fn gap(length: &crate::css_length::CssLength, along_width: bool, render_width: Px) -> Px {
    if length.units() == CssUnits::css_units_percentage {
        if along_width { length.calc_percent(render_width) } else { Px(0.0) }
    } else {
        Px(length.val())
    }
}

/// `get_lines`' second half: the items put on lines in order, a new line when the next item would take the line past
/// the container's main size (unless single-line), each line adding up its items' base and hypothetical main sizes
/// with the gaps and counting their main `auto` margins.
#[inline(never)]
fn split_lines(
    items: Vec<FlexItem>,
    reverse_main: bool,
    reverse_cross: bool,
    main_gap: Px,
    container_main_size: Px,
    single_line: bool,
) -> Vec<FlexLine> {
    let mut lines = Vec::new();
    let mut line = FlexLine::new(reverse_main, reverse_cross);
    line.main_gap = main_gap;
    // Add flex items to lines
    for item in items {
        if !line.items.is_empty() && !single_line && line.main_size + main_gap + item.main_size > container_main_size {
            lines.push(line);
            line = FlexLine::new(reverse_main, reverse_cross);
            line.main_gap = main_gap;
        }
        if !line.items.is_empty() {
            line.base_size += main_gap;
            line.main_size += main_gap;
        }
        line.base_size += item.base_size;
        line.main_size += item.main_size;
        if !item.auto_margin_main_start.is_default() {
            line.num_auto_margin_main_start += 1;
        }
        if !item.auto_margin_main_end.is_default() {
            line.num_auto_margin_main_end += 1;
        }
        line.items.push(item);
    }
    // Add the last line to the lines list
    if !line.items.is_empty() {
        lines.push(line);
    }
    lines
}

impl Document {
    /// `render_item_flex::_render_content(x, y, second_pass, self_size, fmt_ctx)`: the main size (a column's the
    /// height, min and max height; an `auto` height lays a column out on one line that fits its content), the
    /// lines, each line's flexible lengths and cross size (a single column line no wider than the container unless
    /// measuring), the free cross size (`align-content: stretch` growing the lines), the lines placed, the items
    /// placed in them; the box as high as its lowest item. Gives the lines' widths added up (a row's hypothetical
    /// main sizes, a column's cross sizes) as both widths.
    pub(crate) fn flex_render_content(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        x: Px,
        y: Px,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) -> RenderedWidth {
        let css = self.ri_css(this);
        let (is_row_direction, reverse) = match css.m_flex_direction {
            flex_direction_column => (false, false),
            flex_direction_column_reverse => (false, true),
            flex_direction_row_reverse => (true, true),
            _ => (true, false),
        };
        let flex_wrap = css.m_flex_wrap;
        let mut container_main_size = self_size.render_width.px();

        let mut single_line = flex_wrap == flex_wrap_nowrap;
        let mut fit_container = false;

        if !is_row_direction {
            if self_size.height.ty != CbcValueType::auto {
                container_main_size = self_size.height.value - self.box_sizing_height(this);
            } else {
                // Direction columns, height is auto - always in single line
                container_main_size = Px(0.0);
                single_line = true;
                fit_container = true;
            }
            if self_size.min_height.ty != CbcValueType::auto && self_size.min_height.value > container_main_size {
                container_main_size = self_size.min_height.px();
            }
            if self_size.max_height.ty != CbcValueType::auto && self_size.max_height.ty != CbcValueType::none {
                if self_size.height.ty != CbcValueType::auto {
                    if self_size.max_height.value < container_main_size {
                        container_main_size = self_size.max_height.px();
                    }
                } else if flex_wrap != flex_wrap_nowrap {
                    container_main_size = self_size.max_height.px();
                    single_line = false;
                }
            }
        }

        // Split flex items to lines; the new lines own their items, the last layout's let theirs go
        let mut lines = self.flex_get_lines(cx, this, self_size, fmt_ctx, is_row_direction, container_main_size, single_line);
        self.flex_set_lines(this, &lines);

        let ret_width = self.flex_lay_out_lines(
            cx,
            this,
            &mut lines,
            self_size,
            fmt_ctx,
            container_main_size,
            fit_container,
            is_row_direction,
            reverse,
        );

        let it = self.render.item_mut(this);
        // calculate the final position
        it.pos.move_to(x, y);
        it.pos.x += it.content_offset_left();
        it.pos.y += it.content_offset_top();
        it.flex.get_or_insert_with(Default::default).lines = lines;

        RenderedWidth::new(ret_width, ret_width)
    }

    /// `m_lines = lines`: the new lines' items held, the last lines' let go.
    #[inline(never)]
    fn flex_set_lines(&mut self, this: RenderId, lines: &[FlexLine]) {
        for line in lines {
            for item in &line.items {
                self.render.hold(item.el);
            }
        }
        let old = std::mem::take(&mut self.render.item_mut(this).flex.get_or_insert_with(Default::default).lines);
        for line in old {
            for item in line.items {
                self.release_render(item.el);
            }
        }
    }

    /// `_render_content`'s work on the lines: their flexible lengths and cross sizes, then their places and their
    /// items'. Kept out of the caller's frame, which every level of nested flex boxes keeps on the stack.
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn flex_lay_out_lines(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        lines: &mut [FlexLine],
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
        container_main_size: Px,
        fit_container: bool,
        is_row_direction: bool,
        reverse: bool,
    ) -> Px {
        let mut container_main_size = container_main_size;
        let mut sum_cross_size = Px(0.0);
        let mut sum_main_size = Px(0.0);
        let mut ret_width = Px(0.0);

        let css = self.ri_css(this);
        let cross_gap =
            gap(if is_row_direction { &css.m_row_gap } else { &css.m_column_gap }, !is_row_direction, self_size.render_width.px());
        let (flex_wrap, align_content_css, justify_content_css) = (css.m_flex_wrap, css.m_flex_align_content, css.m_flex_justify_content);
        if lines.len() > 1 {
            sum_cross_size = cross_gap * Px::from_int((lines.len() - 1) as i32);
        }

        // Resolving Flexible Lengths
        // REF: https://www.w3.org/TR/css-flexbox-1/#resolve-flexible-lengths
        let single = lines.len() == 1;
        for ln in lines.iter_mut() {
            if is_row_direction {
                ret_width += ln.main_size;
            }
            self.flex_line_init(cx, ln, container_main_size, fit_container, is_row_direction, self_size, fmt_ctx);
            if !is_row_direction && single && (self_size.size_mode & size_mode_content) == 0 && ln.cross_size > self_size.render_width.px()
            {
                ln.cross_size = Px(0.0).max(self_size.render_width.px());
            }
            sum_cross_size += ln.cross_size;
            sum_main_size = sum_main_size.max(ln.main_size);
            if reverse {
                ln.items.reverse();
            }
        }

        let mut free_cross_size = Px(0.0);
        let is_wrap_reverse = flex_wrap == flex_wrap_wrap_reverse;
        if container_main_size == Px(0.0) {
            container_main_size = sum_main_size;
        }

        // Calculate free cross size
        if is_row_direction {
            if self_size.height.ty != CbcValueType::auto {
                let height = self_size.height.value - self.box_sizing_height(this);
                free_cross_size = height - sum_cross_size;
            } else if self_size.min_height.ty != CbcValueType::none && self_size.min_height.ty != CbcValueType::percentage {
                free_cross_size = Px(0.0).max(self_size.min_height.value - sum_cross_size);
            }
        } else {
            free_cross_size = self_size.render_width.value - sum_cross_size;
            ret_width = sum_cross_size;
        }

        // Fix align-content property
        let mut align_content = align_content_css;
        if align_content == flex_align_content_space_between {
            // A negative leftover or a single line: flex-start.
            if lines.len() == 1 || free_cross_size < Px(0.0) {
                align_content = flex_align_content_flex_start;
            }
        }
        if align_content == flex_align_content_space_around {
            // A negative leftover or a single line: center.
            if lines.len() == 1 || free_cross_size < Px(0.0) {
                align_content = flex_align_content_center;
            }
        }

        // Distribute free cross size for align-content: stretch
        if align_content_css == flex_align_content_stretch && free_cross_size > Px(0.0) && !lines.is_empty() {
            let add = free_cross_size / Px::from_int(lines.len() as i32);
            for ln in lines.iter_mut() {
                ln.cross_size += add;
            }
            free_cross_size = Px(0.0);
        }

        // Reverse lines for flex-wrap: wrap-reverse
        if flex_wrap == flex_wrap_wrap_reverse {
            lines.reverse();
        }

        // Align flex lines
        let mut line_pos = Px(0.0);
        let mut add_before_line = Px(0.0);
        let mut add_after_line = Px(0.0);
        match align_content {
            flex_align_content_flex_start => {
                if is_wrap_reverse {
                    line_pos = free_cross_size;
                }
            }
            flex_align_content_flex_end => {
                if !is_wrap_reverse {
                    line_pos = free_cross_size;
                }
            }
            flex_align_content_end => line_pos = free_cross_size,
            flex_align_content_center => line_pos = free_cross_size / Px(2.0),
            flex_align_content_space_between => {
                add_after_line = free_cross_size / Px::from_int(lines.len().wrapping_sub(1) as i32);
            }
            flex_align_content_space_around => {
                add_before_line = free_cross_size / Px::from_int(lines.len().wrapping_mul(2) as i32);
                add_after_line = add_before_line;
            }
            _ => {
                if is_wrap_reverse {
                    line_pos = free_cross_size;
                }
            }
        }
        for ln in lines.iter_mut() {
            line_pos += add_before_line;
            ln.cross_start = line_pos;
            line_pos += ln.cross_size + add_after_line + cross_gap;
        }

        // Fix justify-content property
        let mut justify_content = justify_content_css;
        if (justify_content == flex_justify_content_right || justify_content == flex_justify_content_left) && !is_row_direction {
            justify_content = flex_justify_content_start;
        }

        // Align flex items in flex lines
        for ln in lines.iter_mut() {
            let height =
                self.flex_line_calculate_items_position(cx, ln, container_main_size, justify_content, is_row_direction, self_size, fmt_ctx);
            let it = self.render.item_mut(this);
            it.pos.height = it.pos.height.max(height);
        }

        ret_width
    }

    /// `render_item_flex::get_lines(self_size, fmt_ctx, is_row_direction, container_main_size, single_line)`: each
    /// absolute or fixed child given its in-flow pass (`render_positioned` takes its shrink-to-fit size from it),
    /// each other child made a flex item, sorted by `order` when any differs from the first's, then the items put on
    /// lines, a new line when the next would pass the container's main size (unless single-line).
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn flex_get_lines(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
        is_row_direction: bool,
        container_main_size: Px,
        single_line: bool,
    ) -> Vec<FlexLine> {
        let css = self.ri_css(this);
        let reverse_cross = css.m_flex_wrap == flex_wrap_wrap_reverse;
        let reverse_main = if is_row_direction {
            css.m_flex_direction == flex_direction_row_reverse
        } else {
            css.m_flex_direction == flex_direction_column_reverse
        };

        let main_gap =
            gap(if is_row_direction { &css.m_column_gap } else { &css.m_row_gap }, is_row_direction, self_size.render_width.px());

        let (mut items, sort_required) = self.flex_make_items(cx, this, self_size, fmt_ctx, is_row_direction);

        if sort_required {
            items.sort_by_key(|item| (item.order, item.src_order));
        }

        split_lines(items, reverse_main, reverse_cross, main_gap, container_main_size, single_line)
    }

    /// `get_lines`' walk over the children: each absolute or fixed child's in-flow pass, each other child's flex
    /// item, made in the list it goes to (on the heap, out of the frame every level of nested flex boxes keeps), and
    /// whether an `order` differs from the first item's.
    #[inline(never)]
    fn flex_make_items(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
        is_row_direction: bool,
    ) -> (Vec<FlexItem>, bool) {
        let align_items = self.ri_css(this).m_flex_align_items;
        let mut items: Vec<FlexItem> = Vec::new();
        let mut src_order = 0;
        let mut sort_required = false;
        let mut prev_order = DefValue::new(0);

        let mut k = 0;
        while k < self.render.children(this).len() {
            let el = self.render.children(this)[k];
            k += 1;
            let (position, display) = {
                let el_css = self.ri_css(el);
                (el_css.m_el_position, el_css.m_display)
            };
            if position == element_position_absolute || position == element_position_fixed {
                // Out of flow, but render_positioned() takes the shrink-to-fit size from this pass, as the
                // block context does; left unrendered the box stays 0 x 0.
                if display != display_none {
                    self.render_out_of_flow(cx, el, Px(0.0), self_size, fmt_ctx);
                }
                continue;
            }
            items.push(FlexItem::new(el, is_row_direction));
            let item = items.last_mut().expect("the item just made");
            self.flex_item_init(cx, item, self_size, fmt_ctx, align_items);
            item.src_order = src_order;
            src_order += 1;

            if prev_order.is_default() {
                prev_order.set(item.order);
            } else if !sort_required && item.order != prev_order.get() {
                sort_required = true;
            }
        }
        (items, sort_required)
    }

    /// `render_item_flex::get_first_baseline()`: a row's first line's first baseline (else its last) below the
    /// content's top, else the first item's first baseline, else the box's outer height.
    pub(crate) fn flex_first_baseline(&self, id: RenderId) -> Px {
        self.flex_baseline(id, true)
    }

    /// `render_item_flex::get_last_baseline()`: as the first, from the first line's last baseline (else its first)
    /// and the first item's last baseline.
    pub(crate) fn flex_last_baseline(&self, id: RenderId) -> Px {
        self.flex_baseline(id, false)
    }

    fn flex_baseline(&self, id: RenderId, first: bool) -> Px {
        let it = self.render.item(id);
        let lines: &[FlexLine] = it.flex.as_ref().map_or(&[], |f| &f.lines);
        let direction = self.ri_css(id).m_flex_direction;
        if (direction == flex_direction_row || direction == flex_direction_row_reverse)
            && let Some(first_line) = lines.first()
        {
            let (preferred, other) = if first {
                (&first_line.first_baseline, &first_line.last_baseline)
            } else {
                (&first_line.last_baseline, &first_line.first_baseline)
            };
            for baseline in [preferred, other] {
                if baseline.ty != BaselineType::baseline_type_none {
                    return first_line.cross_start + baseline.get_offset_from_top(first_line.cross_size) + it.content_offset_top();
                }
            }
        }
        if let Some(item) = lines.first().and_then(|line| line.items.first()) {
            let baseline = if first { self.get_first_baseline(item.el) } else { self.get_last_baseline(item.el) };
            return baseline + it.content_offset_top();
        }
        it.height()
    }
}
