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

#![allow(dead_code, unused_imports)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use twidget_testkit::{Comparison, Lcg, Oracle, framed, hex, mutate};

use twidget_litehtml::css_length::*;
use twidget_litehtml::css_tokenizer::*;
use twidget_litehtml::css_values::*;
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

// ---- The corpus ---------------------------------------------------------------------------------

pub fn twidget() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

pub fn widget_sandbox() -> PathBuf {
    std::env::var_os("TWIDGET_WIDGET_SANDBOX")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../widget-sandbox"))
}
