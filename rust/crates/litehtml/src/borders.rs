//! `borders.h`: what painting hands the container of a box's borders: `border` and `borders` (widths in pixels, a
//! style and a colour per side), `border_radiuses` (each corner's two radii in pixels, kept non-negative, made to
//! fit a box), and the computed style's `css_border_radius::calc_percents` and `css_borders::is_visible`, which
//! `css_properties.rs` keeps the types of.

use crate::Px;
use crate::css_properties::{CssBorder, CssBorderRadius, CssBorders};
use crate::css_values::BorderStyle;
use crate::types::Margins;
use crate::web_color::WebColor;

/// `border`: one side's width in pixels, its style (`border_style`) and colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub width: Px,
    pub style: i32,
    pub color: WebColor,
}

impl Default for Border {
    fn default() -> Border {
        Border { width: Px(0.0), style: BorderStyle::border_style_none.index(), color: WebColor::default() }
    }
}

impl From<&CssBorder> for Border {
    /// `border(const css_border&)`: the width's number as pixels.
    fn from(val: &CssBorder) -> Border {
        Border { width: Px(val.width.val()), style: val.style, color: val.color }
    }
}

/// `border_radiuses`: each corner's horizontal and vertical radius.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BorderRadiuses {
    pub top_left_x: Px,
    pub top_left_y: Px,
    pub top_right_x: Px,
    pub top_right_y: Px,
    pub bottom_right_x: Px,
    pub bottom_right_y: Px,
    pub bottom_left_x: Px,
    pub bottom_left_y: Px,
}

impl BorderRadiuses {
    /// `operator+=(margins)`: each radius grown by the sides it meets, then kept non-negative.
    pub fn grow(&mut self, mg: &Margins) {
        self.top_left_x += mg.left;
        self.top_left_y += mg.top;
        self.top_right_x += mg.right;
        self.top_right_y += mg.top;
        self.bottom_right_x += mg.right;
        self.bottom_right_y += mg.bottom;
        self.bottom_left_x += mg.left;
        self.bottom_left_y += mg.bottom;
        self.fix_values();
    }

    /// `operator-=(margins)`: each radius shrunk by the sides it meets, then kept non-negative.
    pub fn shrink(&mut self, mg: &Margins) {
        self.top_left_x -= mg.left;
        self.top_left_y -= mg.top;
        self.top_right_x -= mg.right;
        self.top_right_y -= mg.top;
        self.bottom_right_x -= mg.right;
        self.bottom_right_y -= mg.bottom;
        self.bottom_left_x -= mg.left;
        self.bottom_left_y -= mg.bottom;
        self.fix_values();
    }

    /// `fix_values()`: every radius at least 0 (`std::max` with the fuzzy comparison, so a NaN stays).
    pub fn fix_values(&mut self) {
        for r in [
            &mut self.top_left_x,
            &mut self.top_left_y,
            &mut self.top_right_x,
            &mut self.top_right_y,
            &mut self.bottom_right_x,
            &mut self.bottom_right_y,
            &mut self.bottom_left_x,
            &mut self.bottom_left_y,
        ] {
            *r = r.max(Px(0.0));
        }
    }

    /// `fix_values(width, height)`: non-negative, then each corner past half the box scaled down (both radii by the
    /// smaller of the two ratios, a zero radius giving an infinite or NaN ratio as the C++ divides).
    pub fn fix_values_in(&mut self, width: Px, height: Px) {
        self.fix_values();
        let half_width = width / Px(2.0);
        let half_height = height / Px(2.0);
        let fix_one = |radii_x: &mut Px, radii_y: &mut Px| {
            let factor = (half_width / *radii_x).min(half_height / *radii_y);
            *radii_x *= factor;
            *radii_y *= factor;
        };
        if self.top_left_x > half_width || self.top_left_y > half_height {
            fix_one(&mut self.top_left_x, &mut self.top_left_y);
        }
        if self.top_right_x > half_width || self.top_right_y > half_height {
            fix_one(&mut self.top_right_x, &mut self.top_right_y);
        }
        if self.bottom_right_x > half_width || self.bottom_right_y > half_height {
            fix_one(&mut self.bottom_right_x, &mut self.bottom_right_y);
        }
        if self.bottom_left_x > half_width || self.bottom_left_y > half_height {
            fix_one(&mut self.bottom_left_x, &mut self.bottom_left_y);
        }
    }
}

impl CssBorderRadius {
    /// `calc_percents(width, height)`: the radii in pixels against the box, made to fit it.
    pub fn calc_percents(&self, width: Px, height: Px) -> BorderRadiuses {
        let mut ret = BorderRadiuses {
            bottom_left_x: self.bottom_left_x.calc_percent(width),
            bottom_left_y: self.bottom_left_y.calc_percent(height),
            top_left_x: self.top_left_x.calc_percent(width),
            top_left_y: self.top_left_y.calc_percent(height),
            top_right_x: self.top_right_x.calc_percent(width),
            top_right_y: self.top_right_y.calc_percent(height),
            bottom_right_x: self.bottom_right_x.calc_percent(width),
            bottom_right_y: self.bottom_right_y.calc_percent(height),
        };
        ret.fix_values_in(width, height);
        ret
    }
}

impl CssBorders {
    /// `is_visible()`: a side's width is not 0 as pixels (whatever its style).
    pub fn is_visible(&self) -> bool {
        Px(self.left.width.val()) != Px(0.0)
            || Px(self.right.width.val()) != Px(0.0)
            || Px(self.top.width.val()) != Px(0.0)
            || Px(self.bottom.width.val()) != Px(0.0)
    }
}

/// `borders`: the four sides and the radii.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Borders {
    pub left: Border,
    pub top: Border,
    pub right: Border,
    pub bottom: Border,
    pub radius: BorderRadiuses,
}

impl From<&CssBorders> for Borders {
    /// `borders(const css_borders&)`: the sides, the radii left 0.
    fn from(val: &CssBorders) -> Borders {
        Borders {
            left: Border::from(&val.left),
            top: Border::from(&val.top),
            right: Border::from(&val.right),
            bottom: Border::from(&val.bottom),
            radius: BorderRadiuses::default(),
        }
    }
}

impl Borders {
    /// `is_visible()`: a side's width is not 0 (whatever its style).
    pub fn is_visible(&self) -> bool {
        self.left.width != Px(0.0) || self.right.width != Px(0.0) || self.top.width != Px(0.0) || self.bottom.width != Px(0.0)
    }
}
