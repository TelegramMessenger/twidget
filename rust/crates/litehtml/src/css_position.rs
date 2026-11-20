//! `css_position.h`: a box and a size as lengths.

use crate::css_length::CssLength;

/// `css_position`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CssPosition {
    pub x: CssLength,
    pub y: CssLength,
    pub width: CssLength,
    pub height: CssLength,
}

/// `css_size`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CssSize {
    pub width: CssLength,
    pub height: CssLength,
}

impl CssSize {
    /// `css_size(width, height)`.
    pub fn new(width: CssLength, height: CssLength) -> CssSize {
        CssSize { width, height }
    }
}

/// `size_vector`.
pub type SizeVector = Vec<CssSize>;
