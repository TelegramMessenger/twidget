//! `render_inline_context.h` and `render_inline_context.cpp` (stage L8), with `iterators.cpp`'s walk of the inline
//! items and `render_text`'s fragment bounds: an inline formatting context's content. Its inline items, inside the
//! inline elements it goes into, are placed on line boxes one after another: collapsible spaces after a space
//! skipped, inline boxes laid out first, a text too wide for the line cut by the container's text breaker into
//! pieces (`render_text` fragments, each measured), floats placed beside the lines (the last line made to fit), a
//! new line where an item does not fit or a break forces one; then the lines finished, `text-overflow: ellipsis`
//! applied, the block's margins collapsed through its lines and its height. Its baselines, `vertical-align` in a
//! table cell and the content measure of a box whose words may break anywhere (stages L10 and L12 ask them).

use crate::Px;
use crate::css_properties::CssProperties;
use crate::document::{Document, StyleCx};

use crate::formatting_context::{ElPosition, FormattingContext};
use crate::line_box::{LbiType, LineBox, LineBoxItem};
use crate::render_item::*;
use crate::types::*;

/// `iterator_item_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IterItem {
    child,
    start_parent,
    end_parent,
}

/// `css_properties::get_wrapping()`: `word-break: break-all` and `break-word` are `overflow-wrap: anywhere`.
pub fn get_wrapping(css: &CssProperties) -> i32 {
    let wb = css.m_word_break;
    if wb == crate::css_values::WordBreak::word_break_break_all.index() || wb == crate::css_values::WordBreak::word_break_break_word.index()
    {
        overflow_wrap_anywhere
    } else {
        css.m_overflow_wrap
    }
}

impl Document {
    /// The inline context's own state, taken out while its content is laid out.
    pub(crate) fn take_inline_state(&mut self, id: RenderId) -> Box<InlineState> {
        self.render.item_mut(id).inline.take().unwrap_or_default()
    }

    /// The inline context's own state, put back.
    pub(crate) fn put_inline_state(&mut self, id: RenderId, st: Box<InlineState>) {
        self.render.item_mut(id).inline = Some(st);
    }

    /// `render_text::update_fragment_bounds()`: a cut text's place, the box around its pieces.
    pub(crate) fn update_fragment_bounds(&mut self, text: RenderId) {
        let fragments = match &self.render.item(text).text {
            Some(t) if !t.fragments.is_empty() => t.fragments.clone(),
            _ => return,
        };
        let mut pos = self.render.item(fragments[0]).pos;
        for &fragment in &fragments {
            let b = self.render.item(fragment).pos;
            let right = pos.right().max(b.right());
            let bottom = pos.bottom().max(b.bottom());
            pos.x = pos.x.min(b.x);
            pos.y = pos.y.min(b.y);
            pos.width = right - pos.x;
            pos.height = bottom - pos.y;
        }
        self.render.item_mut(text).pos = pos;
    }

    /// `elements_iterator(true, go_inside_inline, inline_selector).process(container, …)`: the inline elements gone
    /// into (start and end) and the texts, inline boxes and floats inside them, in order.
    fn inline_events(&self, container: RenderId, out: &mut Vec<(RenderId, IterItem)>) {
        for &el in self.render.children(container) {
            let css = self.ri_css(el);
            let d = css.m_display;
            if d == display_inline && css.m_float == float_none {
                out.push((el, IterItem::start_parent));
                self.inline_events(el, out);
                out.push((el, IterItem::end_parent));
            } else if d == display_inline_text
                || d == display_inline_table
                || d == display_inline_block
                || d == display_inline_flex
                || d == display_inline_grid
                || css.m_float != float_none
            {
                out.push((el, IterItem::child));
            }
        }
    }

    /// `render_item_inline_context::_render_content(x, y, second_pass, self_size, fmt_ctx)`.
    pub(crate) fn inline_context_render_content(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) -> RenderedWidth {
        let mut st = self.take_inline_state(this);
        st.line_boxes.clear();
        st.fragmented_texts.clear();
        st.rendered_width.reset();
        st.va_applied = Px(0.0);

        let ws = self.ri_css(this).m_white_space;
        let skip_spaces = ws == white_space_normal || ws == white_space_nowrap || ws == white_space_pre_line;

        let mut was_space = false;

        let mut events = Vec::new();
        self.inline_events(this, &mut events);
        for index in 0..events.len() {
            let (el, item_type) = events[index];
            st.trailing = Px(0.0);
            if item_type == IterItem::child {
                self.trailing_of(cx, &events, index, self_size, &mut st);
            }
            match item_type {
                IterItem::child => {
                    // skip spaces to make rendering a bit faster
                    if skip_spaces {
                        let src = self.render.item(el).element;
                        if self.is_white_space(src) {
                            if was_space {
                                self.render.item_mut(el).skip = true;
                                continue;
                            }
                            was_space = true;
                        } else {
                            // skip all spaces after line break
                            was_space = self.is_break(src);
                        }
                    }
                    // place element into rendering flow
                    self.place_inline(cx, this, &mut st, LineBoxItem::text(el), self_size, fmt_ctx);
                }
                IterItem::start_parent => {
                    self.clear_inline_boxes(el);
                    self.render.item_mut(el).pos = Position::default();
                    let item = self.lbi_start(el);
                    self.place_inline(cx, this, &mut st, item, self_size, fmt_ctx);
                }
                IterItem::end_parent => {
                    let item = self.lbi_end(el);
                    self.place_inline(cx, this, &mut st, item, self_size, fmt_ctx);
                }
            }
        }
        st.trailing = Px(0.0);

        self.finish_last_box(this, &mut st, true, self_size);
        for k in 0..st.fragmented_texts.len() {
            let text = st.fragmented_texts[k];
            self.update_fragment_bounds(text);
            if get_wrapping(self.ri_css(text)) != overflow_wrap_anywhere || self_size.render_width.px() > Px(0.0) {
                let unbroken = self.get_content_size(cx, self.render.item(text).element, self_size.render_width.px());
                st.rendered_width.natural_width = st.rendered_width.natural_width.max(unbroken.width);
            }
        }

        self.apply_ellipsis(cx, this, &mut st, self_size.render_width.px());

        if !st.line_boxes.is_empty() {
            if self.collapse_top_margin(this) {
                let old_top = self.render.item(this).margins.top;
                let top = st.line_boxes[0].top_margin().max(old_top);
                self.render.item_mut(this).margins.top = top;
                if top != old_top {
                    fmt_ctx.update_floats(self, top - old_top, this);
                }
            }
            let collapse = self.collapse_bottom_margin(this);
            let back = st.line_boxes.last().expect("a line");
            let it = self.render.item_mut(this);
            if collapse {
                it.margins.bottom = back.bottom_margin().max(it.margins.bottom);
                it.pos.height = back.bottom() - back.bottom_margin();
            } else {
                it.pos.height = back.bottom();
            }
        }

        let ret = st.rendered_width;
        self.put_inline_state(this, st);
        ret
    }

    /// The room a text item leaves after it for what may not start a line: the ends of the inline elements it closes,
    /// and a following text that starts with closing punctuation (with the ends after that).
    #[inline(never)]
    fn trailing_of(
        &self,
        cx: &StyleCx,
        events: &[(RenderId, IterItem)],
        index: usize,
        self_size: &ContainingBlockContext,
        st: &mut InlineState,
    ) {
        let el = events[index].0;
        let mut next = index + 1;
        while next < events.len() && events[next].1 == IterItem::end_parent {
            st.trailing += self.lbi_width(&self.lbi_end(events[next].0));
            next += 1;
        }
        let src = self.render.item(el).element;
        if next > index + 1
            && next < events.len()
            && events[next].1 == IterItem::child
            && self.is_text(src)
            && !self.is_white_space(src)
            && !self.is_break(src)
        {
            let next_src = self.render.item(events[next].0).element;
            if self.is_text(next_src) {
                let shown = self.display_text(next_src);
                if let Some(&first) = shown.first()
                    && b",.;:!?%)]}".contains(&first)
                {
                    let measured = self.get_content_size(cx, next_src, self_size.render_width.px());
                    st.trailing += measured.width;
                    next += 1;
                    while next < events.len() && events[next].1 == IterItem::end_parent {
                        st.trailing += self.lbi_width(&self.lbi_end(events[next].0));
                        next += 1;
                    }
                }
            }
        }
    }

    /// `apply_ellipsis(room)`: with `text-overflow: ellipsis` on a box that clips (left to right), each line too long
    /// for the room cut at the last whole character that leaves room for "…" (searched by halves), what follows not
    /// drawn, and the mark placed.
    fn apply_ellipsis(&mut self, cx: &StyleCx, this: RenderId, st: &mut InlineState, room: Px) {
        let element = self.render.item(this).element;
        let mode = if self.kind(element).is_html_tag() { self.raw_declaration(cx.names, element, b"text-overflow") } else { Vec::new() };
        let css = self.ri_css(this);
        let clips = css.m_overflow > overflow_visible || (css.m_contain & contain_paint) != 0;
        let active = !mode.is_empty()
            && crate::html::lowcase(&crate::html::trim(&mode, b" \n\r\t\x0c")).as_slice() == b"ellipsis"
            && clips
            && css.m_direction != direction_rtl;
        // Most boxes have no ellipsis and never had one: their lines are left alone.
        if !active && !st.cut {
            return;
        }
        st.cut = active;
        st.ellipses.clear();
        let font = self.ri_css(this).m_font;
        let mark = if active { cx.container.text_width("\u{2026}".as_bytes(), font) } else { Px(0.0) };
        for line in &st.line_boxes {
            let mut right = Px(0.0);
            for item in &line.items {
                if let Some(text) = self.render.item_mut(item.element).text.as_mut() {
                    text.cut = -1;
                }
                right = right.max(self.lbi_right(item));
            }
            if !active || right <= room {
                continue;
            }
            // Whole characters up to the room the mark leaves, as browsers cut; what follows is not drawn.
            let limit = room - mark;
            let mut done = false;
            let mut at = Position::new(line.left, line.top, Px(0.0), line.height);
            for item in &line.items {
                let is_text = self.render.item(item.element).kind == RenderKind::render_text;
                if done {
                    if is_text {
                        self.render.item_mut(item.element).text.as_mut().expect("a text").cut = 0;
                    }
                    continue;
                }
                if self.lbi_right(item) <= limit {
                    at.x = at.x.max(self.lbi_right(item));
                    continue;
                }
                done = true;
                let src = self.render.item(item.element).element;
                if !is_text || self.parent(src).is_none() {
                    continue;
                }
                let is_fragment = self.render.item(item.element).text.as_ref().expect("a text").is_fragment;
                let source = self.is_text(src);
                if !is_fragment && !source {
                    continue;
                }
                let shown = if is_fragment {
                    self.render.item(item.element).text.as_ref().expect("a text").fragment_text.clone()
                } else {
                    self.display_text(src).to_vec()
                };
                let parent = self.parent(src).expect("a parent");
                let font = self.el(parent).css.m_font;
                let mut fit = 0usize;
                let mut used = Px(0.0);
                // A prefix is no narrower than a shorter one: the cut is searched, not walked a character at a time.
                let mut ends = Vec::new();
                let mut i = 0;
                while i < shown.len() {
                    let mut next = i + 1;
                    while next < shown.len() && (shown[next] & 0xC0) == 0x80 {
                        next += 1;
                    }
                    ends.push(next);
                    i = next;
                }
                let (mut low, mut high) = (0usize, ends.len());
                let left = self.lbi_left(item);
                while low < high {
                    let middle = low + (high - low).div_ceil(2);
                    let width = cx.container.text_width(crate::gumbo::c_str(&shown[..ends[middle - 1]]), font);
                    if left + width > limit {
                        high = middle - 1;
                    } else {
                        low = middle;
                        fit = ends[middle - 1];
                        used = width;
                    }
                }
                self.render.item_mut(item.element).text.as_mut().expect("a text").cut = fit as i32;
                let pos = self.render.item(item.element).pos;
                at.x = left + used;
                at.y = pos.y;
                at.height = pos.height;
            }
            at.width = mark;
            st.ellipses.push(at);
        }
    }

    /// `fix_line_width(flt, self_size, fmt_ctx)`: after a float, the last line's items placed again around it, or,
    /// when its first text clears that float's side, the line given its new sides and what no longer fits placed on.
    pub(crate) fn fix_line_width(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        st: &mut InlineState,
        flt: i32,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) {
        let Some(back) = st.line_boxes.last() else { return };
        let el_front = self.line_box_first_text_part(back);

        let mut was_cleared = false;
        if let Some(front) = el_front {
            let clear = self.ri_css(front).m_clear;
            if clear != clear_none
                && (clear == clear_both || (flt == float_left && clear == clear_left) || (flt == float_right && clear == clear_right))
            {
                was_cleared = true;
            }
        }

        if !was_cleared {
            let items = std::mem::take(&mut st.line_boxes.last_mut().expect("a line").items);
            st.line_boxes.pop();

            for item in items {
                // The line before hands the next box its open inlines again: placed anew, these would open them twice.
                if item.ty == LbiType::type_inline_continue {
                    continue;
                }
                self.place_inline(cx, this, st, item, self_size, fmt_ctx);
            }
        } else {
            let line_top = back.top;

            let (mut line_left, line_right) = fmt_ctx.get_line_left_right(line_top, self_size.render_width.px());

            if st.line_boxes.len() == 1 {
                let css = self.ri_css(this);
                if css.m_list_style_type != list_style_type_none && css.m_list_style_position == list_style_position_inside {
                    let sz_font = css.get_font_size();
                    line_left += sz_font;
                }

                if Px(css.m_css_text_indent.val()) != Px(0.0) {
                    line_left += css.m_css_text_indent.calc_percent(self_size.width.px());
                }
            }

            let mut back = st.line_boxes.pop().expect("a line");
            let items = self.line_box_new_width(&mut back, line_left, line_right);
            st.line_boxes.push(back);
            for item in items {
                self.place_inline(cx, this, st, item, self_size, fmt_ctx);
            }
        }
    }

    /// `finish_last_box(end_of_render, self_size)`: the last line finished (dropped when it ends the content empty),
    /// its widths added (the first line's with its inside marker and indent); what the next line starts with.
    fn finish_last_box(
        &mut self,
        this: RenderId,
        st: &mut InlineState,
        end_of_render: bool,
        self_size: &ContainingBlockContext,
    ) -> Vec<LineBoxItem> {
        let mut ret = Vec::new();

        if let Some(mut back) = st.line_boxes.pop() {
            ret = self.line_box_finish(&mut back, end_of_render, self_size);

            if self.line_box_is_empty(&back) && end_of_render {
                // remove the last empty line
                return ret;
            }
            st.line_boxes.push(back);
            let mut line_width = st.line_boxes.last().expect("a line").rendered_width;
            // The first line starts after an inside list marker and the text indent, which its items
            // do not count; without them a max-content width is short and the last word wraps.
            if st.line_boxes.len() == 1 {
                let css = self.ri_css(this);
                if css.m_list_style_type != list_style_type_none && css.m_list_style_position == list_style_position_inside {
                    line_width.natural_width += css.get_font_size();
                }
                if Px(css.m_css_text_indent.val()) != Px(0.0) {
                    line_width.natural_width += css.m_css_text_indent.calc_percent(self_size.width.px());
                }
            }
            st.rendered_width.merge(&line_width);
        }
        ret
    }

    /// `new_box(el, self_size, fmt_ctx)`: the last line finished and a new one below it (and below what `el`
    /// clears), beside the floats, after an inside marker and the indent when it is the first; what the last line
    /// handed on goes first. Gives the new line's top.
    fn new_box(
        &mut self,
        this: RenderId,
        st: &mut InlineState,
        el: &LineBoxItem,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) -> Px {
        let items = self.finish_last_box(this, st, false, self_size);
        let mut line_top = Px(0.0);
        if let Some(back) = st.line_boxes.last() {
            line_top = back.bottom();
        }
        line_top = fmt_ctx.get_cleared_top(self, el.element, line_top);

        let mut first_line_margin = Px(0.0);
        let mut text_indent = Px(0.0);
        let css = self.ri_css(this);
        if st.line_boxes.is_empty() {
            if css.m_list_style_type != list_style_type_none && css.m_list_style_position == list_style_position_inside {
                first_line_margin = css.get_font_size();
            }
            if Px(css.m_css_text_indent.val()) != Px(0.0) {
                text_indent = css.m_css_text_indent.calc_percent(self_size.width.px());
            }
        }

        let mut el_pos = ElPosition { container_width: self_size.render_width.px(), el_pos: self.lbi_pos(el), ..ElPosition::default() };
        el_pos.el_pos.y = line_top;
        el_pos.el_pos.x = Px(0.0);
        el_pos.el_pos.width += text_indent + first_line_margin;
        let new_pos = fmt_ctx.place_to_left(&el_pos);

        let css = self.ri_css(this);
        let line = LineBox::new(
            new_pos.top,
            new_pos.left + first_line_margin + text_indent,
            new_pos.left + new_pos.width,
            &css.m_line_height,
            &css.m_font_metrics,
            css.m_text_align,
            css.m_direction == direction_rtl,
        );
        st.line_boxes.push(line);

        // Add items returned by finish_last_box function into the new line
        let mut back = st.line_boxes.pop().expect("a line");
        for it in items {
            self.line_box_add_item(&mut back, it);
        }
        st.line_boxes.push(back);

        new_pos.top
    }

    /// `place_inline(item, self_size, fmt_ctx)`: the item into the flow: a float beside the lines, an inline box
    /// laid out (shrunk to its natural width when `auto`), a text measured and, when its words may break and it does
    /// not fit, cut into pieces line by line; then on the last line, or on a new one when it does not fit there, a
    /// block-level item with its top margin collapsed.
    pub(crate) fn place_inline(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        st: &mut InlineState,
        mut item: LineBoxItem,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) {
        let el = item.element;
        let src = self.render.item(el).element;
        if self.el(src).css.m_display == display_none {
            return;
        }

        if self.el(src).css.m_float != float_none {
            let mut line_top = Px(0.0);
            if let Some(back) = st.line_boxes.last() {
                line_top = back.top;
            }
            let rw = self.place_float(cx, this, Some(st), el, line_top, self_size, fmt_ctx);
            st.rendered_width.merge(&rw);
            return;
        }

        if item.ty == LbiType::type_text_part {
            if self.is_inline_box(src) {
                let min_rendered_width = self.render(cx, el, Px(0.0), Px(0.0), self_size, Some(fmt_ctx), false).natural_width;
                if min_rendered_width < self.render.item(el).width() && self.el(src).css.m_css_width.is_predefined() {
                    self.render(cx, el, Px(0.0), Px(0.0), &self_size.new_width(min_rendered_width, size_mode_normal), Some(fmt_ctx), false);
                }
                item.rendered_min_width = min_rendered_width;
            } else if self.el(src).css.m_display == display_inline_text
                && self.render.item(el).kind == RenderKind::render_text
                && !self.render.item(el).text.as_ref().expect("a text").is_fragment
                && self.place_text(cx, this, st, &mut item, self_size, fmt_ctx)
            {
                return;
            }
        }

        let mut add_box = true;

        if let Some(back) = st.line_boxes.last() {
            let ws = self.ri_css(this).m_white_space;
            let trailing = if item.ty == LbiType::type_text_part { st.trailing } else { Px(0.0) };
            if self.line_box_can_hold(back, &item, ws, trailing) {
                add_box = false;
            }
        }
        if add_box {
            self.new_box(this, st, &item, self_size, fmt_ctx);
        }

        if !self.is_inline(src) {
            let margin_top = self.render.item(el).margins.top;
            let count = st.line_boxes.len();
            if count == 1 {
                if self.collapse_top_margin(this) {
                    let shift = margin_top;
                    if shift >= Px(0.0) {
                        let mut back = st.line_boxes.pop().expect("a line");
                        self.line_box_y_shift(&mut back, -shift);
                        st.line_boxes.push(back);
                    }
                }
            } else {
                let prev_margin = st.line_boxes[count - 2].bottom_margin();

                let shift = if prev_margin > margin_top { margin_top } else { prev_margin };
                if shift >= Px(0.0) {
                    let mut back = st.line_boxes.pop().expect("a line");
                    self.line_box_y_shift(&mut back, -shift);
                    st.line_boxes.push(back);
                }
            }
        }

        let mut back = st.line_boxes.pop().expect("a line");
        self.line_box_add_item(&mut back, item);
        st.line_boxes.push(back);
    }

    /// A text item of `place_inline`: its old pieces dropped, its size, and, when its words may break (not a space,
    /// not a break, `overflow-wrap` not `normal`, white space that wraps) and it is wider than its line or may break
    /// anywhere, a wrapped item; when it does not fit, cut by the breaker into pieces placed line by line. True when
    /// it was cut (its pieces are placed, not it).
    #[inline(never)]
    fn place_text(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        st: &mut InlineState,
        item: &mut LineBoxItem,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) -> bool {
        let text_item = item.element;
        for old in std::mem::take(&mut self.render.item_mut(text_item).text.as_mut().expect("a text").fragments) {
            self.release_render(old);
        }
        let src = self.render.item(text_item).element;
        let sz = self.get_content_size(cx, src, self_size.render_width.px());
        self.render.item_mut(text_item).pos.set_size(sz);
        item.rendered_min_width = sz.width;
        let is_text = self.is_text(src);
        let wrapping = get_wrapping(self.ri_css(text_item));
        let whitespace = self.ri_css(text_item).m_white_space;
        if !(is_text && self.breaks_in_line(src)) {
            return false;
        }
        let top = st.line_boxes.last().map_or(Px(0.0), |b| b.top);
        let (mut line_left, line_right) = fmt_ctx.get_line_left_right(top, self_size.render_width.px());
        if st.line_boxes.is_empty() {
            let css = self.ri_css(this);
            line_left += css.m_css_text_indent.calc_percent(self_size.width.px());
            if css.m_list_style_type != list_style_type_none && css.m_list_style_position == list_style_position_inside {
                line_left += css.get_font_size();
            }
        } else {
            line_left = line_left.max(st.line_boxes.last().expect("a line").left);
        }
        let line_width = line_right - line_left;
        let breaker = if wrapping == overflow_wrap_anywhere || sz.width + st.trailing > line_width { self.breaker(cx, src) } else { 0 };
        if breaker == 0 {
            return false;
        }
        let minimum = if wrapping == overflow_wrap_anywhere { cx.container.text_breaker_minimum_width(breaker) } else { sz.width };
        *item = LineBoxItem::wrapped(text_item, minimum);
        // Not the C++'s `>`: which is not `<=` with a NaN.
        if !matches!((sz.width + st.trailing).partial_cmp(&line_width), Some(std::cmp::Ordering::Greater)) {
            return false;
        }
        let content = self.display_text(src).to_vec();
        let needs_line = match st.line_boxes.last() {
            None => true,
            Some(back) => !self.line_box_can_hold(back, item, whitespace, st.trailing),
        };
        self.render.item_mut(text_item).pos.width = cx.container.text_breaker_minimum_width(breaker);
        if needs_line {
            self.new_box(this, st, item, self_size, fmt_ctx);
        }
        let trailing = st.trailing;
        let font = self.el(src).css.m_font;
        let mut start = 0usize;
        while start < content.len() {
            let back = st.line_boxes.last().expect("a line");
            let available = back.line_right() - back.right_edge();
            let mut end = cx.container.text_breaker_fit(breaker, start, available);
            if end <= start || end > content.len() {
                break;
            }
            if end == content.len() && trailing > Px(0.0) {
                end = cx.container.text_breaker_fit(breaker, start, available - trailing);
            }
            // std::string::substr(start, end - start): to the end of the text when the count runs past it, or wraps.
            let piece = if end >= start { content[start..end.min(content.len())].to_vec() } else { content[start..].to_vec() };
            let fragment = self.new_render_item(cx, RenderKind::render_text, src);
            let parent = self.render.parent(text_item);
            self.render.set_parent(fragment, parent);
            let width = cx.container.text_width(crate::gumbo::c_str(&piece), font);
            {
                let f = self.render.item_mut(fragment);
                let t = f.text.as_mut().expect("a text");
                t.is_fragment = true;
                t.fragment_text = piece;
                f.pos.set_size(Size::new(width, sz.height));
            }
            self.render.hold(fragment);
            self.render.item_mut(text_item).text.as_mut().expect("a text").fragments.push(fragment);
            st.trailing = if end == content.len() { trailing } else { Px(0.0) };
            self.place_inline(cx, this, st, LineBoxItem::wrapped(fragment, minimum), self_size, fmt_ctx);
            start = end;
            if start < content.len() {
                self.new_box(this, st, item, self_size, fmt_ctx);
            }
        }
        st.trailing = trailing;
        if !self.render.item(text_item).text.as_ref().expect("a text").fragments.is_empty() {
            st.fragmented_texts.push(text_item);
            return true;
        }
        self.render.item_mut(text_item).pos.set_size(sz);
        false
    }

    /// `render_item_inline_context::apply_vertical_align()`: the lines moved down by `vertical-align`'s middle or
    /// bottom share of the room below them; a box already moved is laid out again first.
    pub fn inline_context_apply_vertical_align(&mut self, cx: &StyleCx, this: RenderId) {
        let (empty, last_bottom, va_applied) = match self.render.item(this).inline.as_ref() {
            Some(st) => (st.line_boxes.is_empty(), st.line_boxes.last().map_or(Px(0.0), |b| b.bottom()), st.va_applied),
            None => (true, Px(0.0), Px(0.0)),
        };
        if empty {
            return;
        }
        let mut add = Px(0.0);

        let content_height = last_bottom - va_applied;

        let height = self.render.item(this).pos.height;
        if height > content_height {
            let va = self.ri_css(this).m_vertical_align;
            if va == va_middle {
                add = (height - content_height) / 2;
            } else if va == va_bottom {
                add = height - content_height;
            }
        }

        if add == va_applied {
            return;
        }
        let mut shift = add;
        if va_applied != Px(0.0) {
            let it = self.render.item(this);
            if it.layout.last_layout.valid {
                let placed = it.pos;
                let (x, y, cb, second_pass) =
                    (it.layout.last_x, it.layout.last_y, it.layout.last_layout.cb, it.layout.last_layout.second_pass);
                self.render.item_mut(this).layout.layout_dirty = true;
                self.render(cx, this, x, y, &cb, None, second_pass);
                self.render.item_mut(this).pos = placed;
            } else {
                shift = add - va_applied;
            }
        }
        let mut st = self.take_inline_state(this);
        if shift != Px(0.0) {
            for b in st.line_boxes.iter_mut() {
                self.line_box_y_shift(b, shift);
            }
        }
        st.va_applied = add;
        for &text in &st.fragmented_texts {
            self.update_fragment_bounds(text);
        }
        self.put_inline_state(this, st);
    }

    /// `control_baseline_inset()`: a checkbox's or radio button's baseline sits 2 px above its bottom.
    fn control_baseline_inset(&self, this: RenderId) -> Px {
        let element = self.render.item(this).element;
        if self.tag(element) != crate::string_id::_input_ {
            return Px(0.0);
        }
        match self.get_attr(element, b"type") {
            Some(ty) => {
                let ty = crate::gumbo::c_str(ty);
                if crate::html::t_strcasecmp(ty, b"checkbox") == 0 || crate::html::t_strcasecmp(ty, b"radio") == 0 {
                    Px(2.0)
                } else {
                    Px(0.0)
                }
            }
            None => Px(0.0),
        }
    }

    /// `render_item_inline_context::get_first_baseline()`: the first line's.
    pub fn inline_context_first_baseline(&self, this: RenderId) -> Px {
        let it = self.render.item(this);
        match it.inline.as_ref().and_then(|st| st.line_boxes.first()) {
            Some(line) => line.bottom() - line.baseline + it.content_offset_top(),
            None => it.height() - it.margins.bottom - self.control_baseline_inset(this),
        }
    }

    /// `render_item_inline_context::get_last_baseline()`: the last line's; with no line an input's bottom, else the
    /// box's.
    pub fn inline_context_last_baseline(&self, this: RenderId) -> Px {
        let it = self.render.item(this);
        match it.inline.as_ref().and_then(|st| st.line_boxes.last()) {
            Some(line) => line.bottom() - line.baseline + it.content_offset_top(),
            None => {
                if self.tag(it.element) == crate::string_id::_input_ {
                    it.height() - it.margins.bottom - self.control_baseline_inset(this)
                } else {
                    it.height()
                }
            }
        }
    }

    /// `text_content_size()`: a `<textarea>`'s content as its lines and line height make it, for its scroll view.
    pub fn text_content_size(&self, this: RenderId) -> Size {
        let it = self.render.item(this);
        let st = it.inline.as_ref();
        let mut width = st.map_or(Px(0.0), |st| st.rendered_width.natural_width);
        let lines = st.map_or(&[][..], |st| &st.line_boxes[..]);
        let mut height = lines.last().map_or(Px(0.0), |b| b.bottom());
        if self.tag(it.element) == crate::string_id::_textarea_ {
            let line = self.ri_css(this).m_line_height.computed_value;
            height = Px(lines.len() as f32) * line;
            if let Some(&last) = it.children.last()
                && self.is_break(self.render.item(last).element)
            {
                height += line;
            }
            height = height.max(line);
            if self.ri_css(this).m_white_space == white_space_pre_wrap {
                width = Px(0.0);
                for &child in &it.children {
                    if !self.is_space(self.render.item(child).element) {
                        width = width.max(self.render.item(child).right());
                    }
                }
            }
        }
        Size::new(width, height)
    }

    /// `render_item_inline_context::measure(x, y, cb, fmt_ctx, second_pass, measured_height)`: a content measure
    /// (no height asked) of an `auto`-sized box with no room, whose children are all texts whose words may break
    /// anywhere, is its widest unbreakable piece, without laying it out; else `render_item::measure`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn inline_context_measure(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        x: Px,
        y: Px,
        cb: &ContainingBlockContext,
        fmt_ctx: Option<&mut FormattingContext>,
        second_pass: bool,
        measured_height: Option<&mut Px>,
    ) -> RenderedWidth {
        let css = self.ri_css(this);
        if measured_height.is_none()
            && (cb.size_mode & size_mode_content) != 0
            && css.m_css_width.is_predefined()
            && css.m_css_min_width.is_predefined()
            && css.m_css_max_width.is_predefined()
            && (css.m_contain & contain_size) == 0
            && (css.m_contain & contain_inline_size) == 0
            && (css.m_list_style_type == list_style_type_none || css.m_list_style_position != list_style_position_inside)
            && css.m_css_text_indent.val() == 0.0
            && !self.render.children(this).is_empty()
        {
            self.calc_outlines(this, cb.width.px());
            let self_size = self.calculate_containing_block_context(cx, this, cb);
            if self_size.render_width.px() <= Px(0.0) {
                let mut minimum = Px(0.0);
                let mut supported = true;
                for k in 0..self.render.children(this).len() {
                    let child = self.render.children(this)[k];
                    let text = self.render.item(child).element;
                    let tcss = &self.el(text).css;
                    if !self.is_text(text)
                        || get_wrapping(tcss) != overflow_wrap_anywhere
                        || tcss.m_white_space == white_space_pre
                        || tcss.m_white_space == white_space_nowrap
                    {
                        supported = false;
                        break;
                    }
                    if !self.breaks_at_narrowest(text) {
                        continue;
                    }
                    let breaker = self.breaker(cx, text);
                    if breaker == 0 {
                        supported = false;
                        break;
                    }
                    minimum = minimum.max(cx.container.text_breaker_minimum_width(breaker));
                }
                if supported {
                    let it = self.render.item(this);
                    let natural_width = minimum + it.content_offset_width();
                    let overflow = self.ri_css(this).m_overflow;
                    let min_width =
                        if overflow == overflow_visible || overflow == overflow_clip { natural_width } else { it.content_offset_width() };
                    return RenderedWidth { natural_width, min_width };
                }
            }
        }
        self.render_item_measure(cx, this, x, y, cb, fmt_ctx, second_pass, measured_height)
    }
}

impl Document {
    /// `render_item_inline_context::draw_children(hdc, x, y, clip, flag, zindex)`: the base class's, then in the
    /// inlines' pass each ellipsis `text-overflow` placed, in the block's font and colour.
    #[allow(clippy::too_many_arguments)]
    pub fn inline_context_draw_children(
        &mut self,
        cx: &StyleCx,
        id: RenderId,
        hdc: u64,
        x: Px,
        y: Px,
        clip: Option<&Position>,
        flag: crate::paint::DrawFlag,
        zindex: i32,
    ) {
        self.render_item_draw_children(cx, id, hdc, x, y, clip, flag, zindex);
        let css = self.ri_css(id);
        let ellipses = self.render.item(id).inline.as_ref().map_or(&[][..], |ic| &ic.ellipses[..]);
        if flag != crate::paint::DrawFlag::draw_inlines || ellipses.is_empty() || css.m_font == 0 {
            return;
        }
        let it = self.render.item(id);
        for at in ellipses {
            let mut at = *at;
            at.x += it.pos.x + x;
            at.y += it.pos.y + y;
            cx.container.draw_text(hdc, "\u{2026}".as_bytes(), css.m_font, css.m_color, &at);
        }
    }
}
