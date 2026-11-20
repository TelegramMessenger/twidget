//! `document_container.h` and `document_container.cpp`: what litehtml asks of the program that renders with
//! it, as far as the ported stages ask it. Later stages add the rest of the C++ interface here. A method with a
//! body here has that body in the C++ too, but those the C++ leaves pure, which do here what the renderer's
//! container does where that is nothing (core/src/renderer.cpp), or answer as a container without fonts would.

use crate::Px;
use crate::css_tokenizer::CssToken;
use crate::element::ElementId;
use crate::font_description::FontDescription;
use crate::types::{FontMetrics, MediaFeatures, Position, Size};
use crate::utf8_strings::{utf8_to_utf32, utf32_to_utf8};
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

    /// `sanitize_element(tag, svg, attrs)`: whether an element Gumbo parsed (in the SVG namespace or not) is
    /// built; the container may change its tag and attributes first.
    fn sanitize_element(&self, _tag: &mut Vec<u8>, _svg: bool, _attrs: &mut crate::element::StringMap) -> bool {
        true
    }

    /// `sanitizer_limit(what)`: a document met one of its limits (`"elements"`, `"depth"`, `"content"`,
    /// `"table"`).
    fn sanitizer_limit(&self, _what: &[u8]) {}

    /// `split_text(text, on_word, on_space)`: a text node's text as the words and spaces that become its
    /// `el_text` and `el_space` elements, in order; `out` is told each piece and whether it is a space.
    fn split_text(&self, text: &[u8], out: &mut dyn FnMut(&[u8], bool)) {
        split_text(text, out);
    }

    /// `set_base_url(base_url)`: a `<base>`'s `href` (`None` without one). Pure in the C++.
    fn set_base_url(&self, _base_url: Option<&[u8]>) {}

    /// `set_caption(caption)`: a `<title>`'s text. Pure in the C++.
    fn set_caption(&self, _caption: &[u8]) {}

    /// `link(doc, el)`: a `<link>` that brought no style sheet. Pure in the C++.
    fn link(&self, _el: ElementId) {}

    /// `create_font(descr, doc, fm)`: a font handle (0 for none) and its metrics (left as they were, the default,
    /// when the container sets none). Pure in the C++.
    fn create_font(&self, _descr: &FontDescription) -> (u64, FontMetrics) {
        (0, FontMetrics::default())
    }

    /// `text_width(text, font)`: the width of the text (up to its first NUL) in the font. Pure in the C++.
    fn text_width(&self, _text: &[u8], _font: u64) -> Px {
        Px(0.0)
    }

    /// `get_default_font_name()`. Pure in the C++; the renderer's.
    fn get_default_font_name(&self) -> Vec<u8> {
        b"-apple-system, system-ui, sans-serif".to_vec()
    }

    /// `load_image(src, baseurl, redraw_on_ready)`: an image the document will draw (`baseurl` a null pointer when
    /// `None`). Pure in the C++.
    fn load_image(&self, _src: &[u8], _baseurl: Option<&[u8]>, _redraw_on_ready: bool) {}

    /// `transform_text(text, tt)`: the text as a `text-transform` value (`text_transform_*`) makes it. Pure in the
    /// C++.
    fn transform_text(&self, _text: &mut Vec<u8>, _tt: i32) {}

    /// `get_language(language, culture)`. Pure in the C++; the renderer's.
    fn get_language(&self) -> (Vec<u8>, Vec<u8>) {
        (b"en".to_vec(), Vec::new())
    }

    /// `trusted_styles(trusted)`: the document parses its master and user sheets between a `true` and a `false`.
    fn trusted_styles(&self, _trusted: bool) {}

    /// `get_viewport(viewport)`: the viewport's place and size. Pure in the C++.
    fn get_viewport(&self) -> Position {
        Position::default()
    }

    /// `get_image_size(src, baseurl, sz)`: an image's size (`baseurl` a null pointer when `None`), left 0 × 0 when
    /// the container does not know it. Pure in the C++.
    fn get_image_size(&self, _src: &[u8], _baseurl: Option<&[u8]>) -> Size {
        Size::default()
    }

    /// `create_text_breaker(text, font)`: a breaker of the text in the font, as a handle the document keeps and
    /// passes to `text_breaker_fit` and `text_breaker_minimum_width`; 0 for none (the C++'s null pointer).
    fn create_text_breaker(&self, _text: &[u8], _font: u64) -> u64 {
        0
    }

    /// `text_breaker::fit(start, width)`: where the piece of the text from byte `start` that fits in `width` ends.
    fn text_breaker_fit(&self, _breaker: u64, start: usize, _width: Px) -> usize {
        start
    }

    /// `text_breaker::minimum_width()`: the width of the text's widest piece that cannot be broken.
    fn text_breaker_minimum_width(&self, _breaker: u64) -> Px {
        Px(0.0)
    }

    /// The document no longer keeps the breaker (the C++'s last `shared_ptr` to it goes). No call in the C++.
    fn drop_text_breaker(&self, _breaker: u64) {}
}

/// A container as `parse_color` takes it: for the names it resolves.
pub fn resolver(container: Option<&dyn DocumentContainer>) -> Option<&dyn ResolveColor> {
    container.map(|c| c as &dyn ResolveColor)
}

/// `document_container::split_text`'s own body: whitespace (space, tab, newline, return, form feed) is one space
/// each, a CJK ideograph one word, a hyphen between letters and a `?` between visible characters end the word
/// they belong to; a NUL, or a sequence the reader takes for one, ends the text.
pub fn split_text(text: &[u8], out: &mut dyn FnMut(&[u8], bool)) {
    let mut str: Vec<u32> = Vec::new();
    let str_in = utf8_to_utf32(text);
    let letter = |ch: u32| {
        (u32::from(b'a')..=u32::from(b'z')).contains(&ch)
            || (u32::from(b'A')..=u32::from(b'Z')).contains(&ch)
            || (ch >= 0xC0 && ch != 0xD7 && ch != 0xF7 && ch < 0x2000)
    };
    for i in 0..str_in.len() {
        let c = str_in[i];
        let prev = if i > 0 { str_in[i - 1] } else { 0 };
        let next = if i + 1 < str_in.len() { str_in[i + 1] } else { 0 };
        if ((c == u32::from(b'-') || c == 0x2010) && letter(prev) && letter(next)) || (c == u32::from(b'?') && next > 0x20 && prev > 0x20) {
            str.push(c);
            out(&c_string(&utf32_to_utf8(&str)), false);
            str.clear();
            continue;
        }
        if c <= 0x20 && matches!(c, 0x20 | 0x09 | 0x0a | 0x0d | 0x0c) {
            if !str.is_empty() {
                out(&c_string(&utf32_to_utf8(&str)), false);
                str.clear();
            }
            str.push(c);
            out(&c_string(&utf32_to_utf8(&str)), true);
            str.clear();
        }
        // CJK character range
        else if (0x4E00..=0x9FCC).contains(&c) {
            if !str.is_empty() {
                out(&c_string(&utf32_to_utf8(&str)), false);
                str.clear();
            }
            str.push(c);
            out(&c_string(&utf32_to_utf8(&str)), false);
            str.clear();
        } else {
            str.push(c);
        }
    }
    if !str.is_empty() {
        out(&c_string(&utf32_to_utf8(&str)), false);
    }
}

/// A piece as the callbacks get it, a `const char*`: up to its first NUL.
fn c_string(bytes: &[u8]) -> Vec<u8> {
    crate::gumbo::c_str(bytes).to_vec()
}
