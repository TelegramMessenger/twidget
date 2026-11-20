//! `el_svg.cpp` as a document is built: an `<svg>` marks its document and its `width` and `height` are dimensions.
//! Its shapes, text and painting are stage L14's.

use crate::document::{Document, Env};
use crate::element::ElementId;
use crate::string_id::{_height_, _width_};

impl Document {
    /// `el_svg::parse_attributes()`: `html_tag`'s, then a non-empty `width` and `height`.
    pub fn el_svg_parse_attributes(&mut self, env: &Env, id: ElementId) {
        self.html_tag_parse_attributes(env, id);
        for name in [&b"width"[..], b"height"] {
            if let Some(value) = self.get_attr(id, name).map(<[u8]>::to_vec)
                && !value.is_empty()
            {
                self.map_to_dimension_property(id, if name == b"width" { _width_ } else { _height_ }, &value);
            }
        }
    }
}
