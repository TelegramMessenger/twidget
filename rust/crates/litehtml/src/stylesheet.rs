//! `stylesheet.h` and `stylesheet.cpp`: raw rules and declarations, and `css::parse_css_stylesheet`, which
//! walks a sheet's rules into style rules under their `@media` lists, `@supports` conditions, `@layer`
//! blocks and `@import`ed sheets.
//!
//! A style rule's prelude is a selector list and its block a [`Style`] every selector of the rule shares; a
//! `@supports` declaration holds when a style keeps a property of it.

use std::sync::Arc;

use crate::css_parser::{f_componentize, normalize_tokens, parse_stylesheet, parse_stylesheet_tokens, skip_whitespace};
use crate::css_selector::{CssSelector, DocumentMode, parse_selector_list, sort_selectors, strict_mode};
use crate::css_tokenizer::{CURLY_BLOCK, CV_FUNCTION, CssToken, CssTokenVector, ROUND_BLOCK, SEMICOLON, STRING};
use crate::document_container::DocumentContainer;
use crate::html::lowcase;
use crate::media_query::{MediaDocument, MediaQueryListList, parse_media_query_list};
use crate::string_id::{_charset_, _import_, _layer_, _media_, _supports_};
use crate::style::{Style, parse_url};
use crate::{Names, StringId};

/// `raw_declaration` (https://www.w3.org/TR/cssom-1/#css-declarations): a name, its value as component
/// values, and `!important`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RawDeclaration {
    pub name: Vec<u8>,
    pub value: CssTokenVector,
    pub important: bool,
}

impl RawDeclaration {
    /// `operator bool`: a declaration has a name.
    pub fn is_some(&self) -> bool {
        !self.name.is_empty()
    }
}

/// `raw_rule::rule_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleType {
    qualified,
    at,
}

/// `raw_rule`: a qualified rule (prelude and `{}` block) or an at-rule (name, prelude, and a block or not),
/// half parsed.
#[derive(Clone, Debug, PartialEq)]
pub struct RawRule {
    pub ty: RuleType,
    pub name: Vec<u8>,
    pub prelude: CssTokenVector,
    /// A CURLY_BLOCK, or a default token (type 0) for an at-rule without one.
    pub block: CssToken,
}

impl RawRule {
    pub fn new(ty: RuleType, name: Vec<u8>) -> RawRule {
        RawRule { ty, name, prelude: Vec::new(), block: CssToken::default() }
    }
}

/// What `css::parse_css_stylesheet` asks of its document and of the stages that come later.
pub trait StyleSheetHost {
    fn names(&self) -> &Names;

    /// `doc->container()`.
    fn container(&self) -> &dyn DocumentContainer;

    /// The document as media queries read it.
    fn media_document(&self) -> MediaDocument<'_>;

    /// `doc->mode()`.
    fn mode(&self) -> DocumentMode;

    /// `doc->add_media_list(list)`.
    fn add_media_list(&self, list: &Arc<MediaQueryListList>);
}

/// A style rule as the walk keeps it: its prelude, its base url, and the C++'s `style` of its block, which
/// every selector of the rule shares.
#[derive(Clone, Debug)]
pub struct StyleRule {
    pub prelude: CssTokenVector,
    pub style: Style,
    pub baseurl: Vec<u8>,
}

/// What `css::parse_css_stylesheet` reads: a text, or the component values of a block.
#[derive(Clone, Copy, Debug)]
pub enum StyleSheetInput<'a> {
    Text(&'a [u8]),
    Tokens(&'a [CssToken]),
}

/// `css`: a stylesheet's selectors in sheet order.
#[derive(Clone, Debug, Default)]
pub struct Css {
    m_selectors: Vec<Arc<CssSelector>>,
    /// Inside an `@layer` block.
    m_layer_depth: i32,
}

impl Css {
    pub fn selectors(&self) -> &[Arc<CssSelector>] {
        &self.m_selectors
    }

    /// `sort_selectors()`: by specificity, then order.
    pub fn sort_selectors(&mut self) {
        sort_selectors(&mut self.m_selectors);
    }

    /// `add_selector(selector)`.
    fn add_selector(&mut self, mut selector: CssSelector) {
        selector.m_order = self.m_selectors.len() as i32;
        selector.m_layered = self.m_layer_depth > 0;
        self.m_selectors.push(Arc::new(selector));
    }

    /// `parse_css_stylesheet(input, baseurl, doc, media, top_level)`
    /// (https://www.w3.org/TR/css-syntax-3/#parse-a-css-stylesheet): its style rules, then the at-rules the
    /// container lets through: `@charset` ignored, `@import` before any rule, `@media`, `@supports` and
    /// `@layer` blocks read as sheets of their own. Cascade layers are not implemented: a layer statement is
    /// ignored and a layer block's rules are taken as if unlayered, but marked.
    pub fn parse_css_stylesheet(
        &mut self,
        input: StyleSheetInput,
        baseurl: &[u8],
        doc: &dyn StyleSheetHost,
        media: Option<&Arc<MediaQueryListList>>,
        top_level: bool,
    ) {
        if let Some(media) = media {
            doc.add_media_list(media);
        }

        let rules = match input {
            StyleSheetInput::Text(text) => parse_stylesheet(text, top_level),
            StyleSheetInput::Tokens(tokens) => parse_stylesheet_tokens(tokens, top_level),
        };
        let mut import_allowed = top_level;

        for rule in &rules {
            if rule.ty == RuleType::qualified {
                if self.parse_style_rule(rule, baseurl, doc, media) {
                    import_allowed = false;
                }
                continue;
            }

            // an at-rule
            let name = lowcase(&rule.name);
            if !doc.container().sanitize_at_rule(&name) {
                continue;
            }
            match doc.names().id(&name) {
                // ignored  https://www.w3.org/TR/css-syntax-3/#charset-rule
                _charset_ => {}
                _import_ => {
                    if import_allowed {
                        self.parse_import_rule(rule, baseurl, doc, media);
                    }
                    // else: incorrect placement of @import rule
                }
                // https://www.w3.org/TR/css-conditional-3/#at-media  @media <media-query-list> { <stylesheet> }
                _media_ => {
                    if rule.block.ty != CURLY_BLOCK {
                        continue;
                    }
                    let mq_list = parse_media_query_list(&rule.prelude, &doc.media_document());
                    // An empty media query list evaluates to true.
                    let new_media = if mq_list.empty() {
                        media.cloned()
                    } else {
                        let mut list = media.map(|m| (**m).clone()).unwrap_or_default();
                        list.add(mq_list);
                        Some(Arc::new(list))
                    };
                    self.parse_css_stylesheet(StyleSheetInput::Tokens(&rule.block.value), baseurl, doc, new_media.as_ref(), false);
                    import_allowed = false;
                }
                // https://drafts.csswg.org/css-conditional-3/#at-supports  @supports <supports-condition> { <stylesheet> }
                _supports_ => {
                    if rule.block.ty != CURLY_BLOCK {
                        continue;
                    }
                    let condition = normalize_tokens(rule.prelude.clone(), f_componentize, None);
                    if eval_supports_condition(&condition, doc) {
                        self.parse_css_stylesheet(StyleSheetInput::Tokens(&rule.block.value), baseurl, doc, media, false);
                    }
                    import_allowed = false;
                }
                // https://drafts.csswg.org/css-cascade-5/#at-layer  @layer <layer-name># ;  @layer <layer-name>? { <stylesheet> }
                _layer_ => {
                    if rule.block.ty == CURLY_BLOCK {
                        self.m_layer_depth += 1;
                        self.parse_css_stylesheet(StyleSheetInput::Tokens(&rule.block.value), baseurl, doc, media, false);
                        self.m_layer_depth -= 1;
                        import_allowed = false;
                    }
                    // a layer statement is allowed before @import and does not disallow it
                }
                // unrecognized rule
                StringId(_) => {}
            }
        }
    }

    /// `parse_import_rule(rule, baseurl, doc, media)` (https://drafts.csswg.org/css-cascade-5/#at-import):
    /// `@import [ <url> | <string> ] <media-query-list>?`, without `layer` and `supports`; the container gives
    /// the sheet's text and base url.
    fn parse_import_rule(&mut self, rule: &RawRule, baseurl: &[u8], doc: &dyn StyleSheetHost, media: Option<&Arc<MediaQueryListList>>) {
        let tokens = &rule.prelude;
        let mut index = 0;
        skip_whitespace(tokens, &mut index);
        let default = CssToken::default();
        let tok = tokens.get(index).unwrap_or(&default);
        let url = match parse_url(tok) {
            Some(url) => url,
            None if tok.ty == STRING => tok.str.clone(),
            // invalid @import rule
            None => return,
        };
        let mut css_baseurl = baseurl.to_vec();
        let css_text = doc.container().import_css(&url, &mut css_baseurl);

        let rest = &tokens[(index + 1).min(tokens.len())..];
        let mq_list = parse_media_query_list(rest, &doc.media_document());
        let new_media = if mq_list.empty() {
            media.cloned()
        } else {
            let mut list = media.map(|m| (**m).clone()).unwrap_or_default();
            list.add(mq_list);
            Some(Arc::new(list))
        };

        self.parse_css_stylesheet(StyleSheetInput::Text(&css_text), &css_baseurl, doc, new_media.as_ref(), true);
    }

    /// `parse_style_rule(rule, baseurl, doc, media)` (https://www.w3.org/TR/css-syntax-3/#style-rules): false
    /// when the prelude is no selector list; else each selector, with the rule's style and media lists, its
    /// specificity reckoned.
    fn parse_style_rule(
        &mut self,
        rule: &RawRule,
        baseurl: &[u8],
        doc: &dyn StyleSheetHost,
        media: Option<&Arc<MediaQueryListList>>,
    ) -> bool {
        let list = parse_selector_list(doc.names(), &rule.prelude, strict_mode, doc.mode());
        if list.is_empty() {
            // invalid selector
            return false;
        }

        // The content of the qualified rule's block is parsed as a style block's contents.
        let mut block = Style::default();
        block.add(doc.names(), &rule.block.value, baseurl, Some(doc.container()));
        let style = Arc::new(StyleRule { prelude: rule.prelude.clone(), style: block, baseurl: baseurl.to_vec() });

        for sel in list {
            let mut sel = Arc::try_unwrap(sel).unwrap_or_else(|shared| (*shared).clone());
            sel.m_style = Some(Arc::clone(&style));
            sel.m_media_query = media.cloned();
            sel.calc_specificity();
            self.add_selector(sel);
        }
        true
    }
}

/// `eval_supports_declaration(tokens, container)`: `( <declaration> )` holds when it is one declaration that
/// gives a property.
fn eval_supports_declaration(tokens: &[CssToken], doc: &dyn StyleSheetHost) -> bool {
    if tokens.iter().any(|t| t.ch() == SEMICOLON) {
        return false;
    }
    // the declaration is supported if litehtml was able to parse it
    let mut st = Style::default();
    st.add(doc.names(), tokens, b"", Some(doc.container()));
    !st.empty()
}

fn at(tokens: &[CssToken], index: usize) -> CssToken {
    tokens.get(index).cloned().unwrap_or_default()
}

/// `<supports-in-parens> = ( <supports-condition> ) | <supports-feature> | <general-enclosed>`: false when it
/// is none of them; a function evaluates false, a block that is no condition is a declaration.
fn parse_supports_in_parens(token: &CssToken, result: &mut bool, doc: &dyn StyleSheetHost) -> bool {
    // <general-enclosed> = <function-token> <any-value>? ), unsupported
    if token.ty == CV_FUNCTION {
        *result = false;
        return true;
    }
    if token.ty != ROUND_BLOCK {
        return false;
    }

    // ( <supports-condition> )
    let mut index = 0;
    let mut value = false;
    if parse_supports_condition(&token.value, &mut index, &mut value, doc) {
        skip_whitespace(&token.value, &mut index);
        if index == token.value.len() {
            *result = value;
            return true;
        }
    }
    // ( <declaration> ), or ( <any-value>? ), which is false
    *result = eval_supports_declaration(&token.value, doc);
    true
}

/// https://drafts.csswg.org/css-conditional-3/#typedef-supports-condition: `not <supports-in-parens>`, or
/// supports-in-parens joined by `and` or by `or` (never both).
fn parse_supports_condition(tokens: &[CssToken], index: &mut usize, result: &mut bool, doc: &dyn StyleSheetHost) -> bool {
    let parse_operand = |index: &mut usize, value: &mut bool| -> bool {
        skip_whitespace(tokens, index);
        if !parse_supports_in_parens(&at(tokens, *index), value, doc) {
            return false;
        }
        *index += 1;
        true
    };
    let next_ident = |index: &mut usize| -> Vec<u8> {
        skip_whitespace(tokens, index);
        lowcase(&at(tokens, *index).ident())
    };

    if next_ident(index) == b"not" {
        *index += 1;
        if !parse_operand(index, result) {
            return false;
        }
        *result = !*result;
        return true;
    }

    if !parse_operand(index, result) {
        return false;
    }

    let op = next_ident(index);
    if op != b"and" && op != b"or" {
        // a single <supports-in-parens>; the caller checks what follows it
        return true;
    }
    while next_ident(index) == op {
        *index += 1;
        let mut operand = false;
        if !parse_operand(index, &mut operand) {
            return false;
        }
        *result = if op == b"and" { *result && operand } else { *result || operand };
    }
    true
}

/// `eval_supports_condition(tokens, container)`: an invalid condition is false.
fn eval_supports_condition(tokens: &[CssToken], doc: &dyn StyleSheetHost) -> bool {
    let mut index = 0;
    let mut result = false;
    if !parse_supports_condition(tokens, &mut index, &mut result, doc) {
        return false;
    }
    skip_whitespace(tokens, &mut index);
    index == tokens.len() && result
}

/// The `( … )` blocks of a `@supports` prelude that may be read as a declaration: every round block in it.
pub fn supports_candidates(prelude: &[CssToken], out: &mut Vec<CssTokenVector>) {
    for token in prelude {
        if token.ty == ROUND_BLOCK {
            out.push(token.value.clone());
        }
        supports_candidates(&token.value, out);
    }
}
