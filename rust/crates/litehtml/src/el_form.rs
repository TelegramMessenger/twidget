//! `el_form.cpp`: what the form controls take from their attributes and the boxes they make for what they show
//! (`lh-input-text`, `lh-input-spin`, `lh-range-track` and `lh-range-thumb`, `lh-select-value`,
//! `lh-placeholder`), each an `html_tag` with an inline style, and their look as their computed style decides it
//! (with what it changes of that style). Their painting is a later stage's.

use crate::css_length::CssLength;
use crate::css_parser::{f_componentize, f_remove_whitespace, normalize};
use crate::css_properties::{Background, CssBorders, CssProperties};
use crate::css_values::*;
use crate::cxx;
use crate::document::{Document, Env, StyleCx};
use crate::element::{ElementId, ElementKind, ElementState};
use crate::html::{WHITESPACE, lowcase, trim};
use crate::string_id::{_legend_, _optgroup_, _option_};
use crate::web_color::{WebColor, parse_color};

/// What a control shows when it would show nothing: a zero-width space, so that its line has a height.
const ZERO_WIDTH_SPACE: &[u8] = b"\xE2\x80\x8B";

/// `date_pattern()`: the date order a date field shows. On Apple systems the C++ asks CoreFoundation for the
/// user's locale's (unless `TW_LITEHTML_NO_CORE_FOUNDATION`), which is not ported; elsewhere it is this.
fn date_pattern() -> &'static [u8] {
    b"MM/dd/yyyy"
}

/// `%Nd` of `sscanf`: whitespace skipped, then a sign and digits in at most `width` characters.
fn scan_int(value: &[u8], at: &mut usize, width: usize) -> Option<i32> {
    while *at < value.len() && twidget_cstd::is_space(value[*at]) {
        *at += 1;
    }
    let start = *at;
    let mut i = *at;
    let negative = value.get(i) == Some(&b'-');
    if matches!(value.get(i), Some(b'-' | b'+')) {
        i += 1;
    }
    let digits_start = i;
    let mut n: i64 = 0;
    while i < value.len() && i - start < width && value[i].is_ascii_digit() {
        n = n * 10 + i64::from(value[i] - b'0');
        i += 1;
    }
    if i == digits_start {
        return None;
    }
    *at = i;
    Some(if negative { -n } else { n } as i32)
}

/// `sscanf(value, "%4d-%2d-%2d%c", …)`: how many fields it fills, and the three numbers.
fn scan_date(value: &[u8]) -> (i32, i32, i32, i32) {
    let mut at = 0;
    let (mut year, mut month, mut day) = (0, 0, 0);
    let Some(y) = scan_int(value, &mut at, 4) else { return (0, year, month, day) };
    year = y;
    if value.get(at) != Some(&b'-') {
        return (1, year, month, day);
    }
    at += 1;
    let Some(m) = scan_int(value, &mut at, 2) else { return (1, year, month, day) };
    month = m;
    if value.get(at) != Some(&b'-') {
        return (2, year, month, day);
    }
    at += 1;
    let Some(d) = scan_int(value, &mut at, 2) else { return (2, year, month, day) };
    day = d;
    (if at < value.len() { 4 } else { 3 }, year, month, day)
}

/// `format_date(value, placeholder)`: a `yyyy-mm-dd` value in the pattern's order, or the pattern as a hint (and
/// `placeholder` set) for anything else.
fn format_date(value: &[u8]) -> (Vec<u8>, bool) {
    let pattern = date_pattern();
    let (fields, year, month, day) = if value.len() != 10 { (0, 0, 0, 0) } else { scan_date(value) };
    if value.len() != 10 || fields != 3 || !(1..=12).contains(&month) || !(1..=31).contains(&day) || year < 1 {
        let hint = pattern.iter().map(|&ch| if ch == b'M' { b'm' } else { ch }).collect();
        return (hint, true);
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < pattern.len() {
        let ch = pattern[i];
        let mut run = 1;
        while i + run < pattern.len() && pattern[i + run] == ch {
            run += 1;
        }
        if ch == b'y' {
            out.extend_from_slice(format!("{year:04}").as_bytes());
        } else if ch == b'M' || ch == b'd' {
            let n = if ch == b'M' { month } else { day };
            out.extend_from_slice(if run >= 2 { format!("{n:02}") } else { format!("{n}") }.as_bytes());
        } else if ch != b'\'' {
            out.extend(std::iter::repeat_n(ch, run));
        }
        i += run;
    }
    (out, false)
}

/// `is_text_like()` of a type.
fn is_text_like(ty: &[u8]) -> bool {
    ![&b"hidden"[..], b"checkbox", b"radio", b"range", b"color", b"button", b"submit", b"reset", b"image", b"file"].contains(&ty)
}

/// `is_button_like()` of a type.
fn is_button_like(ty: &[u8]) -> bool {
    [&b"button"[..], b"submit", b"reset"].contains(&ty)
}

fn is_generated_tag(tag: &[u8]) -> bool {
    tag.starts_with(b"lh-")
}

impl Document {
    /// `attr_number(el, name, def)`: the attribute as `strtof` reads it (C locale), `def` when it is missing,
    /// empty, no number or not finite.
    fn attr_number(&self, id: ElementId, name: &[u8], def: f32) -> f32 {
        let Some(value) = self.get_attr(id, name) else { return def };
        if value.is_empty() {
            return def;
        }
        let (result, end) = twidget_cstd::strtof(value);
        if end == 0 || !result.is_finite() { def } else { result }
    }

    /// `append_content(parent, tag, style, text)`: a generated `html_tag` with the style, holding the text as one
    /// `el_text` when there is any, appended to the control.
    fn append_content(&mut self, env: &Env, parent: ElementId, tag: &[u8], style: &[u8], text: &[u8]) -> ElementId {
        let child = self.new_element(ElementKind::html_tag);
        self.set_tag_name(env.names, child, tag);
        self.set_attr(env.names, child, b"style", style);
        if !text.is_empty() {
            let text = self.new_text(text, false);
            self.append_child(child, text);
        }
        self.append_child(parent, child);
        child
    }

    /// `has_generated(parent)`: a child whose tag starts with `lh-`.
    pub fn has_generated(&self, env: &Env, parent: ElementId) -> bool {
        self.children(parent).iter().any(|&child| is_generated_tag(&self.get_tag_name(env.names, child)))
    }

    /// `textarea_text(parent)`: the text of the children not generated.
    pub fn textarea_text(&self, env: &Env, parent: ElementId) -> Vec<u8> {
        let mut text = Vec::new();
        for &child in self.children(parent) {
            if !is_generated_tag(&self.get_tag_name(env.names, child)) {
                self.get_text(child, &mut text);
            }
        }
        text
    }

    /// `remove_generated(parent)`: the generated children taken out.
    pub fn remove_generated(&mut self, env: &Env, parent: ElementId) {
        let generated: Vec<ElementId> =
            self.children(parent).iter().copied().filter(|&child| is_generated_tag(&self.get_tag_name(env.names, child))).collect();
        for child in generated {
            self.remove_child(parent, child);
        }
    }

    /// `el_input::parse_attributes()`: `html_tag`'s, then `m_type` (lower-case, `text` by default) and
    /// `m_checked`, and what the type shows: a text-like field its value (a date in the pattern's order, a
    /// password as bullets, the placeholder in grey when empty) sized by `size`, and a number field its spinner;
    /// a button its value or its type's word; a range its place between `min` and `max` as a track and thumb; a
    /// colour its value as a swatch.
    pub fn el_input_parse_attributes(&mut self, env: &Env, id: ElementId) {
        self.html_tag_parse_attributes(env, id);
        self.remove_generated(env, id);
        let ty = lowcase(self.get_attr(id, b"type").unwrap_or(b"text"));
        let checked = self.get_attr(id, b"checked").is_some();
        let value = self.get_attr(id, b"value").map(<[u8]>::to_vec);
        let (mut fraction, mut swatch) = match &self.el(id).state {
            ElementState::Input { fraction, swatch, .. } => (*fraction, *swatch),
            _ => (0.5, WebColor::BLACK),
        };
        if is_text_like(&ty) {
            let mut text = value.clone().unwrap_or_default();
            let mut placeholder = false;
            if ty == b"date" {
                (text, placeholder) = format_date(&text);
            } else if text.is_empty()
                && let Some(hint) = self.get_attr(id, b"placeholder")
                && !hint.is_empty()
            {
                text = hint.to_vec();
                placeholder = true;
            }
            if ty == b"password" && !placeholder {
                let mut dots = Vec::new();
                for &ch in &text {
                    if (ch & 0xC0) != 0x80 {
                        dots.extend_from_slice(b"\xE2\x80\xA2");
                    }
                }
                text = dots;
            }
            let size = self.attr_number(id, b"size", 20.0);
            let size = if (1.0..=1000.0).contains(&size) { size } else { 20.0 };
            let width = size * 0.636 + 0.182 - if ty == b"number" { 0.91 } else { 0.0 };
            let mut style = b"display:block;flex:1 1 auto;min-width:0;overflow:hidden;white-space:pre;width:".to_vec();
            style.extend_from_slice(&cxx::to_string_f64(f64::from(width)));
            style.extend_from_slice(b"em");
            if placeholder {
                style.extend_from_slice(b";color:#a9a9a9");
            }
            self.append_content(env, id, b"lh-input-text", &style, if text.is_empty() { ZERO_WIDTH_SPACE } else { &text });
            if ty == b"number" {
                self.append_content(env, id, b"lh-input-spin", b"display:block;flex:none;width:10px;align-self:stretch", b"");
            }
        } else if is_button_like(&ty) {
            let text = match &value {
                Some(value) => value.clone(),
                None if ty == b"reset" => b"Reset".to_vec(),
                None if ty == b"submit" => b"Submit".to_vec(),
                None => Vec::new(),
            };
            self.append_content(
                env,
                id,
                b"lh-input-text",
                b"display:block;white-space:pre",
                if text.is_empty() { ZERO_WIDTH_SPACE } else { &text },
            );
        } else if ty == b"range" {
            let min = self.attr_number(id, b"min", 0.0);
            let max = self.attr_number(id, b"max", 100.0);
            let val = self.attr_number(id, b"value", min + (max - min) / 2.0);
            fraction = if max > min { cxx::max_f32(0.0, cxx::min_f32(1.0, (val - min) / (max - min))) } else { 0.0 };
            let mut percent = twidget_cstd::format_general(f64::from(fraction * 100.0)).into_bytes();
            percent.push(b'%');
            let track = self.new_element(ElementKind::html_tag);
            self.set_tag_name(env.names, track, b"lh-range-track");
            self.set_attr(env.names, track, b"style", b"display:block;flex:1 1 auto;position:relative;min-width:0");
            let thumb = self.new_element(ElementKind::html_tag);
            self.set_tag_name(env.names, thumb, b"lh-range-thumb");
            let style =
                [&b"display:block;position:absolute;top:0;left:"[..], &percent, b";transform:translateX(-", &percent, b")"].concat();
            self.set_attr(env.names, thumb, b"style", &style);
            self.append_child(track, thumb);
            self.append_child(id, track);
            self.el_mut(track).master_sheet = true;
        } else if ty == b"color" {
            swatch = WebColor::BLACK;
            if let Some(value) = value.filter(|v| !v.is_empty()) {
                let tokens = normalize(&value, f_componentize | f_remove_whitespace, None);
                if tokens.len() == 1
                    && let Some(color) = parse_color(env.names, &tokens[0], Some(env.container))
                {
                    swatch = color;
                }
            }
        }
        self.el_mut(id).state = ElementState::Input { ty, checked, fraction, swatch };
    }

    /// `el_select::parse_attributes()`: `html_tag`'s, then the label of the first selected option (or else the
    /// first option), options in groups included, trimmed, as its value box.
    pub fn el_select_parse_attributes(&mut self, env: &Env, id: ElementId) {
        self.html_tag_parse_attributes(env, id);
        self.remove_generated(env, id);
        let mut label = Vec::new();
        let mut found = false;
        self.select_label(id, false, &mut label, &mut found);
        if !found {
            self.select_label(id, true, &mut label, &mut found);
        }
        let trimmed = trim(&label, WHITESPACE);
        self.append_content(
            env,
            id,
            b"lh-select-value",
            b"display:block;flex:1 1 auto;min-width:0;overflow:hidden;white-space:pre",
            if trimmed.is_empty() { ZERO_WIDTH_SPACE } else { &trimmed },
        );
    }

    /// `el_select::parse_attributes`'s `visit(el, any)`: the text of the first option (selected unless `any`).
    fn select_label(&self, el: ElementId, any: bool, label: &mut Vec<u8>, found: &mut bool) {
        for &child in self.children(el) {
            if *found {
                return;
            }
            if self.tag(child) == _option_ {
                if any || self.get_attr(child, b"selected").is_some() {
                    self.get_text(child, label);
                    *found = true;
                    return;
                }
            } else if self.tag(child) == _optgroup_ {
                self.select_label(child, any, label, found);
            }
        }
    }

    /// `el_textarea::parse_attributes()`: `html_tag`'s, then the placeholder as a box when the text is empty.
    pub fn el_textarea_parse_attributes(&mut self, env: &Env, id: ElementId) {
        self.html_tag_parse_attributes(env, id);
        self.remove_generated(env, id);
        if let Some(hint) = self.get_attr(id, b"placeholder").map(<[u8]>::to_vec)
            && !hint.is_empty()
            && self.textarea_text(env, id).is_empty()
        {
            let placeholder = self.append_content(env, id, b"lh-placeholder", b"display:inline", &hint);
            self.el_mut(placeholder).master_sheet = true;
        }
    }
}

// ---- The look: what the controls make of their computed style ------------------------------------------------

/// `palette`: the colours a control is drawn in, light or dark (the ones its computed style reads).
struct Palette {
    field: WebColor,
    text: WebColor,
    control_text: WebColor,
    disabled_text: WebColor,
    button_face: WebColor,
    button_border: WebColor,
    legacy_field_border: WebColor,
}

/// `colors(dark)`.
fn colors(dark: bool) -> Palette {
    let c = WebColor::rgba;
    if dark {
        Palette {
            field: c(30, 30, 30, 255),
            text: c(255, 255, 255, 255),
            control_text: c(255, 255, 255, 216),
            disabled_text: c(255, 255, 255, 64),
            button_face: c(90, 90, 90, 255),
            button_border: c(90, 90, 90, 255),
            legacy_field_border: c(128, 128, 128, 255),
        }
    } else {
        Palette {
            field: c(255, 255, 255, 255),
            text: c(0, 0, 0, 255),
            control_text: c(0, 0, 0, 216),
            disabled_text: c(0, 0, 0, 64),
            button_face: c(192, 192, 192, 255),
            button_border: c(192, 192, 192, 255),
            legacy_field_border: c(128, 128, 128, 255),
        }
    }
}

/// `sentinel_border`, `sentinel_back` and `sentinel_text`: the colours that mean a control's box and text are
/// the UA's.
const sentinel_border: WebColor = WebColor::rgba(1, 2, 3, 1);
const sentinel_back: WebColor = WebColor::rgba(3, 2, 1, 1);
const sentinel_text: WebColor = WebColor::rgba(2, 3, 1, 1);

/// `make_flex(css, direction, justify, align)`: a block a flex box, an inline or inline block an inline flex one,
/// with the layout; any other display stays.
fn make_flex(css: &mut CssProperties, direction: i32, justify: i32, align: i32) {
    if css.m_display == StyleDisplay::display_block.index() {
        css.m_display = StyleDisplay::display_flex.index();
    } else if css.m_display == StyleDisplay::display_inline.index() || css.m_display == StyleDisplay::display_inline_block.index() {
        css.m_display = StyleDisplay::display_inline_flex.index();
    } else {
        return;
    }
    css.set_flex_layout(direction, justify, align);
}

/// A flex row from the start, centred across: what the fields and the select lay out in.
fn make_flex_row(css: &mut CssProperties) {
    make_flex(
        css,
        FlexDirection::flex_direction_row.index(),
        FlexJustifyContent::flex_justify_content_flex_start.index(),
        FlexAlignItems::flex_align_items_center.index(),
    );
}

/// `radius_set(borders, ua)`: a horizontal radius other than the UA's.
fn radius_set(borders: &CssBorders, ua: f32) -> bool {
    let r = &borders.radius;
    [&r.top_left_x, &r.top_right_x, &r.bottom_right_x, &r.bottom_left_x].iter().any(|len| (len.val() - ua).abs() > 0.01)
}

/// `has_image(bg)`.
fn has_image(bg: &Background) -> bool {
    bg.m_image.iter().any(|img| !img.is_empty())
}

/// `authored_box(css, ua_radius)`: a border colour, background or radius the page gave.
fn authored_box(css: &CssProperties, ua_radius: f32) -> bool {
    let b = &css.m_css_borders;
    if [&b.top, &b.right, &b.bottom, &b.left].iter().any(|side| side.color != sentinel_border) {
        return true;
    }
    let bg = &css.m_bg;
    bg.m_color != sentinel_back || has_image(bg) || radius_set(b, ua_radius)
}

/// `resolve_box(css, native, border, back)`: the UA's border colours and background as the look has them.
fn resolve_box(css: &mut CssProperties, native: bool, border: WebColor, back: WebColor) {
    let borders = &mut css.m_css_borders;
    for side in [&mut borders.top, &mut borders.right, &mut borders.bottom, &mut borders.left] {
        if native || side.color == sentinel_border {
            side.color = if native { WebColor::TRANSPARENT } else { border };
        }
    }
    if native || css.m_bg.m_color == sentinel_back {
        css.m_bg.m_color = if native { WebColor::TRANSPARENT } else { back };
    }
}

/// `resolve_text(css, color)`.
fn resolve_text(css: &mut CssProperties, color: WebColor) {
    if css.m_color == sentinel_text {
        css.m_color = color;
    }
}

/// `add_padding(css, top, right, bottom, left, replace)`: px added to each side (to 0 when `replace` or a %).
fn add_padding(css: &mut CssProperties, top: f32, right: f32, bottom: f32, left: f32, replace: bool) {
    let bump = |len: &mut CssLength, v: f32| {
        let base = if replace || len.units() == CssUnits::css_units_percentage { 0.0 } else { len.val() };
        len.set_value(base + v, CssUnits::css_units_px);
    };
    let padding = &mut css.m_css_padding;
    bump(&mut padding.top, top);
    bump(&mut padding.right, right);
    bump(&mut padding.bottom, bottom);
    bump(&mut padding.left, left);
}

/// A length of `v` px, as the UA's paddings are told apart.
fn is_px(len: &CssLength, v: f32) -> bool {
    len.units() == CssUnits::css_units_px && (len.val() - v).abs() < 0.0001
}

/// `ua_padding(css)`: the UA's field padding.
fn ua_padding(css: &CssProperties) -> bool {
    let p = &css.m_css_padding;
    is_px(&p.top, 1.01) && is_px(&p.bottom, 1.01) && is_px(&p.left, 2.01) && (is_px(&p.right, 2.01) || is_px(&p.right, 2.02))
}

/// `button_padding(css, native)`: the UA's button padding as the look has it.
fn button_padding(css: &mut CssProperties, native: bool) {
    let p = &css.m_css_padding;
    if !is_px(&p.top, 0.01) || !is_px(&p.right, 6.01) || !is_px(&p.bottom, 1.01) || !is_px(&p.left, 6.01) {
        return;
    }
    let padding = &mut css.m_css_padding;
    padding.top.set_value(if native { 0.0 } else { 2.0 }, CssUnits::css_units_px);
    padding.bottom.set_value(if native { 1.0 } else { 3.0 }, CssUnits::css_units_px);
    padding.left.set_value(6.0, CssUnits::css_units_px);
    padding.right.set_value(6.0, CssUnits::css_units_px);
}

/// `field_padding(css, native, number)`: the UA's field padding as the look has it.
fn field_padding(css: &mut CssProperties, native: bool, number: bool) {
    if !ua_padding(css) {
        return;
    }
    let size = css.get_font_size().value();
    let v = if native { (size * 0.2).round() } else { 1.0 };
    let h = if native { (size * 0.5).floor() } else { 1.0 };
    let padding = &mut css.m_css_padding;
    padding.top.set_value(v, CssUnits::css_units_px);
    padding.bottom.set_value(v, CssUnits::css_units_px);
    padding.left.set_value(h, CssUnits::css_units_px);
    padding.right.set_value(if number && native { 2.0 } else { h }, CssUnits::css_units_px);
}

impl Document {
    fn compute_children(&mut self, cx: &StyleCx, id: ElementId) {
        let mut i = 0;
        while i < self.el(id).children.len() {
            let child = self.el(id).children[i];
            self.compute_styles(cx, child, true);
            i += 1;
        }
    }

    /// `el_input::compute_styles(recursive)`: a text-like field's or button's look (native unless the page styled
    /// its box), padding, box and text colours, a field a flex row; a checkbox, radio, range or colour's look, a
    /// range a flex row when not native, a native one's background transparent; then the children (a native
    /// range's hidden).
    pub fn el_input_compute_styles(&mut self, cx: &StyleCx, id: ElementId, recursive: bool) {
        self.html_tag_compute_styles(cx, id, false);
        let ty = match &self.el(id).state {
            ElementState::Input { ty, .. } => ty.clone(),
            _ => Vec::new(),
        };
        let disabled = self.get_attr(id, b"disabled").is_some();
        let mut look = self.el(id).look;
        let c = &mut self.el_mut(id).css;
        look.dark = c.m_dark_scheme != 0;
        look.disabled = disabled;
        let appearance = c.m_appearance != Appearance::appearance_none.index();
        let p = colors(look.dark);
        if is_text_like(&ty) {
            look.native = appearance && !authored_box(c, 0.0);
            field_padding(c, look.native, ty == b"number");
            resolve_box(c, look.native, p.legacy_field_border, p.field);
            resolve_text(c, if look.disabled { p.disabled_text } else { p.text });
            make_flex_row(c);
        } else if is_button_like(&ty) {
            look.native = appearance && !authored_box(c, 0.0);
            button_padding(c, look.native);
            resolve_box(c, look.native, p.button_border, p.button_face);
            resolve_text(c, if look.disabled { p.disabled_text } else { p.control_text });
            c.m_center_content = true;
        } else {
            look.native = appearance;
            if ty == b"range" && !look.native {
                make_flex_row(c);
            }
            resolve_box(c, false, WebColor::TRANSPARENT, WebColor::TRANSPARENT);
            resolve_text(c, p.text);
            if look.native && [&b"checkbox"[..], b"radio", b"range", b"color"].contains(&&ty[..]) && !has_image(&c.m_bg) {
                c.m_bg.m_color = WebColor::TRANSPARENT;
            }
        }
        self.el_mut(id).look = look;
        if recursive {
            self.compute_children(cx, id);
        }
        if ty == b"range" && look.native {
            for child in self.el(id).children.clone() {
                self.el_mut(child).css.m_display = StyleDisplay::display_none.index();
            }
        }
    }

    /// `el_select::compute_styles(recursive)`: native unless the page styled its box, the arrow with any
    /// appearance, box and text colours, padding for the arrow, a flex row; then the children.
    pub fn el_select_compute_styles(&mut self, cx: &StyleCx, id: ElementId, recursive: bool) {
        self.html_tag_compute_styles(cx, id, false);
        let disabled = self.get_attr(id, b"disabled").is_some();
        let mut look = self.el(id).look;
        let c = &mut self.el_mut(id).css;
        let p = colors(c.m_dark_scheme != 0);
        look.dark = c.m_dark_scheme != 0;
        look.disabled = disabled;
        let appearance = c.m_appearance != Appearance::appearance_none.index();
        look.native = appearance && !authored_box(c, 5.0);
        let arrow = appearance;
        resolve_box(c, look.native, p.control_text, p.field);
        resolve_text(c, if look.disabled { p.disabled_text } else { p.control_text });
        if look.native {
            add_padding(c, 2.5, 22.7, 2.5, 8.5, false);
        } else if appearance {
            let v = c.get_font_size().value() * 0.1;
            add_padding(c, v, 22.7, v, 8.5, true);
        }
        make_flex_row(c);
        self.el_mut(id).look = look;
        self.el_mut(id).arrow = arrow;
        if recursive {
            self.compute_children(cx, id);
        }
    }

    /// `el_button::compute_styles(recursive)`.
    pub fn el_button_compute_styles(&mut self, cx: &StyleCx, id: ElementId, recursive: bool) {
        self.html_tag_compute_styles(cx, id, false);
        let disabled = self.get_attr(id, b"disabled").is_some();
        let mut look = self.el(id).look;
        let c = &mut self.el_mut(id).css;
        let p = colors(c.m_dark_scheme != 0);
        look.dark = c.m_dark_scheme != 0;
        look.disabled = disabled;
        look.native = c.m_appearance != Appearance::appearance_none.index() && !authored_box(c, 0.0);
        button_padding(c, look.native);
        resolve_box(c, look.native, p.button_border, p.button_face);
        resolve_text(c, if look.disabled { p.disabled_text } else { p.control_text });
        c.m_center_content = true;
        self.el_mut(id).look = look;
        if recursive {
            self.compute_children(cx, id);
        }
    }

    /// `el_textarea::compute_styles(recursive)`: the placeholder box gone once there is text, then the look.
    pub fn el_textarea_compute_styles(&mut self, cx: &StyleCx, id: ElementId, recursive: bool) {
        let env = cx.env();
        if self.has_generated(&env, id) && !self.textarea_text(&env, id).is_empty() {
            self.remove_generated(&env, id);
        }
        self.html_tag_compute_styles(cx, id, false);
        let disabled = self.get_attr(id, b"disabled").is_some();
        let mut look = self.el(id).look;
        let c = &mut self.el_mut(id).css;
        let p = colors(c.m_dark_scheme != 0);
        look.dark = c.m_dark_scheme != 0;
        look.disabled = disabled;
        look.native = c.m_appearance != Appearance::appearance_none.index() && !authored_box(c, 0.0);
        resolve_box(c, look.native, p.legacy_field_border, p.field);
        resolve_text(c, if look.disabled { p.disabled_text } else { p.text });
        self.el_mut(id).look = look;
        if recursive {
            self.compute_children(cx, id);
        }
    }

    /// `el_meter::compute_styles(recursive)`.
    pub fn el_meter_compute_styles(&mut self, cx: &StyleCx, id: ElementId, recursive: bool) {
        self.html_tag_compute_styles(cx, id, recursive);
        let c = &self.el(id).css;
        let (dark, native) = (c.m_dark_scheme != 0, c.m_appearance != Appearance::appearance_none.index());
        let look = &mut self.el_mut(id).look;
        look.dark = dark;
        look.native = native;
    }

    /// `el_fieldset::legend()`: the first child when it is a `<legend>` in flow, past spaces and comments.
    fn legend(&self, id: ElementId) -> Option<ElementId> {
        for &child in self.children(id) {
            if self.tag(child) == _legend_ {
                let lc = &self.el(child).css;
                if lc.m_display == StyleDisplay::display_none.index()
                    || lc.m_float != ElementFloat::float_none.index()
                    || lc.m_el_position == ElementPosition::element_position_absolute.index()
                    || lc.m_el_position == ElementPosition::element_position_fixed.index()
                {
                    return None;
                }
                return Some(child);
            }
            if !self.is_white_space(child) && !self.is_comment(child) {
                return None;
            }
        }
        None
    }

    /// `el_fieldset::compute_styles(recursive)`: the legend's margins set so that it sits on the border.
    pub fn el_fieldset_compute_styles(&mut self, cx: &StyleCx, id: ElementId, recursive: bool) {
        self.html_tag_compute_styles(cx, id, recursive);
        let Some(first) = self.legend(id) else { return };
        let c = &self.el(id).css;
        let hidden = BorderStyle::border_style_hidden.index();
        let border = if c.m_css_borders.top.style > hidden { c.m_css_borders.top.width.val() } else { 0.0 };
        let padding = if c.m_css_padding.top.units() == CssUnits::css_units_percentage { 0.0 } else { c.m_css_padding.top.val() };
        let lc = &mut self.el_mut(first).css;
        let lh = lc.m_line_height.computed_value.value();
        let px = |len: &CssLength| if len.units() == CssUnits::css_units_percentage { 0.0 } else { len.val() };
        let lp = &lc.m_css_padding;
        let lb = &lc.m_css_borders;
        let legend_h = lh
            + px(&lp.top)
            + px(&lp.bottom)
            + if lb.top.style > hidden { lb.top.width.val() } else { 0.0 }
            + if lb.bottom.style > hidden { lb.bottom.width.val() } else { 0.0 };
        lc.m_css_margins.top.set_value(-(border + padding), CssUnits::css_units_px);
        lc.m_css_margins.bottom.set_value(padding + cxx::max_f32(0.0, border - legend_h), CssUnits::css_units_px);
    }

    /// `el_image::compute_styles(recursive)`: a tag's, then the image loaded the first time (to redraw when ready
    /// when both its width and height are given).
    pub fn el_image_compute_styles(&mut self, cx: &StyleCx, id: ElementId, recursive: bool) {
        self.html_tag_compute_styles(cx, id, recursive);
        let css = &self.el(id).css;
        let redraw = !css.m_css_height.is_predefined() && !css.m_css_width.is_predefined();
        if let ElementState::Image { src, loaded } = &mut self.el_mut(id).state
            && !src.is_empty()
            && !*loaded
        {
            *loaded = true;
            cx.container.load_image(crate::gumbo::c_str(src), None, redraw);
        }
    }
}
