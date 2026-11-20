//! `css_parser.h` and `css_parser.cpp`, as far as the ported stages need them: the normalizing of a text or
//! of tokens into component values (https://www.w3.org/TR/css-syntax-3/#normalize-into-a-token-stream), the
//! whitespace removal that comes with it, and the list and value checks on component values. Rules,
//! declarations and stylesheets come with stage L2.

use crate::css_tokenizer::{
    BAD_STRING, BAD_URL, COMMA, CV_FUNCTION, CssToken, CssTokenVector, EOF, FUNCTION, LEFT_BRACE, LEFT_BRACKET, LEFT_PAREN, WHITESPACE,
    mirror, tokenize,
};

/// `filter_code_points(input)` (https://www.w3.org/TR/css-syntax-3/#css-filter-code-points): `\r\n`, `\r` and
/// `\f` become `\n`, a NUL becomes U+FFFD.
pub fn filter_code_points(input: &[u8]) -> Vec<u8> {
    const XFFFD: &[u8] = b"\xEF\xBF\xBD";

    let null_count = input.iter().filter(|&&c| c == 0).count();

    let mut result = vec![0u8; input.len() + 2 * null_count];

    let mut j = 0;
    let mut i = 0;
    while i < input.len() {
        match input[i] {
            b'\r' => {
                result[j] = b'\n';
                j += 1;
                if i + 1 < input.len() && input[i + 1] == b'\n' {
                    i += 1; // skip \n after \r
                }
            }
            0x0c => {
                result[j] = b'\n';
                j += 1;
            }
            0 => {
                result[j..j + 3].copy_from_slice(XFFFD);
                j += 3;
            }
            c => {
                result[j] = c;
                j += 1;
            }
        }
        i += 1;
    }

    // trim trailing NULs: the result ends at its first NUL (`strlen`)
    let end = result.iter().position(|&c| c == 0).unwrap_or(result.len());
    result.truncate(end);
    result
}

/// `keep_whitespace_fn`: whether a whitespace token between `left` and `right` stays.
pub type KeepWhitespace<'a> = &'a dyn Fn(&CssToken, &CssToken) -> bool;

/// `kLargeSize`: past this many tokens a list drops its whitespace in one pass.
const K_LARGE_SIZE: usize = 50;

/// `at(tokens, i)`: the token, or a default one (type 0) past either end.
fn at(tokens: &[CssToken], index: usize) -> CssToken {
    tokens.get(index).cloned().unwrap_or_default()
}

fn remove_whitespace_large(tokens: &mut CssTokenVector, keep_whitespace: Option<KeepWhitespace>) {
    let mut keep_idx = Vec::with_capacity(tokens.len());
    for i in 0..tokens.len() {
        let mut keep = true;
        if tokens[i].ty == WHITESPACE {
            let left = if i > 0 { tokens[i - 1].clone() } else { CssToken::default() };
            let right = at(tokens, i + 1);
            keep = keep_whitespace.is_some_and(|f| f(&left, &right));
        } else if tokens[i].is_component_value() {
            let value = &mut tokens[i].value;
            if value.len() > K_LARGE_SIZE {
                remove_whitespace_large(value, keep_whitespace);
            } else {
                remove_whitespace_small(value, keep_whitespace);
            }
        }
        if keep {
            keep_idx.push(i);
        }
    }

    if keep_idx.len() == tokens.len() {
        return;
    }
    let mut old: Vec<Option<CssToken>> = std::mem::take(tokens).into_iter().map(Some).collect();
    let mut tmp = Vec::with_capacity(keep_idx.len());
    for idx in keep_idx {
        if let Some(token) = old[idx].take() {
            tmp.push(token);
        }
    }
    *tokens = tmp;
}

fn remove_whitespace_small(tokens: &mut CssTokenVector, keep_whitespace: Option<KeepWhitespace>) {
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].ty == WHITESPACE {
            let left = if i > 0 { tokens[i - 1].clone() } else { CssToken::default() };
            let right = at(tokens, i + 1);
            let keep = keep_whitespace.is_some_and(|f| f(&left, &right));
            if !keep {
                tokens.remove(i);
                continue;
            }
        } else if tokens[i].is_component_value() {
            let value = &mut tokens[i].value;
            if value.len() > K_LARGE_SIZE {
                remove_whitespace_large(value, keep_whitespace);
            } else {
                remove_whitespace_small(value, keep_whitespace);
            }
        }
        i += 1;
    }
}

/// `remove_whitespace(tokens, keep_whitespace)`: whitespace tokens out of the list and every component
/// value in it, but those `keep_whitespace` keeps. A long list decides each against its neighbours as
/// they were, a short one against the tokens left so far.
pub fn remove_whitespace(tokens: &mut CssTokenVector, keep_whitespace: Option<KeepWhitespace>) {
    if tokens.len() > K_LARGE_SIZE {
        remove_whitespace_large(tokens, keep_whitespace);
    } else {
        remove_whitespace_small(tokens, keep_whitespace);
    }
}

/// `componentize(tokens)`: the tokens as component values.
pub fn componentize(tokens: &mut CssTokenVector) {
    let mut parser = CssParser::new(std::mem::take(tokens));
    let mut result = Vec::new();
    loop {
        let tok = parser.consume_component_value();
        if tok.ty == EOF {
            break;
        }
        result.push(tok);
    }
    *tokens = result;
}

/// `normalize` options.
pub const f_componentize: i32 = 1;
pub const f_remove_whitespace: i32 = 2;

/// `normalize(input, options, keep_whitespace)` of tokens.
pub fn normalize_tokens(mut input: CssTokenVector, options: i32, keep_whitespace: Option<KeepWhitespace>) -> CssTokenVector {
    if options & f_componentize != 0 {
        componentize(&mut input);
    }
    if options & f_remove_whitespace != 0 {
        remove_whitespace(&mut input, keep_whitespace);
    }
    input
}

/// `normalize(input, options, keep_whitespace)` of a text: its code points filtered, tokenized, then as
/// [`normalize_tokens`].
pub fn normalize(input: &[u8], options: i32, keep_whitespace: Option<KeepWhitespace>) -> CssTokenVector {
    let input = filter_code_points(input);
    let tokens = tokenize(&input);
    normalize_tokens(tokens, options, keep_whitespace)
}

/// The parser, and whatever walks its tokens, recurses once per level of nesting (`kMaxNesting`).
const K_MAX_NESTING: i32 = 128;

/// `css_parser`: a list of tokens being consumed.
#[derive(Debug, Default)]
pub struct CssParser {
    tokens: CssTokenVector,
    index: usize,
    depth: i32,
}

impl CssParser {
    pub fn new(tokens: CssTokenVector) -> CssParser {
        CssParser { tokens, index: 0, depth: 0 }
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-the-next-input-token: a copy, EOF past the end.
    fn next_token(&mut self) -> CssToken {
        if self.index == self.tokens.len() {
            return CssToken::new(EOF);
        }
        self.index += 1;
        self.tokens[self.index - 1].clone()
    }

    /// `skip_block(closing_bracket)`: past the block's end, nested blocks and functions included.
    fn skip_block(&mut self, closing_bracket: u8) {
        let mut closers = vec![closing_bracket];
        while let Some(&last) = closers.last() {
            let token = self.next_token();
            if token.ty == EOF {
                return;
            }
            if token.ty == i32::from(last) {
                closers.pop();
            } else if token.ty == FUNCTION {
                closers.push(b')');
            } else if matches!(token.ty, LEFT_BRACE | LEFT_BRACKET | LEFT_PAREN) {
                closers.push(mirror(token.ch() as u8));
            }
        }
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-simple-block; past the nesting limit the block is empty
    /// and its contents skipped.
    pub fn consume_simple_block(&mut self, opening_bracket: u8) -> CssToken {
        let mut block = CssToken::new(-100 - i32::from(opening_bracket));

        let closing_bracket = mirror(opening_bracket);

        if self.depth >= K_MAX_NESTING {
            self.skip_block(closing_bracket);
            return block;
        }
        self.depth += 1;

        loop {
            let token = self.next_token();

            if token.ty == i32::from(closing_bracket) {
                self.depth -= 1;
                return block;
            }
            if token.ty == EOF {
                // eof in simple block
                self.depth -= 1;
                return block;
            }
            // Reconsume the current input token and append a component value.
            self.index -= 1;
            let val = self.consume_component_value();
            block.value.push(val);
        }
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-component-value
    pub fn consume_component_value(&mut self) -> CssToken {
        let token = self.next_token();

        match token.ty {
            LEFT_BRACE | LEFT_BRACKET | LEFT_PAREN => self.consume_simple_block(token.ch() as u8),
            FUNCTION => self.consume_function(&token.str),
            _ => token,
        }
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-function; past the nesting limit the function is empty.
    pub fn consume_function(&mut self, name: &[u8]) -> CssToken {
        let mut function = CssToken::with_str(CV_FUNCTION, name);

        if self.depth >= K_MAX_NESTING {
            self.skip_block(b')');
            return function;
        }
        self.depth += 1;

        loop {
            let token = self.next_token();

            match token.ty {
                0x29 => {
                    self.depth -= 1;
                    return function;
                }
                EOF => {
                    // eof in function
                    self.depth -= 1;
                    return function;
                }
                _ => {
                    self.index -= 1;
                    let val = self.consume_component_value();
                    function.value.push(val);
                }
            }
        }
    }
}

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

/// `is_any_value(tokens)` (https://drafts.csswg.org/css-syntax-3/#typedef-any-value) of component values.
pub fn is_any_value(tokens: &[CssToken]) -> bool {
    if tokens.is_empty() {
        return false;
    }
    for tok in tokens {
        if matches!(tok.ty, BAD_STRING | BAD_URL | 0x29 | 0x5D | 0x7D) {
            return false;
        }
        if tok.is_component_value() && !is_any_value(&tok.value) {
            return false;
        }
    }
    true
}

/// `is_declaration_value(tokens, index)` (https://drafts.csswg.org/css-syntax-3/#typedef-declaration-value)
/// of component values from `index`: `;` and `!` are allowed inside them only.
pub fn is_declaration_value(tokens: &[CssToken], index: usize) -> bool {
    if index >= tokens.len() {
        return false;
    }
    for tok in &tokens[index..] {
        if matches!(tok.ty, BAD_STRING | BAD_URL | 0x29 | 0x5D | 0x7D | 0x3B | 0x21) {
            return false;
        }
        if tok.is_component_value() && !is_any_value(&tok.value) {
            return false;
        }
    }
    true
}

/// `skip_whitespace(tokens, index)`: past the whitespace tokens at `index` (there may be several in a row);
/// whether there were any.
pub fn skip_whitespace(tokens: &[CssToken], index: &mut usize) -> bool {
    let start = *index;
    while tokens.get(*index).is_some_and(|t| t.ty == WHITESPACE) {
        *index += 1;
    }
    *index != start
}
