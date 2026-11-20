//! `html_microsyntaxes.cpp`: the HTML spec's integer and dimension microsyntaxes, which the presentational
//! attributes (`width`, `height`, `border`, `cellspacing`) are read with. A value is a C string: it ends at
//! its first NUL.

use crate::Px;
use crate::html::{digit_value, is_digit, is_whitespace};

/// `html_dimension_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtmlDimensionType {
    html_length,
    html_percentage,
}

fn at(str: &[u8], i: usize) -> u8 {
    str.get(i).copied().unwrap_or(0)
}

/// `strtol(str, &end, 10)` assigned to an `int`: C's `long` saturates (32 bits on Windows, 64 elsewhere), and
/// the conversion to `int` keeps the low 32 bits.
fn strtol_int(str: &[u8]) -> (i32, usize) {
    let (value, n) = twidget_cstd::strtol(str);
    (value as i32, n)
}

/// `html_parse_integer(str, val)` (https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#rules-for-parsing-integers):
/// what `strtol` reads, as an `int`.
pub fn html_parse_integer(str: &[u8]) -> Option<i32> {
    let str = crate::gumbo::c_str(str);
    let (n, end) = strtol_int(str);
    if end == 0 {
        return None;
    }
    Some(n)
}

/// `html_parse_non_negative_integer(str, val)`.
pub fn html_parse_non_negative_integer(str: &[u8]) -> Option<i32> {
    match html_parse_integer(str) {
        Some(n) if n >= 0 => Some(n),
        _ => None,
    }
}

/// `html_parse_dimension_value(str, result, type)` (https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#rules-for-parsing-dimension-values).
pub fn html_parse_dimension_value(str: &[u8]) -> Option<(f32, HtmlDimensionType)> {
    let str = crate::gumbo::c_str(str);
    // 1. Let input be the string being parsed.
    // 2. Let position be a position variable for input, initially pointing at the start of input.
    let mut position = 0;
    // 3. Skip ASCII whitespace within input given position.
    while is_whitespace(i32::from(at(str, position))) {
        position += 1;
    }
    // 4. If position is past the end of input or the code point at position within input is not an ASCII digit,
    // then return failure.
    if !is_digit(i32::from(at(str, position))) {
        return None;
    }
    // 5. Collect a sequence of code points that are ASCII digits from input given position, and interpret the
    // resulting sequence as a base-ten integer. Let value be that number.
    let (long, n) = twidget_cstd::strtol(&str[position..]);
    let mut value = long as f32;
    position += n;
    // 6. If position is past the end of input, then return value as a length.
    if at(str, position) == 0 {
        return Some((value, HtmlDimensionType::html_length));
    }
    // 7. If the code point at position within input is U+002E (.), then:
    if at(str, position) == b'.' {
        // 1. Advance position by 1.
        position += 1;
        // 2. If position is past the end of input or the code point at position within input is not an ASCII digit,
        // then return the current dimension value with value, input, and position.
        if !is_digit(i32::from(at(str, position))) {
            return Some((value, kind(at(str, position))));
        }
        // 3. Let divisor have the value 1.
        let mut divisor: f32 = 1.0;
        // 4. While true:
        loop {
            // 1. Multiply divisor by ten.
            divisor *= 10.0;
            // 2. Add the value of the code point at position within input, interpreted as a base-ten digit (0..9)
            // and divided by divisor, to value.
            value += digit_value(i32::from(at(str, position))) as f32 / divisor;
            // 3. Advance position by 1.
            position += 1;
            // 4. If position is past the end of input, then return value as a length.
            if at(str, position) == 0 {
                return Some((value, HtmlDimensionType::html_length));
            }
            // 5. If the code point at position within input is not an ASCII digit, then break.
            if !is_digit(i32::from(at(str, position))) {
                break;
            }
        }
    }
    // 8. Return the current dimension value with value, input, and position.
    Some((value, kind(at(str, position))))
}

fn kind(c: u8) -> HtmlDimensionType {
    if c == b'%' { HtmlDimensionType::html_percentage } else { HtmlDimensionType::html_length }
}

/// `html_parse_nonzero_dimension_value(str, val, type)`: as [`html_parse_dimension_value`], but a value
/// `pixel_t` takes for zero (within 1e-4) is a failure.
pub fn html_parse_nonzero_dimension_value(str: &[u8]) -> Option<(f32, HtmlDimensionType)> {
    let (x, t) = html_parse_dimension_value(str)?;
    if Px(x) == Px(0.0) {
        return None;
    }
    Some((x, t))
}
