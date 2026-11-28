//! The C++ library's comparisons as renderer.cpp uses them on floats: `std::max(a, b)` is `a < b ? b : a`,
//! `std::min(a, b)` is `b < a ? b : a`, so a NaN keeps whichever side the comparison leaves.

pub(crate) fn cmax(a: f32, b: f32) -> f32 {
    if a < b { b } else { a }
}

pub(crate) fn cmin(a: f32, b: f32) -> f32 {
    if b < a { b } else { a }
}

/// `std::strtoll(text, nullptr, 10)` on a C string: leading whitespace, a sign, decimal digits; past `long long`'s
/// range the bound; 0 when no digit.
pub(crate) fn strtoll(text: &[u8]) -> i64 {
    let at = |i: usize| text.get(i).copied().unwrap_or(0);
    let mut i = 0;
    while matches!(at(i), b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r') {
        i += 1;
    }
    let negative = at(i) == b'-';
    if matches!(at(i), b'-' | b'+') {
        i += 1;
    }
    let mut value: i128 = 0;
    while at(i).is_ascii_digit() {
        value = (value * 10 + i128::from(at(i) - b'0')).min(i128::from(i64::MAX) + 1);
        i += 1;
    }
    let value = if negative { -value } else { value };
    value.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}
