//! `gradient.h` and `gradient.cpp`: angles, and the gradients and images a background declares
//! (https://drafts.csswg.org/css-images-4/#gradients), parsed as the fork parses them.

use crate::Px;
use crate::css_length::{CssLength, f_length, f_length_percentage, f_positive};
use crate::css_parser::parse_comma_separated_list;
use crate::css_tokenizer::{CV_FUNCTION, CssToken, DIMENSION, NUMBER, PERCENTAGE};
use crate::css_values::{
    BackgroundPosition, ColorSpace, CssValues, HueInterpolation, RadialExtent, color_space_strings, hue_interpolation_strings,
    radial_extent_strings, split_css_values,
};
use crate::document_container::{DocumentContainer, resolver};
use crate::html::lowcase;
use crate::string_id::{
    _bottom_, _conic_gradient_, _deg_, _grad_, _left_, _linear_gradient_, _rad_, _radial_gradient_, _repeating_conic_gradient_,
    _repeating_linear_gradient_, _repeating_radial_gradient_, _right_, _top_, _turn_, empty_id,
};
use crate::style::{at, parse_bg_position, parse_keyword};
use crate::web_color::{WebColor, parse_color};
use crate::{Names, StringId};

#[allow(clippy::approx_constant, clippy::excessive_precision)]
const PI: f32 = 3.14159265;

/// `gradient_side`: where a gradient starts or sits, as flags.
pub const gradient_side_none: u32 = 0;
pub const gradient_side_left: u32 = 0x01;
pub const gradient_side_right: u32 = 0x02;
pub const gradient_side_top: u32 = 0x04;
pub const gradient_side_bottom: u32 = 0x08;
pub const gradient_side_x_center: u32 = 0x10;
pub const gradient_side_y_center: u32 = 0x20;
pub const gradient_side_x_length: u32 = 0x40;
pub const gradient_side_y_length: u32 = 0x80;

/// `radial_shape_t`.
pub const radial_shape_none: i32 = 0;
pub const radial_shape_circle: i32 = 1;
pub const radial_shape_ellipse: i32 = 2;

/// `parse_angle(tok, angle, percents_allowed)` (https://drafts.csswg.org/css-values-4/#angles): degrees, or
/// None. A unitless zero is 0 and a percentage, where allowed, a share of 360; the unit is looked up through
/// the name table, which keeps it (`_id`), as the C++ does.
pub fn parse_angle(names: &Names, tok: &CssToken, percents_allowed: bool) -> Option<f32> {
    // The unit identifier may be omitted if the <angle> is zero.
    if tok.ty == NUMBER && Px(tok.n.number) == Px(0.0) {
        return Some(0.0);
    }

    // <angle-percentage> in conic gradient
    if tok.ty == PERCENTAGE && percents_allowed {
        return Some(tok.n.number * 360.0 / 100.0);
    }

    if tok.ty == DIMENSION {
        let number = tok.n.number;
        return match names.id(&lowcase(tok.unit())) {
            _deg_ => Some(number),
            _grad_ => Some((number / 400.0) * 360.0),
            _rad_ => Some((number / (2.0 * PI)) * 360.0),
            _turn_ => Some(number * 360.0),
            StringId(_) => None,
        };
    }

    None
}

/// `gradient::color_stop`: `<linear-color-stop> | <linear-color-hint> | <angular-color-stop> |
/// <angular-color-hint>`; a stop with two positions is two stops of one colour.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColorStop {
    pub is_color_hint: bool,
    pub color: WebColor,
    pub length: Option<CssLength>,
    pub angle: Option<f32>,
}

/// `gradient`.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    /// The function's name: `_linear_gradient_` … `_repeating_conic_gradient_`, `empty_id` for none.
    pub m_type: StringId,
    /// `gradient_side` flags; none for a linear gradient given by its angle.
    pub m_side: u32,
    /// A linear gradient's angle, read when `m_side` is none.
    pub angle: f32,
    pub m_colors: Vec<ColorStop>,
    /// A radial or conic gradient's position, with `m_side`.
    pub position_x: CssLength,
    pub position_y: CssLength,
    /// A `radial_shape_*`.
    pub radial_shape: i32,
    /// None when a radius is given.
    pub radial_extent: RadialExtent,
    pub radial_radius_x: CssLength,
    pub radial_radius_y: CssLength,
    pub conic_from_angle: f32,
    pub color_space: ColorSpace,
    pub hue_interpolation: HueInterpolation,
}

impl Default for Gradient {
    fn default() -> Gradient {
        Gradient::new(empty_id)
    }
}

impl Gradient {
    /// `gradient(type)`.
    pub fn new(ty: StringId) -> Gradient {
        Gradient {
            m_type: ty,
            m_side: gradient_side_none,
            angle: 180.0,
            m_colors: Vec::new(),
            position_x: CssLength::predef_value(0),
            position_y: CssLength::predef_value(0),
            radial_shape: radial_shape_ellipse,
            radial_extent: RadialExtent::radial_extent_farthest_corner,
            radial_radius_x: CssLength::predef_value(0),
            radial_radius_y: CssLength::predef_value(0),
            conic_from_angle: 0.0,
            color_space: ColorSpace::color_space_oklab,
            hue_interpolation: HueInterpolation::hue_interpolation_shorter,
        }
    }

    /// `is_empty()`.
    pub fn is_empty(&self) -> bool {
        self.m_type == empty_id || self.m_colors.is_empty()
    }

    pub fn is_linear(&self) -> bool {
        self.m_type == _linear_gradient_ || self.m_type == _repeating_linear_gradient_
    }

    pub fn is_radial(&self) -> bool {
        self.m_type == _radial_gradient_ || self.m_type == _repeating_radial_gradient_
    }

    pub fn is_conic(&self) -> bool {
        self.m_type == _conic_gradient_ || self.m_type == _repeating_conic_gradient_
    }
}

/// `image::type`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageType {
    #[default]
    type_none,
    type_url,
    type_gradient,
}

/// `image`: a background layer's image, none, a url or a gradient.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Image {
    pub ty: ImageType,
    pub url: Vec<u8>,
    pub m_gradient: Gradient,
}

impl Image {
    /// `is_empty()`.
    pub fn is_empty(&self) -> bool {
        match self.ty {
            ImageType::type_none => true,
            ImageType::type_url => self.url.is_empty(),
            ImageType::type_gradient => self.m_gradient.is_empty(),
        }
    }
}

/// `parse_gradient(token, gradient, container)`: `<gradient> = <linear-gradient()> |
/// <repeating-linear-gradient()> | <radial-gradient()> | <repeating-radial-gradient()> | <conic-gradient()> |
/// <repeating-conic-gradient()>`. Any function's name is looked up through the name table first, which keeps
/// it, as the C++ does.
pub fn parse_gradient(names: &Names, token: &CssToken, container: Option<&dyn DocumentContainer>) -> Option<Gradient> {
    if token.ty != CV_FUNCTION {
        return None;
    }

    let ty = names.id(&lowcase(token.name()));

    if !matches!(
        ty,
        _linear_gradient_
            | _repeating_linear_gradient_
            | _radial_gradient_
            | _repeating_radial_gradient_
            | _conic_gradient_
            | _repeating_conic_gradient_
    ) {
        return None;
    }

    let mut grad = Gradient::new(ty);

    if !grad.is_linear() {
        // radial and conic position defaults to 'center'
        grad.m_side = gradient_side_x_center | gradient_side_y_center;
    }

    let mut list = parse_comma_separated_list(&token.value);
    if list.is_empty() {
        return None;
    }

    let ok = if grad.is_linear() {
        parse_linear_gradient_direction_and_interpolation(names, &list[0], &mut grad)
    } else if grad.is_radial() {
        parse_radial_gradient_shape_size_position_interpolation(&list[0], &mut grad)
    } else {
        parse_conic_gradient_angle_position_interpolation(names, &list[0], &mut grad)
    };

    if ok {
        list.remove(0);
    }

    let ok = if grad.is_conic() {
        parse_color_stop_list::<f32>(names, &list, &mut grad, container)
    } else {
        parse_color_stop_list::<CssLength>(names, &list, &mut grad, container)
    };

    if !ok {
        return None;
    }

    Some(grad)
}

/// A `<length-percentage>` or an `<angle-percentage>`, as a colour stop or hint takes it.
trait Lenang: Sized {
    /// `parse_lenang(tok, lenang)`.
    fn parse(names: &Names, tok: &CssToken) -> Option<Self>;
    /// `color_stop(lenang)`: a hint.
    fn hint(self) -> ColorStop;
    /// `color_stop(color, lenang)`.
    fn stop(self, color: WebColor) -> ColorStop;
}

impl Lenang for CssLength {
    fn parse(_names: &Names, tok: &CssToken) -> Option<CssLength> {
        let mut length = CssLength::default();
        length.from_token(tok, f_length_percentage, CssValues::default()).then_some(length)
    }
    fn hint(self) -> ColorStop {
        ColorStop { is_color_hint: true, length: Some(self), ..ColorStop::default() }
    }
    fn stop(self, color: WebColor) -> ColorStop {
        ColorStop { color, length: Some(self), ..ColorStop::default() }
    }
}

impl Lenang for f32 {
    fn parse(names: &Names, tok: &CssToken) -> Option<f32> {
        parse_angle(names, tok, true)
    }
    fn hint(self) -> ColorStop {
        ColorStop { is_color_hint: true, angle: Some(self), ..ColorStop::default() }
    }
    fn stop(self, color: WebColor) -> ColorStop {
        ColorStop { color, angle: Some(self), ..ColorStop::default() }
    }
}

/// `parse_color_hint<T>(tokens, color_stops)`: `<color-hint> = <length-percentage> | <angle-percentage>`.
fn parse_color_hint<T: Lenang>(names: &Names, tokens: &[CssToken], color_stops: &mut Vec<ColorStop>) -> bool {
    if tokens.len() == 1
        && let Some(lenang) = T::parse(names, &tokens[0])
    {
        color_stops.push(lenang.hint());
        return true;
    }
    false
}

/// `parse_color_stop<T>(tokens, color_stops, container)`: `<color> <length-percentage>{1,2}?` or
/// `<color> <angle-percentage>{1,2}?`.
fn parse_color_stop<T: Lenang>(
    names: &Names,
    tokens: &[CssToken],
    color_stops: &mut Vec<ColorStop>,
    container: Option<&dyn DocumentContainer>,
) -> bool {
    if tokens.is_empty() || tokens.len() > 3 {
        return false;
    }

    let Some(color) = parse_color(names, &tokens[0], resolver(container)) else { return false };

    if tokens.len() == 1 {
        color_stops.push(ColorStop { color, ..ColorStop::default() });
        return true;
    }
    if tokens.len() == 2 {
        if let Some(lenang) = T::parse(names, &tokens[1]) {
            color_stops.push(lenang.stop(color));
            return true;
        }
    } else if tokens.len() == 3
        && let Some(lenang1) = T::parse(names, &tokens[1])
        && let Some(lenang2) = T::parse(names, &tokens[2])
    {
        color_stops.push(lenang1.stop(color));
        color_stops.push(lenang2.stop(color));
        return true;
    }
    false
}

/// `parse_color_stop_list<T>(list, grad, container)`: `<color-stop> , [ <color-hint>? , <color-stop> ]#`.
fn parse_color_stop_list<T: Lenang>(
    names: &Names,
    list: &[Vec<CssToken>],
    grad: &mut Gradient,
    container: Option<&dyn DocumentContainer>,
) -> bool {
    // at least two color-stops must be present
    if list.len() < 2 {
        return false;
    }

    if !parse_color_stop::<T>(names, &list[0], &mut grad.m_colors, container) {
        return false;
    }

    // [ <color-hint>? , <color-stop> ]#
    let mut i = 1;
    while i < list.len() {
        if parse_color_hint::<T>(names, &list[i], &mut grad.m_colors) {
            i += 1;
            if i == list.len() {
                // color-hint not followed by color-stop
                return false;
            }
        }
        if !parse_color_stop::<T>(names, &list[i], &mut grad.m_colors, container) {
            return false;
        }
        i += 1;
    }
    true
}

/// `parse_linear_gradient_direction_and_interpolation(tokens, gradient)`:
/// `[ <angle> | to <side-or-corner> ] || <color-interpolation-method>`, the gradient set only when it is.
fn parse_linear_gradient_direction_and_interpolation(names: &Names, tokens: &[CssToken], gradient: &mut Gradient) -> bool {
    let mut angle = 180.0;
    let mut side = gradient_side_none as i32;
    let mut color_space = ColorSpace::color_space_oklab;
    let mut hue_interpolation = HueInterpolation::hue_interpolation_shorter;

    let mut index = 0;
    if parse_linear_gradient_direction(names, tokens, &mut index, &mut angle, &mut side) {
        parse_color_interpolation_method(tokens, &mut index, &mut color_space, &mut hue_interpolation);
    } else if parse_color_interpolation_method(tokens, &mut index, &mut color_space, &mut hue_interpolation) {
        parse_linear_gradient_direction(names, tokens, &mut index, &mut angle, &mut side);
    } else {
        return false;
    }

    if index != tokens.len() as i32 {
        return false;
    }

    gradient.angle = angle;
    gradient.m_side = side as u32;
    gradient.color_space = color_space;
    gradient.hue_interpolation = hue_interpolation;
    true
}

fn is_side(ident: &[u8]) -> bool {
    matches!(ident, b"left" | b"right" | b"top" | b"bottom")
}

/// `parse_linear_gradient_direction(tokens, index, angle, side)`: `<angle> | to <side-or-corner>`, where
/// `<side-or-corner> = [left | right] || [top | bottom]`.
fn parse_linear_gradient_direction(names: &Names, tokens: &[CssToken], index: &mut i32, angle: &mut f32, side: &mut i32) -> bool {
    if let Some(a) = parse_angle(names, at(tokens, *index), false) {
        *angle = a;
        *index += 1;
        return true;
    }

    if at(tokens, *index).ident() != b"to" {
        return false;
    }

    let mut a = at(tokens, *index + 1).ident();
    let mut b = at(tokens, *index + 2).ident();

    if is_side(&a) {
        if !is_side(&b) {
            match names.id(&a) {
                _top_ => *angle = 0.0,
                _bottom_ => *angle = 180.0,
                _left_ => *angle = 270.0,
                _right_ => *angle = 90.0,
                StringId(_) => return false,
            }
            *index += 2;
            return true;
        }
        // fix order
        if a == b"top" || a == b"bottom" {
            std::mem::swap(&mut a, &mut b);
        }

        // check order
        if !(a == b"left" || a == b"right") || !(b == b"top" || b == b"bottom") {
            return false;
        }

        *side = if a == b"left" { gradient_side_left } else { gradient_side_right } as i32;
        *side |= if b == b"top" { gradient_side_top } else { gradient_side_bottom } as i32;
        *index += 3;
        return true;
    }
    false
}

/// `parse_conic_angle_position(tokens, index, gradient)`: `[ from <angle> ]? [ at <position> ]?`, which
/// always holds; what it reads it writes into the gradient at once.
fn parse_conic_angle_position(names: &Names, tokens: &[CssToken], index: &mut i32, gradient: &mut Gradient) -> bool {
    if at(tokens, *index).ident() == b"from"
        && let Some(angle) = parse_angle(names, at(tokens, *index + 1), false)
    {
        gradient.conic_from_angle = angle;
        *index += 2;
    }

    let mut i = *index;
    if at(tokens, i).ident() == b"at" {
        i += 1;
        if parse_gradient_position(tokens, &mut i, gradient) {
            *index = i;
        }
    }

    true
}

/// `parse_conic_gradient_angle_position_interpolation(tokens, gradient)`:
/// `[ [ from <angle> ]? [ at <position> ]? ] || <color-interpolation-method>`.
fn parse_conic_gradient_angle_position_interpolation(names: &Names, tokens: &[CssToken], gradient: &mut Gradient) -> bool {
    if tokens.is_empty() {
        return false;
    }

    let mut color_space = ColorSpace::color_space_oklab;
    let mut hue_interpolation = HueInterpolation::hue_interpolation_shorter;

    let mut index = 0;
    // checking color interpolation first because parse_conic_angle_position always succeeds
    if parse_color_interpolation_method(tokens, &mut index, &mut color_space, &mut hue_interpolation) {
        parse_conic_angle_position(names, tokens, &mut index, gradient);
    } else if parse_conic_angle_position(names, tokens, &mut index, gradient) {
        parse_color_interpolation_method(tokens, &mut index, &mut color_space, &mut hue_interpolation);
    } else {
        return false;
    }

    if index != tokens.len() as i32 {
        return false;
    }

    gradient.color_space = color_space;
    gradient.hue_interpolation = hue_interpolation;
    true
}

/// `parse_color_interpolation_method(tokens, index, color_space, hue_interpolation)`
/// (https://www.w3.org/TR/css-color-4/#color-interpolation-method): `in <color-space>`, then for a polar
/// space `<hue-interpolation-method> hue` if it follows.
fn parse_color_interpolation_method(
    tokens: &[CssToken],
    index: &mut i32,
    color_space: &mut ColorSpace,
    hue_interpolation: &mut HueInterpolation,
) -> bool {
    if at(tokens, *index).ident() == b"in"
        && let Some(space) = parse_keyword(at(tokens, *index + 1), CssValues::from(&color_space_strings), 1)
    {
        *color_space = ColorSpace::from_index(space).unwrap_or_default();
        *index += 2;
    } else {
        return false;
    }

    // "hue" is checked before the keyword, which would otherwise be taken without it.
    if *color_space >= ColorSpace::color_space_polar_start
        && at(tokens, *index + 1).ident() == b"hue"
        && let Some(hue) = parse_keyword(at(tokens, *index), CssValues::from(&hue_interpolation_strings), 1)
    {
        *hue_interpolation = HueInterpolation::from_index(hue).unwrap_or_default();
        *index += 2;
    }
    true
}

/// `parse_radial_size(tokens, index, gradient)` (https://www.w3.org/TR/css-images-3/#typedef-radial-size):
/// `<radial-extent> | <length [0,∞]> | <length-percentage [0,∞]>{2}`.
fn parse_radial_size(tokens: &[CssToken], index: &mut i32, gradient: &mut Gradient) -> bool {
    let tok0 = at(tokens, *index);
    let tok1 = at(tokens, *index + 1);

    if let Some(extent) = parse_keyword(tok0, CssValues::from(&radial_extent_strings), 1) {
        gradient.radial_extent = RadialExtent::from_index(extent).unwrap_or_default();
        *index += 1;
        return true;
    }

    let mut length = [CssLength::default(), CssLength::default()];
    let none = CssValues::default();
    if length[0].from_token(tok0, f_length_percentage | f_positive, none)
        && length[1].from_token(tok1, f_length_percentage | f_positive, none)
    {
        let [x, y] = length;
        gradient.radial_extent = RadialExtent::radial_extent_none;
        gradient.radial_radius_x = x;
        gradient.radial_radius_y = y;
        *index += 2;
        return true;
    }

    if length[0].from_token(tok0, f_length | f_positive, none) {
        gradient.radial_extent = RadialExtent::radial_extent_none;
        gradient.radial_radius_x = length[0].clone();
        *index += 1;
        return true;
    }

    false
}

/// `parse_gradient_position(tokens, index, gradient)`: a `<bg-position>` as the gradient's sides and
/// position.
fn parse_gradient_position(tokens: &[CssToken], index: &mut i32, gradient: &mut Gradient) -> bool {
    let mut x = CssLength::default();
    let mut y = CssLength::default();
    if !parse_bg_position(tokens, index, &mut x, &mut y, false) {
        return false;
    }

    let center = BackgroundPosition::background_position_center.index();
    gradient.m_side = 0;
    if x.is_predefined() {
        if x.predef() == center {
            gradient.m_side |= gradient_side_x_center;
        } else {
            gradient.m_side |= 1u32.wrapping_shl(x.predef() as u32);
        }
    } else {
        gradient.m_side |= gradient_side_x_length;
        gradient.position_x = x;
    }

    if y.is_predefined() {
        if y.predef() == center {
            gradient.m_side |= gradient_side_y_center;
        } else {
            gradient.m_side |= 1u32.wrapping_shl(y.predef() as u32);
        }
    } else {
        gradient.m_side |= gradient_side_y_length;
        gradient.position_y = y;
    }
    true
}

/// What the radial prelude's parsers write as they go (the C++ binds them to these by reference).
struct Radial {
    shape: i32,
    grad: Gradient,
    color_space: ColorSpace,
    hue_interpolation: HueInterpolation,
}

impl Radial {
    fn radial_shape(&mut self, tokens: &[CssToken], index: &mut i32) -> bool {
        const VALS: [&str; 2] = split_css_values::<2>("circle;ellipse");
        let Some(shape) = parse_keyword(at(tokens, *index), CssValues::from(&VALS), 1) else { return false };
        self.shape = shape;
        *index += 1;
        true
    }

    fn radial_size(&mut self, tokens: &[CssToken], index: &mut i32) -> bool {
        parse_radial_size(tokens, index, &mut self.grad)
    }

    fn radial_position(&mut self, tokens: &[CssToken], index: &mut i32) -> bool {
        parse_gradient_position(tokens, index, &mut self.grad)
    }

    fn color_interpolation_method(&mut self, tokens: &[CssToken], index: &mut i32) -> bool {
        parse_color_interpolation_method(tokens, index, &mut self.color_space, &mut self.hue_interpolation)
    }

    /// `oror(radial_shape, radial_size)`.
    fn shape_or_size(&mut self, tokens: &[CssToken], index: &mut i32) -> bool {
        if self.radial_shape(tokens, index) {
            self.radial_size(tokens, index);
            return true;
        }
        if self.radial_size(tokens, index) {
            self.radial_shape(tokens, index);
            return true;
        }
        false
    }

    /// `seq(opt(oror(radial_shape, radial_size)), opt(seq("at"_x, radial_position)))`, which always holds.
    fn shape_size_position(&mut self, tokens: &[CssToken], index: &mut i32) -> bool {
        self.shape_or_size(tokens, index);
        let save = *index;
        let at_position = if at(tokens, *index).ident() == b"at" {
            *index += 1;
            self.radial_position(tokens, index)
        } else {
            false
        };
        if !at_position {
            // backtrack
            *index = save;
        }
        true
    }

    /// `oror(color_interpolation_method, seq(…))`.
    fn parse(&mut self, tokens: &[CssToken], index: &mut i32) -> bool {
        if self.color_interpolation_method(tokens, index) {
            self.shape_size_position(tokens, index);
            return true;
        }
        if self.shape_size_position(tokens, index) {
            self.color_interpolation_method(tokens, index);
            return true;
        }
        false
    }
}

/// `parse_radial_gradient_shape_size_position_interpolation(tokens, result)`:
/// `[ [ <radial-shape> || <radial-size> ]? [ at <position> ]? ] || <color-interpolation-method>`.
fn parse_radial_gradient_shape_size_position_interpolation(tokens: &[CssToken], result: &mut Gradient) -> bool {
    // this check is needed because parse may succeed without consuming any input
    if tokens.is_empty() {
        return false;
    }

    let mut st = Radial {
        shape: radial_shape_none,
        grad: Gradient::default(),
        color_space: ColorSpace::color_space_oklab,
        hue_interpolation: HueInterpolation::hue_interpolation_shorter,
    };

    let mut index = 0;
    let ok = st.parse(tokens, &mut index) && index == tokens.len() as i32;
    if !ok {
        return false;
    }

    // A circle or no shape may be given one <length [0,∞]>.
    if st.shape == radial_shape_ellipse && !st.grad.radial_radius_x.is_predefined() && st.grad.radial_radius_y.is_predefined() {
        return false;
    }

    // An ellipse or no shape may instead be given <length-percentage [0,∞]>{2}.
    if st.shape == radial_shape_circle && !st.grad.radial_radius_y.is_predefined() {
        return false;
    }

    // With no shape, a single <length> makes a circle, anything else an ellipse.
    if st.shape == radial_shape_none {
        st.shape = if !st.grad.radial_radius_x.is_predefined() && st.grad.radial_radius_y.is_predefined() {
            radial_shape_circle
        } else {
            radial_shape_ellipse
        };
    }

    result.radial_shape = st.shape;

    result.radial_extent = st.grad.radial_extent;
    result.radial_radius_x = st.grad.radial_radius_x;
    result.radial_radius_y = st.grad.radial_radius_y;

    result.m_side = st.grad.m_side;
    result.position_x = st.grad.position_x;
    result.position_y = st.grad.position_y;

    result.color_space = st.color_space;
    result.hue_interpolation = st.hue_interpolation;

    true
}
