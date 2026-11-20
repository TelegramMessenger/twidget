//! `render_flex.h` and `render_flex.cpp` as far as the render tree goes: `render_item_flex::init()` (a
//! `render_item_grid`'s too), which makes every child a flex item: block boxes as they are, a run of text in an
//! anonymous block (leading collapsible spaces dropped, trailing spaces cut), any other child in an anonymous
//! block of its own. Lines and their sizing are a later stage's.

use crate::document::{Document, StyleCx};
use crate::element::ElementId;
use crate::render_item::*;

impl Document {
    /// `render_item_flex::init()`.
    pub(crate) fn render_flex_init(&mut self, cx: &StyleCx, this: RenderId) -> RenderId {
        let element = self.render.item(this).element;
        self.add_render(element, this);
        let mut new_children = Vec::new();
        let mut inlines = Vec::new();

        for el in self.render.children(this).to_vec() {
            let el_element = self.render.item(el).element;
            if self.el(el_element).css.m_display == display_inline_text {
                // Text after text, or text that is not collapsible space
                if !inlines.is_empty() || !self.is_white_space(el_element) {
                    inlines.push(el);
                }
            } else {
                self.convert_inlines(cx, this, element, &mut inlines, &mut new_children);
                if self.is_block_box(el_element) {
                    // Add block boxes as is
                    self.render.set_parent(el, Some(this));
                    let made = self.render_init(cx, el);
                    new_children.push(made);
                } else {
                    // Wrap inlines with anonymous block box
                    let made = self.wrapped_flex_item(cx, this, el, el_element);
                    new_children.push(made);
                }
            }
        }
        self.convert_inlines(cx, this, element, &mut inlines, &mut new_children);
        self.set_render_children(this, new_children);

        this
    }

    /// `convert_inlines`: the run of text so far, without its trailing spaces, as an anonymous block's children,
    /// that block's `init()` a flex item.
    #[inline(never)]
    fn convert_inlines(
        &mut self,
        cx: &StyleCx,
        this: RenderId,
        element: ElementId,
        inlines: &mut Vec<RenderId>,
        new_children: &mut Vec<RenderId>,
    ) {
        if inlines.is_empty() {
            return;
        }
        // Find last not space
        if let Some(at) = inlines.iter().rposition(|&el| !self.is_space(self.render.item(el).element)) {
            // Erase all spaces at the end
            inlines.truncate(at + 1);
        }

        let anon_el = self.new_anonymous_tag(cx, element, b"display: block");
        let anon_ri = self.new_render_item(cx, RenderKind::render_item_block, anon_el);
        for &inl in inlines.iter() {
            self.add_render_child(anon_ri, inl);
        }
        self.render.set_parent(anon_ri, Some(this));

        let made = self.render_init(cx, anon_ri);
        new_children.push(made);
        if made != anon_ri {
            self.discard_render(anon_ri);
        }
        inlines.clear();
    }

    /// A child that is neither text nor a block box: an anonymous block of the child's element around what its
    /// `init()` makes it, and that block's `init()`.
    #[inline(never)]
    fn wrapped_flex_item(&mut self, cx: &StyleCx, this: RenderId, el: RenderId, el_element: ElementId) -> RenderId {
        let anon_el = self.new_anonymous_tag(cx, el_element, b"display: block");
        let anon_ri = self.new_render_item(cx, RenderKind::render_item_block, anon_el);
        let inner = self.render_init(cx, el);
        self.add_render_child(anon_ri, inner);
        self.render.set_parent(anon_ri, Some(this));
        let made = self.render_init(cx, anon_ri);
        if made != anon_ri {
            self.discard_render(anon_ri);
        }
        made
    }
}
