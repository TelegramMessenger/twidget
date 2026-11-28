//! `css_tokenizer.h` and `css_tokenizer.cpp`: CSS tokens (https://www.w3.org/TR/css-syntax-3/#tokenization)
//! and component values, the "fat" tokens the parser makes of them, and the number conversion of the
//! syntax spec as litehtml writes it.
//!
//! A token's type is an `i32` as in the C++, where it shares its storage with `ch`: a delim token's type is
//! its code point, `:;,()[]{}` are their characters, and the other kinds are the negative values below.

use crate::html::{digit_value, is_digit, is_hex_digit, is_letter, is_surrogate, lowcase, lowcase_int, match_at};
use crate::utf8_strings::{append_char, read_utf8_char};

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

/// `css_parse_error(msg)`: litehtml reports nothing.
fn css_parse_error(_msg: &str) {}

/// `css_tokenizer`: one CSS text into tokens (https://www.w3.org/TR/css-syntax-3/#tokenization), as the fork
/// reads it. It expects what `filter_code_points` and UTF-8 decoding leave (no NUL, no `\r`, no `\f`,
/// well-formed UTF-8) and does not check: a NUL, or a sequence that decodes to 0, ends the text there;
/// `\r` and `\f` are no whitespace to it. It decides with decoded code points but reads a number's bytes,
/// so a token that starts with an overlong form of a digit (`C0 B0`, `E0 80 B0`), or with `+`, `-` or `.`
/// before one, is a number that takes nothing, and `tokenize` makes such empty tokens without end, as the
/// C++ does. Only text that skipped UTF-8 decoding can hold one.
#[derive(Debug)]
pub struct CssTokenizer {
    /// The input stream: https://www.w3.org/TR/css-syntax-3/#input-stream
    str: Vec<u8>,
    /// The next input char: https://www.w3.org/TR/css-syntax-3/#next-input-code-point
    index: usize,
    prev_index: usize,
    /// The current input code point, kept to tell "just read the last char" from "already at the end and
    /// read NUL" when unconsuming; without it the tokenizer loops forever on "a".
    current_char: i32,
}

/// `three_chars`.
#[derive(Clone, Copy, Debug)]
struct ThreeChars {
    _1: i32,
    _2: i32,
    _3: i32,
}

impl CssTokenizer {
    pub fn new(input: &[u8]) -> CssTokenizer {
        CssTokenizer { str: input.to_vec(), index: 0, prev_index: 0, current_char: 0 }
    }

    /// `str[i]` as the C++ reads it: the byte, a NUL at the end (std::string keeps one there).
    fn at(&self, i: usize) -> i32 {
        i32::from(self.str.get(i).copied().unwrap_or(0))
    }

    /// https://www.w3.org/TR/css-syntax-3/#whitespace (`\r` and `\f` were made `\n` by filter_code_points)
    fn is_whitespace(ch: i32) -> bool {
        ch == i32::from(b'\n') || ch == i32::from(b'\t') || ch == i32::from(b' ')
    }

    /// https://www.w3.org/TR/css-syntax-3/#non-printable-code-point
    fn is_non_printable_code_point(ch: i32) -> bool {
        (0..=8).contains(&ch) || ch == 0xB || (0xE..=0x1F).contains(&ch) || ch == 0x7F
    }

    /// https://www.w3.org/TR/css-syntax-3/#ident-start-code-point
    fn is_ident_start_code_point(ch: i32) -> bool {
        is_letter(ch) || ch >= 0x80 || ch == i32::from(b'_')
    }

    /// https://www.w3.org/TR/css-syntax-3/#ident-code-point
    fn is_ident_code_point(ch: i32) -> bool {
        Self::is_ident_start_code_point(ch) || is_digit(ch) || ch == i32::from(b'-')
    }

    /// Consumes the next input code point and returns it; at a NUL or the end `index` stays.
    fn consume_char(&mut self) -> i32 {
        self.prev_index = self.index;
        self.current_char = read_utf8_char(&self.str, &mut self.index) as i32;
        self.current_char
    }

    /// https://www.w3.org/TR/css-syntax-3/#reconsume-the-current-input-code-point
    fn unconsume_char(&mut self) {
        // see current_char
        if self.current_char == 0 {
            return;
        }
        self.index = self.prev_index;
    }

    fn peek_char(&self) -> i32 {
        let mut i = self.index;
        read_utf8_char(&self.str, &mut i) as i32
    }

    fn peek_chars(&self) -> ThreeChars {
        let mut i = self.index;
        let first = read_utf8_char(&self.str, &mut i) as i32;
        let second = read_utf8_char(&self.str, &mut i) as i32;
        let third = read_utf8_char(&self.str, &mut i) as i32;
        ThreeChars { _1: first, _2: second, _3: third }
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-comments
    fn consume_comments(&mut self) {
        loop {
            if self.at(self.index) == i32::from(b'/') && self.at(self.index + 1) == i32::from(b'*') {
                let from = self.index + 2;
                let found = self.str.get(from..).and_then(|rest| rest.windows(2).position(|w| w == b"*/"));
                match found {
                    Some(i) => self.index = from + i + 2,
                    None => {
                        self.index = self.str.len();
                        css_parse_error("eof in comment");
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-escaped-code-point, after the `\` and before no newline.
    fn consume_escaped_code_point(&mut self) -> i32 {
        // Consume the next input code point.
        let mut ch = self.consume_char();

        if is_hex_digit(ch) {
            let mut number = digit_value(ch);
            // Consume as many hex digits as possible, but no more than 5.
            let mut max = 5;
            while max > 0 && {
                max -= 1;
                is_hex_digit(self.at(self.index))
            } {
                ch = self.consume_char();
                number = number * 16 + digit_value(ch);
            }
            // If the next input code point is whitespace, consume it as well.
            if Self::is_whitespace(self.at(self.index)) {
                self.consume_char();
            }
            // Zero, a surrogate or past the maximum allowed code point is U+FFFD.
            if number == 0 || is_surrogate(number) || number > 0x10FFFF {
                return 0xFFFD;
            }
            return number;
        }
        if ch == 0 {
            // EOF: a parse error, U+FFFD.
            css_parse_error("eof in escaped codepoint");
            return 0xFFFD;
        }
        // anything else: the current input code point
        ch
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-string-token
    fn consume_string_token(&mut self, ending_code_point: i32) -> CssToken {
        let mut token = CssToken::new(STRING);

        loop {
            let ch = self.consume_char();
            match ch {
                0 => {
                    // EOF: a parse error; the string as it is.
                    css_parse_error("eof in string");
                    return token;
                }
                0x0A => {
                    // A newline: a parse error; reconsume it and give a <bad-string-token>.
                    css_parse_error("newline in string");
                    self.unconsume_char();
                    return CssToken::new(BAD_STRING);
                }
                0x5C => {
                    // `\` before EOF: nothing; before a newline: consume it; else an escaped code point.
                    if self.at(self.index) == 0 {
                    } else if self.at(self.index) == i32::from(b'\n') {
                        self.index += 1;
                    } else {
                        let code = self.consume_escaped_code_point();
                        append_char(&mut token.str, code as u32);
                    }
                }
                _ => {
                    if ch == ending_code_point {
                        return token;
                    }
                    append_char(&mut token.str, ch as u32);
                }
            }
        }
    }

    /// https://www.w3.org/TR/css-syntax-3/#would-start-an-identifier
    fn would_start_ident_sequence(chars: ThreeChars) -> bool {
        let (c1, c2, c3) = (chars._1, chars._2, chars._3);

        if c1 == i32::from(b'-') {
            // An ident-start code point or `-` second, or a valid escape second and third.
            return Self::is_ident_start_code_point(c2) || c2 == i32::from(b'-') || (c2 == i32::from(b'\\') && c3 != i32::from(b'\n'));
        }
        if Self::is_ident_start_code_point(c1) {
            return true;
        }
        if c1 == i32::from(b'\\') {
            // A valid escape first and second.
            return c2 != i32::from(b'\n');
        }
        false
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-name
    fn consume_ident_sequence(&mut self) -> Vec<u8> {
        let mut result = Vec::new();

        loop {
            let ch = self.consume_char();

            if Self::is_ident_code_point(ch) {
                append_char(&mut result, ch as u32);
            }
            // a valid escape: ch has been consumed already, the code point after it is the next input one
            else if ch == i32::from(b'\\') && self.at(self.index) != i32::from(b'\n') {
                let code = self.consume_escaped_code_point();
                append_char(&mut result, code as u32);
            } else {
                self.unconsume_char();
                return result;
            }
        }
    }

    /// https://www.w3.org/TR/css-syntax-3/#starts-with-a-number
    fn would_start_a_number(x: i32, y: i32, z: i32) -> bool {
        if x == i32::from(b'+') || x == i32::from(b'-') {
            // A digit second, or `.` second and a digit third.
            if is_digit(y) {
                return true;
            }
            if y == i32::from(b'.') && is_digit(z) {
                return true;
            }
            return false;
        }
        if x == i32::from(b'.') {
            return is_digit(y);
        }
        is_digit(x)
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-number: the value and its type, from the bytes as they
    /// come (no UTF-8 decoding: a number is ASCII).
    fn consume_number(&mut self) -> (f64, CssNumberType) {
        let mut number_type = CssNumberType::css_number_integer;
        let mut repr = Vec::new();
        let ascii = |c: u8| i32::from(c);

        // 2. A sign.
        if self.at(self.index) == ascii(b'+') || self.at(self.index) == ascii(b'-') {
            repr.push(self.str[self.index]);
            self.index += 1;
        }

        // 3. Digits.
        while is_digit(self.at(self.index)) {
            repr.push(self.str[self.index]);
            self.index += 1;
        }

        // 4. `.` and a digit, then digits.
        if self.at(self.index) == ascii(b'.') && is_digit(self.at(self.index + 1)) {
            repr.extend_from_slice(&self.str[self.index..self.index + 2]);
            self.index += 2;
            number_type = CssNumberType::css_number_number;
            while is_digit(self.at(self.index)) {
                repr.push(self.str[self.index]);
                self.index += 1;
            }
        }

        // 5. `e` or `E`, a sign perhaps, and a digit, then digits.
        let e = lowcase_int(self.at(self.index)) == ascii(b'e');
        let a =
            e && (self.at(self.index + 1) == ascii(b'+') || self.at(self.index + 1) == ascii(b'-')) && is_digit(self.at(self.index + 2));
        let b = e && is_digit(self.at(self.index + 1));

        if a || b {
            let n = if a { 3 } else { 2 };
            repr.extend_from_slice(&self.str[self.index..self.index + n]);
            self.index += n;
            number_type = CssNumberType::css_number_number;
            while is_digit(self.at(self.index)) {
                repr.push(self.str[self.index]);
                self.index += 1;
            }
        }

        // 6. Convert repr to a number.
        (convert_string_to_number(&repr), number_type)
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-numeric-token: the number narrowed to float.
    fn consume_numeric_token(&mut self) -> CssToken {
        let (number, number_type) = self.consume_number();
        let number = number as f32;

        // An ident sequence next: a <dimension-token> with it as the unit.
        if Self::would_start_ident_sequence(self.peek_chars()) {
            let mut token = CssToken::with_number(DIMENSION, number, number_type);
            token.str = self.consume_ident_sequence();
            return token;
        }

        // `%` next: a <percentage-token>, whose type flag stays "integer".
        if self.at(self.index) == i32::from(b'%') {
            self.index += 1;
            return CssToken::with_number(PERCENTAGE, number, CssNumberType::css_number_integer);
        }

        CssToken::with_number(NUMBER, number, number_type)
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-remnants-of-bad-url
    fn consume_remnants_of_bad_url(&mut self) {
        loop {
            let ch = self.consume_char();
            if ch == i32::from(b')') || ch == 0 {
                return;
            }
            // a valid escape (ch consumed already)
            if ch == i32::from(b'\\') && self.at(self.index) != i32::from(b'\n') {
                self.consume_escaped_code_point();
            }
        }
    }

    fn bad_url(&mut self) -> CssToken {
        css_parse_error("invalid char in unquoted url");
        self.consume_remnants_of_bad_url();
        CssToken::new(BAD_URL)
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-url-token
    fn consume_url_token(&mut self) -> CssToken {
        let mut token = CssToken::new(URL);

        // Consume as much whitespace as possible.
        while Self::is_whitespace(self.at(self.index)) {
            self.index += 1;
        }

        loop {
            let ch = self.consume_char();
            match ch {
                0x29 => return token,
                0 => {
                    // EOF: a parse error; the url as it is.
                    css_parse_error("eof in unquoted url");
                    return token;
                }
                0x0A | 0x09 | 0x20 => {
                    while Self::is_whitespace(self.at(self.index)) {
                        self.index += 1;
                    }
                    // `)` or EOF next: consume it and give the url (EOF is a parse error).
                    if self.at(self.index) == i32::from(b')') || self.at(self.index) == 0 {
                        if self.at(self.index) == 0 {
                            css_parse_error("eof in unquoted url");
                        } else {
                            self.index += 1;
                        }
                        return token;
                    }
                    // else the remnants of a bad url
                    self.consume_remnants_of_bad_url();
                    return CssToken::new(BAD_URL);
                }
                0x22 | 0x27 | 0x28 => return self.bad_url(),
                0x5C => {
                    // A valid escape: its code point; else a parse error and a bad url.
                    if self.at(self.index) != i32::from(b'\n') {
                        let code = self.consume_escaped_code_point();
                        append_char(&mut token.str, code as u32);
                    } else {
                        css_parse_error("escaped newline in unquoted url");
                        self.consume_remnants_of_bad_url();
                        return CssToken::new(BAD_URL);
                    }
                }
                _ => {
                    if Self::is_non_printable_code_point(ch) {
                        return self.bad_url();
                    }
                    append_char(&mut token.str, ch as u32);
                }
            }
        }
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-ident-like-token
    fn consume_ident_like_token(&mut self) -> CssToken {
        let name = self.consume_ident_sequence();
        if self.at(self.index) != i32::from(b'(') {
            return CssToken { ty: IDENT, str: name, ..CssToken::default() };
        }
        self.index += 1;

        // `url` in any ASCII case and `(` next: consume it.
        if name.eq_ignore_ascii_case(b"url") {
            while Self::is_whitespace(self.at(self.index)) {
                self.index += 1;
            }

            if self.at(self.index) == i32::from(b'"') || self.at(self.index) == i32::from(b'\'') {
                // Not the standard's words but the same tokens: the whitespace before the quote stays one.
                if Self::is_whitespace(self.at(self.index - 1)) {
                    self.index -= 1;
                }
            } else {
                return self.consume_url_token();
            }
        }

        CssToken { ty: FUNCTION, str: name, ..CssToken::default() }
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-token
    fn consume_token(&mut self) -> CssToken {
        self.consume_comments();

        let mut token = CssToken::default();
        let start = self.index;

        let ch = self.consume_char();

        match ch {
            0x0A | 0x09 | 0x20 => {
                while Self::is_whitespace(self.at(self.index)) {
                    self.index += 1;
                }
                token.ty = WHITESPACE;
            }
            0x22 | 0x27 => token = self.consume_string_token(ch),
            0x23 => {
                // `#`: an ident code point or a valid escape next makes a <hash-token>.
                if Self::is_ident_code_point(self.peek_char())
                    || (self.at(self.index) == i32::from(b'\\') && self.at(self.index + 1) != i32::from(b'\n'))
                {
                    token.ty = HASH;
                    token.hash_type = if Self::would_start_ident_sequence(self.peek_chars()) {
                        CssHashType::css_hash_id
                    } else {
                        CssHashType::css_hash_unrestricted
                    };
                    token.str = self.consume_ident_sequence();
                } else {
                    token.ty = ch;
                }
            }
            0x2B | 0x2E => {
                // `+` and `.`: a number when one starts here.
                let next = self.peek_chars();
                if Self::would_start_a_number(ch, next._1, next._2) {
                    self.unconsume_char();
                    token = self.consume_numeric_token();
                } else {
                    token.ty = ch;
                }
            }
            0x2D => {
                // `-`: a number, `-->`, an ident-like token, or a delim.
                let next = self.peek_chars();
                if Self::would_start_a_number(ch, next._1, next._2) {
                    self.unconsume_char();
                    token = self.consume_numeric_token();
                } else if next._1 == i32::from(b'-') && next._2 == i32::from(b'>') {
                    self.index += 2;
                    token.ty = CDC;
                } else if Self::would_start_ident_sequence(ThreeChars { _1: ch, _2: next._1, _3: next._2 }) {
                    self.unconsume_char();
                    token = self.consume_ident_like_token();
                } else {
                    token.ty = ch;
                }
            }
            0x3C => {
                // `<`: `<!--` is a <CDO-token>.
                if match_at(&self.str, self.index as i32, b"!--") {
                    self.index += 3;
                    token.ty = CDO;
                } else {
                    token.ty = ch;
                }
            }
            0x40 => {
                // `@`: an ident sequence next makes an <at-keyword-token>.
                if Self::would_start_ident_sequence(self.peek_chars()) {
                    token.ty = AT_KEYWORD;
                    token.str = self.consume_ident_sequence();
                } else {
                    token.ty = ch;
                }
            }
            0x5C => {
                // `\`: a valid escape starts an ident-like token; else a parse error and a delim.
                if self.at(self.index) != i32::from(b'\n') {
                    self.unconsume_char();
                    token = self.consume_ident_like_token();
                } else {
                    css_parse_error("escaped newline outside a string");
                    token.ty = ch;
                }
            }
            0 => token.ty = EOF,
            _ => {
                if is_digit(ch) {
                    self.unconsume_char();
                    token = self.consume_numeric_token();
                } else if Self::is_ident_start_code_point(ch) {
                    self.unconsume_char();
                    token = self.consume_ident_like_token();
                } else {
                    // a <delim-token>; `:;,()[]{}` are delims here too
                    token.ty = ch;
                }
            }
        }

        token.repr = self.str[start..self.index].to_vec();
        token
    }

    /// `tokenize()`: every token to the end.
    pub fn tokenize(&mut self) -> CssTokenVector {
        let mut tokens = Vec::new();
        loop {
            let token = self.consume_token();
            if token.ty == EOF {
                break;
            }
            tokens.push(token.clone());
        }
        tokens
    }
}

/// `tokenize(str)`.
pub fn tokenize(str: &[u8]) -> CssTokenVector {
    CssTokenizer::new(str).tokenize()
}
