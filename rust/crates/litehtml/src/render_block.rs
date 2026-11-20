//! `render_block.h` and `render_block.cpp` as far as the render tree goes: `render_item_block::init()`, which
//! numbers a list item, splits the inline children around the block boxes inside them, and makes the block its
//! formatting context: a `render_item_block_context` with the runs of inline children wrapped in anonymous
//! blocks when it has a block-level child, else a `render_item_inline_context`. Laying a block out is a later
//! stage's.

use crate::document::{Document, StyleCx};
use crate::render_item::*;

impl Document {
    /// `render_item_block::init()`: what the block becomes.
    pub(crate) fn render_block_init(&mut self, cx: &StyleCx, this: RenderId) -> RenderId {
        let element = self.render.item(this).element;

        // Initialize indexes for list items
        if self.el(element).css.m_display == display_list_item && self.el(element).css.m_list_style_type >= list_style_type_armenian {
            self.number_list_item(cx, element);
        }
        // Split inline blocks with box blocks inside
        let mut k = 0;
        while k < self.render.children(this).len() {
            let child = self.render.children(this)[k];
            if self.render_display(child) == display_inline
                && !self.render.children(child).is_empty()
                && let Some((first, block, last)) = self.split_inlines(cx, child)
            {
                for part in [first, block, last] {
                    self.render.hold(part);
                }
                self.render_children_splice(this, k, &[first, block, last]);
                self.release_render(child);
                for part in [first, block, last] {
                    self.render.set_parent(part, Some(this));
                }
                continue;
            }
            k += 1;
        }

        let mut has_block_level = false;
        let mut has_inlines = false;
        for &child in self.render.children(this) {
            let el = self.render.item(child).element;
            if !self.is_float(el) {
                if self.is_block_box(el) {
                    has_block_level = true;
                } else if self.is_inline(el) {
                    has_inlines = true;
                }
            }
            if has_block_level && has_inlines {
                break;
            }
        }
        let parent = self.render.parent(this);
        let ret = if has_block_level {
            let ret = self.new_render_item(cx, RenderKind::render_item_block_context, element);
            self.render.set_parent(ret, parent);
            let mut new_children = Vec::new();
            let mut inlines = Vec::new();
            let mut not_ws_added = false;
            for child in self.render.children(this).to_vec() {
                let el = self.render.item(child).element;
                if self.is_inline(el) {
                    inlines.push(child);
                    if !self.is_white_space(el) {
                        not_ws_added = true;
                    }
                } else {
                    if not_ws_added {
                        let anon = self.anonymous_block(cx, element, &inlines, ret);
                        not_ws_added = false;
                        new_children.push(anon);
                    }
                    new_children.push(child);
                    self.render.set_parent(child, Some(ret));
                    inlines.clear();
                }
            }
            if !inlines.is_empty() && not_ws_added {
                let anon = self.anonymous_block(cx, element, &inlines, ret);
                new_children.push(anon);
            }
            self.set_render_children(ret, new_children);
            ret
        } else {
            let ret = self.new_render_item(cx, RenderKind::render_item_inline_context, element);
            self.render.set_parent(ret, parent);
            let children = self.render.children(this).to_vec();
            self.set_render_children(ret, children.clone());
            for child in children {
                self.render.set_parent(child, Some(ret));
            }
            ret
        };

        self.add_render(element, ret);

        for k in 0..self.render.children(ret).len() {
            self.init_render_child(cx, ret, k);
        }
        ret
    }

    /// A list item's number as an attribute, `list_index`: the parent's `start` (1 without one) and one more for each
    /// list item before it among its parent's children, up to `INT_MAX`. An item its parent does not list gets none.
    #[inline(never)]
    fn number_list_item(&mut self, cx: &StyleCx, element: crate::element::ElementId) {
        let Some(p) = self.parent(element) else { return };
        let start = self.get_attr(p, b"start").unwrap_or(b"1");
        let mut val = twidget_cstd::atoi(crate::gumbo::c_str(start));
        let mut index = None;
        for &child in self.children(p) {
            if child == element {
                index = Some(val);
                break;
            }
            if self.el(child).css.m_display == display_list_item && val < i32::MAX {
                val += 1;
            }
        }
        if let Some(val) = index {
            self.set_attr(cx.names, element, b"list_index", val.to_string().as_bytes());
        }
    }

    /// An anonymous block (`html_tag(src_el())`, `display: block`) around `inlines`, its parent `parent`.
    #[inline(never)]
    fn anonymous_block(&mut self, cx: &StyleCx, element: crate::element::ElementId, inlines: &[RenderId], parent: RenderId) -> RenderId {
        let anon_el = self.new_anonymous_tag(cx, element, b"display: block");
        let anon_ri = self.new_render_item(cx, RenderKind::render_item_block, anon_el);
        for &inl in inlines {
            self.add_render_child(anon_ri, inl);
        }
        self.render.set_parent(anon_ri, Some(parent));
        anon_ri
    }
}
