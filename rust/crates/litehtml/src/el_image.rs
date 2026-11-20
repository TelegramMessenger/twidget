//! `el_image.cpp` as a document is built: an `<img>` or `<canvas>` (and a generated image) keeps its `src`, and
//! its `width` and `height` are dimensions. What it draws and measures is later stages'.

use crate::document::{Document, Env};
use crate::element::{ElementId, ElementState};
use crate::string_id::{_height_, _width_};

impl Document {
    /// `el_image::parse_attributes()`: `m_src` and the size attributes; its children's are not parsed.
    pub fn el_image_parse_attributes(&mut self, _env: &Env, id: ElementId) {
        let src = self.get_attr(id, b"src").unwrap_or(b"").to_vec();
        self.el_mut(id).state = ElementState::Image { src, loaded: false };
        // https://html.spec.whatwg.org/multipage/rendering.html#attributes-for-embedded-content-and-images:the-img-element-5
        if let Some(str) = self.get_attr(id, b"width").map(<[u8]>::to_vec) {
            self.map_to_dimension_property(id, _width_, &str);
        }
        if let Some(str) = self.get_attr(id, b"height").map(<[u8]>::to_vec) {
            self.map_to_dimension_property(id, _height_, &str);
        }
    }
}
