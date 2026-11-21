//! `document_container.h` and `document_container.cpp`: what litehtml asks of the program that renders with
//! it, as far as the ported stages ask it. Later stages add the rest of the C++ interface here. A method with a
//! body here has that body in the C++ too, but those the C++ leaves pure, which do here what the renderer's
//! container does where that is nothing (core/src/renderer.cpp), or answer as a container without fonts would.
//! The paint calls the C++ leaves pure (the texts, markers, layers, borders and clips) have no body here either.

use crate::Px;
use crate::background::{BackgroundLayer, ConicGradient, LinearGradient, RadialGradient};
use crate::borders::{BorderRadiuses, Borders};
use crate::css_effects::{FilterPaint, PaintGroup, ShadowPaint, VectorPath};
use crate::css_tokenizer::CssToken;
use crate::element::ElementId;
use crate::font_description::FontDescription;
use crate::types::{FontMetrics, MediaFeatures, Position, Size};
use crate::utf8_strings::{utf8_to_utf32, utf32_to_utf8};
use crate::web_color::{ResolveColor, WebColor};

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

    /// `get_image_aspect(src)`: how an image fits its box when no `object-fit` says (an SVG's `preserveAspectRatio`,
    /// such as `xMinYMid slice`; `slice` covers, anything else contains), empty for none, as the C++'s body answers.
    fn get_image_aspect(&self, _src: &[u8]) -> Vec<u8> {
        Vec::new()
    }

    /// `create_text_breaker(text, font)`: a breaker of the text in the font, as a handle the document keeps and
    /// passes to `text_breaker_fit` and `text_breaker_minimum_width`; 0 for none (the C++'s null pointer).
    fn create_text_breaker(&self, _text: &[u8], _font: u64) -> u64 {
        0
    }

    /// `measures_text_later()`: an `el_text` may owe its width until it is read, so that a container measures many at
    /// once.
    fn measures_text_later(&self) -> bool {
        false
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

    // ---- Painting (stage L13), in the C++'s order --------------------------------------------------------------

    /// `draw_text(hdc, text, font, color, pos)`: a text (up to its first NUL) in the font and colour, its box at
    /// `pos`.
    fn draw_text(&self, hdc: u64, text: &[u8], font: u64, color: WebColor, pos: &Position);

    /// `draw_list_marker(hdc, marker)`: a list item's marker that is not drawn as text.
    fn draw_list_marker(&self, hdc: u64, marker: &ListMarker);

    /// `draw_image(hdc, layer, url, base_url)`: a background image's layer.
    fn draw_image(&self, hdc: u64, layer: &BackgroundLayer, url: &[u8], base_url: &[u8]);

    /// `draw_solid_fill(hdc, layer, color)`: a background colour's layer (and the form controls' fills).
    fn draw_solid_fill(&self, hdc: u64, layer: &BackgroundLayer, color: &WebColor);

    /// `draw_linear_gradient(hdc, layer, gradient)`.
    fn draw_linear_gradient(&self, hdc: u64, layer: &BackgroundLayer, gradient: &LinearGradient);

    /// `draw_radial_gradient(hdc, layer, gradient)`.
    fn draw_radial_gradient(&self, hdc: u64, layer: &BackgroundLayer, gradient: &RadialGradient);

    /// `draw_conic_gradient(hdc, layer, gradient)`.
    fn draw_conic_gradient(&self, hdc: u64, layer: &BackgroundLayer, gradient: &ConicGradient);

    /// `draw_borders(hdc, borders, draw_pos, root)`: a box's borders (or outline) around `draw_pos`.
    fn draw_borders(&self, hdc: u64, borders: &Borders, draw_pos: &Position, root: bool);

    /// `set_clip(pos, bdr_radius)`: what is drawn until the matching `del_clip` is clipped to the rounded box.
    fn set_clip(&self, pos: &Position, bdr_radius: &BorderRadiuses);

    /// `del_clip()`: the last clip set (or clip path) ends.
    fn del_clip(&self);

    /// `suspend_clips()`: a fixed box escapes the clips set so far, until `resume_clips`.
    fn suspend_clips(&self) {}

    /// `resume_clips()`.
    fn resume_clips(&self) {}

    /// `draw_box_shadow(hdc, box, radius, shadow)`.
    fn draw_box_shadow(&self, _hdc: u64, _box: &Position, _radius: &BorderRadiuses, _shadow: &ShadowPaint) {}

    /// `set_clip_path(path)`: a `clip-path` clips until the matching `del_clip`.
    fn set_clip_path(&self, _path: &VectorPath) {}

    /// `point_in_clip_path(path, x, y)`: whether a point is inside a `clip-path` (hit testing, stage L16).
    fn point_in_clip_path(&self, _path: &VectorPath, _x: Px, _y: Px) -> bool {
        true
    }

    /// `begin_group(hdc, group)`: what is drawn until the matching `end_group` is composed as one group.
    fn begin_group(&self, _hdc: u64, _group: &PaintGroup) {}

    /// `draw_filtered(hdc, filter, paint)`: `paint` draws the filtered content into the target it is given.
    fn draw_filtered(&self, hdc: u64, _filter: &FilterPaint, paint: &mut dyn FnMut(u64)) {
        paint(hdc);
    }

    /// `draw_backdrop(hdc, filter)`: what is behind the box filtered.
    fn draw_backdrop(&self, _hdc: u64, _filter: &FilterPaint) {}

    /// `draw_path(hdc, path)`: a vector path filled or stroked.
    fn draw_path(&self, _hdc: u64, _path: &VectorPath) {}

    /// `end_group(hdc)`.
    fn end_group(&self, _hdc: u64) {}

    /// `draw_cached(hdc, box, key, paint)`: whether the container drew a paint it keeps under the key (calling
    /// `paint` to make it, when it does not have it yet); false when it keeps none.
    fn draw_cached(&self, _hdc: u64, _box: &Position, _key: u64, _paint: &mut dyn FnMut(u64)) -> bool {
        false
    }

    /// `draw_isolated(hdc, box, paint)`: `paint` drawn apart from what is around it.
    fn draw_isolated(&self, hdc: u64, _box: &Position, paint: &mut dyn FnMut(u64)) {
        paint(hdc);
    }

    /// `controls_active()`: whether form controls are drawn in their active (accent) look.
    fn controls_active(&self) -> bool {
        false
    }

    /// `control_accent()`: the accent colour of controls whose style gives none.
    fn control_accent(&self) -> WebColor {
        WebColor::rgba(0, 122, 255, 255)
    }

    // ---- The pointer (stage L16) ---------------------------------------------------------------------------------

    /// `on_anchor_click(url, el)`: an anchor with an `href` was clicked (the url up to its first NUL). Pure in the
    /// C++; the renderer's does nothing.
    fn on_anchor_click(&self, _url: &[u8], _el: ElementId) {}

    /// `on_element_click(el)`: an element was clicked; true when the container took the click, which then goes no
    /// further up the tree.
    fn on_element_click(&self, _el: ElementId) -> bool {
        false
    }

    /// `on_mouse_event(el, event)`: the pointer entered or left the element (none when the hovered element went and
    /// none came). Pure in the C++; the renderer's does nothing.
    fn on_mouse_event(&self, _el: Option<ElementId>, _event: MouseEvent) {}

    /// `set_cursor(cursor)`: the hovered element's `cursor`, empty when none is hovered (up to its first NUL). Pure
    /// in the C++; the renderer keeps it.
    fn set_cursor(&self, _cursor: &[u8]) {}
}

/// `mouse_event`: what `on_mouse_event` tells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MouseEvent {
    mouse_event_enter,
    mouse_event_leave,
}

/// `list_marker`: a list item's marker as `draw_list_marker` gets it: its image (none when empty) and base url (a
/// null pointer when `None`), `list_style_type`, colour, box, number (-1 for the shapes) and font.
#[derive(Clone, Debug, Default)]
pub struct ListMarker {
    pub image: Vec<u8>,
    pub baseurl: Option<Vec<u8>>,
    pub marker_type: i32,
    pub color: WebColor,
    pub pos: Position,
    pub index: i32,
    pub font: u64,
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
