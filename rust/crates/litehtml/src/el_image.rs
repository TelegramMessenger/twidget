//! `el_image.cpp`: an `<img>` or `<canvas>` (and a generated image) keeps its `src`, and its `width` and `height` are
//! dimensions; its box is painted as a tag's (stage L13), then its image over the box as a background layer that the
//! box clips, placed by `object-fit` and `object-position` or by the container's aspect for the source (`fitted`,
//! stage L15). Loading the image (`compute_styles`) is in `el_form.rs`, its size (`get_content_size`) in `layout.rs`.

use crate::Px;
use crate::background::BackgroundLayer;
use crate::css_values::BackgroundRepeat;
use crate::cxx;
use crate::document::{Document, Env, StyleCx};
use crate::element::{ElementId, ElementState};
use crate::html::{WHITESPACE, lowcase, split_string, trim};
use crate::render_item::RenderId;
use crate::string_id::{_height_, _width_};
use crate::types::Position;

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

    /// `m_src`.
    fn el_image_src(&self, id: ElementId) -> &[u8] {
        match &self.el(id).state {
            ElementState::Image { src, .. } => src,
            _ => b"",
        }
    }

    /// `el_image::draw(hdc, x, y, clip, ri)`: the tag's drawing; then, where the clip meets its rounded box and the
    /// box is not empty, the image drawn as a background layer: clipped to the content box, placed by `fitted`, bounded
    /// by the border box with the box's radii, not repeated, with no base url. Drawn whatever `src` is, empty too.
    #[allow(clippy::too_many_arguments)]
    pub fn el_image_draw(&self, cx: &StyleCx, id: ElementId, hdc: u64, x: Px, y: Px, clip: Option<&Position>, ri: RenderId) {
        self.html_tag_draw(cx, id, hdc, x, y, clip, ri);
        let it = self.render.item(ri);
        let mut pos = it.pos;
        pos.x += x;
        pos.y += y;
        pos.round(self.paint.snap_scale);

        // draw image as background
        if pos.does_intersect(clip, false) && pos.width > Px(0.0) && pos.height > Px(0.0) {
            let mut layer =
                BackgroundLayer { clip_box: pos, origin_box: self.el_image_fitted(cx, id, &pos), border_box: pos, ..Default::default() };
            layer.border_box.grow(&it.padding);
            layer.border_box.grow(&it.borders);
            layer.repeat = BackgroundRepeat::background_repeat_no_repeat.index();
            layer.border_radius = self.el(id).css.m_css_borders.radius.calc_percents(layer.border_box.width, layer.border_box.height);
            cx.container.draw_image(hdc, &layer, self.el_image_src(id), b"");
        }
    }

    /// `el_image::fitted(box)`: where the picture goes in `box`, by `object-fit` and `object-position` (their raw
    /// declarations, trimmed and lower-cased); without a fit (or `fill`) by the container's aspect for the source
    /// (`slice` covers, anything else contains, its `xMin`/`xMax`/`yMin`/`yMax` place it), and the box itself when
    /// there is none or the image has no size. A position is keywords, a percentage of the room left, or a length
    /// from the box's corner (read with `strtof`, unit ignored); one value pairs with `center` (a vertical keyword with
    /// itself), and a vertical keyword first swaps the two.
    fn el_image_fitted(&self, cx: &StyleCx, id: ElementId, bx: &Position) -> Position {
        let src = crate::gumbo::c_str(self.el_image_src(id));
        let mut fit = lowcase(&trim(&self.raw_declaration(cx.names, id, b"object-fit"), WHITESPACE));
        let mut aspect = Vec::new();
        if fit.is_empty() || fit == b"fill" {
            aspect = cx.container.get_image_aspect(src);
            if aspect.is_empty() {
                return *bx;
            }
            fit = if contains(&aspect, b"slice") { b"cover".to_vec() } else { b"contain".to_vec() };
        }
        let natural = cx.container.get_image_size(src, None);
        if natural.width <= Px(0.0) || natural.height <= Px(0.0) {
            return *bx;
        }
        let sx = (bx.width / natural.width).value();
        let sy = (bx.height / natural.height).value();
        let mut scale = 1.0f32;
        if fit == b"contain" {
            scale = cxx::min_f32(sx, sy);
        } else if fit == b"cover" {
            scale = cxx::max_f32(sx, sy);
        } else if fit == b"scale-down" {
            scale = cxx::min_f32(1.0, cxx::min_f32(sx, sy));
        }
        let w = natural.width.value() * scale;
        let h = natural.height.value() * scale;
        // object-position: keywords, percentages of the room left, or lengths from the box's corner.
        let mut place = self.raw_declaration(cx.names, id, b"object-position");
        if !aspect.is_empty() {
            let across: &[u8] = if contains(&aspect, b"xmin") {
                b"left"
            } else if contains(&aspect, b"xmax") {
                b"right"
            } else {
                b"center"
            };
            let down: &[u8] = if contains(&aspect, b"ymin") {
                b" top"
            } else if contains(&aspect, b"ymax") {
                b" bottom"
            } else {
                b" center"
            };
            place = [across, down].concat();
        }
        let place = lowcase(&place);
        let mut parts = split_string(&place, b" ", b"", b"\"");
        parts.retain(|s| !s.is_empty());
        let vertical = |s: &[u8]| s == b"top" || s == b"bottom";
        if parts.len() == 1 {
            let second = if vertical(&parts[0]) { parts[0].clone() } else { b"center".to_vec() };
            parts.push(second);
        }
        if parts.len() >= 2 && vertical(&parts[0]) {
            parts.swap(0, 1);
        }
        let offset = |value: &[u8], room: f32| -> f32 {
            if value == b"left" || value == b"top" {
                return 0.0;
            }
            if value == b"right" || value == b"bottom" {
                return room;
            }
            if value.is_empty() || value == b"center" {
                return room / 2.0;
            }
            let v = twidget_cstd::strtof(crate::gumbo::c_str(value)).0;
            if value.last() == Some(&b'%') { room * v / 100.0 } else { v }
        };
        let dx = offset(if parts.len() >= 2 { &parts[0] } else { b"center" }, bx.width.value() - w);
        let dy = offset(if parts.len() >= 2 { &parts[1] } else { b"center" }, bx.height.value() - h);
        Position::new(bx.x + Px(dx), bx.y + Px(dy), Px(w), Px(h))
    }
}

/// `std::string::find(what) != npos`.
fn contains(s: &[u8], what: &[u8]) -> bool {
    s.windows(what.len()).any(|w| w == what)
}
