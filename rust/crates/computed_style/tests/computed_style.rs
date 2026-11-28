//! The strings getComputedStyle answers, as core_tests' `cs` checks and Chromium write them: colours with the
//! shortest alpha, lengths in px, box shorthands, alignments, decorations, custom properties, and an
//! element's whole answer.

use twidget_computed_style::*;
use twidget_litehtml::css_parser::{f_componentize, f_remove_whitespace, normalize};
use twidget_litehtml::css_values::*;
use twidget_litehtml::web_color::WebColor;

fn len(val: f32, units: CssUnits) -> Length {
    Length { predefined: false, predef: 0, units, val }
}

#[test]
fn colours() {
    assert_eq!(color(&WebColor::rgba(0, 100, 0, 255)), "rgb(0, 100, 0)");
    assert_eq!(color(&WebColor::rgba(1, 2, 3, 128)), "rgba(1, 2, 3, 0.5)", "two decimals that map back to the byte");
    assert_eq!(color(&WebColor::rgba(1, 2, 3, 0)), "rgba(1, 2, 3, 0)");
    assert_eq!(color(&WebColor::rgba(1, 2, 3, 1)), "rgba(1, 2, 3, 0.004)", "three when two do not");
}

#[test]
fn lengths() {
    assert_eq!(length(&len(2.0, CssUnits::css_units_em), 13.0, "auto"), "26px");
    assert_eq!(length(&len(3.0, CssUnits::css_units_pt), 16.0, "auto"), "4px");
    assert_eq!(length(&len(33.3333, CssUnits::css_units_percentage), 16.0, "auto"), "33.333%");
    assert_eq!(length(&len(1.0, CssUnits::css_units_in), 16.0, "auto"), "1in", "absolute units past pt keep their own");
    assert_eq!(length(&Length { predefined: true, ..Length::default() }, 16.0, "auto"), "auto");
    assert_eq!(px(0.12345), "0.123px");
}

#[test]
fn shorthands_and_keywords() {
    assert_eq!(box_shorthand("1px", "1px", "1px", "1px"), "1px");
    assert_eq!(box_shorthand("1px", "2px", "1px", "2px"), "1px 2px");
    assert_eq!(box_shorthand("1px", "2px", "3px", "2px"), "1px 2px 3px");
    assert_eq!(box_shorthand("1px", "2px", "3px", "4px"), "1px 2px 3px 4px");
    assert_eq!(alignment(FlexAlignItems::flex_align_items_center.index() | FlexAlignItems::flex_align_items_safe.index()), "safe center");
    assert_eq!(
        alignment(FlexAlignItems::flex_align_items_baseline.index() | FlexAlignItems::flex_align_items_last.index()),
        "last baseline"
    );
    assert_eq!(alignment(200), "", "past the table");
    assert_eq!(text_decoration_line(5), "underline line-through");
    assert_eq!(text_decoration_line(0), "none");
    assert_eq!(keyword(&style_display_strings, -1), "");
    assert_eq!(pseudo_kind(b":before"), Some(true));
    assert_eq!(pseudo_kind(b"::marker"), None);
}

#[test]
fn custom_properties() {
    let text = |s: &str| {
        String::from_utf8(custom_property_text(&normalize(s.as_bytes(), f_componentize | f_remove_whitespace, None))).expect("text")
    };
    assert_eq!(text(" 8px 12px "), "8px 12px", "one space between two values");
    assert_eq!(text("1px , 2px"), "1px,2px", "none next to punctuation");
    // Chromium keeps a custom property's text as written; the fork's tokens have lost the spaces inside a function.
    assert_eq!(text("rgba(0, 0, 0, .5)"), "rgba(0,0,0,.5)");
}

#[test]
fn an_element_and_its_pseudo_elements() {
    let mut el = Element {
        is_tag: true,
        display: StyleDisplay::display_block.index(),
        font_size: 16.0,
        font_weight: 700.0,
        font_family: b"serif".to_vec(),
        line_height_predefined: true,
        color: WebColor::rgba(10, 20, 30, 255),
        margins: [
            len(1.0, CssUnits::css_units_in),
            len(2.0, CssUnits::css_units_px),
            len(0.0, CssUnits::css_units_px),
            len(10.0, CssUnits::css_units_percentage),
        ],
        layout: Some(Layout { margins: [0.0, 2.0, 0.0, 30.0], paddings: [1.0; 4], borders: [0.0; 4] }),
        placement: (100.0, 20.0),
        raw: vec![(b"transform".to_vec(), b" rotate(90deg) ".to_vec())],
        chain: vec![(true, Vec::new(), Vec::new()), (true, b"none".to_vec(), b"'a' 'b'".to_vec())],
        opacity: 1.0,
        flex_shrink: 1.0,
        ..Element::default()
    };
    el.after = Some(Box::new(Element { is_tag: true, content: b"'x'".to_vec(), font_size: 16.0, ..Element::default() }));
    let cs = computed_style(&el, b"", None);
    let get = |key: &str| String::from_utf8_lossy(cs.get(key).as_str()).into_owned();
    assert_eq!(
        get("margin-top"),
        "1px",
        "a margin the fork left in inches (it converts them as it computes) would be its number in px, as the C++ reads it back"
    );
    assert_eq!(get("margin-left"), "30px", "a percentage margin is the one layout used");
    assert_eq!(get("width"), "100px");
    assert_eq!(get("font"), "700 16px serif");
    assert_eq!(get("transform"), "matrix(0, 1, -1, 0, 0, 0)");
    assert_eq!(get("pointer-events"), "none", "inherited from the parent");
    assert_eq!(get("quotes"), "'a' 'b'");
    let after = computed_style(&el, b"::after", None);
    assert_eq!(after.get("content").as_str(), b"\"x\"", "generated content is quoted as Chromium writes it");
    let none = computed_style(&el, b"::before", None);
    assert_eq!(none.dump(), b"{\"content\":\"none\",\"display\":\"inline\"}", "a pseudo-element nothing generates");
}
