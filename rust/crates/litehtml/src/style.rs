//! `style.h` and `style.cpp`: a style block (a rule's declarations, a `style` attribute, a presentational
//! hint) parsed into the typed values the fork keeps per property, before any cascade: keywords as their
//! index, lengths, colours, images and the rest, shorthands expanded into their longhands in the fork's
//! order, `!important` and the order of declaration kept, invalid values dropped as the fork drops them, and
//! what waits for the element kept as tokens: values with `var()`, custom properties, and the properties the
//! computed style parses itself (`transform`, `box-shadow`, `filter`, grid lines…). The names the renderer
//! keeps raw get a `-lh-raw-` twin with their tokens.
//!
//! What the C++ keeps for the life of the process is explicit here: the `-lh-raw-` names are interned in a
//! [`Names`] the first time a style asks for them (as the C++'s function-local static is, once per process),
//! and the cache `style::add(text)` keeps of each declaration's effect is a [`DeclarationCache`]. The C++'s
//! thread-local switches (`recorded_ops`, `suppress_raw`) belong to the style being added to.
//!
//! `subst_vars` asks the element whose style it is for its custom properties through a [`VarElement`], which
//! the cascade gives.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Mutex;

use crate::css_effects::{fold_math, has_foldable_math, has_viewport_units};
use crate::css_length::{CssLength, f_integer, f_length, f_length_percentage, f_number, f_positive};
use crate::css_parser::{
    CssParser, f_componentize, f_remove_whitespace, is_declaration_value, normalize, parse_comma_separated_list, remove_whitespace,
};
use crate::css_position::CssSize;
use crate::css_tokenizer::{
    CV_FUNCTION, CssHashType, CssNumberType, CssToken, CssTokenVector, IDENT, NUMBER, Number, PERCENTAGE, STRING, URL, get_repr,
};
use crate::css_values::*;
use crate::cxx;
use crate::document_container::{DocumentContainer, resolver};
use crate::gradient::{Image, ImageType, parse_gradient};
use crate::html::{WHITESPACE, lowcase, trim};
use crate::string_id::*;
use crate::types::{contain_inline_size, contain_layout, contain_paint, contain_size, contain_style};
use crate::web_color::{WebColor, parse_color};
use crate::{Names, Px, StringId};

// ---- Values ---------------------------------------------------------------------------------------

/// `css_wide_keyword::kind_t`: `initial`, `unset` or `revert`, which stand for what the element and the
/// property make of them when the property is read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CssWideKeyword {
    #[default]
    initial,
    unset,
    revert,
}

/// The alternatives of `property_value`, in the C++ variant's order ([`Value::index`] is its `index()`).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Value {
    /// `invalid`: "not found".
    #[default]
    Invalid,
    /// `inherit`.
    Inherit,
    CssWideKeyword(CssWideKeyword),
    /// A keyword's index, flags, or an integer.
    Int(i32),
    IntVector(Vec<i32>),
    Length(CssLength),
    LengthVector(Vec<CssLength>),
    Float(f32),
    Color(WebColor),
    Images(Vec<Image>),
    String(Vec<u8>),
    Strings(Vec<Vec<u8>>),
    Sizes(Vec<CssSize>),
    /// Tokens: a value with `var()` (its parse waits for the element), a custom property, or a property the
    /// computed style parses.
    Tokens(CssTokenVector),
}

impl Value {
    /// `index()`: the alternative's place in the variant.
    pub fn index(&self) -> u8 {
        match self {
            Value::Invalid => 0,
            Value::Inherit => 1,
            Value::CssWideKeyword(_) => 2,
            Value::Int(_) => 3,
            Value::IntVector(_) => 4,
            Value::Length(_) => 5,
            Value::LengthVector(_) => 6,
            Value::Float(_) => 7,
            Value::Color(_) => 8,
            Value::Images(_) => 9,
            Value::String(_) => 10,
            Value::Strings(_) => 11,
            Value::Sizes(_) => 12,
            Value::Tokens(_) => 13,
        }
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Value {
        Value::Int(v)
    }
}

impl From<CssLength> for Value {
    fn from(v: CssLength) -> Value {
        Value::Length(v)
    }
}

impl From<WebColor> for Value {
    fn from(v: WebColor) -> Value {
        Value::Color(v)
    }
}

/// `property_value`: a value with `!important`, whether it waits for `var()`, and its place in the order of
/// declaration.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropertyValue {
    pub value: Value,
    pub m_important: bool,
    /// The value is tokens whose parse waits for `var()`.
    pub m_has_var: bool,
    pub m_order: i32,
}

impl PropertyValue {
    /// `property_value(val, important)`.
    pub fn new(value: Value, important: bool) -> PropertyValue {
        PropertyValue { value, m_important: important, m_has_var: false, m_order: 0 }
    }

    /// `property_value(val, important, has_var)`.
    pub fn with_var(value: Value, important: bool, has_var: bool) -> PropertyValue {
        PropertyValue { value, m_important: important, m_has_var: has_var, m_order: 0 }
    }
}

/// `get_property`'s answer for a property a style does not declare.
static INVALID_PROPERTY: PropertyValue = PropertyValue { value: Value::Invalid, m_important: false, m_has_var: false, m_order: 0 };

/// `at(vec, index)`'s invalid item: a token of type 0.
static INVALID_TOKEN: CssToken = CssToken {
    ty: 0,
    str: Vec::new(),
    n: Number { number: 0.0, number_type: CssNumberType::css_number_integer },
    hash_type: CssHashType::css_hash_unrestricted,
    value: Vec::new(),
    repr: Vec::new(),
};

/// `at(vec, index)`: the token at `index` (a negative one counts from the end), or a token of type 0.
pub fn at(tokens: &[CssToken], index: i32) -> &CssToken {
    let n = tokens.len() as i64;
    let mut index = i64::from(index);
    if index < 0 {
        index += n;
    }
    if index >= 0 && index < n { &tokens[index as usize] } else { &INVALID_TOKEN }
}

/// `parse_keyword(tok, val, keywords, first_keyword_value)`: the identifier's index in the table, plus
/// `first`.
pub fn parse_keyword(tok: &CssToken, keywords: CssValues, first: i32) -> Option<i32> {
    keywords.value_index(&tok.ident()).map(|idx| first + idx)
}

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

/// `parse_length(tok, length, options, keywords)`: `from_token` on the length as it is.
pub fn parse_length(tok: &CssToken, length: &mut CssLength, options: i32, keywords: CssValues) -> bool {
    length.from_token(tok, options, keywords)
}

const AUTO: [&str; 1] = split_css_values::<1>("auto");
const NONE: [&str; 1] = split_css_values::<1>("none");
const NORMAL: [&str; 1] = split_css_values::<1>("normal");
const AUTO_CURRENTCOLOR: [&str; 2] = split_css_values::<2>("auto;currentcolor");
const WIDTH_KEYWORDS: [&str; 4] = split_css_values::<4>("auto;min-content;max-content;fit-content");

/// `style::m_valid_values`: the keywords of the properties that take one (or a comma list of them).
fn valid_values(name: StringId) -> CssValues {
    CssValues::new(match name {
        _display_ => &style_display_strings,
        _visibility_ => &visibility_strings,
        _position_ => &element_position_strings,
        _float_ => &element_float_strings,
        _clear_ => &element_clear_strings,
        _overflow_ | _overflow_x_ | _overflow_y_ => &overflow_strings,
        _appearance_ => &appearance_strings,
        _resize_ => &resize_strings,
        _box_sizing_ => &box_sizing_strings,

        _text_align_ => &text_align_strings,
        _direction_ => &direction_strings,
        _vertical_align_ => &vertical_align_strings,
        _text_transform_ => &text_transform_strings,
        __webkit_font_smoothing_ => &font_smoothing_strings,
        _white_space_ => &white_space_strings,
        _overflow_wrap_ => &overflow_wrap_strings,
        _word_break_ => &word_break_strings,
        _user_select_ => &user_select_strings,

        _font_style_ => &font_style_strings,
        _font_variant_ => &font_variant_strings,
        _font_weight_ => &font_weight_strings,

        _list_style_type_ => &list_style_type_strings,
        _list_style_position_ => &list_style_position_strings,

        _border_left_style_ | _border_right_style_ | _border_top_style_ | _border_bottom_style_ => &border_style_strings,
        _border_collapse_ => &border_collapse_strings,
        _table_layout_ => &table_layout_strings,
        _outline_style_ => &border_style_strings,

        // these 4 properties are comma-separated lists of keywords, see parse_keyword_comma_list
        _background_attachment_ => &background_attachment_strings,
        _background_repeat_ => &background_repeat_strings,
        _background_clip_ | _background_origin_ => &background_box_strings,

        _flex_direction_ => &flex_direction_strings,
        _flex_wrap_ => &flex_wrap_strings,
        _justify_content_ => &flex_justify_content_strings,
        _align_content_ => &flex_align_content_strings,
        _align_items_ | _align_self_ => &flex_align_items_strings,

        _caption_side_ => &caption_side_strings,

        _text_decoration_style_ => &style_text_decoration_style_strings,
        _text_emphasis_position_ => &style_text_emphasis_position_strings,
        // std::map::at throws for any other; no caller asks for one
        _ => &[],
    })
}

/// `shorthands`: the longhands a shorthand's `inherit`, `initial`, `unset` and `revert` go to, in this order.
pub fn shorthand(name: StringId) -> &'static [StringId] {
    match name {
        _font_ => &[_font_style_, _font_variant_, _font_variant_numeric_, _font_weight_, _font_size_, _line_height_, _font_family_],
        _background_ => &[
            _background_color_,
            _background_position_x_,
            _background_position_y_,
            _background_repeat_,
            _background_attachment_,
            _background_image_,
            _background_image_baseurl_,
            _background_size_,
            _background_origin_,
            _background_clip_,
        ],
        _list_style_ => &[_list_style_image_, _list_style_image_baseurl_, _list_style_position_, _list_style_type_],
        _margin_ => &[_margin_top_, _margin_right_, _margin_bottom_, _margin_left_],
        _padding_ => &[_padding_top_, _padding_right_, _padding_bottom_, _padding_left_],
        _border_width_ => &[_border_top_width_, _border_right_width_, _border_bottom_width_, _border_left_width_],
        _border_style_ => &[_border_top_style_, _border_right_style_, _border_bottom_style_, _border_left_style_],
        _border_color_ => &[_border_top_color_, _border_right_color_, _border_bottom_color_, _border_left_color_],
        _border_top_ => &[_border_top_width_, _border_top_style_, _border_top_color_],
        _border_right_ => &[_border_right_width_, _border_right_style_, _border_right_color_],
        _border_bottom_ => &[_border_bottom_width_, _border_bottom_style_, _border_bottom_color_],
        _border_left_ => &[_border_left_width_, _border_left_style_, _border_left_color_],
        _border_ => &[
            _border_top_width_,
            _border_right_width_,
            _border_bottom_width_,
            _border_left_width_,
            _border_top_style_,
            _border_right_style_,
            _border_bottom_style_,
            _border_left_style_,
            _border_top_color_,
            _border_right_color_,
            _border_bottom_color_,
            _border_left_color_,
        ],
        _flex_ => &[_flex_grow_, _flex_shrink_, _flex_basis_],
        _flex_flow_ => &[_flex_direction_, _flex_wrap_],
        _text_decoration_ => &[_text_decoration_color_, _text_decoration_line_, _text_decoration_style_, _text_decoration_thickness_],
        _text_emphasis_ => &[_text_emphasis_style_, _text_emphasis_color_],
        _ => &[],
    }
}

// ---- The raw declarations the renderer reads ---------------------------------------------------------

/// `raw_declaration_names`: the properties whose declarations the renderer reads as written (animations,
/// transitions, what it interpolates or hands to the platform), each kept again as `-lh-raw-<name>`.
pub const RAW_DECLARATION_NAMES: [&str; 48] = [
    "animation",
    "animation-name",
    "animation-duration",
    "animation-timing-function",
    "animation-delay",
    "animation-iteration-count",
    "animation-direction",
    "animation-fill-mode",
    "animation-play-state",
    "transition",
    "transition-property",
    "transition-duration",
    "transition-timing-function",
    "transition-delay",
    "transform",
    "opacity",
    "color",
    "background-color",
    "background",
    "width",
    "height",
    "top",
    "left",
    "right",
    "bottom",
    "box-shadow",
    "border-color",
    "max-height",
    "stroke-dashoffset",
    "fill",
    "stroke",
    "translate",
    "scale",
    "rotate",
    "filter",
    "letter-spacing",
    "font-size",
    "margin-top",
    "margin-left",
    "flex-grow",
    "transform-origin",
    "backdrop-filter",
    "pointer-events",
    "touch-action",
    "quotes",
    "object-fit",
    "object-position",
    "text-overflow",
];

/// `raw_declaration_ids()` and the set `is_raw_declaration` reads: each raw-declared name's id with its
/// `-lh-raw-` twin's.
#[derive(Debug, Default)]
pub struct RawDeclarationIds {
    by_name: BTreeMap<StringId, StringId>,
    raws: BTreeSet<StringId>,
}

/// Interns the raw-declared names in the order the C++ static does: `map[_id(name)] = _id("-lh-raw-" +
/// name)`, whose right side C++17 evaluates first, so each twin is interned before its name.
pub(crate) fn intern_raw_declaration_ids(names: &Names) -> RawDeclarationIds {
    let mut by_name = BTreeMap::new();
    for name in RAW_DECLARATION_NAMES {
        let raw = names.id(&[b"-lh-raw-", name.as_bytes()].concat());
        by_name.insert(names.id(name.as_bytes()), raw);
    }
    let raws = by_name.values().copied().collect();
    RawDeclarationIds { by_name, raws }
}

/// `raw_declaration_id(name)`: the `-lh-raw-` twin of a raw-declared name, `empty_id` for any other.
pub fn raw_declaration_id(names: &Names, name: StringId) -> StringId {
    names.raw_declaration_ids().by_name.get(&name).copied().unwrap_or(empty_id)
}

/// `is_raw_declaration(name)`: whether the name is a `-lh-raw-` twin.
pub fn is_raw_declaration(names: &Names, name: StringId) -> bool {
    names.raw_declaration_ids().raws.contains(&name)
}

// ---- The declaration cache ---------------------------------------------------------------------------

/// `style_op`: one effect of a declaration on a style, as `style::add(text)` replays it.
#[derive(Clone, Debug, PartialEq)]
pub struct StyleOp {
    pub remove: bool,
    pub important: bool,
    pub name: StringId,
    pub value: PropertyValue,
}

/// The cache lives as long as the process: long declarations stay out, and so does more than 1 MB of them.
const max_cached_declaration: usize = 512;
const max_cached_bytes: usize = 1 << 20;
const max_cached_declarations: usize = 50000;

#[derive(Debug, Default)]
struct CacheInner {
    map: HashMap<Vec<u8>, Vec<StyleOp>>,
    bytes: usize,
}

/// `declaration_cache`: what each declaration a `style::add(text)` met did, by its text and whether a
/// container read it; emptied whole when it holds more than 50000 or past 1 MB of keys. One per process in
/// the C++.
#[derive(Debug, Default)]
pub struct DeclarationCache {
    inner: Mutex<CacheInner>,
}

impl DeclarationCache {
    fn lock(&self) -> std::sync::MutexGuard<'_, CacheInner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn get(&self, key: &[u8]) -> Option<Vec<StyleOp>> {
        self.lock().map.get(key).cloned()
    }

    fn put(&self, key: Vec<u8>, ops: &[StyleOp]) {
        let mut cache = self.lock();
        if cache.map.len() > max_cached_declarations || cache.bytes > max_cached_bytes {
            cache.map.clear();
            cache.bytes = 0;
        }
        let size = key.len();
        if let std::collections::hash_map::Entry::Vacant(slot) = cache.map.entry(key) {
            slot.insert(ops.to_vec());
            cache.bytes += size;
        }
    }

    /// How many declarations it holds.
    pub fn len(&self) -> usize {
        self.lock().map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `isspace` in the C locale.
fn is_space(c: u8) -> bool {
    c == b' ' || (0x09..=0x0d).contains(&c)
}

/// `is_animation_layer(txt, start)`: the declaration at `start` is `-lh-anim`'s.
fn is_animation_layer(txt: &[u8], start: usize) -> bool {
    if txt.get(start..start + 8) != Some(&b"-lh-anim"[..]) {
        return false;
    }
    let next = txt.get(start + 8).copied().unwrap_or(b':');
    next == b':' || is_space(next)
}

/// `split_declarations(txt, out)`: the declarations of a style text between `;`s outside quotes and
/// brackets, trimmed, as (start, length); false for text with a comment, a backslash or a brace, or
/// unbalanced.
fn split_declarations(txt: &[u8], out: &mut Vec<(usize, usize)>) -> bool {
    if txt.windows(2).any(|w| w == b"/*") || txt.iter().any(|c| matches!(c, b'\\' | b'{' | b'}')) {
        return false;
    }
    let mut depth = 0i32;
    let mut quote = 0u8;
    let mut start = 0;
    for i in 0..=txt.len() {
        let c = if i < txt.len() { txt[i] } else { b';' };
        if quote != 0 {
            if c == quote {
                quote = 0;
            }
            continue;
        }
        if c == b'"' || c == b'\'' {
            quote = c;
        } else if c == b'(' || c == b'[' {
            depth += 1;
        } else if c == b')' || c == b']' {
            depth -= 1;
        } else if c == b';' && depth == 0 {
            let mut a = start;
            let mut b = i;
            while a < b && is_space(txt[a]) {
                a += 1;
            }
            while b > a && is_space(txt[b - 1]) {
                b -= 1;
            }
            if b > a {
                out.push((a, b - a));
            }
            start = i + 1;
        }
        if depth < 0 {
            return false;
        }
    }
    quote == 0 && depth == 0
}

/// `has_var(tokens)`: a `var()` at any depth.
pub fn has_var(tokens: &[CssToken]) -> bool {
    for tok in tokens {
        if tok.ty == CV_FUNCTION && tok.name().eq_ignore_ascii_case(b"var") {
            return true;
        }
        if tok.is_component_value() && has_var(&tok.value) {
            return true;
        }
    }
    false
}

fn is_custom(names: &Names, name: StringId) -> bool {
    names.s(name).starts_with(b"--")
}

// ---- Fonts --------------------------------------------------------------------------------------------

/// `join_font_features(features)`: `tag=value` by tag, between commas.
fn join_font_features(features: &BTreeMap<Vec<u8>, i32>) -> Vec<u8> {
    let mut out = Vec::new();
    for (tag, value) in features {
        if !out.is_empty() {
            out.push(b',');
        }
        out.extend_from_slice(tag);
        out.push(b'=');
        out.extend(cxx::to_string_int(*value));
    }
    out
}

/// `parse_font_variant_numeric(tokens, out)`: `normal` (no features), or the OpenType features of numeric
/// keywords, each once.
fn parse_font_variant_numeric(tokens: &[CssToken]) -> Option<Vec<u8>> {
    if tokens.len() == 1 && tokens[0].ident() == b"normal" {
        return Some(Vec::new());
    }
    let mut features = BTreeMap::new();
    for token in tokens {
        let tag: &[u8] = match &token.ident()[..] {
            b"lining-nums" => b"lnum",
            b"oldstyle-nums" => b"onum",
            b"proportional-nums" => b"pnum",
            b"tabular-nums" => b"tnum",
            b"diagonal-fractions" => b"frac",
            b"stacked-fractions" => b"afrc",
            b"ordinal" => b"ordn",
            b"slashed-zero" => b"zero",
            _ => return None,
        };
        if features.insert(tag.to_vec(), 1).is_some() {
            return None;
        }
    }
    let out = join_font_features(&features);
    (!out.is_empty()).then_some(out)
}

/// `parse_font_feature_settings(tokens, out)`: `normal`, or `"tag" [<integer> | on | off]?` between commas,
/// a later tag's value over an earlier one's.
fn parse_font_feature_settings(tokens: &[CssToken]) -> Option<Vec<u8>> {
    if tokens.len() == 1 && tokens[0].ident() == b"normal" {
        return Some(Vec::new());
    }
    let mut features = BTreeMap::new();
    for item in parse_comma_separated_list(tokens) {
        if item.is_empty()
            || item.len() > 2
            || item[0].ty != STRING
            || item[0].str.len() != 4
            || !item[0].str.iter().all(|c| (0x20..=0x7e).contains(c))
        {
            return None;
        }
        let mut value = 1;
        if item.len() == 2 {
            if item[1].ty == NUMBER && item[1].n.number_type == CssNumberType::css_number_integer && item[1].n.number >= 0.0 {
                value = cxx::f32_to_int(cxx::min_f32(item[1].n.number, 65535.0));
            } else if item[1].ident() == b"on" || item[1].ident() == b"off" {
                value = i32::from(item[1].ident() == b"on");
            } else {
                return None;
            }
        }
        features.insert(item[0].str.clone(), value);
    }
    let out = join_font_features(&features);
    (!out.is_empty()).then_some(out)
}

/// `parse_font_weight(tok, weight)`: a keyword's index, or a number from 1 to 1000 (fractions allowed).
fn parse_font_weight(tok: &CssToken, weight: &mut CssLength) -> bool {
    if let Some(idx) = CssValues::new(&font_weight_strings).value_index(&tok.ident()) {
        weight.set_predef(idx);
        return true;
    }

    // https://drafts.csswg.org/css-fonts/#font-weight-absolute-values
    if tok.ty == NUMBER && tok.n.number >= 1.0 && tok.n.number <= 1000.0 {
        weight.set_value(tok.n.number, CssUnits::css_units_none);
        return true;
    }

    false
}

/// `parse_font_style_variant_weight(tokens, index, style, variant, weight)`: up to three of `normal`, a
/// style, a variant and a weight, each once.
fn parse_font_style_variant_weight(
    tokens: &[CssToken],
    index: &mut i32,
    style: &mut i32,
    variant: &mut i32,
    weight: &mut CssLength,
) -> bool {
    let mut style_found = false;
    let mut variant_found = false;
    let mut weight_found = false;
    let mut res = false;

    let mut i = *index;
    let mut count = 0;
    while i < tokens.len() as i32 {
        count += 1;
        if count > 3 {
            break;
        }
        let tok = &tokens[i as usize];
        i += 1;
        // "normal" is every one's initial value: it changes nothing.
        if tok.ident() == b"normal" {
            *index += 1;
            res = true;
        } else if !style_found && let Some(v) = parse_keyword(tok, CssValues::new(&font_style_strings), 0) {
            *style = v;
            style_found = true;
            *index += 1;
            res = true;
        } else if !variant_found && let Some(v) = parse_keyword(tok, CssValues::new(&font_variant_strings), 0) {
            *variant = v;
            variant_found = true;
            *index += 1;
            res = true;
        } else if !weight_found && parse_font_weight(tok, weight) {
            weight_found = true;
            *index += 1;
            res = true;
        } else {
            break;
        }
    }
    res
}

/// `is_custom_ident(tok)` (https://www.w3.org/TR/css-values-4/#custom-idents): an identifier but a CSS-wide
/// keyword or `default`.
fn is_custom_ident(tok: &CssToken) -> bool {
    if tok.ty != IDENT {
        return false;
    }
    !matches!(&lowcase(tok.name())[..], b"default" | b"initial" | b"inherit" | b"unset")
}

/// `parse_font_family(tokens, font_family)`: strings and runs of custom identifiers between commas, joined
/// by commas (a run's identifiers by spaces).
fn parse_font_family(tokens: &[CssToken]) -> Option<Vec<u8>> {
    let list = parse_comma_separated_list(tokens);
    if list.is_empty() {
        return None;
    }

    let mut result = Vec::new();
    for name in &list {
        if name.len() == 1 && name[0].ty == STRING {
            result.extend_from_slice(&name[0].str);
            result.push(b',');
            continue;
        }

        // Otherwise: name must be a list of <custom-ident>s
        let mut str = Vec::new();
        for tok in name {
            if !is_custom_ident(tok) {
                return None;
            }
            str.extend_from_slice(tok.name());
            str.push(b' ');
        }
        result.extend(trim(&str, WHITESPACE));
        result.push(b',');
    }
    // remove last ','
    result.pop();
    Some(result)
}

// ---- Borders, lengths, backgrounds -----------------------------------------------------------------------

/// `parse_border_width(token, w)`: a length or `thin`, `medium`, `thick` as their px.
fn parse_border_width(token: &CssToken) -> Option<CssLength> {
    let mut width = CssLength::default();
    if !width.from_token(token, f_length | f_positive, CssValues::new(&border_width_strings)) {
        return None;
    }

    if width.is_predefined() {
        width.set_value(border_width_values[width.predef() as usize], CssUnits::css_units_px);
    }

    Some(width)
}

/// `parse_border_helper(tokens, container, width, style, color)`: `<line-width> || <line-style> || <color>`,
/// medium, none and currentcolor for what is not given.
fn parse_border_helper(
    names: &Names,
    tokens: &[CssToken],
    container: Option<&dyn DocumentContainer>,
) -> Option<(CssLength, i32, WebColor)> {
    let mut width = CssLength::new(border_width_medium_value, CssUnits::css_units_px);
    let mut style = BorderStyle::border_style_none.index();
    let mut color = WebColor::CURRENT_COLOR;

    let mut width_found = false;
    let mut style_found = false;
    let mut color_found = false;

    for token in tokens {
        if !width_found && let Some(w) = parse_border_width(token) {
            width = w;
            width_found = true;
        } else if !style_found && let Some(s) = parse_keyword(token, CssValues::new(&border_style_strings), 0) {
            style = s;
            style_found = true;
        } else if !color_found && let Some(c) = parse_color(names, token, resolver(container)) {
            color = c;
            color_found = true;
        } else {
            return None;
        }
    }

    Some((width, style, color))
}

/// `parse_two_lengths(tokens, len, options)`: one or two lengths; one stands for both.
fn parse_two_lengths(tokens: &[CssToken], options: i32) -> Option<[CssLength; 2]> {
    let n = tokens.len();
    if n != 1 && n != 2 {
        return None;
    }

    let mut a = CssLength::default();
    let mut b = CssLength::default();
    if !a.from_token(&tokens[0], options, CssValues::default()) {
        return None;
    }
    if n == 1 {
        b = a.clone();
    }

    if n == 2 && !b.from_token(&tokens[1], options, CssValues::default()) {
        return None;
    }

    Some([a, b])
}

/// `parse_1234_values(tokens, result, parse, args…)`: one to four values, each parsed into a fresh slot;
/// none when there are more or one fails.
fn parse_1234_values<T>(tokens: &[CssToken], mut parse: impl FnMut(&CssToken) -> Option<T>) -> Vec<T> {
    if tokens.len() > 4 {
        return Vec::new();
    }
    let mut result = Vec::with_capacity(tokens.len());
    for tok in tokens {
        match parse(tok) {
            Some(v) => result.push(v),
            None => return Vec::new(),
        }
    }
    result
}

/// `parse_1234_lengths(tokens, len, options, keywords)`.
fn parse_1234_lengths(tokens: &[CssToken], options: i32, keywords: CssValues) -> Vec<CssLength> {
    parse_1234_values(tokens, |tok| {
        let mut len = CssLength::default();
        len.from_token(tok, options, keywords).then_some(len)
    })
}

/// `is_one_of_predef(x, idx1, idx2)`.
fn is_one_of_predef(x: &CssLength, idx1: BackgroundPosition, idx2: BackgroundPosition) -> bool {
    x.is_predefined() && (x.predef() == idx1.index() || x.predef() == idx2.index())
}

/// `parse_bg_position(tokens, index, x, y, convert_keywords_to_percents)`
/// (https://drafts.csswg.org/css-backgrounds/#typedef-bg-position): one or two of `left | center | right |
/// top | bottom | <length-percentage>`, a pair of keywords in either order; side-relative values are not
/// read. A single value has `center` for the other; keywords become their percentages when asked.
pub fn parse_bg_position(
    tokens: &[CssToken],
    index: &mut i32,
    x: &mut CssLength,
    y: &mut CssLength,
    convert_keywords_to_percents: bool,
) -> bool {
    use BackgroundPosition::*;
    let keywords = CssValues::new(&background_position_strings);

    let mut a = CssLength::default();
    let mut b = CssLength::default();

    if !a.from_token(at(tokens, *index), f_length_percentage, keywords) {
        return false;
    }

    if !b.from_token(at(tokens, *index + 1), f_length_percentage, keywords) {
        // If only one value is specified, the second value is assumed to be center.
        b.set_predef(background_position_center.index());

        // fix wrong order
        if is_one_of_predef(&a, background_position_top, background_position_bottom) {
            std::mem::swap(&mut a, &mut b);
        }

        *index += 1;
    } else {
        // A pair of keywords can be reordered, while a combination of keyword and length or percentage cannot.
        if (is_one_of_predef(&a, background_position_top, background_position_bottom) && b.is_predefined())
            || (a.is_predefined() && is_one_of_predef(&b, background_position_left, background_position_right))
        {
            std::mem::swap(&mut a, &mut b);
        }

        // check for wrong order
        if is_one_of_predef(&a, background_position_top, background_position_bottom)
            || is_one_of_predef(&b, background_position_left, background_position_right)
        {
            return false;
        }

        *index += 2;
    }

    if convert_keywords_to_percents {
        if a.is_predefined() {
            a.set_value(background_position_percentages[a.predef() as usize], CssUnits::css_units_percentage);
        }
        if b.is_predefined() {
            b.set_value(background_position_percentages[b.predef() as usize], CssUnits::css_units_percentage);
        }
    }

    *x = a;
    *y = b;
    true
}

/// `parse_bg_size(tokens, index, size)` (https://drafts.csswg.org/css-backgrounds/#typedef-bg-size):
/// `[ <length-percentage [0,∞]> | auto ]{1,2} | cover | contain`.
fn parse_bg_size(tokens: &[CssToken], index: &mut i32, size: &mut CssSize) -> bool {
    let mut a = CssLength::default();
    let mut b = CssLength::default();

    if !a.from_token(at(tokens, *index), f_length_percentage | f_positive, CssValues::new(&background_size_strings)) {
        return false;
    }

    // cover | contain
    if a.is_predefined() && a.predef() != BackgroundSize::background_size_auto.index() {
        size.width = a.clone();
        size.height = a;
        *index += 1;
        return true;
    }

    if b.from_token(at(tokens, *index + 1), f_length_percentage | f_positive, CssValues::new(&AUTO)) {
        *index += 2;
    } else {
        // If only one value is given the second is assumed to be auto.
        b.set_predef(BackgroundSize::background_size_auto.index());
        *index += 1;
    }

    size.width = a;
    size.height = b;
    true
}

/// `parse_bg_position_size(tokens, index, x, y, size)`: `<bg-position> [ / <bg-size> ]?`. A size that does
/// not parse leaves the position written and the index at the `/`.
fn parse_bg_position_size(tokens: &[CssToken], index: &mut i32, x: &mut CssLength, y: &mut CssLength, size: &mut CssSize) -> bool {
    if !parse_bg_position(tokens, index, x, y, true) {
        return false;
    }

    if at(tokens, *index).ch() != i32::from(b'/') {
        // no [ / <bg-size> ]
        return true;
    }

    *index += 1;
    if !parse_bg_size(tokens, index, size) {
        // restore index to point to '/'
        *index -= 1;
        return false;
    }

    true
}

/// `parse_bg_image(token, bg_image, container)`: `none`, a url or a gradient, into the image as it is.
fn parse_bg_image(names: &Names, tok: &CssToken, bg_image: &mut Image, container: Option<&dyn DocumentContainer>) -> bool {
    if tok.ident() == b"none" {
        bg_image.ty = ImageType::type_none;
        return true;
    }

    if let Some(url) = parse_url(tok) {
        bg_image.ty = ImageType::type_url;
        bg_image.url = url;
        return true;
    }

    if let Some(gradient) = parse_gradient(names, tok, container) {
        bg_image.m_gradient = gradient;
        bg_image.ty = ImageType::type_gradient;
        return true;
    }

    false
}

/// One layer of `background`, as `parse_bg_layer` fills a `background`'s first entries.
struct BgLayer {
    color: WebColor,
    image: Image,
    position_x: CssLength,
    position_y: CssLength,
    size: CssSize,
    repeat: i32,
    attachment: i32,
    origin: i32,
    clip: i32,
}

/// `parse_list_style_image(tok, url)`: `none` (empty) or a url.
fn parse_list_style_image(tok: &CssToken) -> Option<Vec<u8>> {
    if tok.ident() == b"none" {
        return Some(Vec::new());
    }
    parse_url(tok)
}

// ---- The element side -----------------------------------------------------------------------------------

/// What `style::subst_vars` asks past the style it substitutes (the element's parent chain), and what `subst_vars_`
/// asks of an element; the cascade's is `html_tag::ElementVars`.
pub trait VarElement {
    /// `get_custom_property(name, result, invalid)`: the tokens of the custom property where the element or an
    /// ancestor declares it; false when none does, with `invalid` set when the nearest declares the
    /// guaranteed-invalid value.
    fn get_custom_property(&self, name: StringId, result: &mut CssTokenVector, invalid: &mut bool) -> bool;

    /// `get_document()->container()`.
    fn container(&self) -> Option<&dyn DocumentContainer>;
}

/// The element whose style is being substituted, as `get_custom_property` reads it: the style itself (as it is
/// so far), then what `el` gives (the parent's chain).
struct OwnFirst<'a> {
    own: &'a BTreeMap<StringId, PropertyValue>,
    el: &'a dyn VarElement,
}

impl VarElement for OwnFirst<'_> {
    fn get_custom_property(&self, name: StringId, result: &mut CssTokenVector, invalid: &mut bool) -> bool {
        match self.own.get(&name).map(|p| &p.value) {
            Some(Value::Tokens(tokens)) => {
                *result = tokens.clone();
                true
            }
            Some(Value::CssWideKeyword(_)) => {
                // invalid at computed-value time
                *invalid = true;
                false
            }
            _ => self.el.get_custom_property(name, result, invalid),
        }
    }

    fn container(&self) -> Option<&dyn DocumentContainer> {
        self.el.container()
    }
}

/// A declaration may grow to this many tokens (nested ones included) through `var()`; past it the
/// declaration is invalid at computed-value time.
const max_var_substitution_tokens: usize = 16384;

/// `var_budget`.
#[derive(Clone, Copy, Debug)]
struct VarBudget {
    left: usize,
    invalid: bool,
    exhausted: bool,
}

impl Default for VarBudget {
    fn default() -> VarBudget {
        VarBudget { left: max_var_substitution_tokens, invalid: false, exhausted: false }
    }
}

/// `check_var_syntax(args)`: `var( <custom-property-name> , <declaration-value>? )`.
pub fn check_var_syntax(args: &[CssToken]) -> bool {
    if args.is_empty() {
        return false;
    }

    let name = args[0].ident();
    if !name.starts_with(b"--") || name.len() <= 2 {
        return false;
    }

    if args.len() > 1 && args[1].ch() != crate::css_tokenizer::COMMA {
        return false;
    }
    if args.len() > 2 && !is_declaration_value(args, 2) {
        return false;
    }

    true
}

/// `deep_token_count(tokens, limit)`: the tokens at every depth, counted until past `limit`.
fn deep_token_count(tokens: &[CssToken], limit: usize) -> usize {
    let mut n = tokens.len();
    for t in tokens {
        if n > limit {
            break;
        }
        if !t.value.is_empty() {
            n += deep_token_count(&t.value, limit - n);
        }
    }
    n
}

/// `resolve_vars(tokens, el, chain, depth, budget)`: every `var()` replaced by what it stands for, at any
/// depth to 32; false for a cycle, bad syntax, a missing property without fallback or a spent budget.
fn resolve_vars(
    names: &Names,
    tokens: &mut CssTokenVector,
    el: &dyn VarElement,
    chain: &mut BTreeSet<StringId>,
    depth: i32,
    budget: &mut VarBudget,
) -> bool {
    if depth > 32 {
        return false;
    }
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].ty == CV_FUNCTION && lowcase(tokens[i].name()) == b"var" {
            let mut args = tokens[i].value.clone();
            if !check_var_syntax(&args) {
                return false;
            }
            let name = names.id(args[0].name());
            if chain.contains(&name) {
                return false;
            }
            let mut value = Vec::new();
            let mut invalid = false;
            if !el.get_custom_property(name, &mut value, &mut invalid) {
                if args.len() == 1 {
                    budget.invalid = invalid;
                    return false;
                }
                args.drain(0..2);
                value = args;
            }
            let size = deep_token_count(&value, budget.left);
            if size > budget.left {
                budget.invalid = true;
                budget.exhausted = true;
                return false;
            }
            budget.left -= size;
            chain.insert(name);
            let ok = resolve_vars(names, &mut value, el, chain, depth + 1, budget);
            chain.remove(&name);
            if !ok {
                return false;
            }
            let len = value.len();
            tokens.splice(i..i + 1, value);
            i += len;
            continue;
        }
        if tokens[i].is_component_value() && !resolve_vars(names, &mut tokens[i].value, el, chain, depth + 1, budget) {
            return false;
        }
        i += 1;
    }
    true
}

/// `subst_vars_(name, tokens, el)`: the tokens with their `var()`s replaced; false when the substitution ran
/// over the budget or met a property that did.
pub fn subst_vars_(names: &Names, name: StringId, tokens: &mut CssTokenVector, el: &dyn VarElement) -> bool {
    let mut chain = BTreeSet::from([name]);
    let mut budget = VarBudget::default();
    resolve_vars(names, tokens, el, &mut chain, 0, &mut budget);
    !budget.invalid
}

// ---- The style ---------------------------------------------------------------------------------------------

/// `style`: the properties of a style block by name, each with its value, `!important` and order.
#[derive(Clone, Debug, Default)]
pub struct Style {
    m_properties: BTreeMap<StringId, PropertyValue>,
    m_counter: i32,
    /// `recorded_ops`: every add while the declaration cache watches this style.
    recording: Option<Vec<StyleOp>>,
    /// `suppress_raw`: past a `-lh-anim` declaration no raw twins are added.
    suppress_raw: bool,
}

impl Style {
    /// The properties in the C++ map's order (by id).
    pub fn properties(&self) -> &BTreeMap<StringId, PropertyValue> {
        &self.m_properties
    }

    /// `m_counter`: the last order given.
    pub fn counter(&self) -> i32 {
        self.m_counter
    }

    /// `get_property(name)`: the property, or an invalid value.
    pub fn get_property(&self, name: StringId) -> &PropertyValue {
        self.m_properties.get(&name).unwrap_or(&INVALID_PROPERTY)
    }

    pub fn empty(&self) -> bool {
        self.m_properties.is_empty()
    }

    /// `declares_custom_property()`.
    pub fn declares_custom_property(&self, names: &Names) -> bool {
        self.m_properties.keys().any(|&name| {
            let name = names.s(name);
            name.len() > 2 && name[0] == b'-' && name[1] == b'-'
        })
    }

    /// `only_declares(allowed)`.
    pub fn only_declares(&self, allowed: impl Fn(StringId) -> bool) -> bool {
        self.m_properties.keys().all(|&name| allowed(name))
    }

    /// `has_var_property()`.
    pub fn has_var_property(&self) -> bool {
        self.m_properties.values().any(|p| p.m_has_var)
    }

    pub fn clear(&mut self) {
        self.m_properties.clear();
    }

    /// `add(txt, baseurl, container)`: a style text. Without a base url and when it splits plainly, each
    /// declaration's effect comes from the cache or is recorded into it (not for one longer than 512 bytes or
    /// with a viewport length, which folds at the current viewport); past `-lh-anim` no raw declaration is
    /// replayed. Otherwise the text is added as tokens.
    pub fn add_text(
        &mut self,
        names: &Names,
        cache: &DeclarationCache,
        txt: &[u8],
        baseurl: &[u8],
        container: Option<&dyn DocumentContainer>,
    ) {
        let mut declarations = Vec::new();
        if !baseurl.is_empty() || !split_declarations(txt, &mut declarations) {
            let tokens = normalize(txt, f_componentize, None);
            self.add_tokens(names, tokens, baseurl, container);
            return;
        }
        let mut layer = false;
        for (start, len) in declarations {
            if is_animation_layer(txt, start) {
                layer = true;
            }
            let mut key = txt[start..start + len].to_vec();
            key.push(u8::from(container.is_some()));
            let ops = match cache.get(&key) {
                Some(ops) => ops,
                None => {
                    let mut scratch = Style { recording: Some(Vec::new()), ..Style::default() };
                    let tokens = normalize(&txt[start..start + len], f_componentize, None);
                    let cacheable = key.len() <= max_cached_declaration && !has_viewport_units(&tokens);
                    scratch.add_tokens(names, tokens, baseurl, container);
                    let ops = scratch.recording.take().unwrap_or_default();
                    // Viewport lengths fold to px at the current viewport, so their result holds for this parse only.
                    if cacheable {
                        cache.put(key, &ops);
                    }
                    ops
                }
            };
            for op in ops {
                if layer && is_raw_declaration(names, op.name) {
                    continue;
                }
                if op.remove {
                    self.remove_property(op.name, op.important);
                } else {
                    self.add_parsed_property(op.name, op.value);
                }
            }
        }
    }

    /// `add(tokens, baseurl, container)`: a style block's component values: its declarations (rules inside
    /// it ignored), each value without whitespace, each name lower-case unless custom and looked up through
    /// the name table (which keeps it), those the container sanitizes away skipped.
    pub fn add(&mut self, names: &Names, tokens: &[CssToken], baseurl: &[u8], container: Option<&dyn DocumentContainer>) {
        self.add_tokens(names, tokens.to_vec(), baseurl, container);
    }

    pub(crate) fn add_tokens(&mut self, names: &Names, tokens: Vec<CssToken>, baseurl: &[u8], container: Option<&dyn DocumentContainer>) {
        let mut decls = Vec::new();
        let mut rules = Vec::new();
        CssParser::new(tokens).consume_style_block_contents(&mut decls, &mut rules);
        // a rule inside a style block is a parse error
        if decls.is_empty() {
            return;
        }

        let saved = self.suppress_raw;
        for mut decl in decls {
            if decl.name == b"-lh-anim" {
                self.suppress_raw = true;
            }
            remove_whitespace(&mut decl.value, None);
            // decl.value is already componentized, and may be empty.
            if !decl.name.starts_with(b"--") {
                decl.name.make_ascii_lowercase();
            }
            if let Some(container) = container
                && !container.sanitize_declaration(&decl.name, &decl.value)
            {
                continue;
            }
            let id = names.id(&decl.name);
            self.add_property_owned(names, id, decl.value, baseurl, decl.important, container);
        }
        self.suppress_raw = saved;
    }

    /// `add_property(name, val, baseurl, important, container)`: a value as text, componentized without
    /// whitespace.
    pub fn add_property_text(
        &mut self,
        names: &Names,
        name: StringId,
        value: &[u8],
        baseurl: &[u8],
        important: bool,
        container: Option<&dyn DocumentContainer>,
    ) {
        let tokens = normalize(value, f_componentize | f_remove_whitespace, None);
        self.add_property_owned(names, name, tokens, baseurl, important, container);
    }

    /// `inherit_property(name, important)`.
    fn inherit_property(&mut self, name: StringId, important: bool) {
        self.keyword_property(name, PropertyValue::new(Value::Inherit, important));
    }

    /// `keyword_property(name, value)`: the value for each longhand of a shorthand, or for the property.
    fn keyword_property(&mut self, name: StringId, value: PropertyValue) {
        let atomic_properties = shorthand(name);
        if !atomic_properties.is_empty() {
            for &atomic in atomic_properties {
                self.add_parsed_property(atomic, value.clone());
            }
        } else {
            self.add_parsed_property(name, value);
        }
    }

    fn add_value(&mut self, name: StringId, value: impl Into<Value>, important: bool) {
        self.add_parsed_property(name, PropertyValue::new(value.into(), important));
    }

    /// `add_length_property(name, val, keywords, options, important)`.
    fn add_length_property(&mut self, name: StringId, val: &CssToken, keywords: CssValues, options: i32, important: bool) {
        let mut length = CssLength::default();
        if length.from_token(val, options, keywords) {
            self.add_value(name, length, important);
        }
    }

    /// `add_four_properties(top_name, val, n, important)`: top, right, bottom and left (or the corners from
    /// top-left), which follow one another in the name table, from one to four values.
    fn add_four_properties<T: Clone + Into<Value>>(&mut self, top_name: StringId, val: &[T], important: bool) {
        let n = val.len();
        let top = top_name;
        let right = StringId(top_name.0 + 1);
        let bottom = StringId(top_name.0 + 2);
        let left = StringId(top_name.0 + 3);

        // n      4 3 2 1
        // top    0 0 0 0  0
        // right  1 1 1 0  n>1
        // bottom 2 2 0 0  n/3*2
        // left   3 1 1 0  n/2+n/4
        self.add_value(top, val[0].clone(), important);
        self.add_value(right, val[usize::from(n > 1)].clone(), important);
        self.add_value(bottom, val[n / 3 * 2].clone(), important);
        self.add_value(left, val[n / 2 + n / 4].clone(), important);
    }

    /// `add_property(name, value, baseurl, important, container)`: one declaration, its value component
    /// values without whitespace.
    pub fn add_property(
        &mut self,
        names: &Names,
        name: StringId,
        value: &[CssToken],
        baseurl: &[u8],
        important: bool,
        container: Option<&dyn DocumentContainer>,
    ) {
        self.add_property_tokens(names, name, Cow::Borrowed(value), baseurl, important, container);
    }

    /// `add_property` of tokens the caller has no more use for: a property that keeps the tokens as they are takes
    /// them instead of a copy.
    pub fn add_property_owned(
        &mut self,
        names: &Names,
        name: StringId,
        value: Vec<CssToken>,
        baseurl: &[u8],
        important: bool,
        container: Option<&dyn DocumentContainer>,
    ) {
        self.add_property_tokens(names, name, Cow::Owned(value), baseurl, important, container);
    }

    fn add_property_tokens(
        &mut self,
        names: &Names,
        mut name: StringId,
        tokens: Cow<'_, [CssToken]>,
        baseurl: &[u8],
        important: bool,
        container: Option<&dyn DocumentContainer>,
    ) {
        let value: &[CssToken] = &tokens;
        if is_raw_declaration(names, name) {
            self.add_parsed_property(name, PropertyValue::with_var(Value::Tokens(value.to_vec()), important, has_var(value)));
            return;
        }
        if name == __webkit_backdrop_filter_ {
            name = _backdrop_filter_;
        }
        let raw = raw_declaration_id(names, name);
        if raw != empty_id && !value.is_empty() && !self.suppress_raw {
            self.add_parsed_property(raw, PropertyValue::with_var(Value::Tokens(value.to_vec()), important, has_var(value)));
        }

        let custom = is_custom(names, name);

        // Note: empty value is a valid value for a custom property.
        if value.is_empty() && !custom {
            return;
        }

        if has_var(value) {
            self.add_parsed_property(name, PropertyValue::with_var(Value::Tokens(value.to_vec()), important, true));
            return;
        }

        if !custom && has_foldable_math(value) {
            let mut folded = value.to_vec();
            if fold_math(names, &mut folded, container, 0) {
                self.add_property_owned(names, name, folded, baseurl, important, container);
                return;
            }
        }

        // valid only if value contains a single token
        let default_token = CssToken::default();
        let val = if value.len() == 1 { &value[0] } else { &default_token };
        // nonempty if value is a single identifier
        let ident = val.ident();

        if name == _word_wrap_ {
            name = _overflow_wrap_;
        }
        if name == __webkit_user_select_ {
            name = _user_select_;
        }
        if ident == b"inherit" {
            self.inherit_property(name, important);
            return;
        }
        if !custom && matches!(&ident[..], b"initial" | b"unset" | b"revert") {
            let keyword = match &ident[..] {
                b"initial" => CssWideKeyword::initial,
                b"unset" => CssWideKeyword::unset,
                _ => CssWideKeyword::revert,
            };
            self.keyword_property(name, PropertyValue::new(Value::CssWideKeyword(keyword), important));
            return;
        }

        let none = CssValues::default();
        let colors = resolver(container);

        match name {
            //  =============================  SINGLE KEYWORD  =============================
            _overflow_ => {
                let first = if !value.is_empty() { value[0].ident() } else { Vec::new() };
                let second = if value.len() == 2 { value[1].ident() } else { first.clone() };
                let x = valid_values(_overflow_).value_index(&first);
                let y = valid_values(_overflow_).value_index(&second);
                if value.len() <= 2
                    && let (Some(x), Some(y)) = (x, y)
                {
                    self.add_value(_overflow_, x, important);
                    self.add_value(_overflow_x_, x, important);
                    self.add_value(_overflow_y_, y, important);
                }
            }

            _display_
            | _visibility_
            | _position_
            | _float_
            | _clear_
            | _appearance_
            | _resize_
            | _box_sizing_
            | _overflow_x_
            | _overflow_y_
            | _text_align_
            | _direction_
            | _vertical_align_
            | _text_transform_
            | __webkit_font_smoothing_
            | _white_space_
            | _overflow_wrap_
            | _word_break_
            | _user_select_
            | _font_style_
            | _font_variant_
            | _text_decoration_style_
            | _list_style_type_
            | _list_style_position_
            | _border_top_style_
            | _border_bottom_style_
            | _border_left_style_
            | _border_right_style_
            | _border_collapse_
            | _table_layout_
            | _outline_style_
            | _flex_direction_
            | _flex_wrap_
            | _justify_content_
            | _align_content_
            | _caption_side_ => {
                if let Some(index) = valid_values(name).value_index(&ident) {
                    self.add_value(name, index, important);
                }
            }

            //  =============================  LENGTH  =============================

            // auto | <integer>
            _z_index_ => self.add_length_property(name, val, CssValues::new(&AUTO), f_integer, important),

            // <length-percentage>
            _text_indent_ => self.add_length_property(name, val, none, f_length_percentage, important),

            // <length-percentage [0,∞]>
            _padding_left_ | _padding_right_ | _padding_top_ | _padding_bottom_ => {
                self.add_length_property(name, val, none, f_length_percentage | f_positive, important)
            }

            // auto | <length-percentage>
            _left_ | _right_ | _top_ | _bottom_ | _margin_left_ | _margin_right_ | _margin_top_ | _margin_bottom_ => {
                self.add_length_property(name, val, CssValues::new(&AUTO), f_length_percentage, important)
            }

            // auto | min-content | max-content | fit-content | <length-percentage [0,∞]>
            // Predefined 1 to 3 size a block to its content (render_block); anything else takes them as auto.
            _width_ => self.add_length_property(name, val, CssValues::new(&WIDTH_KEYWORDS), f_length_percentage | f_positive, important),
            _height_ | _min_width_ | _min_height_ => {
                self.add_length_property(name, val, CssValues::new(&AUTO), f_length_percentage | f_positive, important)
            }

            // none | <length-percentage [0,∞]>
            _max_width_ | _max_height_ => {
                self.add_length_property(name, val, CssValues::new(&NONE), f_length_percentage | f_positive, important)
            }

            //  normal | <number [0,∞]> | <length-percentage [0,∞]>
            _line_height_ => {
                self.add_length_property(name, val, CssValues::new(&NORMAL), f_number | f_length_percentage | f_positive, important)
            }

            // <absolute-size> | <relative-size> | <length-percentage [0,∞]>
            _font_size_ => {
                self.add_length_property(name, val, CssValues::new(&font_size_strings), f_length_percentage | f_positive, important)
            }

            _margin_ => {
                let len = parse_1234_lengths(value, f_length_percentage, CssValues::new(&AUTO));
                if !len.is_empty() {
                    self.add_four_properties(_margin_top_, &len, important);
                }
            }

            _padding_ => {
                let len = parse_1234_lengths(value, f_length_percentage | f_positive, none);
                if !len.is_empty() {
                    self.add_four_properties(_padding_top_, &len, important);
                }
            }

            _inset_ => {
                let len = parse_1234_lengths(value, f_length_percentage, CssValues::new(&AUTO));
                if !len.is_empty() {
                    self.add_four_properties(_top_, &len, important);
                }
            }

            //  =============================  COLOR  =============================
            _color_ if ident == b"currentcolor" => self.inherit_property(name, important),

            _color_ | _background_color_ | _border_top_color_ | _border_bottom_color_ | _border_left_color_ | _border_right_color_ => {
                if let Some(clr) = parse_color(names, val, colors) {
                    self.add_value(name, clr, important);
                }
            }

            //  =============================  BACKGROUND  =============================
            _background_ => self.parse_background(names, value, baseurl, important, container),

            _background_image_ => self.parse_background_image(names, value, baseurl, important, container),

            _background_position_ => self.parse_background_position(value, important),

            _background_size_ => self.parse_background_size(value, important),

            _background_repeat_ | _background_attachment_ | _background_origin_ | _background_clip_ => {
                self.parse_keyword_comma_list(name, value, important)
            }

            //  =============================  BORDER  =============================
            _border_ => self.parse_border(names, value, important, container),

            _border_left_ | _border_right_ | _border_top_ | _border_bottom_ => {
                self.parse_border_side(names, name, value, important, container)
            }

            _border_width_ => {
                let len = parse_1234_values(value, parse_border_width);
                if !len.is_empty() {
                    self.add_four_properties(_border_top_width_, &len, important);
                }
            }
            _border_style_ => {
                let idx = parse_1234_values(value, |tok| parse_keyword(tok, CssValues::new(&border_style_strings), 0));
                if !idx.is_empty() {
                    self.add_four_properties(_border_top_style_, &idx, important);
                }
            }
            _border_color_ => {
                let clr = parse_1234_values(value, |tok| parse_color(names, tok, colors));
                if !clr.is_empty() {
                    self.add_four_properties(_border_top_color_, &clr, important);
                }
            }

            _border_top_width_ | _border_bottom_width_ | _border_left_width_ | _border_right_width_ => {
                if let Some(len) = parse_border_width(val) {
                    self.add_value(name, len, important);
                }
            }

            // <length-percentage [0,∞]>{1,2}
            _border_bottom_left_radius_ | _border_bottom_right_radius_ | _border_top_right_radius_ | _border_top_left_radius_ => {
                if let Some([x, y]) = parse_two_lengths(value, f_length_percentage | f_positive) {
                    let base = names.s(name);
                    let x_name = names.id(&[&base[..], b"-x"].concat());
                    self.add_value(x_name, x, important);
                    let y_name = names.id(&[&base[..], b"-y"].concat());
                    self.add_value(y_name, y, important);
                }
            }

            _border_radius_x_ | _border_radius_y_ => {
                let top_left = if name == _border_radius_x_ { _border_top_left_radius_x_ } else { _border_top_left_radius_y_ };
                let len = parse_1234_lengths(value, f_length_percentage | f_positive, none);
                if !len.is_empty() {
                    self.add_four_properties(top_left, &len, important);
                }
            }

            _border_radius_ => self.parse_border_radius(value, important),

            // <length>{1,2}, not negative
            _border_spacing_ => {
                if let Some([x, y]) = parse_two_lengths(value, f_length | f_positive) {
                    self.add_value(__litehtml_border_spacing_x_, x, important);
                    self.add_value(__litehtml_border_spacing_y_, y, important);
                }
            }

            //  =============================  LIST  =============================
            _list_style_image_ => {
                if let Some(url) = parse_list_style_image(val) {
                    self.add_value(_list_style_image_, Value::String(url), important);
                    self.add_value(_list_style_image_baseurl_, Value::String(baseurl.to_vec()), important);
                }
            }

            _list_style_ => self.parse_list_style(value, baseurl, important),

            //  =============================  FONT  =============================
            _font_ => self.parse_font(value.to_vec(), important),

            _font_family_ => {
                if let Some(str) = parse_font_family(value) {
                    self.add_value(name, Value::String(str), important);
                }
            }

            _font_weight_ => {
                let mut len = CssLength::default();
                if parse_font_weight(val, &mut len) {
                    self.add_value(name, len, important);
                }
            }

            _text_decoration_ => self.parse_text_decoration(names, value, important, container),

            _text_decoration_thickness_ => self.add_length_property(
                name,
                val,
                CssValues::new(&style_text_decoration_thickness_strings),
                f_length_percentage | f_positive,
                important,
            ),

            _text_decoration_color_ => {
                self.parse_text_decoration_color(names, val, important, container);
            }

            _text_decoration_line_ => self.parse_text_decoration_line(value, important),

            _text_emphasis_ => self.parse_text_emphasis(names, value, important, container),

            _text_emphasis_style_ => {
                let str = get_repr(value, 0, -1, true);
                self.add_value(name, Value::String(str), important);
            }

            _font_variant_numeric_ => {
                if let Some(str) = parse_font_variant_numeric(value) {
                    self.add_value(name, Value::String(str), important);
                }
            }

            _font_feature_settings_ => {
                if let Some(str) = parse_font_feature_settings(value) {
                    self.add_value(name, Value::String(str), important);
                }
            }

            _text_emphasis_color_ => {
                self.parse_text_emphasis_color(names, val, important, container);
            }

            _text_emphasis_position_ => self.parse_text_emphasis_position(value, important),

            //  =============================  FLEX  =============================
            _flex_ => self.parse_flex(names, value, important),

            // <number [0,∞]>
            _flex_grow_ | _flex_shrink_ => {
                if val.ty == NUMBER && val.n.number >= 0.0 {
                    self.add_value(name, Value::Float(val.n.number), important);
                }
            }

            _flex_basis_ => {
                self.add_length_property(name, val, CssValues::new(&flex_basis_strings), f_length_percentage | f_positive, important)
            }

            _flex_flow_ => self.parse_flex_flow(value, important),

            _align_items_ | _align_self_ => self.parse_align_self(name, value, important),

            // <integer>
            _order_ => {
                if val.ty == NUMBER && val.n.number_type == CssNumberType::css_number_integer {
                    let number = f64::from(val.n.number);
                    // std::clamp<double>: the value unless it is below or above, so NaN stays NaN.
                    let clamped = if number < f64::from(i32::MIN) {
                        f64::from(i32::MIN)
                    } else if f64::from(i32::MAX) < number {
                        f64::from(i32::MAX)
                    } else {
                        number
                    };
                    self.add_value(name, cxx::f64_to_int(clamped), important);
                }
            }

            _gap_ | _grid_gap_ => {
                let normal = CssValues::new(&NORMAL);
                let mut len = [CssLength::default(), CssLength::default()];
                if (value.len() == 1 || value.len() == 2)
                    && len[0].from_token(&value[0], f_length_percentage | f_positive, normal)
                    && (value.len() == 1 || len[1].from_token(&value[1], f_length_percentage | f_positive, normal))
                {
                    self.add_value(_row_gap_, len[0].clone(), important);
                    let column = if value.len() == 2 { len[1].clone() } else { len[0].clone() };
                    self.add_value(_column_gap_, column, important);
                }
            }

            _row_gap_ | _column_gap_ | _grid_row_gap_ | _grid_column_gap_ => {
                let target = match name {
                    _grid_row_gap_ => _row_gap_,
                    _grid_column_gap_ => _column_gap_,
                    _ => name,
                };
                self.add_length_property(target, val, CssValues::new(&NORMAL), f_length_percentage | f_positive, important)
            }

            _justify_items_ | _justify_self_ => self.parse_align_self(name, value, important),

            _grid_ | _grid_template_ => self.parse_grid_shorthand(name, value, important),

            _place_items_ | _place_self_ => {
                if value.len() == 1 || value.len() == 2 {
                    let items = name == _place_items_;
                    self.parse_align_self(if items { _align_items_ } else { _align_self_ }, &[value[0].clone()], important);
                    self.parse_align_self(
                        if items { _justify_items_ } else { _justify_self_ },
                        &[value[value.len() - 1].clone()],
                        important,
                    );
                }
            }

            _place_content_ => {
                if value.len() == 1 || value.len() == 2 {
                    let mut align = value[0].ident();
                    let mut justify = value[value.len() - 1].ident();
                    if align == b"normal" {
                        align = b"stretch".to_vec();
                    }
                    if justify == b"normal" || justify == b"stretch" {
                        justify = b"flex-start".to_vec();
                    }
                    let a = valid_values(_align_content_).value_index(&align);
                    let j = valid_values(_justify_content_).value_index(&justify);
                    if let (Some(a), Some(j)) = (a, j) {
                        self.add_value(_align_content_, a, important);
                        self.add_value(_justify_content_, j, important);
                    }
                }
            }

            _aspect_ratio_ => {
                let mut numbers = Vec::new();
                for tok in value {
                    if tok.ty == NUMBER {
                        numbers.push(tok);
                    } else if tok.ident() != b"auto" && tok.ch() != i32::from(b'/') {
                        return;
                    }
                }
                if numbers.len() == 1 || numbers.len() == 2 {
                    let w = numbers[0].n.number;
                    let h = if numbers.len() == 2 { numbers[1].n.number } else { 1.0 };
                    if w > 0.0 && h > 0.0 {
                        self.add_value(name, Value::Float(w / h), important);
                    }
                }
            }

            _opacity_ => {
                if val.ty == NUMBER || val.ty == PERCENTAGE {
                    let opacity = if val.ty == PERCENTAGE { val.n.number / 100.0 } else { val.n.number };
                    self.add_value(name, Value::Float(cxx::max_f32(0.0, cxx::min_f32(1.0, opacity))), important);
                }
            }

            _letter_spacing_ | _word_spacing_ => self.add_length_property(name, val, CssValues::new(&NORMAL), f_length, important),

            _contain_ => {
                let mut flags = 0;
                let mut valid = !value.is_empty();
                for tok in value {
                    let word = tok.ident();
                    if tok.ty == crate::css_tokenizer::WHITESPACE {
                        continue;
                    }
                    match &word[..] {
                        b"none" if value.len() == 1 => flags = 0,
                        b"strict" => flags |= contain_size | contain_layout | contain_paint | contain_style,
                        b"content" => flags |= contain_layout | contain_paint | contain_style,
                        b"size" => flags |= contain_size,
                        b"inline-size" => flags |= contain_inline_size,
                        b"layout" => flags |= contain_layout,
                        b"paint" => flags |= contain_paint,
                        b"style" => flags |= contain_style,
                        _ => valid = false,
                    }
                }
                if valid {
                    self.add_value(name, flags, important);
                }
            }

            _backface_visibility_ => {
                if ident == b"hidden" || ident == b"visible" {
                    self.add_value(name, i32::from(ident == b"hidden"), important);
                }
            }

            _color_scheme_ => {
                let mut light = false;
                let mut dark = false;
                for tok in value {
                    let word = tok.ident();
                    light = light || word == b"light";
                    dark = dark || word == b"dark";
                }
                self.add_value(name, i32::from(dark && !light), important);
            }

            _accent_color_ => {
                if ident == b"auto" {
                    self.add_value(name, WebColor::TRANSPARENT, important);
                } else if let Some(clr) = parse_color(names, val, colors) {
                    self.add_value(name, clr, important);
                }
            }

            _outline_ => self.parse_border_side(names, name, value, important, container),

            _outline_width_ => {
                if let Some(len) = parse_border_width(val) {
                    self.add_value(name, len, important);
                }
            }

            _outline_color_ => {
                if let Some(clr) = parse_color(names, val, colors) {
                    self.add_value(name, clr, important);
                }
            }

            _outline_offset_ => self.add_length_property(name, val, none, f_length, important),

            _fill_
            | _fill_opacity_
            | _fill_rule_
            | _stroke_
            | _stroke_width_
            | _stroke_opacity_
            | _stroke_linecap_
            | _stroke_linejoin_
            | _stroke_miterlimit_
            | _stroke_dasharray_
            | _stroke_dashoffset_
            | _text_anchor_
            | _box_shadow_
            | _text_shadow_
            | _filter_
            | _backdrop_filter_
            | _transform_
            | _translate_
            | _rotate_
            | _scale_
            | _transform_origin_
            | _transform_box_
            | _clip_path_
            | _grid_template_columns_
            | _grid_template_rows_
            | _grid_auto_rows_
            | _grid_column_
            | _grid_row_
            | _grid_column_start_
            | _grid_column_end_
            | _grid_row_start_
            | _grid_row_end_ => self.add_value(name, Value::Tokens(tokens.into_owned()), important),

            //  =============================  COUNTER, CONTENT  =============================
            _counter_increment_ | _counter_reset_ => {
                let strings = value.iter().map(|tok| tok.get_repr(true)).collect();
                self.add_value(name, Value::Strings(strings), important);
            }

            _content_ => {
                let str = get_repr(value, 0, -1, true);
                self.add_value(name, Value::String(str), important);
            }

            //  ==================================  OTHER  ==================================
            _cursor_ => {
                let str = get_repr(value, 0, -1, true);
                self.add_value(name, Value::String(str), important);
            }

            //  =============================  CUSTOM PROPERTY  =============================
            // https://drafts.csswg.org/css-variables-2/#defining-variables
            _ => {
                if custom && names.s(name).len() >= 3 && (value.is_empty() || is_declaration_value(value, 0)) {
                    self.add_value(name, Value::Tokens(value.to_vec()), important);
                }
            }
        }
    }

    /// `parse_list_style(tokens, baseurl, important)`: `<'list-style-position'> || <'list-style-image'> ||
    /// <'list-style-type'>`, a `none` going to whichever of image and type is not otherwise set.
    fn parse_list_style(&mut self, tokens: &[CssToken], baseurl: &[u8], important: bool) {
        let mut ty = ListStyleType::list_style_type_disc.index();
        let mut position = ListStylePosition::list_style_position_outside.index();
        let mut image = Vec::new();

        let mut type_found = false;
        let mut position_found = false;
        let mut image_found = false;
        let mut none_count = 0;

        for token in tokens {
            if token.ident() == b"none" {
                none_count += 1;
                continue;
            }
            if !type_found && let Some(v) = parse_keyword(token, CssValues::new(&list_style_type_strings), 0) {
                ty = v;
                type_found = true;
            } else if !position_found && let Some(v) = parse_keyword(token, CssValues::new(&list_style_position_strings), 0) {
                position = v;
                position_found = true;
            } else if !image_found && let Some(url) = parse_list_style_image(token) {
                image = url;
                image_found = true;
            } else {
                // syntax error
                return;
            }
        }

        match none_count {
            0 => {}
            1 => {
                if type_found && image_found {
                    return;
                }
                if !type_found {
                    ty = ListStyleType::list_style_type_none.index();
                }
            }
            2 => {
                if type_found || image_found {
                    return;
                }
                ty = ListStyleType::list_style_type_none.index();
            }
            // syntax error
            _ => return,
        }

        self.add_value(_list_style_type_, ty, important);
        self.add_value(_list_style_position_, position, important);
        self.add_value(_list_style_image_, Value::String(image), important);
        self.add_value(_list_style_image_baseurl_, Value::String(baseurl.to_vec()), important);
    }

    /// `parse_border_radius(tokens, important)`: `<length-percentage [0,∞]>{1,4} [ / <length-percentage
    /// [0,∞]>{1,4} ]?`.
    fn parse_border_radius(&mut self, tokens: &[CssToken], important: bool) {
        let options = f_length_percentage | f_positive;
        let slash = tokens.iter().position(|t| t.ch() == i32::from(b'/'));
        match slash {
            None => {
                let len = parse_1234_lengths(tokens, options, CssValues::default());
                if !len.is_empty() {
                    self.add_four_properties(_border_top_left_radius_x_, &len, important);
                    self.add_four_properties(_border_top_left_radius_y_, &len, important);
                }
            }
            Some(i) => {
                let rx = parse_1234_lengths(&tokens[..i], options, CssValues::default());
                let ry = parse_1234_lengths(&tokens[i + 1..], options, CssValues::default());
                if !rx.is_empty() && !ry.is_empty() {
                    self.add_four_properties(_border_top_left_radius_x_, &rx, important);
                    self.add_four_properties(_border_top_left_radius_y_, &ry, important);
                }
            }
        }
    }

    /// `parse_border(tokens, important, container)`: every side's width, then style, then colour.
    fn parse_border(&mut self, names: &Names, tokens: &[CssToken], important: bool, container: Option<&dyn DocumentContainer>) {
        let Some((width, style, color)) = parse_border_helper(names, tokens, container) else { return };

        for name in [_border_left_width_, _border_right_width_, _border_top_width_, _border_bottom_width_] {
            self.add_value(name, width.clone(), important);
        }
        for name in [_border_left_style_, _border_right_style_, _border_top_style_, _border_bottom_style_] {
            self.add_value(name, style, important);
        }
        for name in [_border_left_color_, _border_right_color_, _border_top_color_, _border_bottom_color_] {
            self.add_value(name, color, important);
        }
    }

    /// `parse_border_side(name, tokens, important, container)`: `border-top` … and `outline`, into
    /// `<name>-width`, `-style` and `-color`.
    fn parse_border_side(
        &mut self,
        names: &Names,
        name: StringId,
        tokens: &[CssToken],
        important: bool,
        container: Option<&dyn DocumentContainer>,
    ) {
        let Some((width, style, color)) = parse_border_helper(names, tokens, container) else { return };
        let base = names.s(name);
        let width_name = names.id(&[&base[..], b"-width"].concat());
        self.add_value(width_name, width, important);
        let style_name = names.id(&[&base[..], b"-style"].concat());
        self.add_value(style_name, style, important);
        let color_name = names.id(&[&base[..], b"-color"].concat());
        self.add_value(color_name, color, important);
    }

    /// `parse_background(tokens, baseurl, important, container)`: layers between commas, a colour in the last
    /// only; every longhand of every layer, or nothing when one layer does not parse.
    fn parse_background(
        &mut self,
        names: &Names,
        tokens: &[CssToken],
        baseurl: &[u8],
        important: bool,
        container: Option<&dyn DocumentContainer>,
    ) {
        let layers = parse_comma_separated_list(tokens);
        if layers.is_empty() {
            return;
        }

        let mut color = WebColor::default();
        let mut images = Vec::new();
        let mut x_positions = Vec::new();
        let mut y_positions = Vec::new();
        let mut sizes = Vec::new();
        let mut repeats = Vec::new();
        let mut attachments = Vec::new();
        let mut origins = Vec::new();
        let mut clips = Vec::new();

        for (i, layer) in layers.iter().enumerate() {
            let Some(bg) = parse_bg_layer(names, layer, container, i == layers.len() - 1) else { return };

            color = bg.color;
            images.push(bg.image);
            x_positions.push(bg.position_x);
            y_positions.push(bg.position_y);
            sizes.push(bg.size);
            repeats.push(bg.repeat);
            attachments.push(bg.attachment);
            origins.push(bg.origin);
            clips.push(bg.clip);
        }

        self.add_value(_background_color_, color, important);
        self.add_value(_background_image_, Value::Images(images), important);
        self.add_value(_background_image_baseurl_, Value::String(baseurl.to_vec()), important);
        self.add_value(_background_position_x_, Value::LengthVector(x_positions), important);
        self.add_value(_background_position_y_, Value::LengthVector(y_positions), important);
        self.add_value(_background_size_, Value::Sizes(sizes), important);
        self.add_value(_background_repeat_, Value::IntVector(repeats), important);
        self.add_value(_background_attachment_, Value::IntVector(attachments), important);
        self.add_value(_background_origin_, Value::IntVector(origins), important);
        self.add_value(_background_clip_, Value::IntVector(clips), important);
    }

    /// `parse_background_image(tokens, baseurl, important, container)`: one image per layer.
    fn parse_background_image(
        &mut self,
        names: &Names,
        tokens: &[CssToken],
        baseurl: &[u8],
        important: bool,
        container: Option<&dyn DocumentContainer>,
    ) {
        let layers = parse_comma_separated_list(tokens);
        if layers.is_empty() {
            return;
        }

        let mut images = Vec::new();
        for layer in &layers {
            let mut image = Image::default();
            if layer.len() != 1 {
                return;
            }
            if !parse_bg_image(names, &layer[0], &mut image, container) {
                return;
            }
            images.push(image);
        }

        self.add_value(_background_image_, Value::Images(images), important);
        self.add_value(_background_image_baseurl_, Value::String(baseurl.to_vec()), important);
    }

    /// `parse_keyword_comma_list(name, tokens, important)`: one keyword per layer.
    fn parse_keyword_comma_list(&mut self, name: StringId, tokens: &[CssToken], important: bool) {
        let layers = parse_comma_separated_list(tokens);
        if layers.is_empty() {
            return;
        }

        let mut vec = Vec::new();
        for layer in &layers {
            if layer.len() != 1 {
                return;
            }
            let Some(idx) = parse_keyword(&layer[0], valid_values(name), 0) else { return };
            vec.push(idx);
        }

        self.add_value(name, Value::IntVector(vec), important);
    }

    /// `parse_background_position(tokens, important)`: one position per layer, keywords as percentages.
    fn parse_background_position(&mut self, tokens: &[CssToken], important: bool) {
        let layers = parse_comma_separated_list(tokens);
        if layers.is_empty() {
            return;
        }

        let mut x_positions = Vec::new();
        let mut y_positions = Vec::new();
        for layer in &layers {
            let mut x = CssLength::default();
            let mut y = CssLength::default();
            let mut index = 0;
            if !parse_bg_position(layer, &mut index, &mut x, &mut y, true) || index != layer.len() as i32 {
                return;
            }
            x_positions.push(x);
            y_positions.push(y);
        }

        self.add_value(_background_position_x_, Value::LengthVector(x_positions), important);
        self.add_value(_background_position_y_, Value::LengthVector(y_positions), important);
    }

    /// `parse_background_size(tokens, important)`: one size per layer.
    fn parse_background_size(&mut self, tokens: &[CssToken], important: bool) {
        let layers = parse_comma_separated_list(tokens);
        if layers.is_empty() {
            return;
        }

        let mut sizes = Vec::new();
        for layer in &layers {
            let mut size = CssSize::default();
            let mut index = 0;
            if !parse_bg_size(layer, &mut index, &mut size) || index != layer.len() as i32 {
                return;
            }
            sizes.push(size);
        }

        self.add_value(_background_size_, Value::Sizes(sizes), important);
    }

    /// `parse_font(tokens, important)` (https://drafts.csswg.org/css-fonts/#font-prop): a system font's name,
    /// or `<font-style-weight>? <font-size> [ / <line-height> ]? <font-family>`; every longhand.
    fn parse_font(&mut self, mut tokens: CssTokenVector, important: bool) {
        // initial values
        let mut style = FontStyle::font_style_normal.index();
        let mut variant = FontVariant::font_variant_normal.index();
        let mut weight = CssLength::predef_value(FontWeight::font_weight_normal.index());
        let mut size = CssLength::predef_value(FontSize::font_size_medium.index());
        let mut line_height = CssLength::predef_value(LineHeight::line_height_normal.index());
        let font_family;

        if tokens.len() == 1
            && (tokens[0].ty == STRING || tokens[0].ty == IDENT)
            && CssValues::new(&font_system_family_name_strings).has(&tokens[0].str)
        {
            font_family = tokens[0].str.clone();
        } else {
            let mut index = 0;
            parse_font_style_variant_weight(&tokens, &mut index, &mut style, &mut variant, &mut weight);

            // font-size = <absolute-size> | <relative-size> | <length-percentage [0,∞]> | math
            if !size.from_token(at(&tokens, index), f_length_percentage | f_positive, CssValues::new(&font_size_strings)) {
                return;
            }
            index += 1;

            if at(&tokens, index).ch() == i32::from(b'/') {
                index += 1;
                // line-height = normal | <number> | <length> | <percentage>
                if !line_height.from_token(at(&tokens, index), f_number | f_length_percentage, CssValues::new(&line_height_strings)) {
                    return;
                }
                index += 1;
            }

            let count = (index as usize).min(tokens.len());
            tokens.drain(0..count);
            match parse_font_family(&tokens) {
                Some(family) => font_family = family,
                None => return,
            }
        }
        self.add_value(_font_style_, style, important);
        self.add_value(_font_variant_, variant, important);
        self.add_value(_font_variant_numeric_, Value::String(Vec::new()), important);
        self.add_value(_font_weight_, weight, important);
        self.add_value(_font_size_, size, important);
        self.add_value(_line_height_, line_height, important);
        self.add_value(_font_family_, Value::String(font_family), important);
    }

    /// `parse_text_decoration(tokens, important, container)`: each colour, thickness and style as it comes,
    /// then the line of the tokens left.
    fn parse_text_decoration(&mut self, names: &Names, tokens: &[CssToken], important: bool, container: Option<&dyn DocumentContainer>) {
        let mut len = CssLength::default();
        let mut line_tokens = Vec::new();
        for token in tokens {
            if self.parse_text_decoration_color(names, token, important, container) {
                continue;
            }

            if parse_length(token, &mut len, f_length_percentage | f_positive, CssValues::new(&style_text_decoration_thickness_strings)) {
                self.add_value(_text_decoration_thickness_, len.clone(), important);
            } else if token.ty == IDENT {
                match CssValues::new(&style_text_decoration_style_strings).value_index(&token.ident()) {
                    Some(style) => self.add_value(_text_decoration_style_, style, important),
                    None => line_tokens.push(token.clone()),
                }
            } else {
                line_tokens.push(token.clone());
            }
        }
        if !line_tokens.is_empty() {
            self.parse_text_decoration_line(&line_tokens, important);
        }
    }

    /// `parse_text_decoration_color(token, important, container)`: a colour, or `auto` and `currentcolor` as
    /// the current colour.
    fn parse_text_decoration_color(
        &mut self,
        names: &Names,
        token: &CssToken,
        important: bool,
        container: Option<&dyn DocumentContainer>,
    ) -> bool {
        if let Some(color) = parse_color(names, token, resolver(container)) {
            self.add_value(_text_decoration_color_, color, important);
            return true;
        }
        if token.ty == IDENT && CssValues::new(&AUTO_CURRENTCOLOR).has(&token.ident()) {
            self.add_value(_text_decoration_color_, WebColor::CURRENT_COLOR, important);
            return true;
        }
        false
    }

    /// `parse_text_decoration_line(tokens, important)`: the flags of the line keywords among the tokens.
    fn parse_text_decoration_line(&mut self, tokens: &[CssToken], important: bool) {
        let mut val = 0;
        for token in tokens {
            if token.ty == IDENT
                && let Some(idx) = CssValues::new(&style_text_decoration_line_strings).value_index(&token.ident())
                && idx > 0
            {
                val |= 1 << (idx - 1);
            }
        }
        self.add_value(_text_decoration_line_, val, important);
    }

    /// `parse_text_emphasis(tokens, important, container)`: colours (the first wins), the rest as the style's
    /// text.
    fn parse_text_emphasis(&mut self, names: &Names, tokens: &[CssToken], important: bool, container: Option<&dyn DocumentContainer>) {
        let mut style = Vec::new();
        for token in tokens.iter().rev() {
            if self.parse_text_emphasis_color(names, token, important, container) {
                continue;
            }
            let mut word = token.str.clone();
            word.push(b' ');
            style.splice(0..0, word);
        }
        let style = trim(&style, WHITESPACE);
        if !style.is_empty() {
            self.add_value(_text_emphasis_style_, Value::String(style), important);
        }
    }

    /// `parse_text_emphasis_color(token, important, container)`.
    fn parse_text_emphasis_color(
        &mut self,
        names: &Names,
        token: &CssToken,
        important: bool,
        container: Option<&dyn DocumentContainer>,
    ) -> bool {
        if let Some(color) = parse_color(names, token, resolver(container)) {
            self.add_value(_text_emphasis_color_, color, important);
            return true;
        }
        if token.ty == IDENT && CssValues::new(&AUTO_CURRENTCOLOR).has(&token.ident()) {
            self.add_value(_text_emphasis_color_, WebColor::CURRENT_COLOR, important);
            return true;
        }
        false
    }

    /// `parse_text_emphasis_position(tokens, important)`.
    fn parse_text_emphasis_position(&mut self, tokens: &[CssToken], important: bool) {
        let mut val = 0;
        for token in tokens {
            if token.ty == IDENT
                && let Some(idx) = CssValues::new(&style_text_emphasis_position_strings).value_index(&token.ident())
                && idx > 0
            {
                val |= 1 << (idx - 1);
            }
        }
        self.add_value(_text_emphasis_position_, val, important);
    }

    /// `parse_flex(tokens, important)` (https://drafts.csswg.org/css-flexbox/#flex-property):
    /// `none | [ <'flex-grow'> <'flex-shrink'>? || <'flex-basis'> ]`. A single token is looked up through the
    /// name table, which keeps it.
    fn parse_flex(&mut self, names: &Names, tokens: &[CssToken], important: bool) {
        let n = tokens.len();
        if n > 3 {
            return;
        }

        let a = at(tokens, 0);
        let b = at(tokens, 1);
        let c = at(tokens, 2);

        let mut flex = Flex { grow: 1.0, shrink: 1.0, basis: CssLength::new(0.0, CssUnits::css_units_px) };

        if n == 1 {
            let ident = names.id(&a.ident());
            if matches!(ident, _initial_ | _auto_ | _none_) {
                let auto = CssLength::predef_value(FlexBasis::flex_basis_auto.index());
                flex = match ident {
                    _initial_ => Flex { grow: 0.0, shrink: 1.0, basis: auto },
                    _auto_ => Flex { grow: 1.0, shrink: 1.0, basis: auto },
                    _ => Flex { grow: 0.0, shrink: 0.0, basis: auto },
                };
            } else {
                let ok = flex.grow(a) || flex.basis(a, false);
                if !ok {
                    return;
                }
            }
        } else if n == 2 {
            // <number> <number>, <number> <basis>, <basis> <number>
            let ok = (flex.grow(a) && (flex.shrink(b) || flex.basis(b, false))) || (flex.basis(a, false) && flex.grow(b));
            if !ok {
                return;
            }
        } else {
            // <number> <number> <basis>, <basis> <number> <number>
            let ok = (flex.grow(a) && flex.shrink(b) && flex.basis(c, true)) || (flex.basis(a, false) && flex.grow(b) && flex.shrink(c));
            if !ok {
                return;
            }
        }

        self.add_value(_flex_grow_, Value::Float(flex.grow), important);
        self.add_value(_flex_shrink_, Value::Float(flex.shrink), important);
        self.add_value(_flex_basis_, flex.basis, important);
    }

    /// `parse_flex_flow(tokens, important)`: `<'flex-direction'> || <'flex-wrap'>`.
    fn parse_flex_flow(&mut self, tokens: &[CssToken], important: bool) {
        let mut flex_direction = FlexDirection::flex_direction_row.index();
        let mut flex_wrap = FlexWrap::flex_wrap_nowrap.index();

        let mut direction_found = false;
        let mut wrap_found = false;

        for token in tokens {
            if !direction_found && let Some(v) = parse_keyword(token, CssValues::new(&flex_direction_strings), 0) {
                flex_direction = v;
                direction_found = true;
            } else if !wrap_found && let Some(v) = parse_keyword(token, CssValues::new(&flex_wrap_strings), 0) {
                flex_wrap = v;
                wrap_found = true;
            } else {
                return;
            }
        }

        self.add_value(_flex_direction_, flex_direction, important);
        self.add_value(_flex_wrap_, flex_wrap, important);
    }

    /// `parse_align_self(name, tokens, important)` (https://www.w3.org/TR/css-align/#align-self-property):
    /// `auto | normal | stretch | [ [first | last]? && baseline ] | [safe | unsafe]? <self-position>`, as an
    /// index with flags; `align-items` takes no `auto`.
    fn parse_align_self(&mut self, name: StringId, tokens: &[CssToken], important: bool) {
        use FlexAlignItems::*;
        let n = tokens.len();
        if n > 2 {
            return;
        }
        // tokens[0] of no tokens is undefined in the C++; no caller passes none.
        let Some(first) = tokens.first() else { return };
        if first.ty != IDENT || (n == 2 && tokens[1].ty != IDENT) {
            return;
        }

        let mut a = first.ident();

        if name == _align_items_ && a == b"auto" {
            return;
        }

        if n == 1 {
            if let Some(idx) = CssValues::new(&flex_align_items_strings).value_index(&a) {
                self.add_value(name, idx, important);
            }
            return;
        }

        // Otherwise: n == 2
        let mut b = tokens[1].ident();

        if a == b"baseline" {
            std::mem::swap(&mut a, &mut b);
        }
        if b == b"baseline" && (a == b"first" || a == b"last") {
            let idx = flex_align_items_baseline.index()
                | if a == b"first" { flex_align_items_first.index() } else { flex_align_items_last.index() };
            self.add_value(name, idx, important);
            return;
        }

        // <overflow-position> <self-position>
        let idx = CssValues::new(&self_position_strings).value_index(&b);
        if let Some(idx) = idx
            && (a == b"safe" || a == b"unsafe")
        {
            let position = flex_align_items_center.index() + idx;
            let value = position | if a == b"safe" { flex_align_items_safe.index() } else { flex_align_items_unsafe.index() };
            self.add_value(name, value, important);
        }
    }

    /// `parse_grid_shorthand(name, tokens, important)`: `grid-template: none | <rows> / <columns>`; `grid` also
    /// `auto-flow <auto rows>? / <columns>`. Named areas, column flow and dense packing are dropped.
    fn parse_grid_shorthand(&mut self, name: StringId, tokens: &[CssToken], important: bool) {
        let mut rows = Vec::new();
        let mut columns = Vec::new();
        let mut slash = false;
        for tok in tokens {
            if tok.ty == STRING {
                return;
            }
            if tok.ch() == i32::from(b'/') {
                if slash {
                    return;
                }
                slash = true;
                continue;
            }
            if slash { &mut columns } else { &mut rows }.push(tok.clone());
        }
        let none = vec![CssToken::with_str(IDENT, b"none")];
        let mut auto_rows = vec![CssToken::with_str(IDENT, b"auto")];
        if !slash {
            if rows.len() != 1 || rows[0].ident() != b"none" {
                return;
            }
            columns = none.clone();
        } else if name == _grid_ && !rows.is_empty() && rows[0].ident() == b"auto-flow" {
            if rows.len() > 1 && rows[1].ident() == b"dense" {
                return;
            }
            if rows.len() > 1 {
                auto_rows = rows[1..].to_vec();
            }
            rows = none.clone();
        }
        if rows.is_empty() || columns.is_empty() {
            return;
        }
        if columns.iter().any(|tok| tok.ident() == b"auto-flow") {
            return;
        }
        self.add_value(_grid_template_rows_, Value::Tokens(rows), important);
        self.add_value(_grid_template_columns_, Value::Tokens(columns), important);
        if name == _grid_ {
            self.add_value(_grid_auto_rows_, Value::Tokens(auto_rows), important);
        }
    }

    /// `replace_properties(names, base, patch)`: each of the names as `base` has it (or none), then as `patch`
    /// adds it, its values moved out of `patch` as the C++ moves them (no one reads `patch` after); a name given
    /// again takes a copy until its last time.
    pub fn replace_properties(&mut self, names: &[StringId], base: &Style, patch: &mut Style) {
        for &name in names {
            match base.m_properties.get(&name) {
                None => {
                    self.m_properties.remove(&name);
                }
                Some(from) => {
                    self.m_properties.insert(name, from.clone());
                }
            }
        }
        for (i, &name) in names.iter().enumerate() {
            let from =
                if names[i + 1..].contains(&name) { patch.m_properties.get(&name).cloned() } else { patch.m_properties.remove(&name) };
            if let Some(from) = from {
                self.add_parsed_property(name, from);
            }
        }
    }

    /// `add_parsed_property(name, propval)`: the value, unless the property is `!important` and the value is
    /// not; with the next order.
    pub fn add_parsed_property(&mut self, name: StringId, propval: PropertyValue) {
        if let Some(ops) = &mut self.recording {
            ops.push(StyleOp { remove: false, important: false, name, value: propval.clone() });
        }
        match self.m_properties.get_mut(&name) {
            Some(prop) => {
                if !prop.m_important || propval.m_important {
                    *prop = propval;
                    self.m_counter += 1;
                    prop.m_order = self.m_counter;
                }
            }
            None => {
                self.m_counter += 1;
                let mut slot = propval;
                slot.m_order = self.m_counter;
                self.m_properties.insert(name, slot);
            }
        }
    }

    /// `remove_property(name, important)`.
    pub fn remove_property(&mut self, name: StringId, important: bool) {
        if let Some(ops) = &mut self.recording {
            ops.push(StyleOp { remove: true, important, name, value: PropertyValue::default() });
        }
        if let Some(prop) = self.m_properties.get(&name)
            && (!prop.m_important || important)
        {
            self.m_properties.remove(&name);
        }
    }

    /// `combine(src)`: `src`'s properties added after this style's, keeping their order among themselves.
    pub fn combine(&mut self, src: &Style) {
        let order_offset = self.m_counter;
        for (&name, property) in &src.m_properties {
            let order = property.m_order;
            let before = self.m_counter;
            self.add_parsed_property(name, property.clone());
            if self.m_counter != before {
                self.m_properties.entry(name).or_default().m_order = order_offset + order;
            }
        }
        self.m_counter = self.m_counter.max(order_offset + src.m_counter);
    }

    /// `subst_vars(el)`: each property waiting for `var()`, in the order of declaration, with its `var()`s
    /// replaced (a custom property looked up in this style as it is so far, then through `el`, which stands for
    /// the element's parent chain) and parsed again (a shorthand's longhands take its place in the order; a later declaration of
    /// one keeps it). One budget for all of the element's declarations; past it, or for a missing variable
    /// without fallback, a property is `unset` and a custom property the guaranteed-invalid value.
    pub fn subst_vars(&mut self, names: &Names, el: &dyn VarElement) {
        let mut pending: Vec<(StringId, PropertyValue)> =
            self.m_properties.iter().filter(|(_, p)| p.m_has_var).map(|(&name, p)| (name, p.clone())).collect();
        pending.sort_by_key(|(_, p)| p.m_order);
        let mut budget = VarBudget::default();
        for (name, prop) in pending {
            let mut value = match &prop.value {
                Value::Tokens(tokens) => tokens.clone(),
                // std::get of another alternative throws; a property with m_has_var holds tokens
                _ => continue,
            };
            let custom = is_custom(names, name);
            let mut chain = BTreeSet::from([name]);
            budget.invalid = budget.exhausted;
            if !budget.exhausted {
                resolve_vars(names, &mut value, &OwnFirst { own: &self.m_properties, el }, &mut chain, 0, &mut budget);
            }
            if budget.invalid {
                if custom {
                    // the guaranteed-invalid value: var() of it takes its fallback, nothing is inherited
                    let mut invalid_value = PropertyValue::new(Value::CssWideKeyword(CssWideKeyword::initial), prop.m_important);
                    invalid_value.m_order = prop.m_order;
                    self.m_properties.insert(name, invalid_value);
                    continue;
                }
                value = vec![CssToken::with_str(IDENT, b"unset")];
            } else if let Some(container) = el.container()
                && !custom
                && !container.sanitize_declaration(&names.s(name), &value)
            {
                value = vec![CssToken::with_str(IDENT, b"unset")];
            }
            let mut later = Vec::new();
            for &longhand in shorthand(name) {
                if let Some(it) = self.m_properties.get(&longhand)
                    && it.m_order > prop.m_order
                    && (it.m_important || !prop.m_important)
                {
                    later.push((longhand, it.clone()));
                }
            }
            // re-adding the same property: a custom one as tokens, a standard one parsed as its typed value
            self.add_property(names, name, &value, b"", prop.m_important, el.container());
            // The longhands it wrote take its place in the cascade, not a fresh later one.
            for &longhand in shorthand(name) {
                if let Some(it) = self.m_properties.get_mut(&longhand) {
                    it.m_order = prop.m_order;
                }
            }
            for (longhand, kept) in later {
                self.m_properties.insert(longhand, kept);
            }
        }
    }
}

/// `parse_bg_layer(tokens, container, bg, final_layer)` (https://drafts.csswg.org/css-backgrounds/#typedef-bg-layer):
/// `<bg-image> || <bg-position> [ / <bg-size> ]? || <repeat-style> || <attachment> || <visual-box> ||
/// <visual-box>`, and a colour in the final layer. One box sets origin and clip, a second the clip.
fn parse_bg_layer(names: &Names, tokens: &[CssToken], container: Option<&dyn DocumentContainer>, final_layer: bool) -> Option<BgLayer> {
    let mut bg = BgLayer {
        color: WebColor::TRANSPARENT,
        image: Image::default(),
        position_x: CssLength::new(0.0, CssUnits::css_units_percentage),
        position_y: CssLength::new(0.0, CssUnits::css_units_percentage),
        size: CssSize::new(
            CssLength::predef_value(BackgroundSize::background_size_auto.index()),
            CssLength::predef_value(BackgroundSize::background_size_auto.index()),
        ),
        repeat: BackgroundRepeat::background_repeat_repeat.index(),
        attachment: BackgroundAttachment::background_attachment_scroll.index(),
        origin: BackgroundBox::background_box_padding.index(),
        clip: BackgroundBox::background_box_border.index(),
    };

    let mut color_found = false;
    let mut image_found = false;
    let mut position_found = false;
    let mut repeat_found = false;
    let mut attachment_found = false;
    let mut origin_found = false;
    let mut clip_found = false;

    let mut i: i32 = 0;
    while i < tokens.len() as i32 {
        // A failed position/size may leave i at its '/', which nothing below takes.
        let tok = at(tokens, i);
        if !color_found
            && final_layer
            && let Some(color) = parse_color(names, tok, resolver(container))
        {
            bg.color = color;
            color_found = true;
        } else if !image_found && parse_bg_image(names, tok, &mut bg.image, container) {
            image_found = true;
        } else if !position_found && parse_bg_position_size(tokens, &mut i, &mut bg.position_x, &mut bg.position_y, &mut bg.size) {
            position_found = true;
            i -= 1;
        }
        // Note: double keyword <repeat-style> values are not supported yet.
        else if !repeat_found && let Some(v) = parse_keyword(at(tokens, i), CssValues::new(&background_repeat_strings), 0) {
            bg.repeat = v;
            repeat_found = true;
        } else if !attachment_found && let Some(v) = parse_keyword(at(tokens, i), CssValues::new(&background_attachment_strings), 0) {
            bg.attachment = v;
            attachment_found = true;
        } else if !origin_found && let Some(v) = parse_keyword(at(tokens, i), CssValues::new(&background_box_strings), 0) {
            bg.origin = v;
            origin_found = true;
            bg.clip = bg.origin;
        } else if !clip_found && let Some(v) = parse_keyword(at(tokens, i), CssValues::new(&background_box_strings), 0) {
            bg.clip = v;
            clip_found = true;
        } else {
            return None;
        }
        i += 1;
    }
    Some(bg)
}

/// `parse_flex`'s `flex`: the three values with what each token may be.
struct Flex {
    grow: f32,
    shrink: f32,
    basis: CssLength,
}

impl Flex {
    fn grow(&mut self, tok: &CssToken) -> bool {
        if tok.ty != NUMBER || tok.n.number < 0.0 {
            return false;
        }
        self.grow = tok.n.number;
        true
    }

    fn shrink(&mut self, tok: &CssToken) -> bool {
        if tok.ty != NUMBER || tok.n.number < 0.0 {
            return false;
        }
        self.shrink = tok.n.number;
        true
    }

    /// A unitless zero that is not already preceded by two flex factors is a flex factor.
    fn basis(&mut self, tok: &CssToken, unitless_zero_allowed: bool) -> bool {
        if !unitless_zero_allowed && tok.ty == NUMBER && Px(tok.n.number) == Px(0.0) {
            return false;
        }
        self.basis.from_token(tok, f_length_percentage | f_positive, CssValues::new(&flex_basis_strings))
    }
}
