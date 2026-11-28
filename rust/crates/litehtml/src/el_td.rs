//! `el_td.cpp`: a cell's presentational attributes (https://html.spec.whatwg.org/multipage/rendering.html#tables-2).

use crate::document::{Document, Env};
use crate::element::ElementId;
use crate::string_id::{_background_color_, _background_image_, _height_, _text_align_, _vertical_align_, _width_};

impl Document {
    /// `el_td::parse_attributes()`: `width` and `height` (not zero), `background` as `url('…')` around it as
    /// written, `bgcolor` (with the container), `align`, `valign`, then `html_tag`'s.
    pub fn el_td_parse_attributes(&mut self, env: &Env, id: ElementId) {
        if let Some(str) = self.get_attr(id, b"width").map(<[u8]>::to_vec) {
            self.map_to_dimension_property_ignoring_zero(id, _width_, &str);
        }
        if let Some(str) = self.get_attr(id, b"height").map(<[u8]>::to_vec) {
            self.map_to_dimension_property_ignoring_zero(id, _height_, &str);
        }
        if let Some(str) = self.get_attr(id, b"background").map(<[u8]>::to_vec) {
            let url = [&b"url('"[..], &str, b"')"].concat();
            self.add_text_declaration(id, _background_image_, &url);
        }
        if let Some(str) = self.get_attr(id, b"bgcolor").map(<[u8]>::to_vec) {
            self.add_text_declaration_with_container(id, _background_color_, &str);
        }
        if let Some(str) = self.get_attr(id, b"align").map(<[u8]>::to_vec) {
            self.add_text_declaration(id, _text_align_, &str);
        }
        if let Some(str) = self.get_attr(id, b"valign").map(<[u8]>::to_vec) {
            self.add_text_declaration(id, _vertical_align_, &str);
        }
        self.html_tag_parse_attributes(env, id);
    }
}
