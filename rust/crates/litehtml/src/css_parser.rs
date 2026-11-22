//! `css_parser.h` and `css_parser.cpp`: the normalizing of a text or of tokens into component values
//! (https://www.w3.org/TR/css-syntax-3/#normalize-into-a-token-stream) and the whitespace removal that comes
//! with it, the parse of a stylesheet into rules, of a style block into declarations and nested rules, and
//! the list and value checks on component values.

use crate::css_tokenizer::{
    AMPERSAND, AT_KEYWORD, BAD_STRING, BAD_URL, CDC, CDO, COLON, COMMA, CURLY_BLOCK, CV_FUNCTION, CssToken, CssTokenVector, EOF, FUNCTION,
    IDENT, LEFT_BRACE, LEFT_BRACKET, LEFT_PAREN, SEMICOLON, WHITESPACE, mirror, tokenize,
};
use crate::encodings::{Encoding, decode};
use crate::stylesheet::{RawDeclaration, RawRule, RuleType};

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
    tokens: std::vec::IntoIter<CssToken>,
    depth: i32,
}

/// `css_parser::parse_stylesheet(input, top_level)` (https://www.w3.org/TR/css-syntax-3/#parse-stylesheet) of
/// bytes: decoded as UTF-8 (a BOM may say UTF-16), normalized without componentizing, then as
/// [`parse_stylesheet_tokens`].
pub fn parse_stylesheet(input: &[u8], top_level: bool) -> Vec<RawRule> {
    // 1. Decode: UTF-8 is always assumed, broken UTF-8 made valid.
    let str = decode(input, Encoding::utf_8);
    // 2. Normalize.
    let tokens = normalize(&str, 0, None);
    CssParser::new(tokens).consume_list_of_rules(top_level)
}

/// `css_parser::parse_stylesheet(input, top_level)` of tokens: the list of rules, the top-level flag set
/// or not (https://www.w3.org/TR/css-syntax-3/#consume-list-of-rules).
pub fn parse_stylesheet_tokens(input: &[CssToken], top_level: bool) -> Vec<RawRule> {
    CssParser::new(input.to_vec()).consume_list_of_rules(top_level)
}

/// `trim_whitespace(tokens)`: whitespace tokens off both ends.
pub fn trim_whitespace(tokens: &mut CssTokenVector) {
    while tokens.first().is_some_and(|t| t.ty == WHITESPACE) {
        tokens.remove(0);
    }
    while tokens.last().is_some_and(|t| t.ty == WHITESPACE) {
        tokens.pop();
    }
}

impl CssParser {
    pub fn new(tokens: CssTokenVector) -> CssParser {
        CssParser { tokens: tokens.into_iter(), depth: 0 }
    }

    fn next_token(&mut self) -> CssToken {
        self.tokens.next().unwrap_or_else(|| CssToken::new(EOF))
    }

    fn peek_type(&self) -> i32 {
        self.tokens.as_slice().first().map_or(EOF, |token| token.ty)
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-list-of-rules: CDO and CDC are skipped at the top level.
    pub fn consume_list_of_rules(&mut self, top_level: bool) -> Vec<RawRule> {
        let mut rules = Vec::new();

        loop {
            match self.peek_type() {
                WHITESPACE => {
                    self.next_token();
                }
                EOF => {
                    self.next_token();
                    return rules;
                }
                CDO | CDC if top_level => {
                    self.next_token();
                }
                AT_KEYWORD => {
                    if let Some(rule) = self.consume_at_rule() {
                        rules.push(rule);
                    }
                }
                _ => {
                    if let Some(rule) = self.consume_qualified_rule() {
                        rules.push(rule);
                    }
                }
            }
        }
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-qualified-rule: none when the input ends before its block.
    pub fn consume_qualified_rule(&mut self) -> Option<RawRule> {
        let mut rule = RawRule::new(RuleType::qualified, Vec::new());

        loop {
            let token = self.next_token();

            match token.ty {
                // eof in qualified rule
                EOF => return None,
                LEFT_BRACE => {
                    rule.block = self.consume_simple_block(b'{');
                    return Some(rule);
                }
                CURLY_BLOCK => {
                    rule.block = token;
                    return Some(rule);
                }
                _ => {
                    let value = self.component_value(token);
                    rule.prelude.push(value);
                }
            }
        }
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-at-rule: always a rule, ended by `;`, a block or the input.
    pub fn consume_at_rule(&mut self) -> Option<RawRule> {
        let token = self.next_token();
        let mut rule = RawRule::new(RuleType::at, token.str);

        loop {
            let token = self.next_token();

            match token.ty {
                SEMICOLON => return Some(rule),
                // eof in at-rule
                EOF => return Some(rule),
                LEFT_BRACE => {
                    rule.block = self.consume_simple_block(b'{');
                    return Some(rule);
                }
                CURLY_BLOCK => {
                    rule.block = token;
                    return Some(rule);
                }
                _ => {
                    let value = self.component_value(token);
                    rule.prelude.push(value);
                }
            }
        }
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-declaration, the next token an IDENT: its name, `:`, and the
    /// value with `!important` taken off and the whitespace of both ends (the leading one too, unlike the
    /// standard); a declaration without a name (as the C++ returns `{}`) when no `:` follows the name.
    pub fn consume_declaration(&mut self) -> RawDeclaration {
        let token = self.next_token();
        let mut decl = RawDeclaration { name: token.str, value: Vec::new(), important: false };

        // 1. Whitespace.
        while self.peek_type() == WHITESPACE {
            self.next_token();
        }

        // 2. Anything but `:` is a parse error.
        if self.peek_type() != COLON {
            return RawDeclaration::default();
        }
        self.next_token();

        // 3. Whitespace.
        while self.peek_type() == WHITESPACE {
            self.next_token();
        }

        // 4. Component values to the end.
        while self.peek_type() != EOF {
            let value = self.consume_component_value();
            decl.value.push(value);
        }

        // 5. `!` `important` at the end.
        trim_whitespace(&mut decl.value);
        let n = decl.value.len();
        if n >= 1 && decl.value[n - 1].ident() == b"important" && n >= 2 && decl.value[n - 2].ch() == i32::from(b'!') {
            decl.value.truncate(n - 2);
            decl.important = true;
        }

        // 6. Whitespace at the end.
        trim_whitespace(&mut decl.value);

        decl
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-style-block: the declarations and the rules (`@` rules and
    /// `&` nested ones) of a style block, apart.
    pub fn consume_style_block_contents(&mut self, decls: &mut Vec<RawDeclaration>, rules: &mut Vec<RawRule>) {
        loop {
            match self.peek_type() {
                WHITESPACE | SEMICOLON => {
                    self.next_token();
                }
                EOF => {
                    self.next_token();
                    return;
                }
                AT_KEYWORD => {
                    if let Some(rule) = self.consume_at_rule() {
                        rules.push(rule);
                    }
                }
                IDENT => {
                    // The token and every component value to `;` or the end, then a declaration of them.
                    let mut temp = vec![self.next_token()];
                    while !matches!(self.peek_type(), SEMICOLON | EOF) {
                        let value = self.consume_component_value();
                        temp.push(value);
                    }

                    let decl = CssParser::new(temp).consume_declaration();
                    if decl.is_some() {
                        decls.push(decl);
                    }
                }
                AMPERSAND => {
                    if let Some(rule) = self.consume_qualified_rule() {
                        rules.push(rule);
                    }
                }
                _ => {
                    // unexpected token in a style block: component values to `;` or the end, thrown away
                    while !matches!(self.peek_type(), SEMICOLON | EOF) {
                        self.consume_component_value();
                    }
                }
            }
        }
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
            let val = self.component_value(token);
            block.value.push(val);
        }
    }

    /// https://www.w3.org/TR/css-syntax-3/#consume-component-value
    pub fn consume_component_value(&mut self) -> CssToken {
        let token = self.next_token();
        self.component_value(token)
    }

    fn component_value(&mut self, token: CssToken) -> CssToken {
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
                    let val = self.component_value(token);
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
