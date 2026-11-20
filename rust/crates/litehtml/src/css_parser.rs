//! `css_parser.h` and `css_parser.cpp`, as far as the ported stages need them.

use crate::css_tokenizer::{COMMA, CssToken, CssTokenVector};

/// `parse_comma_separated_list(tokens)`
/// (https://www.w3.org/TR/css-syntax-3/#parse-comma-separated-list-of-component-values): the runs between
/// top-level commas. Never empty: no tokens are one empty run.
pub fn parse_comma_separated_list(tokens: &[CssToken]) -> Vec<CssTokenVector> {
    let mut result = Vec::new();
    let mut list = Vec::new();
    for tok in tokens {
        if tok.ty == COMMA {
            result.push(std::mem::take(&mut list));
            continue;
        }
        list.push(tok.clone());
    }
    result.push(list);
    result
}
