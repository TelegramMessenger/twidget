//! `css_selector.h` and `css_selector.cpp`: selectors (https://www.w3.org/TR/selectors-4/) parsed from
//! component values, and their specificity. Matching them is `html_tag.cpp`'s, in [`crate::html_tag`].

use std::sync::Arc;

use crate::css_parser::{normalize, parse_comma_separated_list, skip_whitespace};
use crate::css_tokenizer::{CV_FUNCTION, CssHashType, CssToken, CssTokenVector, HASH, IDENT, SQUARE_BLOCK, STRING, get_repr};
use crate::html::{WHITESPACE, equal_i, is_whitespace, lowcase, trim};
use crate::media_query::MediaQueryListList;
use crate::string_id::*;
use crate::stylesheet::StyleRule;
use crate::{Names, StringId};

/// `document_mode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DocumentMode {
    #[default]
    no_quirks_mode,
    quirks_mode,
    limited_quirks_mode,
}

/// `selector_specificity`: (a, b, c, d), compared in that order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SelectorSpecificity {
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub d: i32,
}

impl std::ops::AddAssign for SelectorSpecificity {
    fn add_assign(&mut self, val: SelectorSpecificity) {
        self.a += val.a;
        self.b += val.b;
        self.c += val.c;
        self.d += val.d;
    }
}

impl PartialOrd for SelectorSpecificity {
    fn partial_cmp(&self, other: &SelectorSpecificity) -> Option<std::cmp::Ordering> {
        Some((self.a, self.b, self.c, self.d).cmp(&(other.a, other.b, other.c, other.d)))
    }
}

/// `attr_select_type`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AttrSelectType {
    #[default]
    select_class,
    select_id,
    select_attr,
    select_pseudo_class,
    select_pseudo_element,
}

/// `attr_matcher` (https://www.w3.org/TR/selectors-4/#attribute-selectors): `=` and the character before it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AttrMatcher {
    #[default]
    attribute_exists,
    /// `=`
    attribute_equals,
    /// `*=`
    attribute_contains_string,
    /// `~=`
    attribute_contains_word,
    /// `^=`
    attribute_starts_with_string,
    /// `|=`
    attribute_starts_with_string_hyphen,
    /// `$=`
    attribute_ends_with_string,
}

impl AttrMatcher {
    /// The C++ value: 0 or the character.
    pub fn ch(self) -> u8 {
        match self {
            AttrMatcher::attribute_exists => 0,
            AttrMatcher::attribute_equals => b'=',
            AttrMatcher::attribute_contains_string => b'*',
            AttrMatcher::attribute_contains_word => b'~',
            AttrMatcher::attribute_starts_with_string => b'^',
            AttrMatcher::attribute_starts_with_string_hyphen => b'|',
            AttrMatcher::attribute_ends_with_string => b'$',
        }
    }
}

/// `css_attribute_selector`: a subclass or pseudo-element selector. "None" (the C++ `operator bool` false)
/// is an empty name.
#[derive(Clone, Debug)]
pub struct CssAttributeSelector {
    pub ty: AttrSelectType,
    /// `[prefix|name]`
    pub prefix: StringId,
    /// `.name`, `#name`, `[name]`, `:name`
    pub name: StringId,
    /// `[name=value]`, `:lang(value)`
    pub value: Vec<u8>,
    pub matcher: AttrMatcher,
    /// The value is matched ASCII case-insensitively.
    pub caseless_match: bool,
    /// `:is()`, `:where()`, `:not()`, `:nth-child(… of S)`: shared by every copy, as the C++ shares it.
    pub selector_list: Arc<Vec<Arc<CssSelector>>>,
    /// `:nth-child(an+b)`
    pub a: i32,
    pub b: i32,
}

impl Default for CssAttributeSelector {
    fn default() -> CssAttributeSelector {
        CssAttributeSelector::new(AttrSelectType::select_class, empty_id)
    }
}

impl CssAttributeSelector {
    /// `css_attribute_selector(type, name)`, the name interned by the caller.
    pub fn new(ty: AttrSelectType, name: StringId) -> CssAttributeSelector {
        CssAttributeSelector {
            ty,
            prefix: empty_id,
            name,
            value: Vec::new(),
            matcher: AttrMatcher::attribute_exists,
            caseless_match: false,
            selector_list: Arc::new(Vec::new()),
            a: 0,
            b: 0,
        }
    }

    /// `operator bool`.
    pub fn is_some(&self) -> bool {
        self.name != empty_id
    }
}

/// `css_element_selector`: a compound selector, `div.class:hover`.
#[derive(Clone, Debug)]
pub struct CssElementSelector {
    pub m_prefix: StringId,
    pub m_tag: StringId,
    /// Subclass and pseudo-element selectors.
    pub m_attrs: Vec<CssAttributeSelector>,
}

impl Default for CssElementSelector {
    fn default() -> CssElementSelector {
        CssElementSelector { m_prefix: empty_id, m_tag: empty_id, m_attrs: Vec::new() }
    }
}

/// `css_combinator`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CssCombinator {
    /// ` `
    #[default]
    combinator_descendant,
    /// `>`
    combinator_child,
    /// `+`
    combinator_adjacent_sibling,
    /// `~`
    combinator_general_sibling,
}

impl CssCombinator {
    /// The C++ value: the character.
    pub fn ch(self) -> u8 {
        match self {
            CssCombinator::combinator_descendant => b' ',
            CssCombinator::combinator_child => b'>',
            CssCombinator::combinator_adjacent_sibling => b'+',
            CssCombinator::combinator_general_sibling => b'~',
        }
    }

    fn from_ch(c: i32) -> CssCombinator {
        match c {
            0x3E => CssCombinator::combinator_child,
            0x2B => CssCombinator::combinator_adjacent_sibling,
            0x7E => CssCombinator::combinator_general_sibling,
            _ => CssCombinator::combinator_descendant,
        }
    }
}

/// `css_selector`: a complex selector, `div + p`, as a chain to the left; with the order, media lists, style
/// and layer flag of the rule it comes from once a stylesheet has it.
#[derive(Clone, Debug, Default)]
pub struct CssSelector {
    pub m_specificity: SelectorSpecificity,
    pub m_order: i32,
    pub m_left: Option<Arc<CssSelector>>,
    pub m_right: CssElementSelector,
    pub m_combinator: CssCombinator,
    pub m_media_query: Option<Arc<MediaQueryListList>>,
    /// The rule's style; the declarations as `style::add` takes them until stage L5 ports the style.
    pub m_style: Option<Arc<StyleRule>>,
    /// From an `@layer` block.
    pub m_layered: bool,
}

impl CssSelector {
    /// `calc_specificity()`: d for a type, b for each id, c for each other subclass or pseudo-element (`:is()`,
    /// `:not()` and `:nth-child(… of S)` count one, whatever their arguments), nothing for `:where()`; the
    /// left part's added. It adds to what the selector had.
    pub fn calc_specificity(&mut self) {
        if self.m_right.m_tag != star_id {
            self.m_specificity.d = 1;
        }
        for attr in &self.m_right.m_attrs {
            // :where() and its arguments add nothing  https://www.w3.org/TR/selectors-4/#zero-matches
            if attr.ty == AttrSelectType::select_pseudo_class && attr.name == _where_ {
                continue;
            }
            if attr.ty == AttrSelectType::select_id {
                self.m_specificity.b += 1;
            } else {
                self.m_specificity.c += 1;
            }
        }
        if let Some(left) = &mut self.m_left {
            let left = Arc::make_mut(left);
            left.calc_specificity();
            self.m_specificity += left.m_specificity;
        }
    }

    /// `is_media_valid()`: no media lists, or lists in use.
    pub fn is_media_valid(&self) -> bool {
        self.m_media_query.as_ref().is_none_or(|m| m.is_used())
    }

    /// `operator<`: by specificity, then order.
    pub fn less(&self, other: &CssSelector) -> bool {
        if self.m_specificity == other.m_specificity {
            return self.m_order < other.m_order;
        }
        self.m_specificity < other.m_specificity
    }

    /// `css_selector::parse(text, mode)`: one complex selector from text.
    pub fn parse(names: &Names, text: &[u8], mode: DocumentMode) -> Option<CssSelector> {
        let tokens = normalize(text, crate::css_parser::f_componentize, None);
        parse_complex_selector(names, &tokens, mode, &mut Compounds::default()).map(|s| (*s).clone())
    }
}

/// `parse_selector_list` options.
pub const strict_mode: i32 = 0;
pub const forgiving_mode: i32 = 1;
pub const forbid_pseudo_elements: i32 = 1 << 1;

/// The C++'s thread-local `g_selector_depth` and `g_compounds`: the compounds of one top-level complex
/// selector, nested lists included, counted against `kMaxCompounds`.
#[derive(Debug, Default)]
struct Compounds {
    depth: i32,
    count: i32,
}

/// `kMaxCompounds`: specificity, matching and destruction recurse once per compound.
const K_MAX_COMPOUNDS: i32 = 128;

fn at(tokens: &[CssToken], index: usize) -> &CssToken {
    static INVALID: std::sync::OnceLock<CssToken> = std::sync::OnceLock::new();
    tokens.get(index).unwrap_or_else(|| INVALID.get_or_init(CssToken::default))
}

/// `parse_ns_prefix` (https://www.w3.org/TR/selectors-4/#typedef-ns-prefix): `[ <ident-token> | '*' ]? '|'`.
fn parse_ns_prefix(tokens: &[CssToken], index: &mut usize) -> Vec<u8> {
    let a = at(tokens, *index);
    let b = at(tokens, *index + 1);

    if a.ch() == i32::from(b'|') {
        *index += 1;
        return Vec::new();
    }

    if (a.ty == IDENT || a.ch() == i32::from(b'*')) && b.ch() == i32::from(b'|') {
        *index += 2;
        return if a.ty == IDENT { a.name().to_vec() } else { b"*".to_vec() };
    }

    Vec::new()
}

/// `wq_name`.
#[derive(Debug, Default)]
struct WqName {
    prefix: Vec<u8>,
    name: Vec<u8>,
}

/// `parse_wq_name`: `<ns-prefix>? <ident-token>`, no whitespace between; `[x|=a]` is the name `x`.
fn parse_wq_name(tokens: &[CssToken], index: &mut usize) -> WqName {
    let start = *index;
    let prefix = parse_ns_prefix(tokens, index);

    let tok = at(tokens, *index);
    if tok.ty == IDENT {
        *index += 1;
        return WqName { prefix, name: tok.name().to_vec() };
    }
    // back to before the prefix when no ident follows it
    *index = start;

    // a name taken for a prefix, as in [x|=a]
    let tok = at(tokens, *index);
    if tok.ty == IDENT {
        *index += 1;
        return WqName { prefix: Vec::new(), name: tok.name().to_vec() };
    }

    WqName::default()
}

/// `parse_type_selector` (https://www.w3.org/TR/selectors-4/#typedef-type-selector): `<ns-prefix>? [
/// <ident-token> | '*' ]`, lower-case (ASCII case-insensitive in HTML whatever the mode).
fn parse_type_selector(tokens: &[CssToken], index: &mut usize) -> WqName {
    let start = *index;
    let prefix = parse_ns_prefix(tokens, index);

    let tok = at(tokens, *index);
    if tok.ty == IDENT || tok.ch() == i32::from(b'*') {
        *index += 1;
        let name = if tok.ty == IDENT { tok.name().to_vec() } else { b"*".to_vec() };
        return WqName { prefix: lowcase(&prefix), name: lowcase(&name) };
    }
    *index = start;
    WqName::default()
}

/// `parse_attr_matcher`: `[ '~' | '|' | '^' | '$' | '*' ]? '='`.
fn parse_attr_matcher(tokens: &[CssToken], index: &mut usize) -> Option<AttrMatcher> {
    let a = at(tokens, *index);
    let b = at(tokens, *index + 1);

    if a.ch() == i32::from(b'=') {
        *index += 1;
        return Some(AttrMatcher::attribute_equals);
    }

    let matcher = match a.ch() {
        0x7E => AttrMatcher::attribute_contains_word,
        0x7C => AttrMatcher::attribute_starts_with_string_hyphen,
        0x5E => AttrMatcher::attribute_starts_with_string,
        0x24 => AttrMatcher::attribute_ends_with_string,
        0x2A => AttrMatcher::attribute_contains_string,
        _ => return None,
    };
    if b.ch() != i32::from(b'=') {
        return None;
    }
    *index += 2;
    Some(matcher)
}

/// The attributes whose values HTML matches ASCII case-insensitively unless `s` is given
/// (https://html.spec.whatwg.org/multipage/semantics-other.html#case-sensitivity-of-selectors).
const SPECIAL_ATTRIBUTES: [&str; 46] = [
    "accept",
    "accept-charset",
    "align",
    "alink",
    "axis",
    "bgcolor",
    "charset",
    "checked",
    "clear",
    "codetype",
    "color",
    "compact",
    "declare",
    "defer",
    "dir",
    "direction",
    "disabled",
    "enctype",
    "face",
    "frame",
    "hreflang",
    "http-equiv",
    "lang",
    "language",
    "link",
    "media",
    "method",
    "multiple",
    "nohref",
    "noresize",
    "noshade",
    "nowrap",
    "readonly",
    "rel",
    "rev",
    "rules",
    "scope",
    "scrolling",
    "selected",
    "shape",
    "target",
    "text",
    "type",
    "valign",
    "valuetype",
    "vlink",
];

/// `parse_attribute_selector(block)` (https://www.w3.org/TR/selectors-4/#typedef-attribute-selector):
/// `'[' <wq-name> ']'` or `'[' <wq-name> <attr-matcher> [ <string-token> | <ident-token> ] <attr-modifier>? ']'`.
fn parse_attribute_selector(names: &Names, block: &CssToken) -> CssAttributeSelector {
    let mut selector = CssAttributeSelector::default();

    let tokens = &block.value;
    let mut index = 0;

    // <wq-name>
    skip_whitespace(tokens, &mut index);
    let wq_name = parse_wq_name(tokens, &mut index);
    if wq_name.name.is_empty() {
        return CssAttributeSelector::default();
    }

    // attribute names are ASCII case-insensitive in HTML whatever the mode
    let prefix = lowcase(&wq_name.prefix);
    let name = lowcase(&wq_name.name);

    skip_whitespace(tokens, &mut index);
    if index == tokens.len() {
        // [name]
        selector.ty = AttrSelectType::select_attr;
        selector.prefix = names.id(&prefix);
        selector.name = names.id(&name);
        selector.matcher = AttrMatcher::attribute_exists;
        return selector;
    }

    // <attr-matcher>
    skip_whitespace(tokens, &mut index);
    let Some(matcher) = parse_attr_matcher(tokens, &mut index) else { return CssAttributeSelector::default() };

    // <string-token> | <ident-token>
    skip_whitespace(tokens, &mut index);
    let value = at(tokens, index);
    if value.ty != STRING && value.ty != IDENT {
        return CssAttributeSelector::default();
    }
    index += 1;

    // <attr-modifier>?
    skip_whitespace(tokens, &mut index);
    let mut modifier = 0u8;
    let tok = at(tokens, index);
    if tok.ty == IDENT {
        let ident = tok.ident();
        if ident == b"s" {
            modifier = b's';
        } else if ident == b"i" {
            modifier = b'i';
        } else {
            // junk at the end of the attribute selector
            return CssAttributeSelector::default();
        }
        index += 1;
    }

    skip_whitespace(tokens, &mut index);
    if index != tokens.len() {
        // junk at the end of the attribute selector
        return CssAttributeSelector::default();
    }

    selector.ty = AttrSelectType::select_attr;
    selector.prefix = names.id(&prefix);
    selector.name = names.id(&name);
    selector.matcher = matcher;
    selector.caseless_match = modifier == b'i' || (modifier == 0 && SPECIAL_ATTRIBUTES.iter().any(|a| a.as_bytes() == name));
    selector.value = if selector.caseless_match { lowcase(value.str.as_slice()) } else { value.str.clone() };
    selector
}

/// `to_int(s, number)`: the whole of `s` as `strtol` reads it (leading whitespace and a sign allowed), made
/// an `int` as the C++ narrows a `long`.
fn to_int(s: &[u8]) -> Option<i32> {
    if s.is_empty() {
        return None;
    }
    let (n, end) = twidget_cstd::strtol(s);
    // the C string ends at a NUL, and `end` must reach the string's own end
    if end != s.len() {
        return None;
    }
    Some(n as i32)
}

/// `parse_an_b(s)` (https://www.w3.org/TR/css-syntax-3/#anb-syntax) on the text of the tokens: `even`, `odd`,
/// `B`, or `An+B` with optional parts; spaces after `n` and after the sign of B are allowed, escapes are not.
fn parse_an_b(s: &[u8]) -> Option<(i32, i32)> {
    let s = lowcase(&trim(s, WHITESPACE));
    if s == b"even" {
        return Some((2, 0));
    }
    if s == b"odd" {
        return Some((2, 1));
    }

    let Some(i) = s.iter().position(|c| *c == b'n') else {
        return to_int(&s).map(|b| (0, b));
    };

    let str_a = &s[..i];
    let mut str_b = trim(&s[i + 1..], WHITESPACE);

    let a = if str_a.is_empty() || str_a == b"+" || str_a == b"-" { if str_a == b"-" { -1 } else { 1 } } else { to_int(str_a)? };

    let b = if !str_b.is_empty() {
        if str_b[0] == b'+' || str_b[0] == b'-' {
            // spaces after the sign are allowed
            while is_whitespace(i32::from(str_b.get(1).copied().unwrap_or(0))) {
                str_b.remove(1);
            }
        }
        to_int(&str_b)?
    } else {
        0
    };

    Some((a, b))
}

/// `find_of_keyword(tokens)`.
fn find_of_keyword(tokens: &[CssToken]) -> Option<usize> {
    tokens.iter().position(|t| t.ident() == b"of")
}

/// `parse_nth_child(token, of_keyword, mode)`: `:nth-child(An+B [of S]?)` and the like
/// (https://www.w3.org/TR/selectors-4/#the-nth-child-pseudo), S a forgiving selector list without
/// pseudo-elements, which may be empty.
fn parse_nth_child(
    names: &Names,
    token: &CssToken,
    of_keyword: bool,
    mode: DocumentMode,
    compounds: &mut Compounds,
) -> CssAttributeSelector {
    let mut selector = CssAttributeSelector::new(AttrSelectType::select_pseudo_class, names.id(&lowcase(token.name())));

    let tokens = &token.value;

    let i = if of_keyword { find_of_keyword(tokens) } else { None };
    if let Some(i) = i {
        let selector_tokens = &tokens[i + 1..];
        selector.selector_list =
            Arc::new(parse_selector_list_in(names, selector_tokens, forgiving_mode + forbid_pseudo_elements, mode, compounds));
    }

    // the text between "nth-child(" and "of" or ")", comments left out
    let str = get_repr(tokens, 0, i.map_or(-1, |i| i as i32), false);

    let Some((a, b)) = parse_an_b(&str) else { return CssAttributeSelector::default() };
    selector.a = a;
    selector.b = b;
    selector
}

/// `parse_function_pseudo_class(token, mode)`: `:nth-*()`, `:is()`, `:where()`, `:not()`, `:lang()`.
fn parse_function_pseudo_class(names: &Names, token: &CssToken, mode: DocumentMode, compounds: &mut Compounds) -> CssAttributeSelector {
    let name = lowcase(token.name());
    if name == b"nth-child" || name == b"nth-last-child" {
        return parse_nth_child(names, token, true, mode, compounds);
    }
    if name == b"nth-of-type" || name == b"nth-last-of-type" {
        return parse_nth_child(names, token, false, mode, compounds);
    }
    // https://www.w3.org/TR/selectors-4/#matches  :is() and :where() differ only in specificity
    if name == b"is" || name == b"where" {
        let mut selector = CssAttributeSelector::new(AttrSelectType::select_pseudo_class, names.id(&name));
        selector.selector_list =
            Arc::new(parse_selector_list_in(names, &token.value, forgiving_mode + forbid_pseudo_elements, mode, compounds));
        return selector;
    }
    // https://www.w3.org/TR/selectors-4/#negation
    if name == b"not" {
        let mut selector = CssAttributeSelector::new(AttrSelectType::select_pseudo_class, names.id(&name));
        selector.selector_list =
            Arc::new(parse_selector_list_in(names, &token.value, strict_mode + forbid_pseudo_elements, mode, compounds));
        if selector.selector_list.is_empty() {
            return CssAttributeSelector::default();
        }
        return selector;
    }
    // https://www.w3.org/TR/selectors-4/#the-lang-pseudo
    if name == b"lang" {
        let mut selector = CssAttributeSelector::new(AttrSelectType::select_pseudo_class, names.id(&name));
        selector.value = get_repr(&token.value, 0, -1, false);
        return selector;
    }
    CssAttributeSelector::default()
}

/// The simple pseudo-classes the fork accepts; `:host` never matches (shadow trees are not implemented),
/// but is listed so that `:root, :host` keeps its other selector.
const SUPPORTED_SIMPLE_PSEUDO_CLASSES: [&str; 24] = [
    "any-link",
    "link",
    "visited",
    "local-link",
    "target",
    "target-within",
    "scope",
    "hover",
    "active",
    "focus",
    "focus-visible",
    "focus-within",
    "root",
    "empty",
    "first-child",
    "last-child",
    "only-child",
    "first-of-type",
    "last-of-type",
    "only-of-type",
    "checked",
    "disabled",
    "enabled",
    "host",
];

/// `is_supported_simple_pseudo_class(name)`.
fn is_supported_simple_pseudo_class(name: &[u8]) -> bool {
    let name = lowcase(name);
    SUPPORTED_SIMPLE_PSEUDO_CLASSES.iter().any(|n| n.as_bytes() == name)
}

/// `parse_pseudo_class(tokens, index, mode)` (https://www.w3.org/TR/selectors-4/#typedef-pseudo-class-selector):
/// `:` and a supported name, or `:` and a supported function.
fn parse_pseudo_class(
    names: &Names,
    tokens: &[CssToken],
    index: &mut usize,
    mode: DocumentMode,
    compounds: &mut Compounds,
) -> CssAttributeSelector {
    let a = at(tokens, *index);
    let b = at(tokens, *index + 1);

    if a.ch() != i32::from(b':') {
        return CssAttributeSelector::default();
    }

    if b.ty == IDENT {
        // unsupported pseudo-classes are invalid  https://www.w3.org/TR/selectors-4/#w3c-partial
        if !is_supported_simple_pseudo_class(&b.ident()) {
            return CssAttributeSelector::default();
        }
        *index += 2;
        return CssAttributeSelector::new(AttrSelectType::select_pseudo_class, names.id(&b.ident()));
    }
    if b.ty == CV_FUNCTION {
        let sel = parse_function_pseudo_class(names, b, mode, compounds);
        if sel.is_some() {
            *index += 2;
        }
        return sel;
    }
    CssAttributeSelector::default()
}

/// `parse_subclass_selector(tokens, index, mode)` (https://www.w3.org/TR/selectors-4/#typedef-subclass-selector):
/// an id (a hash of type id), a class, an attribute selector or a pseudo-class; ids and classes lower-case in
/// quirks mode.
fn parse_subclass_selector(
    names: &Names,
    tokens: &[CssToken],
    index: &mut usize,
    mode: DocumentMode,
    compounds: &mut Compounds,
) -> CssAttributeSelector {
    let mut selector = CssAttributeSelector::default();

    let tok0 = at(tokens, *index);
    let tok1 = at(tokens, *index + 1);

    match tok0.ty {
        HASH => {
            if tok0.hash_type == CssHashType::css_hash_id {
                *index += 1;
                selector.ty = AttrSelectType::select_id;
                let name = if mode == DocumentMode::quirks_mode { lowcase(tok0.name()) } else { tok0.name().to_vec() };
                selector.name = names.id(&name);
                return selector;
            }
            CssAttributeSelector::default()
        }
        0x2E => {
            if tok1.ty == IDENT {
                *index += 2;
                selector.ty = AttrSelectType::select_class;
                let name = if mode == DocumentMode::quirks_mode { lowcase(tok1.name()) } else { tok1.name().to_vec() };
                selector.name = names.id(&name);
                return selector;
            }
            CssAttributeSelector::default()
        }
        SQUARE_BLOCK => {
            let selector = parse_attribute_selector(names, tok0);
            if selector.is_some() {
                *index += 1;
            }
            selector
        }
        _ => parse_pseudo_class(names, tokens, index, mode, compounds),
    }
}

/// The pseudo-elements the fork accepts. Only `::before` and `::after` make a box; the rest parse and never
/// match, so that a rule such as `.btn, ::file-selector-button` keeps its other selectors.
fn is_supported_simple_pseudo_element(name: &[u8]) -> bool {
    let name = lowcase(name);
    [
        "first-line",
        "first-letter",
        "selection",
        "before",
        "after",
        "marker",
        "placeholder",
        "backdrop",
        "-webkit-slider-thumb",
        "-webkit-slider-runnable-track",
        "file-selector-button",
    ]
    .iter()
    .any(|n| n.as_bytes() == name)
}

/// `parse_pseudo_element(tokens, index)`: `::` and a supported name, or the four legacy ones with one `:`.
fn parse_pseudo_element(names: &Names, tokens: &[CssToken], index: &mut usize) -> CssAttributeSelector {
    let a = at(tokens, *index);
    let b = at(tokens, *index + 1);
    let c = at(tokens, *index + 2);

    if a.ch() != i32::from(b':') {
        return CssAttributeSelector::default();
    }
    if b.ch() != i32::from(b':') && b.ty != IDENT {
        return CssAttributeSelector::default();
    }

    if b.ty == IDENT {
        // https://www.w3.org/TR/selectors-4/#single-colon-pseudos
        let ident = b.ident();
        if !matches!(&ident[..], b"before" | b"after" | b"first-line" | b"first-letter") {
            return CssAttributeSelector::default();
        }
        *index += 2;
        return CssAttributeSelector::new(AttrSelectType::select_pseudo_element, names.id(&ident));
    }

    if c.ty == IDENT {
        if !is_supported_simple_pseudo_element(&c.ident()) {
            return CssAttributeSelector::default();
        }
        *index += 3;
        return CssAttributeSelector::new(AttrSelectType::select_pseudo_element, names.id(&c.ident()));
    }

    CssAttributeSelector::default()
}

/// `parse_compound_selector(tokens, index, mode)` (https://www.w3.org/TR/selectors-4/#typedef-compound-selector):
/// a type, subclass selectors, then pseudo-elements each with pseudo-classes after it; at least one part,
/// the type `*` when none is given.
fn parse_compound_selector(
    names: &Names,
    tokens: &[CssToken],
    index: &mut usize,
    mode: DocumentMode,
    compounds: &mut Compounds,
) -> Option<CssElementSelector> {
    let mut selector = CssElementSelector::default();

    // <type-selector>?
    let wq_name = parse_type_selector(tokens, index);
    selector.m_prefix = names.id(&wq_name.prefix);
    selector.m_tag = names.id(&wq_name.name);

    // <subclass-selector>*
    loop {
        let sel = parse_subclass_selector(names, tokens, index, mode, compounds);
        if !sel.is_some() {
            break;
        }
        selector.m_attrs.push(sel);
    }

    // [ <pseudo-element-selector> <pseudo-class-selector>* ]*
    loop {
        let sel = parse_pseudo_element(names, tokens, index);
        if !sel.is_some() {
            break;
        }
        selector.m_attrs.push(sel);

        loop {
            let sel = parse_pseudo_class(names, tokens, index, mode, compounds);
            if !sel.is_some() {
                break;
            }
            selector.m_attrs.push(sel);
        }
    }

    // at least one part  https://www.w3.org/TR/css-values-4/#mult-req
    if selector.m_tag == empty_id && selector.m_attrs.is_empty() {
        return None;
    }

    if selector.m_tag == empty_id {
        selector.m_tag = star_id;
    }

    Some(selector)
}

/// `parse_combinator(tokens, index)`: `>`, `+` or `~` with the whitespace around it, or whitespace alone
/// (the descendant combinator); 0 for none. `||` is not implemented.
fn parse_combinator(tokens: &[CssToken], index: &mut usize) -> i32 {
    let ws = skip_whitespace(tokens, index);

    let tok = at(tokens, *index);
    if matches!(tok.ch(), 0x3E | 0x2B | 0x7E) {
        let ch = tok.ch();
        *index += 1;
        skip_whitespace(tokens, index);
        return ch;
    }

    if ws { i32::from(b' ') } else { 0 }
}

/// `has_pseudo_element(selector)`.
fn has_pseudo_element(selector: &CssElementSelector) -> bool {
    selector.m_attrs.iter().any(|a| a.ty == AttrSelectType::select_pseudo_element)
}

/// `generated_part_selector(selector)`: `::placeholder`, `::-webkit-slider-runnable-track` and
/// `::-webkit-slider-thumb` become the elements the renderer makes for them (`lh-placeholder`, `lh-range-track`
/// as children, `lh-range-thumb` as a descendant), the pseudo-classes after them theirs.
fn generated_part_selector(names: &Names, mut selector: CssSelector) -> CssSelector {
    let attrs = &selector.m_right.m_attrs;
    for i in 0..attrs.len() {
        if attrs[i].ty != AttrSelectType::select_pseudo_element {
            continue;
        }
        let name = lowcase(&names.s(attrs[i].name));
        let tag: &[u8] = match &name[..] {
            b"placeholder" => b"lh-placeholder",
            b"-webkit-slider-runnable-track" => b"lh-range-track",
            b"-webkit-slider-thumb" => b"lh-range-thumb",
            _ => return selector,
        };
        let mut part = CssSelector::default();
        part.m_right.m_tag = names.id(tag);
        part.m_right.m_attrs = selector.m_right.m_attrs[i + 1..].to_vec();
        selector.m_right.m_attrs.truncate(i);
        if selector.m_right.m_tag == empty_id {
            selector.m_right.m_tag = star_id;
        }
        part.m_combinator =
            if name == b"-webkit-slider-thumb" { CssCombinator::combinator_descendant } else { CssCombinator::combinator_child };
        part.m_left = Some(Arc::new(selector));
        return part;
    }
    selector
}

/// `parse_complex_selector(tokens, mode)`: compounds and combinators over all the tokens; none past 128
/// compounds counted with the nested lists.
fn parse_complex_selector(names: &Names, tokens: &[CssToken], mode: DocumentMode, compounds: &mut Compounds) -> Option<Arc<CssSelector>> {
    if compounds.depth == 0 {
        compounds.count = 0;
    }
    compounds.depth += 1;
    let result = parse_complex_selector_counted(names, tokens, mode, compounds);
    compounds.depth -= 1;
    result
}

fn parse_complex_selector_counted(
    names: &Names,
    tokens: &[CssToken],
    mode: DocumentMode,
    compounds: &mut Compounds,
) -> Option<Arc<CssSelector>> {
    let mut index = 0;
    skip_whitespace(tokens, &mut index);
    let sel = parse_compound_selector(names, tokens, &mut index, mode, compounds);
    let sel = sel?;
    compounds.count += 1;
    if compounds.count > K_MAX_COMPOUNDS {
        return None;
    }

    let mut selector = CssSelector { m_right: sel, ..CssSelector::default() };

    // All the whitespace goes through parse_combinator; a complex selector takes every token.
    loop {
        let combinator = parse_combinator(tokens, &mut index);
        if index == tokens.len() {
            // 0: the index was at the end before the call
            return if combinator == 0 || combinator == i32::from(b' ') {
                Some(Arc::new(generated_part_selector(names, selector)))
            } else {
                None
            };
        }
        if combinator == 0 {
            // not the end and no combinator
            return None;
        }

        // only the last compound may hold a pseudo-element  https://www.w3.org/TR/selectors-4/#typedef-complex-selector
        if has_pseudo_element(&selector.m_right) {
            return None;
        }

        let sel = parse_compound_selector(names, tokens, &mut index, mode, compounds)?;
        compounds.count += 1;
        if compounds.count > K_MAX_COMPOUNDS {
            return None;
        }

        selector = CssSelector {
            m_left: Some(Arc::new(selector)),
            m_right: sel,
            m_combinator: CssCombinator::from_ch(combinator),
            ..CssSelector::default()
        };
    }
}

/// `has_selector(selector, type, name)`: whether a compound of the chain has a subclass or pseudo-element
/// selector of the type (and the name, ASCII case-insensitively; any name for an empty one).
pub fn has_selector(names: &Names, selector: &CssSelector, ty: AttrSelectType, name: &[u8]) -> bool {
    for sel in &selector.m_right.m_attrs {
        if sel.ty == ty && (name.is_empty() || equal_i(&names.s(sel.name), name)) {
            return true;
        }
    }
    match &selector.m_left {
        Some(left) => has_selector(names, left, ty, name),
        None => false,
    }
}

/// `parse_selector_list(tokens, options, mode)` (https://www.w3.org/TR/selectors-4/#selector-list): the
/// complex selectors between commas; in forgiving mode a bad one is left out, in strict mode it fails the list.
pub fn parse_selector_list(names: &Names, tokens: &[CssToken], options: i32, mode: DocumentMode) -> Vec<Arc<CssSelector>> {
    parse_selector_list_in(names, tokens, options, mode, &mut Compounds::default())
}

fn parse_selector_list_in(
    names: &Names,
    tokens: &[CssToken],
    options: i32,
    mode: DocumentMode,
    compounds: &mut Compounds,
) -> Vec<Arc<CssSelector>> {
    let list_of_lists: Vec<CssTokenVector> = parse_comma_separated_list(tokens);
    let mut result = Vec::new();

    for list in &list_of_lists {
        let selector = parse_complex_selector(names, list, mode, compounds);

        let bad = match &selector {
            None => true,
            Some(s) => (options & forbid_pseudo_elements) != 0 && has_selector(names, s, AttrSelectType::select_pseudo_element, b""),
        };
        if bad {
            // forgiving: the bad selector is left out; strict: the whole list fails
            if options & forgiving_mode != 0 {
                continue;
            }
            return Vec::new();
        }

        if let Some(selector) = selector {
            result.push(selector);
        }
    }
    result
}

/// `css::sort_selectors()`'s order: by specificity, then order.
pub fn sort_selectors(selectors: &mut [Arc<CssSelector>]) {
    selectors.sort_by(|a, b| {
        if a.less(b) {
            std::cmp::Ordering::Less
        } else if b.less(a) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
}
