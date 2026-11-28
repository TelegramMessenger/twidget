//! `num_cvt.cpp`: list markers as letters and Roman numerals. A number outside a style's range (below 1;
//! past 3999 for the Roman ones) is written in decimal, as css-counter-styles has it.

use crate::cxx::to_string_int;

const LATIN_LOWER: [&str; 26] =
    ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z"];
const LATIN_UPPER: [&str; 26] =
    ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z"];
const GREEK_LOWER: [&str; 24] =
    ["α", "β", "γ", "δ", "ε", "ζ", "η", "θ", "ι", "κ", "λ", "μ", "ν", "ξ", "ο", "π", "ρ", "σ", "τ", "υ", "φ", "χ", "ψ", "ω"];

/// `to_mapped_alpha(num, map)`: bijective base `map.len()`.
fn to_mapped_alpha(num: i32, map: &[&str]) -> Vec<u8> {
    if num < 1 {
        return to_string_int(num);
    }
    let mut dividend = num as usize;
    let mut out = Vec::new();
    while dividend > 0 {
        let modulo = (dividend - 1) % map.len();
        out.splice(0..0, map[modulo].bytes());
        dividend = (dividend - modulo) / map.len();
    }
    out
}

/// `num_cvt::to_latin_lower`.
pub fn to_latin_lower(val: i32) -> Vec<u8> {
    to_mapped_alpha(val, &LATIN_LOWER)
}

/// `num_cvt::to_latin_upper`.
pub fn to_latin_upper(val: i32) -> Vec<u8> {
    to_mapped_alpha(val, &LATIN_UPPER)
}

/// `num_cvt::to_greek_lower`.
pub fn to_greek_lower(val: i32) -> Vec<u8> {
    to_mapped_alpha(val, &GREEK_LOWER)
}

fn to_roman(mut value: i32, numerals: &[(i32, &str); 13]) -> Vec<u8> {
    if !(1..=3999).contains(&value) {
        return to_string_int(value);
    }
    let mut result = Vec::new();
    for &(v, numeral) in numerals {
        while value >= v {
            result.extend_from_slice(numeral.as_bytes());
            value -= v;
        }
    }
    result
}

/// `num_cvt::to_roman_lower`.
pub fn to_roman_lower(value: i32) -> Vec<u8> {
    to_roman(
        value,
        &[
            (1000, "m"),
            (900, "cm"),
            (500, "d"),
            (400, "cd"),
            (100, "c"),
            (90, "xc"),
            (50, "l"),
            (40, "xl"),
            (10, "x"),
            (9, "ix"),
            (5, "v"),
            (4, "iv"),
            (1, "i"),
        ],
    )
}

/// `num_cvt::to_roman_upper`.
pub fn to_roman_upper(value: i32) -> Vec<u8> {
    to_roman(
        value,
        &[
            (1000, "M"),
            (900, "CM"),
            (500, "D"),
            (400, "CD"),
            (100, "C"),
            (90, "XC"),
            (50, "L"),
            (40, "XL"),
            (10, "X"),
            (9, "IX"),
            (5, "V"),
            (4, "IV"),
            (1, "I"),
        ],
    )
}
