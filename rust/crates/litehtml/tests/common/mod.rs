//! What the litehtml tests share: the Rust side of every `oracle/lhdump` record, the generators of what the
//! records carry, and the corpus.
//!
//! lhdump reads records `u8 op, u32 length, bytes` and answers each with `u8 status, u32 length, bytes`
//! (a setting gets no answer). Numbers are little-endian, floats their bits, a string a u32 length and its
//! bytes. A token is `i32 type (ch), f32 number, u8 number_type, u8 hash_type, str, repr, u32 count` and
//! that many tokens of its value; the number fields are zero but for NUMBER, PERCENTAGE and DIMENSION and
//! the hash type but for HASH where lhdump writes them, and set only there where it reads them. A colour
//! is `u8 r, g, b, a, is_current_color`.
//!
//! The name table is the process's in the C++, so an [`Lh`] keeps one [`Names`] for every case of a run, in
//! the order the cases go to lhdump. Each record:
//!
//! - `E` NAME=VALUE: a setting, an `LH_NO_*` switch for the cases after it (lhdump clears those it inherits).
//! - `d` text: `t_strtod` (f64, u32 end) and `t_strtof` (f32).
//! - `c` text: `convert_string_to_number` (f64) and that as a float.
//! - `p` f32 a, f32 b, i32 k: `pixel_t` comparisons as six bits, then every operator's result (see `pixel`).
//! - `n` u32 steps, each `i` str, `f` str, `s` u32 or `u`: `_id` (u32), `_find_id` (u8, u32), `_s` (str),
//!   `_id_usage` (u64, u64).
//! - `N`: the table, u32 count and every name, then `empty_id` and `star_id`.
//! - `t`: the keyword tables, u32 count, each its name, u32 size and entries; then `border_width_values` and
//!   `background_position_percentages`.
//! - `v` u8 table, u32 index, text: `value_index` (u8, i32), `value_by_index` (str), `has` (u8).
//! - `C` u8 which (0 hash, 1 rgb, 2 hsl, 3 function), a token: status 1 and the colour, or 0 and 1, 2, 3, 4.
//! - `H` f32 h, s, l: `hsl_to_rgb`, three f32.
//! - `q` f32 x, lo, hi, f32 value, u8 units, f32 max: `clamp` (f32) and `calc_percent_and_clamp` (u8).
//! - `D` r, g, b, a, f64 fraction: `darken`'s colour. `S` r, g, b, a: `to_string`. `R` text: `resolve_name`.
//! - `L` u8 start, i32 options, u8 keywords table (past the tables: none), a token: `from_token`'s status
//!   and the length after it (see `length`).
//! - `A` u8 percents_allowed, a token: `parse_angle`'s status and the angle (-7 when it gives none).
//! - `k` u8 style, i32 value: a list marker. `u` text: UTF-8 read both ways (see `utf8`). `a` u32 code points:
//!   `append_char`. `s` delims, preserve, quote, text: `split_string`. `m` a, b: html.cpp's helpers (see
//!   `helpers`). `x`: the URL classes of every byte.
//! - `T` text: `tokenize` of it as it is, u32 count and the tokens.
//! - `Z` u8 options, u8 keep (0 none, 1 media queries'), text: `normalize`, the tokens, then `get_repr` of
//!   them without and with spaces.
//! - `F` text: `filter_code_points`.
//! - `K` text: normalized (componentized, no whitespace), u32 count, then `parse_color` of a single token
//!   (status and colour as for `C`).
//! - `G` i32 options, u8 table, text: normalized as for `K`, u32 count, `from_token` of a single token and the
//!   length after it.
//! - `P` text: componentized, its comma-separated lists, `is_any_value`, `is_declaration_value`,
//!   `skip_whitespace` from 0 (u8, i32), each token's `ident()`.
//! - `M`: litehtml's `master_css`.
//!
//! The stylesheet records read lhdump's one document (a 375 x 667 screen at 2x, 16 px default font,
//! `TestContainer`'s imports) and evaluate media queries against the grid of the last `g`:
//!
//! - `g` u32 count, each i32 type, f32 width, height, device width and height, i32 color, color index,
//!   monochrome, f32 resolution: a setting, the grid.
//! - `r` u8 top_level, text: `css_parser::parse_stylesheet`, u32 count and each rule: u8 type, name, prelude
//!   tokens, block token, then a style rule's declarations (name, value tokens, u8 important) and nested rules,
//!   or an at-rule's block as a list of rules.
//! - `B` text: componentized, a style block's declarations and rules, then the declarations as `style::add`
//!   takes them.
//! - `e` text: `parse_media_query_list`, every query (u8 not, i32 type, its conditions: u32 op, u32 count, each
//!   u8 kind and a condition, or a feature's name, f32 value and value2, u32 op and op2), then for each grid
//!   point every query's trilean and the list's answer.
//! - `w` u8 scheme (0 as is, 1 light, 2 dark: the renderer's rewrite first), text: `css::parse_css_stylesheet`,
//!   u32 count and each selector's i32 order, u8 layered, i32 rule and i32 media list (-1 none, else in order
//!   of first use), then u32 lists and each over the grid (u8 used).
//! - `o` u8 dark, text: the renderer's `rewrite_color_scheme`.
//! - `Q` u32 count and tokens: how many selectors `parse_selector_list` makes of them (stage L4's, asked
//!   before a run). `Y` u32 count and tokens: status 1 when `style::add` keeps a property of them (L5's).
//! - `U` a token: `parse_url`. `X` bytes: `decode` as UTF-8. `V` a token: `from_token` with
//!   f_length_percentage, then the document's `to_pixels` (f32) and `cvt_units` (the length after it).

#![allow(dead_code, unused_imports)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use twidget_testkit::{Comparison, Lcg, Oracle, framed, hex, mutate};

use twidget_litehtml::css_length::*;
use twidget_litehtml::css_parser::{self, CssParser, f_componentize, f_remove_whitespace, normalize, normalize_tokens};
use twidget_litehtml::css_tokenizer::*;
use twidget_litehtml::css_values::*;
use twidget_litehtml::document::Units;
use twidget_litehtml::document_container::DocumentContainer;
use twidget_litehtml::encodings::{Encoding, decode};
use twidget_litehtml::html::lowcase;
use twidget_litehtml::media_query::{MediaCondition, MediaDocument, MediaInParens, MediaQueryListList, parse_media_query_list_text};
use twidget_litehtml::style::parse_url;
use twidget_litehtml::stylesheet::{
    Css, RawDeclaration, RawRule, RuleType, StyleRule, StyleSheetHost, StyleSheetInput, supports_candidates,
};
use twidget_litehtml::types::{FontMetrics, MediaFeatures};
use twidget_litehtml::*;

#[derive(Default)]
pub struct Out {
    pub s: Vec<u8>,
}

impl Out {
    pub fn u8(&mut self, v: u8) -> &mut Out {
        self.s.push(v);
        self
    }
    pub fn u32(&mut self, v: u32) -> &mut Out {
        self.s.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn i32(&mut self, v: i32) -> &mut Out {
        self.s.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u64(&mut self, v: u64) -> &mut Out {
        self.s.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn f32(&mut self, v: f32) -> &mut Out {
        self.u32(v.to_bits())
    }
    pub fn f64(&mut self, v: f64) -> &mut Out {
        self.s.extend_from_slice(&v.to_bits().to_le_bytes());
        self
    }
    pub fn str(&mut self, t: &[u8]) -> &mut Out {
        self.u32(t.len() as u32);
        self.s.extend_from_slice(t);
        self
    }
    pub fn token(&mut self, t: &CssToken) -> &mut Out {
        let numeric = matches!(t.ty, NUMBER | PERCENTAGE | DIMENSION);
        self.i32(t.ty);
        self.f32(if numeric { t.n.number } else { 0.0 });
        self.u8(if numeric { t.n.number_type as u8 } else { 0 });
        self.u8(if t.ty == HASH { t.hash_type as u8 } else { 0 });
        self.str(&t.str);
        self.str(&t.repr);
        self.u32(t.value.len() as u32);
        for v in &t.value {
            self.token(v);
        }
        self
    }
    pub fn color(&mut self, c: &web_color::WebColor) -> &mut Out {
        self.u8(c.red).u8(c.green).u8(c.blue).u8(c.alpha).u8(u8::from(c.is_current_color))
    }
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.s)
    }
}

pub struct In<'a> {
    pub s: &'a [u8],
    pub at: usize,
}

impl<'a> In<'a> {
    pub fn new(s: &'a [u8]) -> In<'a> {
        In { s, at: 0 }
    }
    fn raw<const N: usize>(&mut self) -> [u8; N] {
        let mut v = [0; N];
        if self.at + N <= self.s.len() {
            v.copy_from_slice(&self.s[self.at..self.at + N]);
        }
        self.at = (self.at + N).min(self.s.len());
        v
    }
    pub fn u8(&mut self) -> u8 {
        self.raw::<1>()[0]
    }
    pub fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.raw())
    }
    pub fn i32(&mut self) -> i32 {
        i32::from_le_bytes(self.raw())
    }
    pub fn f32(&mut self) -> f32 {
        f32::from_bits(self.u32())
    }
    pub fn f64(&mut self) -> f64 {
        f64::from_bits(u64::from_le_bytes(self.raw()))
    }
    pub fn str(&mut self) -> Vec<u8> {
        let n = (self.u32() as usize).min(self.s.len() - self.at);
        let out = self.s[self.at..self.at + n].to_vec();
        self.at += n;
        out
    }
    pub fn rest(&mut self) -> Vec<u8> {
        let out = self.s[self.at..].to_vec();
        self.at = self.s.len();
        out
    }
    /// A token as lhdump's `read_token` builds it.
    pub fn token(&mut self, depth: u32) -> CssToken {
        let mut t = CssToken::new(self.i32());
        let number = self.f32();
        let number_type = self.u8();
        let hash_type = self.u8();
        if matches!(t.ty, NUMBER | PERCENTAGE | DIMENSION) {
            t.n = Number {
                number,
                number_type: if number_type == 0 { CssNumberType::css_number_integer } else { CssNumberType::css_number_number },
            };
        }
        if t.ty == HASH {
            t.hash_type = if hash_type == 0 { CssHashType::css_hash_unrestricted } else { CssHashType::css_hash_id };
        }
        t.str = self.str();
        t.repr = self.str();
        let children = self.u32();
        let mut i = 0;
        while i < children && depth < 64 && self.at < self.s.len() {
            t.value.push(self.token(depth + 1));
            i += 1;
        }
        t
    }
}

/// The keyword tables in lhdump's order.
pub const TABLES: [(&str, &[&str]); 53] = [
    ("style_display_strings", &style_display_strings),
    ("font_size_strings", &font_size_strings),
    ("line_height_strings", &line_height_strings),
    ("font_style_strings", &font_style_strings),
    ("font_system_family_name_strings", &font_system_family_name_strings),
    ("font_variant_strings", &font_variant_strings),
    ("font_weight_strings", &font_weight_strings),
    ("list_style_type_strings", &list_style_type_strings),
    ("list_style_position_strings", &list_style_position_strings),
    ("vertical_align_strings", &vertical_align_strings),
    ("border_width_strings", &border_width_strings),
    ("border_style_strings", &border_style_strings),
    ("element_float_strings", &element_float_strings),
    ("element_clear_strings", &element_clear_strings),
    ("css_units_strings", &css_units_strings),
    ("background_attachment_strings", &background_attachment_strings),
    ("background_repeat_strings", &background_repeat_strings),
    ("background_box_strings", &background_box_strings),
    ("background_position_strings", &background_position_strings),
    ("element_position_strings", &element_position_strings),
    ("direction_strings", &direction_strings),
    ("text_align_strings", &text_align_strings),
    ("text_transform_strings", &text_transform_strings),
    ("font_smoothing_strings", &font_smoothing_strings),
    ("white_space_strings", &white_space_strings),
    ("overflow_strings", &overflow_strings),
    ("background_size_strings", &background_size_strings),
    ("visibility_strings", &visibility_strings),
    ("border_collapse_strings", &border_collapse_strings),
    ("table_layout_strings", &table_layout_strings),
    ("content_property_strings", &content_property_strings),
    ("user_select_strings", &user_select_strings),
    ("overflow_wrap_strings", &overflow_wrap_strings),
    ("word_break_strings", &word_break_strings),
    ("resize_strings", &resize_strings),
    ("appearance_strings", &appearance_strings),
    ("box_sizing_strings", &box_sizing_strings),
    ("media_type_strings", &media_type_strings),
    ("flex_direction_strings", &flex_direction_strings),
    ("flex_wrap_strings", &flex_wrap_strings),
    ("flex_justify_content_strings", &flex_justify_content_strings),
    ("self_position_strings", &self_position_strings),
    ("flex_align_items_strings", &flex_align_items_strings),
    ("flex_align_content_strings", &flex_align_content_strings),
    ("flex_basis_strings", &flex_basis_strings),
    ("caption_side_strings", &caption_side_strings),
    ("style_text_decoration_line_strings", &style_text_decoration_line_strings),
    ("style_text_decoration_style_strings", &style_text_decoration_style_strings),
    ("style_text_decoration_thickness_strings", &style_text_decoration_thickness_strings),
    ("style_text_emphasis_position_strings", &style_text_emphasis_position_strings),
    ("radial_extent_strings", &radial_extent_strings),
    ("color_space_strings", &color_space_strings),
    ("hue_interpolation_strings", &hue_interpolation_strings),
];

pub fn keywords(index: u8) -> CssValues {
    TABLES.get(usize::from(index)).map(|t| CssValues::new(t.1)).unwrap_or_default()
}

/// lhdump's `scale_unit`: each unit its own scale.
pub fn scale_unit(value: f32, unit: CssUnits) -> f32 {
    value * (unit.index() as f32 + 0.5)
}

/// The calc length a `L` case may start from: `calc(50% - 3em)`.
pub fn start_calc() -> CssToken {
    let mut calc = CssToken::with_str(CV_FUNCTION, b"calc");
    calc.value = vec![
        CssToken::with_number(PERCENTAGE, 50.0, CssNumberType::css_number_integer),
        CssToken::new(WHITESPACE),
        CssToken::new(i32::from(b'-')),
        CssToken::new(WHITESPACE),
        CssToken { str: b"em".to_vec(), ..CssToken::with_number(DIMENSION, 3.0, CssNumberType::css_number_integer) },
    ];
    calc
}

/// The length after a case, as lhdump's `write_length` writes it.
pub fn length(o: &mut Out, len: &CssLength) {
    o.u8(u8::from(len.is_predefined())).i32(len.predef()).u8(len.units().index() as u8).f32(len.val()).u8(u8::from(len.calc().is_some()));
    o.str(&len.to_string());
    for w in [0.0f32, 100.0, -37.5, 1e9, 3.3] {
        o.f32(len.calc_percent(Px(w)).value());
    }
    let calc = len.calc().map(|c| &**c);
    for base in [0.0f32, 250.0, -1.0] {
        o.f32(calc_evaluate(calc, Px(base), None).value());
        o.f32(calc_evaluate(calc, Px(base), Some(&scale_unit)).value());
    }
    let mut px = -1.0;
    let resolved = calc_resolve_units(len.calc(), &scale_unit, &mut px);
    o.f32(px);
    o.u8(match (&resolved, len.calc()) {
        (None, _) => 0,
        (Some(r), Some(c)) if Arc::ptr_eq(r, c) => 1,
        _ => 2,
    });
    o.f32(calc_evaluate(resolved.as_deref(), Px(300.0), None).value());
}

/// `p`'s answer.
pub fn pixel(o: &mut Out, a: Px, b: Px, k: i32) {
    let flags = u8::from(a == b)
        | u8::from(a != b) << 1
        | u8::from(a < b) << 2
        | u8::from(a > b) << 3
        | u8::from(a <= b) << 4
        | u8::from(a >= b) << 5;
    o.u8(flags);
    for v in [a.max(b), a.min(b), a + b, a - b, a * b, a / b, -a, a + k, a - k, a * k, a / k, k + a, k - a, k * a, k / a] {
        o.f32(v.value());
    }
    let mut c = a;
    c += b;
    o.f32(c.value());
    c -= k;
    o.f32(c.value());
    c *= b;
    o.f32(c.value());
    c /= k;
    o.f32(c.value());
    o.f32(c.inc().value());
    let before = c;
    c.dec();
    o.f32(before.value());
    o.f32(c.value());
    o.i32(a.to_int());
    o.i32(html::round_f(a.value()));
    o.i32(html::round_d(f64::from(a.value())));
    o.f32(Px::from_int(k).value());
    o.f32(clamp_length(a.value()));
}

/// `u`'s answer.
pub fn utf8(o: &mut Out, text: &[u8]) {
    use twidget_litehtml::utf8_strings::*;
    let mut reads = Vec::new();
    let mut index = 0;
    loop {
        let before = index;
        let ch = read_utf8_char(text, &mut index);
        reads.push((ch, index as u32));
        if index == before {
            break;
        }
    }
    o.u32(reads.len() as u32);
    for (ch, index) in reads {
        o.u32(ch).u32(index);
    }
    let mut backs = Vec::new();
    let mut index = text.len();
    while index > 0 {
        prev_utf8_char(text, &mut index);
        backs.push(index as u32);
    }
    o.u32(backs.len() as u32);
    for index in backs {
        o.u32(index);
    }
    o.u8(u8::from(is_valid_utf8(text)));
    let wide = utf8_to_utf32(text);
    o.u32(wide.len() as u32);
    for &ch in &wide {
        o.u32(ch);
    }
    o.str(&utf32_to_utf8(&wide));
}

/// `m`'s answer.
pub fn helpers(o: &mut Out, a: &[u8], b: &[u8]) {
    use twidget_litehtml::html::*;
    o.str(&trim(a, WHITESPACE));
    o.str(&trim(a, b));
    o.str(&lowcase(a));
    // The C++ hands c_str()s over: the strings end at their first NUL.
    let ca = &a[..a.iter().position(|c| *c == 0).unwrap_or(a.len())];
    let cb = &b[..b.iter().position(|c| *c == 0).unwrap_or(b.len())];
    o.i32(t_strcasecmp(ca, cb));
    for n in [0, 1, 3, a.len(), b.len()] {
        o.i32(t_strncasecmp(ca, cb, n));
    }
    o.u8(u8::from(equal_i(a, b)));
    o.u8(u8::from(is_number(a, true)));
    o.u8(u8::from(is_number(a, false)));
    o.str(&get_escaped_string(a));
    for (open, close) in [(b'(', b')'), (b'[', b']'), (b'{', b'}')] {
        o.i32(find_close_bracket(a, 0, open, close).map_or(-1, |at| at as i32));
    }
    for index in [0, 1.min(a.len() as i32), -1, -2, a.len() as i32] {
        o.u8(u8::from(match_at(a, index, b)));
        o.u8(u8::from(match_i(a, index, b)));
    }
    o.str(&join_string(&[a.to_vec(), b.to_vec(), a.to_vec()], b));
}

pub fn tokens(o: &mut Out, list: &[CssToken]) {
    o.u32(list.len() as u32);
    for t in list {
        o.token(t);
    }
}

fn media_keep(left: &CssToken, right: &CssToken) -> bool {
    (left.ch() == i32::from(b'<') || left.ch() == i32::from(b'>')) && right.ch() == i32::from(b'=')
}

/// lhdump's `keeper`: no keep_whitespace, or media queries'.
pub fn keeper(mode: u8) -> Option<css_parser::KeepWhitespace<'static>> {
    if mode == 0 { None } else { Some(&media_keep) }
}

/// The Rust side of one lhdump process: the name table its cases share.
#[derive(Default)]
pub struct Lh {
    pub names: Names,
}

impl Lh {
    pub fn new() -> Lh {
        Lh::default()
    }

    /// What lhdump answers to record `op` with `input`.
    pub fn answer(&self, op: u8, input: &[u8]) -> (u8, Vec<u8>) {
        use twidget_litehtml::web_color::*;
        let mut i = In::new(input);
        let mut o = Out::default();
        let names = &self.names;
        match op {
            b'd' => {
                let (v, end) = strtod::t_strtod(input);
                o.f64(v).u32(end as u32).f32(html::t_strtof(input).0);
                (1, o.take())
            }
            b'c' => {
                let v = convert_string_to_number(input);
                o.f64(v).f32(v as f32);
                (1, o.take())
            }
            b'p' => {
                let a = Px(i.f32());
                let b = Px(i.f32());
                let k = i.i32();
                pixel(&mut o, a, b, k);
                (1, o.take())
            }
            b'n' => {
                let steps = i.u32();
                for _ in 0..steps {
                    match i.u8() {
                        b'i' => {
                            o.u32(names.id(&i.str()).0);
                        }
                        b'f' => {
                            let found = names.find_id(&i.str());
                            o.u8(u8::from(found.is_some())).u32(found.map_or(0, |id| id.0));
                        }
                        b's' => {
                            o.str(&names.s(StringId(i.u32())));
                        }
                        b'u' => {
                            let usage = names.usage();
                            o.u64(usage.names as u64).u64(usage.bytes as u64);
                        }
                        other => panic!("no name step {other}"),
                    }
                }
                (1, o.take())
            }
            b'N' => {
                let usage = names.usage();
                o.u32(usage.names as u32);
                for id in 0..usage.names {
                    o.str(&names.s(StringId(id as u32)));
                }
                o.u32(string_id::empty_id.0).u32(string_id::star_id.0);
                (1, o.take())
            }
            b't' => {
                o.u32(TABLES.len() as u32);
                for (name, entries) in TABLES {
                    o.str(name.as_bytes()).u32(entries.len() as u32);
                    for e in entries {
                        o.str(e.as_bytes());
                    }
                }
                for v in border_width_values {
                    o.f32(v);
                }
                for v in background_position_percentages {
                    o.f32(v);
                }
                (1, o.take())
            }
            b'v' => {
                let values = keywords(i.u8());
                let index = i.u32();
                let text = i.rest();
                let found = values.value_index(&text);
                o.u8(u8::from(found.is_some())).i32(found.unwrap_or(0));
                o.str(values.value_by_index(index as usize).as_bytes());
                o.u8(u8::from(values.has(&text)));
                (1, o.take())
            }
            b'C' => {
                let which = i.u8();
                let tok = i.token(0);
                let color = match which {
                    0 => parse_hash_color(&tok),
                    1 => parse_rgb_func(names, &tok),
                    2 => parse_hsl_func(names, &tok),
                    _ => parse_func_color(names, &tok),
                };
                o.color(&color.unwrap_or(WebColor::rgba(1, 2, 3, 4)));
                (u8::from(color.is_some()), o.take())
            }
            b'H' => {
                let (r, g, b) = hsl_to_rgb(i.f32(), i.f32(), i.f32());
                o.f32(r).f32(g).f32(b);
                (1, o.take())
            }
            b'q' => {
                let (x, lo, hi) = (i.f32(), i.f32(), i.f32());
                o.f32(clamp(x, lo, hi));
                let value = i.f32();
                let units = CssUnits::from_index(i32::from(i.u8())).expect("a unit");
                o.u8(calc_percent_and_clamp(&CssLength::new(value, units), i.f32()));
                (1, o.take())
            }
            b'D' => {
                let c = WebColor::rgba(i.u8(), i.u8(), i.u8(), i.u8());
                o.color(&c.darken(i.f64()));
                (1, o.take())
            }
            b'S' => {
                let c = WebColor::rgba(i.u8(), i.u8(), i.u8(), i.u8());
                (1, c.to_string())
            }
            b'R' => (1, resolve_name(input, None)),
            b'L' => {
                let start = i.u8();
                let options = i.i32();
                let words = keywords(i.u8());
                let tok = i.token(0);
                let mut len = CssLength::default();
                if start == 1 {
                    len.set_value(2.5, CssUnits::css_units_em);
                }
                if start == 2 {
                    len.set_predef(7);
                }
                if start == 3 {
                    len.from_token(&start_calc(), f_length_percentage, CssValues::default());
                }
                let ok = len.from_token(&tok, options, words);
                length(&mut o, &len);
                (u8::from(ok), o.take())
            }
            b'A' => {
                let percents = i.u8() != 0;
                let tok = i.token(0);
                let angle = gradient::parse_angle(names, &tok, percents);
                o.f32(angle.unwrap_or(-7.0));
                (u8::from(angle.is_some()), o.take())
            }
            b'k' => {
                let style = i.u8();
                let value = i.i32();
                let text = match style {
                    0 => num_cvt::to_latin_lower(value),
                    1 => num_cvt::to_latin_upper(value),
                    2 => num_cvt::to_greek_lower(value),
                    3 => num_cvt::to_roman_lower(value),
                    _ => num_cvt::to_roman_upper(value),
                };
                (1, text)
            }
            b'u' => {
                utf8(&mut o, input);
                (1, o.take())
            }
            b'a' => {
                let mut out = Vec::new();
                while i.at + 4 <= input.len() {
                    utf8_strings::append_char(&mut out, i.u32());
                }
                (1, out)
            }
            b's' => {
                let (delims, preserve, quote) = (i.str(), i.str(), i.str());
                let tokens = html::split_string(&i.rest(), &delims, &preserve, &quote);
                o.u32(tokens.len() as u32);
                for t in &tokens {
                    o.str(t);
                }
                (1, o.take())
            }
            b'm' => {
                let (a, b) = (i.str(), i.str());
                helpers(&mut o, &a, &b);
                (1, o.take())
            }
            b'T' => {
                tokens(&mut o, &tokenize(input));
                (1, o.take())
            }
            b'Z' => {
                let options = i32::from(i.u8());
                let keep = i.u8();
                let list = normalize(&i.rest(), options, keeper(keep));
                tokens(&mut o, &list);
                o.str(&get_repr(&list, 0, -1, false)).str(&get_repr(&list, 0, -1, true));
                (1, o.take())
            }
            b'F' => (1, css_parser::filter_code_points(input)),
            b'K' => {
                let list = normalize(input, f_componentize | f_remove_whitespace, None);
                o.u32(list.len() as u32);
                let color = if list.len() == 1 { parse_color(names, &list[0], None) } else { None };
                o.color(&color.unwrap_or(WebColor::rgba(1, 2, 3, 4)));
                (u8::from(color.is_some()), o.take())
            }
            b'G' => {
                let options = i.i32();
                let words = keywords(i.u8());
                let list = normalize(&i.rest(), f_componentize | f_remove_whitespace, None);
                o.u32(list.len() as u32);
                let mut len = CssLength::default();
                let ok = list.len() == 1 && len.from_token(&list[0], options, words);
                length(&mut o, &len);
                (u8::from(ok), o.take())
            }
            b'P' => {
                let list = normalize(input, f_componentize, None);
                let lists = css_parser::parse_comma_separated_list(&list);
                o.u32(lists.len() as u32);
                for l in &lists {
                    tokens(&mut o, l);
                }
                o.u8(u8::from(css_parser::is_any_value(&list))).u8(u8::from(css_parser::is_declaration_value(&list, 0)));
                let mut index = 0;
                o.u8(u8::from(css_parser::skip_whitespace(&list, &mut index))).i32(index as i32);
                for t in &list {
                    o.str(&t.ident());
                }
                (1, o.take())
            }
            b'M' => (1, master_css()),
            b'x' => {
                for c in 0..=255u8 {
                    o.u8(u8::from(codepoint::is_url_reserved_codepoint(c))
                        | u8::from(codepoint::is_url_scheme_codepoint(c)) << 1
                        | u8::from(codepoint::is_ascii_codepoint(c)) << 2);
                }
                (1, o.take())
            }
            other => panic!("no lhdump record {}", char::from(other)),
        }
    }

    /// Adds a record to `c` with the answer this side gives it.
    pub fn case(&self, c: &mut Comparison, op: u8, input: &[u8], what: impl Into<String>) {
        c.case(&framed(op, input), self.answer(op, input), what);
    }
}

// ---- Generators ---------------------------------------------------------------------------------

pub fn pick<'a, T: ?Sized>(random: &mut Lcg, from: &[&'a T]) -> &'a T {
    from[random.below(from.len())]
}

/// Number texts of every shape both number readers meet, and past them.
pub fn any_number_text(random: &mut Lcg) -> Vec<u8> {
    let mut s = Vec::new();
    match random.below(10) {
        0 => {
            return pick(
                random,
                &[
                    "",
                    ".",
                    "-",
                    "+",
                    "-.",
                    "e5",
                    ".e5",
                    "1e",
                    "1e+",
                    "1E-",
                    "-0",
                    "+0",
                    "0e999",
                    "-0e999",
                    "1e400",
                    "1e-400",
                    "1e308",
                    "1e-308",
                    "4.9e-324",
                    "2.5e-3",
                    "1e2147483648",
                    "1e-2147483649",
                    "00000000000000000000001",
                    "0.0000000000000000000001",
                    "123456789012345678901234567890",
                    "1234567890.1234567890123",
                    "9007199254740993",
                    "0.1",
                    "0.2",
                    "0.3",
                    "1.5e+38",
                    "3.4028236e38",
                    "3.4028235e38",
                    "1e39",
                    "-1e39",
                    "1.17549435e-38",
                    "1e-45",
                    "1e-46",
                    "\u{b}5",
                    " \t\n5",
                    "\u{a0}5",
                    "5\0",
                    "\x005",
                    "1.2.3",
                    "1..2",
                    "1e1e1",
                    "+-1",
                    "-+1",
                    "1e+-1",
                    "١",
                    "0x10",
                    "inf",
                    "nan",
                ],
            )
            .as_bytes()
            .to_vec();
        }
        1 => s.extend_from_slice(format!("{}", random.next() as f64 / f64::from(1 + random.below(1000) as u32)).as_bytes()),
        2 => s.extend_from_slice(format!("{:e}", f64::from_bits(u64::from(random.next()) << 32 | u64::from(random.next()))).as_bytes()),
        _ => {
            if random.chance(4) {
                s.extend_from_slice(pick(random, &[" ", "\t", "\n", "\x0b", "\x0c", "\r"]).as_bytes());
            }
            if random.chance(3) {
                s.push(if random.chance(2) { b'-' } else { b'+' });
            }
            let digits = |random: &mut Lcg, s: &mut Vec<u8>, most: usize| {
                let n = if random.chance(8) { 15 + random.below(30) } else { random.below(most) };
                for _ in 0..n {
                    s.push(b'0' + random.below(10) as u8);
                }
            };
            digits(random, &mut s, 6);
            if random.chance(2) {
                s.push(b'.');
                digits(random, &mut s, 6);
            }
            if random.chance(3) {
                s.push(if random.chance(2) { b'e' } else { b'E' });
                if random.chance(2) {
                    s.push(if random.chance(2) { b'-' } else { b'+' });
                }
                if random.chance(6) {
                    s.extend_from_slice(format!("{}", random.next()).as_bytes());
                } else {
                    digits(random, &mut s, 4);
                }
            }
        }
    }
    if random.chance(5) {
        s.extend_from_slice(pick(random, &["px", "%", " ", "e", ".", "\0", "\u{e9}", "-", "+1", "x", ".5", "e+"]).as_bytes());
    }
    s
}

/// A float from the values lengths, colours and angles meet, and their edges.
pub fn any_f32(random: &mut Lcg) -> f32 {
    match random.below(9) {
        8 => f32::from_bits([0xffc0_0000, 0xffc0_0001, 0x7fc0_0001, 0x7f80_0001, 0xff80_0001, 0x7fff_ffff][random.below(6)]),
        0 => *pick(
            random,
            &[
                &0.0f32,
                &-0.0,
                &1.0,
                &-1.0,
                &0.5,
                &255.0,
                &256.0,
                &100.0,
                &50.0,
                &360.0,
                &-360.0,
                &400.0,
                &1e-5,
                &5e-5,
                &1e-4,
                &2e-4,
                &1e30,
                &-1e30,
                &f32::INFINITY,
                &f32::NEG_INFINITY,
                &f32::NAN,
                &f32::MAX,
                &f32::MIN_POSITIVE,
                &33554432.0,
                &3e9,
                &-3e9,
                &2147483520.0,
                &2147483648.0,
                &-2147483648.0,
                &0.49999997,
                &1.5,
                &-1.5,
                &2.5,
                &-0.5,
            ],
        ),
        1 => f32::from_bits(random.next() << 1 | (random.next() & 1)),
        2 => random.below(2000) as f32 / 8.0 - 125.0,
        3 => (random.next() as f32 - 1073741824.0) / 1e3,
        4 => random.below(101) as f32,
        5 => random.below(1000) as f32 / 1000.0,
        _ => random.below(720) as f32 - 360.0,
    }
}

pub fn number_token(random: &mut Lcg, ty: CssTokenType) -> CssToken {
    let number_type = if random.chance(2) { CssNumberType::css_number_integer } else { CssNumberType::css_number_number };
    let mut t = CssToken::with_number(ty, any_f32(random), number_type);
    t.repr = b"1".to_vec();
    t
}

pub fn any_unit(random: &mut Lcg) -> &'static [u8] {
    pick(
        random,
        &[
            "px", "em", "rem", "%", "deg", "DEG", "grad", "rad", "turn", "Turn", "in", "cm", "mm", "ex", "pt", "pc", "vw", "vh", "vmin",
            "vmax", "ch", "none", "-lh-calc", "", "s", "ms", "x", "fr", "dppx", "PX", "\u{e9}m", "q",
        ],
    )
    .as_bytes()
}

fn ident(random: &mut Lcg) -> CssToken {
    let words = [
        "none",
        "None",
        "NONE",
        "auto",
        "normal",
        "inherit",
        "currentcolor",
        "currentColor",
        "red",
        "transparent",
        "block",
        "inline",
        "thin",
        "medium",
        "thick",
        "solid",
        "--var",
        "--Var",
        "left",
        "center",
        "small",
        "larger",
        "nowrap",
        "flex",
        "grid",
        "x",
        "",
    ];
    let mut t = CssToken::with_str(IDENT, pick(random, &words).as_bytes());
    t.repr = t.str.clone();
    t
}

fn delim(c: u8) -> CssToken {
    let mut t = CssToken::new(i32::from(c));
    t.repr = vec![c];
    t
}

fn whitespace() -> CssToken {
    let mut t = CssToken::new(WHITESPACE);
    t.repr = b" ".to_vec();
    t
}

pub fn dimension(random: &mut Lcg) -> CssToken {
    let mut t = number_token(random, DIMENSION);
    t.str = any_unit(random).to_vec();
    t
}

/// A value of a colour or math function: numbers, percentages, angles, `none`, `/` and commas, with
/// whitespace sometimes, in the shapes the functions take and near them.
fn function_value(random: &mut Lcg, depth: u32) -> Vec<CssToken> {
    let mut out = Vec::new();
    let n = if random.chance(6) { random.below(9) } else { [1, 3, 3, 4, 5][random.below(5)] };
    let comma = random.chance(2);
    for k in 0..n {
        if k > 0 {
            if comma {
                out.push(delim(b','));
            } else if k == 3 && random.chance(2) {
                out.push(delim(b'/'));
            }
            if random.chance(5) {
                out.push(whitespace());
            }
        }
        out.push(match random.below(10) {
            0..=3 => number_token(random, NUMBER),
            4 | 5 => number_token(random, PERCENTAGE),
            6 => dimension(random),
            7 => ident(random),
            8 if depth < 4 => any_token(random, depth + 1),
            _ => delim(*pick(random, &[&b'/', &b',', &b'*', &b'+', &b'-'])),
        });
    }
    out
}

/// A calc expression's value: atoms, operators, whitespace, nested blocks and functions.
fn calc_value(random: &mut Lcg, depth: u32) -> Vec<CssToken> {
    let mut out = Vec::new();
    let most = if random.chance(4) { 12 } else { 4 };
    let n = 1 + random.below(most);
    for k in 0..n {
        if k > 0 {
            if random.chance(2) {
                out.push(whitespace());
            }
            if !random.chance(8) {
                out.push(delim(*pick(random, &[&b'+', &b'-', &b'*', &b'/', &b','])));
            }
            if random.chance(2) {
                out.push(whitespace());
            }
        }
        out.push(match random.below(9) {
            0 | 1 => number_token(random, NUMBER),
            2 => number_token(random, PERCENTAGE),
            3 | 4 => dimension(random),
            5 if depth < 6 => {
                let mut b = CssToken::new(ROUND_BLOCK);
                b.value = calc_value(random, depth + 1);
                b
            }
            6 if depth < 6 => calc_function(random, depth + 1),
            7 => ident(random),
            _ => number_token(random, NUMBER),
        });
    }
    out
}

/// A math function as calc's grammar has it: sums of products, `*` and `/` by numbers, nested blocks,
/// `calc()`, `min()`, `max()` and `clamp()`, whitespace around the operators or not.
pub fn math_function(random: &mut Lcg, depth: u32) -> CssToken {
    fn atom(random: &mut Lcg, depth: u32) -> CssToken {
        match random.below(if depth < 5 { 9 } else { 6 }) {
            0 | 1 => {
                let mut d = dimension(random);
                d.str = pick(random, &["px", "em", "rem", "vw", "vh", "pt", "in", "ch", "ex", "vmin", "PX"]).as_bytes().to_vec();
                if random.chance(2) {
                    d.n.number = random.below(200) as f32 / 4.0 - 10.0;
                }
                d
            }
            2 | 3 => {
                let mut p = number_token(random, PERCENTAGE);
                if random.chance(2) {
                    p.n.number = random.below(150) as f32 - 25.0;
                }
                p
            }
            4 | 5 => {
                let mut n = number_token(random, NUMBER);
                if random.chance(2) {
                    n.n.number = random.below(9) as f32 - 2.0;
                }
                n
            }
            6 => {
                let mut b = CssToken::new(ROUND_BLOCK);
                b.value = sum(random, depth + 1);
                b
            }
            7 => {
                let mut f = CssToken::with_str(CV_FUNCTION, b"calc");
                f.value = sum(random, depth + 1);
                f
            }
            _ => {
                let name = pick(random, &["min", "max", "clamp", "MIN"]);
                let mut f = CssToken::with_str(CV_FUNCTION, name.as_bytes());
                let n = if name == "clamp" { 3 } else { 1 + random.below(4) };
                for k in 0..n {
                    if k > 0 {
                        f.value.push(delim(b','));
                        if random.chance(2) {
                            f.value.push(whitespace());
                        }
                    }
                    f.value.extend(sum(random, depth + 1));
                }
                f
            }
        }
    }
    fn op(random: &mut Lcg, out: &mut Vec<CssToken>, c: u8) {
        let spaced = random.chance(2);
        if spaced {
            out.push(whitespace());
        }
        out.push(delim(c));
        if spaced {
            out.push(whitespace());
        }
    }
    fn sum(random: &mut Lcg, depth: u32) -> Vec<CssToken> {
        let mut out = Vec::new();
        for k in 0..1 + random.below(3) {
            if k > 0 {
                let c = if random.chance(2) { b'+' } else { b'-' };
                op(random, &mut out, c);
            }
            out.push(atom(random, depth));
            for _ in 0..random.below(2) {
                let c = if random.chance(2) { b'*' } else { b'/' };
                op(random, &mut out, c);
                let mut n = number_token(random, NUMBER);
                n.n.number = [2.0, 0.5, 3.0, -1.0, 0.0, 1e30][random.below(6)];
                out.push(if random.chance(10) { atom(random, depth) } else { n });
            }
        }
        out
    }
    let name = pick(random, &["calc", "min", "max", "clamp", "Calc"]);
    let mut f = CssToken::with_str(CV_FUNCTION, name.as_bytes());
    let n = match name {
        "clamp" => 3,
        "min" | "max" => 1 + random.below(3),
        _ => 1,
    };
    for k in 0..n {
        if k > 0 {
            f.value.push(delim(b','));
        }
        f.value.extend(sum(random, depth));
    }
    f
}

pub fn calc_function(random: &mut Lcg, depth: u32) -> CssToken {
    let name = pick(random, &["calc", "CALC", "min", "max", "clamp", "Clamp", "calc", "var", "env"]);
    let mut t = CssToken::with_str(CV_FUNCTION, name.as_bytes());
    t.value = calc_value(random, depth);
    t
}

/// A HASH of the lengths and digits a hex colour has, and others.
pub fn hash_token(random: &mut Lcg) -> CssToken {
    let len = if random.chance(3) { random.below(10) } else { [3, 4, 6, 8][random.below(4)] };
    let digits = b"0123456789abcdefABCDEFgxz-\xc3\xa9";
    let mut t = CssToken::new(HASH);
    t.hash_type = if random.chance(2) { CssHashType::css_hash_id } else { CssHashType::css_hash_unrestricted };
    t.str = (0..len).map(|_| if random.chance(12) { digits[random.below(digits.len())] } else { digits[random.below(22)] }).collect();
    t
}

/// `rgb()`, `rgba()`, `hsl()` or `hsla()` in the legacy or the modern syntax as the parser leaves them
/// (componentized, whitespace removed), now and then a part wrong.
pub fn color_function(random: &mut Lcg, hsl: bool) -> CssToken {
    let name = if hsl { pick(random, &["hsl", "hsla", "HSL", "Hsla"]) } else { pick(random, &["rgb", "rgba", "RGB", "rgbA"]) };
    let mut t = CssToken::with_str(CV_FUNCTION, name.as_bytes());
    let value = |random: &mut Lcg, kind: usize| -> CssToken {
        match kind {
            0 => {
                let mut n = number_token(random, NUMBER);
                if random.chance(2) {
                    n.n.number = random.below(300) as f32 - 20.0 + if random.chance(3) { 0.5 } else { 0.0 };
                }
                n
            }
            1 => {
                let mut n = number_token(random, PERCENTAGE);
                if random.chance(2) {
                    n.n.number = random.below(130) as f32 - 10.0;
                }
                n
            }
            2 => {
                let mut d = dimension(random);
                d.str = pick(random, &["deg", "grad", "rad", "turn", "DEG", "px"]).as_bytes().to_vec();
                d
            }
            _ => {
                let mut i = CssToken::with_str(IDENT, pick(random, &["none", "NONE", "auto"]).as_bytes());
                i.repr = i.str.clone();
                i
            }
        }
    };
    let legacy = random.chance(2);
    let count = if random.chance(2) { 3 } else { 4 };
    let percentages = random.chance(2);
    for k in 0..count {
        if k > 0 {
            if legacy {
                t.value.push(delim(b','));
            } else if k == 3 {
                t.value.push(delim(if random.chance(10) { b'*' } else { b'/' }));
            }
        }
        let kind = if random.chance(10) {
            random.below(4)
        } else if !legacy && random.chance(8) {
            3
        } else if k == 0 && hsl {
            if random.chance(2) { 0 } else { 2 }
        } else if k == 3 {
            random.below(2)
        } else if hsl {
            1
        } else {
            usize::from(percentages)
        };
        t.value.push(value(random, kind));
    }
    if random.chance(12) {
        t.value.push(whitespace());
    }
    t
}

/// A token of the kinds colours, lengths and angles read, or a component value made of them.
pub fn any_token(random: &mut Lcg, depth: u32) -> CssToken {
    match random.below(12) {
        0 => number_token(random, NUMBER),
        1 => number_token(random, PERCENTAGE),
        2 => dimension(random),
        3 => ident(random),
        4 => hash_token(random),
        5 if depth < 3 => {
            let hsl = random.chance(2);
            color_function(random, hsl)
        }
        6 => {
            let name = pick(random, &["rgb", "rgba", "RGB", "Rgba", "hsl", "hsla", "HSL", "hwb", "rgbx", "color"]);
            let mut t = CssToken::with_str(CV_FUNCTION, name.as_bytes());
            t.value = function_value(random, depth);
            t
        }
        7 | 8 => calc_function(random, depth),
        9 => {
            let mut b = CssToken::new(*pick(random, &[&ROUND_BLOCK, &SQUARE_BLOCK, &CURLY_BLOCK]));
            b.value = calc_value(random, depth + 1);
            b
        }
        10 => delim(*pick(random, &[&b'/', &b',', &b'*', &b'+', &b'-', &b'#'])),
        _ => {
            let mut t = CssToken::with_str(*pick(random, &[&STRING, &URL, &FUNCTION, &AT_KEYWORD, &BAD_URL, &CDO]), b"x");
            t.repr = b"x".to_vec();
            t
        }
    }
}

/// Text for the string helpers: delimiters, quotes, brackets, case and bytes past ASCII.
pub fn any_text(random: &mut Lcg) -> Vec<u8> {
    let pieces = [
        "a",
        "B",
        "zz",
        " ",
        "\t",
        "\n",
        "\r",
        "\x0c",
        ",",
        ";",
        "\"",
        "'",
        "(",
        ")",
        "[",
        "]",
        "{",
        "}",
        "\\",
        "?",
        "\x07",
        "\x08",
        "\x0b",
        "\0",
        "\u{e9}",
        "\u{fffd}",
        "\u{1f600}",
        "1",
        "2.5",
        ".",
        "-",
        "url(",
        "Red",
        "rgb(1, 2, 3)",
        "AliceBlue",
        "aliceblue",
        "--x",
        "_",
    ];
    let n = random.below(12);
    let mut out = Vec::new();
    for _ in 0..n {
        out.extend_from_slice(pick(random, &pieces).as_bytes());
    }
    out
}

/// Bytes for the UTF-8 readers: well-formed text, broken sequences, NULs and lone bytes.
pub fn any_utf8(random: &mut Lcg) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..random.below(10) {
        match random.below(8) {
            0 => out.push(random.next() as u8),
            1 => out.extend_from_slice(pick(
                random,
                &[&b"\xc0\x80"[..], b"\xe0\x80\x80", b"\xf0\x80\x80\x80", b"\xed\xa0\x80", b"\xf4\x90\x80\x80"],
            )),
            2 => out.push(0x80 | random.below(64) as u8),
            3 => out.push(0xc0 | random.below(64) as u8),
            4 => {
                let c = char::from_u32(random.below(0x110000) as u32).unwrap_or('\u{fffd}');
                let mut b = [0; 4];
                out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
            }
            5 => out.push(0),
            _ => out.push(b'a' + random.below(26) as u8),
        }
    }
    out
}

/// CSS text for the tokenizer: every token kind, their edges and their broken forms (comments and
/// strings left open, escapes of every length, bad urls, NULs, `\r`, `\f`, bytes that are no UTF-8), and
/// nesting past the parser's limit now and then.
pub fn any_css(random: &mut Lcg) -> Vec<u8> {
    let pieces: &[&str] = &[
        " ",
        "  ",
        "\t",
        "\n",
        "\r\n",
        "\r",
        "\x0c",
        "/* c */",
        "/*",
        "*/",
        "/**/",
        "a",
        "div",
        "-x",
        "--v",
        "-",
        "--",
        "->",
        "-->",
        "<!--",
        "<",
        "<!-",
        "#",
        "#a1",
        "#1a",
        "#-",
        "#\\41",
        "#\\\n",
        "@",
        "@media",
        "@-x",
        "@1",
        "\"s\"",
        "'s'",
        "\"a\\\"b\"",
        "\"a\\\nb\"",
        "\"open",
        "'x\ny'",
        "\"\\",
        "url(",
        "url(a)",
        "url( a b )",
        "url(a\"b)",
        "URL(  'q')",
        "url( \"q\")",
        "url(a\\)b)",
        "url(a\x01)",
        "url(\\\n)",
        "url(a )",
        "url(a",
        "u",
        "rl(",
        "(",
        ")",
        "[",
        "]",
        "{",
        "}",
        ":",
        ";",
        ",",
        "!",
        ".",
        "&",
        "+",
        "*",
        "/",
        "%",
        "=",
        ">",
        "~",
        "|",
        "^",
        "$",
        "?",
        "\\",
        "\\41",
        "\\41 ",
        "\\0",
        "\\d800",
        "\\110000",
        "\\FFFFFF",
        "\\1F600x",
        "\\\n",
        "\\g",
        "1",
        "12",
        "1.5",
        ".5",
        "1.",
        "+1",
        "-1",
        "+.5",
        "-.5",
        "1e3",
        "1E+3",
        "1e-3",
        "1e",
        "1e+",
        "2px",
        "2PX",
        "3%",
        "4em",
        "1e3px",
        "5\\",
        "6-x",
        "7--",
        "8_",
        "0e999",
        "1e999",
        "123456789012345678901234567890",
        "\0",
        "\u{e9}",
        "\u{fffd}",
        "\u{1f600}",
        "\u{a0}",
        "\u{7f}",
        "\x01",
        "\x0b",
        "calc(",
        "rgb(",
        "hsl(",
        "var(--x)",
        "-webkit-",
        "!important",
        "u+0-7f",
    ];
    let mut out = Vec::new();
    for _ in 0..random.below(24) {
        match random.below(30) {
            0 => out.push(random.next() as u8),
            1 => out.extend_from_slice(pick(
                random,
                &[&b"\xc0\x80"[..], b"\xe2\x82", b"\xf0\x9f", b"\xff", b"\x80", b"\xed\xa0\x80", b"\xf8\x88\x80\x80\x80"],
            )),
            2 => {
                let depth = if random.chance(4) { 120 + random.below(20) } else { random.below(8) };
                let open: &str = pick(random, &["(", "[", "{", "f("]);
                for _ in 0..depth {
                    out.extend_from_slice(open.as_bytes());
                }
                out.push(b'x');
                if random.chance(2) {
                    let close: &[u8] = match open {
                        "[" => b"]",
                        "{" => b"}",
                        _ => b")",
                    };
                    for _ in 0..depth {
                        out.extend_from_slice(close);
                    }
                }
            }
            _ => out.extend_from_slice(pick(random, pieces).as_bytes()),
        }
    }
    out
}

// ---- The corpus ---------------------------------------------------------------------------------

pub fn twidget() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

pub fn widget_sandbox() -> PathBuf {
    std::env::var_os("TWIDGET_WIDGET_SANDBOX")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../widget-sandbox"))
}

fn html_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            html_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "html") {
            out.push(path);
        }
    }
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    (from..=hay.len().checked_sub(needle.len())?).find(|&i| &hay[i..i + needle.len()] == needle)
}

fn between<'a>(text: &'a [u8], open: &[u8], close: &[u8], out: &mut Vec<&'a [u8]>) {
    let mut i = 0;
    while let Some(a) = find(text, open, i) {
        let start = a + open.len();
        let Some(b) = find(text, close, start) else { break };
        out.push(&text[start..b]);
        i = b + close.len();
    }
}

/// The style text of every widget in widget-sandbox's corpus (`corpus/**/*.html`): its `<style>` blocks
/// and its `style="…"` and `style='…'` attributes.
pub fn corpus_styles() -> Vec<Vec<u8>> {
    let mut files = Vec::new();
    html_files(&widget_sandbox().join("corpus"), &mut files);
    let mut out = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read(&path) else { continue };
        let mut found = Vec::new();
        let mut blocks = Vec::new();
        between(&text, b"<style", b"</style>", &mut blocks);
        for block in blocks {
            if let Some(end) = block.iter().position(|c| *c == b'>') {
                found.push(&block[end + 1..]);
            }
        }
        between(&text, b"style=\"", b"\"", &mut found);
        between(&text, b"style='", b"'", &mut found);
        out.extend(found.into_iter().map(<[u8]>::to_vec));
    }
    out
}

/// The text of a C++ raw string literal `R"delim(…)delim"` that follows `marker` in a file of twidget's.
fn raw_string(path: &Path, marker: &str) -> Vec<u8> {
    let text = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let at = find(&text, marker.as_bytes(), 0).unwrap_or_else(|| panic!("{marker} in {}", path.display()));
    let open = find(&text, b"R\"", at).expect("a raw string") + 2;
    let paren = find(&text, b"(", open).expect("its delimiter");
    let close = [b")", &text[open..paren], b"\""].concat();
    let end = find(&text, &close, paren).expect("its end");
    // The compiler reads the file's line ends as one newline each (a checkout may have CRLF).
    let mut out = Vec::with_capacity(end - paren);
    for (i, &c) in text[paren + 1..end].iter().enumerate() {
        if !(c == b'\r' && text.get(paren + 2 + i) == Some(&b'\n')) {
            out.push(c);
        }
    }
    out
}

/// litehtml's `master_css` (litehtml/include/litehtml/master_css.h).
pub fn master_css() -> Vec<u8> {
    raw_string(&twidget().join("litehtml/include/litehtml/master_css.h"), "master_css")
}

/// The renderer's UA rules before the profile's (core/src/renderer.cpp, `UA_EXTRA`).
pub fn ua_extra() -> Vec<u8> {
    raw_string(&twidget().join("core/src/renderer.cpp"), "UA_EXTRA = ")
}

/// The profile's UA layer (`tables.uaStylesheet` of widget-sandbox's packages/core/profile.json), its `var()`s
/// as they are.
pub fn ua_stylesheet() -> Vec<u8> {
    let text = std::fs::read(widget_sandbox().join("packages/core/profile.json")).expect("profile.json");
    let json = twidget_json::Json::parse(&text).expect("profile.json parses");
    let sheet = json["tables"]["uaStylesheet"].as_str().to_vec();
    assert!(!sheet.is_empty(), "tables.uaStylesheet");
    sheet
}

/// The declarations of a style text, split as plainly as a test may: at `;`, `{` and `}` and the first `:`
/// outside parentheses and quotes, trimmed.
pub fn declarations(style: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut depth = 0i32;
    let mut quote = 0u8;
    let mut start = 0;
    let mut pieces = Vec::new();
    for (i, &c) in style.iter().enumerate() {
        if quote != 0 {
            if c == quote {
                quote = 0;
            }
            continue;
        }
        match c {
            b'"' | b'\'' => quote = c,
            b'(' => depth += 1,
            b')' => depth -= 1,
            b';' | b'{' | b'}' if depth <= 0 => {
                pieces.push(&style[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    pieces.push(&style[start..]);
    let trim = |b: &[u8]| twidget_litehtml::html::trim(b, twidget_litehtml::html::WHITESPACE);
    pieces
        .into_iter()
        .filter_map(|piece| piece.iter().position(|c| *c == b':').map(|colon| (trim(&piece[..colon]), trim(&piece[colon + 1..]))))
        .collect()
}

/// The text with every lead byte that reads as an ASCII character (an overlong form such as `C0 B0` for `0`)
/// made 0xFF, and whether there was one. A token that starts with such a digit, or with `+`, `-` or `.`
/// before one, is a number the tokenizer reads byte by byte: it takes nothing, and the fork's tokenizer, as
/// its port, makes empty tokens forever (`.` E0 80 B0 never returns). The comparison keeps them out.
pub fn without_overlong_ascii(text: &[u8]) -> (Vec<u8>, bool) {
    let mut out = text.to_vec();
    let mut changed = false;
    for p in 0..out.len() {
        if (0xC0..0xF8).contains(&out[p]) {
            let mut index = p;
            let ch = twidget_litehtml::utf8_strings::read_utf8_char(&out, &mut index);
            if (1..0x80).contains(&ch) {
                out[p] = 0xFF;
                changed = true;
            }
        }
    }
    (out, changed)
}

// ---- Stage L2: stylesheets and media queries -------------------------------------------------------

/// lhdump's `OracleContainer` as far as a stylesheet asks it.
#[derive(Debug, Default)]
pub struct TestContainer;

impl twidget_litehtml::web_color::ResolveColor for TestContainer {
    fn resolve_color(&self, _name: &[u8]) -> Vec<u8> {
        Vec::new()
    }
}

impl DocumentContainer for TestContainer {
    fn pt_to_px(&self, pt: f32) -> Px {
        Px(pt * 96.0 / 72.0)
    }
    fn get_default_font_size(&self) -> Px {
        Px(16.0)
    }
    fn import_css(&self, url: &[u8], baseurl: &mut Vec<u8>) -> Vec<u8> {
        match imported(url) {
            Some(text) => {
                *baseurl = [&b"imported/"[..], url].concat();
                text.to_vec()
            }
            None => Vec::new(),
        }
    }
}

/// The sheets lhdump's container gives `@import`.
pub fn imported(url: &[u8]) -> Option<&'static [u8]> {
    match url {
        b"a.css" => Some(b"i{color:red}"),
        b"m.css" => Some(b"@media print{p{x:y}} q{z:w}"),
        b"nested.css" => Some(b"@import 'a.css' screen; r{s:t} @import 'm.css';"),
        b"layer.css" => Some(b"@layer{u{v:w}} @supports (color:red){x{y:z}}"),
        _ => None,
    }
}

/// The media features of lhdump's document: a 375 x 667 screen at 2x.
pub fn document_media() -> MediaFeatures {
    MediaFeatures {
        ty: MediaType::media_type_screen.index(),
        width: Px(375.0),
        height: Px(667.0),
        device_width: Px(375.0),
        device_height: Px(667.0),
        color: 8,
        color_index: 256,
        monochrome: 0,
        resolution: Px(192.0),
    }
}

/// The grid media queries are evaluated against: viewports from none to huge in both orientations, 1x to
/// 3x, colour and monochrome devices, screen and print.
pub fn media_grid() -> Vec<MediaFeatures> {
    let mut grid = Vec::new();
    let widths = [0.0f32, 320.0, 375.0, 599.99, 600.0, 768.0, 1024.0, 1e6];
    let heights = [0.0f32, 400.0, 667.0, 1080.0];
    for (i, &width) in widths.iter().enumerate() {
        for (j, &height) in heights.iter().enumerate() {
            let k = i * heights.len() + j;
            grid.push(MediaFeatures {
                ty: [MediaType::media_type_screen, MediaType::media_type_screen, MediaType::media_type_print, MediaType::media_type_all]
                    [k % 4]
                    .index(),
                width: Px(width),
                height: Px(height),
                device_width: Px(if k.is_multiple_of(3) { width } else { 1920.0 }),
                device_height: Px(if k.is_multiple_of(3) { height } else { 1080.0 }),
                color: if k % 7 == 6 { 0 } else { 8 },
                color_index: if k % 5 == 4 { 0 } else { 256 },
                monochrome: if k % 7 == 6 { 8 } else { 0 },
                resolution: Px([96.0, 192.0, 288.0, 72.0][k % 4]),
            });
        }
    }
    grid
}

/// The grid as lhdump's `g` setting.
pub fn grid_setting(grid: &[MediaFeatures]) -> Vec<u8> {
    let mut o = Out::default();
    o.u32(grid.len() as u32);
    for f in grid {
        o.i32(f.ty).f32(f.width.value()).f32(f.height.value()).f32(f.device_width.value()).f32(f.device_height.value());
        o.i32(f.color).i32(f.color_index).i32(f.monochrome).f32(f.resolution.value());
    }
    framed(b'g', &o.take())
}

pub fn token_list_bytes(list: &[CssToken]) -> Vec<u8> {
    let mut o = Out::default();
    tokens(&mut o, list);
    o.take()
}

fn declarations_out(o: &mut Out, decls: &[RawDeclaration]) {
    o.u32(decls.len() as u32);
    for d in decls {
        o.str(&d.name);
        tokens(o, &d.value);
        o.u8(u8::from(d.important));
    }
}

/// lhdump's `write_rules`.
pub fn rules_out(o: &mut Out, rules: &[RawRule]) {
    o.u32(rules.len() as u32);
    for rule in rules {
        o.u8(rule.ty as u8).str(&rule.name);
        tokens(o, &rule.prelude);
        o.token(&rule.block);
        if rule.ty == RuleType::qualified {
            let mut decls = Vec::new();
            let mut nested = Vec::new();
            CssParser::new(rule.block.value.clone()).consume_style_block_contents(&mut decls, &mut nested);
            declarations_out(o, &decls);
            rules_out(o, &nested);
        } else if rule.block.ty == CURLY_BLOCK {
            rules_out(o, &css_parser::parse_stylesheet_tokens(&rule.block.value, false));
        }
    }
}

fn condition_out(o: &mut Out, condition: &MediaCondition) {
    o.u32(condition.op.0);
    o.u32(condition.m_conditions.len() as u32);
    for item in &condition.m_conditions {
        match item {
            MediaInParens::Condition(c) => {
                o.u8(0);
                condition_out(o, c);
            }
            MediaInParens::Feature(f) => {
                o.u8(1).str(&f.name).f32(f.value).f32(f.value2).u32(u32::from(f.op)).u32(u32::from(f.op2));
            }
            MediaInParens::Unknown => {
                o.u8(2);
            }
        }
    }
}

/// What the stylesheet cases need of a run beyond the name table: the grid, and the answers of the stages
/// not ported yet (selector lists, declarations), learnt from lhdump before the run.
#[derive(Debug, Default)]
pub struct Later {
    pub selector_counts: std::collections::HashMap<Vec<u8>, usize>,
    pub supported: std::collections::HashMap<Vec<u8>, bool>,
}

impl Later {
    /// Every prelude a walk of these sheets may read as a selector list, and every block of an `@supports`
    /// prelude it may read as a declaration, asked of lhdump once.
    pub fn learn(&mut self, oracle: &Oracle, sheets: &[Vec<u8>]) {
        let mut preludes = Vec::new();
        let mut candidates = Vec::new();
        fn walk(rules: &[RawRule], preludes: &mut Vec<CssTokenVector>, candidates: &mut Vec<CssTokenVector>) {
            for rule in rules {
                if rule.ty == RuleType::qualified {
                    preludes.push(rule.prelude.clone());
                    continue;
                }
                if lowcase(&rule.name) == b"supports" {
                    supports_candidates(&normalize_tokens(rule.prelude.clone(), f_componentize, None), candidates);
                }
                if rule.block.ty == CURLY_BLOCK {
                    walk(&css_parser::parse_stylesheet_tokens(&rule.block.value, false), preludes, candidates);
                }
            }
        }
        let mut texts: Vec<Vec<u8>> = sheets.to_vec();
        for url in [&b"a.css"[..], b"m.css", b"nested.css", b"layer.css"] {
            texts.push(imported(url).unwrap_or_default().to_vec());
        }
        for text in &texts {
            for scheme in [None, Some(false), Some(true)] {
                let text = match scheme {
                    None => text.clone(),
                    Some(dark) => twidget_css::rewrite_color_scheme(text, dark),
                };
                walk(&css_parser::parse_stylesheet(&text, true), &mut preludes, &mut candidates);
            }
        }
        let mut input = Vec::new();
        let mut asked_preludes = Vec::new();
        for prelude in preludes {
            let key = token_list_bytes(&prelude);
            if !self.selector_counts.contains_key(&key) && !asked_preludes.contains(&key) {
                input.extend(framed(b'Q', &key));
                asked_preludes.push(key);
            }
        }
        let mut asked_candidates = Vec::new();
        for candidate in candidates {
            let key = token_list_bytes(&candidate);
            if !self.supported.contains_key(&key) && !asked_candidates.contains(&key) {
                input.extend(framed(b'Y', &key));
                asked_candidates.push(key);
            }
        }
        let answers = oracle.run(input, asked_preludes.len() + asked_candidates.len());
        let (counts, supported) = answers.split_at(asked_preludes.len());
        for (key, answer) in asked_preludes.into_iter().zip(counts) {
            self.selector_counts.insert(key, u32::from_le_bytes(answer.1[..4].try_into().expect("a count")) as usize);
        }
        for (key, answer) in asked_candidates.into_iter().zip(supported) {
            self.supported.insert(key, answer.0 == 1);
        }
    }
}

/// The document a stylesheet case reads, as lhdump's.
pub struct Sheets<'a> {
    pub lh: &'a Lh,
    pub later: &'a Later,
    pub container: TestContainer,
}

impl StyleSheetHost for Sheets<'_> {
    fn names(&self) -> &Names {
        &self.lh.names
    }
    fn container(&self) -> &dyn DocumentContainer {
        &self.container
    }
    fn media_document(&self) -> MediaDocument<'_> {
        MediaDocument {
            names: &self.lh.names,
            units: Units { container: &self.container, media: document_media(), root_font_size: Px(16.0) },
        }
    }
    fn selector_count(&self, prelude: &[CssToken]) -> usize {
        *self.later.selector_counts.get(&token_list_bytes(prelude)).expect("a prelude the run learnt")
    }
    fn supports_declaration(&self, tokens: &[CssToken]) -> bool {
        *self.later.supported.get(&token_list_bytes(tokens)).expect("a declaration the run learnt")
    }
    fn add_media_list(&self, _list: &std::sync::Arc<MediaQueryListList>) {}
}

impl Lh {
    /// What lhdump answers to a stylesheet record (`r`, `B`, `e`, `w`, `o`, `U`, `X`, `V`) against `grid`.
    pub fn sheet_answer(&self, later: &Later, grid: &[MediaFeatures], op: u8, input: &[u8]) -> (u8, Vec<u8>) {
        let mut i = In::new(input);
        let mut o = Out::default();
        let sheets = Sheets { lh: self, later, container: TestContainer };
        match op {
            b'r' => {
                let top_level = i.u8() != 0;
                rules_out(&mut o, &css_parser::parse_stylesheet(&i.rest(), top_level));
                (1, o.take())
            }
            b'B' => {
                let mut decls = Vec::new();
                let mut rules = Vec::new();
                CssParser::new(normalize(input, f_componentize, None)).consume_style_block_contents(&mut decls, &mut rules);
                declarations_out(&mut o, &decls);
                rules_out(&mut o, &rules);
                for decl in &mut decls {
                    css_parser::remove_whitespace(&mut decl.value, None);
                    if !decl.name.starts_with(b"--") {
                        decl.name = lowcase(&decl.name);
                    }
                }
                declarations_out(&mut o, &decls);
                (1, o.take())
            }
            b'e' => {
                let list = parse_media_query_list_text(input, &sheets.media_document());
                o.u32(list.m_queries.len() as u32);
                for q in &list.m_queries {
                    o.u8(u8::from(q.m_not)).i32(q.m_media_type).u32(q.m_conditions.len() as u32);
                    for c in &q.m_conditions {
                        condition_out(&mut o, c);
                    }
                }
                for features in grid {
                    for q in &list.m_queries {
                        o.u8(q.check(&self.names, features) as u8);
                    }
                    o.u8(u8::from(list.check(&self.names, features)));
                }
                (1, o.take())
            }
            b'w' => {
                let scheme = i.u8();
                let mut text = i.rest();
                if scheme != 0 {
                    text = twidget_css::rewrite_color_scheme(&text, scheme == 2);
                }
                let mut sheet = Css::default();
                sheet.parse_css_stylesheet(StyleSheetInput::Text(&text), b"", &sheets, None, true);
                let mut rules: Vec<*const StyleRule> = Vec::new();
                let mut lists: Vec<std::sync::Arc<MediaQueryListList>> = Vec::new();
                o.u32(sheet.selectors().len() as u32);
                for sel in sheet.selectors() {
                    let ptr = std::sync::Arc::as_ptr(&sel.rule);
                    let rule = rules.iter().position(|r| *r == ptr).unwrap_or_else(|| {
                        rules.push(ptr);
                        rules.len() - 1
                    });
                    let list = match &sel.media_query {
                        None => -1,
                        Some(m) => lists.iter().position(|l| std::sync::Arc::ptr_eq(l, m)).unwrap_or_else(|| {
                            lists.push(std::sync::Arc::clone(m));
                            lists.len() - 1
                        }) as i32,
                    };
                    o.i32(sel.order).u8(u8::from(sel.layered)).i32(rule as i32).i32(list);
                }
                o.u32(lists.len() as u32);
                for list in &lists {
                    for features in grid {
                        list.apply_media_features(&self.names, features);
                        o.u8(u8::from(list.is_used()));
                    }
                }
                (1, o.take())
            }
            b'o' => {
                let dark = i.u8() != 0;
                (1, twidget_css::rewrite_color_scheme(&i.rest(), dark))
            }
            b'U' => match parse_url(&i.token(0)) {
                Some(url) => (1, url),
                None => (0, Vec::new()),
            },
            b'X' => (1, decode(input, Encoding::utf_8)),
            b'V' => {
                let tok = i.token(0);
                let mut len = CssLength::default();
                let ok = len.from_token(&tok, f_length_percentage, CssValues::default());
                let fm = FontMetrics { font_size: Px(16.0), x_height: Px(16.0), ch_width: Px(7.0), ..FontMetrics::default() };
                let units = sheets.media_document().units;
                o.f32(units.to_pixels(&len, &fm, Px(200.0)).value());
                units.cvt_units(&mut len, &fm, Px(200.0));
                length(&mut o, &len);
                (u8::from(ok), o.take())
            }
            other => panic!("no stylesheet record {}", char::from(other)),
        }
    }
}

/// Selectors of every kind and some no selector at all.
fn any_selector(random: &mut Lcg) -> String {
    let parts = [
        "div",
        "p",
        "*",
        ".a",
        "#b",
        "a.c",
        "[x]",
        "[x=y]",
        "[x~='z' i]",
        ":hover",
        ":not(.a)",
        ":nth-child(2n+1)",
        "::before",
        ":is(p, div)",
        ":where(.q)",
        "html",
        "body",
        "li:first-child",
        "input[type=checkbox]",
        ":root",
        "svg|a",
        "!",
        "1",
        ".1a",
        "#",
        "[",
        ":unknown(",
        ":lang(en)",
        "button:active",
        "&",
        "& > a",
    ];
    let combinators = [" ", " > ", " + ", " ~ ", ",", ", ", ""];
    let mut s = String::new();
    for k in 0..1 + random.below(3) {
        if k > 0 {
            s.push_str(pick(random, &combinators));
        }
        s.push_str(pick(random, &parts));
    }
    s
}

/// A media query list in every syntax, valid and not.
pub fn any_media_query_list(random: &mut Lcg) -> String {
    fn feature(random: &mut Lcg) -> String {
        let names = [
            "width",
            "height",
            "min-width",
            "max-width",
            "min-height",
            "max-height",
            "aspect-ratio",
            "min-aspect-ratio",
            "orientation",
            "resolution",
            "min-resolution",
            "max-resolution",
            "color",
            "min-color",
            "color-index",
            "monochrome",
            "device-width",
            "max-device-width",
            "device-height",
            "device-aspect-ratio",
            "min-orientation",
            "hover",
            "prefers-color-scheme",
            "prefers-reduced-motion",
            "WIDTH",
            "grid",
        ];
        let values = [
            "0",
            "600px",
            "40em",
            "30rem",
            "50vw",
            "50vh",
            "10vmin",
            "1in",
            "2.54cm",
            "72pt",
            "6pc",
            "10mm",
            "1ch",
            "2ex",
            "16/9",
            "4 / 3",
            "1/0",
            "0/0",
            "portrait",
            "landscape",
            "2dppx",
            "2x",
            "192dpi",
            "75dpcm",
            "infinite",
            "3",
            "-1",
            "1.5",
            "dark",
            "light",
            "none",
            "600",
            "calc(1px)",
            "2foo",
            "-2foo",
        ];
        let name = pick(random, &names);
        match random.below(6) {
            0 => format!("({name})"),
            1 | 2 => format!("({name}:{})", pick(random, &values)),
            3 => format!("({name} {} {})", pick(random, &["<", "<=", ">", ">=", "=", "< =", "=>"]), pick(random, &values)),
            4 => format!("({} {} {name})", pick(random, &values), pick(random, &["<", "<=", ">", ">=", "="])),
            _ => {
                let op = pick(random, &["<", "<=", ">", ">="]);
                format!("({} {op} {name} {op} {})", pick(random, &values), pick(random, &values))
            }
        }
    }
    fn condition(random: &mut Lcg, depth: u32) -> String {
        let mut s = String::new();
        if random.chance(5) {
            s.push_str("not ");
        }
        let joiner = pick(random, &[" and ", " or ", " and ", " AND "]);
        for k in 0..1 + random.below(3) {
            if k > 0 {
                s.push_str(joiner);
            }
            if depth < 3 && random.chance(5) {
                s.push_str(&format!("({})", condition(random, depth + 1)));
            } else if random.chance(10) {
                s.push_str(pick(random, &["fn(x)", "(x y)", "(!)", "([)", "()"]));
            } else {
                s.push_str(&feature(random));
            }
        }
        s
    }
    let mut queries = Vec::new();
    for _ in 0..1 + random.below(3) {
        let q = match random.below(4) {
            0 => condition(random, 0),
            1 => pick(
                random,
                &["screen", "print", "all", "tv", "speech", "SCREEN", "only screen", "not print", "unknown", "not", "and", "layer", ""],
            )
            .to_string(),
            _ => {
                let ty = pick(random, &["screen", "print", "all", "only screen", "not screen", "not all", "tv"]);
                format!("{ty} and {}", condition(random, 1))
            }
        };
        queries.push(q);
    }
    queries.join(pick(random, &[", ", ","]))
}

/// A stylesheet of every rule kind: style rules with declarations, `@media`, `@supports`, `@layer`,
/// `@import`, `@charset`, unknown at-rules, nested rules, comments, CDO and CDC, and broken ones.
pub fn any_stylesheet(random: &mut Lcg, depth: u32) -> String {
    fn declarations(random: &mut Lcg) -> String {
        let props = [
            "color",
            "COLOR",
            "width",
            "margin",
            "--x",
            "--X",
            "display",
            "background",
            "font-size",
            "-webkit-x",
            "x",
            "1x",
            "transform",
            "opacity",
        ];
        let values = [
            "red",
            "1px",
            "calc(1px + 2%)",
            "0",
            "var(--x)",
            "rgb(1 2 3)",
            "none !important",
            "a b c",
            "1px ! important",
            "\"s\"",
            "url(a.png)",
            "",
            "  2em  ",
            "{x}",
            "(a;b)",
            "!important",
        ];
        let mut s = String::new();
        for k in 0..random.below(5) {
            if k > 0 {
                s.push_str(pick(random, &[";", "; ", ";;", "\n"]));
            }
            let sep = if random.chance(12) { " " } else { ":" };
            s.push_str(&format!("{}{sep}{}", pick(random, &props), pick(random, &values)));
        }
        if random.chance(2) {
            s.push(';');
        }
        s
    }
    let mut s = String::new();
    for _ in 0..1 + random.below(5) {
        match random.below(14) {
            0..=5 => s.push_str(&format!("{}{{{}}}", any_selector(random), declarations(random))),
            6 if depth < 4 => s.push_str(&format!("@media {}{{{}}}", any_media_query_list(random), any_stylesheet(random, depth + 1))),
            7 if depth < 4 => {
                let condition = pick(
                    random,
                    &[
                        "(color: red)",
                        "(display: grid)",
                        "not (x: y)",
                        "(color:red) and (width:1px)",
                        "(color:red) or (nope:1)",
                        "selector(a)",
                        "(color:red) and (a:b) or (c:d)",
                        "((color: red))",
                        "(color: red; width: 1px)",
                        "",
                        "x",
                        "(--v: 1)",
                        "not ((a))",
                    ],
                );
                s.push_str(&format!("@supports {condition}{{{}}}", any_stylesheet(random, depth + 1)));
            }
            8 if depth < 4 => {
                let name = pick(random, &["", " base", " a.b", " x, y"]);
                if random.chance(3) {
                    s.push_str(&format!("@layer{name};"));
                } else {
                    s.push_str(&format!("@layer{name}{{{}}}", any_stylesheet(random, depth + 1)));
                }
            }
            9 => {
                let url =
                    pick(random, &["'a.css'", "\"m.css\"", "url(nested.css)", "url('layer.css')", "url( \"a.css\" )", "'x.css'", "a.css"]);
                let media = pick(random, &["", " screen", " print", " (min-width: 600px)", " screen and (orientation: portrait)"]);
                s.push_str(&format!("@import {url}{media};"));
            }
            10 => s.push_str(pick(
                random,
                &[
                    "@charset \"utf-8\";",
                    "@font-face{font-family:x}",
                    "@page{margin:0}",
                    "@keyframes k{from{x:y}}",
                    "@x;",
                    "@MEDIA all{a{b:c}}",
                ],
            )),
            11 => s.push_str(&format!("a{{{}{{{}}}}}", pick(random, &["& b", "&:hover", "@media print", "b"]), declarations(random))),
            12 => s.push_str(pick(random, &["/* c */", "<!--", "-->", " ", "}", "{", ";", "a{", "@media screen{a{b:c}"])),
            _ => s.push_str(&format!("@media (prefers-color-scheme: {}){{a{{b:c}}}}", pick(random, &["dark", "light", "Dark", ""]))),
        }
        s.push_str(pick(random, &["", " ", "\n"]));
    }
    s
}
