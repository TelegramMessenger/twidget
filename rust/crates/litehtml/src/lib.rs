//! The litehtml fork twidget renders with (`litehtml/src`, `litehtml/include/litehtml`), ported a stage at a
//! time (rust/PLAN.md, "L. The litehtml port"). Each module is the C++ file of the same name; text is bytes,
//! as the C++ `std::string`s hold it, and every float is the width the C++ computes it in (`pixel_t` is
//! [`Px`], an `f32` with litehtml's fuzzy comparisons).
//!
//! What the C++ keeps process-wide is an object here: the name table is a [`Names`], made once and handed
//! to whatever reads or adds a name.
//!
//! Constants and enum values keep their C++ names (`_border_color_`, `css_units_px`, `f_length`), so the C++
//! and the port read alike; types are in Rust's case (`css_units` is [`css_values::CssUnits`]).

#![forbid(unsafe_code)]
#![allow(non_upper_case_globals, non_camel_case_types)]

pub mod codepoint;
pub mod css_effects;
pub mod css_length;
pub mod css_parser;
pub mod css_position;
pub mod css_properties;
pub mod css_selector;
pub mod css_tokenizer;
pub mod css_values;
pub mod cxx;
pub mod document;
pub mod document_container;
pub mod el_anchor;
pub mod el_base;
pub mod el_before_after;
pub mod el_div;
pub mod el_font;
pub mod el_form;
pub mod el_image;
pub mod el_link;
pub mod el_para;
pub mod el_style;
pub mod el_svg;
pub mod el_table;
pub mod el_td;
pub mod el_text;
pub mod el_title;
pub mod el_tr;
pub mod element;
pub mod encodings;
pub mod font_description;
pub mod formatting_context;
pub mod gradient;
pub mod gumbo;
pub mod html;
pub mod html_microsyntaxes;
pub mod html_tag;
pub mod layout;
pub mod line_box;
pub mod media_query;
pub mod num_cvt;
pub mod pixel_type;
pub mod render_block;
pub mod render_block_context;
pub mod render_flex;
pub mod render_image;
pub mod render_inline_context;
pub mod render_item;
pub mod render_table;
pub mod string_id;
pub mod strtod;
pub mod style;
pub mod stylesheet;
pub mod svg_paint;
pub mod table;
pub mod types;
pub mod utf8_strings;
pub mod web_color;

pub use document::Document;
pub use element::{ElementId, ElementKind};
pub use pixel_type::Px;
pub use string_id::{Names, StringId};
