//! The C library behaviours the ported C++ leans on, in the "C" locale the widget helper runs in:
//! `strtod` (and `atof`) with the whole C grammar — whitespace, sign, `inf`, `infinity`, `nan(…)`,
//! decimal and C99 hexadecimal floats, correctly rounded — `strtof` (rounded once to a float), `strtol`, `atoi`,
//! `isspace`, `tolower`, `%.*f` and `%g`.

#![forbid(unsafe_code)]

/// C's `isspace` in the "C" locale: space, \t, \n, \v, \f, \r.
pub fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// C's `tolower` in the "C" locale: A–Z only.
pub fn to_lower(c: u8) -> u8 {
    c.to_ascii_lowercase()
}

/// The NaN `strtod` makes of `nan`: the Windows C runtime sets every payload bit; the other C libraries
/// make the default quiet NaN (their payloads from `nan(…)` are not reproduced: no ported caller lets a
/// NaN through).
#[cfg(target_env = "msvc")]
const STRTOD_NAN: u64 = 0x7fff_ffff_ffff_ffff;
#[cfg(not(target_env = "msvc"))]
const STRTOD_NAN: u64 = 0x7ff8_0000_0000_0000;

/// `strtod(text, &end)`: the value and how many bytes it took; 0 bytes (and 0.0) when no conversion
/// could be made.
pub fn strtod(text: &[u8]) -> (f64, usize) {
    let mut i = 0;
    while i < text.len() && is_space(text[i]) {
        i += 1;
    }
    let negative = match text.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let signed = |v: f64| if negative { -v } else { v };
    let rest = &text[i..];
    if starts_with_ignore_case(rest, b"inf") {
        let n = if starts_with_ignore_case(rest, b"infinity") { 8 } else { 3 };
        return (signed(f64::INFINITY), i + n);
    }
    if starts_with_ignore_case(rest, b"nan") {
        let mut n = 3;
        if rest.get(3) == Some(&b'(') {
            let close = rest[4..].iter().position(|c| !(c.is_ascii_alphanumeric() || *c == b'_'));
            if let Some(k) = close
                && rest[4 + k] == b')'
            {
                n = 4 + k + 1;
            }
        }
        return (signed(f64::from_bits(STRTOD_NAN)), i + n);
    }
    if rest.len() >= 2
        && rest[0] == b'0'
        && (rest[1] == b'x' || rest[1] == b'X')
        && let Some((value, n)) = hex_float(&rest[2..])
    {
        return (signed(value), i + 2 + n);
    }
    match decimal(rest) {
        Some((value, n)) => (signed(value), i + n),
        None => (0.0, 0),
    }
}

/// `atof`: `strtod` of the longest prefix, 0 when there is none.
pub fn atof(text: &[u8]) -> f64 {
    strtod(text).0
}

/// `atoi` for the values the ported code gives it (it is undefined past `int`): whitespace, a sign and
/// digits, saturated at the ends of `i32`.
pub fn atoi(text: &[u8]) -> i32 {
    let mut i = 0;
    while i < text.len() && is_space(text[i]) {
        i += 1;
    }
    let negative = text.get(i) == Some(&b'-');
    if matches!(text.get(i), Some(b'-' | b'+')) {
        i += 1;
    }
    let mut value: i64 = 0;
    while let Some(c) = text.get(i).filter(|c| c.is_ascii_digit()) {
        value = (value * 10 + i64::from(c - b'0')).min(i64::from(i32::MAX) + 1);
        i += 1;
    }
    let value = if negative { -value } else { value };
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// `snprintf("%.*f", precision, value)`: exact digits, ties to even, `-` kept on a negative zero; a
/// negative precision is C's default of 6.
pub fn format_fixed(value: f64, precision: i32) -> String {
    let p = if precision < 0 { 6 } else { precision as usize };
    if value.is_nan() {
        return if value.is_sign_negative() { "-nan".into() } else { "nan".into() };
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf".into() } else { "inf".into() };
    }
    format!("{value:.p$}")
}

fn starts_with_ignore_case(text: &[u8], word: &[u8]) -> bool {
    text.len() >= word.len() && text[..word.len()].eq_ignore_ascii_case(word)
}

/// A decimal float at the start of `text` (after the sign): digits with at most one point, at least one
/// digit, then an exponent when one with digits follows.
fn decimal(text: &[u8]) -> Option<(f64, usize)> {
    let mut i = 0;
    let mut digits = 0;
    while i < text.len() && text[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if text.get(i) == Some(&b'.') {
        i += 1;
        while i < text.len() && text[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if matches!(text.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(text.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        if text.get(j).is_some_and(u8::is_ascii_digit) {
            while j < text.len() && text[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    let literal = std::str::from_utf8(&text[..i]).ok()?;
    Some((literal.parse().ok()?, i))
}

/// A hexadecimal float after its `0x`: hex digits with at most one point, at least one digit, then a
/// binary exponent when a `p` with digits follows; rounded to nearest, ties to even.
fn hex_float(text: &[u8]) -> Option<(f64, usize)> {
    let (mantissa, sticky, exponent, n) = hex_parts(text)?;
    Some((binary_value(mantissa, sticky, exponent), n))
}

/// What [`hex_float`] reads: the mantissa's leading bits, whether a non-zero digit followed them, the binary
/// exponent of the mantissa's last bit and the bytes taken.
fn hex_parts(text: &[u8]) -> Option<(u128, bool, i64, usize)> {
    let mut mantissa: u128 = 0;
    let mut exponent: i64 = 0;
    let mut sticky = false;
    let mut digits = 0;
    let mut i = 0;
    let mut point = false;
    loop {
        match text.get(i) {
            Some(b'.') if !point => point = true,
            Some(c) if c.is_ascii_hexdigit() => {
                let d = u128::from(char::from(*c).to_digit(16).expect("a hex digit"));
                digits += 1;
                if mantissa >> 124 == 0 {
                    mantissa = (mantissa << 4) | d;
                    if point {
                        exponent -= 4;
                    }
                } else {
                    sticky |= d != 0;
                    if !point {
                        exponent += 4;
                    }
                }
            }
            _ => break,
        }
        i += 1;
    }
    if digits == 0 {
        return None;
    }
    if matches!(text.get(i), Some(b'p' | b'P')) {
        let mut j = i + 1;
        let negative = text.get(j) == Some(&b'-');
        if matches!(text.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        if text.get(j).is_some_and(u8::is_ascii_digit) {
            let mut e: i64 = 0;
            while let Some(c) = text.get(j).filter(|c| c.is_ascii_digit()) {
                e = (e * 10 + i64::from(c - b'0')).min(1 << 40);
                j += 1;
            }
            exponent += if negative { -e } else { e };
            i = j;
        }
    }
    Some((mantissa, sticky, exponent, i))
}

/// mantissa × 2^exponent, plus a non-zero tail below the mantissa when `sticky`, as the nearest double.
fn binary_value(mantissa: u128, sticky: bool, exponent: i64) -> f64 {
    if mantissa == 0 {
        return 0.0;
    }
    let length = 128 - i64::from(mantissa.leading_zeros());
    let top = exponent + length - 1;
    if top > 1023 {
        return f64::INFINITY;
    }
    let bits = if top >= -1022 { 53 } else { top + 1075 };
    if bits <= 0 {
        // At most half the smallest subnormal: a tie (exactly half) goes to the even zero.
        let above_half = bits == 0 && (mantissa != 1 << (length - 1) || sticky);
        return if above_half { f64::from_bits(1) } else { 0.0 };
    }
    let shift = length - bits;
    let significand = if shift <= 0 {
        mantissa << (-shift)
    } else {
        let kept = mantissa >> shift;
        let rest = mantissa & ((1u128 << shift) - 1);
        let half = 1u128 << (shift - 1);
        let up = rest > half || (rest == half && (sticky || kept & 1 == 1));
        if up { kept + 1 } else { kept }
    };
    if top >= -1022 {
        let (significand, top) = if significand >> 53 != 0 { (significand >> 1, top + 1) } else { (significand, top) };
        if top > 1023 {
            return f64::INFINITY;
        }
        f64::from_bits((((top + 1023) as u64) << 52) | (significand as u64 & ((1 << 52) - 1)))
    } else {
        // A subnormal: rounding up into 2^52 is the smallest normal, which these bits are too.
        f64::from_bits(significand as u64)
    }
}

/// mantissa × 2^exponent, plus a non-zero tail below the mantissa when `sticky`, as the nearest float:
/// [`binary_value`] for 24 bits and the float's exponents.
fn binary_value_f32(mantissa: u128, sticky: bool, exponent: i64) -> f32 {
    if mantissa == 0 {
        return 0.0;
    }
    let length = 128 - i64::from(mantissa.leading_zeros());
    let top = exponent + length - 1;
    if top > 127 {
        return f32::INFINITY;
    }
    let bits = if top >= -126 { 24 } else { top + 150 };
    if bits <= 0 {
        let above_half = bits == 0 && (mantissa != 1 << (length - 1) || sticky);
        return if above_half { f32::from_bits(1) } else { 0.0 };
    }
    let shift = length - bits;
    let significand = if shift <= 0 {
        mantissa << (-shift)
    } else {
        let kept = mantissa >> shift;
        let rest = mantissa & ((1u128 << shift) - 1);
        let half = 1u128 << (shift - 1);
        let up = rest > half || (rest == half && (sticky || kept & 1 == 1));
        if up { kept + 1 } else { kept }
    };
    if top >= -126 {
        let (significand, top) = if significand >> 24 != 0 { (significand >> 1, top + 1) } else { (significand, top) };
        if top > 127 {
            return f32::INFINITY;
        }
        f32::from_bits((((top + 127) as u32) << 23) | (significand as u32 & ((1 << 23) - 1)))
    } else {
        // A subnormal: rounding up into 2^23 is the smallest normal, which these bits are too.
        f32::from_bits(significand as u32)
    }
}

/// `strtof(text, &end)`: [`strtod`]'s grammar, the value rounded once to a float (not through a double).
pub fn strtof(text: &[u8]) -> (f32, usize) {
    let mut i = 0;
    while i < text.len() && is_space(text[i]) {
        i += 1;
    }
    let negative = match text.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let signed = |v: f32| if negative { -v } else { v };
    let rest = &text[i..];
    if starts_with_ignore_case(rest, b"inf") || starts_with_ignore_case(rest, b"nan") {
        let (value, n) = strtod(text);
        return (value as f32, n);
    }
    if rest.len() >= 2
        && rest[0] == b'0'
        && (rest[1] == b'x' || rest[1] == b'X')
        && let Some((mantissa, sticky, exponent, n)) = hex_parts(&rest[2..])
    {
        return (signed(binary_value_f32(mantissa, sticky, exponent)), i + 2 + n);
    }
    match decimal(rest) {
        Some((_, n)) => {
            let literal = std::str::from_utf8(&rest[..n]).expect("ASCII digits");
            (signed(literal.parse().expect("a decimal strtod took")), i + n)
        }
        None => (0.0, 0),
    }
}

/// `snprintf("%g", value)`: six significant digits, trailing zeros dropped, the exponent form below 1e-4 and
/// from 1e6 on.
pub fn format_general(value: f64) -> String {
    if value.is_nan() {
        return if value.is_sign_negative() { "-nan".into() } else { "nan".into() };
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf".into() } else { "inf".into() };
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    // The exponent of the value rounded to six digits decides the form, as C has it.
    let scientific = format!("{value:.5e}");
    let (mantissa, exponent) = scientific.split_once('e').expect("an exponent");
    let exponent: i32 = exponent.parse().expect("a decimal exponent");
    let strip = |digits: &str| -> String {
        if digits.contains('.') { digits.trim_end_matches('0').trim_end_matches('.').to_string() } else { digits.to_string() }
    };
    if !(-4..6).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{}e{sign}{:02}", strip(mantissa), exponent.abs());
    }
    let decimals = (5 - exponent) as usize;
    strip(&format!("{value:.decimals$}"))
}

/// `strtol(text, &end, 10)` on a C string: the value and how many bytes it took, 0 bytes (and 0) when it
/// read no digit. Past the range of C's `long` it is `LONG_MAX` or `LONG_MIN`; `long` is 32 bits on Windows
/// and 64 elsewhere.
pub fn strtol(text: &[u8]) -> (i64, usize) {
    let (min, max) = if cfg!(windows) { (i64::from(i32::MIN), i64::from(i32::MAX)) } else { (i64::MIN, i64::MAX) };
    let at = |i: usize| text.get(i).copied().unwrap_or(0);
    let mut i = 0;
    while is_space(at(i)) {
        i += 1;
    }
    let negative = at(i) == b'-';
    if matches!(at(i), b'-' | b'+') {
        i += 1;
    }
    let start = i;
    let mut value: i128 = 0;
    while at(i).is_ascii_digit() {
        value = (value * 10 + i128::from(at(i) - b'0')).min(i128::from(i64::MAX) + 1);
        i += 1;
    }
    if i == start {
        return (0, 0);
    }
    let value = if negative { -value } else { value };
    (value.clamp(i128::from(min), i128::from(max)) as i64, i)
}
