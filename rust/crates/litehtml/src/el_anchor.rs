//! `el_anchor.cpp`: an anchor follows its `href` when clicked (stage L16) and is a `:link` while sheets are
//! applied.

use crate::document::Document;
use crate::element::ElementId;
use crate::string_id::_link_;

impl Document {
    /// `el_anchor::apply_stylesheet(stylesheet)`'s own part, before `html_tag`'s: an anchor with an `href` gains
    /// `:link`, once more for every sheet applied (`m_pseudo_classes` keeps each).
    pub fn el_anchor_apply_stylesheet(&mut self, id: ElementId) {
        if self.get_attr(id, b"href").is_some() {
            self.el_mut(id).pseudo_classes.push(_link_);
        }
    }
}
