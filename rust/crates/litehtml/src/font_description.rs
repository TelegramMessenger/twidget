//! `font_description.h`: what a document asks its container's `create_font` for, and the key the document caches
//! the answer under.

use crate::Px;
use crate::css_length::CssLength;
use crate::css_values::{TextDecorationLine, TextDecorationStyle, TextEmphasisPosition};
use crate::cxx;
use crate::web_color::WebColor;

/// `font_description::text_shadow`: a text shadow past the first, as the font draws it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextShadow {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub color: WebColor,
}

/// `font_description`.
#[derive(Clone, Debug, PartialEq)]
pub struct FontDescription {
    pub family: Vec<u8>,
    pub size: Px,
    /// A `font_style` value.
    pub style: i32,
    pub weight: i32,
    /// `text_decoration_line` flags.
    pub decoration_line: i32,
    pub decoration_thickness: CssLength,
    /// A `text_decoration_style` value.
    pub decoration_style: i32,
    pub decoration_color: WebColor,
    pub emphasis_style: Vec<u8>,
    pub emphasis_color: WebColor,
    pub emphasis_position: i32,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub features: Vec<u8>,
    pub smoothing: i32,
    pub shadow_x: f32,
    pub shadow_y: f32,
    pub shadow_blur: f32,
    pub shadow_color: WebColor,
    pub extra_shadows: Vec<TextShadow>,
}

impl Default for FontDescription {
    fn default() -> FontDescription {
        FontDescription {
            family: Vec::new(),
            size: Px(0.0),
            style: 0,
            weight: 0,
            decoration_line: TextDecorationLine::text_decoration_line_none.index(),
            decoration_thickness: CssLength::default(),
            decoration_style: TextDecorationStyle::text_decoration_style_solid.index(),
            decoration_color: WebColor::CURRENT_COLOR,
            emphasis_style: Vec::new(),
            emphasis_color: WebColor::CURRENT_COLOR,
            emphasis_position: TextEmphasisPosition::text_emphasis_position_over.index(),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            features: Vec::new(),
            smoothing: 0,
            shadow_x: 0.0,
            shadow_y: 0.0,
            shadow_blur: 0.0,
            shadow_color: WebColor::TRANSPARENT,
            extra_shadows: Vec::new(),
        }
    }
}

impl FontDescription {
    /// `hash()`: every field that tells two fonts apart, as text (`std::to_string` of the numbers).
    pub fn hash(&self) -> Vec<u8> {
        let f = |v: f32| cxx::to_string_f64(f64::from(v));
        let i = cxx::to_string_int;
        let mut out = Vec::new();
        out.extend_from_slice(&self.family);
        out.extend_from_slice(b":sz=");
        out.extend(f(self.size.value()));
        out.extend_from_slice(b":st=");
        out.extend(i(self.style));
        out.extend_from_slice(b":w=");
        out.extend(i(self.weight));
        out.extend_from_slice(b":dl=");
        out.extend(i(self.decoration_line));
        out.extend_from_slice(b":dt=");
        out.extend(self.decoration_thickness.to_string());
        out.extend_from_slice(b":ds=");
        out.extend(i(self.decoration_style));
        out.extend_from_slice(b":dc=");
        out.extend(self.decoration_color.to_string());
        out.extend_from_slice(b":ephs=");
        out.extend_from_slice(&self.emphasis_style);
        out.extend_from_slice(b":ephc=");
        out.extend(self.emphasis_color.to_string());
        out.extend_from_slice(b":ephp=");
        out.extend(i(self.emphasis_position));
        if self.letter_spacing != 0.0 {
            out.extend_from_slice(b":ls=");
            out.extend(f(self.letter_spacing));
        }
        if self.word_spacing != 0.0 {
            out.extend_from_slice(b":wsp=");
            out.extend(f(self.word_spacing));
        }
        if !self.features.is_empty() {
            out.extend_from_slice(b":ff=");
            out.extend_from_slice(&self.features);
        }
        if self.smoothing != 0 {
            out.extend_from_slice(b":fs=");
            out.extend(i(self.smoothing));
        }
        if self.shadow_color.alpha != 0 {
            out.extend_from_slice(b":ts=");
            out.extend(f(self.shadow_x));
            out.push(b',');
            out.extend(f(self.shadow_y));
            out.push(b',');
            out.extend(f(self.shadow_blur));
            out.push(b',');
            out.extend(self.shadow_color.to_string());
        }
        for extra in &self.extra_shadows {
            out.push(b';');
            out.extend(f(extra.x));
            out.push(b',');
            out.extend(f(extra.y));
            out.push(b',');
            out.extend(f(extra.blur));
            out.push(b',');
            out.extend(extra.color.to_string());
        }
        out
    }
}
