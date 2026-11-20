//! `css_properties.h` and `css_properties.cpp`, with `background.h`'s `background`, `borders.h`'s `css_border`,
//! `css_border_radius` and `css_borders`, `css_margins.h` and `css_offsets.h`: an element's computed style, as
//! `compute` makes it from the element's cascaded style, its parent's computed one and the document (its
//! container's fonts, its media and its root's font size).
//!
//! Keyword-valued fields are the C++ enums' numbers (`i32`), as the C++ casts whatever the style holds. A
//! property is read through [`Get`], `html_tag::get_property`'s rules: the declared value, `revert` as the master
//! sheet alone gives it, `initial` and `unset`, and inheritance from the parent's computed field.

use std::collections::BTreeMap;

use crate::css_effects::*;
use crate::css_length::{CssLength, clamp_length};
use crate::css_parser::{f_componentize, f_remove_whitespace, normalize};
use crate::css_position::CssSize;
use crate::css_tokenizer::IDENT;
use crate::css_values::*;
use crate::cxx;
use crate::document::{Document, StyleCx, Units};
use crate::element::{ElementId, ElementKind};
use crate::font_description::{FontDescription, TextShadow};
use crate::gradient::{Image, ImageType};
use crate::html::{equal_i, round_f, t_strcasecmp, trim};
use crate::string_id::*;
use crate::style::{CssWideKeyword, PropertyValue, Value};
use crate::types::{FontMetrics, contain_inline_size, contain_size};
use crate::web_color::WebColor;
use crate::{Px, StringId};

/// The C++ enum values the computation names, as their numbers.
macro_rules! numbers {
    ($($ty:ident :: $name:ident),* $(,)?) => {
        $(#[allow(dead_code)] const $name: i32 = $ty::$name.index();)*
    };
}

numbers!(
    StyleDisplay::display_none,
    StyleDisplay::display_block,
    StyleDisplay::display_inline,
    StyleDisplay::display_inline_block,
    StyleDisplay::display_inline_table,
    StyleDisplay::display_list_item,
    StyleDisplay::display_table,
    StyleDisplay::display_table_caption,
    StyleDisplay::display_table_cell,
    StyleDisplay::display_table_column,
    StyleDisplay::display_table_column_group,
    StyleDisplay::display_table_footer_group,
    StyleDisplay::display_table_header_group,
    StyleDisplay::display_table_row,
    StyleDisplay::display_table_row_group,
    StyleDisplay::display_inline_text,
    StyleDisplay::display_flex,
    StyleDisplay::display_inline_flex,
    StyleDisplay::display_grid,
    StyleDisplay::display_inline_grid,
    ElementPosition::element_position_static,
    ElementPosition::element_position_absolute,
    ElementPosition::element_position_fixed,
    ElementPosition::element_position_sticky,
    Visibility::visibility_visible,
    ElementFloat::float_none,
    ElementClear::clear_none,
    Appearance::appearance_none,
    Resize::resize_none,
    BoxSizing::box_sizing_content_box,
    Overflow::overflow_visible,
    TextAlign::text_align_start,
    TextAlign::text_align_center,
    TextAlign::text_align_th_center,
    Direction::direction_ltr,
    Direction::direction_rtl,
    VerticalAlign::va_baseline,
    TextTransform::text_transform_none,
    FontSmoothing::font_smoothing_auto,
    WhiteSpace::white_space_normal,
    OverflowWrap::overflow_wrap_normal,
    WordBreak::word_break_normal,
    UserSelect::user_select_auto,
    CaptionSide::caption_side_top,
    BorderStyle::border_style_none,
    BorderStyle::border_style_hidden,
    BorderCollapse::border_collapse_separate,
    TableLayout::table_layout_auto,
    ListStyleType::list_style_type_none,
    ListStyleType::list_style_type_disc,
    ListStylePosition::list_style_position_outside,
    FontSize::font_size_xx_small,
    FontSize::font_size_x_small,
    FontSize::font_size_small,
    FontSize::font_size_medium,
    FontSize::font_size_large,
    FontSize::font_size_x_large,
    FontSize::font_size_xx_large,
    FontSize::font_size_smaller,
    FontSize::font_size_larger,
    FontWeight::font_weight_normal,
    FontWeight::font_weight_bold,
    FontWeight::font_weight_bolder,
    FontWeight::font_weight_lighter,
    FontStyle::font_style_normal,
    TextDecorationLine::text_decoration_line_none,
    TextDecorationStyle::text_decoration_style_solid,
    TextDecorationThickness::text_decoration_thickness_auto,
    TextEmphasisPosition::text_emphasis_position_over,
    BackgroundAttachment::background_attachment_scroll,
    BackgroundRepeat::background_repeat_repeat,
    BackgroundBox::background_box_border,
    BackgroundBox::background_box_padding,
    BackgroundSize::background_size_auto,
    FlexDirection::flex_direction_row,
    FlexWrap::flex_wrap_nowrap,
    FlexJustifyContent::flex_justify_content_flex_start,
    FlexAlignItems::flex_align_items_auto,
    FlexAlignItems::flex_align_items_normal,
    FlexAlignItems::flex_align_items_stretch,
    FlexAlignContent::flex_align_content_stretch,
    FlexBasis::flex_basis_auto,
);

/// `doc->cvt_units(field, m_font_metrics, 0)` on each of the fields, in order.
macro_rules! cvt_fields {
    ($this:ident, $g:ident, $($($f:ident).+),* $(,)?) => {
        $($this.cvt_field($g, |c| &mut c.$($f).+);)*
    };
}

/// `css_margins`, and `css_offsets` (the same four lengths).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CssMargins {
    pub left: CssLength,
    pub right: CssLength,
    pub top: CssLength,
    pub bottom: CssLength,
}

/// `css_offsets`.
pub type CssOffsets = CssMargins;

/// `css_border`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CssBorder {
    pub width: CssLength,
    /// A `border_style` value.
    pub style: i32,
    pub color: WebColor,
}

/// `css_border_radius`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CssBorderRadius {
    pub top_left_x: CssLength,
    pub top_left_y: CssLength,
    pub top_right_x: CssLength,
    pub top_right_y: CssLength,
    pub bottom_right_x: CssLength,
    pub bottom_right_y: CssLength,
    pub bottom_left_x: CssLength,
    pub bottom_left_y: CssLength,
}

/// `css_borders`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CssBorders {
    pub left: CssBorder,
    pub top: CssBorder,
    pub right: CssBorder,
    pub bottom: CssBorder,
    pub radius: CssBorderRadius,
}

/// `background`: the layers' values, one list per property.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Background {
    pub m_image: Vec<Image>,
    pub m_baseurl: Vec<u8>,
    pub m_color: WebColor,
    pub m_attachment: Vec<i32>,
    pub m_position_x: Vec<CssLength>,
    pub m_position_y: Vec<CssLength>,
    pub m_size: Vec<CssSize>,
    pub m_repeat: Vec<i32>,
    pub m_clip: Vec<i32>,
    pub m_origin: Vec<i32>,
}

/// `css_line_height_t`: the declared length and what it computes to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineHeight {
    pub css_value: CssLength,
    pub computed_value: Px,
}

/// `css_properties`.
#[derive(Clone, Debug, PartialEq)]
pub struct CssProperties {
    pub m_el_position: i32,
    pub m_text_align: i32,
    pub m_direction: i32,
    pub m_overflow: i32,
    pub m_overflow_x: i32,
    pub m_overflow_y: i32,
    pub m_white_space: i32,
    pub m_overflow_wrap: i32,
    pub m_word_break: i32,
    pub m_user_select: i32,
    pub m_display: i32,
    pub m_visibility: i32,
    pub m_appearance: i32,
    pub m_resize: i32,
    pub m_box_sizing: i32,
    pub m_z_index: CssLength,
    pub m_vertical_align: i32,
    pub m_float: i32,
    pub m_clear: i32,
    pub m_css_margins: CssMargins,
    pub m_css_padding: CssMargins,
    pub m_css_borders: CssBorders,
    pub m_css_width: CssLength,
    pub m_cell_width: CssLength,
    pub m_css_height: CssLength,
    pub m_css_min_width: CssLength,
    pub m_css_min_height: CssLength,
    pub m_css_max_width: CssLength,
    pub m_css_max_height: CssLength,
    pub m_css_offsets: CssOffsets,
    pub m_css_text_indent: CssLength,
    pub m_css_line_height: CssLength,
    pub m_line_height: LineHeight,
    pub m_list_style_type: i32,
    pub m_list_style_position: i32,
    pub m_list_style_image: Vec<u8>,
    pub m_list_style_image_baseurl: Vec<u8>,
    pub m_bg: Background,
    /// `uint_ptr m_font`: the container's handle.
    pub m_font: u64,
    pub m_font_size: CssLength,
    pub m_font_family: Vec<u8>,
    pub m_font_weight: CssLength,
    pub m_font_style: i32,
    pub m_text_decoration_line: i32,
    pub m_text_decoration_style: i32,
    pub m_text_decoration_thickness: CssLength,
    pub m_text_decoration_color: WebColor,
    pub m_text_emphasis_style: Vec<u8>,
    pub m_text_emphasis_color: WebColor,
    pub m_text_emphasis_position: i32,
    pub m_font_metrics: FontMetrics,
    pub m_shared_id: i32,
    pub m_text_transform: i32,
    pub m_font_smoothing: i32,
    pub m_color: WebColor,
    pub m_cursor: Vec<u8>,
    pub m_content: Vec<u8>,
    pub m_border_collapse: i32,
    pub m_table_layout: i32,
    pub m_css_border_spacing_x: CssLength,
    pub m_css_border_spacing_y: CssLength,
    pub m_flex_grow: f32,
    pub m_flex_shrink: f32,
    pub m_flex_basis: CssLength,
    pub m_flex_direction: i32,
    pub m_flex_wrap: i32,
    pub m_flex_justify_content: i32,
    pub m_flex_align_items: i32,
    pub m_flex_align_self: i32,
    pub m_flex_align_content: i32,
    pub m_caption_side: i32,
    pub m_order: i32,
    pub m_row_gap: CssLength,
    pub m_column_gap: CssLength,
    pub m_justify_items: i32,
    pub m_justify_self: i32,
    pub m_grid_template_columns: GridTrackList,
    pub m_grid_template_rows: GridTrackList,
    pub m_grid_auto_rows: GridTrackList,
    pub m_grid_column: GridPlacement,
    pub m_grid_row: GridPlacement,
    pub m_opacity: f32,
    pub m_aspect_ratio: f32,
    pub m_center_content: bool,
    pub m_letter_spacing: CssLength,
    pub m_word_spacing: CssLength,
    pub m_font_variant_numeric: Vec<u8>,
    pub m_font_feature_settings: Vec<u8>,
    pub m_text_shadows: Vec<BoxShadow>,
    pub m_box_shadows: Vec<BoxShadow>,
    pub m_filter: Vec<FilterFunction>,
    pub m_backdrop_filter: Vec<FilterFunction>,
    pub m_clip_path: ClipPathShape,
    pub m_transform: Vec<TransformFunction>,
    pub m_transform_origin_x: CssLength,
    pub m_transform_origin_y: CssLength,
    pub m_transform_fill_box: bool,
    pub m_backface_hidden: i32,
    pub m_contain: i32,
    pub m_outline: CssBorder,
    pub m_outline_offset: CssLength,
    pub m_accent_color: WebColor,
    pub m_dark_scheme: i32,
    pub m_keyword_size: bool,
    pub m_generic_mono: bool,
}

/// `css_length(v)`: a length in px.
fn px(v: f32) -> CssLength {
    CssLength::new(v, CssUnits::css_units_px)
}

impl Default for CssProperties {
    fn default() -> CssProperties {
        CssProperties {
            m_el_position: element_position_static,
            m_text_align: text_align_start,
            m_direction: direction_ltr,
            m_overflow: overflow_visible,
            m_overflow_x: overflow_visible,
            m_overflow_y: overflow_visible,
            m_white_space: white_space_normal,
            m_overflow_wrap: overflow_wrap_normal,
            m_word_break: word_break_normal,
            m_user_select: user_select_auto,
            m_display: display_inline,
            m_visibility: visibility_visible,
            m_appearance: appearance_none,
            m_resize: resize_none,
            m_box_sizing: box_sizing_content_box,
            m_z_index: px(0.0),
            m_vertical_align: va_baseline,
            m_float: float_none,
            m_clear: clear_none,
            m_css_margins: CssMargins::default(),
            m_css_padding: CssMargins::default(),
            m_css_borders: CssBorders::default(),
            m_css_width: CssLength::default(),
            m_cell_width: CssLength::default(),
            m_css_height: CssLength::default(),
            m_css_min_width: CssLength::default(),
            m_css_min_height: CssLength::default(),
            m_css_max_width: CssLength::default(),
            m_css_max_height: CssLength::default(),
            m_css_offsets: CssOffsets::default(),
            m_css_text_indent: CssLength::default(),
            m_css_line_height: px(0.0),
            m_line_height: LineHeight::default(),
            m_list_style_type: list_style_type_none,
            m_list_style_position: list_style_position_outside,
            m_list_style_image: Vec::new(),
            m_list_style_image_baseurl: Vec::new(),
            m_bg: Background::default(),
            m_font: 0,
            m_font_size: px(0.0),
            m_font_family: Vec::new(),
            m_font_weight: CssLength::default(),
            m_font_style: font_style_normal,
            m_text_decoration_line: text_decoration_line_none,
            m_text_decoration_style: text_decoration_style_solid,
            m_text_decoration_thickness: CssLength::default(),
            m_text_decoration_color: WebColor::default(),
            m_text_emphasis_style: Vec::new(),
            m_text_emphasis_color: WebColor::default(),
            m_text_emphasis_position: 0,
            m_font_metrics: FontMetrics::default(),
            m_shared_id: 0,
            m_text_transform: text_transform_none,
            m_font_smoothing: font_smoothing_auto,
            m_color: WebColor::default(),
            m_cursor: Vec::new(),
            m_content: Vec::new(),
            m_border_collapse: border_collapse_separate,
            m_table_layout: table_layout_auto,
            m_css_border_spacing_x: CssLength::default(),
            m_css_border_spacing_y: CssLength::default(),
            m_flex_grow: 0.0,
            m_flex_shrink: 1.0,
            m_flex_basis: CssLength::default(),
            m_flex_direction: flex_direction_row,
            m_flex_wrap: flex_wrap_nowrap,
            m_flex_justify_content: flex_justify_content_flex_start,
            m_flex_align_items: flex_align_items_stretch,
            m_flex_align_self: flex_align_items_auto,
            m_flex_align_content: flex_align_content_stretch,
            m_caption_side: caption_side_top,
            m_order: 0,
            m_row_gap: CssLength::default(),
            m_column_gap: CssLength::default(),
            m_justify_items: flex_align_items_normal,
            m_justify_self: flex_align_items_auto,
            m_grid_template_columns: GridTrackList::default(),
            m_grid_template_rows: GridTrackList::default(),
            m_grid_auto_rows: GridTrackList::default(),
            m_grid_column: GridPlacement::default(),
            m_grid_row: GridPlacement::default(),
            m_opacity: 1.0,
            m_aspect_ratio: 0.0,
            m_center_content: false,
            m_letter_spacing: CssLength::default(),
            m_word_spacing: CssLength::default(),
            m_font_variant_numeric: Vec::new(),
            m_font_feature_settings: Vec::new(),
            m_text_shadows: Vec::new(),
            m_box_shadows: Vec::new(),
            m_filter: Vec::new(),
            m_backdrop_filter: Vec::new(),
            m_clip_path: ClipPathShape::default(),
            m_transform: Vec::new(),
            m_transform_origin_x: CssLength::new(50.0, CssUnits::css_units_percentage),
            m_transform_origin_y: CssLength::new(50.0, CssUnits::css_units_percentage),
            m_transform_fill_box: false,
            m_backface_hidden: 0,
            m_contain: 0,
            m_outline: CssBorder::default(),
            m_outline_offset: CssLength::default(),
            m_accent_color: WebColor::TRANSPARENT,
            m_dark_scheme: 0,
            m_keyword_size: true,
            m_generic_mono: false,
        }
    }
}

/// `font_size_table`: the keyword sizes for default font sizes of 9 to 16 px.
const font_size_table: [[f32; 7]; 8] = [
    [9.0, 9.0, 9.0, 9.0, 11.0, 14.0, 18.0],
    [9.0, 9.0, 9.0, 10.0, 12.0, 15.0, 20.0],
    [9.0, 9.0, 9.0, 11.0, 13.0, 17.0, 22.0],
    [9.0, 9.0, 10.0, 12.0, 14.0, 18.0, 24.0],
    [9.0, 9.0, 10.0, 13.0, 16.0, 20.0, 26.0],
    [9.0, 9.0, 11.0, 14.0, 17.0, 21.0, 28.0],
    [9.0, 10.0, 12.0, 15.0, 17.0, 23.0, 30.0],
    [9.0, 10.0, 13.0, 16.0, 18.0, 24.0, 32.0],
];

/// `merge_font_features(variant, settings)`: the two `tag=value` lists as one, in tag order, a setting winning
/// over a variant of the same tag; either alone as it is.
pub fn merge_font_features(variant: &[u8], settings: &[u8]) -> Vec<u8> {
    if settings.is_empty() || variant.is_empty() {
        return if variant.is_empty() { settings.to_vec() } else { variant.to_vec() };
    }
    let mut features: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    for list in [variant, settings] {
        let mut start = 0;
        while start < list.len() {
            let end = list[start..].iter().position(|c| *c == b',').map_or(list.len(), |k| start + k);
            let entry = &list[start..end];
            if let Some(eq) = entry.iter().position(|c| *c == b'=') {
                features.insert(entry[..eq].to_vec(), entry[eq + 1..].to_vec());
            }
            start = end + 1;
        }
    }
    let mut out = Vec::new();
    for (tag, value) in features {
        if !out.is_empty() {
            out.push(b',');
        }
        out.extend_from_slice(&tag);
        out.push(b'=');
        out.extend_from_slice(&value);
    }
    out
}

/// `std::clamp(v, lo, hi)`: `v < lo ? lo : hi < v ? hi : v`.
fn clamp_f32(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}

/// `html_tag::get_property<Type>(name, inherited, default_value, member)` and what `css_properties` reads of the
/// element through it.
pub struct Get<'a> {
    pub doc: &'a Document,
    pub cx: &'a StyleCx<'a>,
    pub id: ElementId,
}

impl std::fmt::Debug for Get<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Get").field("id", &self.id).finish_non_exhaustive()
    }
}

impl<'a> Get<'a> {
    fn parent(&self) -> Option<&'a CssProperties> {
        self.doc.parent(self.id).map(|p| &self.doc.el(p).css)
    }

    /// The declared value as `get_property` takes it: `revert` is what the master sheet alone gives.
    fn get<T: Clone>(
        &self,
        name: StringId,
        inherited: bool,
        default: T,
        pick: impl Fn(&Value) -> Option<&T>,
        field: impl Fn(&CssProperties) -> &T,
    ) -> T {
        let declared = self.doc.el(self.id).style.get_property(name);
        let ua: PropertyValue;
        let value = if matches!(declared.value, Value::CssWideKeyword(CssWideKeyword::revert)) {
            ua = self.doc.ua_property(self.cx, self.id, name);
            &ua
        } else {
            declared
        };
        if let Some(v) = pick(&value.value) {
            return v.clone();
        }
        let wide = match declared.value {
            Value::CssWideKeyword(kind) => Some(kind),
            _ => None,
        };
        // initial, and unset on a property that does not inherit (a revert the UA sheet has no value for is unset)
        if let Some(kind) = wide
            && !matches!(value.value, Value::Inherit)
            && (!inherited || kind == CssWideKeyword::initial)
        {
            return default;
        }
        if inherited || matches!(value.value, Value::Inherit) || wide.is_some() {
            return match self.parent() {
                Some(parent) => field(parent).clone(),
                None => default,
            };
        }
        default
    }

    pub fn int(&self, name: StringId, inherited: bool, default: i32, field: impl Fn(&CssProperties) -> &i32) -> i32 {
        self.get(name, inherited, default, |v| if let Value::Int(i) = v { Some(i) } else { None }, field)
    }

    pub fn float(&self, name: StringId, inherited: bool, default: f32, field: impl Fn(&CssProperties) -> &f32) -> f32 {
        self.get(name, inherited, default, |v| if let Value::Float(f) = v { Some(f) } else { None }, field)
    }

    pub fn length(&self, name: StringId, inherited: bool, default: CssLength, field: impl Fn(&CssProperties) -> &CssLength) -> CssLength {
        self.get(name, inherited, default, |v| if let Value::Length(l) = v { Some(l) } else { None }, field)
    }

    pub fn color(&self, name: StringId, inherited: bool, default: WebColor, field: impl Fn(&CssProperties) -> &WebColor) -> WebColor {
        self.get(name, inherited, default, |v| if let Value::Color(c) = v { Some(c) } else { None }, field)
    }

    pub fn string(&self, name: StringId, inherited: bool, default: Vec<u8>, field: impl Fn(&CssProperties) -> &Vec<u8>) -> Vec<u8> {
        self.get(name, inherited, default, |v| if let Value::String(s) = v { Some(s) } else { None }, field)
    }

    pub fn ints(&self, name: StringId, inherited: bool, default: Vec<i32>, field: impl Fn(&CssProperties) -> &Vec<i32>) -> Vec<i32> {
        self.get(name, inherited, default, |v| if let Value::IntVector(i) = v { Some(i) } else { None }, field)
    }

    pub fn lengths(
        &self,
        name: StringId,
        inherited: bool,
        default: Vec<CssLength>,
        field: impl Fn(&CssProperties) -> &Vec<CssLength>,
    ) -> Vec<CssLength> {
        self.get(name, inherited, default, |v| if let Value::LengthVector(l) = v { Some(l) } else { None }, field)
    }

    pub fn sizes(
        &self,
        name: StringId,
        inherited: bool,
        default: Vec<CssSize>,
        field: impl Fn(&CssProperties) -> &Vec<CssSize>,
    ) -> Vec<CssSize> {
        self.get(name, inherited, default, |v| if let Value::Sizes(s) = v { Some(s) } else { None }, field)
    }

    pub fn images(
        &self,
        name: StringId,
        inherited: bool,
        default: Vec<Image>,
        field: impl Fn(&CssProperties) -> &Vec<Image>,
    ) -> Vec<Image> {
        self.get(name, inherited, default, |v| if let Value::Images(i) = v { Some(i) } else { None }, field)
    }

    /// `get_token_property(name)`: the tokens the element's style holds for the name.
    pub fn tokens(&self, name: StringId) -> Option<&'a Vec<crate::css_tokenizer::CssToken>> {
        match &self.doc.el(self.id).style.get_property(name).value {
            Value::Tokens(tokens) => Some(tokens),
            _ => None,
        }
    }

    /// `has_own_property(name)`.
    pub fn has_own_property(&self, name: StringId) -> bool {
        !matches!(self.doc.el(self.id).style.get_property(name).value, Value::Invalid)
    }
}

/// `el_text::strong_direction()` of a text: 1 right to left, -1 left to right, 0 none.
pub fn strong_direction(text: &[u8]) -> i32 {
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c < 0x80 {
            if (c | 0x20) >= b'a' && (c | 0x20) <= b'z' {
                return -1;
            }
            i += 1;
            continue;
        }
        let n = (if (c >> 5) == 6 {
            2
        } else if (c >> 4) == 14 {
            3
        } else {
            4
        })
        .min(text.len() - i);
        let mut cp: u32 = u32::from(if n == 2 {
            c & 0x1F
        } else if n == 3 {
            c & 0x0F
        } else {
            c & 0x07
        });
        for k in 1..n {
            cp = (cp << 6) | u32::from(text[i + k] & 0x3F);
        }
        i += n;
        // Hebrew, Arabic, Syriac, Thaana, NKo, Samaritan and their presentation forms
        if (0x0590..=0x08FF).contains(&cp)
            || (0xFB1D..=0xFDFF).contains(&cp)
            || (0xFE70..=0xFEFF).contains(&cp)
            || (0x10800..=0x10FFF).contains(&cp)
            || (0x1E800..=0x1EFFF).contains(&cp)
        {
            return 1;
        }
        // Not punctuation, symbols, combining marks, variation selectors or emoji: a letter of a left to right script.
        if cp >= 0xC0
            && cp != 0xD7
            && cp != 0xF7
            && !(0x0300..=0x036F).contains(&cp)
            && !(0x2000..=0x2BFF).contains(&cp)
            && !(0x3000..=0x303F).contains(&cp)
            && !(0xFE00..=0xFE6F).contains(&cp)
            && !(0xFF00..=0xFF20).contains(&cp)
            && cp < 0x1F000
        {
            return -1;
        }
    }
    0
}

/// `dir=auto`'s `first_strong(parent)`: the direction of the first strong character of the element's text, depth
/// first.
fn first_strong(doc: &Document, parent: ElementId) -> i32 {
    for &child in doc.children(parent) {
        let strong = if doc.is_text(child) { strong_direction(&doc.el(child).text) } else { first_strong(doc, child) };
        if strong != 0 {
            return strong;
        }
    }
    0
}

/// Whether the value is one of the displays a positioned, floated or root box becomes a block from.
fn blockified(display: i32, list_item: bool) -> bool {
    matches!(
        display,
        display_inline
            | display_table_row_group
            | display_table_column
            | display_table_column_group
            | display_table_header_group
            | display_table_footer_group
            | display_table_row
            | display_table_cell
            | display_table_caption
            | display_inline_block
    ) || (list_item && display == display_list_item)
}

impl CssProperties {
    /// `get_font_size()`.
    pub fn get_font_size(&self) -> Px {
        Px(self.m_font_size.val())
    }

    /// `get_z_index()`: the value, clamped to `int`.
    pub fn get_z_index(&self) -> i32 {
        let v = f64::from(self.m_z_index.val());
        let v = if v < f64::from(i32::MIN) {
            f64::from(i32::MIN)
        } else if f64::from(i32::MAX) < v {
            f64::from(i32::MAX)
        } else {
            v
        };
        cxx::f64_to_int(v)
    }

    /// `is_paint_group()`.
    pub fn is_paint_group(&self) -> bool {
        !self.m_clip_path.empty()
            || self.m_el_position == element_position_sticky
            || self.m_opacity < 1.0
            || !self.m_transform.is_empty()
            || self.m_backface_hidden != 0
            || !self.m_filter.is_empty()
            || !self.m_backdrop_filter.is_empty()
    }

    /// `set_flex_layout(direction, justify, align)`.
    pub fn set_flex_layout(&mut self, direction: i32, justify: i32, align: i32) {
        self.m_flex_direction = direction;
        self.m_flex_justify_content = justify;
        self.m_flex_align_items = align;
    }

    /// The document's lengths as this element converts them: `rem` from the root's font size, which is this
    /// one's as it is so far when the element is the root.
    fn units<'a>(&self, g: &Get<'a>) -> Units<'a> {
        let root_font_size = match g.doc.root {
            Some(root) if root == g.id => self.get_font_size(),
            Some(root) => g.doc.el(root).css.get_font_size(),
            None => Px(0.0),
        };
        Units { container: g.cx.container, media: g.doc.media, root_font_size }
    }

    fn cvt(&self, g: &Get, len: &mut CssLength) {
        let units = self.units(g);
        units.cvt_units(len, &self.m_font_metrics, Px(0.0));
    }

    /// `compute(el, doc)`: every property from the element's style, its parent's computed one and the document.
    #[inline(never)]
    pub fn compute(&mut self, g: &Get) {
        let doc = g.doc;
        let el = g.id;
        self.m_color = g.color(_color_, true, WebColor::BLACK, |c| &c.m_color);

        self.m_el_position = g.int(_position_, false, element_position_static, |c| &c.m_el_position);
        self.m_display = g.int(_display_, false, display_inline, |c| &c.m_display);
        // inline-text is what a text node computes to: a tag that declares it would be laid out as one.
        if self.m_display == display_inline_text {
            self.m_display = display_inline;
        }
        self.m_visibility = g.int(_visibility_, true, visibility_visible, |c| &c.m_visibility);
        self.m_float = g.int(_float_, false, float_none, |c| &c.m_float);
        self.m_clear = g.int(_clear_, false, clear_none, |c| &c.m_clear);
        self.m_appearance = g.int(_appearance_, false, appearance_none, |c| &c.m_appearance);
        self.m_resize = g.int(_resize_, false, resize_none, |c| &c.m_resize);
        self.m_box_sizing = g.int(_box_sizing_, false, box_sizing_content_box, |c| &c.m_box_sizing);
        self.m_overflow_x = g.int(_overflow_x_, false, overflow_visible, |c| &c.m_overflow_x);
        self.m_overflow_y = g.int(_overflow_y_, false, overflow_visible, |c| &c.m_overflow_y);
        self.m_overflow = if self.m_overflow_y != overflow_visible { self.m_overflow_y } else { self.m_overflow_x };
        self.m_text_align = g.int(_text_align_, true, text_align_start, |c| &c.m_text_align);
        self.m_direction = g.int(_direction_, true, direction_ltr, |c| &c.m_direction);
        if let Some(dir) = doc.get_attr(el, b"dir")
            && equal_i(dir, b"auto")
        {
            // dir=auto: the first strong character of the element's text decides.
            self.m_direction = if first_strong(doc, el) > 0 { direction_rtl } else { direction_ltr };
        }
        if self.m_text_align == text_align_th_center {
            let inherited = g.parent().map_or(text_align_start, |p| p.m_text_align);
            self.m_text_align = if inherited == text_align_start { text_align_center } else { inherited };
        }
        self.m_vertical_align = g.int(_vertical_align_, false, va_baseline, |c| &c.m_vertical_align);
        self.m_text_transform = g.int(_text_transform_, true, text_transform_none, |c| &c.m_text_transform);
        self.m_font_smoothing = g.int(__webkit_font_smoothing_, true, font_smoothing_auto, |c| &c.m_font_smoothing);
        self.m_white_space = g.int(_white_space_, true, white_space_normal, |c| &c.m_white_space);
        self.m_overflow_wrap = g.int(_overflow_wrap_, true, overflow_wrap_normal, |c| &c.m_overflow_wrap);
        self.m_word_break = g.int(_word_break_, true, word_break_normal, |c| &c.m_word_break);
        self.m_user_select = g.int(_user_select_, false, user_select_auto, |c| &c.m_user_select);
        self.m_caption_side = g.int(_caption_side_, true, caption_side_top, |c| &c.m_caption_side);

        // https://www.w3.org/TR/CSS22/visuren.html#dis-pos-flo
        if self.m_display == display_none {
            // 1. If 'display' has the value 'none', then 'position' and 'float' do not apply.
            self.m_float = float_none;
        } else if self.m_el_position == element_position_absolute || self.m_el_position == element_position_fixed {
            // 2. Absolutely positioned: float is none and display becomes a block.
            self.m_float = float_none;
            if self.m_display == display_inline_table {
                self.m_display = display_table;
            } else if blockified(self.m_display, false) {
                self.m_display = display_block;
            }
        } else if self.m_float != float_none {
            // 3. Floated: display becomes a block.
            if self.m_display == display_inline_table {
                self.m_display = display_table;
            } else if blockified(self.m_display, false) {
                self.m_display = display_block;
            }
        } else if doc.parent(el).is_none() {
            // 4. The root: display becomes a block, list-item too.
            if self.m_display == display_inline_table {
                self.m_display = display_table;
            } else if blockified(self.m_display, true) {
                self.m_display = display_block;
            }
        } else if doc.is_replaced(el) && self.m_display == display_inline {
            self.m_display = display_inline_block;
        }
        // 5. Otherwise, the remaining 'display' property values apply as specified.

        self.compute_font(g);
        let font_size = self.get_font_size();

        let auto = CssLength::predef_value(0);
        let none = auto.clone();
        let normal = auto.clone();

        self.m_css_width = g.length(_width_, false, auto.clone(), |c| &c.m_css_width);
        self.m_css_height = g.length(_height_, false, auto.clone(), |c| &c.m_css_height);

        self.m_css_min_width = g.length(_min_width_, false, auto.clone(), |c| &c.m_css_min_width);
        self.m_css_min_height = g.length(_min_height_, false, auto.clone(), |c| &c.m_css_min_height);

        self.m_css_max_width = g.length(_max_width_, false, none.clone(), |c| &c.m_css_max_width);
        self.m_css_max_height = g.length(_max_height_, false, none, |c| &c.m_css_max_height);

        cvt_fields!(self, g, m_css_width);
        self.m_cell_width = self.m_css_width.clone();
        cvt_fields!(self, g, m_css_height, m_css_min_width, m_css_min_height, m_css_max_width, m_css_max_height);

        self.m_css_margins.left = g.length(_margin_left_, false, px(0.0), |c| &c.m_css_margins.left);
        self.m_css_margins.right = g.length(_margin_right_, false, px(0.0), |c| &c.m_css_margins.right);
        self.m_css_margins.top = g.length(_margin_top_, false, px(0.0), |c| &c.m_css_margins.top);
        self.m_css_margins.bottom = g.length(_margin_bottom_, false, px(0.0), |c| &c.m_css_margins.bottom);
        cvt_fields!(self, g, m_css_margins.left, m_css_margins.right, m_css_margins.top, m_css_margins.bottom);

        self.m_css_padding.left = g.length(_padding_left_, false, px(0.0), |c| &c.m_css_padding.left);
        self.m_css_padding.right = g.length(_padding_right_, false, px(0.0), |c| &c.m_css_padding.right);
        self.m_css_padding.top = g.length(_padding_top_, false, px(0.0), |c| &c.m_css_padding.top);
        self.m_css_padding.bottom = g.length(_padding_bottom_, false, px(0.0), |c| &c.m_css_padding.bottom);
        cvt_fields!(self, g, m_css_padding.left, m_css_padding.right, m_css_padding.top, m_css_padding.bottom);

        let color = self.m_color;
        self.m_css_borders.left.color = self.get_color_property(g, _border_left_color_, false, color, |c| &c.m_css_borders.left.color);
        self.m_css_borders.right.color = self.get_color_property(g, _border_right_color_, false, color, |c| &c.m_css_borders.right.color);
        self.m_css_borders.top.color = self.get_color_property(g, _border_top_color_, false, color, |c| &c.m_css_borders.top.color);
        self.m_css_borders.bottom.color =
            self.get_color_property(g, _border_bottom_color_, false, color, |c| &c.m_css_borders.bottom.color);

        self.m_css_borders.left.style = g.int(_border_left_style_, false, border_style_none, |c| &c.m_css_borders.left.style);
        self.m_css_borders.right.style = g.int(_border_right_style_, false, border_style_none, |c| &c.m_css_borders.right.style);
        self.m_css_borders.top.style = g.int(_border_top_style_, false, border_style_none, |c| &c.m_css_borders.top.style);
        self.m_css_borders.bottom.style = g.int(_border_bottom_style_, false, border_style_none, |c| &c.m_css_borders.bottom.style);

        let medium = px(border_width_medium_value);
        self.m_css_borders.left.width = g.length(_border_left_width_, false, medium.clone(), |c| &c.m_css_borders.left.width);
        self.m_css_borders.right.width = g.length(_border_right_width_, false, medium.clone(), |c| &c.m_css_borders.right.width);
        self.m_css_borders.top.width = g.length(_border_top_width_, false, medium.clone(), |c| &c.m_css_borders.top.width);
        self.m_css_borders.bottom.width = g.length(_border_bottom_width_, false, medium.clone(), |c| &c.m_css_borders.bottom.width);

        for border in
            [&mut self.m_css_borders.left, &mut self.m_css_borders.right, &mut self.m_css_borders.top, &mut self.m_css_borders.bottom]
        {
            if border.style == border_style_none || border.style == border_style_hidden {
                border.width.assign(0.0);
            }
        }

        self.snap_field(g, |c| &mut c.m_css_borders.left.width);
        self.snap_field(g, |c| &mut c.m_css_borders.right.width);
        self.snap_field(g, |c| &mut c.m_css_borders.top.width);
        self.snap_field(g, |c| &mut c.m_css_borders.bottom.width);

        let r = |name, field: fn(&CssProperties) -> &CssLength| g.length(name, false, px(0.0), field);
        self.m_css_borders.radius.top_left_x = r(_border_top_left_radius_x_, |c| &c.m_css_borders.radius.top_left_x);
        self.m_css_borders.radius.top_left_y = r(_border_top_left_radius_y_, |c| &c.m_css_borders.radius.top_left_y);
        self.m_css_borders.radius.top_right_x = r(_border_top_right_radius_x_, |c| &c.m_css_borders.radius.top_right_x);
        self.m_css_borders.radius.top_right_y = r(_border_top_right_radius_y_, |c| &c.m_css_borders.radius.top_right_y);
        self.m_css_borders.radius.bottom_left_x = r(_border_bottom_left_radius_x_, |c| &c.m_css_borders.radius.bottom_left_x);
        self.m_css_borders.radius.bottom_left_y = r(_border_bottom_left_radius_y_, |c| &c.m_css_borders.radius.bottom_left_y);
        self.m_css_borders.radius.bottom_right_x = r(_border_bottom_right_radius_x_, |c| &c.m_css_borders.radius.bottom_right_x);
        self.m_css_borders.radius.bottom_right_y = r(_border_bottom_right_radius_y_, |c| &c.m_css_borders.radius.bottom_right_y);
        cvt_fields!(
            self,
            g,
            m_css_borders.radius.top_left_x,
            m_css_borders.radius.top_left_y,
            m_css_borders.radius.top_right_x,
            m_css_borders.radius.top_right_y,
            m_css_borders.radius.bottom_left_x,
            m_css_borders.radius.bottom_left_y,
            m_css_borders.radius.bottom_right_x,
            m_css_borders.radius.bottom_right_y
        );

        self.m_border_collapse = g.int(_border_collapse_, true, border_collapse_separate, |c| &c.m_border_collapse);
        self.m_table_layout = g.int(_table_layout_, false, table_layout_auto, |c| &c.m_table_layout);

        self.m_css_border_spacing_x = g.length(__litehtml_border_spacing_x_, true, px(0.0), |c| &c.m_css_border_spacing_x);
        self.m_css_border_spacing_y = g.length(__litehtml_border_spacing_y_, true, px(0.0), |c| &c.m_css_border_spacing_y);
        self.cvt_field(g, |c| &mut c.m_css_border_spacing_x);
        self.cvt_field(g, |c| &mut c.m_css_border_spacing_y);

        self.m_css_offsets.left = g.length(_left_, false, auto.clone(), |c| &c.m_css_offsets.left);
        self.m_css_offsets.right = g.length(_right_, false, auto.clone(), |c| &c.m_css_offsets.right);
        self.m_css_offsets.top = g.length(_top_, false, auto.clone(), |c| &c.m_css_offsets.top);
        self.m_css_offsets.bottom = g.length(_bottom_, false, auto.clone(), |c| &c.m_css_offsets.bottom);
        cvt_fields!(self, g, m_css_offsets.left, m_css_offsets.right, m_css_offsets.top, m_css_offsets.bottom);

        self.m_z_index = g.length(_z_index_, false, auto, |c| &c.m_z_index);
        self.m_content = g.string(_content_, false, Vec::new(), |c| &c.m_content);
        self.m_cursor = g.string(_cursor_, true, b"auto".to_vec(), |c| &c.m_cursor);

        self.m_css_text_indent = g.length(_text_indent_, true, px(0.0), |c| &c.m_css_text_indent);
        self.cvt_field(g, |c| &mut c.m_css_text_indent);

        self.m_line_height.css_value = g.length(_line_height_, true, normal, |c| &c.m_line_height.css_value);
        if self.m_line_height.css_value.is_predefined() {
            self.m_line_height.computed_value = self.m_font_metrics.height;
        } else if self.m_line_height.css_value.units() == CssUnits::css_units_none {
            // Fractional, as browsers keep it (LayoutUnit).
            self.m_line_height.computed_value = Px(clamp_length(self.m_line_height.css_value.val() * font_size.value()));
        } else {
            let units = self.units(g);
            self.m_line_height.computed_value =
                units.to_pixels(&self.m_line_height.css_value, &self.m_font_metrics, self.m_font_metrics.font_size);
            self.m_line_height.css_value.assign(self.m_line_height.computed_value.value());
        }

        self.m_list_style_type = g.int(_list_style_type_, true, list_style_type_disc, |c| &c.m_list_style_type);
        self.m_list_style_position = g.int(_list_style_position_, true, list_style_position_outside, |c| &c.m_list_style_position);

        self.m_list_style_image = g.string(_list_style_image_, true, Vec::new(), |c| &c.m_list_style_image);
        if !self.m_list_style_image.is_empty() {
            self.m_list_style_image_baseurl = g.string(_list_style_image_baseurl_, true, Vec::new(), |c| &c.m_list_style_image_baseurl);
            g.cx.container.load_image(c_str(&self.m_list_style_image), Some(c_str(&self.m_list_style_image_baseurl)), true);
        }

        self.m_order = g.int(_order_, false, 0, |c| &c.m_order);

        self.compute_background(g);
        self.compute_flex(g);
        self.compute_grid(g);
        self.compute_effects(g);
    }

    /// `snap_border_width(field, doc)` on one of the fields.
    fn snap_field(&mut self, g: &Get, field: impl Fn(&mut CssProperties) -> &mut CssLength) {
        let mut width = std::mem::take(field(self));
        self.snap_border_width(g, &mut width);
        *field(self) = width;
    }

    /// `doc->cvt_units(field, m_font_metrics, 0)` on one of the fields.
    fn cvt_field(&mut self, g: &Get, field: impl Fn(&mut CssProperties) -> &mut CssLength) {
        let mut len = std::mem::take(field(self));
        self.cvt(g, &mut len);
        *field(self) = len;
    }

    /// `get_color_property(el, name, inherited, default_value, member)`: a colour, the element's `color` for
    /// `currentcolor` (every colour property but `color` itself).
    fn get_color_property(
        &self,
        g: &Get,
        name: StringId,
        inherited: bool,
        default: WebColor,
        field: impl Fn(&CssProperties) -> &WebColor,
    ) -> WebColor {
        let mut color = g.color(name, inherited, default, field);
        if color.is_current_color {
            color = self.m_color;
        }
        color
    }

    /// `compute_font(el, doc)`: the size, family, weight, style, decorations, emphasis, spacing and text shadows,
    /// and the font the document's container gives for them.
    fn compute_font(&mut self, g: &Get) {
        let doc = g.doc;
        let el = g.id;
        // initialize font size
        let sz = g.length(_font_size_, true, CssLength::predef_value(font_size_medium), |c| &c.m_font_size);

        let doc_font_size = g.cx.container.get_default_font_size();
        let el_parent = g.parent();
        let parent_sz = match el_parent {
            Some(parent) => parent.get_font_size(),
            None => doc_font_size,
        };

        let mut font_size = if sz.is_predefined() {
            let idx_in_table = round_f(doc_font_size.value() - 9.0);
            if (0..=7).contains(&idx_in_table) {
                if sz.predef() >= font_size_xx_small && sz.predef() <= font_size_xx_large {
                    Px(font_size_table[idx_in_table as usize][sz.predef() as usize])
                } else if sz.predef() == font_size_smaller {
                    parent_sz / Px(1.2)
                } else if sz.predef() == font_size_larger {
                    parent_sz * Px(1.2)
                } else {
                    parent_sz
                }
            } else {
                match sz.predef() {
                    font_size_xx_small => doc_font_size * Px(3.0) / Px(5.0),
                    font_size_x_small => doc_font_size * Px(3.0) / Px(4.0),
                    font_size_small => doc_font_size * Px(8.0) / Px(9.0),
                    font_size_large => doc_font_size * Px(6.0) / Px(5.0),
                    font_size_x_large => doc_font_size * Px(3.0) / Px(2.0),
                    font_size_xx_large => doc_font_size * Px(2.0),
                    font_size_smaller => parent_sz / Px(1.2),
                    font_size_larger => parent_sz * Px(1.2),
                    _ => parent_sz,
                }
            }
        } else if sz.units() == CssUnits::css_units_percentage {
            sz.calc_percent(parent_sz)
        } else {
            let fm = FontMetrics { x_height: parent_sz, font_size: parent_sz, ..FontMetrics::default() };
            self.units(g).to_pixels(&sz, &fm, Px(0.0))
        };

        self.m_font_family = g.string(_font_family_, true, g.cx.container.get_default_font_name(), |c| &c.m_font_family);

        let parent_keyword = el_parent.is_none_or(|p| p.m_keyword_size);
        let parent_mono = el_parent.is_some_and(|p| p.m_generic_mono);
        let own_size = g.has_own_property(_font_size_);
        self.m_keyword_size = if own_size { sz.is_predefined() && sz.predef() <= font_size_xx_large } else { parent_keyword };
        let family = trim(&self.m_font_family, b" \t\"'");
        self.m_generic_mono = self.m_keyword_size && t_strcasecmp(c_str(&family), b"monospace") == 0;
        if self.m_generic_mono && (own_size || !parent_mono) {
            font_size = font_size * Px(13.0) / Px(16.0);
        } else if !self.m_generic_mono && parent_mono && !own_size {
            font_size = font_size * Px(16.0) / Px(13.0);
        }

        self.m_font_size.assign(font_size.value());

        self.m_font_weight = g.length(_font_weight_, true, CssLength::predef_value(font_weight_normal), |c| &c.m_font_weight);
        self.m_font_style = g.int(_font_style_, true, font_style_normal, |c| &c.m_font_style);
        let propagate_decoration =
            !matches!(self.m_display, display_inline_block | display_inline_table | display_inline_flex | display_inline_grid)
                && self.m_float == float_none
                && !matches!(self.m_el_position, element_position_absolute | element_position_fixed);

        self.m_text_decoration_line =
            g.int(_text_decoration_line_, propagate_decoration, text_decoration_line_none, |c| &c.m_text_decoration_line);

        // Merge parent text decoration with child text decoration
        if propagate_decoration && let Some(parent) = el_parent {
            self.m_text_decoration_line |= parent.m_text_decoration_line;
        }

        if self.m_text_decoration_line != 0 {
            self.m_text_decoration_thickness =
                g.length(_text_decoration_thickness_, propagate_decoration, CssLength::predef_value(text_decoration_thickness_auto), |c| {
                    &c.m_text_decoration_thickness
                });
            self.m_text_decoration_style =
                g.int(_text_decoration_style_, propagate_decoration, text_decoration_style_solid, |c| &c.m_text_decoration_style);
            self.m_text_decoration_color =
                self.get_color_property(g, _text_decoration_color_, propagate_decoration, WebColor::CURRENT_COLOR, |c| {
                    &c.m_text_decoration_color
                });
        } else {
            self.m_text_decoration_thickness = CssLength::predef_value(text_decoration_thickness_auto);
            self.m_text_decoration_color = WebColor::CURRENT_COLOR;
        }

        // text-emphasis
        self.m_text_emphasis_style = g.string(_text_emphasis_style_, true, Vec::new(), |c| &c.m_text_emphasis_style);
        self.m_text_emphasis_position = g.int(_text_emphasis_position_, true, text_emphasis_position_over, |c| &c.m_text_emphasis_position);
        self.m_text_emphasis_color =
            self.get_color_property(g, _text_emphasis_color_, true, WebColor::CURRENT_COLOR, |c| &c.m_text_emphasis_color);

        if let Some(parent) = el_parent {
            if self.m_text_emphasis_style.is_empty() || self.m_text_emphasis_style == b"initial" || self.m_text_emphasis_style == b"unset" {
                self.m_text_emphasis_style = parent.m_text_emphasis_style.clone();
            }
            if self.m_text_emphasis_color == WebColor::CURRENT_COLOR {
                self.m_text_emphasis_color = parent.m_text_emphasis_color;
            }
            self.m_text_emphasis_position |= parent.m_text_emphasis_position;
        }

        if self.m_font_weight.is_predefined() {
            let inherited = || el_parent.map_or(400, |p| cxx::f32_to_int(p.m_font_weight.val()));
            match self.m_font_weight.predef() {
                font_weight_bold => self.m_font_weight.assign(700.0),
                font_weight_bolder => {
                    // The root has no parent: relative to the initial weight, as browsers take it.
                    let inherited = inherited();
                    self.m_font_weight.assign(if inherited < 400 {
                        400.0
                    } else if inherited < 600 {
                        700.0
                    } else {
                        900.0
                    });
                }
                font_weight_lighter => {
                    let inherited = inherited();
                    self.m_font_weight.assign(if inherited < 600 {
                        100.0
                    } else if inherited < 800 {
                        400.0
                    } else {
                        700.0
                    });
                }
                _ => self.m_font_weight.assign(400.0),
            }
        }

        let spacing = |this: &CssProperties, name, field: fn(&CssProperties) -> &CssLength| -> CssLength {
            let value = g.length(name, true, CssLength::predef_value(0), field);
            let mut out = CssLength::default();
            if value.is_predefined() {
                out.assign(0.0);
            } else {
                let fm = FontMetrics { x_height: font_size, font_size, ..FontMetrics::default() };
                out.assign(this.units(g).to_pixels(&value, &fm, Px(0.0)).value());
            }
            out
        };
        self.m_letter_spacing = spacing(self, _letter_spacing_, |c| &c.m_letter_spacing);
        self.m_word_spacing = spacing(self, _word_spacing_, |c| &c.m_word_spacing);
        self.m_font_variant_numeric = g.string(_font_variant_numeric_, true, Vec::new(), |c| &c.m_font_variant_numeric);
        self.m_font_feature_settings = g.string(_font_feature_settings_, true, Vec::new(), |c| &c.m_font_feature_settings);

        self.m_text_shadows.clear();
        if let Some(tokens) = g.tokens(_text_shadow_) {
            if parse_box_shadow(g.cx.names, tokens, &mut self.m_text_shadows, Some(g.cx.container)) {
                let fm = FontMetrics { x_height: font_size, font_size, ..FontMetrics::default() };
                let units = self.units(g);
                for shadow in &mut self.m_text_shadows {
                    for len in [&mut shadow.x, &mut shadow.y, &mut shadow.blur] {
                        let v = units.to_pixels(len, &fm, Px(0.0)).value();
                        len.assign(v);
                    }
                }
            }
        } else if let Some(parent) = el_parent {
            self.m_text_shadows = parent.m_text_shadows.clone();
        }

        let mut descr = FontDescription {
            letter_spacing: self.m_letter_spacing.val(),
            word_spacing: self.m_word_spacing.val(),
            features: merge_font_features(&self.m_font_variant_numeric, &self.m_font_feature_settings),
            smoothing: self.m_font_smoothing,
            ..FontDescription::default()
        };
        if let Some(shadow) = self.m_text_shadows.first()
            && !shadow.inset
        {
            descr.shadow_x = shadow.x.val();
            descr.shadow_y = shadow.y.val();
            descr.shadow_blur = cxx::max_f32(0.0, shadow.blur.val());
            descr.shadow_color = if shadow.color.is_current_color { self.m_color } else { shadow.color };
            for extra in self.m_text_shadows.iter().take(8).skip(1) {
                if extra.inset {
                    continue;
                }
                descr.extra_shadows.push(TextShadow {
                    x: extra.x.val(),
                    y: extra.y.val(),
                    blur: cxx::max_f32(0.0, extra.blur.val()),
                    color: if extra.color.is_current_color { self.m_color } else { extra.color },
                });
            }
        }
        descr.family = self.m_font_family.clone();
        descr.size = Px(font_size.value());
        descr.style = self.m_font_style;
        descr.weight = cxx::f32_to_int(self.m_font_weight.val());
        descr.decoration_line = self.m_text_decoration_line;
        descr.decoration_thickness = self.m_text_decoration_thickness.clone();
        descr.decoration_style = self.m_text_decoration_style;
        descr.decoration_color = self.m_text_decoration_color;
        descr.emphasis_style = self.m_text_emphasis_style.clone();
        descr.emphasis_color = self.m_text_emphasis_color;
        descr.emphasis_position = self.m_text_emphasis_position;

        let (font, metrics) = doc.get_font(g.cx, &descr);
        self.m_font = font;
        if let Some(metrics) = metrics {
            self.m_font_metrics = metrics;
        }
        let _ = el;
    }

    /// `compute_background_color(el)`.
    pub fn compute_background_color(&mut self, g: &Get) {
        self.m_bg.m_color = self.get_color_property(g, _background_color_, false, WebColor::TRANSPARENT, |c| &c.m_bg.m_color);
    }

    /// `compute_opacity(el)`.
    pub fn compute_opacity(&mut self, g: &Get) {
        self.m_opacity = g.float(_opacity_, false, 1.0, |c| &c.m_opacity);
    }

    /// `compute_transform(el, doc)`: `translate`, `rotate` and `scale` (each the function of its name with the
    /// property's values as arguments), then `transform`, the lengths in px.
    pub fn compute_transform(&mut self, g: &Get) {
        let names = g.cx.names;
        self.m_transform.clear();
        for name in [_translate_, _rotate_, _scale_] {
            let Some(tokens) = g.tokens(name) else { continue };
            if tokens.is_empty() || (tokens.len() == 1 && tokens[0].ident() == b"none") {
                continue;
            }
            let mut args = Vec::new();
            for tok in tokens {
                if !args.is_empty() {
                    args.push(b',');
                }
                args.extend(tok.get_repr(false));
            }
            let text = [&names.s(name)[..], b"(", &args, b")"].concat();
            let mut functions = Vec::new();
            if parse_transform(names, &normalize(&text, f_componentize | f_remove_whitespace, None), &mut functions) {
                self.m_transform.extend(functions);
            }
        }
        if let Some(tokens) = g.tokens(_transform_) {
            let mut functions = Vec::new();
            if parse_transform(names, tokens, &mut functions) {
                self.m_transform.extend(functions);
            }
        }
        let units = self.units(g);
        for f in &mut self.m_transform {
            units.cvt_units(&mut f.x, &self.m_font_metrics, Px(0.0));
            units.cvt_units(&mut f.y, &self.m_font_metrics, Px(0.0));
        }
    }

    /// `compute_background(el, doc)`.
    fn compute_background(&mut self, g: &Get) {
        self.compute_background_color(g);

        let auto_auto = CssSize::new(CssLength::predef_value(background_size_auto), CssLength::predef_value(background_size_auto));
        self.m_bg.m_position_x =
            g.lengths(_background_position_x_, false, vec![CssLength::new(0.0, CssUnits::css_units_percentage)], |c| &c.m_bg.m_position_x);
        self.m_bg.m_position_y =
            g.lengths(_background_position_y_, false, vec![CssLength::new(0.0, CssUnits::css_units_percentage)], |c| &c.m_bg.m_position_y);
        self.m_bg.m_size = g.sizes(_background_size_, false, vec![auto_auto], |c| &c.m_bg.m_size);

        let units = self.units(g);
        let fm = self.m_font_metrics;
        for x in &mut self.m_bg.m_position_x {
            units.cvt_units(x, &fm, Px(0.0));
        }
        for y in &mut self.m_bg.m_position_y {
            units.cvt_units(y, &fm, Px(0.0));
        }
        for size in &mut self.m_bg.m_size {
            units.cvt_units(&mut size.width, &fm, Px(0.0));
            units.cvt_units(&mut size.height, &fm, Px(0.0));
        }

        self.m_bg.m_attachment = g.ints(_background_attachment_, false, vec![background_attachment_scroll], |c| &c.m_bg.m_attachment);
        self.m_bg.m_repeat = g.ints(_background_repeat_, false, vec![background_repeat_repeat], |c| &c.m_bg.m_repeat);
        self.m_bg.m_clip = g.ints(_background_clip_, false, vec![background_box_border], |c| &c.m_bg.m_clip);
        self.m_bg.m_origin = g.ints(_background_origin_, false, vec![background_box_padding], |c| &c.m_bg.m_origin);

        self.m_bg.m_image = g.images(_background_image_, false, vec![Image::default()], |c| &c.m_bg.m_image);
        self.m_bg.m_baseurl = g.string(_background_image_baseurl_, false, Vec::new(), |c| &c.m_bg.m_baseurl);

        for image in &mut self.m_bg.m_image {
            match image.ty {
                ImageType::type_none => {}
                ImageType::type_url => {
                    if !image.url.is_empty() {
                        g.cx.container.load_image(c_str(&image.url), Some(c_str(&self.m_bg.m_baseurl)), true);
                    }
                }
                ImageType::type_gradient => {
                    for item in &mut image.m_gradient.m_colors {
                        if let Some(length) = &mut item.length {
                            units.cvt_units(length, &fm, Px(0.0));
                        }
                    }
                }
            }
        }
    }

    /// `compute_flex(el, doc)`.
    fn compute_flex(&mut self, g: &Get) {
        self.m_flex_direction = flex_direction_row;
        self.m_flex_wrap = flex_wrap_nowrap;
        self.m_flex_justify_content = flex_justify_content_flex_start;
        self.m_flex_align_items = flex_align_items_stretch;
        self.m_flex_align_content = flex_align_content_stretch;
        self.m_flex_grow = 0.0;
        self.m_flex_shrink = 1.0;
        self.m_flex_basis = CssLength::default();
        if self.m_display == display_grid || self.m_display == display_inline_grid {
            self.m_flex_justify_content = g.int(_justify_content_, false, flex_justify_content_flex_start, |c| &c.m_flex_justify_content);
            self.m_flex_align_items = g.int(_align_items_, false, flex_align_items_normal, |c| &c.m_flex_align_items);
            self.m_flex_align_content = g.int(_align_content_, false, flex_align_content_stretch, |c| &c.m_flex_align_content);
        }
        if self.m_display == display_flex || self.m_display == display_inline_flex {
            self.m_flex_direction = g.int(_flex_direction_, false, flex_direction_row, |c| &c.m_flex_direction);
            self.m_flex_wrap = g.int(_flex_wrap_, false, flex_wrap_nowrap, |c| &c.m_flex_wrap);

            self.m_flex_justify_content = g.int(_justify_content_, false, flex_justify_content_flex_start, |c| &c.m_flex_justify_content);
            self.m_flex_align_items = g.int(_align_items_, false, flex_align_items_normal, |c| &c.m_flex_align_items);
            self.m_flex_align_content = g.int(_align_content_, false, flex_align_content_stretch, |c| &c.m_flex_align_content);
        }
        self.m_flex_align_self = g.int(_align_self_, false, flex_align_items_auto, |c| &c.m_flex_align_self);
        if let Some(parent) = g.parent()
            && (parent.m_display == display_flex || parent.m_display == display_inline_flex)
        {
            self.m_flex_grow = g.float(_flex_grow_, false, 0.0, |c| &c.m_flex_grow);
            self.m_flex_shrink = g.float(_flex_shrink_, false, 1.0, |c| &c.m_flex_shrink);
            self.m_flex_basis = g.length(_flex_basis_, false, CssLength::predef_value(flex_basis_auto), |c| &c.m_flex_basis);
            if !self.m_flex_basis.is_predefined()
                && self.m_flex_basis.units() == CssUnits::css_units_none
                && Px(self.m_flex_basis.val()) != Px(0.0)
            {
                // flex-basis property must contain units
                self.m_flex_basis.set_predef(flex_basis_auto);
            }
            self.cvt_field(g, |c| &mut c.m_flex_basis);
            if self.m_display == display_inline || self.m_display == display_inline_block {
                self.m_display = display_block;
            } else if self.m_display == display_inline_table {
                self.m_display = display_table;
            } else if self.m_display == display_inline_flex {
                self.m_display = display_flex;
            }
        }
    }

    /// `compute_grid(el, doc)`.
    fn compute_grid(&mut self, g: &Get) {
        self.m_grid_template_columns = GridTrackList::default();
        self.m_grid_template_rows = GridTrackList::default();
        self.m_grid_auto_rows = GridTrackList::default();
        self.m_justify_items = flex_align_items_normal;
        self.m_justify_self = flex_align_items_auto;
        self.m_grid_column = GridPlacement::default();
        self.m_grid_row = GridPlacement::default();
        if self.m_display == display_grid || self.m_display == display_inline_grid {
            if let Some(tokens) = g.tokens(_grid_template_columns_) {
                parse_grid_track_list(tokens, &mut self.m_grid_template_columns);
            }
            if let Some(tokens) = g.tokens(_grid_template_rows_) {
                parse_grid_track_list(tokens, &mut self.m_grid_template_rows);
            }
            if let Some(tokens) = g.tokens(_grid_auto_rows_) {
                parse_grid_track_list(tokens, &mut self.m_grid_auto_rows);
            }
            let units = self.units(g);
            let fm = self.m_font_metrics;
            for list in [&mut self.m_grid_template_columns, &mut self.m_grid_template_rows, &mut self.m_grid_auto_rows] {
                for sizes in [&mut list.tracks, &mut list.repeat] {
                    for size in sizes.iter_mut() {
                        units.cvt_units(&mut size.min_len, &fm, Px(0.0));
                        units.cvt_units(&mut size.max_len, &fm, Px(0.0));
                    }
                }
            }
            self.m_justify_items = g.int(_justify_items_, false, flex_align_items_normal, |c| &c.m_justify_items);
        }

        let Some(parent) = g.parent() else { return };
        if parent.m_display != display_grid && parent.m_display != display_inline_grid {
            return;
        }
        self.m_justify_self = g.int(_justify_self_, false, flex_align_items_auto, |c| &c.m_justify_self);
        let placement = |shorthand, start, end, out: &mut GridPlacement| {
            if let Some(tokens) = g.tokens(shorthand) {
                parse_grid_placement(tokens, out);
            }
            let mut part = GridPlacement::default();
            if let Some(tokens) = g.tokens(start)
                && parse_grid_placement(tokens, &mut part)
            {
                out.start = part.start;
                out.span = if part.span != 0 { part.span } else { out.span };
            }
            if let Some(tokens) = g.tokens(end)
                && parse_grid_placement(tokens, &mut part)
            {
                out.end = part.start;
                out.span = if part.span != 0 { part.span } else { out.span };
            }
        };
        placement(_grid_column_, _grid_column_start_, _grid_column_end_, &mut self.m_grid_column);
        placement(_grid_row_, _grid_row_start_, _grid_row_end_, &mut self.m_grid_row);
        if self.m_display == display_inline || self.m_display == display_inline_block {
            self.m_display = display_block;
        } else if self.m_display == display_inline_table {
            self.m_display = display_table;
        } else if self.m_display == display_inline_flex {
            self.m_display = display_flex;
        } else if self.m_display == display_inline_grid {
            self.m_display = display_grid;
        }
    }

    /// `compute_effects(el, doc)`.
    fn compute_effects(&mut self, g: &Get) {
        let normal = CssLength::predef_value(0);
        self.m_row_gap = g.length(_row_gap_, false, normal.clone(), |c| &c.m_row_gap);
        self.m_column_gap = g.length(_column_gap_, false, normal, |c| &c.m_column_gap);
        if self.m_row_gap.is_predefined() {
            self.m_row_gap.assign(0.0);
        } else {
            self.cvt_field(g, |c| &mut c.m_row_gap);
        }
        if self.m_column_gap.is_predefined() {
            self.m_column_gap.assign(0.0);
        } else {
            self.cvt_field(g, |c| &mut c.m_column_gap);
        }

        self.compute_opacity(g);
        self.m_aspect_ratio = g.float(_aspect_ratio_, false, 0.0, |c| &c.m_aspect_ratio);
        self.m_backface_hidden = g.int(_backface_visibility_, false, 0, |c| &c.m_backface_hidden);
        self.m_contain = g.int(_contain_, false, 0, |c| &c.m_contain);
        if self.m_contain != 0 {
            if matches!(
                self.m_display,
                display_inline
                    | display_none
                    | display_table_row
                    | display_table_row_group
                    | display_table_header_group
                    | display_table_footer_group
                    | display_table_column
                    | display_table_column_group
            ) {
                self.m_contain = 0;
            } else if matches!(self.m_display, display_table | display_inline_table | display_table_cell | display_table_caption) {
                self.m_contain &= !(contain_size | contain_inline_size);
            }
        }
        self.m_accent_color = g.color(_accent_color_, true, WebColor::TRANSPARENT, |c| &c.m_accent_color);
        self.m_dark_scheme = g.int(_color_scheme_, true, 0, |c| &c.m_dark_scheme);

        self.m_outline.style = g.int(_outline_style_, false, border_style_none, |c| &c.m_outline.style);
        self.m_outline.color = self.get_color_property(g, _outline_color_, false, self.m_color, |c| &c.m_outline.color);
        self.m_outline.width = g.length(_outline_width_, false, px(border_width_medium_value), |c| &c.m_outline.width);
        if self.m_outline.style == border_style_none || self.m_outline.style == border_style_hidden {
            self.m_outline.width.assign(0.0);
        }
        let mut width = std::mem::take(&mut self.m_outline.width);
        self.snap_border_width(g, &mut width);
        self.m_outline.width = width;
        self.m_outline_offset = g.length(_outline_offset_, false, px(0.0), |c| &c.m_outline_offset);
        self.cvt_field(g, |c| &mut c.m_outline_offset);

        self.m_box_shadows.clear();
        if let Some(tokens) = g.tokens(_box_shadow_)
            && parse_box_shadow(g.cx.names, tokens, &mut self.m_box_shadows, Some(g.cx.container))
        {
            let units = self.units(g);
            let fm = self.m_font_metrics;
            let color = self.m_color;
            for shadow in &mut self.m_box_shadows {
                for len in [&mut shadow.x, &mut shadow.y, &mut shadow.blur, &mut shadow.spread] {
                    units.cvt_units(len, &fm, Px(0.0));
                }
                if shadow.color.is_current_color {
                    shadow.color = color;
                }
            }
        }

        for id in [_filter_, _backdrop_filter_] {
            let mut list = std::mem::take(if id == _filter_ { &mut self.m_filter } else { &mut self.m_backdrop_filter });
            list.clear();
            if let Some(tokens) = g.tokens(id)
                && parse_filter(g.cx.names, tokens, &mut list, Some(g.cx.container))
            {
                let units = self.units(g);
                let fm = self.m_font_metrics;
                for f in &mut list {
                    for len in [&mut f.x, &mut f.y, &mut f.radius] {
                        units.cvt_units(len, &fm, Px(0.0));
                    }
                    f.radius = px(cxx::min_f32(f.radius.val(), 256.0));
                    f.x = px(clamp_f32(f.x.val(), -1024.0, 1024.0));
                    f.y = px(clamp_f32(f.y.val(), -1024.0, 1024.0));
                    if f.color.is_current_color {
                        f.color = self.m_color;
                    }
                }
            }
            if id == _filter_ {
                self.m_filter = list;
            } else {
                self.m_backdrop_filter = list;
            }
        }
        if !self.m_backdrop_filter.is_empty() {
            g.doc.has_backdrop.set(true);
        }

        self.m_clip_path = ClipPathShape::default();
        if let Some(tokens) = g.tokens(_clip_path_)
            && parse_clip_path(tokens, &mut self.m_clip_path)
        {
            let units = self.units(g);
            let fm = self.m_font_metrics;
            for len in &mut self.m_clip_path.values {
                units.cvt_units(len, &fm, Px(0.0));
            }
        }

        self.compute_transform(g);
        self.m_transform_origin_x = CssLength::new(50.0, CssUnits::css_units_percentage);
        self.m_transform_origin_y = CssLength::new(50.0, CssUnits::css_units_percentage);
        if let Some(tokens) = g.tokens(_transform_origin_)
            && parse_transform_origin(tokens, &mut self.m_transform_origin_x, &mut self.m_transform_origin_y)
        {
            self.cvt_field(g, |c| &mut c.m_transform_origin_x);
            self.cvt_field(g, |c| &mut c.m_transform_origin_y);
        }
        // transform-box: fill-box or stroke-box, whose centre is the same: the SVG shape's own box (el_svg).
        self.m_transform_fill_box = false;
        if let Some(tokens) = g.tokens(_transform_box_) {
            for token in tokens {
                if token.ty == IDENT && (token.ident() == b"fill-box" || token.ident() == b"stroke-box") {
                    self.m_transform_fill_box = true;
                }
            }
        }
    }

    /// `snap_border_width(width, doc)` (https://www.w3.org/TR/css-values-4/#snap-a-length-as-a-border-width): a
    /// width in px, at least half a pixel when it is more than none, else floored to a half.
    fn snap_border_width(&self, g: &Get, width: &mut CssLength) {
        if width.is_predefined() || width.units() == CssUnits::css_units_percentage {
            return;
        }

        let mut px = self.units(g).to_pixels(width, &self.m_font_metrics, Px(0.0));

        if px > Px(0.0) && px < Px(0.5) {
            px = Px(0.5);
        } else {
            px = Px((px.value() * 2.0).floor() / 2.0);
        }

        width.set_value(px.value(), CssUnits::css_units_px);
    }
}

/// A `const char*` as the C++ hands it on: up to its first NUL.
fn c_str(value: &[u8]) -> &[u8] {
    &value[..value.iter().position(|c| *c == 0).unwrap_or(value.len())]
}

/// Whether the element is one `css_properties::compute` sees as replaced (`is_replaced()`).
pub fn is_replaced_kind(kind: ElementKind) -> bool {
    matches!(kind, ElementKind::el_image | ElementKind::el_svg)
}
