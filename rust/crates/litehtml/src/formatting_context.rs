//! `formatting_context.h` and `formatting_context.cpp`: a block formatting context's floats while its boxes are laid
//! out. The left floats are kept by their right edges, the right floats by their left edges, each list in the order
//! `add_float` puts them; the line left and right of one `y` are cached until a float comes or goes. Positions are
//! the context's own: `push_position`/`pop_position` move the origin a box inside it lays out from.

use crate::Px;
use crate::css_values::{ElementClear, ElementFloat};
use crate::document::Document;
use crate::render_item::RenderId;
use crate::types::{ContainingBlockContext, Margins, Position};

const float_none: i32 = ElementFloat::float_none.index();
const float_left: i32 = ElementFloat::float_left.index();
const float_right: i32 = ElementFloat::float_right.index();
const clear_left: i32 = ElementClear::clear_left.index();
const clear_right: i32 = ElementClear::clear_right.index();
const clear_both: i32 = ElementClear::clear_both.index();

/// `floated_box`: a float placed in the context.
#[derive(Clone, Copy, Debug)]
pub struct FloatedBox {
    pub pos: Position,
    pub float_side: i32,
    pub clear_floats: i32,
    pub context: i32,
    pub min_width: Px,
    pub el: RenderId,
}

/// `pixel_pixel_cache`: the answer for one `y`.
#[derive(Clone, Copy, Debug, Default)]
pub struct PixelCache {
    pub hash: Px,
    pub val: Px,
    pub is_valid: bool,
    pub is_default: bool,
}

impl PixelCache {
    /// `invalidate()`.
    pub fn invalidate(&mut self) {
        self.is_valid = false;
        self.is_default = false;
    }

    /// `set_value(hash, val)`.
    pub fn set_value(&mut self, hash: Px, val: Px) {
        self.hash = hash;
        self.val = val;
        self.is_valid = true;
    }
}

/// `formatting_context::new_position`: where a box goes (`found` false: where it was asked), and the width it has.
#[derive(Clone, Copy, Debug, Default)]
pub struct NewPosition {
    pub found: bool,
    pub new_line: bool,
    pub top: Px,
    pub left: Px,
    pub width: Px,
}

/// `formatting_context::el_position`: a box with its margins, and the width of its container.
#[derive(Clone, Copy, Debug, Default)]
pub struct ElPosition {
    pub el_margins: Margins,
    pub el_pos: Position,
    pub container_width: Px,
}

/// `formatting_context`.
#[derive(Clone, Debug, Default)]
pub struct FormattingContext {
    pub floats_left: Vec<FloatedBox>,
    pub floats_right: Vec<FloatedBox>,
    pub cache_line_left: PixelCache,
    pub cache_line_right: PixelCache,
    pub current_top: Px,
    pub current_left: Px,
}

impl FormattingContext {
    /// `push_position(x, y)`.
    pub fn push_position(&mut self, x: Px, y: Px) {
        self.current_left += x;
        self.current_top += y;
    }

    /// `pop_position(x, y)`.
    pub fn pop_position(&mut self, x: Px, y: Px) {
        self.current_left -= x;
        self.current_top -= y;
    }

    /// `add_float(el, min_width, context)`: the laid-out float in its list, before the first float it reaches past
    /// (a left float further right, a right float further left).
    pub fn add_float(&mut self, doc: &Document, el: RenderId, min_width: Px, context: i32) {
        let item = doc.render.item(el);
        let css = &doc.el(item.element).css;
        let fb = FloatedBox {
            pos: Position::new(item.left() + self.current_left, item.top() + self.current_top, item.width(), item.height()),
            float_side: css.m_float,
            clear_floats: css.m_clear,
            context,
            min_width,
            el,
        };
        if fb.float_side == float_left {
            if self.floats_left.is_empty() {
                self.floats_left.push(fb);
            } else {
                match self.floats_left.iter().position(|i| fb.pos.right() > i.pos.right()) {
                    Some(at) => self.floats_left.insert(at, fb),
                    None => self.floats_left.push(fb),
                }
            }
            self.cache_line_left.invalidate();
        } else if fb.float_side == float_right {
            if self.floats_right.is_empty() {
                self.floats_right.push(fb);
            } else {
                match self.floats_right.iter().position(|i| fb.pos.left() < i.pos.left()) {
                    Some(at) => self.floats_right.insert(at, fb),
                    None => self.floats_right.push(fb),
                }
            }
            self.cache_line_right.invalidate();
        }
    }

    /// `get_floats_height(el_float)`: the bottom of every float (`float_none`), or the top of the floats that clear
    /// the side, from the context's top.
    pub fn get_floats_height(&self, el_float: i32) -> Px {
        let mut h = self.current_top;
        for fb in self.floats_left.iter().chain(&self.floats_right) {
            let process = if el_float == float_none {
                true
            } else if el_float == float_left {
                fb.clear_floats == clear_left || fb.clear_floats == clear_both
            } else if el_float == float_right {
                fb.clear_floats == clear_right || fb.clear_floats == clear_both
            } else {
                false
            };
            if process {
                if el_float == float_none {
                    h = h.max(fb.pos.bottom());
                } else {
                    h = h.max(fb.pos.top());
                }
            }
        }
        h - self.current_top
    }

    /// `get_left_floats_height()`.
    pub fn get_left_floats_height(&self) -> Px {
        let mut h = Px(0.0);
        for fb in &self.floats_left {
            h = h.max(fb.pos.bottom());
        }
        h - self.current_top
    }

    /// `get_right_floats_height()`.
    pub fn get_right_floats_height(&self) -> Px {
        let mut h = Px(0.0);
        for fb in &self.floats_right {
            h = h.max(fb.pos.bottom());
        }
        h - self.current_top
    }

    /// `get_line_left(y)`: the right edge of the left floats at `y`, not below 0.
    pub fn get_line_left(&mut self, y: Px) -> Px {
        let y = y + self.current_top;

        if self.cache_line_left.is_valid && self.cache_line_left.hash == y {
            if self.cache_line_left.val - self.current_left < Px(0.0) {
                return Px(0.0);
            }
            return self.cache_line_left.val - self.current_left;
        }

        let mut w = Px(0.0);
        for fb in &self.floats_left {
            if y >= fb.pos.top() && y < fb.pos.bottom() {
                w = w.max(fb.pos.right());
                if w < fb.pos.right() {
                    break;
                }
            }
        }
        self.cache_line_left.set_value(y, w);
        w -= self.current_left;
        if w < Px(0.0) {
            return Px(0.0);
        }
        w
    }

    /// `get_line_left_right(y, def_right, ln_left, ln_right)`.
    pub fn get_line_left_right(&mut self, y: Px, def_right: Px) -> (Px, Px) {
        let left = self.get_line_left(y);
        let right = self.get_line_right(y, def_right);
        (left, right)
    }

    /// `get_line_right(y, def_right)`: the left edge of the right floats at `y`, or `def_right`.
    pub fn get_line_right(&mut self, y: Px, def_right: Px) -> Px {
        let y = y + self.current_top;
        let def_right = def_right + self.current_left;
        if self.cache_line_right.is_valid && self.cache_line_right.hash == y {
            if self.cache_line_right.is_default {
                return def_right - self.current_left;
            }
            let w = self.cache_line_right.val.min(def_right) - self.current_left;
            if w < Px(0.0) {
                return Px(0.0);
            }
            return w;
        }

        let mut w = def_right;
        self.cache_line_right.is_default = true;
        for fb in &self.floats_right {
            if y >= fb.pos.top() && y < fb.pos.bottom() {
                w = w.min(fb.pos.left());
                self.cache_line_right.is_default = false;
                if w > fb.pos.left() {
                    break;
                }
            }
        }
        self.cache_line_right.set_value(y, w);
        let w = w - self.current_left;
        if w < Px(0.0) {
            return Px(0.0);
        }
        w
    }

    /// `clear_floats(context)`: the floats of this context and the ones inside it go.
    pub fn clear_floats(&mut self, context: i32) {
        let before = self.floats_left.len();
        self.floats_left.retain(|fb| fb.context < context);
        if self.floats_left.len() != before {
            self.cache_line_left.invalidate();
        }
        let before = self.floats_right.len();
        self.floats_right.retain(|fb| fb.context < context);
        if self.floats_right.len() != before {
            self.cache_line_right.invalidate();
        }
    }

    /// `get_cleared_top(el, line_top)`: below the floats the box's `clear` (or, for a float, its side's clearing
    /// floats) asks to be below.
    pub fn get_cleared_top(&self, doc: &Document, el: RenderId, line_top: Px) -> Px {
        let css = &doc.el(doc.render.item(el).element).css;
        let mut line_top = line_top;
        let clear = css.m_clear;
        let fh = if clear == clear_left {
            Some(self.get_left_floats_height())
        } else if clear == clear_right {
            Some(self.get_right_floats_height())
        } else if clear == clear_both {
            Some(self.get_floats_height(float_none))
        } else if css.m_float != float_none {
            Some(self.get_floats_height(css.m_float))
        } else {
            None
        };
        if let Some(fh) = fh
            && fh != Px(0.0)
            && fh > line_top
        {
            line_top = fh;
        }
        line_top
    }

    /// `place_to_left(el_pos)`: the box moved right past the left floats on its line, and down below them (or the
    /// right floats it meets) when it no longer fits; each turn but the last moves it below one more float.
    pub fn place_to_left(&self, el_pos: &ElPosition) -> NewPosition {
        let mut pos_el = el_pos.el_pos;
        pos_el.x += self.current_left + el_pos.el_margins.left;
        pos_el.y += self.current_top + el_pos.el_margins.top;
        pos_el.width -= el_pos.el_margins.left + el_pos.el_margins.right;
        let max_right = el_pos.container_width + self.current_left;
        let mut was_changed = false;
        let mut next_line = false;
        let mut left_side;
        let mut right_side;

        // Every turn but the last moves the box below one more float; geometry that is not finite never gets past one.
        let mut turns = self.floats_left.len() + self.floats_right.len() + 2;
        while turns > 0 {
            turns -= 1;
            let mut max_left_pos: Option<Position> = None;
            let mut found = false;
            let mut max_left = self.current_left;
            left_side = false;
            // check intersection with left floats
            for fb in &self.floats_left {
                if fb.pos.height == Px(0.0) {
                    continue;
                }
                if fb.pos.on_same_line(&pos_el, true) {
                    left_side = true;
                    max_left = max_left.max(fb.pos.right());
                    if pos_el.x < fb.pos.right() {
                        pos_el.x = fb.pos.right();
                        max_left_pos = Some(fb.pos);
                        found = true;
                        was_changed = true;
                    }
                }
            }
            if pos_el.right() > max_right && found {
                // move to next line
                next_line = true;
                pos_el.x = self.current_left + el_pos.el_margins.left;
                pos_el.y = max_left_pos.expect("a left float").bottom();
            } else {
                found = false;
                let mut min_right = max_right;
                right_side = false;
                // check intersection with right floats
                for fb in &self.floats_right {
                    if fb.pos.height == Px(0.0) {
                        continue;
                    }
                    // calculate minimum right position
                    if fb.pos.on_same_line(&pos_el, true) {
                        right_side = true;
                        min_right = min_right.min(fb.pos.left());
                    }
                    // if element intersects float box move it to the next line
                    if fb.pos.does_intersect(Some(&pos_el), true) {
                        right_side = false;
                        pos_el.x = self.current_left + el_pos.el_margins.left;
                        pos_el.y = match max_left_pos {
                            Some(p) => p.bottom().min(fb.pos.bottom()),
                            None => fb.pos.bottom(),
                        };
                        found = true;
                        next_line = true;
                        was_changed = true;
                        break;
                    }
                }
                if !found {
                    // position found
                    let mut pos = NewPosition {
                        found: was_changed,
                        new_line: next_line,
                        top: pos_el.y - self.current_top - el_pos.el_margins.top,
                        left: pos_el.x - self.current_left - el_pos.el_margins.left,
                        width: min_right - max_left,
                    };
                    if left_side {
                        pos.width += el_pos.el_margins.left;
                    }
                    if right_side {
                        pos.width += el_pos.el_margins.right;
                    }
                    return pos;
                }
            }
        }
        // position not found
        NewPosition { found: false, new_line: false, top: el_pos.el_pos.y, left: el_pos.el_pos.x, width: el_pos.container_width }
    }

    /// `place_to_right(el_pos)`: as `place_to_left`, from the right.
    pub fn place_to_right(&self, el_pos: &ElPosition) -> NewPosition {
        let mut pos_el = el_pos.el_pos;
        pos_el.x += self.current_left + el_pos.el_margins.left;
        pos_el.y += self.current_top + el_pos.el_margins.top;
        pos_el.width -= el_pos.el_margins.left + el_pos.el_margins.right;
        let max_right = el_pos.container_width + self.current_left;
        let mut was_changed = false;
        let mut next_line = false;
        let mut left_side;
        let mut right_side;

        let mut turns = self.floats_left.len() + self.floats_right.len() + 2;
        while turns > 0 {
            turns -= 1;
            let mut min_right_pos: Option<Position> = None;
            let mut found = false;
            let mut min_right = max_right;
            right_side = false;
            // check intersection with right floats
            for fb in &self.floats_right {
                if fb.pos.height == Px(0.0) {
                    continue;
                }
                // if element intersects float box move it to the left of float box
                if fb.pos.on_same_line(&pos_el, true) {
                    right_side = true;
                    min_right = min_right.min(fb.pos.left());
                    if pos_el.right() > fb.pos.left() {
                        pos_el.x = fb.pos.left() - pos_el.width;
                        min_right_pos = Some(fb.pos);
                        found = true;
                        was_changed = true;
                    }
                }
            }
            if pos_el.left() < self.current_left && found {
                // move to next line
                next_line = true;
                pos_el.x = max_right - pos_el.width - el_pos.el_margins.left;
                pos_el.y = min_right_pos.expect("a right float").bottom();
            } else {
                found = false;
                let mut max_left = self.current_left;
                left_side = false;
                // check intersection with left floats
                for fb in &self.floats_left {
                    if fb.pos.height == Px(0.0) {
                        continue;
                    }
                    // calculate maximum left position
                    if fb.pos.on_same_line(&pos_el, true) {
                        left_side = true;
                        max_left = max_left.max(fb.pos.right());
                    }
                    // if element intersects float box move it to the next line
                    if fb.pos.does_intersect(Some(&pos_el), true) {
                        left_side = false;
                        pos_el.x = max_right - pos_el.width - el_pos.el_margins.left;
                        pos_el.y = match min_right_pos {
                            Some(p) => p.bottom().min(fb.pos.bottom()),
                            None => fb.pos.bottom(),
                        };
                        found = true;
                        next_line = true;
                        was_changed = true;
                        break;
                    }
                }
                if !found {
                    // position found
                    let mut pos = NewPosition {
                        found: was_changed,
                        new_line: next_line,
                        top: pos_el.y - self.current_top - el_pos.el_margins.top,
                        left: pos_el.x - self.current_left - el_pos.el_margins.left,
                        width: min_right - max_left,
                    };
                    if left_side {
                        pos.width += el_pos.el_margins.left;
                    }
                    if right_side {
                        pos.width += el_pos.el_margins.right;
                    }
                    return pos;
                }
            }
        }
        // position not found
        NewPosition { found: false, new_line: false, top: el_pos.el_pos.y, left: el_pos.el_pos.x, width: el_pos.container_width }
    }

    /// `update_floats(dy, parent)`: the floats inside `parent`'s element moved down by `dy`, each list from its end.
    pub fn update_floats(&mut self, doc: &Document, dy: Px, parent: RenderId) {
        let parent_el = doc.render.item(parent).element;
        let mut reset_cache = false;
        for fb in self.floats_left.iter_mut().rev() {
            if doc.is_ancestor(doc.render.item(fb.el).element, parent_el) {
                reset_cache = true;
                fb.pos.y += dy;
            }
        }
        if reset_cache {
            self.cache_line_left.invalidate();
        }
        reset_cache = false;
        for fb in self.floats_right.iter_mut().rev() {
            if doc.is_ancestor(doc.render.item(fb.el).element, parent_el) {
                reset_cache = true;
                fb.pos.y += dy;
            }
        }
        if reset_cache {
            self.cache_line_right.invalidate();
        }
    }

    /// `apply_relative_shift(containing_block_size)`: the left floats' relative offsets (the right floats' never).
    pub fn apply_relative_shift(&self, doc: &mut Document, containing_block_size: &ContainingBlockContext) {
        for fb in &self.floats_left {
            doc.apply_relative_shift(fb.el, containing_block_size);
        }
    }

    /// `find_min_left(y, context_idx)`: the minimum widths of the context's left floats at `y`.
    pub fn find_min_left(&self, y: Px, context_idx: i32) -> Px {
        let y = y + self.current_top;
        let mut min_left = self.current_left;
        for fb in &self.floats_left {
            if y >= fb.pos.top() && y < fb.pos.bottom() && fb.context == context_idx {
                min_left += fb.min_width;
            }
        }
        if min_left < self.current_left {
            return Px(0.0);
        }
        min_left - self.current_left
    }

    /// `find_min_right(y, right, context_idx)`.
    pub fn find_min_right(&self, y: Px, right: Px, context_idx: i32) -> Px {
        let y = y + self.current_top;
        let mut min_right = right + self.current_left;
        for fb in &self.floats_right {
            if y >= fb.pos.top() && y < fb.pos.bottom() && fb.context == context_idx {
                min_right -= fb.min_width;
            }
        }
        if min_right < self.current_left {
            return Px(0.0);
        }
        min_right - self.current_left
    }
}
