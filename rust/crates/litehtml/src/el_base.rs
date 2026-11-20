//! `el_base.cpp`: a `<base>` tells the container its `href`.

use crate::document::{Document, Env};
use crate::element::ElementId;

impl Document {
    /// `el_base::parse_attributes()`: `set_base_url(href)`; its children's attributes are not parsed.
    pub fn el_base_parse_attributes(&mut self, env: &Env, id: ElementId) {
        env.container.set_base_url(self.get_attr(id, b"href"));
    }
}
