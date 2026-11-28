//! `el_table.cpp`: a `<table>` keeps only its row groups and caption as children ([`Document::append_child`]), and
//! its presentational attributes map to properties (https://html.spec.whatwg.org/multipage/rendering.html#tables-2).

use crate::document::{Document, Env};
use crate::element::ElementId;
use crate::string_id::{_background_color_, _border_spacing_, _border_width_, _height_, _width_};

impl Document {
    /// `el_table::parse_attributes()`: `width` (not zero), `height`, `cellspacing`, `border` (1 when it is no
    /// number), `bgcolor` (with the container), then `html_tag`'s.
    pub fn el_table_parse_attributes(&mut self, env: &Env, id: ElementId) {
        if let Some(str) = self.get_attr(id, b"width").map(<[u8]>::to_vec) {
            self.map_to_dimension_property_ignoring_zero(id, _width_, &str);
        }
        if let Some(str) = self.get_attr(id, b"height").map(<[u8]>::to_vec) {
            self.map_to_dimension_property(id, _height_, &str);
        }
        if let Some(str) = self.get_attr(id, b"cellspacing").map(<[u8]>::to_vec) {
            self.map_to_pixel_length_property(id, _border_spacing_, &str);
        }
        if let Some(str) = self.get_attr(id, b"border").map(<[u8]>::to_vec) {
            self.map_to_pixel_length_property_with_default_value(id, _border_width_, &str, 1);
        }
        if let Some(str) = self.get_attr(id, b"bgcolor").map(<[u8]>::to_vec) {
            self.add_text_declaration_with_container(id, _background_color_, &str);
        }
        self.html_tag_parse_attributes(env, id);
    }
}
