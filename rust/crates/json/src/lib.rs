//! widget-sandbox's `twidget::Json` (`packages/desktop/sandbox/include/twidget/json.h`): the small JSON
//! value the glue and the renderer share. A strict parser with bounded depth for what the sandbox sends,
//! objects as ordered members (duplicates kept, the first one found), strings as the bytes they hold
//! (not necessarily UTF-8), and the C++'s exact text for what goes back.

#![forbid(unsafe_code)]

use std::fmt::Write as _;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Json {
    #[default]
    Null,
    Bool(bool),
    Number(f64),
    String(Vec<u8>),
    Array(Vec<Json>),
    Object(Vec<(Vec<u8>, Json)>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    Null,
    Bool,
    Number,
    String,
    Array,
    Object,
}

/// What a `\uD800`-style escape of a lone surrogate becomes: U+FFFD (the default), or the surrogate's own
/// 3-byte WTF-8 form (ED A0..BF xx), which is how the engine hands a JS string with a lone surrogate to the
/// glue outside JSON. `Keep` is for the SDK checks: a URL with a lone surrogate must stay refusable, and
/// Web Storage values must round-trip unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Surrogates {
    #[default]
    Replace,
    Keep,
}

/// json.h's default nesting bound.
pub const DEFAULT_MAX_DEPTH: i32 = 128;

static NULL: Json = Json::Null;

impl From<bool> for Json {
    fn from(b: bool) -> Json {
        Json::Bool(b)
    }
}

impl From<f64> for Json {
    fn from(d: f64) -> Json {
        Json::Number(d)
    }
}

macro_rules! from_integer {
    ($($t:ty),*) => {
        $(impl From<$t> for Json {
            fn from(i: $t) -> Json {
                Json::Number(i as f64)
            }
        })*
    };
}

from_integer!(i32, i64, u32, u64, isize, usize);

impl From<&str> for Json {
    fn from(s: &str) -> Json {
        Json::String(s.as_bytes().to_vec())
    }
}

impl From<String> for Json {
    fn from(s: String) -> Json {
        Json::String(s.into_bytes())
    }
}

impl From<&[u8]> for Json {
    fn from(s: &[u8]) -> Json {
        Json::String(s.to_vec())
    }
}

impl From<Vec<u8>> for Json {
    fn from(s: Vec<u8>) -> Json {
        Json::String(s)
    }
}

impl From<Vec<Json>> for Json {
    fn from(a: Vec<Json>) -> Json {
        Json::Array(a)
    }
}

impl std::ops::Index<usize> for Json {
    type Output = Json;

    fn index(&self, i: usize) -> &Json {
        self.at(i)
    }
}

impl std::ops::Index<&str> for Json {
    type Output = Json;

    fn index(&self, key: &str) -> &Json {
        self.get(key)
    }
}

impl Json {
    pub fn array() -> Json {
        Json::Array(Vec::new())
    }

    pub fn object() -> Json {
        Json::Object(Vec::new())
    }

    pub fn kind(&self) -> Type {
        match self {
            Json::Null => Type::Null,
            Json::Bool(_) => Type::Bool,
            Json::Number(_) => Type::Number,
            Json::String(_) => Type::String,
            Json::Array(_) => Type::Array,
            Json::Object(_) => Type::Object,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    pub fn is_bool(&self) -> bool {
        matches!(self, Json::Bool(_))
    }

    pub fn is_number(&self) -> bool {
        matches!(self, Json::Number(_))
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Json::String(_))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Json::Array(_))
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Json::Object(_))
    }

    pub fn as_bool(&self, def: bool) -> bool {
        match self {
            Json::Bool(b) => *b,
            _ => def,
        }
    }

    pub fn as_number(&self, def: f64) -> f64 {
        match self {
            Json::Number(d) => *d,
            _ => def,
        }
    }

    /// Truncated toward zero; a number past either end of `i32` is that end.
    pub fn as_int(&self, def: i32) -> i32 {
        match self {
            Json::Number(d) => int_of(*d, def),
            _ => def,
        }
    }

    /// The string's bytes, or none for anything else.
    pub fn as_str(&self) -> &[u8] {
        match self {
            Json::String(s) => s,
            _ => &[],
        }
    }

    pub fn arr(&self) -> &[Json] {
        match self {
            Json::Array(a) => a,
            _ => &[],
        }
    }

    pub fn obj(&self) -> &[(Vec<u8>, Json)] {
        match self {
            Json::Object(o) => o,
            _ => &[],
        }
    }

    pub fn size(&self) -> usize {
        match self {
            Json::Array(a) => a.len(),
            Json::Object(o) => o.len(),
            _ => 0,
        }
    }

    /// The first member named `key`, or None.
    pub fn find(&self, key: impl AsRef<[u8]>) -> Option<&Json> {
        let key = key.as_ref();
        self.obj().iter().find(|(k, _)| k.as_slice() == key).map(|(_, v)| v)
    }

    /// The `i`-th item, null past the end or when not an array.
    pub fn at(&self, i: usize) -> &Json {
        self.arr().get(i).unwrap_or(&NULL)
    }

    /// The first member named `key`, null when there is none.
    pub fn get(&self, key: impl AsRef<[u8]>) -> &Json {
        self.find(key).unwrap_or(&NULL)
    }

    /// Replaces the first member named `key`, or appends one; makes `self` an empty object first when it is
    /// not one.
    pub fn set(&mut self, key: impl Into<Vec<u8>>, value: impl Into<Json>) -> &mut Json {
        let key = key.into();
        let value = value.into();
        if !self.is_object() {
            *self = Json::object();
        }
        if let Json::Object(members) = self {
            match members.iter_mut().find(|(k, _)| *k == key) {
                Some((_, v)) => *v = value,
                None => members.push((key, value)),
            }
        }
        self
    }

    /// Appends an item; makes `self` an empty array first when it is not one.
    pub fn push(&mut self, value: impl Into<Json>) -> &mut Json {
        let value = value.into();
        if !self.is_array() {
            *self = Json::array();
        }
        if let Json::Array(items) = self {
            items.push(value);
        }
        self
    }

    pub fn dump(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.dump_to(&mut out);
        out
    }

    /// The dump as text, for logs and tests; a string's bytes that are not UTF-8 become U+FFFD.
    pub fn dump_lossy(&self) -> String {
        String::from_utf8_lossy(&self.dump()).into_owned()
    }

    fn dump_to(&self, out: &mut Vec<u8>) {
        match self {
            Json::Null => out.extend_from_slice(b"null"),
            Json::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
            Json::Number(d) => out.extend_from_slice(number(*d).as_bytes()),
            Json::String(s) => quote_to(s, out),
            Json::Array(items) => {
                out.push(b'[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    item.dump_to(out);
                }
                out.push(b']');
            }
            Json::Object(members) => {
                out.push(b'{');
                for (i, (key, value)) in members.iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    quote_to(key, out);
                    out.push(b':');
                    value.dump_to(out);
                }
                out.push(b'}');
            }
        }
    }

    /// Strict parse; None on any error. `max_depth` bounds nesting (hostile input).
    pub fn parse(text: impl AsRef<[u8]>) -> Option<Json> {
        Json::parse_with(text, Surrogates::Replace, DEFAULT_MAX_DEPTH)
    }

    pub fn parse_with(text: impl AsRef<[u8]>, surrogates: Surrogates, max_depth: i32) -> Option<Json> {
        let mut p = Parser { s: text.as_ref(), pos: 0, max_depth, keep_surrogates: surrogates == Surrogates::Keep };
        p.ws();
        let v = p.value(0)?;
        p.ws();
        (p.pos == p.s.len()).then_some(v)
    }
}

fn int_of(d: f64, def: i32) -> i32 {
    if !d.is_finite() {
        def
    } else if d >= f64::from(i32::MAX) {
        i32::MAX
    } else if d <= f64::from(i32::MIN) {
        i32::MIN
    } else {
        d as i32
    }
}

/// Quoted, escaped JSON string literal.
pub fn quote(s: impl AsRef<[u8]>) -> Vec<u8> {
    let s = s.as_ref();
    let mut out = Vec::with_capacity(s.len() + 2);
    quote_to(s, &mut out);
    out
}

fn quote_to(s: &[u8], out: &mut Vec<u8>) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push(b'"');
    for &c in s {
        match c {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x08 => out.extend_from_slice(b"\\b"),
            0x0c => out.extend_from_slice(b"\\f"),
            c if c < 0x20 => {
                out.extend_from_slice(b"\\u00");
                out.push(HEX[usize::from(c >> 4)]);
                out.push(HEX[usize::from(c & 15)]);
            }
            c => out.push(c),
        }
    }
    out.push(b'"');
}

/// A number as the glue writes it: `null` when not finite, an integer's digits below 1e15, and otherwise
/// the platform's form: `std::to_chars`' shortest on Windows and Linux, the shortest of 15, 16 and 17
/// significant digits on Apple, whose deployment targets have no floating `to_chars`.
pub fn number(d: f64) -> String {
    if !d.is_finite() {
        return "null".to_string();
    }
    if d == d.floor() && d.abs() < 1e15 {
        return (d as i64).to_string();
    }
    if cfg!(target_vendor = "apple") { shortest_of_15_to_17(d) } else { to_chars_shortest(d) }
}

fn split_scientific(text: &str) -> (bool, String, i32) {
    let (mantissa, exponent) = text.split_once('e').expect("{:e} has an exponent");
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    (negative, digits, exponent.parse().expect("{:e}'s exponent is a number"))
}

/// The fewest digits that read back as `d` and, of those, the ones nearest it, a tie to the even digit
/// (`std::to_chars`' rule), with the decimal exponent of the first. Rust's `{:e}` gives the count but
/// breaks a tie upward; `d` rounded to that many digits, which `{:.N$e}` does exactly and to even, is the
/// nearest and is taken when it still reads back.
fn shortest_digits(d: f64) -> (bool, String, i32) {
    let shortest = split_scientific(&format!("{:e}", d));
    let nearest = format!("{:.*e}", shortest.1.len() - 1, d);
    if nearest.parse::<f64>() == Ok(d) { split_scientific(&nearest) } else { shortest }
}

/// printf's exponent: a sign and at least two digits.
fn c_exponent(out: &mut String, exponent: i32) {
    let _ = write!(out, "e{}{:02}", if exponent < 0 { '-' } else { '+' }, exponent.unsigned_abs());
}

/// `std::to_chars(first, last, d)`: the shortest digits that read back as `d`, written as `%f` or `%e`
/// would, whichever is shorter, `%f` on a tie.
pub fn to_chars_shortest(d: f64) -> String {
    let (negative, digits, exponent) = shortest_digits(d);
    let sign = if negative { "-" } else { "" };
    let mut scientific = String::from(sign);
    scientific.push_str(&digits[..1]);
    if digits.len() > 1 {
        scientific.push('.');
        scientific.push_str(&digits[1..]);
    }
    c_exponent(&mut scientific, exponent);
    let mut fixed = String::from(sign);
    if exponent < 0 {
        fixed.push_str("0.");
        fixed.extend(std::iter::repeat_n('0', (-exponent - 1) as usize));
        fixed.push_str(&digits);
    } else {
        let whole = exponent as usize + 1;
        if digits.len() <= whole {
            // Every integer of these digits reads back as `d`; to_chars writes the one nearest it, `d` itself.
            fixed = format!("{d:.0}");
        } else {
            fixed.push_str(&digits[..whole]);
            fixed.push('.');
            fixed.push_str(&digits[whole..]);
        }
    }
    if scientific.len() < fixed.len() { scientific } else { fixed }
}

/// `printf("%.*g", precision, d)`: `%e` or `%f` by the exponent at that precision, trailing zeros gone.
pub fn format_g(d: f64, precision: usize) -> String {
    let p = precision.max(1);
    let scientific = format!("{:.*e}", p - 1, d);
    let (mantissa, exponent) = scientific.split_once('e').expect("{:e} has an exponent");
    let exponent: i32 = exponent.parse().expect("{:e}'s exponent is a number");
    let strip =
        |s: &str| -> String { if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s.to_string() } };
    if exponent < p as i32 && exponent >= -4 {
        strip(&format!("{:.*}", (p as i32 - 1 - exponent) as usize, d))
    } else {
        let mut out = strip(mantissa);
        c_exponent(&mut out, exponent);
        out
    }
}

/// json.h's fallback where `std::to_chars` has no doubles: the shortest of 15, 16 and 17 significant
/// digits (`%g`) that reads back as the same double.
pub fn shortest_of_15_to_17(d: f64) -> String {
    let mut out = String::new();
    for precision in 15..=17 {
        out = format_g(d, precision);
        if parse_double(&out) == d {
            break;
        }
    }
    out
}

/// strtod in the C locale, of a number the grammar has already checked.
pub fn parse_double(text: &str) -> f64 {
    text.parse().unwrap_or(0.0)
}

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
    max_depth: i32,
    keep_surrogates: bool,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn lit(&mut self, word: &[u8]) -> bool {
        if !self.s[self.pos..].starts_with(word) {
            return false;
        }
        self.pos += word.len();
        true
    }

    fn utf8(out: &mut Vec<u8>, cp: u32) {
        if cp < 0x80 {
            out.push(cp as u8);
        } else if cp < 0x800 {
            out.extend_from_slice(&[(0xC0 | (cp >> 6)) as u8, (0x80 | (cp & 63)) as u8]);
        } else if cp < 0x10000 {
            out.extend_from_slice(&[(0xE0 | (cp >> 12)) as u8, (0x80 | ((cp >> 6) & 63)) as u8, (0x80 | (cp & 63)) as u8]);
        } else {
            out.extend_from_slice(&[
                (0xF0 | (cp >> 18)) as u8,
                (0x80 | ((cp >> 12) & 63)) as u8,
                (0x80 | ((cp >> 6) & 63)) as u8,
                (0x80 | (cp & 63)) as u8,
            ]);
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        if self.pos + 4 > self.s.len() {
            return None;
        }
        let mut v = 0u32;
        for _ in 0..4 {
            let c = self.s[self.pos];
            self.pos += 1;
            v = (v << 4) | char::from(c).to_digit(16)?;
        }
        Some(v)
    }

    fn string(&mut self) -> Option<Vec<u8>> {
        if self.peek() != Some(b'"') {
            return None;
        }
        self.pos += 1;
        let mut out = Vec::new();
        while let Some(c) = self.peek() {
            self.pos += 1;
            if c == b'"' {
                return Some(out);
            }
            if c < 0x20 {
                return None;
            }
            if c != b'\\' {
                out.push(c);
                continue;
            }
            let e = self.peek()?;
            self.pos += 1;
            match e {
                b'"' => out.push(b'"'),
                b'\\' => out.push(b'\\'),
                b'/' => out.push(b'/'),
                b'b' => out.push(0x08),
                b'f' => out.push(0x0c),
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b't' => out.push(b'\t'),
                b'u' => {
                    let mut cp = self.hex4()?;
                    if (0xD800..=0xDBFF).contains(&cp) {
                        // A pair only with a low surrogate right after; any other escape after it is read on its own.
                        let mut lo = 0;
                        if self.s[self.pos..].starts_with(b"\\u") {
                            let at = self.pos;
                            self.pos += 2;
                            lo = self.hex4()?;
                            if !(0xDC00..=0xDFFF).contains(&lo) {
                                self.pos = at;
                            }
                        }
                        if (0xDC00..=0xDFFF).contains(&lo) {
                            cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                        } else if !self.keep_surrogates {
                            cp = 0xFFFD;
                        }
                    } else if (0xDC00..=0xDFFF).contains(&cp) && !self.keep_surrogates {
                        cp = 0xFFFD;
                    }
                    Parser::utf8(&mut out, cp);
                }
                _ => return None,
            }
        }
        None
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.pos - start
    }

    fn number(&mut self) -> Option<Json> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek()? {
            b'0' => self.pos += 1,
            b'1'..=b'9' => {
                self.digits();
            }
            _ => return None,
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if self.digits() == 0 {
                return None;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return None;
            }
        }
        let text = std::str::from_utf8(&self.s[start..self.pos]).ok()?;
        Some(Json::Number(parse_double(text)))
    }

    fn value(&mut self, depth: i32) -> Option<Json> {
        if depth > self.max_depth {
            return None;
        }
        match self.peek()? {
            b'{' => {
                self.pos += 1;
                let mut members = Vec::new();
                self.ws();
                if self.peek() == Some(b'}') {
                    self.pos += 1;
                    return Some(Json::Object(members));
                }
                loop {
                    self.ws();
                    let key = self.string()?;
                    self.ws();
                    if self.peek() != Some(b':') {
                        return None;
                    }
                    self.pos += 1;
                    self.ws();
                    let v = self.value(depth + 1)?;
                    members.push((key, v));
                    self.ws();
                    match self.peek()? {
                        b',' => self.pos += 1,
                        b'}' => {
                            self.pos += 1;
                            return Some(Json::Object(members));
                        }
                        _ => return None,
                    }
                }
            }
            b'[' => {
                self.pos += 1;
                let mut items = Vec::new();
                self.ws();
                if self.peek() == Some(b']') {
                    self.pos += 1;
                    return Some(Json::Array(items));
                }
                loop {
                    self.ws();
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    match self.peek()? {
                        b',' => self.pos += 1,
                        b']' => {
                            self.pos += 1;
                            return Some(Json::Array(items));
                        }
                        _ => return None,
                    }
                }
            }
            b'"' => self.string().map(Json::String),
            b't' => self.lit(b"true").then_some(Json::Bool(true)),
            b'f' => self.lit(b"false").then_some(Json::Bool(false)),
            b'n' => self.lit(b"null").then_some(Json::Null),
            _ => self.number(),
        }
    }
}
