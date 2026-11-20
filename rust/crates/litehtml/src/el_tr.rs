//! `el_tr.cpp`: a row's presentational attributes (https://html.spec.whatwg.org/multipage/rendering.html#tables-2).

use crate::document::{Document, Env};
use crate::element::ElementId;
use crate::string_id::{_background_color_, _height_, _text_align_, _vertical_align_};

impl Document {
    /// `el_tr::parse_attributes()`: `height`, `align`, `valign`, `bgcolor` (with the container), then `html_tag`'s.
    pub fn el_tr_parse_attributes(&mut self, env: &Env, id: ElementId) {
        if let Some(str) = self.get_attr(id, b"height").map(<[u8]>::to_vec) {
            self.map_to_dimension_property(id, _height_, &str);
        }
        if let Some(str) = self.get_attr(id, b"align").map(<[u8]>::to_vec) {
            self.add_text_declaration(id, _text_align_, &str);
        }
        if let Some(str) = self.get_attr(id, b"valign").map(<[u8]>::to_vec) {
            self.add_text_declaration(id, _vertical_align_, &str);
        }
        if let Some(str) = self.get_attr(id, b"bgcolor").map(<[u8]>::to_vec) {
            self.add_text_declaration_with_container(id, _background_color_, &str);
        }
        self.html_tag_parse_attributes(env, id);
    }
}
