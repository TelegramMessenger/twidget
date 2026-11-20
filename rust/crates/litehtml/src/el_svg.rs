//! `el_svg.cpp` and `el_svg.h`: an `<svg>` as a document is built (it marks its document and its `width` and
//! `height` are dimensions), the geometry of what it draws, and its painting.
//!
//! The geometry: path data with its error handling, arcs as cubics, the outlines of `rect`, `circle`, `ellipse`,
//! `line`, `polyline` and `polygon`, the `transform` attribute and CSS transforms on SVG elements (with
//! `transform-box: fill-box`), the current transformation matrix, the view box and `preserveAspectRatio`, the
//! intrinsic size, a `<use>`'s placement with its caps, the parents a `<use>` lends its target, a gradient's
//! coordinates over a path's box, and path length and the point at a length.
//!
//! The painting: `svg_painter`'s walk (groups with opacity and transforms, `display` and `visibility` as
//! attributes and as style, the elements it never draws, shapes filled and stroked through `svg_paint.rs`'s
//! lookups, gradients, dashes, caps and joins, texts and their `<tspan>`s, `<use>` with its budget, nested
//! `<svg>`s drawn as groups, a top-level group drawn through the container's cache under its key), text runs
//! (`svg_text_runs`, `svg_text_box`), `el_svg::draw` with its clip, its paint key (`cached_paint_key`,
//! `cacheable_paint_key`, the elements' `paint_signature`), the container's cache or an isolated paint, and the
//! `<use>` budget its draws share per paint generation, and `el_svg::paint_shapes`. Hit testing (`element_at`,
//! the painter's hit mode) is stage L16's.
//!
//! What it reads of an element comes through [`SvgElements`]: tests implement it on what svg_oracle writes, and
//! [`DocSvg`] on the document.

use std::collections::{BTreeMap, BTreeSet};

use crate::borders::BorderRadiuses;
use crate::css_effects::{PaintGroup, PathOp, SvgGradient, TransformFunction, TransformKind, TransformMatrix, VectorPath};
use crate::css_length::CssLength;
use crate::css_tokenizer::{DIMENSION, NUMBER};
use crate::css_values::{StyleDisplay, Visibility};
use crate::cxx;
use crate::document::{Document, Env, StyleCx};
use crate::element::{ElementId, ElementKind, ElementState};
use crate::html::{WHITESPACE, lowcase, trim};
use crate::string_id::{
    _fill_, _fill_opacity_, _fill_rule_, _height_, _stroke_, _stroke_dasharray_, _stroke_dashoffset_, _stroke_linecap_, _stroke_linejoin_,
    _stroke_miterlimit_, _stroke_opacity_, _stroke_width_, _text_anchor_, _transform_, _transform_origin_, _width_,
};
use crate::style::{Value, has_var};
use crate::svg_paint::{svg_url_id, with_opacity};
use crate::types::{Position, Size};
use crate::web_color::WebColor;
use crate::{Px, StringId};

impl Document {
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

// ---- The document's elements --------------------------------------------------------------------------------------

/// FNV-1a's prime, which every paint key multiplies by.
const FNV_PRIME: u64 = 1099511628211;

/// FNV-1a's offset basis, where `paint_signature` and `cacheable_paint_key` start.
const FNV_BASIS: u64 = 1469598103934665603;

/// The document's elements as [`SvgElements`]: the DOM, the cascade's own declarations and computed transform, and
/// where an element's render items are placed.
#[derive(Clone, Copy)]
pub struct DocSvg<'a> {
    pub doc: &'a Document,
    pub cx: &'a StyleCx<'a>,
}

impl std::fmt::Debug for DocSvg<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocSvg").finish_non_exhaustive()
    }
}

impl SvgElements for DocSvg<'_> {
    type Node = ElementId;
    fn parent(&self, el: ElementId) -> Option<ElementId> {
        self.doc.parent(el)
    }
    fn tag_name(&self, el: ElementId) -> Vec<u8> {
        self.doc.get_tag_name(self.cx.names, el)
    }
    fn get_attr(&self, el: ElementId, name: &[u8]) -> Option<&[u8]> {
        self.doc.get_attr(el, name)
    }
    fn is_html_tag(&self, el: ElementId) -> bool {
        self.doc.kind(el).is_html_tag()
    }
    fn is_svg(&self, el: ElementId) -> bool {
        self.doc.kind(el) == ElementKind::el_svg
    }
    fn root(&self) -> Option<ElementId> {
        self.doc.root
    }
    fn select_one(&self, root: ElementId, selector: &[u8]) -> Option<ElementId> {
        let sel = crate::css_selector::CssSelector::parse(self.cx.names, selector, self.doc.mode)?;
        self.doc.matcher(self.cx).select_one(root, &sel)
    }
    fn has_own_property(&self, el: ElementId, name: StringId) -> bool {
        self.doc.kind(el).is_html_tag() && !matches!(self.doc.el(el).style.get_property(name).value, Value::Invalid)
    }
    fn transform(&self, el: ElementId) -> &[TransformFunction] {
        &self.doc.el(el).css.m_transform
    }
    fn transform_origin_x(&self, el: ElementId) -> &CssLength {
        &self.doc.el(el).css.m_transform_origin_x
    }
    fn transform_origin_y(&self, el: ElementId) -> &CssLength {
        &self.doc.el(el).css.m_transform_origin_y
    }
    fn transform_fill_box(&self, el: ElementId) -> bool {
        self.doc.el(el).css.m_transform_fill_box
    }
    /// `element::get_placement()`: the first live render item's placement, moved right and down to any later
    /// one's that starts further right or down.
    fn placement(&self, el: ElementId) -> Position {
        let mut pos = Position::default();
        let mut is_first = true;
        for &ri in &self.doc.el(el).renders {
            if !self.doc.render.alive(ri) {
                continue;
            }
            let ri_pos = self.doc.get_placement(self.cx, ri);
            if is_first {
                is_first = false;
                pos = ri_pos;
            } else {
                if pos.x < ri_pos.x {
                    pos.x = ri_pos.x;
                }
                if pos.y < ri_pos.y {
                    pos.y = ri_pos.y;
                }
            }
        }
        pos
    }
}

/// What an `<svg>` keeps between its paints: the last paint key and how many draws in a row it held
/// (`m_last_paint_key`, `m_stable_draws`), its content's key, the paint generation it was made in and whether
/// there is one (`m_content_key`, `m_content_generation`, `m_content_cacheable`), whether its groups may be drawn
/// through the cache (`m_group_cache_eligible`, once the content changed while it had a key), and the `<use>`
/// budget its draws took in a paint generation (`m_use_generation`, `m_use_allowance`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SvgPaintState {
    pub last_paint_key: u64,
    pub stable_draws: i32,
    pub content_key: u64,
    pub content_generation: u32,
    pub content_cacheable: bool,
    pub group_cache_eligible: bool,
    pub use_generation: u32,
    pub use_allowance: i32,
}

/// `svg_text_run`: a piece of an SVG `<text>`, its own characters or a `<tspan>`'s, styled by `style`, its box in
/// user space.
#[derive(Clone, Debug)]
pub struct SvgTextRun {
    pub style: ElementId,
    pub text: Vec<u8>,
    pub r#box: Position,
}

/// A paint signature mixed into a paint key.
fn mix_key(key: u64, signature: u64) -> u64 {
    (key ^ signature).wrapping_mul(FNV_PRIME)
}

/// A float's bits mixed into a paint key.
fn mix_bits(key: u64, v: f32) -> u64 {
    (key ^ u64::from(v.to_bits())).wrapping_mul(FNV_PRIME)
}

/// The tags a cached paint never holds: what they draw comes from elsewhere.
fn uncacheable_tag(name: &[u8]) -> bool {
    matches!(name, b"use" | b"image" | b"foreignobject" | b"canvas" | b"img")
}

/// The attributes whose `url(…)` a paint key looks at.
const URL_ATTRIBUTES: [&[u8]; 9] =
    [b"fill", b"stroke", b"style", b"mask", b"clip-path", b"filter", b"marker-start", b"marker-mid", b"marker-end"];

/// Where `needle` is in `hay` at or after `from`, as `strstr` from there finds it.
fn find_from(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| from + p)
}

impl Document {
    /// `element::paint_signature()`, and `html_tag::paint_signature()` for a tag: an FNV-1a hash of a text's
    /// characters, every selector it matched (its place and whether it is used), and for a tag its name's id as a
    /// decimal number and each attribute's name and value, each of those ended by 0xff.
    pub fn paint_signature(&self, id: ElementId) -> u64 {
        fn mix(h: &mut u64, bytes: &[u8]) {
            for &b in bytes {
                *h ^= u64::from(b);
                *h = h.wrapping_mul(FNV_PRIME);
            }
        }
        fn mix_str(h: &mut u64, value: &[u8]) {
            mix(h, value);
            *h ^= 0xff;
            *h = h.wrapping_mul(FNV_PRIME);
        }
        let mut h = FNV_BASIS;
        if self.is_text(id) {
            let mut text = Vec::new();
            self.get_text(id, &mut text);
            mix(&mut h, &text);
        }
        let el = self.el(id);
        for used in &el.used_styles {
            mix(&mut h, &used.selector.place().to_le_bytes());
            mix(&mut h, &[u8::from(used.used)]);
        }
        if el.kind.is_html_tag() {
            mix_str(&mut h, &cxx::to_string_int(el.tag.0 as i32));
            for (name, value) in &el.attrs {
                mix_str(&mut h, name);
                mix_str(&mut h, value);
            }
        }
        h
    }

    /// What an `<svg>`'s paints kept (the default for an element that is no `<svg>`).
    pub fn svg_paint_state(&self, id: ElementId) -> SvgPaintState {
        match self.el(id).state {
            ElementState::Svg(state) => state,
            _ => SvgPaintState::default(),
        }
    }

    fn set_svg_paint_state(&mut self, id: ElementId, state: SvgPaintState) {
        self.el_mut(id).state = ElementState::Svg(state);
    }

    /// `svg_text_runs(el, container, runs, box)`: what an SVG `<text>` draws, run by run, and where all of it goes,
    /// in its user space. Its own characters and each displayed `<tspan>`'s are one text with whitespace collapsed
    /// across them; a run starts at each `<tspan>` and where the text goes on after one. Each starts at its own
    /// `x`, `y` (their first numbers) when it has them, moved by `dx`, `dy` (`em` in its font size), and is as
    /// wide as the container measures it in its font (a run without characters or a font is skipped); the whole
    /// is moved by `text-anchor` (`middle`, `end`). None without a run.
    pub fn svg_text_runs(&self, cx: &StyleCx, hosts: &UseHosts<ElementId>, el: ElementId) -> Option<(Vec<SvgTextRun>, Position)> {
        struct Pending<'d> {
            style: ElementId,
            text: Vec<u8>,
            x: Option<&'d [u8]>,
            y: Option<&'d [u8]>,
            dx: Option<&'d [u8]>,
            dy: Option<&'d [u8]>,
        }
        let part = |style: ElementId, attrs: bool| Pending {
            style,
            text: Vec::new(),
            x: if attrs { self.get_attr(style, b"x") } else { None },
            y: if attrs { self.get_attr(style, b"y") } else { None },
            dx: if attrs { self.get_attr(style, b"dx") } else { None },
            dy: if attrs { self.get_attr(style, b"dy") } else { None },
        };
        let mut parts = vec![part(el, true)];
        let (mut space, mut any) = (false, false);
        // collect(node, style) as a walk: each node's children in order, a <tspan>'s before the rest of its parent's.
        let mut stack: Vec<(ElementId, ElementId, usize)> = vec![(el, el, 0)];
        while let Some(top) = stack.last_mut() {
            let (node, style, i) = *top;
            let Some(&child) = self.children(node).get(i) else {
                stack.pop();
                continue;
            };
            top.2 += 1;
            if self.is_text(child) {
                let mut text = Vec::new();
                self.get_text(child, &mut text);
                if parts.last().is_none_or(|p| p.style != style) {
                    parts.push(part(style, false));
                }
                let Some(last) = parts.last_mut() else { continue };
                for &ch in &text {
                    if twidget_cstd::is_space(ch) {
                        space = any;
                        continue;
                    }
                    if space {
                        last.text.push(b' ');
                    }
                    space = false;
                    any = true;
                    last.text.push(ch);
                }
            } else if lowcase(&self.get_tag_name(cx.names, child)) == b"tspan"
                && self.el(child).css.m_display != StyleDisplay::display_none.index()
            {
                parts.push(part(child, true));
                stack.push((child, child, 0));
            }
        }
        let first = |value: &[u8]| numbers(Some(value)).first().copied().unwrap_or(0.0);
        let shift = |value: Option<&[u8]>, em: f32| -> f32 {
            let Some(value) = value else { return 0.0 };
            if ch(value, 0) == 0 {
                return 0.0;
            }
            let mut p = 0;
            let Some(v) = next_number(value, &mut p) else { return 0.0 };
            if ch(value, p) == b'e' && ch(value, p + 1) == b'm' { v * em } else { v }
        };
        let mut runs = Vec::new();
        let (mut x, mut y, mut start) = (0.0f32, 0.0f32, 0.0f32);
        for (n, part) in parts.iter().enumerate() {
            let css = &self.el(part.style).css;
            let em = css.m_font_size.val();
            if let Some(v) = part.x {
                x = first(v);
            }
            if let Some(v) = part.y {
                y = first(v);
            }
            x += shift(part.dx, em);
            y += shift(part.dy, em);
            if n == 0 {
                start = x;
            }
            let font = css.m_font;
            if part.text.is_empty() || font == 0 {
                continue;
            }
            let fm = css.m_font_metrics;
            let width = cx.container.text_width(&part.text, font);
            runs.push(SvgTextRun {
                style: part.style,
                text: part.text.clone(),
                r#box: Position::new(Px(x), Px(y) - fm.ascent, width, fm.height),
            });
            x += width.value();
        }
        if runs.is_empty() {
            return None;
        }
        let anchor = self.svg_ident_in(cx, hosts, el, _text_anchor_);
        let total = x - start;
        let offset = if anchor == b"middle" {
            -total / 2.0
        } else if anchor == b"end" {
            -total
        } else {
            0.0
        };
        let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for run in &mut runs {
            run.r#box.x += Px(offset);
            x0 = cxx::min_f32(x0, run.r#box.x.value());
            y0 = cxx::min_f32(y0, run.r#box.y.value());
            x1 = cxx::max_f32(x1, run.r#box.right().value());
            y1 = cxx::max_f32(y1, run.r#box.bottom().value());
        }
        Some((runs, Position::new(Px(x0), Px(y0), Px(x1 - x0), Px(y1 - y0))))
    }

    /// `svg_text_box(el, container, box)`: where all of an SVG `<text>` goes, with no `<use>` being drawn.
    pub fn svg_text_box(&self, cx: &StyleCx, el: ElementId) -> Option<Position> {
        self.svg_text_runs(cx, &UseHosts::new(), el).map(|(_, b)| b)
    }

    /// `svg_group_key(root, key)`: a group's key for the container's cache: its own and every descendant's paint
    /// signature in tree order, then each ancestor's, unless one of them names a `url(…)` in a paint, style, mask,
    /// clip path, filter or marker attribute (or a paint of its style does, or holds a `var()`), a `var(` in its
    /// `fill` or `stroke` attribute, or the group holds a `use`, `image`, `foreignObject`, `canvas` or `img`, more
    /// than 4000 elements or fewer than 8.
    pub fn svg_group_key(&self, cx: &StyleCx, root: ElementId) -> Option<u64> {
        let mut hash: u64 = 0x67726f7570;
        let mut count = 0;
        let include = |hash: &mut u64, el: ElementId| -> bool {
            for attribute in URL_ATTRIBUTES {
                if let Some(value) = self.get_attr(el, attribute)
                    && contains(&lowcase(value), b"url(")
                {
                    return false;
                }
            }
            if self.kind(el).is_html_tag() {
                for name in [_fill_, _stroke_] {
                    if let Value::Tokens(tokens) = &self.el(el).style.get_property(name).value
                        && (has_var(tokens) || !svg_url_id(tokens).is_empty())
                    {
                        return false;
                    }
                }
            }
            for attribute in [&b"fill"[..], b"stroke"] {
                if let Some(value) = self.get_attr(el, attribute)
                    && contains(&lowcase(value), b"var(")
                {
                    return false;
                }
            }
            *hash = mix_key(*hash, self.paint_signature(el));
            true
        };
        // visit(root) as a walk in tree order.
        let mut stack = vec![root];
        while let Some(el) = stack.pop() {
            count += 1;
            if count > 4000 || !include(&mut hash, el) {
                return None;
            }
            if uncacheable_tag(&lowcase(&self.get_tag_name(cx.names, el))) {
                return None;
            }
            stack.extend(self.children(el).iter().rev());
        }
        if count < 8 {
            return None;
        }
        let mut parent = self.parent(root);
        while let Some(p) = parent {
            if !include(&mut hash, p) {
                return None;
            }
            parent = self.parent(p);
        }
        Some(hash)
    }

    /// `el_svg::cacheable_paint_key(key)`: the key of what an `<svg>` draws: its own and every descendant's paint
    /// signature in tree order, then each ancestor's; none when it holds a `use`, `image`, `foreignObject`,
    /// `canvas` or `img` or more than 4000 elements, or a `url(#id)` in a paint (its style's or attribute's, the
    /// ancestors' too) or in an attribute that may name one names an id none of its elements has.
    fn cacheable_paint_key(&self, cx: &StyleCx, id: ElementId) -> Option<u64> {
        let mut h = FNV_BASIS;
        let mut ids: BTreeSet<Vec<u8>> = BTreeSet::new();
        let mut references: Vec<Vec<u8>> = Vec::new();
        let collect_paint_references = |el: ElementId, references: &mut Vec<Vec<u8>>| {
            if !self.kind(el).is_html_tag() {
                return;
            }
            for name in [_fill_, _stroke_] {
                let id = if let Value::Tokens(tokens) = &self.el(el).style.get_property(name).value {
                    svg_url_id(tokens)
                } else if self.get_attr(el, &cx.names.s(name)).is_some_and(|value| value.contains(&b'(')) {
                    svg_url_id(&self.svg_property(cx, el, name, false))
                } else {
                    Vec::new()
                };
                if !id.is_empty() {
                    references.push(id);
                }
            }
        };
        let mut count = 0;
        // visit(this) as a walk in tree order.
        let mut stack = vec![id];
        while let Some(el) = stack.pop() {
            count += 1;
            if count > 4000 {
                return None;
            }
            if uncacheable_tag(&lowcase(&self.get_tag_name(cx.names, el))) {
                return None;
            }
            if let Some(value) = self.get_attr(el, b"id") {
                ids.insert(value.to_vec());
            }
            collect_paint_references(el, &mut references);
            for attribute in URL_ATTRIBUTES {
                let Some(value) = self.get_attr(el, attribute) else { continue };
                let mut at = find_from(value, b"url(", 0);
                while let Some(a) = at {
                    let hash = value[a..].iter().position(|&c| c == b'#').map(|p| a + p);
                    let end = value[a..].iter().position(|&c| c == b')').map(|p| a + p);
                    let (Some(hash), Some(end)) = (hash, end) else { break };
                    if hash > end {
                        break;
                    }
                    let mut reference = value[hash + 1..end].to_vec();
                    while matches!(reference.last(), Some(b'"' | b'\'' | b' ')) {
                        reference.pop();
                    }
                    references.push(reference);
                    at = find_from(value, b"url(", end);
                }
            }
            h = mix_key(h, self.paint_signature(el));
            stack.extend(self.children(el).iter().rev());
        }
        let mut parent = self.parent(id);
        while let Some(p) = parent {
            collect_paint_references(p, &mut references);
            h = mix_key(h, self.paint_signature(p));
            parent = self.parent(p);
        }
        if references.iter().any(|r| !ids.contains(r)) {
            return None;
        }
        Some(h)
    }

    /// `el_svg::cached_paint_key(key)`: the content's key, made again in a new paint generation (a key not made
    /// leaves the last one), and whether there is one. Once the key changed between two generations that both had
    /// one, the `<svg>`'s groups may be drawn through the cache.
    pub fn cached_paint_key(&mut self, cx: &StyleCx, id: ElementId) -> (bool, u64) {
        let generation = self.paint_generation();
        let mut st = self.svg_paint_state(id);
        if st.content_generation != generation {
            let previous_cacheable = st.content_cacheable;
            let previous_key = st.content_key;
            st.content_generation = generation;
            let key = self.cacheable_paint_key(cx, id);
            st.content_cacheable = key.is_some();
            if let Some(key) = key {
                st.content_key = key;
            }
            st.group_cache_eligible =
                st.group_cache_eligible || (previous_cacheable && st.content_cacheable && previous_key != st.content_key);
            self.set_svg_paint_state(id, st);
        }
        (st.content_cacheable, st.content_key)
    }

    /// `el_svg::draw(hdc, x, y, clip, ri)`: the tag's drawing; then, unless tracing, where its box is not empty and
    /// the clip meets it, what it draws inside, clipped to its box: through the container's cache under its paint
    /// key (once the key held for as many draws as `LH_SVG_LATE_CACHE` asks), else isolated, the view's group
    /// around the painter's walk. Its draws in a paint generation share the document's `<use>` budget: the first
    /// takes what is left, and later ones what the first spent.
    #[allow(clippy::too_many_arguments)]
    pub fn el_svg_draw(
        &mut self,
        cx: &StyleCx,
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
        let (view, group) = view_group(&DocSvg { doc: self, cx }, id, &pos);
        let sx = group.matrix.a;
        let sy = group.matrix.d;
        let tx = group.matrix.tx + view[0] * sx;
        let ty = group.matrix.ty + view[1] * sy;
        let container = cx.container;
        container.set_clip(&pos, &BorderRadiuses::default());
        let cache_bounds = Position::new(
            Px((pos.x.value() - group.matrix.tx) / sx),
            Px((pos.y.value() - group.matrix.ty) / sy),
            Px(pos.width.value() / sx),
            Px(pos.height.value() / sy),
        );
        let generation = self.paint_generation();
        let budget = self.svg_use_budget(use_budget);
        let granted = self.svg_paint_state(id).use_generation == generation;
        let allowance = if granted { self.svg_paint_state(id).use_allowance } else { budget };
        let mut spent = 0;
        let (cacheable, mut key) = self.cached_paint_key(cx, id);
        let mut try_cache = false;
        if cacheable {
            for v in [pos.width.value(), pos.height.value(), view[0], view[1], view[2], view[3], sx, sy, tx - tx.floor(), ty - ty.floor()] {
                key = mix_bits(key, v);
            }
            let mut st = self.svg_paint_state(id);
            if key == st.last_paint_key {
                st.stable_draws += 1;
            } else {
                st.last_paint_key = key;
                st.stable_draws = 0;
            }
            self.set_svg_paint_state(id, st);
            let stable_draws_needed = if cx.switches.svg_late_cache { 2 } else { 0 };
            try_cache = st.stable_draws >= stable_draws_needed;
        }
        let eligible = self.svg_paint_state(id).group_cache_eligible;
        let doc: &Document = self;
        let mut paint = |target: u64| {
            container.begin_group(target, &group);
            let mut painter = SvgPainter::new(doc, cx, target, view, eligible.then_some(cache_bounds), allowance);
            painter.draw_children(id, -1, 0);
            spent = allowance - painter.use_left();
            container.end_group(target);
        };
        let mut cached = false;
        if try_cache {
            cached = container.draw_cached(hdc, &pos, key, &mut paint);
        }
        if !cached {
            container.draw_isolated(hdc, &pos, &mut paint);
        }
        if !granted {
            let mut st = self.svg_paint_state(id);
            st.use_generation = generation;
            st.use_allowance = spent;
            self.set_svg_paint_state(id, st);
            self.paint.svg_use_budget -= spent;
        }
        container.del_clip();
    }

    /// `el_svg::paint_shapes(hdc, pos)`: what the `<svg>` draws inside, its view fitted to `pos`, with a budget of
    /// its own and no cache; false for an empty box.
    pub fn el_svg_paint_shapes(&self, cx: &StyleCx, id: ElementId, hdc: u64, pos: &Position) -> bool {
        if pos.width <= Px(0.0) || pos.height <= Px(0.0) {
            return false;
        }
        let (view, group) = view_group(&DocSvg { doc: self, cx }, id, pos);
        cx.container.begin_group(hdc, &group);
        SvgPainter::new(self, cx, hdc, view, None, kMaxUseDrawn).draw_children(id, -1, 0);
        cx.container.end_group(hdc);
        true
    }
}

// ---- The painter ----------------------------------------------------------------------------------------------

/// `svg_painter::attribute_visibility(el)`: the `visibility` attribute: 1 hidden (`hidden`, `collapse`), 0
/// `visible`, -1 none or another word.
fn attribute_visibility(doc: &Document, el: ElementId) -> i32 {
    let Some(value) = doc.get_attr(el, b"visibility") else { return -1 };
    let v = trim(&lowcase(value), WHITESPACE);
    if v == b"hidden" || v == b"collapse" {
        1
    } else if v == b"visible" {
        0
    } else {
        -1
    }
}

/// `svg_painter::attribute_hides(el)`: the `display` attribute is `none`.
fn attribute_hides(doc: &Document, el: ElementId) -> bool {
    doc.get_attr(el, b"display").is_some_and(|value| trim(&lowcase(value), WHITESPACE) == b"none")
}

/// The elements the painter never draws: definitions, metadata, and what only a reference draws.
fn never_drawn(name: &[u8]) -> bool {
    matches!(
        name,
        b"defs"
            | b"style"
            | b"title"
            | b"desc"
            | b"lineargradient"
            | b"radialgradient"
            | b"clippath"
            | b"mask"
            | b"pattern"
            | b"symbol"
            | b"marker"
            | b"filter"
            | b"script"
            | b"metadata"
    )
}

/// What a child the painter draws is.
#[derive(Clone, Copy, Debug)]
enum ChildKind {
    /// `g` (which the cache may hold at the top), `a`, `svg`, `switch`: its children are drawn.
    Group {
        g: bool,
    },
    Text,
    Use,
    /// A shape, or anything else (which draws nothing).
    Other,
}

/// What `draw_child` decided about a child: what it is, its visibility (-1 none, 0 visible, 1 hidden) and whether
/// it is hidden, and whether a group was begun for it.
#[derive(Clone, Copy, Debug)]
struct ChildLook {
    kind: ChildKind,
    visibility: i32,
    hidden: bool,
    grouped: bool,
}

/// The bounds of the painter's groups: everything near the view.
const GROUP_BOUNDS: Position = Position { x: Px(-100000.0), y: Px(-100000.0), width: Px(200000.0), height: Px(200000.0) };

/// `svg_painter` drawing (its hit mode is stage L16's): the walk over what an `<svg>` holds in its view, with the
/// `<use>` budget it was given, the references it looked up, the `<use>`s it is inside, and, when an `<svg>`'s
/// draw lets it, its top-level groups drawn through the container's cache in `cache_bounds`.
pub struct SvgPainter<'a> {
    doc: &'a Document,
    cx: &'a StyleCx<'a>,
    hdc: u64,
    view: [f32; 4],
    cache_groups: bool,
    cache_bounds: Position,
    uses: SvgUses<ElementId>,
    hosts: UseHosts<ElementId>,
}

impl std::fmt::Debug for SvgPainter<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SvgPainter").field("hdc", &self.hdc).field("view", &self.view).field("uses", &self.uses).finish_non_exhaustive()
    }
}

impl<'a> SvgPainter<'a> {
    /// `svg_painter(container, hdc, view, cache_bounds, use_left)`: the group cache is on when bounds are given and
    /// `svg_group_cache_enabled`.
    pub fn new(
        doc: &'a Document,
        cx: &'a StyleCx<'a>,
        hdc: u64,
        view: [f32; 4],
        cache_bounds: Option<Position>,
        use_left: i32,
    ) -> SvgPainter<'a> {
        SvgPainter {
            doc,
            cx,
            hdc,
            view,
            cache_groups: cache_bounds.is_some() && cx.switches.svg_group_cache,
            cache_bounds: cache_bounds.unwrap_or_default(),
            uses: SvgUses::new(use_left),
            hosts: UseHosts::new(),
        }
    }

    /// `use_left()`.
    pub fn use_left(&self) -> i32 {
        self.uses.use_left
    }

    fn tree(&self) -> DocSvg<'a> {
        DocSvg { doc: self.doc, cx: self.cx }
    }

    /// `draw_children(el, inherited, depth)`: each child in order; inside a `<use>` a list costs one unit for every
    /// 256 children.
    pub fn draw_children(&mut self, el: ElementId, inherited: i32, depth: i32) {
        let doc = self.doc;
        let children = doc.children(el);
        if self.uses.use_depth > 0 && !self.uses.use_charge(children.len() / 256) {
            return;
        }
        for &child in children {
            self.draw_child(child, inherited, depth);
        }
    }

    /// `draw_child(child, inherited, depth)`: nothing for a text, an element its style or `display` attribute hides,
    /// or one never drawn; else, in a group when it has opacity (its style's times its attribute's) or a
    /// transform, a group's children (a top-level `<g>` through the cache when it may be), or unless its
    /// visibility (its attribute's, else the one inherited, else its style's) hides it, a shape, a text or a
    /// `<use>`. Inside a `<use>` every element costs one unit. Recursion goes through here once a level: what is
    /// decided about the child is in [`SvgPainter::open_child`], what is drawn of a leaf in its own function.
    fn draw_child(&mut self, child: ElementId, inherited: i32, depth: i32) {
        if self.uses.use_depth > 0 && !self.doc.is_text(child) && !self.uses.use_charge(1) {
            return;
        }
        let Some(child_look) = self.open_child(child, inherited) else { return };
        match child_look.kind {
            ChildKind::Group { g } => {
                let cached = g
                    && self.cache_groups
                    && depth == 0
                    && !child_look.grouped
                    && self.draw_cached_group(child, child_look.visibility, depth);
                if !cached {
                    self.draw_children(child, child_look.visibility, depth + 1);
                }
            }
            _ if child_look.hidden => {}
            ChildKind::Text => self.draw_text(child),
            ChildKind::Use => self.draw_use(child, child_look.visibility, depth),
            ChildKind::Other => self.draw_shape(child),
        }
        if child_look.grouped {
            self.cx.container.end_group(self.hdc);
        }
    }

    /// The start of `draw_child` past the budget: none when the child is a text, its style or `display` attribute
    /// hides it or it is never drawn; else what it is, its visibility and whether that hides it, and whether it is
    /// drawn in a group of its opacity and transform, which is begun here.
    #[inline(never)]
    fn open_child(&mut self, child: ElementId, inherited: i32) -> Option<ChildLook> {
        let doc = self.doc;
        let css = &doc.el(child).css;
        if doc.is_text(child) || css.m_display == StyleDisplay::display_none.index() || attribute_hides(doc, child) {
            return None;
        }
        let mut visibility = attribute_visibility(doc, child);
        if visibility < 0 {
            visibility = inherited;
        }
        let hidden = if visibility >= 0 { visibility == 1 } else { css.m_visibility != Visibility::visibility_visible.index() };
        let name = lowcase(&doc.get_tag_name(self.cx.names, child));
        if never_drawn(&name) {
            return None;
        }
        let mut opacity = css.m_opacity;
        if let Some(attr) = doc.get_attr(child, b"opacity") {
            opacity *= cxx::max_f32(0.0, cxx::min_f32(1.0, twidget_cstd::strtof(attr).0));
        }
        let view = self.view;
        let view_box = Position::new(Px(view[0]), Px(view[1]), Px(view[2]), Px(view[3]));
        let group = PaintGroup {
            matrix: svg_own_transform(&self.tree(), child, &view_box),
            opacity,
            bounds: GROUP_BOUNDS,
            ..PaintGroup::default()
        };
        let grouped = opacity < 1.0 || !group.matrix.is_identity();
        if grouped {
            self.cx.container.begin_group(self.hdc, &group);
        }
        let kind = match &name[..] {
            b"g" | b"a" | b"svg" | b"switch" => ChildKind::Group { g: name == b"g" },
            b"text" => ChildKind::Text,
            b"use" => ChildKind::Use,
            _ => ChildKind::Other,
        };
        Some(ChildLook { kind, visibility, hidden, grouped })
    }

    /// A top-level group through `draw_cached` under its key (none: not drawn so), drawn (when the container asks)
    /// by a painter of its own: a full budget, no cache.
    #[inline(never)]
    fn draw_cached_group(&mut self, child: ElementId, visibility: i32, depth: i32) -> bool {
        let (doc, cx, view) = (self.doc, self.cx, self.view);
        let Some(mut key) = doc.svg_group_key(cx, child) else { return false };
        let b = self.cache_bounds;
        for v in [view[0], view[1], view[2], view[3], b.x.value(), b.y.value(), b.width.value(), b.height.value()] {
            key = mix_bits(key, v);
        }
        let mut hosts = std::mem::take(&mut self.hosts);
        let cached = cx.container.draw_cached(self.hdc, &self.cache_bounds, key, &mut |target| {
            let mut painter = SvgPainter::new(doc, cx, target, view, None, kMaxUseDrawn);
            painter.hosts = std::mem::take(&mut hosts);
            painter.draw_children(child, visibility, depth + 1);
            hosts = std::mem::take(&mut painter.hosts);
        });
        self.hosts = hosts;
        cached
    }

    /// `draw_use(use, visibility, depth)`: the element a `<use>` names in a group at its placement, its target
    /// lending it the `<use>`'s parents; a `<symbol>`'s children rather than itself.
    #[inline(never)]
    fn draw_use(&mut self, r#use: ElementId, visibility: i32, depth: i32) {
        let tree = self.tree();
        let Ok(placement) = self.uses.use_placement(&tree, r#use, depth, &self.view) else { return };
        let group = PaintGroup { matrix: placement.matrix, bounds: GROUP_BOUNDS, ..PaintGroup::default() };
        let container = self.cx.container;
        container.begin_group(self.hdc, &group);
        self.hosts.push(placement.target, r#use);
        self.uses.use_depth += 1;
        if placement.symbol {
            self.draw_children(placement.target, visibility, depth + 1);
        } else {
            self.draw_child(placement.target, visibility, depth + 1);
        }
        self.uses.use_depth -= 1;
        self.hosts.pop();
        container.end_group(self.hdc);
    }

    /// A shape's outline in the painter's view (a radius in percent of its diagonal), painted.
    #[inline(never)]
    fn draw_shape(&mut self, child: ElementId) {
        let view = self.view;
        let diag = ((view[2] * view[2] + view[3] * view[3]) / 2.0).sqrt();
        let name = lowcase(&self.doc.get_tag_name(self.cx.names, child));
        let mut builder = PathBuilder::new();
        if shape_path(&self.tree(), child, &name, view[2], view[3], diag, &mut builder) {
            self.paint(child, &mut builder.path);
        }
    }

    /// `paint(el, path)`: an outline that is not empty filled and stroked as the element's properties say (inside
    /// a `<use>` it costs one unit for every 256 points): the fill (black by default) or else its gradient, the
    /// stroke (none by default) or else, when it names one, its gradient, each at its opacity; the fill rule, the
    /// stroke's width, caps, joins, miter limit, dashes (an odd list twice) and their offset; drawn when it fills
    /// or strokes with a width.
    fn paint(&mut self, el: ElementId, path: &mut VectorPath) {
        if path.ops.is_empty() {
            return;
        }
        if self.uses.use_depth > 0 && !self.uses.use_charge(path.points.len() / 256) {
            return;
        }
        let (doc, cx, tree) = (self.doc, self.cx, self.tree());
        let hosts = &self.hosts;
        let uses = &mut self.uses;
        path.fill = doc.svg_paint_in(cx, hosts, el, _fill_, WebColor::BLACK, true, &mut path.fill_color, None);
        let mut stroke_gradient = false;
        path.stroke = doc.svg_paint_in(cx, hosts, el, _stroke_, WebColor::BLACK, false, &mut path.stroke_color, Some(&mut stroke_gradient));
        path.fill_color = with_opacity(path.fill_color, doc.svg_number_in(cx, hosts, el, _fill_opacity_, 1.0, true));
        if !path.fill {
            let opacity = doc.svg_number_in(cx, hosts, el, _fill_opacity_, 1.0, true);
            if let Some(g) = doc.svg_gradient_paint(cx, hosts, el, _fill_, path, opacity, &mut |id| uses.reference(&tree, id)) {
                path.fill_gradient = g;
                path.fill = true;
                if let Some(&(_, color)) = path.fill_gradient.stops.last() {
                    path.fill_color = color;
                }
            }
        }
        path.stroke_color = with_opacity(path.stroke_color, doc.svg_number_in(cx, hosts, el, _stroke_opacity_, 1.0, true));
        if !path.stroke && stroke_gradient {
            let opacity = doc.svg_number_in(cx, hosts, el, _stroke_opacity_, 1.0, true);
            if let Some(g) = doc.svg_gradient_paint(cx, hosts, el, _stroke_, path, opacity, &mut |id| uses.reference(&tree, id)) {
                path.stroke_gradient = g;
                path.stroke = true;
                if let Some(&(_, color)) = path.stroke_gradient.stops.last() {
                    path.stroke_color = color;
                }
            }
        }
        path.evenodd = doc.svg_ident_in(cx, hosts, el, _fill_rule_) == b"evenodd";
        path.stroke_width = cxx::max_f32(0.0, doc.svg_number_in(cx, hosts, el, _stroke_width_, 1.0, true));
        let cap = doc.svg_ident_in(cx, hosts, el, _stroke_linecap_);
        path.line_cap = if cap == b"round" {
            1
        } else if cap == b"square" {
            2
        } else {
            0
        };
        let join = doc.svg_ident_in(cx, hosts, el, _stroke_linejoin_);
        path.line_join = if join == b"round" {
            1
        } else if join == b"bevel" {
            2
        } else {
            0
        };
        path.miter_limit = doc.svg_number_in(cx, hosts, el, _stroke_miterlimit_, 4.0, true);
        path.dash_offset = doc.svg_number_in(cx, hosts, el, _stroke_dashoffset_, 0.0, true);
        let dashes = doc.svg_property_in(cx, hosts, el, _stroke_dasharray_, true);
        for tok in &dashes {
            if tok.ty == NUMBER || tok.ty == DIMENSION {
                path.dashes.push(cxx::max_f32(0.0, tok.n.number));
            }
        }
        if path.dashes.len() % 2 == 1 {
            let copy = path.dashes.clone();
            path.dashes.extend(copy);
        }
        if path.stroke_width == 0.0 {
            path.stroke = false;
        }
        if path.fill || path.stroke {
            cx.container.draw_path(self.hdc, path);
        }
    }

    /// `draw_text(el)`: each run of a `<text>` (inside a `<use>` costing one unit for every 256 bytes) in its fill
    /// at its fill opacity, unless its fill is none.
    #[inline(never)]
    fn draw_text(&mut self, el: ElementId) {
        let (doc, cx) = (self.doc, self.cx);
        let Some((runs, _)) = doc.svg_text_runs(cx, &self.hosts, el) else { return };
        for run in &runs {
            if self.uses.use_depth > 0 && !self.uses.use_charge(run.text.len() / 256) {
                return;
            }
            let mut color = WebColor::default();
            if !doc.svg_paint_in(cx, &self.hosts, run.style, _fill_, WebColor::BLACK, true, &mut color, None) {
                continue;
            }
            color = with_opacity(color, doc.svg_number_in(cx, &self.hosts, run.style, _fill_opacity_, 1.0, true));
            cx.container.draw_text(self.hdc, &run.text, doc.el(run.style).css.m_font, color, &run.r#box);
        }
    }
}
