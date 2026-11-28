//! `css_values.h`: the keyword tables (`split_css_values<N>` of a `;`-separated list, at compile time) and
//! the enums whose values index them. A table's entries are what a keyword must equal, byte for byte, to
//! have that index; an empty entry (a table longer than its list) matches nothing. The enums keep the C++
//! names of their values, and their values; a value that is a flag (`text_decoration_line_*`) is combined
//! with others as its `i32`.

/// `split_css_values<N>(str)`: the pieces between `;`, in an array of `N` (the rest empty); more pieces than
/// `N` do not compile.
pub const fn split_css_values<const N: usize>(str: &'static str) -> [&'static str; N] {
    let mut result: [&'static str; N] = [""; N];
    let bytes = str.as_bytes();
    let mut start = 0;
    let mut index = 0;
    let mut i = 0;
    while i <= bytes.len() {
        if i == bytes.len() || bytes[i] == b';' {
            let (_, tail) = bytes.split_at(start);
            let (piece, _) = tail.split_at(i - start);
            result[index] = match core::str::from_utf8(piece) {
                Ok(piece) => piece,
                Err(_) => panic!("a table is ASCII"),
            };
            index += 1;
            start = i + 1;
        }
        i += 1;
    }
    result
}

/// `css_values`: a table to look keywords up in.
#[derive(Clone, Copy, Debug, Default)]
pub struct CssValues {
    pub data: &'static [&'static str],
}

impl<const N: usize> From<&'static [&'static str; N]> for CssValues {
    fn from(data: &'static [&'static str; N]) -> CssValues {
        CssValues { data }
    }
}

impl CssValues {
    pub const fn new(data: &'static [&'static str]) -> CssValues {
        CssValues { data }
    }

    /// `value_index(str)`: the index of the entry `str` equals; never for the empty string.
    pub fn value_index(&self, str: &[u8]) -> Option<i32> {
        if str.is_empty() {
            return None;
        }
        self.data.iter().position(|entry| entry.as_bytes() == str).map(|i| i as i32)
    }

    /// `has(str)`.
    pub fn has(&self, str: &[u8]) -> bool {
        self.value_index(str).is_some()
    }

    /// `value_by_index(index)`: the entry, empty past the end.
    pub fn value_by_index(&self, index: usize) -> &'static str {
        self.data.get(index).copied().unwrap_or("")
    }
}

/// A C++ enum: its values in order, with the values the C++ gives them.
macro_rules! css_enum {
    (pub enum $name:ident { $($variant:ident $(= $value:expr)?),* $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(i32)]
        pub enum $name {
            $($variant $(= $value)?),*
        }

        impl $name {
            /// Every value, in the C++ order.
            pub const ALL: &'static [$name] = &[$($name::$variant),*];

            /// The value whose number is `index`.
            pub fn from_index(index: i32) -> Option<$name> {
                Self::ALL.iter().copied().find(|v| *v as i32 == index)
            }

            /// The value's number.
            pub const fn index(self) -> i32 {
                self as i32
            }
        }

        /// The first value, which is 0, as the C++ value-initializes it.
        impl Default for $name {
            fn default() -> $name {
                Self::ALL[0]
            }
        }
    };
}

// CSS Property: display

pub const style_display_strings: [&str; 20] = split_css_values::<20>(concat!(
    "none;block;inline;inline-block;inline-table;list-item;table;table-caption;table-cell;table-column;",
    "table-column-group;table-footer-group;table-header-group;table-row;table-row-group;inline-text;flex;",
    "inline-flex;grid;inline-grid"
));

css_enum! {
    pub enum StyleDisplay {
        display_none,
        display_block,
        display_inline,
        display_inline_block,
        display_inline_table,
        display_list_item,
        display_table,
        display_table_caption,
        display_table_cell,
        display_table_column,
        display_table_column_group,
        display_table_footer_group,
        display_table_header_group,
        display_table_row,
        display_table_row_group,
        display_inline_text,
        display_flex,
        display_inline_flex,
        display_grid,
        display_inline_grid,
    }
}

// CSS Property: font-size

pub const font_size_strings: [&str; 9] = split_css_values::<9>("xx-small;x-small;small;medium;large;x-large;xx-large;smaller;larger");

css_enum! {
    pub enum FontSize {
        font_size_xx_small,
        font_size_x_small,
        font_size_small,
        font_size_medium,
        font_size_large,
        font_size_x_large,
        font_size_xx_large,
        font_size_smaller,
        font_size_larger,
    }
}

// CSS Property: line-height

pub const line_height_strings: [&str; 1] = split_css_values::<1>("normal");

css_enum! {
    pub enum LineHeight {
        line_height_normal,
    }
}

// CSS Property: font-style

pub const font_style_strings: [&str; 2] = split_css_values::<2>("normal;italic");

css_enum! {
    pub enum FontStyle {
        font_style_normal,
        font_style_italic,
    }
}

// CSS Property: font-system-family-name

pub const font_system_family_name_strings: [&str; 6] = split_css_values::<6>("caption;icon;menu;message-box;small-caption;status-bar");

// CSS Property: font-variant

pub const font_variant_strings: [&str; 2] = split_css_values::<2>("normal;small-caps");

css_enum! {
    pub enum FontVariant {
        font_variant_normal,
        font_variant_small_caps,
    }
}

// CSS Property: font-weight

pub const font_weight_strings: [&str; 4] = split_css_values::<4>("normal;bold;bolder;lighter");

css_enum! {
    pub enum FontWeight {
        font_weight_normal,
        font_weight_bold,
        font_weight_bolder,
        font_weight_lighter,
    }
}

// CSS Property: list-style-type

pub const list_style_type_strings: [&str; 23] = split_css_values::<23>(concat!(
    "none;circle;disc;square;disclosure-open;disclosure-closed;armenian;cjk-ideographic;decimal;decimal-leading-zero;georgian;hebrew;hiragana;",
    "hiragana-iroha;katakana;katakana-iroha;lower-alpha;lower-greek;lower-latin;lower-roman;upper-alpha;",
    "upper-latin;upper-roman"
));

css_enum! {
    pub enum ListStyleType {
        list_style_type_none,
        list_style_type_circle,
        list_style_type_disc,
        list_style_type_square,
        // Shapes like the three above (anything from armenian on is text): <summary>'s triangles.
        list_style_type_disclosure_open,
        list_style_type_disclosure_closed,
        list_style_type_armenian,
        list_style_type_cjk_ideographic,
        list_style_type_decimal,
        list_style_type_decimal_leading_zero,
        list_style_type_georgian,
        list_style_type_hebrew,
        list_style_type_hiragana,
        list_style_type_hiragana_iroha,
        list_style_type_katakana,
        list_style_type_katakana_iroha,
        list_style_type_lower_alpha,
        list_style_type_lower_greek,
        list_style_type_lower_latin,
        list_style_type_lower_roman,
        list_style_type_upper_alpha,
        list_style_type_upper_latin,
        list_style_type_upper_roman,
    }
}

// CSS Property: list-style-position

pub const list_style_position_strings: [&str; 2] = split_css_values::<2>("inside;outside");

css_enum! {
    pub enum ListStylePosition {
        list_style_position_inside,
        list_style_position_outside,
    }
}

// CSS Property: font-variant

pub const vertical_align_strings: [&str; 8] = split_css_values::<8>("baseline;sub;super;top;text-top;middle;bottom;text-bottom");

css_enum! {
    pub enum VerticalAlign {
        va_baseline,
        va_sub,
        va_super,
        va_top,
        va_text_top,
        va_middle,
        va_bottom,
        va_text_bottom,
    }
}

// CSS Property: border-width

pub const border_width_strings: [&str; 3] = split_css_values::<3>("thin;medium;thick");

css_enum! {
    pub enum BorderWidth {
        border_width_thin,
        border_width_medium,
        border_width_thick,
    }
}

pub const border_width_thin_value: f32 = 1.0;

pub const border_width_medium_value: f32 = 3.0;

pub const border_width_thick_value: f32 = 5.0;

pub const border_width_values: [f32; 3] = [border_width_thin_value, border_width_medium_value, border_width_thick_value];

// CSS Property: border-style

pub const border_style_strings: [&str; 10] = split_css_values::<10>("none;hidden;dotted;dashed;solid;double;groove;ridge;inset;outset");

css_enum! {
    pub enum BorderStyle {
        border_style_none,
        border_style_hidden,
        border_style_dotted,
        border_style_dashed,
        border_style_solid,
        border_style_double,
        border_style_groove,
        border_style_ridge,
        border_style_inset,
        border_style_outset,
    }
}

// CSS Property: float

pub const element_float_strings: [&str; 3] = split_css_values::<3>("none;left;right");

css_enum! {
    pub enum ElementFloat {
        float_none,
        float_left,
        float_right,
    }
}

// CSS Property: clear

pub const element_clear_strings: [&str; 4] = split_css_values::<4>("none;left;right;both");

css_enum! {
    pub enum ElementClear {
        clear_none,
        clear_left,
        clear_right,
        clear_both,
    }
}

// CSS units

pub const css_units_strings: [&str; 17] = split_css_values::<17>("none;%;in;cm;mm;em;ex;pt;pc;px;vw;vh;vmin;vmax;rem;ch;-lh-calc");

css_enum! {
    pub enum CssUnits {
        css_units_none,
        css_units_percentage,
        css_units_in,
        css_units_cm,
        css_units_mm,
        css_units_em,
        css_units_ex,
        css_units_pt,
        css_units_pc,
        css_units_px,
        css_units_vw,
        css_units_vh,
        css_units_vmin,
        css_units_vmax,
        css_units_rem,
        css_units_ch,
        // calc(), min(), max() or clamp() mixing units: the value is the expression's index (calc_evaluate)
        css_units_calc,
    }
}

// CSS Property: background-attachment

pub const background_attachment_strings: [&str; 2] = split_css_values::<2>("scroll;fixed");

css_enum! {
    pub enum BackgroundAttachment {
        background_attachment_scroll,
        background_attachment_fixed,
    }
}

// CSS Property: background-repeat

pub const background_repeat_strings: [&str; 4] = split_css_values::<4>("repeat;repeat-x;repeat-y;no-repeat");

css_enum! {
    pub enum BackgroundRepeat {
        background_repeat_repeat,
        background_repeat_repeat_x,
        background_repeat_repeat_y,
        background_repeat_no_repeat,
    }
}

// CSS Property: background-attachment

// https://drafts.csswg.org/css-box-4/#typedef-visual-box

pub const background_box_strings: [&str; 3] = split_css_values::<3>("border-box;padding-box;content-box");

css_enum! {
    pub enum BackgroundBox {
        background_box_border,
        background_box_padding,
        background_box_content,
    }
}

// CSS Property: background-position

pub const background_position_strings: [&str; 5] = split_css_values::<5>("left;right;top;bottom;center");

pub const background_position_percentages: [f32; 5] = [0.0, 100.0, 0.0, 100.0, 50.0];

css_enum! {
    pub enum BackgroundPosition {
        background_position_left,
        background_position_right,
        background_position_top,
        background_position_bottom,
        background_position_center,
    }
}

// CSS Property: position

pub const element_position_strings: [&str; 5] = split_css_values::<5>("static;relative;absolute;fixed;sticky");

css_enum! {
    pub enum ElementPosition {
        element_position_static,
        element_position_relative,
        element_position_absolute,
        element_position_fixed,
        element_position_sticky,
    }
}

// CSS Property: align

pub const direction_strings: [&str; 2] = split_css_values::<2>("ltr;rtl");

css_enum! {
    pub enum Direction {
        direction_ltr,
        direction_rtl,
    }
}

pub const text_align_strings: [&str; 7] = split_css_values::<7>("left;right;center;justify;start;end;-internal-th-center");

css_enum! {
    pub enum TextAlign {
        text_align_left,
        text_align_right,
        text_align_center,
        text_align_justify,
        text_align_start,
        text_align_end,
        text_align_th_center,
    }
}

// CSS Property: text-transform

pub const text_transform_strings: [&str; 4] = split_css_values::<4>("none;capitalize;uppercase;lowercase");

css_enum! {
    pub enum TextTransform {
        text_transform_none,
        text_transform_capitalize,
        text_transform_uppercase,
        text_transform_lowercase,
    }
}

pub const font_smoothing_strings: [&str; 4] = split_css_values::<4>("auto;none;antialiased;subpixel-antialiased");

css_enum! {
    pub enum FontSmoothing {
        font_smoothing_auto,
        font_smoothing_none,
        font_smoothing_antialiased,
        font_smoothing_subpixel_antialiased,
    }
}

// CSS Property: white-space

pub const white_space_strings: [&str; 5] = split_css_values::<5>("normal;nowrap;pre;pre-line;pre-wrap");

css_enum! {
    pub enum WhiteSpace {
        white_space_normal,
        white_space_nowrap,
        white_space_pre,
        white_space_pre_line,
        white_space_pre_wrap,
    }
}

// CSS Property: overflow

pub const overflow_strings: [&str; 7] = split_css_values::<7>("visible;hidden;scroll;auto;no-display;no-content;clip");

css_enum! {
    pub enum Overflow {
        overflow_visible,
        overflow_hidden,
        overflow_scroll,
        overflow_auto,
        overflow_no_display,
        overflow_no_content,
        overflow_clip,
    }
}

// CSS Property: background-size

pub const background_size_strings: [&str; 3] = split_css_values::<3>("auto;cover;contain");

css_enum! {
    pub enum BackgroundSize {
        background_size_auto, // must be first, see parse_bg_size
        background_size_cover,
        background_size_contain,
    }
}

// CSS Property: visibility

pub const visibility_strings: [&str; 3] = split_css_values::<3>("visible;hidden;collapse");

css_enum! {
    pub enum Visibility {
        visibility_visible,
        visibility_hidden,
        visibility_collapse,
    }
}

// CSS Property: border-collapse

pub const border_collapse_strings: [&str; 2] = split_css_values::<2>("collapse;separate");

pub const table_layout_strings: [&str; 2] = split_css_values::<2>("auto;fixed");

css_enum! {
    pub enum TableLayout {
        table_layout_auto,
        table_layout_fixed,
    }
}

css_enum! {
    pub enum BorderCollapse {
        border_collapse_collapse,
        border_collapse_separate,
    }
}

// CSS Property: content

pub const content_property_strings: [&str; 6] = split_css_values::<6>("none;normal;open-quote;close-quote;no-open-quote;no-close-quote");

css_enum! {
    pub enum ContentProperty {
        content_property_none,
        content_property_normal,
        content_property_open_quote,
        content_property_close_quote,
        content_property_no_open_quote,
        content_property_no_close_quote,
    }
}

// `all` and `contain` are outside widget profile 1.

pub const user_select_strings: [&str; 3] = split_css_values::<3>("auto;text;none");

css_enum! {
    pub enum UserSelect {
        user_select_auto,
        user_select_text,
        user_select_none,
    }
}

pub const overflow_wrap_strings: [&str; 3] = split_css_values::<3>("normal;break-word;anywhere");

css_enum! {
    pub enum OverflowWrap {
        overflow_wrap_normal,
        overflow_wrap_break_word,
        overflow_wrap_anywhere,
    }
}

pub const word_break_strings: [&str; 4] = split_css_values::<4>("normal;break-all;keep-all;break-word");

css_enum! {
    pub enum WordBreak {
        word_break_normal,
        word_break_break_all,
        word_break_keep_all,
        word_break_break_word,
    }
}

pub const resize_strings: [&str; 6] = split_css_values::<6>("none;both;horizontal;vertical;block;inline");

css_enum! {
    pub enum Resize {
        resize_none,
        resize_both,
        resize_horizontal,
        resize_vertical,
        resize_block,
        resize_inline,
    }
}

// CSS Property: appearance

pub const appearance_strings: [&str; 16] = split_css_values::<16>(concat!(
    "none;auto;menulist-button;textfield;button;checkbox;listbox;menulist;meter;progress-bar;push-button;radio;",
    "searchfield;slider-horizontal;square-button;textarea"
));

css_enum! {
    pub enum Appearance {
        appearance_none,
        appearance_auto,
        appearance_menulist_button,
        appearance_textfield,
        appearance_button,
        appearance_checkbox,
        appearance_listbox,
        appearance_menulist,
        appearance_meter,
        appearance_progress_bar,
        appearance_push_button,
        appearance_radio,
        appearance_searchfield,
        appearance_slider_horizontal,
        appearance_square_button,
        appearance_textarea,
    }
}

// CSS Property: box-sizing

pub const box_sizing_strings: [&str; 2] = split_css_values::<2>("content-box;border-box");

css_enum! {
    pub enum BoxSizing {
        box_sizing_content_box,
        box_sizing_border_box,
    }
}

// CSS Property: media-type

// https://drafts.csswg.org/mediaqueries/#media-types

// User agents must recognize the following media types as valid, but must make them match nothing.

macro_rules! deprecated_media_type_strings {
    () => {
        "tty;tv;projection;handheld;braille;embossed;aural;speech"
    };
}

pub const media_type_strings: [&str; 11] = split_css_values::<11>(concat!("all;print;screen;", deprecated_media_type_strings!()));

css_enum! {
    pub enum MediaType {
        media_type_unknown,
        media_type_all,
        media_type_print,
        media_type_screen,
        media_type_first_deprecated,
    }
}

// CSS Property: flex-direction

pub const flex_direction_strings: [&str; 4] = split_css_values::<4>("row;row-reverse;column;column-reverse");

css_enum! {
    pub enum FlexDirection {
        flex_direction_row,
        flex_direction_row_reverse,
        flex_direction_column,
        flex_direction_column_reverse,
    }
}

// CSS Property: flex-wrap

pub const flex_wrap_strings: [&str; 3] = split_css_values::<3>("nowrap;wrap;wrap-reverse");

css_enum! {
    pub enum FlexWrap {
        flex_wrap_nowrap,
        flex_wrap_wrap,
        flex_wrap_wrap_reverse,
    }
}

// CSS Property: flex-justify-content

pub const flex_justify_content_strings: [&str; 12] =
    split_css_values::<12>("normal;flex-start;flex-end;center;space-between;space-around;start;end;left;right;space-evenly;stretch");

css_enum! {
    pub enum FlexJustifyContent {
        flex_justify_content_normal,
        flex_justify_content_flex_start,
        flex_justify_content_flex_end,
        flex_justify_content_center,
        flex_justify_content_space_between,
        flex_justify_content_space_around,
        flex_justify_content_start,
        flex_justify_content_end,
        flex_justify_content_left,
        flex_justify_content_right,
        flex_justify_content_space_evenly,
        flex_justify_content_stretch,
    }
}

macro_rules! self_position_vals {
    () => {
        "center;start;end;self-start;self-end;flex-start;flex-end"
    };
}

pub const self_position_strings: [&str; 7] = split_css_values::<7>(self_position_vals!());

pub const flex_align_items_strings: [&str; 11] = split_css_values::<11>(concat!("auto;normal;stretch;baseline;", self_position_vals!()));

css_enum! {
    pub enum FlexAlignItems {
        flex_align_items_auto, // used for align-self property only
        flex_align_items_normal,
        flex_align_items_stretch,
        flex_align_items_baseline,
        flex_align_items_center,
        flex_align_items_start,
        flex_align_items_end,
        flex_align_items_self_start,
        flex_align_items_self_end,
        flex_align_items_flex_start,
        flex_align_items_flex_end,
        flex_align_items_first = 0x100,
        flex_align_items_last = 0x200,
        flex_align_items_unsafe = 0x400,
        flex_align_items_safe = 0x800,
    }
}

// CSS Property: flex-align-content

pub const flex_align_content_strings: [&str; 8] =
    split_css_values::<8>("flex-start;start;flex-end;end;center;space-between;space-around;stretch");

css_enum! {
    pub enum FlexAlignContent {
        flex_align_content_flex_start,
        flex_align_content_start,
        flex_align_content_flex_end,
        flex_align_content_end,
        flex_align_content_center,
        flex_align_content_space_between,
        flex_align_content_space_around,
        flex_align_content_stretch,
    }
}

// CSS Property: flex-basis

pub const flex_basis_strings: [&str; 5] = split_css_values::<5>("auto;content;fit-content;min-content;max-content");

css_enum! {
    pub enum FlexBasis {
        flex_basis_auto,
        flex_basis_content,
        flex_basis_fit_content,
        flex_basis_min_content,
        flex_basis_max_content,
    }
}

// CSS Property: caption-side

pub const caption_side_strings: [&str; 2] = split_css_values::<2>("top;bottom");

css_enum! {
    pub enum CaptionSide {
        caption_side_top,
        caption_side_bottom,
    }
}

// CSS Property: text-decoration-line

pub const style_text_decoration_line_strings: [&str; 4] = split_css_values::<4>("none;underline;overline;line-through");

css_enum! {
    pub enum TextDecorationLine {
        text_decoration_line_none = 0x00,
        text_decoration_line_underline = 0x01,
        text_decoration_line_overline = 0x02,
        text_decoration_line_line_through = 0x04,
    }
}

// CSS Property: text-decoration-style

pub const style_text_decoration_style_strings: [&str; 5] = split_css_values::<5>("solid;double;dotted;dashed;wavy");

css_enum! {
    pub enum TextDecorationStyle {
        text_decoration_style_solid,
        text_decoration_style_double,
        text_decoration_style_dotted,
        text_decoration_style_dashed,
        text_decoration_style_wavy,
        text_decoration_style_max,
    }
}

// CSS Property: text-decoration-thickness

pub const style_text_decoration_thickness_strings: [&str; 2] = split_css_values::<2>("auto;from-font");

css_enum! {
    pub enum TextDecorationThickness {
        text_decoration_thickness_auto,
        text_decoration_thickness_from_font,
    }
}

// CSS Property: text-emphasis-position

pub const style_text_emphasis_position_strings: [&str; 4] = split_css_values::<4>("over;under;left;right");

css_enum! {
    pub enum TextEmphasisPosition {
        text_emphasis_position_over = 0x00,
        text_emphasis_position_under = 0x01,
        text_emphasis_position_left = 0x02,
        text_emphasis_position_right = 0x04,
    }
}

// CSS Property: radial-extent

pub const radial_extent_strings: [&str; 4] = split_css_values::<4>("closest-corner;closest-side;farthest-corner;farthest-side");

css_enum! {
    pub enum RadialExtent {
        radial_extent_none,
        radial_extent_closest_corner,
        radial_extent_closest_side,
        radial_extent_farthest_corner,
        radial_extent_farthest_side,
    }
}

// CSS Property: color-space

pub const color_space_strings: [&str; 15] =
    split_css_values::<15>("srgb;srgb-linear;display-p3;a98-rgb;prophoto-rgb;rec2020;lab;oklab;xyz;xyz-d50;xyz-d65;hsl;hwb;lch;oklch");

css_enum! {
    pub enum ColorSpace {
        color_space_none,
        // rectangular-color-space
        color_space_srgb,
        color_space_srgb_linear,
        color_space_display_p3,
        color_space_a98_rgb,
        color_space_prophoto_rgb,
        color_space_rec2020,
        color_space_lab,
        color_space_oklab,
        color_space_xyz,
        color_space_xyz_d50,
        color_space_xyz_d65,
        // polar-color-space
        color_space_hsl,
        color_space_hwb,
        color_space_lch,
        color_space_oklch,
    }
}

impl ColorSpace {
    pub const color_space_polar_start: ColorSpace = ColorSpace::color_space_hsl;
}

// CSS Property: hue-interpolation

pub const hue_interpolation_strings: [&str; 4] = split_css_values::<4>("shorter;longer;increasing;decreasing");

css_enum! {
    pub enum HueInterpolation {
        hue_interpolation_none,
        hue_interpolation_shorter,
        hue_interpolation_longer,
        hue_interpolation_increasing,
        hue_interpolation_decreasing,
    }
}
