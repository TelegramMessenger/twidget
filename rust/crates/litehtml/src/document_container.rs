//! `document_container.h`: what litehtml asks of the program that renders with it, as far as the ported
//! stages ask it. Later stages add the rest of the C++ interface here; a method with a body here has that
//! body in the C++ too.

use crate::Px;
use crate::web_color::ResolveColor;

/// `document_container`.
pub trait DocumentContainer: ResolveColor {
    /// `pt_to_px(pt)`.
    fn pt_to_px(&self, pt: f32) -> Px;

    /// `get_default_font_size()`.
    fn get_default_font_size(&self) -> Px;

    /// `import_css(text, url, baseurl)`: the text of an `@import`ed sheet; `baseurl` may be changed to the
    /// sheet's own.
    fn import_css(&self, url: &[u8], baseurl: &mut Vec<u8>) -> Vec<u8>;

    /// `sanitize_at_rule(name)`: whether an at-rule of this (lower-case) name is read at all.
    fn sanitize_at_rule(&self, _name: &[u8]) -> bool {
        true
    }
}
