//! `el_text.cpp` and `el_space.cpp` as the cascade has them: a text takes its parent's font, line height and text
//! properties, its text as `text-transform` and white-space make it, and its measure in the parent's font; and its
//! drawing (stage L13): each piece in its parent's font and colour, an ellipsis's cut text cut.

use crate::Px;
use crate::css_values::{ElementFloat, ElementPosition, StyleDisplay, TextTransform, WhiteSpace};
use crate::document::{Document, StyleCx};
use crate::element::ElementId;
use crate::render_item::RenderId;
use crate::types::FontMetrics;

impl Document {
    /// `el_space::is_white_space()`: in a `normal`, `nowrap` or `pre-line` white-space.
    pub fn el_space_is_white_space(&self, id: ElementId) -> bool {
        let ws = self.el(id).css.m_white_space;
        ws == WhiteSpace::white_space_normal.index()
            || ws == WhiteSpace::white_space_nowrap.index()
            || ws == WhiteSpace::white_space_pre_line.index()
    }

    /// `el_space::is_break()`: a newline in a `pre`, `pre-line` or `pre-wrap` white-space.
    pub fn el_space_is_break(&self, id: ElementId) -> bool {
        let ws = self.el(id).css.m_white_space;
        (ws == WhiteSpace::white_space_pre.index()
            || ws == WhiteSpace::white_space_pre_line.index()
            || ws == WhiteSpace::white_space_pre_wrap.index())
            && self.el(id).text == b"\n"
    }

    /// `el_text::compute_styles()`: the parent's line height, font and metrics, white-space, wrapping and
    /// transform; inline text, no float; a relatively positioned inline ancestor's offsets (and then static, as
    /// any text below a box is); the text as shown (a collapsible space one space, a tab four, a newline nothing,
    /// else as transformed by the container); and its size in the parent's font, measured again only when the
    /// font or the transform changed.
    #[inline(never)]
    pub fn el_text_compute_styles(&mut self, cx: &StyleCx, id: ElementId) {
        self.mark_element_layout_dirty(cx, id);
        let el_parent = self.parent(id);
        if let Some(parent) = el_parent {
            let pc = &self.el(parent).css;
            let line_height = pc.m_line_height.clone();
            let (font, metrics) = (pc.m_font, pc.m_font_metrics);
            let (white_space, overflow_wrap, word_break, text_transform) =
                (pc.m_white_space, pc.m_overflow_wrap, pc.m_word_break, pc.m_text_transform);
            let css = &mut self.el_mut(id).css;
            css.m_line_height = line_height;
            css.m_font = font;
            css.m_font_metrics = metrics;
            css.m_white_space = white_space;
            css.m_overflow_wrap = overflow_wrap;
            css.m_word_break = word_break;
            css.m_text_transform = text_transform;
        }
        self.el_mut(id).css.m_display = StyleDisplay::display_inline_text.index();
        self.el_mut(id).css.m_float = ElementFloat::float_none.index();

        let text_transform = self.el(id).css.m_text_transform;
        if text_transform != TextTransform::text_transform_none.index() {
            let mut transformed = self.el(id).text.clone();
            cx.container.transform_text(&mut transformed, text_transform);
            let state = &mut self.el_mut(id).text_state;
            state.transformed_text = transformed;
            state.use_transformed = true;
        } else {
            self.el_mut(id).text_state.use_transformed = false;
        }

        let mut p = self.parent(id);
        while let Some(e) = p {
            if self.el(e).css.m_display != StyleDisplay::display_inline.index() {
                break;
            }
            if self.el(e).css.m_el_position == ElementPosition::element_position_relative.index() {
                let offsets = self.el(e).css.m_css_offsets.clone();
                let css = &mut self.el_mut(id).css;
                css.m_css_offsets = offsets;
                css.m_el_position = ElementPosition::element_position_relative.index();
                break;
            }
            p = self.parent(e);
        }
        if p.is_some() {
            self.el_mut(id).css.m_el_position = ElementPosition::element_position_static.index();
        }

        let white_space = self.is_white_space(id);
        let text = self.el(id).text.clone();
        let state = &mut self.el_mut(id).text_state;
        if white_space {
            state.transformed_text = b" ".to_vec();
            state.use_transformed = true;
        } else {
            if text == b"\t" {
                state.transformed_text = b"    ".to_vec();
                state.use_transformed = true;
            }
            if text == b"\n" || text == b"\r" {
                state.transformed_text = Vec::new();
                state.use_transformed = true;
            }
        }

        let (font, fm) = match el_parent {
            Some(parent) => (self.el(parent).css.m_font, self.el(parent).css.m_font_metrics),
            None => (0, FontMetrics::default()),
        };
        let is_break = self.is_break(id);
        let css_transform = self.el(id).css.m_text_transform;
        let state = &self.el(id).text_state;
        if is_break || font == 0 {
            let state = &mut self.el_mut(id).text_state;
            state.size = (Px(0.0), Px(0.0));
            state.measured_font = 0;
        } else {
            let transform = if state.use_transformed { if white_space { -2 } else { css_transform } } else { -3 };
            let (measured_font, measured_transform) = (state.measured_font, state.measured_transform);
            let shown = if state.use_transformed { state.transformed_text.clone() } else { text };
            let state = &mut self.el_mut(id).text_state;
            state.size.1 = fm.height;
            if measured_font != font || measured_transform != transform {
                let breaker = std::mem::take(&mut state.breaker);
                if breaker != 0 {
                    cx.container.drop_text_breaker(breaker);
                }
                let state = &mut self.el_mut(id).text_state;
                state.size.0 = cx.container.text_width(crate::gumbo::c_str(&shown), font);
                state.measured_font = font;
                state.measured_transform = transform;
            }
        }
        self.el_mut(id).text_state.draw_spaces = fm.draw_spaces;
    }

    /// `display_text()`: the text as transformed, or as written.
    pub fn display_text(&self, id: ElementId) -> &[u8] {
        let el = self.el(id);
        if el.text_state.use_transformed { &el.text_state.transformed_text } else { &el.text }
    }

    /// `breaker()`: the text's breaker, which the container makes once (of the shown text in the text's font) and
    /// the text keeps until its font or transform changes; 0 when the container makes none.
    pub fn breaker(&mut self, cx: &StyleCx, id: ElementId) -> u64 {
        if self.el(id).text_state.breaker == 0 {
            let font = self.el(id).css.m_font;
            let made = cx.container.create_text_breaker(self.display_text(id), font);
            self.el_mut(id).text_state.breaker = made;
        }
        self.el(id).text_state.breaker
    }

    /// `el_text::draw(hdc, x, y, clip, ri)`: nothing for collapsible white space the font does not draw; a text cut
    /// into pieces draws each; else its text (a piece's own, cut where an ellipsis cut it) in its parent's font and
    /// colour at its box snapped vertically, when the clip meets the box grown by half its height (at least 4) and
    /// its parent's text shadows.
    #[allow(clippy::too_many_arguments)]
    pub fn el_text_draw(&self, cx: &StyleCx, id: ElementId, hdc: u64, x: Px, y: Px, clip: Option<&crate::types::Position>, ri: RenderId) {
        if self.is_white_space(id) && !self.el(id).text_state.draw_spaces {
            return;
        }
        let item = self.render.item(ri);
        let text_item = item.text.as_deref();
        if let Some(text) = text_item
            && !text.fragments.is_empty()
        {
            for &fragment in &text.fragments {
                self.el_text_draw(cx, id, hdc, x, y, clip, fragment);
            }
            return;
        }

        let mut pos = item.pos;
        pos.x += x;
        pos.y += y;
        {
            let k = self.paint.snap_scale;
            pos.y = Px((pos.y.value() * k).round() / k);
        }

        let mut ink = pos;
        {
            let mut slack = Px(4.0).max(ink.height / Px(2.0));
            if let Some(owner) = self.parent(id) {
                let mut reach = 0.0f32;
                for shadow in &self.el(owner).css.m_text_shadows {
                    reach = crate::cxx::max_f32(
                        reach,
                        shadow.blur.val().abs() * 2.0
                            + shadow.spread.val().abs()
                            + crate::cxx::max_f32(shadow.x.val().abs(), shadow.y.val().abs()),
                    );
                }
                slack += Px(reach);
            }
            ink.x -= slack;
            ink.y -= slack;
            ink.width += slack * Px(2.0);
            ink.height += slack * Px(2.0);
        }
        if ink.does_intersect(clip, false)
            && let Some(el_parent) = self.parent(id)
        {
            let font = self.el(el_parent).css.m_font;
            if font != 0 {
                let color = self.el(el_parent).css.m_color;
                let shown: &[u8] = match text_item {
                    Some(text) if text.is_fragment => &text.fragment_text,
                    _ => self.display_text(id),
                };
                match text_item {
                    Some(text) if text.cut >= 0 => {
                        // text-overflow: ellipsis cut it (render_item_inline_context::apply_ellipsis).
                        if text.cut > 0 {
                            let cut = (text.cut as usize).min(shown.len());
                            cx.container.draw_text(hdc, crate::gumbo::c_str(&shown[..cut]), font, color, &pos);
                        }
                    }
                    _ => cx.container.draw_text(hdc, crate::gumbo::c_str(shown), font, color, &pos),
                }
            }
        }
    }
}
