//! `css_tokenizer.h` and `css_tokenizer.cpp`: CSS tokens (https://www.w3.org/TR/css-syntax-3/#tokenization)
//! and component values, the "fat" tokens the parser makes of them, and the number conversion of the
//! syntax spec as litehtml writes it.
//!
//! A token's type is an `i32` as in the C++, where it shares its storage with `ch`: a delim token's type is
//! its code point, `:;,()[]{}` are their characters, and the other kinds are the negative values below.

use crate::html::{digit_value, is_digit, lowcase};

/// `css_token_type`, and the code point of a delim token.
pub type CssTokenType = i32;

pub const WHITESPACE: CssTokenType = b' ' as i32;
/// `_EOF`, C's `EOF`.
pub const EOF: CssTokenType = -1;
pub const LEFT_BRACE: CssTokenType = b'{' as i32;
pub const RIGHT_BRACE: CssTokenType = b'}' as i32;
pub const LEFT_BRACKET: CssTokenType = b'[' as i32;
pub const RIGHT_BRACKET: CssTokenType = b']' as i32;
pub const LEFT_PAREN: CssTokenType = b'(' as i32;
pub const RIGHT_PAREN: CssTokenType = b')' as i32;
pub const COLON: CssTokenType = b':' as i32;
pub const SEMICOLON: CssTokenType = b';' as i32;
pub const COMMA: CssTokenType = b',' as i32;
pub const BANG: CssTokenType = b'!' as i32;
pub const DOT: CssTokenType = b'.' as i32;
pub const AMPERSAND: CssTokenType = b'&' as i32;

/// Below any code point.
pub const IDENT: CssTokenType = -20;
/// `calc(`
pub const FUNCTION: CssTokenType = -19;
/// `@media`
pub const AT_KEYWORD: CssTokenType = -18;
/// `#foo`
pub const HASH: CssTokenType = -17;
/// `"xxx"` or `'xxx'`
pub const STRING: CssTokenType = -16;
pub const BAD_STRING: CssTokenType = -15;
/// `url(x.com)`, but not `url("x.com")`, which is a function, a string and `)`.
pub const URL: CssTokenType = -14;
pub const BAD_URL: CssTokenType = -13;
/// `25`
pub const NUMBER: CssTokenType = -12;
/// `25%`
pub const PERCENTAGE: CssTokenType = -11;
/// `25px`
pub const DIMENSION: CssTokenType = -10;
/// `<!--`
pub const CDO: CssTokenType = -9;
/// `-->`
pub const CDC: CssTokenType = -8;

/// Component values (https://www.w3.org/tr/css-syntax-3/#component-value): a function and the simple
/// blocks.
pub const CV_FUNCTION: CssTokenType = -100;
pub const CURLY_BLOCK: CssTokenType = -100 - b'{' as i32;
pub const ROUND_BLOCK: CssTokenType = -100 - b'(' as i32;
pub const SQUARE_BLOCK: CssTokenType = -100 - b'[' as i32;

/// `css_number_type`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CssNumberType {
    #[default]
    css_number_integer,
    css_number_number,
}

/// `css_hash_type`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CssHashType {
    #[default]
    css_hash_unrestricted,
    css_hash_id,
}

/// `css_token::number`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Number {
    /// NUMBER, PERCENTAGE, DIMENSION
    pub number: f32,
    /// NUMBER, DIMENSION
    pub number_type: CssNumberType,
}

/// `css_token`: a token or a component value.
///
/// The C++ keeps `hash_type` and `n` in one union; no reader takes the number of a hash or the hash type of
/// a number, so they are two fields here. `clone` is the C++ copy constructor: it keeps `n` only for a
/// NUMBER, PERCENTAGE or DIMENSION, `hash_type` only for a HASH and `value` only for a component value,
/// the rest zero.
#[derive(Debug, Default, PartialEq)]
pub struct CssToken {
    /// `type`, and `ch` for a delim or `:;,()[]{}`.
    pub ty: CssTokenType,
    /// `m_str`: the value of a STRING or URL, the name of a HASH, IDENT, AT_KEYWORD, FUNCTION or
    /// CV_FUNCTION, the unit of a DIMENSION.
    pub str: Vec<u8>,
    pub n: Number,
    pub hash_type: CssHashType,
    /// The contents of a CV_FUNCTION or a block.
    pub value: Vec<CssToken>,
    /// The text it was read from: https://www.w3.org/TR/css-syntax-3/#representation
    pub repr: Vec<u8>,
}

impl Clone for CssToken {
    fn clone(&self) -> CssToken {
        let mut copy = CssToken { ty: self.ty, str: self.str.clone(), repr: self.repr.clone(), ..CssToken::default() };
        match self.ty {
            HASH => copy.hash_type = self.hash_type,
            NUMBER | PERCENTAGE | DIMENSION => copy.n = self.n,
            CV_FUNCTION | CURLY_BLOCK | ROUND_BLOCK | SQUARE_BLOCK => copy.value = self.value.clone(),
            _ => {}
        }
        copy
    }
}

impl CssToken {
    /// `css_token(type)`.
    pub fn new(ty: CssTokenType) -> CssToken {
        CssToken { ty, ..CssToken::default() }
    }

    /// `css_token(type, number, number_type)`.
    pub fn with_number(ty: CssTokenType, number: f32, number_type: CssNumberType) -> CssToken {
        CssToken { ty, n: Number { number, number_type }, ..CssToken::default() }
    }

    /// `css_token(type, str)`.
    pub fn with_str(ty: CssTokenType, str: &[u8]) -> CssToken {
        CssToken { ty, str: str.to_vec(), ..CssToken::default() }
    }

    /// `ch`: the delim's code point (the type).
    pub fn ch(&self) -> i32 {
        self.ty
    }

    /// `name()`, `unit()` and `str()` are one string.
    pub fn name(&self) -> &[u8] {
        &self.str
    }

    pub fn unit(&self) -> &[u8] {
        &self.str
    }

    /// `is_component_value()`.
    pub fn is_component_value(&self) -> bool {
        self.ty <= CV_FUNCTION
    }

    /// `ident()`: an IDENT's name, lowercase unless it is a custom property name (`--…`); empty for any
    /// other token.
    pub fn ident(&self) -> Vec<u8> {
        if self.ty != IDENT {
            return Vec::new();
        }
        if self.str.starts_with(b"--") { self.str.clone() } else { lowcase(&self.str) }
    }

    /// `get_repr(insert_spaces)`: the text, a component value's made again from its parts.
    pub fn get_repr(&self, insert_spaces: bool) -> Vec<u8> {
        if !self.is_component_value() {
            return self.repr.clone();
        }
        if self.ty == CV_FUNCTION {
            let mut out = self.str.clone();
            out.push(b'(');
            out.extend(get_repr(&self.value, 0, -1, insert_spaces));
            out.push(b')');
            return out;
        }
        let opening_bracket = (-self.ty - 100) as u8;
        let closing_bracket = mirror(opening_bracket);
        let mut out = vec![opening_bracket];
        out.extend(get_repr(&self.value, 0, -1, insert_spaces));
        out.push(closing_bracket);
        out
    }
}

/// `css_token_vector`.
pub type CssTokenVector = Vec<CssToken>;

/// `mirror(c)`: the closing bracket of an opening one; any other character itself.
pub fn mirror(c: u8) -> u8 {
    match c {
        b'{' => b'}',
        b'[' => b']',
        b'(' => b')',
        _ => c,
    }
}

/// `get_repr(tokens, index, count, insert_spaces)`: the representations of `count` tokens from `index`
/// (-1: to the end) concatenated, with a space between them when asked.
pub fn get_repr(tokens: &[CssToken], index: i32, count: i32, insert_spaces: bool) -> Vec<u8> {
    let count = if count == -1 { tokens.len() as i32 - index } else { count };
    let mut str = Vec::new();
    for i in index..index + count {
        str.extend(tokens[i as usize].get_repr(insert_spaces));
        if insert_spaces {
            str.push(b' ');
        }
    }
    if insert_spaces {
        str.pop();
    }
    str
}

/// `css_tokenizer::convert_string_to_number(str)` (https://www.w3.org/TR/css-syntax-3/#convert-string-to-number)
/// on a C string: a sign, integer digits, a point, fraction digits, an exponent mark, its sign and digits,
/// each optional and read in that order, then s·(i + f·10⁻ᵈ)·10ᵗᵉ in doubles, with the C library's `pow`. A
/// zero with an exponent past the double range is 0·∞, a NaN.
pub fn convert_string_to_number(str: &[u8]) -> f64 {
    let at = |i: usize| i32::from(str.get(i).copied().unwrap_or(0));
    let mut p = 0;

    // 1. A sign.
    let mut s = 1.0;
    if at(p) == i32::from(b'-') {
        s = -1.0;
        p += 1;
    } else if at(p) == i32::from(b'+') {
        p += 1;
    }

    // 2. An integer part.
    let mut i = 0.0;
    while is_digit(at(p)) {
        i = i * 10.0 + f64::from(digit_value(at(p)));
        p += 1;
    }

    // 3. A decimal point.
    if at(p) == i32::from(b'.') {
        p += 1;
    }

    // 4. A fractional part, and d the number of its digits.
    let mut f = 0.0;
    let mut d = 0.0;
    while is_digit(at(p)) {
        f = f * 10.0 + f64::from(digit_value(at(p)));
        p += 1;
        d += 1.0;
    }

    // 5. An exponent indicator.
    if at(p) == i32::from(b'e') || at(p) == i32::from(b'E') {
        p += 1;
    }

    // 6. An exponent sign.
    let mut t = 1.0;
    if at(p) == i32::from(b'-') {
        t = -1.0;
        p += 1;
    } else if at(p) == i32::from(b'+') {
        p += 1;
    }

    // 7. An exponent.
    let mut e = 0.0;
    while is_digit(at(p)) {
        e = e * 10.0 + f64::from(digit_value(at(p)));
        p += 1;
    }

    // Return the number s·(i + f·10ᐨᵈ)·10ᵗᵉ.
    s * (i + f * 10f64.powf(-d)) * 10f64.powf(t * e)
}
