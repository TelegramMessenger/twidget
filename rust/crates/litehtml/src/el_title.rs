//! `el_title.cpp`: a `<title>`'s text is the container's caption.

use crate::document::{Document, Env};
use crate::element::ElementId;

impl Document {
    /// `el_title::parse_attributes()`: `set_caption` of its text (up to a NUL); its children's attributes are not
    /// parsed.
    pub fn el_title_parse_attributes(&mut self, env: &Env, id: ElementId) {
        let mut text = Vec::new();
        self.get_text(id, &mut text);
        env.container.set_caption(crate::gumbo::c_str(&text));
    }
}
