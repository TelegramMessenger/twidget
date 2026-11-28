//! `html.h` and `html.cpp`: the string and character helpers the rest of litehtml shares. Character
//! classes are ASCII only, whatever the byte (`char` is signed where the C++ is built, so a byte past 0x7f is
//! a negative value no class has); C strings end at their first NUL.

use crate::Px;
use crate::cxx;
use crate::strtod::t_strtod;

/// `whitespace`: the default trim and split set.
pub const WHITESPACE: &[u8] = b" \n\r\t\x0c";

fn find_first_not_of(s: &[u8], chars: &[u8]) -> Option<usize> {
    s.iter().position(|c| !chars.contains(c))
}

fn find_last_not_of(s: &[u8], chars: &[u8]) -> Option<usize> {
    s.iter().rposition(|c| !chars.contains(c))
}

fn find_first_of(s: &[u8], chars: &[u8], from: usize) -> Option<usize> {
    s.get(from..)?.iter().position(|c| chars.contains(c)).map(|i| i + from)
}

/// `trim(s, chars_to_trim)`.
pub fn trim(s: &[u8], chars_to_trim: &[u8]) -> Vec<u8> {
    let Some(first) = find_first_not_of(s, chars_to_trim) else { return Vec::new() };
    let last = find_last_not_of(s, chars_to_trim).unwrap_or(s.len() - 1);
    s[first..=last].to_vec()
}

/// `lcase(s)`, `lowcase(std::string)`: A–Z to a–z.
pub fn lowcase(s: &[u8]) -> Vec<u8> {
    s.iter().map(|&c| t_tolower(c)).collect()
}

/// `find_close_bracket(s, off, open_b, close_b)`: where the bracket opened at or after `off` closes.
pub fn find_close_bracket(s: &[u8], off: usize, open_b: u8, close_b: u8) -> Option<usize> {
    let mut cnt = 0;
    for i in off..s.len() {
        if s[i] == open_b {
            cnt += 1;
        } else if s[i] == close_b {
            cnt -= 1;
            if cnt == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// `split_string(str, tokens, delims, delims_preserve, quote)`: the pieces between delimiters, empty ones
/// dropped; a preserved delimiter is a piece of its own; a quote, or a bracket among the quotes, runs to
/// its closing match.
pub fn split_string(str: &[u8], delims: &[u8], delims_preserve: &[u8], quote: &[u8]) -> Vec<Vec<u8>> {
    let mut tokens = Vec::new();
    if str.is_empty() || (delims.is_empty() && delims_preserve.is_empty()) {
        return tokens;
    }

    let all_delims = [delims, delims_preserve, quote].concat();

    let mut token_start = 0;
    let mut token_end = find_first_of(str, &all_delims, token_start);
    loop {
        while let Some(end) = token_end.filter(|&e| quote.contains(&str[e])) {
            token_end = match str[end] {
                b'(' => find_close_bracket(str, end, b'(', b')'),
                b'[' => find_close_bracket(str, end, b'[', b']'),
                b'{' => find_close_bracket(str, end, b'{', b'}'),
                c => find_first_of(str, &[c], end + 1),
            };
            if let Some(end) = token_end {
                token_end = find_first_of(str, &all_delims, end + 1);
            }
        }

        let token = match token_end {
            None => &str[token_start..],
            Some(end) => &str[token_start..end],
        };
        if !token.is_empty() {
            tokens.push(token.to_vec());
        }
        if let Some(end) = token_end
            && !delims_preserve.is_empty()
            && delims_preserve.contains(&str[end])
        {
            tokens.push(vec![str[end]]);
        }

        let Some(end) = token_end else { break };
        token_start = end + 1;
        if token_start == str.len() {
            break;
        }
        token_end = find_first_of(str, &all_delims, token_start);
    }
    tokens
}

/// `join_string(str, tokens, delims)`.
pub fn join_string(tokens: &[Vec<u8>], delims: &[u8]) -> Vec<u8> {
    tokens.join(delims)
}

fn c_at(s: &[u8], i: usize) -> i32 {
    i32::from(s.get(i).copied().unwrap_or(0))
}

/// `t_strcasecmp(s1, s2)` on C strings: -1, 0 or 1.
pub fn t_strcasecmp(s1: &[u8], s2: &[u8]) -> i32 {
    let mut i = 0;
    loop {
        let c = lower_int(c_at(s1, i));
        let d = c - lower_int(c_at(s2, i));
        if d < 0 {
            return -1;
        }
        if d > 0 {
            return 1;
        }
        if c == 0 {
            return 0;
        }
        i += 1;
    }
}

/// `t_strncasecmp(s1, s2, n)` on C strings.
pub fn t_strncasecmp(s1: &[u8], s2: &[u8], n: usize) -> i32 {
    for i in 0..n {
        let c = lower_int(c_at(s1, i));
        let d = c - lower_int(c_at(s2, i));
        if d < 0 {
            return -1;
        }
        if d > 0 {
            return 1;
        }
        if c == 0 {
            return 0;
        }
    }
    0
}

/// `equal_i(s1, s2)`: the same length and equal but for ASCII case, up to a NUL.
pub fn equal_i(s1: &[u8], s2: &[u8]) -> bool {
    s1.len() == s2.len() && t_strncasecmp(s1, s2, s1.len()) == 0
}

fn wrap_index(len: usize, index: i32) -> Option<usize> {
    let index = if index < 0 { index as i64 + len as i64 } else { i64::from(index) };
    // Past the end `substr` throws std::out_of_range; no caller gets there.
    (0..=len as i64).contains(&index).then_some(index as usize)
}

/// `match(str, index, substr)`: `substr` at `index` (negative counts from the end).
pub fn match_at(str: &[u8], index: i32, substr: &[u8]) -> bool {
    let Some(at) = wrap_index(str.len(), index) else { return false };
    &str[at..(at + substr.len()).min(str.len())] == substr
}

/// `match_i(str, index, substr)`.
pub fn match_i(str: &[u8], index: i32, substr: &[u8]) -> bool {
    let Some(at) = wrap_index(str.len(), index) else { return false };
    equal_i(&str[at..(at + substr.len()).min(str.len())], substr)
}

/// `is_number(string, allow_dot)`: only digits, and points where allowed (an empty string is one).
pub fn is_number(string: &[u8], allow_dot: bool) -> bool {
    string.iter().all(|&ch| t_isdigit(i32::from(ch)) || (allow_dot && ch == b'.'))
}

/// `get_escaped_string`: C escapes for quotes, `?`, `\` and the control characters that have one.
pub fn get_escaped_string(in_str: &[u8]) -> Vec<u8> {
    let mut ret = Vec::new();
    for &ch in in_str {
        match ch {
            b'\'' => ret.extend_from_slice(b"\\'"),
            b'"' => ret.extend_from_slice(b"\\\""),
            b'?' => ret.extend_from_slice(b"\\?"),
            b'\\' => ret.extend_from_slice(b"\\\\"),
            0x07 => ret.extend_from_slice(b"\\a"),
            0x08 => ret.extend_from_slice(b"\\b"),
            0x0c => ret.extend_from_slice(b"\\f"),
            b'\n' => ret.extend_from_slice(b"\\n"),
            b'\r' => ret.extend_from_slice(b"\\r"),
            b'\t' => ret.extend_from_slice(b"\\t"),
            0x0b => ret.extend_from_slice(b"\\v"),
            _ => ret.push(ch),
        }
    }
    ret
}

/// `is_whitespace(c)`: https://infra.spec.whatwg.org/#ascii-whitespace
pub fn is_whitespace(c: i32) -> bool {
    c == b' '.into() || c == b'\t'.into() || c == b'\n'.into() || c == b'\r'.into() || c == 0x0c
}

/// `t_isalpha`, `is_letter`.
pub fn is_letter(c: i32) -> bool {
    (i32::from(b'A')..=i32::from(b'Z')).contains(&c) || (i32::from(b'a')..=i32::from(b'z')).contains(&c)
}

fn lower_int(c: i32) -> i32 {
    if (i32::from(b'A')..=i32::from(b'Z')).contains(&c) { c + i32::from(b'a' - b'A') } else { c }
}

/// `t_tolower`, `lowcase(int)` on a code point or a char's value.
pub fn lowcase_int(c: i32) -> i32 {
    lower_int(c)
}

/// `t_tolower` on a byte.
pub fn t_tolower(c: u8) -> u8 {
    c.to_ascii_lowercase()
}

/// `t_isdigit`, `is_digit`.
pub fn t_isdigit(c: i32) -> bool {
    (i32::from(b'0')..=i32::from(b'9')).contains(&c)
}

/// `is_digit` on a code point or a char's value.
pub fn is_digit(c: i32) -> bool {
    t_isdigit(c)
}

/// `is_hex_digit`.
pub fn is_hex_digit(ch: i32) -> bool {
    is_digit(ch) || (i32::from(b'a')..=i32::from(b'f')).contains(&ch) || (i32::from(b'A')..=i32::from(b'F')).contains(&ch)
}

/// `digit_value`: a digit's or a hex letter's value (any other letter gives what the formula gives).
pub fn digit_value(ch: i32) -> i32 {
    if is_digit(ch) { ch - i32::from(b'0') } else { lowcase_int(ch) - i32::from(b'a') + 10 }
}

/// `is_surrogate`.
pub fn is_surrogate(ch: i32) -> bool {
    (0xD800..0xE000).contains(&ch)
}

/// `round_f`: truncate, then up when the rest is at least a half (toward +∞ for negatives too).
pub fn round_f(val: f32) -> i32 {
    let mut int_val = cxx::f32_to_int(val);
    if val - int_val as f32 >= 0.5 {
        int_val = int_val.wrapping_add(1);
    }
    int_val
}

/// `round_d`.
pub fn round_d(val: f64) -> i32 {
    let mut int_val = cxx::f64_to_int(val);
    if val - f64::from(int_val) >= 0.5 {
        int_val = int_val.wrapping_add(1);
    }
    int_val
}

/// `t_strtof(str, &end)`: `t_strtod` narrowed to float.
pub fn t_strtof(str: &[u8]) -> (f32, usize) {
    let (v, end) = t_strtod(str);
    (v as f32, end)
}

/// `baseline_align`.
pub fn baseline_align(line_height: Px, line_base_line: Px, height: Px, baseline: Px) -> Px {
    (line_height - line_base_line) - (height - baseline)
}
