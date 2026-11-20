//! `el_form.cpp` as a document is built: what the form controls take from their attributes and the boxes they
//! make for what they show (`lh-input-text`, `lh-input-spin`, `lh-range-track` and `lh-range-thumb`,
//! `lh-select-value`, `lh-placeholder`), each an `html_tag` with an inline style. Their look, computed style and
//! painting are later stages'.

use crate::css_parser::{f_componentize, f_remove_whitespace, normalize};
use crate::cxx;
use crate::document::{Document, Env};
use crate::element::{ElementId, ElementKind, ElementState};
use crate::html::{WHITESPACE, lowcase, trim};
use crate::string_id::{_optgroup_, _option_};
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
    fn remove_generated(&mut self, env: &Env, parent: ElementId) {
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
