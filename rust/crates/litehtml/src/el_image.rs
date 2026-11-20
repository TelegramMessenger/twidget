//! `el_image.cpp` as a document is built: an `<img>` or `<canvas>` (and a generated image) keeps its `src`, and
//! its `width` and `height` are dimensions; its box is painted as a tag's (stage L13). The image it draws over its
//! box (`object-fit`, `fitted`) is stage L15's: painting reaches it as a seam.

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

    /// `el_image::draw(hdc, x, y, clip, ri)`: the tag's drawing; then, where the clip meets its rounded box and the
    /// box is not empty, the image drawn as a background layer (stage L15: the seam, logged as unported).
    #[allow(clippy::too_many_arguments)]
    pub fn el_image_draw(
        &self,
        cx: &crate::document::StyleCx,
        id: ElementId,
        hdc: u64,
        x: crate::Px,
        y: crate::Px,
        clip: Option<&crate::types::Position>,
        ri: crate::render_item::RenderId,
    ) {
        self.html_tag_draw(cx, id, hdc, x, y, clip, ri);
        let mut pos = self.render.item(ri).pos;
        pos.x += x;
        pos.y += y;
        pos.round(self.paint.snap_scale);
        if pos.does_intersect(clip, false) && pos.width > crate::Px(0.0) && pos.height > crate::Px(0.0) {
            cx.container.paint_unported(hdc, "image");
        }
    }
}
