//! `css_effects.h` and `css_effects.cpp`, as far as reading declarations goes: the math a declaration folds
//! before it is parsed (`calc()`, `min()`, `max()`, `clamp()`, `env()`, `color-mix()` and viewport units,
//! `fold_math`), and the parsers of the values a style keeps as tokens until the computed style reads them
//! (`box-shadow`, `filter`, `transform` and its fast path, `transform-origin`, `clip-path`, grid track lists
//! and placements), and the types painters get (`transform_matrix`, `paint_group`, `vector_path`,
//! `svg_gradient`). What they compute for painting (`clip_path_shape::resolve`, the matrices of a CSS
//! transform) comes with the stages that paint.

use crate::Names;
use crate::css_length::{CssLength, f_length, f_length_percentage, f_percentage, f_positive};
use crate::css_parser::parse_comma_separated_list;
use crate::css_tokenizer::{
    COMMA, CV_FUNCTION, CssNumberType, CssToken, DIMENSION, HASH, IDENT, NUMBER, PERCENTAGE, ROUND_BLOCK, SQUARE_BLOCK, WHITESPACE,
    convert_string_to_number,
};
use crate::css_values::{CssUnits, CssValues};
use crate::cxx;
use crate::document_container::{DocumentContainer, resolver};
use crate::gradient::parse_angle;
use crate::html::lowcase;
use crate::types::Position;
use crate::web_color::{WebColor, parse_color};

// ---- box-shadow, transform, filter -----------------------------------------------------------------

/// `box_shadow`.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxShadow {
    pub x: CssLength,
    pub y: CssLength,
    pub blur: CssLength,
    pub spread: CssLength,
    pub color: WebColor,
    pub inset: bool,
}

impl Default for BoxShadow {
    fn default() -> BoxShadow {
        BoxShadow {
            x: CssLength::default(),
            y: CssLength::default(),
            blur: CssLength::default(),
            spread: CssLength::default(),
            color: WebColor::CURRENT_COLOR,
            inset: false,
        }
    }
}

/// `transform_function::kind_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TransformKind {
    #[default]
    translate,
    scale,
    rotate,
    skew,
    matrix,
    rotate_x,
    rotate_y,
}

/// `transform_function`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransformFunction {
    pub kind: TransformKind,
    pub x: CssLength,
    pub y: CssLength,
    pub v: [f32; 6],
}

/// `filter_function::kind_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilterKind {
    #[default]
    blur,
    brightness,
    contrast,
    grayscale,
    hue_rotate,
    invert,
    opacity,
    saturate,
    sepia,
    drop_shadow,
}

/// `filter_function`.
#[derive(Clone, Debug, PartialEq)]
pub struct FilterFunction {
    pub kind: FilterKind,
    pub amount: f32,
    pub x: CssLength,
    pub y: CssLength,
    pub radius: CssLength,
    pub color: WebColor,
}

impl Default for FilterFunction {
    fn default() -> FilterFunction {
        FilterFunction {
            kind: FilterKind::blur,
            amount: 0.0,
            x: CssLength::default(),
            y: CssLength::default(),
            radius: CssLength::default(),
            color: WebColor::CURRENT_COLOR,
        }
    }
}

/// `clip_path_shape::kind_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClipPathKind {
    #[default]
    none,
    inset,
    circle,
    ellipse,
    polygon,
}

/// `clip_path_shape`: the shape's lengths (an inset's four and its radius, a circle's or ellipse's radii
/// and centre, a polygon's points).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClipPathShape {
    pub kind: ClipPathKind,
    pub values: Vec<CssLength>,
    pub evenodd: bool,
}

impl ClipPathShape {
    pub fn empty(&self) -> bool {
        self.kind == ClipPathKind::none
    }
}

/// `grid_track_size::type_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrackType {
    length,
    fraction,
    #[default]
    automatic,
    min_content,
    max_content,
}

/// `grid_track_size`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridTrackSize {
    pub min_type: TrackType,
    pub max_type: TrackType,
    pub min_len: CssLength,
    pub max_len: CssLength,
    pub fr: f32,
}

/// `grid_track_list`.
#[derive(Clone, Debug, PartialEq)]
pub struct GridTrackList {
    pub tracks: Vec<GridTrackSize>,
    pub repeat: Vec<GridTrackSize>,
    pub repeat_at: i32,
    pub auto_fit: bool,
}

impl Default for GridTrackList {
    fn default() -> GridTrackList {
        GridTrackList { tracks: Vec::new(), repeat: Vec::new(), repeat_at: -1, auto_fit: false }
    }
}

impl GridTrackList {
    pub fn empty(&self) -> bool {
        self.tracks.is_empty() && self.repeat.is_empty()
    }
}

/// `grid_placement`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridPlacement {
    pub start: i32,
    pub end: i32,
    pub span: i32,
}

// ---- What painters get: matrices, groups, vector paths ----------------------------------------------

/// `transform_matrix`: a 2D affine map, `x' = a x + c y + tx`, `y' = b x + d y + ty`; identity by default.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransformMatrix {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Default for TransformMatrix {
    fn default() -> TransformMatrix {
        TransformMatrix { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 }
    }
}

impl TransformMatrix {
    /// `transform_matrix{a, b, c, d, tx, ty}`.
    pub const fn new(a: f32, b: f32, c: f32, d: f32, tx: f32, ty: f32) -> TransformMatrix {
        TransformMatrix { a, b, c, d, tx, ty }
    }

    /// `is_identity()`.
    pub fn is_identity(&self) -> bool {
        self.a == 1.0 && self.b == 0.0 && self.c == 0.0 && self.d == 1.0 && self.tx == 0.0 && self.ty == 0.0
    }

    /// `then(m)`: this map, then `m`.
    pub fn then(&self, m: &TransformMatrix) -> TransformMatrix {
        TransformMatrix {
            a: self.a * m.a + self.b * m.c,
            b: self.a * m.b + self.b * m.d,
            c: self.c * m.a + self.d * m.c,
            d: self.c * m.b + self.d * m.d,
            tx: self.tx * m.a + self.ty * m.c + m.tx,
            ty: self.tx * m.b + self.ty * m.d + m.ty,
        }
    }
}

/// `paint_group`: what a painter composes a group with.
#[derive(Clone, Copy, Debug)]
pub struct PaintGroup {
    pub opacity: f32,
    pub matrix: TransformMatrix,
    pub backface_hidden: bool,
    pub bounds: Position,
    pub exact_bounds: bool,
    /// The element whose group this is; none for an SVG painter's own groups.
    pub owner: Option<crate::ElementId>,
}

impl Default for PaintGroup {
    fn default() -> PaintGroup {
        PaintGroup {
            opacity: 1.0,
            matrix: TransformMatrix::default(),
            backface_hidden: false,
            bounds: Position::default(),
            exact_bounds: false,
            owner: None,
        }
    }
}

/// `svg_gradient`: a linear (`ty` 1) or radial (2) gradient in a path's user space, and its stops.
#[derive(Clone, Debug, PartialEq)]
pub struct SvgGradient {
    pub ty: i32,
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub r: f32,
    pub fx: f32,
    pub fy: f32,
    pub stops: Vec<(f32, WebColor)>,
}

impl Default for SvgGradient {
    fn default() -> SvgGradient {
        SvgGradient { ty: 0, x1: 0.0, y1: 0.0, x2: 1.0, y2: 0.0, r: 0.5, fx: 0.5, fy: 0.5, stops: Vec::new() }
    }
}

/// `vector_path::op_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathOp {
    r#move,
    line,
    quad,
    cubic,
    close,
}

/// `vector_path`: an outline (`ops`, each with its points: two for a move or line, four for a quad, six for a
/// cubic, none for a close) and how it is filled and stroked.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorPath {
    pub ops: Vec<PathOp>,
    pub points: Vec<f32>,
    pub fill: bool,
    pub fill_color: WebColor,
    pub evenodd: bool,
    pub stroke: bool,
    pub stroke_color: WebColor,
    pub stroke_width: f32,
    pub line_cap: i32,
    pub line_join: i32,
    pub miter_limit: f32,
    pub dashes: Vec<f32>,
    pub dash_offset: f32,
    pub fill_gradient: SvgGradient,
    pub stroke_gradient: SvgGradient,
}

impl Default for VectorPath {
    fn default() -> VectorPath {
        VectorPath {
            ops: Vec::new(),
            points: Vec::new(),
            fill: false,
            fill_color: WebColor::default(),
            evenodd: false,
            stroke: false,
            stroke_color: WebColor::default(),
            stroke_width: 1.0,
            line_cap: 0,
            line_join: 0,
            miter_limit: 4.0,
            dashes: Vec::new(),
            dash_offset: 0.0,
            fill_gradient: SvgGradient::default(),
            stroke_gradient: SvgGradient::default(),
        }
    }
}

/// `parse_clip_path(tokens, shape)`: `polygon()`, `inset()`, `circle()` or `ellipse()` as one function; on
/// failure the shape is none.
pub fn parse_clip_path(tokens: &[CssToken], out: &mut ClipPathShape) -> bool {
    *out = ClipPathShape::default();
    if tokens.len() != 1 || tokens[0].ty != CV_FUNCTION {
        return false;
    }
    let none = CssValues::default();
    let lp = f_length | f_percentage;
    let fn_ = &tokens[0];
    let mut shape = ClipPathShape::default();
    shape.kind = match &lowcase(fn_.name())[..] {
        b"polygon" => ClipPathKind::polygon,
        b"inset" => ClipPathKind::inset,
        b"circle" => ClipPathKind::circle,
        b"ellipse" => ClipPathKind::ellipse,
        _ => return false,
    };
    let mut at = 0;
    let args = &fn_.value;
    if shape.kind == ClipPathKind::polygon {
        if !args.is_empty() && (args[0].ident() == b"evenodd" || args[0].ident() == b"nonzero") {
            shape.evenodd = args[0].ident() == b"evenodd";
            if args.len() < 2 || args[1].ch() != COMMA {
                return false;
            }
            at = 2;
        }
        while at < args.len() {
            for _axis in 0..2 {
                let mut len = CssLength::default();
                if at >= args.len() {
                    return false;
                }
                at += 1;
                if !len.from_token(&args[at - 1], lp, none) {
                    return false;
                }
                shape.values.push(len);
            }
            if at < args.len() {
                at += 1;
                if args[at - 1].ch() != COMMA || at == args.len() {
                    return false;
                }
            }
        }
        if shape.values.len() < 6 {
            return false;
        }
    } else if shape.kind == ClipPathKind::inset {
        while at < args.len() && args[at].ident() != b"round" {
            let mut len = CssLength::default();
            if shape.values.len() == 4 {
                return false;
            }
            at += 1;
            if !len.from_token(&args[at - 1], lp, none) {
                return false;
            }
            shape.values.push(len);
        }
        if shape.values.is_empty() {
            return false;
        }
        let v = &shape.values;
        let four = vec![
            v[0].clone(),
            if v.len() > 1 { v[1].clone() } else { v[0].clone() },
            if v.len() > 2 { v[2].clone() } else { v[0].clone() },
            if v.len() > 3 {
                v[3].clone()
            } else if v.len() > 1 {
                v[1].clone()
            } else {
                v[0].clone()
            },
        ];
        shape.values = four;
        if at < args.len() {
            at += 1;
            let mut radius = CssLength::default();
            if at + 1 != args.len() || !radius.from_token(&args[at], lp, none) {
                return false;
            }
            if radius.val() < 0.0 {
                return false;
            }
            shape.values.push(radius);
        }
    } else {
        let radii = if shape.kind == ClipPathKind::circle { 1 } else { 2 };
        for _ in 0..radii {
            let mut len = CssLength::default();
            if at >= args.len() {
                return false;
            }
            at += 1;
            if !len.from_token(&args[at - 1], lp, none) {
                return false;
            }
            if len.val() < 0.0 {
                return false;
            }
            shape.values.push(len);
        }
        if at == args.len() {
            shape.values.push(CssLength::new(50.0, CssUnits::css_units_percentage));
            shape.values.push(CssLength::new(50.0, CssUnits::css_units_percentage));
        } else {
            at += 1;
            if args[at - 1].ident() != b"at" || at + 2 != args.len() {
                return false;
            }
            for i in 0..2 {
                let mut len = CssLength::default();
                let tok = &args[at];
                at += 1;
                let ident = tok.ident();
                if ident == b"center" {
                    len = CssLength::new(50.0, CssUnits::css_units_percentage);
                } else if ident == if i == 0 { &b"left"[..] } else { b"top" } {
                    len.assign(0.0);
                } else if ident == if i == 0 { &b"right"[..] } else { b"bottom" } {
                    len = CssLength::new(100.0, CssUnits::css_units_percentage);
                } else if !len.from_token(tok, lp, none) {
                    return false;
                }
                shape.values.push(len);
            }
        }
    }
    *out = shape;
    true
}

/// `parse_box_shadow(tokens, shadows, container)`: `none`, or shadows of two to four lengths, `inset` and a
/// colour each, between commas; on failure there are none.
pub fn parse_box_shadow(
    names: &Names,
    tokens: &[CssToken],
    shadows: &mut Vec<BoxShadow>,
    container: Option<&dyn DocumentContainer>,
) -> bool {
    shadows.clear();
    if tokens.len() == 1 && tokens[0].ident() == b"none" {
        return true;
    }
    let mut shadow = BoxShadow::default();
    let mut lengths = 0;
    let mut has_color = false;
    let flush = |shadow: &mut BoxShadow, lengths: &mut usize, has_color: &mut bool, shadows: &mut Vec<BoxShadow>| -> bool {
        if *lengths < 2 {
            return false;
        }
        shadows.push(std::mem::take(shadow));
        *lengths = 0;
        *has_color = false;
        true
    };
    for tok in tokens {
        if tok.ch() == COMMA {
            if !flush(&mut shadow, &mut lengths, &mut has_color, shadows) {
                shadows.clear();
                return false;
            }
            continue;
        }
        if tok.ident() == b"inset" {
            shadow.inset = true;
            continue;
        }
        let mut len = CssLength::default();
        if lengths < 4 && len.from_token(tok, f_length, CssValues::default()) {
            match lengths {
                0 => shadow.x = len,
                1 => shadow.y = len,
                2 => shadow.blur = len,
                _ => shadow.spread = len,
            }
            lengths += 1;
            continue;
        }
        if !has_color && let Some(color) = parse_color(names, tok, resolver(container)) {
            shadow.color = color;
            has_color = true;
            continue;
        }
        shadows.clear();
        return false;
    }
    if !flush(&mut shadow, &mut lengths, &mut has_color, shadows) {
        shadows.clear();
        return false;
    }
    true
}

/// `function_args(tok, args)`: a function's arguments without the commas; whether there are any.
fn function_args(tok: &CssToken, args: &mut Vec<CssToken>) -> bool {
    args.clear();
    for arg in &tok.value {
        if arg.ch() != COMMA {
            args.push(arg.clone());
        }
    }
    !args.is_empty()
}

/// `parse_number(tok, value)`: a number, or a percentage as a fraction.
fn parse_number(tok: &CssToken) -> Option<f32> {
    if tok.ty == NUMBER {
        return Some(tok.n.number);
    }
    if tok.ty == PERCENTAGE {
        return Some(tok.n.number / 100.0);
    }
    None
}

/// `parse_radians(tok, value)`.
fn parse_radians(names: &Names, tok: &CssToken) -> Option<f32> {
    let degrees = parse_angle(names, tok, false)?;
    #[allow(clippy::approx_constant, clippy::excessive_precision)]
    Some(degrees * 3.14159265f32 / 180.0)
}

// ---- Math folded before parsing ---------------------------------------------------------------------

/// `calc_value`: a number in doubles and its unit (lower-case, `%` or none).
#[derive(Clone, Debug, Default, PartialEq)]
struct CalcValue {
    number: f64,
    unit: Vec<u8>,
}

fn skip_ws(tokens: &[CssToken], index: &mut usize) {
    while *index < tokens.len() && tokens[*index].ty == WHITESPACE {
        *index += 1;
    }
}

/// `calc_atom(tokens, index, out, depth)`: a number, percentage or dimension, a `min()`, `max()` or
/// `clamp()` whose arguments share a unit (a bare zero takes any), or a `calc()` or `( … )` of a sum.
fn calc_atom(tokens: &[CssToken], index: &mut usize, out: &mut CalcValue, depth: i32) -> bool {
    skip_ws(tokens, index);
    if *index >= tokens.len() || depth > 16 {
        return false;
    }
    let tok = &tokens[*index];
    *index += 1;
    if tok.ty == NUMBER {
        *out = CalcValue { number: f64::from(tok.n.number), unit: Vec::new() };
        return true;
    }
    if tok.ty == PERCENTAGE {
        *out = CalcValue { number: f64::from(tok.n.number), unit: b"%".to_vec() };
        return true;
    }
    if tok.ty == DIMENSION {
        *out = CalcValue { number: f64::from(tok.n.number), unit: lowcase(tok.unit()) };
        return true;
    }
    let name = if tok.ty == CV_FUNCTION { lowcase(tok.name()) } else { Vec::new() };
    if tok.ty == CV_FUNCTION && matches!(&name[..], b"min" | b"max" | b"clamp") {
        let mut args: Vec<CalcValue> = Vec::new();
        let mut inner = 0;
        loop {
            let mut arg = CalcValue::default();
            if !calc_sum(&tok.value, &mut inner, &mut arg, depth + 1) {
                return false;
            }
            args.push(arg);
            skip_ws(&tok.value, &mut inner);
            if inner == tok.value.len() {
                break;
            }
            if tok.value[inner].ch() != COMMA {
                return false;
            }
            inner += 1;
        }
        if args.len() > 32 || (name == b"clamp" && args.len() != 3) {
            return false;
        }
        let mut unit = Vec::new();
        for arg in &args {
            if !arg.unit.is_empty() {
                unit = arg.unit.clone();
            }
        }
        for arg in &args {
            if arg.unit != unit && !(arg.unit.is_empty() && arg.number == 0.0) {
                return false;
            }
        }
        let mut value = args[0].number;
        if name == b"clamp" {
            value = cxx::max_f64(args[0].number, cxx::min_f64(args[1].number, args[2].number));
        } else {
            for arg in &args {
                value = if name == b"min" { cxx::min_f64(value, arg.number) } else { cxx::max_f64(value, arg.number) };
            }
        }
        *out = CalcValue { number: value, unit };
        return value.is_finite();
    }
    if tok.ty == ROUND_BLOCK || (tok.ty == CV_FUNCTION && name == b"calc") {
        let mut inner = 0;
        if !calc_sum(&tok.value, &mut inner, out, depth + 1) {
            return false;
        }
        skip_ws(&tok.value, &mut inner);
        return inner == tok.value.len();
    }
    false
}

/// `calc_operator(tokens, index, allowed)`: the operator after whitespace, if it is one of `allowed`.
fn calc_operator(tokens: &[CssToken], index: &mut usize, allowed: &[u8]) -> i32 {
    let mut probe = *index;
    skip_ws(tokens, &mut probe);
    if probe < tokens.len() && tokens[probe].ch() > 0 && tokens[probe].ch() < 128 && allowed.contains(&(tokens[probe].ch() as u8)) {
        *index = probe + 1;
        return tokens[probe].ch();
    }
    0
}

/// `calc_product(tokens, index, out, depth)`: atoms between `*` and `/`; one side of a product has no unit,
/// a divisor neither a unit nor zero.
fn calc_product(tokens: &[CssToken], index: &mut usize, out: &mut CalcValue, depth: i32) -> bool {
    if !calc_atom(tokens, index, out, depth) {
        return false;
    }
    loop {
        let op = calc_operator(tokens, index, b"*/");
        if op == 0 {
            break;
        }
        let mut right = CalcValue::default();
        if !calc_atom(tokens, index, &mut right, depth) {
            return false;
        }
        if op == i32::from(b'*') {
            if !out.unit.is_empty() && !right.unit.is_empty() {
                return false;
            }
            let unit = if out.unit.is_empty() { right.unit } else { std::mem::take(&mut out.unit) };
            *out = CalcValue { number: out.number * right.number, unit };
        } else {
            if !right.unit.is_empty() || right.number == 0.0 {
                return false;
            }
            out.number /= right.number;
        }
    }
    out.number.is_finite()
}

/// `calc_sum(tokens, index, out, depth)`: products between `+` and `-`, of one unit unless a side is zero.
fn calc_sum(tokens: &[CssToken], index: &mut usize, out: &mut CalcValue, depth: i32) -> bool {
    if !calc_product(tokens, index, out, depth) {
        return false;
    }
    loop {
        let op = calc_operator(tokens, index, b"+-");
        if op == 0 {
            break;
        }
        let mut right = CalcValue::default();
        if !calc_product(tokens, index, &mut right, depth) {
            return false;
        }
        if out.unit != right.unit && out.number != 0.0 && right.number != 0.0 {
            return false;
        }
        let unit = if out.unit.is_empty() { right.unit } else { std::mem::take(&mut out.unit) };
        let number = if op == i32::from(b'+') { out.number + right.number } else { out.number - right.number };
        *out = CalcValue { number, unit };
    }
    out.number.is_finite()
}

/// The token a folded value becomes: a number, a percentage or a dimension, a non-integer each.
fn folded_token(value: &CalcValue) -> CssToken {
    if value.unit.is_empty() {
        CssToken::with_number(NUMBER, value.number as f32, CssNumberType::css_number_number)
    } else if value.unit == b"%" {
        CssToken::with_number(PERCENTAGE, value.number as f32, CssNumberType::css_number_number)
    } else {
        let mut tok = CssToken::with_number(DIMENSION, value.number as f32, CssNumberType::css_number_number);
        tok.str = value.unit.clone();
        tok
    }
}

/// `fold_calc(tok)`: a `calc()` of one unit as its value; true for any other token, false for a `calc()`
/// that does not fold.
fn fold_calc(tok: &mut CssToken) -> bool {
    if tok.ty != CV_FUNCTION || lowcase(tok.name()) != b"calc" {
        return true;
    }
    let mut value = CalcValue::default();
    let mut index = 0;
    if !calc_sum(&tok.value, &mut index, &mut value, 0) {
        return false;
    }
    skip_ws(&tok.value, &mut index);
    if index != tok.value.len() {
        return false;
    }
    *tok = folded_token(&value);
    true
}

/// `fold_env(tok)`: `env(name, fallback)` as its one-token fallback, a safe-area or keyboard inset as 0px.
fn fold_env(tok: &mut CssToken) -> bool {
    if tok.value.is_empty() || tok.value[0].ty != IDENT {
        return false;
    }
    let name = lowcase(tok.value[0].name());
    if tok.value.len() > 2 && tok.value[1].ch() == COMMA {
        let fallback: Vec<CssToken> = tok.value[2..].to_vec();
        if fallback.len() != 1 {
            return false;
        }
        *tok = fallback[0].clone();
        return true;
    }
    if tok.value.len() == 1
        && (name.starts_with(b"safe-area-inset-") || name.starts_with(b"keyboard-inset-") || name.starts_with(b"safe-area-max-inset-"))
    {
        let mut zero = CssToken::with_number(DIMENSION, 0.0, CssNumberType::css_number_integer);
        zero.str = b"px".to_vec();
        *tok = zero;
        return true;
    }
    false
}

/// `mix_part(part, color, percent, container)`: a colour that is not black (it compares equal to
/// `currentcolor`) and an optional percentage up to 100 (-1 when none).
fn mix_part(names: &Names, part: &[CssToken], color: &mut WebColor, percent: &mut f32, container: Option<&dyn DocumentContainer>) -> bool {
    *percent = -1.0;
    let mut have_color = false;
    for tok in part {
        if tok.ty == PERCENTAGE && *percent < 0.0 {
            *percent = tok.n.number;
            continue;
        }
        if !have_color && let Some(c) = parse_color(names, tok, resolver(container)) {
            *color = c;
            if !(*color == WebColor::CURRENT_COLOR) {
                have_color = true;
                continue;
            }
        }
        return false;
    }
    have_color && (*percent < 0.0 || (*percent >= 0.0 && *percent <= 100.0))
}

/// `fold_color_mix(tok, container)`: `color-mix(in srgb, a p%, b q%)` as a hash of the mix.
fn fold_color_mix(names: &Names, tok: &mut CssToken, container: Option<&dyn DocumentContainer>) -> bool {
    let parts = parse_comma_separated_list(&tok.value);
    if parts.len() != 3 || parts[0].len() != 2 || parts[0][0].ident() != b"in" || parts[0][1].ident() != b"srgb" {
        return false;
    }
    let mut a = WebColor::default();
    let mut b = WebColor::default();
    let mut pa = 0.0f32;
    let mut pb = 0.0f32;
    if !mix_part(names, &parts[1], &mut a, &mut pa, container) || !mix_part(names, &parts[2], &mut b, &mut pb, container) {
        return false;
    }
    if pa < 0.0 && pb < 0.0 {
        pa = 50.0;
        pb = 50.0;
    } else if pa < 0.0 {
        pa = 100.0 - pb;
    } else if pb < 0.0 {
        pb = 100.0 - pa;
    }
    let sum = pa + pb;
    if sum <= 0.0 {
        return false;
    }
    let multiplier = if sum < 100.0 { sum / 100.0 } else { 1.0 };
    let wa = pa / sum;
    let wb = pb / sum;
    let aa = f32::from(a.alpha) / 255.0;
    let ab = f32::from(b.alpha) / 255.0;
    let alpha = aa * wa + ab * wb;
    let channel = |ca: u8, cb: u8| -> f32 {
        if alpha <= 0.0 {
            return 0.0;
        }
        (cxx::min_f32(255.0, cxx::max_f32(0.0, (f32::from(ca) * aa * wa + f32::from(cb) * ab * wb) / alpha))).round()
    };
    let hex = format!(
        "{:02x}{:02x}{:02x}{:02x}",
        cxx::f32_to_int(channel(a.red, b.red)),
        cxx::f32_to_int(channel(a.green, b.green)),
        cxx::f32_to_int(channel(a.blue, b.blue)),
        cxx::f32_to_int((cxx::min_f32(1.0, cxx::max_f32(0.0, alpha * multiplier)) * 255.0).round())
    );
    let mut folded = CssToken::with_str(HASH, hex.as_bytes());
    folded.hash_type = crate::css_tokenizer::CssHashType::css_hash_unrestricted;
    *tok = folded;
    true
}

/// `viewport_axis(token)`: 1 for a width unit (`vw`, `svw`, `lvw`, `dvw`), 2 height, 3 min, 4 max; 0 for
/// anything else.
fn viewport_axis(token: &CssToken) -> i32 {
    if token.ty != DIMENSION {
        return 0;
    }
    match &lowcase(token.unit())[..] {
        b"vw" | b"svw" | b"lvw" | b"dvw" => 1,
        b"vh" | b"svh" | b"lvh" | b"dvh" => 2,
        b"vmin" | b"svmin" | b"lvmin" | b"dvmin" => 3,
        b"vmax" | b"svmax" | b"lvmax" | b"dvmax" => 4,
        _ => 0,
    }
}

/// `fold_math(tokens, container, depth)`: viewport lengths in px at the container's viewport (with a
/// container), `env()`, and every math function and `color-mix()` that folds, at any depth to 16; whether
/// anything changed.
pub fn fold_math(names: &Names, tokens: &mut [CssToken], container: Option<&dyn DocumentContainer>, depth: i32) -> bool {
    if depth > 16 {
        return false;
    }
    let mut changed = false;
    for tok in tokens.iter_mut() {
        let axis = viewport_axis(tok);
        if axis != 0
            && let Some(container) = container
        {
            let media = container.get_media_features();
            let width = media.width.value();
            let height = media.height.value();
            let extent = match axis {
                1 => width,
                2 => height,
                3 => cxx::min_f32(width, height),
                _ => cxx::max_f32(width, height),
            };
            let mut px = CssToken::with_number(DIMENSION, extent * tok.n.number / 100.0, CssNumberType::css_number_number);
            px.str = b"px".to_vec();
            *tok = px;
            changed = true;
        }
        if !tok.is_component_value() {
            continue;
        }
        if tok.ty == CV_FUNCTION && lowcase(tok.name()) == b"env" && fold_env(tok) {
            changed = true;
            if !tok.is_component_value() {
                continue;
            }
        }
        if fold_math(names, &mut tok.value, container, depth + 1) {
            changed = true;
        }
        if tok.ty != CV_FUNCTION {
            continue;
        }
        let name = lowcase(tok.name());
        if matches!(&name[..], b"calc" | b"min" | b"max" | b"clamp") {
            let single = vec![tok.clone()];
            let mut value = CalcValue::default();
            let mut index = 0;
            if !calc_atom(&single, &mut index, &mut value, 0) {
                continue;
            }
            *tok = folded_token(&value);
            changed = true;
        } else if name == b"color-mix" && fold_color_mix(names, tok, container) {
            changed = true;
        }
    }
    changed
}

/// `has_foldable_math(tokens)`: a viewport length, or a math function, `env()` or `color-mix()`, at any
/// depth.
pub fn has_foldable_math(tokens: &[CssToken]) -> bool {
    for tok in tokens {
        if viewport_axis(tok) != 0 {
            return true;
        }
        if tok.ty == CV_FUNCTION && matches!(&lowcase(tok.name())[..], b"calc" | b"min" | b"max" | b"clamp" | b"env" | b"color-mix") {
            return true;
        }
        if tok.is_component_value() && has_foldable_math(&tok.value) {
            return true;
        }
    }
    false
}

/// `has_viewport_units(tokens)`: a viewport length at any depth.
pub fn has_viewport_units(tokens: &[CssToken]) -> bool {
    for tok in tokens {
        if viewport_axis(tok) != 0 {
            return true;
        }
        if tok.is_component_value() && has_viewport_units(&tok.value) {
            return true;
        }
    }
    false
}

/// `has_viewport_units(text)`: whether CSS or HTML text may hold a viewport length (`50vw`, `10svh`,
/// `2vmin`), a quick scan that may answer yes for text that is not CSS, never no for text that is.
pub fn has_viewport_units_text(text: &[u8]) -> bool {
    let count = text.len();
    let word = |at: usize, w: &[u8]| (0..w.len()).all(|k| at + k < count && text[at + k].to_ascii_lowercase() == w[k]);
    let mut i = 1;
    while i + 1 < count {
        if text[i] != b'v' && text[i] != b'V' {
            i += 1;
            continue;
        }
        let mut number = i - 1;
        let prefix = text[number].to_ascii_lowercase();
        if (prefix == b's' || prefix == b'l' || prefix == b'd') && number > 0 {
            number -= 1;
        }
        let digit = text[number];
        if !digit.is_ascii_digit() && digit != b'.' {
            i += 1;
            continue;
        }
        let mut end = i + 1;
        let unit = text[end].to_ascii_lowercase();
        if unit == b'w' || unit == b'h' {
            end += 1;
        } else if word(end, b"min") || word(end, b"max") {
            end += 3;
        } else {
            i += 1;
            continue;
        }
        if end >= count || !text[end].is_ascii_alphabetic() {
            return true;
        }
        i += 1;
    }
    false
}

/// `filter_reach(filters)`: how far a chain of filters reaches past the box, in px.
pub fn filter_reach(filters: &[FilterFunction]) -> f32 {
    let mut reach = 0.0f32;
    for f in filters {
        if f.kind == FilterKind::blur {
            reach += (f.radius.val().abs() * 3.0).ceil();
        } else if f.kind == FilterKind::drop_shadow {
            reach += (f.radius.val().abs() * 3.0).ceil() + cxx::max_f32(f.x.val().abs(), f.y.val().abs());
        }
    }
    reach
}

/// `parse_filter(tokens, functions, container)`: `none`, or up to 32 filter functions, each argument's
/// `calc()` folded; on failure there are none.
pub fn parse_filter(
    names: &Names,
    tokens: &[CssToken],
    functions: &mut Vec<FilterFunction>,
    container: Option<&dyn DocumentContainer>,
) -> bool {
    functions.clear();
    if tokens.len() == 1 && tokens[0].ident() == b"none" {
        return true;
    }
    if tokens.is_empty() || tokens.len() > 32 {
        return false;
    }
    let mut args = Vec::new();
    for tok in tokens {
        if tok.ty != CV_FUNCTION {
            functions.clear();
            return false;
        }
        function_args(tok, &mut args);
        for arg in &mut args {
            if !fold_calc(arg) {
                functions.clear();
                return false;
            }
        }
        let name = lowcase(tok.name());
        let mut f = FilterFunction::default();
        let mut ok = args.len() <= 1;
        if name == b"blur" {
            f.kind = FilterKind::blur;
            if !args.is_empty() {
                ok = ok && f.radius.from_token(&args[0], f_length | f_positive, CssValues::default());
            }
        } else if name == b"hue-rotate" {
            f.kind = FilterKind::hue_rotate;
            if !args.is_empty() {
                ok = ok
                    && if args[0].ty == NUMBER && args[0].n.number == 0.0 {
                        true
                    } else {
                        match parse_angle(names, &args[0], false) {
                            Some(angle) => {
                                f.amount = angle;
                                true
                            }
                            None => false,
                        }
                    };
            }
        } else if name == b"drop-shadow" {
            f.kind = FilterKind::drop_shadow;
            let mut count = 0;
            let mut colors = false;
            ok = !args.is_empty() && args.len() <= 4;
            let mut i = 0;
            while ok && i < args.len() {
                let mut len = CssLength::default();
                if count < 3 && len.from_token(&args[i], f_length, CssValues::default()) {
                    match count {
                        0 => f.x = len,
                        1 => f.y = len,
                        _ => f.radius = len,
                    }
                    count += 1;
                    i += 1;
                    continue;
                }
                if !colors && let Some(color) = parse_color(names, &args[i], resolver(container)) {
                    f.color = color;
                    colors = true;
                    i += 1;
                    continue;
                }
                ok = false;
                i += 1;
            }
            ok = ok && count >= 2 && f.radius.val() >= 0.0;
        } else {
            const AMOUNTS: [(&[u8], FilterKind); 7] = [
                (b"brightness", FilterKind::brightness),
                (b"contrast", FilterKind::contrast),
                (b"grayscale", FilterKind::grayscale),
                (b"invert", FilterKind::invert),
                (b"opacity", FilterKind::opacity),
                (b"saturate", FilterKind::saturate),
                (b"sepia", FilterKind::sepia),
            ];
            ok = false;
            for (entry, kind) in AMOUNTS {
                if name == entry {
                    f.kind = kind;
                    f.amount = 1.0;
                    ok = args.len() <= 1
                        && (args.is_empty()
                            || match parse_number(&args[0]) {
                                Some(amount) => {
                                    f.amount = amount;
                                    true
                                }
                                None => false,
                            })
                        && f.amount >= 0.0;
                    if ok && matches!(f.kind, FilterKind::grayscale | FilterKind::invert | FilterKind::opacity | FilterKind::sepia) {
                        f.amount = cxx::min_f32(f.amount, 1.0);
                    }
                    break;
                }
            }
        }
        if !ok || !f.amount.is_finite() {
            functions.clear();
            return false;
        }
        functions.push(f);
    }
    true
}

fn fast_alpha(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

fn fast_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

fn fast_one_of(word: &[u8], list: &[&[u8]]) -> bool {
    let low = lowcase(word);
    list.contains(&&low[..])
}

/// `fast_transform_tokens(value, tokens)`: the tokens of a plain `transform` value (`none`, or up to 32 of
/// translate, rotate, scale, skew and matrix with up to 6 numbers each in px, deg, rad, turn or %) read
/// without the tokenizer, as the renderer's transform patches use it; false for anything else.
pub fn fast_transform_tokens(value: &[u8], tokens: &mut Vec<CssToken>) -> bool {
    tokens.clear();
    let size = value.len();
    if size == 0 || size > 512 {
        return false;
    }
    let ws = |c: u8| c == b' ' || c == b'\t' || c == b'\n';
    for &c in value {
        if !(fast_alpha(c) || fast_digit(c) || ws(c) || matches!(c, b'.' | b',' | b'+' | b'-' | b'(' | b')' | b'%')) {
            return false;
        }
    }
    let mut flat: Vec<CssToken> = Vec::new();
    let mut i = 0;
    let whitespace = |i: &mut usize| {
        while *i < size && ws(value[*i]) {
            *i += 1;
        }
    };
    let word = |i: &mut usize| -> Vec<u8> {
        let start = *i;
        while *i < size && fast_alpha(value[*i]) {
            *i += 1;
        }
        value[start..*i].to_vec()
    };
    let ends_ident = |i: usize| i >= size || !(fast_alpha(value[i]) || fast_digit(value[i]) || value[i] == b'-');
    whitespace(&mut i);
    if i < size && fast_alpha(value[i]) {
        let start = i;
        let name = word(&mut i);
        if lowcase(&name) == b"none" && ends_ident(i) && (i >= size || value[i] != b'(') {
            let mut tok = CssToken::with_str(IDENT, &name);
            tok.repr = value[start..i].to_vec();
            flat.push(tok);
            whitespace(&mut i);
            if i != size {
                return false;
            }
            *tokens = flat;
            return true;
        }
        i = start;
    }
    let mut functions = 0;
    while i < size {
        let name = word(&mut i);
        if name.is_empty() || i >= size || value[i] != b'(' {
            return false;
        }
        functions += 1;
        if functions > 32
            || !fast_one_of(
                &name,
                &[b"translate", b"translatex", b"translatey", b"rotate", b"scale", b"scalex", b"scaley", b"skewx", b"skewy", b"matrix"],
            )
        {
            return false;
        }
        i += 1;
        let mut f = CssToken::with_str(CV_FUNCTION, &name);
        let mut args = 0;
        loop {
            whitespace(&mut i);
            if i >= size {
                return false;
            }
            let c = value[i];
            if c == b')' {
                flat.push(f);
                i += 1;
                break;
            }
            if c == b',' {
                let mut tok = CssToken::new(COMMA);
                tok.repr = b",".to_vec();
                f.value.push(tok);
                i += 1;
                continue;
            }
            let num = i;
            if c == b'+' || c == b'-' {
                i += 1;
            }
            let digits = i;
            while i < size && fast_digit(value[i]) {
                i += 1;
            }
            let integral = i > digits;
            let mut fraction = false;
            if i + 1 < size && value[i] == b'.' && fast_digit(value[i + 1]) {
                i += 1;
                while i < size && fast_digit(value[i]) {
                    i += 1;
                }
                fraction = true;
            }
            if !integral && !fraction {
                return false;
            }
            if i - num > 32 {
                return false;
            }
            let number = convert_string_to_number(&value[num..i]);
            if !number.is_finite() || number.abs() > 1e9 {
                return false;
            }
            let ty = if fraction { CssNumberType::css_number_number } else { CssNumberType::css_number_integer };
            if i < size && value[i] == b'%' {
                i += 1;
                // css_token(PERCENTAGE, number) leaves the number type an integer.
                let mut tok = CssToken::with_number(PERCENTAGE, number as f32, CssNumberType::css_number_integer);
                tok.repr = value[num..i].to_vec();
                f.value.push(tok);
            } else if i < size && fast_alpha(value[i]) {
                let unit = word(&mut i);
                if !ends_ident(i) || !fast_one_of(&unit, &[b"px", b"deg", b"rad", b"turn"]) {
                    return false;
                }
                let mut tok = CssToken::with_number(DIMENSION, number as f32, ty);
                tok.str = unit;
                tok.repr = value[num..i].to_vec();
                f.value.push(tok);
            } else {
                let mut tok = CssToken::with_number(NUMBER, number as f32, ty);
                tok.repr = value[num..i].to_vec();
                f.value.push(tok);
            }
            args += 1;
            if args > 6 {
                return false;
            }
            if i < size && !(ws(value[i]) || value[i] == b',' || value[i] == b')') {
                return false;
            }
        }
        if args == 0 {
            return false;
        }
        whitespace(&mut i);
    }
    if functions == 0 {
        return false;
    }
    *tokens = flat;
    true
}

/// `parse_transform(tokens, functions)`: `none`, or transform functions (2D, and the 3D ones that are 2D:
/// `translate3d`, `scale3d`, `rotateZ`, `rotate3d` about an axis; `perspective()` and `translateZ()`
/// skipped), each argument's `calc()` folded where it folds; on failure there are none.
pub fn parse_transform(names: &Names, tokens: &[CssToken], functions: &mut Vec<TransformFunction>) -> bool {
    functions.clear();
    if tokens.len() == 1 && tokens[0].ident() == b"none" {
        return true;
    }
    let mut args = Vec::new();
    let none = CssValues::default();
    for tok in tokens {
        if tok.ty != CV_FUNCTION || !function_args(tok, &mut args) {
            functions.clear();
            return false;
        }
        // Math that mixes % and px does not fold: it stays a calc length, which layout resolves against the box.
        for arg in &mut args {
            fold_calc(arg);
        }
        let name = lowcase(tok.name());
        let mut f = TransformFunction::default();
        let mut ok = true;
        match &name[..] {
            b"translate" | b"translatex" | b"translatey" | b"translate3d" => {
                f.kind = TransformKind::translate;
                if name == b"translatey" {
                    ok = f.y.from_token(&args[0], f_length_percentage, none);
                } else {
                    ok = f.x.from_token(&args[0], f_length_percentage, none);
                    if ok && args.len() > 1 && name != b"translatex" {
                        ok = f.y.from_token(&args[1], f_length_percentage, none);
                    }
                }
            }
            b"scale" | b"scalex" | b"scaley" | b"scale3d" => {
                f.kind = TransformKind::scale;
                let mut sx = 1.0;
                let mut sy = 1.0;
                if name == b"scaley" {
                    match parse_number(&args[0]) {
                        Some(v) => sy = v,
                        None => ok = false,
                    }
                } else {
                    match parse_number(&args[0]) {
                        Some(v) => sx = v,
                        None => ok = false,
                    }
                    sy = if name == b"scalex" { 1.0 } else { sx };
                    if ok && args.len() > 1 && name != b"scalex" {
                        match parse_number(&args[1]) {
                            Some(v) => sy = v,
                            None => ok = false,
                        }
                    }
                }
                f.v[0] = sx;
                f.v[1] = sy;
            }
            b"rotate" | b"rotatez" => {
                f.kind = TransformKind::rotate;
                match parse_radians(names, &args[0]) {
                    Some(v) => f.v[0] = v,
                    None => ok = false,
                }
            }
            b"rotatex" | b"rotatey" => {
                f.kind = if name == b"rotatex" { TransformKind::rotate_x } else { TransformKind::rotate_y };
                match parse_radians(names, &args[0]) {
                    Some(v) => f.v[0] = v,
                    None => ok = false,
                }
            }
            b"rotate3d" => {
                let mut x = 0.0;
                let mut y = 0.0;
                let mut z = 0.0;
                ok = args.len() == 4
                    && parse_number(&args[0]).map(|v| x = v).is_some()
                    && parse_number(&args[1]).map(|v| y = v).is_some()
                    && parse_number(&args[2]).map(|v| z = v).is_some()
                    && parse_radians(names, &args[3]).map(|v| f.v[0] = v).is_some();
                if ok && x == 0.0 && y == 0.0 {
                    f.kind = TransformKind::rotate;
                    f.v[0] = if z < 0.0 { -f.v[0] } else { f.v[0] };
                } else if ok && y == 0.0 && z == 0.0 {
                    f.kind = TransformKind::rotate_x;
                } else if ok && x == 0.0 && z == 0.0 {
                    f.kind = TransformKind::rotate_y;
                } else {
                    continue;
                }
            }
            b"skew" | b"skewx" | b"skewy" => {
                f.kind = TransformKind::skew;
                if name == b"skewy" {
                    match parse_radians(names, &args[0]) {
                        Some(v) => f.v[1] = v,
                        None => ok = false,
                    }
                } else {
                    match parse_radians(names, &args[0]) {
                        Some(v) => f.v[0] = v,
                        None => ok = false,
                    }
                    if ok && args.len() > 1 && name == b"skew" {
                        match parse_radians(names, &args[1]) {
                            Some(v) => f.v[1] = v,
                            None => ok = false,
                        }
                    }
                }
            }
            b"matrix" => {
                f.kind = TransformKind::matrix;
                ok = args.len() == 6;
                let mut i = 0;
                while ok && i < 6 {
                    ok = args[i].ty == NUMBER;
                    if ok {
                        f.v[i] = args[i].n.number;
                    }
                    i += 1;
                }
            }
            b"perspective" | b"translatez" => continue,
            _ => ok = false,
        }
        if !ok {
            functions.clear();
            return false;
        }
        functions.push(f);
    }
    true
}

/// `parse_transform_origin(tokens, x, y)`: up to two of keywords and lengths, 50% 50% for what is not given.
pub fn parse_transform_origin(tokens: &[CssToken], x: &mut CssLength, y: &mut CssLength) -> bool {
    *x = CssLength::new(50.0, CssUnits::css_units_percentage);
    *y = CssLength::new(50.0, CssUnits::css_units_percentage);
    let mut x_set = false;
    for (index, tok) in tokens.iter().enumerate() {
        if index >= 2 {
            break;
        }
        let ident = tok.ident();
        let mut len = CssLength::default();
        if ident == b"left" || ident == b"right" {
            *x = CssLength::new(if ident == b"left" { 0.0 } else { 100.0 }, CssUnits::css_units_percentage);
            x_set = true;
        } else if ident == b"top" || ident == b"bottom" {
            *y = CssLength::new(if ident == b"top" { 0.0 } else { 100.0 }, CssUnits::css_units_percentage);
        } else if ident == b"center" {
            // the default
        } else if len.from_token(tok, f_length_percentage, CssValues::default()) {
            if index == 0 && !x_set {
                *x = len;
                x_set = true;
            } else {
                *y = len;
            }
        } else {
            return false;
        }
    }
    true
}

/// `parse_track_breadth(tok, type, len, fr)`.
fn parse_track_breadth(tok: &CssToken, ty: &mut TrackType, len: &mut CssLength, fr: &mut f32) -> bool {
    let ident = tok.ident();
    if ident == b"auto" {
        *ty = TrackType::automatic;
        return true;
    }
    if ident == b"min-content" {
        *ty = TrackType::min_content;
        return true;
    }
    if ident == b"max-content" {
        *ty = TrackType::max_content;
        return true;
    }
    if tok.ty == DIMENSION && lowcase(tok.unit()) == b"fr" && tok.n.number >= 0.0 {
        *ty = TrackType::fraction;
        *fr = tok.n.number;
        return true;
    }
    if len.from_token(tok, f_length_percentage | f_positive, CssValues::default()) {
        *ty = TrackType::length;
        return true;
    }
    false
}

/// `parse_track_size(tok, size)`: a breadth, `minmax()` or `fit-content()`.
fn parse_track_size(tok: &CssToken, size: &mut GridTrackSize) -> bool {
    if tok.ty == CV_FUNCTION && lowcase(tok.name()) == b"minmax" {
        let mut args = Vec::new();
        if !function_args(tok, &mut args) || args.len() != 2 {
            return false;
        }
        let mut fr = 0.0;
        if !parse_track_breadth(&args[0], &mut size.min_type, &mut size.min_len, &mut fr) || size.min_type == TrackType::fraction {
            return false;
        }
        return parse_track_breadth(&args[1], &mut size.max_type, &mut size.max_len, &mut size.fr);
    }
    if tok.ty == CV_FUNCTION && lowcase(tok.name()) == b"fit-content" {
        let mut args = Vec::new();
        if !function_args(tok, &mut args)
            || args.len() != 1
            || !size.max_len.from_token(&args[0], f_length_percentage, CssValues::default())
        {
            return false;
        }
        size.min_type = TrackType::automatic;
        size.max_type = TrackType::length;
        return true;
    }
    if !parse_track_breadth(tok, &mut size.max_type, &mut size.max_len, &mut size.fr) {
        return false;
    }
    if size.max_type == TrackType::fraction {
        size.min_type = TrackType::automatic;
    } else {
        size.min_type = size.max_type;
        size.min_len = size.max_len.clone();
    }
    true
}

/// `parse_grid_track_list(tokens, list)`: `none`, or track sizes and `repeat()`s (one `auto-fill` or
/// `auto-fit`), names skipped, at most 1000 tracks.
pub fn parse_grid_track_list(tokens: &[CssToken], list: &mut GridTrackList) -> bool {
    // What repeat() may count to is also what a whole list may hold: the layout allocates per track.
    const MAX_TRACKS: usize = 1000;
    *list = GridTrackList::default();
    if tokens.len() == 1 && tokens[0].ident() == b"none" {
        return true;
    }
    for tok in tokens {
        if tok.ty == SQUARE_BLOCK {
            continue;
        }
        if tok.ty == CV_FUNCTION && lowcase(tok.name()) == b"repeat" {
            let mut args = Vec::new();
            if !function_args(tok, &mut args) || args.len() < 2 {
                return false;
            }
            let mut sizes = Vec::new();
            for arg in &args[1..] {
                if arg.ty == SQUARE_BLOCK {
                    continue;
                }
                let mut size = GridTrackSize::default();
                if !parse_track_size(arg, &mut size) {
                    return false;
                }
                sizes.push(size);
            }
            if sizes.is_empty() {
                return false;
            }
            let count = args[0].ident();
            if count == b"auto-fill" || count == b"auto-fit" {
                if list.repeat_at >= 0 || sizes.len() > MAX_TRACKS {
                    return false;
                }
                list.repeat_at = list.tracks.len() as i32;
                list.auto_fit = count == b"auto-fit";
                list.repeat = sizes;
            } else if args[0].ty == NUMBER && args[0].n.number_type == CssNumberType::css_number_integer && args[0].n.number > 0.0 {
                let n = cxx::f32_to_int(cxx::min_f32(args[0].n.number, 1000.0));
                let mut i = 0;
                while i < n && list.tracks.len() + sizes.len() <= MAX_TRACKS {
                    list.tracks.extend(sizes.iter().cloned());
                    i += 1;
                }
            } else {
                return false;
            }
            continue;
        }
        let mut size = GridTrackSize::default();
        if !parse_track_size(tok, &mut size) {
            return false;
        }
        if list.tracks.len() < MAX_TRACKS {
            list.tracks.push(size);
        }
    }
    true
}

/// `parse_grid_placement(tokens, placement)`: `<start> [/ <end>]` of integers, `span` and `auto`.
pub fn parse_grid_placement(tokens: &[CssToken], placement: &mut GridPlacement) -> bool {
    *placement = GridPlacement::default();
    let mut part = 0;
    let mut span = false;
    for tok in tokens {
        if tok.ch() == i32::from(b'/') {
            part += 1;
            if part > 1 {
                return false;
            }
            span = false;
            continue;
        }
        let ident = tok.ident();
        if ident == b"span" {
            span = true;
            continue;
        }
        if ident == b"auto" {
            continue;
        }
        if tok.ty != NUMBER || tok.n.number_type != CssNumberType::css_number_integer || tok.n.number == 0.0 {
            return false;
        }
        let value = cxx::f32_to_int(cxx::max_f32(-1000.0, cxx::min_f32(1000.0, tok.n.number)));
        if span {
            if value < 0 {
                return false;
            }
            placement.span = value;
        } else if part == 0 {
            placement.start = value;
        } else {
            placement.end = value;
        }
    }
    true
}
