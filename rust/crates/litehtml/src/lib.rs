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
pub mod css_length;
pub mod css_parser;
pub mod css_selector;
pub mod css_tokenizer;
pub mod css_values;
pub mod cxx;
pub mod document;
pub mod document_container;
pub mod encodings;
pub mod gradient;
pub mod gumbo;
pub mod html;
pub mod html_tag;
pub mod media_query;
pub mod num_cvt;
pub mod pixel_type;
pub mod string_id;
pub mod strtod;
pub mod style;
pub mod stylesheet;
pub mod types;
pub mod utf8_strings;
pub mod web_color;

pub use pixel_type::Px;
pub use string_id::{Names, StringId};
