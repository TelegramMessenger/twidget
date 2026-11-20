//! `types.h`, as far as the ported stages need it.

use crate::Px;

/// `font_metrics`: a font's measures in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontMetrics {
    /// The size `create_font` was given.
    pub font_size: Px,
    pub height: Px,
    /// From the baseline to the top of a line of text.
    pub ascent: Px,
    /// From the baseline to the bottom of a line of text.
    pub descent: Px,
    /// The height of an `x`.
    pub x_height: Px,
    /// The width of a `0`.
    pub ch_width: Px,
    /// Whether spaces are drawn rather than skipped by their width.
    pub draw_spaces: bool,
    pub sub_shift: Px,
    pub super_shift: Px,
}

impl Default for FontMetrics {
    fn default() -> FontMetrics {
        FontMetrics {
            font_size: Px(0.0),
            height: Px(0.0),
            ascent: Px(0.0),
            descent: Px(0.0),
            x_height: Px(0.0),
            ch_width: Px(0.0),
            draw_spaces: true,
            sub_shift: Px(0.0),
            super_shift: Px(0.0),
        }
    }
}

impl FontMetrics {
    /// `base_line()`.
    pub fn base_line(&self) -> Px {
        self.descent
    }
}

/// `media_features`: what media queries are evaluated against.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MediaFeatures {
    /// A `media_type` value (`css_values::MediaType`; 4 and past it are the deprecated types).
    pub ty: i32,
    /// The viewport's width, a rendered scroll bar included.
    pub width: Px,
    pub height: Px,
    /// The rendering surface's width.
    pub device_width: Px,
    pub device_height: Px,
    /// Bits per colour component; 0 for no colour device.
    pub color: i32,
    /// Entries in the colour lookup table; 0 for none.
    pub color_index: i32,
    /// Bits per pixel of a monochrome frame buffer; 0 for no monochrome device.
    pub monochrome: i32,
    /// In dpi.
    pub resolution: Px,
}

/// `contain_flags`: what `contain` declares, as flags.
pub const contain_size: i32 = 0x01;
pub const contain_inline_size: i32 = 0x02;
pub const contain_layout: i32 = 0x04;
pub const contain_style: i32 = 0x08;
pub const contain_paint: i32 = 0x10;

/// `position`: a box, its corner and its size in pixels.
#[derive(Clone, Copy, Debug, Default)]
pub struct Position {
    pub x: Px,
    pub y: Px,
    pub width: Px,
    pub height: Px,
}

impl Position {
    /// `position(x, y, width, height)`.
    pub const fn new(x: Px, y: Px, width: Px, height: Px) -> Position {
        Position { x, y, width, height }
    }

    /// `right()`.
    pub fn right(&self) -> Px {
        self.x + self.width
    }

    /// `bottom()`.
    pub fn bottom(&self) -> Px {
        self.y + self.height
    }

    /// `left()`.
    pub fn left(&self) -> Px {
        self.x
    }

    /// `top()`.
    pub fn top(&self) -> Px {
        self.y
    }
}

/// `size`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Size {
    pub width: Px,
    pub height: Px,
}

/// `margins`: four sides in pixels (a box's margins, padding or border widths).
#[derive(Clone, Copy, Debug, Default)]
pub struct Margins {
    pub left: Px,
    pub right: Px,
    pub top: Px,
    pub bottom: Px,
}

impl Margins {
    /// `width()`.
    pub fn width(&self) -> Px {
        self.left + self.right
    }

    /// `height()`.
    pub fn height(&self) -> Px {
        self.top + self.bottom
    }
}
