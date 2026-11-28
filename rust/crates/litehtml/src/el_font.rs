//! `el_font.cpp`: a `<font>`'s `color`, `face` and `size`.

use crate::document::{Document, Env};
use crate::element::ElementId;
use crate::string_id::{_color_, _font_family_, _font_size_};

impl Document {
    /// `el_font::parse_attributes()`: `color` (with the container), `face` as `font-family`, and `size` as a
    /// keyword: 1 and below `x-small`, 6 and above `xx-large`, a signed size relative to 3 after a clamp to ±7.
    pub fn el_font_parse_attributes(&mut self, env: &Env, id: ElementId) {
        if let Some(str) = self.get_attr(id, b"color").map(<[u8]>::to_vec) {
            self.add_text_declaration_with_container(id, _color_, &str);
        }
        if let Some(str) = self.get_attr(id, b"face").map(<[u8]>::to_vec) {
            self.add_text_declaration(id, _font_family_, &str);
        }
        if let Some(str) = self.get_attr(id, b"size").map(<[u8]>::to_vec) {
            let mut sz = twidget_cstd::atoi(&str);
            if str.first() == Some(&b'+') || str.first() == Some(&b'-') {
                // relative size
                sz = 3 + (-7).max(sz.min(7));
            }
            let size: Option<&[u8]> = if sz <= 1 {
                Some(b"x-small")
            } else if sz >= 6 {
                Some(b"xx-large")
            } else {
                match sz {
                    2 => Some(b"small"),
                    3 => Some(b"medium"),
                    4 => Some(b"large"),
                    5 => Some(b"x-large"),
                    _ => None,
                }
            };
            if let Some(size) = size {
                self.add_text_declaration(id, _font_size_, size);
            }
        }
        self.html_tag_parse_attributes(env, id);
    }
}
