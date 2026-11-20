//! `document_container.h`: what litehtml asks of the program that renders with it, as far as the ported
//! stages ask it. Later stages add the rest of the C++ interface here; a method with a body here has that
//! body in the C++ too.

use crate::Px;
use crate::css_tokenizer::CssToken;
use crate::types::MediaFeatures;
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

    /// `get_media_features(media)`: the features as the container has them now (a viewport length in a
    /// declaration is folded against these, not the document's).
    fn get_media_features(&self) -> MediaFeatures;

    /// `sanitize_at_rule(name)`: whether an at-rule of this (lower-case) name is read at all.
    fn sanitize_at_rule(&self, _name: &[u8]) -> bool {
        true
    }

    /// `sanitize_declaration(name, value)`: whether a declaration (its name lower-case unless custom, its value
    /// without whitespace) is kept.
    fn sanitize_declaration(&self, _name: &[u8], _value: &[CssToken]) -> bool {
        true
    }
}

/// A container as `parse_color` takes it: for the names it resolves.
pub fn resolver(container: Option<&dyn DocumentContainer>) -> Option<&dyn ResolveColor> {
    container.map(|c| c as &dyn ResolveColor)
}
