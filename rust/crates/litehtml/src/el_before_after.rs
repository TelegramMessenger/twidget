//! `el_before_after.cpp`: a `::before` or `::after` box takes a rule's style like any tag, and its `content` as
//! the boxes it holds: text (escapes read), quotes from the inherited `quotes`, `attr()`, `counter()`,
//! `counters()` and images, each box and byte counted against the document's limits for generated content.

use crate::css_parser::{f_componentize, f_remove_whitespace, normalize};
use crate::css_tokenizer::{CV_FUNCTION, IDENT, STRING, get_repr};
use crate::css_values::{ContentProperty, CssValues, content_property_strings};
use crate::document::{Document, StyleCx};
use crate::element::{ElementId, ElementKind, ElementState};
use crate::html::{WHITESPACE, lowcase, split_string, trim};
use crate::string_id::*;
use crate::style::{Style, Value, parse_url};
use crate::utf8_strings::utf32_to_utf8;

/// `max_generations`: content a box makes at most this many times between two styles.
const max_generations: i32 = 8;

impl Document {
    fn generated(&self, id: ElementId) -> (i32, i32, usize) {
        match self.el(id).state {
            ElementState::Generated { generations, boxes, bytes } => (generations, boxes, bytes),
            _ => (0, 0, 0),
        }
    }

    fn set_generated(&mut self, id: ElementId, generations: i32, boxes: i32, bytes: usize) {
        self.el_mut(id).state = ElementState::Generated { generations, boxes, bytes };
    }

    /// `el_before_after_base::add_style(style)`: the style as a tag takes it, then, for a rule with content (at most
    /// eight times since the style was empty), the boxes the content makes in place of the ones before (which stay
    /// when it makes none).
    pub fn before_after_add_style(&mut self, cx: &StyleCx, id: ElementId, style: &Style) {
        if self.el(id).style.empty() {
            let (_, boxes, bytes) = self.generated(id);
            self.set_generated(id, 0, boxes, bytes);
        }
        self.html_tag_add_style(cx, id, style);

        let Value::String(content) = &style.get_property(_content_).value else { return };
        let (generations, boxes, bytes) = self.generated(id);
        if content.is_empty() || generations >= max_generations {
            return;
        }
        self.set_generated(id, generations + 1, boxes, bytes);

        let children = std::mem::take(&mut self.el_mut(id).children);
        self.set_generated(id, generations + 1, 0, 0);

        let idx = CssValues::new(&content_property_strings).value_index(content);
        if idx == Some(ContentProperty::content_property_open_quote.index())
            || idx == Some(ContentProperty::content_property_close_quote.index())
        {
            self.add_quote(cx, id, idx == Some(ContentProperty::content_property_open_quote.index()));
        } else if idx.is_none() {
            // The value is kept as written: read it with the tokenizer, as the sanitizer did, so that an escaped
            // quote stays inside its string and only a url token or url() names an image.
            for tok in normalize(content, f_componentize | f_remove_whitespace, None) {
                if self.generated_limited {
                    break;
                }
                if tok.ty == STRING {
                    self.add_text(cx, id, &tok.str, false);
                } else if tok.ty == IDENT {
                    let name = lowcase(tok.name());
                    if name == b"open-quote" || name == b"close-quote" {
                        self.add_quote(cx, id, name == b"open-quote");
                    }
                } else if let Some(url) = parse_url(&tok) {
                    self.add_image(cx, id, &url);
                } else if tok.ty == CV_FUNCTION {
                    self.add_function(cx, id, &lowcase(tok.name()), &get_repr(&tok.value, 0, -1, false));
                }
            }
        }

        if self.el(id).children.is_empty() {
            self.el_mut(id).children = children;
            let (generations, _, _) = self.generated(id);
            self.set_generated(id, generations, boxes, bytes);
        } else {
            self.release_generated(boxes, bytes);
        }
    }

    /// `add_generated(el, bytes)`: the box appended when the document has room for it.
    fn add_generated(&mut self, cx: &StyleCx, id: ElementId, el: ElementId, bytes: usize) -> bool {
        if !self.take_generated(&cx.env(), bytes) {
            return false;
        }
        let (generations, boxes, total) = self.generated(id);
        self.set_generated(id, generations, boxes + 1, total + bytes);
        self.html_tag_append_child(id, el);
        true
    }

    /// `add_text(txt, escapes)`: words and spaces (each whitespace character a space of its own), with CSS escapes
    /// (a backslash and up to four letters or digits) read when `escapes`.
    pub fn add_text(&mut self, cx: &StyleCx, id: ElementId, txt: &[u8], escapes: bool) {
        let mut word: Vec<u8> = Vec::new();
        let mut esc: Vec<u8> = Vec::new();

        for &chr in txt {
            if escapes && (chr == b'\\' || (!esc.is_empty() && esc.len() < 5 && chr.is_ascii_alphanumeric())) {
                if !esc.is_empty() && chr == b'\\' {
                    word.extend(convert_escape(&esc[1..]));
                    esc.clear();
                }
                esc.push(chr);
            } else {
                if !esc.is_empty() {
                    word.extend(convert_escape(&esc[1..]));
                    esc.clear();
                }
                if twidget_cstd::is_space(chr) {
                    if !word.is_empty() {
                        let el = self.new_text(&word, false);
                        if !self.add_generated(cx, id, el, word.len()) {
                            return;
                        }
                        word.clear();
                    }
                    word.push(chr);
                    let el = self.new_text(&word, true);
                    if !self.add_generated(cx, id, el, word.len()) {
                        return;
                    }
                    word.clear();
                } else {
                    word.push(chr);
                }
            }
        }

        if !esc.is_empty() {
            word.extend(convert_escape(&esc[1..]));
        }
        if !word.is_empty() {
            let el = self.new_text(&word, false);
            self.add_generated(cx, id, el, word.len());
        }
    }

    /// `add_quote(open)`: a mark of the nearest `quotes` an ancestor declares (`none` makes none; `auto`, nothing
    /// usable or no ancestor, English marks), the pair chosen by how many `<q>` enclose the element's parent.
    fn add_quote(&mut self, cx: &StyleCx, id: ElementId, open: bool) {
        let mut marks: Vec<Vec<u8>> = Vec::new();
        let mut el = self.parent(id);
        while let Some(e) = el {
            if !marks.is_empty() {
                break;
            }
            let value = if self.kind(e).is_html_tag() { self.raw_declaration(cx.names, e, b"quotes") } else { Vec::new() };
            let value = trim(&value, WHITESPACE);
            if value.is_empty() || value == b"inherit" || value == b"unset" {
                el = self.parent(e);
                continue;
            }
            if value == b"none" {
                return;
            }
            let mut i = 0;
            while i < value.len() {
                if value[i] != b'"' && value[i] != b'\'' {
                    i += 1;
                    continue;
                }
                let Some(end) = value[i + 1..].iter().position(|c| *c == value[i]).map(|k| i + 1 + k) else { break };
                marks.push(value[i + 1..end].to_vec());
                i = end + 1;
            }
            break;
        }
        // auto, or nothing usable: English marks, as browsers use for lang=en.
        if marks.len() < 2 {
            marks = vec![b"\xE2\x80\x9C".to_vec(), b"\xE2\x80\x9D".to_vec(), b"\xE2\x80\x98".to_vec(), b"\xE2\x80\x99".to_vec()];
        }
        let mut depth = 0;
        if let Some(q) = self.parent(id) {
            let mut el = self.parent(q);
            while let Some(e) = el {
                depth += usize::from(self.tag(e) == _q_);
                el = self.parent(e);
            }
        }
        let pair = depth.min(marks.len() / 2 - 1);
        let mark = marks[pair * 2 + if open { 0 } else { 1 }].clone();
        self.add_text(cx, id, &mark, true);
    }

    /// `add_function(fnc, params)`: `attr(name)` of the parent, `counter(name)`, `counters(name, separator)`.
    fn add_function(&mut self, cx: &StyleCx, id: ElementId, fnc: &[u8], params: &[u8]) {
        match fnc {
            // attr
            b"attr" => {
                let p_name = lowcase(&trim(params, WHITESPACE));
                if let Some(el_parent) = self.parent(id)
                    && let Some(attr_value) = self.get_attr(el_parent, crate::gumbo::c_str(&p_name)).map(<[u8]>::to_vec)
                {
                    self.add_text(cx, id, &attr_value, false);
                }
            }
            // counter
            b"counter" => {
                let value = self.get_counter_value(cx.names, id, params);
                self.add_text(cx, id, &value, true);
            }
            // counters
            b"counters" => {
                let tokens: Vec<Vec<u8>> = split_string(params, b",", b"", b"\"").iter().map(|t| trim(t, WHITESPACE)).collect();
                let value = self.get_counters_value(cx.names, id, &tokens);
                self.add_text(cx, id, &value, true);
            }
            _ => {}
        }
    }

    /// `add_image(url)`: an `<img>` of the url, an inline block, its attributes parsed.
    fn add_image(&mut self, cx: &StyleCx, id: ElementId, url: &[u8]) {
        if url.is_empty() {
            return;
        }
        let el = self.new_element(ElementKind::el_image);
        self.set_attr(cx.names, el, b"src", url);
        self.set_attr(cx.names, el, b"style", b"display:inline-block");
        self.set_tag_name(cx.names, el, b"img");
        if !self.add_generated(cx, id, el, url.len()) {
            return;
        }
        self.parse_attributes(&cx.env(), el);
        self.flush_declarations(cx);
    }
}

/// `convert_escape(txt)`: the hexadecimal number `strtol` reads at the start (a `0x` may lead it), as UTF-8, up to
/// a NUL.
fn convert_escape(txt: &[u8]) -> Vec<u8> {
    let mut digits = txt;
    if txt.len() >= 3 && txt[0] == b'0' && (txt[1] | 0x20) == b'x' && txt[2].is_ascii_hexdigit() {
        digits = &txt[2..];
    }
    let mut value: u32 = 0;
    for &c in digits {
        let Some(d) = (c as char).to_digit(16) else { break };
        value = value.wrapping_mul(16).wrapping_add(d);
    }
    let out = utf32_to_utf8(&[value]);
    crate::gumbo::c_str(&out).to_vec()
}
