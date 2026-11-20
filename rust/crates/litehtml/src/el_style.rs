//! `el_style.cpp`: a `<style>` keeps its text in a list of its own and gives it to the document as a sheet.

use crate::document::Document;
use crate::element::ElementId;

impl Document {
    /// `el_style::parse_attributes()`: the text of its own children as a sheet, with no base url and no media
    /// (`get_attr` of an `el_style`, which is no `html_tag`, has no `media` to give).
    pub fn el_style_parse_attributes(&mut self, id: ElementId) {
        let mut text = Vec::new();
        for &el in &self.el(id).own_children {
            self.get_text(el, &mut text);
        }
        self.add_stylesheet(&text, None, None);
    }
}
