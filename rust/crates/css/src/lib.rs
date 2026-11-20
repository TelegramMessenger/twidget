//! TwidgetCore's small CSS helpers (`core/src/css.h`, `css.cpp`), shared by the shadow tree, the animation
//! engine and the canvas. None of them is a CSS parser: they read the narrow, already sanitised text the
//! validator forwards. Text is bytes, as the C++ `std::string`s hold it; numbers read as C's `strtod`
//! reads them; floats and doubles are where the C++ has them.

#![forbid(unsafe_code)]

use twidget_cstd::{atof, atoi, format_fixed, strtod};
use twidget_json::Json;
use twidget_wire::{Affine, Color};

fn space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

fn alpha(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

fn digit(c: u8) -> bool {
    c.is_ascii_digit()
}

fn valid_name(name: &[u8], max: usize, first: fn(u8) -> bool, rest: fn(u8) -> bool) -> bool {
    !name.is_empty() && name.len() <= max && first(name[0]) && name[1..].iter().all(|c| rest(*c))
}

/// `s` without the CSS spaces at either end.
pub fn trim(s: &[u8]) -> Vec<u8> {
    trimmed(s).to_vec()
}

fn trimmed(s: &[u8]) -> &[u8] {
    let mut a = 0;
    let mut b = s.len();
    while a < b && space(s[a]) {
        a += 1;
    }
    while b > a && space(s[b - 1]) {
        b -= 1;
    }
    &s[a..b]
}

/// ASCII lowercase, as `tolower` in the "C" locale.
pub fn lower(s: &[u8]) -> Vec<u8> {
    s.to_ascii_lowercase()
}

pub fn iequals(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// A finite double from the whole of `s` (trimmed), as `strtod` reads it, or None.
pub fn number(s: &[u8]) -> Option<f64> {
    let t = trimmed(s);
    if t.is_empty() {
        return None;
    }
    let (v, n) = strtod(t);
    (n == t.len() && v.is_finite()).then_some(v)
}

/// Splits at `separator` outside parentheses and quotes (`a, rgb(1,2,3)` is two parts); a last empty
/// part is dropped.
pub fn split_top_level(text: &[u8], separator: u8) -> Vec<Vec<u8>> {
    let mut parts = Vec::new();
    let mut current = Vec::new();
    let mut depth: i32 = 0;
    let mut quote = 0u8;
    let mut escaped = false;
    for &ch in text {
        if escaped {
            escaped = false;
            current.push(ch);
            continue;
        }
        if ch == b'\\' {
            escaped = true;
            current.push(ch);
            continue;
        }
        if quote != 0 {
            if ch == quote {
                quote = 0;
            }
            current.push(ch);
            continue;
        }
        if ch == b'"' || ch == b'\'' {
            quote = ch;
        } else if ch == b'(' {
            depth += 1;
        } else if ch == b')' {
            depth = (depth - 1).max(0);
        }
        if ch == separator && depth == 0 {
            parts.push(std::mem::take(&mut current));
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Declaration {
    pub name: Vec<u8>,
    pub value: Vec<u8>,
    pub important: bool,
}

/// An inline `style` text as declarations; invalid names are dropped, a later duplicate wins.
pub fn parse_declarations(text: &[u8]) -> Vec<Declaration> {
    let mut result: Vec<Declaration> = Vec::new();
    for part in split_top_level(text, b';') {
        let Some(colon) = part.iter().position(|c| *c == b':') else { continue };
        let mut name = trim(&part[..colon]);
        let mut value = trim(&part[colon + 1..]);
        if name.is_empty() || value.is_empty() {
            continue;
        }
        let mut important = false;
        if value.len() >= 10 && iequals(&value[value.len() - 10..], b"!important") {
            important = true;
            value = trim(&value[..value.len() - 10]);
        }
        if !name.starts_with(b"--") {
            name = lower(&name);
        }
        if !is_property_name(&name) {
            continue;
        }
        result.retain(|d| d.name != name);
        result.push(Declaration { name, value, important });
    }
    result
}

/// What may be written into the HTML the renderer generates.
pub fn is_tag_name(name: &[u8]) -> bool {
    valid_name(name, 64, alpha, |c| alpha(c) || digit(c) || c == b'-')
}

pub fn is_attribute_name(name: &[u8]) -> bool {
    valid_name(
        name,
        64,
        |c| alpha(c) || c == b'_' || c == b':',
        |c| alpha(c) || digit(c) || c == b'_' || c == b':' || c == b'.' || c == b'-',
    )
}

pub fn is_property_name(name: &[u8]) -> bool {
    let body = if name.starts_with(b"--") {
        &name[2..]
    } else if name.starts_with(b"-") {
        &name[1..]
    } else {
        name
    };
    let ok = |c: u8| alpha(c) || digit(c) || c == b'-' || c == b'_';
    valid_name(body, 128, ok, ok)
}

/// A character that continues a CSS identifier.
pub fn ident_char(c: u8) -> bool {
    alpha(c) || digit(c) || c == b'-' || c == b'_' || c >= 0x80
}

/// Past what a text rewrite must not look into when it starts at `i`: a string, a comment, an escape or
/// an unquoted url(…). `i` itself when none starts there.
pub fn skip_opaque(css: &[u8], i: usize) -> usize {
    if i >= css.len() {
        return i;
    }
    let c = css[i];
    if c == b'"' || c == b'\'' {
        // To the closing quote, past escapes; a newline ends a bad string, as the tokenizer has it.
        let mut j = i + 1;
        while j < css.len() {
            if css[j] == b'\\' {
                j += 1;
            } else if css[j] == c {
                return j + 1;
            } else if css[j] == b'\n' {
                return j;
            }
            j += 1;
        }
        return css.len();
    }
    if c == b'/' && i + 1 < css.len() && css[i + 1] == b'*' {
        return match find(css, b"*/", i + 2) {
            None => css.len(),
            Some(end) => end + 2,
        };
    }
    if c == b'\\' {
        return css.len().min(i + 2);
    }
    // An unquoted url(…) is one token: its text is the URL's.
    if (c == b'u' || c == b'U') && (i == 0 || !ident_char(css[i - 1])) && iequals(&css[i..(i + 4).min(css.len())], b"url(") {
        let mut j = i + 4;
        while j < css.len() && matches!(css[j], b' ' | b'\t' | b'\n') {
            j += 1;
        }
        if j < css.len() && (css[j] == b'"' || css[j] == b'\'') {
            return i;
        }
        while j < css.len() {
            if css[j] == b'\\' {
                j += 1;
            } else if css[j] == b')' {
                return j + 1;
            }
            j += 1;
        }
        return css.len();
    }
    i
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > hay.len() || needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

fn find_byte(hay: &[u8], byte: u8, from: usize) -> Option<usize> {
    hay.get(from..)?.iter().position(|c| *c == byte).map(|p| p + from)
}

/// Replaces every `light-dark(a, b)` with the side the theme picks: litehtml knows no such function. One
/// inside the chosen side is resolved too, 32 levels down; a side that nests deeper is dropped.
pub fn resolve_light_dark(css: &[u8], dark: bool, nesting: i32) -> Vec<u8> {
    const KEY: &[u8] = b"light-dark(";
    const MAX_NESTING: i32 = 32;
    // A function token: outside strings, comments and URLs, and not the end of a longer name.
    let find_key = |s: &[u8], from: usize| -> Option<usize> {
        let mut i = from;
        while i + KEY.len() <= s.len() {
            let skip = skip_opaque(s, i);
            if skip != i {
                i = skip;
                continue;
            }
            if (i == 0 || !ident_char(s[i - 1])) && iequals(&s[i..i + KEY.len()], KEY) {
                return Some(i);
            }
            i += 1;
        }
        None
    };
    if find_key(css, 0).is_none() {
        return css.to_vec();
    }
    if nesting >= MAX_NESTING {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut pos = 0;
    for _ in 0..4096 {
        let Some(at) = find_key(css, pos) else { break };
        out.extend_from_slice(&css[pos..at]);
        let i = at + KEY.len();
        let mut depth = 1;
        let mut close = i;
        while close < css.len() {
            let skip = skip_opaque(css, close);
            if skip != close {
                close = skip;
                continue;
            }
            if css[close] == b'(' {
                depth += 1;
            } else if css[close] == b')' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            close += 1;
        }
        let parts = split_top_level(&css[i..close.min(css.len())], b',');
        let chosen = if parts.len() == 2 {
            parts[usize::from(dark)].clone()
        } else if parts.is_empty() {
            Vec::new()
        } else {
            parts[0].clone()
        };
        // The chosen side may hold another light-dark().
        out.extend(resolve_light_dark(&trim(&chosen), dark, nesting + 1));
        pos = if close < css.len() { close + 1 } else { css.len() };
    }
    out.extend_from_slice(&css[pos..]);
    out
}

/// calc() of times (as twidget's CSSTime) or of lengths in px and %, with the % resolved against
/// `percent_of`: + and - of like quantities, * and / by numbers.
#[derive(Clone, Copy)]
struct Quantity {
    value: f64,
    unit: bool,
}

struct MathScanner<'a> {
    s: &'a [u8],
    lengths: bool,
    percent_of: f64,
    i: usize,
}

impl MathScanner<'_> {
    fn skip(&mut self) {
        while self.i < self.s.len() && space(self.s[self.i]) {
            self.i += 1;
        }
    }

    fn take(&mut self, c: u8) -> bool {
        self.skip();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            return true;
        }
        false
    }

    fn at_end(&mut self) -> bool {
        self.skip();
        self.i == self.s.len()
    }

    fn at(&self, word: &[u8]) -> bool {
        self.s[self.i..].starts_with(word)
    }

    fn sum(&mut self, depth: i32) -> Option<Quantity> {
        if depth >= 16 {
            return None;
        }
        let mut left = self.product(depth)?;
        loop {
            if self.take(b'+') {
                let right = self.product(depth)?;
                if right.unit != left.unit {
                    return None;
                }
                left.value += right.value;
            } else if self.take(b'-') {
                let right = self.product(depth)?;
                if right.unit != left.unit {
                    return None;
                }
                left.value -= right.value;
            } else {
                return Some(left);
            }
        }
    }

    fn product(&mut self, depth: i32) -> Option<Quantity> {
        let mut left = self.factor(depth)?;
        loop {
            if self.take(b'*') {
                let right = self.factor(depth)?;
                if left.unit && right.unit {
                    return None;
                }
                left = Quantity { value: left.value * right.value, unit: left.unit || right.unit };
            } else if self.take(b'/') {
                let right = self.factor(depth)?;
                if right.unit || right.value == 0.0 {
                    return None;
                }
                left.value /= right.value;
            } else {
                return Some(left);
            }
        }
    }

    fn factor(&mut self, depth: i32) -> Option<Quantity> {
        self.skip();
        if self.at(b"calc(") {
            self.i += 4;
        }
        if self.take(b'(') {
            let inner = self.sum(depth + 1)?;
            if !self.take(b')') {
                return None;
            }
            return Some(inner);
        }
        let start = self.i;
        if self.i < self.s.len() && (self.s[self.i] == b'-' || self.s[self.i] == b'+') {
            self.i += 1;
        }
        while self.i < self.s.len() && (digit(self.s[self.i]) || self.s[self.i] == b'.') {
            self.i += 1;
        }
        if self.i == start {
            return None;
        }
        let v = number(&self.s[start..self.i])?;
        if self.lengths {
            if self.at(b"px") {
                self.i += 2;
                return Some(Quantity { value: v, unit: true });
            }
            if self.s.get(self.i) == Some(&b'%') {
                self.i += 1;
                return Some(Quantity { value: v * self.percent_of / 100.0, unit: true });
            }
        } else {
            if self.at(b"ms") {
                self.i += 2;
                return Some(Quantity { value: v / 1000.0, unit: true });
            }
            if self.s.get(self.i) == Some(&b's') {
                self.i += 1;
                return Some(Quantity { value: v, unit: true });
            }
        }
        Some(Quantity { value: v, unit: false })
    }
}

/// `1.5s`, `200ms` or a `calc()` of times, in seconds.
pub fn parse_time(text: &[u8]) -> Option<f64> {
    let t = lower(trimmed(text));
    if t.starts_with(b"calc(") {
        if t.len() > 512 {
            return None;
        }
        let mut scanner = MathScanner { s: &t, lengths: false, percent_of: 0.0, i: 0 };
        let q = scanner.sum(0)?;
        if !scanner.at_end() || !q.unit || !q.value.is_finite() {
            return None;
        }
        return Some(q.value);
    }
    if t.ends_with(b"ms") {
        return number(&t[..t.len() - 2]).map(|v| v / 1000.0);
    }
    if t.ends_with(b"s") {
        return number(&t[..t.len() - 1]);
    }
    None
}

/// CSS Color 4 named colours.
const NAMED: [(&[u8], u32); 148] = [
    (b"aliceblue", 0xf0f8ff),
    (b"antiquewhite", 0xfaebd7),
    (b"aqua", 0x00ffff),
    (b"aquamarine", 0x7fffd4),
    (b"azure", 0xf0ffff),
    (b"beige", 0xf5f5dc),
    (b"bisque", 0xffe4c4),
    (b"black", 0x000000),
    (b"blanchedalmond", 0xffebcd),
    (b"blue", 0x0000ff),
    (b"blueviolet", 0x8a2be2),
    (b"brown", 0xa52a2a),
    (b"burlywood", 0xdeb887),
    (b"cadetblue", 0x5f9ea0),
    (b"chartreuse", 0x7fff00),
    (b"chocolate", 0xd2691e),
    (b"coral", 0xff7f50),
    (b"cornflowerblue", 0x6495ed),
    (b"cornsilk", 0xfff8dc),
    (b"crimson", 0xdc143c),
    (b"cyan", 0x00ffff),
    (b"darkblue", 0x00008b),
    (b"darkcyan", 0x008b8b),
    (b"darkgoldenrod", 0xb8860b),
    (b"darkgray", 0xa9a9a9),
    (b"darkgreen", 0x006400),
    (b"darkgrey", 0xa9a9a9),
    (b"darkkhaki", 0xbdb76b),
    (b"darkmagenta", 0x8b008b),
    (b"darkolivegreen", 0x556b2f),
    (b"darkorange", 0xff8c00),
    (b"darkorchid", 0x9932cc),
    (b"darkred", 0x8b0000),
    (b"darksalmon", 0xe9967a),
    (b"darkseagreen", 0x8fbc8f),
    (b"darkslateblue", 0x483d8b),
    (b"darkslategray", 0x2f4f4f),
    (b"darkslategrey", 0x2f4f4f),
    (b"darkturquoise", 0x00ced1),
    (b"darkviolet", 0x9400d3),
    (b"deeppink", 0xff1493),
    (b"deepskyblue", 0x00bfff),
    (b"dimgray", 0x696969),
    (b"dimgrey", 0x696969),
    (b"dodgerblue", 0x1e90ff),
    (b"firebrick", 0xb22222),
    (b"floralwhite", 0xfffaf0),
    (b"forestgreen", 0x228b22),
    (b"fuchsia", 0xff00ff),
    (b"gainsboro", 0xdcdcdc),
    (b"ghostwhite", 0xf8f8ff),
    (b"gold", 0xffd700),
    (b"goldenrod", 0xdaa520),
    (b"gray", 0x808080),
    (b"green", 0x008000),
    (b"greenyellow", 0xadff2f),
    (b"grey", 0x808080),
    (b"honeydew", 0xf0fff0),
    (b"hotpink", 0xff69b4),
    (b"indianred", 0xcd5c5c),
    (b"indigo", 0x4b0082),
    (b"ivory", 0xfffff0),
    (b"khaki", 0xf0e68c),
    (b"lavender", 0xe6e6fa),
    (b"lavenderblush", 0xfff0f5),
    (b"lawngreen", 0x7cfc00),
    (b"lemonchiffon", 0xfffacd),
    (b"lightblue", 0xadd8e6),
    (b"lightcoral", 0xf08080),
    (b"lightcyan", 0xe0ffff),
    (b"lightgoldenrodyellow", 0xfafad2),
    (b"lightgray", 0xd3d3d3),
    (b"lightgreen", 0x90ee90),
    (b"lightgrey", 0xd3d3d3),
    (b"lightpink", 0xffb6c1),
    (b"lightsalmon", 0xffa07a),
    (b"lightseagreen", 0x20b2aa),
    (b"lightskyblue", 0x87cefa),
    (b"lightslategray", 0x778899),
    (b"lightslategrey", 0x778899),
    (b"lightsteelblue", 0xb0c4de),
    (b"lightyellow", 0xffffe0),
    (b"lime", 0x00ff00),
    (b"limegreen", 0x32cd32),
    (b"linen", 0xfaf0e6),
    (b"magenta", 0xff00ff),
    (b"maroon", 0x800000),
    (b"mediumaquamarine", 0x66cdaa),
    (b"mediumblue", 0x0000cd),
    (b"mediumorchid", 0xba55d3),
    (b"mediumpurple", 0x9370db),
    (b"mediumseagreen", 0x3cb371),
    (b"mediumslateblue", 0x7b68ee),
    (b"mediumspringgreen", 0x00fa9a),
    (b"mediumturquoise", 0x48d1cc),
    (b"mediumvioletred", 0xc71585),
    (b"midnightblue", 0x191970),
    (b"mintcream", 0xf5fffa),
    (b"mistyrose", 0xffe4e1),
    (b"moccasin", 0xffe4b5),
    (b"navajowhite", 0xffdead),
    (b"navy", 0x000080),
    (b"oldlace", 0xfdf5e6),
    (b"olive", 0x808000),
    (b"olivedrab", 0x6b8e23),
    (b"orange", 0xffa500),
    (b"orangered", 0xff4500),
    (b"orchid", 0xda70d6),
    (b"palegoldenrod", 0xeee8aa),
    (b"palegreen", 0x98fb98),
    (b"paleturquoise", 0xafeeee),
    (b"palevioletred", 0xdb7093),
    (b"papayawhip", 0xffefd5),
    (b"peachpuff", 0xffdab9),
    (b"peru", 0xcd853f),
    (b"pink", 0xffc0cb),
    (b"plum", 0xdda0dd),
    (b"powderblue", 0xb0e0e6),
    (b"purple", 0x800080),
    (b"rebeccapurple", 0x663399),
    (b"red", 0xff0000),
    (b"rosybrown", 0xbc8f8f),
    (b"royalblue", 0x4169e1),
    (b"saddlebrown", 0x8b4513),
    (b"salmon", 0xfa8072),
    (b"sandybrown", 0xf4a460),
    (b"seagreen", 0x2e8b57),
    (b"seashell", 0xfff5ee),
    (b"sienna", 0xa0522d),
    (b"silver", 0xc0c0c0),
    (b"skyblue", 0x87ceeb),
    (b"slateblue", 0x6a5acd),
    (b"slategray", 0x708090),
    (b"slategrey", 0x708090),
    (b"snow", 0xfffafa),
    (b"springgreen", 0x00ff7f),
    (b"steelblue", 0x4682b4),
    (b"tan", 0xd2b48c),
    (b"teal", 0x008080),
    (b"thistle", 0xd8bfd8),
    (b"tomato", 0xff6347),
    (b"turquoise", 0x40e0d0),
    (b"violet", 0xee82ee),
    (b"wheat", 0xf5deb3),
    (b"white", 0xffffff),
    (b"whitesmoke", 0xf5f5f5),
    (b"yellow", 0xffff00),
    (b"yellowgreen", 0x9acd32),
];

fn named_color(word: &[u8]) -> Option<Color> {
    let l = lower(word);
    if l == b"transparent" {
        return Some(Color::rgba(0, 0, 0, 0));
    }
    NAMED.iter().find(|(name, _)| *name == l.as_slice()).map(|(_, rgb)| Color::rgba((rgb >> 16) as u8, (rgb >> 8) as u8, *rgb as u8, 255))
}

fn hex_value(c: u8) -> Option<u32> {
    char::from(c).to_digit(16).filter(|_| c.is_ascii_hexdigit())
}

fn parse_hex(hex: &[u8]) -> Option<[f64; 4]> {
    let v: Vec<f64> = hex.iter().map(|c| hex_value(*c).map(f64::from)).collect::<Option<_>>()?;
    let mut rgba = [0.0; 4];
    match hex.len() {
        3 | 4 => {
            for i in 0..hex.len() {
                rgba[i] = v[i] * 17.0;
            }
            rgba[3] = if hex.len() == 4 { rgba[3] / 255.0 } else { 1.0 };
        }
        6 | 8 => {
            for i in 0..hex.len() / 2 {
                rgba[i] = v[2 * i] * 16.0 + v[2 * i + 1];
            }
            rgba[3] = if hex.len() == 8 { rgba[3] / 255.0 } else { 1.0 };
        }
        _ => return None,
    }
    Some(rgba)
}

/// The numbers inside `rgb(...)`/`hsl(...)`, split at commas, spaces and `/`; `%` divides by 100.
fn components(inner: &[u8], mut percent: Option<&mut [bool; 3]>) -> Vec<f64> {
    let mut out = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut flush = |cur: &mut Vec<u8>, out: &mut Vec<f64>| {
        if cur.is_empty() {
            return;
        }
        let pct = cur.last() == Some(&b'%');
        if pct {
            cur.pop();
        }
        if cur.ends_with(b"deg") {
            cur.truncate(cur.len() - 3);
        }
        if let Some(v) = number(cur) {
            out.push(if pct { v / 100.0 } else { v });
            if let Some(p) = percent.as_deref_mut()
                && out.len() <= 3
            {
                p[out.len() - 1] = pct;
            }
        }
        cur.clear();
    };
    for &c in inner {
        if c == b',' || c == b'/' || space(c) {
            flush(&mut cur, &mut out);
        } else {
            cur.push(c);
        }
    }
    flush(&mut cur, &mut out);
    out
}

fn rgba_string(c: &[f64; 4], out: &mut Vec<u8>) {
    out.extend_from_slice(b"rgba(");
    out.extend_from_slice(format_number(c[0], 4).as_bytes());
    out.extend_from_slice(b", ");
    out.extend_from_slice(format_number(c[1], 4).as_bytes());
    out.extend_from_slice(b", ");
    out.extend_from_slice(format_number(c[2], 4).as_bytes());
    out.extend_from_slice(b", ");
    out.extend_from_slice(format_number(c[3], 4).as_bytes());
    out.push(b')');
}

/// `uint8_t(std::lround(std::clamp(v, 0.0, 255.0)))`.
fn channel(v: f64) -> u8 {
    if v.is_nan() {
        return 0;
    }
    v.clamp(0.0, 255.0).round() as u8
}

/// A CSS colour: hex, rgb[a](), hsl[a](), a named colour, `transparent` or `currentcolor` (black).
pub fn parse_color(text: &[u8]) -> Option<Color> {
    let t = lower(trimmed(text));
    if t.is_empty() {
        return None;
    }
    if t[0] == b'#' {
        let c = parse_hex(&t[1..])?;
        return Some(Color::rgba(channel(c[0]), channel(c[1]), channel(c[2]), channel(c[3] * 255.0)));
    }
    if let Some(open) = t.iter().position(|c| *c == b'(')
        && t.last() == Some(&b')')
    {
        let func = &t[..open];
        let inner = &t[open + 1..t.len() - 1];
        if func == b"rgb" || func == b"rgba" {
            let mut pct = [false; 3];
            let mut v = components(inner, Some(&mut pct));
            if v.len() < 3 {
                return None;
            }
            for i in 0..3 {
                if pct[i] {
                    v[i] *= 255.0;
                }
            }
            let a = if v.len() > 3 { v[3] } else { 1.0 };
            return Some(Color::rgba(channel(v[0]), channel(v[1]), channel(v[2]), channel(a * 255.0)));
        }
        if func == b"hsl" || func == b"hsla" {
            let v = components(inner, None);
            if v.len() < 3 {
                return None;
            }
            let h = ((v[0] % 360.0) + 360.0) % 360.0 / 360.0;
            let s = v[1].clamp(0.0, 1.0);
            let l = v[2].clamp(0.0, 1.0);
            let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
            let p = 2.0 * l - q;
            let hue = |t0: f64| {
                let t1 = if t0 < 0.0 {
                    t0 + 1.0
                } else if t0 > 1.0 {
                    t0 - 1.0
                } else {
                    t0
                };
                if t1 < 1.0 / 6.0 {
                    p + (q - p) * 6.0 * t1
                } else if t1 < 1.0 / 2.0 {
                    q
                } else if t1 < 2.0 / 3.0 {
                    p + (q - p) * (2.0 / 3.0 - t1) * 6.0
                } else {
                    p
                }
            };
            let a = if v.len() > 3 { v[3] } else { 1.0 };
            return Some(Color::rgba(
                channel(hue(h + 1.0 / 3.0) * 255.0),
                channel(hue(h) * 255.0),
                channel(hue(h - 1.0 / 3.0) * 255.0),
                channel(a * 255.0),
            ));
        }
        return None;
    }
    if t == b"currentcolor" {
        return Some(Color::rgba(0, 0, 0, 255));
    }
    named_color(&t)
}

/// Rewrites every colour in `text` as `rgba(r, g, b, a)`, so two values interpolate number by number.
pub fn normalize_colors(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 16);
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c == b'#' {
            let mut j = i + 1;
            while j < text.len() && hex_value(text[j]).is_some() {
                j += 1;
            }
            if let Some(rgba) = parse_hex(&text[i + 1..j]) {
                rgba_string(&rgba, &mut out);
                i = j;
                continue;
            }
        }
        if alpha(c) && (i == 0 || !(alpha(text[i - 1]) || digit(text[i - 1]) || text[i - 1] == b'-')) {
            let mut j = i;
            while j < text.len() && (alpha(text[j]) || text[j] == b'-') {
                j += 1;
            }
            let word = &text[i..j];
            if j < text.len() && text[j] == b'(' {
                if (iequals(word, b"rgb") || iequals(word, b"rgba"))
                    && let Some(close) = find_byte(text, b')', j)
                {
                    let mut pct = [false; 3];
                    let mut v = components(&text[j + 1..close], Some(&mut pct));
                    if v.len() >= 3 {
                        for k in 0..3 {
                            if pct[k] {
                                v[k] *= 255.0;
                            }
                        }
                        let rgba = [v[0], v[1], v[2], if v.len() > 3 { v[3] } else { 1.0 }];
                        rgba_string(&rgba, &mut out);
                        i = close + 1;
                        continue;
                    }
                }
            } else if let Some(named) = named_color(word) {
                let rgba = [f64::from(named.r), f64::from(named.g), f64::from(named.b), f64::from(named.a) / 255.0];
                rgba_string(&rgba, &mut out);
                i = j;
                continue;
            }
            out.extend_from_slice(word);
            i = j;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Shortest decimal form: integers below 1e9 without a point, otherwise at most `digits` fractional
/// digits (`%.*f`) without trailing zeros; not finite is "0".
pub fn format_number(value: f64, digits: i32) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    if value == value.round() && value.abs() < 1e9 {
        return (value as i64).to_string();
    }
    let mut s = format_fixed(value, digits);
    // The C++ prints into a 64-byte buffer: a longer number is cut to its first 63 characters.
    s.truncate(63);
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    if s == "-0" {
        s = "0".to_string();
    }
    s
}

/// The `font` shorthand as the canvas and `measureText` read it.
#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    pub family: Vec<u8>,
    pub size: f32,
    pub weight: i32,
    pub italic: bool,
}

impl Default for Font {
    fn default() -> Font {
        Font { family: b"sans-serif".to_vec(), size: 10.0, weight: 400, italic: false }
    }
}

pub fn parse_font(font: &[u8]) -> Font {
    let mut f = Font::default();
    let mut tokens: Vec<Vec<u8>> = Vec::new();
    let mut cur = Vec::new();
    let mut quote = 0u8;
    for &c in font {
        if quote != 0 {
            if c == quote {
                quote = 0;
            }
            cur.push(c);
        } else if c == b'"' || c == b'\'' {
            quote = c;
            cur.push(c);
        } else if space(c) {
            if !cur.is_empty() {
                tokens.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    let mut i = 0;
    while i < tokens.len() {
        let t = lower(&tokens[i]);
        if t == b"italic" || t == b"oblique" {
            f.italic = true;
        } else if t == b"bold" || t == b"bolder" {
            f.weight = 700;
        } else if t == b"lighter" {
            f.weight = 300;
        } else if t == b"normal" || t == b"small-caps" {
        } else if t.len() == 3 && t.iter().all(|c| digit(*c)) {
            f.weight = atoi(&t);
        } else if digit(t[0]) || t[0] == b'.' {
            let size = &t[..t.iter().position(|c| *c == b'/').unwrap_or(t.len())];
            let mut v = atof(size) as f32;
            if size.ends_with(b"pt") {
                v *= 96.0f32 / 72.0f32;
            } else if size.ends_with(b"rem") || size.ends_with(b"em") {
                v *= 16.0f32;
            } else if size.ends_with(b"%") {
                v *= 16.0f32 / 100.0f32;
            }
            if v > 0.0 && v < 4096.0 {
                f.size = v;
            }
            i += 1;
            break;
        } else {
            break;
        }
        i += 1;
    }
    if i < tokens.len() {
        f.family.clear();
        for token in &tokens[i..] {
            if !f.family.is_empty() {
                f.family.push(b' ');
            }
            f.family.extend_from_slice(token);
        }
    }
    f
}

const PI: f64 = std::f64::consts::PI;

/// A number with its unit, lowercased: `12.5px` → (12.5, "px").
fn dimension(text: &[u8]) -> Option<(f64, Vec<u8>)> {
    let t = trimmed(text);
    let mut i = 0;
    if i < t.len() && (t[i] == b'+' || t[i] == b'-') {
        i += 1;
    }
    while i < t.len() && (digit(t[i]) || t[i] == b'.') {
        i += 1;
    }
    if i < t.len() && (t[i] == b'e' || t[i] == b'E') && i + 1 < t.len() && (digit(t[i + 1]) || t[i + 1] == b'-' || t[i + 1] == b'+') {
        i += 2;
        while i < t.len() && digit(t[i]) {
            i += 1;
        }
    }
    let value = number(&t[..i])?;
    Some((value, lower(&t[i..])))
}

fn length(text: &[u8], percent_of: f64) -> Option<f64> {
    let t = lower(trimmed(text));
    if t.starts_with(b"calc(") {
        if t.len() > 512 {
            return None;
        }
        let mut scanner = MathScanner { s: &t, lengths: true, percent_of, i: 0 };
        let q = scanner.sum(0)?;
        // A bare 0 is a length; any other unitless result is not.
        if !scanner.at_end() || (!q.unit && q.value != 0.0) || !q.value.is_finite() {
            return None;
        }
        return Some(q.value);
    }
    let (value, unit) = dimension(text)?;
    if unit == b"px" || (unit.is_empty() && value == 0.0) {
        return Some(value);
    }
    if unit == b"%" {
        return Some(value * percent_of / 100.0);
    }
    None
}

fn angle(text: &[u8]) -> Option<f64> {
    let (value, unit) = dimension(text)?;
    match unit.as_slice() {
        b"deg" => Some(value * PI / 180.0),
        b"" if value == 0.0 => Some(value * PI / 180.0),
        b"rad" => Some(value),
        b"grad" => Some(value * PI / 200.0),
        b"turn" => Some(value * 2.0 * PI),
        _ => None,
    }
}

fn scalar(text: &[u8]) -> Option<f64> {
    let (value, unit) = dimension(text)?;
    match unit.as_slice() {
        b"" => Some(value),
        b"%" => Some(value / 100.0),
        _ => None,
    }
}

fn arguments(inner: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for part in split_top_level(inner, b',') {
        let mut token = Vec::new();
        let mut depth = 0;
        for &c in &part {
            if c == b'(' {
                depth += 1;
            } else if c == b')' && depth > 0 {
                depth -= 1;
            }
            if space(c) && depth == 0 {
                if !token.is_empty() {
                    out.push(std::mem::take(&mut token));
                }
            } else {
                token.push(c);
            }
        }
        if !token.is_empty() {
            out.push(token);
        }
    }
    out
}

fn rotation(radians: f64) -> Affine {
    let c = radians.cos() as f32;
    let s = radians.sin() as f32;
    Affine { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
}

fn function(name: &[u8], args: &[Vec<u8>], w: f32, h: f32) -> Option<Affine> {
    let n = args.len();
    match name {
        b"matrix" => {
            if n != 6 {
                return None;
            }
            let mut v = [0.0f32; 6];
            for i in 0..6 {
                v[i] = number(&args[i])? as f32;
            }
            Some(Affine { a: v[0], b: v[1], c: v[2], d: v[3], e: v[4], f: v[5] })
        }
        b"translate" | b"translate3d" => {
            if n < 1 || n > if name == b"translate" { 2 } else { 3 } {
                return None;
            }
            let x = length(&args[0], f64::from(w))?;
            let y = if n > 1 { length(&args[1], f64::from(h))? } else { 0.0 };
            // Without perspective a z offset or z scale does not move anything on screen, so it is accepted and dropped.
            if n == 3 && length(&args[2], 0.0).is_none() {
                return None;
            }
            Some(Affine::translate(x as f32, y as f32))
        }
        b"translatex" | b"translatey" => {
            if n != 1 {
                return None;
            }
            let x = name == b"translatex";
            let v = length(&args[0], f64::from(if x { w } else { h }))?;
            Some(if x { Affine::translate(v as f32, 0.0) } else { Affine::translate(0.0, v as f32) })
        }
        b"translatez" => (n == 1 && length(&args[0], 0.0).is_some()).then_some(Affine::IDENTITY),
        b"scale" | b"scale3d" => {
            if n < 1 || n > if name == b"scale" { 2 } else { 3 } {
                return None;
            }
            let x = scalar(&args[0])?;
            let y = if n > 1 { scalar(&args[1])? } else { x };
            if n == 3 && scalar(&args[2]).is_none() {
                return None;
            }
            Some(Affine { a: x as f32, b: 0.0, c: 0.0, d: y as f32, e: 0.0, f: 0.0 })
        }
        b"scalex" | b"scaley" => {
            let v = if n == 1 { scalar(&args[0])? } else { return None };
            Some(if name == b"scalex" {
                Affine { a: v as f32, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 }
            } else {
                Affine { a: 1.0, b: 0.0, c: 0.0, d: v as f32, e: 0.0, f: 0.0 }
            })
        }
        b"rotate" | b"rotatez" => {
            let a = if n == 1 { angle(&args[0])? } else { return None };
            Some(rotation(a))
        }
        b"skew" | b"skewx" | b"skewy" => {
            if n < 1 || n > if name == b"skew" { 2 } else { 1 } {
                return None;
            }
            let ax = if name == b"skewy" { 0.0 } else { angle(&args[0])? };
            let ay = if name == b"skewy" {
                angle(&args[0])?
            } else if n > 1 {
                angle(&args[1])?
            } else {
                0.0
            };
            Some(Affine { a: 1.0, b: ay.tan() as f32, c: ax.tan() as f32, d: 1.0, e: 0.0, f: 0.0 })
        }
        _ => None,
    }
}

fn none_or_empty(t: &[u8]) -> bool {
    t.is_empty() || iequals(t, b"none")
}

/// A `transform` list as one matrix for a box of `w` × `h` (percentages), about the box's top left.
/// Only 2D functions in px, % and angle units, and calc() of px and %; None for anything else.
pub fn parse_transform(text: &[u8], w: f32, h: f32) -> Option<Affine> {
    let t = trimmed(text);
    if none_or_empty(t) {
        return Some(Affine::IDENTITY);
    }
    let mut result = Affine::IDENTITY;
    let mut pos = 0;
    loop {
        while pos < t.len() && space(t[pos]) {
            pos += 1;
        }
        if pos == t.len() {
            break;
        }
        let open = find_byte(t, b'(', pos)?;
        let mut close = open + 1;
        let mut depth = 1;
        while close < t.len() {
            if t[close] == b'(' {
                depth += 1;
            } else if t[close] == b')' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            close += 1;
        }
        if close == t.len() {
            return None;
        }
        let name = lower(trimmed(&t[pos..open]));
        let args = arguments(&t[open + 1..close]);
        let m = function(&name, &args, w, h)?;
        result = result * m;
        pos = close + 1;
    }
    Some(result)
}

/// The individual `translate` property.
pub fn parse_translate(text: &[u8], w: f32, h: f32) -> Option<Affine> {
    let t = trimmed(text);
    if none_or_empty(t) {
        return Some(Affine::IDENTITY);
    }
    let args = arguments(t);
    if args.is_empty() || args.len() > 3 {
        return None;
    }
    let x = length(&args[0], f64::from(w))?;
    let y = if args.len() > 1 { length(&args[1], f64::from(h))? } else { 0.0 };
    if args.len() == 3 && length(&args[2], 0.0) != Some(0.0) {
        return None;
    }
    Some(Affine::translate(x as f32, y as f32))
}

/// The individual `rotate` property.
pub fn parse_rotate(text: &[u8]) -> Option<Affine> {
    let t = trimmed(text);
    if none_or_empty(t) {
        return Some(Affine::IDENTITY);
    }
    let mut args = arguments(t);
    if args.len() == 2 && iequals(&args[0], b"z") {
        args.remove(0);
    }
    let a = if args.len() == 1 { angle(&args[0])? } else { return None };
    Some(rotation(a))
}

/// The individual `scale` property.
pub fn parse_scale(text: &[u8]) -> Option<Affine> {
    let t = trimmed(text);
    if none_or_empty(t) {
        return Some(Affine::IDENTITY);
    }
    let args = arguments(t);
    if args.is_empty() || args.len() > 3 {
        return None;
    }
    let x = scalar(&args[0])?;
    let y = if args.len() > 1 { scalar(&args[1])? } else { x };
    if args.len() == 3 && scalar(&args[2]) != Some(1.0) {
        return None;
    }
    Some(Affine { a: x as f32, b: 0.0, c: 0.0, d: y as f32, e: 0.0, f: 0.0 })
}

/// `transform-origin` within a `w` × `h` box; None when it is not 2D px, % or keywords.
pub fn parse_origin(text: &[u8], w: f32, h: f32) -> Option<(f32, f32)> {
    let mut args = arguments(trimmed(text));
    if args.is_empty() {
        return Some((w / 2.0, h / 2.0));
    }
    if args.len() > 3 || (args.len() == 3 && length(&args[2], 0.0) != Some(0.0)) {
        return None;
    }
    // A vertical keyword first (`top left`) means the same as the other order.
    if args.len() >= 2 {
        let a = lower(&args[0]);
        let b = lower(&args[1]);
        if a == b"top" || a == b"bottom" || b == b"left" || b == b"right" {
            args.swap(0, 1);
        }
    } else {
        let a = lower(&args[0]);
        if a == b"top" || a == b"bottom" {
            args = vec![b"center".to_vec(), args[0].clone()];
        }
    }
    let axis = |v: &[u8], size: f32, horizontal: bool| -> Option<f32> {
        let k = lower(v);
        if k == b"center" {
            return Some(size / 2.0);
        }
        if k == if horizontal { &b"left"[..] } else { &b"top"[..] } {
            return Some(0.0);
        }
        if k == if horizontal { &b"right"[..] } else { &b"bottom"[..] } {
            return Some(size);
        }
        length(v, f64::from(size)).map(|l| l as f32)
    };
    let x = axis(&args[0], w, true)?;
    let y = if args.len() > 1 { axis(&args[1], h, false)? } else { h / 2.0 };
    Some((x, y))
}

/// `opacity`: a number or a percentage, clamped to 0..1.
pub fn parse_opacity(text: &[u8]) -> Option<f32> {
    scalar(text).map(|v| v.clamp(0.0, 1.0) as f32)
}

const THEME_KEYS: [&str; 15] = [
    "bg_color",
    "text_color",
    "hint_color",
    "link_color",
    "button_color",
    "button_text_color",
    "secondary_bg_color",
    "accent_text_color",
    "subtitle_text_color",
    "destructive_text_color",
    "section_bg_color",
    "section_header_text_color",
    "section_separator_color",
    "header_bg_color",
    "bottom_bar_bg_color",
];

/// DEFAULT_THEME_PARAMS in sdk-policy.ts, light then dark, in THEME_KEYS order.
const THEME_DEFAULTS: [[&str; 15]; 2] = [
    [
        "#ffffff", "#000000", "#707579", "#3390ec", "#3390ec", "#ffffff", "#f4f4f5", "#3390ec", "#707579", "#e53935", "#ffffff", "#3390ec",
        "#e7e7e7", "#ffffff", "#ffffff",
    ],
    [
        "#212121", "#ffffff", "#aaaaaa", "#8774e1", "#8774e1", "#ffffff", "#181818", "#8774e1", "#aaaaaa", "#ff595a", "#212121", "#8774e1",
        "#3d3d3d", "#212121", "#212121",
    ],
];

const BUTTON_VARS: [(&str, &str); 8] = [
    ("bg", "--tg-button-bg"),
    ("text", "--tg-button-text"),
    ("radius", "--tg-button-radius"),
    ("font", "--tg-button-font"),
    ("padding", "--tg-button-padding"),
    ("height", "--tg-button-height"),
    ("pressedOpacity", "--tg-button-pressed-opacity"),
    ("disabledOpacity", "--tg-button-disabled-opacity"),
];

/// The text tokens and the reference set that stands for any the host leaves out (DEFAULT_TEXT_TOKENS).
const TEXT_VARS: [(&str, &str, &str); 3] = [
    ("fontSize", "--tg-text-font-size", "15px"),
    ("lineHeight", "--tg-text-line-height", "1.35"),
    ("padding", "--tg-text-padding", "8px 12px"),
];

const GENERIC_FAMILIES: [&[u8]; 8] =
    [b"system-ui", b"sans-serif", b"serif", b"monospace", b"ui-sans-serif", b"ui-serif", b"ui-monospace", b"ui-rounded"];

const SAFE_AREA_VARS: [(&str, &str); 6] = [
    ("top", "--tg-content-safe-area-inset-top"),
    ("right", "--tg-content-safe-area-inset-right"),
    ("bottom", "--tg-content-safe-area-inset-bottom"),
    ("left", "--tg-content-safe-area-inset-left"),
    ("cornerLeft", "--tg-content-safe-area-corner-left"),
    ("cornerRight", "--tg-content-safe-area-corner-right"),
];

fn hex_color(v: &[u8]) -> bool {
    v.len() == 7 && v[0] == b'#' && v[1..].iter().all(u8::is_ascii_hexdigit)
}

/// Consumes `[0-9]{1,int_digits}(\.[0-9]{1,frac_digits})?` at `i`.
fn consume_number(v: &[u8], i: &mut usize, int_digits: usize, frac_digits: usize) -> bool {
    let start = *i;
    while *i < v.len() && digit(v[*i]) && *i - start < int_digits {
        *i += 1;
    }
    if *i == start {
        return false;
    }
    if *i < v.len() && v[*i] == b'.' {
        *i += 1;
        let frac = *i;
        while *i < v.len() && digit(v[*i]) && *i - frac < frac_digits {
            *i += 1;
        }
        if *i == frac {
            return false;
        }
    }
    true
}

fn consume_unit(v: &[u8], i: &mut usize) -> bool {
    for unit in [&b"px"[..], b"rem", b"em"] {
        if v[*i..].starts_with(unit) {
            *i += unit.len();
            return true;
        }
    }
    false
}

/// `0`, or a length in px, em or rem: what a button token may hold.
fn token_length(v: &[u8]) -> bool {
    if v == b"0" {
        return true;
    }
    let mut i = 0;
    consume_number(v, &mut i, 4, 3) && consume_unit(v, &mut i) && i == v.len()
}

/// A number in [min, max] with at most `decimals` decimals, given as a number or as
/// `[0-9]{1,2}(.d…)` + `unit`; written back rounded, with the unit.
fn bounded(value: &Json, min: f64, max: f64, decimals: usize, unit: &[u8]) -> Option<Vec<u8>> {
    let n = match value {
        Json::Number(n) => *n,
        Json::String(v) => {
            let mut i = 0;
            if !consume_number(v, &mut i, 2, decimals) || &v[i..] != unit {
                return None;
            }
            let end = v.iter().position(|c| *c == 0).unwrap_or(v.len());
            strtod(&v[..end]).0
        }
        _ => return None,
    };
    if !n.is_finite() || n < min || n > max {
        return None;
    }
    let f = 10f64.powf(decimals as f64);
    let mut out = twidget_json::number((n * f).round() / f).into_bytes();
    out.extend_from_slice(unit);
    Some(out)
}

/// `[normal|italic] [normal|bold|N00] size[/line-height] family, …` with generic families only.
fn token_font(v: &[u8]) -> bool {
    let mut i = 0;
    let word = |i: &mut usize, words: &[&[u8]]| -> bool {
        for w in words {
            if v[*i..].starts_with(w) && *i + w.len() < v.len() && v[*i + w.len()] == b' ' {
                *i += w.len() + 1;
                return true;
            }
        }
        false
    };
    word(&mut i, &[b"normal", b"italic"]);
    if !word(&mut i, &[b"normal", b"bold"]) && i + 3 < v.len() && (b'1'..=b'9').contains(&v[i]) && &v[i + 1..i + 4] == b"00 " {
        i += 4;
    }
    if !consume_number(v, &mut i, 3, 2) || !consume_unit(v, &mut i) {
        return false;
    }
    if i < v.len() && v[i] == b'/' {
        i += 1;
        if !consume_number(v, &mut i, 3, 2) {
            return false;
        }
        consume_unit(v, &mut i);
    }
    if i >= v.len() || v[i] != b' ' {
        return false;
    }
    let families = &v[i + 1..];
    if families.is_empty() {
        return false;
    }
    let mut start = 0;
    while start <= families.len() {
        let comma = find_byte(families, b',', start);
        let family = &families[start..comma.unwrap_or(families.len())];
        let mut next = comma;
        if let Some(c) = comma
            && c + 1 < families.len()
            && families[c + 1] == b' '
        {
            next = Some(c + 1);
        }
        if family.is_empty() || !GENERIC_FAMILIES.contains(&family) {
            return false;
        }
        match next {
            None => break,
            Some(c) => start = c + 1,
        }
    }
    true
}

/// 0 to 1 with at most three decimals: `0`, `.5`, `0.25`, `1`, `1.0`.
fn token_opacity(v: &[u8]) -> bool {
    let mut i = 0;
    if v.is_empty() {
        return false;
    }
    if v[0] == b'1' {
        i = 1;
        if i < v.len() && v[i] == b'.' {
            i += 1;
            let frac = i;
            while i < v.len() && v[i] == b'0' && i - frac < 3 {
                i += 1;
            }
            if i == frac {
                return false;
            }
        }
        return i == v.len();
    }
    if v[0] == b'0' {
        i = 1;
    }
    if i < v.len() && v[i] == b'.' {
        i += 1;
        let frac = i;
        while i < v.len() && digit(v[i]) && i - frac < 3 {
            i += 1;
        }
        if i == frac {
            return false;
        }
    }
    i > 0 && i == v.len()
}

fn button_token(field: &str, value: &Json) -> Option<Vec<u8>> {
    if field == "pressedOpacity" || field == "disabledOpacity" {
        let s = match value {
            Json::Number(n) => twidget_json::number(*n).into_bytes(),
            _ => value.as_str().to_vec(),
        };
        return token_opacity(&s).then_some(s);
    }
    let Json::String(v) = value else { return None };
    match field {
        "bg" | "text" => hex_color(v).then(|| lower(v)),
        "radius" | "height" => token_length(v).then(|| v.clone()),
        "font" => token_font(v).then(|| v.clone()),
        _ => {
            // padding: one to four lengths, one space apart
            let mut parts = 0;
            let mut start = 0;
            loop {
                let space = find_byte(v, b' ', start);
                parts += 1;
                if parts > 4 || !token_length(&v[start..space.unwrap_or(v.len())]) {
                    return None;
                }
                match space {
                    None => return Some(v.clone()),
                    Some(s) => start = s + 1,
                }
            }
        }
    }
}

fn theme_variable_name(key: &str) -> Vec<u8> {
    format!("--tg-theme-{key}").replace('_', "-").into_bytes()
}

pub type Variables = Vec<(Vec<u8>, Vec<u8>)>;

/// The custom properties the host's `theme` message sets on the widget's root (widget-sandbox
/// docs/sdk-v1.md §2.3, §2.4): the palette completed from the scheme's defaults, `--tg-color-scheme`,
/// each button token whose value passes its grammar, and the text tokens completed from the reference set.
pub fn theme_variables(theme: &Json) -> Variables {
    let dark = theme["colorScheme"].as_str() == b"dark";
    let params = &theme["params"];
    let mut vars = Vec::with_capacity(THEME_KEYS.len() + 1 + BUTTON_VARS.len() + TEXT_VARS.len());
    for (k, key) in THEME_KEYS.iter().enumerate() {
        let v = params[*key].as_str();
        let value = if hex_color(v) { lower(v) } else { THEME_DEFAULTS[usize::from(dark)][k].as_bytes().to_vec() };
        vars.push((theme_variable_name(key), value));
    }
    vars.push((b"--tg-color-scheme".to_vec(), if dark { b"dark".to_vec() } else { b"light".to_vec() }));
    let button = &theme["button"];
    for (field, name) in BUTTON_VARS {
        if let Some(value) = button.find(field)
            && let Some(token) = button_token(field, value)
        {
            vars.push((name.as_bytes().to_vec(), token));
        }
    }
    let text = &theme["text"];
    for (field, name, fallback) in TEXT_VARS {
        let token = text.find(field).and_then(|value| match field {
            "fontSize" => bounded(value, 8.0, 64.0, 2, b"px"),
            "lineHeight" => bounded(value, 1.0, 3.0, 3, b""),
            _ => button_token("padding", value),
        });
        vars.push((name.as_bytes().to_vec(), token.unwrap_or_else(|| fallback.as_bytes().to_vec())));
    }
    vars
}

/// The `--tg-content-safe-area-*` properties of the host's `safeArea` message (docs/sdk-v1.md §2.17): all
/// six, each inset 0–64 and corner 0–200 px at two decimals, anything else 0px.
pub fn content_safe_area_variables(inset: &Json) -> Variables {
    SAFE_AREA_VARS
        .iter()
        .map(|(field, name)| {
            let max = if field.starts_with("corner") { 200.0 } else { 64.0 };
            let mut n = inset[*field].as_number(0.0);
            if !n.is_finite() || n < 0.0 || n > max {
                n = 0.0;
            }
            let mut value = twidget_json::number((n * 100.0).round() / 100.0).into_bytes();
            value.extend_from_slice(b"px");
            (name.as_bytes().to_vec(), value)
        })
        .collect()
}

/// A name `theme_variables` or `content_safe_area_variables` may set: the widget's own style ops cannot
/// set it on the root.
pub fn is_theme_variable(name: &[u8]) -> bool {
    name == b"--tg-color-scheme"
        || SAFE_AREA_VARS.iter().any(|(_, v)| v.as_bytes() == name)
        || BUTTON_VARS.iter().any(|(_, v)| v.as_bytes() == name)
        || TEXT_VARS.iter().any(|(_, v, _)| v.as_bytes() == name)
        || THEME_KEYS.iter().any(|k| theme_variable_name(k) == name)
}

/// `css` with every `var(--name[, fallback])` replaced by the value of `--name` in `vars`, else by its
/// fallback; a var() with neither stays.
pub fn substitute_variables(css: &[u8], vars: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(css.len());
    let mut i = 0;
    while i < css.len() {
        let Some(at) = find(css, b"var(", i) else { break };
        // The matching ')': a fallback may hold functions of its own, `rgba(…)`.
        let mut end = at + 4;
        let mut comma = None;
        let mut depth = 1;
        while end < css.len() && depth != 0 {
            match css[end] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                b',' if depth == 1 && comma.is_none() => comma = Some(end),
                _ => {}
            }
            end += 1;
        }
        if depth != 0 {
            break;
        }
        let name = trim(&css[at + 4..comma.unwrap_or(end - 1)]);
        out.extend_from_slice(&css[i..at]);
        if let Some((_, value)) = vars.iter().find(|(k, _)| *k == name) {
            out.extend_from_slice(value);
        } else if let Some(c) = comma {
            out.extend(trim(&css[c + 1..end - 1]));
        } else {
            out.extend_from_slice(&css[at..end]);
        }
        i = end;
    }
    out.extend_from_slice(&css[i..]);
    out
}

/// Every `:where(X)` as `X`, for the profile's UA layer in litehtml's master CSS.
pub fn unwrap_where(css: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(css.len());
    let mut i = 0;
    while i < css.len() {
        let Some(at) = find(css, b":where(", i) else { break };
        let mut end = at + 7;
        let mut depth = 1;
        let mut list = false;
        while end < css.len() && depth != 0 {
            match css[end] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                b',' if depth == 1 => list = true,
                _ => {}
            }
            end += 1;
        }
        if depth != 0 {
            break;
        }
        out.extend_from_slice(&css[i..at]);
        // A list followed by more of its compound selector cannot be unwrapped: `:where(a,b).x` is not `a,b.x`.
        let next = css.get(end).copied().unwrap_or(b'{');
        if list && next != b'{' && next != b',' {
            out.extend_from_slice(&css[at..end]);
        } else {
            out.extend_from_slice(&css[at + 7..end - 1]);
        }
        i = end;
    }
    out.extend_from_slice(&css[i..]);
    out
}
