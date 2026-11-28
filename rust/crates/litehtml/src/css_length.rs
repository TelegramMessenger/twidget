//! `css_length.h` and `css_length.cpp`: `<length> | <percentage> | <number>` and keywords, and the
//! `calc()`, `min()`, `max()` and `clamp()` that only layout can finish (they mix units), kept as an
//! expression that every copy of the length shares.

use std::sync::Arc;

use crate::Px;
use crate::css_tokenizer::{CV_FUNCTION, CssNumberType, CssToken, DIMENSION, IDENT, NUMBER, PERCENTAGE, ROUND_BLOCK, WHITESPACE};
use crate::css_values::{CssUnits, CssValues, css_units_strings};
use crate::cxx;
use crate::html::lowcase;

/// `from_token` options (flags).
/// `<length>`, a unitless zero included.
pub const f_length: i32 = 1;
/// `<percentage>`
pub const f_percentage: i32 = 2;
/// `<length-percentage>`
pub const f_length_percentage: i32 = f_length | f_percentage;
/// `<number>`
pub const f_number: i32 = 4;
/// `<integer>`
pub const f_integer: i32 = 8;
/// ⩾0 (non-negative)
pub const f_positive: i32 = 16;

/// `calc_node::kind_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CalcKind {
    #[default]
    Leaf,
    /// args, each with its sign in `signs`
    Sum,
    /// args[0] times value
    Scaled,
    Min,
    Max,
    Clamp,
}

/// `calc_node`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalcNode {
    pub kind: CalcKind,
    /// leaf: the number; scaled: the factor
    pub value: f32,
    pub unit: CssUnits,
    pub args: Vec<CalcNode>,
    pub signs: Vec<i8>,
}

/// `calc_expression`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalcExpression {
    pub root: CalcNode,
}

const MAX_NODES: usize = 128;

fn node_count(root: &CalcNode) -> usize {
    let mut count = 0;
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        count += 1;
        for arg in &node.args {
            pending.push(arg);
        }
    }
    count
}

fn skip_whitespace(tokens: &[CssToken], i: &mut usize) {
    while *i < tokens.len() && tokens[*i].ty == WHITESPACE {
        *i += 1;
    }
}

fn parse_list(tokens: &[CssToken], out: &mut Vec<CalcNode>, depth: i32) -> bool {
    let mut i = 0;
    loop {
        let mut arg = CalcNode::default();
        if !parse_sum(tokens, &mut i, &mut arg, depth) {
            return false;
        }
        out.push(arg);
        skip_whitespace(tokens, &mut i);
        if i == tokens.len() {
            return out.len() <= 32;
        }
        if tokens[i].ch() != i32::from(b',') {
            return false;
        }
        i += 1;
    }
}

/// `value_index(lowcase(unit))` of the lower-case unit names.
fn units_index(unit: &[u8]) -> Option<i32> {
    if unit.is_empty() {
        return None;
    }
    css_units_strings.iter().position(|entry| entry.as_bytes().eq_ignore_ascii_case(unit)).map(|i| i as i32)
}

fn parse_atom(tokens: &[CssToken], i: &mut usize, out: &mut CalcNode, depth: i32) -> bool {
    skip_whitespace(tokens, i);
    if *i >= tokens.len() || depth > 16 {
        return false;
    }
    let tok = &tokens[*i];
    *i += 1;
    if tok.ty == NUMBER || tok.ty == PERCENTAGE {
        out.value = tok.n.number;
        out.unit = if tok.ty == NUMBER { CssUnits::css_units_none } else { CssUnits::css_units_percentage };
        return true;
    }
    if tok.ty == DIMENSION {
        let unit = units_index(tok.unit());
        let Some(unit) = unit.and_then(CssUnits::from_index) else { return false };
        if matches!(unit, CssUnits::css_units_none | CssUnits::css_units_percentage | CssUnits::css_units_calc) {
            return false;
        }
        out.value = tok.n.number;
        out.unit = unit;
        return true;
    }
    let name = if tok.ty == CV_FUNCTION { lowcase(tok.name()) } else { Vec::new() };
    if tok.ty == ROUND_BLOCK || name == b"calc" {
        let mut inner = 0;
        if !parse_sum(&tok.value, &mut inner, out, depth + 1) {
            return false;
        }
        skip_whitespace(&tok.value, &mut inner);
        return inner == tok.value.len();
    }
    if name == b"min" || name == b"max" || name == b"clamp" {
        out.kind = match &name[..] {
            b"min" => CalcKind::Min,
            b"max" => CalcKind::Max,
            _ => CalcKind::Clamp,
        };
        return parse_list(&tok.value, &mut out.args, depth + 1) && (out.kind != CalcKind::Clamp || out.args.len() == 3);
    }
    false
}

/// A plain number, unitless and constant: the only thing a product may scale by.
fn is_number(node: &CalcNode) -> bool {
    node.kind == CalcKind::Leaf && node.unit == CssUnits::css_units_none
}

fn parse_product(tokens: &[CssToken], i: &mut usize, out: &mut CalcNode, depth: i32) -> bool {
    if !parse_atom(tokens, i, out, depth) {
        return false;
    }
    loop {
        let mut probe = *i;
        skip_whitespace(tokens, &mut probe);
        if probe >= tokens.len() || (tokens[probe].ch() != i32::from(b'*') && tokens[probe].ch() != i32::from(b'/')) {
            return true;
        }
        let divide = tokens[probe].ch() == i32::from(b'/');
        *i = probe + 1;
        let mut right = CalcNode::default();
        if !parse_atom(tokens, i, &mut right, depth) {
            return false;
        }
        if divide || is_number(&right) {
            if !is_number(&right) || (divide && right.value == 0.0) {
                return false;
            }
            let factor = if divide { 1.0 / right.value } else { right.value };
            if is_number(out) || out.kind == CalcKind::Scaled {
                out.value *= factor;
                continue;
            }
            let inner = std::mem::take(out);
            *out = CalcNode { kind: CalcKind::Scaled, value: factor, args: vec![inner], ..CalcNode::default() };
        } else if is_number(out) {
            let factor = out.value;
            *out = CalcNode { kind: CalcKind::Scaled, value: factor, args: vec![right], ..CalcNode::default() };
        } else {
            return false;
        }
    }
}

fn parse_sum(tokens: &[CssToken], i: &mut usize, out: &mut CalcNode, depth: i32) -> bool {
    let mut first = CalcNode::default();
    if !parse_product(tokens, i, &mut first, depth) {
        return false;
    }
    let mut sum = CalcNode { kind: CalcKind::Sum, args: vec![first], signs: vec![1], ..CalcNode::default() };
    loop {
        let mut probe = *i;
        skip_whitespace(tokens, &mut probe);
        if probe >= tokens.len() || (tokens[probe].ch() != i32::from(b'+') && tokens[probe].ch() != i32::from(b'-')) {
            break;
        }
        *i = probe + 1;
        let mut term = CalcNode::default();
        if !parse_product(tokens, i, &mut term, depth) {
            return false;
        }
        sum.args.push(term);
        sum.signs.push(if tokens[probe].ch() == i32::from(b'-') { -1 } else { 1 });
    }
    *out = if sum.args.len() == 1 { sum.args.pop().unwrap_or_default() } else { sum };
    true
}

/// `calc_convert_fn`: a leaf's value in another unit than % and px, in px.
pub type CalcConvert<'a> = &'a dyn Fn(f32, CssUnits) -> f32;

fn evaluate(node: &CalcNode, base: f32, convert: Option<CalcConvert>) -> f32 {
    match node.kind {
        CalcKind::Leaf => {
            let unit = node.unit;
            if unit == CssUnits::css_units_percentage {
                return base * node.value / 100.0;
            }
            match convert {
                Some(convert) if unit != CssUnits::css_units_none && unit != CssUnits::css_units_px => convert(node.value, unit),
                _ => node.value,
            }
        }
        CalcKind::Sum => {
            let mut total = 0.0;
            for i in 0..node.args.len() {
                total += f32::from(node.signs[i]) * evaluate(&node.args[i], base, convert);
            }
            total
        }
        CalcKind::Scaled => node.value * evaluate(&node.args[0], base, convert),
        CalcKind::Clamp => {
            // The C++ leaves the order of the last two evaluations to the compiler (MSVC and GCC go right to
            // left, clang left to right); it shows only through a `convert` with effects.
            let low = evaluate(&node.args[0], base, convert);
            let middle = evaluate(&node.args[1], base, convert);
            let high = evaluate(&node.args[2], base, convert);
            cxx::max_f32(low, cxx::min_f32(middle, high))
        }
        CalcKind::Min | CalcKind::Max => {
            let mut value = evaluate(&node.args[0], base, convert);
            for i in 1..node.args.len() {
                let arg = evaluate(&node.args[i], base, convert);
                value = if node.kind == CalcKind::Min { cxx::min_f32(value, arg) } else { cxx::max_f32(value, arg) };
            }
            value
        }
    }
}

fn has_percent(node: &CalcNode) -> bool {
    if node.kind == CalcKind::Leaf {
        return node.unit == CssUnits::css_units_percentage;
    }
    node.args.iter().any(has_percent)
}

fn is_converted(node: &CalcNode) -> bool {
    if node.kind == CalcKind::Leaf {
        return matches!(node.unit, CssUnits::css_units_percentage | CssUnits::css_units_none | CssUnits::css_units_px);
    }
    node.args.iter().all(is_converted)
}

fn convert_leaves(node: &mut CalcNode, convert: CalcConvert) {
    if node.kind == CalcKind::Leaf {
        let unit = node.unit;
        if unit != CssUnits::css_units_percentage && unit != CssUnits::css_units_none && unit != CssUnits::css_units_px {
            node.value = convert(node.value, unit);
            node.unit = CssUnits::css_units_px;
        }
        return;
    }
    for arg in &mut node.args {
        convert_leaves(arg, convert);
    }
}

/// `calc_evaluate(expression, percent_base, convert)`: the expression's value, 0 when it is not finite.
pub fn calc_evaluate(expression: Option<&CalcExpression>, percent_base: Px, convert: Option<CalcConvert>) -> Px {
    let Some(expression) = expression else { return Px(0.0) };
    let value = evaluate(&expression.root, percent_base.value(), convert);
    Px(if value.is_finite() { value } else { 0.0 })
}

/// `calc_resolve_units(expression, convert, px)`: the expression with every leaf but % in px, or None when
/// no % is left, with `px` its value then (0 when not finite). An expression already converted that has a %
/// is the same one (shared). The C++ throws `bad_function_call` for an expression to convert and an empty
/// `convert`; here `convert` is always there.
pub fn calc_resolve_units(expression: Option<&Arc<CalcExpression>>, convert: CalcConvert, px: &mut f32) -> Option<Arc<CalcExpression>> {
    *px = 0.0;
    let expression = expression?;
    let converted = is_converted(&expression.root);
    if converted && has_percent(&expression.root) {
        return Some(Arc::clone(expression));
    }
    let mut node = expression.root.clone();
    if !converted {
        convert_leaves(&mut node, convert);
    }
    if !has_percent(&node) {
        *px = evaluate(&node, 0.0, None);
        if !px.is_finite() {
            *px = 0.0;
        }
        return None;
    }
    Some(Arc::new(CalcExpression { root: node }))
}

/// `clamp_length(px)`: the longest length is 2^25 px, about what browsers lay out, so sums of lengths
/// stay finite; NaN is 0.
pub fn clamp_length(px: f32) -> f32 {
    if px.is_nan() { 0.0 } else { cxx::min_f32(cxx::max_f32(px, -33554432.0), 33554432.0) }
}

fn is_math_function(name: &[u8]) -> bool {
    matches!(&lowcase(name)[..], b"calc" | b"min" | b"max" | b"clamp")
}

/// `css_length`: a value and its unit, or a keyword's index (`predef`), or a `calc` expression.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CssLength {
    /// The C++ union of `float m_value` and `int m_predef`, as its bits.
    raw: u32,
    units: CssUnits,
    is_predefined: bool,
    calc: Option<Arc<CalcExpression>>,
}

impl CssLength {
    /// `css_length(val, units)`.
    pub fn new(val: f32, units: CssUnits) -> CssLength {
        CssLength { raw: val.to_bits(), units, ..CssLength::default() }
    }

    /// `operator=(float)`: a px value.
    pub fn assign(&mut self, val: f32) {
        self.raw = val.to_bits();
        self.units = CssUnits::css_units_px;
        self.is_predefined = false;
        self.calc = None;
    }

    /// `is_predefined()`.
    pub fn is_predefined(&self) -> bool {
        self.is_predefined
    }

    /// `predef(val)`: a keyword's index; the unit stays what it was.
    pub fn set_predef(&mut self, val: i32) {
        self.raw = val as u32;
        self.is_predefined = true;
        self.calc = None;
    }

    /// `predef()`: the keyword's index, 0 for a value.
    pub fn predef(&self) -> i32 {
        if self.is_predefined { self.raw as i32 } else { 0 }
    }

    /// `predef_value(val)`.
    pub fn predef_value(val: i32) -> CssLength {
        let mut len = CssLength::default();
        len.set_predef(val);
        len
    }

    /// `set_value(val, units)`.
    pub fn set_value(&mut self, val: f32, units: CssUnits) {
        self.raw = val.to_bits();
        self.is_predefined = false;
        self.units = units;
        self.calc = None;
    }

    /// `set_calc(expression)`.
    pub fn set_calc(&mut self, expression: Option<Arc<CalcExpression>>) {
        self.raw = 0.0f32.to_bits();
        self.is_predefined = false;
        self.units = CssUnits::css_units_calc;
        self.calc = expression;
    }

    /// `calc()`: the expression, shared.
    pub fn calc(&self) -> Option<&Arc<CalcExpression>> {
        self.calc.as_ref()
    }

    /// `val()`: the value; a calc length's against no size (its % parts count as zero); 0 for a keyword.
    pub fn val(&self) -> f32 {
        if !self.is_predefined {
            return if self.units() == CssUnits::css_units_calc {
                calc_evaluate(self.calc.as_deref(), Px(0.0), None).value()
            } else {
                f32::from_bits(self.raw)
            };
        }
        0.0
    }

    /// `units()`.
    pub fn units(&self) -> CssUnits {
        self.units
    }

    /// `calc_percent(width)`: a % of `width`, a calc against it, a value as it is; 0 for a keyword.
    pub fn calc_percent(&self, width: Px) -> Px {
        if !self.is_predefined() {
            if self.units() == CssUnits::css_units_percentage {
                return Px(clamp_length(width.value() * f32::from_bits(self.raw) / 100.0));
            }
            if self.units() == CssUnits::css_units_calc {
                return Px(clamp_length(calc_evaluate(self.calc.as_deref(), width, None).value()));
            }
            return Px(self.val());
        }
        Px(0.0)
    }

    /// `from_token(token, options, predefined_keywords)`: the token as what `options` allow, or false (and
    /// the length as it was).
    pub fn from_token(&mut self, token: &CssToken, options: i32, keywords: CssValues) -> bool {
        if (options & f_positive) != 0 && matches!(token.ty, NUMBER | DIMENSION | PERCENTAGE) && token.n.number < 0.0 {
            return false;
        }

        // Math the stylesheet could not fold (style.cpp, fold_math) mixes units: it waits for layout.
        if token.ty == CV_FUNCTION && (options & f_length) != 0 && (options & f_percentage) != 0 && is_math_function(token.name()) {
            let mut node = CalcNode::default();
            let mut i = 0;
            let single = vec![token.clone()];
            if !parse_atom(&single, &mut i, &mut node, 0) || is_number(&node) || node_count(&node) > MAX_NODES {
                return false;
            }
            self.set_calc(Some(Arc::new(CalcExpression { root: node })));
            return true;
        }

        if token.ty == IDENT {
            let Some(idx) = keywords.value_index(&lowcase(token.name())) else { return false };
            self.set_predef(idx);
            return true;
        }
        if token.ty == DIMENSION {
            if (options & f_length) == 0 {
                return false;
            }

            // note: 1none and 1\% are invalid
            let Some(unit) = units_index(token.unit()).and_then(CssUnits::from_index) else { return false };
            if matches!(unit, CssUnits::css_units_none | CssUnits::css_units_percentage | CssUnits::css_units_calc) {
                return false;
            }

            self.set_value(token.n.number, unit);
            return true;
        }
        if token.ty == PERCENTAGE {
            if (options & f_percentage) == 0 {
                return false;
            }

            self.set_value(token.n.number, CssUnits::css_units_percentage);
            return true;
        }
        if token.ty == NUMBER {
            // if token is a nonzero number and neither f_number nor f_integer is specified in the options
            if (options & (f_number | f_integer)) == 0 && Px(token.n.number) != Px(0.0) {
                return false;
            }
            // if token is a zero number and neither of f_number, f_integer or f_length are specified in the options
            if (options & (f_number | f_integer | f_length)) == 0 && Px(token.n.number) == Px(0.0) {
                return false;
            }
            if (options & f_integer) != 0 && token.n.number_type != CssNumberType::css_number_integer {
                return false;
            }

            self.set_value(token.n.number, CssUnits::css_units_none);
            return true;
        }
        false
    }

    /// `to_string()`: `def(N)` for a keyword, else the value as `%f` and the unit in braces.
    pub fn to_string(&self) -> Vec<u8> {
        if self.is_predefined {
            let mut out = b"def(".to_vec();
            out.extend(cxx::to_string_int(self.raw as i32));
            out.push(b')');
            return out;
        }
        let mut res = cxx::to_string_f64(f64::from(f32::from_bits(self.raw)));
        res.push(b'{');
        res.extend_from_slice(CssValues::from(&css_units_strings).value_by_index(self.units().index() as usize).as_bytes());
        res.push(b'}');
        res
    }
}
