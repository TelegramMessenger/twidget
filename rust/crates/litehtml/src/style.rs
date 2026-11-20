//! `style.h` and `style.cpp`, as far as the ported stages need them.

use crate::css_tokenizer::{CV_FUNCTION, CssToken, STRING, URL};
use crate::html::{WHITESPACE, lowcase, trim};

/// `parse_url(tok, url)` (https://drafts.csswg.org/css-values-4/#urls): an unquoted `url(x)`, or `url("x")`
/// and `src("x")` whose value is one string (the caller has removed whitespace), trimmed.
pub fn parse_url(tok: &CssToken) -> Option<Vec<u8>> {
    // legacy syntax without quotes: url(x.com)
    if tok.ty == URL {
        return Some(trim(&tok.str, WHITESPACE));
    }

    if tok.ty == CV_FUNCTION && matches!(&lowcase(tok.name())[..], b"url" | b"src") && tok.value.len() == 1 && tok.value[0].ty == STRING {
        return Some(trim(&tok.value[0].str, WHITESPACE));
    }

    None
}
