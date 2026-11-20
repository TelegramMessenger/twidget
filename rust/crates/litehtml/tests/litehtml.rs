//! tests/core_tests.cpp's check of the fork's strtod, ported by name, then what the foundations promise:
//! the name table's spelling, the keyword tables against their enums, `pixel_t`'s fuzzy operators, and
//! the colour, length and marker forms the C++ gives.

mod common;

use common::*;
use twidget_litehtml::css_length::*;
use twidget_litehtml::css_tokenizer::*;
use twidget_litehtml::css_values::*;
use twidget_litehtml::string_id::*;
use twidget_litehtml::web_color::*;
use twidget_litehtml::*;

/// core_tests.cpp, "the fork's own strtod and an exponent that fits no int".
#[test]
fn the_forks_own_strtod_and_an_exponent_that_fits_no_int() {
    let text = b"1e2147483648";
    let (big, end) = strtod::t_strtod(text);
    let (small, _) = strtod::t_strtod(b"1e-2147483649");
    assert!(big.is_infinite() && end == 12 && small == 0.0 && strtod::t_strtod(b"2.5e-3").0 == 0.0025);
}

#[test]
fn strtod_ends_where_tcl_ends() {
    assert_eq!(strtod::t_strtod(b"  -12.5e1x"), (-125.0, 9));
    // No digit: the end is the start and the zero keeps the sign.
    let (v, end) = strtod::t_strtod(b" -x");
    assert!(v == 0.0 && v.is_sign_negative() && end == 0);
    // An exponent mark with no digit is taken all the same.
    assert_eq!(strtod::t_strtod(b"1e"), (1.0, 2));
    assert_eq!(strtod::t_strtod(b"1e+"), (1.0, 3));
    assert_eq!(strtod::t_strtod(b"0x10"), (0.0, 1));
    assert_eq!(strtod::t_strtod(b"5\0 7"), (5.0, 1));
    assert_eq!(html::t_strtof(b"0.1").0, 0.1f32);
}

#[test]
fn the_number_conversion_is_the_spec_formula() {
    assert_eq!(convert_string_to_number(b"12.5e-1"), 1.25);
    assert_eq!(convert_string_to_number(b"-.5"), -0.5);
    assert_eq!(convert_string_to_number(b""), 0.0);
    assert!(convert_string_to_number(b"0e999").is_nan());
    assert_eq!(convert_string_to_number(b"1e999"), f64::INFINITY);
}

#[test]
fn names_start_as_the_list_spells_them() {
    let names = Names::new();
    assert_eq!(&*names.s(_a_), b"a");
    assert_eq!(&*names.s(_border_color_), b"border-color");
    assert_eq!(&*names.s(__tag_before_), b"-tag-before");
    assert_eq!(&*names.s(__webkit_user_select_), b"-webkit-user-select");
    assert_eq!(&*names.s(_monochrome_), b"monochrome");
    assert_eq!(names.usage().names as u32, INITIAL_COUNT + 2);
    assert_eq!(names.find_id(b""), Some(empty_id));
    assert_eq!(names.find_id(b"*"), Some(star_id));
    assert_eq!(names.find_id(b"Border-Color"), None);
    let id = names.id(b"Border-Color");
    assert_eq!(id, StringId(INITIAL_COUNT + 2));
    assert_eq!(names.id(b"Border-Color"), id);
    assert_eq!(names.usage(), StringIdUsage { names: (INITIAL_COUNT + 3) as usize, bytes: names.usage().bytes });
    assert!(names.s(StringId(1 << 30)).is_empty());
}

#[test]
fn tables_are_their_lists() {
    for (name, entries) in TABLES {
        assert!(entries.iter().all(|e| !e.is_empty()), "{name} has an empty entry");
    }
    assert_eq!(css_units_strings[16], "-lh-calc");
    assert_eq!(media_type_strings[10], "speech");
    assert_eq!(flex_align_items_strings[4], "center");
    let units = CssValues::from(&css_units_strings);
    assert_eq!(units.value_index(b"px"), Some(CssUnits::css_units_px.index()));
    assert_eq!(units.value_index(b"PX"), None);
    assert_eq!(units.value_index(b""), None);
    assert_eq!(units.value_by_index(99), "");
    for (i, u) in CssUnits::ALL.iter().enumerate() {
        assert_eq!(u.index(), i as i32);
    }
    assert_eq!(FlexAlignItems::flex_align_items_safe.index(), 0x800);
    assert_eq!(FlexAlignItems::from_index(10), Some(FlexAlignItems::flex_align_items_flex_end));
    assert_eq!(ColorSpace::color_space_polar_start, ColorSpace::color_space_hsl);
    assert_eq!(StyleDisplay::default(), StyleDisplay::display_none);
}

#[test]
fn pixels_compare_within_a_band() {
    let (a, b) = (Px(1.0), Px(1.00005));
    assert!(a == b && !a.lt(&b) && a <= b && a >= b);
    // std::max and std::min keep the first inside the band.
    assert_eq!(a.max(b).value(), 1.0);
    assert_eq!(b.min(a).value(), 1.00005);
    assert!(Px(1.0) < Px(1.001));
    assert!(Px(f32::NAN) != Px(f32::NAN) && !Px(f32::NAN).le(&Px(0.0)));
    assert!(Px(f32::INFINITY) != Px(f32::INFINITY));
    assert_eq!((Px(3.0) / 2).value(), 1.5);
    assert_eq!((1 - Px(0.25)).value(), 0.75);
}

#[test]
fn colors_parse_as_the_fork_parses_them() {
    let names = Names::new();
    let mut hash = CssToken::with_str(HASH, b"f0a8");
    assert_eq!(parse_hash_color(&hash), Some(WebColor::rgba(0xff, 0x00, 0xaa, 0x88)));
    hash.str = b"F0A".to_vec();
    assert_eq!(parse_hash_color(&hash), Some(WebColor::rgba(0xff, 0x00, 0xaa, 0xff)));
    hash.str = b"f0a8z".to_vec();
    assert_eq!(parse_hash_color(&hash), None);

    let n = |v: f32| CssToken::with_number(NUMBER, v, CssNumberType::css_number_integer);
    let p = |v: f32| CssToken::with_number(PERCENTAGE, v, CssNumberType::css_number_integer);
    let comma = || CssToken::new(COMMA);
    let mut rgb = CssToken::with_str(CV_FUNCTION, b"RGBA");
    rgb.value = vec![n(300.0), comma(), n(-5.0), comma(), n(127.5), comma(), p(50.0)];
    assert_eq!(parse_func_color(&names, &rgb), Some(WebColor::rgba(255, 0, 128, 128)));
    // Legacy and modern syntax do not mix: a percentage after numbers is no colour.
    rgb.value = vec![n(1.0), comma(), p(2.0), comma(), n(3.0)];
    assert_eq!(parse_func_color(&names, &rgb), None);
    let mut none = CssToken::with_str(IDENT, b"NONE");
    none.repr = b"NONE".to_vec();
    rgb.value = vec![none, n(255.0), p(100.0), CssToken::new(i32::from(b'/')), p(25.0)];
    assert_eq!(parse_func_color(&names, &rgb), Some(WebColor::rgba(0, 255, 255, 64)));

    let mut hsl = CssToken::with_str(CV_FUNCTION, b"hsl");
    let mut turn = CssToken::with_number(DIMENSION, 0.5, CssNumberType::css_number_number);
    turn.str = b"turn".to_vec();
    hsl.value = vec![turn, comma(), p(100.0), comma(), p(50.0)];
    assert_eq!(parse_func_color(&names, &hsl), Some(WebColor::rgba(0, 255, 255, 255)));
    assert!(names.find_id(b"turn").is_some());

    assert_eq!(resolve_name(b"rEbEcCaPuRpLe", None), b"#663399");
    assert_eq!(resolve_name(b"nocolor", None), b"");
    assert_eq!(WebColor::rgba(1, 2, 0xab, 0).to_string(), b"0102AB");
    assert_eq!(WebColor::rgba(100, 50, 10, 7).darken(0.5), WebColor::rgba(50, 25, 5, 7));
    assert!(WebColor::CURRENT_COLOR == WebColor::BLACK);
}

#[test]
fn lengths_take_what_their_options_allow() {
    let mut len = CssLength::default();
    let mut dim = CssToken::with_number(DIMENSION, 12.0, CssNumberType::css_number_integer);
    dim.str = b"PX".to_vec();
    assert!(len.from_token(&dim, f_length, CssValues::default()));
    assert_eq!((len.val(), len.units()), (12.0, CssUnits::css_units_px));
    assert_eq!(len.to_string(), b"12.000000{px}");
    dim.str = b"none".to_vec();
    assert!(!len.from_token(&dim, f_length, CssValues::default()));
    let zero = CssToken::with_number(NUMBER, 0.00001, CssNumberType::css_number_number);
    assert!(len.from_token(&zero, f_length, CssValues::default()));
    assert!(!len.from_token(&zero, f_percentage, CssValues::default()));
    let auto = CssToken::with_str(IDENT, b"Auto");
    assert!(len.from_token(&auto, f_length, CssValues::from(&flex_basis_strings)));
    assert!(len.is_predefined() && len.predef() == 0 && len.to_string() == b"def(0)");
    // The unit stays what it was under a keyword.
    assert_eq!(len.units(), CssUnits::css_units_none);

    assert!(len.from_token(&start_calc(), f_length_percentage, CssValues::default()));
    assert_eq!(len.units(), CssUnits::css_units_calc);
    let em = |v: f32, unit: CssUnits| if unit == CssUnits::css_units_em { v * 16.0 } else { v };
    assert_eq!(calc_evaluate(len.calc().map(|c| &**c), Px(200.0), Some(&em)).value(), 52.0);
    assert_eq!(len.calc_percent(Px(200.0)).value(), 97.0);
    let mut px = 0.0;
    let resolved = calc_resolve_units(len.calc(), &em, &mut px).expect("a % is left");
    assert_eq!(calc_evaluate(Some(&resolved), Px(100.0), None).value(), 2.0);
    assert_eq!(clamp_length(1e30), 33554432.0);
    assert_eq!(clamp_length(f32::NAN), 0.0);
}

#[test]
fn markers_fall_back_to_decimal() {
    assert_eq!(num_cvt::to_latin_lower(28), b"ab");
    assert_eq!(num_cvt::to_latin_upper(702), b"ZZ");
    assert_eq!(num_cvt::to_greek_lower(25), "αα".as_bytes());
    assert_eq!(num_cvt::to_latin_lower(0), b"0");
    assert_eq!(num_cvt::to_roman_lower(1994), b"mcmxciv");
    assert_eq!(num_cvt::to_roman_upper(4000), b"4000");
    assert_eq!(num_cvt::to_roman_upper(-3), b"-3");
}

#[test]
fn utf8_reads_lead_bytes_only() {
    use twidget_litehtml::utf8_strings::*;
    let mut i = 0;
    assert_eq!(read_utf8_char(b"\xc0\x80a", &mut i), 0);
    assert_eq!(i, 2);
    let mut i = 0;
    assert_eq!(read_utf8_char("é".as_bytes(), &mut i), 0xe9);
    let mut i = 0;
    assert_eq!(read_utf8_char(b"\xe2\x82", &mut i), 0x2080);
    assert_eq!(i, 2);
    let mut out = Vec::new();
    append_char(&mut out, 0xd800);
    append_char(&mut out, 0x110000);
    assert!(out.is_empty());
    assert!(!is_valid_utf8(b"\xed\xa0\x80") && is_valid_utf8("€".as_bytes()));
}
