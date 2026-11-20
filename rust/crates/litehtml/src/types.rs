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

    /// `operator+=(margins)`: grown by the margins on every side.
    pub fn grow(&mut self, mg: &Margins) {
        self.x -= mg.left;
        self.y -= mg.top;
        self.width += mg.left + mg.right;
        self.height += mg.top + mg.bottom;
    }

    /// `operator-=(margins)`: shrunk by the margins on every side.
    pub fn shrink(&mut self, mg: &Margins) {
        self.x += mg.left;
        self.y += mg.top;
        self.width -= mg.left + mg.right;
        self.height -= mg.top + mg.bottom;
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        *self = Position::default();
    }

    /// `operator=(size)`: the size taken, the corner kept.
    pub fn set_size(&mut self, sz: Size) {
        self.width = sz.width;
        self.height = sz.height;
    }

    /// `move_to(x, y)`.
    pub fn move_to(&mut self, x: Px, y: Px) {
        self.x = x;
        self.y = y;
    }

    /// `operator==`: every field alike within `pixel_t`'s band.
    pub fn same(&self, val: &Position) -> bool {
        self.x == val.x && self.y == val.y && self.width == val.width && self.height == val.height
    }

    /// `does_intersect(val, can_touch)`: with `None` (a null pointer), always.
    pub fn does_intersect(&self, val: Option<&Position>, can_touch: bool) -> bool {
        let Some(val) = val else { return true };
        if !can_touch {
            return (self.left() <= val.right() && self.right() >= val.left() && self.bottom() >= val.top() && self.top() <= val.bottom())
                || (val.left() <= self.right() && val.right() >= self.left() && val.bottom() >= self.top() && val.top() <= self.bottom());
        }
        (self.left() < val.right() && self.right() > val.left() && self.bottom() > val.top() && self.top() < val.bottom())
            || (val.left() < self.right() && val.right() > self.left() && val.bottom() > self.top() && val.top() < self.bottom())
    }

    /// `on_same_line(val, can_touch)`.
    pub fn on_same_line(&self, val: &Position, can_touch: bool) -> bool {
        if can_touch {
            return !(self.bottom() <= val.top() || self.top() >= val.bottom());
        }
        !(self.bottom() < val.top() || self.top() > val.bottom())
    }

    /// `is_point_inside(x, y)`.
    pub fn is_point_inside(&self, x: Px, y: Px) -> bool {
        x >= self.left() && x < self.right() && y >= self.top() && y < self.bottom()
    }
}

/// `size`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Size {
    pub width: Px,
    pub height: Px,
}

impl Size {
    /// `size(w, h)`.
    pub const fn new(width: Px, height: Px) -> Size {
        Size { width, height }
    }
}

/// `rendered_width`: the widths a layout reports.
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderedWidth {
    /// The width the box takes without wrapping more than it must.
    pub natural_width: Px,
    /// The narrowest the box can be.
    pub min_width: Px,
}

impl RenderedWidth {
    /// `rendered_width{natural, min}`.
    pub const fn new(natural_width: Px, min_width: Px) -> RenderedWidth {
        RenderedWidth { natural_width, min_width }
    }

    /// `merge(other)`: the larger of each, by `std::max`.
    pub fn merge(&mut self, other: &RenderedWidth) {
        self.natural_width = self.natural_width.max(other.natural_width);
        self.min_width = self.min_width.max(other.min_width);
    }

    /// `reset()`.
    pub fn reset(&mut self) {
        *self = RenderedWidth::default();
    }
}

/// `containing_block_context::cbc_value_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CbcValueType {
    /// The size is an absolute value.
    absolute,
    /// The size is a percentage.
    percentage,
    /// The size is `auto`.
    auto,
    /// A min or max size is `none`.
    none,
}

/// `containing_block_context::typed_pixel`: a value and what kind it is. Assigning a `pixel_t` (`set`) changes
/// the value and keeps the kind.
#[derive(Clone, Copy, Debug)]
pub struct TypedPixel {
    pub value: Px,
    pub ty: CbcValueType,
}

impl TypedPixel {
    pub const fn new(value: Px, ty: CbcValueType) -> TypedPixel {
        TypedPixel { value, ty }
    }

    /// `operator pixel_t()`.
    pub fn px(&self) -> Px {
        self.value
    }

    /// `operator=(pixel_t)`.
    pub fn set(&mut self, value: Px) {
        self.value = value;
    }
}

/// `containing_block_context::cbc_size_mode` flags.
pub const size_mode_normal: u32 = 0x00;
pub const size_mode_exact_width: u32 = 0x01;
pub const size_mode_exact_height: u32 = 0x02;
pub const size_mode_content: u32 = 0x04;
pub const size_mode_resolved_width: u32 = 0x08;

/// `containing_block_context`: what a box is laid out in.
#[derive(Clone, Copy, Debug)]
pub struct ContainingBlockContext {
    pub width: TypedPixel,
    pub render_width: TypedPixel,
    pub min_width: TypedPixel,
    pub max_width: TypedPixel,
    pub height: TypedPixel,
    pub min_height: TypedPixel,
    pub max_height: TypedPixel,
    pub context_idx: i32,
    pub size_mode: u32,
}

impl Default for ContainingBlockContext {
    fn default() -> ContainingBlockContext {
        let auto = TypedPixel::new(Px(0.0), CbcValueType::auto);
        let none = TypedPixel::new(Px(0.0), CbcValueType::none);
        ContainingBlockContext {
            width: auto,
            render_width: auto,
            min_width: none,
            max_width: none,
            height: auto,
            min_height: none,
            max_height: none,
            context_idx: 0,
            size_mode: size_mode_normal,
        }
    }
}

impl ContainingBlockContext {
    /// `new_width(w, size_mode)`: the width (and the render width by as much) changed, the kinds kept.
    pub fn new_width(&self, w: Px, size_mode: u32) -> ContainingBlockContext {
        let mut ret = *self;
        ret.render_width.set(w - (ret.width.px() - ret.render_width.px()));
        ret.width.set(w);
        ret.size_mode = size_mode;
        ret
    }

    /// `new_width_height(w, h, size_mode)`.
    pub fn new_width_height(&self, w: Px, h: Px, size_mode: u32) -> ContainingBlockContext {
        let mut ret = *self;
        ret.render_width.set(w - (ret.width.px() - ret.render_width.px()));
        ret.width.set(w);
        ret.height.set(h);
        ret.size_mode = size_mode;
        ret
    }
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
