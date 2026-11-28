//! `media_query.h` and `media_query.cpp`: media query lists (https://drafts.csswg.org/mediaqueries-5/),
//! parsed with their lengths in px as the document had them when it parsed, and evaluated three-valued
//! against media features.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::css_length::{CssLength, f_length};
use crate::css_parser::{f_componentize, f_remove_whitespace, is_any_value, normalize, normalize_tokens, parse_comma_separated_list};
use crate::css_tokenizer::{CV_FUNCTION, CssNumberType, CssToken, DIMENSION, IDENT, NUMBER, ROUND_BLOCK};
use crate::css_values::{CssValues, MediaType, media_type_strings, split_css_values};
use crate::document::Units;
use crate::html::lowcase;
use crate::string_id::*;
use crate::types::{FontMetrics, MediaFeatures};
use crate::{Names, Px, StringId};

/// `trilean` (https://drafts.csswg.org/mediaqueries/#evaluating).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trilean {
    False,
    True,
    Unknown,
}

impl Trilean {
    fn from_bool(b: bool) -> Trilean {
        if b { Trilean::True } else { Trilean::False }
    }

    /// `a && b`.
    pub fn and(self, b: Trilean) -> Trilean {
        if self == Trilean::False || b == Trilean::False {
            return Trilean::False;
        }
        if self == Trilean::True && b == Trilean::True {
            return Trilean::True;
        }
        Trilean::Unknown
    }

    /// `a || b`.
    pub fn or(self, b: Trilean) -> Trilean {
        if self == Trilean::True || b == Trilean::True {
            return Trilean::True;
        }
        if self == Trilean::False && b == Trilean::False {
            return Trilean::False;
        }
        Trilean::Unknown
    }
}

/// `!x`.
impl std::ops::Not for Trilean {
    type Output = Trilean;
    fn not(self) -> Trilean {
        match self {
            Trilean::False => Trilean::True,
            Trilean::True => Trilean::False,
            Trilean::Unknown => Trilean::Unknown,
        }
    }
}

/// The comparison operators of a media feature: `<`, `>`, `=` and the C++'s `u'⩽'`, `u'⩾'`, `u'≠'`.
pub const OP_LT: u16 = b'<' as u16;
pub const OP_GT: u16 = b'>' as u16;
pub const OP_EQ: u16 = b'=' as u16;
pub const OP_LE: u16 = 0x2A7D;
pub const OP_GE: u16 = 0x2A7E;
pub const OP_NE: u16 = 0x2260;

/// `media_query`: `<media-condition> | [ not | only ]? <media-type> [ and <media-condition-without-or> ]?`.
/// One that failed to parse stays `not all`.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaQuery {
    pub m_not: bool,
    /// A `media_type` value; a deprecated type is its table index plus one.
    pub m_media_type: i32,
    pub m_conditions: Vec<MediaCondition>,
}

impl Default for MediaQuery {
    fn default() -> MediaQuery {
        MediaQuery { m_not: true, m_media_type: MediaType::media_type_all.index(), m_conditions: Vec::new() }
    }
}

/// `media_condition`: `not <media-in-parens>`, or media-in-parens joined by `and` or by `or`.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaCondition {
    /// `_not_`, `_and_` or `_or_`.
    pub op: StringId,
    pub m_conditions: Vec<MediaInParens>,
}

impl Default for MediaCondition {
    fn default() -> MediaCondition {
        MediaCondition { op: _and_, m_conditions: Vec::new() }
    }
}

/// `media_feature`: `( [ <mf-plain> | <mf-boolean> | <mf-range> ] )`, its value(s) converted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaFeature {
    pub name: Vec<u8>,
    pub value: f32,
    pub value2: f32,
    pub op: u16,
    pub op2: u16,
}

/// `media_in_parens`: `( <media-condition> ) | <media-feature> | <general-enclosed>`.
#[derive(Clone, Debug, PartialEq)]
pub enum MediaInParens {
    Condition(MediaCondition),
    Feature(MediaFeature),
    /// `unknown`: <general-enclosed>.
    Unknown,
}

impl Default for MediaInParens {
    fn default() -> MediaInParens {
        MediaInParens::Condition(MediaCondition::default())
    }
}

/// `media_query_list`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaQueryList {
    pub m_queries: Vec<MediaQuery>,
}

/// `media_query_list_list`: the lists of nested `@media` rules, all of which must hold; shared by every
/// selector under them and the document, which marks it used or not.
#[derive(Debug, Default)]
pub struct MediaQueryListList {
    m_media_query_lists: Vec<MediaQueryList>,
    m_is_used: AtomicBool,
}

impl Clone for MediaQueryListList {
    fn clone(&self) -> MediaQueryListList {
        MediaQueryListList { m_media_query_lists: self.m_media_query_lists.clone(), m_is_used: AtomicBool::new(self.is_used()) }
    }
}

impl MediaQueryListList {
    pub fn add(&mut self, mq_list: MediaQueryList) {
        self.m_media_query_lists.push(mq_list);
    }

    pub fn lists(&self) -> &[MediaQueryList] {
        &self.m_media_query_lists
    }

    pub fn is_used(&self) -> bool {
        self.m_is_used.load(Ordering::Relaxed)
    }

    /// `apply_media_features(features)`: whether every list holds, kept; true when that changed.
    pub fn apply_media_features(&self, names: &Names, features: &MediaFeatures) -> bool {
        let mut apply = true;
        for mq_list in &self.m_media_query_lists {
            if !mq_list.check(names, features) {
                apply = false;
                break;
            }
        }
        let ret = apply != self.is_used();
        self.m_is_used.store(apply, Ordering::Relaxed);
        ret
    }
}

/// `eval_op(x, op, value)`: within 1e-5 `=`, `⩾` and `⩽` hold and `≠` fails; past that the comparison is
/// `pixel_t`'s (within 1e-4 equal).
pub fn eval_op(x: f32, op: u16, value: f32) -> bool {
    let epsilon = 0.00001f32;
    if (x - value).abs() < epsilon {
        if op == OP_EQ || op == OP_GE || op == OP_LE {
            return true;
        }
        if op == OP_NE {
            return false;
        }
    }

    match op {
        OP_LT => Px(x) < Px(value),
        OP_LE => Px(x) <= Px(value),
        OP_GT => Px(x) > Px(value),
        OP_GE => Px(x) >= Px(value),
        OP_EQ => Px(x) == Px(value),
        OP_NE => Px(x) != Px(value),
        _ => false,
    }
}

impl MediaFeature {
    /// `compare(x)`.
    pub fn compare(&self, x: f32) -> bool {
        if self.op2 == 0 {
            return eval_op(x, self.op, self.value);
        }
        eval_op(self.value, self.op, x) && eval_op(x, self.op2, self.value2)
    }

    fn compare_int(&self, x: i32) -> bool {
        self.compare(x as f32)
    }

    /// `check(features)`.
    pub fn check(&self, names: &Names, feat: &MediaFeatures) -> bool {
        match names.id(&self.name) {
            _width_ => self.compare(feat.width.value()),
            _height_ => self.compare(feat.height.value()),
            _device_width_ => self.compare(feat.device_width.value()),
            _device_height_ => self.compare(feat.device_height.value()),
            _orientation_ => self.compare_int(if feat.height >= feat.width { _portrait_.0 } else { _landscape_.0 } as i32),
            // 1/0 is a degenerate ratio, which browsers order after every other; 0/0 they take alike.
            _aspect_ratio_ => {
                if feat.height != Px(0.0) {
                    self.compare(feat.width.value() / feat.height.value())
                } else {
                    false
                }
            }
            _device_aspect_ratio_ => {
                if feat.device_height != Px(0.0) {
                    self.compare(feat.device_width.value() / feat.device_height.value())
                } else {
                    false
                }
            }
            _color_ => self.compare_int(feat.color),
            _color_index_ => self.compare_int(feat.color_index),
            _monochrome_ => self.compare_int(feat.monochrome),
            _resolution_ => self.compare(feat.resolution.value()),
            // never: unknown media features are refused when parsed
            StringId(_) => false,
        }
    }
}

impl MediaCondition {
    /// `check(features)`.
    pub fn check(&self, names: &Names, features: &MediaFeatures) -> Trilean {
        if self.op == _not_ {
            return !self.m_conditions[0].check(names, features);
        }
        if self.op == _and_ {
            // true if every term is, false if one is, unknown otherwise
            let mut result = Trilean::True;
            for condition in &self.m_conditions {
                result = result.and(condition.check(names, features));
                if result == Trilean::False {
                    return result;
                }
            }
            return result;
        }
        if self.op == _or_ {
            // false if every term is, true if one is, unknown otherwise
            let mut result = Trilean::False;
            for condition in &self.m_conditions {
                result = result.or(condition.check(names, features));
                if result == Trilean::True {
                    return result;
                }
            }
            return result;
        }
        Trilean::False
    }
}

impl MediaInParens {
    /// `check(features)`: <general-enclosed> is unknown.
    pub fn check(&self, names: &Names, features: &MediaFeatures) -> Trilean {
        match self {
            MediaInParens::Condition(condition) => condition.check(names, features),
            MediaInParens::Feature(feature) => Trilean::from_bool(feature.check(names, features)),
            MediaInParens::Unknown => Trilean::Unknown,
        }
    }
}

impl MediaQuery {
    /// `check(features)`: the deprecated media types and an unknown one match nothing.
    pub fn check(&self, names: &Names, features: &MediaFeatures) -> Trilean {
        let mut result = if self.m_media_type >= MediaType::media_type_first_deprecated.index()
            || self.m_media_type == MediaType::media_type_unknown.index()
        {
            Trilean::False
        } else if self.m_media_type == MediaType::media_type_all.index() {
            Trilean::True
        } else {
            Trilean::from_bool(self.m_media_type == features.ty)
        };

        if result == Trilean::True {
            for condition in &self.m_conditions {
                result = result.and(condition.check(names, features));
                if result == Trilean::False {
                    break;
                }
            }
        }

        if self.m_not {
            result = !result;
        }
        result
    }
}

impl MediaQueryList {
    pub fn empty(&self) -> bool {
        self.m_queries.is_empty()
    }

    /// `check(features)` (https://drafts.csswg.org/mediaqueries/#mq-list): an empty list holds; unknown is false.
    pub fn check(&self, names: &Names, features: &MediaFeatures) -> bool {
        if self.empty() {
            return true;
        }
        let mut result = Trilean::False;
        for query in &self.m_queries {
            result = result.or(query.check(names, features));
            if result == Trilean::True {
                break;
            }
        }
        result == Trilean::True
    }
}

/// What a media query reads of its document: the name table and the document's units.
#[derive(Clone, Copy, Debug)]
pub struct MediaDocument<'a> {
    pub names: &'a Names,
    pub units: Units<'a>,
}

/// `at(tokens, index)`: the token, a default one (type 0) out of range.
fn at(tokens: &[CssToken], index: usize) -> &CssToken {
    static INVALID: std::sync::OnceLock<CssToken> = std::sync::OnceLock::new();
    tokens.get(index).unwrap_or_else(|| INVALID.get_or_init(CssToken::default))
}

/// `parse_media_query_list(tokens, doc)` (https://drafts.csswg.org/mediaqueries-5/#typedef-media-query-list):
/// whitespace dropped but between `<` or `>` and `=`; a query that fails to parse is kept as `not all`.
pub fn parse_media_query_list(tokens: &[CssToken], doc: &MediaDocument) -> MediaQueryList {
    let keep_whitespace =
        |left: &CssToken, right: &CssToken| (left.ch() == i32::from(b'<') || left.ch() == i32::from(b'>')) && right.ch() == i32::from(b'=');
    let tokens = normalize_tokens(tokens.to_vec(), f_componentize | f_remove_whitespace, Some(&keep_whitespace));
    // an empty media query list evaluates to true
    if tokens.is_empty() {
        return MediaQueryList::default();
    }

    let mut result = MediaQueryList::default();
    for list in parse_comma_separated_list(&tokens) {
        let mut query = MediaQuery::default();
        parse_media_query(&list, &mut query, doc);
        // appended even when it failed to parse, as the standard has it
        result.m_queries.push(query);
    }
    result
}

/// `parse_media_query_list(str, doc)`.
pub fn parse_media_query_list_text(str: &[u8], doc: &MediaDocument) -> MediaQueryList {
    let tokens = normalize(str, 0, None);
    parse_media_query_list(&tokens, doc)
}

fn is_one_of_str(s: &[u8], words: &[&str]) -> bool {
    words.iter().any(|w| w.as_bytes() == s)
}

/// `parse_media_query(tokens, mquery, doc)`.
fn parse_media_query(tokens: &[CssToken], mquery: &mut MediaQuery, doc: &MediaDocument) -> bool {
    if tokens.is_empty() {
        return false;
    }
    let mut index = 0;

    let mut condition = MediaCondition::default();
    if parse_media_condition(tokens, &mut index, true, &mut condition, doc) && index == tokens.len() {
        mquery.m_not = false;
        mquery.m_media_type = MediaType::media_type_all.index();
        mquery.m_conditions = vec![condition];
        return true;
    }

    let ident = tokens[0].ident();
    let mut not = false;
    if ident == b"not" {
        index += 1;
        not = true;
    } else if ident == b"only" {
        index += 1; // ignored  https://drafts.csswg.org/mediaqueries-5/#mq-only
    }

    // <media-type> = <ident> except only, not, and, or, and layer
    let ident = at(tokens, index).ident();
    if is_one_of_str(&ident, &["", "only", "not", "and", "or", "layer"]) {
        return false;
    }
    let media_type = match CssValues::from(&media_type_strings).value_index(&ident) {
        Some(idx) => idx + 1,
        None => MediaType::media_type_unknown.index(),
    };
    index += 1;

    if at(tokens, index).ident() == b"and" {
        index += 1;
        if !parse_media_condition(tokens, &mut index, false, &mut condition, doc) || index != tokens.len() {
            return false;
        }
        mquery.m_conditions = vec![condition];
    }
    if index != tokens.len() {
        return false;
    }
    mquery.m_not = not;
    mquery.m_media_type = media_type;
    true
}

/// `parse_media_condition(tokens, index, or_allowed, condition, doc)`: `<media-not> | <media-in-parens>
/// [ <media-and>* | <media-or>* ]`, without `or` when it is not allowed. `condition` keeps what it was given
/// where the parse does not set it.
fn parse_media_condition(
    tokens: &[CssToken],
    index: &mut usize,
    or_allowed: bool,
    condition: &mut MediaCondition,
    doc: &MediaDocument,
) -> bool {
    let mut media_in_parens = MediaInParens::default();
    if at(tokens, *index).ident() == b"not" {
        if !parse_media_in_parens(at(tokens, *index + 1), &mut media_in_parens, doc) {
            return false;
        }
        condition.op = _not_;
        condition.m_conditions = vec![media_in_parens];
        *index += 2;
        return true;
    }

    if !parse_media_in_parens(at(tokens, *index), &mut media_in_parens, doc) {
        return false;
    }
    condition.m_conditions = vec![media_in_parens.clone()];
    *index += 1;

    let mut or_allowed = or_allowed;
    let mut and_allowed = true;
    loop {
        let ident = at(tokens, *index).ident();
        if ident == b"and" && and_allowed {
            condition.op = _and_;
            or_allowed = false;
        } else if ident == b"or" && or_allowed {
            condition.op = _or_;
            and_allowed = false;
        } else {
            return true;
        }
        *index += 1;

        if !parse_media_in_parens(at(tokens, *index), &mut media_in_parens, doc) {
            return false;
        }
        condition.m_conditions.push(media_in_parens.clone());
        *index += 1;
    }
}

/// `parse_media_in_parens(token, media_in_parens, doc)`: `( <media-condition> ) | <media-feature> |
/// <general-enclosed>`, the last a function or a block of any value.
fn parse_media_in_parens(token: &CssToken, media_in_parens: &mut MediaInParens, doc: &MediaDocument) -> bool {
    if token.ty == CV_FUNCTION {
        if !token.value.is_empty() && !is_any_value(&token.value) {
            return false;
        }
        *media_in_parens = MediaInParens::Unknown;
        return true;
    }

    if token.ty != ROUND_BLOCK {
        return false;
    }
    let tokens = &token.value;

    let mut index = 0;
    let mut condition = MediaCondition::default();
    let mut media_feature = MediaFeature::default();
    if parse_media_condition(tokens, &mut index, true, &mut condition, doc) && index == tokens.len() {
        *media_in_parens = MediaInParens::Condition(condition);
    } else if parse_media_feature(token, &mut media_feature, doc) {
        *media_in_parens = MediaInParens::Feature(media_feature);
    } else if !tokens.is_empty() && !is_any_value(tokens) {
        return false;
    } else {
        *media_in_parens = MediaInParens::Unknown;
    }
    true
}

/// `mf_info`: a media feature's type (`_range_` or `_discrete_`), its value type and its keywords.
#[derive(Clone, Debug)]
struct MfInfo {
    ty: StringId,
    value_type: StringId,
    keywords: &'static [StringId],
}

/// `supported_media_features[name]`, or none (`at` of the map gives an empty one).
fn supported_media_feature(name: &[u8]) -> Option<MfInfo> {
    let (ty, value_type, keywords): (StringId, StringId, &'static [StringId]) = match name {
        // 4. Viewport/Page Dimensions Media Features
        b"width" | b"height" => (_range_, _length_, &[]),
        b"aspect-ratio" => (_range_, _ratio_, &[]),
        b"orientation" => (_discrete_, _keyword_, &[_portrait_, _landscape_]),
        // 5. Display Quality Media Features
        b"resolution" => (_range_, _resolution_, &[]),
        // 6. Color Media Features
        b"color" | b"color-index" | b"monochrome" => (_range_, _integer_, &[]),
        // Deprecated Media Features
        b"device-width" | b"device-height" => (_range_, _length_, &[]),
        b"device-aspect-ratio" => (_range_, _ratio_, &[]),
        _ => return None,
    };
    Some(MfInfo { ty, value_type, keywords })
}

const ALLOWED_RESOLUTION_UNITS: [&str; 4] = split_css_values::<4>("dpi;dpcm;dppx;x");

/// `convert_units(mfi, val, doc)`: the value(s) checked against the feature's value type and made a
/// number in px, dpi, a ratio or a keyword's id.
fn convert_units(mfi: &MfInfo, val: &mut [CssToken; 2], doc: &MediaDocument) -> bool {
    match mfi.value_type {
        // nothing to convert, just verify
        _integer_ => val[0].ty == NUMBER && val[0].n.number_type == CssNumberType::css_number_integer && val[1].ty == 0,
        _length_ => {
            if val[1].ty != 0 {
                return false;
            }
            let mut length = CssLength::default();
            if !length.from_token(&val[0], f_length, CssValues::default()) {
                return false;
            }
            let mut fm = FontMetrics::default();
            fm.font_size = doc.units.container.get_default_font_size();
            fm.x_height = fm.font_size;
            doc.units.cvt_units(&mut length, &fm, Px(0.0));
            val[0].n.number = length.val();
            true
        }
        // https://drafts.csswg.org/css-values-4/#resolution
        _resolution_ => {
            if val[1].ty != 0 {
                return false;
            }
            if val[0].ty == DIMENSION {
                let unit = lowcase(val[0].unit());
                // As the C++ has it: a unit of the list, or a negative value, is refused, and any other unit
                // is taken as dpi.
                if CssValues::from(&ALLOWED_RESOLUTION_UNITS).has(&unit) || val[0].n.number < 0.0 {
                    return false;
                }
                // dppx is 96 dpi, as document_container::get_media_features has it.
                if unit == b"dppx" || unit == b"x" {
                    val[0].n.number *= 96.0;
                } else if unit == b"dpcm" {
                    val[0].n.number *= 2.54; // 1in = 2.54cm
                }
                return true;
            }
            // https://drafts.csswg.org/mediaqueries/#resolution
            if val[0].ident() == b"infinite" {
                val[0] = CssToken::with_number(NUMBER, f32::INFINITY, CssNumberType::css_number_number);
                return true;
            }
            // <resolution> takes no unitless zero
            false
        }
        // https://drafts.csswg.org/css-values-4/#ratio  <ratio> = <number [0,∞]> [ / <number [0,∞]> ]?
        _ratio_ => {
            if val[0].ty == NUMBER && val[0].n.number >= 0.0 && ((val[1].ty == NUMBER && val[1].n.number >= 0.0) || val[1].ty == 0) {
                if val[1].ty == NUMBER {
                    // may be 0, and the ratio infinite
                    val[0].n.number /= val[1].n.number;
                }
                return true;
            }
            false
        }
        _keyword_ => {
            if val[1].ty != 0 {
                return false;
            }
            let ident = doc.names.id(&val[0].ident());
            if !mfi.keywords.contains(&ident) {
                return false;
            }
            val[0] = CssToken::with_number(NUMBER, ident.0 as f32, CssNumberType::css_number_integer);
            true
        }
        _ => false,
    }
}

/// Which syntax `verify_and_convert_units` reads: `(name)`, `(name: value)` or a range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Syntax {
    Boolean,
    Plain,
    Range,
}

impl MediaFeature {
    fn named(name: Vec<u8>) -> MediaFeature {
        MediaFeature { name, ..MediaFeature::default() }
    }

    /// `verify_and_convert_units(syntax, val, val2, doc)`.
    fn verify_and_convert_units(
        &mut self,
        syntax: Syntax,
        val: &mut [CssToken; 2],
        val2: Option<&mut [CssToken; 2]>,
        doc: &MediaDocument,
    ) -> bool {
        // https://drafts.csswg.org/mediaqueries/#mq-boolean-context: min- and max- are invalid there
        if syntax == Syntax::Boolean {
            let Some(mf_info) = supported_media_feature(&self.name) else { return false };
            self.value = if mf_info.value_type == _keyword_ { _none_.0 as f32 } else { 0.0 };
            self.op = OP_NE;
            return true;
        }
        if syntax == Syntax::Plain {
            let prefix = &self.name[..self.name.len().min(4)];
            if prefix == b"min-" || prefix == b"max-" {
                let min = prefix == b"min-";
                let real_name = self.name[4..].to_vec();
                let Some(mf_info) = supported_media_feature(&real_name) else { return false };
                if mf_info.ty == _discrete_ {
                    return false;
                }
                if !convert_units(&mf_info, val, doc) {
                    return false;
                }
                self.value = val[0].n.number;
                self.op = if min { OP_GE } else { OP_LE };
                self.name = real_name;
                return true;
            }
            let Some(mf_info) = supported_media_feature(&self.name) else { return false };
            if !convert_units(&mf_info, val, doc) {
                return false;
            }
            self.value = val[0].n.number;
            self.op = OP_EQ;
            return true;
        }
        // range syntax
        let Some(mf_info) = supported_media_feature(&self.name) else { return false };
        if mf_info.ty == _discrete_ {
            return false;
        }
        if !convert_units(&mf_info, val, doc) {
            return false;
        }
        self.value = val[0].n.number;
        if let Some(val2) = val2 {
            if !convert_units(&mf_info, val2, doc) {
                return false;
            }
            self.value2 = val2[0].n.number;
        }
        true
    }
}

/// `parse_media_feature(token, result, doc)`: `( [ <mf-plain> | <mf-boolean> | <mf-range> ] )`.
fn parse_media_feature(token: &CssToken, result: &mut MediaFeature, doc: &MediaDocument) -> bool {
    if token.ty != ROUND_BLOCK || token.value.is_empty() {
        return false;
    }
    let tokens = &token.value;

    if tokens.len() == 1 {
        let mut mf = MediaFeature::named(tokens[0].ident());
        let mut none = <[CssToken; 2]>::default();
        if !mf.verify_and_convert_units(Syntax::Boolean, &mut none, None, doc) {
            return false;
        }
        *result = mf;
        return true;
    }

    if tokens[0].ty == IDENT && tokens[1].ch() == i32::from(b':') {
        let mut val = <[CssToken; 2]>::default();
        let mut index = 2;
        if !parse_mf_value(tokens, &mut index, &mut val) || index != tokens.len() {
            return false;
        }
        let mut mf = MediaFeature::named(tokens[0].ident());
        if !mf.verify_and_convert_units(Syntax::Plain, &mut val, None, doc) {
            return false;
        }
        *result = mf;
        return true;
    }

    parse_mf_range(tokens, result, doc)
}

/// `parse_mf_value(tokens, index, val)`: `<number> | <dimension> | <ident> | <ratio>`; a ratio fills both
/// values, anything else the first only.
fn parse_mf_value(tokens: &[CssToken], index: &mut usize, val: &mut [CssToken; 2]) -> bool {
    let a = at(tokens, *index);
    let b = at(tokens, *index + 1);
    let c = at(tokens, *index + 2);

    if !matches!(a.ty, NUMBER | DIMENSION | IDENT) {
        return false;
    }

    if a.ty == NUMBER && a.n.number >= 0.0 && b.ch() == i32::from(b'/') && c.ty == NUMBER && c.n.number >= 0.0 {
        val[0] = a.clone();
        val[1] = c.clone();
        *index += 3;
    } else {
        val[0] = a.clone();
        *index += 1;
    }
    true
}

/// `mirror(op)`: the operator with its sides swapped.
fn mirror(op: u16) -> u16 {
    match op {
        OP_LT => OP_GT,
        OP_GT => OP_LT,
        OP_LE => OP_GE,
        OP_GE => OP_LE,
        _ => op,
    }
}

/// `parse_mf_range(tokens, result, doc)`: `<mf-name> <mf-comparison> <mf-value>`, the same reversed, or a
/// value, the name and a value between two `<` or two `>`. The values read stay in `val` across the
/// alternatives tried, as in the C++.
fn parse_mf_range(tokens: &[CssToken], result: &mut MediaFeature, doc: &MediaDocument) -> bool {
    if tokens.len() < 3 {
        return false;
    }

    let mut index: usize;
    let mut name = Vec::new();
    let mf_name = |index: &mut usize, name: &mut Vec<u8>| -> bool {
        if at(tokens, *index).ty != IDENT {
            return false;
        }
        *name = at(tokens, *index).ident();
        // (infinite = resolution): resolution is the only range feature that takes a keyword
        if name == b"infinite" {
            return false;
        }
        *index += 1;
        true
    };
    let mf_lt_gt = |index: &mut usize, lg: u8, op: &mut u16| -> bool {
        let tok = at(tokens, *index);
        let tok1 = at(tokens, *index + 1);
        if tok.ch() != i32::from(lg) {
            return false;
        }
        if tok1.ch() == i32::from(b'=') {
            *index += 2;
            *op = if lg == b'<' { OP_LE } else { OP_GE };
        } else {
            *index += 1;
            *op = u16::from(lg);
        }
        true
    };
    let mf_comparison = |index: &mut usize, op: &mut u16| -> bool {
        if at(tokens, *index).ch() == i32::from(b'=') {
            *index += 1;
            *op = OP_EQ;
            return true;
        }
        mf_lt_gt(index, b'<', op) || mf_lt_gt(index, b'>', op)
    };
    let end = |index: usize| index == tokens.len();

    let mut op = 0u16;
    let mut val = <[CssToken; 2]>::default();
    let first = {
        index = 0;
        mf_name(&mut index, &mut name) && mf_comparison(&mut index, &mut op) && parse_mf_value(tokens, &mut index, &mut val) && end(index)
    } || {
        index = 0;
        parse_mf_value(tokens, &mut index, &mut val)
            && mf_comparison(&mut index, &mut op)
            && mf_name(&mut index, &mut name)
            && end(index)
            && {
                op = mirror(op);
                true
            }
    };
    if first {
        let mut mf = MediaFeature::named(name);
        mf.op = op;
        if !mf.verify_and_convert_units(Syntax::Range, &mut val, None, doc) {
            return false;
        }
        *result = mf;
        return true;
    }
    let mut op2 = 0u16;
    let mut val2 = <[CssToken; 2]>::default();
    let second = {
        index = 0;
        parse_mf_value(tokens, &mut index, &mut val)
            && mf_lt_gt(&mut index, b'<', &mut op)
            && mf_name(&mut index, &mut name)
            && mf_lt_gt(&mut index, b'<', &mut op2)
            && parse_mf_value(tokens, &mut index, &mut val2)
            && end(index)
    } || {
        index = 0;
        parse_mf_value(tokens, &mut index, &mut val)
            && mf_lt_gt(&mut index, b'>', &mut op)
            && mf_name(&mut index, &mut name)
            && mf_lt_gt(&mut index, b'>', &mut op2)
            && parse_mf_value(tokens, &mut index, &mut val2)
            && end(index)
    };
    if second {
        let mut mf = MediaFeature::named(name);
        mf.op = op;
        mf.op2 = op2;
        if !mf.verify_and_convert_units(Syntax::Range, &mut val, Some(&mut val2), doc) {
            return false;
        }
        *result = mf;
        return true;
    }
    false
}

/// A shared list of lists, as selectors hold it.
pub type MediaQueryListListPtr = Arc<MediaQueryListList>;
