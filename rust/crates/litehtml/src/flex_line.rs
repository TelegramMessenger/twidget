//! `flex_line.h` and `flex_line.cpp`: a flex line (stage L10): its items, its main and cross sizes and baselines;
//! the free space distributed by the flex factors as css-flexbox-1 §9.7 resolves flexible lengths (grow or shrink,
//! frozen items, min and max violations), the items laid out at their main sizes to find the line's cross size and
//! baselines, and the items placed by `justify-content` and `auto` margins.

use crate::Px;
use crate::css_values::*;
use crate::document::{Document, StyleCx};
use crate::flex_item::{FlexClampState, FlexItem, flex_align_items_last};
use crate::formatting_context::FormattingContext;
use crate::types::*;

const flex_align_items_baseline: i32 = FlexAlignItems::flex_align_items_baseline.index();
const flex_justify_content_flex_start: i32 = FlexJustifyContent::flex_justify_content_flex_start.index();
const flex_justify_content_flex_end: i32 = FlexJustifyContent::flex_justify_content_flex_end.index();
const flex_justify_content_center: i32 = FlexJustifyContent::flex_justify_content_center.index();
const flex_justify_content_space_between: i32 = FlexJustifyContent::flex_justify_content_space_between.index();
const flex_justify_content_space_around: i32 = FlexJustifyContent::flex_justify_content_space_around.index();
const flex_justify_content_start: i32 = FlexJustifyContent::flex_justify_content_start.index();
const flex_justify_content_end: i32 = FlexJustifyContent::flex_justify_content_end.index();
const flex_justify_content_left: i32 = FlexJustifyContent::flex_justify_content_left.index();
const flex_justify_content_right: i32 = FlexJustifyContent::flex_justify_content_right.index();
const flex_justify_content_space_evenly: i32 = FlexJustifyContent::flex_justify_content_space_evenly.index();

/// `flex_line`.
#[derive(Clone, Debug)]
pub struct FlexLine {
    pub items: Vec<FlexItem>,
    /// For a row the line's top, for a column its left.
    pub cross_start: Px,
    /// The items' main sizes added up (at first their hypothetical main sizes).
    pub main_size: Px,
    pub cross_size: Px,
    pub base_size: Px,
    /// Items with an `auto` margin at the main start (left or top), and at the main end.
    pub num_auto_margin_main_start: i32,
    pub num_auto_margin_main_end: i32,
    pub first_baseline: Baseline,
    pub last_baseline: Baseline,
    pub reverse_main: bool,
    pub reverse_cross: bool,
    pub main_gap: Px,
}

impl FlexLine {
    /// `flex_line(reverse_main, reverse_cross)`.
    pub fn new(reverse_main: bool, reverse_cross: bool) -> FlexLine {
        FlexLine {
            items: Vec::new(),
            cross_start: Px(0.0),
            main_size: Px(0.0),
            cross_size: Px(0.0),
            base_size: Px(0.0),
            num_auto_margin_main_start: 0,
            num_auto_margin_main_end: 0,
            first_baseline: Baseline::default(),
            last_baseline: Baseline::default(),
            reverse_main,
            reverse_cross,
            main_gap: Px(0.0),
        }
    }

    /// `gaps()`: the gaps between the items.
    pub fn gaps(&self) -> Px {
        if self.items.len() > 1 { self.main_gap * Px::from_int((self.items.len() - 1) as i32) } else { Px(0.0) }
    }

    /// `distribute_free_space(container_main_size)`: grow when the items' hypothetical sizes leave room, else shrink.
    fn distribute_free_space(&mut self, container_main_size: Px) {
        // 1 Determine the used flex factor. Sum the outer hypothetical main sizes of all items on the line.
        // If the sum is less than the flex container's inner main size, use the flex grow factor for the
        // rest of this algorithm; otherwise, use the flex shrink factor.
        if self.main_size - self.gaps() < container_main_size {
            self.distribute_free_space_grow(container_main_size);
        } else {
            self.distribute_free_space_shrink(container_main_size);
        }
    }

    /// `distribute_free_space_grow(container_main_size)`.
    fn distribute_free_space_grow(&mut self, container_main_size: Px) {
        let mut initial_free_space = container_main_size;
        let mut all_inflexible = true;

        for item in self.items.iter_mut() {
            // 2. Size inflexible items: a zero factor, or a base size greater than the hypothetical main size.
            // 3. Calculate initial free space: frozen items by their target, others by their base size.
            if item.grow == 0 || item.base_size > item.main_size {
                item.frozen = true;
                item.clamp_state = FlexClampState::flex_clamp_state_inflexible;
                initial_free_space -= item.main_size;
            } else {
                initial_free_space -= item.base_size;
                all_inflexible = false;
            }
        }

        // 4.a If all the flex items on the line are frozen, free space has been distributed.
        if all_inflexible {
            return;
        }

        loop {
            // 4.b The remaining free space; scaled down when the unfrozen factors add up to less than one.
            let mut sum_flex_grow_factor: i64 = 0;
            let mut remaining_free_space = container_main_size;

            for item in &self.items {
                if item.frozen {
                    remaining_free_space -= item.main_size;
                } else {
                    remaining_free_space -= item.base_size;
                    sum_flex_grow_factor += i64::from(item.grow);
                }
            }

            if sum_flex_grow_factor < 1000 {
                let adjusted_free_space = initial_free_space * Px(sum_flex_grow_factor as f32) / Px(1000.0);
                if adjusted_free_space < remaining_free_space {
                    remaining_free_space = adjusted_free_space;
                }
            }

            // 4.c Distribute free space proportional to the flex factors.
            if remaining_free_space != Px(0.0) {
                for item in self.items.iter_mut() {
                    if !item.frozen {
                        item.main_size = item.base_size + remaining_free_space * Px::from_int(item.grow) / Px(sum_flex_grow_factor as f32);
                    }
                }
            }

            if self.fix_min_max_violations() {
                break;
            }
        }
    }

    /// `distribute_free_space_shrink(container_main_size)`.
    fn distribute_free_space_shrink(&mut self, container_main_size: Px) {
        let mut initial_free_space = container_main_size;
        let mut all_inflexible = true;

        for item in self.items.iter_mut() {
            // 2. Size inflexible items: a zero factor, or a base size smaller than the hypothetical main size.
            // 3. Calculate initial free space.
            if item.shrink == 0 || item.base_size < item.main_size {
                item.frozen = true;
                item.clamp_state = FlexClampState::flex_clamp_state_inflexible;
                initial_free_space -= item.main_size;
            } else {
                initial_free_space -= item.base_size;
                all_inflexible = false;
            }
        }

        // 4.a
        if all_inflexible {
            return;
        }

        loop {
            // 4.b
            let mut sum_flex_shrink_factor: i64 = 0;
            let mut sum_scaled_flex_shrink_factor = Px(0.0);
            let mut remaining_free_space = container_main_size;

            for item in &self.items {
                if item.frozen {
                    remaining_free_space -= item.main_size;
                } else {
                    remaining_free_space -= item.base_size;
                    sum_flex_shrink_factor += i64::from(item.shrink);
                    sum_scaled_flex_shrink_factor += item.scaled_flex_shrink_factor;
                }
            }

            if sum_flex_shrink_factor < 1000 {
                let adjusted_free_space = initial_free_space * Px(sum_flex_shrink_factor as f32) / Px(1000.0);
                if adjusted_free_space > remaining_free_space {
                    remaining_free_space = adjusted_free_space;
                }
            }

            // 4.c Each unfrozen item's base size less its scaled shrink factor's share of the remaining space.
            if remaining_free_space != Px(0.0) {
                for item in self.items.iter_mut() {
                    if !item.frozen {
                        item.main_size = if sum_scaled_flex_shrink_factor > Px(0.0) {
                            item.base_size + remaining_free_space * item.scaled_flex_shrink_factor / sum_scaled_flex_shrink_factor
                        } else {
                            item.base_size
                        };
                    }
                }
            }

            if self.fix_min_max_violations() {
                break;
            }
        }
    }

    /// `fix_min_max_violations()`: 4.d the unfrozen items clamped by their minimum and maximum, 4.e then frozen: all
    /// when the clamps add up to nothing (done), else those clamped the way the total goes.
    fn fix_min_max_violations(&mut self) -> bool {
        let mut total_violation = Px(0.0);

        for item in self.items.iter_mut() {
            if !item.frozen {
                if item.main_size < item.min_size {
                    total_violation += item.min_size - item.main_size;
                    item.main_size = item.min_size;
                    item.clamp_state = FlexClampState::flex_clamp_state_min_violation;
                } else if !item.max_size.is_default() && item.main_size > item.max_size.get() {
                    total_violation += item.max_size.get() - item.main_size;
                    item.main_size = item.max_size.get();
                    item.clamp_state = FlexClampState::flex_clamp_state_max_violation;
                }
            }
        }

        if total_violation == Px(0.0) {
            return true;
        }

        let mut all_frozen = true;
        let state_to_freeze = if total_violation > Px(0.0) {
            FlexClampState::flex_clamp_state_min_violation
        } else {
            FlexClampState::flex_clamp_state_max_violation
        };

        for item in self.items.iter_mut() {
            if !item.frozen {
                if item.clamp_state == state_to_freeze {
                    item.frozen = true;
                } else {
                    all_frozen = false;
                    item.clamp_state = FlexClampState::flex_clamp_state_unclamped;
                }
            }
        }

        all_frozen
    }

    /// `distribute_main_auto_margins(free_main_size)`: positive free space shared by the main `auto` margins.
    pub fn distribute_main_auto_margins(&mut self, free_main_size: Px) -> bool {
        if free_main_size > Px(0.0) && (self.num_auto_margin_main_start != 0 || self.num_auto_margin_main_end != 0) {
            let mut count = 0;
            for item in &self.items {
                if !item.auto_margin_main_start.is_default() {
                    count += 1;
                }
                if !item.auto_margin_main_end.is_default() {
                    count += 1;
                }
            }
            if count == 0 {
                return false;
            }
            let add = free_main_size / count;
            for item in self.items.iter_mut() {
                if !item.auto_margin_main_start.is_default() {
                    item.auto_margin_main_start.set(add);
                    item.main_size += add;
                    self.main_size += add;
                }
                if !item.auto_margin_main_end.is_default() {
                    item.auto_margin_main_end.set(add);
                    item.main_size += add;
                    self.main_size += add;
                }
            }
            return true;
        }
        false
    }
}

impl Document {
    /// `flex_line::init(container_main_size, fit_container, is_row_direction, self_size, fmt_ctx)`: the free space
    /// distributed (unless the container fits its content), then every item laid out at its main size: a row's at an
    /// exact width, the line as high as its baseline-aligned items' extent or its highest other item (no higher than
    /// the container's height or, past it, max-height); a column's at its own width (its CSS width, min and max
    /// applied) and an exact height, the line as wide as its widest item.
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn flex_line_init(
        &mut self,
        cx: &StyleCx,
        ln: &mut FlexLine,
        container_main_size: Px,
        fit_container: bool,
        is_row_direction: bool,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) {
        if !fit_container {
            let free = container_main_size - ln.gaps();
            ln.distribute_free_space(free);
        }

        ln.cross_size = Px(0.0);
        ln.main_size = Px(0.0);
        ln.first_baseline.set(Px(0.0), BaselineType::baseline_type_none);
        ln.last_baseline.set(Px(0.0), BaselineType::baseline_type_none);

        if is_row_direction {
            self.flex_line_init_row(cx, ln, self_size, fmt_ctx);
        } else {
            self.flex_line_init_column(cx, ln, self_size, fmt_ctx);
        }
    }

    fn flex_line_init_row(&mut self, cx: &StyleCx, ln: &mut FlexLine, self_size: &ContainingBlockContext, fmt_ctx: &mut FormattingContext) {
        let mut first_baseline_top = DefValue::new(Px(0.0));
        let mut first_baseline_bottom = DefValue::new(Px(0.0));
        let mut last_baseline_top = DefValue::new(Px(0.0));
        let mut last_baseline_bottom = DefValue::new(Px(0.0));
        let mut non_baseline_height = Px(0.0);

        // Calculate maximum cross size
        let mut max_cross_size = DefValue::new(Px(0.0));
        if self_size.height.ty != CbcValueType::auto {
            max_cross_size.set(self_size.height.px());
        }
        if self_size.max_height.ty != CbcValueType::none {
            if max_cross_size.is_default() {
                max_cross_size.set(self_size.max_height.px());
            } else {
                max_cross_size.set(max_cross_size.get().max(self_size.max_height.px()));
            }
        }

        // Render items into new size, find the line's cross size and its first and last baselines
        for k in 0..ln.items.len() {
            let (el, main_size, align) = (ln.items[k].el, ln.items[k].main_size, ln.items[k].align);
            let cb = self_size.new_width(main_size - self.render_offset_width(el), size_mode_exact_width);
            self.render(cx, el, Px(0.0), Px(0.0), &cb, Some(&mut *fmt_ctx), false);

            if (align & 0xFF) == flex_align_items_baseline {
                if (align & flex_align_items_last) != 0 {
                    ln.last_baseline.ty =
                        if ln.reverse_cross { BaselineType::baseline_type_top } else { BaselineType::baseline_type_bottom };

                    let top = -self.get_last_baseline(el);
                    let bottom = top + self.render.item(el).height();

                    if last_baseline_top.is_default() {
                        last_baseline_top.set(top);
                    } else {
                        last_baseline_top.set(last_baseline_top.get().min(top));
                    }

                    if last_baseline_bottom.is_default() {
                        last_baseline_bottom.set(bottom);
                    } else {
                        last_baseline_bottom.set(last_baseline_bottom.get().max(bottom));
                    }
                } else {
                    ln.first_baseline.ty =
                        if ln.reverse_cross { BaselineType::baseline_type_bottom } else { BaselineType::baseline_type_top };
                    let top = -self.get_first_baseline(el);
                    let bottom = top + self.render.item(el).height();

                    if first_baseline_top.is_default() {
                        first_baseline_top.set(top);
                    } else {
                        first_baseline_top.set(first_baseline_top.get().min(top));
                    }

                    if first_baseline_bottom.is_default() {
                        first_baseline_bottom.set(bottom);
                    } else {
                        first_baseline_bottom.set(first_baseline_bottom.get().max(bottom));
                    }
                }
            } else {
                non_baseline_height = non_baseline_height.max(self.render.item(el).height());
            }
            ln.main_size += self.render.item(el).width();
        }

        ln.main_size += ln.gaps();
        ln.cross_size = (first_baseline_bottom.get() - first_baseline_top.get()).max(last_baseline_bottom.get() - last_baseline_top.get());
        ln.cross_size = ln.cross_size.max(non_baseline_height);
        if !max_cross_size.is_default() && ln.cross_size > max_cross_size.get() {
            ln.cross_size = max_cross_size.get();
        }

        ln.first_baseline.calc(first_baseline_top.get(), first_baseline_bottom.get());
        ln.last_baseline.calc(last_baseline_top.get(), last_baseline_bottom.get());
    }

    fn flex_line_init_column(
        &mut self,
        cx: &StyleCx,
        ln: &mut FlexLine,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) {
        // Calculate maximum cross size
        let mut max_cross_size = DefValue::new(Px(0.0));
        if self_size.width.ty != CbcValueType::auto {
            max_cross_size.set(self_size.width.px());
        }
        if self_size.max_width.ty != CbcValueType::none {
            if max_cross_size.is_default() {
                max_cross_size.set(self_size.max_width.px());
            } else {
                max_cross_size.set(max_cross_size.get().max(self_size.max_width.px()));
            }
        }

        let render_width = self_size.render_width.px();
        for k in 0..ln.items.len() {
            let (el, main_size) = (ln.items[k].el, ln.items[k].main_size);
            let mut el_ret_width = self.render(cx, el, Px(0.0), Px(0.0), self_size, Some(&mut *fmt_ctx), false).natural_width;
            let css = self.ri_css(el);
            if !css.m_css_width.is_predefined() {
                el_ret_width = self.render.item(el).width();
            }
            if !css.m_css_max_width.is_predefined() {
                el_ret_width = el_ret_width.min(css.m_css_max_width.calc_percent(render_width) + self.render_offset_width(el));
            }
            if !css.m_css_min_width.is_predefined() {
                el_ret_width = el_ret_width.max(css.m_css_min_width.calc_percent(render_width) + self.render_offset_width(el));
            }
            let cb = self_size.new_width_height(
                el_ret_width - self.render_offset_width(el),
                main_size - self.render_offset_height(el),
                size_mode_exact_width | size_mode_exact_height | size_mode_resolved_width,
            );
            self.render(cx, el, Px(0.0), Px(0.0), &cb, Some(&mut *fmt_ctx), false);
            ln.main_size += self.render.item(el).height();
            ln.cross_size = ln.cross_size.max(self.render.item(el).width());
        }
        ln.main_size += ln.gaps();
        if !max_cross_size.is_default() && ln.cross_size > max_cross_size.get() {
            ln.cross_size = max_cross_size.get();
        }
    }

    /// `flex_line::calculate_items_position(container_main_size, justify_content, is_row_direction, self_size,
    /// fmt_ctx)`: the free main space given to the `auto` margins, the rest by `justify-content` (falling back as
    /// the spec says, the remainder of a split handed out a pixel at a time), every item placed; the lowest item
    /// bottom.
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn flex_line_calculate_items_position(
        &mut self,
        cx: &StyleCx,
        ln: &mut FlexLine,
        container_main_size: Px,
        justify_content: i32,
        is_row_direction: bool,
        self_size: &ContainingBlockContext,
        fmt_ctx: &mut FormattingContext,
    ) -> Px {
        // Distribute main axis free space for auto-margins
        let mut free_main_size = container_main_size - ln.main_size;
        ln.distribute_main_auto_margins(free_main_size);
        free_main_size = container_main_size - ln.main_size;

        // Fix justify-content property
        let mut justify_content = justify_content;
        let items = ln.items.len();
        if justify_content == flex_justify_content_left || justify_content == flex_justify_content_right {
            if !is_row_direction {
                justify_content = flex_justify_content_start;
            }
        } else if justify_content == flex_justify_content_space_between {
            // A negative leftover or a single item: flex-start.
            if items == 1 || free_main_size < Px(0.0) {
                justify_content = flex_justify_content_flex_start;
            }
        } else if (justify_content == flex_justify_content_space_around || justify_content == flex_justify_content_space_evenly)
            && (items == 1 || free_main_size < Px(0.0))
        {
            // A negative leftover or a single item: center.
            justify_content = flex_justify_content_center;
        }

        // Distribute free main size using justify-content property
        let mut main_pos = Px(0.0);
        let mut add_before_item = Px(0.0);
        let mut add_after_item = Px(0.0);
        let mut item_remainder = Px(0.0);

        // find initial main position and spaces between items
        if justify_content == flex_justify_content_right {
            main_pos = free_main_size;
        } else if justify_content == flex_justify_content_left || justify_content == flex_justify_content_start {
            main_pos = Px(0.0);
        } else if justify_content == flex_justify_content_end {
            main_pos = free_main_size;
        } else if justify_content == flex_justify_content_flex_end {
            if !ln.reverse_main {
                main_pos = free_main_size;
            }
        } else if justify_content == flex_justify_content_center {
            main_pos = free_main_size / Px(2.0);
        } else if justify_content == flex_justify_content_space_between {
            let gaps = items.wrapping_sub(1) as i32;
            add_after_item = free_main_size / gaps;
            item_remainder = free_main_size - (add_after_item * gaps);
        } else if justify_content == flex_justify_content_space_around {
            let shares = items.wrapping_mul(2) as i32;
            add_before_item = free_main_size / shares;
            add_after_item = add_before_item;
            item_remainder = free_main_size - (add_after_item * shares);
        } else if justify_content == flex_justify_content_space_evenly {
            let shares = items.wrapping_add(1) as i32;
            add_before_item = free_main_size / shares;
            item_remainder = free_main_size - add_before_item * shares;
        } else if ln.reverse_main {
            main_pos = free_main_size;
        }

        // Place all items in main and cross positions
        let mut height = Px(0.0);

        let distribute_step = Px(1.0);
        for k in 0..ln.items.len() {
            main_pos += add_before_item;
            if add_before_item > Px(0.0) && item_remainder > Px(0.0) {
                main_pos += distribute_step;
                item_remainder -= distribute_step;
            }
            self.flex_item_place(cx, &ln.items[k], ln, main_pos, self_size, fmt_ctx);
            main_pos += self.flex_item_el_main_size(&ln.items[k]) + add_after_item + ln.main_gap;
            if add_after_item > Px(0.0) && item_remainder > Px(0.0) {
                main_pos += distribute_step;
                item_remainder -= distribute_step;
            }
            height = height.max(self.render.item(ln.items[k].el).bottom());
        }
        height
    }
}
