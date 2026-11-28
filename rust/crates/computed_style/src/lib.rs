//! The `cs` query, getComputedStyle (`core/src/computed_style.h`, `computed_style.cpp`): what the litehtml
//! fork computed for an element, written out as Chromium writes it, since widgets and widget-sandbox's
//! conformance set compare against the browser's strings.
//!
//! The C++ reads an element of the fork; what it reads is an [`Element`]: every computed value, the layout box,
//! the raw declarations and the ancestors' ones `computed_style` looks at, the SVG paints and the `::before` and
//! `::after` children, as the fork has them. [`Element::of_cascade`] makes one of an element of the Rust
//! litehtml after its cascade (stage L5); the layout box is later stages'. The helpers (`px`, `color`,
//! `length`, …) are the C++'s, by name.

#![forbid(unsafe_code)]

use twidget_css::{format_number, parse_color, parse_transform, trim};
use twidget_json::Json;
use twidget_litehtml::css_length::CssLength;
use twidget_litehtml::css_properties::CssBorder;
use twidget_litehtml::css_tokenizer::CssToken;
use twidget_litehtml::css_values::*;
use twidget_litehtml::document::{Document, StyleCx};
use twidget_litehtml::element::{ElementId, ElementKind};
use twidget_litehtml::gradient::ImageType;
use twidget_litehtml::web_color::WebColor;

/// A `css_length` as computed_style reads it: keyword or value, its unit, and `val()` (a calc length's
/// value against no size).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Length {
    pub predefined: bool,
    pub predef: i32,
    pub units: CssUnits,
    pub val: f32,
}

impl Default for Length {
    fn default() -> Length {
        Length { predefined: false, predef: 0, units: CssUnits::css_units_none, val: 0.0 }
    }
}

impl Length {
    pub fn of(l: &CssLength) -> Length {
        Length { predefined: l.is_predefined(), predef: l.predef(), units: l.units(), val: l.val() }
    }
}

/// A `css_border`: its style, width and colour.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Border {
    pub style: i32,
    pub width: Length,
    pub color: WebColor,
}

/// A background layer's image: `type` (0 none, 1 url, 2 gradient), its URL, and a gradient's type name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Image {
    pub kind: i32,
    pub url: Vec<u8>,
    pub gradient: Vec<u8>,
}

/// The boxes layout used (a render item's margins, paddings and borders), top, right, bottom, left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Layout {
    pub margins: [f32; 4],
    pub paddings: [f32; 4],
    pub borders: [f32; 4],
}

/// One element as computed_style reads it from the fork. Sides are top, right, bottom, left; radii top-left,
/// top-right, bottom-right, bottom-left, x then y.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Element {
    /// An `html_tag` (the raw declarations and pseudo children are a tag's).
    pub is_tag: bool,
    pub display: i32,
    pub position: i32,
    pub float: i32,
    pub clear: i32,
    pub box_sizing: i32,
    pub visibility: i32,
    pub z_index: i32,
    pub direction: i32,
    pub borders: [Border; 4],
    pub radius: [Length; 8],
    pub margins: [Length; 4],
    pub padding: [Length; 4],
    pub width: Length,
    pub height: Length,
    pub border_collapse: i32,
    pub border_spacing_x: Length,
    pub border_spacing_y: Length,
    pub caption_side: i32,
    pub min_width: Length,
    pub min_height: Length,
    pub max_width: Length,
    pub max_height: Length,
    pub offsets: [Length; 4],
    pub overflow_x: i32,
    pub overflow_y: i32,
    pub white_space: i32,
    pub overflow_wrap: i32,
    pub word_break: i32,
    pub color: WebColor,
    pub font_size: f32,
    /// `font_weight().val()`.
    pub font_weight: f32,
    pub font_style: i32,
    pub font_family: Vec<u8>,
    /// `line_height().css_value.is_predefined()` and `computed_value`.
    pub line_height_predefined: bool,
    pub line_height: f32,
    pub text_align: i32,
    pub text_indent: Length,
    pub text_transform: i32,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub vertical_align: i32,
    pub text_decoration_line: i32,
    pub text_decoration_style: i32,
    pub text_decoration_color: WebColor,
    pub user_select: i32,
    pub list_style_type: i32,
    pub list_style_position: i32,
    pub list_style_image: Vec<u8>,
    pub bg_color: WebColor,
    pub bg_images: Vec<Image>,
    pub bg_sizes: Vec<(Length, Length)>,
    pub bg_position_x: Vec<Length>,
    pub bg_position_y: Vec<Length>,
    pub bg_repeat: Vec<i32>,
    pub opacity: f32,
    pub cursor: Vec<u8>,
    pub transform_origin_x: Length,
    pub transform_origin_y: Length,
    pub outline: Border,
    pub outline_offset: f32,
    pub flex_direction: i32,
    pub flex_wrap: i32,
    pub justify_content: i32,
    pub align_items: i32,
    pub align_self: i32,
    pub align_content: i32,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: Length,
    pub order: i32,
    pub row_gap: Length,
    pub column_gap: Length,
    pub aspect_ratio: f32,
    pub content: Vec<u8>,
    /// The render item's boxes, when it has one.
    pub layout: Option<Layout>,
    /// `get_placement()`'s width and height.
    pub placement: (f32, f32),
    /// `raw_declaration(name)` for the names computed_style reads, as the tag keeps them (empty when none).
    pub raw: Vec<(Vec<u8>, Vec<u8>)>,
    /// From the element up to the root: whether each is a tag, and its raw `pointer-events` and `quotes`.
    pub chain: Vec<(bool, Vec<u8>, Vec<u8>)>,
    /// `svg_paint_color` for fill and for stroke, when it found one.
    pub fill_paint: Option<WebColor>,
    pub stroke_paint: Option<WebColor>,
    /// The element's first `::before` and `::after` children.
    pub before: Option<Box<Element>>,
    pub after: Option<Box<Element>>,
}

impl Element {
    fn raw_declaration(&self, name: &str) -> &[u8] {
        self.raw.iter().find(|(n, _)| n == name.as_bytes()).map_or(&[][..], |(_, v)| v)
    }
}

/// The names whose `-lh-raw-` twins computed_style reads, in the order [`Element::raw`] keeps them.
pub const RAW_NAMES: [&str; 11] = [
    "touch-action",
    "font-variant-numeric",
    "filter",
    "backdrop-filter",
    "box-shadow",
    "translate",
    "rotate",
    "scale",
    "transform",
    "fill",
    "stroke",
];

impl Element {
    /// What computed_style reads of an element of the Rust litehtml after its cascade (stage L5): everything but
    /// the layout, which later stages give (no render item, a placement of none).
    pub fn of_cascade(doc: &Document, cx: &StyleCx, id: ElementId) -> Element {
        Element::of_cascade_at(doc, cx, id, 0)
    }

    fn of_cascade_at(doc: &Document, cx: &StyleCx, id: ElementId, depth: i32) -> Element {
        let el = doc.el(id);
        let css = &el.css;
        let is_tag = el.kind.is_html_tag();
        let border = |b: &CssBorder| Border { style: b.style, width: Length::of(&b.width), color: b.color };
        let b = &css.m_css_borders;
        let r = &b.radius;
        let m = &css.m_css_margins;
        let p = &css.m_css_padding;
        let o = &css.m_css_offsets;
        let mut chain = Vec::new();
        let mut cur = Some(id);
        while let Some(e) = cur {
            let tag = doc.kind(e).is_html_tag();
            let raw = |name: &[u8]| if tag { doc.raw_declaration(cx.names, e, name) } else { Vec::new() };
            chain.push((tag, raw(b"pointer-events"), raw(b"quotes")));
            cur = doc.parent(e);
        }
        let (fill, fill_color) = doc.svg_paint_color(cx, id, true);
        let (stroke, stroke_color) = doc.svg_paint_color(cx, id, false);
        let mut before = None;
        let mut after = None;
        if is_tag && depth == 0 {
            for &child in doc.children(id) {
                if before.is_none() && doc.kind(child) == ElementKind::el_before {
                    before = Some(child);
                }
                if after.is_none() && doc.kind(child) == ElementKind::el_after {
                    after = Some(child);
                }
            }
        }
        let pseudo = |child: Option<ElementId>| child.map(|c| Box::new(Element::of_cascade_at(doc, cx, c, depth + 1)));
        Element {
            is_tag,
            display: css.m_display,
            position: css.m_el_position,
            float: css.m_float,
            clear: css.m_clear,
            box_sizing: css.m_box_sizing,
            visibility: css.m_visibility,
            z_index: css.get_z_index(),
            direction: css.m_direction,
            borders: [border(&b.top), border(&b.right), border(&b.bottom), border(&b.left)],
            radius: [
                Length::of(&r.top_left_x),
                Length::of(&r.top_left_y),
                Length::of(&r.top_right_x),
                Length::of(&r.top_right_y),
                Length::of(&r.bottom_right_x),
                Length::of(&r.bottom_right_y),
                Length::of(&r.bottom_left_x),
                Length::of(&r.bottom_left_y),
            ],
            margins: [Length::of(&m.top), Length::of(&m.right), Length::of(&m.bottom), Length::of(&m.left)],
            padding: [Length::of(&p.top), Length::of(&p.right), Length::of(&p.bottom), Length::of(&p.left)],
            width: Length::of(&css.m_css_width),
            height: Length::of(&css.m_css_height),
            border_collapse: css.m_border_collapse,
            border_spacing_x: Length::of(&css.m_css_border_spacing_x),
            border_spacing_y: Length::of(&css.m_css_border_spacing_y),
            caption_side: css.m_caption_side,
            min_width: Length::of(&css.m_css_min_width),
            min_height: Length::of(&css.m_css_min_height),
            max_width: Length::of(&css.m_css_max_width),
            max_height: Length::of(&css.m_css_max_height),
            offsets: [Length::of(&o.top), Length::of(&o.right), Length::of(&o.bottom), Length::of(&o.left)],
            overflow_x: css.m_overflow_x,
            overflow_y: css.m_overflow_y,
            white_space: css.m_white_space,
            overflow_wrap: css.m_overflow_wrap,
            word_break: css.m_word_break,
            color: css.m_color,
            font_size: css.get_font_size().value(),
            font_weight: css.m_font_weight.val(),
            font_style: css.m_font_style,
            font_family: css.m_font_family.clone(),
            line_height_predefined: css.m_line_height.css_value.is_predefined(),
            line_height: css.m_line_height.computed_value.value(),
            text_align: css.m_text_align,
            text_indent: Length::of(&css.m_css_text_indent),
            text_transform: css.m_text_transform,
            letter_spacing: css.m_letter_spacing.val(),
            word_spacing: css.m_word_spacing.val(),
            vertical_align: css.m_vertical_align,
            text_decoration_line: css.m_text_decoration_line,
            text_decoration_style: css.m_text_decoration_style,
            text_decoration_color: css.m_text_decoration_color,
            user_select: css.m_user_select,
            list_style_type: css.m_list_style_type,
            list_style_position: css.m_list_style_position,
            list_style_image: css.m_list_style_image.clone(),
            bg_color: css.m_bg.m_color,
            bg_images: css
                .m_bg
                .m_image
                .iter()
                .map(|img| Image {
                    kind: img.ty as i32,
                    url: img.url.clone(),
                    gradient: if img.ty == ImageType::type_gradient { cx.names.s(img.m_gradient.m_type).to_vec() } else { Vec::new() },
                })
                .collect(),
            bg_sizes: css.m_bg.m_size.iter().map(|s| (Length::of(&s.width), Length::of(&s.height))).collect(),
            bg_position_x: css.m_bg.m_position_x.iter().map(Length::of).collect(),
            bg_position_y: css.m_bg.m_position_y.iter().map(Length::of).collect(),
            bg_repeat: css.m_bg.m_repeat.clone(),
            opacity: css.m_opacity,
            cursor: css.m_cursor.clone(),
            transform_origin_x: Length::of(&css.m_transform_origin_x),
            transform_origin_y: Length::of(&css.m_transform_origin_y),
            outline: border(&css.m_outline),
            outline_offset: css.m_outline_offset.val(),
            flex_direction: css.m_flex_direction,
            flex_wrap: css.m_flex_wrap,
            justify_content: css.m_flex_justify_content,
            align_items: css.m_flex_align_items,
            align_self: css.m_flex_align_self,
            align_content: css.m_flex_align_content,
            flex_grow: css.m_flex_grow,
            flex_shrink: css.m_flex_shrink,
            flex_basis: Length::of(&css.m_flex_basis),
            order: css.m_order,
            row_gap: Length::of(&css.m_row_gap),
            column_gap: Length::of(&css.m_column_gap),
            aspect_ratio: css.m_aspect_ratio,
            content: css.m_content.clone(),
            layout: None,
            placement: (0.0, 0.0),
            raw: RAW_NAMES
                .iter()
                .map(|n| (n.as_bytes().to_vec(), if is_tag { doc.raw_declaration(cx.names, id, n.as_bytes()) } else { Vec::new() }))
                .collect(),
            chain,
            fill_paint: Some(fill_color).filter(|_| fill),
            stroke_paint: Some(stroke_color).filter(|_| stroke),
            before: pseudo(before),
            after: pseudo(after),
        }
    }
}

/// `px(v)`: three decimals at most, and "px".
pub fn px(v: f64) -> String {
    format_number(v, 3) + "px"
}

/// `rgb(r, g, b)`, or `rgba(…, a)` with the shortest of 2 or 3 decimals that maps back to the alpha byte.
pub fn color(c: &WebColor) -> String {
    let rgb = format!("{}, {}, {}", c.red, c.green, c.blue);
    if c.alpha == 255 {
        return format!("rgb({rgb})");
    }
    // Chromium keeps 8 bits of alpha and writes the shortest of 2 or 3 decimals that maps back to them.
    let a = f64::from(c.alpha) / 255.0;
    let two = (a * 100.0).round() / 100.0;
    let alpha = if (two * 255.0).round() as i64 == i64::from(c.alpha) { format_number(two, 2) } else { format_number(a, 3) };
    format!("rgba({rgb}, {alpha})")
}

/// `Keyword(names, index)`: the entry, empty past the table (a negative index too).
pub fn keyword(names: &[&str], index: i32) -> String {
    usize::try_from(index).ok().and_then(|i| names.get(i)).map_or(String::new(), |s| s.to_string())
}

/// A declared length as a computed one: px for absolute and font-relative units, % kept.
pub fn length(l: &Length, font_size: f64, predefined: &str) -> String {
    if l.predefined {
        return predefined.to_string();
    }
    let v = f64::from(l.val);
    match l.units {
        CssUnits::css_units_percentage => format_number(v, 3) + "%",
        CssUnits::css_units_em => px(v * font_size),
        CssUnits::css_units_rem => px(v * 16.0),
        CssUnits::css_units_pt => px(v * 4.0 / 3.0),
        // The fork keeps no text for math that waits for layout: what it makes of no size is the best answer.
        CssUnits::css_units_none | CssUnits::css_units_px | CssUnits::css_units_calc => px(v),
        units => format_number(v, 3) + &keyword(&css_units_strings, units.index()),
    }
}

/// `Alignment(value)`: an align-items or align-self keyword with its safe, unsafe or last flag.
pub fn alignment(value: i32) -> String {
    let result = keyword(&flex_align_items_strings, value & 0xFF);
    if result.is_empty() {
        return result;
    }
    if value & FlexAlignItems::flex_align_items_safe.index() != 0 {
        return "safe ".to_string() + &result;
    }
    if value & FlexAlignItems::flex_align_items_unsafe.index() != 0 {
        return "unsafe ".to_string() + &result;
    }
    if value & FlexAlignItems::flex_align_items_last.index() != 0 {
        return "last ".to_string() + &result;
    }
    result
}

/// `top right bottom left` as Chromium shortens a box shorthand.
pub fn box_shorthand(top: &str, right: &str, bottom: &str, left: &str) -> String {
    if left == right {
        if top == bottom {
            return if top == right { top.to_string() } else { format!("{top} {right}") };
        }
        return format!("{top} {right} {bottom}");
    }
    format!("{top} {right} {bottom} {left}")
}

/// `raw(tag, name, fallback)`: the declaration trimmed, or `fallback` when there is none (or no tag).
pub fn raw(declaration: Option<&[u8]>, fallback: &[u8]) -> Vec<u8> {
    let v = declaration.map(trim).unwrap_or_default();
    if v.is_empty() { fallback.to_vec() } else { v }
}

pub fn text_decoration_line(bits: i32) -> &'static str {
    let under = bits & TextDecorationLine::text_decoration_line_underline.index() != 0;
    let over = bits & TextDecorationLine::text_decoration_line_overline.index() != 0;
    let through = bits & TextDecorationLine::text_decoration_line_line_through.index() != 0;
    match (under, over, through) {
        (true, true, true) => "underline overline line-through",
        (true, true, false) => "underline overline",
        (true, false, true) => "underline line-through",
        (false, true, true) => "overline line-through",
        (true, false, false) => "underline",
        (false, true, false) => "overline",
        (false, false, true) => "line-through",
        (false, false, false) => "none",
    }
}

/// Which pseudo-element `pseudo` names: `Some(true)` for `::before` (or `:before`), `Some(false)` for
/// `::after`, None for anything else.
pub fn pseudo_kind(pseudo: &[u8]) -> Option<bool> {
    match pseudo {
        b"::before" | b":before" => Some(true),
        b"::after" | b":after" => Some(false),
        _ => None,
    }
}

/// `computed_custom_property`'s text of the tokens the cascade left: the fork drops a declaration's whitespace
/// as it parses, so one space goes back between two values, as in `8px 12px`, and none next to punctuation
/// (`,` `/`), whose types are their characters, the values' negative. Trimmed.
pub fn custom_property_text(tokens: &[CssToken]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        if i > 0 && tokens[i - 1].ty < 0 && t.ty < 0 {
            out.push(b' ');
        }
        out.extend_from_slice(&t.get_repr(false));
    }
    trim(&out)
}

/// `computed_touch_action`: the `touch-action` an element declares; "auto" when it declares none.
pub fn computed_touch_action(el: &Element) -> Vec<u8> {
    raw(el.is_tag.then(|| el.raw_declaration("touch-action")), b"auto")
}

/// The first of a chain's declared values that is not `inherit` or `unset`, `initial` as "auto"; "auto"
/// when none: `computed_pointer_events` and the `quotes` computed_style writes.
fn inherited(chain: impl Iterator<Item = Vec<u8>>) -> Option<Vec<u8>> {
    for v in chain {
        if !v.is_empty() && v != b"inherit" && v != b"unset" {
            return Some(if v == b"initial" { b"auto".to_vec() } else { v });
        }
    }
    None
}

/// `computed_pointer_events`: the `pointer-events` an element has, inherited from its ancestors; "auto"
/// when none declares one.
pub fn computed_pointer_events(el: &Element) -> Vec<u8> {
    inherited(el.chain.iter().map(|(tag, events, _)| raw(tag.then_some(&events[..]), b""))).unwrap_or_else(|| b"auto".to_vec())
}

fn text(v: &[u8]) -> String {
    String::from_utf8_lossy(v).into_owned()
}

/// Every property a widget may read, by its CSS name. `pseudo` is "::before" or "::after" (or the one-colon
/// forms), or empty for the element itself; `animated` the element's animated values (property, value), a
/// later one winning.
pub fn computed_style(element: &Element, pseudo: &[u8], animated: Option<&[(Vec<u8>, Vec<u8>)]>) -> Json {
    let mut o = Json::object();
    let el = if pseudo.is_empty() {
        Some(element)
    } else {
        match pseudo_kind(pseudo) {
            Some(before) if element.is_tag => {
                if before {
                    element.before.as_deref()
                } else {
                    element.after.as_deref()
                }
            }
            _ => None,
        }
    };
    let Some(el) = el else {
        // A pseudo-element nothing generates: Chromium answers its initial values.
        if !pseudo.is_empty() {
            o.set("content", "none").set("display", "inline");
        }
        return o;
    };
    let font_size = f64::from(el.font_size);
    let len = |l: &Length, predefined: &str| length(l, font_size, predefined);
    let tag_raw = |name: &str, fallback: &[u8]| raw(el.is_tag.then(|| el.raw_declaration(name)), fallback);

    let mut display = keyword(&style_display_strings, el.display);
    if display == "inline-text" {
        display = "inline".to_string();
    }
    o.set("display", display);
    o.set("position", keyword(&element_position_strings, el.position));
    o.set("float", keyword(&element_float_strings, el.float));
    o.set("clear", keyword(&element_clear_strings, el.clear));
    o.set("box-sizing", keyword(&box_sizing_strings, el.box_sizing));
    o.set("visibility", keyword(&visibility_strings, el.visibility));
    let z = if el.position == ElementPosition::element_position_static.index() || el.z_index == 0 {
        "auto".to_string()
    } else {
        el.z_index.to_string()
    };
    o.set("z-index", z);
    o.set("direction", keyword(&direction_strings, el.direction)).set("unicode-bidi", "normal");

    // The box: what layout used, as getComputedStyle resolves width, height, margins and padding.
    let mut margin: [String; 4] = Default::default();
    let mut padding: [String; 4] = Default::default();
    let mut border_width: [String; 4] = Default::default();
    let sides = &el.borders;
    if let Some(ri) = &el.layout {
        let mut ms = ri.margins.map(f64::from);
        // Layout collapses adjoining margins to 0; getComputedStyle keeps the element's own. Auto and percentage
        // margins are only known from layout.
        for i in 0..4 {
            let dm = &el.margins[i];
            if !dm.predefined && dm.units != CssUnits::css_units_percentage {
                ms[i] = stod(&length(dm, font_size, "0px"));
            }
        }
        let ps = ri.paddings.map(f64::from);
        let bs = ri.borders.map(f64::from);
        for i in 0..4 {
            margin[i] = px(ms[i]);
            padding[i] = px(ps[i]);
            border_width[i] = px(bs[i]);
        }
        let mut w = f64::from(el.placement.0);
        let mut h = f64::from(el.placement.1);
        if el.box_sizing == BoxSizing::box_sizing_border_box.index() {
            w += ps[1] + ps[3] + bs[1] + bs[3];
            h += ps[0] + ps[2] + bs[0] + bs[2];
        }
        let inline_box = el.display == StyleDisplay::display_inline.index() || el.display == StyleDisplay::display_inline_text.index();
        o.set("width", if inline_box { "auto".to_string() } else { px(w) })
            .set("height", if inline_box { "auto".to_string() } else { px(h) });
    } else {
        for i in 0..4 {
            margin[i] = len(&el.margins[i], "0px");
            padding[i] = len(&el.padding[i], "0px");
            let none =
                sides[i].style == BorderStyle::border_style_none.index() || sides[i].style == BorderStyle::border_style_hidden.index();
            border_width[i] = if none { "0px".to_string() } else { len(&sides[i].width, "3px") };
        }
        o.set("width", len(&el.width, "auto")).set("height", len(&el.height, "auto"));
    }
    let edges = ["top", "right", "bottom", "left"];
    for i in 0..4 {
        o.set(format!("margin-{}", edges[i]), margin[i].clone());
        o.set(format!("padding-{}", edges[i]), padding[i].clone());
        o.set(format!("border-{}-width", edges[i]), border_width[i].clone());
        o.set(format!("border-{}-style", edges[i]), keyword(&border_style_strings, sides[i].style));
        o.set(format!("border-{}-color", edges[i]), color(&sides[i].color));
    }
    o.set("margin", box_shorthand(&margin[0], &margin[1], &margin[2], &margin[3]));
    o.set("padding", box_shorthand(&padding[0], &padding[1], &padding[2], &padding[3]));
    o.set("border-width", box_shorthand(&border_width[0], &border_width[1], &border_width[2], &border_width[3]));
    let styles: [String; 4] = std::array::from_fn(|i| keyword(&border_style_strings, sides[i].style));
    let colors: [String; 4] = std::array::from_fn(|i| color(&sides[i].color));
    o.set("border-style", box_shorthand(&styles[0], &styles[1], &styles[2], &styles[3]));
    o.set("border-color", box_shorthand(&colors[0], &colors[1], &colors[2], &colors[3]));
    let uniform = border_width[0] == border_width[1]
        && border_width[0] == border_width[2]
        && border_width[0] == border_width[3]
        && styles[0] == styles[1]
        && styles[0] == styles[2]
        && styles[0] == styles[3]
        && colors[0] == colors[1]
        && colors[0] == colors[2]
        && colors[0] == colors[3];
    o.set("border", if uniform { format!("{} {} {}", border_width[0], styles[0], colors[0]) } else { String::new() });
    let radius = |x: &Length, y: &Length| {
        let a = len(x, "0px");
        let b = len(y, "0px");
        if a == b { a } else { format!("{a} {b}") }
    };
    let r = &el.radius;
    let radii = [radius(&r[0], &r[1]), radius(&r[2], &r[3]), radius(&r[4], &r[5]), radius(&r[6], &r[7])];
    o.set("border-top-left-radius", radii[0].clone())
        .set("border-top-right-radius", radii[1].clone())
        .set("border-bottom-right-radius", radii[2].clone())
        .set("border-bottom-left-radius", radii[3].clone());
    o.set("border-radius", box_shorthand(&radii[0], &radii[1], &radii[2], &radii[3]));
    o.set("border-collapse", keyword(&border_collapse_strings, el.border_collapse));
    o.set("border-spacing", len(&el.border_spacing_x, "0px") + " " + &len(&el.border_spacing_y, "0px"));
    o.set("caption-side", keyword(&caption_side_strings, el.caption_side));

    o.set("min-width", len(&el.min_width, "auto")).set("min-height", len(&el.min_height, "auto"));
    o.set("max-width", len(&el.max_width, "none")).set("max-height", len(&el.max_height, "none"));
    let off = &el.offsets;
    o.set("top", len(&off[0], "auto"))
        .set("right", len(&off[1], "auto"))
        .set("bottom", len(&off[2], "auto"))
        .set("left", len(&off[3], "auto"));

    let ox = keyword(&overflow_strings, el.overflow_x);
    let oy = keyword(&overflow_strings, el.overflow_y);
    let overflow = if ox == oy { ox.clone() } else { format!("{ox} {oy}") };
    o.set("overflow-x", ox).set("overflow-y", oy).set("overflow", overflow);
    o.set("white-space", keyword(&white_space_strings, el.white_space));
    let wrap = keyword(&overflow_wrap_strings, el.overflow_wrap);
    o.set("overflow-wrap", wrap.clone()).set("word-wrap", wrap);
    o.set("word-break", keyword(&word_break_strings, el.word_break));

    // Text and fonts.
    o.set("color", color(&el.color));
    o.set("font-size", px(font_size));
    let weight = cxx_float_to_int(el.font_weight);
    o.set("font-weight", if weight > 0 { weight } else { 400 }.to_string());
    let style = keyword(&font_style_strings, el.font_style);
    o.set("font-style", style.clone());
    o.set("font-family", el.font_family.clone());
    let line_height = if el.line_height_predefined { "normal".to_string() } else { px(f64::from(el.line_height)) };
    o.set("line-height", line_height.clone());
    let mut font = Vec::new();
    if style != "normal" {
        font.extend_from_slice(format!("{style} ").as_bytes());
    }
    if !(weight == 400 || weight <= 0) {
        font.extend_from_slice(format!("{weight} ").as_bytes());
    }
    font.extend_from_slice(px(font_size).as_bytes());
    if line_height != "normal" {
        font.extend_from_slice(format!(" / {line_height}").as_bytes());
    }
    font.push(b' ');
    font.extend_from_slice(&el.font_family);
    o.set("font", font);
    o.set("font-variant-numeric", tag_raw("font-variant-numeric", b"normal"));
    o.set("text-align", keyword(&text_align_strings, el.text_align));
    o.set("text-indent", len(&el.text_indent, "0px"));
    o.set("text-transform", keyword(&text_transform_strings, el.text_transform));
    o.set("letter-spacing", if f64::from(el.letter_spacing) == 0.0 { "normal".to_string() } else { px(f64::from(el.letter_spacing)) });
    o.set("word-spacing", px(f64::from(el.word_spacing)));
    o.set("vertical-align", keyword(&vertical_align_strings, el.vertical_align));
    let decoration = text_decoration_line(el.text_decoration_line);
    let decoration_style = keyword(&style_text_decoration_style_strings, el.text_decoration_style);
    let decoration_color = color(if el.text_decoration_color.alpha != 0 { &el.text_decoration_color } else { &el.color });
    o.set("text-decoration-line", decoration)
        .set("text-decoration-style", decoration_style.clone())
        .set("text-decoration-color", decoration_color.clone());
    o.set("text-decoration", format!("{decoration} {decoration_style} {decoration_color}"));
    // What the widget asked for. The renderer selects no text yet, an exception v0.1.1 allows a native renderer.
    let select = if el.user_select == UserSelect::user_select_text.index() {
        "text"
    } else if el.user_select == UserSelect::user_select_none.index() {
        "none"
    } else {
        "auto"
    };
    o.set("user-select", select).set("-webkit-user-select", select);

    // Lists.
    o.set("list-style-type", keyword(&list_style_type_strings, el.list_style_type));
    let quotes =
        inherited(el.chain.iter().map(|(tag, _, quotes)| raw(tag.then_some(&quotes[..]), b""))).unwrap_or_else(|| b"auto".to_vec());
    o.set("quotes", quotes);
    o.set("list-style-position", keyword(&list_style_position_strings, el.list_style_position));
    o.set(
        "list-style-image",
        if el.list_style_image.is_empty() { "none".to_string() } else { format!("url(\"{}\")", text(&el.list_style_image)) },
    );

    // Backgrounds.
    o.set("background-color", color(&el.bg_color));
    let mut images = Vec::new();
    let mut sizes = String::new();
    let mut positions = String::new();
    let mut repeats = String::new();
    for (i, image) in el.bg_images.iter().enumerate() {
        let sep = if i > 0 { ", " } else { "" };
        let one = match image.kind {
            1 => [b"url(\"".as_slice(), &image.url, b"\")"].concat(),
            2 => [image.gradient.as_slice(), b"(...)"].concat(),
            _ => b"none".to_vec(),
        };
        images.extend_from_slice(sep.as_bytes());
        images.extend_from_slice(&one);
        if let Some((w_len, h_len)) = el.bg_sizes.get(i) {
            let w = len(w_len, "auto");
            let h = len(h_len, "auto");
            let size = if w_len.predefined && w_len.predef != 0 {
                keyword(&background_size_strings, w_len.predef)
            } else if h == "auto" && w == "auto" {
                w
            } else {
                format!("{w} {h}")
            };
            sizes += &(sep.to_string() + &size);
        }
        if i < el.bg_position_x.len() && i < el.bg_position_y.len() {
            positions += &format!("{sep}{} {}", len(&el.bg_position_x[i], "0%"), len(&el.bg_position_y[i], "0%"));
        }
        if let Some(repeat) = el.bg_repeat.get(i) {
            repeats += &(sep.to_string() + &keyword(&background_repeat_strings, *repeat));
        }
    }
    o.set("background-image", if images.is_empty() { b"none".to_vec() } else { images });
    o.set("background-size", if sizes.is_empty() { "auto".to_string() } else { sizes });
    o.set("background-position", if positions.is_empty() { "0% 0%".to_string() } else { positions });
    o.set("background-repeat", if repeats.is_empty() { "repeat".to_string() } else { repeats });

    // Effects.
    o.set("opacity", format_number(f64::from(el.opacity), 3));
    o.set("cursor", if el.cursor.is_empty() { b"auto".to_vec() } else { el.cursor.clone() });
    o.set("pointer-events", computed_pointer_events(el));
    let declared = |name: &str, fallback: &[u8]| -> Vec<u8> {
        if let Some(animated) = animated
            && pseudo.is_empty()
            && let Some((_, v)) = animated.iter().rev().find(|(n, _)| n == name.as_bytes())
        {
            return trim(v);
        }
        tag_raw(name, fallback)
    };
    o.set("touch-action", tag_raw("touch-action", b"auto"));
    o.set("filter", declared("filter", b"none")).set("backdrop-filter", declared("backdrop-filter", b"none"));
    o.set("box-shadow", declared("box-shadow", b"none"));
    o.set("translate", declared("translate", b"none")).set("rotate", declared("rotate", b"none")).set("scale", declared("scale", b"none"));
    {
        // Chromium resolves a transform to the matrix it draws with, about the border box.
        let mut bw = f64::from(el.placement.0);
        let mut bh = f64::from(el.placement.1);
        if let Some(ri) = &el.layout {
            bw += f64::from(ri.paddings[3]) + f64::from(ri.paddings[1]) + f64::from(ri.borders[3]) + f64::from(ri.borders[1]);
            bh += f64::from(ri.paddings[0]) + f64::from(ri.paddings[2]) + f64::from(ri.borders[0]) + f64::from(ri.borders[2]);
        }
        let mut transform = declared("transform", b"none");
        if transform != b"none"
            && let Some(m) = parse_transform(&transform, bw as f32, bh as f32)
        {
            let f = |v: f32| format_number(f64::from(v), 6);
            transform = format!("matrix({}, {}, {}, {}, {}, {})", f(m.a), f(m.b), f(m.c), f(m.d), f(m.e), f(m.f)).into_bytes();
        }
        o.set("transform", transform);
        let origin = |l: &Length, size: f64| {
            if l.units == CssUnits::css_units_percentage { px(f64::from(l.val) * size / 100.0) } else { len(l, "0px") }
        };
        o.set("transform-origin", origin(&el.transform_origin_x, bw) + " " + &origin(&el.transform_origin_y, bh));
    }
    let outline = &el.outline;
    let outlined = outline.style != BorderStyle::border_style_none.index() && outline.style != BorderStyle::border_style_hidden.index();
    o.set("outline-style", keyword(&border_style_strings, outline.style));
    o.set("outline-width", if outlined { len(&outline.width, "3px") } else { "0px".to_string() });
    o.set("outline-color", color(if outline.color.alpha != 0 { &outline.color } else { &el.color }));
    o.set("outline-offset", px(f64::from(el.outline_offset)));
    for fill in [true, false] {
        // A paint server stays as written. Otherwise the paint is the one litehtml draws with: inherited (through a
        // <use> to what it draws, too), currentColor resolved.
        let name = if fill { "fill" } else { "stroke" };
        let value = declared(name, b"");
        let painted = if fill { &el.fill_paint } else { &el.stroke_paint };
        if value.windows(4).any(|w| w == b"url(") {
            o.set(name, value);
        } else if let Some(c) = parse_color(&value) {
            o.set(name, color(&WebColor::rgba(c.r, c.g, c.b, c.a)));
        } else if let Some(p) = painted {
            o.set(name, color(p));
        } else {
            o.set(name, "none");
        }
    }

    // Flex and grid.
    o.set("flex-direction", keyword(&flex_direction_strings, el.flex_direction));
    o.set("flex-wrap", keyword(&flex_wrap_strings, el.flex_wrap));
    o.set("justify-content", keyword(&flex_justify_content_strings, el.justify_content));
    o.set("align-items", alignment(el.align_items));
    o.set("align-self", alignment(el.align_self));
    o.set("align-content", keyword(&flex_align_content_strings, el.align_content));
    o.set("flex-grow", format_number(f64::from(el.flex_grow), 3)).set("flex-shrink", format_number(f64::from(el.flex_shrink), 3));
    let basis = if el.flex_basis.predefined { keyword(&flex_basis_strings, el.flex_basis.predef) } else { len(&el.flex_basis, "auto") };
    o.set("flex-basis", basis);
    o.set("order", el.order.to_string());
    let row_gap = len(&el.row_gap, "normal");
    let column_gap = len(&el.column_gap, "normal");
    let gap = if row_gap == column_gap { row_gap.clone() } else { format!("{row_gap} {column_gap}") };
    o.set("row-gap", row_gap).set("column-gap", column_gap).set("gap", gap);
    o.set("aspect-ratio", if el.aspect_ratio > 0.0 { format_number(f64::from(el.aspect_ratio), 4) + " / 1" } else { "auto".to_string() });

    // Generated content: a pseudo-element's own text, quoted as Chromium writes it; keywords and functions as
    // written.
    if !pseudo.is_empty() {
        let mut content = trim(&el.content);
        if content.len() >= 2 && (content[0] == b'\'' || content[0] == b'"') && content.last() == content.first() {
            content = [b"\"".as_slice(), &content[1..content.len() - 1], b"\""].concat();
        }
        o.set("content", if content.is_empty() { b"none".to_vec() } else { content });
    } else {
        o.set("content", "normal");
    }
    o
}

/// `std::stod` of what `length()` wrote: its leading number (the text always starts with one).
fn stod(s: &str) -> f64 {
    let end = s.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E')).unwrap_or(s.len());
    s[..end].parse().unwrap_or(0.0)
}

/// `int(float)` as MSVC's x64 code converts it: truncated, and INT_MIN out of range or for NaN (undefined in C++).
fn cxx_float_to_int(v: f32) -> i32 {
    if v.is_nan() || !(-2147483648.0..2147483648.0).contains(&v) { i32::MIN } else { v as i32 }
}
