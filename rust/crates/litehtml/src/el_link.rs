//! `el_link.cpp`: a `<link rel=stylesheet>` brings the sheet its container imports; any other link goes to the
//! container.

use crate::document::{Document, Env};
use crate::element::ElementId;

impl Document {
    /// `el_link::parse_attributes()`: a stylesheet link with an `href` whose import gives text adds that sheet
    /// (with the import's base url and the link's `media`); otherwise `link(doc, el)`. Its children's attributes
    /// are not parsed.
    pub fn el_link_parse_attributes(&mut self, env: &Env, id: ElementId) {
        let mut processed = false;
        if self.get_attr(id, b"rel") == Some(&b"stylesheet"[..]) {
            let media = self.get_attr(id, b"media").map(<[u8]>::to_vec);
            let href = self.get_attr(id, b"href").map(<[u8]>::to_vec);
            if let Some(href) = href.filter(|h| !h.is_empty()) {
                let mut css_baseurl = Vec::new();
                let css_text = env.container.import_css(&href, &mut css_baseurl);
                if !css_text.is_empty() {
                    self.add_stylesheet(&css_text, Some(&css_baseurl), media.as_deref());
                    processed = true;
                }
            }
        }
        if !processed {
            env.container.link(id);
        }
    }
}
