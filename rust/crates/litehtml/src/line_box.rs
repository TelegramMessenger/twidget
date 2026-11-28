//! `line_box.h` and `line_box.cpp`: a line of an inline formatting context and what it holds. A line box item is a
//! text part (a word, a space, a piece of a wrapped text, an inline box: its place is its render item's), or the
//! start, continuation or end of an inline element (with a place of its own). A line lays its items out left to
//! right as they come, then `finish` aligns them (`text-align`, justification), sets them on their baselines
//! (`vertical-align`, line heights, the strut), mirrors right-to-left runs (`reorder_bidi`), shifts relatively
//! positioned inlines and records the inline elements' boxes.
//!
//! The C++'s list of `unique_ptr`s to items of five classes is a list of [`LineBoxItem`]s of four kinds;
//! `wrapped_text_item` (a text part whose natural width is its width) is the `wrapped` flag. The items' virtual
//! methods are the document's `lbi_*` functions, since a text part's place is its render item's.

use crate::Px;
use crate::css_properties::LineHeight;
use crate::css_values::*;
use crate::document::Document;
use crate::render_item::*;
use crate::types::{ContainingBlockContext, FontMetrics, Position, RenderedWidth};

/// `line_box_item::element_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LbiType {
    type_text_part,
    type_inline_start,
    type_inline_continue,
    type_inline_end,
}

/// A line box item: `line_box_item` (a text part), `wrapped_text_item`, `lbi_start`, `lbi_end`, `lbi_continue`.
#[derive(Clone, Copy, Debug)]
pub struct LineBoxItem {
    pub ty: LbiType,
    /// A `wrapped_text_item`.
    pub wrapped: bool,
    /// `m_element`.
    pub element: RenderId,
    /// `m_rendered_min_width`, `m_items_top`, `m_items_bottom`.
    pub rendered_min_width: Px,
    pub items_top: Px,
    pub items_bottom: Px,
    /// `lbi_start::m_pos`: a start's, an end's and a continuation's own place.
    pub pos: Position,
}

impl LineBoxItem {
    /// `line_box_item(element)`.
    pub fn text(element: RenderId) -> LineBoxItem {
        LineBoxItem {
            ty: LbiType::type_text_part,
            wrapped: false,
            element,
            rendered_min_width: Px(0.0),
            items_top: Px(0.0),
            items_bottom: Px(0.0),
            pos: Position::default(),
        }
    }

    /// `wrapped_text_item(item, minimum)`.
    pub fn wrapped(element: RenderId, minimum: Px) -> LineBoxItem {
        LineBoxItem { wrapped: true, rendered_min_width: minimum, ..LineBoxItem::text(element) }
    }

    /// `reset_items_height()`.
    pub fn reset_items_height(&mut self) {
        self.items_top = Px(0.0);
        self.items_bottom = Px(0.0);
    }

    /// `add_item_height(item_top, item_bottom)`.
    pub fn add_item_height(&mut self, item_top: Px, item_bottom: Px) {
        self.items_top = self.items_top.min(item_top);
        self.items_bottom = self.items_bottom.max(item_bottom);
    }
}

/// `line_box`.
#[derive(Clone, Debug)]
pub struct LineBox {
    pub top: Px,
    pub left: Px,
    pub right: Px,
    pub height: Px,
    pub width: Px,
    pub default_line_height: LineHeight,
    pub font_metrics: FontMetrics,
    pub baseline: Px,
    pub text_align: i32,
    pub rtl: bool,
    pub rendered_width: RenderedWidth,
    pub items: Vec<LineBoxItem>,
}

impl LineBox {
    /// `line_box(top, left, right, line_height, fm, align, rtl)`.
    pub fn new(top: Px, left: Px, right: Px, line_height: &LineHeight, fm: &FontMetrics, align: i32, rtl: bool) -> LineBox {
        LineBox {
            top,
            left,
            right,
            height: Px(0.0),
            width: Px(0.0),
            default_line_height: line_height.clone(),
            font_metrics: *fm,
            baseline: Px(0.0),
            text_align: align,
            rtl,
            rendered_width: RenderedWidth::default(),
            items: Vec::new(),
        }
    }

    /// `bottom()`.
    pub fn bottom(&self) -> Px {
        self.top + self.height
    }

    /// `right()`: where the items end.
    pub fn right_edge(&self) -> Px {
        self.left + self.width
    }

    /// `line_right()`.
    pub fn line_right(&self) -> Px {
        self.right
    }

    /// `top_margin()`.
    pub fn top_margin(&self) -> Px {
        Px(0.0)
    }

    /// `bottom_margin()`.
    pub fn bottom_margin(&self) -> Px {
        Px(0.0)
    }
}

/// `va_context`: the baseline the items are set on and the font around them; `start_lbi` an item of the line.
#[derive(Clone, Copy, Debug)]
struct VaContext {
    line_height: Px,
    baseline: Px,
    fm: FontMetrics,
    start_lbi: Option<usize>,
}

/// `items_dimensions`.
#[derive(Clone, Copy, Debug, Default)]
struct ItemsDimensions {
    top: Px,
    bottom: Px,
    count: i32,
    max_height: Px,
}

impl ItemsDimensions {
    fn add(&mut self, top: Px, bottom: Px, height: Px) {
        self.top = self.top.min(top);
        self.bottom = self.bottom.max(bottom);
        self.max_height = self.max_height.max(height);
        self.count += 1;
    }

    fn height(&self) -> Px {
        self.bottom - self.top
    }
}

impl Document {
    // ---- The items' virtual methods ----------------------------------------------------------------------------

    /// `lbi_start(element)`: the element's font height, its left offsets as its width.
    pub fn lbi_start(&self, element: RenderId) -> LineBoxItem {
        let item = self.render.item(element);
        let height = self.el(item.element).css.m_font_metrics.height;
        LineBoxItem {
            ty: LbiType::type_inline_start,
            pos: Position { width: item.content_offset_left(), height, ..Position::default() },
            ..LineBoxItem::text(element)
        }
    }

    /// `lbi_end(element)`: its right offsets as its width.
    pub fn lbi_end(&self, element: RenderId) -> LineBoxItem {
        let item = self.render.item(element);
        let height = self.el(item.element).css.m_font_metrics.height;
        LineBoxItem {
            ty: LbiType::type_inline_end,
            pos: Position { width: item.content_offset_right(), height, ..Position::default() },
            ..LineBoxItem::text(element)
        }
    }

    /// `lbi_continue(element)`: no width.
    pub fn lbi_continue(&self, element: RenderId) -> LineBoxItem {
        let item = self.render.item(element);
        let height = self.el(item.element).css.m_font_metrics.height;
        LineBoxItem {
            ty: LbiType::type_inline_continue,
            pos: Position { width: Px(0.0), height, ..Position::default() },
            ..LineBoxItem::text(element)
        }
    }

    /// `pos()`: a text part's render item's place, else the item's own.
    pub fn lbi_pos(&self, lbi: &LineBoxItem) -> Position {
        match lbi.ty {
            LbiType::type_text_part => self.render.item(lbi.element).pos,
            _ => lbi.pos,
        }
    }

    /// `pos()` to change.
    pub fn lbi_pos_mut<'a>(&'a mut self, lbi: &'a mut LineBoxItem) -> &'a mut Position {
        match lbi.ty {
            LbiType::type_text_part => &mut self.render.item_mut(lbi.element).pos,
            _ => &mut lbi.pos,
        }
    }

    /// `place_to(x, y)`.
    pub fn lbi_place_to(&mut self, lbi: &mut LineBoxItem, x: Px, y: Px) {
        match lbi.ty {
            LbiType::type_text_part => {
                let item = self.render.item_mut(lbi.element);
                item.pos.x = x + item.content_offset_left();
                item.pos.y = y + item.content_offset_top();
            }
            LbiType::type_inline_start => {
                let offset = self.render.item(lbi.element).content_offset_left();
                lbi.pos.x = x + offset;
                lbi.pos.y = y;
            }
            LbiType::type_inline_end | LbiType::type_inline_continue => {
                lbi.pos.x = x;
                lbi.pos.y = y;
            }
        }
    }

    /// `width()`.
    pub fn lbi_width(&self, lbi: &LineBoxItem) -> Px {
        match lbi.ty {
            LbiType::type_text_part => self.render.item(lbi.element).width(),
            LbiType::type_inline_start | LbiType::type_inline_end => lbi.pos.width,
            LbiType::type_inline_continue => Px(0.0),
        }
    }

    /// `top()`.
    pub fn lbi_top(&self, lbi: &LineBoxItem) -> Px {
        match lbi.ty {
            LbiType::type_text_part => self.render.item(lbi.element).top(),
            _ => lbi.pos.y,
        }
    }

    /// `bottom()`.
    pub fn lbi_bottom(&self, lbi: &LineBoxItem) -> Px {
        match lbi.ty {
            LbiType::type_text_part => self.render.item(lbi.element).bottom(),
            _ => lbi.pos.y + lbi.pos.height,
        }
    }

    /// `right()`.
    pub fn lbi_right(&self, lbi: &LineBoxItem) -> Px {
        match lbi.ty {
            LbiType::type_text_part => self.render.item(lbi.element).right(),
            LbiType::type_inline_start | LbiType::type_inline_continue => lbi.pos.x,
            LbiType::type_inline_end => lbi.pos.x + lbi.pos.width,
        }
    }

    /// `left()`.
    pub fn lbi_left(&self, lbi: &LineBoxItem) -> Px {
        match lbi.ty {
            LbiType::type_text_part => self.render.item(lbi.element).left(),
            LbiType::type_inline_start => lbi.pos.x - self.render.item(lbi.element).content_offset_left(),
            LbiType::type_inline_end | LbiType::type_inline_continue => lbi.pos.x,
        }
    }

    /// `height()`.
    pub fn lbi_height(&self, lbi: &LineBoxItem) -> Px {
        match lbi.ty {
            LbiType::type_text_part => self.render.item(lbi.element).height(),
            _ => lbi.pos.height,
        }
    }

    /// `get_rendered_min_width()`: a start's (an end's, a continuation's) is its width.
    pub fn lbi_rendered_min_width(&self, lbi: &LineBoxItem) -> Px {
        match lbi.ty {
            LbiType::type_text_part => lbi.rendered_min_width,
            _ => self.lbi_width(lbi),
        }
    }

    /// `get_rendered_natural_width()`: a wrapped text's is its width.
    pub fn lbi_rendered_natural_width(&self, lbi: &LineBoxItem) -> Px {
        if lbi.wrapped { self.lbi_width(lbi) } else { self.lbi_rendered_min_width(lbi) }
    }

    /// `y_shift(shift)`: a text part, a start and a continuation shift their render item (not a start's own place);
    /// an end nothing.
    pub fn lbi_y_shift(&mut self, lbi: &LineBoxItem, shift: Px) {
        if lbi.ty != LbiType::type_inline_end {
            self.y_shift(lbi.element, shift);
        }
    }

    /// The element of a line box item's render item.
    fn lbi_css(&self, lbi: &LineBoxItem) -> &crate::css_properties::CssProperties {
        &self.el(self.render.item(lbi.element).element).css
    }

    /// `src_el()` of the item's render item.
    fn lbi_src(&self, lbi: &LineBoxItem) -> crate::element::ElementId {
        self.render.item(lbi.element).element
    }

    // ---- The line box --------------------------------------------------------------------------------------------

    /// `add_item(item)`: shown (a collapsible space only after something that is not one), at the line's end.
    pub fn line_box_add_item(&mut self, lb: &mut LineBox, mut item: LineBoxItem) {
        self.render.item_mut(item.element).skip = false;
        let add = match item.ty {
            LbiType::type_text_part if self.is_white_space(self.lbi_src(&item)) => {
                !self.line_box_is_empty(lb) && !self.line_box_have_last_space(lb)
            }
            _ => true,
        };
        if add {
            self.lbi_place_to(&mut item, lb.left + lb.width, lb.top);
            lb.width += self.lbi_width(&item);
            lb.height = lb.height.max(self.render.item(item.element).height());
            lb.items.push(item);
        } else {
            self.render.item_mut(item.element).skip = true;
        }
    }

    /// `calc_va_baseline(current, va, new_font, top, bottom)`.
    fn calc_va_baseline(current: &VaContext, va: i32, new_font: &FontMetrics, top: Px, bottom: Px) -> Px {
        match va {
            va_super => current.baseline - current.fm.super_shift,
            va_sub => current.baseline + current.fm.sub_shift,
            va_middle => {
                current.baseline - current.fm.x_height / Px(2.0) + (new_font.height - new_font.base_line() - new_font.base_line()) / Px(2.0)
            }
            va_text_top => current.baseline - (current.fm.height - current.fm.base_line()) + new_font.height - new_font.base_line(),
            va_text_bottom => current.baseline + current.fm.base_line() - new_font.base_line(),
            va_bottom => bottom - new_font.base_line(),
            va_top => top + new_font.height - new_font.base_line(),
            _ => current.baseline,
        }
    }

    /// `finish(last_box, containing_block_size)`: trailing spaces dropped (and, mid-text, trailing starts handed to
    /// the next line), the items aligned and set on their baselines, the line's height and baseline, bidi runs
    /// mirrored, relative shifts, the inline elements' boxes. Gives what the next line starts with: a continuation
    /// of each inline element still open, then the starts it took off.
    pub fn line_box_finish(
        &mut self,
        lb: &mut LineBox,
        last_box: bool,
        containing_block_size: &ContainingBlockContext,
    ) -> Vec<LineBoxItem> {
        let mut moved_items: Vec<LineBoxItem> = Vec::new();
        let mut finished_with_break = false;

        if !last_box {
            if !self.line_box_is_empty(lb) {
                let last = lb.items.last().expect("an item");
                finished_with_break = self.is_break(self.lbi_src(last));
            }
            while let Some(last) = lb.items.last().copied() {
                if last.ty == LbiType::type_text_part {
                    // remove trailing spaces
                    let src = self.lbi_src(&last);
                    if self.is_break(src) || self.is_white_space(src) {
                        lb.width -= self.lbi_width(&last);
                        self.render.item_mut(last.element).skip = true;
                        lb.items.pop();
                    } else {
                        break;
                    }
                } else if last.ty == LbiType::type_inline_start {
                    // remove trailing empty inline_start markers
                    // these markers will be added at the beginning of the next line box
                    lb.width -= self.lbi_width(&last);
                    moved_items.push(last);
                    lb.items.pop();
                } else {
                    break;
                }
            }
        } else {
            // remove trailing spaces
            let mut iter = lb.items.len();
            while iter > 0 {
                let k = iter - 1;
                let item = lb.items[k];
                if item.ty == LbiType::type_text_part {
                    if self.is_white_space(self.lbi_src(&item)) {
                        self.render.item_mut(item.element).skip = true;
                        let w = self.lbi_width(&item);
                        lb.width -= w;
                        // Space can be between text and inline_end marker
                        // We have to shift all items on the right side
                        for r in (k + 1)..lb.items.len() {
                            let width = self.lbi_width(&item);
                            let mut right = lb.items[r];
                            self.lbi_pos_mut(&mut right).x -= width;
                            lb.items[r] = right;
                        }
                        // erase white space element
                        lb.items.remove(k);
                        iter = k;
                    } else {
                        break;
                    }
                } else {
                    iter -= 1;
                }
            }
        }

        if self.line_box_is_empty(lb) || (!self.line_box_is_empty(lb) && last_box && self.line_box_is_break_only(lb)) {
            lb.height = lb.default_line_height.computed_value;
            lb.baseline = lb.font_metrics.base_line();
            return moved_items;
        }

        let mut spacing_x = Px(0.0); // Number of pixels to distribute between elements
        let mut shift_x = Px(0.0); // Shift elements by X to apply the text-align

        // The line is laid out left to right and mirrored afterwards when it is right to left (reorder_bidi), so
        // its alignment is mirrored here: start is the left until then.
        let mut align = lb.text_align;
        if align == text_align_start {
            align = text_align_left;
        } else if align == text_align_end || (lb.rtl && align == text_align_left) {
            align = text_align_right;
        } else if lb.rtl && align == text_align_right {
            align = text_align_left;
        }

        match align {
            text_align_right | text_align_end => {
                if lb.width < (lb.right - lb.left) {
                    shift_x = (lb.right - lb.left) - lb.width;
                }
            }
            text_align_center => {
                if lb.width < (lb.right - lb.left) {
                    shift_x = ((lb.right - lb.left) - lb.width) / Px(2.0);
                }
            }
            text_align_justify => {
                if lb.width < (lb.right - lb.left) && !last_box && !finished_with_break {
                    shift_x = Px(0.0);
                    spacing_x = (lb.right - lb.left) - lb.width;
                }
            }
            _ => shift_x = Px(0.0),
        }

        let mut counter: i32 = 0;
        let offj = spacing_x.value() / crate::cxx::max_f32(1.0, lb.items.len() as f32 - 1.0);
        let mut cixx = 0.0f32;

        let mut line_height: Option<Px> = None;

        if !lb.default_line_height.css_value.is_predefined() {
            line_height = Some(lb.default_line_height.computed_value);
        }

        let mut current_context =
            VaContext { line_height: lb.default_line_height.computed_value, baseline: Px(0.0), fm: lb.font_metrics, start_lbi: None };
        let mut contexts: Vec<VaContext> = Vec::new();

        lb.rendered_width.reset();

        let mut line_max_height = ItemsDimensions::default();
        let mut leading_boxes = ItemsDimensions::default();
        let mut top_aligned_max_height = ItemsDimensions::default();
        let mut bottom_aligned_max_height = ItemsDimensions::default();
        let mut inline_boxes_dims = ItemsDimensions::default();

        // First pass
        // 1. Apply text-align-justify
        // 1. Align all items by baseline
        // 2. top/button aligned items are aligned by baseline
        // 3. Calculate top and button of the linebox separately for items in baseline
        //    and for top and bottom aligned items
        let count = lb.items.len();
        for i in 0..count {
            let mut lbi = lb.items[i];
            // Apply text-align-justify
            lb.rendered_width.natural_width += self.lbi_rendered_natural_width(&lbi);
            lb.rendered_width.min_width = lb.rendered_width.min_width.max(self.lbi_rendered_min_width(&lbi));
            if spacing_x != Px(0.0) && counter != 0 {
                cixx += offj;
                if (counter + 1) == count as i32 {
                    cixx += 0.99;
                }
                self.lbi_pos_mut(&mut lbi).x += Px(cixx);
            }
            counter += 1;
            if (align == text_align_right || spacing_x != Px(0.0)) && counter == count as i32 {
                // Forcible justify the last element to the right side for text align right and justify.
                // Its outer edge meets the side: padding, borders and margins of an inline-block or an
                // inline end stay inside the line, as in browsers (pos() alone is the content box).
                let shift = lb.right - self.lbi_right(&lbi);
                self.lbi_pos_mut(&mut lbi).x += shift;
            } else if shift_x != Px(0.0) {
                self.lbi_pos_mut(&mut lbi).x += shift_x;
            }
            lb.items[i] = lbi;

            let el_css_va = self.lbi_css(&lbi).m_vertical_align;
            let el_fm = self.lbi_css(&lbi).m_font_metrics;
            // Calculate new baseline for inline start/continue
            // Inline start/continue elements are inline containers like <span>
            if lbi.ty == LbiType::type_inline_start || lbi.ty == LbiType::type_inline_continue {
                contexts.push(current_context);
                if el_css_va == va_top || el_css_va == va_bottom {
                    // top/bottom aligned inline boxes are aligned by baseline == 0
                    current_context.baseline = Px(0.0);
                    current_context.start_lbi = Some(i);
                    lb.items[i].reset_items_height();
                } else if let Some(start) = current_context.start_lbi {
                    let (top, bottom) = (self.lbi_top(&lb.items[start]), self.lbi_bottom(&lb.items[start]));
                    current_context.baseline = Document::calc_va_baseline(&current_context, el_css_va, &el_fm, top, bottom);
                } else {
                    current_context.start_lbi = None;
                    current_context.baseline =
                        Document::calc_va_baseline(&current_context, el_css_va, &el_fm, line_max_height.top, line_max_height.bottom);
                }
                current_context.fm = el_fm;
                current_context.line_height = self.lbi_css(&lbi).m_line_height.computed_value;
            }
            let mut lbi = lb.items[i];

            let mut bl = current_context.baseline;
            let mut content_offset = Px(0.0);
            let mut is_top_bottom_box = false;
            let mut ignore = false;

            // Align element by baseline
            let display = self.lbi_css(&lbi).m_display;
            if display != display_inline_text && display != display_inline {
                // Apply margins, paddings and border for inline boxes
                content_offset = self.render.item(lbi.element).content_offset_top();
                match el_css_va {
                    va_bottom | va_top => {
                        // Align by base line 0 all inline boxes with top and bottom vertical align
                        bl = Px(0.0);
                        is_top_bottom_box = true;
                    }
                    va_text_bottom => {
                        let h = self.render.item(lbi.element).height();
                        self.lbi_pos_mut(&mut lbi).y = bl + current_context.fm.base_line() - h + content_offset;
                        ignore = true;
                    }
                    va_text_top => {
                        self.lbi_pos_mut(&mut lbi).y = bl - current_context.fm.ascent + content_offset;
                        ignore = true;
                    }
                    va_middle => {
                        let h = self.render.item(lbi.element).height();
                        self.lbi_pos_mut(&mut lbi).y = bl - current_context.fm.x_height / Px(2.0) - h / Px(2.0) + content_offset;
                        ignore = true;
                    }
                    _ => {
                        bl = Document::calc_va_baseline(&current_context, el_css_va, &el_fm, line_max_height.top, line_max_height.bottom);
                    }
                }
            }
            if !ignore {
                let last_baseline = self.get_last_baseline(lbi.element);
                self.lbi_pos_mut(&mut lbi).y = bl - last_baseline + content_offset;
            }
            lb.items[i] = lbi;

            let (top, bottom, height) = (self.lbi_top(&lbi), self.lbi_bottom(&lbi), self.lbi_height(&lbi));
            let is_inline_box = self.is_inline_box(self.lbi_src(&lbi));
            if is_top_bottom_box {
                match el_css_va {
                    va_top => top_aligned_max_height.add(top, bottom, height),
                    va_bottom => bottom_aligned_max_height.add(top, bottom, height),
                    _ => {}
                }
            } else if let Some(start) = current_context.start_lbi {
                lb.items[start].add_item_height(top, bottom);
                match self.lbi_css(&lb.items[start]).m_vertical_align {
                    va_top => top_aligned_max_height.add(top, bottom, height),
                    va_bottom => bottom_aligned_max_height.add(top, bottom, height),
                    _ => {}
                }
            } else if !is_inline_box {
                line_max_height.add(top, bottom, height);
                let leading = self.lbi_css(&lbi).m_line_height.computed_value - height;
                let above = Px((leading / Px(2.0)).value().trunc());
                leading_boxes.top = if leading_boxes.count != 0 { leading_boxes.top.min(top - above) } else { top - above };
                leading_boxes.bottom =
                    if leading_boxes.count != 0 { leading_boxes.bottom.max(bottom + leading - above) } else { bottom + leading - above };
                leading_boxes.count += 1;
            } else {
                inline_boxes_dims.add(top, bottom, height);
            }

            if !is_inline_box && !self.lbi_css(&lbi).m_line_height.css_value.is_predefined() {
                let lh = self.lbi_css(&lbi).m_line_height.computed_value;
                line_height = Some(match line_height {
                    Some(v) => v.max(lh),
                    None => lh,
                });
            }

            if lbi.ty == LbiType::type_inline_end
                && let Some(context) = contexts.pop()
            {
                current_context = context;
            }
        }

        if line_max_height.count == 0
            && !lb.items.is_empty()
            && (inline_boxes_dims.count != 0 || top_aligned_max_height.count != 0 || bottom_aligned_max_height.count != 0)
            && self.mode == crate::css_selector::DocumentMode::no_quirks_mode
        {
            line_max_height.top = -(lb.font_metrics.height - lb.font_metrics.base_line());
            line_max_height.bottom = lb.font_metrics.base_line();
            line_max_height.count = 1;
            if line_height.is_none() {
                line_height = Some(lb.default_line_height.computed_value);
            }
        }

        let mut top_shift;
        if line_max_height.count != 0 {
            let strut_leading = lb.default_line_height.computed_value - lb.font_metrics.height;
            let strut_above = Px((strut_leading / Px(2.0)).value().trunc());
            let mut top = -(lb.font_metrics.height - lb.font_metrics.base_line()) - strut_above;
            let mut bottom = lb.font_metrics.base_line() + strut_leading - strut_above;
            if leading_boxes.count != 0 {
                top = top.min(leading_boxes.top);
                bottom = bottom.max(leading_boxes.bottom);
            }
            if inline_boxes_dims.count != 0 {
                top = top.min(inline_boxes_dims.top);
                bottom = bottom.max(inline_boxes_dims.bottom);
            }
            lb.height = bottom - top;
            top_shift = -top;
            lb.baseline = bottom;

            let top_down_height = top_aligned_max_height.max_height.max(bottom_aligned_max_height.max_height);
            if top_down_height > lb.height {
                if bottom_aligned_max_height.count != 0 {
                    top_shift += bottom_aligned_max_height.height() - lb.height;
                }
                lb.height = top_down_height;
            }
        } else if let Some(lh) = line_height {
            lb.height = lh;
            if line_max_height.count != 0 {
                // We have inline items
                top_shift = Px(line_max_height.top.value().abs());

                let top_shift_correction = Px(((lh - line_max_height.height()) / Px(2.0)).value().trunc());
                // We have to calculate the baseline from the top of the line box due to possible round errors.
                // The top_shift_correction is actually text offset from the top of the line box.
                // The (top_shift_correction + line_max_height.height()): is the bottom of the text with shift
                // (text_bottom). The line_max_height.bottom is the text baseline here. So the formula is: baseline =
                // line_height - text_bottom + text_baseline The linebox baseline is the length from the bottom of the line
                // box to the text baseline.
                lb.baseline = lh - (top_shift_correction + line_max_height.height()) + line_max_height.bottom;
                top_shift += top_shift_correction;
                if inline_boxes_dims.count != 0 {
                    let diff2 = Px(inline_boxes_dims.top.value().abs() - top_shift.value().abs());
                    if diff2 > Px(0.0) {
                        lb.height += diff2;
                        top_shift += diff2;
                        lb.baseline += diff2;
                    }
                    let diff1 = inline_boxes_dims.bottom - (lh - top_shift_correction - line_max_height.height() + line_max_height.bottom);
                    if diff1 > Px(0.0) {
                        lb.height += diff1;
                    }
                }
            } else if inline_boxes_dims.count != 0 {
                // We have inline boxes only
                lb.height = inline_boxes_dims.height();
                top_shift = Px(inline_boxes_dims.top.value().abs());
                lb.baseline = inline_boxes_dims.bottom;
            } else {
                // We don't have inline items and inline boxes
                top_shift = Px(0.0);
            }

            let top_down_height = top_aligned_max_height.max_height.max(bottom_aligned_max_height.max_height);
            if top_down_height > lb.height {
                if bottom_aligned_max_height.count != 0 {
                    top_shift += bottom_aligned_max_height.height() - lb.height;
                }
                lb.height = top_down_height;
            }
        } else {
            // Add inline boxes dimensions
            line_max_height.top = line_max_height.top.min(inline_boxes_dims.top);
            line_max_height.bottom = line_max_height.bottom.max(inline_boxes_dims.bottom);

            // Height is maximum from inline elements height and top/bottom aligned elements height
            lb.height = line_max_height.height().max(top_aligned_max_height.height()).max(bottom_aligned_max_height.height());

            top_shift = -line_max_height.top.min(line_max_height.bottom - bottom_aligned_max_height.height());
            lb.baseline = line_max_height.bottom;
        }

        self.line_box_reorder_bidi(lb);

        contexts.clear();

        current_context.baseline = Px(0.0);
        current_context.fm = lb.font_metrics;
        current_context.start_lbi = None;

        // Second pass:
        // 1. Vertical align top/bottom
        // 2. Apply relative shift
        // 3. Calculate inline boxes
        let mut inlines: Vec<(RenderId, Position)> = Vec::new();
        for i in 0..lb.items.len() {
            let mut lbi = lb.items[i];
            let va = self.lbi_css(&lbi).m_vertical_align;
            if lbi.ty == LbiType::type_inline_start || lbi.ty == LbiType::type_inline_continue {
                contexts.push(current_context);
                current_context.fm = self.lbi_css(&lbi).m_font_metrics;

                if va == va_top {
                    current_context.baseline = lb.top - lbi.items_top;
                    current_context.start_lbi = Some(i);
                } else if va == va_bottom {
                    current_context.baseline = lb.top + lb.height - lbi.items_bottom;
                    current_context.start_lbi = Some(i);
                }
            } else if lbi.ty == LbiType::type_inline_end
                && let Some(context) = contexts.pop()
            {
                current_context = context;
            }

            if current_context.start_lbi.is_some() {
                let y = current_context.baseline - self.get_last_baseline(lbi.element) + self.render.item(lbi.element).content_offset_top();
                self.lbi_pos_mut(&mut lbi).y = y;
            } else if (va == va_top || va == va_bottom) && lbi.ty == LbiType::type_text_part {
                if va == va_top {
                    let y = lb.top + self.render.item(lbi.element).content_offset_top();
                    self.lbi_pos_mut(&mut lbi).y = y;
                } else {
                    let y = lb.top + lb.height - (self.lbi_bottom(&lbi) - self.lbi_top(&lbi))
                        + self.render.item(lbi.element).content_offset_bottom();
                    self.lbi_pos_mut(&mut lbi).y = y;
                }
            } else {
                // move element to the correct position
                self.lbi_pos_mut(&mut lbi).y += lb.top + top_shift;
            }

            self.apply_relative_shift(lbi.element, containing_block_size);
            // What a relatively positioned inline element holds moves with it: its text and boxes are items of their own.
            // So do its start and end, which keep a position of their own that the element's shift above does not move.
            let marker = lbi.ty != LbiType::type_text_part;
            let src = self.lbi_src(&lbi);
            let mut p = if marker { Some(src) } else { self.parent(src) };
            while let Some(e) = p {
                if self.el(e).css.m_display != display_inline {
                    break;
                }
                if self.el(e).css.m_el_position == element_position_relative {
                    let offsets = self.el(e).css.m_css_offsets.clone();
                    if !offsets.left.is_predefined() {
                        self.lbi_pos_mut(&mut lbi).x += offsets.left.calc_percent(containing_block_size.width.px());
                    } else if !offsets.right.is_predefined() {
                        self.lbi_pos_mut(&mut lbi).x -= offsets.right.calc_percent(containing_block_size.width.px());
                    }
                    if !offsets.top.is_predefined() {
                        self.lbi_pos_mut(&mut lbi).y += offsets.top.calc_percent(containing_block_size.height.px());
                    } else if !offsets.bottom.is_predefined() {
                        self.lbi_pos_mut(&mut lbi).y -= offsets.bottom.calc_percent(containing_block_size.height.px());
                    }
                }
                p = self.parent(e);
            }
            lb.items[i] = lbi;

            // Calculate and push inline box into the render item element. An inline box spans whatever its items
            // take, in whichever order a reordered line (reorder_bidi) shows them.
            let (left, right) = (self.lbi_left(&lbi), self.lbi_right(&lbi));
            for open in inlines.iter_mut() {
                let r = open.1.right().max(right);
                open.1.x = open.1.x.min(left);
                open.1.width = r - open.1.x;
            }
            if lbi.ty == LbiType::type_inline_start || lbi.ty == LbiType::type_inline_continue {
                if lbi.ty == LbiType::type_inline_start {
                    self.clear_inline_boxes(lbi.element);
                }
                let el = self.render.item(lbi.element);
                let top = self.lbi_top(&lbi);
                let bottom = self.lbi_bottom(&lbi);
                let b = Position::new(left, top - el.content_offset_top(), right - left, bottom - top + el.content_offset_height());
                inlines.push((lbi.element, b));
            } else if lbi.ty == LbiType::type_inline_end
                && let Some((element, b)) = inlines.pop()
            {
                self.add_inline_box(element, b);
            }
        }

        let mut ret_items: Vec<LineBoxItem> = Vec::with_capacity(inlines.len() + moved_items.len());
        for &(element, b) in inlines.iter().rev() {
            self.add_inline_box(element, b);
        }
        for &(element, _) in &inlines {
            ret_items.push(self.lbi_continue(element));
        }
        ret_items.extend(moved_items);
        ret_items
    }

    /// `get_first_text_part()`.
    pub fn line_box_first_text_part(&self, lb: &LineBox) -> Option<RenderId> {
        lb.items.iter().find(|i| i.ty == LbiType::type_text_part).map(|i| i.element)
    }

    /// `get_last_text_part()`.
    pub fn line_box_last_text_part(&self, lb: &LineBox) -> Option<RenderId> {
        lb.items.iter().rev().find(|i| i.ty == LbiType::type_text_part).map(|i| i.element)
    }

    /// `can_hold(item, ws, trailing)`.
    pub fn line_box_can_hold(&self, lb: &LineBox, item: &LineBoxItem, ws: i32, trailing: Px) -> bool {
        let src = self.lbi_src(item);
        if !self.is_inline(src) {
            return false;
        }

        if item.ty == LbiType::type_text_part {
            // force new line on floats clearing
            if self.is_break(src) && self.lbi_css(item).m_clear != ElementClear::clear_none.index() {
                return false;
            }

            let Some(last_el) = self.line_box_last_text_part(lb) else {
                // the first word is always can be hold
                return true;
            };

            // force new line if the last placed element was line break
            // Skip If the break item is float clearing
            let last_src = self.render.item(last_el).element;
            if self.is_break(last_src) && self.el(last_src).css.m_clear == ElementClear::clear_none.index() {
                return false;
            }

            // line break should stay in current line box
            if self.is_break(src) {
                return true;
            }

            if ws == white_space_nowrap || ws == white_space_pre || (ws == white_space_pre_wrap && self.is_space(src)) {
                return true;
            }

            if lb.left + lb.width + self.lbi_width(item) + trailing > lb.right {
                return false;
            }
        }

        true
    }

    /// `have_last_space()`.
    fn line_box_have_last_space(&self, lb: &LineBox) -> bool {
        match self.line_box_last_text_part(lb) {
            Some(last) => {
                let src = self.render.item(last).element;
                self.is_white_space(src) || self.is_break(src)
            }
            None => false,
        }
    }

    /// `is_empty()`: nothing but skipped text parts (a break counts), or only a break that clears.
    pub fn line_box_is_empty(&self, lb: &LineBox) -> bool {
        if lb.items.is_empty() {
            return true;
        }
        if lb.items.len() == 1 {
            let src = self.lbi_src(&lb.items[0]);
            if self.is_break(src) && self.el(src).css.m_clear != ElementClear::clear_none.index() {
                return true;
            }
        }
        for el in &lb.items {
            if el.ty == LbiType::type_text_part && (!self.render.item(el.element).skip || self.is_break(self.lbi_src(el))) {
                return false;
            }
        }
        true
    }

    /// `y_shift(shift)`.
    pub fn line_box_y_shift(&mut self, lb: &mut LineBox, shift: Px) {
        lb.top += shift;
        for el in &lb.items {
            self.lbi_y_shift(el, shift);
        }
    }

    /// `is_break_only()`: a break and skipped text parts only.
    fn line_box_is_break_only(&self, lb: &LineBox) -> bool {
        if lb.items.is_empty() {
            return false;
        }

        let mut break_found = false;

        for item in lb.items.iter().rev() {
            if item.ty == LbiType::type_text_part {
                if self.is_break(self.lbi_src(item)) {
                    break_found = true;
                } else if !self.render.item(item.element).skip {
                    return false;
                }
            }
        }
        break_found
    }

    /// `new_width(left, right)`: the line moved to its new sides, the items after the first that no longer fit given
    /// back. (On an empty line the C++ steps past `end()`; with the standard libraries' circular lists that is
    /// `end()` again, and nothing happens, as here.)
    pub fn line_box_new_width(&mut self, lb: &mut LineBox, left: Px, right: Px) -> Vec<LineBoxItem> {
        let add = left - lb.left;
        if add != Px(0.0) {
            lb.left = left;
            lb.right = right;
            lb.width = Px(0.0);
            let mut remove_begin = lb.items.len();
            let mut i = 1;
            while i < lb.items.len() {
                let mut item = lb.items[i];
                if !self.render.item(item.element).skip {
                    if lb.left + lb.width + self.lbi_width(&item) > lb.right {
                        remove_begin = i;
                        break;
                    }
                    self.lbi_pos_mut(&mut item).x += add;
                    lb.width += self.render.item(item.element).width();
                    lb.items[i] = item;
                }
                i += 1;
            }
            return lb.items.split_off(remove_begin);
        }
        Vec::new()
    }

    /// `reorder_bidi()`: a right-to-left line mirrored, then each run of the other direction turned back (a word's
    /// direction is its first strong character's; neutrals and boxes take the direction around them).
    fn line_box_reorder_bidi(&mut self, lb: &mut LineBox) {
        let direction_of = |doc: &Document, lbi: &LineBoxItem| -> i32 {
            if lbi.ty != LbiType::type_text_part {
                return 0;
            }
            let src = doc.lbi_src(lbi);
            if doc.is_text(src) { crate::css_properties::strong_direction(&doc.el(src).text) } else { 0 }
        };
        // Most lines are left to right through and through: they are left as they are, without allocating.
        if !lb.rtl && !lb.items.iter().any(|lbi| direction_of(self, lbi) > 0) {
            return;
        }
        let dirs: Vec<i32> = lb.items.iter().map(|lbi| direction_of(self, lbi)).collect();
        let mirror = |doc: &mut Document, item: &mut LineBoxItem, from: Px, to: Px| {
            let shift = from + to - doc.lbi_right(item) - doc.lbi_left(item);
            doc.lbi_pos_mut(item).x += shift;
        };
        if lb.rtl {
            let (left, right) = (lb.left, lb.right);
            for k in 0..lb.items.len() {
                let mut item = lb.items[k];
                mirror(self, &mut item, left, right);
                lb.items[k] = item;
            }
        }
        // Runs of the other direction read the other way: neutrals inside a run go with it, at its ends with the line.
        let other = if lb.rtl { -1 } else { 1 };
        let mut i = 0;
        while i < lb.items.len() {
            if dirs[i] != other {
                i += 1;
                continue;
            }
            let mut last = i;
            let mut j = i;
            while j < lb.items.len() && dirs[j] != -other {
                if dirs[j] == other {
                    last = j;
                }
                j += 1;
            }
            // The run takes the edges of the inline boxes it opens and closes, which keep their words.
            let mut first = i;
            while first > 0 && matches!(lb.items[first - 1].ty, LbiType::type_inline_start | LbiType::type_inline_continue) {
                first -= 1;
            }
            while last + 1 < lb.items.len() && lb.items[last + 1].ty == LbiType::type_inline_end {
                last += 1;
            }
            let mut from = self.lbi_left(&lb.items[first]);
            let mut to = self.lbi_right(&lb.items[first]);
            for k in first..=last {
                from = from.min(self.lbi_left(&lb.items[k]));
                to = to.max(self.lbi_right(&lb.items[k]));
            }
            for k in first..=last {
                let mut item = lb.items[k];
                mirror(self, &mut item, from, to);
                lb.items[k] = item;
            }
            i = last + 1;
        }
    }

    /// `add_inline_box(box)`: an inline's (and a text's) boxes; the base class keeps none.
    pub fn add_inline_box(&mut self, id: RenderId, b: Position) {
        let item = self.render.item_mut(id);
        if matches!(item.kind, RenderKind::render_item_inline | RenderKind::render_text) {
            item.inline_boxes.push(b);
        }
    }

    /// `clear_inline_boxes()`.
    pub fn clear_inline_boxes(&mut self, id: RenderId) {
        let item = self.render.item_mut(id);
        if matches!(item.kind, RenderKind::render_item_inline | RenderKind::render_text) {
            item.inline_boxes.clear();
        }
    }
}
