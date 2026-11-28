//! `string_id.h` and `string_id.cpp`: the name table. Every name litehtml compares by identity (tags,
//! attributes, properties, pseudo-classes, units, at-rules) is a [`StringId`], its index in the table. The
//! table starts with the names of the `STRING_ID` list, in its order, spelled as `init()` spells them (the
//! underscores around each identifier dropped, the others made hyphens: `_border_color_` is
//! `border-color`, `__tag_before_` is `-tag-before`), then `""` and `"*"`; any other name gets the next index
//! the first time it is asked for, and keeps it.
//!
//! In the C++ the table is one per process behind a mutex; here it is a [`Names`], which the helper makes
//! once and shares (it is `Sync`), and a test can make fresh.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crate::html::{split_string, trim};

/// `string_id`: an index in a [`Names`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StringId(pub u32);

macro_rules! string_ids {
    ($($name:ident),* $(,)?) => {
        #[allow(dead_code, clippy::enum_variant_names)]
        #[repr(u32)]
        enum Initial {
            $($name),*
        }

        $(
            pub const $name: StringId = StringId(Initial::$name as u32);
        )*

        /// `initial_string_ids`: the `STRING_ID` list as its stringification spells it.
        pub const INITIAL_STRING_IDS: &str = stringify!($($name),*,);

        /// How many names the list has.
        pub const INITIAL_COUNT: u32 = [$(stringify!($name)),*].len() as u32;
    };
}

string_ids!(
    _a_,
    _abbr_,
    _acronym_,
    _address_,
    _applet_,
    _area_,
    _article_,
    _aside_,
    _audio_,
    _b_,
    _base_,
    _basefont_,
    _bdi_,
    _bdo_,
    _big_,
    _blockquote_,
    _body_,
    _br_,
    _button_,
    _canvas_,
    _caption_,
    _center_,
    _cite_,
    _code_,
    _col_,
    _colgroup_,
    _data_,
    _datalist_,
    _dd_,
    _del_,
    _details_,
    _dfn_,
    _dialog_,
    _dir_,
    _div_,
    _dl_,
    _dt_,
    _em_,
    _embed_,
    _fieldset_,
    _figcaption_,
    _figure_,
    _footer_,
    _form_,
    _frame_,
    _frameset_,
    _h1_,
    _h2_,
    _h3_,
    _h4_,
    _h5_,
    _h6_,
    _head_,
    _header_,
    _hr_,
    _html_,
    _i_,
    _iframe_,
    _img_,
    _input_,
    _ins_,
    _kbd_,
    _label_,
    _legend_,
    _li_,
    _link_,
    _main_,
    _map_,
    _mark_,
    _meta_,
    _meter_,
    _nav_,
    _noframes_,
    _noscript_,
    _object_,
    _ol_,
    _optgroup_,
    _option_,
    _output_,
    _p_,
    _param_,
    _picture_,
    _pre_,
    _progress_,
    _q_,
    _rp_,
    _rt_,
    _ruby_,
    _s_,
    _samp_,
    _script_,
    _section_,
    _select_,
    _small_,
    _source_,
    _span_,
    _strike_,
    _strong_,
    _style_,
    _sub_,
    _summary_,
    _sup_,
    _svg_,
    _table_,
    _tbody_,
    _td_,
    _template_,
    _textarea_,
    _tfoot_,
    _th_,
    _thead_,
    _time_,
    _title_,
    _tr_,
    _track_,
    _tt_,
    _u_,
    _ul_,
    _var_,
    _video_,
    _wbr_,
    __tag_before_,
    __tag_after_,
    _before_,
    _after_,
    _root_,
    _empty_,
    _only_child_,
    _only_of_type_,
    _first_child_,
    _first_of_type_,
    _last_child_,
    _last_of_type_,
    _nth_child_,
    _nth_of_type_,
    _nth_last_child_,
    _nth_last_of_type_,
    _is_,
    _where_,
    _not_,
    _lang_,
    _checked_,
    _disabled_,
    _enabled_,
    _active_,
    _hover_,
    _background_,
    _background_color_,
    _background_image_,
    _background_image_baseurl_,
    _background_repeat_,
    _background_origin_,
    _background_clip_,
    _background_attachment_,
    _background_size_,
    _background_position_,
    _background_position_x_,
    _background_position_y_,
    _background_gradient_,
    _border_,
    _border_width_,
    _border_style_,
    _border_color_,
    _border_spacing_,
    __litehtml_border_spacing_x_,
    __litehtml_border_spacing_y_,
    _border_top_,
    _border_right_,
    _border_bottom_,
    _border_left_,
    _border_top_style_,
    _border_right_style_,
    _border_bottom_style_,
    _border_left_style_,
    _border_top_width_,
    _border_right_width_,
    _border_bottom_width_,
    _border_left_width_,
    _border_top_color_,
    _border_right_color_,
    _border_bottom_color_,
    _border_left_color_,
    _border_radius_,
    _border_radius_x_,
    _border_radius_y_,
    _border_top_left_radius_,
    _border_top_right_radius_,
    _border_bottom_right_radius_,
    _border_bottom_left_radius_,
    _border_top_left_radius_x_,
    _border_top_right_radius_x_,
    _border_bottom_right_radius_x_,
    _border_bottom_left_radius_x_,
    _border_top_left_radius_y_,
    _border_top_right_radius_y_,
    _border_bottom_right_radius_y_,
    _border_bottom_left_radius_y_,
    _list_style_,
    _list_style_type_,
    _list_style_position_,
    _list_style_image_,
    _list_style_image_baseurl_,
    _margin_,
    _margin_top_,
    _margin_right_,
    _margin_bottom_,
    _margin_left_,
    _padding_,
    _padding_top_,
    _padding_right_,
    _padding_bottom_,
    _padding_left_,
    _font_,
    _font_family_,
    _font_style_,
    _font_variant_,
    _font_weight_,
    _font_size_,
    _line_height_,
    _text_decoration_,
    _text_decoration_style_,
    _text_decoration_line_,
    _text_decoration_color_,
    _text_decoration_thickness_,
    _text_emphasis_,
    _text_emphasis_style_,
    _text_emphasis_color_,
    _text_emphasis_position_,
    _white_space_,
    _overflow_wrap_,
    _word_wrap_,
    _word_break_,
    _text_align_,
    _direction_,
    _vertical_align_,
    _color_,
    _width_,
    _height_,
    _min_width_,
    _min_height_,
    _max_width_,
    _max_height_,
    _position_,
    _overflow_,
    _overflow_x_,
    _overflow_y_,
    _display_,
    _visibility_,
    _appearance_,
    _resize_,
    _box_sizing_,
    _z_index_,
    _float_,
    _clear_,
    _text_indent_,
    _top_,
    _right_,
    _bottom_,
    _left_,
    _cursor_,
    _content_,
    _user_select_,
    __webkit_user_select_,
    _border_collapse_,
    _text_transform_,
    _flex_,
    _flex_flow_,
    _flex_direction_,
    _flex_wrap_,
    _justify_content_,
    _align_items_,
    _align_content_,
    _align_self_,
    _flex_grow_,
    _flex_shrink_,
    _flex_basis_,
    _caption_side_,
    _order_,
    _counter_reset_,
    _counter_increment_,
    _gap_,
    _row_gap_,
    _column_gap_,
    _grid_gap_,
    _grid_row_gap_,
    _grid_column_gap_,
    _grid_,
    _grid_template_,
    _grid_template_columns_,
    _grid_template_rows_,
    _grid_auto_rows_,
    _grid_column_,
    _grid_row_,
    _grid_column_start_,
    _grid_column_end_,
    _grid_row_start_,
    _grid_row_end_,
    _justify_items_,
    _justify_self_,
    _place_items_,
    _place_self_,
    _place_content_,
    _letter_spacing_,
    _word_spacing_,
    _opacity_,
    _clip_path_,
    _box_shadow_,
    _transform_,
    _translate_,
    _rotate_,
    _scale_,
    _transform_origin_,
    _transform_box_,
    _backface_visibility_,
    _contain_,
    _accent_color_,
    _color_scheme_,
    _outline_,
    _outline_width_,
    _outline_style_,
    _outline_color_,
    _outline_offset_,
    _inset_,
    _fill_,
    _fill_opacity_,
    _fill_rule_,
    _stroke_,
    _stroke_width_,
    _stroke_opacity_,
    _stroke_linecap_,
    _stroke_linejoin_,
    _stroke_miterlimit_,
    _stroke_dasharray_,
    _stroke_dashoffset_,
    _text_anchor_,
    _text_shadow_,
    _filter_,
    _backdrop_filter_,
    __webkit_backdrop_filter_,
    __webkit_font_smoothing_,
    _table_layout_,
    _font_variant_numeric_,
    _font_feature_settings_,
    _deg_,
    _grad_,
    _rad_,
    _turn_,
    _initial_,
    _auto_,
    _none_,
    _linear_gradient_,
    _repeating_linear_gradient_,
    _radial_gradient_,
    _repeating_radial_gradient_,
    _conic_gradient_,
    _repeating_conic_gradient_,
    _charset_,
    _layer_,
    _supports_,
    _import_,
    _media_,
    _and_,
    _or_,
    _boolean_,
    _plain_,
    _range_,
    _discrete_,
    _integer_,
    _length_,
    _resolution_,
    _ratio_,
    _keyword_,
    _orientation_,
    _portrait_,
    _landscape_,
    _device_width_,
    _device_height_,
    _aspect_ratio_,
    _device_aspect_ratio_,
    _color_index_,
    _monochrome_
);

/// `empty_id`: `_id("")`, the first name after the list.
pub const empty_id: StringId = StringId(INITIAL_COUNT);
/// `star_id`: `_id("*")`.
pub const star_id: StringId = StringId(INITIAL_COUNT + 1);

/// `string_id_usage`: how many names the table holds and their bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StringIdUsage {
    pub names: usize,
    pub bytes: usize,
}

thread_local! {
    /// string_id.cpp's `thread_names` and `thread_bytes`: the names the calling thread added to a table and their
    /// bytes, since the thread began (one count for every table, as the C++ has one table).
    static THREAD_USAGE: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

#[derive(Debug, Default)]
struct Table {
    map: BTreeMap<Vec<u8>, StringId>,
    array: Vec<Arc<[u8]>>,
    array_bytes: usize,
}

impl Table {
    fn id(&mut self, str: &[u8]) -> StringId {
        if let Some(&id) = self.map.get(str) {
            return id;
        }
        // str not found: add it to the array and the map
        self.array.push(Arc::from(str));
        self.array_bytes += str.len();
        THREAD_USAGE.with(|u| {
            let (names, bytes) = u.get();
            u.set((names + 1, bytes + str.len()));
        });
        let id = StringId((self.array.len() - 1) as u32);
        self.map.insert(str.to_vec(), id);
        id
    }
}

/// litehtml's name table (`_id`, `_find_id`, `_s`, `_id_usage`).
#[derive(Debug)]
pub struct Names {
    table: Mutex<Table>,
    /// style.cpp's `raw_declaration_ids()`, a function-local static: interned the first time it is asked for.
    raw_declarations: OnceLock<crate::style::RawDeclarationIds>,
}

impl Default for Names {
    fn default() -> Names {
        Names::new()
    }
}

impl Names {
    /// The table as the C++ has it when its statics are made: `init()`, then `empty_id` and `star_id`.
    pub fn new() -> Names {
        let names = Names { table: Mutex::new(Table::default()), raw_declarations: OnceLock::new() };
        for name in split_string(INITIAL_STRING_IDS.as_bytes(), b",", b"", b"\"") {
            let name = trim(&name, crate::html::WHITESPACE);
            debug_assert!(name.first() == Some(&b'_') && name.last() == Some(&b'_'));
            // _border_color_ -> border_color -> border-color
            let name: Vec<u8> = name[1..name.len() - 1].iter().map(|&c| if c == b'_' { b'-' } else { c }).collect();
            names.id(&name);
        }
        let empty = names.id(b"");
        let star = names.id(b"*");
        debug_assert!(empty == empty_id && star == star_id);
        names
    }

    fn lock(&self) -> MutexGuard<'_, Table> {
        self.table.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// `_id(str)`: the name's id, added when it is new.
    pub fn id(&self, str: &[u8]) -> StringId {
        self.lock().id(str)
    }

    /// `_find_id(str, id)`: the name's id when the table has it.
    pub fn find_id(&self, str: &[u8]) -> Option<StringId> {
        self.lock().map.get(str).copied()
    }

    /// `_s(id)`: the name. An id the table never gave is undefined in the C++ (a deque read past its end);
    /// here it is the empty name.
    pub fn s(&self, id: StringId) -> Arc<[u8]> {
        self.lock().array.get(id.0 as usize).cloned().unwrap_or_else(|| Arc::from(&b""[..]))
    }

    /// `_id_usage()`.
    pub fn usage(&self) -> StringIdUsage {
        let table = self.lock();
        StringIdUsage { names: table.array.len(), bytes: table.array_bytes }
    }

    /// `_id_thread_usage()`: the names the calling thread added and their bytes, since the thread began.
    pub fn thread_usage() -> StringIdUsage {
        let (names, bytes) = THREAD_USAGE.with(Cell::get);
        StringIdUsage { names, bytes }
    }

    /// The names whose declarations the renderer keeps raw, with their `-lh-raw-` twins (style.cpp's
    /// `raw_declaration_ids()`), interned in this table the first time any style asks: once per process, as the
    /// C++ static.
    pub fn raw_declaration_ids(&self) -> &crate::style::RawDeclarationIds {
        self.raw_declarations.get_or_init(|| crate::style::intern_raw_declaration_ids(self))
    }
}
