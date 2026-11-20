//! `el_svg.cpp` and `el_svg.h`: an `<svg>` as a document is built (it marks its document and its `width` and
//! `height` are dimensions), and the geometry of what it draws: path data with its error handling, arcs as
//! cubics, the outlines of `rect`, `circle`, `ellipse`, `line`, `polyline` and `polygon`, the `transform`
//! attribute and CSS transforms on SVG elements (with `transform-box: fill-box`), the current transformation
//! matrix, the view box and `preserveAspectRatio`, the intrinsic size, a `<use>`'s placement with its caps,
//! the parents a `<use>` lends its target, a gradient's coordinates over a path's box, and path length and the
//! point at a length.
//!
//! What it reads of an element that the DOM does not hold (the cascade's own declarations and transforms,
//! layout's placement) comes through [`SvgElements`], which later stages implement on the document.
//!
//! Not here yet (the painting half of stage L14): `svg_painter`'s walk (groups, opacity, visibility, the group
//! cache and its keys, hit testing), `svg_property` and the paints read through it (`svg_paint`,
//! `svg_number`, `svg_ident`, a gradient's stops, dashes, caps), text runs (`svg_text_runs`, `svg_text_box`:
//! fonts and layout), and `el_svg::draw` with its paint cache and `<use>` budget per paint generation: painting
//! (stage L13) draws an `<svg>`'s box as a tag's and reaches what it draws inside as a seam.

use std::collections::BTreeMap;

use crate::css_effects::{PaintGroup, PathOp, SvgGradient, TransformFunction, TransformKind, TransformMatrix, VectorPath};
use crate::css_length::CssLength;
use crate::cxx;
use crate::document::{Document, Env};
use crate::element::ElementId;
use crate::html::lowcase;
use crate::string_id::{_height_, _transform_, _transform_origin_, _width_};
use crate::types::{Position, Size};
use crate::{Px, StringId};

impl Document {
    /// `el_svg::draw(hdc, x, y, clip, ri)`: the tag's drawing; then, unless tracing, where its box is not empty and
    /// the clip meets it, what it draws inside (stage L14: the seam, logged as unported).
    #[allow(clippy::too_many_arguments)]
    pub fn el_svg_draw(
        &self,
        cx: &crate::document::StyleCx,
        id: ElementId,
        hdc: u64,
        x: Px,
        y: Px,
        clip: Option<&Position>,
        ri: crate::render_item::RenderId,
    ) {
        self.html_tag_draw(cx, id, hdc, x, y, clip, ri);
        if self.paint.trace.is_some() {
            return;
        }
        let mut pos = self.render.item(ri).pos;
        pos.x += x;
        pos.y += y;
        if pos.width <= Px(0.0) || pos.height <= Px(0.0) || !pos.does_intersect(clip, false) {
            return;
        }
        cx.container.paint_unported(hdc, "svg");
    }

    /// `el_svg::parse_attributes()`: `html_tag`'s, then a non-empty `width` and `height`.
    pub fn el_svg_parse_attributes(&mut self, env: &Env, id: ElementId) {
        self.html_tag_parse_attributes(env, id);
        for name in [&b"width"[..], b"height"] {
            if let Some(value) = self.get_attr(id, name).map(<[u8]>::to_vec)
                && !value.is_empty()
            {
                self.map_to_dimension_property(id, if name == b"width" { _width_ } else { _height_ }, &value);
            }
        }
    }
}

/// What the SVG geometry reads of a document's elements: the tree and the attributes (the DOM's), the
/// cascade's own declarations and computed transform (stage L5), and where layout placed an `<svg>`.
pub trait SvgElements {
    type Node: Copy + Eq + std::fmt::Debug;
    /// `parent()`.
    fn parent(&self, el: Self::Node) -> Option<Self::Node>;
    /// `get_tagName()`: an `html_tag`'s tag name (lower case), `script` and `style` for those, `""` for the rest.
    fn tag_name(&self, el: Self::Node) -> Vec<u8>;
    /// `get_attr(name)`: an `html_tag`'s attribute by its lower-case name, as written; none for other classes.
    fn get_attr(&self, el: Self::Node, name: &[u8]) -> Option<&[u8]>;
    /// `dynamic_cast<html_tag*>` succeeds.
    fn is_html_tag(&self, el: Self::Node) -> bool;
    /// `dynamic_cast<el_svg*>` succeeds.
    fn is_svg(&self, el: Self::Node) -> bool;
    /// `get_document()->root()`.
    fn root(&self) -> Option<Self::Node>;
    /// `html_tag::select_one(selector)` of `root`: the text parsed as a selector in the document's mode, then
    /// the first element of `root`'s tree (itself first) it matches; none when it does not parse.
    fn select_one(&self, root: Self::Node, selector: &[u8]) -> Option<Self::Node>;
    /// `html_tag::has_own_property(name)`: the element's own style holds a value for it.
    fn has_own_property(&self, el: Self::Node, name: StringId) -> bool;
    /// `css().get_transform()`: the computed transform functions, their lengths in px but percentages.
    fn transform(&self, el: Self::Node) -> &[TransformFunction];
    /// `css().get_transform_origin_x()`.
    fn transform_origin_x(&self, el: Self::Node) -> &CssLength;
    /// `css().get_transform_origin_y()`.
    fn transform_origin_y(&self, el: Self::Node) -> &CssLength;
    /// `css().get_transform_fill_box()`: `transform-box` is `fill-box` or `stroke-box`.
    fn transform_fill_box(&self, el: Self::Node) -> bool;
    /// `get_placement()`: the element's content box in document coordinates.
    fn placement(&self, el: Self::Node) -> Position;
}

/// `pi`, el_svg.cpp's own: `3.14159265f`, which is the float nearest to π.
pub const pi: f32 = std::f32::consts::PI;

/// `svg_painter::kMaxUseDrawn`: the elements `<use>`s may draw in one pass; uses of uses multiply (ten per
/// level, eight levels deep, are 10^8), so past it the rest of what they name is left out.
pub const kMaxUseDrawn: i32 = 10000;

/// `svg_painter::kUseLookup`: what looking up a new reference costs inside a `<use>`.
pub const kUseLookup: usize = 16;

/// `use_budget`: what each pass starts with.
pub const use_budget: i32 = kMaxUseDrawn;

/// The control-point factor of a quarter ellipse as cubics.
const k: f32 = 0.552_284_8;

// ---- Numbers ---------------------------------------------------------------------------------------------------

/// The byte at `i` as the C++ reads a `const char*`: 0 at the end, and a NUL ends the text where it is.
fn ch(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// What a `std::string` made of a `const char*` holds: the text up to its first NUL.
fn c_str(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|c| *c == 0).unwrap_or(s.len())]
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// `skip_separators(s)`: whitespace (the C locale's) and commas.
fn skip_separators(s: &[u8], at: &mut usize) {
    while ch(s, *at) != 0 && (twidget_cstd::is_space(ch(s, *at)) || ch(s, *at) == b',') {
        *at += 1;
    }
}

/// `next_number(s, value)`: after separators, a sign, digits with at most one point (a digit somewhere), and
/// an exponent when digits follow its `e`, read by `strtof`. On failure `at` stays past the separators.
pub fn next_number(s: &[u8], at: &mut usize) -> Option<f32> {
    skip_separators(s, at);
    if ch(s, *at) == 0 {
        return None;
    }
    let start = *at;
    let mut p = start;
    if ch(s, p) == b'+' || ch(s, p) == b'-' {
        p += 1;
    }
    let mut digits = false;
    while ch(s, p).is_ascii_digit() {
        p += 1;
        digits = true;
    }
    if ch(s, p) == b'.' {
        p += 1;
        while ch(s, p).is_ascii_digit() {
            p += 1;
            digits = true;
        }
    }
    if !digits {
        return None;
    }
    if ch(s, p) == b'e' || ch(s, p) == b'E' {
        let mut e = p + 1;
        if ch(s, e) == b'+' || ch(s, e) == b'-' {
            e += 1;
        }
        if ch(s, e).is_ascii_digit() {
            p = e;
            while ch(s, p).is_ascii_digit() {
                p += 1;
            }
        }
    }
    let value = twidget_cstd::strtof(&s[start..p]).0;
    *at = p;
    Some(value)
}

/// `next_flag(s, flag)`: after separators, a `0` or a `1`.
pub fn next_flag(s: &[u8], at: &mut usize) -> Option<i32> {
    skip_separators(s, at);
    let c = ch(s, *at);
    if c == b'0' || c == b'1' {
        *at += 1;
        return Some(i32::from(c - b'0'));
    }
    None
}

/// `numbers(s)`: every number of a list, each one's unit letters and `%` skipped; none of a null text.
pub fn numbers(s: Option<&[u8]>) -> Vec<f32> {
    let mut out = Vec::new();
    let Some(s) = s else { return out };
    let mut at = 0;
    while let Some(v) = next_number(s, &mut at) {
        out.push(v);
        while ch(s, at) != 0 && ch(s, at).is_ascii_alphabetic() {
            at += 1;
        }
        if ch(s, at) == b'%' {
            at += 1;
        }
    }
    out
}

/// `attr_length(el, name, base, def)`: the attribute's first number, a percentage of `base` when a `%`
/// follows it; `def` when it is missing, empty or no number.
pub fn attr_length<T: SvgElements>(tree: &T, el: T::Node, name: &[u8], base: f32, def: f32) -> f32 {
    let Some(value) = tree.get_attr(el, name) else { return def };
    if ch(value, 0) == 0 {
        return def;
    }
    let mut p = 0;
    let Some(v) = next_number(value, &mut p) else { return def };
    if ch(value, p) == b'%' { v * base / 100.0 } else { v }
}

/// `gradient_coordinate(el, name, def)`: as [`attr_length`], a percentage a fraction.
pub fn gradient_coordinate<T: SvgElements>(tree: &T, el: T::Node, name: &[u8], def: f32) -> f32 {
    let Some(value) = tree.get_attr(el, name) else { return def };
    if ch(value, 0) == 0 {
        return def;
    }
    let mut p = 0;
    let Some(v) = next_number(value, &mut p) else { return def };
    if ch(value, p) == b'%' { v / 100.0 } else { v }
}

/// `svg_reference_id(id)`: an id a reference may name: 1 to 128 bytes, each an ASCII letter or digit, `-`,
/// `_` or past ASCII.
pub fn svg_reference_id(id: &[u8]) -> bool {
    if id.is_empty() || id.len() > 128 {
        return false;
    }
    id.iter().all(|&c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c >= 0x80)
}

// ---- Paths ----------------------------------------------------------------------------------------------------

/// How [`PathBuilder::parse`] stopped. The C++ returns nothing; what was built up to there is the path
/// either way (SVG 2, "Error handling in path data").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathEnd {
    /// The data ran out.
    End,
    /// The data did not start with a moveto: another command letter, or something else.
    NoMoveto,
    /// A command (upper case) whose numbers or flags were missing or malformed.
    BadArguments(u8),
    /// Something after a closepath that is no command: a closepath takes nothing and never repeats.
    AfterClose,
    /// A letter that is no command (upper case).
    UnknownCommand(u8),
}

/// How [`PathBuilder::arc`] drew an arc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArcEnd {
    /// A zero radius, or the end point the current one: a line.
    Line,
    /// Its centre or angles came out no number: a line.
    NotFinite,
    /// That many cubics, one per quarter turn or less.
    Cubics(i32),
}

/// `path_builder`: a path and the points its commands continue from (the current point, the subpath's start,
/// the last quad's and cubic's control points for the smooth forms, the last command).
#[derive(Clone, Debug, Default)]
pub struct PathBuilder {
    pub path: VectorPath,
    pub cx: f32,
    pub cy: f32,
    pub sx: f32,
    pub sy: f32,
    pub qx: f32,
    pub qy: f32,
    pub kx: f32,
    pub ky: f32,
    pub last: u8,
}

impl PathBuilder {
    pub fn new() -> PathBuilder {
        PathBuilder::default()
    }

    /// `add(op, pts)`.
    fn add(&mut self, op: PathOp, pts: &[f32]) {
        self.path.ops.push(op);
        self.path.points.extend_from_slice(pts);
    }

    /// `move(x, y)`: a new subpath.
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.add(PathOp::r#move, &[x, y]);
        self.cx = x;
        self.sx = x;
        self.cy = y;
        self.sy = y;
    }

    /// `line(x, y)`.
    pub fn line(&mut self, x: f32, y: f32) {
        self.add(PathOp::line, &[x, y]);
        self.cx = x;
        self.cy = y;
    }

    /// `cubic(x1, y1, x2, y2, x, y)`.
    pub fn cubic(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.add(PathOp::cubic, &[x1, y1, x2, y2, x, y]);
        self.kx = x2;
        self.ky = y2;
        self.cx = x;
        self.cy = y;
    }

    /// `quad(x1, y1, x, y)`.
    pub fn quad(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.add(PathOp::quad, &[x1, y1, x, y]);
        self.qx = x1;
        self.qy = y1;
        self.cx = x;
        self.cy = y;
    }

    /// `close()`: back to the subpath's start.
    pub fn close(&mut self) {
        self.add(PathOp::close, &[]);
        self.cx = self.sx;
        self.cy = self.sy;
    }

    /// `arc(rx, ry, rotation, large, sweep, x2, y2)`: an elliptical arc from the current point (SVG 1.1 F.6.5,
    /// radii scaled up when too small), as one cubic per quarter turn or less ending exactly at the end point.
    pub fn arc(&mut self, mut rx: f32, mut ry: f32, rotation: f32, large: i32, sweep: i32, x2: f32, y2: f32) -> ArcEnd {
        let (x1, y1) = (self.cx, self.cy);
        if rx == 0.0 || ry == 0.0 || (x1 == x2 && y1 == y2) {
            self.line(x2, y2);
            return ArcEnd::Line;
        }
        rx = rx.abs();
        ry = ry.abs();
        let phi = rotation * pi / 180.0;
        let (cphi, sphi) = (phi.cos(), phi.sin());
        let (dx2, dy2) = ((x1 - x2) / 2.0, (y1 - y2) / 2.0);
        let x1p = cphi * dx2 + sphi * dy2;
        let y1p = -sphi * dx2 + cphi * dy2;
        let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
        if lambda > 1.0 {
            rx *= lambda.sqrt();
            ry *= lambda.sqrt();
        }
        let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
        let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
        let mut coef = if den > 0.0 { cxx::max_f32(0.0, num / den).sqrt() } else { 0.0 };
        if large == sweep {
            coef = -coef;
        }
        let cxp = coef * rx * y1p / ry;
        let cyp = -coef * ry * x1p / rx;
        let ccx = cphi * cxp - sphi * cyp + (x1 + x2) / 2.0;
        let ccy = sphi * cxp + cphi * cyp + (y1 + y2) / 2.0;
        let angle = |ux: f32, uy: f32, vx: f32, vy: f32| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
        let theta1 = angle(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
        let mut dtheta = angle((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
        if !theta1.is_finite() || !dtheta.is_finite() {
            self.line(x2, y2);
            return ArcEnd::NotFinite;
        }
        if sweep == 0 && dtheta > 0.0 {
            dtheta -= 2.0 * pi;
        } else if sweep != 0 && dtheta < 0.0 {
            dtheta += 2.0 * pi;
        }
        // |dtheta| is at most 2π here: the count is 1 to 4.
        let segments = 1.max(cxx::f32_to_int((dtheta.abs() / (pi / 2.0) - 0.001).ceil()));
        let delta = dtheta / segments as f32;
        let t = 4.0f32 / 3.0 * (delta / 4.0).tan();
        let point = |a: f32| (ccx + rx * a.cos() * cphi - ry * a.sin() * sphi, ccy + rx * a.cos() * sphi + ry * a.sin() * cphi);
        let derivative = |a: f32| (-rx * a.sin() * cphi - ry * a.cos() * sphi, -rx * a.sin() * sphi + ry * a.cos() * cphi);
        for i in 0..segments {
            let a1 = theta1 + delta * i as f32;
            let a2 = a1 + delta;
            let (p1x, p1y) = point(a1);
            let (mut p2x, mut p2y) = point(a2);
            let (d1x, d1y) = derivative(a1);
            let (d2x, d2y) = derivative(a2);
            if i == segments - 1 {
                p2x = x2;
                p2y = y2;
            }
            self.cubic(p1x + t * d1x, p1y + t * d1y, p2x - t * d2x, p2y - t * d2y, p2x, p2y);
        }
        ArcEnd::Cubics(segments)
    }

    /// `ellipse(ex, ey, rx, ry)`: four cubics from the rightmost point, clockwise, closed.
    pub fn ellipse(&mut self, ex: f32, ey: f32, rx: f32, ry: f32) {
        self.move_to(ex + rx, ey);
        self.cubic(ex + rx, ey + ry * k, ex + rx * k, ey + ry, ex, ey + ry);
        self.cubic(ex - rx * k, ey + ry, ex - rx, ey + ry * k, ex - rx, ey);
        self.cubic(ex - rx, ey - ry * k, ex - rx * k, ey - ry, ex, ey - ry);
        self.cubic(ex + rx * k, ey - ry, ex + rx, ey - ry * k, ex + rx, ey);
        self.close();
    }

    /// `rect(x, y, w, h, rx, ry)`: the radii at most half the sides; square corners when either is not
    /// positive.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, rx: f32, ry: f32) {
        let rx = cxx::min_f32(rx, w / 2.0);
        let ry = cxx::min_f32(ry, h / 2.0);
        if rx <= 0.0 || ry <= 0.0 {
            self.move_to(x, y);
            self.line(x + w, y);
            self.line(x + w, y + h);
            self.line(x, y + h);
            self.close();
            return;
        }
        self.move_to(x + rx, y);
        self.line(x + w - rx, y);
        self.cubic(x + w - rx + rx * k, y, x + w, y + ry - ry * k, x + w, y + ry);
        self.line(x + w, y + h - ry);
        self.cubic(x + w, y + h - ry + ry * k, x + w - rx + rx * k, y + h, x + w - rx, y + h);
        self.line(x + rx, y + h);
        self.cubic(x + rx - rx * k, y + h, x, y + h - ry + ry * k, x, y + h - ry);
        self.line(x, y + ry);
        self.cubic(x, y + ry - ry * k, x + rx - rx * k, y, x + rx, y);
        self.close();
    }

    /// `parse(d)`: path data, command by command, an implicit command repeating the last (a moveto's as a
    /// lineto); the first error ends the path where it is. There is no cap on the commands: the attribute's
    /// length bounds them.
    pub fn parse(&mut self, d: &[u8]) -> PathEnd {
        let s = d;
        let mut at = 0;
        let mut cmd: u8 = 0;
        while ch(s, at) != 0 {
            skip_separators(s, &mut at);
            if ch(s, at) == 0 {
                break;
            }
            let mut explicit_cmd = false;
            if ch(s, at).is_ascii_alphabetic() {
                // Path data starts with a moveto; anything else is an error from the start (SVG 2).
                if cmd == 0 && ch(s, at) != b'M' && ch(s, at) != b'm' {
                    return PathEnd::NoMoveto;
                }
                cmd = ch(s, at);
                at += 1;
                explicit_cmd = true;
            } else if cmd == 0 {
                return PathEnd::NoMoveto;
            }
            let rel = cmd.is_ascii_lowercase();
            let (ox, oy) = if rel { (self.cx, self.cy) } else { (0.0, 0.0) };
            let mut v = [0f32; 7];
            let read = |n: usize, v: &mut [f32; 7], at: &mut usize| {
                for slot in v.iter_mut().take(n) {
                    match next_number(s, at) {
                        Some(value) => *slot = value,
                        None => return false,
                    }
                }
                true
            };
            let upper = cmd.to_ascii_uppercase();
            match upper {
                b'M' => {
                    if !read(2, &mut v, &mut at) {
                        return PathEnd::BadArguments(upper);
                    }
                    self.move_to(ox + v[0], oy + v[1]);
                    cmd = if rel { b'l' } else { b'L' };
                }
                b'L' => {
                    if !read(2, &mut v, &mut at) {
                        return PathEnd::BadArguments(upper);
                    }
                    self.line(ox + v[0], oy + v[1]);
                }
                b'H' => {
                    if !read(1, &mut v, &mut at) {
                        return PathEnd::BadArguments(upper);
                    }
                    self.line(ox + v[0], self.cy);
                }
                b'V' => {
                    if !read(1, &mut v, &mut at) {
                        return PathEnd::BadArguments(upper);
                    }
                    self.line(self.cx, oy + v[0]);
                }
                b'C' => {
                    if !read(6, &mut v, &mut at) {
                        return PathEnd::BadArguments(upper);
                    }
                    self.cubic(ox + v[0], oy + v[1], ox + v[2], oy + v[3], ox + v[4], oy + v[5]);
                }
                b'S' => {
                    if !read(4, &mut v, &mut at) {
                        return PathEnd::BadArguments(upper);
                    }
                    let smooth = self.last == b'C' || self.last == b'S';
                    let x1 = if smooth { 2.0 * self.cx - self.kx } else { self.cx };
                    let y1 = if smooth { 2.0 * self.cy - self.ky } else { self.cy };
                    self.cubic(x1, y1, ox + v[0], oy + v[1], ox + v[2], oy + v[3]);
                }
                b'Q' => {
                    if !read(4, &mut v, &mut at) {
                        return PathEnd::BadArguments(upper);
                    }
                    self.quad(ox + v[0], oy + v[1], ox + v[2], oy + v[3]);
                }
                b'T' => {
                    if !read(2, &mut v, &mut at) {
                        return PathEnd::BadArguments(upper);
                    }
                    let smooth = self.last == b'Q' || self.last == b'T';
                    let x1 = if smooth { 2.0 * self.cx - self.qx } else { self.cx };
                    let y1 = if smooth { 2.0 * self.cy - self.qy } else { self.cy };
                    self.quad(x1, y1, ox + v[0], oy + v[1]);
                }
                b'A' => {
                    let number = |slot: usize, v: &mut [f32; 7], at: &mut usize| match next_number(s, at) {
                        Some(value) => {
                            v[slot] = value;
                            true
                        }
                        None => false,
                    };
                    let mut large = 0;
                    let mut sweep = 0;
                    let flag = |out: &mut i32, at: &mut usize| match next_flag(s, at) {
                        Some(value) => {
                            *out = value;
                            true
                        }
                        None => false,
                    };
                    if !number(0, &mut v, &mut at)
                        || !number(1, &mut v, &mut at)
                        || !number(2, &mut v, &mut at)
                        || !flag(&mut large, &mut at)
                        || !flag(&mut sweep, &mut at)
                        || !number(3, &mut v, &mut at)
                        || !number(4, &mut v, &mut at)
                    {
                        return PathEnd::BadArguments(upper);
                    }
                    self.arc(v[0], v[1], v[2], large, sweep, ox + v[3], oy + v[4]);
                }
                b'Z' => {
                    // closepath takes no arguments, so it never repeats: data after it that is no command is an
                    // error, and the path ends there (SVG 2, "Error handling in path data").
                    if !explicit_cmd {
                        return PathEnd::AfterClose;
                    }
                    self.close();
                }
                _ => return PathEnd::UnknownCommand(upper),
            }
            self.last = upper;
        }
        PathEnd::End
    }
}

/// `svg_path_length(path, at, x, y)`: the length of a path's outline (curves as 64 chords each), and with `at`
/// not below 0 the point at that length along it (a move's own point when it is reached there, the end
/// when the path is shorter). A NaN `at` gives no point, as a negative one.
///
/// The path is one a [`PathBuilder`] made: each op has its points.
pub fn svg_path_length(path: &VectorPath, at: f32) -> (f32, Option<(f32, f32)>) {
    let mut total = 0.0f32;
    let (mut cx, mut cy, mut sx, mut sy) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut found: Option<(f32, f32)> = None;
    let mut kk = 0usize;
    let seg = |total: &mut f32, found: &mut Option<(f32, f32)>, x0: f32, y0: f32, x1: f32, y1: f32| {
        let d = (x1 - x0).hypot(y1 - y0);
        if found.is_none() && at >= 0.0 && *total + d >= at {
            let t = if d > 0.0 { (at - *total) / d } else { 0.0 };
            *found = Some((x0 + (x1 - x0) * t, y0 + (y1 - y0) * t));
        }
        *total += d;
    };
    for &op in &path.ops {
        match op {
            PathOp::r#move => {
                cx = path.points[kk];
                sx = cx;
                cy = path.points[kk + 1];
                sy = cy;
                kk += 2;
                if found.is_none() && at >= 0.0 && total >= at {
                    found = Some((cx, cy));
                }
            }
            PathOp::line => {
                seg(&mut total, &mut found, cx, cy, path.points[kk], path.points[kk + 1]);
                cx = path.points[kk];
                cy = path.points[kk + 1];
                kk += 2;
            }
            PathOp::quad | PathOp::cubic => {
                let cubic = op == PathOp::cubic;
                let p = &path.points[kk..kk + if cubic { 6 } else { 4 }];
                let (mut px, mut py) = (cx, cy);
                const steps: i32 = 64;
                for i in 1..=steps {
                    let t = i as f32 / steps as f32;
                    let u = 1.0 - t;
                    let (x, y) = if cubic {
                        (
                            u * u * u * cx + 3.0 * u * u * t * p[0] + 3.0 * u * t * t * p[2] + t * t * t * p[4],
                            u * u * u * cy + 3.0 * u * u * t * p[1] + 3.0 * u * t * t * p[3] + t * t * t * p[5],
                        )
                    } else {
                        (u * u * cx + 2.0 * u * t * p[0] + t * t * p[2], u * u * cy + 2.0 * u * t * p[1] + t * t * p[3])
                    };
                    seg(&mut total, &mut found, px, py, x, y);
                    px = x;
                    py = y;
                }
                cx = if cubic { p[4] } else { p[2] };
                cy = if cubic { p[5] } else { p[3] };
                kk += if cubic { 6 } else { 4 };
            }
            PathOp::close => {
                seg(&mut total, &mut found, cx, cy, sx, sy);
                cx = sx;
                cy = sy;
            }
        }
    }
    if found.is_none() && at >= 0.0 {
        found = Some((cx, cy));
    }
    (total, found)
}

// ---- Shapes ---------------------------------------------------------------------------------------------------

/// The outline of a shape element (`circle`, `ellipse`, `rect`, `line`, `polyline`, `polygon`, `path`, by its
/// lower-case `name`) from its attributes, lengths in percent of a view of `vw` × `vh` (a radius of its
/// diagonal `diag`), as `svg_geometry` and `svg_painter::draw_child` both build it; false for another name.
pub fn shape_path<T: SvgElements>(tree: &T, el: T::Node, name: &[u8], vw: f32, vh: f32, diag: f32, builder: &mut PathBuilder) -> bool {
    match name {
        b"circle" => {
            let r = attr_length(tree, el, b"r", diag, 0.0);
            if r > 0.0 {
                builder.ellipse(attr_length(tree, el, b"cx", vw, 0.0), attr_length(tree, el, b"cy", vh, 0.0), r, r);
            }
        }
        b"ellipse" => {
            let rx = attr_length(tree, el, b"rx", vw, 0.0);
            let ry = attr_length(tree, el, b"ry", vh, 0.0);
            if rx > 0.0 && ry > 0.0 {
                builder.ellipse(attr_length(tree, el, b"cx", vw, 0.0), attr_length(tree, el, b"cy", vh, 0.0), rx, ry);
            }
        }
        b"rect" => {
            let w = attr_length(tree, el, b"width", vw, 0.0);
            let h = attr_length(tree, el, b"height", vh, 0.0);
            let mut rx = attr_length(tree, el, b"rx", vw, -1.0);
            let mut ry = attr_length(tree, el, b"ry", vh, -1.0);
            if rx < 0.0 {
                rx = if ry < 0.0 { 0.0 } else { ry };
            }
            if ry < 0.0 {
                ry = rx;
            }
            if w > 0.0 && h > 0.0 {
                builder.rect(attr_length(tree, el, b"x", vw, 0.0), attr_length(tree, el, b"y", vh, 0.0), w, h, rx, ry);
            }
        }
        b"line" => {
            builder.move_to(attr_length(tree, el, b"x1", vw, 0.0), attr_length(tree, el, b"y1", vh, 0.0));
            builder.line(attr_length(tree, el, b"x2", vw, 0.0), attr_length(tree, el, b"y2", vh, 0.0));
        }
        b"polyline" | b"polygon" => {
            let pts = numbers(Some(tree.get_attr(el, b"points").unwrap_or(b"")));
            let mut i = 0;
            while i + 1 < pts.len() {
                if i == 0 {
                    builder.move_to(pts[i], pts[i + 1]);
                } else {
                    builder.line(pts[i], pts[i + 1]);
                }
                i += 2;
            }
            if name == b"polygon" && pts.len() >= 4 {
                builder.close();
            }
        }
        b"path" => {
            builder.parse(tree.get_attr(el, b"d").unwrap_or(b""));
        }
        _ => return false,
    }
    true
}

/// `svg_geometry(el, path)`: a shape element's outline in its user space, lengths in percent of the nearest
/// `<svg>` ancestor's view box (its `width` and `height` without one, 0 × 0 without an ancestor); none for
/// another element or an empty outline.
pub fn svg_geometry<T: SvgElements>(tree: &T, el: T::Node) -> Option<VectorPath> {
    let name = lowcase(c_str(&tree.tag_name(el)));
    let (mut vw, mut vh) = (0.0f32, 0.0f32);
    let mut p = tree.parent(el);
    while let Some(svg) = p {
        if tree.is_svg(svg) {
            if let Some(b) = view_box(tree, svg) {
                vw = b[2];
                vh = b[3];
            } else {
                vw = numbers(tree.get_attr(svg, b"width")).first().copied().unwrap_or(0.0);
                vh = numbers(tree.get_attr(svg, b"height")).first().copied().unwrap_or(0.0);
            }
            break;
        }
        p = tree.parent(svg);
    }
    let diag = ((vw * vw + vh * vh) / 2.0).sqrt();
    let mut builder = PathBuilder::new();
    if !shape_path(tree, el, &name, vw, vh, diag, &mut builder) {
        return None;
    }
    (!builder.path.ops.is_empty()).then_some(builder.path)
}

/// The geometry of `svg_gradient_paint` once its target is found: a `linearGradient`'s or `radialGradient`'s
/// type and coordinates over the box of a path's points (`gradientUnits="userSpaceOnUse"` as written, else
/// fractions of the box); none for another element. Its stops are the painter's.
pub fn svg_gradient_geometry<T: SvgElements>(tree: &T, target: T::Node, path: &VectorPath) -> Option<SvgGradient> {
    let tag = lowcase(c_str(&tree.tag_name(target)));
    let linear = tag == b"lineargradient";
    if !linear && tag != b"radialgradient" {
        return None;
    }
    let (mut minx, mut miny, mut maxx, mut maxy) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut i = 0;
    while i + 1 < path.points.len() {
        let (x, y) = (path.points[i], path.points[i + 1]);
        if i == 0 {
            minx = x;
            maxx = x;
            miny = y;
            maxy = y;
        } else {
            minx = cxx::min_f32(minx, x);
            maxx = cxx::max_f32(maxx, x);
            miny = cxx::min_f32(miny, y);
            maxy = cxx::max_f32(maxy, y);
        }
        i += 2;
    }
    let user = tree.get_attr(target, b"gradientunits").is_some_and(|u| c_str(u) == b"userSpaceOnUse");
    let (w, h) = (maxx - minx, maxy - miny);
    let mapx = |v: f32| if user { v } else { minx + v * w };
    let mapy = |v: f32| if user { v } else { miny + v * h };
    let mut g = SvgGradient::default();
    if linear {
        g.ty = 1;
        g.x1 = mapx(gradient_coordinate(tree, target, b"x1", 0.0));
        g.y1 = mapy(gradient_coordinate(tree, target, b"y1", 0.0));
        g.x2 = mapx(gradient_coordinate(tree, target, b"x2", 1.0));
        g.y2 = mapy(gradient_coordinate(tree, target, b"y2", 0.0));
    } else {
        g.ty = 2;
        let cx = gradient_coordinate(tree, target, b"cx", 0.5);
        let cy = gradient_coordinate(tree, target, b"cy", 0.5);
        g.x1 = mapx(cx);
        g.y1 = mapy(cy);
        g.fx = mapx(gradient_coordinate(tree, target, b"fx", cx));
        g.fy = mapy(gradient_coordinate(tree, target, b"fy", cy));
        let r = gradient_coordinate(tree, target, b"r", 0.5);
        g.r = if user { r } else { r * ((w * w + h * h) / 2.0).sqrt() };
    }
    Some(g)
}

// ---- Transforms -----------------------------------------------------------------------------------------------

/// `parse_svg_transform(s)`: a `transform` attribute's functions (`matrix` of six numbers, `translate`,
/// `scale`, `rotate` about the origin or a point, `skewX`, `skewY`, by name in any case), composed so the last
/// applies first. A function that is unknown or has too few numbers counts as the identity; the list ends
/// at a function with no `(` or no `)`. Identity for a null text.
pub fn parse_svg_transform(s: Option<&[u8]>) -> TransformMatrix {
    let mut m = TransformMatrix::default();
    let Some(s) = s else { return m };
    let mut list: Vec<TransformMatrix> = Vec::new();
    let mut at = 0;
    while ch(s, at) != 0 {
        skip_separators(s, &mut at);
        let name = at;
        while ch(s, at) != 0 && ch(s, at).is_ascii_alphabetic() {
            at += 1;
        }
        let function = lowcase(&s[name..at]);
        while ch(s, at) != 0 && ch(s, at) != b'(' {
            at += 1;
        }
        if ch(s, at) == 0 {
            break;
        }
        let Some(close) = (at..).take_while(|&i| ch(s, i) != 0).find(|&i| s[i] == b')') else { break };
        let args = numbers(Some(&s[at + 1..close]));
        at = close + 1;
        let mut f = TransformMatrix::default();
        if function == b"matrix" && args.len() == 6 {
            f = TransformMatrix::new(args[0], args[1], args[2], args[3], args[4], args[5]);
        } else if function == b"translate" && !args.is_empty() {
            f.tx = args[0];
            f.ty = if args.len() > 1 { args[1] } else { 0.0 };
        } else if function == b"scale" && !args.is_empty() {
            f.a = args[0];
            f.d = if args.len() > 1 { args[1] } else { args[0] };
        } else if function == b"rotate" && !args.is_empty() {
            let a = args[0] * pi / 180.0;
            let r = TransformMatrix::new(a.cos(), a.sin(), -a.sin(), a.cos(), 0.0, 0.0);
            f = if args.len() == 3 {
                TransformMatrix::new(1.0, 0.0, 0.0, 1.0, -args[1], -args[2])
                    .then(&r)
                    .then(&TransformMatrix::new(1.0, 0.0, 0.0, 1.0, args[1], args[2]))
            } else {
                r
            };
        } else if function == b"skewx" && !args.is_empty() {
            f.c = (args[0] * pi / 180.0).tan();
        } else if function == b"skewy" && !args.is_empty() {
            f.b = (args[0] * pi / 180.0).tan();
        }
        list.push(f);
    }
    for f in list.iter().rev() {
        m = m.then(f);
    }
    m
}

/// `svg_css_transform(el, border_box)`: an SVG element's CSS transform as a matrix in its user space: about
/// its `transform-origin` when it declares one, in `border_box` (the view box), or with `transform-box:
/// fill-box` in the box of its outline's points, whose corner is then the default origin (else the user
/// space's origin).
pub fn svg_css_transform<T: SvgElements>(tree: &T, el: T::Node, border_box: &Position) -> TransformMatrix {
    let has_origin = tree.is_html_tag(el) && tree.has_own_property(el, _transform_origin_);
    // transform-box: fill-box: the shape's own box in place of the view box, and its corner the default origin.
    let mut bx = *border_box;
    let mut fill_box = false;
    if tree.transform_fill_box(el)
        && let Some(geometry) = svg_geometry(tree, el)
        && geometry.points.len() >= 2
    {
        let (mut x0, mut y0) = (geometry.points[0], geometry.points[1]);
        let (mut x1, mut y1) = (x0, y0);
        let mut i = 0;
        while i + 1 < geometry.points.len() {
            x0 = cxx::min_f32(x0, geometry.points[i]);
            x1 = cxx::max_f32(x1, geometry.points[i]);
            y0 = cxx::min_f32(y0, geometry.points[i + 1]);
            y1 = cxx::max_f32(y1, geometry.points[i + 1]);
            i += 2;
        }
        bx = Position::new(Px(x0), Px(y0), Px(x1 - x0), Px(y1 - y0));
        fill_box = true;
    }
    let ox = if has_origin {
        bx.x + tree.transform_origin_x(el).calc_percent(bx.width)
    } else if fill_box {
        bx.x
    } else {
        Px(0.0)
    };
    let oy = if has_origin {
        bx.y + tree.transform_origin_y(el).calc_percent(bx.height)
    } else if fill_box {
        bx.y
    } else {
        Px(0.0)
    };
    let mut m = TransformMatrix::new(1.0, 0.0, 0.0, 1.0, -ox.value(), -oy.value());
    for function in tree.transform(el).iter().rev() {
        let mut f = TransformMatrix::default();
        let v = &function.v;
        match function.kind {
            TransformKind::translate => {
                f.tx = function.x.calc_percent(bx.width).value();
                f.ty = function.y.calc_percent(bx.height).value();
            }
            TransformKind::scale => {
                f.a = v[0];
                f.d = v[1];
            }
            TransformKind::rotate => {
                f.a = v[0].cos();
                f.b = v[0].sin();
                f.c = -f.b;
                f.d = f.a;
            }
            TransformKind::skew => {
                f.b = v[1].tan();
                f.c = v[0].tan();
            }
            TransformKind::matrix => f = TransformMatrix::new(v[0], v[1], v[2], v[3], v[4], v[5]),
            TransformKind::rotate_x => f.d = v[0].cos(),
            TransformKind::rotate_y => f.a = v[0].cos(),
        }
        m = m.then(&f);
    }
    m.then(&TransformMatrix::new(1.0, 0.0, 0.0, 1.0, ox.value(), oy.value()))
}

/// An SVG element's own transform as the painter and `svg_ctm` take it: its CSS `transform` when its own style
/// declares one, else its `transform` attribute.
pub fn svg_own_transform<T: SvgElements>(tree: &T, el: T::Node, view_box: &Position) -> TransformMatrix {
    if tree.is_html_tag(el) && tree.has_own_property(el, _transform_) {
        svg_css_transform(tree, el, view_box)
    } else {
        parse_svg_transform(tree.get_attr(el, b"transform"))
    }
}

/// `svg_ctm(el, ctm)`: an SVG element's user space to document coordinates, as the painter composes them: its
/// own transform first, then each parent's up to the nearest `<svg>`, then that one's view; none for an
/// `<svg>` itself or an element outside one.
pub fn svg_ctm<T: SvgElements>(tree: &T, el: T::Node) -> Option<TransformMatrix> {
    let mut chain = Vec::new();
    let mut svg = None;
    let mut p = Some(el);
    while let Some(e) = p {
        if tree.is_svg(e) {
            svg = Some(e);
            break;
        }
        chain.push(e);
        p = tree.parent(e);
    }
    let svg = svg?;
    if chain.is_empty() {
        return None;
    }
    let (view, group) = view_group(tree, svg, &tree.placement(svg));
    let view_box = Position::new(Px(view[0]), Px(view[1]), Px(view[2]), Px(view[3]));
    let mut m = TransformMatrix::default();
    for &p in &chain {
        m = m.then(&svg_own_transform(tree, p, &view_box));
    }
    Some(m.then(&group.matrix))
}

// ---- The <svg> element ----------------------------------------------------------------------------------------

/// `el_svg::view_box(box)`: the `viewBox` attribute's four numbers, its width and height positive.
pub fn view_box<T: SvgElements>(tree: &T, el: T::Node) -> Option<[f32; 4]> {
    let values = numbers(tree.get_attr(el, b"viewbox"));
    if values.len() != 4 || values[2] <= 0.0 || values[3] <= 0.0 {
        return None;
    }
    Some([values[0], values[1], values[2], values[3]])
}

/// `el_svg::get_content_size(sz, max_width)`: the `width` and `height` attributes' first numbers; with a view
/// box the missing one from its aspect ratio (the width `max_width` when both are missing), without one 300
/// × 150 for the missing ones.
pub fn get_content_size<T: SvgElements>(tree: &T, el: T::Node, max_width: Px) -> Size {
    content_size_of(tree.get_attr(el, b"viewbox"), tree.get_attr(el, b"width"), tree.get_attr(el, b"height"), max_width)
}

/// `el_svg::get_content_size(sz, max_width)` of the `viewBox`, `width` and `height` attributes.
pub fn content_size_of(viewbox: Option<&[u8]>, width: Option<&[u8]>, height: Option<&[u8]>, max_width: Px) -> Size {
    let b = {
        let values = numbers(viewbox);
        if values.len() != 4 || values[2] <= 0.0 || values[3] <= 0.0 { None } else { Some([values[0], values[1], values[2], values[3]]) }
    };
    let w = numbers(width);
    let h = numbers(height);
    if let Some(b) = b {
        let ratio = b[3] / b[2];
        let width = if let Some(&w) = w.first() {
            w
        } else if let Some(&h) = h.first() {
            h / ratio
        } else {
            max_width.value()
        };
        Size { width: Px(width), height: Px(if let Some(&h) = h.first() { h } else { width * ratio }) }
    } else {
        Size { width: Px(w.first().copied().unwrap_or(300.0)), height: Px(h.first().copied().unwrap_or(150.0)) }
    }
}

/// `el_svg::view_group(pos, view, group)`: the view (the view box, or `pos`'s size from 0, 0) and the group
/// that maps it onto `pos`: scaled to fit, or to cover with `slice`, aligned by `preserveAspectRatio`'s
/// `xMin`/`xMax`/`yMin`/`yMax` (centred by default), or stretched with `none` or without a view box.
pub fn view_group<T: SvgElements>(tree: &T, el: T::Node, pos: &Position) -> ([f32; 4], PaintGroup) {
    let mut view = [0.0, 0.0, pos.width.value(), pos.height.value()];
    let b = view_box(tree, el);
    let has_box = b.is_some();
    if let Some(b) = b {
        view = b;
    }
    let mut sx = pos.width.value() / view[2];
    let mut sy = pos.height.value() / view[3];
    let mut tx = pos.x.value();
    let mut ty = pos.y.value();
    let aspect = lowcase(c_str(tree.get_attr(el, b"preserveaspectratio").unwrap_or(b"")));
    if has_box && !contains(&aspect, b"none") {
        let slice = contains(&aspect, b"slice");
        let s = if slice { cxx::max_f32(sx, sy) } else { cxx::min_f32(sx, sy) };
        let (mut fx, mut fy) = (0.5f32, 0.5f32);
        if contains(&aspect, b"xmin") {
            fx = 0.0;
        } else if contains(&aspect, b"xmax") {
            fx = 1.0;
        }
        if contains(&aspect, b"ymin") {
            fy = 0.0;
        } else if contains(&aspect, b"ymax") {
            fy = 1.0;
        }
        tx += (pos.width.value() - view[2] * s) * fx;
        ty += (pos.height.value() - view[3] * s) * fy;
        sx = s;
        sy = s;
    }
    let group = PaintGroup {
        matrix: TransformMatrix::new(sx, 0.0, 0.0, sy, tx - view[0] * sx, ty - view[1] * sy),
        bounds: Position::new(Px(view[0]), Px(view[1]), Px(view[2]), Px(view[3])),
        ..PaintGroup::default()
    };
    (view, group)
}

// ---- <use> ----------------------------------------------------------------------------------------------------

/// `use_hosts()`: the `<use>`s being drawn, innermost last, each with the target it draws. The C++ keeps it per
/// thread; here whoever draws owns one.
#[derive(Debug)]
pub struct UseHosts<N> {
    hosts: Vec<(N, N)>,
}

impl<N> Default for UseHosts<N> {
    fn default() -> UseHosts<N> {
        UseHosts { hosts: Vec::new() }
    }
}

impl<N: Copy + Eq> UseHosts<N> {
    pub fn new() -> UseHosts<N> {
        UseHosts::default()
    }

    /// `use_host_scope(target, use)`: a `<use>` starts drawing its target.
    pub fn push(&mut self, target: N, r#use: N) {
        self.hosts.push((target, r#use));
    }

    /// The scope's end.
    pub fn pop(&mut self) {
        self.hosts.pop();
    }

    pub fn len(&self) -> usize {
        self.hosts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hosts.is_empty()
    }

    /// `svg_parent(el, hosts_above)`: a target's parent is the innermost `<use>` below `hosts_above` drawing it,
    /// which is then left behind (so mutually referencing targets never loop); any other element's is its own.
    pub fn svg_parent<T: SvgElements<Node = N>>(&self, tree: &T, el: N, hosts_above: &mut usize) -> Option<N> {
        let mut i = (*hosts_above).min(self.hosts.len());
        while i > 0 {
            i -= 1;
            if self.hosts[i].0 == el {
                *hosts_above = i;
                return Some(self.hosts[i].1);
            }
        }
        tree.parent(el)
    }
}

/// Why [`SvgUses::use_placement`] draws nothing. The C++ returns; the port says why, for its tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseRefusal {
    /// No `href` (nor `xlink:href`) that is `#` and a reference id.
    NoReference,
    /// The `<use>` is more than 8 deep.
    TooDeep,
    /// The id names nothing, or looking it up was past the budget.
    NoTarget,
    /// The target is the `<use>` or one of its ancestors.
    Cycle,
    /// The budget is spent.
    Budget,
}

/// Where a `<use>` draws its target: the matrix of its group (its `x`, `y`, and a `<symbol>`'s view box fitted
/// into its `width` × `height`), and whether the target is a symbol (whose children are drawn, not itself).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UsePlacement<N> {
    pub target: N,
    pub matrix: TransformMatrix,
    pub symbol: bool,
}

/// What `svg_painter` keeps for `<use>`: the budget left for this pass (`m_use_left`), how many `<use>`s deep
/// it is drawing (`m_use_depth`), and the references it looked up (`m_references`).
#[derive(Debug)]
pub struct SvgUses<N> {
    pub use_left: i32,
    pub use_depth: i32,
    references: BTreeMap<Vec<u8>, Option<N>>,
}

impl<N: Copy + Eq> SvgUses<N> {
    /// A painter's, with `use_left` of the budget.
    pub fn new(use_left: i32) -> SvgUses<N> {
        SvgUses { use_left, use_depth: 0, references: BTreeMap::new() }
    }

    /// `use_charge(units)`: takes `units` of the budget; when they are not there it is spent for good.
    pub fn use_charge(&mut self, units: usize) -> bool {
        if self.use_left < 0 || units > self.use_left as usize {
            self.use_left = 0;
            return false;
        }
        self.use_left -= units as i32;
        true
    }

    /// `reference(from, id)`: the element `#id` selects in the document, looked up once per pass (inside a
    /// `<use>` a lookup costs [`kUseLookup`]; when that is not there, none and nothing remembered).
    pub fn reference<T: SvgElements<Node = N>>(&mut self, tree: &T, id: &[u8]) -> Option<N> {
        if let Some(found) = self.references.get(id) {
            return *found;
        }
        if self.use_depth > 0 && !self.use_charge(kUseLookup) {
            return None;
        }
        let target = tree.root().and_then(|root| tree.select_one(root, &[b"#", id].concat()));
        self.references.insert(id.to_vec(), target);
        target
    }

    /// `svg_painter::draw_use(use, visibility, depth)` up to its group: the target an `href` (or `xlink:href`)
    /// `#id` names, unless the `<use>` is more than 8 deep, the target contains the `<use>` or the budget is
    /// spent (each `<use>` drawn costs 1), and the matrix that places it in the view `view`.
    pub fn use_placement<T: SvgElements<Node = N>>(
        &mut self,
        tree: &T,
        r#use: N,
        depth: i32,
        view: &[f32; 4],
    ) -> Result<UsePlacement<N>, UseRefusal> {
        let href = tree.get_attr(r#use, b"href").or_else(|| tree.get_attr(r#use, b"xlink:href"));
        let Some(href) = href.map(c_str) else { return Err(UseRefusal::NoReference) };
        if ch(href, 0) != b'#' || !svg_reference_id(&href[1..]) {
            return Err(UseRefusal::NoReference);
        }
        if depth > 8 {
            return Err(UseRefusal::TooDeep);
        }
        let Some(target) = self.reference(tree, &href[1..]) else { return Err(UseRefusal::NoTarget) };
        let mut p = Some(r#use);
        while let Some(e) = p {
            if e == target {
                return Err(UseRefusal::Cycle);
            }
            p = tree.parent(e);
        }
        if !self.use_charge(1) {
            return Err(UseRefusal::Budget);
        }
        let mut m = TransformMatrix::new(
            1.0,
            0.0,
            0.0,
            1.0,
            attr_length(tree, r#use, b"x", view[2], 0.0),
            attr_length(tree, r#use, b"y", view[3], 0.0),
        );
        let symbol = lowcase(c_str(&tree.tag_name(target))) == b"symbol";
        let b = numbers(tree.get_attr(target, b"viewbox"));
        if symbol && b.len() == 4 && b[2] > 0.0 && b[3] > 0.0 {
            let w = attr_length(tree, r#use, b"width", view[2], view[2]);
            let h = attr_length(tree, r#use, b"height", view[3], view[3]);
            let s = cxx::min_f32(w / b[2], h / b[3]);
            m = TransformMatrix::new(s, 0.0, 0.0, s, -b[0] * s + (w - b[2] * s) / 2.0, -b[1] * s + (h - b[3] * s) / 2.0).then(&m);
        }
        Ok(UsePlacement { target, matrix: m, symbol })
    }
}
