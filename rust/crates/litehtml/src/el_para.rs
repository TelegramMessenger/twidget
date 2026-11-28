//! `el_para.cpp`: a `<p>`'s `align` is its `text-align`.

use crate::document::{Document, Env};
use crate::element::ElementId;
use crate::string_id::_text_align_;

impl Document {
    /// `el_para::parse_attributes()`.
    pub fn el_para_parse_attributes(&mut self, env: &Env, id: ElementId) {
        if let Some(str) = self.get_attr(id, b"align").map(<[u8]>::to_vec) {
            self.add_text_declaration(id, _text_align_, &str);
        }
        self.html_tag_parse_attributes(env, id);
    }
}
