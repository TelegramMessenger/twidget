//! The paint values of `core/src/display_list.h`, `canvas_ops.h` and `backend.h`: what one paint of a
//! document consists of, what a canvas runs, and the metrics a font answers with. Members, their order
//! and their initial values are the C++ structs'.

use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color::rgba(0, 0, 0, 255);
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }
}

impl Default for Color {
    fn default() -> Color {
        Color::BLACK
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }
}

/// A 2D affine map, SVG order: x' = a·x + c·y + e, y' = b·x + d·y + f.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Default for Affine {
    fn default() -> Affine {
        Affine::IDENTITY
    }
}

impl Affine {
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    pub const fn translate(x: f32, y: f32) -> Affine {
        Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: x, f: y }
    }

    /// `self` after `next`: maps a point through `next` first.
    pub fn then(&self, next: &Affine) -> Affine {
        Affine {
            a: self.a * next.a + self.c * next.b,
            b: self.b * next.a + self.d * next.b,
            c: self.a * next.c + self.c * next.d,
            d: self.b * next.c + self.d * next.d,
            e: self.a * next.e + self.c * next.f + self.e,
            f: self.b * next.e + self.d * next.f + self.f,
        }
    }

    pub fn apply_x(&self, x: f32, y: f32) -> f32 {
        self.a * x + self.c * y + self.e
    }

    pub fn apply_y(&self, x: f32, y: f32) -> f32 {
        self.b * x + self.d * y + self.f
    }

    /// None when the map flattens the plane and has no inverse.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn invert(&self) -> Option<Affine> {
        let det = self.a * self.d - self.b * self.c;
        if !(det != 0.0) || !det.is_finite() {
            return None;
        }
        Some(Affine {
            a: self.d / det,
            b: -self.b / det,
            c: -self.c / det,
            d: self.a / det,
            e: (self.c * self.f - self.d * self.e) / det,
            f: (self.b * self.e - self.a * self.f) / det,
        })
    }
}

impl std::ops::Mul for Affine {
    type Output = Affine;

    fn mul(self, next: Affine) -> Affine {
        self.then(&next)
    }
}

/// Per-corner elliptical radii (CSS order, x then y). All zero = plain rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Radii {
    pub tlx: f32,
    pub tly: f32,
    pub trx: f32,
    pub try_: f32,
    pub brx: f32,
    pub bry: f32,
    pub blx: f32,
    pub bly: f32,
}

impl Radii {
    pub fn any(&self) -> bool {
        self.tlx > 0.0
            || self.tly > 0.0
            || self.trx > 0.0
            || self.try_ > 0.0
            || self.brx > 0.0
            || self.bry > 0.0
            || self.blx > 0.0
            || self.bly > 0.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stop {
    /// 0..1
    pub offset: f32,
    pub color: Color,
}

impl Stop {
    pub const fn new(offset: f32, color: Color) -> Stop {
        Stop { offset, color }
    }
}

/// Declares a fieldless `u8` enum with its values in order, `from_u8` for the decoder and `LAST`.
macro_rules! byte_enum {
    ($(#[$meta:meta])* $name:ident { $first:ident $(, $rest:ident)* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u8)]
        pub enum $name {
            #[default]
            $first,
            $($rest,)*
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$name::$first $(, $name::$rest)*];
            pub const LAST: $name = $name::ALL[$name::ALL.len() - 1];

            pub fn from_u8(value: u8) -> Option<$name> {
                $name::ALL.get(usize::from(value)).copied()
            }

            pub fn as_u8(self) -> u8 {
                self as u8
            }
        }
    };
}

byte_enum! {
    /// A CSS filter function (filter, backdrop-filter), as Filter Effects 1 defines it.
    FilterKind { Blur, Brightness, Contrast, Grayscale, HueRotate, Invert, Opacity, Saturate, Sepia, DropShadow }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Filter {
    pub kind: FilterKind,
    /// Blur and DropShadow: the blur radius in CSS px; HueRotate: degrees; the others: the factor.
    pub amount: f32,
    /// DropShadow: the offset.
    pub x: f32,
    pub y: f32,
    /// DropShadow.
    pub color: Color,
}

pub type Filters = Vec<Filter>;

byte_enum! {
    /// What a display list command paints; `core/src/display_list.h` documents the members each reads.
    Cmd {
        FillRect, StrokeRect, FillEllipse, StrokeEllipse, Text, Image, Linear, Radial, Conic, ClipPush, ClipPop,
        Path, GroupPush, GroupPop, Shadow, FilterPush, FilterPop, Backdrop,
    }
}

byte_enum! {
    PathOp { Move, Line, Quad, Cubic, Close }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub ops: Vec<PathOp>,
    /// 2 per Move/Line, 4 per Quad, 6 per Cubic.
    pub points: Vec<f32>,
    /// And evenodd: the fill rule of a clip path too.
    pub fill: bool,
    pub evenodd: bool,
    pub fill_color: Color,
    /// 0 none, 1 linear (x1, y1 → x2, y2), 2 radial (centre x1, y1, radius r, focus fx, fy).
    pub gradient: i32,
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub r: f32,
    pub fx: f32,
    pub fy: f32,
    pub stops: Vec<Stop>,
    pub stroke: bool,
    pub stroke_color: Color,
    pub stroke_width: f32,
    /// 0 butt, 1 round, 2 square.
    pub line_cap: i32,
    /// 0 miter, 1 round, 2 bevel.
    pub line_join: i32,
    pub miter_limit: f32,
    pub dashes: Vec<f32>,
    pub dash_offset: f32,
    /// As the fill's gradient, sg = x1, y1, x2, y2, r, fx, fy.
    pub stroke_gradient: i32,
    pub sg: [f32; 7],
    pub stroke_stops: Vec<Stop>,
}

impl Default for Path {
    fn default() -> Path {
        Path {
            ops: Vec::new(),
            points: Vec::new(),
            fill: false,
            evenodd: false,
            fill_color: Color::BLACK,
            gradient: 0,
            x1: 0.0,
            y1: 0.0,
            x2: 0.0,
            y2: 0.0,
            r: 0.0,
            fx: 0.0,
            fy: 0.0,
            stops: Vec::new(),
            stroke: false,
            stroke_color: Color::BLACK,
            stroke_width: 1.0,
            line_cap: 0,
            line_join: 0,
            miter_limit: 4.0,
            dashes: Vec::new(),
            dash_offset: 0.0,
            stroke_gradient: 0,
            sg: [0.0; 7],
            stroke_stops: Vec::new(),
        }
    }
}

pub const IDENTITY_MATRIX: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub kind: Cmd,
    pub rect: Rect,
    pub radii: Radii,
    pub clip: Rect,
    pub has_clip: bool,
    pub color: Color,
    pub width: f32,
    pub font: i32,
    /// Bytes as the C++ `std::string` holds them: not necessarily UTF-8.
    pub text: Vec<u8>,
    pub x: f32,
    pub y: f32,
    pub image: i32,
    /// Image: a canvas bitmap (Backend::canvas_run) instead of a decoded image.
    pub canvas: i32,
    pub ax: f32,
    pub ay: f32,
    pub bx: f32,
    pub by: f32,
    pub stops: Vec<Stop>,
    pub path: Option<Arc<Path>>,
    /// FilterPush, Backdrop.
    pub filter: Option<Arc<Filters>>,
    pub opacity: f32,
    /// a, b, c, d, tx, ty.
    pub matrix: [f32; 6],
    pub hidden: bool,
    pub inset: bool,
    pub key: u64,
}

impl Default for Command {
    fn default() -> Command {
        Command {
            kind: Cmd::FillRect,
            rect: Rect::default(),
            radii: Radii::default(),
            clip: Rect::default(),
            has_clip: false,
            color: Color::BLACK,
            width: 0.0,
            font: 0,
            text: Vec::new(),
            x: 0.0,
            y: 0.0,
            image: 0,
            canvas: 0,
            ax: 0.0,
            ay: 0.0,
            bx: 0.0,
            by: 0.0,
            stops: Vec::new(),
            path: None,
            filter: None,
            opacity: 1.0,
            matrix: IDENTITY_MATRIX,
            hidden: false,
            inset: false,
            key: 0,
        }
    }
}

pub type DisplayList = Vec<Command>;

/// A path as Move/Line/Quad/Cubic/Close with its points.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    pub ops: Vec<PathOp>,
    pub points: Vec<f32>,
}

impl Geometry {
    pub fn empty(&self) -> bool {
        self.ops.is_empty()
    }
}

byte_enum! {
    PaintKind { Solid, Linear, Radial, Conic }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CanvasPaint {
    pub kind: PaintKind,
    pub color: Color,
    /// Linear: x0, y0, x1, y1. Radial: x0, y0, r0, x1, y1, r1. Conic: start angle (radians), x, y. In user space.
    pub args: [f32; 6],
    /// Sorted by offset.
    pub stops: Vec<Stop>,
}

byte_enum! {
    /// globalCompositeOperation, in the spec's order.
    Composite {
        SourceOver, SourceIn, SourceOut, SourceAtop, DestinationOver, DestinationIn, DestinationOut, DestinationAtop,
        Lighter, Copy, Xor, Multiply, Screen, Overlay, Darken, Lighten, ColorDodge, ColorBurn, HardLight, SoftLight,
        Difference, Exclusion, Hue, Saturation, Color, Luminosity,
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CanvasClip {
    /// Canvas pixels.
    pub path: Geometry,
    pub evenodd: bool,
}

pub type Clips = Vec<CanvasClip>;
pub type Pixels = Vec<u8>;

byte_enum! {
    /// Reset, Fill, Stroke, Clear, Text, Image, PutImage: `core/src/canvas_ops.h` documents each.
    OpKind { Reset, Fill, Stroke, Clear, Text, Image, PutImage }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CanvasOp {
    pub kind: OpKind,
    pub path: Geometry,
    pub evenodd: bool,
    pub paint: CanvasPaint,
    /// User → canvas px: a, b, c, d, tx, ty.
    pub matrix: [f32; 6],
    pub line_width: f32,
    /// 0 butt, 1 round, 2 square.
    pub line_cap: i32,
    /// 0 miter, 1 round, 2 bevel.
    pub line_join: i32,
    pub miter_limit: f32,
    pub dashes: Vec<f32>,
    pub dash_offset: f32,
    pub alpha: f32,
    pub composite: Composite,
    pub shadow_color: Color,
    pub shadow_blur: f32,
    pub shadow_x: f32,
    pub shadow_y: f32,
    /// Intersected, in order; None = none.
    pub clips: Option<Arc<Clips>>,
    pub font: i32,
    pub text: Vec<u8>,
    pub x: f32,
    pub y: f32,
    pub scale_x: f32,
    /// textAlign offset, before scale_x.
    pub text_dx: f32,
    pub stroke_text: bool,
    /// A Backend::load_image id.
    pub image: i32,
    /// Or another canvas.
    pub canvas: i32,
    pub sx: f32,
    pub sy: f32,
    pub sw: f32,
    pub sh: f32,
    pub dx: f32,
    pub dy: f32,
    pub dw: f32,
    pub dh: f32,
    pub smoothing: bool,
    pub rgba: Option<Arc<Pixels>>,
    pub w: i32,
    pub h: i32,
}

impl Default for CanvasOp {
    fn default() -> CanvasOp {
        CanvasOp {
            kind: OpKind::Fill,
            path: Geometry::default(),
            evenodd: false,
            paint: CanvasPaint::default(),
            matrix: IDENTITY_MATRIX,
            line_width: 1.0,
            line_cap: 0,
            line_join: 0,
            miter_limit: 10.0,
            dashes: Vec::new(),
            dash_offset: 0.0,
            alpha: 1.0,
            composite: Composite::SourceOver,
            shadow_color: Color::TRANSPARENT,
            shadow_blur: 0.0,
            shadow_x: 0.0,
            shadow_y: 0.0,
            clips: None,
            font: 0,
            text: Vec::new(),
            x: 0.0,
            y: 0.0,
            scale_x: 1.0,
            text_dx: 0.0,
            stroke_text: false,
            image: 0,
            canvas: 0,
            sx: 0.0,
            sy: 0.0,
            sw: -1.0,
            sh: -1.0,
            dx: 0.0,
            dy: 0.0,
            dw: 0.0,
            dh: 0.0,
            smoothing: true,
            rgba: None,
            w: 0,
            h: 0,
        }
    }
}

impl CanvasOp {
    pub fn has_shadow(&self) -> bool {
        self.shadow_color.a > 0 && (self.shadow_blur > 0.0 || self.shadow_x != 0.0 || self.shadow_y != 0.0)
    }
}

pub type Batch = Vec<CanvasOp>;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FontMetrics {
    /// Baseline → top of the line box.
    pub ascent: f32,
    /// Baseline → bottom.
    pub descent: f32,
    /// Line height the font asks for (ascent + descent + leading).
    pub height: f32,
    pub x_height: f32,
    /// Advance of "0" (the `ch` unit).
    pub zero_width: f32,
    /// The em box canvas text is placed on; 0 and 0 when the backend cannot read it.
    pub em_ascent: f32,
    pub em_descent: f32,
}

/// Sets `m`'s em box from an ascent and a descent (any unit, the descent positive) for a font `size` px:
/// the ascent rounded to 1/64 px, the descent the rest. False, leaving it, when they make no box.
pub fn set_em_box(m: &mut FontMetrics, ascent: f32, descent: f32, size: f32) -> bool {
    let height = ascent + descent;
    if ascent < 0.0 || descent < 0.0 || height <= 0.0 || size <= 0.0 {
        return false;
    }
    m.em_ascent = (ascent * size / height * 64.0).round() / 64.0;
    m.em_descent = size - m.em_ascent;
    true
}
