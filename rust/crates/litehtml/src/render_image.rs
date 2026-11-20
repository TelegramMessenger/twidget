//! `render_image.h` and `render_image.cpp` (stage L7): a replaced box (`<img>`, `<svg>`) sized from its content
//! size, its CSS width and height, the other kept in proportion, `max-width` and `max-height` applied; the line
//! height it gets is its height.

use crate::Px;
use crate::css_values::CssUnits;
use crate::document::{Document, StyleCx};
use crate::render_item::*;
use crate::types::*;

impl Document {
    /// `render_item_image::_render(x, y, containing_block_size, fmt_ctx, second_pass)`.
    pub(crate) fn render_image_render(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        x: Px,
        y: Px,
        containing_block_size: &ContainingBlockContext,
    ) -> RenderedWidth {
        let parent_width = containing_block_size.width.px();
        let self_size = self.calculate_containing_block_context(cx, this, containing_block_size);

        self.calc_outlines(this, parent_width);

        self.render.item_mut(this).pos.move_to(x, y);

        let element = self.render.item(this).element;
        let sz = self.get_content_size(cx, element, containing_block_size.width.px());

        {
            let it = self.render.item_mut(this);
            it.pos.width = sz.width;
            it.pos.height = sz.height;
        }

        let height = self.render.item(this).height();
        self.el_mut(element).css.m_line_height.computed_value = height;

        let units = self.units(cx);
        let css = &self.el(element).css;
        let fm = css.m_font_metrics;
        let width_auto = css.m_css_width.is_predefined();
        let height_auto = css.m_css_height.is_predefined();
        let max_width_set = !css.m_css_max_width.is_predefined();
        let max_height_set = !css.m_css_max_height.is_predefined();
        let max_width_len = css.m_css_max_width.clone();
        let max_height_len = css.m_css_max_height.clone();
        let css_width = css.m_css_width.clone();
        let height_percent = css.m_css_height.units() == CssUnits::css_units_percentage;
        let max_width = || units.to_pixels(&max_width_len, &fm, parent_width);
        let calc_max_height = |image_height: Px, containing_block_height: Px| {
            units.to_pixels(&max_height_len, &fm, if containing_block_height == Px(0.0) { image_height } else { containing_block_height })
        };

        let it = self.render.item_mut(this);
        if height_auto && width_auto {
            it.pos.height = sz.height;
            it.pos.width = sz.width;

            // check for max-width
            if max_width_set {
                let max_width = max_width();
                if it.pos.width > max_width {
                    it.pos.width = max_width;
                }
                if sz.width != Px(0.0) {
                    it.pos.height = it.pos.width * sz.height / sz.width;
                } else {
                    it.pos.height = sz.height;
                }
            }

            // check for max-height
            if max_height_set {
                let max_height = calc_max_height(sz.height, containing_block_size.height.px());
                if it.pos.height > max_height {
                    it.pos.height = max_height;
                }
                if sz.height != Px(0.0) {
                    it.pos.width = it.pos.height * sz.width / sz.height;
                } else {
                    it.pos.width = sz.width;
                }
            }
        } else if !height_auto && width_auto {
            if self_size.height.ty != CbcValueType::auto && self_size.height.value > Px(0.0) {
                it.pos.height = self_size.height.px();
            }

            // check for max-height
            if max_height_set {
                let max_height = calc_max_height(sz.height, containing_block_size.height.px());
                if it.pos.height > max_height {
                    it.pos.height = max_height;
                }
            }

            if sz.height != Px(0.0) {
                it.pos.width = it.pos.height * sz.width / sz.height;
            } else {
                it.pos.width = sz.width;
            }
        } else if height_auto && !width_auto {
            it.pos.width = css_width.calc_percent(parent_width);

            // check for max-width
            if max_width_set {
                let max_width = max_width();
                if it.pos.width > max_width {
                    it.pos.width = max_width;
                }
            }

            if sz.width != Px(0.0) {
                it.pos.height = it.pos.width * sz.height / sz.width;
            } else {
                it.pos.height = sz.height;
            }
        } else {
            it.pos.width = css_width.calc_percent(parent_width);
            it.pos.height = Px(0.0);
            if self_size.height.ty != CbcValueType::auto && self_size.height.value > Px(0.0) {
                it.pos.height = self_size.height.px();
            } else if height_percent && sz.width != Px(0.0) {
                // A % height with no height to resolve against is auto: the ratio's.
                it.pos.height = it.pos.width * sz.height / sz.width;
            }

            // check for max-height
            if max_height_set {
                let max_height = calc_max_height(sz.height, containing_block_size.height.px());
                if it.pos.height > max_height {
                    it.pos.height = max_height;
                }
            }

            // check for max-height
            if max_width_set {
                let max_width = max_width();
                if it.pos.width > max_width {
                    it.pos.width = max_width;
                }
            }
        }

        it.pos.x += it.content_offset_left();
        it.pos.y += it.content_offset_top();

        let natural_width = it.pos.width + it.content_offset_left() + it.content_offset_right();
        RenderedWidth { natural_width, min_width: natural_width }
    }
}
